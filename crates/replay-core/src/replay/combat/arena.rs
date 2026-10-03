//! updateArena 流解析：subtype 过滤收集、PERIOD 阶段、wrapper6 击杀播报、
//! AoI 在场生命周期、0x0c 反馈计数、type=39 作者炮线帧。

use super::*;
use serde::{Deserialize, Serialize};

/// Avatar updateArena 的 methodID（method 流直方图 + 子类型 ID 分布双重实证）
pub const ARENA_UPDATE_METHOD: u32 = 48;

/// 子类型名（二进制名字表逐项导出；10=former_teamkills 为原表小写原名）
pub fn arena_subtype_name(id: u32) -> &'static str {
    match id {
        1 => "VEHICLE_LIST",
        2 => "VEHICLE_ADDED",
        3 => "PERIOD",
        4 => "STATISTICS",
        5 => "VEHICLE_STATISTICS",
        6 => "VEHICLE_KILLED",
        7 => "AVATAR_READY",
        8 => "BASE_POINTS",
        9 => "BASE_CAPTURED",
        10 => "former_teamkills",
        11 => "VEHICLE_UPDATED",
        12 => "STRATEGIC_POINT_STATUS",
        13 => "WIN_POINTS",
        14 => "PLAYER_NAME",
        15 => "RELOAD_TIME",
        16 => "OBSERVED_STATUS",
        17 => "RELOAD_TIME_LIST",
        18 => "GAME_MODE_DATA",
        19 => "VEHICLE_WAIT_RESPAWN",
        20 => "VEHICLE_RESURRECT",
        21 => "TEAM_RESPAWNS_LEFT",
        22 => "VAMPIRIC_CURSE",
        23 => "BATTLE_HINTS_INFO",
        24 => "BOSSMODE_INFO",
        25 => "TOTAL_GAME_MODE_INFO",
        26 => "TIER_EQUALIZER_DATA",
        27 => "UNKNOWN_TYPE",
        _ => "UNKNOWN",
    }
}

/// 一条 updateArena 更新（子类型 + 原始消息体；字段级解码按子类型另行解析）。
/// args 布局 = [subtype u8][len u8][protobuf]（len = 其后字节数）。
///
/// 载荷以原始字节存储（收集零转换，消费方 kill_feed/periods/comps 直接读字节）；
/// JSON 契约的 `payload_hex` hex 字符串字段在序列化时惰性编码。
#[derive(Debug, Clone)]
pub struct ArenaUpdate {
    pub clock: f32,
    pub subtype: u32,
    pub name: &'static str,
    /// protobuf 消息体（原始字节）
    pub payload: Vec<u8>,
}

impl Serialize for ArenaUpdate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("ArenaUpdate", 4)?;
        s.serialize_field("clock", &self.clock)?;
        s.serialize_field("subtype", &self.subtype)?;
        s.serialize_field("name", self.name)?;
        let hex: String = self.payload.iter().map(|b| format!("{:02x}", b)).collect();
        s.serialize_field("payload_hex", &hex)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for ArenaUpdate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            clock: f32,
            subtype: u32,
            payload_hex: String,
        }
        let r = Raw::deserialize(d)?;
        let payload = decode_hex(&r.payload_hex)
            .ok_or_else(|| serde::de::Error::custom("payload_hex 非法 hex"))?;
        Ok(ArenaUpdate {
            clock: r.clock,
            subtype: r.subtype,
            name: arena_subtype_name(r.subtype),
            payload,
        })
    }
}

/// 收集 updateArena 流（作者 Avatar 实体广播；类型=8 方法流，methodID=48）。
/// args 布局 = [subtype u8][len u8][protobuf]；len 不符的包按健壮路径仍从偏移 2 解析
pub fn collect_arena_updates(packets: &[(u32, f32, &[u8])]) -> Vec<ArenaUpdate> {
    collect_arena_updates_filtered(packets, |_| true)
}

/// [`collect_arena_updates`] 的 subtype 过滤版：高频子类型（RELOAD_TIME ~20B/条）在
/// 收集期即丢弃，不再"全量收集 → 消费方按 subtype 挑拣"。现有消费方只需 {1,3,6}
/// （comps/periods/kill_feed），无过滤的全量收集纯属浪费。
pub fn collect_arena_updates_filtered(
    packets: &[(u32, f32, &[u8])],
    keep_subtype: impl Fn(u32) -> bool,
) -> Vec<ArenaUpdate> {
    let mut out = Vec::new();
    for (_t, clock, p) in packets {
        if p.len() < 15 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != ARENA_UPDATE_METHOD {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if 12 + alen > p.len() || alen < 2 {
            continue;
        }
        let subtype = p[12] as u32;
        if !keep_subtype(subtype) {
            continue;
        }
        out.push(ArenaUpdate {
            clock: *clock,
            subtype,
            name: arena_subtype_name(subtype),
            payload: p[14..12 + alen].to_vec(),
        });
    }
    out
}

/// AoI 实体在场生命周期（WotbTools visibility-lifecycle PROVEN）：
/// Type33(预备)→~0.4s→Type5(物化)=进入观察集；Type4=离开（硬黑屏，485/485 隐藏段零更新）。
/// Type4 ≠ 死亡（503/503 敌方、485/485 同 eid 重入）；死亡面 = prop1/血量终态（death_events）。
/// 消费：OBSERVED（段内可精确播）/ LAST_KNOWN（Type4 前最后 type10，中位 0.101s）/ 隐藏段禁插值。
/// playback 的 coverage（采样间隙 >2s 断开）已承担渲染侧插值防护；本收集器提供协议精确边界
/// （0.094~2s 的短隐藏段 coverage 不断，协议面可收紧——P3/前端消费）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AoiPresence {
    pub eid: u32,
    /// 进入观察集（Type5 物化时刻）
    pub t_in: f32,
    /// 离开（Type4 时刻）；None = 战斗结束仍在场
    pub t_out: Option<f32>,
    /// 开段 Type5 物化快照的原始 HP u16（偏移 51，仅 entityTypeId == 2 战斗车辆且载荷足长；
    /// 否则缺省）。**每次重入都有**——与血量链 seed（`initial_hp`，仅首条 Type5）不同，
    /// 这是重入时刻的当前血量证据（隐藏期间的掉血在此兑现）。原样透传不解释：
    /// 0 / ≥0xFF00 哨兵族的语义由消费方按自己的口径分类（unknown ≠ 0）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hp_raw: Option<u16>,
}

/// Type5 物化包的战斗车辆 entityTypeId（payload[4..6) u16）
const ENTITY_TYPE_COMBAT_VEHICLE: u16 = 2;
/// Type5 战斗车辆物化快照的当前 HP 偏移（u16 LE；与 `collect_initial_hp` 同一偏移）
const MATERIALIZATION_HP_OFFSET: usize = 51;

/// Type5 物化快照的原始 HP（仅战斗车辆且载荷足长）
fn materialization_hp_raw(p: &[u8]) -> Option<u16> {
    if p.len() < MATERIALIZATION_HP_OFFSET + 2 {
        return None;
    }
    if u16::from_le_bytes([p[4], p[5]]) != ENTITY_TYPE_COMBAT_VEHICLE {
        return None;
    }
    Some(u16::from_le_bytes([
        p[MATERIALIZATION_HP_OFFSET],
        p[MATERIALIZATION_HP_OFFSET + 1],
    ]))
}

/// 收集 AoI 在场区段：Type33 与 Type5 一一配对（3,869:3,869，间隔 0.046~1.207s）取 Type5
/// 时刻为进入；Type4 关闭当前段。跨场段数 0..N（敌方重入常见，485/503 重入）。
pub fn collect_aoi_lifecycle(packets: &[(u32, f32, &[u8])]) -> Vec<AoiPresence> {
    // 文件序状态机：Type33 记 pending（按 eid，取首个）；Type5 消费 pending 开段；Type4 关段
    let mut pending33: std::collections::HashSet<u32> = Default::default();
    let mut open: std::collections::HashMap<u32, (f32, Option<u16>)> = Default::default();
    let mut out: Vec<AoiPresence> = Vec::new();
    for (ptype, clock, p) in packets {
        if p.len() < 4 {
            continue;
        } // Type17 等零长/短包无 eid 头（payloadLen==0 合法）
        let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        match *ptype {
            33 => {
                pending33.insert(eid);
            }
            5 => {
                if pending33.remove(&eid) && !open.contains_key(&eid) {
                    open.insert(eid, (*clock, materialization_hp_raw(p)));
                }
            }
            4 => {
                if let Some((t_in, hp_raw)) = open.remove(&eid) {
                    out.push(AoiPresence {
                        eid,
                        t_in,
                        t_out: Some(*clock),
                        hp_raw,
                    });
                }
                pending33.remove(&eid);
            }
            _ => {}
        }
    }
    for (eid, (t_in, hp_raw)) in open {
        out.push(AoiPresence {
            eid,
            t_in,
            t_out: None,
            hp_raw,
        });
    }
    out.sort_by(|a, b| a.eid.cmp(&b.eid).then(a.t_in.partial_cmp(&b.t_in).unwrap()));
    out
}

