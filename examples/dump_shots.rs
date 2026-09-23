//! 走 extract_shot_replays 完整管线导出射击复现 JSON（含渲染锚点/tick 时间线/segment），
//! 与 dump_round 的 m8 尾部按时间拼接做装甲片对照。
//! 用法：cargo run --release --example dump_shots -- <path.wotbreplay> <out.json> [name]
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
    let path = std::env::args().nth(1).expect("usage");
    let out_path = std::env::args().nth(2).expect("missing out");
    let name = std::env::args().nth(3).unwrap_or_default();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        (t, pkt.clock_secs, &pkt.raw_payload[..])
    }).collect();
    let file_name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let author = combat_mod::resolve_author_player_eid(&packets, file_name);
    let shots = combat_mod::extract_shot_replays(&packets, author)
        .expect("extract_shot_replays failed");
    let mut out = std::fs::File::create(&out_path).unwrap();
    let _ = out.write_all(b"{\"shots\":[");
    for (i, s) in shots.iter().enumerate() {
        if i > 0 { let _ = out.write_all(b","); }
        let j = serde_json::json!({
            "time_s": s.time_s, "fire_time": s.fire_time, "shot_id": s.shot_id,
            "damage": s.damage, "is_author": s.is_author,
            "target_pos": s.target_pos, "target_ang": s.target_ang,
            "target_turret_yaw": s.target_turret_yaw,
            "target_gun_pitch": s.target_gun_pitch,
            "ball_a": s.ball_a, "ball_b": s.ball_b,
            "game_hit_result": s.game_hit_result,
            "server_part_index": s.server_part_index,
            "armor_group": s.armor_group, "hit_triangle": s.hit_triangle,
            "shell_id": s.shell_id, "segment": s.segment,
            "target_render": s.target_render,
            "shooter_render": s.shooter_render,
            "tick_samples_last": s.tick_samples.last(),
        });
        let _ = out.write_all(j.to_string().as_bytes());
    }
    let _ = out.write_all(b"]}");
    println!("wrote {} (author_eid=0x{:08x}, shots={})", out_path, author, shots.len());
}
