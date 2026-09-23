//! J39 扩样本：全部 create 包的 [48..56] 字段 + 进世界后 type10 pitch，
//! 拟合 [54]/[55] 字段 → pitch 映射公式。
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut pitch: HashMap<u32, Vec<(f32, f32)>> = HashMap::new();
    let mut cre: Vec<(f32, u32, Vec<u8>)> = Vec::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    pitch.entry(u32le(&p[0..4])).or_default().push((
                        pkt.clock_secs, f32le(&p[40..44])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 56 {
                    cre.push((pkt.clock_secs, u32le(&p[0..4]), p.to_vec()));
                }
            }
            _ => {}
        }
    }
    println!("eid        t       u16@48  byte54 byte55 | type10pitch@+0.3");
    for (t, eid, p) in &cre {
        let hp = pitch.get(eid).and_then(|m| m.iter()
            .find(|(pt, _)| *pt >= *t && *pt <= t + 0.3))
            .map(|(_, v)| *v).unwrap_or(f32::NAN) * 57.29578;
        let u48 = u16::from_le_bytes([p[48], p[49]]);
        println!("0x{:08x} {:8.3} 0x{:04x} ({:3},{:3})  | {:+7.2}°",
            eid, t, u48, p[54], p[55], hp);
    }
    println!("共 {} 个 create 包", cre.len());
    let _ = f32le;
}
