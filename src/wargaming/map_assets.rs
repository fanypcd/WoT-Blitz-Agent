//! 回放底图资源：从本机 WoTB 客户端提取每张地图的小地图贴图（MiniMapSmall，
//! 标准 WebP 容器 + DVPL 壳），支持 `data/maps/` 手动覆盖与提取缓存。
//!
//! 数据源（本机游戏目录内均已验证）：
//! - 贴图：`Data/Gfx/UI/BattleScreenHUD/minimap/<内部名>/MiniMapSmall[@2x].packed.webp.dvpl`
//!   （DVPL compression_type=0，去掉 20 字节 footer 即完整 WebP）；
//! - 名称映射：`Data/Strings/en.yaml.dvpl` 内 `#maps:<minimap目录>:<space>/<space>.sc2: "<显示名>"`
//!   条目，与 wotbreplay-parser 的 MapId 枚举 Debug 名（去空格后）一一对应，下表即由此生成；
//! - 对齐：底图覆盖世界 [-300,+300]²（600×600 米、世界原点居中、图上边=+z、图右边=+x），
//!   来源为客户端 SC2 场景的 worldBounds（29 图统一 -300..300；WotbTools 项目同款约定，
//!   其 playableBounds 与本机回放轨迹实测范围互证）。3d/Maps/*/blitz/*.mkm 是 1024 格
//!   通行性掩码，与底图跨度无关。个别地图可用 `data/maps/<MapName>.json`
//!   （`{"size_m":..,"x":..,"z":..,"rot90":..,"flip_x":..}`）微调。

use crate::wargaming::dvpl::DvplFile;
use crate::wargaming::game_extract::resolve_game_dir;
use crate::data::data_path;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 覆盖图/标定文件目录（data/maps/），提取缓存在其下 _cache/。
pub const MAP_DIR: &str = "maps";
/// 底图默认边长（米）：客户端 SC2 worldBounds 统一 [-300,+300]，即 600×600 方框。
const DEFAULT_SIZE_M: f32 = 600.0;

/// 解析器 MapId Debug 名 → 游戏内 minimap 目录名（权威对照见模块注释）。
static MAP_DIRS: &[(&str, &str)] = &[
    ("DesertSands", "desert_train"),
    ("Middleburg", "erlenberg"),
    ("Copperfield", "karieri"),
    ("Alpenstadt", "lumber"),
    ("Mines", "rudniki"),
    ("DeadRail", "medvedkovo"),
    ("FortDespair", "fort"),
    ("Himmelsdorf", "himmelsdorf"),
    ("BlackGoldville", "mountain"),
    ("OasisPalms", "savanna"),
    ("GhostFactory", "plant"),
    ("Molendijk", "holland"),
    ("PortBay", "port"),
    ("WinterMalinovka", "malinovka"),
    ("Castilla", "pliego"),
    ("Canal", "canal"),
    ("Vineyards", "italy"),
    ("YamatoHarbor", "milbase"),
    ("Canyon", "canyon"),
    ("MayanRuins", "rock"),
    ("DynastyPearl", "grossberg"),
    ("NavalFrontier", "skit"),
    ("FallsCreek", "amigosville"),
    ("NewBay", "forgecity"),
    ("Normandy", "neptune"),
    ("Wasteland", "holmeisk"),
];

/// 底图铺设参数（前端按此放置平面；来自 data/maps/<MapName>.json，缺省全默认）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapMeta {
    /// 底图边长（米）
    pub size_m: f32,
    /// 平面中心偏移（场景系，米；默认原点）
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub z: f32,
    /// 顺时针 90° 旋转次数（0-3）
    #[serde(default)]
    pub rot90: u32,
    /// 纹理水平镜像（方向校准用）
    #[serde(default)]
    pub flip_x: bool,
}

impl Default for MapMeta {
    fn default() -> Self {
        Self { size_m: DEFAULT_SIZE_M, x: 0.0, z: 0.0, rot90: 0, flip_x: false }
    }
}

