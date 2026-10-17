# 游戏数据来源与 BlitzKit 依赖评估

> 回答两个问题：**每一份数据到底从哪来**，以及**能否摆脱对 BlitzKit 数据源的依赖、
> 改由解析本机游戏文件自产**。文末附资产面（COS）发布流程与运维陷阱。
> 所有结论均有实测证据。

## 一、结论

**数据内容可以本地自产，运行期 11+1 项数据现已全部客户端化。** BlitzKit 降级为
"**交叉校验对象**"（`fetch-blitzkit` 保留，定期用本地提取结果与其 pb 做 diff，漂移即
告警），运行期与提取链都不再依赖它。

- 已由游戏文件替代：`tanks.pb` 全部内容、`models.pb` 全部内容、GLB 几何、坦克封面图、
  地图资产（场景/地形/底图/小地图/可破坏物）；
- `tank_id` ↔ 游戏模型名的桥：游戏本地文件**不含现成的表**，但
  `tank_id = (list.xml <id> << 8) | 国家基数` 可**纯客户端重建**（全量一致），桥表已
  固化入库（§三）；
- 提取链与对照链共用同一套口径，换源过程以"BK ↔ 客户端"全量对照收口
  （剩余 122 处差异全部定性，见 [data-inventory.md](data-inventory.md) §一）。

---

## 二、数据源清单

### 2.1 当前来源

| 产物 | 当前来源 | 本地可否替代 | 证据 |
|---|---|---|---|
| `data/tanks.pb` 坦克数据库 | 本机客户端解包编码（`tools/extract_vehicles.py` → `tools/emit_vehicle_pb.py` + 补充表；BlitzKit 降为对照校验源） | ✅ 完全 | §2.2 |
| `data/models.pb` 模型定义 | 同上（同批） | ✅ 完全 | §2.3 |
| `data/cache/models/{id}/*.glb` | **本机客户端**（`tools/export_tank_glb.py` 自产；BlitzKit CDN 仅缺失兜底） | ✅ 已实现并接线 | §2.4 |
| `data/cache/tank_images/{id}.webp` | **本机客户端**（`tools/export_tank_icons.py` 剥壳字节；BlitzKit 仅缺失兜底） | ✅ 已实现并接线 | §2.5 |
| `data/game_data/{id}.json` 装甲/碰撞盒 | 本机客户端 DVPL | — | §2.2/§2.3 |
| `data/tank_cache.json` | `tanks.pb` 派生（纯本地，**不经 WG API**） | ✅ 随 pb | — |
| 地图资产（底图/地形/小地图/场景） | 本机客户端 | — | `tools/export_map_glb.py` |
| 玩家战绩 | WG API | 不可（本来就不是本地数据） | — |

`fetch-tanks` 的实现是 `TankResolver::from_blitzkit()` 后落盘，`api_client.rs` 明确
"坦克百科数据已全部来自 BlitzKit，不走 WG 百科"——WG API 只服务战绩查询。

### 2.2 tanks.pb 的内容 ← 游戏 XML

来源是 `Data/XML/item_defs.zip`（3MB、892 条目）内：

- 车辆定义 `vehicles/{nation}/{model_name}.xml.dvpl`（约 790 个；其中 **6 辆**在
  `<extras><armorsStatesController>` 声明动态装甲状态，由 `tools/extract_armor_states.py`
  提取为 `data/armor_states.json`，运行时语义见 [data-inventory.md](data-inventory.md) §1b）
- 共享模块 `vehicles/{nation}/components/{guns,shells,turrets,chassis,engines,radios,fuelTanks}.xml.dvpl`（72 个）
- tier：`Data/Configs/TechTree/{nation}_tree.yaml.dvpl` 的 `position[0]`
- 名称（**两个字符串源**）：
  1. 随包基础快照 `Data/Strings/<lang>.yaml.dvpl` 的 `#<nation>_vehicles:<KEY>`——**必须按
     元素自带的完整前缀键字面查**（`gb_vehicles` 450 键与 `uk_vehicles` 478 键并存、逐条
     不同；裸键回退会跨系相撞），值里的 YAML 转义（`ä`→ä）需解码、值捕获须
     转义感知（`"` 正则会 被 `\"` 截断），A/B 尾字母重复条目的名字可回退其基础键；
  2. **运行时本地化覆盖层**：客户端从 CDN 下载并缓存到
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

**弹药列表的权威来源是共享炮定义**：`vehicles/{nation}/components/guns.xml`
的 `<shots><shell_tag><piercingPower>` 带弹道与穿深；车辆 XML 内联 `<shots>` 是价格/可用性叠加
（`<shell>shared<price>…`），个别车引用的弹种族与共享定义不同（`J24_Type_57`：内联 base 族在
客户端无任何弹道数据 → 穿深 0；共享 A 族 = 218/260/65 与 BlitzKit 一致）。解析规则：按共享
`<shots>` 列表迭代，内联同标签条目合并覆盖其余字段。

**其它名称/参数相关文件**（扫描所见，未接入）：`hangarBaseParamsPrecomputedData_generated.yaml`
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
| 炮塔初始姿态 `initial_turret_rotation` | 车辆 XML `<turretInitialRotation><yaw>/<pitch>/<roll>`（度）——与 models.pb field3 逐值一致（.sc2 的 TransformComponent 全是单位阵，不是来源） |
| 履带厚度 / 原点 | chassis 段；已按**逐底盘 map** 保留（C1 修复，见 [decoupling-status.md](decoupling-status.md) §2） |

