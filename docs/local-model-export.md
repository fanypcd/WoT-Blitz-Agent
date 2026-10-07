# 坦克模型的本机自产管线与两来源对照

> 2026-10-01 实施记录。写作时的口径是"**不替换运行期数据源**"（BlitzKit 缓存
> `data/cache/models/` 仍是 Web/查看器的来源，本管线写到 `data/cache/local_models/` 并存
> 逐辆对照）；**2026-10-07 已改为替换**：`data/cache/models/` 整体换为本地导出，与
> `local_models/` 逐辆逐字节一致（735/735），包与 COS 随发——现状与接线证据见
> [data-inventory.md](data-inventory.md) §3。可行性依据见
> [feasibility-glb-local-export.md](feasibility-glb-local-export.md)（报告 B）。

## 0. 结论

在 735 辆全量语料上，本机客户端自产的 GLB 与 BlitzKit 产物**逐字节等价**：

| 产物 | 等价 | 说明 |
|---|---|---|
| `collision.glb` | **735 / 735** | 节点集合/顺序 + POSITION/NORMAL/索引原始字节全等 |
| `model.glb` | **733 / 735** | 同上，另含 TEXCOORD_0/1/2；差异 2 辆成因见 §5 |

并排渲染（同一相机/同一光照，**认 `alphaMode: MASK` 镂空**）大多逐辆轮廓 IoU = 1.000，
像素平均绝对差 0.1–1.6（0–255 量纲）；少数含履带镂空的车 IoU 0.966–0.998，成因与 §4.2 的
baseColor 残差同源。25 辆跨九系抽样 + 8 辆定点抽样 + 一组老式车（BT-2/StuG III/RenaultFT/PzIV）
结果一致。贴图**槽位集合**与 BlitzKit 全部对齐（731/735 图片数完全相同、无一辆缺槽位），
逐槽位像素一致度见 §4.2。

> 口径提醒：渲染指标在"认 alpha 之前"看不见镂空差异——早期的 IoU=1.000 只覆盖了形状与实体色，
> 不代表 alphaTest 行为一致。现在渲染器按 glTF 语义丢弃 `alpha < alphaCutoff` 的顶点，该维度
> 才进入指标；上表的 0.966–0.998 就是这条维度加入后暴露出来的既有残差，不是新引入的。

这比报告 B 的验收口径更严：报告 B 只比"按可达节点求和的顶点/索引数"（700/735），
本管线比到**逐属性原始字节**、且要求**节点顺序一致**（733/735）。

## 1. 工具

```bash
# 导出（并行；进度原子写入 <out>/_export_status.json，与 export_map_glb.py 同约定）
python tools/export_tank_glb.py --all                        # 全量 → data/cache/local_models/<id>/
python tools/export_tank_glb.py --tank 9489 --tank 7169
python tools/export_tank_glb.py --all --texture-mode none    # 只比几何，最快
python tools/export_tank_glb.py --game-data <Data 目录>       # 缺省按常见 Steam 路径探测
python tools/export_tank_glb.py --list                       # tank_id → 游戏模型名

# 对照
python tools/compare_tank_glb.py --all --audit               # 数值等价回归
python tools/compare_tank_glb.py --tank 9489 --render        # 三联图：[BlitzKit | 本地 | 差异]
python tools/compare_tank_glb.py --tank 9489 --render-mode tri  # 逐三角精渲（慢，边缘精确）

# 逆向探针（推导批次/开关规则时用）
python tools/probe_switch.py <nation> <stem> [关键字]
```

输出布局刻意与 BlitzKit 缓存平行，便于直接对照：

```
data/cache/models/<tank_id>/{model,collision}.glb        ← 2026-10-07 起 = 本机自产（此前为 BlitzKit 缓存）
data/cache/local_models/<tank_id>/{model,collision}.glb  ← 本机自产（导出落点；已与上者逐辆一致）
data/cache/local_compare/<tank_id>_compare.png           ← 三视图并排 + 差异图
data/cache/local_compare/compare_report.json             ← 逐辆/逐节点差异明细
```

