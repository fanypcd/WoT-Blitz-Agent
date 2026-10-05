# 文档索引

> 全部 Markdown 文档的定位与状态一览（2026-10-03 整理）。入口永远是根目录 [README](../README.md)。

## 使用文档（随项目演进，保持最新）

| 文档 | 定位 |
|---|---|
| [README.md](../README.md) | 项目总入口：项目定位、能力总览、与 WotbTools 的关系（分发形态）、仓库结构 |
| [docs/回放与射击逆向总集.md](回放与射击逆向总集.md) | 射击/回放逆向**唯一权威参考**：第一篇=协议层数据段（字节布局/语义/使用状态/死路清单/消费地图）、第二篇=客户端处理架构、第三篇=客户端弹道与命中表现（含 DecodeShotSegment 权威定义）、第四篇=WI 对照与射击复现实现、第五篇=遗留未定项 |
| [docs/replay-contract-v2-supremacy-type39.md](replay-contract-v2-supremacy-type39.md) | **回放契约 v2**：争霸基地状态（sparse 重建 + 零值省略/占领中断归零补正）+ 实时点数 + type39 原始帧用途与 `aim_frames` 删除记录、门禁与版本护栏 |
| [docs/wotbtools-cross-reference.md](wotbtools-cross-reference.md) | 与 WotbTools 逆向结论的逐条裁决记录（采纳/驳回/互证），防止误采或回退已定案；**文末附面向消费方切面的最新进展** |
| [docs/architecture-debt.md](architecture-debt.md) | 架构债与长期改动方案：已完成项（combat.rs 拆分、双路径合并）与仍留存的 tankViewer 目录拆分 |
| [docs/game-data-sources.md](game-data-sources.md) | **数据来源权威表**：每份数据取自 BlitzKit / 本机客户端 / WG API / 自产；本地提取可行性评估；间隙甲 spaced 判定规则；提取链与 COS 资产面发布流程；2026-10 game_data 冻结故障复盘 |
| [docs/data-inventory.md](data-inventory.md) | **数据面清单（谁在用 / 谁维护 / 怎么分发）**：运行期实际读取的 11 项数据及其来源与维护代码；三条分发渠道（COS 资产包 / GitHub Release 引擎 / 消费方前端常量）与包内布局；已备好但未接线的替换来源及其阻塞项；尚无代码的缺口 |

## 方案文档（已执行完毕，留档）

| 文档 | 状态 |
|---|---|
| [docs/vue-migration-plan.md](vue-migration-plan.md) | ✅ 已完成（2026-09-28）：前端四页全部切流 Vue 3 SPA，嵌入 HTML 与 web/vendor 已退役 |

> `docs/mobile_plan.md` 及移动端（`mobile/` Tauri 壳、`mobile_assets/` 随包资产）已于
> **2026-10 随 Android 分发形态一同移除**（详见 [README §与 WotbTools 的关系](../README.md)）；
> 其文内提及的 `scripts/{asset_manifest,export_mobile_maps}.py` 同步删除。
> [docs/vue-migration-plan.md](vue-migration-plan.md) 里提到的 `scripts/package.ps1` /
> `build-all.ps1` 同样只作历史记录——桌面便携包打包链已删除，前端仅本机调试用。

## 对接消费方（WotbTools）的当前进度（2026-10-03）

本项目的回放能力由 WotbTools 以 WASM/静态资产面消费（上游契约与版本锁定见对方仓
`contracts/agent/replay-facets-v2.md`、`deploy/agent/source.json`）。面向消费方的**最新一轮**
版本记录（v0.3.4 起；`Cargo.toml` 的 `[workspace.metadata.release].version` 是唯一发行版本源。该属性变更合并到 `main` 后，Release workflow 自动测试、构建、创建对应 `v<version>` tag 并发布 WASM Release）：

