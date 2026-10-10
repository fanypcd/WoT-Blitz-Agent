# 客户端坦克悬挂（Suspension）逆向：处理链与逐车数据面

> 2026-10-10。分析对象：Steam PC 版 `wotblitz.exe`（32 位 PE，2026-09-02 build，
> image base 0x400000，72,718,336 B）＋客户端资源 `Data/`（DVPL 解码）。
> 目的：3D 回放里「履带/负重轮离地」要做成**客户端同构**——车体 = 回放记录位姿、
> 负重轮/履带由悬挂逐帧解算（车体侧已在 WotbTools 回退定稿，见其
> `docs/features/battle-playback.md`）。本文只记**可复现的事实与锚点**，不猜公式。
>
> 记法：`RVA` = 该字符串在客户端镜像中的地址（本文所有值都来自**字符串位置**，即
> `类::方法` 断言/日志字符串与节点绑定格式串——它们可再验证、也是下一轮 objdump
> 定向反汇编的 xref 锚点；**不是**函数入口地址）。方法：PE 段表映像 + 全文件字节扫描。

## TL;DR

1. **客户端把悬挂拆成三个逐帧系统**：`SuspensionSystem`（总成/履带垂挂）、
   `SuspensionWheelsSystem`（负重轮）、`SuspensionTrackSystem`（履带 chunk 渲染），
   每车另有 `SuspensionComponent` / `SuspensionWheelsComponent` /
   `SuspensionTrackComponent` 三件。**车体不在其中**——车体吃回放记录位姿。
2. **逐车悬挂数据是现成的**（旧结论「悬挂参数三处全查空」对本条不成立，见 §四勘误）：
   `Data/3d/Tanks/Parameters/<nation>/<stem>.yaml` 的 `suspension:` 块，本机 730/764 辆有；
   含逐轮数组（`wheels`）、履带折线（`left/rightTrackChain(s)`）、弯曲/铺放系数。
3. **逐轮几何（半径/连杆/枢轴）不 authored**：由 `SuspensionGenerator`（`GenerateSuspensionSegments`
   / `GenerateWheelsClosestUpperPoints` / `GenerateUpperPointsSupportWheels`）与
   `SuspensionUtils::GetSuspensionWheelsInfo` 从模型节点现算；履带形变是"chunk"（`TrackChunkGenerator`）。
4. **逐轮/逐带怎么动已还原**（第二轮，§六）：负重轮 = **纯竖直平移**，夹到
   `[pz−b, pz+a]`（`wheels` 的两条 f32 = 行程上下限，`flag` = 负重轮/非负重轮）、按
   `wheelsReactionSpeed` 限速；轮自转 = 绕骨骼局部 X 轴 `θ += Δs_side·k`（k 候选 1/r）；
   履带 = **运行时切 chunk + GPU 实例化**，每帧由「静止折线 + 逐轮偏移 + 悬空段垂弧/包轮 +
   铺地」得到 2D 链路，花纹按 `chassis.textureScale / chunkLength` 每米滚动。
   仍缺的（§五）：半径的构造口径、k 的构造、`chunkOffset` 写入者、`trackLayingInfo` 五项落位。
5. 这份数据**不在分发面内**（客户端 yaml 不随资产包发）。前端要仿客户端，
   需本仓先把它导进包（additive）——规模 ≈1.5–2 KB/辆、全量 ≈1.2–1.5 MB，分级路径见 §七。

---

## 一、处理链（二进制证据）

源码路径字符串给出工程结构（`C:/ba/tc/work/t/client/dava.framework/Modules/GameplayCommon/Sources/GameplayCommon/`）：

| 字符串 | RVA |
|---|---|
| `…/BattleCommon/VehicleSuspension/SuspensionGenerator.cpp` | 0x3238fa8 |
| `…/BattleCommon/VehicleSuspension/TrackChunkGenerator.cpp` | 0x32393c8 |
| `…/Components/SuspensionComponent.cpp` | 0x32432c8 |
| `…/Systems/SuspensionSystem.cpp` | 0x3249178 |
| `…/Systems/SuspensionTrackSystem.cpp` | 0x3249280 |
| `…/Systems/SuspensionWheelsSystem.cpp` | 0x3249488 |

`类::方法` 限定名（断言/日志字符串，作为 xref 锚点）：

| 字符串 | RVA |
|---|---|
| `SuspensionGenerator::GenerateSuspensionSegments` | 0x32390c0 |
| `SuspensionGenerator::GenerateWheelsClosestUpperPoints` | 0x32390f0 |
| `SuspensionGenerator::GenerateUpperPointsSupportWheels` | 0x3239128 |
| `SuspensionUtils::BuildWheelsRelativeTransform` | 0x3239230 |
| `SuspensionUtils::GetSuspensionWheelsInfo` | 0x3239284 |
| `SuspensionUtils::GetOriginalTrackBatch` | 0x3239308 |
| `SuspensionUtils::GetTracksEntities` | 0x3239348 |
| `SuspensionUtils::GetWheelsEntity` | 0x323936c |
| `SuspensionUtils::AddSuspension` | 0x3239390 |
| `Suspension::GetTrackChain` | 0x324241c |
| `SSuspensionParameter::RetrieveBinaryProperty` | 0x3242ae4 |
| `SuspensionComponent::SuspensionComponent` | 0x3243340 |
| `SuspensionSystem::ProcessHangingSegmentsBending` | 0x32490f8 |
| `SuspensionSystem::ProcessHangingSegmentsLaying` | 0x3249128 |
| `SuspensionSystem::Process` | 0x324920c |
| `SSuspensionTrackSystem::SetupChunkRenderBatch` | 0x32492f8 |
| `SSuspensionTrackSystem::SetupOriginalRenderBatch` | 0x3249328 |
| `SuspensionTrackSystem::Process` | 0x32493e8 |
| `SuspensionWheelsSystem::Process` | 0x3249528 |

