//! method1 血量事件 + 0x07 弹种广播 + method38 原始转储（指定时间窗）
use wotbreplay_parser::replay::Replay;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (path, t0, t1) = (args[1].clone(), args[2].parse::<f32>().unwrap(), args[3].parse::<f32>().unwrap());
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut names = std::collections::HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 58 {
                let l = p[57] as usize;
                if 58 + l <= p.len() {
                    if let Ok(n) = std::str::from_utf8(&p[58..58 + l]) {
                        names.insert(u32le(&p[0..4]), n.to_string());
                    }
                }
            }
        }
    }
    for pkt in &data.packets {
        let c = pkt.clock_secs;
        if c < t0 || c > t1 { continue; }
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() < 12 { continue; }
                let env = u32le(&p[0..4]);
                let m = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if 12 + alen > p.len() { continue; }
                let a = &p[12..12 + alen];
                if m == 0x01 && alen >= 10 {
                    let hp = i16::from_le_bytes([a[0], a[1]]) as i32;
                    let src = u32le(&a[2..6]);
                    let cause = a[6];
                    println!("t={:8.3} m01 血量 env={}({}) hp<={} src={}({}) cause={}",
                        c, env, names.get(&env).map(|s| s.as_str()).unwrap_or("?"), hp,
                        src, names.get(&src).map(|s| s.as_str()).unwrap_or("?"), cause);
                } else if m == 0x07 && alen >= 5 {
                    println!("t={:8.3} m07 弹种 a0={} shell={}", c, a[0], u32le(&a[1..5]));
                } else if m == 0x26 {
                    // method38 命中反馈（Avatar）—— args 布局见 combat.rs
                    println!("t={:8.3} m38 反馈 args[{}]: {}", c, alen,
                        a.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
                } else if m == 0x08 {
                    if alen >= 21 && a[8] == 1 {
                        println!("t={:8.3} m08 直击 shooter={}({}) victim={}({}) res={} cmp={} tail={:02x}",
                            c, u32le(&a[0..4]), names.get(&u32le(&a[0..4])).map(|s| s.as_str()).unwrap_or("?"),
                            u32le(&a[4..8]), names.get(&u32le(&a[4..8])).map(|s| s.as_str()).unwrap_or("?"),
                            a[9], a[10], a[17]);
                    }
                } else if m == 0x00 {
                    println!("t={:8.3} m00 开火 env={}({})", c, env, names.get(&env).map(|s| s.as_str()).unwrap_or("?"));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 26 {
                    println!("t={:8.3} t32 通知 eid={:08x} seg={}", c, u32le(&p[0..4]),
                        u64::from_le_bytes(p[p.len()-8..].try_into().unwrap()));
                }
            }
            _ => {}
        }
    }
}
