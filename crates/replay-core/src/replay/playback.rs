//! 全场实时回放数据层：把现有解析成果（渲染层滤波位姿 + prop2 炮塔角 + 射击/血量链）
//! 组装成前端可直接按 0.1s 网格推进的全场时间线。
//!
//! 与单发复现（viewer Shot Replay 滑块）同语义、不同消费方式：
//! - 位姿 = [`FilteredTimeline`]（filter.rs，客户端 AvatarFilter 移植）60Hz 输出的 0.1s 降采样；
//! - 炮塔角 = [`combat::prop2_at`]（客户端 0x1440C70 时间线语义：0.1s 前瞻/短弧插值/clamp 不外推）
//!   的相对角 + **同网格刻**车体 yaw 合成绝对角（与 combat.rs ⑩/⑪ 步 `rel + hullYaw` 同式）；
//! - 角度序列（hull_yaw/turret_yaw）落盘前做**相位解卷绕**（[`unwrap_angle`]）：相邻网格
//!   物理角差 ≪ π（0.1s 车体/炮塔极限转速远低于 90°/格），解卷绕后序列连续——消费方
//!   朴素线性插值即物理正确，±π 边界不出现 ≈2π 数值跳变（跳变会让插值反甩 ~360°）；
//! - 炮管俯仰 = prop2 frac 按车型极限解码（[`combat::decode_prop2_gun_pitch`]）；
//! - 射击 = 作者严格路径 + 他人宽松路径合并（与 web `replay_shots_handler` 同构）；
//! - 血量 = type=5 满血锚点 + method1 事件链；死亡 = type=7 sub=1（击杀者取 hp==0 事件 source）。
//!
//! 车辆筛选 = **st10 ∧ prop2 双流**（KineticObject/DetachedTurret 也有 type=10 移动流但无
//! 炮塔角广播）；花名册联表失败的车辆（昵称不在 battle_results 花名册，如观察者）
//! 保留为 team=0/tank_id=0 的"未知"车，不丢战局画面（昵称域为 UTF-8 全域，见 combat::nickname）。
//!
//! 序列化约定：位姿为列式 flat 数组（`pos` = [x,y,z]×N，其余各 N 项），时刻 `t_i = t_start + i*0.1`；
//! 前端线性插值即可（hull_yaw/turret_yaw 为解卷绕连续域，见上）。
//! **渲染位姿另有 `pose_kf`（关键帧折线）**：`pos`/`hull_yaw`/`hull_pitch` 的 10Hz 网格
//! 会把滤波器的保持-跳变阶梯混叠成速度摆动（见 [`PoseKeyframes`]），渲染消费方应以
//! `pose_kf` 为准（缺省 = 旧 facet，回退网格插值）；`turret_yaw`/`gun_pitch`/`hull_roll`
//! 仍只有网格列（prop2/原始采样语义，不参与折线）。
//! `coverage` = 有效数据区段（原始采样间隙 >2s 视为
//! AoI 空洞——滤波器在无输入期会原地站住，前端按此隐藏车辆避免"幽灵车停在过期位置"）。

use std::collections::{BTreeMap, HashMap};
/// serde skip_serializing_if 助手：`false` 按缺省处理（= 无已证实目标）
fn is_false(b: &bool) -> bool {
    !*b
}

use anyhow::bail;
use serde::Serialize;

use super::combat::{
    self, AoiPresence, AssaultBaseStateTransition, ConsumableTransition, GunPitchLimits,
    ModuleCrewStateEvent, RawReloadDuration, RawReloadPhase, ShotReplayData,
    SupremacyBaseStateTransition, SupremacyPointsSample,
};
use super::filter::{self, FilteredTimeline};

/// 位姿网格步长（秒）——与现有 render_timeline / prop2 密集采样同惯例
pub const GRID_DT: f32 = 0.1;
/// 位姿关键帧走廊容差：位置（米）。2cm 在典型机位（20~50m）下约 1px 量级，仍远超网格路径精度
const KF_TOL_POS: f32 = 0.02;
/// 位姿关键帧走廊容差：角度（弧度 ≈ 0.40°）
const KF_TOL_ANG: f32 = 0.007;
/// 原始采样间隙超过此值视为 AoI 空洞（coverage 断开）
const COVERAGE_GAP: f32 = 2.0;
/// 实体收录的最少 type=10 采样数（噪声/瞬时实体过滤）
const MIN_ST10_SAMPLES: usize = 20;

/// 玩家身份（battle_results 联表产物，调用方组装；replay 层不依赖 TankResolver）
#[derive(Debug, Clone)]
pub struct PlaybackPlayer {
    pub account_id: u32,
    pub nickname: String,
    /// 1 / 2（0 = 未知）
    pub team: u8,
    pub tank_id: u32,
}

/// 构建输入：packets + 战绩联表 + 俯仰极限锚定表 + 地图元信息
/// （`players`/`author_account_id` 仅由 [`build_playback_data`] 用于
/// [`crate::replay::model::ReplayModel::scan`] 联表；投影层身份一律取模型，
/// 见 [`PlaybackRenderInput`]）。
pub struct PlaybackInput<'a> {
    pub packets: &'a [(u32, f32, &'a [u8])],
    pub players: Vec<PlaybackPlayer>,
    pub author_account_id: u32,
    /// 胜方队伍（1/2，0 = 平局/未知）
    pub winner_team: u8,
    pub map_id: u32,
    pub map_name: String,
    pub pitch_limits: &'a GunPitchLimits,
    /// tank_id → 坦克名（调用方由 TankResolver 预解析；缺失可传空表）
    pub tank_names: HashMap<u32, String>,
}

/// 投影（渲染）入参：identity 一律来自 [`crate::replay::model::ReplayModel`] 实体并表
/// （eid → 昵称/account_id/team/tank_id/is_author），本结构**不含花名册**——投影层从
/// 类型上不可能重新 JOIN（身份域单一事实源，防昵称规则漂移）。
pub struct PlaybackRenderInput<'a> {
    /// 胜方队伍（1/2，0 = 平局/未知）
    pub winner_team: u8,
    pub map_id: u32,
    pub map_name: String,
    pub pitch_limits: &'a GunPitchLimits,
    /// tank_id → 坦克名（调用方由 TankResolver 预解析；缺失可传空表）
    pub tank_names: &'a HashMap<u32, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaybackMeta {
    pub map_id: u32,
    pub map_name: String,
    pub winner_team: u8,
    /// 作者队伍（友方视角配色基准）
    pub friendly_team: u8,
    /// 作者车辆实体 id（0 = 未解析）
    pub author_eid: u32,
    /// 全局网格起点（回放时钟域，秒）
    pub t_start: f32,
    /// 网格点数；时刻 t_i = t_start + i × 0.1s
    pub samples: usize,
    /// 总时长（秒）= t_start + (samples−1) × 0.1
    pub duration: f32,
}

/// 一辆车的全场时间线（列式；N = meta.samples，无效段由 coverage 控制）
#[derive(Debug, Clone, Serialize)]
pub struct VehicleTrack {
    pub eid: u32,
    pub account_id: u32,
    /// type=5 昵称（UTF-8 全域；缺失为空串 → 前端显示 Unknown）
    pub nickname: String,
    pub tank_id: u32,
    pub tank_name: String,
    /// 1 / 2（0 = 未知，前端灰色渲染）
    pub team: u8,
    pub is_author: bool,
    pub max_hp: u16,
    /// 车体位置 flat [x,y,z] × N（回放世界系，米）
    pub pos: Vec<f32>,
    /// 车体偏航（弧度，解卷绕连续域，可超 ±π）× N
    pub hull_yaw: Vec<f32>,
    /// 车体俯仰（弧度）× N
    pub hull_pitch: Vec<f32>,
    /// 车体侧倾（弧度）× N。**与 hull_pitch 不同源**：俯仰取自渲染滤波（AvatarFilter）输出，
    /// 而滤波层不输出侧倾，故本列取**原始 type=10 volatile 采样**的最近邻（与
    /// `combat::anchors::select_anchor_state` 对 pitch/roll 的规则一致：段内不插值，
    /// 不跨 AoI 断段编造中间姿态）；无采样时保持最近已知值。消费端可忽略（0 = 水平）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hull_roll: Vec<f32>,
    /// 炮塔绝对朝向（弧度，解卷绕连续域）= prop2 相对角 + 同刻车体 yaw 再解卷绕 × N
    pub turret_yaw: Vec<f32>,
    /// 炮管俯仰（弧度，正=仰角；无俯仰极限锚定时 = 车体 pitch 兜底）× N
    pub gun_pitch: Vec<f32>,
    /// HP 变化点 [(t, hp)]（含满血锚点）
    pub hp: Vec<(f32, u16)>,
    pub death_t: Option<f32>,
    /// 击杀者实体（method1 hp==0 事件 source；0 = 未知/环境）
    pub killer_eid: u32,
    /// 该车发射过的弹种全局 id（去重，最多 16 个）——实际搭载配置推断证据
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shell_ids: Vec<u32>,
    /// 开局 loadout raw item 描述符（6×14B；item[0..2]=3 消耗品、item[3..5]=3 给养，
    /// 内部字段未解码故原样透传）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loadout_items: Vec<[u8; 14]>,
    /// 9 字节装备选择串（每字节 = 装备数值 ID 的 ASCII 码点；缺省不输出）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipment: Option<[u8; 9]>,
    /// 实际搭载的炮塔配置（build_configs dense 索引；None = 证据不足，前端用顶级配置）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turret_index: Option<u32>,
    /// 实际搭载的主炮配置（dense 索引；语义同上）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gun_index: Option<u32>,
    /// 有效数据区段 flat [t0, t1, ...]（闭区间对）
    pub coverage: Vec<f32>,
    /// 位姿关键帧折线（渲染层**精确**表示，见 [`PoseKeyframes`]）。旧 facet 缺省 = None，
    /// 消费端此时回退到 10Hz 网格列（`pos`/`hull_yaw`/`hull_pitch`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose_kf: Option<PoseKeyframes>,
}

