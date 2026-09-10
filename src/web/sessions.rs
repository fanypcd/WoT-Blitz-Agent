//! 会话管理：actor-per-session 模板。
//!
//! 每个活跃会话对应一个常驻 tokio 任务（actor），独占 `Agent` 本体；
//! HTTP 层通过 mpsc 命令通道投递指令，彻底消灭旧实现里
//! `Option<Agent>` take/归还的竞态（删除会话后后台任务把旧 Agent
//! 塞回新会话导致"历史复活"）。
//!
//! 设计要点：
//! - 同一会话的对话串行执行（actor 逐条处理命令），忙时新对话返回 409
//! - 每会话独立取消令牌（每轮开始前换新）→ 取消/删除精确到会话（旧实现是全局标志）
//! - 每轮对话结束即落盘 `data/sessions/{id}.json`（复用 CLI 的 SavedSession）
//!   → 重启后历史不丢；非活跃会话直接从磁盘懒加载，不实例化 Agent
//! - 删除 = cancel + abort + 移除句柄 + 删文件，运行中的对话也会被终止；
//!   Agent 从不离开 actor，被删会话不可能被后台任务复活
//! - 事件走共享缓冲（前端轮询增量渲染）；将来升级 SSE 只需把写缓冲
//!   换成 broadcast channel，其余不动

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agent::llm_client::ChatMessage;
use crate::agent::{Agent, AgentEvent, SavedSession};

/// actor 接受的命令。
enum SessionCmd {
    /// 执行一轮对话。
    Chat { text: String },
}

/// 对话请求失败的原因。
pub enum ChatError {
    /// 该会话已有对话在执行（HTTP 409）。
    Busy,
    /// 会话 id 不合法（HTTP 400）。
    InvalidId,
    /// 内部错误（HTTP 500）。
    Failed(String),
}

/// actor 与 HTTP 层共享的可变快照。
/// 事件/历史用 std Mutex：写入方（含同步事件回调）短暂持锁，不丢事件。
#[derive(Clone)]
struct SessionShared {
    /// 事件缓冲：actor 写入，HTTP 轮询读取
    events: Arc<Mutex<Vec<AgentEvent>>>,
    /// 对话历史镜像：actor 在每轮结束后刷新；读取方无需打断 actor
    history: Arc<Mutex<Vec<ChatMessage>>>,
    /// 当前轮对话的取消令牌槽：actor 每轮开始前换入新令牌，
    /// 取消方读取槽内令牌并 cancel（只影响当前轮）
    cancel_slot: Arc<Mutex<CancellationToken>>,
    /// 是否有对话在执行（供 409 快速判断，真正的串行化由 actor 保证）
    busy: Arc<AtomicBool>,
}

impl SessionShared {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            history: Arc::new(Mutex::new(Vec::new())),
            cancel_slot: Arc::new(Mutex::new(CancellationToken::new())),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    fn push_event(&self, e: AgentEvent) {
        if let Ok(mut v) = self.events.lock() {
            v.push(e);
        }
    }

    fn clear_events(&self) {
        if let Ok(mut v) = self.events.lock() {
            v.clear();
        }
    }

    /// 追加事件（move 语义，供事件回调闭包用）。
    fn push_event_fn(&self) -> impl Fn(AgentEvent) + Send + use<> {
        let events = self.events.clone();
        move |e| {
            if let Ok(mut v) = events.lock() {
                v.push(e);
            }
        }
    }

    fn set_history(&self, msgs: Vec<ChatMessage>) {
        if let Ok(mut v) = self.history.lock() {
            *v = msgs;
        }
    }

    /// 换入本轮的取消令牌。
    fn swap_token(&self, t: CancellationToken) {
        if let Ok(mut slot) = self.cancel_slot.lock() {
            *slot = t;
        }
    }

    /// 取出当前取消令牌（取消方用）。
    fn current_token(&self) -> Option<CancellationToken> {
        self.cancel_slot.lock().ok().map(|t| t.clone())
    }

    fn set_busy(&self, b: bool) {
        self.busy.store(b, Ordering::SeqCst);
    }

    fn is_busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    fn history_snapshot(&self) -> Vec<ChatMessage> {
        self.history.lock().map(|v| v.clone()).unwrap_or_default()
    }

