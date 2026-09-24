use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::filter::FilteredTimeline;

/// serde skip_serializing_if 助手：false 不序列化
fn is_false(b: &bool) -> bool { !*b }

/// 单发射击的数据质量标注（宽松降级与快照陈旧度的可视化依据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotQuality {
    /// 射手状态快照距开火时刻的偏移（ms，负=早于开火；|值|大 = type=10 稀疏）
    pub shooter_state_dt_ms: i32,
    /// 射手位置为炮口坐标兜底（type=10 快照缺失，AoI 裁剪；见 ShooterAimData 注）
    #[serde(skip_serializing_if = "is_false")]
    pub shooter_pos_from_muzzle: bool,
    /// 目标状态采样距命中包时刻的偏移（ms，≤0 = 状态为命中批次前最后已知值）；脱靶弹无目标 = None。
    /// 命中弹的状态来源 = method8 通知处理时刻受击者的运行状态（WI 对齐锚点）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_state_dt_ms: Option<i32>,
    /// 炮塔朝向降级为车体朝向（type=7 prop2 缺失）："shooter" / "target"
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub turret_degraded: Vec<String>,
    /// 命中弹未在血量链找到伤害区间（服务器未记账 HP，damage 可能为 0）
    #[serde(skip_serializing_if = "is_false")]
    pub dmg_unattributed: bool,
    /// 锚点快照来源（仅降级/回退路径序列化，供 UI 徽章告警；正常路径不序列化）：
    /// - shooter："nearest"=最近包（AoI 稀疏）、"extrapolated"=段末速度外推；"filtered"（段内插值）=正常不序列化；
    /// - target："nearest"/"filtered"/"extrapolated" 均为 method8 命中通知缺失后的回退路径；
    ///   "wi_hit_state"（method8 通知状态）= 正常路径不序列化。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shooter_anchor_src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_anchor_src: Option<String>,
    /// 弹种来自 method0x07 开火广播兜底（type=32 命中通知未转发）；false = segment 权威来源
    #[serde(skip_serializing_if = "is_false")]
    pub shell_from_broadcast: bool,
    /// 射手炮管俯仰由发射速度向量推算（prop2 缺失回退；作者路径恒 false——作者回退走 prop9）
    #[serde(skip_serializing_if = "is_false")]
    pub shooter_pitch_from_velocity: bool,
    /// 射手炮管俯仰回退到 avatar prop9（瞄准角，非炮管物理角；仅作者路径 prop2 缺失时）
    #[serde(default, skip_serializing_if = "is_false")]
    pub shooter_pitch_from_prop9: bool,
    /// 炮管俯仰回退（prop2 frac 不可得）："shooter"=射手回退速度向量/prop9，"target"=受击方回退车体 pitch
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gun_pitch_degraded: Vec<String>,
    /// 俯仰采样流陈旧（prop2 断流 >2s，AoI 边界/补发簇；frac 恒定本身是炮管定点/贴
    /// 极限的如实上报，不算冻结）："shooter" / "target"
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pitch_frozen: Vec<String>,
}

/// 客户端渲染层锚点（逆向文档 §七）：客户端位置滤波器（WGVehicleFilter2 核心 = OSS AvatarFilterHelper 移植，
/// 见 replay/filter.rs）在事件时刻所在帧的输出 = 游戏画面里模型实际呈现的位姿。
/// 与判定层锚点（method8 通知状态）的距离 = 渲染滞后（latency≈0.1~0.2s）× 目标速度 + 路径点平滑差。
/// 两层语义用途不同：装甲命中几何用判定层（shooter_pos/target_pos），"玩家当时看到的"用渲染层。
/// 滤波器输出时间线采样（渲染层严格对齐用）：滤波器（filter.rs）在
/// `锚点时刻 + dt` 帧的实际显示位姿，pos 为绝对世界坐标（与原始采样同约定）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RenderTimelineSample {
    pub dt: f32,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderAnchorData {
    /// 渲染位置（回放世界系，米）
    pub pos: [f32; 3],
    /// 渲染朝向 [yaw, pitch, roll]（roll = 原始 volatile 插值，滤波层不输出侧倾）
    pub ang: [f32; 3],
    /// 渲染时刻的滤波器延迟（秒）：渲染的是 output_time = 帧时刻 − latency 时刻的位置
    pub latency: f32,
    /// 渲染锚点与判定层锚点的 3D 距离（米）
    pub dist_to_judgment: f32,
}


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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatTimeline {
    pub events: Vec<CombatEvent>,
    pub entity_count: usize,
    pub death_count: usize,
    pub total_damage_tracked: u32,
    pub entity_names: HashMap<u32, String>,
}

/// 从 type=5 数据包提取"实体 ID → 昵称"映射；昵称是载荷偏移 57 处的长度前缀 ASCII 串（1B 长度 + 字符串）。
fn extract_entity_names(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for (pkt_type, _, payload) in packets {
        if *pkt_type != 5 || payload.len() < 60 {
            continue;
        }
        let eid = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
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
    pub fn parse_packets(packets: &[(u32, f32, &[u8])]) -> Self {
        let entity_names = extract_entity_names(packets);
        let mut events = Vec::new();
        // 血量缓存以 type=5 满血锚点预置：否则受害者首个 prop3 事件的降幅恒为 0（首刀不进掉血表）
        let mut entity_health: HashMap<u32, u16> = collect_initial_hp(packets).into_iter()
            .map(|(k, (_, hp))| (k, hp)).collect();
        let mut death_entities = std::collections::HashSet::new();

        for (pkt_type, clock, payload) in packets {
            if *pkt_type != 7 || payload.len() < 8 {
                continue;
            }

            let entity_id = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            let sub_type = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let entity_name = entity_names.get(&entity_id).cloned().unwrap_or_else(|| format!("0x{:08x}", entity_id));

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
                        event_type: CombatEventType::HealthUpdate { health, damage_taken },
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
                        event_type: CombatEventType::DamageCounter { cumulative_damage: cum_dmg },
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

    /// 推断"每发射击"：作者伤害计数器（sub=10）递增即一次开炮命中（差值=本次伤害），再在同一时刻附近找血量下降的敌方实体作为目标。
    pub fn infer_shots(&self, author_eid: u32) -> Vec<ShotEvent> {
        let health = self.health_timeline();
        let deaths = self.death_events();

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
            let window_lo = prev_dc_time.max(*dc_time - 3.0);
            let nearby: Vec<&(f32, u32, String, u16, u16)> = health.iter()
                .filter(|(t, eid, _, _, _)| {
                    *t > window_lo && *t <= *dc_time + 0.05 && *eid != author_eid
                })
                .collect();

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

/// type=10 采样（受击坦克锚命中时刻 / 射手坦克锚开火时刻，渲染多 tick 幽灵框）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickSample {
    /// 相对锚点时刻的偏移（秒）——受击窗锚命中、射手窗锚开火，两侧秒轴不重合
    pub dt: f32,
    /// 服务器竞技场 tick 计数器（type=35，10Hz u8 回绕展开）@ 本采样包 clock：双方窗口唯一对齐键（同一 tick 编号 = 同一服务器时刻），跨侧对齐用编号不用各自 dt 秒偏移。
    pub tick: f32,
    /// 相对命中通知状态锚点（target_pos；method8 缺失回退 = 命中时刻位置）的位置偏移（世界系，米）
    pub pos: [f32; 3],
    /// 车体偏航（弧度）
    pub yaw: f32,
    /// 车体俯仰（弧度）
    pub pitch: f32,
    /// 车体侧倾（弧度）
    pub roll: f32,
    /// 合成的"游戏渲染位"采样（非 type=10 原始包）：位置滤波器（replay/filter.rs）在
    /// 锚点时刻所在帧的输出 = 游戏画面里模型实际呈现的位姿（渲染层锚点，filter.rs /《游戏回放数据处理分析报告.md》§7；roll 按原始 volatile 插值）。
    /// pos 相对锚点同上；仅 viewer 下拉展示用，不参与判定。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub render: bool,
}

impl TickSample {
    /// 原始 type=10 采样
    fn raw(dt: f32, tick: f32, pos: [f32; 3], yaw: f32, pitch: f32, roll: f32) -> Self {
        Self { dt, tick, pos, yaw, pitch, roll, render: false }
    }
    /// 合成的渲染层采样（dt=锚点帧偏移；tick=NaN → viewer 标签走渲染位分支）。
    /// roll = 原始 volatile 插值（滤波层不输出侧倾），保证与滑块时间线姿态一致。
    fn render_ghost(dt: f32, pos: [f32; 3], yaw: f32, pitch: f32, roll: f32) -> Self {
        Self { dt, tick: f32::NAN, pos, yaw, pitch, roll, render: true }
    }
}

/// 一次射击事件的"复现数据"：双方位置/朝向（type=10 实体状态包解码），供 3D 查看器复现热力图视角。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotReplayData {
    pub index: usize,
    pub time_s: f32,
    pub damage: u32,
    pub target_name: String,
    pub is_kill: bool,
    /// 射手实体 id（method29 shooterEntityId；作者路径恒为作者本人实体）
    pub shooter_eid: u32,
    /// 射手昵称（type=5 名册；未知为空串）
    #[serde(skip_serializing_if = "String::is_empty")]
    pub shooter_name: String,
    /// 是否回放作者本人的射击。false = 其他玩家：命中结果来自 method8 枚举 + 血量链伤害（无 method38 细节）
    #[serde(skip_serializing_if = "is_false")]
    pub is_author: bool,
    /// 射手（作者玩家实体）位置 [x, y(离地), z]
    pub shooter_pos: [f32; 3],
    /// 射手朝向（3 个浮点，语义为最可能猜测：偏航/俯仰/侧倾，弧度）
    pub shooter_ang: [f32; 3],
    pub target_pos: [f32; 3],
    pub target_ang: [f32; 3],
    /// 目标炮塔绝对朝向（弧度，与 hull yaw 同参考系，顺时针为正）；来源 type=7 sub=2 高 10 位粗值，
    /// ang = (u16>>6)/1024×2π − π（实测校准有 180° 偏移；低 6 位 = 炮管俯仰比例，不参与偏航）。
    pub target_turret_yaw: f32,
    /// 受击方炮管俯仰（弧度，炮塔系，正=仰角）；来源 prop2 低 6 位 frac：
    /// pitch = ele − frac/63×(dep+ele)（按车型极限锚定，frac=63 ↔ 俯角极限、0 ↔ 仰角极限）。
    /// 回退（无 prop2 采样/无车型极限）：车体 pitch（type10，正=车头下坡，语义不同仅兜底），
    /// 见 quality.gun_pitch_degraded。流断流 >2s 时 quality.pitch_frozen 提示陈旧。
    pub target_gun_pitch: f32,
    /// 存在 type=32 服务器解码的抵达成角（来向方位角校验通过，正=仰角）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub target_gun_pitch_server: bool,
    /// 恒 0（占位兼容字段）。type=32 通知包内**无**受击者炮塔角——26/27B 的
    /// 字节9=每实体序号、26B 字节11=cmpIndex、27B 字节10(7bit) 均非炮塔角
    /// （2026-09-23 五回放验证，逆向分析 §10.4）；主用 target_turret_yaw（prop2）。
    pub type32_turret_yaw: f32,
    /// 射手炮塔绝对朝向（弧度）= sub2_rel + shooter_hullYaw，用于精确入射方位角（替代位置差推算）。
    pub shooter_turret_yaw: f32,
    /// 射手炮管俯仰（弧度，炮塔系，正=仰角）；来源与受击方同源 = prop2 frac 解码（按射手车型极限）。
    /// 回退：作者 = avatar prop9（瞄准角，狙击模式下≈炮管角，quality.shooter_pitch_from_prop9）；
    /// 他人 = 发射速度向量反解（quality.shooter_pitch_from_velocity）。
    pub shooter_gun_pitch: f32,
    /// 弹着点相对【命中通知状态目标位置】的偏移 [x, y, z]（米）；来源 type=8 method20（shotId 配对）。
    /// 注意：method20 为弹道终点（穿透后出射/停止点，可在目标另一侧）。
    pub aim_point: [f32; 3],
    /// 炮口位置相对【命中通知状态目标位置】的偏移（与 aim_point 同基准）；viewer 与 launch_velocity 组合成弹道射线。
    pub launch_point_rel: [f32; 3],
    /// 弹道两点（回放世界系，米）：ball_a = 炮口发射位置（method29），ball_b = 弹道终点（method20，shotId 配对）；
    /// 两点确定弹道直线——viewer 用方向做 raycast，轴映射只需一次方向变换。
    pub ball_a: [f32; 3],
    pub ball_b: [f32; 3],
    /// 发射速度向量 [vx, vy, vz]（m/s）；method29 launchVelocity 服务器权威弹道方向（含俯仰），
    /// 与 launchPoint→终点连线夹角实测 <0.1°。
    pub launch_velocity: [f32; 3],
    /// 命中结果位图（u32 = flags16 | headerHi16<<16；wotinspector hit_flags 同源）。已实证位：
    /// 0x0001 直接击杀 / 0x0008 跳弹 / 0x0010 材料击穿 / 0x0020 未击穿（材料止）/ 0x0040 间隙层被穿透 /
    /// 0x0080 间隙层未穿 / 0x0100 内部模块被击穿 / 0x0400 履带受损 / 0x0800 火炮受损 / 0x1000 HE 爆炸伤害分支；
    /// 0x20000 = headerHi 基础位（wotinspector 样本所有非零 hit_flags 均含此位，本地 4 个回放 headerHi 恒 0x0002 ✓）。
    pub hit_flags: u32,
    /// 模块受损位掩码——wotinspector crit_modules 同源；bit = componentToken - 31（token 31..43 → bit 0..12；实测 token33 受损 → WI bit2=0x04 ✓）。
    pub crit_modules: u32,
    /// 模块摧毁位掩码（映射同上，state=2；实测 token35 履带摧毁 → bit4）。
    pub destroyed_modules: u32,
    /// 游戏原生命中段 u64（type=32 警告包尾 8 字节 LE）：`[result u8][shell_global_id u24 LE][0x00][X][Y][Z]`。
    /// - result：命中结果枚举（同 game_hit_result）。
    /// - shell_global_id（u24 LE）= (shells.xml 局部 id << 8) | 国家基数（nation_id×16+10：uk=0x5a、japan=0x6a、usa=0x2a）。
    ///   实测：GSOR AP 2040 → 522330、金HE 2039 → 522074、Type2605 112 → 28778、XM551 2018 → 516650，与 WI 逐发一致 ✓。
    /// - 字节4 恒 0x00；末 3 字节语义未解（WI segment = [result][layer][hash6] 亦不含——服务器不下发片元编号）；仅转发部分警告（GB109 覆盖 7/10），0 = 未获取。
    pub segment: u64,
    /// 命中弹种全局 id（24 位，含国家基数字节；与 WI shell_id 同值同源；0 = 未获取）
    pub shell_id: u32,
    /// segment[7]（B7）= 命中装甲板 plateId——五回放 15 型号 27 发渲染位姿 raycast
    /// 对照 23 匹配/3 相邻板/1 miss（逆向分析 §10.2）；viewer 片元验证 fragOk 用同源值
    pub armor_group: u8,
    /// segment[5..7] 组成的 u16 BE——**非三角形索引（已证伪）**，res=1 未穿时恒负值
    /// （可作未穿判定冗余位）、res=0 恒 ~71-83；参照系未解，保留透传
    pub hit_triangle: u16,
    /// 游戏命中结果枚举（method8 b9 / type=32 segment 低字节同源，86/86 事件实测一致）：
    /// 0=无命中结果 1=未击穿 2=间隙层止 3=有伤害（击穿/HE 爆炸）4=履带/模块交互
    /// （**非纯跳弹**——1436 五回放交叉表：b9=4 9/9 伴随内部模块穿旗标，含履带穿透带全额伤害、
    /// 履带吸收零伤与击杀穿透，与 0x0001/伤害共存证伪"跳弹"旧标注）；255 = 未获取。
    pub game_hit_result: u8,
    /// method8 ↔ type=32 同事件共享的 6 字节（86/86 一致）。
    /// 语义 = 游戏客户端 DecodeShotSegment 两点编码：出入点的部件 AABB 量化坐标
    /// （轴序/解码/盒源详见《WI 射击参数与命中位置分析》§5.1）；3D 查看器据此
    /// 标注出入点并构建 P1→P2 判定射线。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_token: Option<String>,
    /// 特殊弹药效果 ID（WotbTools PROVEN：1=精准火力 2=钨芯弹，可同发共存）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<u32>,
    /// 开火时刻（秒）——与 method29 发射包确定性匹配
    pub fire_time: f32,
    /// shotId——method29 发射 ↔ method20 终点 确定性配对键
    pub shot_id: u32,
    /// 弹药槽位——type=28 选择状态在发射时刻的值（3D 视图弹种选择器索引用）
    pub shell_slot: u32,
    /// type=35 服务器竞技场 tick 计数器 @ 开火时刻（10Hz u8 递增，回放时钟秒×10）；开火 tick 判定 100/100 实测对齐。
    pub fire_tick: f32,
    /// 受击坦克 type=10 采样（命中 ±1s，位置相对命中通知状态锚点，世界系米；dt 锚定命中包时刻）。
    /// 末项可能为合成"游戏渲染位"采样（render=true，dt=0，滤波器命中帧输出，viewer ◎渲染位）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tick_samples: Vec<TickSample>,
    /// 射手坦克 type=10 采样（开火 ±0.2s，世界系绝对坐标）；末项可能为合成渲染位采样（render=true）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shooter_tick_samples: Vec<TickSample>,
    /// 地形命中数据（Avatar method 0x1b；仅当该发未命中任何坦克且服务器广播时存在，约覆盖 2/3 地形弹）。
    /// args(34) = [shotId u32][shell_global_id u32][material u8][impactPoint 3×f32][segmentStartPoint 3×f32][tail u8]。
    /// impact_point == method20 弹道终点（4 回放逐发一致）；segment_start = 弹道末段起点（直线弹 = method29 发射点，误差 0.000m；其余为弹跳点）；
    /// material 落点材质类（观测 0/1/2/4/5，命名未定）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terrain_impact: Option<TerrainImpactData>,
    /// 开火时刻瞄准快照（Avatar method36，缺失时 None）。炮塔相对偏航为 f64 全精度（prop2 为 u16 量化）；
    /// state_before/after 为未定名状态常量（非扩散度，见 ShooterAimData 注）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shooter_aim: Option<ShooterAimData>,
    /// 兼容旧字段：= type32_turret_yaw（曾误标为"来袭方向"，实为受击者炮塔角）。
    pub incoming_yaw: f32,
    /// 兼容旧字段：= target_gun_pitch（曾误标为"来袭俯角"，实为受击者炮管俯仰）。
    pub incoming_pitch: f32,
    /// 数据质量标注（快照陈旧度/降级项；作者路径填快照偏移，他人路径另含降级标记）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<ShotQuality>,
    /// 服务器下发的受击部件索引 cmpIndex（method8 args[10]；0=底盘/履带 1=车体 2=炮塔 3=炮管，
    /// 命中高度统计实证）——游戏 showDamageFromShot/DecodeShotSegment 用它在指定部件上放
    /// 弹着点（报告 §4.7）；与本地 raycast 的部件选择对照 = 命中位置偏差的校准基准。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_part_index: Option<u8>,
    /// 射手渲染层锚点（客户端位置滤波器输出；None = type=10 采样缺失不可构建时间线）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shooter_render: Option<RenderAnchorData>,
    /// 受击者渲染层锚点（仅命中弹；"玩家当时看到的"受击者位姿）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_render: Option<RenderAnchorData>,
    /// 受击方渲染时间线（滤波器输出 0.1s 降采样，命中 −3.0~+2.0s，pos 绝对世界坐标）——
    /// 滑块严格对齐游戏每帧实际显示位姿（含 latency 移位/误差盒钳位/外推）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_render_timeline: Vec<RenderTimelineSample>,
    /// 射手方渲染时间线（开火 −2.0~+2.0s，pos 绝对世界坐标）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shooter_render_timeline: Vec<RenderTimelineSample>,
    /// 受击方炮塔相对角时间线（prop2，命中 −3.0~+2.0s，[dt, rel_yaw 弧度]）——滑块实时驱动炮塔
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_turret_timeline: Vec<(f32, f32)>,
    /// 射手方炮塔相对角时间线（prop2，开火 −2.0~+2.0s，[dt, rel_yaw 弧度]）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shooter_turret_timeline: Vec<(f32, f32)>,
    /// 射手炮管俯仰时间线（prop2 frac 解码，开火 −2.0~+2.0s，[dt, 弧度，正=仰角]）；
    /// 无车型极限锚定时作者路径回退 prop9（瞄准角，弧度）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shooter_gun_timeline: Vec<(f32, f32)>,
    /// 受击方炮管俯仰时间线（prop2 frac 解码，命中 −3.0~+2.0s，[dt, 弧度，正=仰角]）——
    /// 滑块实时驱动炮管（与 target_turret_timeline 配对）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_gun_timeline: Vec<(f32, f32)>,
}

/// method 0x1b 地形命中数据（字段语义见 [`ShotReplayData::terrain_impact`]）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainImpactData {
    /// 落点材质类（0/1/2/4/5 观测，具体命名未定）
    pub material: u8,
    /// 精确落点（回放世界系，米；== method20 弹道终点）
    pub impact_point: [f32; 3],
    /// 弹道末段起点（直线弹 = 发射点；弹跳弹 = 弹跳点）
    pub segment_start: [f32; 3],
}

/// Avatar method36 (0x24) 开火时刻瞄准快照（可选，无快照则 None）；args = [payloadLen u8][protobuf]。
/// field1(f64)=炮塔相对车体偏航（与 prop2 同语义，实测 |Δ|≤0.024 rad）；开火时刻成对出现（射击前/后各一条，f1 恒同）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShooterAimData {
    pub turret_rel_yaw: f64,
    /// 成对快照 field6.field1（射击前；跨坦克/跨发实测恒 ≈0.842——旧标注"扩散度"与实测矛盾，语义未定，透传供研究）
    pub state_before: f64,
    /// 成对快照 field6.field1（射击后；实测恒 ≈0.906；无成对快照时 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_after: Option<f64>,
}

/// protobuf 最小遍历：varint / fixed64 / 定长子消息，返回 (field_no, wire_type, 内容偏移, 内容长)；仅用于 method36 快照，格式不合法返回 None（fail-soft 调用方忽略）。
fn proto_fields(b: &[u8]) -> Option<Vec<(u32, u8, usize, usize)>> {
    fn varint(b: &[u8], mut o: usize) -> Option<(u64, usize)> {
        let mut v = 0u64;
        let mut s = 0u32;
        loop {
            let x = *b.get(o)?;
            o += 1;
            v |= ((x & 0x7f) as u64) << s;
            if x & 0x80 == 0 { return Some((v, o)); }
            s += 7;
            if s > 63 { return None; }
        }
    }
    let mut out = Vec::new();
    let mut o = 0usize;
    while o < b.len() {
        let (tag, o2) = varint(b, o)?;
        let (no, wt) = ((tag >> 3) as u32, (tag & 7) as u8);
        let (start, len) = match wt {
            0 => { let (_, o3) = varint(b, o2)?; (o2, o3 - o2) }
            1 => (o2, 8),
            5 => (o2, 4),
            2 => { let (l, o3) = varint(b, o2)?; (o3, l as usize) }
            _ => return None,
        };
        let end = start.checked_add(len)?;
        if end > b.len() { return None; }
        out.push((no, wt, start, len));
        o = end;
    }
    Some(out)
}

/// 解析 method36 args：返回 (field1 炮塔相对偏航, field6.field1 扩散度)。
fn parse_method36(args: &[u8]) -> (Option<f64>, Option<f64>) {
    if args.is_empty() { return (None, None); }
    // args[0] = payload 长度前缀（= args.len()-1），容错取 min
    let end = (args[0] as usize + 1).min(args.len());
    let payload = &args[1..end];
    let fields = match proto_fields(payload) {
        Some(f) => f,
        None => return (None, None),
    };
    let fixed64 = |b: &[u8], f: &(u32, u8, usize, usize)| {
        f64::from_le_bytes(b[f.2..f.2 + 8].try_into().unwrap())
    };
    let f1 = fields.iter().find(|f| f.0 == 1 && f.1 == 1).map(|f| fixed64(payload, f));
    let dispersion = fields.iter().find(|f| f.0 == 6 && f.1 == 2).and_then(|f| {
        let sub = &payload[f.2..f.2 + f.3];
        proto_fields(sub)?
            .iter()
            .find(|s| s.0 == 1 && s.1 == 1)
            .map(|s| fixed64(sub, s))
    });
    (f1, dispersion)
}

/// 收集每实体的 type=7 同钟多属性刷新簇时钟（AoI 补发/通道切换签名，逆向文档 6.x：
/// 锚点后多属性 <2ms 同钟成簇 = 批量补发快照）；开火/命中事件触发更新方案切换 + 状态补发，簇时钟即切换点。
fn collect_refresh_clusters(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, Vec<f32>> {
    // 每实体 (clock, sub) 排序后滑窗：窗口内出现 ≥2 个不同 sub（<2ms）→ 簇时钟
    let mut seqs: HashMap<u32, Vec<(f32, u32)>> = HashMap::new();
    for (t, clock, p) in packets {
        if *t != 7 || p.len() < 14 { continue; }
        seqs.entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
            .or_default()
            .push((*clock, u32::from_le_bytes([p[4], p[5], p[6], p[7]])));
    }
    let mut out: HashMap<u32, Vec<f32>> = HashMap::new();
    for (eid, mut seq) in seqs {
        seq.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)));
        let mut cluster_clocks: Vec<f32> = Vec::new();
        let mut i = 0usize;
        while i < seq.len() {
            let mut j = i + 1;
            let mut subs: std::collections::HashSet<u32> = std::collections::HashSet::new();
            subs.insert(seq[i].1);
            while j < seq.len() && (seq[j].0 - seq[i].0).abs() < 0.002 {
                subs.insert(seq[j].1);
                j += 1;
            }
            if subs.len() >= 2 { cluster_clocks.push(seq[i].0); }
            i = j;
        }
        if cluster_clocks.is_empty() { continue; }
        // 相邻簇时钟 <0.05s 合并取首（一次补发可能跨几毫秒的多条包）
        let mut merged: Vec<f32> = Vec::new();
        for c in cluster_clocks {
            match merged.last() {
                Some(lc) if c - *lc < 0.05 => {}
                _ => merged.push(c),
            }
        }
        out.insert(eid, merged);
    }
    out
}