落点选 `data/cache/` 有两个原因：`.gitignore` 已覆盖（不入库），且 `bundle` 特性的
rust-embed 有 `#[exclude = "cache/**"]`，**不会**把这几百 MB 嵌进 exe。

## 2. tank_id → 游戏模型名（不经 BlitzKit）

自包含地从 `data/tanks.pb` 直读：field1 = `tank_id`、field11 = `nation`、**field32 = 游戏模型名**
（`E-100`/`T-34`；field2 是 BlitzKit 的 slug `m4a3e2`，**不能用于文件定位**）。随后按
`3d/Tanks/Parameters/<nation>/<模型名>.yaml` 的 `resourcesPath.blitzModelPath` / `collisionMesh`
解析权威路径，缺失时回退目录约定。这一环即报告 A §3.4 的"路线 B（固化映射表）"，
但映射表是**运行时从 pb 派生**的，不额外入库。

## 3. 实现铁律（违反任一条都会静默出错）

以下每条都是本轮实测得出的**新**结论，报告 B 未覆盖或与其表述不符：

1. **UV 偏移随 `vertexFormat` 变化，不能硬编码 floats 6:8。** 顶点流的 UV0 偏移由
   `vertexFormat` 位掩码决定：`bits{0,1}` → 24 B（UV 在 floats 6:8，如 E-100）；
   `bits{0,1,2}` → 28 B（UV 在 floats 7:9，中间那个 4 字节通道实测是 NaN，如 P44_Pantera）。
   按 6:8 硬读会整列错位（影响 59 辆）。实现见 `Geometry.uvs()`。
   判据：偏移由格式位**确定性**给出，正常直接采信；只防"整列落在非有限值"这一种猜错，
   **不要**加"数值幅度上限"守卫——UV 平铺会合法地超过 1（实测有 89.9），也不要求全有限
   （实测有网格 UV0 含 NaN，交由 `canon_nan` 规范化）。
2. **每个部件要导出所有通过筛选的批次，而不是只取一个。** 客户端批次激活规则与地图一致
   （`lodIndex ∈ {0, -1}` 且 `switchIndex ∈ {activeState, -1}`），但**同一部件可能有 0000/0001
   两个 LOD0 批次**（`SU-152_exp`/`PzVI_Tiger_P`/`Cruiser_Mk_II` 等），只取一个会漏几何。
   `Shadow_Material` 批次丢弃。
3. **实体必须有 `LodComponent`。** 这一条干净地收敛了报告 B §5.0 归的 A/B/C/D 四类缺口
   （报告估 1–2 人日）：主体部件全部具备；游离对象（`Object117`、`polySurface2`）、
   动画骨持有节点（`hull_anim_bore`）、引用的子场景（`EagleSpirit2.sc2`）都没有，
   BlitzKit 产物同样不含。
4. **不要按"动画骨骼"排除 `_anim_*`。** 骨架子树（`gun_03_mask_anim_tower`、`umbrella_anim`、
   `..._fish_anim`、`hull_panels_anim_1` 等）BlitzKit 是**保留**的——按名字排除会反向丢掉
   5 辆的几何。要排除的是烘焙 LOD 变体实体（`_lod\d`，如 `chassis_track_R_lod2`）。
5. **坦克不做实体可见位过滤。** 客户端进场隐藏的皮肤/开关态子树（`visibility=2`，如
   `hull_hide_elements_skin3`）BlitzKit 产物里是**保留**的。这与 `tools/export_map_glb.py`
   的地图规则**相反**，属两套产物的目标差异，不要互相套用。
