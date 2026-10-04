# 数据面清单：使用中 / 未接入 / 分发

> 2026-10-03 整理。回答三个问题：**运行起来到底读了哪些数据**、**每份数据谁在维护、
> 怎么送到消费方**、**哪些替代来源已经备好但还没接线**。
>
> 姊妹篇：[game-data-sources.md](game-data-sources.md) 讲"每份数据从哪来、能否脱离
> BlitzKit"（含逐字段来源映射、spaced 规则、故障复盘）；本文讲"谁在用、谁维护、怎么分发"。
> 两份文档共享同一套来源口径，改动其一时须同步另一份。

## 一、正在使用的数据

运行期（Web / 3D 查看器 / 回放 / CLI）真正会读的共 **11 项**。

| # | 数据 | 来源 | 维护代码 | 分发去处 |
|---|---|---|---|---|
| 1 | `data/tanks.pb` | **BlitzKit** `/definitions/tanks.pb` | `wargaming/blitzkit.rs::fetch_and_save` | COS `data/tanks.pb` |
| 2 | `data/models.pb` | **BlitzKit** `/definitions/models.pb` | 同上 | COS `data/models.pb` |
| 3 | `data/tank_cache.json` | **本机自产**（由 `tanks.pb` 派生，间接 BlitzKit） | `update-data` | COS `data/tank_cache.json` + 派生 `data/tank_names.json` |
| 4 | `data/game_data/{id}.json`（装甲模型） | **客户端解包** `XML/item_defs/vehicles/{nation}/{model}.xml.dvpl` | `wargaming/dvpl.rs::ArmorModel::parse_from_xml` + `wargaming/game_extract.rs` | COS `game_data/` |
| 5 | `data/game_data/{id}.json`（碰撞盒） | **客户端解包** `3d/Tanks/Parameters/{nation}/{model}.yaml.dvpl` | `wargaming/dvpl.rs::CollisionData::parse_from_yaml` | COS `game_data/`（同一文件） |
| 6 | `data/tank_data/{id}.json` | **混合**：`models.pb`（BlitzKit）+ `game_data`（客户端） | `wargaming/tank_configs.rs::export_tank_data` | COS `tank/{id}.json`（改名） |
| 7 | 弹种反解表（全局 shell id → 弹种数据） | **本机自产**（`tanks.pb` 全量展开，离线生成） | `replay/loadout.rs::ShellKindTable::from_tanks_pb` + CLI `dump-shell-kinds` | **消费方前端常量**（`shellKinds.json`），**不进 COS** |
| 8 | `data/cache/models/{id}/*.glb` | **BlitzKit** `/tanks/{id}/{model,collision}.glb` | `web/assets.rs::ensure_glb_bytes`（运行期懒加载）+ `wargaming/model_fetch.rs`（CLI 批量） | COS `glb/` |
| 9 | `data/cache/tank_images/{id}.webp` | **BlitzKit** `/tanks/{id}/icons/big.webp` | `wargaming/blitzkit.rs::download_all_icons` | COS `tank_images/` |
| 10 | `data/cache/maps/`、`data/cache/terrain/` | **客户端解包** `3d/Maps/<space>/`（`.sc2`/`.scg`、landscape heightmap、colormap） | 生成：`tools/export_map_glb.py`（离线）；读取：`wargaming/map_assets.rs` | COS `map/` |
| 11 | `data/data_version.json` | **本机自产**（各项时间戳汇总；`game_version` 取自客户端 `Data/version.txt.dvpl`） | `wargaming/data_version.rs::save` | COS `data/data_version.json` |

来源分布：BlitzKit 4 项（#1/#2/#8/#9）、客户端解包 3 项（#4/#5/#10）、本机自产 3 项（#3/#7/#11）、混合 1 项（#6）。

附注：**WG API**（`api.wotblitz.{asia,eu,com}`，`wargaming/api_client.rs`）只用于玩家战绩，
不落 `data/`、不进任何分发。

### 1a. 消费方从 `tank_cache.json` 读的字段

资产包里的 `data/tank_cache.json` 是百科的**列表/概要**数据源。消费方当前读取：

| 字段 | 说明 |
|---|---|
| `name` / `tier` / `nation` / `type` / `hp` | 行展示与筛选、排序 |
| `is_premium` / `is_collector` | 车型标记。**两者是 `tanks.pb` field13 的同一枚举**（1=金币 `is_premium`、2=收藏 `is_collector`），故互斥——实测 735 辆：金币 91 / 收藏 338 / 同时为真 0。WotbTools 据此给卡片上不同颜色的边框（金币=警告色、收藏=`--color-info` 蓝） |
| `shells` | 弹种表（`penetration`/`damage`/…）。**取顶级炮塔 × 顶级主炮**（§3.5 口径），消费方由此派生 `pen_max` |
| `armor` | 六面装甲摘要（**顶级**炮塔档位，见 [game-data-sources.md](game-data-sources.md) §3.5） |
| `view_range` / `turret_traverse_speed` | 视野 / 炮塔转速，**顶级**炮塔 |
| `speed_forward` / `speed_reverse` / `hull_traverse` / `gun_depression` / `gun_elevation` | 机动与俯仰 |

