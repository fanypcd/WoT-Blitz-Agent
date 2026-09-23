use std::collections::HashMap;
use std::path::Path;
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};

// =====================================================================
//  坦克数据解析器：从本地数据文件（BlitzKit pb 解析结果 + 游戏提取数据）
//  构建 ID → 完整信息映射，无需联网、无需 WG API。
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

/// 归一化名称索引项：坦克名预归一结果，避免每次模糊查询对全部坦克重新归一（逐辆 2 次 String 分配）。
#[derive(Debug, Clone)]
pub(crate) struct NameIndexEntry {
    /// 归一名（'-'/'·'/'.'/'_' → 空格 + 小写）
    pub(crate) norm: String,
    /// 归一名去空白形态（"e100"↔"E 100"）
    pub(crate) norm_ns: String,
    pub(crate) id: u32,
}

#[derive(Debug, Clone)]
pub struct TankResolver {
    cache: HashMap<u32, TankInfo>,
    /// 与 cache 同步维护的归一化名称索引（模糊匹配用）
    name_index: Vec<NameIndexEntry>,
}

/// 名称归一：'-'/'·'/'.'/'_' 全部视为空格并转小写，使 "E 100" 与 "E-100"、"IS-7" 与 "IS 7" 等可互相命中。
pub(crate) fn norm_name(s: &str) -> String {
    s.chars().map(|c| if c=='-'||c=='·'||c=='.'||c=='_' {' '} else {c}).collect::<String>().to_lowercase()
}

/// 去空格形态：归一后再剥掉全部空白——"hori"↔"Ho-Ri"、"e100"↔"E 100"。
/// 否则 "hori" 无法命中 "ho ri"（工具返回查不到 → LLM 用目标车数据幻觉补全）。
pub(crate) fn strip_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

impl TankResolver {
    pub fn new() -> Self {
        Self { cache: HashMap::new(), name_index: Vec::new() }
    }

    pub fn resolve(&self, tank_id: u32) -> Option<String> {
        self.cache.get(&tank_id).map(|info| info.name.clone())
    }

    pub fn resolve_info(&self, tank_id: u32) -> Option<&TankInfo> {
        self.cache.get(&tank_id)
    }

    /// 昵称 → 炮管俯仰限制锚定表：prop2 frac 解码用（combat::decode_prop2_gun_pitch，
    /// 扇区化——俯仰范围随炮塔朝向 front/back 分段，T95E6 旋转实验定案）。
    /// 数据源（优先级）：models.pb 顶级配置（最后炮塔×最后炮）的模块级
    /// GunModelDefinition.pitch（含扇区）> 本表 TankInfo 的 gun_angles 静态回退（无扇区）。
    /// 缺两者的玩家不入选（其俯仰走提取链回退路径并打质量标记）。注意匿名玩家共用
    /// 显示名 "Anonyme"，同场多个匿名玩家会互相覆盖（按昵称连接的固有歧义）；取顶级
    /// 配置（多配置车辆的模块级差异未区分，见"俯仰锚定粒度"审计）。
    pub fn pitch_limits_from_battle_results(
        &self,
        br: &wotbreplay_parser::models::battle_results::BattleResults,
    ) -> HashMap<String, crate::replay::combat::GunPitchRange> {
        let mut m: HashMap<String, crate::replay::combat::GunPitchRange> = HashMap::new();
        for p in &br.players {
            let tank_id = br.player_results.iter()
                .find(|pr| pr.info.account_id == p.account_id)
                .map(|pr| pr.info.tank_id);
            let Some(tid) = tank_id else { continue };
            // models.pb 顶级配置（含扇区）
            let from_models = crate::wargaming::blitzkit::tank_full(tid).and_then(|tank| {
                let top_gun_module = tank.turrets.last().and_then(|t| t.guns.last())?.module_id;
                let mi = crate::wargaming::blitzkit::model_info(tid)?;
                let pl = mi.turrets.iter().flat_map(|t| t.guns.iter())
                    .find(|gm| gm.gun_module_id == top_gun_module)
                    .and_then(|gm| gm.pitch_limits.clone())?;
                Some(crate::replay::combat::GunPitchRange {
                    dep: pl.max,
                    ele: -pl.min,
                    front: pl.front.map(|f| crate::replay::combat::SectorLimits { min: f.min, max: f.max, range: f.range }),
                    back: pl.back.map(|b| crate::replay::combat::SectorLimits { min: b.min, max: b.max, range: b.range }),
                    transition: pl.transition,
                })
            });
            if let Some(r) = from_models {
                m.insert(p.info.nickname.clone(), r);
                continue;
            }
            // 静态回退：gun_angles.json（无扇区）
            if let Some(info) = self.resolve_info(tid) {
                if let (Some(dep), Some(ele)) = (info.gun_depression, info.gun_elevation) {
                    m.insert(p.info.nickname.clone(), crate::replay::combat::GunPitchRange {
                        dep, ele, front: None, back: None, transition: None,
                    });
                }
            }
        }
        m
    }

