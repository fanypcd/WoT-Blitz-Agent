# 可行性报告 B：用本机客户端解包自行生成坦克 `model.glb` / `collision.glb`

> 2026-10-01 产出的评估报告。语料：本机 WoT Blitz 客户端 **11.20.0**，目标契约取自
> 本地已全量缓存的 BlitzKit 产物 `data/cache/models/{tank_id}/`（735 个目录）。
> 复核方式为**对抗式重验**：独立重写 GLB 读取器与比对器、**27 辆抽样泛化测试**、
> **735 辆全量路径覆盖**，并以证伪立场检查上一轮原型结论。脚本见 §9。
>
> **2026-10-03 整理注记**（本文为历史评估，实施结果与修正以
> [local-model-export.md](local-model-export.md) 为准）：
> ① **已实施**：`tools/export_tank_glb.py` 落地，验收口径从"按可达节点求和"收紧到
> **逐字节 + 节点顺序**——`collision` **735/735**、`model` **733/735**；§5.0 的 35 辆
> "4 类规则缺口"已由 `LodComponent` 判据、全批次导出等规则收敛（见该文 §3）。
> ⚠️ 2026-10-10 起其中 **23 辆为有意分歧**（静态变换烘焙 / 状态过滤），等价数变
> **710/735**——见 [local-model-export.md](local-model-export.md) §5.1。
> ② **§3.3 的槽位指派结论已被逐通道实测推翻**：BlitzKit 的 `normal` 实为 legacy
> `images/<T>_NM`（DXT1，非"miscMap.R 灰度"）；`metallicRoughness` 的 **G** 实为
> `baseRMMap` 通道 0（粗糙度，非"legacy normalmap"）；`occlusion` 与 `miscMap.R`
> 全量 1011/1011 逐像素一致（非"baseRMMap.R"）；真正无客户端来源的是 BlitzKit 的
> MR **B 通道（金属度位）**。通道语义判别与摆放见 local-model-export.md §4.1/§4.2。
> ③ §4 的"验收口径待决策"已定为 **semantic**；§7 的开工量估算随实施完成而过时。

## 0. 结论

**几何层面：可行，已完成全量 735 辆验收**——`collision.glb` **735/735 完全等价**，
`model.glb` **700/735 完全等价**，剩余 35 辆已归类为 4 类可收敛的规则缺口（§5.0）。
**贴图层面：槽位映射已完整逆向**——客户端 8 槽的语义、BlitzKit 的实际指派
（4 槽中 3 槽在 PBR 语义上是坏的）、3 槽的精确复现路径都已确证（§3.3）；
材质解析覆盖率实测 **648/735（88%）** 用一条直白规则即可解出（§3.4）。
剩下两件事：① **验收口径决策**——走"语义正确"路线（推荐，工作量小且效果更好），
还是"与 BlitzKit 逐字节一致"（需继续追 occlusion 的两个通道，不建议）；
② 老式车（429 辆）的**材质 `parentMaterialKey` 解析链**要补。

综合工作量 **约 4–7 人日**（走语义路线；构成见 §7。与早前估计的差异：路径解析风险被高估、
贴图工作量与材质解析链被低估、且 model.glb 几何还需收敛四类规则缺口）。

三条关键结论：

1. **客户端侧不存在找不到的坦克**：735/735 的 `blitzModelPath`、`.sc2`、`.scg`、
   `collisionMesh` 全部存在，**0 失败**，且**不需要**任何模糊文件名匹配（见 §4）。
2. **前端契约不依赖蒙皮**：目标 GLB `skins=0 / animations=0`，前端按**节点名正则**寻址并
   显式排序，`frontend/src/scene/` 下**没有任何 `.children[` 位置索引访问** →
   转换器不必实现骨架/动画。
3. **原型存在一个只在抽样时暴露的静默 bug**：LOD0 批次选择在 27 辆中有 **6 辆选错**
   （成因与修法见 §5.2）——这正是"只验 2 辆车不够"的证明。

## 1. 目标契约（替换的验收标准）

