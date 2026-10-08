# 数据面清单：使用中 / 未接入 / 分发

> 2026-10-03 整理，**2026-10-07 随资产面全量重导/重传刷新**（包统计与逐目录清单、地图
> 导出与俯视合成链路、上传工具的漏传陷阱、陈旧包警告解除；同日复核**坦克模型已换客户端
> 解包源**——`cache/models` ≡ `local_models` 735/735，§1 #8 与 §3 已按新状态改写）。回答
> 三个问题：**运行起来到底读了哪些数据**、**每份数据谁在维护、怎么送到消费方**、**哪些
> 替代来源已经备好但还没接线**。
>
> 姊妹篇：[game-data-sources.md](game-data-sources.md) 讲"每份数据从哪来、能否脱离
> BlitzKit"（含逐字段来源映射、spaced 规则、故障复盘）；本文讲"谁在用、谁维护、怎么分发"。
> 两份文档共享同一套来源口径，改动其一时须同步另一份。

## 一、正在使用的数据

运行期（Web / 3D 查看器 / 回放 / CLI）真正会读的共 **11 项**（每项背后的提取 / 维护工具见 §六）。

| # | 数据 | 来源 | 维护代码 | 分发去处 |
|---|---|---|---|---|
| 1 | `data/tanks.pb` | **BlitzKit** `/definitions/tanks.pb` | `wargaming/blitzkit.rs::fetch_and_save` | COS `data/tanks.pb` |
| 2 | `data/models.pb` | **BlitzKit** `/definitions/models.pb` | 同上 | COS `data/models.pb` |
| 3 | `data/tank_cache.json` | **本机自产**（由 `tanks.pb` 派生，间接 BlitzKit） | `update-data` | COS `data/tank_cache.json` + 派生 `data/tank_names.json` |
| 4 | `data/game_data/{id}.json`（装甲模型） | **客户端解包** `XML/item_defs/vehicles/{nation}/{model}.xml.dvpl` | `wargaming/dvpl.rs::ArmorModel::parse_from_xml` + `wargaming/game_extract.rs` | COS `game_data/` |
| 5 | `data/game_data/{id}.json`（碰撞盒） | **客户端解包** `3d/Tanks/Parameters/{nation}/{model}.yaml.dvpl` | `wargaming/dvpl.rs::CollisionData::parse_from_yaml` | COS `game_data/`（同一文件） |
| 6 | `data/tank_data/{id}.json` | **混合**：`models.pb`（BlitzKit）+ `game_data`（客户端） | `wargaming/tank_configs.rs::export_tank_data` | COS `tank/{id}.json`（改名） |
| 7 | 弹种反解表（全局 shell id → 弹种数据） | **本机自产**（`tanks.pb` 全量展开，离线生成） | `replay/loadout.rs::ShellKindTable::from_tanks_pb` + CLI `dump-shell-kinds` | **消费方前端常量**（`shellKinds.json`），**不进 COS** |
| 8 | `data/cache/models/{id}/*.glb` | **客户端解包**（`tools/export_tank_glb.py` 本机自产；**2026-10-07 已换源**，BlitzKit CDN 退为缺失兜底） | 生成：`tools/export_tank_glb.py`（离线）；兜底：`web/assets.rs::ensure_glb_bytes`（BlitzKit 下载，reqwest→curl 回退）+ `wargaming/model_fetch.rs`（CLI 批量） | COS `glb/` |
| 9 | `data/cache/tank_images/{id}.webp` | **BlitzKit** `/tanks/{id}/icons/big.webp` | `wargaming/blitzkit.rs::download_all_icons` | COS `tank_images/` |
| 10 | `data/cache/maps/`、`data/cache/terrain/` | **客户端解包** `3d/Maps/<space>/`（`.sc2`/`.scg`、landscape heightmap、colormap） | 生成：`tools/export_map_glb.py`（离线；场景 GLB + 地面原始烘焙）；地面 `ground.webp` 再经 `tools/composite_overhead.py` 叠俯视渲染合成（渲染由消费方仓 `frontend/scripts/bake-ground-overhead.mjs` 用真实 three.js 跑出，缓存 `release/overhead-bake/`）；读取：`wargaming/map_assets.rs` | COS `map/` |
| 10a | `data/cache/maps/{key}/destructibles.json`（可破坏物实例清单：位置 / 100m 格子 / 碰撞耐久 / 类型库联表） | **客户端解包** `3d/Maps/<space>/<space>.sc2`（StateSwitcher/SpeedTree + CollisionTypeComponent）+ `XML/destructibles.xml.dvpl`（类型库） | 生成：`tools/export_map_destructibles.py`（离线） | COS `map/{key}/destructibles.json`；消费方与回放切面 `destructible_events` 联表（逆向总集 §5.4） |
| 11 | `data/data_version.json` | **本机自产**（各项时间戳汇总；`game_version` 取自客户端 `Data/version.txt.dvpl`） | `wargaming/data_version.rs::save` | COS `data/data_version.json` |

