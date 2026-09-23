//! 按实体统计 prop2 俯仰解码与 WI gun_pitch 的匹配分布
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let wi_text = std::fs::read_to_string("tmp_wi2/wi_shots.json").unwrap();
    let wi: serde_json::Value = serde_json::from_str(&wi_text).unwrap();
    let mut p2: HashMap<u32, Vec<(f32, u16)>> = HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 && u32le(&p[4..8]) == 2 {
                let alen = u32le(&p[8..12]) as usize;
                if alen == 2 && 12 + 2 <= p.len() {
                    p2.entry(u32le(&p[0..4])).or_default().push((
                        pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                }
            }
        }
    }
    let decode = |u: u16| -> f64 {
        let coarse = (u >> 6) as f64;
        let mut p = coarse * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
        while p > std::f64::consts::PI { p -= std::f64::consts::TAU; }
        while p < -std::f64::consts::PI { p += std::f64::consts::TAU; }
        p * 57.29578
    };
    // eid → 匹配数（|差|<1.5°）
    let mut hits_per_eid: HashMap<u32, usize> = HashMap::new();
    let mut n_total = 0; let mut n_close = 0;
    let mut rows: Vec<(f32, u32, f64, f64)> = Vec::new();
    for s in wi.as_array().unwrap() {
        let t = s["time"].as_f64().unwrap() as f32;
        let gp = s["gun_pitch"].as_f64().unwrap() * 57.29578;
        n_total += 1;
        let mut best = f64::MAX; let mut best_e = 0u32;
        for (e, seq) in &p2 {
            for (pt, u) in seq {
                if (*pt - t).abs() <= 0.15 {
                    let d = (decode(*u) - gp).abs();
                    if d < best { best = d; best_e = *e; }
                }
            }
        }
        if best < 1.5 {
            n_close += 1;
            *hits_per_eid.entry(best_e).or_insert(0) += 1;
            rows.push((t, best_e, gp, best));
        }
    }
    println!("命中 {}/99。按实体分布:", n_close);
    let mut v: Vec<(u32, usize)> = hits_per_eid.into_iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    for (e, c) in v { println!("  0x{:08x}: {} 发", e, c); }
    println!("\n命中明细（前 20）:");
    for (t, e, gp, d) in rows.iter().take(20) {
        println!("  t={:8.3} eid=0x{:08x} WI={:+7.2}° |Δ|={:.2}°", t, e, gp, d);
    }
}
