//! 全流载荷尺寸扫描：找 type=7/其他类型的 protobuf 大包（VehicleDetails/optDevicePreset 候选）。
use std::collections::HashMap;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let f = std::fs::File::open(&args[1]).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let mut hist: HashMap<u32, (usize, usize, Vec<usize>)> = HashMap::new(); // type -> (count, max, sample lens)
    for pkt in &data.packets {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            _ => 999,
        };
        let e = hist.entry(t).or_insert((0, 0, Vec::new()));
        e.0 += 1;
        if pkt.raw_payload.len() > e.1 {
            e.1 = pkt.raw_payload.len();
            if e.2.len() < 8 { e.2.push(pkt.raw_payload.len()); }
        }
    }
    let mut ts: Vec<_> = hist.iter().map(|(k,v)| (*k, v)).collect();
    ts.sort_by_key(|(k,_)| std::cmp::Reverse(*k));
    for (t,(cnt,max,lens)) in ts {
        println!("type={:>3} n={:>5} max={:>6}", t, cnt, max);
    }
    // 大包明细（type=7 大于 20B 的）
    println!("\ntype=7 包体 >20B 明细：");
    for pkt in &data.packets {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            _ => 999,
        };
        if t == 7 && pkt.raw_payload.len() > 20 {
            println!("  clock={:.2} len={}", pkt.clock_secs, pkt.raw_payload.len());
        }
    }
    // type=8 里 args 大于 37 的 method（其他 VehicleDetails 载体候选）
    println!("\ntype=8 method args_len 分布（非 0x1d/0x14/0x08/0x26/0x07）:");
    for pkt in &data.packets {
        if !matches!(pkt.payload, wotbreplay_parser::models::data::payload::Payload::EntityMethod(_)) { continue; }
        let p = &pkt.raw_payload;
        if p.len() < 12 { continue; }
        let method = u32::from_le_bytes([p[4],p[5],p[6],p[7]]);
        let al = u32::from_le_bytes([p[8],p[9],p[10],p[11]]) as usize;
        if !matches!(method, 0x1d|0x14|0x08|0x26|0x07|0x00) && al > 20 {
            println!("  method={:#x} args={} clock={:.2}", method, al, pkt.clock_secs);
        }
    }
}
