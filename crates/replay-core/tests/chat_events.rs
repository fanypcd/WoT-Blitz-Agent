//! 聊天域收集的真实语料回归：15/15 场频道表 + 通知形状 + 中文聊天样本。
//!
//! 证据边界：本测试锁定**当前跟踪语料**上的收集器行为（形状门禁零漏报/零误报）；
//! 语料外回放按 fail-closed 静默跳过未过门禁的包（不产出事件），不在此断言。

use std::path::PathBuf;

use wotb_replay_core::replay::combat::{
    collect_chat_channels, collect_chat_messages, collect_notifications,
};
use wotb_replay_core::replay::packets::read_raw_packets;

fn all_fixtures() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/replay_samples");
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wotbreplay"))
        .collect();
    v.sort();
    v
}

fn tuples(path: &PathBuf) -> Vec<(u32, f32, Vec<u8>)> {
    let raw = std::fs::read(path).expect("read fixture");
    read_raw_packets(&raw)
        .expect("frame packets")
        .into_iter()
        .map(|p| (p.packet_type, p.clock_secs, p.payload))
        .collect()
}

fn refs(packets: &[(u32, f32, Vec<u8>)]) -> Vec<(u32, f32, &[u8])> {
    packets.iter().map(|(t, c, p)| (*t, *c, p.as_slice())).collect()
}

/// 每场恰两条频道（team + common），channelName 形如 `#chat:channels/battle/<后缀>`。
#[test]
fn chat_channels_two_per_battle() {
    let mut battles = 0;
    for f in all_fixtures() {
        let packets = tuples(&f);
        let channels = collect_chat_channels(&refs(&packets));
        battles += 1;
        assert_eq!(channels.len(), 2, "{}: expected 2 channels", f.display());
        let mut names: Vec<&str> = channels.iter().map(|c| c.channel_name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["#chat:channels/battle/common", "#chat:channels/battle/team"]);
        // pickle 内频道顺序不固定（20260902_2045 场为 [team, common]）；
        // 观测不变量：team 通道 id 严格大于 common（创建序）
        let mut by_name: Vec<(&str, i64)> =
            channels.iter().map(|c| (c.channel_name.as_str(), c.id)).collect();
        by_name.sort_unstable();
        assert!(by_name[1].1 > by_name[0].1, "{}: team id > common id", f.display());
        // owner 出现且与 pickle owner 域一致（同场两频道 owner 相同）
        assert_eq!(channels[0].owner, channels[1].owner, "{}: same owner", f.display());
    }
    assert!(battles >= 15, "corpus shrank: {battles} battles");
}

/// 单发通知：形状门禁全过（subject/code 提取成功），54 条观测基线（12B 定形）。
#[test]
fn notifications_corpus_baseline() {
    let mut total = 0usize;
    for f in all_fixtures() {
        let packets = tuples(&f);
        total += collect_notifications(&refs(&packets)).len();
    }
    assert_eq!(total, 54, "notification count drifted (observed baseline 54)");
}

/// 聊天文本：跟踪语料恰 1 条（中文客户端样本），内容逐字节锁定（含「屑」而非「屌」）。
#[test]
fn chat_messages_corpus_baseline() {
    let mut msgs = Vec::new();
    for f in all_fixtures() {
        let packets = tuples(&f);
        msgs.extend(collect_chat_messages(&refs(&packets)));
    }
    assert_eq!(msgs.len(), 1, "chat message count drifted");
    assert_eq!(msgs[0].tag, 0x01);
    assert_eq!(msgs[0].seq, 0xBA75_4B3D);
    assert_eq!(msgs[0].text, "爱玩游戏的屑南凉");
}
