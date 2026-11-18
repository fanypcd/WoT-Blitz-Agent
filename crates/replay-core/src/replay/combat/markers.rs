//! 战场快捷标记：Avatar method 0x2e（`onBattleMarker` 族）。
//!
//! 证据（2026-10-24 BZT-70 场 + 15 场语料）：
//! - args = `[entity_id u32][account_id u32][0 u32][len u8][protobuf]`；
//!   `len` 恰好吃满其余字节（15/15 一致），第三个 u32 在全部样本中为 0，作为形状门禁。
//! - protobuf 顶层恰一个字段，字段号 = 标记类型，wire type = 2（嵌套消息）：
//!   - type 1（moving_here）：嵌套 `field1 { field1 x f32, field2 y f32, field3 z f32 }`
//!     ——世界坐标；语料 5 条。
//!   - type 4（attack）：嵌套 `field2 targetEntityId varint`；语料 5 条。
//!   - type 5（ally_retreat）：嵌套 `field2 targetEntityId varint`；语料 2 条。
//! - 语义名与客户端快捷指令表同域（`chatCommand/moving_here` / `chatCommand/attack` /
//!   `chatCommand/ally_retreat`，blitzinfo `yt` 分派与 BZT-70 战斗日志逐条对应：
//!   t=47.72 type1 → “Moving here”，t=71.83 type5 → “Let's fall back!”）。
//! - 未观测类型（如 6/9/10/15/18/26）没有形状证据 → 原样保留 `marker_type`，
//!   `at`/`target_eid` 缺省；消费方不得命名（fail-closed，禁猜）。
//! - 形状不符（长度字段不匹配 / 第三个 u32 非零 / 顶层字段不是单一 wire2 /
//!   已知类型的嵌套坐标或目标缺失）→ 整包跳过，不产出半条事件。

use serde::Serialize;

use super::pb::{pb_varint, proto_fields};

/// Avatar method 0x2e：战场快捷标记。
const AVATAR_BATTLE_MARKER: u32 = 0x2E;
/// type=8 实体方法包（与 chat.rs 同值；replay/packets.rs 线格式）。
const METHOD_PACKET: u32 = 8;

/// 一条战场标记事件。`marker_type` 的语义名只在 1/4/5 有证据（见模块头注）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BattleMarkerEvent {
    /// 回放时钟（秒，由收集器填入；投影层 r2 舍入）
    pub t: f32,
    /// 发送者实体 id（Vehicle/Player 实体；与名册 `vehicles[].eid` 同域）
    pub entity_id: u32,
    /// 发送者账号 id（与名册 `vehicles[].account_id` 同域）
    pub account_id: u32,
    /// 标记类型（protobuf 顶层字段号；1/4/5 语义已证，其余原样透传）
    pub marker_type: u32,
    /// type=1：标记世界坐标 [x,y,z]（米）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f32; 3]>,
    /// type=4/5：目标车辆实体 id（与名册 `vehicles[].eid` 同域）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_eid: Option<u32>,
}

/// 0x2e 标记包收集（时钟升序；形状门禁不过的包跳过）。
pub fn collect_battle_markers(packets: &[(u32, f32, &[u8])]) -> Vec<BattleMarkerEvent> {
    let mut out = Vec::new();
    for (kind, clock, payload) in packets {
        if *kind != METHOD_PACKET || payload.len() < 12 {
            continue;
        }
        let method = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
        if method != AVATAR_BATTLE_MARKER {
            continue;
        }
        let alen = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]) as usize;
        if 12usize.checked_add(alen) != Some(payload.len()) {
            continue;
        }
        let Some(ev) = parse_marker_args(*clock, &payload[12..]) else {
            continue;
        };
        out.push(ev);
    }
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// args 形状：`[entity_id u32][account_id u32][0 u32][len u8][protobuf]`。
/// 注意：发送者身份取 **args 内 entity_id**（与名册 eid 同域）；信封头里的 Avatar
/// eid 是本机 Avatar 实体（BZT-70 场 136067526），不是标记发送者（args=136106019）。
fn parse_marker_args(clock: f32, args: &[u8]) -> Option<BattleMarkerEvent> {
    if args.len() < 13 {
        return None;
    }
    let entity_id = u32::from_le_bytes([args[0], args[1], args[2], args[3]]);
    let account_id = u32::from_le_bytes([args[4], args[5], args[6], args[7]]);
    let zero = u32::from_le_bytes([args[8], args[9], args[10], args[11]]);
    if zero != 0 {
        return None; // 观测形状之外：布局可能不同，整包跳过
    }
    let body_len = args[12] as usize;
    let body = args.get(13..)?;
    if body.len() != body_len {
        return None;
    }
    // 顶层恰一个字段；字段号 = 标记类型，wire type 必须为 2。
    let outer = proto_fields(body)?;
    if outer.len() != 1 || outer[0].1 != 2 {
        return None;
    }
    let (marker_type, _, start, len) = outer[0];
    if marker_type == 0 {
        return None;
    }
    let msg = body.get(start..start.checked_add(len)?)?;
    match marker_type {
        1 => {
            let at = parse_position(msg)?;
            Some(BattleMarkerEvent {
                t: clock,
                entity_id,
                account_id,
                marker_type,
                at: Some(at),
                target_eid: None,
            })
        }
        4 | 5 => {
            let target = parse_target(msg)?;
            Some(BattleMarkerEvent {
                t: clock,
                entity_id,
                account_id,
                marker_type,
                at: None,
                target_eid: Some(target),
            })
        }
        // 未观测类型：只保留类型本身，不解释嵌套消息（禁猜）。
        _ => Some(BattleMarkerEvent {
            t: clock,
            entity_id,
            account_id,
            marker_type,
            at: None,
            target_eid: None,
        }),
    }
}