/// 位姿关键帧折线：客户端渲染路径的**折点**序列。消费端在相邻关键帧间**线性插值**，
/// 即复现客户端逐帧画面（关键帧之间的路径本身是线性的，偏差 ≤ 走廊容差 2cm/0.4°）。
///
/// 为什么不能只给 10Hz 网格：滤波器在「预测-保持」阶段（latency 未收敛到输入间隔，= 车辆
/// 刚进 AoI 的数秒内）逐帧输出是**保持-跳变阶梯**；按固定 10Hz 网格重采样、再在网格间线性
/// 插值，会把阶梯混叠成速度大幅摆动的滑行（实测某 0.5s 窗内前端线速度 4.5→29.6 m/s，
/// 同段真值稳定）——3D 回放里"一顿一顿/不连续"的观感来源。折线保留保持段两端与跳变段，
/// 点数 7~10 点/秒/车（≈ 位置更新率，与 10Hz 网格同量级）。
///
/// 时序语义与网格列一致：早于首点/晚于末点按端点保持。
#[derive(Debug, Clone, Default, Serialize)]
pub struct PoseKeyframes {
    /// 关键帧时刻（秒，回放时钟域，升序）
    pub t: Vec<f32>,
    /// flat [x,y,z] × K（回放世界系，米；与 `pos` 网格同坐标系）
    pub pos: Vec<f32>,
    /// 车体偏航（弧度，**解卷绕连续域**；与 `hull_yaw` 网格同域——相邻关键帧 |Δ| < π，
    /// 消费端朴素线性插值即物理正确）
    pub yaw: Vec<f32>,
    /// 车体俯仰（弧度；与 `hull_pitch` 网格同域）
    pub pitch: Vec<f32>,
}

/// 从 60Hz 渲染帧拟合关键帧折线（公开给探针/测试；facet 层在 `VehicleTrack.pose_kf` 落盘）。
///
/// 贪心走廊：从帧 i 起尽量延长到 j，使 [i, j] 内所有中间帧与线性插值之差 ≤ 容差
/// （位置 = 欧氏距离，角度 = 最短弧差）。走廊跨越「保持→跳变」时中间帧偏差必然超限
/// → 跳变被保留为一段陡斜率，而不是被抹平。
pub fn pose_keyframes(tl: &FilteredTimeline) -> PoseKeyframes {
    let (start, _) = tl.time_range();
    let n = tl.frame_count();
    let mut frames: Vec<KfPos> = Vec::with_capacity(n);
    let mut prev_yaw: Option<f32> = None;
    for k in 0..n {
        // 帧中点查询：floor((t−start)/dt) 恒等 = k，规避 1/60 累加的浮点边界
        let t = start + (k as f64 + 0.5) * filter::FRAME_DT;
        let Some(p) = tl.pose_at(t, false) else { break };
        // 航向解卷绕（与 `hull_yaw` 网格列同域）：消费端朴素线性插值即物理正确，
        // ±π 边界不出现 ≈2π 跳变（否则关键帧跨 ±π 时插值会反甩一整圈）
        let yaw = unwrap_angle(prev_yaw, p.ang[0]);
        prev_yaw = Some(yaw);
        frames.push(KfPos {
            t: p.time,
            pos: p.pos,
            yaw,
            pitch: p.ang[1],
        });
    }
    let mut out = PoseKeyframes::default();
    if frames.is_empty() {
        return out;
    }
    // 落盘舍入（pos 1cm / 角度 0.001rad / 时刻 0.1ms）：未舍入的 f32 按最短往返表示序列化
    // （≈9 字符/数），体积大 ~40%。时刻取 0.1ms 而不是 1ms——跳变段只有 ~17ms 长，毫秒级
    // 时间舍入会把该段斜率改变百分之几（1.4m 跳变 → 中段偏差 ~2cm），0.1ms 下可忽略。
    // 保持段内两端取值相同，舍入后仍逐值相等（阶梯不被破坏）。
    let push = |out: &mut PoseKeyframes, f: &KfPos| {
        out.t.push(r4(f.t as f32));
        out.pos
            .extend_from_slice(&[r2(f.pos[0]), r2(f.pos[1]), r2(f.pos[2])]);
        out.yaw.push(r3(f.yaw));
        out.pitch.push(r3(f.pitch));
    };
    push(&mut out, &frames[0]);
    let mut i = 0usize;
    while i + 1 < frames.len() {
        let mut j = i + 1;
        while j + 1 < frames.len() && kf_within(&frames, i, j + 1) {
            j += 1;
        }
        push(&mut out, &frames[j]);
        i = j;
    }
    out
}

/// 走廊判据：[i, j] 段内所有中间帧与线性插值的偏差是否都在容差内（角度 = 最短弧差）。
fn kf_within(frames: &[KfPos], i: usize, j: usize) -> bool {
    let a = frames[i];
    let b = frames[j];
    let span = b.t - a.t;
    if !(span > 0.0) {
        return false;
    }
    let dyaw = wrap_pi(b.yaw - a.yaw);
    for f in &frames[i + 1..j] {
        let u = ((f.t - a.t) / span) as f32;
        let mut d2 = 0.0f32;
        for c in 0..3 {
            let v = a.pos[c] + (b.pos[c] - a.pos[c]) * u;
            d2 += (v - f.pos[c]) * (v - f.pos[c]);
        }
        if d2 > KF_TOL_POS * KF_TOL_POS {
            return false;
        }
        if wrap_pi(f.yaw - a.yaw - dyaw * u).abs() > KF_TOL_ANG {
            return false;
        }
        if (f.pitch - (a.pitch + (b.pitch - a.pitch) * u)).abs() > KF_TOL_ANG {
            return false;
        }
    }
    true
}

/// 走廊拟合的中间帧（= 一帧 60Hz 渲染位姿）
#[derive(Clone, Copy)]
struct KfPos {
    t: f64,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
}

/// 一发射击（弹道飞行 + 结果标记；原始语义透传，前端做标签映射）
#[derive(Debug, Clone, Serialize)]
pub struct PlaybackShot {
    /// 开火时刻（回放时钟域，秒）
    pub t_fire: f32,
    pub shooter_eid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_eid: Option<u32>,
    /// 炮口（method29 launchPoint）
    pub from: [f32; 3],
    /// 弹道终点（method20，穿透出射/停止点）
    pub to: [f32; 3],
    /// 直线近似飞行时长（秒）= |to−from| / |launch_velocity|
    pub flight_secs: f32,
    pub shell_speed: f32,
    /// 是否命中车辆（有可解析目标）
    pub hit: bool,
    /// hit_flags 0x0008 跳弹位
    pub ricochet: bool,
    /// 游戏命中结果枚举（0=无 1=未击穿 2=间隙止 3=有伤害 4=履带/模块 255=未获取）
    pub game_hit_result: u8,
    pub damage: u32,
    pub is_kill: bool,
    pub is_author: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub shell_kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub shooter_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target_name: String,
}

