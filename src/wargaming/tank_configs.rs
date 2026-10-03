//! 坦克配置与装甲领域层：
//! - build_configs / resolve_config_index / shell_index_by_global_id：
//!   tanks.pb × models.pb × GLB 节点的炮塔/主炮配置面与实际搭载三级证据链；
//! - synth_armor_model：models.pb 逐板装甲合成；
//! - GLOBAL_RESOLVER：进程级 TankResolver（web/standalone 启动时注入一次，
//!   armor_view 数据面按请求读取）；
//! - model_cache_path / tank_data_value(_prefixed)：资产路径与坦克数据 JSON。

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::wargaming::dvpl::ArmorModel;

/// GLB 缓存目录（data/cache/models；路径解析归领域层，HTTP 伺服在 web::assets）
pub(crate) const GLB_CACHE_DIR: &str = "cache/models";
use crate::wargaming::tank_resolver::TankResolver;

/// 从 models.pb 合成精确装甲模型（逐板厚度/spaced/履带厚度，BlitzKit 唯一来源）。
/// primaryArmor 是 BlitzKit 缺项，从 game_data 同节段拷贝（缺失时留空串——仅影响展示，
/// 装甲摘要链走 game_data 原路径不受影响）；炮塔/主炮取顶级配置（最后炮塔×最后炮），
/// 与 game_data 的 XML 顶级配置语义对齐。
pub(crate) fn synth_armor_model(tank_id: u32) -> Option<ArmorModel> {
    let mi = crate::wargaming::blitzkit::model_info(tank_id)?;
    let game_am = crate::wargaming::game_extract::load_game_data(
        tank_id,
        &crate::data::data_dir().join("game_data"),
    )
    .and_then(|gd| gd.armor_model);
    let section = |plates: &std::collections::BTreeMap<u32, f32>,
                   spaced: &[u32],
                   primary: Option<&crate::wargaming::dvpl::PrimaryArmor>| {
        crate::wargaming::dvpl::SectionArmor {
            plates: plates.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            primary: primary
                .cloned()
                .unwrap_or(crate::wargaming::dvpl::PrimaryArmor {
                    front: String::new(),
                    sides: String::new(),
                    rear: String::new(),
                }),
            spaced: spaced.iter().map(|s| s.to_string()).collect(),
        }
    };
    let top_module = crate::wargaming::blitzkit::tank_full(tank_id)
        .and_then(|t| t.turrets.last().map(|t2| t2.module_id));
    let top_turret = mi
        .turrets
        .iter()
        .find(|t| Some(t.module_id) == top_module)
        .or_else(|| mi.turrets.last());
    Some(ArmorModel {
        hull: section(
            &mi.hull_plates,
            &mi.hull_spaced,
            game_am.as_ref().map(|am| &am.hull.primary),
        ),
        turret: top_turret.map(|t| {
            section(
                &t.turret_plates,
                &t.turret_spaced,
                game_am
                    .as_ref()
                    .and_then(|am| am.turret.as_ref())
                    .map(|s| &s.primary),
            )
        }),
        gun: top_turret.and_then(|t| t.guns.last()).map(|g| {
            section(
                &g.gun_plates,
                &g.gun_spaced,
                game_am
                    .as_ref()
                    .and_then(|am| am.gun.as_ref())
                    .map(|s| &s.primary),
            )
        }),
        chassis: mi
            .track_thickness
            .map(|t| crate::wargaming::dvpl::ChassisArmor {
                left_track: t,
                right_track: t,
            }),
    })
}

static GLOBAL_RESOLVER: std::sync::OnceLock<Arc<TankResolver>> = std::sync::OnceLock::new();

pub fn global_resolver() -> Arc<TankResolver> {
    GLOBAL_RESOLVER
        .get_or_init(|| {
            TankResolver::load_from_json_file(&crate::data::data_path("tank_cache.json"))
                .map(Arc::new)
                .unwrap_or_default()
        })
        .clone()
}

