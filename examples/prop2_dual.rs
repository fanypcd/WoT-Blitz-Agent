//! prop2 双解码检验：受击方(0x100c7d6c)的 prop2 u16 序列，
//! 奇偶/连续交替解读——yaw 解码 = u16/65535×360−180（+车体45.76=世界92→rel46.3→u16≈41162）
//! pitch 解码 = (u>>6)×0.7−360（pitch 4.4°→u16≈33250）
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut seq: Vec<(f32, u16)> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[0..4]) == 0x100c7d6d {
                let alen = u32le(&p[8..12]) as usize;
                if alen == 2 && 12 + 2 <= p.len() {
                    seq.push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                }
            }
        }
    }
    println!("受击方 prop2 共 {} 包。dt 相对 t=41.8（命中-3s）。双解码：", seq.len());
    println!("{:>8} {:>6}   yaw解码(rel°)  pitch解码(°)", "t-dt", "u16");
    for (t, u) in seq.iter().filter(|(t, _)| *t >= 43.5 && *t <= 46.0).take(1290) {
        let dt = t;
        
        let yaw_rel = *u as f64 / 65535.0 * 360.0 - 180.0;
        let pitch = ((u >> 6) as f64) * 0.7 - 360.0;
        println!("t={:8.3} 0x{:04x}   yaw_rel={:+8.2}°   pitch_dec={:+7.2}°", t, u, yaw_rel, pitch);
    }
}
