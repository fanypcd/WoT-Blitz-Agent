fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut n = 0;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 49 || n > 400 { continue; }
            let eid = u32le(&p[0..4]);
            if eid != 0x100c7d6c && eid != 0x100c7d6d { continue; }
            let e = [f32le(&p[24..28]), f32le(&p[28..32]), f32le(&p[32..36])];
            let b48 = p[48];
            println!("t={:8.3} eid=..{:02x} err=[{:+.4} {:+.4} {:+.4}] st={:02x}",
                pkt.clock_secs, (eid & 0xff) as u8, e[0], e[1], e[2], b48);
            n += 1;
        }
    }
}
