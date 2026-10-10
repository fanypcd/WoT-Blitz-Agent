# 数据面清单：运行所需数据总账（来源 · 可替代性 · 完成度）

> 2026-10-03 整理（"使用中 / 未接入 / 分发"三问）；**2026-10-09 改版为"数据总账"形式**：
> 逐项回答**目前来源**、**是否存在可替代来源**、**替代完成度**（数据层 / 接线层两轴），
> 由 10-07 / 10-09 两轮全量重导与逐字段对照实测支撑（包统计与溯源见 §2.1，弹种 / 枪序
> 核验见 §一注）。
>
> 完成度刻度：`✅ 已接线`（本机解包即现役来源）· `🟢 数据就绪未接线`（本地产物齐备且对照
> 通过，只差代码切换）· `🟡 有缺口`（本地数据缺项或口径待修）· `⚪ 阻塞`（需决策）·
> `➖ 无需替代`。
>
> 姊妹篇：[game-data-sources.md](game-data-sources.md) 讲"每份数据从哪来、能否脱离
> BlitzKit"（含逐字段来源映射、spaced 规则、故障复盘）；本文讲"谁在用、谁维护、怎么分发"。
> 两份文档共享同一套来源口径，改动其一时须同步另一份。

## 一、运行期核心数据（11 项）

运行期（Web / 3D 查看器 / 回放 / CLI）真正会读的 11 项。数据层 = 本地解包产物覆盖度与
对照结论；接线层 = 代码与分发链的切换状态。提取 / 维护工具索引见 §六。

| # | 数据 | 目前来源 | 可替代来源 | 替代完成度 | 剩余工作 / 备注 |
|---|---|---|---|---|---|
| 1 | `data/tanks.pb`（整车数值：弹种/火炮/HP/机动/模块） | **BlitzKit CDN（现役）** `/definitions/tanks.pb`（`blitzkit.rs::fetch_and_save`；包内直发 pb） | 客户端解包 `tools/extract_vehicles.py` → `tools/emit_vehicle_pb.py` 可编码为**同格式 pb**（工具已备，**未接线**） | 🟢 数据 ~99.5% / 接线 0% | ⚠️ **2026-10-09 试接当日已回退**（本地版本验证未完成，不得直接替换 `data/*.pb`）。与 BK 的剩余差异共 **122 处**、已全部定性（§一注）：119 枪序（研发序口径）+ 10625 explosion_radius（**BK 单点错值**，客户端 0.1）+ 名字 2 条；试接批实测：重建 `tank_cache.json` 仅 1 行差异 |
| 2 | `data/models.pb`（逐板装甲/spaced/履带厚/模型原点/包围盒/限位） | **BlitzKit CDN（现役）** `/definitions/models.pb`（同上） | 客户端解包 `local_pb/models.json` + `game_data`（编码器同 #1，未接线） | 🟢 数据 ~99% / 接线 0% | ⚠️ 试接已回退（同 #1）。接线时按**游戏研发序**选档（BK 为模块 id 升序：105 辆枪序不同、91 辆末位主炮不同、**15 辆**板集实际不同；试接批实测包内 `armor_model.gun` 与 `game_data` 收敛，仅 2 辆 f32 量化差）；`initial_turret_rotation` 已找到客户端直源（车辆 XML `<turretInitialRotation>`，4 辆非零值与 BK 逐值一致、731 辆无元素=BK 的 None，对照 0 差异）；43 辆无炮塔 TD 的炮塔 bbox 为**空心占位**（两侧皆空，见 [game-data-sources.md](game-data-sources.md) §2.3） |
| 3 | `data/tank_cache.json` | 本机自产（派生自**现役 BlitzKit pb**；俯仰 ← models.pb；六面装甲摘要 ← game_data 客户端优先） | 同一派生链改接客户端 pb（未接线） | 🟢 随 ①② | `shells[].shell_type_id` 已透出（§1a） |
| 4/5 | `data/game_data/{id}.json`（装甲模型 + 碰撞盒） | **客户端解包（现役）**：`XML/item_defs/vehicles/{nation}/{model}.xml.dvpl` + `3d/Tanks/Parameters/{nation}/{model}.yaml.dvpl` | 它本身就是 | ✅ 100/100 | 链内 2 处 pb 依赖待拆：文件定位用 pb 的 `model_name`（field32）；炮塔/炮管碰撞盒**选段**用 models.pb 的 module→node 映射（缺失回退"末位"） |
| 6 | `data/tank_data/{id}.json`（→ 包 `tank/{id}.json`） | **混合三源**：models.pb（板/spaced/履带/原点/限位/bbox）+ tanks.pb（数值/弹种）+ game_data（primary、chassis/gun bbox） | 客户端三源齐备 | 🟡 数据 ~95% / 接线 0% | 缺口同 ①②；`configs[]` 的节点数/索引已走客户端 GLB（`model.glb` 节点名）✅；⚠️ 43 辆无炮塔 TD 的 `collision_boxes.turret` 为 null——**两侧都只有空心占位**（客户端 YAML 无 turret 段，models.pb 是 4 字节空 bbox），非缺数据，见 [game-data-sources.md](game-data-sources.md) §2.3 |
| 7 | 弹种反解表（全局 shell id → 弹种数据） | 本机自产（`ShellKindTable::from_tanks_pb` + CLI `dump-shell-kinds`，输入为现役 BlitzKit pb） | 客户端 `shells.xml <kind>`（4 值闭集，已实测；提取器与编码器均已支持，未接线） | 🟢 数据 100% / 接线 0% | `atgm_heat`→HOLLOW_CHARGE 验证一致；接线随 #1 |
| 8 | `data/cache/models/{id}/*.glb` | **客户端解包（现役，2026-10-07 换源）**；BlitzKit CDN 退为缺失兜底（`web/assets.rs::ensure_glb_bytes` / `wargaming/model_fetch.rs`，当前休眠） | 它本身就是 | ✅ 100/100 | 735/735；与 BlitzKit 逐字节一致在 **23 辆上刻意打破**（静态变换烘焙 / 状态过滤，2026-10-10，见 [local-model-export.md](local-model-export.md) §5.1）；包内 `glb/` 已替换；**COS 已同步**（2026-10-10 只传坦克 glb：294 件 = 本批 23 ∪ MR 因子批 271，回拉 1470/1470 一致；§2.1 上传链） |
| 9 | `data/cache/tank_images/{id}.webp` | **BlitzKit CDN**（离线 `download_all_icons` + 运行期懒下载 `web/assets.rs:235`） | `tools/export_tank_icons.py` → `local_tank_icons/` **735/735**（2026-10-10 改为**逐车 `bigIconPath` 声明**精确匹配——764 份参数文件中 755 份逐车文件全含、按整文件名含国家标签解析；备选链=注册表/短名/全名/模型名/别名 + 唯一性护栏） | ⚪ 数据 100%（接线待做；同源美术、接近视觉无损） | 与 BK **同源同画**（1:1 像素、左上角对齐、BK 画布为裁剪后小画布；轮廓 IoU 中位 0.986；旧 NCC 0.21 结论已撤销，见 [decoupling-status.md](decoupling-status.md) §3.4） |
| 10 | `data/cache/maps/`、`data/cache/terrain/` | **客户端解包（现役）** `.sc2`/`.scg`、heightmap、colormap；地面再经 `tools/composite_overhead.py` 叠俯视合成（渲染缓存 `release/overhead-bake/`；⚠️ 该缓存为 **09:49 版**、早于当日光照/水面对齐批——俯视层里的建筑是 Lambert 时代观感，刷新口径见 [game-data-sources.md](game-data-sources.md) §5.5） | 它本身就是 | ✅ 100/100 | 2026-10-09 全量重导随包（§2.1） |
| 10a | `data/cache/maps/{key}/destructibles.json`（可破坏物清单） | **客户端解包（现役）** `.sc2` + `XML/destructibles.xml.dvpl` | 它本身就是 | ✅ 100/100 | 消费方与回放切面 `destructible_events` 联表（逆向总集 §5.4） |
| 11 | `data/data_version.json` | 本机自产（`game_version` 取客户端 `Data/version.txt.dvpl`） | 它本身就是 | ✅ 100/100 | `blitzkit_updated_at` 仅溯源戳 |

