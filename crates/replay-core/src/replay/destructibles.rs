//! AreaDestructibles 区域可破坏物事件流（type=32 短广播、envelope = 区域实体）。
//!
//! **物体寻址公式（已闭合，2026-10-05 四场回放 799/811 命中 = 98.5%）**：
//! `[byte0: flag(1)+prop(2)+序号][byte1..n-1: 计数器/串号 varint][末字节: slot]`，
//! 携带事件 envelope（区域实体 = 100m 格子锚点）：
//! **物体 = lka 逆表[(floor(x/100)+0x7F, floor(z/100)+0x7F, slot)] → 场景实体 id**
//! （`blitz/<map>.lka`：KeyedArchive，key=场景实体 id，value=(cellX,cellY,slot)）。
//! 与客户端反汇编机制吻合：事件元素 → 记录+0x78 = u32 (cellX,cellY,slot) →
//! FNV-1a 哈希表（LoadModelsMap 按 lka 填充）→ 场景节点 → 状态切换。
//! 验证：受控实验 10/10（作者碾压帐篷/树/沙袋 + HE 射树，槽位↔物体逐一命中）、
//! R32 8/8、J39 713/713、GB48 58/70（miss 均为区域锚点贴格边界的查找失配，
//! 消费端可用锚点格 ±1 邻域兜底）。**坐标为直接对应（replay x,z ↔ scene x,y），
//! 无镜像**（旧「镜像」结论系污染样本的伪象，已撤回）。
//!
//! **倒向（树，prop=3）**：body 倒数第二字节 = 8 位倒向角（服务器权威；
//! 反向平行对照对 164.1° 验证；dir ≈ 碾压车速方向 + 180°）——未点亮碾压者
//! 亦携带，客户端无需车辆姿态即可复原倒向。
//! prop ↔ 类别（受控实验 10/10 + 跨图一致）：1=fragiles（帐篷/沙袋/栅栏/箱子）、
//! 3=树倒（SpeedTree；erlenberg 观测含灯笼类 fallingAtom）、2=柱状物（城市图）、
//! 0=destroyedModules（未观测）。分派器 0x15C4600 读取 [1 bit][bitlen(4)=2 bit]，
//! >3 断言 "Invalid slice property change"。
//!
//! 实测基础（2026-10-05 受控实验 R132_T100LT@lagoon ×2 + J39/GB48 被动样本）：
//! 每图开局创建 51B 定长 type=5 区域实体（class u16=3），位置 = 100m 格子锚点；
//! 事件与车辆 AoI 解耦（未点亮车辆的碾压照常广播）。type=32 线格式全族统一：
//! `[eid u32][flag u8][bodyLen u32][body]`；flag=0 = 消耗品族（collect.rs）。
//!
//! fail-closed：envelope 不在区域实体集合内的一律不收；bodyLen ∉ [2,7] 丢弃。

use serde::Serialize;

/// 区域实体（100m 格子锚点）。位置取 type=5 创建包世界坐标（y 恒 0，不采集）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DestructibleArea {
    pub eid: u32,
    pub x: f32,
    pub z: f32,
}

/// 单条可破坏物事件。物体寻址 = 区域实体格子 × [`Self::slot`] 查 lka 逆表
/// （公式见模块注释）。`args` 原样透传（头 3 位 = [flag][prop]，中段 = 计数器/
/// 串号 varint，末字节 = slot——即 `Self::slot`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AreaDestructibleEvent {
    pub clock: f32,
    pub area_eid: u32,
    /// 类别（byte0 bits 6-5）：1=fragiles、2=柱状物、3=树倒、0=modules（未观测）
    pub prop: u8,
    /// lka 槽位（body 末字节）——与区域实体格子联表唯一定位场景物体
    pub slot: u8,
    /// 倒向/冲量角（body 倒数第二字节，8 位角，LSB=1.40625°）。
    /// **树倒（prop=3）实测为服务器权威倒向**（受控实验 2026-10-05 终版：
    /// 反向平行对照对相差 164.1° 与观察吻合；移动碾压样本 dir ≈ 车速方向
    /// +180°，即指向冲量来源）——未点亮碾压者也携带，客户端据此复原倒向。
    /// fragiles（prop=1）同位置字节亦变化（疑碎片冲量向），语义未单独标定。
    pub fall_dir: u8,
    /// type=32 的 bodyLen（3..=7 观测于区域实体；即文档旧称的「method」）
    pub body_len: u8,
    pub args: Vec<u8>,
}

/// type=5 class=3 → 区域实体集合（eid → 格子锚点）。
pub fn collect_destructible_areas(packets: &[(u32, f32, &[u8])]) -> Vec<DestructibleArea> {
    let mut out: Vec<DestructibleArea> = Vec::new();
    for (_, _, p) in packets {
        // 头部 [eid u32][class u16=3][spaceId u32]…，位置 f32 @14/@22（探针 51B 全满足）
        if p.len() < 26 || u16::from_le_bytes([p[4], p[5]]) != 3 {
            continue;
        }
        let f = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
        out.push(DestructibleArea {
            eid: u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
            x: f(14),
            z: f(22),
        });
    }
    out.sort_by_key(|a| a.eid);
    out
}

