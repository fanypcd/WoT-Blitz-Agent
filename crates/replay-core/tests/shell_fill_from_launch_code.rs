//! 回归：cmpIndex=0（底盘/履带）命中无 26/27B 段包时的**弹种指纹回填**——method29 args[8]
//! 弹种编码（同场同射手自校准，编码优先、弹速去重/兜底）。
//!
//! 两层覆盖：
//! - **入库样本**（`data/replay_samples/` 中随仓库跟踪的样本，CI 可跑）：命中发次不得再有
//!   "命中但弹种未知"，且确有发次由弹种编码定案（证明编码路径被真实数据覆盖）；
//! - **定点复核**（`GB48_FV215b_183` 样本，**不入库** ⇒ 缺失即跳过）：该场他人路径 8 发
//!   "命中但弹种未知"全部为履带/底盘命中（无段包），其中 1 发（t≈180.89，编码 0x06、
//!   |v|=632）经编码定案为 32138（FV215b 183 的 AP）。
//!
//! 探针样本不入 CI（见 .github/workflows/ci.yml 注释），故第二层必须容忍样本缺席。

use std::path::{Path, PathBuf};

use wotb_replay_core::replay::combat::{
    extract_other_shot_replays_with_limits, resolve_author_player_eid_by_nick, GunPitchLimits,
    OtherShotsExtraction,
};
use wotb_replay_core::replay::packets::read_raw_packets;

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/replay_samples")
}

/// 对单个样本跑他人路径提取（作者昵称统一 "Anonyme"——样本均为该昵称录制）。
fn extract_others(path: &Path) -> OtherShotsExtraction {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("读样本 {} 失败: {e}", path.display()));
    let raw = read_raw_packets(&bytes).expect("分帧");
    let tuples: Vec<(u32, f32, &[u8])> = raw
        .iter()
        .map(|pk| (pk.packet_type, pk.clock_secs, pk.payload.as_slice()))
        .collect();
    let author_eid = resolve_author_player_eid_by_nick(&tuples, "Anonyme");
    let limits = GunPitchLimits::new();
    extract_other_shot_replays_with_limits(&tuples, author_eid, &limits)
}

fn by_code_count(others: &OtherShotsExtraction) -> usize {
    others
        .shots
        .iter()
        .filter(|s| {
            s.quality
                .as_ref()
                .is_some_and(|q| q.shell_from_launch_code)
        })
        .count()
}

/// CI 层：入库样本上，命中发次不得留"弹种未知"，且编码回填路径被真实数据覆盖。
#[test]
fn tracked_samples_have_no_unknown_shell_on_hits() {
    let dir = samples_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|it| {
            it.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "wotbreplay"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    assert!(!files.is_empty(), "data/replay_samples 必须有入库样本");

    let mut total_by_code = 0usize;
    for path in &files {
        let others = extract_others(path);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let unknown: Vec<(f32, Option<u32>)> = others
            .shots
            .iter()
            .filter(|s| s.target_eid.is_some() && s.shell_id == 0)
            .map(|s| (s.fire_time, s.target_eid))
            .collect();
        assert!(
            unknown.is_empty(),
            "{name}: 指纹回填后仍有命中无弹种：{unknown:?}"
        );
        total_by_code += by_code_count(&others);
    }
    assert!(
        total_by_code > 0,
        "入库样本合计应有发次由弹种编码定案（证明编码路径被真实数据覆盖），实测 0"
    );
}

/// 定点复核层：`GB48_FV215b_183` 样本（不入库）在场时逐点校验；缺席即跳过。
#[test]
fn cmp0_hits_get_shell_by_launch_code_fingerprint() {
    let path = samples_dir().join("20260930_2127__Anonyme_GB48_FV215b_183_581811140883713611.wotbreplay");
    if !path.is_file() {
        eprintln!(
            "定点样本不在库内（{}），跳过——探针样本不入 CI（本机有样本时执行）",
            path.display()
        );
        return;
    }
    let others = extract_others(&path);

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
    let by_code = by_code_count(&others);
    assert!(by_code >= 8, "应至少 8 发由弹种编码定案，实测 {by_code}");

    // 定点复核：t≈180.89（编码 0x06、|v|=632）→ 弹种 32138（FV215b 183 AP）
    let spot = others
        .shots
        .iter()
        .find(|s| (s.fire_time - 180.89).abs() < 0.02)
        .expect("样本应有 t≈180.89 发次");
    assert_eq!(spot.shell_id, 32138, "定点复核弹种编码映射");
}