来源分布：BlitzKit 3 项（#1/#2/#9）、客户端解包 5 条（#4/#5/#8/#10/#10a）、本机自产 3 项（#3/#7/#11）、混合 1 项（#6）。
按完成度：`✅ 已接线` 6 条（#4/#5/#8/#10/#10a/#11）· `🟢 数据就绪未接线` 4 项（#1/#2/#3/#7；客户端解包→pb 编码器已备，**2026-10-09 试接后回退，待验证**）· `🟡 有缺口` 1 项（#6）· `⚪ 阻塞` 1 项（#9）。

**2026-10-09 弹种权威化**（v0.4.1）：`tanks.pb` field9（客户端 `shells.xml <kind>` 语义枚举的
翻译，0=AP / 1=APCR / 2=HEAT / 3=HE）成为弹种判定权威，field7 的 `icon` 词表降为回退——#3 的
`shells[]`、#6 的 `tank/{id}.json` 都透出该 id（各 1369/2079 有值；空值集恰好是 AP 系 icon 串，
即 proto3 零值省略，回退语义一致）。运行期 `PenetrationRequest` 新增 additive `shell_type_id`，
`resolve_shell_type` 三级优先（id → icon 词表 → HE）；#7 反解表本身未变（仍 field7 icon 分类，
tanks.pb 未动）。

**2026-10-09 枪序核验与换源口径**：`tanks.pb` 顶塔枪序与客户端 XML 文档序（= 游戏研发序，
末位为顶级主炮）**逐辆一致（735/735）**，而 models.pb 为模块 id 升序——"末位 = 顶级主炮"的
约定在 models.pb 侧不成立，实测影响：**105 辆**顶塔枪序不同、**91 辆**末位主炮不同，其中
**15 辆**末位主炮的板集实际不同（接线时应按研发序选档——试接批实测包内 `armor_model.gun` 与
`game_data` 收敛；此前"82 辆不一致"的计数把 `game_data` 的零厚板（`"5": 0.0`）与 `<gun>` 命名板算成
了差异，属口径错误——正确数 15 + 2 辆 f32 量化）。客户端 XML 的 gun/turret 条目自带
`<level>` 研发等级与 `<unlocks>` 研发图，是"顶级配置"的权威判据。

**2026-10-09 弹种列表权威（试接批发现的第二处口径）**：车辆的弹药列表以**共享炮定义
（`components/guns.xml` 的 `<shots>`）**为权威——弹道与穿深（`piercingPower`）只在那里；
车辆内联 `<shots>` 是价格/可用性叠加，个别车引用的弹种族与共享定义不同（`J24_Type_57`
实测：内联 base 族在客户端没有任何弹道数据 → 穿深 0；共享 A 族 218/260/65 与 BlitzKit
一致）。提取器已改为按共享列表迭代、内联同标签条目合并覆盖其余字段。

### 1a. 消费方从 `tank_cache.json` 读的字段

资产包里的 `data/tank_cache.json` 是百科的**列表/概要**数据源。消费方当前读取：

