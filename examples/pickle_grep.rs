//! method 0x31（PlayerSynchronizedOptions）zlib pickle 提取 + 关键字扫描。
use std::io::Read;

fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    if from >= h.len() || n.len() > h.len() { return None; }
    for i in from..=h.len() - n.len() {
        if &h[i..i + n.len()] == n { return Some(i); }
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let f = std::fs::File::open(&args[1]).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    for pkt in &data.packets {
        if !matches!(pkt.payload, wotbreplay_parser::models::data::payload::Payload::EntityMethod(_)) { continue; }
        let p = &pkt.raw_payload;
        if p.len() < 12 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x31 { continue; }
        let al = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if al < 100 { continue; }
        let a = &p[12..12 + al];
        let mut solved = false;
        'outer: for off in 0..a.len().min(16) {
            for zlib in [true, false] {
                let mut out = Vec::new();
                let r = if zlib {
                    let mut d = flate2::read::ZlibDecoder::new(&a[off..]);
                    d.read_to_end(&mut out)
                } else {
                    let mut d = flate2::read::DeflateDecoder::new(&a[off..]);
                    d.read_to_end(&mut out)
                };
                if let Ok(n) = r {
                    if n > 200 {
                        println!("method 0x31: off={} zlib={} inflated {} bytes", off, zlib, n);
                        std::fs::write("tmp_wi_js/pickle_031.bin", &out).unwrap();
                        for kw in ["calib", "enhance", "optDevice", "equipment", "device", "shell", "shoot", "gun"] {
                            let mut hits = Vec::new();
                            let mut i = 0;
                            while let Some(pos) = find(&out, kw.as_bytes(), i) { hits.push(pos); i = pos + 1; }
                            println!("  '{}': {} hits {:?}", kw, hits.len(), &hits[..hits.len().min(8)]);
                        }
                        solved = true;
                        break 'outer;
                    }
                }
            }
        }
        if !solved { println!("method 0x31 args={}: 未找到可解压流", al); }
        break;
    }
}
