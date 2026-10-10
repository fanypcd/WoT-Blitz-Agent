# 地图光照对齐实施设计（3D 回放场景）

> 2026-10-09 制定 · **状态：待批准（本文只定方案，不动代码/资产）**。
> 依据：[feasibility-map-lighting.md](feasibility-map-lighting.md)（可行性实测报告，含 36 图参数表与
> 8 项未定项）。所有权边界见 [AGENTS.md](../AGENTS.md)：**期 1 在本仓，期 2/3 在 WotbTools**。

---


> **进度（2026-10-09 更新）**：**期 1a 已落地**（`tools/export_map_lighting.py` → 逐图
> `lighting.json`，55 张全量零 warnings；打包器已收入 `map/<key>/lighting.json`）。
> **期 2a 已落地一半**：太阳方向/色/强度 + 环境色（半球光）已由 `lighting.json` 驱动
> （WotbTools `applyMapLighting`，兜底灯保留、会话复位）；**曝光/色调映射对齐（A2）与
> 雾/IBL、以及期 2b 的烘焙光照图未做**。触发这两步的现场：删除 `flatShading` 后
> authored 法线生效，硬编码假太阳的错方位立刻显形（用户报障"面向太阳的面偏暗"）。
> 同批落地的还有**动画混合层**（烟/瀑/浪的滚动纹理）与**期 2b 烘焙光照图**（36 图全量：
> 图集随包 + 逐实例 `aLm` 变换 + 前端 unlit 材质；动态阴影混合与 `pbrLightmap` 未做）——
> 均见 [index.md](index.md) 同名条目；**期 2d（坦克 IBL）亦已落地**（`ibl.webp` + `scene.environment`）；**B2 环境反射遮罩**（5 图，含着色器编译门禁）亦已落地，见 [index.md](index.md)。

## 一、方案总览

一句话：**把客户端已经烘焙/已经配置好的光照数据原样搬到资产包，消费端按客户端着色器源码逐式复刻**——
不做"重新打光"，不做延迟管线，不改任何回放契约。

四刀按收益排序，也是施工顺序：

| # | 内容 | 客户端数据 | 现状 | 收益 |
|---|---|---|---|---|
| 1 | 静态场景用**烘焙光照图** | 每图 2–11 张 2048² 图集 + UV1 × 逐实例 `uvScale/uvOffset` | Lambert 假光 + 0.75 压暗 | ★★★ 最大 |
| 2 | **逐图太阳/环境色/曝光** | `DirectionalLightComponent` | 36 图一套写死光 | ★★★ |
| 3 | **天空穹 + 逐图雾** | 天穹网格（35/36 已在包内）+ sky/flowmap + `SceneRenderConfigComponent` | 纯色背景、无雾 | ★★ |
| 4 | **坦克 IBL** | `IBL/diffuse`+`specular` cube 逐图齐备 | 无环境反射 | ★★ |

**明确不做**（防止方案膨胀）：不重做延迟渲染；不为静态几何实现 CSM（烘焙光照图已覆盖，动态阴影只服务坦克与 `SHADOW_RECEIVER` 批次）；不重建世界空间光照图（UV1 是逐面平面投影，不可反解，实测拟合 maxerr 0.02–0.036 UV）；不改 `PlaybackData`/切面/WASM 契约——**本改动只动资产包与消费端渲染，无引擎发版需求**。

---

## 二、关键设计决策（含被否方案与依据）

### D1 光照图怎么进渲染？—— 原始 UV1 + 逐实例 UV 变换