/// type=39 作者瞄准/炮线帧（len=28，**7×f32**；WotbTools PROVEN，本地 B 组交叉验证闭合）：
/// f0=世界系瞄准/炮线 yaw（度，开火锚定 0.27°）、f1=世界系 pitch（度，取负存储，0.45°）、
/// f2..4=世界系瞄准射线一点、f5=相对炮塔偏航族（PARTIAL——死亡/观战后失效，禁当炮塔角）、
/// f6=车体系炮管俯仰（rad，开火时刻 0.17°；仰角上限呈车型离散档）。
/// 门控：作者死亡/观战切换后 f0/f1 旋转、f5 冻结——消费须限作者存活期（death_events 门）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Type39Frame {
    pub clock: f32,
    /// f0 世界系炮线 yaw（rad，由度转换）
    pub gun_yaw: f32,
    /// f1 世界系炮线 pitch（rad，取负还原；正=仰角约定与项目一致需按 −f1）
    pub gun_pitch_world: f32,
    /// f2..4 世界系瞄准射线一点
    pub ray_point: [f32; 3],
    /// f5 相对偏航族（PARTIAL，原样透传）
    pub f5_rel_yaw: f32,
    /// f6 车体系炮管俯仰（rad）
    pub gun_pitch_local: f32,
}

/// method38 (0x26) resultFlags 位常量（WotbTools 全 16 位 PROVEN，11.19 China；样本复现）。
/// 高 16 位 = headerHi（多数 0x0002=录像者/直击关联位；Maus 批量边界见 0x0012/0x0028，
/// 保留 raw 禁当命中位解码）。位 0x0001/0x0004 潜在例外：撞击死（reason=2）与延迟火烧死不置 0x0001。
pub mod hit_flags_mod {
    pub const DIRECT_KILL: u32 = 0x0001;
    pub const TARGET_ALREADY_DEAD: u32 = 0x0002;
    pub const FIRE_STARTED: u32 = 0x0004;
    pub const RICOCHET: u32 = 0x0008;
    pub const MATERIAL_PENETRATION: u32 = 0x0010;
    pub const NON_PENETRATION: u32 = 0x0020;
    pub const SPACED_PIERCED: u32 = 0x0040;
    pub const SPACED_NOT_PIERCED: u32 = 0x0080;
    pub const DEVICE_PIERCED: u32 = 0x0100;
    pub const DEVICE_NOT_PIERCED: u32 = 0x0200;
    pub const TRACK_DAMAGED: u32 = 0x0400;
    pub const GUN_DAMAGED: u32 = 0x0800;
    pub const EXPLOSION_MATERIAL: u32 = 0x1000;
    pub const EXPLOSION_SPACED: u32 = 0x2000;
    pub const EXPLOSION_DEVICE_INVOLVED: u32 = 0x4000;
    pub const EXPLOSION_DEVICE_DAMAGED: u32 = 0x8000;
    /// 穿透族谓词（WotbTools PROVEN：对结算 penetrations 269/270，r≈0.9924；版本门控，勿命名官方掩码）
    pub const PENETRATION_FAMILY: u32 = MATERIAL_PENETRATION | DEVICE_PIERCED | EXPLOSION_MATERIAL;
}

/// 收集 type=39 帧流（作者 avatar 观战相机域；28B=7×f32）。
pub fn collect_type39_frames(packets: &[(u32, f32, &[u8])]) -> Vec<Type39Frame> {
    let mut out = Vec::new();
    for (_t, clock, p) in packets {
        if *_t != 39 || p.len() < 28 {
            continue;
        }
        let f = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
        out.push(Type39Frame {
            clock: *clock,
            gun_yaw: f(0).to_radians(),
            gun_pitch_world: -f(4).to_radians(),
            ray_point: [f(8), f(12), f(16)],
            f5_rel_yaw: f(20),
            gun_pitch_local: f(24),
        });
    }
    out
}

/// VEHICLE_KILLED（subtype 6，WotbTools wrapper6）击杀播报事件。
/// 字段语义（WotbTools PROVEN，283 例 post-start 闭合）：
/// field1=victim 实体、field2=killer 实体、field3=>50% 先前伤害助攻者（官方 >50% 通知规则，
/// 46/46 = 最高伤害非击杀源；阈值版本门控：8.1=51% / 9.3+=50%）、field4=可选非默认死亡原因
/// （1=火 2=撞 3=世界 5=溺水，缺省=普通击毁）。field5 稀疏未解，忽略。
/// 注意：开局初始化段也有 wrapper6 记录（非真实击杀），消费方须用时序门控。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillFeedEvent {
    pub clock: f32,
    pub victim_eid: u32,
    pub killer_eid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assister_eid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub death_reason: Option<u32>,
}

/// 收集击杀播报时间线（subtype 6；全实体广播经作者 Avatar）。
/// 载荷结构（本地 dump + WotbTools wrapper6→root field6 双证）：protobuf **root field6
/// (length-delimited)** 包裹击杀记录，内层 field1=victim / field2=killer / field3=助攻 /
/// field4=死因（varint）。
/// 开局初始化记录（period 3 之前）照样收录，消费方用 clock 门控（战斗开始锚点见 wrapper3）。
pub fn collect_kill_feed(packets: &[(u32, f32, &[u8])]) -> Vec<KillFeedEvent> {
    kill_feed_from_updates(&collect_arena_updates_filtered(packets, |s| s == 6))
}

/// [`collect_kill_feed`] 的共享扫描形态：从已收集的 subtype=6 update 流解析
/// （`ReplayModel::scan` 与 comps/periods 合用一次 arena pass 时复用）。
pub fn kill_feed_from_updates(updates: &[ArenaUpdate]) -> Vec<KillFeedEvent> {
    let mut out = Vec::new();
    for u in updates {
        if u.subtype != 6 {
            continue;
        }
        let bytes = &u.payload;
        let Some(record) = find_field(bytes, 6) else {
            continue;
        };
        let mut o = 0usize;
        let (mut victim, mut killer) = (0u32, 0u32);
        let (mut assister, mut reason) = (None, None);
        let mut ok = true;
        while o < record.len() {
            let Some(key) = pb_varint(record, &mut o) else {
                ok = false;
                break;
            };
            let (field, wt) = (key >> 3, key & 7);
            if wt == 0 {
                let Some(v) = pb_varint(record, &mut o) else {
                    ok = false;
                    break;
                };
                match field {
                    1 => victim = v as u32,
                    2 => killer = v as u32,
                    3 => assister = Some(v as u32),
                    4 => reason = Some(v as u32),
                    _ => {}
                }
            } else {
                let skip = match wt {
                    2 => pb_varint(record, &mut o).map(|l| l as usize),
                    1 => Some(8),
                    5 => Some(4),
                    _ => None,
                };
                match skip {
                    Some(n) if o + n <= record.len() => o += n,
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if ok && victim != 0 {
            out.push(KillFeedEvent {
                clock: u.clock,
                victim_eid: victim,
                killer_eid: killer,
                assister_eid: assister,
                death_reason: reason,
            });
        }
    }
    out
}

// ---------- 0x0c 战斗反馈计数（作者 Avatar method12；《回放与射击逆向总集》第一篇 §3.10【使用中】） ----------

/// 0x0c 事件码（baseType，WotbTools PROVEN）；未列出的码原样透传，不猜语义。
pub mod feedback_code {
    pub const DAMAGE_DEALT: u8 = 1;
    pub const SPOTTED: u8 = 2;
    pub const KILL: u8 = 3;
    pub const BLOCKED: u8 = 5;
    pub const DESTRUCTION_ASSIST: u8 = 15;
    pub const TOTAL_ASSIST: u8 = 17;
}

/// 一条战斗反馈计数事件：作者个人过程计数的带时标广播。
/// args 6B = [eventCode u16][count u16][value u16]（envelope = 作者 Avatar 实体）。
/// eventCode 为复合编码：低字节 = 事件基类型（1=累计伤害 2=点亮 3=击杀 5=挡伤
/// 15=毁灭协助 17=总助攻），高字节 = 同类型内序号（同类型多事件靠 seq 区分，
/// 不可整 u16 直判事件类型）。
/// count/value 的逐项口径以 facets 结算互验为准，对不上保持原样透传，不猜语义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackCounterEvent {
    pub clock: f32,
    /// 作者 Avatar 实体 id（envelope）
    pub avatar_eid: u32,
    /// 事件基类型 = 原始 code 低字节
    pub event_code: u8,
    /// 同类型内序号 = 原始 code 高字节
    pub seq: u8,
    pub count: u16,
    pub value: u16,
}

/// 收集战斗反馈计数流（作者 Avatar 专属；队友无对应广播，点亮归因只有结算总量）。
/// 解析健壮性：args <6B 或截断的包跳过（帧错误不猜）；事件按包序即时钟序（作者流单调）。
pub fn collect_feedback_counters(packets: &[(u32, f32, &[u8])]) -> Vec<FeedbackCounterEvent> {
    let mut out = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 12 + 6 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x0C {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if alen < 6 || 12 + alen > p.len() {
            continue;
        }
        let a = &p[12..12 + alen];
        let raw_code = u16::from_le_bytes([a[0], a[1]]);
        out.push(FeedbackCounterEvent {
            clock: *clock,
            avatar_eid: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            event_code: (raw_code & 0xFF) as u8,
            seq: (raw_code >> 8) as u8,
            count: u16::from_le_bytes([a[2], a[3]]),
            value: u16::from_le_bytes([a[4], a[5]]),
        });
    }
    out
}

/// PERIOD (subtype=3) 解析结果：战局阶段时间线
/// 消息体 = protobuf field3 嵌套 { field1 varint: period, field2 fixed64: 阶段剩余秒, field3 varint: 阶段时长 }
/// 实测 J39：period 1=准备 → 2=倒计时 → 3=战斗；剩余秒与包时刻互洽
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
        if u.subtype != 3 {
            continue;
        }
        let b = &u.payload[..];
        // 顶层 field3 (tag 0x1a) 长度前缀嵌套
        let nested = match find_field(b, 3) {
            Some(n) => n,
            None => continue,
        };
        let period = read_varint(nested, 1);
        let remaining = read_fixed64(nested, 2);
        let duration = read_varint(nested, 3);
        if let (Some(period), Some(remaining)) = (period, remaining) {
            out.push(ArenaPeriod {
                clock: u.clock,
                period,
                remaining_s: remaining,
                duration_s: duration.unwrap_or(0),
            });
        }
    }
    out
}

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
            payload: decode_hex("1a0e0803110000000000407a4018a403").unwrap(),
        };
        let periods = parse_arena_periods(&[u]);
        assert_eq!(periods.len(), 1);
        let p = &periods[0];
        assert_eq!(p.period, 3);
        assert!(
            (p.remaining_s - 420.0).abs() < 1e-9,
            "remaining={}",
            p.remaining_s
        );
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

    /// 0x0c 反馈计数：合成包帧 [eid][mid=0x0C][alen=6][code u16][count u16][value u16]，
    /// 帧/字段解码 + 非 0x0c 方法不误收 + args 截断包跳过（帧错误不猜）。
    #[test]
    fn feedback_counter_decode() {
        let mk = |mid: u32, args: &[u8]| {
            let mut p = Vec::new();
            p.extend_from_slice(&0x5Au32.to_le_bytes()); // eid
            p.extend_from_slice(&mid.to_le_bytes());
            p.extend_from_slice(&(args.len() as u32).to_le_bytes());
            p.extend_from_slice(args);
            p
        };
        let ok = mk(0x0C, &[2, 0, 5, 0, 3, 0]);
        let other = mk(0x01, &[1, 0, 2, 0, 3, 0, 0]);
        let truncated = mk(0x0C, &[2, 0, 5]);
        let packets: Vec<(u32, f32, &[u8])> =
            vec![(8, 10.0, &ok), (8, 11.0, &other), (8, 12.0, &truncated)];
        let ev = collect_feedback_counters(&packets);
        assert_eq!(ev.len(), 1, "只应收录合法 0x0c 包");
        assert_eq!(ev[0].avatar_eid, 0x5A);
        assert_eq!(ev[0].event_code, feedback_code::SPOTTED);
        assert_eq!(ev[0].count, 5);
        assert_eq!(ev[0].value, 3);
        assert!((ev[0].clock - 10.0).abs() < 1e-6);
    }
}