/// 该实体在 (t, t+0.35] 内首个补发簇时钟相对 t 的偏移（无则 None）。
fn refresh_cluster_after(clusters: &HashMap<u32, Vec<f32>>, eid: u32, t: f32) -> Option<f32> {
    let v = clusters.get(&eid)?;
    let &c = v.iter().find(|c| **c > t && **c <= t + 0.35)?;
    Some(c - t)
}

/// type=10 状态采样（锚点选择与 tick 采样共用）。
#[derive(Debug, Clone)]
pub struct St10Sample {
    pub clock: f32,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// 位置误差盒半径（bytes [24..36]，3×f32 每实体小常数）——滤波器推测外推的钳位边界
    /// （游戏 AvatarFilterHelper Waypoint clamp 的原生用途，逆向文档 §7.3/7.4）。
    pub pos_error: [f32; 3],
}

/// 锚点状态选择 —— 客户端位置处理仿真（用户决策；逆向文档 6.3/6.4）：
/// 复现位置 = 游戏客户端渲染位置（volatile 流 + 移动滤波重建），而非最近原始包。
/// 1) 通道边界（相邻采样速度 >25 m/s = AoI 切换跳变，或其间有补发簇时钟；客户端重置滤波）禁止跨段插值；
/// 2) 段内：位置线性插值（yaw 最短弧，pitch/roll 近邻）；3) 段末：末段速度外推 ≤0.5s 超限退回最近包；4) t 早于首包/单采样 → 最近包。
/// 返回 (pos, ang, 带符号 clock 偏移, 来源 "filtered"/"extrapolated"/"nearest")。
fn select_anchor_state(
    samples: &[St10Sample],
    clusters: &HashMap<u32, Vec<f32>>,
    eid: u32,
    t: f32,
) -> Option<([f32; 3], [f32; 3], f32, &'static str)> {
    if samples.is_empty() { return None; }
    if samples.len() == 1 {
        let s = &samples[0];
        return Some((s.pos, [s.yaw, s.pitch, s.roll], s.clock - t, "nearest"));
    }

    const TELEPORT_SPEED: f32 = 25.0;   // m/s，WoTB 最高车速 ~19 m/s + 余量；超限 = AoI 通道切换跳变
    let is_boundary = |i: usize| -> bool {
        let a = &samples[i];
        let b = &samples[i + 1];
        let dt = b.clock - a.clock;
        if dt <= 0.0 { return true; }
        let d = dist3(a.pos, b.pos);
        if d / dt > TELEPORT_SPEED { return true; }
        if let Some(v) = clusters.get(&eid) {
            if v.iter().any(|c| *c > a.clock && *c < b.clock) { return true; }
        }
        false
    };

    let pos_of = |s: &St10Sample| s.pos;
    let ang_of = |s: &St10Sample| [s.yaw, s.pitch, s.roll];

    if t <= samples[0].clock {
        let s = &samples[0];
        return Some((pos_of(s), ang_of(s), s.clock - t, "nearest"));
    }
    let last = samples.len() - 1;
    if t > samples[last].clock {
        // 段末外推：末包与前包同段（无边界）时用其速度，否则原地保持
        let a = &samples[last - 1];
        let b = &samples[last];
        let dt = t - b.clock;
        if dt > 0.5 {
            let s = &samples[last];
            return Some((pos_of(s), ang_of(s), s.clock - t, "nearest"));
        }
        if !is_boundary(last - 1) && b.clock > a.clock {
            let v = [
                (b.pos[0] - a.pos[0]) / (b.clock - a.clock),
                (b.pos[1] - a.pos[1]) / (b.clock - a.clock),
                (b.pos[2] - a.pos[2]) / (b.clock - a.clock),
            ];
            let pos = [b.pos[0] + v[0]*dt, b.pos[1] + v[1]*dt, b.pos[2] + v[2]*dt];
            return Some((pos, ang_of(b), -dt, "extrapolated"));
        }
        return Some((pos_of(b), ang_of(b), b.clock - t, "nearest"));
    }

    let mut i = 0usize;
    while i + 1 < samples.len() && samples[i + 1].clock < t { i += 1; }
    let a = &samples[i];
    let b = &samples[i + 1];
    if is_boundary(i) {
        // 跨边界：不可插值（客户端在边界处跳变/重置），取时间近者
        let s = if (t - a.clock).abs() <= (b.clock - t).abs() { a } else { b };
        return Some((pos_of(s), ang_of(s), s.clock - t, "nearest"));
    }
    // 段内插值（客户端滤波的延迟插值渲染模型）
    let f = (t - a.clock) / (b.clock - a.clock);
    let pos = [
        a.pos[0] + (b.pos[0] - a.pos[0]) * f,
        a.pos[1] + (b.pos[1] - a.pos[1]) * f,
        a.pos[2] + (b.pos[2] - a.pos[2]) * f,
    ];
    // yaw 最短弧插值
    let mut dyaw = b.yaw - a.yaw;
    if dyaw > std::f32::consts::PI { dyaw -= std::f32::consts::TAU; }
    if dyaw < -std::f32::consts::PI { dyaw += std::f32::consts::TAU; }
    let ang = [a.yaw + dyaw * f, a.pitch + (b.pitch - a.pitch) * f, a.roll + (b.roll - a.roll) * f];
    Some((pos, ang, 0.0, "filtered"))
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0]-b[0]).powi(2) + (a[1]-b[1]).powi(2) + (a[2]-b[2]).powi(2)).sqrt()
}

// =====================================================================
//  UpdateArena 竞技场状态流（wotblitz.exe 二进制实证，见《游戏回放数据处理分析报告.md》§4.5）：
//  Avatar 方法 method=48 (0x30)，args[0] = 子类型 ID（1..=27，名字表 @VA 0x40391D8，
//  首字节 dec 后查表跳转），其后为 WG protobuf-lite 消息体（0xFF 转义 u24 字段号扩展）。
// =====================================================================

/// Avatar updateArena 的 methodID（method 流直方图 + 子类型 ID 分布双重实证）
pub const ARENA_UPDATE_METHOD: u32 = 48;

/// 子类型名（二进制名字表逐项导出；10=former_teamkills 为原表小写原名）
pub fn arena_subtype_name(id: u32) -> &'static str {
    match id {
        1 => "VEHICLE_LIST", 2 => "VEHICLE_ADDED", 3 => "PERIOD",
        4 => "STATISTICS", 5 => "VEHICLE_STATISTICS", 6 => "VEHICLE_KILLED",
        7 => "AVATAR_READY", 8 => "BASE_POINTS", 9 => "BASE_CAPTURED",
        10 => "former_teamkills", 11 => "VEHICLE_UPDATED", 12 => "STRATEGIC_POINT_STATUS",
        13 => "WIN_POINTS", 14 => "PLAYER_NAME", 15 => "RELOAD_TIME",
        16 => "OBSERVED_STATUS", 17 => "RELOAD_TIME_LIST", 18 => "GAME_MODE_DATA",
        19 => "VEHICLE_WAIT_RESPAWN", 20 => "VEHICLE_RESURRECT", 21 => "TEAM_RESPAWNS_LEFT",
        22 => "VAMPIRIC_CURSE", 23 => "BATTLE_HINTS_INFO", 24 => "BOSSMODE_INFO",
        25 => "TOTAL_GAME_MODE_INFO", 26 => "TIER_EQUALIZER_DATA", 27 => "UNKNOWN_TYPE",
        _ => "UNKNOWN",
    }
}

/// 一条 updateArena 更新（子类型 + 原始消息体；字段级解码按子类型另行解析）。
/// args 布局 = [subtype u8][len u8][protobuf]（len = 其后字节数，实测逐条吻合）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaUpdate {
    pub clock: f32,
    pub subtype: u32,
    pub name: &'static str,
    /// protobuf 消息体（hex；RELOAD_TIME 高频 ~20B/条，JSON 体量可控）
    pub payload_hex: String,
}

/// 收集 updateArena 流（作者 Avatar 实体广播；类型=8 方法流，methodID=48）。
/// args 布局 = [subtype u8][len u8][protobuf]；len 不符的包按健壮路径仍从偏移 2 解析
pub fn collect_arena_updates(packets: &[(u32, f32, &[u8])]) -> Vec<ArenaUpdate> {
    let mut out = Vec::new();
    for (_t, clock, p) in packets {
        if p.len() < 15 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != ARENA_UPDATE_METHOD { continue; }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if 12 + alen > p.len() || alen < 2 { continue; }
        let subtype = p[12] as u32;
        out.push(ArenaUpdate {
            clock: *clock,
            subtype,
            name: arena_subtype_name(subtype),
            payload_hex: p[14..12 + alen].iter().map(|x| format!("{:02x}", x)).collect(),
        });
    }
    out
}

/// PERIOD (subtype=3) 解析结果：战局阶段时间线
/// 消息体 = protobuf field3 嵌套 { field1 varint: period, field2 fixed64: 阶段剩余秒, field3 varint: 阶段时长 }
/// 实测 J39：period 1=准备(60s) → 2=倒计时(7s) → 3=战斗(duration=420s)；
/// 剩余秒与包时刻互洽（t=0.17 时准备期剩 59.4s）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaPeriod {
    pub clock: f32,
    pub period: u64,
    /// 阶段剩余秒（f64）
    pub remaining_s: f64,
    pub duration_s: u64,
}

/// 从 updateArena 流解析 PERIOD 时间线（fail-soft：解析失败的单条跳过）
pub fn parse_arena_periods(updates: &[ArenaUpdate]) -> Vec<ArenaPeriod> {
    let mut out = Vec::new();
    for u in updates {
        if u.subtype != 3 { continue; }
        let b = match decode_hex(&u.payload_hex) { Some(b) => b, None => continue };
        // 顶层 field3 (tag 0x1a) 长度前缀嵌套
        let nested = match find_field(&b, 3) { Some(n) => n, None => continue };
        let period = read_varint(nested, 1);
        let remaining = read_fixed64(nested, 2);
        let duration = read_varint(nested, 3);
        if let (Some(period), Some(remaining)) = (period, remaining) {
            out.push(ArenaPeriod { clock: u.clock, period, remaining_s: remaining, duration_s: duration.unwrap_or(0) });
        }
    }
    out
}

