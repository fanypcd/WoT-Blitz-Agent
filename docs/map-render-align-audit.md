# 3D 回放地图渲染 × 客户端：差异总账与对齐清单

> 全量对照总账（本仓导出物 + WotbTools 现役渲染 + 客户端 36 图实测普查）：
> 逐项列"客户端怎么做 / 我们怎么做 / 后果 / 对齐收益 / 成本 / 归类"，各行状态列为**现役状态**。
> 光照专项的可行性报告与实施设计另见 [feasibility-map-lighting.md](feasibility-map-lighting.md)、
> [map-lighting-plan.md](map-lighting-plan.md)；本文覆盖其之外的**材质、贴图、水、天空、雾、阴影、植被、画质档**。

---

## 结论（TL;DR）

三方对照下来，**共 23 条**（§三逐条列出，另 1 条 D1 为已对齐基线），按性质分三类：

- **错误（我们的输出与客户端不一致、且已有证据指出偏差）6 条**（C1 已于 2026-10-09 修复）：静态场景用假光而非烘焙光照图（A3）、ACES/曝光与客户端"线性 × exposure"不符并派生一批补偿旋钮（A2，2026-10-09 出屏口径已改客户端 linear，残余见 §A）、逐图光照数据未使用（A1）、场景几何 `flatShading` 抹掉 authored 法线（I4，v0.4.1 的成果被前端丢弃）、~~贴图分辨率被导出器封顶 1024（C1，已修）~~、GLB 贴图无各向异性（C2）、地面在伽马空间做乘法而场景侧走 sRGB 解码（C3，口径不统一）。
- **缺失（客户端有、我们完全没有）9 条**：天空穹（E1）、雾（E2）、水面着色（F1）、动态阴影（G1）、`DETAIL` 细节层（B1）、`ENVIRONMENT_MAPPING` 环境反射（B2）、`TILED_DECAL_MASK`（B4）、地面动态贴花（D2）、草（H1）。
- **画质档/取舍差异 7 条**：均衡档整块移除建筑与分层地面（I1）、战场静帧贴花 vs 动态铺展（B3）、软透明语义（B5）、树的风相位（H2）、只导 LOD0（I2）、全场景 `DoubleSide`（I3）、ULTRA 档 PBR 路径（A4）。

**影响面最大的三件**（按"可见收益 × 覆盖批次"排序）：① 静态场景烘焙光照图（36 图 / 42,651 个批次绑定了光照图）；② 逐图太阳/环境色 + 线性曝光（替掉现有一整套补偿 hack）；③ 天空 + 雾（36/36 图）。这三件已在光照文档里排期，本文补的是它们之外的部分。

---

## 一、客户端地图渲染的构成（实测普查：36 图、约 9.3 万个渲染批次）

按批次所属材质的 shader 族统计（`tools/export_map_glb.py::collect_renderables` 同源口径，逐批次沿 `parentMaterialKey` 链解析材质文件）：

| shader 族 | 批次 | 客户端行为 |
|---|---:|---|
| `materials` + `MATERIAL_LIGHTMAP`（烘焙路径） | **55,082** | albedo × 烘焙光照图(UV1) × 2 + 动态阴影混合 |
| `speedtree-materials`（叶卡/树） | 28,411 | SH/顶点色染色 + billboard 叶卡 + 风摆 |
| `pbr`（UBER 档） | 6,361 | PBR + IBL + `pbrLightmap`(RG: 方向阴影/AO) |
| `lit-materials`（`BlinnPhongAllQualities`，含 spherical-lit） | 2,378 | 动态受光（太阳 + 阴影），无烘焙光图 |
| `materials` 无光图（unlit textured） | 114 | albedo × 动态阴影 |
| `water-*` | 40 | 逐档水面着色（反射/折射/浪/泡沫） |
| `skyobject` | 36 | 天穹 unlit + flowmap 云动画 |

**两个必须记住的陷阱**：

1. **`Standard*AllQualities` 是"按画质档切 shader"的模板**：`StandardLightmapAllQualities` = ULTRA→`PBR.material`、HIGH/MEDIUM/LOW→`Textured.material`；`StandardAllQualities` = ULTRA→PBR、其余→`BlinnPhongAllQualities`；`StandardSpeedTreeAllQualities` = ULTRA→`PBRSpeedTree`、其余→`SpeedTree.SphericalLit`；`StandardLandscapeAllQualities` = ULTRA→`PBRLandscape`、其余→`TileMaskAllQualities`。**用它的 5 张图（02/03/13/15 等）在不同画质档下是两套完全不同的着色路径与两套光图资产**（`lightmapAtlases/` vs `pbrLightmapAtlases/`）。
2. **光照图绑定在材质链上**：批次 → 叶子 NMaterial（`textures.lightmap` + `uvScale/uvOffset`）→ 父级材质（albedo）。部分特征按批次计数：绑定光照图 **42,651**、`envReflectionMask` **535**、`detail` **437**、`decal` **273**、天空 `flowmap` **54**、`TILED_DECAL_MASK` 少量（holland/forgecity）。

