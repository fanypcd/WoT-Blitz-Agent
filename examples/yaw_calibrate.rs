//! 炮塔偏航公式定标：1617 整圈旋转段精确测量 + J39 WI turret_yaw 86 发配对拟合。
//! 1) 1617：旋转段（58~66.6s）前后的静止点 coarse、单调扫过级数、按不同 counts/rev
//!    假设换算的旋转角；
//! 2) J39：method8 配对 victim 的 prop2 coarse @ 通知时刻 vs WI turret_yaw，
//!    在 counts/rev ∈ {1000..1030} 网格上找误差中位数最优的刻度，并与 1024 对照。
//! 用法：cargo run --release --example yaw_calibrate -- <1617.wotbreplay> <j39.wotbreplay> <wi_shots.json>
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let p1617 = &args[1];
    let pj39 = &args[2];
    let wi_path = &args[3];

    // ---------- 1) 1617 旋转段 ----------
    let f = std::fs::File::open(p1617).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    const ENEMY: u32 = 0x0813ce12;
    let mut p2: Vec<(f32, u16)> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 && u32le(&p[0..4]) == ENEMY {
                p2.push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
            }
        }
    }
    // 旋转段 = 58.0~66.6s 的连续下降；起止取段外最近静止值
    let at = |t: f32| -> u16 { *p2.iter().min_by_key(|(c, _)| ((c - t).abs() * 1000.0) as u32).map(|(_, u)| u).unwrap() };
    let start_u = at(57.90);   // 旋转前最后静止（58.09 前一拍）
    let end_u = at(66.65);     // 旋转后第一个回稳点
    let (sc, ec) = (start_u >> 6, end_u >> 6);
    let swept_mod = |n: i64| -> i64 { ((sc as i64 - ec as i64).rem_euclid(n)) };
    println!("=== 1617 整圈旋转定标 ===");
    println!("起点 u16={:#06x} coarse={} (t≈57.9)，终点 u16={:#06x} coarse={} (t≈66.65)", start_u, sc, end_u, ec);
    for n in [1024i64, 1018, 1012, 1008, 1006, 1004, 1002, 1000] {
        let s = swept_mod(n);
        println!("  counts/rev={:<5} → 旋转角 = {:>7.2}°（扫过 {} 级）", n, s as f64 * 360.0 / n as f64, s);
    }
    // 单调段内极值（校验 sweep 与极值一致）
    let seg: Vec<(f32, u16)> = p2.iter().filter(|(c, _)| *c >= 58.0 && *c <= 66.7).cloned().collect();
    let mn = seg.iter().map(|(_, u)| u >> 6).min().unwrap();
    let mx = seg.iter().map(|(_, u)| u >> 6).max().unwrap();
    println!("段内 coarse 极值: {} .. {}（wrap 处）", mn, mx);

    // ---------- 2) J39 WI turret_yaw 拟合 ----------
    let f2 = std::fs::File::open(pj39).unwrap();
    let mut rep2 = wotbreplay_parser::replay::Replay::open(f2).unwrap();
    let data2 = rep2.read_data().unwrap();
    let mut p2j: std::collections::HashMap<u32, Vec<(f32, u16)>> = std::collections::HashMap::new();
    let mut m8: Vec<(f32, u32, u32)> = Vec::new();
    for pkt in &data2.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 {
                    p2j.entry(u32le(&p[0..4])).or_default().push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 20 && u32le(&p[4..8]) == 0x08 {
                    let alen = u32le(&p[8..12]) as usize;
                    if alen >= 8 && 12 + 8 <= p.len() {
                        m8.push((pkt.clock_secs, u32le(&p[12..16]), u32le(&p[16..20])));
                    }
                }
            }
            _ => {}
        }
    }
    let wi: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(wi_path).unwrap()).unwrap();
    println!("\n=== J39 WI turret_yaw 刻度拟合（victim 配对，coarse 解码）===");
    let mut best = (f64::MAX, 0i64);
    for n in 990..=1040i64 {
        let mut errs: Vec<f64> = Vec::new();
        for s in wi.as_array().unwrap() {
            let t = s["time"].as_f64().unwrap() as f32;
            let ty = s["turret_yaw"].as_f64().unwrap() as f64;
            // WI 的 target 字段 = 车辆 entity id（tmp_wi2/verify_yaw.py 原验证即此用法）
            let veid = s["target"].as_u64().unwrap() as u32;
            let Some(seq) = p2j.get(&veid) else { continue };
            let Some((_, u)) = seq.iter().filter(|(c, _)| *c <= t + 0.1)
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap()) else { continue };
            let yaw = (u >> 6) as f64 / n as f64 * std::f64::consts::TAU - std::f64::consts::PI;
            let mut d = (yaw - ty).abs();
            while d > std::f64::consts::PI { d = std::f64::consts::TAU - d; }
            errs.push(d);
        }
        if errs.len() < 10 { continue; }
        errs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = errs[errs.len() / 2];
        if med < best.0 { best = (med, n); }
        if n == 990 || n == 1004 || n == 1010 || n == 1016 || n == 1020 || n == 1024 || n == 1030 || n == 1040 {
            println!("  counts/rev={:<4} n={:<3} 误差中位 {:>7.3}° = {:>6.4} rad（≈{:.1} 级@1024）",
                n, errs.len(), med * 57.29578, med, med / (std::f64::consts::TAU / 1024.0));
        }
    }
    if best.1 > 0 {
        println!("最优刻度: counts/rev={}（误差中位 {:.4} rad = {:.3}°）", best.1, best.0, best.0 * 57.29578);
    } else {
        println!("配对样本不足");
    }
}
