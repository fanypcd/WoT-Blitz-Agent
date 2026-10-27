//! 录像者车辆首条可信世界位姿（轻量独立能力）。
//!
//! 证据边界：
//! - 身份 = ARENA_INFO comp blob 的 account_id 精确匹配（与 ReplayModel::scan 同
//!   SSOT），未命中退回既有 type=5 昵称精确解析；
//! - 位姿 = 该 eid 的 type=10 采样，要求 parent==0（世界系）、位置/时钟有限、
//!   载荷可解码；多条候选取 replay clock 最小者（同钟保持包流首条）；
//! - 输出原样 replay 世界坐标，不做地图坐标/出生侧语义转换。
//!
//! 不构建 ReplayModel/PlaybackData/AiReviewFacet；只做一次定向包扫描。

use std::collections::HashMap;

use serde::Serialize;

use super::combat;
use super::playback::CompDescriptor;

/// type=10 载荷最小长度（位置/姿态/误差盒完整域；尾部 state 字节语义未定，
/// 不参与本能力判定）。与共享索引解码器同一阈值。
const ST10_MIN_PAYLOAD_LEN: usize = 48;

/// 录像者车辆首条可信世界位姿。
///
/// `clock` = 被选中采样的原始 replay clock；`position` = 原始 replay 世界坐标
/// [x, y, z]（y 为高程）。不是地图 spawn 点、不做 WotbTools 坐标系转换。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AuthorInitialPosition {
    pub author_eid: u32,
    pub clock: f32,
    pub position: [f32; 3],
}

/// 作者车辆 EID（与 `ReplayModel::scan` 完全同语义）：
/// comp blob `account_id` 精确匹配优先（重名/匿名免疫）；未命中退回既有
/// type=5 昵称精确匹配。返回 0 = 未解析（调用方 fail-closed）。
pub(crate) fn resolve_author_vehicle_eid(
    comps: &HashMap<String, CompDescriptor>,
    packets: &[(u32, f32, &[u8])],
    author_account_id: u32,
    author_nickname: &str,
) -> u32 {
    comps
        .values()
        .find(|c| c.account_id != 0 && c.account_id == author_account_id as u64)
        .map(|c| c.eid)
        .filter(|e| *e != 0)
        .unwrap_or_else(|| combat::resolve_author_player_eid_by_nick(packets, author_nickname))
}

/// 该 eid 的首条可信世界系 type=10 位姿。
///
/// 逐包筛选：type==10、eid 匹配、payload ≥48B、parent==0、位置三轴与 clock 有限。
/// 返回 replay clock 最小的样本（同钟保持包流首条）；无候选 → None。
pub(crate) fn first_world_position(
    packets: &[(u32, f32, &[u8])],
    author_eid: u32,
) -> Option<AuthorInitialPosition> {
    if author_eid == 0 {
        return None;
    }
    let mut best: Option<(f32, [f32; 3])> = None;
    for (kind, clock, payload) in packets {
        if *kind != 10 || payload.len() < ST10_MIN_PAYLOAD_LEN {
            continue;
        }
        let eid = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        if eid != author_eid {
            continue;
        }
        // offset 8..12 = attachment/parent entity ID；非 0 表示挂接实体，
        // 其 pos 不是直接世界坐标（WotbTools PROVEN behavioral role）。
        let parent = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
        if parent != 0 {
            continue;
        }
        let Some(sample) = combat::decode_st10_sample(*clock, payload) else {
            continue;
        };
        if !sample.clock.is_finite() || !sample.pos.iter().all(|v| v.is_finite()) {
            continue;
        }
        if best.is_none_or(|(t, _)| sample.clock < t) {
            best = Some((sample.clock, sample.pos));
        }
    }
    best.map(|(clock, position)| AuthorInitialPosition {
        author_eid,
        clock,
        position,
    })
}