---

## 二、我方现状（**审计基线**，留档对照；现役状态以 §三 各行状态列与各主题文档为准）

**导出物**（`release/asset_pack/map/<key>/`）：`scenery.glb`（几何 + 嵌入 albedo；材质按 (datasource, albedo) 合并；**无 UV1、无光照图、无 detail、无环境反射遮罩**）、`ground.webp` + 分层 `ground/*.webp` + `ground.layers.json`、`terrain.json`/`terrain.u16.bin`、`mini.webp`、`destructibles.json`。**没有**：光照图、IBL、天空贴图、水面贴图/立方图、光照参数、雾参数。

**消费端**（WotbTools 现役，`frontend/src/scene/playbackScene.js` 3524 行 + `sceneryMaterials.js` + `sceneryInstancing.js`）：

- 场景 GLB → 一律降级 `MeshLambertMaterial`（`flatShading: true`、`DoubleSide`），光源是硬编码 `HemisphereLight(0xbfd4e8,0x2a2f36,2.4)` + `DirectionalLight(0xffffff,3.0)`；再乘 `SCENERY_LAMBERT_EXPOSURE=0.75` 压暗。
- 渲染器 `ACESFilmicToneMapping` + `toneMappingExposure 1.15`；天空节点被显式隐藏（`if (/sky/i.test(o.name)) o.visible=false`）；**无雾**；**无阴影贴图**（`shadowMap` 从不启用）；水体靠透明度近似并强制写深度。
- 分层地面（`ground.layers.json`）用自定义 ShaderMaterial 复刻 `tilemask-fp.sl`（GLOBAL_TINT/SEPARATE_LM/SCALED_TILES/HEIGHT_BLEND 四个分支齐全），**伽马空间直采直写**；地面贴图设了各向异性，**场景 GLB 贴图没设**。
- 画质档：`low`（无场景、无分层、缩略图底图）/ `mid`（**无场景**、无分层、俯视烘焙底图）/ `high` / `ultra`（均含场景 + 分层）。

**已有补偿性 hack（代码注释自认，全部与"光照不是客户端的"同源）**：`SCENERY_LAMBERT_EXPOSURE=0.75`、地面 shader 两处 `×2`、烘焙底图 `toneMapped=false`、叶卡 `occMean` 新旧双方程、`ST|` 必须自定义 shader 绕 sRGB 往返、炮线 `toneMapped=false` + 专用亮色板、水体 `depthWrite=true` 折中、叶卡包围球 +2m。

---

## 三、差异总账（19 条）

标注：**〔错误〕** = 我们与客户端口径不一致且有偏差；**〔缺失〕** = 客户端有我们没有；**〔画质〕** = 各自的取舍/档位差异。影响面按批次或图数计。

### A. 光照与色调

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| A1 | 逐图太阳（色/强度/方向/环境色）+ 逐图雾 + IBL | 36 图共用一套硬编码光 | 黄昏/夜战/雪图/沙漠全都一个调子 | ★★★ | **部分已落地（2026-10-09）**：太阳方向/色/强度 + 环境色由 `lighting.json` 驱动（[map-lighting-plan.md](map-lighting-plan.md)）；**雾（决策不做）与烘焙光照图（已落地）见下**；IBL 已落地 | 〔错误→部分修复〕 |
| A2 | 出屏 = 线性 × exposure（filmic 曲线在发布版被注释掉） | ACES + 1.15，另有 4 处 `toneMapped=false` 绕过 | 全局对比/饱和度与客户端不同；为迁就它派生一批补偿旋钮（0.75 压暗、亮色板、加深阵营色） | ★★★ 对齐后补偿旋钮可整体退役 | **已落地（2026-10-09；2026-10-10 收紧）**：`renderer.toneMapping = LinearToneMapping` + `toneMappingExposure = 1.0`（= 客户端"线性 × 曝光 → 编码"同式），**不留任何出屏旋钮**（`?tonemap`/`?exposure` 已撤除：客户端曝光是运行时自动曝光、数据面无静态值可抄，替代参数只能手工调）。（切换依据：离线拟合 ACES@1.15 等效显示增益 0.974≈1，差在曲线形状非亮度。）**残余缺口**：① 自动曝光未实现 ⇒ 整体亮度可能与客户端有系统性差异（如实记录，不用手调参数盖）；② legacy 类材质不经出屏变换 —— **C3 已核实与客户端同构**（legacy 材质在客户端也 raw 写出），非缺陷。〔错误→已修复（部分）〕 |
| A3 | 静态几何用烘焙光照图（42.6k 批次） | Lambert 假光照 + 全局压暗 | 建筑明暗关系全错（朝上面吃满天光、岩石过曝、红房子过暗） | ★★★ | **已落地（2026-10-09）**：36 图全量——图集随包 + 逐实例 UV 变换（`InstancedBufferAttribute aLm`）+ 前端 unlit `albedo × lightmap × 2`；动态阴影混合（LMGateFactor）与 `pbrLightmap`（ULTRA 档 RG 光图）未做 | 〔错误→回退为客户端同构〕 |
| A4 | 5 张"AllQualities"图在 ULTRA 走 PBR + `pbrLightmap`(RG) | 无 PBR 路径 | 极高画质档下这几张图的观感差异更大 | ★（仅 ultra 档） | 视是否支持 ULTRA 档 | 〔画质〕 |

