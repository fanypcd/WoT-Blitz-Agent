
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    use std::io::Read;
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid != 0x31 || alen != 6030 { continue; }
            let a = &p[12..12 + alen];
            // 头: [00 00 00][ff][87 17 00 78 01 ...] — 找 0x78 0x01 zlib 头位置
            let zpos = a.windows(2).position(|w| w[0] == 0x78 && (w[1] & 0x20) == 0 && ((w[0] as u16) << 8 | w[1] as u16) % 31 == 0);
            println!("zlib 头位置: {:?}", zpos.map(|z| z));
            if let Some(z) = zpos {
                let mut d = flate2::read::ZlibDecoder::new(&a[z..]);
                let mut out = Vec::new();
                match d.read_to_end(&mut out) {
                    Ok(n) => {
                        println!("解压 {} → {} 字节", alen - z, n);
                        let ascii: String = out.iter()
                            .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                            .collect();
                        println!("ASCII 预览: {}", &ascii[..ascii.len().min(2000)]);
                    }
                    Err(e) => println!("解压失败: {}", e),
                }
            }
            break;
        }
    }
}