/// 导出全部 per-tank JSON（资产打包用）：初始化全局 resolver 后逐车物化。
/// 返回导出份数。CLI `dump-tank-data` 与打包器消费。
pub fn export_tank_data(out: &std::path::Path) -> anyhow::Result<usize> {
    let resolver = crate::wargaming::tank_resolver::TankResolver::load_from_json_file(
        crate::data::data_path("tank_cache.json").as_path(),
    )
    .unwrap_or_default();
    set_global_resolver(resolver);
    std::fs::create_dir_all(out)?;
    let cache: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(crate::data::data_path("tank_cache.json"))?,
    )?;
    let total = cache.len();
    for (i, (tid, _)) in cache.iter().enumerate() {
        let Ok(id) = tid.parse::<u32>() else {
            eprintln!("  跳过非数字键 {tid}");
            continue;
        };
        let value = tank_data_value(id);
        std::fs::write(
            out.join(format!("{id}.json")),
            serde_json::to_vec_pretty(&value)?,
        )?;
        if (i + 1) % 100 == 0 {
            eprintln!("  {}/{total}", i + 1);
        }
    }
    Ok(total)
}

pub(crate) fn set_global_resolver(resolver: TankResolver) {
    let _ = GLOBAL_RESOLVER.set(Arc::new(resolver));
}

/// 单个 GLB 的缓存路径（data/cache/models/{tank_id}/{filename}），按需服务与 fetch-models 全量预热共用。
pub(crate) fn model_cache_path(tank_id: u32, filename: &str) -> std::path::PathBuf {
    crate::data::data_path(GLB_CACHE_DIR)
        .join(tank_id.to_string())
        .join(filename)
}

pub(crate) fn tank_data_value(tank_id: u32) -> Value {
    tank_data_value_prefixed(tank_id, "")
}