关键断言（钉住数据契约）：

| 字符串 | RVA | 含义 |
|---|---|---|
| `wheelsComponent->wheelInfos.size() == suspension.wheels.size()` | 0x32392b0 | **`wheels` 数组与逐轮信息 1:1**（长度 = 该车每侧轮数） |
| `wheelsEntity->HasComponent<SuspensionWheelsComponent>()` | 0x324336c | 轮组是独立实体 |
| `trackEntity->HasComponent<SuspensionTrackComponent>()` | 0x32433a4 | 履带是独立实体 |

RTTI 类型名（`.?AV…@@`，均为字符串位置）：

| 字符串 | RVA |
|---|---|
| `.?AVSuspensionComponent@@` | 0x3c594bc |
| `.?AVSuspensionTrackComponent@@` | 0x3c5950c |
| `.?AVSuspensionWheelsComponent@@` | 0x3c59534 |
| `.?AVSuspensionTrackSystem@@` | 0x3c5a260 |
| `.?AVSuspensionParameter@@` | 0x3eb434c |
| `.?AVSuspensionSystem@@` | 0x3eb7f98 |
| `.?AV?$_Func_base@MABUHangingSegment@Suspension@@@std@@` | 0x3eb7fb8 |
| `.?AVSuspensionWheelsSystem@@` | 0x3eb7ff8 |

最后一行说明存在嵌套类型 **`Suspension::HangingSegment`**（"垂挂段"）：与
`ProcessHangingSegmentsBending` / `ProcessHangingSegmentsLaying` 对应，即**支座轮之间的履带段**
（悬空段）按 `trackBendingInfo` / `trackLayingInfo` 弯曲/铺放。高度只走地形面：
`[Landscape::GetHeightAtPoint] Trying to get height a…`（0x329e4c8）与
`memoryfile_landscape_height`（0x329e4a8）——全二进制里**只有这一个**高度 API 字符串。

## 二、节点绑定契约（模型侧）

绑定用的名字格式串（同一区块，RVA 连续，便于对照）：

| 字符串 | RVA | 说明 |
|---|---|---|
| `chassis_wheel_L_%.2d` / `chassis_wheel_R_%.2d` | 0x323abf8 / 0x323ac10 | 逐轮节点（编号从 01 起） |
| `chassis_track_L_%.2d` / `chassis_track_R_%.2d` | 0x323abc8 / 0x323abe0 | 逐段履带节点（多段模型用） |
| `chassis_track_L` / `chassis_track_R` | 0x3234e50 / 0x3234e60 | 整条履带（单段模型用） |
| `chassis_track_crash_L` / `chassis_track_crash_R` | 0x3234e70 / 0x3234e88 | 断带形态 |
| `chassis_wheels_L` / `chassis_wheels_R` | 0x3234f4c / 0x3234f60 | **运行时实体**（不是模型节点，见下） |
| `wheel_L_%.2d` / `wheel_R_%.2d` | 0x323ac28 / 0x323ac38 | 未定（同区块；或为旧命名/状态实体） |
| `state_entity_%.2d` / `%s_state_%.2d` | 0x323ac48 / 0x323ac5c | 未定（状态实体） |
| 正则 `^track_(?P<side>(R\|L))_(?P<index>([0-9]{2}))$` | 0x323ac6c | 履带段名解析（`track_L_00` 形态） |
| 正则 `^(.+?)_number_([0-9]+?)$`、`^(.+?)_number_([0-9]+?)_([0-9]+?)$` | 0x323ac9c / 0x323acb8 | 未定（编号名） |

**包内普查（本仓 735 辆 model.glb，节点名直接来自 sc2）**：

| 项 | 结果 |
|---|---|
| 有 `chassis_wheel_{L,R}_NN` | **735/735** |
| 有 `chassis_track_L` / `chassis_track_R` | 731/735 |
| 用**编号分段**履带 `chassis_track_{L,R}_NN` 代替整条 | 4/735（tank_id 11633 / 21057 / 21825 / 22305；如 Forest Witch = `_L_01` `_L_02`） |
| 有 `chassis_wheels_L/R`（复数，整组容器） | **0/735** ⇒ 它是客户端运行时建的实体，不是模型节点 |
| 每侧轮数（`chassis_wheel_L` 计数） | 1…22，中位 11（分布：6–15 占绝大多数，186 辆为 11） |

⇒ 前端按 `/^chassis_(track|wheel)_/i` 分离底盘与车体子树（WotbTools `playbackScene.collectGlbParts`）
在 735/735 上都有命中；多段履带的 4 辆走同一条正则（`chassis_track_L_01` 也匹配）。

## 三、逐车数据面：`suspension:` 块

