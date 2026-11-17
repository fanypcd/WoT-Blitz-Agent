# 回放聊天域取证（chat_channels / chat_messages / notifications）

> 状态：v0.5.0 已实现（`crates/replay-core/src/replay/combat/chat.rs`）。
> 本文是该 facet 的完整证据链、形状定义、fail-closed 边界与已知未解项。

## 一、协议位置

type=8 实体方法流，信封 `[eid u32][method u32][argsLen u32][args]`（总集 §三信封定案）。
聊天族落在 **Avatar 实体方法空间 ≥0x2b**：

| mid | 语义 | 参数形态 | 语料覆盖 |
|---|---|---|---|
| **0x2f (47)** | **onChatAction（聊天域动作，主数据源）** | `[ff×8][code u16][f64][f64][channel u32][session u32][0 u32][nameLen u8][sender][00][pickleLen u8][pickle]` | **305 条动作事件 / 15 场**（见 §2.1） |
| 0x2f (47) | 聊天频道表（同方法大包） | 44B 前缀 + pickle 列表（>300B） | 15/15 场，每场恰 1 条 |
| 0x2b (43) | 另域文本（**非战斗聊天**） | `[tag u8][u64 频道句柄][len u8][UTF-8×len]` | 1 条（中文客户端样本） |
| 0x12 (18) | 单发通知（语义未定） | `[subject u32][code u16][00×6]` | 54 条，形状全部一致 |

### 2.1 0x2f 动作事件（v0.5.1 全类型）

`code` = args[8..10] u16；`channel` = args[26..30] u32（与频道表 `id` 同域）；`sender` =
args[39..39+nameLen]（UTF-8 原文）；pickle（protocol 2）给出类别载荷：

| code | kind | pickle 载荷 | 语义证据 |
|---|---|---|---|
| **1** | `message` | 顶层 = 文本串 | **真实聊天正文**："how do get that tank?" / "?" / "zetzu" / 泰文句子，发送者 = 前缀名 |
| **20 (0x14)** | `command` | 元组 `(cmdId, targetId, 0, '', '')` | 快捷指令/战场标记；cmdId ∈ {24,26,29,…} 语义未逐一闭合 → 原样透传（禁命名） |
| 0 | `join` | 名字串 | 频道成员进入（t 4.4–124.6s，开局段） |
| 2 | `leave` | `None` | 频道成员离开（t 56.5–314.8s） |
| 7 | `channel_created` | 元组 `(ownerName, greeting)` | 开局频道建立/问候（全部 t < 20s） |
| 22/4/8 | `other` | 各异 | 频率极低（≤2 条），raw 透传 |

**v0.5.0 的归类修正**：此前 `chat_messages`（0x2b）被当作聊天正文；实测其 `u64` 字段是
**频道句柄**、文本是另一域（改名/公告族，单样本），并非战斗聊天。战斗聊天正文只在
0x2f code 1（43 条 / 全语料）。两字段均保留（additive），消费方应以 `chat_events` 为准。

### 2.2 method38 组件 token → 逐模块损伤（`module_damage_events`）

`method38`（作者射击）尾部组件表 `count × (componentToken u8 + rawState u8)` 逐项展开：
`victim_eid` + `component`（31 引擎 / 32 弹药架 / 33 油箱 / 34-35 右左履带 / 36 火炮 /
37 炮塔旋转机 / 38 观察装置 / 39 车长 / 40 驾驶员 / 41 炮手 / 43 装填手 / 42 UNKNOWN 禁猜）
+ `rawState`（**0=命中但无新持久负面 / 1=受损或乘员受伤 / 2=critical 禁用族**）。
他人路径无此包；`module_crew_states`（method16）继续覆盖全车状态事件。

**blitzinfo 交叉验证**（第二独立来源）：其 `replay-core.wasm` 数据段含 `chat`/`text`、
`command`/`args`/`response`、`marker`/`markerType`、`channelName`/`channel` 事件族字段名；
其分派函数对 Avatar 类方法 ≥0x2b 走聊天解析分支（WAT func392 `(class==5 ∧ method≥43)` 守卫，
直接证据）。UI 侧 `chatCommand/{ally_attack,ally_retreat,attack,attack_enemy,attention_to_cell,
back_to_base,follow_me,help_me,moving_here,negative,positive}` 共 11 种快捷指令 kind。

## 二、0x2f 频道表（pickle）

args 前缀 44B（双 f64 时间戳、`ff×8`、`ff 4e 01 00` 等，未闭合——**不解析，按 `\x80\x02`
定位 pickle 起点**）。pickle 为 protocol-2 值栈：`EMPTY_LIST` + 2×`EMPTY_DICT`（字典键经
BINPUT/BINGET memo 复用）。

