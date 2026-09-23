//! 1436: prop9 是否 = 炮管俯仰流（含固定时钟偏移）？
//! 对每个候选偏移 Δ ∈ [-3, +3]s，计算 Σ|prop9(t_fire−Δ) − 弹速仰角|，找最小值。
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("C:/Users/Administrator/AppData/Local/wotblitz/DAVAProject/replays/20260906_1436__Anonyme_T110_1155501458890492367.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut p9: Vec<(f32, f32)> = Vec::new();
    let mut fires: Vec<(f32, f32)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 16 && u32le(&p[4..8]) == 9 {
                    let alen = u32le(&p[8..12]) as usize;
                    if alen == 4 && 12 + 4 <= p.len() {
                        p9.push((pkt.clock_secs, f32le(&p[12..16])));
                    }
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
    println!("prop9 {} 包, 开火 {} 发", p9.len(), fires.len());
    for delta in [-3.0, -2.0, -1.0, -0.5, -0.2, -0.1, 0.0, 0.1, 0.2, 0.5, 1.0, 2.0, 3.0] {
        let mut err = 0.0; let mut n = 0;
        for (ft, te) in &fires {
            let target = te + delta;   // 在 prop9 时序里找 t = fire+Δ 处的值
            if let Some((_, v)) = p9.iter().min_by_key(|(t, _)| ((t - target).abs() * 1000.0) as u32) {
                err += (v - te).abs(); n += 1;
            }
        }
        println!("Δ={:+.1}s  平均|prop9−弹速仰角| = {:.2}°", delta, err / n as f32 * 57.29578);
    }
    let _ = p9.len();
}
