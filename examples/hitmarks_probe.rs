//! hitMarks 属性定位探针：扫描 type=7 属性流，按 (eid, propId) 统计，
//! 找出与"被击次数"相关的属性（HitMarkPlacementInfo protobuf 载体）。
//! 用法：cargo run --release --example hitmarks_probe -- <path.wotbreplay>
use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: hitmarks_probe <file>");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // method8 命中事件：victim → [(t, cmpIdx, hash6)]
    let mut hits_by_victim: HashMap<u32, Vec<(f32, u8, [u8; 6])>> = HashMap::new();
    // type=7 属性：eid → propId → [(t, payload)]
    let mut props: HashMap<u32, HashMap<u32, Vec<(f32, Vec<u8>)>>> = HashMap::new();
    let mut has_t10: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for pkt in &data.packets {
        let t = pkt.clock_secs;
        let p = &pkt.raw_payload[..];
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                if p.len() >= 4 { has_t10.insert(u32le(&p[0..4])); }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                if p.len() >= 16 {
                    let mid = u32le(&p[4..8]);
                    let alen = u32le(&p[8..12]) as usize;
                    if 12 + alen <= p.len() {
                        let a = &p[12..12 + alen];
                        if mid == 0x08 && alen >= 17 && a[8] == 1 {
                            hits_by_victim.entry(u32le(&a[4..8])).or_default()
                                .push((t, a[10], [a[11], a[12], a[13], a[14], a[15], a[16]]));
                        }
                    }
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                // 布局（prop2 已验证）：[eid u32][propId u32][len u32][payload]
                if p.len() >= 12 {
                    let eid = u32le(&p[0..4]);
                    let prop = u32le(&p[4..8]);
                    let l = u32le(&p[8..12]) as usize;
                    if 12 + l <= p.len() {
                        props.entry(eid).or_default().entry(prop).or_default()
                            .push((t, p[12..12 + l].to_vec()));
                    }
                }
            }
            _ => {}
        }
    }

    // 汇总：只看有 type10 流的实体（车辆）
    let mut eids: Vec<u32> = props.keys().copied().collect();
    eids.sort();
    for eid in eids {
        if !has_t10.contains(&eid) { continue; }
        let plist = &props[&eid];
        let hits = hits_by_victim.get(&eid).map(Vec::len).unwrap_or(0);
        let mut row = format!("eid={:08x} hits={} props:", eid, hits);
        let mut ids: Vec<u32> = plist.keys().copied().collect();
        ids.sort();
        for pid in &ids {
            let v = &plist[pid];
            row.push_str(&format!(" p{}×{}", pid, v.len()));
        }
        println!("{}", row);
        // 频次与被击数接近的 prop：dump 样本
        for pid in &ids {
            let v = &plist[pid];
            if hits > 0 && v.len() >= hits && v.len() <= hits * 3 + 2 && v[0].1.len() >= 8 {
                println!("   候选 p{} (len={}):", pid, v[0].1.len());
                for (t, pay) in v.iter().take(4) {
                    let hex: Vec<String> = pay.iter().take(32).map(|b| format!("{:02x}", b)).collect();
                    println!("     t={:.3} len={} {}", t, pay.len(), hex.join(""));
                }
            }
        }
    }
    // 各 propId 的 payload 长度直方图（全实体聚合）
    println!("\n--- propId → 长度直方图（全实体）---");
    let mut agg: HashMap<u32, HashMap<usize, usize>> = HashMap::new();
    for plist in props.values() {
        for (pid, v) in plist {
            for (_, pay) in v {
                *agg.entry(*pid).or_default().entry(pay.len()).or_insert(0) += 1;
            }
        }
    }
    let mut ids: Vec<u32> = agg.keys().copied().collect();
    ids.sort();
    for pid in ids {
        let mut l: Vec<(usize, usize)> = agg[&pid].iter().map(|(k, v)| (*k, *v)).collect();
        l.sort();
        let total: usize = l.iter().map(|x| x.1).sum();
        let brief: Vec<String> = l.iter().rev().take(5).map(|(len, n)| format!("len{}×{}", len, n)).collect();
        println!("  p{}: n={} {}", pid, total, brief.join(" "));
    }
}