实测字典键集（11 键）与两频道样本值（`1.wotbreplay` 场）：

| 键 | common | team | 类型 |
|---|---|---|---|
| channelName | `#chat:channels/battle/common` | `#chat:channels/battle/team` | str |
| id | 1893677 (0x001CE12D) | 1893678 | int |
| dbID | -8128884527998580 | -8128888822965876 | int（符号语义未定） |
| flags | 2 | 3 | int |
| owner | 4225111443252852（同场两频道同值，= accountId 域） | 同左 | int |
| isSystem / isReadOnly / isSecured | false | false | bool |
| notifyFlags | 0 | 0 | int |
| greeting / ownerName | `""` | `""` | str |

**已证不变量**：恰 2 频道、名称恒 common/team 二值；pickle 内顺序不固定
（20260902_2045 场为 [team, common]）；`team.id > common.id`（创建序，15/15）。
id 跨场为小整数（1.4M~1.8M 量级），非实体 id。

## 三、0x2b 聊天文本消息（单样本）

中文客户端样本（20261002_2147__shenle_S16_Kranvagn_15612263746483708，t=163.17）：

```
args(34) = 01 | 3d 4b 75 ba 00 00 00 00 | 18 | e788b1 e78ea9 e6b8b8 e6888f e79a84 e5b191 e58d97 e58789
           tag  seq(u64 LE)=3128700733    24  「爱玩游戏的屑南凉」（e5 b1 91 = 屑，非「屌」）
```

门禁：`len == argsLen - 10`（长度恰好吃满）且 UTF-8 合法。`tag`（观测 0x01）与
`seq`（观测 3128700733，账号 id 域以下量级）**语义未定，原样透传，禁猜**。
发送者昵称不在包内（UI 需另行联表或留空——消费方注意）。

## 四、0x12 单发通知（54 样本）

形状 `args(12) = [subject u32][code u16][00×6]`，全部 54 条形状一致。
观测 `code`：11515 (0x2CFB) 多场、26109 (0x65FD) 两场；`subject` 在部分场为实体 id
量级（0x16661a…），另两场为账号 id 量级（136528022/136686289）——两者语义均**未定**，
原样透传。客户端名表未定位（总集未记录该方法号），消费方不得命名。

## 五、fail-closed 边界

- pickle 栈机只实现**观测操作码子集**（PROTO/MARK/EMPTY_LIST/EMPTY_DICT/BININT1/2/4/
  INT 文本/SHORT_BINSTRING/BINUNICODE/NEWTRUE/NEWFALSE/BINPUT/LONG_BINPUT/BINGET/
  SETITEMS('u')/APPENDS/STOP）；未知操作码、悬空 MARK、容器种类错配、缺
  channelName/id 必备键 → 整包跳过。注：单对 SETITEM（'s'，0x73）未观测，不支持。
- 0x2b：长度门禁不过 / 非 UTF-8 → 跳过。
- 0x12：argsLen≠12 或尾 6 字节非零 → 跳过。
- 全部收集器只做形状校验，无内容推断；未过门禁的包在 `packet_histogram` 中仍按原
  type 计数（现有诊断面），不静默消失。

## 六、已知未解（后续任务候选）

1. **快捷指令 marker**：blitzinfo 白名单 {1,6,9,10,15,18,26}（WAT 0x1048642 位掩码）
   证明 markerType 编码存在，但其包级定位未完成（本语料 15 场 0 条可归因 marker 包，
   静态追踪 func392 marker 分支参数语义未闭合）——需要含快捷指令的语料。
2. 0x2f 的 44B 前缀结构（双 f64 时间戳已识别为时间量纲，字段布局未闭合）。
3. 0x2b 的 `tag`/`seq` 语义与发送者归属（需多聊天样本，尤其含 id→昵称可互验者）。
4. 0x12 通知 `code` 名表（需客户端二进制侧定位该方法的分派表）。
5. `receiveNotification` / `onQuestCompleted` 等同族 Avatar 方法的包级归属仍待逆向
   （总集 §4.2 待办不变）。

## 七、消费方契约（WotbTools）

- `chat_channels`：频道表为**开局状态**（非时序事件）；事件流不展示。
- `chat_messages[].t`：事件流条目（聊天过滤片），文案 = `text` 原文；`tag`/`seq` 不上 UI。
- `notifications[].t`：事件流「其他」过滤片，显示为 `通知 #<code>`（不猜测含义）；
  `subject` 不上 UI（语义未定）。
- additive 契约：旧消费方忽略未知键零扰动；pin bump 后重生成 AI 投影 golden
  （预期仅 provenance 变化——聊天三键不进 AI 切面）。
