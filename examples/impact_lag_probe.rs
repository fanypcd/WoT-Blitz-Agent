//! 移动目标弹着点偏差构成测量：对每发命中弹输出——
//! 渲染锚点 vs 判定位置的偏差 |d|、目标速度 v（tick_samples 差分）、
//! 折算滞后时间 |d|/v、滤波器 latency 值。
//! 判据：|d|/v ≈ latency（0.1~0.2s）→ 与客户端显示滞后一致（固有）；
//!       |d|/v ≫ latency → 滤波器移植 bug。
//! 用法：cargo run --release --example impact_lag_probe -- <a.wotbreplay>
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
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
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
    let shots = combat_mod::extract_shot_replays_auto(&raw_packets, file_name).unwrap();

    println!("{:>3} {:>7} {:>6} {:>8} {:>8} {:>8} {:>9}  偏差方向(回放系 x,y,z)",
        "#", "t", "latency", "|d|渲染判定", "速度", "|d|/v", "latency差");
    for s in &shots {
        let Some(r) = &s.target_render else { continue };
        // 目标速度：tick_samples 是相对 tp 的位移序列（dt ∈ [-1, 0.09]），取跨 ≥0.4s 的两点差分
        let tks = &s.tick_samples;
        let mut speed = f32::NAN;
        'sp: for a in tks {
            if a.render { continue; }
            for b in tks.iter().rev() {
                if b.render || b.dt <= a.dt { continue; }
                if b.dt - a.dt >= 0.4 {
                    let dx = b.pos[0] - a.pos[0];
                    let dy = b.pos[1] - a.pos[1];
                    let dz = b.pos[2] - a.pos[2];
                    speed = (dx*dx + dy*dy + dz*dz).sqrt() / (b.dt - a.dt);
                    break 'sp;
                }
            }
        }
        let d = [
            r.pos[0] - s.target_pos[0],
            r.pos[1] - s.target_pos[1],
            r.pos[2] - s.target_pos[2],
        ];
        let dn = (d[0]*d[0] + d[1]*d[1] + d[2]*d[2]).sqrt();
        let lag_t = if speed > 0.5 { dn / speed } else { f32::NAN };
        let lag_diff = if lag_t.is_finite() { lag_t - r.latency } else { f32::NAN };
        println!("{:>3} {:>7.2} {:>6.3} {:>8.2}m {:>7.2}m/s {:>7.3}s {:>+8.3}s  ({:+.2},{:+.2},{:+.2})",
            s.index, s.time_s, r.latency, dn, speed, lag_t, lag_diff, d[0], d[1], d[2]);
    }
    let _ = std::io::stdout().flush();
}
