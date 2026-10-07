//! 战斗事件层：type=7 事件流解析（CombatTimeline）与 method1 血量事件。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::decode_type5_nickname;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatEvent {
    pub timestamp: f32,
    pub entity_id: u32,
    pub entity_name: String,
    pub event_type: CombatEventType,
    pub value: u32,
}

/// 事件子类型（type=7 包内偏移 4..8 的 sub_type）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CombatEventType {
    /// 生命值变化：记录当前血量与受击伤害（sub=3）
    HealthUpdate { health: u16, damage_taken: u16 },
    /// 死亡（sub=1）
    Death,
    /// 作者累计伤害计数器（sub=10）
    DamageCounter { cumulative_damage: u32 },
    /// 游戏时钟进度（sub=9）
    GameClock { progress: f32 },
    /// 通用状态更新（sub=4）
    StateUpdate { state: u8, data: u8 },
    /// 通用数据更新（sub=2）
    GenericUpdate { data: u16 },
    /// 未识别的子类型
    Unknown { sub_type: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatTimeline {
    pub events: Vec<CombatEvent>,
    pub entity_count: usize,
    pub death_count: usize,
    pub total_damage_tracked: u32,
    pub entity_names: HashMap<u32, String>,
}

/// 从 type=5 数据包提取"实体 ID → 昵称"映射；解码语义见 [`decode_type5_nickname`]
/// （原始 UTF-8 全域，长度前缀串 @57——SSOT，勿在此处内联解析）。
pub(crate) fn extract_entity_names(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for (pkt_type, _, payload) in packets {
        if *pkt_type != 5 {
            continue;
        }
        if let Some((eid, s)) = decode_type5_nickname(payload) {
            names.entry(eid).or_insert_with(|| s.to_string());
        }
    }
    names
}

impl CombatTimeline {
    pub fn parse_packets(packets: &[(u32, f32, &[u8])]) -> Self {
        let entity_names = extract_entity_names(packets);
        let mut events = Vec::new();
        // 血量缓存以 type=5 满血锚点预置：否则受害者首个 prop3 事件的降幅恒为 0（首刀不进掉血表）
        let mut entity_health: HashMap<u32, u16> = collect_initial_hp(packets)
            .into_iter()
            .map(|(k, (_, hp))| (k, hp))
            .collect();
        let mut death_entities = std::collections::HashSet::new();

        for (pkt_type, clock, payload) in packets {
            if *pkt_type != 7 || payload.len() < 8 {
                continue;
            }

            let entity_id = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let sub_type = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let entity_name = entity_names
                .get(&entity_id)
                .cloned()
                .unwrap_or_else(|| format!("0x{:08x}", entity_id));

            let event = match sub_type {
                1 => {
                    death_entities.insert(entity_id);
                    CombatEvent {
                        timestamp: *clock,
                        entity_id,
                        entity_name,
                        event_type: CombatEventType::Death,
                        value: 0,
                    }
                }
                3 => {
                    let health = if payload.len() >= 14 {
                        u16::from_le_bytes([payload[12], payload[13]])
                    } else {
                        0
                    };
                    let prev = entity_health.get(&entity_id).copied().unwrap_or(health);
                    let damage_taken = prev.saturating_sub(health);
                    entity_health.insert(entity_id, health);
                    CombatEvent {
                        timestamp: *clock,
                        entity_id,
                        entity_name,
                        event_type: CombatEventType::HealthUpdate {
                            health,
                            damage_taken,
                        },
                        value: damage_taken as u32,
                    }
                }
                10 => {
                    let cum_dmg = if payload.len() >= 16 {
                        u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]])
                    } else {
                        0
                    };
                    CombatEvent {
                        timestamp: *clock,
                        entity_id,
                        entity_name,
                        event_type: CombatEventType::DamageCounter {
                            cumulative_damage: cum_dmg,
                        },
                        value: cum_dmg,
                    }
                }
                9 => {
                    let progress = if payload.len() >= 16 {
                        f32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]])
                    } else {
                        0.0
                    };
                    CombatEvent {
                        timestamp: *clock,
                        entity_id,
                        entity_name,
                        event_type: CombatEventType::GameClock { progress },
                        value: 0,
                    }
                }
                4 => {
                    let (state, data) = if payload.len() >= 14 {
                        (payload[12], payload[13])
                    } else {
                        (0, 0)
                    };
                    CombatEvent {
                        timestamp: *clock,
                        entity_id,
                        entity_name,
                        event_type: CombatEventType::StateUpdate { state, data },
                        value: 0,
                    }
                }
                2 => {
                    let data = if payload.len() >= 14 {
                        u16::from_le_bytes([payload[12], payload[13]])
                    } else {
                        0
                    };
                    CombatEvent {
                        timestamp: *clock,
                        entity_id,
                        entity_name,
                        event_type: CombatEventType::GenericUpdate { data },
                        value: 0,
                    }
                }
                _ => CombatEvent {
                    timestamp: *clock,
                    entity_id,
                    entity_name,
                    event_type: CombatEventType::Unknown { sub_type },
                    value: 0,
                },
            };
            events.push(event);
        }

        let total_damage_tracked = events
            .iter()
            .filter_map(|e| match &e.event_type {
                CombatEventType::DamageCounter { cumulative_damage } => Some(*cumulative_damage),
                _ => None,
            })
            .max()
            .unwrap_or(0);

        let entity_count = entity_health.len();

        CombatTimeline {
            events,
            entity_count,
            death_count: death_entities.len(),
            total_damage_tracked,
            entity_names,
        }
    }

    /// 提取所有生命值变化事件：`(时间, 实体ID, 名称, 当前血量, 受击伤害)`。
    pub fn health_timeline(&self) -> Vec<(f32, u32, String, u16, u16)> {
        self.events
            .iter()
            .filter_map(|e| match &e.event_type {
                CombatEventType::HealthUpdate {
                    health,
                    damage_taken,
                } => Some((
                    e.timestamp,
                    e.entity_id,
                    e.entity_name.clone(),
                    *health,
                    *damage_taken,
                )),
                _ => None,
            })
            .collect()
    }

    /// 提取所有死亡事件：`(时间, 实体ID, 名称)`。
    pub fn death_events(&self) -> Vec<(f32, u32, String)> {
        self.events
            .iter()
            .filter_map(|e| match e.event_type {
                CombatEventType::Death => Some((e.timestamp, e.entity_id, e.entity_name.clone())),
                _ => None,
            })
            .collect()
    }
}

