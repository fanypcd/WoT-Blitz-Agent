use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::Result;

// =====================================================================
//  全局配置（config.toml）
//  顶层配置由三部分组成：WG API 接入、LLM 模型接入、回放路径。
// =====================================================================

/// 顶层配置，对应 `config.toml` 文件结构。
///
/// 顶层分为三个小节：
/// - `wg_api`：Wargaming 公共 API 的接入信息
/// - `llm`   ：大模型 API（OpenAI 兼容）的接入与计价信息
/// - `replay`：本地回放目录与坦克缓存文件路径
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub wg_api: WgApiConfig,
    pub llm: LlmConfig,
    pub replay: ReplayConfig,
}

/// WG API 小节：用于战绩查询（坦克数据已全部来自 BlitzKit，不再走 WG 百科）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WgApiConfig {
    /// Wargaming 开发者平台申请的 Application ID
    pub application_id: String,
    /// 服务器分区：asia / eu / na（决定 API 域名）
    pub server: String,
}

/// LLM 模型小节：OpenAI 兼容接口参数 + Token 计价 + 预算。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// API 端点（如 `https://api.openai.com/v1`），可切换到任意 OpenAI 兼容服务
    pub endpoint: String,
    /// API Key
    pub api_key: String,
    /// 模型名称（如 `gpt-4o`、`glm-5`）
    pub model: String,
    /// 上下文窗口长度（token）
    pub context_length: u32,
    /// 是否开启思考模式
    pub thinking_mode: bool,
    /// 输入价格（美元 / 1000 token）
    pub price_input_per_1k: f64,
    /// 输出价格（美元 / 1000 token）
    pub price_output_per_1k: f64,
    /// 单次回答最大 token 数
    pub max_tokens: Option<u32>,
    /// 总预算（美元），超预算自动中断
    pub budget: Option<f64>,
}

/// 回放小节：本地 `.wotbreplay` 文件路径配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayConfig {
    /// 回放文件所在目录
    pub replay_dir: String,
    /// 坦克缓存文件路径（`tank_cache.json`）
    pub tank_cache_path: String,
    /// 路径风格转换。程序存在 Windows 与 WSL/Linux 两种运行版本，
    /// 用户输入的回放路径可能是任一风格。两种风格的输入随时都接受：
    ///   auto（默认）= 转成本次运行平台的本地形式（Windows 版把 /mnt/c/... → C:/...，
    ///                WSL/Linux 版把 C:\... → /mnt/c/...）
    ///   windows / wsl = 强制转成对应风格
    ///   off = 不转换（自定义挂载点等特殊场景）
    #[serde(default = "default_path_translate")]
    pub path_translate: String,
}

fn default_path_translate() -> String { "auto".to_string() }

impl ReplayConfig {
    /// 按配置把用户输入的路径转成本地可用形式（见 [`Self::path_translate`]）。
    pub fn translate(&self, input: &str) -> String {
        Self::translate_with_mode(input, &self.path_translate)
    }

    /// 路径风格转换内核（mode = auto | windows | wsl | off）。
    ///
    /// 双向映射无歧义，盘符大小写归一：
    ///   `/mnt/<d>/rest`  ⇄  `<D>:/rest`（反斜杠统一为正斜杠）
    /// 其余形式（相对路径、/home/...、UNC \\server\share）原样返回。
    pub fn translate_with_mode(input: &str, mode: &str) -> String {
        // 复制粘贴常带首尾引号（资源管理器"复制文件地址"），一并剥掉
        let p = input.trim().trim_matches('"').trim();
        if p.is_empty() {
            return p.to_string();
        }
        let mode = mode.trim().to_ascii_lowercase();
        if mode == "off" || mode == "none" || mode == "raw" {
            return p.to_string();
        }
        let target = match mode.as_str() {
            "windows" => "win",
            "wsl" | "linux" => "wsl",
            // auto：按运行平台的本地形式（编译期即确定，WSL 内构建即 Linux 目标）
            _ => if cfg!(windows) { "win" } else { "wsl" },
        };
        if target == "win" {
            if let Some((drive, rest)) = Self::parse_wsl_drive(p) {
                return format!("{}:/{}", drive.to_ascii_uppercase(), rest);
            }
        } else if let Some((drive, rest)) = Self::parse_win_drive(p) {
            return format!("/mnt/{}/{}", drive.to_ascii_lowercase(), rest.replace('\\', "/"));
        }
        p.to_string()
    }

