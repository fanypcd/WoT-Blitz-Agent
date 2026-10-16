# 数据面清单：运行所需数据总账（来源 · 可替代性 · 完成度）

> 逐项回答运行期每份数据的**目前来源**、**是否存在可替代来源**、**替代完成度**；
> 另附分发渠道、包内布局、跨领域接线护栏与本地提取工具索引。
>
> 替代完成度刻度：`✅ 已接线`（本机解包即现役来源）· `🟢 数据就绪未接线` ·
> `🟡 有缺口` · `⚪ 阻塞`（需决策）· `➖ 无需替代`。
>
> 姊妹篇：[game-data-sources.md](game-data-sources.md) 讲"每份数据从哪来、能否脱离
> BlitzKit"（逐字段来源映射、spaced 规则、DVPL 契约、COS 发布流程与陷阱）；本文讲
> "谁在用、谁维护、怎么分发"。两份文档共享同一套来源口径，改动其一时须同步另一份。

## 一、运行期核心数据（11 项）

运行期（Web / 3D 查看器 / 回放 / CLI）真正会读的 11 项。数据层 = 本机解包产物覆盖度与
对照结论；接线层 = 代码与分发链的切换状态。提取 / 维护工具索引见 §六。

| # | 数据 | 目前来源 | 可替代来源 | 替代完成度 | 剩余工作 / 备注 |
|---|---|---|---|---|---|
| 1 | `data/tanks.pb`（整车数值：弹种/火炮/HP/机动/模块） | **本机客户端解包编码（现役）**：`tools/extract_vehicles.py` → `tools/emit_vehicle_pb.py` 编码**同格式 pb**（+补充表 `local_pb_supplement.json` 沿用 2 条 BK 自建名） | 它本身就是；BlitzKit CDN 降为对照校验源（`fetch-blitzkit` 保留） | ✅ 数据/接线 100% | 与 BK 全量对照 = **122 处且全部定性**：119 枪序研发序口径 + 1 处 `10625` explosion_radius（**BK 错值**）+ 2 条名字（客户端两源皆无，补表兜底）。换源后 `armor_model.gun` × `game_data` 矛盾 15→0；`tank/{id}.json` 字节级差异 118 辆 = 18 辆实体（17 辆顶级炮板集/族修正 + 10625）+ 100 辆仅 ±0.0 符号零（IEEE 等值，无行为影响） |
| 2 | `data/models.pb`（逐板装甲/spaced/履带厚/模型原点/包围盒/限位） | 同 #1（同批换源，编码器同） | 它本身就是 | ✅ 数据/接线 100% | 按**游戏研发序**选档（BK 为模块 id 升序：105 辆枪序/92 辆末位主炮不同，其中 15 辆板集与 game_data 实质矛盾，换源即修复）；`initial_turret_rotation` 客户端直源逐值一致；43 辆无炮塔 TD 的炮塔 bbox 为**空心占位**（两侧皆空，见 [game-data-sources.md](game-data-sources.md) §2.3）；履带厚度/原点已改**逐底盘 map**（C1 塌缩修复，消费端按顶级底盘语义选取，见 [decoupling-status.md](decoupling-status.md) §2） |
| 3 | `data/tank_cache.json` | 本机自产（派生自现役本机 pb；俯仰 ← models.pb；六面装甲摘要 ← game_data 客户端优先） | 同一派生链 | ✅ 随 ①② | 消费方读取字段清单见 §1a |
| 4/5 | `data/game_data/{id}.json`（装甲模型 + 碰撞盒） | **客户端解包**：`XML/item_defs/vehicles/{nation}/{model}.xml.dvpl` + `3d/Tanks/Parameters/{nation}/{model}.yaml.dvpl` | 它本身就是 | ✅ 100/100 | 链内 2 处 pb 依赖：文件定位用 pb 的 `model_name`（field32）；炮塔/炮管碰撞盒**选段**用 models.pb 的 module→node 映射（缺失回退"末位"） |
| 6 | `data/tank_data/{id}.json`（→ 包 `tank/{id}.json`） | **本机 pb（models/tanks）+ game_data**——全链客户端解包 | 它本身就是 | ✅ 数据/接线 100% | 顶层 `shells` **直接引用末档配置弹表的同一份 JSON**（结构性单源化，字段集天然同步）；`configs[]` 节点数/索引走客户端 GLB ✅；43 辆无炮塔 TD 的 `collision_boxes.turret` = null（两侧都只有空心占位，非缺数据） |
| 7 | 弹种反解表（全局 shell id → 弹种数据；包 `data/shellKinds.json`） | 本机自产（`ShellKindTable::from_tanks_pb`；CLI `dump-shell-kinds --out data/shell_kinds.json`，输入为现役 pb 且**必须与 pb 同批重导**——打包器 fail-closed） | 客户端 `shells.xml <kind>`（4 值闭集，提取器与编码器均已支持） | ✅ 数据/接线 100%（消费方运行期直读，原内嵌件已退役） | `atgm_heat`→HOLLOW_CHARGE 验证一致；坍缩按 tank_id 升序取首个（确定性） |
| 8 | `data/cache/models/{id}/*.glb` | **客户端解包（现役）**；BlitzKit CDN 退为缺失兜底（`web/assets.rs::ensure_glb_bytes` / `wargaming/model_fetch.rs`，休眠） | 它本身就是 | ✅ 100/100 | 738 辆（glb/ 1476 件）；与 BlitzKit 逐字节一致在 **23 辆上刻意打破**（静态变换烘焙 / StateSwitcher 状态过滤，见 [local-model-export.md](local-model-export.md) §5.1）；11.21.0 另按新客户端重导 3 辆新车与 2 辆碰撞变更（24945/8225） |
| 9 | `data/cache/tank_images/{id}.webp` + `crops.json` | **本机客户端解包**：`tools/export_tank_icons.py`（DVPL 剥壳裸 webp、不重编码；逐车 `iconPath` 声明 738/738 命中）→ `local_tank_icons/` → 同步进 `cache/tank_images/`（两目录逐字节一致）；BlitzKit 退为缺失兜底（休眠） | 它本身就是 | ✅ 数据/接线 100% | 与 BK 同源同画：alpha 轮廓 100% 一致（BK 为二次重编码 + 更紧 1px 裁剪，RGB 均差 1.79 / max ≤17）；**展示裁剪**：`--emit-crops` 输出逐车内容框（alpha>0）→ 包 `tank_images/crops.json`（打包器 fail-closed），消费方展示层按框缩放（不改图标字节） |
| 10 | `data/cache/maps/`、`data/cache/terrain/` | **客户端解包** `.sc2`/`.scg`、heightmap、colormap；地面再经 `tools/composite_overhead.py` 叠俯视合成（渲染件 `release/overhead-bake/` 须与线上同代，见 [game-data-sources.md](game-data-sources.md) §5.5） | 它本身就是 | ✅ 100/100 | 高度图缓存 `{key}.hm.c2.u16.bin`（terrainLayout 2 布局自描述）；小地图按 space **注册表首条目**选取；11.21.0 随版更新 mountain 高度图/小地图，36 图 scenery 全量复导对齐 |
| 10b | 2D 战术底图 canonical（29 张 1254² WebP → WotbTools `frontend/src/assets/maps/`） | **本仓自产**：包内 `map/<key>/ground.webp`（4096²）→ `tools/export_2d_basemaps.py`（冻结配方：对比 ×1.17 / 饱和 ×0.9748 / USM B，全 29 张同一套） | 不适用（既非客户端资产也非 BK；旧批曾用外部 AI 增强件） | ✅ 29/29（`--verify` 全量重算；与用户验收版逐字节一致） | 合计 **21.50 MiB**（q90 m6）；**Web 与 APK 共用这一份字节**（Android 不再派生，2026-10-16 起）；取向由契约确定、无需翻转；**COS 免发版通道**（包内 `map2d/` + `index.json`，逐图 sha256 判据 + 在线校验 + 离线回落包内）见 [game-data-sources.md](game-data-sources.md) §5.6 |
| 10a | `data/cache/maps/{key}/destructibles.json`（可破坏物清单） | **客户端解包** `.sc2` + `XML/destructibles.xml.dvpl` | 它本身就是 | ✅ 100/100 | 消费方与回放切面 `destructible_events` 联表（总集 §5.4）；**55 键**（36 基础 + 19 出生点变体；打包器按 space 只物化首个 key）；共享类型库 `typeTableSize` 随客户端走（11.21.0 = 941） |
| 11 | `data/data_version.json` | 本机自产（`game_version` 取客户端 `Data/version.txt.dvpl`；`tank_count` / `game_data_files`） | 它本身就是 | ✅ 100/100 | 现值 `release/11.21.0` / 738；时间戳仅 `tank_cache_updated_at` / `game_data_updated_at` |

