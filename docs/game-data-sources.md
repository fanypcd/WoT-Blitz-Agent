# 游戏数据来源与 BlitzKit 依赖评估

> 2026-10-01 整理。回答两个问题：**每一份数据到底从哪来**，以及**能否摆脱对 BlitzKit
> 数据源的依赖、改由解析本机游戏文件自产**。文末附本轮 game_data 冻结故障的复盘与
> 资产面（COS）发布流程。所有结论均有实测证据，证据出处随文标注。

## 一、结论

**数据内容可以本地自产，但 `tank_id` ↔ 游戏模型名的映射桥游戏本地文件给不出。**
把这一项单独固化之后，BlitzKit 就能从"每次刷新都要拉的数据源"降级为"一次性桥接表
生成器"，运行期与提取链都不再需要它。

- 已实测可由游戏文件替代：`tanks.pb` 全部内容、`models.pb` 全部内容、GLB 几何（需补
  一个转换器）、坦克封面图（此前误以为只能取自 CDN）
- 唯一缺口：`tank_id` 是 WG 图鉴分配的编号，回放里用的就是它，但游戏本地**任何文本
  资源都不含这张表**（搜索范围见 §三）
- 该缺口有两条替代路径（WG API + 字符串反查 ≈ 90% 覆盖；或一次性固化映射表），建议
  以"固化映射表为主、模糊匹配为辅"

---

## 二、数据源清单

### 2.1 当前四类来源

| 产物 | 当前来源 | 本地可否替代 | 证据 |
|---|---|---|---|
| `data/tanks.pb` 坦克数据库 | BlitzKit `definitions/tanks.pb` | ✅ 完全 | §2.2 |
| `data/models.pb` 模型定义 | BlitzKit `definitions/models.pb` | ✅ 完全 | §2.3 |
| `data/cache/models/{id}/*.glb` | **本机客户端**（`tools/export_tank_glb.py` 自产；2026-10-07 已换源并接线，BlitzKit CDN 仅缺失兜底） | ✅ 已实现并接线 | §2.4 |
| `data/cache/tank_images/{id}.webp` | BlitzKit `tanks/{id}/icons/big.webp` | ✅ 完全 | §2.5 |
| `data/game_data/{id}.json` 装甲/碰撞盒 | 本机客户端 DVPL（已本地） | — | §2.6 |
| `data/tank_cache.json` | `tanks.pb` 派生（纯本地，**不经 WG API**） | ✅ 随 pb | — |
| 地图资产（底图/地形/小地图/场景） | 本机客户端（已本地） | — | `tools/export_map_glb.py` |
| 玩家战绩 | WG API | 不可（本来就不是本地数据） | — |

`fetch-tanks` 的实现是 `TankResolver::from_blitzkit()` 后落盘，`api_client.rs` 明确
"坦克百科数据已全部来自 BlitzKit，不走 WG 百科"——WG API 只服务战绩查询。

### 2.2 tanks.pb 的内容 ← 游戏 XML

来源是 `Data/XML/item_defs.zip`（3MB、892 条目）内：

- 车辆定义 `vehicles/{nation}/{model_name}.xml.dvpl`（约 790 个）
- 共享模块 `vehicles/{nation}/components/{guns,shells,turrets,chassis,engines,radios,fuelTanks}.xml.dvpl`（72 个）
- tier：`Data/Configs/TechTree/{nation}_tree.yaml.dvpl` 的 `position[0]`
- 名称（**两个字符串源**，2026-10-10 勘误）：
  1. 随包基础快照 `Data/Strings/<lang>.yaml.dvpl` 的 `#<nation>_vehicles:<KEY>`——**必须按
     元素自带的完整前缀键字面查**（`gb_vehicles` 450 键与 `uk_vehicles` 478 键并存、逐条
     不同；裸键回退会跨系相撞），值里的 YAML 转义（`ä`→ä、` `）需解码、值捕获须
     转义感知（`"` 正则会 被 `\"` 截断），A/B 尾字母重复条目的名字可回退其基础键；
  2. **运行时本地化覆盖层**（此前漏掉、2026-10-10 发现）：客户端从 CDN 下载并缓存到
     `%LOCALAPPDATA%/wotblitz/DAVAProject/cache/`——共两处、用途不同（全盘 token 扫描
     31300 个文件定界）：
     * `localizations/<lang>.yaml`（**车辆/模块名**所在，带 `.etag` HTTP 缓存；
       `Data/server_config_urls.yaml` 指向 conf CDN）；
     * `dynamicContentLocalizations/<lang>.yaml`（**活动/offer 文案**，键形如
       `2025-tank-12/Title`；实测**不含**车辆/模块名——名字面剩余的 2 条枪/履带名在
       两处都没有）。**随包 strings 不含上线后新增的
     活动/BP 车与模块名**——BP 车在 `camouflages.yaml` 注册、名字键（`<stem>_Custom[_short]`）
     只在覆盖层里——这就是"游戏能正常显示名字、而只读 `Data/` 的解包查不到"的原因。
     覆盖层按客户端语言缓存（实测机为 zh-Hans），但**专名跨语言同形**（Turbo/Magnate/
     Panlong 等 24/24 与 BlitzKit 英文名逐字一致）；提取器把它作**缺失键回退、只接受拉丁值**。
     依赖注记：per-user 运行时缓存（需客户端登录同步过；新装/未同步时会缺）

**定位陷阱**：共享弹种定义不在 `vehicles/` 顶层，按文件名搜 `*shell*` 会一无所获；
正确做法是解码整个 zip 后全文检索弹种名，命中 `components/shells.xml`。

**逐字段实测（T-34 样例，与 BlitzKit `tank_cache.json` 完全一致）**：

| BlitzKit 字段 | 游戏来源 |
|---|---|
| `penetration` / `penetration_far` | `guns.xml` 的 `<shots><X><piercingPower>85 72</piercingPower>`——一个标签两个值，近/远 |
| `damage` / `module_damage` | `shells.xml` 的 `<damage><armor>140</armor><devices>105</devices></damage>` |
| `shell_type`（`ap`/`hc_premium`/…，field7） | 同 shell 的 `<icon>`（9 值显示令牌） |
| 弹种权威枚举（field9） | 同 shell 的 `<kind>` → 0=AP/1=APCR/2=HEAT/3=HE（4 值闭集；与 icon 交叉表零矛盾，`atgm_heat`→HOLLOW_CHARGE 即 field9 修复的坑） |
| `velocity` / `range` | `guns.xml` 的 `<speed>` / `<maxDistance>` |
| `caliber` / `normalization` / `ricochet` / `explosion_radius` | `shells.xml` 的 `<caliber>` / `<normalizationAngle>` / `<ricochetAngle>` / `<explosionRadius>` |
| 装填/瞄准/散布/弹鼓/连发 | gun 级 `<reloadTime>` `<aimingTime>` `<shotDispersionRadius>` `<clip>` `<burst>` |
| `hp` | 车体 `<maxHealth>` + 炮塔 `<maxHealth>` |
| 机动 | `<speedLimits>`、chassis 的 `<rotationSpeed>` |
| shell `id` | `shells.xml` 的 `<id>`，经 `loadout::blitzkit_shell_global_id` 换算到回放/BlitzKit 的全局弹种域 |

