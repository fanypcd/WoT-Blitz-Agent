//! 定位粗角命中（开火 ±0.12s、pitch 域 u16）的宿主包：
//! 命中 → (包 type, eid, 包内偏移)。真字段 = (type, 内偏移) 高度集中。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut fires: Vec<(f32, f32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut recs: Vec<(f32, u32, u32, Vec<u8>)> = Vec::new();
    for pkt in &data.packets {
        let pt = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type as u32,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            _ => continue,
        };
        let p = &pkt.raw_payload[..];
        let eid = if p.len() >= 4 { u32le(&p[0..4]) } else { 0 };
        recs.push((pkt.clock_secs, pt, eid, p.to_vec()));
        if pt == 8 && p.len() >= 49 {
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                let a = &p[12..12 + alen];
                let sid = u32le(&a[4..8]);
                if seen.insert(sid) {
                    let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                    let n = (v[0]*v[0]+v[1]*v[1]+v[2]*v[2]).sqrt();
                    fires.push((pkt.clock_secs, (v[1]/n).asin()));
                }
            }
        }
    }
    let pitch_to_coarse = |p: f64| (p + std::f64::consts::TAU) / (std::f64::consts::TAU * 7.0 / 3600.0);
    // (type, 内偏移) → 命中发数
    let mut buckets: HashMap<(u32, usize, u32), usize> = HashMap::new();
    for (ft, te) in &fires {
        let tc = pitch_to_coarse(*te as f64);
        for (t, pt, eid, p) in &recs {
            if (*t - *ft).abs() > 0.12 { continue; }
            for off in 0..p.len().saturating_sub(2) {
                let u = u16le(&p[off..off+2]);
                let c = (u >> 6) as f64;
                if (c - tc).abs() <= 1.5 {
                    *buckets.entry((*pt, off, *eid)).or_insert(0) += 1;
                }
            }
        }
    }
    let mut v: Vec<((u32, usize, u32), usize)> = buckets.into_iter().filter(|(_, c)| *c >= 5).collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    println!("命中≥5 发的 (type, 包内off, eid) 聚类:");
    for ((pt, off, eid), c) in v.iter().take(15) {
        println!("  type={} off={} eid=0x{:08x} → {} 发", pt, off, eid, c);
    }
    if v.is_empty() { println!("（无聚类）"); }
}
