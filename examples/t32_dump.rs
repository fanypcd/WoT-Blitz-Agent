
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // 25B: [0..4]eid [4]=00 [5..9]=00 00 00 a8(变) [9..11]=80 0b/40 09/80 0b/00 0d
    //      [11]=01/02/03/ff [12..20]=f64 [20..24]=f32
    // 打印 (eid, tag9..11, idx, f32 尾字段) 时序——检验 f32 是否俯仰
    let mut n = 0;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() != 25 { continue; }
            let eid = u32le(&p[0..4]);
            let tag = format!("{:02x}{:02x}", p[9], p[10]);
            let idx = p[11];
            let f64v = u64::from_le_bytes(p[12..20].try_into().unwrap()) as f64;
            let f32v = f32le(&p[20..24]);
            println!("t={:8.3} eid=..{:02x} tag={} idx={:2x} f64={:12.2} f32={:9.3}",
                pkt.clock_secs, eid & 0xff, tag, idx, f64v, f32v);
            n += 1;
            if n > 45 { break; }
        }
    }
}
