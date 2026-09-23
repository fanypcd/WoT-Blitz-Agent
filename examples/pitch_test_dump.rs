//! 纯俯仰测试回放：全部 type=7 prop 流 + type=32 25B 值时序 + type=10 pitch
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f64le = |b: &[u8]| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
    // type=10 pitch 时序（每实体）
    let mut t10: HashMap<u32, Vec<(f32, f32)>> = HashMap::new();
    // type=7
    let mut p2: Vec<(f32, u32, u16)> = Vec::new();
    let mut p4: Vec<(f32, u32, u16)> = Vec::new();
    // type=32 25B
    let mut t32: Vec<(f32, u32, u8, f32)> = Vec::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    t10.entry(u32le(&p[0..4])).or_default().push((
                        pkt.clock_secs, f32le(&p[40..44])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 {
                    let prop = u32le(&p[4..8]);
                    let alen = u32le(&p[8..12]) as usize;
                    let eid = u32le(&p[0..4]);
                    if alen == 2 && 12 + 2 <= p.len() {
                        let u = u16le(&p[12..14]);
                        if prop == 2 { p2.push((pkt.clock_secs, eid, u)); }
                        if prop == 4 { p4.push((pkt.clock_secs, eid, u)); }
                    }
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } => {
                let p = &pkt.raw_payload[..];
                if p.len() == 25 {
                    let eid = u32le(&p[0..4]);
                    let idx = p[12];
                    let v = f32le(&p[21..25]);
                    t32.push((pkt.clock_secs, eid, idx, v));
                }
            }
            _ => {}
        }
    }
    println!("=== type10 pitch（每实体首尾 + 值域）===");
    for (e, s) in &t10 {
        let vals: Vec<f32> = s.iter().map(|(_, v)| *v).collect();
        let mn = vals.iter().cloned().fold(f32::MAX, f32::min);
        let mx = vals.iter().cloned().fold(f32::MIN, f32::max);
        println!("  eid=0x{:08x} n={} pitch {:+.3}~{:+.3} rad ({:+.1}°~{:+.1}°) t {:.1}~{:.1}",
            e, s.len(), mn, mx, mn*57.3, mx*57.3, s[0].0, s[s.len()-1].0);
    }
    println!("\n=== prop2 时序（全部）===");
    for (t, e, u) in &p2 {
        println!("  t={:8.3} eid=0x{:08x} u=0x{:04x} yaw解码={:+.2}° pitch解码(×0.7-360wrap)={:+.2}°",
            t, e, u, *u as f64/65535.0*360.0-180.0, {
                let c = (u >> 6) as f64;
                let mut p = c * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
                while p > std::f64::consts::PI { p -= std::f64::consts::TAU; }
                while p < -std::f64::consts::PI { p += std::f64::consts::TAU; }
                p * 57.29578
            });
    }
    println!("\n=== prop4 时序（前 20）===");
    for (t, e, u) in p4.iter().take(20) {
        println!("  t={:8.3} eid=0x{:08x} u={}", t, e, u);
    }
    println!("\n=== type=32 25B 值时序 ===");
    for (t, e, i, v) in &t32 {
        println!("  t={:8.3} eid=0x{:08x} idx={} val={:+.3}", t, e, i, v);
    }
}
