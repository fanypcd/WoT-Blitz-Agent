//! 决定性验证：type=7 prop 流按 propID 分组，检查是否存在
//! "每 propID 解码 pitch = coarse*2π*7/3600−2π" 与 prop2 炮塔角配对成 (yaw,pitch) 的通道。
//! 重点：type=7 的 u16 全域枚举 + 与开火真值对照（按 eid 分组）。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // 玩家 eid → 昵称（type5）；method29 真值
    let mut nick_of_eid: HashMap<u32, String> = HashMap::new();
    let mut fires: Vec<(f32, u32, f32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } => {
                let p = &pkt.raw_payload[..];
                if p.len() < 60 { continue; }
                let eid = u32le(&p[0..4]);
                let l = p[57] as usize;
                if !(3..=30).contains(&l) || 58 + l > p.len() { continue; }
                if let Ok(name) = std::str::from_utf8(&p[58..58 + l]) { nick_of_eid.insert(eid, name.to_string()); }
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
                        let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                        let n = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
                        fires.push((pkt.clock_secs, u32le(&a[0..4]), (v[1]/n).asin()));
                    }
                }
            }
            _ => {}
        }
    }
    // type=7: propID = u32@[4..8]; 对每个 propID，把 u16@[12..14] 当 pitch_p 解码
    // 按实体分桶。输出每 propID 的 (包数, 与真值mae)
    let mut best_per_prop: HashMap<u32, (usize, f32, usize)> = HashMap::new(); // prop → (count, mae, n)
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 14 { continue; }
            let prop = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if alen < 2 || 12 + alen > p.len() { continue; }
            let pitch_p = u16le(&p[12..14]);
            if pitch_p == 0 { continue; }
            let coarse = (pitch_p >> 6) as f64;
            let mut pitch = coarse * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
            while pitch > std::f64::consts::PI { pitch -= std::f64::consts::TAU; }
            while pitch < -std::f64::consts::PI { pitch += std::f64::consts::TAU; }
            if pitch.abs() > 0.35 { continue; }
            let t = pkt.clock_secs;
            let e = best_per_prop.entry(prop).or_insert((0, f32::MAX, 0));
            e.0 += 1;
            let mut err_sum = 0.0; let mut n = 0;
            for (ft, _pe, te) in &fires {
                if (*ft - t).abs() <= 0.12 {
                    err_sum += (pitch as f32 - te).abs(); n += 1;
                }
            }
            if n > 0 {
                let mae = err_sum / n as f32;
                if mae < e.1 { e.1 = mae; e.2 = n; }
            }
        }
    }
    println!("type=7 propID → count, best fire-mae, n:");
    let mut v: Vec<(u32, (usize, f32, usize))> = best_per_prop.into_iter().collect();
    v.sort_by_key(|(p, _)| *p);
    for (p, (c, mae, n)) in v {
        println!("  prop {:2}: count={:5} best_mae={:.3}° (n={})", p, c, mae, n);
    }
    let _ = nick_of_eid;
}
