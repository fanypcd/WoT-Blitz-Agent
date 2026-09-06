use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =====================================================================
//  战斗事件解码（自主逆向）
//  回放的 `data.wotreplay` 数据包流里，type=7 的事件包有多种子类型
//  （生命值变化、死亡、累计伤害、游戏时钟等）。本模块负责把这些
//  原始包解码成结构化事件，并据此推断"每发射击"。
// =====================================================================

/// 一条战斗事件（时间 + 实体 + 类型 + 数值）。
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

/// 推断出的一次"射击事件"：把作者伤害计数器递增与敌方生命值下降相关联。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotEvent {
    pub timestamp: f32,
    pub damage: u32,
    pub target_name: String,
    /// 推断的目标实体 ID（按"伤害最接近"匹配）。
    pub target_eid: u32,
    pub target_hp_after: Option<u16>,
    pub target_damage: u16,
    pub is_kill: bool,
    pub hit: bool,
}

/// 一场战斗的完整事件时间线。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatTimeline {
    pub events: Vec<CombatEvent>,
    pub entity_count: usize,
    pub death_count: usize,
    pub total_damage_tracked: u32,
    pub entity_names: HashMap<u32, String>,
}

/// 从 type=5 数据包提取"实体 ID → 昵称"映射（用于把事件中的实体 ID 翻译成玩家名）。
///
/// 昵称是载荷偏移 57 处的"长度前缀 ASCII 字符串"（1 字节长度 + 字符串）。
fn extract_entity_names(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for (pkt_type, _, payload) in packets {
        if *pkt_type != 5 || payload.len() < 60 {
            continue;
        }
        // Entity ID is at offset 0 (first 4 bytes)
        let eid = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        // Nickname is at offset 57: 1-byte length + ASCII string
        let offset = 57;
        if offset >= payload.len() {
            continue;
        }
        let str_len = payload[offset] as usize;
        if !(3..=30).contains(&str_len) || offset + 1 + str_len > payload.len() {
            continue;
        }
        if let Ok(s) = std::str::from_utf8(&payload[offset + 1..offset + 1 + str_len]) {
            if s.chars().all(|c| c.is_ascii_graphic()) {
                names.entry(eid).or_insert_with(|| s.to_string());
            }
        }
    }
    names
}

impl CombatTimeline {
    /// 把原始数据包列表解码成事件时间线。
    pub fn parse_packets(packets: &[(u32, f32, &[u8])]) -> Self {
        let entity_names = extract_entity_names(packets);
        let mut events = Vec::new();
        // 记录各实体最近一次生命值（用于计算 damage_taken = 前值 - 现值）
        let mut entity_health: HashMap<u32, u16> = HashMap::new();
        // 记录已死亡的实体（用于统计死亡数、判断击杀）
        let mut death_entities = std::collections::HashSet::new();

        for (pkt_type, clock, payload) in packets {
            // 只处理 type=7（事件）包，且载荷至少 8 字节（含实体 ID + 子类型）
            if *pkt_type != 7 || payload.len() < 8 {
                continue;
            }

            // 前 4 字节 = 实体 ID，接着 4 字节 = 子类型
            let entity_id = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let sub_type = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let entity_name = entity_names.get(&entity_id).cloned().unwrap_or_else(|| format!("0x{:08x}", entity_id));

            let event = match sub_type {
                // sub=1：死亡
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
                // sub=3：生命值变化（偏移 12..14 是当前血量）
                3 => {
                    let health = if payload.len() >= 14 {
                        u16::from_le_bytes([payload[12], payload[13]])
                    } else {
                        0
                    };
                    // 用上一帧血量减去当前血量得到"本次受击伤害"
                    let prev = entity_health.get(&entity_id).copied().unwrap_or(health);
                    let damage_taken = prev.saturating_sub(health);
                    entity_health.insert(entity_id, health);
                    CombatEvent {
                        timestamp: *clock,
                        entity_id,
                        entity_name,
                        event_type: CombatEventType::HealthUpdate { health, damage_taken },
                        value: damage_taken as u32,
                    }
                }
                // sub=10：作者累计伤害计数器（偏移 12..16 是累计伤害）
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
                        event_type: CombatEventType::DamageCounter { cumulative_damage: cum_dmg },
                        value: cum_dmg,
                    }
                }
                // sub=9：游戏时钟（偏移 12..16 是进度浮点）
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
                // sub=4：状态更新（偏移 12..14）
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
                // sub=2：通用数据更新（偏移 12..14）
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

        // 作者累计伤害的最大值即为全场追踪到的总伤害
        let total_damage_tracked = events.iter()
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
        self.events.iter()
            .filter_map(|e| match &e.event_type {
                CombatEventType::HealthUpdate { health, damage_taken } => {
                    Some((e.timestamp, e.entity_id, e.entity_name.clone(), *health, *damage_taken))
                }
                _ => None,
            })
            .collect()
    }

