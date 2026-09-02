use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    routing::{get, post},
    response::{Html, IntoResponse, Response},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::agent::{Agent, AgentEvent};

// =====================================================================
//  Web 图形界面后端（axum 服务器）
//  提供 Agent 对话（SSE 事件流）、玩家/回放/对比/前瞻/配置/用量等面板 API，
//  以及与 CLI 共用的业务逻辑。`wotb-agent web` 启动。
// =====================================================================

/// 一个运行中的 Agent 会话：Agent 本体 + 已产生事件的队列。
/// 事件队列被 `Mutex` 保护，供对话任务写入、前端轮询读取。
struct Session {
    agent: Agent,
    events: Arc<Mutex<Vec<AgentEvent>>>,
}

/// 全局共享状态：配置路径 + 会话表（按 session_id 区分）。
#[derive(Clone)]
struct AppState {
    config_path: std::path::PathBuf,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
}

/// 启动 Web 服务器（绑定随机端口，自动打开浏览器）。
pub async fn serve(config_path: std::path::PathBuf) -> anyhow::Result<()> {
    let state = AppState {
        config_path,
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };

    // 坦克解析器（供内嵌 3D 装甲检视）。
    let viewer_resolver = crate::wargaming::tank_resolver::TankResolver::load_from_json_file(
        crate::data::data_path("tank_cache.json").as_path(),
    )
    .ok()
    .unwrap_or_default();
    // 让 3D 查看器 handler 用全局 resolver（惰性加载 tank_cache.json）。
    crate::wargaming::viewer::set_global_resolver(viewer_resolver);

    // 注册所有路由
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/tank/{tank_id}", get(tank_detail_page_handler))
        .route("/api/chat", post(chat_handler))
        .route("/api/chat/events", get(chat_events_handler))
        .route("/api/chat/cancel", post(chat_cancel_handler))
        .route("/api/session", get(session_get))
        .route("/api/config", get(config_get).post(config_set))
        .route("/api/usage", get(usage_get))
        .route("/api/player/{nickname}", get(player_handler))
        .route("/api/scan", post(scan_handler))
        .route("/api/snapshot", post(snapshot_handler))
        .route("/api/prematch", post(prematch_handler))
        .route("/api/tanks", get(tanks_handler))
        .route("/api/tank_detail/{tank_id}", get(tank_detail_handler))
        .route("/api/tank_image/{tank_id}", get(tank_image_handler))
        .route("/api/vendor/{*path}", get(vendor_handler))
        // 内嵌 3D 装甲检视：页面入口 + 其 API/模型路由（复用 3D 查看器 handler）。
        .route("/armor_view/view/{tank_id}", get(armor_view_handler))
        .route("/armor_view/", get(armor_view_root))
        .route("/armor_view/glb/{tank_id}/{filename}", get(crate::wargaming::viewer::glb_handler))
        .route("/armor_view/vendor/three/{*path}", get(crate::wargaming::viewer::vendor_handler))
        .route("/armor_view/api/tank/{tank_id}", get(armor_tank_data_handler))
        .route("/armor_view/api/tank_filter", get(crate::wargaming::viewer::tank_filter_handler))
        .route("/armor_view/api/tank_image/{tank_id}", get(crate::wargaming::viewer::tank_image_handler))
        .route("/armor_view/api/shells/{tank_id}", get(crate::wargaming::viewer::shells_handler))
        .route("/armor_view/api/penetrate", post(crate::wargaming::viewer::penetrate_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 0));
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

/// 首页：返回内嵌的前端 HTML。
async fn index_handler() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

/// 独立坦克详情页（坦克百科卡片点击后在新窗口打开）。
async fn tank_detail_page_handler(axum::extract::Path(_tank_id): axum::extract::Path<u64>) -> Html<&'static str> {
    Html(include_str!("tank_detail.html"))
}

/// 内嵌 3D 装甲检视页面（tank_id 从路径取），供坦克百科详情弹窗用 iframe 加载。
async fn armor_view_handler(axum::extract::Path(tank_id): axum::extract::Path<u64>) -> Html<String> {
    Html(crate::wargaming::viewer::viewer_index_html(tank_id as u32, tank_id as u32, "/armor_view"))
}

