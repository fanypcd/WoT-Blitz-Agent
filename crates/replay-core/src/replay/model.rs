//! 内部回放模型（架构契约第 4/5 节）：一次包扫描的权威领域产物。
//!
//! - Rust 内部模型 ≠ 对外切面：切面（`crate::facets`）从本模型纯投影，依赖方向恒为
//!   切面 → 模型，模型不感知任何消费方；
//! - 包流只扫一遍：位姿/炮塔/血量/死亡/击杀播报/可见性/反馈计数/阶段/弹道在
//!   [`ReplayModel::scan`] 一次聚合，各投影（全场回放/评审/名人堂）不再各自扫包；
//! - unknown ≠ 0：档案字段一律 Option（联表失败/缺失 = None），0 仅作协议哨兵透传；
//! - 结算（battle_results）不是包流产物，以花名册形式并表；作者视角字段留在结算层，
//!   由切面投影时剔除。

use std::collections::{BTreeMap, HashMap};

use super::combat::{
    self, hp_terminal_normalized, AoiPresence, ArenaPeriod, AssaultBaseStateTransition,
    CombatEventType, CombatTimeline, ConsumableTransition, FeedbackCounterEvent, GunPitchLimits,
    HpEvent, KillFeedEvent, ModuleCrewStateEvent, RawReloadDuration, RawReloadPhase,
    ShotReplayData, St10Sample, SupremacyBaseStateTransition, SupremacyPointsSample,
};
use super::playback::{self, KillEvent, PlaybackPlayer};

/// 单实体静态档案（全场不变的证据链并表产物）
#[derive(Debug, Clone, Default)]
pub struct EntityRecord {
    pub eid: u32,
    /// type=5 昵称（SSOT UTF-8 解码；非法/缺失 → None）
    pub nickname: Option<String>,
    /// 结算花名册联表（按昵称；匿名/联表失败 = None，不编码 0）
    pub account_id: Option<u32>,
    pub team: Option<u8>,
    pub tank_id: Option<u32>,
    /// type=5 满血锚点（血量链 seed；None = 无锚点）
    pub max_hp: Option<u16>,
    /// 9 字节配件选择串（type=5 尾部 loadout 块，字节域 100..=123 校验通过）
    pub equipment: Option<[u8; 9]>,
    /// 开局 loadout 的 raw item 描述符（6×14B：item[0..2]=3 消耗品、item[3..5]=3 给养；
    /// 内部字段未解码 → 原样保留，不赋语义）
    pub loadout_items: Vec<[u8; 14]>,
    /// ARENA_INFO 15B 组成 blob：实际搭载炮塔局部 id（按昵称联表，缺失 = None）
    pub turret_local: Option<u16>,
    /// 组成 blob 主炮局部 id（语义同上）
    pub gun_local: Option<u16>,
    pub is_author: bool,
}

/// 一次死亡：prop1 死亡广播时刻 + 血量链 hp==0 事件的击杀者/死因
/// （两路证据配对归一；killer_eid = 0 表示无血量链归属/环境——如溺死血量不归零）
#[derive(Debug, Clone)]
pub struct DeathRecord {
    pub t: f32,
    pub killer_eid: u32,
    /// method1 cause（killer_eid != 0 时有效）
    pub cause: u8,
}