| 方案 | 结论 | 依据（实测） |
|---|---|---|
| A. 导出期把 UV1 预变换到图集空间（烘进顶点） | ❌ **否** | 同网格的多实例各有不同图集偏移 ⇒ 顶点缓冲不能再共享，实例展开后顶点数涨 10–30×（holland `scenery.glb` 41 MB → 150 MB 量级） |
| B. 逐实例把 `albedo × lightmap` 烘成贴图 | ❌ 否 | 2497 个实例 ↔ 2320 组唯一 (图集, scale, offset)，几乎一实例一贴图 |
| C. **原始 UV1 随导 + 逐节点 extras `{atlas, scale, offset}`** | ✅ **采用** | 几何共享不变（实测 UV1 成本仅 **+8 B/顶点 = 每图 +0.3–3.6 MB**：himmelsdorf 10.5 万顶点、holland 44.7 万）；图集贴图跨节点共享 |
| D. 反解世界空间光照图 | ❌ 否 | UV1 非世界位置线性函数（单网格内逐面投影，拟合残差 41–74 纹素） |

渲染期两种接法，**B 为推荐、A 为可用的过渡**：

- **A（过渡）**：逐节点克隆材质、把 `scale/offset` 放 uniform（stock `MeshLambertMaterial.lightMap` 也支持：`lightMapTex.channel = 1`、`repeat/offset` = scale/offset、`lightMapIntensity = 2π` 正好等于客户端 `×2`）——代价是光照图批次失去 InstancedMesh 合批（himmelsdorf 约 1000 批、forgecity 约 2100 批 draw call）。
- **B（推荐）**：InstancedMesh 保留，UV 变换走**逐实例属性 `vec4`**（16 B/实例，可忽略），自定义材质按客户端公式取样。WotbTools 已有实例化合批（PR #555），加一个 InstancedBufferAttribute 即可。

### D2 图集贴图交付与体积

- 形态：`map/<key>/lightmap/textureN.webp`，**外部引用**（不嵌 GLB——图集被成百个节点共享，嵌入会重复膨胀；GLB 侧只在节点 extras 记路径）。
- 分辨率：先按 **2048² / q92 ≈ 1.6 MB/张**（全量 **+145 MB**）出对照；若用户接受可降到 1024²（**+40 MB**）。降档线：`32_faust` 夜战与 `himmelsdorf` 城市最先看出差异，这两张单独看。
- 逐材质调整：实测场景光照图材质**不带 `materialLightmapAdjustment`**（2497/2497 缺省）⇒ 复刻式里这一项可省；`GLOBAL_TINT`/`FLATCOLOR`（forgecity 121 批带 `flatColor`）仍按源码保留。

### D3 立方图（IBL）交付

- `ibl/diffuse.f{0..5}.webp`（64²）+ `ibl/specular.m{mip}.f{face}.webp`（256²×9 mip），全量 **+44 KB/图**；DDS 面序 = three.js `CubeTexture` 序（+X,−X,+Y,−Y,+Z,−Z），方向需一次校验。
- 消费端：期 2 用 `PMREMGenerator.fromCubemap()` 近似（一行接入）；期 3 若要贴着客户端的 `MipFromRoughness(roughness, mipmapLevel.x)`（`PBR_ROUGHEST_MIP=4.0 / MIP_SCALE=1.2`）逐档取样，再换 `sampler2DArray`/自定义材质。

### D4 天空 / 雾 / 曝光

- **天空**：复用包内已有天穹网格（35/36 图），改 unlit + flowmap 动画。顶点相位公式（`skyobject-materials-vp.sl`）：
  `t = globalTime*flowAnimSpeed; ph = frac(vec2(t, t+0.5)) − 0.5; flowData = vec3(ph*flowAnimOffset, abs(ph.x*2))`；
  片元：`dir = flowmap*2−1; color = lerp(albedo(uv+dir*flowData.x), albedo(uv+dir*flowData.y), flowData.z)`（`skyobject-materials-fp.sl`）。天空不参与雾（材质 flags `VERTEX_FOG: 0`）、不写深度（客户端 `position.w −= 0.0001` 的等效处理）。
