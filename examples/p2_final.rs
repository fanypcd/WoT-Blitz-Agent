
use std::collections::HashMap;
fn main() {
    let path = "C:/Users/Administrator/AppData/Local/wotblitz/DAVAProject/replays/20260906_1436__Anonyme_T110_1155501458890492367.wotbreplay";
    let f = std::fs::File::open(path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut p2: HashMap<u32, Vec<(f32, u16)>> = HashMap::new();
    let mut t10: HashMap<u32, Vec<(f32, f32, f32)>> = HashMap::new(); // yaw pitch roll
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[4..8]) == 2 {
                    let alen = u32le(&p[8..12]) as usize;
                    if alen == 2 && 12 + 2 <= p.len() {
                        p2.entry(u32le(&p[0..4])).or_default().push((
                            pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                    }
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    t10.entry(u32le(&p[0..4])).or_default().push((
                        pkt.clock_secs, f32le(&p[36..40]), f32le(&p[40..44])));
                }
            }
            _ => {}
        }
    }
    for eid in [0x100c7d6cu32, 0x100c7d6du32] {
        println!("===== eid=0x{:08x} =====", eid);
        let seq = &p2[&eid];
        let hull = &t10[&eid];
        // 每 0.5s 打一对
        let mut next = 40.0f32;
        for (t, u) in seq {
            if *t < 40.0 || *t > 47.0 { continue; }
            if *t >= next {
                next += 0.5;
                let yaw_rel = *u as f64 / 65535.0 * 360.0 - 180.0;
                let pitch_dec = ((u >> 6) as f64) * 0.7 - 360.0;
                // 最近的 hull yaw
                let hy = hull.iter().rev().find(|(t2, _, _)| *t2 <= *t)
                    .map(|(_, y, _)| *y).unwrap_or(0.0);
                println!("  t={:8.3} u16=0x{:04x} yaw_rel={:+8.2}° (hull {:+8.2} → world {:+8.2})   pitch解码={:+8.2}°",
                    t, u, yaw_rel, hy*57.29578, (yaw_rel + hy as f64)*57.29578, pitch_dec);
            }
        }
    }
}
