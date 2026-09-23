//! 俯仰锚定粒度审计（根级 #[path] 复用 crate 源码，使 crate::data 可解析）：
//! 1) 车辆级（当前实现：gun_angles.json 每车一对）vs 模块级（models.pb 每炮塔×主炮
//!    PitchLimitsInfo）——统计有多少车存在配置间俯仰差异；
//! 2) front/back 扇区极值差异（同一炮前后俯仰不同）的覆盖面；
//! 3) gun_angles.json 的 (dep,ele) 与 models.pb 顶级配置 (min,max) 的对应关系。
//! 用法：cargo run --release --example pitch_granularity_stats
use std::collections::HashMap;

#[path = "../src/data.rs"]
pub mod data;
#[path = "../src/wargaming/blitzkit.rs"]
pub mod blitzkit;

fn main() {
    let tanks = blitzkit::load_tanks();
    println!("tanks.pb 车辆数: {}", tanks.len());

    let ga: HashMap<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string("data/gun_angles.json").unwrap()).unwrap();
    println!("gun_angles.json 条目数: {}", ga.len());

    let (mut n_parsed, mut n_multi_cfg, mut n_cfg_diff, mut n_fb_diff) = (0, 0, 0, 0);
    let mut diff_examples: Vec<String> = Vec::new();
    let mut fb_examples: Vec<String> = Vec::new();
    let mut ga_mismatch: Vec<String> = Vec::new();

    for (id, tank) in tanks.iter() {
        let Some(mi) = blitzkit::model_info(*id) else { continue };
        // 该车全部炮塔×主炮的 pitch_limits
        let mut cfgs: Vec<(String, f32, f32)> = Vec::new(); // (gun 名, dep=max, ele=-min)
        for tur in &tank.turrets {
            for gun in &tur.guns {
                let pl = mi.turrets.iter()
                    .flat_map(|t| t.guns.iter())
                    .find(|gm| gm.gun_module_id == gun.module_id)
                    .and_then(|gm| gm.pitch_limits.clone());
                if let Some(pl) = pl {
                    cfgs.push((gun.name.clone(), pl.max, -pl.min));
                    if let (Some(f), Some(b)) = (&pl.front, &pl.back) {
                        if (f.min - b.min).abs() > 0.05 || (f.max - b.max).abs() > 0.05 {
                            n_fb_diff += 1;
                            if fb_examples.len() < 6 {
                                fb_examples.push(format!("{} [{}]: front(min{:.1},max{:.1}) back(min{:.1},max{:.1})",
                                    tank.name, gun.name, f.min, f.max, b.min, b.max));
                            }
                        }
                    }
                }
            }
        }
        if cfgs.is_empty() { continue; }
        n_parsed += 1;
        if cfgs.len() > 1 {
            n_multi_cfg += 1;
            let deps: Vec<f32> = cfgs.iter().map(|(_, d, _)| *d).collect();
            let eles: Vec<f32> = cfgs.iter().map(|(_, _, e)| *e).collect();
            let (dmin, dmax) = deps.iter().cloned().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
            let (emin, emax) = eles.iter().cloned().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
            if (dmax - dmin).abs() > 0.05 || (emax - emin).abs() > 0.05 {
                n_cfg_diff += 1;
                if diff_examples.len() < 10 {
                    diff_examples.push(format!("{}: {} 配置，dep {:.0}~{:.0}°、ele {:.0}~{:.0}°",
                        tank.name, cfgs.len(), dmin, dmax, emin, emax));
                }
            }
        }
        // gun_angles 对照 models.pb 顶级配置（最后炮塔的最后炮）
        if let Some(gav) = ga.get(&id.to_string()) {
            let gdep = gav.get("gun_depression").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let gele = gav.get("gun_elevation").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let top = tank.turrets.last().and_then(|t| t.guns.last()).and_then(|g| {
                mi.turrets.iter().flat_map(|t| t.guns.iter())
                    .find(|gm| gm.gun_module_id == g.module_id)
                    .and_then(|gm| gm.pitch_limits.as_ref())
                    .map(|pl| (pl.max, -pl.min))
            });
            if let Some((mdep, mele)) = top {
                if (gdep - mdep).abs() > 0.05 || (gele - mele).abs() > 0.05 {
                    if ga_mismatch.len() < 8 {
                        ga_mismatch.push(format!("{}: gun_angles({}, {}) vs models.pb 顶级({:.0},{:.0})",
                            tank.name, gdep, gele, mdep, mele));
                    }
                }
            }
        }
    }
    println!("\n有 pitch_limits 的车: {}", n_parsed);
    println!("多主炮配置的车: {}，其中配置间俯仰【不同】的: {}", n_multi_cfg, n_cfg_diff);
    for e in &diff_examples { println!("  {}", e); }
    println!("存在 front/back 扇区极值差异的炮: {}", n_fb_diff);
    for e in &fb_examples { println!("  {}", e); }
    println!("\ngun_angles.json vs models.pb 顶级配置不一致（前 8）:");
    for e in &ga_mismatch { println!("  {}", e); }
    if ga_mismatch.is_empty() { println!("  （全部一致）"); }
}