/// 从完整 .wotbreplay 字节构建轻量能力结果。
///
/// 只读容器 meta + battle_results 与包流分帧；**不**调用 ReplayParser /
/// ReplayModel::scan / PlaybackData 构建。battle_results 缺失时仍尝试 meta 昵称
/// 回退（截断/未结算回放只要已有作者世界位姿仍可返回）；两面证据都没有 → None。
pub fn from_replay_bytes(bytes: &[u8]) -> anyhow::Result<Option<AuthorInitialPosition>> {
    let mut replay = wotbreplay_parser::replay::Replay::open(std::io::Cursor::new(bytes))?;
    let meta = replay.read_meta().ok();
    let battle_results = replay.read_battle_results().ok();

    let author_account_id = battle_results
        .as_ref()
        .map(|br| br.author.account_id)
        .unwrap_or(0);
    // 与 ReplayParser 同序：meta.player_name 优先，缺失时回退结算花名册昵称。
    // 不按文件名/军团拼接格式猜作者身份。
    let author_nickname = meta
        .as_ref()
        .map(|m| m.player_name.clone())
        .or_else(|| {
            battle_results
                .as_ref()
                .map(combat::author_nick_from_battle_results)
        })
        .unwrap_or_default();
    let valid_tanks: Vec<u32> = battle_results
        .as_ref()
        .map(|br| br.player_results.iter().map(|pr| pr.info.tank_id).collect())
        .unwrap_or_default();

    let raw = super::packets::read_raw_packets(bytes)?;
    let packets: Vec<(u32, f32, &[u8])> = raw
        .iter()
        .map(|p| (p.packet_type, p.clock_secs, p.payload.as_slice()))
        .collect();

    // account_id 路径才需要 comps（ARENA_INFO subtype=1）；无结算账号时直接走昵称回退。
    let comps = if author_account_id != 0 {
        let arena_updates = combat::collect_arena_updates_filtered(&packets, |s| s == 1);
        super::playback::comp_descriptors_from_updates(&arena_updates, &valid_tanks)
    } else {
        Default::default()
    };

    Ok(extract(
        &packets,
        &comps,
        author_account_id,
        &author_nickname,
    ))
}

