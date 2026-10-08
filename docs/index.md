# 文档索引

> 全部 Markdown 文档的定位与状态一览（2026-10-06 整理）。入口永远是根目录 [README](../README.md)。

## 使用文档（随项目演进，保持最新）

| 文档 | 定位 |
|---|---|
| [README.md](../README.md) | 项目总入口：项目定位、能力总览、与 WotbTools 的关系（分发形态）、仓库结构 |
| [docs/回放与射击逆向总集.md](回放与射击逆向总集.md) | 射击/回放逆向**唯一权威参考**：第一篇=协议层数据段（字节布局/语义/使用状态/死路清单/消费地图）、第二篇=客户端处理架构、第三篇=客户端弹道与命中表现（含 DecodeShotSegment 权威定义）、第四篇=WI 对照与射击复现实现、第五篇=遗留未定项 |
| [docs/replay-contract-v2-supremacy-type39.md](replay-contract-v2-supremacy-type39.md) | **回放契约 v2**：争霸基地状态（sparse 重建 + 零值省略/占领中断归零补正）+ 实时点数 + type39 原始帧用途与 `aim_frames` 删除记录、门禁与版本护栏 |
| [docs/wotbtools-cross-reference.md](wotbtools-cross-reference.md) | 与 WotbTools 逆向结论的逐条裁决记录（采纳/驳回/互证），防止误采或回退已定案；**文末附面向消费方切面的最新进展** |
| [docs/architecture-debt.md](architecture-debt.md) | 架构债与长期改动方案：已完成项（combat.rs 拆分、双路径合并）与仍留存的 tankViewer 目录拆分 |
| [docs/game-data-sources.md](game-data-sources.md) | **数据来源权威表**：每份数据取自 BlitzKit / 本机客户端 / WG API / 自产；本地提取可行性评估；间隙甲 spaced 判定规则；提取链与 COS 资产面发布流程；2026-10 game_data 冻结故障复盘 |
| [docs/data-inventory.md](data-inventory.md) | **数据面清单（谁在用 / 谁维护 / 怎么分发）**：运行期实际读取的 11 项数据及其来源与维护代码；三条分发渠道（COS 资产包 / GitHub Release 引擎 / 消费方前端常量）与包内布局；已备好但未接线的替换来源及其阻塞项；尚无代码的缺口（**2026-10-07 刷新**：包统计与逐目录清单、地图导出 + 俯视合成链路、上传工具的"同尺寸漏传 / 渲染产物随包"两个陷阱、陈旧包警告解除） |

## 方案文档（已执行完毕，留档）

| 文档 | 状态 |
|---|---|
| [docs/vue-migration-plan.md](vue-migration-plan.md) | ✅ 已完成（2026-09-28）：前端四页全部切流 Vue 3 SPA，嵌入 HTML 与 web/vendor 已退役 |

> `docs/mobile_plan.md` 及移动端（`mobile/` Tauri 壳、`mobile_assets/` 随包资产）已于
> **2026-10 随 Android 分发形态一同移除**（详见 [README §与 WotbTools 的关系](../README.md)）；
> 其文内提及的 `scripts/{asset_manifest,export_mobile_maps}.py` 同步删除。
> [docs/vue-migration-plan.md](vue-migration-plan.md) 里提到的 `scripts/package.ps1` /
> `build-all.ps1` 同样只作历史记录——桌面便携包打包链已删除，前端仅本机调试用。

