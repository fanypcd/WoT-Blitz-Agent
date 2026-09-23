//! 在 create 包属性 blob 中搜索打包俯仰初值：
//! 已知受击方进世界时刻 type10 pitch（炮管≈顺坡，车体相对≈0）→ 俯仰域 u16 候选扫描
use std::collections::HashMap;
fn main() {
    let f = std::fs::File::open("C:/Users/Administrator/AppData/Local/wotblitz/DAVAProject/replays/20260906_1436__Anonyme_T110_1155501458890492367.wotbreplay").unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut pitch: HashMap<u32, Vec<(f32, f32)>> = HashMap::new();
    let mut cre: Vec<(f32, u32, Vec<u8>)> = Vec::new();
    for pkt in &data.packets {
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 10 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 48 {
                    pitch.entry(u32le(&p[0..4])).or_default().push((
                        pkt.clock_secs, f32le(&p[40..44])));
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } => {
                let p = &pkt.raw_payload[..];
                if p.len() >= 52 {
                    cre.push((pkt.clock_secs, u32le(&p[0..4]), p.to_vec()));
                }
            }
            _ => {}
        }
    }
    // 全部 4 个大 create 包：进世界时刻 type10 pitch → 打包 u16 候选 = (pitch+360)/0.7
    for (t, eid, p) in &cre {
        let hp = pitch.get(eid).and_then(|m| m.iter()
            .find(|(pt, _)| *pt >= *t && *pt <= t + 0.5))
            .map(|(_, v)| *v).unwrap_or(f32::NAN) * 57.29578;
        if hp.is_nan() { continue; }
        let target_coarse = (hp + 360.0) / 0.7;
        let lo = ((target_coarse - 3.0) as u16).max(0) << 6;
        let hi = ((target_coarse + 3.0) as u16).min(1023) << 6;
        let mut hits = Vec::new();
        for off in 0..p.len().saturating_sub(2) {
            let u = u16::from_le_bytes([p[off], p[off+1]]);
            if u >= lo && u <= hi { hits.push((off, u)); }
        }
        println!("create eid=0x{:08x} t={:.3} pitch={:+.2}° coarse目标={:.0} 候选u16命中: {:?}",
            eid, t, hp, target_coarse, hits.iter().map(|(o, u)| format!("@{}=0x{:04x}", o, u)).collect::<Vec<_>>());
    }
}