6. **老式车的贴图内联槽名是 `albedo` / `normalmap`**（不是 `baseColorMap`/`baseNormalMap`），
   且没有 RM/MISC；缺内联时再走命名约定 `3d/Tanks/<Nation>/images/<材质名去 _mtr>`
   （`T_34_mtr` → `images/T-34`、`T_34_track_mtr` → `images/T-34_track`；后缀 `_NM` 法线、
   `_RM`/`_MISC`/`_MASK`）。补上内联 legacy 槽后覆盖率达 734/735，报告 B §3.4 估的
   "648/735 用一条直白规则"是被高估了难点——它没用内联 legacy 槽。
   ⚠️ 材质名到文件名的还原要**按 stem 前缀**做（材质名是模型名把 `-` 写成 `_`），
   全局 `_`→`-` 替换会把 `M6E2BP_grill` 里的真下划线改错。
7. **NaN 规范化到 `0x7fc00000`，索引按最大索引自适应 uint16。** 客户端源数据本身含
   `0xffc00000`（E-100.scg 54 处），BlitzKit 的 JS 管线把它规范化了；不规范化就只能做到
   "数值相等"而非逐字节相等（报告 B §3.1 的两项"否掉逐字节"表述由此消解）。
8. **碰撞按装甲板号切连续 run**，命名 `<part>_armor_<N>`——与客户端三角序一致，735/735 复现。
9. **`.tex` 路径按 base 目录相对解析（`../` 必须归一化，不能剥前缀）。** 跨目录复用贴图写作
   `../German/images/Hetzer_GuP.tex`（日本联动车用德国 Hetzer 的贴图），剥掉 `../` 会拼出
   `<Nation>/German/images/...` 这种不存在的路径。早期实现如此，导致 **196 辆**静默丢槽位、
   1 辆（`G09_Hetzer_GuP`）**一张贴图都没有**。
10. **扩展名回退链必须包含 `.dx11.pvr.dvpl`。** 部分槽位在 PC 包内只有 pvr 版而没有 dds 版
    （如 BT-2 的 `images/BT-2_track` 仅 pvr，而其 `_NM` 有 dds），症状是"法线解出来了、
    baseColor 没有"——影响 **160 辆**。解码复用 `tools/export_map_glb.py` 的 `decode_pvr3()`。
    ⚠️ **保留它的垂直翻转**：坦克侧 DDS 不翻转、PVR3 翻转，两者朝向相反。这不是笔误——
    本机把 13 个 PVR 来源的 baseColor 逐个与 BlitzKit 比对，**13/13 都在翻转后更接近**
    （545 `T1_track`：原样 MAD 43.6 / 翻转 23.1）。"对齐 DDS 路径"会让所有 PVR 贴图上下颠倒。
11. **alphaTest 材质的判据是"`alphatestThreshold` 属性存在 **且** 其色贴图的 alpha 真的有变化"。**
    `alphaCutoff` **直接取该属性值**（实测与 BlitzKit 逐值相等：0.3 / 0.05 / 0.5 / 0.03 全对）。
    只凭属性存在会把 24% 的材质误判成 MASK（169 例实测），加上"alpha 真有变化"后 **1561/1575
    材质与 BlitzKit 一致（99.1%）**。
13. **`doubleSided` 与 `alphaMode` 无关，判据是客户端的 `customCullMode == 0`**（DAVA cull mode
    NONE = 不剔除 ⇒ 双面）。全量实测 **1598/1598 材质零反例**：810 个 `cullMode=0` 全为双面、
    786 个字段缺省 + 2 个 `=2` 全为单面。
    ⚠️ 该字段可能只在**皮肤变体** `configArchive_N` 里而不在 NMaterial 顶层
    （`F34_ARL_V39_BP_flag_R_mtr`、`It115_Rinoceronte_mtr` 都是这样），所以 `_flatten()` 除了
    `textures`/`properties` 还要把它带出来。
    ⚠️ 早期实现把 `doubleSided` 与 MASK 绑死（"MASK 就双面"），实测 **29 个材质不一致**
    （一半是 BK 双面而我们漏、一半相反）——那条"绑定"是错的，BlitzKit 并没有这么干。
    ⚠️ 早期实现用 `attenuationBoxPosition` 当判据 → **407 个 BlitzKit 标为 MASK 的材质被输出成
    不透明**；且 `_encode_webp` 强转 RGB → **1088 个材质的色贴图丢了 alpha**。两者叠加会让履带
    在查看器里渲染成**实心板**而不是镂空履带。修后 alphaMode 不一致降到 13，alpha 缺失归零。