### B. 静态场景材质

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| B1 | `MATERIAL_DETAIL`：detail 纹理按 `detailTileCoordScale` 平铺 ×2 | ~~未处理~~ | ~~近景墙面/地面缺细节层，发平~~ | ★★ | **已修复**：实测 **453 实例 / 10 图**，材质全为 `Detail.material`（全档无条件）；导出器加 `detail` 判据 + `extras.detail = {texture, scale}`，前端 `makeLightmappedMaterial` 第 4 参与 Lambert 补丁（55 个无光图实例）双路接入。**包增量仅 +2.2 MB**（10 图 scenery 293.2 → 295.4 MB，51 个材质带 extras——detail 贴图本身小且多为灰度） | 〔缺失→已修复〕 |
| B2 | `ENVIRONMENT_MAPPING`：`envReflectionMask` + cubemap 反射（535 批次 / 5 图） | 未处理 | 玻璃/金属/潮湿面全无反射，发死 | ★★（城市图明显） | **已落地（2026-10-09）**：判据=绑两槽；遮罩成图 + 天空立方图转等距柱状 + `ENV_REFLECTION` 分支逐项复刻（含 BlinnPhong 高光）；配套新增**着色器编译门禁** | 〔缺失→已修复〕 |
| B3 | `Decal.material`（UV1 投影贴花）+ `decal-spreading` 动态铺展 | 导出期把 decal 烘进 albedo（静帧） | 静帧一致，但「贴花随时间铺开」的表现没有 | ★ | **已核实（2026-10-09）：铺展在本版内容零启用**——`spreadingProgress` 等属性全内容零出现（`TILED_DECAL_SPREADING` 属拼花砖系统，同见 B4）；静态 `decal` 槽（333 实例）是烘焙进 albedo 的静帧贴花，视觉等价、无需动态 ⇒ 不改 | 〔画质→无对象〕 |
| B4 | `TILED_DECAL_MASK` + `decalTileColor`（holland/forgecity 等） | 未处理 | 拼花砖/瓦片层缺失 | ★ | **已核实（2026-10-09）：本版客户端内容零启用**——76 个材质文件无一声明该 define、全部实例无 `decalmask`/`decaltexture` 槽、全属性键无 `decalTileColor`/`decalTileCoordScale` ⇒ 按 fail-closed **不实现**（公式与位置已读好备查：materials-fp.sl:344-348，环境反射之后、细节层之前） | 〔缺失→无对象（本版）〕 |
| B5 | `ALPHABLEND`/`ALPHAMASK` 效果层：albedo.a × 掩码(UV1) 的**软** alpha（烟/瀑布/浪）+ `TEXTURE0_ANIMATION_SHIFT` 滚动 | alphaMode BLEND 曾是死值（掩码恒 1 → 二值剪影），前端伪透明启发式再削一刀 | 烟/瀑成硬边卡片 | ★★ | **已修复**：掩码通道修复（`FP_A8` 同义取值）+ `extras.blendLayer` + 前端尊重标记；**滚动动画已落地**：掩码独立成图 + `TEXCOORD_1` + 双采样器材质，按回放时钟驱动 | 〔缺失→已修复〕 |
| B6 | `enabledPresets.AlphaBlend`（`Textured.material` 里 = `TransclucentRenderLayer` + `blend: true` + `depthWrite: false`）+ `BLEND_BY_ANGLE` 的**视角淡出软混合**效果片（光束/光柱：`rays.sc2` 类） | 导出器**不读 `enabledPresets`**，一律按 `has_alpha` 落 `alphaMode=MASK, alphaCutoff=0.33`；`BLEND_BY_ANGLE` 完全未处理 | 光束片被裁成硬边暖白块：medvedkovo `rays.sc2` 的贴图是"白 RGB + 图案全在 alpha"（A 均值 14、仅 5% 像素高于 0.33）⇒ **95% 内容被裁掉** | ★★ | 小-中：导出器捕获 `enabledPresets`/`BLEND_BY_ANGLE` + `angleBlendBounds/Power/Inversion` 并写进 extras；前端按客户端公式（`a *= pow(saturate((VdotN−x)/(y−x)), power)`，`VdotN=|dot(view,n)|/(|view||n|)`，含 inversion 插值）走软混合。影响面：`BLEND_BY_ANGLE` **48 实例 / 2 图**（medvedkovo 1、idle 47）；`AlphaBlend` 预设共 7652 实例（其中植被/建筑的裁切路径是既有取舍，非本行范围） | **已修复（2026-10-09）**：导出器读 `enabledPresets` + `BLEND_BY_ANGLE` 三属性 → `alphaMode=BLEND` + `extras.alphaBlend/blendByAngle`；前端新增 `makeBlendByAngleMaterial`（软混合 + 客户端视角因子 + depthWrite:false），并由 `alphaBlend` 标记让伪透明启发式放行。实测 medvedkovo rays 材质 `MASK/0.33` → `BLEND` | 〔错误→已修复〕 |

