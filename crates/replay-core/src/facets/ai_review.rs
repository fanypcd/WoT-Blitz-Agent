//! 智能体评审切面（架构契约第 5/7 节）：花名册 + 归一化事件流 + 结算锚点
//! → JSON → WotBTools Java → 大语言模型。
//!
//! 与回放切面共享同一 Rust 核心但 DTO 不同：评审切面按事件语义组织（带类型标签），
//! 不含渲染网格；喂给模型的粒度（降采样/摘要）由下游编排决定，本层保持权威全量。
//! 已知边界如实透出：队友点亮的事件级归属不在回放中（仅结算总量），可见性窗口
//! 是本队视角（AoI 生命周期）。

use serde::Serialize;

use crate::models::battle::BattleSummary;
use crate::models::replay_dataset::{PlayerSettlementRow, ReplayDataset};
use crate::replay::combat::ArenaPeriod;
use crate::replay::model::{EntityRecord, ReplayModel};

/// 智能体评审切面（单场）
#[derive(Debug, Clone, Serialize)]
pub struct AiReviewFacet {
    /// 契约版本（当前 1；不兼容变更递增）
    pub version: u32,
    pub battle: AiBattleHeader,
    pub rosters: Vec<AiRosterEntry>,
    /// 按回放时钟升序的归一化事件流
    pub events: Vec<AiEvent>,
    /// 结算锚点（模型结论与过程互验用；行结构见 ReplayDataset 阶段 1 契约）
    pub settlements: Vec<PlayerSettlementRow>,
    /// 原始世界位姿观测（type=10，**未滤波**；attachmentParent≠0 的挂接局部变换不是世界坐标，
    /// 不收）。回放切面的 0.1 s 网格是渲染滤波输出——AoI 重入后有收敛滞后，不能当位置证据。
    pub poses: Vec<RawPoseTrack>,
    /// 原始炮塔属性广播（type=7 prop2 u16，未解码；高 10 位 = 相对偏航 coarse、低 6 位 = 俯仰比例）
    pub turrets: Vec<RawTurretTrack>,
}

/// 单实体原始位姿观测（列式；各列等长、按包序 = 时钟序）
#[derive(Debug, Clone, Serialize)]
pub struct RawPoseTrack {
    pub eid: u32,
    pub t: Vec<f32>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub z: Vec<f32>,
    /// 车体偏航（rad，原始域，不解卷绕）
    pub yaw: Vec<f32>,
}

/// 单实体原始 prop2 观测（列式）
#[derive(Debug, Clone, Serialize)]
pub struct RawTurretTrack {
    pub eid: u32,
    pub t: Vec<f32>,
    pub raw: Vec<u16>,
}

/// type=10 原始世界位姿 + type=7 prop2 原始值（按 eid 升序、每实体包序）。
/// 载荷：type10 `[eid][spaceId][attachmentParent][x y z][posError×3][yaw pitch roll]`（≥48）。
pub fn collect_raw_tracks(
    packets: &[(u32, f32, &[u8])],
) -> (Vec<RawPoseTrack>, Vec<RawTurretTrack>) {
    use std::collections::BTreeMap;
    let mut poses: BTreeMap<u32, RawPoseTrack> = BTreeMap::new();
    let mut turrets: BTreeMap<u32, RawTurretTrack> = BTreeMap::new();
    for (ptype, clock, p) in packets {
        let eid_of = |p: &[u8]| u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        if *ptype == 10 && p.len() >= 48 {
            if u32::from_le_bytes([p[8], p[9], p[10], p[11]]) != 0 {
                continue;
            }
            let f = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
            let eid = eid_of(p);
            let tr = poses.entry(eid).or_insert_with(|| RawPoseTrack {
                eid,
                t: Vec::new(),
                x: Vec::new(),
                y: Vec::new(),
                z: Vec::new(),
                yaw: Vec::new(),
            });
            tr.t.push(*clock);
            tr.x.push(f(12));
            tr.y.push(f(16));
            tr.z.push(f(20));
            tr.yaw.push(f(36));
        } else if *ptype == 7 && p.len() >= 14 && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2
        {
            let eid = eid_of(p);
            let tr = turrets.entry(eid).or_insert_with(|| RawTurretTrack {
                eid,
                t: Vec::new(),
                raw: Vec::new(),
            });
            tr.t.push(*clock);
            tr.raw.push(u16::from_le_bytes([p[12], p[13]]));
        }
    }
    (
        poses.into_values().collect(),
        turrets.into_values().collect(),
    )
}