/// 只经白名单表转换，不做路径拼接（防穿越）；表外名字返回 None。
pub fn internal_dir(map_name: &str) -> Option<&'static str> {
    MAP_DIRS.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(map_name))
        .map(|(_, v)| *v)
}

fn meta_path(map_name: &str) -> Option<std::path::PathBuf> {
    let ok = !map_name.is_empty()
        && map_name.len() <= 40
        && map_name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then(|| data_path(MAP_DIR).join(format!("{map_name}.json")))
}

/// 读取某图的铺设参数：data/maps/<MapName>.json 存在则用之，否则全默认。
pub fn map_meta(map_name: &str) -> MapMeta {
    meta_path(map_name)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// GET /api/playback/map?name=<MapName>：优先手动覆盖图，其次提取缓存，最后游戏提取。
pub fn map_image_response(map_name: &str) -> Response {
    let name = map_name.trim();
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing name").into_response();
    }

    // 1) 手动覆盖：data/maps/<MapName>.{webp,png,jpg,jpeg}
    for (ext, ct) in [("webp", "image/webp"), ("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg")] {
        let p = data_path(MAP_DIR).join(format!("{name}.{ext}"));
        if let Ok(bytes) = std::fs::read(&p) {
            return map_response(bytes, ct, name);
        }
    }

    let Some(dir) = internal_dir(name) else {
        return (axum::http::StatusCode::NOT_FOUND, "map image not available").into_response();
    };

    // 2) 提取缓存
    let cache = data_path(MAP_DIR).join("_cache").join(format!("{name}.webp"));
    if let Ok(bytes) = std::fs::read(&cache) {
        return map_response(bytes, "image/webp", name);
    }

    // 3) 游戏客户端提取
    let Some(bytes) = extract_minimap(dir) else {
        return (axum::http::StatusCode::NOT_FOUND, "map image not available").into_response();
    };
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, &bytes);
    eprintln!("[map-assets] extracted {dir} -> {}", cache.display());
    map_response(bytes, "image/webp", name)
}

/// 响应附带 X-Map-Meta（铺设参数 JSON），前端据此放置底图平面。
fn map_response(bytes: Vec<u8>, content_type: &str, map_name: &str) -> Response {
    let meta = serde_json::to_string(&map_meta(map_name)).unwrap_or_default();
    (
        [
            (axum::http::header::CONTENT_TYPE, content_type.to_string()),
            (axum::http::header::HeaderName::from_static("x-map-meta"), meta),
        ],
        bytes,
    ).into_response()
}

/// 从游戏目录解出 MiniMapSmall WebP 字节（优先 @2x，退基础版）。
fn extract_minimap(internal: &str) -> Option<Vec<u8>> {
    let game = resolve_game_dir(None).ok()?;
    let base = Path::new("Gfx/UI/BattleScreenHUD/minimap").join(internal);
    for file in ["MiniMapSmall@2x.packed.webp.dvpl", "MiniMapSmall.packed.webp.dvpl"] {
        let p = game.join(&base).join(file);
        if !p.exists() {
            continue;
        }
        match DvplFile::read(&p) {
            Ok(dv) => return Some(dv.data),
            Err(e) => eprintln!("[map-assets] DVPL 解码失败 {}: {e:?}", p.display()),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 26 张地图全部能查到内部目录名，且不含路径非法字符。
    #[test]
    fn all_maps_have_internal_dir() {
        for (name, dir) in MAP_DIRS {
            assert_eq!(internal_dir(name), Some(*dir));
            assert!(dir.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()));
        }
    }

    /// 表外名字一律拒绝（不拼路径）。
    #[test]
    fn unknown_name_rejected() {
        assert!(internal_dir("Undefined").is_none());
        assert!(internal_dir("../etc").is_none());
        assert!(meta_path("../evil").is_none());
    }
}
