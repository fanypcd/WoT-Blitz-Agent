# 解耦：BlitzKit → 本机客户端自产（现状与已定案差异）

> **现状 / 接线 / 分发**见 [data-inventory.md](data-inventory.md)（其 §一/§1b 数据总账、
> §四"剩余缺口与依赖注记"、§五"完成度汇总"是权威口径）；GLB 管线的实现铁律与逐维度
> 差异见 [local-model-export.md](local-model-export.md)；数据来源与逐字段映射见
> [game-data-sources.md](game-data-sources.md)。
>
> 本文只保留四块独占内容：**解耦现状**、**已定案口径（C1/C2/C3）**、
> **封面图命名域与素材差异**、**已定案的 BlitzKit 侧差异清单与对外沟通材料**。

## 1. 解耦现状（已完成）

运行期 11+1 项数据已全部客户端化：BlitzKit 只剩"交叉校验对象"角色（`fetch-blitzkit`
保留）+ 两处缺失兜底（GLB / 图标，休眠）。各项接通状态：

| 项 | 状态 | 入口 |
|---|---|---|
| `game_data` 冻结故障修复（field2→field32） | ✅ 已在 main | 见 git 历史与 [data-inventory.md](data-inventory.md) §一 #4 |
| 两处 pb 字段号 bug（引擎起火率 / 履带阻力） | ✅ 已修复 | 报告 A §8；`data/tank_data/*.json` 重生成 |
| **GLB 自产管线** | ✅ 已接线 | `data/cache/models/` 为自产导出：738 辆 `model.glb` + `collision.glb`（generator = `wotb-agent local sc2 exporter`），与 `local_models/` 逐辆逐字节一致，包随 Alibaba asset origin 发布（[data-inventory.md](data-inventory.md) §一 #8）；工具 `tools/export_tank_glb.py` + `tools/compare_tank_glb.py`；等价性口径（对照基准为 735 辆时代）：`collision` 735/735、`model` 623/735 逐字节等价（含 UV0/1/2 与节点顺序），规则见 [local-model-export.md](local-model-export.md) §3；**其中 111 辆为有意分歧**（静态变换烘焙 / 状态过滤，见该文 §5.1） |
| **封面图自产管线** | ✅ 已接线（客户端剥壳字节为现役来源；BlitzKit 退为缺失兜底/对照） | `tools/export_tank_icons.py`；**738/738 且全为大图档**（逐车 `iconPath` 声明全命中；接线复核见 §3） |
| **`tanks.pb`/`models.pb` 本机解析器 + 换源转正** | ✅ 已接线 | `tools/extract_vehicles.py`（客户端 XML/components/yaml/en.yaml → 同构 JSON）+ `tools/compare_vehicle_data.py`（pb→同形 dict 逐字段对照）+ `tools/emit_vehicle_pb.py`（同格式 pb 编码器）；`data/tanks.pb` / `data/models.pb` 现为本机解包编码（同格式 pb + 补充表），BlitzKit 降为**对照校验源**。`data/tank_id_bridge.json` 738 条已固化，**并已证明可纯客户端重建** |
| `baseRMMap` 通道搬迁（MR 槽） | ✅ 已对齐 | ch0→G（粗糙度）、ch1→B（金属度）；修后与 BlitzKit 的 G 通道一致 1010/1011；[local-model-export.md](local-model-export.md) §4.1 |
| UV1/UV2 / `doubleSided` 对齐 | ✅ | TEXCOORD_1/2（139/18 辆）；`customCullMode==0`（1598/1598 零反例）——见 [local-model-export.md](local-model-export.md) §3 |

**换源对照口径（收口数字）**：与 BK 全量对照剩余差异共 **122 处**且全部定性——
119 枪序研发序口径 + 1 处 `10625` explosion_radius（BK 错值，随换源修复）+ 2 条名字
（客户端两源皆无，补表兜底）；弹种集合"差异"经查为 BK 按 gun module 塌缩（见 §2 C4）。
弹种 id 常量偏移（`7009`/`12929`）经查为 BK 侧按 gun module 塌缩 + 孤儿弹种族所致，
客户端数据自洽。转正回归三门（tank_cache 逐字段 / `armor_model.gun`×game_data 矛盾
15→0 / GLB 节点号抽样）见 [game-data-sources.md](game-data-sources.md) §5.2b。

## 2. 已定案口径

### C1 数据模型塌缩（项目侧缺陷，不是客户端不可信）——✅ 已修复

