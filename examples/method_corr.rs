//! 未知方法相关性分析：0x0c/0x11/0x04/0x06/0x26 与 method38（模块损伤）/method8（命中）对时。
use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: method_corr <file>");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    struct M38 { t: f32, victim: u32, flags: u32, comps: Vec<(u8, u8)> }
    let mut m38s: Vec<M38> = Vec::new();
    let mut unknown: Vec<(u32, f32, u32, Vec<u8>)> = Vec::new(); // mid, t, eid, args

    for pkt in &data.packets {
        if !matches!(&pkt.payload, wotbreplay_parser::models::data::payload::Payload::EntityMethod(_)) { continue; }
        let t = pkt.clock_secs;
        let p = &pkt.raw_payload[..];
        if p.len() < 12 { continue; }
        let eid = u32le(&p[0..4]);
        let mid = u32le(&p[4..8]);
        let alen = u32le(&p[8..12]) as usize;
        if 12 + alen > p.len() { continue; }
        let a = p[12..12 + alen].to_vec();
        match mid {
            0x26 => { // method38: [shooter][victim][n][comps...][mods]
                if alen >= 9 {
                    let n = a[8] as usize;
                    let mut comps = Vec::new();
                    for i in 0..n {
                        let off = 9 + i * 2;
                        if off + 1 < alen { comps.push((a[off], a[off + 1])); }
                    }
                    m38s.push(M38 { t, victim: u32le(&a[4..8]), flags: 0, comps });
                }
            }
            0x0c | 0x11 | 0x04 | 0x06 | 0x26_ | _ => {
                if matches!(mid, 0x0c | 0x11 | 0x04 | 0x06) {
                    unknown.push((mid, t, eid, a));
                }
            }
        }
    }

    // 对每个未知事件：找 ±0.25s 内的 method38（victim=eid 或包 envelope=eid）
    println!("mid | t | eid | args(hex) | 最近 method38 (Δt, victim, comps)");
    for (mid, t, eid, a) in &unknown {
        let hex: Vec<String> = a.iter().map(|b| format!("{:02x}", b)).collect();
        let nearest = m38s.iter()
            .filter(|m| (m.t - t).abs() <= 0.25)
            .min_by(|x, y| x.t.partial_cmp(&y.t).unwrap());
        let near = match nearest {
            Some(m) => {
                let comps: Vec<String> = m.comps.iter().map(|(c, st)| format!("{}:{}", c, st)).collect();
                format!("Δt={:+.3} victim={:08x} comps=[{}]", m.t - t, m.victim, comps.join(","))
            }
            None => "—".into(),
        };
        println!("0x{:02x} t={:.3} eid={:08x} {} | {}", mid, t, eid, hex.join(""), near);
    }
}
