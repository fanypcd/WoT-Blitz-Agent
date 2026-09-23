//! 决定性重测：type=10 [40..44] "pitch" 字段到底是不是炮管俯仰？
//! 对照 J39 WI 99 发 gun_pitch 真值（该发 target 的 type10 pitch）。
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // WI shots: time + target(account) + gun_pitch。account→vehicle eid 映射:
    // J39 shooter=作者 Anonyme, target=account id。击中时受击方 = target。
    // account_id 与 entity id 不同——但我们只需【pitch 值分布对照】:
    // 收集全部 type10 pitch 值时序 + WI gun_pitch 值时序，看值域/变化模式是否同源。
    let mut pitch_series: Vec<(f32, u32, f32)> = Vec::new(); // t, eid, pitch
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 48 {
                pitch_series.push((pkt.clock_secs, u32le(&p[0..4]), f32le(&p[40..44])));
            }
        }
    }
    let wi_text = std::fs::read_to_string("tmp_wi2/wi_shots.json").unwrap();
    let wi: serde_json::Value = serde_json::from_str(&wi_text).unwrap();
    println!("type10 pitch 值域 vs WI gun_pitch 值域:");
    let mut t10min = f32::MAX; let mut t10max = f32::MIN;
    for (_, _, p) in &pitch_series {
        if *p < t10min { t10min = *p; }
        if *p > t10max { t10max = *p; }
    }
    let wimin = wi.as_array().unwrap().iter().map(|s| s["gun_pitch"].as_f64().unwrap() as f32)
        .fold(f32::MAX, f32::min);
    let wimax = wi.as_array().unwrap().iter().map(|s| s["gun_pitch"].as_f64().unwrap() as f32)
        .fold(f32::MIN, f32::max);
    println!("  type10 pitch: {:+.3} ~ {:+.3} rad ({:+.1}° ~ {:+.1}°)", t10min, t10max, t10min*57.3, t10max*57.3);
    println!("  WI gun_pitch: {:+.3} ~ {:+.3} rad ({:+.1}° ~ {:+.1}°)", wimin, wimax, wimin*57.3, wimax*57.3);
    // 时序对照：每个 WI shot time 前 0.15s 内的 type10 pitch（全部实体），找接近 gun_pitch 的
    println!("\nWI shot 时刻的 type10 pitch 候选（|Δ|<0.03 rad 的数量）:");
    let mut n_close = 0; let mut n_total = 0;
    for s in wi.as_array().unwrap() {
        let t = s["time"].as_f64().unwrap() as f32;
        let gp = s["gun_pitch"].as_f64().unwrap() as f32;
        let mut best = f32::MAX;
        for (pt, _e, p) in &pitch_series {
            if (*pt - t).abs() <= 0.15 {
                let d = (*p - gp).abs();
                if d < best { best = d; }
            }
        }
        n_total += 1;
        if best < 0.03 { n_close += 1; }
        if n_total <= 10 {
            println!("  t={:8.3} WI={:+.3} rad, 最近 type10 pitch 差={:.3} rad", t, gp, best);
        }
    }
    println!("99 发中 |差|<0.03 rad 的比例: {}/{}", n_close, n_total);
}