// ---------- Supremacy（争霸）目标状态：subtype48 wrapper12/root11（WotbTools PROVEN 移植） ----------
//
// 来源与 provenance：WotbTools `EntityMethodDecoder.parseRawSupremacyBaseUpdates` +
// `SupremacyBaseStateReconstructor`（docs/research/replay/supremacy-base-state.md）。
// 数据链：Type 8 EntityMethod → subtype 48（updateArena2）→ wrapper field 12 → root field 11
// → repeated base 块；嵌套字段 field1=base index(0..3=A..D)、field2=owner team、field3=capturing
// team、field4=capture progress、field5/6=UNKNOWN 原样透传（禁命名）。
// 语义红线：wire 块是 SPARSE UPDATE——absent 字段=维持前值；显式 0=清空；不推断、不外推。

pub const WRAPPER_SUPREMACY_BASE: u32 = 12;
/// 实时点数广播（WotbTools PROVEN；仅二次校验用，
/// 不得由点数反推基地归属）
pub const WRAPPER_SUPREMACY_POINTS: u32 = 13;

/// subtype48 updateArena2 解包（Java `decodeUpdateArena2` 同构，含 0xFF 逃逸的
/// bug-compatible 布局：`0xFF + u16le + 1 pad`，proto 起点在 off+4）。返回 (wrapper, root)。
fn decode_update_arena2(args: &[u8]) -> Option<(u32, &[u8])> {
    let mut off = 0usize;
    let wrapper = pb_varint(args, &mut off)? as u32;
    if off >= args.len() {
        return None;
    }
    let first = args[off] as usize;
    let (msg_len, proto_off) = if first == 0xFF {
        if off + 4 > args.len() {
            return None;
        }
        (
            u16::from_le_bytes([args[off + 1], args[off + 2]]) as usize,
            off + 4,
        )
    } else {
        (first, off + 1)
    };
    if proto_off + msg_len != args.len() {
        return None;
    }
    Some((wrapper, &args[proto_off..proto_off + msg_len]))
}

/// wrapper12/root11 sparse 原始更新（字段缺省 = None；语义与 WotbTools Java 同名 PROVEN）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSupremacyBaseUpdate {
    pub clock: f32,
    /// field1 = base index（0..3 = A..D）；wire 缺省在 canonical 边界按 wire default 0（=A），
    /// 仅此一处补缺省（Java 同式注释），其余字段 absent 一律 None
    pub base_index: Option<u8>,
    /// field2 = owner team（显式 0 = 清空归属）
    pub owner_team: Option<u8>,
    /// field3 = capturing team（显式 0 = 清空占领方，连带清 progress）
    pub capturing_team: Option<u8>,
    /// field4 = capture progress（0..99）
    pub capture_progress: Option<u8>,
    /// field5 = UNKNOWN（原样透传，禁命名）
    pub raw_field5: Option<u64>,
    /// field6 = UNKNOWN（原样透传，禁命名）
    pub raw_field6: Option<u64>,
}

impl RawSupremacyBaseUpdate {
    /// 全缺省行（除 base_index 外**没有任何**字段）= 显式清空该基地。
    ///
    /// 服务端按 proto3 语义**省略零值字段**：整个基地状态回到全零（无主/无占领方/进度 0）
    /// 时，wire 上就是一条只带 base_index 的块。该形态只出现在两类时刻——开局的状态
    /// 广播，与**占领中断**（占领车辆出圈/被击毁，进度作废）；进度进行中从不出现。
    /// 详见 `reconstruct_supremacy_base_states` 的注释与契约文档。
    ///
    /// 未知字段（f5/f6）带值时**不**判为清空：其语义未证实，缺省语义 = "维持前值"
    /// （fail-closed，不拿未证实证据改状态）。
    fn is_blank(&self) -> bool {
        self.owner_team.is_none()
            && self.capturing_team.is_none()
            && self.capture_progress.is_none()
            && self.raw_field5.is_none()
            && self.raw_field6.is_none()
    }
}

/// 重建后的 canonical 基地状态迁移：每条 raw 更新一条，携带该基地更新后的完整状态。
/// 消费（seek 语义）= 取 ≤t 的每基地最后一条逐字段折叠。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupremacyBaseStateTransition {
    pub clock: f32,
    /// 0..3 = A..D
    pub base_id: u8,
    /// None = 无主
    pub owner_team: Option<u8>,
    pub capturing_team: Option<u8>,
    pub capture_progress: Option<u8>,
}