> 派生字段（`pen_max`）由消费方自行聚合，**不在本仓生产**：前端 `tankopediaQuery.js`
> 取 `shells[].penetration` 的最大值。所以弹种取哪一档炮会直接影响列表卡片的穿深与排序。

## 二、分发渠道

共 **三条**，彼此独立：

### 2.1 COS 桶（数据面主力）

- 桶 `wotbtools-assets-1478073677`，地域 `ap-shanghai`
  （`wotbtools-assets-1478073677.cos.ap-shanghai.myqcloud.com`）
- 布局与 `release/asset_pack/` **逐项对应**，`manifest.json` 含全量 sha256
- 写入凭据由 `wotbtools-asset-publisher` 子账号持有（整桶读 + 写）
  —— **密钥不入库、不写进任何被跟踪的文件**，需要时通过环境变量传入
- 发布流程（差分上传 + 上传后回拉逐对象校验 + 回滚素材）见
  [game-data-sources.md](game-data-sources.md) §5.2

包内布局（本地实测：**4124 个文件 / 2932.5 MB**，`generated` = `2026-10-02T16:28:10Z`）：

| 包内路径 | 文件数 | 来自（见第一节编号） |
|---|---|---|
| `glb/{tank_id}/{model,collision}.glb` | 1470 | #8 |
| `tank/{tank_id}.json` | 735 | #6（由 `tank_data/` 改名） |
| `tank_images/{id}.webp` | 735 | #9 |
| `game_data/{id}.json` | 728 | #4 / #5 |
| `map/{key}/…` | 450 | #10（底图 / 小地图 / 地形 / 场景 / groundtex） |
| `data/` | 5 | #1 `tanks.pb`、#2 `models.pb`、#3 `tank_cache.json`、#11 `data_version.json`、打包器派生 `tank_names.json` |
| `index.json` | 1 | 打包器生成（地图 id → key/display） |
| `manifest.json` | — | 打包器生成（全量 sha256；本身不登记自己） |

打包器：[scripts/export_asset_pack.py](../scripts/export_asset_pack.py)。注意它不是纯拷贝，
中间有改名与派生：`tank_data/` → `tank/`；`tank_cache.json` 额外派生 `tank_names.json`；
地图地形多一个 `terrain.json` sidecar（把 `X-Terrain-Meta` 头物化）；`index.json` /
`manifest.json` 均为生成物。

> ⚠️ **盘上这份包早于 2026-10-03 的 field32 修复**：包内 `game_data` 仍为 728 个，
> 而源 `data/game_data/` 已是 735 个。重新发布前必须重跑打包器，否则会把修复前的
> 旧数据推给消费方。

**不进包的**：`data/cache/local_*`（见第三节）、`data/replay_samples/`、
`data/sessions/`、`data/snapshots/`、`data/token_usage.json`。

### 2.2 GitHub Release（引擎，非数据）

`wotb-replay-wasm-v<version>.zip` —— 由 [.github/workflows/release.yml](../.github/workflows/release.yml)
在 `[workspace.metadata.release].version` 变更合并到 `main` 时自动测试、构建、打 tag 并发布
（版本策略见 [AGENTS.md](../AGENTS.md)）。消费方按 **commit + sha256** 锁定，落到自己的
`/wasm/<40 位 commit>/` 目录；**不浮动跟随 upstream main**。

### 2.3 消费方前端构建（唯一绕过 COS 的数据项）

弹种反解表（#7）以 `shellKinds.json` 的形式随消费方前端构建走，运行期由前端
`import()` 后注入 WASM（`parseShotReplays` 的可选入参）。**它不经过 COS 桶**，
是第一节 11 项里唯一不在资产包内的。

## 三、已备好但未接入的替换来源

产物已在盘上、导出器已可用，但**运行期与打包器都还指向旧来源**：

| 目标（现役来源） | 已备好的替代 | 维护代码 | 落盘位置 | 阻塞项 |
|---|---|---|---|---|
| 坦克 GLB（BlitzKit CDN） | 客户端导出 | [tools/export_tank_glb.py](../tools/export_tank_glb.py) | `data/cache/local_models/`（1470 个 glb） | 几何 **733/735 逐字节等价**（含 UV0/1/2 与节点顺序；余 2 辆为 BlitzKit 侧行为）；贴图槽位完全对齐、`alphaMode` 99.1%、`doubleSided` 100%、MR.G 1010/1011（见 [local-model-export.md](local-model-export.md)）。剩余阻塞：**接线决策**（打包器源目录切换 + 来源混用护栏），非能力缺口 |
| 封面图（BlitzKit CDN） | 客户端导出 | [tools/export_tank_icons.py](../tools/export_tank_icons.py) | `data/cache/local_tank_icons/`（730 张） | 覆盖 **730/735**（差 5 辆，客户端无对应图标）；且打包器的 `cp` 对缺失文件静默跳过、不报错；⚠️ 与 BlitzKit 封面**不是同一幅画**（NCC 中位 0.21，见 [decoupling-status.md](decoupling-status.md) §3.4）——换素材需先过观感决策 |
| `models.pb` 的逐板装甲 | 客户端 XML | `wargaming/dvpl.rs::ArmorModel::parse_from_xml` | `data/game_data/{id}.json` 内已有 `armor_model`（含 plates / spaced / primary） | ✅ **2026-10-03 已对齐**：提取器改取 `<turrets0>` 的顶级炮塔 × 其末个主炮，与 models.pb 同档。全量核对板集差异 turret 434→0、hull 381→0（残留仅 float32 量化），摘要与 `armor_model` 不再矛盾 |
| （对照用途） | 本地导出 vs BlitzKit 逐辆比对 | [tools/compare_tank_glb.py](../tools/compare_tank_glb.py) | `data/cache/local_compare/` | — |

