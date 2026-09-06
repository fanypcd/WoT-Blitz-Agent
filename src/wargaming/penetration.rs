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
    // 外部模块（gun / chassis / gunBarrel）：flat 抵消
    Gun,
    /// 间隙甲：角度等效（可跳弹/转正），对齐 BlitzKit 非外部分支
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
    /// 跳弹角（AP/APCR 70°；HEAT/HE 在 BlitzKit 中 isExplosive → 强制 90°，永不跳弹）。
    pub fn ricochet_angle_deg(&self) -> f64 {
        match self {
            ShellType::AP | ShellType::APCR => 70.0,
            ShellType::HEAT | ShellType::HE => 90.0,
        }
    }
    /// 从字符串解析弹种（覆盖 tanks.pb 原始类型串与归一化名——
    /// 原始串：hc/hc_premium=HEAT、ap_cr*/apcr=APCR、ap_premium=AP、he_premium=HE）。
    pub fn from_str(s: &str) -> Self {
        let s = s.to_lowercase();
        match s.as_str() {
            "ap" | "ap_premium" => ShellType::AP,
            "apcr" | "ap_cr" | "ap_cr_premium" => ShellType::APCR,
            "he" | "he_premium" => ShellType::HE,
            "heat" | "hc" | "hc_premium" => ShellType::HEAT,
            _ => ShellType::HE,
        }
    }
}

impl ArmorSection {
    /// 是否为外部模块（flat 抵消，无角度/转正/跳弹）。
    /// 注意：Spaced（间隙甲）不是外部模块——BlitzKit 中它走角度等效分支（可跳弹/转正）。
    pub fn is_module(&self) -> bool {
        matches!(self, ArmorSection::Chassis | ArmorSection::GunBarrel | ArmorSection::Gun)
    }
    /// 是否为主装甲盒（hull/turret），穿透它才算 PENETRATION。
    pub fn is_interior(&self) -> bool {
        matches!(self, ArmorSection::Hull | ArmorSection::Turret)
    }
    /// BlitzKit 外部模块按 variant 去重（track/gun），而非逐板 ID。
    /// chassis(左右履带/负重轮) 同一 variant="track"；gunBarrel/gun 同一 variant="gun"；
    /// spaced 不属于外部模块（走角度分支，不参与 variant 去重）。
    pub fn module_variant(&self) -> Option<&'static str> {
        match self {
            ArmorSection::Chassis => Some("track"),
            ArmorSection::GunBarrel | ArmorSection::Gun => Some("gun"),
            _ => None,
        }
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
    /// Calibrated Shells equipment (ID 103): +6% (AP/APCR) / +7% (HEAT/HE) penetration
    #[serde(default)]
    pub calibrated_shells: bool,
    /// 远距离穿深（穿深衰减终点）。None 表示不衰减。
    #[serde(default)]
    pub penetration_far: Option<f32>,
    /// 最大射程（衰减参考距离）。
    #[serde(default)]
    pub range: Option<f32>,
    /// Enhanced Armor equipment (ID 110): +4% armor thickness
    #[serde(default)]
    pub enhanced_armor: bool,
    /// 炮口到命中点的距离（m）。穿深随距离从近值线性衰减到远值。
    #[serde(default)]
    pub distance: f32,
    /// 本次射线是否允许跳弹。主射入（allowRicochet=true）可跳弹；
    /// 跳弹后的出射射线（BlitzKit 重建 `shoot(...,false,...)`）不再允许跳弹，
    /// 使 threeCalibersRule 恒真（不会再跳弹，转为穿透判定）。
    #[serde(default = "default_true")]
    pub allow_ricochet: bool,
}

