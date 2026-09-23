use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut pose: HashMap<u32, Vec<(f32, f32, f32)>> = HashMap::new(); // t, pitch, roll
    let mut fires: Vec<(f32, f32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let _ = u32le;
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    pose.entry(u32le(&p[0..4])).or_default().push((
                        pkt.clock_secs, f32le(&p[40..44]), f32le(&p[44..48])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 49 {
                    let mid = u32le(&p[4..8]);
                    let alen = u32le(&p[8..12]) as usize;
                    if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                        let a = &p[12..12 + alen];
                        let sid = u32le(&a[4..8]);
                        if seen.insert(sid) {
                            let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                            let n = (v[0]*v[0]+v[1]*v[1]+v[2]*v[2]).sqrt();
                            fires.push((pkt.clock_secs, (v[1]/n).asin()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // 作者车 0x100c7d6c 的弹速真值(前面已算): (t, we)

    let seq = &pose[&0x100c7d6c];
    println!("作者车(6c) 开火: 弹速世界仰角 vs 同刻 type10 pitch");
    for (ft, we) in fires.iter() {
        if let Some((pt, pitch, roll)) = seq.iter().rev().find(|(pt, _, _)| *pt <= *ft + 0.15) {
            println!("t={:8.3} 弹速世界={:+7.2}°  type10 pitch={:+7.2}°  roll={:+7.2}°  Δ={:+5.2}°",
                ft, we*57.2958, pitch*57.2958, roll*57.2958, (we-pitch)*57.2958);
        }
    }
    let _ = fires;
}