| B7 | 光照图只在 `MATERIAL_LIGHTMAP` define 下采样（来源：材质顶层 define / 实例启用的 `LightMap` 预设 / `*LightmapAllQualities*` 模板族） | 判据是"**绑了 lightmap 槽即受光图**"——过宽 | 客户端 unlit 的网格被乘上暗光图：medvedkovo 外围群山（跨 1.2 km）被压成深灰带黑斑（实测采到均值 0.23、p10 0.012 的窗口） | ★★ | **已修复**：新增 `lightmap_capable`（+ `preset_names`/`preset_defines` 纯函数与两条单测）；影响面 111 实例 / 7 图，其余 42540 判定不变 | 〔错误→已修复〕 |
| B8 | alpha 裁切只在声明 `ALPHATEST` 时启用（材质链顶层 define 或实例启用 `AlphaTest` 预设；客户端证据 `Detail.material` 基础层 = `OpaqueRenderLayer`） | 判据是"**albedo 贴图有 alpha 通道即 MASK 0.33**"——未核对 `enabledPresets` | plant walls.sc2 墙板（`Detail.material`、无 AlphaTest 预设、UV 区 53% 像素 alpha<0.33）整批被裁 ⇒ 墙面消失、露出真地形 ⇒ 墙板整批被裁、露出真地形（"地形塌陷"观感） | ★★ | **已修复**：`alpha_test_capable`（与 lightmap/detail 判据同口径）+ MASK 分支加门（SpeedTree 豁免其着色器自带 discard；AlphaBlend 预设实例维持既有 MASK 取舍）；plant GLB 重导、墙板恢复不透明 | 〔错误→已修复〕 |
### C. 贴图与采样

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| C1 | 场景贴图 1024–2048²，按需 mip | ~~导出器 `decode_dds(max_dim=1024)` **硬性封顶**~~ | ~~近景建筑贴图糊一档~~ | ★★ | **已修复**：`decode_dds/decode_pvr3` 默认上限取消（`_cap_dim`，`max_dim=0`=不缩放）、地面路径 2048 → 原生、立方图等距柱状 512 → 1024——所有贴图一律按客户端原生分辨率入包。实测（medvedkovo 外围群山 `mountains_001kl_`）albedo/lightmap 1024² → **2048² 原生**。代价：scenery.glb 620 → 834 MB（×1.34）、整包 +5.7% | 〔错误→已修复〕 |
| C2 | 贴图各向异性按画质档（我们预设里 ultra=8） | 地面贴图设了，**场景 GLB 贴图没设**（默认 1） | 掠射角下建筑/道路贴图发糊、闪烁 | ★★ 低成本高收益 | 一行（遍历 GLB 纹理设 anisotropy） | 〔错误→已修复〕 |
| C3 | 硬件 sRGB 解码 + 线性空间做乘法 | 地面 shader 在伽马空间直乘（且离线烘焙与在线 shader 口径需一致）；场景 GLB 侧 GLTFLoader 走 sRGB（正确） | 若源贴图是 sRGB，地面中调可比客户端亮约 1.4×；场景/地面两条链口径不统一 | ★★ | **已核实（2026-10-09）：客户端"两类并存"**——PBR 类 albedo 带 DXGI `*_UNORM_SRGB`（实测 Maus `_BC` = BC1_UNORM_SRGB）⇒ sRGB 解码 + 线性运算 + 显式 `LinearToSRGB` 出屏；legacy 类贴图全为 legacy DXT（**无 sRGB 变体**：landscape DXT3、colormap DXT5、老式坦克 albedo DXT5）⇒ 原样采样 + 显示空间 + raw 写出。我们的两条链已各按各类对齐（自定义材质全 NoColorSpace；坦克 GLTFLoader sRGB），**"1.4×"前提被证伪、无需改画质**；新增"采样空间白名单"守卫 | 〔错误→已核实（对齐，零改动）〕 |

