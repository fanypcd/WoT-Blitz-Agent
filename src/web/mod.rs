use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    routing::{delete, get, post},
    response::{Html, IntoResponse, Response},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::agent::{Agent, AgentEvent};

/// 一个运行中的 Agent 会话：Agent 本体 + 已产生事件的队列。
/// 事件队列被 `Mutex` 保护，供对话任务写入、前端轮询读取。
struct Session {
    agent: Option<Agent>,
    events: Arc<std::sync::Mutex<Vec<AgentEvent>>>,
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

    let viewer_resolver = crate::wargaming::tank_resolver::TankResolver::load_from_json_file(
        crate::data::data_path("tank_cache.json").as_path(),
    )
    .ok()
    .unwrap_or_default();
    crate::wargaming::viewer::set_global_resolver(viewer_resolver);

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/tank/{tank_id}", get(tank_detail_page_handler))
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
        .route("/api/snapshot", post(snapshot_handler))
        .route("/api/prematch", post(prematch_handler))
        .route("/api/tanks", get(tanks_handler))
        .route("/api/tank_detail/{tank_id}", get(tank_detail_handler))
        .route("/api/tank_image/{tank_id}", get(tank_image_handler))
        .route("/api/vendor/{*path}", get(vendor_handler))
        .route("/screenshots/{*path}", get(screenshots_handler))
        .route("/armor_view/view/{tank_id}", get(armor_view_handler))
        .route("/armor_view/", get(armor_view_root))
        .route("/armor_view/glb/{tank_id}/{filename}", get(crate::wargaming::viewer::glb_handler))
        .route("/armor_view/vendor/three/{*path}", get(crate::wargaming::viewer::vendor_handler))
        .route("/armor_view/api/tank/{tank_id}", get(armor_tank_data_handler))
        .route("/armor_view/api/tank_filter", get(crate::wargaming::viewer::tank_filter_handler))
        .route("/armor_view/api/tank_image/{tank_id}", get(crate::wargaming::viewer::tank_image_handler))
        .route("/armor_view/api/shells/{tank_id}", get(crate::wargaming::viewer::shells_handler))
        .route("/armor_view/api/penetrate", post(crate::wargaming::viewer::penetrate_handler))
        .route("/armor_view/api/replay_shot", get(replay_shots_embedded_handler))
        .with_state(state);

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

/// 首页：返回内嵌的前端 HTML。
async fn index_handler() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

/// 独立坦克详情页（坦克百科卡片点击后在新窗口打开）。
async fn tank_detail_page_handler(axum::extract::Path(_tank_id): axum::extract::Path<u64>) -> Html<&'static str> {
    Html(include_str!("tank_detail.html"))
}

