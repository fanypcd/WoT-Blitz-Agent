# 地图光照画质对齐（3D 回放场景）可行性评估

> 2026-10-09 整理 · **状态：待决策——本次只出报告，未改任何代码、未重导任何资产**。
> 结论均以**本机客户端全树实测**（Steam `Data/`，36 张可玩图全量扫描）与**客户端自带着色器源码**
> （110 个 `.sl`/`.slh` 全量解码）双向锁定；未验证项在 §七 单列，不混入结论。
> 姊妹文档：[game-data-sources.md](game-data-sources.md)（数据来源权威表）、
> [data-inventory.md](data-inventory.md)（运行期数据总账）、
> [game-data-sources.md §5.5](game-data-sources.md)（俯视合成朝向契约）。

---

## 结论

**可行，且比预期便宜**：客户端每一张图的光照都是**数据驱动**的，而且这些数据就在我们已经会解的那几个容器里——

| 画质要素 | 客户端数据在哪 | 我们现在的状态 |
|---|---|---|
| 太阳（方向/颜色/强度）+ 环境色 | 地图 `.sc2` 的 `#sceneComponentSets.Default` → `DirectionalLightComponent` | ❌ 完全没用：前端写死 `HemisphereLight(2.4) + DirectionalLight(3.0)`，36 张图同一套光 |
| 静态场景的**烘焙光照** | 每图 2–11 张 `lightmapAtlases/textureN` + 网格 UV1 + 每个材质实例的 `uvScale/uvOffset` | ❌ 没导：`scenery.glb` 只有 TEXCOORD_0，没有图集贴图；目前用 Lambert 假光照 + 全局压暗系数（`SCENERY_LAMBERT_EXPOSURE 0.75`）凑 |
| 环境反射（IBL） | 每图 `IBL/diffuse`(64² cube) + `IBL/specular`(256²×9mip cube) | ❌ 没导 |
| 天空穹 | 每图 sky 贴图 + flowmap（35/36 图的 `scenery.glb` **已含**天穹网格，只是被前端 `visible=false` 隐藏） | ❌ 前端背景是 `0x11161d` 纯色 |
| 雾 | 每图 `SceneRenderConfigComponent`（密度/半空间/大气天日色散射） | ❌ 没有任何雾 |
| 动态阴影 | `DirectionalShadowComponent`（三档级联参数）+ 着色器 `shadow-mapping.slh` | ❌ 无阴影 |
| 光照方程 | **客户端着色器源码**（`materials-fp.sl`/`lit-materials-fp.sl`/`pbr-fp.sl`/`ibl.slh`/`vendor` 雾数学） | —— 可逐行复刻，不必猜 |

按"可见收益 / 成本"排序，前四项是**性价比最高的四刀**：① 静态场景改用烘焙光照图（当前最刺眼的偏差：建筑被假半球光洗白，还要靠 0.75 系数回压）；② 逐图太阳/环境色（现在黄昏图、夜战图、雪地图共用一套白光）；③ 天空穹 + 逐图雾（现在天空是纯色 void）；④ 坦克 IBL（金属反射环境，现在是纯色高光）。

**关键量化（实测）**：光照图图集解码后再压 WebP，36 图合计约 **+145 MB**（2048² 原分辨率）或 **约 +40 MB**（降到 1024²）；天空贴图 **约 +7 MB**；IBL 立方图 **每图 < 50 KB**。相对现在 3.6 GB 的资产包，属于可接受量级。数据格式、容器解析、贴图解码链本项目**已经全部具备**（`tools/wotbtools/` + `tools/export_map_glb.py`），缺口主要在导出器与消费端渲染，不在逆向。

---

## 一、客户端光照的构成（实测清点）

### 1.1 每图光照/环境配置：`<map>.sc2` 的场景组件集

地图场景文件 `Data/3d/Maps/<space>/<space>.sc2.dvpl` 的 `#sceneComponentSets.Default`（**注意不是 `#sceneComponents`**，后者 `count: 0`）是一个 7–8 项的组件列表，逐条都是光照/环境：

