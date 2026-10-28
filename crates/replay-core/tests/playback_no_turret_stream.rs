//! **type=7 炮塔流全局缺失**的回放必须仍能建出回放数据（2026-10-10 用户"3D 回放打不开"）。
//!
//! 样本 `20261010_1212__Anonyme_R132_T100LT_…`（训练房 7 s）实测：type=10 位姿 195 包、
//! **type=7 零条**（坦克全程未瞄炮）。车辆身份由 Type5 class=2 决定，与炮塔流无关 ⇒ 该车
//! 正常收录；炮塔/炮管取 type=5 创建包前缀属性 blob 的 prop2 初值（§2.6），无变化流时
//! 列值恒定。同场另一条 type=10 流没有 Type5 class
//! （回放相机实体，位置随后跳到 0,0,0）——旧 `st10` 退化口径会把它当第二辆车，现口径不收录。

use std::collections::HashMap;
use std::path::PathBuf;

use wotb_replay_core::replay::combat::GunPitchLimits;
use wotb_replay_core::replay::packets::read_raw_packets;
use wotb_replay_core::replay::playback::{build_playback_data, PlaybackInput, PlaybackPlayer};

#[test]
fn training_room_without_any_turret_stream_keeps_the_real_vehicle_only() {
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
        // 结算花名册是显示车辆的权威名单：样本里唯一 class=2 车辆昵称即 "Anonyme"
        players: vec![PlaybackPlayer {
            account_id: 2033684170,
            nickname: "Anonyme".into(),
            team: 2,
            tank_id: 6225,
        }],
        author_account_id: 0,
        winner_team: 0,
        map_id: 21,
        map_name: "Canal".into(),
        pitch_limits: &limits,
        tank_names: HashMap::new(),
    };
    let pb = build_playback_data(&input).expect("prop2 全局缺失不应 fail-closed");
    assert_eq!(pb.vehicles.len(), 1, "唯一 class=2 车辆；无 class 的相机流不得成车");
    let v = &pb.vehicles[0];
    assert_eq!(v.nickname, "Anonyme", "Type5 昵称身份应保留");
    assert_eq!(v.turret_yaw.len(), v.hull_yaw.len(), "炮塔列与车体列严格同长");
    assert_eq!(v.gun_pitch.len(), v.hull_yaw.len());
    // 无 type=7 变化流 ≠ 无 prop2 数据：type=5 创建包前缀属性 blob 的 id2 是 prop2 初值
    // （《回放与射击逆向总集》§2.6）。全程不转炮塔/不动炮管时列值恒定在该初值上；
    // 旧口径（整列中性 0）会丢掉真实初值（2026-10-28 训练房射击分析样本）。
    let rel: Vec<f32> = v
        .turret_yaw
        .iter()
        .zip(&v.hull_yaw)
        .map(|(turret, hull)| turret - hull)
        .collect();
    assert!(
        rel.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-6),
        "无 type=7 变化流时炮塔相对角应恒定（保持 type=5 初值）"
    );
    assert!(
        v.gun_pitch.iter().all(|p| (*p - v.gun_pitch[0]).abs() < 1e-6),
        "无 type=7 变化流时炮管俯仰应恒定（保持 type=5 初值）"
    );
}
