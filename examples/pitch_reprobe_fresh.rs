//! frac 新鲜度分组检验：frac=俯仰比例假设 + 快速偏航期间冻结（T110 发现）的相容性。
//! 对每发 WI shot × {victim, shooter}：取该实体射击时刻最后已知 prop2，
//! "新鲜" = 该包与其前一包的 frac 不同（俯仰通道正在更新）。
//! 预期：若比例语义成立且冻结机制存在 → 新鲜组吻合、冻结组不吻合。
//! 用法：cargo run --release --example pitch_reprobe_fresh -- <j39.wotbreplay> <wi_shots.json>
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let replay_path = args[1].clone();
    let wi_path = args[2].clone();

    let f = std::fs::File::open(&replay_path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let br = replay.read_battle_results().ok();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    let mut eid_to_name: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let slen = p[57] as usize;
            if (3..=30).contains(&slen) && 58 + slen <= p.len() {
                if let Ok(s) = std::str::from_utf8(&p[58..58 + slen]) {
                    if s.chars().all(|c| c.is_ascii_graphic()) {
                        eid_to_name.entry(u32le(&p[0..4])).or_insert_with(|| s.to_string());
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
    let cache: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("data/tank_cache.json").unwrap()).unwrap();
    let mut tank_limits: std::collections::HashMap<u32, (f32, f32)> = std::collections::HashMap::new();
    if let serde_json::Value::Object(map) = &cache {
        for (k, v) in map {
            if let (Ok(tid), Some(dep), Some(ele)) = (k.parse::<u32>(), v.get("gun_depression").and_then(|x| x.as_f64()), v.get("gun_elevation").and_then(|x| x.as_f64())) {
                tank_limits.insert(tid, (dep as f32, ele as f32));
            }
        }
    }
    let tank_of = |eid: u32| -> u32 {
        eid_to_name.get(&eid).and_then(|n| name_to_tank.get(n)).copied().unwrap_or(0)
    };

    let mut p2: std::collections::HashMap<u32, Vec<(f32, u16)>> = std::collections::HashMap::new();
    let mut m8: Vec<(f32, u32, u32)> = Vec::new();
    let mut m0: Vec<(f32, u32)> = Vec::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 {
                    p2.entry(u32le(&p[0..4])).or_default().push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                let method = if p.len() >= 8 { u32le(&p[4..8]) } else { 0xffffffff };
                if method == 0x08 && p.len() >= 20 {
                    let alen = u32le(&p[8..12]) as usize;
                    if alen >= 8 && 12 + 8 <= p.len() {
                        m8.push((pkt.clock_secs, u32le(&p[12..16]), u32le(&p[16..20])));
                    }
                }
                if method == 0x00 { m0.push((pkt.clock_secs, u32le(&p[0..4]))); }
            }
            _ => {}
        }
    }

    let wi: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&wi_path).unwrap()).unwrap();

    for (role_name, use_victim) in [("victim", true), ("shooter", false)] {
        let mut fresh_rows: Vec<(f32, f32, f32, u32, u32, bool)> = Vec::new(); // (t, 解码, 真值, eid, tank, err<1.5)
        let mut stale_n = 0; let mut stale_hit = 0; let mut stale_sum = 0.0f32;
        let mut fresh_n = 0; let mut fresh_hit = 0; let mut fresh_sum = 0.0f32;
        for s in wi.as_array().unwrap() {
            let t = s["time"].as_f64().unwrap() as f32;
            let gp = s["gun_pitch"].as_f64().unwrap() as f32 * 57.29578;
            // 配对：优先 method8，退回 m0（shooter）
            let eid = if use_victim {
                match m8.iter().filter(|(t8, _, _)| (*t8 - t).abs() <= 0.25).min_by_key(|(t8, _, _)| ((t8 - t).abs() * 1000.0) as i32) {
                    Some((_, _, v)) => Some(*v), None => None,
                }
            } else {
                match m8.iter().filter(|(t8, _, _)| (*t8 - t).abs() <= 0.25).min_by_key(|(t8, _, _)| ((t8 - t).abs() * 1000.0) as i32) {
                    Some((_, sh, _)) => Some(*sh),
                    None => m0.iter().filter(|(t0, _)| (*t0 - t).abs() <= 0.15).min_by_key(|(t0, _)| ((t0 - t).abs() * 1000.0) as i32).map(|(_, e)| *e),
                }
            };
            let eid = match eid { Some(e) => e, None => continue };
            let seq = match p2.get(&eid) { Some(s) => s, None => continue };
            // 最后已知值 + 前一包
            let idx = seq.iter().rposition(|(pt, _)| *pt <= t + 0.1);
            let idx = match idx { Some(i) => i, None => continue };
            let (pt, u) = seq[idx];
            let tank = tank_of(eid);
            let (dep, ele) = match tank_limits.get(&tank) { Some(x) => *x, None => continue };
            let frac = (u & 63) as f32;
            let pitch = ele - frac / 63.0 * (dep + ele); // 反向锚定：63=俯角极限 0=仰角极限
            let err = (pitch - gp).abs();
            let fresh = if idx == 0 { true } else { (seq[idx].1 & 63) != (seq[idx - 1].1 & 63) };
            if fresh {
                fresh_n += 1; fresh_sum += err;
                if err < 1.5 { fresh_hit += 1; }
                fresh_rows.push((t, pitch, gp, eid, tank, err < 1.5));
            } else {
                stale_n += 1; stale_sum += err;
                if err < 1.5 { stale_hit += 1; }
            }
        }
        println!("=== {} 组：新鲜 {} 发（±1.5° 命中 {}，avg {:.2}°）| 冻结 {} 发（命中 {}，avg {:.2}°）===",
            role_name, fresh_n, fresh_hit, fresh_sum / fresh_n.max(1) as f32, stale_n, stale_hit, stale_sum / stale_n.max(1) as f32);
        for (t, pitch, gp, eid, tank, ok) in &fresh_rows {
            println!("  {} t={:7.2} eid=0x{:08x} tank={:<6} 解{:+7.2}° 真{:+7.2}° err={:5.2}°", if *ok { "✓" } else { " " }, t, eid, tank, pitch, gp, (pitch - gp).abs());
        }
    }
}
