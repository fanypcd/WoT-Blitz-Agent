# 履带印（TreadSystem 地面碾压痕）可行性定案与数据面实施

> 2026-10-11。回答"3D 回放里坦克开过后地面上的履带印能不能做"；结论：**能**——
> 客户端 TreadSystem 的作者数据（印记参数 + 贴图本体）**全部在可得数据面**，2026-10-10
> [map-render-align-audit.md](map-render-align-audit.md) D2 "不做"定案的重做前置
> （"先定位客户端贴图"）对碾压痕这一支**已满足**（弹坑/血迹仍无源，维持不做）。
> 数据面导出已落地（本仓）；3D 呈现归 WotbTools 前端（本仓不出前端代码）。
> 2026-10-11 用户裁示：做，两仓分支落地，测试页观感由用户验收。

## TL;DR

1. **贴图在数据面**：`Data/3d/Tanks/Common/images/` 有 **13 个** `*tread*.dx11.dds.dvpl`
   （256²，DXT3/DXT5，软边暗色泥痕条带，非单节履带板图案）；逐车 yaml 的
   `treadParameters:` 块（**88/764** 辆有、5 字段形状完全一致）给出
   `texturePath` + 印记条带宽度/间距。此前 D2"贴图不在数据面"的判断对 **geo-decal
   弹痕**成立、对**碾压痕不成立**——后者是 `3d/` 资产，不是游戏层传入。
2. **驱动数据现成**：位姿 = `PlaybackData` 10Hz 网格 + `pose_kf`（7–10 点/秒/车）；
   贴地 = 前端既有 `sampleHeight` 高度场；左右带间距可从包内
   `suspension/<tank_id>.json` 折线或模型包围盒取。
3. **渲染路径有同构先例**：几何贴花（InstancedMesh 逐段四边形 + `sampleHeight` 贴地）
   与 destructibles（树倒）同一机制（逐帧写 instanceMatrix 已打通）；全画质档可用
   （`groundShaderMaterial` 只在 high/ultra 存在，几何贴花不受此限）。量级无压力：
   ~1.5 m 一段、20 车全场约 6k–2 万实例，一次 draw call。
4. **与"只保留客户端有实际依据者"的口径对账**：贴图、宽度、间距三样是客户端作者数据
   （2026-10-09 撤除的程序化近似正是缺这三样）；仍属自定的只剩**印记寿命/池上限**与
   **无 author 数据车辆的回落**两处产品参数，见 §五。
5. 客户端 Windows 默认档 `TankTreads=false`（默认关）——严格对齐 = 默认不画；
   做成用户开关、默认值由产品决定（本功能的存在本身即用户裁示的产品决定）。

## 一、客户端构成（可复现事实）

- **TreadSystem**：引擎侧地面履带印系统（`DECAL_TREAD` 材质、Left/Right spawner、
  调试开关 `Draw Treads`），见 [tank-suspension-client-re.md](tank-suspension-client-re.md)
  §6.5——该文此前把"地面履带印"列为不做/超范围，本文即其翻案记录（数据面部分）。
- **逐车参数**（`Data/3d/Tanks/Parameters/<nation>/<stem>.yaml`，读取经 DLC 覆盖层）：

  ```yaml
  treadParameters:
      forceManualWidth: true      # IS-7；宽度是否强制 authored
      manualWidth: 0.835616       # 印记条带宽（m）
      forceManualSpace: false     # 间距是否强制 authored
      manualSpace: 1.000000       # 沿路前进多少米铺一段
      texturePath: "~res:/3d/Tanks/Common/images/USSR_01_tread.tex"
  ```

- **普查**（764 个 Parameters yaml，2026-10-11）：**88** 个文件带 `treadParameters`，
  字段形状**完全一致**（5 字段全带，无缺省/空块/异序）；贴图引用去重 **9** 种
  （每国一张 `_01_tread` + `German_01_double_tread`）。按 tanks.pb 的 735 tank_id 展开：
  **311 辆**有块、424 辆无（一辆 yaml 对应多个 tank_id 变体）。
  `manualWidth` 全量范围 **0.260..1.342 m**。
- **贴图实体**：`~res:` 路径在数据面的落点是 `3d/Tanks/Common/images/<stem>.dx11.dds.dvpl`
  （`.tex` → `.dx11.dds.dvpl`，与坦克贴图同一映射规则）。目录下 `*tread*` 共 **13** 个：
  被引用的 9 种 + 回落候选 `Tread`（通用）/`Japan_01`/`Other_Oth50_STT`/`USA_01_double`
  （Japan 参数 yaml 零引用但贴图在场 ⇒ 客户端对无块车辆按国别默认取图的旁证，未逆向定案）。
- **贴图内容**（解码实测）：256² RGBA；`USSR_01_tread` = 横向软边暗色泥痕条带
  （alpha 列均值 @0/128/255 = **0/217/0**，纵向 114–149 基本均匀）——碾压痕是
  "压过的暗土"风格，不是镂空履带板；`German_01_double`/`Tread` 是双带变体
  （alpha 列 0/166/0/166/0 形）。解码链 = `decode_dvpl`（wotb_sc2）+
  `decode_dds`（export_tank_glb，自写 BC1/2/3/5）——两件都已在本仓服役。