| 字段 | 说明 |
|---|---|
| `name` / `tier` / `nation` / `type` / `hp` | 行展示与筛选、排序 |
| `is_premium` / `is_collector` | 车型标记。**两者是 `tanks.pb` field13 的同一枚举**（1=金币 `is_premium`、2=收藏 `is_collector`），故互斥——实测 735 辆：金币 91 / 收藏 338 / 同时为真 0。WotbTools 据此给卡片上不同颜色的边框（金币=警告色、收藏=`--color-info` 蓝） |
| `shells` | 弹种表（`penetration`/`damage`/…）。**取顶级炮塔 × 顶级主炮**（§3.5 口径），消费方由此派生 `pen_max` |
| `shells[].shell_type_id` | 弹种权威 id（**2026-10-09 新增**）：tanks.pb field9 翻译，1=APCR / 2=HEAT / 3=HE；空值仅出现在 AP 系（proto3 零值省略 → 按 0=AP 或 icon 回退，语义一致）。判定弹种（跳弹角/溅射分支）应以此为准，icon 串仅作显示 |
| `armor` | 六面装甲摘要（**顶级**炮塔档位，见 [game-data-sources.md](game-data-sources.md) §3.5） |
| `view_range` / `turret_traverse_speed` | 视野 / 炮塔转速，**顶级**炮塔 |
| `speed_forward` / `speed_reverse` / `hull_traverse` / `gun_depression` / `gun_elevation` | 机动与俯仰 |

> 派生字段（`pen_max`）由消费方自行聚合，**不在本仓生产**：前端 `tankopediaQuery.js`
> 取 `shells[].penetration` 的最大值。所以弹种取哪一档炮会直接影响列表卡片的穿深与排序。

### 1b. 派生 / 注入 / 支撑数据

不是独立来源，而是由 §一 各项再加工、或注入消费方的产物——**间接依赖最容易漏记**：

| 数据 | 目前来源 | 可替代来源 | 替代完成度 | 剩余工作 / 备注 |
|---|---|---|---|---|
| `data/local_pb_supplement.json`（**备而未用**，编码器配套） | 客户端无对应概念/数据、试接时沿用既有 BlitzKit 值的字段（集中登记，不假装它们是客户端数据） | 逐项消除：`dev_name` 客户端无此概念（无消费者，可删）；名字仅剩 **2 条**（1 枪 + 1 履带，两种字符串源都没有） | 未接线（当前产物：dev_name 735 / 名字 2；`initial_turret_rotation` 已客户端化、不再需要） | 由 `tools/emit_vehicle_pb.py --emit-supplement` 生成，随游戏版本人工维护 |
| 包 `data/tank_names.json` ＋ WASM 注入表 `tankNamesJson` | 打包器从 tank_cache 抽（**BK 名**） | 客户端名：**两源合并后与 BK 仅差 2 条**（2026-10-10：修 4 处查询缺陷 145→77，接入运行时本地化覆盖层 77→3，残差 1 枪 1 履带） | 🟢 数据就绪未接线 | 覆盖层为 per-user 运行时缓存（需客户端同步过）；2 条残差由补充表兜底（见 [decoupling-status.md](decoupling-status.md) §2 C2、[game-data-sources.md](game-data-sources.md) §2.2） |
| WASM 注入表 `limitsJson`（炮管俯仰锚定） | 包 `tank/{id}.json` 的 `pitch_limits` ← 现役 models.pb | `local_pb` 的 `pitch_limits`（零差异） | 🟢 数据 100%（未接线） | 接线随 #2 |
| WASM 注入表 `shellsJson`（弹种徽标/判定） | `dump-shell-kinds` ← 现役 tanks.pb | 客户端 `shells.xml <kind>` | 🟢 数据 100%（未接线） | 接线随 #7 |
| `data/tank_id_bridge.json`（735 条 tank_id ↔ 模型名） | 现表由 `tanks.pb` 导出（`--emit-bridge`） | **纯客户端可重建（2026-10-10 实测 735/735 一致）**：`tank_id = (list.xml <id> << 8) \| 国家基数`（与 guns/shells 同编码；`--emit-bridge --from-client`）；客户端另有 **20 辆** BK 没有的车（教程 bot/超测）可顺带覆盖 | ✅ 能力已验证（slug 列无消费者可删） | 接线时改用客户端源即可自动覆盖新车 |
| 包内 `data/tanks.pb` / `data/models.pb` | **BlitzKit 原始文件直发** | 换源接线后可从包内移除 | ➖ 随 ①② | 移除会改变包结构——消费方若直读 pb 需同步 |
| 运行期 web 端点（`/api/tanks` 花名册、`/api/tank_filter`、`tank_detail.is_collector`、`/api/shells`、`models_status`） | 运行期**直读 pb**（花名册 / 名 / tier / nation / type / collector / 弹种 / models.pb id 表） | 改读 tank_cache / 本地数据 | 🟢 代码改动小 | 仅本机 web GUI 用（前端已冻结，不进分发） |
| 解包底座（DVPL/DAVA 容器、DLC `packs/` 覆盖层） | 本机自产代码（Rust `dvpl.rs` + Python `tools/wotbtools/`，fail-closed 两端对齐） | 不适用 | ✅ 100/100 | 本地链硬前提：需本机装客户端（`Data/` + `packs/`），游戏版本更新后整链重跑 |

引擎产物（WASM Release，`wotb-replay-wasm-*.zip`）构建自 `crates/replay-core|wasm`，**不含
任何 pb**；上表三张注入表（车型名 / 俯仰锚定 / 弹种表）是消费方唯一的**间接 BK 面**——它们的
输出（`tank_name`、`gun_pitch` 解码、`shell_kind`/`shell`）都随注入数据来源变化。

### 1c. 非数据类外部依赖