**弹药列表的权威来源是共享炮定义**（2026-10-09 核订）：`vehicles/{nation}/components/guns.xml`
的 `<shots><shell_tag><piercingPower>` 带弹道与穿深；车辆 XML 内联 `<shots>` 是价格/可用性叠加
（`<shell>shared<price>…`），个别车引用的弹种族与共享定义不同（`J24_Type_57`：内联 base 族在
客户端无任何弹道数据 → 穿深 0；共享 A 族 = 218/260/65 与 BlitzKit 一致）。解析规则：按共享
`<shots>` 列表迭代，内联同标签条目合并覆盖其余字段。

**其它名称/参数相关文件**（扫描所见，暂未接入）：`hangarBaseParamsPrecomputedData_generated.yaml`
（车库预计算表，含各车参数与 stem）、`DAVAProject/cef_data/cache/`（内嵌浏览器新闻缓存，噪声）。

### 2.3 models.pb 的内容 ← 游戏 XML + 几何

| BlitzKit 字段 | 游戏来源 |
|---|---|
| 逐板厚度 `plates` | 车辆 XML `<armor><armor_N>VALUE` |
| 间隙甲 `spaced` | `<armor_N>` 内含 `<vehicleDamageFactor>` 子节点（规则见 §四） |
| 主装甲 `primary` | `<primaryArmor>armor_1 armor_3 armor_12`（位置序 front/sides/rear） |
| 俯仰极限 `pitch` | gun 的 `<pitchLimits>-20 10</pitchLimits>` + `<extraPitchLimits><back>-20 3 27</back>` |
| 射界 `yaw` | turret 的 `<yawLimits>-180 180` |
| 炮塔/主炮→模型节点号 | 碰撞 `.sc2` 的节点名（`hull` / `turret_01` / `gun_01`） |
| 碰撞盒 / 原点 | 碰撞模型几何（需 DAVA 解析） |
| 炮塔初始姿态 `initial_turret_rotation` | 车辆 XML `<turretInitialRotation><yaw>/<pitch>/<roll>`（度）——2026-10-10 实测与 models.pb field3 逐值一致（4 辆意系 SPG 非零；.sc2 的 TransformComponent 全是单位阵，不是来源） |
| 履带厚度 | chassis 段 |

**枪序口径（2026-10-09 核订）**：顶塔主炮列表按 **XML 文档序 = 游戏研发序**（末位 = 顶级
主炮）导出；BlitzKit 的 models.pb 是**模块 id 升序**（与研发序在 105 辆上序不同——91 辆末位
主炮不同，其中 15 辆末位主炮板集实际不同）。客户端 XML 每个 gun/turret 条目带 `<level>` 研发
等级与 `<unlocks>` 研发图，可作为交叉校验。

**空心碰撞盒口径（2026-10-09 核订）**：43 辆无炮塔 TD（AT-SPG，固定战斗室车，如 StuG III G /
SU-85 / Hetzer / JPanther / Tortoise / T110E3）的**炮塔**碰撞盒两侧都只有空占位——客户端参数
YAML 无 `turret_NN` 段（实测 SU-85：`turret_` / `gun_` 0 处），item_defs XML 的炮塔 bbox 为全 0，
BlitzKit models.pb 也只有 **4 字节空消息**（`0a 00 12 00`，min/max 两个空 Vec3）。因此判据统一
为：**min/max 双双为空消息 = 无数据**，不是"零盒"。Rust 严格解析（`parse_vec3` 空 → `None`）
把它落成 `null`（包内这 43 辆 `collision_boxes.turret` = null，其余 chassis/hull/gun 三盒
735/735 齐备）；Python 对照器与 `extract_vehicles.py` 为对齐 pb 序列化而保留全 0
（该文件注明"全 0 bbox 不省略——实测 43 个炮塔"）。⚠️ 换源/对照时不得把空心读成实值。
`gun_bbox` 不在此列：735/735 有值，历史上 154 辆为空的缺口是 2026-09-30 field32 冻结故障
所致、已修复（见 §六）。

### 2.4 GLB 几何 ← 客户端模型文件

**权威路径不在目录约定里，而在逐车的参数 YAML**：`Data/3d/Tanks/Parameters/<小写国名>/<模型名>.yaml.dvpl`
的 `resourcesPath.blitzModelPath`（碰撞走 `resourcesPath.collisionMesh`）。目录约定只是**回退**：

| 资产 | 权威来源 | 回退（按约定） |
|---|---|---|
| 视觉模型 | `resourcesPath.blitzModelPath` 指向的目录 | `Data/3d/Tanks/<大写国名>/<模型名>.sc2.dvpl` + `.scg.dvpl` |
| 碰撞模型 | `resourcesPath.collisionMesh` | `Data/3d/Tanks/CollisionMeshes/<小写国名>-<模型名>.sc2.dvpl` + `.scg.dvpl` |

**两套国名互不相同，按目录去猜必错**（2026-10-03 实测踩坑）：

- `XML/item_defs/vehicles/` 与 `CollisionMeshes/` 用 **tanks.pb 的小写国名**（`germany`/`uk`/`ussr`…）
- `3d/Tanks/` 下的车辆目录是**另一套大写名**，且不是简单大写化——映射表在
  `tools/export_tank_glb.py` 的 `NATION_DIR`：
  `{germany→German, ussr→USSR, usa→USA, uk→GB, china→China, japan→Japan, france→France, european→European, other→Other}`
- 即便国名对了，`3d/Tanks/<国>/<模型名>.sc2.dvpl` 也**不普遍成立**（如 PzIV 的模型在
  `German/G79_Pz_IV_AusfGH/...`），所以必须走上述权威路径，别用约定拼。

**这是唯一需要新写代码的一块。** 都是 DAVA 格式，而 `tools/wotbtools/`
（`wotb_sc2.py` 解 KeyedArchive 实体树、`wotb_scg.py` 解 PolygonGroup 顶点/索引流）与
`export_map_glb.py` 的 `GlbBuilder` 已具备全部原语——地图资产就是这么导出的，坦克是
同一套格式，属移植现成管线而非从零。