/// 时序层：全部带回放时钟（秒）。
/// 原始采样不滤波——滤波/0.1s 网格/角度解卷绕是回放切面投影层职责（playback.rs）。
#[derive(Debug, Default)]
pub struct Timeline {
    /// DetachedTurret class-8 ownership and AoI materialization (raw rigid body).
    pub detached_turrets: Vec<super::detached_turrets::DetachedTurretRecord>,
    /// Known class-8 eids must never enter vehicle fallback, including rejected tracks.
    pub detached_turret_eids: std::collections::HashSet<u32>,
    /// Type5 class=2 战斗车辆实体集合（协议权威实体类型；类冲突的实体已剔除）。
    /// 车辆身份只取这里——type=10 位姿流也出现在相机/区域/炮塔残骸上，
    /// 炮塔角流只在炮塔转动时存在，二者都不是"是不是车"的判据。
    /// 播放切面另与结算花名册求交（只显示花名册参与者）；本集合保留协议的完整
    /// class=2 事实（含观察者/未联表实体）。
    pub vehicle_eids: std::collections::HashSet<u32>,
    /// 原始位姿流（type=10，未滤波）
    pub poses: HashMap<u32, Vec<St10Sample>>,
    /// 炮塔打包角流（type=7 prop2：(clock, 相对角 rad, frac 原始 6bit)）
    pub turret: HashMap<u32, Vec<(f32, f32, u16)>>,
    /// 血量链原始事件（method1：受害者/来源/原因；overkill 负值原样保留）
    pub hp_events: Vec<HpEvent>,
    /// 满血锚点（type=5，血量链 seed）
    pub initial_hp: HashMap<u32, (f32, u16)>,
    /// 血量链去重序列（含满血锚点；与原逐车组装同式，回放切面直接取用）
    pub hp_series: BTreeMap<u32, Vec<(f32, u16)>>,
    /// 死亡终态（有 prop1 死亡广播或血量链归零的实体）＝[`Timeline::death_records`]
    /// 首条（单死回放二者一致；保留供既有消费方）
    pub deaths: BTreeMap<u32, DeathRecord>,
    /// 全部死亡记录（t 升序）。普通回放每车 ≤1 条；复活模式同一 eid 可多条——
    /// 第 n 条与第 n-1 条之间必有复活证据（[`Timeline::resurrections`]）。
    /// 构成 = prop1 死亡广播 × method1 hp==0 归零事件配对（build 内注释）。
    pub death_records: BTreeMap<u32, Vec<DeathRecord>>,
    /// 复活时刻（升序）：死亡未闭合期内「绝对血量证据终态(归一化 0) → 正值」的首次回归。
    /// 载体 = method1 ∪ prop3（录像者自身血量常只走 prop3，见 [`combat::Prop3Health`]）。
    /// 无复活证据则为空——fail-closed：消费方按死亡终态语义处理，行为与既往一致。
    pub resurrections: BTreeMap<u32, Vec<f32>>,
    /// 击杀播报原始流（wrapper6；未门控，消费用 [`ReplayModel::kill_events`]）
    pub kill_feed: Vec<KillFeedEvent>,
    /// AoI 可见窗口（Type33/Type5 物化开段、Type4 关段；敌方重入 = 多段）
    pub presence: Vec<AoiPresence>,
    /// method8 伤害/命中反馈通知原始流（全变体，不分类；见 [`combat::HitNotice`]）
    pub hit_notices: Vec<combat::HitNotice>,
    /// prop3（type=7 sub=3）血量属性广播原始流（录像者自身血量常只走这一路；见 [`combat::Prop3Health`]）
    pub prop3_health: Vec<combat::Prop3Health>,
    /// 作者战斗反馈计数（0x0c；code 语义见 [`combat::feedback_code`]）
    pub counters: Vec<FeedbackCounterEvent>,
    /// prop10 累计伤害进度（原始序列；相邻差 = 区段伤害）
    pub damage_progress: BTreeMap<u32, Vec<(f32, u32)>>,
    /// 战局阶段（updateArena PERIOD）
    pub periods: Vec<ArenaPeriod>,
    /// 全部弹道（作者严格 + 他人宽松合并，按开火时刻排序）
    pub shots: Vec<ShotReplayData>,
    /// true = shots 来自宽松降级路径（作者严格提取失败；provenance 契约见
    /// `playback::collect_all_shots`）
    pub shots_from_loose_path: bool,
    /// type=5 昵称表（eid → 昵称）
    pub entity_names: HashMap<u32, String>,
    /// 作者 Avatar 实体（0 = 未解析）
    pub author_eid: u32,
    /// Supremacy 基地状态时间线（争霸模式；非争霸场为空）。
    /// WotbTools wrapper12/root11 PROVEN 移植：absent=维持前值、显式 0=清空、
    /// 占领中 owner 变更清 capture。seek 语义 = 取 ≤t 的每基地最后一条。
    pub supremacy_bases: Vec<SupremacyBaseStateTransition>,
    /// Supremacy 实时点数采样（wrapper13/root12；仅真实广播，不推算）
    pub supremacy_points: Vec<SupremacyPointsSample>,
    /// 攻防战/遭遇战单基地目标存在性（wrapper8/root8 目标族出现即真；与是否有
    /// 占领进度无关）——供前端在"全程无人占领"时仍能画出目标圈
    pub assault_objective_present: bool,
    /// 攻防战单基地占领进度时间线（wrapper8/root8；非攻防战场次为空）
    pub assault_bases: Vec<AssaultBaseStateTransition>,
    /// 消耗品生命周期事件（Type32 flag=0；含 wireCode/state/param 原样）
    pub consumables: Vec<ConsumableTransition>,
    /// 车辆模块/乘员状态事件（Avatar method16）
    pub module_crew_states: Vec<ModuleCrewStateEvent>,
    /// 区域可破坏物实体（100m 格子锚点；type=5 class=3，eid 升序）
    pub destructible_areas: Vec<crate::replay::destructibles::DestructibleArea>,
    /// 区域可破坏物事件（type=32 短广播、envelope=区域实体；时钟升序）
    pub destructible_events: Vec<crate::replay::destructibles::AreaDestructibleEvent>,
    /// 实时装填相位（arena subtype 15/16/17，**仅本方全队**；相位码 f2 与计数 f4 原样透传）
    pub reloads: Vec<RawReloadPhase>,
    /// 权威「当前生效完整装填时长」（方法 35；时间升序）
    pub reload_effective: Vec<RawReloadDuration>,
}

/// 内部回放模型：包流单次扫描产物 + 结算花名册并表
#[derive(Debug, Default)]
pub struct ReplayModel {
    /// 实体静态档案（eid 升序）
    pub entities: Vec<EntityRecord>,
    pub timeline: Timeline,
    /// 包类型直方图（诊断层原料）
    pub packet_histogram: BTreeMap<u32, u64>,
}

/// 模型扫描输入
pub struct ScanInput<'a> {
    pub packets: &'a [(u32, f32, &'a [u8])],
    /// 结算花名册（battle_results 联表产物；缺结算时传 `&[]`，档案联表字段为 None）
    pub roster: &'a [PlaybackPlayer],
    pub author_account_id: u32,
    /// 俯仰极限锚定表（弹道提取/炮管俯仰解码；无锚定传空表）
    pub pitch_limits: &'a GunPitchLimits,
}

fn r2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

