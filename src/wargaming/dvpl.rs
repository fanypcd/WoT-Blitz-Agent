use anyhow::{anyhow, Result};

// =====================================================================
//  DVPL 解码 + 装甲/碰撞解析（DVPL 为 WG 本地资源压缩格式，末尾 20 字节 footer）
// =====================================================================

/// 一个已解码的 DVPL 文件（解压后的内容 + 压缩类型）。
pub struct DvplFile {
    pub data: Vec<u8>,
    pub compression_type: u32,
}

impl DvplFile {
    /// 读取并解码一个 DVPL 文件。末尾 20 字节 footer：
    /// `input_size(4) + compressed_size(4) + crc32(4) + compression_type(4) + "DVPL"(4)`。
    pub fn read(filepath: &std::path::Path) -> Result<Self> {
        let raw = std::fs::read(filepath)?;
        if raw.len() < 20 {
            return Err(anyhow!("File too small for DVPL footer"));
        }

        let footer = &raw[raw.len() - 20..];
        let magic = &footer[16..20];
        if magic != b"DVPL" {
            return Err(anyhow!("Not a DVPL file (magic mismatch)"));
        }

        let original_size =
            u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]) as usize;
        let comp_size = u32::from_le_bytes([footer[4], footer[5], footer[6], footer[7]]) as usize;
        let _crc32 = u32::from_le_bytes([footer[8], footer[9], footer[10], footer[11]]);
        let comp_type = u32::from_le_bytes([footer[12], footer[13], footer[14], footer[15]]);

        let compressed = &raw[..comp_size];

        // 按压缩类型解压：0=未压缩、1/2=LZ4、3=zlib
        let data = match comp_type {
            0 => compressed.to_vec(),
            1 | 2 => lz4_decompress(compressed, original_size)?,
            3 => {
                use std::io::Read;
                let mut decoder = flate2::read::ZlibDecoder::new(compressed);
                let mut buf = Vec::with_capacity(original_size);
                decoder.read_to_end(&mut buf)?;
                buf
            }
            _ => return Err(anyhow!("Unknown compression type: {}", comp_type)),
        };

        Ok(Self {
            data,
            compression_type: comp_type,
        })
    }
}

/// 自实现的 LZ4 块解压（用于 compression_type 1 / 2）。
fn lz4_decompress(src: &[u8], output_size: usize) -> Result<Vec<u8>> {
    let mut dst = vec![0u8; output_size];
    let mut si = 0;
    let mut di = 0;

    while si < src.len() && di < output_size {
        let token = src[si];
        si += 1;

        let mut lit_len = ((token >> 4) & 0x0f) as usize;
        if lit_len == 15 {
            while si < src.len() {
                let b = src[si];
                si += 1;
                lit_len += b as usize;
                if b != 255 {
                    break;
                }
            }
        }

        for _ in 0..lit_len {
            if si >= src.len() || di >= output_size {
                break;
            }
            dst[di] = src[si];
            si += 1;
            di += 1;
        }

        if si >= src.len() || di >= output_size {
            break;
        }
        if si + 2 > src.len() {
            break;
        }

        let offset = (src[si] as usize) | ((src[si + 1] as usize) << 8);
        si += 2;

        let mut match_len = ((token & 0x0f) as usize) + 4;
        if (token & 0x0f) == 15 {
            while si < src.len() {
                let b = src[si];
                si += 1;
                match_len += b as usize;
                if b != 255 {
                    break;
                }
            }
        }

        for _ in 0..match_len {
            if di >= output_size {
                break;
            }
            if di < offset {
                di += 1;
                continue;
            }
            dst[di] = dst[di - offset];
            di += 1;
        }
    }

    Ok(dst)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoundingBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// 一辆坦克的碰撞数据（包围盒 + 各部件 points 偏移，用于 3D 定位）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CollisionData {
    pub hull_bbox: Option<BoundingBox>,
    pub turret_bbox: Option<BoundingBox>,
    pub gun_bbox: Option<BoundingBox>,
    pub chassis_bbox: Option<BoundingBox>,
    pub average_thickness_hull: Option<f32>,
    pub average_thickness_turret: Option<f32>,
    #[serde(default)]
    pub hull_points: Option<[f32; 3]>,
    #[serde(default)]
    pub turret_points: Option<[f32; 3]>,
    #[serde(default)]
    pub gun_points: Option<[f32; 3]>,
    /// 车体相对底盘的位置（来自 item_defs XML 的 `<hullPosition>`）。
    /// visual 模型中车体即放在此位置，是 collision 坐标与 visual 坐标之间的权威桥梁。
    #[serde(default)]
    pub hull_position: Option<[f32; 3]>,
    /// YAML 内全部 `turret_NN:` 段的包围盒（按模型节点号索引；extract 阶段按
    /// models.pb 模块→节点映射挑选顶级配置写入 turret_bbox）。
    #[serde(default)]
    pub turret_bboxes: Vec<NumberedBBox>,
    /// YAML 内全部 `gun_NN:` 段的包围盒（同上，对应 gun_bbox）。
    #[serde(default)]
    pub gun_bboxes: Vec<NumberedBBox>,
}

