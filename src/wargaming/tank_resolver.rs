use std::collections::HashMap;
use std::path::Path;
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};

// =====================================================================
//  坦克数据解析器
//  从本地数据文件（BlitzKit pb 解析结果 + 游戏提取数据）构建
//  723 辆坦克的 ID → 完整信息映射，无需联网、无需 WG API。
// =====================================================================

/// 一辆坦克的完整信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TankInfo {
    pub name: String,
    pub tier: u8,
    #[serde(rename = "type")]
    pub tank_type: String,
    pub nation: String,
    pub is_premium: bool,
    #[serde(default)]
    pub armor: Option<ArmorData>,
    #[serde(default)]
    pub shells: Vec<ShellData>,
    #[serde(default)]
    pub hp: Option<u32>,
    #[serde(default)]
    pub speed_forward: Option<u32>,
    #[serde(default)]
    pub speed_reverse: Option<u32>,
    /// 车体旋转速度（deg/s）
    #[serde(default)]
    pub hull_traverse: Option<f32>,
    /// 视野（m）
    #[serde(default)]
    pub view_range: Option<f32>,
    /// 炮塔旋转速度（deg/s）
    #[serde(default)]
    pub turret_traverse_speed: Option<f32>,
    #[serde(default)]
    pub gun_depression: Option<f32>,
    #[serde(default)]
    pub gun_elevation: Option<f32>,
    #[serde(default)]
    pub turret_traverse_left: Option<f32>,
    #[serde(default)]
    pub turret_traverse_right: Option<f32>,
}

/// 装甲摘要（前/侧/后，单位 mm），用于信息面板显示。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorData {
    pub turret_front: u32,
    pub turret_sides: u32,
    pub turret_rear: u32,
    pub hull_front: u32,
    pub hull_sides: u32,
    pub hull_rear: u32,
}

/// 一种弹（弹药）的数据：类型、穿深、血量伤害、模块伤害。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellData {
    pub shell_type: String,
    pub penetration: u32,
    /// HP（血量）伤害——穿透主装甲盒时对敌人血量造成的伤害
    pub damage: u32,
    /// 模块伤害——仅命中履带/炮管等外部模块时的伤害
    pub module_damage: u32,
    /// HE 爆炸半径（m）；旧缓存缺失时为 0。
    #[serde(default)]
    pub explosion_radius: f64,
}

/// 坦克解析器：缓存 `tank_id → TankInfo`。
#[derive(Debug, Clone)]
pub struct TankResolver {
    cache: HashMap<u32, TankInfo>,
}

impl TankResolver {
    pub fn new() -> Self {
        Self { cache: HashMap::new() }
    }

    /// 按 ID 解析坦克名称。
    pub fn resolve(&self, tank_id: u32) -> Option<String> {
        self.cache.get(&tank_id).map(|info| info.name.clone())
    }

    /// 按 ID 获取完整坦克信息。
    pub fn resolve_info(&self, tank_id: u32) -> Option<&TankInfo> {
        self.cache.get(&tank_id)
    }

    /// 插入一辆坦克的信息。
    pub fn add(&mut self, tank_id: u32, info: TankInfo) {
        self.cache.insert(tank_id, info);
    }

    /// 缓存里的坦克数量。
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// 遍历所有坦克 `(tank_id, TankInfo)`，供模糊搜索/枚举。
    pub fn iter(&self) -> impl Iterator<Item = (u32, &TankInfo)> {
        self.cache.iter().map(|(id, info)| (*id, info))
    }