| 组件 | 关键字段（himmelsdorf 实测值） |
|---|---|
| `DirectionalLightComponent` | `light.color` (1,1,1)、`light.intensity` 5、`light.ambColor` (0.88,0.86,1.0)、`light.type` 0；`transformNode.transform` 给位置 + **四元数（太阳朝向）** |
| `SceneRenderConfigComponent` | `fogColor`、`fogDensity`、`FOG_ATMOSPHERE`、`fogAtmosphereColor{Sky,Sun}`、`fogAtmosphereDistance/Scattering`、半空间雾 `fogHalfspace{Height,Falloff,Density,Limit}` |
| `DirectionalShadowComponent` | `shadowConfig{Low,Medium,High}` 各含级联范围 `ranges` (20/40/64/84 m)、`fadeOutWidth`、`shadowColor` (0.745 灰)、`LMGateFactor` (0.5/1.7)、`litDiffuseSpecAmbientMult`、`litNormalScale` |
| `IBLComponent` | `ibl.diffuse`/`ibl.specular`/`ibl.environmentMultiplier`/`ibl.environmentGroundFactor`/`ibl.environmentGamma`/`ibl.dimensions=256` |
| `WindComponent` | 风速/风力 + 包围盒（植被晃动） |
| `MapTreadSettingsComponent` / `MapDirtCoverageSettingsComponent` | 履带痕 / 车体污渍（画质细节） |

**36 张图全部具备**（唯一例外是非地图测试场景 `chestball`）；逐图数值见附录 A。太阳仰角实测分布 **21.5°–65.7°**（夜战图 `32_faust_fa_night` 43°、月面图 `40_moon_mn` 35°），方位角覆盖全象限——这正是"每张图观感不同"的来源。

**太阳朝向的解读（本报告的关键推断）**：把 `transformNode.quaternion` 按 DAVA 常规 `(x,y,z,w)` 解开后，**光源局部 −Y 轴**旋转到世界系即"阳光传播方向"——36 张图**全部**给出地平线以上的太阳（仰角 21.5°–65.7°，无例外）；而 −Z/+Z/−X 等候选轴在 8–12 张图上会算出"太阳在地下"（负仰角），全部排除。此推断**尚缺一次与客户端画面的 A/B 对照**（§七 #1），但候选已收敛到唯一自洽解。

### 1.2 静态场景 = 烘焙光照图（这一块是画质差距的主体）

**材质普查（36 图全量）**：静态布景的材质族以光照图类为绝对主体，每图 **25–325 个** `TextureLightmap.material` / `StandardLightmapAllQualities.material` 材质实例；**地图里没有任何 PBR 材质**（`PBR.material` 计数恒为 0——PBR 只用于坦克）。按渲染批次统计（6 图抽样）：

| 图 | 渲染批次 | 绑光照图的批次 | 占比 | 其余主力族 |
|---|---|---|---|---|
| 19_himmelsdorf_hm | 2148 | 1003 | 47% | `Textured`(979)、`SpeedTree`(164) |
| 16_holland_hl | 3031 | 2082 | 69% | SpeedTree 两族 (774) |
| 34_forgecity_fc | 2863 | 2129 | 74% | SpeedTree 两族 (581) |
| 17_karelia_ka | 2374 | 560 | 24% | `SpeedTree`(1051)、`Textured`(648) |
| 40_moon_mn | 186 | 184 | 99% | — |

未绑光照图的两族在客户端**并非不受光**：`Textured.material` 走"albedo × 动态阴影"，`SpeedTree` 走"SH 环境色 + 顶点色"（后者本项目已在 v0.4.1 期间逐条对齐）。

**光照图数据本体**：

