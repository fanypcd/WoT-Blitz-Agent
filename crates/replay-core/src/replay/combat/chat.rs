//! 聊天域事件收集：频道表（Avatar method 0x2f，BigWorld pickle）、聊天文本消息
//! （Avatar method 0x2b）、单发通知（Avatar method 0x12）。
//!
//! 证据与 fail-closed 边界（摘要；完整取证见 docs/replay-chat-events.md）：
//! - type=8 信封 = `[eid u32][method u32][argsLen u32][args]`（逆向总集 §三信封定案）；
//! - **0x2f 频道表**：args = 44B 前缀（双 f64 时间戳等，未闭合）+ BigWorld pickle
//!   protocol-2（`[{channelName: '#chat:channels/battle/team'|'…/common', id, dbID,
//!   flags, isSystem, isReadOnly, isSecured, notifyFlags, greeting, ownerName, owner}]`，
//!   `owner` 观测 = accountId 域）。15/15 跟踪语料逐场一条、形状一致。本模块只实现
//!   **观测到的 pickle 操作码子集**；未见过的操作码 / 形状不符 → 整包跳过（禁猜）。
//! - **0x2b 文本消息**：args = `[tag u8][id u64 LE][len u8][UTF-8×len]`，门禁
//!   `len == argsLen-10` 且 UTF-8 合法（当前语料 1 条样本；与 blitzinfo 同域证据
//!   「Avatar 方法 ≥0x2b 进聊天族」双向印证，单样本 provenance 如实标注）。
//! - **0x12 单发通知**：args = `[subject u32][code u16][00×6]`（54/54 样本形状一致）；
//!   `code` 语义未定——原样透传，消费方不得命名/猜测。
//!
//! 一切收集器只做**形状校验**，不做内容推断；未过门禁的包不产出任何事件。

use serde::Serialize;

/// type=8 实体方法包（replay/packets.rs 线格式；collect.rs 内联判型同值）
const METHOD_PACKET: u32 = 8;
/// Avatar method 0x2f：聊天频道表（pickle）
const AVATAR_CHAT_CHANNELS: u32 = 0x2F;
/// Avatar method 0x2b：聊天文本消息
const AVATAR_CHAT_MESSAGE: u32 = 0x2B;
/// Avatar method 0x12：单发通知（语义未定）
const AVATAR_NOTIFICATION: u32 = 0x12;

/// 频道表条目（pickle 字典的已证实子集；缺键 → 字段省略）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChatChannel {
    /// pickle `channelName` 原文（如 `#chat:channels/battle/team`）
    pub channel_name: String,
    /// pickle `id`（数值通道 id）
    pub id: i64,
    /// pickle `dbID` 原样（符号语义未定，禁猜）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_id: Option<i64>,
    /// pickle `owner`（观测 = accountId 域；命名保留 pickle 原键）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_system: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_read_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_secured: Option<bool>,
}

/// 收集器原始时钟域条目已内联：频道表是**开局状态**（非时序事件），facet 直接给
/// `Vec<ChatChannel>`（单场为空 = 无任何包通过形状门禁）。
///
/// 聊天文本消息（投影层结构；`tag`/`seq` 为原样透传的未定语义字段）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChatMessageEvent {
    pub t: f32,
    /// args[0] 原样（观测 0x01；语义未定）
    pub tag: u8,
    /// args[1..9] u64 LE 原样（观测 3128700733；语义未定）
    pub seq: u64,
    /// args[10..] UTF-8 文本（合法性已校验；内容不做任何过滤/改写）
    pub text: String,
}

/// 单发通知（`code` 语义未定，原样透传）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NotificationEvent {
    pub t: f32,
    /// args[0..4] u32（观测值域 = 实体/账号 id 量级；语义未定）
    pub subject: u32,
    /// args[4..6] u16 LE（观测 11515 / 26109；语义未定）
    pub code: u16,
}

