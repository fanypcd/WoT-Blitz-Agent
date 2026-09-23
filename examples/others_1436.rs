//! 1436 他人路径伤害归属核验：extract_other_shot_replays vs 血量链真值。
//! 真值（prop3 链 + method8）：44.865→0、53.972→0、97.962→322、180.663→398、
//! 188.965→407、200.256→0、261.662→428、271.065→416、278.959→333、290.362→0。
//! 用法：cargo run --release --example others_1436 -- <path.wotbreplay>
use wotbreplay_parser::replay::Replay;

mod replay_shim {
    #[path = "../../src/replay/filter.rs"]
    pub mod filter;
    #[path = "../../src/replay/combat.rs"]
    pub mod combat;
}
use replay_shim::combat as combat_mod;

fn main() {
    let path = std::env::args().nth(1).expect("usage: <path.wotbreplay>");
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
    let others = combat_mod::extract_other_shot_replays(&packets, author);
    println!("author_eid=0x{:08x} launches={} shots={} skipped_ep={} skipped_state={} muzzle_fb={}",
        author, others.total_launches, others.shots.len(),
        others.skipped_no_endpoint, others.skipped_no_target_state, others.muzzle_fallback);
    for s in &others.shots {
        let unattr = s.quality.as_ref().map(|q| q.dmg_unattributed).unwrap_or(false);
        println!("  t={:8.3} shooter={:<16} dmg={:<5} kill={} result={} tgt={:<16} unattr={}",
            s.time_s, s.shooter_name, s.damage, s.is_kill, s.game_hit_result, s.target_name, unattr);
    }
    let _ = std::io::empty();
}
