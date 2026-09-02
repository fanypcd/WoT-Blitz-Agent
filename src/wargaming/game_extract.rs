use crate::wargaming::dvpl::{ArmorModel, CollisionData, DvplFile};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

// =====================================================================
//  游戏数据批量提取
//  从本机 WoTB 游戏安装目录的 DVPL 文件里，把每辆坦克的装甲模型与
//  碰撞数据解析出来，导出为 `game_data/{tank_id}.json`，实现"可移植"：
//  其他设备无需安装游戏即可用这些数据做分析。
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

/// 读取之前提取好的某辆坦克数据（`game_data/{tank_id}.json`）。
///
/// 3D 查看器优先用这份便携数据，缺失时才回退到游戏安装目录。
pub fn load_game_data(tank_id: u32, dir: &Path) -> Option<TankGameData> {
    let path = dir.join(format!("{}.json", tank_id));
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
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

/// 批量提取全部坦克的装甲/碰撞数据到 `game_data/`（`extract-game` 命令）。
pub fn extract_all(
    game_dir: Option<&Path>,
    output_dir: &Path,
    force: bool,
) -> Result<ExtractStats> {
    let game_dir = resolve_game_dir(game_dir)?;
    // 运行时解析 tanks.pb（唯一数据源），拿到各坦克的 dev_name/nation 用于定位游戏文件。
    let tanks: Vec<PbTankEntry> = crate::wargaming::blitzkit::load_tanks()
        .into_values().map(|t| PbTankEntry {
            tank_id: t.tank_id,
            model_name: if t.dev_name.is_empty() { None } else { Some(t.dev_name) },
            nation: if t.nation.is_empty() { None } else { Some(t.nation) },
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

        // 该坦克在游戏目录里的 XML（装甲）+ YAML（碰撞）DVPL 文件
        let xml_path = game_dir.join(format!("XML/item_defs/vehicles/{}/{}.xml.dvpl", nation, model_name));
        let yaml_path = game_dir.join(format!("3d/Tanks/Parameters/{}/{}.yaml.dvpl", nation, model_name));

        if !xml_path.exists() || !yaml_path.exists() {
            stats.missing_files += 1;
            continue;
        }

        // 解码 DVPL 并解析成结构化数据（失败则视为 null，不算致命）
        let armor_model = DvplFile::read(&xml_path)
            .ok()
            .and_then(|d| ArmorModel::parse_from_xml(&String::from_utf8_lossy(&d.data)));
        let collision = DvplFile::read(&yaml_path)
            .ok()
            .and_then(|d| CollisionData::parse_from_yaml(&String::from_utf8_lossy(&d.data)));

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