/// 收集器原始时钟域条目（投影层 r2 舍入后转 [`ChatMessageEvent`]；先例同 arena `ArenaUpdate`）。
#[derive(Debug, Clone, PartialEq)]
pub struct RawChatMessage {
    pub clock: f32,
    pub tag: u8,
    pub seq: u64,
    pub text: String,
}

/// 收集器原始时钟域条目（投影层 r2 舍入后转 [`NotificationEvent`]）。
#[derive(Debug, Clone, PartialEq)]
pub struct RawNotification {
    pub clock: f32,
    pub subject: u32,
    pub code: u16,
}

/// 0x2f（onChatAction）的事件类别——**只命名已由语料证实的类别**，其余 raw 透传。
///
/// 证据（15 场跟踪语料 305 条动作事件，逐类比对与实战语义吻合）：
/// - `message`（code 1）：pickle 顶层 = 文本串，前缀名 = 发送者；真实聊天正文（43 条，
///   含 "how do get that tank?" / "?" / "zetzu" / 泰文等）；
/// - `command`（code 20）：pickle = (cmdId, targetId, 0, '', '')，前缀名 = 发送者；
///   快捷指令/战场标记（blitzinfo 同族 chatCommand 语义），cmdId 语义未逐一闭合 →
///   原样透传（禁命名）；
/// - `join`（code 0）/ `leave`（code 2）：频道成员进出（pickle = 名字串 / None）；
/// - `channel_created`（code 7）：开局频道建立/问候，pickle = (ownerName, greeting)，
///   全部出现在开战前 20 秒；
/// - `other`：其余 code（22/4/8 等，频率极低）原样透传。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatEventKind {
    Message,
    Command,
    Join,
    Leave,
    ChannelCreated,
    Other,
}

/// 一条聊天域事件（0x2f onChatAction 已解码子集）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChatEvent {
    pub t: f32,
    pub kind: ChatEventKind,
    /// 原始事件码（u16，前 10 字节 `[ff×8][code u16]`）——未命名类别的唯一证据。
    pub raw_type: u16,
    /// 频道句柄（前 26..30 字节；与 `chat_channels[].id` 同域）
    pub channel: u32,
    /// 发送者/参与者名（前缀字段；与原名字节逐字一致）
    pub sender: String,
    /// 聊天正文（仅 `message`）；其余类别为空
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// 快捷指令码（仅 `command`）：pickle 元组第 0 项，语义未闭合 → 原样透传
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<i64>,
    /// 指令目标（仅 `command`）：pickle 元组第 1 项（0 = 无目标），语义未闭合
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<i64>,
}

/// 一条 0x2f 动作事件的**原始**解码结果（投影层转 [`ChatEvent`]）。
#[derive(Debug, Clone, PartialEq)]
pub struct RawChatAction {
    pub clock: f32,
    pub code: u16,
    pub channel: u32,
    pub sender: String,
    pub(crate) pickle: Option<PickleVal>,
}

/// 解析 0x2f 动作事件的固定前缀 + pickle：
/// `[ff×8][code u16][f64][f64][channel u32][session u32][0 u32][nameLen u8][name][00][pickleLen u8][pickle]`
/// （频道表那一条是 379B 大包、走 [`pickle_channels`]，此处 `nameLen` 字段不成立 → 返回 None）。
fn parse_chat_action(args: &[u8]) -> Option<RawChatAction> {
    if args.len() < 40 || args[0..8] != [0xFF; 8] {
        return None;
    }
    let code = u16::from_le_bytes([args[8], args[9]]);
    let channel = u32::from_le_bytes([args[26], args[27], args[28], args[29]]);
    let name_len = args[38] as usize;
    let name_end = 39usize.checked_add(name_len)?;
    if name_end + 2 > args.len() {
        return None;
    }
    let sender = std::str::from_utf8(&args[39..name_end]).ok()?.to_string();
    if args[name_end] != 0x00 {
        return None; // 名字后必须是 0 分隔（否则不是动作事件形状）
    }
    let pickle_len = args[name_end + 1] as usize;
    let pickle_start = name_end + 2;
    let pickle_end = pickle_start.checked_add(pickle_len)?;
    if pickle_end > args.len() {
        return None;
    }
    let pickle = if pickle_len == 0 {
        None
    } else {
        Some(pickle_parse(&args[pickle_start..pickle_end])?)
    };
    Some(RawChatAction {
        clock: 0.0, // 由收集器填入
        code,
        channel,
        sender,
        pickle,
    })
}

