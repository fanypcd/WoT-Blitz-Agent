//! 精确弹着点验证探针（逆向文档 §4.7 / 报告 §4.7）：
//! method8 元素的 6 个量化字节 → 部件 AABB（game_data collision）反推世界弹着点，
//! 与射击射线（method29 炮口 + 速度方向）的距离检验。
//! 判定：若反推点距射线 << 受击者车体距射线的基线，则 AABB 数据源与坐标系映射成立。
//!
//! 用法：cargo run --example hitpoint_probe -- <path.wotbreplay> [game_data_dir]
//! （game_data_dir 缺省 data/game_data）

use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: hitpoint_probe <file> [game_data_dir]");
    let gd_dir = std::env::args().nth(2).unwrap_or_else(|| "data/game_data".into());
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // ① 名册：eid → 昵称 → 坦克 ID（battle_results players/player_results）
    let br = replay.read_battle_results().expect("battle_results");
    let mut nick_of_eid: HashMap<u32, String> = HashMap::new();
    let mut tank_of_nick: HashMap<String, u32> = HashMap::new();
    let mut tank_of_account: HashMap<u32, u32> = HashMap::new();
    for pr in &br.player_results {
        tank_of_account.insert(pr.info.account_id, pr.info.tank_id);
    }
    for p in &br.players {
        if let Some(t) = tank_of_account.get(&p.account_id) {
            tank_of_nick.entry(p.info.nickname.clone()).or_insert(*t);
        }
    }
    for pr in &br.player_results {
        tank_of_account.insert(pr.info.account_id, pr.info.tank_id);
    }
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            let l = p[57] as usize;
            if !(3..=30).contains(&l) || 58 + l > p.len() { continue; }
            if let Ok(name) = std::str::from_utf8(&p[58..58 + l]) {
                nick_of_eid.insert(eid, name.to_string());
            }
        }
    }

    // ② game_data 部件 AABB 缓存（坦克 ID → [part] → (min, max)；part 0=chassis 1=hull 2=turret 3=gun）
    let mut bbox_cache: HashMap<u32, Option<[([f32; 3], [f32; 3]); 4]>> = HashMap::new();
    let load_bbox = |tank_id: u32, cache: &mut HashMap<u32, Option<[([f32; 3], [f32; 3]); 4]>>|
        -> Option<[([f32; 3], [f32; 3]); 4]> {
        if let Some(v) = cache.get(&tank_id) { return v.clone(); }
        let path = std::path::Path::new(&gd_dir).join(format!("{}.json", tank_id));
        let v = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|d| {
                let c = d.get("collision")?;
                let get = |name: &str| -> Option<([f32; 3], [f32; 3])> {
                    let b = c.get(name)?;
                    let min = b.get("min")?;
                    let max = b.get("max")?;
                    Some((
                        [min[0].as_f64()? as f32, min[1].as_f64()? as f32, min[2].as_f64()? as f32],
                        [max[0].as_f64()? as f32, max[1].as_f64()? as f32, max[2].as_f64()? as f32],
                    ))
                };
                Some([get("chassis_bbox")?, get("hull_bbox")?, get("turret_bbox")?, get("gun_bbox")?])
            });
        cache.insert(tank_id, v.clone());
        v
    };

    let mut packets: Vec<(u32, f32, &[u8])> = Vec::new();
    for pkt in &data.packets {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            _ => continue,
        };
        packets.push((t, pkt.clock_secs, &pkt.raw_payload[..]));
    }

    // ③ method29 发射（全射手）+ method8 命中（含 cmpIndex + 量化坐标）+ type10 位姿（文件序）
    let mut launches: Vec<(f32, u32, [f32; 3], [f32; 3])> = Vec::new(); // (t, shooter, ball_a, vel)
    let mut hits: Vec<(f32, u32, u32, [u8; 6], u8)> = Vec::new();        // (t, shooter, victim, hash6=量化坐标, cmpIndex)
    let mut pose: HashMap<u32, ([f32; 3], f32)> = HashMap::new();
    let mut launches_all: Vec<(f32, u32, [f32; 3], [f32; 3])> = Vec::new();        // eid → (pos, yaw)
    let mut turret_rel: HashMap<u32, Vec<(f32, f32)>> = HashMap::new();  // eid → [(clock, prop2 相对偏航)]
    let mut seen = std::collections::HashSet::new();
    for (t2, clock, p) in &packets {
        if *t2 == 10 && p.len() >= 48 {
            let g = |o: usize| f32le(&p[o..o + 4]);
            pose.insert(u32le(&p[0..4]), ([g(12), g(16), g(20)], g(36)));
            continue;
        }
        if *t2 == 7 && p.len() >= 14 && u32le(&p[4..8]) == 2 {
            // prop2 = 炮塔相对车体偏航（u16 → rad，逆向文档 §三）
            let rel = u16::from_le_bytes([p[12], p[13]]) as f32 / 65535.0 * std::f32::consts::TAU
                - std::f32::consts::PI;
            turret_rel.entry(u32le(&p[0..4])).or_default().push((*clock, rel));
            continue;
        }
        if *t2 == 8 {
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if 12 + alen > p.len() { continue; }
            let a = &p[12..12 + alen];
            if mid == 0x1d && alen >= 37 {
                let sid = u32le(&a[4..8]);
                if seen.insert(sid) {
                    launches.push((*clock, u32le(&a[0..4]),
                        [f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21])],
                        [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])]));
                }
            } else if mid == 0x08 && alen >= 17 && a[8] == 0x01 {
                // 元素 8B = [result a9][cmpIndex a10][量化坐标 a11..a17]
                hits.push((*clock, u32le(&a[0..4]), u32le(&a[4..8]),
                    [a[11], a[12], a[13], a[14], a[15], a[16]], a[10]));
            }
            if mid == 0x1d && alen >= 37 {
                // 附带导出所有发射（供回归：命中点估计 = 同批发射射线 × 受击者位姿）
                let sid = u32le(&a[4..8]);
                if seen.insert(sid ^ 0x1d000000) {
                    launches_all.push((*clock, u32le(&a[0..4]),
                        [f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21])],
                        [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])]));
                }
            }
        }
    }

    // ④ 逐命中验证
    println!("=== 精确弹着点验证（量化坐标 + game_data AABB + 受击者位姿 → 射线距离）===");
    let point_to_ray = |p: [f32; 3], o: [f32; 3], d: [f32; 3]| -> f32 {
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1e-6 { return f32::MAX; }
        let d = [d[0] / len, d[1] / len, d[2] / len];
        let ap = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
        let t = ap[0] * d[0] + ap[1] * d[1] + ap[2] * d[2];
        let c = [ap[0] - d[0] * t, ap[1] - d[1] * t, ap[2] - d[2] * t];
        (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
    };
    let mut d1_all: Vec<f32> = Vec::new();
    let mut base_all: Vec<f32> = Vec::new();
    let mut n_ok = 0usize;
    let mut n_skip = 0usize;
    let mut gmin_all: Vec<f32> = Vec::new();
    // method29.shooter = 射手【玩家实体】，method8.shooter = 射手【车辆实体】——命名空间不同，
    // 经 type=5 名册昵称桥接（两类实体同名册条目）
    let nick_of = |eid: &u32| nick_of_eid.get(eid).cloned();
    for (t_hit, shooter, victim, h6, cmp) in &hits {
        let Some(shooter_nick) = nick_of(shooter) else { continue };
        let Some(nick) = nick_of_eid.get(victim) else { continue };
        let Some(tank_id) = tank_of_nick.get(nick).copied().or_else(|| tank_of_account.get(victim).copied()) else { continue };
        let Some(parts) = load_bbox(tank_id, &mut bbox_cache) else { continue };
        let Some((vpos, vyaw)) = pose.get(victim) else { continue };
        if vpos[0] == 0.0 && vpos[2] == 0.0 { continue; }  // avatar 占位
        let part = *cmp as usize;   // 0=chassis 1=hull 2=turret 3=gun
        if part >= 4 { continue; }
        let (cy, sy) = (vyaw.cos(), vyaw.sin());
        let deq = |q: u8, i: usize| parts[part].0[i] + q as f32 * (parts[part].1[i] - parts[part].0[i]) / 255.0;
        let p1 = [deq(h6[0], 0), deq(h6[1], 1), deq(h6[2], 2)];
        let p2 = [deq(h6[3], 0), deq(h6[4], 1), deq(h6[5], 2)];
        // 世界坐标：车体件用底盘 yaw；炮塔件（J39 实测）叠加 prop2 相对偏航，
        // 座圈高度取 hull bbox 顶 2.357（= turret_points z 的绝对值）
        let (w_a, w_b) = if tank_id == 9057 && part == 2 {
            let rel = turret_rel.get(victim).and_then(|v| v.iter().rev()
                .find(|(c, _)| *c <= *t_hit)).map(|(_, r)| *r).unwrap_or(0.0);
            let ty = vyaw + rel;
            let (tcy, tsy) = (ty.cos(), ty.sin());
            let pivot_z = 2.357f32;
            let wa = [
                vpos[0] + p1[0] * tcy + p1[1] * tsy,
                vpos[1] + pivot_z + p1[2],
                vpos[2] - p1[0] * tsy + p1[1] * tcy,
            ];
            let wb = [
                vpos[0] + p1[0] * tcy - p1[1] * tsy,
                vpos[1] + pivot_z + p1[2],
                vpos[2] - p1[0] * tsy - p1[1] * tcy,
            ];
            (wa, wb)
        } else {
            let wa = [
                vpos[0] + p1[0] * cy + p1[1] * sy,
                vpos[1] + p1[2],
                vpos[2] - p1[0] * sy + p1[1] * cy,
            ];
            let wb = [
                vpos[0] + p1[0] * cy - p1[1] * sy,
                vpos[1] + p1[2],
                vpos[2] - p1[0] * sy - p1[1] * cy,
            ];
            (wa, wb)
        };
        // 候选发射 = 同昵称、与命中同批次（hitscan：method29 与 method8 同 tick 到达，±0.1s；
        // 同射手快速连发时选【射线实际穿过受击者】的那条）
        // 验证在【受击者局部系】进行（量化坐标 = 部件 AABB 局部坐标，itemDefs 轴 x=右 y=前 z=高；
        // world: x=x, y=高, z=z）。局部系对比可消去记录者位姿延迟链（位置项随原点平移消去）。
        let to_local = |w: [f32; 3]| -> [f32; 3] {
            let dxw = w[0] - vpos[0];
            let dyw = w[1] - vpos[1];
            let dzw = w[2] - vpos[2];
            let (s, c) = vyaw.sin_cos();
            // world fwd=(sin,0,cos) → local y=前; world right=(cos,0,-sin) → local x=右; 高 → local z
            [dxw * c - dzw * s, dxw * s + dzw * c, dyw]
        };
        let mut best: Option<(f32, [f32; 3], [f32; 3])> = None; // (baseline, local_origin, local_dir_end)
        for (t, s, ball_a, vel) in &launches {
            if nick_of(s).as_deref() != Some(shooter_nick.as_str()) { continue; }
            if (*t - *t_hit).abs() > 0.1 { continue; }
            let base = point_to_ray(*vpos, *ball_a, *vel);
            let lo = to_local(*ball_a);
            let le = to_local([ball_a[0] + vel[0], ball_a[1] + vel[1], ball_a[2] + vel[2]]);
            if best.map(|(b, _, _)| base < b).unwrap_or(true) {
                best = Some((base, lo, le));
            }
        }
        if best.is_none() {
            for (t, s, ball_a, vel) in &launches {
                if nick_of(s).as_deref() != Some(shooter_nick.as_str()) { continue; }
                if *t > *t_hit || *t_hit - *t > 2.5 { continue; }
                let base = point_to_ray(*vpos, *ball_a, *vel);
                let lo = to_local(*ball_a);
                let le = to_local([ball_a[0] + vel[0], ball_a[1] + vel[1], ball_a[2] + vel[2]]);
                if best.map(|(b, _, _)| base < b).unwrap_or(true) {
                    best = Some((base, lo, le));
                }
            }
        }
        let Some((base, lo, le)) = best else { n_skip += 1; continue };
        let l_dir = [le[0] - lo[0], le[1] - lo[1], le[2] - lo[2]];
        let d1_local = point_to_ray(p1, lo, l_dir);
        let d2_local = point_to_ray(p2, lo, l_dir);
        d1_all.push(d1_local.min(d2_local));
        base_all.push(base);
        // 独立强检验：全战斗所有发射中，局部系距反推点最近的射线（不限射手/时间）
        let gmin = launches.iter()
            .map(|(_, _, ba, v)| {
                let o = to_local(*ba);
                let e = to_local([ba[0] + v[0], ba[1] + v[1], ba[2] + v[2]]);
                point_to_ray(p1, o, [e[0] - o[0], e[1] - o[1], e[2] - o[2]])
            })
            .fold(f32::MAX, f32::min);
        gmin_all.push(gmin);
        n_ok += 1;
        if n_ok <= 12 {
            println!("t={:7.2} victim={:08x} part={} q={:02x}{:02x}{:02x}  局部射线距: p1={:6.3} p2={:6.3} (基线={:5.2}m 全局min={:6.3})",
                t_hit, victim, part, h6[0], h6[1], h6[2], d1_local, d2_local, base, gmin);
        }
    }
    let med = |mut v: Vec<f32>| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); if v.is_empty() { 0.0 } else { v[v.len() / 2] } };
    println!("=== 汇总（{} 发可验证，{} 发无候选发射）===", n_ok, n_skip);
    println!("局部系反推点距射线 med(p1∪p2 取近) = {:.4} m", med(d1_all));
    println!("基线（受击者车体距射线，世界系）med = {:.3} m", med(base_all));
    println!("局部系全局最近射线 med = {:.4} m", med(gmin_all));

    // ===== 线性回归数据导出：命中点局部估计 ↔ 量化字节 =====
    // 命中点估计 = 同批发射射线上距受击者最近的点，转受击者局部系（itemDefs 轴 x=右 y=前 z=高）
    println!("=== REG_DUMP ===");
    println!("nick,tank_id,cmp,q0,q1,q2,q3,q4,q5,hx,hy,hz,t");
    for (t_hit, shooter, victim, h6, cmp) in &hits {
        let Some(shooter_nick) = nick_of(shooter) else { continue };
        let Some(nick) = nick_of_eid.get(victim) else { continue };
        let Some(tank_id) = tank_of_nick.get(nick).copied().or_else(|| tank_of_account.get(victim).copied()) else { continue };
        let Some((vpos, vyaw)) = pose.get(victim) else { continue };
        if vpos[0] == 0.0 && vpos[2] == 0.0 { continue; }
        // 同批发射（±0.1s 同昵称）
        let mut best: Option<f32> = None;
        let mut best_l: Option<(f32, u32, [f32; 3], [f32; 3])> = None;
        for (t, s2, ba, v) in &launches_all {
            if nick_of(s2).as_deref() != Some(shooter_nick.as_str()) { continue; }
            if (*t - *t_hit).abs() > 0.1 { continue; }
            let base = point_to_ray(*vpos, *ba, *v);
            if best.map(|b| base < b).unwrap_or(true) {
                best = Some(base); best_l = Some((*t, *s2, *ba, *v));
            }
        }
        let Some((_lt, _ls, ba, v)) = best_l else { continue };
        let (cy, sy) = vyaw.sin_cos();
        // 射线最近点（世界）→ 局部（itemDefs 轴：局部 x=右=世界(cos,-sin)方向… 直接逆变换）
        let wx = *vpos;  // 占位：下面手工算
        let _ = wx;
        let apex = {
            let ap = [ba[0] - vpos[0], ba[1] - vpos[1], ba[2] - vpos[2]];
            let vl = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if vl < 1e-3 { *vpos } else {
                let t = (ap[0] * v[0] + ap[1] * v[1] + ap[2] * v[2]) / (vl * vl);
                [ba[0] + v[0] * t, ba[1] + v[1] * t, ba[2] + v[2] * t]
            }
        };
        // world → local (itemDefs: x=右, y=前, z=高)：dxw/dzw 转 (x,y)，dyw=高
        let dxw = apex[0] - vpos[0];
        let dyw = apex[1] - vpos[1];
        let dzw = apex[2] - vpos[2];
        let lx = dxw * cy + dzw * sy;
        let ly = -dxw * sy + dzw * cy;
        let lz = dyw;
        println!("{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.2}",
            nick, tank_id, cmp, h6[0], h6[1], h6[2], h6[3], h6[4], h6[5], lx, ly, lz, t_hit);
    }
}