## 对接消费方（WotbTools）的当前进度（2026-10-06）

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
| v0.4.0 | **回放解析确定性收口（语义/契约变更，消费方需 v0.4.0 适配——WotbTools PR #555 已同步）**：①`damage_received` 语义修正——**无证据 = `null`**（≠ 0，此前两者不可区分；Rust `Option<u32>`，旧版恒为数字）；②击杀原因统一 **255 = 未知/其他哨兵**（唯一可判定的规范值，此前未知可能落成任意缺省值）；③`infer_shots` 推断路径删除（无证据不产出）；④`PlaybackData.shots_from_loose_path` additive provenance 字段（炮弹来自宽松路径时置位）；⑤method38 权威配靶下 subtype=1 ARENA_INFO 全场 comp blob 修正——`turret_local`/`gun_local` 覆盖率 1/N → **N/N**；⑥#7–#12 遗留清零：HP seed 来源分级、comp 昵称窗放宽 1..=255、tank 白名单低 16 位 masked 匹配、decoded_target_gun_pitch 令牌优先、viewer 身份联表 eid 化（`tank_of_eid`/`comp_by_eid`）；type=28 槽位兜底 fail-closed。9 场语料全量重跑零回归 |
| v0.3.15 | **实际搭载配置透出（弹容 N 权威化）+ AoI 重入段化滤波（D1 卡顿修复）**：①`vehicles[]` additive 新增 `config_idx`（`resolve_config_index` 三级证据链钉定的 `configs[]` 下标，与 shots 的 `shooter_config_idx` 同域）/`burst_size`（该配置弹夹容量原值，0=单发；configs 唯一时无歧义直接给出）/`turret_local`+`gun_local`（comp blob 模块局部 id，纯回放证据，服务器与 WASM 双路径产出）——多炮坦克各炮弹容不同（资产包 735 台中 52 台跨配置不一致），装填条弹容 N 的唯一权威 = 实际搭载配置，禁跨配置取最大/剩余弹数+1 推断；附带修复 annotate 解析缓存键（原按 tank_id 单键共享，同车不同玩家不同炮互相串值）；②位置滤波器按 AoI 在场段独立实例化（客户端 `onEnterAoI → setFilterOnEntity()` 每次新建，原整场单实例——重入车辆先钉上一段末位 ~1s 再滑移 ~1.5s 收敛，即 3D 回放「追赶滑移」卡顿），段首以 Type5 物化快照为种子，`visibility[]` additive 新增 `pose`（pos@14/yaw@26/pitch@30）；`vehicles[].pos/hull_yaw/pose_kf` 与 shots 炮口锚点在重入车辆上数值修正（9 场 79 次重入实测：重入帧渲染位 vs 真值 med 28.4m → **0.4m**），消费端零改动；附带更正 indexes.rs 两处 0x1440C70 注释（D2 翻案：越末帧=硬保持无外推分支，0.9=稀疏 bracket 跨度阈值因子） |
| v0.3.14 | **type=5 昵称去掉 30 字节上限**（超长昵称阵营/车型联表失败修复）：真实对局存在 >30 字节的长 UTF-8 昵称（13 全角字符 = 39 字节，S16 Kranvagn 实测样本），旧 `1..=30` 长度域把合法昵称整条拒掉 → 实体无昵称 → 按昵称联花名册失败 → 该车 `team`/`tank_id` 落 0（fail-closed，前端表现为"阵营无法识别"）；长度域放宽到 u8 前缀全域 `1..=255`，保留 len==0 拒绝 + 载荷边界 + 合法 UTF-8 + 控制字符校验，fail-closed 语义不变；纯解码修复，无契约字段变化 |
| v0.3.13 | **渲染位姿关键帧折线 + 弹道折线（via/leg_secs）**：①`vehicles[].pose_kf`（additive）——位姿来源是客户端滤波器（AvatarFilter 移植）60Hz 逐帧输出，滤波器在「预测-保持」阶段（刚进 AoI 数秒内）输出为保持-跳变阶梯，旧 0.1s 网格重采样 + 网格间线性插值把阶梯混叠成速度摆动（实测某 0.5s 窗内前端线速度 4.5→29.6 m/s、全时线逐帧位置最大偏差 21m，即 3D 回放顿挫来源）；贪心走廊拟合（容差 2cm/0.4°）保留保持段两端与跳变段，点数 7~10/秒/车，消费端线性插值即复现客户端画面；②`shots[].via`/`leg_secs`（additive）——弹道折线：同一发炮弹的全部 method29 共享 (shooter, shotId)，链首 = 发射段、后续 = 跳弹/穿透续段（起点 = 装甲接触点），method20 = 弹道最终停止点（跳弹后落在出射方向延长线上）；段时长按 \|Δ\|/段速度计（不用 10Hz 包钟——跳弹常与发射同刻，用钟得零长段）；`flight_secs` = 各段之和、`end_time` = 抵达时刻（与 method8 同刻，命中归属随之修正）；确定性归属 = (shooter, shotId) 链 + method20 按 shotId 配对，无任何相关性匹配 |
| v0.3.12 | **可破坏地形事件切面**：`PlaybackData` 新增 additive 字段 `destructible_areas`（100m 格子区域实体锚点）与 `destructible_events`（时刻/类别/槽位/倒向）——类别 prop 1=fragiles 2=柱状 3=树倒、末字节 = lka 槽位（与区域格子联表 = 唯一物体寻址，逆向总集 §5.4 公式闭合，四场回放 799/801）、倒数字节 = 8 位倒向角（服务器权威，未点亮碾压者亦携带）；配套资产管线 `tools/export_map_destructibles.py`（36 图清单+lka serverId）与 scenery GLB 的 D_ 损毁态网格导出（`export_map_glb.py`/`export_asset_pack.py` 已随包）。**2026-10-06 选表勘误（逆向总集 §5.4 第七轮）**：slot 编号存在分段索引表 `blitz/<stem>.erN.lka` 时整体替代主表（两套编号体系，erlenberg 实测 829 公共键 455 个 serverId 不同 + 268 键仅在分段表；Middleburg 回放 11 事件地面真值 er0 表 11/11 命中、主表 4 MISS+6 错联）——`parse_lka` 已按新口径取表，erlenberg 数据需重导重打包。**2026-10-06 叶卡勘误**：新世代 SpeedTree（erlenberg Spruce/Linden/bush 等，92B/顶点混合批 = 刚体树枝 w=0 + 锚定叶簇 w=1，整簇顶点共享 pivot）旧导出器不识别、整批压成静态几何 → 树叶/草丛成固定朝向平面片；`decode_speedtree_card_gen2` 按 pivot.w 切分（卡=56B 同式 billboard 属性 `_CORNER`/`COLOR_0`，刚体余量走静态路径，守卫 fail-closed），30 图缓存 GLB 全量重导后随包生效。**2026-10-07 场景变体组勘误（逆向总集 §5.4）**：9 张多变体图的 .sc2 按变体（md1/dt2/er0… 标签组）挂出生点/边界/专属布景，客户端一局只激活一组，导出器全量导出使组外布景成回放幻影（Dead Rail 基础局立着 Railroad 变体的 stn_07 石头，作者出生点与 md1 SpawnTeam1_01 精确重合钉死激活组）——GLB 现随包打标（节点 extras `mdVariant` + asset extras `variantByMapId` 序数配对，组数≠key 数省略 fail-open），消费端按对局 map_id 剔除非本组节点（WotbTools `scene/variantFilter.js`），9 图 GLB 已 `--scenery-only` 重导随包生效。**2026-10-07 铁轨灰带真因（UV 平铺被幅值守卫误拒）**：`decode_group_uvs` 的幅值守卫（绝对值>64 拒绝）把合法平铺 TEXCOORD0 当垃圾拒掉——铁轨条沿轨道平铺贴图，v=-118 实测，REPEAT 采样下平铺倍数无上限——被拒后回退抓位 4 备用图集对当 UV0，该条铁轨只铺一个断面区间拉成灰带（平铺倍数小的条正常，故表现为『部分铁轨灰色』）。现只拒非有限值，平铺 UV 恢复导出，36 图 GLB 重导随包生效。附带记录：TextureLightmap 管线（materials-fp.sl：albedo × lightmap(UV1) × 2，unlit+烘焙光照图集）两次烘焙尝试分别因缺 ×2 与解出的 UV1 落在光照图集暗区而失败，均已回退，待与 UV 通道口径一并再核。**2026-10-07 叶卡颜色方程勘误**：旧实现 `min(occ/occMean×SH, 1.35)` 的 1.35 乘积钳 + occMean 均值归一化把 erlenberg 类 √π 灰 SH（应 ×1.575）压平成 ×1.35 并抹掉叶簇内 AO 明暗对比 → 树叶发灰发平（实测报障；高亮雪地贴图被 tone mapping 掩盖为 +9% 故长期未显形）；且 SH 只取 R 通道当灰度，丢掉 karelia (0.37,0.50,0.50) 冷调黄昏等真实每树彩色环境。修正：SHCoeff L0 按 RGB 三通道字面值导出（√π 灰与彩色环境都是客户端按字面乘的数据），occMean 固定 1.0（vOcc 直乘），前端 uSH 向量化 + 乘积钳 2.0（occMean=1.0 哨兵区分新旧包，旧包旧方程不受影响）。**2026-10-07 同树叶片双色勘误**：gen2 混合组的 w=0 子集是【固定朝向叶片】而非树枝（三角形尺寸与卡片叶同量级、同叶图集；如 Spruce 组 w=1 占比仅 0.25），客户端对两种叶片都乘 varVertexColor——gen2 刚体子集漏导 COLOR_0 使固定叶全亮、billboard 叶带 AO 偏暗 = 同树双色。修正：gen2 刚体网格随导 COLOR_0（VEC4）+ 前端 ST\| 静态材质透传 `vertexColors`（GLTFLoader 对带 COLOR_0 的几何自动置位，重建材质须透传；材质缓存键同步加入），36 图重导随包生效。**2026-10-07 场景 GLB 三项勘误（Naval Frontier 报障：冷杉叶片只剩零星小点、芦苇蕨丛发亮发平）**：①**材质族改按材质判定**——新增 `ClientMaterialFamily` 解析 `Data/Materials/<fx>.material` 的 `Shader:` 与 UniqueDefines（模板沿 `MaterialTemplate` ULTRA/HIGH/… 引用链取并集；`IgnoreDefines` 是"关闭"语义、不计），不再按 .sc2 实体类：SpeedTreeObject 类 + `Textured.material`（skit 芦苇/蕨、各图远景板）在客户端走普通【受光】`materials` 着色器，旧口径把它们当 ST\| 不受光 + SH 加亮；billboard 重建同样只在 `speedtree-materials` 族上做。②**ST 染色分族**：`SPHERICAL_LIT/PBR_SPEEDTREE` 族维持 SH(L0) 字面值（10-07 定版）；legacy（`SpeedTree.material`）族改用客户端公式 `color0 × treeLeafColorMul × treeLeafOcclusionMul + Offset`、**SH 完全不参与**（`speedtree-materials-vp.sl` 的 `#elif SPEED_TREE_OBJECT //legacy` 分支；skit 该组属性 = (1,1,1)/1.0 ⇒ 叶色 = albedo × COLOR0，旧口径乘 SH(1.7725) 使叶片偏亮 77%）。③**贴图嵌入按文件原始行序**：`decode_dds`/`decode_pvr3` 内部各翻一次、嵌入统一再翻回；此前只对 DDS 翻、**PVR 漏翻** → PVR 源贴图上下颠倒，叶卡 UV 窗口整个落在图集空白区（skit `skt_fir_leafs` 主窗口覆盖率 5.4% → 翻正后 35.2%、叶卡组窗口 5.4% → 20.5%）——"只剩零星小点"的直接原因；方向另由 `env_57_signs_01` 标牌贴图（原始行序为正立螃蟹）实测佐证。附带：`FLATCOLOR` 材质整图染色（客户端 speedtree-fp/materials-fp 皆为采样后 `baseColor *= flatColor`，旧实现只随 UV1 覆盖烘焙顺带应用）。影响面：36 图场景 GLB 全量重导（PVR 翻正 41 个图号、legacy 染色改判 38 个、ST 类非 speedtree 材质改回受光覆盖全部图号），地面原始重烤 + 俯视重渲合成重跑（渲染读新 GLB），包重建（4168 文件 / 3788.3MB）并同步 COS（72 对象 / 751.6MB，逐对象 sha256 回拉 73/73 一致；误传的 `overhead/` 73 个渲染对象 ~2.4GB 已清理）；`tools/upload_asset_pack_cos.py` 与 `map_index.json` 入库、本机调试产物入 `.gitignore`（包统计与上传要点见 [data-inventory.md](data-inventory.md) §2.1） |
| v0.3.11 | **基地占领中断归零 + 单基地存在性补正 + 删 `aim_frames` + 渲染网格加 `hull_roll`**：①争霸（wrapper12）全缺省行 = 显式清空、单基地（wrapper8）双缺省块 = 进度归零——wire 按 proto3 省略零值字段，中断（车辆出圈/被击毁）此前被「缺省=维持前值」吞掉，进度与占领方永久挂在基地上；②`assault_objective_present` 按字段契约放宽为「目标族出现即真」（实现此前多要求 `f3\|\|f4`，把"有目标但全程未占领"的场次整场压掉）；③删除 `aim_frames`（零消费方，实测占回放 JSON 57.6%，20 车样本 6.92MB → 2.93MB；原始 type39 帧保留给射击复现），`PlaybackData.version` 保持 2；④`vehicles[].hull_roll`（additive）取原始 type=10 最近邻——滤波层不输出侧倾 |
| v0.3.10 | **射击复现多 interaction 关联修复**：`unique shotId = 一次开火 = 一个 Shot`；作者严格路径在同 victim / 同钟出现多个 type=32 segment 时，优先用 `method8.hash6 ↔ type32.hash6` 确定关联；重复 method8 广播按 hash 去重，证据不足时继续 fail-fast，不猜选 |

