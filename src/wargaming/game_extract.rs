use crate::wargaming::dvpl::{ArmorModel, CollisionData, DvplFile};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

// =====================================================================
//  游戏数据批量提取：从本机 WoTB 安装目录的 DVPL 文件解析每辆坦克的
//  装甲模型与碰撞数据，导出为 game_data/{tank_id}.json（其他设备免装游戏）。
// =====================================================================

/// 常见游戏安装目录（WSL / Windows 各一份，供自动探测）。
pub const DEFAULT_GAME_DIRS: &[&str] = &[
    "/mnt/d/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
    "D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
    "/mnt/c/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
    "C:/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
];

/// 坦克条目（只取 tank_id/model_name/nation，源自 tanks.pb 唯一数据源）。
#[derive(Debug, Deserialize)]
struct PbTankEntry {
    tank_id: u32,
    #[serde(default)]
    model_name: Option<String>,
    #[serde(default)]
    nation: Option<String>,
}

/// 一辆坦克的提取结果（可移植的游戏数据替代品）。
#[derive(Debug, Serialize, Deserialize)]
pub struct TankGameData {
    pub tank_id: u32,
    pub dev_name: String,
    pub nation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor_model: Option<ArmorModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision: Option<CollisionData>,
}

/// 读取之前提取好的某辆坦克数据；3D 查看器优先用便携数据，缺失时才回退游戏目录。
pub fn load_game_data(tank_id: u32, dir: &Path) -> Option<TankGameData> {
    let path = dir.join(format!("{}.json", tank_id));
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// 从 item_defs XML 提取 `<hullPosition>x y z</hullPosition>`（车体相对底盘位置）。
fn parse_hull_position(xml: &str) -> Option<[f32; 3]> {
    let start = xml.find("<hullPosition>")? + "<hullPosition>".len();
    let end = xml[start..].find("</hullPosition>")? + start;
    let nums: Vec<f32> = xml[start..end]
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    if nums.len() == 3 { Some([nums[0], nums[1], nums[2]]) } else { None }
}

/// 确定游戏数据目录：优先用显式路径，否则在常见目录里自动探测。
pub fn resolve_game_dir(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(d) = explicit {
        if d.exists() {
            return Ok(d.to_path_buf());
        }
        anyhow::bail!("Game directory not found: {}", d.display());
    }
    for d in DEFAULT_GAME_DIRS {
        let p = Path::new(d);
        if p.exists() {
            return Ok(p.to_path_buf());
        }
    }
    anyhow::bail!("No game data directory found. Pass --game-dir explicitly.")
}

/// 归一化用于文件名比较：去掉所有非字母数字并转小写（如 "GB91_Super_Conqueror" → "gb91superconqueror"）。
fn norm_file_name(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}
/// 在民族目录内解析某坦克的车辆 DVPL 文件。tanks.pb 的 dev_name 与游戏文件名常
/// 不一致（如 "super-conqueror" → "GB91_Super_Conqueror"），匹配优先级：
/// 1) 精确/归一化相等；2) 文件名归一化后【包含】dev_name（最短优先，通常是基准车）；
/// 3) dev_name 以文件名归一化为【前缀或后缀】（最长优先）。反向匹配必须锚定词缀边界、
/// 不允许纯内嵌子串——否则 "xm66f" 会被 "M6"（内嵌子串且更短）抢走匹配，装上别人的装甲。
fn resolve_vehicle_file(
    game_dir: &Path,
    rel: &str,
    nation: &str,
    dev_name: &str,
    ext: &str,
) -> Option<PathBuf> {
    let dir = game_dir.join(rel).join(nation);
    let exact = dir.join(format!("{}{}", dev_name, ext));
    if exact.exists() {
        return Some(exact);
    }
    let target = norm_file_name(dev_name);
    if target.is_empty() { return None; }
    let entries = std::fs::read_dir(&dir).ok()?;
    // (rank, tie, path)：rank 越小越优先；tie 在 rank 内部决定次序
    let mut cands: Vec<(u8, usize, PathBuf)> = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.ends_with(ext) { continue; }
        let base = &name[..name.len() - ext.len()];
        // 排除 tutorial/bot 等衍生变体
        let base_lower = base.to_lowercase();
        if base_lower.contains("tutorial") || base_lower.contains("bot") {
            continue;
        }
        let norm = norm_file_name(base);
        let (rank, tie) = if norm == target {
            (0u8, 0usize)
        } else if norm.contains(&target) {
            (1, base.len())              // 正向：最短（最接近 dev_name）优先
        } else if target.starts_with(&norm) || target.ends_with(&norm) {
            (2, usize::MAX - norm.len()) // 反向：最长（最具体）优先
        } else {
            continue;
        };
        cands.push((rank, tie, e.path()));
    }
    cands.sort_by_key(|&(rank, tie, _)| (rank, tie));
    cands.into_iter().next().map(|(_, _, p)| p)
}

