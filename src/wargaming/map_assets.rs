//! 回放底图资源：与客户端一致，从游戏注册表解析地图并提取底图/地形/场景。
//!
//! 数据全部来自本机 WoTB 客户端（Data/，DVPL 壳）：
//! - 注册表：`Data/maps.yaml`（回放数字 id → localName = "space/space.sc2"）联查
//!   `Data/Strings/en.yaml` 的 `"#maps:<dir>:<space>/<space>.sc2": "<显示名>"` 条目，
//!   得到 id → {键名, space 目录, 本地化显示名}。这条链与客户端
//!   处理 arenaTypeID 完全同源（battle_results 的 mode_map_id 低 16 位即 maps.yaml
//!   的 id 字段）；wotbreplay-parser 的 MapId 枚举个别判别值与客户端数据不一致
//!   （Alpenstadt/FallsCreek 互换），因此一律以数字 id 解析，不信任枚举名；
//! - 底图：`data/cache/maps/<space>.ground.webp`（离线导出的 colormap 高清地面；
//!   全图已导出，缺失即 404）；
//! - 地形：`3d/Maps/<space>/landscape/*heightmap*.dvpl`（8 字节头 + 512² u16）；
//!   高度尺度 zmax 来自 `data/cache/maps/<space>.json` sidecar 的 Landscape 世界
//!   包围盒（tools/export_map_glb.py 按客户端数据写出），sidecar 缺失则不伺服；
//! - 场景 GLB：`data/cache/maps/<space>.glb`，同由导出器预生成。
//!
//! 对齐：底图覆盖世界 [-300,+300]²（600×600 米、原点居中、图上边=+z、图右边=+x）。
//! 个别地图可用 `data/maps/<key>.json`（`{"size_m":..,"x":..,"z":..,"rot90":..}`）微调。

use crate::data::{cache_path, data_path};
use crate::wargaming::dvpl::DvplFile;
use crate::wargaming::game_extract::resolve_game_dir;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// 底图默认边长（米）：客户端 SC2 Landscape worldBounds 统一 [-300,+300]。
const DEFAULT_SIZE_M: f32 = 600.0;

/// 一张地图的客户端注册信息（maps.yaml ∩ en.yaml）。
#[derive(Debug, Clone)]
pub struct MapEntry {
    /// 回放数字地图 id（battle_results.mode_map_id 低 16 位 = maps.yaml 的 id）
    pub map_id: u32,
    /// maps.yaml 键名（如 "malinovka"）
    pub key: String,
    /// localName："12_malinovka_ma/12_malinovka_ma.sc2"
    pub sc2: String,
    /// 3d/Maps 下的 space 目录（sc2 路径首段）
    pub space: String,
    /// 客户端本地化显示名（如 "Winter Malinovka"）
    pub display: String,
    /// 小地图目录名（en.yaml `#maps:<dir>:` 的 dir，即 Gfx/UI/BattleScreenHUD/minimap/ 子目录）
    pub minimap_dir: String,
}

impl MapEntry {
    /// space 目录只允许安全字符（用于路径拼接前的白名单二次确认）。
    fn is_safe(&self) -> bool {
        !self.space.is_empty()
            && self.space.len() <= 40
            && self
                .space
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }
}

static REGISTRY: OnceLock<Vec<MapEntry>> = OnceLock::new();

/// 解析 maps.yaml：`    <key>:` 段内的 `        id:` / `        localName:`。
fn parse_maps_yaml(text: &str) -> Vec<(u32, String, String)> {
    let mut out = Vec::new();
    let mut key: Option<&str> = None;
    let mut id: Option<u32> = None;
    let mut local: Option<String> = None;
    let flush = |key: &mut Option<&str>,
                 id: &mut Option<u32>,
                 local: &mut Option<String>,
                 out: &mut Vec<(u32, String, String)>| {
        if let (Some(k), Some(i), Some(l)) = (*key, *id, local.take()) {
            out.push((i, k.to_string(), l));
        }
        *id = None;
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_suffix(':') {
            let trimmed = rest.trim_start();
            let indent = rest.len() - trimmed.len();
            if indent == 4
                && !trimmed.is_empty()
                && trimmed
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                flush(&mut key, &mut id, &mut local, &mut out);
                key = Some(trimmed);
                continue;
            }
        }
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("id:") {
            if let Ok(v) = rest.trim().parse::<u32>() {
                id = Some(v);
            }
        } else if let Some(rest) = t.strip_prefix("localName:") {
            let v = rest.trim().trim_matches('"');
            if !v.is_empty() {
                local = Some(v.to_string());
            }
        }
    }
    flush(&mut key, &mut id, &mut local, &mut out);
    out
}

