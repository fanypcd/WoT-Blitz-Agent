# Replay Contract v2：Supremacy / Supremacy Points / Type39 / Additive 切面

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

wire 块是 **SPARSE UPDATE**，重建语义（本仓 `reconstruct_supremacy_base_states`，
移植自 WotbTools `SupremacyBaseStateReconstructor`，**空闲行判据为本仓自有、与该参考
口径不同**）：

- absent 字段 = 维持前值（不推断）——**但空闲整播行例外，见下一条**
- **空闲整播（zero-omission）判据**：一行的 f3/f4/f5/f6 全缺省 = 该基地的**整条状态广播**
  （proto3 省略零值），**保留本行 owner（缺省/0 = 无主），清空 capturing/progress**——
  中立基地（只有 base index 的行）与已归属基地（base index + owner 的行）统一处理。
  规则只读字段在场/缺省，不使用车辆位置、也不使用任何统计阈值推断重置。
- 显式 0（owner/capturing）= 清空该字段
- 显式 capturing 清空 → progress 连带清空
- 占领中（capturing 在案）发生 owner 变更 → capturing 与 progress 一并清空
- 输出：`SupremacyBaseStateTransition { clock, base_id, owner_team, capturing_team, capture_progress }`——每条 raw 更新一条，携带该基地更新后的完整状态
- seek 消费：取 ≤t 的每基地最后一条逐字段折叠
- 门禁：wrapperFieldNumber 必须 == 12；不合法块整体跳过，绝不产出部分状态

合成协议示例（不含私人回放信息）：

| 状态 | wrapper12/root11 子块 hex | 语义 |
|---|---|---|
| 占领中（已归属基地被对方占领） | `0801 1002 1801 2028` | base=1，owner=2，capturing=1，progress=40 |
| 中断后 | `0801 1002` | base=1，owner=2；占领结束，原归属保留（capturing/progress 归零被省略） |
| 中立基地中断 | `0801` | base=1；整条状态为空 |

### 1.1 fail-closed 边界

- 争霸侧：带未知字段（f5/f6）的块**不**按清空处理——其语义未证实，维持"缺省 = 维持前值"。
- 单基地（wrapper8/root8）侧：`field3`/`field4` **双缺省块 = 进度归零**（`progress = 0`，
  显示层映射为 idle），但**只在确有进度被清掉时**产出归零行（上一条已产出行 progress > 0）——
  普通对局同样会发的裸初始化对不得合成 0 事件，否则 `assault_bases` 恒非空，
  旧产物（无 `assault_objective_present`）的存在性回退判据会被误判成"有目标"。

对消费方的影响：`PlaybackData.version` **不变**（字段形状未变，只是取值语义）。
`supremacy_bases` 会出现"清空"迁移行，`assault_bases` 会出现 `progress = 0` 行；
显示层把 progress 0 / capturing 缺省映射为"无占领"（本仓 `frontend/src/scene/baseStatus.js`
即此口径，无需改动）。迁移行总数不变（每条 raw 行照旧产出一条），变的只是行内取值。
WotbTools 侧需更新 WASM pin 才拿到修复，并在同一次 PR 内按对方 AGENTS.md 的 Android
同步口径递增 `wotbVersion`（WASM pin 变更属该口径的覆盖范围）。

**与参考实现的关系**：WotbTools 研究契约 `supremacy-base-state.md` 把中断清除列为
必须覆盖的回归场景（regression 列表 "owned by team2 → team1 capturing → capture cancelled /
cleared"），但把 wire 判据留给后端重建层、未闭合。消费方同步以 WotbTools 侧实现为准。

## 2. Supremacy 实时点数（PROVEN，wrapper13/root12）

`subtype 48 → wrapper 13 → root field 12 → repeated team 块`；块内 field1=team（1/2）、field2=points（0..100000）。
门禁缺一不可：wrapper 必须 == 13（wrapper=1 名册等即使 root 结构相同也绝不产出点数事件）。
已对真实回放交叉验证（事件数 185/161/69/204/201，点数区间与击毁 ±40 点事件吻合）。
**只消费回放真实广播，绝不按游戏规则推算比分；点数不得反推基地归属**（归属只来自 wrapper12/root11）。

## 3. Type39 瞄准帧（原始帧仍在用；contract 投影已删除）

