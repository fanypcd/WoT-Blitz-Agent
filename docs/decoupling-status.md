# 解耦：剩余决策、封面图细节与已定案差异（BlitzKit → 本机客户端自产）

> 2026-10-03 整理（自 2026-10-02 版收敛）。**现状 / 接线 / 分发**已并入
> [data-inventory.md](data-inventory.md)（其 §一/§1b 的数据总账——每项"目前来源 / 可替代
> 来源 / 替代完成度"、§四"尚无代码的缺口与非代码障碍"、§五"完成度汇总"是当前的权威口径，
> 2026-10-09 改版为总账形式）；GLB 管线的实现铁律与逐维度差异见
> [local-model-export.md](local-model-export.md)；数据来源与逐字段映射见
> [game-data-sources.md](game-data-sources.md)。
>
> 本文只保留四块独占内容：**已完成时间线**、**待决策口径（C1/C2/C4）**、
> **封面图命名域与素材差异**、**已定案的 BlitzKit 侧差异清单与对外沟通材料**。

## 1. 已完成（时间线）

| 项 | 状态 | 证据/入口 |
|---|---|---|
| `game_data` 冻结故障修复（field2→field32） | ✅ 已在 main（`e3b22c3`；原分支载体已删） | [game-data-sources.md](game-data-sources.md) §6 |
| 两处 pb 字段号 bug（引擎起火率 / 履带阻力） | ✅ 已提交 `5032251` | 报告 A §8；`data/tank_data/*.json` 重生成 |
| **GLB 自产管线** | ✅ **已接线（2026-10-07）** | `data/cache/models/` 已整体换为自产导出：735 辆的 `model.glb` + `collision.glb` 全量 generator = `wotb-agent local sc2 exporter`，与 `data/cache/local_models/` 逐辆逐字节一致（735/735），包与 COS 已随发（[data-inventory.md](data-inventory.md) §一 #8）；工具 `tools/export_tank_glb.py` + `tools/compare_tank_glb.py`；原等价性口径：`collision` **735/735**、`model` **733/735** 逐字节等价（含 UV0/1/2 与节点顺序），规则见 [local-model-export.md](local-model-export.md) §3 |
| **封面图自产管线** | ✅ 可用，未接线（包内仍 BlitzKit 源，抽样 80/80 命中） | `tools/export_tank_icons.py`；**735/735 且全为大图档**（2026-10-10 接入**逐车 `bigIconPath` 声明**后（全部由声明源命中）；详见 §3） |
| **`tanks.pb`/`models.pb` 本机解析器** | ✅ 可用，未接线 | `tools/extract_vehicles.py`（客户端 XML/components/yaml/en.yaml → 同构 JSON）+ `tools/compare_vehicle_data.py`（pb→同形 dict 逐字段对照）+ `tools/emit_vehicle_pb.py`（同格式 pb 编码器）。`data/tank_id_bridge.json` 735 条已固化，**并已证明可纯客户端重建**。全量对照（2026-10-04 首轮）：**全部数值字段 0 不一致**；**2026-10-10 复测**：与 BK 剩余差异共 **122 处**且全部定性——119 枪序（研发序口径）、2 条名字（客户端两源皆无，补表兜底）、1 处 `10625` explosion_radius（BK 错值）、1 辆类别词（`81`，已改为精确 token 判定修掉）；弹种集合"差异"经查为 BK 按 gun module 塌缩（见 §2 C5） |
| `baseRMMap` 通道搬迁（MR 槽） | ✅ 已提交 `923144d` | ch0→G（粗糙度）、ch1→B（金属度）；修后与 BlitzKit 的 G 通道一致 **1010/1011**；[local-model-export.md](local-model-export.md) §4.1 |
| UV1/UV2 / `doubleSided` 对齐 | ✅ | TEXCOORD_1/2（139/18 辆）；`customCullMode==0`（1598/1598 零反例）——均见 [local-model-export.md](local-model-export.md) §3 |

## 2. 待决策口径

### C1 数据模型塌缩（项目侧缺陷，不是客户端不可信）

- **`TrackData` 是单条**（`blitzkit.rs:159`）：一辆车只有一组 `module_id / weight / traverse_speed /
  resistance_hard / medium`，而客户端 `tracks[]` 可以有**多个底盘**，各自有重量/转速/阻力。
- **`track_thickness: Option<f32>` 是单值**（`blitzkit.rs:310`），赋值处
  `if track_thickness.is_none() { track_thickness = thickness }`（933-934）——**取第一个遇到的板厚**，
  不是按模块或顶级底盘确定；消费点 `tank_configs.rs:51` 直接拿它拼 `ChassisArmor`。`gun_thickness` 同型。
