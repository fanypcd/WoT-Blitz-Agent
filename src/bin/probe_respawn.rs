// 复活模式探针（issue #622 一次性诊断）：扫描回放的 updateArena 子类型直方图，
// 重点关注 19 VEHICLE_WAIT_RESPAWN / 20 VEHICLE_RESURRECT / 21 TEAM_RESPAWNS_LEFT，
// 并逐 eid 打印 prop1 血量链（找"归零后又回升"= 复活血量锚点）。
// 用法：cargo run --release --bin probe_respawn -- <file.wotbreplay | 目录>

use std::collections::BTreeMap;
use std::path::PathBuf;

use wotb_replay_core::replay::combat::{
    arena_subtype_name, collect_arena_updates_filtered, collect_initial_hp, parse_hp_events,
};
use wotb_replay_core::replay::packets::read_raw_packets;

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "data/replay_samples".into());
    let path = PathBuf::from(&arg);
    let files: Vec<PathBuf> = if path.is_dir() {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&path)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map(|x| x == "wotbreplay").unwrap_or(false))
            .collect();
        v.sort();
        v
    } else {
        vec![path]
    };

    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = match std::fs::read(&f) {
            Ok(b) => b,
            Err(e) => {
                println!("== {name}: 读取失败 {e}");
                continue;
            }
        };
        let packets = match read_raw_packets(&bytes) {
            Ok(p) => p,
            Err(e) => {
                println!("== {name}: 分帧失败 {e:?}");
                continue;
            }
        };
        let tuples: Vec<(u32, f32, &[u8])> = packets
            .iter()
            .map(|p| (p.packet_type, p.clock_secs, p.payload.as_slice()))
            .collect();

        let updates = collect_arena_updates_filtered(&tuples, |_| true);
        let mut hist: BTreeMap<u32, usize> = BTreeMap::new();
        for u in &updates {
            *hist.entry(u.subtype).or_default() += 1;
        }
        println!("== {name}  (packets={} )", packets.len());
        let respawn_like: Vec<String> = hist
            .iter()
            .filter(|(k, _)| matches!(k, 18 | 19 | 20 | 21 | 24 | 25))
            .map(|(k, v)| format!("{}:{}({})", k, arena_subtype_name(*k), v))
            .collect();
        if respawn_like.is_empty() {
            println!("   复活类子类型: 无");
        } else {
            println!("   复活类子类型: {}", respawn_like.join("  "));
        }
        for u in updates.iter().filter(|u| matches!(u.subtype, 19 | 20)) {
            println!(
                "   t={:8.2} subtype={:2} {} payload[:40]={}",
                u.clock,
                u.subtype,
                u.name,
                u.payload.iter().take(40).map(|b| format!("{:02x}", b)).collect::<String>()
            );
        }

        let initial = collect_initial_hp(&tuples);
        let events = parse_hp_events(&tuples);
        let mut chain: BTreeMap<u32, Vec<(f32, u16)>> = BTreeMap::new();
        for (eid, (t0, h0)) in &initial {
            chain.entry(*eid).or_default().push((*t0, *h0));
        }
        for e in &events {
            let hp_v = if e.hp > 32767 { 0u16 } else { e.hp };
            let s = chain.entry(e.victim).or_default();
            if s.last().map(|(_, h)| *h) == Some(hp_v) {
                continue;
            }
            s.push((e.clock, hp_v));
        }
        for (eid, s) in &chain {
            let rose_after_zero = s.windows(2).any(|w| w[0].1 == 0 && w[1].1 > w[0].1);
            let tag = if rose_after_zero { "  <== 归零后回升" } else { "" };
            println!(
                "   eid={eid:<10} hp链 {} 项{}",
                s.len(),
                tag
            );
            if rose_after_zero {
                for (t, h) in s {
                    println!("      {:8.2} -> {}", t, h);
                }
            }
        }
        println!();
    }
}