| 依赖 | 用途 | 可替代性 |
|---|---|---|
| WG API（`api.wotblitz.{asia,eu,com}`，`wargaming/api_client.rs`） | 玩家战绩 | ➖ 服务器数据，**无本地替代也不需要**；不落盘、不进分发 |
| 本机客户端安装（Steam `Data/` + `packs/`） | 所有本地解包链的输入前提 | ➖ 必须（`--game-data` / `--game-dir` 可覆盖路径） |
| BlitzKit 作为交叉校验对象 | 定期用本地提取结果与其 pb 做 diff，漂移即告警 | ➖ 保留该角色（本轮两轮对照即此用途） |

## 二、分发渠道

共 **三条**，彼此独立：

### 2.1 COS 桶（数据面主力）

- 桶 `wotbtools-assets-1478073677`，地域 `ap-shanghai`
  （`wotbtools-assets-1478073677.cos.ap-shanghai.myqcloud.com`）
- 布局与 `release/asset_pack/` **逐项对应**，`manifest.json` 含全量 sha256（2026-10-10 起
  与包内容全量自洽：`--refresh-manifest` 在合成之后、上传之前重算，见下）
- 写入凭据由 `wotbtools-asset-publisher` 子账号持有（整桶读 + 写）
  —— **密钥不入库、不写进任何被跟踪的文件**，需要时通过环境变量传入
- 发布流程（差分上传 + 上传后回拉逐对象校验 + 回滚素材）见
  [game-data-sources.md](game-data-sources.md) §5.2

