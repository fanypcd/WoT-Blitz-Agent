//! 敌车俯仰重分析 · 第三步：实验窗（43~76s）全类型全字段扫描。
//! 1) 窗口内 (type, len) 直方图；2) 哪些包首 4B = 敌车 eid / avatar eid；
//! 3) 对同 (type,len) 组的每个 u16/f32 偏移，检测 45~58s 振荡特征（多周期往复）
//!    与 66~74s 极限特征（触顶后回弹）。
//! 用法：cargo run --release --example pitch_reprobe_scan -- <file.wotbreplay>
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    const ENEMY: u32 = 0x0813ce12;
    const AVATAR: u32 = 0x0815bc12;
    const T0: f32 = 43.0;
    const T1: f32 = 76.0;

    // (type, len) → (count, count_with_enemy_eid, count_with_avatar_eid)
    let mut groups: std::collections::BTreeMap<(u32, usize), [usize; 2]> = std::collections::BTreeMap::new();
    for pkt in &data.packets {
        let ty = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        if pkt.clock_secs < T0 || pkt.clock_secs > T1 { continue; }
        let p = &pkt.raw_payload[..];
        let e = if p.len() >= 4 { u32le(&p[0..4]) } else { u32::MAX };
        let g = groups.entry((ty, p.len())).or_insert([0, 0]);
        g[0] += (e == ENEMY) as usize;
        g[1] += (e == AVATAR) as usize;
    }
    println!("=== 窗口 {}~{}s 内 (type, len) 直方图 [n_enemy, n_avatar] ===", T0, T1);
    for ((ty, len), c) in &groups {
        let mark = if c[0] > 0 || c[1] > 0 { format!("  <-- eid命中 敌:{} avatar:{}", c[0], c[1]) } else { String::new() };
        println!("  type={:<3} len={:<4} {}", ty, len, mark);
    }

    // —— 振荡/极限检测：对窗口内每个 (type,len) 组、每个偏移的 u16 与 f32 解读 ——
    // 特征 A（振荡段 45~58s）：符号变化次数 >= 6 的往复（一阶差分变号）
    // 特征 B（极限段 66~74s）：值在窗内取到全局 max 或 min 且远离中值
    println!("\n=== 字段扫描（候选：振荡 A / 极限 B）===");
    let mut series: std::collections::BTreeMap<(u32, usize, usize, u8), Vec<(f32, f64)>> = std::collections::BTreeMap::new();
    for pkt in &data.packets {
        let ty = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        let clock = pkt.clock_secs;
        if clock < T0 || clock > T1 { continue; }
        let p = &pkt.raw_payload[..];
        if p.len() < 2 { continue; }
        for off in 0..=p.len().saturating_sub(2) {
            let u = u16::from_le_bytes([p[off], p[off + 1]]) as f64;
            series.entry((ty, p.len(), off, 0u8)).or_default().push((clock, u));
        }
        for off in 0..=p.len().saturating_sub(4) {
            let v = f32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]) as f64;
            if v.is_finite() && v.abs() < 1e6 { series.entry((ty, p.len(), off, 1u8)).or_default().push((clock, v)); }
        }
    }
    let mut candidates = 0;
    for ((ty, len, off, kind), vals) in &series {
        if vals.len() < 20 { continue; }
        // 特征 A：45~58s 内差分变号次数
        let win: Vec<(f32, f64)> = vals.iter().filter(|(t, _)| *t >= 45.0 && *t <= 58.0).cloned().collect();
        if win.len() < 10 { continue; }
        let mut sign_changes = 0;
        let mut last_d: f64 = 0.0;
        for w in win.windows(2) {
            let d = w[1].1 - w[0].1;
            if d != 0.0 {
                if last_d != 0.0 && d.signum() != last_d.signum() { sign_changes += 1; }
                last_d = d;
            }
        }
        let span_a = win.iter().map(|(_, v)| *v).fold(f64::MIN, f64::max) - win.iter().map(|(_, v)| *v).fold(f64::MAX, f64::min);
        // 特征 B：66~74s 触及全窗极值
        let (gmin, gmax) = vals.iter().map(|(_, v)| *v).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
        let win_b: Vec<(f32, f64)> = vals.iter().filter(|(t, _)| *t >= 66.0 && *t <= 74.0).cloned().collect();
        let b_min = win_b.iter().map(|(_, v)| *v).fold(f64::MAX, f64::min);
        let b_max = win_b.iter().map(|(_, v)| *v).fold(f64::MIN, f64::max);
        let hits_extreme = b_min <= gmin + 1e-9 || b_max >= gmax - 1e-9;
        let osc = sign_changes >= 8 && span_a > 0.0;
        if osc || hits_extreme {
            let kind_s = if *kind == 0 { "u16" } else { "f32" };
            println!("  type={} len={} off={} {}  n={} A:变号{}跨度{:.4} B:{} min={:.4} max={:.4}",
                ty, len, off, kind_s, vals.len(), sign_changes, span_a, if hits_extreme { "Y" } else { "-" }, gmin, gmax);
            candidates += 1;
            if candidates > 60 { println!("  ...（截断）"); break; }
        }
    }
    if candidates == 0 { println!("  （无候选字段）"); }
}