/// protobuf 最小读取助手（wire 解析；仅用于 ArenaPeriod，格式不合法返回 None）
fn find_field(b: &[u8], want: u32) -> Option<&[u8]> {
    let mut i = 0usize;
    while i < b.len() {
        let tag = b[i];
        i += 1;
        let field = (tag >> 3) as u32;
        let wire = tag & 7;
        match wire {
            0 => { while i < b.len() && b[i] & 0x80 != 0 { i += 1; } i += 1; }
            1 => i += 8,
            2 => {
                let mut len = 0usize;
                let mut shift = 0u32;
                while i < b.len() {
                    len |= ((b[i] & 0x7f) as usize) << shift;
                    shift += 7;
                    let cont = b[i] & 0x80 != 0;
                    i += 1;
                    if !cont { break; }
                }
                if i + len > b.len() { return None; }
                if field == want { return Some(&b[i..i + len]); }
                i += len;
            }
            5 => i += 4,
            _ => return None,
        }
    }
    None
}

fn field_value(b: &[u8], want: u32) -> Option<(&[u8], u8)> {
    let mut i = 0usize;
    while i < b.len() {
        let tag = b[i];
        i += 1;
        let field = (tag >> 3) as u32;
        let wire = tag & 7;
        match wire {
            0 => {
                let start = i;
                while i < b.len() && b[i] & 0x80 != 0 { i += 1; }
                i += 1;
                if field == want && i <= b.len() { return Some((&b[start..i], 0)); }
            }
            1 => { if i + 8 > b.len() { return None; } if field == want { return Some((&b[i..i + 8], 1)); } i += 8; }
            2 => {
                let mut len = 0usize;
                let mut shift = 0u32;
                while i < b.len() {
                    len |= ((b[i] & 0x7f) as usize) << shift;
                    shift += 7;
                    let cont = b[i] & 0x80 != 0;
                    i += 1;
                    if !cont { break; }
                }
                if i + len > b.len() { return None; }
                if field == want { return Some((&b[i..i + len], 2)); }
                i += len;
            }
            5 => { if i + 4 > b.len() { return None; } if field == want { return Some((&b[i..i + 4], 1)); } i += 4; }
            _ => return None,
        }
    }
    None
}

fn read_varint(b: &[u8], field: u32) -> Option<u64> {
    let (v, wire) = field_value(b, field)?;
    if wire != 0 { return None; }
    let mut val = 0u64;
    let mut shift = 0u32;
    for &x in v {
        val |= ((x & 0x7f) as u64) << shift;
        shift += 7;
        if x & 0x80 == 0 { return Some(val); }
    }
    None
}

fn read_fixed64(b: &[u8], field: u32) -> Option<f64> {
    let (v, wire) = field_value(b, field)?;
    if wire != 1 || v.len() != 8 { return None; }
    Some(f64::from_le_bytes(v.try_into().ok()?))
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 { return None; }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// **method8 元素后 6 字节（旧称 hash6/"命中令牌"）的结构（2026-09 恢复）**：
/// [shell u16][来向 yaw u16][抵达 pitch u16]——结构化弹道数据，见
/// `decoded_target_gun_pitch`（yaw/pitch 的 (u16−32768)/32768 比例尺解码 + 来向方位角
/// ≤15° 校验 + |pitch|≤30° 有效域，校验通过者入 target_gun_pitch）。


#[cfg(test)]
mod arena_tests {
    use super::*;

    /// J39 真实 PERIOD 包（subtype=3）：period=3(战斗)、剩余 420.0s、时长 420s
    #[test]
    fn arena_period_decode() {
        let u = ArenaUpdate {
            clock: 18.727,
            subtype: 3,
            name: "PERIOD",
            payload_hex: "1a0e0803110000000000407a4018a403".into(),
        };
        let periods = parse_arena_periods(&[u]);
        assert_eq!(periods.len(), 1);
        let p = &periods[0];
        assert_eq!(p.period, 3);
        assert!((p.remaining_s - 420.0).abs() < 1e-9, "remaining={}", p.remaining_s);
        assert_eq!(p.duration_s, 420);
    }

    /// 子类型名表完整性（二进制名字表 27 项）
    #[test]
    fn arena_subtype_names() {
        assert_eq!(arena_subtype_name(1), "VEHICLE_LIST");
        assert_eq!(arena_subtype_name(3), "PERIOD");
        assert_eq!(arena_subtype_name(6), "VEHICLE_KILLED");
        assert_eq!(arena_subtype_name(10), "former_teamkills");
        assert_eq!(arena_subtype_name(17), "RELOAD_TIME_LIST");
        assert_eq!(arena_subtype_name(27), "UNKNOWN_TYPE");
        assert_eq!(arena_subtype_name(28), "UNKNOWN");
    }
}

/// 渲染层锚点（客户端位置滤波器，《游戏回放数据处理分析报告.md》§7）：per-entity 滤波器时间线惰性构建 + 事件时刻所在帧的滤波输出。
/// `at_or_after=true` 语义 = 包在该帧网络泵处理后于当帧渲染（游戏客户端帧循环次序）。
/// 时间线 per-entity 只建一次（60Hz × 战斗时长，每实体 ~1MB）。
fn render_anchor(
    cache: &mut HashMap<u32, FilteredTimeline>,
    eid: u32,
    samples: Option<&Vec<St10Sample>>,
    t: f32,
    judgment_pos: [f32; 3],
) -> Option<RenderAnchorData> {
    let samples = samples?;
    if samples.is_empty() { return None; }
    if !cache.contains_key(&eid) {
        cache.insert(eid, FilteredTimeline::build(samples)?);
    }
    let tl = cache.get(&eid)?;
    let pose = tl.pose_at(t as f64, true)?;
    // roll 滤波层不输出（视觉侧倾来自物理层），按原始 volatile 线性插值补齐——
    // 与 render_timeline 同规则。恒 0 会使加载摆放（渲染位采样）与滑块时间线的
    // 车体侧倾不一致，炮塔偏航下炮管仰角漂移 ~5.6°（T110 1436 shot4 实测）。
    let mut ang = pose.ang;
    {
        let mut prev = &samples[0];
        let t64 = t as f64;
        for s in samples.iter() {
            if (s.clock as f64) >= t64 {
                let span = s.clock as f64 - prev.clock as f64;
                let f = if span > 1e-6 { ((t64 - prev.clock as f64) / span) as f32 } else { 0.0 };
                ang[2] = prev.roll + (s.roll - prev.roll) * f;
                break;
            }
            prev = s;
        }
    }
    Some(RenderAnchorData {
        pos: pose.pos,
        ang,
        latency: pose.latency,
        dist_to_judgment: dist3(pose.pos, judgment_pos),
    })
}

/// 滤波时间线降采样：[base+dt_from, base+dt_to] 步长 step 的滤波器输出（渲染层严格对齐，
/// 滑块用）。pos 为绝对世界坐标；roll 按原始 volatile 插值。时间线 per-entity 惰性构建并缓存。
fn render_timeline(
    cache: &mut HashMap<u32, FilteredTimeline>,
    eid: u32,
    samples: Option<&Vec<St10Sample>>,
    base_t: f32,
    dt_from: f32,
    dt_to: f32,
    step: f32,
) -> Vec<RenderTimelineSample> {
    let Some(samples) = samples.filter(|s| !s.is_empty()) else { return Vec::new() };
    if !cache.contains_key(&eid) {
        match FilteredTimeline::build(samples) {
            Some(tl) => { cache.insert(eid, tl); }
            None => return Vec::new(),
        }
    }
    let Some(tl) = cache.get(&eid) else { return Vec::new() };
    // roll 不经滤波层（视觉侧倾来自物理层），但原始 volatile 携带车体/地形侧倾，
    // 置 0 会丢姿态（T110 1436 shot4 实证：13° roll 使 1.95m 高的炮闩枢轴横移
    // 0.44m，炮闩标注与 launchpoint 永不对齐）——按原始采样线性插值补齐。
    let roll_at = |t: f64| -> f32 {
        if samples.is_empty() { return 0.0; }
        let mut prev = &samples[0];
        for s in samples.iter() {
            if (s.clock as f64) >= t {
                let span = s.clock as f64 - prev.clock as f64;
                let f = if span > 1e-6 { ((t - prev.clock as f64) / span) as f32 } else { 0.0 };
                return prev.roll + (s.roll - prev.roll) * f;
            }
            prev = s;
        }
        prev.roll
    };
    // 只输出时间线覆盖范围内（该实体首/末 volatile 之间）的采样：
    // 覆盖范围外 = 客户端尚无/已无该实体数据（AoI 外不渲染），交给前端隐藏模型
    let (cov_start, cov_end) = tl.time_range();
    let mut out = Vec::new();
    let mut dt = dt_from;
    while dt <= dt_to + 1e-6 {
        let t = (base_t + dt) as f64;
        if t >= cov_start - 1e-6 && t <= cov_end + 1e-6 {
            if let Some(pose) = tl.pose_at(t, true) {
                out.push(RenderTimelineSample {
                    dt,
                    // pos 为绝对世界坐标（与原始采样 shooter_tick_samples 同约定，前端零换算直通）
                    pos: pose.pos,
                    yaw: pose.ang[0],
                    pitch: pose.ang[1],
                    roll: roll_at(t),
                });
            }
        }
        dt += step;
    }
    out
}

/// 标量时间线降采样：[base+from, base+to] 内的采样 → (dt, 值)（prop2 炮塔角 / prop9 俯仰用）
fn timeline_1f(
    series: Option<&Vec<(f32, f32)>>,
    base: f32,
    from: f32,
    to: f32,
) -> Vec<(f32, f32)> {
    let Some(list) = series else { return Vec::new() };
    list.iter()
        .filter(|(c, _)| *c >= base + from && *c <= base + to)
        .map(|(c, v)| (c - base, *v))
        .collect()
}

/// tick 采样统一截断（逆向文档 6.2/6.3）：有真锚点（|dt|<0.05）→ 只保留锚点及之前（其后包属新通道）；
/// 无锚点但有补发簇签名 → 保留簇时钟之前（原 +0.09 兜底包可能已属新通道，按簇时钟收紧）；
/// 无锚点无签名 → 保持原行为（≤0.09 兜底包近似锚点）。
fn trim_tick_samples(samples: &mut Vec<TickSample>, cluster_dt: Option<f32>) {
    let has_anchor = samples.iter().any(|s| s.dt.abs() < 0.05);
    if !has_anchor && cluster_dt.is_none() { return; }
    let mut cut = if has_anchor { 0.05f32 } else { 0.09 };
    if let Some(cd) = cluster_dt { cut = cut.min((cd - 0.02).max(0.0)); }
    samples.retain(|s| s.dt < cut);
    samples.sort_by(|a, b| a.dt.partial_cmp(&b.dt).unwrap());
}

/// 全链原始流导出（WI 对齐扫描 / 探针用）：per-entity type=10 状态流 + type=7 prop2 流 +
/// 全 shooter 的 method29 发射 / method20 终点 / method8 直击通知。
pub fn dump_replay_streams(packets: &[(u32, f32, &[u8])]) -> serde_json::Value {
    let (mut st10, prop2) = build_entity_indexes(packets);
    for v in st10.values_mut() {
        v.sort_by(|a, b| a.clock.partial_cmp(&b.clock).unwrap());
    }
    let st10_json: serde_json::Map<String, serde_json::Value> = st10.into_iter()
        .map(|(eid, v)| (format!("{eid:08x}"), serde_json::Value::Array(v.into_iter()
            .map(|s| serde_json::json!([s.clock, s.pos[0], s.pos[1], s.pos[2], s.yaw, s.pitch, s.roll]))
            .collect())))
        .collect();
    let prop2_json: serde_json::Map<String, serde_json::Value> = prop2.into_iter()
        .map(|(eid, v)| (format!("{eid:08x}"), serde_json::Value::Array(v.into_iter()
            .map(|(c, r, fr)| serde_json::json!([c, r, fr]))
            .collect())))
        .collect();
    let (launches, _) = collect_launches(packets, |_| true);
    let endpoints = collect_endpoints(packets);
    // 文件序事件序列（state-machine 扫描用）：volatile/launch/endpoint/direct8 四类事件按包内出现顺序
    let mut seq: Vec<serde_json::Value> = Vec::new();
    for (t, clock, p) in packets {
        if *t == 10 && p.len() >= 48 {
            let f = |o: usize| f32::from_le_bytes([p[o], p[o+1], p[o+2], p[o+3]]);
            seq.push(json!(["v", clock, u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                f(12), f(16), f(20)]));
            continue;
        }
        if p.len() < 12 { continue; }
        let m = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if p.len() < 12 + args_len { continue; }
        let a = &p[12..12 + args_len];
        let f = |o: usize| f32::from_le_bytes([a[o], a[o+1], a[o+2], a[o+3]]);
        if *t == 8 && m == 0x1d && args_len >= 37 {
            seq.push(json!(["l", clock, u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                u32::from_le_bytes([a[4], a[5], a[6], a[7]]),
                f(9), f(13), f(17)]));
        } else if *t == 8 && m == 0x14 && args_len >= 16 {
            seq.push(json!(["e", clock, u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                f(4), f(8), f(12)]));
        } else if *t == 8 && m == 0x08 && args_len >= 10 && a[8] == 1 {
            seq.push(json!(["h", clock, u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                u32::from_le_bytes([a[4], a[5], a[6], a[7]])]));
        } else if *t == 8 && m == 0x00 {
            // method0x00 开火事件：envelope entityId = 射手车辆实体，args=[01]
            seq.push(json!(["f", clock, u32::from_le_bytes([p[0], p[1], p[2], p[3]])]));
        }
    }
    use serde_json::json;
    json!({
        "st10": st10_json,
        "prop2": prop2_json,
        "sequence": seq,
        "launches": launches.iter().map(|l| json!({
            "t": l.t, "shooter": l.shooter, "shot_id": l.shot_id,
            "point": l.point, "vel": l.vel,
        })).collect::<Vec<_>>(),
        "endpoints": endpoints.iter().map(|(sid, (t, p))|
            json!({"shot_id": sid, "t": t, "p": p})).collect::<Vec<_>>(),
        "direct_hits8": collect_direct_hits8(packets).iter().map(|d| json!({
            "t": d.t, "shooter": d.shooter, "victim": d.victim, "result": d.result,
        })).collect::<Vec<_>>(),
    })
}

/// type=10 相邻快照段的运动学检验（回放流含服务器纠偏"倒车滑移"段，速度可达真实极限 2~3 倍且弹道几何不可达）。
/// 坦克约束：倒车 ≤5.5 m/s（全游戏上限）、侧移 ≤5.0、前向 ≤30、加速度 ≤8 m/s²；位移 <0.25 m 微跳不截断（避免误杀厘米级纠偏）。
/// [已撤回] 实测会把真实倒车（LT-432 等轻坦倒车极速 >20km/h，超 5.5m/s 阈值）整段误杀导致 tick 切换丢失；
/// 用户决定完全按回放原始数据渲染，不再调用，函数体保留供参考。
#[allow(dead_code)]
fn seg_speed(a: &TickSample, b: &TickSample) -> (f32, f32, f32, f32) {
    let dt = (b.dt - a.dt).abs().max(1e-3);
    let dx = b.pos[0] - a.pos[0];
    let dz = b.pos[2] - a.pos[2];
    let v = (dx * dx + dz * dz).sqrt() / dt;
    let mut err = (dx.atan2(dz) - b.yaw).rem_euclid(std::f32::consts::TAU);
    if err > std::f32::consts::PI { err -= std::f32::consts::TAU; }
    (v, err.abs(), dt, (dx * dx + dz * dz).sqrt())
}

#[allow(dead_code)]
fn cut_needed(v: f32, err: f32, disp: f32, prev_v: Option<(f32, f32)>) -> bool {
    if disp < 0.25 || v <= 0.5 { return false; }
    let hard = if err < 45.0f32.to_radians() {
        v > 30.0
    } else if err > 135.0f32.to_radians() {
        v > 5.5
    } else {
        v > 5.0
    };
    if hard { return true; }
    // 反向/侧向段附加加速度检验：坦克加/制动 ≤8 m/s²，纠偏滑移远超此限
    if err > 45.0f32.to_radians() {
        if let Some((pv, pdt)) = prev_v {
            if pdt > 0.03 && (v - pv).abs() / pdt > 8.0 { return true; }
        }
    }
    false
}

/// 全链扫描，截除最后一段不合理样本之前的前缀（其后样本已收敛到服务器权威基线）。
#[allow(dead_code)]
fn truncate_implausible_prefix(samples: &mut Vec<TickSample>) {
    let mut cut = 0usize;   // 保留 samples[cut..]
    let mut prev: Option<(f32, f32)> = None;
    for i in 1..samples.len() {
        let (v, err, dt, disp) = seg_speed(&samples[i - 1], &samples[i]);
        if cut_needed(v, err, disp, prev) {
            cut = i;
        }
        prev = Some((v, dt));
    }
    if cut > 0 {
        samples.drain(0..cut);
    }
}

/// Vehicle method1（type=8 method=0x01）血量/来源/原因事件（WotbTools AFFIRMED）：
/// envelope entityId = 受害者；args 7B = [currentHpRaw u16][sourceEntity u32][causeFlag u8]；
/// cause：0=炮弹直击 1=火焰 2=撞击 3=世界/环境 5=溺水。攻击者+原因+受害者三键确定性归属，无时间窗猜测。
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
        if *ptype != 5 || p.len() < 53 { continue; }
        let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        out.entry(eid).or_insert((*clock, u16::from_le_bytes([p[51], p[52]])));
    }
    out
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

// ===== 作者/他人射击路径共用的收集阶段（按 author 过滤参数化） =====

/// method29 (0x1d) 发射事件。args 布局（alen=37）：
/// [shooterEntityId u32][shotId u32][rawFlag u8][launchPoint 3×f32][launchVelocity 3×f32][terminalRaw f32]
struct LaunchEntry {
    t: f32,
    shooter: u32,
    shot_id: u32,
    point: [f32; 3],
    vel: [f32; 3],
    /// method29 包处理时刻（**流序**）射手的最后已知 prop2 原始 u16——WI 解析器同构快照。
    /// 与时钟序"≤t 最后采样"的差异仅在同 tick 内包序：method29 包之前到达的 prop2 才计入。
    shooter_prop2: Option<(f32, u16)>,   // (采样钟, 原始 u16)
}

