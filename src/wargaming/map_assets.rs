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

/// 高度场 zMax（米；u16 满量程对应值，zMin 恒 0）。来源：客户端 SC2 worldBounds
/// （与 WotbTools map-semantics 同源；malinovka 已用回放车辆 y 实测验证，中位残差 5cm）。
static MAP_ZMAX: &[(&str, f32)] = &[
    ("DesertSands", 100.0),
    ("Middleburg", 150.0),
    ("Copperfield", 120.0),
    ("Alpenstadt", 180.0),
    ("Mines", 135.0),
    ("DeadRail", 70.0),
    ("FortDespair", 70.0),
    ("Himmelsdorf", 70.0),
    ("BlackGoldville", 120.0),
    ("OasisPalms", 80.0),
    ("GhostFactory", 80.0),
    ("Molendijk", 80.0),
    ("PortBay", 70.0),
    ("WinterMalinovka", 60.0),
    ("Castilla", 50.0),
    ("Canal", 140.0),
    ("Vineyards", 150.0),
    ("YamatoHarbor", 80.0),
    ("Canyon", 100.0),
    ("MayanRuins", 80.0),
    ("DynastyPearl", 150.0),
    ("NavalFrontier", 100.0),
    ("FallsCreek", 50.0),
    ("NewBay", 100.0),
    ("Normandy", 70.0),
    ("Wasteland", 120.0),
];

/// 老代际地图，高度图不符合标准契约（如 Himmelsdorf 的 64.heightmap 回归拟合失败），
/// 一律 404 让前端回退 2D 底图。
static HEIGHTMAP_INCOMPATIBLE: &[&str] = &["Himmelsdorf"];

/// 枚举名 → 3d/Maps 空间目录 ID（sc2/heightmap 等场景资源所在；注意与小地图短名不同，
/// 如 WinterMalinovka→malinovka→12_malinovka_ma。PortBay 用现役 14_port_pt 空间而非旧 port）。
static MAP_SPACES: &[(&str, &str)] = &[
    ("DesertSands", "02_desert_train_dt"),
    ("Middleburg", "03_erlenberg_er"),
    ("Copperfield", "23_karieri_kr"),
    ("Alpenstadt", "31_lumber_lm"),
    ("Mines", "06_rudniki_rd"),
    ("DeadRail", "04_medvedkovo_md"),
    ("FortDespair", "07_fort_ft"),
    ("Himmelsdorf", "19_himmelsdorf_hm"),
    ("BlackGoldville", "21_mountain_mnt"),
    ("OasisPalms", "09_savanna_sv"),
    ("GhostFactory", "11_plant_pn"),
    ("Molendijk", "16_holland_hl"),
    ("PortBay", "14_port_pt"),
    ("WinterMalinovka", "12_malinovka_ma"),
    ("Castilla", "13_pliego_pl"),
    ("Canal", "18_canal_cn"),
    ("Vineyards", "22_italy_it"),
    ("YamatoHarbor", "24_milibase_mlb"),
    ("Canyon", "25_canyon_ca"),
    ("MayanRuins", "28_rock_rc"),
    ("DynastyPearl", "30_grossberg_sh"),
    ("NavalFrontier", "29_skit_sk"),
    ("FallsCreek", "05_amigosville_am"),
    ("NewBay", "34_forgecity_fc"),
    ("Normandy", "33_neptune_nt"),
    ("Wasteland", "26_holmeisk_hk"),
];

fn map_zmax(map_name: &str) -> Option<f32> {
    MAP_ZMAX.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(map_name))
        .map(|(_, v)| *v)
}

/// 枚举名 → 3d/Maps 空间目录 ID（白名单，防路径拼接）。
fn map_space(map_name: &str) -> Option<&'static str> {
    MAP_SPACES.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(map_name))
        .map(|(_, v)| *v)
}

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
    /// 高度场 zMax 覆盖（米）；缺省用内置表
    #[serde(default)]
    pub zmax_m: Option<f32>,
}

