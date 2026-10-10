# 文档索引

> 全部 Markdown 文档的定位与状态一览（2026-10-06 整理）。入口永远是根目录 [README](../README.md)。

## 使用文档（随项目演进，保持最新）

| 文档 | 定位 |
|---|---|
| [README.md](../README.md) | 项目总入口：项目定位、能力总览、与 WotbTools 的关系（分发形态）、仓库结构 |
| [docs/回放与射击逆向总集.md](回放与射击逆向总集.md) | 射击/回放逆向**唯一权威参考**：第一篇=协议层数据段（字节布局/语义/使用状态/死路清单/消费地图）、第二篇=客户端处理架构、第三篇=客户端弹道与命中表现（含 DecodeShotSegment 权威定义）、第四篇=WI 对照与射击复现实现、第五篇=遗留未定项 |
| [docs/replay-contract-v2-supremacy-type39.md](replay-contract-v2-supremacy-type39.md) | **回放契约 v2**：争霸基地状态（sparse 重建 + 零值省略/占领中断归零补正）+ 实时点数 + type39 原始帧用途与 `aim_frames` 删除记录、门禁与版本护栏 |
| [docs/wotbtools-cross-reference.md](wotbtools-cross-reference.md) | 与 WotbTools 逆向结论的逐条裁决记录（采纳/驳回/互证），防止误采或回退已定案；**文末附面向消费方切面的最新进展** |
| [docs/tank-suspension-client-re.md](tank-suspension-client-re.md) | **客户端坦克悬挂逆向（2026-10-10，两轮）**：①处理链与节点绑定契约（包内 735 辆普查）；②逐车 `suspension:` 数据面（730/764 有；`wheels` 的 `{flag,a,b}` 定案为**负重轮标记 + 上行/下行行程**、履带折线 = 静止形状、`chassis.textureScale`）；③**第二轮逐条还原**（三路反汇编）：负重轮 = 纯竖直平移夹到 `[pz−b, pz+a]` + `wheelsReactionSpeed` 限速（无弹簧阻尼）、轮自转 = 绕局部 X 轴 `θ+=Δs_侧·k`、履带 = 运行时切 chunk + GPU 实例化（静止折线 + 逐轮偏移 + 悬空段垂弧/包轮 + 铺地）、花纹每米 = `textureScale/chunkLength`、履带不采样地形；④勘误（「悬挂参数三处全查空」不成立）；⑤**对齐可行性分级 T1–T4** 与剩余未定项；⑥**数据面已导出**（`tools/export_tank_suspension.py` → 包内 `suspension/<tank_id>.json`，725/735 辆，回归 7 项）并随前端求解器接线（逐轮贴地 + 履带形变 + 自转/花纹滚动） | ✅ 机制已还原、数据已导出、前端已接线（T4 待再 RE） |
| [docs/architecture-debt.md](architecture-debt.md) | 架构债与长期改动方案：已完成项（combat.rs 拆分、双路径合并）与仍留存的 tankViewer 目录拆分 |
| [docs/game-data-sources.md](game-data-sources.md) | **数据来源权威表**：每份数据取自 BlitzKit / 本机客户端 / WG API / 自产；本地提取可行性评估；间隙甲 spaced 判定规则；提取链与 COS 资产面发布流程；2026-10 game_data 冻结故障复盘 |
| [docs/data-inventory.md](data-inventory.md) | **数据面清单 / 运行所需数据总账（来源 · 可替代性 · 完成度）**：运行期 11 项核心数据的逐项"目前来源 / 可替代来源 / 替代完成度"（✅ 已接线 · 🟢 数据就绪未接线 · 🟡 有缺口 · ⚪ 阻塞，2026-10-09 两轮全量对照实测）+ 派生/注入数据（WASM 三张注入表、`tank_names`、tank_id 桥等间接面）+ 非数据类外部依赖；三条分发渠道（COS 资产包 / GitHub Release 引擎 / 消费方前端常量）与包内布局；尚无代码的缺口与非代码障碍；完成度汇总（**2026-10-09 改版为总账形式**：客户端解包→同格式 pb 编码器已备（**试接后回退，未接线**）、manifest↔36 张 ground 语义偏差、弹种 `<kind>`/枪序口径核验；此前 2026-10-07：包统计与逐目录清单、地图导出 + 俯视合成链路、上传工具两个陷阱） |

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
| v0.4.2 | **地形让位掩码 + 逐图光照/IBL + 解析器"无炮塔流"回放放行**：①**解析器**（`playback.rs`）：车辆筛选在 `prop2`（type=7 炮塔流）**全局缺失**时退化为仅 type=10——训练房全程未瞄炮，客户端不发炮塔更新（实测样本 `20261010_1212__Anonyme_R132_T100LT_…`：803 包中 type=10 位姿 195 包、两实体 100/95 样本，**type=7 零条**）⇒ 旧判据 `st10 ∧ prop2` 交集为空、整场打不开；无 prop2 的车辆按"炮塔随车体朝向、炮管水平"中性渲染；仍 fail-closed（退化后仍空才报错）。**切面契约未变**（PlaybackData v2 / AiReviewFacet v1），只是此前被拒的这类回放现在可解析（回归夹具入库）。②**地形让位掩码**（数据面新件）：`tools/bake_terrain_cover.py` + 36 图 `cover.u16.bin` + `terrain.json.cover`——贴地薄构件（铁轨基板/压顶/铺装）在远视距会被客户端同源的自适应 LOD 合法盖住，掩码把**渲染用**高度场逐 texel 夹到结构面之下（只压不抬、压低 ≤0.5 m、留 0.1 m 几何间隙、20 texel 收尾）⇒ 铁轨越顶点 60/150/250 m = 1/2/12 → **0**；**水面片与水下薄板排除出贴地判定**（否则会把地形压走、扰动可见岸线）。③**逐图光照/IBL 资产**：`lighting.json`（太阳方向/色相/强度、环境色、fog 原始块）与 `ibl.webp`（等距柱状环境反射）随包。④导出器其余轮次：`extras.water` 水面材质、`extras.decal` 贴花、`extras.lightmap` 光照图（UV1 随导）、`extras.surfaceFlags` 表面标记、平卡/ST 卡片、坦克悬挂与本地模型导出工具。资产面随本轮**全量重导 + 俯视重渲合成 + manifest 重算 + COS 上传**（截至本版）。 |
| v0.4.1 | **数据面与资产管线修正 + 弹种权威化**：①地图几何改用文件内 authored 法线（静态 8985/9751 组 + SpeedTree 刚体子集 18/54 组；fail-closed 门槛不过则回退现算）；②PVR3 单通道 L8/A8 解码补全（954 张此前被静默丢弃；其中 18 张经地图 `alphamask` 被消费、934 张无消费方）；③`PenetrationRequest` 新增 additive `shell_type_id`、`tank/{id}.json` 新增 `type_id`——弹种判定改走 BlitzKit field9（客户端 `shells.xml <kind>` 语义枚举的翻译，4 值闭集），icon 词表降级为回退；④DVPL 解压分配上限 512 MiB；⑤其余为此前轮次的资产管线修正（DLC `packs/` 覆盖层读取、PVR3 宽高、SpeedTree 掩码判据、DXT5nm 法线、RM 金属度真实通道、地图导出器 gen2 悬空引用、坦克贴图双根回退）；资产面随本轮全量重导重建并已同步 COS（2026-10-09 回拉核验：线上 manifest 与本地逐字节一致；manifest↔grounds 语义偏差见 [data-inventory.md](data-inventory.md) §2.1）。**切面契约未变**（PlaybackData v2 / AiReviewFacet v1 不变），消费方无需适配。 |
| v0.4.0 | **回放解析确定性收口（语义/契约变更，消费方需 v0.4.0 适配——WotbTools PR #555 已同步）**：①`damage_received` 语义修正——**无证据 = `null`**（≠ 0，此前两者不可区分；Rust `Option<u32>`，旧版恒为数字）；②击杀原因统一 **255 = 未知/其他哨兵**（唯一可判定的规范值，此前未知可能落成任意缺省值）；③`infer_shots` 推断路径删除（无证据不产出）；④`PlaybackData.shots_from_loose_path` additive provenance 字段（炮弹来自宽松路径时置位）；⑤method38 权威配靶下 subtype=1 ARENA_INFO 全场 comp blob 修正——`turret_local`/`gun_local` 覆盖率 1/N → **N/N**；⑥#7–#12 遗留清零：HP seed 来源分级、comp 昵称窗放宽 1..=255、tank 白名单低 16 位 masked 匹配、decoded_target_gun_pitch 令牌优先、viewer 身份联表 eid 化（`tank_of_eid`/`comp_by_eid`）；type=28 槽位兜底 fail-closed。9 场语料全量重跑零回归 |
| v0.3.15 | **实际搭载配置透出（弹容 N 权威化）+ AoI 重入段化滤波（D1 卡顿修复）**：①`vehicles[]` additive 新增 `config_idx`（`resolve_config_index` 三级证据链钉定的 `configs[]` 下标，与 shots 的 `shooter_config_idx` 同域）/`burst_size`（该配置弹夹容量原值，0=单发；configs 唯一时无歧义直接给出）/`turret_local`+`gun_local`（comp blob 模块局部 id，纯回放证据，服务器与 WASM 双路径产出）——多炮坦克各炮弹容不同（资产包 735 台中 52 台跨配置不一致），装填条弹容 N 的唯一权威 = 实际搭载配置，禁跨配置取最大/剩余弹数+1 推断；附带修复 annotate 解析缓存键（原按 tank_id 单键共享，同车不同玩家不同炮互相串值）；②位置滤波器按 AoI 在场段独立实例化（客户端 `onEnterAoI → setFilterOnEntity()` 每次新建，原整场单实例——重入车辆先钉上一段末位 ~1s 再滑移 ~1.5s 收敛，即 3D 回放「追赶滑移」卡顿），段首以 Type5 物化快照为种子，`visibility[]` additive 新增 `pose`（pos@14/yaw@26/pitch@30）；`vehicles[].pos/hull_yaw/pose_kf` 与 shots 炮口锚点在重入车辆上数值修正（9 场 79 次重入实测：重入帧渲染位 vs 真值 med 28.4m → **0.4m**），消费端零改动；附带更正 indexes.rs 两处 0x1440C70 注释（D2 翻案：越末帧=硬保持无外推分支，0.9=稀疏 bracket 跨度阈值因子） |
| v0.3.14 | **type=5 昵称去掉 30 字节上限**（超长昵称阵营/车型联表失败修复）：真实对局存在 >30 字节的长 UTF-8 昵称（13 全角字符 = 39 字节，S16 Kranvagn 实测样本），旧 `1..=30` 长度域把合法昵称整条拒掉 → 实体无昵称 → 按昵称联花名册失败 → 该车 `team`/`tank_id` 落 0（fail-closed，前端表现为"阵营无法识别"）；长度域放宽到 u8 前缀全域 `1..=255`，保留 len==0 拒绝 + 载荷边界 + 合法 UTF-8 + 控制字符校验，fail-closed 语义不变；纯解码修复，无契约字段变化 |
| v0.3.13 | **渲染位姿关键帧折线 + 弹道折线（via/leg_secs）**：①`vehicles[].pose_kf`（additive）——位姿来源是客户端滤波器（AvatarFilter 移植）60Hz 逐帧输出，滤波器在「预测-保持」阶段（刚进 AoI 数秒内）输出为保持-跳变阶梯，旧 0.1s 网格重采样 + 网格间线性插值把阶梯混叠成速度摆动（实测某 0.5s 窗内前端线速度 4.5→29.6 m/s、全时线逐帧位置最大偏差 21m，即 3D 回放顿挫来源）；贪心走廊拟合（容差 2cm/0.4°）保留保持段两端与跳变段，点数 7~10/秒/车，消费端线性插值即复现客户端画面；②`shots[].via`/`leg_secs`（additive）——弹道折线：同一发炮弹的全部 method29 共享 (shooter, shotId)，链首 = 发射段、后续 = 跳弹/穿透续段（起点 = 装甲接触点），method20 = 弹道最终停止点（跳弹后落在出射方向延长线上）；段时长按 \|Δ\|/段速度计（不用 10Hz 包钟——跳弹常与发射同刻，用钟得零长段）；`flight_secs` = 各段之和、`end_time` = 抵达时刻（与 method8 同刻，命中归属随之修正）；确定性归属 = (shooter, shotId) 链 + method20 按 shotId 配对，无任何相关性匹配 |
| v0.3.12 | **可破坏地形事件切面**：`PlaybackData` 新增 additive 字段 `destructible_areas`（100m 格子区域实体锚点）与 `destructible_events`（时刻/类别/槽位/倒向）——类别 prop 1=fragiles 2=柱状 3=树倒、末字节 = lka 槽位（与区域格子联表 = 唯一物体寻址，逆向总集 §5.4 公式闭合，四场回放 799/801）、倒数字节 = 8 位倒向角（服务器权威，未点亮碾压者亦携带）；配套资产管线 `tools/export_map_destructibles.py`（36 图清单+lka serverId）与 scenery GLB 的 D_ 损毁态网格导出（`export_map_glb.py`/`export_asset_pack.py` 已随包）。**2026-10-06 选表勘误（逆向总集 §5.4 第七轮）**：slot 编号存在分段索引表 `blitz/<stem>.erN.lka` 时整体替代主表（两套编号体系，erlenberg 实测 829 公共键 455 个 serverId 不同 + 268 键仅在分段表；Middleburg 回放 11 事件地面真值 er0 表 11/11 命中、主表 4 MISS+6 错联）——`parse_lka` 已按新口径取表，erlenberg 数据需重导重打包。**2026-10-06 叶卡勘误**：新世代 SpeedTree（erlenberg Spruce/Linden/bush 等，92B/顶点混合批 = 刚体树枝 w=0 + 锚定叶簇 w=1，整簇顶点共享 pivot）旧导出器不识别、整批压成静态几何 → 树叶/草丛成固定朝向平面片；`decode_speedtree_card_gen2` 按 pivot.w 切分（卡=56B 同式 billboard 属性 `_CORNER`/`COLOR_0`，刚体余量走静态路径，守卫 fail-closed），30 图缓存 GLB 全量重导后随包生效。**2026-10-07 场景变体组勘误（逆向总集 §5.4）**：9 张多变体图的 .sc2 按变体（md1/dt2/er0… 标签组）挂出生点/边界/专属布景，客户端一局只激活一组，导出器全量导出使组外布景成回放幻影（Dead Rail 基础局立着 Railroad 变体的 stn_07 石头，作者出生点与 md1 SpawnTeam1_01 精确重合钉死激活组）——GLB 现随包打标（节点 extras `mdVariant` + asset extras `variantByMapId` 序数配对，组数≠key 数省略 fail-open），消费端按对局 map_id 剔除非本组节点（WotbTools `scene/variantFilter.js`），9 图 GLB 已 `--scenery-only` 重导随包生效。**2026-10-07 铁轨灰带真因（UV 平铺被幅值守卫误拒）**：`decode_group_uvs` 的幅值守卫（绝对值>64 拒绝）把合法平铺 TEXCOORD0 当垃圾拒掉——铁轨条沿轨道平铺贴图，v=-118 实测，REPEAT 采样下平铺倍数无上限——被拒后回退抓位 4 备用图集对当 UV0，该条铁轨只铺一个断面区间拉成灰带（平铺倍数小的条正常，故表现为『部分铁轨灰色』）。现只拒非有限值，平铺 UV 恢复导出，36 图 GLB 重导随包生效。附带记录：TextureLightmap 管线（materials-fp.sl：albedo × lightmap(UV1) × 2，unlit+烘焙光照图集）两次烘焙尝试分别因缺 ×2 与解出的 UV1 落在光照图集暗区而失败，均已回退，待与 UV 通道口径一并再核。**2026-10-07 叶卡颜色方程勘误**：旧实现 `min(occ/occMean×SH, 1.35)` 的 1.35 乘积钳 + occMean 均值归一化把 erlenberg 类 √π 灰 SH（应 ×1.575）压平成 ×1.35 并抹掉叶簇内 AO 明暗对比 → 树叶发灰发平（实测报障；高亮雪地贴图被 tone mapping 掩盖为 +9% 故长期未显形）；且 SH 只取 R 通道当灰度，丢掉 karelia (0.37,0.50,0.50) 冷调黄昏等真实每树彩色环境。修正：SHCoeff L0 按 RGB 三通道字面值导出（√π 灰与彩色环境都是客户端按字面乘的数据），occMean 固定 1.0（vOcc 直乘），前端 uSH 向量化 + 乘积钳 2.0（occMean=1.0 哨兵区分新旧包，旧包旧方程不受影响）。**2026-10-07 同树叶片双色勘误**：gen2 混合组的 w=0 子集是【固定朝向叶片】而非树枝（三角形尺寸与卡片叶同量级、同叶图集；如 Spruce 组 w=1 占比仅 0.25），客户端对两种叶片都乘 varVertexColor——gen2 刚体子集漏导 COLOR_0 使固定叶全亮、billboard 叶带 AO 偏暗 = 同树双色。修正：gen2 刚体网格随导 COLOR_0（VEC4）+ 前端 ST\| 静态材质透传 `vertexColors`（GLTFLoader 对带 COLOR_0 的几何自动置位，重建材质须透传；材质缓存键同步加入），36 图重导随包生效。**2026-10-07 场景 GLB 三项勘误（Naval Frontier 报障：冷杉叶片只剩零星小点、芦苇蕨丛发亮发平）**：①**材质族改按材质判定**——新增 `ClientMaterialFamily` 解析 `Data/Materials/<fx>.material` 的 `Shader:` 与 UniqueDefines（模板沿 `MaterialTemplate` ULTRA/HIGH/… 引用链取并集；`IgnoreDefines` 是"关闭"语义、不计），不再按 .sc2 实体类：SpeedTreeObject 类 + `Textured.material`（skit 芦苇/蕨、各图远景板）在客户端走普通【受光】`materials` 着色器，旧口径把它们当 ST\| 不受光 + SH 加亮；billboard 重建同样只在 `speedtree-materials` 族上做。②**ST 染色分族**：`SPHERICAL_LIT/PBR_SPEEDTREE` 族维持 SH(L0) 字面值（10-07 定版）；legacy（`SpeedTree.material`）族改用客户端公式 `color0 × treeLeafColorMul × treeLeafOcclusionMul + Offset`、**SH 完全不参与**（`speedtree-materials-vp.sl` 的 `#elif SPEED_TREE_OBJECT //legacy` 分支；skit 该组属性 = (1,1,1)/1.0 ⇒ 叶色 = albedo × COLOR0，旧口径乘 SH(1.7725) 使叶片偏亮 77%）。③**贴图嵌入按文件原始行序**：`decode_dds`/`decode_pvr3` 内部各翻一次、嵌入统一再翻回；此前只对 DDS 翻、**PVR 漏翻** → PVR 源贴图上下颠倒，叶卡 UV 窗口整个落在图集空白区（skit `skt_fir_leafs` 主窗口覆盖率 5.4% → 翻正后 35.2%、叶卡组窗口 5.4% → 20.5%）——"只剩零星小点"的直接原因；方向另由 `env_57_signs_01` 标牌贴图（原始行序为正立螃蟹）实测佐证。附带：`FLATCOLOR` 材质整图染色（客户端 speedtree-fp/materials-fp 皆为采样后 `baseColor *= flatColor`，旧实现只随 UV1 覆盖烘焙顺带应用）。影响面：36 图场景 GLB 全量重导（PVR 翻正 41 个图号、legacy 染色改判 38 个、ST 类非 speedtree 材质改回受光覆盖全部图号），地面原始重烤 + 俯视重渲合成重跑（渲染读新 GLB），包重建（4168 文件 / 3788.3MB）并同步 COS（72 对象 / 751.6MB，逐对象 sha256 回拉 73/73 一致；误传的 `overhead/` 73 个渲染对象 ~2.4GB 已清理）；`tools/upload_asset_pack_cos.py` 与 `map_index.json` 入库、本机调试产物入 `.gitignore`（包统计与上传要点见 [data-inventory.md](data-inventory.md) §2.1） |
| v0.3.11 | **基地占领中断归零 + 单基地存在性补正 + 删 `aim_frames` + 渲染网格加 `hull_roll`**：①争霸（wrapper12）全缺省行 = 显式清空、单基地（wrapper8）双缺省块 = 进度归零——wire 按 proto3 省略零值字段，中断（车辆出圈/被击毁）此前被「缺省=维持前值」吞掉，进度与占领方永久挂在基地上；②`assault_objective_present` 按字段契约放宽为「目标族出现即真」（实现此前多要求 `f3\|\|f4`，把"有目标但全程未占领"的场次整场压掉）；③删除 `aim_frames`（零消费方，实测占回放 JSON 57.6%，20 车样本 6.92MB → 2.93MB；原始 type39 帧保留给射击复现），`PlaybackData.version` 保持 2；④`vehicles[].hull_roll`（additive）取原始 type=10 最近邻——滤波层不输出侧倾 |
| v0.3.10 | **射击复现多 interaction 关联修复**：`unique shotId = 一次开火 = 一个 Shot`；作者严格路径在同 victim / 同钟出现多个 type=32 segment 时，优先用 `method8.hash6 ↔ type32.hash6` 确定关联；重复 method8 广播按 hash 去重，证据不足时继续 fail-fast，不猜选 |