位置：`Data/3d/Tanks/Parameters/<nation>/<stem>.yaml`（DVPL）。**读取须经 DLC 覆盖层**
（`client_path()` / `resolve_client_path()`），因为 `packs/` 可能覆盖。

### 3.1 标量字段

| 字段 | 类型 | 样例（IS-7 / Maus） | 备注 |
|---|---|---|---|
| `enabled` | bool | true | 764 辆里仅 1 辆 false |
| `wheelsReactionSpeed` | f32 | 1.1 / 1.0 | 轮对地形的反应速度 |
| `chunkPrototypeExtending` | f32 | 0.1 / 0.1 | 履带 chunk 原型外扩（配 `TrackChunkGenerator`） |
| `trackBendingInfo.frontDriveWheel` | bool | false / false | 前驱？——false = 后驱动轮（与 `wheels` 末位 flag=0 的位置方向一致） |
| `trackBendingInfo.upperMin/upperFactor/frontFactor/backFactor/lengthPower/speed` | f32 | 0.28/0.59/0.4/0.25/0.5/0.9（IS-7） | 悬空段弯曲参数 |
| `trackLayingInfo.bendingFactor/lengthPower/pointCountPower/pressurePower/primaryPower` | f32 | 0.744/0.92/0.918/0.26/0.67（IS-7） | 履带铺放参数 |

### 3.2 二进制块（base64，**尾部带一个 0x0C 终止字节**）

编码是"base64 字符 + 0x0C 结束"（`Oth10_WarDuck` 的空块整串就是一个 0x0C）——解码前先去掉
尾部 0x0C。读取方为 `SSuspensionParameter::RetrieveBinaryProperty`（0x3242ae4）。

**`wheels`：每侧逐轮数组，每记录 12 B = `{u32 flag, f32 a, f32 b}`（小端）**

- 记录数 **== 该车每侧 `chassis_wheel_{L,R}_NN` 节点数**（已复核：IS-7 9/9、Maus 14/14、
  T-54 7/7、E 100 10/10；全量 yaml 侧 1…22、中位 11，与包内节点普查一致）。
  与断言 `wheelInfos.size() == suspension.wheels.size()` 互证：**逐轮 1:1，按节点编号序**。
- `flag`：`1` = 负重轮（参与逐轮贴地解算），`0` = 诱导轮/主动轮/托带轮（不参与）——
  IS-7 `0 1×7 0`、Maus `0 1×12 0`、T-54 `0 1×5 0`、E 100 `0 1×8 0`，SPHT `0 1 0 1 1 0 1 1 0 1 0`
  （三处 0 恰是 r≈0.15 m 的托带轮，见 §六.2）。
- `a`/`b`（两条 f32）：**= 行程上下限**（第二轮反汇编定案，见 §六.2：`a` 上行/压缩、`b` 下行/下垂）。观测：IS-7 全体 `0.13/0.13`；
  Maus 首轮 `0/0`、其余 `0.0406/0.0493`（含末位 flag=0 的主动轮带值）；
  T-54 同车两值 `(0.0723,0.0542)` 与 `(0.1238,0.0542)` ⇒ **逐轮 authored**，不是每车常数。
  普查（729 辆/7761 条）：`a` ∈ [0, 0.175]、`b` ∈ [0, 0.20]、中位各 0.07 m（厘米级行程）；
  SPHT（11 轮/侧）是最干净的样本：`flag` 与模型几何逐一对上（见 §六.2）。
- 空块合法：`other/Oth10_WarDuck` 的 `wheels` 为空（0 记录）而模型仍有轮节点 ⇒ 消费端
  必须 fail-closed 按"无 authored 悬挂"处理。

**`leftTrackChains` / `rightTrackChains`：`{ 段序号: 折线 }`，折线每点 12 B = `{f32 x, f32 y, u32 flag}`**

- 点数 ≈ 59–63（IS-7 63、Maus 61、T-54 62、E 100 61）；`x` 沿车长（模型系，m）、`y` 为高度（m）。
- `flag` 分布与小轮/托带位置相关（Maus 4/61、IS-7 2/63、SPHT 见 §六.2）——语义仍未定。
- 左右两份**不保证相同**：120 辆抽样中 89 辆逐字节相同、29 辆不同（非对称模型）；
  段序号 key 左右集合一致，多段（>1 个 key）在抽样里 1 例。
- 旧式 schema：85/764 辆用**单数键** `leftTrackChain` / `rightTrackChain`（字符串，非 map），
  645/764 用复数 map。两种键都在二进制里有（`leftTrackChain` ×3、`leftTrackChains` ×2
  处字符串），客户端两代都读。
- 这份折线就是履带的**静止形状**：`Suspension::GetTrackChain`（0x324241c）取用，
  悬空段（`Suspension::HangingSegment`）再按 `trackBendingInfo/trackLayingInfo` 弯曲/铺放。

### 3.3 覆盖（本机 764 辆 Parameters 普查）

| 项 | 数 |
|---|---|
| yaml 总数 | 764 |
| 有 `suspension:` 块 | **730**（无块 34） |
| `enabled: false` | 1 |
| 单数键 `leftTrackChain` | 85 |
| 复数键 `leftTrackChains` | 645 |
| 缺履带链 / 空 `wheels` | 0 / 1（WarDuck 空块） |

## 四、勘误：旧结论「悬挂参数数据源三处全查空」不成立

