//! 敌车俯仰重分析 · 第一步概览：实体清单（type=5 昵称）+ 敌我队别（battle_results）
//! + 每实体数据流覆盖（type=7 各 sub/alen 计数、type=10 计数、其他带 eid 前缀的包类型）。
//! 用法：cargo run --release --example pitch_reprobe_overview -- <file.wotbreplay>
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let meta = replay.read_meta().ok();
    let br = replay.read_battle_results().ok();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    println!("client_version={} packets={}", data.client_version, data.packets.len());
    if let Some(m) = &meta {
        println!("author={} tank_id={} duration={:.1}s", m.player_name, m.tank_id, m.battle_duration_secs);
    }
    let clock_max = data.packets.iter().map(|p| p.clock_secs).fold(f32::MIN, f32::max);
    println!("clock range: 0 .. {:.3}s", clock_max);

    // 实体昵称（type=5，同 combat.rs extract_entity_names）
    let mut names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut create_eids: Vec<u32> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            create_eids.push((eid, pkt.clock_secs).0);
            let off = 57;
            let slen = p[off] as usize;
            if (3..=30).contains(&slen) && off + 1 + slen <= p.len() {
                if let Ok(s) = std::str::from_utf8(&p[off + 1..off + 1 + slen]) {
                    if s.chars().all(|c| c.is_ascii_graphic()) {
                        names.entry(eid).or_insert_with(|| s.to_string());
                    }
                }
            }
        }
    }

    // 敌我：battle_results players（team=1/2；作者队 = author.team_number）
    let mut team_of: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
    let mut author_team = 0u8;
    if let Some(br) = &br {
        author_team = if br.author.team_number == 1 { 1 } else { 2 };
        for p in &br.players {
            team_of.insert(p.info.nickname.clone(), if p.info.team == 1 { 1 } else { 2 });
        }
    }

    // 每实体流覆盖
    let mut per_entity: std::collections::BTreeMap<u32, std::collections::BTreeMap<String, usize>> = std::collections::BTreeMap::new();
    let mut bump = |eid: u32, key: String, m: &mut std::collections::BTreeMap<u32, std::collections::BTreeMap<String, usize>>| {
        m.entry(eid).or_default().entry(key).and_modify(|c| *c += 1).or_insert(1);
    };
    for pkt in &data.packets {
        let (ty, p) = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => (0u32, &pkt.raw_payload[..]),
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => (8u32, &pkt.raw_payload[..]),
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => (*packet_type, &pkt.raw_payload[..]),
        };
        if p.len() < 4 { continue; }
        let eid = u32le(&p[0..4]);
        match ty {
            7 => {
                let sub = u32le(&p[4..8]);
                let alen = if p.len() >= 12 { u32le(&p[8..12]) as usize } else { 0 };
                bump(eid, format!("t7sub{}({})", sub, alen), &mut per_entity);
            }
            10 => bump(eid, "t10".into(), &mut per_entity),
            5 => bump(eid, "t5create".into(), &mut per_entity),
            4 => bump(eid, "t4leave".into(), &mut per_entity),
            _ => bump(eid, format!("t{}", ty), &mut per_entity),
        }
    }

    println!("\n=== 实体流覆盖（实体数 {}）===", per_entity.len());
    for (eid, streams) in &per_entity {
        let name = names.get(eid).cloned().unwrap_or_else(|| "-".into());
        let team = team_of.get(&name).copied().unwrap_or(0);
        let side = if team == 0 { "?" } else if team == author_team { "己方" } else { "敌方" };
        let is_author = meta.as_ref().map(|m| m.player_name == name).unwrap_or(false);
        println!("eid=0x{:08x} {:>14} {} {} streams: {:?}", eid, name,
            if is_author { "[作者]" } else { "" }, side, streams);
    }

    // 全局类型直方图
    let mut hist: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for pkt in &data.packets {
        let ty = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        *hist.entry(ty).or_insert(0) += 1;
    }
    println!("\n全局类型直方图: {:?}", hist);
}
