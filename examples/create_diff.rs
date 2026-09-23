//! 同实体两个 create 包字节差分：变化字节 = 该实体在两次进世界之间变化过的属性
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut cre: Vec<(f32, Vec<u8>)> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 4 && u32le(&p[0..4]) == 0x100c7d6c {
                cre.push((pkt.clock_secs, p.to_vec()));
            }
        }
    }
    if cre.len() < 2 { println!("create 包不足 2 个: {}", cre.len()); return; }
    let (t1, a) = &cre[0];
    let (t2, b) = &cre[cre.len() - 1];
    println!("create#1 t={:.3} len={} / create#2 t={:.3} len={}", t1, a.len(), t2, b.len());
    let n = a.len().min(b.len());
    let mut i = 0;
    while i < n {
        if a[i] != b[i] {
            let start = i;
            while i < n && a[i] != b[i] { i += 1; }
            let ha: Vec<String> = a[start..i].iter().map(|x| format!("{:02x}", x)).collect();
            let hb: Vec<String> = b[start..i].iter().map(|x| format!("{:02x}", x)).collect();
            // u16 双读
            let u16a: Vec<u16> = a[start..i].chunks(2).filter(|c| c.len()==2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            println!("off [{:3}..{:3}) A[{}] B[{}] u16A={:?}", start, i, ha.join(" "), hb.join(" "), u16a);
        } else { i += 1; }
    }
}