此前的会话结论把「车辆 XML / 模型 sc2 / Parameters yaml」三处判为都没有悬挂参数。
**对本条勘误**：Parameters yaml 里 `suspension:` 块是完整的（730/764，含逐轮数组与履带折线）；
XML 与 sc2 侧确实没有（XML 是物理/经济参数，sc2 只有节点）。旧结论可能只查了逐轮
**几何**（半径/连杆/枢轴）——那部分确实不在数据里，由 `SuspensionGenerator` /
`SuspensionUtils::GetSuspensionWheelsInfo` 从模型现算。**实现上按新口径**：
「用现成数据（车轮数组 + 履带折线 + 弯曲/铺放系数）驱动逐轮贴地/履带形变，
几何由模型节点现算」。

## 五、未定项（第二轮后）

**已定案**（第二轮，详见 §六）：`wheels` 的两条 f32 = 行程上下限；`flag` = 负重轮/非负重轮；
逐轮解算与回写公式；轮自转机制与相位/花纹滚动的量纲；履带 = chunk 实例化 + 链路折线
（垂弧/包轮/铺地骨架与 `lengthPower` 幂次）。

仍缺（按对保真度的影响排序）：

1. **逐轮 `wheelInfos`（16 B/条）的构造处** —— 半径到底从哪个几何量算（骨骼？包围盒？）。
   影响：我们自己从模型算半径的口径；也是自转系数 `k` 的候选来源。
   入口：`SuspensionUtils::AddSuspension` 邻域（VA 0x7028b0 → 0x703220 → 0x709a50 /
   0x704800 / 0x708260 / 0x7053b0 / 0x7080d0）。
2. **自转系数 `k` 的构造**（第 7.2 节的记录 `+0x08`）：候选 `k = 1/r_i`（逐轮半径不同
   ⇒ 逐轮 k 不同，物理上必须如此才能让花纹与地面同速）。入口：写该 16 B 记录的代码
   （从 0x780ed0 的读侧反查）。
3. **`chunkOffset` 的写入者**（履带花纹逐帧滚动的驱动量）。已知每米 V 变化 =
   `chassis.textureScale / chunkLength`、`chunkOffset` 是 material 属性（VA 0x82de82 设置、
   0x82ddc1 读 `component+0x20`），但**全二进制找不到该字段的浮点写点**（`.data` 尾部为
   零填充，静态读不到初始化）。入口：`SuspensionTrackComponent` 的 vtable 槽位、或
   `0x83edf0`/`0x83ecc0` 一类"无直接 caller 的虚 Process"。
4. **`trackLayingInfo` 五项落位**（bendingFactor / lengthPower / pointCountPower /
   pressurePower / primaryPower）：Laying 的流程已确定（接触树 → 权重表 0x836360 →
   铺放 0x8366e0 → 段内 max），但公式项未逐一对上。
5. **射线语义**：`BulletRaycastManager::RayTest`（VA 0x74fc00）是否含静态物件（房子/残骸/
   桥）。影响：前端用高度场近似时，桥/残骸附近的差异大小。
6. 小项：`c = −fashion[+0x10c]·0.008` 的含义；`+0x29`/`+0x2a` 标志的精确定义
   （"本帧着地/上帧着地" vs 纯可见性，两份报告不一致）；履带折线 flag 语义
   （§三 3.2）；`TankSuspension`/`EnhancedSuspension`/`Vehicle Suspension`/`wheel_L_%.2d`
   等字符串职责（候选：模块名、状态实体、旧命名）。

**方法备注**：`.?AVSuspension*` 的 TypeDescriptor→COL→vtable 走链在本 build 不成立
（§一 旧记录）；有效路径是 **`类::方法` 断言串 xref**（`pe.find_abs_ptr`：搜 `.text` 里内嵌的
绝对 VA = 0x400000+RVA）＋沿调用边展开；本轮的局部工具留在
`tmp_analysis/re/`（`pe.py` / `annot.py` / `tinfo.py` / `imports.py`，会话本地产物）。

---

## 六、处理链还原（第二轮，2026-10-10 三路并行反汇编）

记法与 §一 相同：VA = 镜像地址（0x400000 + RVA）。

### 6.1 总览：四个写者，各司其职

| 谁 | 写什么 | 触发 |
|---|---|---|
| 车体（回放记录位姿） | 整车根变换 | 回放数据 |
| **负重轮系统** `SuspensionWheelsSystem` | 逐轮骨骼的**位置 z**（竖直平移） | 逐帧（有轮可见时） |
| **车辆视觉状态系统**（自转在 0x780ed0） | 逐轮骨骼的**旋转块**（绕局部 X 自转） | 逐帧 |
| **履带系统** `SuspensionTrackSystem` | 链路折线 + material 属性（实例化链） | 逐帧 |

同一骨骼两个写者（z vs 旋转块）互不覆盖 —— 前端照这个分工做，最接近客户端结构。

### 6.2 负重轮：行程夹紧 + 限速的竖直平移

- 数据：`SSuspensionParameter` 布局 `+0x4 enabled` / `+0x8 wheelsReactionSpeed` /
  `+0xc chunkPrototypeExtending` / `+0x10 vector<12B>`；`RetrieveBinaryProperty`
  （0x3242ae4）是**纯 memcpy**：base64 → 断言末字节 0x0C → 断言长度 %12 == 0 → 拷贝，
  **不解释字段** ⇒ 语义全在消费侧。