/// 战斗头
#[derive(Debug, Clone, Serialize)]
pub struct AiBattleHeader {
    /// 战斗开始 Unix 秒
    pub start_time: i64,
    pub map_id: u32,
    pub map_name: String,
    pub room_type: String,
    pub winner: u8,
    /// 结算口径整秒时长（root5 尚未解码 → null；绝不以 meta 口径冒充——unknown ≠ 0）
    pub duration_secs: Option<u32>,
    /// 元数据口径时长（meta.json battleDuration；缺失/0 → None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_duration_secs: Option<f64>,
    /// 战局阶段（准备/倒计时/战斗）
    pub periods: Vec<ArenaPeriod>,
}

/// 花名册行（实体 ↔ 结算联表结果）
#[derive(Debug, Clone, Serialize)]
pub struct AiRosterEntry {
    /// 回放实体 id（0 = 该实体未在包流出现）
    pub eid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tank_id: Option<u32>,
    pub tank_name: String,
    pub is_author: bool,
}

/// 归一化事件（serde 内部标签；t = 回放时钟秒）
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AiEvent {
    /// 满血锚点（血量链起点）
    Spawn { t: f32, eid: u32, max_hp: u16 },
    /// 开火 + 结果（全玩家；结果字段语义与回放切面弹道一致）
    Shot {
        t: f32,
        shooter_eid: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        target_eid: Option<u32>,
        hit: bool,
        ricochet: bool,
        /// 游戏命中结果枚举（0=无 1=未击穿 2=间隙止 3=有伤害 4=履带/模块 255=未获取）
        game_hit_result: u8,
        damage: u32,
        is_kill: bool,
        is_author: bool,
        #[serde(skip_serializing_if = "String::is_empty")]
        shell_kind: String,
    },
    /// 血量变化事件（method1；hp = 事件后绝对血量，overkill/哨兵钳 0）。
    /// `hp_raw` = 未钳制的原始 u16（0x0000 / 0xFFFD / 0xFFFF / 0xFFFE 等终态哨兵族原样保留），
    /// 供消费方按自己的口径区分「确知 HP=0」与「终态哨兵（血量未知）」——`hp` 的钳 0
    /// 只是显示便利，不是血量事实。
    Damage {
        t: f32,
        victim_eid: u32,
        hp: u16,
        hp_raw: u16,
        source_eid: u32,
        cause: u8,
    },
    /// 击毁（击杀播报归属增强：击杀者/死因/≥50% 助攻）
    Kill {
        t: f32,
        killer_eid: u32,
        victim_eid: u32,
        cause: u8,
        #[serde(skip_serializing_if = "Option::is_none")]
        assister_eid: Option<u32>,
    },
    /// 可见性窗口（AoI 生命周期，本队视角；t_out = None 表示战斗结束仍在场）。
    /// `hp_raw` = 开段 Type5 物化快照的原始 HP u16（战斗车辆；每次重入都有，见 `AoiPresence`）
    Visibility {
        t_in: f32,
        eid: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        t_out: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        hp_raw: Option<u16>,
    },
    /// prop3（type=7 sub=3）血量属性广播：原始 u16（哨兵族原样）。与 `Damage`（method1）
    /// 互补而非重复——录像者自身车辆的血量变化常只有这一路。
    Health { t: f32, eid: u32, hp_raw: u16 },
    /// method8 伤害/命中反馈通知（原始全变体，不分类；字段缺失 = 载荷不足）。
    /// eid = envelope 方法调用目标实体；分类口径（直击 / 未解码变体 / 短体）属于消费方。
    HitNotice {
        t: f32,
        eid: u32,
        payload_len: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        shooter_eid: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        victim_eid: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        secondary: Option<u8>,
    },
    /// 作者战斗反馈计数（0x0c；code 语义见 combat::feedback_code）
    Counter {
        t: f32,
        code: u8,
        count: u16,
        value: u16,
    },
    /// 累计伤害进度（prop10；相邻差 = 区段内伤害）
    DamageTick { t: f32, eid: u32, cumulative: u32 },
}

