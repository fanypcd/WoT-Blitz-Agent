//! 主视角坦克验证：作者坦克 prop2.frac 是否也是炮管俯仰（6 位量化）。
//！ 联合时间线：prop2 (t, coarse, frac) × prop9 (t, 瞄准角°) × clamp(prop9, T110E5 −8°/+15°)。
//! 判据：慢速瞄准段 frac ≈ 量化后的 clamp(prop9)；快速甩瞄段 frac 滞留（炮管跟不上）；
//! 瞄准超出极限并保持时（39s 后 aim=−16.5°）炮管应钳在 −8° → 若有包则 frac 应钉 0。
//! 用法：cargo run --release --example pitch_reprobe_authorjoint -- <file.wotbreplay>
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    const AUTHOR: u32 = 0x0813ce11;
    const AVATAR: u32 = 0x0815bc12;
    // T110E5: 俯角 8° / 仰角 15°；frac 假设 0↔−8°、63↔+15°（23°/63 级）
    const DEP: f32 = 8.0;
    const ELE: f32 = 15.0;

    let mut p2: Vec<(f32, u16)> = Vec::new();
    let mut p9: Vec<(f32, f32)> = Vec::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 && u32le(&p[0..4]) == AUTHOR {
                    p2.push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                }
                if p.len() >= 16 && u32le(&p[4..8]) == 9 && u32le(&p[8..12]) == 4 && u32le(&p[0..4]) == AVATAR {
                    p9.push((pkt.clock_secs, f32le(&p[12..16])));
                }
            }
            _ => {}
        }
    }
    println!("作者坦克 prop2: {} 包 | avatar prop9: {} 包", p2.len(), p9.len());
    // prop9 值域（换算度）
    let (mn, mx) = p9.iter().map(|(_, v)| v * 57.29578).fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    println!("prop9 值域: {:+.2}° .. {:+.2}°（T110E5 极限 {:-.0}°/{:+.0}° → prop9 为瞄准角非炮管角）", mn, mx, -DEP, ELE);

    let aim_at = |t: f32| -> f32 {
        let mut v = f32::NAN;
        for (tt, x) in &p9 { if *tt <= t + 0.05 { v = x * 57.29578; } else { break; } }
        v
    };
    let frac_to_pitch = |fr: u16| -> f32 { -DEP + (fr as f32 / 63.0) * (DEP + ELE) };

    // 联合打印：每 0.25s 一行 + 每个 prop2 变化行
    println!("\n{:>8} {:>18} {:>18} {:>10}", "t", "aim(prop9)", "clamp(aim)", "frac→pitch");
    let mut last_frac: Option<u16> = None;
    let mut li = 0;
    let mut next_t = 0.0f32;
    while next_t <= 78.0 {
        // 找该时刻最近 prop2
        while li < p2.len() && p2[li].0 <= next_t { li += 1; }
        let cur = if li > 0 { Some(p2[li - 1]) } else { None };
        let aim = aim_at(next_t);
        let clamped = aim.clamp(-DEP, ELE);
        if let Some((pt, u)) = cur {
            let fr = u & 63;
            let changed = last_frac.map(|l| l != fr).unwrap_or(true);
            // 打印条件：frac 变化、或每 2s 采样
            if changed || (next_t * 4.0) as i32 % 8 == 0 {
                println!("{:8.3}{:>13}{:>13}{:>15}   (t_p2={:.3} coarse={})",
                    next_t,
                    format!("{:+7.2}°", aim),
                    format!("{:+7.2}°", clamped),
                    format!("{:+7.2}°", frac_to_pitch(fr)),
                    pt, u >> 6);
            }
            last_frac = Some(fr);
        } else if !aim.is_nan() {
            println!("{:8.3}{:>13}{:>13}{:>15}   (无prop2)", next_t,
                format!("{:+7.2}°", aim), format!("{:+7.2}°", clamped), "-");
        }
        next_t += 0.25;
    }
}