Type39 = 作者 Avatar 瞄准/炮线帧（28B = 7×f32）：f0=世界系 yaw、f1=世界系 pitch（取负）、
f2..4=世界系射线一点、f5=相对偏航族（PARTIAL，死亡/观战后失效）、f6=车体系俯仰。

- **原始帧（`Type39Frame` / `collect_type39_frames`）保留**：射击复现（`ShotReplayData`）
  用它在开火时刻锚定世界系炮线（|dt|≤0.05s，326/326 三明治保证开火帧存在）。
- **`PlaybackData.aim_frames` 已删除**（连同 `AimFrame` contract 投影）：该字段零消费方
  （实测占回放 JSON 的 57.6%），代价只有序列化与传输。版本**保持 2**：该字段自始为可选
  （`skip_serializing_if`）且无消费方读取，删除不改变任何必需形状；若将来需要作者瞄准线，
  应以**真正被消费**的形态加入（例如随射击复现下发）。

## 4. contract version guard

`PlaybackData.version` 1 → **2**（新增 `supremacy_bases` / `supremacy_points`，
空数组安全序列化）。消费端必须对版本显式拒绝：错版 WASM 不允许被静默解析成半残数据。

## 5. gameplay mode 纪律

`meta.arenaBonusType` 不是 gameplay objective mode 的权威（只覆盖 random/training/tournament
类别）。第一版 Supremacy 目标状态存在性以 canonical base state timeline 为强事实；
Assault/Encounter 的多 candidate 变体映射无证据，fail-closed（消费端不猜）。

### 5.1 单基地目标存在性判定

`assault_objective_present` 的字段契约为「wrapper8/root8 目标族（`field2==1`、
`field1 ∈ {1,2}`）出现即真，**不要求有进度**」——真实反例：10v10
（`arena_bonus_type=45`）的 Mayan Ruins 场次**有目标**，但全场只发 `f1=1,f2=1` +
`f1=2,f2=1` 两条（无 `f3` 进度、无 `f4` 标志流），比契约更严的实现会把整场目标圈压掉。
把这一对读成"双方各自的目标记录初始化、当前无人占领"。修正后：目标族出现过即判存在；
**进度时间线不受影响**（仍只由真实进度行与归零行构成），"有目标但全程未占领"的场次显示
一个 **idle 目标圈（无水位）**。

代价与回归口子：若将来证明确有**无目标**的场次也发这一对，phantom 圈会出现在那些场次上；
届时应改回"需超出初始化对的证据"，以该反例样本为准。多 candidate 几何仍 fail-closed；
争霸/单基地互斥不变。

## 6. 实现

- `crates/replay-core/src/replay/combat/arena.rs`：`collect_supremacy_base_updates` /
  `reconstruct_supremacy_base_states` / `collect_supremacy_points`（`AimFrame` 已删除；
  `collect_type39_frames` 保留给射击复现）
- `crates/replay-core/src/replay/model.rs`：`Timeline.{supremacy_bases, supremacy_points}`
- `crates/replay-core/src/replay/playback.rs`：PlaybackData v2 字段
- 测试：`arena::supremacy_tests`（sparse 重建/显式清空/owner 变更清 capture/wrapper 门禁/
  非法块跳过/多字节 varint 点数）；`facets_smoke`（version=2 + 新键安全序列化 +
  `aim_frames` 不得再出现）

provenance 源：WotBTools `docs/research/replay/supremacy-base-state.md`、
`java/wotb-core/.../EntityMethodDecoder.java`（parseRawSupremacyBaseUpdates /
parseSupremacyPoints / decodeUpdateArena2）、`SupremacyBaseStateReconstructor.java`。

## 7. 独立炮塔刚体（additive）

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
（对照样本：50 场含 class8、27 场无此实体；83 条 class8 物化全部符合上述布局、
83/83 owner 均联到 class2 战斗车辆。）

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
seek 回 start 之前恢复完整模型。回归仅使用合成协议样本（私人回放不入库）。

## 8. 生命轨道（复活模式，additive）

爆燃等复活模式下战车死亡后可复活；协议面本就有复活语义（updateArena 子类型
`19 VEHICLE_WAIT_RESPAWN` / `20 VEHICLE_RESURRECT` / `21 TEAM_RESPAWNS_LEFT`；
AoI 收集器已实证"同 eid 重入"）。切面以 additive 字段表达多死与复活：

