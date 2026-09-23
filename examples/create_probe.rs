//! type=5 车辆全量状态包探针：每实体首条 type=5 的 offset-51 u16（= 初始 HP 候选）
//! 与该实体全部 method1 HP 事件对照，验证 offset 51 的稳定性。
//! 用法：cargo run --example create_probe -- <file>
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    // 实体 → (首条 type=5 (t, hp@51, len), method1 事件列表)
    let mut first5: BTreeMap<u32, (f32, u16, usize, usize)> = BTreeMap::new();
    let mut hp1: BTreeMap<u32, Vec<(f32, u16)>> = BTreeMap::new();
    for pkt in &data.packets {
        let p = &pkt.raw_payload[..];
        if p.len() < 4 { continue; }
        let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } => {
                if p.len() >= 53 && !first5.contains_key(&eid) {
                    first5.insert(eid, (pkt.clock_secs, u16le(&p[51..53]), p.len(), 51));
                }
            }
            _ => {
                // method1: [eid][01][alen=7][hp u16][src u32][cause]
                if p.len() >= 19
                    && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 0x01
                    && u32::from_le_bytes([p[8], p[9], p[10], p[11]]) == 7
                {
                    hp1.entry(eid).or_default().push((pkt.clock_secs, u16le(&p[12..14])));
                }
            }
        }
    }
    println!("{:<12} {:>8} {:>6} {:>5} | method1 首条 (t, hp)", "eid", "t5_t", "hp51", "len");
    for (eid, (t, hp, len, _off)) in &first5 {
        let m1 = hp1.get(eid).and_then(|v| v.first());
        let m1s = m1.map(|(t, h)| format!("t={:7.3} hp={}", t, h)).unwrap_or_else(|| "-".into());
        println!("{:08x}   {:8.3} {:6} {:5} | {}", eid, t, hp, len, m1s);
    }
}