### 2.5 封面图 ← 客户端本地（本轮新发现）

- `Data/Gfx/UI/BigTankIcons/<国家标签>-<名字>.packed.webp.dvpl`（2312 个，含 `@2x` 与 `_skinN` 变体）
- `Data/Gfx/UI/BattleScreenHUD/SmallTankIcons/`（1474 个，128×32，作兜底）

命名规则与资产面的 `tank_images/{id}.webp` 对应；`.packed.webp.dvpl` 的解包链项目已
跑通（`fetch-minimaps` 解的就是这一类）。

**命名比"标签-模型名"复杂**，`tools/export_tank_icons.py` 里已沉淀三层：

1. 国家标签有一处客户端错拼须一并接受：`britsh`（另有 `british`）；
2. 有些图标名**不带任何国家标签**（如 `Frankentank_event`）；
3. 有些用**内部代号/缩写**，单靠拼名字推不出（`ST_I` → `ussr-R63_ST_IBD`、
   `Pro_Ag_A` → `germany-Leopard_PT_A`、`M2_med` → `usa-M2_MT`）。

第 3 类走 `ICON_ALIAS` **人工核对别名表**（宁可缺失也不猜——错挂别车的图标比缺图更糟）；
别名表**优先**于启发式匹配（表内每条实测确认同车且为基础档）。2026-10-10 修复三处提取缺陷后
（① 索引侧对称归一化：剥 `_skinN` 与内部编号，此前候选侧剥、索引侧不剥，导致同车不同皮肤
后缀的图标永远匹配不上；② 小图标命中会压掉更好的大图/别名候选；③ 别名表缺 3 条），
覆盖率 **735/735 且全为大图档**（0 皮肤档 / 0 小图标）。**权威来源 = 客户端逐车声明**：
`3d/Tanks/Parameters/<nation>/<stem>.yaml.dvpl` 的 `bigIconPath`（764 份参数文件中 755 份为逐车文件、全含；按
**整文件名含国家标签精确解析**，735/735 全部由此命中——唯一特例 `KV-1s-BP` 的声明名带变体后缀 `.china`、按去尾后缀再精确一次）。此前"客户端无
T1_hvy / R71_IS_2B 图标"的结论均已撤销：声明分别为 `usa-T1Heavy`、`ussr-IS_2`。
对照诊断用 `tools/compare_tank_icons.py`。

### 2.6 客户端关键路径速查

| 用途 | 路径 |
|---|---|
| 车辆与模块定义（DVPL 压缩，容器为 zip） | `Data/XML/item_defs.zip` |
| 逐车渲染参数（贴图变换/后坐/悬挂/掩体平面） | `Data/3d/Tanks/Parameters/{nation}/{model_name}.yaml.dvpl` |
| 科技树与 tier | `Data/Configs/TechTree/{nation}_tree.yaml.dvpl` |
| 本地化显示名 | `Data/Strings/{lang}.yaml.dvpl` |
| 坦克图标 | `Data/Gfx/UI/BigTankIcons/`、`.../BattleScreenHUD/SmallTankIcons/` |
| 客户端版本号 | `Data/version.txt.dvpl`（`update-data` 的版本感知依据） |

逐车 yaml 的 `suspension:` 块（逐轮 12 B 数组 + 履带折线 + 弯曲/铺放系数，730/764 辆有）
已由 `tools/export_tank_suspension.py` 导出为包内 `suspension/<tank_id>.json`（additive，
725/735 辆）；字节结构、节点绑定契约与未定项见
[tank-suspension-client-re.md](tank-suspension-client-re.md)；接线状态见
[data-inventory.md](data-inventory.md) §四。

---

## 三、`tank_id` ↔ 游戏模型名的桥（**已可由客户端重建**，2026-10-10）

`tank_id` 在客户端**并非不可得**：`Data/XML/item_defs/vehicles/<nation>/list.xml` 每个车辆
条目的 `<id>` 是国家内局部 id，全局 id = `(局部 id << 8) | 国家基数`（与 guns/shells 同一
编码，国家基数见 `tools/extract_vehicles.py::NATION_LOW`）。全量实测：735/735 与 pb 导出的
桥表 stem/nation 逐条一致，另多出 **20 辆** BK 没有的车（教程 bot、超测/开发车）。工具入口：
`python tools/extract_vehicles.py --emit-bridge --from-client`。`slug`（BK 专有）无消费者，
可留空——文件定位一律用 `stem`（游戏模型名）。

（历史记录：本节原述"游戏本地任何文本资源都不含此表"，经 2026-10-10 复核为误判。）

### 3.1 为什么绕不开

`tank_id` 是 WG 图鉴分配的编号（T-34 = 1、M4A3E2 = 10017），回放 `battle_results` /
`ARENA_INFO` 里用的就是它，所以这个 id 域无法回避。提取时要定位
`vehicles/{nation}/{model_name}.xml.dvpl`，就必须把 `tank_id` 换成 `model_name`。

### 3.2 搜索范围与否定结论

以下位置**均只有模型名/显示名，没有任何数字 id**：

- `item_defs.zip` 全部 892 条目（含 vehicles 与 components 全文）
- `Configs/TechTree/*`、`Strings/en.yaml`、`camouflages.yaml`、
  `battlePreloadResources.yaml`、`hangarBaseParamsPrecomputedData_generated.yaml`

另已排除：`tank_id` 不是模型名的哈希（fnv1a / djb2 / crc32 量级均为 10⁹~10¹⁰，实际 id
为 1~2 万量级）。项目依赖的 `wotbreplay-parser` 也不带车辆表，只有 `map_id` /
`room_type` 两个枚举。

**2026-10-04 全量复扫（彻底闭合）**：对 Data 下 **31,516 个文本容器**（含此前未扫的
`UI/`、`Stories/`、`MessageFilters/`、`Materials/`、顶层 60 个 dvpl）解码后按
「已知 tank_id 字面量 ↔ 模型名共现」搜索，**0 命中**；`tank_id`/`tankopedia` 字面量
0 命中。唯一的关联字面量是 16 处 UI 绑定文件（Inventory/Profile/BattleResults 等）
里的 **`vehicleId`**——它绑定的是运行期视图模型（`tankProgress.tankDescr` 等），
即服务器车库数据的 UI 投影，不是本地表。两条来源侧证据：