切面字段均为**附加**（`AiReviewFacet` v1 / `PlaybackData` v2 版本不变）。

**工作区已合入、待发版的契约变更（2026-10-06，解析确定性改造第一批）**——目标：
全部解析输出改为回放数据确定性导出或 fail-closed（None/哨兵），移除启发式匹配与无标记降级：

1. **comp blob 全玩家提取**（P1 探针裁决：subtype=1 ARENA_INFO 单条 update 携带全场
   玩家的 comp blob，9/9 场实测 blob 数=花名册人数；旧 `parse_args` 提取首个即 return，
   实际只覆盖作者 1 车）→ `collect_comp_descriptors` 逐 blob 收集后，`vehicles[].turret_local`/
   `gun_local` 从 1/N 变为 **N/N**（9 场语料验证，昵称联表 97/98，唯一 miss 为样本花名册
   截断）；纯回放证据现随模型实体并表进入**所有**投影路径（WASM/在线 viewer/切面导出
   此前三路不一致，切面导出恒空）。
2. **`PlaybackData.shots_from_loose_path`**（additive，缺省 false 不序列化）：作者严格
   路径失败降级宽松全路径时置 true——此前降级只有一条 stderr，消费端无从区分证据等级。
3. **`PlayerSummary.damage_received` → `Option<u32>`**（unknown ≠ 0）：结算缺失时输出
   null 而非合成 0；消费端按"数值或 null"处理。
