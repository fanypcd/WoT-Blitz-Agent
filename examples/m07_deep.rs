fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    use std::collections::BTreeMap;
    let mut fb: BTreeMap<u8, usize> = BTreeMap::new();
    let mut by_time: Vec<(f32, u8, u32)> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 17 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid != 0x07 || alen < 5 || 12 + alen > p.len() { continue; }
            let a = &p[12..12 + alen];
            *fb.entry(a[0]).or_insert(0) += 1;
            by_time.push((pkt.clock_secs, a[0], u32le(&a[1..5])));
        }
    }
    println!("首字节分布: {:?}", fb);
    by_time.sort_by_key(|(t, _, _)| (*t * 100.0) as i64);
    for (t, a0, sid) in by_time.iter().take(30) {
        println!("t={:8.3} a0={:02x} u32={}", t, a0, sid);
    }
}
