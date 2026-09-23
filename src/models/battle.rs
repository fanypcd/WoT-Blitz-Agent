use serde::{Deserialize, Serialize};

/// 单场战斗的汇总信息：一场 7v7 对局（14 名玩家）的地图、模式、胜负及作者与全部玩家战绩。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleSummary {
    /// `.wotbreplay` 回放文件名（如 `20260730_1916__Anonyme_E-100_xxx.wotbreplay`）
    pub file_name: String,
    /// 战斗开始时间戳（秒）
    pub timestamp: i64,
    /// 战斗开始时间（格式化字符串，如 `2026-07-30 19:16:00`）
    pub datetime: String,
    /// 对局模式（Rating 排位 / Regular 随机 / TrainingRoom 训练）
    pub room_type: String,
    pub map_id: u32,
    pub map_name: String,
    /// 战斗总时长（秒）
    pub battle_duration_secs: f64,
    /// 获胜队伍（1 或 2）
    pub winner_team: u8,
    pub author_account_id: u32,
    pub author_nickname: String,
    pub author_tank_id: u32,
    pub author_tank_name: String,
    /// 作者所在队伍（1 或 2）
    pub author_team: u8,
    pub author_won: bool,
    pub author: AuthorStats,
    /// 全部 14 名玩家（含作者）的战绩
    pub players: Vec<PlayerSummary>,
}

/// 作者（当前玩家）的详细战斗统计，主要来自回放的 `battle_results.dat`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorStats {
    /// 战斗结束时剩余血量（0 表示被击毁）
    pub hitpoints_left: i32,
    /// 本次战斗获得的银币（有是否自动击毁等修正）
    pub total_credits: u32,
    pub total_xp: u32,
    pub n_shots: u32,
    pub n_hits: u32,
    /// 溅射命中次数（HE 弹）
    pub n_splashes: u32,
    pub n_penetrations: u32,
    pub damage_dealt: u32,
    /// 是否被判定为"自动击毁"（如溺水、坠桥、友军击杀）
    pub is_auto_destroyed: bool,
}

/// 单名玩家的完整战绩（一场战斗共 14 名，含作者自己）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSummary {
    pub account_id: u32,
    pub nickname: String,
    /// 所在队伍（1 或 2）
    pub team: u8,
    /// 组队 ID（组队玩家标识，单独玩家为 None）
    pub platoon_id: Option<u32>,
    pub clan_tag: Option<String>,
    pub tank_id: u32,
    pub tank_name: String,
    pub base_xp: u32,
    pub credits_earned: u32,
    pub n_shots: u32,
    pub n_hits_dealt: u32,
    pub n_penetrations_dealt: u32,
    pub damage_dealt: u32,
    /// 格挡伤害（被敌弹挡住的部分）
    pub damage_blocked: u32,
    /// 助攻伤害（如点亮协助）
    pub damage_assisted_1: u32,
    /// 助攻伤害（如断带协助）
    pub damage_assisted_2: u32,
    pub n_hits_received: u32,
    pub n_penetrations_received: u32,
    pub n_enemies_damaged: u32,
    pub n_enemies_destroyed: u32,
    /// 排位 mm 评级
    pub mm_rating: Option<f32>,
    /// 排位显示评级
    pub display_rating: Option<u32>,
}

impl BattleSummary {
    /// 构造全默认值的空战斗汇总，用于缺少回放字段时的占位（模式 Regular / 当前时间）。
    pub fn from_naive(timestamp: i64) -> Self {
        let dt = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| timestamp.to_string());
        Self {
            file_name: String::new(),
            timestamp,
            datetime: dt,
            room_type: "Regular".to_string(),
            map_id: 0,
            map_name: String::new(),
            battle_duration_secs: 0.0,
            winner_team: 0,
            author_account_id: 0,
            author_nickname: String::new(),
            author_tank_id: 0,
            author_tank_name: String::new(),
            author_team: 0,
            author_won: false,
            author: AuthorStats {
                hitpoints_left: 0,
                total_credits: 0,
                total_xp: 0,
                n_shots: 0,
                n_hits: 0,
                n_splashes: 0,
                n_penetrations: 0,
                damage_dealt: 0,
                is_auto_destroyed: false,
            },
            players: Vec::new(),
        }
    }
}