来源分布（**全量客户端化后**）：**11 项全部客户端解包/本机自产**；BlitzKit 仅保留
"交叉校验对象"角色（`fetch-blitzkit`）与两处缺失兜底（GLB `web/assets.rs:113`、
图标 `web/assets.rs:233`，均休眠）。

**弹种权威口径**：`tanks.pb` field9（客户端 `shells.xml <kind>` 语义枚举的翻译，
0=AP / 1=APCR / 2=HEAT / 3=HE）是弹种判定权威，field7 的 `icon` 词表降为回退——
#3 的 `shells[].shell_type_id`、#6 的 `tank/{id}.json` `type_id` 均透出该 id；
运行期 `PenetrationRequest` additive `shell_type_id`，`resolve_shell_type` 三级优先
（id → icon 词表 → HE）。

**枪序与弹药列表口径**：顶塔主炮列表按**客户端 XML 文档序 = 游戏研发序**（末位 =
顶级主炮）导出——BK 的 models.pb 为模块 id 升序，不成立。车辆的弹药列表以**共享炮定义**
（`components/guns.xml` 的 `<shots>`，弹道与穿深只在那里）为权威，车辆内联 `<shots>`
是价格/可用性叠加，按共享列表迭代、内联同标签条目合并覆盖。

### 1a. 消费方从 `tank_cache.json` 读的字段