### D. 地面

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| D1 | Landscape 网格 + `tilemask-fp.sl`（四分支已复刻）+ 原生分辨率 tile 平铺 | 已对齐（分层 shader + 3D 地形 + 各向异性） | —— 这条是**已对齐项**，列为基线 | — | — | ✅ |
| D3 | 地形 = 高度图**程序化网格**：顶点在 texel 原位（`i/size·span`）、高度取 texel 值（`GetHeightClamp`，无平滑）；查询 `GetHeightAtPoint` 同口径 | 顶点与 texel **相位错位**（`PlaneGeometry(seg)`+双线性采样、采样误用 `n−1`） | 陡坡/切槽处地形面向外探出（等高线水平 +0.5–0.75 m、高度 +0.04 中位/+1.34 最大）⇒ 俯瞰下**穿出挡土墙/桥台贴面** | ★★ | **已修复（2026-10-09，前端）**：客户端同构**补片级自适应 LOD**（补片 = 8×8 四边形，判据三条：屏幕半径/屏幕高度/绝对 3 m，阈值随 fov 在 zoom/normal 间插值；**顶点高度 = 两通道（原始 texel × 双抽头均值）按误差比 morph 插值**，morphFunc 与 zeroLodMul 均直译自引擎；顶点 = texel 原值、层级交界按"最粗相邻格边界直线"取高 ⇒ 无 T 型接缝/无空洞，实测最大缝 0.655→1.8e-6 m）+ 几何直接建在场景系 + `GetHeightClamp` 夹取 + `sampleHeight` 改除 `n`；实测墙区 p95 0.65→0.28 m、max 1.86→1.60 m。**共面接缝不做人工补偿**：客户端材质层 52/52 无 `DepthBias` 声明、几何层同一份数据同样共面 ⇒ 缝上的逐像素抢胜与客户端同源（相机差异）。**禁止复活**：`?poff`（目视定参的负 `polygonOffset`）与 `?seamfix`（实例几何抬升）已撤除（无客户端依据）；共面/越顶属客户端同机位的忠实结果（它靠贴地相机从不看到），如实接受；另见 D4。 | 〔错误→已修复〕 |
| D4 | 贴地薄结构的**净空**（铁轨/路缘/贴花只有 0.2–0.3 m）；地形 LOD 判据 = 补片 8×8 内部网格 + 屏幕半径/高度/绝对三条（`SubdivisionPatch`） | 地形网格把补片当"一个四边形"且**漏了屏幕半径判据** ⇒ 同距离下单元格粗 8 倍、窄特征（路堑/沟槽）漏检 | 贴地薄构件在远视距被粗格"合法"盖住（客户端贴地相机从不看到；实测 60/150/250/300/450 m = 1/2/12/50/97 点，细真值恒定 0） | ★★ | **已修复（前端）**：补片级细分 + 8×8 内部网格 + 三条判据（半径判据先行）；同两点在 78–250 m 各视距下网格 = 22.330/22.335（与真值逐点一致），判据值零违反；守卫锁"终止补片满足三判据"+"单元格边长 ≤ 0.08·视距"。**最终态 = 纯客户端口径**：折中方案（数据面让位掩码 `bake_terrain_cover.py`）与继任的自研脚印判据均已按"只对齐客户端，不自创补偿"**撤销**（地形 = 三判据 LOD + 两通道 morph + 真值高度场一份到底）；远距盖住如实接受。**LOD 重建为单向滞回**（`terrainLodStale`：拉近立即细化、拉远 ≥40% 才变粗）——双向滞回会让网格长期停在更粗层级、粗格插值抬过薄构件，属"时有时无"的来源。**水面片/水下薄板不入贴地判定**（避免扰动可见岸线）。禁止复活让位掩码/脚印判据 | 〔错误→已修复〕 |
| D5 | 水面与地形交界：客户端片元末按"水面片元 vs **背后表面**（深度预通道 `dynamicDepthPrepass`）的**相对**深度差"做 `coastLine` 淡化（`water-fp.sl` `RETRIEVE_FRAG_DEPTH_AVAILABLE` 分支 + `depth-fetch.slh`），随后 `fresnel *= coastLine` | 我们只有硬边相交（水/地边界逐 texel 阶梯） | 岸线生硬、随 LOD 出现阶梯 | ★ | **已实现（前端）**：机理 = **水面自身贡献**在岸线带里与地形逐像素抢深度（地形跨水面的等高线本身平滑；所谓"水面"= `env_ma_ice02_*` 冰面片，其下 1 cm 另有不透明冰 `env_ma_ice01_*`、客户端 `ro.flags` 含 `VISIBLE_REFRACTION`）。无深度预通道 ⇒ 由同一等式反推：`ΔndcZ/ndcZ = Δz_沿视线/z_沿视线`、`z_沿视线·|视线.y| = 相机高 − 水面高` ⇒ **`coastLine = saturate(dNdcZ / (2·uDepthUlp))`**（`dNdcZ = 2fn/((f−n)z²)·(水面高−地形高)/|视线.y|`，`uDepthUlp = 2/2^depthBits` 实测位深、near/far 取相机实际值）——**淡出带 = 该视距的深度抢胜带（2 ulp）**、深水任何角度满反射（含俯视）；地形高采渲染用高度场、缺则 fail-open。**边界**：背后表面只算地形（结构/浮冰不参与；冰面上方保留少量反射）。**禁止复活的两条废弃口径**（∝视距的"单元格上界"；相机高分母——后者致俯视深水透明），守卫锁死 | 〔缺失→已实现〕 |
| D7 | 沿岸线的**特征解析**：客户端相机（TPS，贴地）让"水陆平缓坡"天生落在 1 texel 级网格上 | 回放相机常在数百米外 ⇒ 客户端三条判据在同一相机位置**合法地**给粗格 ⇒ 2–5 cm/texel 的平缓坡被量化成米级台阶（网格量化，非深度抢闪/非数据噪声） | 岸线呈锯齿台阶、拉近才变细 | ★★ | **回退为客户端同构**：自研第四条判据（`terrainMesh.js` `shores`，水陆交界特征细分）已整体删除（DAVA `SubdividePatch` 只有三条判据——"纯客户端三判据"目标下无自研判据位置；负向守卫锁死不复活）。远相机下岸线的网格量化 = 客户端同相机位置的忠实结果。**保留**：根补片步长取 2 的幂（非 2 的幂时 `step>>1` 会留空洞——与判据无关的几何修复）。**已知差异**：1 texel 级残余起伏属数据自身噪声（客户端同有） | 〔错误→已修复〕 |
| D6 | **水下静态件**的着色通道：客户端的 `ReflectionRefraction` 通道不计漫反射（`materials-fp.sl` 的 `albedo × lightmap × 2` 只在 `#if MATERIAL_LIGHTMAP && VIEW_DIFFUSE` 下发生；`#else` 支注释 "do not scale lightmap in view diffuse only case" ⇒ `color = albedo`），而水面在客户端是**不透明**的（`water-fp.sl` REAL_REFLECTION 分支 `outColor.a = 1`）⇒ 水面覆盖处的可见结果 = 折射画面，**不含光照图乘法** | 我们按主通道渲染水下件：`albedo × lightmap × 2`。马利诺夫卡 25 片水下冰 `env_ma_ice01_*` 在客户端图集里落在**未烘焙黑格**（逐片实测 63–71% 黑，邻片 0–9%：烘焙器跳过河面）⇒ 冰面成黑块 | 黑冰面 + 黑色边界锯齿 | ★★ | **已修复（前端）**：纯几何判据（整个包围盒落在某片水面占地内 0.5 m 余量、且低于其标高 5 cm 余量）⇒ 不接光照图、走不受光 albedo（`MeshBasicMaterial`，即 `VIEW_DIFFUSE=0` 口径）。受灾面实测全 36 图仅 2 张（malinovka 26 实例、italy 1）。**已知差异**：客户端另有 `flatColor` 末乘（冰上 ≤4%）未随包导出；部分淹没件（桥/沉船）仍按主通道渲染。另：水下薄板**逐件**带作者染色（马利诺夫卡冰面多数 (0.922,1.0,0.973)、`ice01_09` 为 (0.647,0.753,0.796) 偏蓝）⇒ 在染色变化的那条 tile 边界上有一道 ~22% 色阶——这是客户端作者数据本身；导出器不施加"统一染色"，如实保留客户端数据 | 〔错误→已修复〕 |
| D5 | 贴地"被地形着色"的贴花件（铁轨/地界标线：材质 `Decal.material`，`MATERIAL_DECAL`）：`albedo(UV0) × 地图 colormap(UV1).rgb（separate_lm 图再 × colormap.a）× 2.0`，**不受光**，UV 用网格自带 UV1（无 uvScale） | 导出器无贴花判据（材质不打标）且 UV1 随导门槛只覆盖光照图/动画掩码 ⇒ 贴花批次丢 UV1；前端无贴花材质 ⇒ 落到受光 Lambert | 铁轨贴图比客户端**亮约 1.5–2.3×**（客户端等效乘子 0.84 vs 我们受光 1.2–1.9） | ★★ | **已修复**：`decal_capable` 判据 + `extras.decal`/`decalLmAdjust`/`decalSeparateLm` + UV1 随导（fail-closed 计数）+ 前端 `makeDecalMaterial`（不受光，含 GLOBAL_TINT 调整分支）；后续审计修复 decal 烘焙连乘残留与 FLATALBEDO 丢染色（判定 = 有效 define 集 ∪ 实例 flags） | 〔错误→已修复〕 |
| D2 | 地面贴花（碾压痕/弹坑/血迹）动态生成 | 无（客户端贴图不在可得数据面：游戏层 geo-decal 贴图由游戏层传入） | ~~战场缺少"被踩过"的痕迹~~ ⇒ 无可对齐实现 | ★ | **弹坑/血迹：不做**——客户端构成 = FX 粒子（`3d/FX/<map>/hit_surface/groundHit_*.sc2`）+ 游戏层 geo-decal（`GeoDecalManager`；**贴图不在数据面**）；重做前置 = 先定位客户端弹痕贴图。**碾压痕：已落地**——TreadSystem 作者数据在数据面（逐车 `treadParameters` 块 88/764 yaml + `3d/Tanks/Common/images/` 13 张 `*_tread.dx11.dds.dvpl`），数据面已导出（包内 `tread/`）、前端由 WotbTools 几何贴花消费（[feasibility-track-marks.md](feasibility-track-marks.md)） | 〔弹坑/血迹不做；碾压痕已落地〕 |
| P3 | 信号灯等"**状态切换器 + 骨骼网格**"对象：客户端每盏灯 = `env_fc_trafic_light_02.sc2`（渲染组件 = 杆）**＋同位置的 `env_fc_trafic_light_01.sc2`**（`StateSwitcherComponent` 3 状态 + 子实体 `State 0/1/2` = **`SkinnedMesh`** 灯头/悬臂） | 导出器渲染类白名单只有 `Mesh`/`SpeedTreeObject` ⇒ **`SkinnedMesh` 整类被跳过** ⇒ `_01` 全图缺失（GLB 内该名字节点 0） | 2026-10-10 用户"图中的信号灯组件不完全"（只有杆、没有灯头/悬臂） | ★★ | **已修复（2026-10-10，上游导出）**：白名单纳入 `SkinnedMesh`，按**静止绑定姿态**导出（不读骨架/动作 ⇒ 不随动），并**只取激活状态** `State 0`（全图 335 个切换器无一 `activeState≠0`，故规则严格成立；损毁态 `State 1/2` 按用户决定不导，撞毁仍"直接消失"）。forgecity 实测：`_01` 节点 **0 → 61**（每个 450 顶点、z ∈ [4.27, 6.98] 的灯头+悬臂），杆 `_02` 完好形态另已逐层核实与客户端一致（138 顶点/240 索引/80 三角形 = datasource 77797、包围盒逐位相同、材质链 `TextureLightmap`+`traffic_light.tex`+lightmap、逐实例 UV 变换 61/61）。客户端破坏机制（`FallingType 2` 倒伏 + `objects_falling_lamppost_creak/down` + 扬尘 + `fallingAtoms` 碎件）本仓不实现 | 〔缺失→已修复〕 |