- 每图 **2–11 个图集文件**（`lightmapAtlases/textureN`、少数图在 `lightmaps/`、pliego 有个 `lightmapsPBR/landscape`），2048² BC1 + 完整 mip 链，磁盘（DVPL 未压缩存储）合计 **252 MB / 36 图**；himmelsdorf 单图 4 张 × 2.79 MB。
- **绑定关系在材质链上**：渲染批次的 `rb.nmatname` → 叶子 NMaterial（`textures.lightmap` + `properties.uvScale/uvOffset`）→ `parentMaterialKey` → `TextureLightmap.material`（`textures.albedo`）。即"贴图来自父级、光照图实例带图集与 UV 变换"。
- **UV1 需逐材质变换**：客户端 `materials-vp.sl:117` 明确 `varTexCoord1 = uvScale * texcoord1 + uvOffset`。实测 himmelsdorf 每图 2497 个光照图材质实例对应 **2320 组不同的 (图集, uvScale, uvOffset)**，`uvScale` 为 1/16 或 1/32——即**每个物体占图集里的一块 128²/64² 瓦片**，瓦片按物体在世界中的位置分配（同一网格在不同位置的实例，图集与 scale 相同、offset 不同）。这解释了此前两次"albedo × lightmap × 2"烘焙失败中的第二次：**旧实现拿原始 UV1 直接采样，缺了这层 `uvScale/uvOffset` 变换，所以整片落在图集暗区**。
- 图集内容实测有结构（himmelsdorf texture3：全图均值 0.215 / p95 0.631；`stn_ss04` 实例瓦片 [0.063,0.3149]+1/16 → 均值 0.367、max 0.871、std 0.199）——是"低亮度 + 高动态"的可乘光照图特征，与 `×2` 因子吻合。

### 1.3 动态物体（坦克等）= 平行光 + IBL + 级联阴影

`IBL/diffuse.dx11.dds`（64² cube，BC1，12 KB）与 `IBL/specular.dx11.dds`（256² cube，**9 级 mip 预滤链**，262 KB）逐图齐备，另有全树共享的 BRDF LUT `Materials/Textures/brdf_lut.png`。着色器侧（`pbr-lighting.slh`）：

```
mip = MipFromRoughness(roughness, mipmapLevel.x)      // MIP0 = 4.0 - 1.2*log2(roughness)，再对 NumMip-1 取反
diffuseIBL  = texCUBElod(diffuseIrradianceMap, N, 0)
specularIBL = texCUBElod(specularReflectionMap, R, mip) * F
F = EnvBRDFApprox(F0, roughness, NdotV)（非 ULTRA）| BRDF LUT 采样（ULTRA）
```

### 1.4 天空与雾

- 天空是**天穹网格**（实体名 `SkyFlattenSphere.sc2` 等）+ `Skyobject.material`（unlit，`albedo` + 可选 `flowmap` 云流动画）。36 图**全部**有天空贴图（2048² BC1，另 30 图另配 flowmap），并且**35/36 已随 `scenery.glb` 导出**（仅 `moon` 无天穹节点），当前只是被消费端按名字隐藏。
- 白天穹贴图压 WebP 后极小（实测 2.7 MB DDS → **187–190 KB** WebP，全 mip 链）。
- 雾方程在 `vp-fog-math.slh`：距离项（线性或 `1-exp(-density·d)`）、半空间项（Lengyel unified fog 变体 + 高度衰减）、大气项（`fogAtmosphereColorSky ↔ Sun` 按视角与光向夹角 `dot(view, lightDir)` 做 `pow(·, scattering)` 插值，`ENABLE_HIGH_QUALITY_FOG` 时启用）。

### 1.5 客户端着色器源码可解码（不需要猜方程）

`Data/Materials/Shaders/` 下 **110 个** `.sl`/`.slh` 全部可用项目现有 `decode_dvpl` 解出原文，包括：`lit-materials-fp.sl`(561 行)、`pbr-fp.sl`(543)、`materials-fp.sl`(389)、`lighting.slh`、`ibl.slh`、`setup-lightmap.slh`、`shadow-mapping.slh`、`vp-fog-math.slh`、`vp-fog-props.slh`、`Utilities/exposure-tonemapping-fp.sl`、`Utilities/{diffuse-convolution,rough-specular,specular-brdf}-fp.sl`。**这是本报告能给出确定方程、而不是"大致相似"的原因。**

---

## 二、客户端光照方程（逐行读出）

### 2.1 静态场景（烘焙路径，`materials-fp.sl`，正常渲染时 `VIEW_DIFFUSE=1 && VIEW_ALBEDO=1`——由 `common.slh:145` 的兜底分支保证）

```
out = albedo(UV0)                                   // ALPHATEST / flatColor / vertexColor 照旧
out *= lightmap(UV1') × 2.0                          // UV1' = uv1*uvScale + uvOffset
      其中 lightmap 先过 materialLightmapAdjustment（gamma→contrast→brightness，GLOBAL_TINT 门控）
out *= globalFlatColor × 2.0                         // 仅 GLOBAL_TINT
out = lerp(out, out * shadowMapColor, …)             // RECEIVE_SHADOW：动态阴影按 LM gate 混入
```

