# Replay Contract v2：Supremacy / Supremacy Points / Type39 Aim Frames

状态标记沿用 WotBTools 惯例：PROVEN（真实回放交叉验证）/ INFERRED / UNKNOWN。

## 1. Supremacy 基地状态（PROVEN，wrapper12/root11）

数据链：`Type 8 EntityMethod → subtype 48 (updateArena2) → wrapperFieldNumber=12 → root field 11 → repeated base 块`。

嵌套字段（varint）：

| field | 语义 | 约束 |
|---|---|---|
| 1 | base index，0..3 = A..D | absent 在 canonical 边界按 wire default 0（=A）；唯一补缺省处 |
| 2 | owner team | ∈ {0,1,2}；0 = 显式清空归属 |
| 3 | capturing team | ∈ {0,1,2}；0 = 显式清空占领方（连带清 progress） |
| 4 | capture progress | 0..99 |
| 5/6 | UNKNOWN | 原样透传，禁命名 |

wire 块是 **SPARSE UPDATE**，重建语义（`SupremacyBaseStateReconstructor` 逐行移植）：

- absent 字段 = 维持前值（不推断）——**但全缺省行例外，见 1.1**
- 显式 0（owner/capturing）= 清空该字段
- 显式 capturing 清空 → progress 连带清空
- 占领中（capturing 在案）发生 owner 变更 → capturing 与 progress 一并清空
- 输出：`SupremacyBaseStateTransition { clock, base_id, owner_team, capturing_team, capture_progress }`——每条 raw 更新一条，携带该基地更新后的完整状态
- seek 消费：取 ≤t 的每基地最后一条逐字段折叠
- 门禁：wrapperFieldNumber 必须 == 12；不合法块整体跳过，绝不产出部分状态

### 1.1 零值省略与「占领中断归零」（2026-10-03 契约补正）

wire 上的 base 块按 **proto3 语义省略零值字段**：一个块的值全为零时，线上只剩 base 的
判别字段（甚至整块只带 base_index）。这条约定此前被"缺省 = 维持前值"的 sparse 折叠吞掉，
于是**占领中断（占领车辆出圈 / 被击毁，进度作废）在协议上无法表达**——旧进度与旧占领方
永久挂在基地上，前端表现为进度条不归零，直到下一次占领把它覆盖。

两处解码修正（同一根因，两个互斥载体）：

| 载体 | 旧行为 | 新行为 |
|---|---|---|
| 争霸 wrapper12/root11 | 只有 base_index 的全缺省行 = 空操作 → 进度/占领方卡住 | 全缺省行 = **显式清空**该基地（owner/capturing/progress 全归零） |
| 单基地 wrapper8/root8 | 缺 field3 的块整块丢弃 → 进度停在最后一个正值 | `field3`/`field4` **双缺省块 = 进度归零**（`progress = 0`，显示层映射为 idle） |

证据（四份真实回放，两个载体一致）：

- 争霸 `20260930_2127…`（134 行）：全缺省行共 7 条——开局状态广播 3+3 条（状态本就是全空，
  与清空同义）+ 基地 C 在 `t=107.01` 的出圈中断（17% 起算，4.5s 后重新从 0 起算，中断时刻
  恰是该行）；进度进行中零出现。
- 单基地 `J39` / `J20` / `XM551`：每条进度序列（1,2,3…）之后**必然**跟一对双缺省块
  （J39 一处 / J20 四处 / XM551 三处），进度进行中零出现。

fail-closed 边界：

- 争霸侧：带未知字段（f5/f6）的块**不**按清空处理——其语义未证实，维持"缺省 = 维持前值"。
  本仓实测样本里 f5/f6 全程零出现（134 行 0 条），故该例外不影响已证事实。
- 单基地侧：双缺省块**只在确有进度被清掉时**产出归零行（上一条已产出行 progress > 0）。
  普通对局同样会发的裸初始化对不得合成 0 事件，否则 `assault_bases` 恒非空，旧产物
  （无 `assault_objective_present`）的存在性回退判据会被误判成"有目标"。

对消费方的影响：`PlaybackData.version` **不变**（字段形状未变，只是取值语义补正）。
`supremacy_bases` 现在会出现"清空"迁移行，`assault_bases` 会出现 `progress = 0` 行；
显示层应把 progress 0 / capturing 缺省映射为"无占领"（本仓 `frontend/src/scene/baseStatus.js`
即此口径，无需改动）。

## 2. Supremacy 实时点数（PROVEN，wrapper13/root12）

