//! 跨回放验证（J39，14 车含队友，99 发 WI 真值）：prop2 低 6 位 = 炮管俯仰假说检验。
//! 对每发 WI shot：shooter（及 target 对照）账号 → 昵称 → 车辆 eid → 该 eid 在射击时刻
//! 最近的 prop2 → frac 解码 pitch = −dep + frac/63×(dep+ele)（按车型极限锚定）
//! → 与 WI gun_pitch 真值比对（±1.5° 计命中；统计随机符合基线）。
//! 用法：cargo run --release --example pitch_reprobe_wicross -- <j39.wotbreplay> <wi_shots.json> [tank_cache.json]
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

    // eid → 昵称（type=5）
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
    // 昵称 → (account_id, tank_id)；account → 昵称
    let mut name_to_tank: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
    if let Some(br) = &br {
        for p in &br.players {
            let tank = br.player_results.iter().find(|pr| pr.info.account_id == p.account_id)
                .map(|pr| pr.info.tank_id).unwrap_or(0);
            name_to_tank.insert(p.info.nickname.clone(), (p.account_id, tank));
        }
    }
    // tank_id → (dep, ele)
    let cache: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cache_path).unwrap()).unwrap();
    let mut tank_limits: std::collections::HashMap<u32, (f32, f32)> = std::collections::HashMap::new();
    if let serde_json::Value::Object(map) = &cache {
        for (k, v) in map {
            if let (Ok(tid), Some(dep), Some(ele)) = (k.parse::<u32>(), v.get("gun_depression").and_then(|x| x.as_f64()), v.get("gun_elevation").and_then(|x| x.as_f64())) {
                tank_limits.insert(tid, (dep as f32, ele as f32));
            }
        }
    }

    // 每 eid 的 prop2 序列
    let mut p2: std::collections::HashMap<u32, Vec<(f32, u16)>> = std::collections::HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 {
                p2.entry(u32le(&p[0..4])).or_default().push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
            }
        }
    }

    // WI shots
    let wi: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&wi_path).unwrap()).unwrap();
    let wi_shots = wi.as_array().unwrap();

    // 账号无法直接映射（WI 用内部编号）。改用 turret_yaw 自包含识别射手：
    // 对每发，在全部 14 车的 prop2 里找 |dt|<=0.15 且偏航解码吻合 WI turret_yaw 的实体，
    // 然后检验同一包的 frac 解码俯仰 vs WI gun_pitch。
    // 偏航两种解码并行统计：A=coarse10/1024、B=全 u16/65536。
    println!("prop2 实体数: {}，玩家数: {}", p2.len(), name_to_tank.len());
    let wrap_pi = |a: f32| -> f32 {
        let mut v = a;
        while v > std::f32::consts::PI { v -= std::f32::consts::TAU; }
        while v < -std::f32::consts::PI { v += std::f32::consts::TAU; }
        v
    };
    for yaw_mode in 0..2 {
        let mut n_shots = 0;
        let mut n_yaw_match = 0;
        let mut n_pitch_15 = 0; let mut n_pitch_30 = 0; let mut sum_err = 0.0f32;
        // 对照基线：所有实体（不做偏航匹配）的 frac 俯仰误差
        let mut base_15 = 0; let mut base_n = 0; let mut base_sum = 0.0f32;
        let mut rows: Vec<String> = Vec::new();
        for s in wi_shots {
            let t = s["time"].as_f64().unwrap() as f32;
            let gp = s["gun_pitch"].as_f64().unwrap() as f32;
            let ty = s["turret_yaw"].as_f64().unwrap() as f32;
            n_shots += 1;
            let mut best: Option<(f32, u16, u32, f32)> = None; // (dt, u16, eid, yaw_err)
            for (eid, seq) in &p2 {
                let tank_id = eid_to_name.get(eid)
                    .and_then(|n| name_to_tank.get(n)).map(|(_, tk)| *tk).unwrap_or(0);
                let _ = tank_id;
                for (pt, u) in seq {
                    let dt = *pt - t;
                    if dt.abs() > 0.15 { continue; }
                    let yaw = if yaw_mode == 0 {
                        (u >> 6) as f32 / 1024.0 * std::f32::consts::TAU - std::f32::consts::PI
                    } else {
                        *u as f32 / 65535.0 * std::f32::consts::TAU - std::f32::consts::PI
                    };
                    let ye = wrap_pi(yaw - ty).abs();
                    if ye < 0.08 && best.map(|b| ye < b.3).unwrap_or(true) {
                        best = Some((dt, *u, *eid, ye));
                    }
                }
            }
            // 基线：全部实体全部包
            for (eid, seq) in &p2 {
                let tank_id = eid_to_name.get(eid)
                    .and_then(|n| name_to_tank.get(n)).map(|(_, tk)| *tk).unwrap_or(0);
                let (dep, ele) = tank_limits.get(&tank_id).copied().unwrap_or((8.0, 15.0));
                for (pt, u) in seq {
                    if (*pt - t).abs() > 0.15 { continue; }
                    let pitch = -dep + (u & 63) as f32 / 63.0 * (dep + ele);
                    base_n += 1;
                    base_sum += (pitch - gp * 57.29578).abs();
                    if (pitch - gp * 57.29578).abs() < 1.5 { base_15 += 1; }
                }
            }
            if let Some((dt, u, eid, ye)) = best {
                n_yaw_match += 1;
                let tank_id = eid_to_name.get(&eid)
                    .and_then(|n| name_to_tank.get(n)).map(|(_, tk)| *tk).unwrap_or(0);
                let (dep, ele) = tank_limits.get(&tank_id).copied().unwrap_or((8.0, 15.0));
                let pitch = -dep + (u & 63) as f32 / 63.0 * (dep + ele);
                let err = (pitch - gp * 57.29578).abs();
                if err < 1.5 { n_pitch_15 += 1; }
                if err < 3.0 { n_pitch_30 += 1; }
                sum_err += err;
                if rows.len() < 15 {
                    rows.push(format!("  t={:7.2} eid=0x{:08x} tank={:<6} dt={:+.3} yawerr={:.3} frac={:>2} 解码={:+7.2}° WI={:+7.2}° err={:5.2}°",
                        t, eid, tank_id, dt, ye, (u & 63), pitch, gp * 57.29578, err));
                }
            }
        }
        let ym = if yaw_mode == 0 { "coarse10" } else { "全u16" };
        println!("\n=== 偏航解码 {}：99 发中偏航匹配 {} 发 ===", ym, n_yaw_match);
        for r in &rows { println!("{}", r); }
        if n_yaw_match > 0 {
            println!("  frac 俯仰 vs gun_pitch：±1.5° {}/{} ({:.0}%) | ±3.0° {} | 平均 {:+.2}°", n_pitch_15, n_yaw_match, 100.0 * n_pitch_15 as f32 / n_yaw_match as f32, n_pitch_30, sum_err / n_yaw_match as f32);
        }
        println!("  基线（不匹配直接全扫）：±1.5° {}/{} ({:.0}%) 平均 {:+.2}°", base_15, base_n, 100.0 * base_15 as f32 / base_n as f32, base_sum / base_n as f32);
    }
}
