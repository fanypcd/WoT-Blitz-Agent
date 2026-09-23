use anyhow::Result;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::models::config::{Config, TokenUsage};
use crate::agent::llm_client::{LlmClient, ChatMessage, ToolDefinition};
use crate::agent::tools::AgentTools;

pub mod llm_client;
pub mod tools;

// Agent：对话 → LLM → 工具调用循环；支持 CLI（阻塞包装）与 Web（async + 流式事件）。

/// 全局"打断"标志（Ctrl+C 或 Web 打断按钮写入）。
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// 设置打断标志（Ctrl+C handler / Web 取消接口调用）。
pub fn set_interrupted() {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// 查询是否已请求打断（Agent Loop 每步检查）。
pub fn is_interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Agent 一次对话回合中上报的事件，供 GUI 流式渲染进度。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    StepStart { step: usize, max: usize },
    ToolCall { name: String, args: Value },
    ToolResult { name: String, result: String },
    Interrupted,
    Done { content: String },
    Error { message: String },
}

/// 一次可保存/加载的会话（用于 R5 会话持久化）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedSession {
    pub datetime: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub token_usage: TokenUsage,
}

/// 一个 Agent 实例：持有配置、对话历史、工具集、token 统计。
pub struct Agent {
    pub config: Config,
    /// 对话消息历史（以系统提示开头）
    messages: Vec<ChatMessage>,
    tools: AgentTools,
    tool_defs: Vec<ToolDefinition>,
    usage: TokenUsage,
    usage_path: std::path::PathBuf,
    /// 本实例专属的取消令牌（Web 会话级打断；每轮对话开始前由会话
    /// actor 换入新令牌，cancel 只影响当前轮。CLI Ctrl+C 仍走全局标志回退）
    cancel: CancellationToken,
    pub replay_dir: String,
    pub tank_cache: Option<std::path::PathBuf>,
}

