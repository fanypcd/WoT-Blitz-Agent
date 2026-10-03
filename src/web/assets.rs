//! 3D 查看器与资产端点的 HTTP 面（自 wargaming/viewer.rs 收敛——表现层归 web/，
//! 见架构债文档第 3 节）：查看器路由、GLB/坦克图片的缓存-下载-伺服链、
//! 坦克数据/弹表/穿透判定端点、热力图就绪门控。

use std::path::Path;

use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::wargaming::heatmap_ready::{mark_session_ready, session_ready};
use crate::wargaming::penetration::{self, PenetrationRequest};
use crate::wargaming::tank_configs::{
    model_cache_path, parse_gun_caliber, set_global_resolver, tank_data_value,
};
use crate::wargaming::tank_resolver::TankResolver;

const GLB_CACHE_DIR: &str = "cache/models";
const GLB_FILES: [&str; 2] = ["collision.glb", "model.glb"];

pub(crate) fn wsl_ip() -> String {
    std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().next().map(|s| s.to_string()))
        .unwrap_or_else(|| "localhost".to_string())
}

pub fn build_viewer_router(
    tank_resolver: TankResolver,
    tank_id: u32,
    shooter_id: Option<u32>,
    base_prefix: &str,
) -> axum::Router {
    set_global_resolver(tank_resolver);

    // 页面已切流至 Vue SPA（crate::web 嵌入产物）；根路径重定向到检视路由。
    // 原 query（headless/shot/shell/scfg 等查看参数）必须透传——307 重定向不合并
    // query，丢了它们无头截图与射击复现参数全部失效
    let _ = base_prefix;
    let redirect_target = match shooter_id {
        Some(s) if s != tank_id => format!("/armor_view/view/{tank_id}?shooter={s}"),
        _ => format!("/armor_view/view/{tank_id}"),
    };
    Router::new()
        .route(
            "/",
            get(move |raw: axum::extract::RawQuery| {
                let base = redirect_target.clone();
                async move {
                    let target = match raw.0 {
                        Some(q) if !q.is_empty() => {
                            format!("{base}{}{q}", if base.contains('?') { "&" } else { "?" })
                        }
                        _ => base,
                    };
                    axum::response::Redirect::temporary(&target)
                }
            }),
        )
        .route(
            "/armor_view/view/{tank_id}",
            get(|| async { crate::web::spa_index_response() }),
        )
        .route(
            "/assets/{*path}",
            get(
                |axum::extract::Path(path): axum::extract::Path<String>| async move {
                    crate::web::spa_asset_response(&path)
                },
            ),
        )
        .route("/glb/{tank_id}/{filename}", get(glb_handler))
        .route("/api/tank/{tank_id}", get(tank_data_handler))
        .route("/api/tank_filter", get(tank_filter_handler))
        .route("/api/tank_image/{tank_id}", get(tank_image_handler))
        .route("/api/shells/{tank_id}", get(shells_handler))
        .route("/api/penetrate", post(penetrate_handler))
        .route("/api/hold", get(hold_handler))
        .route("/api/ready", get(ready_handler))
        .with_state(())
}