/// 便捷入口：身份解析 + 首条世界位姿。
fn extract(
    packets: &[(u32, f32, &[u8])],
    comps: &HashMap<String, CompDescriptor>,
    author_account_id: u32,
    author_nickname: &str,
) -> Option<AuthorInitialPosition> {
    let eid = resolve_author_vehicle_eid(comps, packets, author_account_id, author_nickname);
    first_world_position(packets, eid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type10(eid: u32, parent: u32, clock: f32, pos: [f32; 3]) -> (u32, f32, Vec<u8>) {
        let mut p = vec![0u8; ST10_MIN_PAYLOAD_LEN];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[8..12].copy_from_slice(&parent.to_le_bytes());
        for (i, v) in pos.iter().enumerate() {
            p[12 + i * 4..16 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        (10, clock, p)
    }

    fn type5(eid: u32, nickname: &str) -> (u32, f32, Vec<u8>) {
        let mut p = vec![0u8; 60.max(58 + nickname.len())];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[4..6].copy_from_slice(&2u16.to_le_bytes());
        p[57] = nickname.len() as u8;
        p[58..58 + nickname.len()].copy_from_slice(nickname.as_bytes());
        (5, 0.0, p)
    }

    fn refs(rows: &[(u32, f32, Vec<u8>)]) -> Vec<(u32, f32, &[u8])> {
        rows.iter()
            .map(|(t, c, p)| (*t, *c, p.as_slice()))
            .collect()
    }

    fn comp(eid: u32, account_id: u64, nickname: &str) -> CompDescriptor {
        CompDescriptor {
            nickname: nickname.to_string(),
            eid,
            account_id,
            tank_id: 1,
            turret_local: 1,
            gun_local: 1,
        }
    }

    #[test]
    fn selects_author_eid_and_earliest_author_pose_not_first_vehicle() {
        let rows = vec![
            type10(200, 0, 0.8, [9.0, 9.0, 9.0]),
            type10(100, 0, 1.0, [1.0, 2.0, 3.0]),
            type10(100, 0, 1.1, [4.0, 5.0, 6.0]),
        ];
        let mut comps = HashMap::new();
        comps.insert("author".into(), comp(100, 42, "author"));
        let got = extract(&refs(&rows), &comps, 42, "author").expect("作者位姿应解析");
        assert_eq!(got.author_eid, 100);
        assert_eq!(got.clock, 1.0);
        assert_eq!(got.position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn picks_earliest_clock_even_when_stream_order_is_late() {
        let rows = vec![
            type10(100, 0, 1.2, [12.0, 0.0, 0.0]),
            type10(100, 0, 1.0, [10.0, 0.0, 0.0]),
        ];
        let got = first_world_position(&refs(&rows), 100).expect("应有候选");
        assert_eq!(got.clock, 1.0);
        assert_eq!(got.position[0], 10.0);
    }

    #[test]
    fn skips_parent_attached_nonfinite_and_short_samples() {
        let mut short = vec![0u8; 20];
        short[0..4].copy_from_slice(&100u32.to_le_bytes());
        let rows = vec![
            type10(100, 55, 0.5, [50.0, 0.0, 0.0]), // parent != 0：局部/挂接坐标
            type10(100, 0, 0.6, [f32::NAN, 0.0, 0.0]),
            type10(100, 0, 0.7, [f32::INFINITY, 0.0, 0.0]),
            (10, 0.8, short),
            type10(100, 0, 1.0, [1.0, 2.0, 3.0]),
        ];
        let got = first_world_position(&refs(&rows), 100).expect("应跳过无效样本");
        assert_eq!(got.clock, 1.0);
        assert_eq!(got.position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn skips_nonfinite_clock() {
        let rows = vec![
            type10(100, 0, f32::NAN, [1.0, 2.0, 3.0]),
            type10(100, 0, f32::INFINITY, [4.0, 5.0, 6.0]),
            type10(100, 0, 1.0, [7.0, 8.0, 9.0]),
        ];
        let got = first_world_position(&refs(&rows), 100).expect("应跳过坏时钟");
        assert_eq!(got.clock, 1.0);
        assert_eq!(got.position, [7.0, 8.0, 9.0]);
    }

    #[test]
    fn missing_author_or_no_pose_is_unavailable() {
        let rows = vec![type10(200, 0, 1.0, [1.0, 2.0, 3.0])];
        assert_eq!(first_world_position(&refs(&rows), 0), None);
        assert_eq!(first_world_position(&refs(&rows), 100), None);
        let comps = HashMap::new();
        assert_eq!(extract(&refs(&rows), &comps, 0, ""), None);
    }

    #[test]
    fn account_id_identity_beats_nickname_and_falls_back_to_nickname() {
        let rows = vec![
            type5(200, "author"),
            type10(100, 0, 1.0, [1.0, 2.0, 3.0]),
            type10(200, 0, 0.5, [9.0, 9.0, 9.0]),
        ];
        let mut comps = HashMap::new();
        comps.insert("author".into(), comp(100, 42, "author"));
        // account_id 精确命中 → 100（不是昵称命中的 200）
        assert_eq!(
            resolve_author_vehicle_eid(&comps, &refs(&rows), 42, "author"),
            100
        );
        // comp eid 未识别 → 退回既有昵称解析 → 200（该 type5 的首个匹配）
        comps.insert("author".into(), comp(0, 42, "author"));
        assert_eq!(
            resolve_author_vehicle_eid(&comps, &refs(&rows), 42, "author"),
            200
        );
        // 无 comps、账号未命中 → 昵称解析
        assert_eq!(
            resolve_author_vehicle_eid(&HashMap::new(), &refs(&rows), 42, "author"),
            200
        );
        // 昵称为空且无账号证据 → 0
        assert_eq!(
            resolve_author_vehicle_eid(&HashMap::new(), &refs(&rows), 42, ""),
            0
        );
    }

    #[test]
    fn unavailable_serializes_as_json_null() {
        let none: Option<AuthorInitialPosition> = None;
        assert_eq!(serde_json::to_string(&none).unwrap(), "null");
    }

    #[test]
    fn serialized_shape_is_stable() {
        let v = serde_json::to_value(AuthorInitialPosition {
            author_eid: 100,
            clock: 1.0,
            position: [1.0, 2.0, 3.0],
        })
        .unwrap();
        assert_eq!(v["author_eid"], 100);
        assert_eq!(v["clock"], 1.0);
        assert_eq!(v["position"], serde_json::json!([1.0, 2.0, 3.0]));
        assert_eq!(v.as_object().unwrap().len(), 3, "公开面只有三个字段");
    }
}
