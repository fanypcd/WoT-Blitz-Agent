# 地图画质增强计划（逐项状态见正文：W1/W3/G1/P1 已落地，L2 已撤回，其余仍为计划）

> **状态：W1（动态反射）、W3（反射档位）、G1（草几何档）、P1 已于 2026-11-17 落地
> （消费端/导出器），L2 已撤回（2026-11-17 用户裁示）；其余项仍为计划，开工前需单独
> 确认（"写入计划暂不执行"的原始裁示只对未动工项继续有效）。** 本文记录可选项、
> 客户端依据、实施边界与验收判据。现役差异总账见
> [map-render-align-audit.md](map-render-align-audit.md)（40 条），光照专项见
> [map-lighting-plan.md](map-lighting-plan.md)；本文覆盖"画质表现增强"方向（水面全档、
> 阴影、大气、植被、贴花、后期/呈现），不重复已落地项。

## 0. 口径与证据总览

客户端依据（本机 11.21.0 数据实测；shader/材质逐字核对）：

| 系统 | 证据位置 | 关键事实 |
|---|---|---|
| 水面四档 | `Materials/Water*.material` 的 `UniqueDefines` | ULTRA_HIGH=`PIXEL_LIT,REAL_REFLECTION,REAL_REFRACTION`；HIGH=`PIXEL_LIT,REAL_REFLECTION,FRESNEL_TO_ALPHA`；MEDIUM=`PIXEL_LIT,FRESNEL_TO_ALPHA`；LOW=无 |
| 水面 shader | `Materials/Shaders/Default/water-fp.sl` / `water-vp.sl` | REAL_REFLECTION 用**屏幕空间**采样 `tex2D(dynamicReflection, 投影坐标 + 波法线×reflectionDistortion)`；REAL_REFRACTION 采 `dynamicRefraction`（场景色）+ 深度；SPECULAR 台阳高光仅 REAL_REFLECTION 档内；`RETRIEVE_FRAG_DEPTH_AVAILABLE` 做 coastLine（已实现 D5） |
| 水面实例旗标（全 36 图） | 各图 `.sc2` `#dataNodes` 的 water 材质 `flags` | 仅 `SPECULAR(20)/VERTEX_FOG(19)/FOG_*(…)/FLATCOLOR(3)/WATER_RENDER_OBJECT(14)/SHADOW_RECEIVER(2)/SOFT_COASTLINE(1)`；**`WATER_RIPPLES/WATER_TESSELLATION/WATER_DEFORMATION` 零实例启用** |
| 动态阴影 | 场景 `DirectionalShadowComponent.shadowConfigHigh` | 4 级 `ranges=20/40/64/84m`、`filterRadius=5/3/1/1`、constant/slope bias、`cascadesBlendWidth`、`fadeOutWidth`、`LMGateFactor=0.5–1.5`、`landscapeLMGateFactor`、`shadowColor`、`litDiffuseSpecAmbientMult`（逐图不同） |
| 风场 | 场景 `WindComponent` | `quaternion`（风向）+ `wc.windSpeed` / `wc.windForce`（逐图） |
| 草分档 | 场景 VegetationRenderObject ×3 | 实体名 60k/120k/500k；**500 档作者几何 38 顶点/株，OFF 档 27 顶点**（2026-11-17 起导出器已取 500 档几何；实例预算仍 60k，分档预算待做） |
| 天空云 | 天空材质 `textures.flowmap`（54 个 flowmap 批次） | 客户端双层 flowmap 动画；当前未导 |
| 后期 | `Materials/ColorGrading.material`、`ExposureTonemapping.material` | 全局调色/自动曝光材质存在；**无逐图 LUT 资源**（Data 内只有 UI `grade_booster.sc2` 与坦克 `*_grade` 贴图） |
| 坦克泥污 | 场景 `MapDirtCoverageSettingsComponent` | `dirtColor`、`dirt_height/dust_normal` 贴图、覆盖/清洗时间、强度（逐地形材质） |
| 履带印 | 场景 `MapTreadSettingsComponent` | `mtsc.color`、`treadBlending`、`shadowReceiver` |

仓分工（AGENTS 硬约束）：**渲染实现全部属于 WotbTools**（本仓只负责导出器/数据面与文档）；
`frontend/` 冻结不碰。视觉验收一律归用户。

---

## 1. 水面（优先级最高）

当前现役 = MEDIUM 档（cubemap 预烘反射 + 双层法线 + 菲涅尔 alpha）。距离"客户端 HIGH/ULTRA"
的差距 = 反射是否来自当帧场景、以及是否做折射。