    /// 提取所有死亡事件：`(时间, 实体ID, 名称)`。
    pub fn death_events(&self) -> Vec<(f32, u32, String)> {
        self.events.iter()
            .filter_map(|e| match e.event_type {
                CombatEventType::Death => Some((e.timestamp, e.entity_id, e.entity_name.clone())),
                _ => None,
            })
            .collect()
    }

    /// 打印事件时间线（人类可读，`combat` 命令用）。
    pub fn print_timeline(&self) {
        println!();
        println!("--- Combat Event Timeline ---");
        println!("  Entities: {}", self.entity_count);
        println!("  Deaths: {}", self.death_count);
        println!("  Total damage tracked: {}", self.total_damage_tracked);
        println!();

        if !self.entity_names.is_empty() {
            println!("  Player mapping ({}):", self.entity_names.len());
            for (eid, name) in self.entity_names.iter() {
                println!("    0x{:08x} = {}", eid, name);
            }
            println!();
        }

        let deaths = self.death_events();
        if !deaths.is_empty() {
            println!("  Deaths:");
            for (t, _, name) in &deaths {
                println!("    t={:.1}s  {}", t, name);
            }
            println!();
        }

        let health = self.health_timeline();
        if !health.is_empty() {
            println!("  Health changes (damage > 100):");
            println!("    {:>8} {:<25} {:>10} {:>8}", "Time", "Player", "Damage", "HP left");
            println!("    {}", "-".repeat(55));
            for (t, _, name, hp, dmg) in health.iter().filter(|(_, _, _, _, d)| *d > 100) {
                println!("    {:>7.1}s {:<25} -{:>8} {:>8}", t, name, dmg, hp);
            }
        }
    }

    /// 推断"每发射击"：关联作者的伤害计数器递增与敌方生命值下降。
    ///
    /// 思路：作者伤害计数器（sub=10）每次递增即代表一次开炮命中，从递增差值
    /// 得到本次伤害；再在同一时刻附近找到血量下降的敌方实体作为目标。
    pub fn infer_shots(&self, author_eid: u32) -> Vec<ShotEvent> {
        let health = self.health_timeline();
        let deaths = self.death_events();

        // 1) 找出作者伤害计数器的每次递增（递增差值 = 单发伤害）
        let mut dmg_increases: Vec<(f32, u32)> = Vec::new();
        let mut last_cum = 0u32;
        for e in &self.events {
            if let CombatEventType::DamageCounter { cumulative_damage } = &e.event_type {
                if *cumulative_damage > last_cum {
                    dmg_increases.push((e.timestamp, *cumulative_damage - last_cum));
                    last_cum = *cumulative_damage;
                }
            }
        }

        let mut shots = Vec::new();
        for (dc_time, dc_delta) in &dmg_increases {
            // 2) 在同一时刻(<0.5s)附近找非作者的、血量下降的实体作为候选目标
            let nearby: Vec<&(f32, u32, String, u16, u16)> = health.iter()
                .filter(|(t, eid, _, _, _)| (*t - dc_time).abs() < 0.5 && *eid != author_eid)
                .collect();

            // 唯一目标直接用；多个候选时选受击伤害与本次伤害最接近的那个
            let target = if nearby.len() == 1 {
                Some(nearby[0])
            } else if nearby.len() > 1 {
                nearby.iter().min_by_key(|x| {
                    (x.4 as i32 - *dc_delta as i32).unsigned_abs()
                }).copied()
            } else {
                None
            };

            // 3) 若目标在本次射击后 1 秒内死亡，则判定为击杀
            let is_kill = if let Some((_, target_eid, _, _, _)) = target {
                deaths.iter().any(|(d_time, d_eid, _)|
                    *d_eid == *target_eid && (*d_time - dc_time).abs() < 1.0)
            } else { false };

            shots.push(ShotEvent {
                timestamp: *dc_time,
                damage: *dc_delta,
                target_name: target.map(|(_, _, name, _, _)| name.clone()).unwrap_or_else(|| "miss/assist".to_string()),
                target_eid: target.map(|(_, eid, _, _, _)| *eid).unwrap_or_default(),
                target_hp_after: target.map(|(_, _, _, hp, _)| *hp),
                target_damage: target.map(|(_, _, _, _, dmg)| *dmg).unwrap_or(0),
                is_kill,
                hit: target.is_some(),
            });
        }
        shots
    }

