//! 2056 回放诊断：他人路径提取（作者=受击方），检查受击方炮塔指向字段 + prop2 双解码对照。
use wotbreplay_parser::replay::Replay;
mod replay_shim {
    #[path = "../../src/replay/filter.rs"]
    pub mod filter;
    #[path = "../../src/replay/combat.rs"]
    pub mod combat;
}
use replay_shim::combat as combat_mod;
use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).unwrap();
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

    // 玩家与车型 + 极限表（tank_cache.json 内联读）
    let mut tank_of: HashMap<String, u32> = HashMap::new();
    if let Some(br) = &br {
        for p in &br.players {
            let tank = br.player_results.iter().find(|pr| pr.info.account_id == p.account_id)
                .map(|pr| pr.info.tank_id).unwrap_or(0);
            tank_of.insert(p.info.nickname.clone(), tank);
            println!("player {:>16} team={} tank={}", p.info.nickname, p.info.team, tank);
        }
    }
    let mut limits = combat_mod::GunPitchLimits::new();
    if let Ok(txt) = std::fs::read_to_string("data/tank_cache.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            for (nick, tid) in &tank_of {
                if let (Some(dep), Some(ele)) = (
                    v.get(tid.to_string()).and_then(|t| t.get("gun_depression")).and_then(|x| x.as_f64()),
                    v.get(tid.to_string()).and_then(|t| t.get("gun_elevation")).and_then(|x| x.as_f64())) {
                    limits.insert(nick.clone(), combat_mod::GunPitchRange {
                        dep: dep as f32, ele: ele as f32,
                        front: None, back: None, transition: None,
                    });
                }
            }
        }
    }
    let file_name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let author_eid = combat_mod::resolve_author_player_eid(&raw_packets, file_name);
    println!("author_eid = 0x{:08x}, limits: {:?}", author_eid, limits);

    let others = combat_mod::extract_other_shot_replays_with_limits(&raw_packets, author_eid, &limits);
    println!("others: {} 发（总发射 {}，跳过：终点缺 {} 受击态缺 {} 炮口兜底 {}）",
        others.shots.len(), others.total_launches, others.skipped_no_endpoint, others.skipped_no_target_state, others.muzzle_fallback);
    for s in others.shots.iter().take(12) {
        let q = s.quality.clone().unwrap_or(combat_mod::ShotQuality { shooter_state_dt_ms: 0, shooter_pos_from_muzzle: false, target_state_dt_ms: None, turret_degraded: Vec::new(), dmg_unattributed: false, shell_from_broadcast: false, shooter_pitch_from_velocity: false, shooter_pitch_from_prop9: false, gun_pitch_degraded: Vec::new(), pitch_frozen: Vec::new(), shooter_anchor_src: None, target_anchor_src: None });
        println!("#{} t={:.2} shooter={:>14} target={:<14} tgt_turret_yaw={:+8.2}° hull={:+8.2}° rel={:+8.2}° tgt_gun_pitch={:+7.2}° frozen={:?} degraded={:?} tl={}",
            s.index, s.time_s, s.shooter_name, s.target_name,
            s.target_turret_yaw*57.2958, s.target_ang[0]*57.2958,
            (s.target_turret_yaw-s.target_ang[0])*57.2958,
            s.target_gun_pitch*57.2958, q.pitch_frozen, q.gun_pitch_degraded,
            s.target_turret_timeline.len());
    }
    // 作者 prop2 原始流（粗位 vs 全 u16 解码对照）
    let mut p2: Vec<(f32, u16)> = Vec::new();
    for (t, c, p) in &raw_packets {
        if *t == 7 && p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[0..4]) == author_eid {
            p2.push((*c, u16::from_le_bytes([p[12], p[13]])));
        }
    }
    println!("\n作者(受击方) prop2 采样 {} 条，前 12 条双解码对照：", p2.len());
    for (c, v) in p2.iter().take(12) {
        let coarse = (v >> 6) as f32 / 1024.0 * 360.0 - 180.0;
        let full = *v as f32 / 65535.0 * 360.0 - 180.0;
        println!("  t={:8.3} u16=0x{:04x} coarse={:+8.2}° full={:+8.2}° Δ={:+.3}°", c, v, coarse, full, full-coarse);
    }
}