4. **`KillEvent.cause` 禁猜**：血量链 killer_eid 缺失时不再合成 0（直击）/3（环境），
   一律 255（结构体契约既有的"未获取"哨兵）。
5. **移除 `infer_shots`/"伤害最接近"匹配**（`CombatTimeline::infer_shots`、`ShotEvent`）：
   唯一的模糊配靶路径退役，射击数据一律来自 shots.rs 权威提取（method38 victim 包内
   字段 + hash6 令牌 + 歧义 fail-fast）；CLI `combat` 的 JSON `shots` 键与推断表随之删除
   （`shot_replay` 键不受影响）。
6. **replay_shot 的 `&shell=` URL 参数可缺省**：弹种解析链失败不再兜底 type=28 槽位
   快照（已知切弹竞态），3D 端按默认弹渲染。

**P2 探针阴性结论（配靶升级路线）**：method8 与 type=32 全字节域（u32+u16 滑窗）均无
shotId/发射关联字段（0/453 + 0/2692）——他人路径 launch↔hit 绑定升级只能走
hash6 几何预测。

**第二批（同日）：配置链收敛 + 身份/门控探针**

1. **证据 1/2 退役（契约变更）**：`resolve_config_index` 收敛为 **comp blob 精确对号**
   单一证据，删除"发射弹种 ⊆ 弹表 / 初始血量 ±2 容差"两级启发式与"多匹配/全空 →
   顶级"回退——comp 缺失/对号失败/configs 唯一 → None（消费端自选顶级为显示默认）。
   依据：P1 实证 comp 覆盖 N/N 后推断级证据无存在必要（fail-closed，禁猜）。9 场语料
   config_idx 输出与退役前逐位一致（零回归）；`vehicles[].shell_ids` 文档同步（不再
   是配置证据，仅回放事实透传）。
