//! 修复 Anonyme 重名问题：统计 J39 各实体昵称重复情况，并在 type=5 创建包载荷里
//! 搜索 battle_results 各玩家 account_id（u32 LE）出现的位置 → 建立 eid→account 直接映射。
//! 用法：cargo run --release --example pitch_reprobe_anonmap -- <j39.wotbreplay>
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let br = replay.read_battle_results().ok();
    let data = replay.read_data().unwrap();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // 实体昵称
    let mut eid_to_name: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut creates: Vec<(u32, f32, Vec<u8>)> = Vec::new();
    for pkt in &data.packets {
        if let wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type: 5 } = pkt.payload {
            let p = &pkt.raw_payload[..];
            if p.len() < 60 { continue; }
            let eid = u32le(&p[0..4]);
            creates.push((eid, pkt.clock_secs, p.to_vec()));
            let off = 57;
            let slen = p[off] as usize;
            if (3..=30).contains(&slen) && off + 1 + slen <= p.len() {
                if let Ok(s) = std::str::from_utf8(&p[off + 1..off + 1 + slen]) {
                    if s.chars().all(|c| c.is_ascii_graphic()) {
                        eid_to_name.entry(eid).or_insert_with(|| s.to_string());
                    }
                }
            }
        }
    }
    let mut name_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for n in eid_to_name.values() { *name_count.entry(n.clone()).or_insert(0) += 1; }
    println!("=== 实体昵称（{} 个实体）===", eid_to_name.len());
    let mut items: Vec<_> = name_count.into_iter().collect();
    items.sort();
    for (n, c) in &items { println!("  {:>4} × {}", c, n); }

    // battle_results 玩家
    let mut players: Vec<(u32, String, u32)> = Vec::new(); // (account, nick, tank)
    if let Some(br) = &br {
        for p in &br.players {
            let tank = br.player_results.iter().find(|pr| pr.info.account_id == p.account_id)
                .map(|pr| pr.info.tank_id).unwrap_or(0);
            players.push((p.account_id, p.info.nickname.clone(), tank));
        }
    }
    println!("\n=== battle_results 玩家（{}）===", players.len());
    for (a, n, t) in &players { println!("  account={:<10} {:>16} tank={}", a, n, t); }

    // 在 type=5 载荷里搜每个 account_id
    println!("\n=== account_id 在创建包中的出现位置 ===");
    for (acc, _nick, _tank) in &players {
        let bytes = acc.to_le_bytes();
        for (eid, t, p) in &creates {
            let mut off = 0;
            while off + 4 <= p.len() {
                if &p[off..off + 4] == &bytes[..] {
                    // 排除 eid 字段本身（off==0）
                    println!("  account={} 出现于 eid=0x{:08x} (t={:.2}) 偏移 {}", acc, eid, t, off);
                }
                off += 1;
            }
        }
    }
}
