//! 回归：**prop2（炮塔流）全局缺失**的回放必须仍能建出回放数据（2026-10-10 用户"3D 回放打不开"）。
//!
//! 样本 `20261010_1212__Anonyme_R132_T100LT_…`（训练房 7 s）实测：type=10 位姿 195 包、
//! 两个实体各 100/95 样本（≥ MIN_ST10_SAMPLES），**type=7 零条**（坦克全程未瞄炮）。旧判据
//! `st10 ∧ prop2` 交集为空 ⇒ `无任何车辆姿态流` ⇒ 回放打不开。现口径：prop2 全局缺失时退化
//! 为仅 st10（地图对象没有 type=10 流 ⇒ 不引入幻影）；该车炮塔按"随车体、炮管水平"中性渲染。

use std::collections::HashMap;
use std::path::PathBuf;

use wotb_replay_core::replay::combat::GunPitchLimits;
use wotb_replay_core::replay::packets::read_raw_packets;
use wotb_replay_core::replay::playback::{build_playback_data, PlaybackInput};

#[test]
fn training_room_without_any_turret_stream_still_builds() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/replay_samples")
        .join("20261010_1212__Anonyme_R132_T100LT_579650759449754426.wotbreplay");
    let bytes = std::fs::read(&path).expect("样本必须在库内");
    let raw = read_raw_packets(&bytes).expect("分帧");
    let tuples: Vec<(u32, f32, &[u8])> = raw
        .iter()
        .map(|pk| (pk.packet_type, pk.clock_secs, pk.payload.as_slice()))
        .collect();
    // 前提自证：本场确有 type=10、确无 type=7（否则用例空转）
    assert!(tuples.iter().any(|(t, _, _)| *t == 10), "样本应有 type=10 位姿流");
    assert!(!tuples.iter().any(|(t, _, _)| *t == 7), "样本应无 type=7 炮塔流");

    let limits = GunPitchLimits::default();
    let input = PlaybackInput {
        packets: &tuples,
        players: Vec::new(),
        author_account_id: 0,
        winner_team: 0,
        map_id: 21,
        map_name: "Canal".into(),
        pitch_limits: &limits,
        tank_names: HashMap::new(),
    };
    let pb = build_playback_data(&input).expect("prop2 全局缺失时应退化放行，而不是 fail-closed");
    assert!(!pb.vehicles.is_empty(), "至少一辆车（实测 2 辆）");
    for v in &pb.vehicles {
        assert_eq!(v.turret_yaw.len(), v.hull_yaw.len(), "炮塔列与车体列严格同长");
        assert_eq!(v.gun_pitch.len(), v.hull_yaw.len());
    }
}