/// en.yaml `#maps:` 条目：sc2 路径 → [(minimap 目录, 显示名)]（出生点变体多条）。
fn parse_en_yaml_maps(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("\"#maps:") {
        let after = &rest[pos + 1..];
        let Some(key_end) = after.find("\":") else {
            break;
        };
        let key = &after[..key_end]; // "#maps:<dir>:<path>"
        let Some(value_start) = after[key_end..].find(": \"") else {
            break;
        };
        let value = &after[key_end + value_start + 3..];
        let Some(value_end) = value.find('"') else {
            break;
        };
        let mut parts = key.splitn(3, ':');
        let dir = parts.nth(1).unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        out.push((path, dir, value[..value_end].to_string()));
        rest = &after[key_end + value_start + 3 + value_end..];
    }
    out
}

fn load_registry() -> Vec<MapEntry> {
    let Ok(game) = resolve_game_dir(None) else {
        return Vec::new();
    };
    let Ok(maps_dvpl) = DvplFile::read(&game.join("maps.yaml.dvpl")) else {
        return Vec::new();
    };
    let Ok(en_dvpl) = DvplFile::read(&game.join("Strings").join("en.yaml.dvpl")) else {
        return Vec::new();
    };
    let maps_text = String::from_utf8_lossy(&maps_dvpl.data);
    let en_text = String::from_utf8_lossy(&en_dvpl.data);
    let en_entries = parse_en_yaml_maps(&en_text);

    let mut out = Vec::new();
    for (id, key, sc2) in parse_maps_yaml(&maps_text) {
        // 显示名/小地图目录：优先 dir == maps.yaml 键的基础变体，否则取首条
        let mut pick: Option<String> = None;
        let mut pick_dir: Option<String> = None;
        for (path, dir, display) in &en_entries {
            if path != &sc2 {
                continue;
            }
            let (base, base_dir) = (pick.take(), pick_dir.take());
            match base {
                Some(s) => {
                    if dir == &key {
                        pick = Some(display.clone());
                        pick_dir = Some(dir.clone());
                    } else {
                        pick = Some(s);
                        pick_dir = base_dir;
                    }
                }
                None => {
                    pick = Some(display.clone());
                    pick_dir = Some(dir.clone());
                }
            }
        }
        let entry = MapEntry {
            map_id: id,
            key,
            sc2: sc2.clone(),
            space: sc2.split('/').next().unwrap_or_default().to_string(),
            display: pick.unwrap_or_default(),
            minimap_dir: pick_dir.unwrap_or_default(),
        };
        if entry.is_safe() {
            out.push(entry);
        }
    }
    out
}

/// 全量注册表（游戏目录不可用时为空表）。
pub fn registry() -> &'static [MapEntry] {
    REGISTRY.get_or_init(load_registry)
}

/// 注册表的 JSON 投影（`dump-map-index` CLI / 资产打包器消费）：
/// 数字 id → space/key/display/minimap_dir，静态资产面据此建 index.json。
pub fn dump_map_index() -> serde_json::Value {
    serde_json::Value::Array(
        registry()
            .iter()
            .map(|e| {
                serde_json::json!({
                    "map_id": e.map_id,
                    "key": e.key,
                    "space": e.space,
                    "display": e.display,
                    "minimap_dir": e.minimap_dir,
                })
            })
            .collect(),
    )
}

