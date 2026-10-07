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
    /// 名人堂/去重键（battle_results.dat pickle tuple[0] 的 arenaUniqueId）。以
    /// **字符串**输出：该值可超 JS 安全整数（实测 581811140883713611）。
    #[serde(default)]
    pub arena_id: Option<String>,
    /// meta.json 的原始 arenaBonusType 数值（名人堂准入白名单 {1,7}、联赛模式
    /// {2,4} 判定依据；room_type 字符串仅为其枚举名，不替代数值）。
    #[serde(default)]
    pub arena_bonus_type: Option<u32>,
    /// 结算根字段 finishReason（1 全歼 EXTERMINATION / 6 积分上限 WIN_POINTS_CAP；
    /// 其余值原始透传）。缺省 None = 未取得。
    #[serde(default)]
    pub finish_reason: Option<u32>,
    /// 结算层公共战斗时长（**整秒**，root f5）。与 `battle_duration_secs`（meta.json
    /// 来源）不同源——后者不是可靠的对局时钟，此字段才是结算口径。
    #[serde(default)]
    pub result_duration_secs: Option<u32>,
    /// 客户端版本串（`data.wotreplay` 头部，如 `11.20.0`）。协议语义只在 11.19/11.20
    /// 验证，消费方据此做版本门禁。缺省 None（不猜）。
    #[serde(default)]
    pub client_version: Option<String>,
    pub map_id: u32,
    pub map_name: String,
    /// meta.json 原始 `mapName`（地图代号，如 `skit`）。底图 / 语义 / i18n 按此键控；
    /// `map_name` 是解析器枚举名，未知地图会退化为 `map_{id}`。缺省 None（不猜）。
    #[serde(default)]
    pub map_key: Option<String>,
    /// 结算阵容完整性：battle_results 花名册（players）与战绩（player_results）的账号集合完全一致
    /// （所有参战成员都有结算记录、花名册无无法解释的多余账号）。任一侧为空 → false；
    /// 未读到 battle_results → None。消费方据此决定能否用「一方全员阵亡」推导结束方式。
    #[serde(default)]
    pub roster_complete: Option<bool>,
    /// meta.json 原始 `playerVehicleName`（录像者车辆代号，如 `GB84_Chieftain_Mk6`）。缺省 None（不猜）。
    #[serde(default)]
    pub author_vehicle_codename: Option<String>,
    /// 战斗总时长（秒）
    pub battle_duration_secs: f64,
    /// 获胜队伍（1 或 2）；0 = 无胜方（平局 / 结算缺胜方字段）
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
    /// 承受伤害（#301 f11；None = 结算缺失或字段未下发——unknown ≠ 0，0 是真实数值）
    #[serde(default)]
    pub damage_received: Option<u32>,
    /// 争霸/积分模式获得点数（#301 f32；非该模式为 None）
    #[serde(default)]
    pub victory_points_earned: Option<u32>,
    /// 争霸/积分模式夺取点数（#301 f33；非该模式为 None）
    #[serde(default)]
    pub victory_points_seized: Option<u32>,
    /// 结算剩余血量（#301 f1，**全玩家**；负值/哨兵族为终态，原样透传）
    #[serde(default)]
    pub hitpoints_left: Option<i32>,
    /// 段位/状态（root #201 info f9）。**模式相关语义随版本解释**（PROVEN/PARTIAL），
    /// 仅作展示列，不做跨模式比较。
    #[serde(default)]
    pub rank: Option<u32>,
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
    // ===== 结算补充字段（battle_results #301 crate 未暴露部分，wargaming::battle_results_extra）=====
    /// 死亡原因：-1=存活哨兵、缺省=普通击毁、1=火焰、2=撞击、3=世界/环境（结算缺失时 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death_reason: Option<i32>,
    /// 是否存活（death_reason == -1 推导；death_reason 缺省 = 普通击毁 → false；
    /// 只有该战斗者整条结算缺失时才为 None，不猜）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub survived: Option<bool>,
    /// 存活寿命（整秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub life_time_secs: Option<u32>,
    /// 击杀者 ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub killer_id: Option<u32>,
    /// 点亮敌人数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n_enemies_spotted: Option<u32>,
    /// 毁灭协助次数（≥25% 伤害后盟友击毁）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destruction_assistance: Option<u32>,
    /// 炮印数（0..3）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gun_marks: Option<u32>,
    /// 经验（#301 f23，WotbTools PROVEN；crate `base_xp` 在 11.19 语料中为 0，以此为准）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xp: Option<u32>,
    /// 银币（#301 f106，WotbTools PROVEN；crate `credits_earned` 在 11.19 语料中为 0，以此为准）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<u32>,
    /// 本战斗者的结算 result/entity ID（#301 外层 f1；`killer_id` 引用此命名空间）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<u32>,
    /// 击杀者账号 ID：`killer_id` 经同场 `result_id` 联表得到；联不上为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub killer_account_id: Option<u32>,
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
            arena_id: None,
            arena_bonus_type: None,
            finish_reason: None,
            result_duration_secs: None,
            client_version: None,
            map_id: 0,
            map_name: String::new(),
            map_key: None,
            roster_complete: None,
            author_vehicle_codename: None,
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

#[cfg(test)]
impl PlayerSummary {
    /// 测试辅助：全默认值 + 指定账号/昵称。
    pub fn for_test(account_id: u32, nickname: &str) -> Self {
        Self {
            account_id,
            nickname: nickname.to_string(),
            team: 0,
            platoon_id: None,
            clan_tag: None,
            tank_id: 0,
            tank_name: String::new(),
            base_xp: 0,
            credits_earned: 0,
            n_shots: 0,
            n_hits_dealt: 0,
            n_penetrations_dealt: 0,
            damage_dealt: 0,
            damage_received: None,
            victory_points_earned: None,
            victory_points_seized: None,
            hitpoints_left: None,
            rank: None,

            damage_blocked: 0,
            damage_assisted_1: 0,
            damage_assisted_2: 0,
            n_hits_received: 0,
            n_penetrations_received: 0,
            n_enemies_damaged: 0,
            n_enemies_destroyed: 0,
            mm_rating: None,
            display_rating: None,
            death_reason: None,
            survived: None,
            life_time_secs: None,
            killer_id: None,
            n_enemies_spotted: None,
            destruction_assistance: None,
            gun_marks: None,
            xp: None,
            credits: None,
            result_id: None,
            killer_account_id: None,
        }
    }
}
