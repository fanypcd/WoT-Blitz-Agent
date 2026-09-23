//! 扇区俯仰数据结构探查：打印带 front/back 的炮的完整 PitchLimitsInfo，
//! 推断 range/transition 的几何含义（扇区宽度/过渡角），并核验扇区覆盖和 = 360°。
#[path = "../src/data.rs"]
pub mod data;
#[path = "../src/wargaming/blitzkit.rs"]
pub mod blitzkit;

fn main() {
    let tanks = blitzkit::load_tanks();
    let mut n_shown = 0;
    println!("=== 带 front/back 的完整数据（前 12 门）===");
    for (id, tank) in tanks.iter() {
        let Some(mi) = blitzkit::model_info(*id) else { continue };
        for tur in &tank.turrets {
            for gun in &tur.guns {
                let Some(pl) = mi.turrets.iter().flat_map(|t| t.guns.iter())
                    .find(|gm| gm.gun_module_id == gun.module_id)
                    .and_then(|gm| gm.pitch_limits.as_ref()) else { continue };
                if pl.front.is_some() || pl.back.is_some() {
                    let f = pl.front.as_ref();
                    let b = pl.back.as_ref();
                    let fs = f.map(|x| format!("{{min{:.1} max{:.1} range{:.1}}}", x.min, x.max, x.range)).unwrap_or("None".into());
                    let bs = b.map(|x| format!("{{min{:.1} max{:.1} range{:.1}}}", x.min, x.max, x.range)).unwrap_or("None".into());
                    let sum = match (f, b, pl.transition) {
                        (Some(f), Some(b), Some(tr)) => format!("覆盖和={:.1}°", f.range + b.range + 2.0 * tr),
                        _ => "transition=None".into(),
                    };
                    println!("{} [{}]: min{:.1} max{:.1} front={} back={} trans={:?} {}",
                        tank.name, gun.name, pl.min, pl.max, fs, bs, pl.transition, sum);
                    n_shown += 1;
                    if n_shown >= 12 { break; }
                }
            }
            if n_shown >= 12 { break; }
        }
        if n_shown >= 12 { break; }
    }
    // 全量统计：front.range+back.range+2*transition 是否恒 = 360
    let (mut n_ok, mut n_bad, mut n_notr) = (0, 0, 0);
    for (_id, tank) in tanks.iter() {
        let Some(mi) = blitzkit::model_info(*_id) else { continue };
        for tur in &tank.turrets {
            for gun in &tur.guns {
                let Some(pl) = mi.turrets.iter().flat_map(|t| t.guns.iter())
                    .find(|gm| gm.gun_module_id == gun.module_id)
                    .and_then(|gm| gm.pitch_limits.as_ref()) else { continue };
                if let (Some(f), Some(b)) = (&pl.front, &pl.back) {
                    match pl.transition {
                        Some(tr) => {
                            let s = f.range + b.range + 2.0 * tr;
                            if (s - 360.0).abs() < 0.5 { n_ok += 1; } else { n_bad += 1; }
                        }
                        None => n_notr += 1,
                    }
                }
            }
        }
    }
    println!("\n覆盖和=360°: {} 门；≠360°: {} 门；无 transition: {} 门", n_ok, n_bad, n_notr);
    // T95E6 原始核查：确认 models.pb 是否真的没有扇区数据
    for tid in [18977u32, 10785, 9057] {
        let tank = blitzkit::tank_full(tid).unwrap();
        let mi = blitzkit::model_info(tid).unwrap();
        for tur in &tank.turrets {
            for gun in &tur.guns {
                let pl = mi.turrets.iter().flat_map(|t| t.guns.iter())
                    .find(|gm| gm.gun_module_id == gun.module_id)
                    .and_then(|gm| gm.pitch_limits.as_ref());
                println!("tank {} [{}] pitch_limits={:?}", tid, gun.name, pl);
            }
        }
    }
}