来源分布：BlitzKit 3 项（#1/#2/#9）、客户端解包 4 项（#4/#5/#8/#10）、本机自产 3 项（#3/#7/#11）、混合 1 项（#6）。

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

包内布局（本地实测：**4168 个文件 / 3788.3 MB**）。`manifest.json` 的 `upstream_commit` /
`worktree_dirty` / `generated` 是包内容的溯源锚点——`worktree_dirty=false` 表示该 commit 的
**原样工作区**即可复现整包（语义见打包器 `git_provenance()`）；**现值以包内 manifest 为准，
每次重打包或改溯源戳后刷新本句**。2026-10-07 的历史脉络：资产重传时 = `90f1bf8` + dirty
（当时导出器修复与工具入库尚未提交）；同日随文档同步**仅改溯源戳**（资产字节不变）→
`upstream_commit` = 文档同步那次提交、`worktree_dirty` = false。

| 包内路径 | 文件数 | 来自（见第一节编号） |
|---|---|---|
| `glb/{tank_id}/{model,collision}.glb` | 1470 | #8 |
| `tank/{tank_id}.json` | 735 | #6（由 `tank_data/` 改名） |
| `tank_images/{id}.webp` | 735 | #9 |
| `game_data/{id}.json` | 735 | #4 / #5 |
| `map/{key}/…` | 486 | #10：36 场景 `scenery.glb` + 36 `ground.webp` + 234 分层 `ground/{cm,lm,tile0,tile1,mask0,mask1[,hmap0,hmap1]}.webp` + 36 `ground.layers.json` + 36 `terrain.json` + 36 `terrain.u16.bin` + 36 `mini.webp` + 36 `destructibles.json` |
| `data/` | 5 | #1 `tanks.pb`、#2 `models.pb`、#3 `tank_cache.json`、#11 `data_version.json`、打包器派生 `tank_names.json` |
| `index.json` | 1 | 打包器生成（地图 id → key/display） |
| `manifest.json` | — | 打包器生成（全量 sha256；本身不登记自己） |

打包器：[scripts/export_asset_pack.py](../scripts/export_asset_pack.py)。注意它不是纯拷贝，
中间有改名与派生：`tank_data/` → `tank/`；`tank_cache.json` 额外派生 `tank_names.json`；
地图地形多一个 `terrain.json` sidecar（把 `X-Terrain-Meta` 头物化）；`index.json` /
`manifest.json` 均为生成物。

**上传**：[tools/upload_asset_pack_cos.py](../tools/upload_asset_pack_cos.py)
（凭据只从环境变量 `COS_SECRET_ID` / `COS_SECRET_KEY` 读，不落盘）。远端同
`Content-Length` 即跳过——⚠️ **内容变了但字节数恰好相同的对象会被漏传**（2026-10-07 的
`map/lagoon/ground.webp` 即此例，需用 SDK 强制 `put_object` 覆盖）；`manifest.json` 最后
强传作完整性锚点；`.json` 走 `no-cache`、二进制走 `max-age=3600`。手工发布流程与注意点见
[game-data-sources.md](game-data-sources.md) §5.2。