/// 解析地图标识：纯数字 = 回放数字 id；否则显示名/键名（去分隔符归一，大小写不敏感）。
pub fn resolve_map(param: &str) -> Option<&'static MapEntry> {
    let s = param.trim();
    if s.is_empty() {
        return None;
    }
    let reg = registry();
    if let Ok(id) = s.parse::<u32>() {
        return reg.iter().find(|e| e.map_id == id);
    }
    let norm = |x: &str| -> String {
        x.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let want = norm(s);
    reg.iter()
        .find(|e| norm(&e.display) == want)
        .or_else(|| reg.iter().find(|e| norm(&e.key) == want))
}

/// 回放数字 id → 客户端显示名（用于界面展示，修正解析器枚举名与客户端的不一致）。
pub fn display_name(map_id: u32) -> Option<&'static str> {
    registry()
        .iter()
        .find(|e| e.map_id == map_id)
        .map(|e| e.display.as_str())
}

/// 底图铺设参数（前端按此放置平面；data/maps/<key>.json 缺省全默认）。
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
    /// 高度场 zMax 覆盖（米）；缺省用 sidecar（data/cache/maps/<space>.json）
    #[serde(default)]
    pub zmax_m: Option<f32>,
    /// 游戏内实际战场边界（米，回放坐标系；export_map_glb 从场景
    /// MapBorderComponent.mbc.rect 提取）。None = 该图未导出/无组件，消费端回退 worldBounds。
    #[serde(default, rename = "playableBounds")]
    pub playable_bounds: Option<PlayableBounds>,
}

/// 游戏内实际战场边界（回放坐标 x/z 系，米）：比真实地图 worldBounds(±300) 小。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PlayableBounds {
    #[serde(rename = "xMin")]
    pub x_min: f32,
    #[serde(rename = "yMin")]
    pub y_min: f32,
    #[serde(rename = "xMax")]
    pub x_max: f32,
    #[serde(rename = "yMax")]
    pub y_max: f32,
}

impl Default for MapMeta {
    fn default() -> Self {
        Self {
            size_m: DEFAULT_SIZE_M,
            x: 0.0,
            z: 0.0,
            rot90: 0,
            flip_x: false,
            zmax_m: None,
            playable_bounds: None,
        }
    }
}

fn meta_path(map_name: &str) -> Option<std::path::PathBuf> {
    let ok = !map_name.is_empty()
        && map_name.len() <= 40
        && map_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then(|| data_path(&format!("maps/{map_name}.json")))
}

/// 读取某图的铺设参数：data/maps/<key>.json 存在则用之，否则全默认。
pub fn map_meta(entry: &MapEntry) -> MapMeta {
    meta_path(&entry.key)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// GET /api/playback/map?name=<显示名|键>|?id=<数字>：优先手动覆盖图，
/// 其次导出的高清地面贴图，最后小地图提取。
pub fn map_image_response(map_param: &str) -> Response {
    let name = map_param.trim();
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing map").into_response();
    }
    let entry = resolve_map(name);

    // 1) 手动覆盖：data/maps/<参数原样|key>.{webp,png,jpg,jpeg}
    let mut override_stems: Vec<String> = vec![name.to_string()];
    if let Some(e) = entry {
        override_stems.push(e.key.clone());
    }
    for stem in &override_stems {
        for (ext, ct) in [
            ("webp", "image/webp"),
            ("png", "image/png"),
            ("jpg", "image/jpeg"),
            ("jpeg", "image/jpeg"),
        ] {
            if let Some(path) = meta_path(stem).map(|p| p.with_extension(ext)) {
                if let Ok(bytes) = std::fs::read(&path) {
                    return map_response(bytes, ct, entry);
                }
            }
        }
    }

    let Some(entry) = entry else {
        return (axum::http::StatusCode::NOT_FOUND, "map image not available").into_response();
    };

    // 2) 高清地面贴图（客户端 colormap 离线导出，2048²，分辨率约为小地图 4 倍）
    if let Some(bytes) =
        crate::data::read_shareable(&format!("data/cache/maps/{}.ground.webp", entry.space))
    {
        return map_response(bytes, "image/webp", Some(entry));
    }

    (axum::http::StatusCode::NOT_FOUND, "map image not available").into_response()
}

