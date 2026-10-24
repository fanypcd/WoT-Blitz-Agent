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
| [docs/game-data-sources.md](game-data-sources.md) | **数据来源权威表**：每份数据取自 BlitzKit / 本机客户端 / WG API / 自产；本地提取链、间隙甲 spaced 规则、DVPL 校验契约、DLC 覆盖层、资产 origin 发布流程（Alibaba；COS legacy）与运维陷阱、俯视合成朝向契约 |
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

- 版本源 = `Cargo.toml` 的 `[workspace.metadata.release].version`（现值 **v0.4.11**）。
  该值变更合并到 `main` 后，Release workflow 自动测试、构建、创建 `v<version>` tag
  并发布 `wotb-replay-wasm-v<version>.zip`；发布策略与 Android 同步规则见
  [AGENTS.md](../AGENTS.md)。
- 消费方式：WotbTools 经 `deploy/agent/source.json` 按 commit + sha256 锁定 WASM，
  pin bump 时重生成 AI 投影 golden（provenance 字段变化；**行为变化才动字段值**）。
- 面向消费方的版本记录（v0.3.4 起）：

| 版本 | 内容 |
|---|---|
| v0.4.11 | **射击复现匹配与配件解析修正**：①其他玩家/作者射击在 method8/method38 绑定后按 HP 终态剔除“受击方开火前已死亡”的残骸命中（`ShotQuality.target_dead`，0.1s 容差；样本 7 发），不再发布为活目标；`OtherShotsExtraction.skipped_dead_target` 统计。②Type5 loadout 的 item 描述符数量变长（实测 3..=7，含 `0A 04`），且装备检测不再依赖 `0A KK` 描述符块——只认 `0B` 配件串 + `0C 00 0D` 帧尾；描述符块缺失时 `items` 为空但 `equipment` 仍采集（校准弹 103 正常识别，XM551 #30 实测）。③`game_hit_result=4` 保持 raw（消费方统一「结果未定（4）」，不得命名跳弹/履带/击穿）。④`collision_boxes` 四盒直读客户端 Parameters YAML（hull/turret 选段仍用 models.pb 模块→节点映射；数值不变，非 WASM 行为）。⑤`PenetrationRequest` additive `require_primary`（默认 false；射击复现传 true）。**消费方**：WotbTools pin bump 后 `shots[]` 的残骸命中、`target_dead`、`skipped_dead_target` 与射手装备可见；需按新产物重生成 AI 投影 golden；前端射击复现路径使用 `require_primary`。 |
| v0.4.10 | **车辆集合判据修正 + 装甲厚度序列化精度**：①`vehicles[]` 入选改为 **Type5 class=2 实体类型 ∧ 结算花名册联表成功 ∧ 有 type=10 位姿**，删除 `st10 ∧ prop2` 双流交集、20 采样阈值与 prop2 退化分支——修复炮塔全程不转的敌方车辆整条丢失（2026-10-19 训练房 SU-130PM 样本，709 条 type=10、0 条 prop2），同时排除相机/区域可破坏物/炮塔残骸与未进结算花名册的观察者/中途离开实体。影响面：`vehicles[]`、`visibility[]`、时间轴 `meta.t_start/samples/duration`、`shots[]` 联表兜底；切面 v2 形状不变，WotbTools 随 pin bump 直接受益。②`tank/{id}.json` 的 plate/track 厚度序列化从 0.1mm 改为 0.01mm，保留 6.35 等真实板值（旧口径生成 6.3，HE/判定少 0.05mm）；738 辆数据重生成、manifest 刷新，8 个字节变化的 tank JSON 已差分同步 COS（730 个按 sha256 跳过）。消费方随 pin bump 重生成 AI 投影 golden：车辆集合修复带来的字段值变化属预期。 |
| v0.4.9 | **血量终态分类字段（additive）**：`AiReviewFacet` 的 `damage`（method1）与 `health`（prop3）事件新增 `hp_state`（`current`/`zero`/`terminal`，**恒在场**）——血量 u16 的**唯一权威分类**（规则 = `hp_terminal_normalized`，int16 ≤ 0 全域；实测终态族 0xFFFF/0xFFFE/0xFFFD，禁按分布裁剪），消费方判死亡面读它、禁自行解释 `hp_raw`。动因：消费方 canonical 只认 {0x0000, 0xFFFD}，−1/−2 档的弹药架殉爆被判成存活（血量冻在最后正值、3D 装填条继续外播；实例 1910 局 Intotherainy `0xFFFE`）。**消费方适配**：字段 additive、旧产物缺省即旧行为；**要覆盖 −1/−2 档死亡必须消费 `hp_state`**（缺省路径保持 legacy 分类——仅 {0x0000, 0xFFFD} 终态，禁在缺省上放宽）；pin bump 后 AI 投影 golden 预期仅 provenance 变化（新字段不进投影产物）。`PlaybackData` v2 / `AiReviewFacet` v1 形状不变。裁决与迁移清单见 [wotbtools-cross-reference.md](wotbtools-cross-reference.md) §七。 |
| v0.4.8 | **复活模式生命轨道（additive）+ method8 结算伤害直读 + 伤害归属确定性化 + 争霸残留占领清理**：①`VehicleTrack` 新增 `death_times`（**≥2 死才输出**）与 `resurrection_times`（**非空才输出**）——复活模式（爆燃等）同 eid 死而复生不再被判终态；普通回放 JSON 零扰动，消费方按"字段缺省 = 死亡终态"处理即行为逐位不变。②`shots[]` 伤害改**直读服务器结算值**（method8 直击通知尾部 `[u16 伤害][u8 状态]`；状态 ≠0 才已结算、overkill 已封顶），作者/他人两路径同优先，HP 链降级为无结算值时的兜底；22 场对账仅 3 处变化且全部为修正。③**伤害归属确定性化**：逐事件采样（删除"同钟异值整组丢弃"）+ method8 命中时钟精确配对，配不上 → 0 + `dmg_unattributed`（fail-closed）；22 场仅 5 发变化、全部对账。④已归属基地的占领中断后**清空残留进度**（保留归属；新增合成回归与公开契约）。⑤clippy `-D warnings` 清零（纯机械等价替换）。**消费方注意**：`shots[]` 的数值/归属与 HP 链兜底路径有可见变化（皆 bug 修正），WotbTools 随 pin bump 须重生成 golden 并逐条归因（复活字段则无需适配，缺省即旧行为）。切面 v2 形状未变。**附带（工具/资产面，非 WASM 行为）**：2D 战术底图改 pack `ground.webp` 派生 + COS `map2d/` 免发版通道（索引 schemaVersion 2 带世界框/取向几何契约）+ 客户端 HUD 小地图取景实测记录。 |
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

