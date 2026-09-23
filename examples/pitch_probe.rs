//! 炮管俯仰通道搜索探针：以"该车自己开炮"时刻的弹速反解俯仰为真值标尺，
//! 在开火 ±0.5s 窗口内扫描所有未解析通道（type7 sub0/sub4、prop2 细分位、
//! type32 短广播/属性流、type39），找与真值相关的载值。
//! 用法：cargo run --example pitch_probe -- <path.wotbreplay> [game_data_dir 缺省]
use std::collections::HashMap;

fn main() {
    let path = std::env::args().nth(1).expect("usage: pitch_probe <file>");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // ① 名册：player eid → 昵称
    let mut nick_of_eid: HashMap<u32, String> = HashMap::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            let l = p[57] as usize;
            if !(3..=30).contains(&l) || 58 + l > p.len() { continue; }
            if let Ok(name) = std::str::from_utf8(&p[58..58 + l]) {
                nick_of_eid.insert(eid, name.to_string());
            }
        }
    }

    // ② method29 发射 + type10 位姿 + 全部可疑包
    struct Launch { t: f32, player: u32, elev_world: f32, ball: [f32; 3] }
    let mut launches: Vec<Launch> = Vec::new();
    let mut pose: HashMap<u32, ([f32; 3], f32, f32, f32)> = HashMap::new(); // eid → (pos,yaw,pitch,roll)
    struct Pkt { t: f32, kind: &'static str, eid: u32, bytes: Vec<u8> }
    let mut misc: Vec<Pkt> = Vec::new();
    let mut seen_sid = std::collections::HashSet::new();
    for pkt in &data.packets {
        let t = pkt.clock_secs;
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                let p = &pkt.raw_payload[..];
                if p.len() < 16 { continue; }
                let eid = u32le(&p[0..4]);
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if 12 + alen > p.len() { continue; }
                let a = &p[12..12 + alen];
                if mid == 0x1d && alen >= 37 {
                    let sid = u32le(&a[4..8]);
                    if seen_sid.insert(sid) {
                        let v = [f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33])];
                        let n = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
                        launches.push(Launch { t, player: u32le(&a[0..4]),
                            elev_world: (v[1] / n).asin(), ball: [f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21])] });
                    }
                }
            }
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => {
                let p = &pkt.raw_payload[..];
                match *packet_type {
                    10 if p.len() >= 48 => {
                        pose.insert(u32le(&p[0..4]), ([f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])],
                            f32le(&p[36..40]), f32le(&p[40..44]), f32le(&p[44..48])));
                    }
                    7 if p.len() >= 14 => {
                        let eid = u32le(&p[0..4]);
                        let sub = u32le(&p[4..8]);
                        let alen = u32le(&p[8..12]) as usize;
                        if 12 + alen > p.len() { continue; }
                        let kind = match sub { 0 => "t7s0", 4 => "t7s4", _ => continue };
                        misc.push(Pkt { t, kind, eid, bytes: p[12..12 + alen].to_vec() });
                    }
                    32 => {
                        // b4 位区分变体；全部截头 16 字节记录
                        let b4 = if p.len() > 4 { p[4] } else { 0 };
                        let kind = if p.len() >= 26 && b4 == 1 { "t32hit" }
                            else if p.len() >= 24 { "t32prop" }
                            else { "t32short" };
                        misc.push(Pkt { t, kind, eid: u32le(&p[0..4]), bytes: p[..p.len().min(20)].to_vec() });
                    }
                    39 if p.len() >= 24 => {
                        misc.push(Pkt { t, kind: "t39", eid: 0, bytes: p[..24].to_vec() });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // ③ 车辆 eid 映射：method29 shooter = 玩家实体；其车辆 = type10 位置最接近 ball 的实体
    let mut veh_of_player: HashMap<u32, u32> = HashMap::new();
    for l in &launches {
        if veh_of_player.contains_key(&l.player) { continue; }
        let mut best: Option<(f32, u32)> = None;
        for (eid, (pos, _, _, _)) in &pose {
            if pos[0] == 0.0 && pos[2] == 0.0 { continue; }
            let d = (pos[0]-l.ball[0]).powi(2) + (pos[2]-l.ball[2]).powi(2);
            if best.map(|(bd, _)| d < bd).unwrap_or(true) { best = Some((d, *eid)); }
        }
        if let Some((_, eid)) = best { veh_of_player.insert(l.player, eid); }
    }

    // ④ 每个玩家开火时刻的Ground Truth + 窗口扫描
    println!("=== 开火时刻真值（弹速反解）===");
    for l in launches.iter().take(14) {
        let nick = nick_of_eid.get(&l.player).map(|s| s.as_str()).unwrap_or("?");
        let veh = veh_of_player.get(&l.player).copied();
        let hull = veh.and_then(|e| pose.get(&e));
        let (rel, hp) = match hull {
            Some((_, yaw, pitch, _)) => (l.elev_world - pitch, *pitch),
            None => (l.elev_world, 0.0),
        };
        println!("t={:8.3} {:<16} veh=0x{:08x} 世界仰角={:+7.2}° 车体pitch={:+6.2}° 相对俯仰={:+7.2}°",
            l.t, nick, veh.unwrap_or(0), l.elev_world * 57.29578, hp * 57.29578, rel * 57.29578);
    }

    // ⑤ 窗口扫描：对每次开火，列出该车 ±0.3s 内全部可疑包原始值
    println!("\n=== 开火 ±0.3s 窗口内可疑包（前 6 次开火）===");
    for l in launches.iter().take(6) {
        let nick = nick_of_eid.get(&l.player).map(|s| s.as_str()).unwrap_or("?");
        let veh = match veh_of_player.get(&l.player) { Some(v) => *v, None => continue };
        println!("--- t={:.3} {} (veh=0x{:08x}, 真值 世界={:+.2}° 相对={:+.2}°) ---",
            l.t, nick, veh, l.elev_world * 57.29578, (l.elev_world - pose.get(&veh).map(|p| p.2).unwrap_or(0.0)) * 57.29578);
        let mut n = 0;
        for m in &misc {
            if m.t < l.t - 0.3 || m.t > l.t + 0.3 { continue; }
            if m.eid != 0 && m.eid != veh { continue; }
            if m.kind == "t39" { continue; }   // 相机流单独看
            let hexv: Vec<String> = m.bytes.iter().map(|b| format!("{:02x}", b)).collect();
            // f32 读法
            let f32s: Vec<String> = m.bytes.chunks(4).filter(|c| c.len() == 4)
                .map(|c| format!("{:.4}", f32le(c))).collect();
            println!("  {:7} t={:8.3} raw[{}] f32[{}]", m.kind, m.t, hexv.join(" "), f32s.join(" "));
            n += 1;
            if n > 25 { println!("  ..."); break; }
        }
    }

    // ⑥ prop2 细分位检验：取某车连续 prop2（这里从 misc 没存，改由独立循环）——单独统计
    println!("\n=== prop2 u16 高10位/低6位行为（验证低6位=插值分数 还是 俯仰）===");
    let mut prop2_seq: Vec<(f32, u32, u16)> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 7 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() >= 14 && u32le(&p[4..8]) == 2 {
                let alen = u32le(&p[8..12]) as usize;
                if alen == 2 && 12 + 2 <= p.len() {
                    prop2_seq.push((pkt.clock_secs, u32le(&p[0..4]),
                        u16::from_le_bytes([p[12], p[13]])));
                }
            }
        }
    }
    // 对每个 eid：低 6 位在"粗角不变"时段的分布
    use std::collections::BTreeMap;
    let mut by_eid: BTreeMap<u32, Vec<(f32, u16)>> = BTreeMap::new();
    for (t, e, u) in &prop2_seq { by_eid.entry(*e).or_default().push((*t, *u)); }
    for (e, seq) in &by_eid {
        // 粗角相同相邻对中低 6 位是否变化（插值分数应连续变化；俯仰则与俯仰事件相关）
        let mut same_coarse = 0; let mut fine_var = 0;
        for w in seq.windows(2) {
            if (w[0].1 >> 6) == (w[1].1 >> 6) {
                same_coarse += 1;
                if (w[0].1 & 63) != (w[1].1 & 63) { fine_var += 1; }
            }
        }
        println!("eid=0x{:08x} samples={} same_coarse_pairs={} fine_bits_changed={}", e, seq.len(), same_coarse, fine_var);
    }
}