/// type=1 嵌套：`field1 { field1 x f32, field2 y f32, field3 z f32 }`。
fn parse_position(msg: &[u8]) -> Option<[f32; 3]> {
    let pos_msg = single_message_field(msg, 1)?;
    let fields = proto_fields(pos_msg)?;
    if fields.len() != 3 {
        return None;
    }
    let mut xyz = [0f32; 3];
    for (want, slot) in xyz.iter_mut().enumerate() {
        let (no, wt, start, len) = fields[want];
        if no != want as u32 + 1 || wt != 5 || len != 4 {
            return None;
        }
        let raw = pos_msg.get(start..start + 4)?;
        *slot = f32::from_le_bytes(raw.try_into().ok()?);
    }
    Some(xyz)
}

/// type=4/5 嵌套：恰一个 `field2 targetEntityId` varint。
fn parse_target(msg: &[u8]) -> Option<u32> {
    let fields = proto_fields(msg)?;
    if fields.len() != 1 {
        return None;
    }
    let (no, wt, start, len) = fields[0];
    if no != 2 || wt != 0 {
        return None;
    }
    let mut i = start;
    let value = pb_varint(msg, &mut i)?;
    if i != start.checked_add(len)? {
        return None;
    }
    u32::try_from(value).ok().filter(|v| *v != 0)
}

/// 顶层恰一个 `want` 号 wire2 字段的嵌套消息。
fn single_message_field(msg: &[u8], want: u32) -> Option<&[u8]> {
    let fields = proto_fields(msg)?;
    if fields.len() != 1 {
        return None;
    }
    let (no, wt, start, len) = fields[0];
    if no != want || wt != 2 {
        return None;
    }
    msg.get(start..start.checked_add(len)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn packet(eid: u32, args: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&eid.to_le_bytes());
        p.extend_from_slice(&AVATAR_BATTLE_MARKER.to_le_bytes());
        p.extend_from_slice(&(args.len() as u32).to_le_bytes());
        p.extend_from_slice(args);
        p
    }

    const BZT_MOVING: &str = "23d01c08ca8e377900000000130a110a0f0ddc123043157ee679421da4e56f42";
    const BZT_RETREAT: &str = "23d01c08ca8e377900000000072a0510a4a0f340";
    const ATTACK: &str = "9d4023086551377a000000000722051097818d41";

    #[test]
    fn moving_marker_parses_world_position() {
        let args = hex(BZT_MOVING);
        let raw = packet(136067526, &args);
        let evs = collect_battle_markers(&[(METHOD_PACKET, 47.72, raw.as_slice())]);
        assert_eq!(evs.len(), 1);
        let ev = &evs[0];
        assert_eq!(ev.entity_id, 136106019);
        assert_eq!(ev.account_id, 2033684170);
        assert_eq!(ev.marker_type, 1);
        assert_eq!(ev.target_eid, None);
        let at = ev.at.expect("type1 carries position");
        assert!((at[0] - 176.07367).abs() < 1e-4);
        assert!((at[1] - 62.47509).abs() < 1e-4);
        assert!((at[2] - 59.97426).abs() < 1e-4);
    }

    #[test]
    fn retreat_marker_parses_target_entity() {
        let args = hex(BZT_RETREAT);
        let raw = packet(136067526, &args);
        let evs = collect_battle_markers(&[(METHOD_PACKET, 71.833, raw.as_slice())]);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].marker_type, 5);
        assert_eq!(evs[0].target_eid, Some(136106020));
        assert_eq!(evs[0].at, None);
    }

    #[test]
    fn attack_marker_parses_target_entity() {
        let args = hex(ATTACK);
        let raw = packet(136528029, &args);
        let evs = collect_battle_markers(&[(METHOD_PACKET, 26.9, raw.as_slice())]);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].marker_type, 4);
        assert_eq!(evs[0].target_eid, Some(136528023));
    }

    #[test]
    fn unknown_marker_type_is_preserved_without_payload_guess() {
        // 顶层 field6 wire2，内容 1 字节：类型未知，只保留 marker_type。
        let args = hex("01020304050607080000000003320100");
        let raw = packet(7, &args);
        let evs = collect_battle_markers(&[(METHOD_PACKET, 1.0, raw.as_slice())]);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].marker_type, 6);
        assert_eq!(evs[0].at, None);
        assert_eq!(evs[0].target_eid, None);
    }

    #[test]
    fn malformed_marker_shapes_are_dropped() {
        let args = hex(BZT_MOVING);
        // 第三个 u32 非零（布局不再匹配观测形状）
        let mut bad_zero = args.clone();
        bad_zero[8] = 1;
        // payload 长度字段与剩余字节不一致
        let mut bad_len = args.clone();
        bad_len[12] = 0x12;
        // 顶层字段不是 wire2
        let bad_wire = hex("010203040506070800000000020801");
        for a in [bad_zero, bad_len, bad_wire] {
            let raw = packet(1, &a);
            assert!(collect_battle_markers(&[(METHOD_PACKET, 1.0, raw.as_slice())]).is_empty());
        }
        // 方法号不是 0x2e
        let mut other = packet(1, &args);
        other[4] = 0x2b;
        assert!(collect_battle_markers(&[(METHOD_PACKET, 1.0, other.as_slice())]).is_empty());
        // 非实体方法包
        let raw = packet(1, &args);
        assert!(collect_battle_markers(&[(0x27, 1.0, raw.as_slice())]).is_empty());
    }
}