- **雾**：按 `vp-fog-math.slh` 在共享 chunk 内复刻（距离项 + 半空间 Lengyel 项 + 大气 `fogAtmosphereColorSky ↔ Sun` 按 `pow(dot(view,lightDir)*0.5+0.5, scattering)` 插值），参数逐图来自 `lighting.json`；天空与 SpeedTree 叶卡按客户端 flags 决定是否参与。
- **曝光**：客户端发布版 filmic 曲线是注释掉的 ⇒ 出屏 = 线性 × exposure。three.js 对应 `toneMapping = LinearToneMapping`（即 `color * toneMappingExposure`），替换现有 `ACESFilmic + 1.15`。exposure 数值来源未在随包 YAML 找到（未定项 #5）→ **以对照图标定**。

---

## 三、期 0：标定（0.5–1 人日，先做，不做完不进期 1 的写盘）

目的：把可行性报告 §七 的 8 项未定项收敛到"可写代码"的程度。**优先级最高的是 #1（太阳方向约定）与 #5（曝光）**。

1. **自证式校验（本仓，无需用户）**：取 2–3 张晴空图（`02_desert_train_dt` / `21_mountain_mnt` / `41_iceworld_ic`），在天空贴图里找最亮斑（太阳），用天穹网格的 UV↔世界几何反投影出该斑的世界方向，与 `−Y` 约定解出的太阳方向比对——一致即闭环；不一致则回头审约定。
2. **IBL/天空方向与 sRGB 校验**：解出的立方图/天空图各出一张 PNG 缩略对照（朝 +X/+Y/+Z 三个面与客户端观感比对）。
3. **A/B 对照（需用户提供客户端截图）**：`himmelsdorf`（城市/阴影硬）、`karelia`（冷调低饱和）、`32_faust_fa_night`（夜战）各一张，机位尽量已知（俯视或平视均可）。工具：WotbTools 侧 headless Chrome + three.js（仿 `frontend/scripts/bake-ground-overhead.mjs`，读本仓 `release/asset_pack`，**是验证工具不是产品 UI**）。
4. **输出**：一份标定记录（追加到 [feasibility-map-lighting.md](feasibility-map-lighting.md) §七，逐项改判"已定/仍待定"），以及三个数值：太阳方向约定、exposure 初值、`×2` 是否保留。

**若用户暂不提供截图**：期 1 全部可做（纯数据搬运，不依赖标定）；期 2 先按 `−Y` + exposure 1.0 + 保留 `×2` 上场，用 WotbTools 本地调试页由用户现场判。

---

## 四、期 1：数据面（本仓，3–5 人日）

### 1a. 新增 `tools/export_map_lighting.py`

输入 `<Data>/3d/Maps/<space>/<space>.sc2.dvpl`（经 `tools/wotbtools/dlc_packs.py` 走 packs 覆盖层），输出 `data/cache/maps/<space>/`：

```
lighting.json          # 见下方 schema
ibl/diffuse.f{0..5}.webp
ibl/specular.m{mip}.f{face}.webp
sky.webp  sky.flow.webp
```

```jsonc
{
  "space": "19_himmelsdorf_hm",
  "sun": { "direction": [x,y,z], "color": [r,g,b], "intensity": 5.0, "ambient": [r,g,b],
           "quaternion": [x,y,z,w], "convention": "dava-light-local-negative-y" },  // 原始值留档
  "ibl": { "diffuse": "ibl/diffuse", "specular": "ibl/specular", "multiplier": 1.0,
           "groundFactor": 0.5, "gamma": 1.0, "dimensions": 256, "enabled": true },
  "fog": { "enabled": true, "atmosphere": {...}, "halfspace": {...}, "linear": {...} },
  // "shadow": { ... }   // 动态阴影曾按此形状导出（G1），2026-10-09 用户决定放弃 ⇒ 未随包交付
  "sky": { "albedo": "sky.webp", "flowmap": "sky.flow.webp",
           "flowAnimSpeed": -0.1, "flowAnimOffset": 1.0 },
  "wind": { "force": 2.0, "speed": 3.0 },
  "warnings": []          // fail-closed：缺件即列名，不猜默认值
}
```

