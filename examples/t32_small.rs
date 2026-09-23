fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if !(9..=19).contains(&p.len()) { continue; }
            let hexv: Vec<String> = p.iter().map(|b| format!("{:02x}", b)).collect();
            println!("t={:8.3} len={} eid=0x{:08x} [{}]",
                pkt.clock_secs, p.len(), u32le(&p[0..4]), hexv.join(" "));
        }
    }
}
