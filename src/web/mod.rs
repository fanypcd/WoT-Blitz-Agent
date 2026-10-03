use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use axum::{
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::models::config::Config;
use crate::web::sessions::{ChatError, SessionManager};
use rust_embed::RustEmbed;

/// Vue 前端构建产物（frontend/dist/，构建入口 `cd frontend && npm run build`）。
/// 默认 debug 构建运行时读盘（npm run build 后刷新即生效）；release 构建编译期嵌入。
/// 排除 release/：dist 只认 npm 产物，导出资产包若误落 dist 内会被编译期嵌入撑爆构建。
#[derive(RustEmbed)]
#[folder = "frontend/dist/"]
#[exclude = "release/*"]
#[exclude = "release/**"]
struct SpaAssets;

pub mod assets;
pub mod sessions;

/// 配置缓存：按 config.toml 的 mtime 判定是否需要重新解析。
/// 保留"每请求都能读到最新配置"的原语义（用户手动编辑、config_set 写回均即时生效），
/// 只是 mtime 未变时免去重复读盘 + TOML 解析。解析失败不缓存（保持每次重试）。
struct ConfigCache {
    path: std::path::PathBuf,
    cached: Mutex<Option<(Option<SystemTime>, Config)>>,
}

impl ConfigCache {
    fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            cached: Mutex::new(None),
        }
    }

    fn load(&self) -> anyhow::Result<Config> {
        let cur = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        {
            let guard = self.cached.lock().unwrap();
            if let Some((mt, cfg)) = guard.as_ref() {
                if *mt == cur {
                    return Ok(cfg.clone());
                }
            }
        }
        let cfg = Config::load_or_create(&self.path)?;
        // 以解析成功后的 mtime 入缓存；解析期间文件又被改则下次请求自然重载
        let mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        *self.cached.lock().unwrap() = Some((mtime, cfg.clone()));
        Ok(cfg)
    }

    /// config_set 写回成功后调用：保存的配置即为文件最新内容，直接入缓存。
    fn refresh(&self, cfg: Config) {
        let mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        *self.cached.lock().unwrap() = Some((mtime, cfg));
    }
}

/// tank_cache.json 缓存：该文件只在离线 fetch-tanks 时变化，按 mtime 判定失效
/// （服务运行期间被外部重建，下次请求自动重新加载，语义与逐请求重读等价）。
struct TankCache {
    path: std::path::PathBuf,
    state: Mutex<TankCacheState>,
}

#[derive(Default)]
struct TankCacheState {
    mtime: Option<SystemTime>,
    resolver: Option<Arc<crate::wargaming::tank_resolver::TankResolver>>,
    json: Option<Arc<Value>>,
}

impl TankCache {
    fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            state: Mutex::new(TankCacheState::default()),
        }
    }

    /// mtime 变化时重新解析；加载失败时 resolver/json 均为 None（降级为 Null）。
    fn snapshot(
        &self,
    ) -> (
        Option<Arc<crate::wargaming::tank_resolver::TankResolver>>,
        Arc<Value>,
    ) {
        let mtime = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        let mut st = self.state.lock().unwrap();
        if st.mtime != mtime {
            let resolver =
                crate::wargaming::tank_resolver::TankResolver::load_from_json_file(&self.path)
                    .ok()
                    .map(Arc::new);
            let json = std::fs::read_to_string(&self.path)
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .map(Arc::new);
            *st = TankCacheState {
                mtime,
                resolver,
                json,
            };
        }
        (
            st.resolver.clone(),
            st.json.clone().unwrap_or_else(|| Arc::new(Value::Null)),
        )
    }

    fn resolver(&self) -> Option<Arc<crate::wargaming::tank_resolver::TankResolver>> {
        self.snapshot().0
    }

    /// 原始 JSON（文件缺失/损坏时为 Null）。
    fn json(&self) -> Arc<Value> {
        self.snapshot().1
    }
}

/// 全局共享状态：配置缓存 + 坦克缓存 + 会话管理器（actor-per-session，见 sessions.rs）。
#[derive(Clone)]
struct AppState {
    config: Arc<ConfigCache>,
    tank_cache: Arc<TankCache>,
    config_path: std::path::PathBuf,
    sessions: SessionManager,
}