- **症状**：多底盘车的履带厚度/阻力只能显示一条，且丢掉了"这条属于哪个底盘"的对应关系。
- **选项**：改 schema 与客户端同构（+2–3 人日）；或接受"取顶级配置"近似并**显式记录**
  （`data-inventory.md` §3.5 的"顶级炮塔 × 顶级主炮"口径即此近似的现状版）。

### C2 车辆/模块本地化名的客户端缺口（2026-10-04 全量对照后的精确口径）

**2026-10-10 复测与勘误（对照口径 `compare_vehicle_data.py`）**：与 BK 的**名称**差异
**145 → 77 → 2**，过程如下（对照总差异 122 处 = 名称 2 + 枪序 119［口径］+ explosion_radius 1
［BK 错值］，见 [data-inventory.md](data-inventory.md) §一注）：
* 145 → 77：修 4 处提取缺陷（见下）；
* 77 → 2：接入**第二字符串源**——客户端运行时下载的**本地化覆盖层**
  （`%LOCALAPPDATA%/wotblitz/DAVAProject/cache/localizations/<lang>.yaml`，带 `.etag`）。
  **此前"24 辆车 + 34 模块客户端确无"的结论是错的**：它们都在覆盖层里，随包
  `Data/Strings/*.yaml` 只是构建时的基础快照、不含上线后新增的活动车与模块名（BP 车在
  `camouflages.yaml` 注册、名字键 `<stem>_Custom[_short]` 只在覆盖层里）——这正是"游戏能正常
  显示名字而只读 `Data/` 的解包查不到"的原因。覆盖层只缓存客户端语言（实测机为 zh-Hans），
  但专名跨语言同形（Turbo/Magnate/Panlong 等 24/24 与 BK 英文名逐字一致）；提取器把它作
  **缺失键回退、只接受拉丁值**。
* 残差 **2 条**（两种来源都没有、BK 自建名）：1 枪（`_90mm_KwK_E_L56`，Tank 881 Edelweiss）
  + 1 履带（`chassis_WZ-135G_FT`，Tank 2161 WZ Blaze）→ 由补充表兜底。
* 依赖注记：覆盖层是 **per-user 运行时缓存**（需客户端登录同步过；新装/未同步时会缺），
  提取链对缺名保持软失败 + XML 标签回退。客户端 `cache/` 下另有
  `dynamicContentLocalizations/<lang>.yaml`（**活动/offer 文案**，键形 `2025-tank-N/Title`；
  实测不含车辆/模块名——名字残差的 2 条在其与 localizations 两处都没有）。

**本轮修的 4 处提取缺陷**（此前把这些误算成"客户端缺失"）：
1. **段前缀按字面查**：客户端条目的 userString 段前缀 ≠ pb 国家名（uk 的 list.xml/shells 用
   `gb_vehicles:`，en.yaml 里 `gb_vehicles` 450 键与 `uk_vehicles` 478 键**并存**——历史结论
   "uk 就是 uk_vehicles"与"uk 用 gb_vehicles"都只对一半，正确做法是按元素自带的前缀字面查）；
   裸键回退会跨系相撞（`_47mm_3pdrAP` 在 usa 段的值是字面 "None"，uk 段才是 'QF AP Mk. IIIT'）；
2. **A/B 变体回退**：重复条目的尾字母变体（`_75mm_M61AB`）无字符串，回退其基础键（仅在基础键
   真实存在时）；
3. **YAML 转义解码**：值里的 `ä`/` ` 等未被解码（3 个名字带字面反斜杠）；
4. **转义感知的值捕获**：`"..."` 正则被转义引号截断（`Maybach HL 295.003 \"Somua\"`）。
* **1 辆**（GB01 Medium Mark I）的 list.xml tags 缺类别词，BK 侧为 mediumTank。
历史口径（报告 A 记 24、按 userString 全名口径测量）与此处 24 辆短名口径一致。

方法与历史发现（保留）：`list.xml` 的 `<userString>#<nat>_vehicles:KEY</userString>` → 用 KEY 查
`Strings/en.yaml.dvpl`。报告 A 称"uk 的前缀是 `gb_vehicles`"**有误**（实测 `uk_vehicles`，按它写会
漏掉整个英系）；list.xml 的 userString 键与模型名不一致的有 7 例（`AMX_50B`→`AMX_50_68t` 等）。
影响与处置：换源后这些车要么回退模型名，要么用 WG API/BK 名表补齐（见上方精确口径）。

