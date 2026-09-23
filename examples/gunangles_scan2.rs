//! 穷举扫描：任意包型任意偏移的 (u16 yaw_p, u16 pitch_p) 候选，
//! 解码 pitch = coarse*2π*7/3600−2π ∈ ±20° 且 yaw 任意；
//! 重点检验与开火时刻真值的时间对齐。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    // 开火真值: (t, elev_world)
    let mut fires: Vec<(f32, f32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 49 {
                let mid = u32::from_le_bytes(p[4..8].try_into().unwrap());
                let alen = u32::from_le_bytes(p[8..12].try_into().unwrap()) as usize;
                if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                    let a = &p[12..12 + alen];
                    let sid = u32::from_le_bytes(a[4..8].try_into().unwrap());
                    if seen.insert(sid) {
                        let v = [
                            f32::from_le_bytes(a[21..25].try_into().unwrap()),
                            f32::from_le_bytes(a[25..29].try_into().unwrap()),
                            f32::from_le_bytes(a[29..33].try_into().unwrap()),
                        ];
                        let n = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
                        fires.push((pkt.clock_secs, (v[1]/n).asin()));
                    }
                }
            }
        }
    }
    // 扫描所有包内 u16 对
    let mut per_off: HashMap<(u8, usize), Vec<(f32, f32)>> = HashMap::new(); // → (t, pitch_deg)
    for pkt in &data.packets {
        let pt = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type as u8,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            _ => continue,
        };
        let p = &pkt.raw_payload[..];
        for off in 0..p.len().saturating_sub(4) {
            let pitch_p = u16le(&p[off+2..off+4]);
            if pitch_p == 0 { continue; }
            let coarse = (pitch_p >> 6) as f64;
            let mut pitch = coarse * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
            while pitch > std::f64::consts::PI { pitch -= std::f64::consts::TAU; }
            while pitch < -std::f64::consts::PI { pitch += std::f64::consts::TAU; }
            if pitch.abs() > 0.35 { continue; }
            per_off.entry((pt, off)).or_default().push((pkt.clock_secs, pitch as f32));
        }
    }
    // 对每个 (type,off) 候选：在开火时刻 ±0.1s 有采样且值接近真值？
    println!("候选 (type,off) 与开火真值对照:");
    let mut scored: Vec<(_, f32, usize)> = Vec::new();
    for (k, v) in &per_off {
        if v.len() < 20 { continue; }
        let mut err_sum = 0.0; let mut n = 0;
        for (ft, te) in &fires {
            let best = v.iter().filter(|(t, _)| (*t - *ft).abs() <= 0.12)
                .map(|(_, p)| (p - te).abs())
                .fold(f32::MAX, f32::min);
            if best < f32::MAX { err_sum += best; n += 1; }
        }
        if n >= 5 {
            let mae = err_sum / n as f32;
            scored.push((k.clone(), mae, n));
        }
    }
    scored.sort_by_key(|(_, m, _)| (*m * 1000.0) as i64);
    for ((pt, off), mae, n) in scored.iter().take(12) {
        println!("  type={} off={:4} mae={:.3}° over {} fires", pt, off, mae, n);
    }
}
