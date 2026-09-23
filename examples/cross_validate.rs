//! 跨回放批量验证：
//! 1. prop9 宿主实体数（结论：恒=1，己方 avatar）
//! 2. prop9 值域（若=炮管俯仰应限于俯仰极限±15°；若=瞄准/相机可超）
//! 3. type=8 方法直方图差异（是否存在未见的、可能携带俯仰的方法/参数组合）
//! 4. 每车 prop2 俯仰解码超限率（结论：=偏航，俯仰解码物理不可能）
use std::collections::HashMap;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut summary: Vec<String> = Vec::new();
    let mut method_universe: HashMap<u32, usize> = HashMap::new();
    let mut p9_host_ok = 0; let mut p9_host_bad = 0;
    let mut p2_impossible_total = 0; let mut p2_total = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !name.ends_with(".wotbreplay") { continue; }
        let f = match std::fs::File::open(&path) { Ok(f) => f, Err(_) => continue };
        let mut replay = match wotbreplay_parser::replay::Replay::open(f) { Ok(r) => r, Err(_) => { summary.push(format!("{}: 解析失败", &name[..name.len().min(24)])); continue; } };
        let data = match replay.read_data() { Ok(d) => d, Err(_) => { summary.push(format!("{}: 数据读取失败", &name[..name.len().min(24)])); continue; } };
        let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let mut p9_host: HashMap<u32, usize> = HashMap::new();
        let mut p9_vals: Vec<f32> = Vec::new();
        let mut p2_by_eid: HashMap<u32, Vec<u16>> = HashMap::new();
        let mut methods: HashMap<u32, HashMap<(usize,), usize>> = HashMap::new();
        for pkt in &data.packets {
            match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } => {
                    let p = &pkt.raw_payload[..];
                    if p.len() >= 16 && u32le(&p[4..8]) == 9 {
                        let alen = u32le(&p[8..12]) as usize;
                        if alen == 4 && 12 + 4 <= p.len() {
                            let e = u32le(&p[0..4]);
                            *p9_host.entry(e).or_insert(0) += 1;
                            p9_vals.push(f32le(&p[12..16]));
                        }
                    }
                    if p.len() >= 14 && u32le(&p[4..8]) == 2 {
                        let alen = u32le(&p[8..12]) as usize;
                        if alen == 2 && 12 + 2 <= p.len() {
                            p2_by_eid.entry(u32le(&p[0..4])).or_default()
                                .push(u16::from_le_bytes([p[12], p[13]]));
                        }
                    }
                }
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                    let p = &pkt.raw_payload[..];
                    if p.len() >= 16 {
                        let mid = u32le(&p[4..8]);
                        let alen = u32le(&p[8..12]) as usize;
                        *method_universe.entry(mid).or_insert(0) += 1;
                        methods.entry(mid).or_default().entry((alen,)).or_insert(0);
                    }
                }
                _ => {}
            }
        }
        let short = &name[..name.len().min(16)];
        // 1. prop9 宿主
        if p9_host.len() == 1 { p9_host_ok += 1; } else if p9_host.len() > 1 { p9_host_bad += 1; }
        let p9r = if p9_vals.is_empty() { "无".to_string() } else {
            let mn = p9_vals.iter().cloned().fold(f32::MAX, f32::min);
            let mx = p9_vals.iter().cloned().fold(f32::MIN, f32::max);
            format!("{:+.2}~{:+.2}°", mn * 57.29578, mx * 57.29578)
        };
        // 2. prop2 俯仰解码超限统计（|pitch_dec wrap| > 15° 的占比 → 应接近 100% 若=偏航）
        let mut imp = 0; let mut tot = 0;
        for (_e, vals) in &p2_by_eid {
            for &u in vals {
                let coarse = (u >> 6) as f64;
                let mut pd = coarse * 0.7 - 360.0;
                while pd > 180.0 { pd -= 360.0; }
                while pd < -180.0 { pd += 360.0; }
                tot += 1;
                if pd.abs() > 15.0 { imp += 1; }
            }
        }
        p2_impossible_total += imp; p2_total += tot;
        let n_p9 = if p9_host.len() == 1 { p9_host.values().next().copied().unwrap_or(0) } else { 0 };
        summary.push(format!("{}: p9宿主={} p9包={} p9域={} | p2包={} 俯仰解码超限={}/{}",
            short, p9_host.len(), n_p9, p9r, p2_by_eid.values().map(|v| v.len()).sum::<usize>(), imp, tot));
    }
    println!("=== 汇总（{} 回放）===", summary.len());
    for s in &summary { println!("{}", s); }
    println!("\nprop9 宿主=1 的回放: {}/{}; 宿主>1: {}", p9_host_ok, summary.len(), p9_host_bad);
    println!("prop2 俯仰解码超限总计: {}/{} ({}%)——>15° 即非俯仰（=偏航）",
        p2_impossible_total, p2_total, if p2_total > 0 { p2_impossible_total * 100 / p2_total } else { 0 });
    println!("\n全量 method 直方图（跨 {} 回放）:", summary.len());
    let mut mv: Vec<(u32, usize)> = method_universe.into_iter().collect();
    mv.sort_by_key(|(m, _)| *m);
    for (m, c) in mv { println!("  0x{:02x}: {}", m, c); }
}