pub(crate) fn tank_data_value_prefixed(tank_id: u32, base_prefix: &str) -> Value {
    let resolver = global_resolver();

    let info = resolver.resolve_info(tank_id);

    // C 类数据：炮管/底盘碰撞盒仍取本机客户端提取（BlitzKit 无对应数据）
    let game_data = crate::wargaming::game_extract::load_game_data(
        tank_id,
        &crate::data::data_dir().join("game_data"),
    );
    // 逐板装甲：BlitzKit models.pb 唯一来源（primary 为 BlitzKit 缺项，合成时从 game_data 拷贝）
    let armor_model = synth_armor_model(tank_id);

    let name = resolver
        .resolve(tank_id)
        .unwrap_or_else(|| format!("tank_{}", tank_id));
    let tier = info
        .as_ref()
        .map(|i| i.tier as u32)
        .filter(|t| *t > 0)
        .unwrap_or(0);
    let tank_type = info
        .as_ref()
        .map(|i| i.tank_type.clone())
        .filter(|t| !t.is_empty() && t != "unknown")
        .unwrap_or_else(|| "unknown".to_string());
    let nation = info
        .as_ref()
        .map(|i| i.nation.clone())
        .filter(|n| !n.is_empty() && n != "unknown")
        .unwrap_or_else(|| "unknown".to_string());

    let armor = info.as_ref().and_then(|i| i.armor.as_ref()).map(|a| {
        json!({
            "turret": {"front": a.turret_front, "sides": a.turret_sides, "rear": a.turret_rear},
            "hull": {"front": a.hull_front, "sides": a.hull_sides, "rear": a.hull_rear},
        })
    });

    let shells = info
        .as_ref()
        .map(|i| {
            i.shells
                .iter()
                .map(|s| {
                    json!({
                        "type": s.shell_type,
                        "penetration": s.penetration,
                        "damage": s.damage,
                        "module_damage": s.module_damage,
                        "explosion_radius": s.explosion_radius,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let armor_model_val = armor_model.as_ref().map(|m| {
        let mut v = serde_json::to_value(m).unwrap_or(json!(null));
        // f32 序列化尾差清洗：62.400001525878906 → 62.4（装甲厚度精度 0.1mm 已足够；
        // 只清洗 plates/track 厚度，hull_position 等坐标保持原精度）
        if let Some(obj) = v.as_object_mut() {
            for sec in ["hull", "turret", "gun"] {
                if let Some(plates) = obj
                    .get_mut(sec)
                    .and_then(|s| s.get_mut("plates"))
                    .and_then(|p| p.as_object_mut())
                {
                    for (_, n) in plates.iter_mut() {
                        if let Some(f) = n.as_f64() {
                            *n = json!((f * 10.0).round() / 10.0);
                        }
                    }
                }
            }
            if let Some(ch) = obj.get_mut("chassis").and_then(|c| c.as_object_mut()) {
                for k in ["left_track", "right_track"] {
                    if let Some(n) = ch.get_mut(k) {
                        if let Some(f) = n.as_f64() {
                            *n = json!((f * 10.0).round() / 10.0);
                        }
                    }
                }
            }
        }
        v
    });

    let configs = build_configs(tank_id);
    let caliber = configs
        .first()
        .and_then(|c| c.get("caliber"))
        .and_then(|v| v.as_u64())
        .unwrap_or(120) as u32;
    let m_info = crate::wargaming::blitzkit::model_info(tank_id);
    let hull_spaced = m_info
        .as_ref()
        .map(|m| m.hull_spaced.clone())
        .unwrap_or_default();
    // 模型原点（models.pb，DAVA→GLB correctZY: (x,z,y)）——装甲节点定位基准，
    // 对齐 BlitzKit SpacedArmorScene 的 hullOrigin/turretOrigin 分组装配。
    let model_origins = m_info
        .as_ref()
        .and_then(|m| match (m.track_origin, m.turret_origin) {
            (Some(tk), Some(tu)) => Some(json!({
                "track": [tk[0], tk[2], tk[1]],
                "turret": [tu[0], tu[2], tu[1]],
            })),
            _ => None,
        });
    let initial_turret_rotation = m_info
        .as_ref()
        .and_then(|m| m.initial_turret_rotation.clone());
    // 炮管碰撞盒（game_data/{id}.json 的 collision.gun_bbox，564/723 车有数据）。
    // 坐标系与 GLB 内部一致（x=右 y=前 z=上），原点=炮管节点（枢轴）——
    // min[1]（后伸量）= 炮闩位置的文件标定。缺失时前端回退到枢轴本身。
    let gun_collision = game_data
        .as_ref()
        .and_then(|gd| gd.collision.as_ref())
        .and_then(|c| c.gun_bbox.clone())
        .map(|b| json!({ "min": b.min, "max": b.max }));
    // 各部件原生碰撞盒（坐标系 = 各部件节点枢轴系 x右/y前/z上；chassis/hull 位于模型原点）。
    // 供 DecodeShotSegment 解码盒使用。hull/turret：models.pb（BlitzKit 唯一来源，炮塔取
    // 顶级配置）；gun/chassis：BlitzKit 无对应数据，仍取本机客户端提取（game_data）。
    let top_module = crate::wargaming::blitzkit::tank_full(tank_id)
        .and_then(|t| t.turrets.last().map(|t2| t2.module_id));
    let hull_bbox = m_info.as_ref().and_then(|mi| mi.hull_bbox.clone());
    let turret_bbox = m_info.as_ref().and_then(|mi| {
        mi.turrets
            .iter()
            .find(|t| Some(t.module_id) == top_module)
            .or_else(|| mi.turrets.last())
            .and_then(|t| t.bbox.clone())
    });
    let gd_collision = game_data.as_ref().and_then(|gd| gd.collision.as_ref());
    let bbox_json = |b: Option<crate::wargaming::dvpl::BoundingBox>| {
        b.map(|b| json!({ "min": b.min, "max": b.max }))
    };
    let collision_boxes = json!({
        "chassis": bbox_json(gd_collision.and_then(|c| c.chassis_bbox.clone())),
        "hull": bbox_json(hull_bbox),
        "turret": bbox_json(turret_bbox),
        "gun": bbox_json(gd_collision.and_then(|c| c.gun_bbox.clone())),
    });

    json!({
        "tank_id": tank_id,
        "name": name,
        "tier": tier,
        "type": tank_type,
        "nation": nation,
        "model_url": format!("{}/glb/{}/collision.glb", base_prefix, tank_id),
        "visual_model_url": format!("{}/glb/{}/model.glb", base_prefix, tank_id),
        "armor": armor,
        "hull_spaced": hull_spaced,
        "model_origins": model_origins,
        "initial_turret_rotation": initial_turret_rotation,
        "gun_collision": gun_collision,
        "collision_boxes": collision_boxes,
        "armor_model": armor_model_val,
        "caliber": caliber,
        "shells": shells,
        "configs": configs.as_ref(),
        "hp": info.as_ref().and_then(|i| i.hp),
        "speed": info.as_ref().and_then(|i| i.speed_forward),
        // 顶层显式前后极速（形状对齐 /api/tank_detail）
        "speed_forward": info.as_ref().and_then(|i| i.speed_forward),
        "speed_reverse": info.as_ref().and_then(|i| i.speed_reverse),
        "gun_depression": info.as_ref().and_then(|i| i.gun_depression).map(|v| v as f64),
        "gun_elevation": info.as_ref().and_then(|i| i.gun_elevation).map(|v| v as f64),
        "turret_traverse_left": info.as_ref().and_then(|i| i.turret_traverse_left).map(|v| v as f64),
        "turret_traverse_right": info.as_ref().and_then(|i| i.turret_traverse_right).map(|v| v as f64),
    })
}

fn model_config_nodes(tank_id: u32) -> (Vec<String>, Vec<String>) {
    let mut guns = Vec::new();
    let mut turrets = Vec::new();
    let path = crate::data::data_path(GLB_CACHE_DIR)
        .join(tank_id.to_string())
        .join("model.glb");
    if let Ok(bytes) = std::fs::read(&path) {
        if let Some(names) = parse_glb_top_nodes(&bytes) {
            for nm in names {
                if let Some(g) = nm.strip_prefix("gun_") {
                    if g.chars().all(|c| c.is_ascii_digit()) {
                        guns.push(nm);
                    }
                } else if let Some(t) = nm.strip_prefix("turret_") {
                    if t.chars().all(|c| c.is_ascii_digit()) {
                        turrets.push(nm);
                    }
                }
            }
            guns.reverse();
            turrets.reverse();
        }
    }
    (guns, turrets)
}

fn parse_glb_top_nodes(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.len() < 20 {
        return None;
    }
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    if 20 + json_len > bytes.len() {
        return None;
    }
    let js: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).ok()?;
    let nodes = js.get("nodes")?.as_array()?;
    let scene = js
        .get("scenes")?
        .as_array()?
        .first()?
        .get("nodes")?
        .as_array()?;
    let root_idx = scene.first()?;
    let root = nodes.get(root_idx.as_u64()? as usize)?;
    let children = root.get("children").and_then(|c| c.as_array())?;
    Some(
        children
            .iter()
            .filter_map(|c| {
                nodes
                    .get(c.as_u64()? as usize)?
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            })
            .collect(),
    )
}

/// build_configs 进程级缓存（tank_id → 配置表）。配置是 tanks.pb + models.pb + GLB
/// 的纯函数，进程内不变；一场回放的富化循环会逐玩家/逐发调用，必须缓存复用。
static CONFIGS_CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<u32, Arc<Vec<Value>>>>> =
    std::sync::OnceLock::new();

pub(crate) fn build_configs(tank_id: u32) -> Arc<Vec<Value>> {
    let cache = CONFIGS_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(c) = guard.get(&tank_id) {
            return Arc::clone(c);
        }
    }
    let configs = Arc::new(build_configs_uncached(tank_id));
    // 仅在 model.glb 已就位时入缓存：未下载的车型保持逐次重建，下载完成后
    // 下一次调用自然构建完整配置（含 gun/turret 模型节点映射）
    let glb_ready = crate::data::data_path(GLB_CACHE_DIR)
        .join(tank_id.to_string())
        .join("model.glb")
        .exists();
    if glb_ready {
        if let Ok(mut guard) = cache.lock() {
            guard.insert(tank_id, Arc::clone(&configs));
        }
    }
    configs
}

fn build_configs_uncached(tank_id: u32) -> Vec<Value> {
    let Some(tank) = crate::wargaming::blitzkit::tank_full(tank_id) else {
        return Vec::new();
    };

    let (model_guns, model_turrets) = model_config_nodes(tank_id);

    let mut gun_nums: Vec<u32> = model_guns
        .iter()
        .filter_map(|g| g.strip_prefix("gun_")?.parse().ok())
        .collect();
    gun_nums.sort();
    gun_nums.dedup();
    let gun_dense: std::collections::HashMap<u32, u32> = gun_nums
        .iter()
        .enumerate()
        .map(|(i, n)| (*n, i as u32))
        .collect();
    let mut turret_nums: Vec<u32> = model_turrets
        .iter()
        .filter_map(|g| g.strip_prefix("turret_")?.parse().ok())
        .collect();
    turret_nums.sort();
    turret_nums.dedup();
    let turret_dense: std::collections::HashMap<u32, u32> = turret_nums
        .iter()
        .enumerate()
        .map(|(i, n)| (*n, i as u32))
        .collect();

    let tmod_info = crate::wargaming::blitzkit::model_info(tank_id);
    // 炮塔模型信息按 module_id 建索引，避免循环内对 tmod_info.turrets 反复线性查找
    let tmod_by_module: std::collections::HashMap<
        u32,
        &crate::wargaming::blitzkit::TurretModelInfo,
    > = tmod_info
        .as_ref()
        .map(|mi| mi.turrets.iter().map(|t| (t.module_id, t)).collect())
        .unwrap_or_default();

    let mut gun_idx_by_module: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::new();
    let mut distinct_gun_modules = Vec::new();
    for tur in &tank.turrets {
        for gun in &tur.guns {
            if let std::collections::hash_map::Entry::Vacant(e) =
                gun_idx_by_module.entry(gun.module_id)
            {
                let idx = distinct_gun_modules.len() as u32;
                e.insert(idx);
                distinct_gun_modules.push(gun.module_id);
            }
        }
    }

    let turret_model_node = |ti: usize, tmod: u32| -> Option<u32> {
        tmod_by_module
            .get(&tmod)
            .map(|t| t.model_node)
            .or_else(|| Some((ti as u32) + 1))
    };
    // (model_node, gun_thickness, gun_mask, gun_spaced)
    type GunModelInfo = (u32, Option<f32>, Option<f32>, Vec<u32>);
    let gun_model_info = |tmod: u32, gmod: u32| -> Option<GunModelInfo> {
        tmod_by_module
            .get(&tmod)
            .and_then(|t| t.guns.iter().find(|g| g.gun_module_id == gmod))
            .map(|g| (g.model_node, g.thickness, g.mask, g.gun_spaced.clone()))
    };

    let mut configs = Vec::new();
    let mut count = 0u32;
    for (ti, tur) in tank.turrets.iter().enumerate() {
        let turret_def = tmod_by_module.get(&tur.module_id);
        let turret_name = tur.name.clone();
        let turret_weight = Some(tur.weight);
        let turret_traverse = Some(tur.traverse_speed);
        let view_range = Some(tur.view_range);
        let turret_index = turret_model_node(ti, tur.module_id)
            .and_then(|n| turret_dense.get(&n).copied())
            .unwrap_or(ti as u32);
        for gun in &tur.guns {
            let (gun_node, gun_thickness, gun_mask, gun_spaced) = gun_model_info(
                tur.module_id,
                gun.module_id,
            )
            .unwrap_or((u32::MAX, None, None, Vec::new()));
            let gun_index = if gun_node != u32::MAX {
                gun_dense.get(&gun_node).copied()
            } else {
                None
            }
            .unwrap_or_else(|| *gun_idx_by_module.get(&gun.module_id).unwrap_or(&0));
            let turret_spaced = turret_def
                .map(|t| t.turret_spaced.clone())
                .unwrap_or_default();
            // 火炮原点（models.pb TurretModelDefinition.gun_origin，DAVA→GLB correctZY: (x,z,y)）
            let gun_origin = turret_def
                .and_then(|t| t.gun_origin)
                .map(|d| [d[0], d[2], d[1]]);
            let yaw_limits = turret_def.and_then(|t| t.yaw_limits.clone());
            let pitch_limits = turret_def
                .and_then(|t| t.guns.iter().find(|g| g.gun_module_id == gun.module_id))
                .and_then(|g| g.pitch_limits.clone());
            let name = if gun.name.is_empty() {
                format!("gun_{}", gun_index)
            } else {
                gun.name.clone()
            };
            let caliber = parse_gun_caliber(&name)
                .map(|c| c.round() as u32)
                .unwrap_or(120);
            let aim_time = Some(gun.aim_time);
            let dispersion = Some(gun.dispersion);
            let gr = &gun.reload;
            let reload_time = if gr.reload > 0.0 {
                Some(gr.reload)
            } else {
                None
            };
            let is_burst = gr.is_burst;
            let burst_size = gr.burst_size;
            let burst_interval = gr.burst_interval;
            let burst_reloads = gr.burst_reloads.clone();
            let is_drum = gr.is_drum;
            let shells: Vec<Value> = gun
                .shells
                .iter()
                .map(|s| {
                    json!({
                        "type": s.shell_type,
                        "penetration": s.penetration,
                        "penetration_far": s.penetration_far,
                        "damage": s.damage,
                        "module_damage": s.module_damage,
                        "explosion_radius": s.explosion_radius,
                        "velocity": s.velocity,
                        "range": s.range,
                        "caliber": s.caliber,
                        "normalization": s.normalization,
                        "ricochet": s.ricochet,
                    })
                })
                .collect();
            // 标准弹药单发伤害：优先 AP，其次任意非金币弹（金币变体 shell_type 含 premium 不计）；
            // DPM 与 Alpha 单发均基于此值
            let is_premium_shell = |t: &str| t.contains("premium");
            let standard_damage = gun
                .shells
                .iter()
                .find(|s| s.shell_type == "ap")
                .or_else(|| gun.shells.iter().find(|s| !is_premium_shell(&s.shell_type)))
                .map(|s| s.damage)
                .or_else(|| {
                    let m = gun
                        .shells
                        .iter()
                        .map(|s| s.damage)
                        .fold(f64::NEG_INFINITY, f64::max);
                    if m.is_finite() {
                        Some(m)
                    } else {
                        None
                    }
                })
                .unwrap_or(0.0);
            let dpm = if is_burst {
                None
            } else {
                reload_time
                    .filter(|r| *r > 0.0)
                    .map(|r| (standard_damage * 60.0 / r).round())
            };

            configs.push(json!({
                "id": count,
                "label": name,
                "caliber": caliber,
                "shells": shells,
                "turret_index": turret_index,
                "gun_index": gun_index,
                // 模块局部 id（module_id>>8）——与 updateArena ARENA_INFO comp blob 对号，
                // 实际搭载配置解析（resolve_config_index）用
                "turret_local": (tur.module_id >> 8) as u16,
                "gun_local": (gun.module_id >> 8) as u16,
                "shell_global_ids": gun.shells.iter()
                    .filter(|s| s.id > 0)
                    .filter_map(|s| crate::replay::loadout::blitzkit_shell_global_id(&tank.nation, (s.id >> 8) as u64))
                    .collect::<Vec<u32>>(),
                "gun_thickness": gun_thickness,
                "gun_mask": gun_mask,
                "gun_spaced": gun_spaced,
                "turret_spaced": turret_spaced,
                // 火炮原点（correctZY 后的 GLB 坐标，装甲定位用，对齐 BlitzKit SpacedArmorScene）
                "gun_origin": gun_origin,
                "yaw_limits": yaw_limits,
                "pitch_limits": pitch_limits,
                "turret_name": turret_name,
                "reload_time": reload_time,
                "aim_time": aim_time,
                "dispersion": dispersion,
                "dpm": dpm,
                "standard_damage": standard_damage,
                "is_burst": is_burst,
                "is_drum": is_drum,
                "burst_size": burst_size,
                "burst_interval": burst_interval,
                "burst_reloads": burst_reloads,
                "turret_weight": turret_weight,
                "turret_traverse_speed": turret_traverse,
                "view_range": view_range,
                // 血量细分：总 HP = 车体 hp + 炮塔 health（对齐 TankResolver 口径）
                "hull_hp": tank.hp,
                "turret_health": tur.health,
                "model_gun_count": model_guns.len(),
                "model_turret_count": model_turrets.len(),
                "engines": tank.engines.clone(),
                "tracks": tank.tracks.clone(),
                "weight": tank.weight,
            }));
            count += 1;
        }
    }
    if configs.is_empty() {
        configs.push(json!({
            "id": 0, "label": "Default", "caliber": 120, "shells": [],
            "turret_index": 0, "gun_index": 0, "turret_name": "",
        }));
    }
    configs
}

/// 发射弹种 → 射手弹表下标（确定性弹种选择）：
/// 按 shell_global_ids（= tanks.pb 弹种全局 id）匹配 build_configs 各配置的弹表，
/// 返回首个包含该弹的配置中弹的下标。type=28 槽位快照存在切弹竞态，
/// shell_id 才是发射弹种的权威标识。
pub fn shell_index_by_global_id(tank_id: u32, shell_id: u32) -> Option<usize> {
    resolve_shell_by_global_id(tank_id, shell_id, None).map(|(_, si, _)| si)
}

/// 发射弹种解析（配置内下标 + 完整弹数据）：shell_id → (配置下标, 配置内弹下标, 弹数据)。
/// 优先实际搭载配置（cfg_hint = shooter_config_idx；多炮坦克各炮弹表不同，hint 域
/// 必须钉死）；hint 未命中（数据不全/未解析）再全配置扫描（从后往前 = 顶级偏好）。
/// 弹数据取自匹配配置的 shells 数组（与 shell_global_ids 同源同序）。
pub fn resolve_shell_by_global_id(
    tank_id: u32,
    shell_id: u32,
    cfg_hint: Option<usize>,
) -> Option<(usize, usize, Value)> {
    if shell_id == 0 {
        return None;
    }
    let configs = build_configs(tank_id);
    let pos_in = |c: &Value| -> Option<(usize, Value)> {
        let gids = c.get("shell_global_ids")?.as_array()?;
        let shells = c.get("shells")?.as_array()?;
        let si = gids
            .iter()
            .position(|g| g.as_u64() == Some(shell_id as u64))?;
        Some((si, shells.get(si)?.clone()))
    };
    if let Some(ci) = cfg_hint {
        if let Some(c) = configs.get(ci) {
            if let Some((si, sh)) = pos_in(c) {
                return Some((ci, si, sh));
            }
        }
    }
    configs
        .iter()
        .enumerate()
        .rev()
        .find_map(|(ci, c)| pos_in(c).map(|(si, sh)| (ci, si, sh)))
}

/// 实际搭载配置解析（共享证据链，射击复现与实时回放同步使用）：
/// 证据 0 = comp blob 局部 id（updateArena ARENA_INFO，确定性：炮塔/主炮 module_id>>8 直接对号）；
/// 证据 1 = 发射弹种 ⊆ 配置弹表（shell_global_ids）；
/// 证据 2 = 初始血量 vs 车体+炮塔 health（改进耐久 ×1.125，±2 容差）；
/// 依次回退，多匹配取最后一档（顶级），全无 → None（调用方默认顶级）。
/// 返回 = (build_configs 数组下标, turret_index, gun_index)。
pub fn resolve_config_index(
    tank_id: u32,
    comp: Option<(u16, u16)>,
    shell_ids: &[u32],
    hp: u16,
) -> Option<(usize, u32, u32)> {
    let configs = build_configs(tank_id);
    if configs.len() <= 1 {
        return None;
    }
    // 证据 0：comp blob 确定性对号
    if let Some((cl, gl)) = comp {
        let exact: Vec<usize> = (0..configs.len())
            .filter(|&i| {
                configs[i]["turret_local"].as_u64() == Some(cl as u64)
                    && configs[i]["gun_local"].as_u64() == Some(gl as u64)
            })
            .collect();
        if !exact.is_empty() {
            let i = *exact.last().unwrap();
            return Some((
                i,
                configs[i]["turret_index"].as_u64().unwrap_or(0) as u32,
                configs[i]["gun_index"].as_u64().unwrap_or(0) as u32,
            ));
        }
    }
    let fired: std::collections::HashSet<u32> = shell_ids.iter().copied().collect();
    let gun_ok: Vec<bool> = configs
        .iter()
        .map(|c| {
            fired.is_empty() || {
                match c["shell_global_ids"].as_array() {
                    Some(a) if !a.is_empty() => fired
                        .iter()
                        .all(|id| a.iter().any(|s| s.as_u64() == Some(*id as u64))),
                    _ => true, // 弹表缺失（数据不全）→ 不以此排除
                }
            }
        })
        .collect();
    let hp_val = hp as u32;
    let hp_ok: Vec<bool> = configs
        .iter()
        .map(|c| {
            if hp_val == 0 {
                return true;
            }
            let base = c["hull_hp"].as_u64().unwrap_or(0) as u32
                + c["turret_health"].as_u64().unwrap_or(0) as u32;
            if base == 0 {
                return true;
            }
            let boosted = ((base as f64) * 1.125).round() as u32;
            hp_val.abs_diff(base) <= 2 || hp_val.abs_diff(boosted) <= 2
        })
        .collect();
    let both: Vec<usize> = (0..configs.len())
        .filter(|&i| gun_ok[i] && hp_ok[i])
        .collect();
    let mut cands = both;
    if cands.is_empty() {
        cands = (0..configs.len()).filter(|&i| gun_ok[i]).collect();
    }
    if cands.is_empty() {
        cands = (0..configs.len()).filter(|&i| hp_ok[i]).collect();
    }
    let i = *cands.last()?;
    Some((
        i,
        configs[i]["turret_index"].as_u64().unwrap_or(0) as u32,
        configs[i]["gun_index"].as_u64().unwrap_or(0) as u32,
    ))
}

pub(crate) fn parse_gun_caliber(name: &str) -> Option<f64> {
    let lower = name.to_lowercase();
    let (unit_mul, idx) = if let Some(i) = lower.find(" mm") {
        (1.0, i)
    } else {
        let i = lower.find(" cm")?;
        (10.0, i)
    };
    let bytes = lower.as_bytes();
    let mut start = idx;
    while start > 0 {
        let c = bytes[start - 1];
        if c.is_ascii_digit() || c == b'.' || c == b',' {
            start -= 1;
        } else {
            break;
        }
    }
    let num = &lower[start..idx].replace(',', ".");
    num.parse::<f64>().ok().map(|v| v * unit_mul)
}

#[cfg(test)]
mod synth_tests {
    use super::*;

    /// IS-7 合成锚点：逐板厚度/履带来自 models.pb（BlitzKit 唯一来源），
    /// primaryArmor 来自 game_data 拷贝；键格式（数值字符串）与前端 plateId 查找兼容。
    #[test]
    fn synth_armor_model_migrates_to_blitzkit() {
        let am = synth_armor_model(7169).expect("IS-7 synth");
        // 车体逐板厚度：models.pb（0 值板省略）
        assert_eq!(am.hull.plates.get("1"), Some(&150.0));
        assert_eq!(am.hull.plates.get("5"), Some(&270.0));
        assert!(!am.hull.plates.contains_key("8"), "0 值板省略");
        assert_eq!(
            am.hull
                .spaced
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["9"]
        );
        // primary：game_data 同节段拷贝
        assert_eq!(am.hull.primary.front, "armor_1");
        // 炮塔/主炮（顶级配置）
        let t = am.turret.as_ref().expect("turret");
        assert_eq!(t.plates.get("2"), Some(&210.0));
        assert_eq!(t.primary.front, "armor_1");
        let g = am.gun.as_ref().expect("gun");
        assert_eq!(g.plates.get("1"), Some(&350.0));
        // 履带厚度：models.pb track
        let ch = am.chassis.as_ref().expect("chassis");
        assert_eq!(ch.left_track, 20.0);
        assert_eq!(ch.right_track, 20.0);
    }
}