/// 带模型节点号的包围盒（`turret_02:` → node=2）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NumberedBBox {
    pub node: u32,
    #[serde(flatten)]
    pub bbox: BoundingBox,
}

/// 从游戏 XML 解析出的完整装甲模型（hull / turret / gun / chassis 各部件）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArmorModel {
    pub hull: SectionArmor,
    pub turret: Option<SectionArmor>,
    pub gun: Option<SectionArmor>,
    pub chassis: Option<ChassisArmor>,
}

/// 某个部件（车体/炮塔/炮管）的装甲板集合。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SectionArmor {
    /// 装甲板 ID → 厚度（mm），如 `"1" -> 62.0`
    pub plates: std::collections::BTreeMap<String, f32>,
    /// 主装甲板引用（前/侧/后分别指向哪块板）
    pub primary: PrimaryArmor,
    /// 标记为 spaced（附加装甲，不计伤害）的板 ID
    #[serde(default)]
    pub spaced: std::collections::BTreeSet<String>,
}

/// 主装甲板引用：front/sides/rear 各自对应的装甲板 ID（如 `"armor_1"`）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrimaryArmor {
    pub front: String,
    pub sides: String,
    pub rear: String,
}

/// 底盘（履带）装甲。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChassisArmor {
    pub left_track: f32,
    pub right_track: f32,
}

impl ArmorModel {
    /// 从车辆 XML 文本解析装甲模型。
    pub fn parse_from_xml(text: &str) -> Option<Self> {
        let hull_armor = parse_section_armor(text, "<hull>")?;
        // 炮塔取 <turrets0> 的**顶级**条目（末个炮塔 × 其末个炮管），与 BlitzKit
        // models.pb 的 turrets.last() × guns.last() 同档；两者混用会让装甲摘要与
        // armor_model 互相矛盾。
        let turret_armor = parse_turret_armor(text);
        let gun_armor = parse_gun_armor(text);
        let chassis_armor = parse_chassis_armor(text);

        Some(ArmorModel {
            hull: hull_armor,
            turret: turret_armor,
            gun: gun_armor,
            chassis: chassis_armor,
        })
    }
}

/// 装甲板标签匹配（`<armor_N>厚度`），进程内只编译一次。
static ARMOR_PLATE_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"<armor_(\d+)>\s*(\d+(?:\.\d+)?)").expect("armor plate regex should compile")
});

