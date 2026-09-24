//! 弹着点候选数据筛查：对每发命中弹导出全部可能与"命中位置"相关的字段 +
//! 几何重建的接触点（弦 × 部件 OBB @ 判定位姿），供编码假设检验。
//! 导出字段：cmpIndex / segment B5B6(i16 BE)+B7(plateId) / method8 尾 b0..b3 / hash6 六字节 /
//! 接触点（车体系+世界系）/ 弦入射距离 / contact→ball_b 距离 / 面元厚度表。
//! 用法：cargo run --release --example hitpos_screen -- <a.wotbreplay> <out.json>
use std::io::Write;
use wotbreplay_parser::replay::Replay;

mod replay_shim {
    #[path = "../../src/replay/filter.rs"]
    pub mod filter;
    #[path = "../../src/replay/combat.rs"]
    pub mod combat;
}
use replay_shim::combat as combat_mod;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (path, out) = (args[1].clone(), args[2].clone());
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = Replay::open(f).unwrap();
    let br = replay.read_battle_results().ok();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    let raw_packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        (t, pkt.clock_secs, &pkt.raw_payload[..])
    }).collect();

    let file_name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let shots = combat_mod::extract_shot_replays_auto(&raw_packets, file_name).unwrap();

    // eid → 昵称 → tank_id
    let mut eid_to_name: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            let slen = p[57] as usize;
            if (3..=30).contains(&slen) && 58 + slen <= p.len() {
                if let Ok(n) = std::str::from_utf8(&p[58..58 + slen]) {
                    eid_to_name.insert(eid, n.to_string());
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

    // method8 尾部字节（shooter+victim+时刻匹配）
    let mut m8tails: Vec<(f32, u32, u32, [u8; 4], [u8; 6], u8, u8)> = Vec::new(); // t, shooter, victim, b0..b3, hash6, result, cmp
    for (t, clock, p) in &raw_packets {
        if *t != 8 || p.len() < 30 { continue; }
        if u32le(&p[4..8]) != 0x08 { continue; }
        let alen = u32le(&p[8..12]) as usize;
        if alen < 21 || 12 + alen > p.len() { continue; }
        let a = &p[12..12 + alen];
        if a[8] != 1 { continue; }
        m8tails.push((*clock, u32le(&a[0..4]), u32le(&a[4..8]),
            [a[17], a[18], a[19], a[20]],
            [a[11], a[12], a[13], a[14], a[15], a[16]], a[9], a[10]));
    }

    // game_data collision + armor_model
    let load_tank = |tid: u32| -> Option<serde_json::Value> {
        let txt = std::fs::read_to_string(format!("data/game_data/{}.json", tid)).ok()?;
        serde_json::from_str(&txt).ok()
    };

    let mut rows: Vec<serde_json::Value> = Vec::new();
    for s in &shots {
        let (Some(tok), Some(part)) = (&s.hit_token, s.server_part_index) else { continue };
        if part > 3 { continue; }
        let victim = s.target_pos;
        let Some(tank) = name_to_tank.get(&s.target_name).copied() else { continue };
        let Some(gd) = load_tank(tank) else { continue };
        let Some(c) = gd.get("collision") else { continue };
        // 部件盒（局部）
        let key = ["chassis_bbox", "hull_bbox", "turret_bbox", "gun_bbox"][part as usize];
        let Some(bb) = c.get(key).and_then(|b| Some((b.get("min")?, b.get("max")?))) else { continue };
        let f3 = |v: &serde_json::Value| -> [f32; 3] {
            let a = v.as_array().unwrap();
            [a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32, a[2].as_f64().unwrap() as f32]
        };
        let (bmin, bmax) = (f3(bb.0), f3(bb.1));
        let f3o = |k: &str| -> Option<[f32; 3]> {
            c.get(k).and_then(|v| v.as_array()).map(|a| [a[0].as_f64().unwrap() as f32, a[1].as_f64().unwrap() as f32, a[2].as_f64().unwrap() as f32])
        };
        let tp_ = f3o("turret_points").unwrap_or([0.0; 3]);
        let ring = [-tp_[0], -tp_[1]];
        let gp = f3o("gun_points").unwrap_or([0.0; 3]);
        let node = if part == 3 { [-gp[0] - ring[0], -gp[1] - ring[1]] } else { [0.0; 2] };
        let rotates = part >= 2;
        let hull_yaw = s.target_ang[0];
        let rel_yaw = s.target_turret_yaw - hull_yaw;

        // 弦 → 部件局部系，求 OBB 入射（t ∈ [0,1] 沿弦）
        let to_local = |p: [f32; 3]| -> [f32; 3] {
            let dx = p[0] - victim[0]; let dy = p[1] - victim[1]; let dz = p[2] - victim[2];
            let (sinv, cosv) = hull_yaw.sin_cos();
            let x = dx * cosv - dz * sinv;
            let z = dx * sinv + dz * cosv;
            let (lx, lz) = if rotates {
                let rx = x - ring[0]; let rz = z - ring[1];
                let (s2, c2) = rel_yaw.sin_cos();
                (rx * c2 - rz * s2 - node[0], rx * s2 + rz * c2 - node[1])
            } else { (x, z) };
            [lx, lz, dy]
        };
        let la = to_local(s.ball_a);
        let lb = to_local(s.ball_b);
        let mut t_enter: Option<f32> = None;
        {
            let (mut t0, mut t1) = (0.0f32, 1.0f32);
            let mut ok = true;
            for i in 0..3 {
                let (p0, p1) = (la[i], lb[i]);
                if (p1 - p0).abs() < 1e-9 {
                    if p0 < bmin[i] || p0 > bmax[i] { ok = false; break; }
                } else {
                    let mut a = (bmin[i] - p0) / (p1 - p0);
                    let mut b = (bmax[i] - p0) / (p1 - p0);
                    if a > b { std::mem::swap(&mut a, &mut b); }
                    t0 = t0.max(a); t1 = t1.min(b);
                    if t0 > t1 { ok = false; break; }
                }
            }
            if ok { t_enter = Some(t0); }
        }
        // 接触点（弦入射，车体局部 + 世界）
        let chord = [s.ball_b[0] - s.ball_a[0], s.ball_b[1] - s.ball_a[1], s.ball_b[2] - s.ball_a[2]];
        let cw_map = std::cell::RefCell::new(([0.0f32; 3], 0.0f32));
        let (contact_local, contact_world, entry_dist, contact_to_end) = match t_enter {
            Some(t) => {
                let cl = [la[0] + (lb[0] - la[0]) * t, la[1] + (lb[1] - la[1]) * t, la[2] + (lb[2] - la[2]) * t];
                // 局部 → 世界（逆变换）
                let (lx, ly, lz) = (cl[0], cl[1], cl[2]);
                let (wx, wz) = if rotates {
                    let (s2, c2) = (-rel_yaw).sin_cos();
                    let rx = lx * c2 - lz * s2 + node[0]; let rz = lx * s2 + lz * c2 + node[1];
                    (rx + ring[0], rz + ring[1])
                } else { (lx, lz) };
                let (sinv, cosv) = (-hull_yaw).sin_cos();
                let world = [victim[0] + wx * cosv - wz * sinv, victim[1] + ly, victim[2] + wx * sinv + wz * cosv];
                let full = (chord[0]*chord[0] + chord[1]*chord[1] + chord[2]*chord[2]).sqrt();
                *cw_map.borrow_mut() = (world, t * full);
                (Some(cl), Some(world), Some(t * full), Some((1.0 - t) * full))
            }
            None => (None, None, None, None),
        };

        // segment 字节 B5B6(i16 BE) / B7
        let seg = s.segment.to_le_bytes();
        let b56 = i16::from_be_bytes([seg[5], seg[6]]);
        let b7 = seg[7];
        // method8 尾部（匹配 shooter/victim/时刻）
        let tail = m8tails.iter().find(|(t, sh, vi, ..)| (*t - s.time_s).abs() <= 0.25 && *sh == s.shooter_eid && *vi == {
            // victim eid：target_name → eid（type5 映射反查）
            eid_to_name.iter().find(|(_, n)| **n == s.target_name).map(|(e, _)| *e).unwrap_or(0)
        });
        // 面元厚度
        let thickness = gd.get("armor_model").and_then(|am| am.get(["chassis", "hull", "turret", "gun"][part as usize]))
            .and_then(|p| p.get("plates")).and_then(|pl| pl.get(b7.to_string()))
            .and_then(|v| v.as_f64());

        // —— 窗口字节扫描：method8 时刻 ±0.3s 内所有包，逐偏移尝试多种编码 ——
        // f32 直值 ≈ 接触点(世界/局部)/入射距离（±0.25m）；
        // u8 /255、u16、i16 量化映射到部件盒轴（±0.18m）——DecodeShotSegment 风格 ——
        // 命中记录 (载体, 偏移, 编码类别)，跨发聚合判稳定性 ——
        let mut byte_matches: Vec<serde_json::Value> = Vec::new();
        {
            let m8t = *tail.map(|(t, _, _, _, _, _, _)| t).unwrap_or(&s.time_s);
            let cl = contact_local.unwrap_or([0.0; 3]);
            let cw_map = cw_map.into_inner();
            let spans = [(bmax[0] - bmin[0]), (bmax[1] - bmin[1]), (bmax[2] - bmin[2])];
            let targets: [(&str, f32); 7] = [
                ("wx", cw_map.0[0]), ("wy", cw_map.0[1]), ("wz", cw_map.0[2]),
                ("lx", cl[0]), ("lz", cl[1]), ("ly", cl[2]),
                ("dist", cw_map.1),
            ];
            let mut dbg_n = 0usize;
            for (t2, clock2, p) in raw_packets.iter() {
                if (clock2 - m8t).abs() > 0.3 { continue; }
                dbg_n += 1;
                let ty = *t2;
                let carrier = if ty == 8 { format!("t8/m{:02x}", u32le(&p[4..8])) }
                    else if ty == 7 { format!("t7/p{}", u32le(&p[4..8])) } else { format!("t{}", ty) };
                let mut push = |k: String, o: usize, v: f32| {
                    byte_matches.push(serde_json::json!({"c": carrier, "o": o, "k": k, "v": v}));
                };
                for off in 0..p.len().saturating_sub(3) {
                    let fv = f32::from_le_bytes([p[off], p[off+1], p[off+2], p[off+3]]);
                    if !fv.is_finite() { continue; }
                    for (label, target) in targets {
                        if (fv - target).abs() <= 0.25 { push(format!("f32/{}", label), off, fv); }
                    }
                }
                for off in 0..p.len() {
                    // u8 量化 → 各轴
                    for (ax, axis_name) in [(0usize, "x"), (1, "z"), (2, "y")] {
                        if spans[ax] <= 0.01 { continue; }
                        let q = p[off] as f32 / 255.0 * spans[ax] + bmin[ax];
                        let tgt = [cl[0], cl[1], cl[2]][ax];
                        if (q - tgt).abs() <= 0.18 { push(format!("u8/{}/{}", axis_name, axis_name), off, q); }
                    }
                    if off + 1 < p.len() {
                        for (label, iv) in [
                            ("be", i16::from_be_bytes([p[off], p[off+1]]) as f32),
                            ("le", i16::from_le_bytes([p[off], p[off+1]]) as f32)] {
                            // i16 直接毫米/厘米 + 量化映射
                            for (ax, axis_name) in [(0usize, "x"), (1, "z"), (2, "y")] {
                                if spans[ax] <= 0.01 { continue; }
                                let tgt = [cl[0], cl[1], cl[2]][ax];
                                if (iv / 1000.0 - tgt).abs() <= 0.18 { push(format!("i16{}/{}mm", label, axis_name), off, iv); }
                                if (iv / 100.0 - tgt).abs() <= 0.18 { push(format!("i16{}/{}cm", label, axis_name), off, iv); }
                                let q = iv / 32768.0 * spans[ax] + bmin[ax];
                                if (q - tgt).abs() <= 0.18 { push(format!("i16{}/{}q", label, axis_name), off, q); }
                            }
                        }
                        for (label, uv) in [
                            ("be16", u16::from_be_bytes([p[off], p[off+1]]) as f32),
                            ("le16", u16::from_le_bytes([p[off], p[off+1]]) as f32)] {
                            for (ax, axis_name) in [(0usize, "x"), (1, "z"), (2, "y")] {
                                if spans[ax] <= 0.01 { continue; }
                                let tgt = [cl[0], cl[1], cl[2]][ax];
                                let q = uv / 65535.0 * spans[ax] + bmin[ax];
                                if (q - tgt).abs() <= 0.18 { push(format!("{}u/{}q", label, axis_name), off, q); }
                                if (uv / 1000.0 - tgt).abs() <= 0.18 { push(format!("{}u/{}mm", label, axis_name), off, uv); }
                            }
                        }
                    }
                }
            }
            if std::env::var("DBG").is_ok() && rows.len() < 3 {
                eprintln!("[dbg] shot#{} m8t={:.2} winpackets={} matches={} cl={:?} cw={:?} spans={:?}",
                    s.index, m8t, dbg_n, byte_matches.len(), contact_local, cw_map, spans);
            }
        }

        rows.push(serde_json::json!({
            "index": s.index, "t": s.time_s, "tank": tank, "part": part,
            "result": s.game_hit_result,
            "cmp": part,
            "b56": b56, "b7": b7,
            "tail": tail.map(|(_, _, _, b, _, _, _)| b),
            "m8_result": tail.map(|(_, _, _, _, _, r, _)| r),
            "hash6": tok,
            "has_entry": t_enter.is_some(),
            "contact_local": contact_local,
            "contact_world": contact_world,
            "entry_dist": entry_dist,
            "contact_to_end": contact_to_end,
            "thickness": thickness,
            "bbox": [bmin, bmax],
            "byte_matches": byte_matches,
        }));
    }
    let out_path = out.clone();
    let mut out = std::fs::File::create(&out).unwrap();
    writeln!(out, "{}", serde_json::to_string_pretty(&rows).unwrap()).unwrap();
    println!("rows={} → {}", rows.len(), out_path);
    let _ = std::io::stdout().flush();
}