切面字段均为**附加**（`AiReviewFacet` v1 / `PlaybackData` v2 版本不变）。

**数据面换源：已备未接线（2026-10-09 试接后回退）**：客户端解包 → 同格式 pb 的编码器
（`tools/extract_vehicles.py` → `tools/emit_vehicle_pb.py`）已就绪并跑通回归（试接批
`tank_cache` 重建仅 1 行差异），但**本地解包版本的验证未完成**，`data/tanks.pb` /
`data/models.pb` 已回退为 BlitzKit 现役版本、暂不使用。待验证项与复用方法见
[game-data-sources.md](game-data-sources.md) §5.2b、[data-inventory.md](data-inventory.md)
§一注/§四。

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
| [docs/feasibility-map-lighting.md](feasibility-map-lighting.md) | **3D 地图光照画质对齐可行性（2026-10-09）**：客户端光照**全部数据驱动、数据都在已会解的容器里**——逐图太阳/环境色/雾/阴影三档/IBL 在 `.sc2` 的 `#sceneComponentSets.Default`；静态场景走**烘焙光照图**（每图 2–11 张 2048² 图集 + 网格 UV1 × 每实例 `uvScale/uvOffset`，后者正是此前两次烘焙失败的缺项）；天空穹 35/36 图已在 `scenery.glb` 内（仅被前端隐藏）；IBL diffuse/specular cube 逐图齐备；**110 个客户端着色器源码可解**（方程可逐行复刻，不必猜）。收益排序：烘焙光照图 > 逐图太阳/环境色 > 天空+雾 > 坦克 IBL。体积预算 +40~+155 MB（现包 3.6 GB）；缺口在导出器与消费端渲染，不在逆向。含 36 图光照参数表、三期实施、8 项未定项（fail-closed） | ⬜ 待决策（报告已出，未改代码/资产；前端侧改动归 WotbTools） |
| ↳ [docs/map-render-align-audit.md](map-render-align-audit.md) | **地图渲染 × 客户端差异总账（2026-10-09）**：36 图 / 9.3 万批次全量普查后的 **23 条**对齐清单（另 1 条已对齐基线），分**错误 7 / 缺失 9 / 画质档取舍 7**。三方对照（客户端 36 图实测 + 本仓导出物 + WotbTools 现役渲染）。**已对齐基线**：分层地面（`tilemask-fp.sl` 四分支）、SpeedTree 染色与叶卡。**最大三类**：① 静态场景烘焙光照图（42,651 批）② 逐图太阳 + 线性曝光（替掉 ACES 与一批补偿 hack）③ 天空 + 雾（36/36 图）。**P0 里另有 3 项可独立先合**：天空穹（35/36 图网格已在 GLB 内，只被隐藏，~20 行）、场景 GLB 贴图各向异性（一行）、关 `flatShading`（现前端把 v0.4.1 的 authored 法线丢掉了）。未定口径 4 项（逐贴图 sRGB 空间、客户端画质档默认值、exposure、`materialLightmapAdjustment` 缺省）| ⬜ 待决策 |
| ↳ [docs/map-lighting-plan.md](map-lighting-plan.md) | **实施设计（2026-10-09，待批准）**——上一条的落地版：四个关键决策（① 光照图交付＝**原始 UV1 + 逐节点 `{atlas,scale,offset}`**，附被否方案实测依据：预变换 UV1 会让顶点膨胀 10–30×、逐实例烘贴图＝2497 实例↔2320 组唯一变换；② 图集 WebP 2048² q92 = 1.6 MB/张、全量 +145 MB，1024² 备选 +40 MB；③ IBL 立方图逐面逐 mip WebP（+44 KB/图）；④ 天空 flowmap 公式 / 雾 `vp-fog-math` / 曝光 = `LinearToneMapping`）。期 0 标定（含无需用户的自证校验：天空最亮点反投影验太阳方向）、期 1 本仓数据面（3–5 人日，含"无光照图批次逐字节零回归"验收线）、期 2 WotbTools 五步（5–8 人日，含 Android `wotbVersion` 递增与降级门）、期 3 精修 | ⬜ 待批准（未动代码/资产） |

## 实现记录（已落地）

| 文档 | 状态 |
|---|---|
| [docs/decoupling-status.md](decoupling-status.md) | **解耦的剩余决策与已定案差异**（2026-10-03 收敛；**2026-10-07 更新：GLB 自产管线已接线**——`data/cache/models/` 换为客户端解包导出，`cache/models` ≡ `local_models` 735/735）：已完成时间线；待决策口径（数据模型塌缩 / 25 辆无显示名 / `hull_traverse` 非同量）；封面图与 BK 复核为**同源同画**（旧 NCC 0.21/非同一幅画结论已撤销）；已定案的 BlitzKit 侧差异清单；对外沟通材料。进度/接线/分发现状见 [data-inventory.md](data-inventory.md) |
| [docs/local-model-export.md](local-model-export.md) | ✅ 已实施（2026-10-01，2026-10-02 补 §4 贴图实测）：报告 B 的几何替代做成 `tools/export_tank_glb.py`，验收口径从"按可达节点求和"收紧到**逐字节 + 节点顺序**——`collision.glb` **735/735**、`model.glb` **733/735** 等价（余 2 辆为 BlitzKit 侧行为、另 23 辆 2026-10-10 起**有意修正**（静态变换烘焙 / 状态过滤，等价数 710/735），见该文 §5/§5.1）；贴图槽位与 BlitzKit 完全对齐（731/735 图片数相同、无缺槽位）。§4 逐通道实测推翻了报告 B §3.3 的图源判断，并定下 `baseRMMap` 的**通道搬迁**（ch0→G 粗糙度、ch1→B 金属度）。**2026-10-07 起已替换运行期数据源**：`data/cache/models/` 换为本地导出（`cache/models` ≡ `local_models` 735/735，包与 COS 随发；见 [data-inventory.md](data-inventory.md) §一 #8） |

### Type5 配件串改变长解析（解析器，2026-10-10，未发版）

**报障**（WotbTools 射击复现）：60TP 射击 Strv K 那发，车辆与装备面板的"校准弹"未勾选
（对面 60TP 实际装配了校准炮弹）。

**根因**：Type5 物化尾部配件串在线路上是 **`0B <n>` + nB 变长**（n = 实装件数 1..=9，
空槽省略；满配 9），解析器（ported 自 Java `VehicleBattleLoadout`）硬编码只认 `0B 09`+9B
→ 凡不满 9 件的玩家**整条 loadout 被静默丢弃** → `shooter_equipment=null` → 前端按"未装备"
渲染。实测该场 14 人中 7 人落此形态（对面 60TP 串 = `0B 02 67 6c` = 103 校准弹 + 108
改进型模块）；全语料 `0B <n>` 直方图 {1..9}（n=9 占多数）。

**修复**：`replay-core combat::collect::scan_loadout` 改变长解析——`0B <n>`（n∈1..=9）+
n 字节 + ID 域校验（100..=123）+ 新帧尾护栏（串后恒 `0C 00 0D`，全语料 175/175 成立）
+ 原有 `0A KK`+KK×14B 描述符回找对齐；短串按原序填入 9 槽、空槽补 0（`equipment` 形状与
facet 不变，`raw` 仍是 9 字节）。回归：`collects_variable_length_equipment_string`
（含缺帧尾拒收负例）+ 既有满配用例。真实回放校验：该场 loadout 采集 7/14 → **14/14**，
`t=125.33` 那发 `shooter_equipment.calibrated_shells = true`。

**blast radius**：只改"漏采 → 采到"；此前为空值的少数玩家现在有值（additive），满配玩家
行为逐字节不变，`shooter_equipment`/`raw` 形状（9 字节）不变。**消费侧连带**：校准弹从
"未知→已装备"后，该发的穿深模拟会按 ×1.06 计入校准加成（此前漏计）。文档：总集 §2.6
尾部表同步更正（定长 9 → 变长）。注：`collect.rs` 与另一会话在途的"弹种指纹回填"改动同
文件，提交时两批一起走。

### 顶层弹表补等效厚度判定输入（数据面，2026-10-10，未发版）

**报障**（WotbTools 装甲查看器）：同一位置，初始"同车检视"（Strv K 射击 Strv K）显示蓝色可
击穿，手动重选同一射击方后变红挡弹——界面无任何可见变化（弹表下拉只显示弹种/穿深/伤害）。

**根因**：射手弹表两条取数路径不同源——初始同车检视经 `applyConfig` 走 `configs[].shells`
（全字段），重选射击方经 `loadShooter` 走**顶层摘要** `shells`；而顶层投影缺 `normalization`
（转正角）等判定输入，消费方按 0° 兜底 ⇒ 等效厚度被抬高（`eff = t/cos(angle − norm)`，两倍
口径规则下转正角还会放大 `1.4×norm×caliber/(2t)`）。Strv K AP：5° → 0°，实测 100mm 板 @68°
由"可击穿（等效 220）"翻为"挡弹（等效 267）"。同缺陷对**任何射手≠目标的检视**从打开起就
存在（一律走顶层表）。

**修复**（本仓数据面字段全量补齐，消费方零改动）：
- `tank_resolver.rs`：`ShellData` 增 `caliber`/`normalization`/`ricochet`（serde default，
  旧缓存可读），顶级炮投影随弹填值 → `tank_cache.json` 重生成（735 辆 / **2079/2079** 弹
  全量有值；与换字段前逐车 diff **仅**新增三项）
- `tank_configs.rs::tank_data_value_prefixed`（包内 `tank/{id}.json` 顶层弹表）与
  `web/assets.rs::shells_handler`（`/api/shells`）同步补齐；`/api/tank` 走缓存透传自动获得
- 回归：`shell_summary_keeps_penetration_decision_inputs`（全量动能弹三项 > 0；锚点 Strv K
  AP = 105mm / 5° / 70°）+ `top_level_shells_carry_decision_inputs_matching_top_gun_config`
  （顶层弹表 ≡ 末档配置：弹种顺序 + 三项逐值相等；车级 `caliber` ≡ 顶层弹表弹径）
- **同批修正车级 `caliber` 档位**：`tank_data_value_prefixed` 原取 `configs.first()`（初始炮）
  口径，取不到时按炮名解析回退 **120**——与"顶层弹表"不同档。跨车选射手时查看器把车级口径
  当弹表口径用（三倍/两倍口径规则 + 转正放大），216 辆多炮车因此用错口径（E 50：88 vs 顶级炮
  105；BT-2/AT 7 类落 120 回退）。现改为**顶级炮弹表自带弹径**（权威源），炮名解析（189/735
  与弹径不一致）与初始档彻底退出该字段；326 辆车道级口径值变更（`S35 CA` 120→90、`O-I`
  100→135、`E 50` 88→105…）
- **blast radius**：735 辆 `tank/{id}.json` + `data/tank_cache.json` + `data/data_version.json`
  内容变更（文件数不变，约 +0.3 MB；additive 字段 + 一处车级口径修正，消费方零适配）；
  `release/asset_pack` 已替换并 `--refresh-manifest` 重算（5000 文件 / 4310.9 MB）——
  **COS 已同步**：`--only tank/ --only data/` 上传 737 件 / 8.8 MB、`manifest.json` 就地补丁
  登记 737 条，回拉 740/740 逐对象 sha256 一致（`.json` 走 no-cache，前端即时可见）
- 数据面契约注记见 [game-data-sources.md](game-data-sources.md) §3.5

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

### 外部仓库格式对照与修复（2026-10-08，未发版）

对 GitHub 上 35 个 WoT Blitz 格式/解包相关仓库做了一轮系统对照（DAVA 引擎源码、SC2/SCG
格式文档、14 个 DVPL 实现、纹理/模型工具、BlitzKit 上游定义），逐条与本仓实现核对后落地
以下修复。**结论均以"外部权威定义 + 本机真实客户端实测"双向锁定**，非单侧推断。

**活缺陷（改变输出）**

| 项 | 事实与证据 |
|---|---|
| **DLC 覆盖层 `packs/` 未纳入读取路径** | 客户端把微更新写到 `%LOCALAPPDATA%\wotblitz\packs` 并**同相对路径覆盖** `Data/`。实测 45 个 packs 文件中 **5 个覆盖 Data 同名不同内容**——含坦克可视模型 `3d/Tanks/German/Ferdinand.sc2`（10458 vs 7832 B）与同车 `.scg`（**922191 vs 791385 B**）；另 40 个 Data 里根本没有（`G37_Ferdinand_skin` 皮肤资产）。修复：Rust `game_extract::packs_dir/resolve_client_path` + Python `tools/wotbtools/dlc_packs.py`，客户端资源读取改为 packs 优先、缺失回退。 |
| **PVR3 头宽高读反** | PVR3 版式为 **height@24 / width@28**（DAVA `PVRFormatHelper.h` 结构体 + writer/reader 双向赋值），本仓读成 w@24/h@28。实测 1328 个 PVR 中 **5 张非方形**（`palm_trunk`/`a_sosna01`×2/`env_kr_cactus`/`monument`）被 reshape 成转置图；同资产 `palm_trunk` 的 DDS 副本为高 512×宽 256，与 `@24=512` 互证。修复后逐张比对：**1323 张一致、恰好这 5 张修正、零异常**。 |
| **SpeedTree 叶卡判据按掩码而非 stride 硬编码** | 旧实现只认 56B/92B 两种 stride；实测含 `PIVOT4`(bit10) 的组共 **668 个 / 10 种掩码**，覆盖仅 420 个 → 248 组整组掉进静态路径（pivot 被忽略 → 叶片固定朝向）。改为掩码驱动（流序 = EVF 位号序，见 `PolygonGroup.cpp:49-166`）。新旧比对：**回归 0、原有 324 组输出逐项一致、新增覆盖 122 组**。 |
| **弹种静默误判** | 弹种按 `icon` 串词表映射，未知键**静默落 HE**（HE 走溅射并强制永不跳弹）。按上游 proto 加权威来源 `field9 = ShellType 枚举`（0 AP/1 APCR/2 HEAT/3 HE），并加等价性测试**全量 4426 发**守护——首跑即抓出真实漏项 **`atgm_heat`（2 发）被误判为 HE**（应为 HEAT）。 |
| **`hull_traverse` 取错字段** | 本仓读 tanks.pb field27 当车体转速并 ×180/π 透出；上游 `tank_definitions.proto:37` + 生成器确认 **field27 = `camouflage_still`（静止迷彩系数）**。修复：改取 `tracks[].traverse_speed`（T-34 顶级履带 46 deg/s），键名与语义不变、零契约变更。 |

**稳健性 / 口径统一（不改变当前输出）**

- **顶点位表按 DAVA 权威收口**：`EVF_*` 位定义 + `GetVertexSize`（`RenderBase.h:158-177,213-261`）
  与两个独立外部实现三方一致；本仓表 bit9/10/12/13 四位全错（16/8/12/16 应为 4/16/4/8）且缺
  5/6/11/14/15/16-19。旧表没出错的原因是偏移累加只遍历 bit0-2，恰好落在两表一致区间。
  新表对**全部 43 种实测掩码**（地图 32 种 9751 组 + 坦克 25 种 66712 组）**零 stride 失配**。
- **UV 偏移改为按置位累加**：旧实现对 bit4 硬编码 `low_off + 8`、默认 TEXCOORD0 必然存在；
  mask `0x11`（只置 TEXCOORD1）的 **20 个组**UV 起点被算到 stride 之外而静默丢弃，修复后
  20/20 恢复（坦克侧本就正确，两侧口径现已一致）。
- **DXGI 表纠错**：85-90 曾映射为 BC6H/BC7（真值 B5G6R5/B5G5R5A1/B8G8R8A8 等非 BCn），
  改为正确的 95-97/98-100；实测 DX10 只用 71/72/74/75/77/78，旧表从未触发，属埋雷。
- **DDS 注释纠错**：`fourCC@84` / DX10 `dxgi@128` / 像素 `@148` **全是标准偏移**，原注释
  "DAVA 整体 +4"是误读（行为本就正确，仅注释会误导后续维护者）。
- 字段命名纠错：gun `field5` = `rotation_speed`（曾名 `caliber_factor`）、`field9` = `tier`
  （曾误读 `shell_count`）、新增 `field18 = shell_capacity`。

**blast radius**：改动触及资产提取链（Rust `extract-game`、Python 两个 GLB 导出器）与坦克
数据导出面。**产物会变**——PVR / SpeedTree / 顶点表 / UV 修正都改变地图与坦克 GLB 的几何与
贴图，属"会改 WotbTools 可见输出"的改动：需重导资产包并重烤俯视图，视觉终判按 AGENTS.md
归用户。回放切面与 WASM 契约未触碰。

**尚未落地（已定位、待排期）**：`export_tank_glb.py` 的 DXT5+位31 仍按 BC5 解（外部证据指向
标准 DXT5nm：A=X/G=Y；实测 `x²+y²≤1` 占比 98.6% vs 当前 66.5%），属改法线/金属度观感的改动，
需与 BlitzKit 对照 + 用户视觉验收；地图侧材质仍用 `compute_normals` 现算（坦克侧已用文件内
authored NORMAL），属内部口径不一致；PVR3 单通道 L8/A8（1328 中 954 个）未解码（消费者待验）；
`ShellType` 权威路径 field9 尚未接线进 `PenetrationRequest`；battle_results `117=damage_blocked` /
`107=mm_rating` 与竞技场条目 `f4=队伍号` 未采用。

### 俯视合成朝向改契约常量（2026-10-09，未发版）

**触发**：用户报障——均衡档（mid）地面在部分地图呈现镜像（3D/极致档正常），例：锡默尔斯多夫。

**根因**：`tools/composite_overhead.py` 旧版对 8 朝向做梯度相关取 argmax 定朝向。在
"底图与渲染层无共同结构"的图上该判决退化为**噪声 argmax**：`himmelsdorf`（城市地面底图
vs 屋顶渲染）8 个候选全落在 |score| ≤ 0.12 的噪声带、winner `xY` margin 0.0124，把俯视层
烘成了**上下镜像**（同病灶 `erlenberg_old` margin 0.0054，只是碰巧选中正确候选）。

**修复**：朝向改为**契约常量**——渲染侧（`bake-ground-overhead.mjs`：qFrame
`Ry(π)·Rx(−π/2)` + 正交相机 `up(0,0,−1)` ⇒ 渲染图左列 = `−X_scene`、顶行 = `−Z_scene`）
与底图侧（`ground.webp` 契约 `uv = (0.5−X/s, 0.5−Z/s)`、flipY=true ⇒ 左列 = `+X_scene`、
顶行 = 北）相差 180° ⇒ **恒取 `YX`，任何地图都不需要特殊处理**；8 朝向相关降级为 `audit`
诊断项（不影响写盘，选中别的候选时在报告里标注）。新增 `--verify` 读回校验（按合成式反解
包内实际烘入朝向）、`--render-dir`/`--base-dir`（重烘直读 `release/overhead-bake/` 与
`data/cache/maps/`，避免把渲染复制进包、避免在已合成件上二次合成）。

- 证据：①反解重建 36 图中 35 张实际烘入 = `YX`（corr 0.993–0.997），仅 `himmelsdorf`
  = `xY`（corr 0.9966）；按 `xY` 重建的旧件与问题文件**逐字节相同**（`bac8d3bb…`）；
  ②客户端小地图（北朝上）独立判据：渲染 vs 小地图 33/36 选 `YX`，底图 vs 小地图 33/36 选
  恒等（含 himmelsdorf，margin 0.0459 ⇒ 底图本身没歪）；③用户锚点：港湾小镇（`port`，朝向
  正确）= `YX`，与推导互证；④**全量重烘等价性**：以 `--base-dir data/cache/maps` 重跑 36 图，
  产物与现盘**逐字节相同 36/36**（回归 0），`--verify` 全包 `ok=true`