/// 击杀事件（kill feed）
#[derive(Debug, Clone, Serialize)]
pub struct KillEvent {
    pub t: f32,
    pub killer_eid: u32,
    pub victim_eid: u32,
    /// method1 cause 原始值（0=炮弹直击 1=火焰 2=撞击 3=世界/环境 5=溺水；255=未获取）
    pub cause: u8,
    /// wrapper6 >50% 先前伤害助攻者（官方击杀通知同款；无则 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assister_eid: Option<u32>,
    /// wrapper6 非默认死亡原因（1=火 2=撞 3=世界 5=溺水；缺省=普通击毁）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death_reason: Option<u32>,
}

/// 实际搭载配置描述符（updateArena subtype 1 ARENA_INFO 的 field 1.2 blob）：
/// 15B = `[tank_id u16][chassis_local u16][engine_local u16][204 u32][turret_local u16][gun_local u16][00]`，
/// local = item_defs 模块局部 id（module_id >> 8，与 tanks.pb module_id 同基）。确定性搭载证据。
#[derive(Debug, Clone)]
pub struct CompDescriptor {
    /// blob 所属玩家昵称（同包 field 1.3，原始 UTF-8 不经 ascii 过滤）
    pub nickname: String,
    pub tank_id: u32,
    pub turret_local: u16,
    pub gun_local: u16,
}

impl CompDescriptor {
    /// 从 ARENA_INFO args（去 [subtype][len] 头后的 protobuf）提取：field 1.2 = blob、field 1.3 = 昵称。
    /// 容错扫描：blob 以 tank_id u16le 开头且长度 15、前置标记 `12 0f`——对分组/前缀等
    /// 编码变体稳健。
    fn parse_args(args: &[u8], valid_tanks: &[u32]) -> Option<Self> {
        for i in 2..args.len().saturating_sub(14) {
            if args[i - 2] != 0x12 || args[i - 1] != 0x0f {
                continue;
            }
            let blob = &args[i..i + 15];
            let tank_id = u16::from_le_bytes([blob[0], blob[1]]) as u32;
            if !valid_tanks.contains(&tank_id) {
                continue;
            }
            let turret_local = u16::from_le_bytes([blob[10], blob[11]]);
            let gun_local = u16::from_le_bytes([blob[12], blob[13]]);
            if turret_local == 0 || gun_local == 0 {
                continue;
            }
            // 昵称 = blob 之后首个 `1a <len> <可打印 UTF-8>`（3..30B）
            let mut nickname = String::new();
            for j in (i + 15)..args.len().min(i + 60) {
                if args[j] != 0x1a {
                    continue;
                }
                let l = args[j + 1] as usize;
                if !(3..=30).contains(&l) || j + 2 + l > args.len() {
                    continue;
                }
                if let Ok(s) = std::str::from_utf8(&args[j + 2..j + 2 + l]) {
                    if s.chars().all(|c| !c.is_control()) {
                        nickname = s.to_string();
                    }
                }
                break;
            }
            return Some(Self {
                nickname,
                tank_id,
                turret_local,
                gun_local,
            });
        }
        None
    }
}

/// 收集全部玩家实际搭载描述符：subtype=1 的 updateArena（ARENA_INFO，每玩家一条广播）。
/// `valid_tanks` = battle_results 里的 tank_id 全集（blob[0..2] 白名单，防误配）。
pub fn collect_comp_descriptors(
    packets: &[(u32, f32, &[u8])],
    valid_tanks: &[u32],
) -> HashMap<String, CompDescriptor> {
    comp_descriptors_from_updates(
        &combat::collect_arena_updates_filtered(packets, |s| s == 1),
        valid_tanks,
    )
}

/// [`collect_comp_descriptors`] 的共享扫描形态：从已收集的 ARENA_INFO（subtype=1）
/// update 流提取（`ReplayModel::scan` 与 kill_feed/periods 合用一次 arena pass 时复用）。
pub fn comp_descriptors_from_updates(
    updates: &[combat::ArenaUpdate],
    valid_tanks: &[u32],
) -> HashMap<String, CompDescriptor> {
    let mut out: HashMap<String, CompDescriptor> = HashMap::new();
    let valid: Vec<u32> = valid_tanks.to_vec();
    for u in updates {
        if u.subtype != 1 || u.payload.len() < 15 {
            continue;
        }
        if let Some(d) = CompDescriptor::parse_args(&u.payload, &valid) {
            out.insert(d.nickname.clone(), d);
        }
    }
    out
}

/// 战局阶段点（updateArena PERIOD；前端据此显示战斗计时器）
#[derive(Debug, Clone, Serialize)]
pub struct PeriodPoint {
    pub clock: f32,
    pub period: u64,
    pub remaining_s: f64,
    pub duration_s: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaybackData {
    /// 切面契约版本（**当前 2**；不兼容变更递增，同版本只加可选字段；消费端按版本门禁拒绝错版）
    #[serde(default)]
    pub version: u32,
    /// Supremacy 基地状态时间线（争霸模式；非争霸场为空）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supremacy_bases: Vec<SupremacyBaseStateTransition>,
    /// Supremacy 实时点数采样（仅真实广播；非争霸场为空）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supremacy_points: Vec<SupremacyPointsSample>,
    /// 攻防战/遭遇战单基地目标存在性（wrapper8/root8 目标族出现即真，**不要求有进度**）。
    /// 缺省/ false = 无已证实的单基地目标（与 WotbTools `assaultObjectivePresent` 同义）。
    #[serde(default, skip_serializing_if = "is_false")]
    pub assault_objective_present: bool,
    /// 攻防战单基地占领进度（wrapper8/root8；非攻防战场次为空）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assault_bases: Vec<AssaultBaseStateTransition>,
    /// 消耗品生命周期事件（Type32 flag=0；非消耗品场次为空）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumables: Vec<ConsumableTransition>,
    /// 车辆模块/乘员状态事件（Avatar method16；无事件场次为空）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub module_crew_states: Vec<ModuleCrewStateEvent>,

    /// 区域可破坏物实体（100m 格子锚点；type=5 class=3，eid 升序）。
    /// `destructible_events[].area_eid` 联表本列表得事件所属格子。
    /// 契约 additive：消费方忽略未知键。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destructible_areas: Vec<crate::replay::destructibles::DestructibleArea>,
    /// 区域可破坏物事件（type=32 短广播、envelope=区域实体；时钟升序）。
    /// `object_id` = 服务器侧 u16 物体 id（body_len=5/6 位置已验证；PARTIAL——
    /// 每图窄窗口、与客户端静态表不同域，跨图标定属资产管线）；
    /// `body_len`/`args` 原样透传。事件与车辆 AoI 解耦：未点亮车辆的破坏照常广播
    /// （受控实验 R132 8/8）。契约 additive：消费方忽略未知键。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub destructible_events: Vec<crate::replay::destructibles::AreaDestructibleEvent>,

    /// 实时装填相位（arena subtype 15/16/17，**仅本方全队**；相位码 f2 与计数 f4 原样透传）。
    /// 语义表见 `combat::arena` 的相位常量块：f2=1 剩余弹数更新 / 3 整夹重装 / 4 中途时长变更 /
    /// 5 就绪（f4=1 是就绪标志，非剩余数）/ 6 弹鼓逐发补槽 / 7 夹内推弹（不补弹）/ 8 语义未定（禁猜，
    /// 样本仅 tank 21793 发出、无 f3/f4）；**除 f2=5 外 f4 = 服务器剩余弹数快照**
    /// （消费侧据此重锚，纠正本地外推漂移）。
    /// 契约 additive：同版本只加字段（消费方忽略未知键），不递增 playbacks 契约版本。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reloads: Vec<RawReloadPhase>,
    /// 权威「当前生效完整装填时长」（方法 35/0x23，**仅本方**）：作整夹重装（f2=3）相位的
    /// 时长刻度（相位 f3 只在相位起点给出，配置中途变化只能由本流补上）。
    /// 契约 additive：消费方忽略未知键。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reload_effective: Vec<RawReloadDuration>,
    pub meta: PlaybackMeta,
    /// 实体 id 升序（确定性输出）
    pub vehicles: Vec<VehicleTrack>,
    /// 按开火时刻排序（作者 + 他人全部射击）
    pub shots: Vec<PlaybackShot>,
    /// 按时刻排序的击杀事件
    pub kills: Vec<KillEvent>,
    pub periods: Vec<PeriodPoint>,
    /// AoI 可见窗口（仅收录车辆实体；Type33/5 物化开段、Type4 关段，重入 = 多段）。
    /// 语义来源 = collect_aoi_lifecycle（协议精确边界）；渲染插值防护仍看 coverage。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visibility: Vec<AoiPresence>,
}

/// 位置保留 2 位小数（0.01m）；角度 3 位（0.057°，prop2 coarse 步进 ~0.35° 之下）
/// 侧倾取值：**最近邻**（不插值——与 `combat::anchors::select_anchor_state` 对
/// pitch/roll 的规则一致，避免跨 AoI 断段编造中间姿态）。`samples` 按时钟升序
/// （`Timeline::poses` 的契约）。无采样返回 None（调用方落 0 = 水平）。
fn roll_nearest(samples: &[combat::St10Sample], t: f32) -> Option<f32> {
    if samples.is_empty() {
        return None;
    }
    let i = samples.partition_point(|s| s.clock < t);
    let pick = if i == 0 {
        0
    } else if i >= samples.len() {
        samples.len() - 1
    } else if (t - samples[i - 1].clock) <= (samples[i].clock - t) {
        i - 1
    } else {
        i
    };
    Some(samples[pick].roll)
}

fn r2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}
fn r3(x: f32) -> f32 {
    (x * 1000.0).round() / 1000.0
}
/// 0.1ms 舍入（关键帧时刻：跳变段仅 ~17ms 长，毫秒级舍入会显著改变该段斜率）
fn r4(x: f32) -> f32 {
    (x * 10000.0).round() / 10000.0
}

