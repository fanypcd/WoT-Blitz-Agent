use serde::{Deserialize, Serialize};

// =====================================================================
//  统一击穿判定（核心算法，前端点击时经 POST /api/penetrate 调用）
//  逐行对齐 BlitzKit SpacedArmorSceneComponent shoot() 的判定语义：
//  跳弹 / 转正 / overmatch / 多层消耗 / 外部模块 flat 抵消 / HE 特殊 /
//  HEAT 间隙衰减（含 gap 层记录）/ 末层状态决定结果与伤害归属。
//  穿深只取 near 值 × 装备系数（BlitzKit 无距离衰减）。
// =====================================================================

/// 装甲部件分类（对齐 BlitzKit ArmorType：Primary / Spaced / External）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ArmorSection {
    /// 主装甲 Primary（角度等效，可跳弹可转正）
    Hull,
    Turret,
    /// 炮盾装甲板：BlitzKit 归 Primary（角度等效分支），非外部模块
    Gun,
    /// 间隙甲 Spaced：角度等效（可跳弹/转正），收集阶段不终止遍历
    Spaced,
    /// 外部模块 External（BlitzKit variant="track"，左右履带/负重轮共用）
    Chassis,
    /// 外部模块 External（BlitzKit variant="gun"，炮管本体）
    #[serde(rename = "gunBarrel")]
    GunBarrel,
}

/// 弹种类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellType {
    AP,
    APCR,
    HE,
    HEAT,
}

impl ShellType {
    /// 动能弹（Calibrated Shells +6% 分支）。
    pub fn is_kinetic(&self) -> bool {
        matches!(self, ShellType::AP | ShellType::APCR)
    }
    /// 高爆弹（canSplash：仅 HE）。
    pub fn is_explosive(&self) -> bool {
        matches!(self, ShellType::HE)
    }
    /// BlitzKit isExplosive()：HEAT 与 HE 都算 → 跳弹角强制 90°（永不跳弹）。
    pub fn is_explosive_type(&self) -> bool {
        matches!(self, ShellType::HEAT | ShellType::HE)
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
    /// 外部模块（flat 抵消，无角度/转正/跳弹）= BlitzKit External。
    /// Spaced 与 Primary（含炮盾）同走角度等效分支。
    pub fn is_module(&self) -> bool {
        matches!(self, ArmorSection::Chassis | ArmorSection::GunBarrel)
    }
    /// 主装甲 Primary（hull/turret/炮盾板）：收集阶段 push 后立即停止。
    pub fn is_primary(&self) -> bool {
        matches!(self, ArmorSection::Hull | ArmorSection::Turret | ArmorSection::Gun)
    }
    /// BlitzKit 外部模块按 variant 去重（ExternalModuleVariant = "gun" | "track"），
    /// 而非逐板 ID；spaced/primary 不参与 variant 去重。
    pub fn module_variant(&self) -> Option<&'static str> {
        match self {
            ArmorSection::Chassis => Some("track"),
            ArmorSection::GunBarrel => Some("gun"),
            _ => None,
        }
    }
}

/// 一次 raycast 命中的一层装甲（输入给判定）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorHit {
    pub section: ArmorSection,
    /// 装甲板 ID（显示用；判定不做逐板去重，对齐 blitzkit）
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
    /// 每层判定详情（含 HEAT 的 Gap 层，对齐 blitzkit 的 type=null 层）
    pub layers: Vec<LayerResult>,
    /// 第一层主装甲的入射角（度）
    pub first_armor_angle_deg: f32,
    /// 第一层主装甲施加的转正角（度）
    pub first_armor_norm_deg: f32,
    /// 是否跳弹
    pub ricochet: bool,
    /// 跳弹后剩余穿深（×0.75）
    pub ricochet_remaining_pen: f32,
    /// 造成伤害（blitzkit：末层被穿透 → 全额 armor_damage）
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
    /// Shell armor damage（blitzkit shell.armor_damage，HE 溅射与穿透伤害共用）
    #[serde(default)]
    pub damage: f32,
    /// HE explosion radius（溅射计算）
    #[serde(default)]
    pub explosion_radius: f32,
    /// Calibrated Shells equipment (ID 103): +6% (AP/APCR) / +7% (HEAT/HE) penetration
    #[serde(default)]
    pub calibrated_shells: bool,
    /// Enhanced Armor equipment (ID 110): +4% armor thickness
    #[serde(default)]
    pub enhanced_armor: bool,
    /// 每发弹转正角（度，tanks.pb shell.normalization；缺失按 0，对齐 blitzkit ?? 0）
    #[serde(default)]
    pub normalization_deg: Option<f32>,
    /// 每发弹跳弹临界角（度，tanks.pb shell.ricochet；HEAT/HE 由判定强制 90°）
    #[serde(default)]
    pub ricochet_deg: Option<f32>,
    /// 本次射线是否允许跳弹。主射入（true）可跳弹；
    /// 跳弹后的出射射线（blitzkit 递归 shoot(...,false,...)）传 false。
    #[serde(default = "default_true")]
    pub allow_ricochet: bool,
}