### 射击复现匹配与配件解析（2026-10-23）

- **死亡目标/残骸命中剔除**：其他玩家路径在 method8 绑定后按 HP 事件链检查受击方开火前
  是否已死亡（0.1s 容差吸收时钟量化，同 tick 击杀保留）；已死亡 → 按 miss 处理
  （`target_eid`/`target_name` 置空，保留 method20 弹道/地形落点），新增
  `ShotQuality.target_dead`（skip-when-false）与 `OtherShotsExtraction.skipped_dead_target`。
  作者路径同时检查 method38 `TARGET_ALREADY_DEAD (0x0002)` 与 HP 终态。修复内容：
  GB109 样本 6 发、GB48 1 发残骸命中不再发布为活目标（如受击方为已死亡的 P46）。
- **Type5 loadout 描述符数量**：`0A KK` 的 KK 实测 3..=7（旧实现只接受 6..=10，
  少带消耗品/给养的玩家整条 loadout 被丢弃，射击复现无法自动识别校准弹）；
  解析放宽到 1..=10，观察者/非战斗实体由 eid/class/结算花名册过滤，不按 KK 猜身份。
  **装备检测不再依赖 `0A KK` 描述符块**：只认 `0B <n>` 配件串 + `0C 00 0D` 帧尾，
  描述符块缺失/数量不认识时 `items` 为空但 `equipment` 仍采集（XM551 #30 实测
  `0A 04` + 9 件串；校准弹 103 正常识别）。