- **blast radius**：均衡档地面单一资产（`map/<key>/ground.webp`）。本地包仅重烘
  `himmelsdorf`（`a142e94e…`，7080664 B），其余 35 张内容未变；`manifest.json` 对 36 张 ground
  记"合成前哈希"的语义未变（见 [data-inventory.md](data-inventory.md) §2.1）。
  **已同步 COS**——delta = 1 个对象 + manifest 强传，上传后回拉逐字节一致、读回朝向 = `YX`
  （旧镜像件 `bac8d3bb…` 曾是线上内容，已被覆盖）
- 契约推导、重烘口径与读回校验命令见 [game-data-sources.md](game-data-sources.md) §5.5

### 掩码通道修复：烟/瀑布类效果层 alpha 烘成二值剪影（2026-10-09，未发版）

**触发**：用户 3D 回放报障——锡默尔斯多夫一侧两个烟雾贴图"没有正确加载"（附 `[scene-pick]`
定位：`smoke.sc2` / `smoke3.sc2` 两个实体）。

**根因**：这两个实体是 `Textured.material` + `AlphaBlend/AlphaMask` 预设 + `TEXTURE0_ANIMATION_SHIFT`
（滚动烟）+ `FLATCOLOR`；**albedo 自身 alpha 恒 255，alpha 只能来自 `alphamask`**（客户端
`materials-fp.sl` 的 `FP_A8(tex2D(alphamask, uv1))` 取该单通道贴图的值）。而这些掩码是 PVR3
**8bpp 单通道（L8）**贴图，本仓 `decode_pvr3` 按既定口径把值放在 **RGB、A 恒 255**；导出器的
`bake_uv1_overlays` 却恒取 `.a` → 掩码恒等于 1 → 烘焙 alpha 退化成"网格 UV0 覆盖区 = 255、
其余 = 0"的**二值剪影**（实测中间调占比 0%）→ 烟雾成硬边不透明卡片。

**修复**（三处，逐条有测试）：
1. `tools/export_map_glb.py::bake_uv1_overlays` 掩码取值改为"**A 无信息（min≥250）时取亮度**，
   否则取 A"（前者=单通道掩码，后者=RGBA 掩码，与客户端 `FP_A8` 同义）。实测两处烟雾 alpha
   中间调占比 0% → **74.3% / 30.5%**（软渐变）。
2. 导出器给真混合层写 `material.extras.blendLayer`（烟雾/瀑布/浪这类"动画+掩码"层；水不在内）。
3. WotbTools `convMat` 尊重该标记：**不再**把"BLEND 但不透明度≈1"的伪透明启发式套到它身上
   （该启发式会把软 alpha 再削成 alphaTest 0.33 硬边卡片），保留 BLEND + 不写深度。旧包无此
   extras ⇒ 行为与改前一致（前向兼容）。

- **证据**：复现脚本逐字节对齐包内旧件（alpha 二值、0% 中间调）→ 修复后软渐变；新旧代码同图
  重导对照 `idle` 19.96→19.88 MB、`neptune` 26.90→26.91 MB（体积影响 ±1 MB）；本仓单测
  `tools/test_bake_uv1_mask.py` 3/3、WotbTools `src/scene/` 420/420（含新增"真混合层"守卫与
  天穹/各向异性/authored 法线守卫）。
- **blast radius**：**15 图 144 个 mask 批次受影响**（另 161 个 `TEXTURE0_ANIMATION_SHIFT` 批次
  无掩码、不受影响）；本次重导 **13 图 scenery.glb** 并重打包（4168 文件 / 3940.2 MB，
  `worktree_dirty=true`）——**COS 尚未同步**（delta ≈ 13 个 `map/*/scenery.glb` + `manifest.json` +
  `index.json`）。回放契约与 WASM 未触碰。
- **同日续做：滚动动画 + 逐图太阳**（见下条）。

### 逐图光照接入 + 动画混合层（2026-10-09，未发版）

**两件事同一个出口**：3D 场景的光照从"硬编码两盏灯"改为"读逐图真实数据"，同时把烟/瀑/浪
的滚动动画按客户端语义做出来。

**① 逐图太阳（修"朝向反了"）**。触发：用户报障"面向光源的建筑面偏暗、背光的反而亮"。
取证：① 包内几何**绕序与 authored 法线 100% 一致**（中位 dot=+1.000，地图与坦克都是）——
不是法线翻转；② 真因是那盏**假太阳的方位**：硬编码 `(120,260,80)` 在场景系是"东北来光"，
而锡默尔斯多夫真实太阳在南（客户端 `DirectionalLightComponent` 解出）。此前 `flatShading`
用屏幕导数现算面法线、光照不随真法线走，方向错也看不出来；删掉它（authored 法线生效）
后立刻显形。

- 新增 `tools/export_map_lighting.py`：逐图 `lighting.json`（太阳方向/色/强度 + 环境色 +
  雾 + IBL 引用，后两者供后续对齐）。方向 = `R(quaternion)·(0,−1,0)`（光源局部 **−Y** =
  阳光**传播**方向；36 图实测仰角 21.5°–65.7° 自洽，见
  [feasibility-map-lighting.md](feasibility-map-lighting.md) §七 #1），并同时给出
  `direction_scene`（qFrame `(−x,z,y)`，消费端直接用）。**55 张（含变体/旧图）全部导出、零 warnings**。
- 消费端：取 `map/<key>/lighting.json` → 方向光 position = −direction（平行光只方向有意义，
  target 入场景图）、`color/intensity` 按客户端公式直用（客户端 `color*intensity*NdotL/π`，
  three 物理光单位同式）、半球光天空色取 `sun.ambient`（客户端 `ambColor`）；读不到则回落
  原来的兜底灯（旧包行为不变），会话拆除复位（防换图残留）。
- **雾：明确不做（2026-10-09 用户裁示）**。`lighting.json` 里的 `fog` 块（距离/半空间/大气
  三路参数 + 天/日色）**随包保留备查**，但渲染层不接——3D 回放的用途是**高视角分析对局**，
  雾会削弱远景/战场可见性，属"影响预期功能的特效"。前端曾实现过一版完整复刻
  （`vp-fog-math.slh`：距离项 + 半空间项 + 大气散射），按此裁示**已全量回退**（代码与测试
  均删除、零残留）；后如需"可选开关"再另行评估。审计表 E2 相应记为〔非目标〕。
- **未接（已登记）**：偏航只是"直接光方向"对齐，**烘焙光照图仍未使用**（静态场景在客户端
  是烘焙光图，见 [map-lighting-plan.md](map-lighting-plan.md) 期 2b）——因此阴影/AO 仍缺，
  且 `SCENERY_LAMBERT_EXPOSURE` 补偿旋钮暂留（期 2a 余项：曝光/色调映射对齐）。

**② 动画混合层（烟雾/瀑布/浪）**。客户端语义（`materials-vp.sl`/`-fp.sl`）：
`uv0 += texture0Shift + frac(tex0ShiftPerSecond × globalTime)`（**只动 albedo**），掩码走
`varTexCoord1 = texcoord1`（**不动**）⇒ 观感 = **轮廓固定 + 内部纹理滚动**。因此不能把掩码
烘进同一张贴图的 alpha（位移会连轮廓一起滑走、`frac` 回绕时轮廓跳变）。

- 导出器：动画+掩码的效果层改为**掩码单独成图**（`extras.maskTexture` = glTF texture 下标，
  值统一落在 alpha 通道，取值口径与烘焙共用 `mask_channel_values`）+ 随导 **TEXCOORD_1** +
  `extras.tex0ShiftPerSecond/texture0Shift`；材质显式 `alphaMode: BLEND`。**13 图 144 个批次**。
- 消费端：`makeBlendLayerMaterial` 双采样器（albedo 走 UV0+位移、掩码走 UV1）× 回放时钟驱动
  `uTime`（暂停即停、seek 后相位确定、截图可复现）；掩码经 GLTFLoader `associations` 预解析
  （texture 依赖异步、材质遍历同步）。旧包（无 maskTexture/UV1）自动回落烘焙口径。
- 测试：本仓 `tools/test_export_lighting.py` 5/5、`tools/test_bake_uv1_mask.py` 4/4；
  WotbTools `src/scene/` **422/422**（新增"动画混合层"与"逐图太阳"守卫）。
- **blast radius**：13 图 `scenery.glb` 重导 + 36 张 `lighting.json` 新增 → 重打包
  **4203 文件 / 3939.4MB**（`generated=2026-10-09T12:16:35Z`、`worktree_dirty=true`）；
  **COS 未同步**（delta ≈ 13 scenery + 36 lighting.json + manifest + index.json）。回放契约未触碰。

### 烘焙光照图接入（期 2b，2026-10-09，未发版）

**静态场景改用客户端的烘焙光照**——这是审计里 ★★★ 的首项（此前用假光照 + 全局压暗补偿，
客户端静态几何**根本不受实时光**：`materials-fp.sl` 的 `MATERIAL_LIGHTMAP` 路径是
`color = albedo(UV0) × lightmap(UV1 × uvScale + uvOffset) × 2`，着色器里没有任何光照项）。

**设计决策（量化后定案）**：光照图的 UV 变换是**逐材质实例**的（同一网格在不同位置图集
瓦片不同）。若按"逐材质"实现，同一网格的每个实例都要各建一份几何——实测顶点膨胀
**2.3–11.3×**（himmelsdorf 10.5 万 → 118 万，不可接受）；故改为：**图集贴图挂材质**
（`extras.lightmap`，材质键含图集，实测一个材质仍只引一张图集）＋**逐实例 UV 变换挂节点**
（`extras.lm = [sx,sy,ox,oy]`）→ 前端汇成 **`InstancedBufferAttribute('aLm')`**（16 B/实例）。

- 导出：36/36 图全部有光照图（`lightmapAtlases/`，2–11 张/图，2048² BC1）；图集解码后
  按 1024² JPEG q92 内嵌（单张 ~0.5MB）；光图网格随导 **TEXCOORD_1**；SpeedTree 族/卡片
  路径不接（它们有自己的 SH/顶点色公式）；无水/天空（不进实例合批、拿不到 aLm）；
  条数不对齐/图集缺失一律**回落**受光材质（fail-closed，不产出错误画面）。
- 消费：`makeLightmappedMaterial`（unlit，`albedo × lightmap(uv1×aLm.xy+aLm.zw) × 2`，
  镂空经 `discard` 保留）。**几何零拷贝保住**：批次挂 aLm 时用"薄包装几何"（属性对象
  原样引用，three 按属性对象建 GPU 缓冲 → 不重复上传；`geometry.clone()` 会复制属性
  对象导致顶点重复上传，已用守卫禁止）。
- 量（本机实测）：全部 **42,650 个节点**带逐实例光照图变换、2105 个光照图材质、
  121 张内嵌图集；包 **4043.7 MB**（+104 MB，+2.6%）；批次数量级不变（himmelsdorf 237 /
  forgecity 941 个光照图批次，draw call 与改前同阶）。
- 测试：本仓 `tools/test_export_lightmap_data.py` 2/2（`uvScale/uvOffset` blob 解析 +
  节点 extras 合并——`lm` 与 `mdVariant` 必须共存）；WotbTools `src/scene/` **423/423**
  （新增"烘焙光照图"守卫：公式/×2/不受光/镂空/实例属性/禁用 clone）。
- **口径与未接**：伽马直采直写（与叶卡/ST|/混合层同口径；线性口径待 C3 的 sRGB 项）；动态
  阴影 × 光照图的混合（客户端 `LMGateFactor`）未做（无动态阴影）；`pbrLightmap`（4 张
  "AllQualities" 图在 ULTRA 档的 RG 光图）未做。**COS 未同步**（delta ≈ 36 图 scenery +
  manifest + index.json）。

### 逐图 IBL 环境反射接入（期 2d，2026-10-09，未发版）

**数据面**：`data/cache/maps/<space>.ibl.webp`（等距柱状投影，1024×512，36 图合计 **~1.0 MB**）。
来源 = 客户端 `IBL/specular.dx11.dds`（256² 立方图 + 9 级预滤 mip）的 **mip0**——预滤链里最锐的
一级（≈粗糙度 0），喂给 three 的 PMREM 由引擎生成粗糙度链，最接近客户端 `MipFromRoughness`
（精确复刻属期 3）。转投影的面表**逐字取自客户端** `Materials/Shaders/cubemap-faces.slh`：

- 面序 = DDS 标准 `[+X,−X,+Y,−Y,+Z,−Z]`，但**坐标是 DAVA 世界系（z 上）**⇒
  **face4(+Z) = 天、face5(−Z) = 地**（旧实现按"Y 上是 up"猜 → 反射上下颠倒）。
  实测佐证：himmelsdorf face4=125 最亮 / face5=41 最暗；holland face4=61 / face5=23。
- 方向 = `N + (2u−1)·U + (2v−1)·V`（客户端同式）；输出转到**场景系（y 上）**、行 0 = 上，
  与 three 的 `EquirectangularReflectionMapping` 一致。另修一个解码真 bug：DDS 立方图是
  "**每面各带完整 mip 链**"布局，按"逐 mip 逐面"读会把 mip1 当成第二个面。

**消费端**：逐图加载 → `scene.environment`（等距柱状，sRGB）+ `scene.environmentIntensity`
= `ibl.environmentMultiplier`（逐图 0.8–4.0）。只影响 glTF PBR 材质（**坦克**）；自定义
ShaderMaterial（场景/地面/植被/天穹）不吃 environment。时序守卫：资产阶段可早于 `initScene`
（scene 未创建）→ 加载时判空 + `initScene` 末尾补挂；会话拆除摘环境并 dispose（PMREM 缓存
随纹理释放）。缺图（旧包/未配置资产面）→ 不设环境贴图，行为与改前一致。

- 测试：本仓 `tools/test_export_ibl.py` 4/4（面表逐字对照 + "+Z 落顶行/−Z 落底行" +
  "+X 落等距柱缝"朝向不变量）；WotbTools `src/scene/` **424/424**（新增"逐图 IBL"守卫）。
- **未做**：`neutralizeDefaultMetalness`（老式车金属度归零）**仍在**——解除它需要单独对照
  客户端老式车的材质模型；`ibl.environmentGroundFactor`/`environmentGamma` 未接。
- **blast radius**：36 张 `ibl.webp` 新增 → 重打包 **4239 文件 / 4044.7 MB**（`generated =
  2026-10-09T12:41:36Z`、`worktree_dirty=true`）；**COS 未同步**（delta ≈ 36 图 scenery +
  36 lighting.json + 36 ibl.webp + manifest + index.json）。

### 环境反射遮罩接入（B2，2026-10-09，未发版）

**静态几何的玻璃/金属/潮湿面反射**（客户端 `ENVIRONMENT_MAPPING`：`materials-fp.sl:320-338`）。
本轮为该功能的**第二次**实施——第一次在收尾前整体回退（原因与教训见文末"着色器静默失败"）。

- **判据是"绑了 `envReflectionMask` + `cubemap` 两个槽"，不是 `ENVIRONMENT_MAPPING` 旗标**
  ——实测旗标只在部分实例上（idle 有、forgecity 的窗户材质没有），而绑槽才是客户端真正开
  反射的条件（全树 535 个实例，与槽位普查逐数吻合）。
- 数据：遮罩单通道成图（值落 alpha，UV0 采样）+ **逐图天空立方图**（同一套 `wotb_cube` 面表
  → 等距柱状 512×256 内嵌 JPEG）+ 逐材质属性（gloss/菲涅尔/brightenEnv/specular/lerpEnvMap/
  addDiffuse/maskMultiplier/multLightmap/cubeIntensity；缺省按客户端 property 默认值，只写非缺省项）。
- 前端：`makeLightmappedMaterial` 增 `ENV_REFLECTION` 分支逐项复刻客户端公式（`maskScaled` →
  `FresnelShlickVec3` → `envMult = F·brighten/3` → `lightenLM = saturate(lm × multLightmap)` →
  `mix(color, color·addDiffuse + refl, min(mask·lerp,1)) + specular·maskScaled`，高光用
  `lighting.slh` 的 BlinnPhong），反射向量取客户端 `materials-vp.sl:274-276` 的世界系
  `reflect(view, normal)`；`lightColor0` 经共享 `sunUniforms`（换图时写 color×intensity）。
- **朝向口径**：内嵌等距柱状图**不翻** ⇒ glTF `flipY=false` ⇒ 采样 `v = 0.5 − asin(y)/π`；
  **与逐图 IBL 走 `scene.environment`（flipY=true ⇒ `v = 0.5 + asin(y)/π`）相反，两侧不要互抄**。
- 量：**5 图 / 67 个反射材质**（idle、plant、holland、forgecity、iceworld）；包 **4239 文件 /
  4047.0 MB**。水/SpeedTree/卡片不接（各有自己的材质路径）。

**新增：场景材质着色器编译门禁**（`WotbTools/frontend/scripts/check-scenery-shaders.mjs` +
页面侧 `scripts/shader-check-page.js`，`node scripts/check-scenery-shaders.mjs`）。把每种材质
工厂真建一遍、以 InstancedMesh 渲染一帧，读 three 的 program diagnostics——**7 个用例 / 6 个
程序 / 0 编译失败**。这类失败在页面上只表现为"某些物体不渲染"（无红屏），是本次两次"建筑不见了"
的同一病根；门禁把它变成命令行判据。⚠️ 页面逻辑必须在**真文件**里（中间件内联 HTML 不经 vite
的 HTML 变换，裸 `'three'` 解析不了——上一版自检脚本即卡死在此）。

**着色器静默失败四类（本轮全部踩过，已各有防线）**：
① 顶点**声明 varying 却不赋值**、片元读它；② 片元**孤儿 varying**（顶点没有）；③ GLSL 模板里
出现**反引号**（提前闭合 JS 模板 → 语法错误）；④ `#if ENV_REFLECTION` 在 three 的 define 注入下
展开为空（应写 `#ifdef`）。防线：门禁（编译真跑）+ 源码守卫（varying 必须赋值、片元 ⊆ 顶点、
禁 `#if <bool define>`）+ **每次文本替换都断言命中**（本轮误把代码插进"混合层"材质、以及
"以为插了其实没插"，都是静默替换所致）。

### 水面判定改材质驱动（修"建筑发白"，2026-10-09，未发版）

**触发**：用户报障——埃尔伦贝格（显示名"米德尔堡"）的 `bld_er_water_tower_pbr` **整体发白**，
其余正常（附 `[scene-pick]` 定位）。

**根因**：消费端判水面用的是**节点名启发式**（`/water|sea|lake|river|fountain/i`），而这座楼叫
`bld_er_water_tower_pbr`（**water** tower）→ 被判成水面 → ① 排除出烘焙光照图路径、② 排除出实例
合批（拿不到逐实例 `aLm`）→ 退回 Lambert + 真实太阳/半球光。其 albedo 均值仅 96/255，但受光路径
把它推到近白（实测包内数据本身没问题：albedo 均值 96、光照图切片均值 0.42，与同图其他建筑同量级；
正确路径应得 `0.376 × 0.42 × 2 ≈ 0.32`）。

**修复（数据驱动）**：
- 导出器新增 `is_water_material(mat_desc)`——判据是**材质文件 fxName 含 `water`**（`WaterAllQualities` /
  `WaterPerPixel*`），与节点名无关；结果写进材质 extras `water: true`（36 图共 **14 个**水材质）。
- GLB 根 extras 增 `surfaceFlags: 1`（声明"材质标记权威"），与 `variantByMapId` 同一落点
  （`asset.extras` → `gltf.scene.userData`）。
- 消费端：`strictSurfaces` 为真时**只认材质标记**（`extras.water`）；旧包（无 `surfaceFlags`）退回
  名字启发式（行为不变，属已知瑕疵）。三处判定（光照图门槛 / 水面修整分支 / 实例合批过滤）统一走
  `isWaterNode(name, mat)`。
- 测试：本仓 `tools/test_export_surface_flags.py` 2/2（水材质识别 + **名字带 water 的建筑不算水**）；
  WotbTools `src/scene/` **427/427**（"水体"守卫改为断言材质驱动三处 + 新助手）。
- **blast radius**：36 图 scenery 全量重导（+`water`/`surfaceFlags` 元数据）→ 重打包
  **4239 文件 / 4047.1 MB**（`generated = 2026-10-09T13:35:39Z`、`worktree_dirty=true`）；
  **COS 未同步**。

