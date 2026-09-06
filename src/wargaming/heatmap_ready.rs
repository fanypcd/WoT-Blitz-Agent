// =====================================================================
//  热力图截图的就绪门控：无头浏览器渲染 3D 查看器时，页面用长轮询 XHR
//  扣住 Chrome 的虚拟时间（pending XHR 会阻止 virtual time 推进——而
//  GLTFLoader 的 fetch() 不会），直到热力图渲染完成才释放，保证截图
//  等待模型加载完成之后再进行。
// =====================================================================
use std::collections::HashSet;
use std::sync::Mutex;

static READY_SESSIONS: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();

fn ready_set() -> &'static Mutex<HashSet<String>> {
    READY_SESSIONS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 页面（3D 查看器）在热力图渲染完成后调用：标记会话就绪。
pub fn mark_session_ready(sess: &str) {
    if sess.is_empty() { return; }
    ready_set().lock().unwrap().insert(sess.to_string());
}

pub fn session_ready(sess: &str) -> bool {
    ready_set().lock().unwrap().contains(sess)
}

/// GET /api/hold?sess=X —— 长轮询：挂起直到会话就绪或超时（60s）。
/// 挂起的 XHR 扣住 Chrome 虚拟时间，让截图等待模型加载 + 热力图渲染。
pub async fn hold_handler(
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let sess = q.get("sess").cloned().unwrap_or_default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if session_ready(&sess) {
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
    let sess = q.get("sess").cloned().unwrap_or_default();
    mark_session_ready(&sess);
    "ok".into_response()
}