**逐炮塔/逐主炮装甲透出**（2026-10-17）：`tank/{id}.json` 的 `configs[]` 现在带
**additive** `turret_plates`/`turret_spaced`（按 `config.turret_index` 对应的 models.pb
炮塔变体）与 `gun_plates`/`gun_spaced`（逐主炮；`gun_thickness` 已有）。顶层
`armor_model.turret/gun` 仅保留为 tank 详情卡的**顶级摘要**。多炮塔/多主炮车（如 10625
stock `turret_01` plate 11 = 130 mm 主装甲 vs top `turret_02` plate 11 = 30 mm 间隙甲）
必须按当前 config 取表，不能把顶级摘要套到 stock——消费方按 `turret_index`/当前主炮
选表；字段缺失按 0/空 **fail-closed，不回退 `armor_model`**。

**枪序口径**：顶塔主炮列表按 **XML 文档序 = 游戏研发序**（末位 = 顶级主炮）导出；
BlitzKit 的 models.pb 是**模块 id 升序**（与研发序在 105 辆上序不同——92 辆末位主炮不同，
其中 15 辆末位主炮板集实际不同）。客户端 XML 每个 gun/turret 条目带 `<level>` 研发
等级与 `<unlocks>` 研发图，可作为交叉校验。

**空心碰撞盒口径**：43 辆无炮塔 TD（AT-SPG，固定战斗室车，如 StuG III G /
SU-85 / Hetzer / JPanther / Tortoise / T110E3）的**炮塔**碰撞盒两侧都只有空占位——客户端参数
YAML 无 `turret_NN` 段（实测 SU-85：`turret_` / `gun_` 0 处），item_defs XML 的炮塔 bbox 为全 0，
BlitzKit models.pb 也只有 **4 字节空消息**（`0a 00 12 00`，min/max 两个空 Vec3）。因此判据统一
为：**min/max 双双为空消息 = 无数据**，不是"零盒"。Rust 严格解析（`parse_vec3` 空 → `None`）
把它落成 `null`（包内这 43 辆 `collision_boxes.turret` = null，其余 chassis/hull/gun 三盒
738/738 齐备）；Python 对照器与 `extract_vehicles.py` 为对齐 pb 序列化而保留全 0
（该文件注明"全 0 bbox 不省略——实测 43 个炮塔"）。⚠️ 换源/对照时不得把空心读成实值。

### 2.4 GLB 几何 ← 客户端模型文件

**权威路径不在目录约定里，而在逐车的参数 YAML**：`Data/3d/Tanks/Parameters/<小写国名>/<模型名>.yaml.dvpl`
的 `resourcesPath.blitzModelPath`（碰撞走 `resourcesPath.collisionMesh`）。目录约定只是**回退**：

| 资产 | 权威来源 | 回退（按约定） |
|---|---|---|
| 视觉模型 | `resourcesPath.blitzModelPath` 指向的目录 | `Data/3d/Tanks/<大写国名>/<模型名>.sc2.dvpl` + `.scg.dvpl` |
| 碰撞模型 | `resourcesPath.collisionMesh` | `Data/3d/Tanks/CollisionMeshes/<小写国名>-<模型名>.sc2.dvpl` + `.scg.dvpl` |

**两套国名互不相同，按目录去猜必错**：

- `XML/item_defs/vehicles/` 与 `CollisionMeshes/` 用 **tanks.pb 的小写国名**（`germany`/`uk`/`ussr`…）
- `3d/Tanks/` 下的车辆目录是**另一套大写名**，且不是简单大写化——映射表在
  `tools/export_tank_glb.py` 的 `NATION_DIR`：
  `{germany→German, ussr→USSR, usa→USA, uk→GB, china→China, japan→Japan, france→France, european→European, other→Other}`
- 即便国名对了，`3d/Tanks/<国>/<模型名>.sc2.dvpl` 也**不普遍成立**（如 PzIV 的模型在
  `German/G79_Pz_IV_AusfGH/...`），所以必须走上述权威路径，别用约定拼。

导出实现 = `tools/export_tank_glb.py`（复用地图管线的 DAVA 原语：`wotb_sc2.py` 解
KeyedArchive 实体树、`wotb_scg.py` 解 PolygonGroup 顶点/索引流 + `GlbBuilder`）。
验收口径 = **逐字节 + 节点顺序**（见 [local-model-export.md](local-model-export.md)）。

### 2.5 封面图 ← 客户端本地

> 现役来源 = 本目录（`tools/export_tank_icons.py` 剥壳裸 webp，738/738）→ 同步进
> `data/cache/tank_images/`；BlitzKit `fetch-icons` 与运行期懒下载退为缺失兜底。
> 与 BK 同源同画：alpha 轮廓 100% 一致、RGB 差为 BK 二次重编码噪声（mean 1.79 / max 17）。

- `Data/Gfx/UI/BigTankIcons/<国家标签>-<名字>.packed.webp.dvpl`（2312 个，含 `@2x` 与 `_skinN` 变体）
- `Data/Gfx/UI/BattleScreenHUD/SmallTankIcons/`（1474 个，128×32，作兜底）

