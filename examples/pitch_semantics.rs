//! 语义判定：type10 [40..44] = 车体俯仰 vs 炮管俯仰？
//! 方法：对每发 method29（弹速真值世界仰角已知），取【射手车辆】开火时刻
//! 的 type10 pitch，比对两者。若 |pitch - 世界仰角| << |pitch - 0| 且
//! 车体相对俯仰(弹速仰角-车体pitch假设)自洽，则 pitch=车体俯仰；
//! 若 pitch ≈ 弹速世界仰角，则 pitch=炮管世界俯仰。
//! 真值判别：炮管【车体相对】俯仰应通常在 [-depression, +elevation] 内（±8~15°），
//! 且车辆上下坡时随弹道而变。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut pose: HashMap<u32, Vec<(f32, f32, f32, f32)>> = HashMap::new(); // t, yaw, pitch, roll // t, pos, yaw, pitch, roll
    let mut fires: Vec<(f32, u32, f32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    pose.entry(u32le(&p[0..4])).or_default().push((pkt.clock_secs,
                        f32le(&p[36..40]), f32le(&p[40..44]), f32le(&p[44..48])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 49 {
                    let mid = u32le(&p[4..8]);
                    let alen = u32le(&p[8..12]) as usize;
                    if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                        let a = &p[12..12 + alen];
                        let sid = u32le(&a[4..8]);
                        if seen.insert(sid) {
                            let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                            let n = (v[0]*v[0]+v[1]*v[1]+v[2]*v[2]).sqrt();
                            fires.push((pkt.clock_secs, u32le(&a[0..4]), (v[1]/n).asin()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    println!("开火时刻: 弹速世界仰角 vs 该车 type10 pitch（同刻）:");
    println!("debug: fires={} pose_eids={}", fires.len(), pose.len());
    for (eid, seq) in pose.iter().take(40) {
        if let Some(last) = seq.last() { print!("  0x{:08x}@{:.1}({})", eid, last.0, seq.len()); }
    }
    println!();
    for (t, _pe, we) in fires.iter().take(16) {
        // 找该玩家的车辆 eid：method29 shooter 是玩家实体；type10 车辆——
        // 1436 已知: Anonyme 车=0x100c7d6c, _NeoP 车=0x100c7d6d（前面 veh_of_player 结果）
        // 这里直接枚举全部 type10 实体在 t 时刻的 pitch，列出与 we 最接近的
        let mut best: Option<(f32, u32, f32)> = None; // |pitch-we|, eid, pitch
        for (eid, seq) in &pose {
            // 找 t 前最近样本
            if let Some((pt, _y, pitch, _r)) = seq.iter().rev().find(|(pt, _, _, _)| *pt <= *t + 0.15) {
                if (*pt - *t).abs() > 0.15 { continue; }
                let d = (*pitch - *we).abs();
                if best.map(|(bd, _, _)| d < bd).unwrap_or(true) { best = Some((d, *eid, *pitch)); }
            }
        }
        if let Some((d, eid, pitch)) = best {
            println!("t={:8.3} 弹速仰角={:+7.2}°  最近实体 0x{:08x} type10 pitch={:+7.2}° |Δ|={:.2}°",
                t, we*57.2958, (eid & 0xff) as u8, pitch*57.2958, d*57.2958);
        }
    }
}