### C3 `hull_traverse`（pb field27）与客户端非同量

链路是通的：`blitzkit.rs:425` 读 field27（fixed32）→ `tank_resolver.rs:252` 做 rad/s→deg/s（`×180/π`）
→ 落进 `tank_cache.json` → `web/mod.rs` 在 API 透出。**但前端零引用**，且数值与客户端 chassis 的
`<rotationSpeed>` 不是同一个量（T-34：换算 16.5 deg/s vs 客户端 46 deg/s——后者才是车体原地转向）。
结论：**已解析、已透出、无人消费**；换源时别拿它当车体转速权威值（客户端该取 chassis
`<rotationSpeed>`），保留为对照或在 schema 标废弃。

## 3. 封面图：能力已就绪，但"换素材 ≠ 等价替换"

### 3.1 来源与形态（实测）

| 项 | 值 |
|---|---|
| 大图标 | `Data/Gfx/UI/BigTankIcons/<name>.packed.webp.dvpl`（2312 个，含 `@2x` 与 `_skinN`） |
| 小图标（兜底） | `Data/Gfx/UI/BattleScreenHUD/SmallTankIcons/<name>.packed.webp.dvpl`（1474 个） |
| 容器 | DVPL 壳里**就是裸 webp**（`RIFF…WEBP`）——**剥壳即得，不重编码** |
| 分辨率 | 基础 256×128 / `@2x` 512×256 / 小图 128×32 |

### 3.2 命名域：图标名是**第三套命名**，与模型名不规则对应

客户端没有任何权威清单（车辆 XML 与 `list.xml` 都不引用图标名；`list.xml` 的 `<userString>` 键只是
车辆名本身），所以只能逆出规则。实测样本：

| 模型名 | 图标名 | 差异 |
|---|---|---|
| `Ch01_Type59` | `china-Type59` | 丢 `Ch01_` |
| `Ch_WZ-112v2` | `china-WZ-112v2` | 丢**无数字**前缀 `Ch_` |
| `GB10_Black_Prince` | `britsh-BlackPrince` | 丢 `GB10_`、去下划线、国家标签**错拼** `britsh` |
| `StuGIII` | `germany-StugIII` | 大小写不同 |
| `Oth08_WH_Vindicator` | `other-Oth08_Vindicator` | 丢中缀 `WH_` |
| `S04_Lago-I` | `european-S04_Lago_I` | `-`↔`_` |
| `Oth10_WarDuck` | `WarDuck` | **无国家前缀** |
| `Sherman_Jumbo` | `usa-M4A3E2_Sherman_Jumbo` | 多 `M4A3E2`（来自**显示名**） |
| `M48A1` | `usa-M48A1_Patton` | 显示名是 "M48 Patton"，又是第三种拼法 |

实现（`tools/export_tank_icons.py`）：**权威声明源 + 归一化候选梯子（备选）**。主路径读逐车 `3d/Tanks/Parameters/<nation>/<stem>.yaml.dvpl` 的 `bigIconPath` 声明、按**整文件名（含国家标签）**精确取图（见 §3.3）；仅当声明缺失才走下列归一化梯子。两侧都"去国家标签 → 转小写 →
去非字母数字"后比对；候选 = 模型名 / 去 `XxNN_` 前缀 / 去 `Ch_` 类非数字前缀 / 去 `WH_` 中缀 /
**显示名**（`Strings/en.yaml`）/ 模型名＋显示名多出的词。皮肤变体在排序里被压到最后（不误用）。

### 3.3 覆盖与残差

**735/735，且 735 辆全部为 `big` 档**（0 皮肤档 / 0 小图标；来源与档位记在
`_export_status.json` 的 `match_src`/`grade`）。**匹配的权威来源是客户端逐车声明**：
`3d/Tanks/Parameters/<nation>/<stem>.yaml.dvpl` 的 **`bigIconPath` / `smallIconPath`**
（`~res:/Gfx/UI/BigTankIcons/<名>`；764 份参数文件中 755 份为逐车文件、全含该字段）——**按整文件名
（含国家标签）精确解析**（`ussr-IS_2` 必须命中苏联图标，不能归一化后撞上同键的 `china-IS2`），
**735/735 全部由声明源命中**（唯一特例 `KV-1s-BP`：声明名带资源变体后缀 `ussr-KV_1s_BP.china`、磁盘无此名，按去尾后缀再精确一次命中 `ussr-KV_1s_BP`）。
备选来源（仅在声明缺失时使用，按序）：`camouflages.yaml` 注册表 → list.xml 短名/全名 →
模型名及去前缀变体 → stem＋显示名词 → 人工别名表；弱来源加**唯一性护栏**（`T34_hvy` 的全名
"T34"、`Chi_Ha` 的短名都会撞他车，命中被弃）。
**接入声明源后纠出 3 处跨车误挂**（此前版本曾发生）：`T-34` 曾挂 `china-T34`、`T-28` 曾挂
`usa-T28`；`ST-I`（10753）此前经手核别名表误挂 `ussr-R63_ST_IBD`——那实为 `T-2020`（20993）之档
（全表 30 条逐条反查后仅此 1 条入错）。现分别为 `ussr-T-34` / `ussr-T-28` / `ussr-ST-1`。

