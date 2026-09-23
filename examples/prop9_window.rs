//! 1436: prop9 时序 40~50s（含 shot#4 开火 44.789）+ type10 pitch 对照
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("C:/Users/Administrator/AppData/Local/wotblitz/DAVAProject/replays/20260906_1436__Anonyme_T110_1155501458890492367.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut p9: Vec<(f32, f32)> = Vec::new();
    let mut hull: Vec<(f32, f32)> = Vec::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 16 && u32le(&p[4..8]) == 9 {
                    let alen = u32le(&p[8..12]) as usize;
                    if alen == 4 && 12 + 4 <= p.len() {
                        p9.push((pkt.clock_secs, f32le(&p[12..16])));
                    }
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 && u32le(&p[0..4]) == 0x100c7d6c {
                    hull.push((pkt.clock_secs, f32le(&p[40..44])));
                }
            }
            _ => {}
        }
    }
    let mut last_h = 0.0f32;
    println!("t      prop9(°)   hull pitch(°)");
    for (t, v) in &p9 {
        if *t < 40.0 || *t > 50.0 { continue; }
        let hp = hull.iter().rev().find(|(t2, _)| *t2 <= *t).map(|(_, v)| *v).unwrap_or(f32::NAN);
        if (*t * 10.0) as i32 % 2 == 0 {
            println!("{:8.3}  {:+7.2}   {:+7.2}", t, v * 57.29578, hp * 57.29578);
        }
    }
}