fn default_true() -> bool {
    true
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// 主判定入口：按弹种与命中列表计算穿透结果。
pub fn calculate(req: &PenetrationRequest) -> PenetrationResult {
    // 解析弹种与相关规则参数
    let shell = ShellType::from_str(&req.shell_type);
    let is_he = shell.is_explosive();
    let is_heat = matches!(shell, ShellType::HEAT);
    let caliber = req.caliber;
    // 装备修正（对齐 BlitzKit resolvePenetrationCoefficient）：
    // Calibrated Shells 穿深 +6%(AP/APCR) / +7%(HEAT/HE)，Enhanced Armor 装甲厚度 +4%
    let calib_coeff = if req.calibrated_shells {
        if shell.is_kinetic() { 1.06 } else { 1.07 }
    } else {
        1.0
    };
    // BlitzKit 仅使用 near 穿深（shell.penetration.near × 系数），无距离衰减
    let pen = req.penetration * calib_coeff;
    let thickness_coeff = if req.enhanced_armor { 1.04 } else { 1.0 };
    // 每发弹参数（blitzkit：normalization ?? 0；ricochet 仅非 explosive 弹使用，
    // HEAT/HE 的 ricochet 在上游即为 90° —— 永不跳弹）
    let norm_deg = req.normalization_deg.unwrap_or(0.0);
    let ricochet_deg = if shell.is_explosive_type() {
        90.0
    } else {
        req.ricochet_deg.unwrap_or(70.0)
    };

    // 把视线方向归一化（炮弹沿 -view 飞行）
    let view_dir = req.view_dir;
    let normalize = |v: [f32; 3]| {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 0.0 {
            [v[0] / len, v[1] / len, v[2] / len]
        } else {
            [0.0, 1.0, 0.0]
        }
    };
    let view = normalize(view_dir);
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

    // —— 收集阶段（对齐 blitzkit noDuplicateIntersections）——
    // 外部模块按 variant 去重；非外部逐个 push，push 首个 Primary 后立即 break
    // （其后所有命中——包括外部模块——都不收集）。
    let mut seen_variants: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    let mut filtered_hits: Vec<&ArmorHit> = Vec::new();
    for ah in &req.hits {
        if ah.section.is_module() {
            if let Some(variant) = ah.section.module_variant() {
                if seen_variants.insert(variant) {
                    filtered_hits.push(ah);
                }
            }
        } else {
            filtered_hits.push(ah);
            if ah.section.is_primary() {
                break;
            }
        }
    }
    // blitzkit：出射射线（allowRicochet=false）未命中任何 Primary → 返回 null。
    // 这里以空结果 "-" 表达（无出射段、伤害 0）。
    if !req.allow_ricochet && filtered_hits.iter().all(|h| !h.section.is_primary()) {
        return PenetrationResult {
            result: "-".to_string(),
            total_effective: 0.0,
            layers: Vec::new(),
            first_armor_angle_deg: -1.0,
            first_armor_norm_deg: -1.0,
            ricochet: false,
            ricochet_remaining_pen: 0.0,
            damage: 0.0,
        };
    }

    // —— 逐层消耗 ——
    let mut remaining_pen = pen; // 剩余穿深
    let mut total_effective = 0.0f32; // 累计等效厚度
    let mut layers = Vec::new(); // 每层结果
    let mut first_armor_angle_deg = -1.0f32;
    let mut first_armor_norm_deg = -1.0f32;
    let mut ricochet = false;
    let mut ricochet_remaining_pen = 0.0f32;
    let mut layer_index: usize = 0;

    for ah in &filtered_hits {
        let thickness = ah.thickness * thickness_coeff;

        // HEAT 间隙衰减：位于每层处理最前（对齐 blitzkit 顺序），并像 blitzkit
        // 一样 push 一条 gap 层（type=null）——gap 阻断时它成为末层 → blocked。
        if is_heat && layer_index > 0 {
            let prev_point = filtered_hits[layer_index - 1].point;
            let distance = dist3(ah.point, prev_point);
            let before = remaining_pen;
            remaining_pen -= 0.5 * remaining_pen * distance;
            let gap_blocked = remaining_pen <= 0.0;
            layers.push(LayerResult {
                part_name: format!("Gap {:.2}m", distance),
                thickness: 0.0,
                effective: 0.0,
                remaining_before: before,
                penetrated: !gap_blocked,
                ricochet: false,
                overmatch: false,
            });
            if gap_blocked {
                break;
            }
        }

        let eff: f32;
        let layer_penetrated: bool;
        let mut is_overmatch = false;
        if ah.section.is_module() {
            // 外部模块（履带/炮管本体）：flat 厚度，无角度/转正/跳弹
            eff = thickness;
            if is_he {
                // HE 弹遇外部模块：永远 blocked，但消耗穿深并继续（算溅射）
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
                remaining_pen -= eff;
                layer_index += 1;
                continue;
            }
            // blitzkit：减去厚度后 remaining <= 0 即 blocked（严格大于才穿透）
            layer_penetrated = remaining_pen > eff;
        } else {
            // —— Primary（hull/turret/炮盾板）与 Spaced：角度等效 + 转正 + 跳弹 ——
            let n = normalize(ah.normal);
            // blitzkit angleTo 无 abs；正面命中时 dot>0，abs 仅在法线翻转时兜底（等价）
            let cos_a = dot(n, view).abs().min(1.0);
            let angle_rad = cos_a.acos();
            let angle_deg = angle_rad.to_degrees();

            if first_armor_angle_deg < 0.0 {
                first_armor_angle_deg = angle_deg;
            }

            // Overmatch 规则（对齐 BlitzKit）：
            // - 3× 口径规则：口径 > 厚度×3，或已穿过一层(index>0)，或出射射线 → 强制不跳弹
            // - 2× 口径规则：增强转正
            let three_calibers_rule =
                caliber > thickness * 3.0 || layer_index > 0 || !req.allow_ricochet;
            is_overmatch = three_calibers_rule;
            let two_calibers_rule = caliber > thickness * 2.0;

            let normalization = if two_calibers_rule {
                (1.4 * norm_deg * caliber) / (2.0 * thickness)
            } else {
                norm_deg
            };

            // 跳弹：入射角 ≥ 跳弹角 且 未 overmatch（HEAT/HE 跳弹角=90° 永不触发）
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

            // blitzkit 对所有弹种统一应用 shell.normalization（HEAT/HE 数据即为 0）
            let final_angle = (angle_rad - normalization.to_radians()).max(0.0);
            // blitzkit 无下限钳制；1e-6 仅防 f32 在 90° 时 cos 变负（f64 下 blitzkit 为正）
            eff = thickness / final_angle.cos().max(1e-6);

            if first_armor_norm_deg < 0.0 {
                first_armor_norm_deg = normalization;
            }

            // HE 命中非 Primary（间隙甲）：blitzkit 标 blocked，但穿深照常消耗
            layer_penetrated = remaining_pen > eff && !(is_he && !ah.section.is_primary());
        }

        // —— 通用消耗 ——
        total_effective += eff;
        layers.push(LayerResult {
            part_name: ah.part_name.clone(),
            thickness,
            effective: eff,
            remaining_before: remaining_pen,
            penetrated: layer_penetrated,
            ricochet: false,
            overmatch: is_overmatch,
        });

        if layer_penetrated {
            remaining_pen -= eff;
            // 无需 break：收集阶段已在首个 Primary 后截断（blitzkit 同）
        } else if !is_he {
            break; // blitzkit：remaining <= 0 即终止（HE 除外）
        } else {
            remaining_pen -= eff; // HE 的 blocked 层同样消耗穿深
        }

        layer_index += 1;
    }

    // —— HE 判定 & 溅射伤害（对齐 BlitzKit）——
    // totalSpacedThickness = 所有非 Primary 层（外部 flat + 间隙角度等效）的等效厚度；
    // finalDamage = 0.5*dmg*(1-dist/radius) - 1.1*(lastLayer 厚度 + min(pen, totalSpaced))。
    // layers>1 或末层 blocked → splash/blocked；否则单层穿透 → 全额 damage。
    let (result_damage, is_splash, he_penetrated) = if is_he && !layers.is_empty() {
        // HE 无 gap 层，layers 与 filtered_hits 一一对应
        let total_spaced_thickness: f32 = filtered_hits
            .iter()
            .zip(layers.iter())
            .filter(|(h, _)| !h.section.is_primary())
            .map(|(_, l)| l.effective)
            .sum();
        // blitzkit：lastLayer.thicknessAngled 对 external 层不存在 → NaN → 永不 splash。
        // 【已修正】NaN 复现导致 HE 打履带/外部模块时永远 BLOCKED(0 伤害),
        // 与游戏不符(GB109 shot2: HE 打履带, 游戏判有伤害并掉血 126)。
        // 修正:外部模块末层按其 flat 厚度参与溅射衰减 → 判定方向与游戏一致
        // (SPLASH/有伤害);伤害数值可能仍偏大(游戏对履带吞噬溅射有额外衰减)。
        let last_eff = layers[layers.len() - 1].effective;
        let dist = dist3(
            filtered_hits[filtered_hits.len() - 1].point,
            filtered_hits[0].point,
        );
        if req.explosion_radius > 0.0 {
            let final_damage = 0.5 * req.damage * (1.0 - dist / req.explosion_radius)
                - 1.1 * (last_eff + total_spaced_thickness.min(pen));
            let multi_or_blocked = layers.len() > 1 || !layers.last().unwrap().penetrated;
            if multi_or_blocked {
                let s = final_damage > 0.0;
                (if s { final_damage } else { 0.0 }, s, false)
            } else {
                (req.damage, false, true)
            }
        } else {
            // 无爆炸半径（数据缺失兜底，blitzkit 无此分支）：仅单层穿透拿全额伤害
            let single_pen = layers.len() == 1 && layers[0].penetrated;
            (if single_pen { req.damage } else { 0.0 }, false, single_pen)
        }
    } else {
        // 伤害归属（对齐 blitzkit lastLayer.status）：
        // 末层被穿透 → 全额 armor_damage（含仅穿透履带/间隙甲的情形，blitzkit 语义）；
        // 否则 0。跳弹段的伤害由前端二次判定补充。
        match layers.last() {
            Some(last) if last.penetrated => (req.damage, false, true),
            _ => (0.0, false, false),
        }
    };

    let result = if ricochet {
        "RICOCHET".to_string()
    } else if is_splash {
        "SPLASH".to_string()
    } else if he_penetrated {
        "PENETRATION".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(section: ArmorSection, thickness: f32, point: [f32; 3]) -> ArmorHit {
        let part_name = format!("{:?}", section);
        ArmorHit {
            section,
            plate_id: "1".into(),
            thickness,
            normal: [0.0, 1.0, 0.0],
            point,
            part_name,
        }
    }

    fn req(shell_type: &str, pen: f32, caliber: f32, hits: Vec<ArmorHit>) -> PenetrationRequest {
        PenetrationRequest {
            shell_type: shell_type.into(),
            penetration: pen,
            caliber,
            view_dir: [0.0, 1.0, 0.0],
            hits,
            damage: 400.0,
            explosion_radius: 0.0,
            calibrated_shells: false,
            enhanced_armor: false,
            normalization_deg: None,
            ricochet_deg: None,
            allow_ricochet: true,
        }
    }

    #[test]
    fn track_only_penetration_gives_full_damage() {
        // blitzkit：末层穿透即 penetration + 全额 armor_damage
        let r = calculate(&req("ap", 100.0, 100.0, vec![hit(ArmorSection::Chassis, 20.0, [0.0; 3])]));
        assert_eq!(r.result, "PENETRATION");
        assert_eq!(r.damage, 400.0);
    }

    #[test]
    fn gun_armor_is_primary_angled_not_flat() {
        // 炮盾 = Primary：60° 入射按角度等效（20/cos60° = 40mm），35mm 穿深被挡
        // （若按外部模块 flat 处理则 35 > 20 会穿透——以此区分两种归类）
        let mut h = hit(ArmorSection::Gun, 20.0, [0.0; 3]);
        h.normal = [0.0, (60f32).to_radians().cos(), (60f32).to_radians().sin()];
        let r = calculate(&req("ap", 35.0, 60.0, vec![h]));
        assert_eq!(r.result, "BLOCKED");
        assert!((r.layers[0].effective - 40.0).abs() < 0.1, "{}", r.layers[0].effective);
        // 同板正面（0°）：20mm 直接穿透
        let r2 = calculate(&req("ap", 35.0, 60.0, vec![hit(ArmorSection::Gun, 20.0, [0.0; 3])]));
        assert_eq!(r2.result, "PENETRATION");
    }

    #[test]
    fn gun_barrel_and_gun_armor_are_separate_layers() {
        // 炮管(External, flat) → 炮盾(Primary, angled)：两层都消耗（blitzkit 不合并去重）
        let mut h = hit(ArmorSection::Gun, 20.0, [0.0, 0.0, 1.0]);
        h.normal = [0.0, 1.0, 0.0];
        let hits = vec![hit(ArmorSection::GunBarrel, 30.0, [0.0; 3]), h];
        let r = calculate(&req("ap", 100.0, 50.0, hits));
        assert_eq!(r.layers.len(), 2);
        assert_eq!(r.result, "PENETRATION");
    }

    #[test]
    fn boundary_equal_thickness_is_blocked() {
        // blitzkit：remaining_after <= 0 即 blocked（等厚不穿透）
        let r = calculate(&req("ap", 100.0, 50.0, vec![hit(ArmorSection::Hull, 100.0, [0.0; 3])]));
        assert_eq!(r.result, "BLOCKED");
        let r2 = calculate(&req("ap", 100.01, 50.0, vec![hit(ArmorSection::Hull, 100.0, [0.0; 3])]));
        assert_eq!(r2.result, "PENETRATION");
    }

    #[test]
    fn ricochet_at_70_deg_and_overmatch_suppression() {
        let mut h = hit(ArmorSection::Hull, 100.0, [0.0; 3]);
        h.normal = [0.0, (75f32).to_radians().cos(), (75f32).to_radians().sin()];
        let r = calculate(&req("ap", 300.0, 100.0, vec![h.clone()]));
        assert_eq!(r.result, "RICOCHET");
        assert!((r.ricochet_remaining_pen - 225.0).abs() < 1e-3);
        // 3× 口径 overmatch：310 > 50×3 → 强制不跳弹；等效 50/cos75° ≈ 193 < 300 → 穿透
        let mut h50 = h.clone();
        h50.thickness = 50.0;
        let r2 = calculate(&req("ap", 300.0, 310.0, vec![h50]));
        assert_eq!(r2.result, "PENETRATION");
    }

    #[test]
    fn two_caliber_enhanced_normalization() {
        // 2× 口径：转正 = 1.4·5°·150/(2·50) = 10.5°；60° 入射 → 等效 50/cos(49.5°) ≈ 76.97
        let mut h = hit(ArmorSection::Hull, 50.0, [0.0; 3]);
        h.normal = [0.0, (60f32).to_radians().cos(), (60f32).to_radians().sin()];
        let mut rq = req("ap", 80.0, 150.0, vec![h.clone()]);
        rq.normalization_deg = Some(5.0);
        let r = calculate(&rq);
        assert!((r.layers[0].effective - 76.97).abs() < 0.1, "{}", r.layers[0].effective);
        // 非 2× 口径：转正 5° → 50/cos(55°) ≈ 87.2
        let mut rq2 = req("ap", 80.0, 60.0, vec![h]);
        rq2.normalization_deg = Some(5.0);
        let r2 = calculate(&rq2);
        assert!((r2.layers[0].effective - 87.18).abs() < 0.1, "{}", r2.layers[0].effective);
    }

    #[test]
    fn heat_gap_decay_blocks_across_large_gap() {
        // 200m 间隙：剩余穿深 ×0.5^200 → 0 → gap 层 blocked → BLOCKED
        let hits = vec![
            hit(ArmorSection::Spaced, 5.0, [0.0, 0.0, 0.0]),
            hit(ArmorSection::Spaced, 5.0, [0.0, 0.0, 200.0]),
        ];
        let r = calculate(&req("heat", 500.0, 100.0, hits));
        assert_eq!(r.result, "BLOCKED");
        assert!(r.layers.iter().any(|l| l.part_name.starts_with("Gap")));
        assert!(!r.layers.last().unwrap().penetrated);
    }

    #[test]
    fn he_track_only_splashes_like_game() {
        // HE 仅命中履带：blitzkit NaN 复现已撤回——游戏实际判有伤害
        // (GB109 shot2 实测: HE 打履带掉血 126)。履带 flat 20mm 参与衰减:
        // final = 0.5·100·(1-0/5) - 1.1·(20+min(100,20)) = 50 - 44 = +6 → SPLASH
        let mut rq = req("he", 100.0, 150.0, vec![hit(ArmorSection::Chassis, 20.0, [0.0; 3])]);
        rq.explosion_radius = 5.0;
        let r = calculate(&rq);
        assert_eq!(r.result, "SPLASH");
        assert!(r.damage > 0.0);
    }

    #[test]
    fn he_splash_formula_matches_blitzkit() {
        // HE：履带(20mm flat) → 主装甲(100mm, 0°)。
        // totalSpaced = 20；dist = d；final = 0.5·400·(1-d/5) - 1.1·(100 + 20)
        let mut rq = req("he", 250.0, 150.0, vec![
            hit(ArmorSection::Chassis, 20.0, [0.0, 0.0, 0.0]),
            hit(ArmorSection::Hull, 100.0, [0.0, 0.0, 1.0]),
        ]);
        rq.explosion_radius = 5.0;
        let r = calculate(&rq);
        let expect = 0.5 * 400.0 * (1.0 - 1.0 / 5.0) - 1.1 * (100.0 + 20.0);
        assert_eq!(r.result, "SPLASH");
        assert!((r.damage - expect).abs() < 1e-3, "{} vs {}", r.damage, expect);
    }

    #[test]
    fn he_single_primary_penetration_full_damage() {
        let mut rq = req("he", 300.0, 150.0, vec![hit(ArmorSection::Hull, 100.0, [0.0; 3])]);
        rq.explosion_radius = 5.0;
        let r = calculate(&rq);
        assert_eq!(r.result, "PENETRATION");
        assert_eq!(r.damage, 400.0);
    }

    #[test]
    fn out_ray_without_primary_returns_no_shot() {
        // blitzkit：出射射线未命中 Primary → null（这里为 "-"）
        let mut rq = req("ap", 300.0, 100.0, vec![hit(ArmorSection::Chassis, 20.0, [0.0; 3])]);
        rq.allow_ricochet = false;
        let r = calculate(&rq);
        assert_eq!(r.result, "-");
        assert!(r.layers.is_empty());
    }

    #[test]
    fn external_after_primary_is_not_collected() {
        // 首个 Primary 之后的一切（含新 variant 外部模块）不收集
        let hits = vec![
            hit(ArmorSection::Hull, 50.0, [0.0; 3]),
            hit(ArmorSection::GunBarrel, 30.0, [0.0, 0.0, 1.0]),
        ];
        let r = calculate(&req("ap", 100.0, 50.0, hits));
        assert_eq!(r.layers.len(), 1);
    }

    #[test]
    fn calibrated_shells_coefficient() {
        let mut rq = req("ap", 100.0, 100.0, vec![hit(ArmorSection::Hull, 105.0, [0.0; 3])]);
        rq.calibrated_shells = true;
        assert_eq!(r_result(&rq), "PENETRATION"); // 100×1.06 = 106 > 105
        let mut rq2 = req("heat", 100.0, 100.0, vec![hit(ArmorSection::Hull, 105.0, [0.0; 3])]);
        rq2.calibrated_shells = true;
        assert_eq!(r_result(&rq2), "PENETRATION"); // 100×1.07 = 107 > 105
        let mut rq3 = req("heat", 100.0, 100.0, vec![hit(ArmorSection::Hull, 108.0, [0.0; 3])]);
        rq3.calibrated_shells = true;
        assert_eq!(r_result(&rq3), "BLOCKED"); // 107 < 108
    }

    fn r_result(rq: &PenetrationRequest) -> String {
        calculate(rq).result
    }
}