资产包里的 `data/tank_cache.json` 是百科的**列表/概要**数据源。消费方当前读取：

| 字段 | 说明 |
|---|---|
| `name` / `tier` / `nation` / `type` / `hp` | 行展示与筛选、排序 |
| `is_premium` / `is_collector` | 车型标记。**两者是 `tanks.pb` field13 的同一枚举**（1=金币 `is_premium`、2=收藏 `is_collector`），故互斥——实测无"同时为真"。WotbTools 据此给卡片上不同颜色边框 |
| `shells` | 弹种表（`penetration`/`damage`/…）。**取顶级炮塔 × 顶级主炮**（枪序与弹药列表口径见 [game-data-sources.md](game-data-sources.md) §2.3），消费方由此派生 `pen_max` |
| `shells[].shell_type_id` | 弹种权威 id：tanks.pb field9 翻译，1=APCR / 2=HEAT / 3=HE；空值仅出现在 AP 系（proto3 零值省略 → 按 0=AP 或 icon 回退，语义一致）。判定弹种（跳弹角/溅射分支）以此为准，icon 串仅作显示 |
| `shells[].caliber` / `normalization` / `ricochet` | **等效厚度判定输入**：弹径（mm）/ 转正角（°）/ 跳弹临界角（°），2079/2079 全量有值。装甲查看器按弹算 `eff = t / cos(angle − 转正)`，两倍口径规则还会放大转正角（`1.4×norm×caliber/(2t)`）——**缺失即按 0° 兜底 = 判定被静默削弱**。包内 `tank/{id}.json` 顶层弹表与 `/api/shells` 同步同字段（[game-data-sources.md](game-data-sources.md) §2.3）；顶层表与 `configs[]` 同一份 JSON，字段集结构上不可能再漂移 |
| `armor` | 六面装甲摘要（**顶级**炮塔档位，见 [game-data-sources.md](game-data-sources.md) §2.3） |
| `view_range` / `turret_traverse_speed` | 视野 / 炮塔转速，**顶级**炮塔 |
| `speed_forward` / `speed_reverse` / `hull_traverse` / `gun_depression` / `gun_elevation` | 机动与俯仰 |

> 派生字段（`pen_max`）由消费方自行聚合，**不在本仓生产**：前端 `tankopediaQuery.js`
> 取 `shells[].penetration` 的最大值。所以弹种取哪一档炮会直接影响列表卡片的穿深与排序。

### 1b. 派生 / 注入 / 支撑数据

不是独立来源，而是由 §一 各项再加工、或注入消费方的产物——**间接依赖最容易漏记**：

