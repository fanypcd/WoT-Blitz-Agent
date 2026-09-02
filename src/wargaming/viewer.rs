use axum::{routing::{get, post}, response::{Html, IntoResponse, Response}, Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::path::Path;
use crate::wargaming::tank_resolver::TankResolver;
use crate::wargaming::dvpl::{DvplFile, CollisionData, ArmorModel};
use crate::wargaming::penetration::{self, PenetrationRequest};

const GLB_CACHE_DIR: &str = "glb_cache";
const VENDOR_DIR: &str = "web/vendor/three";
const GLB_FILES: [&str; 2] = ["collision.glb", "model.glb"];

/// 获取 WSL 的局域网 IP（用于打印可访问的地址）。
fn wsl_ip() -> String {
    std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().next().map(|s| s.to_string()))
        .unwrap_or_else(|| "localhost".to_string())
}

/// 从游戏安装目录加载某坦克的完整装甲模型（优先 game_data/，回退游戏文件）。
fn load_armor_model(tank_id: u32) -> Option<ArmorModel> {
    // Prefer pre-extracted portable data (no game install required)
    if let Some(crate::wargaming::game_extract::TankGameData { armor_model: Some(m), .. }) =
        crate::wargaming::game_extract::load_game_data(tank_id, &crate::data::data_dir().join("game_data"))
    {
        return Some(m);
    }

    let dev_name = find_dev_name(tank_id)?;
    let game_dirs = [
        "/mnt/d/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
        "D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
    ];
    let nations = ["ussr", "usa", "germany", "uk", "japan", "china", "france", "european", "other"];
    for game_dir in &game_dirs {
        for nation in &nations {
            let filepath = format!("{}/XML/item_defs/vehicles/{}/{}.xml.dvpl", game_dir, nation, dev_name);
            if Path::new(&filepath).exists() {
                if let Ok(dvpl) = DvplFile::read(Path::new(&filepath)) {
                    let text = String::from_utf8_lossy(&dvpl.data);
                    return ArmorModel::parse_from_xml(&text);
                }
            }
        }
    }
    None
}

/// 从游戏安装目录加载某坦克的碰撞数据（优先 game_data/，回退游戏文件）。
fn load_collision_data(_resolver: &TankResolver, tank_id: u32) -> Option<CollisionData> {
    // Prefer pre-extracted portable data (no game install required)
    if let Some(crate::wargaming::game_extract::TankGameData { collision: Some(c), .. }) =
        crate::wargaming::game_extract::load_game_data(tank_id, &crate::data::data_dir().join("game_data"))
    {
        return Some(c);
    }

    let dev_name = find_dev_name(tank_id)?;
    
    let game_dirs = [
        "/mnt/d/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
        "D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
    ];
    
    let nations = ["ussr", "usa", "germany", "uk", "japan", "china", "france", "european", "other"];
    
    for game_dir in &game_dirs {
        for nation in &nations {
            let filepath = format!("{}/3d/Tanks/Parameters/{}/{}.yaml.dvpl", game_dir, nation, dev_name);
            if Path::new(&filepath).exists() {
                if let Ok(dvpl) = DvplFile::read(Path::new(&filepath)) {
                    let text = String::from_utf8_lossy(&dvpl.data);
                    let text_ref: &str = &text;
                    return CollisionData::parse_from_yaml(text_ref);
                }
            }
        }
    }
    None
}

/// 从 tanks.pb 获取某坦克的 game dev 名（用于 DVPL 回退路径）。
fn find_dev_name(tank_id: u32) -> Option<String> {
    crate::wargaming::blitzkit::tank_full(tank_id).map(|t| t.dev_name)
}

/// 全局 3D 查看器坦克解析器（供各 handler 在不依赖 axum state 的情况下使用，
/// 便于 Web GUI 以 `/armor_view` 前缀复用这些 API）。
static GLOBAL_RESOLVER: std::sync::OnceLock<Arc<TankResolver>> = std::sync::OnceLock::new();

fn global_resolver() -> Arc<TankResolver> {
    GLOBAL_RESOLVER.get_or_init(|| {
        TankResolver::load_from_json_file(&crate::data::data_path("tank_cache.json"))
            .map(Arc::new)
            .unwrap_or_default()
    }).clone()
}

/// 设置全局坦克解析器（Web GUI 启动时注入，供 `/armor_view` 下的 handler 复用）。
pub fn set_global_resolver(resolver: TankResolver) {
    let _ = GLOBAL_RESOLVER.set(Arc::new(resolver));
}

/// 启动 3D 查看器服务器：加载坦克数据、注册路由、绑定端口并打开浏览器。
/// 供 `view <tank_id>` 命令和 Agent 的 view_tank 工具调用。
///
/// `shooter_id` 可选：若给定，前端会把该坦克预选为"射击车辆"，`tank_id` 作为受击车辆。
pub async fn serve(tank_resolver: TankResolver, tank_id: u32, shooter_id: Option<u32>) -> anyhow::Result<()> {
    let app = build_viewer_router(tank_resolver, tank_id, shooter_id, "");

    let addr = SocketAddr::from(([0, 0, 0, 0], 0));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    // Try localhost first, fall back to WSL IP
    let url = format!("http://127.0.0.1:{}", local_addr.port());

    eprintln!("Server running at {}", url);
    eprintln!("If browser doesn't open, try: http://localhost:{} or http://{}:{}",
        local_addr.port(), wsl_ip(), local_addr.port());
    eprintln!("Opening browser...");

    if webbrowser::open(&url).is_err() {
        eprintln!("Please open {} in your browser manually.", url);
    }

    axum::serve(listener, app).await?;

    Ok(())
}

/// 渲染 3D 查看器页面（注入 tank_id / shooter_id，并把资源/API 路径加上 `base_prefix`）。
/// 供独立 `serve` 与 Web GUI 内嵌（`/armor_view/view/{tank_id}`）复用。
pub fn viewer_index_html(tank_id: u32, shooter_id: u32, base_prefix: &str) -> String {
    // Vendored Three.js (offline-capable) with CDN fallback
    let vendor_local = Path::new(VENDOR_DIR).join("three.module.js").exists();
    let importmap = if vendor_local {
        r#"{ "imports": { "three": "/vendor/three/three.module.js", "three/addons/": "/vendor/three/addons/" } }"#
    } else {
        r#"{ "imports": { "three": "https://cdn.jsdelivr.net/npm/three@0.169.0/build/three.module.js", "three/addons/": "https://cdn.jsdelivr.net/npm/three@0.169.0/examples/jsm/" } }"#
    };
    INDEX_HTML
        .replace("__IMPORTMAP__", importmap)
        .replace("__INITIAL_TANK_VALUE__", &tank_id.to_string())
        .replace("__INITIAL_SHOOTER_VALUE__", &shooter_id.to_string())
        // 给前端内嵌的资源/API 绝对路径加前缀（独立 serve 时 base_prefix 为空，无副作用）
        .replace("\"/glb", &format!("\"{}/glb", base_prefix))
        .replace("\"/vendor", &format!("\"{}/vendor", base_prefix))
        .replace("\"/api/", &format!("\"{}/api/", base_prefix))
        .replace("'/api/", &format!("'{}/api/", base_prefix))
        .replace("src=\"/", &format!("src=\"{}/", base_prefix))
}

/// 构建 3D 查看器的 Router（供独立 `view` 命令与 Web GUI 内嵌复用）。
/// 内嵌时用 `nest` 挂到子路径下即可；`base_prefix` 为该子路径（如 `/armor_view`），
/// 会把前端资源/API 路径加上前缀，避免与宿主路由冲突。
pub fn build_viewer_router(
    tank_resolver: TankResolver,
    tank_id: u32,
    shooter_id: Option<u32>,
    base_prefix: &str,
) -> axum::Router {
    set_global_resolver(tank_resolver);

    let index_html = viewer_index_html(tank_id, shooter_id.unwrap_or(tank_id), base_prefix);

    Router::new()
        .route("/", get({
            let index_html = index_html.clone();
            move || async move { Html(index_html.clone()) }
        }))
        .route("/glb/{tank_id}/{filename}", get(glb_handler))
        .route("/vendor/three/{*path}", get(vendor_handler))
        .route("/api/tank/{tank_id}", get(tank_data_handler))
        .route("/api/tank_filter", get(tank_filter_handler))
        .route("/api/tank_image/{tank_id}", get(tank_image_handler))
        .route("/api/shells/{tank_id}", get(shells_handler))
        .route("/api/penetrate", post(penetrate_handler))
        .with_state(())
}

/// 3D 模型代理：`glb_cache/` 有则直接返回，否则从 BlitzKit CDN 下载并落盘。
/// 仅允许 collision.glb / model.glb 两个文件名。
pub(crate) async fn glb_handler(
    axum::extract::Path((tank_id, filename)): axum::extract::Path<(u32, String)>,
) -> Response {
    if !GLB_FILES.contains(&filename.as_str()) {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid GLB filename").into_response();
    }

    // 1. Serve from local cache
    let cache_dir = Path::new(GLB_CACHE_DIR).join(tank_id.to_string());
    let cache_path = cache_dir.join(&filename);
    if let Ok(bytes) = std::fs::read(&cache_path) {
        return glb_response(bytes);
    }

    // 2. Download from BlitzKit CDN, persist to cache, serve
    let url = format!("https://api.blitzkit.app/tanks/{}/{}", tank_id, filename);
    eprintln!("[glb-cache] downloading {} ...", url);
    // 带超时的客户端，避免 CDN 慢时请求挂起、前端一直 loading。
    // BlitzKit CDN 偶发慢/失败，这里重试两次；仍失败则返回明确错误。
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build();
    let mut last_err: Option<String> = None;
    for _attempt in 0..3 {
        let c = match &client { Ok(c) => c.clone(), Err(_) => reqwest::Client::new() };
        match c.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.bytes().await {
                    Ok(bytes) => {
                        let vec = bytes.to_vec();
                        let _ = std::fs::create_dir_all(&cache_dir);
                        match std::fs::write(&cache_path, &vec) {
                            Ok(_) => eprintln!("[glb-cache] cached {} ({} bytes)", cache_path.display(), vec.len()),
                            Err(e) => eprintln!("[glb-cache] cache write failed: {}", e),
                        }
                        return glb_response(vec);
                    }
                    Err(e) => last_err = Some(format!("read body failed: {}", e)),
                }
            }
            Ok(resp) => last_err = Some(format!("BlitzKit CDN returned {}", resp.status())),
            Err(e) => last_err = Some(format!("{}", e)),
        }
        eprintln!("[glb-cache] attempt failed, retrying... ({})", last_err.clone().unwrap_or_default());
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    (
        axum::http::StatusCode::BAD_GATEWAY,
        format!("BlitzKit CDN unreachable: {} (model not in glb_cache/)", last_err.unwrap_or_default()),
    ).into_response()
}

fn glb_response(bytes: Vec<u8>) -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "model/gltf-binary")],
        bytes,
    ).into_response()
}

const TANK_IMAGE_DIR: &str = "tank_images";

/// Serve a tank preview icon (big.webp) from BlitzKit, cached into tank_images/.
/// 坦克封面图代理：`tank_images/` 有则返回，否则从 BlitzKit 下载并落盘。
pub(crate) async fn tank_image_handler(axum::extract::Path(tank_id): axum::extract::Path<u32>) -> Response {
    let cache_path = Path::new(TANK_IMAGE_DIR).join(format!("{}.webp", tank_id));
    if let Ok(bytes) = std::fs::read(&cache_path) {
        return image_response(bytes);
    }

    let url = format!("https://api.blitzkit.app/tanks/{}/icons/big.webp", tank_id);
    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => {
            match resp.bytes().await {
                Ok(bytes) => {
                    let vec = bytes.to_vec();
                    let _ = std::fs::create_dir_all(TANK_IMAGE_DIR);
                    if std::fs::write(&cache_path, &vec).is_ok() {
                        eprintln!("[image-cache] cached {} ({} bytes)", cache_path.display(), vec.len());
                    }
                    image_response(vec)
                }
                Err(e) => (axum::http::StatusCode::BAD_GATEWAY, format!("image download failed: {}", e)).into_response(),
            }
        }
        Ok(resp) => (
            axum::http::StatusCode::BAD_GATEWAY,
            format!("BlitzKit icon returned {}", resp.status()),
        ).into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_GATEWAY,
            format!("BlitzKit icon unreachable: {}", e),
        ).into_response(),
    }
}

fn image_response(bytes: Vec<u8>) -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "image/webp")],
        bytes,
    ).into_response()
}

/// 提供 `web/vendor/` 下的静态资源（Three.js 等，带路径穿越防护）。
pub(crate) async fn vendor_handler(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    if path.contains("..") {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid path").into_response();
    }
    let full = Path::new(VENDOR_DIR).join(&path);
    match std::fs::read(&full) {
        Ok(bytes) => {
            let ct = if path.ends_with(".js") {
                "application/javascript"
            } else if path.ends_with(".map") {
                "application/json"
            } else {
                "application/octet-stream"
            };
            ([(axum::http::header::CONTENT_TYPE, ct)], bytes).into_response()
        }
        Err(_) => (axum::http::StatusCode::NOT_FOUND, format!("vendor file not found: {}", path)).into_response(),
    }
}

/// 击穿判定端点：前端点击后 POST 命中列表，调用 penetration::calculate 返回结果。
pub(crate) async fn penetrate_handler(Json(req): Json<PenetrationRequest>) -> Json<Value> {
    let result = penetration::calculate(&req);
    Json(serde_json::to_value(result).unwrap_or(json!(null)))
}

/// 返回某坦克的完整数据（元数据、配置列表、弹种、装甲、模型 URL 等）供前端渲染。
pub(crate) async fn tank_data_handler(
    axum::extract::Path(tank_id): axum::extract::Path<u32>,
) -> Json<Value> {
    Json(tank_data_value(tank_id))
}

/// 构建某坦克的完整检视数据（供独立 handler 与 Web GUI 内嵌复用）。
/// `base_prefix` 为资源前缀（如 `/armor_view`），空表示不加前缀。
pub(crate) fn tank_data_value(tank_id: u32) -> Value {
    tank_data_value_prefixed(tank_id, "")
}

