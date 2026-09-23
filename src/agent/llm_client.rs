use anyhow::{Result, anyhow, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::config::{Config, TokenUsage};

// LLM 客户端（OpenAI 兼容）：发消息 + 工具定义，解析回复（文本/工具调用），记录 token 用量。

/// 一条对话消息（对齐 OpenAI Chat Completions message；`tool_call_id` 供 tool 消息回指所属调用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// 一个工具调用（模型要求执行某个函数）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// 本次调用的唯一标识（tool 消息用它回指）
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// 工具调用里的函数名 + 参数（参数是 JSON 字符串）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// 注册给模型的工具定义（OpenAI tools 数组元素）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub def_type: String,
    pub function: ToolFunction,
}

/// 工具函数元信息：名称、描述、JSON Schema 参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// LLM 客户端（保存配置用于计价与预算；HTTP 客户端只构建一次，跨对话复用连接池）。
pub struct LlmClient {
    http: reqwest::Client,
    config: Config,
}

impl LlmClient {
    /// 从全局配置构造客户端。
    pub fn new(config: &Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Self { http, config: config.clone() }
    }

    /// 发起一次对话补全（async）：`messages` 为完整对话历史（含系统提示），`tools` 可空；
    /// 本次调用的 token 用量记入 `usage`。
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
        usage: &mut TokenUsage,
    ) -> Result<ChatMessage> {
        let url = format!("{}/chat/completions", self.config.llm.endpoint);

        let mut body = serde_json::json!({
            "model": &self.config.llm.model,
            "messages": messages,
            "temperature": 0.7,
            "max_tokens": self.config.llm.max_tokens.unwrap_or(4096),
        });

        // 思考模式（GLM 等支持 thinking 的模型）
        if self.config.llm.thinking_mode {
            body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": 4096});
        }

        if let Some(t) = tools {
            if !t.is_empty() {
                body["tools"] = serde_json::to_value(t)?;
            }
        }

        let resp = self.http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.llm.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send LLM request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("LLM API error {}: {}", status, text));
        }

        let mut resp_json: Value = resp.json().await.context("Failed to parse LLM response")?;

        let input_tokens = resp_json["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let output_tokens = resp_json["usage"]["completion_tokens"].as_u64().unwrap_or(0);

        usage.record(
            &self.config.llm.model,
            input_tokens,
            output_tokens,
            self.config.llm.price_input_per_1k,
            self.config.llm.price_output_per_1k,
            "chat",
        );

        let content = resp_json["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
        // 零拷贝取出 tool_calls（take 后 resp_json 不再使用；get_mut 链对异常响应形状安全，不 panic 不插入）
        let taken = resp_json.get_mut("choices")
            .and_then(|c| c.get_mut(0))
            .and_then(|c| c.get_mut("message"))
            .and_then(|m| m.get_mut("tool_calls"))
            .map(|v| v.take());
        let tool_calls = match taken {
            Some(v) if !v.is_null() => {
                let calls: Vec<ToolCall> = serde_json::from_value(v).unwrap_or_default();
                if calls.is_empty() { None } else { Some(calls) }
            }
            _ => None,
        };

        Ok(ChatMessage {
            role: "assistant".to_string(),
            content,
            tool_calls,
            tool_call_id: None,
        })
    }

    /// 检查当前累计费用是否超出预算（未配置预算则视为不设限）。
    pub fn check_budget(&self, usage: &TokenUsage) -> bool {
        if let Some(budget) = self.config.llm.budget {
            usage.check_budget(budget)
        } else {
            true
        }
    }

}