    /// 打印射击推断结果（人类可读）。
    pub fn print_shots(&self, shots: &[ShotEvent]) {
        let hits = shots.iter().filter(|s| s.hit).count();
        let kills = shots.iter().filter(|s| s.is_kill).count();
        let total_dmg: u32 = shots.iter().map(|s| s.damage).sum();

        println!();
        println!("--- Shot Event Inference ---");
        println!("  Shots (damage counter): {}", shots.len());
        println!("  Hits (matched to HP decrease): {}", hits);
        println!("  Kills: {}", kills);
        println!("  Total damage: {}", total_dmg);
        println!();
        println!("  {:>7} {:>6} {:<25} {:>6} {:>6} {:>4}", "Time", "Dmg", "Target", "TgtHP", "TgtDmg", "Kill");
        println!("  {}", "-".repeat(60));
        for s in shots {
            let kill = if s.is_kill { "KILL" } else { "" };
            let hp = s.target_hp_after.map(|h| h.to_string()).unwrap_or_else(|| "-".to_string());
            println!("  {:>6.1}s {:>6} {:<25} {:>6} {:>6} {:>4}",
                s.timestamp, s.damage, s.target_name, hp, s.target_damage, kill);
        }
    }

}

/// 一次射击事件的"复现数据"：双方位置/朝向（type=10 实体状态包解码），
/// 供 3D 查看器按当时态势复现热力图视角。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotReplayData {
    pub index: usize,
    pub time_s: f32,
    pub damage: u32,
    pub target_name: String,
    pub is_kill: bool,
    /// 射手（作者玩家实体）位置 [x, y(离地), z]
    pub shooter_pos: [f32; 3],
    /// 射手朝向（3 个浮点，语义为最可能猜测：偏航/俯仰/侧倾，弧度）
    pub shooter_ang: [f32; 3],
    /// 目标位置
    pub target_pos: [f32; 3],
    /// 目标朝向
    pub target_ang: [f32; 3],
    /// 目标炮塔绝对朝向（弧度，游戏约定：与 hull yaw 同参考系，顺时针为正）。
    /// 来源：type=7 sub=2 的 u16，ang = u16/65535×2π - π（实测校准有 180° 偏移）。
    pub target_turret_yaw: f32,
    /// 受击者炮管俯仰（弧度，正=仰角）。
    /// 来源：type=32 警告包 pitch = (u16-32768)/32768×(π/2)（eid=受击者 → 属性是受击者自己的）。
    /// shot 1 = -2.43°（KingScopion 瞄准略下压 ✓）；物理约束 ±20°，超界回退 hull pitch。
    pub target_gun_pitch: f32,
    /// 受击者炮塔朝向（type=32 备份，精度低于 sub2；主用 target_turret_yaw）。
    pub type32_turret_yaw: f32,
    /// 射手炮塔绝对朝向（弧度）= sub2_rel + shooter_hullYaw。
    /// 用于精确入射方位角（替代位置差推算）。
    pub shooter_turret_yaw: f32,
    /// 射手炮管俯仰（弧度）= type=7 sub9 f32（度→弧度）。
    /// avatar 相机俯仰，狙击模式下 = 炮管俯仰。
    pub shooter_gun_pitch: f32,
    /// 弹着点相对【开火时刻目标位置】的偏移 [x, y, z]（米，回放世界系）。
    /// 来源：type=8 0x14（计数器与开火包确定性配对）。
    /// 注意：0x14 为弹道终点（穿透后出射/停止点，可在目标另一侧）。
    pub aim_point: [f32; 3],
    /// 弹道两点（回放世界系，米）：
    ///   ball_a = 射手位置 @ 开火时刻（type=10）
    ///   ball_b = 弹着点绝对坐标（type=8 0x14 计数器配对）
    /// 两点确定弹道直线——viewer 用方向做 raycast，轴映射只需一次方向变换。
    pub ball_a: [f32; 3],
    pub ball_b: [f32; 3],
    /// 兼容旧字段：= type32_turret_yaw（曾误标为"来袭方向"，实为受击者炮塔角）。
    pub incoming_yaw: f32,
    /// 兼容旧字段：= target_gun_pitch（曾误标为"来袭俯角"，实为受击者炮管俯仰）。
    pub incoming_pitch: f32,
}