/// 血量终态哨兵族（WotbTools PROVEN：终态 prop3/method1 分布 0:-223/-1:68/-2:1/-3:59，
/// 即 {0x0000, 0xFFFF, 0xFFFE, 0xFFFD} 四值；受控溺水实验证明死亡时 HP 可为正——
/// hp<=0 是充分非必要条件，cause=5 溺死不经血量归零）。归一化后 DmgLoss.hp_cur==0
/// 覆盖全族，击杀判定与降幅推导对哨兵终态同样成立。
pub fn hp_terminal_normalized(hp: u16) -> u16 {
    if (hp as i16) < 0 {
        0
    } else {
        hp
    }
}

/// method1 (0x01) 血量/来源/原因事件（WotbTools AFFIRMED）：
/// envelope entityId = 受害者；args 7B = [currentHpRaw u16][sourceEntity u32][causeFlag u8]；
/// currentHpRaw = 事件后的绝对血量快照（非增量），终态哨兵族见 [`hp_terminal_normalized`]；
/// cause：0=炮弹直击 1=火焰 2=撞击 3=世界/环境 5=溺水（4=未观测，禁按序数推断）；
/// source 按 cause 分域 PROVEN：cause 0/1/2 = 攻击者/点燃者/碰撞对方（≠victim），
/// cause 3/5 = 自身（==victim）。
#[derive(Debug, Clone)]
pub struct HpEvent {
    pub clock: f32,
    pub victim: u32,
    pub hp: u16,
    pub source: u32,
    pub cause: u8,
}

/// type=5 车辆全量状态包 → 每实体首条出现时的当前血量（偏移 51 的 u16，满血锚点）。
/// 20 实体实证（GB109）：首条 type=5 恒早于该实体首次掉血，且链路闭合（如
/// KingScopion seed 1650 − 首条 method1 1279 = 371 = 作者首发伤害）；血量在包内出现两次
/// （51 与 ~190，后者随载荷长度漂移），取前者的固定偏移。
/// 用途：血量链 seed——受害者首个 method1 事件已是掉血后血量时（受击前无任何 method1），
/// 无锚点则首刀降幅无法推导（CLI 掉血表缺行 + dmg_unattributed 误报）。
pub fn collect_initial_hp(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, (f32, u16)> {
    let mut out: HashMap<u32, (f32, u16)> = HashMap::new();
    for (ptype, clock, p) in packets {
        if *ptype != 5 || p.len() < 53 {
            continue;
        }
        let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        out.entry(eid)
            .or_insert((*clock, u16::from_le_bytes([p[51], p[52]])));
    }
    out
}

/// type=7 sub=3（prop3）血量属性广播：`[eid u32][sub u32][...][health u16 @12]`。
/// 与 method1 不是镜像——**录像者自身车辆**的血量变化常只有 prop3、没有 method1
/// （WotbTools 冻结样本：3 场 4/17/11 条录像者 prop3 无对应 method1），同刻两者也可能取值不同。
/// 只收载荷足长（≥14）的包，原始 u16 原样保留（哨兵族不解释；[`CombatTimeline`] 对短包写 0
/// 是显示口径，不能当事实）。按时钟排序（稳定，保留包序）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Prop3Health {
    pub clock: f32,
    pub eid: u32,
    pub hp_raw: u16,
}

pub fn collect_prop3_health(packets: &[(u32, f32, &[u8])]) -> Vec<Prop3Health> {
    let mut out: Vec<Prop3Health> = Vec::new();
    for (ptype, clock, p) in packets {
        if *ptype != 7 || p.len() < 14 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 3 {
            continue;
        }
        out.push(Prop3Health {
            clock: *clock,
            eid: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            hp_raw: u16::from_le_bytes([p[12], p[13]]),
        });
    }
    out.sort_by(|x, y| x.clock.partial_cmp(&y.clock).unwrap());
    out
}

/// 解析全部 method1 血量事件（全实体、全来源），按时钟排序。
pub fn parse_hp_events(packets: &[(u32, f32, &[u8])]) -> Vec<HpEvent> {
    let mut out: Vec<HpEvent> = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 12 + 7 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x01 {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len != 7 || 12 + args_len > p.len() {
            continue;
        }
        let a = &p[12..12 + args_len];
        out.push(HpEvent {
            clock: *clock,
            victim: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            hp: u16::from_le_bytes([a[0], a[1]]),
            source: u32::from_le_bytes([a[2], a[3], a[4], a[5]]),
            cause: a[6],
        });
    }
    out.sort_by(|x, y| x.clock.partial_cmp(&y.clock).unwrap());
    out
}

// ===== 作者/他人射击路径共用的收集阶段（按 author 过滤参数化） =====