/// `/armor_view/` 根：重定向到默认坦克（IS-7）的检视页。
async fn armor_view_root() -> axum::response::Redirect {
    axum::response::Redirect::temporary("/armor_view/view/7169")
}

/// 3D 检视的坦克数据：调用 viewer 核心逻辑并给 model_url 加 `/armor_view` 前缀，
/// 否则 iframe 内前端会去请求顶层的 `/glb/...`（404）。
async fn armor_tank_data_handler(axum::extract::Path(tank_id): axum::extract::Path<u64>) -> Json<Value> {
    Json(crate::wargaming::viewer::tank_data_value_prefixed(tank_id as u32, "/armor_view"))
}

// ---------- Agent 对话 ----------

/// 发起一次对话：创建（或复用）会话，后台启动 Agent 循环，返回 session_id。
async fn chat_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let session_id = req["session_id"].as_str().unwrap_or("default").to_string();
    let text = req["message"].as_str().unwrap_or("").to_string();
    if text.trim().is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "empty message").into_response();
    }

    // 取或建该会话的 Agent（tokio Mutex，跨 await 持锁保证单会话串行）
    {
        let mut guard = state.sessions.lock().await;
        if !guard.contains_key(&session_id) {
            match Agent::new(&state.config_path) {
                Ok(agent) => {
                    guard.insert(session_id.clone(), Session {
                        agent,
                        events: Arc::new(Mutex::new(Vec::new())),
                    });
                }
                Err(e) => {
                    return (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to init agent: {e}")).into_response();
                }
            }
        }
    }

    // 后台任务驱动 Agent 循环，事件写入会话队列（用 try_lock 避免卡住循环）
    let state2 = state.clone();
    let (s_id, msg) = (session_id.clone(), text);
    tokio::spawn(async move {
        let mut guard = state2.sessions.lock().await;
        let Some(sess) = guard.get_mut(&s_id) else { return };
        let sink = sess.events.clone();
        let sink2 = sink.clone();
        let res = sess.agent.chat_async(&msg, move |e| {
            let mut ev = sink.try_lock();
            if let Ok(ref mut v) = ev {
                v.push(e);
            }
        }).await;
        if let Err(e) = res {
            let mut ev = sink2.try_lock();
            if let Ok(ref mut v) = ev {
                v.push(AgentEvent::Error { message: e.to_string() });
            }
        }
    });

    Json(json!({ "session_id": session_id, "status": "started" })).into_response()
}

/// 拉取某会话的事件流（前端约每 400ms 轮询一次，增量渲染进度）。
async fn chat_events_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let session_id = q.get("session_id").cloned().unwrap_or("default".into());
    let guard = state.sessions.lock().await;
    let Some(sess) = guard.get(&session_id) else {
        return Json(json!({ "events": [], "done": true })).into_response();
    };
    let events = sess.events.lock().await;
    let arr: Vec<Value> = events.iter().map(|e| serde_json::to_value(e).unwrap_or(Value::Null)).collect();
    Json(json!({ "events": arr })).into_response()
}

/// 打断当前会话的任务（设置全局中断标志）。
async fn chat_cancel_handler(
    axum::extract::State(_state): axum::extract::State<AppState>,
) -> Response {
    crate::agent::set_interrupted();
    Json(json!({ "status": "cancelling" })).into_response()
}

// ---------- 会话 ----------

/// 读取某会话的消息历史（不含系统提示）。
async fn session_get(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let session_id = q.get("session_id").cloned().unwrap_or("default".into());
    let guard = state.sessions.lock().await;
    let Some(sess) = guard.get(&session_id) else {
        return Json(json!({ "messages": [] })).into_response();
    };
    let history: Vec<Value> = sess.agent.history().iter()
        .map(|m| serde_json::to_value(m).unwrap_or(Value::Null)).collect();
    Json(json!({ "messages": history })).into_response()
}

// ---------- 配置 / 用量 ----------