fn default_true() -> bool { true }

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
    // 装备修正（对齐 BlitzKit resolvePenetrationCoefficient）：
    // Calibrated Shells 穿深 +6%(AP/APCR) / +7%(HEAT/HE)，Enhanced Armor 装甲厚度 +4%
    let calib_coeff = if req.calibrated_shells {
        if shell.is_kinetic() { 1.06 } else { 1.07 }
    } else { 1.0 };
    let mut pen = req.penetration * calib_coeff;
    // 穿深随距离线性衰减：近距(near) → 远距(far)。衰减参考距离取 720m（多数弹的射程）。
    // 距离<=0（无炮口参考）时不衰减，保持旧有行为。
    if req.distance > 0.0 {
        if let Some(far) = req.penetration_far {
            let range = req.range.unwrap_or(720.0).max(1.0);
            let t = (req.distance / range).min(1.0);
            let decayed = req.penetration - (req.penetration - far) * t as f32;
            pen *= decayed / req.penetration;
        }
    }
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

    // 去重：主装甲按 section:plate_id 只算一次；外部模块按 BlitzKit 的 variant(track/gun) 只算一次。
    // 记录是否已见到主装甲（external 出现在主装甲之后即为"出口装甲"，停止收集）。
    let mut seen_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_variants: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    let mut filtered_hits: Vec<&ArmorHit> = Vec::new();
    let mut saw_primary = false;

    for ah in &req.hits {
        if ah.section.is_module() {
            // BlitzKit：外部模块按 variant 去重（track 只有一层，gun 只有一层）。
            if let Some(variant) = ah.section.module_variant() {
                if seen_variants.insert(variant) {
                    filtered_hits.push(ah);
                }
            } else if seen_keys.insert(format!("{:?}:{}", ah.section, ah.plate_id)) {
                filtered_hits.push(ah);
            }
            // External modules after the first primary are exit armor — stop collecting
            if saw_primary { break; }
        } else {
            // 主装甲/间隙甲：第一块主装甲之后的命中都是出射装甲——停止收集（对齐 BlitzKit
            // noDuplicateIntersections 在 push 首个 Primary 后立即 break 的行为）。
            if saw_primary { break; }
            if seen_keys.insert(format!("{:?}:{}", ah.section, ah.plate_id)) {
                filtered_hits.push(ah);
            }
            if ah.section.is_interior() {
                saw_primary = true;
            }
        }
    }

    for ah in &filtered_hits {
        let mut is_overmatch = false;
        let thickness = ah.thickness * thickness_coeff;
        let is_interior = ah.section.is_interior();

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

        let eff: f32;
        let layer_penetrated: bool;
        if ah.section.is_module() {
            // 外部模块（履带/炮管/炮盾外观）：flat 厚度，无角度/转正/跳弹
            eff = thickness;
            // HE 弹碰到外部装甲：永远 blocked，但消耗穿深并继续处理后续层，
            // 以便计算溅射伤害（totalSpacedThickness 与 lastLayer）。
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
                total_effective += eff;
                all_penetrated = false;
                remaining_pen -= eff;
                layer_index += 1;
                continue;
            }
            layer_penetrated = remaining_pen >= eff;
        } else {
            // —— 主装甲盒(hull/turret)与间隙甲(spaced)：角度等效 + 转正 + 跳弹
            //    （对齐 BlitzKit shoot() 的非外部分支：Spaced 与 Primary 同路处理）——
            let n = normalize(ah.normal);
            let cos_a = dot(n, view).abs().min(1.0);
            let angle_rad = cos_a.acos();
            let angle_deg = angle_rad.to_degrees();

            if first_armor_angle_deg < 0.0 {
                first_armor_angle_deg = angle_deg;
            }

            // Overmatch 规则（对齐 BlitzKit）：
            // - 3× 口径规则：口径 > 厚度×3，或已穿过一层(index>0)，或不允许跳弹(出射射线) → 强制不跳弹
            // - 2× 口径规则：增强转正
            let three_calibers_rule = caliber > thickness * 3.0 || layer_index > 0 || !req.allow_ricochet;
            let two_calibers_rule = caliber > thickness * 2.0;

            let normalization = if two_calibers_rule {
                (1.4 * norm_deg * caliber) / (2.0 * thickness)
            } else {
                norm_deg
            };

            // 跳弹：入射角 ≥ 跳弹角 且 未 overmatch（HE/HEAT 跳弹角=90° 所以永不触发）。
            if !three_calibers_rule && angle_deg >= ricochet_deg {
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

            // HE 命中非主装甲盒（间隙甲）：BlitzKit 标记 blocked，但仍消耗穿深并继续。
            layer_penetrated = remaining_pen >= eff && !(is_he && !is_interior);
        }

        // —— 通用消耗 ——
        total_effective += eff;
        if is_interior { hit_interior = true; }
        layers.push(LayerResult {
            part_name: ah.part_name.clone(),
            thickness,
            effective: eff,
            remaining_before: remaining_pen,
            penetrated: layer_penetrated,
            ricochet: false,
            overmatch: is_overmatch,
        });

        // 穿透则扣减穿深；穿透主装甲盒后停止；否则在本层被阻挡。
        // HE 不提前 break——BlitzKit 对 HE 处理完所有层，以便计算溅射伤害与间隙厚度；
        // 且 HE 的 blocked 层同样消耗穿深（BlitzKit 在分支内无条件 -=）。
        if layer_penetrated {
            remaining_pen -= eff;
            if is_interior && !is_he { break; }
        } else {
            all_penetrated = false;
            if is_he {
                remaining_pen -= eff;
            } else {
                break;
            }
        }

        layer_index += 1;
    }

    // HE 特判：若 ray 未穿透任何主装甲盒（全部被外部/首道主装甲阻挡）则绝非 penetration。
    if is_he && !layers.is_empty() {
        all_penetrated = hit_interior && !layers.iter().any(|l| !l.penetrated && l.effective > 0.0);
        hit_interior = layers.iter().any(|l| l.penetrated && l.ricochet == false) && hit_interior;
    }

        // —— HE 判定 & 溅射伤害（对齐 BlitzKit）——
    // BlitzKit：保留全部 layers；totalSpacedThickness = 所有非主装甲层(外部flat+间隙espaced)的等效厚度；
    // finalDamage = 0.5*dmg*(1-dist/radius) - 1.1*(lastLayer.effective + min(pen, totalSpacedThickness))。
    // 若 layers.length>1 或 最后一层 blocked → 溅射/阻挡(finalDamage)；否则单层穿透 → 全额 damage。
    let (result_damage, is_splash) = if is_he && !layers.is_empty() {
        // totalSpacedThickness：所有非主装甲层的 effective（外部flat + 间隙spaced 角度等效）。
        // layers 与 filtered_hits 前缀一一对应（zip 防御 ricochet 提前 break 的错位）。
        let total_spaced_thickness: f32 = filtered_hits.iter()
            .zip(layers.iter())
            .filter(|(h, _)| !h.section.is_interior())
            .map(|(_, l)| l.effective)
            .sum();
        let last = layers[layers.len() - 1].effective.max(0.01);
        let first_pt = filtered_hits[0].point;
        let last_pt = filtered_hits[filtered_hits.len() - 1].point;
        let dx = last_pt[0] - first_pt[0];
        let dy = last_pt[1] - first_pt[1];
        let dz = last_pt[2] - first_pt[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if req.explosion_radius > 0.0 {
            let final_damage = 0.5 * req.damage * (1.0 - dist / req.explosion_radius)
                - 1.1 * (last + total_spaced_thickness.min(pen));
            let multi_or_blocked = layers.len() > 1 || !all_penetrated;
            if multi_or_blocked {
                let s = final_damage > 0.0;
                (if s { final_damage } else { 0.0 }, s)
            } else {
                (req.damage, false)
            }
        } else {
            // 无爆炸半径：只有单层穿透才算 penetration（拿全额伤害），否则 blocked。
            (if all_penetrated { req.damage } else { 0.0 }, false)
        }
    } else {
        // 伤害归属（以最后一层状态为准，对齐 BlitzKit lastLayer.status；并保留本项目的
        // HP/模块伤害区分改进）：
        // - 最终穿透主装甲盒（hull/turret）→ HP 伤害；
        // - 最后一层为可破坏外部模块（履带/炮管）且被穿透 → 模块伤害；
        // - 仅穿透间隙甲（spaced，不可破坏模块）后被主装甲阻挡 / 未触及主装甲 → 0
        //   （间隙甲穿透不算击穿坦克，也不造成伤害）。
        let d = if layers.is_empty() {
            0.0
        } else {
            let last_idx = layers.len() - 1;
            let last_is_module = filtered_hits.get(last_idx)
                .map(|h| h.section.is_module())
                .unwrap_or(false);
            if all_penetrated && hit_interior {
                req.damage
            } else if !hit_interior && layers[last_idx].penetrated && last_is_module {
                req.module_damage
            } else {
                0.0
            }
        };
        (d, false)
    };

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
