//! J39 frac 分布特征 + m0 开火事件识别射手的 WI 对照。
//! 1) 全 14 实体 prop2 frac 直方图与 0/63 钳位统计（对照训练房特征）
//! 2) type=8 method=0x00 开火事件（envelope=射手 eid）在 WI 时刻 ±0.15s 内识别射手
//! 3) 该射手实体射击时刻的 frac 解码 vs WI gun_pitch（含 hull pitch 参考系修正尝试）
//! 用法：cargo run --release --example pitch_reprobe_j39frac -- <j39.wotbreplay> <wi_shots.json> [tank_cache.json]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let replay_path = args[1].clone();
    let wi_path = args[2].clone();
    let cache_path = args.get(3).cloned().unwrap_or_else(|| "data/tank_cache.json".into());

    let f = std::fs::File::open(&replay_path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let br = replay.read_battle_results().ok();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // eid → 昵称 → tank
    let mut eid_to_name: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            let off = 57;
            let slen = p[off] as usize;
            if (3..=30).contains(&slen) && off + 1 + slen <= p.len() {
                if let Ok(s) = std::str::from_utf8(&p[off + 1..off + 1 + slen]) {
                    if s.chars().all(|c| c.is_ascii_graphic()) {
                        eid_to_name.entry(eid).or_insert_with(|| s.to_string());
                    }
                }
            }
        }
    }
    let mut name_to_tank: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    if let Some(br) = &br {
        for p in &br.players {
            let tank = br.player_results.iter().find(|pr| pr.info.account_id == p.account_id)
                .map(|pr| pr.info.tank_id).unwrap_or(0);
            name_to_tank.insert(p.info.nickname.clone(), tank);
        }
    }
    let cache: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cache_path).unwrap()).unwrap();
    let mut tank_limits: std::collections::HashMap<u32, (f32, f32)> = std::collections::HashMap::new();
    if let serde_json::Value::Object(map) = &cache {
        for (k, v) in map {
            if let (Ok(tid), Some(dep), Some(ele)) = (k.parse::<u32>(), v.get("gun_depression").and_then(|x| x.as_f64()), v.get("gun_elevation").and_then(|x| x.as_f64())) {
                tank_limits.insert(tid, (dep as f32, ele as f32));
            }
        }
    }

    // prop2 / t10 / m0 收集
    let mut p2: std::collections::HashMap<u32, Vec<(f32, u16)>> = std::collections::HashMap::new();
    let mut t10: std::collections::HashMap<u32, Vec<(f32, f32)>> = std::collections::HashMap::new(); // hull pitch
    let mut m0_fires: Vec<(f32, u32)> = Vec::new(); // (t, shooter eid)
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 {
                    p2.entry(u32le(&p[0..4])).or_default().push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    t10.entry(u32le(&p[0..4])).or_default().push((pkt.clock_secs, f32le(&p[40..44])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 12 && u32le(&p[4..8]) == 0x00 {
                    m0_fires.push((pkt.clock_secs, u32le(&p[0..4])));
                }
            }
            _ => {}
        }
    }

    // 1) frac 分布
    println!("=== 各实体 frac 分布（0-63 直方图摘要：min/max/众数区/包数/钳0/钳63）===");
    for (eid, seq) in &p2 {
        let tank = eid_to_name.get(eid).and_then(|n| name_to_tank.get(n)).copied().unwrap_or(0);
        let mut hist = [0usize; 16]; // 4-bit bins
        let (mut mn, mut mx) = (64usize, 0usize);
        let (mut p0, mut p63) = (0, 0);
        for (_, u) in seq {
            let fr = (u & 63) as usize;
            hist[fr / 4] += 1;
            mn = mn.min(fr); mx = mx.max(fr);
            if fr == 0 { p0 += 1; } if fr == 63 { p63 += 1; }
        }
        let dom = hist.iter().enumerate().max_by_key(|(_, c)| **c).unwrap().0;
        println!("  eid=0x{:08x} tank={:<6} n={:<5} frac[{},{},] 众数{}-{} 钳0:{} 钳63:{}", eid, tank, seq.len(), mn, mx, dom * 4, dom * 4 + 3, p0, p63);
    }

    // 2)+3) m0 识别射手 → frac 对照
    let wi: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&wi_path).unwrap()).unwrap();
    println!("\n=== m0 开火识别射手 → frac 解码 vs WI gun_pitch ===");
    println!("{:>8} {:>10} {:>6} {:>5} {:>8} {:>8} {:>7} {:>8}", "t", "eid", "tank", "frac", "解hull°", "真°", "err°", "hullP°");
    let mut stats = (0usize, 0usize, 0usize, 0.0f32); // n, hit15, hit30, sum
    let mut stats_hull = (0usize, 0usize, 0.0f32);
    for s in wi.as_array().unwrap() {
        let t = s["time"].as_f64().unwrap() as f32;
        let gp = s["gun_pitch"].as_f64().unwrap() as f32 * 57.29578;
        // 找 ±0.15s 内 m0
        let fires: Vec<(f32, u32)> = m0_fires.iter().filter(|(ft, _)| (*ft - t).abs() <= 0.15).cloned().collect();
        if fires.is_empty() { continue; }
        // 射手 eid：取最近 m0
        let (ft, eid) = fires.iter().min_by_key(|(ft, _)| ((ft - t).abs() * 1000.0) as i32).unwrap();
        let tank = eid_to_name.get(eid).and_then(|n| name_to_tank.get(n)).copied().unwrap_or(0);
        let (dep, ele) = tank_limits.get(&tank).copied().unwrap_or((8.0, 15.0));
        let seq = match p2.get(eid) { Some(s) => s, None => continue };
        let best = seq.iter().filter(|(pt, _)| (*pt - t).abs() <= 0.3)
            .min_by_key(|(pt, _)| ((*pt - t).abs() * 1000.0) as i32);
        let (pt, u) = match best { Some(b) => b, None => continue };
        let frac = (u & 63) as f32;
        let pitch_hull = -dep + frac / 63.0 * (dep + ele);
        // hull pitch（t10 最近值）
        let hp = t10.get(eid).and_then(|v| v.iter().rev().find(|(tt, _)| *tt <= *pt + 0.05).map(|(_, p)| *p)).unwrap_or(0.0) * 57.29578;
        let pitch_world = pitch_hull + hp; // 若 frac 为车体系、WI 为世界系
        let err = (pitch_hull - gp).abs();
        let err_w = (pitch_world - gp).abs();
        stats.0 += 1; stats.3 += err;
        if err < 1.5 { stats.1 += 1; } if err < 3.0 { stats.2 += 1; }
        stats_hull.0 += 1; stats_hull.2 += err_w;
        if err_w < 1.5 { stats_hull.1 += 1; }
        println!("{:8.2} 0x{:08x} {:<6} {:>5.0} {:>8.2} {:>+8.2} {:>7.2} {:>+8.2}{}", t, eid, tank, frac, pitch_hull, gp, err, hp,
            if err_w < err.abs() { format!(" (world:{:.2})", err_w) } else { String::new() });
    }
    println!("\n车体系解码: n={} ±1.5°:{} ±3°:{} 平均err {:.2}°", stats.0, stats.1, stats.2, stats.3 / stats.0.max(1) as f32);
    println!("世界系修正: ±1.5°:{} 平均err {:.2}°", stats_hull.1, stats_hull.2 / stats_hull.0.max(1) as f32);
}