| 数据 | 目前来源 | 可替代来源 | 替代完成度 | 剩余工作 / 备注 |
|---|---|---|---|---|
| `data/local_pb_supplement.json`（编码器取数） | 客户端无对应概念/数据、沿用既有 BlitzKit 值的字段（集中登记，不假装它们是客户端数据） | 逐项消除：`dev_name` 客户端无此概念——**保留为提取链匹配键**（`game_extract.rs::resolve_vehicle_file` 按它解析客户端车辆 XML/YAML；删掉会失配/误配）；名字仅剩 **2 条**（1 枪 + 1 履带，两种字符串源都没有） | ✅ 编码器消费（dev_name 738 / 名字 2） | 由 `tools/emit_vehicle_pb.py --emit-supplement` 生成，随游戏版本人工维护 |
| 包 `data/tank_names.json` ＋ WASM 注入表 `tankNamesJson` | 打包器从 tank_cache 抽（现为本机 pb 携带的客户端名） | 客户端名：随包基础快照 ∪ **运行时本地化覆盖层**（两源合并后仅差 2 条） | ✅ 已接线 | 覆盖层为 per-user 运行时缓存（需客户端登录同步过）；2 条残差由补充表兜底（[decoupling-status.md](decoupling-status.md) §2 C2） |
| WASM 注入表 `limitsJson`（炮管俯仰锚定） | 包 `tank/{id}.json` 的 `pitch_limits` ← 现役 models.pb | `local_pb` 的 `pitch_limits`（零差异） | ✅ 已接线 | 接线随 #2 |
| WASM 注入表 `shellsJson`（弹种徽标/判定） | 包 `data/shellKinds.json`（消费方运行期直读） | 客户端 `shells.xml <kind>` | ✅ 已接线 | 接线随 #7 |
| `data/tank_id_bridge.json`（738 条 tank_id ↔ 模型名） | 现表由 `tanks.pb` 导出（`--emit-bridge`） | **纯客户端可重建（735/735 一致）**：`tank_id = (list.xml <id> << 8) | 国家基数`（`--emit-bridge --from-client`）；客户端另有 **20 辆** BK 没有的车（教程 bot/超测）可顺带覆盖 | ✅ 能力已验证（`slug` 列无消费者） | 接线时改用客户端源即可自动覆盖新车 |
| `data/armor_states.json`（**6 辆**动态装甲车的状态声明，12 态） | 客户端 item_defs `vehicles/*/<stem>.xml` 的 `<extras><armorsStatesController>`（`tools/extract_armor_states.py` 直出） | 不适用（客户端独有概念，BlitzKit 无对应） | ✅ 已接线（解析器按它门控状态语义；消费方查"弹着时刻激活板集"同源） | 每态 `armors` = 激活的 `<part>.<armor_N>` 集合；初态 = `<default/>` 标记的 state；状态流载体 = **propID 16 `armorsStates`**（`[u8 n][n×4B]`，bit16=激活态；[总集 §2.5](回放与射击逆向总集.md)） |
| 包内 `data/tanks.pb` / `data/models.pb` | **本机客户端解包编码**：`extract_vehicles` + `emit_vehicle_pb`（+补充表） | 可选：从包内移除（改由消费方自行派生） | ✅ 已接线 | 移除会改变包结构——消费方若直读 pb 需同步 |
| 运行期 web 端点（`/api/tanks` 花名册、`/api/tank_filter`、`tank_detail.is_collector`、`/api/shells`、`models_status`） | 运行期**直读 pb**（花名册 / 名 / tier / nation / type / collector / 弹种 / models.pb id 表） | 改读 tank_cache / 本地数据 | 🟢 代码改动小 | 仅本机 web GUI 用（前端已冻结，不进分发） |
| 坦克悬挂数据（包 `suspension/<id>.json`） | 客户端逐车 yaml `suspension:` 块（`tools/export_tank_suspension.py`） | 不适用 | ✅ 已接线（前端求解器消费：逐轮贴地 + 履带形变 + 自转/花纹滚动） | 728 文件 / 2271 KB；字段 = 逐轮 `{flag,a,b}` + 履带静止折线 + 弯曲/铺放系数 + `chassis.textureScale`；逐轮半径/挂点由前端从模型 GLB 现算；剩余未定项（T4）见 [tank-suspension-client-re.md](tank-suspension-client-re.md) |
| 履带印数据（包 `tread/`） | 客户端逐车 yaml `treadParameters` + 13 张 `*tread*.dds` 贴图（`tools/export_tank_tread.py`） | 不适用 | ✅ 已接线（WotbTools `scene/treadMarks.js` 几何贴花） | 738 json（314 authored + 424 国别主贴图回落）+ 13 贴图 + `textures.json` alpha 归一化表 / 156 KB + 983 KB；见 [feasibility-track-marks.md](feasibility-track-marks.md) |
| 解包底座（DVPL/DAVA 容器、DLC `packs/` 覆盖层） | 本机自产代码（Rust `dvpl.rs` + Python `tools/wotbtools/`，fail-closed 两端对齐） | 不适用 | ✅ 100/100 | 本地链硬前提：需本机装客户端（`Data/` + `packs/`），游戏版本更新后整链重跑 |

引擎产物（WASM Release，`wotb-replay-wasm-*.zip`）构建自 `crates/replay-core|wasm`，**不含
任何 pb**；上表三张注入表（车型名 / 俯仰锚定 / 弹种表）是消费方的间接数据面——它们的
输出（`tank_name`、`gun_pitch` 解码、`shell_kind`/`shell`）都随注入数据来源变化。

### 1c. 非数据类外部依赖

| 依赖 | 用途 | 可替代性 |
|---|---|---|
| WG API（`api.wotblitz.{asia,eu,com}`，`wargaming/api_client.rs`） | 玩家战绩 | ➖ 服务器数据，**无本地替代也不需要**；不落盘、不进分发 |
| 本机客户端安装（Steam `Data/` + `packs/`） | 所有本地解包链的输入前提 | ➖ 必须（`--game-data` / `--game-dir` 可覆盖路径） |
| BlitzKit 作为交叉校验对象 | 定期用本地提取结果与其 pb 做 diff，漂移即告警 | ➖ 保留该角色 |

## 二、分发渠道

共 **两条**，彼此独立：COS 资产包（数据面主力）与 GitHub Release（WASM 引擎）。

### 2.1 COS 桶（数据面主力）

- 桶 `wotbtools-assets-1478073677`，地域 `ap-shanghai`
  （`wotbtools-assets-1478073677.cos.ap-shanghai.myqcloud.com`）
- 布局与 `release/asset_pack/` **逐项对应**，`manifest.json` 含全量逐文件 sha256
  （在合成之后、上传之前用 `--refresh-manifest` 重算，见下）
- 写入凭据由 `wotbtools-asset-publisher` 子账号持有（整桶读 + 写）
  —— **密钥不入库、不写进任何被跟踪的文件**，需要时通过环境变量传入
- 发布流程（差分上传 + 上传后回拉逐对象校验 + 回滚素材）见
  [game-data-sources.md](game-data-sources.md) §5.2

**当前包内布局**（本地实测；**5766 个文件 / 4200.3 MB** 为本地口径，远端仍为 5736 / 4177.7——`map2d/`（30 件 / 22.0 MB）尚未上传，见 §5.6）：

