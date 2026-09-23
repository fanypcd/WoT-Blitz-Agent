//! 方法流全量扫描：按 (mid) 统计频次/载荷长度/样本十六进制，标记已知集合之外的未解析方法。
//! 用法：cargo run --release --example method_sweep -- <path.wotbreplay>
use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: method_sweep <file>");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // 已识别 mid（逆向文档 §4.2/§4.3/§三）
    let known: &[u32] = &[0x00, 0x01, 0x07, 0x08, 0x0d, 0x14, 0x17, 0x1b, 0x23, 0x24, 0x30];

    let mut mids: HashMap<u32, (usize, HashMap<usize, usize>, Vec<(f32, u32, Vec<u8>)>)> = HashMap::new();
    for pkt in &data.packets {
        if !matches!(&pkt.payload, wotbreplay_parser::models::data::payload::Payload::EntityMethod(_)) { continue; }
        let t = pkt.clock_secs;
        let p = &pkt.raw_payload[..];
        if p.len() < 12 { continue; }
        let eid = u32le(&p[0..4]);
        let mid = u32le(&p[4..8]);
        let alen = u32le(&p[8..12]) as usize;
        if 12 + alen > p.len() { continue; }
        let e = mids.entry(mid).or_default();
        e.0 += 1;
        *e.1.entry(alen).or_insert(0) += 1;
        if e.2.len() < 3 { e.2.push((t, eid, p[12..12 + alen].to_vec())); }
    }

    let mut ids: Vec<u32> = mids.keys().copied().collect();
    ids.sort();
    println!("mid  频次  载荷长度直方图  [已知/未知] 样本");
    for mid in ids {
        let (n, lens, samples) = &mids[&mid];
        let mut lh: Vec<String> = lens.iter().map(|(l, c)| format!("{}×{}", l, c)).collect();
        lh.sort();
        let tag = if known.contains(&mid) { "已知" } else { "**未知**" };
        println!("0x{:02x}  n={}  {}  {}", mid, n, lh.join(" "), tag);
        for (t, eid, a) in samples.iter().take(2) {
            let hex: Vec<String> = a.iter().take(24).map(|b| format!("{:02x}", b)).collect();
            println!("      t={:.2} eid={:08x} args={}", t, eid, hex.join(""));
        }
    }
}
