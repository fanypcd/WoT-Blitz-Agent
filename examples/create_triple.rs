//! 受击方 3 个 create 包：提取 u16@[48..50] 与其它候选字段，
//! 对照各进世界时刻的 prop2（炮塔）与 type10 pitch（车体俯仰），解出字段映射。
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("C:/Users/Administrator/AppData/Local/wotblitz/DAVAProject/replays/20260906_1436__Anonyme_T110_1155501458890492367.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut p2: HashMap<u32, Vec<(f32, u16)>> = HashMap::new();
    let mut pitch: HashMap<u32, Vec<(f32, f32)>> = HashMap::new();
    let mut cre: Vec<(f32, Vec<u8>)> = Vec::new();
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
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    pitch.entry(u32le(&p[0..4])).or_default().push((
                        pkt.clock_secs, f32le(&p[40..44])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 52 && u32le(&p[0..4]) == 0x100c7d6c {
                    cre.push((pkt.clock_secs, p.to_vec()));
                }
            }
            _ => {}
        }
    }
    for (t, p) in &cre {
        let yaw16 = u16::from_le_bytes([p[48], p[49]]);
        let other16 = u16::from_le_bytes([p[50], p[51]]);
        // 进世界后最近 type10 pitch（+0.2s 内）
        let hp = pitch.values().flat_map(|m| m.iter())
            .find(|(pt, _)| *pt >= *t && *pt <= t + 0.3).map(|(_, v)| *v).unwrap_or(f32::NAN);
        // 进世界后最近 prop2
        let p2v = p2.get(&0x100c7d6c).and_then(|s| s.iter()
            .filter(|(pt, _)| *pt >= *t && *pt <= t + 3.0).next()).map(|(_, v)| *v);
        let c48 = yaw16 >> 6; let f48 = yaw16 & 63;
        println!("create t={:8.3} u16@48=0x{:04x} coarse={} fine={} | u16@50=0x{:04x} | type10 pitch@+0.2={:+.2}° | prop2近期={:?}",
            t, yaw16, c48, f48, other16, hp * 57.2958, p2v);
        // 双解码
        let yaw_dec = yaw16 as f64 / 65535.0 * 360.0 - 180.0;
        let pitch_coarse = (yaw16 >> 6) as f64;
        println!("           u16@48 解码: yaw式={:+.2}°  pitch式(coarse×0.7−360)={:+.2}°  pitch式(coarse×0.35−180)={:+.2}°",
            yaw_dec, pitch_coarse * 0.7 - 360.0, pitch_coarse * 0.35 - 180.0);
        let _ = f32le;
    }
}
