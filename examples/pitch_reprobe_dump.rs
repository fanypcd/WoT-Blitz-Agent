//! 敌车俯仰重分析 · 第二步：目标敌车 0x0813ce12 全流转储。
//! prop2(2B) 逐包：raw u16 / hex / coarse10=u>>6 / frac6=u&63 / 双解码角；
//! t10 [40..44] 车体俯仰、[36..40] 车体偏航对照；作者 avatar prop9 对照（控制组）。
//! 用法：cargo run --release --example pitch_reprobe_dump -- <file.wotbreplay> [t0] [t1]
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let t0: f32 = std::env::args().nth(2).map(|s| s.parse().unwrap()).unwrap_or(0.0);
    let t1: f32 = std::env::args().nth(3).map(|s| s.parse().unwrap()).unwrap_or(9999.0);
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);

    const ENEMY: u32 = 0x0813ce12;
    const AVATAR: u32 = 0x0815bc12;

    let mut p2: Vec<(f32, u16)> = Vec::new();          // 敌车 prop2
    let mut t10: Vec<(f32, [f32; 3], [f32; 3])> = Vec::new(); // 敌车 (pos, yaw/pitch/roll)
    let mut p9: Vec<(f32, f32)> = Vec::new();          // 作者 avatar prop9（控制组）
    let mut t10_pos0: Vec<(f32, [f32; 3])> = Vec::new();

    for pkt in &data.packets {
        let clock = pkt.clock_secs;
        if clock < t0 || clock > t1 { continue; }
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() < 12 { continue; }
                let eid = u32le(&p[0..4]);
                let sub = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if sub == 2 && alen == 2 && 12 + 2 <= p.len() && eid == ENEMY {
                    p2.push((clock, u16le(&p[12..14])));
                }
                if sub == 9 && alen == 4 && 12 + 4 <= p.len() && eid == AVATAR {
                    p9.push((clock, f32le(&p[12..16])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    let eid = u32le(&p[0..4]);
                    if eid == ENEMY {
                        t10.push((clock,
                            [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])],
                            [f32le(&p[36..40]), f32le(&p[40..44]), f32le(&p[44..48])]));
                    }
                    if eid == AVATAR {
                        t10_pos0.push((clock, [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])]));
                    }
                }
            }
            _ => {}
        }
    }

    println!("敌车 prop2: {} 包 | t10: {} 包 | avatar prop9: {} 包", p2.len(), t10.len(), p9.len());

    // —— 敌车 prop2 逐包（粗/细位分解 + 双解码）——
    println!("\n=== 敌车 prop2 全序列 ===");
    println!("{:>8} {:>7} {:>8} {:>7} {:>6} {:>9} {:>10}", "t", "u16", "hex", "coarse", "frac", "yaw全u16°", "yaw粗10°");
    let mut last_coarse: Option<u16> = None;
    for (t, u) in &p2 {
        let coarse = u >> 6;
        let frac = u & 63;
        let yaw_full = *u as f64 / 65535.0 * 360.0 - 180.0;
        let yaw_coarse = coarse as f64 / 1023.0 * 360.0 - 180.0;
        let mark = if last_coarse != Some(coarse) { "*" } else { " " };
        println!("{:8.3} {:>7} 0x{:04x}{} {:>7} {:>6} {:>+9.2} {:>+10.2}", t, u, u, mark, coarse, frac, yaw_full, yaw_coarse);
        last_coarse = Some(coarse);
    }

    // —— 敌车 t10 姿态（车体 yaw/pitch/roll）——
    println!("\n=== 敌车 t10 姿态（每 0.5s 采样 + 变化点）===");
    let mut last: Option<[f32; 3]> = None;
    let mut last_t = f32::MIN;
    for (t, _, ang) in &t10 {
        let changed = last.map(|l| (l[0] - ang[0]).abs() > 0.005 || (l[1] - ang[1]).abs() > 0.005 || (l[2] - ang[2]).abs() > 0.005).unwrap_or(true);
        if changed || *t - last_t > 0.5 {
            println!("{:8.3}  yaw={:+8.2}° pitch={:+8.2}° roll={:+8.2}°", t,
                ang[0] * 57.29578, ang[1] * 57.29578, ang[2] * 57.29578);
            last_t = *t;
        }
        last = Some(*ang);
    }

    // —— prop9（作者自身俯仰，控制组：应基本静止）——
    println!("\n=== avatar prop9（作者俯仰控制组，每 2s）===");
    let mut last_print = f32::MIN;
    let (mut mn, mut mx) = (f32::MAX, f32::MIN);
    for (t, v) in &p9 {
        mn = mn.min(*v); mx = mx.max(*v);
        if *t - last_print >= 2.0 { println!("{:8.3}  {:+8.2}°", t, v * 57.29578); last_print = *t; }
    }
    println!("prop9 值域: {:+.2}° .. {:+.2}°", mn * 57.29578, mx * 57.29578);

    // —— 统计：prop2 粗位变化点（潜在 yaw 台阶）与纯细位变化点（潜在 pitch 振荡）——
    println!("\n=== prop2 变化分类 ===");
    let mut n_same = 0; let mut n_frac_only = 0; let mut n_coarse = 0;
    let mut prev: Option<u16> = None;
    for (_, u) in &p2 {
        if let Some(pv) = prev {
            if u == &pv { n_same += 1; }
            else if u >> 6 == pv >> 6 { n_frac_only += 1; }
            else { n_coarse += 1; }
        }
        prev = Some(*u);
    }
    println!("重复值: {} | 仅细位(frac6)变化: {} | 粗位(coarse10)变化: {}", n_same, n_frac_only, n_coarse);
}
