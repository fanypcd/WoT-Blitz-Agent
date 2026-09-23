//! type=7 属性流探针：per-propID 载荷长度直方图 + 样本十六进制转储。
//! 目的：验证 prop2（炮塔朝向 u16）是否为 set_gunAnglesPacked（2×u16 打包 = 偏航+俯仰），
//! 以及各 propID 与 Vehicle 属性名（hitMarks/armorsStates/publicInfo...）的量级对应。
//! 用法：cargo run --example prop_probe -- <path.wotbreplay> [propID]
//! 可选第二个参数只看某个 propID。

use std::collections::BTreeMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: prop_probe <file> [propID] | ts <file> <eid-hex> <propID>");
    if std::env::args().nth(1).as_deref() == Some("ts") {
        let f = std::fs::File::open(std::env::args().nth(2).unwrap()).unwrap();
        let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
        let data = replay.read_data().unwrap();
        let eid = u32::from_str_radix(std::env::args().nth(3).unwrap().trim_start_matches("0x"), 16).unwrap();
        let want: u32 = std::env::args().nth(4).unwrap().parse().unwrap();
        let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        for pkt in &data.packets {
            if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[0..4]) == eid && u32le(&p[4..8]) == want {
                    let alen = u32le(&p[8..12]) as usize;
                    println!("{:.3} {}", pkt.clock_secs,
                        p[12..12 + alen].iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" "));
                }
            }
            if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            }
        }
        return;
    }
    let only_prop: Option<u32> = std::env::args().nth(2).and_then(|s| s.parse().ok());
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // propID → (count, len→count, 样本 hex, 涉及实体集合)
    let mut props: BTreeMap<u32, (usize, BTreeMap<usize, usize>, Vec<String>, std::collections::BTreeSet<u32>)> = BTreeMap::new();
    let mut t28: Vec<String> = Vec::new();
    for pkt in &data.packets {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            _ => continue,
        };
        let p = &pkt.raw_payload[..];
        if t == 7 && p.len() >= 14 {
            let eid = u32le(&p[0..4]);
            let prop = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if 12 + alen > p.len() { continue; }
            if let Some(want) = only_prop { if prop != want { continue; } }
            let e = props.entry(prop).or_default();
            e.0 += 1;
            *e.1.entry(alen).or_insert(0) += 1;
            if e.2.len() < 4 { e.2.push(hex(p[12..12 + alen.min(12 + alen)].to_vec())); }
            e.3.insert(eid);
        }
        if t == 28 && only_prop.is_none() && t28.len() < 8 {
            t28.push(format!("len={} {}", p.len(), hex(p[..p.len().min(28)].to_vec())));
        }
    }

    println!("=== type=7 property stream（{} 个 propID）===", props.len());
    for (prop, (cnt, lens, samples, eids)) in &props {
        let lens_str: Vec<String> = lens.iter().map(|(l, c)| format!("{}B×{}", l, c)).collect();
        println!("prop {:>2}: n={:<6} len[{}] eids={}", prop, cnt, lens_str.join(","), eids.len());
        for s in samples.iter().take(2) {
            println!("        {}", s);
        }
    }
    if !t28.is_empty() {
        println!("=== type=28 样本（{}）===", t28.len());
        for s in &t28 { println!("  {}", s); }
    }
}

fn hex(b: Vec<u8>) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" ")
}