### W1 — HIGH 档动态反射（推荐 P0）——**已落地（2026-11-17，消费端）**

> **状态**：WotbTools `playback/waterReflection.ts` 已实现镜像相机 + 斜裁剪 + 半分辨率 RT
> （high/ultra 档启用；正俯视/水下 fail-closed 回退 cubemap），水面 shader 按屏幕 UV +
> 法线扰动采样并乘 `reflectionTintColor`；RT 实测 mean 154 / std 79（真实场景内容）。
> 导出器 `water_props` 已补 `reflectionDistortion`/`distortionFallSquareDist`/`inGlossiness`/
> `inSpecularity`/`reflectanceColor`（当前候选包尚未重导，消费端用缺省值）。**太阳高光已落地**
> （water shader BlinnPhong + Fresnel Schlick，参数取 extras props，uSunSpecColor 按曝光
> 归一）；W2 折射仍待做；W3 反射分辨率/更新频率档位**已落地（2026-11-17，见 W3）**。

- **客户端依据**：HIGH 材质 `REAL_REFLECTION + FRESNEL_TO_ALPHA`；`water-fp.sl` 采样
  `dynamicReflection` 于**投影坐标**，偏移 = 波法线 × `reflectionDistortion`（距离衰减
  `distortionFallSquareDist`）；`SPECULAR` 分支加太阳 Blinn-Phong 高光（`inGlossiness`/
  `inSpecularity`/`reflectanceColor`）；混合 alpha = 菲涅尔。
- **做法**：镜像相机渲染半分辨率反射 RT（斜裁剪面防漏光；层遮罩排除水/草/粒子/天空穹），
  水面着色时按屏幕投影 UV + 波法线扰动采样；菲涅尔 alpha 混合；太阳高光复用
  `lightColor0`（已有 `sunUniforms`）。
- **数据缺口（导出器需补）**：`extras.water` 增加 `specular: true`、
  `inGlossiness`/`inSpecularity`/`reflectanceColor`/`distortionFallSquareDist`/
  `reflectionDistortion`（2026-11-17 W1 已补：`water_props` 现 12 键全写；此前只写 normal/fresnel/tint）。
- **收益**：★★★（建筑/桥/岸线倒影；当前"蓝玻璃"总观感的主因）。
- **成本**：中高（多一趟场景渲染、RT、层管理、与变体裁剪/实例化共存）。
- **风险**：反射 RT 与透明物排序；斜裁剪面漏光；移动端带宽。缓解：半/四分之一分辨率、
  隔帧更新、档位门控。
- **验收**：① 单测：投影 UV/扰动公式纯函数；② 接线守卫：反射 RT 创建/释放随会话；
  ③ 用户视觉：诺曼底/运河/荷兰水面能看到当帧建筑倒影，移动相机倒影随之变化。

### W2 — ULTRA 档折射（P1）

- **依据**：ULTRA_HIGH 额外 `REAL_REFRACTION`；`water-fp.sl` 采 `dynamicRefraction` +
  `RETRIEVE_FRAG_DEPTH_AVAILABLE` 深度做扭曲/厚度，`refractionTintColor`/
  `refractionConstColor`；该档水面**不透明**（`outColor.a=1`）。
- **做法**：场景色拷贝 + 深度纹理；扭曲采样 + 深度吸收；按档切换不透明/混合。
- **数据缺口**：`extras.water` 补 `refractionTintColor`/`refractionConstColor`/
  `refractionDistortion`。
- **收益**：★★（深水颜色、水下可见度）。
- **成本**：中（色拷贝/深度；与 W1 共用投影坐标）。
- **验收**：用户视觉（深水不再一律反射天空，近岸可见水下暗部）；单测扭曲 UV。

### W3 — 反射质量档位（P1，随 W1）——**已落地（2026-11-17）**

> **状态**：`waterReflection.ts` 接受 `scale`/`intervalFrames`，`configure()` 运行时改档
> （RT 按新比例惰性重建、旧 target dispose）；`getExcludedRoots()` 在反射 pass 期间临时
> 隐藏草层/战斗特效/3D 标注（渲染后恢复，异常路径 finally 同样恢复），避免镜像 RT 里
> 出现标注副本并省填充率。语义来源 = `quality.ts` 的 `QUALITY_FEATURES.water`：
> 高清 1/2 分辨率 + 隔帧（每 2 帧）、极致 1/2 + 每帧；低/均衡不建 RT（回退 MEDIUM
> cubemap，现役路径保留）。单测覆盖隔帧调用次数、RT 比例/运行时改档、覆盖层隐藏与
> 异常恢复；运行时切档由 `setQuality` 重新应用 feature 表。
> **未做**：1/4 分辨率档位（低/均衡无反射，暂无档位需求）。