**权威来源 = 客户端逐车声明**：`3d/Tanks/Parameters/<nation>/<stem>.yaml.dvpl` 的
`bigIconPath` / `smallIconPath`（764 份参数文件中 755 份为逐车文件、全含该字段）——
**按整文件名（含国家标签）精确解析**（`ussr-IS_2` 必须命中苏联图标，不能归一化后撞上
同键的 `china-IS2`），738/738 全部由此命中。命名比"标签-模型名"复杂，需一并接受的形态：

1. 国家标签有一处客户端错拼须一并接受：`britsh`（另有 `british`）；
2. 有些图标名**不带任何国家标签**（如 `Frankentank_event`）；
3. 有些用**内部代号/缩写**，单靠拼名字推不出（`ST_I` → `ussr-R63_ST_IBD`、
   `Pro_Ag_A` → `germany-Leopard_PT_A`、`M2_med` → `usa-M2_MT`）——走 `ICON_ALIAS`
   人工核对别名表（宁可缺失也不猜，且别名表**优先**于启发式匹配）。

备选来源（仅在声明缺失时使用，按序）：`camouflages.yaml` 注册表 → list.xml 短名/全名 →
模型名及去前缀变体 → stem＋显示名词 → 人工别名表；弱来源加**唯一性护栏**（会撞他车的
命中被弃），并加"剥编号须留 ≥2 词"约束。**刻意不启用模糊匹配**：实验里模糊匹配会落到
**别的车**（`PzV_PzIV`→`PzIV`、`GB24_Centurion_Mk3`→`Oth41_Centurion_Mk3_S2`），
宁可留空也不串车。对照诊断用 `tools/compare_tank_icons.py`。

**展示裁剪**：图标字节保持游戏原始画布（256×128，坦克锚定左上、右下留白），
`export_tank_icons.py --emit-crops` 输出逐车内容框（alpha>0，确定性、无统计判定）
→ `data/cache/tank_icon_crops.json` → 包内 `tank_images/crops.json`（打包器
fail-closed）；消费方展示层按框缩放定位。图标重导后须重跑 `--emit-crops`。

### 2.6 客户端关键路径速查

| 用途 | 路径 |
|---|---|
| 车辆与模块定义（DVPL 压缩，容器为 zip） | `Data/XML/item_defs.zip` |
| 逐车渲染参数（贴图变换/后坐/悬挂/掩体平面） | `Data/3d/Tanks/Parameters/{nation}/{model_name}.yaml.dvpl` |
| 科技树与 tier | `Data/Configs/TechTree/{nation}_tree.yaml.dvpl` |
| 本地化显示名 | `Data/Strings/{lang}.yaml.dvpl` + 运行时本地化覆盖层（§2.2） |
| 坦克图标 | `Data/Gfx/UI/BigTankIcons/`、`.../BattleScreenHUD/SmallTankIcons/` |
| 客户端版本号 | `Data/version.txt.dvpl`（`update-data` 的版本感知依据） |

逐车 yaml 的 `suspension:` 块（逐轮行程数组 + 履带折线 + 弯曲/铺放系数）已由
`tools/export_tank_suspension.py` 导出为包内 `suspension/<tank_id>.json`（728/738 辆；
9 辆无 `suspension` 块、1 辆空悬挂按 fail-closed 不产文件）；字节结构、节点绑定契约与
未定项见 [tank-suspension-client-re.md](tank-suspension-client-re.md)。

同文件族的 `treadParameters:` 块（地面履带印：印记条带宽/间距 + `texturePath`）
与 `Data/3d/Tanks/Common/images/*tread*.dx11.dds.dvpl` 贴图（13 张，256² 软边泥痕条带）
已由 `tools/export_tank_tread.py` 导出为包内 `tread/`（**738/738 辆**：authored + 国别主
贴图回落）+ `tread/textures/*.webp`（13 张 DDS 无损转码）+ `tread/textures.json`
（alpha 归一化表）；`.tex` 引用在数据面的落点规则 = `<stem>.tex` → `<stem>.dx11.dds.dvpl`
（与坦克贴图同）。可行性定案与前端消费口径见 [feasibility-track-marks.md](feasibility-track-marks.md)。

---

## 三、`tank_id` ↔ 游戏模型名的桥（可由客户端重建）

`tank_id` 是 WG 图鉴分配的编号（T-34 = 1、M4A3E2 = 10017），回放 `battle_results` /
`ARENA_INFO` 里用的就是它；提取时要定位 `vehicles/{nation}/{model_name}.xml.dvpl`，
必须把 `tank_id` 换成 `model_name`。

**桥的现状**：`data/tank_id_bridge.json`（738 条）由 `tanks.pb` 导出（`--emit-bridge`）；
**并可纯客户端重建**——`Data/XML/item_defs/vehicles/<nation>/list.xml` 每个车辆条目的
`<id>` 是国家内局部 id，全局 id = `(局部 id << 8) | 国家基数`（与 guns/shells 同一编码，
国家基数见 `tools/extract_vehicles.py::NATION_LOW`）。全量实测与 pb 导出的桥表
stem/nation 逐条一致，另多出 **20 辆** BK 没有的车（教程 bot、超测/开发车）。
工具入口：`python tools/extract_vehicles.py --emit-bridge --from-client`。`slug`（BK 专有）
无消费者，可留空——文件定位一律用 `stem`（游戏模型名）。