/// 归一化到 [−π, π]（合成角规范化 + 解卷绕差值短弧化；落盘契约 = 解卷绕连续域，非短弧）
fn wrap_pi(x: f32) -> f32 {
    const PI: f32 = std::f32::consts::PI;
    const TAU: f32 = std::f32::consts::TAU;
    let mut v = x;
    while v > PI {
        v -= TAU;
    }
    while v < -PI {
        v += TAU;
    }
    v
}

/// 相位解卷绕：相对前值走短弧（相邻 0.1s 网格物理角差 ≪ π，连续性由探针 P3 哨兵守护）。
/// 滤波器 yaw 在 waypoint 对切换处、prop2 合成角在 ±π 边界都会出现 ≈2π 的数值跳变
/// （sin/cos 等价、逐帧渲染无害，但落盘后朴素线性插值会反甩 ~360°），解卷绕后序列连续。
fn unwrap_angle(prev: Option<f32>, v: f32) -> f32 {
    match prev {
        Some(p) => p + wrap_pi(v - p),
        None => v,
    }
}

/// 原始采样时刻 → coverage 闭区间对（间隙 > [`COVERAGE_GAP`] 断开）
fn build_coverage(clocks: &[f32]) -> Vec<f32> {
    let mut sorted: Vec<f32> = clocks.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut out = Vec::new();
    let Some(&first) = sorted.first() else {
        return out;
    };
    let mut start = first;
    let mut prev = first;
    for &c in &sorted[1..] {
        if c - prev > COVERAGE_GAP {
            out.push(r2(start));
            out.push(r2(prev));
            start = c;
        }
        prev = c;
    }
    out.push(r2(start));
    out.push(r2(prev));
    out
}

/// coverage 区段内的时刻判定（前端同式）
#[allow(dead_code)]
fn coverage_contains(cov: &[f32], t: f32) -> bool {
    cov.as_chunks::<2>()
        .0
        .iter()
        .any(|p| t >= p[0] && t <= p[1])
}

/// 弹道直线飞行时长（秒）：|to−from| / |launch_velocity|，速度不可信时 0.5s 兜底
fn flight_secs(from: &[f32; 3], to: &[f32; 3], vel: &[f32; 3]) -> f32 {
    let speed = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
    let dist =
        ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2)).sqrt();
    if speed > 1.0 {
        (dist / speed).clamp(0.02, 8.0)
    } else {
        0.5
    }
}

/// 从射击复现数据映射回放弹道记录
fn to_playback_shot(s: &ShotReplayData, _name_to_eid: &HashMap<String, u32>) -> PlaybackShot {
    // 受击方身份 = 原始 eid 直传（method38/method8 服务器权威）——
    // eid 是身份域，名字仅显示域。
    let target_eid = s.target_eid;
    PlaybackShot {
        t_fire: r2(s.fire_time),
        shooter_eid: s.shooter_eid,
        target_eid,
        from: s.ball_a,
        to: s.ball_b,
        flight_secs: r2(flight_secs(&s.ball_a, &s.ball_b, &s.launch_velocity)),
        shell_speed: r2((s.launch_velocity[0] * s.launch_velocity[0]
            + s.launch_velocity[1] * s.launch_velocity[1]
            + s.launch_velocity[2] * s.launch_velocity[2])
            .sqrt()),
        hit: target_eid.is_some(),
        ricochet: s.hit_flags & 0x0008 != 0,
        game_hit_result: s.game_hit_result,
        damage: s.damage,
        is_kill: s.is_kill,
        is_author: s.is_author,
        shell_kind: s.shell_kind.clone(),
        shooter_name: s.shooter_name.clone(),
        target_name: s.target_name.clone(),
    }
}

/// 构建全场回放数据（兼容入口）：单次扫描建内部模型 → 回放切面投影。
/// fail 点不变：回放无任何实体姿态流（非对战文件/损坏）。
pub fn build_playback_data(input: &PlaybackInput) -> anyhow::Result<PlaybackData> {
    let model = crate::replay::model::ReplayModel::scan(&crate::replay::model::ScanInput {
        packets: input.packets,
        roster: &input.players,
        author_account_id: input.author_account_id,
        pitch_limits: input.pitch_limits,
    })?;
    let render = PlaybackRenderInput {
        winner_team: input.winner_team,
        map_id: input.map_id,
        map_name: input.map_name.clone(),
        pitch_limits: input.pitch_limits,
        tank_names: &input.tank_names,
    };
    from_model(&model, &render)
}

