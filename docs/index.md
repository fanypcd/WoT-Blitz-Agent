# 文档索引

> 全部 Markdown 文档的定位与当前状态。入口永远是根目录 [README](../README.md)。
> 本页只承载**当前状态与结论**；推导过程与历史记录见 git 历史。

## 使用文档（随项目演进，保持最新）

| 文档 | 定位 |
|---|---|
| [README.md](../README.md) | 项目总入口：定位、能力总览、与 WotbTools 的关系（分发形态）、仓库结构 |
| [docs/回放与射击逆向总集.md](回放与射击逆向总集.md) | 射击/回放逆向**唯一权威参考**：第一篇=协议层数据段（字节布局/语义/使用状态/死路清单/消费地图）、第二篇=客户端处理架构、第三篇=客户端弹道与命中表现（含 DecodeShotSegment 权威定义）、第四篇=WI 对照与射击复现实现、第五篇=遗留未定项 |
| [docs/replay-contract-v2-supremacy-type39.md](replay-contract-v2-supremacy-type39.md) | **回放契约 v2**：争霸基地状态（sparse 重建 + 空闲整播判据）、实时点数、type39 原始帧、独立炮塔与生命轨道等 additive 切面契约、门禁与版本护栏 |
| [docs/wotbtools-cross-reference.md](wotbtools-cross-reference.md) | 与 WotbTools 逆向结论的逐条裁决（采纳/驳回/互证），防止误采或回退已定案；附跨仓协作现状 |
| [docs/tank-suspension-client-re.md](tank-suspension-client-re.md) | **客户端坦克悬挂逆向**：处理链与节点绑定契约、逐车 `suspension:` 数据面（`wheels`/履带折线/`chassis.textureScale`）、负重轮-履带求解公式（反汇编还原）、对齐可行性分级与剩余未定项 |
| [docs/architecture-debt.md](architecture-debt.md) | 架构债与长期改动方案：已完成项（combat.rs 拆分、双路径合并）与仍留存项 |
| [docs/game-data-sources.md](game-data-sources.md) | **数据来源权威表**：每份数据取自 BlitzKit / 本机客户端 / WG API / 自产；本地提取链、间隙甲 spaced 规则、DVPL 校验契约、DLC 覆盖层、COS 资产面发布流程与运维陷阱、俯视合成朝向契约 |
| [docs/data-inventory.md](data-inventory.md) | **数据面清单 / 运行所需数据总账**：运行期 11+1 项核心数据的"目前来源 / 可替代来源 / 替代完成度"+ 派生/注入数据 + 非数据类外部依赖；分发渠道与包内布局；工具索引 |
| [docs/decoupling-status.md](decoupling-status.md) | 解耦现状（BlitzKit → 本机客户端自产，已全量完成）：已定案差异清单、封面图与命名域细节、对外沟通材料 |
| [docs/local-model-export.md](local-model-export.md) | 坦克模型本机自产管线：实现铁律（违反会静默出错）、贴图语义口径与通道搬迁、与 BlitzKit 的逐维度对照结论 |

## 档案（已执行完毕 / 报告留档）

| 文档 | 状态 |
|---|---|
| [docs/feasibility-pb-local-extraction.md](feasibility-pb-local-extraction.md) | ✅ 已实施：本机解包替代 `tanks.pb`/`models.pb`（提取器 + 同格式编码器已接线转正，见 decoupling-status §1） |
| [docs/feasibility-glb-local-export.md](feasibility-glb-local-export.md) | ✅ 已实施（结果与修正以 [local-model-export.md](local-model-export.md) 为准） |
| [docs/feasibility-map-lighting.md](feasibility-map-lighting.md) | ✅ 已实施：逐图光照 / 烘焙光照图 / IBL / 天空（数据面与消费端均已落地，逐项状态见 map-render-align-audit） |
| [docs/feasibility-track-marks.md](feasibility-track-marks.md) | ✅ 数据面已落地（包内 `tread/`）；3D 呈现归 WotbTools 前端 |
| [docs/map-render-align-audit.md](map-render-align-audit.md) | 地图渲染 × 客户端**差异总账**（36 图普查的 23 条清单与逐项状态）；未落地项以此文为准 |
| [docs/map-lighting-plan.md](map-lighting-plan.md) | 光照实施设计：期 0–2 主体已落地（逐图光照/光照图/IBL）；剩余项见该文与 map-render-align-audit |
| [docs/vue-migration-plan.md](vue-migration-plan.md) | ✅ 已完成（2026-09-28）留档：前端切流 Vue 3 SPA 后本仓前端已冻结；文中 `scripts/package.ps1` 等打包链已删除 |