/// 读取当前配置（返回给设置页）。
async fn config_get(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    match crate::models::config::Config::load_or_create(&state.config_path) {
        Ok(c) => Json(json!({
            "llm": { "model": c.llm.model, "endpoint": c.llm.endpoint,
                     "context_length": c.llm.context_length,
                     "thinking_mode": c.llm.thinking_mode,
                     "max_tokens": c.llm.max_tokens,
                     "budget": c.llm.budget,
                     // 仅回显是否已设置 key，不回显明文，避免泄露。
                     "api_key_set": !c.llm.api_key.is_empty() },
            "wg_api": { "server": c.wg_api.server },
            "replay": { "replay_dir": c.replay.replay_dir },
        })).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    }
}

/// 保存配置（从设置页表单字段更新并写回 config.toml）。
async fn config_set(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let mut c = match crate::models::config::Config::load_or_create(&state.config_path) {
        Ok(c) => c,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    };
    if let Some(model) = req["model"].as_str() { c.llm.model = model.into(); }
    if let Some(endpoint) = req["endpoint"].as_str() { c.llm.endpoint = endpoint.into(); }
    if let Some(api_key) = req["api_key"].as_str() { c.llm.api_key = api_key.into(); }
    if let Some(ctx) = req["context_length"].as_u64() { c.llm.context_length = ctx as u32; }
    if let Some(thinking) = req["thinking_mode"].as_bool() { c.llm.thinking_mode = thinking; }
    if let Some(max) = req["max_tokens"].as_u64() { c.llm.max_tokens = Some(max as u32); }
    if let Some(budget) = req["budget"].as_f64() { c.llm.budget = Some(budget); }
    match c.save(&state.config_path) {
        Ok(_) => Json(json!({ "status": "saved" })).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    }
}

/// 返回 Token 用量统计（供用量面板）。
async fn usage_get(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let usage = crate::models::config::TokenUsage::load_from_file(
        std::path::Path::new("token_usage.json")).unwrap_or_default();
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
    })).into_response()
}

// ---------- 玩家查询 ----------

/// 按昵称查询玩家战绩（返回前 10 个匹配玩家的统计）。
async fn player_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(nickname): axum::extract::Path<String>,
) -> Response {
    let config = match crate::models::config::Config::load_or_create(&state.config_path) {
        Ok(c) => c,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
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
    }).await;

    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    }
}

// ---------- 回放扫描 ----------

/// 按模式/日期扫描回放目录，返回聚合报告 JSON。
async fn scan_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let config = match crate::models::config::Config::load_or_create(&state.config_path) {
        Ok(c) => c,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    };
    let replay_dir = req["dir"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string())
        .unwrap_or(config.replay.replay_dir.clone());
    let mode = req["mode"].as_str().unwrap_or("all").to_string();
    let days = req["days"].as_i64();

    let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
        let filter = crate::replay::scanner::ScanFilter::from_mode(&mode, days);
        // 复用坦克解析器（若 tank_cache.json 存在）以翻译坦克名
        let resolver = crate::wargaming::tank_resolver::TankResolver::load_from_json_file(
            crate::data::data_path("tank_cache.json").as_path()).ok();
        let scanner = match &resolver {
            Some(r) => crate::replay::scanner::ReplayScanner::with_resolver(r),
            None => crate::replay::scanner::ReplayScanner::new(),
        };
        let battles = scanner.scan_dir(std::path::Path::new(&replay_dir), &filter, |_| {})?;
        let report = crate::models::report::AggregatedReport::from_battles(&battles, &mode);
        Ok(serde_json::to_value(&report).unwrap_or(Value::Null))
    }).await;

    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    }
}

// ---------- 快照 ----------

/// 快照操作：take（采集）/ list（列出）/ diff（新旧差值）。
async fn snapshot_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let config = match crate::models::config::Config::load_or_create(&state.config_path) {
        Ok(c) => c,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    };
    let app_id = config.wg_api.application_id.clone();
    let server = config.wg_api.server.clone();
    let nickname = req["nickname"].as_str().unwrap_or("").to_string();
    let action = req["action"].as_str().unwrap_or("take").to_string();
    let dir = req["dir"].as_str().unwrap_or("snapshots").to_string();

    // WG API 是阻塞调用，放到 spawn_blocking 避免阻塞 tokio 运行时
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
                let arr: Vec<Value> = snaps.iter().map(|s| json!({
                    "timestamp": s.timestamp, "datetime": s.datetime,
                    "nickname": s.player.nickname,
                    "battles": s.player.rating_battles,
                    "mm_rating": s.player.rating_mm_rating,
                })).collect();
                Ok(json!({ "status": "ok", "snapshots": arr }))
            }
            "diff" => {
                let from = store.oldest()?.ok_or_else(|| anyhow::anyhow!("no snapshots"))?;
                let to = store.latest()?.ok_or_else(|| anyhow::anyhow!("no snapshots"))?;
                let diff = store.diff(&from, &to);
                Ok(serde_json::to_value(&diff).unwrap_or(Value::Null))
            }
            _ => {
                let snap = crate::wargaming::snapshot::Snapshot::from_player_stats(stats);
                let path = store.save(&snap)?;
                Ok(json!({ "status": "ok", "path": path.to_string_lossy() }))
            }
        }
    }).await;

    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    }
}