- **`game_hit_result=4` 保持 raw**：按
  [wotbtools-cross-reference §三.2](wotbtools-cross-reference.md) 裁决，4 的语义未定，
  不得命名跳弹/履带/击穿；消费方（WotbTools）显示「结果未定（4）」并从结果对照中排除。
- **射击复现无主装甲链门禁**：`PenetrationRequest` additive `require_primary`（默认 false
  保持 BlitzKit 相机点击语义；射击复现传 true）。无 hull/turret/gun（0mm Primary 仍算）
  且非 HE/非跳弹时，末层履带/炮管/间隙甲穿透不再返回 PEN + 全额伤害；Rust `/api/penetrate`
  与 WotbTools `penetration.js` 同口径。
- **碰撞数据口径（J20 #77 排查）**：`collision_boxes` 四盒**直读客户端 Parameters YAML**
  （hash6 量化框；hull/turret 的选段用 models.pb 模块→节点映射，数值与 YAML 逐值相等），
  `collision.glb` 是 CollisionMeshes `.sc2/.scg` 的装甲三角网格——两者都是客户端解包、
  **不是 BlitzKit**；部件盒不是网格 AABB（738 辆中 287 辆完全相等，其余主要在 +z 差，
  中位 20.8mm）。射击复现盒缺失改为 fail-closed，不再按网格自算 AABB。J20 #77 的 27.5mm
  盒-网格差不是选板原因：换 GLB 盒重跑仍为 plate3/RICOCHET，且全量换框会让首发板匹配
  488→457、部件匹配 577→502。详见
  [回放与射击逆向总集 第四篇 §6.3「固有差异界限」](回放与射击逆向总集.md) 与
  [game-data-sources §2.4](game-data-sources.md)。
- 行为变化随本轮 **v0.4.11** 发布 WASM；WotbTools 需 pin bump 后生效。

### 文档勘误与代码注释体检（2026-10-24，纯文档/.gitignore，无行为变化）

全仓文档对照现实逐条核查后修复过期事实（权威口径以被引文档为准），并完成两仓代码注释/
工作区体检。**修改面**：仅 docs/、README.md、.gitignore；Rust/Python 代码零改动
（clippy `--workspace --all-targets` 零警告，无需动）。
- **数字过期**：v0.4.7→v0.4.11（data-inventory §2.2）；有意分歧车辆 23→**111 辆**、
  `model` 等价 733/735→**623/735**（data-inventory §一 #8、decoupling-status §1、
  local-model-export §0/头部、feasibility-glb 头部——与 local-model-export §0 表对齐）；
  悬挂 725/735→**728/738**（tank-suspension-client-re §七）；t11 线上 release 名补
  `-destructibles` 后缀（data-inventory §1b）。
- **COS→Alibaba 残留**：local-model-export「工具链（发布链）」的 COS 上传命令改为指向
  Alibaba origin 流程（game-data-sources §5.2）；「包与 COS 随发」措辞更正（local-model-export
  头部、decoupling-status §1）；map-lighting-plan §九 加留档注记（清单不回勾、COS 链已废弃）；
  feasibility-track-marks §二「COS 未同步」按其 §五实施状态回写。
- **自相矛盾/指向漂移**：game-data-sources §5.6 删「Android 再走 ≤1024² 派生」（与本节结尾
  及 data-inventory #10b 矛盾）；map-render-align-audit TL;DR/§三 计数 23/19→**33**（后续追加
  B6-B8/D3-D8/P3 未回写）、重复编号 D5（贴地贴花）改 **D8**；README「契约第 6 节」失效引用
  改指 replay-wasm lib.rs 文档注释 + WotbTools contracts/agent/replay-facets-v2.md；
  「总集 §6.3」歧义（两处 §6.3）标注为第四篇；decoupling-status 冻结故障行指向 §5 的死链改指
  data-inventory §一 #4。