补充事实（全量扫描定界）：客户端**静态文件不含现成的 tank_id 表**——Data 下 31,516 个
文本容器按"已知 tank_id 字面量 ↔ 模型名共现"搜索 0 命中；`tank_id` 由 WG 服务器在车库
协议中下发（`wotblitz.exe` 协议字段名表 + 运行期缓存 `base_stuff_Asia.dat` 双向佐证），
客户端在运行期才知道。`tanks.pb` 每条坦克的主消息内 **field 1 = tank_id、field 32 =
游戏模型名**并列（field 2 = `dev_name` 是 slug，**不能用于文件定位**）。

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
- `data/shell_kinds.json`（弹种反解表，`dump-shell-kinds --out` 产物）是 **tanks.pb 的直接
  派生**：每次 pb 刷新后必须同批重导，否则打包器物化的 `data/shellKinds.json` 与包内 pb 脱版。
  打包器对它 fail-closed（缺源即拒绝打包），但**同版性只能靠刷新链顺序保证**——先 `update-data`
  再 `dump-shell-kinds`，同一回合完成

### 5.2 资产面（COS）增量发布流程

桶：`wotbtools-assets-1478073677`（ap-shanghai），布局与 `release/asset_pack/` 逐项对应，
根 `manifest.json` 含全量逐文件 sha256（**5766 条**）。发布链固定为
**重打包 → 俯视合成 → `--refresh-manifest`（合成之后、上传之前）→ 差分上传**——
`--refresh-manifest` 是必需的：合成器在打包之后改写 `ground.webp`，不重算则这 36 项的
manifest 条目与包内件不一致。朝向契约、重烘与读回校验见 §5.5。

**首选工具**：
[tools/upload_asset_pack_cos.py](../tools/upload_asset_pack_cos.py)（差分比对 + 并发上传 +
`manifest.json` 强传 + Cache-Control 策略；`--only <prefix>` = **局部发布**——只传匹配前缀的
文件，且 manifest 改为**拉远端清单就地补丁**、只登记本次上传的条目，多会话共用一个包目录时
不会把他人在飞的改动宣布为线上内容）。下列要点同时是手工流程的检查单：

1. **先做一致性安全检查**：比对桶内 `manifest.json` 与本地旧包快照的 sha256，并抽查若干
   对象核对清单哈希——确认桶处于预期状态，避免覆盖他人改动
2. **按 manifest 差分上传**，只传真正变化的：`新增 + 内容变更` 的对象，外加 `manifest.json`
   本身
3. **跳过判定 = 桶内 `manifest.json` 的逐文件 sha256**：内容变了就必须传，**哪怕尺寸相同**
   （只比 `Content-Length` 会把这类修正静默跳过——如 `map/lagoon/ground.webp` 与坦克
   顶点烘焙：只改内容不改字节数）。远端 manifest 缺该条目（首传/旧对象）时
   回退尺寸比对；取不到远端 manifest 则整体回退并告警——此时第 5 步的逐对象回拉校验是唯一兜底
4. ⚠️ **上传会遍历包目录下全部文件**：俯视烘焙的渲染中间产物 `<pack>/overhead/*.rgba`
   （按图 64 MB，非包内资产）必须提前移出，约定缓存在 `release/overhead-bake/`
   （曾误传 73 个对象 / ~2.4 GB，已清理）
5. **上传后回拉校验**：逐对象下载与**本地包文件**比对 sha256（manifest 条目现与包内容自洽，
   可当基准；仍建议抽样回拉以防传输侧意外）
6. **不设 Content-Type**：桶内既有对象均未设置，保持一致以免引入元数据漂移
7. `manifest.json` 的 `generated` 是本次更新时刻，可能与 `index.json`（地图资产未变）不同步；
   需两者一致须重跑 `export_asset_pack.py` 全量重打包（或按第 3 条判据重传 `index.json`）。
   另：`worktree_dirty=true` 的包**不等于**标注 commit 的原样工作区（见
   [data-inventory.md](data-inventory.md) §2.1）
8. 回滚素材：旧包状态的本地快照（`game_data/` + `tank/` + `manifest.json`），必要时原样传回
9. ⚠️ **凭据字节要干净**：Windows 下经管道/命令替换传 `COS_SECRET_*` 时注意尾部 `\r`
   （python 文本模式的换行转换会把 `\r` 带进 SecretKey ⇒ 比对与上传**全部** `SignatureDoesNotMatch`；
   上传阶段全失败即先核对凭据长度是否与源文件一致，多 1 字节即此坑）
10. ⚠️ **发布后"客户端仍显示旧图"先查缓存窗口**：二进制资产带 `Cache-Control: max-age=3600`
   （有意折中，见 `tools/upload_asset_pack_cos.py` 注释）⇒ 浏览器在新鲜期内**不回源**，改图后
   最多约 1 小时才可见。诊断三步：①带 `?v=` 查询串另开一页对照（换缓存键 ⇒ 必为服务端现内容）；
   ②DevTools → Network 看 `ground.webp` 的 `(disk cache)` 与 `Content-Length`/`Last-Modified`；
   ③需要精确判定就 `fetch(url, {cache:'no-store'})` 算 sha256 与本地件比对。App 端不同：
   Native 代理（`AgentAssetProxy.kt`）每次请求都带 `If-None-Match` 条件重取 ⇒ 换内容即时生效。

### 5.2b 本机解包 → pb 编码器（现役数据源）

工具链（本仓）：

