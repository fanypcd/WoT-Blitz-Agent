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

fn wsl_ip() -> String {
    std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().next().map(|s| s.to_string()))
        .unwrap_or_else(|| "localhost".to_string())
}

fn load_armor_model(tank_id: u32) -> Option<ArmorModel> {
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

fn find_dev_name(tank_id: u32) -> Option<String> {
    crate::wargaming::blitzkit::tank_full(tank_id).map(|t| t.dev_name)
}

static GLOBAL_RESOLVER: std::sync::OnceLock<Arc<TankResolver>> = std::sync::OnceLock::new();

pub fn global_resolver() -> Arc<TankResolver> {
    GLOBAL_RESOLVER.get_or_init(|| {
        TankResolver::load_from_json_file(&crate::data::data_path("tank_cache.json"))
            .map(Arc::new)
            .unwrap_or_default()
    }).clone()
}

pub fn set_global_resolver(resolver: TankResolver) {
    let _ = GLOBAL_RESOLVER.set(Arc::new(resolver));
}

pub async fn serve(tank_resolver: TankResolver, tank_id: u32, shooter_id: Option<u32>) -> anyhow::Result<()> {
    let app = build_viewer_router(tank_resolver, tank_id, shooter_id, "");

    let addr = SocketAddr::from(([0, 0, 0, 0], 0));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

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

pub fn viewer_index_html(tank_id: u32, shooter_id: u32, base_prefix: &str) -> String {
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
        .replace("\"/glb", &format!("\"{}/glb", base_prefix))
        .replace("\"/vendor", &format!("\"{}/vendor", base_prefix))
        .replace("\"/api/", &format!("\"{}/api/", base_prefix))
        .replace("'/api/", &format!("'{}/api/", base_prefix))
        .replace("src=\"/", &format!("src=\"{}/", base_prefix))
}

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

pub(crate) async fn ensure_glb_bytes(tank_id: u32, filename: &str) -> Result<Vec<u8>, String> {
    if !GLB_FILES.contains(&filename) {
        return Err(format!("invalid GLB filename: {}", filename));
    }
    let cache_dir = Path::new(GLB_CACHE_DIR).join(tank_id.to_string());
    let cache_path = cache_dir.join(filename);
    if let Ok(bytes) = std::fs::read(&cache_path) {
        return Ok(bytes);
    }

    let url = format!("https://api.blitzkit.app/tanks/{}/{}", tank_id, filename);
    eprintln!("[glb-cache] downloading {} ...", url);
    // Client 构建一次复用（内部含连接池），重试循环内不再 clone/重建；
    // 构建失败时退化为默认 Client，重试语义不变
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => reqwest::Client::new(),
    };
    let mut last_err;   // 循环内每个分支都会先赋值
    for _attempt in 0..3 {
        match client.get(&url).send().await {
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
                    Err(e) => last_err = format!("read body failed: {}", e),
                }
            }
            Ok(resp) => last_err = format!("BlitzKit CDN returned {}", resp.status()),
            Err(e) => last_err = format!("{}", e),
        }
        eprintln!("[glb-cache] attempt failed, retrying... ({last_err})");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // reqwest 全部重试失败 → 系统 curl 回退。实测（GFW 环境）rustls 指纹的大文件
    // 流会被中途重置（reqwest 报 read body failed: error decoding response body），
    // 而 curl（不同 TLS 栈）可完整拉取同一资源。
    eprintln!("[glb-cache] reqwest 失败，尝试系统 curl 回退...");
    let tmp_path = cache_path.with_extension("download");
    let out = std::process::Command::new("curl")
        .args([
            "-sfL", "--max-time", "180",
            "-o", tmp_path.to_string_lossy().as_ref(),
            &url,
        ])
        .output();
    match out {
        Ok(o) if o.status.success() && tmp_path.exists() => {
            match std::fs::read(&tmp_path) {
                Ok(bytes) if !bytes.is_empty() => {
                    let _ = std::fs::create_dir_all(&cache_dir);
                    let _ = std::fs::write(&cache_path, &bytes);
                    let _ = std::fs::remove_file(&tmp_path);
                    eprintln!("[glb-cache] curl 回退成功，已入缓存 {} ({} bytes)", cache_path.display(), bytes.len());
                    return Ok(bytes);
                }
                Ok(_) => last_err = "curl 回退：响应体为空".to_string(),
                Err(e) => last_err = format!("curl 回退：读取失败 {}", e),
            }
        }
        Ok(o) => {
            last_err = format!(
                "curl 回退失败（exit {:?}: {}）",
                o.status.code(),
                String::from_utf8_lossy(&o.stderr).chars().take(200).collect::<String>()
            );
        }
        Err(e) => last_err = format!("curl 回退不可用: {}", e),
    }
    let _ = std::fs::remove_file(&tmp_path);
    Err(format!("BlitzKit CDN unreachable: {last_err} (model not in glb_cache/)"))
}

pub async fn start_viewer_server(tank_resolver: TankResolver, tank_id: u32, shooter_id: u32) -> anyhow::Result<u16> {
    start_viewer_server_with_data(tank_resolver, tank_id, Some(shooter_id), None).await
}

