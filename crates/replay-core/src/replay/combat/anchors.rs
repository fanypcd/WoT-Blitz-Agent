//! 判定锚点选择（客户端位置处理仿真）与渲染层滤波时间线
//! （AvatarFilter 输出的锚点/降采样；《回放与射击逆向总集》第二篇 §4.1 / 第三篇 §五）。

use super::*;
use crate::replay::filter::SegmentedPoseTimeline;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 客户端渲染层锚点（《回放与射击逆向总集》第二篇 §4.1 / 第三篇 §五）：客户端位置滤波器（WGVehicleFilter2 核心 = OSS AvatarFilterHelper 移植，
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
    /// 锚点时刻所在帧的输出 = 游戏画面里模型实际呈现的位姿（渲染层锚点，实现 = replay/filter.rs；客户端机制见《回放与射击逆向总集》第二篇 §4.1 / 第三篇 §五；roll 按原始 volatile 插值）。
    /// pos 相对锚点同上；仅 viewer 下拉展示用，不参与判定。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub render: bool,
}

impl TickSample {
    /// 原始 type=10 采样
    pub(crate) fn raw(dt: f32, tick: f32, pos: [f32; 3], yaw: f32, pitch: f32, roll: f32) -> Self {
        Self {
            dt,
            tick,
            pos,
            yaw,
            pitch,
            roll,
            render: false,
        }
    }
    /// 合成的渲染层采样（dt=锚点帧偏移；tick=NaN → viewer 标签走渲染位分支）。
    /// roll = 原始 volatile 插值（滤波层不输出侧倾），保证与滑块时间线姿态一致。
    pub(crate) fn render_ghost(dt: f32, pos: [f32; 3], yaw: f32, pitch: f32, roll: f32) -> Self {
        Self {
            dt,
            tick: f32::NAN,
            pos,
            yaw,
            pitch,
            roll,
            render: true,
        }
    }
}