1. `python tools/extract_vehicles.py --all` → `data/cache/local_pb/{tanks,models}.json`
2. `python tools/emit_vehicle_pb.py --emit-supplement` → `data/local_pb_supplement.json`
   （客户端无对应概念/数据的字段集中登记：`dev_name` 738 与**名字 2 条**——1 枪 + 1 履带，
   两种字符串源都没有；`initial_turret_rotation` 已客户端化、不再需要）
3. `python tools/emit_vehicle_pb.py` → 编码为同格式 pb（只写 `blitzkit.rs` 运行期读取的字段；
   弹鼓按 field1×N + field2/3；全 0 向量/空心盒不写）
4. 对照/回归：`python tools/compare_vehicle_data.py [--pb-dir data/cache/blitzkit_pb_snapshot]`
   （换源前 pb 快照在 `data/cache/blitzkit_pb_snapshot/`，随时可复现 BK↔客户端对照）

**换源转正回归（三门全过）**：① `tank_cache.json` 逐字段对照仅 `10625`
explosion_radius 一处差异（客户端值，BK 错值）；② `armor_model.gun` × `game_data`
矛盾 **15 → 0**（turret 2 / hull 6 残留两源同数 = f32 量化）；③ GLB 节点号耦合抽样 5/5。
`tank_data` 18 辆差异 = 15 辆顶塔末位主炮板集（研发序）+ 2 辆 explosion_radius + 1 辆形状
——即 BK 源下"末位=顶级"误选的修复。弹种 id 常量偏移（`7009`/`12929`）经查为
**BK 侧按 gun module 塌缩 + 孤儿弹种族**所致，客户端数据自洽（共享炮 `<shots>` 为权威后
0 配对失败）。**BK↔客户端剩余差异共 122 处**（119 枪序＝研发序口径、1 explosion_radius＝
BK 错值、2 名字＝补表兜底）。

### 5.3 DVPL 外壳的校验契约（fail-closed）

提取链的每条读盘入口都过 `dvpl.rs`：装甲 XML / 碰撞 YAML（`extract-game`）、
`version.txt.dvpl`（`update-data`）、小地图与 `maps.yaml` / `en.yaml`（`map_assets`）。

footer 布局为 `decoded_size(4) + encoded_size(4) + crc32(4) + type(4) + "DVPL"(4)`，其中
**crc32 覆盖存储载荷（压缩后字节），不是原始数据**。解码器核对三条不变量，任一不符即
返回 `Err`：编码长度 == 实际载荷长度、存储载荷 CRC32、解压长度 == `decoded_size`。
Python 侧（`tools/wotbtools/wotb_sc2.py`）同口径——两端一致 fail-closed。

**LZ4 零偏移**：真机语料存在 `offset = 0` 的匹配序列，这不是规范 LZ4，但客户端确实产出、
参考实现（lz4 C，即 Python `lz4.block`）也照样解开——回引位置就是当前写出位置、且该处尚未
写出、缓冲区初值为零，等价于**写出 match_len 个零字节**。故两侧都按零处理而非拒绝。
全树 45016 个文件里仅 1 例：
`3d/Tanks/France/images_pbr/F114_Projet_4_1_skin_MISC.dx11.dds.dvpl`
——位于 `Skin_01` 配置里，`MaterialLibrary::_flatten` 只取 `configName == "Default"`，
**当前不可达**，属潜在陷阱；两端口径已一致，将来导出器支持皮肤配置也不会撞上。

复核探针（应全量零拒绝：本机客户端 `Data/` 45016 个 `.dvpl` / 20.3 GiB）：

```
WOTB_DVPL_PROBE=<Data 目录> cargo test dvpl_client_probe -- --ignored --nocapture
```

### 5.4 DLC 覆盖层（`packs/`）——读取优先级

客户端把 DLC 微更新写到 **`%LOCALAPPDATA%\wotblitz\packs`**，以**相同的相对路径覆盖**
游戏 `Data/` 下的同名文件（第三方 mod 工具文档明写 "its files override the base ones"）。
**微更新不落在 `Data/`**，所以只读 `Data/` 会拿到 DLC 应用前的旧版本。本机实测 45 个
`.dvpl`：5 个与 `Data/` 同名、内容不同（含坦克可视模型 `3d/Tanks/German/Ferdinand.sc2.dvpl`
与同车 `.scg.dvpl`）；另 40 个 `Data/` 里根本没有（全是 `G37_Ferdinand_skin` 皮肤资产）。

**约定：所有客户端资源读取都经统一解析器，packs 优先、缺失回退 `Data/`**：

- Rust：`src/wargaming/game_extract.rs` 的 `packs_dir()` / `resolve_client_path()`；已接入
  车辆 XML/YAML 定位、`version.txt.dvpl`、`maps.yaml` / `Strings/en.yaml`、地图 landscape 目录。
- Python：`tools/wotbtools/dlc_packs.py` 的 `packs_dir()` / `client_path()`；已接入
  `export_tank_glb.py` 的模型/碰撞/参数路径解析。回归测试 `tools/test_dlc_packs.py`。

非 Windows / 无 `packs` 目录（WSL、纯净检出）时自动退回只读 `Data/`，行为不变。

### 5.5 俯视合成（均衡档地面）的朝向契约与重烘

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
低信号图上的梯度相关 argmax 会退化为噪声判决（曾把图形烘成上下镜像），因此
**该搜索已降级为 `audit` 诊断项**（不影响写盘，选中别的候选时在报告里标注）而不是判据。