/// 为回放射击复现启动无头查看器：解析回放 → 挂载 /api/replay_shot → 返回
/// (端口, 所选射击的弹药槽位)。shell_slot 供调用方拼接 `&shell=N` URL 参数
/// （缺省会回落到查看器默认槽 0，非 0 槽弹种渲染错误）。
pub async fn start_viewer_server_for_replay(
    replay_path: &std::path::Path,
    tank_resolver: TankResolver,
    shot_no: usize,
) -> anyhow::Result<(u16, u32, Option<usize>)> {
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
    // 先把带 DamageCounter 事件的 entity_id 收进 HashSet，避免对每个候选实体
    // 重新全量扫描 events（O(N×M) → O(N+M)）
    let dmg_counter_eids: std::collections::HashSet<u32> = timeline.events.iter()
        .filter(|e| matches!(e.event_type, crate::replay::combat::CombatEventType::DamageCounter { .. }))
        .map(|e| e.entity_id)
        .collect();
    let author_eid = *timeline.entity_names.iter()
        .find(|(eid, _)| dmg_counter_eids.contains(eid))
        .map(|(eid, _)| eid)
        .unwrap_or(&0);
    let shots = timeline.infer_shots(author_eid);
    if shots.is_empty() {
        return Err(anyhow::anyhow!("No shot events detected in this replay."));
    }
    // 作者昵称 = battle_results 权威来源（meta.json 非 UTF-8 时 read_meta 整体失败，不可依赖；
    // meta.player_name 仅作兜底）
    let br = replay.read_battle_results().ok();
    let author_nickname = br.as_ref()
        .map(|br| crate::replay::combat::author_nick_from_battle_results(br))
        .or_else(|| meta.as_ref().map(|m| m.player_name.clone()))
        .unwrap_or_default();
    let author_player_eid = crate::replay::combat::resolve_author_player_eid_by_nick(&raw_packets, &author_nickname);
    // 双方炮管俯仰的车型极限锚定表（昵称→俯角/仰角）——prop2 frac 比例解码用
    let pitch_limits = br.as_ref()
        .map(|br| tank_resolver.pitch_limits_from_battle_results(br))
        .unwrap_or_default();
    let tank_of = |nick: &str| -> Option<u32> {
        let br = br.as_ref()?;
        br.players.iter().find(|p| p.info.nickname == nick)
            .and_then(|p| br.player_results.iter().find(|pr| pr.info.account_id == p.account_id))
            .map(|pr| pr.info.tank_id)
    };
    let mut replay_data = crate::replay::combat::extract_shot_replays_with_limits(&raw_packets, author_player_eid, &pitch_limits)?;
    // 弹种回填：全局 shell_id → tanks.pb 原始弹种串（/api/replay_shot 透传给 3D 视图）
    crate::replay::loadout::ShellKindTable::from_tanks_pb().annotate(&mut replay_data);
    if shot_no == 0 || shot_no > replay_data.len() {
        return Err(anyhow::anyhow!("shot {} out of range (1..={})", shot_no, replay_data.len()));
    }
    let shot = &replay_data[shot_no - 1];
    let target_tank = tank_of(&shot.target_name);
    let author_nickname = meta.as_ref().map(|m| m.player_name.clone()).unwrap_or_default();
    let shooter_tank = tank_of(&author_nickname)
        .or_else(|| meta.as_ref().map(|m| m.tank_id as u32).filter(|v| *v > 0));
    eprintln!("[replay_shot] shot={}_{} target_name={} target_tank={:?} shooter_tank={:?} target_ang={:?}",
        shot_no, shot.damage, shot.target_name, target_tank, shooter_tank, shot.target_ang);

    let viewed_tank = target_tank.or(shooter_tank).unwrap_or(0);
    let shell_slot = shot.shell_slot;
    // 发射弹种的权威标识 = shell_id（type=28 槽位快照存在切弹竞态）；
    // URL 的 shell 参数按「弹种在射手弹表中的下标」传递
    let shell_for_url = shooter_tank
        .and_then(|st| shell_index_by_global_id(st, shot.shell_id))
        .map(|i| i as u32)
        .unwrap_or(shell_slot);
    // 实际搭载配置下标（目标/射手）：comp blob → 发射弹种 → 初始血量 证据链，注入每发数据
    let valid_tanks: Vec<u32> = br.as_ref().map(|br| br.player_results.iter()
        .map(|pr| pr.info.tank_id).collect()).unwrap_or_default();
    let comps = crate::replay::playback::collect_comp_descriptors(&raw_packets, &valid_tanks);
    let initial_hp_all = crate::replay::combat::collect_initial_hp(&raw_packets);
    let mut player_shells: std::collections::HashMap<String, Vec<u32>> = std::collections::HashMap::new();
    for s in &replay_data {
        if s.shell_id == 0 { continue; }
        let v = player_shells.entry(s.shooter_name.clone()).or_default();
        if !v.contains(&s.shell_id) { v.push(s.shell_id); }
    }
    let mut nick_hp: std::collections::HashMap<String, u16> = std::collections::HashMap::new();
    for (eid, nick) in &timeline.entity_names {
        if let Some((_, hp)) = initial_hp_all.get(eid) { nick_hp.insert(nick.clone(), *hp); }
    }
    let cfg_of = |nick: &str, tank: u32| -> Option<usize> {
        if tank == 0 { return None; }
        let comp = comps.get(nick).and_then(|c| {
            ((c.tank_id & 0xFFFF) == (tank & 0xFFFF)).then_some((c.turret_local, c.gun_local))
        });
        let shells = player_shells.get(nick).map(|v| v.as_slice()).unwrap_or(&[]);
        let hp = nick_hp.get(nick).copied().unwrap_or(0);
        resolve_config_index(tank, comp, shells, hp).map(|(idx, _, _)| idx)
    };
    let viewed_cfg = if let Some(tt) = target_tank {
        cfg_of(&shot.target_name, tt)
    } else {
        shooter_tank.and_then(|st| cfg_of(&author_nickname, st))
    };
    let mut replay_json = serde_json::to_value(&replay_data)?;
    if let Some(arr) = replay_json.as_array_mut() {
        for s in arr.iter_mut() {
            let shooter_name = s["shooter_name"].as_str().unwrap_or("").to_string();
            let target_name = s["target_name"].as_str().unwrap_or("").to_string();
            if let Some(idx) = tank_of(&shooter_name).and_then(|t| cfg_of(&shooter_name, t)) {
                s["shooter_config_idx"] = json!(idx);
            }
            if let Some(idx) = tank_of(&target_name).and_then(|t| cfg_of(&target_name, t)) {
                s["target_config_idx"] = json!(idx);
            }
            let shell_id = s["shell_id"].as_u64().unwrap_or(0) as u32;
            if let Some(st) = tank_of(&shooter_name) {
                if shell_id != 0 {
                    if let Some(si) = shell_index_by_global_id(st, shell_id) {
                        s["shooter_shell_idx"] = json!(si);
                    }
                }
            }
        }
    }
    eprintln!("[replay_shot] 配置下标注入完成（shot={}，viewed_cfg={:?}）", shot_no, viewed_cfg);
    let port = start_viewer_server_with_data(tank_resolver, viewed_tank, shooter_tank, Some(replay_json)).await?;
    Ok((port, shell_for_url, viewed_cfg))
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

pub(crate) async fn penetrate_handler(Json(req): Json<PenetrationRequest>) -> Json<Value> {
    let result = penetration::calculate(&req);
    Json(serde_json::to_value(result).unwrap_or(json!(null)))
}

pub(crate) async fn tank_data_handler(
    axum::extract::Path(tank_id): axum::extract::Path<u32>,
) -> Json<Value> {
    Json(tank_data_value(tank_id))
}

pub(crate) fn tank_data_value(tank_id: u32) -> Value {
    tank_data_value_prefixed(tank_id, "")
}

pub(crate) fn tank_data_value_prefixed(tank_id: u32, base_prefix: &str) -> Value {
    let resolver = global_resolver();

    let info = resolver.resolve_info(tank_id);

    let armor_plates = std::fs::read_to_string(crate::data::data_path("armor_cache.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get(tank_id.to_string()).cloned());

    let game_data = crate::wargaming::game_extract::load_game_data(tank_id, &crate::data::data_dir().join("game_data"));
    let armor_model = match &game_data {
        Some(gd) => gd.armor_model.clone(),
        None => load_armor_model(tank_id),
    };
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

    let armor_model_val = armor_model.as_ref().map(|m| {
        let mut v = serde_json::to_value(m).unwrap_or(json!(null));
        // f32 序列化尾差清洗：62.400001525878906 → 62.4（装甲厚度精度 0.1mm 已足够；
        // 只清洗 plates/track 厚度，hull_position 等坐标保持原精度）
        if let Some(obj) = v.as_object_mut() {
            for sec in ["hull", "turret", "gun"] {
                if let Some(plates) = obj.get_mut(sec)
                    .and_then(|s| s.get_mut("plates"))
                    .and_then(|p| p.as_object_mut())
                {
                    for (_, n) in plates.iter_mut() {
                        if let Some(f) = n.as_f64() { *n = json!((f * 10.0).round() / 10.0); }
                    }
                }
            }
            if let Some(ch) = obj.get_mut("chassis").and_then(|c| c.as_object_mut()) {
                for k in ["left_track", "right_track"] {
                    if let Some(n) = ch.get_mut(k) {
                        if let Some(f) = n.as_f64() { *n = json!((f * 10.0).round() / 10.0); }
                    }
                }
            }
        }
        v
    });

    let configs = build_configs(tank_id);
    let caliber = configs.first().and_then(|c| c.get("caliber")).and_then(|v| v.as_u64()).unwrap_or(120) as u32;
    // models.pb 模型信息只解析一次，hull_spaced / 原点 / 初始炮塔旋转共用同一份
    let m_info = crate::wargaming::blitzkit::model_info(tank_id);
    let hull_spaced = m_info.as_ref().map(|m| m.hull_spaced.clone()).unwrap_or_default();
    // 模型原点（models.pb，DAVA→GLB correctZY: (x,z,y)）——装甲节点定位基准，
    // 对齐 BlitzKit SpacedArmorScene 的 hullOrigin/turretOrigin 分组装配。
    let model_origins = m_info.as_ref().and_then(|m| match (m.track_origin, m.turret_origin) {
        (Some(tk), Some(tu)) => Some(json!({
            "track": [tk[0], tk[2], tk[1]],
            "turret": [tu[0], tu[2], tu[1]],
        })),
        _ => None,
    });
    let initial_turret_rotation = m_info.as_ref().and_then(|m| m.initial_turret_rotation.clone());
    // 炮管碰撞盒（game_data/{id}.json 的 collision.gun_bbox，564/723 车有数据）。
    // 坐标系与 GLB 内部一致（x=右 y=前 z=上），原点=炮管节点（枢轴）——
    // min[1]（后伸量）= 炮闩位置的文件标定。缺失时前端回退到枢轴本身。
    let gun_collision = game_data.as_ref().and_then(|gd| gd.collision.as_ref())
        .and_then(|c| c.gun_bbox.clone())
        .map(|b| json!({ "min": b.min, "max": b.max }));
    // 各部件原生碰撞盒（游戏数据，坐标系 = 各部件节点枢轴系 x右/y前/z上；
    // chassis/hull 位于模型原点）。供 DecodeShotSegment 解码盒使用。
    let collision_boxes = game_data.as_ref().and_then(|gd| gd.collision.as_ref())
        .map(|c| json!({
            "chassis": c.chassis_bbox.clone().map(|b| json!({ "min": b.min, "max": b.max })),
            "hull": c.hull_bbox.clone().map(|b| json!({ "min": b.min, "max": b.max })),
            "turret": c.turret_bbox.clone().map(|b| json!({ "min": b.min, "max": b.max })),
            "gun": c.gun_bbox.clone().map(|b| json!({ "min": b.min, "max": b.max })),
        }));

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
        "gun_collision": gun_collision,
        "collision_boxes": collision_boxes,
        "armor_model": armor_model_val,
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
            guns.reverse();
            turrets.reverse();
        }
    }
    (guns, turrets)
}

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

    let (model_guns, model_turrets) = model_config_nodes(tank_id);

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

    let tmod_info = crate::wargaming::blitzkit::model_info(tank_id);
    // 炮塔模型信息按 module_id 建索引，避免循环内对 tmod_info.turrets 反复线性查找
    let tmod_by_module: std::collections::HashMap<u32, &crate::wargaming::blitzkit::TurretModelInfo> =
        tmod_info.as_ref().map(|mi| {
            mi.turrets.iter().map(|t| (t.module_id, t)).collect()
        }).unwrap_or_default();

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

    let turret_model_node = |ti: usize, tmod: u32| -> Option<u32> {
        tmod_by_module.get(&tmod).map(|t| t.model_node)
            .or_else(|| Some((ti as u32) + 1))
    };
    let gun_model_info = |tmod: u32, gmod: u32| -> Option<(u32, Option<f32>, Option<f32>, Vec<u32>)> {
        tmod_by_module.get(&tmod)
            .and_then(|t| t.guns.iter().find(|g| g.gun_module_id == gmod))
            .map(|g| (g.model_node, g.thickness, g.mask, g.gun_spaced.clone()))
    };

    let mut configs = Vec::new();
    let mut count = 0u32;
    for (ti, tur) in tank.turrets.iter().enumerate() {
        // 该炮塔的模型信息只查一次，供 turret_spaced / gun_origin / yaw_limits 共用
        let turret_def = tmod_by_module.get(&tur.module_id);
        let turret_name = tur.name.clone();
        let turret_weight = Some(tur.weight);
        let turret_traverse = Some(tur.traverse_speed);
        let view_range = Some(tur.view_range);
        let turret_index = turret_model_node(ti, tur.module_id)
            .and_then(|n| turret_dense.get(&n).copied())
            .unwrap_or(ti as u32);
        for gun in &tur.guns {
            let (gun_node, gun_thickness, gun_mask, gun_spaced) = gun_model_info(tur.module_id, gun.module_id)
                .unwrap_or((u32::MAX, None, None, Vec::new()));
            let gun_index = if gun_node != u32::MAX {
                gun_dense.get(&gun_node).copied()
            } else { None }
                .unwrap_or_else(|| *gun_idx_by_module.get(&gun.module_id).unwrap_or(&0));
            let turret_spaced = turret_def
                .map(|t| t.turret_spaced.clone())
                .unwrap_or_default();
            // 火炮原点（models.pb TurretModelDefinition.gun_origin，DAVA→GLB correctZY: (x,z,y)）
            let gun_origin = turret_def
                .and_then(|t| t.gun_origin)
                .map(|d| [d[0], d[2], d[1]]);
            let yaw_limits = turret_def.and_then(|t| t.yaw_limits.clone());
            let pitch_limits = turret_def
                .and_then(|t| t.guns.iter().find(|g| g.gun_module_id == gun.module_id))
                .and_then(|g| g.pitch_limits.clone());
            let name = if gun.name.is_empty() { format!("gun_{}", gun_index) } else { gun.name.clone() };
            let caliber = parse_gun_caliber(&name).map(|c| c.round() as u32).unwrap_or(120);
            let aim_time = Some(gun.aim_time);
            let dispersion = Some(gun.dispersion);
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
            // 标准弹药单发伤害：优先 AP，其次任意非金币弹（金币变体 shell_type 含 premium 不计）；
            // DPM 与 Alpha 单发均基于此值
            let is_premium_shell = |t: &str| t.contains("premium");
            let standard_damage = gun.shells.iter()
                .find(|s| s.shell_type == "ap")
                .or_else(|| gun.shells.iter().find(|s| !is_premium_shell(&s.shell_type)))
                .map(|s| s.damage)
                .or_else(|| {
                    let m = gun.shells.iter().map(|s| s.damage).fold(f64::NEG_INFINITY, f64::max);
                    if m.is_finite() { Some(m) } else { None }
                })
                .unwrap_or(0.0);
            let dpm = if is_burst { None } else {
                reload_time.filter(|r| *r > 0.0).map(|r| (standard_damage * 60.0 / r).round())
            };

            configs.push(json!({
                "id": count,
                "label": name,
                "caliber": caliber,
                "shells": shells,
                "turret_index": turret_index,
                "gun_index": gun_index,
                // 模块局部 id（module_id>>8）——与 updateArena ARENA_INFO comp blob 对号，
                // 实际搭载配置解析（resolve_config_index）用
                "turret_local": (tur.module_id >> 8) as u16,
                "gun_local": (gun.module_id >> 8) as u16,
                "shell_global_ids": gun.shells.iter()
                    .filter(|s| s.id > 0)
                    .filter_map(|s| crate::replay::loadout::blitzkit_shell_global_id(&tank.nation, (s.id >> 8) as u64))
                    .collect::<Vec<u32>>(),
                "gun_thickness": gun_thickness,
                "gun_mask": gun_mask,
                "gun_spaced": gun_spaced,
                "turret_spaced": turret_spaced,
                // 火炮原点（correctZY 后的 GLB 坐标，装甲定位用，对齐 BlitzKit SpacedArmorScene）
                "gun_origin": gun_origin,
                "yaw_limits": yaw_limits,
                "pitch_limits": pitch_limits,
                "turret_name": turret_name,
                "reload_time": reload_time,
                "aim_time": aim_time,
                "dispersion": dispersion,
                "dpm": dpm,
                "standard_damage": standard_damage,
                "is_burst": is_burst,
                "is_drum": is_drum,
                "burst_size": burst_size,
                "burst_interval": burst_interval,
                "burst_reloads": burst_reloads,
                "turret_weight": turret_weight,
                "turret_traverse_speed": turret_traverse,
                "view_range": view_range,
                // 血量细分：总 HP = 车体 hp + 炮塔 health（对齐 TankResolver 口径）
                "hull_hp": tank.hp,
                "turret_health": tur.health,
                "model_gun_count": model_guns.len(),
                "model_turret_count": model_turrets.len(),
                "engines": tank.engines.clone(),
                "tracks": tank.tracks.clone(),
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

/// 发射弹种 → 射手弹表下标（确定性弹种选择）：
/// 按 shell_global_ids（= tanks.pb 弹种全局 id）匹配 build_configs 各配置的弹表，
/// 返回首个包含该弹的配置中弹的下标。type=28 槽位快照存在切弹竞态（shot6 实测），
/// shell_id 才是发射弹种的权威标识。
pub fn shell_index_by_global_id(tank_id: u32, shell_id: u32) -> Option<usize> {
    if shell_id == 0 { return None; }
    build_configs(tank_id).iter().find_map(|c| {
        c["shell_global_ids"].as_array().and_then(|a| {
            a.iter().position(|s| s.as_u64() == Some(shell_id as u64))
        })
    })
}

/// 实际搭载配置解析（共享证据链，射击复现与实时回放同步使用）：
/// 证据 0 = comp blob 局部 id（updateArena ARENA_INFO，确定性：炮塔/主炮 module_id>>8 直接对号）；
/// 证据 1 = 发射弹种 ⊆ 配置弹表（shell_global_ids）；
/// 证据 2 = 初始血量 vs 车体+炮塔 health（改进耐久 ×1.125，±2 容差）；
/// 依次回退，多匹配取最后一档（顶级），全无 → None（调用方默认顶级）。
/// 返回 = (build_configs 数组下标, turret_index, gun_index)。
pub fn resolve_config_index(
    tank_id: u32,
    comp: Option<(u16, u16)>,
    shell_ids: &[u32],
    hp: u16,
) -> Option<(usize, u32, u32)> {
    let configs = build_configs(tank_id);
    if configs.len() <= 1 { return None; }
    // 证据 0：comp blob 确定性对号
    if let Some((cl, gl)) = comp {
        let exact: Vec<usize> = (0..configs.len())
            .filter(|&i| {
                configs[i]["turret_local"].as_u64() == Some(cl as u64)
                    && configs[i]["gun_local"].as_u64() == Some(gl as u64)
            })
            .collect();
        if !exact.is_empty() {
            let i = *exact.last().unwrap();
            return Some((i,
                configs[i]["turret_index"].as_u64().unwrap_or(0) as u32,
                configs[i]["gun_index"].as_u64().unwrap_or(0) as u32));
        }
    }
    let fired: std::collections::HashSet<u32> = shell_ids.iter().copied().collect();
    let gun_ok: Vec<bool> = configs.iter().map(|c| {
        fired.is_empty() || {
            match c["shell_global_ids"].as_array() {
                Some(a) if !a.is_empty() => fired.iter().all(|id| {
                    a.iter().any(|s| s.as_u64() == Some(*id as u64))
                }),
                _ => true,   // 弹表缺失（数据不全）→ 不以此排除
            }
        }
    }).collect();
    let hp_val = hp as u32;
    let hp_ok: Vec<bool> = configs.iter().map(|c| {
        if hp_val == 0 { return true; }
        let base = c["hull_hp"].as_u64().unwrap_or(0) as u32
            + c["turret_health"].as_u64().unwrap_or(0) as u32;
        if base == 0 { return true; }
        let boosted = ((base as f64) * 1.125).round() as u32;
        hp_val.abs_diff(base) <= 2 || hp_val.abs_diff(boosted) <= 2
    }).collect();
    let both: Vec<usize> = (0..configs.len()).filter(|&i| gun_ok[i] && hp_ok[i]).collect();
    let mut cands = both;
    if cands.is_empty() { cands = (0..configs.len()).filter(|&i| gun_ok[i]).collect(); }
    if cands.is_empty() { cands = (0..configs.len()).filter(|&i| hp_ok[i]).collect(); }
    let i = *cands.last()?;
    Some((i,
        configs[i]["turret_index"].as_u64().unwrap_or(0) as u32,
        configs[i]["gun_index"].as_u64().unwrap_or(0) as u32))
}

fn parse_gun_caliber(name: &str) -> Option<f64> {
    let lower = name.to_lowercase();
    let (unit_mul, idx) = if let Some(i) = lower.find(" mm") {
        (1.0, i)
    } else {
        let i = lower.find(" cm")?;
        (10.0, i)
    };
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

pub(crate) async fn tank_filter_handler() -> Json<Value> {
    let mut out: Vec<serde_json::Value> = crate::wargaming::blitzkit::load_tanks()
        .values().map(|t| json!({
            "id": t.tank_id,
            "name": if t.name.is_empty() { t.dev_name.clone() } else { t.name.clone() },
            "tier": t.tier,
            "nation": t.nation.clone(),
            "type": t.tank_type.clone(),
        })).collect();

    out.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });
    Json(json!(out))
}

pub(crate) async fn shells_handler(axum::extract::Path(tank_id): axum::extract::Path<u32>) -> Json<Value> {
    let result: Value = crate::wargaming::blitzkit::tank_full(tank_id)
        .and_then(|t| t.turrets.first().and_then(|tur| tur.guns.first()).map(|g| {
            let caliber_mm = parse_gun_caliber(&g.name).map(|c| c.round() as u32).unwrap_or(120);
            let shells: Vec<Value> = g.shells.iter().map(|s| json!({
                "type": s.shell_type,
                // 全局弹种 id（与回放 shell_id 同域）：射击复现按 shell_id 反查槽位弹种用
                "global_id": crate::replay::loadout::blitzkit_shell_global_id(&t.nation, s.id as u64),
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
        /* 左上角排：信息面板与坦克选择器并排 */
        #corner-tl {
            position: fixed; top: 20px; left: 20px;
            display: flex; gap: 20px; align-items: flex-start;
        }
        #info-panel {
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
        /* 右下角栈：调试按钮(JS 动态挂入)与操作提示上下排列，互不遮挡 */
        #corner-br {
            position: fixed; bottom: 20px; right: 20px;
            display: flex; flex-direction: column; align-items: flex-end; gap: 8px;
        }
        #controls-hint { font-size: 0.8em; color: var(--muted); }
        /* 调试信息窗口：原挂在 3D 场景内的 sprite 文字标签改为在此集中展示，场景只留几何标记（球/线） */
        #debug-info {
            background: var(--panel); padding: 10px 14px; border-radius: var(--radius-sm);
            backdrop-filter: blur(12px); border: 1px solid var(--border); box-shadow: var(--shadow);
            display: none; min-width: 230px; max-width: 380px; max-height: 46vh; overflow-y: auto;
            font-family: Consolas, monospace; font-size: 11px;
        }
        #debug-info h4 { margin: 0 0 6px 0; font-size: 11px; font-weight: bold; color: var(--accent-2); font-family: system-ui, sans-serif; }
        #debug-info .dbg-row { display: flex; align-items: center; gap: 6px; margin: 3px 0; white-space: nowrap; }
        #debug-info .dbg-dot { flex: none; width: 8px; height: 8px; border-radius: 50%; }
        #debug-info .dbg-name { color: var(--muted); flex: none; }
        #debug-info .dbg-val { color: var(--txt); }
        #turret-controls {
            position: fixed; bottom: 20px; left: 20px;
            background: var(--panel); padding: 12px 16px; border-radius: var(--radius);
            backdrop-filter: blur(12px); border: 1px solid var(--border); box-shadow: var(--shadow);
            display: none; min-width: 280px; max-width: min(520px, 44vw);
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
        /* 右上角栈：弹种选择器 + 视图切换按钮 */
        #corner-tr {
            position: fixed; top: 20px; right: 20px;
            display: flex; flex-direction: column; align-items: flex-end; gap: 10px;
        }
        #shell-selector {
            background: var(--panel); padding: 10px 15px; border-radius: var(--radius-sm);
            backdrop-filter: blur(12px); border: 1px solid var(--border); box-shadow: var(--shadow);
        }
        #shell-selector select { background: #2c2724; color: var(--txt); border: 1px solid var(--border-hi); border-radius: var(--radius-sm); padding: 4px 9px; }
        #tank-selectors {
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
    <div id="corner-tl">
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
    </div>
    <div id="corner-tr">
        <div id="shell-selector" style="display:none;">
            <label style="font-size:0.85em;">Shell: </label>
            <select id="shell-select"></select>
        </div>
        <div id="view-toggle">
            <button id="collision-btn">Show Collision</button>
            <button id="penetration-btn">穿透热力图</button>
        </div>
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
    <div id="corner-br">
        <div id="controls-hint">Drag to rotate · Scroll to zoom · Left-click: armor · Right-drag: turret/gun</div>
    </div>
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
        let collisionMode = false;     // 碰撞模型显示开关（按钮与热力图退出恢复共用）
        let moduleMeshes = [];

        function tidyTrajectory() {
            if (trajGroup) { scene.remove(trajGroup); trajGroup = null; }
            trajInfoPos = null;
            document.getElementById('traj-info').style.display = 'none';
            document.getElementById('click-info').style.display = 'none';
        }

        function getPlateThickness(section, plateId) {
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
                // 炮管：XML 提取的 armor_model 优先，缺失时回退 models.pb 的 gun_thickness
                // （armor_cache 的 gun_plates 只含炮盾板，无 'gun' 炮管值）
                const gp = tankData.armor_model?.gun?.plates;
                return gp?.['gun'] ?? currentConfig()?.gun_thickness ?? null;
            }
            return null;
        }

        function isRealArmorThickness(t) {
            // BlitzKit resolveArmor：缺失/0 厚度按 0mm 渲染（thickness[index] ?? 0），
            // 仅 null（两份数据源都缺失）才视为 deco 跳过
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

        // ===== 实时穿透热力图（逐行对齐 BlitzKit PrimaryArmorSceneComponent fragment.glsl）=====
        // fragment shader 逐像素计算击穿概率着色：绿=稳定击穿 红=稳定挡住 渐变=概率过渡；
        // 跳弹(角度≥ricochet 且不满足三倍口径规则)→蓝紫高亮
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
            uniform bool greenPenetration;
            uniform bool advancedHighlighting;
            uniform bool opaque;
            uniform vec2 resolution;
            uniform float metersPerUnit;   // 视空间单位 → 米（场景原生米制，恒为 1，保留 uniform 兼容热力图管线）
            uniform sampler2D spacedArmorBuffer;   // R=外部/间隙甲 thickness/penetration, alpha!=0 表示有覆盖
            uniform highp sampler2D spacedArmorDepth; // 深度（HE 溅射用）
            uniform mat4 inverseProjectionMatrix;
            #include <clipping_planes_pars_fragment>
            float getDist(vec2 coord, float depth) {
              vec4 clip = vec4(coord * 2.0 - 1.0, depth * 2.0 - 1.0, 1.0);
              vec4 eye = inverseProjectionMatrix * clip;
              return length(eye.xyz / eye.w);
            }
            vec3 getPenetrationColor(bool isThreeCalibersRule, bool couldHaveRicochet) {
              if (advancedHighlighting && couldHaveRicochet) {
                return vec3(0.0, 1.0, isThreeCalibersRule ? 1.0 : 0.0);
              }
              return vec3(0.0, 1.0, 0.0);
            }
            void main() {
              #include <clipping_planes_fragment>
              vec2 sc = gl_FragCoord.xy / resolution;
              vec4 spacedData = texture2D(spacedArmorBuffer, sc);
              bool underSpaced = spacedData.a != 0.0;
              float viewDistance = length(vViewPos);
              float angle = acos(dot(vNormal, -vViewPos) / viewDistance);

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
                    // 深度反投影得到视空间距离；场景原生米制，metersPerUnit=1（恒等）
                    float distArmor = (primaryDist - spacedDist) * metersPerUnit;
                    if (canSplash) {
                      float finalDamage = 0.5 * damage * (1.0 - distArmor / explosionRadius) - 1.1 * (finalThick + spacedThick);
                      splashChance = step(0.0, finalDamage);
                      penChance = 0.0;
                    } else {
                      rem -= 0.5 * rem * distArmor;      // HEAT gap decay
                    }
                  }
                }
                if (penChance < 0.0) {
                  rem = max(0.0, rem);
                  float delta = finalThick - rem;
                  float rand = rem * 0.05;               // ±5% randomization band
                  penChance = clamp(1.0 - (delta + rand) / (2.0 * rand), 0.0, 1.0);
                  if (canSplash) {
                    float splash = 0.5 * damage - 1.1 * finalThick;
                    splashChance = step(0.0, splash);
                  }
                }
              }
              float alpha = opaque ? 1.0 : 0.5;
              vec3 base = vec3(1.0, splashChance * 0.392, 0.0);
              if (advancedHighlighting && ricocheted) base = vec3(1.0, base.g, 1.0);
              if (greenPenetration || advancedHighlighting) {
                float fall = 1.0 - penChance * penChance;
                float gain = 1.0 - (penChance - 1.0) * (penChance - 1.0);
                vec3 penColor = getPenetrationColor(threeCal, mayRicochet);
                gl_FragColor = vec4(fall * base + gain * penColor, alpha);
              } else {
                gl_FragColor = vec4(base, (1.0 - penChance) * alpha);
              }
              gl_FragColor.a *= opacity;
            }
        `;

        // ===== 穿透热力图：对齐 BlitzKit SpacedArmorScene（Armor/index.tsx useFrame）=====
        // spacedArmorScene 在【单次】gl.render 内按 renderOrder 排序：
        //   0 主装甲 omit(colorWrite:false, depthWrite:true，仅供 RT 遮挡)；
        //   1-2 间隙甲 additive(R=thickness/penetration)；3-4 外部模块(3=depth, 4=additive)；5 间隙甲 depth。
        // 结果写入 RT，primaryArmorScene(renderOrder 1) 读 RT 着色到屏幕。
        // 关键：omit/additive 是同一节点的两个独立 Mesh，深度缓冲贯穿全部 renderOrder → 被遮挡模块正确剔除。
        // 主循环顺序：autoClear=true 渲染 RT → autoClear=false 渲染场景 → clearDepth() 渲染 primaryArmorScene。
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

        let primaryMeshes = [], spacedMeshes = [], externalMeshes = [];
        let gunClipPlane = null;
        const _gunClipPlaneObj = new THREE.Plane();
        let gunMuzzleWorld = null;
        // 装甲旋转枢轴（alignArmorModules 按 models.pb 原点写入：track+turret / track+turret+gun）。
        // updateTurretGun 以此为炮塔/炮管的旋转中心（对齐 BlitzKit 旋转语义）。
        let armorPivotTurret = null, armorPivotGun = null;
        function computeGunClipPlane() {
            gunClipPlane = null;
            gunMuzzleWorld = null;
            const act = activeGunNumber();
            const cfg = currentConfig();
            if (act == null || !tankModel) return;
            // BlitzKit 真值语义：mask=0 视同无 mask（gunModelDefinition.mask ? ...）
            const hasMask = cfg && typeof cfg.gun_mask === 'number' && cfg.gun_mask !== 0;
            tankModel.updateMatrixWorld(true);
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
                // 更远的一端为炮口（按"远离世界原点"判向会因包围盒中心居中于原点而随机反转）。
                // cfg 与上文为同一次 currentConfig()（纯数组查找，原重复调用已合并）
                const mo0 = tankData && tankData.model_origins;
                let gunOriginWorld = null;
                if (mo0 && mo0.track && mo0.turret && cfg && cfg.gun_origin) {
                    const g = new THREE.Vector3(
                        mo0.track[0] + mo0.turret[0] + cfg.gun_origin[0],
                        mo0.track[1] + mo0.turret[1] + cfg.gun_origin[1],
                        mo0.track[2] + mo0.turret[2] + cfg.gun_origin[2]);
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
                const center = wb.getCenter(new THREE.Vector3());
                const halfAlongDir = (Math.abs(dir.x)*sz.x + Math.abs(dir.y)*sz.y + Math.abs(dir.z)*sz.z) / 2;
                gunMuzzleWorld = center.add(dir.clone().multiplyScalar(halfAlongDir));
            }
            if (!hasMask) return;
            const mo = tankData && tankData.model_origins;
            if (!(mo && mo.track && mo.turret && cfg.gun_origin && barrel)) return;
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
                if (node.visible === false || node.userData.configHidden) return;
                const sec = node.userData.armorSection;
                if (sec === 'deco') return;
                if (sec === 'spaced') spacedMeshes.push(node);
                else primaryMeshes.push(node);
            });
            externalMeshes = [];
            const act = activeGunNumber();
            const cfg = currentConfig();
            // BlitzKit 真值语义：mask=0 视同无 mask（SpacedArmorScene `gunModelDefinition.mask ?`）
            const hasMask = cfg && typeof cfg.gun_mask === 'number' && cfg.gun_mask !== 0;
            computeGunClipPlane();
            moduleMeshes.forEach(function(node) {
                if (!node.isMesh) return;
                if (node.visible === false) return;
                if (node.userData.gunConfig != null && act != null && node.userData.gunConfig !== act) return;
                // BlitzKit 选择规则：mask 有值 → gun_01 + gun_01_mask 子树都渲染；
                // mask 无值 → 只渲染 gun_01（炮管本体），mask 网格不参与
                if (node.userData.gunMaskPart && !hasMask) return;
                externalMeshes.push(node);
            });
        }

        const omitMaterial = new THREE.MeshBasicMaterial({ colorWrite: false, depthTest: true, depthWrite: true });
        const excludeMaterial = new THREE.MeshBasicMaterial({ colorWrite: false, depthTest: true, depthWrite: true });
        function penetrationMaterial(thickness) {
            return new THREE.ShaderMaterial({
                vertexShader: PBR_VERT, fragmentShader: PBR_FRAG,
                transparent: true, depthWrite: false,
                uniforms: {
                    thickness: { value: thickness },
                    penetration: { value: 200 },
                    caliber: { value: 120 },
                    ricochet: { value: 70.0 * Math.PI / 180 },
                    normalization: { value: 0.0 },
                    greenPenetration: { value: false },
                    advancedHighlighting: { value: true },
                    opaque: { value: false },
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
              gl_FragColor = vec4(thickness / penetration, 0.0, 0.0, 1.0);
            }
        `;
        function externalMaterial(thickness, penetration, clip) {
            return new THREE.ShaderMaterial({
                vertexShader: EXTERNAL_VERT, fragmentShader: EXTERNAL_FRAG,
                depthWrite: false, depthTest: true,
                blending: THREE.AdditiveBlending,
                clipping: clip != null,
                clippingPlanes: clip ? [clip] : null,
                uniforms: { thickness: { value: thickness }, penetration: { value: penetration || 200 } },
            });
        }
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
              float viewDistance = length(vViewPos);
              float angle = acos(dot(vNormal, -vViewPos) / viewDistance);
              bool threeCal = caliber > thickness * 3.0;
              if (!threeCal && angle >= ricochet) { gl_FragColor = vec4(1.0, 0.0, 0.0, 1.0); return; }
              bool twoCal = caliber > thickness * 2.0 && thickness > 0.0;
              float norm = twoCal ? (1.4 * normalization * caliber) / (2.0 * thickness) : normalization;
              float finalThick = thickness / cos(max(0.0, angle - norm));
              gl_FragColor = vec4(finalThick / penetration, 0.0, 0.0, 1.0);
            }
        `;
        function spacedMaterial(thickness, penetration) {
            return new THREE.ShaderMaterial({
                vertexShader: SPACED_VERT, fragmentShader: SPACED_FRAG,
                depthWrite: false, depthTest: true,
                blending: THREE.AdditiveBlending,
                uniforms: {
                    thickness: { value: thickness }, penetration: { value: penetration || 200 },
                    caliber: { value: 120 }, ricochet: { value: 70 * Math.PI / 180 }, normalization: { value: 0 },
                },
            });
        }
        function shellTypeOf(sh) {
            const t = ((sh && sh.type) || '').toLowerCase();
            if (t === 'hc' || t === 'hc_premium' || t === 'heat') return 'heat';
            if (t === 'ap_cr' || t === 'ap_cr_premium' || t === 'apcr') return 'apcr';
            if (t === 'he' || t === 'he_premium') return 'he';
            if (t === 'ap' || t === 'ap_premium') return 'ap';
            return t;
        }
        const SHELL_LABEL = { ap: 'AP', apcr: 'APCR', heat: 'HEAT', he: 'HE' };
        function shellLabel(s) { return SHELL_LABEL[shellTypeOf(s)] || (s && s.type) || '?'; }
        // 调试信息窗口：坐标等调试文字集中展示在窗口行内（颜色点 = 对应 3D 标记），不再往场景挂 sprite 标签
        function fmt3(x, y, z) { return '(' + x.toFixed(1) + ', ' + y.toFixed(1) + ', ' + z.toFixed(1) + ')'; }
        // dbgReset 在 world 分支入口重建窗口（清上一发残留行）；dbgInfo 惰性建行并更新文本
        function dbgReset(title) {
            let w = document.getElementById('debug-info');
            if (!w) {
                w = document.createElement('div');
                w.id = 'debug-info';
                (document.getElementById('corner-tr') || document.body).appendChild(w);
            }
            w.innerHTML = '<h4>' + title + '</h4>';
        }
        function dbgInfo(id, color, name, text) {
            const w = document.getElementById('debug-info');
            if (!w) return;
            let r = document.getElementById(id);
            if (!r) {
                r = document.createElement('div');
                r.id = id; r.className = 'dbg-row';
                r.innerHTML = '<span class="dbg-dot" style="background:' + color + '"></span>'
                    + '<span class="dbg-name">' + name + '</span><span class="dbg-val"></span>';
                w.appendChild(r);
            }
            r.querySelector('.dbg-val').textContent = text;
        }
        // 调试模式开关工厂（原命中/脱靶两分支各建一份几乎相同的 __debugSetVisible + 调试按钮）：
        // 收纳 World View 面板 + 调试信息窗口 + 全部调试标记；extraRefresh = 分支附加刷新
        // （命中分支传 __updateAnchorMarkers 刷新基准点标记，脱靶分支传 null）。
        // URL debug=1 的自动开启时序两分支不同，留在各自调用点处理
        function makeDebugToggle(extraRefresh) {
            window.__debugOn = false;
            window.__debugSetVisible = function(on) {
                window.__debugOn = on;
                if (window.__worldAnno) window.__worldAnno.visible = on;
                if (window.__moveAnno) window.__moveAnno.visible = on;
                if (window.__shooterMuzzleMk) window.__shooterMuzzleMk.visible = on;
                if (window.__shooterMuzzleLine) window.__shooterMuzzleLine.visible = on;
                if (extraRefresh) extraRefresh(on);
                // 面板与移动标注复选框仅在调试模式可交互
                const st = document.getElementById('turret-controls');
                if (st) st.style.display = on ? 'block' : 'none';
                // 调试信息窗口随开关显隐（内容行由各标记更新函数写入）
                const di = document.getElementById('debug-info');
                if (di) di.style.display = on ? 'block' : 'none';
            };
            const btn = document.createElement('button');
            btn.id = 'debug-toggle';
            btn.textContent = '调试标注';
            btn.style.cssText = 'padding:6px 14px;background:var(--panel);color:var(--accent);' +
                'border:1px solid var(--border);border-radius:var(--radius-sm);' +
                'font-size:0.85em;cursor:pointer;backdrop-filter:blur(12px);';
            btn.onclick = function() {
                window.__debugSetVisible(!window.__debugOn);
                this.textContent = window.__debugOn ? '隐藏调试标注' : '调试标注';
            };
            // 挂右下角栈：按钮贴角、操作提示在其上方，不重叠
            (document.getElementById('corner-br') || document.body).appendChild(btn);
            return btn;
        }
        // segment 弹种全局 id 解码（type=32 / method0x07 同源编码）：
        // 全局 = (shells.xml 局部 id << 8) | 国家基数字节（nation_id×16+10）
        function shellIdParts(gid) {
            if (!gid) return null;
            return { local: gid >> 8, nation: gid & 0xff };
        }
        // 模块损伤位掩码解码（method38 components：bit = componentToken − 31）
        function decodeModules(mask) {
            const NAMES = ['引擎','弹药架','油箱','右履带','左履带','火炮','?37','观察装置','?39','?40','?41','?42','?43'];
            const out = [];
            if (!mask) return out;
            for (let b = 0; b < 13; b++) if (mask & (1 << b)) out.push(NAMES[b] || ('bit' + b));
            return out;
        }
        // 命中结果分类：作者 = method38 位图（权威）；他人 hit_flags 恒 0 → 用 game_hit_result 枚举映射
        function shotResultClass(s) {
            // 伤害仲裁优先：一发命中可与目标多次装甲交互（method38 多消息已按位图并集合并，
            // combat.rs ③'）——先弹开又击穿的弹同时带 0x0008 与 0x0010 位，此时以 HP 伤害
            // 定性（伤害>0 = 击穿级结果）；HE 弹（0x1000）无论如何先按 HE 分类；
            // 伤害=0 时才按跳弹/未穿位与结果枚举细分。
            if (!s.target_name) return 'MISS';
            const flg = s.hit_flags || 0;
            const dmg = s.damage || 0;
            if (flg & 0x1000) return 'HE BLAST';
            if (dmg > 0) return 'PENETRATION';
            if (flg & 0x0008) return 'RICOCHET';
            if (flg & 0x0020 || flg & 0x0040 || flg & 0x0080) return 'NO PENETRATION';
            const r = s.game_hit_result;
            if (r === 4) return 'RICOCHET';
            if (r === 3) return 'PENETRATION';
            if (r === 1 || r === 2) return 'NO PENETRATION';
            return 'HIT';   // 0/255：服务器未通知，仅知有目标
        }
        // 单发数据质量提示（ShotQuality 降级/回退项，3D 面板展示）
        function srQualityIssues(s) {
            const q = s.quality;
            const issues = [];
            if (!q) return issues;
            if (q.shooter_pos_from_muzzle) issues.push('射手位置为炮口坐标兜底');
            if (q.target_anchor_src === 'nearest') issues.push('命中通知缺失，目标锚点回退命中时刻最近包');
            else if (q.target_anchor_src === 'extrapolated') issues.push('命中通知缺失，目标锚点按末段速度外推');
            else if (q.target_anchor_src === 'filtered') issues.push('命中通知缺失，目标锚点回退命中时刻插值');
            if ((q.turret_degraded || []).includes('target')) issues.push('目标炮塔角降级为车体朝向');
            if ((q.turret_degraded || []).includes('shooter')) issues.push('射手炮塔角降级为车体朝向');
            if (q.shell_from_broadcast) issues.push('弹种来自开火广播兜底');
            if (q.shell_from_terrain) issues.push('弹种来自地形命中广播兜底（0x1b）');
            if (q.shooter_pitch_from_velocity) issues.push('射手炮管俯仰由弹道推算');
            if (q.dmg_unattributed) issues.push('伤害未记账');
            if (s.target_name && !s.shell_id) issues.push('弹种未知');
            if (s.target_name && s.game_hit_result === 255) issues.push('服务器未通知命中结果');
            return issues;
        }
        function shellPenMul(s) {
            const calEl = document.getElementById('eq-calibrated');
            if (!(calEl && calEl.checked)) return 1.0;
            const t = shellTypeOf(s);
            return (t === 'ap' || t === 'apcr') ? 1.06 : 1.07;
        }
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
        let spacedArmorScene = null;
        let primaryArmorScene = null;

        // 穿透场景克隆工厂：原 addOmitClone/addColorClone/addDepthExcludeClone 三函数
        // 完全同构仅材质不同，合并为单一函数，调用点传各自材质
        function addClone(scene, src, mat, renderOrder) {
            const m = new THREE.Mesh(src.geometry, mat);
            m.renderOrder = renderOrder;
            m.userData._src = src;
            src.updateWorldMatrix(true, false);
            m.matrixAutoUpdate = false;
            m.matrix.copy(src.matrixWorld);
            scene.add(m);
            return m;
        }

        function buildSpacedArmorScene() {
            if (!armorModel) return;
            if (spacedArmorScene) spacedArmorScene.clear();
            else spacedArmorScene = new THREE.Scene();
            const pen = ((selectedShell && selectedShell.penetration) || 200) * equipmentCoeffs().penMul;
            primaryMeshes.forEach(function(node){ addClone(spacedArmorScene, node, omitMaterial, 0); });
            spacedMeshes.forEach(function(node){
                const t = node.userData.armorThickness || 0;
                const cm = addClone(spacedArmorScene, node, spacedMaterial(t, pen), 2);
                cm.userData._baseThickness = t;
                addClone(spacedArmorScene, node, omitMaterial, 5);
            });
            externalMeshes.forEach(function(node){
                const t = node.userData.armorThickness || 20;
                const clipped = node.userData.armorSection === 'gunBarrel' && gunClipPlane;
                const dmat = clipped
                    ? (() => { const m = externalDepthMaterial.clone(); m.clippingPlanes = [gunClipPlane]; return m; })()
                    : externalDepthMaterial;
                const cmat = externalMaterial(t, pen, clipped ? gunClipPlane : null);
                const od = addClone(spacedArmorScene, node, omitMaterial, 3);
                od.material = dmat;
                const cm2 = addClone(spacedArmorScene, node, cmat, 4);
                cm2.userData._baseThickness = t;
            });
        }
        function buildPrimaryArmorScene() {
            if (!armorModel) return;
            if (primaryArmorScene) primaryArmorScene.clear();
            else primaryArmorScene = new THREE.Scene();
            primaryMeshes.forEach(function(node){
                const t = node.userData.armorThickness;
                addClone(primaryArmorScene, node, excludeMaterial, 0);                        // 正面深度遮罩
                const cm = addClone(primaryArmorScene, node, penetrationMaterial(t == null ? 0 : t), 1);  // 着色
                cm.userData._baseThickness = t == null ? 0 : t;
            });
        }
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
        function updateSpacedUniforms(sh) {
            if (!spacedArmorScene) return;
            const t = shellTypeOf(sh);
            const isExplosive = t === 'he' || t === 'heat';
            const { penMul } = equipmentCoeffs();
            const pen = (sh.penetration || 0) * penMul;
            const cal = sh.caliber || 120;
            // BlitzKit：degToRad(shell.normalization ?? 0)
            const norm = (sh.normalization != null ? sh.normalization : 0) * Math.PI / 180;
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

        function disposeMaterial(mat) {
            if (!mat) return;
            if (mat.uniforms) {
                for (const u of Object.values(mat.uniforms)) {
                    const v = u && u.value;
                    if (v && v.isTexture && v !== _emptySpacedTex && !v.isRenderTargetTexture) v.dispose();
                }
            }
            if (mat.map && mat.map.isTexture && mat.map !== _emptySpacedTex) mat.map.dispose();
            mat.dispose();
        }
        // 单场景材质释放（原 primary/spaced 两段 traverse+dispose 完全重复，提取复用）
        function disposeSceneMaterials(scene) {
            if (!scene) return;
            scene.traverse(function(node){
                if (node.isMesh) { if (node.material && node.material.uniforms) disposeMaterial(node.material); }
            });
            scene.clear();
        }
        function disposePenetrationResources() {
            disposeSceneMaterials(primaryArmorScene);
            disposeSceneMaterials(spacedArmorScene);
            if (penetrationRT) {
                if (penetrationRT.depthTexture) penetrationRT.depthTexture.dispose();
                penetrationRT.dispose();
                penetrationRT = null;
            }
        }

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
        // 装甲模型视图样式（collision 按钮与热力图退出共用一份写入，避免各自硬编码漂移）：
        // 碰撞视图=按厚度上色并隐藏视觉车体；普通视图=透明覆盖层并显示视觉车体
        function applyArmorViewStyle(collision) {
            if (tankModel) tankModel.visible = !collision;
            armorModel.traverse(function(node) {
                if (!node.isMesh) return;
                if (node.userData.armorSection === 'deco') { node.visible = false; return; }
                node.material = collision
                    ? new THREE.MeshStandardMaterial({
                        color: thicknessToColor(node.userData.armorThickness), metalness: 0.4, roughness: 0.6,
                        transparent: true, opacity: 0.95, depthWrite: true,
                    })
                    : new THREE.MeshStandardMaterial({
                        color: 0x444444, metalness: 0.3, roughness: 0.8,
                        transparent: true, opacity: 0, depthWrite: false,
                    });
            });
        }
        function applyPenetrationMode(on) {
            if (!armorModel) return;
            if (!on) {
                disposePenetrationResources();
                armorModel.visible = true;
                // 回到进入热力图前的视图样式（collisionMode 仍开启则恢复碰撞视图）。
                // 不触碰模块网格可见性：热力图开启路径从不隐藏它们；此前在这里按
                // "mask 无值→隐藏 gunMaskPart" 重写视觉模型，会把 gun_XX_mask 炮盾视觉
                // 永久藏掉（有 mask 值的车则反向把 hide_elements 拆件点亮）——该规则只属于
                // collectPenetrationMeshes 的热力图外层网格选取
                applyArmorViewStyle(collisionMode);
                renderer.autoClear = true;
                renderer.setRenderTarget(null);
                return;
            }
            if (tankModel) tankModel.visible = true;
            rebuildHeatmapScenes();
            armorModel.visible = true;
            externalMeshes.forEach(function(node){ node.visible = true; });
            penetrationActive = true;
        }

        function updatePenetrationUniforms(sh) {
            const t = shellTypeOf(sh);
            const isHE = t === 'he';
            const isExplosive = isHE || t === 'heat';
            const cal = sh.caliber || 120;
            const { penMul } = equipmentCoeffs();
            const pen = (sh.penetration || 0) * penMul;
            // BlitzKit：degToRad(shell.normalization ?? 0)
            const norm = (sh.normalization != null ? sh.normalization : 0) * Math.PI / 180;
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
                    if (node.material.uniforms.metersPerUnit) {
                        node.material.uniforms.metersPerUnit.value = worldMetersPerUnit || 1;
                    }
                }
            });
            if (primaryArmorScene) apply(primaryArmorScene);
        }

        function renderSpacedArmorPass() {
            if (!spacedArmorScene) return;
            const rt = syncPenetrationRT();
            syncCloneMatrices(spacedArmorScene);
            computeGunClipPlane();
            const inject = (obj) => { obj.traverse(function(node){
                if (node.isMesh && node.material && node.material.uniforms && node.material.uniforms.spacedArmorBuffer) {
                    node.material.uniforms.spacedArmorBuffer.value = rt.texture;
                    if (node.material.uniforms.spacedArmorDepth && rt.depthTexture) node.material.uniforms.spacedArmorDepth.value = rt.depthTexture;
                    if (node.material.uniforms.inverseProjectionMatrix) node.material.uniforms.inverseProjectionMatrix.value = camera.projectionMatrixInverse;
                }
            }); };
            if (primaryArmorScene) inject(primaryArmorScene);
            renderer.autoClear = true;
            renderer.setRenderTarget(rt);
            renderer.setClearColor(0x000000, 0);
            renderer.render(spacedArmorScene, camera);
            renderer.autoClear = false;
            renderer.setRenderTarget(null);
        }

        document.getElementById('penetration-btn').addEventListener('click', function() {
            penetrationMode = !penetrationMode;
            this.classList.toggle('active', penetrationMode);
            this.textContent = penetrationMode ? '关闭热力图' : '穿透热力图';
            applyPenetrationMode(penetrationMode);
        });

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
                node.userData.armorSection = spaced ? 'spaced' : (section === 'gun' ? 'turret' : section);
            });
        }

        function tagArmorPlates(model) {
            model.traverse(function(node) {
                if (!node.isMesh) return;
                const name = node.name || '';
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
                    if (!isRealArmorThickness(t)) {
                        node.userData.armorSection = 'deco';
                        node.userData.armorPlateId = plateId;
                        node.userData.armorThickness = 0;
                        return;
                    }
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
                    node.userData.armorSection = 'gunBarrel';
                    node.userData.armorPlateId = 'gun';
                    node.userData.armorThickness = getPlateThickness('gunBarrel', 'gun');
                    node.userData.gunMaskPart = false;
                    const gm = parentName.match(/^gun_(\d+)/);
                    node.userData.gunConfig = gm ? parseInt(gm[1], 10) : null;
                    moduleMeshes.push(node);
                } else if (/^gun_\d+_/.test(parentName)) {
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

        let worldMetersPerUnit = 1;      // 场景原生米制：1 单位 = 1 米（模型不再缩放）
        let modelMaxDim = 6;             // 模型实际最长边（米）——仅用于相机取景推算
        // 不缩放（场景原生米制，1 单位 = 1 米）+【模型原点 = 场景原点】：glb 原点即游戏引擎
        // 的车体锚点（= 回放 type10 位置锚点），弹着点/炮口等回放数据零偏移直通；按包围盒
        // 中心对齐会有数厘米残差且被炮管前伸拉偏。代价：OrbitControls 旋转围绕车体锚点
        // （地面高度）而非视觉中心——数据精确性优先。
        function applyModelTransforms(model) {
            model.rotation.x = -Math.PI / 2;
            model.scale.setScalar(1);
            worldMetersPerUnit = 1;
            const box = new THREE.Box3().setFromObject(model);
            const size = box.getSize(new THREE.Vector3());
            modelMaxDim = Math.max(size.x, size.y, size.z);
            model.position.set(0, 0, 0);
        }

        function syncTransforms() {
            if (!tankModel || !armorModel) return;
            armorModel.rotation.copy(tankModel.rotation);
            armorModel.scale.copy(tankModel.scale);
            armorModel.position.copy(tankModel.position);
            collectConfigNodes(tankModel);
            alignArmorModules();
        }

        // models.pb 权威原点装配（对齐 BlitzKit SpacedArmorScene 的 hull/turret/gunOrigin 分组）。
        function alignArmorModules() {
            if (!armorModel || !tankModel) return;
            armorModel.position.copy(tankModel.position);
            armorModel.updateMatrixWorld(true);
            tankModel.updateMatrixWorld(true);

            const installPivot = (mesh, pivot) => {
                if (!mesh.userData.origPos) mesh.userData.origPos = mesh.position.clone();
                mesh.position.copy(mesh.userData.origPos).add(pivot);
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
            _armorPrefixCache = null;   // 装甲模型重建后前缀缓存失效
            if (trajGroup) { scene.remove(trajGroup); trajGroup = null; }
            if (window.__hitMarker) { scene.remove(window.__hitMarker); window.__hitMarker = null; }
            if (window.__endMarker) { scene.remove(window.__endMarker); window.__endMarker = null; }
            if (window.__dbgGroup) { scene.remove(window.__dbgGroup); window.__dbgGroup = null; }
            moduleMeshes = [];
            turretNode = null; gunNodesList = [];
            gunBarrelNodes = []; gunMaskNodes = [];
            configGunGroups = []; configTurretNodes = [];
            origMatrices = null; armorOrigMatrices = null;
        }

        function loadModels() {
            document.getElementById('loading').style.display = 'block';
            document.getElementById('loading').textContent = 'Loading tank model...';
            window.__LOAD__ = 'start';
            clearModels();
            const loader = new GLTFLoader();
            const fail = (phase) => (error) => {
                const msg = (typeof error === 'string') ? error : (error && (error.message || error.statusText || String(error))) || 'unknown';
                window.__LOAD__ = 'fail:' + phase + ':' + msg;
                console.error('Failed to load ' + phase + ':', error);
                document.getElementById('loading').textContent = 'Failed to load ' + phase + ': ' + msg;
            };

            loader.load(tankData.model_url, function(gltf) {
                armorModel = gltf.scene;
                _armorPrefixCache = null;   // 装甲模型重建后前缀缓存失效
                tagArmorPlates(armorModel);
                armorModel.traverse(function(node) {
                    if (node.isMesh && node.geometry) {
                        // 逐面法线（非索引化 + 逐面重算）：热力图着色的法线必须与点击判定
                        // 的 raycast 面法线同源——hit.face.normal 是几何面法线，若保留 GLB
                        // 平滑法线，曲面/棱线处热力图入射角会偏离判定入射角（实测 T110E5
                        // p90 差 6°，约 11% 表面击穿概率结论相反）。代价是逐面块状着色。
                        if (node.geometry.index) node.geometry = node.geometry.toNonIndexed();
                        node.geometry.computeVertexNormals();
                    }
                });
                armorModel.traverse(function(node) {
                    if (node.isMesh) {
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

            loader.load(tankData.visual_model_url, function(gltf) {
                tankModel = gltf.scene;
                applyModelTransforms(tankModel);
                tagModuleMeshes(tankModel);
                // 游戏的 hide_elements 拆件 = 特定状态（击杀镜头/部件脱落等）专用，
                // 战斗渲染恒隐藏——否则会作为多余组件暴露在炮塔/炮盾上且随其转动
                tankModel.traverse(function(node) {
                    if (!/^(gun_\d+|turret_\d+|hull)_hide_elements$/.test(node.name || '')) return;
                    node.traverse(function(m) {
                        if (m.isMesh) m.visible = false;
                    });
                });
                tankModel.traverse(function(node) {
                    if (node.isMesh) {
                        node.castShadow = true;
                        node.receiveShadow = true;
                    }
                });
                scene.add(tankModel);
                    // ?debug=1：标注模型原点（=车体原点=场景原点）与两个包围盒中心（验证定位）
                if (QP.get('debug') === '1') {
                    const dbg = new THREE.Group();
                    // 原点：模型原点 = 车体原点 = 场景原点（三轴 RGB=XYZ）
                    dbg.add(new THREE.AxesHelper(0.8));
                    const mk = (color) => new THREE.Mesh(
                        new THREE.SphereGeometry(0.09, 12, 10),
                        new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.95, depthTest: false }));
                    let sum = new THREE.Vector3(); let cnt = 0;
                    tankModel.updateWorldMatrix(true, true);
                    tankModel.traverse(function(node) {
                        if (!node.isMesh || !node.geometry || !node.geometry.attributes.position) return;
                        const pos = node.geometry.attributes.position;
                        for (let i = 0; i < pos.count; i++) {
                            sum.add(new THREE.Vector3().fromBufferAttribute(pos, i).applyMatrix4(node.matrixWorld));
                            cnt++;
                        }
                    });
                    const vC = cnt > 0 ? sum.divideScalar(cnt)
                        : new THREE.Box3().setFromObject(tankModel).getCenter(new THREE.Vector3());
                    const vM = mk(0x00ffff); vM.position.copy(vC); vM.renderOrder = 997; dbg.add(vM);
                    if (armorModel) {
                        const aC = new THREE.Box3().setFromObject(armorModel).getCenter(new THREE.Vector3());
                        const aM = mk(0x2b7bff); aM.position.copy(aC); aM.renderOrder = 997; dbg.add(aM);
                    }
                    window.__dbgGroup = dbg;
                    scene.add(dbg);
                }
                document.getElementById('loading').style.display = 'none';
                window.__LOAD__ = 'ok:' + tankData.tank_id;
                controls.target.set(0, 0, 0);   // 旋转中心 = 地面网格原点（模型锚点）
                controls.update();
                syncTransforms();
                applyConfig(currentConfigIdx);
                applyUrlOptionsOnce();
                window.__DBG__ = { scene, tankModel, armorModel, tankData, THREE, controls };
            }, undefined, fail('tank model'));
        }

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
            const shooterTank = parseInt(QP.get('shooter'), 10);
            if (!isNaN(shooterTank) && shooterTank > 0) {
                loadShooter(shooterTank);
            }
            const view = QP.get('view');
            const azOv = num('az'), distOv = num('dist'), hOv = num('h');
            if (view || azOv !== null || hOv !== null || distOv !== null) {
                // 炮线高度（米）：gun 枢轴的 glb z（场景原生米制，无缩放）
                const gunLine = (armorPivotGun ? armorPivotGun.z : 2.0);
                // 视角预设（对齐 BlitzKit 语义）：front/rear/left/right = 炮线高度水平视角；
                // hull_down = 低机位仰视炮塔；top = 俯视。机位距离 = 模型最长边倍数
                //（取景需要，与数据无关）：水平 0.80×、斜角 0.87×、顶视 1.17×；顶视高度 = 炮线 + 2×模型长
                const P = ({
                    front:       {az:0,   h:gunLine,     ty:gunLine,      d:0.80*modelMaxDim},
                    rear:        {az:180, h:gunLine,     ty:gunLine,      d:0.80*modelMaxDim},
                    left:        {az:90,  h:gunLine,     ty:gunLine,      d:0.80*modelMaxDim},
                    right:       {az:270, h:gunLine,     ty:gunLine,      d:0.80*modelMaxDim},
                    hull_down:   {az:0,   h:1.1,         ty:gunLine+0.3,  d:0.83*modelMaxDim},
                    top:         {az:0,   h:gunLine+2.0*modelMaxDim, ty:0.1*modelMaxDim, d:1.17*modelMaxDim},
                    front_left:  {az:45,  h:gunLine,     ty:gunLine,      d:0.87*modelMaxDim},
                    front_right: {az:315, h:gunLine,     ty:gunLine,      d:0.87*modelMaxDim},
                    rear_left:   {az:135, h:gunLine,     ty:gunLine,      d:0.87*modelMaxDim},
                    rear_right:  {az:225, h:gunLine,     ty:gunLine,      d:0.87*modelMaxDim},
                })[view] || {az:0, h:gunLine, ty:gunLine, d:0.80*modelMaxDim};
                const d = distOv !== null ? distOv : P.d;
                const a = (azOv !== null ? azOv : P.az) * Math.PI / 180;
                const h = hOv !== null ? hOv : P.h;
                camera.position.set(Math.sin(a) * d, h, -Math.cos(a) * d);
                controls.target.set(0, 0, 0);   // 旋转中心 = 地面网格原点（与初始状态一致）
                controls.update();
            }
            if (QP.get('heatmap') === '1' && !penetrationMode) {
                penetrationMode = true;
                const btn = document.getElementById('penetration-btn');
                btn.classList.add('active'); btn.textContent = '关闭热力图';
                applyPenetrationMode(true);
            }
            const shotNo = parseInt(QP.get('shot'), 10);
            const isShotReplay = !isNaN(shotNo);
            if (isShotReplay) {
                fetch('/api/replay_shot').then(r => {
                    if (!r.ok) { throw new Error('接口错误 HTTP ' + r.status); }
                    return r.json();
                }).then(d => {
                    const shots = d.shots || d;
                    const s = (Array.isArray(shots) ? shots : []).find(x => x.index === shotNo);
                    if (!s) { showShotError('shot #' + shotNo + ' 不存在（接口返回 ' + (Array.isArray(shots) ? shots.length : 0) + ' 发）'); return; }
                    window.__autoRelView = true;   // 默认相对视角（沿入射方向）
                    // 场景原生米制（1 单位 = 1 米，模型不缩放）：回放数据为真实米，直通使用
                    if (!armorPivotGun) { showShotError('炮管枢轴未安装（模型装配异常）'); return; }
                    const gunLine = armorPivotGun.z;   // 受击坦克炮管离地高（米）
                    // ===== 模型保持默认朝向（车头 -Z），相机做相对调整 =====
                    // toModel = 正交旋转 Ry(π−hullYaw)（无镜像）：世界前向 (sin,0,+cos) 映到模型
                    // 前向 -Z、世界右方映到模型右方；含 z 取反的反射版会使相对视图左右互换。
                    const ta = s.target_ang || [0, 0, 0];
                    // ===== world=1 模式：双模型世界坐标渲染 =====
                    const isWorld = true;   // 世界模式 = 射击复现主视图（旧装甲查看器主视图已删除，world=1 参数兼容但不再需要）
                    if (isWorld) mirrorShotData(s);   // 函数声明提升，定义见下
                    // ===== 命中冲击姿态滤波（必须在 mirror【之后】构造）=====
                    // 实测命中时刻 type10 姿态含冲击晃动（0.1s 内 roll 突变 ~12°），弹丸命中的是
                    // 冲击【前】车体 → 命中 tick(dt≈0) 的 pitch/roll 用最近 dt<0 采样替代（yaw 保留）。
                    // mirror 会原地取反 tick_samples 的 yaw/roll——此处读到的已是镜像值，与
                    // 滑块/回退路径的相邻采样同号（先拷贝会残留未镜像 roll，命中时刻侧倾
                    // 符号翻转 → 移动转向目标姿态跳变、弹着点偏移）。
                    let hitAtt = null;
                    {
                        let pre = null;
                        for (const t of (s.tick_samples || [])) {
                            if (t.dt < -0.02 && (!pre || t.dt > pre.dt)) pre = t;
                        }
                        if (pre && pre.dt > -0.35) hitAtt = { pitch: pre.pitch, roll: pre.roll };
                    }
                    // 判定层姿态回退（无渲染锚点时摆放用；同因必须在 mirror 后读取）
                    const taF = [ (ta[0]||0),
                        (hitAtt ? hitAtt.pitch : (ta[1]||0)),
                        (hitAtt ? hitAtt.roll : (ta[2]||0)) ];
                    // ===== 世界镜像校正（数据入口一次性变换：x/yaw/roll 取反）=====
                    // 回放坐标系(BigWorld)与 three.js 右手系在水平面上手性相反：直接渲染左右舷互换。
                    // 镜像 = x 取反、yaw/roll 取反（pitch 不变——绕 x 轴旋转在 x 镜像下不变），
                    // 模型网格不动，经共轭旋转自动呈正确手性。
                    function mirrorShotData(s) {
                        if (s.__worldMirrored) return;
                        s.__worldMirrored = true;
                        const negX = (a) => { if (Array.isArray(a) && a.length >= 3) a[0] = -a[0]; };
                        const negAng = (a) => { if (Array.isArray(a) && a.length >= 3) { a[0] = -a[0]; a[2] = -a[2]; } };
                        negX(s.ball_a); negX(s.ball_b); negX(s.launch_velocity);
                        negX(s.shooter_pos); negX(s.target_pos);
                        negX(s.aim_point); negX(s.launch_point_rel);
                        // 炮塔相对角时间线：透传不取反（存游戏系原始值）。
                        // 镜像场景的【节点旋转角】须取负（−rel，镜像翻转旋转方向），
                        // 该取反在使用点完成——同初始摆放 turretDegT = 镜像炮塔角−镜像车体角 = −rel
                        const mirTL = (tl) => { if (Array.isArray(tl)) for (const t of tl) { negX(t.pos); t.yaw = -t.yaw; t.roll = -t.roll; } };
                        mirTL(s.target_render_timeline); mirTL(s.shooter_render_timeline);
                        negAng(s.shooter_ang); negAng(s.target_ang);
                        if (typeof s.shooter_turret_yaw === 'number') s.shooter_turret_yaw = -s.shooter_turret_yaw;
                        if (typeof s.target_turret_yaw === 'number') s.target_turret_yaw = -s.target_turret_yaw;
                        for (const t of (s.tick_samples || [])) { negX(t.pos); t.yaw = -t.yaw; t.roll = -t.roll; }
                        for (const t of (s.shooter_tick_samples || [])) { negX(t.pos); t.yaw = -t.yaw; t.roll = -t.roll; }
                        if (s.terrain_impact) { negX(s.terrain_impact.impact_point); negX(s.terrain_impact.segment_start); }
                        if (s.shooter_aim && typeof s.shooter_aim.turret_rel_yaw === 'number') {
                            s.shooter_aim.turret_rel_yaw = -s.shooter_aim.turret_rel_yaw;
                        }
                        // 渲染锚点与时间线同规则镜像：pos.x / yaw / roll 取反（pitch 不变）。
                        // 此前漏取反 roll——初始摆放（锚点）与滑块 dt=0（时间线，已取反）在
                        // 有侧倾的移动目标上姿态差一个侧倾符号，拖动滑块才"恢复"。
                        if (s.target_render) { negX(s.target_render.pos); negAng(s.target_render.ang); }
                        if (s.shooter_render) { negX(s.shooter_render.pos); negAng(s.shooter_render.ang); }
                    }
                     // world 穿透判定标志每次进入射击复现重置（防上次会话残留）
                     window.__worldPenMode = false;
                     window.__worldServerInfo = null;
                     // 片元交叉验证上下文（doPenetrationCheck 读取）：世界模式同样提供 segment 解码字段
                     window.__shotCtx = Object.assign({}, window.__shotCtx || {}, {
                         segArmorGroup: s.armor_group || 0,
                         segShellId: s.shell_id || 0,
                         segResult: (typeof s.game_hit_result === 'number') ? s.game_hit_result : 255,
                     });
                    // 射手炮塔/炮管姿态（回放原始数据）：炮塔 = type7 prop2 绝对朝向 − 模型偏航；
                    // 炮管俯仰 = launch_velocity 垂直分量反解（prop9 实测与真实弹道无关，弃用）。
                    // cfgIdx = 射手实际搭载配置下标（build_configs 数组序，回放数据注入）；
                    // 多配置坦克按其选炮塔/主炮变体（applyConfigVisible 同式），null = 默认顶级
                    function poseShooterTurretGun(sModel, sd, turretAbsYaw, lv, hullYawS, hullPitchS, hullRollS, gunPitchRad, turretRelOverride, cfgIdx) {
                        if (!sModel || !sd) return;
                        const turrets = [];
                        const gunGroups = new Map();
                        sModel.traverse(function(n) {
                            const nm = n.name || '';
                            const gm = nm.match(/^gun_(\d+)/);
                            const tm = nm.match(/^turret_(\d+)$/);
                            if (gm) { const g = parseInt(gm[1], 10); if (!gunGroups.has(g)) gunGroups.set(g, []); gunGroups.get(g).push(n); }
                            else if (tm) turrets.push(n);
                        });
                        turrets.sort(function(a, b) { return ((a.name.match(/\d+/)[0] | 0) - (b.name.match(/\d+/)[0] | 0)); });
                        const gKeys = Array.from(gunGroups.keys()).sort(function(a, b) { return a - b; });
                        // cfgIdx = configs 数组下标；越界/缺失 → 顶级（末位）
                        const nCfg = (sd.configs && sd.configs.length) || 1;
                        const cfgSel = (Number.isInteger(cfgIdx) && cfgIdx >= 0 && cfgIdx < nCfg) ? cfgIdx : nCfg - 1;
                        const selCfg = sd.configs ? sd.configs[cfgSel] : null;
                        const selTi = selCfg ? selCfg.turret_index : 0;
                        const selGi = selCfg ? selCfg.gun_index : 0;
                        // 未选变体隐藏（含子树）；选中的炮塔/主炮参与摆位
                        turrets.forEach(function(n, i) { n.visible = (i === (selTi % turrets.length)); });
                        gKeys.forEach(function(k, i) { gunGroups.get(k).forEach(function(n) { n.visible = (i === (selGi % gKeys.length)); }); });
                        let turretNode = turrets.length ? turrets[selTi % turrets.length] : null;
                        let gunNodes = gKeys.length ? gunGroups.get(gKeys[selGi % gKeys.length]) : [];
                        const mo = sd.model_origins;
                        if (!turretNode || !(mo && mo.track && mo.turret)) {
                            console.warn('[world] shooter turret/gun pose skipped:',
                                !turretNode ? 'no turret node' : 'no model_origins');
                            return;
                        }
                        const cfg = selCfg;
                        const tP = [mo.track[0]+mo.turret[0], mo.track[1]+mo.turret[1], mo.track[2]+mo.turret[2]];
                        let gP = tP.slice();
                        if (cfg && cfg.gun_origin) gP = [tP[0]+cfg.gun_origin[0], tP[1]+cfg.gun_origin[1], tP[2]+cfg.gun_origin[2]];
                        // 炮塔相对角：优先用显式 rel（镜像系 = −rel；避免"判定层绝对角 −
                        // 渲染层 hull yaw"往返在移动转向时混入转向率×滤波延迟的角度误差）；
                        // 未提供时退回绝对角相减（旧路径）
                        const tr = (turretRelOverride != null) ? turretRelOverride : (turretAbsYaw - hullYawS);
                        // lv → 射手车体系：undo 完整车体姿态（yaw/pitch/roll）。仅 undo 偏航时
                        // 坡地射击会带上车体俯仰的假俯仰（shot3 T110 实测渲染 +6.5°、真值 −0.1°）。
                        // 注意 poseFromYPR 含 qFrame（z-up→y-up 帧变换），其逆作用得到的是帧前
                        // 中间系，还需转回 glb 车体系（+Y 前/+Z 上）；等价做法：仰角 = asin(lv·upWorld/|lv|)。
                        let gr = 0;
                        if (gunPitchRad != null) {
                            // 回放时序驱动（与客户端一致）：炮管俯仰 = type=10 pitch 时序插值
                            gr = gunPitchRad;
                        } else if (lv) {
                            const qH = poseFromYPR(hullYawS, hullPitchS || 0, hullRollS || 0);
                            const llv = Math.hypot(lv[0], lv[1], lv[2]);
                            if (llv > 0.1) {
                                // 炮管运动学：模型系炮管方向 = Rz(炮塔rel)·Rx(俯仰)·(0,1,0)。
                                // 反解：①世界→车体(qH⁻¹)消地形俯仰/侧倾 → ②车体→炮塔系(Rz(−rel))消
                                // 水平偏航 → ③仰角 = atan2(z, y)。未消炮塔偏航时 roll 混入俯仰
                                // (GB109 shot2:炮塔 rel −67°,解出 +22.8°,真值 +9.5°,差 13°)。
                                const lvB = new THREE.Vector3(lv[0], lv[1], lv[2]).applyQuaternion(qH.clone().invert());
                                const cT = Math.cos(tr), sT = Math.sin(tr);
                                const lvT = new THREE.Vector3(
                                    lvB.x*cT + lvB.y*sT, -lvB.x*sT + lvB.y*cT, lvB.z);
                                gr = Math.atan2(lvT.z, lvT.y);
                            }
                        }
                        let turretRot = new THREE.Matrix4().makeRotationZ(tr);
                        const itr = sd.initial_turret_rotation;
                        if (itr) {
                            turretRot = new THREE.Matrix4().makeRotationFromEuler(new THREE.Euler(
                                -THREE.MathUtils.degToRad(itr.pitch), -THREE.MathUtils.degToRad(itr.roll),
                                tr - THREE.MathUtils.degToRad(itr.yaw), 'XYZ'));
                        }
                        const mT = new THREE.Matrix4().makeTranslation(tP[0], tP[1], tP[2]).multiply(turretRot)
                            .multiply(new THREE.Matrix4().makeTranslation(-tP[0], -tP[1], -tP[2]));
                        const mG = mT.clone().multiply(new THREE.Matrix4().makeTranslation(gP[0], gP[1], gP[2]))
                            .multiply(new THREE.Matrix4().makeRotationX(gr))
                            .multiply(new THREE.Matrix4().makeTranslation(-gP[0], -gP[1], -gP[2]));
                        turretNode.updateMatrix(); turretNode.matrixAutoUpdate = false;
                        // 首次调用缓存各节点烘焙矩阵；后续调用基于烘焙矩阵重摆
                        //（幂等——否则滑块每次输入都会累乘旋转，炮塔越转越乱）
                        if (!turretNode.userData.__poseBaked) {
                            turretNode.userData.__poseBaked = turretNode.matrix.clone();
                            for (const gn of gunNodes) {
                                gn.userData.__poseBaked = gn.matrix.clone();
                                // 必须关自动合成：否则 updateMatrixWorld 用未修改的
                                // quaternion 重写 matrix，炮管姿态被静默丢弃（渲染在车体正前向）
                                gn.matrixAutoUpdate = false;
                            }
                        }
                        turretNode.matrix.copy(mT.clone().multiply(turretNode.userData.__poseBaked));
                        for (const gn of gunNodes) {
                            gn.matrix.copy(mG.clone().multiply(gn.userData.__poseBaked));
                        }
                        sModel.updateMatrixWorld(true);
                        console.log('[world] shooter turret/gun posed: turretRel=' +
                            (-(turretAbsYaw - hullYawS) * 180 / Math.PI).toFixed(1) + '° gunPitch=' +
                            (gr * 180 / Math.PI).toFixed(1) + '° gunNodes=' + gunNodes.length);
                    }
                    // type10 原始欧拉 → 场景四元数（世界直通摆放，双方模型共用）。
                    // 车体前向 = (sin yaw, 0, +cos yaw)，glb 前向 = 内部 +Y（炮管伸出端）；
                    // 帧变换 Q0 = Ry(π)·Rx(−π/2)：内部 +Y→场景+Z、+Z→场景+Y（正交保向）。
                    // pitch/roll 是车体轴旋转，必须作用在【偏航前】的 yaw0 系上（前轴=+Z、
                    // 右轴=−X → −pitch ≡ Rx(+pitch)、+roll = Rz(+roll)）：
                    // Q = Ry(+yaw)·Rx(+pitch)·Rz(+roll)·Q0。把轴写成偏航后世界轴会使俯仰
                    // 落到错误轴——坡地姿态错约 2×坡度（GB109 shot2 爬 15° 坡实测错 19°）。
                    // 符号实证：pitch 正=车头下坡（+15° 上坡全部 tick pitch≈−15.5）；
                    // roll 正=右倾（4 份回放反解 ball_a 车体偏移，垂直分量 std 收窄 2-13×）。
                    function poseFromYPR(yaw, pitch, roll) {
                        const qYpi = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), Math.PI);
                        const qFrame = qYpi.multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), -Math.PI / 2));
                        const qYaw = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), yaw || 0);
                        const qPitch = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), pitch || 0);
                        const qRoll = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), roll || 0);
                        return qYaw.multiply(qPitch).multiply(qRoll).multiply(qFrame);
                    }
                    // 射手模型加载工厂（原命中/脱靶两分支大段重复：并行 fetch /api/tank +
                    // GLTFLoader 加载 → applyPose 摆位 → 炮闩 bake 前捕获 → 半透明材质克隆
                    // → scene.add）。resolve({ sd, sModel, breechGunLocal })；GLB 加载失败
                    // resolve(null)。分支差异参数化：logTag（日志前缀）、castShadow
                    // （命中分支网格投影阴影，脱靶分支不投影）、applyPose（各自位姿闭包）。
                    // applyPose 必须在炮闩捕获前调用——breechGunLocal 依赖模型当前世界矩阵。
                    function loadShooterModel(tid, opts) {
                        const castShadow = !!(opts && opts.castShadow);
                        const logTag = (opts && opts.logTag) || '[world]';
                        // URL 前缀跟随目标模型（Web 服务挂 /armor_view/glb/...，独立 viewer 挂 /glb/...，
                        // 硬编码 /glb/ 在 Web 下 404）；/api/tank 不手动拼前缀——viewer_index_html
                        // 已对字面量 '/api/ 加前缀，手动拼会双重前缀 404。
                        const shooterGlbUrl = tankData.visual_model_url.replace(/\/glb\/\d+\//, '/glb/' + tid + '/');
                        return Promise.all([
                            fetch('/api/tank/' + tid).then(r => r.ok ? r.json() : null).catch(() => null),
                            new Promise(function(res) {
                                new GLTFLoader().load(shooterGlbUrl,
                                    function(g) { res(g); }, undefined, function() { res(null); });
                            }),
                        ]).then(function(arr) {
                            const sd = arr[0], gltf = arr[1];
                            if (!gltf) { console.warn(logTag + ' shooter model load failed'); return null; }
                            const sModel = gltf.scene;
                            sModel.scale.setScalar(1);
                            // 同受击方：hide_elements 拆件战斗渲染恒隐藏（否则随炮塔/炮盾转动暴露）
                            sModel.traverse(function(n) {
                                if (!/^(gun_\d+|turret_\d+|hull)_hide_elements$/.test(n.name || '')) return;
                                n.traverse(function(m) {
                                    if (m.isMesh) m.visible = false;
                                });
                            });
                            if (opts && opts.applyPose) opts.applyPose(sModel);
                            // 炮闩 gun 局部坐标——必须在炮塔/炮管 bake【之前】捕获：
                            // bake 会改写 gun 节点矩阵（绕枢轴偏航/俯仰），之后无法从模型系反推。
                            let breechGunLocal = null;
                            try {
                                let gunPre = null;
                                sModel.traverse(function(n) {
                                    if (!gunPre && /^gun_\d+$/.test(n.name || '')) gunPre = n;
                                });
                                const moPre = sd && sd.model_origins;
                                if (gunPre && moPre) {
                                    const scP = (sd.configs && sd.configs.length) ? sd.configs[sd.configs.length - 1] : null;
                                    // 炮闩标注 = 炮管枢轴 gP(models.pb 原点链)本身，不用碰撞 YAML 后伸量修正
                                    const breechModel = new THREE.Vector3(
                                        moPre.track[0]+moPre.turret[0]+((scP && scP.gun_origin) ? scP.gun_origin[0] : 0),
                                        moPre.track[1]+moPre.turret[1]+((scP && scP.gun_origin) ? scP.gun_origin[1] : 0),
                                        moPre.track[2]+moPre.turret[2]+((scP && scP.gun_origin) ? scP.gun_origin[2] : 0));
                                    sModel.updateMatrixWorld(true);
                                    breechGunLocal = gunPre.worldToLocal(sModel.localToWorld(breechModel));
                                }
                            } catch (e) { console.warn(logTag + ' breech capture failed:', e); }
                            sModel.traverse(function(node) {
                                if (node.isMesh) {
                                    if (castShadow) node.castShadow = true;
                                    if (node.material) {
                                        node.material = node.material.clone();
                                        node.material.transparent = true;
                                        node.material.opacity = 0.6;
                                    }
                                }
                            });
                            sModel.updateMatrixWorld(true);
                            scene.add(sModel);
                            return { sd: sd, sModel: sModel, breechGunLocal: breechGunLocal };
                        });
                    }
                        // 解除装甲检视的近距限制（fog 15-50 / maxDistance 30 会吞掉远距离交战）
                        scene.fog = null;
                        controls.maxDistance = 1e6;
                        controls.minDistance = 0.5;
                        // 右键 = 平移视角（地面平面），禁用手动炮塔/炮管拖拽（回放姿态由数据驱动）
                        controls.mouseButtons.RIGHT = THREE.MOUSE.PAN;
                        controls.screenSpacePanning = false;
                        window.__worldPan = true;
                        const hintEl = document.getElementById('controls-hint');
                        if (hintEl) hintEl.textContent = 'Drag to rotate · Scroll to zoom · Right-drag: pan';
                        // ===== 脱靶弹分支：无目标模型，仅射手 + 弹道 + 落点/材质标注 =====
                        // terrain_impact（method 0x1b）提供精确落点与弹道末段起点。
                        const isMiss = !s.target_name;
                        if (isMiss) {
                            const sTS2 = s.shooter_tick_samples || [];
                            const sHit2 = sTS2.reduce((a, b) =>
                                (Math.abs(b.dt) < Math.abs(a.dt)) ? b : a, sTS2[0]);
                            const shooterPos2 = sHit2 ? sHit2.pos : s.shooter_pos;
                            const ba = s.ball_a || null, bb = s.ball_b || null;
                            if (!ba || !bb || !(ba[0] || ba[1] || ba[2])) { showShotError('脱靶弹缺少弹道数据（ball_a/ball_b）'); return; }
                            dbgReset('调试信息 — Shot #' + s.index + '（脱靶）');
                            // 场景中心 = 弹道弦中点（模型为参照物的最小摆放）
                            const mcx = (ba[0] + bb[0]) / 2, mcy = (ba[1] + bb[1]) / 2, mcz = (ba[2] + bb[2]) / 2;
                            // 目标模型加载但隐藏：viewer 主流程依赖 tankModel/armorModel 存在
                            tankModel.visible = false;
                            armorModel.visible = false;
                            // 射手模型：复用命中分支的加载/放置逻辑（loadShooterModel 工厂）
                            const shooterTid2 = parseInt(QP.get('shooter'), 10);
                            if (shooterTid2 > 0) {
                                loadShooterModel(shooterTid2, {
                                    logTag: '[world-miss]',
                                    applyPose: function(sModel) {
                                        const sYaw2 = sHit2 ? sHit2.yaw : 0;
                                        sModel.quaternion.copy(poseFromYPR(sYaw2, sHit2 ? sHit2.pitch : 0, sHit2 ? sHit2.roll : 0));
                                        sModel.position.set(shooterPos2[0] - mcx, shooterPos2[1] - mcy, shooterPos2[2] - mcz);
                                        // 锚点 = type10 记录位置,不做 ball_a 对齐平移（原因见命中分支注释）
                                        window.__shooterFix = new THREE.Vector3(0, 0, 0);
                                    },
                                }).then(function(res) {
                                    if (!res) return;
                                    const sd = res.sd, sModel = res.sModel, breechGunLocal2 = res.breechGunLocal;
                                    const sYaw2 = sHit2 ? sHit2.yaw : 0;
                                    // 脱靶分支同命中分支：显式 rel（镜像系相减还原 −rel）；prop2
                                    // frac 俯仰直用，回退时弹速反解
                                    const sgp2 = (s.quality && s.quality.gun_pitch_degraded &&
                                        s.quality.gun_pitch_degraded.indexOf('shooter') >= 0)
                                        ? null : (s.shooter_gun_pitch != null ? s.shooter_gun_pitch : null);
                                    const sRel2 = (s.shooter_turret_yaw || 0) - (s.shooter_ang ? s.shooter_ang[0] : 0);
                                    poseShooterTurretGun(sModel, sd, s.shooter_turret_yaw, s.launch_velocity, sYaw2,
                                        sHit2 ? sHit2.pitch : 0, sHit2 ? sHit2.roll : 0, sgp2, sRel2, s.shooter_config_idx);
                                    // 基准点标记（调试模式）：type10 记录位置锚点，坐标写入调试信息窗口。
                                    // 脱靶分支无 tick 切换，模型静态——标记挂 __worldAnno 随调试开关显隐。
                                    if (window.__worldAnno) {
                                        const amk2 = new THREE.Mesh(new THREE.SphereGeometry(0.18, 12, 10),
                                            new THREE.MeshBasicMaterial({ color: 0x00aaff, transparent: true, opacity: 0.95, depthTest: false }));
                                        amk2.position.copy(sModel.position);
                                        amk2.renderOrder = 998;
                                        window.__worldAnno.add(amk2);
                                        dbgInfo('dbg-anchor-shooter', '#00aaff', '基准点·射手',
                                            fmt3(sModel.position.x + mcx, sModel.position.y + mcy, sModel.position.z + mcz));
                                    }
                                    // 炮闩标注（脱靶分支，镜像命中分支）:位置随炮塔/炮管 bake
                                    // 变换，标签附回放世界系坐标；对照线连服务器发射点 ball_a
                                    try {
                                        if (breechGunLocal2 && window.__worldAnno) {
                                            let gunNode2 = null;
                                            sModel.traverse(function(n) {
                                                if (!gunNode2 && /^gun_\d+$/.test(n.name || '')) gunNode2 = n;
                                            });
                                            if (gunNode2) {
                                                gunNode2.updateMatrixWorld(true);
                                                const bpos = breechGunLocal2.clone().applyMatrix4(gunNode2.matrixWorld);
                                                const bm2 = new THREE.Mesh(new THREE.SphereGeometry(0.12, 12, 10),
                                                    new THREE.MeshBasicMaterial({ color: 0xcc66ff, transparent: true, opacity: 0.95, depthTest: false }));
                                                bm2.position.copy(bpos);
                                                bm2.renderOrder = 998;
                                                window.__worldAnno.add(bm2);
                                                dbgInfo('dbg-breech', '#cc66ff', '炮闩',
                                                    fmt3(bpos.x + mcx, bpos.y + mcy, bpos.z + mcz));
                                                const dl2 = new THREE.Line(
                                                    new THREE.BufferGeometry().setFromPoints([bpos.clone(),
                                                        new THREE.Vector3(ba[0]-mcx, ba[1]-mcy, ba[2]-mcz)]),
                                                    new THREE.LineDashedMaterial({
                                                        color: 0xffffff, transparent: true, opacity: 0.8,
                                                        dashSize: 0.35, gapSize: 0.25, depthTest: false }));
                                                dl2.computeLineDistances();
                                                dl2.renderOrder = 996;
                                                window.__worldAnno.add(dl2);
                                            }
                                        }
                                    } catch (e) { console.warn('[world-miss] muzzle marker failed:', e); }
                                });
                            }
                            // 标注组
                            if (window.__worldAnno) { scene.remove(window.__worldAnno); }
                            window.__worldAnno = new THREE.Group();
                            const chordM = Math.sqrt((bb[0]-ba[0])**2 + (bb[1]-ba[1])**2 + (bb[2]-ba[2])**2);
                            const trajGeo2 = new THREE.BufferGeometry().setFromPoints([
                                new THREE.Vector3(ba[0]-mcx, ba[1]-mcy, ba[2]-mcz),
                                new THREE.Vector3(bb[0]-mcx, bb[1]-mcy, bb[2]-mcz),
                            ]);
                            const trajLine2 = new THREE.Line(trajGeo2,
                                new THREE.LineBasicMaterial({ color: 0x00ff00, transparent: true, opacity: 0.7, depthTest: false }));
                            trajLine2.renderOrder = 997;
                            window.__worldAnno.add(trajLine2);
                            const lpM2 = new THREE.Mesh(new THREE.SphereGeometry(0.14, 12, 10),
                                new THREE.MeshBasicMaterial({ color: 0xff6622, transparent: true, opacity: 0.95, depthTest: false }));
                            lpM2.position.set(ba[0]-mcx, ba[1]-mcy, ba[2]-mcz);
                            lpM2.renderOrder = 998;
                            window.__worldAnno.add(lpM2);
                            const lv2 = s.launch_velocity || [0, 0, 0];
                            const spd2 = Math.sqrt(lv2[0]*lv2[0] + lv2[1]*lv2[1] + lv2[2]*lv2[2]);
                            dbgInfo('dbg-launch', '#ff6622', '发射点',
                                fmt3(ba[0], ba[1], ba[2]) + ' · ' + spd2.toFixed(0) + ' m/s');
                            if (spd2 > 1) {
                                const dir2 = new THREE.Vector3(lv2[0], lv2[1], lv2[2]).normalize();
                                const vg2 = new THREE.BufferGeometry().setFromPoints([
                                    lpM2.position.clone(),
                                    lpM2.position.clone().addScaledVector(dir2, Math.max(chordM * 1.3, 10)),
                                ]);
                                const vl2 = new THREE.Line(vg2, new THREE.LineDashedMaterial({
                                    color: 0x00ccff, transparent: true, opacity: 0.8,
                                    dashSize: 1.2, gapSize: 0.8, depthTest: false }));
                                vl2.computeLineDistances();
                                vl2.renderOrder = 997;
                                window.__worldAnno.add(vl2);
                            }
                            // 落点标记（黄）= method20 终点；terrain_impact 附加末段起点（弹跳点）与材质标签
                            const emk2 = new THREE.Mesh(new THREE.SphereGeometry(0.15, 12, 10),
                                new THREE.MeshBasicMaterial({ color: 0xffcc00, transparent: true, opacity: 0.95, depthTest: false }));
                            emk2.position.set(bb[0]-mcx, bb[1]-mcy, bb[2]-mcz);
                            emk2.renderOrder = 998;
                            window.__worldAnno.add(emk2);
                            dbgInfo('dbg-end', '#ffcc00', '落点',
                                fmt3(bb[0], bb[1], bb[2]) + (s.terrain_impact ? ' · 材质' + s.terrain_impact.material : ''));
                            if (s.terrain_impact) {
                                const ss3 = s.terrain_impact.segment_start;
                                // 末段起点 ≠ 发射点 → 存在弹跳：补一条末段线 + 起点标记
                                const segLen = Math.sqrt((bb[0]-ss3[0])**2 + (bb[1]-ss3[1])**2 + (bb[2]-ss3[2])**2);
                                if (segLen > 0.5 && Math.sqrt((ss3[0]-ba[0])**2 + (ss3[1]-ba[1])**2 + (ss3[2]-ba[2])**2) > 0.5) {
                                    const segGeo = new THREE.BufferGeometry().setFromPoints([
                                        new THREE.Vector3(ss3[0]-mcx, ss3[1]-mcy, ss3[2]-mcz),
                                        new THREE.Vector3(bb[0]-mcx, bb[1]-mcy, bb[2]-mcz),
                                    ]);
                                    const segLn = new THREE.Line(segGeo, new THREE.LineBasicMaterial({
                                        color: 0xffaa00, transparent: true, opacity: 0.9, depthTest: false }));
                                    segLn.renderOrder = 997;
                                    window.__worldAnno.add(segLn);   // 随调试开关收纳(原 scene 直挂漏显)
                                    const skM = new THREE.Mesh(new THREE.SphereGeometry(0.11, 12, 10),
                                        new THREE.MeshBasicMaterial({ color: 0xffaa00, transparent: true, opacity: 0.95, depthTest: false }));
                                    skM.position.set(ss3[0]-mcx, ss3[1]-mcy, ss3[2]-mcz);
                                    skM.renderOrder = 998;
                                    window.__worldAnno.add(skM);
                                    dbgInfo('dbg-ricochet', '#ffaa00', '弹跳点', fmt3(ss3[0], ss3[1], ss3[2]));
                                }
                            }
                            scene.add(window.__worldAnno);
                            // 相机 = 射手侧弹道 3/4 视角（与命中分支同构：侧偏 + 上抬）
                            const dirT2 = new THREE.Vector3(bb[0]-ba[0], 0, bb[2]-ba[2]);
                            const lenH2 = dirT2.length();
                            if (lenH2 > 1) {
                                dirT2.divideScalar(lenH2);
                                const perp2 = new THREE.Vector3(-dirT2.z, 0, dirT2.x);
                                camera.position.set(ba[0]-mcx, ba[1]-mcy, ba[2]-mcz)
                                    .addScaledVector(perp2, Math.max(lenH2 * 0.3, 6))
                                    .add(new THREE.Vector3(0, Math.max(lenH2 * 0.22, 4), 0));
                            } else {
                                camera.position.set(ba[0]-mcx, ba[1]-mcy, ba[2]-mcz).add(new THREE.Vector3(0, 6, 8));
                            }
                            controls.target.set((ba[0]+bb[0])/2 - mcx, (ba[1]+bb[1])/2 - mcy, (ba[2]+bb[2])/2 - mcz);
                            controls.update();
                            // 信息面板：落点/材质 + 弹种
                            const st2 = document.getElementById('turret-controls');
                            const flgM = s.hit_flags || 0;
                            if (st2) { st2.innerHTML = '<div class="ctrl-row"><b>World View — Shot #' + s.index + ' (脱靶)</b></div>' +
                                (function() {
                                    const t4 = s.is_author ? '作者' : (s.shooter_team === 'enemy' ? '敌方' : (s.shooter_team === 'ally' ? '我方' : ''));
                                    return '<div class="ctrl-row">Shooter: <b style="color:#ffcf5c;">' + (s.shooter_name || '—') + '</b>' + (t4 ? ' · ' + t4 : '') + '</div>';
                                })() +
                                '<div class="ctrl-row">DMG 0 · MISS · 弦长 ' + chordM.toFixed(0) + 'm</div>' +
                                (s.terrain_impact ? '<div class="ctrl-row" style="font-size:10px;color:#8ab4ff;">落点材质类: ' +
                                    s.terrain_impact.material + ' · 末段起点距落点 ' + Math.sqrt(
                                    (s.terrain_impact.impact_point[0]-s.terrain_impact.segment_start[0])**2 +
                                    (s.terrain_impact.impact_point[1]-s.terrain_impact.segment_start[1])**2 +
                                    (s.terrain_impact.impact_point[2]-s.terrain_impact.segment_start[2])**2).toFixed(1) + 'm</div>' : '') +
                                '<div class="ctrl-row" style="font-size:10px;color:#888;">' +
                                '<span style="color:#ff6622;">●</span> LaunchPoint <span style="color:#00ccff;">┄</span> 速度向量 ' +
                                '<span style="color:#00ff00;">—</span> 弹道弦 <span style="color:#ffcc00;">●</span> 落点 ' +
                                '<span style="color:#ffaa00;">●</span> 弹跳点 ' +
                                '<span style="color:#cc66ff;">●</span> 炮闩(发射起点) ' +
                                '<span style="color:#00aaff;">●</span> 基准点·射手(type10锚)</div>' +
                                '<div class="ctrl-row" style="font-size:10px;color:#888;">flags=' + flgM.toString(16) +
                                ' · shell_id=' + (s.shell_id || '—') +
                                (s.shell_kind ? ' (' + s.shell_kind + ')' : '') +
                                (s.quality && s.quality.shell_from_terrain ? ' · <span style="color:#8ab4ff;">弹种来自 0x1b 地形广播</span>' : '') +
                                '</div>'; }
                            // 脱靶弹分支同样受调试开关收纳:默认隐藏面板与标注
                            const dbgBtnM = makeDebugToggle(null);
                            // URL debug=1 自动开启（与命中分支同语义）
                            window.__debugSetVisible(QP.get('debug') === '1');
                            dbgBtnM.textContent = window.__debugOn ? '隐藏调试标注' : '调试标注';
                            return;
                        }
                        if (s.target_name) {
                        dbgReset('调试信息 — Shot #' + s.index);
                        const tPos = s.target_pos;           // 目标命中时刻位置（世界系绝对坐标）
                        const sTS = s.shooter_tick_samples || [];
                        // 射手开火时刻采样：优先 |dt| 最小者（首条 |dt|<0.1 可能是更早样本，
                        // terrain 俯仰/侧倾与开火时刻差数度）；无窗内样本才回退首条
                        const sHit = sTS.reduce((a, b) =>
                            (Math.abs(b.dt) < Math.abs(a.dt)) ? b : a, sTS[0]);
                        const shooterWorld = s.shooter_render ? s.shooter_render.pos
                            : (sHit ? sHit.pos : s.shooter_pos);   // 射手开火时刻位置（渲染锚点优先）

                        // 场景中心 = 两车中点（相机取景方便）
                        const cx = (tPos[0] + shooterWorld[0]) / 2;
                        const cy = (tPos[1] + shooterWorld[1]) / 2;
                        const cz = (tPos[2] + shooterWorld[2]) / 2;

                        // 目标模型：**渲染锚点**直通（客户端显示位姿 = 滤波器输出，滞后
                        // 0.1~0.2s——游戏画面里模型的实际呈现；弹着点/弹孔 = 玩家所见）。
                        // 无渲染锚点回退判定层（taF）。与滑块 dt=0（滤波时间线起点）同源，
                        // 初始视图与滑块零点严格一致。行进/倒车目标两层相差 latency×速度
                        // （1436 shot#4：倒车 1.5m/s × 0.17s = 0.31m 沿车体前向——判定层
                        // 摆放会显得"偏后"，即弹着点视觉错位的来源）。
                        const useRT = !!s.target_render;
                        const baseT = useRT ? s.target_render.pos : tPos;
                        const angT = useRT ? s.target_render.ang : taF;
                        tankModel.position.set(baseT[0] - cx, baseT[1] - cy, baseT[2] - cz);
                        const qT = poseFromYPR(angT[0], angT[1], angT[2]);
                        tankModel.quaternion.copy(qT);
                        // 热力图/碰撞模型必须与视觉模型同位同姿——position 只在
                        // 首帧缺失会导致装甲模型留在原点
                        armorModel.position.copy(tankModel.position);
                        armorModel.quaternion.copy(qT);
                        tankModel.updateMatrixWorld(true);
                        armorModel.updateMatrixWorld(true);

                        // 目标炮塔/炮管：炮塔相对角 = prop2 粗值 − 判定层 hull yaw（两侧同层
                        // 相消还原纯 rel；不能用渲染层 angT[0]——移动转向时 (判定−渲染) yaw
                        // 差 = 转向率×滤波延迟会混入炮塔角，静止无感、移动偏移的来源）；
                        // 炮管俯仰 = prop2 低 6 位 frac 比例解码（弧度，正=仰角）→ 度，
                        // 回退（无采样/无锚定）时保持 0（车体 pitch 已随节点链呈现）
                        let turretDegT = ((s.target_turret_yaw || 0) - ta[0]) * 180 / Math.PI;
                        turretDegT = ((turretDegT + 180) % 360 + 360) % 360 - 180;
                        window.__fireGunDegT = (s.target_gun_pitch || 0) * 180 / Math.PI;
                        updateTurretGun(turretDegT, window.__fireGunDegT);

                        // 射手模型：加载并放置（loadShooterModel 工厂，并行取 /api/tank 供炮塔/炮管枢轴）
                        const shooterTid = parseInt(QP.get('shooter'), 10);
                        if (shooterTid > 0) {
                            loadShooterModel(shooterTid, {
                                castShadow: true,
                                logTag: '[world]',
                                applyPose: function(sModel) {
                                    // 射手渲染锚点优先（与受击方同一渲染层语义）；无则 type10 直通
                                    const sR = s.shooter_render;
                                    const sYaw = sR ? sR.ang[0] : (sHit ? sHit.yaw : 0);
                                    sModel.quaternion.copy(poseFromYPR(sYaw,
                                        sR ? sR.ang[1] : (sHit ? sHit.pitch : 0),
                                        sR ? sR.ang[2] : (sHit ? sHit.roll : 0)));
                                    sModel.position.set(shooterWorld[0] - cx, shooterWorld[1] - cy, shooterWorld[2] - cz);
                                    // 锚点 = type10 记录位置（与目标方同一原则，不做发射点对齐平移）。
                                    // 实测（T110E5 逐发）：ball_a 是服务器炮膛生成点（静止时在锚点上方
                                    // ~2.0m/沿炮管 0.1~1.3m，与 models.pb 炮管枢轴垂直差 ~0.6m），而 type10
                                    // 位置是客户端平滑值（行进时与服务器真值偏差 ~2m）。拖模型向 ball_a 会让
                                    // 车体错位（炮口对上、履带全偏）——残余偏差如实呈现：静止 <0.5m，行进 ~1-2m。
                                    window.__shooterFix = new THREE.Vector3(0, 0, 0);
                                },
                            }).then(function(res) {
                                if (!res) return;
                                const sd = res.sd, sModel = res.sModel, breechGunLocal = res.breechGunLocal;
                                // 姿态源 = 渲染锚点（与车体摆放同一渲染层）；炮塔相对角 = 显式
                                // rel（镜像系：shooter_turret_yaw 与 shooter_ang 均已被 mirror
                                // 取反，相减还原 −rel，与滑块路径同约定）；炮管俯仰 prop2 frac
                                // 解码直用（炮塔系，正=仰角）；回退（degraded）传 null → 弹速反解
                                const sR2 = s.shooter_render;
                                const sYaw = sR2 ? sR2.ang[0] : (sHit ? sHit.yaw : 0);
                                const sgp = (s.quality && s.quality.gun_pitch_degraded &&
                                    s.quality.gun_pitch_degraded.indexOf('shooter') >= 0)
                                    ? null : (s.shooter_gun_pitch != null ? s.shooter_gun_pitch : null);
                                const sRel = (s.shooter_turret_yaw || 0) - (s.shooter_ang ? s.shooter_ang[0] : 0);
                                poseShooterTurretGun(sModel, sd, s.shooter_turret_yaw, s.launch_velocity, sYaw,
                                    sR2 ? sR2.ang[1] : (sHit ? sHit.pitch : 0),
                                    sR2 ? sR2.ang[2] : (sHit ? sHit.roll : 0), sgp, sRel, s.shooter_config_idx);
                                // 炮闩标注：bake 前捕获的 gun 局部坐标 × bake 后 gun.matrixWorld——
                                // 精确跟随炮塔偏航/炮管俯仰（来源 = 炮管枢轴 gP，不做后伸量修正）。
                                try {
                                    if (breechGunLocal) {
                                        let gunNode = null;
                                        sModel.traverse(function(n) {
                                            if (!gunNode && /^gun_\d+$/.test(n.name || '')) gunNode = n;
                                        });
                                        if (gunNode) {
                                            window.__shooterGunNode = gunNode;
                                            window.__updateMuzzleMarker = function() {
                                                const g = window.__shooterGunNode, mk = window.__shooterMuzzleMk;
                                                if (!g || !mk) return;
                                                g.updateMatrixWorld(true);
                                                mk.position.copy(breechGunLocal.clone().applyMatrix4(g.matrixWorld));
                                                // 可见性跟随调试模式(位置始终重算,切换时无需重放)
                                                const on = !!window.__debugOn;
                                                mk.visible = on;
                                                if (window.__shooterMuzzleLine && window.__launchPointMk) {
                                                    const lgeo = window.__shooterMuzzleLine.geometry;
                                                    lgeo.setFromPoints([mk.position, window.__launchPointMk.position]);
                                                    window.__shooterMuzzleLine.computeLineDistances();
                                                    window.__shooterMuzzleLine.visible = on;
                                                }
                                                // 炮闩坐标（回放世界系 = 场景坐标 + 中心偏移）写入调试信息窗口
                                                dbgInfo('dbg-breech', '#cc66ff', '炮闩',
                                                    fmt3(mk.position.x + cx, mk.position.y + cy, mk.position.z + cz));
                                            };
                                            const mk = new THREE.Mesh(new THREE.SphereGeometry(0.12, 12, 10),
                                                new THREE.MeshBasicMaterial({ color: 0xcc66ff, transparent: true, opacity: 0.95, depthTest: false }));
                                            mk.renderOrder = 998;
                                            scene.add(mk);
                                            window.__shooterMuzzleMk = mk;
                                            // 对照线:炮闩(文件标定) ⇄ 服务器发射点(橙色 LaunchPoint),
                                            // 白色虚线,长度即两套数据的偏差——随 tick 重算一起更新
                                            const dline = new THREE.Line(
                                                new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(), new THREE.Vector3()]),
                                                new THREE.LineDashedMaterial({
                                                    color: 0xffffff, transparent: true, opacity: 0.8,
                                                    dashSize: 0.35, gapSize: 0.25, depthTest: false }));
                                            dline.renderOrder = 996;
                                            scene.add(dline);
                                            window.__shooterMuzzleLine = dline;
                                            window.__updateMuzzleMarker();
                                            console.log('[world] shooter breech(gun-local) @', breechGunLocal.toArray().map(v => +v.toFixed(2)).join(','));
                                        }
                                    }
                                } catch (e) { console.warn('[world] muzzle marker failed:', e); }
                                // 注册全局引用（滑块/炮口标记/基准点标注消费）
                                window.__shooterModel = sModel;
                                window.__shooterData = sd;
                                // 射手模型就位后刷新基准点标注（创建时模型可能尚未加载）
                                if (window.__updateAnchorMarkers) window.__updateAnchorMarkers();
                            });
                        }

                        // 弹道原始数据（method29 launchPoint + method20 终点，世界系绝对坐标）
                        const ba = s.ball_a, bb = s.ball_b;
                        // ===== 基准点标记（调试模式）：模型根节点 = type10 记录位置锚点 =====
                        // 随各自 tick 跟随移动；坐标写入调试信息窗口（原场景内 sprite 标签已移除）。
                        function makeAnchorMarker(colorCss) {
                            const mk = new THREE.Mesh(new THREE.SphereGeometry(0.18, 12, 10),
                                new THREE.MeshBasicMaterial({ color: new THREE.Color(colorCss), transparent: true, opacity: 0.95, depthTest: false }));
                            mk.renderOrder = 998;
                            mk.visible = false;
                            scene.add(mk);
                            return { mk: mk, color: colorCss };
                        }
                        window.__victimAnchor = makeAnchorMarker('#00ffcc');
                        window.__shooterAnchor = makeAnchorMarker('#00aaff');
                        window.__updateAnchorMarkers = function() {
                            const on = !!window.__debugOn;
                            const upd = function(a, model, rowId, name) {
                                if (!a) return;
                                if (!model) { a.mk.visible = false; return; }
                                a.mk.position.copy(model.position);
                                a.mk.visible = on;
                                // 坐标 = 回放世界系（场景坐标 + 中心偏移）
                                dbgInfo(rowId, a.color, name,
                                    fmt3(model.position.x + cx, model.position.y + cy, model.position.z + cz));
                            };
                            upd(window.__victimAnchor, tankModel, 'dbg-anchor-victim', '基准点·受击');
                            upd(window.__shooterAnchor, window.__shooterModel, 'dbg-anchor-shooter', '基准点·射手');
                        };
                        window.__updateAnchorMarkers();
                        if (window.__worldAnno) { scene.remove(window.__worldAnno); }
                        window.__worldAnno = new THREE.Group();
                        // tick 切换重算弹着点所需的射线上下文（在弹道块内赋值）。
                        // 击穿判定射线方向用【弹道弦向量】(ball_b − ball_a)而非初速向量——
                        // 弦已包含重力下坠，与命中终点自洽；launch_velocity 保留作初速
                        // 方向可视化(青虚线)与炮管仰角反解。
                        // 轨迹可视化开关（2026-09-24 用户指令）：命中弹判定已切换到
                        // DecodeShotSegment P1/P2 基准——弦/发射点/速度/终点/来向可视化隐藏；
                        // 脱靶弹无 P1/P2，弹道线是其核心复现内容，保持显示。
                        const SHOW_TRAJ_ANNO = (shotResultClass(s) === 'MISS');
                        let ctxLaunch = null, ctxLvDir = null, ctxLvSpd = 0, ctxRayFar = 100;
                        if (ba && bb) {
                            const chordLen = Math.sqrt(
                                (bb[0]-ba[0])**2 + (bb[1]-ba[1])**2 + (bb[2]-ba[2])**2);
                            // 击穿判定射线 = 弦向量方向(归一化),长度覆盖整条弦
                            const chordDir = new THREE.Vector3(
                                (bb[0]-ba[0])/chordLen, (bb[1]-ba[1])/chordLen, (bb[2]-ba[2])/chordLen);
                            ctxLvDir = chordDir;
                            ctxRayFar = Math.max(chordLen * 1.1, 20);
                            // depthTest=false：击穿弹的弦线穿过车体内部，保持全程可见
                            const trajMat = new THREE.LineBasicMaterial({ color: 0x00ff00, transparent: true, opacity: 0.7, depthTest: false });
                            const trajGeo = new THREE.BufferGeometry().setFromPoints([
                                new THREE.Vector3(ba[0] - cx, ba[1] - cy, ba[2] - cz),
                                new THREE.Vector3(bb[0] - cx, bb[1] - cy, bb[2] - cz),
                            ]);
                            const trajLine = new THREE.Line(trajGeo, trajMat);
                            trajLine.renderOrder = 997;
                            trajLine.visible = SHOW_TRAJ_ANNO;
                            window.__worldAnno.add(trajLine);   // 随调试开关收纳(原 scene 直挂漏显)
                            // ① launchPoint 标记（method29 发射点，橙色）+ 速度标签
                            const lpM = new THREE.Mesh(new THREE.SphereGeometry(0.14, 12, 10),
                                new THREE.MeshBasicMaterial({ color: 0xff6622, transparent: true, opacity: 0.95, depthTest: false }));
                            lpM.position.set(ba[0] - cx, ba[1] - cy, ba[2] - cz);
                            lpM.renderOrder = 998;
                            lpM.visible = SHOW_TRAJ_ANNO;
                            window.__worldAnno.add(lpM);
                            window.__launchPointMk = lpM;   // 供模型发射点对照线引用（隐藏但保留引用）
                            const lv0 = s.launch_velocity || [0, 0, 0];
                            const lvSpd = Math.sqrt(lv0[0]*lv0[0] + lv0[1]*lv0[1] + lv0[2]*lv0[2]);
                            ctxLaunch = lpM.position.clone();
                            ctxLvSpd = lvSpd;
                            if (SHOW_TRAJ_ANNO) dbgInfo('dbg-launch', '#ff6622', '发射点',
                                fmt3(ba[0], ba[1], ba[2]) + ' · ' + lvSpd.toFixed(0) + ' m/s');
                            // ② 速度向量轨迹（青虚线，长度=弦长×1.3）——仅初速方向可视化，判定用弦向量
                            if (lvSpd > 1) {
                                const dir = new THREE.Vector3(lv0[0], lv0[1], lv0[2]).normalize();
                                const vg = new THREE.BufferGeometry().setFromPoints([
                                    lpM.position.clone(),
                                    lpM.position.clone().addScaledVector(dir, Math.max(chordLen * 1.3, 10)),
                                ]);
                                const vl = new THREE.Line(vg, new THREE.LineDashedMaterial({
                                    color: 0x00ccff, transparent: true, opacity: 0.8,
                                    dashSize: 1.2, gapSize: 0.8, depthTest: false }));
                                vl.computeLineDistances();
                                vl.renderOrder = 997;
                                vl.visible = SHOW_TRAJ_ANNO;
                                window.__worldAnno.add(vl);
                            }
                            // ③ 弹道终点标记（method20，黄色）+ 标签——击穿弹终点在车体
                            // 内部/另一侧，≠弹着点（接触点）
                            const eg = new THREE.SphereGeometry(0.15, 12, 10);
                            const em = new THREE.MeshBasicMaterial({ color: 0xffcc00, transparent: true, opacity: 0.95, depthTest: false });
                            const emk = new THREE.Mesh(eg, em);
                            emk.position.set(bb[0] - cx, bb[1] - cy, bb[2] - cz);
                            emk.renderOrder = 998;
                                emk.visible = SHOW_TRAJ_ANNO;
                            window.__worldAnno.add(emk);
                                if (SHOW_TRAJ_ANNO) dbgInfo('dbg-end', '#ffcc00', '服务器终点', fmt3(bb[0], bb[1], bb[2]));
                            // ④ 服务器弹着点 = 弦向量射线(launchPoint + 弦方向) ∩ 目标装甲模型，
                            // 取沿射线首个非 deco 命中（弦含重力下坠，弦终点即 ball_b）。
                            // 若服务器下发了受击部件索引（server_part_index，报告 §4.7），射线
                            // 优先约束到该部件（与游戏 DecodeShotSegment 部件约束同构）；该部件
                            // 无交点时回退全模型（部件标注差异容错）。
                            if (armorModel) {
                                const sPart = (typeof s.server_part_index === 'number') ? s.server_part_index : null;
                                const partOf = o => { const u = o.userData || {}; const sec = u.armorSectionOrig || u.armorSection;
                                    return sec === 'chassis' ? 0 : sec === 'hull' ? 1 : (sec === 'turret' || sec === 'gun') ? 2 : sec === 'gunBarrel' ? 3 : -1; };
                                const rc2 = new THREE.Raycaster();
                                rc2.set(lpM.position.clone(), chordDir);
                                rc2.far = Math.max(chordLen * 1.1, 20);
                                const allHits = rc2.intersectObject(armorModel, true)
                                    .filter(h => h.object.userData.armorSection !== 'deco');
                                let iHits = allHits;
                                let partConstrained = false;
                                if (sPart != null) {
                                    const inPart = allHits.filter(h => partOf(h.object) === sPart);
                                    if (inPart.length) { iHits = inPart; partConstrained = true; }
                                }
                                if (iHits.length > 0) {
                                    const imk = new THREE.Mesh(new THREE.SphereGeometry(0.11, 12, 10),
                                        new THREE.MeshBasicMaterial({ color: 0xff2222, transparent: true, opacity: 0.95, depthTest: false }));
                                    imk.position.copy(iHits[0].point);
                                    imk.renderOrder = 998;
                                    window.__worldAnno.add(imk);
                                    window.__worldImpactMk = imk;
                                    // 弹着点坐标（回放世界系）与部件约束写入调试信息窗口
                                    dbgInfo('dbg-impact', '#ff2222', '弹着点',
                                        fmt3(iHits[0].point.x + cx, iHits[0].point.y + cy, iHits[0].point.z + cz)
                                        + (partConstrained ? ' · 部件P' + sPart : ''));
                                } else {
                                    console.warn('[world] impact raycast: 0 hits（弹道未交装甲——几何/位姿错位）');
                                }
                                // ④' DecodeShotSegment 出入点标注 + 判定射线。
                                // hash6 = 游戏客户端 DecodeShotSegment 两点编码：服务器命中判定
                                // 时刻的入点 P1/出点 P2，部件 AABB 1/255 量化，轴序 x右←b2/b5、
                                // y前←b4/b7、z高←b3/b6（b4 恒 255 = 入点钉盒前界面）。
                                // 盒源：game_data collision.*_bbox（游戏原生部件盒，x右/y前/z上）
                                // 优先，缺失回退装甲网格 rest 顶点级紧致盒；部件帧：hull/chassis=
                                // 模型原点、turret/gun=枢轴系。判定射线 __segRay = {P1−0.5·方向,
                                // P1→P2 解码弦}：raycast 与入射角同源此射线（服务器编码的位置
                                // 与弹向），世界系，随位姿联动。
                                window.__worldSegMk = null;
                                // 判定段线组：独立于调试标注层（非调试模式也常显）
                                if (window.__worldSegGroup) { scene.remove(window.__worldSegGroup); window.__worldSegGroup = null; }
                                if (s.hit_token && /^[0-9a-f]{12}$/i.test(s.hit_token)) {
                                    const hb = [];
                                    for (let hi = 0; hi < 6; hi++) hb.push(parseInt(s.hit_token.substr(hi * 2, 2), 16));
                                    const partOfS = o => { const nm = o.name || '';
                                        if (/^turret_\d+_armor_/.test(nm)) return 2;
                                        if (/^gun_\d+_armor_/.test(nm)) return 3;
                                        const u = o.userData || {};
                                        const sec = u.armorSectionOrig || u.armorSection;
                                        return sec === 'chassis' ? 0 : sec === 'hull' ? 1 : (sec === 'turret' || sec === 'gun') ? 2 : sec === 'gunBarrel' ? 3 : -1; };
                                    const sPart2 = (typeof s.server_part_index === 'number') ? s.server_part_index : null;
                                    const partNodes = [];
                                    armorModel.traverse(function(n) {
                                        if (!n.isMesh) return;
                                        const u = n.userData || {};
                                        if (u.armorSection === 'deco') return;
                                        if (sPart2 != null && partOfS(n) !== sPart2) return;
                                        partNodes.push(n);
                                    });
                                    // part 0（底盘/履带）在装甲模型无网格——回退 hull 盒近似
                                    const partFallback = partNodes.length === 0 && sPart2 === 0;
                                    if (partFallback) {
                                        armorModel.traverse(function(n) {
                                            if (!n.isMesh) return;
                                            const u = n.userData || {};
                                            if (u.armorSection === 'deco') return;
                                            if (partOfS(n) === 1) partNodes.push(n);
                                        });
                                    }
                                    if (partNodes.length) {
                                        const cdBoxes = (tankData && tankData.collision_boxes) || null;
                                        const gameBoxRaw = cdBoxes ? (sPart2 === 0 ? cdBoxes.chassis
                                            : sPart2 === 1 ? cdBoxes.hull : sPart2 === 2 ? cdBoxes.turret
                                            : sPart2 === 3 ? cdBoxes.gun : null) : null;
                                        let boxMin = null, boxMax = null, boxFrame = null;
                                        let refNode = null, refRest = null;
                                        if (gameBoxRaw && gameBoxRaw.min && gameBoxRaw.max) {
                                            boxMin = gameBoxRaw.min; boxMax = gameBoxRaw.max;
                                            boxFrame = (sPart2 === 2) ? 'turret-pivot' : (sPart2 === 3) ? 'gun-pivot' : 'model';
                                        } else {
                                            // 回退：装甲网格 rest 顶点级紧致盒（旋转 AABB 会虚胀）
                                            const box = new THREE.Box3();
                                            const vv = new THREE.Vector3();
                                            for (const n of partNodes) {
                                                if (!n.geometry.boundingBox) n.geometry.computeBoundingBox();
                                                const rest = (armorOrigMatrices && armorOrigMatrices.get(n)) || n.matrix;
                                                const pos = n.geometry.attributes.position;
                                                for (let i = 0; i < pos.count; i++) {
                                                    vv.fromBufferAttribute(pos, i).applyMatrix4(rest);
                                                    box.expandByPoint(vv);
                                                }
                                                if (!refNode && armorOrigMatrices && armorOrigMatrices.has(n)) {
                                                    refNode = n; refRest = armorOrigMatrices.get(n);
                                                }
                                            }
                                            boxMin = box.min.toArray(); boxMax = box.max.toArray();
                                            boxFrame = 'mesh-tight';
                                        }
                                        const szv = [boxMax[0] - boxMin[0], boxMax[1] - boxMin[1], boxMax[2] - boxMin[2]];
                                        const qc = (b, ax) => boxMin[ax] + szv[ax] * (b / 255);
                                        const mkP1 = new THREE.Mesh(new THREE.OctahedronGeometry(0.12),
                                            new THREE.MeshBasicMaterial({ color: 0xffa500, transparent: true, opacity: 0.95, depthTest: false }));
                                        const mkP2 = new THREE.Mesh(new THREE.SphereGeometry(0.12, 12, 10),
                                            new THREE.MeshBasicMaterial({ color: 0x22ff88, wireframe: true, transparent: true, opacity: 0.95, depthTest: false }));
                                        const segLine = new THREE.Line(new THREE.BufferGeometry(),
                                            new THREE.LineDashedMaterial({ color: 0xffa500, transparent: true, opacity: 0.85, dashSize: 0.25, gapSize: 0.15, depthTest: false }));
                                        mkP1.renderOrder = 998; mkP2.renderOrder = 998; segLine.renderOrder = 997;
                                        window.__worldSegGroup = new THREE.Group();
                                        window.__worldSegGroup.add(mkP1);
                                        window.__worldSegGroup.add(mkP2);
                                        window.__worldSegGroup.add(segLine);
                                        scene.add(window.__worldSegGroup);
                                        const findPartMesh = function(re) {
                                            let found = null;
                                            armorModel.traverse(function(n) {
                                                if (!found && n.isMesh && re.test(n.name || '')) found = n;
                                            });
                                            return found;
                                        };
                                        // 量化三元组 [右,前,高] → 场景世界坐标。炮塔/炮管件经部件
                                        // 网格世界矩阵（局部系=枢轴系，矩阵由位姿链驱动 → 标记跟随
                                        // 转角；勿用 Rz(currentTurretDeg) 手动组合，初始位姿时它为 0）
                                        const placePt = function(q) {
                                            const pl = new THREE.Vector3(qc(q[0], 0), qc(q[1], 1), qc(q[2], 2));
                                            if (boxFrame === 'turret-pivot') {
                                                const tm = findPartMesh(/^turret_\d+_armor/);
                                                if (tm) { tm.updateWorldMatrix(true, false); return tm.localToWorld(pl); }
                                                const yaw = THREE.MathUtils.degToRad(currentTurretDeg);
                                                pl.applyMatrix4(new THREE.Matrix4().makeRotationZ(yaw)
                                                    .setPosition(armorPivotTurret.x, armorPivotTurret.y, armorPivotTurret.z));
                                                return armorModel.localToWorld(pl);
                                            }
                                            if (boxFrame === 'gun-pivot') {
                                                const gm = findPartMesh(/^gun_\d+_armor/);
                                                if (gm) { gm.updateWorldMatrix(true, false); return gm.localToWorld(pl); }
                                                const pitch = THREE.MathUtils.degToRad(currentGunDeg);
                                                pl.applyMatrix4(new THREE.Matrix4().makeRotationX(pitch));
                                                if (armorPivotGun && armorPivotTurret) {
                                                    pl.add(armorPivotGun.clone().sub(armorPivotTurret));
                                                    pl.applyMatrix4(new THREE.Matrix4().makeRotationZ(
                                                        THREE.MathUtils.degToRad(currentTurretDeg))
                                                        .setPosition(armorPivotTurret.x, armorPivotTurret.y, armorPivotTurret.z));
                                                }
                                                return armorModel.localToWorld(pl);
                                            }
                                            if (boxFrame === 'mesh-tight' && refNode && refRest) {
                                                return pl.applyMatrix4(refRest.clone().invert()).applyMatrix4(
                                                    refNode.matrixWorld.clone().multiply(refRest.clone().invert()));
                                            }
                                            return armorModel.localToWorld(pl);
                                        };
                                        const updSeg = function(visible) {
                                            // 出入点标记 + 段线（P1→P2 = 游戏编码命中线段）
                                            mkP1.position.copy(placePt([hb[0], hb[2], hb[1]]));
                                            mkP2.position.copy(placePt([hb[3], hb[5], hb[4]]));
                                            // 判定方向 = P1→P2 解码弦（世界系），经 placePt 同款部件矩阵
                                            const chordL = new THREE.Vector3(
                                                qc(hb[3], 0) - qc(hb[0], 0),
                                                qc(hb[5], 1) - qc(hb[2], 1),
                                                qc(hb[4], 2) - qc(hb[1], 2));
                                            let dirS = null;
                                            if (chordL.lengthSq() > 1e-9) {
                                                let m = null;
                                                if (boxFrame === 'turret-pivot') {
                                                    const tm = findPartMesh(/^turret_\d+_armor/);
                                                    if (tm) { tm.updateWorldMatrix(true, false); m = tm.matrixWorld; }
                                                } else if (boxFrame === 'gun-pivot') {
                                                    const gm = findPartMesh(/^gun_\d+_armor/);
                                                    if (gm) { gm.updateWorldMatrix(true, false); m = gm.matrixWorld; }
                                                } else if (boxFrame === 'mesh-tight' && refNode && refRest) {
                                                    m = refNode.matrixWorld.clone().multiply(refRest.clone().invert());
                                                } else {
                                                    m = armorModel.matrixWorld;
                                                }
                                                if (m) dirS = chordL.clone().transformDirection(m).normalize();
                                            }
                                            if (!dirS) {
                                                // P1==P2（点射终止，§2.1）：回退炮塔/模型反水平方向
                                                const e = (findPartMesh(/^turret_\d+_armor/) || armorModel).matrixWorld.elements;
                                                const fh = Math.hypot(e[4], e[6]) || 1;
                                                dirS = new THREE.Vector3(-e[4] / fh, 0, -e[6] / fh);
                                            }
                                            // 段线入射端"无限"延长（工程取 900m，相机远平面内）：
                                            // 沿判定弹向 dirS（世界系，与 __segRay 同源）的反向，
                                            // 入射弹向一目了然；P2（穿出端）与标记保持在解码出入点。
                                            // 勿用未变换的 chordL 方向——部件盒带姿态时外延会指向错误方向
                                            const EXT = 900;
                                            const aPt = mkP1.position.clone().addScaledVector(dirS, -EXT);
                                            const bPt = mkP2.position.clone();
                                            segLine.geometry.setFromPoints([aPt, bPt]);
                                            segLine.computeLineDistances();
                                            // 判定射线：P1 表面外 0.5m 沿弹向进入（raycast 与入射角同源）
                                            window.__segRay = {
                                                origin: mkP1.position.clone().addScaledVector(dirS, -0.5),
                                                dir: dirS, far: 26
                                            };
                                            // 弹着点红点 = P1（服务器编码真实入点）
                                            if (window.__worldImpactMk) {
                                                window.__worldImpactMk.position.copy(mkP1.position);
                                                if (visible) {
                                                    dbgInfo('dbg-impact', '#ff2222', '弹着点',
                                                        fmt3(mkP1.position.x + cx, mkP1.position.y + cy, mkP1.position.z + cz)
                                                        + ' · DecodeShotSegment P1' + (sPart2 != null ? ' · 部件P' + sPart2 : ''));
                                                }
                                            }
                                            if (visible) {
                                                dbgInfo('dbg-seg', '#ffa500', 'DecodeShotSegment',
                                                    'P1 ' + fmt3(mkP1.position.x + cx, mkP1.position.y + cy, mkP1.position.z + cz) +
                                                    ' · P2 ' + fmt3(mkP2.position.x + cx, mkP2.position.y + cy, mkP2.position.z + cz) +
                                                    ' · 俯仰' + (THREE.MathUtils.radToDeg(Math.asin(
                                                        chordL.clone().normalize().z)).toFixed(2)) + '°（炮塔系）' +
                                                    (boxFrame === 'mesh-tight' ? ' · 网格盒回退' : ' · 游戏部件盒'));
                                            }
                                        };
                                        window.__worldSegMk = { update: updSeg, p1: mkP1, p2: mkP2 };
                                        updSeg(!!window.__debugOn);
                                        // 立即以 P1→P2 射线重跑判定：armorModel 异步加载，seg 块晚于
                                        // 600ms 的弦判定计时器——以新基准覆盖其结果（penSeq 丢弃旧响应）
                                        if (window.__segRay && window.__worldPenMode) {
                                            __shotRayOrigin = ctxLaunch.clone();
                                            __shotRayTarget = ctxLaunch.clone().addScaledVector(ctxLvDir, ctxRayFar);
                                            doPenetrationCheck(0, 0);
                                            __shotRayOrigin = null; __shotRayTarget = null;
                                        }
                                        // 诊断挂钩：盒/量化字节（静止局部），供控制台校准轴序
                                        window.__segDiag = {
                                            hb: hb, sPart: sPart2, partFallback: partFallback,
                                            boxFrame: boxFrame,
                                            boxMin: boxMin.slice(0, 3), boxMax: boxMax.slice(0, 3),
                                            armorModel: armorModel
                                        };
                                        // debug=1 自动开启晚于本块——延迟补一次刷新（消除加载期位姿竞态残值）
                                        setTimeout(function() {
                                            if (!window.__worldSegMk) return;
                                            window.__worldSegMk.update(!!window.__debugOn);
                                            if (window.__worldImpactMk) {
                                                const pp = window.__worldSegMk.p1.position;
                                                window.__worldImpactMk.position.copy(pp);
                                                dbgInfo('dbg-impact', '#ff2222', '弹着点',
                                                    fmt3(pp.x + cx, pp.y + cy, pp.z + cz) + ' · DecodeShotSegment P1'
                                                    + (sPart2 != null ? ' · 部件P' + sPart2 : ''));
                                            }
                                        }, 1500);
                                    }
                                }
                            }
                            // ⑥ 游戏弹孔 = 服务器 segment（hash6）按 DecodeShotSegment 解码的
                            // 相机 = 炮口侧后上方 3/4 视角。纯第一人称与弹道共线——坦克与网格原点
                            // 重叠、tick 位移方向垂直于视线，空间关系不可判读；侧偏+上抬赋予视差。
                            const dirTraj = new THREE.Vector3(bb[0]-ba[0], 0, bb[2]-ba[2]);
                            const lenH = dirTraj.length();
                            if (lenH > 1) {
                                dirTraj.divideScalar(lenH);
                                const perp = new THREE.Vector3(-dirTraj.z, 0, dirTraj.x);
                                camera.position.copy(lpM.position)
                                    .addScaledVector(perp, Math.max(lenH * 0.3, 6))
                                    .add(new THREE.Vector3(0, Math.max(lenH * 0.22, 4), 0));
                            } else {
                                camera.position.copy(lpM.position).add(new THREE.Vector3(0, 6, 8));
                            }
                            controls.target.set((ba[0]+bb[0])/2 - cx, (ba[1]+bb[1])/2 - cy, (ba[2]+bb[2])/2 - cz);
                        } else {
                            // 无弹道数据回退：射手后上方看向两车中点
                            const camDist = Math.max(20, Math.sqrt(
                                (tPos[0]-shooterWorld[0])**2 + (tPos[2]-shooterWorld[2])**2) * 0.4);
                            const aimYaw = Math.atan2(tPos[0]-shooterWorld[0], tPos[2]-shooterWorld[2]);
                            camera.position.set(
                                shooterWorld[0] - cx + Math.sin(aimYaw) * camDist * 0.3,
                                shooterWorld[1] - cy + camDist * 0.3,
                                shooterWorld[2] - cz + Math.cos(aimYaw) * camDist * 0.3
                            );
                            controls.target.set(0, 1, 0);
                        }
                        controls.update();

                        scene.add(window.__worldAnno);

                        // ===== world 模式穿透判定（相对模式同源管线）=====
                        // 真实炮口射线 → doPenetrationCheck → /api/penetrate。worldPenMode 开启后
                        // check 内部把世界系交点/射线经 worldToLocal 换算成模型局部米制，结果与相对模式同源。
                        window.__worldPenMode = true;
                        window.__shotIsHit = true;   // 0 armor hits 时 doPenetrationCheck 走报错分支
                        window.__worldServerInfo = (function() {
                            return { cls: shotResultClass(s), result: s.game_hit_result };
                        })();
                        // 自动初始判定（用户要求锁定）：命中弹 = DecodeShotSegment
                        // P1→P2 射线（准确命中位置与弹向），脱靶弹 = 弹道弦；
                        // 结果锁定，点击不触发判定（onClick 拦截）。
                        if (ctxLvDir && ctxLaunch) {
                            __shotRayOrigin = ctxLaunch.clone();
                            __shotRayTarget = ctxLaunch.clone().addScaledVector(ctxLvDir, ctxRayFar);
                            setTimeout(() => {
                                doPenetrationCheck(0, 0);
                                __shotRayOrigin = null; __shotRayTarget = null;
                            }, 600);
                        } else {
                            __shotRayOrigin = null; __shotRayTarget = null;
                        }

                        // ===== tick 位移方向标注（默认开启，"移动方向"复选框可关）=====
                        // 车体底部平面（type10 高度 +0.1m）：青线 = 相邻 tick 位移路径；同一基点并排
                        // 两支箭头（消除透视视差）：青 = 位移方向，绿/橙/黄 = 履带朝向（按位移方向着色）。
                        // 三者均取真实 3D 方向（含俯仰）——只做水平投影会与倾斜模型出现视角差；
                        // 前进/倒车/转向分类仍用水平方位角（atan2(dx,dz) vs yaw）。
                        if (window.__moveAnno) { scene.remove(window.__moveAnno); }
                        window.__moveAnno = new THREE.Group();
                        const tksD = s.tick_samples || [];
                        const moveDirColor = (i) => {
                            if (i <= 0) return 0xffdd33;
                            const dx = tksD[i].pos[0]-tksD[i-1].pos[0], dz = tksD[i].pos[2]-tksD[i-1].pos[2];
                            if (Math.sqrt(dx*dx + dz*dz) < 0.05) return 0xffdd33;
                            let e = Math.abs(Math.atan2(dx, dz) - tksD[i].yaw) % (2*Math.PI);
                            if (e > Math.PI) e = 2*Math.PI - e;
                            const deg = e*180/Math.PI;
                            return deg < 60 ? 0x33ff66 : (deg > 120 ? 0xff6622 : 0xffdd33);
                        };
                        const mkMoveArrow = (len, color) => {
                            const arr = new THREE.ArrowHelper(new THREE.Vector3(0, 0, 1),
                                new THREE.Vector3(), len, color, 0.55, 0.3);
                            arr.line.material.depthTest = false;
                            arr.cone.material.depthTest = false;
                            arr.renderOrder = 996;
                            return arr;
                        };
                        window.__moveArrow = mkMoveArrow(3.0, 0xffdd33);        // 履带朝向
                        window.__moveVecArrow = mkMoveArrow(2.2, 0x00ccff);     // 位移方向
                        window.__moveAnno.add(window.__moveArrow);
                        window.__moveAnno.add(window.__moveVecArrow);
                        // 两支箭头跟随视觉模型：同基点（车底右侧 3m）并排、间隔 0.9m
                        window.updateMoveArrow = function(idx) {
                            if (!tksD[idx]) return;
                            const ts = tksD[idx];
                            const q = poseFromYPR(ts.yaw, ts.pitch, ts.roll);
                            const fwd = new THREE.Vector3(0, 1, 0).applyQuaternion(q);
                            if (fwd.lengthSq() < 0.01) return;
                            fwd.normalize();   // 真实 3D 履带朝向（含俯仰），与模型姿态一致
                            // 沿车体横向右移 ~3m：箭头放在视觉模型旁边而非车体内部
                            const side = new THREE.Vector3(1, 0, 0).applyQuaternion(q);
                            side.y = 0;
                            if (side.lengthSq() > 0.01) side.normalize();
                            // 场景为中心化坐标系（原点 = 双车中点）——位置必须减 cx/cy/cz
                            const base = new THREE.Vector3(
                                ts.pos[0]+tPos[0]-cx, ts.pos[1]+tPos[1]-cy+0.10, ts.pos[2]+tPos[2]-cz);
                            window.__moveArrow.position.copy(base).addScaledVector(side, 0.9);
                            window.__moveVecArrow.position.copy(base).addScaledVector(side, -0.9);
                            window.__moveArrow.setDirection(fwd);
                            window.__moveArrow.setColor(moveDirColor(idx));
                            // 位移方向箭头 = 进入当前 tick 的线段方向（真实 3D，含爬坡升降）
                            if (idx > 0) {
                                const p0 = tksD[idx-1];
                                const mv = new THREE.Vector3(
                                    ts.pos[0]-p0.pos[0], ts.pos[1]-p0.pos[1], ts.pos[2]-p0.pos[2]);
                                if (mv.lengthSq() > 0.0025) {
                                    window.__moveVecArrow.visible = true;
                                    window.__moveVecArrow.setDirection(mv.normalize());
                                } else {
                                    window.__moveVecArrow.visible = false;
                                }
                            } else {
                                window.__moveVecArrow.visible = false;
                            }
                        };
                        tksD.forEach((ts, i) => {
                            if (i === 0) return;
                            const p0 = tksD[i-1];
                            // 中心化坐标系（原点=双车中点）+ 真实 3D 路径（各点取自身 type10 高度）——坡地不失真
                            const geo = new THREE.BufferGeometry().setFromPoints([
                                new THREE.Vector3(p0.pos[0]+tPos[0]-cx, p0.pos[1]+tPos[1]-cy+0.10, p0.pos[2]+tPos[2]-cz),
                                new THREE.Vector3(ts.pos[0]+tPos[0]-cx, ts.pos[1]+tPos[1]-cy+0.10, ts.pos[2]+tPos[2]-cz),
                            ]);
                            const ln = new THREE.Line(geo, new THREE.LineBasicMaterial({
                                color: 0x00ccff, depthTest: false, transparent: true, opacity: 0.9 }));
                            ln.renderOrder = 995;
                            window.__moveAnno.add(ln);
                            const dx = ts.pos[0]-p0.pos[0], dz = ts.pos[2]-p0.pos[2];
                            if (Math.sqrt(dx*dx + dz*dz) >= 0.05) {
                                const dd = Math.sqrt(dx*dx + dz*dz);
                                const v = dd/Math.max(1e-3, ts.dt - p0.dt);
                                const mvAz = Math.atan2(dx, dz);
                                let e = Math.abs(mvAz - ts.yaw) % (2*Math.PI);
                                if (e > Math.PI) e = 2*Math.PI - e;
                                e = e*180/Math.PI;
                                console.log('[move] dt=%+f yaw=%s° move=%s° err=%s° v=%sm/s %s',
                                    ts.dt.toFixed(3), (ts.yaw*180/Math.PI).toFixed(1),
                                    (mvAz*180/Math.PI).toFixed(1), e.toFixed(1), v.toFixed(1),
                                    e < 60 ? '前进' : (e > 120 ? '倒车' : '转向'));
                            } else {
                                console.log('[move] dt=%+f yaw=%s° pivot/静止',
                                    ts.dt.toFixed(3), (ts.yaw*180/Math.PI).toFixed(1));
                            }
                        });
                        window.updateMoveArrow(tksD.length - 1);   // 初始 = 命中锚点 tick
                        scene.add(window.__moveAnno);
                        // 面板数字摘要：当前窗口内 位移vs朝向 偏差（中位/最大，排除倒车段）
                        {
                            const errs = [];
                            for (let i = 1; i < tksD.length; i++) {
                                const dx = tksD[i].pos[0]-tksD[i-1].pos[0], dz = tksD[i].pos[2]-tksD[i-1].pos[2];
                                if (Math.sqrt(dx*dx + dz*dz) < 0.05) continue;
                                let e = Math.abs(Math.atan2(dx, dz) - tksD[i].yaw) % (2*Math.PI);
                                if (e > Math.PI) e = 2*Math.PI - e;
                                errs.push(e*180/Math.PI);
                            }
                            errs.sort((a,b)=>a-b);
                            const el = document.getElementById('move-err');
                            if (el) el.textContent = errs.length
                                ? '位移vs朝向: 中位' + errs[errs.length>>1].toFixed(1) + '° 最大' + errs[errs.length-1].toFixed(1) + '° (' + errs.length + '段)'
                                : '';
                        }

                        // ===== 弹道判定上下文（精简）：弦起点/方向/长度供相对视角按钮、
                        // 滑块弹着点重算与弦判定重跑共用（原 tick 双下拉已移除，位姿调整
                        // 统一走连续时间滑块）=====
                        window.__worldTickCtx = {
                            launch: ctxLaunch, lvDir: ctxLvDir, lvSpd: ctxLvSpd, rayFar: ctxRayFar,
                        };
                        // 进入射击复现默认相对视角（相机沿入射方向回退看向命中点）；
                        // armorModel 未就绪时留待后续 tick 重试
                        if (window.__autoRelView) {
                            if (armorModel) {
                                window.__autoRelView = false;
                                const rb = document.getElementById('rel-view-toggle');
                                if (rb) rb.click();
                            }
                        }

                        const st = document.getElementById('turret-controls');
                        const cls2 = shotResultClass(s) === 'MISS' ? 'MISS'
                            : (shotResultClass(s) === 'RICOCHET') ? 'RICO'
                            : (shotResultClass(s) === 'PENETRATION') ? 'PEN'
                            : (shotResultClass(s) === 'HE BLAST') ? 'HE' : 'NOPEN';
                        const qIssues = srQualityIssues(s);
                        const tksW = s.tick_samples || [];
                        if (tksW.length > 1) {
                            // 弹种自动匹配：按回放数据推断本发实际弹种并切换选择器（修正
                            // BLOCKED vs SPLASH 类差异）。优先级：① hit_flags 0x1000(HE 爆炸)
                            // → explosion_radius>0 的弹；② shell_id ↔ 射手配置弹表
                            // shell_global_ids 精确匹配（确定性别名，FV215b shot6 竞态实测）；
                            // ③ 仅当 shell_id 未知时才槽位兜底（有 shell_id 而匹配失败 = 数据
                            // 不全，保持当前选择，不回退槽位以免盖掉正确弹种）。
                            window.__worldShellSlot = (typeof s.shell_slot === 'number') ? s.shell_slot : null;
                            window.__worldIsHE = !!(s.hit_flags & 0x1000);
                            window.__worldShellId = (typeof s.shell_id === 'number' && s.shell_id) ? s.shell_id : null;
                            setTimeout(function() {
                                const sel = document.getElementById('shell-select');
                                if (!sel) return;
                                let want = null;
                                if (window.__worldIsHE) {
                                    want = (shooterShells || []).findIndex(sh =>
                                        shellTypeOf(sh) === 'he' || (sh && sh.explosion_radius > 0));
                                }
                                if (want == null || want < 0 && window.__worldShellId) {
                                    // 射手弹表全局 id：configs[射手配置].shell_global_ids（与顶层
                                    // shells 同源同序）；全配置扫一遍取首个包含该弹的
                                    const cfgArr = (shooterData && shooterData.configs) || null;
                                    let gids = null;
                                    if (cfgArr) {
                                        for (let ci = cfgArr.length - 1; ci >= 0 && !gids; ci--) {
                                            gids = cfgArr[ci].shell_global_ids || null;
                                        }
                                    }
                                    if (gids) {
                                        want = gids.indexOf(window.__worldShellId);
                                        if (want != null && want < 0) want = null;
                                    }
                                }
                                // 槽位兜底仅在完全无 shell_id 时使用
                                if ((want == null || want < 0) && window.__worldShellId == null) {
                                    want = window.__worldShellSlot;
                                }
                                if (want != null && want >= 0 && want < sel.options.length) {
                                    if (sel.value !== String(want)) {
                                        sel.value = String(want);
                                        sel.dispatchEvent(new Event('change'));
                                    }
                                }
                            }, 800);
                        }
                        if (st) { st.innerHTML = '<div class="ctrl-row"><b>World View — Shot #' + s.index + '</b></div>' +
                            (function() {
                                const t4 = s.is_author ? '作者' : (s.shooter_team === 'enemy' ? '敌方' : (s.shooter_team === 'ally' ? '我方' : ''));
                                return '<div class="ctrl-row">Shooter: <b style="color:#ffcf5c;">' + (s.shooter_name || '—') + '</b>' + (t4 ? ' · ' + t4 : '') + '</div>';
                            })() +
                            '<div class="ctrl-row">DMG ' + s.damage + ' · ' + cls2 + ' · ' + (s.target_name || '—') + '</div>' +
                            '<div class="ctrl-row" id="world-pen-cmp" style="font-size:10px;"></div>' +
                            (function() {
                                const sp3 = shellIdParts(s.shell_id);
                                if (!sp3) return '';
                                return '<div class="ctrl-row" style="font-size:10px;color:#8ab4ff;">弹种: shell_id=' + s.shell_id +
                                    ' (局部' + sp3.local + ' · 国家0x' + sp3.nation.toString(16) + ')' +
                                    (s.shell_kind ? ' · <b>' + s.shell_kind + '</b>' : '') +
                                    ((s.quality && s.quality.shell_from_broadcast) ? ' · 来源0x07广播' : '') +
                                    ((s.quality && s.quality.shell_from_terrain) ? ' · 来源0x1b广播' : '') +
                                    (s.segment ? ' · 装甲组=' + (s.armor_group || '—') : '') + '</div>';
                            })() +
                            (function() {
                                const cr3 = decodeModules(s.crit_modules || 0);
                                const ds3 = decodeModules(s.destroyed_modules || 0);
                                if (!cr3.length && !ds3.length) return '';
                                return '<div class="ctrl-row" style="font-size:10px;color:#ffcf5c;">模块: ' +
                                    cr3.concat(ds3.map(n3 => n3 + '(摧毁)')).join(' · ') + '</div>';
                            })() +
                            (s.shooter_aim ? '<div class="ctrl-row" style="font-size:10px;color:#888;">瞄准: 炮塔偏航 ' +
                                s.shooter_aim.turret_rel_yaw.toFixed(4) + ' rad' +
                                (typeof s.shooter_aim.state_before === 'number'
                                    ? ' · 状态 ' + s.shooter_aim.state_before.toFixed(3) + '→' +
                                      (s.shooter_aim.state_after != null ? s.shooter_aim.state_after.toFixed(3) : '—')
                                    : '') + '</div>' : '') +
                            (s.server_part_index != null ? '<div class="ctrl-row" style="font-size:10px;color:#ff8866;">服务器部件: ' +
                                s.server_part_index + '（0=底盘/履带 1=车体 2=炮塔 3=炮管）· 弹着点已按部件约束</div>' : '') +
                            (s.target_render ? '<div class="ctrl-row" style="font-size:10px;color:#00aaff;">渲染锚点(游戏画面): 滞后 ' +
                                s.target_render.latency.toFixed(3) + 's · 渲染-判定偏差 ' +
                                s.target_render.dist_to_judgment.toFixed(2) + 'm</div>' : '') +
                            // ===== 连续时间滑块（拖动平滑控制双模型位置/姿态） =====
                            (function() {
                                // 滑块范围纳入滤波时间线（−3~+2s 扩展窗口，含命中后）
                                const tlDt = (s.target_render_timeline || []).map(t => t.dt);
                                const sTlDt = (s.shooter_render_timeline || []).map(t => t.dt);
                                const allDt = [].concat(
                                    tksW.filter(t => !t.render).map(t => t.dt),
                                    sTS.filter(t => !t.render).map(t => t.dt),
                                    tlDt, sTlDt
                                ).filter(d => !isNaN(d));
                                if (allDt.length < 2) return '';
                                const tMin = Math.min.apply(null, allDt);
                                const tMax = Math.max.apply(null, allDt);
                                if (tMax - tMin < 0.1) return '';
                                return '<div class="ctrl-row" style="margin-top:6px;gap:6px;">'
                                    + '<span style="font-size:10px;color:#cc66ff;flex:none;">⏱</span>'
                                    + '<input type="range" id="time-scrub" min="' + (tMin*1000).toFixed(0) + '" max="' + (tMax*1000).toFixed(0) + '" value="0" step="10"'
                                    + ' style="flex:1;min-width:0;accent-color:#cc66ff;">'
                                    + '<span id="time-scrub-label" style="font-size:10px;color:#cc66ff;flex:none;min-width:50px;text-align:right;"></span>'
                                    + '</div>';
                            })() +
                            (qIssues.length ? '<div class="ctrl-row" style="font-size:10px;color:#ffcf5c;">⚠ '
                                + qIssues.join(' · ') + '</div>' : '') +
                            '<div class="ctrl-row" style="font-size:10px;color:#888;">' +
                            '<span style="color:#ff6622;">●</span> LaunchPoint <span style="color:#00ccff;">┄</span> 速度向量 ' +
                            '<span style="color:#00ff00;">—</span> 弹道弦 <span style="color:#ff2222;">●</span> 弹着点 ' +
                            '<span style="color:#ffcc00;">●</span> 服务器终点 ' +
                            '<span style="color:#cc66ff;">●</span> 炮闩(发射起点) <span style="color:#fff;">┄</span> 偏差线</div>' +
                            '<div class="ctrl-row" style="font-size:10px;color:#888;">' +
                            '<span style="color:#33ff66;">↑</span>履带朝向 <span style="color:#00ccff;">↑</span>位移方向 ' +
                            '<span style="color:#ff6622;">↑</span>倒车(橙) <span style="color:#ffdd33;">↑</span>转向(黄) ' +
                            '<span style="color:#00ccff;">—</span>tick路径(3D)</div>' +
                            '<div class="ctrl-row" style="font-size:10px;color:#888;">' +
                            '<span style="color:#00ffcc;">●</span>基准点·受击(type10锚) ' +
                            '<span style="color:#00aaff;">●</span>基准点·射手(type10锚)</div>' +
                            '<div class="ctrl-row"><label style="font-size:11px;cursor:pointer;">' +
                            '<input type="checkbox" id="move-toggle" checked> 移动方向标注</label>' +
                            '<span id="move-err" style="font-size:10px;color:#8ab4ff;margin-left:8px;"></span></div>'; }
                        const mt2 = document.getElementById('move-toggle');
                        if (mt2) mt2.onchange = function() {
                            if (window.__moveAnno) window.__moveAnno.visible = this.checked;
                        };
                        // ===== 调试模式开关（默认关闭）：收纳 World View 面板 + 全部调试标注 =====
                        // 可见性集中挂 window.__debugSetVisible。
                        const stEl = document.getElementById('turret-controls');
                        if (stEl) stEl.style.display = 'none';
                        const dbgBtn = makeDebugToggle(function() {
                            // 基准点标注可见性（位置由 __updateAnchorMarkers 维护）
                            if (window.__updateAnchorMarkers) window.__updateAnchorMarkers();
                        });
                        // 相对视角按钮：相机 = 当前位姿的弦命中点沿入射反方向回退 15m（与
                        // showTrajectory 轨迹原点回退距离同语义），看向命中点；弦未命中当前位姿
                        // 时看向车体中心。再点一次恢复世界全景。判定/标注不受影响（弦判定与相机无关）。
                        let __relViewOn = false, __savedCam = null;
                        const relBtn = document.createElement('button');
                        relBtn.id = 'rel-view-toggle';
                        relBtn.textContent = '相对视角';
                        relBtn.style.cssText = 'padding:6px 14px;background:var(--panel);color:var(--accent);' +
                            'border:1px solid var(--border);border-radius:var(--radius-sm);' +
                            'font-size:0.85em;cursor:pointer;backdrop-filter:blur(12px);';
                        relBtn.onclick = function() {
                            if (!__relViewOn) {
                                __savedCam = { pos: camera.position.clone(), target: controls.target.clone() };
                                const ctx = window.__worldTickCtx;
                                if (ctx && ctx.launch && ctx.lvDir) {
                                    const dir = ctx.lvDir.clone().normalize();
                                    let aim = tankModel.position.clone();
                                    const rc = new THREE.Raycaster(ctx.launch.clone(), dir);
                                    rc.far = ctx.rayFar;
                                    const hits = rc.intersectObject(armorModel, true)
                                        .filter(h => h.object.userData.armorSection !== 'deco');
                                    if (hits.length) aim = hits[0].point.clone();
                                    camera.position.copy(aim.clone().addScaledVector(dir, -15));
                                    controls.target.copy(aim);
                                    controls.update();
                                }
                                __relViewOn = true;
                                this.textContent = '世界视角';
                            } else {
                                if (__savedCam) {
                                    camera.position.copy(__savedCam.pos);
                                    controls.target.copy(__savedCam.target);
                                    controls.update();
                                }
                                __relViewOn = false;
                                this.textContent = '相对视角';
                            }
                        };
                        (document.getElementById('corner-br') || document.body).appendChild(relBtn);
                        // 初始即按撤回状态隐藏全部标注（须在 debug=1 自动开启之前，否则被覆盖）
                        window.__debugSetVisible(false);
                        // URL debug=1 自动开启调试标注（与开关按钮同一状态,可再手动关闭）
                        if (QP.get('debug') === '1') {
                            window.__debugSetVisible(true);
                            dbgBtn.textContent = '隐藏调试标注';
                        }
                        // ===== 时间滑块：连续插值双模型位置/姿态（报告 §4.7 渲染层锚点可视化） =====
                        const timeSlider = document.getElementById('time-scrub');
                        if (timeSlider) {
                            // 滑块 0 点 = 渲染位（玩家所见）：存在渲染位采样时，用它替换
                            // dt≈0 的原始锚点采样作为插值关键帧——否则拖动滑块回 0 会回到
                            // 判定层锚点位姿，与初始的渲染位姿相差一个滤波滞后位移。
                            const rSample = (s.tick_samples || []).find(function(t) { return t.render; });
                            // 剔除命中后（dt>0）的兜底采样：数据包源切换致瞬移/朝向跳变
                            //（96 段实测 40 段反转），混入滑块末端 keyframe 会产生翻转行为
                            let vRaw = (s.tick_samples || []).filter(function(t) {
                                return !t.render && t.dt <= 0.001 && Math.abs(t.dt) > 0.025; });
                            if (rSample) {
                                vRaw.push({ dt: rSample.dt, pos: rSample.pos, yaw: rSample.yaw,
                                    pitch: rSample.pitch, roll: rSample.roll, render: true });
                                vRaw.sort(function(a, b) { return a.dt - b.dt; });
                            }
                            const sRaw = (s.shooter_tick_samples || []).filter(function(t) { return !t.render; });
                            const lerpAngle = function(a, b, t) {
                                let d = b - a;
                                while (d > Math.PI) d -= 2 * Math.PI;
                                while (d < -Math.PI) d += 2 * Math.PI;
                                return a + d * t;
                            };
                            const interpTick = function(samples, dt) {
                                if (!samples || !samples.length) return null;
                                for (let i = 0; i < samples.length - 1; i++) {
                                    const a = samples[i], b = samples[i + 1];
                                    if (a.dt <= dt && b.dt >= dt) {
                                        const span = b.dt - a.dt;
                                        const f = span > 1e-6 ? (dt - a.dt) / span : 0;
                                        let dy = b.yaw - a.yaw;
                                        while (dy > Math.PI) dy -= 2 * Math.PI;
                                        while (dy < -Math.PI) dy += 2 * Math.PI;
                                        return {
                                            pos: [0, 1, 2].map(function(k) { return a.pos[k] + (b.pos[k] - a.pos[k]) * f; }),
                                            yaw: a.yaw + dy * f,
                                            pitch: a.pitch + (b.pitch - a.pitch) * f,
                                            roll: a.roll + (b.roll - a.roll) * f
                                        };
                                    }
                                }
                                return dt <= samples[0].dt ? samples[0] : samples[samples.length - 1];
                            };
                            // 滤波时间线（渲染层严格对齐数据源）；null = 无时间线（回退原始采样）
                            const tlT = (s.target_render_timeline && s.target_render_timeline.length > 1) ? s.target_render_timeline : null;
                            const tlS = (s.shooter_render_timeline && s.shooter_render_timeline.length > 1) ? s.shooter_render_timeline : null;
                            // 炮塔相对角：镜像系下取反（相对角 = 炮塔世界角 − 车体角，镜像时两者同时
                            // 取反 → 相对角取反；直接透传会导致炮管指向镜像侧 = 方向反转 180°）
                            if (tlT) for (const t of tlT) t[1] = -t[1];
                            if (tlS) for (const t of tlS) t[1] = -t[1];
                            // 炮塔相对角时间线：镜像系下需取反（车体/炮塔世界 yaw 在镜像系同时翻转，
                            // 相对角 = −原始值；直接透传会导致炮管指向镜像侧 = 方向反转）
                            timeSlider.oninput = function() {
                                const dt = parseInt(this.value, 10) / 1000;
                                const lbl = document.getElementById('time-scrub-label');
                                if (lbl) lbl.textContent = (dt >= 0 ? '+' : '') + dt.toFixed(2) + 's';
                                // 覆盖检查：滤波时间线未覆盖的时段 = 该实体在录像客户端无 volatile
                                // 数据（AoI 外不渲染）——滑块拖入该区间时隐藏模型与附属标记
                                const inCovT = !tlT || (dt >= tlT[0].dt - 0.001 && dt <= tlT[tlT.length - 1].dt + 0.001);
                                const inCovS = !tlS || (dt >= tlS[0].dt - 0.001 && dt <= tlS[tlS.length - 1].dt + 0.001);
                                // 插值受击方：优先滤波时间线（严格对齐游戏每帧实际显示位姿——
                                // 含 latency 移位/误差盒钳位/外推，逆向报告六轮），回退原始采样线性插值。
                                // 时间线 pos = 绝对世界坐标；原始采样 pos = 相对锚点（需 +tPos）
                                const useTlT = !!tlT;
                                const vInt = interpTick(useTlT ? tlT : vRaw, dt);
                                if (vInt && tankModel) {
                                    const wp = useTlT
                                        ? [vInt.pos[0] - cx, vInt.pos[1] - cy, vInt.pos[2] - cz]
                                        : [vInt.pos[0] + tPos[0] - cx, vInt.pos[1] + tPos[1] - cy, vInt.pos[2] + tPos[2] - cz];
                                    tankModel.position.set(wp[0], wp[1], wp[2]);
                                    tankModel.quaternion.copy(poseFromYPR(vInt.yaw, vInt.pitch, vInt.roll));
                                    tankModel.updateMatrixWorld(true);
                                    if (armorModel) {
                                        armorModel.position.copy(tankModel.position);
                                        armorModel.quaternion.copy(tankModel.quaternion);
                                        armorModel.updateMatrixWorld(true);
                                    }
                                    tankModel.visible = inCovT;
                                    if (armorModel) armorModel.visible = inCovT;
                                }
                                // 插值射手方：优先滤波时间线（同受击方）。
                                // 时间线/原始采样的 pos 均为绝对世界坐标——放置逻辑完全同构
                                const sInt = interpTick(tlS ? tlS : sRaw, dt);
                                if (sInt && window.__shooterModel) {
                                    window.__shooterModel.position.set(
                                        sInt.pos[0] - cx, sInt.pos[1] - cy, sInt.pos[2] - cz);
                                    window.__shooterModel.quaternion.copy(poseFromYPR(sInt.yaw, sInt.pitch, sInt.roll));
                                    window.__shooterModel.updateMatrixWorld(true);
                                }
                                // 炮塔/炮管实时：prop2（炮塔相对角）与 prop9（俯仰）时间线插值
                                const lerpTl = (tl, dt2) => {
                                    if (!tl || tl.length < 2) return null;
                                    for (let i = 0; i < tl.length - 1; i++) {
                                        const a = tl[i], b = tl[i + 1];
                                        if (a[0] <= dt2 && b[0] >= dt2) {
                                            const f = (b[0] - a[0]) > 1e-6 ? (dt2 - a[0]) / (b[0] - a[0]) : 0;
                                            return a[1] + (b[1] - a[1]) * f;
                                        }
                                    }
                                    return dt2 <= tl[0][0] ? tl[0][1] : tl[tl.length - 1][1];
                                };
                                // 角度版：跨 ±π 时按短弧插值（与客户端 0x1441e00 短弧归一化一致）
                                const lerpTlAngle = (tl, dt2) => {
                                    if (!tl || tl.length < 2) return null;
                                    for (let i = 0; i < tl.length - 1; i++) {
                                        const a = tl[i], b = tl[i + 1];
                                        if (a[0] <= dt2 && b[0] >= dt2) {
                                            const f = (b[0] - a[0]) > 1e-6 ? (dt2 - a[0]) / (b[0] - a[0]) : 0;
                                            let d = b[1] - a[1];
                                            while (d > Math.PI) d -= 2 * Math.PI;
                                            while (d < -Math.PI) d += 2 * Math.PI;
                                            return a[1] + d * f;
                                        }
                                    }
                                    return dt2 <= tl[0][0] ? tl[0][1] : tl[tl.length - 1][1];
                                };
                                // 受击方炮塔实时：prop2 相对角直接驱动炮塔/炮管节点。
                                // 时间线存游戏系相对角；镜像场景节点旋转角须取负（镜像翻转
                                // 旋转方向）——与初始摆放一致：turretDegT = 镜像炮塔角−镜像车体角 = −rel
                                // 炮管俯仰：target_gun_timeline（prop2 frac 解码，弧度）插值，
                                // 缺时间线时回落命中时刻静态值 __fireGunDegT
                                const relT = lerpTlAngle(s.target_turret_timeline, dt);
                                if (relT != null) {
                                    let deg = -relT * 180 / Math.PI;
                                    deg = ((deg + 180) % 360 + 360) % 360 - 180;
                                    const gpT = lerpTl(s.target_gun_timeline, dt);
                                    updateTurretGun(deg, gpT != null ? gpT * 180 / Math.PI
                                        : (window.__fireGunDegT != null ? window.__fireGunDegT : 0));
                                }
                                // 射手方炮塔实时：炮塔世界角 = 车体 yaw − prop2 相对角
                                //（内部 tr = 炮塔角−车体角 = −rel，同初始摆放的镜像系约定）
                                const relS = lerpTlAngle(s.shooter_turret_timeline, dt);
                                if (sInt && window.__shooterModel && window.__shooterData) {
                                    // 射手炮塔实时：用射手自身的车体位姿（sInt），非受击方的 vInt
                                    // 炮管相对俯仰：shooter_gun_timeline（prop2 frac 解码）插值——
                                    // 与受击方同源同锚定（车体俯仰由 sInt.pitch 时序承载，
                                    // 两者是不同自由度）。无时间线（无锚定/回退）时传 null →
                                    // 弹速反解（method29 弹速仅开火帧存在，窗口内恒定保持）。
                                    const gpS = (s.quality && s.quality.gun_pitch_degraded &&
                                        s.quality.gun_pitch_degraded.indexOf('shooter') >= 0)
                                        ? null : lerpTl(s.shooter_gun_timeline, dt);
                                    poseShooterTurretGun(window.__shooterModel, window.__shooterData,
                                        sInt.yaw - (relS || 0), s.launch_velocity, sInt.yaw, sInt.pitch, sInt.roll,
                                        gpS != null ? gpS : null, -(relS || 0), s.shooter_config_idx);
                                }
                                if (window.__updateMuzzleMarker) window.__updateMuzzleMarker();
                                if (window.__updateAnchorMarkers) window.__updateAnchorMarkers();
                                if (window.__victimAnchor) window.__victimAnchor.mk.visible = inCovT && window.__debugOn;
                                if (window.__shooterAnchor) window.__shooterAnchor.mk.visible = inCovS && window.__debugOn;
                                if (window.__shooterMuzzleMk) window.__shooterMuzzleMk.visible = inCovS && window.__debugOn;
                                if (window.__shooterMuzzleLine) window.__shooterMuzzleLine.visible = inCovS && window.__debugOn;
                                if (window.__updateMuzzleMarker) window.__updateMuzzleMarker();
                                if (window.__updateAnchorMarkers) window.__updateAnchorMarkers();
                                // 弹着点红点随当前位姿重算（部件约束与初始摆放一致）
                                if (window.__worldTickCtx && window.__worldImpactMk && armorModel && window.__worldTickCtx.lvDir) {
                                    if (window.__worldSegMk && window.__worldSegMk.p1) {
                                        // 判定基准 = WI 同构射线（§七补7；update 内随位姿重建
                                        // 射线并重求交），红点贴射线入点（无交点回退解码点）
                                        window.__worldSegMk.update(!!window.__debugOn && inCovT);
                                        const showSeg = window.__debugOn && inCovT;
                                        window.__worldImpactMk.visible = showSeg;
                                        const hitPt = window.__worldSegMk.p1.position;
                                        window.__worldImpactMk.position.copy(hitPt);
                                        if (showSeg) {
                                            dbgInfo('dbg-impact', '#ff2222', '弹着点',
                                                fmt3(hitPt.x + cx, hitPt.y + cy, hitPt.z + cz)
                                                + ' · DecodeShotSegment P1'
                                                + (typeof s.server_part_index === 'number' ? ' · 部件P' + s.server_part_index : ''));
                                        }
                                    } else {
                                    const wc = window.__worldTickCtx;
                                    const rc2 = new THREE.Raycaster();
                                    rc2.set(wc.launch.clone(), wc.lvDir);
                                    rc2.far = wc.rayFar;
                                    const sPart = (typeof s.server_part_index === 'number') ? s.server_part_index : null;
                                    const partOf = o => { const u = o.userData || {}; const sec = u.armorSectionOrig || u.armorSection;
                                        return sec === 'chassis' ? 0 : sec === 'hull' ? 1 : (sec === 'turret' || sec === 'gun') ? 2 : sec === 'gunBarrel' ? 3 : -1; };
                                    let iHits = rc2.intersectObject(armorModel, true)
                                        .filter(h => h.object.userData.armorSection !== 'deco');
                                    if (sPart != null) {
                                        const inPart = iHits.filter(h => partOf(h.object) === sPart);
                                        if (inPart.length) iHits = inPart;
                                    }
                                    const show = iHits.length > 0 && window.__debugOn && inCovT;
                                    window.__worldImpactMk.visible = show;
                                    if (show) {
                                        window.__worldImpactMk.position.copy(iHits[0].point);
                                        dbgInfo('dbg-impact', '#ff2222', '弹着点',
                                            fmt3(iHits[0].point.x + cx, iHits[0].point.y + cy, iHits[0].point.z + cz)
                                            + (sPart != null && iHits.some(h => partOf(h.object) === sPart) ? ' · 部件P' + sPart : ''));
                                    } else {
                                        dbgInfo('dbg-impact', '#ff2222', '弹着点', '（射线无交点）');
                                    }
                                    }
                                }
                                // DecodeShotSegment P1/P2 随位姿重摆（炮塔/炮管部件跟随旋转）
                                if (window.__worldSegMk && armorModel) {
                                    armorModel.updateWorldMatrix(true, false);
                                    window.__worldSegMk.update(!!window.__debugOn && inCovT);
                                }
                                // 弦判定实时重跑（节流尾随 100ms）：滑块连续拖动时模型位姿每刻
                                // 不同，判定/弹道/对比面板若不跟随重算就停留在上次的结果，
                                // 与弹着点红点（上方 raycast）不一致。射线参数 = ctx 的
                                // （launch + 弦向量），判定经 doPenetrationCheck → /api/penetrate，
                                // 过期响应由 __penCheckSeq 丢弃，上屏结果恒对应当前位姿。
                                if (window.__worldTickCtx && window.__worldTickCtx.lvDir && window.__worldPenMode) {
                                    if (timeSlider.__penTimer) clearTimeout(timeSlider.__penTimer);
                                    timeSlider.__penTimer = setTimeout(function() {
                                        timeSlider.__penTimer = null;
                                        const wc2 = window.__worldTickCtx;
                                        if (!wc2 || !wc2.lvDir || !armorModel) return;
                                        __shotRayOrigin = wc2.launch.clone();
                                        __shotRayTarget = wc2.launch.clone().addScaledVector(wc2.lvDir, wc2.rayFar);
                                        doPenetrationCheck(0, 0);
                                        __shotRayOrigin = null; __shotRayTarget = null;
                                    }, 100);
                                }
                            };
                        }
                        return;
                    }
                }).catch(e => showShotError('射击复现数据加载失败: ' + e));
            }
            if (QP.get('clean') === '1' && !isShotReplay) {
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

        // 当前配置的炮盾(mask)有无：BlitzKit 真值语义 mask=0 视同无 mask
        const hasMaskValue = () => {
            const cfg = currentConfig();
            return !!(cfg && typeof cfg.gun_mask === 'number' && cfg.gun_mask !== 0);
        };

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

        function applyConfig(idx) {
            currentConfigIdx = idx;
            const cfg = currentConfig();
            if (!cfg) return;
            document.getElementById('config-select').value = String(idx);

            collectConfigNodes(tankModel);
            applyConfigVisible(tankModel, cfg.gun_index, cfg.turret_index);
            applyArmorConfigVisible(cfg);
            retagSpacedSections();
            alignArmorModules();
            origMatrices = null;
            armorOrigMatrices = null;
            collectTurretGunNodes();
            collectArmorNodes();
            updateTurretGun(currentTurretDeg, currentGunDeg);

            if (shooterData && tankData && shooterData.tank_id === tankData.tank_id) {
                shooterShells = cfg.shells || [];
                shooterCaliber = cfg.caliber || shooterCaliber;
                populateShellSelector(shooterShells);
                selectedShell = shooterShells.length ? shooterShells[0] : null;
            }

            if (penetrationMode && armorModel) rebuildHeatmapScenes();
        }

        function applyArmorConfigVisible(cfg) {
            if (!armorModel) { return; }
            const pk = collectArmorPrefixes();
            const gunPk = pk.gun, turPk = pk.turret;
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
            shooterShells = shells.map(s => (s && s.caliber == null)
                ? Object.assign({}, s, { caliber: caliber }) : s);
            shooterCaliber = caliber;
            populateShellSelector(shooterShells);
            const si = parseInt(QP.get('shell'), 10);
            if (!isNaN(si) && si >= 0 && si < shooterShells.length) {
                const sel = document.getElementById('shell-select');
                if (sel) sel.value = String(si);
                selectedShell = shooterShells[si];
            }
            // 切换射击坦克后同步热力图 uniforms（口径/穿深/跳弹角/转正角），否则判定沿用旧数据
            if (penetrationMode) {
                const sh = selectedShell || null;
                if (sh) { updatePenetrationUniforms(sh); updateSpacedUniforms(sh); }
            }
        }

        let tanksList = [];
        let currentShooterId = null, currentTargetId = null;
        let pickerMode = 'target'; // which selector the picker is currently editing

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
            renderer.localClippingEnabled = true;
            document.getElementById('canvas-container').appendChild(renderer.domElement);
            renderer.toneMapping = THREE.ACESFilmicToneMapping;
            renderer.toneMappingExposure = 1.15;

            controls = new OrbitControls(camera, renderer.domElement);
            controls.enableDamping = true;
            controls.dampingFactor = 0.05;
            controls.rotateSpeed = 0.35;   // 降低旋转灵敏度（默认 1.0 过快，精细对位困难）
            controls.minDistance = 3;
            controls.maxDistance = 30;
            controls.mouseButtons = {
                LEFT: THREE.MOUSE.ROTATE,
                MIDDLE: THREE.MOUSE.DOLLY,
                RIGHT: null,
            };

            const hemi = new THREE.HemisphereLight(0xffffff, 0x8a7f6e, 2.2);
            scene.add(hemi);
            scene.add(new THREE.AmbientLight(0xffffff, 0.9));
            const spot1 = new THREE.SpotLight(0xfff5e1, 500, 60, 0.55, 0.6);
            spot1.position.set(10, 15, 8);
            scene.add(spot1);
            const spot2 = new THREE.SpotLight(0x88aaff, 260, 50, 0.45, 0.5);
            spot2.position.set(-10, 12, -6);
            scene.add(spot2);

            const grid = new THREE.GridHelper(20, 20, 0x4a3a26, 0x2c241c);
            scene.add(grid);

            raycaster = new THREE.Raycaster();
            mouse = new THREE.Vector2();
        }

        function setupEventHandlers() {
            document.getElementById('shell-select').addEventListener('change', function() {
                const idx = parseInt(this.value);
                if (shooterShells && idx < shooterShells.length) {
                    selectedShell = shooterShells[idx];
                    if (penetrationMode) { updatePenetrationUniforms(selectedShell); updateSpacedUniforms(selectedShell); }
                    if (window.__worldPenMode && window.__worldTickCtx && window.__worldTickCtx.lvDir) {
                        // 世界模式：换弹按权威弦射线重跑判定——直接 doPenetrationCheck
                        // 会用相机射线覆盖锁定的复现结果
                        const wc3 = window.__worldTickCtx;
                        __shotRayOrigin = wc3.launch.clone();
                        __shotRayTarget = wc3.launch.clone().addScaledVector(wc3.lvDir, wc3.rayFar);
                        doPenetrationCheck(0, 0);
                        __shotRayOrigin = null; __shotRayTarget = null;
                    } else if (QP.get('shot')) {
                        doPenetrationCheck(0, 0);
                    }
                }
            });

            const onEquipmentChange = function() {
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

            document.getElementById('config-select').addEventListener('change', function() {
                applyConfig(parseInt(this.value));
            });

            document.getElementById('target-select').addEventListener('click', function() { openPicker('target'); });
            document.getElementById('shooter-select').addEventListener('click', function() { openPicker('shooter'); });

            document.getElementById('tp-close').addEventListener('click', closePicker);
            document.getElementById('tp-grid').addEventListener('click', function(e) {
            });
            document.getElementById('tp-search').addEventListener('input', renderGrid);
            document.getElementById('tp-tier').addEventListener('change', renderGrid);
            document.getElementById('tp-nation').addEventListener('change', renderGrid);
            document.getElementById('tp-type').addEventListener('change', renderGrid);
            document.getElementById('tank-picker').addEventListener('click', function(e) {
                if (e.target === this) closePicker();
            });

            document.getElementById('collision-btn').addEventListener('click', function() {
                collisionMode = !collisionMode;
                this.classList.toggle('active', collisionMode);
                this.textContent = collisionMode ? 'Hide Collision' : 'Show Collision';
                if (!armorModel) return;
                applyArmorViewStyle(collisionMode);
            });

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
                mouseDownPos = null;
                isDragging = false;
                onClick(e);
            });

            let rmbDown = false, rmbStartX = 0, rmbStartY = 0, rmbStartTurret = 0, rmbStartGun = 0;
            renderer.domElement.addEventListener('contextmenu', function(e) { e.preventDefault(); });
            renderer.domElement.addEventListener('mousedown', function(e) {
                if (e.button !== 2) return;
                if (window.__worldPan) return;   // 世界模式：右键留给 OrbitControls 平移
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
                const norm180 = (a) => ((a + 180) % 360 + 360) % 360 - 180;
                const yl = currentConfig()?.yaw_limits;
                const pl = currentConfig()?.pitch_limits;
                let yawDeg = rmbStartTurret + dx * 0.5;
                if (yl) {
                    if (yl.max - yl.min < 360) {
                        yawDeg = norm180(Math.max(-yl.max, Math.min(-yl.min, yawDeg)));
                    }
                } else {
                    const tLeft = tankData.turret_traverse_left ?? 180;
                    const tRight = tankData.turret_traverse_right ?? 180;
                    if (!(tLeft >= 180 && tRight >= 180)) {
                        yawDeg = Math.max(-tLeft, Math.min(tRight, yawDeg));
                    }
                }
                let pitchDeg = rmbStartGun - dy * 0.5;
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
        let gunBarrelNodes = [];   // 精确名 gun_XX = 炮管本体（随炮塔+俯仰）
        let gunMaskNodes = [];     // gun_XX_mask = 炮盾（只随炮塔，不随俯仰）
        let origMatrices = null;
        let currentConfigIdx = 0;
        let currentTurretDeg = 0, currentGunDeg = 0;
        let configGunGroups = [];
        let configTurretNodes = [];// all turret_0X root nodes (visual model), sorted by number

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
            const keys = Array.from(byGroup.keys()).sort((a, b) => a - b);
            for (const k of keys) {
                const arr = byGroup.get(k).sort((a, b) => ((a.name||'') < (b.name||'') ? -1 : 1));
                configGunGroups.push(arr);
            }
            configTurretNodes = turrets.sort((a, b) => (a.name.match(/\d+/)?.[0]|0) - (b.name.match(/\d+/)?.[0]|0));
        }

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
            turretNode = configTurretNodes[cfg.turret_index % configTurretNodes.length] || null;
            if (configGunGroups.length) {
                const grp = configGunGroups[cfg.gun_index % configGunGroups.length];
                gunNodesList = grp || [];
            } else {
                gunNodesList = [];
            }
            // 游戏装配语义【用户实证】：炮盾（gun_XX_mask）**随炮管俯仰**（焊在炮管摇篮上），
            // 与炮管本体同链。group 内其余节点 = 状态拆件（hide_elements 等），不参与姿态。
            gunBarrelNodes = gunNodesList.filter(n => /^gun_\d+(_mask)?$/.test(n.name || ''));
            gunMaskNodes = [];
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

            // 旋转枢轴 = models.pb 原点链（alignArmorModules 写入：track+turret / track+turret+gun）
            const tPivot = armorPivotTurret ? armorPivotTurret.clone() : new THREE.Vector3(0, 0, 1.7);
            const gPivot = armorPivotGun ? armorPivotGun.clone() : new THREE.Vector3(0, 0, 2.0);

            const tr = THREE.MathUtils.degToRad(turretDeg);
            const gr = THREE.MathUtils.degToRad(gunDeg);

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
            // 炮管本体 + 炮盾：炮塔旋转 × 俯仰（炮盾焊在炮管摇篮上，同链）
            const barrelNodes = gunBarrelNodes.length ? gunBarrelNodes : gunNodesList;
            for (const gn of barrelNodes) {
                const orig = origMatrices.get(gn);
                const m = mGun.clone();
                m.multiply(orig);
                gn.matrix.copy(m);
                gn.matrixWorldNeedsUpdate = true;
            }

            if (armorModel) {
                if (!armorOrigMatrices) collectArmorNodes();
                if (armorOrigMatrices) {
                    const aTP = tPivot.clone();
                    const aGP = gPivot.clone();
                    const mAT = new THREE.Matrix4();
                    mAT.makeTranslation(aTP.x, aTP.y, aTP.z);
                    // 与视觉炮塔同一旋转（含 initial_turret_rotation）——4 辆意大利固定战斗室 TD
                    // 依赖 itr 定型，漏叠会让装甲板与外观炮塔差 3~6.5° 俯仰
                    mAT.multiply(turretRot.clone());
                    mAT.multiply(new THREE.Matrix4().makeTranslation(-aTP.x, -aTP.y, -aTP.z));

                    const mAG = mAT.clone();
                    mAG.multiply(new THREE.Matrix4().makeTranslation(aGP.x, aGP.y, aGP.z));
                    mAG.multiply(new THREE.Matrix4().makeRotationX(gr));
                    mAG.multiply(new THREE.Matrix4().makeTranslation(-aGP.x, -aGP.y, -aGP.z));

                    for (const [node, orig] of armorOrigMatrices) {
                        const name = node.name || '';
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
            let gunPrefix = null, turretPrefix = null;
            if (cfg) {
                const pk = collectArmorPrefixes();
                const gunPk = pk.gun, turPk = pk.turret;
                if (gunPk.length) gunPrefix = gunPk[cfg.gun_index % gunPk.length];
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

        // 炮管/炮塔装甲前缀收集（原两函数各自全树 traverse，合并为一次遍历同时收集）。
        // 缓存到模块级：armorModel 未变时直接复用，applyArmorConfigVisible 与
        // collectArmorNodes 的重复遍历只发生一次
        let _armorPrefixCache = null;   // { gun: [...], turret: [...] }
        function collectArmorPrefixes() {
            if (_armorPrefixCache) return _armorPrefixCache;
            const gunSet = new Set(), turSet = new Set();
            if (armorModel) armorModel.traverse(n => {
                const name = n.name || '';
                const gm = name.match(/^(gun_\d+)_armor_/);
                if (gm) gunSet.add(gm[1]);
                const tm = name.match(/^(turret_\d+)_armor_/);
                if (tm) turSet.add(tm[1]);
            });
            const sortPk = (set) => Array.from(set).sort((a,b) => (a.match(/\d+/)?.[0]|0)-(b.match(/\d+/)?.[0]|0));
            _armorPrefixCache = { gun: sortPk(gunSet), turret: sortPk(turSet) };
            return _armorPrefixCache;
        }

        let mouseDownPos = null, isDragging = false;
        function onClick(event) {
            if (!armorModel) return;
            // 世界模式(射击复现)：判定结果锁定——初始判定射线已切换为 DecodeShotSegment
            // 基准（P1 位置 + P1→P2 弹向），点击不重跑判定，始终显示该初始结果
            if (window.__worldPenMode) return;
            const rect = renderer.domElement.getBoundingClientRect();
            const ndcX = ((event.clientX - rect.left) / rect.width) * 2 - 1;
            const ndcY = -((event.clientY - rect.top) / rect.height) * 2 + 1;
            doPenetrationCheck(ndcX, ndcY);
        }

        let __shotRayOrigin = null;   // 射击复现：射线起点（射手方向，固定距离）
        let __shotRayTarget = null;   // 射击复现：射线终点（瞄准点）
        let __penCheckSeq = 0;        // 判定代数序号：滑块连续重跑判定时丢弃过期的 /api/penetrate 响应
        // 射击复现错误面板（模块级：doPenetrationCheck 等顶层函数也要调用）
        function showShotError(msg) {
            console.error('[shot-replay] ' + msg);
            const st = document.getElementById('turret-controls');
            if (st) {
                // 世界模式调试关闭时该面板被收纳——错误必须强制可见
                st.style.display = 'block';
                st.innerHTML = '<div class="ctrl-row" style="color:#ff5555;"><b>射击复现错误</b></div>' +
                    '<div class="ctrl-row" style="color:#ff5555;font-size:11px;">' + msg + '</div>';
            }
        }

        // 命中分类 → armorHits 条目（原主循环与跳弹出射循环两段重复实现合并）：
        // parent 不可见 / configHidden / deco 过滤 + userData 或父链正则取名 +
        // getPlateThickness + partName 构造；被过滤或无厚度返回 null。
        // 注意：gunBarrel 的 gunClipPlane 过滤仅主判定循环有，留在调用点
        function classifyHit(hit) {
            if (hit.object.parent && hit.object.parent.visible === false) return null;
            if (hit.object.userData.configHidden) return null;
            if (hit.object.userData.armorSection === 'deco') return null;
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
            if (section === null || thickness === null || thickness === undefined) return null;
            const normal = hit.face ? hit.face.normal.clone() : new THREE.Vector3(0, 1, 0);
            const nm = new THREE.Matrix3().getNormalMatrix(hit.object.matrixWorld);
            normal.applyNormalMatrix(nm).normalize();
            let partName;
            if (section === 'chassis') {
                partName = plateId === 'leftTrack' ? 'Track (Left)' : 'Track (Right)';
            } else if (section === 'gunBarrel') {
                partName = 'Gun Barrel';
            } else {
                const disp = hit.object.userData.armorSectionOrig || section;
                partName = `${disp.charAt(0).toUpperCase() + disp.slice(1)} Plate ${plateId}`;
            }
            return { section, plateId, thickness, point: hit.point, normal, partName };
        }

        function doPenetrationCheck(ndcX, ndcY) {
            if (!armorModel) return;
            // 代数序号：仅最新一次判定的响应可上屏（滑块拖动会连续触发判定）
            const penSeq = ++__penCheckSeq;

            armorModel.traverse(function(node) {
                if (!node.isMesh) return;
                if (node.userData.configHidden) return;
                if (node.userData.armorSection === 'deco') { node.visible = false; return; }
                if (node.visible === false) node.visible = true;
            });
            const activeGun = activeGunNumber();
            const cfgForGun = currentConfig();
            // BlitzKit 真值语义：mask=0 视同无 mask
            const gunHasMask = cfgForGun && typeof cfgForGun.gun_mask === 'number' && cfgForGun.gun_mask !== 0;
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

            if (window.__segRay) {
                // DecodeShotSegment 判定基准（P1 位置 + P1→P2 弹向）优先于一切
                raycaster.set(window.__segRay.origin.clone(), window.__segRay.dir);
            } else if (__shotRayOrigin && __shotRayTarget) {
                const dir = __shotRayTarget.clone().sub(__shotRayOrigin).normalize();
                raycaster.set(__shotRayOrigin.clone(), dir);
            } else {
                mouse.x = ndcX;
                mouse.y = ndcY;
                raycaster.setFromCamera(mouse, camera);
            }
            const objects = [armorModel, ...activeModules];
            const intersects = raycaster.intersectObjects(objects, true);
            // blitzkit：非外部模块不做去重（外部模块的 variant 去重在判定侧进行）
            const armorHits = [];
            for (const hit of intersects) {
                if (hit.object.userData.armorSection === 'gunBarrel' && gunClipPlane &&
                    gunClipPlane.distanceToPoint(hit.point) < 0) continue;
                const entry = classifyHit(hit);
                if (entry) armorHits.push(entry);
            }

            if (armorHits.length === 0) {
                console.warn('[shot-replay] check: 0 armor hits, intersects=' + intersects.length);
                document.getElementById('click-info').style.display = 'none';
                document.getElementById('traj-info').style.display = 'none';
                trajInfoPos = null;
                if (trajGroup) { scene.remove(trajGroup); trajGroup = null; }
                // 弦不再与装甲相交（滑块拖到命中前位姿等）：同步清掉上次判定的命中
                // 标记，避免旧位置的绿/红点残留与"不相交"提示并存
                if (window.__hitMarker) { scene.remove(window.__hitMarker); window.__hitMarker = null; }
                if (window.__shotIsHit) {
                    if (window.__worldPenMode) {
                        // 世界模式：默认 tick 在命中前 ≈0.2s，弦不相交是正常数据态而非几何错误。
                        // 浮动中性提示即可，不覆盖左下 World View 面板（tick 选择器在里面）；
                        // 切到命中时刻附近的 tick 会自动被真实判定替换。
                        const div = document.getElementById('traj-info');
                        div.innerHTML = '<div style="background:rgba(12,14,22,0.97);border-radius:10px;'
                            + 'border-left:4px solid #ffcf5c;padding:10px 16px;font-size:13px;color:#ffcf5c;'
                            + 'white-space:nowrap;box-shadow:0 4px 20px rgba(0,0,0,0.5);">'
                            + '当前 tick 位姿与弹道弦不相交（命中前采样，坦克未到命中点）— 切换 tick 查看命中判定</div>';
                        trajInfoPos = controls.target.clone();
                        // 对比面板同步置中性：否则残留上一次判定的"✓ 一致/✗ 不一致"误导
                        const cmpEl = document.getElementById('world-pen-cmp');
                        if (cmpEl) cmpEl.innerHTML = '<span style="color:#888;">— 当前位姿弹道弦未命中装甲，无判定</span>';
                    } else {
                        showShotError('服务器判定命中但射线未命中任何装甲板——弹道/模型几何错位');
                    }
                }
                return;
            }

            // ===== HE 弹命中层语义修正 =====
            // HE(与一切爆炸弹)命中【首个表面】即爆炸，不沿弹道穿透整车；弦判定沿弦收集
            // 全部连续命中会把两侧装甲都算进溅射衰减 → HE 溅射伤害被过度衰减而误判
            // BLOCKED(服务器同发判有伤害)。修正：HE 只保留【第一个非 deco 命中】为爆炸点。
            // AP/APCR/HEAT 的多层穿透语义不变。
            const shellTypeNow = shellTypeOf(selectedShell);
            const hitsForCheck = (shellTypeNow === 'he' && armorHits.length > 1)
                ? [armorHits[0]] : armorHits;

            const first = armorHits[0];
            const point = first.point;
            // ===== 片元交叉验证：raycast 装甲片 ↔ 游戏原生 segment armor_group =====
            // segment（type=32 命中通知，服务器权威）给出命中装甲组。一致 → 标记绿色（高置信），
            // 不一致 → 红色保留 raycast 点（差异可能来自炮塔/炮管姿态近似）。
            const segCtx = window.__shotCtx || {};
            let fragOk = null;
            if (segCtx.segArmorGroup > 0) {
                const pid = parseInt(first.plateId, 10);
                fragOk = (!isNaN(pid)) && pid === segCtx.segArmorGroup;
            }
            if (window.__hitMarker) { scene.remove(window.__hitMarker); window.__hitMarker = null; }
            const markerColor = fragOk === true ? 0x22cc44 : (fragOk === false ? 0xff2222 : 0xffcc00);
            const markerGeo = new THREE.SphereGeometry(0.08, 16, 12);
            const markerMat = new THREE.MeshBasicMaterial({ color: markerColor, transparent: true, opacity: 0.9, depthTest: false });
            const marker = new THREE.Mesh(markerGeo, markerMat);
            marker.position.copy(point);
            marker.renderOrder = 999;
            scene.add(marker);
            window.__hitMarker = marker;
            const ringGeo = new THREE.RingGeometry(0.12, 0.18, 24);
            const ringMat = new THREE.MeshBasicMaterial({ color: markerColor, transparent: true, opacity: 0.7, side: THREE.DoubleSide, depthTest: false });
            const ring = new THREE.Mesh(ringGeo, ringMat);
            // 圆环面垂直入射弹向（命中点的"靶环"语义）；无弹向时回退朝向相机
            const ringDir = (window.__segRay && window.__segRay.dir)
                ? point.clone().add(window.__segRay.dir.clone()) : camera.position;
            ring.lookAt(ringDir);
            marker.add(ring);
            window.__hitMarkerRing = ring;
            if (fragOk !== null && window.__shotCtx) {
                window.__shotCtx.fragValidated = fragOk;
                console.log('[shot-replay] 片元验证:', fragOk ? '✓ 一致' : '✗ 不一致',
                    'raycast=' + first.section + '#' + first.plateId, 'segment_group=' + segCtx.segArmorGroup);
            }
            // world 复现模式：射线/交点在世界系，判定请求需模型局部米制（/api/penetrate
            // 语义）——经 worldToLocal 刚体逆变换换算；相对模式模型在原点未旋转，世界=局部。
            const worldPen = window.__worldPenMode === true && !!armorModel.parent;
            const toLocalPt = (p) => (worldPen ? armorModel.worldToLocal(p.clone()) : p.clone());
            // 判定入射方向 = 实际判定射线的方向（§七补4/补5：__segRay = P1→P2 解码弦，
            // 世界系，与 raycast 同源）。勿用炮口弦或模型局部系方向：前者与 P1→P2 射线
            // 是两条不同方向的线（入射角会被弦方向污染），后者与世界系命中法线做点积
            // 属坐标系混用（模型有偏航/坡度俯仰时角度全错）。
            const viewDir = (window.__segRay)
                ? window.__segRay.dir.clone()
                : (__shotRayOrigin && __shotRayTarget)
                    ? __shotRayOrigin.clone().sub(point).normalize()
                    : camera.position.clone().sub(point).normalize();
            const shotRayO = (window.__segRay || __shotRayOrigin)
                ? raycaster.ray.origin.clone() : null;

            // 命中距离（米）：射击复现 = 真实炮口(method29 launchPoint) → 命中点，
            // × worldMetersPerUnit 换算回真实米数；其他模式回退炮管几何/相机距离。
            const dist = (__shotRayOrigin
                ? point.distanceTo(__shotRayOrigin)
                : gunMuzzleWorld
                ? point.distanceTo(gunMuzzleWorld)
                : point.distanceTo(camera.position)) * (worldMetersPerUnit || 1);

            const shellType = shellTypeOf(selectedShell);
            const pen = selectedShell ? (selectedShell.penetration || 0) : 0;
            const dmg = selectedShell ? (selectedShell.damage || 0) : 0;
            const modDmg = selectedShell ? (selectedShell.module_damage || 0) : 0;   // 仅显示用
            const caliber = shooterCaliber || (shooterData && shooterData.caliber) || tankData.caliber || 120;
            const isHE = shellType === 'he';
            const eqCal = !!(document.getElementById('eq-calibrated') && document.getElementById('eq-calibrated').checked);
            const eqEnh = !!(document.getElementById('eq-enhanced') && document.getElementById('eq-enhanced').checked);
            const penDisp = pen * shellPenMul(selectedShell);
            const mpu = worldMetersPerUnit || 1;
            // 每发弹参数（blitzkit：normalization ?? 0；ricochet 仅非 explosive 弹使用）
            const shellNormDeg = (selectedShell && selectedShell.normalization != null) ? selectedShell.normalization : null;
            const shellRicoDeg = (selectedShell && selectedShell.ricochet > 0) ? selectedShell.ricochet : null;

            // blitzkit shoot() 只用 near 穿深（无距离衰减）；dist 仅用于显示
            const req = {
                shell_type: shellType,
                penetration: pen,
                caliber: caliber,
                damage: dmg,
                explosion_radius: isHE ? ((selectedShell && selectedShell.explosion_radius) || 3.0) : 0,
                calibrated_shells: eqCal,
                enhanced_armor: eqEnh,
                normalization_deg: shellNormDeg,
                ricochet_deg: shellRicoDeg,
                allow_ricochet: true,
                view_dir: [viewDir.x, viewDir.y, viewDir.z],
                hits: hitsForCheck.map(ah => {
                    const lp = toLocalPt(ah.point);
                    return {
                        section: ah.section,
                        plate_id: ah.plateId,
                        thickness: ah.thickness,
                        normal: [ah.normal.x, ah.normal.y, ah.normal.z],
                        point: [lp.x * mpu, lp.y * mpu, lp.z * mpu],
                        part_name: ah.partName,
                    };
                }),
            };

            fetch('/api/penetrate', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(req),
            }).then(r => { if (!r.ok) throw new Error('API ' + r.status); return r.json(); }).then(res => {
                if (penSeq !== __penCheckSeq) return;   // 过期响应：更新位姿的判定已在途，直接丢弃
                let trajLayers = res.layers.map(l => {
                    const ah = hitsForCheck.find(ah => ah.partName === l.part_name);
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

                if (res.ricochet && res.ricochet_remaining_pen > 0) {
                    const lastLayer = trajLayers[trajLayers.length - 1];
                    if (lastLayer && lastLayer.point) {
                        const shellDir = viewDir.clone().negate(); // incoming shell direction
                        let n = lastLayer.normal;
                        if (!n) {
                            const hitMatch = hitsForCheck.find(ah => ah.point.distanceToSquared(lastLayer.point) < 1e-6);
                            n = hitMatch ? hitMatch.normal : first.normal;
                        }
                        const reflect = shellDir.clone().sub(n.clone().multiplyScalar(2 * shellDir.dot(n))).normalize();
                        const rc = new THREE.Raycaster(lastLayer.point.clone().add(reflect.clone().multiplyScalar(0.05)), reflect, 0.01, 60);
                        const ricIntersects = rc.intersectObjects(objects, true);
                        const ricHits = [];
                        for (const hit of ricIntersects) {
                            if (hit.point.distanceToSquared(lastLayer.point) < 0.01) continue;
                            const entry = classifyHit(hit);
                            if (entry) ricHits.push(entry);
                        }
                        // blitzkit：出射射线（allowRicochet=false）未命中 Primary → shoot 返回
                        // null（无出射段、伤害 0），不进行二次判定
                        const ricHasPrimary = ricHits.some(h => h.section === 'hull' || h.section === 'turret' || h.section === 'gun');
                        if (ricHits.length > 0 && ricHasPrimary) {
                            const ricReq = {
                                shell_type: shellType, penetration: res.ricochet_remaining_pen, caliber: caliber,
                                damage: dmg,
                                enhanced_armor: eqEnh,
                                normalization_deg: shellNormDeg,
                                ricochet_deg: shellRicoDeg,
                                allow_ricochet: false,
                                view_dir: [reflect.x, reflect.y, reflect.z],
                                hits: ricHits.map(ah => ({ section: ah.section, plate_id: ah.plateId, thickness: ah.thickness, normal: [ah.normal.x, ah.normal.y, ah.normal.z], point: [ah.point.x * mpu, ah.point.y * mpu, ah.point.z * mpu], part_name: ah.partName })),
                            };
                            fetch('/api/penetrate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(ricReq) })
                                .then(r => r.ok ? r.json() : null).then(ricRes => {
                                    if (penSeq !== __penCheckSeq) return;   // 过期响应丢弃
                                    if (ricRes) {
                                        const ricLayers = ricRes.layers.map(l => ({ point: ricHits.find(ah => ah.partName === l.part_name)?.point || lastLayer.point, name: l.part_name, thickness: l.thickness, eff: l.effective, remainBefore: l.remaining_before, penetrated: l.penetrated, ricochet: l.ricochet, seg: 1 }));
                                        const combined = { result: 'RICOCHET → ' + ricRes.result, total_effective: res.total_effective, layers: [...trajLayers, ...ricLayers] };
                                        showTrajectory(point, combined.result, combined.total_effective, combined.layers, penDisp, dmg, modDmg, dist, shotRayO);
                                    } else { showTrajectory(point, res.result, res.total_effective, trajLayers, penDisp, dmg, modDmg, dist, shotRayO); }
                                }).catch(() => { showTrajectory(point, res.result, res.total_effective, trajLayers, penDisp, dmg, modDmg, dist, shotRayO); });
                            return;
                        }
                    }
                }

                showTrajectory(point, res.result, res.total_effective, trajLayers, penDisp, dmg, modDmg, dist, shotRayO);
            }).catch(err => {
                if (penSeq !== __penCheckSeq) return;   // 过期请求的失败不覆盖最新结果
                console.error('Penetration API error:', err);
                showTrajectory(point, 'ERROR', 0, [], penDisp, dmg, modDmg, dist, shotRayO);
            });
        }


        let trajGroup = null;
        let trajInfoPos = null;
        function showTrajectory(firstPoint, result, totalEff, layers, penVal, dmgVal, modDmgVal, distVal, trajOrigin) {
            // world 复现模式：本地内核预测 vs 服务器判定（method38 位图 + game_hit_result）
            if (window.__worldPenMode && window.__worldServerInfo) {
                const el = document.getElementById('world-pen-cmp');
                if (el) {
                    const srv = window.__worldServerInfo;
                    // 服务器等价类：跳弹↔RICOCHET；击穿/HE↔PENETRATION；未穿/间隙止↔BLOCKED
                    const srvEq = srv.cls === 'RICOCHET' ? 'RICOCHET'
                        : (srv.cls === 'PENETRATION' || srv.cls === 'HE BLAST') ? 'PENETRATION'
                        : (srv.cls === 'MISS' ? 'MISS' : 'BLOCKED');
                    // 本地预测等价类：HE 爆炸有伤害内核报 PENETRATION；HE 被装甲挡住报 BLOCKED
                    const locEq = result === 'RICOCHET' ? 'RICOCHET'
                        : result === 'PENETRATION' ? 'PENETRATION'
                        : (result === 'BLOCKED' || result === 'ERROR') ? 'BLOCKED' : 'OTHER';
                    const agree = srvEq !== 'MISS' && locEq !== 'OTHER' && srvEq === locEq;
                    const RES_TXT2 = {0:'无结果',1:'未击穿',2:'间隙止',3:'有伤害',4:'跳弹'};
                    el.innerHTML = '<span style="color:' + (agree ? '#5fbf7a' : '#ff6b6b') + ';">'
                        + (agree ? '✓ 一致' : '✗ 不一致') + '</span>'
                        + ' · 本地: ' + result
                        + ' · 服务器: ' + srv.cls
                        + (typeof srv.result === 'number' && srv.result !== 255
                            ? ' (' + (RES_TXT2[srv.result] || srv.result) + ')' : '');
                }
            }
            if (trajGroup) scene.remove(trajGroup);
            trajGroup = new THREE.Group();

            // 轨迹颜色按【最终能否击穿】染色(用户要求):最终击穿=绿,未穿=红,
            // 跳弹但未击穿=橙。复合结果('RICOCHET → PENETRATION' 等)取末段判定。
            const finalOutcome = result.split('→').pop().trim();
            const color = finalOutcome === 'PENETRATION' ? 0x4CAF50
                : (finalOutcome === 'RICOCHET' ? 0xFF8800
                : (finalOutcome === 'BLOCKED' || finalOutcome === 'ERROR' ? 0xf44336 : 0xff8800));
            const camDir = camera.position.clone().sub(firstPoint).normalize();

            // 轨迹管原点：调用方传入的弹道起点（如炮口）优先；否则沿视线反向
            // 长距离回退（300m），入射弹向一眼可见（原 15m 太短——用户反馈）
            const origin = trajOrigin || firstPoint.clone().add(camDir.clone().multiplyScalar(300));
            const trajMat = new THREE.MeshBasicMaterial({ color: color, depthTest: false, transparent: true, opacity: 0.85 });

            const ricLayer = layers.find(l => l.ricochet);
            const ricPoint = ricLayer ? ricLayer.point : null;

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

            for (let i = 0; i < layers.length; i++) {
                const l = layers[i];
                const pColor = l.penetrated ? 0x4CAF50 : (l.ricochet ? 0xFF8800 : 0xf44336);

                const dotGeo = new THREE.SphereGeometry(0.04, 8, 8);
                const dotMat = new THREE.MeshBasicMaterial({ color: pColor, depthTest: false, transparent: true, opacity: 0.95 });
                const dotMesh = new THREE.Mesh(dotGeo, dotMat);
                dotMesh.position.copy(l.point);
                dotMesh.renderOrder = 999;
                trajGroup.add(dotMesh);
            }

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
                // 厚度去尾差：整数显示 150，小数保留一位 62.4
                const th = Number(l.thickness);
                const thStr = isFinite(th) ? (Number.isInteger(Math.round(th * 10) / 10) ? String(Math.round(th)) : (Math.round(th * 10) / 10).toFixed(1)) : l.thickness;
                html += `<div style="font-size:13px;line-height:20px;color:${pc};">`;
                html += `<span style="color:${pc};">●</span> <span style="color:#ddd;">${l.name}</span>`;
                html += `<span style="color:#888;margin-left:16px;">${thStr}mm / ${l.eff.toFixed(0)}eff / pen ${remain}</span>`;
                html += `</div>`;
            }
            html += `</div>`;
            div.innerHTML = html;
            // 轨迹线与轨迹面板无条件展示(用户要求:与正常点击判定一致的轨迹展示)
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
            const offX = 0, offY = 160;
            let px = x + offX, py = y + offY;
            div.style.display = 'block';   // 先显示才能量取实际尺寸
            const w = div.offsetWidth || 300, h = div.offsetHeight || 90;
            // 视口钳制（translateX(-50%)：left 是面板中心 x）
            const clampX = (v2) => Math.max(w / 2 + 10, Math.min(window.innerWidth - w / 2 - 10, v2));
            const clampY = (v2) => Math.max(10, Math.min(window.innerHeight - h - 10, v2));
            px = clampX(px); py = clampY(py);
            // 固定 UI 避让：不压四角与顶部信息面板（左下信息/右下按钮提示/右上栈/左上排）
            const blockers = [];
            const tc = document.getElementById('turret-controls');
            if (tc && tc.style.display !== 'none') blockers.push(tc.getBoundingClientRect());
            const addRect = (id) => {
                const el = document.getElementById(id);
                if (el) { const r = el.getBoundingClientRect(); if (r.width > 0 && r.height > 0) blockers.push(r); }
            };
            addRect('corner-br'); addRect('corner-tr'); addRect('info-panel'); addRect('tank-selectors');
            for (const r of blockers) {
                if (r.width === 0 || r.height === 0) continue;
                const l = px - w / 2, t = py, rr = l + w, b = t + h;
                if (l >= r.right || rr <= r.left || t >= r.bottom || b <= r.top) continue;
                // 首选：整体上移到面板上方；顶部放不下再水平平移到面板侧边
                if (r.top - h - 12 >= 10) { py = clampY(r.top - h - 12); continue; }
                px = clampX(px < (r.left + r.right) / 2 ? r.left - w / 2 - 12 : r.right + w / 2 + 12);
            }
            div.style.transform = 'translateX(-50%)';
            div.style.left = px + 'px';
            div.style.top = py + 'px';
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
                    }
                }
                refreshPenetrationResolution();
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