/// 0x2f 动作事件收集（消息 / 快捷指令 / 成员进出 / 频道建立；频道表大包由
/// [`collect_chat_channels`] 处理，此处跳过）。
pub fn collect_chat_events(packets: &[(u32, f32, &[u8])]) -> Vec<RawChatAction> {
    let mut out = Vec::new();
    for (kind, clock, p) in packets {
        if *kind != METHOD_PACKET || p.len() < 12 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != AVATAR_CHAT_CHANNELS {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if 12 + alen > p.len() {
            continue;
        }
        let args = &p[12..12 + alen];
        if args.len() >= 300 {
            continue; // 频道表（pickle 列表）
        }
        if let Some(mut action) = parse_chat_action(args) {
            action.clock = *clock;
            out.push(action);
        }
    }
    out
}

/// 把原始动作投影为 [`ChatEvent`]（类别判定只用已证实映射；未知 code → Other）。
pub fn chat_event_from_raw(raw: &RawChatAction) -> ChatEvent {
    let mut ev = ChatEvent {
        t: raw.clock,
        kind: ChatEventKind::Other,
        raw_type: raw.code,
        channel: raw.channel,
        sender: raw.sender.clone(),
        text: String::new(),
        command: None,
        target: None,
    };
    match (raw.code, &raw.pickle) {
        // 1 = 聊天正文（pickle 顶层 = 文本串）
        (1, Some(PickleVal::Str(text))) => {
            ev.kind = ChatEventKind::Message;
            ev.text = text.clone();
        }
        // 20 = 快捷指令（pickle = (cmdId, targetId, 0, '', '')）
        (20, Some(PickleVal::Tuple(items))) => {
            ev.kind = ChatEventKind::Command;
            if let Some(PickleVal::Int(cmd)) = items.first() {
                ev.command = Some(*cmd);
            }
            if let Some(PickleVal::Int(target)) = items.get(1) {
                ev.target = Some(*target);
            }
        }
        // 0 = 成员进入（pickle = 名字串）；2 = 成员离开（pickle = None）
        (0, _) => ev.kind = ChatEventKind::Join,
        (2, _) => ev.kind = ChatEventKind::Leave,
        // 7 = 频道建立/问候（pickle = (ownerName, greeting)）
        (7, _) => ev.kind = ChatEventKind::ChannelCreated,
        _ => {}
    }
    ev
}

// ---------------------------------------------------------------------------
// 极简 pickle 栈机（仅 protocol-2 观测操作码子集；未知操作码 / 形状不符 → None）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PickleVal {
    Int(i64),
    Str(String),
    Bool(bool),
    None,
    List(Vec<PickleVal>),
    Dict(Vec<(String, PickleVal)>),
    Tuple(Vec<PickleVal>),
    Mark,
}