pub(crate) async fn ensure_glb_bytes(tank_id: u32, filename: &str) -> Result<Vec<u8>, String> {
    if !GLB_FILES.contains(&filename) {
        return Err(format!("invalid GLB filename: {}", filename));
    }
    let cache_path = model_cache_path(tank_id, filename);
    let cache_dir = cache_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| crate::data::data_path(GLB_CACHE_DIR));
    if let Ok(bytes) = std::fs::read(&cache_path) {
        // 损坏自愈：无 glTF magic（截断/HTML 错误页）视作未命中，走重下覆盖
        if bytes.starts_with(b"glTF") {
            return Ok(bytes);
        }
        eprintln!(
            "[glb-cache] 缓存文件损坏（缺 glTF magic），重新下载: {}",
            cache_path.display()
        );
    }
    // 宿主内置资产兜底（宿主注入 reader 时生效）：直读不落盘，免大文件复制
    if let Some(bytes) =
        crate::data::read_embedded(&format!("data/cache/models/{tank_id}/{filename}"))
    {
        if bytes.starts_with(b"glTF") {
            return Ok(bytes);
        }
    }

    let url = format!("https://api.blitzkit.app/tanks/{}/{}", tank_id, filename);
    eprintln!("[glb-cache] downloading {} ...", url);
    // Client 进程级复用（连接池跨请求共享）
    let client = glb_http_client();
    let mut last_err; // 循环内每个分支都会先赋值
    for _attempt in 0..3 {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    let vec = bytes.to_vec();
                    if !vec.starts_with(b"glTF") {
                        last_err = "响应体不是合法 GLB（缺 glTF magic）".to_string();
                    } else {
                        let _ = std::fs::create_dir_all(&cache_dir);
                        match std::fs::write(&cache_path, &vec) {
                            Ok(_) => eprintln!(
                                "[glb-cache] cached {} ({} bytes)",
                                cache_path.display(),
                                vec.len()
                            ),
                            Err(e) => eprintln!("[glb-cache] cache write failed: {}", e),
                        }
                        return Ok(vec);
                    }
                }
                Err(e) => last_err = format!("read body failed: {}", e),
            },
            Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
                // 确定性 404：该车辆在 CDN 无此模型文件，重试与 curl 回退都无意义
                // （批量预热遇到大量缺失车辆时，逐个重试会拖慢整体进度）
                return Err(format!("BlitzKit CDN 404: {url} (模型不存在)"));
            }
            Ok(resp) => last_err = format!("BlitzKit CDN returned {}", resp.status()),
            Err(e) => last_err = format!("{}", e),
        }
        eprintln!("[glb-cache] attempt failed, retrying... ({last_err})");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // reqwest 全部重试失败 → 系统 curl 回退：rustls 指纹的大文件流可能被中途重置
    // （reqwest 报 read body failed: error decoding response body），
    // curl（不同 TLS 栈）可完整拉取同一资源。
    eprintln!("[glb-cache] reqwest 失败，尝试系统 curl 回退...");
    let tmp_path = cache_path.with_extension("download");
    // tokio 异步子进程：同步 output() 会在 async worker 上阻塞最长 --max-time 180s
    let out = tokio::process::Command::new("curl")
        .args([
            "-sfL",
            "--max-time",
            "180",
            "-o",
            tmp_path.to_string_lossy().as_ref(),
            &url,
        ])
        .output()
        .await;
    match out {
        Ok(o) if o.status.success() && tmp_path.exists() => match std::fs::read(&tmp_path) {
            Ok(bytes) if !bytes.is_empty() && bytes.starts_with(b"glTF") => {
                let _ = std::fs::create_dir_all(&cache_dir);
                let _ = std::fs::write(&cache_path, &bytes);
                let _ = std::fs::remove_file(&tmp_path);
                eprintln!(
                    "[glb-cache] curl 回退成功，已入缓存 {} ({} bytes)",
                    cache_path.display(),
                    bytes.len()
                );
                return Ok(bytes);
            }
            Ok(bytes) if !bytes.is_empty() => {
                last_err = "curl 回退：响应体不是合法 GLB（缺 glTF magic）".to_string()
            }
            Ok(_) => last_err = "curl 回退：响应体为空".to_string(),
            Err(e) => last_err = format!("curl 回退：读取失败 {}", e),
        },
        Ok(o) => {
            last_err = format!(
                "curl 回退失败（exit {:?}: {}）",
                o.status.code(),
                String::from_utf8_lossy(&o.stderr)
                    .chars()
                    .take(200)
                    .collect::<String>()
            );
        }
        Err(e) => last_err = format!("curl 回退不可用: {}", e),
    }
    let _ = std::fs::remove_file(&tmp_path);
    Err(format!(
        "BlitzKit CDN unreachable: {last_err} (model not in data/cache/models/)"
    ))
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
    )
        .into_response()
}

const TANK_IMAGE_DIR: &str = "cache/tank_images";