/// 构建完整 Web GUI 路由（含共享 state）；可被任何宿主（CLI `serve`、自定义协议桥）复用。
/// 调用前须已完成 `data::set_base_dir`（若需要重定向运行目录）。
pub fn build_router(config_path: std::path::PathBuf, sessions_dir: std::path::PathBuf) -> Router {
    let state = AppState {
        sessions: SessionManager::new(config_path.clone(), sessions_dir),
        config: Arc::new(ConfigCache::new(config_path.clone())),
        tank_cache: Arc::new(TankCache::new(crate::data::data_path("tank_cache.json"))),
        config_path,
    };

    let viewer_resolver = crate::wargaming::tank_resolver::TankResolver::load_from_json_file(
        crate::data::data_path("tank_cache.json").as_path(),
    )
    .ok()
    .unwrap_or_default();
    crate::wargaming::tank_configs::set_global_resolver(viewer_resolver);

    Router::new()
        .route("/", get(spa_index_handler))
        .route("/index.html", get(spa_index_handler))
        .route("/assets/{*path}", get(spa_asset_handler))
        // 主 GUI 六 Tab 由 Vue SPA 接管（vue-router 分发）
        .route("/tankopedia", get(spa_index_handler))
        .route("/player", get(spa_index_handler))
        .route("/replay", get(spa_index_handler))
        .route("/compare", get(spa_index_handler))
        .route("/settings", get(spa_index_handler))
        // 坦克详情由 Vue SPA 接管（vue-router 路由 /tank/:tankId）
        .route("/tank/{tank_id}", get(spa_index_handler))
        .route("/api/chat", post(chat_handler))
        .route("/api/chat/events", get(chat_events_handler))
        .route("/api/chat/cancel", post(chat_cancel_handler))
        .route("/api/session", get(session_get))
        .route("/api/sessions", get(sessions_list))
        .route("/api/session/{id}", delete(session_delete))
        .route("/api/session/{id}/export", get(session_export))
        .route("/api/config", get(config_get).post(config_set))
        .route("/api/usage", get(usage_get))
        .route("/api/player/{nickname}", get(player_handler))
        .route("/api/scan", post(scan_handler))
        .route("/api/replay/shots", post(replay_shots_handler))
        .route("/api/replay/upload", post(replay_upload_handler))
        // 实时回放由 Vue SPA 接管（vue-router PlaybackView + scene/playbackScene.js）
        .route("/playback", get(spa_index_handler))
        // 3D 场景资产（回放 GLB 车模/部件数据；与 /armor_view 前缀版共用 handler）
        .route(
            "/glb/{tank_id}/{filename}",
            get(crate::web::assets::glb_handler),
        )
        .route(
            "/api/tank/{tank_id}",
            get(crate::web::assets::tank_data_handler),
        )
        // 装甲检视器 Vue 版走根级数据端点（旧 /armor_view 前缀版保留兼容）
        .route(
            "/api/tank_filter",
            get(crate::web::assets::tank_filter_handler),
        )
        .route(
            "/api/shells/{tank_id}",
            get(crate::web::assets::shells_handler),
        )
        .route(
            "/api/penetrate",
            post(crate::web::assets::penetrate_handler),
        )
        .route("/api/replay_shot", get(replay_shots_embedded_handler))
        .route("/api/hold", get(crate::web::assets::hold_handler))
        .route("/api/ready", get(crate::web::assets::ready_handler))
        .route("/api/playback/data", post(playback_data_handler))
        .route(
            "/api/playback/map",
            get(crate::wargaming::playback_viewer::playback_map_handler),
        )
        .route(
            "/api/playback/terrain",
            get(crate::wargaming::playback_viewer::playback_terrain_handler),
        )
        .route(
            "/api/playback/scenery",
            get(crate::wargaming::playback_viewer::playback_scenery_handler),
        )
        .route(
            "/api/playback/groundmeta",
            get(crate::wargaming::playback_viewer::playback_groundmeta_handler),
        )
        .route(
            "/api/playback/groundtex",
            get(crate::wargaming::playback_viewer::playback_groundtex_handler),
        )
        .route("/api/snapshot", post(snapshot_handler))
        .route("/api/prematch", post(prematch_handler))
        .route("/api/tanks", get(tanks_handler))
        .route("/api/tank_detail/{tank_id}", get(tank_detail_handler))
        .route(
            "/api/tank_image/{tank_id}",
            get(crate::web::assets::tank_image_handler),
        )
        .route("/screenshots/{*path}", get(screenshots_handler))
        // /armor_view/view/{id} 由 Vue SPA 接管（vue-router ArmorView）；
        // 旧 /armor_view 前缀数据路由保留（standalone viewer 与旧书签兼容）
        .route("/armor_view/view/{tank_id}", get(spa_index_handler))
        .route("/armor_view/", get(armor_view_root))
        .route(
            "/armor_view/glb/{tank_id}/{filename}",
            get(crate::web::assets::glb_handler),
        )
        .route(
            "/armor_view/api/tank/{tank_id}",
            get(armor_tank_data_handler),
        )
        .route(
            "/armor_view/api/tank_filter",
            get(crate::web::assets::tank_filter_handler),
        )
        .route(
            "/armor_view/api/tank_image/{tank_id}",
            get(crate::web::assets::tank_image_handler),
        )
        .route(
            "/armor_view/api/shells/{tank_id}",
            get(crate::web::assets::shells_handler),
        )
        .route(
            "/armor_view/api/penetrate",
            post(crate::web::assets::penetrate_handler),
        )
        .route(
            "/armor_view/api/replay_shot",
            get(replay_shots_embedded_handler),
        )
        .route("/api/models/status", get(models_status_handler))
        .route(
            "/api/models/download_all",
            post(models_download_all_handler),
        )
        // 回放上传 body 上限：axum 默认 2MB，而 .wotbreplay 普遍 1-10MB，
        // 不放宽则移动端"导入回放"超限直接 413
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(state)
}

pub async fn serve(config_path: std::path::PathBuf) -> anyhow::Result<()> {
    let app = build_router(config_path.clone(), crate::data::data_path("sessions"));
    let addr = SocketAddr::from(([0, 0, 0, 0], 18999));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}", local_addr.port());
    eprintln!("Web UI running at {}", url);
    if webbrowser::open(&url).is_err() {
        eprintln!("Please open {} in your browser manually.", url);
    }
    axum::serve(listener, app).await?;
    Ok(())
}

/// Vue SPA 入口。被 SPA 接管的页面路由（/tank/{id}、/playback 等）共用；
/// standalone 服务（playback_viewer::serve_standalone）经 spa_index_response 复用。
async fn spa_index_handler() -> Response {
    spa_index_response()
}

pub fn spa_index_response() -> Response {
    match SpaAssets::get("index.html") {
        Some(file) => (
            [
                (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (axum::http::header::CACHE_CONTROL, "no-cache"),
            ],
            file.data,
        )
            .into_response(),
        None => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "frontend/dist/index.html 缺失：请先构建前端（cd frontend && npm ci && npm run build）",
        )
            .into_response(),
    }
}

/// SPA 静态资源（Vite 输出带 content hash，可长缓存）。
/// rust-embed 的键为相对 dist/ 的完整路径，路由通配符捕获的是 /assets/ 之后一段，需补前缀。
async fn spa_asset_handler(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    spa_asset_response(&path)
}

pub fn spa_asset_response(path: &str) -> Response {
    let Some(file) = SpaAssets::get(&format!("assets/{path}")) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            format!("asset not found: {path}"),
        )
            .into_response();
    };
    let ct = match path.rsplit('.').next().unwrap_or("") {
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "html" => "text/html; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    };
    (
        [
            (axum::http::header::CONTENT_TYPE, ct),
            (
                axum::http::header::CACHE_CONTROL,
                "public, max-age=31536000, immutable",
            ),
        ],
        file.data,
    )
        .into_response()
}

/// `/armor_view/` 根：重定向到默认坦克（IS-7）的检视页。
async fn armor_view_root() -> axum::response::Redirect {
    axum::response::Redirect::temporary("/armor_view/view/7169")
}

