//! J39 裸流（12B 记录头 [u32 size][u32 type][f32 time]）精确搜索：
//! 在 WI shot.time ±0.12s 窗口内的所有记录 payload 中搜 pitch coarse 匹配，
//! 输出 (记录 type, 包内偏移) 的跨发聚类。
use std::collections::HashMap;
fn main() {
    let stream = std::fs::read("tmp_wi2/data.wotreplay").unwrap();
    let wi_text = std::fs::read_to_string("tmp_wi2/wi_shots.json").unwrap();
    let wi: serde_json::Value = serde_json::from_str(&wi_text).unwrap();
    let fires: Vec<(f32, f32)> = wi.as_array().unwrap().iter()
        .map(|s| (s["time"].as_f64().unwrap() as f32, s["gun_pitch"].as_f64().unwrap() as f32))
        .collect();
    // 解析记录
    struct Rec { rtype: u32, time: f32, payload: (usize, usize) } // payload range
    let mut recs: Vec<Rec> = Vec::new();
    let mut off = 12 + 42;  // magic+size+subLen+sub
    while off + 12 <= stream.len() {
        let sz = u32::from_le_bytes(stream[off..off+4].try_into().unwrap()) as usize;
        let rt = u32::from_le_bytes(stream[off+4..off+8].try_into().unwrap());
        let tm = f32::from_le_bytes(stream[off+8..off+12].try_into().unwrap());
        if sz > stream.len() || off + 12 + sz > stream.len() { break; }
        recs.push(Rec { rtype: rt, time: tm, payload: (off+12, off+12+sz) });
        off += 12 + sz;
    }
    println!("记录数: {} (流长 {})", recs.len(), stream.len());
    let pitch_to_coarse = |pitch_rad: f64| -> f64 { (pitch_rad + std::f64::consts::TAU) / (std::f64::consts::TAU * 7.0 / 3600.0) };
    // (type, offset_in_payload) → 命中发数
    let mut buckets: HashMap<(u32, usize), usize> = HashMap::new();
    let mut detail: Vec<(f32, u32, usize, u16, f32)> = Vec::new();
    for (ft, pe) in &fires {
        let target_c = pitch_to_coarse(*pe as f64);
        for r in &recs {
            if (r.time - *ft).abs() > 0.12 { continue; }
            let (s0, s1) = r.payload;
            let payload = &stream[s0..s1];
            for po in 0..payload.len().saturating_sub(2) {
                let u = u16::from_le_bytes([payload[po], payload[po+1]]);
                let c = u >> 6;
                if (c as f64 - target_c).abs() <= 1.5 {
                    *buckets.entry((r.rtype, po)).or_insert(0) += 1;
                    if detail.len() < 30 {
                        detail.push((r.time, r.rtype, po, u, *pe));
                    }
                }
            }
        }
    }
    let mut v: Vec<((u32, usize), usize)> = buckets.into_iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    println!("(type,off) 聚类 top 15:");
    for ((rt, po), c) in v.iter().take(15) {
        println!("  type={} off_in_payload={} hits={}", rt, po, c);
    }
    println!("\n明细前 15:");
    for (t, rt, po, u, pe) in detail.iter().take(15) {
        let c = u >> 6;
        let pitch = c as f64 * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
        println!("  t={:8.3} type={} off={:3} u16={:5} coarse={:4} → {:+.2}° (WI {:+.2}°)",
            t, rt, po, u, c, pitch * 57.29578, pe * 57.29578);
    }
}
