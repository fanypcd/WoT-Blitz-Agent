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
    self, AoiPresence, ArenaPeriod, AssaultBaseStateTransition, CombatEventType, CombatTimeline,
    ConsumableTransition, FeedbackCounterEvent, GunPitchLimits, HpEvent, KillFeedEvent,
    ModuleCrewStateEvent, RawReloadDuration, RawReloadPhase, ShotReplayData, St10Sample,
    SupremacyBaseStateTransition, SupremacyPointsSample,
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

/// 死亡终态：prop1 死亡广播时刻 + 血量链 hp==0 事件的击杀者/死因
/// （同值去重语义下有效 hp==0 事件唯一；killer_eid = 0 表示无血量链归属/环境）
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
    /// 死亡终态（有 prop1 死亡广播或血量链归零的实体）
    pub deaths: BTreeMap<u32, DeathRecord>,
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
    /// 实时装填相位（arena subtype 15/17，**仅本方全队**；相位码 f2 与计数 f4 原样透传）
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
        for (t, _, _) in packets {
            *packet_histogram.entry(*t).or_insert(0) += 1;
        }

        let author_nickname = input
            .roster
            .iter()
            .find(|p| p.account_id == input.author_account_id)
            .map(|p| p.nickname.clone())
            .unwrap_or_default();
        let author_eid = combat::resolve_author_player_eid_by_nick(packets, &author_nickname);

        // —— 共享扫描（契约"包流只扫一遍"）：位姿/炮塔/血量/名字/配件/发射等
        // 全部索引只建一份，弹道两路提取（collect_all_shots）与实体档案并表均从此取数。
        let shared = combat::build_shot_scan_shared(packets, author_eid);

        let ct = CombatTimeline::parse_packets(packets);
        let presence = combat::collect_aoi_lifecycle(packets);
        let hit_notices = combat::collect_hit_notices(packets);
        let prop3_health = combat::collect_prop3_health(packets);
        let counters = combat::collect_feedback_counters(packets);
        // arena 流一次收集 {1,3,6}（comps/periods/kill_feed 三个消费方合用；
        // 高频 RELOAD_TIME 等子类型在收集期即丢弃）
        let arena_updates = combat::collect_arena_updates_filtered(packets, |s| {
            s == 1 || s == 3 || s == 6
            // 装填相位（subtype 15/17）也在此收集：本方全队的装填开始/就绪/弹夹内间隔
            || s == combat::ARENA_SUB_RELOAD_TIME
            || s == combat::ARENA_SUB_RELOAD_TIME_UPDATE
            || s == combat::ARENA_SUB_RELOAD_TIME_LIST
        });
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
        let hp_events = &shared.hp_events;
        let initial_hp = &shared.initial_hp;
        let equipment = &shared.vehicle_equipment;
        let loadouts = combat::collect_vehicle_loadout(packets);

        // —— 血量链去重序列 + 死亡终态（一次遍历，语义与原逐车组装逐位一致） ——
        let mut hp_series: BTreeMap<u32, Vec<(f32, u16)>> = BTreeMap::new();
        for (eid, (t0, h0)) in initial_hp.iter() {
            hp_series.entry(*eid).or_default().push((*t0, *h0));
        }
        let mut deaths: BTreeMap<u32, DeathRecord> = BTreeMap::new();
        for (t, eid, _) in ct.death_events() {
            deaths.entry(eid).or_insert(DeathRecord {
                t,
                killer_eid: 0,
                cause: 255,
            });
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
                let d = deaths.entry(e.victim).or_insert(DeathRecord {
                    t: e.clock,
                    killer_eid: 0,
                    cause: 255,
                });
                d.killer_eid = e.source;
                d.cause = e.cause;
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

        // —— 弹道（作者严格 + 他人宽松合并；降级策略见 playback::collect_all_shots） ——
        let shots = playback::collect_all_shots(&shared, author_eid, input.pitch_limits)?;

        // —— 实体档案并表（eid 并集 = 位姿 ∪ 炮塔 ∪ 名字 ∪ 锚点 ∪ 配件 ∪ 在场） ——
        let valid_tanks: Vec<u32> = input.roster.iter().map(|p| p.tank_id).collect();
        let comps = playback::comp_descriptors_from_updates(&arena_updates, &valid_tanks);
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

        let mut entities = Vec::with_capacity(eids.len());
        for eid in eids {
            let nickname = ct.entity_names.get(&eid).cloned();
            let joined = nickname
                .as_ref()
                .and_then(|n| input.roster.iter().find(|p| &p.nickname == n));
            // 组成 blob 昵称是原始 UTF-8（不经 ascii 过滤），与 type=5 名字表对非 ascii
            // 昵称可能不交集——按昵称能联则联，联不上不猜
            let comp = nickname.as_ref().and_then(|n| comps.get(n));
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

        Ok(Self {
            entities,
            timeline: Timeline {
                poses: shared.st10,
                turret: shared.prop2,
                hp_events: shared.hp_events,
                initial_hp: shared.initial_hp,
                hp_series,
                deaths,
                kill_feed,
                presence,
                hit_notices,
                prop3_health,
                counters,
                damage_progress,
                periods,
                shots,
                entity_names: ct.entity_names,
                author_eid,
                supremacy_bases,
                supremacy_points,
                assault_objective_present,
                assault_bases,
                consumables,
                module_crew_states,
                reloads,
                reload_effective,
            },
            packet_histogram,
        })
    }

    /// 击杀事件流：死亡终态 × 击杀播报归属增强（|Δt| ≤ 5s 门控隔离开局初始化记录）。
    /// 全场回放切面按候选车集过滤后取用；评审切面取全量。
    pub fn kill_events(&self) -> Vec<KillEvent> {
        let feed: HashMap<u32, &KillFeedEvent> = self
            .timeline
            .kill_feed
            .iter()
            .filter(|k| {
                self.timeline
                    .deaths
                    .get(&k.victim_eid)
                    .map(|d| (d.t - k.clock).abs() <= 5.0)
                    .unwrap_or(false)
            })
            .map(|k| (k.victim_eid, k))
            .collect();
        let mut out = Vec::new();
        for (eid, d) in &self.timeline.deaths {
            let wf = feed.get(eid);
            out.push(KillEvent {
                t: r2(d.t),
                killer_eid: if d.killer_eid != 0 {
                    d.killer_eid
                } else {
                    wf.map(|k| k.killer_eid).unwrap_or(0)
                },
                victim_eid: *eid,
                cause: if d.killer_eid != 0 {
                    d.cause
                } else if wf.is_some() {
                    0
                } else {
                    3
                },
                assister_eid: wf.and_then(|k| k.assister_eid),
                death_reason: wf.and_then(|k| k.death_reason),
            });
        }
        out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        out
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
}