    pub fn add(&mut self, tank_id: u32, info: TankInfo) {
        let norm = norm_name(&info.name);
        self.name_index.push(NameIndexEntry {
            norm_ns: strip_ws(&norm),
            norm,
            id: tank_id,
        });
        self.cache.insert(tank_id, info);
    }

    /// 归一化名称索引（模糊匹配用，与 cache 同步维护）。
    pub(crate) fn name_index(&self) -> &[NameIndexEntry] {
        &self.name_index
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// 从 JSON 文件加载坦克缓存（`tank_cache.json`）。
    pub fn load_from_json_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read tank cache: {}", path.display()))?;
        let cache: HashMap<u32, TankInfo> = serde_json::from_str(&content)?;
        // 预计算归一化名称索引（按 cache 迭代序构建，与直接遍历 cache 的顺序一致）
        let name_index = cache.iter().map(|(id, info)| {
            let norm = norm_name(&info.name);
            NameIndexEntry { norm_ns: strip_ws(&norm), norm, id: *id }
        }).collect();
        Ok(Self { cache, name_index })
    }

    /// 把坦克缓存写为 JSON 文件（`fetch-tanks` 命令用）。
    pub fn save_to_json_file(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.cache)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// 从本地 BlitzKit 数据文件构建完整解析器（无需 WG API）。
    /// 数据源：tanks.pb（唯一数据源，运行时解析）、gun_angles.json（俯仰角）、
    /// game_data/{id}.json（装甲模型）、armor_cache.json（装甲摘要兜底）。
    pub fn from_blitzkit() -> Result<Self> {
        let mut resolver = Self::new();

        // load_tanks 返回进程内共享缓存引用（零克隆），此处只读
        let tanks = crate::wargaming::blitzkit::load_tanks();
        let gun_angles = std::fs::read_to_string(crate::data::data_path("gun_angles.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

        for (id, tank) in tanks.iter() {
            let name = tank.name.clone();
            let nation = tank.nation.clone();
            let tank_type = tank.tank_type.clone();
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

            let armor = extract_armor_summary(*id);

            resolver.add(*id, TankInfo {
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

/// armor_cache.json 的解析结果（进程内只读取/解析一次，供全部坦克的回退查询共用；
/// 文件缺失/解析失败时为 None，按"无缓存"回退 BlitzKit 数据）。
fn armor_cache() -> Option<&'static serde_json::Value> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            std::fs::read_to_string(crate::data::data_path("armor_cache.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        })
        .as_ref()
}

/// 提取装甲摘要（前/侧/后，mm）：优先用游戏提取的精确装甲模型（game_data/{id}.json，
/// 按 primary 板 ID 定位各板块厚度）；缺失时回退 armor_cache.json（取每组最大厚度近似）。
fn extract_armor_summary(tank_id: u32) -> Option<ArmorData> {
    let game_path = crate::data::data_path(&format!("game_data/{}.json", tank_id));
    if let Ok(content) = std::fs::read_to_string(&game_path) {
        let am = serde_json::from_str::<serde_json::Value>(&content).ok()?;
        let armor_model = am.get("armor_model")?;
        if let Some(data) = armor_from_model(armor_model) {
            return Some(data);
        }
    }

    let cache = armor_cache()?;
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