- **注释体检结论（两仓）**：本仓 Rust/Python 注释纪律良好——零被注释掉的死代码、零过期
  TODO（仅 2 处有效 TODO/占位）、横幅仅存在于验证探针 binary 且承载分节语义，**不做删减**
  （注释均为口径/理由载体）；WotbTools（对照 origin/main）frontend+common 零 TODO/FIXME、
  零死代码块、无违规归档目录，其 pin `deploy/agent/source.json` 已是 v0.4.11 无需 bump。
- **.gitignore**：补 `tmp_wi_probe/`、`tmp_pw/`（`tmp_*` 通配只覆盖 txt/json，两个探针目录
  此前以未跟踪状态挂在 git status 里）。
- **遗留提示**：本地 `tmp_analysis/`(5.4G)+`tmp_pw/`(499M)+`tmp_wi_probe/`(155M) 共约 6G
  调查产物可按需清理；WotbTools 本地检出落后 origin/main 31 提交（分支工作已全在 main），
  其本地 `common/fixtures/tournament/csv/{4x2,5x3}.csv` 的未提交改动仅为行尾符差异。

### 数据面

- **装甲厚度序列化精度（2026-10-20）**：`tank/{id}.json` 的 plate/track 厚度清洗从
  0.1mm 改为 0.01mm：去掉 f32 尾差（62.4000015 → 62.4）的同时保留 6.35 等真实板值
  （0.1mm 会把它舍成 6.3，HE/判定少 0.05mm）。738 辆 `data/tank_data` 已重生成，
  包内 `tank/` 与 manifest 同步、COS 已按 `--only tank/` 差分上传。
- **地图加载正确性（2026-10-18，导出器 + 消费端）**：`scenery.glb` 新增语义 extras
  `sky`（客户端 `Skyobject` 材质）与所有 BLEND 材质的 `alphaBlend`；变体映射不完整时
  写 `variantMappingIncomplete`。消费端据此不再按节点名猜天穹、不再把真混合改写成
  不透明+裁切；多变体地图无可用 `map_id` 映射时 fail-closed 隐藏全部变体节点；
  `terrain.json` 缺 `terrainLayout`/`size`/`zmin`/`zmax`/`span` 或 `worldBounds` 非居中
  正方形时 fail-closed 回退 2D（不再按缺省 layout/zmax 解码）；资产 404/解析失败写入
  `assetIssues`/`assetDegraded`，不再静默计为完成。36 图本地 GLB 全量重导已验证：
  每图 1 个 sky 材质、0 个 BLEND 缺 `alphaBlend`、0 个变体映射不完整。
- **运行期 11+1 项数据已全部由本机客户端解包 / 自产承接**（含 `tanks.pb`/`models.pb` 与
  封面图换源转正）：外部依赖只剩 WG 战绩 API（性质不同不可本地化）与 BlitzKit
  "交叉校验对象"角色（两处缺失兜底休眠）。逐项来源 / 替代完成度见
  [data-inventory.md](data-inventory.md) §一、§五；解耦口径见 [decoupling-status.md](decoupling-status.md)。
- **11.22 tier XI 超测预览包（2026-10-22，非现役数据面）**：公开 CDN 的
  `WotBlitz_11.4.0.119_t11b.zip`（`version.txt` = `11.22_supertest_tier11_4`，20.67 GB，
  支持 Range）完整实装了两辆 XI 车：中国 **BZ-79**（15153）与苏联 **Object 432U**
  （27905）；科技树另登记 6 个 XI 节点，但无车辆定义/主模型。已构建独立预览包
  `release/asset_pack_t11_preview/`（11.21 的 738 辆 + 2 = 740，含 `tank/`、GLB、图标、
  `game_data/`、悬挂/履带印），并生成 **30 件增量** `release/asset_pack_t11_preview_delta/`
  （14.9 MB；其中 8 件是线上 `initial-20261019` 缺失的 0.01mm 精度补差）。WotbTools 端无需改代码（tier 过滤/标签数据驱动）；**已作为
  `releases/t11-preview-20261022` 上传 Alibaba origin**（公网 740 辆、
  `deploy/assets/verify.sh` PASS；旧 `initial-20261019` 保留回滚）。2026-10-22 的
  可破坏物修复已在其上发布 `t11-preview-20261022-destructibles`（见下），
  `t11-preview-20261022` 仍保留回滚；本地验收仍可走
  `?assets=http://127.0.0.1:8124`。来源、口径与
  注意事项（超测属性、图标占位、BZ-79 弹种 `<kind>` 冲突）见
  [game-data-sources.md](game-data-sources.md) §2.7；预览包不进入 11.21 运行期数据面。
