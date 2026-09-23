//! 弹着点复现端到端验证（逆向报告 §二 / combat.rs compute_decal_hit_world）：
//! 与生产 combat.rs compute_decal_hit_world 逐行同构（binary crate 无 lib target，公式内联）
//! 复现游戏客户端弹孔位置，与 method29 射线（同批发射、同射手）的距离检验。
//! 判定：median 应与 hitpoint_probe 基线一致（J39 ≈1.9m，随机基线 ≈17m）。
//!
//! 用法：cargo run --release --example decal_check -- <path.wotbreplay> [game_data_dir]

use std::collections::HashMap;

// game_data/{tank_id}.json 的最小解析结构（与生产 CollisionData 字段对齐）
#[derive(serde::Deserialize)]
struct BBox { min: [f32; 3], max: [f32; 3] }
#[derive(serde::Deserialize)]
struct Collision {
    #[serde(default)] chassis_bbox: Option<BBox>,
    #[serde(default)] hull_bbox: Option<BBox>,
    #[serde(default)] turret_bbox: Option<BBox>,
    #[serde(default)] gun_bbox: Option<BBox>,
    #[serde(default)] turret_points: Option<[f32; 3]>,
    #[serde(default)] gun_points: Option<[f32; 3]>,
}
#[derive(serde::Deserialize)]
struct GameData { #[serde(default)] collision: Option<Collision> }

/// 部件节点几何（GLB 系）：ring=座圈（车体系）、node=炮塔相对偏移（炮管件）
#[derive(Clone, Copy)]
struct PartGeomEx {
    bbox: ([f32; 3], [f32; 3]),
    ring: [f32; 3],
    node: [f32; 2],
    node_z: f32,
    rotates: bool,
}
type PartGeom = Option<PartGeomEx>;

fn main() {
    let path = std::env::args().nth(1).expect("usage: decal_check <file> [game_data_dir]");
    let gd_dir = std::env::args().nth(2).unwrap_or_else(|| "data/game_data".into());
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // ① 名册：eid → 昵称 → 坦克 ID（与 hitpoint_probe 同源）
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

    // ② 部件几何缓存（生产同款：game_data JSON → collision bbox + points |z| 基面高）
    let mut geom_cache: HashMap<u32, Option<[PartGeom; 4]>> = HashMap::new();
    let load_geoms = |tank_id: u32,
                      cache: &mut HashMap<u32, Option<[PartGeom; 4]>>|
     -> Option<[PartGeom; 4]> {
        if let Some(v) = cache.get(&tank_id) { return *v; }
        // binary crate 无 lib target：此处内联 load_game_data 的读取逻辑（与生产一致）
        let content = std::fs::read_to_string(
            std::path::Path::new(&gd_dir).join(format!("{}.json", tank_id))).ok()?;
        let gd: GameData = serde_json::from_str(&content).ok()?;
        let c = gd.collision?;
        let bb = |b: &Option<BBox>| b.as_ref().map(|b| (b.min, b.max));
        let ring: [f32; 3] = c.turret_points.map(|p| [-p[0], -p[1], -p[2]]).unwrap_or([0.0; 3]);
        let gun_origin = c.gun_points
            .map(|g| [-g[0] - ring[0], -g[1] - ring[1], -g[2] - ring[2]]);
        let mk = |b: ([f32; 3], [f32; 3]), node: [f32; 2], node_z: f32, rotates: bool| PartGeomEx {
            bbox: b, ring, node, node_z, rotates,
        };
        let _ = mk;
        let mut parts: [PartGeom; 4] = [None, None, None, None];
        parts[0] = bb(&c.chassis_bbox).map(|b| PartGeomEx { bbox: b, ring, node: [0.0; 2], node_z: 0.0, rotates: false });
        parts[1] = bb(&c.hull_bbox).map(|b| PartGeomEx { bbox: b, ring, node: [0.0; 2], node_z: 0.0, rotates: false });
        parts[2] = bb(&c.turret_bbox).map(|b| PartGeomEx { bbox: b, ring, node: [0.0; 2], node_z: 0.0, rotates: true });
        parts[3] = bb(&c.gun_bbox).map(|b| PartGeomEx {
            bbox: b, ring,
            node: gun_origin.map(|g| [g[0], g[1]]).unwrap_or([0.0; 2]),
            node_z: gun_origin.map(|g| g[2]).unwrap_or(0.0),
            rotates: true,
        });
        cache.insert(tank_id, Some(parts));
        Some(parts)
    };

    // ②' type=5 名册包 → eid → 昵称（method29 射手=玩家实体、method8 射手=车辆实体，
    //    两类实体经名册昵称桥接——与 hitpoint_probe 同源）
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            let l = p[57] as usize;
            if !(3..=30).contains(&l) || 58 + l > p.len() { continue; }
            if let Ok(name) = std::str::from_utf8(&p[58..58 + l]) {
                nick_of_eid.entry(eid).or_insert_with(|| name.to_string());
            }
        }
    }

    // ③ 文件序状态机：type10 姿态 / prop2 炮塔相对角 + method29 发射 / method8 命中
    let mut launches: Vec<(f32, u32, [f32; 3], [f32; 3])> = Vec::new();
    let mut hits: Vec<(f32, u32, u32, [u8; 6], u8)> = Vec::new();
    let mut pose: HashMap<u32, ([f32; 3], f32)> = HashMap::new();
    let mut turret_rel: HashMap<u32, Vec<(f32, f32)>> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        let clock = pkt.clock_secs;
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            _ => continue,
        };
        let p = &pkt.raw_payload[..];
        if t == 10 && p.len() >= 48 {
            let g = |o: usize| f32le(&p[o..o + 4]);
            pose.insert(u32le(&p[0..4]), ([g(12), g(16), g(20)], g(36)));
        } else if t == 7 && p.len() >= 14 && u32le(&p[4..8]) == 2 {
            let rel = u16::from_le_bytes([p[12], p[13]]) as f32 / 65535.0
                * std::f32::consts::TAU - std::f32::consts::PI;
            turret_rel.entry(u32le(&p[0..4])).or_default().push((clock, rel));
        } else if t == 8 && p.len() >= 16 {
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if 12 + alen > p.len() { continue; }
            let a = &p[12..12 + alen];
            if mid == 0x1d && alen >= 37 {
                let sid = u32le(&a[4..8]);
                if seen.insert(sid) {
                    launches.push((clock, u32le(&a[0..4]),
                        [f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21])],
                        [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])]));
                }
            } else if mid == 0x08 && alen >= 17 && a[8] == 0x01 {
                hits.push((clock, u32le(&a[0..4]), u32le(&a[4..8]),
                    [a[11], a[12], a[13], a[14], a[15], a[16]], a[10]));
            }
        }
    }

    // ④ 逐命中：生产函数算世界弹着点 → 匹配同批发射射线 → 距离
    println!("=== 弹着点复现端到端验证（生产路径 compute_decal_hit_world）===");
    let point_to_ray = |p: [f32; 3], o: [f32; 3], d: [f32; 3]| -> f32 {
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1e-6 { return f32::MAX; }
        let d = [d[0] / len, d[1] / len, d[2] / len];
        let ap = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
        let tt = ap[0] * d[0] + ap[1] * d[1] + ap[2] * d[2];
        let c = [ap[0] - d[0] * tt, ap[1] - d[1] * tt, ap[2] - d[2] * tt];
        (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
    };
    let nick_of = |eid: &u32| nick_of_eid.get(eid).cloned();
    let mut dists: Vec<f32> = Vec::new();
    let mut base_all: Vec<f32> = Vec::new();
    let mut gmin_all: Vec<f32> = Vec::new();
    let mut by_part: std::collections::HashMap<u8, Vec<f32>> = HashMap::new();
    let mut by_tank: std::collections::HashMap<u32, Vec<f32>> = HashMap::new();
    let mut n_same = 0usize;
    let mut n_skip = 0usize;
    for (t_hit, shooter, victim, hash6, cmp) in &hits {
        let Some(shooter_nick) = nick_of(shooter) else { continue };
        let Some(nick) = nick_of_eid.get(victim) else { continue };
        let Some(tank_id) = tank_of_nick.get(nick).copied()
            .or_else(|| tank_of_account.get(victim).copied()) else { continue };
        let Some(parts) = load_geoms(tank_id, &mut geom_cache) else { continue };
        let Some(geom) = parts.get(*cmp as usize).and_then(|g| *g) else { continue };
        let Some((vpos, vyaw)) = pose.get(victim) else { continue };
        let (vpos, vyaw) = (*vpos, *vyaw);
        if vpos[0] == 0.0 && vpos[2] == 0.0 { continue; }
        // 部件 2/3 随炮塔绝对朝向（prop2 最新 ≤ 命中时刻 + 车体偏航）
        let yaw = if *cmp >= 2 {
            let rel = turret_rel.get(victim).and_then(|v| v.iter().rev()
                .find(|(c2, _)| *c2 <= *t_hit)).map(|(_, r)| *r).unwrap_or(0.0);
            vyaw + rel
        } else { vyaw };
        // 生产函数：hash6 + 部件盒 + 姿态 → 世界入/出点
        // 生产同款公式内联（combat.rs compute_decal_hit_world 逐行一致）：
        // 节点层级 = 座圈（−turret_points）随车体 + 部件内坐标随炮塔相对角；
        // 炮管件再叠加 gun_origin = −gun_points − ring（turret-relative）
        let rot = |yaw: f32, xy: [f32; 2]| -> [f32; 2] {
            let (s2, c2) = yaw.sin_cos();
            [xy[0] * c2 + xy[1] * s2, -xy[0] * s2 + xy[1] * c2]
        };
        let deq = |q: u8, i: usize| geom.bbox.0[i] + q as f32 * (geom.bbox.1[i] - geom.bbox.0[i]) / 255.0;
        let p1 = [deq(hash6[0], 0), deq(hash6[1], 1), deq(hash6[2], 2)];
        let p2 = [deq(hash6[3], 0), deq(hash6[4], 1), deq(hash6[5], 2)];
        let rel = yaw - vyaw;   // 炮塔相对角（yaw 已含 hull+prop2）
        let to_world = |pt: [f32; 3]| {
            let g = geom;
            if g.rotates {
                let local = rot(rel, [g.node[0] + pt[0], g.node[1] + pt[1]]);
                let ring = rot(vyaw, [g.ring[0], g.ring[1]]);
                [vpos[0] + ring[0] + local[0], vpos[1] + g.ring[2] + g.node_z + pt[2],
                 vpos[2] + ring[1] + local[1]]
            } else {
                // 非旋转件（底盘/车体）：节点水平 = ring 平移（通常≈0）+ 车体偏航旋转
                let off = rot(vyaw, [g.ring[0] + g.node[0] + pt[0], g.ring[1] + g.node[1] + pt[1]]);
                [vpos[0] + off[0], vpos[1] + pt[2], vpos[2] + off[1]]
            }
        };
        let (entry, exit, same) = (to_world(p1), to_world(p2), p1 == p2);
        if same { n_same += 1; }
        // 匹配发射：同昵称、同批（±0.1s）；无则 2.5s 内最近
        let mut best: Option<(f32, [f32; 3], [f32; 3])> = None;
        for (t, s2, ball_a, vel) in &launches {
            if nick_of(s2).as_deref() != Some(shooter_nick.as_str()) { continue; }
            if (*t - *t_hit).abs() > 0.1 { continue; }
            let base = point_to_ray(vpos, *ball_a, *vel);
            if best.map(|(b, _, _)| base < b).unwrap_or(true) {
                best = Some((base, *ball_a, *vel));
            }
        }
        if best.is_none() {
            for (t, s2, ball_a, vel) in &launches {
                if nick_of(s2).as_deref() != Some(shooter_nick.as_str()) { continue; }
                if *t > *t_hit || *t_hit - *t > 2.5 { continue; }
                let base = point_to_ray(vpos, *ball_a, *vel);
                if best.map(|(b, _, _)| base < b).unwrap_or(true) {
                    best = Some((base, *ball_a, *vel));
                }
            }
        }
        let Some((base, ball_a, vel)) = best else { n_skip += 1; continue };
        let d = point_to_ray(entry, ball_a, vel).min(point_to_ray(exit, ball_a, vel));
        dists.push(d);
        base_all.push(base);
        // 主指标：全场所有发射射线中距解码点最近者（延迟链/多弹同飞下时间窗配对
        // 本来就选不中真实命中弹——探针实测该指标 med ≈1.9m，量化语义的证据）
        let gmin = launches.iter()
            .map(|(_, _, ba, v)| point_to_ray(entry, *ba, *v).min(point_to_ray(exit, *ba, *v)))
            .fold(f32::MAX, f32::min);
        gmin_all.push(gmin);
        by_part.entry(*cmp).or_default().push(gmin);
        by_tank.entry(tank_id).or_default().push(gmin);
        if dists.len() <= 8 {
            println!("t={:7.2} victim={:08x} part={} same={}  批配对 {:6.3} m / 全局最近 {:6.3} m (基线 {:5.2})",
                t_hit, victim, cmp, same, d, gmin, base);
        }
    }
    let med = |mut v: Vec<f32>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() { 0.0 } else { v[v.len() / 2] }
    };
    println!("=== 汇总（{} 发可验证，{} 发无候选发射；单点分支 {} 发）===",
        dists.len(), n_skip, n_same);
    println!("全局最近射线（主指标，探针 J39 基线 ≈1.87m / 随机 ≈17m）med = {:.4} m", med(gmin_all));
    println!("批配对射线（延迟链/多弹同飞下会选错，仅参考）med        = {:.4} m", med(dists));
    println!("受击者车体距批配对射线基线 med                          = {:.3} m", med(base_all));
    let mut parts: Vec<_> = by_part.iter().collect(); parts.sort_by_key(|(k, _)| **k);
    for (k, v) in parts { let mut sv = v.clone(); sv.sort_by(|a,b| a.partial_cmp(b).unwrap());
        let p90 = sv.get(sv.len()*9/10).copied().unwrap_or(0.0);
        println!("  part={} n={:3} med={:.3} p90={:.3}", k, v.len(), med(v.clone()), p90); }
    let mut tanks: Vec<_> = by_tank.iter().collect(); tanks.sort_by_key(|(k, _)| **k);
    for (k, v) in tanks { println!("  tank {:6} n={:3} med={:.3}", k, v.len(), med(v.clone())); }
}