- **`wotblitz.exe` 的协议字段名表**里 `tank_id` 与 `password`/`users`/
  `vehicle_serialized_type` 并列——`tank_id` 是**车库/登录协议中服务器下发的字段**；
- 运行期缓存 `DAVAProject/cache/base_stuff_Asia.dat`（zlib 包裹的服务器 pickle，
  **按区命名**）只含物品/活动定义与车辆名引用，无任何 tank_id。

结论：`tank_id` 由 WG 服务器在车库协议中下发，客户端在**运行期**才知道；客户端静态
文件确实不含这张表。"游戏内能显示名称"走的是 en.yaml 的模型名域，与 tank_id 无关。

### 3.3 BlitzKit 当前如何提供这张表

`tanks.pb` 每条坦克的主消息内，**field 1 = tank_id、field 32 = 游戏模型名**并列。
注意 field 2 = `dev_name` 是 slug（`m4a3e2`），**不能用于文件定位**——2026-09 前后
BlitzKit 把游戏模型名从 field 2 挪到了 field 32，这正是 §七 故障的根因。

### 3.4 替代路线

**路线 A：WG API 图鉴 + 游戏字符串反查。**
`tank_id`（WG API `/encyclopedia/tanks/`）→ 显示名 + 国家 → 在 `Strings/en.yaml` 的
9620 条 `#<nation>_vehicles:<KEY>` 中反查 → 模型名。

实测覆盖率：显示名精确命中 **65.4%**；再加"去 `_short`/`_descr` 后缀"规则到 **89.9%**；
仍剩 74 辆（10.1%）查不到，集中在英系（`Matilda` ↔ `GB07_Matilda`、`Cromwell` ↔
`GB21_Cromwell` 这类显示名与游戏串写法不一致的车，加前缀归一化可再捞一批但补不干净）。

代价：引入 WG API 这个网络依赖、覆盖率不足 100%、需要人工兜底表。

**路线 B：固化 `tank_id → model_name` 映射表（推荐）。**
用 BlitzKit 一次性生成 735 条映射（几 KB JSON）入库，之后提取链只读游戏文件。
- BlitzKit 从"数据源"变成"一次性桥接表生成器"
- 顺带消除 §七 那类故障：映射写在仓库里，BlitzKit 再改字段约定也影响不到提取链
- 游戏更新加车时用路线 A 的模糊匹配补新条目 + 人工复核未命中项（通常个位数）

**路线 C：维持现状。** 代价是每次 `update-data` 都受制于 BlitzKit 的稳定性与可达性，
且它一改约定提取链会**静默失效**（§七 就静默了 25 天）。

### 3.5 建议

B 为主、A 为辅：映射表为唯一事实源，新车用 WG API + 名称模糊匹配自动补，未命中的人工
复核。副产品是 BlitzKit 反而更有价值——把它当**交叉校验对象**，定期用本地提取结果与
其 pb 做 diff，一旦漂移即说明游戏更新了或我们的解析有问题。本轮正是靠这种对照才发现
间隙甲规则其实完全一致（先前误判为"BlitzKit 用了更宽的规则"，实为自身数据陈旧 + 零厚板
序列化差异，见 §四与 §七）。

**炮塔/主炮的配置档位已对齐（2026-10-03 修复）**：提取器原先取 XML 里**第一个**炮塔/主炮
（`<turrets0>` 的首个 `<armor>`，即初始炮塔），而 BlitzKit 取**顶级**配置
（`turrets.last()` × `guns.last()`）。这是当时残余板集差异的唯一来源，与 spaced 规则无关。

现已改为取顶级：`<turrets0>` 的条目**以模块名为标签**（如 `<T-34_mod_1942>`），不是固定的
`<turret>`，故按「最后一个 `<guns>` 之前、上一个 `</guns>` 之后」定位顶级炮塔自身字段，
顶级炮塔内最后一个炮管即顶级主炮（`dvpl.rs::top_turret_span`）。

修复后全量核对（735 辆，忽略 0 厚板与浮点精度）：`game_data` 与 `models.pb` 的板集差异
**turret 434 → 0、hull 381 → 0**（残留仅 19.05↔19.0 这类 float32 量化）。同一次修复也让
六面装甲摘要不再低报——改前 210 辆车的摘要与 `armor_model` 自相矛盾，其中 T29 炮塔正面
低报 254mm（25 vs 279）。

**同一档位口径还漏了三处（2026-10-03 一并修）**：`tank_resolver` 的**弹种**取自
`turrets.first().guns.first()`（初始炮）、**视野/炮塔转速**取自 `first_turret`。于是
`tank_cache.json`（列表卡片与「穿深」排序的数据源）显示的是**初始模块**，而详情页
（`configs[]` 按炮逐项展开、默认选中末项）显示**顶级模块**——同一页面两套数字。
实测 T-34：列表 穿深 85 / 视野 200 / 转速 40 → 顶级 **125 / 240 / 49**。

⚠️ **这条链有三处必须同档改**，只改一处会让它们互相矛盾：

| 位置 | 作用 |
|---|---|
| `tank_resolver.rs` | `tank_cache.json`（→ 列表卡片、`pen_max`、穿深排序、`author_shells`） |
| `web/assets.rs::shells_handler` | `/api/shells/{tank_id}`（装甲查看器的弹种选择器） |
| `agent/tools.rs`（热力图工具） | 传给查看器的 `shell` 索引，须与上一行同源 |

改完 grep `turrets\.first|guns\.first` 应为空（测试夹具除外）——本轮就是这样发现另两处的。

**同档之外还有"同字段"要求（2026-10-10）**：`tank_cache.json` / 包内 `tank/{id}.json` 顶层
`shells[]` 是摘要投影（列表卡片与**装甲查看器"切换射击方"路径的默认弹表**都在读它），
**必需携带等效厚度判定输入** `caliber` / `normalization` / `ricochet`（HE 的转正/跳弹数据
本身即 0，属真值；缺项时消费方按 0° 兜底 = 判定被静默削弱）。三条投影链必须同字段：
`tank_resolver.rs` 摘要（→ `tank_cache.json` / `/api/tank`）、
`tank_configs.rs::tank_data_value_prefixed`（包内顶层弹表）、
`web/assets.rs::shells_handler`（`/api/shells`）；`configs[].shells` 一直是全字段基线。
报障形态：装甲查看器同一像素"同车检视（走 `configs[]`）"可击穿、"重选同一射击方（走顶层）"
挡弹——界面无任何可见变化（弹表下拉只显示弹种/穿深/伤害，两表同值）。Strv K AP 转正
5° → 0° 即翻（两倍口径规则下转正角还会放大 `1.4×norm×caliber/(2t)`）。回归锚点：单测
`shell_summary_keeps_penetration_decision_inputs`、
`top_level_shells_carry_decision_inputs_matching_top_gun_config`。

