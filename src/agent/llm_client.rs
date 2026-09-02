use anyhow::{Result, anyhow, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::config::{Config, TokenUsage};

// =====================================================================
//  LLM 客户端（OpenAI 兼容接口）
//  负责把对话消息 + 工具定义发给大模型，解析回复（文本 / 工具调用），
//  并顺手记录本次调用的 token 用量。已重构为 async（供 Web GUI 使用）。
// =====================================================================

/// 一条对话消息（system / user / assistant / tool 四种角色）。
///
/// 与 OpenAI Chat Completions 的 message 结构对齐：
/// - `tool_calls`：assistant 消息里可能的工具调用列表
/// - `tool_call_id`：tool 消息用来回指它所对应的那次工具调用
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

/// LLM 客户端（保存端点/密钥/模型，附带用于计价与预算的配置）。
pub struct LlmClient {
    endpoint: String,
    api_key: String,
    model: String,
    config: Config,
}

impl LlmClient {
    /// 从全局配置构造客户端。
    pub fn new(config: &Config) -> Self {
        Self {
            endpoint: config.llm.endpoint.clone(),
            api_key: config.llm.api_key.clone(),
            model: config.llm.model.clone(),
            config: config.clone(),
        }
    }

    /// 发起一次对话补全请求（async），返回模型的 assistant 回复。
    ///
    /// # 参数
    /// - `messages`：到目前为止的完整对话历史（含系统提示）
    /// - `tools`：注册给模型的工具定义（可空）
    /// - `usage`：本次调用的 token 用量会记录进其中
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[ToolDefinition]>,
        usage: &mut TokenUsage,
    ) -> Result<ChatMessage> {
        // 端点：{endpoint}/chat/completions
        let url = format!("{}/chat/completions", self.endpoint);

        // 组装请求体
        let mut body = serde_json::json!({
            "model": &self.model,
            "messages": messages,
            "temperature": 0.7,
            "max_tokens": self.config.llm.max_tokens.unwrap_or(4096),
        });

        // 思考模式（GLM 等支持 thinking 的模型）
        if self.config.llm.thinking_mode {
            body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": 4096});
        }

        // 注册工具（非空才附带）
        if let Some(t) = tools {
            if !t.is_empty() {
                body["tools"] = serde_json::to_value(t)?;
            }
        }

        // 带 120 秒超时的 HTTP 客户端
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send LLM request")?;

        // 非 2xx 视为接口错误
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("LLM API error {}: {}", status, text));
        }

        // 解析响应体
        let resp_json: Value = resp.json().await.context("Failed to parse LLM response")?;

        // 读取 token 用量（供 R6 统计）
        let input_tokens = resp_json["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        let output_tokens = resp_json["usage"]["completion_tokens"].as_u64().unwrap_or(0);

        // 按配置的单价折算成本并记录
        usage.record(
            &self.model,
            input_tokens,
            output_tokens,
            self.config.llm.price_input_per_1k,
            self.config.llm.price_output_per_1k,
            "chat",
        );

        // 取第一条回复：文本 + 可能的工具调用列表
        let choice = &resp_json["choices"][0];
        let message = &choice["message"];

        let content = message["content"].as_str().unwrap_or("").to_string();
        let tool_calls = if message.get("tool_calls").is_some() {
            let calls: Vec<ToolCall> = serde_json::from_value(message["tool_calls"].clone())
                .unwrap_or_default();
            if calls.is_empty() { None } else { Some(calls) }
        } else {
            None
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
