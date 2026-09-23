//! type=8 方法流探针：methodID 直方图（含车辆实体标记）+ 指定 methodID 的逐包转储。
//! 用法：
//!   cargo run --example method_probe -- <path.wotbreplay>              # 直方图
//!   cargo run --example method_probe -- <path.wotbreplay> <methodID>   # 该 method 逐包转储（t/eid/args hex）

use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).unwrap().clone();
    let want: Option<u32> = args.get(2).and_then(|s| s.parse().ok());
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    if args.get(2).map(|s| s == "pos").unwrap_or(false) {
        // pos 模式：method_probe <file> pos <eid-hex> <t> —— 打印该实体 type=10 采样（t±0.3s）
        let eid = u32::from_str_radix(args.get(3).unwrap_or(&"0".to_string()).trim_start_matches("0x"), 16).unwrap();
        let t: f32 = args.get(4).unwrap_or(&"0".to_string()).parse().unwrap();
        for pkt in data.packets.iter() {
            if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } = pkt.payload {
                let p = &pkt.raw_payload[..];
                let cur = u32le(&p[0..4]);
                let hit = if eid == 0 { (pkt.clock_secs - t).abs() <= 0.05 } else { cur == eid && (pkt.clock_secs - t).abs() <= 0.3 };
                if p.len() >= 48 && hit {
                    println!("t={:8.3} eid={:08x} pos=({:.2}, {:.2}, {:.2}) yaw={:.3}",
                        pkt.clock_secs, cur, f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24]), f32le(&p[36..40]));
                }
            }
        }
        return;
    }
    if args.get(2).map(|s| s == "t32").unwrap_or(false) {
        // t32 模式：method_probe <file> t32 [eid-hex] —— type=32 命中通知逐包转储
        // （[eid u32][01][method u32][u16@9][flag@11][hash6@12..18][segment@18..]），
        // u16@9/flag@11 为未解字段——候选：受击者炮管俯仰（document 旧解读，待 prop9 对照）
        let eid_filter = args.get(3).map(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).unwrap());
        for pkt in data.packets.iter() {
            if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 32 } = pkt.payload {
                let p = &pkt.raw_payload[..];
                if p.len() < 26 { continue; }
                if p[4] != 0x01 { continue; }
                let m = u32le(&p[5..9]);
                if m != 0x11 && m != 0x12 { continue; }
                if let Some(ef) = eid_filter { if u32le(&p[0..4]) != ef { continue; } }
                println!("t={:8.3} eid={:08x} m={:02x} u16@9={:04x} flag@11={:02x} hash6={} seg={:016x}",
                    pkt.clock_secs, u32le(&p[0..4]), m,
                    u16::from_le_bytes([p[9], p[10]]), p[11],
                    p[12..18].iter().map(|x| format!("{:02x}", x)).collect::<String>(),
                    u64::from_le_bytes(p[18..26].try_into().unwrap()));
            }
        }
        return;
    }

    // 车辆实体集合（type=10 流的 eid）
    let mut vehicle_eids: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } = pkt.payload {
            if pkt.raw_payload.len() >= 4 {
                vehicle_eids.insert(u32le(&pkt.raw_payload[0..4]));
            }
        }
    }

    let mut methods: BTreeMap<u32, (usize, BTreeMap<u8, usize>, Vec<String>, usize)> = BTreeMap::new();
    for pkt in &data.packets {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            _ => continue,
        };
        if t != 8 { continue; }
        let p = &pkt.raw_payload[..];
        if p.len() < 12 { continue; }
        let mid = u32le(&p[4..8]);
        let alen = u32le(&p[8..12]) as usize;
        if 12 + alen > p.len() { continue; }
        let eid = u32le(&p[0..4]);
        let on_vehicle = vehicle_eids.contains(&eid);
        if let Some(w) = want {
            if mid != w { continue; }
            println!("t={:8.3} eid={:08x} args[{:3}]={}", pkt.clock_secs, eid, alen,
                p[12..12 + alen].iter().map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" "));
            continue;
        }
        let e = methods.entry(mid).or_default();
        e.0 += 1;
        if on_vehicle { e.3 += 1; }
        if alen >= 1 { *e.1.entry(p[12]).or_insert(0) += 1; }
        if e.2.len() < 3 && alen > 0 {
            e.2.push(format!("{} eid={:08x} args[{}]={}", if on_vehicle { "V" } else { "." }, eid, alen,
                p[12..(12 + alen).min(p.len())].iter().take(16)
                    .map(|x| format!("{:02x}", x)).collect::<Vec<_>>().join(" ")));
        }
    }
    println!("=== type=8 methodID 直方图（{} 个 method；nV=车辆实体包数）===", methods.len());
    for (mid, (cnt, fb, samples, n_veh)) in &methods {
        println!("method {:>3} (0x{:02x}): n={:<6} nV={:<5} args0: {}", mid, mid, cnt, n_veh,
            fb.iter().take(3)
                .map(|(k, c)| format!("{:#04x}×{}", k, c)).collect::<Vec<_>>().join(", "));
        for s in samples { println!("   {}", s); }
    }
}
