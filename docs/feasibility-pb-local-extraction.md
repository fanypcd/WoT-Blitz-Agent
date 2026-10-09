# 可行性报告 A：用本机客户端解包替代 BlitzKit `tanks.pb` / `models.pb`

> 2026-10-01 产出的评估报告。语料：本机 WoT Blitz 客户端 **11.20.0**
> （`D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data`）与本仓库现有
> `data/tanks.pb` / `data/models.pb`（BlitzKit，735 辆）。
> 复核方式为**对抗式重验**：全部结论由独立重写的 protobuf dump 器与客户端解析器
> 复算，不复用前一轮脚本；每项都给出样本量、命中数与反例。脚本见 §8。
>
> **2026-10-03 整理注记**（以最新实测为准，详见 [decoupling-status.md](decoupling-status.md) §2 C2）：
> ① §2 表中"uk 的前缀是 `gb_vehicles`"与后文"uk 用 `uk_vehicles`"**各自只对一半**
> （2026-10-10 复核）：`Strings/en.yaml` 里 `#gb_vehicles:`（450 键）与 `#uk_vehicles:`（478 键）
> **并存**，且客户端条目的 userString 前缀逐条不同（uk 的 `list.xml`/`shells.xml` 多用
> `gb_vehicles:`）。正确做法：**按元素自带的完整前缀键字面查**（`Names.display` 已如此），
> 裸键回退会跨系相撞。
> ② §7 "24 辆车 en.yaml 无本地化名键"——经 `list.xml` 的 userString 键复核为 **25 辆**；
> 且该键与模型名不一致的有 **7 例**（`AMX_50B`→`AMX_50_68t`、`Object252`→`IS-6`、
> `Oth06_Sega_Lupus`→`Sega_Lupus` 等），本地化查询必须用 list 的键而不是模型名。
> ③ 本文评估的 `tanks.pb`/`models.pb` 提取器**仍待实施**（`data-inventory.md` §四）。

## 0. 结论

**可直接替代，且比预想更彻底。** 在 735 辆车的全量语料上（945 炮塔 / 1554 炮 /
1011 履带 / 1086 引擎），`tanks.pb` 与 `models.pb` 的**每一个字段都有客户端等价来源**，
且经逐字段普查后**零实质分歧**——上一轮报告的那份"非零分歧清单"经三方（pb / 磁盘 /
`item_defs.zip`）对比后**全部被证伪**，成因统一为「对照时读了 `item_defs.zip` 这个旧快照」
（见 §5）。

唯一真实缺口是 **数字 `tank_id`**（WG tankopedia id）：客户端数据文件里确实没有，
但有两条可用替代路径（§6）。

替代的收益不只是"去掉一个外部依赖"：客户端 `yaw_limits`、炮塔名、扇区俯仰
（`<extraPitchLimits>`）等字段的覆盖度**优于** BlitzKit 快照；且数据新鲜度会自动跟随
游戏更新，不再依赖人工执行 `fetch-blitzkit`。

## 1. 依赖现状（谁在消费什么）

| 数据 | 来源 | 主要消费方 |
|---|---|---|
| `data/tanks.pb` | `api.blitzkit.app/definitions/tanks.pb`，`src/wargaming/blitzkit.rs` 手写 protobuf 解析 | `blitzkit::load_tanks`/`tank_full`（20+ 处）、`tank_configs`（生成 `data/tank_data/*.json`）、`game_extract`、`web`、`replay/loadout` |
| `data/models.pb` | `api.blitzkit.app/definitions/models.pb`，同文件解析 | 3D 装甲查看器、穿透计算、`tank_configs`、回放俯仰锚定 |
| 封面图 | `api.blitzkit.app/tanks/{id}/icons/big.webp` | Web / 移动端 |
| 坦克 GLB | `api.blitzkit.app/tanks/{id}/{model,collision}.glb` | 3D 查看器（见报告 B） |

`game_data/`（730 个 JSON）已经是本地解包的产物，历史上被定位为"补 BlitzKit 缺项"；
本次验证表明这个定位可以倒转——本地解包是**超集**。

## 2. 客户端数据源速查（替代所需全部文件）

