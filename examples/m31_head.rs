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
            if mid != 0x31 || 12 + alen > p.len() { continue; }
            let a = &p[12..12 + alen];
            println!("t={:.3} alen={}", pkt.clock_secs, alen);
            // 尝试 protobuf field 扫描：打印前 80B hex + 可读 ASCII
            for chunk_start in (0..600).step_by(600) {
                let end = (chunk_start + 600).min(alen);
                let hexv: Vec<String> = a[chunk_start..end].iter().map(|b| format!("{:02x}", b)).collect();
                for r in (0..hexv.len()).step_by(32) {
                    let seg: Vec<String> = hexv[r..(r+32).min(hexv.len())].to_vec();
                    println!("  {:04x}: {}", chunk_start + r, seg.join(" "));
                }
                // ASCII
                let ascii: String = a[chunk_start..end].iter()
                    .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                    .collect();
                println!("  ascii: {}", ascii);
            }
        }
    }
}