要点：字段名与 `.sc2` 原始键一一对应（便于复核）；**任何组件缺失都在 `warnings[]` 留痕并省略该键**，不填默认值。

### 1b. 扩展 `tools/export_map_glb.py`

1. 批次材质链读取：叶子 NMaterial 的 `textures.lightmap` + `properties.{uvScale,uvOffset}`（5 字节头 + 2×f32 的 blob 布局在期 0 复核）、`GLOBAL_TINT`/`FLATCOLOR`/`flatColor`/`SHADOW_RECEIVER` 旗标。
2. `GlbBuilder` 增加 `TEXCOORD_1` 存取器（现有 UV1 已在 `decode_group_uvs` 解出，只是没写库）。
3. 节点 extras：`{"lm": {"atlas": "lightmap/texture1.webp", "scale": [sx,sy], "offset": [ox,oy]}}`；材质 extras：`{"globalFlatColor": [...], "flatColor": [...], "lmFlags": [...]}`。网格合并键仍为 `(datasource, 材质)`——**因为 UV 变换在节点上、不在网格上**。
4. 图集贴图：BC1 2048²（mip0）→ WebP q92，落 `map/<key>/lightmap/textureN.webp`；同一 `.tex` 路径去重。
5. fail-closed：无光照图槽的批次行为**逐字节不变**（当前产物零回归即验收线）。

### 1c. 打包与分发

- `scripts/export_asset_pack.py` 收入新文件（`map/<key>/lighting.json`、`lightmap/`、`ibl/`、`sky*.webp`），manifest 计数更新。
- 全量重导 36 图 → `tools/composite_overhead.py` 重烘俯视 → `tools/upload_asset_pack_cos.py` 差分上传（`manifest.json` 最后强传）→ 逐对象回拉校验。
- **验收**：与现包对比 `scenery.glb` 除 `TEXCOORD_1`/extras 外**逐字节不变**（几何、材质、贴图嵌入全等）——这条同时是"没搞坏现有产物"的回归证明。

---

## 五、期 2：消费端（WotbTools，5–8 人日）

按这个顺序落，每步都可独立上线、独立回退：

| 步 | 内容 | 判据 |
|---|---|---|
| 2a | `lighting.json` → 逐图太阳（`DirectionalLight`，intensity = 客户端 intensity）+ `LinearToneMapping` + exposure。**2026-10-10 落点（用户裁示“保色相、归亮度”）**：客户端那套 intensity（36 图 3–14）是配它的**自动曝光**用的，我们没有自动曝光 ⇒ 照搬强度会让走 three 内建管线的材质（受光场景件/坦克）过曝 2–5×（米德尔堡地面“发白偏橙”用户报障）。现按 `toneMappingExposure = DEFAULT_SUN_INTENSITY / sun.intensity`（钳 0.05–4）把整图亮度归一到基准档：同一图内 太阳:环境:IBL 相对关系不变（夜图/冷图氛围保留），绝对亮度钉回基准（≈ 固定曝光模拟自动曝光）；未能覆盖客户端自动曝光的动态特性（如实记录）。 | 逐图色调差异出现（`karelia` 冷、`32_faust` 夜、`41_iceworld` 冷蓝） |
| 2b | 静态场景改**烘焙光照图**（D1-B 实例属性；或先用 D1-A 快速上线） | `SCENERY_LAMBERT_EXPOSURE` 与假光整段删除；岩石过曝/红房子偏暗（现有注释里记的病灶）同时消失 |
| 2c | 天空穹（unlit + flowmap）；**逐图雾不做**（2026-10-09 用户裁示：高视角分析用途，雾会削弱远景可见性；`lighting.json` 的 `fog` 块仅作数据备查） | 背景不再是纯色 |
| 2d | 坦克 IBL（PMREM 近似）+ 太阳投影（单级 84 m 阴影近似） | 金属反光出现；坦克投影落到地面 |
| 2e | 画质档门控（低档保留旧假光，中/高/极致走新管线） | 移动端帧率不退化 |

