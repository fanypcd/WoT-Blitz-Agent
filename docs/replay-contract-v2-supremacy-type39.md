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

wire 块是 **SPARSE UPDATE**，重建语义（本仓 `reconstruct_supremacy_base_states`；移植自
WotbTools `SupremacyBaseStateReconstructor`，**空闲行判据已偏离该参考口径**，见 §1.2）：

- absent 字段 = 维持前值（不推断）——**但空闲状态行例外（只有基地编号，可附带当前归属），见 §1.2**
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
| 争霸 wrapper12/root11 | 只有 base_index 的全缺省行 = 空操作 → 进度/占领方卡住 | 全缺省行 = **显式清空**该基地（owner/capturing/progress 全归零）——**争霸侧这条判据已被 §1.2 取代**：owner 不再要求缺省或变化 |
| 单基地 wrapper8/root8 | 缺 field3 的块整块丢弃 → 进度停在最后一个正值 | `field3`/`field4` **双缺省块 = 进度归零**（`progress = 0`，显示层映射为 idle） |

证据（四份真实回放，两个载体一致）：

- 争霸 `20260930_2127…`（134 行）：全缺省行共 7 条——开局状态广播 3+3 条（状态本就是全空，
  与清空同义）+ 基地 C 在 `t=107.01` 的出圈中断（17% 起算，4.5s 后重新从 0 起算，中断时刻
  恰是该行）；进度进行中零出现。
- 单基地 `J39` / `J20` / `XM551`：每条进度序列（1,2,3…）之后**必然**跟一对双缺省块
  （J39 一处 / J20 四处 / XM551 三处），进度进行中零出现。

### 1.2 已归属基地的占领中断（2026-10-15 补正）

空闲状态不一定是全零：基地已经属于一方时，争夺中断只清掉 capturing/progress，
原 owner 仍非零。其 wire 结构是 **只有 field1（base index）与 field2（owner）**，
不能要求 owner 也缺省才清空，更不能要求 owner 相比上一行发生变化。
中立与已归属基地统一处理：f3/f4/f5/f6 均缺省时，保留本行 owner（缺省/0 为无主），
清空 capturing/progress。带 f5/f6 的块继续按 sparse 更新处理。

**判据的来源与边界**：这不是新造的启发式，而是 §1.1 那条 proto3 规则的统一化——
「字段全缺省的行 = 该基地的整条状态广播（零值被省略）」对中立与已归属基地同样成立，
旧实现只在 owner 也缺省时才承认这一点，于是已归属基地的中断行落到 sparse 分支里成了
空操作。规则只读字段在场/缺省，不使用车辆位置、也不使用任何统计阈值推断重置。

合成协议示例（不包含私人回放信息）：

| 状态 | wrapper12/root11 子块 hex | 语义 |
|---|---|---|
| 占领中（已归属基地被对方占领） | `0801 1002 1801 2028` | base=1，owner=2，capturing=1，progress=40 |
| 中断后 | `0801 1002` | base=1，owner=2；占领结束，原归属保留（capturing/progress 归零被省略） |
| 中立基地中断（§1.1 情形） | `0801` | base=1；整条状态为空 |

旧实现只识别全空状态或归属变更，于是第二行错误继承上一行的 capturing/progress，
消费方按 `capturing != null` 渲染出一个并不存在的「正在被占领」。

**证据（本仓公开语料 `data/replay_samples/`，14 份中 6 份带 wrapper12/root11，共 150 行；
下面形态串的数字 = 该 field 在场，1=base index、2=owner、3=capturing、4=progress）**：

- 占领进行中的 119 行**全部同时带 f3+f4**（形态 `1234`/`134`/`34`）——语料里不存在
  「占领中却缺占领字段」的行。
- 只有 8 条 owner-only 行（形态 `12`/`2`，全在 `20260930_2127`：t=68.51/69.02、
  75.01/75.51、124.50/125.01、126.52/127.01），**每条都紧跟在一次 97–98% 收尾的占领
  序列之后**（= 占领结束宣告），且约 0.5s 后重发一次；没有一条夹在占领中途。
- 中立基地的中断仍是全缺省行（同场基地 C，t=107.01，形态 `1`，即 §1.1 引用的同一时刻）。
- 反例假设被否：基地 B 在 t=69.02→110.50（41.5s）由 1 方持有，期间该基地**一行都没有**
  ——服务器不周期性重播归属，故不存在「占领途中被保活行误清」的场景。
- 新旧实现对拍：本语料 150 行全等（0 差异），即该修复在公开语料上是无行为变更的加固；
  **触发场景（已归属基地的中断）在公开语料中没有样本**，其证据来自未纳入本仓的私人回放，
  回归因此由合成包覆盖 A–D、双方归属、重新占领与 f5/f6 未知字段保护。