/// 遍历 `<armor_N>VALUE</armor_N>` 装甲板：厚度入 plates；板内容含 vehicleDamageFactor
/// 的（spaced 装甲）记入 spaced。hull / turret / gun 三处装甲块解析共用。
fn parse_armor_plates(
    armor_block: &str,
) -> (
    std::collections::BTreeMap<String, f32>,
    std::collections::BTreeSet<String>,
) {
    let mut plates = std::collections::BTreeMap::new();
    let mut spaced = std::collections::BTreeSet::new();
    for cap in ARMOR_PLATE_RE.captures_iter(armor_block) {
        let plate_id = cap.get(1).unwrap().as_str().to_string();
        let thickness: f32 = cap.get(2).unwrap().as_str().parse().unwrap_or(0.0);
        plates.insert(plate_id.clone(), thickness);
        let plate_end =
            armor_block[cap.get(0).unwrap().end()..].find(&format!("</armor_{}>", plate_id));
        if let Some(end_pos) = plate_end {
            let plate_content =
                &armor_block[cap.get(0).unwrap().end()..cap.get(0).unwrap().end() + end_pos];
            if plate_content.contains("vehicleDamageFactor") {
                spaced.insert(plate_id);
            }
        }
    }
    (plates, spaced)
}

/// 解析某个部件的装甲块（含 `vehicleDamageFactor` 识别 spaced 装甲）。
fn parse_section_armor(text: &str, section_tag: &str) -> Option<SectionArmor> {
    let section_start = text.find(section_tag)?;
    let close_tag = section_tag.replace("<", "</");
    let section_end = text[section_start..].find(&close_tag)?;
    let section = &text[section_start..section_start + section_end];

    let armor_start = section.find("<armor>")?;
    let armor_end_marker = section[armor_start..].find("</armor>")?;
    let armor_block = &section[armor_start + 7..armor_start + armor_end_marker];

    let (plates, spaced) = parse_armor_plates(armor_block);

    let primary = if let Some(pa_start) = section.find("<primaryArmor>") {
        let pa_end = section[pa_start..].find("</primaryArmor>")?;
        let pa_text = &section[pa_start + 14..pa_start + pa_end];
        let parts: Vec<&str> = pa_text.split_whitespace().collect();
        PrimaryArmor {
            front: parts.first().map(|s| s.to_string()).unwrap_or_default(),
            sides: parts.get(1).map(|s| s.to_string()).unwrap_or_default(),
            rear: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
        }
    } else {
        PrimaryArmor {
            front: "armor_1".to_string(),
            sides: "armor_3".to_string(),
            rear: "armor_4".to_string(),
        }
    };

    Some(SectionArmor {
        plates,
        primary,
        spaced,
    })
}

/// `<turrets0>` 段（不含闭合标签）。
fn turrets_section_of(text: &str) -> Option<&str> {
    let start = text.find("<turrets0>")?;
    let end = text[start..].find("</turrets0>")?;
    Some(&text[start..start + end])
}

/// 拆出**顶级炮塔**自身的字段段与它的 `<guns>` 段。
///
/// `<turrets0>` 下的炮塔条目**以模块名为标签**（如 `<T-34_mod_1942>`），不是固定的
/// `<turret>`，所以不能按标签名定位；但每个炮塔的结构恒为
/// `…<armor>…<primaryArmor>…<guns>…</guns>…`。因此「最后一个 `<guns>` 之前、上一个
/// `</guns>` 之后」的区间就是顶级炮塔自己的字段，顶级炮塔内最后一个炮管即顶级主炮。
///
/// 顶级炮塔 = 游戏内"顶级配置" = BlitzKit models.pb 取的那一档
/// （`turrets.last() × guns.last()`）。
fn top_turret_span(turrets_section: &str) -> (&str, &str) {
    let Some(guns_start) = turrets_section.rfind("<guns>") else {
        return (turrets_section, "");
    };
    let guns_end = turrets_section[guns_start..]
        .find("</guns>")
        .map(|e| guns_start + e)
        .unwrap_or(guns_start);
    let own_start = turrets_section[..guns_start]
        .rfind("</guns>")
        .map(|p| p + "</guns>".len())
        .unwrap_or(0);
    (
        &turrets_section[own_start..guns_start],
        &turrets_section[guns_start..guns_end],
    )
}

