
use std::collections::HashMap;
fn main() {
    let path = "replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay";
    let f = std::fs::File::open(path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // method29 launches (player eid) + type10 poses (vehicle eid) + 玩家→车辆映射
    let mut fires: Vec<(f32, u32, f32, [f32;3])> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pose: HashMap<u32, ([f32;3], f32, f32, f32)> = HashMap::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 49 {
                    let mid = u32le(&p[4..8]);
                    let alen = u32le(&p[8..12]) as usize;
                    if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                        let a = &p[12..12 + alen];
                        let sid = u32le(&a[4..8]);
                        if seen.insert(sid) {
                            let ball = [f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21])];
                            let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                            let n = (v[0]*v[0]+v[1]*v[1]+v[2]*v[2]).sqrt();
                            fires.push((pkt.clock_secs, u32le(&a[0..4]), (v[1]/n).asin(), ball));
                        }
                    }
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
    // 玩家 eid → 车辆 eid（位置最近）
    let mut veh_of_player: HashMap<u32, u32> = HashMap::new();
    for (t, pe, _, ball) in &fires {
        if veh_of_player.contains_key(pe) { continue; }
        let mut best: Option<(f32, u32)> = None;
        for (eid, (pos, _, _, _)) in &pose {
            if pos[0] == 0.0 && pos[2] == 0.0 { continue; }
            let d = (pos[0]-ball[0]).powi(2) + (pos[2]-ball[2]).powi(2);
            if best.map(|(bd, _)| d < bd).unwrap_or(true) { best = Some((d, *eid)); }
        }
        if let Some((_, eid)) = best { veh_of_player.insert(*pe, eid); }
    }
    // 车体系相对俯仰 = 世界仰角 - 车体 pitch（近似）+ 对照 WI 值
    println!("t      世界仰角   车体pitch   车体相对  (WI gun_pitch 见 json 侧对照)");
    for (t, pe, we, _ball) in fires.iter().take(12) {
        let veh = veh_of_player.get(pe);
        let hp = veh.and_then(|v| pose.get(v)).map(|p| p.2).unwrap_or(0.0);
        println!("{:8.3}  {:+7.2}°  {:+7.2}°  {:+7.2}°", t, we*57.2958, hp*57.2958, (we-hp)*57.2958);
    }
}
