//! 全文件扫描：u16 coarse∈[480,545]（对应俯仰 ±20°域）的所有 (type, off, eid) 流，
//! 并用两车开火真值打分。命中 = mae < 2° 的流。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // 开火真值 (t, elev_deg) — 全部
    let mut fires: Vec<(f32, f32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 49 {
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                    let a = &p[12..12 + alen];
                    let sid = u32le(&a[4..8]);
                    if seen.insert(sid) {
                        let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                        let n = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
                        fires.push((pkt.clock_secs, (v[1]/n).asin() * 57.29578));
                    }
                }
            }
        }
    }
    let coarse_of = |pitch_rad: f64| -> f64 { (pitch_rad + std::f64::consts::TAU) / (std::f64::consts::TAU * 7.0 / 3600.0) };
    // 扫描
    let mut streams: HashMap<(u8, usize, u32), Vec<(f32, f32)>> = HashMap::new();
    for pkt in &data.packets {
        let pt = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type as u8,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            _ => continue,
        };
        let p = &pkt.raw_payload[..];
        let eid = if p.len() >= 4 { u32le(&p[0..4]) } else { 0 };
        for off in 0..p.len().saturating_sub(2) {
            let u = u16le(&p[off..off+2]);
            let coarse = u >> 6;
            if !(480..=545).contains(&coarse) { continue; }
            let pitch = coarse as f64 * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
            streams.entry((pt, off, eid)).or_default().push((pkt.clock_secs, pitch as f32 * 57.29578));
        }
    }
    // 打分
    let mut out: Vec<(String, f32, usize)> = Vec::new();
    for ((pt, off, eid), v) in &streams {
        if v.len() < 15 { continue; }
        let mut es = 0.0; let mut n = 0;
        for (ft, te) in &fires {
            let best = v.iter().filter(|(t, _)| (*t - *ft).abs() <= 0.12)
                .map(|(_, p)| (p - te).abs())
                .fold(f32::MAX, f32::min);
            if best < f32::MAX { es += best; n += 1; }
        }
        if n >= 8 {
            out.push((format!("type={} off={} eid=0x{:08x} n={}", pt, off, eid, v.len()), es / n as f32, n));
        }
    }
    out.sort_by_key(|(_, m, _)| (*m * 1000.0) as i64);
    for (desc, mae, n) in out.iter().take(15) {
        println!("{} mae={:.3}° ({} 发)", desc, mae, n);
    }
    if out.is_empty() { println!("（无候选流）"); }
}