/// 解析**顶级炮塔**装甲（`<turrets0>` 的最后一个炮塔条目）。
fn parse_turret_armor(text: &str) -> Option<SectionArmor> {
    let (turret_only, _guns) = top_turret_span(turrets_section_of(text)?);

    // 段内已截到本炮塔的 <guns> 之前，故此处 <armor> 必为炮塔本体装甲（非炮管装甲）；
    // 顶级炮塔无装甲块时返回 None，不回头抓上一个炮塔的板。
    let armor_start = turret_only.rfind("<armor>")?;
    let armor_end = turret_only[armor_start..].find("</armor>")?;
    let armor_block = &turret_only[armor_start + 7..armor_start + armor_end];

    let (plates, spaced) = parse_armor_plates(armor_block);

    let primary = if let Some(pa_start) = turret_only.rfind("<primaryArmor>") {
        let pa_end = turret_only[pa_start..].find("</primaryArmor>")?;
        let pa_text = &turret_only[pa_start + 14..pa_start + pa_end];
        let parts: Vec<&str> = pa_text.split_whitespace().collect();
        PrimaryArmor {
            front: parts.first().map(|s| s.to_string()).unwrap_or_default(),
            sides: parts.get(1).map(|s| s.to_string()).unwrap_or_default(),
            rear: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
        }
    } else {
        PrimaryArmor {
            front: "armor_1".to_string(),
            sides: "armor_3".to_string(),
            rear: "armor_4".to_string(),
        }
    };

    Some(SectionArmor {
        plates,
        primary,
        spaced,
    })
}

/// 解析**顶级主炮**装甲（顶级炮塔 `<guns>` 内最后一个炮管），
/// 同时把 `<gun>N</gun>` 的炮管装甲值当作板 "gun"。
fn parse_gun_armor(text: &str) -> Option<SectionArmor> {
    let (_turret, guns_section) = top_turret_span(turrets_section_of(text)?);

    let armor_start = guns_section.rfind("<armor>")?;
    let armor_end = guns_section[armor_start..].find("</armor>")?;
    let armor_block = &guns_section[armor_start + 7..armor_start + armor_end];

    let (mut plates, spaced) = parse_armor_plates(armor_block);

    if let Some(gun_start) = armor_block.find("<gun>") {
        let gun_end = armor_block[gun_start..].find("</gun>")?;
        let val_str = &armor_block[gun_start + 5..gun_start + gun_end];
        if let Ok(val) = val_str.trim().parse::<f32>() {
            plates.insert("gun".to_string(), val);
        }
    }

    Some(SectionArmor {
        plates,
        primary: PrimaryArmor {
            front: "armor_1".to_string(),
            sides: "armor_1".to_string(),
            rear: "armor_1".to_string(),
        },
        spaced,
    })
}

/// 解析底盘（左右履带）装甲。<chassis> 段内按模块名嵌套（如 <T-34_mod_1941>…），
/// 且 <unlocks> 里也有同名 <chassis> 引用标签——按段边界截取会在第一个内嵌
/// </chassis> 处提前截断。leftTrack/rightTrack 全文件唯一，直接全局查找。
fn parse_chassis_armor(text: &str) -> Option<ChassisArmor> {
    let left = extract_tag_value(text, "leftTrack")?;
    let right = extract_tag_value(text, "rightTrack")?;
    Some(ChassisArmor {
        left_track: left,
        right_track: right,
    })
}

/// 从 XML 里取某个标签的值（如 `<leftTrack>20</leftTrack>` → 20）。
fn extract_tag_value(text: &str, tag: &str) -> Option<f32> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = text.find(&open)?;
    let end = text[start..].find(&close)?;
    let value_str = text[start + open.len()..start + end].trim();
    value_str.parse().ok()
}