### W4 — 水面贴图精度（P2，低成本）

cubemap 等距柱状 1024 → 原生、法线图 mip/各向异性按档。收益 ★，成本低。

### W5 — 涟漪/泡沫/水面细分：**无对象，不做**

客户端 shader 有 `WATER_RIPPLES/WATER_DEFORMATION/WATER_TESSELLATION` 分支，但全 36 图
水面实例旗标零启用（上表实测）。按 fail-closed 不实现；若未来版本内容启用，再按
`water-vp/fp` 对应分支补。

---

## 2. 光照 / 阴影 / 大气

### L1 — 逐图 CSM 动态阴影（P1，收益最大但成本高）

- **依据**：`DirectionalShadowComponent.shadowConfigHigh` 全参数（含 4 级距离、filter、
  bias、blend、LMGateFactor、shadowColor）。
- **做法**：导出器把 shadowConfig 写进 `lighting.json`（`shadow` 块）；消费端实现 4 级
  CSM（或先 2 级）：坦克/动态物投射与接收；静态场景受光材质按 `LMGateFactor` 与光照图
  混合（A3 已落地光照图）。
- **收益**：★★★（车辆"落地"、桥下车影）。
- **成本**：高（深度 pass × 级数、接收者材质分支、与光照图门控）。
- **风险**：级联接缝、阴影痤疮（bias 数据在手）、移动端性能。
- **验收**：自动 — 阴影矩阵/级联选择纯函数单测；用户视觉 — 坦克影随太阳方向、近远级联过渡
  无跳变。

### L2 — 自动曝光（P1）——**已撤回（2026-11-17 用户裁示）**

> **状态**：曾在 WotbTools `playback/autoExposure.ts` 实现（64px 探针每 500ms 测对数
> 平均亮度，在逐图基础曝光上乘 0.5–2.5 倍率，high/ultra 启用）。3D 回放实测诺曼底
> avg 0.339 → 倍率 0.53、曝光 0.375→0.19：天空/亮地面把平均值拉高，整图被压暗约一半，
> 坦克等高光比主体明显发黑。用户裁示**放弃光影调整、保留原来的亮度**，模块、探针 RT、
> 调试钩子、画质档 `exposure.auto` 与消费接线已全部删除；现行亮度 = 逐图太阳强度归一后
> 的固定曝光（mapLighting），不随画面自适应。
>
> **若未来重试**：客户端 `ExposureTonemapping.material` 是运行时直方图，均值测光会
> 被天空主导；需要按直方图分位数/中心加权重新设计，并给压暗幅度上限（本次 0.5 倍下限
> 过于激进）。在拿到可验证的客户端曲线前不再启用。

- **依据**：`ExposureTonemapping.material`（客户端运行时直方图）；无逐图静态值（A2 已记录）。
- **收益**：★★（明暗更接近客户端；可退役部分补偿旋钮）。
- **成本/风险**：中；与场景快切/暂停的稳定性（需滞回）与高光比场景的过暗风险。

### L3 — 天空云 flowmap 动画（P0/P1，低成本高观感）

- **依据**：天空材质 `textures.flowmap`（54 批次）；客户端双层滚动。
- **做法**：导出器随 `extras.sky` 写 flowmap 贴图 + `flowAnimSpeed/Offset`（材质属性，
  名称以客户端为准，需在导出时读取实际 property）；消费端在 `makeSkyMaterial` 里做双层
  UV 滚动（`uTime` 走回放时钟/地图墙钟——已有环境时钟）。
- **收益**：★★（天空不再"贴纸"）。
- **成本**：低-中（导出字段 + shader 两行）。
- **验收**：单测 UV 滚动公式；用户视觉看云移动方向与速度。

### L4 — 逐图风场（P2）

- **依据**：`WindComponent`（quaternion 风向 + windSpeed/windForce）。
- **做法**：导出 `lighting.json` 增 `wind` 块；草/树风摆相位与方向取该场（现在草用 VRO
  本地参数各向同性）。
- **收益**：★（全图风一致）。
- **成本**：低。
- **验收**：单测风向→相位；用户视觉：树与草同向摆动。

### L5 — 雾：维持非目标

