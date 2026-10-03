//! 原始分帧与 crate `read_data()` 的等价性：在 crate 能完整解析的真实样本上，
//! `read_raw_packets` 必须逐包产出相同的 (type, clock, payload)。

use std::path::PathBuf;

use wotb_replay_core::replay::packets::read_raw_packets;
use wotbreplay_parser::models::data::payload::Payload;
use wotbreplay_parser::replay::Replay;

fn samples() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/replay_samples");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|it| {
            it.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "wotbreplay"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[test]
fn raw_framing_matches_crate_packets_on_real_samples() {
    let files = samples();
    assert!(!files.is_empty(), "data/replay_samples 必须有真实样本");
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let mut replay = Replay::open(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let data = replay
            .read_data()
            .expect("样本必须能被 crate 完整解析，才有资格做等价基准");
        let expected: Vec<(u32, f32, Vec<u8>)> = data
            .packets
            .iter()
            .map(|pkt| {
                let t = match &pkt.payload {
                    Payload::EntityMethod(_) => 8,
                    Payload::BasePlayerCreate { .. } => 0,
                    Payload::Unknown { packet_type } => *packet_type,
                };
                (t, pkt.clock_secs, pkt.raw_payload.to_vec())
            })
            .collect();

        let actual: Vec<(u32, f32, Vec<u8>)> = read_raw_packets(&bytes)
            .unwrap()
            .into_iter()
            .map(|p| (p.packet_type, p.clock_secs, p.payload))
            .collect();

        assert_eq!(
            actual.len(),
            expected.len(),
            "{}: 包数量不一致",
            path.display()
        );
        for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
            assert_eq!(a, e, "{}: 第 {i} 个包不一致", path.display());
        }
    }
}
