//! type=8 各 method 的参数长度直方图 + 实体覆盖 + 频次 —— 定位 set_gunAnglesPacked
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut hist: BTreeMap<u32, (usize, BTreeMap<usize, usize>, f32, f32, usize)> = BTreeMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            let e = hist.entry(mid).or_insert((0, BTreeMap::new(), pkt.clock_secs, pkt.clock_secs, 0));
            e.0 += 1;
            *e.1.entry(alen).or_insert(0) += 1;
            if pkt.clock_secs < e.2 { e.2 = pkt.clock_secs; }
            if pkt.clock_secs > e.3 { e.3 = pkt.clock_secs; }
            e.4 += 1;
        }
    }
    println!("method  count  t_range          alen_hist          eids");
    for (m, (c, lens, t0, t1, eids)) in &hist {
        let lh: Vec<String> = lens.iter().map(|(l, n)| format!("{}x{}", l, n)).collect();
        println!("0x{:02x}   {:5}  {:7.2}~{:7.2}  {}  eids={}", m, c, t0, t1, lh.join(","), eids);
    }
}