| 内容 | 路径 | 关键标签 |
|---|---|---|
| 车辆定义（**数值权威**） | `XML/item_defs/vehicles/<nation>/<dev>.xml`（DVPL） | `<speedLimits>` `<hull><armor><armor_N>`+`<vehicleDamageFactor>` `<maxHealth>` `<weight>` `<turretPositions>` `<chassis><X><terrainResistance>` `<rotationSpeed>` `<hullPosition>` `<turrets0><X><yawLimits>` `<gunPosition>` `<circularVisionRadius>` `<guns><G><reloadTime>` `<aimingTime>` `<shotDispersionRadius>` `<pitchLimits>` `<extraPitchLimits>` `<clip>` `<burst>` `<pumpGunMode>` |
| 车辆索引（tier/类别/稀有度） | `XML/item_defs/vehicles/<nation>/list.xml` | `<id>` `<level>` `<tags>`（首词=车种，另有 `collectible`）`<price>`（含 `<gold/>`=金币价）`<sellPrice/>`（=收藏车）`<shortUserString>` |
| 共享弹种 | `<nation>/components/shells.xml` | `<id>` `<kind>` `<icon>` `<caliber>` `<damage><armor>/<devices>` `<normalizationAngle>` `<ricochetAngle>` `<explosionRadius>` |
| 共享炮（弹道权威） | `<nation>/components/guns.xml` | `<ids>` `<shots><X><speed>/<piercingPower>近 远</piercingPower>/<maxDistance>` |
| 共享发动机 | `<nation>/components/engines.xml` | `<power>` `<fireStartingChance>` `<weight>` |
| 模块 id 表 | `<nation>/components/{turrets,chassis,guns,engines}.xml` 的 `<ids>` | 名 → 局部 id |
| 碰撞几何 | `3d/Tanks/Parameters/<nation>/<dev>.yaml` | `collision.<part>.min/max/points`、`maskSlice.<gun_NN>.planePosition` |
| 本地化名 | `Strings/en.yaml` | `#<前缀>_vehicles:<键>`；**uk 的前缀是 `gb_vehicles`** |

> ⚠️ **必须读磁盘目录，不要读 `XML/item_defs.zip`**：zip 是旧快照——869 条 vs 磁盘 881 条，
> **0 条仅 zip 有、12 条仅磁盘有**（含 O-I / O-Ni / O-Ho / Type 4 Heavy / O-I Exp / ZQ-90 /
> Zmije / Mark I Male / KV-4 KTTS / IS-8 Taurus 等），且多处平衡数值陈旧（§5）。
> 另外 `item_defs/vehicles/**` 中的 `.model` 路径是遗留逻辑路径，磁盘不存在（与 GLB 相关，
> 见报告 B）。

## 3. 字段级映射与验证结果

### 3.1 `tanks.pb`

