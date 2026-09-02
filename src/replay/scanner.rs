use std::path::Path;
use anyhow::Result;

use crate::models::battle::BattleSummary;
use crate::replay::parser::{ReplayParser, list_replays_in_dir};

// =====================================================================
//  回放批量扫描
//  遍历一个目录下的所有 `.wotbreplay`，逐个解析并按条件过滤，
//  最终返回一场场的 BattleSummary 列表（由上层聚合成报告）。
// =====================================================================

/// 批量扫描器：内部持有 ReplayParser，按目录逐个解析回放。
pub struct ReplayScanner<'a> {
    parser: ReplayParser<'a>,
}

/// 扫描过滤器：按时间范围与对局模式筛选回放。
#[derive(Default)]
pub struct ScanFilter {
    /// 只保留时间戳晚于该值的战斗（不含 None）
    pub since: Option<i64>,
    /// 只保留时间戳早于该值的战斗（不含 None）
    pub until: Option<i64>,
    /// 只保留指定对局模式的战斗（如 Rating / Regular）
    pub room_type: Option<String>,
}


impl ScanFilter {
    /// 仅保留最近 `days` 天内的战斗。
    pub fn last_n_days(days: i64) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            since: Some(now - days * 86400),
            until: None,
            room_type: None,
        }
    }

    /// Build a filter from a mode name (all/rating/regular/training) and optional day limit.
    /// Single source of truth shared by CLI, Agent tools and the Web GUI.
    pub fn from_mode(mode: &str, days: Option<i64>) -> Self {
        let room = match mode {
            "rating" => Some("Rating"),
            "regular" => Some("Regular"),
            "training" => Some("TrainingRoom"),
            _ => None,
        };
        match (room, days) {
            (Some(r), Some(d)) => Self {
                since: Some(chrono::Utc::now().timestamp() - d * 86400),
                until: None,
                room_type: Some(r.to_string()),
            },
            (Some(r), None) => Self { since: None, until: None, room_type: Some(r.to_string()) },
            (None, Some(d)) => Self::last_n_days(d),
            (None, None) => Self::default(),
        }
    }

    /// 判断一场战斗是否满足过滤条件（时间区间 + 模式）。
    pub fn matches(&self, summary: &BattleSummary) -> bool {
        if let Some(since) = self.since {
            if summary.timestamp < since {
                return false;
            }
        }
        if let Some(until) = self.until {
            if summary.timestamp > until {
                return false;
            }
        }
        if let Some(ref rt) = self.room_type {
            if &summary.room_type != rt {
                return false;
            }
        }
        true
    }
}

/// 扫描进度回调结构：每解析完一个文件触发一次，供 CLI/Web 展示进度。
pub struct ScanProgress {
    /// 当前处理到的序号（从 1 开始）
    pub current: usize,
    /// 目录下文件总数
    pub total: usize,
    /// 当前文件名
    pub file_name: String,
    /// 是否成功解析
    pub ok: bool,
    /// 解析失败的错误信息（成功时为 None）
    pub error: Option<String>,
}

impl<'a> ReplayScanner<'a> {
    /// 构造不带解析器的扫描器。
    pub fn new() -> Self {
        Self { parser: ReplayParser::new() }
    }

    /// 构造带 TankResolver 的扫描器（可把 tank_id 翻译成名称）。
    pub fn with_resolver(resolver: &'a crate::wargaming::tank_resolver::TankResolver) -> Self {
        Self { parser: ReplayParser::with_resolver(resolver) }
    }

    /// 扫描目录，逐个解析回放并按 `filter` 过滤，返回命中场次（按时间排序）。
    ///
    /// `progress_callback` 在每解析一个文件后被调用，可用来渲染进度。
    pub fn scan_dir(
        &self,
        dir: &Path,
        filter: &ScanFilter,
        progress_callback: impl FnMut(ScanProgress),
    ) -> Result<Vec<BattleSummary>> {
        let files = list_replays_in_dir(dir)?;
        let total = files.len();
        let mut results = Vec::with_capacity(total);
        let mut callback = progress_callback;

        for (i, path) in files.iter().enumerate() {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();

            match self.parser.parse_file(path) {
                Ok(summary) => {
                    let ok = filter.matches(&summary);
                    if ok {
                        results.push(summary);
                    }
                    callback(ScanProgress {
                        current: i + 1,
                        total,
                        file_name: file_name.clone(),
                        ok: true,
                        error: None,
                    });
                }
                Err(e) => {
                    callback(ScanProgress {
                        current: i + 1,
                        total,
                        file_name: file_name.clone(),
                        ok: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        // 按时间升序排列（方便上层做时间序列/对比）
        results.sort_by_key(|b| b.timestamp);
        Ok(results)
    }
}

impl<'a> Default for ReplayScanner<'a> {
    fn default() -> Self {
        Self::new()
    }
}

