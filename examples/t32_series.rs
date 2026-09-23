fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    // idx 1/2/3 三路分开打印（前 25B 包）
    let mut series: Vec<(f32, u32, u8, f32)> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() != 25 { continue; }
            let eid = u32le(&p[0..4]);
            let idx = p[12];
            if idx == 0xff || idx == 0x45 { continue; }
            let val = f32le(&p[21..25]);
            series.push((pkt.clock_secs, eid, idx, val));
        }
    }
    series.sort_by_key(|(t, _, i, _)| ((*t * 100.0) as i64, *i));
    // 打印 175~185s 窗口（作者 shot#7/9 之间）
    let mut last_t = 0.0;
    for (t, eid, idx, v) in &series {
        if *t < 175.5 || *t > 186.0 { continue; }
        if (*t * 10.0) as i64 != (last_t * 10.0) as i64 { println!(""); }
        print!("t={:.1} e{}x{}={:.1}  ", t, (eid & 0xff) as u8, idx, v);
        last_t = *t;
    }
    println!();
    // 值域统计
    let mut all: Vec<f32> = series.iter().map(|(_,_,_,v)| *v).collect();
    all.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("值域: min={:.1} p5={:.1} med={:.1} p95={:.1} max={:.1} (n={})",
        all[0], all[all.len()/20], all[all.len()/2], all[all.len()*19/20], all[all.len()-1], all.len());
}