`TankModelInfo.track_thickness/track_origin` 两个"map 迭代序先到先得"单值标量已删除，
改为 `tracks: BTreeMap<module_id, TrackModelData{thickness, origin}>` **逐底盘保留**
（field5 map key = 底盘 module_id，与 tanks.pb `tracks[].module_id` 同域，738/738 全中）。
消费端（`tank_configs.rs`）按**顶级底盘**（tanks.pb `tracks` 末位，与 `hull_traverse` /
"顶级炮塔 × 顶级主炮"同口径）语义选取；pb 省略 origin（全 0 消息）时消费端归一化
(0,0,0)——`model_origins` 不因 schema 改动隐去，发射语义与旧解析一致。

- **行驶侧无塌缩**：`TankFullData.tracks: Vec<TrackData>`（重量/转速/阻力逐底盘）本就完整；
  `GunModelInfo.thickness` 已是逐炮。
- **影响面**：多底盘车 243/738，其中厚度/原点逐底盘不同的 6 辆（4369/5729/12081/18305/
  21569/21825）+ 原点不同的 6721——共 7 辆 `tank/{id}.json` 变更（`armor_model.chassis`
  与 `model_origins.track` 改取顶级底盘值），其余 731 辆逐字节不变。
- **同构三角同步**：`extract_vehicles.py`（客户端逐 chassis）、`emit_vehicle_pb.py`
  （field5 逐底盘编码）、`compare_vehicle_data.py` 镜像（tracks map）；
  `data/cache/local_pb` 已按新 schema 重生成，**全量对照复跑仍恰为既有 122 处定性差异、
  tracks map 零新增差异**。
- **测试**：`parse_models_pb_armor_boxes` 锚定 IS-7（单底盘 + 空 origin→raw None）与
  4369（双底盘 10.0/15.0 逐条保留）。

### C2 车辆/模块本地化名的客户端缺口（精确口径）

**与 BK 的名称差异 = 2 条**（对照口径 `compare_vehicle_data.py`；对照总差异 122 处 =
名称 2 + 枪序 119［口径］+ explosion_radius 1［BK 错值］）。名称面由**两个字符串源**共同
覆盖（见 [game-data-sources.md](game-data-sources.md) §2.2）：

- 随包基础快照 `Data/Strings/<lang>.yaml.dvpl`（前缀键字面查 + 转义解码等 4 处提取规则）；
- **运行时本地化覆盖层**（`%LOCALAPPDATA%/wotblitz/DAVAProject/cache/localizations/`）——
  随包 strings 只是构建时快照、**不含上线后新增的活动车与模块名**（BP 车在
  `camouflages.yaml` 注册、名字键 `<stem>_Custom[_short]` 只在覆盖层里），这正是
  "游戏能正常显示名字、只读 `Data/` 的解包查不到"的原因。覆盖层按客户端语言缓存
  （实测机为 zh-Hans），但**专名跨语言同形**（Turbo/Magnate/Panlong 等 24/24 与 BK
  英文名逐字一致）；提取器把它作**缺失键回退、只接受拉丁值**。
- 残差 **2 条**（两种来源都没有、BK 自建名）：1 枪（`_90mm_KwK_E_L56`，Tank 881
  Edelweiss）+ 1 履带（`chassis_WZ-135G_FT`，Tank 2161 WZ Blaze）→ 由补充表兜底。
- 依赖注记：覆盖层是 **per-user 运行时缓存**（需客户端登录同步过；新装/未同步时会缺），
  提取链对缺名保持软失败 + XML 标签回退。`dynamicContentLocalizations/<lang>.yaml`
  （活动/offer 文案）实测不含车辆/模块名。

**名字面提取规则（此前误算成"客户端缺失"的 4 处）**：
1. **段前缀按字面查**：客户端条目的 userString 段前缀 ≠ pb 国家名（`gb_vehicles` 与
   `uk_vehicles` 并存——正确做法是按元素自带的前缀字面查；裸键回退会跨系相撞，
   如 `_47mm_3pdrAP` 在 usa 段是字面 "None"、uk 段才是 'QF AP Mk. IIIT'）；
2. **A/B 变体回退**：重复条目的尾字母变体（`_75mm_M61AB`）无字符串时回退其基础键
   （仅在基础键真实存在时）；
3. **YAML 转义解码**（`ä` 等；3 个名字带字面反斜杠）；
4. **转义感知的值捕获**（`"..."` 正则会 被 `\"` 截断，如 `Maybach HL 295.003 \"Somua\"`）。

另：list.xml 的 userString 键与模型名不一致的有 7 例（`AMX_50B`→`AMX_50_68t` 等）；
`list.xml` tags 缺类别词 1 辆（GB01 Medium Mark I，BK 侧为 mediumTank）。