### 水面着色采客户端 MEDIUM 档（F1，2026-10-09，未发版）

**14 图 / 40 个水面批次**改用客户端 `water-fp.sl` 的 **`!REAL_REFLECTION` 分支 = MEDIUM 档**
（材质文件 `WaterPerPixelCubemapAlphablend.material`：`UniqueDefines: [PIXEL_LIT, FRESNEL_TO_ALPHA]`、
WaterRenderLayer + `blend: true`、`depthWrite: true`）逐项复刻：

```
uv0 = uv × normal0Scale + frac(normal0ShiftPerSecond × t)
uv1 = (uv.x+uv.y, uv.y−uv.x) × normal1Scale + frac(normal1ShiftPerSecond × t)   ← 45° 旋转层
normal = normalize(n0 + n1 − 1)                       // UDN 相加（客户端同式）
fresnel = FresnelShlickCustom(dot(−V, normal), fresnelBias, fresnelPow)
outColor = (texCUBE(cubemap, R) × reflectionTintColor, alpha = fresnel)          // R.z=|R.z| 防穿透
```

客户端四档映射（材质文件逐字实测，供后续选档参照）：`ULTRA_HIGH →
WaterPerPixelRealReflectionsRefractions`（屏幕空间反射+折射，alpha ≡ 1）、`HIGH →
…RealReflectionsAlphablend`（屏幕空间反射，alpha = 菲涅尔）、**`MEDIUM →
WaterPerPixelCubemapAlphablend`（本档）**、`LOW → WaterPerVertexCubemap`（`!PIXEL_LIT`：
不透明、无法线 ⇒ 反射是平面镜、零波浪扰动）。

- 数据（`extras.water = {normal, cube, props}`）：**逐图 water cubemap**（`water/cubemap/*.tex`）
  + `normalmap` 槽（双层滚动的切线空间法线图，实测 DXT1 且 B≈251 ⇒ 无需 DXT5nm 重建；无损 PNG）
  + 7 项逐材质属性（`normal0·1Scale` / `normal0·1ShiftPerSecond` / `fresnelBias` / `fresnelPow` /
  `reflectionTintColor`，**全写**、缺省即客户端 property 缺省）。**两类贴图都必需**：缺任一张即
  fail-closed 不发水面数据（消费端回落普通材质——菲涅尔 alpha 需要法线、反射项需要立方图）。
- **PVR3 立方图支持**（`wotb_cube.decode_pvr3_cube`）：逐图水面 cubemap 两种容器都有（erlenberg/
  malinovka/port 是 PVR3、rudniki/holland/plant 是 DDS），位深实测 **RGB565** 与 RGBA4444；布局与
  DDS 一致（**每面带完整 mip 链**）。统一入口 `decode_cube`（IBL/环境反射/水面共用）。
- 前端 `makeWaterMaterial`：切线基用**片元导数**建（cotangent frame，我们不发 TANGENT 属性）；
  波纹动画走**回放时钟**（`uTime`，暂停即停）；`transparent` + **`depthWrite: true`**（erlenberg
  实测：极简大三角面不写深度会与地形逐像素交替闪）+ 保留"透明水面逐材质修整 + 不设 renderOrder"
  分支；等距柱状朝向与 B2 同式。

**已知观感与限制（2026-10-09 用户报障"俯视白色浪花、低角度逐片消失、反射与水上模型不匹配"后
诊断；属本档固有、非 bug）**：

1. **反射与模型不符＝构造使然**：客户端 HIGH/ULTRA 的 `REAL_REFLECTION` 采**当帧场景渲染**
   （`water-fp.sl:217` `dynamicReflection`），MEDIUM 用的是**预烘环境探针**（erlenberg 256²，
   六面 156–182、均值 168/255、低对比、**不含任何建筑/车辆**），且被当**整体颜色**。
2. **俯视偏亮（"白色浪花"观感）**：该探针按高度带亮度 **顶部(俯视时反射指向天)185 / 地平 154 /
   底部 147** → 俯视最亮且是无细节平摊亮面（读作"白"），视角压低后采样带转暗 → 亮斑**逐片退去**
   （与报障一致）。
3. **"逐片消失"另有几何因素**：地形与水面高差 **−6.9 ~ +7.7 m**（本仓既有实测）→ 露出水面的
   地形块在掠射角遮挡水面（客户端同几何，但其水更"实"故不显）。
4. **"反射和陆地不匹配"的机制（2026-10-09 追加实测，属本档固有）**：反射源是**预烘探针**，于是
   ① **无视差**——探针在某个固定点烘一次，相机沿河岸移动时反射内容不随视点滑动；② **当地几何
   不在图里**；③ 采样按客户端本档规则钳到地平线以上（`R.y=|R.y|`），永远看不到探针下半。逐图
   实测探针内容差异很大：**erlenberg/malinovka 是纯天空探针**（全探针 R/G/B 均值 149/170/184 与
   148/186/223；0–90° 各仰角带的"最暗 10%"仍是蓝色 ⇒ **图里根本没有地物**），所以那两张图的反射
   **只有天空/雾**，自然和岸线对不上；**port 的探针含陆地层**（`pt_cubemap` 0–15° 带最暗 10% =
   (38,28,13) 暗褐、15–30° 带 = (51,37,20)，绿占优像素占 57–75%），其反射会出现暗色剪影，但那是
   **探针拍摄点**的岸线、与你所在的岸线位置无关 ⇒ 同样"不匹配"。**朝向逐环核验（2026-10-09 用户
   疑"朝向有问题"后复检）**：① 坐标帧：以 three.js 实算 qFrame（`Euler(-π/2, π, 0, 'YXZ')`），
   其逆变换与 `cube_to_equirect` 的 `game=(−s.x, s.z, s.y)` 逐基向量一致 ✔；② 面表：与客户端
   `cubemap-faces.slh` 的 faceNormals/Us/Vs 逐字相同 ✔；③ 容器布局：14 图水面立方图（13 个不同
   文件）载荷长度与"6 面 × 每面完整 mip 链"预期**逐张精确吻合（余量 0）** ⇒ 无面/mip 错位 ✔；
   ④ 等距柱状生成与着色器采样同约定（与 IBL 路径的 flipY 语义互证）✔；⑤ **最强实证**：
   malinovka 探针含**紧凑太阳盘**（角展 3.6°），按我们的读法落在**方位 +163.2°，而该图太阳方位
   +165.8°（差 2.6°）** ✔——若存在整体旋转/镜像，方位不可能对上。逐图亮区方位：erlenberg 探针
   −51.5° vs 太阳 −40.8°/天空穹 −63.1°；port −65.4° vs −75.4°/−45°；malinovka +163.3° vs
   +165.8°/+180°（均在 10–25° 内）。**⚠️ 勘误**：本条目早前写的"erlenberg 163° vs 166°"是输出
   截断后**误引了 malinovka 的行**，erlenberg 自身数字如上。
   ⑥ **但探针资产自身与其地图的天空并不一致（这才是"看着像朝向不对"的根因）**：erlenberg 的探针
   **纵向结构与其天空穹相反**（探针最亮带在 60–90° 仰角 = 183.8、地平带最暗 155.7；而该图天空穹是
   地平 229 / 天顶 56 的正常分布），且其最亮的一片（47° 宽的云/雾铺，中心在仰角 **−8°**，即地平线
   以下）**按本档 `R.y=|R.y|` 的钳制永远不会被采样**；malinovka 的太阳盘则比该图太阳**低约 20°**
   （23.4° vs 43.1°）；port 的探针含**拍摄点的陆地**。三者都说明：这类"预烘环境探针"是与地图光照
   流水线**解耦**的老资产，逐图一致性没有保证——我们只是忠实回放它（客户端本档同样如此）。
4. **可选修法（均未做）**：(a) **平面反射**（镜像相机 + 半分辨率 RT + 投影采样 + 客户端
   `reflectionDistortion` 的波浪扰动）——唯一能让"水里照出建筑"的做法（≈ 客户端 HIGH 观感），
   代价是水面在视野内时每帧多一次场景渲染；(b) **压反射权重**（`reflectionTintColor`/探针强度）
   并补底色项——参数级微调，可治"偏亮/像平板"。

**档位试用沿革（同日）**：曾按用户选择把水面改采 **LOW 档**（`!PIXEL_LIT`：不透明、双层水贴图
相乘 ×3 × `decalTintColor` × 印花 + cubemap × `reflectanceColor`，印花走网格原生 TEXCOORD_1）——
实看被判"不好"（该档**没有法线** ⇒ 反射是平面镜、无波浪扰动，观感偏"平板"），按用户裁示
**回退 MEDIUM**（"恢复到 medium 就行"）。LOW 档的代码、数据、守卫与文档描述**已全部删除**（无残留：
导出器不再发 `albedo/decal`，材质键回到 `(normalmap, cubemap, props)`，水面网格不再随导 TEXCOORD_1）。

- **未做**：HIGH/ULTRA 的屏幕空间反射/折射（`REAL_REFLECTION`/`REAL_REFRACTION`，需深度纹理）、
  浪花（`WATER_DEFORMATION`）、涟漪（`WATER_RIPPLES`）、岸线；水面的雾按"雾=非目标"跳过。
- 测试：本仓 `tools/test_export_surface_flags.py` **4/4**（水材质识别 + 名字带 water 的建筑不算水 +
  `water_props` 缺省/实测两例）；WotbTools `src/scene/` 守卫为 MEDIUM 公式逐项（并断言 LOW 档字段
  `albedoTex|decalTex|uReflectance|uDecalTint` **零残留**、透明水面修整分支在场）；着色器门禁
  **8 用例 / 7 程序 / 0 编译失败**。
- **blast radius**：14 图 scenery 重导（水面数据回到 MEDIUM 形态：`{normal, cube, props}`、网格不再
  随导 TEXCOORD_1）→ 重打包 **4239 文件 / 4048.5 MB**（回到 MEDIUM 版体量；LOW 版为 4048.6 MB，
  差在水贴图/印花图替换回法线图）；**COS 未同步**。材质去重键 `(normalmap, cubemap, props)`——
  属性仍进键（浮冰群等"同贴图不同属性"实例不得串用）。**逐图核验**：36 图 GLB 的
  `extras.water` 形态普查 = 22 图无水面数据 + **14 图 `[cube, normal, props]`**（无一张残留 LOW 形态）。

### 立方图解码：DXT3/DXT5 的 BCn 编号修正（2026-10-09，未发版）

**症状**：`wotb_cube.decode_dds_cube` 的 fourCC→BCn 表写成 `{DXT1:1, DXT3:3, DXT5:5}`——
用的是 **DXT 号**而不是 `imagecodecs.bcn_decode` 要的 **BC 号**（DXT1=BC1、DXT3=BC2、
DXT5=BC3）。于是 DXT5 被当 **BC5**（双通道）解，`bcn_decode` 抛
`ValueError: invalid shape=(h,w,4) for BC5`，异常冒到调用端的 `try/except` ⇒ 该立方图
**静默失效**（不报错、不产出、消费端默默降级）。

**影响面**（全图普查水/环境反射立方图的容器与格式后逐项**实测**旧表 vs 新表的解码结果）：
- **真受损 1 处**：`22_italy_it` 水面 cubemap（`water/cubemap/cubemap_rudniki.tex`）是 **DXT5**
  → 旧表抛异常 ⇒ 该图水面数据一直缺失（水面数据要求 `{normal, cube}` 两类贴图都在，缺立方图即
  **整条不发** ⇒ 回落普通材质）——本仓 MEDIUM 版文档此前记"13 图"，少的那一张就是 italy。
- **侥幸无恙 2 处（勿按"DXT3 也在表里"类推）**：`11_plant_pn` 水面 cubemap 与 `08_idle_id`
  环境反射天空立方图都是 **DXT3**，旧表按 **BC3** 解——BC2/BC3 的**颜色块布局逐字节相同**
  （差别只在 8 B alpha 块，而立方图只取 RGB）⇒ 实测两处 **RGB 与正确解法定全等**，没有受损。
- 其余立方图为 **DXT1**（旧表 1 = BC1，恰好正确）与 **PVR3**（另一解码器，不受影响）；
  **IBL 36 图重导逐字节不变**（全 DXT1；重跑 55 张 0 跳过 0 失败），**`env_mapped` 前后同为
  534**（= 535 个绑双槽实例里还绑光照图的那 534 个，非本 bug 所致）。

**修复**：表改为 `{DXT1:1, DXT3:2, DXT5:3}`（与 2D 路径 `export_map_glb.DDS_FOURCC_TO_BCN`
逐项一致——**两张表分家正是本 bug 的成因，已加单测锁相等**）；逐面解码失败收成
fail-closed 返回 None（不再靠调用端兜异常）。

**测试**：`tools/test_export_ibl.py` **8/8**——新增 4 例：DXT5 立方图解出已知 RGB565 色
（旧表下必抛/必 None）、DXT1/DXT3 可解、非立方图（缺 `DDSCAPS2_CUBEMAP`）与未知 fourCC
返回 None、两条路径的 fourCC→BCn 表相等。

**blast radius**：实际只 `22_italy_it` 一件（该图水面从"回落普通材质"变为拿到**完整的 MEDIUM
数据**——法线 + 立方图 + 菲涅尔属性）；与 F1 同批 scenery 重导 → 重打包见 F1 条的 blast radius。

### 贴图口径改客户端原生分辨率（C1 收口，2026-10-09，未发版）

**触发**：用户报障"有些地图的**外层包围地图**发糊"，并给出实拍坐标。定位到
`04_medvedkovo_md`（Dead Rail）的外围群山环实体 **`mountains_001kl_`**（世界包围盒 ±605 m，
即跨 1.2 km，高出地形上沿 104 m）。

**根因（我方，非客户端）**：导出器对贴图做了硬性降采样——
- 场景走 `decode_dds/decode_pvr3` 的**默认 `max_dim=1024`**：该材质的 albedo
  `landscape/mountain/mountains.tex` 与 lightmap `lightmaps/texture0.tex` **原生都是 2048² DXT1**，
  被压到 1024²；1024² 铺 1.2 km ⇒ ≈1.2 m/px，远景观感成片发糊（正是审计表 C1 那条）。
- 地面路径另有 `max_dim=2048`；水面/环境反射的立方图等距柱状输出宽度还是 512（源立方图
  256²/面，512 宽等距柱状反而比源更稀）。

**改动（本仓）**：新增 `_cap_dim()`，**`max_dim = 0` 表示不缩放**并成为
`decode_dds`/`decode_pvr3` 的缺省；地面路径 `max_dim=2048` → 原生；`_env_cube_equirect`
的 `out_w` 512 → 1024。**所有贴图一律按客户端原生分辨率入包**（JPEG/WebP 质量档不动）。
坦克侧无需改动——`export_tank_glb.py` 本就是 `--max-tex 0`（抽样 400 个坦克 GLB：**316 个含
2048² 贴图**，已是原生）；小地图也已是客户端 **@2x**（512²，`MiniMapSmall@2x.packed.webp`，
Rust 侧"优先 @2x"）；`tank_images/*` 来自 BlitzKit CDN，不属客户端贴图口径。

**实测（用户报障点）**：`mountains_001kl_` albedo **1024²/220 KB → 2048²/677 KB**，
lightmap **1024² → 2048²/1726 KB**；该图 scenery.glb 12.6 → 17.6 MB。

**代价（实测）**：`scenery.glb` 合计 **620 → 834 MB（×1.34）**；地面烘焙与分层合计
**525.7 → 543.9 MB**；整包 **4048.5 → 4280.0 MB（+231.5 MB，+5.7%）**。逐图增量最大：
plant +13.6 / desert_train +10.4 / lagoon +9.8 / canal +9.5 MB。**下载代价同步上升**
（高档场景 GLB 逐图 12–43 → 17–52 MB；中/低档的 `ground.webp` 不变）。

**测试**：本仓 8 个测试文件 **49/49**（`test_dvpl_decode` 的 PVR 断言在无上限下同样通过）；
前端无改动（贴图分辨率是数据面属性，运行时不设上限）。

**blast radius**：36 图 scenery + 地面全量重导 → 重打包 **4239 文件 / 4280.0 MB**；
**COS 未同步**（凭据仅走环境变量）。审计表 C1 由〔错误〕转〔已修复〕。

### 视角淡出效果片的软混合（B6 收口，2026-10-09，未发版）

**触发**：用户"我认为贴图存在解码错误，比如废弃轨道（medvedkovo）"。

**解码审计先排除解码问题**（该图实测）：60/60 条贴图路径全部解出（57 albedo + 3 lightmap +
1 alphamask，零静默失败）；唯一自写解码器（PVR3 二维）用**文件自带 mip 链**做无真值校验——
9 张叶/灌木贴图的 alpha 相关 **+0.92~+0.97**、不透明区 RGB **+0.87~+0.97**（DDS 路径
imagecodecs 对照 **+0.985~+0.998** 证明该检验能抓到真错位）⇒ 载荷布局/mip 寻址/4444 通道打包均正确；
地面分层齐全且原生；包内唯一"纯色平图"是我们自产的掩码图（RGB=255 + 掩码在 alpha，按设计）。

**真因（B6）**：`rays.sc2`（雪图**光束/光柱片**，世界包围盒 200–570 m × ±340 m、高至 218 m）
被我们按 `has_alpha` 一律导出成 **`alphaMode=MASK, alphaCutoff=0.33`**——而它的贴图是
"**纯白 RGB + 图案全在 alpha**"（512² DXT3，A 均值 14、**仅 5% 像素高于 0.33**）⇒ **95% 内容被裁掉**，
剩下的 5% 渲染成硬边暖白块（`flatColor` 染色 (1.22, 0.91, 0.86) 已烘进贴图）——看着就像贴图坏了。
客户端语义（材质节点 + 材质文件逐字）：`enabledPresets = {"AlphaBlend": true}` →
`Textured.material` 的 `AlphaBlend` 预设 = `TransclucentRenderLayer` + `blend: true` +
**`depthWrite: false`**；flags **`BLEND_BY_ANGLE`** → 片元末
`alpha *= pow(saturate((VdotN − angleBlendBounds.x) / (bounds.y − bounds.x)), angleBlendPower)`，
`VdotN = |dot(视线, 法线)| / (|视线|·|法线|)`、`inversion` 可翻转（`materials-fp.sl:378-384`）。
即：**软混合 + 按视线"正对程度"淡出**（从侧面看得见、越俯视越淡），我们两样都没有。

**改动**：
- 导出器：`MaterialLibrary.resolve` 新增捕获 **`enabledPresets`**（此前完全未读）；对
  `BLEND_BY_ANGLE` 材质读 `angleBlendBounds/angleBlendPower/angleBlendInversion`，导出
  `alphaMode=BLEND` + `extras.alphaBlend=true` + `extras.blendByAngle={bounds,power,inversion}`；
  淡出参数进材质去重键。
- 前端（WotbTools）：新增 `makeBlendByAngleMaterial`（不受光、`transparent`、
  **`depthWrite:false`**、按上式实现视角因子）；`extras.alphaBlend` 同时让
  "伪透明（opacity≈1）→裁切"启发式放行（内核 `playbackScene` 与俯视烘焙页
  `bake-ground-overhead.mjs` 两处 SSOT 同步，守卫锁 convMat 键指纹）。
- **范围**：只改 `BLEND_BY_ANGLE` 这类**效果片**（**48 实例 / 2 图**：medvedkovo 1 = 该光束片、
  `08_idle_id` 47）。其余 7652 个带 `AlphaBlend` 预设的实例（植被/建筑）**仍走既有裁切取舍**
  （避免透明排序闪烁，见 B5/植被条目），不在本改动范围。

**验证**：medvedkovo 的 rays 材质实测 `alphaMode=BLEND` + `extras={alphaBlend, blendByAngle}`
（改前为 `MASK/0.33`）；本仓 Python **49/49**；WotbTools **216 文件 / 2949 通过 / 2 跳过**；
着色器门禁 **9 用例 / 8 程序 / 0 编译失败**（新增 `blend-by-angle` 用例）。

**blast radius**：2 图 scenery 重导（medvedkovo、idle）→ 重打包见下（与贴图分辨率批累计）；
**COS 未同步**。

### 光照图判据收窄：绑槽 ≠ 客户端会采样（"外围贴图不正常"，2026-10-09，未发版）

**触发**：用户"看起来废弃轨道的外围贴图还是不正常"（B6 修完后仍存在）。