以 E-100（9489）为例，从 BlitzKit 产物逆向出的契约：

| 项 | `model.glb` | `collision.glb` |
|---|---|---|
| 规模 | 73 节点 / 34 mesh / 2 材质 / 8 贴图（`image/webp`） | 33 节点，每节点自带 mesh |
| 节点命名 | 客户端 `.sc2` 实体树直译：`hull`、`turret_02`、`gun_04`、`gun_04_mask`、`chassis_track_L`、`chassis_wheel_L_01`…，mesh 挂在名为 `0000`/`0001` 的批次子节点上 | **扁平**，命名 `<part>_armor_<N>`（`hull_armor_1..13,16`、`turret_02_armor_*`、`gun_04_armor_1..5`） |
| mesh 属性 | `POSITION`/`NORMAL`/`TEXCOORD_0` + 索引 | 仅 `POSITION`/`NORMAL`，**无材质无贴图** |
| mesh 名 | 恒为 `RenderBatch` | — |
| 变换 | **所有节点 identity**（姿态由前端运行时写矩阵）⚠️ 2026-10-10 起超出该契约：**非姿态节点**的静态变换烘进顶点（节点仍全 identity），见 [local-model-export.md](local-model-export.md) §3 铁律 14 | identity |
| 材质 | 名 = 材质继承链的**根** `materialName`（`E_100_mtr`、`G56_E_100_upd_track_mtr`），含 `alphaMode:MASK`/`alphaCutoff:0.03`/`doubleSided` | 无 |
| `userData` | **为空**（装甲分类由前端用节点名 + `models.pb` 厚度合成） | 同 |

- **坐标系**：GLB 坐标 = 客户端 DAVA 世界系原样（z 上、单位米），无缩放无旋转；
  前端用 `applyModelTransforms`（`rotation.x=-π/2`）与 `qFrame = Ry(π)·Rx(-π/2)`、
  `worldMetersPerUnit = 1` 对齐到回放场景系。
- **被排除**的源节点（BlitzKit 三套：`model` / `collision` 均不含）：
  `HP_*` 特效锚点、`chassis_track_crash_*`、`chassis_chassis_*`。
- **`collision.glb` 的装甲板切分不是猜的**：客户端碰撞 SCG 的**每个顶点自带装甲板号**
  （顶点流末尾那个 float），且客户端三角序本就按板分组；按板取连续 run 即得与 BlitzKit
  完全相同的节点集合与顺序（已独立复算）。

## 2. 客户端来源

| 内容 | 路径 | 说明 |
|---|---|---|
| 车辆模型 | `3d/Tanks/<Nation>/<Name>.sc2` + `.scg` | **与地图同一容器**（SceneFileV2 + SCPG），可直接复用 `tools/wotbtools/wotb_sc2.py` / `wotb_scg.py` 与 `tools/export_map_glb.py` |
| 碰撞网格 | `3d/Tanks/CollisionMeshes/<nation>-<Name>.sc2` + `.scg` | 4 个部件节点（`hull`/`turret_NN`/`gun_NN`） |
| 权威路径 | `3d/Tanks/Parameters/<nation>/<Tank>.yaml` 的 `resourcesPath.blitzModelPath` / `.collisionMesh` | **唯一权威**：按 tank_id 对应的 `<dev>` 命名的 yaml 直接存在（§4） |

> ⚠️ 车辆 XML 里 `<models><undamaged>vehicles/…/lod0/Hull.model` 是**遗留逻辑路径，
> 11.20.0 的磁盘上不存在**。不要按它去找模型。

**顶点流格式**（已实测）：可视化 `.scg` 为 `vertexFormat=395`、**stride 56 B = 14 floats**
（`pos(0-2) nrm(3-5) uv(6-7) tangent(8-10) binormal(11-13)`）；碰撞为
`vertexFormat=519`、stride 32 B，末尾 8 字节的第 2 个 float = 装甲板号。

