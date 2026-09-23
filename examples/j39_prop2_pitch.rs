//! 决定性检验：J39 受击方 prop2（弹着时刻最近样本）用俯仰公式
//! pitch = (u>>6)×0.7° − 360°（wrap ±180°）解码，对照 WI gun_pitch 99 发真值。
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // WI 99 发: time, target(account), gun_pitch
    let wi_text = std::fs::read_to_string("tmp_wi2/wi_shots.json").unwrap();
    let wi: serde_json::Value = serde_json::from_str(&wi_text).unwrap();
    // prop2: eid → 时序
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
    // target account → vehicle eid 映射: WI target 是 account id。J39 玩家 account↔entity
    // 关系未知，但 14 辆车的 prop2 都在。策略：对每发 WI shot，在【全部实体】的 prop2 中
    // 找 time ±0.15s 内俯仰解码 == WI gun_pitch (±1°) 的命中（并统计随机符合率）。
    let decode = |u: u16| -> f64 {
        let coarse = (u >> 6) as f64;
        let mut p = coarse * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
        while p > std::f64::consts::PI { p -= std::f64::consts::TAU; }
        while p < -std::f64::consts::PI { p += std::f64::consts::TAU; }
        p * 57.29578
    };
    let mut n_total = 0; let mut n_close = 0; let mut n_exact = 0;
    for s in wi.as_array().unwrap() {
        let t = s["time"].as_f64().unwrap() as f32;
        let gp = s["gun_pitch"].as_f64().unwrap() * 57.29578;
        n_total += 1;
        let mut best = f64::MAX;
        for (_e, seq) in &p2 {
            for (pt, u) in seq {
                if (*pt - t).abs() <= 0.15 {
                    let d = (decode(*u) - gp).abs();
                    if d < best { best = d; }
                }
            }
        }
        if best < 1.5 { n_close += 1; }
        if best < 0.4 { n_exact += 1; }
        if n_total <= 12 {
            println!("t={:8.3} WI={:+7.2}°  最近 prop2 解码差={:6.2}°", t, gp, best);
        }
    }
    println!("\n99 发: |差|<1.5° = {} 发, |差|<0.4° = {} 发", n_close, n_exact);
    println!("（若 prop2=俯仰通道，应接近 99/99 全中且差<0.7°=半级）");
}
