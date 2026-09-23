
use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    let mut eids: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            let kind = format!("len{}", p.len());
            *hist.entry(kind).or_insert(0) += 1;
            let eid = if p.len() >= 4 { u32le(&p[0..4]) } else { 0 };
            eids.entry(format!("len{}", p.len())).or_default().push(eid);
        }
    }
    for (k, n) in hist {
        let es = eids.get(&k).unwrap();
        let uniq: std::collections::BTreeSet<u32> = es.iter().copied().collect();
        let show: Vec<String> = uniq.iter().take(6).map(|e| format!("0x{:08x}", e)).collect();
        println!("{} n={} uniq_eids={} [{}]", k, n, uniq.len(), show.join(" "));
    }
}