/// 实际实现：按前缀生成模型 URL（Web 内嵌时 model_url 需带 `/armor_view` 前缀）。
pub(crate) fn tank_data_value_prefixed(tank_id: u32, base_prefix: &str) -> Value {
    let resolver = global_resolver();

    let info = resolver.resolve_info(tank_id);

    // Armor/plates from armor_cache.json (per-plate dictionary)
    let armor_plates = std::fs::read_to_string(crate::data::data_path("armor_cache.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get(tank_id.to_string()).cloned());

    // Armor model + collision from portable game_data/ extraction (fallback: game install)
    let portable = crate::wargaming::game_extract::load_game_data(tank_id, &crate::data::data_dir().join("game_data"));
    let (armor_model, collision) = match &portable {
        Some(gd) => (gd.armor_model.clone(), gd.collision.clone()),
        None => {
            let armor_model = load_armor_model(tank_id);
            let collision = load_collision_data(&resolver, tank_id);
            (armor_model, collision)
        }
    };

    // Gun angles from models.pb data
    let gun_angles = std::fs::read_to_string(crate::data::data_path("gun_angles.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get(tank_id.to_string()).cloned());

    let name = resolver
        .resolve(tank_id)
        .unwrap_or_else(|| format!("tank_{}", tank_id));
    let tier = info
        .as_ref()
        .map(|i| i.tier as u32)
        .filter(|t| *t > 0)
        .unwrap_or(0);
    let tank_type = info
        .as_ref()
        .map(|i| i.tank_type.clone())
        .filter(|t| !t.is_empty() && t != "unknown")
        .unwrap_or_else(|| "unknown".to_string());
    let nation = info
        .as_ref()
        .map(|i| i.nation.clone())
        .filter(|n| !n.is_empty() && n != "unknown")
        .unwrap_or_else(|| "unknown".to_string());

    let armor = info.as_ref().and_then(|i| i.armor.as_ref()).map(|a| json!({
        "turret": {"front": a.turret_front, "sides": a.turret_sides, "rear": a.turret_rear},
        "hull": {"front": a.hull_front, "sides": a.hull_sides, "rear": a.hull_rear},
    }));

    let shells = info.as_ref().map(|i| {
        i.shells.iter().map(|s| json!({
            "type": s.shell_type,
            "penetration": s.penetration,
            "damage": s.damage,
            "module_damage": s.module_damage,
        })).collect::<Vec<_>>()
    }).unwrap_or_default();

    let armor_model_val = armor_model.as_ref().map(|m| serde_json::to_value(m).unwrap_or(json!(null)));
    let collision_val = collision.as_ref().map(|c| json!({
        "hull_bbox": bbox_to_json(&c.hull_bbox),
        "turret_bbox": bbox_to_json(&c.turret_bbox),
        "gun_bbox": bbox_to_json(&c.gun_bbox),
        "chassis_bbox": bbox_to_json(&c.chassis_bbox),
        "avg_thickness_hull": c.average_thickness_hull,
        "hull_points": c.hull_points,
        "turret_points": c.turret_points,
        "gun_points": c.gun_points,
    }));

    let configs = build_configs(tank_id);
    let caliber = configs.first().and_then(|c| c.get("caliber")).and_then(|v| v.as_u64()).unwrap_or(120) as u32;

    let ga = gun_angles.as_ref();
    json!({
        "tank_id": tank_id,
        "name": name,
        "tier": tier,
        "type": tank_type,
        "nation": nation,
        "model_url": format!("{}/glb/{}/collision.glb", base_prefix, tank_id),
        "visual_model_url": format!("{}/glb/{}/model.glb", base_prefix, tank_id),
        "armor": armor,
        "armor_plates": armor_plates.unwrap_or(json!(null)),
        "armor_model": armor_model_val,
        "collision": collision_val,
        "caliber": caliber,
        "shells": shells,
        "configs": configs,
        "hp": info.as_ref().and_then(|i| i.hp),
        "speed": info.as_ref().and_then(|i| i.speed_forward),
        "gun_depression": ga.and_then(|g| g.get("gun_depression")).and_then(|v| v.as_f64()).or(info.as_ref().and_then(|i| i.gun_depression).map(|v| v as f64)),
        "gun_elevation": ga.and_then(|g| g.get("gun_elevation")).and_then(|v| v.as_f64()).or(info.as_ref().and_then(|i| i.gun_elevation).map(|v| v as f64)),
        "turret_traverse_left": info.as_ref().and_then(|i| i.turret_traverse_left).map(|v| v as f64),
        "turret_traverse_right": info.as_ref().and_then(|i| i.turret_traverse_right).map(|v| v as f64),
    })
}

/// Build the list of selectable turret/gun configurations (configs) for a tank.
/// Each config carries the gun name, caliber, shells, and a positional index used
/// to match the corresponding `turret_0X` / `gun_0X` nodes in the GLB model.
/// 构建某坦克的可选配置（多炮塔/多主炮）列表，每项含枪名、口径、弹种、索引。
/// 从模型 GLB 读取 gun/turret 配置节点名（用于把 pb 火炮/炮塔配置正确映射到模型节点）。
/// 模型节点在数组中按编号降序存放（如 gun_13...gun_01），反转后 = 升序 = 前端 configGunGroups 顺序，
/// 从而保证 config 的 gun_index/turret_index 与前端可切换的模型组索引严格一致。
fn model_config_nodes(tank_id: u32) -> (Vec<String>, Vec<String>) {
    let mut guns = Vec::new();
    let mut turrets = Vec::new();
    let path = std::path::Path::new(GLB_CACHE_DIR).join(tank_id.to_string()).join("model.glb");
    if let Ok(bytes) = std::fs::read(&path) {
        if let Some(names) = parse_glb_top_nodes(&bytes) {
            for nm in names {
                if let Some(g) = nm.strip_prefix("gun_") {
                    if g.chars().all(|c| c.is_ascii_digit()) { guns.push(nm); }
                } else if let Some(t) = nm.strip_prefix("turret_") {
                    if t.chars().all(|c| c.is_ascii_digit()) { turrets.push(nm); }
                }
            }
            // 模型数组为降序（gun_13...gun_01），反转成升序（gun_01...gun_13），与前端 configGunGroups 一致
            guns.reverse();
            turrets.reverse();
        }
    }
    (guns, turrets)
}

/// 极简 GLB 解析：返回场景根节点的顶层子节点名（按数组顺序）。
fn parse_glb_top_nodes(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.len() < 20 { return None; }
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    if 20 + json_len > bytes.len() { return None; }
    let js: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).ok()?;
    let nodes = js.get("nodes")?.as_array()?;
    let scene = js.get("scenes")?.as_array()?.first()?.get("nodes")?.as_array()?;
    let root_idx = scene.first()?;
    let root = nodes.get(root_idx.as_u64()? as usize)?;
    let Some(children) = root.get("children").and_then(|c| c.as_array()) else { return None };
    Some(children.iter()
        .filter_map(|c| nodes.get(c.as_u64()? as usize)?.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
        .collect())
}

pub(crate) fn build_configs(tank_id: u32) -> Vec<Value> {
    let Some(tank) = crate::wargaming::blitzkit::tank_full(tank_id) else { return Vec::new() };

    // 读取模型实际配置节点（gun/turret），用于把火炮配置正确映射到模型可切换组
    let (model_guns, model_turrets) = model_config_nodes(tank_id);

    // 权威映射：models.pb 给出每辆坦克每套炮塔/主炮绑定的模型节点编号（gun_0X / turret_0X）。
    // 前端 configGunGroups / configTurretNodes 按节点编号升序排列，gun_index/turret_index 是
    // 在该升序列表中的稠密下标（0..N-1），而非节点编号本身。因此这里把配置文件按 node 编号
    // 去重升序，得到 node→dense_index 映射，再为每个配置计算它的稠密下标。
    // 这能正确处理：多炮塔共享炮（AC Celeno V1）、多配置共享单节点（116-F3/AC Atlas）、
    // 每炮塔独立节点（Tiger II gun_02/04/07/08/09/10）。
    let mut gun_nums: Vec<u32> = model_guns.iter()
        .filter_map(|g| g.strip_prefix("gun_")?.parse().ok()).collect();
    gun_nums.sort(); gun_nums.dedup();
    let gun_dense: std::collections::HashMap<u32, u32> =
        gun_nums.iter().enumerate().map(|(i, n)| (*n, i as u32)).collect();
    let mut turret_nums: Vec<u32> = model_turrets.iter()
        .filter_map(|g| g.strip_prefix("turret_")?.parse().ok()).collect();
    turret_nums.sort(); turret_nums.dedup();
    let turret_dense: std::collections::HashMap<u32, u32> =
        turret_nums.iter().enumerate().map(|(i, n)| (*n, i as u32)).collect();

    // 权威模型节点信息（models.pb）；缺失时回退到按 module 去重编号。
    let tmod_info = crate::wargaming::blitzkit::model_info(tank_id);

    // 兜底：按 module_id 去重编号（无 models.pb 时的近似方案）。
    let mut gun_idx_by_module: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut distinct_gun_modules = Vec::new();
    for tur in &tank.turrets {
        for gun in &tur.guns {
            if !gun_idx_by_module.contains_key(&gun.module_id) {
                let idx = distinct_gun_modules.len() as u32;
                gun_idx_by_module.insert(gun.module_id, idx);
                distinct_gun_modules.push(gun.module_id);
            }
        }
    }

    // 定位某炮塔的权威模型节点号；找不到返回 None。
    let turret_model_node = |ti: usize, tmod: u32| -> Option<u32> {
        tmod_info.as_ref().and_then(|mi| mi.turrets.iter().find(|t| t.module_id == tmod))
            .map(|t| t.model_node)
            .or_else(|| Some((ti as u32) + 1))
    };
    // 定位某主炮的权威模型节点号。
    let gun_model_node = |tmod: u32, gmod: u32| -> Option<u32> {
        tmod_info.as_ref().and_then(|mi| mi.turrets.iter().find(|t| t.module_id == tmod))
            .and_then(|t| t.guns.iter().find(|g| g.gun_module_id == gmod))
            .map(|g| g.model_node)
    };

    let mut configs = Vec::new();
    let mut count = 0u32;
    for (ti, tur) in tank.turrets.iter().enumerate() {
        let turret_name = tur.name.clone();
        let turret_weight = Some(tur.weight);
        let turret_traverse = Some(tur.traverse_speed);
        let view_range = Some(tur.view_range);
        // turret_index = 该炮塔模型节点号在升序列表中的稠密下标；无节点映射则回退到 ti。
        let turret_index = turret_model_node(ti, tur.module_id)
            .and_then(|n| turret_dense.get(&n).copied())
            .unwrap_or(ti as u32);
        for gun in &tur.guns {
            // gun_index = 该主炮模型节点号在升序列表中的稠密下标；无权威映射则按 module 去重。
            let gun_index = gun_model_node(tur.module_id, gun.module_id)
                .and_then(|n| gun_dense.get(&n).copied())
                .unwrap_or_else(|| *gun_idx_by_module.get(&gun.module_id).unwrap_or(&0));
            let name = if gun.name.is_empty() { format!("gun_{}", gun_index) } else { gun.name.clone() };
            let caliber = parse_gun_caliber(&name).map(|c| c.round() as u32).unwrap_or(120);
            // 瞄准时间(aim) 与 百米精度(dispersion)：直接来自 tanks.pb（语义正确）
            let aim_time = Some(gun.aim_time);
            let dispersion = Some(gun.dispersion);
            // 装填：直接来自 tanks.pb 解析（单发/弹夹/弹鼓）
            let gr = &gun.reload;
            let reload_time = if gr.reload > 0.0 { Some(gr.reload) } else { None };
            let is_burst = gr.is_burst;
            let burst_size = gr.burst_size;
            let burst_interval = gr.burst_interval;
            let burst_reloads = gr.burst_reloads.clone();
            let is_drum = gr.is_drum;
            let shells: Vec<Value> = gun.shells.iter().map(|s| json!({
                "type": s.shell_type,
                "penetration": s.penetration,
                "damage": s.damage,
                "module_damage": s.module_damage,
            })).collect();
            // 标准弹药伤害 = AP（标准弹）；DPM 以此为基准。无 AP 时回退最高血伤弹。
            let ap_damage = gun.shells.iter()
                .find(|s| s.shell_type == "ap")
                .map(|s| s.damage)
                .or_else(|| {
                    let m = gun.shells.iter().map(|s| s.damage).fold(f64::NEG_INFINITY, f64::max);
                    if m.is_finite() { Some(m) } else { None }
                })
                .unwrap_or(0.0);
            // DPM：单发车 = AP 弹伤 × 60 / 装填秒；弹夹/弹鼓车不显示 DPM
            let dpm = if is_burst { None } else {
                reload_time.filter(|r| *r > 0.0).map(|r| (ap_damage * 60.0 / r).round())
            };

            configs.push(json!({
                "id": count,
                "label": name,
                "caliber": caliber,
                "shells": shells,
                "turret_index": turret_index,
                "gun_index": gun_index,
                "turret_name": turret_name,
                "reload_time": reload_time,
                "aim_time": aim_time,
                "dispersion": dispersion,
                "dpm": dpm,
                "is_burst": is_burst,
                "is_drum": is_drum,
                "burst_size": burst_size,
                "burst_interval": burst_interval,
                "burst_reloads": burst_reloads,
                "turret_weight": turret_weight,
                "turret_traverse_speed": turret_traverse,
                "view_range": view_range,
                // 模型里实际可切换的 gun/turret 组数（前端据此 clamp，避免配置数超出模型节点时错位）
                "model_gun_count": model_guns.len(),
                "model_turret_count": model_turrets.len(),
            }));
            count += 1;
        }
    }
    if configs.is_empty() {
        configs.push(json!({
            "id": 0, "label": "Default", "caliber": 120, "shells": [],
            "turret_index": 0, "gun_index": 0, "turret_name": "",
        }));
    }
    configs
}

/// Parse the bore diameter (mm) from a gun name like "130 mm S-70" or "12,8 cm Kw.K.44".
/// 从枪名解析口径（mm）；BlitzKit 的 caliber 字段非口径，只能从名字里读。
fn parse_gun_caliber(name: &str) -> Option<f64> {
    let lower = name.to_lowercase();
    let (unit_mul, idx) = if let Some(i) = lower.find(" mm") {
        (1.0, i)
    } else {
        let i = lower.find(" cm")?;
        (10.0, i)
    };
    // Walk back over the number (digits, dots, commas)
    let bytes = lower.as_bytes();
    let mut start = idx;
    while start > 0 {
        let c = bytes[start - 1];
        if c.is_ascii_digit() || c == b'.' || c == b',' {
            start -= 1;
        } else {
            break;
        }
    }
    let num = &lower[start..idx].replace(',', ".");
    num.parse::<f64>().ok().map(|v| v * unit_mul)
}

fn bbox_to_json(bbox: &Option<crate::wargaming::dvpl::BoundingBox>) -> Value {
    match bbox {
        Some(b) => json!({
            "min": [b.min[0], b.min[1], b.min[2]],
            "max": [b.max[0], b.max[1], b.max[2]],
        }),
        None => Value::Null,
    }
}

/// Tank list enriched with tier/nation/type metadata (merged from tanks.pb) for filtering.
/// 坦克富元数据列表（id/name/tier/nation/type），供图形化筛选器使用。
pub(crate) async fn tank_filter_handler() -> Json<Value> {
    let mut out: Vec<serde_json::Value> = crate::wargaming::blitzkit::load_tanks()
        .into_values().map(|t| json!({
            "id": t.tank_id,
            "name": if t.name.is_empty() { t.dev_name.clone() } else { t.name },
            "tier": t.tier,
            "nation": t.nation,
            "type": t.tank_type,
        })).collect();

    out.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });
    Json(json!(out))
}

/// 某坦克的弹种数据（类型/穿深/血量伤害/模块伤害），来自 tanks.pb。
pub(crate) async fn shells_handler(axum::extract::Path(tank_id): axum::extract::Path<u32>) -> Json<Value> {
    let result: Value = crate::wargaming::blitzkit::tank_full(tank_id)
        .and_then(|t| t.turrets.first().and_then(|tur| tur.guns.first()).map(|g| {
            let caliber_mm = parse_gun_caliber(&g.name).map(|c| c.round() as u32).unwrap_or(120);
            let shells: Vec<Value> = g.shells.iter().map(|s| json!({
                "type": s.shell_type,
                "name": s.name,
                "penetration": s.penetration,
                "damage": s.damage,
                "module_damage": s.module_damage,
            })).collect();
            json!({ "caliber": caliber_mm, "shells": shells })
        }))
        .unwrap_or(json!({ "caliber": 120, "shells": [] }));
    Json(result)
}

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <link rel="icon" href="data:,">
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WoTB Tank Viewer</title>
    <script>
        window.addEventListener('error', function(e) {
            var el = document.getElementById('loading');
            if (el) { el.textContent = 'JS Error: ' + (e.message || e.error) + ' @ ' + (e.filename || '') + ':' + (e.lineno || ''); el.style.color = '#f44336'; el.style.whiteSpace = 'pre-wrap'; }
        });
        window.addEventListener('unhandledrejection', function(e) {
            var el = document.getElementById('loading');
            if (el) { el.textContent = 'Promise Rejection: ' + (e.reason && (e.reason.message || e.reason)); el.style.color = '#f44336'; el.style.whiteSpace = 'pre-wrap'; }
        });
    </script>
    <style>
        :root {
            --bg:#120f0e; --panel:rgba(27,24,23,0.92); --panel2:rgba(35,31,29,0.92);
            --border:rgba(255,255,255,0.12); --border-hi:rgba(255,138,61,0.45);
            --accent:#ff8a3d; --accent-2:#ffb35c; --accent-3:#ffd29b;
            --green:#5fbf7a; --orange:#ff9800; --red:#ff6b6b; --blue:#5fa8e8; --yellow:#ffcf5c;
            --txt:#f3ede6; --muted:#9c8f7f; --shadow:0 10px 34px rgba(0,0,0,0.5);
            --radius:14px; --radius-sm:9px;
        }
        body { margin: 0; padding: 0; background: var(--bg); color: var(--txt); font-family: system-ui, sans-serif; overflow: hidden; }
        #canvas-container { width: 100vw; height: 100vh; background: radial-gradient(1100px 600px at 30% -10%, #3a2412 0%, transparent 60%), radial-gradient(1000px 600px at 90% 0%, #2f1a0c 0%, transparent 55%); }
        #info-panel {
            position: fixed; top: 20px; left: 20px;
            background: var(--panel); padding: 20px; border-radius: var(--radius);
            max-width: 350px; backdrop-filter: blur(12px);
            border: 1px solid var(--border); box-shadow: var(--shadow);
        }
        #info-panel h1 { font-size: 1.5em; margin: 0 0 10px 0; color: var(--accent-3); }
        #info-panel .stat { display: flex; justify-content: space-between; margin: 4px 0; }
        #info-panel .label { color: var(--muted); }
        #info-panel .value { font-weight: bold; }
        #armor-section { margin-top: 15px; padding-top: 10px; border-top: 1px solid var(--border); }
        #armor-section h2 { font-size: 1.1em; margin: 0 0 8px 0; color: var(--accent-2); }
        .armor-row { display: flex; justify-content: space-between; margin: 2px 0; font-size: 0.9em; }
        .armor-front { color: var(--green); }
        .armor-sides { color: var(--orange); }
        .armor-rear { color: var(--red); }
        #armor-table { margin-top: 10px; padding-top: 10px; border-top: 1px solid var(--border); }
        #armor-table table { width: 100%; font-size: 0.85em; border-collapse: collapse; }
        #armor-table th { text-align: left; color: var(--muted); padding: 2px 6px; border-bottom: 1px solid var(--border); }
        #armor-table td { padding: 2px 6px; }
        .plate-thick { color: var(--green); font-weight: bold; }
        .plate-thin { color: var(--red); }
        #loading { position: fixed; top: 50%; left: 50%; transform: translate(-50%,-50%); font-size: 1.2em; color: var(--accent-3); }
        #controls-hint { position: fixed; bottom: 20px; right: 20px; font-size: 0.8em; color: var(--muted); }
        #turret-controls {
            position: fixed; bottom: 20px; left: 20px;
            background: var(--panel); padding: 12px 16px; border-radius: var(--radius);
            backdrop-filter: blur(12px); border: 1px solid var(--border); box-shadow: var(--shadow);
            display: none; min-width: 280px;
        }
        .ctrl-row { display: flex; align-items: center; gap: 8px; margin: 4px 0; font-size: 0.85em; }
        .ctrl-row label { width: 50px; color: var(--muted); }
        .ctrl-row span { color: var(--accent); font-weight: bold; }
        #click-info {
            position: fixed;
            background: var(--panel); padding: 12px 16px; border-radius: var(--radius-sm);
            backdrop-filter: blur(12px); border: 1px solid var(--border-hi); box-shadow: var(--shadow);
            display: none; min-width: 200px; pointer-events: none; z-index: 100;
        }
        #click-info h3 { margin: 0 0 8px 0; font-size: 1em; color: var(--accent-3); }
        #click-info .row { display: flex; justify-content: space-between; margin: 3px 0; font-size: 0.9em; }
        #click-info .pen { color: var(--green); font-weight: bold; }
        #click-info .bounce { color: var(--red); font-weight: bold; }
        #click-info .ricochet { color: var(--orange); font-weight: bold; }
        #shell-selector {
            position: fixed; top: 20px; right: 20px;
            background: var(--panel); padding: 10px 15px; border-radius: var(--radius-sm);
            backdrop-filter: blur(12px); border: 1px solid var(--border); box-shadow: var(--shadow);
        }
        #shell-selector select { background: #2c2724; color: var(--txt); border: 1px solid var(--border-hi); border-radius: var(--radius-sm); padding: 4px 9px; }
        #tank-selectors {
            position: fixed; top: 20px; left: 390px;
            background: var(--panel); padding: 12px 16px; border-radius: var(--radius);
            backdrop-filter: blur(12px); border: 1px solid var(--border); box-shadow: var(--shadow);
            z-index: 20; width: 270px;
        }
        #tank-selectors .sel-row { display: flex; align-items: center; gap: 8px; margin: 6px 0; font-size: 0.85em; }
        #tank-selectors label { width: 58px; color: var(--muted); font-size: 0.8em; }
        #tank-selectors .tank-btn {
            background: #2c2724; color: var(--txt); border: 1px solid var(--border); border-radius: var(--radius-sm);
            padding: 5px 10px; max-width: 176px; cursor: pointer; font-size: 0.85em;
            text-align: center; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; transition: all .12s ease;
        }
        #tank-selectors .tank-btn:hover { border-color: var(--accent); background: #35302c; }
        #shooter-label { color: var(--accent-2); }
        #target-label { color: var(--green); }

        /* Tank picker modal */
        #tank-picker {
            position: fixed; top: 50%; left: 50%; transform: translate(-50%,-50%);
            width: min(1060px, 94vw); height: min(700px, 88vh);
            background: var(--panel); border: 1px solid var(--border-hi);
            border-radius: var(--radius); z-index: 1000; display: none; flex-direction: column;
            box-shadow: 0 16px 70px rgba(0,0,0,0.7); backdrop-filter: blur(14px);
        }
        #tank-picker.open { display: flex; }
        #tp-header { display: flex; align-items: center; gap: 10px; padding: 12px 16px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
        #tp-title { font-size: 1em; font-weight: bold; color: var(--accent-3); }
        #tp-search { flex: 1; min-width: 140px; background: #2c2724; color: var(--txt); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 5px 10px; font-size: 0.85em; }
        #tp-header select { background: #2c2724; color: var(--txt); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 4px 8px; font-size: 0.82em; }
        #tp-count { color: var(--accent); font-size: 0.8em; }
        #tp-close { background: none; border: none; color: var(--muted); font-size: 1.4em; cursor: pointer; line-height: 1; }
        #tp-close:hover { color: #fff; }
        #tp-grid { flex: 1; overflow-y: auto; padding: 12px 16px; display: flex; flex-wrap: wrap; gap: 12px; align-content: flex-start; }
        .tank-card {
            flex: 0 0 196px; max-width: 196px;
            background: linear-gradient(180deg,#251f1c,#1b1715); border: 1px solid var(--border); border-radius: var(--radius-sm);
            overflow: hidden; cursor: pointer; transition: transform 0.08s, border-color 0.08s, box-shadow 0.08s;
        }
        .tank-card:hover { transform: translateY(-2px); border-color: var(--accent); box-shadow: var(--shadow); }
        .tank-card.sel { border-color: var(--accent); box-shadow: 0 0 0 2px rgba(255,138,61,0.4); }
        .tank-card .tc-img { width: 100%; height: 132px; object-fit: contain; background: linear-gradient(180deg,#211b17,#171310); display: block; padding: 4px; }
        .tank-card .tc-body { padding: 6px 8px; }
        .tank-card .tc-name { font-size: 0.84em; color: var(--txt); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
        .tank-card .tc-meta { display: flex; justify-content: space-between; align-items: center; gap: 4px; margin-top: 4px; font-size: 0.74em; }
        .tank-card .tc-tier { color: var(--yellow); font-weight: bold; }
        .tank-card .tc-type { color: var(--muted); }
        .tank-card .tc-nation { color: var(--green); }
        #view-toggle {
            position: fixed; top: 20px; right: 20px; margin-top: 45px;
            background: var(--panel); padding: 8px 14px; border-radius: var(--radius-sm);
            backdrop-filter: blur(12px); border: 1px solid var(--border); box-shadow: var(--shadow);
            display: flex; gap: 8px; flex-wrap: wrap;
        }
        #view-toggle button { background: #35302c; color: var(--txt); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 4px 12px; cursor: pointer; font-size: 0.85em; transition: all .12s ease; }
        #view-toggle button:hover { border-color: var(--accent); }
        #view-toggle button.active { background: linear-gradient(135deg,var(--accent),var(--accent-2)); color: #1a1208; border-color: transparent; }
    </style>
</head>
<body>
    <div id="loading">Loading tank model...</div>
    <div id="canvas-container"></div>
    <div id="info-panel" style="display:none;">
        <h1 id="tank-name">Loading...</h1>
        <div class="stat"><span class="label">Tier</span><span class="value" id="tank-tier"></span></div>
        <div class="stat"><span class="label">Type</span><span class="value" id="tank-type"></span></div>
        <div class="stat"><span class="label">Nation</span><span class="value" id="tank-nation"></span></div>
        <div id="armor-section">
            <h2>Armor (mm)</h2>
            <div class="armor-row"><span>Front</span><span class="armor-front" id="armor-front"></span></div>
            <div class="armor-row"><span>Sides</span><span class="armor-sides" id="armor-sides"></span></div>
            <div class="armor-row"><span>Rear</span><span class="armor-rear" id="armor-rear"></span></div>
        </div>
    </div>
    <div id="shell-selector" style="display:none;">
        <label style="font-size:0.85em;">Shell: </label>
        <select id="shell-select"></select>
    </div>
    <div id="view-toggle">
        <button id="collision-btn">Show Collision</button>
    </div>
    <div id="tank-selectors">
        <div class="sel-row" id="config-row" style="display:none;"><label id="config-label">Config:</label><select id="config-select"></select></div>
        <div class="sel-row"><label id="shooter-label">Shooter:</label><button class="tank-btn" id="shooter-select">—</button></div>
        <div class="sel-row"><label id="target-label">Target:</label><button class="tank-btn" id="target-select">—</button></div>
    </div>
    <div id="tank-picker">
        <div id="tp-header">
            <span id="tp-title">Select Tank</span>
            <input type="text" id="tp-search" placeholder="Search tank...">
            <select id="tp-tier"></select>
            <select id="tp-nation"></select>
            <select id="tp-type"></select>
            <span id="tp-count"></span>
            <button id="tp-close" title="Close">×</button>
        </div>
        <div id="tp-grid"></div>
    </div>
        <div id="click-info">
            <h3 id="click-part">—</h3>
            <div class="row"><span>Base armor</span><span id="click-armor">—</span></div>
            <div class="row"><span>Angle</span><span id="click-angle">—</span></div>
            <div class="row"><span>Effective</span><span id="click-effective">—</span></div>
            <div class="row"><span>Penetration</span><span id="click-pen">—</span></div>
            <div class="row"><span>Result</span><span id="click-result">—</span></div>
        </div>
    <div id="controls-hint">Drag to rotate · Scroll to zoom · Left-click: armor · Right-drag: turret/gun</div>
    <div id="traj-info" style="display:none;position:fixed;z-index:200;pointer-events:none;"></div>
    <div id="turret-controls">
        <div class="ctrl-row"><label>Turret</label><span id="turret-val">0°</span></div>
        <div class="ctrl-row"><label>Gun</label><span id="gun-val">0°</span></div>
    </div>
    <script type="importmap">
    __IMPORTMAP__
    </script>
    <script>
      window.__INITIAL_TANK__ = __INITIAL_TANK_VALUE__;
      window.__INITIAL_SHOOTER__ = __INITIAL_SHOOTER_VALUE__;
    </script>
    <script type="module">
        import * as THREE from 'three';
        import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
        import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';

        let scene, camera, renderer, controls;
        let raycaster, mouse;
        let tankModel = null, armorModel = null, tankData;   // tankData = target tank (model/armor/info)
        let shooterData = null;                              // shooter tank (caliber/shells)
        let shooterShells = [], shooterCaliber = 120;
        let selectedShell = null;
        let moduleMeshes = [];

        function tidyTrajectory() {
            if (trajGroup) { scene.remove(trajGroup); trajGroup = null; }
            trajInfoPos = null;
            document.getElementById('traj-info').style.display = 'none';
            document.getElementById('click-info').style.display = 'none';
        }

        function getPlateThickness(section, plateId) {
            // armor_cache.json 是精简数据源（会省略 0 厚度的装饰性板）；缺失时回退到
            // game_data 提取的 armor_model（更权威、含全部板）。两者都可能没有（返回 null）。
            const ap = tankData.armor_plates;
            if (section === 'hull') return ap?.hull_plates?.[plateId] ?? tankData.armor_model?.hull?.plates?.[plateId] ?? null;
            if (section === 'turret') return ap?.turret_plates?.[plateId] ?? tankData.armor_model?.turret?.plates?.[plateId] ?? null;
            if (section === 'gun') return ap?.gun_plates?.[plateId] ?? tankData.armor_model?.gun?.plates?.[plateId] ?? null;
            if (section === 'chassis') {
                const ch = tankData.armor_model?.chassis;
                if (!ch) return null;
                if (plateId === 'leftTrack') return ch.left_track;
                if (plateId === 'rightTrack') return ch.right_track;
            }
            if (section === 'gunBarrel') {
                const gp = tankData.armor_model?.gun?.plates;
                return gp?.['gun'] ?? null;
            }
            return null;
        }

        // 0/负厚度或缺失的板是游戏里的"装饰性/非碰撞"网格，不是有效装甲：
        // 不应参与穿透判定，也不应作为阻挡面渲染。
        function isRealArmorThickness(t) {
            return typeof t === 'number' && t > 0;
        }

        function thicknessToColor(t) {
            if (t === null || t === undefined) return 0x666666;
            if (t >= 200) return 0x8B0000;
            if (t >= 100) return 0xFF4500;
            if (t >= 60) return 0xFFA500;
            if (t >= 30) return 0xFFD700;
            return 0x228B22;
        }

        function tagArmorPlates(model) {
            const hullSpaced = new Set(tankData.armor_model?.hull?.spaced || []);
            const turretSpaced = new Set(tankData.armor_model?.turret?.spaced || []);
            const gunSpaced = new Set(tankData.armor_model?.gun?.spaced || []);
            model.traverse(function(node) {
                if (!node.isMesh) return;
                const name = node.name || '';
                const m = name.match(/(hull|turret|gun)_\w*?_?armor_(\d+)/);
                if (m) {
                    const section = m[1], plateId = m[2];
                    const t = getPlateThickness(section, plateId);
                    // 0/负厚度或缺失 → 装饰性非装甲网格：不参与穿透判定、不作为装甲面渲染。
                    if (!isRealArmorThickness(t)) {
                        node.userData.armorSection = 'deco';
                        node.userData.armorPlateId = plateId;
                        node.userData.armorThickness = 0;
                        return;
                    }
                    let actualSection = section;
                    // spaced 板是 flat 累积层（不计伤害）。
                    if (section === 'hull' && hullSpaced.has(plateId)) actualSection = 'spaced';
                    if (section === 'turret' && turretSpaced.has(plateId)) actualSection = 'spaced';
                    if (section === 'gun' && gunSpaced.has(plateId)) actualSection = 'spaced';
                    // 炮盾(gun 非 spaced 板)是主装甲：走角度等效+跳弹，等价于 turret 的判定。
                    // 记录原始 section 供 partName 显示；armorSection 用 'turret' 让后端按主装甲处理。
                    node.userData.armorSectionOrig = section;
                    node.userData.armorSection = (section === 'gun' && actualSection !== 'spaced') ? 'turret' : actualSection;
                    node.userData.armorPlateId = plateId;
                    node.userData.armorThickness = t;
                }
            });
        }

        function tagModuleMeshes(model) {
            moduleMeshes = [];
            model.traverse(function(node) {
                if (!node.isMesh) return;
                const parent = node.parent;
                const parentName = parent ? (parent.name || '') : '';
                if (parentName === 'chassis_track_L') {
                    node.userData.armorSection = 'chassis';
                    node.userData.armorPlateId = 'leftTrack';
                    node.userData.armorThickness = getPlateThickness('chassis', 'leftTrack');
                    moduleMeshes.push(node);
                } else if (parentName === 'chassis_track_R') {
                    node.userData.armorSection = 'chassis';
                    node.userData.armorPlateId = 'rightTrack';
                    node.userData.armorThickness = getPlateThickness('chassis', 'rightTrack');
                    moduleMeshes.push(node);
                } else if (/^gun_\d+/.test(parentName)) {
                    node.userData.armorSection = 'gunBarrel';
                    node.userData.armorPlateId = 'gun';
                    node.userData.armorThickness = getPlateThickness('gunBarrel', 'gun');
                    // 记录所属 gun 配置组序号(如 gun_04→4)，供按配置过滤 raycast 与可见性。
                    const gm = parentName.match(/^gun_(\d+)/);
                    node.userData.gunConfig = gm ? parseInt(gm[1], 10) : null;
                    moduleMeshes.push(node);
                }
            });
        }

        // 模块装甲（炮塔/炮盾）与视觉模型的对齐统一由 alignArmorModules() 完成；
        // 车体(hull) 的 game points 恒接近 0，无需偏移，故不再对碰撞模型做预平移。
        function applyModelTransforms(model) {
            model.rotation.x = -Math.PI / 2;
            const box = new THREE.Box3().setFromObject(model);
            const center = box.getCenter(new THREE.Vector3());
            const size = box.getSize(new THREE.Vector3());
            const maxDim = Math.max(size.x, size.y, size.z);
            const scale = 6 / maxDim;
            model.scale.setScalar(scale);
            const box2 = new THREE.Box3().setFromObject(model);
            const center2 = box2.getCenter(new THREE.Vector3());
            const min2 = box2.min.clone();
            model.position.sub(center2);
            model.position.y += (center2.y - min2.y);
        }

        function syncTransforms() {
            if (!tankModel || !armorModel) return;
            armorModel.rotation.copy(tankModel.rotation);
            armorModel.scale.copy(tankModel.scale);
            armorModel.position.copy(tankModel.position);
            collectConfigNodes(tankModel);
            alignArmorModules();
        }

        // Align the armor/collision model's module armor (turret / gun) with the visual model.
        // 某些车辆(如 E 100)的 collision.glb 将模块装甲放在模型原点附近，因此需要从
        // 视觉模型对应节点推导偏移。
        //
        // 关键(多主炮通用)：炮盾(枪座/掩体)对应视觉模型的 `gun_0X_mask` 节点，而不是整个
        // `gun_0X` 组——后者含长炮管，其包围盒中心偏向前方，会把 armor 炮盾错误拉向炮管中段。
        // 因此 gun 的 armor 优先对齐到 `gun_0X_mask`(炮盾根部)；turret 对齐到 `turret_0X`。
        function alignArmorModules() {
            if (!armorModel || !tankModel) return;
            armorModel.updateMatrixWorld(true);
            tankModel.updateMatrixWorld(true);

            // 取视觉模型某节点的几何包围盒中心（世界空间）。找不到返回 null。
            const visualCenterOf = (namePrefix) => {
                const root = tankModel.children[0] || tankModel;
                let grp = null;
                for (const c of root.children) { if ((c.name || '') === namePrefix) { grp = c; break; } }
                if (!grp) return null;
                const b = new THREE.Box3(); let any = false;
                const isNonBody = (n) => /hide_elements|_nc|_switch/i.test(n.name || '');
                (function walk(n) {
                    if (isNonBody(n)) return;   // 跳过隐藏/动画/低模子树
                    if (n.isMesh) { b.union(new THREE.Box3().setFromObject(n)); any = true; }
                    n.children.forEach(walk);
                })(grp);
                return any ? b.getCenter(new THREE.Vector3()) : null;
            };

            // 通病修复：无 `gun_0X_mask` 的坦克（如 112 Glacial），若把炮盾 armor 对齐到整根
            // `gun_0X` 组（含长炮管）的几何中心，会被拉向炮管中段，导致炮盾错位。
            // 此时应以"炮管根部"（炮塔侧一端）作锚点。这里返回视觉 gun 组的 bbox 在 z 上
            // 更靠近炮塔中心的那一端（世界空间），用于对齐炮盾；有 mask 时仍优先用 mask 中心。
            // 注意传入的可能是 `gun_01` / `gun_01_mask`，这里统一落到 `gun_01` 实体组（去除 _mask 后缀）。
            const visualGunRootOf = (namePrefix) => {
                const gunGroupName = namePrefix.replace(/_mask$/, '');
                const root = tankModel.children[0] || tankModel;
                let grp = null;
                for (const c of root.children) { if ((c.name || '') === gunGroupName) { grp = c; break; } }
                if (!grp) return null;
                const b = new THREE.Box3(); let any = false;
                const isNonBody = (n) => /hide_elements|_nc|_switch/i.test(n.name || '');
                (function walk(n) {
                    if (isNonBody(n)) return;
                    if (n.isMesh) { b.union(new THREE.Box3().setFromObject(n)); any = true; }
                    n.children.forEach(walk);
                })(grp);
                if (!any) return null;
                const turretCenter = visualCenterOf(turretPrefixFor(gunGroupName));
                const xMid = (b.min.x + b.max.x) / 2, yMid = (b.min.y + b.max.y) / 2;
                if (turretCenter) {
                    const zMin = Math.abs(b.min.z - turretCenter.z);
                    const zMax = Math.abs(b.max.z - turretCenter.z);
                    return new THREE.Vector3(xMid, yMid, zMin < zMax ? b.min.z : b.max.z);
                }
                return new THREE.Vector3(xMid, yMid, b.max.z);
            };

            // 由 gun 前缀推导其所属炮塔分组名（gun_01 → 找 turret_01；gun_02 → turret_02 …）。
            const turretPrefixFor = (gunPrefix) => {
                const m = gunPrefix.match(/^gun_(\d+)$/);
                return m ? ('turret_' + m[1]) : null;
            };

            // 通病修复：部分坦克（如 112 Glacial）的碰撞炮塔 armor 只有壳体薄片、比视觉炮塔矮，
            // 若按"几何中心"对齐会导致炮塔底部悬空（悬浮在车体上方）。因此对炮塔改用"底部对齐"：
            // 让碰撞炮塔 armor 的底沿(min.y)对齐到视觉炮塔的底沿(min.y)，使炮塔坐上车体顶面。
            // 对本来就贴地的坦克(IS-7/E-100 等)差值≈0，属幂等微调，不产生回归。
            const visualBottomYOf = (namePrefix) => {
                const root = tankModel.children[0] || tankModel;
                let grp = null;
                for (const c of root.children) { if ((c.name || '') === namePrefix) { grp = c; break; } }
                if (!grp) return null;
                const b = new THREE.Box3(); let any = false;
                const isNonBody = (n) => /hide_elements|_nc|_switch/i.test(n.name || '');
                (function walk(n) {
                    if (isNonBody(n)) return;
                    if (n.isMesh) { b.union(new THREE.Box3().setFromObject(n)); any = true; }
                    n.children.forEach(walk);
                })(grp);
                return any ? b.min.y : null;
            };

            // 把给定 armor mesh 组对齐到视觉节点；`useGunRoot` 用炮管根部锚点，
            // `alignBottom` 时按底沿(min.y)对齐（炮塔坐上车体），否则按几何中心对齐。
            const alignGroup = (visualPrefix, meshes, useGunRoot, alignBottom) => {
                let target;
                if (useGunRoot) {
                    target = visualGunRootOf(visualPrefix);
                    if (!target) return;
                    const b = new THREE.Box3();
                    meshes.forEach(m => b.union(new THREE.Box3().setFromObject(m)));
                    const deltaW = target.clone().sub(b.getCenter(new THREE.Vector3()));
                    meshes.forEach(m => {
                        if (!m.parent) return;
                        const wp = new THREE.Vector3();
                        m.getWorldPosition(wp);
                        wp.add(deltaW);
                        const lp = m.parent.worldToLocal(wp);
                        m.position.copy(lp);
                    });
                    return;
                }
                if (alignBottom) {
                    // 底部对齐：碰撞装甲组底沿(min.y)对齐到视觉节点底沿(min.y)，x/z 用中心对齐。
                    const b = new THREE.Box3();
                    meshes.forEach(m => b.union(new THREE.Box3().setFromObject(m)));
                    const ac = b.getCenter(new THREE.Vector3());
                    const visC = visualCenterOf(visualPrefix);
                    if (!visC) return;
                    const dx = visC.x - ac.x;
                    const dz = visC.z - ac.z;
                    const bottomTarget = visualBottomYOf(visualPrefix);
                    if (bottomTarget == null) return;
                    const dy = bottomTarget - b.min.y;
                    meshes.forEach(m => {
                        if (!m.parent) return;
                        const wp = new THREE.Vector3();
                        m.getWorldPosition(wp);
                        wp.add(new THREE.Vector3(dx, dy, dz));
                        const lp = m.parent.worldToLocal(wp);
                        m.position.copy(lp);
                    });
                    return;
                }
                target = visualCenterOf(visualPrefix);
                if (!target || !meshes.length) return;
                const b = new THREE.Box3();
                meshes.forEach(m => b.union(new THREE.Box3().setFromObject(m)));
                const ac = b.getCenter(new THREE.Vector3());
                const deltaW = target.clone().sub(ac);
                meshes.forEach(m => {
                    if (!m.parent) return;
                    const wp = new THREE.Vector3();
                    m.getWorldPosition(wp);
                    wp.add(deltaW);
                    const lp = m.parent.worldToLocal(wp);
                    m.position.copy(lp);
                });
            };

            // 按 armor 前缀收集对应的 armor mesh（匹配 `gun_0X_armor_` / `turret_0X_armor`）。
            const armorMeshesByPrefix = (prefix) => {
                const meshes = [];
                armorModel.traverse(n => {
                    if (n.isMesh && (n.name || '').startsWith(prefix + '_armor')) meshes.push(n);
                });
                return meshes;
            };

            // 逐前缀对齐：gun 优先对齐到 mask(炮盾)，turret/hull 对齐到自身组。
            // 通病修复：某些坦克(如 112 Glacial / AC IV Sentinel / WZ-113G FT / A-32)的
            // collision.glb 把 hull_armor 放在与原点的偏移处(z 轴差 >0.6)，而 hull 此前从不
            // 对齐，导致碰撞车体整体与视觉模型错位。现对 hull 也做统一对齐；
            // 对本来无偏移的坦克，delta≈0，属幂等操作，不产生回归。
            const seen = new Set();
            armorModel.traverse(n => {
                if (!n.isMesh) return;
                const name = n.name || '';
                const hm = name.match(/^(hull)_armor/);
                const gm = name.match(/^(turret_\d+)_armor/);
                const gg = name.match(/^(gun_\d+)_armor/);
                const prefix = gm ? gm[1] : (gg ? gg[1] : (hm ? hm[1] : null));
                if (!prefix || seen.has(prefix)) return;
                seen.add(prefix);
                const meshes = armorMeshesByPrefix(prefix);
                if (!meshes.length) return;
                const isGun = !!gg;
                const isTurret = !!gm;
                const isHull = !!hm;
                const visPrefix = isGun ? prefix + '_mask' : prefix;
                // 通病修复：gun 若没有独立 mask 节点（如 112 Glacial），对齐到整个 gun 组中心
                // 会被长炮管拉偏；改用"炮管根部"（靠近炮塔侧端点）做锚点。有 mask 时仍用 mask 中心。
                // turret/hull 用底部对齐，使碰撞车体/炮塔贴合地面与车体，避免悬浮或略微上浮。
                const hasMask = isGun && !!visualCenterOf(visPrefix);
                alignGroup(visPrefix, meshes, isGun && !hasMask, isTurret || isHull);
            });
        }

        function clearModels() {
            if (tankModel) { scene.remove(tankModel); tankModel = null; }
            if (armorModel) { scene.remove(armorModel); armorModel = null; }
            if (trajGroup) { scene.remove(trajGroup); trajGroup = null; }
            moduleMeshes = [];
            turretNode = null; gunNodesList = [];
            configGunGroups = []; configTurretNodes = [];
            origMatrices = null; armorOrigMatrices = null;
        }

        function loadModels() {
            document.getElementById('loading').style.display = 'block';
            document.getElementById('loading').textContent = 'Loading tank model...';
            window.__LOAD__ = 'start';
            clearModels();
            const loader = new GLTFLoader();
            // 加载失败时把可见错误写入 #loading（便于排查网络/路径问题）
            const fail = (phase) => (error) => {
                const msg = (typeof error === 'string') ? error : (error && (error.message || error.statusText || String(error))) || 'unknown';
                window.__LOAD__ = 'fail:' + phase + ':' + msg;
                console.error('Failed to load ' + phase + ':', error);
                document.getElementById('loading').textContent = 'Failed to load ' + phase + ': ' + msg;
            };

            // Load armor model (hidden, for raycasting)
            loader.load(tankData.model_url, function(gltf) {
                armorModel = gltf.scene;
                tagArmorPlates(armorModel);
                // Keep visible for raycasting but make transparent so it doesn't render visually
                armorModel.traverse(function(node) {
                    if (node.isMesh) {
                        // 0 厚度的装饰性非装甲网格：默认隐藏（不显示、不参与判定）。
                        if (node.userData.armorSection === 'deco') { node.visible = false; return; }
                        node.material = new THREE.MeshStandardMaterial({
                            color: 0x444444, metalness: 0.3, roughness: 0.8,
                            transparent: true, opacity: 0, depthWrite: false,
                        });
                    }
                });
                scene.add(armorModel);
                syncTransforms();
                applyConfig(currentConfigIdx);
            }, undefined, fail('armor model'));

            // Load visual model (visible)
            loader.load(tankData.visual_model_url, function(gltf) {
                tankModel = gltf.scene;
                applyModelTransforms(tankModel);
                tagModuleMeshes(tankModel);
                tankModel.traverse(function(node) {
                    if (node.isMesh) {
                        node.castShadow = true;
                        node.receiveShadow = true;
                    }
                });
                scene.add(tankModel);
                document.getElementById('loading').style.display = 'none';
                window.__LOAD__ = 'ok:' + tankData.tank_id;
                controls.target.set(0, 1, 0);
                controls.update();
                syncTransforms();
                applyConfig(currentConfigIdx);
            }, undefined, fail('tank model'));
        }

        function updateInfoPanel() {
            document.getElementById('tank-name').textContent = tankData.name || '?';
            document.getElementById('tank-tier').textContent = 'Tier ' + (tankData.tier || '?');
            document.getElementById('tank-type').textContent = tankData.type || '?';
            document.getElementById('tank-nation').textContent = tankData.nation || '?';
            if (tankData.armor) {
                const a = tankData.armor;
                document.getElementById('armor-front').textContent =
                    `Turret ${a.turret.front} / Hull ${a.hull.front}`;
                document.getElementById('armor-sides').textContent =
                    `Turret ${a.turret.sides} / Hull ${a.hull.sides}`;
                document.getElementById('armor-rear').textContent =
                    `Turret ${a.turret.rear} / Hull ${a.hull.rear}`;
            }
            document.getElementById('info-panel').style.display = 'block';
        }

        async function loadTarget(tid) {
            tidyTrajectory();
            tankData = await (await fetch('/api/tank/' + tid)).json();
            // 支持 URL ?config=N 指定初始模块（百科详情联动 3D 检视用）。
            const q = new URLSearchParams(location.search);
            const wantCfg = parseInt(q.get('config'), 10);
            currentConfigIdx = (Number.isInteger(wantCfg) && wantCfg >= 0 && wantCfg < tankData.configs.length) ? wantCfg : 0;
            setupConfigSelect();
            updateInfoPanel();
            loadModels();
        }

        function currentConfig() {
            return (tankData && tankData.configs && tankData.configs[currentConfigIdx]) || null;
        }

        // 当前激活配置对应的 gun 组节点名前缀编号(如 gun_04→4)，用于过滤 moduleMeshes。
        // 多主炮车辆切换配置时，未选中的那根炮管应被剔除(不显示、不参与 raycast)。
        const activeGunNumber = () => {
            const cfg = currentConfig();
            if (!cfg) return null;
            const grp = configGunGroups[cfg.gun_index % Math.max(1, configGunGroups.length)];
            if (!grp) return null;
            for (const n of grp) {
                const m = (n.name || '').match(/^gun_(\d+)$/);
                if (m) return parseInt(m[1], 10);
            }
            return null;
        };

        // Populate the configuration (turret/gun) selector from tankData.configs.
        function setupConfigSelect() {
            const sel = document.getElementById('config-select');
            const row = document.getElementById('config-row');
            const cfgs = tankData && tankData.configs ? tankData.configs : [];
            if (cfgs.length <= 1) { row.style.display = 'none'; return; }
            row.style.display = 'flex';
            sel.innerHTML = '';
            cfgs.forEach((c, i) => {
                const o = document.createElement('option');
                o.value = i;
                o.textContent = c.label + (c.turret_name && c.turret_name !== c.label ? ' (' + c.turret_name + ')' : '');
                sel.appendChild(o);
            });
            sel.value = String(currentConfigIdx);
        }

        // Switch to a configuration: show its gun/turret nodes, update shells + caliber + gun armor.
        function applyConfig(idx) {
            currentConfigIdx = idx;
            const cfg = currentConfig();
            if (!cfg) return;
            document.getElementById('config-select').value = String(idx);

            // Show the selected gun/turret config nodes on the visual model.
            collectConfigNodes(tankModel);
            applyConfigVisible(tankModel, cfg.gun_index, cfg.turret_index);
            // Armor model: hide non-selected gun/turret config armor meshes.
            applyArmorConfigVisible(cfg);
            // Align armor module (turret/gun) with the visual model (fixes missing points).
            alignArmorModules();
            // Reset turret/gun matrices for the newly active nodes.
            origMatrices = null;
            armorOrigMatrices = null;
            collectTurretGunNodes();
            collectArmorNodes();
            computePivots();
            // 重放当前炮塔/主炮角度：切换配置后 pivot 已更新，需把已有的旋转/俯仰重新应用，
            // 否则配置切换会丢失用户当前调整的炮塔/炮管角度。
            updateTurretGun(currentTurretDeg, currentGunDeg);

            // When the shooter is the tank being viewed, its shells/caliber follow the config.
            if (shooterData && tankData && shooterData.tank_id === tankData.tank_id) {
                shooterShells = cfg.shells || [];
                shooterCaliber = cfg.caliber || shooterCaliber;
                populateShellSelector(shooterShells);
                selectedShell = shooterShells.length ? shooterShells[0] : null;
            }
        }

        // For the (hidden) collision/armor model, show only the selected config's
        // gun and turret armor meshes; hide the others. Gun/Turret armor meshes are named
        // like "gun_04_armor_3" / "turret_02_armor_1".
        function applyArmorConfigVisible(cfg) {
            if (!armorModel) { return; }
            const gunPk = collectGunArmorPrefixes();
            const turPk = collectTurretArmorPrefixes();
            const activeGun = gunPk.length ? gunPk[cfg.gun_index % gunPk.length] : null;
            const activeTur = turPk.length ? turPk[cfg.turret_index % turPk.length] : null;
            armorModel.traverse(function(node) {
                if (!node.isMesh) return;
                const name = node.name || '';
                const gm = name.match(/^(gun_\d+)_armor_/);
                const tm = name.match(/^(turret_\d+)_armor_/);
                if (gm) {
                    const visible = activeGun ? gm[1] === activeGun : true;
                    node.visible = visible;
                    node.userData.configHidden = !visible;
                    if (visible) node.userData.armorSection = node.userData.armorSection || 'gun';
                } else if (tm) {
                    const visible = activeTur ? tm[1] === activeTur : true;
                    node.visible = visible;
                    node.userData.configHidden = !visible;
                    if (visible) node.userData.armorSection = node.userData.armorSection || 'turret';
                }
            });
        }

        function populateShellSelector(shells) {
            const sel = document.getElementById('shell-select');
            sel.innerHTML = '';
            shells.forEach((s, i) => {
                const opt = document.createElement('option');
                opt.value = i;
                opt.textContent = `${s.type || s.name || '?'} ${s.penetration || 0}mm / ${s.damage || 0}dmg`;
                sel.appendChild(opt);
            });
            selectedShell = shells.length > 0 ? shells[0] : null;
            document.getElementById('shell-selector').style.display = shells.length > 0 ? 'block' : 'none';
        }

        async function loadShooter(tid) {
            shooterData = await (await fetch('/api/tank/' + tid)).json();
            let shells = shooterData.shells || [];
            let caliber = shooterData.caliber || 120;
            if (!shells || shells.length === 0) {
                const resp = await fetch('/api/shells/' + tid);
                const data = await resp.json();
                if (Array.isArray(data)) {
                    shells = data;
                } else {
                    shells = data.shells || [];
                    caliber = data.caliber || 120;
                }
            }
            shooterShells = shells;
            shooterCaliber = caliber;
            populateShellSelector(shells);
        }

        let tanksList = [];
        let currentShooterId = null, currentTargetId = null;
        let pickerMode = 'target'; // which selector the picker is currently editing

        // Populate the tier/nation/type filter dropdowns (in the picker modal).
        function populateFilterOptions() {
            const tiers = [...new Set(tanksList.map(t => t.tier))].sort((a, b) => a - b);
            const nations = [...new Set(tanksList.map(t => t.nation))].sort();
            const types = [...new Set(tanksList.map(t => t.type))].sort();
            const addOpts = (selId, items, labelFn, defLabel) => {
                const sel = document.getElementById(selId);
                sel.innerHTML = '';
                const def = document.createElement('option');
                def.value = '';
                def.textContent = defLabel;
                sel.appendChild(def);
                items.forEach(v => {
                    const o = document.createElement('option');
                    o.value = String(v);
                    o.textContent = labelFn(v);
                    sel.appendChild(o);
                });
            };
            addOpts('tp-tier', tiers, v => 'Tier ' + v, 'Tier');
            addOpts('tp-nation', nations, v => v, 'Nation');
            addOpts('tp-type', types,
                v => ({lightTank:'Light', mediumTank:'Medium', heavyTank:'Heavy', 'AT-SPG':'TD'}[v] || v), 'Type');
        }

        const TYPE_LABEL = { lightTank:'Light', mediumTank:'Medium', heavyTank:'Heavy', 'AT-SPG':'TD' };

        // Compute the filtered tank list from the picker's filter inputs.
        function getFiltered() {
            const q = document.getElementById('tp-search').value.trim().toLowerCase();
            const tier = document.getElementById('tp-tier').value;
            const nation = document.getElementById('tp-nation').value;
            const type = document.getElementById('tp-type').value;
            return tanksList.filter(t =>
                (!q || (t.name || '').toLowerCase().includes(q)) &&
                (!tier || String(t.tier) === tier) &&
                (!nation || t.nation === nation) &&
                (!type || t.type === type)
            );
        }

        // Render the graphical tank grid into the picker modal.
        let renderChunk = null;   // { list, rendered, selId }

        function makeCard(t, selId) {
            const card = document.createElement('div');
            card.className = 'tank-card' + (String(t.id) === String(selId) ? ' sel' : '');
            card.dataset.id = String(t.id);

            const img = document.createElement('img');
            img.className = 'tc-img';
            img.loading = 'lazy';
            img.src = '/api/tank_image/' + t.id;
            img.alt = t.name;

            const body = document.createElement('div');
            body.className = 'tc-body';
            const nm = document.createElement('div');
            nm.className = 'tc-name';
            nm.textContent = t.name;
            const meta = document.createElement('div');
            meta.className = 'tc-meta';
            const tier = document.createElement('span');
            tier.className = 'tc-tier';
            tier.textContent = 'T' + t.tier;
            const typ = document.createElement('span');
            typ.className = 'tc-type';
            typ.textContent = TYPE_LABEL[t.type] || t.type;
            const nat = document.createElement('span');
            nat.className = 'tc-nation';
            nat.textContent = t.nation;
            meta.append(tier, typ, nat);

            body.append(nm, meta);
            card.append(img, body);
            card.addEventListener('click', () => selectFromPicker(t.id));
            return card;
        }

        // Render the grid in chunks: only a window of cards is in the DOM at a time.
        function renderGrid() {
            const grid = document.getElementById('tp-grid');
            const filtered = getFiltered();
            const selId = pickerMode === 'shooter' ? currentShooterId : currentTargetId;
            renderChunk = { list: filtered, rendered: 0, selId };
            grid.innerHTML = '';
            loadMoreCards();
            document.getElementById('tp-count').textContent = filtered.length + '/' + tanksList.length;
        }

        const CHUNK = 90;
        function loadMoreCards() {
            if (!renderChunk) return;
            const grid = document.getElementById('tp-grid');
            const end = Math.min(renderChunk.rendered + CHUNK, renderChunk.list.length);
            for (let i = renderChunk.rendered; i < end; i++) {
                grid.appendChild(makeCard(renderChunk.list[i], renderChunk.selId));
            }
            renderChunk.rendered = end;
        }

        // Update the Shooter/Target button labels.
        function updateTankLabels() {
            const find = (id) => tanksList.find(t => String(t.id) === String(id));
            const s = find(currentShooterId), t = find(currentTargetId);
            document.getElementById('shooter-select').textContent = s ? s.name : '—';
            document.getElementById('target-select').textContent = t ? t.name : '—';
        }

        function openPicker(mode) {
            pickerMode = mode;
            document.getElementById('tp-title').textContent =
                (mode === 'shooter' ? 'Shooter' : 'Target') + ' — Select Tank';
            renderGrid();
            document.getElementById('tank-picker').classList.add('open');
        }

        function closePicker() {
            document.getElementById('tank-picker').classList.remove('open');
        }

        function selectFromPicker(id) {
            id = parseInt(id);
            if (pickerMode === 'shooter') {
                currentShooterId = id;
                updateTankLabels();
                loadShooter(id);
            } else {
                currentTargetId = id;
                updateTankLabels();
                loadTarget(id);
            }
            closePicker();
        }

        async function populateTankLists(initTargetId, initShooterId) {
            tanksList = await (await fetch('/api/tank_filter')).json();
            currentShooterId = initShooterId || initTargetId;
            currentTargetId = initTargetId;
            populateFilterOptions();
            updateTankLabels();
        }

        function initScene() {
            scene = new THREE.Scene();
            scene.background = new THREE.Color(0x1c1410);
            scene.fog = new THREE.Fog(0x1c1410, 15, 50);

            camera = new THREE.PerspectiveCamera(50, window.innerWidth / window.innerHeight, 0.1, 1000);
            camera.position.set(2.5, 3.2, -8);

            renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
            renderer.setSize(window.innerWidth, window.innerHeight);
            renderer.setPixelRatio(window.devicePixelRatio);
            document.getElementById('canvas-container').appendChild(renderer.domElement);
            // 提升环境亮度：ACES 色调映射 + 曝光增益，避免高光过曝的同时让整体更明亮。
            renderer.toneMapping = THREE.ACESFilmicToneMapping;
            renderer.toneMappingExposure = 1.15;

            controls = new OrbitControls(camera, renderer.domElement);
            controls.enableDamping = true;
            controls.dampingFactor = 0.05;
            controls.minDistance = 3;
            controls.maxDistance = 30;
            controls.mouseButtons = {
                LEFT: THREE.MOUSE.ROTATE,
                MIDDLE: THREE.MOUSE.DOLLY,
                RIGHT: null,
            };

            // Lighting
            const hemi = new THREE.HemisphereLight(0xffffff, 0x8a7f6e, 2.2);
            scene.add(hemi);
            scene.add(new THREE.AmbientLight(0xffffff, 0.9));
            const spot1 = new THREE.SpotLight(0xfff5e1, 500, 60, 0.55, 0.6);
            spot1.position.set(10, 15, 8);
            scene.add(spot1);
            const spot2 = new THREE.SpotLight(0x88aaff, 260, 50, 0.45, 0.5);
            spot2.position.set(-10, 12, -6);
            scene.add(spot2);

            // Grid
            const grid = new THREE.GridHelper(20, 20, 0x4a3a26, 0x2c241c);
            scene.add(grid);

            // Raycaster for click detection
            raycaster = new THREE.Raycaster();
            mouse = new THREE.Vector2();
        }

        function setupEventHandlers() {
            // Shell selector change (shooter's shells)
            document.getElementById('shell-select').addEventListener('change', function() {
                const idx = parseInt(this.value);
                if (shooterShells && idx < shooterShells.length) {
                    selectedShell = shooterShells[idx];
                }
            });

            // Configuration selector change (turret/gun config)
            document.getElementById('config-select').addEventListener('change', function() {
                applyConfig(parseInt(this.value));
            });

            // Tank selectors (buttons open the graphical picker)
            document.getElementById('target-select').addEventListener('click', function() { openPicker('target'); });
            document.getElementById('shooter-select').addEventListener('click', function() { openPicker('shooter'); });

            // Picker modal controls
            document.getElementById('tp-close').addEventListener('click', closePicker);
            document.getElementById('tp-grid').addEventListener('click', function(e) {
                // close when clicking outside a card is not needed; cards handle selection.
            });
            document.getElementById('tp-search').addEventListener('input', renderGrid);
            document.getElementById('tp-tier').addEventListener('change', renderGrid);
            document.getElementById('tp-nation').addEventListener('change', renderGrid);
            document.getElementById('tp-type').addEventListener('change', renderGrid);
            // Close the picker when clicking the overlay backdrop (click outside modal content).
            document.getElementById('tank-picker').addEventListener('click', function(e) {
                if (e.target === this) closePicker();
            });

            // Collision model toggle
            let collisionMode = false;
            document.getElementById('collision-btn').addEventListener('click', function() {
                collisionMode = !collisionMode;
                this.classList.toggle('active', collisionMode);
                this.textContent = collisionMode ? 'Hide Collision' : 'Show Collision';
                if (!armorModel) return;
                if (collisionMode) {
                    if (tankModel) tankModel.visible = false;
                    armorModel.traverse(function(node) {
                        if (!node.isMesh) return;
                        // 0 厚度的装饰性非装甲网格不显示为装甲块。
                        if (node.userData.armorSection === 'deco') { node.visible = false; return; }
                        const t = node.userData.armorThickness;
                        const c = thicknessToColor(t);
                        node.material = new THREE.MeshStandardMaterial({
                            color: c, metalness: 0.4, roughness: 0.6,
                            transparent: true, opacity: 0.95, depthWrite: true,
                        });
                    });
                } else {
                    if (tankModel) tankModel.visible = true;
                    armorModel.traverse(function(node) {
                        if (!node.isMesh) return;
                        node.material = new THREE.MeshStandardMaterial({
                            color: 0x444444, metalness: 0.3, roughness: 0.8,
                            transparent: true, opacity: 0, depthWrite: false,
                        });
                    });
                }
            });

            // Click handler: distinguish click vs drag on mouseup (click fires after mouseup,
            // and mouseup resets isDragging, so we decide here whether it was a real click)
            renderer.domElement.addEventListener('mousedown', function(e) {
                if (e.button === 0) {
                    mouseDownPos = { x: e.clientX, y: e.clientY };
                    isDragging = false;
                }
            });
            renderer.domElement.addEventListener('mousemove', function(e) {
                if (mouseDownPos) {
                    const dx = e.clientX - mouseDownPos.x;
                    const dy = e.clientY - mouseDownPos.y;
                    if (dx * dx + dy * dy > 25) isDragging = true; // 5px threshold
                }
            });
            renderer.domElement.addEventListener('mouseup', function(e) {
                if (e.button !== 0) return;
                if (isDragging) { mouseDownPos = null; isDragging = false; return; }
                // Real click (no drag) → run penetration analysis on this click
                mouseDownPos = null;
                isDragging = false;
                onClick(e);
            });

            // Turret/gun angle limits (target tank)
            let rmbDown = false, rmbStartX = 0, rmbStartY = 0, rmbStartTurret = 0, rmbStartGun = 0;
            renderer.domElement.addEventListener('contextmenu', function(e) { e.preventDefault(); });
            renderer.domElement.addEventListener('mousedown', function(e) {
                if (e.button !== 2) return;
                rmbDown = true;
                rmbStartX = e.clientX;
                rmbStartY = e.clientY;
                rmbStartTurret = currentTurretDeg;
                rmbStartGun = currentGunDeg;
            });
            window.addEventListener('mousemove', function(e) {
                if (!rmbDown) return;
                const tLeft = tankData.turret_traverse_left ?? 180;
                const tRight = tankData.turret_traverse_right ?? 180;
                const gDep = tankData.gun_depression ?? 8;
                const gEle = tankData.gun_elevation ?? 20;
                const dx = e.clientX - rmbStartX;
                const dy = e.clientY - rmbStartY;
                currentTurretDeg = Math.max(-tLeft, Math.min(tRight, rmbStartTurret + dx * 0.5));
                currentGunDeg = Math.max(-gDep, Math.min(gEle, rmbStartGun - dy * 0.5));
                document.getElementById('turret-val').textContent = currentTurretDeg.toFixed(0) + '°';
                document.getElementById('gun-val').textContent = currentGunDeg.toFixed(0) + '°';
                updateTurretGun(currentTurretDeg, currentGunDeg);
            });
            window.addEventListener('mouseup', function(e) {
                if (e.button === 2) rmbDown = false;
                // Left-button mouseup handled on canvas (distinguishes click vs drag)
            });

            document.getElementById('turret-controls').style.display = 'block';

            window.addEventListener('resize', function() {
                camera.aspect = window.innerWidth / window.innerHeight;
                camera.updateProjectionMatrix();
                renderer.setSize(window.innerWidth, window.innerHeight);
                });
        }

        async function init() {
            initScene();
            setupEventHandlers();
            const initTargetId = window.__INITIAL_TANK__ || 28689;
            const initShooterId = window.__INITIAL_SHOOTER__ || initTargetId;
            await populateTankLists(initTargetId, initShooterId);
            await loadTarget(initTargetId);
            await loadShooter(initShooterId);
            animate();
        }

        let turretNode = null, gunNodesList = [];
        let origMatrices = null;
        let currentConfigIdx = 0;
        // 当前炮塔/主炮角度（模块级，供配置切换后重放，保证切换后旋转/俯仰仍正确）
        let currentTurretDeg = 0, currentGunDeg = 0;
        // Each config gun group is the array of all sibling nodes sharing the "gun_0X" prefix
        // (e.g. [gun_04, gun_04_mask, gun_04_mask_nc, ...]). Group index == config gun_index.
        let configGunGroups = [];
        let configTurretNodes = [];// all turret_0X root nodes (visual model), sorted by number
        // Pivots in model-local (frame) coordinates, derived from the visual model geometry so the
        // turret/gun rotate about their own center (no drift) even when the vehicle lacks points.
        let turretPivotLocal = null, gunPivotLocal = null;

        // Collect gun / turret configuration node groups from a model's root group.
        // A gun config is identified by the "gun_0X" prefix shared by all its variant nodes
        // (gun_0X, gun_0X_mask, gun_0X_mask_cap, gun_0X_nc, ...), so rotating the gun moves the
        // barrel AND the gun mask (gun root) together.
        function collectConfigNodes(model) {
            configGunGroups = [];
            configTurretNodes = [];
            if (!model) return;
            const root = model.children[0] || model;
            const byGroup = new Map(); // groupNum -> nodes[]
            for (const child of root.children) {
                const nm = child.name || '';
                const gm = nm.match(/^gun_(\d+)/);
                const tm = nm.match(/^turret_(\d+)$/);
                if (gm) {
                    const g = parseInt(gm[1], 10);
                    if (!byGroup.has(g)) byGroup.set(g, []);
                    byGroup.get(g).push(child);
                } else if (tm) {
                    configTurretNodes.push(child);
                }
            }
            // Sort groups by number; each group sorted by name.
            const keys = Array.from(byGroup.keys()).sort((a, b) => a - b);
            for (const k of keys) {
                const arr = byGroup.get(k).sort((a, b) => ((a.name||'') < (b.name||'') ? -1 : 1));
                configGunGroups.push(arr);
            }
            configTurretNodes.sort((a, b) => (a.name.match(/\d+/)?.[0]|0) - (b.name.match(/\d+/)?.[0]|0));
        }

        // Show only the selected gun/turret config group; hide the whole subtree of siblings.
        // 可选主炮可能没有独立的模型组（如 116-F3 / AC Atlas：多配置但仅一个 gun_0X 节点）。
        // 此时 gun_index/turret_index 会超出实际模型组数，需对组数取模回退到已有组，
        // 复用同一个主炮/炮塔模型，否则切换配置会把炮管/炮塔整体隐藏。
        function applyConfigVisible(model, gi, ti) {
            if (!model) return;
            const gCount = Math.max(1, configGunGroups.length);
            const tCount = Math.max(1, configTurretNodes.length);
            configGunGroups.forEach((grp, i) => { grp.forEach(n => { n.visible = (i === (gi % gCount)); }); });
            configTurretNodes.forEach((n, i) => { n.visible = (i === (ti % tCount)); });
        }

        function collectTurretGunNodes() {
            if (!tankModel) return;
            // Find root node (first child of scene)
            const root = tankModel.children[0];
            if (!root) return;
            const cfg = currentConfig();
            // Active gun = gun node matching config's gun_index; turret = config's turret_index.
            // Fall back to legacy gun_01 / turret naming if no gun_0X / turret_0X pattern was found.
            if (configTurretNodes.length) {
                turretNode = configTurretNodes[cfg.turret_index % configTurretNodes.length] || null;
            } else {
                turretNode = root.children.find(c => c.name === 'turret_01' || c.name === 'turret') || null;
            }
            // Active gun config group = all sibling nodes sharing the gun_0X prefix (barrel + mask/gun root).
            if (configGunGroups.length) {
                const grp = configGunGroups[cfg.gun_index % configGunGroups.length];
                gunNodesList = grp || [];
                // Primary barrel node = the exact "gun_0X" node (not _mask / _nc variants).
                const primary = grp.find(n => /^gun_\d+$/.test(n.name || ''));
                // The non-mask members (mask/cap) should follow the gun rotation too -> keep them in the list.
            } else {
                // Legacy: group any gun_01-prefixed siblings.
                gunNodesList = root.children.filter(c => (c.name || '').startsWith('gun_01'));
            }
            origMatrices = new Map();
            if (turretNode) {
                turretNode.updateMatrix();
                origMatrices.set(turretNode, turretNode.matrix.clone());
                turretNode.matrixAutoUpdate = false;
            }
            for (const gn of gunNodesList) {
                gn.updateMatrix();
                origMatrices.set(gn, gn.matrix.clone());
                gn.matrixAutoUpdate = false;
            }
        }

        // Derive turret/gun rotation pivots (model-local frame) from the visual model geometry.
        // Rotating about an object's own center keeps it from drifting. Called after config switch.
        function computePivots() {
            turretPivotLocal = null;
            gunPivotLocal = null;
            if (!tankModel) return;
            const root = tankModel.children[0] || tankModel;
            root.updateMatrixWorld(true);
            const cfg = currentConfig();
            if (!cfg) return;

            // 计算某节点的"本体"包围盒（跳过 hide_elements / _nc / _switch 等非本体子树）。
            // 这类子树会含炮塔外壳/隐藏件等延伸几何，把包围盒中心拉偏，导致 pivot 漂移
            // （如 114 SP2 的 turret_01_nc 炮塔外壳 Z 延伸到 5.81）。
            const isNonBody = (n) => /hide_elements|_nc|_switch|_cap/i.test(n.name || '');
            const bodyBoxOf = (node) => {
                const b = new THREE.Box3();
                let any = false;
                (function walk(n) {
                    if (isNonBody(n)) return;
                    if (n.isMesh) { b.union(new THREE.Box3().setFromObject(n)); any = true; }
                    n.children.forEach(walk);
                })(node);
                return any ? b : null;
            };
            const bodyBoxUnion = (nodes) => {
                const b = new THREE.Box3(); let any = false;
                (nodes || []).forEach(n => {
                    const sub = bodyBoxOf(n);
                    if (sub) { b.union(sub); any = true; }
                });
                return any ? b : null;
            };

            const centerLocalOf = (nodes) => {
                const b = bodyBoxUnion(nodes);
                if (!b) return null;
                const wc = b.getCenter(new THREE.Vector3());
                return root.worldToLocal(wc.clone());
            };
            // Gun pivot should be the gun ROOT (the base that attaches to the turret), not the
            // barrel's geometric center, so pitch rotates the barrel in place. We pick the bbox
            // end (in Z) that is closest to the turret center, which works regardless of which
            // direction the gun points.
            const turretCenterW = (() => {
                if (!configTurretNodes.length) return null;
                const t = configTurretNodes[cfg.turret_index % configTurretNodes.length];
                const b = bodyBoxOf(t);
                return b ? b.getCenter(new THREE.Vector3()) : null;
            })();
            const gunRootLocalOf = (nodes) => {
                const b = bodyBoxUnion(nodes);
                if (!b) return null;
                let wc;
                const xMid = (b.min.x + b.max.x) / 2, yMid = (b.min.y + b.max.y) / 2;
                if (turretCenterW) {
                    const zMin = Math.abs(b.min.z - turretCenterW.z);
                    const zMax = Math.abs(b.max.z - turretCenterW.z);
                    wc = new THREE.Vector3(xMid, yMid, zMin < zMax ? b.min.z : b.max.z);
                } else {
                    wc = new THREE.Vector3(xMid, yMid, b.max.z);
                }
                return root.worldToLocal(wc.clone());
            };

            // Turret pivot: the turret group's own center. Rotating about local Z (vertical).
            const turGroup = configTurretNodes.length
                ? [configTurretNodes[cfg.turret_index % configTurretNodes.length]] : null;
            const turLocal = centerLocalOf(turGroup);
            if (turLocal) turretPivotLocal = turLocal;

            // Gun pivot: the gun config group's root (base) so pitch keeps the root fixed.
            const gunGroup = configGunGroups.length
                ? configGunGroups[cfg.gun_index % configGunGroups.length] : null;
            const gunLocal = gunRootLocalOf(gunGroup);
            if (gunLocal) gunPivotLocal = gunLocal;
        }

        function updateTurretGun(turretDeg, gunDeg) {
            if (!tankModel) return;
            if (!origMatrices) collectTurretGunNodes();
            if (!origMatrices) return;

            // 枢轴（旋转中心）优先级：
            //   1) 游戏权威枢轴点 collision.turret_points / gun_points（经 (x,-y,-z) 变换）
            //      —— 这是炮塔旋转轴 / 炮管俯仰轴（炮塔根部），转动时不漂移；
            //   2) 由视觉几何推导的 turretPivotLocal / gunPivotLocal；
            //   3) 默认值兜底。
            const col = tankData.collision;
            const tPts = col?.turret_points;
            const gPts = col?.gun_points;
            const tPivot = tPts
                ? new THREE.Vector3(tPts[0], -tPts[1], -tPts[2])
                : (turretPivotLocal ? turretPivotLocal.clone() : new THREE.Vector3(0, 0, 1.7));
            const gPivot = gPts
                ? new THREE.Vector3(gPts[0], -gPts[1], -gPts[2])
                : (gunPivotLocal ? gunPivotLocal.clone() : new THREE.Vector3(0, 0, 2.0));

            const tr = THREE.MathUtils.degToRad(turretDeg);
            const gr = THREE.MathUtils.degToRad(gunDeg);

            // Turret rotation matrix: translate(P_t) * rotateZ(angle) * translate(-P_t)
            const mTurret = new THREE.Matrix4();
            mTurret.makeTranslation(tPivot.x, tPivot.y, tPivot.z);
            mTurret.multiply(new THREE.Matrix4().makeRotationZ(tr));
            mTurret.multiply(new THREE.Matrix4().makeTranslation(-tPivot.x, -tPivot.y, -tPivot.z));

            // Gun rotation matrix (composed with turret): M_turret * translate(P_g) * rotateX(angle) * translate(-P_g)
            const mGun = mTurret.clone();
            mGun.multiply(new THREE.Matrix4().makeTranslation(gPivot.x, gPivot.y, gPivot.z));
            mGun.multiply(new THREE.Matrix4().makeRotationX(gr));
            mGun.multiply(new THREE.Matrix4().makeTranslation(-gPivot.x, -gPivot.y, -gPivot.z));

            if (turretNode) {
                const orig = origMatrices.get(turretNode);
                const m = mTurret.clone();
                m.multiply(orig);
                turretNode.matrix.copy(m);
                turretNode.matrixWorldNeedsUpdate = true;
            }
            for (const gn of gunNodesList) {
                const orig = origMatrices.get(gn);
                const m = mGun.clone();
                m.multiply(orig);
                gn.matrix.copy(m);
                gn.matrixWorldNeedsUpdate = true;
            }

            // Same for armor model
            if (armorModel) {
                if (!armorOrigMatrices) collectArmorNodes();
                if (armorOrigMatrices) {
                    const aTP = tPivot.clone();
                    const aGP = gPivot.clone();
                    const mAT = new THREE.Matrix4();
                    mAT.makeTranslation(aTP.x, aTP.y, aTP.z);
                    mAT.multiply(new THREE.Matrix4().makeRotationZ(tr));
                    mAT.multiply(new THREE.Matrix4().makeTranslation(-aTP.x, -aTP.y, -aTP.z));

                    const mAG = mAT.clone();
                    mAG.multiply(new THREE.Matrix4().makeTranslation(aGP.x, aGP.y, aGP.z));
                    mAG.multiply(new THREE.Matrix4().makeRotationX(gr));
                    mAG.multiply(new THREE.Matrix4().makeTranslation(-aGP.x, -aGP.y, -aGP.z));

                    for (const [node, orig] of armorOrigMatrices) {
                        const name = node.name || '';
                        // 炮盾(火炮)装甲节点形如 gun_0X_armor_*；炮塔装甲 turret_0X_armor_*。
                        // 之前硬编码 gun_01_armor_，E-100 双主炮实为 gun_04/gun_06，导致火炮俯仰时
                        // 炮盾装甲不跟随火炮，只吃到炮塔矩阵。统一按 gun_0X_armor_ 前缀匹配。
                        if (/^gun_\d+_armor_/.test(name)) {
                            const m = mAG.clone();
                            m.multiply(orig);
                            node.matrix.copy(m);
                        } else {
                            const m = mAT.clone();
                            m.multiply(orig);
                            node.matrix.copy(m);
                        }
                        node.matrixWorldNeedsUpdate = true;
                    }
                }
            }
        }

        let armorOrigMatrices = null;
        function collectArmorNodes() {
            if (!armorModel) return;
            const cfg = currentConfig();
            // Determine which gun/turret armor prefixes belong to the selected config.
            let gunPrefix = null, turretPrefix = null;
            if (cfg) {
                const gunPk = collectGunArmorPrefixes();
                if (gunPk.length) gunPrefix = gunPk[cfg.gun_index % gunPk.length];
                const turPk = collectTurretArmorPrefixes();
                if (turPk.length) turretPrefix = turPk[cfg.turret_index % turPk.length];
            }
            armorOrigMatrices = new Map();
            armorModel.traverse(function(node) {
                if (!node.isMesh) return;
                const name = node.name || '';
                const gm = name.match(/^(gun_\d+)_armor_/);
                const tm = name.match(/^(turret_\d+)_armor_/);
                const isGun = gm && (!gunPrefix || gm[1] === gunPrefix);
                const isTurret = tm && (!turretPrefix || tm[1] === turretPrefix);
                if (isGun || isTurret) {
                    node.updateMatrix();
                    armorOrigMatrices.set(node, node.matrix.clone());
                    node.matrixAutoUpdate = false;
                }
            });
        }

        // Distinct gun_0X prefixes present in the armor model, sorted numerically.
        function collectGunArmorPrefixes() {
            const set = new Set();
            if (armorModel) armorModel.traverse(n => {
                const m = (n.name || '').match(/^(gun_\d+)_armor_/);
                if (m) set.add(m[1]);
            });
            return Array.from(set).sort((a,b) => (a.match(/\d+/)?.[0]|0)-(b.match(/\d+/)?.[0]|0));
        }
        function collectTurretArmorPrefixes() {
            const set = new Set();
            if (armorModel) armorModel.traverse(n => {
                const m = (n.name || '').match(/^(turret_\d+)_armor_/);
                if (m) set.add(m[1]);
            });
            return Array.from(set).sort((a,b) => (a.match(/\d+/)?.[0]|0)-(b.match(/\d+/)?.[0]|0));
        }

        let mouseDownPos = null, isDragging = false;
        function onClick(event) {
            if (!armorModel) return;

            // Ensure all config-active armor meshes are visible for raycasting,
            // but do NOT re-show meshes hidden by a config switch.
            armorModel.traverse(function(node) {
                if (!node.isMesh) return;
                if (node.userData.configHidden) return;
                if (node.userData.armorSection === 'deco') { node.visible = false; return; }
                if (node.visible === false) node.visible = true;
            });
            // 仅保留当前配置激活的炮管(履带/chassis 始终保留)。未选中的炮管需剔除：
            // 不强制显示、不参与 raycast，避免多主炮车辆出现重叠炮管的碰撞判定。
            const activeGun = activeGunNumber();
            const activeModules = activeGun == null
                ? moduleMeshes   // 无法确定激活炮(如 configGunGroups 未填充)时退化为全部保留
                : moduleMeshes.filter(m => {
                    if (m.userData.gunConfig != null) return m.userData.gunConfig === activeGun;
                    return true;
                });
            for (const mesh of activeModules) {
                if (mesh.visible === false) mesh.visible = true;
            }

            const rect = renderer.domElement.getBoundingClientRect();
            mouse.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
            mouse.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;

            raycaster.setFromCamera(mouse, camera);
            const objects = [armorModel, ...activeModules];
            const intersects = raycaster.intersectObjects(objects, true);
            const armorHits = [];
            const seenKeys = new Set();
            for (const hit of intersects) {
                // 双主炮车辆：未选中配置的炮盾/炮塔装甲(gun_0X_armor_* 或 turret_0X_armor_*)
                // 被 applyArmorConfigVisible 标记为 configHidden。Raycaster 不检查 visible，
                // 必须显式排除这些隐藏配置的模块，否则旋转炮塔后残留的炮盾仍参与判定。
                if (hit.object.parent && hit.object.parent.visible === false) continue;
                if (hit.object.userData.configHidden) continue;
                // 0 厚度的装饰性非装甲网格（如 hull/turret_armor_8 等）：不参与穿透判定。
                if (hit.object.userData.armorSection === 'deco') continue;
                let section = null, plateId = null, thickness = null;
                if (hit.object.userData.armorSection) {
                    section = hit.object.userData.armorSection;
                    plateId = hit.object.userData.armorPlateId;
                    thickness = hit.object.userData.armorThickness;
                } else {
                    let node = hit.object;
                    while (node) {
                        const m = (node.name || '').match(/(hull|turret|gun)_\w*?_?armor_(\d+)/);
                        if (m) { section = m[1]; plateId = m[2]; break; }
                        node = node.parent;
                    }
                    if (section && plateId) thickness = getPlateThickness(section, plateId);
                }
                if (section !== null && thickness !== null && thickness !== undefined) {
                    const key = section + ':' + plateId;
                    if (seenKeys.has(key)) continue;
                    seenKeys.add(key);
                    const normal = hit.face ? hit.face.normal.clone() : new THREE.Vector3(0, 1, 0);
                    const nm = new THREE.Matrix3().getNormalMatrix(hit.object.matrixWorld);
                    normal.applyNormalMatrix(nm).normalize();
                    let partName;
                    if (section === 'chassis') {
                        partName = plateId === 'leftTrack' ? 'Track (Left)' : 'Track (Right)';
                    } else if (section === 'gunBarrel') {
                        partName = 'Gun Barrel';
                    } else {
                        // 炮盾(gun 非 spaced 板)已归类为主装甲(turret)判定，但名称应显示为 Gun。
                        const disp = hit.object.userData.armorSectionOrig || section;
                        partName = `${disp.charAt(0).toUpperCase() + disp.slice(1)} Plate ${plateId}`;
                    }
                    armorHits.push({ section, plateId, thickness, point: hit.point, normal, partName });
                }
            }

            if (armorHits.length === 0) {
                document.getElementById('click-info').style.display = 'none';
                document.getElementById('traj-info').style.display = 'none';
                trajInfoPos = null;
                if (trajGroup) { scene.remove(trajGroup); trajGroup = null; }
                return;
            }

            const first = armorHits[0];
            const point = first.point;
            const viewDir = camera.position.clone().sub(point).normalize();

            // Send to Rust backend for unified penetration calculation
            const shellType = selectedShell ? (selectedShell.type || '').toLowerCase() : '';
            const pen = selectedShell ? (selectedShell.penetration || 0) : 0;
            const dmg = selectedShell ? (selectedShell.damage || 0) : 0;
            const modDmg = selectedShell ? (selectedShell.module_damage || 0) : 0;
            const caliber = shooterCaliber || (shooterData && shooterData.caliber) || tankData.caliber || 120;
            const isHE = shellType === 'he';

            const req = {
                shell_type: shellType,
                penetration: pen,
                caliber: caliber,
                damage: dmg,
                module_damage: modDmg,
                explosion_radius: isHE ? 3.0 : 0,
                calibrated_shells: false,
                enhanced_armor: false,
                view_dir: [viewDir.x, viewDir.y, viewDir.z],
                hits: armorHits.map(ah => ({
                    section: ah.section,
                    plate_id: ah.plateId,
                    thickness: ah.thickness,
                    normal: [ah.normal.x, ah.normal.y, ah.normal.z],
                    point: [ah.point.x, ah.point.y, ah.point.z],
                    part_name: ah.partName,
                })),
            };

            fetch('/api/penetrate', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(req),
            }).then(r => { if (!r.ok) throw new Error('API ' + r.status); return r.json(); }).then(res => {
                let trajLayers = res.layers.map(l => {
                    const ah = armorHits.find(ah => ah.partName === l.part_name);
                    return {
                        point: ah?.point || point,
                        name: l.part_name,
                        thickness: l.thickness,
                        eff: l.effective,
                        remainBefore: l.remaining_before,
                        penetrated: l.penetrated,
                        ricochet: l.ricochet,
                        normal: ah?.normal,
                        seg: 0,
                    };
                });

                // If ricochet, re-cast along the reflected direction with reduced pen
                if (res.ricochet && res.ricochet_remaining_pen > 0) {
                    const lastLayer = trajLayers[trajLayers.length - 1];
                    if (lastLayer && lastLayer.point) {
                        // Reflect direction: shellDir - 2*(shellDir·N)*N
                        // Use the ricochet plate's own world normal; fall back to first.normal
                        const shellDir = viewDir.clone().negate(); // incoming shell direction
                        let n = lastLayer.normal;
                        if (!n) {
                            // Look up normal from the original hit whose point matches the last layer
                            const hitMatch = armorHits.find(ah => ah.point.distanceToSquared(lastLayer.point) < 1e-6);
                            n = hitMatch ? hitMatch.normal : first.normal;
                        }
                        const reflect = shellDir.clone().sub(n.clone().multiplyScalar(2 * shellDir.dot(n))).normalize();
                        // Offset ray origin slightly along the reflection so it doesn't re-hit the same plate
                        const rc = new THREE.Raycaster(lastLayer.point.clone().add(reflect.clone().multiplyScalar(0.05)), reflect, 0.01, 60);
                        const ricIntersects = rc.intersectObjects(objects, true);
                        // Collect deduped hits, skipping the ricochet point itself
                        const ricHits = [];
                        const ricSeen = new Set();
                        for (const hit of ricIntersects) {
                            // Skip hits too close to the ricochet point (same plate re-hit)
                            if (hit.point.distanceToSquared(lastLayer.point) < 0.01) continue;
                             // 双主炮：排除未选中配置的隐藏炮盾/炮塔装甲（见 onClick 同款过滤）。
                             if (hit.object.parent && hit.object.parent.visible === false) continue;
                             if (hit.object.userData.configHidden) continue;
                             // 0 厚度的装饰性非装甲网格：不参与穿透判定。
                             if (hit.object.userData.armorSection === 'deco') continue;
                             let s = null, pid = null, th = null;
                            if (hit.object.userData.armorSection) {
                                s = hit.object.userData.armorSection;
                                pid = hit.object.userData.armorPlateId;
                                th = hit.object.userData.armorThickness;
                            } else {
                                let node = hit.object;
                                while (node) {
                                    const mm = (node.name || '').match(/(hull|turret|gun)_\w*?_?armor_(\d+)/);
                                    if (mm) { s = mm[1]; pid = mm[2]; break; }
                                    node = node.parent;
                                }
                                if (s && pid) th = getPlateThickness(s, pid);
                            }
                            if (s !== null && th !== null && th !== undefined) {
                                const k = s + ':' + pid;
                                if (!ricSeen.has(k)) { ricSeen.add(k); ricHits.push({ section: s, plateId: pid, thickness: th, normal: hit.face ? hit.face.normal.clone().applyMatrix3(new THREE.Matrix3().getNormalMatrix(hit.object.matrixWorld)).normalize() : new THREE.Vector3(0,1,0), partName: s === 'chassis' ? (pid === 'leftTrack' ? 'Track (Left)' : 'Track (Right)') : (s === 'gunBarrel' ? 'Gun Barrel' : ((hit.object.userData.armorSectionOrig || s).charAt(0).toUpperCase() + (hit.object.userData.armorSectionOrig || s).slice(1) + ' Plate ' + pid)), point: hit.point }); }
                            }
                        }
                        if (ricHits.length > 0) {
                            const ricReq = {
                                shell_type: shellType, penetration: res.ricochet_remaining_pen, caliber: caliber,
                                view_dir: [reflect.x, reflect.y, reflect.z],
                                hits: ricHits.map(ah => ({ section: ah.section, plate_id: ah.plateId, thickness: ah.thickness, normal: [ah.normal.x, ah.normal.y, ah.normal.z], point: [ah.point.x, ah.point.y, ah.point.z], part_name: ah.partName })),
                            };
                            fetch('/api/penetrate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(ricReq) })
                                .then(r => r.ok ? r.json() : null).then(ricRes => {
                                    if (ricRes) {
                                        const ricLayers = ricRes.layers.map(l => ({ point: ricHits.find(ah => ah.partName === l.part_name)?.point || lastLayer.point, name: l.part_name, thickness: l.thickness, eff: l.effective, remainBefore: l.remaining_before, penetrated: l.penetrated, ricochet: l.ricochet, seg: 1 }));
                                        const combined = { result: 'RICOCHET → ' + ricRes.result, total_effective: res.total_effective, layers: [...trajLayers, ...ricLayers] };
                                        showTrajectory(point, combined.result, combined.total_effective, combined.layers, pen, dmg, modDmg);
                                    } else { showTrajectory(point, res.result, res.total_effective, trajLayers, pen, dmg, modDmg); }
                                }).catch(() => { showTrajectory(point, res.result, res.total_effective, trajLayers, pen, dmg, modDmg); });
                            return;
                        }
                    }
                }

                showTrajectory(point, res.result, res.total_effective, trajLayers, pen, dmg, modDmg);
            }).catch(err => {
                console.error('Penetration API error:', err);
                // Fallback: show basic result without API
                showTrajectory(point, 'ERROR', 0, [], pen, dmg, modDmg);
            });
        }

        let trajGroup = null;
        let trajInfoPos = null;
        function showTrajectory(firstPoint, result, totalEff, layers, penVal, dmgVal, modDmgVal) {
            if (trajGroup) scene.remove(trajGroup);
            trajGroup = new THREE.Group();

            const color = result === 'PENETRATION' ? 0x4CAF50 : (result === 'RICOCHET' ? 0xFF8800 : (result === 'BLOCKED' ? 0xf44336 : 0xff8800));
            const camDir = camera.position.clone().sub(firstPoint).normalize();

            // Build trajectory as separate straight tubes per segment (incoming / reflected)
            // so the ricochet turn is sharp and aligns exactly with the ricochet point.
            const origin = firstPoint.clone().add(camDir.clone().multiplyScalar(15));
            const trajMat = new THREE.MeshBasicMaterial({ color: color, depthTest: false, transparent: true, opacity: 0.85 });

            // Find the ricochet point (the layer flagged ricochet), if any
            const ricLayer = layers.find(l => l.ricochet);
            const ricPoint = ricLayer ? ricLayer.point : null;

            // Incoming segment: origin → all seg=0 points up to (and including) ricochet point
            const incoming = [origin];
            for (const l of layers) {
                incoming.push(l.point);
                if (l.ricochet) break; // stop at ricochet point
            }
            if (incoming.length >= 2) {
                const curve = new THREE.CatmullRomCurve3(incoming, false, 'catmullrom', 0);
                const geo = new THREE.TubeGeometry(curve, Math.max(2, incoming.length * 2), 0.025, 8, false);
                const mesh = new THREE.Mesh(geo, trajMat);
                mesh.renderOrder = 999;
                trajGroup.add(mesh);
            }

            // Reflected segment: from ricochet point through all seg=1 points
            const reflected = [];
            let sawRicPoint = false;
            for (const l of layers) {
                if (l.ricochet) { sawRicPoint = true; reflected.push(l.point); continue; }
                if (l.seg === 1) {
                    if (!sawRicPoint) { reflected.push(ricPoint); sawRicPoint = true; }
                    reflected.push(l.point);
                }
            }
            if (sawRicPoint && reflected.length >= 2) {
                const curve = new THREE.CatmullRomCurve3(reflected, false, 'catmullrom', 0);
                const geo = new THREE.TubeGeometry(curve, Math.max(2, reflected.length * 2), 0.025, 8, false);
                const mesh = new THREE.Mesh(geo, trajMat);
                mesh.renderOrder = 999;
                trajGroup.add(mesh);
            }

            // Contact point markers + labels (high-res canvas)
            for (let i = 0; i < layers.length; i++) {
                const l = layers[i];
                const pColor = l.penetrated ? 0x4CAF50 : (l.ricochet ? 0xFF8800 : 0xf44336);

                // Small sphere
                const dotGeo = new THREE.SphereGeometry(0.04, 8, 8);
                const dotMat = new THREE.MeshBasicMaterial({ color: pColor, depthTest: false, transparent: true, opacity: 0.95 });
                const dotMesh = new THREE.Mesh(dotGeo, dotMat);
                dotMesh.position.copy(l.point);
                dotMesh.renderOrder = 999;
                trajGroup.add(dotMesh);
            }

            // HTML info window (fixed size, not affected by camera zoom)
            const lastPt = layers.length > 0 ? layers[layers.length - 1].point : firstPoint;
            trajInfoPos = lastPt.clone();
            const div = document.getElementById('traj-info');
            const colorHex = '#' + color.toString(16).padStart(6, '0');
            let html = `<div style="background:rgba(12,14,22,0.97);border-radius:10px;border-left:4px solid ${colorHex};padding:10px 16px;font-family:'Segoe UI',sans-serif;white-space:nowrap;box-shadow:0 4px 20px rgba(0,0,0,0.5);">`;
            html += `<div style="font-size:18px;font-weight:bold;color:${colorHex};margin-bottom:4px;">${result}</div>`;
            const dmgLine = (() => {
                const hp = (typeof dmgVal === 'number') ? dmgVal : 0;
                const md = (typeof modDmgVal === 'number') ? modDmgVal : 0;
                if (result === 'PENETRATION' && hp > 0) return `HP Dmg ${hp.toFixed(0)}`;
                if ((result === 'BLOCKED' || result === 'RICOCHET') && md > 0) return `Module Dmg ${md.toFixed(0)} (HP ${hp.toFixed(0)})`;
                if (hp > 0) return `HP Dmg ${hp.toFixed(0)} / Module ${md.toFixed(0)}`;
                return '';
            })();
            html += `<div style="font-size:13px;color:#8ab;margin-bottom:8px;">Eff ${totalEff.toFixed(0)}mm · Pen ${penVal}mm · Remain ${Math.max(0, penVal - totalEff).toFixed(0)}mm · ${layers.length} layers${dmgLine ? ' · <span style="color:#FFB74D;">' + dmgLine + '</span>' : ''}</div>`;
            html += `<div style="border-top:1px solid rgba(255,255,255,0.1);margin-bottom:6px;"></div>`;
            for (let i = 0; i < layers.length; i++) {
                const l = layers[i];
                const pc = l.penetrated ? '#5fbf64' : (l.ricochet ? '#ff9c40' : '#e85050');
                const remain = l.penetrated ? (l.remainBefore - l.eff).toFixed(0) : 'BLOCKED';
                html += `<div style="font-size:13px;line-height:20px;color:${pc};">`;
                html += `<span style="color:${pc};">●</span> <span style="color:#ddd;">${l.name}</span>`;
                html += `<span style="color:#888;margin-left:16px;">${l.thickness}mm / ${l.eff.toFixed(0)}eff / pen ${remain}</span>`;
                html += `</div>`;
            }
            html += `</div>`;
            div.innerHTML = html;
            div.style.display = 'block';
            updateTrajInfoPos();

            scene.add(trajGroup);
        }

                                        function updateTrajInfoPos() {
            if (!trajInfoPos) { document.getElementById('traj-info').style.display = 'none'; return; }
            const v = trajInfoPos.clone().project(camera);
            const x = (v.x * 0.5 + 0.5) * window.innerWidth;
            const y = (-v.y * 0.5 + 0.5) * window.innerHeight;
            const div = document.getElementById('traj-info');
            if (v.z > 1) { div.style.display = 'none'; return; }
            // Offset downward so the panel doesn't cover the tank model; clamp to viewport.
            const offX = 0, offY = 160;
            let px = x + offX, py = y + offY;
            px = Math.max(10, Math.min(window.innerWidth - 340, px));
            py = Math.max(10, Math.min(window.innerHeight - 140, py));
            div.style.transform = 'translateX(-50%)';
            div.style.left = px + 'px';
            div.style.top = py + 'px';
            div.style.display = 'block';
        }

        function animate() {
            requestAnimationFrame(animate);
            controls.update();
            renderer.render(scene, camera);
            if (trajInfoPos) updateTrajInfoPos();
        }

        init();
    </script>
</body>
</html>"#;