12. **UV 集要把 `vertexFormat` 里出现过的**全部**导出（bit3/4/5 = UV0/UV1/UV2，各 8 字节），
    不是只导 UV0。** BlitzKit 会分别导成 `TEXCOORD_0/1/2`（实测 **139 辆有 UV1、18 辆有 UV2**）；
    只导 UV0 会在这批车上少 accessor、且缺 UV1/UV2 数据（客户端用 UV1 采样镂空/贴花层）。
    偏移同样是确定性的：UV0 在 `low`、UV1 在 `low+8`、UV2 在 `low+16`。
    修后**节点级 UV1/UV2 存在性 0 处相反**，且 733 辆的 UV0/UV1/UV2 原始字节全等。

## 4. 贴图：移交语义口径，不提供"复刻 BlitzKit 指派"

`--texture-mode` 只有 `semantic`（缺省）/`none`。**刻意不提供**复刻 BlitzKit 指派的口径：其指派
在 PBR 语义上不成立（详见 §4.2 的逐通道实测），且 `occlusion` 的 R/B 两通道没有客户端来源，
复刻不可能逐字节一致，留着只会误导。要看 BlitzKit 的实际贴图，直接看它的产物或本工具的三联图。

语义口径装配：

| glTF 槽 | 客户端来源 | 通道处理 |
|---|---|---|
| `baseColorTexture` | `baseColorMap` /（老式车）`albedo` | 保留 alpha（是否真用由"alpha 是否有变化"决定） |
| `normalTexture` | `baseNormalMap` /（老式车）`normalmap` | BC5 双通道按 glTF 约定重建 z |
| `metallicRoughnessTexture` | `baseRMMap` | §4.1 的通道搬迁（**必须搬**，不能原样返回） |
| `occlusionTexture` | `miscMap.R` | R → 三通道灰度 |

BC1/2/3/5 解码自写（含 DAVA 把 BC5 装在 DXT5 fourcc 里的情形，`pfFlags` bit31 置位），
PVR3 复用地图导出器的解码器。

### 4.1 `baseRMMap` 的通道语义与摆放（实测）

客户端 `baseRMMap` 是 BC5 双通道（解码后落在 R/G 位、B 位补零）。**哪个通道是粗糙度**用分布
特征判别（金属度图是物理二值量，粗糙度是连续量）：

| 车 | ch0 极值(<0.1 或 >0.9)占比 | ch1 极值占比 | ch1 的极值里 0/1 占比 | 结论 |
|---|---|---|---|---|
| E-100 | 0.156 | 0.692 | **1.00** | ch1 二值 → **ch1 = 金属度**、ch0 = 粗糙度 |
| IS-7 | 0.164 | 0.615 | 1.00 | 同上 |
| Progetto | 0.179 | 0.705 | 1.00 | 同上 |

glTF 规定 `metallicRoughnessTexture` **G=粗糙度、B=金属度**，所以必须**搬通道**
（ch0→G、ch1→B）。⚠️ 早期实现原样返回 `(ch0, ch1)`，导致 glTF 采到 G=ch1（把金属度当粗糙度）、
且 R 位（glTF 不采样）白占——全量实测**我们的 G 与 BlitzKit 在 1011/1011 上都不一致**；
搬通道后 **G 一致 1010/1011**。三通道来源（老式车的 `images/<T>_RM`）通道语义未证实，仍原样保留。

### 4.2 与 BlitzKit 的贴图一致度（全量实测）

**槽位集合完全对齐**：731/735 图片数相同，其余 4 辆只差 1–2 个图片条目（我们把同一文件的
多个引用折叠成一个 image，BlitzKit 保留重复/未引用的条目），**无一辆缺槽位**。