**同步项**（照 [AGENTS.md §Frontend ownership](../AGENTS.md)）：

- 资产面变更须与消费端同 PR 生效顺序：**先发包（COS）再发前端**，或在 WotbTools 内做"字段存在才启用"的降级门（推荐后者，保证老包不炸）。
- APK 内嵌的是 web bundle 快照 + COS 资产：本次属"场景运行时 + 资产消费路径"变更 ⇒ WotbTools `android/gradle.properties` 的 `wotbVersion` **同 PR 递增**（其 `.agents/AGENTS.md` 规则）。
- 本仓不改 `[workspace.metadata.release].version`（无引擎契约变更，不发 Release）。

---

## 六、期 3：精修（按需）

`SHADOW_RECEIVER` 批次的动态阴影 × 烘焙光照图混合（`LMGateFactor` / `landscapeLMGateFactor`）；`GLOBAL_TINT`/`FLATCOLOR` 逐材质；水面（`water-fp.sl` + 逐图 cubemap）；植被风动（`WindComponent`）；`40_moon_mn` 天空实体定位（未定项 #7）；`StandardLightmapAllQualities` 族（02/03/13/15 图）抽验（#8）；坦克 IBL 精确 mip（`MipFromRoughness`）。

---

## 七、验证与验收

- **本仓（自动化）**：`tools/test_export_lighting.py` 纯函数（组件解析、四元数→方向、fog/IBL 字段映射）+ 36 图回归夹具；`export_map_glb.py` 断言（绑光照图的批次必有 TEXCOORD_1、`lightmap` 引用可解析、extras 齐备、无光照图批次输出不变）；打包器 manifest 自洽。
- **消费端**：WotbTools 原有场景接线守卫 + 本方案新增的"lighting.json 存在才启用新光照"降级路径测试。
- **视觉终判归用户**（[AGENTS.md §Visual verification](../AGENTS.md)）：Agent 只交对照图与不变量测试，不自行截图充当验收。建议用户按 `himmelsdorf` → `karelia` → `32_faust` 顺序逐图判。

---

## 八、风险与回退

| 风险 | 缓解 |
|---|---|
| draw call 上升（D1-A 过渡方案） | 期 2b 先上 A，测帧率；不够就切 B（实例属性） |
| 包体积 +40~+155 MB | 1024² 降档可选；天空/IBL 增量可忽略 |
| 光照图口径细节（sRGB/mip/×2） | 期 0 标定；每一项都可在 WotbTools 侧单独开关比对 |
| 与坦克观感联动 | 2a–2d 一次成组上，避免"新场景 + 旧光"中间态 |
| 回退 | 全链可回退：新文件都是新增路径；`scenery.glb` 旧字段全保留；WotbTools 侧降级门（无 `lighting.json` 即走旧路径） |

---

## 九、清单（照此执行）

- [ ] 期 0：天空最亮点自证 → 标定记录（太阳约定 / exposure / `×2`）
- [ ] 期 1a：`tools/export_map_lighting.py` + `lighting.json` + IBL/天空 WebP
- [ ] 期 1b：`export_map_glb.py` 随导 UV1（raw）+ 节点/材质 extras + 图集 WebP（无光照图批次零回归）
- [ ] 期 1c：打包器 + 全量重导 + 俯视重烘 + COS 差分上传 + 回拉校验
- [ ] 文档：`docs/index.md` 进度条目、`data-inventory.md` §2.1 包布局与文件数、`game-data-sources.md`（新管线与陷阱）
- [ ] 期 2a→2e：WotbTools 场景改造（含 `wotbVersion` 递增）
- [ ] 视觉终判（用户）：三张代表图逐项确认