要点：**×2 是真实存在的**（`VIEW_ALBEDO` 分支）；`GLOBAL_TINT` 的 `globalFlatColor` 默认 0.5 ⇒ ×2 后中性（导出器已在处理地面侧的同式）。

### 2.2 受光物体（`lit-materials-fp.sl` / `pbr-fp.sl`）

```
direct  diffuse  = lightColor0 * (NdotL / π)
direct  specular = lightColor0 * Specular(NdotH …) * F_Schlick
ambient          = lightAmbientColor0  (= 光源 ambColor)
                   + IBL（仅 PBR：diffuseIrradiance*albedo + specularReflection*F，两者 × occlusion）
result = direct + ambient，再 × (1 + (shadowLitDiffuseSpecAmbientMult - 1)*shadow)  // 阴影按档位系数压
```

### 2.3 雾（`vp-fog-math.slh`）
见 §1.4；`FogQuality` 档位控制是否启用高质量大气项（`ENABLE_HIGH_QUALITY_FOG`）。

### 2.4 阴影
`shadow-mapping.slh` 级联选择 + `DirectionalShadowComponent` 的三档参数（范围 20/40/64/84 m、bias、`filterRadius`、`shadowColor`、`LMGateFactor`、`litNormalScale`、`normalSlopeOffset`）。静态几何用"烘焙光照图 × 动态阴影"混合，动态物体用纯 CSM。

### 2.5 后处理（`exposure-tonemapping-fp.sl`）
**发布版把 filmic 曲线注释掉了**，最终只有 `color * exposure`（`gamma = 2.2` 变量存在但未参与该行；sRGB 转换在另一支被注释）。即客户端出屏基本是**线性 × 曝光**——这一点消除了"我们是不是该换 ACES"的悬念：现在前端用的 `ACESFilmicToneMapping + exposure 1.15` 与客户端口径不同，属需要对齐的一项。曝光的数值来源未在随包 YAML 里找到（§七 #5）。

---

## 三、现状差距（我们 vs 客户端）

| 项 | 客户端 | 当前实现（`frontend/src/scene/playbackScene.js`，WotbTools 侧同源） |
|---|---|---|
| 静态场景光照 | albedo × 烘焙光照图 × 2（+ 动态阴影混合） | `MeshLambertMaterial` + `HemisphereLight(0xbfd4e8, 0x2a2f36, 2.4)` + `DirectionalLight(0xffffff,3.0)` 固定方位，再乘 `SCENERY_LAMBERT_EXPOSURE = 0.75` 压暗 |
| 逐图差异 | 逐图太阳色/强度/方向/环境色/雾/IBL | **无**：36 图共用一套光（唯一的逐图差异来自 SpeedTree SH 染色与地面 tint） |
| 天空 | 天穹网格 + flowmap | `scene.background = 0x11161d`，天穹节点显式 `visible=false` |
| 雾 | 逐图三路混合雾 | 无 |
| 动态阴影 | CSM（20–84 m 级联） | 无 |
| 坦克间接光 | IBL cube × BRDF | 纯直接光（无环境反射，金属发死） |
| 色调映射 | 线性 × exposure | ACES + 1.15 |

值得注意：现有代码注释里已经写明"岩石过曝的根因是朝上面吃满了天光，属光照动态范围问题……真正的解法是重平衡光照或补环境光遮蔽"——**本报告给出的大写方案正是那句话的解**：把假光换成客户端真实数据（烘焙光照图 + 逐图太阳），`SCENERY_LAMBERT_EXPOSURE` 这类补偿旋钮就可以整体退役。

---

## 四、实施方案（分三期，每期独立可验收）

> 详细设计、被否方案与依据、逐文件改动清单、验收与回退见
> [map-lighting-plan.md](map-lighting-plan.md)（2026-10-09 制定）。以下为摘要。

> 所有权边界（见 [AGENTS.md](../AGENTS.md) §Frontend ownership）：**期 1 在本仓**（导出器 + 资产包 + 文档），
> **期 2 在 WotbTools**（前端渲染）；本仓不维护产品前端，`frontend/` 只作本机调试。

