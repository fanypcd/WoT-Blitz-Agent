//! armorsStates 状态轨道（Vehicle propID 16）：在真实动态装甲样本上锁定行为学定案。
//!
//! 样本 A `20261013_0954…Oth71_Titan_Strife…`：作者驾驶 Oth71（XML 两态：
//! state0 = gun_01.armor_11 + hull.armor_11 / state1 = armor_10 对），armorMover
//! 实战用 1 次 → 轨道 [0, 1]（prop0 同期翻 12 次——反例存档：prop0 与状态无关）。
//! 样本 B `20261013_1615…`：受击车 Oth44_Charioteer_T（state0 = hull.armor_14 /
//! state1 = turret_01.armor_12 + hull.armor_13 + hull.armor_15），armorMover 用 2 次
//! → 轨道 [0, 1, 0]；首弹弹着（t_fire 68.95 + 飞行 0.04）落在 [57.44, 79.95) → state1，
//! 而该弹命中板 armor_group=15 只在 state1 存在，与战报"未击穿"互证——射击复现必须
//! 还原受击方弹着时刻的状态（问题回归锚点）。propID/载荷/值域的定案见
//! `RawArmorStateTrack` 文档。

use std::collections::HashMap;
use std::path::PathBuf;

use wotb_replay_core::facets::ai_review::{collect_armor_state_tracks, RawArmorStateTrack};
use wotb_replay_core::replay::combat::GunPitchLimits;
use wotb_replay_core::replay::packets::read_raw_packets;
use wotb_replay_core::replay::playback::{build_playback_data, PlaybackInput, PlaybackPlayer};

const SAMPLE_A: &str = "20261013_0954__Anonyme_Oth71_Titan_Strife_579709385753595713.wotbreplay";
const SAMPLE_B: &str = "20261013_1615__Anonyme_Oth71_Titan_Strife_3316571242883186.wotbreplay";

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/replay_samples")
        .join(name)
}

fn tracks_of(replay: &str) -> Vec<RawArmorStateTrack> {
    let bytes = std::fs::read(sample(replay)).expect("样本必须在语料中");
    let packets = read_raw_packets(&bytes).expect("分帧必须走满 EOF");
    collect_armor_state_tracks(
        &packets
            .iter()
            .map(|p| (p.packet_type, p.clock_secs, p.payload.as_slice()))
            .collect::<Vec<_>>(),
    )
}

#[test]
fn single_toggle_sample_track() {
    let tracks = tracks_of(SAMPLE_A);
    assert_eq!(tracks.len(), 1, "只有作者车辆实体有状态轨道: {tracks:?}");
    let tr = &tracks[0];
    assert_eq!(tr.eid, 1_35002059, "作者车辆实体");
    assert_eq!(
        tr.state,
        vec![0, 1],
        "create 快照（默认态 0）+ 1 次 armorMover 切换（1）"
    );
    assert_eq!(tr.t.len(), tr.state.len());
    assert!(
        (tr.t[0] - 0.48).abs() < 0.05,
        "首样本 = 入视野 create 快照时刻，实测 {}",
        tr.t[0]
    );
    assert!(
        (tr.t[1] - 18.23).abs() < 0.05,
        "切换时刻，实测 {}",
        tr.t[1]
    );
    assert_eq!(tr.state_at(18.0), Some(0));
    assert_eq!(tr.state_at(18.3), Some(1));
    assert_eq!(tr.state_at(0.4), None, "快照前 = 未知（消费方默认态兜底）");
}

#[test]
fn charioteer_two_toggles_and_shot_state() {
    let tracks = tracks_of(SAMPLE_B);
    assert_eq!(
        tracks.len(),
        1,
        "仅受击车有轨道；作者车全程默认态 → 全零轨道丢弃: {tracks:?}"
    );
    let tr = &tracks[0];
    assert_eq!(tr.eid, 1_594_090, "受击车 Oth44_Charioteer_T");
    assert_eq!(tr.state, vec![0, 1, 0], "create 快照 + 两次 armorMover");
    for (got, want) in tr.t.iter().zip([41.35f32, 57.44, 79.95]) {
        assert!((got - want).abs() < 0.05, "t={got} 应为 {want}");
    }
    assert_eq!(tr.state_at(68.99), Some(1), "首弹弹着（68.95 + 0.04）");
    assert_eq!(tr.state_at(50.0), Some(0));
    assert_eq!(tr.state_at(41.0), None, "入视野前 = 未知");
}

#[test]
fn non_dynamic_replay_has_empty_tracks() {
    // 静态装甲样本（T100LT）：无 prop16 更新、create 表 prop16 空表（n=0）→ 轨道为空
    // （facet 序列化时字段整体省略）
    let tracks = tracks_of("20261010_1212__Anonyme_R132_T100LT_579650759449754426.wotbreplay");
    assert!(tracks.is_empty(), "静态装甲车不应产生状态轨道: {tracks:?}");
}

#[test]
fn playback_data_backfills_shot_impact_state() {
    // 回放切面（PlaybackData，消费方 3D/射击复现的实际入口）必须与切面收集器同源：
    // 字段 additive（v2 不变），静态车省略；逐弹 target_armor_state = 弹着时刻轨道值。
    let bytes = std::fs::read(sample(SAMPLE_B)).expect("样本必须在语料中");
    let raw = read_raw_packets(&bytes).expect("分帧");
    let tuples: Vec<(u32, f32, &[u8])> = raw
        .iter()
        .map(|pk| (pk.packet_type, pk.clock_secs, pk.payload.as_slice()))
        .collect();
    let limits = GunPitchLimits::default();
    let pb = build_playback_data(&PlaybackInput {
        packets: &tuples,
        // 显示车辆以结算花名册为准（样本 B 的双人训练房）
        players: vec![
            PlaybackPlayer {
                account_id: 0,
                nickname: "F0rget_U_F0rever".into(),
                team: 2,
                tank_id: 0,
            },
            PlaybackPlayer {
                account_id: 2033684170,
                nickname: "Anonyme".into(),
                team: 1,
                tank_id: 17777,
            },
        ],
        author_account_id: 2033684170,
        winner_team: 0,
        map_id: 0,
        map_name: String::new(),
        pitch_limits: &limits,
        tank_names: HashMap::new(),
    })
    .expect("PlaybackData 构建必须成功");

    assert_eq!(pb.armor_states.len(), 1, "只有受击车有状态轨道");
    assert_eq!(pb.armor_states[0].eid, 1_594_090);

    assert_eq!(pb.shots.len(), 4, "四发全部打向受击车");
    for s in &pb.shots {
        assert_eq!(s.target_eid, Some(1_594_090));
    }
    assert_eq!(
        pb.shots[0].target_armor_state,
        Some(1),
        "首弹弹着时受击车在 state1（用户报告问题的回归锚点）"
    );
    for s in &pb.shots[1..] {
        assert_eq!(s.target_armor_state, Some(0), "79.95s 切回默认态后各弹 = state0");
    }

    // 序列化形状：非空时字段出现；静态车（空）字段省略
    let json = serde_json::to_string(&pb).unwrap();
    assert!(json.contains("armorStates") || json.contains("armor_states"));
}