impl ReplayModel {
    /// 单次扫描构建模型。fail 点与原全场回放构建一致：弹道提取的作者严格路径内部
    /// 已降级宽松路径，本函数实际只因包流结构性缺失（非回放文件）而失败。
    pub fn scan(input: &ScanInput) -> anyhow::Result<Self> {
        let packets = input.packets;

        let mut packet_histogram: BTreeMap<u32, u64> = Default::default();
        let mut detached_scan = super::detached_turrets::DetachedTurretScan::default();
        for (kind, clock, payload) in packets {
            *packet_histogram.entry(*kind).or_insert(0) += 1;
            detached_scan.observe(*kind, *clock, payload);
        }

        let author_nickname = input
            .roster
            .iter()
            .find(|p| p.account_id == input.author_account_id)
            .map(|p| p.nickname.clone())
            .unwrap_or_default();

        // arena 流一次收集 {1,3,6}（comps/periods/kill_feed 三个消费方合用；
        // 高频 RELOAD_TIME 等子类型在收集期即丢弃）——提前到作者解析之前：
        // comp 条目的 account_id（P3 探针 field7 定案）是作者实体解析的精确键
        let arena_updates = combat::collect_arena_updates_filtered(packets, |s| {
            s == 1 || s == 3 || s == 6
            // 装填相位（subtype 15/16/17）也在此收集：本方全队的装填开始/就绪/弹夹内间隔
            || s == combat::ARENA_SUB_RELOAD_TIME
            || s == combat::ARENA_SUB_RELOAD_TIME_UPDATE
            || s == combat::ARENA_SUB_RELOAD_TIME_LIST
        });
        let valid_tanks: Vec<u32> = input.roster.iter().map(|p| p.tank_id).collect();
        let comps = playback::comp_descriptors_from_updates(&arena_updates, &valid_tanks);
        // 作者实体 = comps 条目 account_id 精确匹配（重名/匿名免疫），退回昵称匹配
        let author_eid = comps
            .values()
            .find(|c| c.account_id != 0 && c.account_id == input.author_account_id as u64)
            .map(|c| c.eid)
            .filter(|e| *e != 0)
            .unwrap_or_else(|| {
                combat::resolve_author_player_eid_by_nick(packets, &author_nickname)
            });

        // —— 共享扫描（契约"包流只扫一遍"）：位姿/炮塔/血量/名字/配件/发射等
        // 全部索引只建一份，弹道两路提取（collect_all_shots）与实体档案并表均从此取数。
        let shared = combat::build_shot_scan_shared(packets, author_eid);

        let ct = CombatTimeline::parse_packets(packets);
        let presence = combat::collect_aoi_lifecycle(packets);
        let hit_notices = combat::collect_hit_notices(packets);
        let prop3_health = combat::collect_prop3_health(packets);
        let counters = combat::collect_feedback_counters(packets);
        let kill_feed = combat::kill_feed_from_updates(&arena_updates);
        let periods = combat::parse_arena_periods(&arena_updates);
        // Supremacy 目标状态/点数（subtype48 wrapper12/13；非争霸场为空）——
        // WotbTools PROVEN 语义移植，sparse 重建见 arena 模块
        let supremacy_bases = combat::reconstruct_supremacy_base_states(
            combat::collect_supremacy_base_updates(packets),
        );
        let supremacy_points = combat::collect_supremacy_points(packets);
        // 攻防战单基地（wrapper8/root8；与争霸 wrapper12 天然互斥）
        let assault_updates = combat::collect_assault_base_updates(packets);
        // 目标存在性与进度分开：无占领活动的攻防/遭遇战场次 progress 为空但目标已在
        // 实时装填相位（原样透传；消费方只认已闭环子集 f2∈{3,4,7}、f4=1）
        let reloads = combat::reload_phases_from_updates(&arena_updates);
        // 权威有效装填时长（方法 35；独立于 arena 族，直接从包流收集）
        let reload_effective = combat::reload_durations_from_packets(packets);
        let assault_objective_present = combat::has_assault_objective(&assault_updates);
        let assault_bases = combat::reconstruct_assault_base_states(assault_updates);
        // 消耗品生命周期（Type32 flag=0；与 flag=1 炮弹警告同包不同族）
        let consumables = combat::collect_consumable_transitions(packets);
        let module_crew_states = combat::collect_module_crew_states(packets);
        // 区域可破坏物：先收格子锚点（type=5 class=3），再收其上的短广播事件
        let destructible_areas = crate::replay::destructibles::collect_destructible_areas(packets);
        let destructible_events = crate::replay::destructibles::collect_area_destructible_events(
            packets,
            &destructible_areas,
        );
        let hp_events = &shared.hp_events;
        let initial_hp = &shared.initial_hp;
        let equipment = &shared.vehicle_equipment;
        let loadouts = combat::collect_vehicle_loadout(packets);

        // —— 血量链去重序列 + 死亡列表 + 复活轨道（一次遍历，单死语义与原逐车组装逐位一致） ——
        let mut hp_series: BTreeMap<u32, Vec<(f32, u16)>> = BTreeMap::new();
        for (eid, (t0, h0)) in initial_hp.iter() {
            hp_series.entry(*eid).or_default().push((*t0, *h0));
        }
        // 死亡的两路证据：prop1 死亡广播（时刻权威；溺死等血量不归零的死亡只有这一路）
        // 与 method1 hp==0 归零事件（凶手/死因权威；血量链去重语义下每次死亡一条）。
        let mut zero_hits: BTreeMap<u32, Vec<(f32, u32, u8)>> = BTreeMap::new();
        let mut broadcasts: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
        for (t, eid, _) in ct.death_events() {
            broadcasts.entry(eid).or_default().push(t);
        }
        for e in hp_events {
            // overkill 时服务器 HP 略负（int16 语义），按 u16 读出回绕 → 一律钳 0
            let hp_v = if e.hp > 32767 { 0 } else { e.hp };
            let series = hp_series.entry(e.victim).or_default();
            if series.last().map(|(_, h)| *h) == Some(hp_v) {
                continue;
            }
            series.push((e.clock, hp_v));
            if hp_v == 0 {
                zero_hits
                    .entry(e.victim)
                    .or_default()
                    .push((e.clock, e.source, e.cause));
            }
        }
        // —— 隐藏段掉血兑现：重入 Type5 物化快照血量并入血量链 ——
        // Type4 隐藏段「零更新」（type=7/8/10 全断），method1 只覆盖在场段；重广播 type=5
        // 的 offset51 = 物化时刻当前血量（§2.6，非开局值）。客户端语义 = 二次点亮物化时
        // HP 条直接跳到快照值——盲炮/视野外伤害没有 method1 事件（来源不可知，fail-closed
        // 不归属凶手），但绝对血量必须在此兑现，否则重入后血量链停在丢视野前旧值、直到
        // 下一次受击才修正（实测 J20 CN_LEVITHAN：重入快照 348 vs 血量链 1192，缺 844）。
        // 首段与 initial_hp seed 同包同偏移（同钟同值 → 插入后即被去重还原）；哨兵族归一
        // 与 method1 显示口径同判（hp_terminal_normalized；死亡面权威仍在 hp_state/prop1，
        // 不在此扩大）。无锚点实体（首物化短包）以首个带血量快照为链起点——有证据不丢弃。
        for p in &presence {
            let Some(raw) = p.hp_raw else { continue };
            let hp_v = hp_terminal_normalized(raw);
            let Some(series) = hp_series.get_mut(&p.eid) else {
                hp_series.insert(p.eid, vec![(p.t_in, hp_v)]);
                continue;
            };
            // 插在首个 clock ≥ t_in 的既有事件之前（同钟物化先于更新，客户端顺序）
            let idx = series.partition_point(|(t, _)| *t < p.t_in);
            let prev_differs = idx == 0 || series[idx - 1].1 != hp_v;
            if prev_differs {
                series.insert(idx, (p.t_in, hp_v));
                // 同值紧随的既有事件（物化/更新同值突发）由物化时刻代表
                if series.get(idx + 1).map(|(_, h)| *h) == Some(hp_v) {
                    series.remove(idx + 1);
                }
            }
        }
        // 死亡记录 = 广播 × 归零 就近配对（同一次死亡的两路证据；门控 5s 与击杀播报
        // 归属门控同源）。单广播×单归零无条件配对（t 取广播、凶手/死因取归零）——
        // 与既往单死行为逐位一致；仅多证据（复活模式）走 5s 就近配对，落单者按自身
        // 证据立死。复活模式每一正例仍待真实样本验证（fail-closed：配对产物只多不少）。
        let mut death_records: BTreeMap<u32, Vec<DeathRecord>> = BTreeMap::new();
        for (eid, mut hits) in zero_hits {
            let mut bs = broadcasts.remove(&eid).unwrap_or_default();
            hits.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            bs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut records: Vec<DeathRecord> = Vec::new();
            if bs.len() == 1 && hits.len() == 1 {
                records.push(DeathRecord {
                    t: bs[0],
                    killer_eid: hits[0].1,
                    cause: hits[0].2,
                });
            } else {
                let mut hit_used = vec![false; hits.len()];
                for bt in &bs {
                    let best = hits
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !hit_used[*i])
                        .filter(|(_, (t, _, _))| (t - bt).abs() <= 5.0)
                        .min_by(|a, b| {
                            (a.1 .0 - bt)
                                .abs()
                                .partial_cmp(&(b.1 .0 - bt).abs())
                                .unwrap()
                        });
                    match best {
                        Some((i, (_, source, cause))) => {
                            hit_used[i] = true;
                            records.push(DeathRecord {
                                t: *bt,
                                killer_eid: *source,
                                cause: *cause,
                            });
                        }
                        None => records.push(DeathRecord {
                            t: *bt,
                            killer_eid: 0,
                            cause: 255,
                        }),
                    }
                }
                for (i, (t, source, cause)) in hits.iter().enumerate() {
                    if !hit_used[i] {
                        records.push(DeathRecord {
                            t: *t,
                            killer_eid: *source,
                            cause: *cause,
                        });
                    }
                }
            }
            records.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
            death_records.insert(eid, records);
        }
        for (eid, ts) in broadcasts {
            let records = death_records.entry(eid).or_default();
            records.extend(ts.into_iter().map(|t| DeathRecord {
                t,
                killer_eid: 0,
                cause: 255,
            }));
            records.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        }
        // 既有消费方口径：首次死亡（单死回放 = 唯一死亡，逐位等价）
        let deaths: BTreeMap<u32, DeathRecord> = death_records
            .iter()
            .map(|(eid, v)| (*eid, v[0].clone()))
            .collect();
        // 复活轨道：死亡未闭合期内「绝对血量证据 终态(归一化 0) → 正值」的首次回归。
        // 载体 = method1 ∪ prop3（同刻双载体取先到者闭合，天然去重）；prop3 只参与
        // 复活方向、不参与死亡判定（死亡语义不变）。语料库普通回放全零回归见
        // `playback::tests`（无复活证据 → 轨道空 → 消费方行为与既往一致）。
        let mut resurrections: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
        {
            let mut merged: BTreeMap<u32, Vec<(f32, u16)>> = BTreeMap::new();
            for e in hp_events.iter() {
                let hp_v = if e.hp > 32767 { 0 } else { e.hp };
                merged.entry(e.victim).or_default().push((e.clock, hp_v));
            }
            for p in prop3_health.iter() {
                merged
                    .entry(p.eid)
                    .or_default()
                    .push((p.clock, hp_terminal_normalized(p.hp_raw)));
            }
            for (eid, evs) in &mut merged {
                evs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                let Some(ds) = death_records.get(eid) else {
                    continue;
                };
                let mut out: Vec<f32> = Vec::new();
                let (mut di, mut dead) = (0usize, false);
                for &(t, v) in evs.iter() {
                    while di < ds.len() && ds[di].t <= t {
                        dead = true;
                        di += 1;
                    }
                    if dead && v > 0 {
                        out.push(t);
                        dead = false;
                    }
                }
                if !out.is_empty() {
                    resurrections.insert(*eid, out);
                }
            }
        }