E2 已决策不做（回放用于高视角分析，雾削弱战场可见性）。如未来要"观景模式"，可单开关复用
`lighting.json.fog`。

---

## 3. 几何 / 材质 / 植被

### G1 — 草分档与高档作者几何（P0/P1）

- **依据**：三档 VRO（60k/120k/500k）；**500 档几何 38 顶点/株 vs OFF 档 27**。
- **状态（2026-11-17）**：**几何档已落地**（导出器按 lod0 几何量取 500 档，修复“三角形
  草”）+ **消费端实例预算已落地**：`QUALITY_FEATURES.grass.instances`（低/均衡 0、
  高清/极致 60k）经 `mapWorld` 传入 `buildGrassMeshes({maxInstances})`；超预算时按等
  步长抽取（保持空间分布，不取前缀）。单测覆盖预算 0、缺省全量与抽取分布。
- **数据面现状**：打包器只产 60k 实例/bin（每图 ~0.7 MB；导出器注释明确“出货预算
  60k”）。客户端 120k/500k 档需要重新导出更大的 grass.bin（500k ≈ 6 MB/图 × 28 图），
  属于资产体积决策，未执行——消费端上限逻辑已就绪，bin 里有多少就按预算抽多少。
- **分布修正（2026-11-17）**：密度源/通道已核实（Normandy VRO
  `useSeparateDensityMap=false`、`densityMapChannel=4`、`lightmap` alpha；UV 与
  grass-vp.sl:113-117 一致，span 600 与 terrain.json 一致）。旧实现“128×128 格中心配额
  + 格内均匀哈希”会把道路/水边等亚格细节撒上草（Normandy 实测 5.6% 落点 alpha=0）；
  已改为**按密度纹理逐像素 alpha 的逆 CDF 分层采样**（像素内确定性抖动，60k 实例），
  重导 28 图 grass.*。复核 Normandy：alpha≤0 落点 0.01%（浮点边界）、≤0.1 为 0.29%，
  实例局部 alpha 均值 0.907；`densityGrid` 随图 256²–2048² 写入 grass.json。
  候选资产包已更新并重算 manifest（6072 文件 / 4480.7MB）。
  `flippedDensityMap` 仍按 128 格做几何镜像；引擎侧语义无 shader 证据（未取证）。
- **收益**：★★（近景草更密、叶片更完整）。
- **剩余**：120k/500k 数据导出与发布（需先决定包体积）；低/均衡已由预算 0 关闭。

### G2 — 草 LOD1/2（P2）

cgd 每变体 3 个 LOD（lod0/1/2），现只导 lod0。按距离切桶/切网格。收益性能，成本低-中。
验收：LOD 距离纯函数单测。

### G3 — 草 Bend（P2）

客户端 `VEGETATION_BEND`（`grassBendWeight` + 视口参数）。相机贴近时压低草，避免穿镜头。
数据：材质属性可能缺省（0），需导出时读 property；若全内容缺省则无对象（届时记录）。
成本低，验收同 G1。

### G4 — 树逐实例风相位（P2）

H2 未做；按实例位置哈希相位（与草同法）。收益 ★，成本低。

### G5 — 场景 LOD 切换（P3）

I2：客户端 LodComponent 多 LOD；现只导 LOD0。需要导出器多写 LOD 组 + 消费端按距离切换。
性能向（能为远景腾预算），成本中-高。

### G6 — 背面剔除（P3）

I3：现全场景 `DoubleSide`；按材质 `cullMode` 恢复单面。性能向，成本低；需先导出
`cullMode` 字段（材质 RenderState 已有）。

### G7 — ULTRA PBR 路径（P3）

A4：5 张 `Standard*AllQualities` 图在 ULTRA 走 PBR + `pbrLightmap`(RG)。成本高（两套材质/
光图），收益仅极致档。

### G8 — 场景动画（P3）

I6：`AnimationComponent` 472 个/16 空间、`RotationControllerComponent` 14 个；元素已导、
停在初始姿态。需 .sc2 AnimationData 解码 + 运行时采样。成本中，收益 ★。

---

## 4. 贴花与细节

### D1 — 贴花真实投影（P2）

B9 收尾：现为"盒中心平面贴片"；客户端是深度反投影。做法：对场景做射线/深度重建投影网格
（或先只对地面类贴花按高度场细分贴合）。收益 ★★（plant 漆线/污渍贴墙才自然），成本中。

### D2 — 贴花淡出参数（P2）——**无对象，不做（2026-11-17 实测）**