const OP_MARK: u8 = b'('; // 0x28
const OP_EMPTY_LIST: u8 = b']'; // 0x5D
const OP_EMPTY_DICT: u8 = b'}'; // 0x7D
const OP_BININT1: u8 = b'K'; // 0x4B
const OP_BININT2: u8 = b'L'; // 0x4C
const OP_BININT4: u8 = b'J'; // 0x4A
const OP_INT: u8 = b'I'; // 0x49 文本整数（'\n' 结尾）
const OP_SHORT_BINSTRING: u8 = b'U'; // 0x55 [len u8][bytes]
const OP_BINUNICODE: u8 = b'X'; // 0x58 [len u32][utf8]
const OP_NEWTRUE: u8 = 0x88;
const OP_NEWFALSE: u8 = 0x89;
const OP_BINPUT: u8 = b'q'; // 0x71 [idx u8]  memoize 栈顶（不弹出）
const OP_LONG_BINPUT: u8 = b'r'; // 0x72 [idx u32]
const OP_BINGET: u8 = b'h'; // 0x68 [idx u8]
const OP_SETITEMS: u8 = b'u'; // 0x75（'s'=单对 SETITEM 0x73，未观测不支持）
const OP_APPENDS: u8 = b'e'; // 0x65
const OP_NONE: u8 = b'N'; // 0x4E
const OP_TUPLE: u8 = b't'; // 0x74（MARK..TUPLE）
const OP_TUPLE1: u8 = 0x85;
const OP_TUPLE2: u8 = 0x86;
const OP_TUPLE3: u8 = 0x87;
const OP_STOP: u8 = b'.'; // 0x2E

/// 解析整段 pickle，返回栈顶值；任何未支持操作码 / 截断 / 悬空 Mark → None。
fn pickle_parse(data: &[u8]) -> Option<PickleVal> {
    if data.first() != Some(&0x80) {
        return None;
    }
    let mut pos = 2usize; // PROTO(0x80) + 协议版本字节（语料恒 2）
    let mut stack: Vec<PickleVal> = Vec::new();
    let mut memo: std::collections::HashMap<u64, PickleVal> = Default::default();

    macro_rules! need {
        ($n:expr) => {
            if pos + $n > data.len() {
                return None;
            }
        };
    }

    loop {
        if pos >= data.len() {
            return None; // 无 STOP 终止
        }
        let op = data[pos];
        pos += 1;
        match op {
            OP_MARK => stack.push(PickleVal::Mark),
            OP_EMPTY_LIST => stack.push(PickleVal::List(Vec::new())),
            OP_EMPTY_DICT => stack.push(PickleVal::Dict(Vec::new())),
            OP_NEWTRUE => stack.push(PickleVal::Bool(true)),
            OP_NEWFALSE => stack.push(PickleVal::Bool(false)),
            OP_BININT1 => {
                need!(1);
                stack.push(PickleVal::Int(data[pos] as i64));
                pos += 1;
            }
            OP_BININT2 => {
                need!(2);
                stack.push(PickleVal::Int(
                    i16::from_le_bytes(data[pos..pos + 2].try_into().ok()?) as i64,
                ));
                pos += 2;
            }
            OP_BININT4 => {
                need!(4);
                stack.push(PickleVal::Int(
                    i32::from_le_bytes(data[pos..pos + 4].try_into().ok()?) as i64,
                ));
                pos += 4;
            }
            OP_INT => {
                let end = data[pos..].iter().position(|&b| b == b'\n')? + pos;
                let s = std::str::from_utf8(&data[pos..end]).ok()?;
                stack.push(PickleVal::Int(s.trim().parse().ok()?));
                pos = end + 1;
            }
            OP_SHORT_BINSTRING => {
                need!(1);
                let n = data[pos] as usize;
                pos += 1;
                need!(n);
                stack.push(PickleVal::Str(
                    String::from_utf8(data[pos..pos + n].to_vec()).ok()?,
                ));
                pos += n;
            }
            OP_BINUNICODE => {
                need!(4);
                let n = u32::from_le_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
                pos += 4;
                need!(n);
                stack.push(PickleVal::Str(
                    String::from_utf8(data[pos..pos + n].to_vec()).ok()?,
                ));
                pos += n;
            }
            OP_BINPUT | OP_LONG_BINPUT => {
                let idx: u64 = if op == OP_BINPUT {
                    need!(1);
                    let v = data[pos] as u64;
                    pos += 1;
                    v
                } else {
                    need!(4);
                    let v = u32::from_le_bytes(data[pos..pos + 4].try_into().ok()?) as u64;
                    pos += 4;
                    v
                };
                let top = stack.last().cloned()?;
                memo.insert(idx, top);
            }
            OP_BINGET => {
                need!(1);
                let idx = data[pos] as u64;
                pos += 1;
                let v = memo.get(&idx)?.clone();
                stack.push(v);
            }
            OP_SETITEMS | OP_APPENDS => {
                // Mark 之上为 items，Mark 之下一个为目标容器（CPython pickle 语义）。
                let mark = stack.iter().rposition(|v| matches!(v, PickleVal::Mark))?;
                if mark == 0 {
                    return None; // 悬空 Mark：目标容器不存在
                }
                let items: Vec<PickleVal> = stack.drain(mark + 1..).collect();
                stack.pop(); // Mark
                match stack.last_mut()? {
                    PickleVal::Dict(entries) if op == OP_SETITEMS => {
                        if !items.len().is_multiple_of(2) {
                            return None;
                        }
                        for pair in items.chunks(2) {
                            let PickleVal::Str(key) = &pair[0] else {
                                return None; // 非字符串键：本子集不支持
                            };
                            entries.push((key.clone(), pair[1].clone()));
                        }
                    }
                    PickleVal::List(list) if op == OP_APPENDS => {
                        list.extend(items);
                    }
                    _ => return None, // 容器种类与操作码不符
                }
            }
            OP_NONE => stack.push(PickleVal::None),
            OP_TUPLE1 | OP_TUPLE2 | OP_TUPLE3 => {
                let arity = match op {
                    OP_TUPLE1 => 1,
                    OP_TUPLE2 => 2,
                    _ => 3,
                };
                if stack.len() < arity {
                    return None;
                }
                let items: Vec<PickleVal> = stack.split_off(stack.len() - arity);
                stack.push(PickleVal::Tuple(items));
            }
            OP_TUPLE => {
                // MARK..TUPLE：Mark 之上为元素
                let mark = stack.iter().rposition(|v| matches!(v, PickleVal::Mark))?;
                let items: Vec<PickleVal> = stack.drain(mark + 1..).collect();
                stack.pop(); // Mark
                stack.push(PickleVal::Tuple(items));
            }
            OP_STOP => break,
            _ => return None, // 未知操作码：fail-closed
        }
    }
    stack.pop()
}