**地面语义与合成权重**：烘焙**自己渲染地面**（共享模块
`WotbTools/frontend/src/scene/groundPass.js`：地形几何 + 分层地表 `cm×lm×tile/mask` 实时合成 +
水件/贴花按运行时同判保留渲染 + 贴地薄构件走廊掩码强制细分）——`ground.webp` 为
**"与 3D 档同源的正交俯视渲染"**。合成权重随 `<key>.meta.json` 的 `ground` 标志：
渲染含地面 ⇒ **覆盖即替换**（`out = rgb×a + ground×(1−a)`），无该标志的旧件沿用
`out = base×(1−0.92a) + render×0.92a`。剔除规则与运行时同式（天空=`/sky/`；
`dome/loonar/surroundings/^plane` 名字启发式与"≥90% 图幅壳"几何剔除**均已删除**——
穹顶建筑/外圈山体/大地壳/小件与运行时**逐件同判**保留）。具体剔除规则以
`WotbTools/frontend/src/scene/sceneryAssemblyRules.js`（运行时与烘焙同一份判定）为准。

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

⚠️ **渲染缓存必须与线上底图同世代（硬约束）**：地面 pass 世代（P2）的底图**只能**由
「地面 pass 世代的渲染件（meta 带 `ground=true`）」合成——旧世代渲染件（meta 无 `ground`、
alpha 覆盖率 ~20% = 只有俯视件）按本节命令重合成会把 36 张底图**回退一代**。判据是语义
标记而非统计：`meta.ground` 缺失 = 该渲染不含分层地面 pass。**两条硬约束**：

1. **归档助手**：烘焙把渲染件写进**包内** `overhead/`（合成器默认就从那里读），"合成完即清包内"是
   惯例而归档是唯一留存路径——用 `python tools/archive_overhead_bake.py [--verify-only|--clear-pack]`
   归档到 `release/overhead-bake/`：`<key>.rgba` 与 `<key>.meta.json` 必须**成对**（缺一半即整体拒绝，
   不部分归档）、拷贝后逐件字节数核对、逐图打印世代（缺 `ground` 键即警告）。
2. **合成器世代护栏**：`tools/composite_overhead.py` 对 meta 缺 `ground` 键的渲染件 `--write`
   **默认拒写**（fail-closed；每张图报告带 `generation` 字段），显式 `--allow-legacy-render`
   才放行（仅诊断/复现历史，不用于发布）；`--verify` 对旧世代渲染件附
   `generation_warning`（按 0.92 反解地面 pass 成品会失真，该 `ok` 不可用于判定）。
   回归锁：`tools/test_overhead_guard.py`（世代判据 / 拒写闸门 / 成对校验三组用例）。

**现役渲染件**：`release/overhead-bake/` 归档 = ground-pass 世代（与线上底图同代，
包内 == 归档 == 线上）；仅需线上字节时可用 COS 公开源只读回拉
（`https://wotbtools-assets-1478073677.cos.ap-shanghai.myqcloud.com/map/<key>/ground.webp`，
逐张 sha256 对照 manifest）。

### 5.6 2D 战术底图（WotbTools 前端资产）的派生链

2D 地图鸟瞰 / 战局回放用的 29 张底图不是客户端资产，也**不再**是外部 AI 增强件，而是本仓自产：
`release/asset_pack/map/<key>/ground.webp`（4096²，含俯视屋顶/树冠烘焙）经
`tools/export_2d_basemaps.py` 派生 → `release/2d-basemaps/`（canonical 1254² WebP q90 / method 6）
→ 交接 WotbTools `frontend/src/assets/maps/`（web 与 Android 共用；Android 再走其自身的 ≤1024² 派生）。

**冻结配方**（全 29 张同一套，参数 = 工具内常量）：LANCZOS 1254² → 全局对比曲线
`y' = softclip(128 + (y−128)×1.17)`（两端 24 级软肩）→ 饱和 ×0.9748 → 逐通道软肩（无硬裁切）
→ USM 1.0px/80%/thr2 → WebP q90 / method 6。

**取向/世界框 = 契约，不搜索**：`ground.webp` 顶行 = +Z_scene、左列 = +X_scene（与 §5.5 同一契约）；
WotbTools 的 2D 渲染把回放坐标按 `coordinateBounds = ±300` 线性映射（其 `utils/mapView.js`），
代入「场景 x = −游戏 x」的镜像后两侧同为「左 = −300(游戏 x)、上 = +300(游戏 z)」⇒ **无需翻转/裁剪**。
另做过 8 朝向结构相关作为诊断（29 张全部 identity 最优），但按 §5.5 同一纪律：诊断不参与判决。

**刻意不做的**：候选与旧 AI 增强批的色阶宽度差是**逐图属性**（中位 25.9 级，范围 −17…+46），
按 29 张聚合分布求的匹配 LUT 近似恒等；逐图配平会违反 AGENTS.md「同类数据同一变换、不做测量驱动的
逐项决策」⇒ 只用一条全局曲线按同一比例缩差，残余差距随 manifest 输出、如实记录。

**fail closed 前置条件**（工具内断言）：WotbTools 注册表 `mapImages.js` 可解析且文件名唯一；
源 `ground.webp` 必须 4096²；`terrain.json.worldBounds` 必须 ±300 m；输出目录含注册表之外的
webp 即拒写（防指错目录）。`--verify` 按 manifest 逐张重算并比对 sha256（确定性 + 保真）。