/// 收集 wrapper12/root11 sparse 基地更新。校验（Java 同式）：base_index 0..=3、
/// owner/capturing ∈ {0,1,2}、progress 0..=99；不合法块整体跳过，绝不产出部分状态。
pub fn collect_supremacy_base_updates(
    packets: &[(u32, f32, &[u8])],
) -> Vec<RawSupremacyBaseUpdate> {
    let mut out = Vec::new();
    for (_t, clock, p) in packets {
        if p.len() < 15 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != ARENA_UPDATE_METHOD {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if 12 + alen > p.len() || alen < 2 {
            continue;
        }
        let Some((wrapper, root)) = decode_update_arena2(&p[12..12 + alen]) else {
            continue;
        };
        if wrapper != WRAPPER_SUPREMACY_BASE {
            continue;
        }
        let Some(fields) = proto_fields(root) else {
            continue;
        };
        for (f, wire, s, len) in fields {
            if f != 11 || wire != 2 {
                continue;
            }
            let block = &root[s..s + len];
            let Some(bf) = proto_fields(block) else {
                continue;
            };
            let mut u = RawSupremacyBaseUpdate {
                clock: *clock,
                base_index: None,
                owner_team: None,
                capturing_team: None,
                capture_progress: None,
                raw_field5: None,
                raw_field6: None,
            };
            for (n, w, vs, _vl) in bf {
                if w != 0 {
                    continue;
                }
                let Some(mut vo) = (vs <= block.len()).then_some(vs) else {
                    continue;
                };
                let Some(v) = pb_varint(block, &mut vo) else {
                    continue;
                };
                match n {
                    1 => u.base_index = Some(v as u8),
                    2 => u.owner_team = Some(v as u8),
                    3 => u.capturing_team = Some(v as u8),
                    4 => u.capture_progress = Some(v as u8),
                    5 => u.raw_field5 = Some(v),
                    6 => u.raw_field6 = Some(v),
                    _ => {}
                }
            }
            let valid_base = u.base_index.is_none_or(|b| b <= 3);
            let valid_team = |t: Option<u8>| t.is_none_or(|x| x <= 2);
            let valid_progress = u.capture_progress.is_none_or(|x| x <= 99);
            if !valid_base
                || !valid_team(u.owner_team)
                || !valid_team(u.capturing_team)
                || !valid_progress
            {
                continue;
            }
            out.push(u);
        }
    }
    out
}

/// sparse 更新 → canonical 状态时间线（Java `SupremacyBaseStateReconstructor` 逐行移植）：
/// absent = 维持前值；显式 0 = 清空（owner/capturing）；显式 capturing 清空连带清 progress；
/// 占领中 owner 变更 = 完成/作废该次占领（capturing 与 progress 一并清空）。
/// **全缺省行 = 清空整个基地状态**（契约见下）。
/// 排序 = clock 升序稳定排序（同 clock 保包序 = Java 的 sequence 序，同源包流）。
///
/// **全缺省行为何是"清空"**：服务端按 proto3 省略零值字段，故"占领中断（车辆出圈/被击毁，
/// 进度作废）"落在 wire 上是一条只带 base_index、其余字段全缺省的块——若把"字段缺省"
/// 一律当"维持前值"，该块即空操作，旧进度与旧占领方**永久**挂在基地上。
/// 带未知字段（f5/f6）的块不按清空处理，见
/// [`RawSupremacyBaseUpdate::is_blank`]。
pub fn reconstruct_supremacy_base_states(
    mut raw: Vec<RawSupremacyBaseUpdate>,
) -> Vec<SupremacyBaseStateTransition> {
    raw.sort_by(|a, b| {
        a.clock
            .partial_cmp(&b.clock)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    #[derive(Clone, Copy, Default)]
    struct State {
        owner: Option<u8>,
        capturing: Option<u8>,
        progress: Option<u8>,
    }
    let mut states = [State::default(); 4];
    let mut out = Vec::with_capacity(raw.len());
    for u in raw {
        // canonical 边界唯一补缺省处：absent field1 → wire default 0 = A（Java 同式）
        let idx = u.base_index.unwrap_or(0) as usize;
        if idx >= 4 {
            continue;
        }
        let st = &mut states[idx];
        if u.is_blank() {
            // 全零状态的整体广播：三个字段一起回到零值语义，不保留任何前值
            st.owner = None;
            st.capturing = None;
            st.progress = None;
        } else {
            let prev_owner = st.owner;
            let prev_capturing = st.capturing;
            if let Some(o) = u.owner_team {
                st.owner = if o == 0 { None } else { Some(o) };
            }
            if let Some(c) = u.capturing_team {
                st.capturing = if c == 0 { None } else { Some(c) };
            }
            if let Some(p) = u.capture_progress {
                st.progress = Some(p);
            }
            if u.capturing_team.is_some() && st.capturing.is_none() {
                st.progress = None;
            }
            if u.owner_team.is_some() && prev_capturing.is_some() && st.owner != prev_owner {
                st.capturing = None;
                st.progress = None;
            }
        }
        out.push(SupremacyBaseStateTransition {
            clock: u.clock,
            base_id: idx as u8,
            owner_team: st.owner,
            capturing_team: st.capturing,
            capture_progress: st.progress,
        });
    }
    out
}

// ---------- 攻防战单基地实时状态：subtype48 wrapper8/root8（WotbTools PROVEN 移植） ----------
//
// 来源与 provenance：WotbTools `docs/research/replay/assault-base-state.md` +
// `AssaultBaseStateReconstructor`。
// **Assault 不复用 Supremacy 的 wrapper12**——两种模式的承载天然互斥。
//
// 数据链：Type 8 EntityMethod → subtype 48（updateArena2）→ wrapper field 8 → root field 8
// → repeated 单基地更新；嵌套 field1/field2 为**原始判别子（语义 UNKNOWN）**、
// **field3 = 占领进度（PROVEN 0..100）**、field4 属另一族。
// 语义红线：只对 `field1==2 && field2==1` 族的 field3 赋语义；其余字段原样保留、不命名。

pub const WRAPPER_ASSAULT_BASE: u32 = 8;

/// wrapper8/root8 单基地原始更新（字段全保留 raw；判别子不做命名）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawAssaultBaseUpdate {
    pub clock: f32,
    /// nested field1（raw 判别子，语义 UNKNOWN）
    pub raw_field1: Option<u64>,
    /// nested field2（raw 单目标索引，语义 UNKNOWN）
    pub raw_field2: Option<u64>,
    /// nested field3（该族 = 占领进度 0..100，PROVEN）
    pub raw_field3: Option<u64>,
    /// nested field4（另一族字段，保持 raw）
    pub raw_field4: Option<u64>,
}

/// 攻防战占领进度迁移（单基地；只含 PROVEN 的 progress，不赋 owner/team 语义）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssaultBaseStateTransition {
    pub clock: f32,
    /// 占领进度 0..=100
    pub progress: u8,
}

/// 收集 wrapper8/root8 原始更新（不做族过滤；族判定见重建器）。
pub fn collect_assault_base_updates(packets: &[(u32, f32, &[u8])]) -> Vec<RawAssaultBaseUpdate> {
    let mut out = Vec::new();
    for (_t, clock, p) in packets {
        if p.len() < 15 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != ARENA_UPDATE_METHOD {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if 12 + alen > p.len() || alen < 2 {
            continue;
        }
        let Some((wrapper, root)) = decode_update_arena2(&p[12..12 + alen]) else {
            continue;
        };
        if wrapper != WRAPPER_ASSAULT_BASE {
            continue;
        }
        let Some(fields) = proto_fields(root) else {
            continue;
        };
        for (f, wire, st, len) in fields {
            if f != 8 || wire != 2 {
                continue;
            }
            let block = &root[st..st + len];
            let Some(bf) = proto_fields(block) else {
                continue;
            };
            let mut u = RawAssaultBaseUpdate {
                clock: *clock,
                raw_field1: None,
                raw_field2: None,
                raw_field3: None,
                raw_field4: None,
            };
            for (n, w, vs, _vl) in bf {
                if w != 0 {
                    continue;
                }
                let Some(mut vo) = (vs <= block.len()).then_some(vs) else {
                    continue;
                };
                let Some(v) = pb_varint(block, &mut vo) else {
                    continue;
                };
                match n {
                    1 => u.raw_field1 = Some(v),
                    2 => u.raw_field2 = Some(v),
                    3 => u.raw_field3 = Some(v),
                    4 => u.raw_field4 = Some(v),
                    _ => {}
                }
            }
            out.push(u);
        }
    }
    out
}

/// 原始更新 → 占领进度时间线：取 `field2==1` 的条目，按 clock 升序（不施加单调性——
/// 回落/重置原样保留）。无该族则返回空。三类块：
/// - 携带 `field3`（∈ 0..=100）→ 该时刻进度；
/// - 越界 `field3`（>100）→ 剔除；
/// - `field3`/`field4` **双缺省** → 进度归零（见下）；
/// - 仅携带 `field4`（标志流，占领进行中与进度块同包成对）→ 非进度样本，跳过。
///
/// **双缺省块 = 进度归零。** 服务端按 proto3 省略零值字段，"占领中断（车辆出圈/被击毁，
/// 进度作废）"落在 wire 上是一对只带 field1/field2 的块；进度进行中从不出现该形态。
/// 若要求 `field3` 存在会把中断块整块丢弃，时间线停在最后一个正值（进度条永久卡住）。
///
/// **只在"确有进度被清掉"时产出归零行**（上一条已产出行进度 > 0）：普通对局也发的裸
/// 初始化对同样是双缺省块，若一律合成 0 事件，会让 `assault_bases` 恒非空 → 存在性
/// 回退判据误判成"有目标"（见 [`has_assault_objective`]）。
/// 同一时刻的重复块由该条件天然去重（清空后进度已为 0）。
///
/// **`field1` 不做族过滤。** 携带 `field3` 的族会在 `field1=1`/`field1=2` 之间切换
/// （按模式不同可能仅 1、仅 2 或交替）——以 `field1==2` 为进度族判别子会整段丢事件。
/// `field1` 是"哪一方的进度"（owner/占领方；精确语义仍未闭合，保持 raw 不命名），
/// 不是"是否进度族"。同一时刻恰有一族携带 `field3`、另一族携带常量 `field4=1`。
/// 遭遇战（Encounter）与攻防战共用该载体：遭遇战无 wrapper12，进度同样走 wrapper8。
pub fn reconstruct_assault_base_states(
    mut raw: Vec<RawAssaultBaseUpdate>,
) -> Vec<AssaultBaseStateTransition> {
    raw.retain(|u| matches!(u.raw_field1, Some(1) | Some(2)) && u.raw_field2 == Some(1));
    raw.sort_by(|a, b| {
        a.clock
            .partial_cmp(&b.clock)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out: Vec<AssaultBaseStateTransition> = Vec::with_capacity(raw.len());
    for u in raw {
        match u.raw_field3 {
            // 进度样本：0..=100 入选；越界（>100）剔除
            Some(v) if v <= 100 => out.push(AssaultBaseStateTransition {
                clock: u.clock,
                progress: v as u8,
            }),
            Some(_) => {}
            // 双缺省块 = 归零；只在"确有进度被清掉"时产出行（见上文）
            None if u.raw_field4.is_none() && out.last().is_some_and(|s| s.progress > 0) => {
                out.push(AssaultBaseStateTransition {
                    clock: u.clock,
                    progress: 0,
                });
            }
            // 其余：仅 field4 标志流（非进度样本），或无可清之物的双缺省块
            None => {}
        }
    }
    out
}

/// 单基地目标存在性（与"是否已有占领进度"无关）：目标族（`field2==1`、
/// `field1 ∈ {1,2}`）在回放里出现过即真——**不要求有进度**，裸初始化对（双方各一条
/// 目标记录、无其它字段）也算目标存在。用途：让"攻防战/遭遇战但全程无人占领"的
/// 场次仍能画出目标圈。与争霸互斥由调用侧保证。
///
/// 代价与回归口子：若将来证明确有**无目标**的场次也发这一对，phantom 圈会出现在
/// 那些场次上——届时应改回"需超出初始化对的证据"（见
/// `docs/replay-contract-v2-supremacy-type39.md`）。
///
/// 与 WotbTools Java `hasObjective`（`field1==2 && field2==1`）的差异：本判据覆盖两族
/// （`field1 ∈ {1,2}`），因为携带目标记录的族会在 1/2 之间切换。
pub fn has_assault_objective(raw: &[RawAssaultBaseUpdate]) -> bool {
    raw.iter()
        .any(|u| matches!(u.raw_field1, Some(1) | Some(2)) && u.raw_field2 == Some(1))
}

// ---------- 实时装填相位（subtype 15 RELOAD_TIME / 17 RELOAD_TIME_LIST）----------
//
// 线格式：args = [subtype u8][len u8][protobuf]；protobuf 里 wrapper field14（sub15）/ field16（sub17）
// → repeated field1 → 条目 { f1=eid varint, f2=相位码 varint, f3=fixed32 f32 秒, f4=计数 varint }。
//
// 语义边界（只消费已验证者）：f2=3 装填开始（f3 = 本次相位时长）、f2=4 装填中途时长变更
// （肾上腺素/弹药架）、f2=7 弹夹/弹鼓内单发间隔（f3 = 该发时长）、f4=1 = 枪管就绪。
// f2 其余取值、f4 其余计数**语义未闭环**（研究只闭环 f4=1）→ 原样透传、不赋语义。
//
// 覆盖范围（协议广播范围，非实现缺口）：**仅本方全队**，敌方无该流。
// 推送是**相位转移驱动**（非固定采样）→ 天然适合驱动进度条。
pub const ARENA_SUB_RELOAD_TIME: u32 = 15;
/// 装填**时长更新**（= 引擎里的 `ReloadTimeUpdate`；与装填相位流无关）
pub const ARENA_SUB_RELOAD_TIME_UPDATE: u32 = 16;
pub const ARENA_SUB_RELOAD_TIME_LIST: u32 = 17;

/// 权威"当前生效完整装填时长"（方法 0x23/35 的实体字段流；载荷 = [.. ][eid u32][f32 秒]）。
/// 语义 = method35 float1 当前生效完整装填配置时长（肾上腺素/弹药架/装填手联动，非倒计时），
/// 可用于校准/替换相位推断。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawReloadDuration {
    pub clock: f32,
    pub eid: u32,
    pub duration_s: f32,
}

/// 从原始包流收集"有效装填时长"（方法 0x23 = 35）。
pub fn reload_durations_from_packets(packets: &[(u32, f32, &[u8])]) -> Vec<RawReloadDuration> {
    const METHOD_RELOAD_TIME_FIELD: u32 = 0x23;
    let mut out = Vec::new();
    for (_t, clock, p) in packets {
        if p.len() < 20 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != METHOD_RELOAD_TIME_FIELD {
            continue;
        }
        let eid = u32::from_le_bytes([p[12], p[13], p[14], p[15]]);
        let dur = f32::from_le_bytes([p[16], p[17], p[18], p[19]]);
        if !dur.is_finite() || dur <= 0.0 || dur > 600.0 {
            continue;
        }
        out.push(RawReloadDuration {
            clock: *clock,
            eid,
            duration_s: dur,
        });
    }
    out.sort_by(|a, b| {
        a.clock
            .partial_cmp(&b.clock)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}
/// 装填相位码（f2）全表（真值来源：真实回放 + 客户端 item_defs/UI 交叉验证）。
///
/// | f2 | 语义 | f3 | f4 |
/// |----|------|----|----|
/// | 1 | 剩余弹数更新（与同车开火同刻） | 无 | 剩余发数 |
/// | 3 | 整夹重装开始 | 整夹时长 | 剩余发数快照（0） |
/// | 4 | 中途时长变更（肾上腺素/弹药架）——**f3 = 新的完整有效时长**，不是倒计时 | 新完整时长 | 剩余发数快照 |
/// | 5 | 就绪 / 取消 | 无 | **1 = 就绪标志，不是剩余发数** |
/// | 6 | 弹鼓逐发补槽（真装填一发） | 该槽位时长 | 剩余发数快照 |
/// | 7 | 夹内推弹上膛（**不补弹**，只是下一发进膛的间隔） | 该间隔时长 | 剩余发数快照 |
/// | 8 | **语义未定**（禁猜）：仅见 tank 21793「Sheridan Missile」（单发炮，`burst_size=0`），
///     无 f3/f4，紧随其后出现该车 f2=3 整炮重装 | 无 | 无 |
///
/// **除 f2=5 外，f4 = 该事件时刻的服务器剩余弹数快照**（与客户端
/// item_defs `<clip><count>`、BlitzKit `burst_size` 三方一致）。**f2=2 未观测**；8 见上
/// （渲染侧不解释）。subtype 16 是引擎 `ReloadTimeUpdate`（与装填完成/开火零相关），
/// 原样透传、不赋语义。
///
/// f2=**剩余弹数更新**（无时长；f4 = 弹夹/弹鼓剩余发数，与同车开火同刻）
pub const RELOAD_PHASE_AMMO_COUNT: u8 = 1;
/// f2=装填开始（f3 = 本次相位时长）
pub const RELOAD_PHASE_START: u8 = 3;
/// f2=装填中途时长变更（肾上腺素/弹药架）
pub const RELOAD_PHASE_DURATION_CHANGE: u8 = 4;
/// f2=弹夹/弹鼓内单发装填间隔（f3 = 该发时长）
pub const RELOAD_PHASE_MAG_INTERVAL: u8 = 7;
/// f4=1 = 枪管就绪（唯一已闭环的计数取值）
pub const RELOAD_READY_COUNT: u64 = 1;

/// 单条装填相位（**原样透传**：未闭环的相位码/计数不赋语义）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawReloadPhase {
    pub clock: f32,
    pub eid: u32,
    /// 原始相位码 f2（语义表见上方常量块：1/3/4/5/6/7 已闭环）
    pub phase: u8,
    /// 本次相位时长（秒）——f3 缺省为 None；**不是倒计时**（f2=4 时是新的完整时长）
    pub duration_s: Option<f32>,
    /// 原始计数 f4（原样透传）；**除 f2=5 外 = 服务器剩余弹数快照**，f2=5 时 1 = 就绪标志
    pub count: Option<u64>,
}

/// 从已收集的 arena update 流（keep_subtype 需含 15/17）解析装填相位条目，按 clock 升序。
///
/// 消费方（前端进度条）口径：每车取 ≤ t 的最后一条**已闭环**条目——f2=3/7 带时长者 =
/// 正在装填（`progress = clamp((t − clock) / duration_s)`），f4=1 = 就绪（条到 1 后停住）。
pub fn reload_phases_from_updates(updates: &[ArenaUpdate]) -> Vec<RawReloadPhase> {
    let mut out = Vec::new();
    for u in updates {
        if u.subtype != ARENA_SUB_RELOAD_TIME
            && u.subtype != ARENA_SUB_RELOAD_TIME_UPDATE
            && u.subtype != ARENA_SUB_RELOAD_TIME_LIST
        {
            continue;
        }
        // 包装层字段号 = subtype − 1（sub15→field14、sub17→field16；sub16→field15 由同族推得）。
        // 与 subtype 同值的 updateArena2 族不同。
        let wrap_no: u32 = u.subtype - 1;
        let Some(fields) = proto_fields(&u.payload) else {
            continue;
        };
        for (f, wire, st, len) in fields {
            if f != wrap_no || wire != 2 {
                continue;
            }
            let Some(end) = st.checked_add(len).filter(|e| *e <= u.payload.len()) else {
                continue;
            };
            let wrap = &u.payload[st..end];
            let Some(blocks) = proto_fields(wrap) else {
                continue;
            };
            for (bf, bwire, bst, blen) in blocks {
                if bf != 1 || bwire != 2 {
                    continue;
                }
                let Some(bend) = bst.checked_add(blen).filter(|e| *e <= wrap.len()) else {
                    continue;
                };
                let sub = &wrap[bst..bend];
                let Some(ef) = proto_fields(sub) else {
                    continue;
                };
                let (mut eid, mut phase, mut dur, mut count) = (None, None, None, None);
                for (n, w, vs, _vl) in ef {
                    match (n, w) {
                        (1, 0) => {
                            let mut o = vs;
                            eid = pb_varint(sub, &mut o).map(|v| v as u32);
                        }
                        (2, 0) => {
                            let mut o = vs;
                            phase = pb_varint(sub, &mut o).map(|v| v as u8);
                        }
                        (3, 5) => {
                            if vs + 4 <= sub.len() {
                                dur = Some(f32::from_le_bytes([
                                    sub[vs],
                                    sub[vs + 1],
                                    sub[vs + 2],
                                    sub[vs + 3],
                                ]));
                            }
                        }
                        (4, 0) => {
                            let mut o = vs;
                            count = pb_varint(sub, &mut o);
                        }
                        _ => {}
                    }
                }
                let (Some(eid), Some(phase)) = (eid, phase) else {
                    continue;
                };
                out.push(RawReloadPhase {
                    clock: u.clock,
                    eid,
                    phase,
                    duration_s: dur,
                    count,
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.clock
            .partial_cmp(&b.clock)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// 实时点数采样（wrapper13/root12 块：field1=team(1/2)、field2=points）。门禁与 Java
/// 同式：wrapperFieldNumber != 13 时即使 root 结构相同也绝不产出点数事件；
/// 只消费回放真实广播，绝不按游戏规则推算。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupremacyPointsSample {
    pub clock: f32,
    pub team: u8,
    pub points: u32,
}

pub fn collect_supremacy_points(packets: &[(u32, f32, &[u8])]) -> Vec<SupremacyPointsSample> {
    let mut out = Vec::new();
    for (_t, clock, p) in packets {
        if p.len() < 15 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != ARENA_UPDATE_METHOD {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if 12 + alen > p.len() || alen < 2 {
            continue;
        }
        let Some((wrapper, root)) = decode_update_arena2(&p[12..12 + alen]) else {
            continue;
        };
        if wrapper != WRAPPER_SUPREMACY_POINTS {
            continue;
        }
        let Some(fields) = proto_fields(root) else {
            continue;
        };
        for (f, wire, s, len) in fields {
            if f != 12 || wire != 2 {
                continue;
            }
            let block = &root[s..s + len];
            let Some(bf) = proto_fields(block) else {
                continue;
            };
            let mut team = None;
            let mut points = None;
            for (n, w, vs, _vl) in bf {
                if w != 0 {
                    continue;
                }
                let Some(mut vo) = (vs <= block.len()).then_some(vs) else {
                    continue;
                };
                let Some(v) = pb_varint(block, &mut vo) else {
                    continue;
                };
                match n {
                    1 => team = Some(v as u8),
                    2 => points = Some(v as u32),
                    _ => {}
                }
            }
            let (Some(team), Some(points)) = (team, points) else {
                continue;
            };
            if team != 1 && team != 2 {
                continue;
            }
            if points > 100_000 {
                continue;
            }
            out.push(SupremacyPointsSample {
                clock: *clock,
                team,
                points,
            });
        }
    }
    out
}

#[cfg(test)]
mod supremacy_tests {
    use super::*;

    fn varint(v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        let mut v = v;
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out
    }

    /// 构造 subtype48 包：args = varint(wrapper) + msgLen + root
    fn mk48(wrapper: u32, root: &[u8]) -> Vec<u8> {
        let mut args = varint(wrapper as u64);
        assert!(root.len() < 0xFF, "测试用短形长度即可");
        args.push(root.len() as u8);
        args.extend_from_slice(root);
        let mut p = vec![0u8; 12];
        p[4..8].copy_from_slice(&48u32.to_le_bytes());
        p[8..12].copy_from_slice(&(args.len() as u32).to_le_bytes());
        p.extend_from_slice(&args);
        p
    }

    fn varint_block(fields: &[(u32, u64)]) -> Vec<u8> {
        let mut b = Vec::new();
        for (n, v) in fields {
            b.extend_from_slice(&varint((n << 3) as u64));
            b.extend_from_slice(&varint(*v));
        }
        b
    }

    fn root_blocks(field: u32, blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut root = Vec::new();
        for b in blocks {
            root.extend_from_slice(&varint(((field << 3) | 2) as u64));
            root.extend_from_slice(&varint(b.len() as u64));
            root.extend_from_slice(b);
        }
        root
    }

    #[test]
    fn base_sparse_updates_reconstruct_with_java_semantics() {
        let k1 = mk48(
            12,
            &root_blocks(11, &[varint_block(&[(1, 0), (2, 1), (3, 2), (4, 40)])]),
        );
        let k2 = mk48(12, &root_blocks(11, &[varint_block(&[(1, 1), (2, 2)])]));
        let k3 = mk48(12, &root_blocks(11, &[varint_block(&[(1, 0), (3, 0)])]));
        let k4 = mk48(
            12,
            &root_blocks(11, &[varint_block(&[(1, 0), (2, 2), (3, 1), (4, 10)])]),
        );
        let k5 = mk48(12, &root_blocks(11, &[varint_block(&[(1, 0), (2, 1)])]));
        let packets: Vec<(u32, f32, &[u8])> = vec![
            // A：满字段 owner=1 capturing=2 progress=40
            (8, 10.0, &k1),
            // B：sparse——只有 owner（absent 字段 = 维持前值/默认 None）
            (8, 20.0, &k2),
            // A：capturing 显式清空（3=0）→ progress 连带清空
            (8, 30.0, &k3),
            // A：owner 变更且占领中（1→2, capturing=1, progress=10）
            (8, 40.0, &k4),
            // A：owner 再变更（2→1）→ 占领中清空
            (8, 50.0, &k5),
        ];
        let raw = collect_supremacy_base_updates(&packets);
        assert_eq!(raw.len(), 5, "5 条 sparse 更新全部收集");
        let t = reconstruct_supremacy_base_states(raw);
        assert_eq!(t.len(), 5);
        assert_eq!(
            (
                t[0].base_id,
                t[0].owner_team,
                t[0].capturing_team,
                t[0].capture_progress
            ),
            (0, Some(1), Some(2), Some(40))
        );
        assert_eq!((t[1].base_id, t[1].owner_team), (1, Some(2)));
        // capturing 显式清空 → progress 连带清空
        assert_eq!(
            (t[2].owner_team, t[2].capturing_team, t[2].capture_progress),
            (Some(1), None, None)
        );
        assert_eq!(
            (t[3].owner_team, t[3].capturing_team, t[3].capture_progress),
            (Some(2), Some(1), Some(10))
        );
        // owner 再变更且占领中 → capturing/progress 清空
        assert_eq!(
            (t[4].owner_team, t[4].capturing_team, t[4].capture_progress),
            (Some(1), None, None)
        );
    }

    #[test]
    fn explicit_owner_zero_is_neutral_and_absent_base_index_defaults_to_a() {
        // block 无 field1（absent）→ canonical 边界 wire default 0 = A（唯一补缺省处）
        let q1 = mk48(12, &root_blocks(11, &[varint_block(&[(2, 1)])]));
        let q2 = mk48(12, &root_blocks(11, &[varint_block(&[(2, 0)])]));
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (8, 10.0, &q1),
            (8, 20.0, &q2), // owner 显式 0 → 无主
        ];
        let t = reconstruct_supremacy_base_states(collect_supremacy_base_updates(&packets));
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].base_id, 0);
        assert_eq!(t[0].owner_team, Some(1));
        assert_eq!(t[1].owner_team, None, "显式 0 = 清空归属（无主）");
    }

    #[test]
    fn blank_row_clears_base_on_capture_abort() {
        // 车辆出圈（占领中断）：服务端发只有 base_index 的全缺省行——owner/capturing/progress
        // 同为 0 → proto3 省略全部字段；若按"缺省=维持前值"即空操作，
        // 进度与占领方永久挂在基地上。
        let cap = mk48(
            12,
            &root_blocks(11, &[varint_block(&[(1, 0), (3, 2), (4, 17)])]),
        );
        let blank = mk48(12, &root_blocks(11, &[varint_block(&[(1, 0)])]));
        let packets: Vec<(u32, f32, &[u8])> = vec![(8, 10.0, &cap), (8, 20.0, &blank)];
        let t = reconstruct_supremacy_base_states(collect_supremacy_base_updates(&packets));
        assert_eq!(t.len(), 2);
        assert_eq!(
            (t[0].capturing_team, t[0].capture_progress),
            (Some(2), Some(17))
        );
        assert_eq!(
            (t[1].owner_team, t[1].capturing_team, t[1].capture_progress),
            (None, None, None),
            "全缺省行必须清空占领方与进度（出圈重置）"
        );
    }

    #[test]
    fn blank_row_with_unknown_fields_keeps_previous_state() {
        // 带未知字段（f5）的块语义未证实 → 不按清空处理（fail-closed：不拿未证实证据改状态）
        let cap = mk48(
            12,
            &root_blocks(11, &[varint_block(&[(1, 0), (3, 2), (4, 17)])]),
        );
        let with_f5 = mk48(12, &root_blocks(11, &[varint_block(&[(1, 0), (5, 7)])]));
        let packets: Vec<(u32, f32, &[u8])> = vec![(8, 10.0, &cap), (8, 20.0, &with_f5)];
        let t = reconstruct_supremacy_base_states(collect_supremacy_base_updates(&packets));
        assert_eq!(
            (t[1].capturing_team, t[1].capture_progress),
            (Some(2), Some(17)),
            "未知字段在场：维持前值"
        );
    }

    #[test]
    fn wrapper_gate_and_invalid_blocks() {
        // wrapper=1（名册）即使 root 结构相同也绝不产出事件
        let roster = mk48(1, &root_blocks(11, &[varint_block(&[(1, 0), (2, 1)])]));
        let roster_shaped: Vec<(u32, f32, &[u8])> = vec![(8, 10.0, &roster)];
        assert!(collect_supremacy_base_updates(&roster_shaped).is_empty());
        assert!(collect_supremacy_points(&roster_shaped).is_empty());
        // 不合法块整体跳过：progress=100 越界、team=3 非法
        let badp = mk48(
            12,
            &root_blocks(
                11,
                &[
                    varint_block(&[(1, 0), (4, 100)]),
                    varint_block(&[(1, 1), (2, 3)]),
                ],
            ),
        );
        let bad: Vec<(u32, f32, &[u8])> = vec![(8, 10.0, &badp)];
        assert!(
            collect_supremacy_base_updates(&bad).is_empty(),
            "不合法块绝不产出部分状态"
        );
    }

    #[test]
    fn points_samples_with_gate_and_multi_byte_varint() {
        // points=300 需要多字节 varint；team=3 拒绝；wrapper 门禁
        let pt = mk48(
            13,
            &root_blocks(
                12,
                &[
                    varint_block(&[(1, 1), (2, 300)]),
                    varint_block(&[(1, 2), (2, 95)]),
                    varint_block(&[(1, 3), (2, 10)]),
                ],
            ),
        );
        let pw = mk48(12, &root_blocks(12, &[varint_block(&[(1, 1), (2, 50)])]));
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (8, 10.0, &pt),
            (8, 20.0, &pw), // wrapper=12：同 root 结构绝不产出点数
        ];
        let pts = collect_supremacy_points(&packets);
        assert_eq!(pts.len(), 2);
        assert_eq!((pts[0].team, pts[0].points), (1, 300));
        assert_eq!((pts[1].team, pts[1].points), (2, 95));
    }
}

#[cfg(test)]
mod assault_tests {
    use super::*;

    fn varint(v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        let mut v = v;
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out
    }
    fn varint_block(fields: &[(u32, u64)]) -> Vec<u8> {
        let mut b = Vec::new();
        for (n, v) in fields {
            b.extend_from_slice(&varint((n << 3) as u64));
            b.extend_from_slice(&varint(*v));
        }
        b
    }
    fn root_blocks(field: u32, blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut root = Vec::new();
        for b in blocks {
            root.extend_from_slice(&varint(((field << 3) | 2) as u64));
            root.extend_from_slice(&varint(b.len() as u64));
            root.extend_from_slice(b);
        }
        root
    }

    fn mk48a(wrapper: u32, root: &[u8]) -> Vec<u8> {
        let mut args = varint(wrapper as u64);
        args.push(root.len() as u8);
        args.extend_from_slice(root);
        let mut p = vec![0u8; 12];
        p[4..8].copy_from_slice(&48u32.to_le_bytes());
        p[8..12].copy_from_slice(&(args.len() as u32).to_le_bytes());
        p.extend_from_slice(&args);
        p
    }

    #[test]
    fn assault_progress_family_filtered_and_ordered() {
        // field1=2 族（PROVEN）：progress 递增
        let k1 = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 1), (3, 5)])]),
        );
        let k2 = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 1), (3, 42)])]),
        );
        // field1=1 族**携带 field3**：也是进度（载体在两族间切换）——
        // 只有裸 field4 的兄弟族才是"非进度"
        let f1 = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 1), (2, 1), (3, 9)])]),
        );
        let other = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 1), (2, 1), (4, 7)])]),
        );
        // 越界进度（101）：剔除
        let bad = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 1), (3, 101)])]),
        );
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (8, 20.0, &k2),
            (8, 10.0, &k1),
            (8, 12.0, &f1),
            (8, 15.0, &other),
            (8, 30.0, &bad),
        ];
        let raw = collect_assault_base_updates(&packets);
        assert_eq!(raw.len(), 5, "五块全部收集（含非判定族）");
        let tl = reconstruct_assault_base_states(raw);
        assert_eq!(
            tl.len(),
            3,
            "两族带 field3 者 + 越界剔除 + 裸 field4 族不计"
        );
        assert_eq!((tl[0].clock, tl[0].progress), (10.0, 5), "按 clock 升序");
        assert_eq!(
            (tl[1].clock, tl[1].progress),
            (12.0, 9),
            "field1=1 携带的进度同样入选"
        );
        assert_eq!((tl[2].clock, tl[2].progress), (20.0, 42));
    }

    #[test]
    fn assault_progress_under_field1_one_only() {
        // 遭遇战回归：进度**只**由 field1=1 族承载，
        // field1=2 族只有常量 field4=1——按 field1==2 判族会得到空时间线。
        let init_a = mk48a(8, &root_blocks(8, &[varint_block(&[(1, 2), (2, 1)])]));
        let init_b = mk48a(8, &root_blocks(8, &[varint_block(&[(1, 1), (2, 1)])]));
        let flag = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 1), (4, 1)])]),
        );
        let p1 = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 1), (2, 1), (3, 1)])]),
        );
        let p2 = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 1), (2, 1), (3, 19)])]),
        );
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (8, 1.0, &init_a),
            (8, 1.0, &init_b),
            (8, 2.0, &flag),
            (8, 3.0, &p1),
            (8, 4.0, &p2),
            (8, 5.0, &flag),
        ];
        let tl = reconstruct_assault_base_states(collect_assault_base_updates(&packets));
        assert_eq!(tl.len(), 2, "只由 field1=1 承载时仍须产出进度");
        assert_eq!((tl[0].clock, tl[0].progress), (3.0, 1));
        assert_eq!((tl[1].clock, tl[1].progress), (4.0, 19));
    }

    #[test]
    fn assault_blank_pair_resets_progress_on_capture_abort() {
        // 占领中断（车辆出圈）：进度序列后必然跟一对双缺省块（f3/f4 同为零 → proto3 省略）；
        // 若按"f3 存在"过滤会整块丢弃，时间线停在最后一个正值。
        let mk = |t: &[(u32, u64)]| mk48a(8, &root_blocks(8, &[varint_block(t)]));
        let p1 = mk(&[(1, 2), (2, 1), (3, 3)]);
        let p2 = mk(&[(1, 2), (2, 1), (3, 4)]);
        let clr_a = mk(&[(1, 2), (2, 1)]);
        let clr_b = mk(&[(1, 1), (2, 1)]);
        let restart = mk(&[(1, 2), (2, 1), (3, 1)]);
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (8, 10.0, &p1),
            (8, 11.0, &p2),
            (8, 12.0, &clr_a),
            (8, 12.0, &clr_b), // 同刻一对：只产出一条归零
            (8, 20.0, &restart),
        ];
        let tl = reconstruct_assault_base_states(collect_assault_base_updates(&packets));
        assert_eq!(
            tl.iter().map(|s| (s.clock, s.progress)).collect::<Vec<_>>(),
            vec![(10.0, 3), (11.0, 4), (12.0, 0), (20.0, 1)],
            "双缺省对必须产出归零行；同刻重复块去重；其后重新起算"
        );
    }

    #[test]
    fn assault_flag_only_row_is_not_a_reset() {
        // 只有 field4 标志流的块（占领进行中与进度块同包成对的兄弟族）不得当重置
        let flag = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 1), (2, 1), (4, 1)])]),
        );
        let p1 = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 1), (3, 6)])]),
        );
        let p2 = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 1), (3, 19)])]),
        );
        let packets: Vec<(u32, f32, &[u8])> = vec![(8, 1.0, &p1), (8, 2.0, &flag), (8, 3.0, &p2)];
        let tl = reconstruct_assault_base_states(collect_assault_base_updates(&packets));
        assert_eq!(
            tl.iter().map(|s| (s.clock, s.progress)).collect::<Vec<_>>(),
            vec![(1.0, 6), (3.0, 19)],
            "仅 field4 的块既不产出行、也不归零"
        );
    }

    #[test]
    fn assault_blank_row_without_prior_progress_emits_nothing() {
        // 开局全缺省块（裸初始化对）**只清"确有进度"者**：普通对局也发这一对，
        // 若合成 0 事件会让 assault_bases 恒非空 → 存在性回退判据误判成有目标。
        let blank_a = mk48a(8, &root_blocks(8, &[varint_block(&[(1, 2), (2, 1)])]));
        let blank_b = mk48a(8, &root_blocks(8, &[varint_block(&[(1, 1), (2, 1)])]));
        let only_init: Vec<(u32, f32, &[u8])> =
            vec![(8, 1.0, &blank_a), (8, 1.0, &blank_b), (8, 9.0, &blank_a)];
        assert!(
            reconstruct_assault_base_states(collect_assault_base_updates(&only_init)).is_empty(),
            "无进度在先：全缺省块不产出任何行"
        );
        // 进度归零后再来全缺省块：不重复产出 0
        let p = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 1), (3, 2)])]),
        );
        let packets: Vec<(u32, f32, &[u8])> =
            vec![(8, 1.0, &p), (8, 2.0, &blank_a), (8, 3.0, &blank_b)];
        let tl = reconstruct_assault_base_states(collect_assault_base_updates(&packets));
        assert_eq!(
            tl.iter().map(|s| (s.clock, s.progress)).collect::<Vec<_>>(),
            vec![(1.0, 2), (2.0, 0)],
            "归零只产出一条（第二块见进度已为 0 不再产出行）"
        );
    }

    #[test]
    fn assault_objective_present_at_family_init() {
        // 裸初始化对（双方各一条目标记录）→ **即算目标存在**（字段契约「目标族出现即真，
        // 不要求有进度」——有目标但全场只发这一对的场次，按进度判存在会把目标圈整场压掉）
        let init_a = mk48a(8, &root_blocks(8, &[varint_block(&[(1, 2), (2, 1)])]));
        let init_b = mk48a(8, &root_blocks(8, &[varint_block(&[(1, 1), (2, 1)])]));
        let p0: Vec<(u32, f32, &[u8])> = vec![(8, 1.0, &init_a), (8, 2.0, &init_b)];
        let raw0 = collect_assault_base_updates(&p0);
        assert!(
            has_assault_objective(&raw0),
            "目标族出现过即目标存在（无进度要求）"
        );
        assert!(
            reconstruct_assault_base_states(raw0).is_empty(),
            "存在性不合成进度事件：时间线仍空 → 显示层画 idle 目标圈（无水位）"
        );

        // 出现 field4 标志流（目标系统活跃）但尚无进度 → 目标存在、时间线仍空
        let flag = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 1), (4, 1)])]),
        );
        let p1: Vec<(u32, f32, &[u8])> = vec![(8, 1.0, &init_a), (8, 2.0, &flag)];
        let raw1 = collect_assault_base_updates(&p1);
        assert!(has_assault_objective(&raw1), "有 field4 标志流即目标存在");
        assert!(
            reconstruct_assault_base_states(raw1).is_empty(),
            "但仍无进度事件"
        );

        // 有进度 → 目标存在
        let prog = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 1), (2, 1), (3, 7)])]),
        );
        let p2: Vec<(u32, f32, &[u8])> = vec![(8, 1.0, &prog)];
        assert!(has_assault_objective(&collect_assault_base_updates(&p2)));

        // 无 wrapper8（争霸场）：
        let sup = mk48a(12, &root_blocks(11, &[varint_block(&[(1, 0), (2, 1)])]));
        let p3: Vec<(u32, f32, &[u8])> = vec![(8, 1.0, &sup)];
        assert!(!has_assault_objective(&collect_assault_base_updates(&p3)));

        // field2 不是 1 的族：不认
        let other = mk48a(
            8,
            &root_blocks(8, &[varint_block(&[(1, 2), (2, 9), (4, 1)])]),
        );
        let p4: Vec<(u32, f32, &[u8])> = vec![(8, 1.0, &other)];
        assert!(!has_assault_objective(&collect_assault_base_updates(&p4)));
    }

    // ---------- 装填相位（arena subtype 15/17）----------

    /// 条目：f1=eid(0)、f2=phase(0)、f3=f32(5)、f4=count(0, 可选)
    fn reload_entry(eid: u64, phase: u64, dur: f32, count: Option<u64>) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(varint((1 << 3) as u64));
        b.extend(varint(eid));
        b.extend(varint((2 << 3) as u64));
        b.extend(varint(phase));
        b.extend(varint(((3 << 3) | 5) as u64));
        b.extend_from_slice(&dur.to_le_bytes());
        if let Some(c) = count {
            b.extend(varint((4 << 3) as u64));
            b.extend(varint(c));
        }
        b
    }
    /// protobuf：field{wrap_no}(wire2) → repeated field1(wire2) → 条目
    fn reload_root(wrap_no: u32, entries: &[Vec<u8>]) -> Vec<u8> {
        let mut inner = Vec::new();
        for e in entries {
            inner.extend(varint(((1 << 3) | 2) as u64));
            inner.extend(varint(e.len() as u64));
            inner.extend_from_slice(e);
        }
        let mut root = Vec::new();
        root.extend(varint(((wrap_no << 3) | 2) as u64));
        root.extend(varint(inner.len() as u64));
        root.extend_from_slice(&inner);
        root
    }

    #[test]
    fn reload_phases_from_sub15_and_sub17() {
        // sub15 → wrapper field14；sub17 → wrapper field16（两者字段号不同，必须分别取）
        let e_start = reload_entry(500, 3, 12.39, None); // 装填开始 + 时长
        let e_ready = reload_entry(500, 0, 12.49, Some(1)); // f4=1 就绪
        let p15 = mk48a(15, &reload_root(14, &[e_start, e_ready]));
        let p17 = mk48a(17, &reload_root(16, &[reload_entry(700, 7, 2.5, None)])); // 弹夹内单发
                                                                                   // 非装填 subtype（sub1）即使字段结构相同也不产出
        let p1 = mk48a(1, &reload_root(14, &[reload_entry(999, 3, 9.0, None)]));
        let packets: Vec<(u32, f32, &[u8])> = vec![(8, 5.0, &p15), (8, 1.0, &p17), (8, 2.0, &p1)];
        let updates = collect_arena_updates_filtered(&packets, |s| {
            s == ARENA_SUB_RELOAD_TIME || s == ARENA_SUB_RELOAD_TIME_LIST
        });
        assert_eq!(updates.len(), 2, "只有 sub15/sub17 被收集（sub1 过滤掉）");
        let r = reload_phases_from_updates(&updates);
        assert_eq!(r.len(), 3, "sub15 两条 + sub17 一条");
        assert_eq!(
            (r[0].clock, r[0].eid, r[0].phase),
            (1.0, 700, RELOAD_PHASE_MAG_INTERVAL)
        );
        assert_eq!(r[0].duration_s, Some(2.5));
        assert_eq!(
            (r[1].clock, r[1].eid, r[1].phase),
            (5.0, 500, RELOAD_PHASE_START)
        );
        assert_eq!(r[1].duration_s, Some(12.39));
        assert_eq!(
            r[2].count,
            Some(RELOAD_READY_COUNT),
            "f4=1 原样透传（就绪）"
        );
        assert_eq!(r[2].phase, 0);
    }

    #[test]
    fn reload_phases_sorted_and_per_vehicle_kept_apart() {
        // 同一包内多实体、跨包乱序：输出按 clock 升序且 eid 不被合并
        let p_a = mk48a(15, &reload_root(14, &[reload_entry(11, 3, 8.0, None)]));
        let p_b = mk48a(15, &reload_root(14, &[reload_entry(22, 7, 3.0, None)]));
        let packets: Vec<(u32, f32, &[u8])> = vec![(8, 9.0, &p_a), (8, 2.0, &p_b)];
        let r = reload_phases_from_updates(&collect_arena_updates_filtered(&packets, |s| {
            s == ARENA_SUB_RELOAD_TIME
        }));
        assert_eq!(
            r.iter().map(|x| (x.clock, x.eid)).collect::<Vec<_>>(),
            vec![(2.0, 22), (9.0, 11)]
        );
    }

    #[test]
    fn assault_absent_when_no_wrapper8() {
        // 争霸场（wrapper12）：无 wrapper8 → 攻防战时间线为空（两模式天然互斥）
        let sup = mk48a(12, &root_blocks(11, &[varint_block(&[(1, 0), (2, 1)])]));
        let packets: Vec<(u32, f32, &[u8])> = vec![(8, 10.0, &sup)];
        assert!(collect_assault_base_updates(&packets).is_empty());
        assert!(reconstruct_assault_base_states(vec![]).is_empty());
    }
}
