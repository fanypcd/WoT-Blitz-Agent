// battle_results.dat 结算 protobuf 的补充字段解码。
//
// 背景：外部 crate `wotbreplay-parser` 的 PlayerResultsInfo 只暴露部分字段（credits/xp/shots/
// damage 等）；WotbTools `battle-results.md`（PROVEN）给出了完整字段表，其中本项目需要的
// 1/16/24/25/105/119/120 未被 crate 暴露。本模块用最小 protobuf 扫描器从
// `Replay::read_battle_results_dat()` 的原始 buffer 中补齐（unknown ≠ 0：字段缺失保留 None，
// 不猜 0）。
//
// 字段号权威来源：WotbTools battle-results.md + 本项目全量遍历（《回放与射击逆向总集》第一篇 §九）：
//   #301（每战斗者一条，repeated length-delimited）内：
//   1=终局血量 i32（-2=自动击毁/不活动哨兵、-3 语义禁猜）  16=点亮敌人数
//   24=存活寿命整秒                                        25=击杀者 ID
//   105=死亡原因 i32（-1=存活哨兵、缺省=普通击毁、1=火焰、2=撞击、3=世界/环境）
//   119=毁灭协助次数（≥25% 伤害后盟友击毁）                120=炮印 0..3
//   23=经验  106=银币（WotbTools PROVEN 616/616；crate 的 base_xp/credits_earned 在 11.19 语料中为 0）
//   101=账号 ID（键）  103=车辆 comp descriptor（键）
//   #301 外层 f1 = result/entity ID（f25 击杀者 ID 即此命名空间）
//
// 交叉验证（WotbTools）：`initialActualHp = max(hitpoints_left, 0) + damage_received(11)`
// 可与 type=5 开局血量互验（暂未消费，留待 P3 settlement 层）。

/// 单个战斗者的结算补充字段（crate 未暴露部分）。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PlayerSettlement {
    pub account_id: u32,
    pub tank_id: u32,
    /// #301 外层 f1：本战斗者的 result/entity ID（f25 `killer_id` 引用的就是它）
    pub result_id: Option<u32>,
    /// #301 f23 经验（WotbTools PROVEN 616/616；crate `base_xp` 在 11.19 语料中为 0）
    pub xp: Option<u32>,
    /// #301 f106 银币（WotbTools PROVEN 616/616；crate `credits_earned` 在 11.19 语料中为 0）
    pub credits: Option<u32>,
    /// #301 f11 承受伤害（WotbTools PROVEN：缺省即为 0，为真实数值语义 → 输出端按 0）
    pub damage_received: Option<u32>,
    /// #301 f32 争霸/积分模式获得点数（WotbTools PROVEN，270/616）
    pub victory_points_earned: Option<u32>,
    /// #301 f33 争霸/积分模式夺取点数（WotbTools PROVEN，178/616）
    pub victory_points_seized: Option<u32>,
    /// 终局剩余血量；负值=哨兵族（-2 自动击毁、-3 未闭合禁猜），正=幸存余血
    pub hitpoints_left: Option<i32>,
    /// 死亡原因：-1=存活哨兵、缺省=普通击毁、1=火焰、2=撞击、3=世界/环境（4 未观测禁猜）
    pub death_reason: Option<i32>,
    /// 存活寿命（整秒；实时死亡包缺失的 4/287 场景仅此秒级回退，禁止合成亚秒时间戳）
    pub life_time_secs: Option<u32>,
    /// 击杀者（result/entity ID）
    pub killer_id: Option<u32>,
    /// 点亮敌人数（与实时 method12 baseType2 终值收口，WotbTools 15/15）
    pub n_enemies_spotted: Option<u32>,
    /// 毁灭协助次数（对目标造成 ≥25% 伤害后盟友将其击毁；== 实时 baseType15 终值 34/34）
    pub destruction_assistance: Option<u32>,
    /// 炮印数 0..3（持久 player×tank 状态，==wrapper1 field26 476/476）
    pub gun_marks: Option<u32>,
}

/// 跳过一个未知字段；返回 false = 流不合法。
use crate::replay::combat::pb_varint;

fn skip_field(b: &[u8], o: &mut usize, wire_type: u64) -> bool {
    match wire_type {
        0 => pb_varint(b, o).is_some(),
        1 => {
            *o += 8;
            *o <= b.len()
        }
        2 => match pb_varint(b, o) {
            Some(len) => {
                *o += len as usize;
                *o <= b.len()
            }
            None => false,
        },
        5 => {
            *o += 4;
            *o <= b.len()
        }
        _ => false,
    }
}