按**解码后像素**逐槽位比对（缩到 64²、MAD<8 判为一致）：

| 槽位 | 一致 | 接近 | 不同 | 说明 |
|---|---|---|---|---|
| `baseColorTexture` | **1414 / 1575**（89.8%） | 46 | 115 | 主要同源；残差见下 |
| `occlusionTexture` | **1011 / 1011**（100%） | 0 | 0 | 两边都取 `miscMap.R` 灰度（单点 MAD 0.16–0.42） |
| `normalTexture` | 554 / 1568 | 2 | **1012** | **按设计不同**：BlitzKit 取**旧法线图** `images/<T>_NM`（DXT1），我们取 PBR `images_pbr/<T>_NM`（BC5 + 重建 z） |
| `metallicRoughnessTexture` | G 通道 **1010/1011** / B 通道 0/1011 | — | — | G（粗糙度位）两边都取 ch0 ✅；**B（金属度位）只有我们取到真值**（ch1），BlitzKit 的 B 对不上客户端任一张贴图任一通道（最接近也差 26+） |

**逐通道实测**（E-100 / `E_100_mtr`，把 BK 每个输出通道与客户端各贴图各通道逐一比对）：

| BK 的槽 | 实测来源 |
|---|---|
| `baseColor` | `E_100_BC` 三通道（R/G/B MAD 0.17/0.21/0.20）✅ |
| `normal` | **legacy `images/E-100_NM`（DXT1 旧法线）三通道**（0.34/0.40/0.74） |
| `MR` | G = `E_100_RM` 的 **ch0**（0.48）✅；R ≈ 常量 5；**B 对不上任何通道**（最接近 27.7） |
| `occlusion` | `E_100_MISC` 的 R 通道灰度（0.42）✅ |

⚠️ **对报告 B §3.3 的更正**：该表记 `normalTexture ← miscMap.R 灰度`，**实测不成立**——
`miscMap` 的任一通道与 BK 的 `normal` 都差 60+，BK 的 `normal` 实为 legacy `normalmap`；
其记的 `occlusion ← baseRMMap.R` 同样不成立（全量 1011/1011 与 `miscMap.R` 一致）。
`metallicRoughness ← legacy normalmap` 也不准确：BK 的 MR.G 实为 `baseRMMap` 的 ch0。

`normal` 那 554 个"一致"有确定解释：**551/554 是老式车**——老式车没有 PBR 法线槽，
BlitzKit 与我们都退到 legacy `normalmap` 同一份文件，所以一致。

**baseColor 残差全部来自 PVR3 来源的色贴图**（157 个"不同/接近"），且**成因已定**：同一张图，
我们与 BlitzKit 相差一次垂直翻转（已知）加**一个 8 像素的 U 向流位移（16 字节）**。修复该位移后
MAD 从 27–49 降到 4.6–9.5。

谁是地面真值？**残差在 BlitzKit 侧**，三条证据：

1. **分来源一刀切**：1414 个 DDS 来源的 baseColor **原样即吻合**（0 个需要位移），而 158 个
   PVR3 来源的**没有一个原样吻合**、全部需要这 16 字节（`tmp_glb_reverify/pvr_offset_verify.py`）。
   同一个解码器、同一批几何，只有 `.pvr` 路径系统性偏移 → 偏差就在这条路径上。
2. **用客户端自己的数据判定相位**：同材质的**法线贴图是 DDS**（我们与 BlitzKit 已逐像素一致），
   法线与色贴图必然像素对齐。做色↔法的归一化互相关取最佳水平位移：**7 例中 6 例我们更对齐**
   （另一例双方 NCC 都 ≈ 0，属噪声），且我们的最佳位移 ≈ **+1 px**（不是 8）。
3. **容器长度算术支持我们的起点**：`len(payload) − mip链长 = 83`，正落在 `PVR\x03CRC_ len crc`
   这 16 字节块之后；若按 BlitzKit 的起点（=83+16）则 mip 链比文件短 16 字节，等于文件被截断——
   对已发布的游戏数据不合理。