### C3 `hull_traverse`（pb field27）与客户端非同量

链路是通的：`blitzkit.rs` 读 field27（fixed32）→ `tank_resolver.rs` 做 rad/s→deg/s
（`×180/π`）→ 落进 `tank_cache.json` → `web/mod.rs` 在 API 透出。**但前端零引用**，
且数值与客户端 chassis 的 `<rotationSpeed>` 不是同一个量（T-34：换算 16.5 deg/s vs
客户端 46 deg/s——后者才是车体原地转向）。结论：**已解析、已透出、无人消费**；
换源时别拿它当车体转速权威值（客户端该取 chassis `<rotationSpeed>`），保留为对照或在
schema 标废弃。

### C4 BlitzKit 弹种集合差异 = 其按 gun module 塌缩

BK 侧弹种集合与客户端不一致的根因是**按 gun module 塌缩 + 孤儿弹种族**，不是客户端
数据问题；共享炮 `<shots>` 为权威后 0 配对失败（见 [game-data-sources.md](game-data-sources.md) §2.2）。

## 3. 封面图：客户端剥壳字节（命名域与素材差异）

> 现役来源：`export_tank_icons.py --all`（738/738，逐车 `iconPath` 声明全命中）→ 同步进
> `cache/tank_images/`（两目录逐字节一致）→ 随包物化 + manifest 重算。接线复核：alpha
> 轮廓与 BK **100% 一致**；BK 为二次重编码 + 更紧 1px 裁剪（art 区 RGB 平均差 1.79 /
> p99≈8 / max≤17），客户端为游戏原始剥壳字节——保真度只升不降。对照渲染件：
> `data/cache/local_compare/`（左 BK / 右客户端）。BlitzKit `fetch-icons` 与运行期懒下载
> （`web/assets.rs:233`）退为缺失兜底（休眠）。

**"换素材 ≠ 等价替换"的复核结论**：与 BlitzKit 封面是**同一幅画**（同源美术、同像素
尺度）——1:1 无缩放、BK 画布左上角对齐逐辆复核：姿态/光照/细节一致、内容 bbox 逐像素
同位（如 `T-34` 双侧均为 (25,13)-(128,88)）；BK 只是画布更小（高以 100 为主，尾至 69；
客户端一律 256×128）。量化：**轮廓 IoU 中位 0.986**；灰度平均差中位 **1.76/255**
（216/735 < 1＝仅 webp 重编码噪声；32/735 > 5＝高频纹理＋亚像素重采样车，最差
`16241 Oth53_Rammer` 均差 29.8）。接线前抽查对照图
`data/cache/local_tank_icons/_vs_blitzkit.png`（12 辆＝均差最差 6＋最好 6）与
`data/cache/local_compare/same_art_*.png` 即可。

### 3.1 来源与形态

| 项 | 值 |
|---|---|
| 大图标 | `Data/Gfx/UI/BigTankIcons/<name>.packed.webp.dvpl`（2312 个，含 `@2x` 与 `_skinN`） |
| 小图标（兜底） | `Data/Gfx/UI/BattleScreenHUD/SmallTankIcons/<name>.packed.webp.dvpl`（1474 个） |
| 容器 | DVPL 壳里**就是裸 webp**（`RIFF…WEBP`）——**剥壳即得，不重编码** |
| 分辨率 | 基础 256×128 / `@2x` 512×256 / 小图 128×32 |

### 3.2 命名域：图标名是**第三套命名**，与模型名不规则对应

客户端没有任何权威清单（车辆 XML 与 `list.xml` 都不引用图标名），只能逆出规则。
实测样本：

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

实现（`tools/export_tank_icons.py`）：**权威声明源 + 归一化候选梯子（备选）**。主路径读
逐车 `bigIconPath` 声明、按**整文件名（含国家标签）**精确取图（见 §3.3）；仅当声明缺失才
走归一化梯子（候选 = 模型名 / 去 `XxNN_` 前缀 / 去 `Ch_` 类非数字前缀 / 去 `WH_` 中缀 /
**显示名** / 模型名＋显示名多出的词），两侧去国家标签 → 转小写 → 去非字母数字后比对；
皮肤变体在排序里被压到最后（不误用）。

### 3.3 覆盖与残差

