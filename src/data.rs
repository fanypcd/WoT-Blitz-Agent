// =====================================================================
//  数据路径层
//  统一管理项目的数据文件访问路径。所有静态数据（坦克缓存、BlitzKit
//  元数据、装甲/炮塔数据、便携装甲文件等）都放在项目根的 `data/` 目录下，
//  通过本模块提供的 `data_path()` 访问，避免代码里散落硬编码路径。
//
//  目录示意（关键：tanks.pb + models.pb 是 BlitzKit 数据源，运行时直接解析）：
//    data/
//    ├── tanks.pb            BlitzKit 坦克数据库（元数据/武器/装填, 运行时解析, 723 辆）
//    ├── models.pb           BlitzKit 模型定义（炮塔/主炮→gun/turret_0X 节点映射, 运行时解析, 723 辆）
//    ├── armor_cache.json    军用装甲板厚度（BlitzKit models.pb）
//    ├── gun_angles.json     炮管俯仰角（BlitzKit models.pb）
//    ├── tank_cache.json     坦克完整数据缓存（由 tanks.pb 构建, 723 辆）
//    └── game_data/          便携装甲模型/碰撞（DVPL 提取, 723 辆）
// =====================================================================

use std::path::{Path, PathBuf};

/// 数据根目录名（相对工作目录）。
pub const DATA_DIR: &str = "data";

/// 返回 `data/{name}` 的路径（`name` 可含子路径，如 `game_data/7169.json`）。
pub fn data_path(name: &str) -> PathBuf {
    Path::new(DATA_DIR).join(name)
}

/// 返回数据根目录。
pub fn data_dir() -> PathBuf {
    PathBuf::from(DATA_DIR)
}
