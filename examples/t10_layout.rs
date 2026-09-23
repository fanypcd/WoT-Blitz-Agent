//! type=10 布局检验：打印行驶中车辆的单包全 49 字节，对照 ang(24..36) 与 ang(36..48)。
//! 用法：cargo run --release --example t10_layout -- <path.wotbreplay>
use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut last: HashMap<u32, (f32, [f32; 3])> = HashMap::new();
    let mut printed = 0;
    for pkt in &data.packets {
        if pkt.clock_secs < 30.0 { continue; }
        let p: Vec<u8> = pkt.raw_payload.clone();
        if p.len() != 49 { continue; }
        let eid = u32le(&p[0..4]);
        if eid == 0 { continue; }
        let pos = [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])];
        if let Some((t0, pos0)) = last.get(&eid) {
            let dt = pkt.clock_secs - t0;
            let spd = ((pos[0] - pos0[0]).powi(2) + (pos[2] - pos0[2]).powi(2)).sqrt() / dt;
            if spd > 2.0 && spd < 20.0 && dt > 0.0 {
                println!("eid=0x{:08x} t={:.2} spd={:.1}m/s", eid, pkt.clock_secs, spd);
                println!("  f24=[{:.4},{:.4},{:.4}]  f36=[{:.4},{:.4},{:.4}]",
                    f32le(&p[24..28]), f32le(&p[28..32]), f32le(&p[32..36]),
                    f32le(&p[36..40]), f32le(&p[40..44]), f32le(&p[44..48]));
                println!("  raw[24..48] {} [48]={:02x}",
                    p[24..48].iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" "), p[48]);
                printed += 1;
                if printed >= 4 { break; }
            }
        }
        last.insert(eid, (pkt.clock_secs, pos));
    }
}
