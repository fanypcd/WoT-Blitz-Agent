//! 回放包流探测：验证各 method/type 的 envelope 归属（作者专属 or 全场广播），
//! 并用 battle_results 的 per-player 权威统计对照 method29/method8 提取覆盖率。
//! 用法：cargo run --example replay_probe -- <path.wotbreplay>

use std::collections::{BTreeMap, HashSet};
use wotbreplay_parser::replay::Replay;

fn main() {
    let path = std::env::args().nth(1).expect("usage: replay_probe <file>");
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = Replay::open(f).unwrap();
    let data = replay.read_data().unwrap();
    let packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        (t, pkt.clock_secs, &pkt.raw_payload[..])
    }).collect();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);

    // type=5 名册：entity → 昵称
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    for (t, _, p) in &packets {
        if *t != 5 || p.len() < 60 { continue; }
        let eid = u32le(&p[0..4]);
        let off = 57usize;
        if off >= p.len() { continue; }
        let l = p[off] as usize;
        if !(3..=30).contains(&l) || off + 1 + l > p.len() { continue; }
        if let Ok(s) = std::str::from_utf8(&p[off + 1..off + 1 + l]) {
            if s.chars().all(|c| c.is_ascii_graphic()) {
                names.insert(eid, s.to_string());
            }
        }
    }
    let name = |eid: &u32| names.get(eid).map(|s| s.as_str()).unwrap_or("?").to_string();

    // avatar 实体（pos 全零的 type=10）——每个玩家一个还是仅作者？
    let mut avatars: Vec<u32> = Vec::new();
    for (t, _, p) in &packets {
        if *t != 10 || p.len() < 48 { continue; }
        let pos = [&p[12..16], &p[16..20], &p[20..24]];
        if pos.iter().all(|b| u32le(b) == 0) {
            let eid = u32le(&p[0..4]);
            if !avatars.contains(&eid) { avatars.push(eid); }
        }
    }
    println!("=== avatar 实体（pos 全零 type=10）: {} 个", avatars.len());
    for a in &avatars { println!("  {:08x} ({})", a, name(a)); }

    // EntityMethod 各 method 的 envelope 分布
    let mut method_env: BTreeMap<u32, BTreeMap<u32, usize>> = BTreeMap::new();
    for (t, _, p) in &packets {
        if *t != 8 || p.len() < 12 { continue; }
        let m = u32le(&p[4..8]);
        *method_env.entry(m).or_default().entry(u32le(&p[0..4])).or_insert(0) += 1;
    }
    for m in [0x26u32, 0x24, 0x07, 0x1b, 0x00, 0x0d, 0x23, 0x14, 0x1d] {
        if let Some(envs) = method_env.get(&m) {
            let total: usize = envs.values().sum();
            println!("=== method 0x{:02x}: {} 条, envelope {} 个", m, total, envs.len());
            for (eid, c) in envs.iter().take(8) {
                println!("    {:08x} ({}) × {}", eid, name(eid), c);
            }
            if envs.len() > 8 { println!("    … 共 {} 个 envelope", envs.len()); }
        } else {
            println!("=== method 0x{:02x}: 0 条", m);
        }
    }

    // type=7 子类型分布（sub=2 炮塔角 / 9 炮管俯仰 / 10 伤害计数器），envelope 归属
    for sub in [2u32, 9, 10] {
        let mut envs: BTreeMap<u32, usize> = BTreeMap::new();
        for (t, _, p) in &packets {
            if *t != 7 || p.len() < 14 { continue; }
            if u32le(&p[4..8]) != sub { continue; }
            *envs.entry(u32le(&p[0..4])).or_insert(0) += 1;
        }
        println!("=== type=7 sub={} : {} 条, envelope {} 个", sub, envs.values().sum::<usize>(), envs.len());
        for (eid, c) in envs.iter().take(6) {
            println!("    {:08x} ({}) × {}", eid, name(eid), c);
        }
        if envs.len() > 6 { println!("    … 共 {} 个 envelope", envs.len()); }
    }

    // type=28 弹药槽
    let t28: Vec<&(u32, f32, &[u8])> = packets.iter().filter(|(t, _, p)| *t == 28 && p.len() >= 4).collect();
    println!("=== type=28 弹药槽: {} 条", t28.len());

    // method29 per-shooter（args[0..4] = shooterEntityId）
    let mut shooters29: BTreeMap<u32, usize> = BTreeMap::new();
    let mut seen: HashSet<u32> = HashSet::new();
    // shot_id 冲突审计：(shot_id, shooter, clock) 全记录，检查全局唯一性假设
    let mut all_shots: Vec<(u32, u32, f32)> = Vec::new();
    for (t, clock, p) in &packets {
        if *t != 8 || *clock < 5.0 || p.len() < 16 { continue; }
        if u32le(&p[4..8]) != 0x1d { continue; }
        let args_len = u32le(&p[8..12]) as usize;
        if args_len < 4 || 12 + args_len > p.len() { continue; }
        let sh = u32le(&p[12..16]);
        let sid = if args_len >= 8 { u32le(&p[16..20]) } else { 0 };
        all_shots.push((sid, sh, *clock));
        if !seen.insert(sid) { continue; }
        *shooters29.entry(sh).or_insert(0) += 1;
    }
    // 冲突明细：同 shot_id 多条（不同 shooter 或同 shooter 多发）
    all_shots.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.partial_cmp(&b.2).unwrap()));
    let mut i = 0;
    let mut dup_groups = 0;
    while i < all_shots.len() {
        let mut j = i + 1;
        while j < all_shots.len() && all_shots[j].0 == all_shots[i].0 { j += 1; }
        if j - i > 1 {
            dup_groups += 1;
            let cross = all_shots[i..j].iter().any(|s| s.1 != all_shots[i].1);
            println!("=== shot_id {} 重复 {} 条 (跨 shooter={})", all_shots[i].0, j - i, cross);
            for (_, sh, t) in &all_shots[i..j] {
                println!("    shooter {:08x} ({}) t={:.2}s", sh, name(sh), t);
            }
        }
        i = j;
    }
    println!("=== shot_id 重复组: {} 个", dup_groups);

    // method8 per-shooter + result 分布
    let mut shooters8: BTreeMap<u32, usize> = BTreeMap::new();
    let mut res8: BTreeMap<u8, usize> = BTreeMap::new();
    for (t, _, p) in &packets {
        if *t != 8 || p.len() < 22 { continue; }
        if u32le(&p[4..8]) != 0x08 { continue; }
        let args_len = u32le(&p[8..12]) as usize;
        if args_len < 10 || 12 + args_len > p.len() { continue; }
        let a = &p[12..12 + args_len];
        if a[8] != 0x01 { continue; }
        *shooters8.entry(u32le(&a[0..4])).or_insert(0) += 1;
        *res8.entry(a[9]).or_insert(0) += 1;
    }
    println!("=== method8 result 枚举分布: {:?}", res8);

    // method0x00 开火事件 per-envelope（envelope = 射手车辆实体，全场广播）
    let mut fire00: BTreeMap<u32, usize> = BTreeMap::new();
    for (t, _, p) in &packets {
        if *t != 8 || p.len() < 12 { continue; }
        if u32le(&p[4..8]) != 0x00 { continue; }
        *fire00.entry(u32le(&p[0..4])).or_insert(0) += 1;
    }
    let mut type32_count = 0usize;
    for (t, _, p) in &packets {
        if *t != 32 || p.len() < 26 { continue; }
        if p[4] != 0x01 { continue; }
        let m = u32le(&p[5..9]);
        if m == 0x11 || m == 0x12 { type32_count += 1; }
    }
    println!("=== type=32 命中通知 (0x11/0x12): {} 条", type32_count);

    // battle_results per-player 权威统计对照
    let br = replay.read_battle_results().ok();
    if let Some(br) = br {
        println!("=== battle_results per-player vs 提取计数（method29 / method8）===");
        println!("{:<28} {:>5} {:>6} {:>6} {:>8} | {:>4} {:>4} {:>4} {:>8}",
            "player", "team", "n_shot", "n_hits", "dmg_dealt", "m29", "m00", "m8", "m29-n_shot");
        // 血量链 per-source 伤害（method1）
        let mut hp_dmg: BTreeMap<u32, u32> = BTreeMap::new();
        for (t, _, p) in &packets {
            if *t != 8 || p.len() < 19 { continue; }
            if u32le(&p[4..8]) != 0x01 { continue; }
            let args_len = u32le(&p[8..12]) as usize;
            if args_len != 7 || 12 + args_len > p.len() { continue; }
            let a = &p[12..19];
            let _hp = u16::from_le_bytes([a[0], a[1]]);
            let src = u32le(&a[2..6]);
            let cause = a[6];
            // 简化：每 source 累计 HP 降幅需要时间链，这里用"下降事件"粗计（仅供对照）
            if cause == 0 { hp_dmg.entry(src).or_insert(0); }
        }
        for p in &br.players {
            let pr = br.player_results.iter().find(|pr| pr.info.account_id == p.account_id);
            let (ns, nh, dd) = pr.map(|pr| (pr.info.n_shots, pr.info.n_hits_dealt, pr.info.damage_dealt)).unwrap_or((0, 0, 0));
            // 借用比较（避免每次 name(e).to_string() 分配）
            let nick = p.info.nickname.as_str();
            let named = |e: &u32| names.get(e).map(|s| s.as_str()) == Some(nick);
            let m29 = shooters29.iter().find(|(e, _)| named(e)).map(|(_, c)| *c as u32).unwrap_or(0);
            let m8 = shooters8.iter().find(|(e, _)| named(e)).map(|(_, c)| *c as u32).unwrap_or(0);
            let m00 = fire00.iter().find(|(e, _)| named(e)).map(|(_, c)| *c as u32).unwrap_or(0);
            let _hp = hp_dmg.iter().find(|(e, _)| named(e)).map(|(_, c)| *c as u32).unwrap_or(0);
            println!("{:<28} {:>5} {:>6} {:>6} {:>8} | {:>4} {:>4} {:>4} {:>8}",
                p.info.nickname, p.info.team, ns, nh, dd, m29, m00, m8,
                if ns >= m29 { ns - m29 } else { 0 });
        }
    }
}
