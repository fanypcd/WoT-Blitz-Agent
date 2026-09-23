//! 探针：dump 射手车辆实体在开火前后的 type10 采样（时间/位置），检查滑块扩展窗口的数据覆盖。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).expect("usage: shooter_probe <file>");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // 找 shot#5：method29 @t≈50.64 的 shooter 实体
    let mut shooter_eid = 0u32;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 16 {
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if 12 + alen <= p.len() {
                    let a = &p[12..12 + alen];
                    if mid == 0x1d && alen >= 37 {
                        let sid = u32le(&a[4..8]);
                        if sid == 7006278 {  // shot#5 的 shotId
                            shooter_eid = u32le(&a[0..4]);
                            println!("method29 shotId=7006278 t={:.3} shooter={:08x}", pkt.clock_secs, shooter_eid);
                        }
                    }
                }
            }
        }
    }
    // 该实体的全部 type10 采样（开火 −4 ~ +1）
    println!("--- shooter type10 samples, clock ∈ [46.5, 51.8] ---");
    let mut n = 0;
    let mut prev: Option<(f32, [f32; 3])> = None;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 48 && u32le(&p[0..4]) == shooter_eid {
                let t = pkt.clock_secs;
                if t < 44.0 || t > 51.8 { continue; }
                let pos = [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])];
                let yaw = f32le(&p[36..40]);
                let d = prev.map(|(pt, pp)| (t - pt, (pos[0]-pp[0]).hypot(pos[2]-pp[2])));
                match d {
                    Some((dt, dist)) => println!("t={:.3} (fire{:+.2}) pos=({:.1},{:.1},{:.1}) yaw={:+.2} Δt={:.2} Δ={:.2}m v={:.1}m/s",
                        t, t - 50.64, pos[0], pos[1], pos[2], yaw, dt, dist, dist / dt.max(1e-3)),
                    None => println!("t={:.3} (fire{:+.2}) pos=({:.1},{:.1},{:.1}) yaw={:+.2}",
                        t, t - 50.64, pos[0], pos[1], pos[2], yaw),
                }
                prev = Some((t, pos));
                n += 1;
            }
        }
    }
    println!("samples in window: {n}");
}
