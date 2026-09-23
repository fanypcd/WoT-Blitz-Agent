//! 敌车俯仰重分析 · 第五步：收尾细节。
//! 1) 敌车 tank_id/昵称确认；2) 敌车首包时刻（AoI 进入）；3) 窗口内 4 个 type=8 方法包解码；
//! 4) prop2 frac6 位时间线 vs 动作阶段（振荡段 45~58 / 旋转段 58.1~66.5 / 极限段 66.5~74）的
//!    统计特征（每段 frac 活跃度 = 每秒变化次数、钳位 0/63 占比）。
//! 用法：cargo run --release --example pitch_reprobe_final -- <file.wotbreplay>
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let br = replay.read_battle_results().ok();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    const ENEMY: u32 = 0x0813ce12;
    const AVATAR: u32 = 0x0815bc12;

    if let Some(br) = &br {
        for p in &br.players {
            let tank = br.player_results.iter().find(|pr| pr.info.account_id == p.account_id)
                .map(|pr| pr.info.tank_id).unwrap_or(0);
            let team = if p.info.team == 1 { 1 } else { 2 };
            println!("player: {:>16} team={} tank_id={}", p.info.nickname, team, tank);
        }
    }

    // 敌车首包
    let mut first_enemy = f32::MAX;
    let mut methods: Vec<(f32, u32, usize)> = Vec::new();
    for pkt in &data.packets {
        let (ty, p) = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => (8u32, &pkt.raw_payload[..]),
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => (*packet_type, &pkt.raw_payload[..]),
            _ => (0xffffffff, &pkt.raw_payload[..]),
        };
        if p.len() >= 4 && u32le(&p[0..4]) == ENEMY {
            first_enemy = first_enemy.min(pkt.clock_secs);
        }
        if ty == 8 && pkt.clock_secs >= 43.0 && pkt.clock_secs <= 76.0 {
            let method = if p.len() >= 8 { u32le(&p[4..8]) } else { 0 };
            methods.push((pkt.clock_secs, method, p.len()));
        }
    }
    println!("\n敌车 0x{:08x} 首包时刻: {:.3}s", ENEMY, first_enemy);
    println!("窗口内 type=8 方法包（eid@0, method@4）：");
    for (t, m, len) in &methods { println!("  t={:.3} method=0x{:02x} len={}", t, m, len); }

    // prop2 frac 阶段统计
    let mut p2: Vec<(f32, u16)> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 && u32le(&p[0..4]) == ENEMY && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 {
                p2.push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
            }
        }
    }
    let phases = [("P0 初摆", 43.0f32, 44.5f32), ("P1 振荡", 45.0, 58.0), ("P2 旋转", 58.1, 66.5), ("P3 极限", 66.5, 74.0)];
    println!("\nprop2 frac6 分段统计：");
    for (name, t0, t1) in phases {
        let seg: Vec<(f32, u16)> = p2.iter().filter(|(t, _)| *t >= t0 && *t < t1).cloned().collect();
        if seg.is_empty() { println!("  {}: 无包", name); continue; }
        let dur = t1 - t0;
        let mut frac_changes = 0;
        let mut coarse_changes = 0;
        let mut pin0 = 0; let mut pin63 = 0;
        let mut both_change = 0;
        for i in 1..seg.len() {
            let (f_a, c_a) = (seg[i - 1].1 & 63, seg[i - 1].1 >> 6);
            let (f_b, c_b) = (seg[i].1 & 63, seg[i].1 >> 6);
            if f_a != f_b { frac_changes += 1; }
            if c_a != c_b { coarse_changes += 1; }
            if f_a != f_b && c_a != c_b { both_change += 1; }
        }
        pin0 = seg.iter().filter(|(_, u)| (u & 63) == 0).count();
        pin63 = seg.iter().filter(|(_, u)| (u & 63) == 63).count();
        // 步长是否 64 整数倍（纯粗位运动）
        let mut step64 = 0; let mut total_steps = 0;
        for i in 1..seg.len() {
            let d = seg[i].1 as i32 - seg[i - 1].1 as i32;
            if d != 0 { total_steps += 1; if d % 64 == 0 { step64 += 1; } }
        }
        println!("  {} ({}s, {}包): frac变化{}/s coarse变化{}/s 双变{} 钳0:{}包 钳63:{}包 步长64整倍率 {}/{}",
            name, dur, seg.len(),
            (frac_changes as f32 / dur * 10.0).round() / 10.0, (coarse_changes as f32 / dur * 10.0).round() / 10.0,
            both_change, pin0, pin63, step64, total_steps);
    }

    // frac 时间线（P1+P3 段，看钳位与循环）
    println!("\nfrac 时间线（45~74s，每包）：");
    for (t, u) in &p2 {
        if *t >= 45.0 && *t <= 74.0 {
            let f = u & 63;
            let mark = if f == 0 { " <0>" } else if f == 63 { " <63>" } else { "" };
            println!("  {:7.3} coarse={:>4} frac={:>2}{}", t, u >> 6, f, mark);
        }
    }
}