// ---------- 小地图底图（GET /api/playback/map?...&res=mini，低画质档地面） ----------
//
// 客户端小地图：Data/Gfx/UI/BattleScreenHUD/minimap/<dir>/MiniMapSmall[@2x].packed.webp.dvpl，
// DVPL 载荷原样即 webp 文件；与高清底图同覆盖（600m 方框、原点居中，共用 X-Map-Meta）。

/// 小地图 DVPL 候选（优先 @2x 高清版，退普通版）。
const MINIMAP_DVPL: [&str; 2] = [
    "MiniMapSmall@2x.packed.webp.dvpl",
    "MiniMapSmall.packed.webp.dvpl",
];

/// 低画质档小地图底图：提取缓存 → 客户端随取随解 → 高清底图兜底。
pub fn map_minimap_response(map_param: &str) -> Response {
    let name = map_param.trim();
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing map").into_response();
    }
    let entry = resolve_map(name);

    // 1) 提取缓存：data/cache/maps/<space>.minimap.webp
    if let Some(entry) = entry {
        if let Some(bytes) =
            crate::data::read_shareable(&format!("data/cache/maps/{}.minimap.webp", entry.space))
        {
            return map_response(bytes, "image/webp", Some(entry));
        }
    }

    // 2) 客户端提取（与高度场同模式：客户端在场即覆盖全部注册表地图），解出后落缓存
    if let Some(entry) = entry {
        if let Some(bytes) = extract_minimap(entry) {
            let path = cache_path(&format!("maps/{}.minimap.webp", entry.space));
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, &bytes);
            return map_response(bytes, "image/webp", Some(entry));
        }
    }

    // 3) 兜底：高清烘焙底图（离线导出覆盖图）
    map_image_response(name)
}

/// 从游戏目录解出小地图 webp。目录候选：en.yaml 解析的 minimap 目录 → maps.yaml 键名
/// （en.yaml 无 `#maps:` 条目的新图，其目录与键名同名）；目录名限
/// 小写字母/数字/下划线（路径安全，与 [`MapEntry::is_safe`] 同规）。
fn extract_minimap(entry: &MapEntry) -> Option<Vec<u8>> {
    let game = resolve_game_dir(None).ok()?;
    let candidates = [entry.minimap_dir.as_str(), entry.key.as_str()];
    for dir in candidates {
        if dir.is_empty()
            || dir.len() > 40
            || !dir
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            continue;
        }
        let base = game
            .join("Gfx")
            .join("UI")
            .join("BattleScreenHUD")
            .join("minimap")
            .join(dir);
        for name in MINIMAP_DVPL {
            if let Ok(dv) = DvplFile::read(&base.join(name)) {
                if !dv.data.is_empty() {
                    return Some(dv.data);
                }
            }
        }
    }
    None
}