/// 内嵌 3D 装甲检视页面（tank_id 从路径取），供坦克百科详情弹窗用 iframe 加载。
async fn armor_view_handler(
    axum::extract::Path(tank_id): axum::extract::Path<u64>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let shooter = q.get("shooter").and_then(|v| v.parse::<u32>().ok()).unwrap_or(tank_id as u32);
    let mut resp = axum::response::Response::builder()
        .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(axum::http::header::CACHE_CONTROL, "no-cache, max-age=0")
        .body(axum::body::Body::from(
            crate::wargaming::viewer::viewer_index_html(tank_id as u32, shooter, "/armor_view"),
        ))
        .unwrap();
    if let Ok(v) = axum::http::HeaderValue::from_str("no-store") { resp.headers_mut().insert(axum::http::header::PRAGMA, v); }
    resp
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

    {
        let mut guard = state.sessions.lock().await;
        if !guard.contains_key(&session_id) {
            match Agent::new(&state.config_path) {
                Ok(agent) => {
                    guard.insert(session_id.clone(), Session {
                        agent: Some(agent),
                        events: Arc::new(std::sync::Mutex::new(Vec::new())),
                    });
                }
                Err(e) => {
                    return (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to init agent: {e}")).into_response();
                }
            }
        }
    }

    let state2 = state.clone();
    let (s_id, msg) = (session_id.clone(), text);
    tokio::spawn(async move {
        // 取出 Agent + 事件队列引用，然后【立即释放】sessions 锁——
        // 否则轮询接口在聊天期间无法读取事件 → 进度只能批量到达
        let (sink, agent) = {
            let mut guard = state2.sessions.lock().await;
            let Some(sess) = guard.get_mut(&s_id) else { return };
            // 新一轮对话：清空事件缓冲（前端 lastEventCount=0 从零读增量，
            // 否则上一轮事件被整体重放——第二句重复第一句的内容）
            if let Ok(mut v) = sess.events.lock() { v.clear(); }
            let sink = sess.events.clone();
            let agent = std::mem::replace(&mut sess.agent, None);
            (sink, agent)
        };
        let Some(mut agent) = agent else { return };
        let sink2 = sink.clone();
        let res = agent.chat_async(&msg, move |e| {
            // std::sync::Mutex：同步回调中短暂 lock，不丢事件
            if let Ok(mut v) = sink.lock() {
                v.push(e);
            }
        }).await;
        if let Err(e) = res {
            if let Ok(mut v) = sink2.lock() {
                v.push(AgentEvent::Error { message: e.to_string() });
            }
        }
        // 归还 Agent
        let mut guard = state2.sessions.lock().await;
        if let Some(sess) = guard.get_mut(&s_id) {
            sess.agent = Some(agent);
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
    let Ok(events) = sess.events.lock() else { return Json(json!({ "events": [] })).into_response() };
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
    let history: Vec<Value> = sess.agent.as_ref().map(|a| {
        a.history().iter()
            .map(|m| serde_json::to_value(m).unwrap_or(Value::Null)).collect::<Vec<_>>()
    }).unwrap_or_default();
    Json(json!({ "messages": history })).into_response()
}

/// 列出全部会话 ID。
async fn sessions_list(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let guard = state.sessions.lock().await;
    let ids: Vec<&String> = guard.keys().collect();
    Json(json!({ "sessions": ids })).into_response()
}

/// 删除指定会话。
async fn session_delete(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    state.sessions.lock().await.remove(&id);
    Json(json!({ "status": "deleted" })).into_response()
}

/// 导出会话为 Markdown（浏览器下载）。
async fn session_export(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let guard = state.sessions.lock().await;
    // 会话不存在 → 返回空 Markdown（而非 404），保证导出按钮始终可用
    let Some(sess) = guard.get(&id) else {
        let empty = format!("# WoTB Agent Session: {}\n\n*（此会话暂无消息）*\n", id);
        return axum::response::Response::builder()
            .header("Content-Type", "text/markdown; charset=utf-8")
            .header("Content-Disposition", format!("attachment; filename=\"{}.md\"", id))
            .body(axum::body::Body::from(empty))
            .unwrap()
            .into_response();
    };
    let mut md = format!("# WoTB Agent Session: {}\n\n", id);
    for m in sess.agent.as_ref().map(|a| a.history()).unwrap_or_default() {
        let v = serde_json::to_value(m).unwrap_or(Value::Null);
        let role = v["role"].as_str().unwrap_or("").to_string();
        // 只导出用户提问 + assistant 最终回答：
        // 跳过 tool 结果、带 tool_calls 的中间轮次、空内容
        if role == "user" {
            let content = v["content"].as_str().unwrap_or("");
            if content.is_empty() { continue; }
            md.push_str(&format!("## 👤 User\n\n{}\n\n---\n\n", content));
        } else if role == "assistant" {
            let has_calls = v["tool_calls"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
            if has_calls { continue; }
            let content = v["content"].as_str().unwrap_or("");
            if content.is_empty() { continue; }
            md.push_str(&format!("## 🤖 Assistant\n\n{}\n\n---\n\n", content));
        }
    }
    if md.trim() == format!("# WoTB Agent Session: {}", id).trim() {
        md.push_str("\n*（此会话暂无消息）*\n");
    }
    drop(guard);
    axum::response::Response::builder()
        .header("Content-Type", "text/markdown; charset=utf-8")
        .header("Content-Disposition", format!("attachment; filename=\"{}.md\"", id))
        .body(axum::body::Body::from(md))
        .unwrap()
        .into_response()
}

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

/// 全部坦克列表：id/名称/等级/国家/类型/血量/装甲摘要/主炮穿深，供百科网格与筛选。
/// 数据源：tanks.pb（运行时解析，元数据/名称）+ tank_cache.json（属性）。
/// 解析单个回放文件，返回每发射击的复现数据（双方位置/朝向/伤害/目标）。
/// POST { file: "..." } → [{ index, time_s, damage, target_name, is_kill, shooter_pos, shooter_ang, target_pos, target_ang }]
static LAST_REPLAY_SHOTS: std::sync::OnceLock<std::sync::Mutex<Value>> = std::sync::OnceLock::new();

async fn replay_shots_handler(axum::Json(body): axum::Json<Value>) -> Response {
    let file = body["file"].as_str().unwrap_or("").trim().to_string();
    if file.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing file").into_response();
    }
    let path = std::path::PathBuf::from(&file);
    if !path.exists() {
        return (axum::http::StatusCode::NOT_FOUND, format!("replay not found: {}", file)).into_response();
    }

    use wotbreplay_parser::replay::Replay;
    let f = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("open failed: {}", e)).into_response(),
    };
    let mut replay = match Replay::open(f) {
        Ok(r) => r,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("open failed: {}", e)).into_response(),
    };
    let meta = replay.read_meta().ok();
    let author_tank_id = meta.as_ref().map(|m| m.tank_id as u32).unwrap_or(0);
    let data = match replay.read_data() {
        Ok(d) => d,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("read_data failed: {}", e)).into_response(),
    };
    let raw_packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        (t, pkt.clock_secs, &pkt.raw_payload[..])
    }).collect();

    eprintln!("[replay_shots] file={}", file);
    eprintln!("[replay_shots] packets={}", raw_packets.len());
    let timeline = crate::replay::combat::CombatTimeline::parse_packets(&raw_packets);
    eprintln!("[replay_shots] entities={}", timeline.entity_count);
    let author_eid = *timeline.entity_names.iter()
        .find(|(eid, _)| timeline.events.iter().any(|e|
            e.entity_id == **eid && matches!(e.event_type, crate::replay::combat::CombatEventType::DamageCounter { .. })))
        .map(|(eid, _)| eid)
        .unwrap_or(&0);
    let shots = timeline.infer_shots(author_eid);
    eprintln!("[replay_shots] author_eid={:08x} shots={}", author_eid, shots.len());
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // fail-fast：提取失败直接返回 500 + 错误信息（前端可见），不做静默降级
    let shot_replay = match crate::replay::combat::extract_shot_replays_auto(&raw_packets, file_name) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[replay_shots] 提取失败: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("射击复现数据提取失败: {}", e)}))).into_response();
        }
    };
    eprintln!("[replay_shots] shot_replay={}", shot_replay.len());

    // 目标坦克 ID：battle_results 按目标昵称关联（供 3D 查看器打开正确目标车辆）
    let br = replay.read_battle_results().ok();
    let tank_of = |nick: &str| -> Option<u32> {
        let br = br.as_ref()?;
        br.players.iter().find(|p| p.info.nickname == nick)
            .and_then(|p| br.player_results.iter().find(|pr| pr.info.account_id == p.account_id))
            .map(|pr| pr.info.tank_id)
    };
    let enriched: Vec<Value> = shot_replay.iter().map(|s| {
        let mut v = serde_json::to_value(s).unwrap_or(json!(null));
        if let Some(tid) = tank_of(&s.target_name) { v["target_tank_id"] = json!(tid); }
        v
    }).collect();
    let v = json!({
        "shots": enriched,
        "author_tank_id": author_tank_id,
    });
    *LAST_REPLAY_SHOTS.get_or_init(|| std::sync::Mutex::new(json!([]))).lock().unwrap() = v.clone();

    Json(v).into_response()
}