| 包内路径 | 文件数 | 来自（见第一节编号） |
|---|---|---|
| `glb/{tank_id}/{model,collision}.glb` | 1476 | #8（738 辆 × 2） |
| `tank/{tank_id}.json` | 738 | #6（由 `tank_data/` 改名） |
| `tank_images/{id}.webp` + `crops.json` | 739 | #9（展示裁剪数据面） |
| `game_data/{id}.json` | 738 | #4 / #5 |
| `map/{key}/…` | 486 | #10：36 场景 `scenery.glb` + 36 `ground.webp` + 234 分层 `ground/{cm,lm,tile0,tile1,mask0,mask1[,hmap0,hmap1]}.webp` + 36 `ground.layers.json` + 36 `terrain.json` + 36 `terrain.u16.bin` + 36 `mini.webp` + 36 `destructibles.json` |
| `data/` | 6 | #1 `tanks.pb`、#2 `models.pb`、#3 `tank_cache.json`、#11 `data_version.json`、打包器派生 `tank_names.json`、#7 `shellKinds.json` |
| `map/{key}/lighting.json` | 36 | #10 配套：`tools/export_map_lighting.py` 逐图太阳/环境色/雾/IBL |
| `map/{key}/ibl.webp` | 36 | #10 配套：`tools/export_map_ibl.py` 逐图环境反射（等距柱状，~28KB/图） |
| `suspension/{id}.json` | 728 | §1b 悬挂数据 |
| `tread/{id}.json` + `tread/textures/*.webp` + `tread/textures.json` | 752 | §1b 履带印数据（738 + 13 + 1） |
| `map2d/<code>.webp` + `map2d/index.json` | 30 | §10b：`tools/export_2d_basemaps.py` 写（**非打包器产物**，重打包后须重跑；COS 免发版通道） |
| `index.json` | 1 | 打包器生成（地图 id → key/display） |
| `manifest.json` | — | 打包器生成（全量 sha256；本身不登记自己） |

打包器：[scripts/export_asset_pack.py](../scripts/export_asset_pack.py)。注意它不是纯拷贝，
中间有改名与派生：`tank_data/` → `tank/`；`tank_cache.json` 额外派生 `tank_names.json`；
地图地形多一个 `terrain.json` sidecar（把 `X-Terrain-Meta` 头物化，含 `terrainLayout: 2`
布局标记与 `terrainBinSha256` 配对锚点——高度图列口径与提取链见
[game-data-sources.md](game-data-sources.md) "地形高度图布局与提取链"）；
`data/shellKinds.json` 由 `data/shell_kinds.json` 物化（缺源 fail-closed）；`tank_images/crops.json`
由 `data/cache/tank_icon_crops.json` 物化（缺源或数量与图标失配即拒绝打包）。

**发布链（固定次序）**：
`export_asset_pack.py`（重打包）→ **俯视合成**（`composite_overhead.py --all --write`，
渲染件须与线上同代、先归档——[game-data-sources.md](game-data-sources.md) §5.5）→
**2D 底图导出**（`tools/export_2d_basemaps.py`，写 canonical staging 与包内 `map2d/`——
打包器会 `rmtree` 重建整个包，这一步必须在重打包之后，见 [game-data-sources.md](game-data-sources.md) §5.6）→
**`export_asset_pack.py --refresh-manifest`**（按包内现有内容重算全部条目——合成器在打包
之后改写 `ground.webp`，不重算则 manifest 的 36 项与包内件不一致）→
`tools/upload_asset_pack_cos.py`（差分上传 + 回拉校验）。

**上传工具**：[tools/upload_asset_pack_cos.py](../tools/upload_asset_pack_cos.py)
（凭据只从环境变量 `COS_SECRET_ID` / `COS_SECRET_KEY` 读，不落盘）。要点：

- **跳过判定 = 桶内 `manifest.json` 的逐文件 sha256**：内容变了就必须传，**哪怕尺寸相同**
  （旧实现只比 `Content-Length`，会把这类修正静默跳过——"同尺寸不同内容"必须按 sha256 判定）。
  远端 manifest 缺该条目（首传/旧对象）时回退尺寸比对；取不到远端 manifest 则整体回退并告警，
  此时上传后回拉校验是唯一兜底。
- **`--only <prefix>` = 局部发布**：只传匹配前缀的文件，manifest 改为**远端清单就地补丁**、
  只登记本次上传的条目（多会话共用一个包目录时不会把他人在飞的改动宣布为线上内容）。
- `manifest.json` 最后强传作完整性锚点；`.json` 走 `no-cache`、二进制走 `max-age=3600`。
- ⚠️ **上传会遍历包目录下全部文件**：俯视烘焙的渲染中间产物 `<pack>/overhead/*.rgba`
  （按图 64MB）必须在上传前移出——约定缓存在 `release/overhead-bake/`，合成器可直接
  `--render-dir release/overhead-bake` 读缓存；归档助手 `tools/archive_overhead_bake.py`
  的 `--clear-pack` 负责清空包内件。
