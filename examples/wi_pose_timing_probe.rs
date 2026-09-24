//! WI 射击复现的姿态/位置取样时刻考古（流序版）：
//! WI 服务端是单遍流式解析器 —— 假设其状态快照 = method8 包在包流中出现之前的
//! 最后已知 type10 位置 / prop2 打包角（流序，非时钟序）。对照 battle.json 的
//! turret_yaw（coarse10 精确量化）与 distance 逐发验证。
//! 用法：cargo run --release --example wi_pose_timing_probe -- <replay> <battle.json>
use std::collections::HashMap;

struct Hit8 {
    t: f32,
    shooter: u32,
    victim: u32,
    hash6: [u8; 6],
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let replay_path = &args[1];
    let battle_path = &args[2];

    let f = std::fs::File::open(replay_path).unwrap();
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

    // 流序索引（不排序）
    let mut prop2: HashMap<u32, Vec<(usize, f32, u16)>> = HashMap::new();
    let mut st10: HashMap<u32, Vec<(usize, f32, [f32; 3])>> = HashMap::new();
    let mut hits8: Vec<(usize, Hit8)> = Vec::new();

    for (pi, (t2, clock, p)) in raw_packets.iter().enumerate() {
        if *t2 == 10 && p.len() >= 48 {
            let g = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
            st10
                .entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
                .or_default()
                .push((pi, *clock, [g(12), g(16), g(20)]));
            continue;
        }
        if *t2 == 7 && p.len() >= 14 && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2 {
            let v = u16::from_le_bytes([p[12], p[13]]);
            prop2
                .entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
                .or_default()
                .push((pi, *clock, v));
            continue;
        }
        if *t2 == 8 && p.len() >= 12 {
            let method = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
            let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
            if 12 + args_len > p.len() {
                continue;
            }
            let a = &p[12..12 + args_len];
            if method == 0x08 && args_len >= 17 && a[8] == 0x01 {
                hits8.push((
                    pi,
                    Hit8 {
                        t: *clock,
                        shooter: u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                        victim: u32::from_le_bytes([a[4], a[5], a[6], a[7]]),
                        hash6: [a[11], a[12], a[13], a[14], a[15], a[16]],
                    },
                ));
            }
        }
    }

    // 流序最后已知：包索引 < idx 的最后一条
    let lk_stream = |v: &[(usize, f32, u16)], idx: usize| -> Option<(f32, u16)> {
        v.iter().take_while(|(i, _, _)| *i < idx).last().map(|(_, c, v)| (*c, *v))
    };
    let pos_stream = |v: &[(usize, f32, [f32; 3])], idx: usize| -> Option<[f32; 3]> {
        v.iter().take_while(|(i, _, _)| *i < idx).last().map(|(_, _, p)| *p)
    };
    let dist = |a: [f32; 3], b: [f32; 3]| -> f64 {
        let dx = (a[0] - b[0]) as f64;
        let dy = (a[1] - b[1]) as f64;
        let dz = (a[2] - b[2]) as f64;
        (dx * dx + dy * dy + dz * dz).sqrt()
    };

    let battle: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(battle_path).unwrap()).unwrap();
    let shots = battle["shots"].as_array().unwrap();

    let coarse_of = |yaw_deg: f64| -> i64 { ((((yaw_deg + 180.0) / 360.0) * 1024.0).round() as i64).rem_euclid(1024) };

    println!("== 流序锚定（method8 包索引前最后已知）==");
    println!("{:>3} {:>3} {:>8} | {:>4} {:>4} {:>5} {:>6} | {:>10} {:>10} {:>7}",
        "#", "Y", "yaw_WI°", "cW", "cV流", "Δc", "采样Δt", "WI_dist", "D流序", "ΔD");
    let (mut yaw_ok, mut dist_ok, mut n) = (0, 0, 0);
    for (i, s) in shots.iter().enumerate() {
        let seg = s["segment"].as_str().unwrap().parse::<u64>().unwrap();
        let seg_b = seg.to_le_bytes();
        let hash6: [u8; 6] = [seg_b[2], seg_b[3], seg_b[4], seg_b[5], seg_b[6], seg_b[7]];
        let Some((idx8, hit)) = hits8.iter().find(|(_, h)| h.hash6 == hash6) else { continue };
        let idx8 = *idx8;
        let yaw_wi = s["turret_yaw"].as_f64().unwrap().to_degrees();
        let dist_wi = s["distance"].as_f64().unwrap();
        let c_wi = coarse_of(yaw_wi);

        let vic_prop = prop2.get(&hit.victim);
        let (c_stream, clk_v) = vic_prop
            .and_then(|v| lk_stream(v, idx8))
            .map(|(c, v)| ((v >> 6) as i64, c))
            .unwrap_or((-1, f32::NAN));

        let vp = st10.get(&hit.victim).and_then(|v| pos_stream(v, idx8));
        let sp = st10.get(&hit.shooter).and_then(|v| pos_stream(v, idx8));
        let d_stream = match (vp, sp) { (Some(a), Some(b)) => dist(a, b), _ => f64::NAN };

        let ym = c_stream == c_wi;
        let dd = d_stream - dist_wi;
        let dm = dd.abs() < 1e-3;
        n += 1;
        yaw_ok += ym as i32;
        dist_ok += dm as i32;
        println!(
            "{:>3} {:>6} {:>8.3} | {:>4} {:>4} {:>5} {:>+6.2} | {:>10.4} {:>10.4} {:>+7.4}",
            i, if ym { "Y" } else { "." }, yaw_wi, c_wi, c_stream, c_wi - c_stream,
            hit.t as f64 - clk_v as f64, dist_wi, d_stream, dd
        );
    }
    println!("\n炮塔 yaw 流序匹配：{}/{}；距离 μ 级匹配（|ΔD|<1mm）：{}/{}", yaw_ok, n, dist_ok, n);
}