    /// 从 JSON 文件加载坦克缓存（`tank_cache.json`）。
    pub fn load_from_json_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read tank cache: {}", path.display()))?;
        let cache: HashMap<u32, TankInfo> = serde_json::from_str(&content)?;
        Ok(Self { cache })
    }

    /// 把坦克缓存写为 JSON 文件（`fetch-tanks` 命令用）。
    pub fn save_to_json_file(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.cache)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Build a full tank resolver from local BlitzKit data files (no WG API needed).
    ///
    /// Data sources:
    /// - `tanks.pb`            : tier/type/nation/name/hp/speed/guns (唯一数据源，运行时解析)
    /// - `gun_angles.json`      : gun elevation / depression
    /// - `game_data/{id}.json`  : armor model (primary plates -> front/side/rear thickness)
    /// - `armor_cache.json`     : per-plate thickness fallback for the armor summary
    pub fn from_blitzkit() -> Result<Self> {
        let mut resolver = Self::new();

        // 运行时直接解析 tanks.pb（唯一数据源）：元数据+炮塔/主炮+弹种+装填。
        let tanks = crate::wargaming::blitzkit::load_tanks();
        let gun_angles = std::fs::read_to_string(crate::data::data_path("gun_angles.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

        for (id, tank) in tanks {
            // 名称/国家/类型/等级/血量 均来自 tanks.pb
            let name = tank.name;
            let nation = tank.nation;
            let tank_type = tank.tank_type;
            let tier = tank.tier as u8;
            // 血量 = 车体 health（TankDefinition.health）+ 炮塔 health（TurretDefinition.health）
            // ——取顶级炮塔（turrets.at(-1)，对齐 BlitzKit 默认配置），百科显示的总血量
            let hp = Some(tank.hp + tank.turrets.last().map(|t| t.health).unwrap_or(0));
            let is_premium = tank.is_premium;
            let speed_forward = if tank.speed_forward > 0.0 { Some(tank.speed_forward as u32) } else { None };
            let speed_reverse = if tank.speed_reverse > 0.0 { Some(tank.speed_reverse as u32) } else { None };
            let hull_traverse = Some((tank.hull_traverse * 180.0 / std::f64::consts::PI) as f32);

            // 弹种：取第一个炮塔的第一个主炮的 shells
            let mut shells = Vec::new();
            if let Some(gun) = tank.turrets.first().and_then(|t| t.guns.first()) {
                for s in &gun.shells {
                    shells.push(ShellData {
                        shell_type: s.shell_type.clone(),
                        penetration: s.penetration.round() as u32,
                        damage: s.damage.round() as u32,
                        module_damage: s.module_damage.round() as u32,
                        explosion_radius: s.explosion_radius,
                    });
                }
            }

            // 视野 / 炮塔旋转速度（取自第一个炮塔）
            let first_turret = tank.turrets.first();
            let view_range = first_turret.map(|t| t.view_range as f32);
            let turret_traverse_speed = first_turret.map(|t| t.traverse_speed as f32);

            // 俯仰角：来自 gun_angles.json（tanks.pb 不含）
            let gun_depression = gun_angles.as_ref()
                .and_then(|g| g.get(id.to_string()))
                .and_then(|g| g.get("gun_depression"))
                .and_then(|v| v.as_f64()).map(|v| v as f32);
            let gun_elevation = gun_angles.as_ref()
                .and_then(|g| g.get(id.to_string()))
                .and_then(|g| g.get("gun_elevation"))
                    .and_then(|v| v.as_f64()).map(|v| v as f32);

            let armor = extract_armor_summary(id);

            resolver.add(id, TankInfo {
                name,
                tier,
                tank_type,
                nation,
                is_premium,
                armor,
                shells,
                hp,
                speed_forward,
                speed_reverse,
                hull_traverse,
                view_range,
                turret_traverse_speed,
                gun_depression,
                gun_elevation,
                turret_traverse_left: None,
                turret_traverse_right: None,
            });
        }

        Ok(resolver)
    }

}

impl Default for TankResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// 提取坦克的装甲摘要（前/侧/后，mm）。
///
/// 优先用游戏提取的精确装甲模型（`game_data/{id}.json`，能按 primary 板 ID 定位
/// 前/侧/后各厚度）；缺失时回退到 `armor_cache.json`（取每组最大厚度近似正面值）。
fn extract_armor_summary(tank_id: u32) -> Option<ArmorData> {
    let game_path = crate::data::data_path(&format!("game_data/{}.json", tank_id));
    if let Ok(content) = std::fs::read_to_string(&game_path) {
        let am = serde_json::from_str::<serde_json::Value>(&content).ok()?;
        let armor_model = am.get("armor_model")?;
        if let Some(data) = armor_from_model(armor_model) {
            return Some(data);
        }
    }

    // 回退：从 armor_cache.json 取每组装甲板厚度的最大值近似
    let cache = std::fs::read_to_string(crate::data::data_path("armor_cache.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())?;
    let entry = cache.get(tank_id.to_string())?;
    let p = |key: &str| {
        entry.get(key).and_then(|v| v.as_object()).map(|m| {
            m.values().filter_map(|x| x.as_f64()).fold(0.0f64, |a, b| a.max(b)) as u32
        })
    };
    Some(ArmorData {
        turret_front: p("turret_plates").unwrap_or(0),
        turret_sides: p("turret_plates").unwrap_or(0),
        turret_rear: p("turret_plates").unwrap_or(0),
        hull_front: p("hull_plates").unwrap_or(0),
        hull_sides: p("hull_plates").unwrap_or(0),
        hull_rear: p("hull_plates").unwrap_or(0),
    })
}

/// 从游戏提取的 armor_model 里按 primary 板引用定位各板块厚度。
fn armor_from_model(armor_model: &serde_json::Value) -> Option<ArmorData> {
    // 每个板块独立提取：某块板缺失（如 TD 炮塔只有 armor_1、或无 sides/rear 板）时
    // 用同 section 的最大有效厚度兜底，而不是让 `?` 级联失败回退到"取最大值"的粗糙近似。
    let plate_of = |section: &str, slot: &str| -> Option<u32> {
        let section_val = armor_model.get(section)?;
        let plates = section_val.get("plates")?.as_object()?;
        let primary = section_val.get("primary")?;
        let plate_ref = primary.get(slot).and_then(|v| v.as_str());
        let key = plate_ref.and_then(|r| r.rsplit('_').next()).unwrap_or("1");
        // 优先 primary 指向的板；缺失时用该 section 的最大有效厚度（td/少板坦克的合理近似）
        let v = plates.get(key).and_then(|v| v.as_f64());
        if let Some(v) = v { return Some(v.round() as u32); }
        let maxth = plates.values().filter_map(|v| v.as_f64()).fold(0.0f64, f64::max);
        if maxth > 0.0 { Some(maxth.round() as u32) } else { Some(0) }
    };

    Some(ArmorData {
        turret_front: plate_of("turret", "front")?,
        turret_sides: plate_of("turret", "sides")?,
        turret_rear: plate_of("turret", "rear")?,
        hull_front: plate_of("hull", "front")?,
        hull_sides: plate_of("hull", "sides")?,
        hull_rear: plate_of("hull", "rear")?,
    })
}