- ⚠️ **凭据字节要干净**：Windows 下经管道/命令替换传 `COS_SECRET_*` 时注意尾部 `\r`
  （多 1 字节即全部 `SignatureDoesNotMatch`；上传阶段全失败先核对凭据长度）。
- ⚠️ **"客户端仍显示旧图"先查缓存窗口**：二进制资产带 `max-age=3600`，浏览器在新鲜期内
  不回源（App 端 Native 代理每次带 `If-None-Match` 条件重取，换内容即时生效）。
- `manifest.json` 的 `upstream_commit` / `worktree_dirty` / `generated` 是包内容的溯源锚点；
  `worktree_dirty=true` 表示该 commit 的原样工作区**不能**复现整包（语义见打包器
  `git_provenance()`）。**现值以包内 `manifest.json` 为准**。

**不进包的**：`data/cache/local_*`（见第三节）、`data/replay_samples/`、
`data/sessions/`、`data/snapshots/`、`data/token_usage.json`。

### 2.2 GitHub Release（引擎，非数据）

`wotb-replay-wasm-v<version>.zip` —— 由 [.github/workflows/release.yml](../.github/workflows/release.yml)
在 `[workspace.metadata.release].version` 变更合并到 `main` 时自动测试、构建、打 tag 并发布
（版本策略见 [AGENTS.md](../AGENTS.md)；现值 **v0.4.7**）。消费方按 **commit + sha256** 锁定，
落到自己的 `/wasm/<40 位 commit>/` 目录；**不浮动跟随 upstream main**。

## 三、跨领域接线点与护栏

逐项替代完成度见 §一 / §1b；本节只留**不属于任何单项、换源必然撞上**的接线点与护栏：

- **打包器的源目录**：现读 `data/cache/models/` 与 `data/cache/tank_images/`，而本地导出器写
  `local_models/` / `local_tank_icons/`。两边内部布局与文件名完全一致，**只需改目录**——
  但漏改不会报错，只有产物悄悄沿用旧来源。两对目录现均已同步并逐字节一致，
  重导后**必须同步**（`cache/models ≡ local_models`、`cache/tank_images ≡ local_tank_icons`）。
- **来源混用无护栏**：打包器只做拷贝 + sha256（传输完整性），**不做来源一致性校验**。同一辆
  车混用两源在 `tank_configs` 的 `turret_index`/`gun_index` 与 GLB 节点号上是**硬耦合**，会
  静默错位（见 [feasibility-glb-local-export.md](feasibility-glb-local-export.md) §8.4）——
  现实教训：models.pb 枪序与客户端研发序不一致曾致包内 15 辆 `armor_model.gun` 与 `game_data`
  矛盾（换源后为 0）。"拷贝 + 哈希"管不住时序与来源，属同类缺口。

## 四、剩余缺口与依赖注记

- **无阻塞性数据缺口**：运行期 11+1 项全部已接线（§五）。以下为依赖注记：
- 名字表残差 **2 条**（1 枪 + 1 履带，客户端两种字符串源都没有；BK 自建名）→ 补充表兜底；
  运行时本地化覆盖层是 per-user 缓存（新装/未同步时会缺），提取链对缺名保持软失败 + 回退。
- 封面图判定已降级为抽查：与 BK 封面同源同画（换源接近视觉无损），对照渲染件在
  `data/cache/local_compare/`。
- 坦克悬挂：机制已还原、数据已导出、前端已接线；**T4 对齐分级待再 RE**
  （见 [tank-suspension-client-re.md](tank-suspension-client-re.md)）。
- 履带印：数据面已导出并接线（前端几何贴花）；单场按需取数（≤14 json + ≤9 贴图）。
- 俯视合成链的世代硬约束（渲染缓存与线上底图同代、先归档再合成）见 §2.1 与
  [game-data-sources.md](game-data-sources.md) §5.5。

## 五、完成度汇总

```
按完成度（§一 / §1b 逐条；共 12 条 = §一 11 项含 10a + §1b 主要派生项另列）：

  ✅ 已接线（本机解包即现役来源）
       #1 整车数值 · #2 装甲/原点/限位 · #3 tank_cache · #6 tank/{id}.json · #7 弹种表/kind
       #4/#5 game_data（装甲 + 碰撞，含 primary）        #8 GLB（738/738）
       #9 封面图（客户端剥壳字节，738/738）              #10/#10a maps/terrain/destructibles
       #11 data_version · 解包底座
       limitsJson · shellsJson · tankNamesJson · 补充表（dev_name 738 + BK 自建名 2 条）
       悬挂（728）· 履带印（738 + 13 贴图）

  ⚪ 无缺口：BK 侧 122 处差异已全部定性（119 枪序＝口径、1 处 explosion_radius＝BK 错值
     已随换源修复、2 条名字＝补表兜底）；桥表可纯客户端重建（新车自动覆盖）

  ➖ 无需替代      WG API（战绩） · tank_id 命名空间（只能固化桥表） · BlitzKit 交叉校验角色
```