### 期 1：数据面（本仓）

1. **新增 `tools/export_map_lighting.py`**：逐图从 `.sc2` 提 `#sceneComponentSets.Default` 的 7 类组件，产出 `map/<key>/lighting.json`：
   `sun{direction(世界系单位向量), color, intensity, ambient}`、`fog{…}`、`shadow{Tier: {ranges, bias, shadowColor, LMGateFactor, litDiffuseSpecAmbientMult, litNormalScale}}`、`ibl{diffuse, specular, multiplier, groundFactor, gamma, dimensions}`、`wind`、`sky{albedo, flowmap}`。
2. **IBL 与天空贴图**：`decode_dds` 扩展 **cube map 支持**（当前只读 mip0 单面：`tools/export_map_glb.py:862`）→ 导出 6 面 × 全 mip 的 WebP 立方图（或 KTX2）；天空 albedo/flowmap → WebP。压后体积：IBL ≈ 44 KB/图，天空 ≈ 190 KB/图。
3. **光照图图集 + UV1**（核心）：
   - `scenery.glb` 网格随导 **TEXCOORD_1**，值**在导出期预先变换到图集空间**（`uv1*uvScale + uvOffset`）——这一步让"每实例材质参数"消失，材质只需按 `(albedo, 图集)` 去重，网格合并口径基本不变（否则材质数会涨到每图 2–3 千）。
   - 图集贴图随包（`map/<key>/lightmap/textureN.webp`）；材质 extras 记 `lightmap` 槽与 `GLOBAL_TINT`/`flatColor`/`materialLightmapAdjustment`。
4. **打包器与 COS**：`scripts/export_asset_pack.py` 收口新文件；36 图全量重导 + 俯视重烘（`tools/composite_overhead.py`）+ 差分上传。

**体积预算（实测外推）**：光照图 2048² → 约 +145 MB；若降到 1024² → 约 +40 MB（光照图是低频数据，1024 通常够用，按画质档可两版并存）；天空 +7 MB；IBL +1 MB。合计 **+50 ~ +155 MB**（现包 3.6 GB）。

### 期 2：消费端（WotbTools）

1. `lighting.json` 驱动：太阳（方向/颜色/强度）、`ambColor`、雾、阴影参数替换现在写死的两盏灯。
2. 静态场景：删掉 Lambert 假光照与 `SCENERY_LAMBERT_EXPOSURE`，改"albedo × 光照图(UV1)"（`MeshStandardMaterial.lightMap` 走 uv1 通道，或 `onBeforeCompile` 注入客户端的 ×2/调整式以求逐项一致）；`Textured` 族改"albedo × 动态阴影"。
3. 天空穹：解除隐藏，按 `Skyobject` 语义 unlit 渲染（含 flowmap 动画），并与雾色衔接。
4. 坦克：接 IBL（`PMREMGenerator` 从导入的 specular cube 生成，或自定义 mip 选择以复刻 `MipFromRoughness`）+ 太阳投影（`DirectionalLight.castShadow`，级联可先单级近似）。
5. 后处理：把 ACES 换成客户端的线性 × exposure（曝光值需标定，§七 #5）。

### 期 3：精修
`GLOBAL_TINT`/`lightmapAdjustment` 逐材质落地、动态阴影与烘焙光照图的 LM gate 混合（`LMGateFactor`/`landscapeLMGateFactor`）、水面（`water-fp.sl` + 逐图 cubemap）、植被风动（`WindComponent`）、夜间图/特殊图（`32_faust` / `40_moon`）单独过一遍。

---

## 五、成本、体积与风险

**工作量估计**：期 1 ≈ 3–5 人日（导出器扩展 + 立方图解码 + 36 图重导 + 包与 COS 同步）；期 2 ≈ 5–8 人日（WotbTools 场景改造）；期 3 视精修范围另计。**逆向风险已基本消除**——数据格式、材质链、贴图解码、着色器方程全部在手。

**主要风险**：