| pb 字段 | 客户端来源 | 全量命中 |
|---|---|---|
| `tank_id` | ❌ 无（见 §6） | — |
| `dev_name` / `nation` | 文件名 / 目录名 | 735/735 |
| `name` | `en.yaml[name.userString]`，回退 `shortUserString` | 711/735（**24 辆 en.yaml 无键**，需回退 dev_name 或 WG API，见 §7） |
| `tier` (f16) | `list.xml <level>` | 735/735 |
| `tank_type` (f17) | `list.xml <tags>` 里的车种词（1=中/2=重/3=TD/缺=轻） | 735/735 ⚠️ 规则须为"**找到**车种词"而非"取首词"（`Cz44_Zmije` 首词是 `mediumAT-SPG`） |
| `hp` (f10) | hull `<maxHealth>`（总 HP=车体+炮塔） | 735/735 |
| `is_premium` / `is_collector` (f13) | 金价 ∧ ¬`collectible` / 金价 ∧ `collectible` | **735/735 双向，0 反例**（pb 直方图 `{缺:306, 1:91, 2:338}` 与客户端完全一致） |
| `speed_forward/reverse` (f25/f26) | `<speedLimits>` | 735/735 |
| `weight` (f31) | hull `<weight>` | 735/735 |
| `turrets[]` | `<turrets0>` | 945/945 |
| ├ `health` (f2) | turret `<maxHealth>` | 945/945 |
| ├ `view_range` (f3) | turret `<circularVisionRadius>` | 945/945 |
| ├ `traverse_speed` (f4) | turret `<rotationSpeed>` | 945/945 |
| ├ `weight` (f8) | turret `<weight>` | 945/945（939 直接相等 + 6 例"客户端 0=省略"） |
| └ `guns[]` | `<guns>` | 1554/1554 |
| `reload` 单发 (f1) | 炮 `<reloadTime>` | 1306/1306 |
| `reload` 弹夹 (f2) | `<reloadTime>` + `60/<clip><rate>` + `<clip><count>` | 194/194 |
| `reload` 弹鼓 (f3) | `<pumpGunReloadTimes>` 逐发 + `60/<rate>` + `<count>` | 54/54 |
| `aim_time` / `dispersion` | `<aimingTime>` / `<shotDispersionRadius>` | 逐炮一致 |
| `shells[].damage` / `module_damage` | `shells.xml <damage><armor>/<devices>` | 逐弹一致 |
| `shells[].penetration` / `penetration_far` / `velocity` / `range` | `guns.xml <piercingPower>近 远` / `<speed>` / `<maxDistance>` | 逐弹一致 |
| `shells[].caliber`/`normalization`/`ricochet`/`explosion_radius`/`type`/`id` | `shells.xml` 对应标签（`type`=`<icon>`，`id`=局部 id） | 逐弹一致 |
| `engines[].power` (f6) | `engines.xml <power>` | 1086/1086 |
| `engines[].fire_chance` (f5, f32) | `engines.xml <fireStartingChance>` | **1086/1086 逐位相等** |
| `tracks[].weight` (f4) / `traverse_speed` (f5) | chassis `<weight>` / `<rotationSpeed>` | 1011/1011 |
| `tracks[].resistance_hard/medium` (f9/f10，另 f11=软) | `<terrainResistance>硬 中 [软]` | 1011/1011 |
| `tracks[].module_id` (f1) | chassis `<ids>`，需 `>>8`（见 §4.2） | 4596/4596 全模块 |
| `hull_traverse` (f27) | ⚠️ 与 chassis `<rotationSpeed>` **非同量**（T-34：16.5 vs 46） | 项目**无消费点**，建议放弃 |

### 3.2 `models.pb`

| pb 字段 | 客户端来源 | 全量命中 |
|---|---|---|
| `hull_spaced` / `hull_plates` | hull `<armor_N>` + `<vehicleDamageFactor>` | 逐板相等（含 0 值板省略） |
| `hull_bbox` | yaml `collision.hull` 的 min/max（**无坐标换轴**，直接相等） | **735/735** |
| `track_thickness` | chassis `<armor><leftTrack>` | 逐底盘相等 |
| `turret_origin` | hull `<turretPositions><turret>` | **735/735** |
| `track_origin` | chassis `<hullPosition>` | **1011/1011** |
| `initial_turret_rotation` | ❌ 无（在 `.sc2` 节点变换内），仅 4 辆欧洲 TD 有值 | 可忽略 |
| `turrets[].module_id` | `turrets.xml <ids>` | 945/945 |
| `turrets[].model_node` | `<models><undamaged>…/Turret_NN.model` 的 N（key 为 `turret_%02d`） | 945/945 |
| `turrets[].turret_spaced`/`turret_plates` | turret `<armor_N>` + `<vehicleDamageFactor>` | 逐板相等 |
| `turrets[].bbox` | yaml `collision.turret_NN` | **945/945** |
| `turrets[].gun_origin` | turret `<gunPosition>` | **945/945** |
| `turrets[].yaw_limits` | turret `<yawLimits>`（`-180 180` 时 pb 省略） | 945/945；**客户端 742/742 全有 vs pb 仅 241 个配置** |
| `guns[].gun_module_id` / `model_node` | `guns.xml <ids>` / `Gun_NN.model` 的 N（`gun_%02d`） | 1554/1554 |
| `guns[].thickness` | gun `<armor><gun>N</gun>` | 1440/1440（1439 直接 + 0 省略） |
| `guns[].mask` | yaml `maskSlice.gun_NN.planePosition[1]`（**+Y 分量**） | 478/478；`enabled:false` ⇒ pb 省略（1039/1039） |
| `guns[].gun_spaced`/`gun_plates` | gun `<armor_N>` + `<vehicleDamageFactor>` | 逐板相等 |
| `guns[].pitch.min/max` | 炮 `<pitchLimits>`（缺则 `guns.xml`），**首值=min、次值=max，不取负** | 1554/1554 |
| `guns[].pitch.front/back` | `<extraPitchLimits><front>/<back>` | 83/83、785/785 |
| `guns[].pitch.transition` | `<extraPitchLimits><transition>` | 113/114（1 反例 `G85_Auf_Panther`：有两个 `<transition>`，需 last-wins 取后者） |

