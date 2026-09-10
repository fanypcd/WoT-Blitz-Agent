use anyhow::{Result, anyhow};

// =====================================================================
//  DVPL 解码 + 装甲/碰撞解析
//  DVPL 是 Wargaming 游戏的本地资源压缩格式（末尾 20 字节 footer）。
//  本模块负责：解码 DVPL → 从 XML 解析装甲厚度、从 YAML 解析碰撞数据。
// =====================================================================

/// 一个已解码的 DVPL 文件（解压后的内容 + 压缩类型）。
pub struct DvplFile {
    pub data: Vec<u8>,
    pub compression_type: u32,
}

impl DvplFile {
    /// 读取并解码一个 DVPL 文件。
    ///
    /// 文件末尾 20 字节 footer：`input_size(4) + compressed_size(4) + crc32(4)
    /// + compression_type(4) + "DVPL"magic(4)`。
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

        // 解析 footer：原始大小 / 压缩大小 / CRC32 / 压缩类型
        let original_size = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]) as usize;
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

        Ok(Self { data, compression_type: comp_type })
    }
}

/// 自实现的 LZ4 块解压（用于 compression_type 1 / 2）。
fn lz4_decompress(src: &[u8], output_size: usize) -> Result<Vec<u8>> {
    let mut dst = vec![0u8; output_size];
    let mut si = 0;
    let mut di = 0;

    while si < src.len() && di < output_size {
        let token = src[si]; si += 1;

        let mut lit_len = ((token >> 4) & 0x0f) as usize;
        if lit_len == 15 {
            while si < src.len() {
                let b = src[si]; si += 1;
                lit_len += b as usize;
                if b != 255 { break; }
            }
        }

        for _ in 0..lit_len {
            if si >= src.len() || di >= output_size { break; }
            dst[di] = src[si]; si += 1; di += 1;
        }

        if si >= src.len() || di >= output_size { break; }
        if si + 2 > src.len() { break; }

        let offset = (src[si] as usize) | ((src[si + 1] as usize) << 8);
        si += 2;

        let mut match_len = ((token & 0x0f) as usize) + 4;
        if (token & 0x0f) == 15 {
            while si < src.len() {
                let b = src[si]; si += 1;
                match_len += b as usize;
                if b != 255 { break; }
            }
        }

        for _ in 0..match_len {
            if di >= output_size { break; }
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
        // Parse hull armor
        let hull_armor = parse_section_armor(text, "<hull>")?;
        
        // Parse turret armor (XML uses <turrets0> not <turret>)
        let turret_armor = parse_turret_armor(text);
        
        // Parse gun armor (inside <guns> within turret section)
        let gun_armor = parse_gun_armor(text);
        
        // Parse chassis armor
        let chassis_armor = parse_chassis_armor(text);
        
        Some(ArmorModel {
            hull: hull_armor,
            turret: turret_armor,
            gun: gun_armor,
            chassis: chassis_armor,
        })
    }
}

/// 解析某个部件的装甲块（含 `vehicleDamageFactor` 识别 spaced 装甲）。
fn parse_section_armor(text: &str, section_tag: &str) -> Option<SectionArmor> {
    let section_start = text.find(section_tag)?;
    // Find the closing tag
    let close_tag = section_tag.replace("<", "</");
    let section_end = text[section_start..].find(&close_tag)?;
    let section = &text[section_start..section_start + section_end];
    
    // 找到该部件的 <armor>...</armor> 块
    let armor_start = section.find("<armor>")?;
    let armor_end_marker = section[armor_start..].find("</armor>")?;
    let armor_block = &section[armor_start + 7..armor_start + armor_end_marker];
    
    // 解析所有 <armor_N>VALUE</armor_N>，有的含 vehicleDamageFactor 子字段（spaced 装甲）
    let mut plates = std::collections::BTreeMap::new();
    let mut spaced = std::collections::BTreeSet::new();
    let re = regex::Regex::new(r"<armor_(\d+)>\s*(\d+(?:\.\d+)?)").ok()?;
    for cap in re.captures_iter(armor_block) {
        let plate_id = cap.get(1).unwrap().as_str().to_string();
        let thickness: f32 = cap.get(2).unwrap().as_str().parse().unwrap_or(0.0);
        plates.insert(plate_id.clone(), thickness);
        // Check if this plate has vehicleDamageFactor (spaced armor)
        let plate_end = armor_block[cap.get(0).unwrap().end()..].find(&format!("</armor_{}>", plate_id));
        if let Some(end_pos) = plate_end {
            let plate_content = &armor_block[cap.get(0).unwrap().end()..cap.get(0).unwrap().end() + end_pos];
            if plate_content.contains("vehicleDamageFactor") {
                spaced.insert(plate_id);
            }
        }
    }
    
    // Parse <primaryArmor>armor_X armor_Y armor_Z</primaryArmor>
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
    
    Some(SectionArmor { plates, primary, spaced })
}

/// 解析炮塔装甲（XML 用 `<turrets0>` 而非 `<turret>`，注意区分）。
fn parse_turret_armor(text: &str) -> Option<SectionArmor> {
    let turrets_start = text.find("<turrets0>")?;
    let turrets_end = text[turrets_start..].find("</turrets0>")?;
    let turrets_section = &text[turrets_start..turrets_start + turrets_end];

    // Find guns section to exclude it from turret armor search
    let guns_start = turrets_section.find("<guns>");

    // Find the first <armor> block that's before <guns> (turret armor)
    let armor_start = turrets_section.find("<armor>")?;
    // Check this armor block is before guns section
    if let Some(gs) = guns_start {
        if armor_start >= gs {
            // The first armor is inside guns, no separate turret armor
            return None;
        }
    }
    let armor_end = turrets_section[armor_start..].find("</armor>")?;
    let armor_block = &turrets_section[armor_start + 7..armor_start + armor_end];

    let mut plates = std::collections::BTreeMap::new();
    let mut spaced = std::collections::BTreeSet::new();
    let re = regex::Regex::new(r"<armor_(\d+)>\s*(\d+(?:\.\d+)?)").ok()?;
    for cap in re.captures_iter(armor_block) {
        let plate_id = cap.get(1).unwrap().as_str().to_string();
        let thickness: f32 = cap.get(2).unwrap().as_str().parse().unwrap_or(0.0);
        plates.insert(plate_id.clone(), thickness);
        let plate_end = armor_block[cap.get(0).unwrap().end()..].find(&format!("</armor_{}>", plate_id));
        if let Some(end_pos) = plate_end {
            let plate_content = &armor_block[cap.get(0).unwrap().end()..cap.get(0).unwrap().end() + end_pos];
            if plate_content.contains("vehicleDamageFactor") {
                spaced.insert(plate_id);
            }
        }
    }

    // 炮塔的 primaryArmor 在 <guns> 之前，截取该段再解析
    let search_end = guns_start.unwrap_or(turrets_section.len());
    let turret_only = &turrets_section[..search_end];
    let primary = if let Some(pa_start) = turret_only.find("<primaryArmor>") {
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

    Some(SectionArmor { plates, primary, spaced })
}

/// 解析炮管装甲（`<guns>` 段），同时把 `<gun>N</gun>` 的炮管装甲值当作板 "gun"。
fn parse_gun_armor(text: &str) -> Option<SectionArmor> {
    let turrets_start = text.find("<turrets0>")?;
    let turrets_end = text[turrets_start..].find("</turrets0>")?;
    let turrets_section = &text[turrets_start..turrets_start + turrets_end];

    // Find guns section
    let guns_start = turrets_section.find("<guns>")?;
    let guns_end = turrets_section[guns_start..].find("</guns>")?;
    let guns_section = &turrets_section[guns_start..guns_start + guns_end];

    // Find <armor> block within guns section
    let armor_start = guns_section.find("<armor>")?;
    let armor_end = guns_section[armor_start..].find("</armor>")?;
    let armor_block = &guns_section[armor_start + 7..armor_start + armor_end];

    let mut plates = std::collections::BTreeMap::new();
    let mut spaced = std::collections::BTreeSet::new();
    let re = regex::Regex::new(r"<armor_(\d+)>\s*(\d+(?:\.\d+)?)").ok()?;
    for cap in re.captures_iter(armor_block) {
        let plate_id = cap.get(1).unwrap().as_str().to_string();
        let thickness: f32 = cap.get(2).unwrap().as_str().parse().unwrap_or(0.0);
        plates.insert(plate_id.clone(), thickness);
        // 与 parse_section_armor 一致：板内容含 vehicleDamageFactor → spaced 附加装甲
        let plate_end = armor_block[cap.get(0).unwrap().end()..].find(&format!("</armor_{}>", plate_id));
        if let Some(end_pos) = plate_end {
            let plate_content = &armor_block[cap.get(0).unwrap().end()..cap.get(0).unwrap().end() + end_pos];
            if plate_content.contains("vehicleDamageFactor") {
                spaced.insert(plate_id);
            }
        }
    }

    // Parse <gun>N</gun> barrel armor value
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

/// 解析底盘（左右履带）装甲。
fn parse_chassis_armor(text: &str) -> Option<ChassisArmor> {
    let chassis_start = text.find("<chassis>")?;
    let chassis_end = text[chassis_start..].find("</chassis>")?;
    let chassis_section = &text[chassis_start..chassis_start + chassis_end];
    
    let left = extract_tag_value(chassis_section, "leftTrack")?;
    let right = extract_tag_value(chassis_section, "rightTrack")?;
    
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
        };

        // 定位 collision 节并逐个解析各部件
        if let Some(collision_idx) = text.find("collision:") {
            let collision_text = &text[collision_idx..];

            // Parse each section within collision
            data.chassis_bbox = parse_section_bbox(collision_text, "chassis:");
            data.gun_bbox = parse_section_bbox(collision_text, "gun_01:");
            data.hull_bbox = parse_section_bbox(collision_text, "hull:");
            data.turret_bbox = parse_section_bbox(collision_text, "turret_01:");
            data.hull_points = parse_section_points(collision_text, "hull:");
            data.turret_points = parse_section_points(collision_text, "turret_01:");
            data.gun_points = parse_section_points(collision_text, "gun_01:");

            // 解析 hull 的平均厚度（其值在 turret_01 之前）
            if let Some(avg_idx) = collision_text.find("averageThickness:") {
                let after = &collision_text[avg_idx..];
                // The value after "turret_01:" is the hull average thickness
                if let Some(t_idx) = after.find("turret_01:") {
                    let value_str = &after[t_idx + "turret_01:".len()..];
                    let num: String = value_str.trim_start()
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
    // Find the section, then look for bbox within the next ~300 chars
    let section_idx = text.find(section_name)?;
    let section_end = section_idx + 400; // Limit search range
    let section = &text[section_idx..section_end.min(text.len())];

    let min_idx = section.find("min:")?;
    let max_idx = section.find("max:")?;

    let min_str = &section[min_idx + 4..max_idx];
    let max_str = &section[max_idx + 4..];

    let min = parse_float_array(min_str)?;
    let max = parse_float_array(max_str)?;

    Some(BoundingBox { min, max })
}

/// 解析某部件的 `points:` 数组（部件定位偏移）。
fn parse_section_points(text: &str, section_name: &str) -> Option<[f32; 3]> {
    let section_idx = text.find(section_name)?;
    let section_end = section_idx + 400;
    let section = &text[section_idx..section_end.min(text.len())];

    let points_idx = section.find("points:")?;
    let after = &section[points_idx + 7..];
    parse_float_array(after)
}

/// 解析形如 `[x, y, z]` 的浮点数组。
fn parse_float_array(s: &str) -> Option<[f32; 3]> {
    // Extract content between [ and ]
    let start = s.find('[')?;
    let end = s.find(']')?;
    if end <= start { return None; }
    let inner = &s[start+1..end];
    let nums: Vec<f32> = inner.split(',')
        .filter_map(|s| s.trim().parse::<f32>().ok())
        .collect();
    if nums.len() >= 3 {
        Some([nums[0], nums[1], nums[2]])
    } else {
        None
    }
}

