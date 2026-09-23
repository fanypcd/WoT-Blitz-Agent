
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // type=7 1B 属性包按 (eid, propID) 分组时序
    let mut p1b: BTreeMap<(u32, u32), Vec<(f32, u8)>> = BTreeMap::new();
    // type=7 prop3/10/11 4B/2B 值
    let mut p23: BTreeMap<(u32, u32), Vec<(f32, Vec<u8>)>> = BTreeMap::new();
    let mut small: Vec<(f32, u32, Vec<u8>)> = Vec::new();
    let mut m13: Vec<(f32, u32, Vec<u8>)> = Vec::new();
    let mut m18: Vec<(f32, u32, Vec<u8>)> = Vec::new();
    let mut m2c: Vec<(f32, u32, Vec<u8>)> = Vec::new();
    let mut m31: usize = 0;
    let mut t10_state: BTreeMap<u32, BTreeMap<u8, usize>> = BTreeMap::new();
    for pkt in &data.packets {
        let t = pkt.clock_secs;
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 13 {
                    let eid = u32le(&p[0..4]);
                    let prop = u32le(&p[4..8]);
                    let alen = u32le(&p[8..12]) as usize;
                    if 12 + alen > p.len() { continue; }
                    let val = &p[12..12+alen];
                    match alen {
                        1 => { p1b.entry((eid, prop)).or_default().push((t, val[0])); }
                        _ => { p23.entry((eid, prop)).or_default().push((t, val.to_vec())); }
                    }
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 49 {
                    let eid = u32le(&p[0..4]);
                    *t10_state.entry(eid).or_default().entry(p[48]).or_insert(0) += 1;
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => {
                let pt = *packet_type;
                if matches!(pt, 4 | 26 | 29 | 36 | 38) {
                    small.push((t, pt, pkt.raw_payload.clone()));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() < 16 { continue; }
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if 12 + alen > p.len() { continue; }
                let eid = u32le(&p[0..4]);
                let args = p[12..12+alen].to_vec();
                match mid {
                    0x13 => m13.push((t, eid, args)),
                    0x18 => m18.push((t, eid, args)),
                    0x2c => m2c.push((t, eid, args)),
                    0x31 => m31 += 1,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    println!("===== type=7 1B 属性包（按 eid+prop）=====");
    for ((eid, prop), v) in &p1b {
        let vals: Vec<String> = v.iter().map(|(t, x)| format!("{:.1}:{:02x}", t, x)).take(6).collect();
        println!("eid=0x{:08x} prop={} n={} 样本[{}]", eid, prop, v.len(), vals.join(" "));
    }
    println!("\n===== type=7 prop3（HP 族?）及 10/11 =====");
    for ((eid, prop), v) in &p23 {
        if *prop != 3 && *prop != 10 && *prop != 11 { continue; }
        let vals: Vec<String> = v.iter().map(|(t, x)| format!("{:.1}:{}", t,
            x.iter().map(|b| format!("{:02x}", b)).collect::<String>())).take(8).collect();
        println!("eid=0x{:08x} prop={} n={} [{}]", eid, prop, v.len(), vals.join(" | "));
    }
    println!("\n===== type=10 [48] 状态字节分布 =====");
    for (eid, m) in &t10_state {
        let s: Vec<String> = m.iter().map(|(b, c)| format!("{:02x}x{}", b, c)).collect();
        println!("eid=0x{:08x} {}", eid, s.join(" "));
    }
    println!("\n===== 小包型 =====");
    for (t, pt, v) in small.iter().take(20) {
        let hexv: Vec<String> = v.iter().map(|b| format!("{:02x}", b)).collect();
        println!("t={:8.3} type={} [{}]", t, pt, hexv.join(" "));
    }
    println!("\n===== method 0x13 ({}包) =====", m13.len());
    for (t, e, v) in m13.iter().take(6) {
        let hexv: Vec<String> = v.iter().map(|b| format!("{:02x}", b)).collect();
        println!("t={:8.3} eid=0x{:08x} alen={} [{}]", t, e, v.len(), hexv.join(" "));
    }
    println!("\n===== method 0x18 ({}包) =====", m18.len());
    for (t, e, v) in m18.iter().take(6) {
        let hexv: Vec<String> = v.iter().map(|b| format!("{:02x}", b)).collect();
        println!("t={:8.3} eid=0x{:08x} alen={} [{}]", t, e, v.len(), hexv.join(" "));
    }
    println!("\n===== method 0x2c ({}包) =====", m2c.len());
    for (t, e, v) in m2c.iter().take(3) {
        let hexv: Vec<String> = v.iter().map(|b| format!("{:02x}", b)).collect();
        println!("t={:8.3} eid=0x{:08x} alen={} [{}]", t, e, v.len(), hexv.join(" "));
    }
    println!("method 0x31 出现 {} 次（6030B 大包）", m31);
}
