//! 纯俯仰测试回放：prop9（作者车辆 f32 流）+ type10 pitch + prop2 三流并列，
//! 用户动作 = 只做炮管俯仰上下 → 找随动作变化的流
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut p9: Vec<(f32, f32)> = Vec::new();
    let mut p9b: Vec<(f32, u32, f32)> = Vec::new();
    let mut t10: Vec<(f32, u32, f32, f32)> = Vec::new(); // t, eid, pitch, yaw
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 16 && u32le(&p[4..8]) == 9 {
                    let alen = u32le(&p[8..12]) as usize;
                    if alen == 4 && 12 + 4 <= p.len() {
                        let v = f32le(&p[12..16]);
                        p9.push((pkt.clock_secs, v));
                        p9b.push((pkt.clock_secs, u32le(&p[0..4]), v));
                    }
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    t10.push((pkt.clock_secs, u32le(&p[0..4]), f32le(&p[40..44]), f32le(&p[36..40])));
                }
            }
            _ => {}
        }
    }
    println!("prop9 共 {} 包", p9.len());
    let mn = p9.iter().map(|(_, v)| v).cloned().fold(f32::MAX, f32::min);
    let mx = p9.iter().map(|(_, v)| v).cloned().fold(f32::MIN, f32::max);
    println!("prop9 值域: {:+.4} ~ {:+.4} rad ({:+.2}° ~ {:+.2}°)", mn, mx, mn*57.29578, mx*57.29578);
    let mut seen = std::collections::HashMap::new();
    for (t, e, p, _y) in &t10 {
        seen.entry(*e).or_insert((*p, *p));
        let en = seen.get_mut(e).unwrap();
        en.0 = en.0.min(*p); en.1 = en.1.max(*p);
    }
    for (e, (a, b)) in &seen {
        println!("type10 eid=0x{:08x} pitch {:+.3}~{:+.3} rad", e, a, b);
    }
    // 合流打印 t=10~18（俯仰动作窗口）: prop9 与 type10 pitch
    let t10a = &t10;
    println!("\nt      prop9(rad)  prop9(°)   type10 pitch(°)  type10 yaw(°)");
    for (t, v) in &p9 {
        
        let tp = t10a.iter().rev().find(|(t2, _, _, _)| *t2 <= *t && *t - *t2 < 2.0);
        let (tpv, tyv) = match tp { Some((_, _, p, y)) => (p*57.29578, y*57.29578), None => (f32::NAN, f32::NAN) };
        println!("{:8.3}  {:+8.4}  {:+7.2}°   {:+7.2}°        {:+7.2}°", t, v, v*57.29578, tpv, tyv);
    }
}
