//! 锚点（命中/开火事件）前后 type=10 数据流特征检验：
//! ① 包间隔分布突变 ② 锚点后首包是否同钟属性簇（type=7 多 sub 同钟刷新 = 基线补发特征）
//! 用法：cargo run --example anchor_probe -- <path.wotbreplay>

use std::collections::HashMap;
use wotbreplay_parser::replay::Replay;

fn main() {
    let path = std::env::args().nth(1).expect("usage: anchor_probe <file>");
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

    // 每实体 type=10 时钟序列
    let mut st10: HashMap<u32, Vec<f32>> = HashMap::new();
    for (t, clock, p) in &packets {
        if *t != 10 || p.len() < 48 { continue; }
        st10.entry(u32le(&p[0..4])).or_default().push(*clock);
    }
    for v in st10.values_mut() { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); }
    // 每实体 type=7 (clock, sub) 序列
    let mut prop7: HashMap<u32, Vec<(f32, u32)>> = HashMap::new();
    for (t, clock, p) in &packets {
        if *t != 7 || p.len() < 14 { continue; }
        prop7.entry(u32le(&p[0..4])).or_default().push((*clock, u32le(&p[4..8])));
    }

    // method8 直击事件（victim 锚点）
    let mut anchors: Vec<(u32, f32)> = Vec::new();  // (victim, t)
    for (t, clock, p) in &packets {
        if *t != 8 || p.len() < 22 { continue; }
        if u32le(&p[4..8]) != 0x08 { continue; }
        let args_len = u32le(&p[8..12]) as usize;
        if args_len < 10 || 12 + args_len > p.len() { continue; }
        let a = &p[12..12 + args_len];
        if a[8] != 0x01 { continue; }
        anchors.push((u32le(&a[4..8]), *clock));
    }

    // ① 间隔分布：锚点前最后两个包的间隔 vs 锚点前后跨度
    let mut pre_ivs: Vec<f32> = Vec::new();
    let mut post_ivs: Vec<f32> = Vec::new();
    let mut gap_ratio: Vec<f32> = Vec::new();
    for (victim, t_anchor) in &anchors {
        let Some(clocks) = st10.get(victim) else { continue };
        let pre: Vec<f32> = clocks.iter().filter(|c| **c < *t_anchor).copied().collect();
        let post: Vec<f32> = clocks.iter().filter(|c| **c > *t_anchor).take(2).copied().collect();
        if pre.len() < 2 || post.is_empty() { continue; }
        let pre_iv = pre[pre.len()-1] - pre[pre.len()-2];
        let post_iv = post[0] - pre[pre.len()-1];
        pre_ivs.push(pre_iv);
        post_ivs.push(post_iv);
        if pre_iv > 0.01 { gap_ratio.push(post_iv / pre_iv); }
    }
    let med = |mut v: Vec<f32>| -> f32 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() { 0.0 } else { v[v.len()/2] }
    };
    let big_gap = gap_ratio.iter().filter(|r| **r > 2.0).count();
    println!("=== ① 命中锚点间隔检验（{} 个锚点有双侧采样）", pre_ivs.len());
    println!("  锚点前间隔中位数: {:.3}s  锚点后首间隔中位数: {:.3}s", med(pre_ivs.clone()), med(post_ivs.clone()));
    println!("  锚点后首间隔 > 2×前间隔的比例: {}/{} ({:.0}%)", big_gap, gap_ratio.len(),
        if gap_ratio.is_empty() { 0.0 } else { 100.0 * big_gap as f32 / gap_ratio.len() as f32 });

    // ② 同钟属性簇：锚点后 0.2s 内该 victim 的 type=7 包是否多 sub 同钟
    let mut with_cluster = 0usize; let mut with_any7 = 0usize; let mut checked = 0usize;
    for (victim, t_anchor) in &anchors {
        let Some(seqs) = prop7.get(victim) else { continue };
        checked += 1;
        let near: Vec<(f32, u32)> = seqs.iter()
            .filter(|(c, _)| *c >= *t_anchor && *c <= t_anchor + 0.2).copied().collect();
        if near.is_empty() { continue; }
        with_any7 += 1;
        let mut has = false;
        for i in 0..near.len() {
            for j in i+1..near.len() {
                if (near[i].0 - near[j].0).abs() < 0.002 && near[i].1 != near[j].1 { has = true; }
            }
        }
        if has { with_cluster += 1; }
    }
    println!("=== ② 锚点后属性簇检验（{} 个锚点有 type=7 序列）", checked);
    println!("  锚点后 0.2s 内有 type=7 属性更新: {}/{}", with_any7, checked);
    println!("  其中存在同钟(<2ms)不同 sub 属性簇: {}/{}", with_cluster, with_any7);
}
