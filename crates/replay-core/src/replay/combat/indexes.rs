//! per-entity 索引（type=10 位姿 / type=7 prop2 打包角）与 prop2 时间线求值
//! （客户端 0x1440C70 语义；序列按 clock 排序后二分）。

use super::*;
use std::collections::HashMap;

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

/// type=10 状态采样唯一解码器（SSOT）：载荷 ≥48B 时解出位置/姿态/误差盒。
///
/// 解码域与《回放与射击逆向总集》§2.1 一致：eid@0、parent@8、pos@12、
/// pos_error@24、yaw/pitch/roll@36/40/44。**不做** parent==0 世界系过滤——
/// 调用方按语义选择（渲染索引保留全部流；author 模块只取世界系）。
pub(crate) fn decode_st10_sample(clock: f32, p: &[u8]) -> Option<St10Sample> {
    if p.len() < 48 {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
    Some(St10Sample {
        clock,
        pos: [f(12), f(16), f(20)],
        yaw: f(36),
        pitch: f(40),
        roll: f(44),
        pos_error: [f(24), f(28), f(32)],
    })
}

/// per-entity type=10 状态采样流（时钟序）
pub(crate) type St10Index = HashMap<u32, Vec<St10Sample>>;
/// per-entity prop2 打包角流（(clock, 相对角 rad, frac6)，时钟序）
pub(crate) type Prop2Index = HashMap<u32, Vec<(f32, f32, u16)>>;

/// per-entity 索引（作者/他人路径共用）：type=10 状态采样 + type=7 prop2 打包角。
/// prop2 u16 = (炮塔偏航 coarse10 << 6) | 炮管俯仰比例 frac6（T110 1617 受控实验 +
/// J39 实战复核，《回放与射击逆向总集》第一篇 §2.2）：偏航只取高 10 位（低 6 位是俯仰，混入会引入
/// ±0.3° 假跳变），frac 保留供俯仰解码。各实体 Vec 保持包序（未排序）；作者路径
/// 用前需按 clock 排序（锚点选择依赖时序），他人路径沿用包序。
pub(crate) fn build_entity_indexes(packets: &[(u32, f32, &[u8])]) -> (St10Index, Prop2Index) {
    let mut st10: HashMap<u32, Vec<St10Sample>> = HashMap::new();
    let mut prop2: HashMap<u32, Vec<(f32, f32, u16)>> = HashMap::new();
    for (t2, clock, p) in packets {
        if *t2 == 10 {
            if let Some(sample) = decode_st10_sample(*clock, p) {
                st10.entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
                    .or_default()
                    .push(sample);
            }
        }
        if *t2 == 7 && p.len() >= 14 && u32::from_le_bytes([p[4], p[5], p[6], p[7]]) == 2 {
            let v = u16::from_le_bytes([p[12], p[13]]);
            let rel = (v >> 6) as f32 / 1024.0 * std::f32::consts::TAU - std::f32::consts::PI;
            let rel = if rel > std::f32::consts::PI {
                rel - std::f32::consts::TAU
            } else {
                rel
            };
            prop2
                .entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
                .or_default()
                .push((*clock, rel, v & 63));
        }
        // type=5 class=2 车辆实体创建 / 重入物化快照：前缀属性 blob 的 id2（u16 @48）
        // 是物化时刻的 prop2 **当前值**（《回放与射击逆向总集》§2.6：初始属性 blob 含
        // prop2 初值；全样本探针：重入快照与紧随的 type=7 采样同域，coarse 差中位 0、
        // 多数 ≤2 级）。炮塔全程不转的车没有 type=7 prop2 流，缺这条种子会让严格路径
        // 误判「目标炮塔朝向缺失」而整场 fail（2026-10-28 训练房样本）。
        // 布局护栏（fail-closed）：class=2、id1@45=1、id2@47=2、id3@50=3；任一不满足
        // 即不采信（不猜偏移）。
        if *t2 == 5
            && p.len() >= 60
            && u16::from_le_bytes([p[4], p[5]]) == 2
            && p[45] == 1
            && p[47] == 2
            && p[50] == 3
        {
            let v = u16::from_le_bytes([p[48], p[49]]);
            let (rel, frac) = decode_prop2_u16(v);
            prop2
                .entry(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
                .or_default()
                .push((*clock, rel, frac as u16));
        }
    }
    (st10, prop2)
}

/// prop2 原始 u16 → (炮塔相对偏航 rad, frac6)。与 [`build_entity_indexes`] 同一解码
/// （高 10 位 coarse = 偏航、低 6 位 = 俯仰比例）。供流序快照
/// （[`DirectHit8::victim_prop2`] / [`LaunchEntry::shooter_prop2`]）消费。
pub(crate) fn decode_prop2_u16(v: u16) -> (f32, f32) {
    let rel = (v >> 6) as f32 / 1024.0 * std::f32::consts::TAU - std::f32::consts::PI;
    let rel = if rel > std::f32::consts::PI {
        rel - std::f32::consts::TAU
    } else {
        rel
    };
    (rel, (v & 63) as f32)
}

/// prop2 时间线 → **客户端语义密集采样**（0.1s 网格，与 render_timeline 同惯例）：
/// 每个网格点用 [`prop2_at`]（到达约束/短弧插值/末帧保持）求值，
/// 保证滑块 dt=0 与初始摆放严格一致。替代原始采样窗口直通——前端 lerpTl 在稀疏
/// 原始采样上做双侧插值会把查询点之后才到达的包混进来（命中后反应污染），与
/// 0x1440C70 只用已到达帧的语义不符。
pub(crate) fn timeline_prop2_client(
    series: Option<&Vec<(f32, f32, u16)>>,
    base: f32,
    from: f32,
    to: f32,
    limits: Option<&GunPitchRange>,
) -> Vec<(f32, f32)> {
    if series.is_none() {
        return Vec::new();
    }
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
/// - q 超出末关键帧（无 (q, t] 内到达）：**保持末帧**——反汇编复核（D2 翻案，
///   2026-10）：0x1440C70 越末帧分支就是硬保持，**不存在外推分支**；常量 0.9 是
///   稀疏 bracket 的跨度阈值因子（相邻关键帧间隔 > 0.9×更新间隔时按跨度收缩），
///   历史上误读为"限步外推系数"曾致静止段炮塔角错 72°（commit 2058c96 改 clamp）。
///   离线任意时刻查询 = 保持末帧，无任何前向外推；
/// - q 早于首帧：保持首帧；t 前完全无采样（AoI 新进）回退双侧最近初值包。
///
/// 被击时刻姿态不被命中后反应包污染的保证：反应首包到达 > t，永远不进任何括号。
/// 注意：此为**渲染层**（滑块时间线）语义；判定锚点（炮塔朝向/俯仰取样）用
/// [`prop2_at_arrived`](...)（最后到达采样，与 WI turret_yaw 同域）。
pub(crate) fn prop2_at(series: Option<&Vec<(f32, f32, u16)>>, t: f32) -> Option<(f32, f32)> {
    let list = series?;
    // 序列已按 clock 稳定排序（build_shot_scan_shared 统一排序；协议包序=时钟序，排序后
    // 语义不变）；n = 已到达帧数，m = 到达 ≤ q 的帧数——二分取代线性计数（网格构建
    // 每场 ~8 万次调用 × 数千采样的主导热点）
    let n = list.partition_point(|(c, _, _)| *c <= t);
    if n == 0 {
        // AoI 边界：t 前无采样，回退最近初值包
        let (_c, y, fr) = list.iter().min_by(|a, b| {
            (a.0 - t)
                .abs()
                .partial_cmp(&(b.0 - t).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
        return Some((*y, *fr as f32));
    }
    let q = t - 0.1;
    let m = list.partition_point(|(c, _, _)| *c <= q);
    let lerp =
        |(_ca, ya, fa): (f32, f32, u16), (_cb, yb, fb): (f32, f32, u16), f: f32| -> (f32, f32) {
            let mut dy = yb - ya;
            while dy > std::f32::consts::PI {
                dy -= std::f32::consts::TAU;
            }
            while dy < -std::f32::consts::PI {
                dy += std::f32::consts::TAU;
            }
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
    // q 超出末关键帧：保持末帧（0x1440C70 越末帧分支 = 硬保持，无外推分支；
    // 0.9 是稀疏 bracket 跨度阈值因子，非外推系数——D2 翻案，见上方离线等价注）。
    let (_, y, fr) = list[n - 1];
    Some((y, fr as f32))
}

/// 判定锚点采样（受击方/射手炮塔朝向与炮管俯仰取样，与 WI turret_yaw 同域）：
/// 取 t 时刻**最后已到达**采样，不做客户端渲染滞后的 q 插值——命中
/// 判定用服务器广播的最新已知值；t 前无采样（AoI 新进）回退最近初值包。
pub(crate) fn prop2_at_arrived(
    series: Option<&Vec<(f32, f32, u16)>>,
    t: f32,
) -> Option<(f32, f32)> {
    let list = series?;
    let n = list.partition_point(|(c, _, _)| *c <= t);
    if n > 0 {
        let (_, y, fr) = list[n - 1];
        return Some((y, fr as f32));
    }
    let (_c, y, fr) = list.iter().min_by(|a, b| {
        (a.0 - t)
            .abs()
            .partial_cmp(&(b.0 - t).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    Some((*y, *fr as f32))
}

/// prop2 采样新鲜度（仅区分"流陈旧"与"值稳定"）：最后已到达包之后 prop2 是否停止
/// 发送超过 2s。**frac 不变 ≠ 冻结**——prop2 变化驱动，炮管停在极限/定点时 frac 恒定
/// 是物理事实的如实上报（2056/1617 实验实证：快速偏航段 frac 钉极限 = 炮管贴极限的
/// 正常操作常态，非通道冻结；用户实战确认）。仅当整条流断流（AoI 边界/补发簇）时
/// 采样才可能陈旧，用于 quality.pitch_frozen 提示。
pub(crate) fn prop2_frac_frozen(series: Option<&Vec<(f32, f32, u16)>>, t: f32) -> bool {
    let Some(list) = series else { return false };
    match list.partition_point(|(c, _, _)| *c <= t) {
        0 => false,
        idx => t - list[idx - 1].0 > 2.0,
    }
}

#[cfg(test)]
mod prop2_seed_tests {
    use super::*;

    /// type=5 class=2 车辆实体包（前缀属性 blob：id1@45、id2=prop2@47..50、id3@50..53）。
    fn type5_vehicle(eid: u32, blob: u16) -> Vec<u8> {
        let mut p = vec![0u8; 60];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[4..6].copy_from_slice(&2u16.to_le_bytes());
        p[45] = 1;
        p[46] = 1;
        p[47] = 2;
        p[48..50].copy_from_slice(&blob.to_le_bytes());
        p[50] = 3;
        p[51..53].copy_from_slice(&2573u16.to_le_bytes());
        p[53] = 4;
        p[56] = 5;
        p[57] = 0; // 昵称长度 0：种子不依赖昵称域
        p
    }

    #[test]
    fn type5_create_seeds_prop2_initial_value() {
        // 目标全程不转炮塔 → 没有任何 type=7 prop2；创建包 blob 的 id2 是唯一初值证据
        let pkt = type5_vehicle(0x14cc6d, 0x7ef5);
        let refs: Vec<(u32, f32, &[u8])> = vec![(5, 44.19, pkt.as_slice())];
        let (_, prop2) = build_entity_indexes(&refs);
        let series = prop2.get(&0x14cc6d).expect("创建包应种下 prop2");
        assert_eq!(series.len(), 1);
        assert!((series[0].0 - 44.19).abs() < 1e-6);
        let (rel, frac) = decode_prop2_u16(0x7ef5);
        assert!((series[0].1 - rel).abs() < 1e-6);
        assert_eq!(series[0].2, frac as u16);
        assert!((rel - (-0.0307)).abs() < 1e-3, "rel={rel}");
        assert_eq!(frac as u16, 53);
    }

    #[test]
    fn type5_seed_requires_vehicle_layout() {
        // class 非 2（区域实体）：不采信
        let mut pkt = type5_vehicle(7, 0x7ef5);
        pkt[4..6].copy_from_slice(&3u16.to_le_bytes());
        let refs: Vec<(u32, f32, &[u8])> = vec![(5, 1.0, pkt.as_slice())];
        assert!(build_entity_indexes(&refs).1.is_empty());

        // 属性表 id 标记不符（布局变化）：不猜偏移
        let mut pkt = type5_vehicle(8, 0x7ef5);
        pkt[47] = 9;
        let refs: Vec<(u32, f32, &[u8])> = vec![(5, 1.0, pkt.as_slice())];
        assert!(build_entity_indexes(&refs).1.is_empty());
    }

    #[test]
    fn type7_prop2_still_indexed() {
        let mut pkt = vec![0u8; 14];
        pkt[0..4].copy_from_slice(&9u32.to_le_bytes());
        pkt[4..8].copy_from_slice(&2u32.to_le_bytes());
        pkt[12..14].copy_from_slice(&0x8000u16.to_le_bytes());
        let refs: Vec<(u32, f32, &[u8])> = vec![(7, 12.0, pkt.as_slice())];
        let (_, prop2) = build_entity_indexes(&refs);
        assert_eq!(prop2.get(&9).map(Vec::len), Some(1));
        assert!(prop2[&9][0].1.abs() < 1e-6, "0x8000 = 相对 0°");
    }
}
