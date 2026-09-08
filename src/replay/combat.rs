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
        let mut prev_dc_time = 0.0f32;
        for (dc_time, dc_delta) in &dmg_increases {
            // 2) 确定性目标匹配：HP 下降事件必须落在因果区间 (max(prev_dc, dc−3s), dc] 内——
            //    a) 服务器先扣目标血量（HP 包），再更新射手的伤害计数器（DC 包）→ t ≤ dc
            //    b) 两次 DC 之间不重叠 → t > prev_dc
            //    c) 弹丸飞行时间物理上限 3s → t > dc − 3s
            //    区间内多候选（穿透溅射多目标）时取伤害最接近者（同一弹丸的分配）。
            let window_lo = prev_dc_time.max(*dc_time - 3.0);
            let nearby: Vec<&(f32, u32, String, u16, u16)> = health.iter()
                .filter(|(t, eid, _, _, _)| {
                    *t > window_lo && *t <= *dc_time + 0.05 && *eid != author_eid
                })
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
            prev_dc_time = *dc_time;

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
    /// 来源：type=8 method20（shotId 与发射包确定性配对）。
    /// 注意：method20 为弹道终点（穿透后出射/停止点，可在目标另一侧）。
    pub aim_point: [f32; 3],
    /// 炮口位置相对【开火时刻目标位置】的偏移（米，与 aim_point 同基准）。
    /// viewer 中与 launch_velocity 组合成真实弹道射线：命中点 = 射线 ∩ 装甲模型。
    pub launch_point_rel: [f32; 3],
    /// 弹道两点（回放世界系，米）：
    ///   ball_a = 炮口发射位置（method29 launchPoint）
    ///   ball_b = 弹道终点绝对坐标（method20，shotId 配对）
    /// 两点确定弹道直线——viewer 用方向做 raycast，轴映射只需一次方向变换。
    pub ball_a: [f32; 3],
    pub ball_b: [f32; 3],
    /// 发射速度向量 [vx, vy, vz]（m/s，回放世界系）。
    /// 来源：method29 launchVelocity——服务器权威弹道方向（含俯仰），
    /// 与 launchPoint→终点连线夹角实测 <0.1°。
    pub launch_velocity: [f32; 3],
    /// 命中结果位图（WotbTools method38 resultFlags16，0 = miss/无命中结果）。
    /// 已实证位：0x0008 跳弹 / 0x0010 击穿 / 0x0020 未击穿 / 0x0040 间隙层穿透 /
    /// 0x0080 间隙层未穿 / 0x0100 内部模块击穿 / 0x0400 履带 / 0x0800 火炮 /
    /// 0x1000 HE 爆炸伤害分支。
    pub hit_flags: u16,
    /// 开火时刻（秒）——与 method29 发射包确定性匹配
    pub fire_time: f32,
    /// shotId——method29 发射 ↔ method20 终点 确定性配对键
    pub shot_id: u32,
    /// 兼容旧字段：= type32_turret_yaw（曾误标为"来袭方向"，实为受击者炮塔角）。
    pub incoming_yaw: f32,
    /// 兼容旧字段：= target_gun_pitch（曾误标为"来袭俯角"，实为受击者炮管俯仰）。
    pub incoming_pitch: f32,
}

/// 提取某实体在时刻 t 最近的 type=10 状态包：位置 + 朝向（无时间窗口限制，
/// 取全流中 |dt| 最小者——快照语义，type=10 每 ~100ms 一条，空洞场景仍可命中）。
fn entity_state_at(packets: &[(u32, f32, &[u8])], eid: u32, t: f32) -> Option<([f32; 3], [f32; 3])> {
    let mut best: Option<(f32, [f32; 3], [f32; 3])> = None;
    for (t2, clock, p) in packets {
        if *t2 != 10 || p.len() < 12 + 36 { continue; }
        let e = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        if e != eid { continue; }
        let dt = (*clock - t).abs();
        if best.as_ref().map(|(bd, _, _)| dt < *bd).unwrap_or(true) {
            let f: Vec<f32> = (0..9).map(|k| f32::from_le_bytes([
                p[12 + k*4], p[12 + k*4+1], p[12 + k*4+2], p[12 + k*4+3]])).collect();
            best = Some((dt, [f[0], f[1], f[2]], [f[6], f[7], f[8]]));
        }
    }
    best.map(|(_, pos, ang)| (pos, ang))
}