⚠️ 上传工具会遍历包目录下**全部**文件：俯视烘焙的渲染中间产物（`<pack>/overhead/*.rgba`，
按图 64MB）必须在上传前移出，约定缓存在 `release/overhead-bake/`，否则会随包传上 COS
（2026-10-07 曾误传 73 个对象 / ~2.4GB，已清理）。

> ✔️ **盘上这份包与源一致**（2026-10-07 全量重导 + 重打包 + 上传 + 逐对象 sha256 回拉校验）：
> 包内 `game_data` 735 个、地图 36 张，与 `data/` 源目录同版；2026-10-03 记的 field32 陈旧包
> 警告已解除。判断包是否陈旧仍以 `manifest.json` 的逐文件 sha256 为准（打包器只做拷贝 +
> 哈希，**不做来源一致性校验**）。

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

## 三、替换来源：接线状态

**已接线：坦克 GLB（2026-10-07）。** `data/cache/models/` 已整体换为客户端解包的自产导出
（`tools/export_tank_glb.py` 产物：735 辆的 `model.glb` + `collision.glb`，**全量** generator =
`wotb-agent local sc2 exporter`），与 `data/cache/local_models/` 逐辆逐字节一致（735/735，
id 集差为空）；打包器与 COS 随 2026-10-07 的包一同发布。原"打包器源目录切换"阻塞项关闭；
BlitzKit CDN 下载（`web/assets.rs::ensure_glb_bytes` / `wargaming/model_fetch.rs`）退化为
**缺失兜底**。几何等价性口径与残留差异见 [local-model-export.md](local-model-export.md)；
⚠️ **来源混用护栏仍未加**——打包器依旧只做拷贝 + sha256，不校验来源一致性（见本节末两条）。

以下为**尚未接线**的其余替换来源：

| 目标（现役来源） | 已备好的替代 | 维护代码 | 落盘位置 | 阻塞项 |
|---|---|---|---|---|
| 封面图（BlitzKit CDN） | 客户端导出 | [tools/export_tank_icons.py](../tools/export_tank_icons.py) | `data/cache/local_tank_icons/`（730 张） | 覆盖 **730/735**（差 5 辆，客户端无对应图标）；且打包器的 `cp` 对缺失文件静默跳过、不报错；⚠️ 与 BlitzKit 封面**不是同一幅画**（NCC 中位 0.21，见 [decoupling-status.md](decoupling-status.md) §3.4）——换素材需先过观感决策；包内 735 张图标经抽样核验**仍为 BlitzKit 源** |
| `models.pb` 的逐板装甲（包内 `tank/{id}.json` 的 `armor_model`） | 客户端 XML | `wargaming/dvpl.rs::ArmorModel::parse_from_xml` | `data/game_data/{id}.json` 内已有 `armor_model`（含 plates / spaced / primary） | ✅ **2026-10-03 已对齐**：提取器改取 `<turrets0>` 的顶级炮塔 × 其末个主炮，与 models.pb 同档。全量核对板集差异 turret 434→0、hull 381→0（残留仅 float32 量化）。**运行期装甲摘要已走客户端**：`tank_resolver::extract_armor_summary` 优先读 `game_data/{id}.json` 的 `armor_model`，BlitzKit 仅缺失回退；**包内逐板 `armor_model` 未换源**——`tank_configs::synth_armor_model` 仍以 models.pb 为逐板/spaced/履带来源，`primary` 从 game_data 拷贝 |
| （对照用途） | 本地导出 vs BlitzKit 逐辆比对 | [tools/compare_tank_glb.py](../tools/compare_tank_glb.py) | `data/cache/local_compare/` | — |

另外两个**跨领域的接线点**（不在上表，但换源必然撞上）：