        // —— prop10 伤害进度 ——
        let mut damage_progress: BTreeMap<u32, Vec<(f32, u32)>> = BTreeMap::new();
        for e in &ct.events {
            if let CombatEventType::DamageCounter { cumulative_damage } = &e.event_type {
                damage_progress
                    .entry(e.entity_id)
                    .or_default()
                    .push((e.timestamp, *cumulative_damage));
            }
        }

        // —— 弹道（作者严格 + 他人宽松合并；降级策略与 provenance 见 playback::collect_all_shots） ——
        let collected = playback::collect_all_shots(&shared, author_eid, input.pitch_limits)?;
        let shots = collected.shots;

        // —— 实体档案并表（eid 并集 = 位姿 ∪ 炮塔 ∪ 名字 ∪ 锚点 ∪ 配件 ∪ 在场） ——
        // （valid_tanks/comps 已在作者解析前构建——作者实体解析同源共用）
        let mut eids: Vec<u32> = shared
            .st10
            .keys()
            .copied()
            .chain(shared.prop2.keys().copied())
            .chain(ct.entity_names.keys().copied())
            .chain(initial_hp.keys().copied())
            .chain(equipment.keys().copied())
            .chain(presence.iter().map(|p| p.eid))
            .collect();
        eids.sort_unstable();
        eids.dedup();