**材质差异**（与地图相比的额外工作）：坦克材质的贴图与属性藏在 `configArchive_0..N`
（皮肤变体 `Default`/`Skin`/`Skin2..4`）里，`export_map_glb.MaterialLibrary` 读不到，
需先下钻；`parentMaterialKey` 继承链的**根**才是材质名。

## 3. 原型复核结果（`tmp_glb_feasibility/export_tank_glb.py`，~450 行）

### 3.1 `model.glb`（E-100 / IS-7）

| 校验项 | E-100 | IS-7 |
|---|---|---|
| 节点数 / mesh 数 | 73==73 / 34==34 | 58==58 / 27==27 |
| 节点树 DFS 遍历序列 | **完全一致** | **完全一致** |
| 同级兄弟顺序差异组数 | **0** | **0** |
| 顶点总数 / 索引总数 | 10974==10974 / 24600==24600 | 8572==8572 / 18000==18000 |
| `POSITION` 原始字节相等 | 34/34 | 27/27 |
| `TEXCOORD_0` 原始字节相等 | 34/34 | 27/27 |
| `NORMAL` 原始字节相等 | **32/34** | 27/27 |
| 索引**数值**相等 | 34/34 | 27/27 |
| 索引 `componentType` | **uint16(5123) vs uint32(5125)** | 同 |

两项否掉的"逐字节"表述（我独立复算确认）：

- **`NORMAL` 差异只是 NaN 的符号位**：2 个 mesh（`turret_02_nc`、`turret_02_nc_skin4`）
  各 3 个 float 不同，目标 `0x7fc00000` vs 原型 `0xffc00000`。
  **客户端源数据本身含 `0xffc00000`**（`E-100.scg` 内 54 处），
  是 BlitzKit 的 JS 管线把 NaN 规范化成了 `0x7fc00000`。原型补一步 NaN 规范化即可逐字节相等。
- **索引不是逐字节相等**：数值相同，但 BlitzKit/glTF-Transform 产出 **uint16**，原型产出 **uint32**。
  属编码差异，可用"按最大索引自适应选择 uint16"修掉。
- 附带：原型每 mesh 写 4 个 accessor、144 bufferView，BlitzKit 为 136/43（目标做了相同 accessor 去重）。
  仅结构差异，数值一致。

### 3.2 `collision.glb`（E-100）

**全部证实**：33 个节点，**名字集合与顺序均相等**，顺序与从
`CollisionMeshes/germany-E-100.{sc2,scg}` 独立重算的连续板号 run 一致；
每节点 `POSITION`/`NORMAL`/索引**数值相等 33/33**，三角数 33/33，
连 accessor 共享形态都相同（两侧均 8 个 accessor、重数 5/5/13/13/10/10/5/5）。
（索引编码的 uint16/uint32 说明同上。）

### 3.3 贴图：BlitzKit 的槽位分配与客户端语义不符（本轮完整逆向）

**客户端材质是权威。** E-100 的 `E_100_mtr`（`shader = ~res:/Materials/StandardAllQualities.material`）
在 `.sc2` 的 `configArchive_0..4`（皮肤变体 `Default`/`E-100_skin`/`Skin2..4`）里给出 8 个贴图槽：

| 客户端槽位 | 文件（E-100） | 格式 / 尺寸 |
|---|---|---|
| `baseColorMap` | `images_pbr/E_100_BC` | BC1-sRGB 2048² |
| `baseNormalMap` | `images_pbr/E_100_NM` | BC5 2048² |
| `baseRMMap` | `images_pbr/E_100_RM` | BC5 2048² |
| `miscMap` | `images_pbr/E_100_MISC` | **BC5 1024²** |
| `maskMap` | `images_pbr/E_100_MASK` | BC5 1024² |
| `decalmask` | `CamouflageMasks/E-100_CM` | PVR 容器（移动端） |
| `albedo`（legacy） | `images/E-100` | BC3 2048² |
| `normalmap`（legacy） | `images/E-100_NM` | BC1 2048² |

> 这些 `DXT5` 文件带 `pfFlags` bit31：实为 **BC5 双通道数据装在 DXT5 fourcc 里**，
> 必须按 BC5 解码（自写 BC1/BC3/BC5 解码器验证，见 `tmp_tex_probe/bcdds.py`）。