- **车辆轨道（`VehicleTrack`）**：
  - `death_times`（升序；`[0] == death_t`）——**仅当该车死亡 ≥2 次**才输出；
  - `resurrection_times`（升序）——**仅非空才输出**。生命状态重建：`death_t` 起死亡，
    至首个 ≥ 该时刻的复活点回 ALIVE，与后续死亡交替；复活证据缺失时字段缺省，
    消费方按死亡终态语义处理（fail-closed）。
- **模型层（`Timeline`）**：`death_records: BTreeMap<eid, Vec<DeathRecord>>`（全部死亡，
  t 升序）与 `resurrections: BTreeMap<eid, Vec<f32>>`。死亡记录 = **prop1 死亡广播 ×
  method1 hp==0 归零事件配对**（同一次死亡的两路证据）：单广播×单归零无条件配对（t 取
  广播、凶手/死因取归零）；多证据（复活模式）就近配对（5s 门控），落单者按自身证据立死。
  `deaths` 保留为首条记录（既有消费方零改动）；`kill_events` 逐死产出，播报按**就近
  未消费**配对（一条播报只归属一次死亡）。
- **复活判定（统一语义，非统计启发）**：死亡未闭合期内「绝对血量证据 终态(归一化 0) →
  正值」的首次回归 = 复活锚点。载体 = **method1 ∪ prop3**（prop3 = 录像者自身血量的
  唯一来路，作者车复活只可能从这里可见；prop3 只参与复活方向、不参与死亡判定）。
- 普通回放（含全部既有 golden）产物零扰动（单死 = `death_t` 已表达，字段省略）；
  `PlaybackData` v2 / `AiReviewFacet` v1 不变。
- **fail-closed 边界（如实声明）**：语料库无一含复活类子类型、无一血量归零回升——
  「复活模式下血量重置经 method1/prop3 绝对快照广播」这一前提**尚无真实样本验证**
  （探针工具 `src/bin/probe_respawn.rs` 待样本；子类型 19/20/21 载荷布局仍未解码，不猜）。
  若该前提不成立，新字段保持缺省、行为与既往一致（不产生错误复活）。
- 消费侧（WotbTools）：scene fallback 与 canonical `lifeTransitions` 消费
  `death_times` + `resurrection_times` 允许 DESTROYED→ALIVE 回转（字段缺省 = 现状行为，
  无需强制同版升级）。

## 9. 血量终态分类（additive）

`AiReviewFacet` 的 `damage`（method1）与 `health`（prop3）事件新增 `hp_state`
（`current` / `zero` / `terminal`，**恒在场**，非 skip-when-none）——同一个 u16 血量后
消费方的**唯一权威分类**，规则 = `combat::hp_terminal_normalized`（终态 ⇔ 归一化为 0，
即 `hp == 0 ∨ (hp as i16) < 0`）；枚举与函数同在 `replay::combat::events`
（`classify_hp_state`，构造上同判，不会漂移）。

- **语义**：`current` = 可用当前血量；`zero` = 0x0000 确知归零；`terminal` = 负 i16
  终态哨兵（实测族 0xFFFF/0xFFFE/0xFFFD，终态分布 −1:68/−2:1/−3:59；其余负值同属该编码
  ——同族全部取值同一处理，**禁按分布裁剪/扩展**，实测分布只作证据）。
- **死亡面**：`hp_state ∈ {zero, terminal}` ∨ prop1=00 ∨ wrapper6 击杀播报 ∨ 结算 field105。
  消费方判死读 `hp_state`；`hp_raw` 只服务显示口径（确知 0 与"哨兵未知"的区分），
  `visibility.hp_raw`（AoI 物化快照）不是死亡证据载体（维持显示域过滤 `0 < hp < 0xFF00`）。
- **动因**：消费方只认 {0x0000, 0xFFFD} 会把 −1/−2 档死亡判成存活（血量冻在最后正值、
  装填条继续外播）——分歧实例、证据与消费方义务见
  [wotbtools-cross-reference.md §七](wotbtools-cross-reference.md)（含迁移清单 §七.1）。
- `PlaybackData` v2 / `AiReviewFacet` v1 形状不变（additive 字段；旧产物字段缺省即旧口径）。
