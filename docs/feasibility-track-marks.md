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
  - `data/tank_tread/<tank_id>.json` → 包内 `tread/<tank_id>.json`（**311 文件 / 79 KB**，
    additive；无块的 424 辆**不产文件**，fail-closed）。schema：

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

## 三、前端路线（WotbTools，建议口径）

- **生成**（纯函数，可单测）：沿 `posAt/yawAt` 位姿流扫描，车辆移动每满
  `manual_space`（缺省 1.0 m）emit 一段；速度门控（静止不铺）；`coverage[]` 空洞断链；
  左右两条带，横向偏移 = 履带中心距（suspension 折线 x 范围或模型履带盒）。
- **渲染**：逐车动态合并几何（每段 4 顶点、两侧各 1536 段环形覆写），段四边形**逐角
  独立采样高度贴地**（+12 cm 抬升盖住全景视距下 LOD 网格 vs 真值场 5~15 cm 的残差，
  首版单平面曾成片被网格盖住；段长 ×1.25 重叠盖转弯缝）；材质 = 不受光
  （`MeshBasicMaterial` + tread 贴图，alpha blend）——全画质档可用。
- **seek/回退语义**：印记是路径累积态，但**由回放数据确定性生成** ⇒ seek 一律
  "从 t=0 重算到 T"（20 车 × 4200 网格点扫描 <10 ms，比增量回滚简单且无漂移）；
  参照 `updateDestructibles` 的 rewind 分支挂进主循环。
- **开关**：回放设置加 toggle；默认值待产品定（客户端默认关）。
- **量级**：约 500–1500 m/车/场 ⇒ 300–1000 段/车 ⇒ 20 车全场 6k–2 万实例
  （单 InstancedMesh 一次 draw call），内存与帧开销可忽略。

## 四、已知近似与边界（fail-closed 记录）

1. **客户端 spawner 算法未逆向**：TreadSystem 的生成时机/寿命/淡出只有
   `treadParameters` 与 `DECAL_TREAD` 材质两个锚点，逐帧铺放逻辑没有反汇编；
   本实现的铺放节奏（authored `manualSpace`）与条带几何是**依据作者参数的近似**，
   与客户端逐像素不一致。
2. **印记寿命/池上限是自定产品参数**（已落地：环形缓冲 1536 段/侧 + 回放时钟
   60 s 全显 → 30 s 线性淡出 → 消失，用户对照客户端提出淡出行为后补；客户端真实
   时长未逆向）；贴图/宽度/间距三样有客户端依据，与 2026-10-09 撤除的"全自创"
   近似已不是同一性质。
3. **无 `treadParameters` 的 424 辆**：建议回落 = 国别默认贴图（`<Nation>_01_tread`，
   13 张里国别齐备）+ 宽度从模型履带盒量 + 间距 1.0 m；此回落**无客户端 authored
   依据**（客户端真实回落行为未逆向），是产品近似——若要严格 fail-closed 则
   该 424 辆不画印（同场观感不一致，不建议）。
4. **贴地高度只有地形**：桥面/码头/残骸上的印记会贴到地形面（客户端轮子射线打结构面）
   ——与悬挂 T2/T3 的既有已知差异同源，见
   [tank-suspension-client-re.md](tank-suspension-client-re.md) §地面口径一致性核对。
5. `force_*=false` 的车辆（如 IS-7 的间距）：authored 值可能只是客户端派生算法的
   输入之一，直接当定值用是近似。

## 五、实施状态

| 侧 | 状态 |
|---|---|
| 本仓数据面 | ✅ 导出器 + 打包接线 + 回归 6 项 + 本地包增量 + manifest 重算（本文档同 commit） |
| COS 生产面 | ⬜ 未上传（常规增量 `--only tread/`，与下次资产面发布合并亦可） |
| WotbTools 前端 | ⬜ `scene/treadMarks.js` 纯函数 + `playbackScene` 接线 + 开关 + `wotbVersion` bump（同仓 PR 规矩）；观感由用户在测试页验收 |