**BlitzKit 的实际指派**（自写解码器做全通道交叉相关；E-100 与 IS-7 各自独立复现，
逐通道平均绝对差 MAD 在 1 上下即为同一图）：

| BlitzKit 槽位 | 实际图源 | MAD |
|---|---|---|
| `baseColorTexture` | `baseColorMap` 三通道 | 0.89 / 1.04 / 1.05 |
| `normalTexture` | `miscMap`.**R** 复制成灰度（三通道同值、std 相同） | 0.80–0.82 |
| `occlusionTexture` | **G** ← `baseRMMap`.R；R≈0.02、B 通道**无客户端来源** | 1.02（G） |
| `metallicRoughnessTexture` | legacy `normalmap`（`images/<T>_NM`）三通道 | 1.13 / 1.21 / 1.34 |

**关键判断：这个指派在 PBR 语义上是坏的，不能当"另一套约定"接受。**
按 glTF 规范，`occlusionTexture` 采样 **R** 通道（此处 ≈0.02 → 几乎全遮蔽）、
`metallicRoughnessTexture` 采样 **G/B**（此处 G≈0.50、B≈0.97）。而 BlitzKit 把
**miscMap 的灰度**当法线（丢掉全部法线细节）、把**法线贴图**当粗糙度/金属度、
把 RM 的通道塞进 occlusion。合理解释是上游槽位表有误或沿用 legacy 表，
其产物在这几个槽上并不具备正确的 PBR 含义。

**因此验收口径建议定为「语义正确」而非「与 BlitzKit 逐字节一致」**：

```
baseColorTexture        ← baseColorMap
normalTexture           ← baseNormalMap             （BC5，RGB=(x,y,1)，z 重建）
metallicRoughnessTexture← baseRMMap                （BC5，G=roughness、B=metalness）
occlusionTexture        ← miscMap.R 或 maskMap.R    （AO；均为 1024²）
```

这样产出的模型在同样光照下**比 BlitzKit 的更正确**（有真实法线细节、AO 合理）。
代价是"贴图逐字节一致"这条门禁不成立，需显式接受该偏差。

**若坚持逐字节一致**：`baseColor` / `normal` / `metallicRoughness` 三槽已可精确复现
（`baseColorMap`、`miscMap.R` 灰度化、legacy `normalmap`），只剩 `occlusion` 的
**R（≈0.02）与 B（均值 28、std 43）两个通道找不到来源**——它们不等于任何客户端贴图的任何通道，
疑似 BlitzKit 侧另行合成。追它需要读 BlitzKit 生成端实现（其仓库为闭源 submodule），不建议投入。

**影响面**：前端场景有 `HemisphereLight(2.2)` + `AmbientLight(0.9)` + 双 `SpotLight`，
视觉模型走 `GLTFLoader` 默认 PBR 材质（`frontend/src/scene/tankViewer.js:1060-1073`，
**无材质覆盖**；只有 `armorModel` = `collision.glb` 的材质被替换为扁平 `MeshStandardMaterial`）。
故贴图**是用户可见的**，但**不影响任何穿透/热力图数学**（那部分只依赖
`collision.glb` 几何 + `models.pb` 厚度）。

另：**6 辆车没有 legacy `normalmap` 贴图**（`1601, 3089, 5953, 10529, 14097, 51457`），
其目标只发 baseColor+normal 两槽 → 无论走哪条路线，纹理管线都必须**按车动态**决定槽位集合。

### 3.4 贴图解析覆盖率（全量 735 辆实测）

材质解析比几何解析绕：**`.sc2` 里给纹理的是 `NMaterial` 节点**，而它有两种形态：

| 形态 | 车数 | 说明 |
|---|---|---|
| 内联 `configArchive_N`（含 `textures` 字典） | **306** | 新式车（E-100 即此类）：8 槽齐全（257 辆）、7 槽（11）、3 槽（29）、2 槽（9）、无 textures（2） |
| 只有 `materialName: Instance-NN` + `parentMaterialKey` | **429** | 老式车（t-34、Pz.IV G、M3 Stuart、Firefly 等）：纹理需沿父材质解析，**不在本文件内** |