        let comp_by_eid: HashMap<u32, &playback::CompDescriptor> = comps
            .values()
            .filter(|c| c.eid != 0)
            .map(|c| (c.eid, c))
            .collect();
        let mut entities = Vec::with_capacity(eids.len());
        for eid in eids {
            let nickname = ct.entity_names.get(&eid).cloned();
            // 组成条目 account_id（field7，P3 探针实证）是花名册联表的精确键——
            // 重名/匿名 "Anonyme" 免疫；退回昵称首条匹配（comps 未覆盖的实体）
            let joined = comp_by_eid
                .get(&eid)
                .filter(|c| c.account_id != 0)
                .and_then(|c| {
                    input
                        .roster
                        .iter()
                        .find(|p| p.account_id as u64 == c.account_id)
                })
                .or_else(|| {
                    nickname
                        .as_ref()
                        .and_then(|n| input.roster.iter().find(|p| &p.nickname == n))
                });
            // 组成 blob 昵称是原始 UTF-8（不经 ascii 过滤），与 type=5 名字表对非 ascii
            // 昵称可能不交集——按 eid 能联则联，联不上退昵称，再联不上不猜
            let comp = comp_by_eid
                .get(&eid)
                .copied()
                .or_else(|| nickname.as_ref().and_then(|n| comps.get(n)));
            let is_author = if author_eid != 0 {
                eid == author_eid
            } else {
                !author_nickname.is_empty() && nickname.as_deref() == Some(author_nickname.as_str())
            };
            entities.push(EntityRecord {
                eid,
                nickname,
                account_id: joined.map(|p| p.account_id),
                team: joined.map(|p| p.team),
                tank_id: joined.map(|p| p.tank_id),
                max_hp: initial_hp.get(&eid).map(|(_, h)| *h),
                equipment: equipment.get(&eid).copied(),
                loadout_items: loadouts
                    .get(&eid)
                    .map(|l| l.items.clone())
                    .unwrap_or_default(),
                turret_local: comp.map(|c| c.turret_local),
                gun_local: comp.map(|c| c.gun_local),
                is_author,
            });
        }

