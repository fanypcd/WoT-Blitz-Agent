use axum::{routing::{get, post}, response::{Html, IntoResponse, Response}, Json, Router};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::path::Path;
use crate::wargaming::tank_resolver::TankResolver;
use crate::wargaming::dvpl::{DvplFile, ArmorModel};
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
        .route("/api/hold", get(crate::wargaming::heatmap_ready::hold_handler))
        .route("/api/ready", get(crate::wargaming::heatmap_ready::ready_handler))
        .with_state(())
}

/// 3D 模型代理：`glb_cache/` 有则直接返回，否则从 BlitzKit CDN 下载并落盘。
/// 仅允许 collision.glb / model.glb 两个文件名。
/// 确保 GLB 已缓存（glb_cache/ 优先，否则 BlitzKit CDN 下载落盘），返回字节。
/// 供 glb_handler 与热力图渲染器共用。
pub(crate) async fn ensure_glb_bytes(tank_id: u32, filename: &str) -> Result<Vec<u8>, String> {
    if !GLB_FILES.contains(&filename) {
        return Err(format!("invalid GLB filename: {}", filename));
    }
    // 1. Serve from local cache
    let cache_dir = Path::new(GLB_CACHE_DIR).join(tank_id.to_string());
    let cache_path = cache_dir.join(filename);
    if let Ok(bytes) = std::fs::read(&cache_path) {
        return Ok(bytes);
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
                        return Ok(vec);
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
    Err(format!("BlitzKit CDN unreachable: {} (model not in glb_cache/)", last_err.unwrap_or_default()))
}

/// 启动 3D 查看器服务器（不打开浏览器），返回实际端口。
/// 供 Agent 截图工具使用：配合 URL 参数（heatmap=1&clean=1&...）实现无头热力图渲染。
pub async fn start_viewer_server(tank_resolver: TankResolver, tank_id: u32, shooter_id: u32) -> anyhow::Result<u16> {
    start_viewer_server_with_data(tank_resolver, tank_id, Some(shooter_id), None).await
}

/// 启动查看器服务器并可选注入射击复现数据（/api/replay_shot）。
/// 从回放文件直接启动"射击复现"查看器：解析回放 → 抽取该发复现数据 →
/// 注入服务器（/api/replay_shot），目标坦克 = 被命中车辆。返回端口。
pub async fn start_viewer_server_for_replay(
    replay_path: &std::path::Path,
    tank_resolver: TankResolver,
    shot_no: usize,
) -> anyhow::Result<u16> {
    use wotbreplay_parser::replay::Replay;
    let mut replay = Replay::open(std::fs::File::open(replay_path)?)?;
    let meta = replay.read_meta().ok();
    let data = replay.read_data()?;
    let raw_packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        (t, pkt.clock_secs, &pkt.raw_payload[..])
    }).collect();

    let timeline = crate::replay::combat::CombatTimeline::parse_packets(&raw_packets);
    let author_eid = *timeline.entity_names.iter()
        .find(|(eid, _)| timeline.events.iter().any(|e|
            e.entity_id == **eid && matches!(e.event_type, crate::replay::combat::CombatEventType::DamageCounter { .. })))
        .map(|(eid, _)| eid)
        .unwrap_or(&0);
    let shots = timeline.infer_shots(author_eid);
    if shots.is_empty() {
        return Err(anyhow::anyhow!("No shot events detected in this replay."));
    }
    let file_name = replay_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let author_player_eid = crate::replay::combat::resolve_author_player_eid(&raw_packets, file_name);
    let replay_data = crate::replay::combat::extract_shot_replays(&raw_packets, author_player_eid, &shots);
    if shot_no == 0 || shot_no > replay_data.len() {
        return Err(anyhow::anyhow!("shot {} out of range (1..={})", shot_no, replay_data.len()));
    }
    let shot = &replay_data[shot_no - 1];

    // 目标/射手坦克 ID：battle_results 按昵称关联
    let br = replay.read_battle_results().ok();
    let tank_of = |nick: &str| -> Option<u32> {
        let br = br.as_ref()?;
        br.players.iter().find(|p| p.info.nickname == nick)
            .and_then(|p| br.player_results.iter().find(|pr| pr.info.account_id == p.account_id))
            .map(|pr| pr.info.tank_id)
    };
    let target_tank = tank_of(&shot.target_name);
    // 射手坦克：优先 battle_results 按作者昵称查找（与目标同路径、同 ID 空间）；
    // meta.tank_id 可能是车库/账号域的 ID，仅作回退。
    let author_nickname = meta.as_ref().map(|m| m.player_name.clone()).unwrap_or_default();
    let shooter_tank = tank_of(&author_nickname)
        .or_else(|| meta.as_ref().map(|m| m.tank_id as u32).filter(|v| *v > 0));
    eprintln!("[replay_shot] shot={}_{} target_name={} target_tank={:?} shooter_tank={:?} target_ang={:?}",
        shot_no, shot.damage, shot.target_name, target_tank, shooter_tank, shot.target_ang);

    // 目标坦克找不到时回退到作者坦克（至少能看到装甲）
    let viewed_tank = target_tank.or(shooter_tank).unwrap_or(0);
    start_viewer_server_with_data(tank_resolver, viewed_tank, shooter_tank, Some(serde_json::to_value(&replay_data)?)).await
}

pub async fn start_viewer_server_with_data(
    tank_resolver: TankResolver,
    tank_id: u32,
    shooter_id: Option<u32>,
    replay_shots: Option<serde_json::Value>,
) -> anyhow::Result<u16> {
    let mut app = build_viewer_router(tank_resolver, tank_id, shooter_id, "");
    if let Some(shots) = replay_shots {
        app = app.route("/api/replay_shot", get(move || {
            let shots = shots.clone();
            async move { Json(shots) }
        }));
    }
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    eprintln!("[viewer] serving tank {} (shooter {:?}) on http://127.0.0.1:{} (headless)", tank_id, shooter_id, port);
    tokio::spawn(async move { let _ = axum::serve(listener, app).await; });
    Ok(port)
}

