// ReplayDataset 契约：metadata / settlement / diagnostics。
//
// 目标：Rust Replay Core 对外输出**权威语义数据集**，而不是
// Rust-specific DTO / Agent-specific BattleSummary——未来 WotbTools Java 与 AI 编排
// consume 的是本契约。原则：
//   unknown ≠ 0 ≠ false ≠ 没发生 —— 观测缺失一律 Option/哨兵保留 raw，不编码为 0/false；
//   packet unsupported ≠ event didn't happen —— 未消费数据段进 diagnostics.unsupported。
//
// 待扩展：observations（entities/positions/hp/combat 时序）与 simulation
// （shots/projectiles/turret/gun）从 combat.rs 拆层输出。

use serde::Serialize;

use crate::models::battle::BattleSummary;

/// 单场回放的权威数据集（阶段 1 契约面）。
#[derive(Debug, Clone, Serialize)]
pub struct ReplayDataset {
    pub metadata: DatasetMetadata,
    pub settlement: Settlement,
    pub diagnostics: Diagnostics,
}

/// 元数据（回放自描述；battle_duration 为 meta 口径，权威时长用 settlement.battle.duration_secs）。
#[derive(Debug, Clone, Serialize)]
pub struct DatasetMetadata {
    pub file_name: String,
    /// 战斗开始 Unix 秒
    pub start_time: i64,
    pub map_id: u32,
    pub map_name: String,
    /// Rating / Regular / TrainingRoom
    pub room_type: String,
    /// 客户端版本串（未知 None——不猜）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
}

/// 结算层：battle 概览 + 全战斗者结果（含 crate 未暴露的补充字段）。
#[derive(Debug, Clone, Serialize)]
pub struct Settlement {
    pub battle: BattleSettlement,
    /// 全部战斗者（含作者；胜利方 = team == battle.winner）
    pub players: Vec<PlayerSettlementRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BattleSettlement {
    /// 获胜队伍（1/2）
    pub winner: u8,
    /// 结算口径整秒时长（battle_results root5；≠ meta.battleDuration——后者是元数据口径）
    pub duration_secs: Option<u32>,
    pub author_account_id: u32,
    pub author_team: u8,
    pub author_won: bool,
}

/// 单战斗者结算行（unknown 一律 Option；死亡原因语义见 battle_results_extra）。
#[derive(Debug, Clone, Serialize)]
pub struct PlayerSettlementRow {
    pub account_id: u32,
    pub nickname: String,
    pub team: u8,
    pub tank_id: u32,
    pub tank_name: String,
    pub damage_dealt: u32,
    pub kills: u32,
    /// 死亡原因：-1=存活哨兵、缺省=普通击毁、1=火、2=撞车、3=世界（None=结算缺失）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death_reason: Option<i32>,
    /// 是否存活（death_reason == -1 推导；None = 未知，不猜）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub survived: Option<bool>,
    /// 存活寿命（整秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub life_time_secs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub killer_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_enemies_spotted: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destruction_assistance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gun_marks: Option<u32>,
    pub n_shots: u32,
    pub n_hits: u32,
    pub n_penetrations: u32,
    pub damage_blocked: u32,
    pub damage_assisted_total: u32,
}

/// 诊断层：覆盖度（各数据段消费统计）/ 未消费数据段 / 质量降级汇总。
#[derive(Debug, Clone, Serialize, Default)]
pub struct Diagnostics {
    /// 数据段直方图：type → 包数（全量，未区分消费与否）
    pub packet_types: Vec<(u32, u64)>,
    /// 未消费数据段：语义未定/封存的 type（或 type+method 组）→ 包数。
    /// unsupported ≠ 没发生：只代表本版本解析器不消费。
    pub unsupported: Vec<(String, u64)>,
    /// 质量降级聚合：标记名 → 出现次数（来自各 ShotReplayData.quality）
    pub degradation: Vec<(String, u64)>,
    /// 提取射击数（作者/他人）
    pub shots_author: usize,
    pub shots_others: usize,
}

impl ReplayDataset {
    /// 从 BattleSummary（meta + battle_results 联表产物）投影 metadata/settlement。
    /// diagnostics 由调用方补充（需包流/射击复现数据）。
    pub fn from_summary(summary: &BattleSummary) -> Self {
        let settlement_players = summary
            .players
            .iter()
            .map(|p| PlayerSettlementRow {
                account_id: p.account_id,
                nickname: p.nickname.clone(),
                team: p.team,
                tank_id: p.tank_id,
                tank_name: p.tank_name.clone(),
                damage_dealt: p.damage_dealt,
                kills: p.n_enemies_destroyed,
                death_reason: p.death_reason,
                survived: p.survived,
                life_time_secs: p.life_time_secs,
                killer_id: p.killer_id,
                n_enemies_spotted: p.n_enemies_spotted,
                destruction_assistance: p.destruction_assistance,
                gun_marks: p.gun_marks,
                n_shots: p.n_shots,
                n_hits: p.n_hits_dealt,
                n_penetrations: p.n_penetrations_dealt,
                damage_blocked: p.damage_blocked,
                damage_assisted_total: p.damage_assisted_1 + p.damage_assisted_2,
            })
            .collect();
        ReplayDataset {
            metadata: DatasetMetadata {
                file_name: summary.file_name.clone(),
                start_time: summary.timestamp,
                map_id: summary.map_id,
                map_name: summary.map_name.clone(),
                room_type: summary.room_type.clone(),
                client_version: None,
            },
            settlement: Settlement {
                battle: BattleSettlement {
                    winner: summary.winner_team,
                    duration_secs: None, // 结算 root5（crate 未暴露）；meta 口径在 metadata.battle_duration
                    author_account_id: summary.author_account_id,
                    author_team: summary.author_team,
                    author_won: summary.author_won,
                },
                players: settlement_players,
            },
            diagnostics: Diagnostics::default(),
        }
    }
}