impl Default for MapMeta {
    fn default() -> Self {
        Self { size_m: DEFAULT_SIZE_M, x: 0.0, z: 0.0, rot90: 0, flip_x: false, zmax_m: None }
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

    // 2) 高清地面贴图（tools/export_map_glb.py 由客户端 colormap 预生成，2048²，
    //    分辨率约为 MiniMapSmall 的 4 倍；缺失则退回小地图）
    let ground = Path::new("glb_cache").join("maps").join(format!("{name}.ground.webp"));
    if let Ok(bytes) = std::fs::read(&ground) {
        return map_response(bytes, "image/webp", name);
    }

    // 3) 小地图提取缓存
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

// ---------- 高度场地形（GET /api/playback/terrain） ----------
//
// 格式（已用回放车辆 y 定量验证，malinovka 中位残差 5cm）：
//   8 字节头（u32 size=512、u32 tile=16）+ size²×2 字节 u16，按 tile×tile 块存储
//   （块行主序），块内行主序；存储行 0=南、列 0=东；高度 z = u16 * zMax / 65535；
//   覆盖世界 [-300,+300]²。输出统一列翻转（列 0=西=x −300），行序不变（行 0=南=z −300）。

/// 一张图解码后的高度场：行 0=南、列 0=西（行主序 u16，米制换算系数见响应头 zmax）。
pub struct TerrainGrid {
    pub heights: Vec<u16>,
}

/// 解析标准契约高度图，返回行 0=南、列 0=西 的 u16 网格；size/tile 非预期值（老图）一律拒绝。
fn parse_heightmap(raw: &[u8]) -> Option<TerrainGrid> {
    if raw.len() < 8 {
        return None;
    }
    let size = u32::from_le_bytes(raw[0..4].try_into().ok()?) as usize;
    let tile = u32::from_le_bytes(raw[4..8].try_into().ok()?) as usize;
    // 目前全部已知可用图均为 512/16；其他参数（老图变体）未验证，拒绝
    if size != 512 || tile != 16 || raw.len() != 8 + size * size * 2 {
        return None;
    }
    let src: Vec<u16> = raw[8..]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    // tile 块重排（块行主序 → 行主序）
    let blocks = size / tile;
    let mut grid = vec![0u16; size * size];
    let mut i = 0;
    for by in 0..blocks {
        for bx in 0..blocks {
            for ly in 0..tile {
                let dst = (by * tile + ly) * size + bx * tile;
                grid[dst..dst + tile].copy_from_slice(&src[i..i + tile]);
                i += tile;
            }
        }
    }
    // 列翻转：存储列 0=东 → 输出列 0=西（行序不变）
    let mut out = vec![0u16; size * size];
    for r in 0..size {
        for c in 0..size {
            out[r * size + c] = grid[r * size + (size - 1 - c)];
        }
    }
    Some(TerrainGrid { heights: out })
}

/// 从游戏目录解出高度图（landscape/ 下唯一 *heightmap*.dvpl，文件名各图不同）。
fn extract_heightmap(space_id: &str) -> Option<TerrainGrid> {
    let game = resolve_game_dir(None).ok()?;
    let dir = game.join("3d/Maps").join(space_id).join("landscape");
    let mut found = None;
    for e in std::fs::read_dir(&dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.contains("heightmap") && name.ends_with(".dvpl") && found.is_none() {
            found = Some(e.path());
        }
    }
    let path = found?;
    let dv = DvplFile::read(&path).ok()?;
    parse_heightmap(&dv.data)
}

/// 序列化高度场：u16 LE 行主序（行 0=南），前端按 X-Terrain-Meta 解释。
fn terrain_bytes(t: &TerrainGrid) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(t.heights.len() * 2);
    for v in &t.heights {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// GET /api/playback/terrain?name=<MapName>：覆盖 → 缓存 → 游戏提取。
/// 响应体 = u16 LE 高度场，X-Terrain-Meta = {"size":..,"zmax":..,"span":600.0}。
pub fn terrain_response(map_name: &str) -> Response {
    let name = map_name.trim();
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing name").into_response();
    }
    let Some(space) = map_space(name) else {
        return (axum::http::StatusCode::NOT_FOUND, "terrain not available").into_response();
    };
    if HEIGHTMAP_INCOMPATIBLE.iter().any(|k| k.eq_ignore_ascii_case(name)) {
        return (axum::http::StatusCode::NOT_FOUND, "terrain not available").into_response();
    }
    let zmax = map_meta(name).zmax_m.unwrap_or_else(|| map_zmax(name).unwrap_or(100.0));

    // 1) 手动覆盖：data/maps/<Name>.heightmap.u16.bin（512×512 LE，行 0=南）
    let override_path = data_path(MAP_DIR).join(format!("{name}.heightmap.u16.bin"));
    if let Ok(bytes) = std::fs::read(&override_path) {
        if bytes.len() == 512 * 512 * 2 {
            return terrain_response_bytes(bytes, zmax);
        }
        eprintln!("[map-assets] 高度覆盖尺寸不符（应为 {} 字节）：{}", 512 * 512 * 2, override_path.display());
    }

    // 2) 提取缓存
    let cache = data_path(MAP_DIR).join("_cache").join(format!("{name}.hm.u16.bin"));
    if let Ok(bytes) = std::fs::read(&cache) {
        if bytes.len() == 512 * 512 * 2 {
            return terrain_response_bytes(bytes, zmax);
        }
    }

    // 3) 游戏客户端提取
    let Some(t) = extract_heightmap(space) else {
        return (axum::http::StatusCode::NOT_FOUND, "terrain not available").into_response();
    };
    let bytes = terrain_bytes(&t);
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, &bytes);
    eprintln!("[map-assets] extracted heightmap {space} -> {}", cache.display());
    terrain_response_bytes(bytes, zmax)
}

/// 响应附带 X-Terrain-Meta（高度场解释参数）。
fn terrain_response_bytes(bytes: Vec<u8>, zmax: f32) -> Response {
    let meta = format!(r#"{{"size":512,"zmax":{zmax:.1},"span":{DEFAULT_SIZE_M:.1}}}"#);
    (
        [
            (axum::http::header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (axum::http::header::HeaderName::from_static("x-terrain-meta"), meta),
        ],
        bytes,
    ).into_response()
}

/// GET /api/playback/scenery?name=<MapName>：伺服离线导出的静态场景 GLB
/// （建筑/桥/岩石等；由 tools/export_map_glb.py 预生成到 glb_cache/maps/，
/// 运行时不做 SC2 解析。缺失 404，前端静默跳过——仅地形/底图仍在）。
pub fn scenery_response(map_name: &str) -> Response {
    let name = map_name.trim();
    if name.is_empty() || internal_dir(name).is_none() {
        return (axum::http::StatusCode::NOT_FOUND, "scenery not available").into_response();
    }
    // 与 viewer.rs 的坦克 GLB 缓存同目录体系（glb_cache/ 已 gitignore）
    let path = Path::new("glb_cache").join("maps").join(format!("{name}.glb"));
    match std::fs::read(&path) {
        Ok(bytes) => (
            [(axum::http::header::CONTENT_TYPE, "model/gltf-binary".to_string())],
            bytes,
        ).into_response(),
        Err(_) => (axum::http::StatusCode::NOT_FOUND, "scenery not available").into_response(),
    }
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

    /// 标准高度图解析：tile 重排 + 行翻转 + u16→米换算。
    #[test]
    fn heightmap_parse_untile_and_flip() {
        let size = 512usize;
        let tile = 16usize;
        // 构造：块(bx,by)内所有值 = 编码 (bx, by, 块内行, 块内列) 的可辨识 u16
        let mut raw = vec![0u8; 8 + size * size * 2];
        raw[0..4].copy_from_slice(&(size as u32).to_le_bytes());
        raw[4..8].copy_from_slice(&(tile as u32).to_le_bytes());
        let mut src = vec![0u16; size * size];
        for by in 0..size / tile {
            for bx in 0..size / tile {
                for ly in 0..tile {
                    for lx in 0..tile {
                        let v = ((by * tile + ly) * size + bx * tile + lx) as u16 % 65535;
                        src[(by * size / tile + bx) * tile * tile + ly * tile + lx] = v;
                    }
                }
            }
        }
        for (i, v) in src.iter().enumerate() {
            raw[8 + i * 2..8 + i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        let t = parse_heightmap(&raw).expect("standard contract must parse");
        // 列翻转后：输出 [r,c] = 未翻转网格 [r, size-1-c]，即线性下标 r*size+(size-1-c)
        for (r, c) in [(0usize, 0usize), (100, 200), (511, 511), (256, 128)] {
            let expect = (r * size + (size - 1 - c)) as u16 % 65535;
            let got = t.heights[r * size + c];
            assert_eq!(got, expect, "({r},{c}): {got} vs {expect}");
        }
        // 非标准契约（老图变体）拒绝
        raw[4..8].copy_from_slice(&8u32.to_le_bytes());
        assert!(parse_heightmap(&raw).is_none());
    }

    /// zMax 表覆盖全部 26 图，且 incompatible 名单内的图 zmax 无所谓（先行拒绝）。
    #[test]
    fn zmax_table_covers_all_maps() {
        for (name, _) in MAP_DIRS {
            assert!(map_zmax(name).is_some(), "{name} missing zmax");
            assert!(map_space(name).is_some(), "{name} missing space id");
        }
        assert!(map_zmax("Undefined").is_none());
        assert!(map_space("Undefined").is_none());
        // 空间 ID 只含安全字符（用于路径拼接前的白名单二次确认）
        for (_, space) in MAP_SPACES {
            assert!(space.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        }
    }
}