- **`{u32 flag, f32 a, f32 b}` 定案**（`GetSuspensionWheelsInfo` VA 0x709d60 的输出记录，
  44 B/条）：
  - `a` = 上行（压缩）行程、`b` = 下行（下垂）行程 ⇒ 夹紧区间 `[pz − b, pz + a]`
    （`out+0x10 = pz − b`、`out+0x14 = pz + a`）；
  - `flag` = 1 负重轮 / 0 非负重轮（不参与贴地解算）。SPHT 11 轮/侧样本：
    `0 1 0 1 1 0 1 1 0 1 0` 与模型几何逐一对上（三处 0 = r≈0.15 m 托带轮，
    首 = r 0.24 诱导轮、末 = r_x 0.315 主动轮）；
  - 逐轮记录：`+0x00..0x08` 挂点（骨架骨骼 `+0x10`）、`+0x0c` 半径
    （`1.0f / wheelInfos[+8]`）、`+0x10/+0x14` 下/上极限、`+0x18` 目标 z（逐帧覆写）、
    `+0x1c` 原始 pz、`+0x24` 骨骼索引、`+0x28` flag、`+0x29/+0x2a` 标志（语义见 §五.6）。
- 逐帧（本体 0x8344a0，解算 0x837c90，回写 0x836060）：
  1. 逐轮**点-视锥剔除**（世界挂点 + 半径 + 0.01）；全不可见 ⇒ 整实体跳过（省算力）；
  2. **竖直射线 3 条**（轮底 + 两侧 ±45°，z 跨 `[(pz−b) − r − base, pz + a]`，两侧样本抬
     `(1−cos45°)·r`）→ 由实体世界变换映射、命中点再映回局部 → 取 **max**：
     `target = Tz + min(pz + a − Tz, maxHit)`（`Tz` = 挂点经「轮组→车体」相对变换后的 z）；
  3. 回写：`newZ = clamp(boneZ + copysign(min(|Δ|, wheelsReactionSpeed·dt), Δ), pz−b, pz+a)`，
     死区 0.001 m，**首次可见直接吸附**；
  4. **纯竖直平移**：只写骨骼 `+0x18`，四元数原样回写 —— 没有绕枢轴摆动、没有弹簧/阻尼项。
- 几何不 authored：挂点 = 骨架骨骼位置，半径 = `1/wheelInfos[+8]`（构造处见 §五.1）。
- `BuildWheelsRelativeTransform`（0x3239230）算的是**轮组实体→车体的静态相对变换**
  （每车一份，逐帧不变），逐轮挂点经它进局部系。

### 6.3 轮自转（另一系统）

- 载体 0x780ed0：逐轮 16 B 记录 `{+0x04 骨骼索引; +0x08 系数 k; +0x0c 累计角 θ}`；
  `θ = wrap(θ + s·k + c)`（wrap 到 ±2π），随后把 `(cos(−θ/2), 0, 0, sin(−θ/2))` 写进骨骼
  旋转块 ⇒ **绕骨骼局部 X 轴（车宽方向）自转**。
- `s` 逐侧：`s = (侧标志==0) ? sA : sB`，`sA/sB` = 车辆移动滤波器向量的两个分量
  （`fashion[+0xe4]`/`[+0xe8]`，左右纵向位移增量）⇒ 转向时内外侧差速、直行时相同。
- `c = −fashion[+0x10c]·0.008`（含义未定）。
- `k` 未钉死；物理约束指向 `k = 1/r_i`（逐轮半径不同 ⇒ 逐轮 k 不同）。
- 与 §7.1 的关系：悬挂只写 z、视觉只写旋转，两者独立叠加。

### 6.4 履带：运行时切块 + GPU 实例化 + 链路折线

- **表示**：`TrackChunkGenerator::GenerateChunk`（0x708260）从 authored 整圈履带批次切出
  **一份 chunk 原型**（长 = `chunkLength`，两端各外扩 `chunkPrototypeExtending`=0.1 掩缝），
  正常行驶时切到 chunk 批次（`SetupChunkRenderBatch` 0x82db10 设 `INSTANCED_CHAIN` +
  5 个 material 属性；`SetupOriginalRenderBatch` 0x835a00 是反操作）。每帧以 **instanced
  draw** 画 ≤64 个实例，顶点位置在**顶点着色器**里由链路折线摆（`instanced-chain.slh` +
  `materials-vertex-processing.slh` 的 `INSTANCED_CHAIN` 段；两份着色器本机已解出）。
- **每帧链路**：一串 **2D (x=车长, y=高)** 链点（8 B/点）+ `vector<Suspension::HangingSegment>`
  （16 B `{kind; i0; i1; aux}`）；流水线：逐轮接触/行程 → 按「点→轮」映射给链点加偏移
  （`chain[i] += wheelInfo.(+4,+8)`）→ 悬空段铺开（0x8366e0）→ **Bending**（相位 + 垂弧/
  包轮，0x833e60）→ **Laying**（接触树铺地，0x834090）→ 复位标志。