/// 3D 检视坦克数据：model_url 加 `/armor_view` 前缀，否则 iframe 内会去请求顶层 `/glb/...`（404）。
async fn armor_tank_data_handler(
    axum::extract::Path(tank_id): axum::extract::Path<u64>,
) -> Json<Value> {
    Json(crate::wargaming::tank_configs::tank_data_value_prefixed(
        tank_id as u32,
        "/armor_view",
    ))
}

/// 发起一次对话：命令投递给该会话的 actor（串行执行），返回 session_id。
async fn chat_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let session_id = req["session_id"].as_str().unwrap_or("default").to_string();
    let text = req["message"].as_str().unwrap_or("").to_string();
    if text.trim().is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "empty message").into_response();
    }

    match state.sessions.chat(&session_id, text).await {
        Ok(()) => Json(json!({ "session_id": session_id, "status": "started" })).into_response(),
        Err(ChatError::Busy) => (
            axum::http::StatusCode::CONFLICT,
            "session busy: a conversation is already running",
        )
            .into_response(),
        Err(ChatError::InvalidId) => (
            axum::http::StatusCode::BAD_REQUEST,
            "invalid session id (allowed: letters/digits/_/-, max 64 chars)",
        )
            .into_response(),
        Err(ChatError::Failed(e)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to send chat: {e}"),
        )
            .into_response(),
    }
}

/// 拉取某会话的事件流（前端轮询，增量渲染进度）。
async fn chat_events_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let session_id = q.get("session_id").cloned().unwrap_or("default".into());
    // events_of 已在锁内直接序列化为 JSON（避免先深拷贝事件缓冲）
    let arr: Vec<Value> = state.sessions.events_of(&session_id).await;
    Json(json!({ "events": arr })).into_response()
}

/// 打断指定会话的对话（会话级取消令牌；未指定 id 时回退全局标志）。
async fn chat_cancel_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    match q.get("session_id").filter(|s| SessionManager::valid_id(s)) {
        Some(id) => state.sessions.cancel(id).await,
        None => crate::agent::set_interrupted(),
    }
    Json(json!({ "status": "cancelling" })).into_response()
}

/// 读取某会话的消息历史（不含系统提示；活跃读镜像，非活跃读磁盘）。
async fn session_get(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let session_id = q.get("session_id").cloned().unwrap_or("default".into());
    // history_of 已在锁内直接序列化为 JSON（避免先深拷贝消息历史）
    let history: Vec<Value> = state.sessions.history_of(&session_id).await;
    Json(json!({ "messages": history })).into_response()
}

async fn sessions_list(axum::extract::State(state): axum::extract::State<AppState>) -> Response {
    let ids = state.sessions.list_ids().await;
    Json(json!({ "sessions": ids })).into_response()
}

async fn session_delete(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    state.sessions.delete(&id).await;
    Json(json!({ "status": "deleted" })).into_response()
}

async fn session_export(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    // 会话不存在 → 返回空 Markdown（而非 404），保证导出按钮始终可用
    let history = state.sessions.history_of(&id).await;
    let mut md = format!("# WoTB Agent Session: {}\n\n", id);
    for v in &history {
        let role = v["role"].as_str().unwrap_or("").to_string();
        // 只导出用户提问与 assistant 最终回答：跳过 tool 结果、带 tool_calls 的中间轮次、空内容
        if role == "user" {
            let content = v["content"].as_str().unwrap_or("");
            if content.is_empty() {
                continue;
            }
            md.push_str(&format!("## 👤 User\n\n{}\n\n---\n\n", content));
        } else if role == "assistant" {
            let has_calls = v["tool_calls"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if has_calls {
                continue;
            }
            let content = v["content"].as_str().unwrap_or("");
            if content.is_empty() {
                continue;
            }
            md.push_str(&format!("## 🤖 Assistant\n\n{}\n\n---\n\n", content));
        }
    }
    if md.trim() == format!("# WoTB Agent Session: {}", id).trim() {
        md.push_str("\n*（此会话暂无消息）*\n");
    }
    axum::response::Response::builder()
        .header("Content-Type", "text/markdown; charset=utf-8")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}.md\"", id),
        )
        .body(axum::body::Body::from(md))
        .unwrap()
        .into_response()
}