- **游戏版本**：`release/11.21.0`，车辆 **738**（`data_version.json` 为准）。随版刷新链
  （pb / tank_cache / game_data / 图标 / GLB / 悬挂 / 履带印 / shellKinds）与新增
  collector 车清单见 [data-inventory.md](data-inventory.md) §一。
- **可破坏物 `(cell,slot)` 联表修复（2026-10-22，资产管线）**：`parse_lka` 只认
  `er*` 分段表，黄沙荒漠（`dt1/dt2/dt3`）等 8 个多变体 space 回退到改版前遗留主表；
  主表实体 id 被复用，同一 `(cell,slot)` 错联到别的物体（desert_train 178 棵树中 27 棵
  指向非树物体、11 棵是 `bld_desert_house_*`，即「树倒演成房屋倒下」）。改为**通用前缀
  发现 + 按 map_id 序数选本变体分段表**，并加结构自检（分段表每个 key 必须在场景、
  格子码必须等于实体位置；不满足 fail-closed）。全库 9 个 space 复核：分段表
  100% 可解析且格子码一致；主表 desert_train 47% 悬空。7 个映射确有差异的变体在包内
  物化独立 `map/<key>/`（共享资产硬链接，index 指向本 key），其余 12 个变体共用基础
  目录。已发布 `assets.wotbtools.com` 新 release **`t11-preview-20261022-destructibles`**
  （manifest **5891 件 / 0 mismatch**、`deploy/assets/verify.sh` PASS；旧 release 保留
  回滚），WotbTools 无代码改动。
- **资产包与资产 origin**：本地 11.21 包 **5877 件 / 4415.6 MB**（`release/asset_pack/`，
  manifest 逐文件 sha256；含 7 个变体目录）。生产 origin 已迁移到
  **`https://assets.wotbtools.com`**（Alibaba `ali-sh-asset-01`，
  `/srv/wotb-assets/releases/<pack-id>` + `current` 原子软链）；当前线上 release
  **`t11-preview-20261022-destructibles`**（740 辆预览数据 + 本次可破坏物修复，
  manifest **5891 件 / 0 mismatch**）。发布按「新 release 硬链接复制 + 增量覆盖
  （`cp --remove-destination` / `os.replace` 断链）+ 主机重算 manifest + 校验 +
  原子切 `current`」；**不要再走 COS 根目录全量上传**——COS 仅保留旧 APK 与临时回滚，
  本次未动。发布链、增量发布与运维陷阱见 [data-inventory.md](data-inventory.md) §2.1
  与 [game-data-sources.md](game-data-sources.md) §5.2；历史 COS 同步记录保留在版本条目中。
- **俯视烘焙（均衡档底图）**：渲染件必须与线上同代（世代护栏：meta `ground` 键判据，
  旧世代 `--write` 拒写）；"先归档再合成"的次序约束与重烘命令见
  [game-data-sources.md](game-data-sources.md) §5.5。
- **2D 战术底图换源（2026-10-16）**：29 张底图（web 与 Android 共用的 canonical）改由包内
  `ground.webp` 派生，冻结配方 + 取向由契约确定（无需翻转），工具 `tools/export_2d_basemaps.py`
  （`--verify` 全量重算；与用户验收版逐字节一致）。总量 21.50 MiB；WotbTools 侧 Android 派生
  **Web 与 APK 共用同一份字节**（不再派生缩小副本，2026-10-16 决定）且随包版本号已递增。色阶宽度差为逐图属性，按仓规不做逐图配平。
  同时产出 **COS 免发版通道**：包内 `map2d/<code>.webp` + `map2d/index.json`（逐图 sha256 判据），
  App 侧"包内兜底 + 在线校验 + 离线可用"（下载字节 sha256 校验、失败回落包内、索引比包内旧不降级）。
  链条、次序陷阱与纪律见 [game-data-sources.md](game-data-sources.md) §5.6。
