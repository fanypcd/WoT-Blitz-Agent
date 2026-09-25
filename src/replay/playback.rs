//! 全场实时回放数据层：把现有解析成果（渲染层滤波位姿 + prop2 炮塔角 + 射击/血量链）
//! 组装成前端可直接按 0.1s 网格推进的全场时间线。
//!
//! 与单发复现（viewer Shot Replay 滑块）同语义、不同消费方式：
//! - 位姿 = [`FilteredTimeline`]（filter.rs，客户端 AvatarFilter 移植）60Hz 输出的 0.1s 降采样；
//! - 炮塔角 = [`combat::prop2_at`]（客户端 0x1440C70 时间线语义：0.1s 前瞻/短弧插值/clamp 不外推）
//!   的相对角 + **同网格刻**车体 yaw 合成绝对角（与 combat.rs ⑩/⑪ 步 `rel + hullYaw` 同式）；
//! - 炮管俯仰 = prop2 frac 按车型极限解码（[`combat::decode_prop2_gun_pitch`]）；
//! - 射击 = 作者严格路径 + 他人宽松路径合并（与 web `replay_shots_handler` 同构）；
//! - 血量 = type=5 满血锚点 + method1 事件链；死亡 = type=7 sub=1（击杀者取 hp==0 事件 source）。
//!
//! 车辆筛选 = **st10 ∧ prop2 双流**（KineticObject/DetachedTurret 也有 type=10 移动流但无
//! 炮塔角广播）；昵称匹配失败的车辆（type=5 昵称为 ascii_graphic 过滤的非 ascii 昵称）
//! 保留为 team=0/tank_id=0 的"未知"车，不丢战局画面。
//!
//! 序列化约定：位姿为列式 flat 数组（`pos` = [x,y,z]×N，其余各 N 项），时刻 `t_i = t_start + i*0.1`；
//! 前端线性插值即可（滤波器输出本身平滑）。`coverage` = 有效数据区段（原始采样间隙 >2s 视为
//! AoI 空洞——滤波器在无输入期会原地站住，前端按此隐藏车辆避免"幽灵车停在过期位置"）。

use std::collections::{BTreeMap, HashMap};

use anyhow::bail;
use serde::Serialize;

use super::combat::{self, CombatTimeline, GunPitchLimits, ShotReplayData};
use super::filter::FilteredTimeline;

/// 位姿网格步长（秒）——与现有 render_timeline / prop2 密集采样同惯例
pub const GRID_DT: f32 = 0.1;
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
    /// type=5 昵称（非 ascii 或缺失为空串 → 前端显示 Unknown）
    pub nickname: String,
    pub tank_id: u32,
    pub tank_name: String,
    /// 1 / 2（0 = 未知，前端灰色渲染）
    pub team: u8,
    pub is_author: bool,
    pub max_hp: u16,
    /// 车体位置 flat [x,y,z] × N（回放世界系，米）
    pub pos: Vec<f32>,
    /// 车体偏航（弧度）× N
    pub hull_yaw: Vec<f32>,
    /// 车体俯仰（弧度）× N
    pub hull_pitch: Vec<f32>,
    /// 炮塔绝对朝向（弧度）= prop2 相对角 + 同刻车体 yaw × N
    pub turret_yaw: Vec<f32>,
    /// 炮管俯仰（弧度，正=仰角；prop2/极限锚定缺失时 = 车体 pitch 兜底）× N
    pub gun_pitch: Vec<f32>,
    /// HP 变化点 [(t, hp)]（含满血锚点）
    pub hp: Vec<(f32, u16)>,
    pub death_t: Option<f32>,
    /// 击杀者实体（method1 hp==0 事件 source；0 = 未知/环境）
    pub killer_eid: u32,
    /// 该车发射过的弹种全局 id（去重，最多 16 个）——实际搭载配置推断证据
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shell_ids: Vec<u32>,
    /// 实际搭载的炮塔配置（build_configs dense 索引；None = 证据不足，前端用顶级配置）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turret_index: Option<u32>,
    /// 实际搭载的主炮配置（dense 索引；语义同上）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gun_index: Option<u32>,
    /// 有效数据区段 flat [t0, t1, ...]（闭区间对）
    pub coverage: Vec<f32>,
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
}