    /// 识别 Windows 风格 `C:\...` / `C:/...`（返回盘符与剩余部分）；UNC 不算。
    fn parse_win_drive(p: &str) -> Option<(char, &str)> {
        let b = p.as_bytes();
        if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/') {
            Some((b[0] as char, &p[3..]))
        } else {
            None
        }
    }

    /// 识别 WSL 风格 `/mnt/c/...`（返回盘符与剩余部分）。
    fn parse_wsl_drive(p: &str) -> Option<(char, &str)> {
        let b = p.as_bytes();
        if b.len() >= 7 && p.is_char_boundary(7) && &p[..5] == "/mnt/"
            && b[5].is_ascii_alphabetic() && b[6] == b'/' {
            Some((b[5] as char, &p[7..]))
        } else {
            None
        }
    }
}

impl Default for Config {
    /// 生成一份默认配置（首次运行无配置文件时写入）。
    ///
    /// 默认值：WG 服务器 asia、LLM 指向 OpenAI、回放目录为当前目录。
    fn default() -> Self {
        Self {
            wg_api: WgApiConfig {
                application_id: String::new(),
                server: "asia".to_string(),
            },
            llm: LlmConfig {
                endpoint: "https://api.openai.com/v1".to_string(),
                api_key: String::new(),
                model: "gpt-4o".to_string(),
                context_length: 8192,
                thinking_mode: false,
                price_input_per_1k: 0.005,
                price_output_per_1k: 0.015,
                max_tokens: Some(4096),
                budget: Some(10.0),
            },
            replay: ReplayConfig {
                replay_dir: ".".to_string(),
                tank_cache_path: "data/tank_cache.json".to_string(),
                path_translate: "auto".to_string(),
            },
        }
    }
}

impl Config {
    /// 从 TOML 文件加载配置；若文件不存在则写入一份默认配置并返回。
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            // 首次运行：生成默认配置并落盘，方便用户编辑
            let config = Config::default();
            config.save(path)?;
            eprintln!("Created default config at {}", path.display());
            Ok(config)
        }
    }

    /// 把配置序列化成 TOML 并写回文件。
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

// =====================================================================
//  Token 用量统计（R6 要求）
//  精确记录每次调用的输入/输出 token 数、费用，支持预算上限自动中断。
// =====================================================================

/// 全局 token 用量累计与调用明细。
///
/// 每次 LLM 调用都会 [`record`](TokenUsage::record) 一条记录，
/// 并累加到总输入/输出 token、总费用和调用次数上。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    /// 累计输入 token 数
    pub total_input_tokens: u64,
    /// 累计输出 token 数
    pub total_output_tokens: u64,
    /// 累计费用（美元）
    pub total_cost: f64,
    /// 总调用次数
    pub call_count: u32,
    /// 每次调用的明细（按时间先后追加）
    pub calls: Vec<TokenCall>,
}

/// 单次调用的 token 用量记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCall {
    /// 调用时间（格式化字符串）
    pub timestamp: String,
    /// 使用的模型名
    pub model: String,
    /// 本次输入 token 数
    pub input_tokens: u64,
    /// 本次输出 token 数
    pub output_tokens: u64,
    /// 本次费用（美元）
    pub cost: f64,
    /// 调用用途（如 `chat`）
    pub purpose: String,
}

impl TokenUsage {
    /// 构造空的用量统计。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次调用：根据模型单价换算成本，并累加各项总量。
    ///
    /// 费用公式：`cost = input/1000 * price_input + output/1000 * price_output`
    pub fn record(
        &mut self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        price_input: f64,
        price_output: f64,
        purpose: &str,
    ) {
        let cost = (input_tokens as f64 / 1000.0 * price_input)
            + (output_tokens as f64 / 1000.0 * price_output);

        self.calls.push(TokenCall {
            timestamp: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            model: model.to_string(),
            input_tokens,
            output_tokens,
            cost,
            purpose: purpose.to_string(),
        });

        self.total_input_tokens += input_tokens;
        self.total_output_tokens += output_tokens;
        self.total_cost += cost;
        self.call_count += 1;
    }

    /// 当前累计费用是否未超过预算（用于 Agent Loop 每步检查）。
    pub fn check_budget(&self, budget: f64) -> bool {
        self.total_cost < budget
    }