/// 批量提取全部坦克的装甲/碰撞数据到 `game_data/`（`extract-game` 命令）。
pub fn extract_all(
    game_dir: Option<&Path>,
    output_dir: &Path,
    force: bool,
) -> Result<ExtractStats> {
    let game_dir = resolve_game_dir(game_dir)?;
    let tanks: Vec<PbTankEntry> = crate::wargaming::blitzkit::load_tanks()
        .values().map(|t| PbTankEntry {
            tank_id: t.tank_id,
            model_name: if t.dev_name.is_empty() { None } else { Some(t.dev_name.clone()) },
            nation: if t.nation.is_empty() { None } else { Some(t.nation.clone()) },
        }).collect();

    std::fs::create_dir_all(output_dir)?;

    let mut stats = ExtractStats::default();
    let total = tanks.len();

    for (i, tank) in tanks.iter().enumerate() {
        let (Some(model_name), Some(nation)) = (tank.model_name.as_deref(), tank.nation.as_deref())
        else {
            stats.skipped_no_name += 1;
            continue;
        };

        let out_path = output_dir.join(format!("{}.json", tank.tank_id));
        if out_path.exists() && !force {
            stats.cached += 1;
            continue;
        }

        // dev_name（如 "super-conqueror"）与游戏实际文件名（如 "GB91_Super_Conqueror"）
        // 不一致，需按归一化文件名模糊匹配。
        let xml_path = resolve_vehicle_file(&game_dir, "XML/item_defs/vehicles", nation, model_name, ".xml.dvpl");
        let yaml_path = resolve_vehicle_file(&game_dir, "3d/Tanks/Parameters", nation, model_name, ".yaml.dvpl");

        if xml_path.is_none() || yaml_path.is_none() {
            stats.missing_files += 1;
            continue;
        }
        let xml_path = xml_path.unwrap();
        let yaml_path = yaml_path.unwrap();

        // 解码 DVPL 并解析成结构化数据（失败则视为 null，不算致命）
        let xml_text = DvplFile::read(&xml_path)
            .ok()
            .map(|d| String::from_utf8_lossy(&d.data).into_owned());
        let armor_model = xml_text.as_ref().and_then(|t| ArmorModel::parse_from_xml(t));
        // hullPosition 在 item_defs XML 里（车体相对底盘的权威位置），补进 collision 数据。
        let mut collision = DvplFile::read(&yaml_path)
            .ok()
            .and_then(|d| CollisionData::parse_from_yaml(&String::from_utf8_lossy(&d.data)));
        if let (Some(t), Some(ref mut c)) = (&xml_text, collision.as_mut()) {
            c.hull_position = parse_hull_position(t);
        }

        if armor_model.is_none() && collision.is_none() {
            stats.parse_failed += 1;
            continue;
        }

        let entry = TankGameData {
            tank_id: tank.tank_id,
            dev_name: model_name.to_string(),
            nation: nation.to_string(),
            armor_model,
            collision,
        };
        let json = serde_json::to_string(&entry)?;
        if std::fs::write(&out_path, json).is_ok() {
            stats.extracted += 1;
        } else {
            stats.write_failed += 1;
        }

        if (i + 1) % 50 == 0 || i + 1 == total {
            print!("\r  [{}/{}] extracted={} cached={} missing={} failed={}",
                i + 1, total, stats.extracted, stats.cached, stats.missing_files, stats.parse_failed + stats.write_failed);
            let _ = std::io::stdout().flush();
        }
    }
    println!();
    Ok(stats)
}

/// 提取过程的统计计数（用于进度显示与结果汇总）。
#[derive(Default)]
pub struct ExtractStats {
    pub extracted: usize,
    pub cached: usize,
    pub skipped_no_name: usize,
    pub missing_files: usize,
    pub parse_failed: usize,
    pub write_failed: usize,
}

/// 读取本机游戏客户端版本号（游戏目录根的 version.txt.dvpl，内容形如
/// "10.30.0.903 release/11.20.0 WOTB_Win7-"，优先取 release/X.Y.Z 段）。
/// 读不到或解码失败时返回 None。
pub fn game_version(game_dir: &Path) -> Option<String> {
    let dvpl = DvplFile::read(&game_dir.join("version.txt.dvpl")).ok()?;
    let content = String::from_utf8_lossy(&dvpl.data);
    let content = content.trim();
    if content.is_empty() {
        return None;
    }
    content
        .split_whitespace()
        .find(|t| t.starts_with("release/"))
        .map(str::to_string)
        .or_else(|| Some(content.to_string()))
}

/// game_data/ 中已不在当前 tanks.pb 里的孤立 {tank_id}.json（版本更新后坦克被移除的情形，
/// 调用方只报告不删除）。注意：会触发 blitzkit::load_tanks 的进程内缓存加载。
pub fn orphan_game_data_ids(output_dir: &Path) -> Vec<u32> {
    let tanks = crate::wargaming::blitzkit::load_tanks();
    let mut orphans = Vec::new();
    let Ok(entries) = std::fs::read_dir(output_dir) else {
        return orphans;
    };
    for e in entries.flatten() {
        let file_name = e.file_name();
        let Some(name) = file_name.to_str() else { continue };
        let Some(id) = name.strip_suffix(".json").and_then(|s| s.parse::<u32>().ok()) else { continue };
        if !tanks.contains_key(&id) {
            orphans.push(id);
        }
    }
    orphans.sort_unstable();
    orphans
}

/// mtime 兜底启发式（清单缺失时判断游戏是否在数据提取之后更新过）：
/// version.txt.dvpl 的修改时间晚于 game_data/ 里最新的 json → 视为已更新。
/// game_data 为空/不可读时返回 false（增量提取本来就会补齐全部缺失文件）。
pub fn game_dir_newer_than_data(game_dir: &Path, game_data_dir: &Path) -> bool {
    let Ok(vt_mtime) = game_dir.join("version.txt.dvpl").metadata().and_then(|m| m.modified())
    else {
        return false;
    };
    let mut newest_data: Option<std::time::SystemTime> = None;
    for e in std::fs::read_dir(game_data_dir).into_iter().flatten().flatten() {
        let Ok(m) = e.metadata().and_then(|m| m.modified()) else { continue };
        if newest_data.map(|n| m > n).unwrap_or(true) {
            newest_data = Some(m);
        }
    }
    newest_data.is_some_and(|n| vt_mtime > n)
}