### 3.3 两处需修正上一轮表述

1. **`mask` 不是"沿 `planeNormal` 投影"**，而是取 `planePosition` 的 **+Y 分量**。
   按"投影"理解只有 476/478——2 个反例（`Pz35t`、`T7_Combat_Car`）的 `planeNormal` 非轴对齐
   （如 `(0,1,-0.142858)`）。
2. **`transition` 可能有两个**（`G85_Auf_Panther` 的 front 后有 1、back 后有 3），必须取**最后一个**。

## 4. 实现铁律（违反任一条都会静默出错）

### 4.1 四条硬规则

1. **读磁盘 `XML/item_defs/vehicles/**`，不读 `item_defs.zip`**（§5 全部"分歧"的成因）。
2. **按局部 id 逐模块匹配**：多炮塔 / 多底盘 / 多炮**不能塌缩成坦克级单值**。
   项目现有的 `TrackData` 单条、`track_thickness` 单值、`gun_thickness` 单值就是这种塌缩，
   与客户端多值天然不匹配（这是**项目侧数据模型缺陷**，不是客户端不可信）。
3. **重复 XML 标签 last-wins**：`<armor_N>` / `<pitchLimits>` / `<transition>` 都会重复出现，
   本语料共 4 处会因此翻车（`F113_PzV_FR`、`A99_T92E1`、`AMX40`、`Nashorn_BP`）。
4. **0/缺 = 省略语义**：客户端缺 `<weight>` / `<gunPosition>` / `<hullPosition>` /
   `<yawLimits>` / 全 0 包围盒 ⇔ pb 字段**缺失**。切勿把"缺失"当成"不同值"。

### 4.2 模块 id 编码（回放侧强相关）

`components/*.xml` 的 `<ids>` 给出**局部 id**；pb 里的 `module_id` 是
`(局部 id << 8) | (国家序 × 16 + 1)`，国家序基数为
`ussr=1, germany=17, usa=33, china=49, france=65, uk=81, japan=97, other=113, european=129`。
按 `>>8` 解码后，4596 个模块 id 全部命中客户端 `<ids>` 表。

> **回放侧结论：迁移后这一环会变简单。** `CompDescriptor.turret_local`/`gun_local`
> 本来就是这套局部 id（`crates/replay-core/src/replay/playback.rs`），迁移后局部 id
> 成为**原生主键**，省掉一层 `module_id>>8` 反推；`loadout.rs` 的
> `blitzkit_shell_global_id` 公式不变，只是输入可直接取 `shells.xml <id>`，
> `ShellKindTable` 的构建规模也从"全坦克遍历"降到"9 国 components 扫描"。

### 4.3 两个易踩的陷阱

- **弹夹判定只认车辆内联 `<clip>` / `<pumpGunMode>`**：用 `components/*.xml` 的共享定义兜底
  会产生 **6 例假弹夹**（`T1_Cunningham`、`RenaultFT`、`NC27`×2、`MS-1`×2）——这些炮只在
  共享定义里有 `<clip>`，车辆内联没有，pb 实为单发。
- **`<burst>` 是表现参数**（枪口火光/音效），69 门有 `<burst>` 的炮里 `burst.count != clip.count`
  有 62 例、`burst.rate != clip.rate` 有 14 例，而 pb 一律取 **clip** 的值。
  **禁止**用 `<burst>` 推导 `is_burst`。

## 5. 上一轮"分歧清单"的证伪（重要）

上一轮报告了约 30 处"数值分歧"并归因为"版本错位"。三方（pb / 磁盘 11.20.0 / zip）重算后，
**每一处都是脚本假象**，无一例真实差异：

