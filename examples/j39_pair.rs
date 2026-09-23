//! J39: WI 99 发 gun_pitch 真值 vs 来弹方向俯仰（受击方车体系）配对检验。
//! WI shot: time(命中时刻), target(account_id), gun_pitch。
//! 回放: method29(开火, 弹道直线), method14(终点), type10(受击方姿态)。
//! 受击方 account→entity 用 type5 昵称桥接近似（J39 已有映射先例）。
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // WI shots
    let wi_text = std::fs::read_to_string("tmp_wi2/wi_shots.json").unwrap();
    let wi: serde_json::Value = serde_json::from_str(&wi_text).unwrap();
    // method29 + type10 + method14(终点, 16B args = [shotId u32][endPoint 12B])
    let mut fires: Vec<(f32, u32, [f32; 3], [f32; 3])> = Vec::new(); // t, shooterP, ball, vel
    let mut seen = std::collections::HashSet::new();
    let mut pose: HashMap<u32, ([f32; 3], f32, f32, f32)> = HashMap::new();
    let mut endpoints: HashMap<u32, [f32; 3]> = HashMap::new(); // shotId → end
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() < 16 { continue; }
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if 12 + alen > p.len() { continue; }
                let a = &p[12..12 + alen];
                if mid == 0x1d && alen >= 37 {
                    let sid = u32le(&a[4..8]);
                    if seen.insert(sid) {
                        fires.push((pkt.clock_secs, u32le(&a[0..4]),
                            [f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21])],
                            [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])]));
                    }
                } else if mid == 0x14 && alen >= 16 {
                    let sid = u32le(&a[0..4]);
                    endpoints.insert(sid, [f32le(&a[4..8]), f32le(&a[8..12]), f32le(&a[12..16])]);
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    pose.insert(u32le(&p[0..4]), (
                        [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])],
                        f32le(&p[36..40]), f32le(&p[40..44]), f32le(&p[44..48])));
                }
            }
            _ => {}
        }
    }
    // 对每个 method29, 若其 endpoint 存在 → 弹道弦已知。
    // 找弦最近的车（受击方）→ 来弹方向在受击方车体系的俯仰。
    // 输出 (开火时刻, 受击方车体相对来弹俯仰, 受击方 pitch, 世界来弹俯仰)。
    println!("fire_t  victimVeh  来弹世界俯仰  受击方pitch  车体相对俯仰");
    let mut rows: Vec<(f32, f32, f32, f32)> = Vec::new();
    for (t, _sp, ball, vel) in &fires {
        // 找 endpoint: 该发弹道 t 之后最近的 method14?——用 shotId 关联缺失（fires 无 sid）
        // 简化: 弹道方向 vel 归一, 找与 vel 方向线最近的 pose 车
        let n = (vel[0]*vel[0] + vel[1]*vel[1] + vel[2]*vel[2]).sqrt();
        if n < 1.0 { continue; }
        let d = [vel[0]/n, vel[1]/n, vel[2]/n];
        let mut best: Option<(f32, u32, f32)> = None; // dist², eid, param t
        for (eid, (pos, _, _, _)) in &pose {
            if pos[0] == 0.0 && pos[2] == 0.0 { continue; }
            let ap = [pos[0]-ball[0], pos[1]-ball[1], pos[2]-ball[2]];
            let tt = ap[0]*d[0] + ap[1]*d[1] + ap[2]*d[2];
            if tt < 0.0 { continue; }
            let cl = [ap[0]-d[0]*tt, ap[1]-d[1]*tt, ap[2]-d[2]*tt];
            let dist2 = cl[0]*cl[0] + cl[1]*cl[1] + cl[2]*cl[2];
            if dist2 < 16.0 && best.map(|(bd, _, _)| dist2 < bd).unwrap_or(true) {
                best = Some((dist2, *eid, tt));
            }
        }
        if let Some((_, veid, _)) = best {
            let p = &pose[&veid];
            // 世界来弹俯仰（来向 = -d）
            let inc_pitch = (-d[1]).asin();
            // 车体系相对俯仰 ≈ 来弹俯仰 - 车体 pitch（忽略 roll 混合）
            let rel = inc_pitch - p.2;
            rows.push((*t, inc_pitch, p.2, rel));
            println!("{:8.3}  {:08x}  {:+7.2}°  {:+7.2}°  {:+7.2}°",
                t, (veid & 0xff) as u8, inc_pitch*57.2958, p.2*57.2958, rel*57.2958);
        }
    }
    let _ = &wi;
    let _ = &endpoints;
}
