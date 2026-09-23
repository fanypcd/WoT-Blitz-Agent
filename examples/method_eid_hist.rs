
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // eid → mid → count
    let mut m: BTreeMap<u32, BTreeMap<u32, usize>> = BTreeMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let eid = u32le(&p[0..4]);
            let mid = u32le(&p[4..8]);
            *m.entry(eid).or_default().entry(mid).or_insert(0) += 1;
        }
    }
    for (eid, mids) in m {
        let parts: Vec<String> = mids.iter().map(|(k, v)| format!("{:02x}:{}", k, v)).collect();
        println!("eid=0x{:08x}  {}", eid, parts.join(" "));
    }
}