| 车 | 上报分歧 | pb 真值 | 磁盘 | zip（旧） | 成因 |
|---|---|---|---|---|---|
| `bofors-triton` | 引擎 640/615 | **640** | 640 | 615 | 读了 zip |
| `bofors-triton` | 极速 50/47 | **50** | 50 | 47 | 读了 zip |
| `toro` | 车体装甲 1/7/9 | 175/240/240 | 同 | 190/250/250 | 读了 zip |
| `toro` | 引擎 850/810 | **850** | 850 | 810 | 读了 zip |
| `type-5-h-zetsu` | 炮塔重量 43000/10000 | **43000** | 43000 | 10000 | 读了 zip |
| `type-5-h-zetsu` | 履带转速 19/23 | **19** | 19 | 23 | 读了 zip |
| `type-5-h-zetsu` | 收藏判定 | 非收藏 | `['heavyTank']` | 含 `collectible` | 读了 zip |
| `pz-iii`/`ju-nu`/`bz-68`/`sdp-40-zadymka`/`erac-105-proto`/`projet-louis` | 履带厚度 | 逐条等于磁盘 | — | — | 多底盘塌缩成单值 |
| `regressor` | 炮管壁厚 20/50 | 逐炮 20/20/20/50/60 | 逐炮相等 | — | 用"首个配置单值"对比"顶级炮" |
| `amx-40`/`epsilon` | 全局俯仰 | 与磁盘逐炮全等 | 两处 `<pitchLimits>` | 同 | 首/末取法差异（两侧均 last-wins） |
| `m41-bulldog` | 全局俯仰 | 与磁盘逐炮全等 | — | — | 选错炮/炮塔 |
| `ambassador` | 扇区俯仰 back | `-15 9 25` | 同 | `-15 6 30` | 读了 zip |
| `bdr-g1-b` | 履带原点 | 各自等于自己的 `<hullPosition>` | — | — | 单值塌缩 |
| `nebulon` | 炮原点 | 各自等于自己的 `<gunPosition>` | — | — | 炮塔索引错位 |

**因此：BlitzKit pb 与客户端 11.20.0 磁盘数据在这些字段上零实质分歧。**

## 6. 唯一缺口：数字 `tank_id`（WG tankopedia id）

复验确认客户端数据文件里**没有** tankopedia id（全量扫过 `Data/XML`、`Data/Configs`、
`Data/Strings` 含解码，以及文件名；`list.xml` 的 `<id>` 是本地小 id，如 E-100=37、Löwe=212，
**不能**当 tankopedia id 用）。三条可行路径：

1. **沿用现有桥接表（推荐主路径）**：`tmp_cos/tanks_pb_stems.json`
   （`{tank_id: [dev_name, 客户端文件 stem, nation]}`）——本次复验**735/735 全部命中磁盘车辆 XML，0 未命中**。
2. **WG 官方 API 作校验/重建（推荐辅路径）**：`wotb/encyclopedia/vehicles` 的
   `modules_tree[].module_id >> 8` **就是**客户端局部 id，可做**内容式确定性桥接**
   （客户端某车模块局部 id 集合 ∩ API 该 tank_id 的 `module_id>>8` 集合）。
   实测 528/529 唯一命中、与桥接表 0 冲突。局限：只覆盖 **529/735**（`IS-7/7169` 直接返回 `null`）。
3. 客户端 `list.xml <id>` 只能作**本地代理键**（同一辆车跨版本稳定，但与其他系统不通用）。

**结论**：`tank_id` 需要一个维护中的映射表（现有表已覆盖 735/735），客户端每次热更新后
用 WG API 做一次增量校验即可。

## 7. 需决策的残余问题

| 项 | 现状 | 建议 |
|---|---|---|
| **24 辆车 en.yaml 无本地化名键** | pb 侧有 `name`（来自 BlitzKit），客户端 `en.yaml` 缺键 | 回退 `dev_name`；或 WG API 补（实测可补一部分，如 23841→"Super Hellcat"）；或保留 BlitzKit 名表 |
| `tank_cache.json` 只有 `is_premium`、无 `is_collector` | 前端需要三分显示（普通/金币 ★/收藏 ◆） | 用 `field13` 或 `list.xml` 金价+`collectible` 重新生成 |
| 项目数据模型塌缩多值 | `TrackData` 单条、`track_thickness` 单值、`gun_thickness` 单值 | 若追求与客户端同构需改 schema；否则接受"取顶级配置"的近似并**显式记录** |
| GLB 与 `turret_index`/`gun_index` 硬耦合 | 节点号 = `Turret_NN.model`/`Gun_NN.model` 的 N | 若 pb 换源而 GLB 不换，需保证两侧版本一致（见报告 B） |
| `item_defs.zip` 的存在 | 与磁盘不同步，容易被误用 | 实施时显式绕开；或在文档/代码注释中标注此坑 |

## 8. 顺带发现的两个既有 bug（与本迁移独立，建议尽快修）

