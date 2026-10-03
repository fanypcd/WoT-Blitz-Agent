//! Type5 昵称域唯一解码器（SSOT）。
//!
//! type=5 车辆全量状态包在载荷偏移 57 携带 1 字节长度前缀的玩家昵称
//! （`[len u8][bytes]`）。昵称域是**原始 UTF-8**（结算侧 battle_results 与
//! ARENA_INFO 组成 blob 昵称同域可证）——非 ASCII（中文等）昵称合法，
//! 字符过滤会断开身份联表（account_id/team/tank_id）。本模块为全库唯一解码语义（SSOT）：
//!
//! - 长度域 `1..=30`；
//! - 合法 UTF-8（拒绝 `from_utf8_lossy`—— replacement char 会把损坏伪装成
//!   普通 mismatch）；
//! - 拒绝控制字符（与组成 blob 昵称解析同规则）；**不限制 ASCII**。
//!
//! 消费方：[`super::events::extract_entity_names`]（实体名字表）、
//! [`super::shots::resolve_author_player_eid_by_nick`]（作者 eid）、
//! `wotb-agent::replay::loadout::collect_player_loadouts`（开局配置联表）。

/// 解码 type=5 载荷的实体 id 与昵称。非 type=5 布局（过短 / 长度域越界 /
/// 非法 UTF-8 / 控制字符）返回 None——调用方按"该实体无昵称"处理，不猜。
pub fn decode_type5_nickname(payload: &[u8]) -> Option<(u32, &str)> {
    // 偏移 57 的昵称块完整存在于 ≥60B 的满血锚点包；更短的 type=5 变体无昵称域
    if payload.len() < 60 {
        return None;
    }
    let eid = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let off = 57usize;
    let len = payload[off] as usize;
    if !(1..=30).contains(&len) || off + 1 + len > payload.len() {
        return None;
    }
    let s = std::str::from_utf8(&payload[off + 1..off + 1 + len]).ok()?;
    if s.chars().any(char::is_control) {
        return None;
    }
    Some((eid, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造最小 type=5 载荷：eid@[0..4]、满血锚点@51、昵称块@57（[len][bytes@58..]）。
    fn mk_type5(eid: u32, nick: &[u8]) -> Vec<u8> {
        let body = if nick.len() > 30 { &nick[..30] } else { nick };
        let mut p = vec![0u8; 60.max(58 + body.len())];
        p[0..4].copy_from_slice(&eid.to_le_bytes());
        p[51..53].copy_from_slice(&1000u16.to_le_bytes());
        p[57] = body.len() as u8;
        p[58..58 + body.len()].copy_from_slice(body);
        p
    }

    fn nick_of(payload: &[u8]) -> &str {
        decode_type5_nickname(payload).unwrap().1
    }

    #[test]
    fn accepts_ascii_cn_cyrillic() {
        assert_eq!(nick_of(&mk_type5(7, b"Anonyme")), "Anonyme");
        // 中文（本 bug 的主角样本）
        assert_eq!(nick_of(&mk_type5(1, "兰亭公子苏".as_bytes())), "兰亭公子苏");
        assert_eq!(
            nick_of(&mk_type5(2, "他们都叫我袁弟呀".as_bytes())),
            "他们都叫我袁弟呀"
        );
        // 合法西里尔
        assert_eq!(nick_of(&mk_type5(3, "Кирилл".as_bytes())), "Кирилл");
        // 空格合法（昵称域非 ascii_graphic 子集；控制字符才拒绝）
        assert_eq!(nick_of(&mk_type5(4, b"a b")), "a b");
    }

    #[test]
    fn rejects_invalid_utf8_control_and_bounds() {
        // 非法 UTF-8（无效首字节 / 截断的多字节序列）
        assert!(decode_type5_nickname(&mk_type5(5, &[0xFF, 0xFE])).is_none());
        let cn = mk_type5(6, "兰亭公子苏".as_bytes());
        let mut trunc = cn.clone();
        trunc[57] -= 1; // 长度声明少一字节 → 末字节截断
        trunc.pop();
        assert!(decode_type5_nickname(&trunc).is_none());
        // 控制字符
        assert!(decode_type5_nickname(&mk_type5(7, b"a\x01b")).is_none());
        // 长度 0 / 越界
        let mut p0 = mk_type5(8, b"abc");
        p0[57] = 0;
        assert!(decode_type5_nickname(&p0).is_none());
        let mut p31 = mk_type5(9, &[b'x'; 40]);
        p31[57] = 31;
        assert!(decode_type5_nickname(&p31).is_none());
        // 声明长度超出载荷（截断包）
        let mut pt = mk_type5(10, b"abcdef");
        pt[57] = 20;
        assert!(decode_type5_nickname(&pt).is_none());
        // 过短变体（无昵称域）
        assert!(decode_type5_nickname(&[0u8; 59]).is_none());
    }
}