impl CollisionData {
    /// 从车辆 YAML 文本解析碰撞数据（包围盒 + points 定位）。
    pub fn parse_from_yaml(text: &str) -> Option<Self> {
        let mut data = CollisionData {
            hull_bbox: None,
            turret_bbox: None,
            gun_bbox: None,
            chassis_bbox: None,
            average_thickness_hull: None,
            average_thickness_turret: None,
            hull_points: None,
            turret_points: None,
            gun_points: None,
            hull_position: None,
            turret_bboxes: Vec::new(),
            gun_bboxes: Vec::new(),
        };

        if let Some(collision_idx) = text.find("collision:") {
            let collision_text = &text[collision_idx..];

            data.chassis_bbox = parse_section_bbox(collision_text, "chassis:");
            data.gun_bbox = parse_section_bbox(collision_text, "gun_01:");
            data.hull_bbox = parse_section_bbox(collision_text, "hull:");
            data.turret_bbox = parse_section_bbox(collision_text, "turret_01:");
            data.hull_points = parse_section_points(collision_text, "hull:");
            data.turret_points = parse_section_points(collision_text, "turret_01:");
            data.gun_points = parse_section_points(collision_text, "gun_01:");
            // 全量收集带节点号的炮塔/炮管段（段名不全是 _01）。顶级配置的挑选在
            // extract 阶段按 models.pb 模块→节点映射完成。
            data.turret_bboxes = parse_numbered_section_bboxes(collision_text, "turret_");
            data.gun_bboxes = parse_numbered_section_bboxes(collision_text, "gun_");

            // 解析 hull 的平均厚度（其值跟在 turret_01: 之后）
            if let Some(avg_idx) = collision_text.find("averageThickness:") {
                let after = &collision_text[avg_idx..];
                if let Some(t_idx) = after.find("turret_01:") {
                    let value_str = &after[t_idx + "turret_01:".len()..];
                    let num: String = value_str
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                        .collect();
                    if let Ok(f) = num.parse::<f32>() {
                        data.average_thickness_hull = Some(f);
                    }
                }
            }
        }

        Some(data)
    }
}

/// 解析某部件的 min/max 包围盒（限 400 字符搜索范围）。
fn parse_section_bbox(text: &str, section_name: &str) -> Option<BoundingBox> {
    parse_section_bbox_from(text, 0, section_name)
}

/// 从 `from` 偏移起找段名并解析 min/max 包围盒（供同段名多处出现时按位置区分）。
/// 段名必须命中**真段头**（见 [`find_section_header`]）——否则 averageThickness 块的
/// 厚度引用行（`turret_01: 186.08`）会抢先命中，400 字符窗口抓到后续段（常为 chassis）的包围盒。
fn parse_section_bbox_from(text: &str, from: usize, section_name: &str) -> Option<BoundingBox> {
    let section_idx = find_section_header(text.get(from..)?, section_name)? + from;
    // 固定 400 字节窗口可能落在多字节 UTF-8 字符中间：向左回退到安全边界再切片
    let mut end = (section_idx + 400).min(text.len());
    while end > section_idx && !text.is_char_boundary(end) {
        end -= 1;
    }
    let section = &text[section_idx..end];

    let min_idx = section.find("min:")?;
    let max_idx = section.find("max:")?;

    let min_str = &section[min_idx + 4..max_idx];
    let max_str = &section[max_idx + 4..];

    let min = parse_float_array(min_str)?;
    let max = parse_float_array(max_str)?;

    // 退化盒（全零）：部分 TD 的炮塔段在源文件里就是零盒——视为缺失，
    // 让上层回退（game_extract 节点兜底 / 前端网格紧致盒），与 models.pb 的省略一致
    if min == [0.0, 0.0, 0.0] && max == [0.0, 0.0, 0.0] {
        return None;
    }

    Some(BoundingBox { min, max })
}

/// `name`（含冒号）首次作为**真段头**出现的偏移：冒号后仅空白直至行尾。
/// 排除 averageThickness 块里的厚度引用行（`turret_01: 186.08`——冒号后跟数值）；
/// 与 [`parse_numbered_section_bboxes`] 的头行判定同规则。
fn find_section_header(text: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = text[from..].find(name) {
        let idx = from + rel;
        let tail = text[idx + name.len()..].trim_start_matches([' ', '\t']);
        if tail.starts_with('\n') || tail.starts_with("\r\n") {
            return Some(idx);
        }
        from = idx + name.len();
    }
    None
}

