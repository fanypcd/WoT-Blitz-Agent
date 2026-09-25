//! 弹种兜底链覆盖率验证：作者+他人射击合并 → ShellKindTable 回填 → 统计弹种来源分布。
//! 对照：type=32 segment 权威 / 0x07 广播兜底 / 0x1b 地形广播兜底 / 未识别。
//! 用法：cargo run --release --example loadout_probe -- <replay> [...]
mod replay_shim {
    #[path = "../../src/replay/filter.rs"]
    pub mod filter;
    #[path = "../../src/replay/combat.rs"]
    pub mod combat;
    #[path = "../../src/replay/loadout.rs"]
    pub mod loadout;
}
mod data {
    use std::path::PathBuf;
    pub fn data_path(name: &str) -> PathBuf { PathBuf::from("data").join(name) }
}
mod wargaming {
    #[path = "../../src/wargaming/blitzkit.rs"]
    pub mod blitzkit;
}
use replay_shim::combat as combat_mod;
use replay_shim::loadout::ShellKindTable;

fn main() {
    for path in &std::env::args().collect::<Vec<_>>()[1..] {
        println!("\n===== {} =====", path);
        let f = std::fs::File::open(path).unwrap();
        let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
        let data = replay.read_data().unwrap();
        let raw: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
            let t = match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 5,
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            };
            (t, pkt.clock_secs, &pkt.raw_payload[..])
        }).collect();
        let file_name = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("");
        let author = combat_mod::resolve_author_player_eid(&raw, file_name);

        let mut shots = combat_mod::extract_shot_replays_auto_with_limits(
            &raw, file_name, &combat_mod::GunPitchLimits::new()).unwrap_or_default();
        let others = combat_mod::extract_other_shot_replays_with_limits(
            &raw, author, &combat_mod::GunPitchLimits::new());
        shots.extend(others.shots);
        ShellKindTable::from_tanks_pb().annotate(&mut shots);

        let (mut seg, mut b07, mut b1b, mut unknown_id, mut no_id) = (0u32, 0u32, 0u32, 0u32, 0u32);
        for s in &shots {
            let q = s.quality.as_ref();
            if s.shell_kind.is_empty() {
                if s.shell_id == 0 { no_id += 1; } else { unknown_id += 1; }
            } else if q.map(|q| q.shell_from_terrain).unwrap_or(false) { b1b += 1; }
            else if q.map(|q| q.shell_from_broadcast).unwrap_or(false) { b07 += 1; }
            else { seg += 1; }
        }
        let terrain_attached = shots.iter().filter(|s| s.terrain_impact.is_some()).count();
        println!("总弹量={}（作者+他人）| 弹种: segment权威={} 0x07兜底={} 0x1b兜底={} 未识别id={} 无id={}",
            shots.len(), seg, b07, b1b, unknown_id, no_id);
        println!("0x1b terrain_impact 附带落点: {} 发", terrain_attached);
    }
}
