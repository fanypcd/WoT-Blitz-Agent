//! type=5 实体创建初始属性 blob 转储：protobuf 启发式扫描（找 VehicleDetails/optDevicePreset 载体）。
//! 用法：cargo run --release --example t5_dump -- <replay> [eid...]
use std::collections::HashMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let f = std::fs::File::open(&args[1]).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let want: Option<std::collections::HashSet<u32>> = if args.len() > 2 {
        Some(args[2..].iter().map(|x| x.parse().unwrap()).collect())
    } else { None };

    // 实体昵称（type=5 payload[57..]）
    let mut nick: HashMap<u32, String> = HashMap::new();

    let mut count = 0usize;
    for pkt in &data.packets {
        if !matches!(pkt.payload, wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. }) {
            let t = match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
                _ => 999,
            };
            if t != 5 { continue; }
            let p = &pkt.raw_payload;
            if p.len() < 60 { continue; }
            let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            // 昵称抓取
            if p.len() > 57 {
                let l = p[57] as usize;
                if (3..=30).contains(&l) && 58 + l <= p.len() {
                    if let Ok(s) = std::str::from_utf8(&p[58..58 + l]) {
                        nick.insert(eid, s.to_string());
                    }
                }
            }
            if let Some(w) = &want {
                if !w.contains(&eid) { continue; }
            }
            count += 1;
            println!("== type=5 eid={} len={} clock={:.2}", eid, p.len(), pkt.clock_secs);
            if want.as_ref().map(|w| w.len() == 2).unwrap_or(false) {
                let hh: String = p.iter().map(|b| format!("{:02x}", b)).collect();
                println!("FULLHEX {}", hh);
                continue;
            }
            // 启发式：找重复 varint 串（设备 id 列表）与 protobuf 长度前缀模式
            // 打印 blob 的 protobuf 字段概览（从 payload[58+l] 之后的属性区）：
            let start = if p.len() > 57 { 58 + p[57] as usize } else { 58 };
            let blob = &p[start..];
            println!("   attr blob: {} bytes, head: {}", blob.len(), hex_head(&blob[..blob.len().min(64)]));
            // 常见模式：field 头 (tag<<3|wiretype)，找 wiretype=2(LEN) 且长度合理的嵌套
            let mut q = 0usize;
            let mut fields = Vec::new();
            while q < blob.len() && fields.len() < 40 {
                let tag = blob[q];
                let (field, wt) = (tag >> 3, tag & 7);
                match wt {
                    0 => { // varint
                        let mut v = 0u64; let mut s = 0; let mut qq = q + 1;
                        while qq < blob.len() { let b = blob[qq]; v |= ((b & 0x7f) as u64) << s; s += 7; qq += 1; if b & 0x80 == 0 { break; } }
                        fields.push(format!("f{} varint={}", field, v));
                        q = qq;
                    }
                    2 => { // LEN
                        let mut ln = 0usize; let mut s = 0; let mut qq = q + 1;
                        while qq < blob.len() { let b = blob[qq]; ln |= ((b & 0x7f) as usize) << s; s += 7; qq += 1; if b & 0x80 == 0 { break; } }
                        if qq + ln <= blob.len() {
                            let slice = &blob[qq..qq + ln];
                            let printable = slice.iter().all(|c| (0x20..0x7f).contains(c));
                            let vs = slice.iter().all(|c| *c < 0x40);
                            fields.push(format!("f{} len={} {}", field, ln, if printable {
                                format!("str='{}'", String::from_utf8_lossy(slice))
                            } else if vs && ln < 16 {
                                format!("varints={:?}", slice.to_vec())
                            } else {
                                format!("bytes[{}]", hex_head(slice))
                            }));
                            q = qq + ln;
                        } else { break; }
                    }
                    5 => { if q + 5 <= blob.len() { fields.push(format!("f{} fixed32={}", field, u32::from_le_bytes([blob[q+1], blob[q+2], blob[q+3], blob[q+4]]))); } q += 5; }
                    1 => { if q + 9 <= blob.len() { fields.push(format!("f{} fixed64", field)); } q += 9; }
                    _ => break,
                }
            }
            for x in fields { println!("   {}", x); }
        }
    }
    if count == 0 { println!("(无匹配 type=5)"); }
    for (e, s) in &nick { println!("nick {}={}", e, s); }
}

fn hex_head(s: &[u8]) -> String {
    s.iter().take(24).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ")
}