/// 锚点状态选择 —— 客户端位置处理仿真（用户决策；逆向文档 6.3/6.4）：
/// 复现位置 = 游戏客户端渲染位置（volatile 流 + 移动滤波重建），而非最近原始包。
/// ① 通道边界（相邻采样速度 >25 m/s = AoI 切换跳变，或其间有补发簇时钟；客户端重置滤波）禁止跨段插值；
/// ② 段内：位置线性插值（yaw 最短弧，pitch/roll 近邻）；
/// ③ 段末：末段速度外推 ≤0.5s 超限退回最近包；
/// ④ t 早于首包/单采样 → 最近包。
/// 返回 (pos, ang, 带符号 clock 偏移, 来源 "filtered"/"extrapolated"/"nearest")。
pub(crate) fn select_anchor_state(
    samples: &[St10Sample],
    clusters: &HashMap<u32, Vec<f32>>,
    eid: u32,
    t: f32,
) -> Option<([f32; 3], [f32; 3], f32, &'static str)> {
    if samples.is_empty() {
        return None;
    }
    if samples.len() == 1 {
        let s = &samples[0];
        return Some((s.pos, [s.yaw, s.pitch, s.roll], s.clock - t, "nearest"));
    }

    const TELEPORT_SPEED: f32 = 25.0; // m/s，WoTB 最高车速 ~19 m/s + 余量；超限 = AoI 通道切换跳变
    let is_boundary = |i: usize| -> bool {
        let a = &samples[i];
        let b = &samples[i + 1];
        let dt = b.clock - a.clock;
        if dt <= 0.0 {
            return true;
        }
        let d = dist3(a.pos, b.pos);
        if d / dt > TELEPORT_SPEED {
            return true;
        }
        if let Some(v) = clusters.get(&eid) {
            if v.iter().any(|c| *c > a.clock && *c < b.clock) {
                return true;
            }
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
            let pos = [
                b.pos[0] + v[0] * dt,
                b.pos[1] + v[1] * dt,
                b.pos[2] + v[2] * dt,
            ];
            return Some((pos, ang_of(b), -dt, "extrapolated"));
        }
        return Some((pos_of(b), ang_of(b), b.clock - t, "nearest"));
    }

    // 段定位用二分（samples 已按 clock 升序）：此前的线性步进在锚点密集场是逐发 O(n)；
    // 到达此处时 t ≤ 末采样时钟（越界/extrapolated 分支已在上方 return），i+1 必在界内
    let i = samples.partition_point(|s| s.clock < t).saturating_sub(1);
    let a = &samples[i];
    let b = &samples[i + 1];
    if is_boundary(i) {
        // 跨边界：不可插值（客户端在边界处跳变/重置），取时间近者
        let s = if (t - a.clock).abs() <= (b.clock - t).abs() {
            a
        } else {
            b
        };
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
    if dyaw > std::f32::consts::PI {
        dyaw -= std::f32::consts::TAU;
    }
    if dyaw < -std::f32::consts::PI {
        dyaw += std::f32::consts::TAU;
    }
    let ang = [
        a.yaw + dyaw * f,
        a.pitch + (b.pitch - a.pitch) * f,
        a.roll + (b.roll - a.roll) * f,
    ];
    Some((pos, ang, 0.0, "filtered"))
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

// =====================================================================
//  UpdateArena 竞技场状态流（wotblitz.exe 二进制实证，见《回放与射击逆向总集》第二篇 §4.5）：
//  Avatar 方法 method=48 (0x30)，args[0] = 子类型 ID（1..=27，名字表 @VA 0x40391D8，
//  首字节 dec 后查表跳转），其后为 WG protobuf-lite 消息体（0xFF 转义 u24 字段号扩展）。
// =====================================================================

/// 渲染层锚点（客户端位置滤波器，《回放与射击逆向总集》第二篇 §4.1 / 第三篇 §五）：per-entity 滤波器时间线惰性构建 + 事件时刻所在帧的滤波输出。
/// `at_or_after=true` 语义 = 包在该帧网络泵处理后于当帧渲染（游戏客户端帧循环次序）。
/// 时间线 per-entity 只建一次（60Hz × 战斗时长，每实体 ~1MB）。
/// 段化（D1）：按 AoI 在场段独立滤波 + Type5 快照段首种子（`presence` = 全场段列表，
/// 按 eid 过滤）——重入后炮口锚点从真实物化位置开始，不再从上一段末位滑移追赶。
pub(crate) fn render_anchor(
    cache: &mut HashMap<u32, SegmentedPoseTimeline>,
    eid: u32,
    samples: Option<&Vec<St10Sample>>,
    presence: &[AoiPresence],
    t: f32,
    judgment_pos: [f32; 3],
) -> Option<RenderAnchorData> {
    let samples = samples?;
    if samples.is_empty() {
        return None;
    }
    if let std::collections::hash_map::Entry::Vacant(e) = cache.entry(eid) {
        let pres: Vec<AoiPresence> = presence.iter().filter(|p| p.eid == eid).cloned().collect();
        e.insert(SegmentedPoseTimeline::build(samples, &pres)?);
    }
    let tl = cache.get(&eid)?;
    let pose = tl.pose_at(t as f64, true)?;
    // roll 滤波层不输出（视觉侧倾来自物理层），按原始 volatile 线性插值补齐——
    // 与 render_timeline 同规则。恒 0 会使加载摆放（渲染位采样）与滑块时间线的
    // 车体侧倾不一致，炮塔偏航下炮管仰角漂移 ~5.6°。
    let mut ang = pose.ang;
    ang[2] = roll_at_sample(samples, t as f64);
    Some(RenderAnchorData {
        pos: pose.pos,
        ang,
        latency: pose.latency,
        dist_to_judgment: dist3(pose.pos, judgment_pos),
    })
}

/// 原始 volatile 侧倾在 t 时刻的线性插值（roll 不经滤波层，视觉侧倾来自物理层；两端共用）。
/// samples 已按 clock 升序（st10 收集器保证），`partition_point` 二分定位首个 clock ≥ t 的
/// 采样——[`render_timeline`] 逐 0.1s 网格点调用本函数（≈80k 次量级热路径），线性扫描是
/// 纯浪费。t 早于首采样/晚于末采样 → 端点 roll 保持（与原线性扫描语义逐位一致）。
fn roll_at_sample(samples: &[St10Sample], t: f64) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let idx = samples.partition_point(|s| (s.clock as f64) < t);
    if idx == 0 {
        return samples[0].roll;
    }
    if idx >= samples.len() {
        return samples[samples.len() - 1].roll;
    }
    let prev = &samples[idx - 1];
    let s = &samples[idx];
    let span = s.clock as f64 - prev.clock as f64;
    let f = if span > 1e-6 {
        ((t - prev.clock as f64) / span) as f32
    } else {
        0.0
    };
    prev.roll + (s.roll - prev.roll) * f
}

/// 滤波时间线降采样：[base+dt_from, base+dt_to] 步长 step 的滤波器输出（渲染层严格对齐，
/// 滑块用）。pos 为绝对世界坐标；roll 按原始 volatile 插值。时间线 per-entity 惰性构建并缓存。
/// 段化（D1）：在场段内才输出（隐藏段 = 客户端无该实体数据，跳过采样）。
#[allow(clippy::too_many_arguments)] // 窗口语义四参 + 身份三参，聚合结构反而不透明
pub(crate) fn render_timeline(
    cache: &mut HashMap<u32, SegmentedPoseTimeline>,
    eid: u32,
    samples: Option<&Vec<St10Sample>>,
    presence: &[AoiPresence],
    base_t: f32,
    dt_from: f32,
    dt_to: f32,
    step: f32,
) -> Vec<RenderTimelineSample> {
    let Some(samples) = samples.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    if let std::collections::hash_map::Entry::Vacant(e) = cache.entry(eid) {
        let pres: Vec<AoiPresence> = presence.iter().filter(|p| p.eid == eid).cloned().collect();
        match SegmentedPoseTimeline::build(samples, &pres) {
            Some(tl) => {
                e.insert(tl);
            }
            None => return Vec::new(),
        }
    }
    let Some(tl) = cache.get(&eid) else {
        return Vec::new();
    };
    // roll 不经滤波层（视觉侧倾来自物理层），但原始 volatile 携带车体/地形侧倾，
    // 置 0 会丢姿态（T110 1436 shot4 实证：13° roll 使 1.95m 高的炮闩枢轴横移
    // 0.44m，炮闩标注与 launchpoint 永不对齐）——按原始采样线性插值补齐。
    let roll_at = |t: f64| -> f32 { roll_at_sample(samples, t) };
    // 只输出在场段内的采样（AoI 外 = 客户端尚无/已无该实体数据，不渲染，交给前端隐藏模型）
    let mut out = Vec::new();
    let mut dt = dt_from;
    while dt <= dt_to + 1e-6 {
        let t = (base_t + dt) as f64;
        if tl.contains(t) {
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

/// 标量时间线降采样：[base+from, base+to] 内的采样 → (dt, 值)（prop2 炮塔角 / method36 field2 俯仰用）
pub(crate) fn timeline_1f(
    series: Option<&Vec<(f32, f32)>>,
    base: f32,
    from: f32,
    to: f32,
) -> Vec<(f32, f32)> {
    let Some(list) = series else {
        return Vec::new();
    };
    list.iter()
        .filter(|(c, _)| *c >= base + from && *c <= base + to)
        .map(|(c, v)| (c - base, *v))
        .collect()
}

/// tick 采样统一截断（逆向文档 6.2/6.3）：有真锚点（|dt|<0.05）→ 只保留锚点及之前（其后包属新通道）；
/// 无锚点但有补发簇签名 → 保留簇时钟之前（原 +0.09 兜底包可能已属新通道，按簇时钟收紧）；
/// 无锚点无签名 → 保持原行为（≤0.09 兜底包近似锚点）。
pub(crate) fn trim_tick_samples(samples: &mut Vec<TickSample>, cluster_dt: Option<f32>) {
    let has_anchor = samples.iter().any(|s| s.dt.abs() < 0.05);
    if !has_anchor && cluster_dt.is_none() {
        return;
    }
    let mut cut = if has_anchor { 0.05f32 } else { 0.09 };
    if let Some(cd) = cluster_dt {
        cut = cut.min((cd - 0.02).max(0.0));
    }
    samples.retain(|s| s.dt < cut);
    samples.sort_by(|a, b| a.dt.partial_cmp(&b.dt).unwrap());
}

// 曾试过对相邻快照段做运动学检验（倒车 ≤5.5 m/s、侧移 ≤5.0、前向 ≤30、加速度 ≤8 m/s²，
// 位移 <0.25 m 微跳豁免），截除"服务器纠偏倒车滑移"前缀——已否决：会把真实倒车（轻坦
// 倒车极速可超 5.5 m/s）整段误杀导致 tick 切换丢失。项目约定完全按回放原始数据渲染，
// 不做任何运动学合理性过滤。