包内布局（本地实测 **2026-10-10 按包内容重算**：**5000 个文件 / 4310.6 MB**——第二十三次为
**"36 图全量重导 + 俯视重渲合成 + 掩码回填 + COS 全量差分上传"**（地面+场景全量重导后重建包，
`composite_overhead.py --all --write` 重烘、`bake_terrain_cover.py` 回填 36 掩码、`--refresh-manifest`
重算；**COS 已同步**：成功 947 / 失败 0 / 跳过 4053，`manifest.json` 强传，远端清单 5000 条 /
4310.6MB、`cover.u16.bin` 36、`suspension/` 725 均在线上且按 sha256 逐条比对一致。**同时修两处
中间件泄漏**：打包器补掩码缓存源路径（`data/cache/maps/<space>.cover.u16.bin`——此前重建会静默
丢掩码，实测 5000→4964 文件、`terrain.json.cover` 消失），`--refresh-manifest` 与
`tools/upload_asset_pack_cos.py` 均排除 `overhead/` 中间渲染件（防 2026-10-09 的 2.4GB 误传复现）。
见 [index.md](index.md) v0.4.2 条目）；切面以下为历史记录：第二十次为
**"状态切换器的骨骼网格（`SkinnedMesh`）导出"**（36 图 `scenery.glb` 替换：导出器白名单纳入 `SkinnedMesh`、只取激活状态 `State 0`、按静止绑定姿态导出 ⇒ 信号灯灯头/悬臂等"组件不全"补齐；**+3.2 MB**，4307.3 → 4310.5 MB，文件数不变；并随同重烘 36 图让位掩码、`--refresh-manifest` 重算；**COS 未同步**。见 [index.md](index.md) 同名条目）；第二十一次为**"让位掩码适用范围修正（水面/水下件排除）"**（36 图 `cover.u16.bin` **重烘**：水面片自身与水下薄板（冰面）不再入贴地判定 ⇒ 不再扰动可见岸线；文件数/总大小不变（5000 / 4310.5 MB）、内容哈希变化，`--refresh-manifest` 重算；**COS 未同步**。见 [index.md](index.md) "地形让位掩码"条目的同日修正）；第十九次为
**"地形让位掩码（数据面）"**（36 图各新增 `map/<key>/cover.u16.bin` 0.5 MB + `terrain.json`
增 `cover` 字段：**+36 文件 / +19.1 MB**，4964 → 5000 文件、4288.2 → 4307.3 MB；
`--refresh-manifest` 重算；**COS 未同步**。见 [index.md](index.md) 同名条目）；第十八次为
**"图片注册表撞键修复"**（36 图 `scenery.glb` 全量替换：`add_texture` 的 albedo key 曾不含
染色/烘焙内容变体 ⇒ 同 albedo 的材质互相顶替（普查 54 图条目 / 788 变体）；修复后各类变体各自
成图，36 图 scenery 836.6 → **~927.4 MB（+90.8 MB）**；`--scenery-only` 重导，地面与合成缓存
未动；用户报障点 forgecity 挡土墙沥青片 albedo 0.0281 → 0.2580。见 [index.md](index.md) 同名条目）；
第十七次为**"细节层接入（B1）"**（10 图 `scenery.glb` 替换：+51 个材质写 `extras.detail`（贴图 + 平铺倍率），
293.2 → 295.4 MB（+2.2 MB），文件数不变；`--scenery-only` 重导，地面与合成缓存未动）；
另含**他批在途**：`suspension/` 725 件（~2.2 MB，另一会话的坦克悬挂批次，文件已在包内、随本次
manifest 重算登记，**未上传**）；第十六次为
**"动态阴影（G1）→ 同日撤回"**：该次重导给 36 图 `lighting.json` 加了 `shadow` 块（级联范围/
阴影色/LMGate）、给 scenery 材质加了 extras `shadowReceiver`；工具与消费端**均已回到撤回前
状态**（2026-10-09 用户决定放弃），包内这两处字段**保留为惰性数据**（消费端不读；为避免无谓的
36 图重导不回滚已构建的包，**下次重导自然消失**）；manifest 读数 4280.0 MB 属**合成前**口径、
实际字节增量 ≈ 0——口径见下）；第十五次为
**"坦克静态变换烘焙 + StateSwitcher 状态过滤"**（23 辆 model.glb 重导，见
[local-model-export.md](local-model-export.md) §5.1；体积 ±0）；第十四次为
**"坦克材质补齐 PBR 因子"**（270 辆老式车 + 8 辆混合车（部分材质无 MR 贴图）= 566 个材质、共 278 辆无 MR 贴图 → 此前落进 glTF 默认 `metal=1/rough=1`
（全金属、只剩环境反射＝发白）；现写 `metallicFactor 0` + `roughnessFactor 1−inGlossiness`；
465 辆有 MR 贴图的车逐字节不变）；第十三次为
**"光照图判据收窄"**（审计 B7：绑 `lightmaps` 槽 ≠ 客户端会采样——判据加 `MATERIAL_LIGHTMAP`
三来源；7 图 scenery 重导，客户端 unlit 的网格（medvedkovo 外围群山等 111 实例）不再误乘暗光图，
体积微降）；第十二次为
**"视角淡出效果片走软混合"**（审计 B6：`rays.sc2` 类光束片此前被导出成 `alphaMode=MASK/0.33`，
裁掉 95% 内容；现按客户端 `enabledPresets.AlphaBlend` + `BLEND_BY_ANGLE` 导出 `BLEND` + extras，
仅 2 图 scenery 变化 → 重打包；体积不变）；第十一次为
**"贴图全部改客户端原生分辨率"**（用户要求；去掉导出器的 1024/2048 上限与立方图等距柱状 512 宽，
36 图 scenery + 地面全量重导：`scenery.glb` 合计 620 → 834 MB、地面 525.7 → 543.9 MB，
整包 +231.5 MB/+5.7%；用户报障点 medvedkovo 外围群山 `mountains_001kl_` 的 albedo/lightmap
1024² → 2048²）；第十次为
**"水面档位回到 MEDIUM"**（同日先按用户选择试了 LOW 档、实看被判"不好"后回退：14 图 scenery 重导，
水面数据从 `{albedo, decal, cube?, props}` 回到 `{normal, cube, props}`，产物体量与第九次之前一致），
并含同批的**"立方图 DXT5 解码修正"**（`wotb_cube` 的 fourCC→BCn 表曾把 DXT5 当 BC5 解 ⇒ `italy`
的水面立方图静默失效、该图水面回落普通材质；修正后 italy 拿到完整 MEDIUM 数据）；第九次为
"水面试行 LOW 档"（同日、实看被判"不好"，已随第十次回退）；第八次为"水面着色"（36 图 scenery
重导，+逐图水面 cubemap/法线/属性）；第七次为"水面判定改材质驱动"；第六次为
"环境反射遮罩"（5 图 scenery 重导，+2.3 MB）；第五次为"逐图 IBL"（+36 张 `ibl.webp`，~1.0 MB）；第四次为"烘焙光照图接入"（36 图 scenery 全量重导：
内嵌光照图集 121 张 + 光图网格随导 TEXCOORD_1，+104 MB）；第三次为"动画混合层 + 逐图
`lighting.json`"；
其中 `map/himmelsdorf/ground.webp` 为当日朝向修正重烘件 7080664 B，见
[game-data-sources.md](game-data-sources.md) §5.5）。**总量口径已闭合（2026-10-10）**：打包器
报的"4279.9 MB"是**合成前**读数（manifest 在合成器写回 36 张 ground 之前生成），而合成后包内
ground 合计 252.0 MB vs 源（合成前）339.1 MB——**−87.2 MB** 正好解释 4279.9 → 4191.9 的差额
（36 张逐张比对实测）；`--refresh-manifest` 后的 4191.9 MB / 4239 文件即包内实际字节。
`manifest.json` 的
`upstream_commit` / `worktree_dirty` / `generated` 是包内容的溯源锚点——`worktree_dirty=false`
表示该 commit 的**原样工作区**即可复现整包（语义见打包器 `git_provenance()`）；**现值以包内
manifest 为准，每次重打包或改溯源戳后刷新本句**。2026-10-07 的历史脉络：资产重传时 =
`90f1bf8` + dirty（当时导出器修复与工具入库尚未提交）；同日随文档同步**仅改溯源戳**（资产
字节不变）→ `upstream_commit` = 文档同步那次提交、`worktree_dirty` = false。**2026-10-10
现值（十六次"重打包"）**：`upstream_commit = 1b4841bc`、`worktree_dirty = true`
（本批修复未提交时重算）、`generated = 2026-10-09T17:05:28+00:00`；包↔manifest 抽查逐项一致
（23 辆新件 23/23、himmelsdorf ground 条目 == 包内件 `a142e94e…`）✔️，**但 13 图 scenery 的新件
与 23 辆坦克件**已于同日上传**（见下；上一版线上状态为 `cc1cf587` / 10-09T01:58:30Z，
本次为坦克 glb 的局部发布，地图/数据侧仍由另会话在飞）。此前同日第一版：
格式对照修复批全量重导（36 图场景 + 坦克贴图链）后重打包并同步 COS。

