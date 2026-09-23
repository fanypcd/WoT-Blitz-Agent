//! frac 语义终审：method8 直击通知严格配对（args[0]=shooter, args[4]=victim）
//! × WI gun_pitch 真值。修正此前两缺陷：实体配对（改用 victim/shooter 双向检验）
//! 与快照语义（prop2 变化驱动 → 取"最后已知值" pt<=t+0.1，不限陈旧度）。
//! 变体矩阵：配对(victim/shooter/m0shooter) × 锚定(车型极限 | 固定-10+20 | 固定-8+15)
//! × 量化(frac/63 | frac/64) × 参考系(车体 | 世界=+hull pitch)。
//! 用法：cargo run --release --example pitch_reprobe_victim -- <j39.wotbreplay> <wi_shots.json>
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let replay_path = args[1].clone();
    let wi_path = args[2].clone();

    let f = std::fs::File::open(&replay_path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let br = replay.read_battle_results().ok();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // eid → tank
    let mut eid_to_name: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            let slen = p[57] as usize;
            if (3..=30).contains(&slen) && 58 + slen <= p.len() {
                if let Ok(s) = std::str::from_utf8(&p[58..58 + slen]) {
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

    // 流收集
    let mut p2: std::collections::HashMap<u32, Vec<(f32, u16)>> = std::collections::HashMap::new();
    let mut t10p: std::collections::HashMap<u32, Vec<(f32, f32)>> = std::collections::HashMap::new();
    let mut m8: Vec<(f32, u32, u32)> = Vec::new();   // (t, shooter, victim)
    let mut m0: Vec<(f32, u32)> = Vec::new();
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
                if p.len() >= 48 { t10p.entry(u32le(&p[0..4])).or_default().push((pkt.clock_secs, f32le(&p[40..44]))); }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                let method = if p.len() >= 8 { u32le(&p[4..8]) } else { 0xffffffff };
                if method == 0x08 && p.len() >= 12 {
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

    let last_prop2 = |eid: u32, t: f32| -> Option<(f32, u16)> {
        p2.get(&eid)?.iter().filter(|(pt, _)| *pt <= t + 0.1).cloned().max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
    };
    let last_hull_pitch = |eid: u32, t: f32| -> f32 {
        t10p.get(&eid).and_then(|v| v.iter().filter(|(pt, _)| *pt <= t + 0.1).cloned().max_by(|a, b| a.0.partial_cmp(&b.0).unwrap()).map(|(_, p)| p)).unwrap_or(0.0) * 57.29578
    };

    let wi: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&wi_path).unwrap()).unwrap();

    // 预备每发的配对
    struct Shot { t: f32, gp: f32, victim: Option<u32>, shooter: Option<u32>, m0shooter: Option<u32> }
    let mut shots: Vec<Shot> = Vec::new();
    for s in wi.as_array().unwrap() {
        let t = s["time"].as_f64().unwrap() as f32;
        let gp = s["gun_pitch"].as_f64().unwrap() as f32 * 57.29578;
        let m8b = m8.iter().filter(|(t8, _, _)| (*t8 - t).abs() <= 0.25)
            .min_by_key(|(t8, _, _)| ((t8 - t).abs() * 1000.0) as i32);
        let m0b = m0.iter().filter(|(t0, _)| (*t0 - t).abs() <= 0.15)
            .min_by_key(|(t0, _)| ((t0 - t).abs() * 1000.0) as i32);
        shots.push(Shot {
            t, gp,
            victim: m8b.map(|(_, _, v)| *v),
            shooter: m8b.map(|(_, s, _)| *s),
            m0shooter: m0b.map(|(_, s)| *s),
        });
    }
    println!("WI 99 发：method8 配对 {} 发，m0 配对 {} 发",
        shots.iter().filter(|s| s.victim.is_some()).count(),
        shots.iter().filter(|s| s.m0shooter.is_some()).count());

    // 变体矩阵评估
    let pairings: [(&str, fn(&Shot) -> Option<u32>); 3] = [
        ("victim  ", |s: &Shot| s.victim),
        ("m8shoot", |s: &Shot| s.shooter),
        ("m0shoot", |s: &Shot| s.m0shooter),
    ];
    let anchors: [(&str, fn(u32, &std::collections::HashMap<u32, (f32, f32)>) -> (f32, f32)); 3] = [
        ("车型极限", |_t: u32, _m: &std::collections::HashMap<u32, (f32, f32)>| (-99.0, -99.0)), // 占位，实际用 tank_limits
        ("固定-10+20", |_: u32, _: &std::collections::HashMap<u32, (f32, f32)>| (-10.0, 20.0)),
        ("固定-8+15", |_: u32, _: &std::collections::HashMap<u32, (f32, f32)>| (-8.0, 15.0)),
    ];
    for (pname, pget) in &pairings {
        for (aname, aget) in &anchors {
            for &qdiv in [63.0f32, 64.0f32].iter() {
                for &world in [false, true].iter() {
                    let (mut n, mut hit15, mut hit30) = (0usize, 0usize, 0usize);
                    let mut sum = 0.0f32;
                    for s in &shots {
                        let eid = match pget(s) { Some(e) => e, None => continue };
                        let (pt, u) = match last_prop2(eid, s.t) { Some(x) => x, None => continue };
                        let _ = pt;
                        let (dep, ele) = if *aname == "车型极限" {
                            tank_limits.get(&tank_of(eid)).copied().unwrap_or((-99.0, -99.0))
                        } else { aget(0, &tank_limits) };
                        if dep < -50.0 { continue; } // 无锚
                        let frac = (u & 63) as f32;
                        let mut pitch = -dep + frac / qdiv * (dep + ele);
                        if world { pitch += last_hull_pitch(eid, s.t); }
                        let err = (pitch - s.gp).abs();
                        n += 1; sum += err;
                        if err < 1.5 { hit15 += 1; }
                        if err < 3.0 { hit30 += 1; }
                    }
                    if n >= 5 {
                        println!("  {} | {:>8} | /{:.0} | {}系: n={:<3} ±1.5°:{:<3}({:.0}%) ±3°:{:<3} avg {:+.2}°",
                            pname, aname, qdiv, if world { "世界" } else { "车体" }, n, hit15, 100.0 * hit15 as f32 / n as f32, hit30, sum / n as f32);
                    }
                }
            }
        }
    }

    // 最优变体的逐发明细（victim + 车型极限 + /63 + 车体系）
    println!("\n=== 明细：victim + 车型极限 + frac/63 + 车体系 ===");
    println!("{:>8} {:>10} {:>6} {:>5} {:>5} {:>8} {:>8} {:>7}", "t", "eid", "tank", "frac", "dt", "解°", "WI°", "err°");
    for s in &shots {
        let eid = match s.victim { Some(e) => e, None => continue };
        let (pt, u) = match last_prop2(eid, s.t) { Some(x) => x, None => continue };
        let tank = tank_of(eid);
        let (dep, ele) = match tank_limits.get(&tank) { Some(x) => *x, None => continue };
        let frac = (u & 63) as f32;
        let pitch = -dep + frac / 63.0 * (dep + ele);
        let err = (pitch - s.gp).abs();
        let mark = if err < 1.5 { " ✓" } else if err < 3.0 { " ~" } else { "" };
        println!("{:8.2} 0x{:08x} {:<6} {:>5.0} {:+5.1} {:>+8.2} {:>+8.2} {:>7.2}{}", s.t, eid, tank, frac, pt - s.t, pitch, s.gp, err, mark);
    }
}