async fn config_get(axum::extract::State(state): axum::extract::State<AppState>) -> Response {
    match state.config.load() {
        Ok(c) => Json(json!({
            "llm": { "model": c.llm.model, "endpoint": c.llm.endpoint,
                     "context_length": c.llm.context_length,
                     "thinking_mode": c.llm.thinking_mode,
                     "max_tokens": c.llm.max_tokens,
                     "budget": c.llm.budget,
                     "api_key_set": !c.llm.api_key.is_empty() },
            "wg_api": { "server": c.wg_api.server },
            "replay": { "replay_dir": c.replay.replay_dir },
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

async fn config_set(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let mut c = match state.config.load() {
        Ok(c) => c,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response()
        }
    };
    if let Some(model) = req["model"].as_str() {
        c.llm.model = model.into();
    }
    if let Some(endpoint) = req["endpoint"].as_str() {
        c.llm.endpoint = endpoint.into();
    }
    if let Some(api_key) = req["api_key"].as_str() {
        c.llm.api_key = api_key.into();
    }
    if let Some(ctx) = req["context_length"].as_u64() {
        c.llm.context_length = ctx as u32;
    }
    if let Some(thinking) = req["thinking_mode"].as_bool() {
        c.llm.thinking_mode = thinking;
    }
    if let Some(max) = req["max_tokens"].as_u64() {
        c.llm.max_tokens = Some(max as u32);
    }
    if let Some(budget) = req["budget"].as_f64() {
        c.llm.budget = Some(budget);
    }
    match c.save(&state.config_path) {
        Ok(_) => {
            // 写回成功后刷新配置缓存（内容与文件一致，无需下次请求重读）
            state.config.refresh(c);
            Json(json!({ "status": "saved" })).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

async fn usage_get(axum::extract::State(state): axum::extract::State<AppState>) -> Response {
    let usage = crate::models::config::TokenUsage::load_from_file(
        crate::data::data_path("token_usage.json").as_path(),
    )
    .unwrap_or_default();
    Json(json!({
        "total_input_tokens": usage.total_input_tokens,
        "total_output_tokens": usage.total_output_tokens,
        "total_tokens": usage.total_input_tokens + usage.total_output_tokens,
        "total_cost": usage.total_cost,
        "call_count": usage.call_count,
        "calls": usage.calls.iter().map(|c| json!({
            "timestamp": c.timestamp, "model": c.model,
            "input_tokens": c.input_tokens, "output_tokens": c.output_tokens,
            "cost": c.cost,
        })).collect::<Vec<_>>(),
        "config_path": state.config_path.to_string_lossy(),
    }))
    .into_response()
}

async fn player_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(nickname): axum::extract::Path<String>,
) -> Response {
    let config = match state.config.load() {
        Ok(c) => c,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response()
        }
    };
    let app_id = config.wg_api.application_id.clone();
    let server = config.wg_api.server.clone();

    // WG API 是阻塞调用，放到 spawn_blocking 避免阻塞 tokio 运行时
    let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
        let client = crate::wargaming::api_client::WgApiClient::new(&app_id, &server);
        let results = client.search_player(&nickname, true)?;
        if results.is_empty() {
            return Ok(json!({ "players": [] }));
        }
        let mut out = Vec::new();
        for (_name, id) in results.iter().take(10) {
            if let Ok(stats) = client.get_player_stats(*id) {
                out.push(serde_json::to_value(&stats).unwrap_or(Value::Null));
            }
        }
        Ok(json!({ "players": out }))
    })
    .await;

    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

async fn scan_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let config = match state.config.load() {
        Ok(c) => c,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response()
        }
    };
    let replay_dir = req["dir"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| config.replay.translate(s))
        .unwrap_or_else(|| config.replay.translate(&config.replay.replay_dir));
    let mode = req["mode"].as_str().unwrap_or("all").to_string();
    let days = req["days"].as_i64();

    // 可选：显式文件列表（移动端导入后只分析用户实际选择的文件）
    let files: Vec<String> = req["files"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // 坦克缓存：mtime 未变时复用 AppState 缓存
    let resolver = state.tank_cache.resolver();
    let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
        let filter = crate::replay::scanner::ScanFilter::from_mode(&mode, days);
        let scanner = match &resolver {
            Some(r) => crate::replay::scanner::ReplayScanner::with_resolver(r.as_ref()),
            None => crate::replay::scanner::ReplayScanner::new(),
        };
        let battles = if files.is_empty() {
            scanner.scan_dir(std::path::Path::new(&replay_dir), &filter, |_| {})?
        } else {
            let paths: Vec<std::path::PathBuf> =
                files.iter().map(std::path::PathBuf::from).collect();
            scanner.scan_files(&paths, &filter, |_| {})?
        };
        let report = crate::models::report::AggregatedReport::from_battles(battles, &mode);
        Ok(serde_json::to_value(&report).unwrap_or(Value::Null))
    })
    .await;

    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

async fn snapshot_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let config = match state.config.load() {
        Ok(c) => c,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response()
        }
    };
    let app_id = config.wg_api.application_id.clone();
    let server = config.wg_api.server.clone();
    let nickname = req["nickname"].as_str().unwrap_or("").to_string();
    let action = req["action"].as_str().unwrap_or("take").to_string();
    let dir = crate::data::app_path(req["dir"].as_str().unwrap_or("data/snapshots"))
        .to_string_lossy()
        .to_string();

    let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
        let client = crate::wargaming::api_client::WgApiClient::new(&app_id, &server);
        let results = client.search_player(&nickname, true)?;
        let Some((_name, id)) = results.first() else {
            return Ok(json!({ "status": "no_player" }));
        };
        let stats = client.get_player_stats(*id)?;
        let store = crate::wargaming::snapshot::SnapshotStore::new(std::path::Path::new(&dir));
        match action.as_str() {
            "list" => {
                let snaps = store.list()?;
                let arr: Vec<Value> = snaps
                    .iter()
                    .map(|s| {
                        json!({
                            "timestamp": s.timestamp, "datetime": s.datetime,
                            "nickname": s.player.nickname,
                            "battles": s.player.rating_battles,
                            "mm_rating": s.player.rating_mm_rating,
                        })
                    })
                    .collect();
                Ok(json!({ "status": "ok", "snapshots": arr }))
            }
            "diff" => {
                let from = store
                    .oldest()?
                    .ok_or_else(|| anyhow::anyhow!("no snapshots"))?;
                let to = store
                    .latest()?
                    .ok_or_else(|| anyhow::anyhow!("no snapshots"))?;
                let diff = store.diff(&from, &to);
                Ok(serde_json::to_value(&diff).unwrap_or(Value::Null))
            }
            _ => {
                let snap = crate::wargaming::snapshot::Snapshot::from_player_stats(stats);
                let path = store.save(&snap)?;
                Ok(json!({ "status": "ok", "path": path.to_string_lossy() }))
            }
        }
    })
    .await;

    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

/// 对局前瞻：批量查玩家战绩，或用回放提取双方阵容后分析强度。
async fn prematch_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let config = match state.config.load() {
        Ok(c) => c,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response()
        }
    };
    let app_id = config.wg_api.application_id.clone();
    let server = config.wg_api.server.clone();
    let names: Vec<String> = req["names"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let replay = req["replay"].as_str().map(|s| s.to_string());

    // 坦克缓存：mtime 未变时复用 AppState 缓存
    let resolver = state.tank_cache.resolver();
    let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
        let client = crate::wargaming::api_client::WgApiClient::new(&app_id, &server);
        let mut threat = String::new();

        if let Some(rp) = &replay {
            let parser = match &resolver {
                Some(r) => crate::replay::parser::ReplayParser::with_resolver(r.as_ref()),
                None => crate::replay::parser::ReplayParser::new(),
            };
            let summary = parser.parse_file(std::path::Path::new(rp))?;
            let mut team_a = Vec::new();
            let mut team_b = Vec::new();
            for p in &summary.players {
                if p.team == 1 {
                    team_a.push(p.nickname.clone());
                } else {
                    team_b.push(p.nickname.clone());
                }
            }
            threat = format!("team_a={} team_b={}", team_a.len(), team_b.len());
        }

        let mut players = Vec::new();
        for n in &names {
            if let Ok(results) = client.search_player(n, true) {
                if let Some((_, id)) = results.first() {
                    if let Ok(stats) = client.get_player_stats(*id) {
                        players.push(stats);
                    }
                }
            }
        }
        // LineupReport 序列化返回（威胁/薄弱点/建议）；查不到玩家时保持 null
        let lineup = if players.is_empty() {
            Value::Null
        } else {
            serde_json::to_value(crate::wargaming::prematch::analyze_lineup(players)?)?
        };
        Ok(json!({
            "status": "ok",
            "lineup": lineup,
            "info": threat,
        }))
    })
    .await;

    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("{e}"),
        )
            .into_response(),
    }
}