`subtype 48 → wrapper 13 → root field 12 → repeated team 块`；块内 field1=team（1/2）、field2=points（0..100000）。
门禁缺一不可：wrapper 必须 == 13（wrapper=1 名册等即使 root 结构相同也绝不产出点数事件）。
已对 5 场真实回放交叉验证（事件数 185/161/69/204/201，点数区间与击毁 ±40 点事件吻合）。
**只消费回放真实广播，绝不按游戏规则推算比分；点数不得反推基地归属**（归属只来自 wrapper12/root11）。

## 3. Type39 瞄准帧（原始帧仍在用；contract 投影已于 2026-10-03 删除）

Type39 = 作者 Avatar 瞄准/炮线帧（28B = 7×f32）：f0=世界系 yaw、f1=世界系 pitch（取负）、
f2..4=世界系射线一点、f5=相对偏航族（PARTIAL，死亡/观战后失效）、f6=车体系俯仰。

**原始帧（`Type39Frame` / `collect_type39_frames`）保留**：射击复现（`ShotReplayData`）用它在
开火时刻锚定世界系炮线（|dt|≤0.05s，326/326 三明治保证开火帧存在）。

**`PlaybackData.aim_frames` 已删除**（连同 `AimFrame` contract 投影）：字段自 v2 引入以来
**零消费方**——实测占回放 JSON 的 57.6%（20 车样本 6.92MB 中 3.99MB），代价只有序列化与传输。
版本**保持 2**：该字段自始为可选（`skip_serializing_if`）且无消费方读取，删除不改变任何
必需形状；递增版本反而会与消费方 `version === 2` 的硬门禁互锁（错版数据被显式拒绝），
无收益。若将来需要作者瞄准线，应重新评估以**真正被消费**的形态加入（例如随射击复现下发）。

## 4. contract version guard

`PlaybackData.version` 1 → **2**（新增 `supremacy_bases` / `supremacy_points`，
空数组安全序列化）。消费端必须对版本显式拒绝：错版 WASM 不允许被静默解析成半残数据。
3D 仍处 feature flag，允许 breaking；2D 消费方随本契约同批升级。
（2026-10-03：`aim_frames` 删除不触及版本，理由见 §3。）

## 5. gameplay mode 纪律

`meta.arenaBonusType` 不是 gameplay objective mode 的权威（只覆盖 random/training/tournament
类别）。第一版 Supremacy 目标状态存在性以 canonical base state timeline 为强事实；
Assault/Encounter 的多 candidate 变体映射无证据，fail-closed（消费端不猜）。

### 5.1 单基地目标存在性判定修正（2026-10-03）

`assault_objective_present` 的**字段契约自始**为「wrapper8/root8 目标族（`field2==1`、
`field1 ∈ {1,2}`）出现即真，**不要求有进度**」（见 `PlaybackData` 字段注释）。
实现此前额外要求"出现过 `field3` 或 `field4`"，**比契约更严**，属实现偏差。

被证伪的依据（旧实现引用的 62 份样本里 8 份"只发裸初始化对"）：真实反例是
**10v10（`arena_bonus_type=45`）的 Mayan Ruins 场次**——该模式**有目标**，但全场只发
`f1=1,f2=1` + `f1=2,f2=1` 两条（无 `f3` 进度、无 `f4` 标志流），旧判据把整场目标圈压掉。
把这一对读成"双方各自的目标记录初始化、当前无人占领"比"通用广播"更自然。

修正后：目标族出现过即判存在；**进度时间线不受影响**（仍只由真实进度行与归零行构成），
所以"有目标但全程未占领"的场次会显示一个 **idle 目标圈（无水位）**。

代价与回归口子：若将来证明确有**无目标**的场次也发这一对，phantom 圈会出现在那些场次上；
届时应改回"需超出初始化对的证据"，以该反例样本为准。

未受影响：多 candidate 几何仍 fail-closed（消费端不猜坐标）；争霸/单基地互斥不变。

## 6. 实现

- `crates/replay-core/src/replay/combat/arena.rs`：`collect_supremacy_base_updates` /
  `reconstruct_supremacy_base_states` / `collect_supremacy_points`（`AimFrame` 已于
  2026-10-03 删除；`collect_type39_frames` 保留给射击复现）
- `crates/replay-core/src/replay/model.rs`：`Timeline.{supremacy_bases, supremacy_points}`
- `crates/replay-core/src/replay/playback.rs`：PlaybackData v2 字段
- 测试：`arena::supremacy_tests`（sparse 重建/显式清空/owner 变更清 capture/wrapper 门禁/
  非法块跳过/多字节 varint 点数）；`facets_smoke`（version=2 + 新键安全序列化 +
  `aim_frames` 不得再出现）

provenance 源：WotBTools `docs/research/replay/supremacy-base-state.md`、
`java/wotb-core/.../EntityMethodDecoder.java`（parseRawSupremacyBaseUpdates /
parseSupremacyPoints / decodeUpdateArena2）、`SupremacyBaseStateReconstructor.java`。