- **坦克 GLB：`hide_elements` 非激活形态保留在备用 scene、默认不显示（2026-10-17）**：
  10625 CC1 Mk.2 的 `turret_01_nc_skin` 是**传奇涂装的部分组件**（不是完整独立形态）；脚本
  `ScenarioSkinSwitch_turret_01` 在涂装状态激活时对**主体** `turret_01_nc` 施加 `-6.5°` /
  `(0,-0.16,0.146)`。默认 scene 只挂激活态；非激活形态保留在 glTF 备用 scene
  **`wotb_inactive_states`**（scene index 1，节点 `extras.wotb.defaultVisible=false`），
  标准查看器只加载 `gltf.scene` ⇒ **默认不显示、数据不丢**。**92 辆**（173 个多状态容器）
  重导，643 辆 model.glb + 735 辆 collision.glb 未命中车零变化；模型批后本地包 5766 件 /
  4200.4 MB（旧包 4200.3，+0.1 MB；随后逐炮塔/逐主炮 tank 数据补丁 +0.6 MB，整包 **4201.0 MB**），
  manifest 已重算，**COS 已同步**（partial publish 92 条 glb，远端 10625 `1031e5ad…`，
  与本地 0 hash diff）。口径、脚本与受影响集见
  [local-model-export.md](local-model-export.md) §5.1。
- **装甲查看器：逐炮塔/逐主炮 plates 透出（2026-10-17）**：10625 CC1 Mk.2 的 stock
  `turret_01` plate 11 = **130 mm 主装甲**（`turret_spaced=[]`）；top `turret_02` plate 11 =
  **30 mm 间隙甲**（`turret_spaced=[11]`）。`tank/{id}.json` 的 `configs[]` 新增 additive
  `turret_plates`/`turret_spaced`（逐炮塔）与 `gun_plates`/`gun_spaced`（逐主炮，
  均来自 models.pb；`gun_thickness` 已有）；顶层 `armor_model` 仅保留为 tank 详情卡的
  顶级摘要。738 份 `tank/{id}.json` 重生成并上传 COS（partial publish 738 条，0 hash diff；
  整包 4201.0 MB）。WotbTools `frontend/src/scene/tankViewer.js::getPlateThickness` 对
  turret/gun 只读当前 config 的 plates，`gunBarrel` 只读 `config.gun_thickness`；字段缺失
  按 0/空 **fail-closed，不回退 `armor_model`**；切 config 时
  `refreshConfigArmorThickness` 重算 turret/gun 的 `armorThickness`。数据字段 additive；
  尚未部署的新查看器代码仍走旧的 `armor_model` 摘要路径，部署后消失。