| 版本 | 内容 |
|---|---|
| v0.3.4 | 包流**自行分帧**，不再依赖 crate 对 payload 的反序列化——单个 pickle 形状偏差不再让整场失败 |
| v0.3.5 | 原始 HP 证据：`Damage.hp_raw`（未钳制 u16，区分"血量归零"与"终态血量未知"）与 AoI 开段物化 HP；method8 原始命中通知（全变体、不分类，供消费方做 fail-closed 掉血归属） |
| v0.3.6 | prop3 血量属性广播（录像者自身血量的唯一来源，与 method1 非镜像） |
| v0.3.7 | AI 切面透出**原始未滤波**位姿（type=10）与炮塔观测（prop2）——切面 0.1s 网格是渲染滤波输出，不能当位置证据 |
| v0.3.8 | 结算阵容完整性 `roster_complete` 与录像者车辆代号 `author_vehicle_codename` |
| v0.3.9 | **装填数据补齐**：`PlaybackData.reloads` 相位语义定稿（m0x30 subtype 15/16/17：f2=1/3/4/5/6/7 与 f4 = 服务器剩余弹数快照）+ 新增 additive 字段 `reload_effective`（方法 0x23 = 当前生效完整装填配置时长）；上游同步对方 `baseStatus` 攻防基地 canonical 0 → idle |
| v0.3.13 | **渲染位姿关键帧折线 + 弹道折线（via/leg_secs）**：①`vehicles[].pose_kf`（additive）——位姿来源是客户端滤波器（AvatarFilter 移植）60Hz 逐帧输出，滤波器在「预测-保持」阶段（刚进 AoI 数秒内）输出为保持-跳变阶梯，旧 0.1s 网格重采样 + 网格间线性插值把阶梯混叠成速度摆动（实测某 0.5s 窗内前端线速度 4.5→29.6 m/s、全时线逐帧位置最大偏差 21m，即 3D 回放顿挫来源）；贪心走廊拟合（容差 2cm/0.4°）保留保持段两端与跳变段，点数 7~10/秒/车，消费端线性插值即复现客户端画面；②`shots[].via`/`leg_secs`（additive）——弹道折线：同一发炮弹的全部 method29 共享 (shooter, shotId)，链首 = 发射段、后续 = 跳弹/穿透续段（起点 = 装甲接触点），method20 = 弹道最终停止点（跳弹后落在出射方向延长线上）；段时长按 |Δ|/段速度计（不用 10Hz 包钟——跳弹常与发射同刻，用钟得零长段）；`flight_secs` = 各段之和、`end_time` = 抵达时刻（与 method8 同刻，命中归属随之修正）；确定性归属 = (shooter, shotId) 链 + method20 按 shotId 配对，无任何相关性匹配 |
| v0.3.12 | **可破坏地形事件切面**：`PlaybackData` 新增 additive 字段 `destructible_areas`（100m 格子区域实体锚点）与 `destructible_events`（时刻/类别/槽位/倒向）——类别 prop 1=fragiles 2=柱状 3=树倒、末字节 = lka 槽位（与区域格子联表 = 唯一物体寻址，逆向总集 §5.4 公式闭合，四场回放 799/801）、倒数字节 = 8 位倒向角（服务器权威，未点亮碾压者亦携带）；配套资产管线 `tools/export_map_destructibles.py`（36 图清单+lka serverId）与 scenery GLB 的 D_ 损毁态网格导出（`export_map_glb.py`/`export_asset_pack.py` 已随包） |
| v0.3.11 | **基地占领中断归零 + 单基地存在性补正 + 删 `aim_frames` + 渲染网格加 `hull_roll`**：①争霸（wrapper12）全缺省行 = 显式清空、单基地（wrapper8）双缺省块 = 进度归零——wire 按 proto3 省略零值字段，中断（车辆出圈/被击毁）此前被「缺省=维持前值」吞掉，进度与占领方永久挂在基地上；②`assault_objective_present` 按字段契约放宽为「目标族出现即真」（实现此前多要求 `f3\|\|f4`，把"有目标但全程未占领"的场次整场压掉）；③删除 `aim_frames`（零消费方，实测占回放 JSON 57.6%，20 车样本 6.92MB → 2.93MB；原始 type39 帧保留给射击复现），`PlaybackData.version` 保持 2；④`vehicles[].hull_roll`（additive）取原始 type=10 最近邻——滤波层不输出侧倾 |
| v0.3.10 | **射击复现多 interaction 关联修复**：`unique shotId = 一次开火 = 一个 Shot`；作者严格路径在同 victim / 同钟出现多个 type=32 segment 时，优先用 `method8.hash6 ↔ type32.hash6` 确定关联；重复 method8 广播按 hash 去重，证据不足时继续 fail-fast，不猜选 |

切面字段均为**附加**（`AiReviewFacet` v1 / `PlaybackData` v2 版本不变）。

**对方侧状态（2026-10-05 更新）**：`deploy/agent/source.json` 当前 pin **`v0.3.11` / `73ea422a`**
（v0.3.4–v0.3.11 增量已在生产链路；v0.3.12 可破坏地形切面 + 资产已入 COS，待对方 pin 后消费端 `feat/playback-destructibles` 生效）；此前对方完成**客户端解析迁移**（A158Coke/WotbTools#447
「服务器没有 parser」）——服务端解析器模块整体删除，浏览器/Android 跑本项目的 WASM，**本项目由此成为
该仓唯一的回放解析器**；AI 复盘走 WASM → canonical facts → `ClientAiReviewProjection`，parity 由
`ClientAiProjectionParityTest` 进 required CI 常驻看护。