/// 实际搭载配置描述符（updateArena subtype 1 ARENA_INFO 的 field 1.2 blob，2026-09-25 破译）：
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
    /// 编码变体稳健（J39 与训练房两种布局均已实测）。
    fn parse_args(args: &[u8], valid_tanks: &[u32]) -> Option<Self> {
        for i in 2..args.len().saturating_sub(14) {
            if args[i - 2] != 0x12 || args[i - 1] != 0x0f { continue; }
            let blob = &args[i..i + 15];
            let tank_id = u16::from_le_bytes([blob[0], blob[1]]) as u32;
            if !valid_tanks.contains(&tank_id) { continue; }
            let turret_local = u16::from_le_bytes([blob[10], blob[11]]);
            let gun_local = u16::from_le_bytes([blob[12], blob[13]]);
            if turret_local == 0 || gun_local == 0 { continue; }
            // 昵称 = blob 之后首个 `1a <len> <可打印 UTF-8>`（3..30B）
            let mut nickname = String::new();
            for j in (i + 15)..args.len().min(i + 60) {
                if args[j] != 0x1a { continue; }
                let l = args[j + 1] as usize;
                if !(3..=30).contains(&l) || j + 2 + l > args.len() { continue; }
                if let Ok(s) = std::str::from_utf8(&args[j + 2..j + 2 + l]) {
                    if s.chars().all(|c| !c.is_control()) { nickname = s.to_string(); }
                }
                break;
            }
            return Some(Self { nickname, tank_id, turret_local, gun_local });
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
    let mut out: HashMap<String, CompDescriptor> = HashMap::new();
    let valid: Vec<u32> = valid_tanks.to_vec();
    for u in combat::collect_arena_updates(packets) {
        if u.subtype != 1 || u.payload_hex.len() < 30 { continue; }
        // payload_hex → bytes（combat.rs 存 hex 串）
        let Ok(args) = (0..u.payload_hex.len() / 2)
            .map(|k| u8::from_str_radix(&u.payload_hex[k * 2..k * 2 + 2], 16))
            .collect::<Result<Vec<u8>, _>>() else { continue };
        if let Some(d) = CompDescriptor::parse_args(&args, &valid) {
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
    pub meta: PlaybackMeta,
    /// 实体 id 升序（确定性输出）
    pub vehicles: Vec<VehicleTrack>,
    /// 按开火时刻排序（作者 + 他人全部射击）
    pub shots: Vec<PlaybackShot>,
    /// 按时刻排序的击杀事件
    pub kills: Vec<KillEvent>,
    pub periods: Vec<PeriodPoint>,
}

/// 位置保留 2 位小数（0.01m）；角度 3 位（0.057°，prop2 coarse 步进 ~0.35° 之下）
fn r2(x: f32) -> f32 { (x * 100.0).round() / 100.0 }
fn r3(x: f32) -> f32 { (x * 1000.0).round() / 1000.0 }

/// 归一化到 [−π, π]（prop2 相对角 + 车体 yaw 的和可到 ±2π；数据契约统一短弧表示）
fn wrap_pi(x: f32) -> f32 {
    const PI: f32 = std::f32::consts::PI;
    const TAU: f32 = std::f32::consts::TAU;
    let mut v = x;
    while v > PI { v -= TAU; }
    while v < -PI { v += TAU; }
    v
}

/// 原始采样时刻 → coverage 闭区间对（间隙 > [`COVERAGE_GAP`] 断开）
fn build_coverage(clocks: &[f32]) -> Vec<f32> {
    let mut sorted: Vec<f32> = clocks.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut out = Vec::new();
    let Some(&first) = sorted.first() else { return out };
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
    cov.chunks_exact(2).any(|p| t >= p[0] && t <= p[1])
}

/// 弹道直线飞行时长（秒）：|to−from| / |launch_velocity|，速度不可信时 0.5s 兜底
fn flight_secs(from: &[f32; 3], to: &[f32; 3], vel: &[f32; 3]) -> f32 {
    let speed = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
    let dist = ((to[0] - from[0]).powi(2)
        + (to[1] - from[1]).powi(2)
        + (to[2] - from[2]).powi(2))
        .sqrt();
    if speed > 1.0 { (dist / speed).clamp(0.02, 8.0) } else { 0.5 }
}

/// 从射击复现数据映射回放弹道记录
fn to_playback_shot(s: &ShotReplayData, name_to_eid: &HashMap<String, u32>) -> PlaybackShot {
    let target_eid = if s.target_name.is_empty() {
        None
    } else {
        name_to_eid.get(&s.target_name).copied()
    };
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

/// 构建全场回放数据。fail 点：回放无任何实体姿态流（非对战文件/损坏）。
pub fn build_playback_data(input: &PlaybackInput) -> anyhow::Result<PlaybackData> {
    let packets = input.packets;

    // 实体索引：type=10 + prop2（车辆筛选 = 双流交集）
    let (st10, prop2) = combat::build_entity_indexes(packets);
    let timeline = CombatTimeline::parse_packets(packets);
    let entity_names = &timeline.entity_names;

    // 作者昵称 = battle_results 联表内作者账号的昵称（回放自身数据，不依赖文件名）
    let author_nickname = input.players.iter()
        .find(|p| p.account_id == input.author_account_id)
        .map(|p| p.nickname.clone())
        .unwrap_or_default();
    // 作者车辆实体 = type=5 昵称精确匹配
    let author_player_eid = combat::resolve_author_player_eid_by_nick(packets, &author_nickname);

    // 车辆实体 = st10 ∧ prop2 ∧ 采样数达标（BTreeMap 保证确定性顺序）
    let mut candidates: BTreeMap<u32, usize> = BTreeMap::new();
    for (eid, samples) in &st10 {
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

    // 全部弹道（作者 + 他人；时间轴末端扩展与列表共用）
    let shots_raw = collect_all_shots(packets, author_nickname.as_str(), input.pitch_limits, author_player_eid)?;

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
    let periods = combat::parse_arena_periods(&combat::collect_arena_updates(packets));
    for p in &periods {
        t_end = t_end.max(p.clock);
    }
    for s in &shots_raw {
        t_end = t_end.max(s.fire_time + flight_secs(&s.ball_a, &s.ball_b, &s.launch_velocity));
    }
    if !t_start.is_finite() || t_end <= t_start {
        bail!("姿态时间线为空（type=10 采样不足）");
    }
    let samples_n = ((t_end - t_start) / GRID_DT).ceil() as usize + 1;
    let meta = PlaybackMeta {
        map_id: input.map_id,
        map_name: input.map_name.clone(),
        winner_team: input.winner_team,
        friendly_team: input.players.iter()
            .find(|p| p.account_id == input.author_account_id)
            .map(|p| p.team)
            .unwrap_or(0),
        author_eid: author_player_eid,
        t_start: r2(t_start),
        samples: samples_n,
        duration: r2(t_start + (samples_n.saturating_sub(1)) as f32 * GRID_DT),
    };

    // 血量链与死亡（全实体一次解析，循环内查表）
    let initial_hp = combat::collect_initial_hp(packets);
    let hp_events = combat::parse_hp_events(packets);
    let mut death_at: HashMap<u32, f32> = HashMap::new();
    for (t, eid, _) in timeline.death_events() {
        death_at.entry(eid).or_insert(t);
    }

    let mut vehicles_out: Vec<VehicleTrack> = Vec::with_capacity(candidates.len());
    let mut cause_by_eid: HashMap<u32, u8> = HashMap::new();
    for eid in candidates.keys() {
        let tl = FilteredTimeline::build(&st10[eid]);
        let Some(tl) = tl else { continue };
        let clocks: Vec<f32> = st10[eid].iter().map(|s| s.clock).collect();

        let nickname = entity_names.get(eid).cloned().unwrap_or_default();
        let player = input.players.iter()
            .find(|pl| !nickname.is_empty() && pl.nickname == nickname);
        let is_author = if author_player_eid != 0 {
            *eid == author_player_eid
        } else {
            !author_nickname.is_empty() && nickname == author_nickname
        };
        // 俯仰极限锚定：本车昵称优先；作者无锚定时沿用其表项（与作者路径 prop9 兜底同级）
        let limits = input.pitch_limits.get(nickname.as_str())
            .or_else(|| input.pitch_limits.get(author_nickname.as_str()).filter(|_| is_author));

        // 逐网格采样：渲染位姿 + prop2 炮塔/俯仰（同刻合成绝对角）
        let mut pos = Vec::with_capacity(samples_n * 3);
        let mut hull_yaw = Vec::with_capacity(samples_n);
        let mut hull_pitch = Vec::with_capacity(samples_n);
        let mut turret_yaw = Vec::with_capacity(samples_n);
        let mut gun_pitch = Vec::with_capacity(samples_n);
        let prop2_series = prop2.get(eid);
        for i in 0..samples_n {
            let t = meta.t_start + i as f32 * GRID_DT;
            let pose = tl.pose_at(t as f64, false);
            let Some(pose) = pose else { continue };
            pos.push(r2(pose.pos[0]));
            pos.push(r2(pose.pos[1]));
            pos.push(r2(pose.pos[2]));
            hull_yaw.push(r3(pose.ang[0]));
            hull_pitch.push(r3(pose.ang[1]));
            match combat::prop2_at(prop2_series, t) {
                Some((rel, frac)) => {
                    turret_yaw.push(r3(wrap_pi(rel + pose.ang[0])));
                    gun_pitch.push(match limits {
                        Some(lim) => r3(combat::decode_prop2_gun_pitch(frac, lim, rel)),
                        None => r3(pose.ang[1]),
                    });
                }
                None => {
                    turret_yaw.push(r3(pose.ang[0]));
                    gun_pitch.push(r3(pose.ang[1]));
                }
            }
        }

        // HP 链：满血锚点 + method1 事件（同值去重）；击杀者 = hp==0 事件 source
        let mut hp: Vec<(f32, u16)> = Vec::new();
        let mut killer_eid = 0u32;
        let mut cause = 255u8;
        if let Some((t0, hp0)) = initial_hp.get(eid) {
            hp.push((r2(*t0), *hp0));
        }
        for e in &hp_events {
            if e.victim != *eid {
                continue;
            }
            // overkill 时服务器 HP 略负（int16 语义），按 u16 读出回绕（如 -3 → 65533）；
            // 真实 max HP << 32767，负值一律钳 0（死亡）
            let hp_v = if e.hp > 32767 { 0 } else { e.hp };
            if hp.last().map(|(_, h)| *h) == Some(hp_v) {
                continue;
            }
            hp.push((r2(e.clock), hp_v));
            if hp_v == 0 {
                killer_eid = e.source;
                cause = e.cause;
            }
        }
        if killer_eid != 0 {
            cause_by_eid.insert(*eid, cause);
        }

        // 发射弹种全局 id（配置推断证据；去重上限 16）
        let mut shell_ids: Vec<u32> = Vec::new();
        for s in &shots_raw {
            if s.shooter_eid != *eid || s.shell_id == 0 || shell_ids.len() >= 16 { continue; }
            if !shell_ids.contains(&s.shell_id) {
                shell_ids.push(s.shell_id);
            }
        }

        vehicles_out.push(VehicleTrack {
            eid: *eid,
            account_id: player.map(|p| p.account_id).unwrap_or(0),
            nickname,
            tank_id: player.map(|p| p.tank_id).unwrap_or(0),
            tank_name: player.and_then(|p| input.tank_names.get(&p.tank_id).cloned()).unwrap_or_default(),
            team: player.map(|p| p.team).unwrap_or(0),
            is_author,
            max_hp: initial_hp.get(eid).map(|(_, h)| *h).unwrap_or(0),
            pos,
            hull_yaw,
            hull_pitch,
            turret_yaw,
            gun_pitch,
            hp,
            death_t: death_at.get(eid).copied().map(r2),
            killer_eid,
            shell_ids,
            turret_index: None,
            gun_index: None,
            coverage: build_coverage(&clocks),
        });
    }

    let mut kills: Vec<KillEvent> = vehicles_out.iter()
        .filter_map(|v| v.death_t.map(|t| KillEvent {
            t,
            killer_eid: v.killer_eid,
            victim_eid: v.eid,
            cause: cause_by_eid.get(&v.eid).copied()
                .unwrap_or(if v.killer_eid != 0 { 0 } else { 3 }),
        }))
        .collect();
    kills.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());

    let mut shots: Vec<PlaybackShot> = shots_raw.iter()
        .map(|s| to_playback_shot(s, &name_to_eid))
        .collect();
    shots.sort_by(|a, b| a.t_fire.partial_cmp(&b.t_fire).unwrap());

    let periods_out: Vec<PeriodPoint> = periods.iter()
        .map(|p| PeriodPoint {
            clock: r2(p.clock),
            period: p.period,
            remaining_s: p.remaining_s,
            duration_s: p.duration_s,
        })
        .collect();

    Ok(PlaybackData { meta, vehicles: vehicles_out, shots, kills, periods: periods_out })
}

/// 合并作者严格路径 + 他人宽松路径（与 web `replay_shots_handler` 同构；时间轴构建需全部弹道）。
/// 作者严格路径是 fail-fast 设计（边界数据缺失即 bail）——全场回放不应因此整场不可用：
/// 失败时降级为宽松路径提取**全部**发射（含作者，按 shooter_eid 补回 is_author 标记）。
fn collect_all_shots(
    packets: &[(u32, f32, &[u8])],
    author_nick: &str,
    pitch_limits: &GunPitchLimits,
    author_player_eid: u32,
) -> anyhow::Result<Vec<ShotReplayData>> {
    match combat::extract_shot_replays_auto_with_limits(packets, author_nick, pitch_limits) {
        Ok(mut all) => {
            let others = combat::extract_other_shot_replays_with_limits(packets, author_player_eid, pitch_limits);
            all.extend(others.shots);
            all.sort_by(|a, b| a.fire_time.partial_cmp(&b.fire_time).unwrap());
            Ok(all)
        }
        Err(strict_err) => {
            eprintln!("[playback] 作者严格路径提取失败（{strict_err:#}），降级宽松全路径");
            let mut all = combat::extract_other_shot_replays_with_limits(packets, 0, pitch_limits).shots;
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

    // ---------- 端到端探针（真实回放分布证据） ----------
    //
    // 运行：WOTB_PLAYBACK_PROBE=replay_samples cargo test playback_probe -- --ignored --nocapture
    //
    // 判定项：
    //   P1 车辆收录：候选实体数 ∈ [10, 16]（14 车 ± 解析边界；KineticObject 等被双流过滤）；
    //   P2 位姿一致性：开火时刻的网格采样位 ≈ ShotReplayData.shooter_render（同为滤波器输出，
    //      仅 0.1s 网格舍入差，≤0.6m）；
    //   P3 角度值域：turret_yaw ∈ [-π−0.1, π+0.1]、gun_pitch ∈ [-0.7, 0.7] rad；
    //   P4 血量链：max_hp>0、HP 单调不增、死亡车末值 0。
    #[test]
    #[ignore = "端到端探针：WOTB_PLAYBACK_PROBE=<path|dir> cargo test playback_probe -- --ignored --nocapture"]
    fn playback_probe() {
        let root = std::env::var("WOTB_PLAYBACK_PROBE").unwrap_or_else(|_| "replay_samples".into());
        let path = std::path::Path::new(&root);
        let files: Vec<std::path::PathBuf> = if path.is_file() {
            vec![path.to_path_buf()]
        } else {
            std::fs::read_dir(path).unwrap()
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
            let packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
                let t = match &pkt.payload {
                    wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
                    wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                    wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
                };
                (t, pkt.clock_secs, &pkt.raw_payload[..])
            }).collect();
            let br = replay.read_battle_results().ok();
            let players: Vec<PlaybackPlayer> = br.as_ref().map(|br| {
                br.player_results.iter().map(|pr| {
                    let joined = br.players.iter().find(|p| p.account_id == pr.info.account_id);
                    PlaybackPlayer {
                        account_id: pr.info.account_id,
                        nickname: joined.map(|p| p.info.nickname.clone()).unwrap_or_default(),
                        team: joined.map(|p| if p.info.team == 1 { 1u8 } else { 2u8 }).unwrap_or(0),
                        tank_id: pr.info.tank_id,
                    }
                }).collect()
            }).unwrap_or_default();
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
            let pb = build_playback_data(&input)
                .unwrap_or_else(|e| panic!("{name}: 构建失败 {e}"));

            // P1 车辆收录（仅完整战斗；退化样例（训练房/片段，时长短或车辆少）跳过断言）
            let nv = pb.vehicles.len();
            let named = pb.vehicles.iter().filter(|v| v.team != 0).count();
            eprintln!("--- {name}");
            eprintln!("    P1 车辆 {nv}（具名 {named}），射击 {} 发，击杀 {}，时长 {:.1}s",
                pb.shots.len(), pb.kills.len(), pb.meta.duration);
            if nv < 10 || pb.meta.duration < 60.0 {
                eprintln!("    ~~ 退化样例（车辆 {nv} / 时长 {:.1}s），跳过断言", pb.meta.duration);
                continue;
            }
            // 7v7/10v10 等模式车数不同（GB109=10v10 → 20 车）
            assert!((8..=28).contains(&nv), "P1 车辆数异常 {nv}");
            assert_eq!(pb.vehicles.iter().filter(|v| v.is_author).count(), 1, "作者车应恰 1 辆");

            // P2 位姿一致性（开火时刻网格采样 vs 单发复现渲染锚点）
            // PlaybackShot 已精简掉锚点——探针在同模块直接调 collect_all_shots 取原始
            // ShotReplayData（含 shooter_render + fire_time）
            let author_eid = pb.meta.author_eid;
            let shots_raw = collect_all_shots(&packets, &name, &GunPitchLimits::new(), author_eid)
                .unwrap_or_default();
            let mut checked = 0usize;
            let mut worst = 0.0f32;
            let by_eid: HashMap<u32, &VehicleTrack> = pb.vehicles.iter().map(|v| (v.eid, v)).collect();
            for s in &shots_raw {
                let Some(anchor) = &s.shooter_render else { continue };
                let Some(sv) = by_eid.get(&s.shooter_eid) else { continue };
                // 锚点 = 命中/开火事件帧（ceil）；网格是 0.1s 重采样，事件帧必落在相邻两格
                // 之间——判据：锚点距较近端 ≤ 0.3m + 0.12×局部车速（重采样容差，非固定阈值）
                let fi = (s.fire_time - pb.meta.t_start) / GRID_DT;
                if fi < 0.0 { continue; }
                let i0 = (fi.floor() as usize).min(pb.meta.samples - 1);
                let i1 = (i0 + 1).min(pb.meta.samples - 1);
                let gp = |i: usize, k: usize| sv.pos[i * 3 + k];
                let d = |i: usize| ((gp(i, 0) - anchor.pos[0]).powi(2)
                    + (gp(i, 1) - anchor.pos[1]).powi(2)
                    + (gp(i, 2) - anchor.pos[2]).powi(2)).sqrt();
                let dmin = d(i0).min(d(i1));
                let v = ((gp(i1, 0) - gp(i0, 0)).powi(2)
                    + (gp(i1, 1) - gp(i0, 1)).powi(2)
                    + (gp(i1, 2) - gp(i0, 2)).powi(2)).sqrt() / GRID_DT;
                worst = worst.max(dmin);
                checked += 1;
                assert!(dmin <= 0.3 + 0.12 * v,
                    "P2 开火位姿偏差 {dmin:.3}m（局部车速 {v:.1}m/s）@ {} t={}", name, s.fire_time);
            }
            assert!(checked > 0, "P2 无可校验射击（无 shooter_render 锚点）");
            eprintln!("    P2 位姿一致性 {checked} 发，最大偏差 {worst:.3}m");

            // P3 角度值域 + P4 血量链
            for v in &pb.vehicles {
                let n = pb.meta.samples;
                assert_eq!(v.hull_yaw.len(), n, "{} 网格长度", v.eid);
                for i in 0..n {
                    assert!(v.turret_yaw[i] >= -std::f32::consts::PI - 0.1
                        && v.turret_yaw[i] <= std::f32::consts::PI + 0.1,
                        "P3 turret_yaw 越域 {} {}", v.eid, v.turret_yaw[i]);
                    assert!(v.gun_pitch[i] >= -1.575 && v.gun_pitch[i] <= 1.575,
                        // 值域仅为量纲哨兵（度→弧度错开会到 ±57）：解码值域 = 车型俯仰极限
                        // （SPG 仰角 ~75°=1.31rad），兜底值 = 车体 pitch（跌落/翻滚可近 ±π/2）
                        "P3 gun_pitch 越域 {} {}", v.eid, v.gun_pitch[i]);
                }
                if v.team != 0 {
                    assert!(v.max_hp > 0, "P4 具名车 max_hp=0 {}", v.nickname);
                }
                let mut hp_prev = v.max_hp;
                for (_, h) in &v.hp {
                    assert!(*h <= hp_prev, "P4 HP 回升 {} {}: {hp_prev}→{h}", v.eid, v.nickname);
                    hp_prev = *h;
                }
                if v.death_t.is_some() && !v.hp.is_empty() {
                    assert_eq!(v.hp.last().unwrap().1, 0, "P4 死亡车末 HP 应为 0 {}", v.nickname);
                }
            }
        }
    }
}
