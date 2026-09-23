use std::collections::BTreeMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut hdr: BTreeMap<(u8, u8, u8, u8), usize> = BTreeMap::new();
    let mut vals: BTreeMap<u8, Vec<f32>> = BTreeMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if !(p.len() == 24 || p.len() == 25) { continue; }
            let method = u32le(&p[5..9]);
            if method != 0x10 { continue; }
            let a = &p[9..];
            if a.len() < 16 { continue; }
            let k = (a[0], a[1], a[2], a[3]);
            *hdr.entry(k).or_insert(0) += 1;
            let v = f32le(&a[12..16]);
            vals.entry(a[3]).or_default().push(v);
        }
    }
    println!("头部 4B 分布:");
    for (k, c) in hdr {
        println!("  {:02x} {:02x} {:02x} {:02x}  x{}", k.0, k.1, k.2, k.3, c);
    }
    println!("\n按第 4 字节分组的 f32 值域:");
    for (b, vs) in vals {
        let mn = vs.iter().fold(f32::MAX, |a, b| a.min(*b));
        let mx = vs.iter().fold(f32::MIN, |a, b| a.max(*b));
        println!("  {:02x}: n={} min={:.3} max={:.3}", b, vs.len(), mn, mx);
    }
}
