fn main() {
    let f = std::fs::File::open("C:/Users/Administrator/AppData/Local/wotblitz/DAVAProject/replays/20260923_1153__Anonyme_T110_579731637977435380.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 48 {
                let eid = u32le(&p[0..4]);
                let pos = [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])];
                if pos[0] == 0.0 && pos[1] == 0.0 && pos[2] == 0.0 {
                    println!("全零位姿实体: 0x{:08x}", eid);
                }
            }
        }
    }
}