2. **P3 探针：eid→account_id 直连实证**。ARENA_INFO 玩家条目为 protobuf 结构
   `[f1=eid varint][f2=comp blob][f3=昵称]…[f7=account_id varint][f8=公会标签]`，
   84 条目解析 **83/84** 的 f7 与 battle_results 花名册 account_id 精确相等（唯一
   miss 恰为花名册截断样本，其 f7 反而是真值）。**昵称联表歧义（重名/匿名）可根除**
   ——产线化需把 CompDescriptor 提取扩展为带 eid/account 的严格条目结构并全链换键，
   列入后续。
3. **P4 探针：击杀播报结构化门**。语料上 `<首条 PERIOD=3` 门与现行 `|Δt|≤5s` 死亡门
   完全等价（52 真 / 0 误杀 / 1 双门均无法归属的 AoI 裁剪样本）。5s 门无实际误判，
   替换收益仅语义清晰——暂不改代码，结构化门留作 5s 门出误判时的既定替换方案。
4. **P2b 探针：hash6 几何预测部分验证**。u16 量化公式 `(v/π+1)×32768`（yaw，
   atan2(dz,dx) 受击者→射手方位角域）在最优样本残差仅 61 LSB（0.28°），约定与公式
   确认；残差来自探针输入粗化（最后已知位姿而非命中时刻锚点插值、发射速度而非
   重力弹道抵达速度）。产线化须复用 shots.rs `shot_legs` 弹道与锚点系统后重新验证
   ——他人路径现行就近匹配有 0.05s 窗 + hash6↔type32 令牌双重兜底，非燃眉。

