use serde::{Deserialize, Serialize};
use std::collections::HashMap;

//  回放的 `data.wotreplay` 数据包流里，type=7 的事件包有多种子类型

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
    /// 把原始数据包列表解码成事件时间线。
    pub fn parse_packets(packets: &[(u32, f32, &[u8])]) -> Self {
        let entity_names = extract_entity_names(packets);
        let mut events = Vec::new();
        let mut entity_health: HashMap<u32, u16> = HashMap::new();
        let mut death_entities = std::collections::HashSet::new();

        for (pkt_type, clock, payload) in packets {
            // 只处理 type=7（事件）包，且载荷至少 8 字节（含实体 ID + 子类型）
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

    /// 推断"每发射击"：关联作者的伤害计数器递增与敌方生命值下降。
    ///
    /// 思路：作者伤害计数器（sub=10）每次递增即代表一次开炮命中，从递增差值
    /// 得到本次伤害；再在同一时刻附近找到血量下降的敌方实体作为目标。
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

/// 受击坦克 type=10 采样（用于渲染多 tick 幽灵框，测试延迟假设）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickSample {
    /// 相对命中时刻的偏移（秒）
    pub dt: f32,
    /// 相对命中时刻锚点的位置偏移（世界系，米）
    pub pos: [f32; 3],
    /// 车体偏航（弧度）
    pub yaw: f32,
    /// 车体俯仰（弧度）
    pub pitch: f32,
    /// 车体侧倾（弧度）
    pub roll: f32,
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
    /// 命中结果位图（u32 = flags16 | headerHi16<<16；wotinspector hit_flags 同源）。
    /// 已实证位：0x0001 直接击杀 / 0x0008 跳弹 / 0x0010 材料击穿 / 0x0020 未击穿（材料止）/
    /// 0x0040 间隙层被穿透 / 0x0080 间隙层未穿 / 0x0100 内部模块被击穿 / 0x0400 履带受损 /
    /// 0x0800 火炮受损 / 0x1000 HE 爆炸伤害分支；0x20000 = headerHi 基础位（wotinspector
    /// 样本中所有非零 hit_flags 均含此位，本地 4 个回放 headerHi 恒 0x0002 ✓）。
    pub hit_flags: u32,
    /// 模块受损位掩码——wotinspector crit_modules 同源。
    /// bit = componentToken - 31（token 31..43 → bit 0..12；实测对齐：token33 受损 → WI bit2=0x04 ✓）。
    pub crit_modules: u32,
    /// 模块摧毁位掩码（映射同上，state=2；实测 token35 履带摧毁 → bit4）。
    pub destroyed_modules: u32,
    /// 游戏原生命中段 u64（type=32 警告包尾 8 字节 LE），布局（WI 罗塞塔对照修订）：
    /// `[result u8][shell_global_id u24 LE][0x00][X][Y][Z]`
    /// - result：命中结果枚举（同 game_hit_result）
    /// - shell_global_id（u24 LE = B1,B2,B3）：游戏全局弹种 id =
    ///   `(shells.xml 局部 id << 8) | 国家基数字节`（国家基数 = nation_id×16+10：
    ///   uk=0x5a、japan=0x6a、usa=0x2a）。实测：GSOR AP 局部 2040 → 全局 522330、
    ///   金HE 2039 → 522074，与 WI shotsimulate 状态 blob 的 shell 字段逐发一致 ✓；
    ///   Type2605 112 → 28778、XM551 2018 → 516650 ✓
    /// - 字节4 恒 0x00；末 3 字节逐发变化，语义未解（WI 的 segment 字段 =
    ///   `[result][layer][hash6]`，同样不含此 3 字节——服务器不下发片元编号）。
    ///   服务器仅转发部分警告（GB109 覆盖 7/10），0 = 未获取。
    pub segment: u64,
    /// 命中弹种全局 id（24 位，含国家基数字节；与 WI shell_id 同值同源；0 = 未获取）
    pub shell_id: u32,
    /// segment 字节6（语义未解，保留透传）
    pub armor_group: u8,
    /// segment 字节5、6 组成的 u16 BE（语义未解，保留兼容）
    pub hit_triangle: u16,
    /// 游戏命中结果枚举（method8 b9 / type=32 segment 低字节同源，86/86 事件实测一致）：
    /// 0=无命中结果 1=未击穿 2=间隙层止 3=有伤害（击穿/HE 爆炸）4=跳弹；255 = 未获取。
    pub game_hit_result: u8,
    /// 命中令牌（method8 ↔ type=32 同事件共享的 6 字节哈希，86/86 一致；
    /// 用于未来与贴花/结算数据交叉引用）。
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
    /// 受击坦克 type=10 采样（命中时刻 ±1s，位置相对命中锚点，世界系米）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tick_samples: Vec<TickSample>,
    /// 射手坦克 type=10 采样（开火 ±0.2s，世界系绝对坐标）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shooter_tick_samples: Vec<TickSample>,
    /// 地形命中数据（Avatar method 0x1b；仅当该发未命中任何坦克且服务器广播时存在）。
    /// args(34) = [shotId u32][shell_global_id u32][material u8]
    ///            [impactPoint 3×f32][segmentStartPoint 3×f32][tail u8]
    /// - impact_point == method20 弹道终点（4 回放实测逐发一致）；
    /// - segment_start = 弹道末段起点（直线弹 = method29 发射点，误差 0.000m；
    ///   其余为弹跳点——地面跳弹后末段的起始位置）；
    /// - material：落点材质类（观测 0/1/2/4/5，命名未定）。
    /// 约覆盖 2/3 的地形弹（其余命中岩石/建筑/残骸等非地形静态物，无此包）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terrain_impact: Option<TerrainImpactData>,
    /// 开火时刻瞄准快照（Avatar method36；战斗初始/瞄准变化外的开火成对快照，
    /// 缺失时 None）。炮塔相对偏航为 f64 全精度（prop2 为 u16 量化）；
    /// 成对 state_before/after 为未定名状态常量（非扩散度，见 ShooterAimData 注）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shooter_aim: Option<ShooterAimData>,
    /// 兼容旧字段：= type32_turret_yaw（曾误标为"来袭方向"，实为受击者炮塔角）。
    pub incoming_yaw: f32,
    /// 兼容旧字段：= target_gun_pitch（曾误标为"来袭俯角"，实为受击者炮管俯仰）。
    pub incoming_pitch: f32,
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

/// Avatar method36 (0x24) 开火时刻瞄准快照（可选，fail-soft：无快照则 None）。
/// args = [payloadLen u8][protobuf]：field1(f64)=炮塔相对车体偏航（与 prop2 同语义、
/// f64 全精度，实测 |Δ|≤0.024 rad）。开火时刻成对出现（射击前/后各一条，f1 恒同）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShooterAimData {
    /// 炮塔相对车体偏航（rad，开火时刻 f64 全精度）
    pub turret_rel_yaw: f64,
    /// 成对快照 field6.field1（射击前；跨坦克/跨发实测恒 ≈0.842——
    /// 旧标注"扩散度"与实测矛盾，语义未定，透传供后续研究）
    pub state_before: f64,
    /// 成对快照 field6.field1（射击后；实测恒 ≈0.906；无成对快照时 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_after: Option<f64>,
}

/// protobuf 最小遍历：varint / fixed64 / 定长子消息，返回 (field_no, wire_type, 内容偏移, 内容长)。
/// 仅用于 method36 快照；格式不合法返回 None（fail-soft 调用方忽略）。
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

/// type=10 相邻快照段的运动学合理性检验（用于剔除服务器纠偏平滑轨迹）。
///
/// 回放快照流是客户端显示位置（本地预测 + 服务器纠偏平滑收敛），当客户端
/// 预测与服务器权威位置发散时，流中出现沿车体反方向的连续"倒车滑移"段：
/// 速度可达真实倒车极限的 2~3 倍（实测重坦 39 km/h 倒车、0→9 m/s 0.7s），
/// 且弹道几何不可达（炮口偏移 3~5 m）。坦克真实运动学约束：
/// 倒车 ≤5.5 m/s（≈20 km/h，全游戏倒车上限）、侧移 ≤5.0 m/s（坦克不可能
/// 持续侧移）、前向 ≤30 m/s（快车俯坡）、加速度 ≤8 m/s²（坦克加/制动极限；
/// 纠偏滑移实测 ±14~35 m/s²）。位移 <0.25 m 的微跳不截断（保留轨迹连续性，
/// 避免把真实轨迹末端的厘米级纠偏误杀）。
/// [已撤回] 坏数据截断逻辑:原用于剔除命中前"疑似漂移"的采样前缀,实测会把
/// 真实倒车(LT-432 等轻坦倒车极速 >20km/h,超 5.5m/s 阈值)整段误杀,
/// 导致 tick 切换丢失。用户决定完全按回放原始数据渲染,不再调用;
/// 函数体保留供将来参考。
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

/// 全链扫描，截除最后一段不合理样本之前的前缀（其后样本已收敛到服务器
/// 权威基线；此前样本位于纠偏漂移基线上，相对锚点整体错位，不可用）。
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
    //    args 布局（WotbTools PROVEN + 4 回放实测）：[victimVehicleId u32][resultFlags16 u16]
    //    [headerHi16 u16][resultCount u8][resultCount × (componentToken u8 + rawState u8)]
    //    [modifierCount u8][modifierCount × modifierId u32]
    //    rawState：0=无变化 1=受损(crit) 2=摧毁；组件号 31=引擎 32=弹药架 33=油箱
    //    34/35=右/左履带 36=火炮 38=观察装置（WotbTools 枚举）
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

    // ③' 同钟同受击者合并：一发命中可能产生多条结果消息（多次装甲交互——
    // （WotbTools hit-resolution 同原则：同钟重复记录计为一次命中事件。）
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
    let hit_results = merged38;

    // ③'' type=32 来袭炮弹警告/命中通知（eid = 受击者，AoI 广播含他人命中）：
    // len=26 (method 0x11): [eid u32][01][method u32][u16@9][flag@11][hash6@12..18][segment u64@18..26]
    // len=27 (method 0x12): [eid u32][01][method u32][u16@9][flag@11][01@12][hash6@13..19][segment u64@19..27]
    // segment u64 低字节 = 命中结果枚举（与 method8 b9 同域，86/86 实测一致）。
    struct ArenaWarning32 { t: f32, eid: u32, result: u8, segment: u64, hash6: [u8; 6] }
    let mut warnings32: Vec<ArenaWarning32> = Vec::new();
    for (t, clock, p) in packets {
        if *t != 32 || p.len() < 26 { continue; }
        if p[4] != 0x01 { continue; }
        let method = u32::from_le_bytes([p[5], p[6], p[7], p[8]]);
        if method != 0x11 && method != 0x12 { continue; }
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
        });
    }
    warnings32.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());

    // ③''' Vehicle method8 直击通知（全局广播，envelope eid = 受击者）：
    // [shooterEntityId u32][victimEntityId u32][01][result u8][extra u8][hash6][tail...]
    // result 枚举与 type=32 同域；hash6 与同事件 type=32 完全一致（86/86 实测）。
    struct DirectHit8 { t: f32, shooter: u32, victim: u32, result: u8, hash6: [u8; 6] }
    let mut direct_hits8: Vec<DirectHit8> = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 12 + 10 { continue; }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x08 { continue; }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 10 || 12 + args_len > p.len() { continue; }
        let a = &p[12..12 + args_len];
        if a[8] != 0x01 { continue; }
        direct_hits8.push(DirectHit8 {
            t: *clock,
            shooter: u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
            victim: u32::from_le_bytes([a[4], a[5], a[6], a[7]]),
            result: a[9],
            hash6: [a[11], a[12], a[13], a[14], a[15], a[16]],
        });
    }
    direct_hits8.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());

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

    // ⑤' 弹药选择时间线（type=28，payload=u32 LE 槽位；录像者本人的选择状态）
    let mut ammo_selects: Vec<(f32, u32)> = Vec::new();
    for (t, clock, p) in packets {
        if *t != 28 || p.len() < 4 { continue; }
        ammo_selects.push((*clock, u32::from_le_bytes([p[0], p[1], p[2], p[3]])));
    }
    ammo_selects.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    // ⑤'' Avatar method 0x07 弹种广播时间线：args(5) = [a0 u8][shell_global_id u32 LE]。
    // a0=0/1 恒成对同值（双份记录/弹鼓双槽，槽位语义未定），a0=18 为非弹种数据（排除）。
    // shell@fire_time 与 type=32 segment 弹种逐发一致（4 回放 30/30），故在命中通知
    // 未被服务器转发时（含全部脱靶弹）以此兜底补全弹种。
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

    // ⑤''' Avatar method 0x1b 地形命中包（仅无坦克命中时广播）：shotId 配对。
    // args(34) 布局见 TerrainImpactData 文档；impactPoint 与 method20 终点一致。
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

    // ⑤'''' Avatar method36 (0x24) 瞄准快照时间线（envelope = avatar = 录像者本人）。
    // args = [len u8][protobuf]；开火时刻成对（前/后扩散度，f1 恒同）。
    // 布局不合法的包 fail-soft 跳过（不影响主链 fail-fast）。
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

    // ⑥ 游标
    let hp_events = parse_hp_events(packets);   // method1 血量事件（全实体，按时钟排序）
    // ⑥' 确定性伤害降幅区间（参考 WotbTools PlaybackCombatReconstruction.deriveLosses）：
    struct DmgLoss { victim: u32, t_prev: f32, t_cur: f32, dmg: u32, hp_cur: u16 }
    let mut dmg_losses: Vec<DmgLoss> = Vec::new();
    {
        let mut by_victim: std::collections::HashMap<u32, Vec<&HpEvent>> = std::collections::HashMap::new();
        for e in &hp_events { by_victim.entry(e.victim).or_default().push(e); }
        for (victim, evs) in &by_victim {
            let mut samples: Vec<(f32, u16, u32, u8)> = Vec::new();
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
                let (tp, hpp, _, _) = samples[w - 1];
                let (tc, hpc, srcc, causec) = samples[w];
                if hpc < hpp && srcc == author_player_eid && causec == 0 {
                    dmg_losses.push(DmgLoss { victim: *victim, t_prev: tp, t_cur: tc, dmg: (hpp - hpc) as u32, hp_cur: hpc });
                }
            }
        }
        dmg_losses.sort_by(|a, b| a.t_cur.partial_cmp(&b.t_cur).unwrap());
    }
    // 作者伤害计数器（type=7 sub=10）增量序列——首次命中（血量链无前值）兜底。
    // 含撞击/火伤等非弹伤害增量——与 method1 cause≠0 且涉及作者的事件同批
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

        let (sp, sa) = entity_state_at(packets, author_player_eid, fire_time)
            .ok_or_else(|| anyhow::anyhow!("{ctx}: 射手 type=10 状态快照缺失"))?;

        // 弹道终点（shotId 精确配对）；ball_a = method29 炮口发射位置
        let ball_a = l.point;
        let (end_time, ball_b) = endpoints.get(&shot_id).cloned()
            .ok_or_else(|| anyhow::anyhow!("{ctx}: method20 弹道终点缺失（shotId 无配对）"))?;

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
            anyhow::bail!("{ctx}: method38 配对歧义——命中窗口内出现 {} 条命中结果", cands);
        }
        if let Some(j) = matched {
            let hr = &hit_results[j];
            target_eid = Some(hr.victim);
            hit_flags = hr.flags;
            crit_modules = hr.crit_modules;
            destroyed_modules = hr.destroyed_modules;
            modifiers = hr.modifiers.clone();
            hit = true;
            hr_cursor = j + 1;
        }

        if let Some(teid) = target_eid {
            target_name = names.get(&teid)
                .ok_or_else(|| anyhow::anyhow!("{ctx}: 受击者实体 {teid} 不在 type=5 名册中"))?
                .clone();
        }

        // ⑧' 游戏原生命中段 + 结果枚举（wotinspector segment 对齐）：
        // type=32 警告包优先（含 segment u64，低字节=结果枚举）；
        // method8 直击通知兜底结果枚举；两者 hash6 命中令牌一致（86/86 实测）。
        // 歧义 fail-fast；服务器未转发时 segment=0 / result=255。
        let mut segment: u64 = 0;
        let mut shell_id: u32 = 0;
        let mut armor_group: u8 = 0;
        let mut hit_triangle: u16 = 0;
        let mut game_hit_result: u8 = 255;
        let mut hit_token: Option<String> = None;
        if let Some(teid) = target_eid {
            let mut seg_cands: Vec<&ArenaWarning32> = warnings32.iter()
                .filter(|w| w.eid == teid && (w.t - end_time).abs() <= 0.05)
                .collect();
            seg_cands.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());
            if !seg_cands.is_empty() {
                let first = seg_cands[0];
                if seg_cands.iter().any(|w| w.segment != first.segment) {
                    anyhow::bail!("{ctx}: type=32 segment 歧义——窗口内 {} 条互不一致的命中段", seg_cands.len());
                }
                segment = first.segment;
                game_hit_result = first.result;
                hit_token = Some(first.hash6.iter().map(|b| format!("{:02x}", b)).collect());
                // segment 布局解码：[result][tank+9][shell u16 LE][00][tri_hi][tri_lo][armor_group]
                let sb = segment.to_le_bytes();
                // 全局弹种 id = B1B2B3 u24 LE（=(局部 id<<8)|国家基数），与 WI shell_id 同值
                shell_id = (sb[1] as u32) | ((sb[2] as u32) << 8) | ((sb[3] as u32) << 16);
                armor_group = sb[7];
                hit_triangle = u16::from_be_bytes([sb[5], sb[6]]);
            } else {
                let mut r8: Vec<&DirectHit8> = direct_hits8.iter()
                    .filter(|d| d.shooter == author_player_eid && d.victim == teid && (d.t - end_time).abs() <= 0.05)
                    .collect();
                r8.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());
                if !r8.is_empty() {
                    let first = r8[0];
                    if r8.iter().any(|d| d.result != first.result) {
                        anyhow::bail!("{ctx}: method8 结果枚举歧义——窗口内 {} 条互不一致", r8.len());
                    }
                    game_hit_result = first.result;
                    hit_token = Some(first.hash6.iter().map(|b| format!("{:02x}", b)).collect());
                }
            }
        }

        // ⑧'' 弹种兜底：命中通知未被服务器转发（segment=0，含全部脱靶弹）时，
        // 用 method 0x07 弹种广播在发射时刻的最新值补全（与 segment 弹种 30/30 一致）。
        if shell_id == 0 {
            for (t_sel, sh) in &shell_broadcasts {
                if *t_sel <= fire_time { shell_id = *sh; } else { break; }
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
        if hit && hit_flags & (0x0010 | 0x1000) != 0 {
            let victim = target_eid.unwrap_or(0);
            let containing: Vec<&DmgLoss> = dmg_losses.iter()
                .filter(|l| l.victim == victim && l.t_prev < end_time && end_time <= l.t_cur + 1e-6)
                .collect();
            if containing.len() > 1 {
                anyhow::bail!("{ctx}: 伤害归属歧义（{} 个血量降幅区间包含命中时刻）", containing.len());
            }
            let dc_delta = dc_increments.get(dc_cursor).map(|x| x.1);
            if dc_delta.is_some() { dc_cursor += 1; }
            match containing.first() {
                Some(l) => { damage = l.dmg; }
                None => {
                    // ② 计数器亦无增量 = 服务器未记账 HP 伤害（模块-only 击穿等）→ 0
                    damage = dc_delta.unwrap_or(0);
                }
            }
            is_kill = containing.first().map(|l| l.hp_cur == 0).unwrap_or(false)
                || hit_flags & 0x0001 != 0;
        }

        // ⑨ 目标位置与姿态 @ 【命中时刻】（method20 终点 clock = 服务器精确命中时刻；
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
        let (aim_point_val, launch_point_rel) = if ball_b != [0.0; 3] && target_eid.is_some() {
            let rel = |p: [f32; 3]| [p[0] - tp[0], p[1] - tp[1], p[2] - tp[2]];
            (rel(ball_b), rel(ball_a))
        } else { ([0.0; 3], [0.0; 3]) };

        // ⑭ 受击坦克 type=10 多 tick 采样（命中 ±1s，位置相对命中锚点）。
        // 只保留命中 tick 及之前的采样：
        // 命中之后的数据包源会切换（AoI 远端基线 / 延迟缓冲），其后首个包的位置
        // 含数米级瞬移、朝向跳变——混入后 tick 切换会出现"横着滑移"
        // （位移方向 ⟂ 履带方向）与幽灵框朝向错乱（96 段实测 40 段反转）。
        // 窗口放宽到 +0.09 仅作锚点兜底：流里无 |dt|<0.05 的命中 tick 时，
        // 用最近的 +0.09 内包近似；有真锚点时丢弃其后样本。
        // 姿态/弹着点滤波只需命中前的连续车体，受击反馈用 hit_flags/segment 表达。
        let mut tick_samples: Vec<TickSample> = Vec::new();
        if hit {
            if let Some(victim) = target_eid {
                for (t2, clock, p) in packets {
                    if *t2 != 10 || p.len() < 48 { continue; }
                    if u32::from_le_bytes([p[0], p[1], p[2], p[3]]) != victim { continue; }
                    let dt = clock - end_time;
                    if dt < -1.0 || dt > 0.09 { continue; }
                    tick_samples.push(TickSample {
                        dt,
                        pos: [
                            f32::from_le_bytes([p[12], p[13], p[14], p[15]]) - tp[0],
                            f32::from_le_bytes([p[16], p[17], p[18], p[19]]) - tp[1],
                            f32::from_le_bytes([p[20], p[21], p[22], p[23]]) - tp[2],
                        ],
                        yaw: f32::from_le_bytes([p[36], p[37], p[38], p[39]]),
                        pitch: f32::from_le_bytes([p[40], p[41], p[42], p[43]]),
                        roll: f32::from_le_bytes([p[44], p[45], p[46], p[47]]),
                    });
                }
                // 有真锚点（|dt|<0.05）时丢弃其后样本（其必为切换后基线）
                if tick_samples.iter().any(|s| s.dt.abs() < 0.05) {
                    tick_samples.retain(|s| s.dt < 0.05);
                }
                tick_samples.sort_by(|a, b| a.dt.partial_cmp(&b.dt).unwrap());
                // 坏数据截断已撤回(用户决定):倒车等"疑似漂移"段是真实记录,
                // 完全按回放原始数据渲染。truncate_implausible_prefix 保留函数体
                // 仅供参考,不再调用。
            }
        }
        // ⑮ 射手坦克 type=10 采样（开火时刻 ±1.0s，世界系绝对坐标）
        // 与 ⑭ 同因：开火事件同样会触发数据包源切换（位置/朝向跳变），
        // 有真锚点（|dt|<0.05）时只保留锚点及其前采样，无锚点才放宽到 +0.09 兜底。
        // 前窗与受击方一致取 ±1.0s：受击方 tick 跨度 ±1.0s,射手窗过窄(±0.2s)时
        // 靠前 tick 会钳死在开火位置,呈现"射手不动"的假象(实际 8m/s 移动 0.8s)。
        let mut shooter_tick_samples: Vec<TickSample> = Vec::new();
        for (t2, clock, p) in packets {
            if *t2 != 10 || p.len() < 48 { continue; }
            if u32::from_le_bytes([p[0], p[1], p[2], p[3]]) != author_player_eid { continue; }
            let dt = clock - fire_time;
            if dt < -1.0 || dt > 0.09 { continue; }
            shooter_tick_samples.push(TickSample {
                dt,
                pos: [
                    f32::from_le_bytes([p[12], p[13], p[14], p[15]]),
                    f32::from_le_bytes([p[16], p[17], p[18], p[19]]),
                    f32::from_le_bytes([p[20], p[21], p[22], p[23]]),
                ],
                yaw: f32::from_le_bytes([p[36], p[37], p[38], p[39]]),
                pitch: f32::from_le_bytes([p[40], p[41], p[42], p[43]]),
                roll: f32::from_le_bytes([p[44], p[45], p[46], p[47]]),
            });
        }
        if shooter_tick_samples.iter().any(|s| s.dt.abs() < 0.05) {
            shooter_tick_samples.retain(|s| s.dt < 0.05);
        }
        shooter_tick_samples.sort_by(|a, b| a.dt.partial_cmp(&b.dt).unwrap());
        // 坏数据截断已撤回(同 ⑭):完全按回放原始数据渲染

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
            terrain_impact,
            shooter_aim,
        });
    }

    // ⑧' method38 = 作者自己的命中反馈——每条都必须配对到一次发射
    if hr_cursor < hit_results.len() {
        anyhow::bail!("存在未被任何发射配对的 method38 命中结果（{} 条未消费，自 t={:.2}s 起）——发射/命中配对不完整",
            hit_results.len() - hr_cursor, hit_results[hr_cursor].t);
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
