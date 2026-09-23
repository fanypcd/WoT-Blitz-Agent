//! 受击方 3 个 create 包全字段三方对比 + 各时刻 type10 pitch / prop2，
//! 找与 pitch 线性相关的字段（俯仰初值候选）。
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("C:/Users/Administrator/AppData/Local/wotblitz/DAVAProject/replays/20260906_1436__Anonyme_T110_1155501458890492367.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut pitch: HashMap<u32, Vec<(f32, f32)>> = HashMap::new();
    let mut cre: Vec<(f32, Vec<u8>)> = Vec::new();
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
                if p.len() >= 52 && u32le(&p[0..4]) == 0x100c7d6c {
                    cre.push((pkt.clock_secs, p.to_vec()));
                }
            }
            _ => {}
        }
    }
    // 各 create 后最近的 type10 pitch
    let mut ps: Vec<(f32, f32, f32, f32)> = Vec::new(); // t, pitch, yaw, roll
    for (t, p) in &cre {
        let hp = pitch.get(&0x100c7d6c).and_then(|m| m.iter()
            .find(|(pt, _)| *pt >= *t && *pt <= t + 0.3))
            .map(|(_, v)| *v).unwrap_or(f32::NAN) * 57.29578;
        ps.push((*t, hp, f32::NAN, f32::NAN));
    }
    // 全字段 u16 表 + type10 pitch 行
    println!("offset  create#1({:.1}) create#2({:.1}) create#3({:.1})  | type10pitch(°): {:+.2} {:+.2} {:+.2}",
        ps[0].0, ps[1].0, ps[2].0, ps[0].1, ps[1].1, ps[2].1);
    let n = cre.iter().map(|(_, p)| p.len()).min().unwrap();
    let mut i = 0;
    while i + 1 < n {
        let u = [cre[0].1[i] as u16 | ((cre[0].1[i+1] as u16) << 8),
                 cre[1].1[i] as u16 | ((cre[1].1[i+1] as u16) << 8),
                 cre[2].1[i] as u16 | ((cre[2].1[i+1] as u16) << 8)];
        if !(u[0] == u[1] && u[1] == u[2]) {
            println!("[{:3}] {:6} {:6} {:6}", i, u[0], u[1], u[2]);
        }
        i += 2;
    }
}
