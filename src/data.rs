// 数据路径层：全部运行时数据统一放 `{base_dir}/data/`，经 data_path()/cache_path() 访问，
// 避免散落硬编码路径。关键数据源：tanks.pb + models.pb（BlitzKit 坦克数据库/模型定义，
// 运行时直接解析）、tank_cache.json、game_data/（便携装甲模型/碰撞盒）、
// cache/（运行时缓存：坦克 GLB、地图资产、封面图、地形高度场、截图）。
//
// base_dir：不设置 = 当前目录（CLI/Web 默认语义）；宿主须在任何文件访问发生前
// set_base_dir(私有目录)，此后 data/ 下所有相对路径自动落到私有目录。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 数据根目录名（相对运行根目录）。
pub const DATA_DIR: &str = "data";

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 设置运行根目录（仅首次生效，重复调用返回 false）。
/// 必须在任何文件访问发生前调用。
pub fn set_base_dir(dir: PathBuf) -> bool {
    BASE_DIR.set(dir).is_ok()
}

/// 运行根目录（未设置时为当前目录 `.`，桌面/CLI 语义不变）。
pub fn base_dir() -> &'static Path {
    BASE_DIR
        .get()
        .map(|p| p.as_path())
        .unwrap_or(Path::new("."))
}

/// 返回 `{base}/data/{name}` 的路径（`name` 可含子路径，如 `game_data/7169.json`）。
pub fn data_path(name: &str) -> PathBuf {
    base_dir().join(DATA_DIR).join(name)
}

/// 返回数据根目录 `{base}/data`。
pub fn data_dir() -> PathBuf {
    base_dir().join(DATA_DIR)
}

/// 返回运行根目录下不落在 data/ 内的资产路径（如快照目录等宿主级位置）。
pub fn app_path(rel: &str) -> PathBuf {
    base_dir().join(rel)
}

/// 运行时缓存目录名（data/ 下，整体 gitignore；模型 GLB/地图资产/封面图/地形/截图）。
pub const CACHE_DIR: &str = "cache";

/// 返回 `{base}/data/cache/{rel}` 的路径（坦克 GLB、地图资产、封面图、地形高度场、截图）。
pub fn cache_path(rel: &str) -> PathBuf {
    data_path(CACHE_DIR).join(rel)
}

/// 外部内置资产读取钩子（由宿主入口注入，用于直读打包内资产；不注入恒为 None）。
/// 大资产（GLB/地形）不落盘、按需直读。
type EmbeddedAssetReader = fn(&str) -> Option<Vec<u8>>;
static EMBEDDED_ASSET_READER: OnceLock<EmbeddedAssetReader> = OnceLock::new();

/// 注入内置资产读取函数（仅首次生效）。必须在任何资产访问前调用。
pub fn set_embedded_asset_reader(f: EmbeddedAssetReader) -> bool {
    EMBEDDED_ASSET_READER.set(f).is_ok()
}

/// 读取 APK 内置资产（未注入或不存在的路径返回 None）。
pub fn read_embedded(rel: &str) -> Option<Vec<u8>> {
    EMBEDDED_ASSET_READER.get().and_then(|f| f(rel))
}

/// 共享资产读取：磁盘优先，APK 内置资产兜底（用于只读大资产，如 data/cache/maps 地图资产/地形）。
pub fn read_shareable(rel: &str) -> Option<Vec<u8>> {
    std::fs::read(app_path(rel))
        .ok()
        .or_else(|| read_embedded(rel))
}
