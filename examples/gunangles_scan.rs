//! 在回放字节流中扫描可能的 set_gunAnglesPacked 携带包：
//! 特征 = 8B 参数 {u16 yaw_packed, u16 pitch_packed} 且 pitch 真值在 ±0.35 rad（±20°）内。
//! 扫描所有 type=7/8/32 包的任意偏移，统计命中偏移分布。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let mut hits: HashMap<(u8, usize), usize> = HashMap::new();
    let mut sample: Vec<(f32, u8, usize, u16, u16)> = Vec::new();
    for pkt in &data.packets {
        let pt = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type as u8,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            _ => continue,
        };
        let p = &pkt.raw_payload[..];
        // 4 字节对齐窗口扫描（跳过实体 id 头 4B）
        for off in 4..p.len().saturating_sub(8) {
            if off % 4 != 0 { continue; }
            let yaw_p = u16le(&p[off..off+2]);
            let pitch_p = u16le(&p[off+2..off+4]);
            if yaw_p == 0 || pitch_p == 0 { continue; }
            let coarse = (pitch_p >> 6) as f64;
            let mut pitch = coarse * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
            while pitch > std::f64::consts::PI { pitch -= std::f64::consts::TAU; }
            while pitch < -std::f64::consts::PI { pitch += std::f64::consts::TAU; }
            // 炮管俯仰合理域 ±20°
            if pitch.abs() > 0.35 { continue; }
            // yaw 合理域: 任意，但排除明显全 0xff/0x00
            *hits.entry((pt, off)).or_insert(0) += 1;
            if sample.len() < 30 {
                sample.push((pkt.clock_secs, pt, off, yaw_p, pitch_p));
            }
        }
    }
    let mut v: Vec<((u8, usize), usize)> = hits.into_iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    println!("type/off → 命中数 (top 20):");
    for ((pt, off), c) in v.iter().take(20) {
        println!("  type={} off={} count={}", pt, off, c);
    }
    println!("\n样本:");
    for (t, pt, off, y, pp) in sample.iter().take(15) {
        let coarse = pp >> 6;
        println!("  t={:8.3} type={} off={:2} yaw16={:5} pitch16={:5} coarse={:4}", t, pt, off, y, pp, coarse);
    }
}