| 包内路径 | 文件数 | 来自（见第一节编号） |
|---|---|---|
| `glb/{tank_id}/{model,collision}.glb` | 1470 | #8 |
| `tank/{tank_id}.json` | 735 | #6（由 `tank_data/` 改名） |
| `tank_images/{id}.webp` | 735 | #9 |
| `game_data/{id}.json` | 735 | #4 / #5 |
| `map/{key}/…` | 486 | #10：36 场景 `scenery.glb` + 36 `ground.webp` + 234 分层 `ground/{cm,lm,tile0,tile1,mask0,mask1[,hmap0,hmap1]}.webp` + 36 `ground.layers.json` + 36 `terrain.json` + 36 `terrain.u16.bin` + 36 `mini.webp` + 36 `destructibles.json` |
| `data/` | 5 | #1 `tanks.pb`、#2 `models.pb`、#3 `tank_cache.json`、#11 `data_version.json`、打包器派生 `tank_names.json` |
| `map/{key}/lighting.json` | 36 | #10 配套：`tools/export_map_lighting.py` 逐图太阳/环境色/雾/IBL |
| `map/{key}/ibl.webp` | 36 | #10 配套：`tools/export_map_ibl.py` 逐图环境反射（等距柱状，~28KB/图） |
| `index.json` | 1 | 打包器生成（地图 id → key/display） |
| `manifest.json` | — | 打包器生成（全量 sha256；本身不登记自己） |

打包器：[scripts/export_asset_pack.py](../scripts/export_asset_pack.py)。注意它不是纯拷贝，
中间有改名与派生：`tank_data/` → `tank/`；`tank_cache.json` 额外派生 `tank_names.json`；
地图地形多一个 `terrain.json` sidecar（把 `X-Terrain-Meta` 头物化）；`index.json` /
`manifest.json` 均为生成物。另有 `--refresh-manifest` 模式：不重打包、按包目录现有内容重算
`manifest.json` 的逐文件 sha256（合成器改写 ground.webp 之后、上传之前跑，见下）。

**上传**：[tools/upload_asset_pack_cos.py](../tools/upload_asset_pack_cos.py)
（凭据只从环境变量 `COS_SECRET_ID` / `COS_SECRET_KEY` 读，不落盘）。跳过判定 = **桶内
`manifest.json` 的逐文件 sha256**（2026-10-10 起）：内容变了就必须传，**哪怕尺寸相同**——
旧实现只比 `Content-Length`，会把这类修正静默跳过（2026-10-07 的 `map/lagoon/ground.webp`
即此例，需用 SDK 强制 `put_object` 覆盖；2026-10-10 的坦克顶点烘焙同属此类——只改顶点浮点、
不改字节数）；远端 manifest 缺该条目（首传/旧对象）时回退尺寸比对，取不到远端 manifest 则
整体回退并告警；`manifest.json` 最后
强传作完整性锚点；`.json` 走 `no-cache`、二进制走 `max-age=3600`。手工发布流程与注意点见
[game-data-sources.md](game-data-sources.md) §5.2。

⚠️ 上传工具会遍历包目录下**全部**文件：俯视烘焙的渲染中间产物（`<pack>/overhead/*.rgba`，
按图 64MB）必须在上传前移出，约定缓存在 `release/overhead-bake/`，否则会随包传上 COS
（2026-10-07 曾误传 73 个对象 / ~2.4GB，已清理）。合成器重烘已可直接
`--render-dir release/overhead-bake` 读缓存（不必再复制进包，见 [game-data-sources.md](game-data-sources.md) §5.5）。

✅ **`manifest.json` 对 36 张 `map/*/ground.webp` 记"合成前"哈希的偏差已闭（2026-10-10）**：
成因是流程顺序——打包器（生成 manifest）→ `composite_overhead.py --write`（写回合成底图），
合成器不刷 manifest、打包器此前也没有"仅重算 manifest"的开关 → 这 36 项的条目 ==
`data/cache/maps/*.ground.webp` 原始导出版本，而包内/COS 实际是俯视合成产物（本地与线上
一致地如此，不是上传漂移；2026-10-09 himmelsdorf 朝向修正重烘时入档为既有例外）。现打包器
提供 `--refresh-manifest`（按包目录现有内容重算全部条目，不重新打包），发布链固定为
**合成之后、上传之前跑一次**：`python scripts/export_asset_pack.py --refresh-manifest`。
2026-10-10 本轮资产更新已按新链执行——36 项条目 = 包内实际合成产物；上传器的跳过判定也以
该 sha256 为准（条目失配即重传）。
（历史脉络：2026-10-09 朝向修正重烘后，himmelsdorf 的条目仍记合成前哈希 `741b772f…` 而包内
实际内容是重烘件 `a142e94e…`——该偏差已随本轮 `--refresh-manifest` 闭掉，条目现等于包内件。）

> ✔️ **盘上这份包与源一致**（全量重导 + 重打包 + COS 同步回验）：包内 `game_data`
> 735 个、地图 36 张，与 `data/` 源目录同版（抽样 15 项逐字节核对；线上 manifest 与本地
> 逐字节一致、抽样 9 个对象回拉一致）。2026-10-03
> 记的 field32 陈旧包警告已解除。判断包是否陈旧以 `manifest.json` 的逐文件 sha256 为准
> （打包器只做拷贝 + 哈希，**不做来源一致性校验**）。
> **2026-10-09 朝向修正后**：`map/himmelsdorf/ground.webp` 本地已重烘（`a142e94e…`，
> 7080664 B）**并已同步 COS**（delta = 1 个对象 + `manifest.json` 强传；上传后回拉逐字节
> 一致、读回朝向 = `YX`，旧镜像件 `bac8d3bb…` 已被覆盖；其余 35 张 ground 本轮未动）。
> **2026-10-10 坦克静态变换修正后**：包内 `glb/` 已替换 23 辆（清单见
> [local-model-export.md](local-model-export.md) §5.1）、`manifest.json` 已按包内容重算
> （36 张 ground 的条目并入正轨）——**COS 已同步（只传坦克 glb）**：
> 2026-10-10 `--only glb/` 上传 294 件 / 583.8 MB（本批 23 ∪ 文档标着"未同步"的 MR 因子批
> 271；3 件同尺寸不同内容被新判据拦下——旧 Content-Length 逻辑会漏传），manifest 走**远端
> 清单就地补丁**（只登记这 294 条；桶内 manifest 现 4167 条，其余条目照原样）。回拉校验：
> **1470/1470 glb 逐对象 sha256 一致、0 失败**。地图侧（36 张 ground 条目转正、13 图 scenery
> 旧欠、整份 manifest 重传）随地图会话的发布会一并处理。

