//! J39 决定性语义检验：作者车辆的 prop2 到底是 yaw 还是 pitch？
//! 作者开了 14+ 炮，弹速仰角各不相同（−10°~+2°）。
//! 若 prop2 粗角 = 炮管俯仰 → prop2 粗角应随弹道仰角变化（500~517 域）
//! 若 prop2 = 炮塔偏航 → prop2 随炮塔转向变化，与仰角无关
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut p2: HashMap<u32, Vec<(f32, u16)>> = HashMap::new();
    let mut fires: Vec<(f32, u32, f32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
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
    // 每个有 prop2 的实体：取其开火序列，列 prop2 粗角与弹道仰角
    let mut shooters: Vec<u32> = Vec::new();
    for (_, pe, _) in &fires { if !shooters.contains(pe) { shooters.push(*pe); } }
    println!("开火射手实体: {:?}", shooters.iter().map(|e| format!("0x{:08x}", e)).collect::<Vec<_>>());
    for pe in &shooters {
        let seq = match p2.get(pe) { Some(s) if !s.is_empty() => s, _ => continue };
        println!("\n== 射手 0x{:08x}（prop2 {} 包） ==", pe, seq.len());
        for (ft, _fe, we) in fires.iter().filter(|(_, p, _)| *p == *pe) {
            if let Some((t2, u)) = seq.iter().rev().find(|(t2, _)| *t2 <= *ft + 0.05) {
                let coarse = u >> 6;
                let yaw = *u as f64 / 65535.0 * 360.0 - 180.0;
                let pitch = coarse as f64 * 0.7 - 360.0;
                let mut pw = pitch; while pw > 180.0 { pw -= 360.0; }
                println!("  t={:8.3} 弹道仰角={:+7.2}°  prop2=0x{:04x} coarse={:4} yaw解码={:+8.2}° pitch解码={:+8.2}° (wrap {:+.2})",
                    ft, we*57.29578, u, coarse, yaw, pitch, pw);
            }
        }
    }
}
