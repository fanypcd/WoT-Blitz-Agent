//! 全文件暴力搜索：对每个开火真值俯仰计算 coarse 目标值（±3 级容差），
//! 在整个 data.wotreplay 解压流中搜索 u16 小端匹配（任意偏移），输出命中偏移的
//! 跨发聚类统计——若存在俯仰流，会出现"每次开火时刻附近都有命中"的偏移族。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // 开火真值
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
                        fires.push((pkt.clock_secs, (v[1]/n).asin()));
                    }
                }
            }
        }
    }
    // 拼接全部包 payload 成连续流（带包时间索引）
    let mut stream: Vec<u8> = Vec::new();
    let mut marks: Vec<(usize, f32)> = Vec::new(); // stream off → time
    for pkt in &data.packets {
        let raw = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: _ } => Some(&pkt.raw_payload[..]),
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => Some(&pkt.raw_payload[..]),
            _ => None,
        };
        if let Some(raw) = raw {
            marks.push((stream.len(), pkt.clock_secs));
            stream.extend_from_slice(raw);
        }
    }
    // 对每个开火时刻 ±0.12s 的流区间，找 pitch coarse 匹配 u16
    let pitch_to_coarse = |pitch_rad: f64| -> f64 { (pitch_rad + std::f64::consts::TAU) / (std::f64::consts::TAU * 7.0 / 3600.0) };
    let time_at = |off: usize| -> f32 {
        let mut t = 0.0f32;
        for (o, tt) in &marks { if *o <= off { t = *tt; } else { break; } }
        t
    };
    // 对每个 (off, fire) 匹配计数
    let mut hit_map: HashMap<usize, usize> = HashMap::new(); // off/64 bucket → fires matched
    let mut detail: Vec<(f32, usize, u16, f32)> = Vec::new();
    for (ft, te) in &fires {
        let target_c = pitch_to_coarse(*te as f64);
        for off in 0..stream.len().saturating_sub(2) {
            let t = time_at(off);
            if (t - *ft).abs() > 0.12 { continue; }
            let u = u16::from_le_bytes([stream[off], stream[off+1]]);
            let c = u >> 6;
            if (c as f64 - target_c).abs() <= 2.5 {
                *hit_map.entry(off / 64).or_insert(0) += 1;
                detail.push((t, off, u, *te));
            }
        }
    }
    // 聚类命中 ≥8 发的 bucket
    let mut buckets: Vec<(usize, usize)> = hit_map.into_iter().filter(|(_, c)| *c >= 8).collect();
    buckets.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    println!("命中≥8 发的 64B 桶数: {}", buckets.len());
    for (b, c) in buckets.iter().take(12) {
        println!("  bucket(off={}) 命中 {} 发", b * 64, c);
    }
    // 打印前 20 条明细
    detail.sort_by_key(|(t, off, _, _)| ((*t * 100.0) as i64, *off));
    println!("\n明细（前 20）:");
    for (t, off, u, te) in detail.iter().take(20) {
        let c = u >> 6;
        let pitch = c as f64 * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
        println!("  t={:8.3} off={:8} u16={:5} coarse={:4} → {:+.2}° (真值 {:+.2}°)",
            t, off, u, c, pitch * 57.29578, te * 57.29578);
    }
}