- **SpeedTree 静态叶过亮修复（2026-10-18，导出器 + WotbTools 前端）**：客户端
  `speedtree-materials-vp.sl:236-285` 的 SPHERICAL_LIT 净染色 =
  `0.282094 × SH(L0) × vertexOcclusion × shOcclusionMult`，其中
  `vertexOcclusion = aoContrast × (COLOR_0.r − 0.5) + 0.5 + aoBrightness`。旧导出把
  `st_tint` 写成 SH 字面值（√π = 1.7725），且只给叶卡/混合批刚体子集写 COLOR_0；
  纯静态组（`pivot.w=0` 固定叶，如 DesertSands 棕榈 `palm1_dt`/`palm2_dt`）整批丢遮挡
  ⇒ 消费端 `vOcc=1`，叶净值 1.7725 vs 客户端 COLOR_0（叶均值 0.722–0.733），约 **2.4×
  过曝**（旧口径下 **57.8% 可见叶像素在回放侧近白**，客户端不白）。修复：`st_tint = 0.282094×SH×shOcclusionMult`
  （√π 且 aoMult=2 → 1.0；每树彩色 SH 逐通道同式）、**所有 ST 组静态网格随导 COLOR_0**
  并按客户端 AO 对比度/亮度烘焙；legacy 分支染色式不变（SH 不参与）。36 图 scenery 全量
  重导（其中 15 图字节变化），本地包 5766 件 / 4201.1 MB、manifest 已重算；**COS 已局部
  发布**（`--only map/` 15 条变更 scenery，511.7 MB；远端 `files` 与本地 0 hash diff，root
  仍 `58a891e`）。WotbTools 同步改 `sceneryMaterials.js::makeBillboardMaterial`：
  `occMean=1`（新包哨兵）时 COLOR_0 已是 `vertexOcclusion`，去掉 1.25 亮部上限；旧包路径
  不变。数据级实测 DesertSands 棕榈叶 viewer/client 净值逐通道一致（0.722/0.733），
  旧口径的 57.8% 近白差异归零；全库 1284 个 ST 网格均带 COLOR_0。影响面：36 图所有 SpeedTree 几何
  （全库 28,411 个 speedtree 批次）及消费该包的 WotbTools/Android；WotbTools 侧属
  scene-runtime，PR 需按 Android version rule 同步 `wotbVersion`。回归：
  `tools/test_export_speedtree_ao.py` 6 项、既有导出器单测；WotbTools
  `sceneryMaterials` 78 项 + `test:shader-check` 24 程序 0 失败。

### 回放切面（`PlaybackData` v2 / `AiReviewFacet` v1）