## 二、数据面导出（已落地，本仓）

- `tools/export_tank_tread.py`：
  - `data/tank_tread/<tank_id>.json` → 包内 `tread/<tank_id>.json`（**735 文件**，additive；
    **全部车辆都产文件**——311 辆 authored + **424 辆国别主贴图回落**（`fallback:true`、
    `manual_width:null` 由前端从模型履带盒实测；2026-10-11 用户报"215b 比 Kranvagn 黑"
    后不再 fail-closed 留白）。schema：

    ```json
    {"tank_id":7169,"nation":"ussr","stem":"IS-7",
     "source":"3d/Tanks/Parameters/USSR/IS-7.yaml.dvpl",
     "texture":"tread/textures/USSR_01_tread.webp",
     "force_manual_width":true,"manual_width":0.835616,
     "force_manual_space":false,"manual_space":1.0}
    ```

    **只搬数据不做语义推断**：`force_*=false` 时客户端如何派生宽度/间距未逆向，
    flag 与 authored 值照原样透出。
  - `data/tank_tread_textures/<stem>.webp` → 包内 `tread/textures/`（**13 文件 / 983 KB**，
    DDS **无损**转 webp，保客户端像素；幂等可重跑，与逐车参数解耦）。
- 打包接线：`scripts/export_asset_pack.py` 新增 tread 段（紧随 suspension 段，additive）；
  本地包已增量补入并 `--refresh-manifest` 重算（**5288 文件 / 4286.1 MB**，
  tread 计 324 条目）。**COS 未同步**——生产可见需按常规流程上传增量
  （`--only tread/`）。
- 回归：`python tools/test_export_tank_tread.py`（6 项：块解析/fail-closed/贴图路径形状/
  产物自洽/真实客户端抽样含 DDS alpha 软边断言）。

## 三、前端路线（WotbTools，已按客户端槽位模型落地）

- **生成 = 客户端槽位模型（2026-10-12 反汇编定案，同日二次补证）**：每侧一个**固定环
  50 槽且含活戳**（`TreadSystem` 铺放 `0x83d410` 的 `(i + 0x32) % 0x32`；落锚函数
  `0x727b80`（由 `0x843030` 门控调用）给出侧状态 `{ head, write, count 封顶 50 }`，槽
  stride 0x98——一次落锚 = 新触地点写 write 槽 → `count=min(count+1,50); head=write;
  write=(write+1)%50`，**active 槽就是 write 槽、没有独立第 51 槽**），active/write 槽
  持一枚「上次落锚点 → 当前触地点」的扫掠戳；车辆移动时**前沿逐帧拉伸**（活戳末端 =
  当前位置），与锚点距离满 `manualSpace`（带 1e-9 容差防浮点欠冲）**定型并推进 write**
  （`0x83d4ca-0x83d507` 距离门控同款）⇒ 戳长 ∈ [space, space+步长]、无整数边界。
  **激活门 = 0.3 m**（`0x843030` 门控常量 `0x3632a54`，伴随沉积门 0.05 `0x363a068`）：
  净位移 < 0.3 m 的小范围蹭动不建活戳；环满时活戳顶掉最老戳（head 槽）。
  周期上每车还有一个 tread 实体（池上限 20，"Too many tread entities were created,
  limit:" @0x83ddad），逐帧把整批戳记录世界化并**整批替换**（`0xe79050`）。
- **渲染**（终版口径）：逐车动态合并几何（槽位 = 环 50 + 活戳槽 1，左右交错 102 个四边形）。
  **每戳 = 扫掠四边形**——「锚点横截面 → 当前横截面」（横截面 = 过中心线、垂直于**当时
  车体朝向**的宽度线），**相邻戳共享整条接缝边 ⇒ 转弯天生无缝**（客户端
  `deferred-decal-vp.sl` 的剪切 `y→y·z+w` 即此几何）。四角**逐角独立采样高度贴地**
  （+12 cm 抬升盖住 LOD 网格 vs 真值场残差）。**淡出窗 = 客户端引擎常数 5.45 / 7.62 s**
  （反汇编 `0x4010f94 / 0x4010f98`；着色器 `fadeOutTimeStartEnd` 的
  `1 − smoothstep(start, end, age)` 同源推断）。材质 = 不受光（`MeshBasicMaterial` +
  tread 贴图，alpha blend，`DoubleSide`）。**已知缺口**：①按相机距离淡出
  （`treadsNear/FarFadeDistance`，值未定位）；②每图调色/混合（`mtsc.color` /
  `mtsc.treadBlending`，在地图 .sc2 里、未提取）；③戳记录→decal 实例的提交路径
  （`0xe6dfd0` / DeferredDecal 管理系统的连接细节）。**不透明度已按客户端口径**取消全局
  压暗（2026-10-12 用户裁示）：alpha = 贴图 alpha × 淡出系数 × per-stem 归一化（后者是
  用户裁示的跨贴图"统一深浅"），与客户端着色器一致；近距满景下可能因"距离淡出/调色分支
  未复刻"而略深——缺口的直接后果。**重复碾压在 7.62s 窗口内的短暂 alpha 重叠是既定行为**
  （客户端无空间去重的反汇编证据；公开 DAVA 引擎 GeoDecal 亦为普通 alpha blend，实际
  blend 规则待 `mtsc.treadBlending`/0xe6dfd0 逆向定案）。