/// 最近一次 /api/replay/shots 的响应缓存，供内嵌 3D 查看器拉取。
static LAST_REPLAY_SHOTS: std::sync::OnceLock<std::sync::Mutex<Value>> = std::sync::OnceLock::new();

/// 解析单个回放文件，返回每发射击的复现数据（双方位置/朝向/伤害/目标）。
/// POST { file: "..." } → [{ index, time_s, damage, target_name, is_kill, shooter_pos, shooter_ang, target_pos, target_ang }]
async fn replay_shots_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    let file = body["file"].as_str().unwrap_or("").trim().to_string();
    if file.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing file").into_response();
    }
    // Windows / WSL 两种运行版本的路径风格按配置转换（replay.path_translate，默认 auto）；
    // 配置读取失败按 auto 兜底，不让路径转换阻断解析。
    let file = match state.config.load() {
        Ok(c) => c.replay.translate(&file),
        Err(e) => {
            eprintln!("[replay_shots] config load failed, pathTranslate=auto fallback: {e}");
            crate::models::config::ReplayConfig::translate_with_mode(&file, "auto")
        }
    };
    let path = std::path::PathBuf::from(&file);
    if !path.exists() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            format!("replay not found: {}", file),
        )
            .into_response();
    }

    // 解包 / 全量包解析 / 双路射击提取 / 配置富化是 CPU+IO 重活——整体挪
    // spawn_blocking，避免占死 async worker（player/scan/snapshot 同此纪律）
    let tank_cache = state.tank_cache.clone();
    match tokio::task::spawn_blocking(move || replay_shots_blocking(path, file, tank_cache)).await {
        Ok(resp) => resp,
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("task join failed: {}", e),
        )
            .into_response(),
    }
}