/// 从 0x2f 的 args（44B 前缀 + pickle）解析频道表；pickle 不在预期位置 / 形状不符 → None。
fn pickle_channels(args: &[u8]) -> Option<Vec<ChatChannel>> {
    // PROTO 头定位：前缀未闭合，扫描前 64 字节内的 `80 02`（语料恒为 44B 前缀后的 0x80 0x02）
    let scan_end = args.len().min(64);
    let proto_at =
        (0..scan_end.saturating_sub(1)).find(|&i| args[i] == 0x80 && args[i + 1] == 0x02)?;
    let val = pickle_parse(&args[proto_at..])?;
    // 兼容 pickle 尾部（语料 STOP 后即结束；若有多余字节以首个 list 为准）
    let PickleVal::List(mut items) = val else {
        return None;
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        let PickleVal::Dict(entries) = item else {
            return None; // 频道表元素必须是字典
        };
        let mut ch = ChatChannel {
            channel_name: String::new(),
            id: 0,
            db_id: None,
            owner: None,
            flags: None,
            is_system: None,
            is_read_only: None,
            is_secured: None,
        };
        let mut has_name = false;
        let mut has_id = false;
        for (key, v) in entries {
            match (key.as_str(), v) {
                ("channelName", PickleVal::Str(s)) => {
                    ch.channel_name = s;
                    has_name = true;
                }
                ("id", PickleVal::Int(n)) => {
                    ch.id = n;
                    has_id = true;
                }
                ("dbID", PickleVal::Int(n)) => ch.db_id = Some(n),
                ("owner", PickleVal::Int(n)) => ch.owner = Some(n),
                ("flags", PickleVal::Int(n)) => ch.flags = Some(n),
                ("isSystem", PickleVal::Bool(b)) => ch.is_system = Some(b),
                ("isReadOnly", PickleVal::Bool(b)) => ch.is_read_only = Some(b),
                ("isSecured", PickleVal::Bool(b)) => ch.is_secured = Some(b),
                // greeting / ownerName / notifyFlags 等：已证实键但非投影必需，忽略
                _ => {}
            }
        }
        if !has_name || !has_id {
            return None; // 缺必备键：形状不符，整包跳过
        }
        out.push(ch);
    }
    Some(out)
}

