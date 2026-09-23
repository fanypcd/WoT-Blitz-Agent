fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let mut hist: std::collections::BTreeMap<u32, (usize, std::collections::BTreeMap<usize, usize>)> = std::collections::BTreeMap::new();
    for pkt in &data.packets {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8u32,
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0u32,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        let e = hist.entry(t).or_insert((0, std::collections::BTreeMap::new()));
        e.0 += 1;
        *e.1.entry(pkt.raw_payload.len()).or_insert(0) += 1;
    }
    for (t, (n, lens)) in &hist {
        let lh: Vec<String> = lens.iter().map(|(l, c)| format!("{}x{}", l, c)).collect();
        println!("type={:<3} n={:<6} lens: {}", t, n, lh.join(","));
    }
}