**不进包的**：`data/cache/local_*`（见第三节）、`data/replay_samples/`、
`data/sessions/`、`data/snapshots/`、`data/token_usage.json`。

### 2.2 GitHub Release（引擎，非数据）

`wotb-replay-wasm-v<version>.zip` —— 由 [.github/workflows/release.yml](../.github/workflows/release.yml)
在 `[workspace.metadata.release].version` 变更合并到 `main` 时自动测试、构建、打 tag 并发布
（版本策略见 [AGENTS.md](../AGENTS.md)；现值 **v0.4.1**，2026-10-09 发布，产物
`wotb-replay-wasm-v0.4.1.zip`）。消费方按 **commit + sha256** 锁定，落到自己的
`/wasm/<40 位 commit>/` 目录；**不浮动跟随 upstream main**。

### 2.3 消费方前端构建（唯一绕过 COS 的数据项）

弹种反解表（#7）以 `shellKinds.json` 的形式随消费方前端构建走，运行期由前端
`import()` 后注入 WASM（`parseShotReplays` 的可选入参）。**它不经过 COS 桶**，
是第一节 11 项里唯一不在资产包内的。

## 三、跨领域接线点与护栏

逐项替代完成度见 §一 / §1b 的"替代完成度"列（含"坦克 GLB 已接线"等结论）；本节只留
**不属于任何单项、换源必然撞上**的接线点与护栏：

- **打包器的源目录**：现读 `data/cache/models/` 与 `data/cache/tank_images/`，而本地导出器写
  `local_models/` / `local_tank_icons/`。两边内部布局与文件名完全一致，**只需改目录**——
  但漏改不会报错，只有产物悄悄沿用旧来源。**模型一半已按此路径落地（2026-10-07：本地导出
  整体同步进 `cache/models`，2026-10-09 复核 735/735；残留 `3921/model.download` 非 `.glb`、
  不入包）**；封面图仍未切换（§一 #9）。几何等价性口径与残留差异见
  [local-model-export.md](local-model-export.md)。
- **来源混用无护栏**：打包器只做拷贝 + sha256（传输完整性），**不做来源一致性校验**。同一辆
  车混用两源在 `tank_configs` 的 `turret_index`/`gun_index` 与 GLB 节点号上是**硬耦合**，会
  静默错位（见 [feasibility-glb-local-export.md](feasibility-glb-local-export.md) §8.4）——现实
  实例：models.pb 枪序与客户端研发序不一致，已致包内 82 辆 `armor_model.gun` 与 `game_data`
  矛盾（§一 #2）。同族问题：manifest 对 36 张 ground 记的是合成前哈希（§2.1）——"拷贝 + 哈希"
  管不住时序与来源，属同一类缺口。

## 四、尚无代码的缺口与非代码障碍

逐项完成度见 §一 / §1b；此处只列需要**新代码或新流程**的项：

- **换源验证与接线**（2026-10-09 试接后回退）：格式决策已定（写 pb 编码器
  `tools/emit_vehicle_pb.py`，运行期零改动），工具链在库；但本地解包版本的**验证未完成**，
  `data/tanks.pb` / `data/models.pb` 已回退为 BlitzKit 现役版本。接线前须补：① 122 处差异的
  最终确认（119 枪序为口径选择，1 explosion_radius 为 BK 错值，2 条名字由补表兜底）；
  ② 回归（本轮试接批的对照方法与数字已存档于 §一 注与
  [game-data-sources.md](game-data-sources.md) §5.2b，可直接复用）。
- ~~`tank_id` 桥~~ **已解决（2026-10-10）**：`tank_id = (list.xml <id> << 8) | 国家基数`，
  纯客户端可重建且 735/735 与现状一致（另有 20 辆客户端独有车）——见 §1b。
- **名字表**：已收敛到 **2 条**（1 枪 + 1 履带，两种字符串源都没有；BK 自建名）→ 由补充表
  兜底。此前"145 处客户端无源"是漏了**运行时本地化覆盖层**所致的误判，见
  [game-data-sources.md](game-data-sources.md) §2.2。
- **封面图判定已降级为抽查**（§一 #9）：与 BlitzKit 封面为**同源同画**（1:1 像素、左上角
  对齐；旧 NCC 中位 0.21/非同一幅画结论系对照方法伪影，已撤销），换源预期接近视觉无损——
  接线前抽查 `local_tank_icons/_vs_blitzkit.png` 即可。
- **坦克悬挂数据面已导出（2026-10-10）**：`tools/export_tank_suspension.py`
  （`data/tank_suspension/<tank_id>.json` → 包内 `suspension/<tank_id>.json`，additive；
  全表 735 辆中 **725 辆导出**、9 辆无 suspension 块、1 辆空悬挂（`Oth10_WarDuck`）
  按 fail-closed 不产文件）。字段 = 逐轮 `{flag(负重轮), a(上行行程), b(下行行程)}` +
  履带静止折线（两代键式都读）+ `trackBendingInfo`/`trackLayingInfo` +
  `chassis.textureScale`；逐轮半径/挂点/`chunkLength` 由前端从模型 GLB 现算（不必导）。
  规模 2262 KB / 725 文件（前端按需取，单场 ≈14 文件 × 3 KB）。接线状态：前端求解器并行
  落地（逐轮贴地 + 履带形变 + 自转/花纹滚动）；机制、分级路径与剩余未定项见
  [tank-suspension-client-re.md](tank-suspension-client-re.md) §六/§七。
  回归：`python tools/test_export_tank_suspension.py`（7 项，含真实客户端抽样）。
  **分发**：本地包已增量补入（`release/asset_pack/suspension/`，725 文件 / 3.1 MB）；
  生产面需按常规流程重跑 `scripts/export_asset_pack.py`（现已含该目录）并
  `tools/upload_asset_pack_cos.py` 上传增量（COS 凭据仅环境变量）。