/// 从内部模型投影回放切面：位姿滤波/0.1s 网格/角度解卷绕/击杀归属增强都在本层完成，
/// 模型只提供原始采样与事件；身份（昵称/账号/队伍/tank_id/作者标记）一律读模型实体并表。
pub fn from_model(
    model: &crate::replay::model::ReplayModel,
    render: &PlaybackRenderInput,
) -> anyhow::Result<PlaybackData> {
    // 作者档案 = 模型实体并表产物（identity 唯一事实源，本函数不再触碰花名册）
    let author_rec = model.entities.iter().find(|e| e.is_author);
    let author_nickname = author_rec
        .and_then(|e| e.nickname.clone())
        .unwrap_or_default();
    let author_player_eid = model.timeline.author_eid;

    // 实体档案索引：eid → 模型并表记录（昵称/账号/队伍/tank_id 全部取此）
    let ent_by_eid: HashMap<u32, &crate::replay::model::EntityRecord> =
        model.entities.iter().map(|e| (e.eid, e)).collect();

    // 实体索引：模型原始流（type=10 + prop2，车辆筛选 = 双流交集）
    let st10 = &model.timeline.poses;
    let prop2 = &model.timeline.turret;
    let entity_names = &model.timeline.entity_names;

    // 车辆实体 = st10 ∧ prop2 ∧ 采样数达标（BTreeMap 保证确定性顺序）
    let mut candidates: BTreeMap<u32, usize> = BTreeMap::new();
    for (eid, samples) in st10.iter() {
        if samples.len() >= MIN_ST10_SAMPLES && prop2.contains_key(eid) {
            candidates.insert(*eid, samples.len());
        }
    }
    if candidates.is_empty() {
        bail!("无任何车辆姿态流（type=10 + prop2 双流交集为空）");
    }

    // name→eid 反查表（限车辆实体；射击目标归属用）
    let mut name_to_eid: HashMap<String, u32> = HashMap::new();
    for eid in candidates.keys() {
        if let Some(name) = entity_names.get(eid) {
            name_to_eid.entry(name.clone()).or_insert(*eid);
        }
    }

    // 全部弹道（模型一次扫描产物；时间轴末端扩展与列表共用）
    let shots_raw = &model.timeline.shots;

    // 时刻轴：全局 [min覆盖起点, max覆盖终点/阶段末/末弹] 网格
    let mut t_start = f32::MAX;
    let mut t_end = f32::MIN;
    for eid in candidates.keys() {
        let mut clocks: Vec<f32> = st10[eid].iter().map(|s| s.clock).collect();
        clocks.sort_by(|a, b| a.partial_cmp(b).unwrap());
        t_start = t_start.min(clocks[0]);
        // 时间线终点 = build 的末采样 +0.5s 外推域（filter.rs time_range）
        if let Some(&last) = clocks.last() {
            t_end = t_end.max(last + 0.5);
        }
    }
    let periods = &model.timeline.periods;
    for p in periods {
        t_end = t_end.max(p.clock);
    }
    for s in shots_raw.iter() {
        t_end = t_end.max(s.fire_time + flight_secs(&s.ball_a, &s.ball_b, &s.launch_velocity));
    }
    if !t_start.is_finite() || t_end <= t_start {
        bail!("姿态时间线为空（type=10 采样不足）");
    }
    let samples_n = ((t_end - t_start) / GRID_DT).ceil() as usize + 1;
    let meta = PlaybackMeta {
        map_id: render.map_id,
        map_name: render.map_name.clone(),
        winner_team: render.winner_team,
        friendly_team: author_rec.and_then(|e| e.team).unwrap_or(0),
        author_eid: author_player_eid,
        t_start: r2(t_start),
        samples: samples_n,
        duration: r2(t_start + (samples_n.saturating_sub(1)) as f32 * GRID_DT),
    };

    // 血量链与死亡终态（模型一次扫描产出，循环内查表）
    let initial_hp = &model.timeline.initial_hp;

    let mut vehicles_out: Vec<VehicleTrack> = Vec::with_capacity(candidates.len());
    for eid in candidates.keys() {
        let tl = FilteredTimeline::build(&st10[eid]);
        let Some(tl) = tl else { continue };
        let clocks: Vec<f32> = st10[eid].iter().map(|s| s.clock).collect();

        let nickname = entity_names.get(eid).cloned().unwrap_or_default();
        // 身份 = 模型实体并表（scan 期一次 JOIN 的产物；此处只读取不重联）
        let rec = ent_by_eid.get(eid).copied();
        let is_author = rec.map(|r| r.is_author).unwrap_or(false);
        // 俯仰极限锚定：本车昵称优先；作者无锚定时沿用其表项（与作者路径 prop9 兜底同级）
        let limits = render.pitch_limits.get(nickname.as_str()).or_else(|| {
            render
                .pitch_limits
                .get(author_nickname.as_str())
                .filter(|_| is_author)
        });

        // prop2 流在收录条件（st10 ∧ prop2 双流交集）下必然存在——缺流即内部不变量破坏
        let Some(prop2_series) = prop2.get(eid) else {
            bail!("车辆 {eid} 缺 prop2 流（收录条件 = st10 ∧ prop2 双流交集，不应发生）");
        };

        // 逐网格采样：渲染位姿 + prop2 炮塔/俯仰（同刻合成绝对角）；角度序列解卷绕落盘。
        // 列式数组与网格严格同长——任何跳过都会静默错位（pos[3i+k] 不再对齐 t_i），fail-fast。
        let mut pos = Vec::with_capacity(samples_n * 3);
        let mut hull_yaw = Vec::with_capacity(samples_n);
        let mut hull_pitch = Vec::with_capacity(samples_n);
        // 侧倾列取自原始 type=10 序列（滤波层不输出侧倾，见 VehicleTrack.hull_roll 注释）
        let st_series: &[combat::St10Sample] = &st10[eid];
        let mut hull_roll = Vec::with_capacity(samples_n);
        let mut turret_yaw = Vec::with_capacity(samples_n);
        let mut gun_pitch = Vec::with_capacity(samples_n);
        let mut prev_hull_yaw = None;
        let mut prev_turret_yaw = None;
        for i in 0..samples_n {
            let t = meta.t_start + i as f32 * GRID_DT;
            let Some(pose) = tl.pose_at(t as f64, false) else {
                bail!("车辆 {eid} 位姿网格 t={t} 无帧（FilteredTimeline 恒可求值，不应发生）");
            };
            let yaw = unwrap_angle(prev_hull_yaw, pose.ang[0]);
            prev_hull_yaw = Some(yaw);
            pos.push(r2(pose.pos[0]));
            pos.push(r2(pose.pos[1]));
            pos.push(r2(pose.pos[2]));
            hull_yaw.push(r3(yaw));
            hull_pitch.push(r3(pose.ang[1]));
            hull_roll.push(r3(roll_nearest(st_series, t).unwrap_or(0.0)));
            // 非空 prop2 序列恒可求值（AoI 前回退初值包 / 末帧保持）
            let Some((rel, frac)) = combat::prop2_at(Some(prop2_series), t) else {
                bail!("车辆 {eid} prop2 网格 t={t} 无采样（非空序列恒可求值，不应发生）");
            };
            let turret_abs = unwrap_angle(prev_turret_yaw, wrap_pi(rel + pose.ang[0]));
            prev_turret_yaw = Some(turret_abs);
            turret_yaw.push(r3(turret_abs));
            gun_pitch.push(match limits {
                Some(lim) => r3(combat::decode_prop2_gun_pitch(frac, lim, rel)),
                // 无俯仰极限锚定（匿名车/空锚定表）：车体 pitch 兜底（type10 pitch 正向
                // 与"正=仰角"相反，仅近似——锚定表由 battle_results 全量构建，正常场次不触发）
                None => r3(pose.ang[1]),
            });
        }

        // HP 链：模型已按同值去重语义构建（含满血锚点，此处仅做落盘舍入）；
        // 击杀者/死因 = 死亡终态记录（hp==0 事件 + prop1 时刻合并）
        let hp: Vec<(f32, u16)> = model
            .timeline
            .hp_series
            .get(eid)
            .map(|s| s.iter().map(|(t, h)| (r2(*t), *h)).collect())
            .unwrap_or_default();
        let death = model.timeline.deaths.get(eid);
        let killer_eid = death.map(|d| d.killer_eid).unwrap_or(0);

        // 发射弹种全局 id（配置推断证据；去重上限 16）
        let mut shell_ids: Vec<u32> = Vec::new();
        for s in shots_raw.iter() {
            if s.shooter_eid != *eid || s.shell_id == 0 || shell_ids.len() >= 16 {
                continue;
            }
            if !shell_ids.contains(&s.shell_id) {
                shell_ids.push(s.shell_id);
            }
        }

        vehicles_out.push(VehicleTrack {
            eid: *eid,
            account_id: rec.and_then(|r| r.account_id).unwrap_or(0),
            nickname,
            tank_id: rec.and_then(|r| r.tank_id).unwrap_or(0),
            tank_name: rec
                .and_then(|r| r.tank_id)
                .and_then(|tid| render.tank_names.get(&tid).cloned())
                .unwrap_or_default(),
            team: rec.and_then(|r| r.team).unwrap_or(0),
            is_author,
            max_hp: initial_hp.get(eid).map(|(_, h)| *h).unwrap_or(0),
            pos,
            hull_yaw,
            hull_pitch,
            hull_roll,
            turret_yaw,
            gun_pitch,
            hp,
            death_t: death.map(|d| r2(d.t)),
            killer_eid,
            shell_ids,
            loadout_items: rec.map(|r| r.loadout_items.clone()).unwrap_or_default(),
            equipment: rec.and_then(|r| r.equipment),
            turret_index: None,
            gun_index: None,
            coverage: build_coverage(&clocks),
            pose_kf: Some(pose_keyframes(&tl)),
        });
    }

    // 击杀事件：模型统一产出（击杀播报归属增强，|t − death_t| ≤ 5s 门控已在模型内），
    // 回放切面按候选车集过滤——与原逐车内联组装逐位等价
    let kills: Vec<KillEvent> = model
        .kill_events()
        .into_iter()
        .filter(|k| candidates.contains_key(&k.victim_eid))
        .collect();

    let mut shots: Vec<PlaybackShot> = shots_raw
        .iter()
        .map(|s| to_playback_shot(s, &name_to_eid))
        .collect();
    shots.sort_by(|a, b| a.t_fire.partial_cmp(&b.t_fire).unwrap());

    let periods_out: Vec<PeriodPoint> = periods
        .iter()
        .map(|p| PeriodPoint {
            clock: r2(p.clock),
            period: p.period,
            remaining_s: p.remaining_s,
            duration_s: p.duration_s,
        })
        .collect();

    // AoI 可见窗口（仅收录车辆实体；Type33/5 物化开段、Type4 关段，重入 = 多段）
    let visibility: Vec<AoiPresence> = model
        .timeline
        .presence
        .iter()
        .filter(|p| candidates.contains_key(&p.eid))
        .cloned()
        .collect();

    Ok(PlaybackData {
        version: 2, // contract v2：+supremacy_bases/supremacy_points（消费端版本门禁）
        meta,
        vehicles: vehicles_out,
        shots,
        kills,
        periods: periods_out,
        visibility,
        supremacy_bases: model
            .timeline
            .supremacy_bases
            .iter()
            .map(|t| SupremacyBaseStateTransition {
                clock: r2(t.clock),
                ..t.clone()
            })
            .collect(),
        supremacy_points: model
            .timeline
            .supremacy_points
            .iter()
            .map(|p| SupremacyPointsSample {
                clock: r2(p.clock),
                ..p.clone()
            })
            .collect(),
        assault_objective_present: model.timeline.assault_objective_present,
        assault_bases: model
            .timeline
            .assault_bases
            .iter()
            .map(|t| AssaultBaseStateTransition {
                clock: r2(t.clock),
                ..t.clone()
            })
            .collect(),
        consumables: model
            .timeline
            .consumables
            .iter()
            .map(|c| ConsumableTransition {
                clock: r2(c.clock),
                ..*c
            })
            .collect(),
        reload_effective: model
            .timeline
            .reload_effective
            .iter()
            .map(|d| RawReloadDuration {
                clock: r2(d.clock),
                ..d.clone()
            })
            .collect(),
        reloads: model
            .timeline
            .reloads
            .iter()
            .map(|r| RawReloadPhase {
                clock: r2(r.clock),
                ..r.clone()
            })
            .collect(),
        module_crew_states: model
            .timeline
            .module_crew_states
            .iter()
            .map(|m| ModuleCrewStateEvent {
                clock: r2(m.clock),
                ..*m
            })
            .collect(),
        destructible_areas: model.timeline.destructible_areas.clone(),
        destructible_events: model
            .timeline
            .destructible_events
            .iter()
            .map(|e| crate::replay::destructibles::AreaDestructibleEvent {
                clock: r2(e.clock),
                ..e.clone()
            })
            .collect(),
    })
}