**真因**：**光照图判据过宽**。我们的判据是"材质链绑了 `textures.lightmap` 槽即按下不受光烘焙渲染"
（`albedo × lightmap × 2`）；而客户端只在编译期 define 含 `MATERIAL_LIGHTMAP` 时才采样
（`materials-vp.sl:117-119` 的 `varTexCoord1 = uvScale*texcoord1 + uvOffset` 与
`materials-fp.sl:246` 的采样都在该 define 内；`SETUP_LIGHTMAP` 分支实测全库材质无人使用）。
define 的来源只有三个（普查全部 **42651** 个绑光图实例逐项吻合）：
① 材质文件**顶层** UniqueDefines —— `TextureLightmap.material`（绝大多数建筑，37064 个）；
② 实例**启用的预设** —— `Textured.material`/`Detail.material` + `enabledPresets: {LightMap: true}`
（实测 466 + 386 个）；
③ 光照图**模板族**（名字含 lightmap 的模板，`StandardLightmapAllQualities`——按画质档指到
PBR/Textured，实例不启用预设；审计既定口径继续按烘焙光照图渲染，其实测窗口是正常光图值：
erlenberg `env_er_woodenbridge` 均值 0.42/p10 0.17）。

**medvedkovo 外围群山 `mountains_001kl_`（跨 1.2 km）正是三者之外的漏网者**：材质
`Textured.material`、叶子无 `enabledPresets` ⇒ 客户端是 **unlit `albedo × flatColor`**；
我们却乘了 `lightmaps/texture0.tex` 的 (0,0) 象限——实测该窗口**均值 0.23、p10 0.012**
（大片近黑），于是雪白群山（albedo 均值 (0.61,0.60,0.64)）被压成 **深灰带黑斑**（合成均值
67/255）。

**改动**：`ClientMaterialFamily` 增加 `lm_top`（**只看 `Presets:` 之前的顶层段**——第一版整文件
扫描会把 `Textured.material` 的 LightMap 预设计成"自带"，已由新单测锁死分界）与 `lm_presets`；
新增纯函数 `preset_names` / `preset_defines` 与判据 `lightmap_capable(mat_desc, family)`；
导出器的光照图门槛加 `lightmap_capable`。**前端无需改动**（材质有无 `extras.lightmap` 决定路径）。

**影响面（实测）**：**111 个实例 / 7 张图**不再采光照图——`italy` 32、`pliego` 24、
`forgecity` 19、`iceworld` 19、`faust` 15、`medvedkovo` 1、`port` 1；材质仅
`Textured.material`（87，含两图的外围群山）与 `StandardAllQualities.material`（24，客户端的
**受光**族，本该走动态光照）。其余 42540 个实例的判定不变。

**验证**：medvedkovo 出包实测——群山材质 `extras` 由 `['lightmap']` 变 **`[]`**（节点 `lm` 变换与
网格 `TEXCOORD_1` 一并随去），同图 `bld_29_house05`/`tunnel`/`rails_9`（`TextureLightmap` 族）
**仍带光照图** ✔；本仓 `tools/test_export_lightmap_data.py` **4/4**（新增判据与"顶层/预设分界"两例）。

**blast radius**：7 图 scenery 重导（含地面）→ 重打包见下（与 B6/贴图分辨率批累计）；**COS 未同步**。

### 坦克材质：无 MR 贴图时写显式因子（"真实坦克发白"，2026-10-09，未发版）

**触发**：用户"目前真实坦克模型在场景里看起来比较发白"。

**真因**：坦克是回放场景里**唯一的 PBR 对象**（走 three 的 `MeshStandardMaterial`：逐图太阳 +
半球环境色 + 逐图 IBL + ACES 曝光），而 glTF 规范里 `metallicFactor` / `roughnessFactor` 的
**默认值是 1.0 / 1.0**——导出器只在**贴图解析成功**时才写 `metallicRoughnessTexture`，解析不到
就**什么都不写** ⇒ 落进"全金属 + 全粗糙"：金属没有漫反射，整块只剩被 albedo 染色的环境反射，
在逐图 IBL 下就是**发白**。
**影响面（实测）**：735 辆里 **278 辆含"无 MR 贴图材质"**（270 辆全部材质都无 + 8 辆混合车；无 MR 贴图的材质共 **566 个**）
（老式车，内联槽名是 legacy `albedo`/`normalmap`）；有 MR 贴图的现代车那侧数据正常
（粗糙均值 142–178、金属均值 22–86、仅 3–14% 像素 >200）不受影响。
**客户端口径（实测）**：这批车在客户端**根本没有 `_RM`/`_MISC` 文件**（40 辆 / 2996 材质抽查
**0/2996** 存在），作者属性是 `inGlossiness`（**0.5 ×2899 / 0.4 ×84 / 0.3 ×13**）与
`inSpecularity 0.5`、**无任何金属度属性**（0/2996）——即客户端把它们当**涂装钢铁（电介质）**渲染，
所以"解析不到贴图"是对的，缺的是**因子**。

**改动**：`tools/export_tank_glb.py` 新增纯函数 `missing_mr_factors(m)`——无 MR 贴图时写
`metallicFactor = 0`（若确有 `metallic`/`metalness`/`metalAmount` 属性则照用并钳到 [0,1]）、
`roughnessFactor = 1 − inGlossiness`（缺省 0.5 → 0.5）。**278 辆车全部重导**（566 个材质补因子；有 MR 贴图的车 465 辆 GLB 逐字节不变——该分支不触发）。

**验证**：`tools/test_export_tank_material.py` **3/3**（缺省/glossiness 映射/显式金属度与钳位）。

**blast radius**：270 辆坦克 GLB 重导 → 重打包（体积微增：每材质多两个标量）；**COS 已同步**
（2026-10-10 随坦克件批次上传，见下条；回拉逐对象 sha256 一致）。

### 坦克环境光隔离：去掉与 IBL 重复的半球环境（2026-10-09，未发版）

**触发**：用户"坦克整体仍偏亮，继续"（承上一条"发白"的因子修复之后）。

**根因**：坦克在客户端是 **`BlinnPhongAllQualities` 模板**（.sc2 实测引用）——ULTRA 档 →
`PBR.material`，其片元光照（`pbr-lighting.slh`）**环境项只有两张 IBL 立方图**，**没有半球/ambColor
这一层**；而本仓为 Lambert 回退材质挂了一盏 `HemisphereLight`（兜底 2.4、颜色＝逐图 `sun.ambient`），
坦克照单全收 ⇒ 坦克环境＝半球 + IBL 双层叠加。

**实测（两侧可比口径，上半球均值）**：客户端 `IBL/diffuse` 立方图（64²）经其 `environmentGamma` ×
`environmentMultiplier` 后的**有效漫反射辐照** = erlenberg **0.79** / medvedkovo 0.81 / himmelsdorf 0.46；
我们的 `ibl.webp`（specular mip0 → 等距柱状）上半均值 × multiplier = **0.37 / 0.64 / 0.39**，**再加半球
≈0.69** ⇒ 合计 1.06 / 1.33 / 1.08 ⇒ 相对客户端 **1.3–2.4 倍过亮**（与观感一致）。
⚠️ 由此也纠正一个方向性判断：**客户端的 IBL 比我们的更亮**（其 `environmentGamma` 0.6–1.4、
multiplier 0.8–4.0 在引擎侧作用于立方图，我们只用了 multiplier）——所以不能靠"压 IBL"来降亮。

**改动**（WotbTools 前端，本仓数据面零改动）：新增 `patchTankMaterial`/`applyTankLightingFix`
（`frontend/src/scene/sceneryMaterials.js`）——只给坦克的 `MeshStandardMaterial` 在
`#include <lights_fragment_begin>` 之后插入 `irradiance = vec3(0.0);`，把 three 的
**环境/半球/光探针**累加项清零；IBL 在 `lights_fragment_end` 才 `irradiance += iblIrradiance`
（**三 r165+ chunk 顺序逐行核对**），故 IBL 与直接光完整保留 ⇒ 结构上等于客户端的 ULTRA 口径。
幂等（`userData.__tankAmbientFixed`）+ `customProgramCacheKey` 加 `|tank-noambient` 后缀
（否则会复用未打补丁的同参数程序）。接线点在坦克装配处一行（`scene.add(v.glb)` 之后）。
可调旋钮 `TANK_ENV_INTENSITY`（1 = 保持逐图 IBL 总量）。

**验证**：着色器门禁新增 `tank-pbr(ambient-isolated)` 用例，**10 用例 / 9 程序 / 0 编译失败**；
WotbTools 全量 **2955 通过 / 2 跳过**；守卫锁"清零行 + 幂等标记 + 缓存键后缀 + 接线"。
**未做**：按 `environmentGamma` 重烘 `ibl.webp`（今日已记录在案，若仍需对齐客户端 IBL 亮度，
下一步是它 + 相机/曝光随动，属独立小批）。

### 坦克静态变换烘焙 + StateSwitcher 状态过滤（Rhm.Pzw. 炮盾顶盖贴地，2026-10-10，未发版）

**触发**：用户"rhm pzw 炮塔有一个组件没放对位置（看起来是和主炮相关的，相应跟随旋转俯仰），
但是它初始在装甲查看器的底面上"。

**真因**：该件 = 28689（`G125_Spz_57_Rh`）的 `gun_01_mask_cap`（炮盾顶盖，皮肤槽 DecorItem）。
客户端把它放在 `gun_01_mask_cap_pivot` 的 world 变换 `[0, 1.855, 2.278]` 下（自身 local 为零、
world 同值——引擎会施加；对照组：Souleater 的 lamp / F119 的 cap 用负 local 抵消成 world=0，
原始坐标本就正确），而导出沿用 BlitzKit 契约把**所有**节点变换写成 identity ⇒ 网格留在自身
局部坐标 `z≈0`（整块贴地）。它在场景树里挂在 `gun_01_mask` 之下、前端姿态系统照常带着它转
——"跟随俯仰 + 初始贴底"与报障完全吻合。三份 model.glb（`models`/`local_models`/资产包）逐字节
一致 ⇒ 不是本地导出引入，是**契约缺口**：非姿态节点的静态变换在数据面没有落点（WotbTools
只有 GLB、没有 `.sc2`）。场景脚本只对该 pivot 做 `SetRotation`（俯仰驱动动画），从不设平移。

**同轮修掉的同类缺陷（全库 735 辆扫描）**：**StateSwitcher 容器**（`state_entity_NN` 等）此前
不过滤初始态，开/闭两套形态叠着导出——22385 `Oth92_JagdPantherII_Titan` 的护盾实测双份
（00 容器 open 几何 + 01 容器 close 几何）。判据取客户端初始态：`ssc.activeState` 索引
`ssc.stateN` 名对应的子实体、只导激活那个；**越界（-1）= 整容器关闭**（9073/12657/17777/24945
逐辆核对：成对容器恰有一个 `activeState ≥ 0`）。`*_hide_elements*` 容器按产品决策例外
（拆件/皮肤变体全渲染）。**不烘的边界**（都有反例钉死）：姿态节点 `hull`/`turret_NN`/`gun_NN`/
`gun_NN_mask`/`chassis_*`（Pershing 的 `gun_01` 作者残值 +0.478z——烘了炮管穿炮盾顶）与
**场景根**（M-5-Y 根 +0.82y——视觉/碰撞/`models.pb` 原点同处"根前"空间，烘根会拆散三者）；
判据用局部 TRS 累乘（`world*` 含姿态祖先残值）。`IDENTITY_EPS = 1e-5`（数据里存在 1e-6~3e-6
量级的浮点噪声残渣，而最小待修作者值 2 mm 起，两档差两个数量级）。

**改动**：①`tools/export_tank_glb.py` 新增 `is_pose_node`/`local_trs`/`compose_static`/
`matrix_is_identity`/`bake_vertices`/`state_keep_children`——非姿态节点的累计 TRS 烘进
POSITION/NORMAL（法线逆转置 + 归一化，纯平移不碰 NORMAL），GLB 节点仍全 identity
（**消费方零改动**）；单位变换走字节不变快路径。②同口径接入碰撞导出（实测全库为姿态名
⇒ 逐字节无变化）。③`scripts/export_asset_pack.py --refresh-manifest`（按包内容重算 sha256，
闭掉 manifest↔合成后 ground 的既有偏差）+ `tools/upload_asset_pack_cos.py` 跳过判定改
**桶内 manifest 的 sha256**（旧逻辑只比 `Content-Length`——顶点烘焙只改内容不改尺寸，会被
静默跳过、修正永远上不了线）。

**验证**：新单测 `tools/test_export_tank_static_transform.py` **15/15**（姿态判据/四元数/
平移·旋转·非均匀缩放烘焙/状态规则五分支；客户端在场集成：28689 顶盖落位 z∈[2.278,2.320]、
22385 只剩初始态护盾、未命中车逐字节一致）+ `tools/test_upload_asset_pack_skip.py` **5/5**
（跳过判定优先级：sha256 > Content-Length 回退，锁"同尺寸不同内容必须传"）；全量重导 735 辆
逐字节 diff：变化 = **23 辆 model.glb**、**collision.glb 0 辆**、其余 **712 辆零字节变化**
（清单与"未做"项见 [local-model-export.md](local-model-export.md) §5.1）。

**blast radius**：23 辆坦克 GLB 重导 → `data/cache/{models,local_models}` 同步（两目录恢复
逐辆逐字节一致，1470/1470）→ 资产包 `glb/` 替换 23 件 + `--refresh-manifest` 重算（4239 条；
36 张 ground 条目并入正轨）→ **COS 已同步（2026-10-10，只传坦克 glb）**：
`upload_asset_pack_cos.py --only glb/` 上传 **294 件 / 583.8 MB**（本批 23 辆 ∪ 上条 MR 因子批
271 辆——后者此前标着"COS 未同步"，本次一并带齐；其中 **3 件是"同尺寸不同内容"**，按旧
Content-Length 判据本会被静默跳过）；manifest 走**远端清单就地补丁**（只登记这 294 条，其余
条目照桶内原样——地图侧另会话在飞，不越界）；**回拉逐对象 sha256：1470/1470 glb 一致、0 失败**。
WotbTools 侧零改动（纯数据面）。与 BlitzKit 的逐字节一致在 23 辆上**有意打破**（其余 712 辆仍逐字节一致，
`compare_tank_glb.py` 的回归锚不受影响）。**未做（在案）**：①碰撞场景的 `*_state_NN` 变体
（6 辆）不做状态过滤——碰撞 `.sc2` 不带 StateSwitcher，初始态不可判定（详见 §5.1）；
②消费方 WotbTools 侧：装甲查看器把 `gun_01_mask_cap` 按节点名前缀 `gun_\d+_` 归入
"炮盾/炮管"分类（悬停/点击的归类显示），属前端 `scene/tankViewer.js` 的判定口径，本仓
`frontend/` 冻结、不改。

### 出屏口径改客户端 linear（A2，2026-10-09，未发版）

**背景**：审计 A2——我们的出屏是 `ACESFilmicToneMapping + toneMappingExposure 1.15`（自装甲查看器
沿用，非客户端推导），而客户端**没有任何 filmic 曲线**。为迁就它派生过一批补偿旋钮（0.75 压暗、
亮色板、加深阵营色、四处 `toneMapped=false`）。

**客户端证据（逐行核过客户端 `Materials/Shaders`，解出至 `tmp_analysis/shaders/`）**：
- `Default/pbr-fp.sl:481-491`：Uncharted2 与 Hejl/Burgess 两段曲线**都注释掉**，生效行是注释写着
  `Linear to sRGB conversion without tonemapping` 的 `LinearToSRGB`（坦克走这条 = 线性着色 → sRGB）。
- `Default/materials-fp.sl:385`：非 PBR 材质 `output.color = outColor` **直写、不做变换**
  （与我们自定义伽马空间着色器同口径——本条也是 C3 的判据之一）。
- `Utilities/exposure-tonemapping-fp.sl:28-30`：`1 - exp(-luminance × exposure)` **同样注释掉**，
  生效行 `output.color.rgb = luminanceSample * exposure`——线性 × 曝光，且**乘在显示空间**（后处理级）。
- **exposure 无可抄的静态值**：shader 里是 `[auto] property float exposure`（运行时自动曝光）；
  扫过客户端全部小体积配置（`GraphicsPresets.yaml` 只给质量档，`Materials*`/`3d/Configs` 无 exposure
  引用），唯一命中就是该 material 与其 shader 本身。

**实现（WotbTools，纯前端、数据面零改动）**：`playbackScene.js` 里
`renderer.toneMapping = LinearToneMapping`、`toneMappingExposure = 1.0` ——
`= saturate(exposure × 线性色)` → sRGB 编码，与客户端"线性 × 曝光 → 编码"同式。
**不留 `?tonemap` / `?exposure` 旋钮**（2026-10-10 撤除）：客户端曝光是**运行时自动曝光**
（`[auto] property float exposure`），全客户端数据没有一个静态值可抄 ⇒ 任何替代参数都只能手工调，
属"无客户端依据"；自动曝光本身**尚未实现**，如实记为缺口（不拿手调参数盖过去）。

**离线标定（写进决策依据，非肉眼）**：按 three 的 ACES GLSL 逐字实现后拟合——**`ACES@1.15` 的等效
显示增益 = 0.974 ≈ 1**（log-uniform 权重、x∈[0.02,1.5]，log-RMS 0.168）：即默认切档**不改变整体亮度**，
两档之差是**曲线形状**——暗部（x=0.02）linear 亮 +62%（ACES 压暗阴影）、中间调（0.18–0.3）linear 暗
−14.5%、高光 ACES 到 0.98 仍在滚降而 linear 在 x≥1 直接钳到 255/255。这就是客户端的样子（无肩部），
故取 client 口径（不留切换旋钮；2026-10-10 撤除 A2 的两个调参旋钮）。

**已知偏离（如实记）**：① 出屏曲线/曝光只作用于走 three 内建管线的材质（**坦克**）——这不是遗漏，
而是与客户端同构：legacy 类材质（地面/布景）在客户端也是 raw 写出、**不经出屏变换**（C3 已核实，
见下条）；② **自动曝光未实现**（客户端 `[auto] property float exposure` 无静态值可抄）⇒ 整体亮度
可能与客户端有系统性差异，属如实记录的缺口；③ 装甲查看器（`tankViewer.js`，另一视图）用
ACES + 1.15，属独立视图、未动。

**验证**：WotbTools 守卫锁"只允许 `LinearToneMapping` + 曝光 1.0、且不得出现 `?tonemap`/`?exposure` 旋钮"，聚焦 **52/52**、全量 **216 文件 / 2957 通过 / 2 跳过**；着色器门禁 11 用例
/ 10 程序 / 0 编译失败（三处计数为 2026-10-09 撤回动态阴影后的现值）。**blast radius**：前端一处
渲染器设置 + 文档；数据面、资产包、COS 均未动。

### 采样空间口径核实（C3，2026-10-09，未发版）

**背景**：审计 C3 的未定项（§五 #1"逐贴图采样空间 sRGB vs 线性……需按槽位实测"）——若地图贴图是
sRGB，我们的地面链（原样采样 + 伽马空间相乘 + 含 `×2` 常数）会比客户端亮约 1.4×。本轮把它实测钉死。

**实测（本机客户端 DDS 头逐文件解码，非推断）**：

| 槽位/资产 | 实测格式 | sRGB 标志 |
|---|---|---|
| **PBR 坦克 albedo**（`images_pbr/*_BC`，如 `G110_Mauschen_BC`） | **DX10 `BC1_UNORM_SRGB`** | ✅ 有 |
| PBR 坦克数据槽（`_NM/_RM/_MISC/_MASK`） | legacy DXT5 | 无 |
| **老式坦克 albedo**（`images/`，如 `B-1bis_captured`） | legacy DXT5 | 无 |
| 地图地面（`landscape.dds`） | legacy **DXT3** | 无 |
| 地图 colormap（`md_colormap`） | legacy DXT5 | 无 |
| 全局内容（`00_global_content` 抽 497：detail/法线/贴花） | legacy DXT1/DXT5/DXT3（少数 DX10 `BC1_UNORM` 非 SRGB） | 无 |

另外两条排除性证据：① 场景的 `textureSampleStates` 恒为 `0x201940`（himmelsdorf 2586 处、**逐槽位
全同**：albedo/lightmap/normalmap/tileMask/flowmap 一个值）⇒ 它不是按槽位的 sRGB 判据（BlitzKit 亦
仅原样存储该值）；② 材质文件（`Textured.material` 等）无采样器/色彩空间声明，渲染态只有
depthTest/cullMode/blend——**sRGB 决定只来自 DDS 格式标志**。

