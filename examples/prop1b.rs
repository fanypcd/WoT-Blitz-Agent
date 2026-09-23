
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // propID → (count, 值直方图, 实体, 时段)
    let mut m: BTreeMap<u32, (usize, BTreeMap<u8, usize>, std::collections::BTreeSet<u32>, f32, f32)> = BTreeMap::new();
    let mut first = 0;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() != 13 { continue; }
            let eid = u32le(&p[0..4]);
            let prop = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if alen != 1 || 12 + 1 > p.len() { continue; }
            let v = p[12];
            let e = m.entry(prop).or_insert((0, BTreeMap::new(), std::collections::BTreeSet::new(), pkt.clock_secs, pkt.clock_secs));
            e.0 += 1;
            *e.1.entry(v).or_insert(0) += 1;
            e.2.insert(eid);
            if pkt.clock_secs < e.3 { e.3 = pkt.clock_secs; }
            if pkt.clock_secs > e.4 { e.4 = pkt.clock_secs; }
            if first < 3 {
                println!("样本 t={:.3} eid=0x{:08x} prop={} val={:02x}", pkt.clock_secs, eid, prop, v);
                first += 1;
            }
        }
    }
    println!();
    for (prop, (n, vals, eids, t0, t1)) in &m {
        let vh: Vec<String> = vals.iter().map(|(v, c)| format!("{:02x}x{}", v, c)).collect();
        let es: Vec<String> = eids.iter().map(|e| format!("{:08x}", e)).collect();
        println!("prop {} n={} t={:.1}~{:.1} eids=[{}]", prop, n, t0, t1, es.join(","));
        println!("   vals: {}", vh.join(" "));
    }
}