**现役产物**：`release/2d-basemaps/` 29 张 / **21.50 MiB**；`release/2d-basemaps-manifest.json`
记录配方、逐件 sha256 与源 ground 的 sha256；与用户对照页验收版本**逐字节一致**。
**WotbTools 侧不再派生**（2026-10-16 决定）：Web 与 APK 随包的就是这一份 canonical 字节，
像素完全一致（此前 Android 用的 ≤1024²/q72 派生链已整体删除）。

**COS 免发版通道（`map2d/` + `index.json`）**：同一次导出还会把同样字节写成
`release/asset_pack/map2d/<code>.webp` + `map2d/index.json`（**schemaVersion 2**：`generated`、
`recipe`、逐图 `path/sha256/bytes/width/height` **＋几何契约 `worldBounds` / `orientation`**，
2026-10-16 起；schemaVersion 1 缺这两项，消费侧一律不换图），并在 staging 目录写下交接件
`map2d-source.json`（schemaVersion 2：`contentGeneratedAt` = 索引 `generated`、`indexSha256`、
`recipe`，以及**与字节的绑定** `maps: {code: {file, sha256}}`（29 条）与 `mapsDigest`）。
该交接件随 29 张 webp 一起进 WotbTools `frontend/src/assets/maps/`：**包内"内容时间戳"的唯一来源**
——运行时用它与 COS 索引的 `generated` 做单调比较（`index.generated <= contentGeneratedAt ⇒ 包内即最新`）。
**绝不能在消费侧改成构建时间**：stage 晚于 COS 发布才构建时会把真正更新的底图判成更旧而永久拒绝升级，
且同一 commit 重跑构建会产出不同字节的 bundle（破坏 stage 可重跑性与 APK 不可变性）。
**绑定是硬要求**：消费侧必须用本地真实字节重算 `maps`/`mapsDigest` 并 fail closed（WotbTools 构建期
比对随包字节、CI 测试比对仓库 canonical 字节；均不查 COS）。缺了它，"换图但忘了重跑本工具并同步
交接件"会让旧 `contentGeneratedAt` 配上新图，运行时据索引可能把包内更新的图 OTA 降级。
`--verify` 对这三方（交接件 ↔ 索引 ↔ canonical 字节）逐条同源校验。
复现同一批发布时用 `--generated <ISO-8601Z>` 固定时间戳（不加则每次导出都是新 `generated`）。上传走既有发布链（json → `no-cache`、
webp → `max-age=3600`；`--only map2d/` 可局部发布）。**索引是 App 的唯一判据**：
包内那份是否最新、远端该取哪张，全部按 `sha256` 比对；字节在使用前逐件校验，失败回落包内。
App 侧消费（WotbTools `frontend/src/data/map2dOverlay.js`）与"包内兜底 + 在线校验"的完整规则见
其 `docs/reference/maps.md` §2026-10-16。

**几何契约为什么必须随件落盘**：2D 渲染把回放坐标按注册表 `coordinateBounds`（±300 m）线性映射到
整幅图，**栅格分辨率不参与映射**（底图与标记同框拉伸）——所以 sha256/尺寸全对也仍可能"换了图但取景
变了"（裁剪、补边、换框后尺寸可以不变），只有世界框能判出来。索引 v2 的 `worldBounds` +
`orientation` 是消费侧换图的前置条件：与注册表逐值不符 ⇒ 该图不换，`window.__wotbMap2d.maps[code]`
记 `rejected: geometry`。`--verify` 同时回读**真实像素**：必须方形且与索引声明一致。
**本批（2026-10-16）配准证据**（补齐本批此前未做的几何校验）：29 张逐个与
`map/<key>/ground.webp`（±300 全幅渲染，取向/世界框由 `terrain.json` 契约校验）做零位移相关比对——
移位 (0,0)、取向 identity 最优、尺度 1.000、相关 0.987–0.998（残差 = 导出侧色调/USM 逐点滤波），
即**每一张都是包内渲染的全幅等比降采样**（一次性诊断，不入库；判据仍是契约，不搜索）。

### 5.7 客户端小地图（`map/<key>/mini.webp`）——**已退出地面链路**（2026-10-16 裁定：以 ±300 为准）

`src/wargaming/map_assets.rs` 把小地图 DVPL **原样**写成 `data/cache/maps/<space>.minimap.webp`
（`MiniMapSmall@2x` 优先），`scripts/export_asset_pack.py` 再拷成包内 `map/<key>/mini.webp`。

**实测（2026-10-16，36 张）**：把 `mini.webp` 与同图 `ground.webp`（±300 全幅渲染）对齐，36/36 都
**不落在 600 m 方框上**——小地图内容需要**放大约 3%–7%** 才能与契约全幅对齐（canal/himmelsdorf/
malinovka/mountain 聚在 0.962–0.968 ≈ 覆盖 620 m；medvedkovo 0.926），且外围"均匀带"只有 0–3 px
⇒ 多出来的**不是边框，是真实内容**（水面/场景可以超出 Landscape 的 ±300 框；canal 湖面在
`ground.webp` 里被切在画面外、在小地图里完整闭合）。**即：Landscape `worldBounds = ±300` 是地形框，
不等于客户端 HUD 小地图的覆盖范围。**

