//! 敌车俯仰重分析 · 第四步：按实体分组的字段扫描 + t10 额外字节 + type=39 结构。
//! 1) 窗口内每个 (eid, type, len) 组：每个偏移 u16/f32 时间序列的振荡/极值检测
//! 2) 敌车 t10 全 49 字节逐字节值域对照（找实验期变化的字节）
//! 3) type=39 eid 分布与前几个包 hex
//! 用法：cargo run --release --example pitch_reprobe_eidscan -- <file.wotbreplay>
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    const ENEMY: u32 = 0x0813ce12;
    const T0: f32 = 43.0;
    const T1: f32 = 76.0;

    // —— 1) 按实体分组的 (eid,type,len) → 每偏移序列 ——
    let mut series: std::collections::BTreeMap<(u32, u32, usize, usize, u8), Vec<(f32, f64)>> = std::collections::BTreeMap::new();
    let mut t39_eids: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    let mut t39_samples: Vec<(f32, Vec<u8>)> = Vec::new();
    for pkt in &data.packets {
        let ty = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        let clock = pkt.clock_secs;
        let p = &pkt.raw_payload[..];
        if ty == 39 {
            if p.len() >= 4 { *t39_eids.entry(u32le(&p[0..4])).or_insert(0) += 1; }
            if t39_samples.len() < 4 && clock >= 45.0 && clock <= 46.0 { t39_samples.push((clock, p.to_vec())); }
        }
        if clock < T0 || clock > T1 || p.len() < 2 { continue; }
        let eid = u32le(&p[0..4]);
        for off in 0..=p.len().saturating_sub(2) {
            let u = u16::from_le_bytes([p[off], p[off + 1]]) as f64;
            series.entry((eid, ty, p.len(), off, 0u8)).or_default().push((clock, u));
        }
        for off in 0..=p.len().saturating_sub(4) {
            let v = f32::from_le_bytes([p[off], p[off + 1], p[off + 2], p[off + 3]]) as f64;
            if v.is_finite() && v.abs() < 1e6 { series.entry((eid, ty, p.len(), off, 1u8)).or_default().push((clock, v)); }
        }
    }

    println!("=== 按实体字段扫描（45~58s 变号≥6 或 全窗极值在 66~74s）===");
    for ((eid, ty, len, off, kind), vals) in &series {
        if vals.len() < 15 { continue; }
        let win_a: Vec<f64> = vals.iter().filter(|(t, _)| *t >= 45.0 && *t <= 58.0).map(|(_, v)| *v).collect();
        if win_a.len() < 8 { continue; }
        let mut sign_changes = 0;
        let mut last_d = 0.0f64;
        for i in 1..win_a.len() {
            let d = win_a[i] - win_a[i - 1];
            if d != 0.0 {
                if last_d != 0.0 && d.signum() != last_d.signum() { sign_changes += 1; }
                last_d = d;
            }
        }
        let (mn, mx) = vals.iter().map(|(_, v)| *v).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
        let win_b: Vec<f64> = vals.iter().filter(|(t, _)| *t >= 66.0 && *t <= 74.0).map(|(_, v)| *v).collect();
        let b_ext = win_b.iter().fold(f64::MAX, |a, v| a.min(*v)) <= mn + 1e-9
            || win_b.iter().fold(f64::MIN, |a, v| a.max(*v)) >= mx - 1e-9;
        // 静态字段跳过（全窗恒定）
        if mx - mn == 0.0 { continue; }
        if sign_changes >= 6 || b_ext {
            println!("  eid=0x{:08x} type={} len={} off={} {} n={} A变号={} range=[{:.4},{:.4}] B极值={}",
                eid, ty, len, off, if *kind == 0 { "u16" } else { "f32" }, vals.len(), sign_changes, mn, mx, if b_ext { "Y" } else { "-" });
        }
    }

    // —— 2) 敌车 t10 逐字节值域：分实验前(37~43) / 前段(45~58) / 旋转(58~66.5) / 后段(66.5~74) ——
    println!("\n=== 敌车 t10(len=49) 逐字节分段值域 ===");
    let phases = [("pre", 37.0f32, 43.0f32), ("osc", 45.0, 58.0), ("rot", 58.1, 66.5), ("post", 66.5, 74.0)];
    let mut per_phase: Vec<Vec<(u8, u8)>> = vec![Vec::new(); 4]; // phase -> [(min,max)] per byte
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 49 || u32le(&p[0..4]) != ENEMY { continue; }
            for (pi, (_, t0, t1)) in phases.iter().enumerate() {
                if pkt.clock_secs >= *t0 && pkt.clock_secs < *t1 {
                    while per_phase[pi].len() < 49 { per_phase[pi].push((255, 0)); }
                    for b in 0..49 {
                        let e = &mut per_phase[pi][b];
                        e.0 = e.0.min(p[b]); e.1 = e.1.max(p[b]);
                    }
                }
            }
        }
    }
    println!("  byte:   {:>7} {:>7} {:>7} {:>7}", "pre", "osc", "rot", "post");
    for b in 0..49 {
        let rng = |v: &(u8, u8)| -> String {
            if v.1 == 0 { format!("{}", v.0) } else { format!("{}-{}", v.0, v.1) }
        };
        let g = |pi: usize| -> String {
            match per_phase[pi].get(b) { Some(v) => rng(v), None => "-".into() }
        };
        println!("  [{:>2}] {:>7} {:>7} {:>7} {:>7}", b, g(0), g(1), g(2), g(3));
    }

    // —— 3) type=39 ——
    println!("\n=== type=39 eid 分布（前 10）===");
    for (eid, n) in t39_eids.iter().take(10) { println!("  0x{:08x}: {}", eid, n); }
    println!("  （共 {} 个不同 eid，样例包：）", t39_eids.len());
    for (t, p) in &t39_samples {
        println!("  t={:.3} len={} hex={}", t, p.len(), p.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
    }
}