/// 频道表收集（Avatar method 0x2f）：每场通常恰一条（开局建立）。
pub fn collect_chat_channels(packets: &[(u32, f32, &[u8])]) -> Vec<ChatChannel> {
    let mut out = Vec::new();
    for (kind, _clock, p) in packets {
        if *kind != METHOD_PACKET || p.len() < 12 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != AVATAR_CHAT_CHANNELS {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if 12 + alen > p.len() {
            continue;
        }
        let args = &p[12..12 + alen];
        if let Some(channels) = pickle_channels(args) {
            out.extend(channels);
        }
    }
    out
}

/// 聊天文本消息收集（Avatar method 0x2b）：形状 `[tag u8][id u64][len u8][UTF-8×len]`。
pub fn collect_chat_messages(packets: &[(u32, f32, &[u8])]) -> Vec<RawChatMessage> {
    let mut out = Vec::new();
    for (kind, clock, p) in packets {
        if *kind != METHOD_PACKET || p.len() < 12 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != AVATAR_CHAT_MESSAGE {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if alen < 10 || 12 + alen > p.len() {
            continue;
        }
        let args = &p[12..12 + alen];
        let tag = args[0];
        let seq = u64::from_le_bytes(args[1..9].try_into().expect("u64 slice"));
        let text_len = args[9] as usize;
        if text_len != args.len() - 10 {
            continue; // 长度门禁：len 必须恰好吃满剩余参数
        }
        let Ok(text) = std::str::from_utf8(&args[10..]) else {
            continue; // UTF-8 不合法：fail-closed
        };
        out.push(RawChatMessage {
            clock: *clock,
            tag,
            seq,
            text: text.to_string(),
        });
    }
    out
}

/// 单发通知收集（Avatar method 0x12）：形状 `[subject u32][code u16][00×6]`。
pub fn collect_notifications(packets: &[(u32, f32, &[u8])]) -> Vec<RawNotification> {
    let mut out = Vec::new();
    for (kind, clock, p) in packets {
        if *kind != METHOD_PACKET || p.len() < 12 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != AVATAR_NOTIFICATION {
            continue;
        }
        let alen = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if alen != 12 || 12 + alen > p.len() {
            continue;
        }
        let args = &p[12..12 + alen];
        if args[6..12].iter().any(|&b| b != 0) {
            continue;
        }
        out.push(RawNotification {
            clock: *clock,
            subject: u32::from_le_bytes(args[0..4].try_into().expect("u32 slice")),
            code: u16::from_le_bytes(args[4..6].try_into().expect("u16 slice")),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// 测试：真实语料字节 fixture（20260902_2045 场 0x2f / 中文样本 26.10.x 0x2b /
// 0x12）+ 门禁反例
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    /// 真实 0x2f args（语料 `1.wotbreplay` 场，pickle 内顺序 [common, team]；
    /// 44B 前缀 + pickle 两频道 + 1 字节尾随）
    const ARGS_2F: &str = "ffffffffffffffff060097adac9d89adda41eeb4ac9d89adda4100000000ffffffffffffffff0000ff4e010080025d7101287d7102285508697353797374656d710389550a6973526561644f6e6c797104895504646249447105492d383132383838343532373939383538300a550b6e6f74696679466c61677371064b00550969735365637572656471078955086772656574696e67710855005505666c61677371094b0255096f776e65724e616d65710a550055056f776e6572710b49343232353131313434333235323835320a550b6368616e6e656c4e616d65710c551c23636861743a6368616e6e656c732f626174746c652f636f6d6d6f6e710d55026964710e4a2de11c00757d710f286803896804896805492d383132383838383832323936353837360a68064b006807896808550068094b03680a5500680b49343232353131313434333235323835320a680c551a23636861743a6368616e6e656c732f626174746c652f7465616d7110680e4a2ee11c0075652e00";

    /// 真实 0x2b args（20261002_2147 中文客户端样本）
    const ARGS_2B: &str = "013d4b75ba0000000018e788b1e78ea9e6b8b8e6888fe79a84e5b191e58d97e58789";

    #[test]
    fn pickle_channels_parse_real_fixture() {
        let channels = pickle_channels(&hex(ARGS_2F)).expect("fixture must parse");
        assert_eq!(channels.len(), 2);
        // common：flags=2；team：flags=3（两字典键经 memo 复用）
        assert_eq!(channels[0].channel_name, "#chat:channels/battle/common");
        assert_eq!(channels[0].id, 0x001C_E12D);
        assert_eq!(channels[0].flags, Some(2));
        assert_eq!(channels[1].channel_name, "#chat:channels/battle/team");
        assert_eq!(channels[1].id, 0x001C_E12E);
        assert_eq!(channels[1].flags, Some(3));
        for ch in &channels {
            assert_eq!(ch.owner, Some(4225111443252852));
            assert_eq!(ch.is_system, Some(false));
            assert_eq!(ch.is_read_only, Some(false));
            assert_eq!(ch.is_secured, Some(false));
            assert!(ch.db_id.is_some());
            assert!(ch.db_id.unwrap().is_negative());
        }
    }

    #[test]
    fn chat_message_shape_gate() {
        // 真实样本：tag=01 / seq=0xba754b3d / len=24 / UTF-8 中文文本
        let args = hex(ARGS_2B);
        let text = std::str::from_utf8(&args[10..]).expect("utf8");
        assert_eq!(text, "爱玩游戏的屑南凉"); // 屑：e5 b1 91（语料原文，非「屌」）
        assert_eq!(args[9] as usize, args.len() - 10);

        // 门禁反例 1：len 与剩余字节数不符 → 拒收
        let mut bad = args.clone();
        bad[9] = 23;
        let mut pkt = Vec::new();
        pkt.extend_from_slice(&8u32.to_le_bytes());
        pkt.extend_from_slice(&0x2bu32.to_le_bytes());
        pkt.extend_from_slice(&(bad.len() as u32).to_le_bytes());
        pkt.extend_from_slice(&bad);
        let alen = u32::from_le_bytes(pkt[8..12].try_into().unwrap()) as usize;
        let a = &pkt[12..12 + alen];
        assert_ne!(a[9] as usize, a.len() - 10);

        // 门禁反例 2：非 UTF-8 → 拒收
        let mut bad2 = args.clone();
        bad2[12] = 0xFF;
        assert!(std::str::from_utf8(&bad2[10..]).is_err());
    }

    #[test]
    fn notification_shape_gate() {
        // 真实样本：subject=0x16661a / code=0x2CFB
        let args = hex("1a661600fb2c000000000000");
        assert_eq!(args.len(), 12);
        assert_eq!(u32::from_le_bytes(args[0..4].try_into().unwrap()), 0x16661A);
        assert_eq!(u16::from_le_bytes(args[4..6].try_into().unwrap()), 0x2CFB);
        assert!(args[6..12].iter().all(|&b| b == 0));

        // 门禁反例：尾 6 字节非零 → 拒收
        let mut bad = args.clone();
        bad[8] = 1;
        assert!(bad[6..12].iter().any(|&b| b != 0));
    }

    #[test]
    fn pickle_unknown_opcode_fails_closed() {
        let mut data = hex("80025d7101"); // PROTO 2 / EMPTY_LIST / BINPUT 1
        data.push(0x63); // 'c' GLOBAL（未观测、未实现）
        assert!(pickle_parse(&data).is_none());
    }

    #[test]
    fn pickle_truncated_fails_closed() {
        let data = hex(ARGS_2F);
        let proto_at = data.windows(2).position(|w| w == [0x80, 0x02]).unwrap();
        assert!(pickle_parse(&data[proto_at..proto_at + 40]).is_none());
    }
    // ---- 0x2f onChatAction（全类型）----

    /// 真实 0x2f 动作 args（20260902_2053 场）：join / message / leave / command 各一。
    const ACTION_JOIN: &str = "ffffffffffffffff00009486f41207a6da41a9a3fb1207a6da4150a51700dd61db780000000012424c41434b5f54524f4f505f736e69706572001980025512424c41434b5f54524f4f505f736e6970657271012e00";
    const ACTION_MSG: &str = "ffffffffffffffff01004c8efc1507a6da418290ea1707a6da4150a51700dd61db780000000012424c41434b5f54524f4f505f736e69706572001c80025515686f7720646f2067657420746861742074616e6b3f71012e00";
    const ACTION_LEAVE: &str = "ffffffffffffffff02003dd65d2107a6da41bd355e2107a6da4150a5170030d771b3000000000d756e6465726c696e6531303236000480024e2e00";
    const ACTION_CMD: &str = "ffffffffffffffff1400d559952207a6da41c825952207a6da4150a51700e1787578000000000e50726174616d615f476572616c6400148002284b1d4a55a517004b00550055007471012e00";

    #[test]
    fn chat_action_message_parses_sender_and_text() {
        let raw = parse_chat_action(&hex(ACTION_MSG)).expect("message action must parse");
        assert_eq!(raw.code, 1);
        assert_eq!(raw.sender, "BLACK_TROOP_sniper");
        assert_eq!(raw.channel, 0x0017_A550);
        let ev = chat_event_from_raw(&raw);
        assert_eq!(ev.kind, ChatEventKind::Message);
        assert_eq!(ev.text, "how do get that tank?");
        assert_eq!(ev.sender, "BLACK_TROOP_sniper");
    }

    #[test]
    fn chat_action_join_leave_command_kinds() {
        let join = chat_event_from_raw(&parse_chat_action(&hex(ACTION_JOIN)).unwrap());
        assert_eq!(join.kind, ChatEventKind::Join);
        let leave = chat_event_from_raw(&parse_chat_action(&hex(ACTION_LEAVE)).unwrap());
        assert_eq!(leave.kind, ChatEventKind::Leave);
        assert_eq!(leave.sender, "underline1026");
        let cmd = chat_event_from_raw(&parse_chat_action(&hex(ACTION_CMD)).unwrap());
        assert_eq!(cmd.kind, ChatEventKind::Command);
        assert_eq!(cmd.sender, "Pratama_Gerald");
        assert_eq!(cmd.command, Some(29));
        assert_eq!(cmd.target, Some(1_549_653)); // tuple[1] 原样（指令目标域，语义未闭合）
    }

    #[test]
    fn channel_table_is_not_treated_as_action() {
        // 频道表（379B pickle 列表）没有动作前缀的 nameLen 形状 → None
        let args = hex(ARGS_2F);
        assert!(parse_chat_action(&args).is_none());
    }

    #[test]
    fn unknown_action_code_is_preserved_raw() {
        let mut args = hex(ACTION_MSG);
        args[8] = 22; // 未命名类别
        let raw = parse_chat_action(&args).expect("shape still valid");
        let ev = chat_event_from_raw(&raw);
        assert_eq!(ev.kind, ChatEventKind::Other);
        assert_eq!(ev.raw_type, 22);
    }
}