### E. 天空与雾

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| E1 | 天穹网格 unlit + 逐图 sky 贴图（35/36 图**已在 GLB 内**）；顶点 `w=0`（丢弃平移，网格坐标即**方向**）+ 深度推远 | 节点被隐藏，背景纯色 `0x11161d` | 抬头是灰底，直接暴露"游戏外挂视图"感 | ★★★ | **已修复**：复刻客户端的**无限远方向投影**（天穹网格半径仅 ~10m——himmelsdorf ±9.9/iceworld ±10，当普通几何渲染就是地图中心一个小球；顶点 `w=0` 丢弃平移 + 深度推远 + `frustumCulled=false`）。flowmap 云动画未做（待导出器补 flowmap 槽 + `flowAnimSpeed/Offset`） | 〔缺失→已修复（天穹；flowmap 云动画未做）〕 |
| E2 | 逐图雾：距离项 + 半空间项 + 大气天/日色散射 | 无——**决策：不做** | 远景没有空气透视 | — | **非目标**：3D 回放的用途是**高视角分析对局**，雾会削弱远景/战场可见性，属"影响预期功能的特效"。数据（`lighting.json` 的 `fog` 块）随包保留备查，渲染层不接 | 〔**非目标**〕 |

### F. 水

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| F1 | 四档水面着色：屏幕空间反射/折射 + 浪法线 + 泡沫 + 岸线 + 逐图 cubemap | 半透明 Lambert，`opacity 0.7`、强制写深度 | 所有带水图水面是"蓝玻璃" | ★★ | **MEDIUM 档已逐项复刻（2026-10-09）**：`WaterPerPixelCubemapAlphablend`（`PIXEL_LIT`：双层滚动法线 + UDN + 菲涅尔 alpha + 逐图 cubemap×色调）。LOW 档已弃用（`!PIXEL_LIT`：不透明、无法线 ⇒ 反射平面镜、零波浪扰动；验收判"不好"；代码/数据/守卫零残留）。本档固有观感：反射＝预烘探针（不含建筑）且被当整体颜色 ⇒ 俯视偏亮、反射对不上模型；照出建筑需**平面反射**（镜像相机 + RT，≈ 客户端 HIGH），ULTRA 的屏幕空间反射/折射、浪花、涟漪、岸线均未做 | 〔缺失→MEDIUM 档已对齐；HIGH/ULTRA 未做〕 |