**对方提出、待本项目补的字段**（见对方 `docs/architecture/client-replay-engine-migration.md` §后续）：
`PlaybackData.damages[]`（数据已在 `timeline.hp_events`）、`coverage`（packet 计数与
`decodedPacketRatio`）、`finish_reason`、`unsupported_damage`（仅双方无数值），以及实测确认
`Shot.game_hit_result` 与对方 Java `primaryResultRaw` 同义；其中"未钳零原始 HP"已由 v0.3.5 的
`hp_raw` 覆盖。

**前端面收敛（2026-10-03）**：本项目不再维护自己的前端与桌面/移动端分发——
Windows 便携包打包链与 Android（Tauri）形态已整体删除（见 [README §与 WotbTools 的关系](../README.md)），
`frontend/` 冻结留档（目录级护栏见 [frontend/AGENTS.md](../frontend/AGENTS.md)）；
**后续前端开发与前端测试一律在 WotbTools 仓库进行**，本项目只出 Rust 核心与
v* tag 的 WASM 发行产物。**3D 回放 / 模型场景查看等视觉验证由用户执行**：Agent 侧只交
锁定不变量的测试（纯函数单测、场景接线守卫、仓库自动化浏览器门禁），不自行截图充当验收
（见 [AGENTS.md §Visual verification](../AGENTS.md)）。装填条渲染（本轮与客户端逐状态对齐：
整夹一条不分割、夹内推弹不补弹、弹鼓逐发补槽、开火取消、服务器 f4 快照重锚）落在 WotbTools
`scene/reloadBar.js`（42 条单测），本仓同构副本见 `frontend/src/scene/reloadBar.js`。

详见 [docs/wotbtools-cross-reference.md](wotbtools-cross-reference.md) §五。

## 可行性评估（待决策，2026-10-01）

用本机客户端解包替代 BlitzKit 数据源的评估。两份报告均含逐字段/逐项验证数字与反例清单，
并登记了**两处既有 bug**（`tanks.pb` 引擎起火率与履带阻力读错 protobuf 字段号，见报告 A §8）
——两处**已于 2026-10-01 修复**（解析器 + `data/tank_data/*.json` 重新生成）：

| 文档 | 结论摘要 | 状态 |
|---|---|---|
| [docs/feasibility-pb-local-extraction.md](feasibility-pb-local-extraction.md) | `tanks.pb`/`models.pb` 可替代；735/735 覆盖、零实质分歧；唯一缺口 `tank_id`（有两条替代路径）。⚠️ 个别结论有 2026-10-03 注记修正（uk 段名 / 显示名缺失数），见文首 | ⬜ 提取器待实施（约 3–5 人日，见 [data-inventory.md](data-inventory.md) §四） |
| [docs/feasibility-glb-local-export.md](feasibility-glb-local-export.md) | 历史评估留档：实施结果见"实现记录" [local-model-export.md](local-model-export.md)；其 §3.3 槽位指派结论已被逐通道实测修正，见文首注记 | ✅ 已实施（2026-10-01） |

## 实现记录（已落地）

| 文档 | 状态 |
|---|---|
| [docs/decoupling-status.md](decoupling-status.md) | **解耦的剩余决策与已定案差异**（2026-10-03 收敛）：已完成时间线；待决策口径（数据模型塌缩 / 25 辆无显示名 / `hull_traverse` 非同量）；封面图命名域与"换素材 ≠ 等价替换"（NCC 0.21）；已定案的 BlitzKit 侧差异清单；对外沟通材料。进度/接线/分发现状见 [data-inventory.md](data-inventory.md) |
| [docs/local-model-export.md](local-model-export.md) | ✅ 已实施（2026-10-01，2026-10-02 补 §4 贴图实测）：报告 B 的几何替代做成 `tools/export_tank_glb.py`，验收口径从"按可达节点求和"收紧到**逐字节 + 节点顺序**——`collision.glb` **735/735**、`model.glb` **733/735** 等价（余 2 辆为 BlitzKit 侧行为，见该文 §5）；贴图槽位与 BlitzKit 完全对齐（731/735 图片数相同、无缺槽位）。§4 逐通道实测推翻了报告 B §3.3 的图源判断，并定下 `baseRMMap` 的**通道搬迁**（ch0→G 粗糙度、ch1→B 金属度）。**不替换运行期数据源**（产物落 `data/cache/local_models/`） |

## 约定

- 逆向结论的**唯一权威**是 [docs/回放与射击逆向总集.md](回放与射击逆向总集.md)；其余文档与其冲突时，
  先查 [wotbtools-cross-reference.md](wotbtools-cross-reference.md) 是否已有裁决。
- 历史推导过程不在工作区文件中保留，需要时查 git 历史。
- `examples/`（逆向探针脚本）与 `tmp_*/`（分析转储）不入库，文档中提及处仅作证据出处记录。
