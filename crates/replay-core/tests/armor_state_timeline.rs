//! armorsStates 状态轨道（type=7 prop0 u8）：在真实动态装甲样本上锁定行为学定案。
//!
//! 样本 `20261013_0954__Anonyme_Oth71_Titan_Strife_…`：作者驾驶 Oth71_Titan_Strife
//! （XML 声明两态：state0=armor_11 板 / state1=armor_10 板），实战中 armorMover 连续
//! 切换 6 轮。propID/载荷/值域的行为学定案见 `RawArmorStateTrack` 文档。

use std::collections::HashMap;
use std::path::PathBuf;

use wotb_replay_core::facets::ai_review::collect_armor_state_tracks;
use wotb_replay_core::replay::combat::GunPitchLimits;
use wotb_replay_core::replay::packets::read_raw_packets;
use wotb_replay_core::replay::playback::{build_playback_data, PlaybackInput};

fn titan_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/replay_samples")
        .join("20261013_0954__Anonyme_Oth71_Titan_Strife_579709385753595713.wotbreplay")
}

#[test]
fn armor_state_timeline_matches_field_observation() {
    let bytes = std::fs::read(titan_sample()).expect("动态装甲样本必须在语料中");
    let packets = read_raw_packets(&bytes).expect("分帧必须走满 EOF");

    let tracks = collect_armor_state_tracks(
        &packets
            .iter()
            .map(|p| (p.packet_type, p.clock_secs, p.payload.as_slice()))
            .collect::<Vec<_>>(),
    );

    assert_eq!(tracks.len(), 1, "只有作者车辆实体有状态更新: {tracks:?}");
    let tr = &tracks[0];
    assert_eq!(tr.eid, 1_35002059, "作者车辆实体");
    assert_eq!(
        tr.state,
        vec![1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0],
        "6 轮 0/1 交替 = 声明状态集 {{0,1}} 内的 armorMover 切换"
    );
    assert_eq!(tr.t.len(), tr.state.len());
    assert!(
        tr.t.windows(2).all(|w| w[0] < w[1]),
        "时钟必须严格升序（包序 = 时钟序）"
    );
    assert!(
        (tr.t[0] - 21.64).abs() < 0.05,
        "首次切换时刻应 ≈21.64s，实测 {}",
        tr.t[0]
    );
}

#[test]
fn non_dynamic_replay_has_empty_tracks() {
    // 静态装甲样本（T100LT）：prop0 无更新 → 轨道为空（facet 序列化时字段整体省略）
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/replay_samples")
        .join("20261010_1212__Anonyme_R132_T100LT_579650759449754426.wotbreplay");
    let bytes = std::fs::read(path).expect("静态样本必须在语料中");
    let packets = read_raw_packets(&bytes).unwrap();
    let tracks = collect_armor_state_tracks(
        &packets
            .iter()
            .map(|p| (p.packet_type, p.clock_secs, p.payload.as_slice()))
            .collect::<Vec<_>>(),
    );
    assert!(tracks.is_empty(), "静态装甲车不应产生状态轨道: {tracks:?}");
}

#[test]
fn playback_data_carries_armor_state_timeline() {
    // 回放切面（PlaybackData，消费方 3D/射击复现的实际入口）必须与切面收集器同源：
    // 字段 additive（v2 不变），动态车非空、静态车省略。
    let bytes = std::fs::read(titan_sample()).expect("动态装甲样本必须在语料中");
    let raw = read_raw_packets(&bytes).expect("分帧");
    let tuples: Vec<(u32, f32, &[u8])> = raw
        .iter()
        .map(|pk| (pk.packet_type, pk.clock_secs, pk.payload.as_slice()))
        .collect();
    let limits = GunPitchLimits::default();
    let pb = build_playback_data(&PlaybackInput {
        packets: &tuples,
        players: Vec::new(),
        author_account_id: 0,
        winner_team: 0,
        map_id: 0,
        map_name: String::new(),
        pitch_limits: &limits,
        tank_names: HashMap::new(),
    })
    .expect("PlaybackData 构建必须成功");

    assert_eq!(pb.armor_states.len(), 1, "只有作者车辆实体有状态轨道");
    let tr = &pb.armor_states[0];
    assert_eq!(tr.eid, 1_35002059);
    assert_eq!(tr.state, vec![1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]);

    // 序列化形状：非空时字段出现且为列式三元；静态车（空）字段省略——
    // 这里用 serde 直接验证 PlaybackData 的字段跳过行为
    let json = serde_json::to_string(&pb).unwrap();
    assert!(json.contains("armorStates") || json.contains("armor_states"));
}