故这条**不作为待修项**：保留我们的解码（跟随容器布局、且与客户端法线自洽）。
⚠️ 早先本文曾把此处判为"BlitzKit 的贴图与客户端不是同一张图 / 素材版本不同"，那是**错的**——
是同一张图，差一个固定的 16 字节相位，且相位差在 BlitzKit 侧。

alpha 相关口径修好后（见 §3 第 11 条）：**alphaMode 与 BlitzKit 一致 1561/1575（99.1%）**、
**色贴图 alpha 缺失归零**；应用 cutoff 后的不透明像素占比与 BlitzKit 逐位相同（BT-2 0.887/0.887、
StuG III 0.897/0.897、VK2801 0.903/0.903）。

## 5. 残余差异（均为 BlitzKit 侧行为，不建议复刻）

**几何（2 辆）**

| 车 | 现象 | 判断 |
|---|---|---|
| `Ch52_WZ_122_6_F3`（12849） | BlitzKit 把 `hull_nc`/`turret_01_nc` 提升为 `hull`/`turret_01` 的直接子节点，并丢弃 `*_hide_elements_switch` 容器与其 `visibility=2` 的 `_nc_skin` 兄弟；本管线按客户端实体树原样保留 | BlitzKit 侧对 `TankElementComponent`/`ScenarioComponent` 元素做了装配重写，未复刻。几何内容相同，只是层级与命名不同 |
| `М4А3Е8_ВР`（21281） | 两侧节点树完全对应，仅**名字编码**不同：BlitzKit 是 UTF-8 被按 latin1 解出的 mojibake（`Ðœ4Ð3Ð•8_Ð’Ð `），本管线是正确西里尔文 | BlitzKit 侧编码缺陷，**不应复刻** |

**贴图（158 个槽位）**：PVR3 来源的 baseColor——BlitzKit 的图相对容器布局偏 16 字节（8 像素）。
证据与判定见 §4.2；我们跟随容器布局且与客户端法线自洽，故不复刻该偏移。

几何那两辆只影响节点名/层级，不影响几何与渲染（该辆渲染 MAD 0.2）。

## 6. 对照结果怎么读

`compare_report.json` 里有**逐节点**明细：`only_blitzkit`/`only_local`（节点集合差异）、
`diff_position`/`diff_normal`/`diff_uv`/`diff_index_values`（原始字节差异）。渲染三联图的差异列：
**红 = 仅本地有、蓝 = 仅 BlitzKit 有**，灰底是两边都有的像素按 3 倍增益显示的残差（越接近全白越一致）。
`ink` 字段给出两边的轮廓像素占比，用于排除"两图皆空所以 IoU 假 1.0"。

渲染默认用向量化顶点泼溅（`points`，全量 735 辆数十秒级），并按 glTF 语义处理 alphaTest
（`alphaMode: MASK` 的材质按 UV 采样 alpha，`alpha < alphaCutoff` 的顶点不写入）；
要看 1 像素级的轮廓差用 `--render-mode tri`（逐三角光栅，慢两个量级，**不做 alpha 遮罩**）。

诊断脚本（不入库，放 `tmp_glb_reverify/`）：`texture_mismatch.py` 做逐槽位配对归因
（尺寸组合 + 像素 MAD 分档 + normal 一致项的来源验证），`basecolor_attrib.py` 对 baseColor
高差异项做来源类型归因。

## 7. 复跑

```bash
python tools/export_tank_glb.py --all --texture-mode semantic --jobs 8   # ~4.5 分钟（含贴图解码）
python tools/compare_tank_glb.py --all --audit --jobs 8                  # ~8 秒
python tools/compare_tank_glb.py --all --render --size 360 --jobs 6      # 全量三联图
```

`src/` 与既有 `tools/` 文件未作任何改动；本管线是纯新增，且不写入 `data/cache/models/`。
