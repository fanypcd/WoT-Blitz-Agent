fn main() {
    let path = "replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay";
    let f = std::fs::File::open(path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    use std::collections::BTreeMap;
    let mut m: BTreeMap<u32, usize> = BTreeMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 {
                let prop = u32le(&p[4..8]);
                if prop == 2 { *m.entry(u32le(&p[0..4])).or_insert(0) += 1; }
            }
        }
    }
    for (e, c) in m { println!("0x{:08x}  {}", e, c); }
}