/// 响应附带 X-Map-Meta（铺设参数 JSON），前端据此放置底图平面。
/// 地图资源一律 no-cache：导出器重跑后同 URL 内容会变，禁止浏览器拿旧 GLB/旧贴图。
fn map_response(bytes: Vec<u8>, content_type: &str, entry: Option<&MapEntry>) -> Response {
    let meta = entry.map(map_meta).unwrap_or_default();
    let meta = serde_json::to_string(&meta).unwrap_or_default();
    (
        [
            (axum::http::header::CONTENT_TYPE, content_type.to_string()),
            (
                axum::http::header::HeaderName::from_static("x-map-meta"),
                meta,
            ),
            (axum::http::header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        bytes,
    )
        .into_response()
}

// ---------- 高度场地形（GET /api/playback/terrain） ----------
//
// 格式：
//   8 字节头（u32 size=512、u32 tile=16）+ size²×2 字节 u16，按 tile×tile 块存储
//   （块行主序），块内行主序；存储行 0=南、列 0=东；高度 z = u16 * zMax / 65535；
//   覆盖世界 [-300,+300]²。输出统一列翻转（列 0=西），行序不变（行 0=南）。
// zMax 来自导出器 sidecar（Landscape 世界包围盒）。

/// 一张图解码后的高度场：行 0=南、列 0=西（行主序 u16，米制换算系数见响应头 zmax）。
pub struct TerrainGrid {
    pub heights: Vec<u16>,
}

/// sidecar（data/cache/maps/<space>.json）中的地形尺度。
struct TerrainScale {
    zmax: f32,
    zmin: f32,
    span: f32,
    playable: Option<PlayableBounds>,
}

/// 读 sidecar：worldBounds.min/max → span/zmin/zmax（导出器按客户端 Landscape bbox 写出）。
fn terrain_scale(entry: &MapEntry) -> Option<TerrainScale> {
    let bytes = crate::data::read_shareable(&format!("data/cache/maps/{}.json", entry.space))?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let bounds = v.get("worldBounds")?;
    let min = bounds.get("min")?.as_array()?;
    let max = bounds.get("max")?.as_array()?;
    let f = |a: &serde_json::Value| a.as_f64().map(|x| x as f32);
    if min.len() < 3 || max.len() < 3 {
        return None;
    }
    let zmax = f(&max[2])?;
    let zmin = f(&min[2]).unwrap_or(0.0);
    let dx = f(&max[0])? - f(&min[0])?;
    let dy = f(&max[1])? - f(&min[1])?;
    let span = dx.max(dy);
    if zmax <= zmin || span <= 0.0 {
        return None;
    }
    // 游戏内实际战场边界（同一 sidecar；export_map_glb 从场景 MapBorderComponent 提取）
    let playable = v.get("playableBounds").and_then(|pb| {
        let g = |k: &str| pb.get(k).and_then(|x| x.as_f64()).map(|x| x as f32);
        Some(PlayableBounds {
            x_min: g("xMin")?,
            y_min: g("yMin")?,
            x_max: g("xMax")?,
            y_max: g("yMax")?,
        })
    });
    Some(TerrainScale {
        zmax,
        zmin,
        span,
        playable,
    })
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
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
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
fn extract_heightmap(space: &str) -> Option<TerrainGrid> {
    let game = resolve_game_dir(None).ok()?;
    let dir = game.join("3d/Maps").join(space).join("landscape");
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

/// 预提取全部注册表地图的高度场到 `_cache/`（`fetch-terrain` 命令）：
/// 已有缓存且未 `--force` 时跳过。返回 (总数, 新提取, 已缓存, 失败键名列表)。
pub fn cache_all_terrain(force: bool) -> (usize, usize, usize, Vec<String>) {
    let registry = load_registry();
    let mut extracted = 0usize;
    let mut cached = 0usize;
    let mut failed = Vec::new();
    for entry in &registry {
        let cache = crate::data::cache_path(&format!("terrain/{}.hm.u16.bin", entry.key));
        if cache.exists() && !force {
            cached += 1;
            continue;
        }
        match extract_heightmap(&entry.space) {
            Some(t) => {
                let bytes = terrain_bytes(&t);
                if let Some(parent) = cache.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&cache, &bytes).is_ok() {
                    extracted += 1;
                } else {
                    failed.push(entry.key.clone());
                }
            }
            None => failed.push(entry.key.clone()),
        }
    }
    (registry.len(), extracted, cached, failed)
}

/// 预提取全部注册表地图的小地图到 `data/cache/maps/<space>.minimap.webp`
/// （`fetch-minimaps` 命令，低画质档地面）；返回 (注册表总数, 本次提取, 已缓存, 失败名单)。
pub fn cache_all_minimaps(force: bool) -> (usize, usize, usize, Vec<String>) {
    let registry = registry();
    let mut extracted = 0usize;
    let mut cached = 0usize;
    let mut failed = Vec::new();
    for entry in registry {
        let cache = cache_path(&format!("maps/{}.minimap.webp", entry.space));
        if cache.exists() && !force {
            cached += 1;
            continue;
        }
        match extract_minimap(entry) {
            Some(bytes) => {
                if let Some(parent) = cache.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&cache, &bytes).is_ok() {
                    extracted += 1;
                } else {
                    failed.push(entry.key.clone());
                }
            }
            None => failed.push(entry.key.clone()),
        }
    }
    (registry.len(), extracted, cached, failed)
}

/// GET /api/playback/terrain?name=<...>|?id=<n>：覆盖 → 缓存 → 游戏提取。
/// 响应体 = u16 LE 高度场，X-Terrain-Meta = {"size":..,"zmax":..,"zmin":..,"span":..}。
/// 尺度（zmax/zmin/span）必须来自 sidecar 或手动 meta；两者皆缺时拒绝伺服
/// （u16 满量程换算无据，错标尺度比 404 更糟）。
pub fn terrain_response(map_param: &str) -> Response {
    let name = map_param.trim();
    if name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing map").into_response();
    }
    let Some(entry) = resolve_map(name) else {
        return (axum::http::StatusCode::NOT_FOUND, "terrain not available").into_response();
    };
    let manual = map_meta(entry);
    let scale = terrain_scale(entry);
    let playable = scale.as_ref().and_then(|s| s.playable);
    let (zmax, zmin, span) = match scale {
        Some(s) => (manual.zmax_m.unwrap_or(s.zmax), s.zmin, s.span),
        None => match manual.zmax_m {
            Some(z) => (z, 0.0, DEFAULT_SIZE_M),
            None => {
                eprintln!(
                    "[map-assets] {} 缺少地形尺度（data/cache/maps/{}.json），请运行 tools/export_map_glb.py",
                    entry.space, entry.space
                );
                return (axum::http::StatusCode::NOT_FOUND, "terrain not available")
                    .into_response();
            }
        },
    };
    terrain_serve(entry, zmax, zmin, span, playable)
}

fn terrain_serve(
    entry: &MapEntry,
    zmax: f32,
    zmin: f32,
    span: f32,
    playable: Option<PlayableBounds>,
) -> Response {
    // 1) 手动覆盖：data/maps/<key>.heightmap.u16.bin（512×512 LE，行 0=南）
    let override_path = data_path(&format!("maps/{}.heightmap.u16.bin", entry.key));
    if let Ok(bytes) = std::fs::read(&override_path) {
        if bytes.len() == 512 * 512 * 2 {
            return terrain_response_bytes(bytes, zmax, zmin, span, playable);
        }
        eprintln!(
            "[map-assets] 高度覆盖尺寸不符（应为 {} 字节）：{}",
            512 * 512 * 2,
            override_path.display()
        );
    }

    // 2) 提取缓存
    let cache = crate::data::cache_path(&format!("terrain/{}.hm.u16.bin", entry.key));
    if let Ok(bytes) = std::fs::read(&cache) {
        if bytes.len() == 512 * 512 * 2 {
            return terrain_response_bytes(bytes, zmax, zmin, span, playable);
        }
    }

    // 3) 游戏客户端提取
    let Some(t) = extract_heightmap(&entry.space) else {
        return (axum::http::StatusCode::NOT_FOUND, "terrain not available").into_response();
    };
    let bytes = terrain_bytes(&t);
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, &bytes);
    eprintln!(
        "[map-assets] extracted heightmap {} -> {}",
        entry.space,
        cache.display()
    );
    terrain_response_bytes(bytes, zmax, zmin, span, playable)
}