/// method20 (0x14) 弹道终点（shotId 配对）。
/// method8 直击通知（全局广播，envelope eid = 受击者）；
/// args = [shooterEntityId u32][victimEntityId u32][01][result u8][extra u8][hash6][tail...]；result 枚举与 type=32 同域，hash6 与同事件 type=32 完全一致（86/86 实测）。
/// victim_state = 该通知包处理时刻（文件序）受击者的最后已知 type=10 姿态——
/// wotinspector distance 的精确取值基准（99/99 发 μ 级复现，逆向文档 4.0'），受击方锚点。
struct DirectHit8 {
    t: f32,
    shooter: u32,
    victim: u32,
    result: u8,
    /// args[10] = **服务器下发的受击部件索引 cmpIndex**（2026-09 破译：showDamageFromShot
    /// 的 8 字节 segment 描述符元素 byte1，客户端 BWUtils::DecodeShotSegment 以 bboxes[4]/
    /// partMatrixes[4] 按此部件放置着弹点——见《游戏回放数据处理分析报告.md》§4.7）。
    /// 部件对应（命中离地高度统计实证，3 场 26 发）：**0=底盘/履带（med 0.70m，全为跳弹/间隙止）、
    /// 1=车体（med 1.17m）、2=炮塔（med 2.84m）、3=炮管（直射弹未观测）**。
    /// J39 实测分布 {0:22, 1:40, 2:34, 3:4}。
    component_index: Option<u8>,
    hash6: [u8; 6],
    victim_state: Option<([f32; 3], [f32; 3], f32)>,   // (pos, ang[yaw,pitch,roll], 状态采样时钟)
    /// method8 包处理时刻（**流序**）受击者的最后已知 prop2 原始 u16——WI 解析器同构快照
    /// （battle.json turret_yaw/gun_pitch 的取样基准，2026-09-24 T110E5 回放 21/21 逐位验证：
    /// 炮塔 coarse10 与流序快照精确相等，时钟序仅 8/21——同 tick 内 prop2 与 method8 的包序
    /// 决定取值）。(采样钟, 原始 u16)
    victim_prop2: Option<(f32, u16)>,
}

/// type=32 来袭炮弹警告/命中通知（eid = 受击者，AoI 广播含他人命中）。
/// len=26 (method 0x11): [eid u32][01][method u32][u16@9][flag@11][hash6@12..18][segment u64@18..26]
/// len=27 (method 0x12): [eid u32][01][method u32][u16@9][flag@11][01@12][hash6@13..19][segment u64@19..27]
/// hash6 6B = [shell u16][来向 yaw u16][抵达 pitch u16]（原始解码恢复，2026-09 复核）：
///   yaw = (u16−32768)/32768×π = 受击者指向射手的方位角；pitch = (u16−32768)/32768×(π/2)
///   = 抵达垂直角。原始交叉验证（ea2f8c6）：yaw shot1 +66.9° vs 位置推算 +68.8° ✓；
///   pitch shot1 −2.43° ✓。非命中的路过炮弹警告会被 yaw 校验弃用（decoded_target_gun_pitch）。
struct ArenaWarning32 {
    t: f32,
    eid: u32,
    result: u8,
    segment: u64,
    hash6: [u8; 6],
    inc_yaw: f32,
    inc_pitch: f32,
}

/// 血量链降幅区间（参考 WotbTools PlaybackCombatReconstruction.deriveLosses）。
struct DmgLoss { victim: u32, source: u32, t_prev: f32, t_cur: f32, dmg: u32, hp_cur: u16 }

/// method29 (0x1d) 发射事件收集（作者/他人路径共用）：clock ≥5s，按 shooter 过滤 + shotId 去重（重复包保留首条），
/// 按发射时刻排序。返回 (发射列表, 首个 shooter 命中过滤且 args<37 的包的 args_len)——作者路径据此 fail-fast，他人路径忽略。
fn collect_launches(
    packets: &[(u32, f32, &[u8])],
    keep: impl Fn(u32) -> bool,
) -> (Vec<LaunchEntry>, Option<usize>) {
    let mut out: Vec<LaunchEntry> = Vec::new();
    let mut seen_shots: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut short_args: Option<usize> = None;
    // 流序当前 prop2（全实体维护——keep 过滤只作用于发射事件本身）
    let mut ang2: std::collections::HashMap<u32, (f32, u16)> = Default::default();
    for (t2, clock, p) in packets {
        if *clock < 5.0 || p.len() < 12 { continue; }
        if *t2 == 7 && p.len() >= 14 && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2 {
            ang2.insert(u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                (*clock, u16::from_le_bytes([p[12], p[13]])));
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x1d { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 4 || 12 + args_len > p.len() { continue; }   // 连 shooter 都读不出：无法归属，跳过
        let a = &p[12..12 + args_len];
        let shooter = u32::from_le_bytes([a[0], a[1], a[2], a[3]]);
        if !keep(shooter) { continue; }
        if args_len < 37 {
            if short_args.is_none() { short_args = Some(args_len); }
            continue;
        }
        let shot_id = u32::from_le_bytes([a[4], a[5], a[6], a[7]]);
        if !seen_shots.insert(shot_id) { continue; }   // 重复包保留首条（非数据伪造）
        let f = |o: usize| f32::from_le_bytes([a[o], a[o+1], a[o+2], a[o+3]]);
        out.push(LaunchEntry {
            t: *clock,
            shooter,
            shot_id,
            point: [f(9), f(13), f(17)],
            vel: [f(21), f(25), f(29)],
            shooter_prop2: ang2.get(&shooter).copied(),
        });
    }
    out.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());
    (out, short_args)
}

/// method20 (0x14) 弹道终点收集（作者/他人路径共用）：shotId 配对（含 miss 的空地终点），重复 shotId 保留首条。
fn collect_endpoints(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, (f32, [f32; 3])> {
    let mut endpoints: HashMap<u32, (f32, [f32; 3])> = HashMap::new();
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
    endpoints
}

/// method8 直击通知收集（作者/他人路径共用），按时钟排序。
fn collect_direct_hits8(packets: &[(u32, f32, &[u8])]) -> Vec<DirectHit8> {
    // 文件序状态机：按包出现顺序维护每实体最后已知 type=10 姿态，method8 到达时快照受击者。
    // （wi 对齐验证：distance = |state[shooter] − state[victim]|@method8，99/99 发 median 残差 2μm）
    let mut pose: HashMap<u32, ([f32; 3], [f32; 3], f32)> = HashMap::new();
    // 同一状态机的 prop2 通道：method8 到达时快照受击者最后已知 prop2（WI turret_yaw 同基准）
    let mut ang2: HashMap<u32, (f32, u16)> = HashMap::new();
    let mut direct_hits8: Vec<DirectHit8> = Vec::new();
    for (t2, clock, p) in packets {
        if *t2 == 10 && p.len() >= 48 {
            let f = |o: usize| f32::from_le_bytes([p[o], p[o+1], p[o+2], p[o+3]]);
            pose.insert(u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                ([f(12), f(16), f(20)], [f(36), f(40), f(44)], *clock));
            continue;
        }
        if *t2 == 7 && p.len() >= 14 && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2 {
            ang2.insert(u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                (*clock, u16::from_le_bytes([p[12], p[13]])));
            continue;
        }
        if p.len() < 12 + 10 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x08 { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 10 || 12 + args_len > p.len() { continue; }
        let a = &p[12..12 + args_len];
        if a[8] != 0x01 { continue; }
        let victim = u32::from_le_bytes([a[4], a[5], a[6], a[7]]);
        // args = [shooter u32][victim u32][count u8][element 8B = result|cmpIndex|hash6 u48][tail 4B]
        // a[10] = 服务器下发的受击部件索引 cmpIndex（showDamageFromShot/DecodeShotSegment，报告 §4.7）
        let component_index = if args_len >= 11 { Some(a[10]) } else { None };
        direct_hits8.push(DirectHit8 {
            t: *clock,
            shooter: u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
            victim,
            result: a[9],
            component_index,
            hash6: [a[11], a[12], a[13], a[14], a[15], a[16]],
            victim_state: pose.get(&victim).map(|(pos, ang, c)| (*pos, *ang, *c)),
            victim_prop2: ang2.get(&victim).copied(),
        });
    }
    direct_hits8.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());
    direct_hits8
}

/// type=32 警告/命中通知收集（作者/他人路径共用）。不排序：作者路径取窗口内最早一条（取后自排），
/// 他人路径按 hash6 令牌精确配对与顺序无关——各自保持原语义。
fn collect_warnings32(packets: &[(u32, f32, &[u8])]) -> Vec<ArenaWarning32> {
    let mut warnings32: Vec<ArenaWarning32> = Vec::new();
    for (t, clock, p) in packets {
        if *t != 32 || p.len() < 26 { continue; }
        if p[4] != 0x01 { continue; }
        let method = u32::from_le_bytes([p[5], p[6], p[7], p[8]]);
        if method != 0x11 && method != 0x12 { continue; }
        // 尾段 6B = [shell u16][来向 yaw u16][抵达 pitch u16]（原始解码恢复）：
        // off = len≥27 ? 13 : 12（27B 在 hash6 前多一个 01 字节）；yaw@+2 pitch@+4
        let off = if p.len() >= 27 { 13 } else { 12 };
        let u = |k: usize| u16::from_le_bytes([p[off + k], p[off + k + 1]]) as f32;
        let inc_yaw = (u(2) - 32768.0) / 32768.0 * std::f32::consts::PI;
        let inc_pitch = (u(4) - 32768.0) / 32768.0 * (std::f32::consts::FRAC_PI_2);
        let (hash6, seg_bytes) = if p.len() == 26 {
            ([p[12], p[13], p[14], p[15], p[16], p[17]], &p[18..26])
        } else if p.len() == 27 {
            ([p[13], p[14], p[15], p[16], p[17], p[18]], &p[19..27])
        } else {
            continue;
        };
        warnings32.push(ArenaWarning32 {
            t: *clock,
            eid: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            result: seg_bytes[0],
            segment: u64::from_le_bytes(seg_bytes.try_into().unwrap()),
            hash6,
            inc_yaw,
            inc_pitch,
        });
    }
    warnings32
}

/// type=32 警告包抵达成角解码与校验（原始实现恢复，ea2f8c6；逆向文档 §4.1 复核）：
/// 候选 = 受击者 eid 的警告、命中 ±3s 窗口（来袭警告覆盖未命中弹，含路过炮弹）；
/// 选取 = 解码来向方位角与位置推算方位角（受击者→射手）偏差最小者；偏差 >15° 弃用；
/// |pitch|>30°（非直射抵达角）弃用。通过 = 该警告确属命中本车的炮弹，
/// pitch 即"受击者反向瞄准射手的俯仰"（渲染炮口指向射手的炮管俯角）。
fn decoded_target_gun_pitch(
    warnings32: &[ArenaWarning32],
    victim: u32,
    end_time: f32,
    bearing: Option<f32>,
) -> Option<(f32, f32)> {
    let bearing = bearing?;
    let mut best: Option<(f32, f32, f32)> = None; // (yaw 偏差, pitch, yaw)
    for w in warnings32 {
        if w.eid != victim { continue; }
        if w.t > end_time + 0.1 || w.t <= end_time - 3.0 { continue; }
        let err = ((w.inc_yaw - bearing + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI)
            .abs();
        if err > 0.262 { continue; }   // 15°
        if best.as_ref().map(|(be, _, _)| err < *be).unwrap_or(true) {
            best = Some((err, w.inc_pitch, w.inc_yaw));
        }
    }
    let (_, pitch, yaw) = best?;
    if pitch.to_degrees().abs() > 30.0 { return None; }
    Some((pitch, yaw))
}

/// 血量链降幅区间推导（作者/他人路径共用）：同钟同 HP 去冲突后取相邻降幅；
/// cause=0 炮弹直击；author_filter = Some(eid) 仅保留该射手造成的降幅（作者路径），None 保留全部 source（他人路径）。
/// initial_hp（type=5 满血锚点）：早于受害者首个 method1 采样时前插 seed，使首刀降幅可推导；
/// seed 时钟回退 1ms，避免与同钟 method1 事件构成同钟异值冲突而被去重整体丢弃；
/// seed 晚于首个 method1（AoI 迟到全量包）时放弃，保持原行为。
fn derive_dmg_losses(
    hp_events: &[HpEvent],
    author_filter: Option<u32>,
    initial_hp: &HashMap<u32, (f32, u16)>,
) -> Vec<DmgLoss> {
    let mut dmg_losses: Vec<DmgLoss> = Vec::new();
    let mut by_victim: std::collections::HashMap<u32, Vec<&HpEvent>> = std::collections::HashMap::new();
    for e in hp_events { by_victim.entry(e.victim).or_default().push(e); }
    for (victim, evs) in &by_victim {
        let mut samples: Vec<(f32, u16, u32, u8)> = Vec::new();
        if let Some(&(ts, hp)) = initial_hp.get(victim) {
            if evs.first().map_or(true, |e| ts <= e.clock) {
                samples.push(((ts - 1e-3).max(0.0), hp, 0, 0));
            }
        }
        let mut i = 0usize;
        while i < evs.len() {
            let t = evs[i].clock; let hp = evs[i].hp;
            let mut conflict = false; let mut j = i + 1;
            while j < evs.len() && (evs[j].clock - t).abs() <= 1e-6 {
                if evs[j].hp != hp { conflict = true; }
                j += 1;
            }
            if !conflict { samples.push((t, hp, evs[i].source, evs[i].cause)); }
            i = j;
        }
        for w in 1..samples.len() {
            let (t_prev, hpp, _, _) = samples[w - 1];
            let (t_cur, hpc, srcc, causec) = samples[w];
            if hpc < hpp && causec == 0 && author_filter.map_or(true, |a| srcc == a) {
                dmg_losses.push(DmgLoss { victim: *victim, source: srcc, t_prev, t_cur, dmg: (hpp - hpc) as u32, hp_cur: hpc });
            }
        }
    }
    dmg_losses.sort_by(|a, b| a.t_cur.partial_cmp(&b.t_cur).unwrap());
    dmg_losses
}

/// 血量降幅 → 发射的互斥归属（修复 1436 类错配）：每段降幅只归属一次，给区间
/// (t_prev, t_cur] 内 (victim, shooter) 匹配且 end_time 最大的发射。血量事件与命中
/// 同 tick（1436 实证 prop3/method1 时刻 == 命中时刻），降幅属于区间内最后一发命中；
/// 更早命中的伤害会形成独立血量事件不共区间。未穿弹与降幅同区间时不抢归属
/// （end_time 更小即让位：1436 中 44.865/53.972 未穿与 97.962 击穿共区间，正确归属后者）。
/// 返回 发射序号 → (dmg, hp_cur)；无人认领的降幅丢弃（source 不符等）。
fn assign_dmg_losses(
    shots: &[Option<(f32, u32, u32)>],   // (end_time, shooter, victim)；None = 无终点/脱靶
    losses: &[DmgLoss],
) -> HashMap<usize, (u32, u16)> {
    let mut out: HashMap<usize, (u32, u16)> = HashMap::new();
    for lo in losses {
        let mut best: Option<(f32, usize)> = None;   // (end_time, 发射序号)
        for (si, s) in shots.iter().enumerate() {
            let Some((end_time, shooter, victim)) = *s else { continue };
            if victim != lo.victim || shooter != lo.source { continue; }
            if !(lo.t_prev < end_time && end_time <= lo.t_cur + 1e-6) { continue; }
            if out.contains_key(&si) { continue; }
            if best.map_or(true, |(t, _)| end_time > t) { best = Some((end_time, si)); }
        }
        if let Some((_, si)) = best {
            out.insert(si, (lo.dmg, lo.hp_cur));
        }
    }
    out
}

/// type=35 tick 时间线收集（作者/他人路径共用）。
fn collect_tick_timeline(packets: &[(u32, f32, &[u8])]) -> Vec<(f32, u8)> {
    packets.iter()
        .filter(|(t, _, p)| *t == 35 && !p.is_empty())
        .map(|(_, clock, p)| (*clock, p[0]))
        .collect()
}

/// type=35 tick 计数器插值（作者/他人路径共用）：timeline 按 clock 排序 → 二分定位首个 clock ≥ t 的相邻段线性内插；
/// u8 回绕展开（差值掩 0xFF，>128 视为回退取负）；t 早于首包按首段向后外推（与原线性实现一致），t 晚于末包取末值，样本 <2 条取末值。
fn tick_at(tick_timeline: &[(f32, u8)], t: f32) -> f32 {
    if tick_timeline.len() < 2 {
        return tick_timeline.first().map(|(_, v)| *v as f32).unwrap_or(0.0);
    }
    let j = tick_timeline.partition_point(|(c, _)| *c < t);
    if j == tick_timeline.len() {
        return tick_timeline.last().unwrap().1 as f32;
    }
    let i = j.max(1);
    let (t0, v0) = tick_timeline[i - 1];
    let (t1, v1) = tick_timeline[i];
    if t1 <= t0 { return v0 as f32; }
    let dv = ((v1 as i32 - v0 as i32) & 0xFF) as f32;
    let dv = if dv > 128.0 { dv - 256.0 } else { dv };
    let dt = t1 - t0;
    if dt <= 0.0 { return v0 as f32; }
    v0 as f32 + dv * (t - t0) / dt
}

/// per-entity 索引（作者/他人路径共用）：type=10 状态采样 + type=7 prop2 打包角。
/// prop2 u16 = (炮塔偏航 coarse10 << 6) | 炮管俯仰比例 frac6（T110 1617 受控实验 +
/// J39 实战复核，主文档 §2.2）：偏航只取高 10 位（低 6 位是俯仰，混入会引入
/// ±0.3° 假跳变），frac 保留供俯仰解码。各实体 Vec 保持包序（未排序）；作者路径
/// 用前需按 clock 排序（锚点选择依赖时序），他人路径沿用包序。
fn build_entity_indexes(
    packets: &[(u32, f32, &[u8])],
) -> (HashMap<u32, Vec<St10Sample>>, HashMap<u32, Vec<(f32, f32, u16)>>) {
    let mut st10: HashMap<u32, Vec<St10Sample>> = HashMap::new();
    let mut prop2: HashMap<u32, Vec<(f32, f32, u16)>> = HashMap::new();
    for (t2, clock, p) in packets {
        if *t2 == 10 && p.len() >= 48 {
            let f = |o: usize| f32::from_le_bytes([p[o], p[o+1], p[o+2], p[o+3]]);
            st10.entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]])).or_default().push(St10Sample {
                clock: *clock,
                pos: [f(12), f(16), f(20)],
                yaw: f(36), pitch: f(40), roll: f(44),
                pos_error: [f(24), f(28), f(32)],
            });
        }
        if *t2 == 7 && p.len() >= 14
            && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2 {
            let v = u16::from_le_bytes([p[12], p[13]]);
            let rel = (v >> 6) as f32 / 1024.0 * std::f32::consts::TAU - std::f32::consts::PI;
            let rel = if rel > std::f32::consts::PI { rel - std::f32::consts::TAU } else { rel };
            prop2.entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]])).or_default().push((*clock, rel, v & 63));
        }
    }
    (st10, prop2)
}