**第三批（同日）：他人路径多候选配靶升级（阶段 2 综合方案）**

`extract_other_shot_replays` 的目标匹配从"无差别时间最近"升级为三级：
① ±0.05s 命中窗内**唯一候选直接绑定**（P2c 探针：567 发中 69% 唯一、29% 零候选，
   绑定唯一即精确）；
② 多候选（1.8%）按几何判别（[`select_dhit_by_inc_yaw`] 两道门：hash6 来向角自洽门
   15° 阈 + 弹道射线门——炮弹沿发射射线飞行，溅射第二目标不在射线上即被排除；
   射线偏差领先 ≥0.05 rad 才绑定）；
③ 几何判别不出（HE 溅射近点双车近乎共线——此时"一发炮弹两个受害者"本身超出
   1 发 = 1 目标的数据模型粒度）→ 回退时间最近保完整性，计
   **`OtherShotsExtraction.ambiguous_time_fallback`** 外露（web notes 展示）。
语义要点（P2b 定案）：hash6 来向角是命中事件的**自洽属性**（每个 method8 与自身
受击者几何一致），不能做归属判别；判别力来自弹道射线。9 场语料 503 发实测：回退
触发 10 次（与 P2c 双候选数逐场一致），目标/伤害归属完整性无损，错配面收窄为
"同刻近点双车"且首次有计数可见。契约：`OtherShotsExtraction` additive 新增计数
字段；4 个单测锁判别器行为。

**第四批（同日）：身份联表 account_id/eid 直连换键（阶段 4，P3 定案产线化）**

P3 探针实证 ARENA_INFO 玩家条目含 `[f1=eid][f7=account_id]` 后，`CompDescriptor`
additive 新增 `eid`/`account_id` 字段（条目起点反推 + varint 前向扫提取；任一环节
失败置 0 = 退回昵称联表，与全仓 author_eid=0 语义一致）。四个联表点全部升级为
**account/eid 主键、昵称退回**：
① `annotate_vehicle_comp_locals`（facet/WASM 的 turret_local/gun_local 标注）按
   车辆 eid 精确匹配；
② `ReplayModel::scan` 花名册联表（EntityRecord 的 account/team/tank_id）account
   精确匹配——匿名 "Anonyme" 互相覆盖问题根除；作者实体解析同源升级（arena 收集
   提前到作者解析之前，零额外扫描）；
③ `TankResolver::pitch_limits_from_battle_results` 俯仰锚定表 account 主键；
④ 各调用方（viewer/web/main dataset/playback_viewer）作者解析与 config 匹配
   account 主键。
9 场语料实测：**account/eid 提取 100%**（account 全部命中花名册，唯一 miss 恰为
花名册截断样本的真值），comp_locals/config_idx/作者解析输出与改前逐位一致（本语料
无重名，故为等价替换；重名场景的错联从"取首条"变为"按 account 精确"）。
`CompDescriptor` 结构 additive（nickname 键与既有消费者兼容）。2 个新单测锁条目
身份提取（真实 protobuf 布局 + 裸 blob 优雅置 0）。

**第五批（同日）：审计遗留项清零（#7-#12）**

1. **type=5 开局 HP 来源分级**（`src/replay/loadout.rs`，原偏移 51 兜底）：尾表
   id13 在**任意**包上都是开局值（权威）；偏移 51 仅在**首见**包上成立（重广播包
   是当前血量）——旧实现"首个非零值"会让后到重广播包的当前血量顶替首包 0。改为
   分槽记录 + 尾表优先 + 偏移 51 只取首见包；均缺失 = 0（unknown ≠ 值）。
   `events.rs::collect_initial_hp` 的 HP seed 本就只取首见包（语义正确），不动。
2. **comp blob 昵称窗放宽 1..=255**：与 type=5 侧 a3efede 同域（实测 39B 全角昵称
   存在），旧 3..=30 窗会在超长昵称场景静默丢 comp → 掉进昵称联表失败。60B 扫描窗
   实际覆盖 ~42B，与实测同量级；语料内无超长样本（无回归面）。
3. **comp 白名单按低 16 位匹配**：blob 只携带低 16 位 tank_id，旧 `contains` 在
   tank_id 超出 16 位域时静默丢 blob——与各消费方的 masked 校验同式，未来 id 域
   扩展免疫（当前语料全 <65536，无回归面）。