- 已读出的主干：
  - 相位：`phase += ±|v|·dt·(speed×100)`，clamp[0,1]，符号由 `frontDriveWheel == (v>0)` 定；
  - 垂弧（kind==4）：`p[i] += (t − t²)·k·n`（抛物线，t = 段内归一化、n = 段法线）；
  - 包轮（其它 kind）：`y = max(y, cy + √(r² − dx²) + param)`（只抬高，贴轮圆弧）；
  - 段系数：`f = (A + v)·dist^P`（`dist` = 段长、`P = lengthPower` **已确定**；
    `A`/`v` 的字段归属为高置信候选 upperMin/upperFactor/frontFactor/backFactor；
    `speed`、`frontDriveWheel` 两项**已确定**）；
  - 花纹 V：`V[i] = V[i−1] + (Δs/chunkLength)·scale`（`scale` = 逐车 `chassis.textureScale`，
    负值另加折返补偿项）⇒ 每米 V 变化 = `scale/chunkLength`；逐帧滚动 = `chunkOffset`
    （写入者见 §五.3）。
    **⚠️ 勘误（2026-10-10 实测）**：这条 `scale/chunkLength` 常数活在**客户端自己 shader
    重建的 V 空间**（`instanced-chain`：一个 chunk 一份原型，V = 弧长/chunkLength），**不能**
    直接套到导出网格的 UV 上——导出网格是**图集式 per-link V**（IS-7 实测：段内 V 沿弧长线性
    推进、图集行末换行，每 V ≈ 1.56 m）。实测两者比值 **3.4~19×（30 辆抽样、中位 4.5×）**：
    照搬会让花纹速度明显偏快（WotbTools"履带速度与前进速度不匹配"的根因）。消费侧正确口径 =
    **逐带实测沿带 dV/ds**（按弧长分桶取中位 + 剔除换行）× **底段走向**定符号（材料在接地段
    向后流；实测 30/30 辆底段点序为车尾→车头）。
    **实现注意（2026-10-10 实测坑）**：导出履带的 **V 量程跨多个 wrap**（KRV 底段一根四边形
    V 1.55→4.39 = 2.85 个 wrap），写 UV 偏移时**不能逐顶点取模**（会破坏 GPU 的线性插值、
    把长直段拉成"没有纹理"）——取模只能作用于**整带同一相位**（REPEAT 下整数平移观感等价）。
  - **导出履带网格的分辨率**（消费侧注意）：authored 履带是**长直段**低模（实测 KRV 底段
    一根 4.2 m 四边形、全带仅 24 个横截面；Maus 最长直段 7.6 m），而客户端履带是**逐 chunk
    （≈ 逐 link）** 的实例化条带。（曾据此按弧长细分导出网格，WotbTools 侧实测观感更差、
    已回退——长直段对"逐顶点形变随地形起伏"的限制是消费端的已知近似。）
    另：authored UV 是**图集式 per-link V**且绕带一周的缝是**整周期**（IS-7/KRV 实测缝两侧
    V 差 9.000 = 整数 ⇒ 相位连续，花纹在缝处不撕裂）。
  - 履带静止形状 = yaml 折线本身：IS-7 63 点、总长 15.14 m（与 authored 履带周长
    2×(6.70+1.07) ≈ 15.5 m 对得上）、平均段长 0.244 m ≈ `chunkLength`；SPHT 62 点/14.24 m/
    0.233 m。⇒ **`chunkLength` 可由折线与模型推出，不必额外 authored**。
- **接触来源**：履带**不采样地形**；负重轮着地判定用**碰撞平面集合测试**
  （`dot(n, wheelPos) + d > r`，VA 0xe5d7d0）；`Landscape::GetHeightAtPoint` 的调用者只有
  物理侧 8 角足迹探测（VA 0x6e5280 一带）⇒ 前端**不必**逐链点采样高度图，只需逐轮接触结果。

### 6.5 呈现层的其它事实

- `treadParameters`（forceManualWidth/manualWidth/manualSpace/texturePath）不是车体履带，
  而是 **TreadSystem 的地面履带印**（`DECAL_TREAD` 材质、Left/Right spawner、
  调试开关 `Draw Treads`）——此前"可能是车体履带贴图参数"的猜测排除。
- 断带形态 = `chassis_track_crash_L/R`；多段履带模型（4 辆）对应 yaml `leftTrackChains`
  的 map 多 key。
- 我们包内模型已带履带材质与贴图（如 `A178_SPHT_track_mtr`）⇒ 前端可直接在既有履带网格上
  做形变与 UV 滚动，**不必**复刻客户端的实例化架构（180 顶点/侧，逐帧 CPU 更新代价可忽略）。

---

## 七、实现含义（分级路径）

**本仓（数据面）——已落地（2026-10-10）**：`tools/export_tank_suspension.py` → 包内
`suspension/<tank_id>.json`（725/735 辆；回归 `tools/test_export_tank_suspension.py` 7 项）。
细节留档：
- 必须导：`wheels`（flag/a/b，≤22×12 B）、`left/rightTrackChains`（逐侧 59–63 点 ×8 B，
  两种键式都要处理，见 §三 3.2）、`trackBendingInfo`、`trackLayingInfo`、
  `chunkPrototypeExtending`、`chassis.textureScale`（754/764 辆有，含正值 ⇒ 两分支都要实现）。
- 可不导（前端从模型 GLB 现算）：逐轮半径/挂点、`chunkLength`（= 折线平均段长）、
  履带周长、履带网格。