/// 单扇区俯仰极值（度，models.pb PitchExtremaInfo）：min=−仰角上限、max=俯角上限、
/// range=扇区角宽（以正前 0°/正后 180° 为中心，±range/2）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectorLimits {
    pub min: f32,
    pub max: f32,
    pub range: f32,
}

/// 一门主炮的俯仰限制（models.pb GunModelDefinition.pitch，顶级配置）：
/// 基础 (dep=俯角上限, ele=仰角上限) + 可选前/后扇区极值 + 过渡角。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GunPitchRange {
    /// 基础俯角上限（正值，度）——扇区外的全向值
    pub dep: f32,
    /// 基础仰角上限（正值，度）
    pub ele: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front: Option<SectorLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<SectorLimits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<f32>,
}

/// 昵称 → 炮管俯仰限制：prop2 frac 解码的锚定表。
/// 由 battle_results（昵称→tank_id）+ models.pb 顶级配置（`pitch_limits_from_battle_results`）构建。
/// 注意：匿名玩家共用显示名 "Anonyme"，同场多个匿名玩家会互相覆盖（按昵称连接的固有歧义）；
/// 取顶级配置（多配置车辆的模块级差异未区分，见"俯仰锚定粒度"审计）。
pub type GunPitchLimits = HashMap<String, GunPitchRange>;

/// 炮塔相对角 θ（度，wrap ±180）处的有效 (俯角上限, 仰角上限)：
/// front 扇区以 0° 为中心、back 扇区以 180° 为中心（宽 range，各 ±range/2），扇区内用
/// 扇区极值、扇区外用基础值，过渡带（边界 ±transition/2）线性插值。
/// 过渡带几何为合理推断（BlitzKit applyPitchYawLimits 同名语义），transition 缺省 = 0。
fn gun_pitch_range_at(r: &GunPitchRange, turret_rel_deg: f32) -> (f32, f32) {
    let mut theta = turret_rel_deg;
    while theta > 180.0 { theta -= 360.0; }
    while theta < -180.0 { theta += 360.0; }
    let trans = r.transition.unwrap_or(0.0);
    // 单扇区混合：d = |θ−中心|，扇区内→扇区值，扇区外→基础值，边界两侧 ±trans/2 插值
    let blend = |base: f32, sect: f32, d: f32, half: f32| -> f32 {
        if half <= 0.0 { return base; }
        let lo = half - trans * 0.5;
        let hi = half + trans * 0.5;
        if d <= lo { return sect; }
        if d >= hi { return base; }
        let f = if hi > lo { (d - lo) / (hi - lo) } else { 1.0 };
        sect + (base - sect) * f
    };
    // 基础 (dep, ele)；front/back 扇区依次叠加（几何上不重叠，顺序无关）
    let mut dep = r.dep;
    let mut ele = r.ele;
    if let Some(f) = &r.front {
        dep = blend(dep, f.max, theta.abs(), f.range * 0.5);
        ele = blend(ele, -f.min, theta.abs(), f.range * 0.5);
    }
    if let Some(b) = &r.back {
        let d = (180.0 - theta.abs()).abs();
        dep = blend(dep, b.max, d, b.range * 0.5);
        ele = blend(ele, -b.min, d, b.range * 0.5);
    }
    (dep, ele)
}

/// prop2 frac6 → 炮管俯仰（弧度，炮塔系，正=仰角）。
/// **frac=63 ↔ 当前炮塔朝向的俯角极限（炮管最低）、frac=0 ↔ 仰角极限（最高）**：
/// pitch = ele(θ) − frac/63 × (dep(θ) + ele(θ))，其中 (dep,ele) 随炮塔相对角 θ 分段
/// （front/back 扇区，`gun_pitch_range_at`）。**比例锚定随炮塔朝向分段**（2026-09-23
/// T95E6 旋转实验定案：旋转一周 frac 恒钉 63，而后方扇区俯角仅 −0.5° vs 前方 −10°——
/// 物理角随朝向被钳而比例恒定 = 服务器按当前朝向限制打包 frac）。基础锚定证据：
/// T110 1617 受控实验（极限动作 0/63 精确钳位）+ 实战 frac 常态贴 63（第三人称预瞄
/// 点在近地面）。快速偏航段 frac 恒钉极限 = 炮管贴极限的**实时如实上报**（用户实战
/// 确认为正常操作现象，非通道冻结——服务器按需重发不变值）。
#[inline]
pub fn decode_prop2_gun_pitch(frac: f32, range: &GunPitchRange, turret_rel_rad: f32) -> f32 {
    let (dep, ele) = gun_pitch_range_at(range, turret_rel_rad * 57.29578);
    (ele - frac / 63.0 * (dep + ele)).to_radians()
}

/// prop2 原始 u16 → (炮塔相对偏航 rad, frac6)。与 [`build_entity_indexes`] 同一解码
/// （高 10 位 coarse = 偏航、低 6 位 = 俯仰比例）。供流序快照
/// （[`DirectHit8::victim_prop2`] / [`LaunchEntry::shooter_prop2`]）消费。
fn decode_prop2_u16(v: u16) -> (f32, f32) {
    let rel = (v >> 6) as f32 / 1024.0 * std::f32::consts::TAU - std::f32::consts::PI;
    let rel = if rel > std::f32::consts::PI { rel - std::f32::consts::TAU } else { rel };
    (rel, (v & 63) as f32)
}

/// prop2 时间线 → **客户端语义密集采样**（0.1s 网格，与 render_timeline 同惯例）：
/// 每个网格点用 [`prop2_at`]（到达约束/短弧插值/末帧保持）求值，
/// 保证滑块 dt=0 与初始摆放严格一致。替代原始采样窗口直通——前端 lerpTl 在稀疏
/// 原始采样上做双侧插值会把查询点之后才到达的包混进来（命中后反应污染），与
/// 0x1440C70 只用已到达帧的语义不符。
fn timeline_prop2_client(
    series: Option<&Vec<(f32, f32, u16)>>,
    base: f32,
    from: f32,
    to: f32,
    limits: Option<&GunPitchRange>,
) -> Vec<(f32, f32)> {
    if series.is_none() { return Vec::new(); }
    let mut out = Vec::new();
    let mut dt = from;
    while dt <= to + 1e-4 {
        if let Some((y, fr)) = prop2_at(series, base + dt) {
            let v = match limits {
                Some(lim) => decode_prop2_gun_pitch(fr, lim, y),
                None => y,
            };
            out.push((dt, v));
        }
        dt += 0.1;
    }
    out
}

/// 从 prop2 序列取 t 时刻的 (相对偏航, frac)：**客户端 0x1440C70 时间线语义**的
/// 离线等价（0x1445450 关键帧 = 到达时钟 + 0.1s 前瞻常量 0x36a9288；渲染查询滞后
/// 关键帧 0.1s）：
/// - 已到达帧 = 到达 ≤ t 的采样；查询点映射到到达域 **q = t − 0.1**；
/// - 夹逼 [最后到达 ≤ q, 首个到达 ∈ (q, t]]：yaw 短弧插值（0x1441e00 同款）、
///   frac 线性插值——右括号用的是查询前 0.1s 内到达的最新帧（不丢弃）；
/// - q 超出末关键帧（无 (q, t] 内到达）：**保持末帧**——0x1440C70 的 0.9 限步
///   外推属实时逐帧渲染语义（每帧 ≤0.9×速度×dt），离线任意时刻查询按第五轮补 3
///   "clamp 不外推"执行（曾实现为无界外推致 2056 静止段炮塔角错 72°，2026-09-24 修复）；
/// - q 早于首帧：保持首帧；t 前完全无采样（AoI 新进）回退双侧最近初值包。
/// 被击时刻姿态不被命中后反应包污染的保证：反应首包到达 > t，永远不进任何括号。
/// 注意：此为**渲染层**（滑块时间线）语义；判定锚点（炮塔朝向/俯仰取样）用
/// [`prop2_at_arrived`]（最后到达采样，与 WI turret_yaw 同域）。
fn prop2_at(
    series: Option<&Vec<(f32, f32, u16)>>,
    t: f32,
) -> Option<(f32, f32)> {
    let list = series?;
    // 序列按包序（=时钟序）；n = 已到达帧数，m = 到达 ≤ q 的帧数
    let n = list.iter().filter(|(c, _, _)| *c <= t).count();
    if n == 0 {
        // AoI 边界：t 前无采样，回退最近初值包
        let (_c, y, fr) = list.iter()
            .min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap_or(std::cmp::Ordering::Equal))?;
        return Some((*y, *fr as f32));
    }
    let q = t - 0.1;
    let m = list.iter().filter(|(c, _, _)| *c <= q).count();
    let lerp = |(ca, ya, fa): (f32, f32, u16), (cb, yb, fb): (f32, f32, u16), f: f32| -> (f32, f32) {
        let mut dy = yb - ya;
        while dy > std::f32::consts::PI { dy -= std::f32::consts::TAU; }
        while dy < -std::f32::consts::PI { dy += std::f32::consts::TAU; }
        (ya + dy * f, fa as f32 + (fb as f32 - fa as f32) * f)
    };
    if m == 0 {
        // q 早于首帧：保持首帧（首帧可能在 (q, t] 内到达——尚未起效）
        let (_, y, fr) = list[0];
        return Some((y, fr as f32));
    }
    if m < n {
        // 夹逼插值：list[m-1] ≤ q < list[m]（后者已到达）
        let (ca, _, _) = list[m - 1];
        let (cb, _, _) = list[m];
        let f = ((q - ca) / (cb - ca)).clamp(0.0, 1.0);
        return Some(lerp(list[m - 1], list[m], f));
    }
    // q 超出末关键帧：保持末帧。离线任意时刻查询不做前瞻外推——0x1440C70 的
    // 0.9 限步外推属实时逐帧渲染语义（每帧 ≤0.9×速度×dt），离线按第五轮补 3
    // "滑块模式 clamp 不外推"。曾实现为无界外推 f=0.9×(q−c0)/(c0−cp)：炮塔静止段
    // 被停止前末两帧角速度外推数十秒（2056 回放命中时刻离末帧 46s，外推系数 ~414，
    // 炮塔角错 72°，2026-09-24 修复）。
    let (_, y, fr) = list[n - 1];
    Some((y, fr as f32))
}

/// 判定锚点采样（受击方/射手炮塔朝向与炮管俯仰取样，与 WI turret_yaw 同域的
/// 旧语义）：取 t 时刻**最后已到达**采样，不做客户端渲染滞后的 q 插值——命中
/// 判定用服务器广播的最新已知值；t 前无采样（AoI 新进）回退最近初值包。
fn prop2_at_arrived(
    series: Option<&Vec<(f32, f32, u16)>>,
    t: f32,
) -> Option<(f32, f32)> {
    let list = series?;
    if let Some((_, y, fr)) = list.iter().rev().find(|(c, _, _)| *c <= t) {
        return Some((*y, *fr as f32));
    }
    let (_c, y, fr) = list.iter()
        .min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap_or(std::cmp::Ordering::Equal))?;
    Some((*y, *fr as f32))
}

/// prop2 采样新鲜度（仅区分"流陈旧"与"值稳定"）：最后已到达包之后 prop2 是否停止
/// 发送超过 2s。**frac 不变 ≠ 冻结**——prop2 变化驱动，炮管停在极限/定点时 frac 恒定
/// 是物理事实的如实上报（2056/1617 实验实证：快速偏航段 frac 钉极限 = 炮管贴极限的
/// 正常操作常态，非通道冻结；用户实战确认）。仅当整条流断流（AoI 边界/补发簇）时
/// 采样才可能陈旧，用于 quality.pitch_frozen 提示。
fn prop2_frac_frozen(series: Option<&Vec<(f32, f32, u16)>>, t: f32) -> bool {
    let Some(list) = series else { return false };
    match list.iter().rposition(|(c, _, _)| *c <= t) {
        None => false,
        Some(idx) => t - list[idx].0 > 2.0,
    }
}

/// 从射击事件抽取复现数据（WotbTools 权威弹丸生命周期，全部 shotId 确定性配对）：
/// method29 (0x1d) 发射 / method20 (0x14) 终点 / method38 (0x26) 命中结果（仅作者）；目标 = method38 victimVehicleId（服务器权威，无则 miss）；
/// 伤害 = method1 血量链差值（victim + source=作者 + cause=0）。数据完整性 fail-fast：任何缺失/歧义直接返回 Err，不做保守降级（零值掩盖问题）。
pub fn extract_shot_replays(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
) -> anyhow::Result<Vec<ShotReplayData>> {
    extract_shot_replays_with_limits(packets, author_player_eid, &GunPitchLimits::new())
}

