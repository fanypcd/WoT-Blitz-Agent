//! type=5 实体创建包完整转储：定位初始属性块中的 prop2 初值与相邻字段（俯仰候选）
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            let eid = if p.len() >= 4 { u32le(&p[0..4]) } else { 0 };
            if eid != 0x100c7d6c && eid != 0x100c7d6d { continue; }
            println!("=== eid=0x{:08x} len={} t={:.3}", eid, p.len(), pkt.clock_secs);
            for (i, w) in p.chunks(4).enumerate() {
                let hexs: Vec<String> = w.iter().map(|b| format!("{:02x}", b)).collect();
                let f = if w.len() == 4 { format!("{:>12.4}", f32le(w)) } else { hexs.iter().map(|x| format!("{:>12}", x)).collect::<Vec<_>>().join(" ") };
                println!("  [{:02}] {:<11} {}", i*4, hexs.join(" "), f);
            }
        }
    }
}
