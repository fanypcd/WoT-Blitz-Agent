# 解耦：剩余决策、封面图细节与已定案差异（BlitzKit → 本机客户端自产）

> 2026-10-03 整理（自 2026-10-02 版收敛）。**现状 / 接线 / 分发**已并入
> [data-inventory.md](data-inventory.md)（其 §三"已备好未接入"、§四"尚无代码的缺口"、
> §五"接入状态一览"是当前的权威口径）；GLB 管线的实现铁律与逐维度差异见
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
| **GLB 自产管线** | ✅ 可用，未接线 | `tools/export_tank_glb.py` + `tools/compare_tank_glb.py`；`collision` **735/735**、`model` **733/735** 逐字节等价（含 UV0/1/2 与节点顺序）；规则见 [local-model-export.md](local-model-export.md) §3 |
| **封面图自产管线** | ✅ 可用，未接线 | `tools/export_tank_icons.py`；**730/735**（详见 §3） |
| **`tanks.pb`/`models.pb` 本机解析器** | ✅ 可用，未接线 | `tools/extract_vehicles.py`（客户端 XML/components/yaml/en.yaml → 同构 JSON）+ `tools/compare_vehicle_data.py`（pb→同形 dict 逐字段对照）；`data/tank_id_bridge.json` 735 条已固化。全量对照（2026-10-04）：**全部数值字段 0 不一致**，残差仅 BK 侧枪序（238，非规范）、客户端无本地化的模块/车名（≈145，BK 用自家 DB 补齐）、4 辆 BK 弹种集合不同、1 辆类别词缺失（见 §2 C5） |
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

`tools/compare_vehicle_data.py` 对 735 辆交集的对照结果：
* **24 辆车**的短名不在客户端 `en.yaml`（Turbo/M41D/Spark/Magnate…，联动/BP 车为主）——
  提取器回退模型名，BlitzKit 用自家 DB 补齐；
* **≈73 个弹种 + ≈46 个模块**的显示名客户端侧缺失或为脏值（字面 `"None"`、通用名
  "APCR Shell"），BK 侧均为专名——模块/弹种本地化不能完全来自客户端；
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

实现（`tools/export_tank_icons.py`）：**归一化索引 + 候选梯子**。两侧都"去国家标签 → 转小写 →
去非字母数字"后比对；候选 = 模型名 / 去 `XxNN_` 前缀 / 去 `Ch_` 类非数字前缀 / 去 `WH_` 中缀 /
**显示名**（`Strings/en.yaml`）/ 模型名＋显示名多出的词。皮肤变体在排序里被压到最后（不误用）。

### 3.3 覆盖与残差

**730/735**（3 个退到小图标兜底）。未命中 5 辆：`3361 T1_hvy`、`13841 Indien_Panzer`、
`59137 R71_IS_2B`、`63553 F68_AMX_Chasseur_de_char_46`、`63585 PzVI_GuP`——客户端大/小图标目录
均无以模型名或显示名可确定对应的条目。**刻意不启用模糊匹配**：实验里模糊匹配会落到**别的车**
（`PzV_PzIV`→`PzIV`、`GB24_Centurion_Mk3`→`Oth41_Centurion_Mk3_S2`），宁可留空也不串车。

### 3.4 ⚠️ 与 BlitzKit 封面不是同一幅画

保持宽高比（信筒缩放）后逐辆比对：**NCC 中位 0.21、没有任何一对 ≥ 0.7**（BlitzKit 的是它自己的
渲染/裁切、约 147×100 且逐车变尺寸；客户端的是官方 2D 肖像、256×128）。即**换的是素材、
不是等价替换**，前端观感会变。人工判定用对照图：
`data/cache/local_tank_icons/_vs_blitzkit.png`（12 辆，NCC 最高/最低各 6）。

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