> **2026-10-01 已修复**：`src/wargaming/blitzkit.rs` 起火率改读引擎 field 5（fixed32）、
> 履带地形阻力改读 field 9/10，`data/tank_data/*.json` 已按新口径重新生成（735 份），
> 回归断言见 `parse_pb_smoke`。以下为当时的发现记录。

两处都是**把 protobuf 字段号读错**，与"BlitzKit 口径不同"无关：

### 8.1 发动机起火率（影响 715/728 辆车的前端显示）

`src/wargaming/blitzkit.rs` 引擎解析处：`(7, 0) => fire = er.varint()? / 10000.0`。
field 7 实为**引擎重量(kg)**；真正的起火率是 **field 5（fixed32）**。

实测（tank 9489 直接 dump `data/tanks.pb`）：

```
field 5 wire5 = 0.15   ← = 客户端 <fireStartingChance>0.15
field 6 wire0 = 1330   ← 功率
field 7 wire0 = 750    ← 引擎重量 kg（= 客户端 engines.xml <weight>750）
```

后果：`data/tank_data/*.json` 的 `fire_chance == field7/10000` 恒等 1086/1086，
即**全部为错值**（E-100 显示 7.5% 实际 15%；Pz.IV G 显示 5.1% 实际 20%；
M3 Stuart 显示 2.6% 实际 20%）。前端 `frontend/src/views/TankDetailView.vue`
按 `fire_chance*100` 展示，**1053/1075 行在保留 1 位小数后仍与真值不同**。

### 8.2 履带地形阻力（当前无消费点，属静默错误字段）

同文件履带解析处：`(7,5)=resistance_hard`、`(8,5)=resistance_medium`。
field 7/8 实为**客户端 `<shotDispersionFactors><vehicleMovement>/<vehicleRotation>`**
（E-100 两侧均为 0.21/0.21，逐位相等）；真阻力是 **field 9/10/11 = 硬/中/软**
（E-100 = 1.0/1.2/1.62）。

后果：`tank_data` 的 `resistance_hard/medium` 恒等于散步系数（1000/1000），
与真值相等者 **0/1000**。但 `grep -rn resistance src/ frontend/` 显示**全仓零消费点**，
所以目前不产生用户可见错误——属于改名/换源时容易踩的雷。

## 9. 工作量估计（估算，未实施）

| 阶段 | 内容 | 估计 |
|---|---|---|
| 提取器 | 新增 `extract-vehicles` 命令：解析 735 车辆 XML + 9 国 components + 735 yaml + `en.yaml`，输出**与现 schema 同构**的 JSON | 2–3 人日 |
| id 桥接 | 复用 `tanks_pb_stems.json` + WG API 校验脚本（增量） | 0.5–1 人日 |
| 校验基线 | 以现有 `data/tank_data/*.json` 为基线做全量 diff 回归（735 车） | 0.5–1 人日 |
| 消费侧切换 | `blitzkit::load_tanks` 等 20+ 调用点零改动（同 schema）；若修 §7 的多值塌缩与三分稀有度则另计 | 0–1 人日 / +2–3 人日 |

**合计：约 3–5 人日**做到"同 schema 直接替换 + 全量回归通过"；若要一并修正数据模型塌缩，
再加 2–3 人日。估算依据：语义映射已全部验证完毕（本轮工作），剩下的主要是工程实现与回归。

## 10. 证据与复跑

```bash
# 本轮对抗式重验（全部脚本，独立重写的解析器）
cd /d/Class/Rust/Project/tmp_pb_reverify && python run_all.py
```

| 产物 | 内容 |
|---|---|
| `global_census.json` | 全字段 × 全语料分歧普查（38 项，除已解释的 0 省略/重复标签口径外全部 0 mismatch） |
| `verify_9_attrib.json` | 逐车三方（pb/磁盘/zip）对比与归因 |
| `verify_7b.json` | 弹夹/弹鼓/`<burst>` 验证 |
| `pb_tanks_raw.json` / `models_pb_raw.json` | 自写解析器的 pb 全量 dump |
| `client_all.json` | 客户端合并视图（含 last-wins 处理） |

上一轮的覆盖矩阵与逐字段对照另见 `tmp_pb_feasibility/`（`compare_report.json`、
`coverage_report.json`）。全部脚本可重跑；`src/` 未作任何改动。