/// 内嵌 3D 查看器的复现数据端点：返回最近一次解析的射击复现数据。
async fn replay_shots_embedded_handler() -> Response {
    let v = LAST_REPLAY_SHOTS.get_or_init(|| std::sync::Mutex::new(json!([]))).lock().unwrap().clone();
    Json(v).into_response()
}

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
    let shells: Vec<Value> = info.get("shells").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let dmg_max = shells.iter().filter_map(|s| s.get("damage").and_then(|d| d.as_f64())).fold(f64::NEG_INFINITY, f64::max);
    let dmg_max = if dmg_max.is_finite() { Some(dmg_max as u64) } else { None };

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

/// 提供 `web/vendor/` 下的静态文件（带路径穿越防护）。
/// Agent 工具生成的截图（screenshots/ 目录，render_heatmap 输出）。
async fn screenshots_handler(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    if path.contains("..") || path.contains('/') || path.contains('\\') {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid path").into_response();
    }
    let full = std::path::Path::new("screenshots/").join(&path);
    match std::fs::read(&full) {
        Ok(bytes) => {
            let ct = if path.ends_with(".png") { "image/png" }
                else if path.ends_with(".jpg") || path.ends_with(".jpeg") { "image/jpeg" }
                else { "application/octet-stream" };
            ([(axum::http::header::CONTENT_TYPE, ct)], bytes).into_response()
        }
        Err(_) => (axum::http::StatusCode::NOT_FOUND, format!("screenshot not found: {}", path)).into_response(),
    }
}

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