impl AiEvent {
    fn t(&self) -> f32 {
        match self {
            AiEvent::Spawn { t, .. }
            | AiEvent::Shot { t, .. }
            | AiEvent::Damage { t, .. }
            | AiEvent::Kill { t, .. }
            | AiEvent::HitNotice { t, .. }
            | AiEvent::Health { t, .. }
            | AiEvent::Counter { t, .. }
            | AiEvent::DamageTick { t, .. } => *t,
            AiEvent::Visibility { t_in, .. } => *t_in,
        }
    }
}

impl AiReviewFacet {
    /// 从内部模型 + 结算联表投影。
    /// 投影 + 原始位姿/炮塔流（`packets` 与建模所用同一包流）。
    pub fn from_model_with_packets(
        model: &ReplayModel,
        summary: &BattleSummary,
        packets: &[(u32, f32, &[u8])],
    ) -> Self {
        let mut facet = Self::from_model(model, summary);
        let (poses, turrets) = collect_raw_tracks(packets);
        facet.poses = poses;
        facet.turrets = turrets;
        facet
    }

    pub fn from_model(model: &ReplayModel, summary: &BattleSummary) -> Self {
        let mut events: Vec<AiEvent> = Vec::new();

        // 花名册先行：可见性事件只保留可联表到花名册的车辆实体——原始 AoI 流含
        // 未证明实体类型的 EID（投影物/特效物化等），不得作为裸 EID 泄漏进评审切面
        //（playback 切面保留完整 AoI 粒度，不受此约束）。
        // 花名册只收有身份的实体（昵称/结算联表命中）。
        let tank_name_of = |e: &EntityRecord| -> String {
            e.account_id
                .and_then(|aid| summary.players.iter().find(|p| p.account_id == aid))
                .map(|p| p.tank_name.clone())
                .unwrap_or_default()
        };
        let rosters: Vec<AiRosterEntry> = model
            .entities
            .iter()
            .filter(|e| e.nickname.is_some() || e.account_id.is_some())
            .map(|e| AiRosterEntry {
                eid: e.eid,
                account_id: e.account_id,
                nickname: e.nickname.clone(),
                team: e.team,
                tank_id: e.tank_id,
                tank_name: tank_name_of(e),
                is_author: e.is_author,
            })
            .collect();
        let roster_eids: std::collections::HashSet<u32> = rosters.iter().map(|r| r.eid).collect();

        for (eid, (t, hp)) in &model.timeline.initial_hp {
            events.push(AiEvent::Spawn {
                t: *t,
                eid: *eid,
                max_hp: *hp,
            });
        }
        for e in &model.timeline.hp_events {
            // overkill 负值钳 0（与回放切面同式）
            let hp_v = if e.hp > 32767 { 0 } else { e.hp };
            events.push(AiEvent::Damage {
                t: e.clock,
                victim_eid: e.victim,
                hp: hp_v,
                hp_raw: e.hp,
                source_eid: e.source,
                cause: e.cause,
            });
        }
        for k in model.kill_events() {
            events.push(AiEvent::Kill {
                t: k.t,
                killer_eid: k.killer_eid,
                victim_eid: k.victim_eid,
                cause: k.cause,
                assister_eid: k.assister_eid,
            });
        }
        for p in &model.timeline.presence {
            if roster_eids.contains(&p.eid) {
                events.push(AiEvent::Visibility {
                    t_in: p.t_in,
                    eid: p.eid,
                    t_out: p.t_out,
                    hp_raw: p.hp_raw,
                });
            }
        }
        for h in &model.timeline.prop3_health {
            events.push(AiEvent::Health {
                t: h.clock,
                eid: h.eid,
                hp_raw: h.hp_raw,
            });
        }
        for h in &model.timeline.hit_notices {
            events.push(AiEvent::HitNotice {
                t: h.clock,
                eid: h.eid,
                payload_len: h.payload_len,
                shooter_eid: h.shooter_eid,
                victim_eid: h.victim_eid,
                result: h.result,
                secondary: h.secondary,
            });
        }
        for c in &model.timeline.counters {
            events.push(AiEvent::Counter {
                t: c.clock,
                code: c.event_code,
                count: c.count,
                value: c.value,
            });
        }
        for (eid, series) in &model.timeline.damage_progress {
            for (t, cum) in series {
                events.push(AiEvent::DamageTick {
                    t: *t,
                    eid: *eid,
                    cumulative: *cum,
                });
            }
        }
        for s in &model.timeline.shots {
            // 受击方身份直接用弹道自带的 target_eid（作者 = method38 受击者、他人 =
            // method8 直击通知，均为服务器权威）——eid 是身份域，名字仅显示域，
            // 本切面不做昵称反查。名字缺失不作为提取失败条件。
            let target_eid = s.target_eid;
            events.push(AiEvent::Shot {
                t: s.fire_time,
                shooter_eid: s.shooter_eid,
                target_eid,
                hit: target_eid.is_some(),
                ricochet: s.hit_flags & 0x0008 != 0,
                game_hit_result: s.game_hit_result,
                damage: s.damage,
                is_kill: s.is_kill,
                is_author: s.is_author,
                shell_kind: s.shell_kind.clone(),
            });
        }
        events.sort_by(|a, b| a.t().partial_cmp(&b.t()).unwrap());

        let settlements = ReplayDataset::from_summary(summary).settlement.players;

        Self {
            version: 1,
            battle: AiBattleHeader {
                start_time: summary.timestamp,
                map_id: summary.map_id,
                map_name: summary.map_name.clone(),
                room_type: summary.room_type.clone(),
                winner: summary.winner_team,
                duration_secs: None, // 结算口径 root5 未解码；未知 → null
                meta_duration_secs: (summary.battle_duration_secs > 0.0)
                    .then_some(summary.battle_duration_secs),
                periods: model.timeline.periods.clone(),
            },
            rosters,
            events,
            settlements,
            poses: Vec::new(),
            turrets: Vec::new(),
        }
    }
}