/// 响应附带 X-Terrain-Meta（高度场解释参数）。
fn terrain_response_bytes(
    bytes: Vec<u8>,
    zmax: f32,
    zmin: f32,
    span: f32,
    playable: Option<PlayableBounds>,
) -> Response {
    // playableBounds 透传（游戏内实际战场边界；缺失则不发该键，前端回退 worldBounds）
    let pb = match playable {
        Some(b) => format!(
            r#","playableBounds":{{"xMin":{:.3},"yMin":{:.3},"xMax":{:.3},"yMax":{:.3}}}"#,
            b.x_min, b.y_min, b.x_max, b.y_max
        ),
        None => String::new(),
    };
    let meta = format!(r#"{{"size":512,"zmax":{zmax:.1},"zmin":{zmin:.1},"span":{span:.1}{pb}}}"#);
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/octet-stream".to_string(),
            ),
            (
                axum::http::header::HeaderName::from_static("x-terrain-meta"),
                meta,
            ),
            (axum::http::header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        bytes,
    )
        .into_response()
}

/// GET /api/playback/scenery：伺服离线导出的静态场景 GLB
/// （客户端管线导出：建筑/树木真贴图；tools/export_map_glb.py 预生成到
/// data/cache/maps/<space>.glb，运行时不做 SC2 解析。缺失 404，前端静默跳过）。
pub fn scenery_response(map_param: &str) -> Response {
    let Some(entry) = resolve_map(map_param.trim()) else {
        return (axum::http::StatusCode::NOT_FOUND, "scenery not available").into_response();
    };
    // 与 viewer.rs 的坦克 GLB 缓存同目录体系（data/cache/ 已 gitignore）
    match crate::data::read_shareable(&format!("data/cache/maps/{}.glb", entry.space)) {
        Some(bytes) => (
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "model/gltf-binary".to_string(),
                ),
                (axum::http::header::CACHE_CONTROL, "no-cache".to_string()),
            ],
            bytes,
        )
            .into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "scenery not available").into_response(),
    }
}