/// 区域实体上的 type=32 短广播 → 可破坏物事件（时钟升序）。
/// `areas` 来自 [`collect_destructible_areas`]；空集合直接返回空（无区域实体 =
/// 无可破坏物流，如 karieri 开局片段）。
pub fn collect_area_destructible_events(
    packets: &[(u32, f32, &[u8])],
    areas: &[DestructibleArea],
) -> Vec<AreaDestructibleEvent> {
    if areas.is_empty() {
        return Vec::new();
    }
    let is_area: std::collections::HashSet<u32> = areas.iter().map(|a| a.eid).collect();
    let mut out: Vec<AreaDestructibleEvent> = Vec::new();
    for (_, clock, p) in packets {
        let Some(eid) = area_envelope(p, &is_area) else {
            continue;
        };
        let body_len = u32::from_le_bytes([p[5], p[6], p[7], p[8]]) as usize;
        if !(2..=7).contains(&body_len) || 9 + body_len != p.len() {
            continue;
        } // 区域实体观测域 2..=7；断言失败 = 结构漂移，丢弃不猜
        let args = p[9..].to_vec();
        let prop = (args[0] >> 5) & 0b11;
        let slot = args[args.len() - 1];
        let fall_dir = args[args.len() - 2];
        out.push(AreaDestructibleEvent {
            clock: *clock,
            area_eid: eid,
            prop,
            slot,
            fall_dir,
            body_len: body_len as u8,
            args,
        });
    }
    out.sort_by(|a, b| {
        a.clock
            .partial_cmp(&b.clock)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// flag=1 短广播且 envelope ∈ 区域实体集合 → Some(eid)。
fn area_envelope(p: &[u8], is_area: &std::collections::HashSet<u32>) -> Option<u32> {
    if p.len() < 11 || p[4] != 0x01 {
        return None;
    }
    let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
    is_area.get(&eid).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area_create(eid: u32, x: f32, z: f32) -> Vec<u8> {
        // [eid 0..4][class=3 4..6][spaceId 6..10][0000 10..14][x 14..18][y 18..22][z 22..26]
        // （探针实测布局：51B 创建包的前 26 字节）
        let mut p = Vec::new();
        p.extend_from_slice(&eid.to_le_bytes());
        p.extend_from_slice(&3u16.to_le_bytes());
        p.extend_from_slice(&0x040bu32.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&x.to_le_bytes());
        p.extend_from_slice(&0f32.to_le_bytes());
        p.extend_from_slice(&z.to_le_bytes());
        p
    }

    fn event32(eid: u32, body: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&eid.to_le_bytes());
        p.push(0x01);
        p.extend_from_slice(&(body.len() as u32).to_le_bytes());
        p.extend_from_slice(body);
        p
    }

    #[test]
    fn collects_class3_areas_only() {
        let p1 = area_create(100, 154.28, 153.64);
        let mut p2 = area_create(200, 1.0, 2.0);
        p2[4] = 2; // 车辆 class=2
        let packets: Vec<(u32, f32, &[u8])> = vec![(5, 0.5, &p1), (5, 0.6, &p2)];
        let areas = collect_destructible_areas(&packets);
        assert_eq!(
            areas,
            vec![DestructibleArea {
                eid: 100,
                x: 154.28,
                z: 153.64
            }]
        );
    }

    #[test]
    fn parses_len5_and_len6_ids_and_drops_non_area_envelopes() {
        let p_area = area_create(100, 0.0, 0.0);
        let e1 = event32(100, &[0xa0, 0x71, 0x7f, 0x2f, 0x26]); // R132 t=18.45 实测字节
        let e2 = event32(100, &[0xa9, 0x00, 0x40, 0x7e, 0xbd, 0x2b]); // bodyLen=6
        let e_veh = event32(999, &[0xa0, 0x71, 0x7f, 0x2f, 0x26]); // 车辆 envelope：不收
        let e_bad = event32(100, &[0xa0; 8]); // bodyLen=8 观测域外：不收
        let packets: Vec<(u32, f32, &[u8])> = vec![
            (5, 0.5, &p_area),
            (32, 18.45, &e1),
            (32, 29.65, &e2),
            (32, 30.00, &e_veh),
            (32, 30.50, &e_bad),
        ];
        let areas = collect_destructible_areas(&packets);
        let ev = collect_area_destructible_events(&packets, &areas);
        assert_eq!(ev.len(), 2);
        // R132 t=18.45 实测字节：prop=1（fragiles），slot=0x26（→ lka 逆表 = 谷仓）
        assert_eq!(ev[0].prop, 1);
        assert_eq!(ev[0].slot, 0x26);
        assert_eq!(ev[0].fall_dir, 0x2f);
        assert_eq!(ev[0].body_len, 5);
        assert_eq!(ev[1].prop, 1);
        assert_eq!(ev[1].slot, 0x2b);
        assert_eq!(ev[1].body_len, 6);
        // 原始 args 透传
        assert_eq!(ev[0].args, vec![0xa0, 0x71, 0x7f, 0x2f, 0x26]);
    }

    #[test]
    fn len2_len3_len7_pass_through_without_id() {
        let p_area = area_create(100, 0.0, 0.0);
        let e3 = event32(100, &[0xc0, 0x8b, 0x02]);
        let e7 = event32(100, &[0xa8, 0x10, 0x00, 0x00, 0x80, 0x80, 0x0b]);
        let packets: Vec<(u32, f32, &[u8])> =
            vec![(5, 0.5, &p_area), (32, 1.0, &e3), (32, 2.0, &e7)];
        let areas = collect_destructible_areas(&packets);
        let ev = collect_area_destructible_events(&packets, &areas);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].body_len, 3);
        assert_eq!(ev[0].slot, 0x02);
        assert_eq!(ev[0].fall_dir, 0x8b);
        assert_eq!(ev[1].body_len, 7);
        assert_eq!(ev[1].prop, 1);  // 0xa8 = 1010 1000 → prop=01
        assert_eq!(ev[1].args.len(), 7);
    }

    #[test]
    fn empty_area_set_short_circuits() {
        let e = event32(100, &[1, 2, 3, 4, 5]);
        let packets: Vec<(u32, f32, &[u8])> = vec![(32, 1.0, &e)];
        assert!(collect_area_destructible_events(&packets, &[]).is_empty());
    }
}