/// 收集 collision 段内全部 `turret_NN:` / `gun_NN:` 部件头及其包围盒。
/// 头行判定 = 名称+节点号后紧跟冒号且行尾（排除 hull 段 averageThickness 里的
/// `turret_02: 186.08` 引用行——那行冒号后跟数值，不是行尾）。
pub(crate) fn parse_numbered_section_bboxes(text: &str, prefix: &str) -> Vec<NumberedBBox> {
    let mut out: Vec<NumberedBBox> = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find(prefix) {
        let idx = search_from + rel;
        search_from = idx + prefix.len();
        let after = &text[idx + prefix.len()..];
        let digits: usize = after.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            continue;
        }
        let Ok(node) = after[..digits].parse::<u32>() else {
            continue;
        };
        let rest = &after[digits..];
        let is_header = rest.starts_with(":\n")
            || rest.starts_with(":\r\n")
            || rest.starts_with(':') && rest[1..].trim_start_matches('\r').starts_with('\n');
        if !is_header {
            continue;
        }
        // 段名用原始数字串（YAML 零填充：turret_02: 而非 turret_2:）；node 存数值供 models.pb 匹配
        let name = format!("{prefix}{}:", &after[..digits]);
        if let Some(bb) = parse_section_bbox_from(text, idx, &name) {
            if !out.iter().any(|n| n.node == node) {
                out.push(NumberedBBox { node, bbox: bb });
            }
        }
    }
    out
}

/// 解析某部件的 `points:` 数组（部件定位偏移）。
fn parse_section_points(text: &str, section_name: &str) -> Option<[f32; 3]> {
    let section_idx = find_section_header(text, section_name)?;
    // 固定 400 字节窗口可能落在多字节 UTF-8 字符中间：向左回退到安全边界再切片
    let mut end = (section_idx + 400).min(text.len());
    while end > section_idx && !text.is_char_boundary(end) {
        end -= 1;
    }
    let section = &text[section_idx..end];

    let points_idx = section.find("points:")?;
    let after = &section[points_idx + 7..];
    parse_float_array(after)
}