| 风险 | 说明 | 缓解 |
|---|---|---|
| 材质/绘制批次增长 | 光照图绑定使"同 albedo 合并"变为"同 (albedo, 图集) 合并" | 导出期预变换 UV1（期 1.3）；必要时按画质档只在高档启用光照图 |
| 包体积 +5%~+4%... | 见上预算 | 1024² 降档 / 仅高画质档随包 |
| 光照图口径细节 | sRGB 标志（`textureSampleStates`）、mip 选择、wrap、×2 与调整顺序 | 已从源码读出主式；逐项以 A/B 渲染对照钉死（§六） |
| 与坦克观感联动 | 光照系统整体替换会同时改变坦克画面 | 期 2 内一次成组替换，避免"新场景 + 旧光"的中间态上线 |
| 视觉终判 | 属用户（AGENTS.md） | 见 §六 |

---

## 六、验证协议

- **本仓（自动化、可入库）**：`tools/test_*` 纯函数测试锁 `lighting.json` 提取（组件解析、四元数→方向、36 图回归夹具）；导出器断言（每图 UV1 覆盖率、图集引用可解析、材质 extras 齐备）；打包器清单自洽（新增路径计入 manifest）。
- **A/B 对照（推荐先做，1 天内可出结论）**：复用消费方已有的 headless 渲染先例（`WotbTools/frontend/scripts/bake-ground-overhead.mjs`，headless Chrome + three.js）离线渲染 2–3 张代表图（`himmelsdorf` 城市 / `karelia` 冷调 / `32_faust` 夜战）并出对照图。
- **视觉终判归用户**：本报告与后续实现都只交"锁定不变量的测试 + 对照图"，由用户判定是否达到客户端水平；Agent 不自行截图充当验收。

---

## 七、未定项（fail-closed 清单，不得在验证前当地面真值使用）

1. **太阳方向约定**：`光源局部 −Y 轴` 为唯一自洽解（36/36 图仰角在 21.5°–65.7°），但缺一次与客户端画面的 A/B；候选轴已穷举排除其他 5 个。
2. **`uvScale`/`uvOffset` 的字节布局**：实测样本为「5 字节头 + 2×float32」（`01 01 00 00 00` + 两 float，如 0.0625 / 0.693），需按 KeyedArchive 写入方复核后再入代码。
3. **光照图采样口径**：`textureSampleStates`（值 2103616）里的 sRGB/wrap/filter 位、mip 选择、与 `×2` 的相对顺序——按需 A/B 定版。
4. **`VIEW_*` 在发布版的实际定义集**：本报告按 `common.slh:145` 的兜底分支（`VIEW_DIFFUSE=1 && VIEW_ALBEDO=1`）解读；若发布版另有设置，`×2` 项需重估（该分支是 shader 内唯一能产生 ×2 的路径）。
5. **曝光数值来源**：`ExposureTonemapping` 的 `exposure` 是 `[auto][a]`（引擎侧赋值），随包 YAML 未见；需以对照图反推标定。
6. **`ibl.environmentMultiplier` / `environmentGroundFactor` 的作用点**：随图取值 0.8–4.0 / 0.4–0.8，未出现在三个 IBL 生成着色器里，推测在引擎侧合成或采样时施加，落地前需确认。
7. **`40_moon_mn` 无天穹节点**（其天空走 `objects/images/spacedome.tex`），需单独定位其天空实体。
8. **逐图光照图与 `StandardLightmapAllQualities` 族**（02/03/13/15 图）的差异**未逐条比对**（疑为同族不同质量档），落地时按图抽验。

---

## 附录 A：36 图光照参数（实测，按 `−Y` 约定解出的太阳方向）

