//! 综合提取探针：把待解包型原始事件一次性导出 JSON（每回放一个文件），
//! 供 Python 侧做关联分析（装甲指纹/弹种/装填/状态流）。
//! 用法：cargo run --release --example dump_round -- <path.wotbreplay> <out.json> [name]
use std::io::Write;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_round <in> <out.json> [name]");
    let out_path = std::env::args().nth(2).expect("missing out path");
    let name = std::env::args().nth(3).unwrap_or_default();
    let f = std::fs::File::open(&path).unwrap();
    let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
    let meta_tank = replay.read_meta().ok().map(|m| m.tank_id as u32).unwrap_or(0);
    let data = replay.read_data().unwrap();
    let roster: Vec<(u32, String, u32)> = replay
        .read_battle_results()
        .map(|br| {
            br.player_results
                .iter()
                .filter_map(|pr| {
                    let nick = br
                        .players
                        .iter()
                        .find(|p| p.account_id == pr.info.account_id)
                        .map(|p| p.info.nickname.clone())?;
                    Some((pr.info.account_id, nick, pr.info.tank_id))
                })
                .collect()
        })
        .unwrap_or_default();
    let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{:02x}", x)).collect() };

    let mut out = std::fs::File::create(&out_path).unwrap();
    let mut first = true;
    macro_rules! w {
        ($s:expr) => { let _ = out.write_all(($s).as_bytes()); };
    }
    macro_rules! put {
        ($s:expr) => {{
            if first { first = false; } else { let _ = out.write_all(b","); }
            let _ = out.write_all(($s).as_bytes());
        }};
    }

    let roster_json: Vec<String> = roster
        .iter()
        .map(|(acc, nick, tank)| format!(
            "{{\"acc\":{},\"nick\":\"{}\",\"tank_id\":{}}}", acc, nick.replace('"', "'"), tank))
        .collect();
    w!(format!(
        "{{\"file\":\"{}\",\"name\":\"{}\",\"author_tank_id\":{},\"roster\":[{}],\"events\":[",
        path.replace('\\', "/"), name, meta_tank, roster_json.join(",")));

    for pkt in &data.packets {
        let t = pkt.clock_secs;
        let p: &[u8] = &pkt.raw_payload;
        match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => {
                if p.len() < 12 { continue; }
                let eid = u32le(&p[0..4]);
                let mid = u32le(&p[4..8]);
                let alen = u32le(&p[8..12]) as usize;
                if 12 + alen > p.len() { continue; }
                let a = &p[12..12 + alen];
                match mid {
                    // method8 直击通知：[shooter][victim][cnt][result][cmpIndex][hash6 6B][tail 4B]
                    0x08 if alen >= 21 => {
                        put!(format!(
                            "{{\"k\":\"m8\",\"t\":{:.3},\"eid\":{},\"shooter\":{},\"victim\":{},\"cnt\":{},\"result\":{},\"cmp\":{},\"hash6\":\"{}\",\"tail\":\"{}\"}}",
                            t, eid, u32le(&a[0..4]), u32le(&a[4..8]), a[8], a[9], a[10],
                            hex(&a[11..17]), hex(&a[alen-4..])));
                    }
                    // method29 发射：[shooter][shotId][rawFlag][lp 12][vel 12][terminalRaw]
                    0x1d if alen >= 37 => {
                        put!(format!(
                            "{{\"k\":\"m29\",\"t\":{:.3},\"eid\":{},\"shooter\":{},\"shotId\":{},\"rawFlag\":{},\"lp\":[{:.3},{:.3},{:.3}],\"vel\":[{:.4},{:.4},{:.4}],\"termRaw\":{:.6}}}",
                            t, eid, u32le(&a[0..4]), u32le(&a[4..8]), a[8],
                            f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21]),
                            f32le(&a[21..25]), f32le(&a[25..29]), f32le(&a[29..33]),
                            f32le(&a[33..37])));
                    }
                    // method20 终点：[shotId][point 12B]（含更长变体全部导出）
                    0x14 => {
                        if alen >= 16 {
                            put!(format!(
                                "{{\"k\":\"m20\",\"t\":{:.3},\"eid\":{},\"shotId\":{},\"p\":[{:.3},{:.3},{:.3}],\"tail\":\"{}\"}}",
                                t, eid, u32le(&a[0..4]),
                                f32le(&a[4..8]), f32le(&a[8..12]), f32le(&a[12..16]),
                                hex(&a[16..])));
                        } else {
                            put!(format!("{{\"k\":\"m20s\",\"t\":{:.3},\"eid\":{},\"args\":\"{}\"}}", t, eid, hex(a)));
                        }
                    }
                    // method38 命中结果（作者）
                    0x26 => {
                        put!(format!("{{\"k\":\"m38\",\"t\":{:.3},\"eid\":{},\"args\":\"{}\"}}", t, eid, hex(a)));
                    }
                    // method0x1b 地形命中
                    0x1b => {
                        put!(format!("{{\"k\":\"m1b\",\"t\":{:.3},\"eid\":{},\"args\":\"{}\"}}", t, eid, hex(a)));
                    }
                    // method0x07 弹种广播
                    0x07 => {
                        put!(format!("{{\"k\":\"m07\",\"t\":{:.3},\"eid\":{},\"args\":\"{}\"}}", t, eid, hex(a)));
                    }
                    // 装填族 + 开火
                    0x00 | 0x0d | 0x23 => {
                        put!(format!("{{\"k\":\"m{:02x}\",\"t\":{:.3},\"eid\":{},\"args\":\"{}\"}}", mid, t, eid, hex(a)));
                    }
                    // 关注的未知/半解 mid（0x26 被 m38 分支先匹配，这里到不了）
                    0x02 | 0x04 | 0x06 | 0x0c | 0x10 | 0x11 | 0x12 | 0x16 | 0x27 | 0x30 => {
                        put!(format!("{{\"k\":\"m{:02x}\",\"t\":{:.3},\"eid\":{},\"args\":\"{}\"}}", mid, t, eid, hex(a)));
                    }
                    _ => {}
                }
            }
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => {}
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => {
                match *packet_type {
                    // type=10 姿态（49B）：[eid 4][?8][pos@12][state 3×f32 小量@24][ang yaw/pitch/roll@36][01@48]
                    10 if p.len() >= 49 => {
                        put!(format!(
                            "{{\"k\":\"t10\",\"t\":{:.3},\"eid\":{},\"pos\":[{:.3},{:.3},{:.3}],\"state\":[{:.8},{:.8},{:.8}],\"ang\":[{:.5},{:.5},{:.5}],\"tail\":\"{}\"}}",
                            t, u32le(&p[0..4]),
                            f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24]),
                            f32le(&p[24..28]), f32le(&p[28..32]), f32le(&p[32..36]),
                            f32le(&p[36..40]), f32le(&p[40..44]), f32le(&p[44..48]),
                            hex(&p[48..])));
                    }
                    // type=7 属性流（全量）
                    7 if p.len() >= 12 => {
                        let ln = u32le(&p[8..12]) as usize;
                        if 12 + ln <= p.len() {
                            put!(format!(
                                "{{\"k\":\"t7\",\"t\":{:.3},\"eid\":{},\"prop\":{},\"v\":\"{}\"}}",
                                t, u32le(&p[0..4]), u32le(&p[4..8]), hex(&p[12..12 + ln])));
                        }
                    }
                    // type=32 全变体
                    32 => {
                        put!(format!("{{\"k\":\"t32\",\"t\":{:.3},\"eid\":{},\"raw\":\"{}\"}}", t, u32le(&p[0..4]), hex(p)));
                    }
                    // type=5 实体创建（昵称 @offset 57）
                    5 if p.len() >= 60 => {
                        let off = 57usize;
                        let sl = p[off] as usize;
                        if (3..=30).contains(&sl) && off + 1 + sl <= p.len() {
                            if let Ok(s) = std::str::from_utf8(&p[off + 1..off + 1 + sl]) {
                                if s.chars().all(|c| c.is_ascii_graphic()) {
                                    put!(format!("{{\"k\":\"t5\",\"t\":{:.3},\"eid\":{},\"nick\":\"{}\"}}",
                                        t, u32le(&p[0..4]), s.replace('"', "'")));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    w!("]}");
    println!("wrote {} (author_tank={})", out_path, meta_tank);
}