**车级 `caliber` 同批归位（2026-10-10）**：`tank/{id}.json` 顶层 `caliber` 原取
`configs.first()`（**初始炮**口径）、取不到按炮名解析回退 120——与"顶层弹表"不同档；
跨车选射手时消费方把它当弹表口径用。现改为**顶级炮弹表自带弹径**（弹径是判定规则的权威
输入；炮名解析对 189/735 辆与弹径不一致、多为 120 回退）。326 辆的车级口径值随修正变更
（`S35 CA` 120→90、`O-I` 100→135、`E 50` 88→105…）。`configs[].caliber` 仍是炮名解析
近似（显示口径），未动。

---

## 四、间隙甲（spaced）判定规则（权威）

实测确认，与 BlitzKit 在板集一致处 **100% 吻合**（hull 729/729、turret 542/542、
gun 600/600）。规则共三条：

1. `plates[N]` = `<armor_N>` 的数值，**省略 0 厚板**——对齐 BlitzKit 的 pb 序列化器省零值语义
2. `spaced` = { N | `<armor_N>` 元素内含 `<vehicleDamageFactor>` 子节点 }，**零厚板保留**
3. `primary` = `<primaryArmor>` 的板列表，按位置映射到 front / sides / rear

**第 2 条的零厚板细节容易踩**：`plates` 是 map，零值被 pb 序列化器省略；`spaced` 是 id
列表，没有零值可省，所以同一块零厚板会"不在 plates 里、却在 spaced 里"。典型样例是
BT-2（1025）：`<armor_8>0<vehicleDamageFactor>0.0</vehicleDamageFactor>`，BlitzKit 的
`plates` 无 8、`spaced` 有 8。若在比对时把零厚板从 spaced 一并排除，会得出"BlitzKit
规则更宽"的错误结论。

---

## 五、运维：提取链与资产面发布

### 5.1 提取链的增量陷阱

- `extract-game` / `update-data` **默认只补缺失文件**（`out_path.exists() && !force` 即跳过），
  已存在的失效文件永远不会自愈——因外部约定变更导致的失效必须靠 `extract-game --force` 全量重跑
- `data_version.json` 只记一个 `game_data_updated_at`，从外部看像全量刷新过，实为混合版本
- 判定数据是否陈旧的**可靠指纹**：文件最后被哪个提交写过（`git log --name-only -- data/game_data/`），
  或批量统计 `gun.spaced` 非空率。注意 `hull_position` 为零值属合法，**不能当陈旧标志**

### 5.2 资产面（COS）增量发布流程

桶：`wotbtools-assets-1478073677`（ap-shanghai），布局与 `release/asset_pack/` 逐项对应，
根 `manifest.json` 含全量 sha256（**5000 条**，2026-10-10 按包内容重算）。此前"36 张 `map/*/ground.webp`
的条目记**合成前**哈希"的偏差已闭：打包器新增 `--refresh-manifest`（不重打包、按包内容重算
全部条目），发布链固定为**合成之后、上传之前**跑一次
`python scripts/export_asset_pack.py --refresh-manifest`；朝向契约、重烘与读回校验见 §5.5。
**首选工具**：
[tools/upload_asset_pack_cos.py](../tools/upload_asset_pack_cos.py)（差分比对 + 并发上传 +
`manifest.json` 强传 + Cache-Control 策略；`--only <prefix>` = **局部发布**——只传匹配前缀的
文件，且 manifest 改为**拉远端清单就地补丁**、只登记本次上传的条目，多会话共用一个包目录时
不会把他人在飞的改动宣布为线上内容。2026-10-10 坦克件批次即用此模式：`--only glb/` 上传
294 件 / 583.8 MB，桶内 manifest 4167 条中只更新了这 294 条，回拉 1470/1470 glb 一致）；
下列要点同时是手工流程的检查单：

1. **先做一致性安全检查**：比对桶内 `manifest.json` 与本地旧包快照的 sha256，并抽查若干
   对象核对清单哈希——确认桶处于预期状态，避免覆盖他人改动
2. **按 manifest 差分上传**，只传真正变化的：`新增 + 内容变更` 的对象，外加 `manifest.json`
   本身（2026-10-02 那轮 479 个对象 / 2.58 MB；2026-10-07 全量重导那轮 72 个对象 / 751.6 MB，
   其余 4096 个未触碰）
3. **跳过判定 = 桶内 `manifest.json` 的逐文件 sha256**（2026-10-10 起；旧实现只比
   `Content-Length`，把"内容变了但字节数恰好相同"的对象静默跳过——2026-10-07 的
   `map/lagoon/ground.webp` 即此例，当时须用 SDK `put_object` 强制覆盖；2026-10-10 的坦克
   顶点烘焙同属此类：只改顶点浮点、不改字节数）。远端 manifest 缺该条目（首传/旧对象）时
   回退尺寸比对；取不到远端 manifest 则整体回退并告警——此时第 5 步的逐对象回拉校验是唯一兜底
4. ⚠️ **上传会遍历包目录下全部文件**：俯视烘焙的渲染中间产物 `<pack>/overhead/*.rgba`
   （按图 64 MB，非包内资产）必须提前移出，约定缓存在 `release/overhead-bake/`；2026-10-07
   曾误传 73 个对象 / ~2.4 GB（已清理）
5. **上传后回拉校验**：逐对象下载与**本地包文件**比对 sha256（manifest 条目现与包内容自洽，
   可当基准；仍建议抽样回拉以防传输侧意外）
6. **不设 Content-Type**：桶内既有对象均未设置，保持一致以免引入元数据漂移
7. `manifest.json` 的 `generated` 是本次更新时刻，可能与 `index.json`（地图资产未变）不同步
   （2026-10-09 曾出现：COS 的 `index.json` 溯源戳停在 10-07，与本地同尺寸不同内容被旧第 3 条
   判据跳过）；若需两者一致须重跑 `export_asset_pack.py` 全量重打包（或按第 3 条新判据重传
   `index.json`）。
   另：`worktree_dirty=true` 的包**不等于**标注 commit 的原样工作区（见
   [data-inventory.md](data-inventory.md) §2.1）
8. 回滚素材：旧包状态的本地快照（`game_data/` + `tank/` + `manifest.json`），必要时原样传回
9. ⚠️ **凭据字节要干净**：Windows 下经管道/命令替换传 `COS_SECRET_*` 时注意尾部 `\r`
   （python 文本模式的换行转换会把 `\r` 带进 SecretKey ⇒ 比对与上传**全部** `SignatureDoesNotMatch`；
   2026-10-09 曾空跑一次——上传阶段全失败即先核对凭据长度是否与源文件一致，多 1 字节即此坑）