## 五、完成度汇总

```
按完成度（§一 / §1b 逐条；共 12 条 = §一 11 项含 10a + §1b 主要派生项另列）：

  ✅ 已接线（本机解包即现役来源）
       #4/#5 game_data（装甲 + 碰撞，含 primary）        #8 GLB（10-07 换源，735/735）
       #10/#10a maps/terrain/destructibles               #11 data_version · 解包底座

  🟢 数据就绪、未接线（客户端解包 → 同格式 pb 编码器已备；**试接后回退，待验证**）
       #1 整车数值 · #2 装甲/原点/限位（接线按研发序） · #3 tank_cache · #7 弹种表/kind
       limitsJson · shellsJson · tankNamesJson · 运行期 web 端点

  🟡 有缺口        换源验证：122 处差异待最终确认（119 枪序＝口径、1 处 explosion_radius＝
                   BK 错值、2 条名字＝补表兜底）；桥表维护流程（新车补给未固化）

  ⚪ 阻塞          #9 封面图（735/735 已解出、0 缺；同源同画，接线前提降级为抽查对照图）

  ➖ 无需替代      WG API（战绩） · tank_id 命名空间（只能固化桥表） · BlitzKit 交叉校验角色

2026-10-09 分发同步（§2.1）
  格式对照修复批全量重导 → 重打包（upstream cc1cf587 / dirty=false）→ COS 回拉核验通过
  弹种 field9 透出：#3 shells[].shell_type_id、#6 tank/{id}.json type_id（1369/2079）
  ⚠️ manifest↔36 张 ground 语义偏差入档
```

**一句话结论**：运行期 12 个数据条目中 6 条已由本机解包承接（装甲碰撞 / GLB / 地图 / 版本戳
/ 解包底座），4 条"数据就绪、**未接线**"（`tanks.pb`/`models.pb`/派生面——2026-10-09 试接后
已回退为 BlitzKit 现役版本，本地解包版本验证未完成前不得直接使用），1 条有缺口（`tank/{id}.json`
随 ①②），1 条换源待做（封面图——复核为**同源同画**、接线前提降级为对照抽查）。外部依赖仍是 BlitzKit 的 3 个下载入口（`tanks.pb` /
`models.pb` / 封面图）与 WG 战绩 API。引擎产物（WASM Release）不含任何 pb。

## 六、本地提取工具索引

跑一次"从本机客户端取数"要动到哪些文件（2026-10-07 盘点）。入口默认指向 Steam 安装的
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
| 地图场景（GLB + 地面 + 分层 + sidecar） | `tools/export_map_glb.py`（`--ground-only` / `--scenery-only` / `--jobs`） | `data/cache/maps/<space>.{glb,ground.*.webp,json}` | ✅ |
| 地图可破坏物清单（碰撞 / 耐久 / 类型库联表） | `tools/export_map_destructibles.py` | `data/cache/maps/<key>/destructibles.json` | ✅ |
| 坦克模型（`model.glb` + `collision.glb`） | `tools/export_tank_glb.py` | `data/cache/local_models/<id>/`（10-07 起同步进 `cache/models/`）；静态变换烘焙 / StateSwitcher 状态过滤见 [local-model-export.md](local-model-export.md) §1 铁律 14/15，单测 `tools/test_export_tank_static_transform.py` | ✅ |
| 坦克封面图 / 图标 | `tools/export_tank_icons.py` | `data/cache/local_tank_icons/`（735/735） | ⚠️ 未接线 |
| 车辆数值（`tanks.pb` / `models.pb` 等价物） | `tools/extract_vehicles.py`（+ `data/tank_id_bridge.json`） | `data/cache/local_pb/`（735 辆全量） | ⚠️ 未接线 |
| 俯视地面合成 | `tools/composite_overhead.py`（现役；**朝向 = 契约常量 `YX`**，8 朝向相关只作 `audit` 诊断，见 [game-data-sources.md](game-data-sources.md) §5.5）/ `tools/bake_ground_roofs.py`（旧软光栅，已退役） | 写回 `<pack>/map/<key>/ground.webp`（**打包器之后跑**，随后按新链 `python scripts/export_asset_pack.py --refresh-manifest` 重算 manifest——2026-10-10 起，见 §2.1）；重烘须 `--base-dir data/cache/maps`（缺省读包内件会在已合成图上二次合成）；`--verify` 反解读回实际烘入朝向 | ✅ |

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
- **逆向探针**：`examples/*.rs`（**18 个**：`destructible_probe*` / `filter_*_probe` /
  `m29_*_probe` / `p1_comp_probe` / `dump_dvpl` 等），`.gitignore` 覆盖、仅本机存在；
  `cargo test` 仍编译本地副本，但**不入库**。
- **本机构建产物**：`frontend/public/wasm/`（`scripts/build-wasm.ps1` 产物，
  **随包不随库**）——`.gitignore` 覆盖。

**一句话记法**：地图 / 坦克 / 图标 / 车辆数值在 `tools/`（Python，底座是 `tools/wotbtools/`
解 DAVA 容器）；装甲 / 碰撞 / 底图在 `src/wargaming/`（Rust，CLI 驱动）；
`scripts/export_asset_pack.py` 收口，`tools/upload_asset_pack_cos.py` 发 COS。