**一句话结论**：运行期 11+1 个数据条目**全部**已由本机解包/自产承接——含
`tanks.pb` / `models.pb` 及其全部派生面（tank_cache / tank/{id}.json / shellKinds）
与封面图（客户端剥壳字节）。外部依赖：**BlitzKit 仅剩"交叉校验对象"角色**
（`fetch-blitzkit` 保留）+ 两处缺失兜底（GLB `web/assets.rs:113`、图标 `web/assets.rs:233`，
均休眠）+ WG 战绩 API（性质不同，非客户端可解）。引擎产物（WASM Release）不含任何 pb。

## 六、本地提取工具索引

跑一次"从本机客户端取数"要动到哪些文件。入口默认指向 Steam 安装的
`Data/`，可用 `--game-data` / `--game-dir` 覆盖。

### 6.1 Python 提取链（`tools/`）

底座（所有 Python 导出的公共依赖）：

| 路径 | 作用 |
|---|---|
| `tools/wotbtools/wotb_sc2.py` | DAVA `SceneFileV2`(.sc2) + DVPL 解包（KeyedArchive 读取）。DVPL/LZ4 侧 fail-closed 口径与 Rust `dvpl.rs` 对齐，见 [game-data-sources.md](game-data-sources.md) §5.3；契约单测 `tools/test_dvpl_decode.py` |
| `tools/wotbtools/wotb_scg.py` | DAVA `SCPG`(.scg) 几何解码（顶点流 / 索引 / 图元）。顶点位偏移与 stride 按 DAVA `RenderBase.h` 权威位表（流序 = EVF 位号序），43 种实测掩码零失配 |
| `tools/wotbtools/dlc_packs.py` | **DLC 覆盖层解析**：`client_path()` 让 `packs/<rel>` 优先于 `Data/<rel>`（微更新不落 `Data`，见 [game-data-sources.md](game-data-sources.md) §5.4）；单测 `tools/test_dlc_packs.py`。Rust 侧同义实现 `game_extract::resolve_client_path` |

按提取目标的导出器（接线状态见 §一 / §1b 的"替代完成度"列）：

| 提取目标 | 工具 | 产出落点 | 接线 |
|---|---|---|---|
| 2D 战术底图 canonical（29 张 1254²） | `tools/export_2d_basemaps.py`（冻结配方；`--verify` 重算 + 索引/字节/canonical 三方对齐；`--self-test` 锁曲线单调/不裁切/色相） | `release/2d-basemaps/` + manifest → 交接 WotbTools `frontend/src/assets/maps/`；同时写包内 `release/asset_pack/map2d/<code>.webp` + `index.json`（COS 免发版通道，上传由既有发布链承担） | ✅ |
| 地图场景（GLB + 地面 + 分层 + sidecar） | `tools/export_map_glb.py`（`--ground-only` / `--scenery-only` / `--jobs`） | `data/cache/maps/<space>.{glb,ground.*.webp,json}` | ✅ |
| 地图可破坏物清单（碰撞 / 耐久 / 类型库联表） | `tools/export_map_destructibles.py` | `data/cache/maps/<key>/destructibles.json` | ✅ |
| 坦克模型（`model.glb` + `collision.glb`） | `tools/export_tank_glb.py` | `data/cache/local_models/<id>/` → **同步进 `cache/models/`**；静态变换烘焙 / StateSwitcher 状态过滤见 [local-model-export.md](local-model-export.md) §1 铁律 14/15，单测 `tools/test_export_tank_static_transform.py` | ✅ |
| 坦克封面图 / 图标（含展示裁剪框） | `tools/export_tank_icons.py`（DVPL 剥壳裸 webp；`--emit-crops` 内容框；`fetch-icons` 降为 BK 对照/兜底） | `data/cache/local_tank_icons/`（738/738）→ **同步进 `cache/tank_images/`**；`data/cache/tank_icon_crops.json` → 包内 `tank_images/crops.json` | ✅ |
| 车辆数值（`tanks.pb` / `models.pb` 等价物） | `tools/extract_vehicles.py`（+ `data/tank_id_bridge.json` 的 `--emit-bridge`）→ `tools/emit_vehicle_pb.py` | `data/cache/local_pb/`（738 辆全量）→ **`data/tanks.pb` / `data/models.pb` 现役** | ✅ |
| 坦克悬挂 | `tools/export_tank_suspension.py` | `data/tank_suspension/<id>.json` → 包内 `suspension/`（728 辆；9 辆无块、1 辆空悬挂按 fail-closed 不产文件） | ✅ |
| 地面履带印 | `tools/export_tank_tread.py` | `data/tank_tread/<id>.json` + `data/tank_tread_textures/*.webp` → 包内 `tread/` | ✅ |
| 动态装甲状态声明 | `tools/extract_armor_states.py` | `data/armor_states.json`（6 车 12 态；无块车不在产物中） | ✅ |
| 俯视地面合成 | `tools/composite_overhead.py`（**朝向 = 契约常量 `YX`**，8 朝向相关只作 `audit` 诊断，见 [game-data-sources.md](game-data-sources.md) §5.5；**世代护栏**：meta 缺 `ground` 键的旧世代渲染件 `--write` 默认拒写，`--allow-legacy-render` 仅诊断用，回归 `tools/test_overhead_guard.py`）+ `tools/archive_overhead_bake.py`（烘焙件归档助手：成对校验 + 世代体检 + `--clear-pack`） | 写回 `<pack>/map/<key>/ground.webp`（**打包器之后跑**，随后 `--refresh-manifest` 重算 manifest）；重烘须 `--base-dir data/cache/maps`；`--verify` 反解读回实际烘入朝向 | ✅ |