10. ⚠️ **发布后"客户端仍显示旧图"先查缓存窗口**：二进制资产带 `Cache-Control: max-age=3600`
   （有意折中，见 `tools/upload_asset_pack_cos.py` 注释）⇒ 浏览器在新鲜期内**不回源**，改图后
   最多约 1 小时才可见。诊断三步：①带 `?v=` 查询串另开一页对照（换缓存键 ⇒ 必为服务端现内容）；
   ②DevTools → Network 看 `ground.webp` 的 `(disk cache)` 与 `Content-Length`/`Last-Modified`；
   ③需要精确判定就 `fetch(url, {cache:'no-store'})` 算 sha256 与本地件比对。App 端不同：
   Native 代理（`AgentAssetProxy.kt`）每次请求都带 `If-None-Match` 条件重取 ⇒ 换内容即时生效。

### 5.2b 本机解包 → pb 编码器（**未接线**，2026-10-09 试接后回退）

> ⚠️ **当前不生效**：`data/tanks.pb` / `data/models.pb` 是 BlitzKit 现役版本；本节记录的是
> **已备好但未接线**的换源能力与试接批的验证结果，供验证完成后复用。本地解包版本未完成
> 验证前，不得直接替换 `data/*.pb`（试接批的实际差异见下）。

工具链（在库，不影响现役数据）：

1. `python tools/extract_vehicles.py --all` → `data/cache/local_pb/{tanks,models}.json`
2. `python tools/emit_vehicle_pb.py --emit-supplement` → `data/local_pb_supplement.json`
   （客户端无对应概念/数据的字段集中登记：`dev_name` 735（BK slug）与**名字 2 条**
   ——1 枪 + 1 履带，两种字符串源都没有；`initial_turret_rotation` 已客户端化不再需要）
3. `python tools/emit_vehicle_pb.py` → 编码为同格式 pb（只写 `blitzkit.rs` 运行期读取的字段；
   弹鼓按 field1×N + field2/3；全 0 向量/空心盒不写）
4. 对照/回归：`python tools/compare_vehicle_data.py [--pb-dir data/cache/blitzkit_pb_snapshot]`
   （换源前 pb 快照在 `data/cache/blitzkit_pb_snapshot/`，随时可复现 BK↔客户端对照）

**试接批实测（2026-10-09，已回退）**：`tank_cache.json` 重建与换源前仅 1 行差异（`10625`
explosion_radius 0.0→0.1，客户端值）；`tank_data` 18 辆差异 = 15 辆顶塔末位主炮板集（研发序）
+ 2 辆 explosion_radius + 1 辆形状；`game_data` 零重导。待验证项：`7009`/`12929` 各 3 发弹种 id
常量偏移（+0x2000/+0x5100）**经查为 BK 侧按 gun module 塌缩 + 孤儿弹种族所致**——客户端数据
自洽（换共享炮 `<shots>` 为权威后 0 配对失败）；名字已由"随包 Strings ∪ 运行时覆盖层"两源
收敛到 2 条；`initial_turret_rotation` 已客户端化。**BK↔客户端剩余差异共 122 处**（119 枪序
＝研发序口径、1 explosion_radius＝BK 错值、2 名字＝补表兜底）。

### 5.3 DVPL 外壳的校验契约（fail-closed）

提取链的每条读盘入口都过 `dvpl.rs`：装甲 XML / 碰撞 YAML（`extract-game`）、
`version.txt.dvpl`（`update-data`）、小地图与 `maps.yaml` / `en.yaml`（`map_assets`）。

footer 布局为 `decoded_size(4) + encoded_size(4) + crc32(4) + type(4) + "DVPL"(4)`，其中
**crc32 覆盖存储载荷（压缩后字节），不是原始数据**。解码器核对三条不变量，任一不符即
返回 `Err`：编码长度 == 实际载荷长度、存储载荷 CRC32、解压长度 == `decoded_size`。

**2026-10-08 之前 Rust 侧三项全不校验**，属"Python 侧 fail-closed、Rust 侧 fail-open"的
标准分裂——同仓库的 `tools/wotbtools/wotb_sc2.py` 一直三项全查。其中两处把损坏伪装成成功：

- footer 的编码长度被直接当作切片边界，损坏时 **panic**（而非报错）
- 自实现的 LZ4 块解压在输入截断 / 回引偏移非法时 `break` 出循环后返回**零填充的 `Ok`**
  （实测 16/33 字节的合法 LZ4 块 → `Ok, len=4096, trailing_zero_bytes=2794`；空输入 → 全零 `Ok`）

**LZ4 零偏移**：真机语料存在 `offset = 0` 的匹配序列，这不是规范 LZ4，但客户端确实产出、
参考实现（lz4 C，即 Python `lz4.block`）也照样解开——回引位置就是当前写出位置、且该处尚未
写出、缓冲区初值为零，等价于**写出 match_len 个零字节**。故两侧都按零处理而非拒绝（改前的
Rust 旧实现恰好也给出零，行为未变）。全树 45016 个文件里仅 1 例：
`3d/Tanks/France/images_pbr/F114_Projet_4_1_skin_MISC.dx11.dds.dvpl`。

> **可达性已查清（2026-10-08）：当前不可达，属于潜在陷阱而非在线缺陷。**
> 该文件只被一个场景引用（`3d/Tanks/France/F114_Projet_4_1.sc2.dvpl`，2487 个坦克场景全扫），
> 且位于该材质的 **`configArchive_1` / `configName="Skin_01"`** 配置里——`MaterialLibrary::_flatten`
> 只取 `configName == "Default"`（`configArchive_0` 的 `miscMap` 指向另一个文件
> `F114_Projet_4_1_MISC.dx11.dds.dvpl`，它解码正常）。地图侧 85 个 `.sc2` 无引用。
> 佐证：`export_tank_glb.py --tank F114_Projet_4_1` 全程成功，改前改后 `model.glb` /
> `collision.glb` **逐字节相同**。两端已同时对齐该形态（Rust + Python 均按零处理），
> 故若将来导出器开始支持皮肤配置，也不会撞上 `Sc2ParseError`。

影响面止于解码器本身：**没有任何产物或消费方受影响**。2026-10-08 对齐后按真机语料复核：

- `extract-game --force` 全量 735 辆，改动前后 `data/game_data/*.json` **逐字节相同**——新校验
  在真实文件上不改变任何输出