按"纹理路径相对 `3d/Tanks/<Nation>/`、`.tex` 后缀替换为 `.dx11.dds.dvpl`/`.dx11.pvr.dvpl`"这一条规则实测：

| 结果 | 车数 |
|---|---|
| 内联材质且 4 个 PBR 槽（BC/NM/RM/MISC）**全部可解析** | **266** |
| 内联材质但 0/4 可解析（引用不存在的 PBR 贴图，如 `m4-sherman`、`cromwell`、`edelweiss`、`is`） | 38 |
| 内联材质但无 `textures` 段 | 2 |
| 无内联材质，但按约定 `images/<模型名>` **可解析**（老式 legacy 贴图） | **382** |
| 无内联材质，且约定路径也不存在（如 `medium-i`、`standard-b`） | 47 |

即 **648/735（88%）用一条直白规则即可解出贴图**，其余 ~87 辆需要更聪明的规则
（沿 `parentMaterialKey` 追父材质、或处理"多辆共享同一 `.sc2`/材质"的情形）。
注意两个测量口径的限制：① 这只是**几何之外**的额外工程量，**不阻塞几何替换**；
② "0/4" 那 38 辆可能只是我取的是文件里第一个 `configArchive` 节点而非渲染网格真正引用的那一个，
修正节点选择后该数字应会改善。

> 补充事实：735 个 `tank_id` ↔ **735 个互不相同的 `blitzModelPath`**（无文件级模型共享）；
> `blitzModelPath`/`collisionMesh` 的 yaml 解析 **735/735 零失败**。

## 4. 覆盖率（全量 735 辆，非抽样）

| 项 | 数量 |
|---|---|
| 按 `<dev>` 命名的 `Parameters/<nation>/<dev>.yaml` 存在 | **735 / 735** |
| yaml 的 `blitzModelPath` 指向的 `.sc2` 存在 | **735 / 735** |
| 对应 `.scg` 存在 | **735 / 735** |
| `collisionMesh` 的 `.sc2` + `.scg` 存在 | **735 / 735** |
| 失败 | **0** |

- **模糊文件名匹配（`resolve_vehicle_file` 的两跳链）从未被触发**（回退调用 0 次）——
  按 tank_id 对应的 dev 名直接命名的 yaml 总是存在。上一轮"需靠模糊匹配"的风险不成立。
- 资产盘点：`3d/Tanks` 下 `.dx11.dds.dvpl` 11849、`.sc2.dvpl` 2487、`.scg.dvpl` 2478、
  `.pvr.dvpl` 987、`.yaml.dvpl` 766；坦克贴图解析出的 5611 个槽位引用全部是 PC 的
  `.dx11.dds.dvpl`（`.pvr.dvpl` 为移动端，未使用）；**未解析的纹理引用仅 6 个**（即上节那 6 辆车）；
  **`.sc2` 解析错误 0 例**。
- 9 个缺 `.scg` 的 `.sc2` 全部是非坦克资产（`Customization/Fire_skin*`、`Common/Decals/*`），
  **无坦克受影响**。

## 5. 泛化性：全量 735 辆验收 + 早期 27 辆抽样

覆盖 usa / ussr / uk / france / japan / china / european / other / german，
含弹夹车（T57、Bat.-Châtillon 25 t、Bat.-Châtillon 25 CL）、皮肤变体车（E-100）、
三材质车（Type 5 Heavy）、特殊小车（T7 Combat Car、AC Celeno、Ho-Ri）。

### 5.0 全量 735 辆验收（最新，取代此前的抽样结论）

把修正过 LOD0 规则的导出器做成**内存态全量跑批**（不落盘，`tmp_glb_reverify/full_audit.py`
+ `audit2.py`，735 辆约 68 秒），与本地 BlitzKit 产物逐辆比对
（口径：按**可达节点**求和顶点/索引，规避 BlitzKit 的 mesh 去重造成的度量假象）：