/// 解析单条 PlayerResultsInfo 子消息（字段表见模块注释）。
fn parse_player_entry(b: &[u8]) -> Option<PlayerSettlement> {
    let mut s = PlayerSettlement::default();
    let mut o = 0usize;
    while o < b.len() {
        let key = pb_varint(b, &mut o)?;
        let (field, wt) = (key >> 3, key & 7);
        if wt == 0 {
            let v = pb_varint(b, &mut o)?;
            let v32 = v as i32;
            match field {
                1 => s.hitpoints_left = Some(v32),
                16 => s.n_enemies_spotted = Some(v as u32),
                24 => s.life_time_secs = Some(v as u32),
                25 => s.killer_id = Some(v as u32),
                101 => s.account_id = v as u32,
                103 => s.tank_id = v as u32,
                11 => s.damage_received = Some(v as u32),
                23 => s.xp = Some(v as u32),
                106 => s.credits = Some(v as u32),
                32 => s.victory_points_earned = Some(v as u32),
                33 => s.victory_points_seized = Some(v as u32),
                105 => s.death_reason = Some(v32),
                119 => s.destruction_assistance = Some(v as u32),
                120 => s.gun_marks = Some(v as u32),
                _ => {}
            }
        } else if !skip_field(b, &mut o, wt) {
            return None;
        }
    }
    Some(s)
}

/// 结算根字段（`BattleResultsDat.buffer` 顶层）：finishReason + 结算时长。
///
/// WotBTools PROVEN（docs/research/replay/battle-results.md）：
/// - root **f4** = finishReason（1 EXTERMINATION 全歼 / 6 WIN_POINTS_CAP 积分上限；
///   语料仅见 1/6，其他值按原始透传）；
/// - root **f5** = 结算层公共战斗时长（**整秒**）；与 meta.json#battleDuration 不同源，
///   后者不是可靠的对局时钟。
///
/// 缺省一律 None（unknown ≠ 0）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SettlementRootFields {
    pub finish_reason: Option<u32>,
    pub duration_secs: Option<u32>,
}

pub fn parse_root_fields(proto: &[u8]) -> SettlementRootFields {
    let mut out = SettlementRootFields::default();
    let mut o = 0usize;
    while o < proto.len() {
        let Some(key) = pb_varint(proto, &mut o) else {
            break;
        };
        let (field, wt) = (key >> 3, key & 7);
        if wt == 0 {
            let Some(v) = pb_varint(proto, &mut o) else {
                break;
            };
            match field {
                4 => out.finish_reason = Some(v as u32),
                5 => out.duration_secs = Some(v as u32),
                _ => {}
            }
        } else if !skip_field(proto, &mut o, wt) {
            break;
        }
    }
    out
}

/// 段位/状态（root **#201** 名册）：`#201 = { account_id@1, info@2 }`，`info` **f9** =
/// participant rank/status（WotBTools PROVEN，704/704）。返回 account_id → rank。
///
/// 注意：语料证明该 rank 的**模式相关语义随版本解释**（PROVEN/PARTIAL），故消费方
/// 应仅作展示列，不做跨模式比较。
pub fn parse_rank_entries(proto: &[u8]) -> std::collections::HashMap<u32, u32> {
    use std::collections::HashMap;
    let mut out: HashMap<u32, u32> = HashMap::new();
    let mut o = 0usize;
    while o < proto.len() {
        let Some(key) = pb_varint(proto, &mut o) else {
            break;
        };
        let (field, wt) = (key >> 3, key & 7);
        if field == 201 && wt == 2 {
            let Some(len) = pb_varint(proto, &mut o) else {
                break;
            };
            let end = o + len as usize;
            if end > proto.len() {
                break;
            }
            let entry = &proto[o..end];
            let mut io = 0usize;
            let mut account_id: Option<u32> = None;
            let mut rank: Option<u32> = None;
            while io < entry.len() {
                let Some(ikey) = pb_varint(entry, &mut io) else {
                    break;
                };
                let (ifield, iwt) = (ikey >> 3, ikey & 7);
                if ifield == 1 && iwt == 0 {
                    let Some(v) = pb_varint(entry, &mut io) else {
                        break;
                    };
                    account_id = Some(v as u32);
                } else if ifield == 2 && iwt == 2 {
                    let Some(ilen) = pb_varint(entry, &mut io) else {
                        break;
                    };
                    let iend = io + ilen as usize;
                    if iend > entry.len() {
                        break;
                    }
                    // info 内取 f9 = rank
                    let mut jo = io;
                    while jo < iend {
                        let Some(jkey) = pb_varint(entry, &mut jo) else {
                            break;
                        };
                        let (jfield, jwt) = (jkey >> 3, jkey & 7);
                        if jwt == 0 {
                            let Some(v) = pb_varint(entry, &mut jo) else {
                                break;
                            };
                            if jfield == 9 {
                                rank = Some(v as u32);
                            }
                        } else if !skip_field(entry, &mut jo, jwt) {
                            break;
                        }
                    }
                    io = iend;
                } else if !skip_field(entry, &mut io, iwt) {
                    break;
                }
            }
            if let (Some(a), Some(r)) = (account_id, rank) {
                out.insert(a, r);
            }
            o = end;
        } else if !skip_field(proto, &mut o, wt) {
            break;
        }
    }
    out
}

