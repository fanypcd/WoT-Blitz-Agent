use crate::wargaming::api_client::PlayerStats;
use anyhow::Result;
use serde::Serialize;

// =====================================================================
//  对局前瞻（阵容强度分析）
//  进入对局前，对比双方玩家的累计战绩，识别高威胁与薄弱点，
//  生成"重点规避谁、优先集火谁"的开局建议。
// =====================================================================

/// 一支阵容的强度分析报告。
#[derive(Debug, Clone, Serialize)]
pub struct LineupReport {
    pub total_players: usize,
    pub avg_win_rate: f32,
    pub avg_damage: f32,
    pub strongest: PlayerInfo,
    pub weakest: PlayerInfo,
    pub threats: Vec<PlayerInfo>,
    pub weaknesses: Vec<PlayerInfo>,
    pub suggestion: String,
}

/// 单名玩家的强度信息（供阵容分析）。取自其排位战绩。
#[derive(Debug, Clone, Serialize)]
pub struct PlayerInfo {
    pub nickname: String,
    pub account_id: u32,
    pub battles: u32,
    /// 排位胜率（百分比）
    pub win_rate: f32,
    /// 排位场均伤害
    pub avg_damage: f32,
    /// 排位 mm 评级
    pub rating: Option<f32>,
}

impl From<&PlayerStats> for PlayerInfo {
    /// 从 PlayerStats 提取排位战力指标。
    fn from(s: &PlayerStats) -> Self {
        let battles = s.rating_battles.max(1);
        let win_rate = if battles > 0 {
            s.rating_wins as f32 / battles as f32 * 100.0
        } else {
            0.0
        };
        let avg_damage = if battles > 0 {
            s.rating_damage_dealt as f32 / battles as f32
        } else {
            0.0
        };
        Self {
            nickname: s.nickname.clone(),
            account_id: s.account_id,
            battles: s.rating_battles,
            win_rate,
            avg_damage,
            rating: s.rating_mm_rating,
        }
    }
}

/// 分析一组玩家的阵容强度，给出报告。
pub fn analyze_lineup(players: Vec<PlayerStats>) -> Result<LineupReport> {
    if players.is_empty() {
        anyhow::bail!("No players to analyze");
    }

    let infos: Vec<PlayerInfo> = players.iter().map(PlayerInfo::from).collect();

    // 阵容均值
    let avg_win_rate = infos.iter().map(|p| p.win_rate).sum::<f32>() / infos.len() as f32;
    let avg_damage = infos.iter().map(|p| p.avg_damage).sum::<f32>() / infos.len() as f32;

    // 按场均伤害排序，取最强 / 最弱
    let mut sorted = infos.clone();
    sorted.sort_by(|a, b| b.avg_damage.partial_cmp(&a.avg_damage).unwrap());
    let strongest = sorted[0].clone();
    let weakest = sorted[sorted.len() - 1].clone();

    // 高威胁：场均伤害 ≥ 均值 × 1.2
    let threat_threshold = avg_damage * 1.2;
    let threats: Vec<PlayerInfo> = infos.iter()
        .filter(|p| p.avg_damage >= threat_threshold)
        .cloned()
        .collect();

    // 薄弱点：胜率 ≤ 均值 × 0.95
    let weak_threshold = avg_win_rate * 0.95;
    let weaknesses: Vec<PlayerInfo> = infos.iter()
        .filter(|p| p.win_rate <= weak_threshold)
        .cloned()
        .collect();

    let suggestion = build_suggestion(&infos, &strongest, &weakest, &threats, &weaknesses);

    Ok(LineupReport {
        total_players: infos.len(),
        avg_win_rate,
        avg_damage,
        strongest,
        weakest,
        threats,
        weaknesses,
        suggestion,
    })
}

/// 根据分析结果拼装人类可读的开局建议文本。
fn build_suggestion(
    _infos: &[PlayerInfo],
    strongest: &PlayerInfo,
    weakest: &PlayerInfo,
    threats: &[PlayerInfo],
    weaknesses: &[PlayerInfo],
) -> String {
    let mut parts = Vec::new();

    if !threats.is_empty() {
        let names: Vec<&str> = threats.iter().map(|p| p.nickname.as_str()).collect();
        parts.push(format!(
            "高威胁玩家（场均伤害超均值）：{}",
            names.join(", ")
        ));
    }

    if !weaknesses.is_empty() {
        let names: Vec<&str> = weaknesses.iter().map(|p| p.nickname.as_str()).collect();
        parts.push(format!(
            "可针对弱点（胜率偏低）：{}",
            names.join(", ")
        ));
    }

    parts.push(format!(
        "最强者 {}（{} 伤害 / {:.0}% 胜率）需重点规避；最弱 {}（{} 伤害 / {:.0}% 胜率）是突破口。",
        strongest.nickname,
        strongest.avg_damage as u32,
        strongest.win_rate,
        weakest.nickname,
        weakest.avg_damage as u32,
        weakest.win_rate,
    ));

    parts.join("；")
}

/// 把阵容分析报告打印到 stdout（`prematch` 命令 / Web 前瞻面板用）。
pub fn print_report(report: &LineupReport) {
    println!("\n=== 阵容强度分析 ===");
    println!("玩家总数: {}", report.total_players);
    println!("平均胜率: {:.1}%", report.avg_win_rate);
    println!("平均场均伤害: {:.0}", report.avg_damage);
    println!();
    println!("最强者: {} (场均伤害 {:.0}, 胜率 {:.1}%)",
        report.strongest.nickname, report.strongest.avg_damage, report.strongest.win_rate);
    println!("最弱者: {} (场均伤害 {:.0}, 胜率 {:.1}%)",
        report.weakest.nickname, report.weakest.avg_damage, report.weakest.win_rate);
    println!();
    println!("高威胁: {}", report.threats.iter()
        .map(|p| p.nickname.clone()).collect::<Vec<_>>().join(", "));
    println!("薄弱点: {}", report.weaknesses.iter()
        .map(|p| p.nickname.clone()).collect::<Vec<_>>().join(", "));
    println!();
    println!("建议: {}", report.suggestion);
    println!();
}