| 产物 | 完全等价 | 不一致 |
|---|---|---|
| `collision.glb` | **735 / 735（100%）** | 0 |
| `model.glb` | **700 / 735（95.2%）** | **35** |

> 注意口径教训：若按"文件内 mesh 求和"计数，会误报 103 辆不一致——那是 BlitzKit/glTF-Transform
> 对相同 mesh 做了去重、逐节点复用时我方未去重所致。**验收脚本必须按可达节点求和。**

35 辆不一致可归为 4 类**规则缺口**（均已被逐叶定位）：

| 类 | 现象 | 典型样本 | 规模 |
|---|---|---|---|
| A | `*_lod1dummy*` 节点未排除（我方**多出**） | `GB01_Medium_Mark_I` 的 `chassis_wheel_R_1N_lod1dummy_1`；`gun_N_mask_nc_lodNdummy` | 约 8 处 |
| B | `_nc` / `hull_nc` / `hide_elements` 状态子树**整块缺失**（我方少几何） | `J38_Type_95_Ji_Ro` 缺 `hull/hull_nc/0000`（4868 顶点，恰等于差值）；`Oth37/38/39/40/41_*` 同型 | 约 15–20 辆 |
| C | 负重轮节点集合/序号错位 | `Oth37_T26_E4_SuperPershing_S1`（缺 15 个 `chassis_wheel_*`） | 26–29 处 |
| D | 主部件批次选错（漏掉真正的主 mesh） | `PzVI_Tiger_P` 缺 `hull/0000`（3846 顶点，恰等于差值）；`SU-152` 缺 `hull/0001` | 若干 |

关键结论修正：**27 辆抽样给出"0/27 不一致"是不具代表性的**——失败集中在
**皮肤/特殊版车**（`_S1`/`_S2`/`_BP`/`_exp` 后缀）与带 LOD dummy / 多状态切换的车上。
全量跑批是本项唯一可信的验收方式，且成本极低（68 秒）。

### 5.1 27 辆抽样（早期验证，已被 §5.0 取代）

覆盖 usa / ussr / uk / france / japan / china / european / other / german，
含弹夹车（T57、Bat.-Châtillon 25 t、Bat.-Châtillon 25 CL）、皮肤变体车（E-100）、
三材质车（Type 5 Heavy）、特殊小车（T7 Combat Car、AC Celeno、Ho-Ri）。

- **导出失败 0 辆。**
- 过滤规则（排除 `HP_*` / `*_crash_*` / `chassis_chassis*`）在 27 辆目标上**全部成立，
  无需新增任何规则**（27 辆 BlitzKit 目标里这三类节点本就是 0 个）。

### 5.2 早期抽样发现的静默 bug：LOD0 批次选择（**修法已定位**）

当某个部件有**两个 `lodIndex==0` 的批次**（一个真实 mesh + 一个烘焙的 `Shadow_Material` mesh）时，
"取第一个 `lodIndex==0`"会选错：

| 选取规则 | 27 辆中节点名不匹配的车数 |
|---|---|
| 原型原规则（取首个 `lodIndex==0`） | **6 / 27**（T57、Bat.-Châtillon 25 t、Werewolf、Maus、Tiger II、SP I C） |
| "取最后一个 LOD0" | 1 / 27（T7 Combat Car） |
| **正确规则：丢弃 `Shadow_Material`，取剩余的那个 LOD0** | **0 / 27** |

判别依据是**批次材质名**（`Shadow_Material` vs 车辆的 `*_mtr`），与尺寸/switchIndex/顺序无关。
套用该规则后，27 辆在「节点数 / 节点名多重集 / 每节点顶点与索引数 / 每 mesh 几何 /
碰撞节点名与三角数」上全部一致。

### 5.3 残余差异（无害）

3 辆车（Type 5 Heavy 46 vs 45、AC Celeno 33 vs 32、Progetto C50 45 vs 38）mesh 数少于目标，
原因是 glTF-Transform 对完全相同的 mesh 做了去重；**每节点顶点总数完全一致**
（13175=13175、11131=11131、14862=14862）。