另外两个**跨领域的接线点**（不在上表，但换源必然撞上）：

- **打包器的源目录**：现读 `data/cache/models/` 与 `data/cache/tank_images/`，而本地导出器写
  `local_models/` / `local_tank_icons/`。两边内部布局与文件名完全一致，**只需改目录**——
  但漏改不会报错，只有产物悄悄沿用旧来源。
- **来源混用无护栏**：打包器只做拷贝 + sha256（传输完整性），**不做来源一致性校验**。
  同一辆车混用两源在 `tank_configs` 的 `turret_index`/`gun_index` 与 GLB 节点号上是
  **硬耦合**，会静默错位（见 [feasibility-glb-local-export.md](feasibility-glb-local-export.md) §8.4）。

## 四、尚无代码的缺口

以下在客户端有来源、但**仓库内没有任何解析器**：

- `tanks.pb` 承载的整车数值：弹种（穿深近/远、伤害、模块伤害、口径、转正、跳弹角、爆炸半径）、
  火炮（装填/瞄准/散布/弹鼓/连发/俯仰极限）、HP、机动、tier、本地化名、引擎/履带/无线电模块。
  客户端来源路径与逐字段映射**已在** [game-data-sources.md](game-data-sources.md) §2.2/§2.3 列出。
  **Python 侧已实现**：[tools/extract_vehicles.py](../tools/extract_vehicles.py)（2026-10-04，
  产出同构 JSON 至 `data/cache/local_pb/`，与 BlitzKit pb 逐字段对照**数值零不一致**，
  见 [decoupling-status.md](decoupling-status.md) §1）；Rust 运行期仍读 pb，**格式决策
  （写 pb 编码器保持零改动 vs 改读 JSON）未定**，故尚未接线。
- `models.pb` 的模型原点与俯仰/射界（客户端来源在 vehicle XML + `3d/Tanks/Parameters/*.yaml.dvpl`）。

两个非代码障碍：

- **`tank_id` ↔ 游戏模型名的桥**：游戏本地任何文本资源都不含此表（搜索范围见
  [game-data-sources.md](game-data-sources.md) §3），必须固化为仓库内映射表（735 条 JSON），
  或走 WG API + 名称模糊匹配（实测覆盖 89.9%，剩 74 辆需人工）。
- **格式决策**：本地提取的自然产物是 **JSON**，而运行期读的是 **protobuf**
  （`blitzkit.rs::parse_tanks_pb` / `parse_models_pb`）。要么写 pb 编码器保持运行期零改动，
  要么改运行期读 JSON（波及 `blitzkit.rs` + `tank_resolver` + `loadout` + `tank_configs`
  + `tank_cache` 派生链）。

## 五、接入状态一览

```
BlitzKit（4 项）  #1 tanks.pb ─┐
                 #2 models.pb ─┼─→ 运行期 blitzkit.rs（pb 解析）+ web/assets.rs（GLB 懒下载）
                 #8 GLB ───────┤
                 #9 封面图 ─────┘
                 派生下游：#3 tank_cache.json、#6 tank_data（一半）、#7 弹种反解表

客户端解包（3 项）#4/#5 game_data ─→ 运行期 game_extract（装甲摘要 / 碰撞盒）   ✅ 已接入
                 #10 maps/terrain ─→ 运行期 map_assets                        ✅ 已接入

本机自产（3 项）  #3 / #7 / #11                                              ✅ 已接入
混合（1 项）      #6 tank_data                                                ✅ 已接入

已备好未接入（2 项 + 1 特例）
                 local_models/ ─→ 目标替 #8                                   ⚠️ 有阻塞
                 local_tank_icons/ ─→ 目标替 #9                               ⚠️ 有阻塞
                 game_data 的 armor_model ─→ 目标替 #2 的装甲部分             ⚠️ 语义不一致
```

**一句话结论**：BlitzKit 的直接入口只有 4 个，但派生面很宽（3 个下游 + 1 个下载点）；
客户端侧"已接入"的只有装甲碰撞与地图两块，GLB / 封面图 / 逐板装甲三项是"产物已备好、
**尚未接线**"，而整车数值（`tanks.pb`）在客户端侧**完全没有解析代码**。
