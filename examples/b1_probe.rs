//! B1/B3 裁判实验：对比三种锚点状态选择模式的弹道一致性残差。
//! 射手锚点指标：|anchor_pos - ball_a|（ball_a = method29 服务器权威炮口坐标，坦克原点距炮口 ~2-3m）
//! 目标锚点指标：point-to-line(target_pos, ball_a→ball_b)（命中弹弹道弦应穿过车体）
//! 模式：M0=当前（|dt| 最近）  M1=补发快照（锚点后首包 ≤+0.12s）  M2=锚点前双包线性外推
//! 用法：cargo run --example b1_probe -- <path.wotbreplay>

use std::collections::HashMap;
use wotbreplay_parser::replay::Replay;

fn main() {
    let path = std::env::args().nth(1).expect("usage: b1_probe <file>");
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
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let pos_at = |p: &[u8]| [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])];

    // 每实体 type=10 (clock, pos) 序列
    let mut st10: HashMap<u32, Vec<(f32, [f32; 3])>> = HashMap::new();
    for (t, clock, p) in &packets {
        if *t != 10 || p.len() < 48 { continue; }
        st10.entry(u32le(&p[0..4])).or_default().push((*clock, pos_at(p)));
    }
    for v in st10.values_mut() { v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap()); }

    // method29 发射：(shooter, t, ball_a, shot_id)
    struct Launch { shooter: u32, t: f32, ball_a: [f32; 3], shot_id: u32 }
    let mut launches: Vec<Launch> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (t, clock, p) in &packets {
        if *t != 8 || *clock < 5.0 || p.len() < 16 { continue; }
        if u32le(&p[4..8]) != 0x1d { continue; }
        let alen = u32le(&p[8..12]) as usize;
        if alen < 37 || 12 + alen > p.len() { continue; }
        let a = &p[12..12 + alen];
        let sid = u32le(&a[4..8]);
        if !seen.insert(sid) { continue; }
        launches.push(Launch {
            shooter: u32le(&a[0..4]), t: *clock, shot_id: sid,
            ball_a: [f32le(&a[9..13]), f32le(&a[13..17]), f32le(&a[17..21])],
        });
    }
    // method20 终点
    let mut endpoints: HashMap<u32, (f32, [f32; 3])> = HashMap::new();
    for (_t, clock, p) in &packets {
        if p.len() < 28 { continue; }
        if u32le(&p[4..8]) != 0x14 { continue; }
        let alen = u32le(&p[8..12]) as usize;
        if alen < 16 || 12 + alen > p.len() { continue; }
        let a = &p[16..];
        endpoints.entry(u32le(&p[12..16])).or_insert((*clock,
            [f32le(&a[0..4]), f32le(&a[4..8]), f32le(&a[8..12])]));
    }
    // method8 命中：(shooter, victim, t)
    let mut hits: Vec<(u32, u32, f32)> = Vec::new();
    for (_t, clock, p) in &packets {
        if p.len() < 22 { continue; }
        if u32le(&p[4..8]) != 0x08 { continue; }
        let alen = u32le(&p[8..12]) as usize;
        if alen < 10 || 12 + alen > p.len() { continue; }
        let a = &p[12..12 + alen];
        if a[8] != 0x01 { continue; }
        hits.push((u32le(&a[0..4]), u32le(&a[4..8]), *clock));
    }

    let m0 = |eid: u32, t: f32| -> Option<[f32; 3]> {
        st10.get(&eid)?.iter()
            .min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap())
            .map(|(_, p)| *p)
    };
    let m1 = |eid: u32, t: f32| -> Option<[f32; 3]> {
        let seq = st10.get(&eid)?;
        seq.iter().find(|(c, _)| *c > t && *c <= t + 0.12).map(|(_, p)| *p)
            .or_else(|| m0(eid, t))
    };
    let m2 = |eid: u32, t: f32| -> Option<[f32; 3]> {
        let seq = st10.get(&eid)?;
        let pre: Vec<(f32, [f32; 3])> = seq.iter().filter(|(c, _)| *c < t).copied().collect();
        if pre.len() < 2 { return None; }
        let (t0, p0) = pre[pre.len() - 2];
        let (t1, p1) = pre[pre.len() - 1];
        let dt = t1 - t0;
        if dt <= 0.0 || dt > 0.3 || t - t1 > 0.15 { return m0(eid, t); }
        let v = [
            (p1[0] - p0[0]) / dt, (p1[1] - p0[1]) / dt, (p1[2] - p0[2]) / dt];
        Some([p1[0] + v[0] * (t - t1), p1[1] + v[1] * (t - t1), p1[2] + v[2] * (t - t1)])
    };
    let dist = |a: [f32; 3], b: [f32; 3]| -> f32 {
        ((a[0]-b[0]).powi(2) + (a[1]-b[1]).powi(2) + (a[2]-b[2]).powi(2)).sqrt()
    };
    let line_dist = |p: [f32; 3], a: [f32; 3], b: [f32; 3]| -> f32 {
        let ab = [b[0]-a[0], b[1]-a[1], b[2]-a[2]];
        let ap = [p[0]-a[0], p[1]-a[1], p[2]-a[2]];
        let len = (ab[0]*ab[0] + ab[1]*ab[1] + ab[2]*ab[2]).sqrt();
        if len < 1e-6 { return dist(p, a); }
        let cr = [ap[1]*ab[2]-ap[2]*ab[1], ap[2]*ab[0]-ap[0]*ab[2], ap[0]*ab[1]-ap[1]*ab[0]];
        (cr[0]*cr[0] + cr[1]*cr[1] + cr[2]*cr[2]).sqrt() / len
    };
    let stats = |mut v: Vec<f32>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = v.len();
        if n == 0 { return (0.0, 0.0, 0.0); }
        (v[0], v[n*3/4], v[n/2])
    };

    // 射手锚点：|anchor - ball_a|
    let mut s0 = Vec::new(); let mut s1 = Vec::new();
    for l in &launches {
        if let Some(p) = m0(l.shooter, l.t) { s0.push(dist(p, l.ball_a)); }
        if let Some(p) = m1(l.shooter, l.t) { s1.push(dist(p, l.ball_a)); }
    }
    let n0 = s0.len(); let (a, b, c) = stats(s0); println!("射手锚点 M0(最近)   n={n0} min/p75/med = {a:.2}/{b:.2}/{c:.2} m");
    let n1 = s1.len(); let (a, b, c) = stats(s1); println!("射手锚点 M1(补发)   n={n1} min/p75/med = {a:.2}/{b:.2}/{c:.2} m");

    // 目标锚点：命中弹 point-to-line
    let mut t0v = Vec::new(); let mut t1v = Vec::new(); let mut t2v = Vec::new();
    for (shooter, victim, t_hit) in &hits {
        // 配对发射（shooter + ±0.35s）→ shot_id → ball_b
        let l = launches.iter().filter(|l| l.shooter == *shooter && (l.t - *t_hit).abs() <= 0.35)
            .min_by(|x, y| (x.t - *t_hit).abs().partial_cmp(&(y.t - *t_hit).abs()).unwrap());
        let Some(l) = l else { continue };
        let Some((t_end, ball_b)) = endpoints.get(&l.shot_id) else { continue };
        let Some(p0) = m0(*victim, *t_end) else { continue };
        t0v.push(line_dist(p0, l.ball_a, *ball_b));
        if let Some(p) = m1(*victim, *t_end) { t1v.push(line_dist(p, l.ball_a, *ball_b)); }
        if let Some(p) = m2(*victim, *t_end) { t2v.push(line_dist(p, l.ball_a, *ball_b)); }
    }
    let n0 = t0v.len(); let (a, b, c) = stats(t0v); println!("目标锚点 M0(最近)   n={n0} min/p75/med = {a:.2}/{b:.2}/{c:.2} m");
    let n1 = t1v.len(); let (a, b, c) = stats(t1v); println!("目标锚点 M1(补发)   n={n1} min/p75/med = {a:.2}/{b:.2}/{c:.2} m");
    let n2 = t2v.len(); let (a, b, c) = stats(t2v); println!("目标锚点 M2(外推)   n={n2} min/p75/med = {a:.2}/{b:.2}/{c:.2} m");
}
