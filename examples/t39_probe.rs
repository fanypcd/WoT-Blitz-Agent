fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    use std::collections::BTreeMap;
    let mut by_eid: BTreeMap<u32, usize> = BTreeMap::new();
    let mut shown = 0;
    let mut lens = std::collections::BTreeMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 39 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            *lens.entry(p.len()).or_insert(0) += 1;
            if p.len() >= 4 { *by_eid.entry(u32le(&p[0..4])).or_insert(0) += 1; }
            if shown < 8 {
                let f32s: Vec<String> = p.chunks(4).filter(|c| c.len() == 4)
                    .map(|c| format!("{:.3}", f32le(c))).collect();
                println!("t={:8.3} len={} [{}]", pkt.clock_secs, p.len(), f32s.join(" "));
                shown += 1;
            }
        }
    }
    println!("eid 分布: {:?}\nlen 分布: {:?}", by_eid, lens);
}
