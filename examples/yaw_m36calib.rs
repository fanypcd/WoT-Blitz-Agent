//! 炮塔偏航公式定标（回放内部真值）：method36(0x24) 瞄准快照 field1 = f64 炮塔相对偏航
//! （与 prop2 同域），作者开火时刻成对出现 → 对照 prop2 coarse 解码：
//! 带符号误差分布（N=1024, C=0）+ 刻度/偏移联合网格拟合。
//! 用法：cargo run --release --example yaw_m36calib -- <j39.wotbreplay>
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // method36 args = [len u8][protobuf]；field1 (tag 0x09, fixed64) = f64 炮塔相对偏航
    fn parse_m36(a: &[u8]) -> Option<f64> {
        let len = *a.first()? as usize;
        if a.len() < 1 + len { return None; }
        let mut p = &a[1..1 + len];
        let mut yaw = None;
        while !p.is_empty() {
            let tag = p[0];
            p = &p[1..];
            let mut read_varint = |p: &mut &[u8]| -> u64 {
                let mut val = 0u64;
                let mut shift = 0u32;
                while !p.is_empty() {
                    let b = p[0];
                    *p = &p[1..];
                    val |= ((b & 0x7f) as u64) << shift;
                    shift += 7;
                    if b & 0x80 == 0 { break; }
                }
                val
            };
            match tag & 7 {
                0 => { read_varint(&mut p); }
                1 => {
                    if p.len() >= 8 {
                        let mut b8 = [0u8; 8];
                        b8.copy_from_slice(&p[..8]);
                        if tag == 0x09 { yaw = Some(f64::from_le_bytes(b8)); }
                        p = &p[8..];
                    } else { return yaw; }
                }
                2 => {
                    let l = read_varint(&mut p) as usize;
                    if p.len() >= l { p = &p[l..]; } else { return yaw; }
                }
                5 => { if p.len() >= 4 { p = &p[4..]; } else { return yaw; } }
                _ => return yaw,
            }
        }
        yaw
    }

    // 作者 eid：type=5 昵称 Anonyme
    let mut author = 0u32;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 58 {
                let l = p[57] as usize;
                if 58 + l <= p.len() {
                    if let Ok(s) = std::str::from_utf8(&p[58..58 + l]) {
                        if s == "Anonyme" { author = u32le(&p[0..4]); }
                    }
                }
            }
        }
    }
    println!("作者 eid = 0x{:08x}", author);

    let mut p2: Vec<(f32, u16)> = Vec::new();
    let mut m36: Vec<(f32, f64)> = Vec::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[4..8]) == 2 && u32le(&p[8..12]) == 2 && u32le(&p[0..4]) == author {
                    p2.push((pkt.clock_secs, u16::from_le_bytes([p[12], p[13]])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 14 && u32le(&p[4..8]) == 0x24 {
                    let alen = u32le(&p[8..12]) as usize;
                    if alen >= 2 && 12 + alen <= p.len() {
                        if let Some(y) = parse_m36(&p[12..12 + alen]) {
                            m36.push((pkt.clock_secs, y));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    println!("作者 prop2: {} 包，method36 快照: {} 条", p2.len(), m36.len());

    let wrap = |a: f64| -> f64 {
        let mut x = a;
        while x > std::f64::consts::PI { x -= std::f64::consts::TAU; }
        while x < -std::f64::consts::PI { x += std::f64::consts::TAU; }
        x
    };
    let mut pairs: Vec<(f64, f64)> = Vec::new(); // (coarse, m36_yaw)
    for (t, y) in &m36 {
        if let Some((_, u)) = p2.iter()
            .filter(|(c, _)| (*c - t).abs() <= 0.2)
            .min_by_key(|(c, _)| (((*c - t).abs()) * 1000.0) as u32)
        {
            pairs.push(((u >> 6) as f64, *y));
        }
    }
    println!("配对: {}", pairs.len());
    if pairs.is_empty() { return; }
    let errs: Vec<f64> = pairs.iter()
        .map(|(c, y)| wrap(c / 1024.0 * std::f64::consts::TAU - std::f64::consts::PI - y))
        .collect();
    let mut asorted: Vec<f64> = errs.iter().map(|x| x.abs()).collect();
    asorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut ssorted = errs.clone();
    ssorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = ssorted[ssorted.len() / 2];
    println!("N=1024 C=0: 带符号中位 {:+.4}° |err|中位 {:.4}° 90%<{:.4}° max {:.4}°",
        med * 57.29578, asorted[asorted.len() / 2] * 57.29578,
        asorted[(asorted.len() as f32 * 0.9) as usize] * 57.29578, asorted.last().unwrap() * 57.29578);

    let mut best = (f64::MAX, 0i64, 0.0f64);
    for n in 1000..=1049i64 {
        let e: Vec<f64> = pairs.iter()
            .map(|(c, y)| wrap(c / n as f64 * std::f64::consts::TAU - std::f64::consts::PI - y))
            .collect();
        let sx: f64 = e.iter().map(|x| x.cos()).sum();
        let sy: f64 = e.iter().map(|x| x.sin()).sum();
        let c = sy.atan2(sx);
        let rss: f64 = e.iter().map(|x| wrap(x - c)).map(|x| x * x).sum();
        if rss < best.0 { best = (rss, n, c); }
    }
    let (_, n, c) = best;
    let mut es: Vec<f64> = pairs.iter()
        .map(|(cc, y)| wrap(cc / n as f64 * std::f64::consts::TAU - std::f64::consts::PI - y - c).abs())
        .collect();
    es.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("联合最优: N={} C={:+.4}° ({:+.2}级) |err|中位 {:.4}° 90%<{:.4}° max {:.4}°",
        n, c * 57.29578, c / (std::f64::consts::TAU / 1024.0),
        es[es.len() / 2] * 57.29578, es[(es.len() as f32 * 0.9) as usize] * 57.29578, es.last().unwrap() * 57.29578);

    for i in 0..pairs.len().min(12) {
        let (cc, y) = pairs[i];
        println!("  coarse={:>4} 解={:+8.3}° m36={:+8.3}° err={:+7.3}°", cc,
            cc / 1024.0 * 360.0 - 180.0, y * 57.29578, errs[i] * 57.29578);
    }
}
