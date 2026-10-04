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
| `data/cache/models/{id}/*.glb` | BlitzKit `tanks/{id}/{model,collision}.glb` | ✅ 需写转换器 | §2.4 |
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
- 名称：`Data/Strings/en.yaml.dvpl` 的 `#<nation>_vehicles:<KEY>`

**定位陷阱**：共享弹种定义不在 `vehicles/` 顶层，按文件名搜 `*shell*` 会一无所获；
正确做法是解码整个 zip 后全文检索弹种名，命中 `components/shells.xml`。

**逐字段实测（T-34 样例，与 BlitzKit `tank_cache.json` 完全一致）**：

| BlitzKit 字段 | 游戏来源 |
|---|---|
| `penetration` / `penetration_far` | `guns.xml` 的 `<shots><X><piercingPower>85 72</piercingPower>`——一个标签两个值，近/远 |
| `damage` / `module_damage` | `shells.xml` 的 `<damage><armor>140</armor><devices>105</devices></damage>` |
| `shell_type`（`ap`/`hc_premium`/…） | 同 shell 的 `<icon>` 或 `<kind>`（`ARMOR_PIERCING`/`HOLLOW_CHARGE`） |
| `velocity` / `range` | `guns.xml` 的 `<speed>` / `<maxDistance>` |
| `caliber` / `normalization` / `ricochet` / `explosion_radius` | `shells.xml` 的 `<caliber>` / `<normalizationAngle>` / `<ricochetAngle>` / `<explosionRadius>` |
| 装填/瞄准/散布/弹鼓/连发 | gun 级 `<reloadTime>` `<aimingTime>` `<shotDispersionRadius>` `<clip>` `<burst>` |
| `hp` | 车体 `<maxHealth>` + 炮塔 `<maxHealth>` |
| 机动 | `<speedLimits>`、chassis 的 `<rotationSpeed>` |
| shell `id` | `shells.xml` 的 `<id>`，经 `loadout::blitzkit_shell_global_id` 换算到回放/BlitzKit 的全局弹种域 |

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
| 履带厚度 | chassis 段 |

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

第 3 类走 `ICON_ALIAS` **人工核对别名表**（宁可缺失也不猜——错挂别车的图标比缺图更糟）。
对照诊断用 `tools/compare_tank_icons.py`。当前覆盖率：**730/735**，余 5 辆客户端确无大图
（`T1_hvy`/`Indien_Panzer`/`R71_IS_2B`/`F68_AMX_Chasseur_de_char_46`/`PzVI_GuP`）。

### 2.6 客户端关键路径速查

| 用途 | 路径 |
|---|---|
| 车辆与模块定义（DVPL 压缩，容器为 zip） | `Data/XML/item_defs.zip` |
| 逐车渲染参数（贴图变换/后坐/悬挂/掩体平面） | `Data/3d/Tanks/Parameters/{nation}/{model_name}.yaml.dvpl` |
| 科技树与 tier | `Data/Configs/TechTree/{nation}_tree.yaml.dvpl` |
| 本地化显示名 | `Data/Strings/{lang}.yaml.dvpl` |
| 坦克图标 | `Data/Gfx/UI/BigTankIcons/`、`.../BattleScreenHUD/SmallTankIcons/` |
| 客户端版本号 | `Data/version.txt.dvpl`（`update-data` 的版本感知依据） |

---

## 三、唯一缺口：`tank_id` ↔ 游戏模型名的桥

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
根 `manifest.json` 含全量 sha256（4124→4131 条）。发布要点：

1. **先做一致性安全检查**：比对桶内 `manifest.json` 与本地旧包快照的 sha256，并抽查若干
   对象核对清单哈希——确认桶处于预期状态，避免覆盖他人改动
2. **按 manifest 差分上传**，只传真正变化的：`新增 + 内容变更` 的对象，外加 `manifest.json`
   本身；本轮为 479 个对象（7 新增 + 472 变更，2.58 MB），其余 3652 个未触碰
3. **不设 Content-Type**：桶内既有对象均未设置，保持一致以免引入元数据漂移
4. **上传后回拉校验**：逐对象下载比对 sha256
5. `manifest.json` 的 `generated` 是本次更新时刻，会与 `index.json`（地图资产未变）不同步；
   若需两者一致须重跑 `export_asset_pack.py` 全量重打包
6. 回滚素材：旧包状态的本地快照（`game_data/` + `tank/` + `manifest.json`），必要时原样传回

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