### 6.2 Rust 侧解包（`src/wargaming/`，由 CLI 子命令驱动）

| 模块 | 作用 | 驱动命令 |
|---|---|---|
| `dvpl.rs` | DVPL 解码（**fail-closed**：编码长度 / 存储载荷 CRC32 / 解压长度三项核对，不符即 `Err`，见 [game-data-sources.md](game-data-sources.md) §5.3）+ 装甲 XML / 碰撞 YAML 解析（`ArmorModel::parse_from_xml`、`CollisionData::parse_from_yaml`） | `parse-game`、`extract-game` |
| `game_extract.rs` | 批量提取 → `data/game_data/{id}.json`（含 `armor_model` / `collision`） | `extract-game --force`、`update-data` |
| `map_assets.rs` | 与客户端同链解析地图注册表，提取底图 / 地形 / 小地图。**小地图缓存按 space 一份**：`minimap_cache_entries` 取该 space 的**注册表首条目**（与打包器 `seen_spaces` 同规则） | `fetch-terrain`、`fetch-minimaps`（运行期 `/api/playback/map`） |
| `data_version.rs` | `data/data_version.json` 版本指纹（`game_version` 读 `Data/version.txt.dvpl`） | `update-data` |
| `model_fetch.rs` | GLB 全量预热（BlitzKit 兜底路径，已非主来源） | `fetch-models` |
| `tank_configs.rs` | 配置 / 装甲领域层（`synth_armor_model`：逐板装甲仍读 models.pb，`primary` 从 game_data 拷贝） | `dump-tank-data` |
| `tank_resolver.rs` | 装甲**摘要**（优先读客户端 `game_data` 的 `armor_model`，缺失回退 BlitzKit） | CLI 分析链 |

### 6.3 对照 / 验证（同目录，非导出源）

`tools/compare_tank_glb.py`（本地导出 vs BlitzKit 逐辆几何对照）、`tools/compare_tank_icons.py`
（封面图逐像素对齐对照）、`tools/compare_vehicle_data.py`（车辆数值逐字段对照）、
`tools/test_export_variants.py`（变体标签单测）、`tools/probe_switch.py`（.sc2 状态开关解剖）、
`tools/decode_experiment.py`（可破坏物候选物理解码实验）。

### 6.4 打包 / 分发 / 本机辅助

| 路径 | 作用 |
|---|---|
| `scripts/export_asset_pack.py` | 收拢上述产物为 `release/asset_pack/`（`index.json` / `manifest.json` + 逐文件 sha256）；`--refresh-manifest` = 不重打包、按包内容重算 manifest（合成后 / 上传前跑） |
| `tools/upload_asset_pack_cos.py` | 资产包 → COS 差分上传（跳过判定 = 桶内 manifest 的逐文件 sha256，`manifest.json` 最后强传；陷阱见 §2.1） |
| `scripts/serve_asset_pack.mjs` | 本机 CORS 伺服 `release/asset_pack`（消费方 dev server 取用） |
| `scripts/build-wasm.ps1` | WASM 回放解析产物构建（引擎侧，与资产提取无关） |

### 6.5 不在本仓 / 未跟踪

- **俯视渲染器**：消费方仓 `WotbTools/frontend/scripts/bake-ground-overhead.mjs`（headless
  Chrome + three.js，产物 `.rgba` 交 `tools/composite_overhead.py` 合成）。
- **逆向探针**：`examples/*.rs`（`destructible_probe*` / `filter_*_probe` /
  `m29_*_probe` / `p1_comp_probe` / `dump_dvpl` 等），`.gitignore` 覆盖、仅本机存在；
  `cargo test` 仍编译本地副本，但**不入库**。
- **逆向调查报告与转储**：`investigation/`（协议 / 对照报告、facet 转储等本机证据材料；
  docs 正文以相对路径引用）——`.gitignore` 覆盖、**不入库**。
- **本机构建产物**：`frontend/public/wasm/`（`scripts/build-wasm.ps1` 产物，
  **随包不随库**）——`.gitignore` 覆盖。

**一句话记法**：地图 / 坦克 / 图标 / 车辆数值在 `tools/`（Python，底座是 `tools/wotbtools/`
解 DAVA 容器）；装甲 / 碰撞 / 底图在 `src/wargaming/`（Rust，CLI 驱动）；
`scripts/export_asset_pack.py` 收口，`tools/upload_asset_pack_cos.py` 发 COS。
