use std::collections::HashMap;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 16 { continue; }
            let mid = u32le(&p[4..8]);
            let alen = u32le(&p[8..12]) as usize;
            if mid != 0x07 || alen < 4 || 12 + alen > p.len() { continue; }
            let a = &p[12..12 + alen];
            let yaw_p = u16::from_le_bytes([a[0], a[1]]);
            let pitch_p = u16::from_le_bytes([a[2], a[3]]);
            let coarse = pitch_p >> 6;
            let mut pitch = coarse as f64 * std::f64::consts::TAU * 7.0 / 3600.0 - std::f64::consts::TAU;
            while pitch > std::f64::consts::PI { pitch -= std::f64::consts::TAU; }
            while pitch < -std::f64::consts::PI { pitch += std::f64::consts::TAU; }
            let yaw = yaw_p as f64 / 65536.0 * std::f64::consts::TAU - std::f64::consts::PI;
            println!("t={:8.3} eid=0x{:08x} yaw16={:5} pitch16={:5} coarse={:4} yaw={:+7.2}° pitch={:+6.2}°",
                pkt.clock_secs, u32le(&p[0..4]), yaw_p, pitch_p, coarse,
                yaw * 57.29578, pitch * 57.29578);
        }
    }
}