- 规模：≈1.5–2 KB/辆 ⇒ 全量 ≈1.2–1.5 MB；读取走 `client_path()`（DLC 覆盖层）。
- fail-closed：无块（34 辆）/ 空 `wheels`（1 辆 WarDuck）/ 旧式单数键 → 回落"整台刚体"。

**WotbTools（前端）——T1/T2/T3 已接线（2026-10-10）**：求解器 `scene/suspension.js`（纯函数
+ 19 项单测）、接线与守卫见 WotbTools `docs/features/battle-playback.md`；T4 的各近似项
（铺地权重、`upperMin/front*` 落位、物理射线 vs 高度场、`chunkOffset`）仍是已知缺口。
分级（原始评估，留档）：

| 级 | 内容 | 依赖 | 风险 | 量级 |
|---|---|---|---|---|
| **T1 动作层** | ①逐轮自转 `θ += Δs_side/r`（绕轮节点局部 X）；②履带花纹 UV 滚动 `ΔV = Δs_side·textureScale/chunkLength` | 导出 `wheels`(a/b/flag)+`textureScale`；半径前端现算 | 低（不动几何） | 0.5–1 人日 |
| **T2 贴地层** | 逐轮竖直平移：`clamp` 到 `[pz−b, pz+a]`、`wheelsReactionSpeed` 限速、首帧吸附；地形用高度场逐轮采样（客户端是物理射线，桥/残骸处有差异） | T1 + `wheelsReactionSpeed` | 中（近结构物处近似） | 0.5–1 人日 |
| **T3 履带形状** | 折线形变：链点 = 静止折线 + 逐轮偏移 + 悬空段垂弧/包轮 + 铺地近似；变形既有履带网格（或按段实例化） | 导出 chains + bending/laying 字段 | 中高（`trackLayingInfo` 未落位 ⇒ 先用近似系数） | 2–4 人日 |
| **T4 客户端级** | 接触树/权重表全式、`chunkOffset` 驱动、`k` 构造与 `c` 项、静态物件碰撞 | 需再一轮 RE（§五 1–5） | 高（收敛不确定） | 3–5 人日 + RE |

**验收口径**：T1/T2 用纯函数单测锁（符号域、夹紧、限速、V 映射）+ 源码接线守卫；
T3 用"链点解算纯函数"单测 + 场景观感由用户判（按仓库规矩不自行截图验收）。
T1/T2 做完后，用户报障的 1:21 处：**轮**最多只能贴 5.6/8.4 cm（SPHT 的 a/b），
剩余 ~0.7 m 的缝要靠 T3 的履带形变（客户端那边同样如此——那个姿态下轮也够不到地面，
是"履带塌成折线 + 悬空段下垂"让它读起来不浮）。

**姿态链一致性核对（2026-10-10，配合"悬浮是否姿态滤波所致"的排查）**：
- **原始 vs 滤波**（探针：`wotb-agent combat <replay> --streams-json out.json`，逐实体
  type=10 原始流 `[clock,x,y,z,yaw,pitch,roll]`，未滤波）——SPHT @Port Bay：
  存活窗口 `滤波 pitch − 原始 pitch` 中位 **+0.02°**、p95 |Δ| **2.15°**、max 4.34°
  ⇒ 滤波只是对 ≈10 Hz 输入流的滞后/摆动，无系统偏差；报障时刻（t≈92.4）滤波 14.4° vs
  原始 11.0°（滞后 ≈0.2 s 追着 t≈92.0 的尖峰）。
- **把姿态换成原始值重算履带四角 gap**：t≈92.4 → **0.78 m**（滤波 0.93）；t≈92.2 →
  1.02（滤波 0.83）；存活窗口 max|gap| 中位 0.13 / p95 0.91 / max 1.31（滤波
  0.13 / 0.87 / 1.34）⇒ **0.9 m 与滤波无关**（滤波只给瞬时值 ±0.2 m 的摆动）。
- **我们渲染的就是客户端的渲染链**：yaw/pitch = 客户端自己的渲染滤波
  （`filter.rs` = WGVehicleFilter2/AvatarFilterHelper 移植）60 Hz 输出 → facet `pose_kf`
  折线（线性插值，上游口径 ≤2 cm/0.4°）；**roll = 原始 type=10 最近邻**（客户端滤波层
  不输出 roll），与本轮原始流逐点核对一致（max |Δ| 0.03°）；**复合次序 ZYX 内旋**
  （Ry(−yaw)·Rx(pitch)·Rz(−roll)，即先偏航后俯仰再横滚）——由数据验证：整局"记录 pitch
  vs 地形 pitch"中位 0.07° 只有在 ZYX 下成立（换 XYZ/ZXY 会在 yaw≈−80° 时把俯仰轴转进
  世界系，拟合立刻崩）；同族次序（YZX）对 gap 的影响 ≤0.03 m，异族次序可达 1 m。
- 记录两点：① **roll 是唯一未滤波通道**，原始流本身有 10 Hz 尖刺（t=92.0 处 0.2 s 内
  16.2°→2.8°），我们是"网格最近邻 + 线性插值"，客户端回放侧对远程车 roll 的插值口径
  未逐行核对 ⇒ 快瞬态上可能有 ≤半采样步的小差异；② 解析器对**所有车**（含作者车）统一走
  同一滤波（与"回放播放"语义一致；实机自车视角是本地物理、不滤波，两者在快瞬态上差
  ≈0.2 s——这一条不影响本仓切面口径，仅备忘）。