> `docs/mobile_plan.md` 及移动端（`mobile/` Tauri 壳、`mobile_assets/` 随包资产）已随
> Android 分发形态一同移除（详见 [README §与 WotbTools 的关系](../README.md)）。

## 当前状态

### 版本与发行

- 版本源 = `Cargo.toml` 的 `[workspace.metadata.release].version`（现值 **v0.4.7**）。
  该值变更合并到 `main` 后，Release workflow 自动测试、构建、创建 `v<version>` tag
  并发布 `wotb-replay-wasm-v<version>.zip`；发布策略与 Android 同步规则见
  [AGENTS.md](../AGENTS.md)。
- 消费方式：WotbTools 经 `deploy/agent/source.json` 按 commit + sha256 锁定 WASM，
  pin bump 时重生成 AI 投影 golden（provenance 字段变化；**行为变化才动字段值**）。
- 面向消费方的版本记录（v0.3.4 起）：

| 版本 | 内容 |
|---|---|
| v0.4.7 | **独立炮塔回放轨道（additive）**：`detached_turrets`——class-8 初始化关联车辆，输出 AoI 区间与 raw 三轴关键帧（同刻移动收敛为最后接收帧，未知布局/身份冲突/非有限位姿 fail-closed）。附带：共享 st10/prop2 排序统一 `total_cmp`（NaN clock 不再 panic）；`packet_histogram` 键口径订正为包类型（仓内无消费方）。切面 v2 不变，消费方零适配。 |
| v0.4.6 | **动态装甲状态载体重定位（数值语义订正）+ 穿深距离衰减 + 顶层弹表单权威化**：①`armorsStates` 真实载体 = **propID 16**（载荷 `[u8 n][n×u32 描述符]`，bit16=激活态；初态经 type=5 create 表物化）——字段名与形状不变，`armor_states` / `shots[].target_armor_state` 对动态车自本版起可信；②`PenetrationRequest` additive `distance_m`/`penetration_far`，d>100m 线性衰减到 far（HE 豁免；对齐客户端 0x719740）；③`tank/{id}.json` 顶层 `shells` 改为直接引用末档配置弹表同一份 JSON（结构性单源化，杜绝重抄漏字段）。数据面 735 辆重生成随包同步；WotbTools 随 pin bump 重生成 golden（仅 provenance 变化）。 |
| v0.4.5 | **动态装甲状态时间轴（additive）**：`PlaybackData.armor_states` / `AiReviewFacet.armor_states`（列式轨道，空则整字段省略）+ `shots[].target_armor_state`（弹着时刻状态号，skip-when-none）；数据面 `data/armor_states.json`（6 车 12 态，`<armorsStatesController>` 声明 = 状态语义权威源）。**数值语义以 v0.4.6 为准**（本版载体判定已被证伪）。 |
| v0.4.4 | **作者车接收突发时间轴重建（解析器）+ 履带印数据面（tread/）**：①包头 clock 是录制端到达时间，网络突发把同车多个 type=10 压成同一时间戳——现对**作者车**按标称 10 Hz 重建时间轴（`filter::respace_same_clock_bursts`，越界回填整段放弃），**远程车保持原样**；消费方零适配（仅作者车位姿数值更平滑，golden 若有对应差异属预期）。②包内 `tread/`：全车产 `tread/<id>.json`（authored 参数或国别主贴图回落）+ 13 张贴图 + `textures.json` alpha 归一化表。 |
| v0.4.3 | **弹种识别补齐 + 顶层弹表判定输入（数据面）**：①Type5 配件串改**变长解析**（`0B <n>`+nB，n=1..=9，空槽省略）——修复不满 9 件玩家 loadout 整条被丢弃；②**弹种指纹回填**（`method29 args[8]` 弹种编码，同场同射手自校准、弹速去重）——`cmpIndex=0` 命中游戏不发 26/27B 段包时回填 `shell_id`；`ShotQuality` additive 新增 `shell_from_launch_code`/`shell_from_velocity`；③顶层弹表补 `caliber`/`normalization`/`ricochet`（2079/2079），车级 `caliber` 改取顶级炮弹径。切面契约未变。 |
| v0.4.2 | **无炮塔流回放放行 + 光照/IBL 资产**：①`prop2`（type=7 炮塔流）全局缺失时车辆筛选退化为仅 type=10（修复此类回放整场拒开；仍 fail-closed）；②资产面：逐图 `lighting.json`（太阳方向/色/强度、环境色、雾原始块）与 `ibl.webp`（逐图等距柱状环境反射）随包；extras 贴图解（光照图/贴花/水面/细节等）随后续版本持续收编。切面契约未变。 |
| v0.4.1 | **数据面与资产管线修正 + 弹种权威化**：地图几何改文件内 authored 法线；PVR3 单通道 L8/A8 解码补全；`PenetrationRequest` additive `shell_type_id`、`tank/{id}.json` additive `type_id`——弹种判定改走 field9（客户端 `<kind>` 枚举，4 值闭集），icon 词表降级为回退；DVPL 解压分配上限 512 MiB。资产面随全量重导同步 COS。切面契约未变。 |
| v0.4.0 | **回放解析确定性收口（语义/契约变更，消费方需 v0.4.0 适配——WotbTools PR #555 已同步）**：`damage_received` 无证据 = `null`（≠0）；击杀原因未知 = 255 哨兵；`infer_shots` 推断路径删除；`shots_from_loose_path` additive provenance；ARENA_INFO comp blob 全玩家提取（`turret_local`/`gun_local` 覆盖率 1/N → N/N）；遗留 #7–#12 清零（HP seed 分级、昵称窗、白名单 masked 匹配、令牌优先等）；type=28 槽位兜底 fail-closed。 |
| v0.3.15 | **实际搭载配置透出 + AoI 重入段化滤波**：`vehicles[]` additive `config_idx`（comp blob 证据链钉定的配置下标）/`burst_size`（弹容原值——**装填条弹容 N 的唯一权威 = 实际搭载配置**，禁跨配置取最大/剩余弹数推断）/`turret_local`+`gun_local`；位置滤波器按 AoI 在场段独立实例化（客户端 `onEnterAoI → setFilterOnEntity()` 同构），`visibility[]` additive `pose`（段首物化快照种子）⇒ 重入车辆 `pos/hull_yaw/pose_kf` 与炮口锚点数值修正，消费端零改动。 |
| v0.3.14 | type=5 昵称去掉 30 字节上限（长度域放宽到 u8 全域 `1..=255`，保留 len==0 拒绝 + UTF-8/控制字符校验）——修超长全角昵称导致阵营/车型联表失败。纯解码修复。 |
| v0.3.13 | **渲染位姿关键帧折线 + 弹道折线**：`vehicles[].pose_kf`（additive；贪心走廊拟合，消费端线性插值即复现客户端滤波器画面，修 0.1s 网格混叠顿挫）；`shots[].via`/`leg_secs`（跳弹/穿透续段折线，`flight_secs`/`end_time` 与之同刻闭合，归属 = (shooter, shotId) 链 + method20 配对，无相关性匹配）。 |
| v0.3.12 | **可破坏地形事件切面**：`destructible_areas`（区域实体锚点）与 `destructible_events`（时刻/类别/槽位/倒向；槽位与区域格子联表 = 唯一物体寻址）；配套资产管线（36 图清单 + lka serverId、scenery GLB 的 D_ 损毁态网格）；场景变体组打标（`mdVariant`/`variantByMapId`，消费端按对局 map_id 剔除非本组节点）。 |
| v0.3.11 | 基地占领中断归零（proto3 零值省略判据）+ 单基地存在性补正（目标族出现即真）；删除 `aim_frames`（零消费方，占回放 JSON 57.6%；原始 type39 帧保留给射击复现，`PlaybackData.version` 保持 2）；`vehicles[].hull_roll`（additive）。 |
| v0.3.10 | 射击复现多 interaction 关联修复：`unique shotId = 一次开火 = 一个 Shot`；优先 `method8.hash6 ↔ type32.hash6` 令牌关联，证据不足 fail-fast。 |
| v0.3.9 | 装填数据补齐：`reloads` 相位语义定稿（m0x30 subtype 15/16/17）+ additive `reload_effective`（0x23 = 当前生效完整装填配置时长）。 |
| v0.3.8 | `roster_complete`（结算阵容与战绩账号集合一致性门禁）与 `author_vehicle_codename`。 |
| v0.3.7 | 原始未滤波位姿（type=10 `poses`）与炮塔观测（prop2 `turrets`）——切面 0.1s 网格是渲染滤波输出，不能当位置证据。 |
| v0.3.6 | prop3 血量属性广播（录像者自身血量的唯一来源，与 method1 非镜像）。 |
| v0.3.5 | 原始 HP 证据 `Damage.hp_raw`（未钳制 u16，区分零血与终态未知）与 AoI 开段物化 HP；method8 原始命中通知（全变体、不分类）。 |
| v0.3.4 | 包流自行分帧（不再依赖 crate 对 payload 的反序列化——单个 pickle 形状偏差不再让整场失败）。 |