## 6. 与前端契约的兼容性结论

替换的安全性边界（决定了"能否不改前端直接换源"）：

| 前端依赖 | 实际情况 | 替换是否安全 |
|---|---|---|
| 节点**名字**正则（`turret_0X`/`gun_0X`/`gun_0X_mask`/`hull`/`chassis_track_*`/`chassis_wheel_*`） | 我们的产物命名与之一致 | ✅ |
| 节点**遍历顺序** | 前端按名字取节点后**显式排序**（`sort(… name.match(/\d+/) …)`），`frontend/src/scene/` 下无 `.children[` 位置索引 | ✅ 顺序无关 |
| 蒙皮 / 动画 | 目标 `skins=0 / animations=0`，前端不消费 | ✅ 无需实现 |
| `userData.armorSection` | GLB 内为空，前端用节点名正则 + `models.pb` 厚度合成 | ✅ |
| `collision.glb` 的 `<part>_armor_<N>` 命名 | 已 33/33 复现 | ✅ |
| **材质贴图** | 槽位映射当前错误、通道打包未确证 | ⚠️ **未达可替换** |

## 7. 缺口与工作量（修正后）

### A. 现成可复用（已验证可用）
DVPL 解码、SFV2/SCPG 读取、`parentMaterialKey` 继承链、`GlbBuilder`、DDS 解码
（BC1/2/3/DX10，含 DAVA 的"BC5 装在 DXT5 fourcc 里"需加分支）、webp 编码、
**路径解析（735/735，无需模糊匹配）**、56 B/32 B 顶点流、碰撞按板切 run、节点树构建。

### B. 需新写（具体项）

| 项 | 说明 | 估计 |
|---|---|---|
| LOD0 批次规则 | 丢弃 `Shadow_Material` 后取剩余 LOD0（27 辆 100%）——**但全量验收仍有 35 辆不一致**，见 §5.0 的四类缺口 | 1–2 人日（含 A/B/C/D 四类规则收敛 + 全量回归到 735/735） |
| 全量回归门禁 | 把 `tmp_glb_reverify/audit2.py` 的口径（按可达节点求和）固化为 CI 门禁，每辆都必须 100% 等价 | 0.3 人日 |
| **贴图槽位映射（语义路线）** | 按 §3.3 的语义表实现：`baseColorMap` / `baseNormalMap`（BC5 需重建 z）/ `baseRMMap`（G=rough、B=metal）/ `miscMap.R`（AO）；并按车动态处理缺失槽（6 辆车） | 0.5–1 人日 |
| **材质解析链（老式车）** | 429/735 的材质是 `Instance-NN` + `parentMaterialKey`，需解析父材质/共享材质库；并把 §3.4 的 88% 覆盖率推到接近 100% | 1–2 人日（不确定） |
| 贴图槽位映射（逐字节路线，**可选**） | 复刻 BlitzKit 指派：`miscMap.R` 灰度化、legacy `normalmap` 入 MR；`occlusion` 的 R/B 两通道来源需另查（**不建议**） | +1–3 人日（不确定） |
| NaN 规范化 + uint16 索引 | `0xffc00000→0x7fc00000`；按最大索引自适应 uint16；可达逐字节等价 | 0.3 人日 |
| mesh 去重（可选） | 对齐 BlitzKit 的 mesh 数 | 0.3 人日 |
| webp/alpha 编码对齐 | BlitzKit 的履带 baseColor 保留 alpha，原型丢弃 | 0.3 人日 |
| 生产化 | CLI/批处理/`_export_status.json`/验收校验（沿用 `scripts/export_asset_pack.py` 风格） | ~1 人日 |