// ---------- 对局前瞻 ----------

/// 对局前瞻：批量查玩家战绩，或用回放提取双方阵容后分析强度。
async fn prematch_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<Value>,
) -> Response {
    let config = match crate::models::config::Config::load_or_create(&state.config_path) {
        Ok(c) => c,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    };
    let app_id = config.wg_api.application_id.clone();
    let server = config.wg_api.server.clone();
    let names: Vec<String> = req["names"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let replay = req["replay"].as_str().map(|s| s.to_string());

    let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
        let client = crate::wargaming::api_client::WgApiClient::new(&app_id, &server);
        let mut threat = String::new();
        let mut my_avg: Option<f64> = None;

        // 若传入回放，则解析出双方阵容
        if let Some(rp) = &replay {
            let resolver = crate::wargaming::tank_resolver::TankResolver::load_from_json_file(
                crate::data::data_path("tank_cache.json").as_path()).ok();
            let parser = match &resolver {
                Some(r) => crate::replay::parser::ReplayParser::with_resolver(r),
                None => crate::replay::parser::ReplayParser::new(),
            };
            let summary = parser.parse_file(std::path::Path::new(rp))?;
            let mut team_a = Vec::new();
            let mut team_b = Vec::new();
            for p in &summary.players {
                if p.team == 1 { team_a.push(p.nickname.clone()); }
                else { team_b.push(p.nickname.clone()); }
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
        if !players.is_empty() {
            let report = crate::wargaming::prematch::analyze_lineup(players)?;
            my_avg = Some(report.avg_damage as f64);
        }
        Ok(json!({
            "status": "ok",
            "report": if my_avg.is_some() { "generated" } else { "empty" },
            "info": threat,
        }))
    }).await;

    match res {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    }
}

// ---------- 坦克百科（Tankopedia）----------

/// 全部坦克列表：id/名称/等级/国家/类型/血量/装甲摘要/主炮穿深，供百科网格与筛选。
/// 数据源：tanks.pb（运行时解析，元数据/名称）+ tank_cache.json（属性）。
async fn tanks_handler() -> Response {
    let cache: Value = std::fs::read_to_string(crate::data::data_path("tank_cache.json")).ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Null);

    let mut out: Vec<Value> = crate::wargaming::blitzkit::load_tanks()
        .into_values().map(|t| {
        let id = t.tank_id as u64;
        let id_s = id.to_string();
        let info = cache.get(&id_s);
        let hp = info.and_then(|i| i.get("hp")).and_then(|v| v.as_u64());
        let pen = info.and_then(|i| i.get("shells")).and_then(|v| v.as_array())
            .and_then(|a| a.iter().filter_map(|s| s.get("penetration").and_then(|p| p.as_u64())).max());
        let premium = info.and_then(|i| i.get("is_premium")).and_then(|v| v.as_bool()).unwrap_or(false);
        let armor = info.and_then(|i| i.get("armor"));
        let hull_front = armor.and_then(|a| a.get("hull_front")).and_then(|v| v.as_u64());
        let turret_front = armor.and_then(|a| a.get("turret_front")).and_then(|v| v.as_u64());
        let name = if t.name.is_empty() { t.dev_name.clone() } else { t.name };
        json!({
            "id": id,
            "name": name,
            "tier": t.tier,
            "nation": t.nation,
            "type": t.tank_type,
            "hp": hp,
            "is_premium": premium,
            "armor_front": hull_front,
            "armor_turret": turret_front,
            "pen_max": pen,
        })
    }).collect();
    out.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });
    Json(json!(out)).into_response()
}