### 数据面

- **运行期 11+1 项数据已全部由本机客户端解包 / 自产承接**（含 `tanks.pb`/`models.pb` 与
  封面图换源转正）：外部依赖只剩 WG 战绩 API（性质不同不可本地化）与 BlitzKit
  "交叉校验对象"角色（两处缺失兜底休眠）。逐项来源 / 替代完成度见
  [data-inventory.md](data-inventory.md) §一、§五；解耦口径见 [decoupling-status.md](decoupling-status.md)。
- **游戏版本**：`release/11.21.0`，车辆 **738**（`data_version.json` 为准）。随版刷新链
  （pb / tank_cache / game_data / 图标 / GLB / 悬挂 / 履带印 / shellKinds）与新增
  collector 车清单见 [data-inventory.md](data-inventory.md) §一。
- **资产包与 COS**：包内 **5736 件 / 4177.7 MB**（`release/asset_pack/`，manifest 逐文件
  sha256）；COS 桶与本地**逐条一致、同代**（包内 == 归档 == 线上）。包布局、发布链
  （合成 → `--refresh-manifest` → 差分上传）、上传判据与全部运维陷阱见
  [data-inventory.md](data-inventory.md) §2.1 与 [game-data-sources.md](game-data-sources.md) §5.2。
- **俯视烘焙（均衡档底图）**：渲染件必须与线上同代（世代护栏：meta `ground` 键判据，
  旧世代 `--write` 拒写）；"先归档再合成"的次序约束与重烘命令见
  [game-data-sources.md](game-data-sources.md) §5.5。

