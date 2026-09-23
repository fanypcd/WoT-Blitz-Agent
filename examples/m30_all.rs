fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut n8 = 0; let mut first_t = 0.0f32; let mut last_t = 0.0f32;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 20 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid != 0x30 || alen != 8 || 12 + 8 > p.len() { continue; }
            let a = &p[12..20];
            if n8 == 0 { first_t = pkt.clock_secs; }
            last_t = pkt.clock_secs;
            n8 += 1;
            if n8 <= 40 || n8 % 20 == 0 {
                println!("t={:8.3} [{:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}]",
                    pkt.clock_secs, a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7]);
            }
        }
    }
    println!("8B 包总数={} 时段 {:.2}~{:.2}", n8, first_t, last_t);
}