**客户端着色器口径（与格式互相印证）**：PBR 类（`pbr-fp.sl`、`tilemask` 的 `LANDSCAPE_PBR` 分支）
在末尾**显式** `LinearToSRGB`（即其运算在线性空间、入参已解码）；legacy 类（`materials-fp.sl`、
`tilemask` 非 PBR、`water-fp.sl`、SpeedTree legacy）全文**无任何编码调用**、raw 写出（即原样采样、
显示空间运算）。引擎自身的色彩转换都是**逐 pass 显式属性**（`cubemap-mipmap-copy-fp.sl` 的
`convertSRGBToLinear`——若引擎会按标志自动解码，这个开关就没有存在意义）。

**结论：客户端是"两类并存"，我们的两条链已各按各类对齐，无需改动画质**：
- legacy 类 ↔ 我们的自定义 ShaderMaterial（地面/布景/水面/叶卡/天空/效果片）：全部显式
  `NoColorSpace` 原样采样 + 显示空间运算 + raw 写出（含 `×2` 常数与客户端同空间做）——**审计担心的
  "1.4×" 前提（"若源贴图是 sRGB"）被证伪**（legacy 贴图无 sRGB 标志，原样采样就是客户端行为）；
- PBR 类 ↔ 坦克（GLTFLoader 默认 sRGB 解码 + three 线性运算 + 出屏编码）：与客户端
  `_UNORM_SRGB` 硬件解码 + `LinearToSRGB` 出屏同构。

**附带定案**：出屏曲线/曝光只作用于坦克（客户端 legacy 材质同样不经出屏变换）；IBL 等距柱状
（`envTexture`）维持 sRGB 解码（PBR 类环境输入；`environmentGamma` 未接仍是已知残留，见期 2d 条目）。

**守卫（WotbTools）**：材质模块零 `SRGBColorSpace`；场景侧 sRGB 赋值为**白名单恰 4 处**（底图 /
IBL / 基地标记画布 / 伤害飘字画布——都是显示内容）；地面分层纹理不得改采样空间。新增一处 sRGB
（典型误用：给地面分层纹理设）会打破计数 ⇒ 必须先分类。

**blast radius**：零数据面、零资产改动（纯核实 + 守卫 + 文档）。

### 细节层接入（B1，2026-10-09，未发版）

**背景**：审计 B1——客户端 `MATERIAL_DETAIL` 的 detail 纹理层我们完全没做（"近景墙面/地面缺细节层，
发平"），当时按普查估 437 批次受影响。本轮实测（36 图 + 邻图，`collect_renderables` 全量遍历）：

- **453 个实例 / 10 图**绑了 `detail` 槽：forgecity 121、rift 103、skit 63、fort 50、holland 39、
  lumber 28、neptune 27、plant 12、idle 9、faust_night 1；
- 材质**全部**是 `~res:/Materials/Detail.material`（`UniqueDefines: [MATERIAL_TEXTURE,
  MATERIAL_DETAIL]`，**全档无条件**）——`DetailAllQualities.material` 的档位表
  （`ULTRA/MEDIUM: [MATERIAL_DETAIL]`、HIGH 缺项）**没有任何实例使用**，故不涉档位歧义；
- 其中 **398 个同时是光照图批次**（Detail.material 实例多数启用 LightMap 预设），**55 个无光照图槽**
  （neptune 27 / plant 12 / idle 8 等，走我们的 Lambert 降级路径）；
- `detailTileCoordScale` 实测（属性 blob，`_prop_floats` 同口径）分布：2/3/4/9/12.5/25/40×50/50/150
  等（默认 = 着色器 property 默认 `(1,1)`，14 个实例缺该属性）；
- detail 贴图：`detail_maps/rock_detail`、`buildings/images/{brick_tile_ditail,floor_ditail}`、
  `environmet/images/env_*_tile`、`surroundings/tile_edge` 等平铺砖缝/岩面/地砖。

**客户端公式（逐行核过 `materials-vp.sl:122/233` + `materials-fp.sl:100/264/347`）**：
`varDetailTexCoord = varTexCoord0 × detailTileCoordScale`（**UV0**，非 UV1——故无逐实例变换、
材质级 extras 足够）；DRAW PHASE 末尾（光照图/环境反射/拼花砖**之后**、alpha 之前）
`color *= detailTextureColor.rgb * 2.0`（×2 把均值 ≈0.5 的平铺贴图拉回 1.0 附近）。

**实现**：
- 数据面（`tools/export_map_glb.py`）：`ClientMaterialFamily` 新增顶层 `detail` 判据（与
  `lm_top` 同口径：只看 `Presets:` 之前）＋ `detail_capable`（绑槽**且**材质声明 define——
  与`lightmap_capable` 同设计，fail-closed）；材质 extras 写
  `detail = { texture: <glTF texture 下标>, scale: [sx, sy] }`（JPEG q90、与 albedo/lightmap
  同"翻回文件原始行序"）；材质去重键加 `(detail 贴图, scale)`。
- 前端（WotbTools）：`makeLightmappedMaterial` 第 4 参 `detail`——`#ifdef MATERIAL_DETAIL` 分支
  （`vDetailUv = uv * uDetailScale` + 环境反射块后 `color *= texture2D(uDetailTex, vDetailUv).rgb × 2.0`），
  走原样采样（`NoColorSpace`，C3 口径）；**无光照图的 55 个实例**走新
  `patchSceneryDetail`（Lambert `onBeforeCompile` 注入 `<map_fragment>` 之后同一乘法 +
  `customProgramCacheKey` 标记；convMat 缓存键同步加 detail 身份，防同图不同砖纹串用）。
- 俯视烘焙页：键与内核**逐字同构**（`detailTexOf` 置 null 桩）——细节层不进俯视底图（该页是
  Lambert 简化渲染且不做 extras 贴图解析，与它未接光照图/环境反射同因；其刷新口径见
  [game-data-sources.md](game-data-sources.md) §5.5）。

**实测效果面**：单图只多 1–17 张 detail 贴图（**51 个材质**带 extras）；10 图 scenery.glb
**293.2 → 295.4 MB（+2.2 MB）**，36 图 scenery 合计 **836.6 MB**；逐图增量最大 holland +0.47、
idle +0.51 MB（forgecity −0.09 属重编码自然波动）。

**验证**：WotbTools 守卫（乘法式/UV0×scale/位置在环境反射后/依赖解析/两条路径接线/缓存键身份/
烘焙页桩）+ 着色器门禁新增 `lightmapped+detail(B1)` 用例（**12 用例 / 11 程序 / 0 编译失败**）；
上游新增 `test_detail_capability_criterion`（顶层 vs 槽位 fail-closed）。全量计数（2026-10-09 收口
现值）：WotbTools **218 文件 / 2997 通过 / 2 跳过**、上游 Python **85 通过 / 12 文件**（均含 D2 与
他批在途的新增）。**blast radius**：10 图 scenery.glb 重导（`--scenery-only`，
地面/合成缓存**未动**）→ 包内 10 件替换 + `--refresh-manifest` 重算（**COS 未同步**）。

### 拼花砖口径核实（B4，2026-10-09，未发版）与地面动态贴花（D2）结论

**B4（`TILED_DECAL_MASK` + `decalTileColor`）——核实结论：本版客户端内容零启用，不实现。**
实测证据（三轮扫描，全部穷尽）：
- **76 个材质文件**（`Data/Materials/*.material.dvpl` 全量）**无一**声明 `TILED_DECAL_MASK`；
- **全部地图实例**（55 个注册表条目、`collect_renderables` 全量遍历）**无一**绑定 `decalmask` /
  `decaltexture` 槽——全槽位名普查（`albedo/lightmap/detail/decal/...`）里不存在这两个槽；
- 全属性键普查**无** `decalTileColor` / `decalTileCoordScale`（只有 `decalTintColor` 47 处，
  属 `MATERIAL_DECAL` 静态贴花那条线，即审计 B3 的既有烘焙路径）。
⇒ 按 fail-closed（不造未经内容验证的着色器路径）**不实现**；公式已读好备查：唯一乘入点在
`materials-fp.sl:344-348`，`color.rgb += (tile.rgb − color.rgb) × tile.a × maskSample`
（`mask = FP_A8(tex2D(decalmask, uv0))`、`tile = tex2D(decaltexture, uv0 × decalTileCoordScale)
× decalTileColor`），位置在环境反射之后、细节层之前；顶点侧 `materials-vp.sl:222-231`。
**B3 的"动态铺展"（`TILED_DECAL_SPREADING`/`decal-spreading.slh`）同因未启用**——`spreadingProgress`
等属性在内容里零出现；静态 `decal` 槽（333 实例）是烘焙进 albedo 的静帧贴花，视觉等价、无需动态。

**D2（地面动态贴花：碾压痕/弹坑/血迹）——不做（2026-10-10 撤除其近似实现）。**
实测客户端构成：地面命中 = **FX 粒子**（`Data/3d/FX/<map>/hit_surface/groundHit_<弹种><序号>.sc2`，
8 个发射器引用 `~res:/Gfx/Particles/...`）+ **游戏层 geo-decal**（引擎 `GeoDecalManager` 的
`DecalConfig{albedo,normal,specular,dimensions,mapping}`；**贴图路径由游戏层传入，不在可得数据面**）
——场景里的 `DeferredDecalComponent`（490 处）是静态投影贴花，与弹痕无关。客户端画质选项：
Windows 默认档（High）`HitMarks=true`、`TankTreads=false`。
2026-10-09 曾以**程序化画布贴图 + 固定尺寸 + 池 420** 做"持久灼痕"近似（前端），但贴图、尺寸、
上限三处都是自定参数、**没有客户端依据** ⇒ 2026-10-10 依"只保留客户端有实际依据者"全部撤除
（含 `impactDecals.js` 与其单测）。要重做的前置条件是**先定位到客户端弹痕贴图**（游戏层资产，
当前数据面无）；碾压痕客户端 Windows 默认档即关、血迹无数据源 ⇒ 本就不做。

### 图片注册表撞键修复（"新湾桥面变黑"，2026-10-09，未发版）

**现象与定位（用户报障）**：新湾（forgecity）挡土墙 `env_fs_retaining_wall_017.sc2` 的沥青片
（state-1 网格）渲染成黑块。逐层数值复现：该网格材质 = `Detail.material` 实例，
`flatColor = 0.7598`、albedo = `environmet/images/env_fs_bitumen_color.tex`（客户端原图
RGB 均值 **0.342**，正常沥青）——但包内 GLB 里它指向的图片均值只有 **0.0281**（1KB 平黑图）。

**根因：`GlbBuilder.add_texture` 按 key 去重，而 key 不含"内容变体"。**
albedo 的嵌入像素**不只由路径决定**——`FLATCOLOR` 染色（`apply_flat_tint`）与 UV1 覆盖烘焙
（`bake_uv1_overlays`）都会改写它。同一 albedo 路径下有两个材质时：
`Textured.material` 实例（flatColor≈0.082 → 近黑图）先注册 `("jpg", albedo_path)`；
`Detail.material` 实例（flatColor=0.7598，应为 0.258 均值）调用同名 key 被**静默顶替**成那张
近黑图 ⇒ 渲染成黑块。**全树普查：54 个图条目、788 个变体**受同类顶替影响（长期存在——旧包
（envcheck 副本）里同一材质的 albedo 就是 0.028，非近期批次引入；用户在本轮 B1 后细看时发现）。

**同族问题**：动画层掩码的 key 曾写作 `("mask", albedo_path)`——但掩码内容来自 `alphamask`
文件，同 albedo 不同掩码的动画层（烟/瀑）会复用首张掩码。一并修正。

**修复**（`tools/export_map_glb.py`）：
- 调用侧计算 `img_variant`：UV1 烘焙 ⇒ `("bake", decal, mask, flat_rgb)`；仅染色 ⇒
  `("tint", flat_rgb)`；原图 ⇒ `None`。嵌入 key 改为 `(“jpg”|“png”, albedo_path, img_variant)`。
- 掩码 key 改为 `("mask", alphamask 路径)`。
- 其余 `add_texture` 调用（water normal/cube、envMask/envCube、lightmap、detail）逐一核过：
  key 均按**内容来源路径**，无同类问题。

**实测效果面**：36 图 scenery 全部重导（`--scenery-only`，地面/合成缓存未动），**净 +90.8 MB**
（36 图 836.6 → ~927.4 MB；多出来的正是此前被顶替掉的变体图——`rock` +13.3、`desert_train`
+10.0、`rift` +9.2、`idle` +8.5 MB 等，撞键越密的图差值越大）。用户报障点实测：
该网格 albedo **0.0281 → 0.2580**（= 0.342 × 0.7598，与公式吻合）。包 4964 文件 /
**4288.2 MB**（manifest 口径）。**多处表面观感会变**——那正是被顶替材质恢复各自正确染色/烘焙
变体的目的；若某图观感突变，优先按本条目核对（而不是当作新 bug）。

**验证**：新增 `tools/test_export_image_keys.py`（4 条：不同变体不成同图 / 同变体仍去重 /
原图与变体共存 / 掩码按 alphamask 去重）；上游 Python 全绿；重导 10 图冒烟逐节点核对通过。
**blast radius**：36 图 `scenery.glb` 重导 + 包内 36 件替换 + `--refresh-manifest` 重算
（**COS 未同步**）。

### 地形网格改客户端同构（"地形穿出石板面"，2026-10-09，未发版）

**现象（用户报障 + 截图 + 两次 pick）**：新湾（forgecity）挡土墙处，实机来回点两次——一次落在
墙上、一次落在"突出于石板面的不规则地形"上。逐层核到根因后确认：**不是数据错位**——包内
`terrain.u16.bin` 与客户端高度图**逐字节一致**（列翻转后 100% 相等）、141 个地面吸附建筑在
引擎公式下落位 Δ=0.00、墙的世界变换 = `.sc2` 的 `tc.worldTranslation`、用户点位的地形
（31.90 m）与墙顶（31.91 m）本就同高（挡土墙语义）——**而是我们的地形网格与客户端不是同一个格点**。

**引擎口径（源码逐行）**：`Landscape` RO 只有 `hmap`+`bbox`（程序化地形）；`LandscapeSubdivision`
的顶点全部来自 `Heightmap::GetPoint(x, y)` = **顶点落在 texel 原位（`i/size·span`）、高度取
texel 值（不平滑）**；`Landscape::GetHeightAtPoint` 的查询同为 `fx = size·(x−min)/span`（**除
size，不是 size−1**）。我们此前 `PlaneGeometry(seg) + sampleHeight(双线性)`：顶点与 texel 网格
**相位错位**、格宽亦不等 ⇒ 陡坡/切槽处地形面沿坡向外探出（实测等高线水平 **+0.5–0.75 m**、
同点高度 **+0.04 中位 / +1.34 最大**）。墙顶与地形同高，俯瞰视角把这点亚米级错位放大成"地形
穿过墙的贴面"。

**修复（前端，WotbTools）**：地形几何**直接建在场景系**（不再 `PlaneGeometry`+旋转）、高度按
引擎 `GetHeightClamp` 夹取；`sampleHeight` 同步改引擎口径（`(x/span+0.5)·n`，此前 `(n−1)` =
0.2% 拉伸、边缘偏一整格）。实测 vs 引擎采样：墙区 p95 **0.65 → 0.28 m**、max **1.86 → 1.60 m**。
`?debug` 挂 `window.__terrain` 诊断钩子。
**真正的根因（用户追问"客户端无论如何看都没问题"后定案）**：客户端的 landscape 是
**自适应 LOD 网格**（引擎 `LandscapeSubdivision.cpp:252-258`）——最粗层 8×8 四边形（每格
`⌊(size−1)/8⌋` texel），细分判据 `|maxErr| / (distance × tanFovY) ≥ 0.014`
（`SubdivisionMetrics.normalMaxHeightError`）或 `|maxErr| > 3 m`（`normalMaxAbsoluteHeightError`）
⇒ **300 m 视距下容差 ≈ 2.9 m、几乎整图停在最粗层**：顶点是高度图的**抽样**，
厘米~分米级毛刺根本不进网格；贴近才细分。我们此前**逐 texel 全画** ⇒ 高度图里所有毛刺都在
⇒ 在"与结构齐平"（吸附设计，Δ=0.00）的接缝上被顶出来，且**与相机角度无关**（所以客户端
怎么看都干净、我们怎么看都有）。实现改为 `terrainMesh.js`（纯函数四叉细分：顶点 = texel
原值、容差 = 客户端公式、3 m 封顶 + 节点数保护）+ `maybeRebuildTerrainLod`（视距变化 ≥25%
重建，毫秒级；fov 用实际相机取）；阈值系数固定 **1 = 客户端口径**，不留调参旋钮
（`subdivisionMetrics` 的 `tolScale` 参数仅供单测）。

**补修（用户"这次引入了横竖条纹"）**：自适应网格首版用"细侧缝合 k 网格"补共享边——相邻叶混排
多档尺寸时（实测 forgecity：1.17 / 2.34 / 4.69 / 9.38 / 18.75 / 37.5 / 75 m）仍留 **0.58–0.66 m**
的 T 型接缝（沿四边形边成横竖细缝，透出下层地面/水面 = 用户看到的条带）。现口径：顶点高度 =
**该处最粗相邻叶所在层级的边界直线值**——顶点落在该叶**边的内部** ⇒ 取该边直线；落在**格点角** ⇒
texel 原值（若有更粗的象限，它才是"最粗" ⇒ 与本节"抽样而非平均"一致）；边的端点自身可能被更粗
象限拉扯 ⇒ **端点递归**（层级严格变大 ⇒ 良基）⇒ **同一 `(i,j)` 恒得同一高度**、细侧折线落在粗侧
直线上 ⇒ 无需 2:1 平衡，比首版的"平衡 + 缝合"更少代码、更强保证。另按 `(i,j)` 去重顶点（索引网格
⇒ `computeVertexNormals` 法线跨叶连续，不再有逐叶硬边）。实测最大缝 **0.655 m → 1.8e-6 m**
（后者 = 顶点以 float32 存储的 1 ulp，33 m 高度处）。被取代的 `terrainLattice.js` 与其单测已删
（遗留死码）。

**补修二（用户"地形把铁轨相关组件遮挡了"，全面复核引擎后定案）**：报障点 = forgecity
`env_fs_rails_002sc2`，两点拾取 (35.3, 135.8) / (25.0, 131.4)。实测该铁轨是 z ∈ **22.30–22.60** 的
贴地薄板、地形真值 **22.330** ⇒ 净空只有 **0.27 m**；旧口径在 252 m 视距处留下 **75 m 整格**，
该点网格高 **22.889 = 高出真值 +0.56 m ⇒ 越顶盖住铁轨**。逐行复核
`LandscapeSubdivision.{h,cpp}` 后发现我此前**漏了两条客户端口径**：
① 一个补片 = **8×8 四边形**（`PATCH_SIZE_VERTICES−1`），层级 k 的补片边长 `size>>k` texel、
   补片内顶点间距 = 补片边长/8 ⇒ **终止的补片内部仍是 8×8 网格**（我此前把补片当成"一个四边形"
   ⇒ 同距离下单元格粗 8 倍）；
② 细分判据是**三条**（`SubdividePatch:252-258`，`||` 短路）：屏幕半径
   `radius/(patchDist·tanFovY) ≥ maxPatchRadiusError`（0.45，**最左项**）、屏幕高度
   `|maxErr|/(errDist·tanFovY) ≥ maxHeightError`（0.014，距离取**最差样本位置**）、绝对
   `|maxErr| > 3 m`；阈值随 fov 在 zoom(6.5°)/normal(70°) 两套预设间插值（`SubdivisionMetrics`；
   引擎 `Camera::GetFOV()` 是**水平** fov），`tanFovY = tanf(fov/2)/aspect ≡ tan(vfov/2)`。
   误差量 = 补片内 8×8 格各自的"一步细分误差"的最大值（我此前只对整格取 5 个采样点 ⇒ **窄特征
   （路堑/沟槽）被漏检**，粗格直接跨过去）。
补齐后实测（forgecity 真高度场）：同两点在 78 / 93 / 140 / 250 m 视距下网格 = **22.330 / 22.335**
（与真值逐点一致，旧口径 +0.18 ~ +0.56 m）；终止补片判据值 **零违反**且紧贴阈值
（radiusError 0.437/0.450、heightError 0.0139/0.0140 = 恰好停在客户端允许的最粗处）。性能：
远景 25 k / 近景 50 k 四边形（旧均匀网格 262 k），一次重建 **≈18 ms**（定型数组缓存 + 预分配 +
"半径判据先行"：任一样本误差 ≥ `maxHeightError·d_max·tanFovY` 即必然命中，
当场细分 ⇒ 判定与逐项判定完全等价）。守卫：`terrainMesh.test.js` 9 用例（**终止补片满足三条判据** /
**单元格边长 ≤ 0.08·视距**〔判定性回归：旧口径模拟比值 3.61 失败、现状 0.46 通过〕/ 贴地薄板 /
各视距单元格尺度 / 无裂缝 / 无空洞 + 全朝上 等）。

