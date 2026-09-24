//! WI hash6 语义裁决探针：对每发直击弹（method8）输出——
//! 原始 hash6、LE/BE 两种解码的 yaw/pitch、命中时刻几何真值
//! （世界系方位角 target→muzzle、目标车体相对方位、几何抵达角），
//! 裁决 hash6 字节序与参考系（车体系 vs 世界系）。
//! 用法：cargo run --release --example wi_align_probe -- <a.wotbreplay>
use std::io::Write;

mod replay_shim {
    #[path = "../../src/replay/filter.rs"]
    pub mod filter;
    #[path = "../../src/replay/combat.rs"]
    pub mod combat;
}
use replay_shim::combat as combat_mod;

fn norm_pi(a: f32) -> f32 {
    let mut x = a;
    while x > std::f32::consts::PI {
        x -= 2.0 * std::f32::consts::PI;
    }
    while x < -std::f32::consts::PI {
        x += 2.0 * std::f32::consts::PI;
    }
    x
}

fn main() {
    let path = std::env::args().nth(1).expect("usage");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();

    let raw_packets: Vec<(u32, f32, &[u8])> = data
        .packets
        .iter()
        .map(|pkt| {
            let t = match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            };
            (t, pkt.clock_secs, &pkt.raw_payload[..])
        })
        .collect();

    // method8 直击弹原始扫描（偏移同 combat.rs collect_direct_hits8）
    let mut hits: Vec<(f32, u32, u32, [u8; 6], Option<([f32; 3], [f32; 3])>)> = Vec::new();
    let mut pose: std::collections::HashMap<u32, ([f32; 3], [f32; 3], f32)> = Default::default();
    for (t2, clock, p) in &raw_packets {
        if *t2 == 10 && p.len() >= 48 {
            let g = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
            pose.insert(
                u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                ([g(12), g(16), g(20)], [g(36), g(40), g(44)], *clock),
            );
            continue;
        }
        if p.len() < 22 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x08 {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 17 || 12 + args_len > p.len() {
            continue;
        }
        let a = &p[12..12 + args_len];
        if a[8] != 0x01 {
            continue;
        }
        let shooter = u32::from_le_bytes([a[0], a[1], a[2], a[3]]);
        let victim = u32::from_le_bytes([a[4], a[5], a[6], a[7]]);
        let hash6 = [a[11], a[12], a[13], a[14], a[15], a[16]];
        hits.push((*clock, shooter, victim, hash6, pose.get(&victim).map(|(p2, ang, _)| (*p2, *ang))));
    }
    hits.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    let shots = combat_mod::extract_shot_replays_auto(
        &raw_packets,
        std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or(""),
    ).unwrap();

    println!(
        "{:>4} {:>8} {:>7} | {:>8} {:>8} | {:>8} {:>8} {:>8} | {:>8} {:>8}",
        "#", "t", "dist",
        "yawLE", "yawBE",
        "worldB", "hullB", "hullYaw",
        "pitLE", "pitBE"
    );
    for (i, s) in shots.iter().enumerate() {
        // 配对 method8（时间最近）
        let hit = hits.iter().enumerate().min_by(|a, b| {
            (a.1.0 - s.time_s).abs().partial_cmp(&(b.1.0 - s.time_s).abs()).unwrap()
        });
        let Some((_hi, (t8, _sh, _vi, h6, victim_state))) = hit else { continue };
        if (*t8 - s.time_s).abs() > 0.15 {
            continue;
        }
        let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]) as f32;
        let u16be = |b: &[u8]| u16::from_be_bytes([b[0], b[1]]) as f32;
        let yaw_le = (u16le(&h6[2..4]) - 32768.0) / 32768.0 * std::f32::consts::PI;
        let yaw_be = (u16be(&h6[2..4]) - 32768.0) / 32768.0 * std::f32::consts::PI;
        let pit_le = (u16le(&h6[4..6]) - 32768.0) / 32768.0 * (std::f32::consts::FRAC_PI_2);
        let pit_be = (u16be(&h6[4..6]) - 32768.0) / 32768.0 * (std::f32::consts::FRAC_PI_2);

        // 几何真值：muzzle（method29 炮口）与 method8 通知状态目标锚点
        let mz = s.ball_a;
        let tp = s.target_pos;
        let dx = mz[0] - tp[0];
        let dz = mz[2] - tp[2];
        let world_b = norm_pi(f32::atan2(dx, dz));
        let hull_yaw = s.target_ang[0];
        let hull_b = norm_pi(world_b - hull_yaw);
        let horiz = (dx * dx + dz * dz).sqrt();
        let geo_pitch = f32::atan2(tp[1] - mz[1], horiz);
        let v = s.launch_velocity;
        let launch_pitch = f32::atan2(v[1], (v[0] * v[0] + v[2] * v[2]).sqrt());

        println!(
            "{:>4} {:>8.2} {:>7.3} | {:+8.3} {:+8.3} | {:+8.3} {:+8.3} {:+8.3} | {:+8.3} {:+8.3}   h6={:02x}{:02x}{:02x}{:02x}{:02x}{:02x}  geoPit={:+8.3} muzzleH={:.2} tgtY={:.2}",
            i, s.time_s, {
                let ddx = s.shooter_pos[0] - tp[0];
                let ddy = s.shooter_pos[1] - tp[1];
                let ddz = s.shooter_pos[2] - tp[2];
                (ddx * ddx + ddy * ddy + ddz * ddz).sqrt()
            },
            yaw_le.to_degrees(), yaw_be.to_degrees(),
            world_b.to_degrees(), hull_b.to_degrees(), hull_yaw.to_degrees(),
            pit_le.to_degrees(), pit_be.to_degrees(),
            h6[0], h6[1], h6[2], h6[3], h6[4], h6[5],
            geo_pitch.to_degrees(), mz[1], tp[1],
        );
        println!(
            "     launchPitch={:+.3} aimRel=[{:+.2},{:+.2},{:+.2}] seg32={:016x} incDir={:?}",
            launch_pitch.to_degrees(),
            s.aim_point[0] - tp[0], s.aim_point[1] - tp[1], s.aim_point[2] - tp[2],
            s.segment, s.target_inc_dir,
        );
        let _ = victim_state;
    }
    let _ = std::io::stdout().flush();
}
