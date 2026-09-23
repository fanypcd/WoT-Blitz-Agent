//! 会话管理：actor-per-session。
//!
//! 每个活跃会话一个常驻 tokio 任务（actor）独占 `Agent`，HTTP 层经 mpsc 命令通道
//! 投递指令——消除 `Option<Agent>` take/归还式共享在删除会话后把旧 Agent 塞回
//! 新会话导致"历史复活"的竞态。设计要点：
//! - 同会话对话串行执行（actor 逐条处理命令），忙时新对话返回 409
//! - 每轮换新取消令牌（取消/删除精确到会话与当前轮）；轮结束即落盘
//!   `data/sessions/{id}.json`（复用 CLI 的 SavedSession），非活跃会话从磁盘
//!   懒加载、不实例化 Agent
//! - 删除 = cancel + abort + 移除句柄 + 删文件；Agent 从不离开 actor，不会复活
//! - 事件走共享缓冲（前端轮询增量渲染）；升级 SSE 只需换 broadcast channel

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use serde_json::Value;

use crate::agent::llm_client::ChatMessage;
use crate::agent::{Agent, AgentEvent, SavedSession};

enum SessionCmd {
    Chat { text: String },
}

/// 对话请求失败原因（Busy→HTTP 409，InvalidId→400，Failed→500）。
pub enum ChatError {
    Busy,
    InvalidId,
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
    /// 当前轮取消令牌槽：actor 每轮换入新令牌，取消只影响当前轮
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

    fn swap_token(&self, t: CancellationToken) {
        if let Ok(mut slot) = self.cancel_slot.lock() {
            *slot = t;
        }
    }

    fn current_token(&self) -> Option<CancellationToken> {
        self.cancel_slot.lock().ok().map(|t| t.clone())
    }

    fn set_busy(&self, b: bool) {
        self.busy.store(b, Ordering::SeqCst);
    }

    /// 尝试原子占用忙标志（false→true）：成功 = 会话空闲、本轮对话归属当前请求；
    /// 失败 = 已有对话在执行（并发第二个请求由此直接收到 Busy/409，而不是被静默排队）。
    fn try_acquire_busy(&self) -> bool {
        self.busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// 历史快照：锁内直接序列化为 JSON，消除中间的整段 Vec<ChatMessage> 深拷贝。
    fn history_snapshot(&self) -> Vec<Value> {
        self.history.lock().map(|v| {
            v.iter().map(|m| serde_json::to_value(m).unwrap_or(Value::Null)).collect()
        }).unwrap_or_default()
    }

    /// 事件快照：锁内直接序列化为 JSON，消除中间的整段 Vec<AgentEvent> 深拷贝。
    fn events_snapshot(&self) -> Vec<Value> {
        self.events.lock().map(|v| {
            v.iter().map(|e| serde_json::to_value(e).unwrap_or(Value::Null)).collect()
        }).unwrap_or_default()
    }
}

#[derive(Clone)]
struct SessionHandle {
    cmd_tx: mpsc::Sender<SessionCmd>,
    shared: SessionShared,
    /// actor 任务句柄（删除时 abort）
    join: Arc<JoinHandle<()>>,
}

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

    pub async fn chat(&self, id: &str, text: String) -> Result<(), ChatError> {
        if !Self::valid_id(id) {
            return Err(ChatError::InvalidId);
        }
        let handle = self.get_or_create(id).await;
        // 原子占位忙标志：检查与占位一步完成，并发的第二个请求直接收到 Busy（HTTP 409），
        // 而不是都通过检查后被静默排队。actor 每轮结束时 set_busy(false) 释放。
        if !handle.shared.try_acquire_busy() {
            return Err(ChatError::Busy);
        }
        if handle.cmd_tx.send(SessionCmd::Chat { text }).await.is_err() {
            // 投递失败（actor 已终止）必须归还忙标志，否则该会话将永久 409
            handle.shared.set_busy(false);
            return Err(ChatError::Failed("session actor terminated".into()));
        }
        Ok(())
    }

    pub async fn cancel(&self, id: &str) {
        if let Some(h) = self.sessions.lock().await.get(id) {
            if let Some(t) = h.shared.current_token() {
                t.cancel();
            }
        }
    }

    /// 删除会话：abort actor 并删持久化文件；运行中的对话被终止，LLM 调用被丢弃。
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
    /// 返回锁内直接序列化好的 JSON 值（避免先整段克隆再逐条序列化）。
    pub async fn history_of(&self, id: &str) -> Vec<Value> {
        if let Some(h) = self.sessions.lock().await.get(id) {
            return h.shared.history_snapshot();
        }
        self.load_disk_history(id)
    }

    /// 读取事件缓冲（仅活跃会话有事件；不活跃返回空 → 前端结束轮询）。
    /// 返回锁内直接序列化好的 JSON 值。
    pub async fn events_of(&self, id: &str) -> Vec<Value> {
        match self.sessions.lock().await.get(id) {
            Some(h) => h.shared.events_snapshot(),
            None => vec![],
        }
    }

    fn load_disk_history(&self, id: &str) -> Vec<Value> {
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
                    .map(|m| serde_json::to_value(&m).unwrap_or(Value::Null))
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn get_or_create(&self, id: &str) -> SessionHandle {
        let mut guard = self.sessions.lock().await;
        if let Some(h) = guard.get(id) {
            return h.clone();
        }

        let shared = SessionShared::new();
        let (cmd_tx, cmd_rx) = mpsc::channel(16);

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
    // 懒加载：磁盘有历史则恢复；Agent 构建失败不终止 actor，留待下次 Chat 重试（以 Error 事件反馈）。
    let path = dir.join(format!("{id}.json"));
    let mut agent = restore_agent(&config_path, &path, &shared);

    while let Some(cmd) = rx.recv().await {
        match cmd {
            SessionCmd::Chat { text } => {
                shared.set_busy(true);
                // 新一轮：清空事件缓冲（前端游标从零读增量），换入全新取消令牌（cancel 只作用于本轮）
                shared.clear_events();
                let run_token = CancellationToken::new();
                shared.swap_token(run_token.clone());

                if agent.is_none() {
                    agent = restore_agent(&config_path, &path, &shared);
                }
                match agent.as_mut() {
                    Some(a) => {
                        a.set_cancel_token(run_token);
                        let on_event = shared.push_event_fn();
                        if let Err(e) = a.chat_async(&text, on_event).await {
                            shared.push_event(AgentEvent::Error { message: e.to_string() });
                        }
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
