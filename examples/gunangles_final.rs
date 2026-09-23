//! 最终验证：type=7 prop4（751 包 u16@payload[0..2] 通过公式域筛选——可疑！）
//! 与 prop2 相同逐包对照：prop4.u16 >> 6 是否与 prop2 同步变化？
//! 以及 prop4 与开火真值在【双方车辆各自开火时刻】的全量对照。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // 名册 + type10 车辆位姿（找 method29 玩家 eid → 车辆 eid）
    let mut pose: HashMap<u32, ([f32; 3], f32)> = HashMap::new();
    let mut fires: Vec<(f32, u32, f32, [f32; 3])> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    pose.insert(u32le(&p[0..4]), (
                        [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])],
                        f32le(&p[36..40])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() < 49 { continue; }
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                    let a = &p[12..12 + alen];
                    let sid = u32le(&a[4..8]);
                    if seen.insert(sid) {
                        let ball = [f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21])];
                        let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                        let n = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
                        fires.push((pkt.clock_secs, u32le(&a[0..4]), (v[1]/n).asin(), ball));
                    }
                }
            }
            _ => {}
        }
    }
    // type=7 prop2 与 prop4 时序（按 eid）
    let mut prop_seq: HashMap<(u32, u32), Vec<(f32, u16)>> = HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 {
                let prop = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if (prop == 2 || prop == 4) && alen >= 2 && 12 + alen <= p.len() {
                    prop_seq.entry((u32le(&p[0..4]), prop)).or_default()
                        .push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                }
            }
        }
    }
    // 车辆 eid = type10 位置离 ball 最近的
    let mut veh_of_player: HashMap<u32, u32> = HashMap::new();
    for (t, pe, _e, ball) in &fires {
        if veh_of_player.contains_key(pe) { continue; }
        let mut best: Option<(f32, u32)> = None;
        for (eid, (pos, _)) in &pose {
            if pos[0] == 0.0 && pos[2] == 0.0 { continue; }
            let d = (pos[0]-ball[0]).powi(2) + (pos[2]-ball[2]).powi(2);
            if best.map(|(bd, _)| d < bd).unwrap_or(true) { best = Some((d, *eid)); }
        }
        if let Some((_, eid)) = best { veh_of_player.insert(*pe, eid); }
    }
    // prop4 解码值在开火时刻 vs 真值（世界仰角粗对照——prop4 或为车体系）
    println!("开火时刻 prop4 解码（coarse*0.7°−360° wrap）vs 弹速真值:");
    for (ft, pe, te, _ball) in fires.iter().take(20) {
        let veh = veh_of_player.get(pe);
        let mut line = format!("t={:8.3} veh={:?} 真值世界={:+.2}°", ft, veh, te * 57.29578);
        if let Some(ve) = veh {
            for prop in [2u32, 4u32] {
                if let Some(seq) = prop_seq.get(&(*ve, prop)) {
                    if let Some((t2, u)) = seq.iter().rev().find(|(t2, _)| *t2 <= *ft + 0.05) {
                        let coarse = (u >> 6) as f64;
                        let mut ang = coarse * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
                        while ang > std::f64::consts::PI { ang -= std::f64::consts::TAU; }
                        while ang < -std::f64::consts::PI { ang += std::f64::consts::TAU; }
                        line += &format!("  p{}={:+.2}°@Δ{:.2}", prop, ang * 57.29578, t2 - ft);
                    }
                }
            }
        }
        println!("{}", line);
    }
    // prop2 vs prop4 粗角相关性
    println!("\nprop2 vs prop4 粗角同帧对比（前 10 组双有值）:");
    let mut shown = 0;
    if let (Some(p2), Some(p4)) = (prop_seq.get(&(0x100c7d6c, 2)), prop_seq.get(&(0x100c7d6c, 4))) {
        for (t2, u2) in p2 {
            if shown >= 10 { break; }
            if let Some((t4, u4)) = p4.iter().find(|(t4, _)| (*t4 - *t2).abs() < 0.05) {
                println!("  t={:7.2} prop2.coarse={:4} prop4.coarse={:4} prop4.u={:5}", t2, u2 >> 6, u4 >> 6, u4);
                shown += 1;
            }
        }
    }
}