    /// 把用量统计写为 JSON 文件（`token_usage.json`）。
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// 从 JSON 文件加载用量统计；文件不存在则返回空统计。
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let content = std::fs::read_to_string(path)?;
        let usage: TokenUsage = serde_json::from_str(&content)?;
        Ok(usage)
    }

    /// 打印用量汇总 + 最近 10 次调用明细（`usage` 命令）。
    pub fn print_summary(&self) {
        println!();
        println!("========================================================");
        println!("  Token Usage Summary");
        println!("========================================================");
        println!();
        println!("  Total calls:        {}", self.call_count);
        println!("  Total input tokens: {}", self.total_input_tokens);
        println!("  Total output tokens: {}", self.total_output_tokens);
        println!("  Total tokens:       {}", self.total_input_tokens + self.total_output_tokens);
        println!("  Total cost:        ${:.4}", self.total_cost);
        println!();

        if !self.calls.is_empty() {
            println!("  Recent calls:");
            println!("  {:<20} {:<20} {:>8} {:>8} {:>8} {:<15}",
                "Time", "Model", "Input", "Output", "Cost", "Purpose");
            println!("  {}", "-".repeat(85));
            for call in self.calls.iter().rev().take(10) {
                println!("  {:<20} {:<20} {:>8} {:>8} {:>7.4} {:<15}",
                    call.timestamp, call.model,
                    call.input_tokens, call.output_tokens,
                    call.cost, call.purpose);
            }
        }
        println!();
        println!("========================================================");
    }
}

#[cfg(test)]
mod path_translate_tests {
    use super::ReplayConfig;

    #[test]
    fn wsl_style_to_windows() {
        assert_eq!(
            ReplayConfig::translate_with_mode("/mnt/c/Users/me/a.wotbreplay", "windows"),
            "C:/Users/me/a.wotbreplay");
        assert_eq!(
            ReplayConfig::translate_with_mode("/mnt/C/replay_samples", "windows"),
            "C:/replay_samples");
        assert_eq!(
            ReplayConfig::translate_with_mode("/mnt/d/x", "windows"),
            "D:/x");
    }

    #[test]
    fn windows_style_to_wsl() {
        assert_eq!(
            ReplayConfig::translate_with_mode("C:\\Users\\me\\a.wotbreplay", "wsl"),
            "/mnt/c/Users/me/a.wotbreplay");
        assert_eq!(
            ReplayConfig::translate_with_mode("c:/Users/me/a.wotbreplay", "wsl"),
            "/mnt/c/Users/me/a.wotbreplay");
    }

    #[test]
    fn auto_converts_to_running_platform() {
        let out = ReplayConfig::translate_with_mode("/mnt/c/x/a.wotbreplay", "auto");
        if cfg!(windows) {
            assert_eq!(out, "C:/x/a.wotbreplay");
        } else {
            assert_eq!(out, "/mnt/c/x/a.wotbreplay");   // 已是本地形式,不变
        }
        let out = ReplayConfig::translate_with_mode("C:\\x\\a.wotbreplay", "auto");
        if cfg!(windows) {
            assert_eq!(out, "C:\\x\\a.wotbreplay");     // 已是本地形式,不变
        } else {
            assert_eq!(out, "/mnt/c/x/a.wotbreplay");
        }
    }

    #[test]
    fn off_and_native_pass_through() {
        assert_eq!(ReplayConfig::translate_with_mode("/mnt/c/x", "off"), "/mnt/c/x");
        assert_eq!(ReplayConfig::translate_with_mode("C:\\x", "off"), "C:\\x");
        // 非盘符路径(相对路径 / home / UNC)任何模式都不动
        assert_eq!(ReplayConfig::translate_with_mode("replay_samples", "auto"), "replay_samples");
        assert_eq!(ReplayConfig::translate_with_mode("/home/u/a.wotbreplay", "auto"), "/home/u/a.wotbreplay");
        assert_eq!(ReplayConfig::translate_with_mode("\\\\srv\\share\\a", "wsl"), "\\\\srv\\share\\a");
    }

    #[test]
    fn quotes_and_whitespace_trimmed() {
        assert_eq!(
            ReplayConfig::translate_with_mode("  \"C:\\Users\\me\\a.wotbreplay\"  ", "wsl"),
            "/mnt/c/Users/me/a.wotbreplay");
    }

    #[test]
    fn short_and_malformed_inputs_safe() {
        assert_eq!(ReplayConfig::translate_with_mode("", "auto"), "");
        assert_eq!(ReplayConfig::translate_with_mode("/mnt/", "windows"), "/mnt/");
        assert_eq!(ReplayConfig::translate_with_mode("C:", "wsl"), "C:");
        assert_eq!(ReplayConfig::translate_with_mode("/mnt/c", "windows"), "/mnt/c");  // 无尾分隔符,不强转
    }
}