- **seek/回退语义**：印记是路径累积态，但**由回放数据确定性生成** ⇒ seek 一律
  "从 START 重算到 T"（0.1 s 固定网格片喂样，seek 重算与逐帧连续播放逐值一致）；
  AoI 空洞/位姿大跳 = 断链重锚但**保留已铺历史**（按出生时刻淡出），seek/重开/参数变化 =
  全量重建（由下一 anchor 时刻与最后历史戳的比较区分）。
- **开关**：回放设置加 toggle；默认值待产品定（客户端默认关）。
- **量级**：活戳上界 = 51/侧×车（环 50 + 活戳 1），20 车全场 ≤ 2k 四边形——比旧模型
  （1536/侧 + 最多 1.5 km 轨迹）小一到两个数量级。

## 四、已知近似与边界（fail-closed 记录）

1. **客户端铺放算法已部分逆向（2026-10-12，`wotblitz.exe` 32 位 / base 0x400000）**：
   已定案 `TreadSystem::Process` @0x83ecc0 → 每车 `0x83f820` → 每侧铺放 `0x83d410`
   （每侧固定环 50 槽 `(i+0x32)%0x32`、锚点/距离门控、戳记录 0x60 字节、时间门控常数
   5.45/7.62 s）→ 每车实体整批替换 `0xe79050`（矩阵世界化 + AABB）；tread 实体池上限
   20（注册 `0x83bbf0` / 分配 `0x83dbf0`）。**仍未定**：戳记录 → decal 实例的提交路径
   （`0xe6dfd0` 与 DeferredDecal 管理/atlas 系统的连接）、距离淡出近远值、
   每图 `mtsc.color`/`treadBlending` 实际值（地图 .sc2 反射数据未解析）。各值以反汇编
   证明的为准，未证明的一律保留为缺口而不是猜数。
2. **印记参数现状（2026-10-12）**：几何（扫掠四边形+剪切）、环 50 槽、锚点/重锚语义、
   淡出窗（5.45→7.62 s smoothstep）、**不透明度口径（贴图 alpha × 淡出 × per-stem 归一化，
   无全局压暗）均有客户端证据或用户裁示对齐**；仍是产品自定的只剩**无作者数据车辆的回落**
   （国别主贴图 + 模型实测宽度 + 间距 1.0 m）与 **per-stem alpha 归一化**（用户裁示的
   跨贴图"统一深浅"，客户端无此步）。旧列（寿命 10+10 s、池 1536/侧、全局压暗 0.8、
   "段长 1.0 精确相接"）已随槽位模型全部退役。
3. **无 `treadParameters` 的 424 辆**（**已落地，2026-10-11**）：回落 = 国别主贴图
   （`<Nation>_01_tread`）+ **宽度从模型履带盒实测**（前端 `beltW`，对应该类车
   `forceManualWidth=false` 的"从履带几何现算"语义）+ 间距 1.0 m；客户端对无块车的
   真实回落行为未逆向，国别主贴图是"同族深色"的最佳可得代理（产品近似）。
   另：13 张贴图 alpha 按全库中位**归一化**（`tread/textures.json` 的 per-stem scale，
   钳位 [0.65, 1.8]），前端乘入不透明度——authored 贴图间密度差 1.7~4×（USSR 127.6 →
   USA 75.3 → 通用 64.4，RGB/色调本已一致），按用户裁示"统一颜色深浅"归一。
4. **贴地高度只有地形**：桥面/码头/残骸上的印记会贴到地形面（客户端轮子射线打结构面）
   ——与悬挂 T2/T3 的既有已知差异同源，见
   [tank-suspension-client-re.md](tank-suspension-client-re.md) §地面口径一致性核对。
5. `force_*=false` 的车辆（如 IS-7 的间距）：authored 值可能只是客户端派生算法的
   输入之一，直接当定值用是近似。

## 五、实施状态

| 侧 | 状态 |
|---|---|
| 本仓数据面 | ✅ 导出器 + 打包接线 + 回归 6 项 + 本地包增量 + manifest 重算（本文档同 commit） |
| COS 生产面 | ✅ 已上传（`--only tread/`，749 文件 / 1.2 MB，随 v0.4.4 pin bump 发布） |
| WotbTools 前端 | ✅ 槽位模型落地（`scene/treadMarks.js` 环 50 槽 + 锚点/提交 + 前沿拉伸 + 5.45/7.62 s 淡出窗；`playbackScene` 活戳槽/固定全窗口/显式隐藏）——2026-10-12 反汇编定案后**全面改为客户端方式**；观感由用户在测试页验收 |
