//! 弹着点遗漏扫描 v2（不预设坐标系）：
//! 以 method8 命中通知为时间锚，窗口内全部包逐字节偏移解 f32 三元组，同时用两个参照判定——
//!   A. 绝对坐标候选：三元组 ≈ 受击者世界位置（±8m）
//!   B. 车体系相对候选：三元组 ≈ method20 终点 − 受击者位置（aim_point 同源，±1.2m）
//!      且分量都在车体包络内（|v| ≤ 8m）——弹着点若是坦克相对量（如 hash6 弹孔的实数版）
//!      会落在这里。按（载体, 偏移, 对齐）聚合跨命中命中数：真载体跨多发一致，噪声散射。
//! 阳性对照：A 类应看到 t8/m0x14 终点与 t10 位置；B 类为纯发现区。
//! 用法：cargo run --release --example impact_hunt -- <a.wotbreplay>
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).expect("usage");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8], o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);

    let pkgs: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        (t, pkt.clock_secs, &pkt.raw_payload[..])
    }).collect();

    // type10 最后已知位置
    let mut st10: HashMap<u32, Vec<(f32, [f32; 3])>> = HashMap::new();
    for (t, c, p) in &pkgs {
        if *t == 10 && p.len() >= 24 {
            st10.entry(u32le(&p[0..4])).or_default().push((
                *c, [f32le(p, 12), f32le(p, 16), f32le(p, 20)]));
        }
    }
    for v in st10.values_mut() { v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap()); }
    let pos_at = |eid: u32, t: f32| -> Option<[f32; 3]> {
        st10.get(&eid)?.iter().rev().find(|(c, _)| *c <= t + 0.05).map(|(_, p)| *p)
    };

    // method8 命中通知 + 窗口内最近 method20 终点（车体系参照）
    let mut hits: Vec<(f32, u32)> = Vec::new();
    let mut m20: Vec<(f32, [f32; 3])> = Vec::new();
    for (t, c, p) in &pkgs {
        if *t != 8 || p.len() < 16 { continue; }
        let m = u32le(&p[4..8]);
        let alen = u32le(&p[8..12]) as usize;
        if 12 + alen > p.len() { continue; }
        match m {
            0x08 if alen >= 10 && p[12 + 8] == 1 => hits.push((*c, u32le(&p[12 + 4..12 + 8]))),
            0x14 if alen >= 16 => m20.push((*c, [f32le(p, 16), f32le(p, 20), f32le(p, 24)])),
            _ => {}
        }
    }
    println!("method8 命中通知 {} 条，method20 终点 {} 条", hits.len(), m20.len());

    let carrier = |t: u32, p: &[u8]| -> String {
        if t == 8 { format!("t8/m0x{:02x}", u32le(&p[4..8])) }
        else if t == 7 { format!("t7/prop{}", u32le(&p[4..8])) }
        else { format!("t{}", t) }
    };
    // 距离
    let dist = |a: [f32; 3], b: [f32; 3]| ((a[0]-b[0]).powi(2) + (a[1]-b[1]).powi(2) + (a[2]-b[2]).powi(2)).sqrt();

    // 聚合：abs / rel 两类，key=(载体,偏移,类别)，值=(去重命中数, 最小距, 总距, 样例值)
    let mut agg: HashMap<(String, usize, u8), (usize, f32, f32, [f32; 3])> = HashMap::new();
    let mut n_win = 0usize;
    let mut n_rel_ref = 0usize;
    for (ht, victim) in &hits {
        let Some(tp) = pos_at(*victim, *ht) else { continue };
        // 车体系参照：窗口内最近 m20 终点 − 受击者位置
        let rel_ref = m20.iter()
            .filter(|(c, _)| (*c - ht).abs() <= 0.12)
            .min_by(|a, b| (a.0 - ht).abs().partial_cmp(&(b.0 - ht).abs()).unwrap())
            .map(|(_, e)| [e[0] - tp[0], e[1] - tp[1], e[2] - tp[2]]);
        if rel_ref.is_some() { n_rel_ref += 1; }
        n_win += 1;
        // 该窗口内已计过的 (载体,偏移) —— 同包进多窗去重
        let mut seen: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
        for (pi, (t, c, p)) in pkgs.iter().enumerate() {
            if (c - ht).abs() > 0.12 { continue; }
            let name = carrier(*t, p);
            for off in 0..p.len().saturating_sub(12) {
                let v = [f32le(p, off), f32le(p, off + 4), f32le(p, off + 8)];
                if !v.iter().all(|x| x.is_finite()) { continue; }
                // A：绝对坐标
                if v[0].abs() <= 1000.0 && v[2].abs() <= 1000.0 && (-10.0..80.0).contains(&v[1]) {
                    let d = dist(v, tp);
                    if d <= 8.0 {
                        let key = (name.clone(), off, 0u8);
                        if seen.insert((pi, off | 0x1_00_00)) {
                            let e = agg.entry(key).or_insert((0, d, 0.0, v));
                            e.0 += 1; e.1 = e.1.min(d); e.2 += d;
                        }
                        continue;
                    }
                }
                // B：车体系相对（分量车体包络内 + 匹配 rel_ref）
                if let Some(rr) = rel_ref {
                    if v.iter().all(|x| x.abs() <= 8.0 && x.abs() >= 0.3) && v[1].abs() >= 0.0 {
                        let d = dist(v, rr);
                        if d <= 1.2 {
                            let key = (name.clone(), off, 1u8);
                            if seen.insert((pi, off | 0x2_00_00)) {
                                let e = agg.entry(key).or_insert((0, d, 0.0, v));
                                e.0 += 1; e.1 = e.1.min(d); e.2 += d;
                            }
                        }
                    }
                }
            }
        }
    }
    println!("有效窗口 {}（其中带车体系参照 {}）\n", n_win, n_rel_ref);

    for (label, code) in [("A 绝对坐标候选（≈受击者位置）", 0u8), ("B 车体系相对候选（≈m20终点−受击者）", 1u8)] {
        println!("=== {} ===", label);
        let mut rows: Vec<_> = agg.iter()
            .filter(|((_, _, c), _)| *c == code)
            .map(|((n, o, _), v)| (n.clone(), *o, v.0, v.1, v.2, v.3))
            .collect();
        rows.sort_by(|a, b| b.2.cmp(&a.2).then(a.3.partial_cmp(&b.3).unwrap()));
        println!("{:<14} {:>6} {:>5} {:>8} {:>8} {:>22}", "载体", "偏移", "命中数", "最小距", "平均距", "样例值(x,y,z)");
        for (n, o, cnt, mind, sumd, v) in rows.iter().take(25) {
            println!("{:<14} {:>6} {:>5} {:>7.2}m {:>7.2}m ({:+5.2},{:+5.2},{:+5.2})", n, o, cnt, mind, sumd / *cnt as f32, v[0], v[1], v[2]);
        }
        println!();
    }
}