/// [`extract_shot_replays`] 的完整形态：`pitch_limits` = 昵称→(俯角°,仰角°) 锚定表
/// （[`gun_pitch_limits_from`] 构建）。双方炮管俯仰主来源 = prop2 frac 比例解码；
/// 无锚定表时回退旧路径（作者 prop9 瞄准角 / 他人速度向量 / 受击方车体 pitch）并打质量标记。
pub fn extract_shot_replays_with_limits(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
    pitch_limits: &GunPitchLimits,
) -> anyhow::Result<Vec<ShotReplayData>> {
    if author_player_eid == 0 {
        anyhow::bail!("无法解析作者实体：文件名需包含玩家昵称（type=5 昵称匹配失败）");
    }

    // ① 收集作者的 method29 发射事件（全局弹丸流，按 shooterEntityId 过滤；shotId 去重）；
    //    args<37 = 回放版本布局漂移，fail-fast
    let (launches, short_args) = collect_launches(packets, |e| e == author_player_eid);
    if let Some(args_len) = short_args {
        anyhow::bail!("作者的 method29 发射包 args 长度 {} < 37（回放版本布局漂移？）", args_len);
    }

    // ② 收集 method20 弹道终点（shotId 配对；含 miss 的空地终点）
    let endpoints = collect_endpoints(packets);

    // ③ 收集 method38 命中结果（Avatar 方法 = 仅作者自己的射击反馈）
    //    args 布局（WotbTools PROVEN + 4 回放实测）：[victimVehicleId u32][resultFlags16 u16]
    //    [headerHi16 u16][resultCount u8][resultCount × (componentToken u8 + rawState u8)]
    //    [modifierCount u8][modifierCount × modifierId u32]；rawState：0=无变化 1=受损(crit) 2=摧毁；
    //    组件号 31=引擎 32=弹药架 33=油箱 34/35=右/左履带 36=火炮 38=观察装置（WotbTools 枚举）
    struct HitFeedback {
        t: f32,
        victim: u32,
        flags: u32,
        crit_modules: u32,
        destroyed_modules: u32,
        components: Vec<(u8, u8)>,
        modifiers: Vec<u32>,
    }
    let mut hit_results: Vec<HitFeedback> = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 12 + 9 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x26 { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 9 || 12 + args_len > p.len() { continue; }
        let a = &p[12..12 + args_len];
        let mut crit_modules = 0u32;
        let mut destroyed_modules = 0u32;
        let mut components: Vec<(u8, u8)> = Vec::new();
        let mut off = 9usize;
        for _ in 0..a[8] {
            if off + 2 > args_len { break; }
            let (tok, state) = (a[off], a[off + 1]);
            components.push((tok, state));
            let bit = (tok as u32).checked_sub(31).map(|b| 1u32 << b).unwrap_or(0);
            match state {
                1 => crit_modules |= bit,
                2 => destroyed_modules |= bit,
                _ => {}
            }
            off += 2;
        }
        let mut modifiers: Vec<u32> = Vec::new();
        if off < args_len {
            let mcount = a[off];
            off += 1;
            for _ in 0..mcount {
                if off + 4 > args_len { break; }
                modifiers.push(u32::from_le_bytes([a[off], a[off+1], a[off+2], a[off+3]]));
                off += 4;
            }
        }
        hit_results.push(HitFeedback {
            t: *clock,
            victim: u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
            // 完整位图 = flags16 | headerHi<<16（wotinspector hit_flags 同源，0x20000=headerHi 基础位）
            flags: u32::from_le_bytes([a[4], a[5], a[6], a[7]]),
            crit_modules,
            destroyed_modules,
            components,
            modifiers,
        });
    }
    hit_results.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());

    // ③' 同钟同受击者合并：一发命中可产生多条结果消息（多次装甲交互，同钟重复计为一次命中事件）；
    // 位图取并集；组件按 token 取最大 state；modifiers 去重合并。
    let mut merged38: Vec<HitFeedback> = Vec::new();
    for h in hit_results {
        if let Some(last) = merged38.last_mut() {
            if (last.t - h.t).abs() <= 0.05 && last.victim == h.victim {
                last.flags |= h.flags;
                last.crit_modules |= h.crit_modules;
                last.destroyed_modules |= h.destroyed_modules;
                for (tok, st) in h.components {
                    match last.components.iter_mut().find(|(t, _)| *t == tok) {
                        Some(e) => e.1 = e.1.max(st),
                        None => last.components.push((tok, st)),
                    }
                }
                for m in h.modifiers {
                    if !last.modifiers.contains(&m) { last.modifiers.push(m); }
                }
                continue;
            }
        }
        merged38.push(h);
    }
    let mut hit_results = merged38;

    // ③'' type=32 来袭炮弹警告/命中通知（eid = 受击者，AoI 广播含他人命中）；segment u64 低字节 = 命中结果枚举（与 method8 b9 同域，86/86 实测一致）。
    let mut warnings32 = collect_warnings32(packets);
    warnings32.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());

    // ③''' Vehicle method8 直击通知（全局广播，envelope eid = 受击者）
    let direct_hits8 = collect_direct_hits8(packets);

    let names = extract_entity_names(packets);

    // ④' type=7 刷新簇时钟（AoI 补发/通道切换签名，逆向文档 6.x）——tick 采样窗口截断依据
    let refresh_clusters = collect_refresh_clusters(packets);

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

    // ⑤'' type=35 服务器竞技场 tick 计数器（u8 递增 @10Hz，自然回绕）；开火 tick 判定 100/100 实测对齐。
    let tick_timeline = collect_tick_timeline(packets);

    // ⑥ per-entity 索引（循环前一次预建，替代逐发全量扫包）：type=10 状态采样 / type=7 prop2 炮塔偏航 / avatar prop9 炮管俯仰。
    // st10 按 clock 排序以保持原 collect_entity_st10 语义（锚点选择依赖时序）。
    let (mut st10, prop2) = build_entity_indexes(packets);
    for v in st10.values_mut() {
        v.sort_by(|a, b| a.clock.partial_cmp(&b.clock).unwrap());
    }
    let prop9: Vec<(f32, f32)> = packets.iter()   // avatar (clock, 炮管俯仰 rad)
        .filter(|(t, _, p)| {
            *t == 7 && p.len() >= 16
                && u32::from_le_bytes([p[0], p[1], p[2], p[3]]) == avatar_eid
                && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 9
        })
        .map(|(_, clock, p)| (*clock, f32::from_le_bytes([p[12], p[13], p[14], p[15]]).to_radians()))
        .collect();
    // 射手（作者）俯仰解码锚定：昵称 → (俯角°, 仰角°)；无锚定时俯仰回退 prop9（瞄准角）
    let author_name = names.get(&author_player_eid).cloned().unwrap_or_default();
    let shooter_limits = pitch_limits.get(&author_name);

    // ⑤' 弹药选择时间线（type=28，payload=u32 LE 槽位；录像者本人的选择状态）
    let mut ammo_selects: Vec<(f32, u32)> = Vec::new();
    for (t, clock, p) in packets {
        if *t != 28 || p.len() < 4 { continue; }
        ammo_selects.push((*clock, u32::from_le_bytes([p[0], p[1], p[2], p[3]])));
    }
    ammo_selects.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    // ⑤'' Avatar method 0x07 弹种广播时间线：args(5) = [a0 u8][shell_global_id u32 LE]。
    // a0=0/1 恒成对同值（双份记录/弹鼓双槽，槽位语义未定），a0=18 为非弹种数据（排除）。
    // shell@fire_time 与 type=32 segment 弹种 30/30 一致，命中通知未转发时（含脱靶弹）以此兜底。
    let mut shell_broadcasts: Vec<(f32, u32)> = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 17 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x07 { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 5 || 12 + args_len > p.len() { continue; }
        if p[12] != 0 && p[12] != 1 { continue; }
        shell_broadcasts.push((*clock, u32::from_le_bytes([p[13], p[14], p[15], p[16]])));
    }
    shell_broadcasts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    // ⑤''' Avatar method 0x1b 地形命中包（仅无坦克命中时广播）：shotId 配对，args(34) 布局见 TerrainImpactData。
    let mut terrain_impacts: std::collections::HashMap<u32, TerrainImpactData> =
        std::collections::HashMap::new();
    for (_, _, p) in packets {
        if p.len() < 46 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x1b { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 34 || 12 + args_len > p.len() { continue; }
        let a = &p[12..12 + args_len];
        let f = |o: usize| f32::from_le_bytes([a[o], a[o + 1], a[o + 2], a[o + 3]]);
        terrain_impacts.entry(u32::from_le_bytes([a[0], a[1], a[2], a[3]])).or_insert(
            TerrainImpactData {
                material: a[8],
                impact_point: [f(9), f(13), f(17)],
                segment_start: [f(21), f(25), f(29)],
            },
        );
    }

    // ⑤'''' Avatar method36 (0x24) 瞄准快照时间线（envelope = avatar = 录像者本人）；
    // args = [len u8][protobuf]，开火时刻成对；布局不合法的包 fail-soft 跳过（不影响主链 fail-fast）。
    let mut aim_snapshots: Vec<(f32, f64, f64)> = Vec::new();   // (clock, 炮塔相对偏航, 扩散度)
    for (_, clock, p) in packets {
        if p.len() < 14 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x24 { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 2 || 12 + args_len > p.len() { continue; }
        let (yaw, disp) = parse_method36(&p[12..12 + args_len]);
        if let (Some(yaw), Some(disp)) = (yaw, disp) {
            aim_snapshots.push((*clock, yaw, disp));
        }
    }
    aim_snapshots.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    let hp_events = parse_hp_events(packets);   // method1 血量事件（全实体、按时钟排序）
    // ⑥' 确定性伤害降幅区间（WotbTools deriveLosses 同款；仅作者造成的 cause=0 炮弹直击降幅）
    // type=5 满血锚点补链：受害者首个 method1 已是掉血后血量时，首刀降幅才可归属
    let initial_hp = collect_initial_hp(packets);
    let dmg_losses = derive_dmg_losses(&hp_events, Some(author_player_eid), &initial_hp);
    // 降幅互斥消费标记（一段降幅只归属一发，防同区间多发重复计数）
    let mut dmg_losses_used: std::collections::HashSet<usize> = std::collections::HashSet::new();
    // 作者伤害计数器（type=7 sub=10）增量序列——首次命中（血量链无前值）兜底；撞击/火伤等非弹伤害增量与 method1 cause≠0 且涉及作者的事件同批剔除
    let mut non_shell_ticks: Vec<f32> = hp_events.iter()
        .filter(|e| e.cause != 0 && (e.source == author_player_eid || e.victim == author_player_eid))
        .map(|e| e.clock)
        .collect();
    non_shell_ticks.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut dc_increments: Vec<(f32, u32)> = Vec::new();
    {
        let mut last_cum: u32 = 0;
        for (t, clock, p) in packets {
            if *t != 7 || p.len() < 16 { continue; }
            if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 10 { continue; }
            let cum = u32::from_le_bytes([p[12], p[13], p[14], p[15]]);
            if cum > last_cum {
                // 二分判定 |clock - tc| <= 0.3 的污染 tick（non_shell_ticks 已排序）
                let lo = non_shell_ticks.partition_point(|tc| *tc < clock - 0.3);
                let polluted = lo < non_shell_ticks.len() && non_shell_ticks[lo] <= clock + 0.3;
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
    // 渲染层锚点时间线缓存（滤波器，处理分析报告 §7）：per-entity 惰性构建
    let mut render_cache: HashMap<u32, FilteredTimeline> = HashMap::new();
    for (i, l) in launches.iter().enumerate() {
        let fire_time = l.t;
        let shot_id = l.shot_id;
        // ctx 仅在报错路径格式化（避免逐发无条件分配）
        let ctx = || format!("shot #{} (shotId={}, t={:.2}s)", i + 1, shot_id, fire_time);

        let (sp, sa, sp_dt, sp_src) = select_anchor_state(
            st10.get(&author_player_eid).map(Vec::as_slice).unwrap_or(&[]),
            &refresh_clusters, author_player_eid, fire_time)
            .ok_or_else(|| anyhow::anyhow!("{}: 射手 type=10 状态快照缺失", ctx()))?;

        // 弹道终点（shotId 精确配对）；ball_a = method29 炮口发射位置
        let ball_a = l.point;
        let (end_time, ball_b) = endpoints.get(&shot_id).cloned()
            .ok_or_else(|| anyhow::anyhow!("{}: method20 弹道终点缺失（shotId 无配对）", ctx()))?;
        let fire_tick = tick_at(&tick_timeline, fire_time);

        // ⑦' 弹药槽位：发射时刻的最后选择（type=28 时间线 ≤ fire_time 的最新值）
        let mut shell_slot: u32 = 0;
        for (t_sel, slot) in &ammo_selects {
            if *t_sel <= fire_time { shell_slot = *slot; } else { break; }
        }

        // ⑧ 目标实体：method38 victimVehicleId（服务器权威，确定性）
        let mut target_eid: Option<u32> = None;
        let mut damage = 0u32;
        let mut target_name = String::new();
        let mut is_kill = false;
        let mut hit = false;
        let mut hit_flags: u32 = 0;
        let mut crit_modules: u32 = 0;
        let mut destroyed_modules: u32 = 0;
        let mut modifiers: Vec<u32> = Vec::new();

        let mut matched: Option<usize> = None;
        let mut cands = 0usize;
        for (j, hr) in hit_results.iter().enumerate().skip(hr_cursor) {
            if hr.t > end_time + 0.5 { break; }   // 已排序，越过窗口即止
            if (hr.t - end_time).abs() < 0.5 && hr.t >= end_time - 0.05 {
                cands += 1;
                if matched.is_none() { matched = Some(j); }
            }
        }
        if cands > 1 {
            anyhow::bail!("{}: method38 配对歧义——命中窗口内出现 {} 条命中结果", ctx(), cands);
        }
        if let Some(j) = matched {
            let hr = &mut hit_results[j];
            target_eid = Some(hr.victim);
            hit_flags = hr.flags;
            crit_modules = hr.crit_modules;
            destroyed_modules = hr.destroyed_modules;
            // HitFeedback 恰被消费一次（hr_cursor 单调前进），modifiers 直接移走避免克隆
            modifiers = std::mem::take(&mut hr.modifiers);
            hit = true;
            hr_cursor = j + 1;
        }

        if let Some(teid) = target_eid {
            target_name = names.get(&teid)
                .ok_or_else(|| anyhow::anyhow!("{}: 受击者实体 {} 不在 type=5 名册中", ctx(), teid))?
                .clone();
        }

        // ⑧' 游戏原生命中段 + 结果枚举（wotinspector segment 对齐）：type=32 警告包优先（segment 低字节=结果枚举），
        // method8 直击通知兜底；两者 hash6 命中令牌一致（86/86 实测）。歧义 fail-fast；服务器未转发时 segment=0 / result=255。
        let mut segment: u64 = 0;
        let mut shell_id: u32 = 0;
        let mut armor_group: u8 = 0;
        let mut hit_triangle: u16 = 0;
        let mut game_hit_result: u8 = 255;
        let mut hit_token: Option<String> = None;
        if let Some(teid) = target_eid {
            // warnings32 已按 t 排序且 filter 保序，seg_cands 天然有序，无需再排
            let seg_cands: Vec<&ArenaWarning32> = warnings32.iter()
                .filter(|w| w.eid == teid && (w.t - end_time).abs() <= 0.05)
                .collect();
            if !seg_cands.is_empty() {
                let first = seg_cands[0];
                if seg_cands.iter().any(|w| w.segment != first.segment) {
                    anyhow::bail!("{}: type=32 segment 歧义——窗口内 {} 条互不一致的命中段", ctx(), seg_cands.len());
                }
                segment = first.segment;
                game_hit_result = first.result;
                hit_token = Some(first.hash6.iter().map(|b| format!("{:02x}", b)).collect());
                // segment 布局解码：[result][shell_global_id u24 LE（=(局部 id<<8)|国家基数）][00][X][Y][Z=armor_group]
                // （旧 "[tank+9]" 解释为 4 样本巧合，已证伪——B1 是弹种 id 的国家基数字节）
                let sb = segment.to_le_bytes();
                // 全局弹种 id = B1B2B3 u24 LE（=(局部 id<<8)|国家基数），与 WI shell_id 同值
                shell_id = (sb[1] as u32) | ((sb[2] as u32) << 8) | ((sb[3] as u32) << 16);
                armor_group = sb[7];
                hit_triangle = u16::from_be_bytes([sb[5], sb[6]]);
            } else {
                // direct_hits8 已按 t 排序且 filter 保序，r8 天然有序，无需再排
                let r8: Vec<&DirectHit8> = direct_hits8.iter()
                    .filter(|d| d.shooter == author_player_eid && d.victim == teid && (d.t - end_time).abs() <= 0.05)
                    .collect();
                if !r8.is_empty() {
                    let first = r8[0];
                    if r8.iter().any(|d| d.result != first.result) {
                        anyhow::bail!("{}: method8 结果枚举歧义——窗口内 {} 条互不一致", ctx(), r8.len());
                    }
                    game_hit_result = first.result;
                    hit_token = Some(first.hash6.iter().map(|b| format!("{:02x}", b)).collect());
                }
            }
        }

        // ⑧'' 弹种兜底：命中通知未转发（segment=0，含脱靶弹）时用 method 0x07 广播 @ 发射时刻补全（30/30 一致）；
        // 兜底发生时置 shell_from_broadcast 供 UI 徽章提示
        let mut shell_from_broadcast = false;
        if shell_id == 0 {
            for (t_sel, sh) in &shell_broadcasts {
                if *t_sel <= fire_time { shell_id = *sh; shell_from_broadcast = true; } else { break; }
            }
        }
        let terrain_impact = terrain_impacts.get(&shot_id).cloned();

        // ⑧''' 开火时刻瞄准快照（method36 成对，|dt|≤0.05）：前=射击前，后=射击后。
        let shooter_aim = {
            let cands: Vec<(f64, f64)> = aim_snapshots.iter()
                .filter(|(t2, _, _)| (*t2 - fire_time).abs() <= 0.05)
                .map(|(_, y, d)| (*y, *d))
                .collect();
            cands.first().map(|(yaw, disp)| ShooterAimData {
                turret_rel_yaw: *yaw,
                state_before: *disp,
                state_after: cands.get(1).map(|(_, d)| *d),
            })
        };

        // 伤害归属（确定性，WotbTools deriveLosses 同款）：击穿 0x0010 / HE 爆炸 0x1000 →
        // 互斥：一段降幅只归属一次（防同区间双发重复计数，见 assign_dmg_losses 注）
        let mut dmg_unattributed = false;
        if hit && hit_flags & (0x0010 | 0x1000) != 0 {
            let victim = target_eid.unwrap_or(0);
            let containing: Vec<(usize, &DmgLoss)> = dmg_losses.iter().enumerate()
                .filter(|(li, l)| !dmg_losses_used.contains(li)
                    && l.victim == victim && l.t_prev < end_time && end_time <= l.t_cur + 1e-6)
                .collect();
            if containing.len() > 1 {
                anyhow::bail!("{}: 伤害归属歧义（{} 个血量降幅区间包含命中时刻）", ctx(), containing.len());
            }
            let dc_delta = dc_increments.get(dc_cursor).map(|x| x.1);
            if dc_delta.is_some() { dc_cursor += 1; }
            match containing.first() {
                Some((li, l)) => {
                    dmg_losses_used.insert(*li);
                    damage = l.dmg;
                }
                None => {
                    // ② 计数器亦无增量 = 服务器未记账 HP 伤害（模块-only 击穿等）→ 0
                    damage = dc_delta.unwrap_or(0);
                    dmg_unattributed = true;
                }
            }
            is_kill = containing.first().map(|(_, l)| l.hp_cur == 0).unwrap_or(false)
                || hit_flags & 0x0001 != 0;
        }

        // ⑨ 目标位置与姿态 @ 命中通知状态（WI 对齐确定性锚点，逆向文档 4.0'）：
        // method8 命中通知包处理时刻（文件序）受击者的最后已知 type=10 姿态——判定批次内
        // 服务器对受击者的最新已知位置。与 wotinspector 的 distance 取值逐发 μ 级一致
        // （99/99 发，同文件回放对照）；method8 缺失时回退 end_time 插值状态。
        // miss 无目标基准，回退开火时刻。
        let (tp, ta, tp_dt, tp_src) = if hit {
            let teid = target_eid.unwrap_or(0);
            let d8state = direct_hits8.iter().find(|d| {
                d.shooter == author_player_eid && d.victim == teid
                    && (d.t - end_time).abs() <= 0.05 && d.victim_state.is_some()
            })
            .and_then(|d| d.victim_state);
            match d8state {
                // 正常路径（UI 不告警）；回退路径保留 nearest/filtered/extrapolated 供徽章告警
                Some((pos, ang, state_clock)) => (pos, ang, state_clock - end_time, "wi_hit_state"),
                None => select_anchor_state(
                    st10.get(&teid).map(Vec::as_slice).unwrap_or(&[]),
                    &refresh_clusters, teid, end_time)
                    .ok_or_else(|| anyhow::anyhow!("{}: 命中时刻目标 type=10 状态快照缺失", ctx()))?,
            }
        } else { ([0.0; 3], [0.0; 3], 0.0, "nearest") };

        // ⑨' 渲染层锚点（客户端位置滤波器）：位置滤波器输出 = 游戏画面里模型实际呈现的位姿。
        // 判定层（上方 tp/sp）保持不动——装甲命中几何必须用判定层；此字段仅追加"玩家视角"数据。
        let shooter_render = render_anchor(
            &mut render_cache, author_player_eid, st10.get(&author_player_eid), fire_time, sp);
        let target_render = if hit {
            target_eid.and_then(|teid|
                render_anchor(&mut render_cache, teid, st10.get(&teid), end_time, tp))
        } else { None };
        // 渲染时间线（滑块严格对齐游戏每帧显示位姿）：受击方命中 −3.0~+2.0s、
        // 射手方开火 −2.0~+2.0s，0.1s 步长（滤波器输出，pos 相对锚点）。命中后数据
        // 保留：滤波器误差盒钳位会渐进滑向通道切换后的真相，即游戏当时渲染的画面。
        let target_render_timeline = if hit {
            target_eid.map(|teid| render_timeline(
                &mut render_cache, teid, st10.get(&teid), end_time, -3.0, 2.0, 0.1))
                .unwrap_or_default()
        } else { Vec::new() };
        let shooter_render_timeline = render_timeline(
            &mut render_cache, author_player_eid, st10.get(&author_player_eid),
            fire_time, -3.0, 2.0, 0.1);
        // 炮塔/炮管实时时间线：受击方炮塔角（prop2，命中 −3~+2）、射手炮塔角（开火 −2~+2）、
        // 双方炮管俯仰（prop2 frac 解码；射手无锚定时回退 prop9 瞄准角）——全部走
        // timeline_prop2_client（客户端语义 0.1s 网格，与锚点同一求值器）
        let target_limits = pitch_limits.get(&target_name);
        let target_turret_timeline = if hit {
            target_eid.and_then(|teid| prop2.get(&teid))
                .map(|series| timeline_prop2_client(Some(series), end_time, -3.0, 2.0, None))
                .unwrap_or_default()
        } else { Vec::new() };
        let shooter_turret_timeline = timeline_prop2_client(prop2.get(&author_player_eid), fire_time, -2.0, 2.0, None);
        let shooter_gun_timeline = match shooter_limits {
            Some(lim) => timeline_prop2_client(prop2.get(&author_player_eid), fire_time, -2.0, 2.0, Some(lim)),
            None => timeline_1f(Some(&prop9), fire_time, -2.0, 2.0),
        };
        let target_gun_timeline = if hit {
            target_eid.and_then(|teid| prop2.get(&teid))
                .map(|series| timeline_prop2_client(Some(series), end_time, -3.0, 2.0, target_limits))
                .unwrap_or_default()
        } else { Vec::new() };

        // ⑨'' 服务器下发的受击部件索引（method8 args[10]，报告 §4.7）：hit 时刻 method8 通知的
        // cmpIndex 0..3。与本地 raycast 的部件选择对照 = 命中位置偏差的校准基准。
        let server_part_index = if hit {
            let teid = target_eid.unwrap_or(0);
            direct_hits8.iter()
                .find(|d| d.shooter == author_player_eid && d.victim == teid && (d.t - end_time).abs() <= 0.05)
                .and_then(|d| d.component_index)
        } else { None };

        // ⑨''' 受击者炮管俯仰（原始解码恢复，ea2f8c6）：method8/type=32 的抵达成角 pitch
        // （来向方位角校验通过者，见 decoded_target_gun_pitch）——受击者被命中时的
        // 反向瞄准俯仰，viewer 渲染"炮口指向射手"的炮管俯角。无有效解码时回退车体 pitch。
        let bearing = if tp != [0.0; 3] && sp != [0.0; 3] {
            Some((sp[0] - tp[0]).atan2(sp[2] - tp[2]))
        } else { None };
        let server_gun_pitch = if hit {
            decoded_target_gun_pitch(&warnings32, target_eid.unwrap_or(0), end_time, bearing).map(|(p, _)| p)
        } else { None };

        // ⑩ 目标炮塔朝向 = prop2 + hullYaw（命中弹必须有）；prop2 索引保持包序，min_by_key 首最小语义与原扫包一致
        let state_time = end_time;   // 炮塔/炮管取样基准 = 命中通知时刻（与 WI turret_yaw 同域）
        // ⑩' method8 流序 prop2 快照（WI 逐位对齐，2026-09-24 T110E5 21/21 验证）：method8 包
        // 处理时刻受击者的最后已知 prop2——时钟序"≤t 最后采样"在同 tick 包序错位时会取到
        // method8 之后的更新（炮塔转动中差 1~8 个 coarse 步），流序快照与 WI battle.json 逐位相等。
        let d8_prop2 = if hit {
            let teid = target_eid.unwrap_or(0);
            direct_hits8.iter()
                .find(|d| d.shooter == author_player_eid && d.victim == teid && (d.t - end_time).abs() <= 0.05)
                .and_then(|d| d.victim_prop2)
        } else { None };
        let turret_yaw = if hit {
            let teid = target_eid.unwrap();
            let rel = match d8_prop2 {
                Some((_, v)) => { let (r, _) = decode_prop2_u16(v); Some(r) }
                None => prop2_at_arrived(prop2.get(&teid), state_time).map(|(r, _)| r),
            }
                .ok_or_else(|| anyhow::anyhow!("{}: 目标炮塔朝向（type=7 prop2）缺失", ctx()))?;
            rel + ta[0]
        } else { 0.0 };

        // ⑪ 射手炮塔朝向 = prop2 + 射手 hullYaw，@ 开火时刻（method29 流序快照优先，语义同 ⑩'）
        let shooter_rel = match l.shooter_prop2 {
            Some((_, v)) => { let (r, _) = decode_prop2_u16(v); Some(r) }
            None => prop2_at_arrived(prop2.get(&author_player_eid), fire_time).map(|(r, _)| r),
        }
            .ok_or_else(|| anyhow::anyhow!("{}: 射手炮塔朝向（type=7 prop2）缺失", ctx()))?;
        let shooter_turret_yaw = shooter_rel + sa[0];

        // ⑪' 受击方炮管俯仰 = prop2 frac 比例解码（车型极限锚定）@ 命中通知时刻；
        // 流序快照优先（扇区选择用同一快照的偏航角，与 WI 同基准）；prop2 采样或锚定缺失
        // → 回退车体 pitch（type10，语义不同仅兜底，质量标记 "target"）
        let mut gun_pitch_degraded: Vec<String> = Vec::new();
        let mut pitch_frozen: Vec<String> = Vec::new();
        let target_gun_pitch_val = if hit {
            let teid = target_eid.unwrap();
            match d8_prop2.zip(target_limits) {
                Some(((_, v), lim)) => {
                    let (y, fr) = decode_prop2_u16(v);
                    if prop2_frac_frozen(prop2.get(&teid), state_time) {
                        pitch_frozen.push("target".into());
                    }
                    decode_prop2_gun_pitch(fr, lim, y)
                }
                None => match prop2_at_arrived(prop2.get(&teid), state_time).zip(target_limits) {
                    Some(((y, fr), lim)) => {
                        if prop2_frac_frozen(prop2.get(&teid), state_time) {
                            pitch_frozen.push("target".into());
                        }
                        decode_prop2_gun_pitch(fr, lim, y)
                    }
                    None => { gun_pitch_degraded.push("target".into()); ta[1] }
                },
            }
        } else { ta[1] };

        // ⑫ 射手炮管俯仰 = prop2 frac 比例解码 @ 开火时刻（与受击方同源同锚定；method29 流序快照优先）；
        // prop2/锚定缺失 → 回退 prop9（avatar 瞄准角，狙击模式下≈炮管角），仍缺则 fail-fast
        let (shooter_gun_pitch, shooter_pitch_from_prop9) =
            match l.shooter_prop2.zip(shooter_limits) {
                Some(((_, v), lim)) => {
                    let (y, fr) = decode_prop2_u16(v);
                    if prop2_frac_frozen(prop2.get(&author_player_eid), fire_time) {
                        pitch_frozen.push("shooter".into());
                    }
                    (decode_prop2_gun_pitch(fr, lim, y), false)
                }
                None => match prop2_at_arrived(prop2.get(&author_player_eid), fire_time).zip(shooter_limits) {
                    Some(((y, fr), lim)) => {
                        if prop2_frac_frozen(prop2.get(&author_player_eid), fire_time) {
                            pitch_frozen.push("shooter".into());
                        }
                        (decode_prop2_gun_pitch(fr, lim, y), false)
                    }
                    None => {
                        gun_pitch_degraded.push("shooter".into());
                        (prop9.iter()
                            .min_by_key(|(c, _)| (((*c - fire_time).abs()) * 1000.0) as u32)
                            .map(|(_, v)| *v)
                            .ok_or_else(|| anyhow::anyhow!("{}: 射手炮管俯仰（prop2 与 prop9 均缺失）", ctx()))?, true)
                    }
                },
            };

        // ⑬ aim_point / launch_point_rel = 相对【命中通知状态】目标位置（type10 接地高度）的偏移
        let (aim_point_val, launch_point_rel) = if ball_b != [0.0; 3] && target_eid.is_some() {
            let rel = |p: [f32; 3]| [p[0] - tp[0], p[1] - tp[1], p[2] - tp[2]];
            (rel(ball_b), rel(ball_a))
        } else { ([0.0; 3], [0.0; 3]) };

        // ⑭ 受击坦克 type=10 多 tick 采样（命中 ±1s，位置相对命中通知状态锚点 tp）。只保留命中 tick 及之前的采样：
        // 命中后数据包源切换（AoI 远端基线/延迟缓冲），其后首包位置含数米级瞬移、朝向跳变——混入后 tick 切换会
        // 出现"横着滑移"（位移 ⟂ 履带）与幽灵框朝向错乱（96 段实测 40 段反转）。
        // 窗口放宽到 +0.09 仅作锚点兜底（流里无 |dt|<0.05 命中 tick 时用最近包近似；有真锚点则丢弃其后样本）。
        // 姿态/弹着点滤波只需命中前的连续车体，受击反馈用 hit_flags/segment 表达。
        let mut tick_samples: Vec<TickSample> = Vec::new();
        if hit {
            if let Some(victim) = target_eid {
                if let Some(samples) = st10.get(&victim) {
                    for s in samples {
                        let dt = s.clock - end_time;
                        if dt < -1.0 || dt > 0.09 { continue; }
                        tick_samples.push(TickSample::raw(dt,
                            tick_at(&tick_timeline, s.clock),
                            [s.pos[0] - tp[0], s.pos[1] - tp[1], s.pos[2] - tp[2]],
                            s.yaw, s.pitch, s.roll));
                    }
                }
                trim_tick_samples(&mut tick_samples, refresh_cluster_after(&refresh_clusters, victim, end_time));
                // 坏数据截断已撤回(用户决定)：倒车等"疑似漂移"段是真实记录，完全按回放原始数据渲染；
                // dmin 自动选位亦撤回——双方窗口带 tick 编号(type=35 展开值)由 viewer 手动对齐。
                // ⑭' 合成"游戏渲染位"采样（客户端位置滤波器）：位置滤波器在命中帧的输出，viewer 下拉末项
                // ◎渲染位——与判定锚点（真实 hit tick）对照观察渲染滞后。
                if let Some(r) = &target_render {
                    tick_samples.push(TickSample::render_ghost(0.0,
                        [r.pos[0] - tp[0], r.pos[1] - tp[1], r.pos[2] - tp[2]],
                        r.ang[0], r.ang[1], r.ang[2]));
                }
            }
        }
        // ⑮ 射手坦克 type=10 采样（开火 ±1.0s，世界系绝对坐标）：开火同样触发数据包源切换（位置/朝向跳变），
        // 有真锚点（|dt|<0.05）时只保留锚点及其前采样。前窗取 ±1.0s：过窄(±0.2s)时靠前 tick
        // 会钳死在开火位置，呈现"射手不动"的假象（实际 8m/s 移动 0.8s）。
        let mut shooter_tick_samples: Vec<TickSample> = Vec::new();
        if let Some(samples) = st10.get(&author_player_eid) {
            for s in samples {
                let dt = s.clock - fire_time;
                if dt < -1.0 || dt > 0.09 { continue; }
                shooter_tick_samples.push(TickSample::raw(dt,
                    tick_at(&tick_timeline, s.clock), s.pos, s.yaw, s.pitch, s.roll));
            }
        }
        trim_tick_samples(&mut shooter_tick_samples,
                refresh_cluster_after(&refresh_clusters, author_player_eid, fire_time));
        // ⑮' 合成"游戏渲染位"采样（客户端位置滤波器）：滤波器在开火帧的输出（绝对坐标），viewer 下拉末项
        if let Some(r) = &shooter_render {
            shooter_tick_samples.push(TickSample::render_ghost(0.0, r.pos, r.ang[0], r.ang[1], r.ang[2]));
        }

        out.push(ShotReplayData {
            index: i + 1,
            time_s: fire_time,
            damage,
            target_name,
            is_kill,
            shooter_eid: author_player_eid,
            shooter_name: names.get(&author_player_eid).cloned().unwrap_or_default(),
            is_author: true,
            shooter_pos: sp,
            shooter_ang: sa,
            target_pos: tp,
            target_ang: ta,
            target_turret_yaw: turret_yaw,
            target_gun_pitch: target_gun_pitch_val,
            target_gun_pitch_server: server_gun_pitch.is_some(),
            type32_turret_yaw: 0.0,
            shooter_turret_yaw,
            shooter_gun_pitch,
            aim_point: aim_point_val,
            launch_point_rel,
            ball_a,
            ball_b,
            launch_velocity: l.vel,
            hit_flags,
            crit_modules,
            destroyed_modules,
            segment,
            shell_id,
            armor_group,
            hit_triangle,
            game_hit_result,
            hit_token,
            modifiers,
            shell_slot,
            fire_time,
            shot_id,
            incoming_yaw: 0.0,
            incoming_pitch: ta[1],
            tick_samples,
            shooter_tick_samples,
            fire_tick,
            terrain_impact,
            shooter_aim,
            quality: Some(ShotQuality {
                shooter_state_dt_ms: (sp_dt * 1000.0).round() as i32,
                shooter_pos_from_muzzle: false,
                target_state_dt_ms: if hit { Some((tp_dt * 1000.0).round() as i32) } else { None },
                turret_degraded: Vec::new(),   // 作者路径 prop2 缺失即 fail-fast，不存在降级
                dmg_unattributed,
                shell_from_broadcast,
                shooter_pitch_from_velocity: false,
                shooter_pitch_from_prop9,
                gun_pitch_degraded,
                pitch_frozen,
                shooter_anchor_src: if sp_src != "filtered" { Some(sp_src.into()) } else { None },
                // 受击方 filtered 仅出现于 method8 缺失回退路径（正常路径为 wi_hit_state），需序列化提示
                target_anchor_src: if hit && tp_src != "wi_hit_state" { Some(tp_src.into()) } else { None },
            }),
            shooter_render,
            target_render,
            target_render_timeline,
            shooter_render_timeline,
            target_turret_timeline,
            shooter_turret_timeline,
            shooter_gun_timeline,
            target_gun_timeline,
            server_part_index,
        });
    }

    // ⑧' method38 = 作者自己的命中反馈——每条都必须配对到一次发射
    if hr_cursor < hit_results.len() {
        anyhow::bail!("存在未被任何发射配对的 method38 命中结果（{} 条未消费，自 t={:.2}s 起）——发射/命中配对不完整",
            hit_results.len() - hr_cursor, hit_results[hr_cursor].t);
    }

    Ok(out)
}

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
    extract_shot_replays_auto_with_limits(packets, file_name, &GunPitchLimits::new())
}

/// [`extract_shot_replays_auto`] 的完整形态（`pitch_limits` 语义见 [`extract_shot_replays_with_limits`]）。
pub fn extract_shot_replays_auto_with_limits(
    packets: &[(u32, f32, &[u8])],
    file_name: &str,
    pitch_limits: &GunPitchLimits,
) -> anyhow::Result<Vec<ShotReplayData>> {
    let author_player_eid = resolve_author_player_eid(packets, file_name);
    extract_shot_replays_with_limits(packets, author_player_eid, pitch_limits)
}

/// 他人路径提取结果：射击列表 + 回退/跳过统计（web 响应 notes 供用户了解数据边界）。
pub struct OtherShotsExtraction {
    pub shots: Vec<ShotReplayData>,
    /// method29 发射总数（含被跳过者）
    pub total_launches: usize,
    /// method20 弹道终点缺失 → 整发跳过（无复现基准）
    pub skipped_no_endpoint: usize,
    /// 受击方状态完全缺失（method8 无状态且 type=10 无采样）→ 整发跳过
    pub skipped_no_target_state: usize,
    /// 射手 type=10 缺失 → 炮口坐标兜底（未跳过，per-shot 另有 quality 标记）
    pub muzzle_fallback: usize,
}

/// 其他玩家（队友/敌方）射击的宽松提取：与 [`extract_shot_replays`] 同源数据，但 Avatar 专属包不可得，对应字段降级：
/// method38 命中反馈（作者专属）→ hit_flags/crit/destroyed/modifiers 恒空，结果 = method8 result 枚举；目标 = method8 就近匹配（±0.05s）；
/// segment/弹种/装甲组 = method8 hash6 令牌 ↔ type=32 精确配对（86/86 实测同源）；伤害 = 血量链降幅（source=射手，cause=0）；
/// 双方炮管俯仰 = prop2 frac 解码（无锚定时射手回退发射速度向量、受击方回退车体 pitch）；
/// 0x1b 地形命中 / method36 瞄准快照 / type=28 弹药槽 = Avatar 专属 → None / None / 0。
/// 宽松模式：数据缺失的射击跳过并计数，绝不 bail（AoI 裁剪致远端数据稀疏是预期，与作者路径 fail-fast 不同）。
pub fn extract_other_shot_replays(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
) -> OtherShotsExtraction {
    extract_other_shot_replays_with_limits(packets, author_player_eid, &GunPitchLimits::new())
}

/// [`extract_other_shot_replays`] 的完整形态（`pitch_limits` 语义见 [`extract_shot_replays_with_limits`]）。
pub fn extract_other_shot_replays_with_limits(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
    pitch_limits: &GunPitchLimits,
) -> OtherShotsExtraction {
    // ① 全部非作者的 method29 发射事件（作者由严格路径处理；shotId 全局去重）；args<37 的包直接跳过
    let (launches, _) = collect_launches(packets, |e| e != author_player_eid);

    // ② method20 弹道终点（shotId 配对，全量收集，与作者路径同构）
    let endpoints = collect_endpoints(packets);

    // ③ method8 直击通知（全局广播）
    let direct_hits8 = collect_direct_hits8(packets);

    // ③' type=32 命中通知（AoI 广播含他人；按 hash6 令牌与 method8 精确配对）
    let warnings32 = collect_warnings32(packets);

    let names = extract_entity_names(packets);

    // ④ type=35 tick 计数器展开（与作者路径同构）
    let tick_timeline = collect_tick_timeline(packets);

    // ⑤ 血量链降幅区间（全 source 保留——作者路径 ⑥' 的无过滤版本，cause=0 炮弹直击；含 type=5 满血锚点）
    let dmg_losses = derive_dmg_losses(&parse_hp_events(packets), None, &collect_initial_hp(packets));
    // ⑤' 互斥预归属（修复 1436 类错配）：每段降幅给区间内 end_time 最大的命中发射——
    // 旧 find 首个匹配会把同区间未穿弹也计入并重复计数（1436：44.865/53.972 未穿各
    // 抢 322、200.256 未穿抢走 261.662 的 428）。目标识别与循环内同式（method8 ±0.05s）。
    let shot_dmg_inputs: Vec<Option<(f32, u32, u32)>> = launches.iter().map(|l| {
        let (end_time, _) = endpoints.get(&l.shot_id).copied()?;
        let teid = direct_hits8.iter()
            .filter(|d| d.shooter == l.shooter && (d.t - end_time).abs() <= 0.05)
            .min_by(|x, y| (x.t - end_time).abs().partial_cmp(&(y.t - end_time).abs()).unwrap_or(std::cmp::Ordering::Equal))
            .map(|d| d.victim)?;
        Some((end_time, l.shooter, teid))
    }).collect();
    let dmg_assign = assign_dmg_losses(&shot_dmg_inputs, &dmg_losses);

    // ⑥ type=10 状态索引 + type=7 prop2 炮塔偏航索引（每实体）：逐发全量扫描在 ~30 射手 × 数百发下不可行。
    let (st10, prop2) = build_entity_indexes(packets);
    // 补发簇签名（AoI 通道切换点，逆向文档 6.x）——tick 采样截断 + 锚点选择依据
    let refresh_clusters = collect_refresh_clusters(packets);
    let anchor_at = |eid: u32, t: f32| -> Option<([f32; 3], [f32; 3], f32, &'static str)> {
        select_anchor_state(st10.get(&eid)?, &refresh_clusters, eid, t)
    };
    let prop2_yaw_at = |eid: u32, t: f32| -> Option<f32> {
        prop2_at_arrived(prop2.get(&eid), t).map(|(r, _)| r)
    };

    // ⑦ 逐发组装（宽松：缺数据跳过 + 计数）
    let mut out: Vec<ShotReplayData> = Vec::with_capacity(launches.len());
    // 渲染层锚点时间线缓存（滤波器，处理分析报告 §7）：per-entity 惰性构建
    let mut render_cache: HashMap<u32, FilteredTimeline> = HashMap::new();
    let mut skipped_no_endpoint = 0usize;
    let mut skipped_no_target_state = 0usize;
    let mut muzzle_fallback = 0usize;
    for (li, l) in launches.iter().enumerate() {
        // ctx 仅在跳过/兜底打印时格式化（避免逐发无条件分配）
        let ctx = || format!("shotId={} shooter={:08x} t={:.2}s", l.shot_id, l.shooter, l.t);
        let ball_a = l.point;
        let (end_time, ball_b) = match endpoints.get(&l.shot_id) {
            Some(x) => *x,
            None => { skipped_no_endpoint += 1; eprintln!("[replay_others] 跳过 {}: method20 终点缺失", ctx()); continue; }
        };
        // 射手状态快照；AoI 裁剪缺失时用炮口坐标兜底（ball_a = method29 服务器权威发射位置），朝向从速度向量推算，不整发跳过
        let (sp, sa, sp_dt, sp_src, pos_from_muzzle) = match anchor_at(l.shooter, l.t) {
            Some((pos, ang, dt, src)) => (pos, ang, dt, src, false),
            None => {
                let yaw = l.vel[2].atan2(l.vel[0]);
                let pitch = {
                    let horiz = (l.vel[0] * l.vel[0] + l.vel[2] * l.vel[2]).sqrt();
                    if horiz > 1e-6 { l.vel[1].atan2(horiz) } else { 0.0 }
                };
                muzzle_fallback += 1;
                eprintln!("[replay_others] {}: 射手 type=10 状态缺失，用炮口坐标兜底", ctx());
                (ball_a, [yaw, pitch, 0.0], 0.0, "nearest", true)
            }
        };
        let fire_tick = tick_at(&tick_timeline, l.t);

        // 目标 = method8 就近匹配（窗口 ±0.05s，同作者路径命中窗口）
        let dhit = direct_hits8.iter()
            .filter(|d| d.shooter == l.shooter && (d.t - end_time).abs() <= 0.05)
            .min_by(|x, y| (x.t - end_time).abs().partial_cmp(&(y.t - end_time).abs()).unwrap_or(std::cmp::Ordering::Equal));
        let target_eid = dhit.map(|d| d.victim);
        let hit = target_eid.is_some();

        // segment/弹种/装甲组：method8 hash6 令牌 ↔ type=32 精确配对（优于时间窗）
        let mut segment: u64 = 0;
        let mut shell_id: u32 = 0;
        let mut armor_group: u8 = 0;
        let mut hit_triangle: u16 = 0;
        let mut game_hit_result: u8 = dhit.map(|d| d.result).unwrap_or(255);
        let mut hit_token: Option<String> = None;
        if let Some(d) = dhit {
            hit_token = Some(d.hash6.iter().map(|b| format!("{:02x}", b)).collect());
            if let Some(teid) = target_eid {
                if let Some(w) = warnings32.iter().find(|w| w.eid == teid && w.hash6 == d.hash6) {
                    segment = w.segment;
                    game_hit_result = w.result;
                    let sb = segment.to_le_bytes();
                    shell_id = (sb[1] as u32) | ((sb[2] as u32) << 8) | ((sb[3] as u32) << 16);
                    armor_group = sb[7];
                    hit_triangle = u16::from_be_bytes([sb[5], sb[6]]);
                }
            }
        }

        // 伤害归属：互斥预归属结果（assign_dmg_losses：区间内 end_time 最大者得降幅）
        let mut damage = 0u32;
        let mut is_kill = false;
        let mut dmg_unattributed = false;
        let target_name = target_eid.and_then(|e| names.get(&e).cloned()).unwrap_or_default();
        if let Some(d) = dhit {
            if let Some(&(dm, hp_cur)) = dmg_assign.get(&li) {
                damage = dm;
                is_kill = hp_cur == 0;
            } else if d.result == 3 || d.result == 4 {
                // 应伤结果（3=击穿 / 4=履带·模块交互可带伤）却无降幅 = 服务器未记账 HP
                dmg_unattributed = true;
            }
            // result=1/2（未穿/间隙止）无降幅 = 正常零伤，不打标
        }

        // 目标状态 @ 命中通知状态（WI 对齐锚点，与作者路径同源：dhit 快照即受击者在
        // method8 处理时刻的运行状态；状态缺失回退 end_time 插值，仍缺则整发跳过）
        let (tp, ta, tp_dt, tp_src) = if let Some(d) = dhit {
            match d.victim_state {
                // 与作者路径同源：method8 通知状态 = WI 正常语义（不告警）
                Some((pos, ang, state_clock)) => (pos, ang, state_clock - end_time, "wi_hit_state"),
                None => match anchor_at(d.victim, end_time) {
                    Some(x) => x,
                    None => { skipped_no_target_state += 1; eprintln!("[replay_others] 跳过 {}: 命中时刻目标 type=10 状态缺失", ctx()); continue; }
                },
            }
        } else {
            ([0.0; 3], [0.0; 3], 0.0, "nearest")
        };

        // 渲染层锚点（客户端位置滤波器，与作者路径同式）：射手炮口兜底时 sp 非车体位姿，跳过射手侧渲染锚点
        let shooter_render = if !pos_from_muzzle {
            render_anchor(&mut render_cache, l.shooter, st10.get(&l.shooter), l.t, sp)
        } else { None };
        let target_render = if hit {
            target_eid.and_then(|teid|
                render_anchor(&mut render_cache, teid, st10.get(&teid), end_time, tp))
        } else { None };
        // 渲染时间线（与作者路径同式）：受击方命中 −3.0~+2.0s、射手方开火 −2.0~+2.0s
        let target_render_timeline = if hit {
            target_eid.map(|teid| render_timeline(
                &mut render_cache, teid, st10.get(&teid), end_time, -3.0, 2.0, 0.1))
                .unwrap_or_default()
        } else { Vec::new() };
        let shooter_render_timeline = if !pos_from_muzzle {
            render_timeline(&mut render_cache, l.shooter, st10.get(&l.shooter),
                l.t, -3.0, 2.0, 0.1)
        } else { Vec::new() };
        let target_turret_timeline = if hit {
            target_eid.and_then(|teid| prop2.get(&teid))
                .map(|series| timeline_prop2_client(Some(series), end_time, -3.0, 2.0, None))
                .unwrap_or_default()
        } else { Vec::new() };
        let shooter_turret_timeline = timeline_prop2_client(prop2.get(&l.shooter), l.t, -2.0, 2.0, None);
        // 双方炮管俯仰时间线（prop2 frac 解码；无锚定表则空——射手速度向量只在开火帧存在，无时间线可言）
        let shooter_name_val = names.get(&l.shooter).cloned().unwrap_or_default();
        let shooter_limits = pitch_limits.get(&shooter_name_val);
        let target_limits = pitch_limits.get(&target_name);
        let shooter_gun_timeline = match shooter_limits {
            Some(lim) => timeline_prop2_client(prop2.get(&l.shooter), l.t, -2.0, 2.0, Some(lim)),
            None => Vec::new(),
        };
        let target_gun_timeline = if hit {
            target_eid.and_then(|teid| prop2.get(&teid))
                .map(|series| timeline_prop2_client(Some(series), end_time, -3.0, 2.0, target_limits))
                .unwrap_or_default()
        } else { Vec::new() };
        // 服务器受击部件索引（method8 args[10]，报告 §4.7）
        let server_part_index = dhit.and_then(|d| d.component_index);
        // 受击者炮管俯仰（原始解码恢复）：来向方位角校验通过的抵达成角 pitch
        let bearing = if tp != [0.0; 3] && sp != [0.0; 3] {
            Some((sp[0] - tp[0]).atan2(sp[2] - tp[2]))
        } else { None };
        let server_gun_pitch = decoded_target_gun_pitch(&warnings32, target_eid.unwrap_or(0), end_time, bearing).map(|(p, _)| p);

        // 炮塔朝向（prop2 相对角 @ 命中通知时刻；method8 流序快照优先 = WI 逐位同基准，
        // 见 DirectHit8::victim_prop2；AoI 裁剪缺失时降级为车体朝向，不跳过）
        let d8_prop2 = dhit.and_then(|d| d.victim_prop2);
        let mut turret_degraded: Vec<String> = Vec::new();
        let mut gun_pitch_degraded: Vec<String> = Vec::new();
        let mut pitch_frozen: Vec<String> = Vec::new();
        let target_turret_yaw = if hit {
            let teid = target_eid.unwrap_or(0);
            let rel = match d8_prop2 {
                Some((_, v)) => { let (r, _) = decode_prop2_u16(v); Some(r) }
                None => prop2_yaw_at(teid, end_time),
            };
            match rel {
                Some(rel) => rel + ta[0],
                None => { turret_degraded.push("target".into()); ta[0] }
            }
        } else { 0.0 };
        let shooter_turret_yaw = match l.shooter_prop2.map(|(_, v)| decode_prop2_u16(v).0).or_else(|| prop2_yaw_at(l.shooter, l.t)) {
            Some(rel) => rel + sa[0],
            None => { if !pos_from_muzzle { turret_degraded.push("shooter".into()); } sa[0] }
        };

        // 炮管俯仰：prop2 frac 比例解码（双方同源，method29/8 流序快照优先）；prop2/锚定缺失
        // → 射手回退发射速度向量反解（垂直/水平分量），受击方回退车体 pitch，均打质量标记
        let shooter_gun_pitch = match l.shooter_prop2.zip(shooter_limits) {
            Some(((_, v), lim)) => {
                let (y, fr) = decode_prop2_u16(v);
                if prop2_frac_frozen(prop2.get(&l.shooter), l.t) {
                    pitch_frozen.push("shooter".into());
                }
                decode_prop2_gun_pitch(fr, lim, y)
            }
            None => match prop2_at_arrived(prop2.get(&l.shooter), l.t).zip(shooter_limits) {
                Some(((y, fr), lim)) => {
                    if prop2_frac_frozen(prop2.get(&l.shooter), l.t) {
                        pitch_frozen.push("shooter".into());
                    }
                    decode_prop2_gun_pitch(fr, lim, y)
                }
                None => {
                    gun_pitch_degraded.push("shooter".into());
                    let horiz = (l.vel[0] * l.vel[0] + l.vel[2] * l.vel[2]).sqrt();
                    if horiz > 1e-6 { l.vel[1].atan2(horiz) } else { 0.0 }
                }
            },
        };
        let target_gun_pitch_val = if hit {
            let teid = target_eid.unwrap();
            match d8_prop2.zip(target_limits) {
                Some(((_, v), lim)) => {
                    let (y, fr) = decode_prop2_u16(v);
                    if prop2_frac_frozen(prop2.get(&teid), end_time) {
                        pitch_frozen.push("target".into());
                    }
                    decode_prop2_gun_pitch(fr, lim, y)
                }
                None => match prop2_at_arrived(prop2.get(&teid), end_time).zip(target_limits) {
                    Some(((y, fr), lim)) => {
                        if prop2_frac_frozen(prop2.get(&teid), end_time) {
                            pitch_frozen.push("target".into());
                        }
                        decode_prop2_gun_pitch(fr, lim, y)
                    }
                    None => { gun_pitch_degraded.push("target".into()); ta[1] }
                },
            }
        } else { ta[1] };

        // aim_point / launch_point_rel = 相对命中通知状态目标位置的偏移（与作者路径同式）
        let (aim_point_val, launch_point_rel) = if ball_b != [0.0; 3] && target_eid.is_some() {
            let rel = |p: [f32; 3]| [p[0] - tp[0], p[1] - tp[1], p[2] - tp[2]];
            (rel(ball_b), rel(ball_a))
        } else { ([0.0; 3], [0.0; 3]) };

        // 受击方 type=10 采样（命中 ±1s，相对命中通知状态锚点；锚点截断规则同作者路径 ⑭）
        let mut tick_samples: Vec<TickSample> = Vec::new();
        if let Some(victim) = target_eid {
            if let Some(samples) = st10.get(&victim) {
                for s in samples {
                    let dt = s.clock - end_time;
                    if dt < -1.0 || dt > 0.09 { continue; }
                    tick_samples.push(TickSample::raw(dt,
                        tick_at(&tick_timeline, s.clock),
                        [s.pos[0] - tp[0], s.pos[1] - tp[1], s.pos[2] - tp[2]],
                        s.yaw, s.pitch, s.roll));
                }
                trim_tick_samples(&mut tick_samples, refresh_cluster_after(&refresh_clusters, victim, end_time));
                // 合成"游戏渲染位"采样（同作者路径 ⑭'）
                if let Some(r) = &target_render {
                    tick_samples.push(TickSample::render_ghost(0.0,
                        [r.pos[0] - tp[0], r.pos[1] - tp[1], r.pos[2] - tp[2]],
                        r.ang[0], r.ang[1], r.ang[2]));
                }
            }
        }
        // 射手 type=10 采样（开火 ±1s，世界系绝对坐标；规则同作者路径 ⑮）
        let mut shooter_tick_samples: Vec<TickSample> = Vec::new();
        if let Some(samples) = st10.get(&l.shooter) {
            for s in samples {
                let dt = s.clock - l.t;
                if dt < -1.0 || dt > 0.09 { continue; }
                shooter_tick_samples.push(TickSample::raw(dt,
                    tick_at(&tick_timeline, s.clock), s.pos, s.yaw, s.pitch, s.roll));
            }
            trim_tick_samples(&mut shooter_tick_samples,
                refresh_cluster_after(&refresh_clusters, l.shooter, l.t));
            // 合成"游戏渲染位"采样（同作者路径 ⑮'）
            if let Some(r) = &shooter_render {
                shooter_tick_samples.push(TickSample::render_ghost(0.0, r.pos, r.ang[0], r.ang[1], r.ang[2]));
            }
        }

        out.push(ShotReplayData {
            index: 0,   // 合并后由调用方按 time_s 全局重编号
            time_s: l.t,
            damage,
            target_name,
            is_kill,
            shooter_eid: l.shooter,
            shooter_name: names.get(&l.shooter).cloned().unwrap_or_default(),
            is_author: false,
            shooter_pos: sp,
            shooter_ang: sa,
            target_pos: tp,
            target_ang: ta,
            target_turret_yaw,
            target_gun_pitch: target_gun_pitch_val,
            target_gun_pitch_server: server_gun_pitch.is_some(),
            type32_turret_yaw: 0.0,
            shooter_turret_yaw,
            shooter_gun_pitch,
            aim_point: aim_point_val,
            launch_point_rel,
            ball_a,
            ball_b,
            launch_velocity: l.vel,
            hit_flags: 0,           // method38 作者专属，他人不可得（结果看 game_hit_result）
            crit_modules: 0,
            destroyed_modules: 0,
            segment,
            shell_id,
            armor_group,
            hit_triangle,
            game_hit_result,
            hit_token,
            modifiers: Vec::new(),
            shell_slot: 0,          // type=28 弹药槽是作者本人的选择状态，对他人无意义
            fire_time: l.t,
            shot_id: l.shot_id,
            incoming_yaw: 0.0,
            incoming_pitch: ta[1],
            tick_samples,
            shooter_tick_samples,
            fire_tick,
            terrain_impact: None,   // 0x1b 地形命中 = 作者 Avatar 专属
            shooter_aim: None,      // method36 瞄准快照 = 作者 Avatar 专属
            quality: Some(ShotQuality {
                shooter_state_dt_ms: (sp_dt * 1000.0).round() as i32,
                shooter_pos_from_muzzle: pos_from_muzzle,
                target_state_dt_ms: if hit { Some((tp_dt * 1000.0).round() as i32) } else { None },
                turret_degraded,
                dmg_unattributed,
                shell_from_broadcast: false,   // 他人无广播兜底：type=32 未配对则弹种直接未知（shell_id=0）
                shooter_pitch_from_velocity: gun_pitch_degraded.iter().any(|s| s == "shooter"),
                shooter_pitch_from_prop9: false,
                gun_pitch_degraded,
                pitch_frozen,
                shooter_anchor_src: if sp_src != "filtered" { Some(sp_src.into()) } else { None },
                // 受击方 filtered 仅出现于 method8 缺失回退路径（正常路径为 wi_hit_state），需序列化提示
                target_anchor_src: if hit && tp_src != "wi_hit_state" { Some(tp_src.into()) } else { None },
            }),
            shooter_render,
            target_render,
            target_render_timeline,
            shooter_render_timeline,
            target_turret_timeline,
            shooter_turret_timeline,
            shooter_gun_timeline,
            target_gun_timeline,
            server_part_index,
        });
    }
    eprintln!("[replay_others] 其他玩家射击提取: {} 发（终点缺失跳过 {}、受击方状态缺失跳过 {}、射手状态炮口兜底 {}）",
        out.len(), skipped_no_endpoint, skipped_no_target_state, muzzle_fallback);
    OtherShotsExtraction {
        shots: out,
        total_launches: launches.len(),
        skipped_no_endpoint,
        skipped_no_target_state,
        muzzle_fallback,
    }
}