/// [`replay_shots_handler`] 的阻塞实现（spawn_blocking 内执行）。
fn replay_shots_blocking(
    path: std::path::PathBuf,
    file: String,
    tank_cache: Arc<TankCache>,
) -> Response {
    use wotbreplay_parser::replay::Replay;
    let f = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("open failed: {}", e),
            )
                .into_response()
        }
    };
    let mut replay = match Replay::open(f) {
        Ok(r) => r,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("open failed: {}", e),
            )
                .into_response()
        }
    };
    let meta = replay.read_meta().ok();
    let author_tank_id = meta.as_ref().map(|m| m.tank_id as u32).unwrap_or(0);
    let data = match replay.read_data() {
        Ok(d) => d,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("read_data failed: {}", e),
            )
                .into_response()
        }
    };
    let raw_packets: Vec<(u32, f32, &[u8])> = data
        .packets
        .iter()
        .map(|pkt| {
            let t = match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => {
                    *packet_type
                }
            };
            (t, pkt.clock_secs, &pkt.raw_payload[..])
        })
        .collect();

    eprintln!("[replay_shots] file={}", file);
    eprintln!("[replay_shots] packets={}", raw_packets.len());
    let timeline = crate::replay::combat::CombatTimeline::parse_packets(&raw_packets);
    eprintln!("[replay_shots] entities={}", timeline.entity_count);
    let author_eid = *timeline
        .entity_names
        .iter()
        .find(|(eid, _)| {
            timeline.events.iter().any(|e| {
                e.entity_id == **eid
                    && matches!(
                        e.event_type,
                        crate::replay::combat::CombatEventType::DamageCounter { .. }
                    )
            })
        })
        .map(|(eid, _)| eid)
        .unwrap_or(&0);
    let shots = timeline.infer_shots(author_eid);
    eprintln!(
        "[replay_shots] author_eid={:08x} shots={}",
        author_eid,
        shots.len()
    );
    // 作者昵称来自回放自身（battle_results author→花名册；meta.player_name 兜底），不依赖文件名
    let br = replay.read_battle_results().ok();
    let author_nick = br
        .as_ref()
        .and_then(|br| {
            br.players
                .iter()
                .find(|p| p.account_id == br.author.account_id)
                .map(|p| p.info.nickname.clone())
        })
        .or_else(|| meta.as_ref().map(|m| m.player_name.clone()))
        .unwrap_or_default();
    // 双方炮管俯仰的车型极限锚定表（battle_results 昵称→tank_id × TankResolver 极限，
    // 实际搭载 comp blob 优先对号）；comps 与后面的实际搭载配置注入共用
    let valid_tanks: Vec<u32> = br
        .as_ref()
        .map(|br| br.player_results.iter().map(|pr| pr.info.tank_id).collect())
        .unwrap_or_default();
    let comps = crate::replay::playback::collect_comp_descriptors(&raw_packets, &valid_tanks);
    let pitch_limits = br
        .as_ref()
        .and_then(|br| {
            tank_cache
                .resolver()
                .map(|r| r.pitch_limits_from_battle_results(br, &comps))
        })
        .unwrap_or_default();
    // fail-fast：提取失败直接返回 500 + 错误信息（前端可见），不做静默降级。
    // 一次共享扫描完成作者严格 + 他人宽松两路
    let (mut all_shots, others) = match crate::replay::combat::extract_all_shots_auto_with_limits(
        &raw_packets,
        &author_nick,
        0,
        &pitch_limits,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[replay_shots] 提取失败: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("射击复现数据提取失败: {}", e)})),
            )
                .into_response();
        }
    };
    eprintln!(
        "[replay_shots] shot_replay={} total_launches(others)={}",
        all_shots.iter().filter(|s| s.is_author).count(),
        others.total_launches
    );
    // 数据边界提示（前端射击列表头部展示）：他人路径收录覆盖 + 跳过/兜底统计
    let mut extraction_notes: Vec<String> = Vec::new();
    if others.total_launches > 0 {
        extraction_notes.push(format!(
            "其他玩家弹丸已收录 {}/{} 发",
            others.shots.len(),
            others.total_launches
        ));
    }
    if others.skipped_no_endpoint > 0 {
        extraction_notes.push(format!(
            "其他玩家 {} 发因弹道终点未转发被跳过",
            others.skipped_no_endpoint
        ));
    }
    if others.skipped_no_target_state > 0 {
        extraction_notes.push(format!(
            "其他玩家 {} 发因受击方状态缺失被跳过",
            others.skipped_no_target_state
        ));
    }
    if others.muzzle_fallback > 0 {
        extraction_notes.push(format!(
            "其他玩家 {} 发射手位置用炮口坐标兜底",
            others.muzzle_fallback
        ));
    }
    all_shots.extend(others.shots);
    all_shots.sort_by(|a, b| a.time_s.partial_cmp(&b.time_s).unwrap());
    for (i, s) in all_shots.iter_mut().enumerate() {
        s.index = i + 1;
    }
    // 弹种回填：全局 shell_id → tanks.pb 原始弹种串（作者+他人统一，兜底链各级来源均识别）
    crate::replay::loadout::ShellKindTable::from_tanks_pb().annotate(&mut all_shots);
    eprintln!(
        "[replay_shots] total_shots={} (含其他玩家)",
        all_shots.len()
    );

    // 目标坦克 ID：battle_results 按目标昵称关联（供 3D 查看器打开正确目标车辆）
    // 预构建查找表（循环外一次）：昵称 → (account_id, 队伍)，同名玩家取首个匹配
    let mut nick_info: HashMap<&str, (u32, i32)> = HashMap::new();
    // account_id → tank_id：同样取首个匹配
    let mut tank_by_account: HashMap<u32, u32> = HashMap::new();
    if let Some(br) = br.as_ref() {
        for p in &br.players {
            nick_info
                .entry(p.info.nickname.as_str())
                .or_insert((p.account_id, p.info.team));
        }
        for pr in &br.player_results {
            tank_by_account
                .entry(pr.info.account_id)
                .or_insert(pr.info.tank_id);
        }
    }
    let tank_of = |nick: &str| -> Option<u32> {
        let (aid, _) = nick_info.get(nick)?;
        tank_by_account.get(aid).copied()
    };
    // 玩家 → (队伍 1/2, 坦克 id)：射击者归属与筛选下拉的数据源
    let author_name = all_shots
        .iter()
        .find(|s| s.is_author)
        .map(|s| s.shooter_name.clone())
        .unwrap_or_default();
    let author_team: Option<i32> = nick_info.get(author_name.as_str()).map(|(_, t)| *t);
    let team_tank_of = |nick: &str| -> Option<(i32, u32)> {
        let (aid, team) = *nick_info.get(nick)?;
        Some((team, tank_by_account.get(&aid).copied().unwrap_or(0)))
    };

    // —— 实际搭载配置（comp blob 确定性 → 发射弹种 → 初始血量 证据链，与实时回放共享；
    //    comps 已在俯仰锚定表构建时收集）——
    let initial_hp = crate::replay::combat::collect_initial_hp(&raw_packets);
    let mut player_shells: HashMap<String, Vec<u32>> = HashMap::new();
    for s in &all_shots {
        if s.shell_id == 0 {
            continue;
        }
        let v = player_shells.entry(s.shooter_name.clone()).or_default();
        if !v.contains(&s.shell_id) {
            v.push(s.shell_id);
        }
    }
    let mut nick_hp: HashMap<String, u16> = HashMap::new();
    for (eid, nick) in &timeline.entity_names {
        if let Some((_, hp)) = initial_hp.get(eid) {
            nick_hp.insert(nick.clone(), *hp);
        }
    }
    let mut nick_cfg: HashMap<String, u64> = HashMap::new();
    if let Some(br) = br.as_ref() {
        for p in &br.players {
            let nick = &p.info.nickname;
            let Some(tank) = tank_by_account.get(&p.account_id).copied() else {
                continue;
            };
            if tank == 0 {
                continue;
            }
            let comp = comps.get(nick).and_then(|c| {
                ((c.tank_id & 0xFFFF) == (tank & 0xFFFF)).then_some((c.turret_local, c.gun_local))
            });
            let shells = player_shells.get(nick).map(|v| v.as_slice()).unwrap_or(&[]);
            let hp = nick_hp.get(nick).copied().unwrap_or(0);
            if let Some((idx, _, _)) =
                crate::wargaming::tank_configs::resolve_config_index(tank, comp, shells, hp)
            {
                nick_cfg.insert(nick.clone(), idx as u64);
            }
        }
    }
    let enriched: Vec<Value> = all_shots
        .iter()
        .map(|s| {
            let mut v = serde_json::to_value(s).unwrap_or(json!(null));
            if let Some(tid) = tank_of(&s.target_name) {
                v["target_tank_id"] = json!(tid);
            }
            if let Some((team, tid)) = team_tank_of(&s.shooter_name) {
                v["shooter_tank_id"] = json!(tid);
                if let Some(at) = author_team {
                    v["shooter_team"] = json!(if team == at { "ally" } else { "enemy" });
                }
            }
            // 实际搭载配置（build_configs 数组下标）——3D 查看器按此选炮塔/主炮变体
            if let Some(idx) = nick_cfg.get(&s.target_name) {
                v["target_config_idx"] = json!(idx);
            }
            if let Some(idx) = nick_cfg.get(&s.shooter_name) {
                v["shooter_config_idx"] = json!(idx);
            }
            // 发射弹种解析注入（按射手实际搭载配置的弹表）：shell_id → 配置内下标 → 完整弹
            // 数据。消费面只读 shell/shooter_shell_idx；shooter_shell_idx 语义 =
            // shooter_shell_cfg_idx 配置内的弹下标（与前端 &scfg= 选定的弹表同域）。
            let shooter_tank = team_tank_of(&s.shooter_name).map(|(_, t)| t);
            let shooter_cfg = nick_cfg.get(&s.shooter_name).copied().map(|x| x as usize);
            if s.shell_id != 0 {
                if let Some(st) = shooter_tank {
                    if let Some((ci, si, sh)) =
                        crate::wargaming::tank_configs::resolve_shell_by_global_id(
                            st,
                            s.shell_id,
                            shooter_cfg,
                        )
                    {
                        v["shooter_shell_cfg_idx"] = json!(ci);
                        v["shooter_shell_idx"] = json!(si);
                        v["shell"] = sh;
                    }
                }
            }
            v
        })
        .collect();
    // 玩家列表（name/team/tank_id/is_author）：前端射击者筛选下拉的数据源。
    // fire_events = method0x00 开火事件数（全场广播、AoI 独立裁剪）；与已提取射击数之差 = 未收录发数（method29 缺失，弹道无法复现）
    let fire_events: HashMap<String, u32> = {
        let mut m: HashMap<u32, u32> = HashMap::new();
        for pkt in raw_packets.iter() {
            let (t, _, p) = pkt;
            if *t != 8 || p.len() < 12 {
                continue;
            }
            if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x00 {
                continue;
            }
            *m.entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
                .or_insert(0) += 1;
        }
        m.into_iter()
            .filter_map(|(eid, c)| timeline.entity_names.get(&eid).map(|n| (n.clone(), c)))
            .collect()
    };
    let players: Vec<Value> = br
        .as_ref()
        .map(|br| {
            br.players
                .iter()
                .map(|p| {
                    let tid = tank_by_account.get(&p.account_id).copied().unwrap_or(0);
                    json!({
                        "name": p.info.nickname,
                        "team": p.info.team,
                        "tank_id": tid,
                        "is_author": p.info.nickname == author_name,
                        "fire_events": fire_events.get(&p.info.nickname).copied().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // 作者坦克弹种表（按弹药槽位顺序）；查不到坦克时给空数组，前端降级显示"槽N"。
    let author_shells: Vec<Value> = if author_tank_id != 0 {
        tank_cache
            .resolver()
            .and_then(|r| {
                r.resolve_info(author_tank_id).map(|info| {
                    info.shells
                        .iter()
                        .map(|sh| {
                            json!({
                                "shell_type": sh.shell_type,
                                "penetration": sh.penetration,
                                "damage": sh.damage,
                            })
                        })
                        .collect()
                })
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let v = json!({
        "shots": enriched,
        "author_tank_id": author_tank_id,
        "author_shells": author_shells,
        "players": players,
        "extraction_notes": extraction_notes,
    });
    *LAST_REPLAY_SHOTS
        .get_or_init(|| std::sync::Mutex::new(json!([])))
        .lock()
        .unwrap() = v.clone();

    Json(v).into_response()
}

/// 全场实时回放数据端点：与 `/api/replay/shots` 同款路径参数 + path_translate 转换，
/// 构建结果按路径缓存（playback_viewer 内部），gzip 响应。
async fn playback_data_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    let file = body["file"].as_str().unwrap_or("").trim().to_string();
    if file.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing file").into_response();
    }
    let file = match state.config.load() {
        Ok(c) => c.replay.translate(&file),
        Err(e) => {
            eprintln!("[playback] config load failed, pathTranslate=auto fallback: {e}");
            crate::models::config::ReplayConfig::translate_with_mode(&file, "auto")
        }
    };
    let path = std::path::PathBuf::from(&file);
    if !path.exists() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            format!("replay not found: {}", file),
        )
            .into_response();
    }
    crate::wargaming::playback_viewer::playback_data_response(&path, state.tank_cache.resolver())
        .await
}

/// 内嵌 3D 查看器的复现数据端点：返回最近一次解析的射击复现数据。
async fn replay_shots_embedded_handler() -> Response {
    let v = LAST_REPLAY_SHOTS
        .get_or_init(|| std::sync::Mutex::new(json!([])))
        .lock()
        .unwrap()
        .clone();
    Json(v).into_response()
}

async fn tanks_handler(axum::extract::State(state): axum::extract::State<AppState>) -> Response {
    // 坦克缓存：mtime 未变时复用 AppState 缓存
    let cache = state.tank_cache.json();

    let mut out: Vec<Value> = crate::wargaming::blitzkit::load_tanks()
        .values()
        .map(|t| {
            let id = t.tank_id as u64;
            let id_s = id.to_string();
            let info = cache.get(&id_s);
            let hp = info.and_then(|i| i.get("hp")).and_then(|v| v.as_u64());
            let pen = info
                .and_then(|i| i.get("shells"))
                .and_then(|v| v.as_array())
                .and_then(|a| {
                    a.iter()
                        .filter_map(|s| s.get("penetration").and_then(|p| p.as_u64()))
                        .max()
                });
            let premium = info
                .and_then(|i| i.get("is_premium"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let armor = info.and_then(|i| i.get("armor"));
            let hull_front = armor
                .and_then(|a| a.get("hull_front"))
                .and_then(|v| v.as_u64());
            let turret_front = armor
                .and_then(|a| a.get("turret_front"))
                .and_then(|v| v.as_u64());
            let view_range = info
                .and_then(|i| i.get("view_range"))
                .and_then(|v| v.as_f64());
            let speed_forward = info
                .and_then(|i| i.get("speed_forward"))
                .and_then(|v| v.as_f64());
            let speed_reverse = info
                .and_then(|i| i.get("speed_reverse"))
                .and_then(|v| v.as_f64());
            let hull_traverse = info
                .and_then(|i| i.get("hull_traverse"))
                .and_then(|v| v.as_f64());
            let dmg_max = info
                .and_then(|i| i.get("shells"))
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.get("damage").and_then(|d| d.as_f64()))
                        .fold(f64::NEG_INFINITY, f64::max)
                });
            let dmg_max = dmg_max.filter(|m| m.is_finite()).map(|m| m as u64);
            let name = if t.name.is_empty() {
                t.dev_name.clone()
            } else {
                t.name.clone()
            };
            json!({
                "id": id,
                "name": name,
                "tier": t.tier,
                "nation": t.nation.clone(),
                "type": t.tank_type.clone(),
                "hp": hp,
                "is_premium": premium,
                "is_collector": t.is_collector,
                "armor_front": hull_front,
                "armor_turret": turret_front,
                "pen_max": pen,
                "view_range": view_range,
                "damage_max": dmg_max,
                "speed_forward": speed_forward,
                "speed_reverse": speed_reverse,
                "hull_traverse": hull_traverse,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    Json(json!(out)).into_response()
}

async fn tank_detail_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(tank_id): axum::extract::Path<u64>,
) -> Response {
    // 坦克缓存：mtime 未变时复用 AppState 缓存
    let cache = state.tank_cache.json();
    let info = cache
        .get(tank_id.to_string())
        .cloned()
        .unwrap_or(Value::Null);

    let name = info
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let tier = info.get("tier").and_then(|v| v.as_u64()).unwrap_or(0);
    let ttype = info
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let nation = info
        .get("nation")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let is_premium = info
        .get("is_premium")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let armor = info.get("armor").cloned();
    let shells: Vec<Value> = info
        .get("shells")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let dmg_max = shells
        .iter()
        .filter_map(|s| s.get("damage").and_then(|d| d.as_f64()))
        .fold(f64::NEG_INFINITY, f64::max);
    let dmg_max = if dmg_max.is_finite() {
        Some(dmg_max as u64)
    } else {
        None
    };

    let configs = crate::wargaming::tank_configs::build_configs(tank_id as u32);
    // 收藏车标记来自 tanks.pb field13==2（tank_cache.json 不含此字段）
    let is_collector = crate::wargaming::blitzkit::tank_full(tank_id as u32)
        .map(|t| t.is_collector)
        .unwrap_or(false);

    Json(json!({
        "id": tank_id,
        "name": name,
        "tier": tier,
        "type": ttype,
        "nation": nation,
        "is_premium": is_premium,
        "is_collector": is_collector,
        "hp": info.get("hp"),
        "speed_forward": info.get("speed_forward"),
        "speed_reverse": info.get("speed_reverse"),
        "hull_traverse": info.get("hull_traverse"),
        "view_range": info.get("view_range"),
        "turret_traverse_speed": info.get("turret_traverse_speed"),
        "gun_depression": info.get("gun_depression"),
        "gun_elevation": info.get("gun_elevation"),
        "armor": armor,
        "shells": shells,
        "damage_max": dmg_max,
        "configs": configs.as_ref(),
        "image": format!("/api/tank_image/{}", tank_id),
    }))
    .into_response()
}

/// Agent 工具生成的截图（screenshots/，render_heatmap 输出）；仅允许纯文件名，防路径穿越。
async fn screenshots_handler(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    if path.contains("..") || path.contains('/') || path.contains('\\') {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid path").into_response();
    }
    let full = crate::data::data_path("cache/screenshots").join(&path);
    match std::fs::read(&full) {
        Ok(bytes) => {
            let ct = if path.ends_with(".png") {
                "image/png"
            } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                "image/jpeg"
            } else {
                "application/octet-stream"
            };
            ([(axum::http::header::CONTENT_TYPE, ct)], bytes).into_response()
        }
        Err(_) => (
            axum::http::StatusCode::NOT_FOUND,
            format!("screenshot not found: {}", path),
        )
            .into_response(),
    }
}

// ---------- 模型库（GLB）状态与批量下载 ----------

/// 模型库状态：批量下载进度（PROGRESS 全局原子量）+ 车辆就绪度
/// （models.pb 清单内 data/cache/models 两文件齐全的数量；一次目录枚举，轻量）。
async fn models_status_handler() -> Response {
    let (running, total, done, downloaded, cached, failed, bytes) =
        crate::wargaming::model_fetch::PROGRESS.snapshot();
    let (ready_tanks, total_tanks) = tokio::task::spawn_blocking(|| {
        let ids = crate::wargaming::blitzkit::load_model_ids();
        let ready = ids
            .iter()
            .filter(|&&id| {
                crate::wargaming::tank_configs::model_cache_path(id, "model.glb").exists()
                    && crate::wargaming::tank_configs::model_cache_path(id, "collision.glb")
                        .exists()
            })
            .count();
        (ready, ids.len())
    })
    .await
    .unwrap_or((0, 0));
    Json(json!({
        "running": running,
        "total_files": total, "done_files": done,
        "downloaded": downloaded, "cached": cached, "failed": failed,
        "bytes": bytes,
        "ready_tanks": ready_tanks, "total_tanks": total_tanks,
    }))
    .into_response()
}

/// 启动全量模型预下载（非阻塞；进度轮询 /api/models/status；已在校验下载中则忽略）。
async fn models_download_all_handler() -> Response {
    if crate::wargaming::model_fetch::PROGRESS.running() {
        return Json(json!({ "status": "already_running" })).into_response();
    }
    tokio::spawn(async {
        if let Err(e) = crate::wargaming::model_fetch::fetch_all_models(false, 8).await {
            eprintln!("[models-download] failed: {e:#}");
        }
    });
    Json(json!({ "status": "started" })).into_response()
}

/// 浏览器端文件导入：接收 .wotbreplay 原始字节（?name=文件名），保存到配置的回放目录，
/// 返回落盘路径。
async fn replay_upload_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Response {
    if body.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "empty upload").into_response();
    }
    let raw = q
        .get("name")
        .cloned()
        .unwrap_or_else(|| "imported.wotbreplay".into());
    let mut name = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("imported.wotbreplay")
        .to_string();
    if !name.ends_with(".wotbreplay") {
        name.push_str(".wotbreplay");
    }
    let config = match state.config.load() {
        Ok(c) => c,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("{e}"),
            )
                .into_response()
        }
    };
    let dir = config.replay.translate(&config.replay.replay_dir);
    let dir = std::path::PathBuf::from(dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("mkdir failed: {e}"),
        )
            .into_response();
    }
    let path = dir.join(&name);
    let bytes = body.len();
    let display = path.to_string_lossy().to_string();
    // 写盘挪 blocking（async 上下文直接 fs::write 全量 body 会占 worker）
    match tokio::task::spawn_blocking(move || std::fs::write(&path, &body)).await {
        Ok(Ok(_)) => Json(json!({ "path": display, "bytes": bytes })).into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("save failed: {e}"),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("save task failed: {e}"),
        )
            .into_response(),
    }
}