- **打包器的源目录**：现读 `data/cache/models/` 与 `data/cache/tank_images/`，而本地导出器写
  `local_models/` / `local_tank_icons/`。两边内部布局与文件名完全一致，**只需改目录**——
  但漏改不会报错，只有产物悄悄沿用旧来源。**模型一半已按此路径落地（2026-10-07：本地
  导出整体同步进 `cache/models`）**；封面图仍未切换（见上表）。
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
     #3 tank_cache.json、#6 tank_data（一半）、#7 弹种反解表

客户端解包（3 项）#4/#5 game_data ─→ 运行期 game_extract（装甲摘要 / 碰撞盒）   ✅ 已接入
                 #8 GLB（2026-10-07 换源）─→ cache/models ← local_models        ✅ 已接入
                 #10 maps/terrain ─→ 运行期 map_assets                        ✅ 已接入

本机自产（3 项）  #3 / #7 / #11                                              ✅ 已接入
混合（1 项）      #6 tank_data                                                ✅ 已接入

已接线（2026-10-07）
                 local_models/ ─→ #8 换源（cache/models ≡ local_models，735/735；包与 COS 已随发）

已备好未接入（2 项 + 1 特例）
                 local_tank_icons/ ─→ 目标替 #9                               ⚠️ 有阻塞（观感决策；包内仍 BlitzKit 源）
                 game_data 的 armor_model ─→ 目标替 #2 的装甲部分             ✅ 运行期摘要已用它；包内逐板仍 BlitzKit + primary 拷贝
