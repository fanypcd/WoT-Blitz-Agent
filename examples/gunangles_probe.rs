//! 验证 set_gunAnglesPacked（type=8 method 0x10）的参数布局与解码公式：
//! 反汇编结论：args = [u16 yaw_packed][u16 pitch_packed]；
//! yaw  = yaw_packed/65535 × 2π − π（0x10000 满量程，OnReadTurret push 0x10000）
//! pitch: coarse = pitch_packed>>6 (1024 级), fine = &63
//!        pitch = coarse × 2π × 7/3600 − 2π（wrap 到 ±π）＋ fine × (yaw_B−yaw_A)/63 细分?
//!        ——反汇编只用了 coarse 一路（pitch 无 fine 插值），输出后过 0x1441e00 归一化。
//! 用法：cargo run --example gunangles_probe -- <file>
use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let u16le = |b: &[u8]| u16::from_le_bytes([b[0], b[1]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut nick_of_eid: HashMap<u32, String> = HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            let l = p[57] as usize;
            if !(3..=30).contains(&l) || 58 + l > p.len() { continue; }
            if let Ok(name) = std::str::from_utf8(&p[58..58 + l]) { nick_of_eid.insert(eid, name.to_string()); }
        }
    }
    // method29 发射（真值）
    let mut fires: Vec<(f32, u32, f32)> = Vec::new(); // (t, player_eid, world_elev)
    let mut seen = std::collections::HashSet::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 49 {
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if mid == 0x1d && alen >= 37 && 12 + alen <= p.len() {
                    let a = &p[12..12 + alen];
                    let sid = u32le(&a[4..8]);
                    if seen.insert(sid) {
                        let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                        let n = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
                        fires.push((pkt.clock_secs, u32le(&a[0..4]), (v[1]/n).asin()));
                    }
                }
            }
        }
    }
    // type=8 method 0x10 解码
    let mut per_eid: HashMap<u32, Vec<(f32, f32, f32)>> = HashMap::new(); // eid → (t, yaw, pitch)
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid != 0x10 || alen < 4 || 12 + alen > p.len() { continue; }
            let eid = u32le(&p[0..4]);
            let a = &p[12..12 + alen];
            let yaw_p = u16le(&a[0..2]) as f64;
            let pitch_p = u16le(&a[2..4]) as f64;
            let yaw = yaw_p / 65535.0 * std::f64::consts::TAU - std::f64::consts::PI;
            // 反汇编公式: pitch = coarse × 2π × 7/3600 − 2π → wrap
            let coarse = (pitch_p as u16 >> 6) as f64;
            let mut pitch = coarse * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
            while pitch > std::f64::consts::PI { pitch -= std::f64::consts::TAU; }
            while pitch < -std::f64::consts::PI { pitch += std::f64::consts::TAU; }
            per_eid.entry(eid).or_default().push((pkt.clock_secs, yaw as f32, pitch as f32));
        }
    }
    let mut total = 0;
    for (e, v) in &per_eid { total += v.len(); }
    println!("=== type=8 method0x10 set_gunAnglesPacked: {} 包, {} 实体 ===", total, per_eid.len());
    for (e, v) in &per_eid {
        let nick = nick_of_eid.get(e).map(|s| s.as_str()).unwrap_or("?");
        println!("eid=0x{:08x}({}) {} 包, 时段 {:.2}~{:.2}", e, nick, v.len(),
            v.first().map(|x| x.0).unwrap_or(0.0), v.last().map(|x| x.0).unwrap_or(0.0));
    }
    // 开火时刻对照：真值 = 该玩家 method29 弹速仰角（注意 player eid ≠ vehicle eid——
    // method29 shooter 是玩家实体；method10 的 eid 是车辆实体。此处直接对
    // 车辆实体包打"附近有开火"标记并打印真值序列）
    println!("\n=== 开火时刻 ±0.15s 的 method10 角度（对照真值）===");
    for (ft, pe, elev) in &fires {
        let nick = nick_of_eid.get(pe).map(|s| s.as_str()).unwrap_or("?").to_string();
        println!("t={:.3} {} 真值世界仰角={:+.2}°", ft, nick, elev * 57.29578);
        for (e, v) in &per_eid {
            for (t, yaw, pitch) in v {
                if (*t - *ft).abs() <= 0.15 {
                    println!("    veh=0x{:08x} t={:.3} yaw={:+.1}° pitch={:+.2}°", e, t, yaw * 57.29578, pitch * 57.29578);
                }
            }
        }
    }
}