/// GET /api/playback/groundmeta?id=19 —— 地表分层合成参数（tools/export_map_glb.py
/// 的 <space>.ground.layers.json：textureTiling/tileScale/tileColor/HeightBlend 等，
/// 前端按客户端 tilemask-fp.sl 实时合成）。缺失 404，前端回退整图烘焙。
pub fn ground_layers_meta_response(map_param: &str) -> Response {
    let Some(entry) = resolve_map(map_param.trim()) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            "ground layers not available",
        )
            .into_response();
    };
    match crate::data::read_shareable(&format!(
        "data/cache/maps/{}.ground.layers.json",
        entry.space
    )) {
        Some(bytes) => (
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "application/json".to_string(),
                ),
                (axum::http::header::CACHE_CONTROL, "no-cache".to_string()),
            ],
            bytes,
        )
            .into_response(),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            "ground layers not available",
        )
            .into_response(),
    }
}

/// GET /api/playback/groundtex?id=19&k=<层名> —— 地表分层贴图
/// （cm/lm/tile0/tile1/mask0/mask1[/hmap0/hmap1]；全部无 alpha——Chrome 把带
/// alpha 的 webp 解码为预乘 RGB，GPU 侧权重会被压暗近黑；行序为 colormap
/// 原始空间）。
pub fn ground_layer_response(map_param: &str, layer: &str) -> Response {
    let ok = matches!(
        layer,
        "cm" | "lm" | "tile0" | "tile1" | "mask0" | "mask1" | "hmap0" | "hmap1"
    );
    if !ok {
        return (axum::http::StatusCode::BAD_REQUEST, "bad layer").into_response();
    }
    let Some(entry) = resolve_map(map_param.trim()) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            "ground layer not available",
        )
            .into_response();
    };
    match crate::data::read_shareable(&format!(
        "data/cache/maps/{}.ground.{layer}.webp",
        entry.space
    )) {
        Some(bytes) => (
            [
                (axum::http::header::CONTENT_TYPE, "image/webp".to_string()),
                (axum::http::header::CACHE_CONTROL, "no-cache".to_string()),
            ],
            bytes,
        )
            .into_response(),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            "ground layer not available",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 注册表可用时的健全性：id 唯一、space/键安全、26 张现役图齐全。
    #[test]
    fn registry_sanity() {
        let reg = registry();
        if reg.is_empty() {
            eprintln!("[skip] 游戏目录不可用，跳过注册表测试");
            return;
        }
        assert!(
            reg.len() >= 26,
            "注册表应至少覆盖现役 26 图，实际 {}",
            reg.len()
        );
        for e in reg {
            assert!(e.is_safe(), "unsafe space: {}", e.space);
            assert!(!e.key.is_empty());
            assert!(e.sc2.starts_with(&format!("{}/", e.space)));
        }
        let mut ids: Vec<u32> = reg.iter().map(|e| e.map_id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), reg.len(), "地图 id 应唯一");
        // 现役图（wotbreplay-parser MapId 判别值）必须全部可解析
        for id in [
            2u32, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 19, 20, 21, 23, 25, 27, 30, 31, 35, 38,
            40, 42, 71,
        ] {
            let e = resolve_map(&id.to_string());
            assert!(e.is_some(), "现役图 id={id} 未解析到");
            assert!(!e.unwrap().display.is_empty(), "id={id} 缺显示名");
        }
    }

    /// 客户端数据修正解析器枚举的两处互换：id 5 = Falls Creek(amigosville)、
    /// id 38 = Alpenstadt(lumber)。
    #[test]
    fn numeric_id_resolution_follows_client_data() {
        let reg = registry();
        if reg.is_empty() {
            eprintln!("[skip] 游戏目录不可用，跳过注册表测试");
            return;
        }
        let id5 = resolve_map("5").unwrap();
        assert_eq!(id5.space, "05_amigosville_am");
        let id38 = resolve_map("38").unwrap();
        assert_eq!(id38.space, "31_lumber_lm");
    }

    /// 显示名/键名归一解析 + 未知名字拒绝。
    #[test]
    fn resolve_by_display_and_key() {
        let reg = registry();
        if reg.is_empty() {
            eprintln!("[skip] 游戏目录不可用，跳过注册表测试");
            return;
        }
        let by_id = resolve_map("19").unwrap();
        assert_eq!(by_id.space, "12_malinovka_ma");
        let by_display = resolve_map("Winter Malinovka").unwrap();
        assert_eq!(by_display.map_id, 19);
        let by_key = resolve_map("malinovka").unwrap();
        assert_eq!(by_key.map_id, 19);
        assert!(resolve_map("../etc").is_none());
        assert!(resolve_map("").is_none());
        assert!(resolve_map("no_such_map").is_none());
    }

    /// 标准高度图解析：tile 重排 + 列翻转 + u16→米换算。
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

    /// 覆盖/标定文件名白名单（不拼路径）。
    #[test]
    fn meta_path_rejects_traversal() {
        assert!(meta_path("../evil").is_none());
        assert!(meta_path("ok_name1").is_some());
    }

    /// maps.yaml 行解析：嵌套缩进中的 id/localName 配对。
    #[test]
    fn parse_maps_yaml_pairs_id_and_local_name() {
        let text = "maps:\n    malinovka:\n        id: 19\n        tags: \"mn1\"\n        localName: \"12_malinovka_ma/12_malinovka_ma.sc2\"\n        extra:\n            nested: 1\n";
        let parsed = parse_maps_yaml(text);
        assert_eq!(
            parsed,
            vec![(
                19u32,
                "malinovka".to_string(),
                "12_malinovka_ma/12_malinovka_ma.sc2".to_string()
            )]
        );
    }

    /// en.yaml #maps: 行解析：同路径多变体全部收集。
    #[test]
    fn parse_en_yaml_maps_collects_variants() {
        let text = "\"a\": \"b\"\r\n\"#maps:rudniki:06_rudniki_rd/06_rudniki_rd.sc2\": \"Mines\"\r\n\"#maps:rudniki_01:06_rudniki_rd/06_rudniki_rd.sc2\": \"Mines - Hill\"\r\n";
        let parsed = parse_en_yaml_maps(text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            (
                "06_rudniki_rd/06_rudniki_rd.sc2".to_string(),
                "rudniki".to_string(),
                "Mines".to_string()
            )
        );
        assert_eq!(
            parsed[1],
            (
                "06_rudniki_rd/06_rudniki_rd.sc2".to_string(),
                "rudniki_01".to_string(),
                "Mines - Hill".to_string()
            )
        );
    }

    /// 端到端（需游戏目录 + 导出产物）：数字 id 走通底图/地形/场景/草地四端点。
    #[test]
    fn endpoints_served_via_numeric_id() {
        let reg = registry();
        if reg.is_empty() {
            eprintln!("[skip] 游戏目录不可用，跳过端点测试");
            return;
        }
        if crate::data::read_shareable("data/cache/maps/12_malinovka_ma.glb").is_none() {
            eprintln!("[skip] 缺少导出产物，跳过端点测试");
            return;
        }
        assert_eq!(map_image_response("19").status(), 200);
        assert_eq!(terrain_response("19").status(), 200);
        assert_eq!(scenery_response("19").status(), 200);
        assert_eq!(scenery_response("no_such_map").status(), 404);
    }
}