4. **受击方炮管俯仰解码令牌优先**：`decoded_target_gun_pitch` 增加 `prefer` 参数
   （method8↔type32 hash6 令牌已绑定的警告），通过同样的 15°/30° 双校验直接采用，
   免去 ±3s 窗内多警告"偏差择优"（审计 2.6 的最后一块模糊择优）；令牌未绑定
   （AoI 裁剪）时保留原窗口搜索。作者/他人两路径接入。
5. **replay_shot 身份联表 eid 化**（viewer.rs）：目标/射手的 tank 与 config 匹配
   从"昵称首条"升级为 **shot 携带的 eid → comps account → 花名册**（退昵称）；
   逐发注入循环从 JSON 名字反查改为与类型化数据按下标对齐。附带语义修正：
   `shooter_tank` 从"恒为作者"改为"该发射手"（查看他人射击时 viewed/弹表随实际
   射手；作者射击行为不变）。
6. **考虑后不做**：ARENA_INFO 严格化（容错扫描 + 白名单 + 值域校验已是本 wire 域
   的既定稳健策略，实测提取 100%，严格化收益仅形态美观）；P2b 逐位级 hash6 预测
   （判别门已覆盖全部实际歧义，剩余为数据模型粒度限制）。

**对方侧状态（2026-10-07 更新）**：pin 切换 **v0.4.0**（`dd5aafe`，本次确定性收口；对方
PR #558：pin + AI 投影 golden 重生成（字段级对比确认与 v0.3.15 逐字段一致）+ `wotbVersion`
2.1.10 随 APK 内嵌运行时递增）。消费端契约适配与 3D 回放性能/画质专项已随对方 **PR #555**
合入（场景 GLB 实例化合批、画质四档重分档、均衡档俯视烘焙底图、动态分辨率与性能偏好、
可破坏物拾取）；俯视烘焙管线在本仓（`tools/bake_ground_roofs.py` → `tools/composite_overhead.py`，
配对对方 `bake-ground-overhead.mjs`；资产包内容变更须重传 COS，见 AGENTS.md）。此前链路：
v0.3.12 可破坏地形 → v0.3.13 pose_kf+弹道折线 → v0.3.14 昵称修复 → v0.3.15 实际搭载配置
+ 段化滤波 → **v0.4.0 确定性收口**。对方已完成**客户端解析迁移**（A158Coke/WotbTools#447
「服务器没有 parser」）——服务端解析器模块整体删除，浏览器/Android 跑本项目的 WASM，**本项目由此成为
该仓唯一的回放解析器**；AI 复盘走 WASM → canonical facts → `ClientAiReviewProjection`，parity 由
`ClientAiProjectionParityTest` 进 required CI 常驻看护。

**对方提出、待本项目补的字段**（见对方 `docs/architecture/client-replay-engine-migration.md` §后续）：
`PlaybackData.damages[]`（数据已在 `timeline.hp_events`）、`coverage`（packet 计数与
`decodedPacketRatio`）、`finish_reason`、`unsupported_damage`（仅双方无数值），以及实测确认
`Shot.game_hit_result` 与对方 Java `primaryResultRaw` 同义；其中"未钳零原始 HP"已由 v0.3.5 的
`hp_raw` 覆盖。