| 图 | 太阳强度 | 太阳色 | 环境色 | 仰角 | 方位 | 雾密度 | 大气距离 | 散射 | IBL 倍数/地面因子 | 阴影档键 |
|---|---|---|---|---|---|---|---|---|---|---|
| 02_desert_train_dt | 8 | (1.00,0.96,0.86) | (1.00,1.00,1.00) | 24.2° | 41° | 0.0020 | 1500 | 3.0 | 2.0/0.5 | 21 |
| 03_erlenberg_er | 7 | (1.00,0.93,0.58) | (1.00,0.99,0.93) | 46.6° | 311° | 0.0010 | 600 | 0.5 | 1.1/0.5 | 21 |
| 04_medvedkovo_md | 8 | (1.00,1.00,1.00) | (0.92,0.96,1.00) | 33.5° | 251° | 0.0050 | 200 | 40.0 | 1.6/0.6 | 21 |
| 05_amigosville_am | 7 | (0.95,0.87,0.67) | (0.88,0.88,0.88) | 30.2° | 342° | 0.0030 | 300 | 2.5 | 1.3/0.5 | 21 |
| 06_rudniki_rd | 8 | (0.60,0.42,0.08) | (1.00,1.00,1.00) | 25.5° | 137° | 0.0050 | 400 | 1.0 | 1.6/0.6 | 21 |
| 07_fort_ft | 12 | (0.80,0.77,0.64) | (1.12,1.04,0.90) | 32.4° | 29° | — | 500 | 8.0 | 1.3/0.5 | 21 |
| 08_idle_id | 8 | (0.89,0.84,0.64) | (0.94,0.90,0.87) | 29.8° | 43° | 0.0010 | 150 | 40.0 | 2.0/0.5 | 21 |
| 09_savanna_sv | 7 | (1.00,0.88,0.64) | (1.00,0.88,0.73) | 41.8° | 327° | 0.0020 | 300 | 4.0 | 1.1/0.5 | 21 |
| 11_plant_pn | 8 | (0.95,0.91,0.82) | (0.93,0.86,0.69) | 52.7° | 307° | 0.0012 | 500 | 2.5 | 1.3/0.6 | 21 |
| 12_malinovka_ma | 5 | (0.93,0.93,0.93) | (0.79,0.83,0.87) | 43.1° | 104° | 0.0050 | 100 | 2.5 | 1.3/0.5 | 21 |
| 13_pliego_pl | 10 | (1.00,0.88,0.73) | (0.85,0.85,0.85) | 65.7° | 36° | 0.0005 | 800 | 1.0 | 1.1/0.6 | 21 |
| 14_port_pt | 5 | (1.00,1.00,1.00) | (0.84,0.84,0.84) | 39.2° | 345° | 0.0050 | 400 | 1.0 | 1.5/0.5 | 21 |
| 15_lagoon_ln | 7 | (1.00,0.91,0.73) | (1.20,1.20,1.20) | 28.9° | 322° | 0.0010 | 800 | 1.0 | 1.1/0.4 | 21 |
| 16_holland_hl | 5 | (0.89,0.94,1.00) | (0.79,0.85,0.92) | 28.9° | 193° | 0.0025 | 600 | 7.0 | 1.1/0.5 | 21 |
| 17_karelia_ka | 10 | (0.52,0.48,0.37) | (0.99,0.98,0.92) | 42.4° | 167° | 0.0027 | 200 | 3.0 | 1.6/0.5 | 21 |
| 18_canal_cn | 5 | (0.90,0.85,0.72) | (0.69,0.74,0.74) | 50.5° | 173° | 1.0000 | 100 | 4.5 | 0.8/0.5 | 21 |
| 19_himmelsdorf_hm | 5 | (1.00,1.00,1.00) | (0.88,0.86,1.00) | 31.9° | 5° | 0.0040 | 100 | 3.0 | 1.0/0.5 | 21 |
| 21_mountain_mnt | 14 | (1.00,0.73,0.45) | (1.00,0.96,0.92) | 56.9° | 60° | 0.0006 | 400 | 3.0 | 1.3/0.6 | 21 |
| 22_italy_it | 8 | (1.00,0.70,0.57) | (1.00,0.90,0.88) | 21.5° | 98° | 0.0030 | 400 | 2.6 | 1.0/0.7 | 21 |
| 23_karieri_kr | 8 | (1.00,0.90,0.66) | (0.98,0.98,0.95) | 29.4° | 145° | 0.0030 | 100 | 7.0 | 1.3/0.6 | 21 |
| 24_milibase_mlb | 8 | (0.74,0.73,0.67) | (0.96,1.00,1.00) | 37.7° | 124° | 0.0019 | 100 | 2.0 | 1.0/0.6 | 21 |
| 25_canyon_ca | 8 | (1.00,0.72,0.36) | (1.02,1.02,1.01) | 29.8° | 43° | 0.0030 | 200 | 5.0 | 1.6/0.5 | 21 |
| 26_holmeisk_hk | 10 | (0.88,0.88,0.82) | (1.17,1.08,0.99) | 27.9° | 139° | 0.0100 | 300 | 1.3 | 1.0/0.5 | 21 |
| 28_rock_rc | 8 | (1.00,0.80,0.32) | (0.88,0.88,0.85) | 41.4° | 103° | 0.0030 | 300 | 2.5 | 0.9/0.6 | 21 |
| 29_skit_sk | 7 | (1.00,0.88,0.73) | (1.00,1.00,1.00) | 40.3° | 43° | 0.0040 | — | — | 1.1/0.6 | 21 |
| 30_grossberg_sh | 8 | (1.00,0.80,0.60) | (1.10,1.10,1.10) | 39.9° | 54° | 0.0020 | 1200 | 3.0 | 0.9/0.6 | 21 |
| 31_lumber_lm | 8 | (1.00,0.98,0.88) | (0.87,0.82,0.89) | 36.9° | 290° | 0.0100 | 10 | 1.0 | 1.1/0.6 | 21 |
| 32_faust_fa_night | 8 | (0.40,0.50,0.70) | (0.94,0.96,1.00) | 43.4° | 42° | 0.0040 | — | — | 1.7/0.6 | 21 |
| 33_neptune_nt | 8 | (0.43,0.43,0.43) | (0.96,0.98,1.00) | 34.9° | 149° | 0.0035 | 100 | 0.6 | 1.0/0.6 | 21 |
| 34_forgecity_fc | 8 | (1.00,0.93,0.87) | (0.74,0.82,0.87) | 39.9° | 205° | 0.0015 | — | — | 1.5/0.6 | 21 |
| 35_rift_rt | 10 | (0.83,0.83,0.83) | (0.96,0.98,1.00) | 62.2° | 316° | 0.0012 | — | — | 1.6/0.6 | 21 |
| 40_moon_mn | 10 | (0.50,0.60,0.80) | (1.02,1.06,1.12) | 35.3° | 132° | 0.0005 | — | — | 4.0/0.5 | 21 |
| 41_iceworld_ic | 5 | (1.52,1.52,1.52) | (0.59,0.69,0.75) | 34.3° | 249° | — | 100 | 4.0 | 2.0/0.6 | 21 |
| amigosville_old | 7 | (0.95,0.90,0.75) | (0.88,0.88,0.88) | 40.0° | 329° | 0.0025 | 50 | 1.0 | 1.3/0.7 | 21 |
| erlenberg_old | 3 | (1.00,0.89,0.65) | (0.97,0.89,0.82) | 53.2° | 339° | 0.0030 | 350 | 18.0 | 1.2/0.6 | 21 |
| savanna_old | 7 | (1.00,0.82,0.59) | (0.90,0.96,1.00) | 46.9° | 332° | 0.0015 | 300 | 4.0 | 1.2/0.8 | 21 |