`decals.json` 已带 `fadeDistance/verticalFadeWidth/backSideFade`，但全部贴花数据
（plant 490 条；`data/cache/maps/11_plant_pn.decals.json` 与候选包逐条核对）三项均为
`0 / 0 / false`——没有非零样本。客户端 `Default/deferred-decal`（及 `Deferred/` 同名
变体）vertex 只读 auto 材质参数 `nearFadeDistance/farFadeDistance`，fragment 用
`positionMS.|z|` 出界裁切，均与这三个组件字段没有映射证据；vertical fade 还需要深度
重建（静态四边形近似没有 `positionMS.z`）。按 fail-closed 不猜语义：字段保留在契约里，
等出现非零实例再按客户端 shader 复刻。收益归零（无数据），不做。

### D3 — 地形贴合贴花（P2/P3）

地面类贴花按高度场细分，消除坡面悬浮/穿插。

---

## 5. 呈现 / 性能 / 其它

### P1 — 画质档语义重排（P0，前置）——**已落地（2026-11-17）**

> **状态**：WotbTools `playback/quality.ts` 的 `QUALITY_FEATURES`（`QualityFeatures`）是
> 跨模块唯一语义来源，按低/均衡/高清/极致映射：
> - `water`：客户端档位（medium/high/ultra）、动态反射开关、反射 RT 比例、更新间隔帧数；
> - `grass`：实例预算（0/0/60000/60000）、原型几何档（统一 500 档 = 当前导出档）；
> - `shadows.csm/tankContact/silhouettes`、`decals.projected`。
>
> 渲染器数值旋钮仍归 `QUALITY_PRESETS`；两表重叠字段（waterReflection/
> shadowQualityEligible）由 `quality.test.ts` 锁定一致，未实现项（CSM/折射/贴花投影）
> 固定 `false`。`playbackScene` 建场景与运行时 `setQuality` 都经由 `qualityFeatures()`
> 应用，不再直接读 `Q.waterReflection`。
> 草实例档（G1 的 120k/500k 预算）尚未分档，当前高清/极致同为 60k。

### P2 — 坦克动态泥污（P3）

`MapDirtCoverageSettingsComponent` 数据在手（dirtColor、dirt_height/dust_normal、覆盖/
清洗时间、逐地形强度）。属于坦克材质表现，非地图几何；需与坦克渲染路径配合。

### P3 — 履带印对齐（P2，低成本）

`MapTreadSettingsComponent` 的 `color/treadBlending/shadowReceiver` 逐图对齐现有 treadMarks。

### P4 — 抗锯齿/上采样（可选，非客户端）

现 MSAA + 动态分辨率；可加 SMAA/TAA。非客户端口径，默认不做。

---

## 6. 建议波次与依赖

| 波次 | 内容 | 依赖 |
|---|---|---|
| **Wave A（P0）** | W1 动态反射 + L3 天空云 + G1 草 500 档 | 导出器补 water/sky/grass 字段；画质档表（P1） |
| **Wave B（P1）** | L1 CSM（先 2 级）/ W2 折射 / W3 反射档位 | Wave A 的 RT/层管理复用 |
| **Wave C（P2）** | D1/D2 贴花、G2/G3 草 LOD/Bend、L4 风、P3 履带印 | 无强依赖 |
| **Wave D（P3）** | G5 LOD、G7 PBR、G8 动画、P2 泥污、G6 单面 | 各自独立 |

每项开工前单独确认；文本只代表候选项与判据。

## 7. 无对象 / 非目标（实测结论）

- 水面涟漪/泡沫/水面细分：内容零实例启用（见 §0 旗标表）。
- 逐图调色 LUT：无逐图资源（只有全局 ColorGrading/ExposureTonemapping 材质）。
- 常驻粒子 FX：36 图粒子实体 100% 事件驱动（祖先 ActionComponent），静态导出无对象。
- 雾：已决策非目标（E2）。

## 8. 实施边界

- 导出字段与数据面：本仓 `tools/export_map_glb.py` / `export_map_lighting.py`（WaterProps/
  sky flowmap/grass tier/wind/shadow config/water specular 等）。
- 渲染与交互：WotbTools 仓（`frontend/src/scene/…`）；`frontend/` 本仓冻结副本不碰。
- 自动化门禁：本仓导出器纯函数单测 + 消费端纯函数/源码接线守卫；**视觉验收归用户**。
- 每项落地时同步更新 [map-render-align-audit.md](map-render-align-audit.md) 对应行与
  [index.md](index.md) 进度条目（文档随改动同提交）。