**处置（用户裁定，路线 B：以 ±300 为准，外圈不呈现）**：3D 地面贴图**四档统一改用契约化的
`ground.webp`**（`frontend/src/scene/playbackScene.js` 不再按画质档选 `res=mini`；`map-mini`
路由已从 `replaySource.js` 移除；守卫见 WotbTools `playbackScene.test.js`「地面贴图四档统一用
契约化的 `map`」）。历史影响：最低档曾把约 620 m 的小地图按 600 m 铺，内容被压缩约 3% 且圈外内容
摊进边界；2D 底图一直只有 ±300 框 ⇒ 低档 3D 与 2D 对不上（2026-10-16 用户报障）。

**仍在包内的 `mini.webp`**：保留（资产面 `res=mini` 与 `map_minimap_response` 的"提取缓存 → 客户端
随取随解 → 高清底图兜底"链未动），但**没有运行时消费方**；若要省包体（36 张合计约 3.6 MiB）可整体
下线提取与打包，属独立决定。**不要**按像素测量值给 `data/maps/<key>.json` 填逐图 `size_m`
（AGENTS.md：不做测量驱动的逐项决策；无权威判据时 fail closed 并上报）——若将来要走"呈现外圈"的
路线 A，须先取到客户端小地图的**权威世界框**（HUD/地图 UI 配置），再统一归一化重出。

---

## 六、许可


从游戏文件提取的数据（装甲数值、模型、图标）版权归 Wargaming，README 已有声明。
解除 BlitzKit 依赖只把来源从"第三方整理"换成"第一手提取"，**不改变权属**。

## 地形附加数据（让位掩码 / 贴地结构脚印）——已全部撤销

让位掩码（`cover.u16.bin`）与贴地结构脚印（`structs.u8.bin`）两代地形附加件均已撤销
（裁示：**只对齐客户端，不自行发明处理方法**）。**最终态：地形没有任何客户端之外的附加
数据**——包内 `map/<key>/` 只含 `terrain.u16.bin` + `terrain.json`（客户端高度图与元数据）
+ 场景/地面资产；打包器在 sidecar 化时清除历史 cover/structs 字段。地形渲染 = 纯客户端
口径（三判据 LOD + 两通道 morph + 真值高度场一份到底，前端侧实现见 WotbTools
`terrainMesh.js`）。

## 地形高度图布局与提取链（`terrainLayout` 2）

高度图自客户端 `3d/Maps/<space>/landscape/` 下唯一 `*heightmap*.dvpl` 提取（`fetch-terrain`
命令；size/tile 非 512/16 一律拒绝），经 `parse_heightmap` 两步：tile 块重排（块行主序 →
行主序）+ **corner 口径列镜像** `out[c] = grid[min(size−c, size−1)]`——引擎顶点 k 在游戏
x = min + k/size·span（`Heightmap::GetPoint` / `tilemask-vp.sl` 同口径），场景 x = −游戏 x
⇒ 输出列 c 的位置 (c/n−0.5)·span 对应源列 n−c，c=0 夹取到源列 n−1；源列 0（场景 +x 包围盒
边缘、可玩区之外）整列丢弃（n 列表示不下 n+1 个顶点）。产物 = u16 LE 行主序（行 0=南），
以 `map/<key>/terrain.u16.bin` 进包，配 `terrain.json`（`X-Terrain-Meta` 头的 sidecar 化，
写 **`terrainLayout: 2`** 与 **`terrainBinSha256`**——bin 的逐文件 sha256，与 manifest 同源：
`terrain.json`（no-cache）与 bin（max-age=3600）是两个独立请求，发布切换期可能错配
（旧 JSON+新 bin = 双重补正、新 JSON+缓存旧 bin = 漏补正），前端按该 hash 对 bin 的
URL 做缓存破坏即可保证配对；服务端 `/api/playback/terrain` 同链伺服（bin 与
X-Terrain-Meta 同响应返回，天然配对，无需该字段）。

- **缓存自描述**：`data/cache/terrain/{key}.hm.c2.u16.bin`（文件名带布局）。打包器仅无 c2
  时兜底打包且**不写布局标记**（前端读取边界按旧布局平移一列补正，打警告提示重跑
  `fetch-terrain --force`）；服务端不再伺服旧名（视作未命中走实时提取重写）；
  `fetch-terrain` 成功写入 c2 后清除旧名。
- **标记必须与字节一致**：消费端（WotbTools `decodeTerrainField`）对 `terrainLayout: 2`
  原样使用、缺省按旧布局补正、未知值 fail-closed 回退 2D——错标比缺省更糟。字节口径上
  四组合（新/旧包 × 新/旧前端）除"旧包+旧前端=原有偏移"外全部正确；json/bin 两个
  请求的 HTTP 缓存错配窗口（见上）由前端按 `terrainBinSha256` 配对根除（落地前的过渡依赖
  "前端切换晚于 COS 发布 ≥ 1 h bin 缓存窗口"的次序约束）。
- 手动覆盖 `data/maps/<key>.heightmap.u16.bin` 的契约 = corner 口径镜像数据（即
  `parse_heightmap` 的输出格式）。
- 重导链：`fetch-terrain --force` → `export_asset_pack.py` → `composite_overhead`（重导会
  覆写 overhead 烘焙地面，必须重烘——见 §5.5 世代硬约束）→ `--refresh-manifest` → COS 差分上传；
  地形件 ~512 KB/图 × 36 图，模型/地面纹理/场景 GLB 不受影响。