/// 坦克详情：完整属性（元数据/装甲/弹种/俯仰角/血量/速度），供百科详情弹窗。
async fn tank_detail_handler(axum::extract::Path(tank_id): axum::extract::Path<u64>) -> Response {
    let cache: Value = std::fs::read_to_string(crate::data::data_path("tank_cache.json")).ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Null);
    let info = cache.get(tank_id.to_string()).cloned().unwrap_or(Value::Null);

    let name = info.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let tier = info.get("tier").and_then(|v| v.as_u64()).unwrap_or(0);
    let ttype = info.get("type").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let nation = info.get("nation").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
    let is_premium = info.get("is_premium").and_then(|v| v.as_bool()).unwrap_or(false);

    let armor = info.get("armor").cloned();
    // 弹种（默认配置/第一个炮塔）
    let shells: Vec<Value> = info.get("shells").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let dmg_max = shells.iter().filter_map(|s| s.get("damage").and_then(|d| d.as_f64())).fold(f64::NEG_INFINITY, f64::max);
    let dmg_max = if dmg_max.is_finite() { Some(dmg_max as u64) } else { None };

    // 选配模块（炮塔/主炮）配置：复用 3D 查看器的 configs（含口径/弹种/装填/瞄准/散布/DPM/旋转/视野）。
    let configs: Vec<Value> = crate::wargaming::viewer::build_configs(tank_id as u32);

    Json(json!({
        "id": tank_id,
        "name": name,
        "tier": tier,
        "type": ttype,
        "nation": nation,
        "is_premium": is_premium,
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
        "configs": configs,
        "image": format!("/api/tank_image/{}", tank_id),
    })).into_response()
}

/// 坦克封面图代理：`tank_images/` 缓存优先，回退 BlitzKit CDN 并落盘（与 3D 查看器共用缓存目录）。
async fn tank_image_handler(axum::extract::Path(tank_id): axum::extract::Path<u64>) -> Response {
    let dir = std::path::Path::new("tank_images");
    let cache_path = dir.join(format!("{}.webp", tank_id));
    if let Ok(bytes) = std::fs::read(&cache_path) {
        return image_response(bytes);
    }
    let url = format!("https://api.blitzkit.app/tanks/{}/icons/big.webp", tank_id);
    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(bytes) = resp.bytes().await {
                let vec = bytes.to_vec();
                let _ = std::fs::create_dir_all(dir);
                if std::fs::write(&cache_path, &vec).is_ok() {
                    eprintln!("[web-image-cache] cached {} ({} bytes)", cache_path.display(), vec.len());
                }
                return image_response(vec);
            }
            (axum::http::StatusCode::BAD_GATEWAY, "empty image body").into_response()
        }
        Ok(resp) => (axum::http::StatusCode::BAD_GATEWAY, format!("BlitzKit icon returned {}", resp.status())).into_response(),
        Err(e) => (axum::http::StatusCode::BAD_GATEWAY, format!("BlitzKit icon unreachable: {}", e)).into_response(),
    }
}

fn image_response(bytes: Vec<u8>) -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "image/webp")],
        bytes,
    ).into_response()
}

// ---------- 前端静态资源（Chart.js 等，本地 vendored）----------

/// 提供 `web/vendor/` 下的静态文件（带路径穿越防护）。
async fn vendor_handler(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    if path.contains("..") {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid path").into_response();
    }
    let full = std::path::Path::new("web/vendor/").join(&path);
    match std::fs::read(&full) {
        Ok(bytes) => {
            let ct = if path.ends_with(".js") { "application/javascript" }
                else if path.ends_with(".css") { "text/css" }
                else if path.ends_with(".woff2") { "font/woff2" }
                else if path.ends_with(".woff") { "font/woff" }
                else if path.ends_with(".ttf") { "font/ttf" }
                else { "application/octet-stream" };
            ([(axum::http::header::CONTENT_TYPE, ct)], bytes).into_response()
        }
        Err(_) => (axum::http::StatusCode::NOT_FOUND, format!("not found: {}", path)).into_response(),
    }
}
