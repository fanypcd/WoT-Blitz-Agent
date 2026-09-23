use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if (mid == 0x01 || mid == 0x00) && 12 + alen <= p.len() {
                let a = &p[12..12 + alen];
                let hexv: Vec<String> = a.iter().map(|b| format!("{:02x}", b)).collect();
                let u16s: Vec<String> = a.chunks(2).filter(|c| c.len() == 2)
                    .map(|c| format!("{}", u16le(c))).collect();
                println!("m{:02x} t={:8.3} eid=0x{:08x} alen={} [{}] u16[{}]",
                    mid, pkt.clock_secs, u32le(&p[0..4]), alen, hexv.join(" "), u16s.join(" "));
            }
        }
    }
}