/// 解析结算 protobuf 根消息（`BattleResultsDat.buffer`）：提取全部 #301 战斗者条目。
/// 线格式（crate PlayerResults 模型佐证）：#301 = `{ result_id: u32 @1,
/// info: PlayerResultsInfo @2 (length-delimited) }`——字段在嵌套的 info 里，需进两层。
/// 流不合法时返回已成功解析的前缀（fail-soft，调用方按账号联表）。
pub fn parse_settlement_extras(proto: &[u8]) -> Vec<PlayerSettlement> {
    let mut out = Vec::new();
    let mut o = 0usize;
    while o < proto.len() {
        let Some(key) = pb_varint(proto, &mut o) else {
            break;
        };
        let (field, wt) = (key >> 3, key & 7);
        if field == 301 && wt == 2 {
            let Some(len) = pb_varint(proto, &mut o) else {
                break;
            };
            let end = o + len as usize;
            if end > proto.len() {
                break;
            }
            // 进两层：#301 → tag1 result_id / tag2 = PlayerResultsInfo（两者顺序不假设）
            let entry = &proto[o..end];
            let mut io = 0usize;
            let mut result_id: Option<u32> = None;
            let mut parsed: Option<PlayerSettlement> = None;
            while io < entry.len() {
                let Some(ikey) = pb_varint(entry, &mut io) else {
                    break;
                };
                let (ifield, iwt) = (ikey >> 3, ikey & 7);
                if ifield == 1 && iwt == 0 {
                    let Some(v) = pb_varint(entry, &mut io) else {
                        break;
                    };
                    result_id = Some(v as u32);
                } else if ifield == 2 && iwt == 2 {
                    let Some(ilen) = pb_varint(entry, &mut io) else {
                        break;
                    };
                    let iend = io + ilen as usize;
                    if iend > entry.len() {
                        break;
                    }
                    parsed = parse_player_entry(&entry[io..iend]);
                    io = iend;
                } else if !skip_field(entry, &mut io, iwt) {
                    break;
                }
            }
            if let Some(mut s) = parsed {
                s.result_id = result_id;
                out.push(s);
            }
            o = end;
        } else if !skip_field(proto, &mut o, wt) {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
    }
    fn uint_field(field: u64, v: u64, out: &mut Vec<u8>) {
        varint(field << 3, out);
        varint(v, out);
    }
    fn bytes_field(field: u64, body: &[u8], out: &mut Vec<u8>) {
        varint((field << 3) | 2, out);
        varint(body.len() as u64, out);
        out.extend_from_slice(body);
    }

    #[test]
    fn settlement_entry_carries_result_id_xp_and_credits() {
        let mut info = Vec::new();
        uint_field(101, 3_109_395_921, &mut info);
        uint_field(23, 1134, &mut info);
        uint_field(106, 161_784, &mut info);
        uint_field(25, 11_172_396, &mut info);
        // result_id 放在 info 之后：外层字段顺序不得被假设
        let mut entry = Vec::new();
        bytes_field(2, &info, &mut entry);
        uint_field(1, 11_172_391, &mut entry);
        let mut root = Vec::new();
        bytes_field(301, &entry, &mut root);

        let out = parse_settlement_extras(&root);
        assert_eq!(out.len(), 1);
        let s = &out[0];
        assert_eq!(s.account_id, 3_109_395_921);
        assert_eq!(s.result_id, Some(11_172_391));
        assert_eq!(s.xp, Some(1134));
        assert_eq!(s.credits, Some(161_784));
        assert_eq!(s.killer_id, Some(11_172_396));
    }

    #[test]
    fn missing_xp_credits_and_result_id_stay_unknown() {
        let mut info = Vec::new();
        uint_field(101, 42, &mut info);
        let mut entry = Vec::new();
        bytes_field(2, &info, &mut entry);
        let mut root = Vec::new();
        bytes_field(301, &entry, &mut root);

        let s = &parse_settlement_extras(&root)[0];
        assert_eq!((s.result_id, s.xp, s.credits), (None, None, None));
    }
}
