//! 作者路径炮塔回归检查：J39 目标炮塔角 vs 旧（最近采样）版本数值。
use wotbreplay_parser::replay::Replay;
mod replay_shim {
    #[path = "../../src/replay/filter.rs"]
    pub mod filter;
    #[path = "../../src/replay/combat.rs"]
    pub mod combat;
}
use replay_shim::combat as combat_mod;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let raw_packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        (t, pkt.clock_secs, &pkt.raw_payload[..])
    }).collect();
    let file_name = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let shots = combat_mod::extract_shot_replays_auto_with_limits(
        &raw_packets, file_name, &combat_mod::GunPitchLimits::new()).unwrap();
    println!("author shots: {}", shots.len());
    for s in shots.iter().take(8) {
        println!("#{} t={:.2} tgt_turret_yaw={:+8.2}° rel={:+8.2}° shr_turret_yaw={:+8.2}°",
            s.index, s.time_s, s.target_turret_yaw * 57.2958,
            (s.target_turret_yaw - s.target_ang[0]) * 57.2958,
            s.shooter_turret_yaw * 57.2958);
    }
}