（`—` = 该图未启用/未写该字段；"阴影档键"= `shadowConfigHigh` 的标量键数，21 为标准集，`chestball` 为非地图测试场景，0。）

## 附录 B：证据与复现方法

本报告结论由以下方法得出（探针脚本按仓库约定不入库，方法在此记录，可按需重跑）：

1. **场景组件集**：解 `.sc2`（`tools/wotbtools/wotb_sc2.py::read_sc2`）→ `#sceneComponentSets.Default` 逐组件打印；36 图全量。
2. **材质族普查**：遍历 `#dataNodes` 的 NMaterial（`fxName` 计数）与渲染批次（`rb.nmatname` → `parentMaterialKey` 链上找 `textures.lightmap`）。
3. **光照图引用与体积**：按槽名 `lightmap` 解析 `.tex` → 目录解析 `.dx11.dds.dvpl`，统计文件数/字节。
4. **UV1/图集校验**：`decode_group_uvs` 取 UV1、`decode_polygon_positions` 取位置、世界变换后与 `uv1*uvScale+uvOffset` 的图集块做线性拟合与图集采样统计（瓦片均值/极值/标准差）。
5. **着色器源码**：全树 110 个 `.sl/.slh` 经 `decode_dvpl` 解出原文；方程引文见 §二各条（文件名与行号已随文标注）。
6. **贴图规格与体积**：DDS/PVR3 头解析 + `imagecodecs.bcn_decode` + PIL WebP 重编码实测（2048² BC1 图集 1.6 MB@q92；天空 187–190 KB；IBL specular 全 mip 44 KB）。
