use serde::{Deserialize, Serialize};

// =====================================================================
//  统一击穿判定（核心算法，前端点击时经 POST /api/penetrate 调用）
//  复刻 BlitzKit 的穿透机制：跳弹 / 转正 / overmatch / 多层消耗 /
//  外部模块 flat 抵消 / HE 特殊 / HEAT 间隙衰减 / HP 与模块伤害区分。
// =====================================================================

/// 装甲部件分类。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ArmorSection {
    // 主装甲盒（hull / turret）：角度等效，可跳弹可转正
    Hull,
    Turret,
    // 外部模块（gun / spaced / chassis / gunBarrel）：flat 抵消
    Gun,
    Spaced,
    Chassis,
    #[serde(rename = "gunBarrel")]
    GunBarrel,
}

/// 弹种类型及各自规则参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellType {
    AP,
    APCR,
    HE,
    HEAT,
}

impl ShellType {
    /// 是否为动能弹（有转正、会跳弹）。
    pub fn is_kinetic(&self) -> bool {
        matches!(self, ShellType::AP | ShellType::APCR)
    }
    /// 是否为高爆弹。
    pub fn is_explosive(&self) -> bool {
        matches!(self, ShellType::HE)
    }
    /// 转正角（AP 5°、APCR 2°，HE/HEAT 无转正）。
    pub fn normalization_deg(&self) -> f64 {
        match self {
            ShellType::AP => 5.0,
            ShellType::APCR => 2.0,
            _ => 0.0,
        }
    }
    /// 跳弹角（AP/APCR 70°、HEAT 85°、HE 无跳弹=180°）。
    pub fn ricochet_angle_deg(&self) -> f64 {
        match self {
            ShellType::AP | ShellType::APCR => 70.0,
            ShellType::HEAT => 85.0,
            _ => 180.0,
        }
    }
    /// 从字符串解析弹种（兼容 WG 大写与 BlitzKit 小写命名）。
    pub fn from_str(s: &str) -> Self {
        let s = s.to_lowercase();
        match s.as_str() {
            "ap" => ShellType::AP,
            "apcr" | "ap_cr" | "ap_cr_premium" => ShellType::APCR,
            "he" => ShellType::HE,
            "heat" | "hc_premium" => ShellType::HEAT,
            _ => ShellType::HE,
        }
    }
}

impl ArmorSection {
    /// 是否为外部模块（flat 抵消，无角度/转正/跳弹）。
    pub fn is_module(&self) -> bool {
        matches!(self, ArmorSection::Chassis | ArmorSection::GunBarrel | ArmorSection::Gun | ArmorSection::Spaced)
    }
    /// 是否为主装甲盒（hull/turret），穿透它才算 PENETRATION。
    pub fn is_interior(&self) -> bool {
        matches!(self, ArmorSection::Hull | ArmorSection::Turret)
    }
}

/// 一次 raycast 命中的一层装甲（输入给判定）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorHit {
    pub section: ArmorSection,
    /// 装甲板 ID（用于去重）
    pub plate_id: String,
    /// 基础厚度（mm）
    pub thickness: f32,
    /// 表面法线（用于算入射角）
    pub normal: [f32; 3],
    /// 命中点坐标（用于 HEAT 间隙/HE 溅射距离）
    pub point: [f32; 3],
    /// 人类可读的名称（显示用）
    pub part_name: String,
}

/// 每层装甲的判定结果（供前端着色）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerResult {
    pub part_name: String,
    pub thickness: f32,
    pub effective: f32,
    pub remaining_before: f32,
    pub penetrated: bool,
    pub ricochet: bool,
    pub overmatch: bool,
}

/// 整体判定结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PenetrationResult {
    /// PENETRATION / BLOCKED / RICOCHET / SPLASH / "-"
    pub result: String,
    /// 穿越的总等效厚度
    pub total_effective: f32,
    /// 每层判定详情
    pub layers: Vec<LayerResult>,
    /// 第一层主装甲的入射角（度）
    pub first_armor_angle_deg: f32,
    /// 第一层主装甲施加的转正角（度）
    pub first_armor_norm_deg: f32,
    /// 是否跳弹
    pub ricochet: bool,
    /// 跳弹后剩余穿深（×0.75）
    pub ricochet_remaining_pen: f32,
    /// 造成伤害（区分 HP / 模块伤害，见下）
    #[serde(default)]
    pub damage: f32,
}

