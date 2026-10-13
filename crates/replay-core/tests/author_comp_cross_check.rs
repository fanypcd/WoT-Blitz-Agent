//! 回归：作者 comp descriptor 双源交叉验证（meta.json `vehicleCompDescriptor` ×
//! 结算 #301 f103）。2026-10-13 对照调查新增
//! （investigation/client-usage-vs-agent-implementation.md §2.2 P2-2）——两源独立
//! （crate `Meta.tank_id` / `PlayerSettlement.tank_id`），实测逐值一致。

use std::path::PathBuf;

use wotb_replay_core::replay::parser::ReplayParser;

#[test]
fn author_comp_descriptor_sources_agree_on_fixture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/replay_samples")
        .join("2.wotbreplay");
    let summary = ReplayParser::new()
        .parse_file(&path)
        .expect("夹具回放必须可解析");

    let meta = summary.author_comp_descriptor_meta.expect("meta 源应存在");
    let settlement = summary
        .author_comp_descriptor_settlement
        .expect("结算 #301 f103 源应存在");
    assert_eq!(meta, settlement, "两源必须一致");
    assert_eq!(summary.author_comp_descriptor_mismatch, Some(false));
}