2026-10-10 另修三处提取缺陷（当时 730/735、5 辆缺）：① 索引侧对称归一化——候选侧会剥
`XxNN_` 前缀与皮肤后缀，索引侧却不剥，导致"同车不同皮肤/带内部编号"的图标永远匹配不上
（`IS-4` 只有 `_skin` 大图、`Indien_Panzer` 只有 `G88_Indien_Panzer_skin`）；② 小图标命中会
压掉更好的候选（`112 Glacial` 的 128×32 条带压掉了 `china-112_event` 大图）；③ 补 3 条别名
（`F68_..._Chasseur_de_char_46`→`france-CDC`、`Ch23_112`→`china-112_event`、`PzVI_GuP`→
`japan-Tiger_I_GuP`）。**仍刻意不启用模糊匹配**：实验里模糊匹配会落到**别的车**
（`PzV_PzIV`→`PzIV`、`GB24_Centurion_Mk3`→`Oth41_Centurion_Mk3_S2`），宁可留空也不串车；
派生键另加"剥编号须留 ≥2 词"约束（`T34_hvy`→`hvy` 会误撞 `T1_hvy`）与撞键同车校验。

### 3.4 与 BlitzKit 封面是同一幅画（2026-10-10 复核修正）

旧记的"不是同一幅画（NCC 中位 0.21、没有一对 ≥ 0.7）"是**对照方法伪影**：两侧画布尺寸不同
（BK 的画布是裁剪后的小画布），按画布拉伸/信筒缩放比对会让内容整体错位。改按 **1:1 无缩放、
BK 画布左上角对齐**逐辆复核后结论相反——**同一幅画**：姿态/光照/细节一致、内容 bbox 逐像素
同位（如 `T-34` 双侧均为 (25,13)-(128,88)）；BK 只是画布更小（高以 100 为主，554/735，尾至
69；客户端一律 256×128，0 辆比 BK 窄）。量化：**轮廓 IoU 中位 0.986**；灰度平均差中位
**1.76/255**（**216/735 < 1**＝仅 webp 重编码噪声；**32/735 > 5**＝高频纹理＋亚像素重采样车，
最差 `16241 Oth53_Rammer` 均差 29.8、21.7% 像素超 8）。即**换源接近视觉无损**（同源美术、
同像素尺度），不再有"换素材、观感会变"的定性；接线前建议抽查对照图
`data/cache/local_tank_icons/_vs_blitzkit.png`（12 辆＝均差最差 6＋最好 6，左 BK / 右客户端
同区域）与 `data/cache/local_compare/same_art_*.png`。

## 4. 已定案、不再算待办（BlitzKit 侧不可复刻，留作对照基线）

| 类别 | 规模 | 内容 |
|---|---|---|
| 几何 | 2 辆 | `Ch52_WZ_122_6_F3` 元素装配重写；`М4А3Е8_ВР` 的 windows-1252 mojibake |
| 贴图相位 | 158 槽位 | BlitzKit 的 PVR 读法比容器布局晚 16 字节（8 px U 向） |
| MR 金属度位 | 1011 槽位 | BlitzKit 的 MR.B 对不上客户端任一张贴图任一通道（我们取 `baseRMMap` ch1） |
| 图片条目 | 4 辆 | BlitzKit 重复/悬挂的 image 条目（内容相同） |
| 材质名 | 2 辆 | 纯命名（贴图槽与路径逐字相同） |
| 序列化 | 735 / 567 / 77 | 采样器省略默认值；accessor 与 mesh 去重策略 |
| `alphaMode` | 13 辆 / 14 材质 | BlitzKit 侧判据（10 例它漏判、4 例它多判） |