/// 解析形如 `[x, y, z]` 的浮点数组。
fn parse_float_array(s: &str) -> Option<[f32; 3]> {
    let start = s.find('[')?;
    let end = s.find(']')?;
    if end <= start {
        return None;
    }
    let inner = &s[start + 1..end];
    let nums: Vec<f32> = inner
        .split(',')
        .filter_map(|s| s.trim().parse::<f32>().ok())
        .collect();
    if nums.len() >= 3 {
        Some([nums[0], nums[1], nums[2]])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// averageThickness 块的厚度引用行（`turret_01: 55.0`）先于真段头出现时，
    /// 包围盒/points 解析必须跳到真段头——否则首次字面量匹配 + 400 字符窗口
    /// 会抓到后续段（常为 chassis）的包围盒。
    #[test]
    fn section_parse_skips_thickness_reference_lines() {
        let yaml = "\
collision:
  averageThickness:
    hull: 34.2
    turret_01: 55.0
  chassis:
    min: [-1.0, -2.0, 0.0]
    max: [1.0, 2.0, 1.0]
  turret_01:
    min: [-0.5, -0.5, -0.5]
    max: [0.5, 0.5, 0.6]
    points: [0.1, 0.2, 0.3]
";
        let c = CollisionData::parse_from_yaml(yaml).expect("parse");

        // turret_01 引用行被跳过，bbox/points 来自真段头
        let tb = c.turret_bbox.expect("turret bbox");
        assert!((tb.min[0] + 0.5).abs() < 1e-4, "min.x = {}", tb.min[0]);
        assert!((tb.max[2] - 0.6).abs() < 1e-4, "max.z = {}", tb.max[2]);
        let tp = c.turret_points.expect("turret points");
        assert!((tp[2] - 0.3).abs() < 1e-4, "points.z = {}", tp[2]);

        // hull 只有引用行、无真段 → 返回 None（不误抓 chassis 段）
        assert!(c.hull_bbox.is_none(), "hull 引用行不应解析出包围盒");
        assert!(c.hull_points.is_none());

        // chassis 直解不受影响
        let cb = c.chassis_bbox.expect("chassis bbox");
        assert!((cb.max[1] - 2.0).abs() < 1e-4);
    }

    /// 真段头在前、引用行在后的常规布局不受影响（向后兼容）。
    #[test]
    fn section_parse_normal_layout_unchanged() {
        let yaml = "\
collision:
  turret_01:
    min: [-0.5, -0.5, -0.5]
    max: [0.5, 0.5, 0.6]
  averageThickness:
    hull: 34.2
    turret_01: 55.0
";
        let c = CollisionData::parse_from_yaml(yaml).expect("parse");
        let tb = c.turret_bbox.expect("turret bbox");
        assert!((tb.min[0] + 0.5).abs() < 1e-4);
        // averageThickness（hull 均厚 = turret_01: 引用行后的数值）解析不变
        assert!((c.average_thickness_hull.unwrap() - 55.0).abs() < 1e-4);
    }

    /// 多炮塔车辆：炮塔/主炮必须取**顶级**（`<turrets0>` 最后一个条目 × 其末个炮管），
    /// 而不是首个。条目以模块名为标签（非固定 `<turret>`），故测试用实名标签。
    #[test]
    fn turret_and_gun_take_top_config_not_first() {
        let xml = "\
<root>
<hull><armor><armor_1>50</armor_1></armor><primaryArmor>armor_1 armor_1 armor_1</primaryArmor></hull>
<turrets0>
<T_mod_A>
<armor><armor_1>55</armor_1><armor_2>55</armor_2></armor>
<primaryArmor>armor_1 armor_2 armor_2</primaryArmor>
<guns>
<_gun_a><armor><armor_1>20</armor_1><gun>10</gun></armor></_gun_a>
</guns>
</T_mod_A>
<T_mod_B>
<armor><armor_1>60</armor_1><armor_2>60</armor_2><armor_3>60</armor_3>\
<armor_4>25<vehicleDamageFactor>0.0</vehicleDamageFactor></armor_4></armor>
<primaryArmor>armor_1 armor_2 armor_3</primaryArmor>
<guns>
<_gun_a><armor><armor_1>20</armor_1><gun>10</gun></armor></_gun_a>
<_gun_b><armor><armor_1>30</armor_1><gun>25</gun></armor></_gun_b>
</guns>
</T_mod_B>
</turrets0>
</root>";
        let m = ArmorModel::parse_from_xml(xml).expect("parse");

        // 炮塔：顶级条目 T_mod_B 的板（60），而非首个 T_mod_A 的 55
        let turret = m.turret.expect("turret armor");
        assert_eq!(turret.plates.get("1").copied(), Some(60.0), "应取顶级炮塔");
        assert_eq!(turret.plates.get("3").copied(), Some(60.0));
        assert_eq!(turret.primary.front, "armor_1");
        assert_eq!(turret.primary.sides, "armor_2");
        assert_eq!(turret.primary.rear, "armor_3", "primary 也必须来自顶级炮塔");
        // spaced 同样来自顶级炮塔（零厚板保留）
        assert!(turret.spaced.contains("4"), "spaced = {:?}", turret.spaced);

        // 主炮：顶级炮塔的末个炮管
        let gun = m.gun.expect("gun armor");
        assert_eq!(gun.plates.get("gun").copied(), Some(25.0), "应取顶级主炮");
        assert_eq!(gun.plates.get("1").copied(), Some(30.0));
    }

    /// 单炮塔车辆（`<turrets0>` 只含一个条目）行为不变。
    #[test]
    fn single_turret_unchanged() {
        let xml = "\
<root>
<hull><armor><armor_1>50</armor_1></armor><primaryArmor>armor_1 armor_1 armor_1</primaryArmor></hull>
<turrets0>
<Only_mod>
<armor><armor_1>40</armor_1></armor>
<primaryArmor>armor_1 armor_1 armor_1</primaryArmor>
<guns>
<_g><armor><armor_1>5</armor_1><gun>7</gun></armor></_g>
</guns>
</Only_mod>
</turrets0>
</root>";
        let m = ArmorModel::parse_from_xml(xml).expect("parse");
        assert_eq!(
            m.turret.expect("turret").plates.get("1").copied(),
            Some(40.0)
        );
        assert_eq!(m.gun.expect("gun").plates.get("gun").copied(), Some(7.0));
    }
}
