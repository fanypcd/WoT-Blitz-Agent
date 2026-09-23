//! dump method 0x0c（22 包、6B 参数、覆盖全场 24.7~295.7s —— 俯仰候选）
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut nick_of_eid: HashMap<u32, String> = HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            let l = p[57] as usize;
            if !(3..=30).contains(&l) || 58 + l > p.len() { continue; }
            if let Ok(name) = std::str::from_utf8(&p[58..58 + l]) { nick_of_eid.insert(eid, name.to_string()); }
        }
    }
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid != 0x0c || 12 + alen > p.len() { continue; }
            let a = &p[12..12 + alen];
            let hexv: Vec<String> = a.iter().map(|b| format!("{:02x}", b)).collect();
            let f32s: Vec<String> = a.chunks(4).filter(|c| c.len() == 4)
                .map(|c| format!("{:.4}", f32le(c))).collect();
            let u16s: Vec<String> = a.chunks(2).filter(|c| c.len() == 2)
                .map(|c| format!("{}", u16::from_le_bytes([c[0], c[1]]))).collect();
            println!("t={:8.3} eid=0x{:08x}({}) [{}] f32[{}] u16[{}]",
                pkt.clock_secs, u32le(&p[0..4]),
                nick_of_eid.get(&u32le(&p[0..4])).map(|s| s.as_str()).unwrap_or("?"),
                hexv.join(" "), f32s.join(" "), u16s.join(" "));
        }
    }
}