/// 提取某实体在时刻 t 附近（±0.3s）最近的 type=10 状态包：位置 + 朝向。
fn entity_state_at(packets: &[(u32, f32, &[u8])], eid: u32, t: f32) -> Option<([f32; 3], [f32; 3])> {
    let mut best: Option<(f32, [f32; 3], [f32; 3])> = None;
    for (t2, clock, p) in packets {
        if *t2 != 10 || p.len() < 12 + 36 { continue; }
        let e = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        if e != eid { continue; }
        let dt = (*clock - t).abs();
        if dt > 0.3 { continue; }
        if best.as_ref().map(|(bd, _, _)| dt < *bd).unwrap_or(true) {
            let f: Vec<f32> = (0..9).map(|k| f32::from_le_bytes([
                p[12 + k*4], p[12 + k*4+1], p[12 + k*4+2], p[12 + k*4+3]])).collect();
            best = Some((dt, [f[0], f[1], f[2]], [f[6], f[7], f[8]]));
        }
    }
    best.map(|(_, pos, ang)| (pos, ang))
}

/// 从射击事件抽取复现数据：
/// - 射手 = 作者玩家实体（按昵称/文件名匹配出 eid，由调用方传入）
/// - 目标 = 命中时刻最近的生命值变化实体（其 eid 即受击玩家实体）
pub fn extract_shot_replays(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
    shots: &[ShotEvent],
) -> Vec<ShotReplayData> {
    let health = packets.iter()
        .filter_map(|(t, clock, p)| {
            if *t != 7 || p.len() < 14 { return None; }
            let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            let sub = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
            if sub != 3 { return None; }
            let hp = u16::from_le_bytes([p[12], p[13]]);
            Some((*clock, eid, hp))
        })
        .collect::<Vec<_>>();

    shots.iter().enumerate().map(|(i, s)| {
        // 射手状态
        let (sp, sa) = entity_state_at(packets, author_player_eid, s.timestamp)
            .unwrap_or(([0.0; 3], [0.0; 3]));
        // 目标实体：优先用 infer_shots 已按"伤害最接近"推断的 target_eid。
        // （旧实现在此重新按"时间最近"匹配 → 交火场景会选错实体：
        //   shot3 附近 Sulaiman 掉血时刻更近但伤害不符 → 朝向/位置全错。）
        let target_eid = if s.target_eid != 0 { Some(s.target_eid) } else {
            health.iter()
                .filter(|(t, eid, _)| (*t - s.timestamp).abs() < 0.5 && *eid != author_player_eid)
                .min_by_key(|(t, _, _)| ((*t - s.timestamp).abs() * 1000.0) as u32)
                .map(|(_, eid, _)| *eid)
        };
        let (tp, ta) = target_eid.and_then(|eid| entity_state_at(packets, eid, s.timestamp))
            .unwrap_or(([0.0; 3], [0.0; 3]));
        // 目标炮塔朝向 = sub2（相对车体角）+ hullYaw，@ 伤害时刻
        let turret_yaw = target_eid.and_then(|eid| {
            packets.iter()
                .filter(|(t, clock, p)| {
                    *t == 7 && p.len() >= 14
                        && u32::from_le_bytes([p[0], p[1], p[2], p[3]]) == eid
                        && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2
                        && (*clock - s.timestamp).abs() < 0.5
                })
                .min_by_key(|(_, clock, _)| (((*clock - s.timestamp).abs()) * 1000.0) as u32)
                .map(|(_, _, p)| {
                    let v = u16::from_le_bytes([p[12], p[13]]) as f32;
                    // sub2 = 相对车体炮塔角（无 π/16 偏移）。
                    // 校准验证（射手 8 发：sub2_rel + hullYaw vs bearing to target，
                    // 6/8 差 ≤1.7°；shot5/7 差 10/5° = 追踪延迟）。
                    let rel = v / 65535.0 * std::f32::consts::TAU
                        - std::f32::consts::PI;
                    if rel > std::f32::consts::PI { rel - std::f32::consts::TAU } else { rel }
                })
        }).map(|rel| rel + ta[0]).unwrap_or(0.0); // rel + hullYaw = 绝对炮塔角

        // 开火事件（type=8 0x1d）：按【射手 eid + 伤害时刻前的最后一发】定位候选。
        // 开火后才发生伤害（弹丸飞行 0.2-1.5s）。
        // 返回 (fire_time, counter) —— counter 是与 0x14 命中包确定性配对的键。
        let fire_evt = packets.iter()
            .filter(|(t, clock, p)| {
                if *t != 8 || p.len() < 24 { return false; }
                let method = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
                if method != 0x1d { return false; }
                let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
                if args_len < 8 || 12 + args_len > p.len() { return false; }
                let a = &p[12..];
                u32::from_le_bytes([a[0], a[1], a[2], a[3]]) == author_player_eid
                    && *clock <= s.timestamp + 0.1
            })
            .filter(|(_, clock, _)| s.timestamp - clock < 3.0)
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .map(|(_, clock, p)| {
                let a = &p[12..];
                (clock, u32::from_le_bytes([a[4], a[5], a[6], a[7]]))
            });
        let (fire_time_ref, fire_counter) = fire_evt.unwrap_or((&s.timestamp, 0u32));
        let fire_time = *fire_time_ref;

        // 射手炮塔朝向（@ 开火时刻——开火后炮塔可能继续转动）
        let shooter_turret_yaw = {
            let author_eid = author_player_eid;
            packets.iter()
                .filter(|(t, clock, p)| {
                    *t == 7 && p.len() >= 14
                        && u32::from_le_bytes([p[0], p[1], p[2], p[3]]) == author_eid
                        && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2
                        && (*clock - fire_time).abs() < 0.5
                })
                .min_by_key(|(_, clock, _)| (((*clock - fire_time).abs()) * 1000.0) as u32)
                .map(|(_, _, p)| {
                    let v = u16::from_le_bytes([p[12], p[13]]) as f32;
                    let rel = v / 65535.0 * std::f32::consts::TAU
                        - std::f32::consts::PI;
                    if rel > std::f32::consts::PI { rel - std::f32::consts::TAU } else { rel }
                })
                .map(|rel| rel + sa[0])
                .unwrap_or(0.0)
        };

        // 射手炮管俯仰：type=7 sub=9（avatar 实体的相机俯仰，单位=度）。
        // 狙击模式下相机俯仰 = 炮管俯仰。
        // 验证：shot1 -0.40° vs 几何 -0.08°（差0.32°）✓、shot5 +1.08° vs +0.82° ✓
        // （shot2/3/4 差 5-10° → 射手不在狙击模式时的第三人称相机俯仰）。
        let shooter_gun_pitch = {
            // avatar 实体 eid：type=10 中位置恒 [0,0,0] 的实体
            let avatar_eid = packets.iter()
                .filter(|(t, _, p)| *t == 10 && p.len() >= 48)
                .find(|(_, _, p)| {
                    let pos = [
                        f32::from_le_bytes([p[12], p[13], p[14], p[15]]),
                        f32::from_le_bytes([p[16], p[17], p[18], p[19]]),
                        f32::from_le_bytes([p[20], p[21], p[22], p[23]])];
                    pos == [0.0, 0.0, 0.0]
                })
                .map(|(_, _, p)| u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
                .unwrap_or(0);
            packets.iter()
                .filter(|(t, clock, p)| {
                    *t == 7 && p.len() >= 16
                        && u32::from_le_bytes([p[0], p[1], p[2], p[3]]) == avatar_eid
                        && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 9
                        && (*clock - fire_time).abs() < 0.5
                })
                .min_by_key(|(_, clock, _)| (((*clock - fire_time).abs()) * 1000.0) as u32)
                .map(|(_, _, p)| {
                    // sub9 f32 单位=度 → 转弧度
                    let deg = f32::from_le_bytes([p[12], p[13], p[14], p[15]]);
                    deg.to_radians()
                })
                .unwrap_or(0.0)
        };

        // 弹丸命中点：type=8 method 0x14(20) 的 args 末尾 3×f32（世界坐标）。
        // 每次开火（type=8 0x1d）后立即发送，一一对应。
        // 验证：shot1 开火@47.198 → 0x14点@47.296 = (-252.1, 27.6, 149.6)
        //       相对目标(-251.7, 26.6, 152.0) 偏移 (-0.4, +1.0, -2.4) = 目标表面 ✓
        // 弹道 A 点：射手位置 @ 开火时刻
        let ball_a = entity_state_at(packets, author_player_eid, fire_time)
            .map(|(p, _)| p).unwrap_or(sp);
        // 命中点（type=8 0x14）：按【counter 与开火包精确配对】（确定性，无时间窗口）。
        // 验证：142/145 精确匹配（dt=0.000 同 tick）；0x14 args[0..4] = 0x1d args[4..8]。
        // 布局：args = [counter u32][命中点 3×f32]。
        let hit_point_abs_opt: Option<[f32; 3]> = packets.iter()
            .filter(|(t, _, p)| {
                if *t != 8 || p.len() < 28 { return false; }
                let method = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
                if method != 0x14 { return false; }
                let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
                args_len >= 16 && 12 + args_len <= p.len()
                    && u32::from_le_bytes([p[12], p[13], p[14], p[15]]) == fire_counter
            })
            .next()
            .map(|(_, _, p)| {
                let a = &p[16..];  // counter 后 = 3×f32
                [
                    f32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                    f32::from_le_bytes([a[4], a[5], a[6], a[7]]),
                    f32::from_le_bytes([a[8], a[9], a[10], a[11]]),
                ]
            })
            // 弹着点绝对坐标（ball_b）
            ;
        let ball_b = hit_point_abs_opt.unwrap_or([0.0; 3]);
        // 相对偏移（aim_point）：目标在移动，伤害时刻位置 ≠ 开火时刻位置（差可达 7m）
        let aim_point_val = hit_point_abs_opt.map(|hp| {
            let (ftp, _) = target_eid
                .and_then(|eid| entity_state_at(packets, eid, fire_time))
                .unwrap_or((tp, [0.0; 3]));
            [hp[0] - ftp[0], hp[1] - ftp[1], hp[2] - ftp[2]]
        });

        // 来袭炮弹参数：type=32 (method 0x11/0x12) = 来袭炮弹警告包（eid=受击者）：
        //   [eid][01][method u32][u16][flag u8][shell_id u16][yaw u16][pitch u16][segment u64]
        // 尾部 u64 = wotinspector 的 segment 字段（对齐验证 ✓）；
        // yaw = (u16-32768)/32768×π：受击者指向射手的方位角（shot 1 解码 +66.9° vs
        //       位置推算 +68.8°，射手飞行期移动 → 交叉验证了整个轴映射 ✓）；
        // pitch = (u16-32768)/32768×(π/2)：炮弹抵达垂直角（shot 1 = -2.43° ≈ 弹道末段
        //       下落角 ✓；π/2 比例尺待目视校准）。
        // 交火中受击者可能同时有多发来袭警告 → 收集命中前 ±3s 全部候选，
        // 按 yaw 与位置推算方位角的偏差最小者消歧；|pitch|>30° 的非直射弹道丢弃。
        let inc = target_eid.and_then(|eid| {
            let bearing = if tp != [0.0; 3] && sp != [0.0; 3] {
                Some((sp[0] - tp[0]).atan2(sp[2] - tp[2]))
            } else { None };
            let cands: Vec<(f32, f32)> = packets.iter()
                .filter(|(t, clock, p)| {
                    if *t != 32 || p.len() < 26 { return false; }
                    if u32::from_le_bytes([p[0], p[1], p[2], p[3]]) != eid { return false; }
                    let method = u32::from_le_bytes([p[5], p[6], p[7], p[8]]);
                    (method == 0x11 || method == 0x12)
                        && *clock <= s.timestamp + 0.1 && *clock > s.timestamp - 3.0
                })
                .map(|(_, _, p)| {
                    let off = if p.len() >= 27 { 13 } else { 12 };
                    let u = |k: usize| u16::from_le_bytes([p[off + k], p[off + k + 1]]) as f32;
                    (
                        (u(2) - 32768.0) / 32768.0 * std::f32::consts::PI,
                        (u(4) - 32768.0) / 32768.0 * (std::f32::consts::FRAC_PI_2),
                    )
                })
                .collect();
            // 选取：yaw 与位置推算方位角偏差最小者；偏差 >15° 视为其他炮弹的警告，弃用
            let best = cands.into_iter().min_by(|a, b| {
                let err = |y: f32| bearing.map(|b| {
                    ((y - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI).abs()
                }).unwrap_or(0.0);
                err(a.0).partial_cmp(&err(b.0)).unwrap()
            });
            best.filter(|(y, _)| {
                bearing.map(|b| {
                    ((y - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI).abs() < 0.262
                }).unwrap_or(true)
            })
        });
        // type=32 eid=受击者 → yaw/pitch 都是受击者自己的姿态
        // （炮塔朝向备份 + 炮管俯仰）。yaw 与 sub2 交叉验证（shot 1 差 1.9°）。
        // yaw 与位置推算方位角偏差 >15° → 可能是其他炮弹的警告包，弃用。
        let (type32_yaw, type32_pitch) = match inc {
            Some((y, p)) if p.to_degrees().abs() <= 30.0 => (y, p),
            Some((y, _)) => (y, 0.0), // pitch 越界：保留 yaw
            _ => (0.0, 0.0),
        };
        ShotReplayData {
            index: i + 1,
            time_s: s.timestamp,
            damage: s.damage,
            target_name: s.target_name.clone(),
            is_kill: s.is_kill,
            shooter_pos: sp,
            shooter_ang: sa,
            target_pos: tp,
            target_ang: ta,
            target_turret_yaw: turret_yaw,
            target_gun_pitch: if type32_pitch != 0.0 { type32_pitch } else { ta[1] },
            type32_turret_yaw: type32_yaw,
            shooter_turret_yaw,
            shooter_gun_pitch,
            aim_point: aim_point_val.unwrap_or([0.0; 3]),
            ball_a,
            ball_b,
            incoming_yaw: type32_yaw,
            incoming_pitch: if type32_pitch != 0.0 { type32_pitch } else { ta[1] },
        }
    }).collect()
}

/// 便捷封装：从回放文件名自动解析作者玩家实体（昵称 ↔ 文件名匹配），
/// 再抽取射击复现数据。回放解析管线的标准入口。
/// 按回放文件名中的昵称匹配 type=5 包，解析作者玩家实体 eid。
pub fn resolve_author_player_eid(packets: &[(u32, f32, &[u8])], file_name: &str) -> u32 {
    packets.iter()
        .filter_map(|(t, _, p)| {
            if *t != 5 || p.len() < 60 { return None; }
            let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            let off = 57usize;
            if off >= p.len() { return None; }
            let l = p[off] as usize;
            if !(3..=30).contains(&l) || off + 1 + l > p.len() { return None; }
            std::str::from_utf8(&p[off + 1..off + 1 + l]).ok()
                .filter(|s| s.chars().all(|c| c.is_ascii_graphic()))
                .filter(|s| file_name.contains(s))
                .map(|_| eid)
        })
        .next()
        .unwrap_or(0)
}

pub fn extract_shot_replays_auto(
    packets: &[(u32, f32, &[u8])],
    file_name: &str,
    shots: &[ShotEvent],
) -> Vec<ShotReplayData> {
    let author_player_eid = resolve_author_player_eid(packets, file_name);
    extract_shot_replays(packets, author_player_eid, shots)
}