### G. 动态阴影

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| G1 | 坦克/动态物 CSM（级联 20/40/64/84 m，逐图不同）+ `SHADOW_RECEIVER` 批次接收 | 完全没有阴影 | 车辆"浮"在地面上、动态物没有遮挡暗示 | ★★ | 需 CSM + 批次接收。**当前不做**（曾实现单级近似一版后按决定整体撤回，工具/前端/守卫全部回退）；若日后重启，注意 three r185 的 PCF 是 `sampler2DShadow` + `shadow.map.depthTexture`（不是旧版的打包 RGBA 深度）| 〔缺失〕 |

### H. 植被

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| H1 | Grass 系统：实例化草簇 + 风摆 + `GrassQuality` 档 | 无草（GLB 内无草系统网格；`dec_*_grass` 是静态装饰） | 草地大面积"光板"，野外图观感差距明显 | ★★ | 大（新系统）或中（静态草簇近似） | 〔缺失〕 |
| H2 | SpeedTree：SH/顶点色 + 叶卡风摆 + 逐档 LOD | 已对齐染色与叶卡（2026-10-18 补：SPHERICAL_LIT 净染色 = `0.282094×SH×shOcclusionMult`，静态 ST 组随导 COLOR_0 并烘 AO；旧口径 SH 字面值导致 DesertSands 棕榈叶 2.4× 过曝）；风摆用统一相位摆动（无逐实例风相位） | 树是静止的 | ★ | 小-中 | 〔画质〕 |

