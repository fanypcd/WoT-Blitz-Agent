//! 敌车俯仰重分析 · 第六步：用作者自己的坦克做 frac 位语义判别。
//! 作者坦克 0x0813ce11 的 prop2（0~43s）对照作者 avatar 0x0815bc12 的 prop9 俯仰真值：
//! 若 prop2.frac 变化时刻与 prop9 俯仰速度相关（而与 coarse 变化率无关），
//! 则 frac = 俯仰相关分量（打包角理论成立）；否则 frac 只是偏航细位。
//! 用法：cargo run --release --example pitch_reprobe_author -- <file.wotbreplay>
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    const AUTHOR: u32 = 0x0813ce11;   // 作者坦克（车辆实体）
    const AVATAR: u32 = 0x0815bc12;   // 作者 avatar（prop9 宿主）

    let mut p2: Vec<(f32, u16)> = Vec::new();
    let mut p9: Vec<(f32, f32)> = Vec::new();
    let mut t10_yaw: Vec<(f32, f32)> = Vec::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 {
                    if u32le(&p[0..4]) == AUTHOR { p2.push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]]))); }
                }
                if p.len() >= 16 && u32le(&p[4..8]) == 9 && u32le(&p[8..12]) == 4 {
                    if u32le(&p[0..4]) == AVATAR { p9.push((pkt.clock_secs, f32le(&p[12..16]))); }
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 && u32le(&p[0..4]) == AUTHOR {
                    t10_yaw.push((pkt.clock_secs, f32le(&p[36..40])));
                }
            }
            _ => {}
        }
    }
    println!("作者坦克 prop2: {} 包（0~43s），avatar prop9: {} 包，t10: {}", p2.len(), p9.len(), t10_yaw.len());

    // 车体偏航（t10 yaw）用于把 prop2 coarse 换算成炮塔相对角
    let hull_yaw_at = |t: f32| -> f32 {
        let mut v = 0.0;
        for (tt, y) in &t10_yaw { if *tt <= t { v = *y; } else { break; } }
        v
    };
    let pitch_at = |t: f32| -> Option<f32> {
        p9.iter().rev().find(|(tt, _)| *tt <= t + 0.05).map(|(_, v)| *v)
    };

    println!("\n{:>8} {:>6} {:>6} {:>5} {:>9} {:>9} {:>8}", "t", "coarse", "frac", "turret", "pitch9°", "Δpitch°/s", "Δfrac");
    let mut prev: Option<(f32, u16)> = None;
    for (t, u) in &p2 {
        let coarse = u >> 6;
        let frac = u & 63;
        // 炮塔绝对偏航 = coarse/1024*360-180 + 车体偏航
        let turret_abs = (*u as f32) / 65535.0 * 360.0 - 180.0 + hull_yaw_at(*t) * 57.29578;
        let pitch = pitch_at(*t).map(|v| v * 57.29578);
        let (dp, df) = match (prev, pitch) {
            (Some((pt, pu)), Some(pnow)) => {
                let dt = (t - pt).max(0.001);
                let pitch_then = pitch_at(pt).map(|v| v * 57.29578).unwrap_or(pnow);
                (((pnow - pitch_then) / dt), (*u as i32 - pu as i32))
            }
            _ => (f32::NAN, 0),
        };
        if let Some(pp) = prev {
            if (u >> 6) != (pp.1 >> 6) || (u & 63) != (pp.1 & 63) {
                println!("{:8.3} {:>6} {:>4}{} {:>5} {:>9.2} {:>+9.1} {:>+8}",
                    t, coarse, frac, if frac == 0 || frac == 63 { "*" } else { " " },
                    format!("{:+.1}", turret_abs),
                    pitch.unwrap_or(f32::NAN), dp, df);
            }
        }
        prev = Some((*t, *u));
    }

    // 相关性统计：|Δfrac| 与 |Δpitch|、|Δcoarse| 的秩相关（粗略：分段求和对比）
    let mut bins: Vec<(f32, f32, f32)> = vec![(0.0, 0.0, 0.0); 8]; // (Σ|Δfrac|, Σ|Δpitch|, Σ|Δcoarse|) per 5s
    let mut prev2: Option<(f32, u16)> = None;
    for (t, u) in &p2 {
        if let Some((pt, pu)) = prev2 {
            let b = ((t / 5.0) as usize).min(7);
            bins[b].0 += (u & 63) as f32 - 0.0; // 占位，下面重算
            bins[b].0 -= bins[b].0; // noop
            let dfrac = ((u & 63) as i32 - (pu & 63) as i32).abs() as f32;
            let dcoarse = ((u >> 6) as i32 - (pu >> 6) as i32).abs() as f32;
            let dpitch = match (pitch_at(*t), pitch_at(pt)) {
                (Some(a), Some(b)) => ((a - b) * 57.29578).abs(),
                _ => 0.0,
            };
            bins[b].0 += dfrac; bins[b].1 += dpitch; bins[b].2 += dcoarse;
        }
        prev2 = Some((*t, *u));
    }
    println!("\n{:>8} {:>9} {:>10} {:>10}", "bin(s)", "Σ|Δfrac|", "Σ|Δpitch|°", "Σ|Δcoarse|");
    for (i, b) in bins.iter().enumerate() {
        println!("  {:>3}-{:>3} {:>9.0} {:>10.2} {:>10.0}", i * 5, i * 5 + 5, b.0, b.1, b.2);
    }
}