/// 合并作者严格路径 + 他人宽松路径（共享扫描形态：预分析产物与渲染缓存两路复用，
/// 见 [`combat::ShotScanShared`]）。作者严格路径是 fail-fast 设计（边界数据缺失即
/// bail）——全场回放不应因此整场不可用：失败时降级为宽松路径提取**全部**发射
/// （含作者，按 shooter_eid 补回 is_author 标记）。
pub(crate) fn collect_all_shots(
    shared: &combat::ShotScanShared,
    author_player_eid: u32,
    pitch_limits: &GunPitchLimits,
) -> anyhow::Result<Vec<ShotReplayData>> {
    // 滤波时间线缓存跨两路共享（按实体确定）
    let mut render_cache: HashMap<u32, FilteredTimeline> = HashMap::new();
    match combat::extract_shot_replays_from_shared(
        shared,
        author_player_eid,
        pitch_limits,
        &mut render_cache,
    ) {
        Ok(mut all) => {
            let others = combat::extract_other_shot_replays_from_shared(
                shared,
                author_player_eid,
                pitch_limits,
                &mut render_cache,
            );
            all.extend(others.shots);
            all.sort_by(|a, b| a.fire_time.partial_cmp(&b.fire_time).unwrap());
            Ok(all)
        }
        Err(strict_err) => {
            eprintln!("[playback] 作者严格路径提取失败（{strict_err:#}），降级宽松全路径");
            let mut all = combat::extract_other_shot_replays_from_shared(
                shared,
                0,
                pitch_limits,
                &mut render_cache,
            )
            .shots;
            for s in &mut all {
                if author_player_eid != 0 && s.shooter_eid == author_player_eid {
                    s.is_author = true;
                }
            }
            all.sort_by(|a, b| a.fire_time.partial_cmp(&b.fire_time).unwrap());
            Ok(all)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cov(pairs: &[&[f32]]) -> Vec<f32> {
        pairs.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// coverage 合并：小间隙不断开、>2s 空洞断开、末区间闭合到最后采样
    #[test]
    fn coverage_merges_small_gaps_and_splits_large() {
        // 10Hz 连续 3s → 单段 [10.0, 12.9]
        let c1: Vec<f32> = (0..30).map(|i| 10.0 + i as f32 * 0.1).collect();
        assert_eq!(build_coverage(&c1), cov(&[&[10.0, 12.9]]));
        // 中途 2.6s 空洞 → 两段
        let mut c3 = c1.clone();
        c3.extend([15.5, 15.6, 15.7]);
        let got = build_coverage(&c3);
        assert_eq!(got, cov(&[&[10.0, 12.9], &[15.5, 15.7]]));
        assert!(coverage_contains(&got, 10.0));
        assert!(coverage_contains(&got, 15.6));
        assert!(!coverage_contains(&got, 14.0));
    }

    /// 数值精度：可精确表示的值舍入稳定
    #[test]
    fn rounding() {
        assert_eq!(r2(123.5), 123.5);
        assert_eq!(r2(0.125), 0.13);
        assert_eq!(r3(1.0), 1.0);
        assert_eq!(r3(2.125), 2.125);
    }

    /// 飞行时长：300m / 150mps = 2.0s；速度缺失 0.5s 兜底
    #[test]
    fn flight_time() {
        let a = [0.0, 10.0, 0.0];
        let b = [300.0, 10.0, 0.0];
        assert_eq!(flight_secs(&a, &b, &[150.0, 0.0, 0.0]), 2.0);
        assert_eq!(flight_secs(&a, &b, &[0.0, 0.0, 0.0]), 0.5);
    }

    // ---------- 位姿关键帧折线 ----------

    /// 消费端语义求值：端点保持 + 段内线性（与前端 trackInterp 的 sampleKeyframes 同式）
    fn sample_kf(kf: &PoseKeyframes, t: f32) -> ([f32; 3], f32, f32) {
        let last = kf.t.len() - 1;
        if t <= kf.t[0] {
            return (kf.pos[0..3].try_into().unwrap(), kf.yaw[0], kf.pitch[0]);
        }
        if t >= kf.t[last] {
            let o = last * 3;
            return (
                kf.pos[o..o + 3].try_into().unwrap(),
                kf.yaw[last],
                kf.pitch[last],
            );
        }
        let i = kf.t.partition_point(|&x| x <= t) - 1;
        let f = (t - kf.t[i]) / (kf.t[i + 1] - kf.t[i]);
        let mut pos = [0.0f32; 3];
        for c in 0..3 {
            pos[c] = kf.pos[i * 3 + c] + (kf.pos[(i + 1) * 3 + c] - kf.pos[i * 3 + c]) * f;
        }
        let dyaw = wrap_pi(kf.yaw[i + 1] - kf.yaw[i]);
        (
            pos,
            kf.yaw[i] + dyaw * f,
            kf.pitch[i] + (kf.pitch[i + 1] - kf.pitch[i]) * f,
        )
    }

    /// 合成输入：等间隔位置更新，每步 step_m 米（模拟客户端 10~12Hz 位置广播）
    fn synthetic_samples(step_m: f32, interval: f32, n: usize) -> Vec<combat::St10Sample> {
        (0..n)
            .map(|i| {
                let d = i as f32 * step_m;
                combat::St10Sample {
                    clock: 10.0 + i as f32 * interval,
                    pos: [d, 0.0, d * 0.5],
                    yaw: 0.3,
                    pitch: 0.0,
                    roll: 0.0,
                    pos_error: [0.0; 3],
                }
            })
            .collect()
    }

    /// 走廊判据几何：共线 → 通过；中间帧偏移超容差 → 拒绝；航向跨 ±π 用最短弧
    #[test]
    fn kf_within_geometry() {
        let mk = |t: f64, x: f32, yaw: f32| KfPos {
            t,
            pos: [x, 0.0, 0.0],
            yaw,
            pitch: 0.0,
        };
        let line = [mk(0.0, 0.0, 0.0), mk(1.0, 0.5, 0.0), mk(2.0, 1.0, 0.0)];
        assert!(kf_within(&line, 0, 2));
        // 边界两侧用容差本身构造（容差调整后仍成立）：0.5×容差 → 通过；2×容差 → 拒绝
        let soft = [
            mk(0.0, 0.0, 0.0),
            mk(1.0, 0.5 + KF_TOL_POS * 0.5, 0.0),
            mk(2.0, 1.0, 0.0),
        ];
        assert!(kf_within(&soft, 0, 2));
        let bent = [
            mk(0.0, 0.0, 0.0),
            mk(1.0, 0.5 + KF_TOL_POS * 2.0, 0.0),
            mk(2.0, 1.0, 0.0),
        ];
        assert!(!kf_within(&bent, 0, 2));
        // 航向最短弧跨 ±π（+3.10 → −3.10 实为 +0.08 的短弧）
        let wrap = [mk(0.0, 0.0, 3.10), mk(1.0, 0.5, 3.14), mk(2.0, 1.0, -3.10)];
        assert!(kf_within(&wrap, 0, 2), "跨 ±π 的短弧不应被判为大偏差");
        let yawbent = [
            mk(0.0, 0.0, 0.0),
            mk(1.0, 0.5, KF_TOL_ANG * 2.0),
            mk(2.0, 1.0, 0.0),
        ];
        assert!(!kf_within(&yawbent, 0, 2));
    }

    /// 关键帧折线保真：任意时刻与 60Hz 渲染帧之差 ≤ 容差（对前端的契约）
    #[test]
    fn keyframes_reproduce_frames_within_tolerance() {
        let tl = FilteredTimeline::build(&synthetic_samples(1.4, 0.083, 200)).unwrap();
        let kf = pose_keyframes(&tl);
        assert!(kf.t.len() >= 2, "关键帧数过少：{}", kf.t.len());
        assert_eq!(kf.pos.len(), kf.t.len() * 3, "列式数组必须等长");
        assert_eq!(kf.yaw.len(), kf.t.len());
        assert_eq!(kf.pitch.len(), kf.t.len());
        let (start, end) = tl.time_range();
        let mut max_err = 0.0f32;
        for k in 0..tl.frame_count() {
            let t = start + (k as f64 + 0.5) * filter::FRAME_DT;
            let p = tl.pose_at(t, false).unwrap();
            let (kp, ky, kpitch) = sample_kf(&kf, p.time as f32);
            max_err = max_err.max(
                ((kp[0] - p.pos[0]).powi(2)
                    + (kp[1] - p.pos[1]).powi(2)
                    + (kp[2] - p.pos[2]).powi(2))
                .sqrt(),
            );
            assert!(
                wrap_pi(ky - p.ang[0]).abs() <= KF_TOL_ANG + 0.001,
                "yaw 超容差"
            );
            assert!(
                (kpitch - p.ang[1]).abs() <= KF_TOL_ANG + 0.001,
                "pitch 超容差"
            );
        }
        // 上界 = 走廊容差 + 落盘舍入传播（两个端点各 ≤0.5cm 舍入 → 段内 ≤1cm）
        assert!(
            max_err <= KF_TOL_POS + 0.006,
            "位置最大偏差 {max_err}m 超容差"
        );
        assert!(end > start);
        // 航向为解卷绕连续域：相邻关键帧 |Δ| < π（跨 ±π 不出现反甩）
        for i in 0..kf.t.len() - 1 {
            assert!(
                (kf.yaw[i + 1] - kf.yaw[i]).abs() < std::f32::consts::PI,
                "关键帧航向不连续：{i} → {}",
                i + 1
            );
        }
    }

    /// 跳变保留（对照 10Hz 混叠）：客户端在两次位置更新之间保持原地、收包后跳变。
    /// 关键帧折线复现阶梯（存在陡斜率段与保持段），而按 10Hz 网格重采样 + 线性插值会把
    /// 阶梯抹成滑行——同一 60Hz 帧序列上两者偏差相差一个量级。
    #[test]
    fn keyframes_keep_holds_and_jumps_unlike_grid_aliasing() {
        // 12Hz 位置更新（与 0.1s 网格不同相 → 网格重采样必然混叠），每步 1.4m
        let tl = FilteredTimeline::build(&synthetic_samples(1.4, 1.0 / 12.0, 300)).unwrap();
        let kf = pose_keyframes(&tl);
        let (start, end) = tl.time_range();
        let frames: Vec<(f64, [f32; 3])> = (0..tl.frame_count())
            .map(|k| {
                let t = start + (k as f64 + 0.5) * filter::FRAME_DT;
                let p = tl.pose_at(t, false).unwrap();
                (p.time, p.pos)
            })
            .collect();

        let dist = |a: [f32; 3], b: [f32; 3]| {
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        let mut err_kf = 0.0f32;
        for (t, pos) in &frames {
            let (kp, _, _) = sample_kf(&kf, *t as f32);
            err_kf = err_kf.max(dist(kp, *pos));
        }
        // 10Hz 网格重采样 + 线性插值（改动前的渲染路径）
        let grid: Vec<(f32, [f32; 3])> = (0..)
            .map(|i| start + i as f64 * GRID_DT as f64)
            .take_while(|t| *t <= end)
            .map(|t| (t as f32, tl.pose_at(t, false).unwrap().pos))
            .collect();
        let mut err_grid = 0.0f32;
        for (t, pos) in &frames {
            let t = *t as f32;
            let i = grid
                .partition_point(|(gt, _)| *gt <= t)
                .saturating_sub(1)
                .min(grid.len() - 2);
            let f = (t - grid[i].0) / (grid[i + 1].0 - grid[i].0);
            let g = [0, 1, 2].map(|c| grid[i].1[c] + (grid[i + 1].1[c] - grid[i].1[c]) * f);
            err_grid = err_grid.max(dist(g, *pos));
        }
        assert!(err_kf <= KF_TOL_POS + 0.006, "折线偏差 {err_kf}m 超容差");
        assert!(
            err_grid > 0.15,
            "网格混叠偏差应显著（实际 {err_grid}m）——若此断言失败，说明合成节奏与网格同相，\
             调整合成参数而不是放宽断言"
        );
        // 阶梯结构确实被保留：存在 ≥1m 的陡段（跳变）与 ≥0.15s 的零位移保持段
        let mut max_step = 0.0f32;
        let mut max_hold = 0.0f32;
        for i in 0..kf.t.len() - 1 {
            let d = dist(
                [kf.pos[i * 3], kf.pos[i * 3 + 1], kf.pos[i * 3 + 2]],
                [
                    kf.pos[(i + 1) * 3],
                    kf.pos[(i + 1) * 3 + 1],
                    kf.pos[(i + 1) * 3 + 2],
                ],
            );
            let dt = kf.t[i + 1] - kf.t[i];
            max_step = max_step.max(d);
            if d < 1e-4 {
                max_hold = max_hold.max(dt);
            }
        }
        assert!(max_step >= 1.0, "跳变段未被保留（最大段长 {max_step}m）");
        assert!(max_hold >= 0.15, "保持段未被保留（最长保持 {max_hold}s）");
    }

    // ---------- 端到端探针（真实回放分布证据） ----------
    //
    // 运行：WOTB_PLAYBACK_PROBE=data/replay_samples cargo test playback_probe -- --ignored --nocapture
    //
    // 判定项：
    //   P1 车辆收录：候选实体数 ∈ [10, 16]（14 车 ± 解析边界；KineticObject 等被双流过滤）；
    //   P2 位姿一致性：开火时刻的网格采样位 ≈ ShotReplayData.shooter_render（同为滤波器输出，
    //      仅 0.1s 网格舍入差，≤0.6m）；
    //   P3 角度连续性/值域：解卷绕后 hull_yaw/turret_yaw 相邻网格 |Δ| ≤ π/2（0.1s 物理极限，
    //      wrap 跳变 ≈2π 在此拦截）、gun_pitch ∈ [-1.575, 1.575] rad；
    //   P4 血量链：max_hp>0、HP 单调不增、死亡车末值 0。
    #[test]
    #[ignore = "端到端探针：WOTB_PLAYBACK_PROBE=<path|dir> cargo test playback_probe -- --ignored --nocapture"]
    fn playback_probe() {
        let root =
            std::env::var("WOTB_PLAYBACK_PROBE").unwrap_or_else(|_| "data/replay_samples".into());
        let path = std::path::Path::new(&root);
        let files: Vec<std::path::PathBuf> = if path.is_file() {
            vec![path.to_path_buf()]
        } else {
            std::fs::read_dir(path)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "wotbreplay").unwrap_or(false))
                .collect()
        };
        assert!(!files.is_empty(), "未找到回放文件：{root}");

        for file in &files {
            let name = file.file_name().unwrap().to_string_lossy().to_string();
            let f = std::fs::File::open(file).unwrap();
            let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
            let data = replay.read_data().unwrap();
            let packets: Vec<(u32, f32, &[u8])> = data
                .packets
                .iter()
                .map(|pkt| {
                    let t = match &pkt.payload {
                        wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate {
                            ..
                        } => 0,
                        wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                        wotbreplay_parser::models::data::payload::Payload::Unknown {
                            packet_type,
                        } => *packet_type,
                    };
                    (t, pkt.clock_secs, &pkt.raw_payload[..])
                })
                .collect();
            let br = replay.read_battle_results().ok();
            let players: Vec<PlaybackPlayer> = br
                .as_ref()
                .map(|br| {
                    br.player_results
                        .iter()
                        .map(|pr| {
                            let joined = br
                                .players
                                .iter()
                                .find(|p| p.account_id == pr.info.account_id);
                            PlaybackPlayer {
                                account_id: pr.info.account_id,
                                nickname: joined
                                    .map(|p| p.info.nickname.clone())
                                    .unwrap_or_default(),
                                team: joined
                                    .map(|p| if p.info.team == 1 { 1u8 } else { 2u8 })
                                    .unwrap_or(0),
                                tank_id: pr.info.tank_id,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let input = PlaybackInput {
                packets: &packets,
                players,
                author_account_id: br.as_ref().map(|b| b.author.account_id).unwrap_or(0),
                winner_team: 0,
                map_id: 0,
                map_name: String::new(),
                pitch_limits: &GunPitchLimits::new(),
                tank_names: HashMap::new(),
            };
            let pb = build_playback_data(&input).unwrap_or_else(|e| panic!("{name}: 构建失败 {e}"));

            // P1 车辆收录（仅完整战斗；退化样例（训练房/片段，时长短或车辆少）跳过断言）
            let nv = pb.vehicles.len();
            let named = pb.vehicles.iter().filter(|v| v.team != 0).count();
            eprintln!("--- {name}");
            eprintln!(
                "    P1 车辆 {nv}（具名 {named}），射击 {} 发，击杀 {}，时长 {:.1}s",
                pb.shots.len(),
                pb.kills.len(),
                pb.meta.duration
            );
            if nv < 10 || pb.meta.duration < 60.0 {
                eprintln!(
                    "    ~~ 退化样例（车辆 {nv} / 时长 {:.1}s），跳过断言",
                    pb.meta.duration
                );
                continue;
            }
            // 7v7/10v10 等模式车数不同（GB109=10v10 → 20 车）
            assert!((8..=28).contains(&nv), "P1 车辆数异常 {nv}");
            assert_eq!(
                pb.vehicles.iter().filter(|v| v.is_author).count(),
                1,
                "作者车应恰 1 辆"
            );

            // P2 位姿一致性（开火时刻网格采样 vs 单发复现渲染锚点）
            // PlaybackShot 已精简掉锚点——探针在同模块直接调 collect_all_shots 取原始
            // ShotReplayData（含 shooter_render + fire_time）
            let author_eid = pb.meta.author_eid;
            let shared = combat::build_shot_scan_shared(&packets, author_eid);
            let shots_raw =
                collect_all_shots(&shared, author_eid, &GunPitchLimits::new()).unwrap_or_default();
            let mut checked = 0usize;
            let mut worst = 0.0f32;
            let by_eid: HashMap<u32, &VehicleTrack> =
                pb.vehicles.iter().map(|v| (v.eid, v)).collect();
            for s in &shots_raw {
                let Some(anchor) = &s.shooter_render else {
                    continue;
                };
                let Some(sv) = by_eid.get(&s.shooter_eid) else {
                    continue;
                };
                // 锚点 = 命中/开火事件帧（ceil）；网格是 0.1s 重采样，事件帧必落在相邻两格
                // 之间——判据：锚点距较近端 ≤ 0.3m + 0.12×局部车速（重采样容差，非固定阈值）
                let fi = (s.fire_time - pb.meta.t_start) / GRID_DT;
                if fi < 0.0 {
                    continue;
                }
                let i0 = (fi.floor() as usize).min(pb.meta.samples - 1);
                let i1 = (i0 + 1).min(pb.meta.samples - 1);
                let gp = |i: usize, k: usize| sv.pos[i * 3 + k];
                let d = |i: usize| {
                    ((gp(i, 0) - anchor.pos[0]).powi(2)
                        + (gp(i, 1) - anchor.pos[1]).powi(2)
                        + (gp(i, 2) - anchor.pos[2]).powi(2))
                    .sqrt()
                };
                let dmin = d(i0).min(d(i1));
                let v = ((gp(i1, 0) - gp(i0, 0)).powi(2)
                    + (gp(i1, 1) - gp(i0, 1)).powi(2)
                    + (gp(i1, 2) - gp(i0, 2)).powi(2))
                .sqrt()
                    / GRID_DT;
                worst = worst.max(dmin);
                checked += 1;
                assert!(
                    dmin <= 0.3 + 0.12 * v,
                    "P2 开火位姿偏差 {dmin:.3}m（局部车速 {v:.1}m/s）@ {} t={}",
                    name,
                    s.fire_time
                );
            }
            assert!(checked > 0, "P2 无可校验射击（无 shooter_render 锚点）");
            eprintln!("    P2 位姿一致性 {checked} 发，最大偏差 {worst:.3}m");

            // P3 角度连续性/值域 + P4 血量链
            for v in &pb.vehicles {
                let n = pb.meta.samples;
                assert_eq!(v.hull_yaw.len(), n, "{} 网格长度", v.eid);
                for i in 0..n {
                    // 解卷绕契约哨兵：解卷绕后相邻网格差恒 = 短弧 ∈ [-π,π]（r3 舍入余量 0.01），
                    // rawΔ≈±2π 的 wrap 跳变在此拦截（防落盘逻辑改动漏掉解卷绕）。
                    // |Δ|>π/2 的事件性跳变（AoI 断流重续朝向真实改变/死亡重置）为合法数据，
                    // 仅信息打印——它们或落在 coverage 空洞边界（前端不渲染），或本就是信息缺口。
                    if i + 1 < n {
                        let dh = v.hull_yaw[i + 1] - v.hull_yaw[i];
                        assert!(
                            dh.abs() <= std::f32::consts::PI + 0.01,
                            "P3 hull_yaw 存在未解卷绕跳变 {} @{} Δ={dh:.3}",
                            v.eid,
                            i
                        );
                        let d_tw = v.turret_yaw[i + 1] - v.turret_yaw[i];
                        assert!(
                            d_tw.abs() <= std::f32::consts::PI + 0.01,
                            "P3 turret_yaw 存在未解卷绕跳变 {} @{} Δ={d_tw:.3}",
                            v.eid,
                            i
                        );
                        let t = pb.meta.t_start + i as f32 * GRID_DT;
                        if dh.abs() > std::f32::consts::FRAC_PI_2
                            || d_tw.abs() > std::f32::consts::FRAC_PI_2
                        {
                            eprintln!("    P3 大角变 {} t={t:.1} Δhull={dh:+.3} Δturret={d_tw:+.3}（death={:?}）",
                                v.eid, v.death_t);
                        }
                    }
                    assert!(
                        v.gun_pitch[i] >= -1.575 && v.gun_pitch[i] <= 1.575,
                        // 值域仅为量纲哨兵（度→弧度错开会到 ±57）：解码值域 = 车型俯仰极限
                        // （SPG 仰角 ~75°=1.31rad），兜底值 = 车体 pitch（跌落/翻滚可近 ±π/2）
                        "P3 gun_pitch 越域 {} {}",
                        v.eid,
                        v.gun_pitch[i]
                    );
                }
                if v.team != 0 {
                    assert!(v.max_hp > 0, "P4 具名车 max_hp=0 {}", v.nickname);
                }
                let mut hp_prev = v.max_hp;
                for (_, h) in &v.hp {
                    assert!(
                        *h <= hp_prev,
                        "P4 HP 回升 {} {}: {hp_prev}→{h}",
                        v.eid,
                        v.nickname
                    );
                    hp_prev = *h;
                }
                if v.death_t.is_some() && !v.hp.is_empty() {
                    assert_eq!(
                        v.hp.last().unwrap().1,
                        0,
                        "P4 死亡车末 HP 应为 0 {}",
                        v.nickname
                    );
                }
            }
        }
    }
    /// 侧倾列（`roll_nearest`）：辅助构造按时钟升序的 type=10 采样
    fn roll_samples(clocks_rolls: &[(f32, f32)]) -> Vec<combat::St10Sample> {
        clocks_rolls
            .iter()
            .map(|(c, r)| combat::St10Sample {
                clock: *c,
                pos: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                roll: *r,
                pos_error: [0.0; 3],
            })
            .collect()
    }

    /// 侧倾取值 = 最近邻：段内不插值、边界保持最近已知（与 anchors 对 pitch/roll 同规则）
    #[test]
    fn roll_nearest_holds_and_never_interpolates() {
        let s = roll_samples(&[(10.0, 0.1), (10.5, 0.2), (20.0, -0.3)]);
        assert_eq!(roll_nearest(&[], 5.0), None, "无采样 → None（调用方落 0）");
        assert_eq!(roll_nearest(&s, 5.0), Some(0.1), "早于首样本 → 首样本");
        assert_eq!(roll_nearest(&s, 10.0), Some(0.1), "命中样本");
        assert_eq!(roll_nearest(&s, 10.24), Some(0.1), "更近前样本");
        assert_eq!(roll_nearest(&s, 10.26), Some(0.2), "更近后样本");
        assert_eq!(roll_nearest(&s, 10.25), Some(0.1), "等距取前（确定性）");
        assert_eq!(
            roll_nearest(&s, 15.0),
            Some(0.2),
            "跨断段只保持最近已知，不插值"
        );
        assert_eq!(roll_nearest(&s, 99.0), Some(-0.3), "晚于末样本 → 末样本");
    }
}