- 全树探针：本机客户端 `Data/` 的 **45016 个 `.dvpl` / 20.3 GiB 全部解码成功、零拒绝**
  （压缩类型分布 `{0: 13538, 2: 31478}`，即全树只有"未压缩"与"LZ4"两种，无 zlib）
- 上面那个零偏移文件另与参考实现逐字节比对：**sha256 相同**（1398256 字节）

复跑同一探针：

```
WOTB_DVPL_PROBE=<Data 目录> cargo test dvpl_client_probe -- --ignored --nocapture
```

### 5.4 DLC 覆盖层（`packs/`）——读取优先级

客户端把 DLC 微更新写到 **`%LOCALAPPDATA%\wotblitz\packs`**，以**相同的相对路径覆盖**
游戏 `Data/` 下的同名文件（第三方 mod 工具文档明写 "its files override the base ones"）。
**微更新不落在 `Data/`**，所以只读 `Data/` 会拿到 DLC 应用前的旧版本。

2026-10-08 本机实测（packs 下 45 个 `.dvpl`）：

| 类别 | 数量 | 例 |
|---|---|---|
| 与 `Data/` 同名、内容不同 | **5** | `3d/Tanks/German/Ferdinand.sc2.dvpl`（10458 vs 7832 B）、同车 `.scg.dvpl`（**922191 vs 791385 B**）、`XML/item_defs/vehicles/common/camouflages.xml.dvpl`、`camouflages.yaml.dvpl`、`3d/Customization.yaml.dvpl` |
| `Data/` 里根本没有 | **40** | 全是 `G37_Ferdinand_skin` 皮肤资产：`Customization/Skin_G37_Ferdinand_*.sc2/.scg`、`German/images_pbr/G37_Ferdinand_skin_*.dds`、`CamouflageMasks/*.pvr`、`Animations/*.anim`、`Gfx/UI/BigTankIcons/germany-G37_Ferdinand_skin.packed.webp` 等 |

**约定：所有客户端资源读取都经统一解析器，packs 优先、缺失回退 `Data/`**：

- Rust：`src/wargaming/game_extract.rs` 的 `packs_dir()` / `resolve_client_path()`；已接入
  车辆 XML/YAML 定位、`version.txt.dvpl`、`maps.yaml` / `Strings/en.yaml`、地图 landscape 目录。
- Python：`tools/wotbtools/dlc_packs.py` 的 `packs_dir()` / `client_path()`；已接入
  `export_tank_glb.py` 的模型/碰撞/参数路径解析。回归测试 `tools/test_dlc_packs.py`。

注：本机被覆盖的文件里 Rust 提取链实际消费的是车辆 XML 一类（未被覆盖），故 Rust 侧改动
目前是**行为等价的健壮性铺路**；**真正受影响的是 Python 坦克 GLB 导出**——它读的
`3d/Tanks/<Nation>/<model>.sc2/.scg` 正在被覆盖之列（Ferdinand 差 13 万字节几何）。

非 Windows / 无 `packs` 目录（WSL、纯净检出）时自动退回只读 `Data/`，行为不变。

### 5.5 俯视合成（均衡档地面）的朝向契约与重烘（2026-10-09）

**契约：朝向是常量 `YX`（两轴各翻一次 = 180°），不搜索、不逐图特判。** 推导两侧都是固定
代码（与地图数据无关）：

- **渲染侧** `WotbTools/frontend/scripts/bake-ground-overhead.mjs`：`group.rotation` =
  qFrame `Ry(π)·Rx(−π/2)`，把游戏系 `(x,y,z)` 映到场景系 `(−x, z, y)`；正交相机
  `position(0,1000,0)` + `up(0,0,−1)` 俯视 ⇒ 屏幕右 = `+X_scene`、屏幕上 = `−Z_scene`，
  `readPixels` 再翻成图像行序 ⇒ **渲染图左列 = −X_scene、顶行 = −Z_scene**。
- **底图侧** `ground.webp` 契约（`tools/export_map_glb.py` 分层注释「前端采样
  `uv = (0.5−X/s, 0.5−Z/s)`」；`playbackScene` 的分层 ShaderMaterial 与均衡档
  `PlaneGeometry` + `TextureLoader`(flipY=true) 两路同式）⇒ **底图左列 = +X_scene、
  顶行 = +Z_scene（北）**，即「上=+z/北」。

两者相差 180° ⇒ 合成时对渲染图恒取 `YX`（`tools/composite_overhead.py` 的 `ORIENT`）。

**故障与根因（2026-10-09）**：旧版对 8 朝向做梯度相关取 argmax。在低信号图上该判决退化成
噪声 argmax——`himmelsdorf` 的 8 个候选全落在 |score| ≤ 0.12 的噪声带（winner `xY` margin
0.0124），把俯视层烘成了**上下镜像**；均衡档（mid，读这张图）因此显示镜像的建筑层，3D 档
（ultra，读 GLB + 分层地表，不读本图）不受影响。同一病灶当时距翻车仅 0.0054
（`erlenberg_old`，碰巧选中正确候选）。判据与证据：

- **反解重建**（按合成式 `out = base×(1−0.92a) + render×0.92a` 逐候选重建、与包内成品在覆盖区
  比对）：36 图中 35 张实际烘入 = `YX`（corr 0.993–0.997），仅 `himmelsdorf` = `xY`
  （corr 0.9966 / margin 0.64）；按 `xY` 重建的"旧件"与问题文件**逐字节相同**
  （sha256 `bac8d3bb…`）⇒ 诊断闭合。
- **独立判据**：客户端小地图（游戏内北朝上绘制，与底图共用同一平面/uv 契约）vs 渲染覆盖掩膜
  33/36 图选 `YX`（余 3 张信号弱到噪声水平）；小地图 vs 底图 33/36 选恒等（含 himmelsdorf，
  margin 0.0459 —— 底图本身没歪）；用户报障「港湾小镇（`port`）朝向正确」= `YX`，与推导互证。

**重烘（安全口径）**——重烘前**必须**让合成读**合成前底图**，否则会在已合成的图上二次合成：

```bash
python tools/composite_overhead.py --pack release/asset_pack --map himmelsdorf --write \
    --render-dir release/overhead-bake --base-dir data/cache/maps
```

`--render-dir` 直接读 `release/overhead-bake/` 缓存（不必把 64MB/图 的 `.rgba` 复制进包；
包内出现 `overhead/` 时上传工具会连它一起传，见 §5.2 第 4 条）。**重烘后读回校验**（应全为
`YX`、`ok=true`；负向对照：拿旧的镜像件跑同一命令会报 `applied=xY, ok=false`）：