**补充（同一报障的另一半：共面接缝）——查清客户端机制并落地（2026-10-10）**：
① 事实（数据层逐点实测）：铺装（地形）与挡土墙顶/建筑基础在客户端是**吸附齐平**的——沿墙顶缝 92% 的
采样 |地形−结构| ≤1 cm、地形略高者 75% ≤2 cm / 98% ≤5 cm；用户 2026-10-10 报的那处
（`env_fs_retaining_wall_017sc2`，拾取 (73.2, 31.7, 108.2)）沿墙 101 点里 97% 地形在压顶之下、
**4% 高出 5–7 cm**。② 客户端**没有人造深度偏移**：材质层 `Data/Materials/*.material` 52/52 无
`DepthBias`/`SlopeScaleDepthBias`（引擎 `sl_Parser.cpp:282-283` 认得该字段、本图未用；`.sc2` 里同名
命中只是 FX 参数 `constantDepthBias`/`depthDifferenceSlope` 的子串）；几何层同一份数据同样共面。
③ **客户端有一条此前漏掉的地形平滑机制**（逐行核对 `Landscape.cpp:CreateHeightTextureData` +
`Shaders/Landscape/tilemask-vp.sl`）：高度图被引擎打包成 RGBA8，每 mip 纹素 =
**[本层原始高度 u16, 该纹素与相距一个 step 的对侧邻居之半和 u16]**（偶数索引/末索引取自身）；顶点
着色器 `height = lerp(averaged, accurate, morphAmount)`，`morphAmount` 出自 `SubdividePatch` 末尾的
误差比 `subdivMorph = 1 − errorDelta/(error0Delta + errorDelta)`（**误差贴近阈值 ⇒ →1 ⇒ 向"双抽头
均值"靠满**）再经 `morphFunc(x)=4(1−x)⁵−5(1−x)⁴+1` 整形；最外一圈按 `zeroLodMul` 退回原始值。
**已 1:1 直译进 `terrainMesh.js`**（无自由参数；两通道 + morph + 最细层半径误差照算；修掉一处
"半径判据短路未回传误差 ⇒ morph 恒 0"的实现 bug）。实测最细层顶点 22–25% 有 >5 mm 改动；
**线性坡面上 morph 恒等**（双抽头均值在直线上 = 原值）⇒ 不动路基/坡面/贴地薄板（已有单测锁定）。
④ **结论（2026-10-10 二轮实测，回答"客户端为什么不会出问题"）**：整面墙逐 0.5 m 扫描（客户端自己的
高度图 + LOD0 网格）——"地形高出墙表面 >2 cm"共 289 点，绝大多数是墙**埋在地下的部分**（两者都
看不见）；**可见**的是墙顶边缘露头处：拾取点旁 (73.2, 107.6) 地形 31.734 vs 墙顶 31.687 ⇒ **+4.7 cm**；
x≈54–60 齐平段 **+5.5~7.0 cm**；x≈90–93.5 段最大 +22.6 cm；x≈108.5~109 段最大 +39.3 cm。⇒ 这些
高出量就在**客户端自己的数据**里（同一高度图、同一 LOD0 网格、同一变换，均已核）。客户端侧机制已
穷尽：材质层无深度偏移（52/52 + `.sc2` 关键字扫描）、`SnapToLandscapeControllerComponent` 本图
**0 引用**、导出本取 LOD0、全图仅一份高度图、两通道 morph 已实现（对直线坡/台阶恒等）。⇒ 差异只剩
**相机**：客户端贴地第三人称在墙顶之下、被墙自身遮挡，常规视距（60–150 m）下 5–7 cm 的带仅亚像素；
我们的俯瞰/低角近距相机把它展开成沿石板边的地形色带。**闭环需要一次客户端侧同机位对照**；在拿到
之前不再动渲染端。若要无论如何都看不见，只能走数据面有意偏离（地形在贴地结构覆盖处让位）。

**补修四（用户"铁轨被地形遮挡、转动/缩放后又（时不时）变正常"，2026-10-10 二轮）**：
量测（forgecity `env_fs_rails_002`，0.3 m 厚贴地基板；按"我们网格是否盖过基板**顶面**"判据、逐 1 m 采样）：
**真值（高度图双线性）在所有视距下恒定只盖 39/1262 点、最深 +16 cm**；我们的网格 **100→762 点、
最深 +32→+69 cm 且随轨道视距变化** ⇒ "时有时无"的来源。逐视距核对 LOD 层级：**同一相机位置我们的
步长从不比客户端判据允许的更粗**（拉远/拉近轨迹模拟 0/16 帧越界）⇒ 高出量属客户端判据的容许范围
（容许误差 ∝ 视距；0.3 m 厚构件在 ~41 m 外就可能被合法盖住——客户端贴地相机永远在 3–15 m 内，故
看不到）。**但我们旧实现叠加了"滞后"**：`maybeRebuildTerrainLod` 是双向 25% 滞回 ⇒ 网格长期停在
"更远视距"的更粗层级 ⇒ 粗格插值把地形抬到构件之上，随重建时机出现/消失。**已改为单向滞回**
（`terrainMesh.js` 新增 `terrainLodStale`：拉近 ≥5% 立即细化、拉远 ≥40% 才允许变粗、≥120 ms 节流）
⇒ 网格永不比客户端同一相机位置允许的层级更粗，"时有时无"消除。守卫 + 单测锁定。
（另：排查中确认导出与查询口径无误——`.heightmap` 只此一份、导出取 LOD0、`snapToLandscape` 本图 0 引用。）

### 地形让位掩码（数据面，2026-10-10）

**动机（用户定案选择"数据面"路线）**：客户端的地形是按视距自适应 LOD 的补片网格，容许误差
`0.014·距离·tan(fov/2)`（封顶 3 m）⇒ **薄贴地构件（铁轨基板 0.3 m、压顶、铺装）在远视距下会被
"合法地"盖住**：我们实测铁轨可见带（逐 2 m 采样 157 点，判据"我们网格盖过基板顶面 >3 cm"）
60 m 1 点 / 150 m 2 / 250 m 12（+15 cm）/ 300 m 50（+35 cm）/ 450 m 97（+43.5 cm），而**细真值
（高度图双线性）在同一批点上恒定 0**；客户端判据在同一相机位置给出的层级与我们一致（逐视距核对、
拉远/拉近轨迹模拟 0/16 帧越界）⇒ 与客户端"对齐"这条路已走到头。用户此前见过的"没问题"来自两处
渲染端人工补偿（`?poff` 深度偏移、`?seamfix` 结构上抬 ≤9 cm——铁轨实测需抬 +8.96 cm），二者因
"无客户端依据/需手调"被撤除。**本轮改为把作者意图显式化到数据面**：结构面就是这里的地面高度。

**烘焙（新增 `tools/bake_terrain_cover.py`，纯函数单测 `tools/test_bake_terrain_cover.py` 6 用例）**：
读包内 `scenery.glb` + `terrain.u16.bin`（同 `terrain.json` 的 size/span/zmin/zmax）⇒
① 只取**朝上**面片（世界上下分量 > 0.5×法线长，竖直墙面/桥腹不参与）；② 逐 texel 取最高朝上面
高度；③ **非对称贴地判定**：结构面 ∈ [地形−0.15 m, 地形+0.5 m]（下界 0.15 m 容下"吸附齐平"
而排除"埋在铺装下 ≥0.3 m 的箱体"——否则会把它挖出来；上界 0.5 m 容下铁轨基板这类高出 0.27 m 的
低矮构件）；④ **单向天花板**：压低量 = min(0.5 m, 地形−结构面+0.1 m)，覆盖外 20 texel 内按 (1−d/20)
线性收尾（边界连续、无台阶）——压低量恒 ≤0.5 m ⇒ 不会在陡坡旁挖沟；**+0.1 m 是几何间隙**：
压到结构面上会让两者**共面 ⇒ 逐像素 z-fight 闪烁**（首版正是如此，用户"交叠闪烁更严重了"），
留间隙后带内地形处处干净低于结构面（首版"升到原地形"的坡道在陡坡旁
实测可压低 3.6 m，已废）、只压不抬 ⇒ 不会在低处开槽；⑤ 量化同地形（u16 / zmin..zmax），存
`ceil+1`（**0 = 无覆盖**）。36 图全跑 **约 2 分钟**，每图最大压低 ≤0.475 m、覆盖处最小间隙 = 0.100 m（零共面）。

**产出与消费**：包内 `map/<key>/cover.u16.bin`（0.5 MB/图，36 图 +18.9 MB）+ `terrain.json.cover`
声明；`scripts/export_asset_pack.py` 同步复制（缓存 `<space>.cover.u16.bin`）。前端（WotbTools）
新增 `terrainCover.js` 纯函数只做 `渲染高度 = min(原高度, 天花板)`，且**只作用于渲染用高度场**——
`sampleHeight`（拾取/贴地/贴花/查询）继续用真值场，守卫锁这一点（改错会让拾取与贴花整体错位）。

**实测效果（同一铁轨可见带、同一固定视距；前后对照）**：60 m **1 → 0**；150 m **2 → 0**；
250 m **12（+15.1 cm）→ 0**；300 m 50（+35 cm）→ 7（+21.7 cm）；450 m 97 → 78（+33.5 cm）。
⇒ **用户观看区间（≤~250 m）已完全消除**；300 m 以上的残余是"地形本身高于基板"的粗格合法桥接
（客户端同一相机位置亦如此），数据掩码无法在不挖沟的前提下消除（已如实记录）。

**blast radius**：36 图各 +1 个 0.5 MB 二进制（包 4964 → **5000 文件 / 4288.2 → 4307.3 MB**），
`terrain.json` 各 +1 字段；上游新增 2 个工具文件；前端新增 1 个模块 + 1 个测试 + 守卫 6 条。
**COS 同步由发布者执行**（`COS_SECRET_ID/KEY` 环境变量，不入库）。

**同日修正：水面片与水下薄板排除出"贴地判定"**（用户"岸线拉近看还是锯齿、而线上部署版没有"定案）：
上面第 ③ 步的贴地判定会把**水面片自身**（12.260）与**水下薄板**（马利诺夫卡 25 片冰
`env_ma_ice01_*`，12.250）当成"贴地结构"⇒ 把地形压到它们之下 —— 对**铁轨/压顶**这类**岸上**构件
这是对的，但对**水体**：压下去会把**可见岸线**整体挪走并留下台阶。实测（malinovka 用户拾选窗
x∈[10,72]、z∈[−28,28]，逐行追踪"陆→水"转换列）：原始高度场 Σ|Δ列| = 15 格、方向反转 1 次；
含掩码的渲染场 **22 格、2 次**（含一处 5 格≈5.9 m 缺口）；掩码在该窗压低最多 0.475 m、p95 0.425 m。
生产环境（`cover.u16.bin` **404**、场景包也没有水面/光照图标记）因此没有这个问题——它的岸线就是
数据等高线。**修改**：烘焙时把"材质带 `extras.water` 的水面片"与"整个被某片水面盖住且不高于其
标高 +5 cm 的朝上面片"（新纯函数 `water_surfaces_of` / `submerged_faces`，与前端 `underWater`
同口径）排除出贴地判定。水下薄板的**粗格保护**改由同日新增的**岸线特征细分**承担（前端第四条
判据：与水面标高相交的补片细到 1 texel）⇒ 两者互补，掩码不再需要在水体范围内让位。
**效果**：malinovka 贴地 texel 14534 → **3167**（冰面不再入列）、被排除的面片 172（水面）+ 2372
（水下薄板）；**forgecity 贴地 14640 → 14992（铁轨保护完好，甚至更好：此前水面片会顶掉同一 texel
上真实结构的最高面）**；用户拾选窗的岸线**与原始高度场逐行一致**（30 行/Σ15/反转 1）。36 图全部
重烘、`--refresh-manifest` 重算（文件数/总大小不变：5000 / 4310.5 MB）；**COS 未同步**。
单测 `tools/test_bake_terrain_cover.py` 增至 8 用例（新用例覆盖：水面自身按材质排除、水下件按
淹没排除、岸上板保留、无水图行为不变）。

### 水面海岸线 `coastLine`（前端，2026-10-10；同日"淡出宽度"口径纠正）

**报障**：马利诺夫卡水面与地形交界处出现**锯齿状条带、非常生硬**（用户附拣选：`env_ma_ice02_08`，
world 178.1, 12.3, −31.4）。

**真因（实测，非推测）**：① 那"水面"= 25 片 `env_ma_ice02_*` 冰面片（60×60 m、统一高度 **12.260**、
材质带 `extras.water` = 客户端的 water 材质）；每片**正下方 1 cm** 另有一片不透明冰 `env_ma_ice01_*`
（12.250、带 lightmap、无 water 标记、客户端 `ro.flags = 2049` 含 `VISIBLE_REFRACTION`——那一层才是
"透过水看到的冰"，客户端在折射通道里把它画出来）。② **地形穿过水面的等高线本身是平滑的**：逐行
追踪首次越过 12.260 的列号，每行只平移 0–2 texel（单调，无噪声）⇒ 锯齿**不是**"地形戳出水面"。
③ 真正的原因 = **水面自己的贡献**：我们的 alpha 就是菲涅尔反射，岸线带里它与地形（|地形−水面| 小于
该视距深度分辨率的区间）**逐像素抢深度** ⇒ 硬边 + 锯齿；且这些 60×60 的片子有相当比例盖在岸上
（实测 `ice02_08` 覆盖处地形中位数高出水面 1.32 m）⇒ 硬边即"片的覆盖范围 ∩ 地形"。

**客户端机制**（客户端 DVPL 着色器逐行核对：`Data/Materials/Shaders/Default/water-fp.sl` 的
`RETRIEVE_FRAG_DEPTH_AVAILABLE` 分支 + `depth-fetch.slh`）：
```glsl
float depthSample = FetchDepth(projectedPosition);                 // 深度预通道 dynamicDepthPrepass
float4 sampledPosition = mul(float4(projectedPosition.xy, depthSample, 1.0), invProjMatrix);
float4 currentPosition = mul(projectedPosition, invProjMatrix);
half distanceDifference = half(currentPosition.z / currentPosition.w - sampledPosition.z / sampledPosition.w);
half adjustedDifference = abs(distanceDifference * half(2.0)) / half(projectedPosition.z / projectedPosition.w);
half coastLine = saturate(adjustedDifference);
fresnel *= coastLine;   waveOffset *= float(coastLine);
```
⇒ 水面片元与**背后表面**（深度预通道里的最近不透明面）的**相对**深度差越小越"没有水"：岸线带里
`fresnel *= coastLine` 抹掉的是**反射项**；客户端同时有折射通道（水面 α = 1、`resColor = lerp(refraction,
reflection, fresnel)`）⇒ 淡下去露出的是**同一幅地形图**，故岸线处既无硬边、也不受深度抢胜影响。

**本实现对等（不引入额外深度 pass）**：无深度预通道，改按**本渲染器自己的深度分辨率**做同一件事
（客户端 `ndcZ` 的空间口径由运行时 `ndcToZMapping` 注入、绝对值不可移植；可移植的是它掩盖的**现象**：
"深度差小到缓冲分不开"的那条岸线带）：

  `dNdcZ = |∂ndcZ/∂z|·Δz_沿视线`（`∂ndcZ/∂z = 2·f·n/((f−n)·z²)`，`Δz_沿视线 = 高差/|视线.y|`）
  **`coastLine = saturate(dNdcZ / (2·uDepthUlp))`**，`uDepthUlp = 2/2^depthBits`（上下文实测位深）、
  `near/far` 取相机实际值 ⇒ **淡出带 = 该视距下的深度抢胜带（2 ulp）**，`alpha = 菲涅尔 × coastLine`。地形高采自**渲染用**高度场
（含让位掩码 ⇒ 与深度缓冲里的地形一致，跨度与网格同源 `terrainSpanOf()`）；缺高度场 ⇒ 不淡出（fail-open）。
另：`coastLine → 0` 也顺带消掉了"水面片盖在岸上"的硬边（地形高于水面处 coast = 0 ⇒ 片无贡献）。

**两次口径纠正（均按用户报障，守卫禁止复活）**：
① 首版把淡出宽度取成"**该视距下地形单元格上界**"（≈4% 视距）——基于"盖住地形几何锯齿"的
**错误归因**，且宽度 ∝ 视距 ⇒ 远景把整片水洗掉（"水面贴图也不太对了"）；
② 第二版用 `2·(水面高−地形高)/(相机高−水面高)`（由客户端等式按 reversed-Z 反推）——
**相机越高分母越大** ⇒ 俯视时连深水也压成透明（2026-10-10 用户"从上往下看水面基本都是透明的，
只有视角比较接近水面的时候才会反射"）。**现行口径 = 深度分辨率（2 ulp）**：岸线带淡出、
深水任何角度满反射（2 m 深的水在 200 m 处 ⇒ coastLine = 1）。

**已知差异（如实记录）**：① 背后表面只算地形、不算结构/浮冰 ⇒ Malinovka 冰面上那层水保留
`2·Δh_地形/(相机高−水面高)` 的少量反射（客户端取到 1 cm 下那层 `ice01` 冰、coastLine≈0）；
② 无折射通道（水色/扰动差异，同 F1）。

**验证**：着色器门禁 water 用例编译通过（首版曾把 `uHeightmap.r` 写成分量访问、门禁当场抓出）；
WotbTools 全量 **219 文件 / 3014 通过 + 2 跳过**；两次报障各自的判据都已写进守卫
（`sceneryMaterials.test.js`：公式逐字 + 禁止"∝视距"旧口径 + 旧的"按垂直高差满幅淡出"口径）。
**blast radius**：纯前端（无数据面改动；包/掩码/manifest 均不变）。

### 水下静态件不接光照图（前端，2026-10-10）

**报障**：马利诺夫卡"水面边缘位置是**黑色**，而且和地形的分隔是锯齿状的"（用户附拣选：
`env_ma_ice02_03`，world 29.9, 12.3, −45.4）。

**真因链（全部实测，非推测）**：
1. 那圈水面是 25 片 `env_ma_ice02_*` 冰面片（统一 12.260、`extras.water`）；每片正下方 **1 cm**
   是不透明冰 `env_ma_ice01_*`（12.250），材质 `TextureLightmap.material` +
   `flags: {FLATCOLOR: 1}`，客户端 `ro.flags = 2049` 含 **`VISIBLE_REFRACTION`**——它是"只经
   折射通道看到"的件（此前一直不知道这层为什么要带折射可见位，现在闭环了）。
2. 这些冰片在**客户端自己的光照图图集**里落在**未烘焙的黑格**上：按客户端原样的
   `uvScale/uvOffset`（我们逐位抄对了：`[0.0625,0.0625,0.313965,0.188477]`）采样，图集
   `lightmaps/texture1.tex`（2048²，22% 是黑区）逐片实测 `ice01_04` 71% 黑、`ice01_07` 63%、
   `ice01_08` 64%、`ice01_11` 69%，而邻片只有 0–9% ⇒ 烘焙器**跳过了河面**（冰面在客户端主
   通道里本来就不该被看见）。我们照主通道算 `albedo × lightmap × 2` ⇒ **黑冰面**。
3. **客户端看不到这层乘法**：`materials-fp.sl` 的 DRAW PHASE 里 `albedo × lightmap × 2` 只在
   `#if MATERIAL_LIGHTMAP && VIEW_DIFFUSE` 下发生（`#else` 支注释原文 "do not scale lightmap
   in view diffuse only case"，即 `VIEW_DIFFUSE=0` 时 `color = albedo`，不乘 2 也不乘光照图）；
   水下件只出现在 **`ReflectionRefraction` 通道**（材质文件 `Passes: ReflectionRefraction`）的
   画面里，而那片水面在客户端是**不透明**的（`water-fp.sl` 的 REAL_REFLECTION 分支
   `outColor = half4(resColor, 1.0)`），岸线带（coastLine→0）露出的正是折射画面 ⇒ 客户端
   可见的冰 = `albedo`（×flatColor），**不带光照图乘法**。

