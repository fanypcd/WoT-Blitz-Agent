//! 射击提取路径的包收集器：launch/endpoint/direct8/warning32/地形命中/
//! 血量降幅区间/tick 时间线/刷新簇/配件（全库原始流导出 dump 也在本模块）。

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 收集每实体的 type=7 同钟多属性刷新簇时钟（AoI 补发/通道切换签名，逆向文档 6.x：
/// 锚点后多属性 <2ms 同钟成簇 = 批量补发快照）；开火/命中事件触发更新方案切换 + 状态补发，簇时钟即切换点。
pub(crate) fn collect_refresh_clusters(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, Vec<f32>> {
    // 每实体 (clock, sub) 排序后滑窗：窗口内出现 ≥2 个不同 sub（<2ms）→ 簇时钟
    let mut seqs: HashMap<u32, Vec<(f32, u32)>> = HashMap::new();
    for (t, clock, p) in packets {
        if *t != 7 || p.len() < 14 {
            continue;
        }
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
            if subs.len() >= 2 {
                cluster_clocks.push(seq[i].0);
            }
            i = j;
        }
        if cluster_clocks.is_empty() {
            continue;
        }
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
pub(crate) fn refresh_cluster_after(
    clusters: &HashMap<u32, Vec<f32>>,
    eid: u32,
    t: f32,
) -> Option<f32> {
    let v = clusters.get(&eid)?;
    let &c = v.iter().find(|c| **c > t && **c <= t + 0.35)?;
    Some(c - t)
}

/// 全链原始流导出（WI 对齐扫描 / 探针用）：per-entity type=10 状态流 + type=7 prop2 流 +
/// 全 shooter 的 method29 发射 / method20 终点 / method8 直击通知。
pub fn dump_replay_streams(packets: &[(u32, f32, &[u8])]) -> serde_json::Value {
    let (mut st10, prop2) = build_entity_indexes(packets);
    for v in st10.values_mut() {
        v.sort_by(|a, b| a.clock.partial_cmp(&b.clock).unwrap());
    }
    let st10_json: serde_json::Map<String, serde_json::Value> = st10
        .into_iter()
        .map(|(eid, v)| {
            (
                format!("{eid:08x}"),
                serde_json::Value::Array(
                    v.into_iter()
                        .map(|s| {
                            serde_json::json!([
                                s.clock, s.pos[0], s.pos[1], s.pos[2], s.yaw, s.pitch, s.roll
                            ])
                        })
                        .collect(),
                ),
            )
        })
        .collect();
    let prop2_json: serde_json::Map<String, serde_json::Value> = prop2
        .into_iter()
        .map(|(eid, v)| {
            (
                format!("{eid:08x}"),
                serde_json::Value::Array(
                    v.into_iter()
                        .map(|(c, r, fr)| serde_json::json!([c, r, fr]))
                        .collect(),
                ),
            )
        })
        .collect();
    let (launches, _) = collect_launches(packets, |_| true);
    let endpoints = collect_endpoints(packets);
    // 文件序事件序列（state-machine 扫描用）：volatile/launch/endpoint/direct8 四类事件按包内出现顺序
    let mut seq: Vec<serde_json::Value> = Vec::new();
    for (t, clock, p) in packets {
        if *t == 10 && p.len() >= 48 {
            let f = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
            seq.push(json!([
                "v",
                clock,
                u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                f(12),
                f(16),
                f(20)
            ]));
            continue;
        }
        if p.len() < 12 {
            continue;
        }
        let m = u32::from_le_bytes([p[4], p[5], p[6], p[7]]);
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if p.len() < 12 + args_len {
            continue;
        }
        let a = &p[12..12 + args_len];
        let f = |o: usize| f32::from_le_bytes([a[o], a[o + 1], a[o + 2], a[o + 3]]);
        if *t == 8 && m == 0x1d && args_len >= 37 {
            seq.push(json!([
                "l",
                clock,
                u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                u32::from_le_bytes([a[4], a[5], a[6], a[7]]),
                f(9),
                f(13),
                f(17)
            ]));
        } else if *t == 8 && m == 0x14 && args_len >= 16 {
            seq.push(json!([
                "e",
                clock,
                u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                f(4),
                f(8),
                f(12)
            ]));
        } else if *t == 8 && m == 0x08 && args_len >= 10 && a[8] == 1 {
            seq.push(json!([
                "h",
                clock,
                u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
                u32::from_le_bytes([a[4], a[5], a[6], a[7]])
            ]));
        } else if *t == 8 && m == 0x00 {
            // method0x00 开火事件：envelope entityId = 射手车辆实体，args=[01]
            seq.push(json!([
                "f",
                clock,
                u32::from_le_bytes([p[0], p[1], p[2], p[3]])
            ]));
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

/// Type5 物化尾部 loadout 的配件 ID 常量（WotbTools item-catalog，BlitzKit 生产定义同步）：
/// 103 = CALIBRATED_SHELLS 校准弹（AP/APCR 穿深 +6%、其他 +7%）、110 = ENHANCED_ARMOR 强化装甲（厚度 +4%）。
pub const EQ_CALIBRATED_SHELLS: u8 = 103;
pub const EQ_ENHANCED_ARMOR: u8 = 110;

/// 射击一方车辆的配件搭载（Type5 物化 `0B 09` 九字节选择串解码）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleEquipment {
    /// 携带校准弹（ID 103）
    pub calibrated_shells: bool,
    /// 携带强化装甲（ID 110）
    pub enhanced_armor: bool,
    /// 九槽原始配件 ID（诊断/未来扩展；未知 ID 不猜名）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<u8>,
}

impl VehicleEquipment {
    pub(crate) fn from_ids(eq: &[u8; 9]) -> Self {
        VehicleEquipment {
            calibrated_shells: eq.contains(&EQ_CALIBRATED_SHELLS),
            enhanced_armor: eq.contains(&EQ_ENHANCED_ARMOR),
            raw: eq.to_vec(),
        }
    }
}

/// 每实体 Type5 物化 → 9 字节配件选择（首条有效物化为准；配件开局固定，敌方再物化
/// Type4→Type33→Type5 重复携带，WotbTools 683/683）。
/// 扫描契约（Java VehicleBattleLoadout 同款）：offset 可变，搜 `0A 06` + 6×14B 描述符 +
/// `0B 09` + 9B；字节全部落在已知配件 ID 域 100..=123 才采纳（framing 误配不猜名）。
pub fn collect_vehicle_equipment(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, [u8; 9]> {
    collect_vehicle_loadout(packets)
        .into_iter()
        .map(|(k, v)| (k, v.equipment))
        .collect()
}

/// 采集每实体的完整开局 loadout（Type5 3+3+9；含 6 条 raw item 描述符）。
/// 同一实体取首条成功解析者（与 equipment 采集同语义）。
pub fn collect_vehicle_loadout(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, VehicleLoadout> {
    let mut out: HashMap<u32, VehicleLoadout> = HashMap::new();
    for (ptype, _, p) in packets {
        if *ptype != 5 {
            continue;
        }
        let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        if out.contains_key(&eid) {
            continue;
        }
        if let Some(l) = scan_loadout(p) {
            out.insert(eid, l);
        }
    }
    out
}

/// 在单条 Type5 载荷内扫描 loadout 块：先定位 `0B 09` + 9B 配件串（字节域 100..=123 校验），
/// 再回找计数标记 `0A KK`——要求 `0A KK` + KK×14B 描述符 + `0B 09` 严丝合缝且 KK≥6
/// （6 条=标准 3 消耗品+3 给养；7 条=受控场变体，多 1 条 14B 描述符、
/// 配件串本身完好；4 条=观察者族，勿当战斗者搭载，拒收）。
/// Avatar method16：车辆模块/乘员状态事件（WotbTools PROVEN 移植）。
///
/// 线格式（`VehicleModuleCrewStateDecoder`，恒定 22B）：
/// `[0..4) avatarEid | [4..8) method=16 | [8..12) argLen=10 | [12..16) vehicleId |
///  [16] stateCode(codeA) | [17] componentCode(codeB) | [18..22) relatedEntityId`。
///
/// codeB → 组件（PROVEN）：31 引擎 / 32 弹药架 / 33 油箱 / 34 右履带 / 35 左履带 /
/// 36 主炮 / 37 炮塔座圈 / 38 观察装置 / 39 车长 / 40 驾驶员 / 41 炮手 / 43 装填手；
/// 其余（含缺失的 42）原样保留为 UNKNOWN，不猜。
/// codeA → 状态：**乘员**（39/40/41/43）：10 震伤 / 22 治愈；**模块**：4 受损降效 /
/// 5 致命失效 / 18 自动修复至受损 / 19 完全修复；其余 UNKNOWN。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleComponent {
    Engine,
    AmmoRack,
    FuelTank,
    RightTrack,
    LeftTrack,
    Gun,
    TurretRotator,
    ObservationDevice,
    Commander,
    Driver,
    Gunner,
    Loader,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleState {
    DamagedDegraded,
    CriticalDisabled,
    AutoRepairedToDamaged,
    FullRepairedClear,
    CrewShellShocked,
    CrewHealed,
    Unknown,
}

impl ModuleComponent {
    fn from_code(code: u8) -> Self {
        match code {
            31 => Self::Engine,
            32 => Self::AmmoRack,
            33 => Self::FuelTank,
            34 => Self::RightTrack,
            35 => Self::LeftTrack,
            36 => Self::Gun,
            37 => Self::TurretRotator,
            38 => Self::ObservationDevice,
            39 => Self::Commander,
            40 => Self::Driver,
            41 => Self::Gunner,
            43 => Self::Loader,
            _ => Self::Unknown,
        }
    }
    fn is_crew(self) -> bool {
        matches!(
            self,
            Self::Commander | Self::Driver | Self::Gunner | Self::Loader
        )
    }
}

/// 模块/乘员状态事件：raw 码与映射并存（raw 保留以便未来语义扩充时不失真）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModuleCrewStateEvent {
    pub clock: f32,
    /// 受损车辆实体
    pub vehicle_eid: u32,
    /// codeA（raw）
    pub state_code: u8,
    /// codeB（raw）
    pub component_code: u8,
    pub component: ModuleComponent,
    pub state: ModuleState,
    pub related_eid: u32,
}

pub fn collect_module_crew_states(packets: &[(u32, f32, &[u8])]) -> Vec<ModuleCrewStateEvent> {
    let mut out = Vec::new();
    for (t, clock, p) in packets {
        if *t != 8 || p.len() != 22 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 16 {
            continue;
        }
        if u32::from_le_bytes([p[8], p[9], p[10], p[11]]) != 10 {
            continue;
        }
        let vehicle_eid = u32::from_le_bytes([p[12], p[13], p[14], p[15]]);
        let state_code = p[16];
        let component_code = p[17];
        let component = ModuleComponent::from_code(component_code);
        let state = if component.is_crew() {
            match state_code {
                10 => ModuleState::CrewShellShocked,
                22 => ModuleState::CrewHealed,
                _ => ModuleState::Unknown,
            }
        } else {
            match state_code {
                4 => ModuleState::DamagedDegraded,
                5 => ModuleState::CriticalDisabled,
                18 => ModuleState::AutoRepairedToDamaged,
                19 => ModuleState::FullRepairedClear,
                _ => ModuleState::Unknown,
            }
        };
        out.push(ModuleCrewStateEvent {
            clock: *clock,
            vehicle_eid,
            state_code,
            component_code,
            component,
            state,
            related_eid: u32::from_le_bytes([p[18], p[19], p[20], p[21]]),
        });
    }
    out.sort_by(|a, b| {
        a.clock
            .partial_cmp(&b.clock)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// 车辆开局 loadout（Type5 3+3+9 结构；WotbTools PROVEN 位置闭合）。
///
/// - `equipment`：9 字节装备选择串——**每字节即装备数值 ID 本身（ASCII 码点）**；
/// - `items`：k 条 14 字节 item 描述符，位置闭合为 `item[0..2] = 3 消耗品`、
///   `item[3..5] = 3 给养`。**内部字段（计时器/动态状态）尚未完全解码 → 原样保留
///   raw，不赋内部语义**（WotbTools 明确裁决：retain raw until fully decoded）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleLoadout {
    pub equipment: [u8; 9],
    pub items: Vec<[u8; 14]>,
}

fn scan_loadout(p: &[u8]) -> Option<VehicleLoadout> {
    let n = p.len();
    let mut pos = 0usize;
    while pos + 11 <= n {
        if p[pos] == 0x0B && p[pos + 1] == 0x09 {
            let eq = &p[pos + 2..pos + 11];
            if eq.iter().all(|&b| (100..=123).contains(&b)) {
                for k in 6..=10usize {
                    if pos >= 2 + k * 14 {
                        let start = pos - 2 - k * 14;
                        if p[start] == 0x0A && p[start + 1] as usize == k {
                            let mut equipment = [0u8; 9];
                            equipment.copy_from_slice(eq);
                            let items = (0..k)
                                .map(|i| {
                                    let s = start + 2 + i * 14;
                                    let mut it = [0u8; 14];
                                    it.copy_from_slice(&p[s..s + 14]);
                                    it
                                })
                                .collect();
                            return Some(VehicleLoadout { equipment, items });
                        }
                    }
                }
            }
        }
        pos += 1;
    }
    None
}

/// method29 (0x1d) 发射事件。args 布局（alen=37）：
/// [shooterEntityId u32][shotId u32][rawFlag u8][launchPoint 3×f32][launchVelocity 3×f32][terminalRaw f32]
#[derive(Clone)]
pub(crate) struct LaunchEntry {
    pub(crate) t: f32,
    pub(crate) shooter: u32,
    pub(crate) shot_id: u32,
    pub(crate) point: [f32; 3],
    pub(crate) vel: [f32; 3],
    /// method29 包处理时刻（**流序**）射手的最后已知 prop2 原始 u16——WI 解析器同构快照。
    /// 与时钟序"≤t 最后采样"的差异仅在同 tick 内包序：method29 包之前到达的 prop2 才计入。
    pub(crate) shooter_prop2: Option<(f32, u16)>, // (采样钟, 原始 u16)
}

/// method20 (0x14) 弹道终点（shotId 配对）。
/// method8 直击通知（全局广播，envelope eid = 受击者）；
/// args = [shooterEntityId u32][victimEntityId u32][01][result u8][extra u8][hash6][tail...]；result 枚举与 type=32 同域，hash6 与同事件 type=32 完全一致。
/// victim_state = 该通知包处理时刻（文件序）受击者的最后已知 type=10 姿态——
/// wotinspector distance 的精确取值基准，受击方锚点。
pub(crate) struct DirectHit8 {
    pub(crate) t: f32,
    pub(crate) shooter: u32,
    pub(crate) victim: u32,
    pub(crate) result: u8,
    /// args[10] = **服务器下发的受击部件索引 cmpIndex**（showDamageFromShot
    /// 的 8 字节 segment 描述符元素 byte1，客户端 BWUtils::DecodeShotSegment 以 bboxes[4]/
    /// partMatrixes[4] 按此部件放置着弹点——见《回放与射击逆向总集》第三篇 §二）。
    /// 部件对应（命中离地高度分层实证）：**0=底盘/履带、1=车体、2=炮塔、3=炮管（直射弹未观测）**。
    pub(crate) component_index: Option<u8>,
    pub(crate) hash6: [u8; 6],
    pub(crate) victim_state: Option<([f32; 3], [f32; 3], f32)>, // (pos, ang[yaw,pitch,roll], 状态采样时钟)
    /// method8 包处理时刻（**流序**）受击者的最后已知 prop2 原始 u16——WI 解析器同构快照
    /// （battle.json turret_yaw/gun_pitch 的取样基准：炮塔 coarse10 与流序快照逐位相等——
    /// 同 tick 内 prop2 与 method8 的包序决定取值）。(采样钟, 原始 u16)
    pub(crate) victim_prop2: Option<(f32, u16)>,
}

/// type=32 来袭炮弹警告/命中通知（eid = 受击者，AoI 广播含他人命中）。
/// 帧结构（WotbTools 16,850/16,850）：[eid u32][flag u8][bodyLength u32][body]，bodyLength == payloadLen − 9。
/// len=26 (bodyLen=17): [eid u32][01][bodyLen u32][u16@9][flag@11][hash6@12..18][segment u64@18..26]
/// len=27 (bodyLen=18): [eid u32][01][bodyLen u32][u16@9][flag@11][01@12][hash6@13..19][segment u64@19..27]
/// hash6 6B = [shell u16][来向 yaw u16][抵达 pitch u16]：
///   yaw = (u16−32768)/32768×π = 受击者指向射手的方位角；pitch = (u16−32768)/32768×(π/2)
///   = 抵达垂直角。非命中的路过炮弹警告会被 yaw 校验弃用（decoded_target_gun_pitch）。
#[derive(Clone)]
pub(crate) struct ArenaWarning32 {
    pub(crate) t: f32,
    pub(crate) eid: u32,
    pub(crate) result: u8,
    pub(crate) segment: u64,
    pub(crate) hash6: [u8; 6],
    pub(crate) inc_yaw: f32,
    pub(crate) inc_pitch: f32,
}

/// 血量链降幅区间（参考 WotbTools PlaybackCombatReconstruction.deriveLosses）。
pub(crate) struct DmgLoss {
    pub(crate) victim: u32,
    pub(crate) source: u32,
    pub(crate) t_prev: f32,
    pub(crate) t_cur: f32,
    pub(crate) dmg: u32,
    pub(crate) hp_cur: u16,
}

/// method29 (0x1d) 发射事件收集（作者/他人路径共用）：clock ≥5s，按 shooter 过滤。
///
/// **一次开火 = (shooter, shotId) 一条链**：同键的后续 method29 是**同一发炮弹的后续弹道段**
/// （服务器在命中/跳弹时刻重播该发续段：`point` = 跳弹/穿透出射点、`vel` = 续段方向——实测
/// 跳弹样本：首发在炮口仰角 −1.5°，续段起点在受击方装甲处、方向 +38.6°，与该方法8 命中通知
/// 同刻）。因此这里**不做去重**：全部包按 (clock, 流序) 稳定排序返回，由组装方按
/// (shooter, shotId) 分组——首条 = 发射段，其余 = 续段（见 [`shot_segments`]）。
///
/// 按发射时刻排序（稳定排序 → 同刻保流序）。返回 (发射列表, 每射手首个 args<37 包的 args_len)
/// ——作者路径据此对**自己**的短包 fail-fast，他人路径跳过（宽松）。
pub(crate) fn collect_launches(
    packets: &[(u32, f32, &[u8])],
    keep: impl Fn(u32) -> bool,
) -> (Vec<LaunchEntry>, HashMap<u32, usize>) {
    let mut out: Vec<LaunchEntry> = Vec::new();
    let mut short_args: HashMap<u32, usize> = HashMap::new();
    // 流序当前 prop2（全实体维护——keep 过滤只作用于发射事件本身）
    let mut ang2: std::collections::HashMap<u32, (f32, u16)> = Default::default();
    for (t2, clock, p) in packets {
        if *clock < 5.0 || p.len() < 12 {
            continue;
        }
        if *t2 == 7 && p.len() >= 14 && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2 {
            ang2.insert(
                u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                (*clock, u16::from_le_bytes([p[12], p[13]])),
            );
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x1d {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 4 || 12 + args_len > p.len() {
            continue;
        } // 连 shooter 都读不出：无法归属，跳过
        let a = &p[12..12 + args_len];
        let shooter = u32::from_le_bytes([a[0], a[1], a[2], a[3]]);
        if !keep(shooter) {
            continue;
        }
        if args_len < 37 {
            short_args.entry(shooter).or_insert(args_len);
            continue;
        }
        let shot_id = u32::from_le_bytes([a[4], a[5], a[6], a[7]]);
        let f = |o: usize| f32::from_le_bytes([a[o], a[o + 1], a[o + 2], a[o + 3]]);
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

/// 一条弹道链的**续段序列**：(时刻, 起点, 速度)，按飞行顺序（见 [`shot_segments`]）
pub(crate) type ShotSegmentChain = Vec<(f32, [f32; 3], [f32; 3])>;
/// (shooter, shotId) → 该发的续段序列（无续段 = 无键）
pub(crate) type ShotSegments = HashMap<(u32, u32), ShotSegmentChain>;

/// (shooter, shotId) 链 → **续段表**：每条链的首条为发射段，其余为**同一发的续段**
/// （命中/跳弹后继续飞：起点 = 装甲接触点 / 穿透出射点，速度 = 续段方向，见 `collect_launches`）。
///
/// 返回 `(续段标记, 续段表)`：`is_continuation[i]` 为真表示第 i 条是某发的续段（不是独立射击，
/// 组装时跳过）；`segments[(shooter, shotId)]` = 该发**全部续段的 (时刻, 起点, 速度)**，按飞行
/// 顺序（= launches 的 clock/流序）。渲染层用它把弹道画成折线（`from → 续段起点… → method20 终点`），
/// 命中归属窗口用首个续段时刻。无续段时表内无该键（直线弹道，终点用 method20）。
pub(crate) fn shot_segments(launches: &[LaunchEntry]) -> (Vec<bool>, ShotSegments) {
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut is_continuation = vec![false; launches.len()];
    let mut segments: ShotSegments = HashMap::new();
    for (i, l) in launches.iter().enumerate() {
        let key = (l.shooter, l.shot_id);
        if seen.insert(key) {
            continue;
        }
        is_continuation[i] = true;
        segments.entry(key).or_default().push((l.t, l.point, l.vel));
    }
    (is_continuation, segments)
}

/// 弹道折线：`(中间点 via, 各段时长 leg_secs)`。
///
/// 时长按 **几何 / 段速度** 计算（段 k 的速度 = 该段起点处速度：发射段用发射速度，续段用其自带
/// 速度）——**不用包时钟**：包钟是 10Hz 量化的，跳弹常与发射同刻（样本里 71.219 vs 71.22），
/// 用钟会得到零长段（炮弹瞬间闪到跳弹点）。末段速度为最后一个续段速度。
/// `via` 与 `leg_secs` 满足 `leg_secs.len() == via.len() + 1`。
pub(crate) fn shot_legs(
    from: [f32; 3],
    launch_vel: [f32; 3],
    chain: Option<&ShotSegmentChain>,
    to: [f32; 3],
) -> (Vec<[f32; 3]>, Vec<f32>) {
    let empty: ShotSegmentChain = Vec::new();
    let chain = chain.unwrap_or(&empty);
    let mut pts: Vec<[f32; 3]> = Vec::with_capacity(chain.len() + 2);
    pts.push(from);
    for (_, p, _) in chain.iter() {
        pts.push(*p);
    }
    pts.push(to);
    let speed_at = |k: usize| -> f32 {
        let v = if k == 0 {
            launch_vel
        } else {
            chain.get(k - 1).map(|(_, _, v)| *v).unwrap_or(launch_vel)
        };
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    };
    let mut legs: Vec<f32> = Vec::with_capacity(pts.len() - 1);
    for k in 0..pts.len() - 1 {
        let d = [
            pts[k + 1][0] - pts[k][0],
            pts[k + 1][1] - pts[k][1],
            pts[k + 1][2] - pts[k][2],
        ];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let s = speed_at(k);
        legs.push(if s > 1.0 { dist / s } else { 0.5 });
    }
    let via = pts[1..pts.len() - 1].to_vec();
    (via, legs)
}

/// method20 (0x14) 弹道终点收集（作者/他人路径共用）：shotId 配对（含 miss 的空地终点），重复 shotId 保留首条。
pub(crate) fn collect_endpoints(packets: &[(u32, f32, &[u8])]) -> HashMap<u32, (f32, [f32; 3])> {
    let mut endpoints: HashMap<u32, (f32, [f32; 3])> = HashMap::new();
    for (_, clock, p) in packets {
        if p.len() < 28 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x14 {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 16 || 12 + args_len > p.len() {
            continue;
        }
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
pub(crate) fn collect_direct_hits8(packets: &[(u32, f32, &[u8])]) -> Vec<DirectHit8> {
    // 文件序状态机：按包出现顺序维护每实体最后已知 type=10 姿态，method8 到达时快照受击者。
    // （wi 对齐验证：distance = |state[shooter] − state[victim]|@method8，99/99 发 median 残差 2μm）
    let mut pose: HashMap<u32, ([f32; 3], [f32; 3], f32)> = HashMap::new();
    // 同一状态机的 prop2 通道：method8 到达时快照受击者最后已知 prop2（WI turret_yaw 同基准）
    let mut ang2: HashMap<u32, (f32, u16)> = HashMap::new();
    let mut direct_hits8: Vec<DirectHit8> = Vec::new();
    for (t2, clock, p) in packets {
        if *t2 == 10 && p.len() >= 48 {
            let f = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
            pose.insert(
                u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                ([f(12), f(16), f(20)], [f(36), f(40), f(44)], *clock),
            );
            continue;
        }
        if *t2 == 7 && p.len() >= 14 && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2 {
            ang2.insert(
                u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                (*clock, u16::from_le_bytes([p[12], p[13]])),
            );
            continue;
        }
        if p.len() < 12 + 10 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x08 {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 10 || 12 + args_len > p.len() {
            continue;
        }
        let a = &p[12..12 + args_len];
        if a[8] != 0x01 {
            continue;
        }
        let victim = u32::from_le_bytes([a[4], a[5], a[6], a[7]]);
        // args = [shooter u32][victim u32][count u8][element 8B = result|cmpIndex|hash6 u48][tail 4B]
        // a[10] = 服务器下发的受击部件索引 cmpIndex（showDamageFromShot/DecodeShotSegment）
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

/// method8（0x08）伤害/命中反馈通知的**原始**形态：全变体、不分类（[`collect_direct_hits8`]
/// 只收 `args[8]==1` 的直击元素并做射击配对，本收集器保留每一包供消费方自行分类）。
/// envelope eid = 方法调用目标实体（通常 = 受击者）；args =
/// `[shooter u32][victim u32][count u8][result u8][cmpIndex u8][hash6][tail…]`。
/// 字段缺失（载荷不足）一律 None——不臆测；分类口径（直击 / 未解码变体 / 短体）属于消费方。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitNotice {
    pub clock: f32,
    /// envelope 实体（方法调用目标）
    pub eid: u32,
    /// 包载荷总长（字节；含 12 字节 envelope 头）
    pub payload_len: u32,
    pub shooter_eid: Option<u32>,
    pub victim_eid: Option<u32>,
    /// args[9]：游戏命中结果枚举（与 type=32 segment 低字节同域）
    pub result: Option<u8>,
    /// args[10]：受击部件索引 cmpIndex
    pub secondary: Option<u8>,
}

/// method8 原始通知收集（仅 type=8 实体方法包；按时钟排序，稳定保留包序）。
pub fn collect_hit_notices(packets: &[(u32, f32, &[u8])]) -> Vec<HitNotice> {
    let mut out: Vec<HitNotice> = Vec::new();
    for (ptype, clock, p) in packets {
        if *ptype != 8 || p.len() < 12 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x08 {
            continue;
        }
        let a = &p[12..];
        let u32_at = |o: usize| {
            (a.len() >= o + 4).then(|| u32::from_le_bytes([a[o], a[o + 1], a[o + 2], a[o + 3]]))
        };
        out.push(HitNotice {
            clock: *clock,
            eid: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            payload_len: p.len() as u32,
            shooter_eid: u32_at(0),
            victim_eid: u32_at(4),
            result: a.get(9).copied(),
            secondary: a.get(10).copied(),
        });
    }
    out.sort_by(|x, y| x.clock.partial_cmp(&y.clock).unwrap());
    out
}

/// type=32 警告/命中通知收集（作者/他人路径共用）。不排序：作者路径取窗口内最早一条（取后自排），
/// 他人路径按 hash6 令牌精确配对与顺序无关——各自保持原语义。
/// Type32 mobile `flag=0` 长体的**消耗品生命周期**事件（WotbTools PROVEN 移植）。
///
/// 数据链（docs/research/replay/consumable-lifecycle.md，Blitz 11.19 国服 34 场）：
/// Type32 `entityTypeId=2` / `flag=0` / 16 字节体（body 自 p[9] 起）：
/// `wireCode = body[2]`、`state = body[3]`、`bodyClock = f64 LE body[4..12)`、
/// `param = f32 LE body[12..16)`。
///
/// state 语义（该族已闭合）：1 注册/可用、2 激活（param = 有效持续时长，瞬发为 0）、
/// 3 持续结束/冷却转换（param = 有效冷却配置）、255 实体/控制拆除。
///
/// 语义红线：**wireCode 未闭合者原样保留、不猜产品身份**（文档明确；已知
/// 0x08 自动灭火器、0xBD 次级强化引擎仅在消费方映射表中使用）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ConsumableTransition {
    /// 包时钟（回放时钟，秒；时间轴以此为准）
    pub clock: f32,
    pub eid: u32,
    /// body[2] 原始线码（未闭合值不赋身份）
    pub wire_code: u8,
    /// body[3] 状态（1/2/3/255；其他值原样透传）
    pub state: u8,
    /// body[4..12) 原始 f64（与包时钟**不同源**——注册态恒 0、有事件时可达 10s 偏差，
    /// 精确语义未闭合 → 原样保留、不作时间轴依据，时间轴一律用 `clock`）
    pub body_clock: f64,
    /// body[12..16)：state=2 为有效持续时长、state=3 为有效冷却（其余无定义）
    pub param: f32,
}

pub fn collect_consumable_transitions(packets: &[(u32, f32, &[u8])]) -> Vec<ConsumableTransition> {
    let mut out = Vec::new();
    for (t, clock, p) in packets {
        if *t != 32 || p.len() < 25 {
            continue;
        }
        if p[4] != 0x00 {
            continue;
        } // flag=0 族（flag=1 为炮弹警告，另一解析器）
        let body_len = u32::from_le_bytes([p[5], p[6], p[7], p[8]]) as usize;
        if body_len != 16 || 9 + body_len != p.len() {
            continue;
        } // 该族恒 16B 体
        let body = &p[9..];
        out.push(ConsumableTransition {
            clock: *clock,
            eid: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            wire_code: body[2],
            state: body[3],
            body_clock: f64::from_le_bytes(body[4..12].try_into().unwrap()),
            param: f32::from_le_bytes(body[12..16].try_into().unwrap()),
        });
    }
    out.sort_by(|a, b| {
        a.clock
            .partial_cmp(&b.clock)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

pub(crate) fn collect_warnings32(packets: &[(u32, f32, &[u8])]) -> Vec<ArenaWarning32> {
    let mut warnings32: Vec<ArenaWarning32> = Vec::new();
    for (t, clock, p) in packets {
        if *t != 32 || p.len() < 26 {
            continue;
        }
        if p[4] != 0x01 {
            continue;
        }
        // p[5..9] = bodyLength（WotbTools PROVEN：恒 == payloadLen−9，作帧完整性断言）
        let body_len = u32::from_le_bytes([p[5], p[6], p[7], p[8]]) as usize;
        if body_len + 9 != p.len() {
            continue;
        }
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

/// Avatar method 0x1b 地形命中包（仅无坦克命中时广播）：shotId 配对，args(34) 布局见 TerrainImpactData。
/// 全局广播含所有玩家脱靶弹——作者/他人路径共用，args[4..8] 的
/// shell_global_id 同时是弹种兜底链第三级的数据源。gid 掩码 & 0xFFFFFF：
/// byte3（bits24-31）为噪声——不掩码时 u32 值跳出 24 位弹种表键域；
/// 但 bits16-23 承载局部 id 高位（如 IS-7 AP=0x8250a），掩码窄于 24 位会把这类 id 截断成查不中。
/// 0x1b [21..33) = 末段速度方向向量。
pub(crate) fn collect_terrain_impacts(
    packets: &[(u32, f32, &[u8])],
) -> HashMap<u32, (u32, TerrainImpactData)> {
    let mut terrain_impacts: HashMap<u32, (u32, TerrainImpactData)> = HashMap::new();
    for (_, _, p) in packets {
        if p.len() < 46 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x1b {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 34 || 12 + args_len > p.len() {
            continue;
        }
        let a = &p[12..12 + args_len];
        let f = |o: usize| f32::from_le_bytes([a[o], a[o + 1], a[o + 2], a[o + 3]]);
        terrain_impacts
            .entry(u32::from_le_bytes([a[0], a[1], a[2], a[3]]))
            .or_insert((
                u32::from_le_bytes([a[4], a[5], a[6], a[7]]) & 0xFFFFFF,
                TerrainImpactData {
                    material: a[8],
                    impact_point: [f(9), f(13), f(17)],
                    terminal_dir: [f(21), f(25), f(29)],
                },
            ));
    }
    terrain_impacts
}

/// type=32 警告包抵达成角解码与校验：
/// 候选 = 受击者 eid 的警告、命中 ±3s 窗口（来袭警告覆盖未命中弹，含路过炮弹）；
/// 选取 = 解码来向方位角与位置推算方位角（受击者→射手）偏差最小者；偏差 >15° 弃用；
/// |pitch|>30°（非直射抵达角）弃用。通过 = 该警告确属命中本车的炮弹，
/// pitch 即"受击者反向瞄准射手的俯仰"（渲染炮口指向射手的炮管俯角）。
pub(crate) fn decoded_target_gun_pitch(
    warnings32: &[ArenaWarning32],
    victim: u32,
    end_time: f32,
    bearing: Option<f32>,
) -> Option<(f32, f32)> {
    let bearing = bearing?;
    let mut best: Option<(f32, f32, f32)> = None; // (yaw 偏差, pitch, yaw)
    for w in warnings32 {
        if w.eid != victim {
            continue;
        }
        if w.t > end_time + 0.1 || w.t <= end_time - 3.0 {
            continue;
        }
        let err = ((w.inc_yaw - bearing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI)
            .abs();
        if err > 0.262 {
            continue;
        } // 15°
        if best.as_ref().map(|(be, _, _)| err < *be).unwrap_or(true) {
            best = Some((err, w.inc_pitch, w.inc_yaw));
        }
    }
    let (_, pitch, yaw) = best?;
    if pitch.to_degrees().abs() > 30.0 {
        return None;
    }
    Some((pitch, yaw))
}

/// 血量链降幅区间推导（作者/他人路径共用）：同钟同 HP 去冲突后取相邻降幅；
/// cause=0 炮弹直击；author_filter = Some(eid) 仅保留该射手造成的降幅（作者路径），None 保留全部 source（他人路径）。
/// initial_hp（type=5 满血锚点）：早于受害者首个 method1 采样时前插 seed，使首刀降幅可推导；
/// seed 时钟回退 1ms，避免与同钟 method1 事件构成同钟异值冲突而被去重整体丢弃；
/// seed 晚于首个 method1（AoI 迟到全量包）时放弃，保持原行为。
pub(crate) fn derive_dmg_losses(
    hp_events: &[HpEvent],
    author_filter: Option<u32>,
    initial_hp: &HashMap<u32, (f32, u16)>,
) -> Vec<DmgLoss> {
    let mut dmg_losses: Vec<DmgLoss> = Vec::new();
    let mut by_victim: std::collections::HashMap<u32, Vec<&HpEvent>> =
        std::collections::HashMap::new();
    for e in hp_events {
        by_victim.entry(e.victim).or_default().push(e);
    }
    for (victim, evs) in &by_victim {
        let mut samples: Vec<(f32, u16, u32, u8)> = Vec::new();
        if let Some(&(ts, hp)) = initial_hp.get(victim) {
            if evs.first().is_none_or(|e| ts <= e.clock) {
                samples.push(((ts - 1e-3).max(0.0), hp, 0, 0));
            }
        }
        let mut i = 0usize;
        while i < evs.len() {
            let t = evs[i].clock;
            let hp = evs[i].hp;
            let mut conflict = false;
            let mut j = i + 1;
            while j < evs.len() && (evs[j].clock - t).abs() <= 1e-6 {
                if evs[j].hp != hp {
                    conflict = true;
                }
                j += 1;
            }
            if !conflict {
                samples.push((t, hp_terminal_normalized(hp), evs[i].source, evs[i].cause));
            }
            i = j;
        }
        for w in 1..samples.len() {
            let (t_prev, hpp, _, _) = samples[w - 1];
            let (t_cur, hpc, srcc, causec) = samples[w];
            if hpc < hpp && causec == 0 && author_filter.is_none_or(|a| srcc == a) {
                dmg_losses.push(DmgLoss {
                    victim: *victim,
                    source: srcc,
                    t_prev,
                    t_cur,
                    dmg: (hpp - hpc) as u32,
                    hp_cur: hpc,
                });
            }
        }
    }
    dmg_losses.sort_by(|a, b| a.t_cur.partial_cmp(&b.t_cur).unwrap());
    dmg_losses
}

/// 血量降幅 → 发射的互斥归属：每段降幅只归属一次，给区间
/// (t_prev, t_cur] 内 (victim, shooter) 匹配且 end_time 最大的发射。血量事件与命中
/// 同 tick（prop3/method1 时刻 == 命中时刻），降幅属于区间内最后一发命中；
/// 更早命中的伤害会形成独立血量事件不共区间。未穿弹与降幅同区间时不抢归属
/// （end_time 更小即让位）。
/// 返回 发射序号 → (dmg, hp_cur)；无人认领的降幅丢弃（source 不符等）。
pub(crate) fn assign_dmg_losses(
    shots: &[Option<(f32, u32, u32)>], // (end_time, shooter, victim)；None = 无终点/脱靶
    losses: &[DmgLoss],
) -> HashMap<usize, (u32, u16)> {
    let mut out: HashMap<usize, (u32, u16)> = HashMap::new();
    for lo in losses {
        let mut best: Option<(f32, usize)> = None; // (end_time, 发射序号)
        for (si, s) in shots.iter().enumerate() {
            let Some((end_time, shooter, victim)) = *s else {
                continue;
            };
            if victim != lo.victim || shooter != lo.source {
                continue;
            }
            if !(lo.t_prev < end_time && end_time <= lo.t_cur + 1e-6) {
                continue;
            }
            if out.contains_key(&si) {
                continue;
            }
            if best.is_none_or(|(t, _)| end_time > t) {
                best = Some((end_time, si));
            }
        }
        if let Some((_, si)) = best {
            out.insert(si, (lo.dmg, lo.hp_cur));
        }
    }
    out
}

/// type=35 tick 时间线收集（作者/他人路径共用）。
pub(crate) fn collect_tick_timeline(packets: &[(u32, f32, &[u8])]) -> Vec<(f32, u8)> {
    packets
        .iter()
        .filter(|(t, _, p)| *t == 35 && !p.is_empty())
        .map(|(_, clock, p)| (*clock, p[0]))
        .collect()
}

/// type=35 tick 计数器插值（作者/他人路径共用）：timeline 按 clock 排序 → 二分定位首个 clock ≥ t 的相邻段线性内插；
/// u8 回绕展开（差值掩 0xFF，>128 视为回退取负）；t 早于首包按首段向后外推（与原线性实现一致），t 晚于末包取末值，样本 <2 条取末值。
pub(crate) fn tick_at(tick_timeline: &[(f32, u8)], t: f32) -> f32 {
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
    if t1 <= t0 {
        return v0 as f32;
    }
    let dv = ((v1 as i32 - v0 as i32) & 0xFF) as f32;
    let dv = if dv > 128.0 { dv - 256.0 } else { dv };
    let dt = t1 - t0;
    if dt <= 0.0 {
        return v0 as f32;
    }
    v0 as f32 + dv * (t - t0) / dt
}

#[cfg(test)]
mod consumable_tests {
    use super::*;

    /// 合成 Type32 flag=0 帧：[eid u32][flag=0][bodyLen u32=16][body 16B]
    fn mk32(eid: u32, wire: u8, state: u8, body_clock: f64, param: f32) -> Vec<u8> {
        let mut body = vec![0u8; 16];
        body[2] = wire;
        body[3] = state;
        body[4..12].copy_from_slice(&body_clock.to_le_bytes());
        body[12..16].copy_from_slice(&param.to_le_bytes());
        let mut p = vec![0u8; 9];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[4] = 0x00; // flag=0（消耗品族）
        p[5..9].copy_from_slice(&16u32.to_le_bytes());
        p.extend_from_slice(&body);
        p
    }

    #[test]
    fn parses_flag0_consumable_family() {
        let f1 = mk32(0x21, 0x08, 2, 42.5, 6.0); // 激活（持续 6s）
        let f2 = mk32(0x21, 0x08, 3, 48.5, 90.0); // 冷却
        let packets: Vec<(u32, f32, &[u8])> = vec![(32, 48.5, &f2), (32, 42.5, &f1)];
        let t = collect_consumable_transitions(&packets);
        assert_eq!(t.len(), 2);
        assert_eq!(
            (t[0].clock, t[0].eid, t[0].wire_code, t[0].state),
            (42.5, 0x21, 0x08, 2)
        );
        assert!((t[0].body_clock - 42.5).abs() < 1e-9);
        assert!((t[0].param - 6.0).abs() < 1e-6);
        assert_eq!((t[1].clock, t[1].state), (48.5, 3), "按包时钟升序");
    }

    #[test]
    fn ignores_flag1_warning_family_and_bad_shapes() {
        // flag=1（炮弹警告族，26B）不得进消耗品集合；bodyLen 非 16 亦拒
        let mut warn = vec![0u8; 26];
        warn[4] = 0x01;
        warn[5..9].copy_from_slice(&17u32.to_le_bytes()); // 26-9=17 ≠ 16
        let mut wrong_len = mk32(0x22, 0x08, 2, 1.0, 1.0);
        wrong_len[5..9].copy_from_slice(&15u32.to_le_bytes()); // 声明 15 与实际 16 不符
        let packets: Vec<(u32, f32, &[u8])> = vec![(32, 1.0, &warn), (32, 2.0, &wrong_len)];
        assert!(collect_consumable_transitions(&packets).is_empty());
    }
}

#[cfg(test)]
mod loadout_tests {
    use super::*;

    /// 合成 Type5 loadout 块：`0A 06` + 6×14B 描述符 + `0B 09` + 9B 装备串
    fn mk_type5_loadout() -> Vec<u8> {
        let mut p = vec![0u8; 60];
        p[0..4].copy_from_slice(&0x31u32.to_le_bytes());
        p[51..53].copy_from_slice(&1000u16.to_le_bytes());
        p.extend_from_slice(&[0x0A, 0x06]);
        for i in 0..6u8 {
            let mut it = [0u8; 14];
            it[0] = 0x80 + i; // 可辨识的每槽字节
            p.extend_from_slice(&it);
        }
        p.extend_from_slice(&[0x0B, 0x09]);
        p.extend_from_slice(&[100, 101, 102, 103, 104, 105, 106, 107, 108]); // 装备串（ASCII 数值 ID 域）
        p
    }

    #[test]
    fn collects_equipment_and_six_raw_descriptors() {
        let payload = mk_type5_loadout();
        let packets: Vec<(u32, f32, &[u8])> = vec![(5, 1.0, &payload)];
        let l = collect_vehicle_loadout(&packets);
        let v = l.get(&0x31).expect("实体 loadout 应被采集");
        assert_eq!(v.items.len(), 6, "标准 3 消耗品 + 3 给养 = 6 条描述符");
        assert_eq!(v.equipment, [100, 101, 102, 103, 104, 105, 106, 107, 108]);
        assert_eq!(v.items[0][0], 0x80, "描述符原样保留（不解析内部）");
        assert_eq!(v.items[5][0], 0x85);
        // 兼容入口取同一 9B 串
        assert_eq!(
            collect_vehicle_equipment(&packets)
                .get(&0x31)
                .copied()
                .unwrap(),
            v.equipment
        );
    }
}

#[cfg(test)]
mod module_crew_tests {
    use super::*;

    /// 合成 Avatar method16 包（恒定 22B）：[avatarEid][method=16][argLen=10][vehicleId][codeA][codeB][relatedEid]
    fn mk16(avatar: u32, vehicle: u32, code_a: u8, code_b: u8, related: u32) -> Vec<u8> {
        let mut p = vec![0u8; 22];
        p[0..4].copy_from_slice(&avatar.to_le_bytes());
        p[4..8].copy_from_slice(&16u32.to_le_bytes());
        p[8..12].copy_from_slice(&10u32.to_le_bytes());
        p[12..16].copy_from_slice(&vehicle.to_le_bytes());
        p[16] = code_a;
        p[17] = code_b;
        p[18..22].copy_from_slice(&related.to_le_bytes());
        p
    }

    #[test]
    fn maps_module_and_crew_state_families() {
        let engine_dmg = mk16(0xAA, 0x21, 4, 31, 0); // 模块：引擎受损
        let gun_clear = mk16(0xAA, 0x21, 19, 36, 0); // 模块：主炮完全修复
        let crew_shock = mk16(0xAA, 0x22, 10, 41, 0x21); // 乘员：炮手震伤（related=攻击者）
        let crew_heal = mk16(0xAA, 0x22, 22, 43, 0); // 乘员：装填手治愈
        let unknown_mod = mk16(0xAA, 0x23, 4, 42, 0); // 42 未定义 → UNKNOWN 组件
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (8, 20.0, &gun_clear),
            (8, 10.0, &engine_dmg),
            (8, 30.0, &crew_shock),
            (8, 40.0, &crew_heal),
            (8, 50.0, &unknown_mod),
        ];
        let ev = collect_module_crew_states(&packets);
        assert_eq!(ev.len(), 5);
        assert_eq!(
            (ev[0].clock, ev[0].vehicle_eid),
            (10.0, 0x21),
            "按 clock 升序"
        );
        assert_eq!(ev[0].component, ModuleComponent::Engine);
        assert_eq!(ev[0].state, ModuleState::DamagedDegraded);
        assert_eq!(ev[1].component, ModuleComponent::Gun);
        assert_eq!(ev[1].state, ModuleState::FullRepairedClear);
        // 乘员族：同一 codeA 在乘员下解读为震伤/治愈（与模块族不同）
        assert_eq!(ev[2].component, ModuleComponent::Gunner);
        assert_eq!(ev[2].state, ModuleState::CrewShellShocked);
        assert_eq!(ev[2].related_eid, 0x21);
        assert_eq!(ev[3].component, ModuleComponent::Loader);
        assert_eq!(ev[3].state, ModuleState::CrewHealed);
        // 未定义码保持 UNKNOWN，且 raw 码保留
        assert_eq!(ev[4].component, ModuleComponent::Unknown);
        assert_eq!(ev[4].component_code, 42);
    }

    #[test]
    fn rejects_non_method16_and_wrong_shape() {
        let mut other = mk16(0xAA, 0x21, 4, 31, 0);
        other[4..8].copy_from_slice(&35u32.to_le_bytes()); // method=35 → 非 method16
        let mut bad_len = mk16(0xAA, 0x21, 4, 31, 0);
        bad_len[8..12].copy_from_slice(&9u32.to_le_bytes()); // argLen != 10
        let short = vec![0u8; 21]; // 非 22B
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (8, 1.0, &other),
            (8, 2.0, &bad_len),
            (8, 3.0, &short),
            (7, 4.0, &short),
        ];
        assert!(collect_module_crew_states(&packets).is_empty());
    }
}

#[cfg(test)]
mod launch_identity_tests {
    use super::*;

    fn method29(shooter: u32, shot_id: u32, point: [f32; 3], vel: [f32; 3]) -> Vec<u8> {
        let mut args = vec![0u8; 37];
        args[0..4].copy_from_slice(&shooter.to_le_bytes());
        args[4..8].copy_from_slice(&shot_id.to_le_bytes());
        args[8] = 4;
        for (i, v) in point.into_iter().chain(vel).enumerate() {
            let off = 9 + i * 4;
            args[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        let mut p = vec![0u8; 12];
        p[4..8].copy_from_slice(&0x1du32.to_le_bytes());
        p[8..12].copy_from_slice(&(args.len() as u32).to_le_bytes());
        p.extend_from_slice(&args);
        p
    }

    #[test]
    fn repeated_method29_with_same_shot_id_is_one_shoot_and_keeps_primary_launch() {
        let first = method29(
            7,
            45509301,
            [99.82513, 25.24507, 20.54626],
            [-616.206, -2.696, -287.546],
        );
        let continuation = method29(
            7,
            45509301,
            [-51.16804, 24.38959, -49.91319],
            [-668.906, -87.832, 85.235],
        );
        let packets: Vec<(u32, f32, &[u8])> =
            vec![(8, 110.26195, &first), (8, 110.35225, &continuation)];

        let (launches, short) = collect_launches(&packets, |_| true);
        assert!(short.is_empty());
        // 收集层保留全部段（不去重）：同一发的续段要留给组装层作落点用
        assert_eq!(launches.len(), 2, "收集层不去重（续段由组装层折叠）");
        assert_eq!(launches[0].shot_id, 45509301);
        assert!((launches[0].t - 110.26195).abs() < 1e-5);
        assert_eq!(launches[0].point, [99.82513, 25.24507, 20.54626]);
        assert_eq!(launches[0].vel, [-616.206, -2.696, -287.546]);
        // 组装层折叠：射击数仍为 1，链首 = 发射段，续段首点 = 落点（画到这里为止）
        let (is_cont, segs) = shot_segments(&launches);
        assert_eq!(
            is_cont,
            vec![false, true],
            "同一 (shooter, shotId) 只出一次射击"
        );
        let got = segs.get(&(7, 45509301)).cloned().unwrap_or_default();
        assert_eq!(got.len(), 1, "续段按序全部保留（折线用）");
        assert_eq!(got[0].0, 110.35225);
        assert_eq!(got[0].1, [-51.16804, 24.38959, -49.91319]);
    }
}

#[cfg(test)]
mod shot_segments_tests {
    use super::*;

    fn l(t: f32, shooter: u32, shot_id: u32, point: [f32; 3]) -> LaunchEntry {
        LaunchEntry {
            t,
            shooter,
            shot_id,
            point,
            vel: [0.0, 0.0, -680.0],
            shooter_prop2: None,
        }
    }

    /// 同 (shooter, shotId) 的第二条 = 续段（跳弹/穿透），且续段首点被登记为该发的落点；
    /// 不同射手/不同 shotId → 各自独立，互不影响
    #[test]
    fn continuation_is_folded_into_its_own_shot() {
        let launches = vec![
            l(71.219, 11, 48267266, [109.65, 32.33, 135.39]), // 发射段
            l(71.219, 11, 48267266, [131.92, 30.58, 72.50]),  // 跳弹续段（原点=受击方装甲处）
            l(71.5, 11, 48267267, [100.0, 30.0, 150.0]),      // 同射手另一发（无续段）
            l(71.6, 12, 48267266, [10.0, 20.0, 10.0]),        // 另一射手（不同链）
        ];
        let (is_cont, segs) = shot_segments(&launches);
        assert_eq!(is_cont, vec![false, true, false, false]);
        assert_eq!(
            segs.get(&(11, 48267266)).cloned(),
            Some(vec![(71.219, [131.92, 30.58, 72.50], [0.0, 0.0, -680.0])])
        );
        assert!(!segs.contains_key(&(11, 48267267)));
        assert!(!segs.contains_key(&(12, 48267266)));
    }

    /// 三段链：续段**全部按序保留**（折线要一路画到服务器终点）
    #[test]
    fn all_continuations_are_kept_in_order() {
        let launches = vec![
            l(10.0, 5, 7, [0.0, 0.0, 0.0]),
            l(10.1, 5, 7, [10.0, 1.0, 0.0]),
            l(10.3, 5, 7, [30.0, 5.0, 0.0]),
        ];
        let (is_cont, segs) = shot_segments(&launches);
        assert_eq!(is_cont, vec![false, true, true]);
        let got = segs.get(&(5, 7)).cloned().unwrap_or_default();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].1, [10.0, 1.0, 0.0]);
        assert_eq!(got[1].1, [30.0, 5.0, 0.0]);
    }

    /// 折线几何：段时长按 |Δ|/段速度；via 与 leg_secs 满足 len(legs) = len(via)+1
    #[test]
    fn legs_use_per_segment_speed() {
        let chain = vec![
            (1.0f32, [100.0, 0.0, 0.0], [0.0, 0.0, -500.0]),
            (2.0f32, [100.0, 0.0, -300.0], [0.0, 0.0, -250.0]),
        ];
        let (via, legs) = shot_legs(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, -1000.0],
            Some(&chain),
            [100.0, 0.0, -800.0],
        );
        assert_eq!(via, vec![[100.0, 0.0, 0.0], [100.0, 0.0, -300.0]]);
        assert_eq!(legs.len(), 3);
        assert!((legs[0] - 0.1).abs() < 1e-6, "段0: 100m / 1000mps");
        assert!((legs[1] - 0.6).abs() < 1e-6, "段1: 300m / 500mps");
        assert!(
            (legs[2] - 2.0).abs() < 1e-6,
            "段2: 500m / 250mps（末段用最后续段速度）"
        );
    }

    /// 无续段（普通弹）→ 表内无键、折线退化为单段直线
    #[test]
    fn plain_shot_has_no_continuation() {
        let launches = vec![l(10.0, 5, 7, [0.0, 0.0, 0.0])];
        let (is_cont, segs) = shot_segments(&launches);
        assert_eq!(is_cont, vec![false]);
        assert!(segs.is_empty());
        let (via, legs) = shot_legs([0.0, 0.0, 0.0], [0.0, 0.0, -680.0], None, [0.0, 0.0, -68.0]);
        assert!(via.is_empty());
        assert_eq!(legs.len(), 1);
        assert!((legs[0] - 0.1).abs() < 1e-3);
    }
}