        let classes = detached_scan.finish();
        Ok(Self {
            entities,
            timeline: Timeline {
                detached_turrets: classes.records,
                detached_turret_eids: classes.detached_eids,
                vehicle_eids: classes.vehicle_eids,
                poses: shared.st10,
                turret: shared.prop2,
                hp_events: shared.hp_events,
                initial_hp: shared.initial_hp,
                hp_series,
                deaths,
                death_records,
                resurrections,
                kill_feed,
                presence,
                hit_notices,
                prop3_health,
                counters,
                damage_progress,
                periods,
                shots,
                shots_from_loose_path: collected.from_loose_path,
                entity_names: ct.entity_names,
                author_eid,
                supremacy_bases,
                supremacy_points,
                assault_objective_present,
                assault_bases,
                consumables,
                module_crew_states,
                destructible_areas,
                destructible_events,
                reloads,
                reload_effective,
            },
            packet_histogram,
        })
    }

    /// 击杀事件流：每次死亡 × 击杀播报归属增强（|Δt| ≤ 5s 门控隔离开局初始化记录）。
    /// 全场回放切面按候选车集过滤后取用；评审切面取全量。
    /// 普通回放每车至多一次死亡、一条播报——与既往「victim→单条」行为逐位一致；
    /// 复活模式同一受害者可多条播报，按就近未消费配对（一条播报只归属一次死亡）。
    pub fn kill_events(&self) -> Vec<KillEvent> {
        let mut feed_by_victim: BTreeMap<u32, Vec<&KillFeedEvent>> = BTreeMap::new();
        for k in &self.timeline.kill_feed {
            feed_by_victim.entry(k.victim_eid).or_default().push(k);
        }
        let mut out = Vec::new();
        for (eid, records) in &self.timeline.death_records {
            let Some(cands) = feed_by_victim.get(eid) else {
                for d in records {
                    out.push(self.kill_event_of(d.t, d, eid, &None));
                }
                continue;
            };
            let mut used = vec![false; cands.len()];
            for d in records {
                let mut best: Option<(f32, usize)> = None;
                for (i, k) in cands.iter().enumerate() {
                    let dt = (d.t - k.clock).abs();
                    if dt <= 5.0 && best.is_none_or(|(bd, _)| dt < bd) {
                        best = Some((dt, i));
                    }
                }
                let wf = best.map(|(_, i)| {
                    used[i] = true;
                    cands[i]
                });
                out.push(self.kill_event_of(d.t, d, eid, &wf));
            }
        }
        out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        out
    }

    /// 单条死亡记录 → 击杀事件（凶手/死因归属与既往单死语义逐位一致）。
    fn kill_event_of(
        &self,
        t: f32,
        d: &DeathRecord,
        eid: &u32,
        wf: &Option<&KillFeedEvent>,
    ) -> KillEvent {
        KillEvent {
            t: r2(t),
            killer_eid: if d.killer_eid != 0 {
                d.killer_eid
            } else {
                wf.map(|k| k.killer_eid).unwrap_or(0)
            },
            victim_eid: *eid,
            // cause：method1 权威值仅在血量链 killer_eid 非零时可信；击杀播报只给
            // 凶手身份、给不出 method1 cause——未获取一律 255（KillEvent.cause 契约
            // 哨兵），不再合成 0/3（fail-closed，禁猜）
            cause: if d.killer_eid != 0 { d.cause } else { 255 },
            assister_eid: wf.and_then(|k| k.assister_eid),
            death_reason: wf.and_then(|k| k.death_reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unicode 昵称端到端 JOIN 回归：
    /// type5 中文昵称实体 × 中文昵称花名册 → account_id/team/tank_id 联上、
    /// 作者实体可解析（strict 射击路径的前提）。
    #[test]
    fn unicode_nickname_joins_roster() {
        let nick = "兰亭公子苏";
        // 最小 type5：eid@[0..4]、满血锚点@51、昵称块@57（[len][bytes@58..]）
        let body = nick.as_bytes();
        let mut p = vec![0u8; 60.max(58 + body.len())];
        p[0..4].copy_from_slice(&0x21u32.to_le_bytes());
        p[51..53].copy_from_slice(&1000u16.to_le_bytes());
        p[57] = body.len() as u8;
        p[58..58 + body.len()].copy_from_slice(body);
        let packets: Vec<(u32, f32, &[u8])> = vec![(5, 1.0, &p)];
        let roster = vec![PlaybackPlayer {
            account_id: 42,
            nickname: nick.to_string(),
            team: 2,
            tank_id: 30085,
        }];
        let limits = GunPitchLimits::new();
        let model = ReplayModel::scan(&ScanInput {
            packets: &packets,
            roster: &roster,
            author_account_id: 42,
            pitch_limits: &limits,
        })
        .unwrap();
        assert_eq!(
            model.timeline.author_eid, 0x21,
            "中文昵称作者实体必须可解析"
        );
        let e = model.entities.iter().find(|e| e.eid == 0x21).unwrap();
        assert_eq!(e.nickname.as_deref(), Some(nick));
        assert_eq!(e.account_id, Some(42), "昵称联表 account_id");
        assert_eq!(e.team, Some(2), "昵称联表 team");
        assert_eq!(e.tank_id, Some(30085), "昵称联表 tank_id");
        assert!(e.is_author, "作者标记");
    }

    /// 合成血量链：锚点 → 掉血 → 归零（击杀者 7/cause 0）+ 同值重复事件（应去重，
    /// 不改写击杀者）→ 死亡终态与去重序列正确。
    #[test]
    fn hp_series_and_death_record() {
        // 直接测 scan 太重（需完整包流）；这里测血量链+死亡的核心不变式：
        // 构造最小包流（仅 method1 血量事件 + prop1 死亡），scan 不应 panic
        // 且 deaths/hp_series 语义正确。type=8 method1 帧：[eid][mid=1][alen=7][hp u16][source u32][cause]
        let mk_hp = |eid: u32, hp: u16, source: u32, cause: u8| {
            let mut p = vec![0u8; 12];
            p[0..4].copy_from_slice(&eid.to_le_bytes());
            p[4..8].copy_from_slice(&1u32.to_le_bytes());
            p[8..12].copy_from_slice(&7u32.to_le_bytes());
            p.extend_from_slice(&hp.to_le_bytes());
            p.extend_from_slice(&source.to_le_bytes());
            p.push(cause);
            p
        };
        // prop1 死亡广播：type=7 [eid][sub=1]
        let mk_death = |eid: u32| {
            let mut p = vec![0u8; 8];
            p[0..4].copy_from_slice(&eid.to_le_bytes());
            p[4..8].copy_from_slice(&1u32.to_le_bytes());
            p
        };
        let victim = 0x11u32;
        let p_anchored = mk_hp(victim, 1000, 0x22, 0); // 无锚点实体的首事件：掉到 1000，来源 0x22
        let p_kill = mk_hp(victim, 0, 7, 0);
        let p_dup = mk_hp(victim, 0, 9, 1); // 同值 0 重复：去重，不改写击杀者
        let p_death = mk_death(victim);
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (8, 10.0, &p_anchored),
            (8, 20.0, &p_kill),
            (8, 21.0, &p_dup),
            (7, 21.5, &p_death),
        ];
        let roster: Vec<PlaybackPlayer> = Vec::new();
        let limits = GunPitchLimits::new();
        let model = ReplayModel::scan(&ScanInput {
            packets: &packets,
            roster: &roster,
            author_account_id: 0,
            pitch_limits: &limits,
        })
        .unwrap();

        let series = &model.timeline.hp_series[&victim];
        assert_eq!(
            series,
            &vec![(10.0, 1000), (20.0, 0)],
            "锚点缺失从首事件起链，同值去重"
        );
        let d = &model.timeline.deaths[&victim];
        assert!((d.t - 21.5).abs() < 1e-6, "死亡时刻 = prop1 广播");
        assert_eq!(d.killer_eid, 7, "击杀者来自有效 hp==0 事件，重复事件不改写");
        assert_eq!(d.cause, 0);

        let kills = model.kill_events();
        assert_eq!(kills.len(), 1);
        assert_eq!(kills[0].killer_eid, 7);
        assert_eq!(kills[0].cause, 0);
        assert_eq!(kills[0].victim_eid, victim);
    }

    // —— 复活模式语义（issue #622）：多次死亡 + 复活轨道（合成帧锁定，正例待真实样本
    //    验证字节布局；此处锁定的是模型对既有证据语义的归一规则，全部统一适用） ——

    /// type=8 method1 血量帧：[eid][mid=1][alen=7][hp u16][source u32][cause]
    fn mk_hp(eid: u32, hp: u16, source: u32, cause: u8) -> Vec<u8> {
        let mut p = vec![0u8; 12];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[4..8].copy_from_slice(&1u32.to_le_bytes());
        p[8..12].copy_from_slice(&7u32.to_le_bytes());
        p.extend_from_slice(&hp.to_le_bytes());
        p.extend_from_slice(&source.to_le_bytes());
        p.push(cause);
        p
    }
    /// type=7 sub=1 prop1 死亡广播：[eid][sub=1]
    fn mk_death_broadcast(eid: u32) -> Vec<u8> {
        let mut p = vec![0u8; 8];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[4..8].copy_from_slice(&1u32.to_le_bytes());
        p
    }
    /// type=7 sub=3 prop3 血量广播：[eid][sub=3][pad4][hp u16@12]
    fn mk_prop3(eid: u32, hp: u16) -> Vec<u8> {
        let mut p = vec![0u8; 14];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[4..8].copy_from_slice(&3u32.to_le_bytes());
        p[12..14].copy_from_slice(&hp.to_le_bytes());
        p
    }
    /// type=5 满血锚点：[eid][…][hp u16@51]
    fn mk_anchor(eid: u32, hp: u16) -> Vec<u8> {
        let mut p = vec![0u8; 53];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[51..53].copy_from_slice(&hp.to_le_bytes());
        p
    }
    fn scan_min(packets: Vec<(u32, f32, Vec<u8>)>) -> ReplayModel {
        let refs: Vec<(u32, f32, &[u8])> = packets
            .iter()
            .map(|(t, c, p)| (*t, *c, p.as_slice()))
            .collect();
        let roster: Vec<PlaybackPlayer> = Vec::new();
        ReplayModel::scan(&ScanInput {
            packets: &refs,
            roster: &roster,
            author_account_id: 0,
            pitch_limits: &GunPitchLimits::new(),
        })
        .unwrap()
    }

    /// 死→复活→再死：死亡列表两条（各带自己的凶手/死因，不再逐死覆盖）、
    /// 复活锚点一条、`deaths` 仍为首次死亡（既有消费方口径）。
    #[test]
    fn respawn_multi_death_track() {
        let a = 0x11u32;
        let model = scan_min(vec![
            (5, 0.5, mk_anchor(a, 1000)),
            (8, 10.0, mk_hp(a, 0, 7, 0)), // 第一次死亡：击杀者 7
            (7, 11.0, mk_death_broadcast(a)),
            (8, 15.0, mk_hp(a, 1500, 0, 0)), // 复活：血量回归正值
            (8, 20.0, mk_hp(a, 0, 9, 1)),    // 第二次死亡：击杀者 9
            (7, 21.0, mk_death_broadcast(a)),
        ]);
        let rs = &model.timeline.death_records[&a];
        assert_eq!(rs.len(), 2, "两次死亡各成一条记录");
        assert_eq!(rs[0].t, 11.0);
        assert_eq!(
            rs[0].killer_eid, 7,
            "各死各配，第一次死亡凶手不被第二次覆盖"
        );
        assert_eq!(rs[0].cause, 0);
        assert_eq!(rs[1].t, 21.0);
        assert_eq!(rs[1].killer_eid, 9);
        assert_eq!(rs[1].cause, 1);
        assert_eq!(model.timeline.deaths[&a].t, 11.0, "deaths = 首次死亡");
        assert_eq!(
            model.timeline.resurrections.get(&a).map(|v| v.as_slice()),
            Some(&[15.0][..]),
            "死亡未闭合期的血量回归正值 = 复活锚点"
        );
        let kills = model.kill_events();
        assert_eq!(kills.len(), 2, "每次死亡各产一条击杀事件");
        assert_eq!((kills[0].t, kills[0].killer_eid), (11.0, 7));
        assert_eq!((kills[1].t, kills[1].killer_eid), (21.0, 9));
    }

    /// 作者车形态：method1 全缺、死亡只有 prop1 广播、复活证据只走 prop3——
    /// prop3 归零不立死（死亡语义不变），死亡未闭合期的 prop3 回归正值立复活。
    #[test]
    fn resurrection_via_prop3_only() {
        let b = 0x22u32;
        let model = scan_min(vec![
            (5, 0.5, mk_anchor(b, 1750)),
            (7, 10.0, mk_death_broadcast(b)), // 死亡（广播单证）
            (7, 12.0, mk_prop3(b, 0)),        // prop3 终态：不立死、不立复活
            (7, 14.0, mk_prop3(b, 1750)),     // prop3 回归正值：复活
        ]);
        let rs = &model.timeline.death_records[&b];
        assert_eq!(rs.len(), 1, "prop3 归零不立死，死亡仅来自广播");
        assert_eq!(rs[0].t, 10.0);
        assert_eq!(rs[0].killer_eid, 0, "无 method1 归零事件 → 无凶手归属");
        assert_eq!(
            model.timeline.resurrections.get(&b).map(|v| v.as_slice()),
            Some(&[14.0][..]),
            "复活证据经 prop3 载体可达"
        );
    }

    /// 普通战斗 fail-closed：单死、无任何本车回归证据 → 复活轨道为空（消费方按
    /// 死亡终态语义处理，行为与既往一致）；他人存活期的 prop3 广播不得越车生效。
    #[test]
    fn normal_battle_has_empty_resurrection_track() {
        let c = 0x33u32;
        let d = 0x34u32;
        let model = scan_min(vec![
            (5, 0.5, mk_anchor(c, 1000)),
            (8, 30.0, mk_hp(c, 0, 5, 0)),
            (7, 31.0, mk_death_broadcast(c)),
            // 他人（存活）的 prop3 正值广播：不得为已死者 c 立复活
            (7, 32.0, mk_prop3(d, 900)),
        ]);
        assert_eq!(model.timeline.death_records[&c].len(), 1);
        assert!(
            !model.timeline.resurrections.contains_key(&c),
            "无本车生命回归证据 → 复活轨道为空（fail-closed）"
        );
        assert!(!model.timeline.death_records.contains_key(&d));
        assert!(!model.timeline.resurrections.contains_key(&d));
    }

    /// 多证据配对：广播多于归零事件时，落单广播按自身证据立死（无凶手归属），
    /// 已配对者取广播时刻 + 归零事件凶手。
    #[test]
    fn unpaired_broadcast_stands_alone() {
        let e = 0x35u32;
        let model = scan_min(vec![
            (5, 0.5, mk_anchor(e, 1000)),
            (8, 40.0, mk_hp(e, 0, 3, 2)),
            (7, 40.5, mk_death_broadcast(e)), // 配对 40.0 的归零事件
            (7, 46.0, mk_death_broadcast(e)), // 落单广播（5s 门控内无可配归零）
        ]);
        let rs = &model.timeline.death_records[&e];
        assert_eq!(rs.len(), 2);
        assert_eq!((rs[0].t, rs[0].killer_eid, rs[0].cause), (40.5, 3, 2));
        assert_eq!((rs[1].t, rs[1].killer_eid, rs[1].cause), (46.0, 0, 255));
        assert!(!model.timeline.resurrections.contains_key(&e));
    }

    // —— 隐藏段掉血兑现（重入 Type5 物化快照血量）：盲炮/视野外伤害没有 method1
    //    事件，重入快照的当前血量是唯一绝对证据，二次点亮时必须并入血量链 ——

    /// type=5 战斗车辆物化包：[eid][entityTypeId=2 @4..6][…][hp u16 @51]
    fn mk_mat(eid: u32, hp: u16) -> Vec<u8> {
        let mut p = vec![0u8; 53];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[4..6].copy_from_slice(&2u16.to_le_bytes());
        p[51..53].copy_from_slice(&hp.to_le_bytes());
        p
    }
    /// Type33 预备帧 / Type4 离开帧：[eid]
    fn mk_aoi(eid: u32) -> Vec<u8> {
        eid.to_le_bytes().to_vec()
    }

    #[test]
    fn hp_series_folds_hidden_period_damage_on_reentry() {
        let v = 0x41u32;
        let model = scan_min(vec![
            (33, 0.5, mk_aoi(v)),
            (5, 1.0, mk_mat(v, 1000)),     // 首物化 = 满血锚点（同包双证据）
            (8, 5.0, mk_hp(v, 800, 7, 0)), // 在场段受击
            (4, 6.0, mk_aoi(v)),           // 丢视野
            // 隐藏段盲炮掉血 800→650：Type4 隐藏段零更新，无任何包
            (33, 9.0, mk_aoi(v)),
            (5, 9.5, mk_mat(v, 650)),      // 二次点亮物化：当前血量 650
            (8, 12.0, mk_hp(v, 400, 8, 0)), // 再次在场受击
            (4, 13.0, mk_aoi(v)),
        ]);
        assert_eq!(
            &model.timeline.hp_series[&v],
            &vec![(1.0, 1000), (5.0, 800), (9.5, 650), (12.0, 400)],
            "重入物化血量在 t_in 兑现，链不停在丢视野前旧值"
        );
    }

    #[test]
    fn hp_series_reentry_without_hidden_change_stays_deduped() {
        let v = 0x42u32;
        let model = scan_min(vec![
            (33, 0.5, mk_aoi(v)),
            (5, 1.0, mk_mat(v, 1000)),
            (8, 5.0, mk_hp(v, 800, 7, 0)),
            (4, 6.0, mk_aoi(v)),
            (33, 9.0, mk_aoi(v)),
            (5, 9.5, mk_mat(v, 800)), // 隐藏段无掉血：快照 = 丢视野前值
            (8, 12.0, mk_hp(v, 400, 8, 0)),
        ]);
        assert_eq!(
            &model.timeline.hp_series[&v],
            &vec![(1.0, 1000), (5.0, 800), (12.0, 400)],
            "无隐藏段变化不产生新事件（去重序列契约）"
        );
    }

    #[test]
    fn hp_series_reentry_terminal_snapshot_normalizes_to_zero() {
        let v = 0x43u32;
        let model = scan_min(vec![
            (33, 0.5, mk_aoi(v)),
            (5, 1.0, mk_mat(v, 1000)),
            (8, 5.0, mk_hp(v, 800, 7, 0)),
            (4, 6.0, mk_aoi(v)),
            // 隐藏段死亡：重入物化为终态哨兵
            (33, 9.0, mk_aoi(v)),
            (5, 9.5, mk_mat(v, 0xFFFD)),
        ]);
        assert_eq!(
            &model.timeline.hp_series[&v],
            &vec![(1.0, 1000), (5.0, 800), (9.5, 0)],
            "终态哨兵归一与 method1 显示口径同判（钳 0）"
        );
    }

    #[test]
    fn hp_series_starts_from_first_materialization_when_seed_missing() {
        let v = 0x44u32;
        let short = v.to_le_bytes().to_vec(); // <53B：无锚点（首物化短包）
        let model = scan_min(vec![
            (33, 0.5, mk_aoi(v)),
            (5, 1.0, short), // 短包：initial_hp 不收、presence hp_raw=None
            (4, 6.0, mk_aoi(v)),
            (33, 9.0, mk_aoi(v)),
            (5, 9.5, mk_mat(v, 650)), // 重入快照是唯一血量证据
        ]);
        assert_eq!(
            &model.timeline.hp_series[&v],
            &vec![(9.5, 650)],
            "无锚点实体以首个带血量物化快照为链起点（有证据不丢弃）"
        );
    }
}
