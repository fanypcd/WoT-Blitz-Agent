//! 列出 type=7 属性流涉及的实体 eid（区分玩家实体 vs 车辆实体）。
use std::collections::BTreeSet;
fn main() {
    let path = std::env::args().nth(1).expect("usage: eid_list <file>");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut eids: BTreeSet<u32> = BTreeSet::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 { eids.insert(u32le(&p[0..4])); }
        }
    }
    for e in eids { println!("0x{:08x}", e); }
}