```

**一句话结论**：BlitzKit 的直接入口现有 3 个（`tanks.pb` / `models.pb` / 封面图），但派生面很宽
（3 个下游 + 1 个兜底下载点）；客户端侧"已接入"的有装甲碰撞、地图、**坦克 GLB（2026-10-07 换源）**
与装甲摘要读源，封面图与包内逐板装甲尚未换源，而整车数值（`tanks.pb`）在客户端侧**完全没有
解析代码**。

## 六、本地提取工具索引

跑一次"从本机客户端取数"要动到哪些文件（2026-10-07 盘点）。入口默认指向 Steam 安装的
`Data/`，可用 `--game-data` / `--game-dir` 覆盖。

### 6.1 Python 提取链（`tools/`）

底座（所有 Python 导出的公共依赖）：

| 路径 | 作用 |
|---|---|
| `tools/wotbtools/wotb_sc2.py` | DAVA `SceneFileV2`(.sc2) + DVPL 解包（KeyedArchive 读取）。DVPL/LZ4 侧 fail-closed 口径与 Rust `dvpl.rs` 对齐，见 [game-data-sources.md](game-data-sources.md) §5.3；契约单测 `tools/test_dvpl_decode.py` |
| `tools/wotbtools/wotb_scg.py` | DAVA `SCPG`(.scg) 几何解码（顶点流 / 索引 / 图元） |

按提取目标的导出器（接线状态见 §1 / §3）：

| 提取目标 | 工具 | 产出落点 | 接线 |
|---|---|---|---|
| 地图场景（GLB + 地面 + 分层 + sidecar） | `tools/export_map_glb.py`（`--ground-only` / `--scenery-only` / `--jobs`） | `data/cache/maps/<space>.{glb,ground.*.webp,json}` | ✅ |
| 地图可破坏物清单（碰撞 / 耐久 / 类型库联表） | `tools/export_map_destructibles.py` | `data/cache/maps/<key>/destructibles.json` | ✅ |
| 坦克模型（`model.glb` + `collision.glb`） | `tools/export_tank_glb.py` | `data/cache/local_models/<id>/`（10-07 起同步进 `cache/models/`） | ✅ |
| 坦克封面图 / 图标 | `tools/export_tank_icons.py` | `data/cache/local_tank_icons/`（730/735） | ⚠️ 未接线 |
| 车辆数值（`tanks.pb` / `models.pb` 等价物） | `tools/extract_vehicles.py`（+ `data/tank_id_bridge.json`） | `data/cache/local_pb/` | ⚠️ 未接线 |
| 俯视地面合成 | `tools/composite_overhead.py`（现役）/ `tools/bake_ground_roofs.py`（旧软光栅，已退役） | 写回 `<pack>/map/<key>/ground.webp` | ✅ |

### 6.2 Rust 侧解包（`src/wargaming/`，由 CLI 子命令驱动）

| 模块 | 作用 | 驱动命令 |
|---|---|---|
| `dvpl.rs` | DVPL 解码（**fail-closed**：编码长度 / 存储载荷 CRC32 / 解压长度三项核对，不符即 `Err`，见 [game-data-sources.md](game-data-sources.md) §5.3）+ 装甲 XML / 碰撞 YAML 解析（`ArmorModel::parse_from_xml`、`CollisionData::parse_from_yaml`） | `parse-game`、`extract-game` |
| `game_extract.rs` | 批量提取 → `data/game_data/{id}.json`（含 `armor_model` / `collision`） | `extract-game --force`、`update-data` |
| `map_assets.rs` | 与客户端同链解析地图注册表，提取底图 / 地形 / 小地图 | `fetch-terrain`、`fetch-minimaps`（运行期 `/api/playback/map`） |
| `data_version.rs` | `data/data_version.json` 版本指纹（`game_version` 读 `Data/version.txt.dvpl`） | `update-data` |
| `model_fetch.rs` | GLB 全量预热（BlitzKit 兜底路径，已非主来源） | `fetch-models` |
| `tank_configs.rs` | 配置 / 装甲领域层（`synth_armor_model`：逐板装甲仍读 models.pb，`primary` 从 game_data 拷贝） | `dump-tank-data` |
| `tank_resolver.rs` | 装甲**摘要**（优先读客户端 `game_data` 的 `armor_model`，缺失回退 BlitzKit） | CLI 分析链 |

### 6.3 对照 / 验证（同目录，非导出源）

`tools/compare_tank_glb.py`（本地导出 vs BlitzKit 逐辆几何对照）、`tools/compare_tank_icons.py`
（封面图 NCC 对照）、`tools/compare_vehicle_data.py`（车辆数值逐字段对照）、
`tools/test_export_variants.py`（变体标签单测）、`tools/probe_switch.py`（.sc2 状态开关解剖）、
`tools/decode_experiment.py`（可破坏物候选物理解码实验）。

### 6.4 打包 / 分发 / 本机辅助

| 路径 | 作用 |
|---|---|
| `scripts/export_asset_pack.py` | 收拢上述产物为 `release/asset_pack/`（`index.json` / `manifest.json` + 逐文件 sha256） |
| `tools/upload_asset_pack_cos.py` | 资产包 → COS 差分上传（`manifest.json` 最后强传；陷阱见 §2.1） |
| `scripts/serve_asset_pack.mjs` | 本机 CORS 伺服 `release/asset_pack`（消费方 dev server 取用） |
| `scripts/build-wasm.ps1` | WASM 回放解析产物构建（引擎侧，与资产提取无关） |

### 6.5 不在本仓 / 未跟踪

- **俯视渲染器**：消费方仓 `WotbTools/frontend/scripts/bake-ground-overhead.mjs`（headless
  Chrome + three.js，产物 `.rgba` 交 `tools/composite_overhead.py` 合成）。
- **逆向探针**：`examples/*.rs`（**18 个**：`destructible_probe*` / `filter_*_probe` /
  `m29_*_probe` / `p1_comp_probe` / `dump_dvpl` 等），`.gitignore` 覆盖、仅本机存在；
  `cargo test` 仍编译本地副本，但**不入库**。
- **本机构建产物**：`frontend/public/wasm/`（`scripts/build-wasm.ps1` 产物，
  **随包不随库**）——`.gitignore` 覆盖。

**一句话记法**：地图 / 坦克 / 图标 / 车辆数值在 `tools/`（Python，底座是 `tools/wotbtools/`
解 DAVA 容器）；装甲 / 碰撞 / 底图在 `src/wargaming/`（Rust，CLI 驱动）；
`scripts/export_asset_pack.py` 收口，`tools/upload_asset_pack_cos.py` 发 COS。