/// 评审切面用的语义标签再导出（编排层映射 code → 中文标签时用，避免魔法数字散落）
pub use crate::replay::combat::feedback_code as counter_codes;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::combat::feedback_code;

    /// 事件排序与来源映射：Counters/可见性/伤害进入统一事件流且按 t 升序；
    /// 可见性事件必须可联表到花名册（无名实体不出现在评审切面）。
    #[test]
    fn events_sorted_and_typed() {
        let summary = BattleSummary::from_naive(1);
        let limits = crate::replay::combat::GunPitchLimits::new();
        // 最小包流：一条 0x0c 点亮计数 + 一条 AoI 进场（Type33→Type5 物化，带合法昵称 "abc"）
        let mut p33 = vec![0u8; 4];
        p33[0..4].copy_from_slice(&0x33u32.to_le_bytes());
        let mut p5 = vec![0u8; 64];
        p5[0..4].copy_from_slice(&0x33u32.to_le_bytes());
        p5[57] = 3; // 昵称长度前缀
        p5[58..61].copy_from_slice(b"abc");
        let mut c = vec![0u8; 12 + 6];
        c[0..4].copy_from_slice(&0x99u32.to_le_bytes());
        c[4..8].copy_from_slice(&0x0Cu32.to_le_bytes());
        c[8..12].copy_from_slice(&6u32.to_le_bytes());
        c[12..].copy_from_slice(&[2, 0, 1, 0, 1, 0]);
        let packets: Vec<(u32, f32, &[u8])> = vec![(8, 5.0, &c), (33, 6.0, &p33), (5, 6.4, &p5)];
        let roster: Vec<crate::replay::playback::PlaybackPlayer> = Vec::new();
        let model = ReplayModel::scan(&crate::replay::model::ScanInput {
            packets: &packets,
            roster: &roster,
            author_account_id: 0,
            pitch_limits: &limits,
        })
        .unwrap();
        let facet = AiReviewFacet::from_model(&model, &summary);

        // 核心不变量：每条 visibility 的 eid 都必须可联表到花名册（裸 EID 不泄漏）。
        // 0x33 带 type=5 昵称 → 有身份 → 进花名册，其可见性窗口保留且可联表。
        let roster_ids: std::collections::HashSet<u32> =
            facet.rosters.iter().map(|r| r.eid).collect();
        assert!(facet.events.iter().all(|e| match e {
            AiEvent::Visibility { eid, .. } => roster_ids.contains(eid),
            _ => true,
        }));
        assert!(facet
            .rosters
            .iter()
            .any(|r| r.eid == 0x33 && r.nickname.as_deref() == Some("abc")));
        assert!(facet.events.iter().any(|e| matches!(e,
            AiEvent::Visibility { eid: 0x33, t_in, .. } if (*t_in - 6.4).abs() < 1e-5)));

        // 花名册联表（结算花名册含该昵称）→ 队伍/车型补全
        let roster2 = vec![crate::replay::playback::PlaybackPlayer {
            account_id: 1,
            nickname: "abc".into(),
            team: 1,
            tank_id: 1001,
        }];
        let model2 = ReplayModel::scan(&crate::replay::model::ScanInput {
            packets: &packets,
            roster: &roster2,
            author_account_id: 0,
            pitch_limits: &limits,
        })
        .unwrap();
        let facet2 = AiReviewFacet::from_model(&model2, &summary);
        assert!(facet2
            .rosters
            .iter()
            .any(|r| r.eid == 0x33 && r.team == Some(1) && r.tank_id == Some(1001)));

        let ts: Vec<f32> = facet2.events.iter().map(|e| e.t()).collect();
        let mut sorted = ts.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(ts, sorted, "事件必须按 t 升序");

        assert!(facet2.events.iter().any(|e| matches!(e,
            AiEvent::Counter { code, count: 1, value: 1, .. } if *code == feedback_code::SPOTTED)));
        // 未知时长 → null（绝不写 0/0.0）
        assert_eq!(facet2.battle.duration_secs, None);
        assert_eq!(facet2.battle.meta_duration_secs, None);
    }

    /// 原始 HP 透传：method1 终态哨兵（0xFFFD）在 `hp` 钳 0、`hp_raw` 原样；
    /// AoI 开段 Type5（战斗车辆 entityTypeId=2）的物化 HP 随 Visibility 透出，重入各段各自携带。
    #[test]
    fn raw_hp_provenance_is_preserved() {
        let summary = BattleSummary::from_naive(1);
        let limits = crate::replay::combat::GunPitchLimits::new();
        let victim = 0x44u32;
        // Type5 战斗车辆物化：entityTypeId=2 @4..6、HP @51..53、昵称 "abc" @57
        let type5 = |hp: u16| {
            let mut p = vec![0u8; 64];
            p[0..4].copy_from_slice(&victim.to_le_bytes());
            p[4..6].copy_from_slice(&2u16.to_le_bytes());
            p[51..53].copy_from_slice(&hp.to_le_bytes());
            p[57] = 3;
            p[58..61].copy_from_slice(b"abc");
            p
        };
        let mut p33 = vec![0u8; 4];
        p33[0..4].copy_from_slice(&victim.to_le_bytes());
        let mut p4 = vec![0u8; 4];
        p4[0..4].copy_from_slice(&victim.to_le_bytes());
        // method1：[eid][mid=1][alen=7][hp u16][source u32][cause]
        let method1 = |hp: u16| {
            let mut p = vec![0u8; 12 + 7];
            p[0..4].copy_from_slice(&victim.to_le_bytes());
            p[4..8].copy_from_slice(&1u32.to_le_bytes());
            p[8..12].copy_from_slice(&7u32.to_le_bytes());
            p[12..14].copy_from_slice(&hp.to_le_bytes());
            p[14..18].copy_from_slice(&0x55u32.to_le_bytes());
            p[18] = 0;
            p
        };
        let (e1, e2, m1, m2) = (type5(2000), type5(1500), method1(1700), method1(0xFFFD));
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (33, 1.0, &p33),
            (5, 1.4, &e1),
            (8, 2.0, &m1),
            (4, 3.0, &p4),
            (33, 9.0, &p33),
            (5, 9.4, &e2),
            (8, 10.0, &m2),
        ];
        let roster: Vec<crate::replay::playback::PlaybackPlayer> = Vec::new();
        let model = ReplayModel::scan(&crate::replay::model::ScanInput {
            packets: &packets,
            roster: &roster,
            author_account_id: 0,
            pitch_limits: &limits,
        })
        .unwrap();
        let facet = AiReviewFacet::from_model(&model, &summary);

        let vis: Vec<(f32, Option<f32>, Option<u16>)> = facet
            .events
            .iter()
            .filter_map(|e| match e {
                AiEvent::Visibility {
                    t_in,
                    t_out,
                    hp_raw,
                    eid,
                } if *eid == victim => Some((*t_in, *t_out, *hp_raw)),
                _ => None,
            })
            .collect();
        assert_eq!(
            vis,
            vec![(1.4, Some(3.0), Some(2000)), (9.4, None, Some(1500))],
            "每次重入各自携带物化 HP"
        );

        let dmg: Vec<(u16, u16)> = facet
            .events
            .iter()
            .filter_map(|e| match e {
                AiEvent::Damage {
                    victim_eid,
                    hp,
                    hp_raw,
                    ..
                } if *victim_eid == victim => Some((*hp, *hp_raw)),
                _ => None,
            })
            .collect();
        assert_eq!(
            dmg,
            vec![(1700, 1700), (0, 0xFFFD)],
            "hp 钳 0、hp_raw 保留哨兵原值"
        );

        // 非战斗车辆（entityTypeId≠2）的物化不透出 HP
        let mut other = type5(2000);
        other[4..6].copy_from_slice(&3u16.to_le_bytes());
        let packets2: Vec<(u32, f32, &[u8])> = vec![(33, 1.0, &p33), (5, 1.4, &other)];
        let presence = crate::replay::combat::collect_aoi_lifecycle(&packets2);
        assert_eq!(presence.len(), 1);
        assert_eq!(presence[0].hp_raw, None);
    }

    /// method8 原始通知：全变体透出（直击 / 非直击结果 / 短体），字段缺失 = None，非 type=8 包不收。
    #[test]
    fn hit_notices_keep_every_method8_variant() {
        let method8 = |args: &[u8]| {
            let mut p = vec![0u8; 12];
            p[0..4].copy_from_slice(&0x44u32.to_le_bytes());
            p[4..8].copy_from_slice(&8u32.to_le_bytes());
            p[8..12].copy_from_slice(&(args.len() as u32).to_le_bytes());
            p.extend_from_slice(args);
            p
        };
        let mut direct = vec![0u8; 21];
        direct[0..4].copy_from_slice(&0x55u32.to_le_bytes());
        direct[4..8].copy_from_slice(&0x44u32.to_le_bytes());
        direct[8] = 1;
        direct[9] = 3;
        direct[10] = 2;
        let mut other = direct.clone();
        other[9] = 1;
        let short = vec![0x55u8, 0, 0, 0, 0x44];
        let (a, b, c) = (method8(&direct), method8(&other), method8(&short));
        let packets: Vec<(u32, f32, &[u8])> =
            vec![(8, 1.0, &a), (8, 2.0, &b), (8, 3.0, &c), (7, 4.0, &a)];
        let n = crate::replay::combat::collect_hit_notices(&packets);
        assert_eq!(n.len(), 3, "type=7 包不收");
        assert_eq!(
            (
                n[0].shooter_eid,
                n[0].victim_eid,
                n[0].result,
                n[0].secondary
            ),
            (Some(0x55), Some(0x44), Some(3), Some(2))
        );
        assert_eq!(n[1].result, Some(1));
        assert_eq!(
            (
                n[2].payload_len,
                n[2].shooter_eid,
                n[2].victim_eid,
                n[2].result
            ),
            (17, Some(0x55), None, None)
        );
    }

    /// prop3 血量广播：type=7 sub=3 且载荷 ≥14 才收，原始 u16（含哨兵）原样；短包/其它 sub 不收。
    #[test]
    fn prop3_health_is_collected_raw() {
        let prop3 = |sub: u32, hp: u16, len: usize| {
            let mut p = vec![0u8; len];
            p[0..4].copy_from_slice(&0x77u32.to_le_bytes());
            p[4..8].copy_from_slice(&sub.to_le_bytes());
            if len >= 14 {
                p[12..14].copy_from_slice(&hp.to_le_bytes());
            }
            p
        };
        let (a, b, c, d) = (
            prop3(3, 1200, 14),
            prop3(3, 0xFFFD, 16),
            prop3(3, 999, 13),
            prop3(2, 5, 14),
        );
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (7, 2.0, &b),
            (7, 1.0, &a),
            (7, 3.0, &c),
            (7, 4.0, &d),
            (8, 5.0, &a),
        ];
        let got = crate::replay::combat::collect_prop3_health(&packets);
        assert_eq!(
            got,
            vec![
                crate::replay::combat::Prop3Health {
                    clock: 1.0,
                    eid: 0x77,
                    hp_raw: 1200
                },
                crate::replay::combat::Prop3Health {
                    clock: 2.0,
                    eid: 0x77,
                    hp_raw: 0xFFFD
                },
            ]
        );
    }

    /// 原始位姿：只收世界坐标（attachmentParent=0）的 type10，列式等长；prop2 原始 u16 原样。
    #[test]
    fn raw_tracks_keep_world_poses_only() {
        let pose = |eid: u32, parent: u32, x: f32| {
            let mut p = vec![0u8; 48];
            p[0..4].copy_from_slice(&eid.to_le_bytes());
            p[8..12].copy_from_slice(&parent.to_le_bytes());
            p[12..16].copy_from_slice(&x.to_le_bytes());
            p[36..40].copy_from_slice(&0.5f32.to_le_bytes());
            p
        };
        let mut prop2 = vec![0u8; 14];
        prop2[0..4].copy_from_slice(&7u32.to_le_bytes());
        prop2[4..8].copy_from_slice(&2u32.to_le_bytes());
        prop2[12..14].copy_from_slice(&0xABCDu16.to_le_bytes());
        let (a, b, c) = (pose(7, 0, 1.0), pose(7, 99, 2.0), pose(7, 0, 3.0));
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (10, 1.0, &a),
            (10, 2.0, &b),
            (10, 3.0, &c),
            (7, 4.0, &prop2),
        ];
        let (poses, turrets) = collect_raw_tracks(&packets);
        assert_eq!(poses.len(), 1);
        assert_eq!(
            (poses[0].t.clone(), poses[0].x.clone(), poses[0].yaw.clone()),
            (vec![1.0, 3.0], vec![1.0, 3.0], vec![0.5, 0.5])
        );
        assert_eq!(
            (turrets[0].eid, turrets[0].t.clone(), turrets[0].raw.clone()),
            (7, vec![4.0], vec![0xABCD])
        );
    }
}
