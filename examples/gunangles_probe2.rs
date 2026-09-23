//! 第二轮验证：在 method0x10 包参数区尝试多种布局解码，
//! 并打印参数 hex 供人工判读。真值 = method29 弹速仰角（按玩家实体名就近配对车辆）。
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid != 0x10 || 12 + alen > p.len() { continue; }
            let a = &p[12..12 + alen];
            let hexv: Vec<String> = a.iter().map(|b| format!("{:02x}", b)).collect();
            let f32s: Vec<String> = a.chunks(4).filter(|c| c.len() == 4)
                .map(|c| format!("{:.4}", f32le(c))).collect();
            println!("t={:8.3} eid=0x{:08x} alen={} args=[{}] f32=[{}]",
                pkt.clock_secs, u32le(&p[0..4]), alen, hexv.join(" "), f32s.join(" "));
        }
    }
    let _ = f32le;
}
