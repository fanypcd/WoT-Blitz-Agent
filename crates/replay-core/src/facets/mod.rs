//! 消费方数据切面（架构契约 v2）：Rust 内部模型 ≠ 对外 DTO。
//!
//! 能力边界：Agent 只暴露**结果解释**与**时序解释**两个维度，都是纯投影，
//! 依赖方向恒为 投影 → 模型（`replay::model`）：
//! - **结果能力（Result interpretation）** = `models::battle::BattleSummary`，
//!   由 `ReplayParser` 直接产出（毫秒级，不读包流）；
//! - **回放切面** = 全场时序（位姿网格/炮线/击杀/阶段/可见性），序列化形态即
//!   `replay::playback::PlaybackData`（含 visibility；前端 `/api/playback/data` 已在线）；
//! - **智能体评审切面** = 花名册 + 归一化事件流 + 结算锚点（→ Java → 大语言模型）。
//!
//! 名人堂（HoF）不是 Agent 公开能力：它是消费方（WotBTools）产品域，由消费方
//! 从结果能力自行投影——Agent 不感知消费方的下游产品（契约 v2）。
//!
//! 原则：unknown ≠ 0 ≠ false——缺失一律 null/Option；分发机制（npm 包/构建产物/
//! 传输接口）不在本层定义，serde JSON 即契约本体。CLI/写盘 IO 胶水留在 wotb-agent
//!（`facets::export_cli`）。

pub mod ai_review;

pub use crate::replay::playback::PlaybackData as PlaybackFacet;
pub use ai_review::AiReviewFacet;

use std::collections::HashMap;

use crate::models::battle::BattleSummary;
use crate::replay::combat::feedback_code;
use crate::replay::model::ReplayModel;

/// 一条结算互验项（0x0c 过程计数 vs 结算总量，作者口径）。
#[derive(Debug)]
pub struct CrossCheck {
    pub label: &'static str,
    /// 结算总量（None = 结算补充字段缺失，无从对账）
    pub settlement: Option<u32>,
    /// 计数流最终 count
    pub counter_count: u32,
    /// 计数流最终 value
    pub counter_value: u32,
}

impl CrossCheck {
    /// None = 结算缺失无法判定；对不上返回 false——导出方应停下人工判读，禁止改语义硬凑。
    pub fn ok(&self) -> Option<bool> {
        let s = self.settlement?;
        Some(s == self.counter_count || s == self.counter_value)
    }
}

/// 作者反馈计数 × 结算互验：击杀（code 3）与点亮（code 2）。
/// count/value 哪个是"次数"口径未逐项复核——两者任一对上即判 OK。
pub fn cross_check_author_counters(
    model: &ReplayModel,
    summary: &BattleSummary,
) -> Vec<CrossCheck> {
    let author = summary
        .players
        .iter()
        .find(|p| p.account_id == summary.author_account_id);
    let mut max_by_code: HashMap<u8, (u32, u32)> = Default::default();
    for e in &model.timeline.counters {
        let slot = max_by_code.entry(e.event_code).or_insert((0, 0));
        slot.0 = slot.0.max(e.count as u32);
        slot.1 = slot.1.max(e.value as u32);
    }
    let mk = |label: &'static str, code: u8, settlement: Option<u32>| {
        let (count, value) = max_by_code.get(&code).copied().unwrap_or((0, 0));
        CrossCheck {
            label,
            settlement,
            counter_count: count,
            counter_value: value,
        }
    };
    vec![
        mk(
            "击杀 code=KILL vs n_enemies_destroyed",
            feedback_code::KILL,
            author.map(|a| a.n_enemies_destroyed),
        ),
        mk(
            "点亮 code=SPOTTED vs n_enemies_spotted",
            feedback_code::SPOTTED,
            author.and_then(|a| a.n_enemies_spotted),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 互验判定：任一口径对上即 OK；结算缺失 = None；全对不上 = false。
    #[test]
    fn cross_check_verdicts() {
        let mut model = ReplayModel::default();
        model
            .timeline
            .counters
            .push(crate::replay::combat::FeedbackCounterEvent {
                clock: 1.0,
                avatar_eid: 1,
                event_code: feedback_code::KILL,
                seq: 0,
                count: 2,
                value: 2,
            });
        let mut summary = BattleSummary::from_naive(0);
        let mut p = crate::models::battle::PlayerSummary::for_test(7, "a");
        p.n_enemies_destroyed = 2;
        summary.author_account_id = 7;
        summary.players.push(p);

        let checks = cross_check_author_counters(&model, &summary);
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].ok(), Some(true), "击杀 count=2 == 结算 2");
        assert_eq!(checks[1].ok(), None, "点亮结算缺失 → 无法判定");

        // 全对不上
        summary.players[0].n_enemies_destroyed = 5;
        let checks = cross_check_author_counters(&model, &summary);
        assert_eq!(checks[0].ok(), Some(false));
    }
}