/// 前端点击后提交的判定请求。
#[derive(Debug, Clone, Deserialize)]
pub struct PenetrationRequest {
    pub shell_type: String,
    pub penetration: f32,
    pub caliber: f32,
    pub view_dir: [f32; 3],
    pub hits: Vec<ArmorHit>,
    /// Shell damage (HE splash / normal damage)
    #[serde(default)]
    pub damage: f32,
    /// Module damage (tracks, gun barrel etc.) — applied when only modules were hit
    #[serde(default)]
    pub module_damage: f32,
    /// HE explosion radius (for splash calculation)
    #[serde(default)]
    pub explosion_radius: f32,
    /// Calibrated Shells equipment (ID 103): +4% penetration
    #[serde(default)]
    pub calibrated_shells: bool,
    /// Enhanced Armor equipment (ID 110): +4% armor thickness
    #[serde(default)]
    pub enhanced_armor: bool,
}

/// 主判定入口：按弹种与命中列表计算穿透结果。
pub fn calculate(req: &PenetrationRequest) -> PenetrationResult {
    // 解析弹种与相关规则参数
    let shell = ShellType::from_str(&req.shell_type);
    let is_kinetic = shell.is_kinetic();
    let is_he = shell.is_explosive();
    let is_heat = matches!(shell, ShellType::HEAT);
    let norm_deg = shell.normalization_deg() as f32;
    let ricochet_deg = shell.ricochet_angle_deg() as f32;
    let caliber = req.caliber;
    // 装备修正：Calibrated Shells 穿深 +4%，Enhanced Armor 装甲厚度 +4%
    let pen = req.penetration * if req.calibrated_shells { 1.04 } else { 1.0 };
    let thickness_coeff = if req.enhanced_armor { 1.04 } else { 1.0 };

    // 把视线方向归一化（炮弹沿 -view 飞行）
    let view_dir = req.view_dir;
    let normalize = |v: [f32; 3]| {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 0.0 { [v[0] / len, v[1] / len, v[2] / len] } else { [0.0, 1.0, 0.0] }
    };
    let view = normalize(view_dir);
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

    // —— 状态变量 ——
    let mut remaining_pen = pen;           // 剩余穿深
    let mut total_effective = 0.0f32;      // 累计等效厚度
    let mut layers = Vec::new();           // 每层结果
    let mut first_armor_angle_deg = -1.0f32;
    let mut first_armor_norm_deg = -1.0f32;
    let mut ricochet = false;
    let mut ricochet_remaining_pen = 0.0f32;
    let mut layer_index: usize = 0;
    let mut hit_interior = false;          // 是否命中主装甲盒
    let mut all_penetrated = true;

    // 去重：同一 section:plate_id 只算一次；记录是否已见到主装甲
    let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut filtered_hits: Vec<&ArmorHit> = Vec::new();
    let mut saw_primary = false;

    for ah in &req.hits {
        let key = format!("{}:{}", serde_json::to_string(&ah.section).unwrap_or_default(), ah.plate_id);
        if ah.section.is_module() {
            if seen_keys.insert(key) {
                filtered_hits.push(ah);
            }
            // External modules after the first primary are exit armor — stop collecting
            if saw_primary { break; }
        } else {
            if seen_keys.insert(key) {
                filtered_hits.push(ah);
            }
            if ah.section.is_interior() {
                saw_primary = true;
                // Include only one plate after primary (possible second hull layer)
                let primary_count = filtered_hits.iter().filter(|h| h.section.is_interior()).count();
                if primary_count >= 2 { break; }
            }
        }
    }

    for ah in &filtered_hits {
        let eff: f32;
        let mut is_overmatch = false;
        let thickness = ah.thickness * thickness_coeff;

        // HEAT 间隙衰减：跨层空气间隙时穿深按距离衰减
        if is_heat && layer_index > 0 {
            let prev_point: [f32; 3] = filtered_hits[layer_index - 1].point;
            let dx = ah.point[0] - prev_point[0];
            let dy = ah.point[1] - prev_point[1];
            let dz = ah.point[2] - prev_point[2];
            let distance = (dx * dx + dy * dy + dz * dz).sqrt();
            remaining_pen -= 0.5 * remaining_pen * distance;
            if remaining_pen <= 0.0 {
                all_penetrated = false;
                break;
            }
        }

        if ah.section.is_module() {
            // 外部模块：flat 厚度，无角度/转正/跳弹
            eff = thickness;

            // HE 弹碰到外部装甲必定被阻挡
            if is_he {
                layers.push(LayerResult {
                    part_name: ah.part_name.clone(),
                    thickness,
                    effective: eff,
                    remaining_before: remaining_pen,
                    penetrated: false,
                    ricochet: false,
                    overmatch: false,
                });
                all_penetrated = false;
                total_effective = eff;
                break;
            }
        } else {
            // —— 主装甲盒（hull/turret）：算入射角 + 转正 + overmatch ——
            let n = normalize(ah.normal);
            let cos_a = dot(n, view).abs().min(1.0);
            let angle_rad = cos_a.acos();
            let angle_deg = angle_rad.to_degrees();

            if first_armor_angle_deg < 0.0 {
                first_armor_angle_deg = angle_deg;
            }

            // Overmatch 规则：
            // - 3× 口径规则：口径 > 厚度×3（或已穿过一层）→ 强制不跳弹
            // - 2× 口径规则：增强转正
            let three_calibers_rule = caliber > thickness * 3.0 || layer_index > 0;
            let two_calibers_rule = caliber > thickness * 2.0;

            let normalization = if two_calibers_rule {
                (1.4 * norm_deg * caliber) / (2.0 * thickness)
            } else {
                norm_deg
            };

            // 跳弹：动能弹且入射角 ≥ 跳弹角（且未 overmatch）
            if !three_calibers_rule && is_kinetic && angle_deg >= ricochet_deg {
                ricochet = true;
                ricochet_remaining_pen = remaining_pen * 0.75;
                layers.push(LayerResult {
                    part_name: ah.part_name.clone(),
                    thickness,
                    effective: 0.0,
                    remaining_before: remaining_pen,
                    penetrated: false,
                    ricochet: true,
                    overmatch: false,
                });
                break;
            }

            // 有效厚度 = 基础厚度 / cos(最终入射角)，角度下限 0.01 防除零
            let effective_norm = if is_kinetic { normalization } else { 0.0 };
            let final_angle = (angle_rad - effective_norm.to_radians()).max(0.0);
            eff = thickness / final_angle.cos().max(0.01);
            is_overmatch = three_calibers_rule;

            if first_armor_norm_deg < 0.0 && is_kinetic {
                first_armor_norm_deg = effective_norm;
            }
        }

        // —— 通用消耗 ——
        total_effective += eff;
        if ah.section.is_interior() { hit_interior = true; }
        let penetrated = remaining_pen >= eff;
        layers.push(LayerResult {
            part_name: ah.part_name.clone(),
            thickness,
            effective: eff,
            remaining_before: remaining_pen,
            penetrated,
            ricochet: false,
            overmatch: is_overmatch,
        });

        // 穿透则扣减穿深；穿透主装甲盒后停止；否则在本层被阻挡
        if penetrated {
            remaining_pen -= eff;
            if ah.section.is_interior() { break; }
        } else {
            all_penetrated = false;
            break;
        }

        layer_index += 1;
    }

    // HE 特殊处理：只看第一层——外部则阻挡，内部则比穿深
    if is_he && !layers.is_empty() {
        let ah0 = &filtered_hits[0];
        total_effective = layers[0].effective;
        remaining_pen = pen;
        all_penetrated = remaining_pen >= layers[0].effective && ah0.section.is_interior();
        hit_interior = ah0.section.is_interior();
        layers.truncate(1);
        layers[0].penetrated = all_penetrated;
    }

    // —— HE 溅射伤害 ——（伤害随距离衰减，被 spaced + 核心板等效厚度抵消）
    let mut result_damage = 0.0f32;
    let mut is_splash = false;
    if is_he && !layers.is_empty() {
        let last = layers[layers.len() - 1].effective.max(0.01);
        let total_spaced_thickness: f32 = layers.iter()
            .filter(|l| !l.penetrated)
            .map(|l| l.thickness)
            .sum();
        let first_pt = filtered_hits[0].point;
        let last_pt = filtered_hits[filtered_hits.len() - 1].point;
        let dx = last_pt[0] - first_pt[0];
        let dy = last_pt[1] - first_pt[1];
        let dz = last_pt[2] - first_pt[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if req.explosion_radius > 0.0 {
            let final_damage = 0.5 * req.damage * (1.0 - dist / req.explosion_radius)
                - 1.1 * (last + total_spaced_thickness.min(pen));
            if final_damage > 0.0 {
                is_splash = true;
                result_damage = final_damage;
            }
        }
    } else {
        // Distinguish HP damage (main armor box penetrated) from module damage
        // (only external modules like tracks / gun were hit — they take module damage).
        let hit_module_only = !hit_interior && layers.iter().any(|l| l.penetrated);
        result_damage = if all_penetrated && hit_interior {
            req.damage
        } else if hit_module_only {
            req.module_damage
        } else {
            0.0
        };
    }

    let result = if ricochet {
        "RICOCHET".to_string()
    } else if all_penetrated && hit_interior {
        "PENETRATION".to_string()
    } else if is_splash {
        "SPLASH".to_string()
    } else if pen > 0.0 {
        "BLOCKED".to_string()
    } else {
        "-".to_string()
    };

    PenetrationResult {
        result,
        total_effective,
        layers,
        first_armor_angle_deg,
        first_armor_norm_deg,
        ricochet,
        ricochet_remaining_pen,
        damage: result_damage,
    }
}