**装甲查看器点击判定触发面勘误（2026-10-08）**：本仓工具说明曾把"末层被穿透即击穿
（含只穿透履带/间隙甲）"直接等同于 *exactly like BlitzKit's armor inspector*——函数级
parity 成立（`shoot()` 语义逐条一致，判定的 `allow_ricochet=false` 分支本就带"无 Primary
即不判定"），但 BlitzKit 查看器把 `shoot()` 只挂在**主装甲（Primary）网格**的 `onClick` 上
（spaced/外部模块网格挂的是空 handler，只为进入 `event.intersections`），射线未触达主装甲
（只穿间隙甲屏幕、或只碰履带/炮管）根本**不发起判定**，查看器因此从不展示这类结论。消费方
（WotbTools）原相机点击门槛只挡"纯外部模块"，于是只穿间隙甲的点击按末层规则判成
`PENETRATION + 全额伤害`——实测 56TP 炮塔 `plate 12`（10mm + `vehicleDamageFactor=0.0`
屏幕板）约四成点击方向只穿过屏幕、无主装甲参与，即报障现象。已修正：WotbTools
`scene/tankViewer.js` 相机点击门槛改为"射线必须触达 Primary 板"（分类取自判定模块
`scene/penetration.js` 的 `isPrimary`，唯一事实源），不构成判定时清掉上一次的结论面板/轨迹；
夹具加 12mm 间隙甲屏幕板 + `test:browser-armor-aiming` 场景常驻（旧门槛下该场景复现
`PENETRATION · 360`，可作 fail-first 证据）。本仓只同步两处工具说明措辞（`src/agent/tools.rs`：
spaced 是角度等效层而非 flat 消耗层；末层规则的 parity 指向 `shoot()` 而非查看器展示），
判定本体、数据面（`models.pb`/`spaced` 分类与 BlitzKit 线上逐字节一致）与切面均未变。

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
| [docs/decoupling-status.md](decoupling-status.md) | **解耦的剩余决策与已定案差异**（2026-10-03 收敛；**2026-10-07 更新：GLB 自产管线已接线**——`data/cache/models/` 换为客户端解包导出，`cache/models` ≡ `local_models` 735/735）：已完成时间线；待决策口径（数据模型塌缩 / 25 辆无显示名 / `hull_traverse` 非同量）；封面图命名域与"换素材 ≠ 等价替换"（NCC 0.21）；已定案的 BlitzKit 侧差异清单；对外沟通材料。进度/接线/分发现状见 [data-inventory.md](data-inventory.md) |
| [docs/local-model-export.md](local-model-export.md) | ✅ 已实施（2026-10-01，2026-10-02 补 §4 贴图实测）：报告 B 的几何替代做成 `tools/export_tank_glb.py`，验收口径从"按可达节点求和"收紧到**逐字节 + 节点顺序**——`collision.glb` **735/735**、`model.glb` **733/735** 等价（余 2 辆为 BlitzKit 侧行为，见该文 §5）；贴图槽位与 BlitzKit 完全对齐（731/735 图片数相同、无缺槽位）。§4 逐通道实测推翻了报告 B §3.3 的图源判断，并定下 `baseRMMap` 的**通道搬迁**（ch0→G 粗糙度、ch1→B 金属度）。**2026-10-07 起已替换运行期数据源**：`data/cache/models/` 换为本地导出（`cache/models` ≡ `local_models` 735/735，包与 COS 随发；见 [data-inventory.md](data-inventory.md) §3） |

### 代码加固（2026-10-08，未发版）

**DVPL 解码 fail-closed**（[src/wargaming/dvpl.rs](../src/wargaming/dvpl.rs)）：补上 footer 的
编码长度 / 存储载荷 CRC32 / 解压长度三项校验，并把 LZ4 截断（含回引偏移越界）由"静默返回
零填充 `Ok`"改为 `Err`。此前 `_crc32` 读出即丢弃、footer 自述长度被直接当切片边界（损坏会
panic），而同仓库 Python 侧 `tools/wotbtools/wotb_sc2.py` 一直是三项全查——属"Python 侧
fail-closed、Rust 侧 fail-open"的标准分裂；两个外部独立实现（`Jylpah/dvplc`、
`vorlie/DAVA-Resource-Studio`）口径亦相同。

- 证据：`extract-game --force` 全量 735 辆，改动前后 `data/game_data/*.json` **逐字节相同**；
  全树探针 **45016 个 `.dvpl` / 20.3 GiB 零拒绝**；唯一触发 LZ4 零偏移的真机文件与参考实现
  （lz4 C）比对 sha256 一致
- **blast radius：无**——不改任何产物、契约字段或消费方；仅"损坏输入"由静默通过改为报错
- 探针发现真机存在**非规范 LZ4 零偏移**（`offset = 0`，全树仅 1 例）：参考实现解成
  match_len 个零字节，故两侧都按零处理而非拒绝；该文件的可达性已查清——位于被导出器
  跳过的 `Skin_01` 配置里，**当前不可达**，属潜在陷阱，两端口径现已一致
- 细节与判据见 [game-data-sources.md](game-data-sources.md) §5.3

## 约定

- 逆向结论的**唯一权威**是 [docs/回放与射击逆向总集.md](回放与射击逆向总集.md)；其余文档与其冲突时，
  先查 [wotbtools-cross-reference.md](wotbtools-cross-reference.md) 是否已有裁决。
- 历史推导过程不在工作区文件中保留，需要时查 git 历史。
- `examples/`（逆向探针脚本）与 `tmp_*/`（分析转储）不入库，文档中提及处仅作证据出处记录。