**与参考实现的关系**：WotbTools 研究契约 `supremacy-base-state.md` 把中断清除列为
必须覆盖的场景（regression 列表 "owned by team2 → team1 capturing → capture cancelled /
cleared"），但把 wire 判据留给后端重建层、未闭合；本仓据此采用上面的空闲整播判据
（自有判据，与参考实现口径不同）。消费方同步以 WotbTools 侧实现为准。
字段形状和 PlaybackData v2 均不变；消费方升级修复后的 WASM 即可。

### 1.3 fail-closed 边界

- 争霸侧：带未知字段（f5/f6）的块**不**按清空处理——其语义未证实，维持"缺省 = 维持前值"。
  本仓实测样本里 f5/f6 全程零出现（134 行 0 条），故该例外不影响已证事实。
- 单基地侧：双缺省块**只在确有进度被清掉时**产出归零行（上一条已产出行 progress > 0）。
  普通对局同样会发的裸初始化对不得合成 0 事件，否则 `assault_bases` 恒非空，旧产物
  （无 `assault_objective_present`）的存在性回退判据会被误判成"有目标"。

对消费方的影响：`PlaybackData.version` **不变**（字段形状未变，只是取值语义补正）。
`supremacy_bases` 现在会出现"清空"迁移行，`assault_bases` 会出现 `progress = 0` 行；
显示层应把 progress 0 / capturing 缺省映射为"无占领"（本仓 `frontend/src/scene/baseStatus.js`
即此口径，无需改动）。迁移行总数不变（每条 raw 行照旧产出一条），变的只是行内取值。
WotbTools 侧需在上游正式发版后更新 WASM pin 才拿到修复，并在同一次 PR 内按对方
AGENTS.md 的 Android 同步口径递增 `wotbVersion`（WASM pin 变更属该口径的覆盖范围）。

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

## 7. 独立炮塔刚体（2026-10-14，additive，v0.4.7）

`PlaybackData.version` 保持 **2**。可选 `detached_turrets`（无证据时省略）按实体 id
升序，每条为 `{eid, vehicle_eid, coverage, pose_kf}`。`vehicle_eid` 是 Type5 class=8
初始化属性 0 的真实车辆实体 id，并且必须存在于同一 `vehicles[].eid`；不是击杀者、
账号或坦克类型。未知归属、实体 id/class/owner 冲突、一个 owner 对应多个炮塔实体、
初始化布局不符时不输出该轨道。**不按低 HP、死亡原因或车型推断飞头**。

已验证线路形态（11.20.0_china）：Type5 恰为 48B，`entityTypeId:u16@4=8`；
`pos:3×f32@14`、`yaw/pitch/roll:3×f32@26`；`init_len:u32@38=6`；
初始化载荷为 `count:u8@42=1, key:u8@43=0, vehicle_eid:u32@44`。
Type33→Type5 成对开 AoI 段；Type4 关段；同一 eid 再进场须重新验证相同 class/owner
并用新的 Type5 world pose 起段。Type10≥48B：`pos@12`、`yaw/pitch/roll@36/40/44`。

`pose_kf` 复用列式形状，包含 **t、pos、yaw、pitch、roll** 全部列；K≥1，
`pos.length=3K`，其它列长度=K，所有值有限。时刻为**原始回放秒**，严格升序；
初始 Type5 pose 是每段首帧，随后为该 AoI 内真实 Type10。同 clock 的 Type10
取文件序最后一条真实 pose，不编造子帧时刻。三个角均按相邻样本短弧
解卷绕（弧度，可超 ±π），重入段独立起链，消费端在同段内插值。已有车辆
`pose_kf` 不新增 roll 输出、数值不变。炮塔不经过车辆 AvatarFilter、悬挂、地面
高度修正或自动弹道；真实推碰后的位置更新按原样保存。

`coverage` 为升序不相交的**闭区间**对（单帧段可 start=end）。每段首帧 t=start；
end 是明确 Type4 离场时间，或未离场时的包流末时刻。末 Type10 停更不等于离场，
末已知 pose 可保持到该段 end；禁止跨 Type4/AoI 缺口插值，也不按任意采样间隔
猜离场。消费端只在 coverage 内呈现独立炮塔；身体上的炮塔从首段 start 起分离，
seek 回 start 之前恢复完整模型。

证据：用户本机 77 场 11.20 回放只读核对，50 场含 class8、27 场无此实体；
共 83 条 class8 物化全部符合上述布局、83/83 owner 均联到 class2 战斗车辆。
确认 AC Atlas 场次有一条独立炮塔（初始 Type5 + 420 条 Type10），同场 Carro
死亡未出现对应独立实体；相邻场次两个独立炮塔分别有 637/367 条 Type10。
私有回放和个人数据不入库，回归仅使用合成协议样本。
