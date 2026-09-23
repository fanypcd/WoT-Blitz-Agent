
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut lens = std::collections::BTreeMap::new();
    let mut n13 = 0;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            *lens.entry(p.len()).or_insert(0) += 1;
            if p.len() == 13 {
                n13 += 1;
                if n13 <= 3 {
                    println!("len13: eid=0x{:08x} prop={} alen={} val={:02x}",
                        u32le(&p[0..4]), u32le(&p[4..8]), u32le(&p[8..12]), p[12]);
                }
            }
        }
    }
    println!("len 分布: {:?}", lens);
}