**模型摆放（姿态 → 模型）一致性核对（2026-10-10）**：
- **枢轴/偏移 = 与我们同口径**（数据验证，Port Bay/SPHT）：车体水平且地形平坦的 61 个采样点，
  四角 gap 均值 **−0.005 m**（模型 z=0 平面 ≈ 履带接地面，无常数抬升/下沉）；按姿态幅度分箱
  的"平均 gap"无单调趋势（0–6° 段 −0.02 m 量级、6–12° 段随"姿态 vs 地形失配"两向摆动）
  ⇒ **没有绕某个偏置枢轴旋转**（否则 gap 会随 (1−cosθ) 单调）——客户端把模型按
  记录位姿绕**模型原点**摆，和我们一致。
- **客户端侧的代码锚点**（供下一轮继续）：`VehicleTransformSystem`（源
  `…/Systems/VehicleTransformSystem.cpp`，字符串 RVA 0x324a5dc；`::AddEntity` 断言 RVA
  0x324a5f8 / VA 0x83c283，`::RemoveEntity` VA 0x8405c5；RTTI 名 RVA 0x3eb8854）。
  姿态 → 旋转的实现形态已定位：**逐轴四元数构造器**（wrap 到 ±2π、FLT_EPSILON 死区、
  半角 sincos；VA 0x8406a0 / 0x840c4e / 0x842152 / 0x866138 一族），把 `(x,y,z,w)` 写进
  **36 字节骨架节点的旋转块**并置脏位 `0x2000000`、节点标志 `0x2`——与轮自转写骨骼的形态
  完全相同（同一套"逐轴四元数 → 骨骼旋转块"机制）。常数：0.5（半角，RVA 0x32356e4）、
  2π（RVA 0x32356f0）、死区 FLT_EPSILON（RVA 0x3232a40）。
- **复合次序**：不靠代码判读下结论——用整局数据验证（记录姿态 vs 地形姿态中位 0.07°/0.33°
  只在 ZYX 内旋下成立，见上一条），即**结果层面**与客户端一致；代码层面逐轴构造器的
  调用次序（哪个轴先乘）未逐一钉死，标注为可选深化项。
- **唯一记录在案的姿态来源差异**：客户端**网络滤波层 roll ≡ 0**（`filter.rs` 注释：
  「视觉侧倾来自物理层」），即实机里远程车的可见侧倾由本地物理层给；回放器没有物理，
  我们取 **packet roll（发送端物理层的 roll）**——同源、整局与地形吻合 0.33° 中位，
  差异只可能在快瞬态（packet roll 有 10 Hz 尖刺）。

**地面口径一致性核对（2026-10-10，配合"悬浮是否地形不准"的排查）**：
- **数据同源**：客户端地形查询 `Landscape::GetHeightAtPoint`（VA 0xe553b0，本轮反汇编）读的就是
  `3d/Maps/<space>/landscape/*heightmap*.dvpl`（8 字节头 + 512² u16）——与我们
  `src/wargaming/map_assets.rs::parse_heightmap` 解的是同一份文件；两边都是"从高度图程序化
  生成地形网格"（包里没有 landscape 网格，只有静态布景 GLB）。
- **插值同式**：客户端 = 边界裁剪 + `fx=(x−xMin)/(xMax−xMin)·size` + **双线性**（4 角取值两次
  lerp）；我们 `sampleHeight` 也是双线性，但网格步长口径差 0.2%（`size` vs `size−1`）⇒
  水平最多差半格 ≈0.6 m，缓坡折算 5–6 cm、陡岸最多 ~0.3 m；实测本图本点两种口径的 y−h
  中位只差 5 mm。**建议顺手对齐**（几行改动），但不是任何米级现象的来源。
- **数量级证据**（Port Bay / SPHT）：车体水平（|pitch|,|roll|<1°，89 样本）时
  记录 y − 我们的 h = 中位 **+7 mm**、p95 +29 mm、max +39 mm；报障时刻四角 gap
  `[−0.06, +0.48, −0.35, +0.93]` —— **最近地的角 ≈ 0（始终在接触）**，最远的角 +0.93 m
  ⇒ 地形整体高度误差会让四角同向平移，"一角贴地一角翘 0.9 m"只能来自**姿态失配**。
  换采样口径（双线性/两种三角/最近邻）0.93→0.94→1.04 m，结论不动。
- **两类真实差异**（T2/T3 的已知近似，非本图报障点）：① 静态物件（桥面/码头/墙/残骸）不在
  高度图里——客户端轮子射线打到物件面，我们只采地形；全图 y−h 的 +0.4 m 尾部大概率混着
  此类场合，若要做到"履带贴桥面"需加一层布景三角面查询。② 512 网格 1.17 m 分辨率在陡坎处
  （t=110.9 实测：车体水平而坡肩落在一角下方 → 该角"埋"1 m）——几何/摆法问题，T3 的
  逐轮/逐带解算天然吃掉。
- 36 图 `worldBounds` 全居中且正方（普查）⇒ 前端"只取 span、按居中"的采样假设在本包成立。

**不做/超范围**：地面履带印（TreadSystem）、断带物理、悬挂的弹簧/阻尼（客户端本来就没有
——§六.2 第 4 条：没有弹簧阻尼项，只有行程夹紧 + 限速）。
