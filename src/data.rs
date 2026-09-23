// 数据路径层：静态数据统一放项目根 `data/` 目录，经 data_path() 访问，避免散落硬编码路径。
// 关键数据源：tanks.pb + models.pb（BlitzKit 坦克数据库/模型定义，运行时直接解析）、
// armor_cache.json / gun_angles.json / tank_cache.json / game_data/（便携装甲模型）。

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