**实现（前端，纯几何判据、无调参）**：整个包围盒落在某片水面占地内（0.5 m 余量）且低于该水面片
标高（5 cm 余量）⇒ 该网格不接光照图，走不受光 albedo（`MeshBasicMaterial`，`color = albedo`
即客户端 `VIEW_DIFFUSE=0` 口径）。GLB 仍是场景系（z = 高度），判据同导出器 `glb_world_xyz` 约定。

**受灾面实测（全 36 图扫描）**：只有 2 张图有"全淹没的光照图实例"——**malinovka 26 个**
（= 25 片冰 + `ice01_26`）、**italy 1 个**；其余 34 图 0。

**已知差异（如实记录）**：① 客户端另有 `flatColor` 末乘（冰上 ≈(0.92,1.0,0.97)，≤4% 色调；
我们的包未导出该属性）；② 仅**部分**淹没的件（桥 `env_ma_bridge`、沉船 `env_ma_boat*`）仍按
主通道渲染——客户端按水面裁剪逐像素分属两个通道，属已知近似。

**验证**：WotbTools 全量 **219 文件 / 3014 通过 + 2 跳过**；`sceneryMaterials.test.js` 新增守卫
（判据/接线/不受光材质 + 禁止水下件再乘光照图）。**blast radius**：纯前端（**无数据面改动**——
包、掩码、manifest 均不变，因此**不需要 COS 同步**）。

### 岸线特征细分（前端，2026-10-10）

**报障**：上一条把水下冰面刷亮后，用户仍见"和地形的交接处还是锯齿状"。

**定案（用户两个观测，决定性）**：① "拉近之后锯齿变细"；② "旋转时形状固定" ⇒ 是**网格量化**，
不是深度抢闪（那会闪烁跳动）、也不是数据噪声（那与视距无关）。**另有一条同时暴露的成因**：
让位掩码把冰面当"贴地结构"、把地形压到冰面之下 ⇒ 岸线被整体挪走 2–4 texel（局部 5.9 m，见
[地形让位掩码](index.md) 条目的"同日修正"）——该条已修正（水面/水下件排除出贴地判定）。

**机理**：岸线在源数据里是 **2–5 cm/texel 的平缓坡**（实测逐行追踪：越过 12.250 的列号每行只
平移 0–2 texel）；客户端相机贴近地面 ⇒ 它那里的地形网格天然落到 1 texel 级；我们的回放相机常在
数百米外 ⇒ **客户端同一套判据（同一相机位置）合法地给出粗格** ⇒ 平缓坡被粗格插值量化成米级台阶。
这与让位掩码是同一类差异（"客户端看得清、我们从远处看不清"），因此也用同一类办法解决：**把水陆
交界当特征解析**。

**实现（前端；第四条细分判据，`terrainMesh.js` 的 `shores`）**：补片矩形与某片水面占地相交、且该
补片**格点高度范围跨越其标高** ⇒ 一路细分到 `minStep`（1 texel）。跨步判定与发射几何**同源**
（发射的顶点高度也取同一批 texel 值）⇒ "格点不跨步 ⇒ 发射面不跨步"，**不需要余量常数**；细分
终点 = 数据自身分辨率。标高表 = **水面片本身** + 水下薄板（冰面等）的标高与占地（`playbackScene.js`
沿用 `underWater` 那趟预计算；坐标按前端 group 旋转换算到世界系）；地形首建在 GLB 装载前 ⇒
**装载后立即重建一次**，后续 LOD 重建沿用同一表。

**真图实测（malinovka，300 m 相机；栅格化网格高度后逐行追踪岸线）**：
| | 有岸线的行 | 行内来回摆动（>1 次穿越） | 行间跳变 max |
|---|---|---|---|
| 修复前（现网口径） | 51 | **22** | 18 格 ≈ 10.5 m |
| 修复后 | 45 | **7** | 11 格 ≈ 6.4 m |
| 原始场双线性（客户端最细口径） | 45 | **7** | 10 格 ≈ 5.9 m |
⇒ 修复后与**客户端最细口径完全一致**（45/7）；三角形 17k → 70k（只在水陆带加密）。

**顺带修掉一个潜伏 bug**：根补片步长原取 `floor(n/8)`，非 2 的幂时（如 n = 96 ⇒ 12→6→3→1）
`step>>1` 二等分对不上父补片跨度（子补片 2×8·h ≠ 父 8·step）⇒ **整片未铺（空洞）**；现取不超过
它的 2 的幂并加 fail-closed 守卫（常规 n = 512 ⇒ 64，与旧逐位相同）。

**已知差异**：加密到 1 texel 后仍有的细小起伏是**数据自身**的岸线噪声（客户端同样有）——用户
"拉近之后变细但没完全平滑"即此。

**验证**：WotbTools 全量 **219 文件 / 3016 通过 + 2 跳过**；`terrainMesh.test.js` 新增岸线用例
（远相机下带判据细到 1 texel、远离标高仍粗格、混排无裂缝 ≤1e-4 m、预算不触顶）；
`sceneryMaterials.test.js` 新增接线用例。**blast radius**：纯前端（**无数据面改动**，不需要 COS 同步）。

### 回放姿态流判据：prop2 全局缺失时退化为仅 type=10（解析器，2026-10-10）

**报障**：用户某训练房回放（`20261010_1212__Anonyme_R132_T100LT_…`，7.3 s，Canal）"3D 回放不能打开"，
前端报 `playback parse failed: 无任何车辆姿态流（type=10 + prop2 双流交集为空）`。

**实测（本次新增的本地探针 + 端到端 `build_playback_data`；`examples/` 按 .gitignore 不入库）**：该场 803 包中 **type=10 位姿
195 包**，两个实体各 **100 / 95 样本**（均 ≥ `MIN_ST10_SAMPLES = 20`）；**type=7 零条**——坦克全程
未瞄炮 ⇒ 客户端不发炮塔更新 ⇒ `prop2` 流全局缺失。旧判据 `st10 ∧ prop2 ∧ 样本达标` 交集为空 ⇒
fail-closed 拒绝整场。该场 41 个无名地图对象**没有任何 type=10 流** ⇒ prop2 在本场不具"车 vs 地图
对象"的区分力，退化不引入幻影（实测）。

**修改**：`playback.rs` 抽出纯函数 `vehicle_candidates(st10, prop2)`——**prop2 全局缺失时退化为
仅 `st10 ∧ 采样数达标`**（仍保留 fail-closed：退化后仍空才报错，错误文案已注明）；下游对无 prop2 的
车辆不再 `bail`（原为"内部不变量破坏"），改按**中性口径**渲染：炮塔随车体朝向（相对 0）、炮管水平
（该车本就停着没瞄过；不给缺省会让整场打不开）。

**测试**：单元用例 `vehicle_candidates_fall_back_when_turret_stream_absent`（三态：无 prop2 退化 /
prop2 在场按交集 / 样本阈值过滤）；**回归夹具** `crates/replay-core/tests/playback_no_turret_stream.rs`
+ 该 25 KB 样本 `git add -f` 入库 `data/replay_samples/`（`.wotbreplay` 受 .gitignore 约束，须强制添加；与既有 3 个入库样本同规）（自证前提：确有 type=10、确无 type=7；断言建出 ≥1 车辆、
  炮塔/俯仰列与车体列严格同长）。`cargo test -p wotb-replay-core` 全绿（76 通过 / 3 ignored）。

**消费端**：WotbTools 按 `deploy/agent/source.json` 的 pinned commit + Release 附件加载 WASM
（`scripts/build-agent-wasm.sh` 也是按该 SHA 远程浅克隆）⇒ 本修复要生效需：上游**提交 + 发版**
（bump `workspace.metadata.release.version` + 本文档条目）→ WotbTools 更新 pin 并重跑
`fetch-agent-wasm.sh`（按 AGENTS 的 Android/黄金样例规则随 PR 处理）。

### 弹种指纹回填：method29 args[8] 弹种编码（解析器，2026-10-10）

**报障**：用户回放（`20261010_1928__Anonyme_S31_Strv_K_…`，BurningGames）"射击复现"里
敌方 60TP 打我方 Strv K 那发（t=125.33、dmg 977）**弹种未知**。

**根因（协议面缺口，非解析 bug）**：弹种唯一权威载体是 type=32 的 26/27B 段包，而
**cmpIndex=0（底盘/履带）的命中游戏一律不发段包**——本场 123 条 method8 直击中 cmp=0 的
26 条全无、cmp≥1 的 97 条全有；另 3 场样本回放复现（41/41 无、228 发中仅 1 例 res=0 未发）。
该发客户端只收到 method8（result=4/cmp=0）+ 11/12B 短广播（模块 token 提示，不含弹种）；
他人路径的既有来源全部不覆盖（0x07 广播 = 仅作者 avatar 弹药、0x1b 地形 = 仅脱靶弹）⇒
`shell_id=0` ⇒ 前端"弹种未知"徽章。本场此类"命中但弹种未知"共 24 发。

**定案：method29 args[8]（原 rawFlag，文档标记"语义封存"）= 弹种编码**——全语料（36 场含
`data/replay_samples`）**2440 发逐发携**（args 恒 37B）、**137 弹种 (弹种 → 码) 零冲突**、
同炮同弹种跨玩家/跨场一致：低 2 位 = 类别（0=AP/1=HE/2=HEAT/3=APCR，常规弹全数吻合；
"现代脱壳弹族"——T-100 LT「3VBM」/LT-432/Rhm. Pzw. 的 APFSDS 与其 HEAT——独立成组 0x17）；
高位 = 弹种家族（基数 4/8/12/20 + 类别偏移；命名未闭合，不猜）。同场 (射手, 码) 冲突 19 处
**全为同车标准/金币弹同族共码**（如 FV215b 183 he/he_premium 同 0x0d）→ 用**发射弹速**
去重（同场同炮逐弹种恒定：实测 = BlitzKit `shells[].velocity` ×0.8 ×（超充/改进型火药
×1.35），全语料 2440 发的浮点抖动 ≤2e-3 m/s）。旁证：3 场回放全部"命中但弹种未知"发次
（28 处判定）两路独立判定 **28/28 一致、0 冲突**；弹速撞车时（Obj268 三弹同 608 m/s）
编码仍可唯一区分，同码多弹时弹速反过来去重——两路互补。

**修改**：`LaunchEntry` 增 `shell_code`（args[8]）；新增
`fill_shells_from_launch_fingerprints`（shots.rs）——对 `shell_id==0` 的射击，用同场同射手
已判定发次（段包/0x07/0x1b，均服务器权威）现建"编码 → 弹种/弹速 → 弹种"映射，
**编码优先、弹速去重/兜底**，唯一才回填（fail-closed，不猜），置质量标记
`shell_from_launch_code` / `shell_from_velocity`；作者/他人两路径出口各调一次（只补弹种，
不增删发次）。他人路径日志与 `combat`/`dataset` 诊断出具回填计数。

**效果（本场实测）**：他人路径 160 发中回填 **63 发（编码 60 / 编码+弹速 0 / 弹速 3、
未唯一 0）**；含目标发次 t=125.33 → `shell_id=23434`（60TP AP，与弹速指纹独立同判），
全部 24 发"命中但弹种未知"中 23 发得解（余 1 发为该射手此前从未有已判定同码发次——
保持未知；要再进一步需消费方注入坦克炮弹表，见下）。作者路径同场不变（0x07 已覆盖）。

**测试**：单元 `shell_fingerprint_tests`（唯一/多候选+弹速去重/不可唯一/兜底/弹速键抖动
5 例）；集成回归 `crates/replay-core/tests/shell_fill_from_launch_code.rs`（样本
`20260930_2127__Anonyme_GB48_FV215b_183_…`：回填前 8 发履带命中空壳 → 回填后无"命中但
弹种未知"，定点 t≈180.89 → 32138）。`cargo test -p wotb-replay-core` 全绿；
`cargo check --workspace` 通过。

**契约与消费端**：facet 输出新增两个质量字段（附加、可缺省反序列化），WotbTools 前端
`ReplayShotsPane` 的 `q_shell_unknown` 徽章将自然消失于被回填的发次；如要给回填发次出
"指纹来源"徽章（类似 `shell_from_broadcast`）需在 WotbTools 仓加一行映射。**残留覆盖
边界**：仅"同场该射手有过同码/同弹速的已判定发次"可补；要全覆盖需把坦克炮弹表
（tanks.pb shells）注入消费层按"码类别 + 车炮弹表"定案——本次未做。

### 状态切换器的骨骼网格导出（信号灯"组件不完全"，2026-10-10）

**报障与根因**：用户"图中的信号灯组件不完全"（只有杆、没有灯头/悬臂）。逐层核实后：客户端每盏信号灯 =
`env_fc_trafic_light_02.sc2`（带 `RenderComponent` = 5.58 m 的杆，我们已**逐层忠实**导出：138 顶点/240 索引/
80 三角形 = 客户端 LOD0 datasource 77797、包围盒逐位相同、材质链 `TextureLightmap` + `traffic_light.tex` +
lightmap、逐实例 UV 变换 61/61 相异且数值一致）**＋同位置的 `env_fc_trafic_light_01.sc2`**
（无 `RenderComponent`，带 `StateSwitcherComponent`：3 状态 `State 0/1/2`、`activeState: 0`，动作为
`onDestroy → State 2`；其**子实体** `State N` 才是网格，渲染类为 **`SkinnedMesh`**）——而导出器的渲染类白名单
只有 `("Mesh", "SpeedTreeObject")` ⇒ **`SkinnedMesh` 整类被跳过** ⇒ `_01` 全图缺失（包内该名字节点数 0）。

**修复（上游 `tools/export_map_glb.py`）**：白名单纳入 `SkinnedMesh`，按**静止绑定姿态**导出（不读骨架/动作
⇒ 不随动），并**只取激活状态 `State 0`**（全图 335 个切换器里 `activeState≠0` 且含 SkinnedMesh 的有 **0 个**，
规则严格成立；损毁态 `State 1/2` 按用户决定**不导** —— 撞毁仍"直接消失"，不做倒伏动画，客户端那套
`FallingType 2` 倒伏 + `objects_falling_lamppost_creak/down` 音效 + 扬尘 + `fallingAtoms` 碎件模型均不实现）。

**验证**：forgecity `_01` 节点 **0 → 61**（每个 450 顶点，局部 z ∈ [4.27, 6.98] = 灯头 + 6.6 m 悬臂），
杆 `_02` 61 个不变；36 图全部重导（74 s）并随同重烘让位掩码、重算 manifest。

**blast radius**：36 图 `scenery.glb` 替换（**+3.2 MB**，4307.3 → **4310.5 MB**，文件数不变）、36 图掩码重烘、
manifest 重算；前端**零改动**（灯头按普通场景网格渲染，逐实例姿态/光照图/材质同在一条链上）。
**COS 同步由发布者执行。**

**验证**：WotbTools 全量 **3008 通过 / 2 跳过（218 文件）** + 门禁 14 用例 / 13 程序 / 0 失败；
`terrainMesh.test.js` **12 用例**（阈值口径 / morphFunc 与 subdivMorph 直译 / **平面地形 morph 恒等** / 曲面收缩且值域不外扩 / 终止补片满足三条判据 / 单元格边长 ≤ 0.08·视距 /
平地近细远粗且处处 texel 原值 / 陡坎细化 / 贴地薄板不被盖 / 混排层级无裂缝 / 无空洞 + 全朝上 /
契约常量与预算）、`sceneryMaterials.test.js` 里另有**防复活守卫**（不得出现 `?poff`/`?seamfix`/
`?tonemap`/`?exposure`/`?decals`/`?terrainlod` 旋钮，不得引用已撤模块）。
**blast radius**：纯前端 + 文档（数据面、资产包、COS 均未动；俯视烘焙页只渲染布景、不含地形
⇒ 其缓存无需重烘）。

### 铁轨"贴图太亮"：客户端把它当**贴花**渲染（`MATERIAL_DECAL`），我们当成了受光件

**报障（2026-10-10）**：forgecity 铁轨（`env_fs_rails_002sc2`）贴图明显比客户端亮。

**根因（客户端材质 + 着色器逐行核对）**：铁轨那几支批次的材质 `fxName = ~res:/Materials/Decal.material`，
模板顶层 `UniqueDefines: [MATERIAL_TEXTURE, MATERIAL_DECAL]`，且实例**未启用** `LightMap` 预设 ⇒
客户端走**贴花**路径（`materials-vp.sl:473-483` + `materials-fp.sl:183/447-493/539-607`）：
  ① 顶点期 `varTexCoord1 = texcoord1`（**无 uvScale/uvOffset**）；
  ② `decalTextureFetch = tex2D(decal, varTexCoord1)`，`decal` 槽 = **地图 colormap**
     （`landscape/forgecity_colormap.tex`；客户端注释原文 "objects colored with landscape"）；
  ③ `LANDSCAPE_SEPARATE_LIGHTMAP_CHANNEL`（= 地表 `separate_lm`，本图为 **true**）⇒
     `shadowColor *= decalTextureFetch.a`（colormap 的 alpha 通道；本包拆成 `ground/lm.webp` 的 R）；
  ④ DRAW PHASE `color = albedo(UV0) × shadowColor × 2.0` —— **全程无光照项 ⇒ 不受光**。
而我们：导出器既**没有贴花判据**（材质不打标），也**没随导贴花所需的 UV1**（`uv1_attr` 门槛只有
"光照图/动画掩码"两条 ⇒ 铁轨批次 `TEXCOORD_1` 被丢），前端自然落到受光 Lambert ⇒ 亮一档。
**量化**（forgecity 实测）：铁轨处 `colormap.rgb × lm × 2` = **[0.88, 0.88, 0.74]**（等效乘子
≈ **0.84**）；我们的 Lambert ≈ `sunColor×8×NdotL/π + ambient` × 0.75 ≈ **1.2–1.9** ⇒
**偏亮约 1.5–2.3 倍**。

**修复**：上游 `tools/export_map_glb.py` 新增 `decal_capable(mat_desc, family)` 判据（材质链顶层
`MATERIAL_DECAL` 或启用同名预设；与光照图并存时保守走光照图并计数 `decal_with_lightmap`），
命中则：材质写 `extras.decal = true`（`materialLightmapAdjustment` 非默认时另写
`extras.decalLmAdjust`）、**扩展 UV1 随导门槛**（无 UV1 时 fail-closed 退回受光并计数
`decal_dropped_no_uv1`）、`mat_key` 纳入贴花标记、stats 记 `decal_batches`。WotbTools 新增
`makeDecalMaterial`（`albedo(UV0) × colormap(UV1).rgb [× lm 通道] × 2.0`，不受光；含
`GLOBAL_TINT` 的 brightness/contrast/gamma 调整分支与对数深度块）+ 场景派发（先于光照图与受光
兜底；要求 `extras.decal` + 几何 UV1 + 地表分层贴图在位）+ 着色器门禁用例 2 条 + 源码守卫。
**方向性核验（数据）**：铁轨顶点 UV1 与地面着色器的世界→UV 公式**同空间**（样例 (0.4219, 0.2775)
vs 公式 (0.422, 0.2767)）⇒ 直接用 UV1 采 colormap，**无需翻转**。

**验证**：上游单测新增 `test_decal_capability_criterion`（6 通过）；WotbTools 守卫新增贴花用例
（UV1 原样 / colormap 采样 / 无光照项 / ×2.0 / 调整分支 / 接线次序）；门禁 **14 用例 / 13 程序 /
0 编译失败**（含 `decal` 与 `decal+alphaTest` 两个新程序）。
**blast radius**：forgecity `scenery.glb` 重导（-`--scenery-only`）：**3 支材质**打标
（`env_fs_rails_00[1-5]` + `env_fs_border_*`）、随导 `TEXCOORD_1`；包内替换 1 件
（29,905,300 → **30,051,508** 字节，+146 KB）⇒ manifest 按包内容重算（**4964 件 / 4288.4 MB**）。
其余 54 图未重导（其贴花批次待各自 `--scenery-only` 时随导；判据已就位，未命中时不改变输出）。
**COS 未同步**（本地包即测试面）。

## 约定

- 逆向结论的**唯一权威**是 [docs/回放与射击逆向总集.md](回放与射击逆向总集.md)；其余文档与其冲突时，
  先查 [wotbtools-cross-reference.md](wotbtools-cross-reference.md) 是否已有裁决。
- 历史推导过程不在工作区文件中保留，需要时查 git 历史。
- `examples/`（逆向探针脚本）与 `tmp_*/`（分析转储）不入库，文档中提及处仅作证据出处记录。