    fn events_snapshot(&self) -> Vec<AgentEvent> {
        self.events.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

/// 一个活跃会话的句柄：命令入口 + 共享快照。
#[derive(Clone)]
struct SessionHandle {
    cmd_tx: mpsc::Sender<SessionCmd>,
    shared: SessionShared,
    /// actor 任务句柄（删除时 abort）
    join: Arc<JoinHandle<()>>,
}

/// 全局会话表：配置路径 + 持久化目录 + 活跃会话句柄。
#[derive(Clone)]
pub struct SessionManager {
    config_path: PathBuf,
    dir: PathBuf,
    sessions: Arc<tokio::sync::Mutex<HashMap<String, SessionHandle>>>,
}

impl SessionManager {
    pub fn new(config_path: PathBuf, dir: PathBuf) -> Self {
        Self {
            config_path,
            dir,
            sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// 会话 id 合法性：作为文件名落盘，仅允许字母/数字/下划线/连字符，≤64 字符。
    pub fn valid_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 64
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }

    fn file_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// 列出全部会话：内存中活跃的 ∪ 磁盘上的（去重、排序）。
    pub async fn list_ids(&self) -> Vec<String> {
        let mut set: HashSet<String> =
            self.sessions.lock().await.keys().cloned().collect();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("json") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        set.insert(stem.to_string());
                    }
                }
            }
        }
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    }

    /// 发起对话：get-or-create actor → 忙则 Busy → 否则投递 Chat 命令。
    pub async fn chat(&self, id: &str, text: String) -> Result<(), ChatError> {
        if !Self::valid_id(id) {
            return Err(ChatError::InvalidId);
        }
        let handle = self.get_or_create(id).await;
        if handle.shared.is_busy() {
            return Err(ChatError::Busy);
        }
        handle
            .cmd_tx
            .send(SessionCmd::Chat { text })
            .await
            .map_err(|_| ChatError::Failed("session actor terminated".into()))
    }

    /// 取消指定会话当前执行的对话（仅活跃会话；空闲会话无操作）。
    pub async fn cancel(&self, id: &str) {
        if let Some(h) = self.sessions.lock().await.get(id) {
            if let Some(t) = h.shared.current_token() {
                t.cancel();
            }
        }
    }

    /// 删除会话：终止 actor（若有）+ 移除句柄 + 删除持久化文件。
    /// 运行中的对话立即被 abort（LLM 调用被丢弃，不再消耗 token）。
    pub async fn delete(&self, id: &str) {
        if let Some(h) = self.sessions.lock().await.remove(id) {
            if let Some(t) = h.shared.current_token() {
                t.cancel();
            }
            h.join.abort();
        }
        let _ = std::fs::remove_file(self.file_for(id));
    }

    /// 读取对话历史（不含系统提示）：活跃会话读镜像，非活跃读磁盘，都无则空。
    pub async fn history_of(&self, id: &str) -> Vec<ChatMessage> {
        if let Some(h) = self.sessions.lock().await.get(id) {
            return h.shared.history_snapshot();
        }
        self.load_disk_history(id)
    }

    /// 读取事件缓冲（仅活跃会话有事件；不活跃返回空 → 前端结束轮询）。
    pub async fn events_of(&self, id: &str) -> Vec<AgentEvent> {
        match self.sessions.lock().await.get(id) {
            Some(h) => h.shared.events_snapshot(),
            None => vec![],
        }
    }

    /// 非活跃会话直接读 SavedSession 文件，不实例化 Agent。
    fn load_disk_history(&self, id: &str) -> Vec<ChatMessage> {
        if !Self::valid_id(id) {
            return vec![];
        }
        std::fs::read_to_string(self.file_for(id))
            .ok()
            .and_then(|s| serde_json::from_str::<SavedSession>(&s).ok())
            .map(|s| {
                s.messages
                    .into_iter()
                    .filter(|m| m.role != "system")
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 获取（或首次创建）会话 actor。共享快照在此创建，actor 任务只持有克隆。
    async fn get_or_create(&self, id: &str) -> SessionHandle {
        let mut guard = self.sessions.lock().await;
        if let Some(h) = guard.get(id) {
            return h.clone();
        }

        let shared = SessionShared::new();
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

        // actor 启动：磁盘有历史则恢复（懒加载），然后常驻处理命令
        let join = tokio::spawn(session_actor(
            self.config_path.clone(),
            self.dir.clone(),
            id.to_string(),
            cmd_rx,
            shared.clone(),
        ));

        let handle = SessionHandle {
            cmd_tx,
            shared,
            join: Arc::new(join),
        };
        guard.insert(id.to_string(), handle.clone());
        handle
    }
}

/// 会话 actor 主体：独占 Agent，串行处理命令，每轮结束落盘。
async fn session_actor(
    config_path: PathBuf,
    dir: PathBuf,
    id: String,
    mut rx: mpsc::Receiver<SessionCmd>,
    shared: SessionShared,
) {
    // 懒加载：磁盘有历史则恢复；Agent 构建失败不终止 actor，
    // 留待下一次 Chat 时重试（期间以 Error 事件反馈）。
    let path = dir.join(format!("{id}.json"));
    let mut agent = restore_agent(&config_path, &path, &shared);

    while let Some(cmd) = rx.recv().await {
        match cmd {
            SessionCmd::Chat { text } => {
                shared.set_busy(true);
                // 新一轮对话：清空事件缓冲（前端游标从零读增量），
                // 并换入全新取消令牌（cancel 只作用于本轮，无残留）
                shared.clear_events();
                let run_token = CancellationToken::new();
                shared.swap_token(run_token.clone());

                if agent.is_none() {
                    // 上次构建失败 → 本次重试一次，成功则保留并恢复历史
                    agent = restore_agent(&config_path, &path, &shared);
                }
                match agent.as_mut() {
                    Some(a) => {
                        a.set_cancel_token(run_token);
                        let on_event = shared.push_event_fn();
                        if let Err(e) = a.chat_async(&text, on_event).await {
                            shared.push_event(AgentEvent::Error { message: e.to_string() });
                        }
                        // 每轮落盘 + 刷新历史镜像
                        if let Err(e) = a.save_session(&path) {
                            eprintln!("[session:{id}] persist failed: {e}");
                        }
                        shared.set_history(a.history());
                    }
                    None => {
                        shared.push_event(AgentEvent::Error {
                            message: "Agent not initialized. Check config.toml and resend.".into(),
                        });
                    }
                }
                shared.set_busy(false);
            }
        }
    }
}

/// 构建 Agent 并尝试从磁盘恢复历史（失败以 Error 事件反馈，不 panic）。
fn restore_agent(config_path: &std::path::Path, path: &std::path::Path, shared: &SessionShared) -> Option<Agent> {
    match Agent::new(config_path) {
        Ok(mut a) => {
            if path.exists() {
                match a.load_session(path) {
                    Ok(()) => shared.set_history(a.history()),
                    Err(e) => shared.push_event(AgentEvent::Error {
                        message: format!("Failed to restore session: {e}"),
                    }),
                }
            }
            Some(a)
        }
        Err(e) => {
            shared.push_event(AgentEvent::Error {
                message: format!("Failed to init agent: {e}"),
            });
            None
        }
    }
}