**738/738，且全部为 `big` 档**（0 皮肤档 / 0 小图标；来源与档位记在
`_export_status.json` 的 `match_src`/`grade`）。**匹配的权威来源是客户端逐车声明**：
`3d/Tanks/Parameters/<nation>/<stem>.yaml.dvpl` 的 **`bigIconPath` / `smallIconPath`**
（`~res:/Gfx/UI/BigTankIcons/<名>`；764 份参数文件中 755 份为逐车文件、全含该字段）——**按整
文件名（含国家标签）精确解析**（`ussr-IS_2` 必须命中苏联图标，不能归一化后撞上同键的
`china-IS2`），**738/738 全部由声明源命中**（唯一特例 `KV-1s-BP`：声明名带资源变体后缀
`ussr-KV_1s_BP.china`、磁盘无此名，按去尾后缀再精确一次命中 `ussr-KV_1s_BP`）。
备选来源（仅在声明缺失时使用，按序）：`camouflages.yaml` 注册表 → list.xml 短名/全名 →
模型名及去前缀变体 → stem＋显示名词 → 人工别名表；弱来源加**唯一性护栏**（`T34_hvy` 的
全名"T34"、`Chi_Ha` 的短名都会撞他车，命中被弃）。

**接入声明源后纠出的 3 处跨车误挂**（此前版本曾发生，现分别为
`ussr-T-34` / `ussr-T-28` / `ussr-ST-1`）：`T-34` 曾挂 `china-T34`、`T-28` 曾挂
`usa-T28`；`ST-I`（10753）曾误挂 `ussr-R63_ST_IBD`（实为 `T-2020` 20993 之档）。

**另修的三处提取缺陷**：① 索引侧对称归一化——候选侧会剥 `XxNN_` 前缀与皮肤后缀，
索引侧却不剥，导致"同车不同皮肤/带内部编号"的图标永远匹配不上（`IS-4` 只有 `_skin`
大图、`Indien_Panzer` 只有 `G88_Indien_Panzer_skin`）；② 小图标命中会压掉更好的候选
（`112 Glacial` 的 128×32 条带压掉了 `china-112_event` 大图）；③ 补 3 条别名
（`F68_..._Chasseur_de_char_46`→`france-CDC`、`Ch23_112`→`china-112_event`、
`PzVI_GuP`→`japan-Tiger_I_GuP`）。**刻意不启用模糊匹配**：实验里模糊匹配会落到**别的车**
（`PzV_PzIV`→`PzIV`、`GB24_Centurion_Mk3`→`Oth41_Centurion_Mk3_S2`），宁可留空也不串车；
派生键另加"剥编号须留 ≥2 词"约束（`T34_hvy`→`hvy` 会误撞 `T1_hvy`）与撞键同车校验。

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
  逐通道实测显示复刻没有意义：对方的 `normal` 取的是**旧法线图**
  `images/<T>_NM`（DXT1）、`metallicRoughness` 的 G 虽与 `baseRMMap` ch0 一致但 **B（金属度位）
  对不上客户端任一张贴图的任一通道**，照抄等于把已知不成立的 PBR 语义写进产物；而 `occlusion`
  两边**本已相同**（都取 `miscMap.R`，1011/1011 逐像素一致）。见 [local-model-export.md](local-model-export.md) §4。
- **`is_collector` 三分显示**：已解决——`tank_cache.json` 含 `is_collector`（与 `is_premium`
  同为 `tanks.pb` field13 枚举：1=金币、2=收藏，互斥），运行期与消费方均已消费
  （[data-inventory.md](data-inventory.md) §1a）。
- **报告 B 的 35 辆 GLB 缺口**：已由 `LodComponent` 判据 + 全批次导出等规则收敛到
  623/735 逐字节（[local-model-export.md](local-model-export.md) §0/§3，与该文及本文件
  §1 同一口径）；其中 **111 辆为有意分歧**（静态变换烘焙 / 状态过滤，见该文 §5.1；
  本条此前残留旧语料的 733/23 数字，2026-10-31 勘误）。
- **UV1/UV2、`doubleSided`、MR 通道摆放**：均已对齐（§1）。

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

# 封面图：自产 + 展示裁剪框（默认只用大图标；--allow-small 退小图标）
python tools/export_tank_icons.py --all --allow-small
python tools/export_tank_icons.py --emit-crops

# 车辆数值：本机解包 → 同格式 pb + 对照
python tools/extract_vehicles.py --all
python tools/emit_vehicle_pb.py --emit-supplement && python tools/emit_vehicle_pb.py
python tools/compare_vehicle_data.py

# 诊断（tmp_glb_reverify/，不入库）
python tmp_glb_reverify/full_diff_inventory.py    # 逐维度差异清单
python tmp_glb_reverify/diff_sheets.py            # 对外沟通对照图（英文标签）
python tmp_glb_reverify/en_yaml_missing.py        # C2 显示名缺失复核
```
