//! 回归：cmpIndex=0（底盘/履带）命中无 26/27B 段包时的**弹种指纹回填**——method29 args[8]
//! 弹种编码（同场同射手自校准，编码优先、弹速去重/兜底）。
//!
//! 样本 `20260930_2127__Anonyme_GB48_FV215b_183_…` 实测：他人路径 8 发"命中但弹种未知"
//! 全部为履带/底盘命中（无段包），其中 1 发（t≈180.89，编码 0x06、|v|=632）经编码定案为
//! 32138（FV215b 183 的 AP）。回填后不应再有任何"命中但弹种未知"的他人发次。

use std::path::PathBuf;

use wotb_replay_core::replay::combat::{
    extract_other_shot_replays_with_limits, resolve_author_player_eid_by_nick, GunPitchLimits,
};
use wotb_replay_core::replay::packets::read_raw_packets;

#[test]
fn cmp0_hits_get_shell_by_launch_code_fingerprint() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/replay_samples")
        .join("20260930_2127__Anonyme_GB48_FV215b_183_581811140883713611.wotbreplay");
    let bytes = std::fs::read(&path).expect("样本必须在库内");
    let raw = read_raw_packets(&bytes).expect("分帧");
    let tuples: Vec<(u32, f32, &[u8])> = raw
        .iter()
        .map(|pk| (pk.packet_type, pk.clock_secs, pk.payload.as_slice()))
        .collect();
    let author_eid = resolve_author_player_eid_by_nick(&tuples, "Anonyme");
    let limits = GunPitchLimits::new();
    let others = extract_other_shot_replays_with_limits(&tuples, author_eid, &limits);

    // 前提自证：本场确有他人命中发次（否则用例空转）
    let hits: Vec<_> = others
        .shots
        .iter()
        .filter(|s| s.target_eid.is_some())
        .collect();
    assert!(!hits.is_empty(), "样本应含他人命中发次");

    // 核心断言：不再存在"命中但弹种未知"（回填前实测 8 发履带/底盘命中空壳）
    let unknown: Vec<(f32, Option<u32>)> = hits
        .iter()
        .filter(|s| s.shell_id == 0)
        .map(|s| (s.fire_time, s.target_eid))
        .collect();
    assert!(unknown.is_empty(), "指纹回填后仍有无弹种命中：{unknown:?}");

    // 编码档优先：本场空壳发次应至少 8 发由弹种编码定案（不含段包/地形既有来源）
    let by_code = others
        .shots
        .iter()
        .filter(|s| {
            s.quality
                .as_ref()
                .is_some_and(|q| q.shell_from_launch_code)
        })
        .count();
    assert!(by_code >= 8, "应至少 8 发由弹种编码定案，实测 {by_code}");

    // 定点复核：t≈180.89（编码 0x06、|v|=632）→ 弹种 32138（FV215b 183 AP）
    let spot = others
        .shots
        .iter()
        .find(|s| (s.fire_time - 180.89).abs() < 0.02)
        .expect("样本应有 t≈180.89 发次");
    assert_eq!(spot.shell_id, 32138, "定点复核弹种编码映射");
}
