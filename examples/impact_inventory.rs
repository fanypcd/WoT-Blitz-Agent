//! 回放弹着点信息盘点：对样例回放逐发列出所有与"弹着点"相关的数据源并交叉对照——
//! 1) method20 弹道终点（aim_point 相对目标 / ball_b 绝对）——命中弹可为穿透出射点
//! 2) method8/type=32 hash6 → DecodeShotSegment 弹孔 AABB 量化点（entry/exit，客户端
//!    特效/贴花位置；需要 game_data 碰撞盒，此处打印 hash6 与部件号）
//! 3) 0x1b 地形命中（脱靶弹精确落点+材质+末段起点）
//! 4) 与目标尺寸的包络检查（弹道终点是否在车体包络内/对侧 = 出射点形态）
//! 用法：cargo run --release --example impact_inventory -- <a.wotbreplay>
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
    let path = std::env::args().nth(1).expect("usage: impact_inventory <a.wotbreplay>");
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
    // 极限锚定表留空（本盘点只看几何字段，不涉及俯仰）
    let shots = combat_mod::extract_shot_replays_auto_with_limits(
        &raw_packets, file_name, &combat_mod::GunPitchLimits::new()).unwrap();

    // method20 全量（含他人）与 0x1b 地形命中统计
    let mut m20 = 0usize;
    let mut m1b = 0usize;
    let mut m8_hash = 0usize;
    for (t, _, p) in &raw_packets {
        if *t != 8 || p.len() < 12 { continue; }
        let m = u32le(&p[4..8]);
        let alen = u32le(&p[8..12]) as usize;
        if 12 + alen > p.len() { continue; }
        match m {
            0x14 => m20 += 1,
            0x1b => m1b += 1,
            0x08 => { if alen >= 10 { m8_hash += 1; } }
            _ => {}
        }
    }
    println!("=== {} ===", file_name);
    println!("包统计: method20 弹道终点 {}、0x1b 地形命中 {}、method8 直击通知(hash6) {}、提取射击 {} 发",
        m20, m1b, m8_hash, shots.len());

    // 坦克尺寸（粗略包络用）：collision bbox 从 game_data；缺省 3.5×7m
    let mut limits = std::collections::HashMap::new();
    if let Some(br) = &br {
        // 目标昵称 → tank_id（供人工对照，不参与计算）
        for p in &br.players {
            let tank = br.player_results.iter().find(|pr| pr.info.account_id == p.account_id)
                .map(|pr| pr.info.tank_id).unwrap_or(0);
            limits.insert(p.info.nickname.clone(), tank);
        }
    }

    println!("\n{:>3} {:>7} {:>10} {:>14} {:>14} {:>10} {:>6} {:>8} {:>8}",
        "#", "t", "命中?", "aim_pt(x,y,z)", "弹着|off|", "对侧?", "部件", "hash6", "地形命中");
    for s in &shots {
        let hit = !s.target_name.is_empty();
        let ap = s.aim_point;
        let dist = (ap[0]*ap[0] + ap[1]*ap[1] + ap[2]*ap[2]).sqrt();
        // 对侧判定：弹道终点在目标另一侧 = 穿透出射（沿射向投影 > 车长一半的粗判）
        let lv = s.launch_velocity;
        let lvh = (lv[0]*lv[0] + lv[2]*lv[2]).sqrt();
        let along = if lvh > 1e-3 { (ap[0]*lv[0] + ap[2]*lv[2]) / lvh } else { 0.0 };
        let far_side = hit && along > 3.0;
        let terr = s.terrain_impact.as_ref()
            .map(|t| format!("{}@({:.1},{:.1})", t.material, t.impact_point[0], t.impact_point[2]))
            .unwrap_or_default();
        println!("{:>3} {:>7.2} {:>10} ({:+5.1},{:+5.1},{:+5.1}) {:>13.2}m {:>9} {:>6} {:>8} {:>8}",
            s.index, s.time_s, if hit { "命中" } else { "脱靶" },
            ap[0], ap[1], ap[2], dist,
            if far_side { "是(出射)" } else { "-" },
            s.server_part_index.map(|p| p.to_string()).unwrap_or("-".into()),
            s.hit_token.as_deref().unwrap_or("-"),
            terr);
        let _ = &limits;
    }
    let _ = std::io::stdout().flush();
}