### C. 高不确定
- `Shadow_Material` 规则在 735 辆是否 100% 成立（27 辆上是）。
- 非典型材质组合（2 槽车、`_nc`/皮肤/迷彩/贴花材质等超出抽样范围的形态）。
- 是否所有车都 `skins=0`（抽样均为 0，若有例外需兜底）。
- ~~`occlusion`/`metallicRoughness` 通道打包~~ —— **已定性**（§3.3）：BlitzKit 的指派在 PBR 语义上
  是坏的，走语义路线即可绕开；仅"逐字节路线"仍需追 occlusion 的 R/B 两通道。

**合计约 3.5–5.5 人日**。与上一轮"3–5 人日"总量接近，但**构成差异明显**：
上一轮**高估**了路径解析风险（实际 735/735 且零模糊匹配）、**低估**了贴图工作量
（其 normal 0.95 的"高分"是比错了槽位）；且忽略了本轮的 LOD0 批次 bug 与
NaN/索引编码两个"逐字节等价"障碍。

## 8. 建议的实施路径

1. **先落地 `collision.glb`（已 33/33 全等）**：它承载全部穿透/热力图数学，
   且不依赖贴图管线 → 可以先单独替换，立刻解除一半 CDN 依赖，风险最低。
2. **再落地 `model.glb` 几何**（含 LOD0 规则 + 全量跑批核对节点集合）。
3. **最后攻贴图**：先按 §7-B 重做槽位映射拿到"槽位正确但通道可能不准"的版本，
   再决定是否继续追 `occlusion`/`metallicRoughness` 的通道打包；
   若不追，需**显式记录该视觉差异**并评估可接受性。
4. 全程**不要与 BlitzKit 混用同一辆车的两源产物**：`tank_configs` 的
   `turret_index`/`gun_index` 与 GLB 节点号是硬耦合（节点号 = `Turret_NN.model`/`Gun_NN.model` 的 N），
   版本错位会静默错位节点。

## 9. 证据与复跑

```bash
# 本轮对抗式重验（独立重写的 GLB 读取器 + 27 辆泛化 + 735 辆覆盖）
cd /d/Class/Rust/Project/tmp_glb_reverify && python run_all.py
```

| 产物 | 内容 |
|---|---|
| `glbtool.py` | 独立重写的 GLB 读取器（含 byteStride 的 accessor→bufferView→buffer 解析） |
| `a1_model_compare.py` → `out_a1_e100.txt` / `out_a1_is7.txt` | 逐字节/逐 mesh 几何比对 |
| `a3_collision_compare.py` → `out_a3_e100.txt` | 碰撞节点集合、顺序、几何比对 |
| `a4_final.py` / `a4b_channels.py` → `out_a4_final.txt` | 贴图槽位与通道溯源（证明槽位映射错误） |
| `b5v3_generalize.py` / `b5c_lod0_rule.py` → `out_b5v3.txt` | 27 辆泛化 + LOD0 规则验证 |
| `c8_coverage.py` → `out_c8.txt` / `c8_fails.json` | 735 辆全量路径覆盖（0 失败） |
| `c9_inventory.py` → `out_c9.txt` | `3d/Tanks` 资产盘点 |
| `full_audit.py` → `full_audit_out.txt` | **全量 735 辆内存态跑批**：现场生成 GLB 与 BlitzKit 产物比对（不落盘） |
| `audit2.py` → `audit2_mis.json` | **修正口径后的全量验收**（按可达节点求和）：model 700/735 等价、collision 735/735 等价；含 35 辆不一致清单与差异模式归类 |
| `leaf_diff.py` | 逐叶差异定位（给出具体缺/多的节点路径与顶点数） |
| `tmp_tex_probe/bcdds.py` | 自写 BC1/BC3/BC5 DDS 解码器（含 `pfFlags` bit31 = BC5-in-DXT5 分支），用于贴图溯源 |
| `tmp_tex_probe/` 下的比对脚本 | 客户端 8 槽贴图 × BlitzKit 4 槽的逐通道交叉相关（E-100 与 IS-7 各一遍） |

上一轮的原型与产物另见 `tmp_glb_feasibility/`（`export_tank_glb.py`、`out_e100/`、
`out_is7/`、`contract_fixture.json`）。全部脚本可重跑；`src/` 与 `tools/` 既有文件未作改动。