```bash
python tools/composite_overhead.py --pack release/asset_pack --render-dir release/overhead-bake \
    --base-dir data/cache/maps --all --verify
```

**blast radius**：均衡档地面（`map/<key>/ground.webp`）单一资产面；2026-10-09 重烘仅
`himmelsdorf` 一个对象（`a142e94e…`，7080664 B），其余 35 张内容未变——**等价性复核**：修复
后的代码以 `--base-dir data/cache/maps` 重跑全量 36 图，产物与现盘**逐字节相同 36/36**
（回归 0）；`manifest.json` 对这 36 张记的仍是合成前哈希的语义**未变**（见 §5.2 文首例外）。

⚠️ **渲染缓存的年份与它限制的观感（2026-10-09 复核，勿当成"当前材质的样子"）**：
`release/overhead-bake/*.rgba` 是 **09:49 版**渲染——**早于当日"光照/水面对齐批"**
（烘焙光照图、逐图 IBL、环境反射、逐图太阳、水面 LOW 档，均在其后落地）。故俯视层里的
建筑是按**当时**的材质渲的（Lambert 时代观感），不是现役着色器；本批水面改动**不影响**
俯视层（`bake-ground-overhead.mjs` 的剔除表按名丢掉 `sky/dome/water/sea/river/lake/plane`
等节点，水面从不进俯视渲染），且地面烘焙与渲染缓存本批均未改动（`git diff` 无
`export_ground`/`bake_ground` 命中）——本批以**同一缓存** + 重导后的合成前底图重跑全量 36 图，
`--verify` 读回 **36/36 `orient=YX, ok=true`**（本次未留存上一版合成日志，故不主张"逐字节相同"）。
刷新俯视到现役观感需在 WotbTools 侧重跑
`node frontend/scripts/bake-ground-overhead.mjs --pack <pack> --all`（headless Chrome +
SwiftShader）**再**执行上面的合成命令——**本批未做**（属当日光照批的遗留，非本批引入）。

---

## 六、本次故障复盘（2026-09-30 ~ 10-01）

### 6.1 现象

`data/game_data/` 中 273 个文件（占 728 的 37.5%）停留在 2026-09-06 的旧逻辑产物：
`gun.spaced` 几乎全空（272/273），154 辆的 `gun_bbox` / `collision_boxes.gun` 为 `null`。
`extract-game --force` 报"源文件缺失"而**拒绝重写**这些车，尽管文件就在游戏目录里。

### 6.2 根因

BlitzKit 把游戏模型名从 `tanks.pb` **field 2 移到 field 32**，field 2 改为 slug
（`Sherman_Jumbo` → `m4a3e2`）。而提取器的文件定位一直用 field 2，于是
`vehicles/{nation}/{field2}.xml.dvpl` 对不上号，280/735 辆被计入 `missing_files` 跳过。

更隐蔽的一层：field 32 **并非未解析**，而是解析后被丢弃——`(32, 2)` 分支把它赋给
`tank.name`，随后又被本地化名覆盖，只剩"字段是否存在"这一个判据。数据一直在手里，
只是没用上。

### 6.3 修复

- `blitzkit.rs`：新增 `TankFullData.model_name` 单列保存 field 32（`serde(default)`，
  与旧序列化兼容）
- `game_extract.rs`：文件定位改用 `model_name`（缺失回退 `dev_name`，兼容旧版 pb）；
  写产物的 `dev_name` 仍取 field 2 slug，使原本正确的 455 个文件不产生无谓变更
- `resolve_vehicle_file` 的模糊匹配保留为兜底

### 6.4 结果

重提取 **735/735 成功、0 失败**（此前 455 成功 + 280 缺失）。`gun.spaced` 空值
312 → 65（余者为本身无间隙甲的合法空值）；补回 154 辆的炮管碰撞盒；新增 7 辆此前
完全无数据的车。资产面按 §5.2 同步（479 对象），上传后逐对象 sha256 回验通过。

### 6.5 教训

1. **派生数据在充当独立证据前必须先确认新鲜度**。本轮曾把陈旧的 `game_data` 当"客户端
   事实"去与 BlitzKit 对照，得出"BlitzKit 对炮管用了更宽规则"的错误结论并汇报了两次；
   实为数据陈旧 + 零厚板序列化差异。
2. **增量更新不会自愈**：外部约定变更导致的失效，只有 `--force` 全量重跑能修；
   `data_version.json` 的单时间戳会掩盖混合版本。
3. **第三方字段约定是隐性契约**：依赖 `tank_id` 之外还要吃它的字段布局，等于把上游的
   字段调整变成自身的静默故障——这正是 §3.4 路线 B 要固化映射表的理由。

---

## 七、许可

从游戏文件提取的数据（装甲数值、模型、图标）版权归 Wargaming，README 已有声明。
解除 BlitzKit 依赖只把来源从"第三方整理"换成"第一手提取"，**不改变权属**。

## 地形让位掩码（`cover.u16.bin`）的生成与分发

- **生成**：`python tools/bake_terrain_cover.py [--map KEY]… [--stats]`（约 4 s/图，36 图 2m36s）。
  输入 = 包内 `map/<key>/scenery.glb` + `terrain.u16.bin` + `terrain.json`（size/span/zmin/zmax）；
  缓存优先取 `data/cache/maps/<space>.glb`。烘焙期常量（非渲染期开关）：贴地带
  `[地形−0.15 m, 地形+0.5 m]`、压低上限 0.5 m、**几何间隙 0.1 m**（压到结构面上会共面闪烁）、收尾 20 texel。
- **产出**：包内 `map/<key>/cover.u16.bin`（512² u16 LE；`0` = 无覆盖，否则结构面高度量化 +1）
  + `terrain.json` 的 `cover` / `coverStats`；同时写 `data/cache/maps/<space>.cover.u16.bin`，
  供 `export_asset_pack.py` 随包复制（**重打包不会丢**：脚本按缓存名复制并在 `terrain.json` 声明）。
- **消费**：前端 `terrainCover.js` 把**渲染用**高度场夹到天花板之下；查询/放置（`sampleHeight`）
  必须继续用真值高度场（守卫锁）。缺掩码/旧包 ⇒ fail-open（按无掩码渲染）。
- **发布**：与其它包内资产同链路——本地重建包后 `COS_SECRET_ID=… COS_SECRET_KEY=… python
  tools/upload_asset_pack_cos.py --local release/asset_pack`（可 `--only map/<key>/` 逐图发）。
  掩码是**渲染资产**，不进回放数据面（不改 replay 契约）。