### 回放切面（`PlaybackData` v2 / `AiReviewFacet` v1）

切面版本不变、增量字段一律 **additive + 缺省省略**；证据缺失 fail-closed（Option /
哨兵保留 raw，禁猜）。当前能力面（详情见 [总集 §八](回放与射击逆向总集.md#八提取链消费地图字段级回退与质量标记)
与 [契约 v2](replay-contract-v2-supremacy-type39.md)）：

- **车辆配置**：`config_idx` / `burst_size` / `turret_local` / `gun_local`（comp blob 证据链）；
- **位姿与弹道折线**：`pose_kf`、`shots[].via`/`leg_secs`、`hull_roll`；
- **血量与伤害**：`damage = 服务器结算值`（method8 尾部 u16；未结算时降级 HP 链并置
  `dmg_unattributed`）；`hp_raw`、prop3；
- **动态装甲**：`armor_states` / `shots[].target_armor_state`（propID 16，动态车 6 辆）；
- **复活模式**：`death_times`（≥2 死才输出）/`resurrection_times`（契约 §8；绝对血量
  回升前提待真实样本验证，未验证时字段缺省 = 现状行为）；
- **独立炮塔**：`detached_turrets`（契约 §7）；
- **可见性/装填/可破坏物/争霸**：`visibility[]`（AoI 段 + pose 种子）、`reloads` +
  `reload_effective`、`destructible_areas/events`、`supremacy_bases/points`（契约 §1/§2）。

### 跨仓协作现状与未完项

- 本项目是 WotbTools 的**上游回放解析器**（其服务端解析器已整体删除）；协议/契约问题
  在本仓先修，对方再更新 pin。消费方式、pin bump 与 golden 重生成、Android `wotbVersion`
  同步规则见 [AGENTS.md](../AGENTS.md) 与 README。
- 未完项（消费侧接入归 WotbTools 仓，落地状态以对方仓为准）：
  - 复活轨道字段的消费（scene fallback 与 canonical `lifeTransitions` 支持 DESTROYED→ALIVE 回转）；
  - 地图 scenery extras 的消费接线（`decalTexture` / `decalSeparateLm` 等，后端数据已随包）；
  - 对方点名要的字段：`PlaybackData.damages[]`、`coverage`、`finish_reason`、
    `unsupported_damage`（数据已在 `timeline.hp_events` 等的投影工作）。
- 解析侧遗留未定项（禁猜、封存）以 [总集 第五篇](回放与射击逆向总集.md#第五篇-遗留未定项) 为准；
  前端侧未做项（自动曝光、水面 HIGH/ULTRA 反射、雾=非目标等）见
  [map-render-align-audit.md](map-render-align-audit.md)。

## 约定

- 逆向结论的**唯一权威**是 [docs/回放与射击逆向总集.md](回放与射击逆向总集.md)；其余文档与其冲突时，
  先查 [wotbtools-cross-reference.md](wotbtools-cross-reference.md) 是否已有裁决。
- 历史推导过程不在工作区文件中保留，需要时查 git 历史。
- `examples/`（逆向探针脚本）与 `tmp_*/`（分析转储）不入库，文档中提及处仅作证据出处记录。