切面版本不变、增量字段一律 **additive + 缺省省略**；证据缺失 fail-closed（Option /
哨兵保留 raw，禁猜）。当前能力面（详情见 [总集 §八](回放与射击逆向总集.md#八提取链消费地图字段级回退与质量标记)
与 [契约 v2](replay-contract-v2-supremacy-type39.md)）：

- **车辆集合（2026-10-20 修正）**：`vehicles[]` 的入选判据 = **Type5 class=2 实体类型
  ∧ 结算花名册联表成功 ∧ 有 type=10 位姿**（协议权威实体种类；class=3 区域可破坏物、
  class=8 炮塔残骸均非车辆；未进入花名册的观察者/中途离开实体不渲染——用户口径
  "只保留结算花名册中出现过的"）。旧口径 `st10 ∧ prop2` 双流交集把"炮塔流"当成车辆
  身份证据——车辆全程不转炮塔时整场没有 prop2，被误判为非车辆：用户样本
  `20261019_0237`（Copperfield 训练房，作者 Grille 15 vs 对手 SU-130PM 静止未瞄炮）
  旧口径只输出 1 辆车，对手实体 `0x10cddb90`（709 条 type=10 位姿、0 条 prop2）
  整条丢失，3D 回放无法显示。反向错误同样存在：旧"prop2 全局缺失 → 仅 st10"退化会把
  无 Type5 class 的相机实体当成第二辆车（`20261010_1212` 训练房样本，相机流位置随后
  跳到 `(0,0,0)`）。现口径同时移除 20 采样阈值（稀疏采样不是"非车辆"的证据）。
  影响面：`vehicles[]` 集合、由候选集派生的 `visibility[]` 与全场时间轴
  （`meta.t_start/samples/duration`），以及 `shots[]` 的射手/目标联表兜底；切面形状不变，
  WotbTools 3D 回放随 pin bump 直接受益，无消费方适配。
- **车辆配置**：`config_idx` / `burst_size` / `turret_local` / `gun_local`（comp blob 证据链）；
- **位姿与弹道折线**：`pose_kf`、`shots[].via`/`leg_secs`、`hull_roll`；
- **`pose_kf` 段首显示重建（2026-10-18）**：客户端 `setFilterOnEntity` 复位后 latency 从
  0.02s 缓慢收敛到 0.2s，段首 2~5s 的逐帧输出是 `保持→整段跳变` 阶梯（本次逐帧探针复现：
  某 AoI 段首 2s 内 **57/119 帧静止**，p90 0.58 m/帧；喂入策略 `全部已到样本` 与
  `只喂最新` 逐帧一致，排除输入侧）。游戏内这段阶梯被模型加载/入场遮挡，我们的查看器
  模型常驻，表现为"对方坦克刚进视野一顿一顿"。渲染折线改由
  `SegmentedPoseTimeline::build_display` 生成：每个在场段按 10Hz 标称间隔铺满 8 槽、
  latency 从稳态 0.2s 起步——段首仅 0.2s 静止，其后逐帧匀速（样例段首 0.4~1.5s 位移
  0.1666~0.1667 m/帧，原客户端语义 37/60 帧静止）。**判定锚点 / 炮口 / 命中位置仍走
  `SegmentedPoseTimeline::build`（0.02s 复位，WI 对齐不变）**；10Hz 网格列
  `pos/hull_yaw/hull_pitch` 未改。同时修复短隐藏段（0.1~0.5s）下前段滤波尾部的 +0.5s
  外推越过下一段 `t_in`、导致 `pose_kf` 时刻非单调的问题（有界段在 `t_out` 截断，消费端
  二分/插值不再跨隐藏段滑移）。波及：`pose_kf` 消费方（WotbTools 2D/3D）自动变平滑；
  WotbTools 侧炮塔/炮管/侧倾网格采样同步改为段内采样（`sampleChannelWithin`，按 playback
  `visibility[]` 协议精确 AoI 段；短隐藏段在 `coverage` 中不断开，故不能用 `coverage`）；
  契约字段形状不变、无新增字段。
- **血量与伤害**：`damage = 服务器结算值`（method8 尾部 u16；未结算时降级 HP 链并置
  `dmg_unattributed`）；`hp_raw`、prop3；**`hp_state`（additive、`damage`/`health` 事件恒在场：
  `current`/`zero`/`terminal`）= 血量编码的唯一权威分类**——消费方判死亡面读它，禁自行解释
  u16（规则与 `hp_terminal_normalized` 同判；分歧实例、消费方义务与实测证据见
  [wotbtools-cross-reference.md §七](wotbtools-cross-reference.md)）。**v0.4.9 起随发布产物在场**：
  消费方须 pin bump 后消费（缺省路径保持 legacy：仅 {0x0000, 0xFFFD} 终态）；
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
  - **血量终态族消费（2026-10-16 新发现，见 [cross-reference §七](wotbtools-cross-reference.md)）**：
    canonical `hpRawState` 只认 {0x0000, 0xFFFD} 为终态，−1/−2 档死亡被判成存活（2D 血量冻在
    最后正值、3D 装填条继续播放；实例：1910 局 Intotherainy 0xFFFE）——pin bump 时改读上游
    `hp_state`（或扩全族），并给终态样本补 DESTROYED 迁移与装填条 f2=5 取消语义；
  - 复活轨道字段的消费（scene fallback 与 canonical `lifeTransitions` 支持 DESTROYED→ALIVE 回转）；
  - 地图 scenery extras 的消费接线（`decalTexture` / `decalSeparateLm` 等，后端数据已随包）；
  - **履带逐配置装甲/模型适配（2026-10-17 待办）**：`armor_model.chassis` 仍只透出顶级底盘
    `left_track`/`right_track` 厚度（models.pb top track）；`models.pb` 的
    `TrackModelInfo.tracks` 已有逐 module `thickness`/`origin`，但尚未进 `tank/{id}.json`，
    查看器 chassis 仍用顶级摘要。当前扫描 **6 辆** stock/top 厚度不同：4369 Pz. III（10/15）、
    5729 Ju-Nu（20/25）、12081 BZ-68（30/20）、18305 SDP 40 Zadymka（15/20）、
    21569 ERAC 105 Proto（15/20）、21825 Projet Louis（15/20）；另 **6721 BDR G1 B** 仅
    origin 差约 13 mm（厚度同为 20）。可视 GLB 只有一组 `chassis_track_L/R`，当前数据没有
    按履带模块的第二套网格；若客户端确有不同履带外形，需按模型语义另行确认。待办：逐 module
    透出 thickness/origin 并让查看器按当前底盘选；视觉几何差异另行确认。
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