impl Agent {
    /// 从配置文件构造 Agent（加载配置、工具集、token 统计，写入系统提示）。
    pub fn new(config_path: &Path) -> Result<Self> {
        let config = Config::load_or_create(config_path)?;
        
        let replay_dir = config.replay.replay_dir.clone();
        let tank_cache_path = config.replay.tank_cache_path.clone();
        let tank_cache = if Path::new(&tank_cache_path).exists() {
            Some(std::path::PathBuf::from(&tank_cache_path))
        } else {
            None
        };

        let tools = AgentTools::new(
            &config.wg_api.application_id,
            &config.wg_api.server,
            &replay_dir,
            tank_cache.as_deref(),
        );

        let usage_path = std::path::PathBuf::from("token_usage.json");
        let usage = TokenUsage::load_from_file(&usage_path).unwrap_or_default();

        // 系统提示：约束 LLM 只使用工具返回的真实数据，严禁编造坦克统计数字
        let system_prompt = "You are a WoTB (World of Tanks Blitz) game analysis assistant. \
You help players improve by analyzing their replay files and WG API statistics. \
You can search for players, get their stats, scan replay files, parse individual replays, \
and compare recent replay performance vs all-time API stats. \
You can also open the 3D armor viewer (view_tank): pass a `target` tank to inspect, and optionally a \
`shooter` tank whose shell/caliber is used to simulate penetration against the target. \
Both target and shooter are fuzzy-matched by name; if a name matches several tanks the tool returns a \
numbered candidate list — in that case, do NOT open the viewer yourself; relay the numbered list to the \
user and ask which one they mean, then call view_tank again with their chosen exact name. \
Always provide specific, actionable advice based on the data. \
When the user asks about their performance, use the available tools to get real data. \
NEVER invent or estimate tank statistics (caliber, penetration, armor, HP, speed): every stat you \
state must come from a tool result. If a tank-name lookup fails or returns an error, tell the user \
the name was not found and ask for clarification or an exact spelling — never substitute another \
tank's data or guess. When a tool reports shooter/target data, attribute it exactly as labeled \
(shooter = attacking tank's gun, target = defending tank's armor). \
Respond in Chinese if the user speaks Chinese, in English otherwise.";

        let messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }];

        Ok(Self {
            config,
            messages,
            tools,
            tool_defs: AgentTools::definitions(),
            usage,
            usage_path,
            cancel: CancellationToken::new(),
            replay_dir,
            tank_cache,
        })
    }

    /// Agent Loop（async + 流式事件，供 Web GUI）：追加用户输入后循环（最多 5 次）
    /// 调 LLM → 有工具调用则逐个执行并回填 → 直到无工具调用的最终回复。
    /// 每步经 `on_event` 上报 AgentEvent 供前端显示进度。
    pub async fn chat_async<F>(&mut self, user_input: &str, mut on_event: F) -> Result<String>
    where
        F: FnMut(AgentEvent) + Send,
    {
        if self.config.llm.api_key.is_empty() {
            on_event(AgentEvent::Error { message: "LLM API key not configured.".into() });
            return Err(anyhow::anyhow!("LLM API key not configured. Run `wotb-agent config --show` to edit config.toml."));
        }

        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_input.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });

        let llm = LlmClient::new(&self.config);

        let max_iterations = 5;
        for i in 0..max_iterations {
            // 每步检查打断（会话级令牌 + CLI 全局 Ctrl+C 回退）
            if self.cancel.is_cancelled() || is_interrupted() {
                on_event(AgentEvent::Interrupted);
                self.messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: "[Interrupted by user]".to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                });
                self.usage.save_to_file(&self.usage_path).ok();
                return Ok("Interrupted.".to_string());
            }

            if !llm.check_budget(&self.usage) {
                let budget = self.config.llm.budget.unwrap_or(0.0);
                let msg = format!("Token budget exceeded: ${:.4} >= ${:.2}", self.usage.total_cost, budget);
                on_event(AgentEvent::Error { message: msg.clone() });
                return Err(anyhow::anyhow!("{}", msg));
            }

            on_event(AgentEvent::StepStart { step: i + 1, max: max_iterations });
            let response = llm.chat(&self.messages, Some(&self.tool_defs), &mut self.usage).await?;
            // 仅在确有工具调用时克隆 tool_calls；content 随消息直接入历史（最终回复路径零克隆）
            let tool_calls = response.tool_calls.clone();
            self.messages.push(response);

            if let Some(tool_calls) = &tool_calls {
                for tc in tool_calls {
                    if self.cancel.is_cancelled() || is_interrupted() {
                        break;
                    }
                    let tool_name = &tc.function.name;
                    let args: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(Value::Null);
                    on_event(AgentEvent::ToolCall { name: tool_name.clone(), args: args.clone() });

                    let result = match self.tools.execute(tool_name, &args) {
                        Ok(r) => r,
                        Err(e) => format!("Error: {}", e),
                    };
                    on_event(AgentEvent::ToolResult { name: tool_name.clone(), result: result.clone() });

                    self.messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: result,
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                    });
                }
                continue;
            }

            self.usage.save_to_file(&self.usage_path).ok();
            // 最终回复即刚入历史的 assistant 消息
            let content = self.messages.last().expect("assistant reply just pushed").content.clone();
            on_event(AgentEvent::Done { content: content.clone() });
            return Ok(content);
        }

        self.usage.save_to_file(&self.usage_path).ok();
        let msg = "Reached maximum tool call iterations. Please try a more specific question.".to_string();
        on_event(AgentEvent::Done { content: msg.clone() });
        Ok(msg)
    }

    /// CLI 用的阻塞包装：在本线程创建 runtime 驱动 async 循环，并把事件打印到 stderr。
    pub fn chat(&mut self, user_input: &str) -> Result<String> {
        let runtime = tokio::runtime::Runtime::new()?;
        let result = runtime.block_on(self.chat_async(user_input, |e| {
            match &e {
                AgentEvent::StepStart { step, max } => eprintln!("[Agent] Calling LLM (step {}/{})...", step, max),
                AgentEvent::ToolCall { name, args } => eprintln!("[Agent] Tool call: {} ({})", name,
                    if args.is_object() { args.to_string() } else { String::new() }),
                AgentEvent::ToolResult { name: _, result } => eprintln!("[Agent]   Done ({} chars)", result.len()),
                AgentEvent::Interrupted => eprintln!("[Agent]   Interrupted by user"),
                _ => {}
            }
        }))?;
        Ok(result)
    }

    /// 把会话保存为 JSON 文件。
    pub fn save_session(&self, path: &Path) -> Result<()> {
        let session = SavedSession {
            datetime: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            model: self.config.llm.model.clone(),
            messages: self.messages.clone(),
            token_usage: self.usage.clone(),
        };
        let json = serde_json::to_string_pretty(&session)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// 从 JSON 文件加载会话，恢复消息历史与 token 统计。
    pub fn load_session(&mut self, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let session: SavedSession = serde_json::from_str(&content)?;
        self.messages = session.messages;
        self.usage = session.token_usage;
        Ok(())
    }

    /// 立即把 token 用量落盘。
    pub fn save_usage(&self) {
        self.usage.save_to_file(&self.usage_path).ok();
    }

    /// 换入新的取消令牌（会话 actor 每轮对话开始前调用；cancel 只作用当前轮，无需 reset 残留）。
    pub fn set_cancel_token(&mut self, t: CancellationToken) {
        self.cancel = t;
    }

    /// 获取对话历史（不含系统提示），供 Web GUI 展示。
    pub fn history(&self) -> Vec<ChatMessage> {
        self.messages.iter()
            .filter(|m| m.role != "system")
            .cloned()
            .collect()
    }


    /// 打印 token 用量汇总（CLI `usage` 子命令）。
    pub fn print_usage(&self) {
        self.usage.print_summary();
    }

    /// 打印对话历史（CLI `history` 子命令）。
    pub fn print_history(&self) {
        println!("\n=== Conversation History ===\n");
        for msg in &self.messages {
            match msg.role.as_str() {
                "system" => {}
                "user" => println!("User: {}", msg.content),
                "assistant" => {
                    if !msg.content.is_empty() {
                        println!("Assistant: {}", msg.content);
                    }
                    if let Some(tcs) = &msg.tool_calls {
                        for tc in tcs {
                            println!("  [Tool Call: {}]", tc.function.name);
                        }
                    }
                }
                "tool" => println!("  [Tool Result]: {}...", msg.content.chars().take(100).collect::<String>()),
                _ => {}
            }
        }
        println!();
    }
}