逐维度的实测数字与"怎么读对照结果"见 [local-model-export.md](local-model-export.md) §3/§4/§5。

## 5. 已决事项归档

- **贴图验收口径 = semantic**（按 PBR 语义正确装配），不提供"复刻 BlitzKit 指派"的口径。
  逐通道实测（2026-10-02/03）显示复刻没有意义：对方的 `normal` 取的是**旧法线图**
  `images/<T>_NM`（DXT1）、`metallicRoughness` 的 G 虽与 `baseRMMap` ch0 一致但 **B（金属度位）
  对不上客户端任一张贴图的任一通道**，照抄等于把已知不成立的 PBR 语义写进产物；而 `occlusion`
  两边**本已相同**（都取 `miscMap.R`，1011/1011 逐像素一致）。见 [local-model-export.md](local-model-export.md) §4。
- **`is_collector` 三分显示（原 C3）**：2026-10-03 已解决——`tank_cache.json` 现含
  `is_collector`（与 `is_premium` 同为 `tanks.pb` field13 枚举：1=金币、2=收藏，互斥；
  实测 735 辆：金币 91 / 收藏 338 / 同时为真 0），运行期与消费方均已消费
  （[data-inventory.md](data-inventory.md) §1a）。
- **报告 B 的 35 辆 GLB 缺口**：已由 `LodComponent` 判据 + 全批次导出等规则收敛到 733/735 逐字节
  （[local-model-export.md](local-model-export.md) §3）。
- **UV1/UV2、`doubleSided`、MR 通道摆放**：均已对齐（§1 时间线）。

## 6. 对外沟通（BlitzKit 开发者）

已定稿的英文说明（Ankou SP 最小复现版；更全面的问题清单见 §4 与
[local-model-export.md](local-model-export.md) §4/§5）：

```markdown
**One issue: alphaMode missing on alpha-tested parts (tracks/grills render solid)**

We re-extracted all 735 tanks from a local WoT Blitz client (11.20.0) and diffed against
your artifacts. 10 materials have an `alphatestThreshold` property *and* a colour texture
whose alpha channel really varies, but no `alphaMode` is set — so the alpha mask is ignored
and those parts (tracks, chains, grills, masts) render as solid plates.

Minimal repro (attached: `a2_alphamode_63841.png`): tank 63841 **Pz. IV Ankou SP**,
material `Pz_IV_AusfH_track`. Both sides carry the same 256×256 RGBA texture (alpha range
0–221, alpha channel identical between us). We emit `alphaMode: MASK` / `alphaCutoff: 0.3`
(19.8% of pixels are cut out); BlitzKit emits no alphaMode, so the cut-out is lost.

The other 9: 1137 Predator UM, 3073 T-46, 7041 Turbo, 9329 Titan-150, 19009 AMXmas,
20289 Pirate, 21361 Herdbreaker, 25889 Ranger, 64065 FCM 50 t.
(There are also 4 materials the other way round: `alphaMode: MASK` set, but their texture
alpha is constant 255, so the MASK is a no-op — one of them even has `alphaCutoff: 0`.)

Suggested rule, which matches you on 166 of 169 materials we checked:
`alphaMode = MASK` iff `alphatestThreshold` is present **and** the base-colour texture's
alpha actually varies; `alphaCutoff` = that property's value verbatim (we get exact value
equality with yours: 0.3 / 0.05 / 0.5 / 0.03).
```

附件（由 `tmp_glb_reverify/diff_sheets.py` 生成，英文标签，落 `data/cache/local_compare/`）：
`a2_alphamode_63841.png`（主推：两侧贴图逐字节同源、alpha MAD=0，只差 `alphaMode` 字段）、
`a2_alphamode_7041.png`（链条实例）、`a2_alphamode_19009.png`（履带实例）、
`a1_slots_9489.png`（E-100 槽位指派三行对照）。

## 7. 复跑

```bash
# GLB：自产 + 逐字节对照
python tools/export_tank_glb.py --all --texture-mode semantic --jobs 8
python tools/compare_tank_glb.py --all --audit --jobs 8

# 封面图：自产（默认只用大图标；--allow-small 退小图标）
python tools/export_tank_icons.py --all --allow-small

# 诊断（tmp_glb_reverify/，不入库）
python tmp_glb_reverify/full_diff_inventory.py    # 逐维度差异清单
python tmp_glb_reverify/diff_sheets.py            # 对外沟通对照图（英文标签）
python tmp_glb_reverify/en_yaml_missing.py        # C2 显示名缺失复核
```