### I. 性能与档位

| # | 客户端 | 我们 | 后果 | 收益 | 成本 | 类 |
|---|---|---|---|---|---|---|
| I1 | 低档只降采样/减 LOD | 均衡档**整块移除建筑与分层地面**（走俯视烘焙底图） | 均衡档观感与客户端完全不同（平面图 + 烘焙屋顶） | ★★（移动端默认档） | 需重定档位语义（可能与性能预算冲突） | 〔画质〕 |
| I2 | LOD 按距离切换（LodComponent） | 只导 LOD0（SpeedTree 取顶点量最大组） | 画质无损、性能更贵（远景三角形多） | 性能向 | 中 | 〔画质〕 |
| I3 | `cullMode: FACE_BACK` 为主 | 全场景 `DoubleSide` | 过绘制翻倍、背面也参与光照 | 性能向 | 一行（按材质 flags 恢复单面） | 〔画质〕 |
| I4 | 顶点法线：authored（硬边/烘焙法线） | ~~前端 `flatShading: true` 把导出的 authored 法线丢掉了~~ | 场景呈"多面体"感 | ★★ | **已修复**：关 `flatShading`（authored 法线生效），并同步到俯视烘焙页 | 〔错误→已修复〕 |

---

## 四、落地状态（按当时优先级）

**P0（已全部落地）**：A1 + A2 + A3（光照三件，实施设计见 [map-lighting-plan.md](map-lighting-plan.md)）+
**E1 天空** + **C2 场景贴图各向异性** + **I4 关 `flatShading`**（后三项已在 WotbTools 落地：
`makeSkyMaterial` 无限远方向投影、`applyMaterialTextureAnisotropy`、删 `flatShading` 并同步到
俯视烘焙页）。

**P1（已落地 / 非目标）**：F1 水（MEDIUM 档已对齐；HIGH/ULTRA 未做）、B1 detail、B2 环境反射
均已落地；E2 雾 = 非目标（决策不做）；G1 动态阴影 = 不做（撤回）。

**P2（落地状态）**：C1 贴图上限提升已落地（改客户端原生分辨率）；I1 画质档语义重定、H1 草、
A4 PBR/ULTRA 档、I2 LOD、H2 风相位未做（各自为画质取舍或缺失，见 §三各行）；B3/B4/D2
按各行结论（无对象/不做/碾压痕已落地）。

## 五、未定口径（未定前不做相关改动）

1. **客户端画质档默认值**：决定 §一陷阱 1 的 5 张图走 PBR（ULTRA）还是 Textured/lit。
2. **exposure 数值**（`ExposureTonemapping` 的 `[auto][a]` 属性随包 YAML 内未见；客户端为
   运行时自动曝光，无静态值可抄——自动曝光未实现，见 A2）。
3. **`materialLightmapAdjustment` 缺省语义**：场景光照图材质 2497/2497 无该属性 ⇒ 应为恒等；
   需一次实测确认后才能在复刻式里省掉该项。

（原第 1 项"逐贴图采样空间"已定案：sRGB 标志只出现在 PBR 坦克 albedo（DXGI `*_UNORM_SRGB`）；
其余全为 legacy DXT 无标志 ⇒ 按原样采样；客户端两类材质各自成链。见 C3 行。）

---

## 六、附录：普查数据与复现

- 36 图逐图批次家族计数与特征计数：普查脚本输出（`tmp_analysis/family_census.txt`，按仓库约定不入库；方法同 §一表格口径：`collect_renderables` → 材质链 → 材质文件 shader/defines 解析）。
- 关键计数：绑定光照图 **42,651** 批次、`speedtree` **28,411**、`pbr` **6,361**、`lit` **2,378**、`unlit textured` **114**、`water` **40**、`sky` **36**；`envReflectionMask` **535**、`detail` **437**、`decal` **273**、天空 `flowmap` **54**。
- 包内实测（审计时基线；C1 修复后贴图已按客户端原生分辨率入包）：`scenery.glb` 嵌入贴图当时最大 **1024²**（himmelsdorf 43 张 / 6.6 MB；holland 100 张 / 12.8 MB；forgecity 85 张 / 4.4 MB）；场景 GLB 内**无草系统网格**。
- 客户端贴图规格抽样：地面 colormap 2048² **DXT3**（alpha 通道即光照图）、光照图图集 2048² DXT1 + 12 mip、建筑 albedo 1024² DXT1、`00_global_content` 抽样 400 张中 DXT1 351 / DXT5 29 / DXGI-BC1_UNORM 15 / DXT3 5。
