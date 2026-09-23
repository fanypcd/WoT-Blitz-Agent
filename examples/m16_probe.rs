
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
    // method29 发射时刻
    let mut fires: Vec<(f32, u32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 30 {
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                    let a = &p[12..12 + alen];
                    let sid = u32le(&a[4..8]);
                    if seen.insert(sid) {
                        fires.push((pkt.clock_secs, u32le(&a[0..4])));
                    }
                }
            }
        }
    }
    // type32 24/25B method16 包全量
    let mut n = 0;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if !(p.len() == 24 || p.len() == 25) { continue; }
            let eid = u32le(&p[0..4]);
            let method = u32le(&p[5..9]);
            if method != 0x10 { continue; }
            let a = &p[9..];
            if a.len() < 4 { continue; }
            let yaw16 = u16::from_le_bytes([a[0], a[1]]);
            let pitch16 = u16::from_le_bytes([a[2], a[3]]);
            // f64 时间戳 @ a[4..12], f32 尾 @ a[12..16]
            let f64v = if a.len() >= 12 { f64::from_le_bytes(a[4..12].try_into().unwrap()) } else { 0.0 };
            let f32v = if a.len() >= 16 { f32le(&a[12..16]) } else { 0.0 };
            let near_fire = fires.iter().any(|(t, _)| (t - pkt.clock_secs).abs() < 0.3);
            if n < 40 || near_fire {
                println!("t={:8.3} eid=0x{:08x}({}) yaw16={:5} pitch16={:5} f64={:12.2} f32={:8.3} {}",
                    pkt.clock_secs, eid, nick_of_eid.get(&eid).map(|s| s.as_str()).unwrap_or("?"),
                    yaw16, pitch16, f64v, f32v, if near_fire { "<-- 开火±0.3s" } else { "" });
                n += 1;
            }
        }
    }
}
