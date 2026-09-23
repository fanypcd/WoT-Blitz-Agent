fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if !matches!(packet_type, 23 | 33 | 26 | 29) { continue; }
            let hexv: Vec<String> = p.iter().map(|b| format!("{:02x}", b)).collect();
            let f32s: Vec<String> = p.chunks(4).filter(|c| c.len() == 4)
                .map(|c| format!("{:.4}", f32le(c))).collect();
            let u16s: Vec<String> = p.chunks(2).filter(|c| c.len() == 2)
                .map(|c| format!("{}", u16::from_le_bytes([c[0], c[1]]))).collect();
            println!("t={:8.3} type={:<2} len={} [{}] f32[{}] u16[{}]",
                pkt.clock_secs, packet_type, p.len(), hexv.join(" "), f32s.join(" "), u16s.join(" "));
        }
    }
}