/// Vehicle method1（type=8 method=0x01）血量/来源/原因事件（WotbTools AFFIRMED）：
/// envelope entityId = 受害者；args 7B = [currentHpRaw u16][sourceEntity u32][causeFlag u8]。
/// cause：0=炮弹直击 1=火焰 2=撞击 3=世界/环境 5=溺水。
/// 确定性伤害归属的数据源——攻击者+原因+受害者三键直接过滤，无时间窗口猜测。
#[derive(Debug, Clone)]
pub struct HpEvent {
    pub clock: f32,
    pub victim: u32,
    pub hp: u16,
    pub source: u32,
    pub cause: u8,
}

/// 解析全部 method1 血量事件（全实体、全来源），按时钟排序。
pub fn parse_hp_events(packets: &[(u32, f32, &[u8])]) -> Vec<HpEvent> {
    let mut out: Vec<HpEvent> = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 12 + 7 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x01 { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len != 7 || 12 + args_len > p.len() { continue; }
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

/// 从射击事件抽取复现数据（WotbTools 权威弹丸生命周期，全部 shotId 确定性配对）：
/// - method29 (0x1d) 弹丸发射：shooterEntityId + shotId + launchPoint + launchVelocity
/// - method20 (0x14) 弹道终点：shotId + endPoint
/// - method38 (0x26) 命中结果（仅作者自己的射击）：victimVehicleId + resultFlags16
/// - 目标 = method38 victimVehicleId（服务器权威）；无 method38 = miss
/// - 伤害 = Vehicle method1 血量链差值（确定性：victim + source=作者 + cause=0）
///
/// 数据完整性fail-fast：任何缺失/歧义（终点缺失、配对歧义、状态快照缺失、
/// 伤害归属失败等）直接返回 Err，不做保守降级（零值/默认值掩盖问题）。
///
pub fn extract_shot_replays(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
) -> anyhow::Result<Vec<ShotReplayData>> {
    if author_player_eid == 0 {
        anyhow::bail!("无法解析作者实体：文件名需包含玩家昵称（type=5 昵称匹配失败）");
    }

    // ① 收集作者的 method29 发射事件（全局弹丸流，按 shooterEntityId 过滤；shotId 去重）
    //    args 布局（alen=37）：[shooterEntityId u32][shotId u32][rawFlag u8]
    //                          [launchPoint 3×f32][launchVelocity 3×f32][terminalRaw f32]
    struct Launch { t: f32, shot_id: u32, point: [f32; 3], vel: [f32; 3] }
    let mut launches: Vec<Launch> = Vec::new();
    let mut seen_shots: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for (_, clock, p) in packets {
        if *clock < 5.0 || p.len() < 12 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x1d { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 4 || 12 + args_len > p.len() { continue; }   // 连 shooter 都读不出：无法归属，跳过
        let a = &p[12..12 + args_len];
        if u32::from_le_bytes([a[0], a[1], a[2], a[3]]) != author_player_eid { continue; }
        if args_len < 37 {
            anyhow::bail!("作者的 method29 发射包 args 长度 {} < 37（回放版本布局漂移？）", args_len);
        }
        let shot_id = u32::from_le_bytes([a[4], a[5], a[6], a[7]]);
        if !seen_shots.insert(shot_id) { continue; }   // 重复包保留首条（非数据伪造）
        let f = |o: usize| f32::from_le_bytes([a[o], a[o+1], a[o+2], a[o+3]]);
        launches.push(Launch {
            t: *clock,
            shot_id,
            point: [f(9), f(13), f(17)],
            vel: [f(21), f(25), f(29)],
        });
    }
    launches.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());

    // ② 收集 method20 弹道终点（shotId 配对；含 miss 的空地终点）
    let mut endpoints: std::collections::HashMap<u32, (f32, [f32; 3])> = std::collections::HashMap::new();
    for (_, clock, p) in packets {
        if p.len() < 28 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x14 { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 16 || 12 + args_len > p.len() { continue; }
        let shot_id = u32::from_le_bytes([p[12], p[13], p[14], p[15]]);
        let a = &p[16..];
        endpoints.entry(shot_id).or_insert((
            *clock,
            [
                f32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                f32::from_le_bytes([a[4], a[5], a[6], a[7]]),
                f32::from_le_bytes([a[8], a[9], a[10], a[11]]),
            ],
        ));
    }

    // ③ 收集 method38 命中结果（Avatar 方法 = 仅作者自己的射击反馈）
    //    args 布局：[victimVehicleId u32][resultFlags16 u16][headerHi16 u16][resultCount u8]...
    let mut hit_results: Vec<(f32, u32, u16)> = Vec::new();  // (t38, victim_eid, flags)
    for (_, clock, p) in packets {
        if p.len() < 12 + 9 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x26 { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 9 || 12 + args_len > p.len() { continue; }
        let a = &p[12..];
        hit_results.push((
            *clock,
            u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
            u16::from_le_bytes([a[4], a[5]]),
        ));
    }
    hit_results.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    // ④ 构建 entity_id → name 映射
    let names = extract_entity_names(packets);

    // ⑤ avatar 实体（pos 全零的 type=10）——射手炮管俯仰（prop9）的宿主
    let avatar_eid = packets.iter()
        .filter(|(t, _, p)| *t == 10 && p.len() >= 48)
        .find(|(_, _, p)| {
            let pos = [
                f32::from_le_bytes([p[12], p[13], p[14], p[15]]),
                f32::from_le_bytes([p[16], p[17], p[18], p[19]]),
                f32::from_le_bytes([p[20], p[21], p[22], p[23]])];
            pos == [0.0, 0.0, 0.0]
        })
        .map(|(_, _, p)| u32::from_le_bytes([p[0], p[1], p[2], p[3]]));
    let avatar_eid = match avatar_eid {
        Some(e) => e,
        None => anyhow::bail!("未找到 avatar 实体（pos 全零的 type=10 缺失），无法解析射手炮管俯仰"),
    };

    // ⑥ 游标
    let hp_events = parse_hp_events(packets);   // method1 血量事件（全实体，按时钟排序）
    // 作者伤害计数器（type=7 sub=10）增量序列——首次命中（血量链无前值）兜底。
    // 计数器挂在 avatar 实体上（每场回放仅一个），无需 eid 过滤；
    // 含撞击/火伤等非弹伤害增量——与 method1 cause≠0 且涉及作者的事件同批
    // （|Δclock|≤0.3s）的增量剔除，剩下的按顺序与造成伤害的命中一一对应。
    let non_shell_ticks: Vec<f32> = hp_events.iter()
        .filter(|e| e.cause != 0 && (e.source == author_player_eid || e.victim == author_player_eid))
        .map(|e| e.clock)
        .collect();
    let mut dc_increments: Vec<(f32, u32)> = Vec::new();
    {
        let mut last_cum: u32 = 0;
        for (t, clock, p) in packets {
            if *t != 7 || p.len() < 16 { continue; }
            if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 10 { continue; }
            let cum = u32::from_le_bytes([p[12], p[13], p[14], p[15]]);
            if cum > last_cum {
                let polluted = non_shell_ticks.iter().any(|tc| (*clock - *tc).abs() <= 0.3);
                if !polluted {
                    dc_increments.push((*clock, cum - last_cum));
                }
                last_cum = cum;
            }
        }
    }
    let mut hr_cursor = 0usize;   // method38 消费游标（按时间顺序，逐发消费）
    let mut dc_cursor = 0usize;   // 计数器增量顺序游标（每个造成伤害的命中消费一条）

    // ⑦ 逐发处理（fail-fast：任何数据缺失/歧义直接报错）
    let mut out: Vec<ShotReplayData> = Vec::with_capacity(launches.len());
    for (i, l) in launches.iter().enumerate() {
        let fire_time = l.t;
        let shot_id = l.shot_id;
        let ctx = format!("shot #{} (shotId={}, t={:.2}s)", i + 1, shot_id, fire_time);

        // 射手状态 @ 开火时刻
        let (sp, sa) = entity_state_at(packets, author_player_eid, fire_time)
            .ok_or_else(|| anyhow::anyhow!("{ctx}: 射手 type=10 状态快照缺失"))?;

        // 弹道终点（shotId 精确配对）；ball_a = method29 炮口发射位置
        let ball_a = l.point;
        let (end_time, ball_b) = endpoints.get(&shot_id).cloned()
            .ok_or_else(|| anyhow::anyhow!("{ctx}: method20 弹道终点缺失（shotId 无配对）"))?;

        // ⑧ 目标实体：method38 victimVehicleId（服务器权威，确定性）
        //    配对：method38 clock ≈ 弹道终点 clock（命中时刻）；窗口内多条 = 歧义 → 报错；
        //    窗口内无 method38 = miss（弹道终点在地面/障碍物）
        let mut target_eid: Option<u32> = None;
        let mut damage = 0u32;
        let mut target_name = String::new();
        let mut is_kill = false;
        let mut hit = false;
        let mut hit_flags: u16 = 0;

        let mut matched: Option<(usize, u32, u16)> = None;
        let mut cands = 0usize;
        for (j, hr) in hit_results.iter().enumerate().skip(hr_cursor) {
            let (t38, _victim, _flags) = *hr;
            if t38 > end_time + 0.5 { break; }   // 已排序，越过窗口即止
            if (t38 - end_time).abs() < 0.5 && t38 >= end_time - 0.05 {
                cands += 1;
                if matched.is_none() { matched = Some((j, _victim, _flags)); }
            }
        }
        if cands > 1 {
            anyhow::bail!("{ctx}: method38 配对歧义——命中窗口内出现 {} 条命中结果", cands);
        }
        if let Some((j, victim, flags)) = matched {
            target_eid = Some(victim);
            hit_flags = flags;
            hit = true;
            hr_cursor = j + 1;
        }

        // target_name：有 target_eid 时从 entity_names 查（max_hp 兜底需要昵称）
        if let Some(teid) = target_eid {
            target_name = names.get(&teid)
                .ok_or_else(|| anyhow::anyhow!("{ctx}: 受击者实体 {teid} 不在 type=5 名册中"))?
                .clone();
        }

        // 伤害归属（确定性）：击穿 0x0010 / HE 爆炸 0x1000 → method1 血量事件
        // 三键过滤：victim=method38 受击者 ∧ source=作者 ∧ cause=0（炮弹直击）——
        // 撞击(2)/火伤(1) 等非弹伤害被 cause 天然排除，无时间窗口猜测。
        // 伤害 = 受害者血量链前值 − 事件后血量；前值 = 该受害者此前最近的 method1
        // 记录（任意来源）；无前记录 = 满血开战（max_hp 兜底，缺失则报错）。
        if hit && hit_flags & (0x0010 | 0x1000) != 0 {
            let victim = target_eid.unwrap_or(0);
            let cands: Vec<(usize, u16)> = hp_events.iter().enumerate()
                .filter(|(_, e)| e.victim == victim && e.source == author_player_eid && e.cause == 0
                    && (e.clock - end_time).abs() <= 0.5)
                .map(|(i, e)| (i, e.hp))
                .collect();
            if cands.is_empty() {
                anyhow::bail!("{ctx}: 击穿/HE 命中但无 method1 血量事件配对（victim={victim}）");
            }
            if cands.len() > 1 {
                anyhow::bail!("{ctx}: method1 血量事件配对歧义（{} 条）", cands.len());
            }
            let (hidx, hp_after) = cands[0];
            // 前值：该受害者在此事件之前最近的 method1 记录（任意来源）
            let hp_before = hp_events[..hidx].iter().rev()
                .find(|e| e.victim == victim)
                .map(|e| e.hp as u32);
            // 计数器增量：顺序消费（计数器含全部伤害来源，过滤非弹后与伤害命中一一对应）
            let dc_delta = if dc_cursor < dc_increments.len() {
                Some(dc_increments[dc_cursor].1)
            } else { None };
            match hp_before {
                Some(prev) if prev >= hp_after as u32 => {
                    damage = prev - hp_after as u32;
                }
                _ => {
                    // 首次命中（血量链无前值，链前值低于事件后=修理/回复等）：
                    // 伤害 = 计数器增量（服务器记账 = "初始血量扣减"的结果，不依赖百科）
                    damage = dc_delta.ok_or_else(|| anyhow::anyhow!(
                        "{ctx}: 血量链无前值且计数器增量已耗尽，伤害归属失败"))?;
                }
            }
            is_kill = hp_after == 0;
            dc_cursor += 1;
        }

        // ⑨ 目标位置与姿态 @ 【命中时刻】（method20 终点 clock = 服务器精确命中时刻；
        // miss 无终点，回退开火时刻）。模型渲染的就是命中时刻的目标，数据锚点必须同时刻。
        let state_time = if hit { end_time } else { fire_time };
        let (tp, ta) = if hit {
            target_eid.and_then(|eid| entity_state_at(packets, eid, state_time))
                .ok_or_else(|| anyhow::anyhow!("{ctx}: 命中时刻目标 type=10 状态快照缺失"))?
        } else { ([0.0; 3], [0.0; 3]) };

        // ⑩ 目标炮塔朝向 = prop2 + hullYaw（命中弹必须有）
        let turret_yaw = if hit {
            let rel = target_eid.and_then(|eid| {
                packets.iter()
                    .filter(|(t, _, p)| {
                        *t == 7 && p.len() >= 14
                            && u32::from_le_bytes([p[0], p[1], p[2], p[3]]) == eid
                            && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2
                    })
                    .min_by_key(|(_, clock, _)| (((*clock - state_time).abs()) * 1000.0) as u32)
                    .map(|(_, _, p)| {
                        let v = u16::from_le_bytes([p[12], p[13]]) as f32;
                        let rel = v / 65535.0 * std::f32::consts::TAU - std::f32::consts::PI;
                        if rel > std::f32::consts::PI { rel - std::f32::consts::TAU } else { rel }
                    })
            }).ok_or_else(|| anyhow::anyhow!("{ctx}: 目标炮塔朝向（type=7 prop2）缺失"))?;
            rel + ta[0]
        } else { 0.0 };

        // ⑪ 射手炮塔朝向 = prop2 + 射手 hullYaw，@ 开火时刻
        let shooter_rel = packets.iter()
            .filter(|(t, _, p)| {
                *t == 7 && p.len() >= 14
                    && u32::from_le_bytes([p[0], p[1], p[2], p[3]]) == author_player_eid
                    && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2
            })
            .min_by_key(|(_, clock, _)| (((*clock - fire_time).abs()) * 1000.0) as u32)
            .map(|(_, _, p)| {
                let v = u16::from_le_bytes([p[12], p[13]]) as f32;
                let rel = v / 65535.0 * std::f32::consts::TAU - std::f32::consts::PI;
                if rel > std::f32::consts::PI { rel - std::f32::consts::TAU } else { rel }
            })
            .ok_or_else(|| anyhow::anyhow!("{ctx}: 射手炮塔朝向（type=7 prop2）缺失"))?;
        let shooter_turret_yaw = shooter_rel + sa[0];

        // ⑫ 射手炮管俯仰（prop9 @ avatar）
        let shooter_gun_pitch = packets.iter()
            .filter(|(t, _, p)| {
                *t == 7 && p.len() >= 16
                    && u32::from_le_bytes([p[0], p[1], p[2], p[3]]) == avatar_eid
                    && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 9
            })
            .min_by_key(|(_, clock, _)| (((*clock - fire_time).abs()) * 1000.0) as u32)
            .map(|(_, _, p)| {
                let deg = f32::from_le_bytes([p[12], p[13], p[14], p[15]]);
                deg.to_radians()
            })
            .ok_or_else(|| anyhow::anyhow!("{ctx}: 射手炮管俯仰（type=7 prop9）缺失"))?;

        // ⑬ aim_point / launch_point_rel = 相对【命中时刻】目标位置（type10 接地高度）的偏移
        // 模型渲染命中时刻的目标（位姿 @ state_time），数据锚点用同一时刻——
        // 目标在开火→命中之间移动时，fire_time 锚点会使标记/相机相对模型错位。
        // 仅命中（有目标实体）时计算——miss 无目标基准，保持全零（viewer 不建射线）
        let (aim_point_val, launch_point_rel) = if ball_b != [0.0; 3] && target_eid.is_some() {
            let rel = |p: [f32; 3]| [p[0] - tp[0], p[1] - tp[1], p[2] - tp[2]];
            (rel(ball_b), rel(ball_a))
        } else { ([0.0; 3], [0.0; 3]) };

        out.push(ShotReplayData {
            index: i + 1,
            time_s: fire_time,
            damage,
            target_name,
            is_kill,
            shooter_pos: sp,
            shooter_ang: sa,
            target_pos: tp,
            target_ang: ta,
            target_turret_yaw: turret_yaw,
            target_gun_pitch: ta[1],
            type32_turret_yaw: 0.0,
            shooter_turret_yaw,
            shooter_gun_pitch,
            aim_point: aim_point_val,
            launch_point_rel,
            ball_a,
            ball_b,
            launch_velocity: l.vel,
            hit_flags,
            fire_time,
            shot_id,
            incoming_yaw: 0.0,
            incoming_pitch: ta[1],
        });
    }

    // ⑧' method38 = 作者自己的命中反馈——每条都必须配对到一次发射
    if hr_cursor < hit_results.len() {
        anyhow::bail!("存在未被任何发射配对的 method38 命中结果（{} 条未消费，自 t={:.2}s 起）——发射/命中配对不完整",
            hit_results.len() - hr_cursor, hit_results[hr_cursor].0);
    }

    Ok(out)
}

/// 便捷封装
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
) -> anyhow::Result<Vec<ShotReplayData>> {
    let author_player_eid = resolve_author_player_eid(packets, file_name);
    extract_shot_replays(packets, author_player_eid)
}