pub(crate) async fn tank_image_handler(
    axum::extract::Path(tank_id): axum::extract::Path<u32>,
) -> Response {
    let cache_path = crate::data::data_path(TANK_IMAGE_DIR).join(format!("{}.webp", tank_id));
    if let Ok(bytes) = std::fs::read(&cache_path) {
        return image_response(bytes);
    }

    let url = format!("https://api.blitzkit.app/tanks/{}/icons/big.webp", tank_id);
    match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => match resp.bytes().await {
            Ok(bytes) => {
                let vec = bytes.to_vec();
                let _ = std::fs::create_dir_all(crate::data::data_path(TANK_IMAGE_DIR));
                if std::fs::write(&cache_path, &vec).is_ok() {
                    eprintln!(
                        "[image-cache] cached {} ({} bytes)",
                        cache_path.display(),
                        vec.len()
                    );
                }
                image_response(vec)
            }
            Err(e) => (
                axum::http::StatusCode::BAD_GATEWAY,
                format!("image download failed: {}", e),
            )
                .into_response(),
        },
        Ok(resp) => (
            axum::http::StatusCode::BAD_GATEWAY,
            format!("BlitzKit icon returned {}", resp.status()),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::BAD_GATEWAY,
            format!("BlitzKit icon unreachable: {}", e),
        )
            .into_response(),
    }
}

fn image_response(bytes: Vec<u8>) -> Response {
    ([(axum::http::header::CONTENT_TYPE, "image/webp")], bytes).into_response()
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

/// GLB 下载专用 HTTP Client（进程级单例：连接池跨请求复用）。
fn glb_http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

pub(crate) async fn tank_filter_handler() -> Json<Value> {
    let mut out: Vec<serde_json::Value> = crate::wargaming::blitzkit::load_tanks()
        .values()
        .map(|t| {
            json!({
                "id": t.tank_id,
                "name": if t.name.is_empty() { t.dev_name.clone() } else { t.name.clone() },
                "tier": t.tier,
                "nation": t.nation.clone(),
                "type": t.tank_type.clone(),
            })
        })
        .collect();

    out.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    Json(json!(out))
}

pub(crate) async fn shells_handler(
    axum::extract::Path(tank_id): axum::extract::Path<u32>,
) -> Json<Value> {
    // 取**顶级炮塔 × 顶级主炮**：与 models.pb 的装甲/俯仰档位、`configs[]`、
    // 以及列表 `shells` 同档；取初始炮会与同屏渲染的顶级炮塔装甲对不上。
    let result: Value = crate::wargaming::blitzkit::tank_full(tank_id)
        .and_then(|t| t.turrets.last().and_then(|tur| tur.guns.last()).map(|g| {
            let caliber_mm = parse_gun_caliber(&g.name).map(|c| c.round() as u32).unwrap_or(120);
            let shells: Vec<Value> = g.shells.iter().map(|s| json!({
                "type": s.shell_type,
                // 全局弹种 id（与回放 shell_id 同域）：射击复现按 shell_id 反查槽位弹种用。
                // s.id 是 items 形式 (局部 id<<8)|国家序×16+1，须剥低字节取局部 id 再组回放域
                "global_id": crate::replay::loadout::blitzkit_shell_global_id(&t.nation, (s.id >> 8) as u64),
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

// ---------- 热力图截图就绪门控（自 wargaming/heatmap_ready.rs 收敛 HTTP 面） ----------
/// GET /api/hold?sess=X —— 长轮询：挂起直到会话就绪或超时（60s）。
/// 挂起的 XHR 扣住 Chrome 虚拟时间，让截图等待模型加载 + 热力图渲染。
pub async fn hold_handler(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let sess = q.get("sess").map(|s| s.as_str()).unwrap_or("");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if session_ready(sess) {
            return "ready".into_response();
        }
        if std::time::Instant::now() >= deadline {
            return "timeout".into_response();
        }
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    }
}

/// GET /api/ready?sess=X —— 页面通知服务器：热力图已渲染完成。
pub async fn ready_handler(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let sess = q.get("sess").map(|s| s.as_str()).unwrap_or("");
    mark_session_ready(sess);
    "ok".into_response()
}
