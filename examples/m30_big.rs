fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid != 0x30 || alen <= 100 || 12 + alen > p.len() { continue; }
            let a = &p[12..12 + alen];
            let hexv: Vec<String> = a.iter().map(|b| format!("{:02x}", b)).collect();
            println!("t={:8.3} alen={} [{}...]", pkt.clock_secs, alen,
                hexv.iter().take(60).cloned().collect::<Vec<_>>().join(" "));
        }
    }
}