pub(crate) async fn glb_handler(
    axum::extract::Path((tank_id, filename)): axum::extract::Path<(u32, String)>,
) -> Response {
    match ensure_glb_bytes(tank_id, &filename).await {
        Ok(bytes) => glb_response(bytes),
        Err(msg) => (axum::http::StatusCode::BAD_GATEWAY, msg).into_response(),
    }
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

    // Armor model from portable game_data/ extraction (fallback: game install)
    let game_data = crate::wargaming::game_extract::load_game_data(tank_id, &crate::data::data_dir().join("game_data"));
    let armor_model = match &game_data {
        Some(gd) => gd.armor_model.clone(),
        None => load_armor_model(tank_id),
    };
    // 碰撞包围盒数据（车体中心定位用）
    let collision_val = game_data.as_ref().and_then(|gd| gd.collision.as_ref()).map(|c| {
        serde_json::json!({
            "hull_bbox": c.hull_bbox.as_ref().map(|b| json!({"min": b.min, "max": b.max})),
        })
    }).unwrap_or(json!(null));

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
            "explosion_radius": s.explosion_radius,
        })).collect::<Vec<_>>()
    }).unwrap_or_default();

    let armor_model_val = armor_model.as_ref().map(|m| serde_json::to_value(m).unwrap_or(json!(null)));

    let configs = build_configs(tank_id);
    let caliber = configs.first().and_then(|c| c.get("caliber")).and_then(|v| v.as_u64()).unwrap_or(120) as u32;
    // 车体装甲板 spaced 列表（models.pb ModelDefinition.armor.spaced，BlitzKit 分类权威）
    let hull_spaced = crate::wargaming::blitzkit::model_info(tank_id)
        .map(|m| m.hull_spaced)
        .unwrap_or_default();
    // 模型原点（models.pb，DAVA→GLB correctZY: (x,z,y)）——装甲节点定位基准，
    // 对齐 BlitzKit SpacedArmorScene 的 hullOrigin/turretOrigin 分组装配。
    let model_origins = crate::wargaming::blitzkit::model_info(tank_id)
        .and_then(|m| match (m.track_origin, m.turret_origin) {
            (Some(tk), Some(tu)) => Some(json!({
                "track": [tk[0], tk[2], tk[1]],
                "turret": [tu[0], tu[2], tu[1]],
            })),
            _ => None,
        });
    // 初始炮塔姿态（models.pb initial_turret_rotation，度；部分车辆才有）
    let initial_turret_rotation = crate::wargaming::blitzkit::model_info(tank_id)
        .and_then(|m| m.initial_turret_rotation);

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
        "hull_spaced": hull_spaced,
        "model_origins": model_origins,
        "initial_turret_rotation": initial_turret_rotation,
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
    // 定位某主炮的权威模型节点号 + GunModelDefinition 的 thickness/mask/spaced（对齐 BlitzKit）。
    let gun_model_info = |tmod: u32, gmod: u32| -> Option<(u32, Option<f32>, Option<f32>, Vec<u32>)> {
        tmod_info.as_ref().and_then(|mi| mi.turrets.iter().find(|t| t.module_id == tmod))
            .and_then(|t| t.guns.iter().find(|g| g.gun_module_id == gmod))
            .map(|g| (g.model_node, g.thickness, g.mask, g.gun_spaced.clone()))
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
            let (gun_node, gun_thickness, gun_mask, gun_spaced) = gun_model_info(tur.module_id, gun.module_id)
                .unwrap_or((u32::MAX, None, None, Vec::new()));
            let gun_index = if gun_node != u32::MAX {
                gun_dense.get(&gun_node).copied()
            } else { None }
                .unwrap_or_else(|| *gun_idx_by_module.get(&gun.module_id).unwrap_or(&0));
            // 该炮塔的 spaced 列表（models.pb TurretModelDefinition.armor.spaced）
            let turret_spaced = tmod_info.as_ref()
                .and_then(|mi| mi.turrets.iter().find(|t| t.module_id == tur.module_id))
                .map(|t| t.turret_spaced.clone())
                .unwrap_or_default();
            // 火炮原点（models.pb TurretModelDefinition.gun_origin，DAVA→GLB correctZY: (x,z,y)）
            let gun_origin = tmod_info.as_ref()
                .and_then(|mi| mi.turrets.iter().find(|t| t.module_id == tur.module_id))
                .and_then(|t| t.gun_origin)
                .map(|d| [d[0], d[2], d[1]]);
            // 炮塔水平射界 + 炮管俯仰限制（models.pb，对齐 BlitzKit applyPitchYawLimits）
            let turret_info = tmod_info.as_ref()
                .and_then(|mi| mi.turrets.iter().find(|t| t.module_id == tur.module_id));
            let yaw_limits = turret_info.and_then(|t| t.yaw_limits.clone());
            let pitch_limits = turret_info
                .and_then(|t| t.guns.iter().find(|g| g.gun_module_id == gun.module_id))
                .and_then(|g| g.pitch_limits.clone());
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
                "penetration_far": s.penetration_far,
                "damage": s.damage,
                "module_damage": s.module_damage,
                "explosion_radius": s.explosion_radius,
                "velocity": s.velocity,
                "range": s.range,
                "caliber": s.caliber,
                "normalization": s.normalization,
                "ricochet": s.ricochet,
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
                // 炮管外部模块：厚度 + 掩体位置（models.pb GunModelDefinition，对齐 BlitzKit）
                "gun_thickness": gun_thickness,
                "gun_mask": gun_mask,
                // 装甲板 spaced 分类（models.pb 权威，对齐 BlitzKit resolveArmor）：
                // 炮管安装甲普遍为 spaced——穿透它不算击穿坦克，必须继续判定后面的主装甲。
                "gun_spaced": gun_spaced,
                "turret_spaced": turret_spaced,
                // 火炮原点（correctZY 后的 GLB 坐标，装甲定位用，对齐 BlitzKit SpacedArmorScene）
                "gun_origin": gun_origin,
                // 射界（models.pb，对齐 BlitzKit applyPitchYawLimits）
                "yaw_limits": yaw_limits,
                "pitch_limits": pitch_limits,
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
                "engines": tank.engines.clone(),
                "weight": tank.weight,
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
                "explosion_radius": s.explosion_radius,
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
        <button id="penetration-btn">穿透热力图</button>
    </div>
    <div id="tank-selectors">
        <div class="sel-row" id="config-row" style="display:none;"><label id="config-label">Config:</label><select id="config-select"></select></div>
        <div class="sel-row">
            <label>Equip:</label>
            <label style="width:auto;display:flex;align-items:center;gap:3px;cursor:pointer;font-size:0.78em;"><input type="checkbox" id="eq-calibrated"> Calib.Shells</label>
            <label style="width:auto;display:flex;align-items:center;gap:3px;cursor:pointer;font-size:0.78em;"><input type="checkbox" id="eq-enhanced"> Enh.Armor</label>
        </div>
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

        // ---------- 无头截图就绪门控 ----------
        // headless=1 时，立即发起长轮询 XHR 扣住 Chrome 虚拟时间（pending XHR 阻止
        // virtual time 推进，而 GLTFLoader 的 fetch() 不会）——模型加载/热力图渲染
        // 完成后（渲染 5 帧后 fetch /api/ready）服务器释放 hold，虚拟时间才继续，
        // 截图在就绪之后进行。SESS 由本页生成，hold/ready 配对使用。
        const SESS = Math.random().toString(36).slice(2);
        {
            const q0 = new URLSearchParams(location.search);
            if (q0.get('headless') === '1' && SESS) {
                const holdXhr = new XMLHttpRequest();
                holdXhr.open('GET', '/api/hold?sess=' + encodeURIComponent(SESS), true);
                holdXhr.send();
            }
        }
        let heatFrames = 0, heatReadySent = false;

        let scene, camera, renderer, controls;
        let raycaster, mouse;
        let tankModel = null, armorModel = null, tankData;   // tankData = target tank (model/armor/info)
        let shooterData = null;                              // shooter tank (caliber/shells)
        let shooterShells = [], shooterCaliber = 120;
        let selectedShell = null;
        let penetrationMode = false;   // 实时穿透热力图模式开关（animate/切弹/按钮共用）
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

        // 装甲板厚度检查：缺失/null 的板是装饰性非碰撞网格（不参与判定）。
        // 0mm 板是有效装甲板（0 厚度间隙/壳体：穿透后继续到后层，由后端处理 eff=0）。
        function isRealArmorThickness(t) {
            return typeof t === 'number' && t >= 0;
        }

        function thicknessToColor(t) {
            if (t === null || t === undefined) return 0x666666;
            if (t >= 200) return 0x8B0000;
            if (t >= 100) return 0xFF4500;
            if (t >= 60) return 0xFFA500;
            if (t >= 30) return 0xFFD700;
            return 0x228B22;
        }

        // ================= 实时穿透热力图（移植 BlitzKit PrimaryArmorScene 着色逻辑） =================
        // 用 GLSL fragment shader 逐像素计算当前视角下该装甲面的击穿概率并着色：
        //   - 绿色 = 稳定击穿；红色 = 稳定挡住；中间渐变 = 概率过渡
        //   - 跳弹(角度≥ricochet 且不满足三倍口径规则) → 蓝紫色高亮
        // 顶点/优化资源需要的 uniforms：thickness/penetration/caliber/ricochet/normalization。
        const PBR_VERT = `
            varying vec3 vNormal;
            varying vec3 vViewPos;
            void main() {
              vec4 mv = modelViewMatrix * vec4(position, 1.0);
              vViewPos = mv.xyz;
              vNormal = normalMatrix * normal;
              gl_Position = projectionMatrix * mv;
            }
        `;
        const PBR_FRAG = `
            precision mediump float;
            varying vec3 vNormal;
            varying vec3 vViewPos;
            uniform float thickness;
            uniform float penetration;
            uniform float caliber;
            uniform float ricochet;
            uniform float normalization;
            uniform float opacity;
            uniform bool isExplosive;   // HEAT 或 HE（BlitzKit isExplosive）
            uniform bool canSplash;     // 仅 HE（BlitzKit canSplash）
            uniform float damage;
            uniform float explosionRadius;
            uniform vec2 resolution;
            uniform float metersPerUnit;   // 视空间单位 → 米（BlitzKit 模型原生=米，本地缩放到 6 单位需换算）
            uniform sampler2D spacedArmorBuffer;   // R=外部/间隙甲 thickness/penetration, alpha!=0 表示有覆盖
            uniform highp sampler2D spacedArmorDepth; // 深度（HE 溅射用）
            uniform mat4 inverseProjectionMatrix;
            float getDist(vec2 coord, float depth) {
              vec4 clip = vec4(coord * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
              vec4 eye = inverseProjectionMatrix * clip;
              return length(eye.xyz / eye.w);
            }
            void main() {
              vec2 sc = gl_FragCoord.xy / resolution;
              vec4 spacedData = texture2D(spacedArmorBuffer, sc);
              bool underSpaced = spacedData.a != 0.0;
              // 装甲几何已转非索引并按面重算法线（每三角形三顶点法线相同），
              // 插值结果 = 精确面法线，与弹道 raycast 的 face.normal 一致。
              // abs() 抵消碰撞壳内表面/背面法线的反向（穿透厚度相同）。
              float angle = acos(clamp(abs(dot(normalize(vNormal), -normalize(vViewPos))), -1.0, 1.0));


              bool threeCal = caliber > thickness * 3.0 || underSpaced;
              bool mayRicochet = angle >= ricochet;
              float penChance = -1.0;
              float splashChance = 0.0;
              bool ricocheted = false;
              if (!threeCal && mayRicochet) {
                penChance = 0.0; ricocheted = true;
              } else {
                float ratio = thickness > 0.0 ? caliber / thickness : 0.0;
                bool twoCal = ratio > 2.0;
                float norm = twoCal ? (1.4 * normalization * caliber) / (2.0 * thickness) : normalization;
                float finalThick = thickness / cos(max(0.0, angle - norm));
                float rem = penetration;
                if (underSpaced) {
                  float spacedThick = spacedData.r * penetration;
                  rem -= spacedThick;
                  if (isExplosive && rem > 0.0) {
                    float spacedDist = getDist(sc, texture2D(spacedArmorDepth, sc).r);
                    float primaryDist = getDist(sc, gl_FragCoord.z);
                    // 深度反投影得到的是视空间单位距离；BlitzKit 模型原生单位=米可直接使用，
                    // 本地模型缩放到 6 单位（applyModelTransforms），需乘 metersPerUnit 换算回米。
                    float distArmor = (primaryDist - spacedDist) * metersPerUnit;
                    if (canSplash) {
                      float finalDamage = 0.5 * damage * (1.0 - distArmor / explosionRadius) - 1.1 * (finalThick + spacedThick);
                      splashChance = step(0.0, finalDamage);
                      penChance = 0.0;
                    } else {
                      // HEAT：间隙空气衰减（对齐 BlitzKit isExplosive && !canSplash 分支）
                      rem -= 0.5 * rem * distArmor;
                    }
                  }
                }
                if (penChance < 0.0) {
                  rem = max(0.0, rem);
                  float delta = finalThick - rem;
                  float rand = rem * 0.05;
                  penChance = clamp(1.0 - (delta + rand) / (2.0 * rand), 0.0, 1.0);
                  if (canSplash && damage > 0.0) {
                    float splash = 0.5 * damage - 1.1 * finalThick;
                    splashChance = step(0.0, splash);
                  }
                }
              }
              float alpha = 0.75;
              vec3 base = vec3(1.0, splashChance * 0.392, 0.0);
              if (ricocheted) base = vec3(1.0, base.g, 1.0);
              float fall = 1.0 - penChance * penChance;
              float gain = 1.0 - (penChance - 1.0) * (penChance - 1.0);
              gl_FragColor = vec4(fall * base + gain * vec3(0.0, 1.0, 0.0), alpha);
              gl_FragColor.a *= opacity;
            }
        `;

        // ============ 穿透热力图：完整对齐 BlitzKit SpacedArmorScene 机制 ============
        // BlitzKit 结构（来自 Armor/index.tsx 的 useFrame）：
        //   spacedArmorScene(独立 THREE.Scene) —— 单个 gl.render 用 renderOrder 排序：
        //       0: 主装甲 omit（colorWrite:false, depthWrite:true）——只为 RT 写深度，供遮挡判断
        //       1-2: 间隙甲 additive（depthWrite:false，写 R=thickness/penetration）
        //       3-4: 外部模块（3=depth 写入, 4=additive 写 R）
        //       5: 间隙甲 depth 写入
        //   渲染到 spacedArmorRenderTarget（autoClear=true，每帧重建）
        //   primaryArmorScene —— 主装甲着色 shader（renderOrder 1），读 RT，渲染到屏幕。
        // 关键：omit 与 additive 是两个独立 THREE.Mesh（一个节点产两个 mesh），renderOrder 排序，
        //     在【单次】 gl.render 内完成，深度缓冲贯穿所有 renderOrder → 后侧/被遮挡模块被正确剔除。
        //     渲染顺序（BlitzKit useFrame）：
        //       gl.autoClear=true; setRenderTarget(rt); gl.render(spacedArmorScene, camera);
        //       gl.autoClear=false; setRenderTarget(null); gl.render(scene, camera);
        //       gl.clearDepth(); gl.render(primaryArmorScene, camera);
        let penetrationRT = null;
        function ensurePenetrationRT() {
            if (!penetrationRT) {
                const c = renderer.domElement;
                penetrationRT = new THREE.WebGLRenderTarget(c.width, c.height, {
                    depthTexture: new THREE.DepthTexture(c.width, c.height),
                });
            }
            const cx = renderer.domElement;
            penetrationRT.setSize(cx.width, cx.height);
            return penetrationRT;
        }
        // 每帧渲染 spacedArmorScene 前重建 RT：尺寸变化时重建深度纹理（BlitzKit 逻辑）。
        function syncPenetrationRT() {
            const rt = ensurePenetrationRT();
            const c = renderer.domElement;
            const w = c.width, h = c.height;
            if (!rt.depthTexture || rt.depthTexture.width !== w || rt.depthTexture.height !== h) {
                if (rt.depthTexture) rt.depthTexture.dispose();
                rt.depthTexture = new THREE.DepthTexture(w, h);
            }
            rt.setSize(w, h);
            return rt;
        }

        // 角色 mesh 列表：primary(主装甲,着色) / spaced(间隙甲) / external(外部模块)。
        let primaryMeshes = [], spacedMeshes = [], externalMeshes = [];
        // 当前激活炮的掩体裁剪面（mask 数值可用时构建；THREE.Plane，世界空间）。
        // 持久平面对象：材质 clippingPlanes 引用它，逐帧原地更新即可跟随炮塔旋转/炮管俯仰
        // （新建对象会使已构建材质的引用失效）。
        let gunClipPlane = null;
        const _gunClipPlaneObj = new THREE.Plane();
        // 炮口世界坐标（视空间 6 单位制）：炮管包围盒沿炮轴的远端中点。
        // 用于点击判定的命中距离（× worldMetersPerUnit 换算回米，对齐 BlitzKit 单位）。
        let gunMuzzleWorld = null;
        // 装甲旋转枢轴（alignArmorModules 按 models.pb 原点写入：track+turret / track+turret+gun）。
        // updateTurretGun 以此为炮塔/炮管的旋转中心（对齐 BlitzKit 旋转语义）。
        let armorPivotTurret = null, armorPivotGun = null;
        // 计算激活炮的炮口位置与掩体裁剪面。裁剪面对齐 BlitzKit 的**数值公式**：
        //   maskOrigin = mask + trackY + turretY + gunY   (glb y = 前向分量之和，模型本地)
        //   平面法线 = 炮管轴向（炮口方向），过 模型原点 + 炮轴 × maskOrigin
        //   → 面后侧的炮根/炮尾段被 discard（BlitzKit: Plane((0,0,-1), -maskOrigin)）。
        // 注意：不能依赖 gun_0X_mask 网格存在——部分车辆（如 Kranvagn）mask 是纯数值，
        // model.glb 无 mask 网格，旧实现此时丢失裁剪面 → 炮尾段被错误判定为 gun barrel。
        function computeGunClipPlane() {
            gunClipPlane = null;
            gunMuzzleWorld = null;
            const act = activeGunNumber();
            const cfg = currentConfig();
            if (act == null || !tankModel) return;
            const hasMask = cfg && typeof cfg.gun_mask === 'number';
            tankModel.updateMatrixWorld(true);
            // 炮管方向（鲁棒）：取【激活炮】几何最长的网格（按父节点 gun_0X 精确过滤，
            // 避免多炮模型误选备用炮管），世界包围盒长轴，指向远离车体中心的一端（炮口方向）。
            const gunPrefix = 'gun_' + String(act).padStart(2, '0');
            let barrel = null, barrelLen = 0;
            tankModel.traverse(function(n){
                if (!n.isMesh) return;
                const pn = n.parent && n.parent.name || '';
                if (pn !== gunPrefix) return;
                if (!n.geometry.boundingBox) n.geometry.computeBoundingBox();
                const b = n.geometry.boundingBox;
                const len = Math.max(b.max.x-b.min.x, b.max.y-b.min.y, b.max.z-b.min.z);
                if (len > barrelLen) { barrelLen = len; barrel = n; }
            });
            let dir = new THREE.Vector3(0, 1, 0);
            if (barrel) {
                const wb = new THREE.Box3().setFromObject(barrel);
                const sz = new THREE.Vector3(); wb.getSize(sz);
                let axis = 'x';
                if (sz.y >= sz.x && sz.y >= sz.z) axis = 'y';
                else if (sz.z >= sz.x && sz.z >= sz.y) axis = 'z';
                const a = wb.min.clone(), b2 = wb.max.clone();
                if (axis === 'x') { a.y = b2.y = (wb.min.y+wb.max.y)/2; a.z = b2.z = (wb.min.z+wb.max.z)/2; }
                if (axis === 'y') { a.x = b2.x = (wb.min.x+wb.max.x)/2; a.z = b2.z = (wb.min.z+wb.max.z)/2; }
                if (axis === 'z') { a.x = b2.x = (wb.min.x+wb.max.x)/2; a.y = b2.y = (wb.min.y+wb.max.y)/2; }
                const origin = new THREE.Vector3(0, 0, 0);
                // 炮口方向判定：以 models.pb 火炮原点（炮耳轴）为基准——炮管两端中距耳轴
                // 更远的一端为炮口。旧实现按"远离世界原点"判向，而模型包围盒中心经居中
                // 后恰在原点，两端等距导致方向随机反转（裁剪面保留炮尾侧、裁掉炮口侧，
                // 炮尾段仍有 gun barrel 判定）。
                const mo0 = tankData && tankData.model_origins;
                const cfg0 = currentConfig();
                let gunOriginWorld = null;
                if (mo0 && mo0.track && mo0.turret && cfg0 && cfg0.gun_origin) {
                    const g = new THREE.Vector3(
                        mo0.track[0] + mo0.turret[0] + cfg0.gun_origin[0],
                        mo0.track[1] + mo0.turret[1] + cfg0.gun_origin[1],
                        mo0.track[2] + mo0.turret[2] + cfg0.gun_origin[2]);
                    gunOriginWorld = tankModel.localToWorld(g);
                }
                if (gunOriginWorld) {
                    const dA = a.distanceToSquared(gunOriginWorld);
                    const dB = b2.distanceToSquared(gunOriginWorld);
                    dir = (dA > dB) ? a.sub(b2) : b2.sub(a);
                } else {
                    dir = (a.distanceToSquared(origin) > b2.distanceToSquared(origin)) ? a.sub(b2) : b2.sub(a);
                }
                dir.normalize();
                // 炮口 = 炮管包围盒沿 dir 的远端中点（dir 为轴对齐单位向量）
                const center = wb.getCenter(new THREE.Vector3());
                const halfAlongDir = (Math.abs(dir.x)*sz.x + Math.abs(dir.y)*sz.y + Math.abs(dir.z)*sz.z) / 2;
                gunMuzzleWorld = center.add(dir.clone().multiplyScalar(halfAlongDir));
            }
            if (!hasMask) return;
            // ---- 裁剪面：models.pb 数值公式（对齐 BlitzKit maskOrigin）----
            // mask 数值为 proto required 字段、723/723 全覆盖；旧"mask 网格包围盒"回退
            // 已移除（部分车辆如 Kranvagn 无 mask 网格，曾导致裁剪面丢失）。
            const mo = tankData && tankData.model_origins;
            if (!(mo && mo.track && mo.turret && cfg.gun_origin && barrel)) return;
            // glb y = 前向分量：maskOrigin = mask + trackY + turretY + gunY（模型本地单位）
            const maskOriginModel = cfg.gun_mask + mo.track[1] + mo.turret[1] + cfg.gun_origin[1];
            // 模型原点的世界坐标 + 炮轴方向 × (模型本地距离 / worldMetersPerUnit → 世界距离)
            const originWorld = new THREE.Vector3(); tankModel.localToWorld(originWorld.set(0, 0, 0));
            const mpu = worldMetersPerUnit || 1;
            const point = originWorld.add(dir.clone().multiplyScalar(maskOriginModel / mpu));
            gunClipPlane = _gunClipPlaneObj.setFromNormalAndCoplanarPoint(dir, point);
        }
        function collectPenetrationMeshes() {
            primaryMeshes = []; spacedMeshes = [];
            armorModel.traverse(function(node) {
                if (!node.isMesh) return;
                // 隐藏网格与非激活配置的炮塔/炮管装甲（configHidden）不进热力图
                // （对齐 BlitzKit：两场景只渲染当前 model_id 对应的装甲节点）。
                if (node.visible === false || node.userData.configHidden) return;
                const sec = node.userData.armorSection;
                if (sec === 'deco') return;
                if (sec === 'spaced') spacedMeshes.push(node);
                else primaryMeshes.push(node);
            });
            // 外部模块筛选（对齐 BlitzKit SpacedArmorScene 的 gun external 逻辑）：
            //   - 只取当前激活配置的炮（多炮管车不渲染未选中的炮管）
            //   - 该炮定义了 mask：包含 gun_0X* 全部网格（炮管+掩体外观），按掩体面裁剪
            //   - 未定义 mask：只取精确 gun_0X 节点的网格（炮管本体）——判定范围与视觉一致
            externalMeshes = [];
            const act = activeGunNumber();
            const cfg = currentConfig();
            const hasMask = cfg && typeof cfg.gun_mask === 'number';
            computeGunClipPlane();
            moduleMeshes.forEach(function(node) {
                if (!node.isMesh) return;
                if (node.visible === false) return;
                if (node.userData.gunConfig != null && act != null && node.userData.gunConfig !== act) return;
                if (node.userData.gunMaskPart && !hasMask) return;
                externalMeshes.push(node);
            });
        }

        // ---------- 材质 ----------
        // omit：只写深度（BlitzKit renderOrder 0/3/5）。
        const omitMaterial = new THREE.MeshBasicMaterial({ colorWrite: false, depthTest: true, depthWrite: true });
        // 主装甲 exclude 材质：只写深度、不写颜色（BlitzKit PrimaryArmorSceneComponent 的 excludeMaterial，
        // renderOrder 0）。用于遮挡后侧/背面装甲板，避免透视看到被遮挡板 & 透明闪烁。
        const excludeMaterial = new THREE.MeshBasicMaterial({ colorWrite: false, depthTest: true, depthWrite: true });
        // 主装甲着色材质（读 RT）。
        function penetrationMaterial(thickness) {
            return new THREE.ShaderMaterial({
                vertexShader: PBR_VERT, fragmentShader: PBR_FRAG,
                transparent: true, depthWrite: false,
                uniforms: {
                    thickness: { value: thickness },
                    penetration: { value: 200 },
                    caliber: { value: 120 },
                    ricochet: { value: 70.0 * Math.PI / 180 },
                    normalization: { value: 5.0 * Math.PI / 180 },
                    isExplosive: { value: false },
                    canSplash: { value: false },
                    damage: { value: 0 },
                    explosionRadius: { value: 0 },
                    resolution: { value: new THREE.Vector2(1, 1) },
                    metersPerUnit: { value: 1 },
                    opacity: { value: 1 },
                    spacedArmorBuffer: { value: emptySpacedTexture() },
                    spacedArmorDepth: { value: null },
                    inverseProjectionMatrix: { value: null },
                },
            });
        }
        // 外部模块（履带/负重轮/炮管）：flat，Additive 写 R=thickness/penetration。
        // 需支持掩体裁剪面：ShaderMaterial 必须 clipping:true 且 shader 包含 clipping chunks
        // （对齐 BlitzKit SpacedArmorSubExternal 的 shader 与材质设置）。
        const EXTERNAL_VERT = `
            #include <clipping_planes_pars_vertex>
            void main() {
              #include <begin_vertex>
              #include <project_vertex>
              #include <clipping_planes_vertex>
            }
        `;
        const EXTERNAL_FRAG = `
            uniform float thickness;
            uniform float penetration;
            #include <clipping_planes_pars_fragment>
            void main() {
              #include <clipping_planes_fragment>
              gl_FragColor = vec4(thickness / max(penetration, 1.0), 0.0, 0.0, 1.0);
            }
        `;
        function externalMaterial(thickness, penetration, clip) {
            return new THREE.ShaderMaterial({
                vertexShader: EXTERNAL_VERT, fragmentShader: EXTERNAL_FRAG,
                // 注意：不可设 transparent:true——three.js 会把它挪进 transparent 渲染列表，
                // 而 opaque 列表永远先渲染，导致 RT 内深度 pass(3/5) 先于着色 pass(2/4) 执行，
                // 被外部模块遮挡的间隙甲/附加装甲的 R 消耗会被错误剔除。
                // BlitzKit 的 SubSpaced/SubExternal 着色材质同样不设 transparent（保持 opaque）。
                depthWrite: false, depthTest: true,
                blending: THREE.AdditiveBlending,
                clipping: clip != null,
                clippingPlanes: clip ? [clip] : null,
                uniforms: { thickness: { value: thickness }, penetration: { value: penetration || 200 } },
            });
        }
        // 间隙甲：带角度/转正/三倍口径，Additive 写 R=finalThickness/penetration。
        const SPACED_VERT = PBR_VERT;
        const SPACED_FRAG = `
            precision mediump float;
            varying vec3 vNormal;
            varying vec3 vViewPos;
            uniform float thickness;
            uniform float penetration;
            uniform float caliber;
            uniform float ricochet;
            uniform float normalization;
            void main() {
              // 与主装甲 shader 同式：单位向量点积（abs 抵消双面法线反向）。
              // BlitzKit SubSpaced_frag.glsl: acos(dot(vNormal, -vViewPosition)/length(vViewPosition))
              float angle = acos(clamp(abs(dot(normalize(vNormal), -normalize(vViewPos))), -1.0, 1.0));
              bool threeCal = caliber > thickness * 3.0;
              if (!threeCal && angle >= ricochet) { gl_FragColor = vec4(1.0, 0.0, 0.0, 1.0); return; }
              bool twoCal = caliber > thickness * 2.0 && thickness > 0.0;
              float norm = twoCal ? (1.4 * normalization * caliber) / (2.0 * thickness) : normalization;
              float finalThick = thickness / cos(max(0.0, angle - norm));
              gl_FragColor = vec4(finalThick / max(penetration, 1.0), 0.0, 0.0, 1.0);
            }
        `;
        function spacedMaterial(thickness, penetration) {
            return new THREE.ShaderMaterial({
                vertexShader: SPACED_VERT, fragmentShader: SPACED_FRAG,
                // 不可设 transparent:true（见 externalMaterial 注释）——RT 内必须保持
                // opaque 列表以让 renderOrder 严格决定 0→2→3→4→5 的执行顺序（对齐 BlitzKit）。
                depthWrite: false, depthTest: true,
                blending: THREE.AdditiveBlending,
                uniforms: {
                    thickness: { value: thickness }, penetration: { value: penetration || 200 },
                    caliber: { value: 120 }, ricochet: { value: 70 * Math.PI / 180 }, normalization: { value: 5 * Math.PI / 180 },
                },
            });
        }
        // 装备修正（对齐 BlitzKit）：Calibrated Shells 穿深 ×1.06(AP/APCR)/×1.07(HEAT/HE)、
        // Enhanced Armor 装甲厚度 ×1.04。同时作用于热力图 uniforms 与点击判定请求。
        // 弹种类型归一化：tanks.pb 原始串（hc_premium/ap_cr*/he_premium/ap_premium）
        // → BlitzKit ShellType（heat/apcr/he/ap）。BlitzKit 在解析 pb 时即映射为枚举，
        // 前端所有弹种判断（isExplosive/canSplash/装备系数/跳弹角）必须基于归一化后
        // 的类型——直接比较原始串会导致 HEAT 间隙衰减、HE 溅射等分支永不触发。
        function shellTypeOf(sh) {
            const t = ((sh && sh.type) || '').toLowerCase();
            if (t === 'hc' || t === 'hc_premium' || t === 'heat') return 'heat';
            if (t === 'ap_cr' || t === 'ap_cr_premium' || t === 'apcr') return 'apcr';
            if (t === 'he' || t === 'he_premium') return 'he';
            if (t === 'ap' || t === 'ap_premium') return 'ap';
            return t;
        }
        // 弹种正式显示名（开发名 ap_cr/hc_premium → APCR/HEAT）
        const SHELL_LABEL = { ap: 'AP', apcr: 'APCR', heat: 'HEAT', he: 'HE' };
        function shellLabel(s) { return SHELL_LABEL[shellTypeOf(s)] || (s && s.type) || '?'; }
        // Calibrated Shells 穿深系数（分弹种，对齐 BlitzKit resolvePenetrationCoefficient）
        function shellPenMul(s) {
            const calEl = document.getElementById('eq-calibrated');
            if (!(calEl && calEl.checked)) return 1.0;
            const t = shellTypeOf(s);
            return (t === 'ap' || t === 'apcr') ? 1.06 : 1.07;
        }
        // 弹种选项文本：正式名 + 联动 Calib.Shells 的穿深
        function shellOptionText(s) {
            const pen = Math.round((s.penetration || 0) * shellPenMul(s));
            return `${shellLabel(s)} ${pen}mm / ${s.damage || 0}dmg`;
        }
        function equipmentCoeffs() {
            const enhEl = document.getElementById('eq-enhanced');
            const penMul = shellPenMul(selectedShell);
            const thickMul = (enhEl && enhEl.checked) ? 1.04 : 1.0;
            return { penMul, thickMul };
        }
        // Enhanced Armor 开关变化时按克隆上记录的基础厚度重设 thickness uniforms。
        function updateHeatmapThickness() {
            const { thickMul } = equipmentCoeffs();
            const apply = function(obj) {
                if (!obj) return;
                obj.traverse(function(node){
                    if (!node.isMesh || !node.material || !node.material.uniforms) return;
                    const u = node.material.uniforms;
                    if (u.thickness && node.userData._baseThickness != null) {
                        u.thickness.value = node.userData._baseThickness * thickMul;
                    }
                });
            };
            apply(spacedArmorScene); apply(primaryArmorScene);
        }
        // 外部模块/间隙甲"只写深度"材质（BlitzKit renderOrder 3/5，镂空简化几何）。
        const externalDepthMaterial = new THREE.MeshBasicMaterial({ colorWrite: false, depthWrite: true, depthTest: true });
        let _emptySpacedTex = null;
        function emptySpacedTexture() {
            if (!_emptySpacedTex) {
                const d = new Uint8Array([0, 0, 0, 0]);
                _emptySpacedTex = new THREE.DataTexture(d, 1, 1, THREE.RGBAFormat);
                _emptySpacedTex.needsUpdate = true;
            }
            return _emptySpacedTex;
        }
        let penetrationActive = false;
        // 单独场景：渲染进 RT 的"深度+外部/间隙甲"层（BlitzKit 的 spacedArmorScene）。
        let spacedArmorScene = null;
        let primaryArmorScene = null;
        // 主装甲着色 mesh（读 RT 的材质装入 armorModel，直接在主 scene 渲染）。
        // spacedArmorScene 中所有元素：primary-omit(order0) + spaced(additive2/depth5) + external(depth3/additive4)。

        // 给定一个 armor/mesh，创建其"只写深度"克隆（记录 source，供每帧同步世界矩阵 → 跟随炮塔旋转/炮管俯仰）。
        function addOmitClone(scene, src, renderOrder) {
            const m = new THREE.Mesh(src.geometry, omitMaterial);
            m.renderOrder = renderOrder;
            m.userData._src = src;
            src.updateWorldMatrix(true, false);
            m.matrixAutoUpdate = false;
            m.matrix.copy(src.matrixWorld);
            scene.add(m);
            return m;
        }
        // 给定一个 armor/mesh，创建其"着色"克隆（记录 source，供每帧同步世界矩阵）。
        function addColorClone(scene, src, mat, renderOrder) {
            const m = new THREE.Mesh(src.geometry, mat);
            m.renderOrder = renderOrder;
            m.userData._src = src;
            src.updateWorldMatrix(true, false);
            m.matrixAutoUpdate = false;
            m.matrix.copy(src.matrixWorld);
            scene.add(m);
            return m;
        }

        // 构建 spacedArmorScene（进入热力图模式时调用一次）。
        function buildSpacedArmorScene() {
            if (!armorModel) return;
            if (spacedArmorScene) spacedArmorScene.clear();
            else spacedArmorScene = new THREE.Scene();
            const pen = ((selectedShell && selectedShell.penetration) || 200) * equipmentCoeffs().penMul;
            // 0: 主装甲 omit（写深度，排除后侧遮挡）——只需 hull/turret 非 spaced
            primaryMeshes.forEach(function(node){ addOmitClone(spacedArmorScene, node, 0); });
            // 2/5: 间隙甲 additive + depth
            spacedMeshes.forEach(function(node){
                const t = node.userData.armorThickness || 0;
                const cm = addColorClone(spacedArmorScene, node, spacedMaterial(t, pen), 2);
                cm.userData._baseThickness = t;
                addOmitClone(spacedArmorScene, node, 5);
            });
            // 3/4: 外部模块 depth + additive
            externalMeshes.forEach(function(node){
                const t = node.userData.armorThickness || 20;
                // 裁剪面作用于**整个 gun external 组件**（炮管+掩体，对齐 BlitzKit——其 clip
                // 传给整个 SpacedArmorSceneComponent）；旧实现只裁掩体网格，炮管本体的
                // 炮尾段（钻进炮塔的部分）会在 RT 中被错误判定。履带/负重轮不裁剪。
                const clipped = node.userData.armorSection === 'gunBarrel' && gunClipPlane;
                // 掩体部件 depth 克隆用 MeshBasicMaterial（原生支持 clipping），
                // color 克隆的 ShaderMaterial 经 externalMaterial(…, clip) 启用 clipping chunks
                const dmat = clipped
                    ? (() => { const m = externalDepthMaterial.clone(); m.clippingPlanes = [gunClipPlane]; return m; })()
                    : externalDepthMaterial;
                const cmat = externalMaterial(t, pen, clipped ? gunClipPlane : null);
                const od = addOmitClone(spacedArmorScene, node, 3);
                od.material = dmat;
                const cm2 = addColorClone(spacedArmorScene, node, cmat, 4);
                cm2.userData._baseThickness = t;
            });
        }
        // 构建 primaryArmorScene（进入热力图模式时调用一次）。
        // 每个主装甲 mesh 生成两个克隆，与 BlitzKit PrimaryArmorSceneComponent 一致：
        //   exclude(renderOrder 0, colorWrite:false, depthWrite:true) —— 只写正面深度，遮挡后侧/背面板
        //   include(renderOrder 1, 穿透着色 shader) —— 读 RT，渲染到屏幕
        // 渲染到屏幕前 gl.clearDepth()，让 exclude 的正面深度 cut 掉被遮挡/背面装甲板的颜色。
        function buildPrimaryArmorScene() {
            if (!armorModel) return;
            if (primaryArmorScene) primaryArmorScene.clear();
            else primaryArmorScene = new THREE.Scene();
            primaryMeshes.forEach(function(node){
                const t = node.userData.armorThickness;
                addDepthExcludeClone(primaryArmorScene, node, 0);                        // 正面深度遮罩
                const cm = addColorClone(primaryArmorScene, node, penetrationMaterial(t == null ? 0 : t), 1);  // 着色
                cm.userData._baseThickness = t == null ? 0 : t;
            });
        }
        // 主装甲"排除/深度"克隆（colorWrite:false, depthWrite:true）。
        function addDepthExcludeClone(scene, src, renderOrder) {
            const m = new THREE.Mesh(src.geometry, excludeMaterial);
            m.renderOrder = renderOrder;
            m.userData._src = src;
            src.updateWorldMatrix(true, false);
            m.matrixAutoUpdate = false;
            m.matrix.copy(src.matrixWorld);
            scene.add(m);
            return m;
        }
        // 每帧同步：把 spacedArmorScene/primaryArmorScene 内所有克隆的世界矩阵从源节点刷新，
        // 使热力图跟随炮塔旋转/炮管俯仰（BlitzKit 直接渲染活节点，故需手动同步 matrixWorld）。
        function syncCloneMatrices(obj) {
            if (!obj) return;
            obj.traverse(function(node){
                const src = node.userData && node.userData._src;
                if (src) {
                    src.updateWorldMatrix(true, false);
                    node.matrix.copy(src.matrixWorld);
                }
            });
        }
        // 更新 spacedArmorScene 中所有材质的 uniforms（切弹/装备）。
        function updateSpacedUniforms(sh) {
            if (!spacedArmorScene) return;
            const t = shellTypeOf(sh);
            const isExplosive = t === 'he' || t === 'heat';
            const { penMul } = equipmentCoeffs();
            const pen = (sh.penetration || 0) * penMul;
            const cal = sh.caliber || 120;
            const norm = (sh.normalization != null ? sh.normalization : 5) * Math.PI / 180;
            // BlitzKit：HE/HEAT 跳弹角强制 90°（isExplosive ? 90 : shell.ricochet）
            const rico = (isExplosive ? 90 : (sh.ricochet != null ? sh.ricochet : 70)) * Math.PI / 180;
            spacedArmorScene.traverse(function(node){
                if (!node.isMesh || !node.material || !node.material.uniforms) return;
                const u = node.material.uniforms;
                if (u.penetration) u.penetration.value = pen;
                if (u.caliber) u.caliber.value = cal;
                if (u.ricochet) u.ricochet.value = rico;
                if (u.normalization) u.normalization.value = norm;
            });
        }

        // 释放热力图相关 WebGL 资源（材质/深度纹理），避免切弹/开关叠加泄漏 GPU 纹理与程序。
        function disposeMaterial(mat) {
            if (!mat) return;
            if (mat.uniforms) {
                for (const u of Object.values(mat.uniforms)) {
                    const v = u && u.value;
                    // 跳过共享的 1x1 空纹理（._emptySpacedTex 被多材质复用）与 RT 纹理
                    if (v && v.isTexture && v !== _emptySpacedTex && !v.isRenderTargetTexture) v.dispose();
                }
            }
            if (mat.map && mat.map.isTexture && mat.map !== _emptySpacedTex) mat.map.dispose();
            mat.dispose();
        }
        function disposePenetrationResources() {
            // primaryArmorScene 内所有 mesh 的材质（穿透着色 shader）
            if (primaryArmorScene) {
                primaryArmorScene.traverse(function(node){
                    if (node.isMesh) { if (node.material && node.material.uniforms) disposeMaterial(node.material); }
                });
                primaryArmorScene.clear();
            }
            // spacedArmorScene 内所有 mesh 的材质
            if (spacedArmorScene) {
                spacedArmorScene.traverse(function(node){
                    if (node.isMesh) { if (node.material && node.material.uniforms) disposeMaterial(node.material); }
                });
                spacedArmorScene.clear();
            }
            // RT + 深度纹理
            if (penetrationRT) {
                if (penetrationRT.depthTexture) penetrationRT.depthTexture.dispose();
                penetrationRT.dispose();
                penetrationRT = null;
            }
        }

        // 进入/退出热力图模式。
        // （重）构建热力图两个场景并刷新 uniforms——进入模式与切换配置时调用。
        // 先释放旧资源，再按当前配置/弹种/装备重建（对齐 BlitzKit 的响应式重建）。
        function rebuildHeatmapScenes() {
            disposePenetrationResources();
            collectPenetrationMeshes();
            buildSpacedArmorScene();
            buildPrimaryArmorScene();
            const sh = selectedShell || (shooterShells[0]) || null;
            if (sh) { updatePenetrationUniforms(sh); updateSpacedUniforms(sh); }
            updateHeatmapThickness();
            refreshPenetrationResolution();
        }
        function applyPenetrationMode(on) {
            if (!armorModel) return;
            if (!on) {
                // 先释放热力图资源（材质/RT/深度纹理），防止 GPU 内存随开关/切弹累积泄漏
                disposePenetrationResources();
                if (tankModel) tankModel.visible = true;
                // armorModel 恢复可见（射线检测用；材质透明不渲染）
                armorModel.visible = true;
                armorModel.traverse(function(node) {
                    if (!node.isMesh) return;
                    if (node.userData.armorSection === 'deco') { node.visible = false; return; }
                    node.material = new THREE.MeshStandardMaterial({
                        color: 0x444444, metalness: 0.3, roughness: 0.8,
                        transparent: true, opacity: 0, depthWrite: false,
                    });
                });
                moduleMeshes.forEach(function(node){ if (node.isMesh) node.visible = true; });
                // 离开热力图：恢复默认 autoClear 与空渲染目标
                renderer.autoClear = true;
                renderer.setRenderTarget(null);
                return;
            }
            // 视觉模型保持可见（对齐 BlitzKit：根场景渲染视觉坦克，热力图颜色为
            // clearDepth 后叠加的半透明层）——炮管/履带等外部模块因而不消失
            if (tankModel) tankModel.visible = true;
            rebuildHeatmapScenes();
            // armorModel 保持 visible=true（raycast 需要），但其材质为 transparent opacity:0 不绘制；
            // 着色由 primaryArmorScene 单独渲染。
            armorModel.visible = true;
            // 外部模块源网格（履带/炮管/炮盾外观）保持可见——它们是视觉外观的一部分，
            // RT 由其独立克隆渲染，不冲突；隐藏反而会使视觉坦克缺件（对齐 BlitzKit）。
            externalMeshes.forEach(function(node){ node.visible = true; });
            penetrationActive = true;
        }

        // 更新主装甲着色材质 uniforms（切弹/装备）。
        function updatePenetrationUniforms(sh) {
            const t = shellTypeOf(sh);
            const isHE = t === 'he';
            const isExplosive = isHE || t === 'heat';
            const cal = sh.caliber || 120;
            const { penMul } = equipmentCoeffs();
            const pen = (sh.penetration || 0) * penMul;
            const norm = (sh.normalization != null ? sh.normalization : 5) * Math.PI / 180;
            const rico = (isExplosive ? 90 : (sh.ricochet != null ? sh.ricochet : 70)) * Math.PI / 180;
            const applyTo = (node) => {
                if (!node.isMesh || node.visible === false) return;
                if (!node.material || !node.material.uniforms) return;
                const u = node.material.uniforms;
                if (u.penetration) u.penetration.value = pen;
                if (u.caliber) u.caliber.value = cal;
                if (u.ricochet) u.ricochet.value = rico;
                if (u.normalization) u.normalization.value = norm;
                if (u.isExplosive) u.isExplosive.value = isExplosive;
                if (u.canSplash) u.canSplash.value = isHE;
                if (u.damage) u.damage.value = sh.damage || 0;
                if (u.explosionRadius) u.explosionRadius.value = sh.explosion_radius || 0;
            };
            if (primaryArmorScene) primaryArmorScene.traverse(function(node){ if (node.isMesh) applyTo(node); });
        }
        function refreshPenetrationResolution() {
            const c = renderer.domElement;
            const apply = (obj) => obj.traverse(function(node) {
                if (node.isMesh && node.material && node.material.uniforms && node.material.uniforms.resolution) {
                    node.material.uniforms.resolution.value.set(c.width, c.height);
                    // 视空间单位 → 米换算系数（随目标模型加载更新）
                    if (node.material.uniforms.metersPerUnit) {
                        node.material.uniforms.metersPerUnit.value = worldMetersPerUnit || 1;
                    }
                }
            });
            if (primaryArmorScene) apply(primaryArmorScene);
        }

        // 每帧：把 spacedArmorScene 渲染进 RT，再把 RT 注入主装甲着色材质（BlitzKit useFrame）。
        function renderSpacedArmorPass() {
            if (!spacedArmorScene) return;
            const rt = syncPenetrationRT();
            // 同步克隆世界矩阵（接收源节点即时的炮塔/炮管变换）
            syncCloneMatrices(spacedArmorScene);
            // 裁剪面逐帧跟随炮塔旋转/炮管俯仰（原地更新持久平面对象——材质引用不变）
            computeGunClipPlane();
            // 注入 RT/深度/投影逆到主装甲着色材质（primaryArmorScene 内的穿透 shader）
            const inject = (obj) => { obj.traverse(function(node){
                if (node.isMesh && node.material && node.material.uniforms && node.material.uniforms.spacedArmorBuffer) {
                    node.material.uniforms.spacedArmorBuffer.value = rt.texture;
                    if (node.material.uniforms.spacedArmorDepth && rt.depthTexture) node.material.uniforms.spacedArmorDepth.value = rt.depthTexture;
                    if (node.material.uniforms.inverseProjectionMatrix) node.material.uniforms.inverseProjectionMatrix.value = camera.projectionMatrixInverse;
                }
            }); };
            if (primaryArmorScene) inject(primaryArmorScene);
            // gl.autoClear=true; setRenderTarget(rt); gl.render(spacedArmorScene, camera)
            renderer.autoClear = true;
            renderer.setRenderTarget(rt);
            renderer.setClearColor(0x000000, 0);
            renderer.render(spacedArmorScene, camera);
            // gl.autoClear=false; setRenderTarget(null) —— 主场景渲染由 animate 完成
            renderer.autoClear = false;
            renderer.setRenderTarget(null);
        }

        document.getElementById('penetration-btn').addEventListener('click', function() {
            penetrationMode = !penetrationMode;
            this.classList.toggle('active', penetrationMode);
            this.textContent = penetrationMode ? '关闭热力图' : '穿透热力图';
            applyPenetrationMode(penetrationMode);
        });

        // spaced 分类（对齐 BlitzKit resolveArmor：models.pb 的 armor.spaced 为权威）：
        //   hull   → tankData.hull_spaced（回退 XML vehicleDamageFactor）
        //   turret → 当前配置 turret_spaced（回退 XML）
        //   gun    → 当前配置 gun_spaced（回退 XML）——炮管安装甲在 BlitzKit 数据中普遍为
        //            spaced（间隙甲）：穿透它不算击穿坦克，炮弹必须继续判定后面的
        //            车体/炮塔主装甲；非 spaced 的炮管板按主装甲（turret 语义）处理。
        // 配置切换（炮塔/主炮变化）后需重新调用（turret/gun 的 spaced 列表随之变化）。
        function retagSpacedSections(model) {
            const root = model || armorModel;
            if (!root) return;
            const cfg = currentConfig();
            const am = tankData.armor_model || {};
            const hullSpaced = new Set((tankData.hull_spaced ?? am.hull?.spaced ?? []).map(String));
            const turretSpaced = new Set((cfg?.turret_spaced ?? am.turret?.spaced ?? []).map(String));
            const gunSpaced = new Set((cfg?.gun_spaced ?? am.gun?.spaced ?? []).map(String));
            root.traverse(function(node) {
                if (!node.isMesh) return;
                const section = node.userData.armorSectionOrig;
                if (section !== 'hull' && section !== 'turret' && section !== 'gun') return;
                const plateId = String(node.userData.armorPlateId);
                const spaced = (section === 'hull' && hullSpaced.has(plateId)) ||
                               (section === 'turret' && turretSpaced.has(plateId)) ||
                               (section === 'gun' && gunSpaced.has(plateId));
                // spaced → 'spaced'（角度等效消耗，不阻止判定、穿透不算击穿坦克）；
                // 其余 → 主装甲盒语义（gun 板归入 'turret'：角度等效、可跳弹、构成主装甲）。
                node.userData.armorSection = spaced ? 'spaced' : (section === 'gun' ? 'turret' : section);
            });
        }

        function tagArmorPlates(model) {
            model.traverse(function(node) {
                if (!node.isMesh) return;
                const name = node.name || '';
                // 动态装甲双状态网格：只保留默认态 state_00，state_01 按装饰网格剔除。
                // （BlitzKit 单 state 渲染；其两个场景对默认态的选择相反——spaced 场景默认
                //   state_00、primary 场景默认 state_01——此处统一取 state_00，避免双份
                //   装甲板叠加进 RT/着色层与点击判定。）
                if (/state_01/.test(name)) {
                    node.userData.armorSection = 'deco';
                    node.userData.armorPlateId = '';
                    node.userData.armorThickness = 0;
                    return;
                }
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
                    // 记录原始 section / 板号 / 厚度；spaced vs 主装甲分类由
                    // retagSpacedSections() 按 models.pb（回退 XML）完成。
                    node.userData.armorSectionOrig = section;
                    node.userData.armorPlateId = plateId;
                    node.userData.armorThickness = t;
                    node.userData.armorSection = section;
                }
            });
            retagSpacedSections(model);
        }

        function tagModuleMeshes(model) {
            moduleMeshes = [];
            model.traverse(function(node) {
                if (!node.isMesh) return;
                const parent = node.parent;
                const parentName = parent ? (parent.name || '') : '';
                // 履带/负重轮：沿祖先链匹配 chassis_track_* / chassis_wheel_*（对齐 BlitzKit
                // 按节点名前缀判定——负重轮多为顶层节点，不在 chassis_track_L/R 之下）。
                // 两者都按 External(track) 处理，厚度 = 对应侧履带厚度。
                let trackNode = null;
                for (let p = node; p; p = p.parent) {
                    const nm = p.name || '';
                    if (/^chassis_track_/.test(nm) || /^chassis_wheel_/.test(nm)) { trackNode = nm; break; }
                }
                if (trackNode) {
                    const isRight = /(^|_)R(_|$)/.test(trackNode);
                    node.userData.armorSection = 'chassis';
                    node.userData.armorPlateId = isRight ? 'rightTrack' : 'leftTrack';
                    node.userData.armorThickness = getPlateThickness('chassis', node.userData.armorPlateId);
                    moduleMeshes.push(node);
                } else if (/^gun_\d+$/.test(parentName)) {
                    // 炮管本体（父节点精确 gun_0X）——BlitzKit：无 mask 时外部模块只含此节点
                    node.userData.armorSection = 'gunBarrel';
                    node.userData.armorPlateId = 'gun';
                    node.userData.armorThickness = getPlateThickness('gunBarrel', 'gun');
                    node.userData.gunMaskPart = false;
                    // 记录所属 gun 配置组序号(如 gun_04→4)，供按配置过滤 raycast 与可见性。
                    const gm = parentName.match(/^gun_(\d+)/);
                    node.userData.gunConfig = gm ? parseInt(gm[1], 10) : null;
                    moduleMeshes.push(node);
                } else if (/^gun_\d+_/.test(parentName)) {
                    // 炮盾/掩体网格（父节点 gun_0X_mask* 等）——仅当该炮在 models.pb 定义了
                    // mask 时才作为外部模块（并按掩体平面裁剪），否则不参与（对齐 BlitzKit）。
                    node.userData.armorSection = 'gunBarrel';
                    node.userData.armorPlateId = 'gun';
                    node.userData.armorThickness = getPlateThickness('gunBarrel', 'gun');
                    node.userData.gunMaskPart = true;
                    const gm = parentName.match(/^gun_(\d+)/);
                    node.userData.gunConfig = gm ? parseInt(gm[1], 10) : null;
                    moduleMeshes.push(node);
                }
            });
        }

        // 模块装甲（炮塔/炮盾）与视觉模型的对齐统一由 alignArmorModules() 完成；
        // 车体(hull) 的 game points 恒接近 0，无需偏移，故不再对碰撞模型做预平移。
        let worldMetersPerUnit = null;   // 场景单位→米换算系数（maxDim/6，随模型加载更新）
        // applyModelTransforms：缩放到 6 单位 + 【车体中心水平对齐原点】+ 贴地。
        // bodyCenter = 车体中心（glb 坐标系，来自 collision.hull_bbox 中心）；
        // 水平居中基准用车体中心而非全模型包围盒中心（后者被炮管前伸拉偏，
        // 导致 aim_point 等以车体中心为基准的数据在 viewer 中错位）。
        // y 仍用全模型贴地（glb z_min=0 即地面，visual/collision 的 z 原点一致）。
        function applyModelTransforms(model, bodyCenter) {
            model.rotation.x = -Math.PI / 2;
            const box = new THREE.Box3().setFromObject(model);
            const size = box.getSize(new THREE.Vector3());
            const maxDim = Math.max(size.x, size.y, size.z);
            const scale = 6 / maxDim;
            worldMetersPerUnit = maxDim / 6;
            model.scale.setScalar(scale);
            // 车体中心旋转后位置：glb(x,y,z) →(x, z, -y)
            const bc = bodyCenter || box.getCenter(new THREE.Vector3());
            const bcRot = new THREE.Vector3(bc.x, bc.z, -bc.y).multiplyScalar(scale);
            // 水平对齐：position 平移使车体中心 x/z 归零
            model.position.x = -bcRot.x;
            model.position.z = -bcRot.z;
            // 贴地：全模型 z_min(旋转后 y_min) = 0
            const box2 = new THREE.Box3().setFromObject(model);
            model.position.y = -box2.min.y;
        }

        function syncTransforms() {
            if (!tankModel || !armorModel) return;
            armorModel.rotation.copy(tankModel.rotation);
            armorModel.scale.copy(tankModel.scale);
            armorModel.position.copy(tankModel.position);
            collectConfigNodes(tankModel);
            alignArmorModules();
        }

        // Align the armor/collision model with the visual model —— models.pb 权威原点装配
        // （对齐 BlitzKit SpacedArmorScene 的 hullOrigin/turretOrigin/gunOrigin 分组）。
        //
        // collision.glb 的装甲板顶点为 origin 相对坐标：
        //   hull   = 顶点 + trackOrigin
        //   turret = 顶点 + trackOrigin + turretOrigin
        //   gun    = 顶点 + trackOrigin + turretOrigin + gunOrigin(每炮塔)
        // BlitzKit 用 correctZY: DAVA(x,y,z) → GLB(x,z,y) 换算后分组装配（载荷已换算）。
        // 实测验证：Jg.Pz. E 100 的 gun 板 raw z -0.47..0.58 +(0.317,2.375) → 1.9..2.95
        // = 战斗室炮位高度 ✓；T-34 turret 板 +0.791 → 0.82..1.39 ✓。
        // （旧"整体刚性桥接 + 包围盒中心匹配"退化方案已移除——models.pb 原点 723/723
        //   全覆盖，回退路径不可达；历史方案见 backups/ 与 BlitzKit对照审查.md。）
        function alignArmorModules() {
            if (!armorModel || !tankModel) return;
            // 强制回到基准位置(tankModel 变换)：本函数可能被 syncTransforms 或 applyConfig
            // 等多条路径调用，先归零才能保证 installPivot 幂等不漂移。
            armorModel.position.copy(tankModel.position);
            armorModel.updateMatrixWorld(true);
            tankModel.updateMatrixWorld(true);

            const installPivot = (mesh, pivot) => {
                if (!mesh.userData.origPos) mesh.userData.origPos = mesh.position.clone();
                mesh.position.copy(mesh.userData.origPos).add(pivot);
                // 关键：collectArmorNodes 会把 mesh 的 matrixAutoUpdate 设为 false(炮塔旋转
                // 的矩阵冻结优化)，position 修改后 matrix 不会自动刷新——必须显式 updateMatrix，
                // 否则旋转读到旧矩阵位置，armor 整体被抬高/错位。
                mesh.updateMatrix();
            };

            const mo = tankData && tankData.model_origins;
            const cfgO = currentConfig();
            if (!(mo && mo.track && mo.turret)) return;
            const vTrack = new THREE.Vector3(mo.track[0], mo.track[1], mo.track[2]);
            const vTurret = new THREE.Vector3(mo.turret[0], mo.turret[1], mo.turret[2]);
            const pHull = vTrack.clone();
            const pTurret = vTrack.clone().add(vTurret);
            const pGun = (cfgO && cfgO.gun_origin)
                ? pTurret.clone().add(new THREE.Vector3(cfgO.gun_origin[0], cfgO.gun_origin[1], cfgO.gun_origin[2]))
                : pTurret.clone();
            armorPivotTurret = pTurret.clone();
            armorPivotGun = pGun.clone();
            armorModel.traverse(n => {
                if (!n.isMesh) return;
                const nm = n.name || '';
                if (/^turret_\d+_armor/.test(nm)) { installPivot(n, pTurret); }
                else if (/^gun_\d+_armor/.test(nm)) { installPivot(n, pGun); }
                else if (/^hull_armor/.test(nm)) { installPivot(n, pHull); }
            });
            armorModel.updateMatrixWorld(true);

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
                // 重算装甲网格法线：碰撞壳的原始顶点法线不可靠（局部插值法线与几何面法线
                // 不一致甚至反向），导致热力图把大角度等效厚误算成小角度（炮根绿色误判）。
                // 转非索引几何后按面重算 → 每个碎片的插值法线 = 精确面法线，
                // 与弹道 raycast 的 face.normal 完全一致（着色器内 abs() 抵消绕向差异）。
                // 仅装甲模型处理；视觉模型法线保持原样用于 PBR 外观。
                armorModel.traverse(function(node) {
                    if (node.isMesh && node.geometry) {
                        if (node.geometry.index) node.geometry = node.geometry.toNonIndexed();
                        node.geometry.computeVertexNormals();
                    }
                });
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
                applyUrlOptionsOnce();
            }, undefined, fail('armor model'));

            // Load visual model (visible)
            loader.load(tankData.visual_model_url, function(gltf) {
                tankModel = gltf.scene;
                // 车体中心（glb 坐标系）：collision.hull_bbox 的中心
                let bodyCenter = null;
                const col = tankData.collision || null;
                if (col && col.hull_bbox) {
                    const hb = col.hull_bbox;
                    bodyCenter = {
                        x: (hb.min[0] + hb.max[0]) / 2,
                        y: (hb.min[1] + hb.max[1]) / 2,
                        z: (hb.min[2] + hb.max[2]) / 2,
                    };
                }
                applyModelTransforms(tankModel, bodyCenter);
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
                applyUrlOptionsOnce();
                window.__DBG__ = { scene, tankModel, armorModel, tankData, THREE, controls };
            }, undefined, fail('tank model'));
        }

        // ---------- URL 参数自动化（供 Agent 无头截图 / 可分享的热力图链接） ----------
        // 支持：heatmap=1（自动开启热力图）、clean=1（隐藏全部 UI 覆盖层）、
        //       shell=N（弹种索引）、yaw=N / pitch=N（炮塔/炮管角度）、
        //       az=N / dist=N / h=N（相机方位角/距离/高度）。
        // 两个模型都就绪后应用一次（热力图依赖 armorModel）。
        const QP = new URLSearchParams(location.search);
        let urlApplied = false;
        function applyUrlOptionsOnce() {
            if (urlApplied || !tankModel || !armorModel) return;
            urlApplied = true;
            const num = (k) => { const v = parseFloat(QP.get(k)); return isNaN(v) ? null : v; };
            const yaw = num('yaw'), pitch = num('pitch');
            if (yaw !== null || pitch !== null) {
                if (yaw !== null) currentTurretDeg = Math.max(-179, Math.min(179, yaw));
                if (pitch !== null) currentGunDeg = pitch;
                document.getElementById('turret-val').textContent = currentTurretDeg.toFixed(0) + '°';
                document.getElementById('gun-val').textContent = currentGunDeg.toFixed(0) + '°';
                updateTurretGun(currentTurretDeg, currentGunDeg);
            }
            // 射手坦克（射击复现时射手 ≠ 目标）：加载射手的弹种选择器
            const shooterTank = parseInt(QP.get('shooter'), 10);
            if (!isNaN(shooterTank) && shooterTank > 0) {
                loadShooter(shooterTank);
            }
            const si = parseInt(QP.get('shell'), 10);
            if (!isNaN(si) && shooterShells && si >= 0 && si < shooterShells.length) {
                document.getElementById('shell-select').value = String(si);
                selectedShell = shooterShells[si];
                if (penetrationMode) updatePenetrationUniforms(selectedShell);
            }
            const view = QP.get('view');
            const azOv = num('az'), distOv = num('dist'), hOv = num('h');
            if (view || azOv !== null || hOv !== null || distOv !== null) {
                // 炮线高度（世界单位）：gun 枢轴的 glb z × 模型缩放
                const gunLine = (armorPivotGun ? armorPivotGun.z : 2.0) * (tankModel.scale.x || 1);
                // 视角预设（对齐 BlitzKit 语义）：front/rear/left/right = 炮线高度的水平视角；
                // hull_down = 低机位仰视炮塔（卖头视角）；top = 俯视
                const P = ({
                    front:       {az:0,   h:gunLine,     ty:gunLine,      d:4.8},
                    rear:        {az:180, h:gunLine,     ty:gunLine,      d:4.8},
                    left:        {az:90,  h:gunLine,     ty:gunLine,      d:4.8},
                    right:       {az:270, h:gunLine,     ty:gunLine,      d:4.8},
                    hull_down:   {az:0,   h:1.1,         ty:gunLine+0.3,  d:5.0},
                    top:         {az:0,   h:gunLine+12,  ty:0.6,          d:7.0},
                    front_left:  {az:45,  h:gunLine,     ty:gunLine,      d:5.2},
                    front_right: {az:315, h:gunLine,     ty:gunLine,      d:5.2},
                    rear_left:   {az:135, h:gunLine,     ty:gunLine,      d:5.2},
                    rear_right:  {az:225, h:gunLine,     ty:gunLine,      d:5.2},
                })[view] || {az:0, h:gunLine, ty:gunLine, d:4.8};
                const d = distOv !== null ? distOv : P.d;
                const a = (azOv !== null ? azOv : P.az) * Math.PI / 180;
                const h = hOv !== null ? hOv : P.h;
                const ty = num('ty') !== null ? num('ty') : P.ty;
                camera.position.set(Math.sin(a) * d, h, -Math.cos(a) * d);
                controls.target.set(0, ty, 0);
                controls.update();
            }
            if (QP.get('heatmap') === '1' && !penetrationMode) {
                penetrationMode = true;
                const btn = document.getElementById('penetration-btn');
                btn.classList.add('active'); btn.textContent = '关闭热力图';
                applyPenetrationMode(true);
            }
            // 射击复现：shot=N 时从 /api/replay_shot 取该发数据，相机摆到射手 POV，
            // 热力图就绪后自动执行该发的射线判定（trajectoria + 结果面板）。
            // 弹种选择器保留——切换弹种后自动重跑判定。
            const shotNo = parseInt(QP.get('shot'), 10);
            const isShotReplay = !isNaN(shotNo);
            if (isShotReplay) {
                fetch('/api/replay_shot').then(r => {
                    if (!r.ok) { console.warn('[shot-replay] fetch failed:', r.status); throw new Error('HTTP '+r.status); }
                    return r.json();
                }).then(d => {
                    const shots = d.shots || d;
                    const s = (Array.isArray(shots) ? shots : []).find(x => x.index === shotNo);
                    if (!s) { console.warn('[shot-replay] shot', shotNo, 'not found'); return; }
                    const sp = s.shooter_pos, tp = s.target_pos;
                    const scl = tankModel.scale.x || 1;
                    const dxE = sp[0] - tp[0], dzN = sp[2] - tp[2];
                    // 相机距离受 OrbitControls.maxDistance(30) 限制
                    const distH = Math.min(Math.sqrt(dxE*dxE + dzN*dzN) * scl, (controls.maxDistance || 30) - 1);
                    const gunLine = (armorPivotGun ? armorPivotGun.z : 2.0) * scl;
                    // ===== 模型保持默认朝向（车头 -Z），相机做相对调整 =====
                    // 数学等价于"模型旋转 hullYaw + 相机绝对方位"，但改为：
                    // 模型不动 → 相机相对方位 = (绝对方位 − hullYaw)，炮塔相对角同理。
                    // 轴映射（弹道方向验证 9/10 ≤1.1°）：回放 x=东,y=高,z=南 全直通。
                    const ta = s.target_ang || [0, 0, 0];
                    const hullYaw = ta[0] || 0;
                    // 目标→射手绝对方位（atan2 北=0 顺时针）→ 减 hullYaw 转为相对车头
                    const absBearingToShooter = Math.atan2(dxE, dzN);
                    const relBearing = absBearingToShooter - hullYaw;
                    // 相机高度：目标炮线 + 俯仰角×压缩后距离。
                    // 高度差不能直接用 dy×scl——水平距离被 clamp 到 maxDistance 时，
                    // 俯仰角会被放大（真实 -8° 变 -14°，相机陷入地下）。
                    // 正确做法：按真实几何算俯仰角，再乘压缩后的 distH。
                    const realDist = Math.sqrt(dxE*dxE + dzN*dzN) * scl;
                    const dipAngle = Math.atan2(sp[1] - tp[1], Math.sqrt(dxE*dxE + dzN*dzN) || 1);
                    const h = gunLine + Math.tan(dipAngle) * distH;
                    camera.position.set(Math.sin(relBearing) * distH, h, -Math.cos(relBearing) * distH);
                    controls.target.set(0, gunLine, 0);
                    controls.update();
                    __shotRayOrigin = new THREE.Vector3(Math.sin(relBearing) * distH, h, -Math.cos(relBearing) * distH);
                    // ===== 弹道：ball_a(射手@开火) → ball_b(弹着点)，回放系全直通映射 =====
                    // aimDir = 弹道方向（viewer 系单位向量）；aimTarget = 弹着点相对车体中心的偏移
                    const ba = s.ball_a, bb = s.ball_b;
                    let aimDir = null;
                    if (ba && bb && (ba[0] || ba[1] || ba[2]) && (bb[0] || bb[1] || bb[2])) {
                        const dvx = bb[0] - ba[0], dvy = bb[1] - ba[1], dvz = bb[2] - ba[2];
                        const n = Math.sqrt(dvx*dvx + dvy*dvy + dvz*dvz);
                        if (n > 1.0) aimDir = { x: dvx/n, y: dvy/n, z: dvz/n };
                    }
                    const ap2 = s.aim_point || [0, 0, 0];
                    const aimTarget = {
                        x: ap2[0] * scl, y: ap2[1] * scl, z: ap2[2] * scl
                    };
                    // ===== 炮塔：绝对朝向 → 相对车体（模型未旋转 → 相对 = 绝对 − hullYaw） =====
                    // ===== 炮塔：绝对朝向 → 相对车体（模型未旋转 → 相对 = 绝对 − hullYaw） =====
                    const turretAbs = (typeof s.target_turret_yaw === 'number') ? s.target_turret_yaw : 0;
                    let turretDeg = -(turretAbs - hullYaw) * 180 / Math.PI;
                    turretDeg = ((turretDeg + 180) % 360 + 360) % 360 - 180;
                    currentTurretDeg = Math.max(-179, Math.min(179, turretDeg));
                    // 炮管俯仰：type=32 pitch（回退 hull pitch）
                    const gp = (typeof s.target_gun_pitch === 'number' && s.target_gun_pitch !== 0)
                        ? s.target_gun_pitch : (ta[1] || 0);
                    currentGunDeg = Math.max(-25, Math.min(15, gp * 180 / Math.PI));
                    updateTurretGun(currentTurretDeg, currentGunDeg);
                    console.log('[shot-replay] camera=', camera.position.toArray().map(v=>v.toFixed(2)),
                        'absBearing=', (absBearingToShooter*180/Math.PI).toFixed(1),
                        'relBearing=', (relBearing*180/Math.PI).toFixed(1),
                        'turret=', currentTurretDeg.toFixed(1), 'gunPitch=', currentGunDeg.toFixed(1));
                    const st = document.getElementById('turret-controls');
                    if (st) { st.innerHTML = '<div class="ctrl-row"><b>Shot #' + s.index + '</b></div>' +
                        '<div class="ctrl-row">DMG ' + s.damage + (s.is_kill ? ' · KILL' : '') + ' · ' + (s.target_name||'') + '</div>'; }
                    // 弹道两点 → 射线方向（标记球 + doPenetrationCheck 共用）
                    __shotRayOrigin = new THREE.Vector3(Math.sin(bearing) * distH, h, -Math.cos(bearing) * distH);
                    // 射线终点：弹道方向可用 → 沿方向穿过模型（raycast 截断取真实命中）；
                    // 否则用弹着点坐标（<6m 过滤后）。
                    // 射线终点：aimDir 可用 → 沿弹道方向穿过模型（raycast 截断取命中）；
                    // 否则直接用弹着点偏移（无保守 clamp，按原始计算值渲染）
                    const targetPoint = (aimDir)
                        ? __shotRayOrigin.clone()
                            .add(new THREE.Vector3(aimDir.x, aimDir.y, aimDir.z).multiplyScalar(distH * 3))
                        : new THREE.Vector3(aimTarget.x, aimTarget.y, aimTarget.z);
                    // 射线终点延伸到模型对面（沿 origin→target 方向加倍距离）
                    const rayDir = targetPoint.clone().sub(__shotRayOrigin);
                    __shotRayTarget = __shotRayOrigin.clone().add(rayDir.multiplyScalar(2));
                    // 命中点标记在 doPenetrationCheck 成功后渲染（位置 = raycast 实际命中点）

                    setTimeout(() => {
                        doPenetrationCheck(0, 0);
                        __shotRayOrigin = null; __shotRayTarget = null;
                    }, 600);
                }).catch(e => console.warn('replay_shot fetch failed', e));
            }
            if (QP.get('clean') === '1' && !isShotReplay) {
                // 仅在非复现模式的纯净截图中隐藏 UI；射击复现模式保留完整查看器窗口
                ['info-panel','shell-selector','view-toggle','tank-selectors','turret-controls','controls-hint','loading'].forEach(id => {
                    const el = document.getElementById(id); if (el) el.style.display = 'none';
                });
            }
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
            // 默认配置对齐 BlitzKit tankToDuelMember 的 turrets.at(-1).guns.at(-1)
            // （最后一座炮塔的最后一门炮 = 顶层配置）——顶层炮的穿深/转正等与底层不同，
            // 热力图着色（如临界等效厚度的颜色）因此以顶层配置为准。
            const q = new URLSearchParams(location.search);
            const wantCfg = parseInt(q.get('config'), 10);
            const defaultCfg = Math.max(0, (tankData.configs ? tankData.configs.length : 1) - 1);
            currentConfigIdx = (Number.isInteger(wantCfg) && wantCfg >= 0 && wantCfg < tankData.configs.length) ? wantCfg : defaultCfg;
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
            // spaced 分类跟随当前配置（turret_spaced/gun_spaced 随炮塔/主炮变化）
            retagSpacedSections();
            // Align armor module (turret/gun) with the visual model (fixes missing points).
            alignArmorModules();
            // Reset turret/gun matrices for the newly active nodes.
            origMatrices = null;
            armorOrigMatrices = null;
            collectTurretGunNodes();
            collectArmorNodes();
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

            // 热力图开启时按新配置重建 RT/着色场景（对齐 BlitzKit：两场景随配置响应式重建）。
            // 放在弹种更新之后，保证重建时 selectedShell 已是新配置的弹。
            if (penetrationMode && armorModel) rebuildHeatmapScenes();
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
                opt.textContent = shellOptionText(s);
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
            // 材质级裁剪面（炮管外部模块的 mask 平面，对齐 BlitzKit clippingPlanes）
            renderer.localClippingEnabled = true;
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
                    // 热力图模式下切弹需刷新 uniforms / 精度
                    if (penetrationMode) updatePenetrationUniforms(selectedShell);
                    // 射击复现模式：切弹后重跑判定（不同弹种穿深/类型不同）
                    if (QP.get('shot')) doPenetrationCheck(0, 0);
                }
            });

            // 装备修正开关：影响点击判定请求与热力图 uniforms（对齐 BlitzKit 装备系统）
            const onEquipmentChange = function() {
                // 弹种选项文本联动（显示穿深随 Calib.Shells 变化）
                const sel = document.getElementById('shell-select');
                for (let i = 0; i < sel.options.length; i++) {
                    if (shooterShells && i < shooterShells.length) {
                        sel.options[i].textContent = shellOptionText(shooterShells[i]);
                    }
                }
                if (!penetrationMode) return;
                const sh = selectedShell || (shooterShells[0]) || null;
                if (sh) { updatePenetrationUniforms(sh); updateSpacedUniforms(sh); }
                updateHeatmapThickness();
            };
            document.getElementById('eq-calibrated').addEventListener('change', onEquipmentChange);
            document.getElementById('eq-enhanced').addEventListener('change', onEquipmentChange);

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

            // ---------- 实时穿透热力图模式（移植 BlitzKit 着色） ----------
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
                const dx = e.clientX - rmbStartX;
                const dy = e.clientY - rmbStartY;
                // 对齐 BlitzKit applyPitchYawLimits：先钳制炮塔水平射界（yaw_limits，注意
                // BlitzKit 对 pb 值取负：clamp 到 [−max, −min]），再按炮塔朝向解析俯仰限制
                // （front/back 极值 + transition 过渡插值，默认 20°）。
                const norm180 = (a) => ((a + 180) % 360 + 360) % 360 - 180;
                // yaw_limits/pitch_limits 都是每配置字段（configs[]），必须从 currentConfig() 取——
                // 读 tankData 层会恒为 undefined，导致限位射界车辆错误地获得全向旋转
                const yl = currentConfig()?.yaw_limits;
                const pl = currentConfig()?.pitch_limits;
                let yawDeg = rmbStartTurret + dx * 0.5;
                if (yl) {
                    // 全周射界（跨度 ≥360°）：不钳制，允许转整圈（俯仰解析内部自带归一化）
                    if (yl.max - yl.min < 360) {
                        yawDeg = norm180(Math.max(-yl.max, Math.min(-yl.min, yawDeg)));
                    }
                } else {
                    const tLeft = tankData.turret_traverse_left ?? 180;
                    const tRight = tankData.turret_traverse_right ?? 180;
                    // 左右各 180° = 全周：自由旋转（不钳制在 ±180 卡住）；角度自由累加，
                    // 俯仰 front/back 解析内部自带归一化，不受累计值影响
                    if (!(tLeft >= 180 && tRight >= 180)) {
                        yawDeg = Math.max(-tLeft, Math.min(tRight, yawDeg));
                    }
                }
                let pitchDeg = rmbStartGun - dy * 0.5;
                // pitch_limits 为 proto required 字段（723/723 覆盖）——旧单一 depression/
                // elevation 回退分支已移除
                let lower = -pl.max, upper = -pl.min;
                const transition = pl.transition || 20;
                if (pl.back) {
                    const yawRotatedAbs = Math.abs(norm180(yawDeg - 180));
                    if (yawRotatedAbs <= pl.back.range / 2 + transition) {
                        if (yawRotatedAbs <= pl.back.range / 2) {
                            lower = -pl.back.max; upper = -pl.back.min;
                        } else {
                            const tp = (yawRotatedAbs - pl.back.range / 2) / transition;
                            lower = -((1 - tp) * pl.back.max + tp * pl.max);
                            upper = -((1 - tp) * pl.back.min + tp * pl.min);
                        }
                    }
                }
                if (pl.front) {
                    const yawAbs = Math.abs(norm180(yawDeg));
                    if (yawAbs <= pl.front.range / 2 + transition) {
                        if (yawAbs <= pl.front.range / 2) {
                            lower = -pl.front.max; upper = -pl.front.min;
                        } else {
                            const tp = (yawAbs - pl.front.range / 2) / transition;
                            lower = -((1 - tp) * pl.front.max + tp * pl.max);
                            upper = -((1 - tp) * pl.front.min + tp * pl.min);
                        }
                    }
                }
                pitchDeg = Math.max(lower, Math.min(upper, pitchDeg));
                currentTurretDeg = yawDeg;
                currentGunDeg = pitchDeg;
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

        // Collect gun / turret configuration node groups from the model.
        // A gun config is identified by the "gun_0X" prefix shared by all its variant nodes
        // (gun_0X, gun_0X_mask, gun_0X_mask_cap, gun_0X_nc, ...), so rotating the gun moves the
        // barrel AND the gun mask (gun root) together.
        // 收集方式对齐 BlitzKit：按节点名前缀在**整棵树**上筛选（其 Object.values(gltf.nodes)
        // + isCurrentGun/isCurrentTurret 前缀过滤）——不依赖单一根组。model.glb 存在两种布局：
        // 单根（如 T28 Defender：根组包裹全部部件）与多根（如 Kranvagn：部件直接平铺为
        // 场景子节点）；旧实现取 model.children[0] 当唯一根，多根场景会收集为空，
        // 导致炮塔/炮管不跟随旋转、未选配置部件（多余件）保持可见。
        function collectConfigNodes(model) {
            configGunGroups = [];
            configTurretNodes = [];
            if (!model) return;
            const byGroup = new Map(); // groupNum -> nodes[]
            const turrets = [];
            model.traverse(function(n) {
                const nm = n.name || '';
                const gm = nm.match(/^gun_(\d+)/);
                const tm = nm.match(/^turret_(\d+)$/);
                if (gm) {
                    const g = parseInt(gm[1], 10);
                    if (!byGroup.has(g)) byGroup.set(g, []);
                    byGroup.get(g).push(n);
                } else if (tm) {
                    turrets.push(n);
                }
            });
            // Sort groups by number; each group sorted by name.
            const keys = Array.from(byGroup.keys()).sort((a, b) => a - b);
            for (const k of keys) {
                const arr = byGroup.get(k).sort((a, b) => ((a.name||'') < (b.name||'') ? -1 : 1));
                configGunGroups.push(arr);
            }
            configTurretNodes = turrets.sort((a, b) => (a.name.match(/\d+/)?.[0]|0) - (b.name.match(/\d+/)?.[0]|0));
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
            if (!tankModel || !configTurretNodes.length) return;
            const cfg = currentConfig();
            // Active gun = gun node matching config's gun_index; turret = config's turret_index
            // （configTurretNodes/configGunGroups 由 collectConfigNodes 全树收集）。
            turretNode = configTurretNodes[cfg.turret_index % configTurretNodes.length] || null;
            if (configGunGroups.length) {
                const grp = configGunGroups[cfg.gun_index % configGunGroups.length];
                gunNodesList = grp || [];
            } else {
                gunNodesList = [];
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

        function updateTurretGun(turretDeg, gunDeg) {
            if (!tankModel) return;
            if (!origMatrices) collectTurretGunNodes();
            if (!origMatrices) return;

            // 旋转枢轴（对齐 BlitzKit——其旋转枢轴只取 models.pb 原点，
            // 由 alignArmorModules 写入：track+turret / track+turret+gun 原点）
            const tPivot = armorPivotTurret ? armorPivotTurret.clone() : new THREE.Vector3(0, 0, 1.7);
            const gPivot = armorPivotGun ? armorPivotGun.clone() : new THREE.Vector3(0, 0, 2.0);

            const tr = THREE.MathUtils.degToRad(turretDeg);
            const gr = THREE.MathUtils.degToRad(gunDeg);

            // 炮塔旋转矩阵：translate(P_t) * rotateZ(angle) * translate(-P_t)
            // 初始炮塔姿态（models.pb initial_turret_rotation，度，取负）：对齐 BlitzKit
            // useTankTransform 的 Euler(initialPitch, initialRoll, yaw+initialYaw, XYZ) 组合
            // —— R_x(ip)·R_y(ir)·R_z(yaw+iy) = (R_x(ip)·R_y(ir)·R_z(iy))·R_z(yaw)。
            let turretRot = new THREE.Matrix4().makeRotationZ(tr);
            const itr = tankData.initial_turret_rotation;
            if (itr) {
                turretRot = new THREE.Matrix4().makeRotationFromEuler(new THREE.Euler(
                    -THREE.MathUtils.degToRad(itr.pitch),
                    -THREE.MathUtils.degToRad(itr.roll),
                    tr - THREE.MathUtils.degToRad(itr.yaw),
                    'XYZ'));
            }
            const mTurret = new THREE.Matrix4();
            mTurret.makeTranslation(tPivot.x, tPivot.y, tPivot.z);
            mTurret.multiply(turretRot);
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
            // NDC 坐标由 event 计算
            const rect = renderer.domElement.getBoundingClientRect();
            const ndcX = ((event.clientX - rect.left) / rect.width) * 2 - 1;
            const ndcY = -((event.clientY - rect.top) / rect.height) * 2 + 1;
            doPenetrationCheck(ndcX, ndcY);
        }

        /// 射击判定核心（从 NDC 坐标发射射线）：onClick 与射击复现共用。
        let __shotRayOrigin = null;   // 射击复现：射线起点（射手方向，固定距离）
        let __shotRayTarget = null;   // 射击复现：射线终点（瞄准点）
        function doPenetrationCheck(ndcX, ndcY) {
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
            // 对齐 BlitzKit：掩体部件(gun_0X_mask*)仅在该炮定义了 mask 时作为外部模块
            // （判定范围与视觉一致），命中点在掩体平面之后的按 discardClippingPlane 丢弃。
            const activeGun = activeGunNumber();
            const cfgForGun = currentConfig();
            const gunHasMask = cfgForGun && typeof cfgForGun.gun_mask === 'number';
            computeGunClipPlane();
            const activeModules = activeGun == null
                ? moduleMeshes   // 无法确定激活炮(如 configGunGroups 未填充)时退化为全部保留
                : moduleMeshes.filter(m => {
                    if (m.userData.gunConfig != null) return m.userData.gunConfig === activeGun;
                    return true;
                }).filter(m => !(m.userData.gunMaskPart && !gunHasMask));
            for (const mesh of activeModules) {
                if (mesh.visible === false) mesh.visible = true;
            }

            if (__shotRayOrigin && __shotRayTarget) {
                // 射击复现模式：用保存的射线参数（不依赖相机位置——
                // OrbitControls damping/maxDistance 会干扰 camera.position）
                const dir = __shotRayTarget.clone().sub(__shotRayOrigin).normalize();
                raycaster.set(__shotRayOrigin.clone(), dir);
            } else {
                mouse.x = ndcX;
                mouse.y = ndcY;
                raycaster.setFromCamera(mouse, camera);
            }
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
                // 炮管外部模块的掩体裁剪（BlitzKit discardClippingPlane）：命中点在掩体平面
                // 之后（炮根侧，钻进炮塔的部分）不算外部模块命中。
                if (hit.object.userData.armorSection === 'gunBarrel' && gunClipPlane &&
                    gunClipPlane.distanceToPoint(hit.point) < 0) continue;
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
                console.warn('[shot-replay] check: 0 armor hits, intersects=' + intersects.length);
                document.getElementById('click-info').style.display = 'none';
                document.getElementById('traj-info').style.display = 'none';
                trajInfoPos = null;
                if (trajGroup) { scene.remove(trajGroup); trajGroup = null; }
                return;
            }

            const first = armorHits[0];
            const point = first.point;
            // 命中点标记：渲染在 raycast 实际命中的装甲板表面
            if (window.__hitMarker) { scene.remove(window.__hitMarker); window.__hitMarker = null; }
            const markerGeo = new THREE.SphereGeometry(0.08, 16, 12);
            const markerMat = new THREE.MeshBasicMaterial({ color: 0xff2222, transparent: true, opacity: 0.9, depthTest: false });
            const marker = new THREE.Mesh(markerGeo, markerMat);
            marker.position.copy(point);
            marker.renderOrder = 999;
            scene.add(marker);
            window.__hitMarker = marker;
            const ringGeo = new THREE.RingGeometry(0.12, 0.18, 24);
            const ringMat = new THREE.MeshBasicMaterial({ color: 0xff2222, transparent: true, opacity: 0.7, side: THREE.DoubleSide, depthTest: false });
            const ring = new THREE.Mesh(ringGeo, ringMat);
            ring.lookAt(camera.position);
            marker.add(ring);
            window.__hitMarkerRing = ring;
            // 射击复现模式：入射方向用真实弹道（射线起点→命中点），
            // 而非 camera→point（相机被 maxDistance clamp 后方向有偏差）
            const viewDir = (__shotRayOrigin && __shotRayTarget)
                ? __shotRayOrigin.clone().sub(point).normalize()
                : camera.position.clone().sub(point).normalize();

            // 命中距离（米，对齐 BlitzKit 世界单位）：炮口世界坐标 → 命中点，× worldMetersPerUnit
            // 换算回真实米数。炮管几何不可用时回退为相机到命中点的真实米数（不再乘 10 伪系数）。
            // 注意：旧实现=相机距离×10，随缩放变化且单位失真；现缩放相机不改变判定结果。
            const dist = (gunMuzzleWorld
                ? point.distanceTo(gunMuzzleWorld)
                : point.distanceTo(camera.position)) * (worldMetersPerUnit || 1);

            // Send to Rust backend for unified penetration calculation
            const shellType = shellTypeOf(selectedShell);
            const pen = selectedShell ? (selectedShell.penetration || 0) : 0;
            const penFar = selectedShell ? (selectedShell.penetration_far || 0) : 0;
            const shellRange = selectedShell ? (selectedShell.range || 0) : 0;
            const dmg = selectedShell ? (selectedShell.damage || 0) : 0;
            const modDmg = selectedShell ? (selectedShell.module_damage || 0) : 0;
            const caliber = shooterCaliber || (shooterData && shooterData.caliber) || tankData.caliber || 120;
            const isHE = shellType === 'he';
            // 装备开关（对齐 BlitzKit 装备系统）：同时作用于点击判定与热力图 uniforms。
            // 注意：跳弹后的二次请求不能重复传 calibrated_shells（剩余穿深已含系数），
            // 但 enhanced_armor 影响装甲厚度，需要继承。
            const eqCal = !!(document.getElementById('eq-calibrated') && document.getElementById('eq-calibrated').checked);
            const eqEnh = !!(document.getElementById('eq-enhanced') && document.getElementById('eq-enhanced').checked);
            // 轨迹面板显示的穿深 = 校准后穿深（与后端判定一致）
            const penDisp = pen * shellPenMul(selectedShell);
            // 命中点坐标换算为米（对齐 BlitzKit：其世界坐标原生=米）。后端 HEAT 逐层间隙
            // 衰减与 HE 溅射距离都基于这些点计算，不换算会因模型缩放(6单位)而失真。
            const mpu = worldMetersPerUnit || 1;

            const req = {
                shell_type: shellType,
                penetration: pen,
                penetration_far: penFar > 0 ? penFar : null,
                range: shellRange > 0 ? shellRange : null,
                distance: dist,
                caliber: caliber,
                damage: dmg,
                module_damage: modDmg,
                // HE 爆炸半径：tanks.pb field 12（每弹不同）；旧数据缺失时回退 3.0
                explosion_radius: isHE ? ((selectedShell && selectedShell.explosion_radius) || 3.0) : 0,
                calibrated_shells: eqCal,
                enhanced_armor: eqEnh,
                allow_ricochet: true,
                view_dir: [viewDir.x, viewDir.y, viewDir.z],
                hits: armorHits.map(ah => ({
                    section: ah.section,
                    plate_id: ah.plateId,
                    thickness: ah.thickness,
                    normal: [ah.normal.x, ah.normal.y, ah.normal.z],
                    point: [ah.point.x * mpu, ah.point.y * mpu, ah.point.z * mpu],
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
                                enhanced_armor: eqEnh,
                                allow_ricochet: false,
                                view_dir: [reflect.x, reflect.y, reflect.z],
                                hits: ricHits.map(ah => ({ section: ah.section, plate_id: ah.plateId, thickness: ah.thickness, normal: [ah.normal.x, ah.normal.y, ah.normal.z], point: [ah.point.x * mpu, ah.point.y * mpu, ah.point.z * mpu], part_name: ah.partName })),
                            };
                            fetch('/api/penetrate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(ricReq) })
                                .then(r => r.ok ? r.json() : null).then(ricRes => {
                                    if (ricRes) {
                                        const ricLayers = ricRes.layers.map(l => ({ point: ricHits.find(ah => ah.partName === l.part_name)?.point || lastLayer.point, name: l.part_name, thickness: l.thickness, eff: l.effective, remainBefore: l.remaining_before, penetrated: l.penetrated, ricochet: l.ricochet, seg: 1 }));
                                        const combined = { result: 'RICOCHET → ' + ricRes.result, total_effective: res.total_effective, layers: [...trajLayers, ...ricLayers] };
                                        showTrajectory(point, combined.result, combined.total_effective, combined.layers, penDisp, dmg, modDmg);
                                    } else { showTrajectory(point, res.result, res.total_effective, trajLayers, penDisp, dmg, modDmg, dist); }
                                }).catch(() => { showTrajectory(point, res.result, res.total_effective, trajLayers, penDisp, dmg, modDmg, dist); });
                            return;
                        }
                    }
                }

                showTrajectory(point, res.result, res.total_effective, trajLayers, penDisp, dmg, modDmg, dist);
            }).catch(err => {
                console.error('Penetration API error:', err);
                // Fallback: show basic result without API
                showTrajectory(point, 'ERROR', 0, [], penDisp, dmg, modDmg, dist);
            });
        }

        let trajGroup = null;
        let trajInfoPos = null;
        function showTrajectory(firstPoint, result, totalEff, layers, penVal, dmgVal, modDmgVal, distVal) {
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
            const decayedPen = layers.length && layers[0].remainBefore != null ? layers[0].remainBefore : null;
            const distPart = (typeof distVal === 'number' && distVal > 0)
                ? ` · <span style="color:#9fc3e8;">Dist ${distVal.toFixed(0)}m</span>` + (decayedPen != null && Math.abs(decayedPen - penVal) > 0.5 ? ` · Pen@${distVal.toFixed(0)}m <span style="color:#FFD29B;">${decayedPen.toFixed(1)}</span>` : '')
                : '';
            html += `<div style="font-size:13px;color:#8ab;margin-bottom:8px;">Eff ${totalEff.toFixed(0)}mm · Pen ${penVal}mm${distPart} · Remain ${Math.max(0, (decayedPen??penVal) - totalEff).toFixed(0)}mm · ${layers.length} layers${dmgLine ? ' · <span style="color:#FFB74D;">' + dmgLine + '</span>' : ''}</div>`;
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
            if (penetrationMode && armorModel) {
                if (SESS && heatFrames < 5) {
                    heatFrames++;
                    if (heatFrames === 5 && !heatReadySent) {
                        heatReadySent = true;
                        fetch('/api/ready?sess=' + encodeURIComponent(SESS)).catch(()=>{});
                        // 注意：不能停止 RAF 循环——渲染停止后 WebGL 画布在无头截图
                        // 重新合成时会丢失内容（空场景）。保持渲染直到预算耗尽。
                    }
                }
                // 视角/分辨率随时可能变，热力图逐帧刷新 resolution（着色像素正确）
                refreshPenetrationResolution();
                // 对齐 BlitzKit useFrame() 序列（Armor/index.tsx）：
                //   1. autoClear=true;  setRenderTarget(rt);  render(spacedArmorScene, camera)
                //   2. autoClear=false; setRenderTarget(null); render(scene, camera)   —— 地面/背景
                //   3. clearDepth();                          render(primaryArmorScene, camera) —— 主装甲着色(读 RT)
                renderSpacedArmorPass();
                renderer.render(scene, camera);                       // 背景/网格（autoClear 已置 false）
                renderer.clearDepth();                                 // 清除深度，让 exclude 深度遮罩生效
                if (primaryArmorScene) { syncCloneMatrices(primaryArmorScene); renderer.render(primaryArmorScene, camera); }
            } else {
                renderer.render(scene, camera);
            }
            if (trajInfoPos) updateTrajInfoPos();
        }

        init();
    </script>
</body>
</html>"#;
