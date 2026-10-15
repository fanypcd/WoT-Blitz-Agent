# WotbTools 交叉引用与裁决记录

> 对 [A158Coke/WotbTools](https://github.com/A158Coke/WotbTools) `docs/research/replay/`
> 与本项目的逐条比对结论：**采纳 / 驳回 / 本地验证裁决**，附依据。后续接手者据此防止
> 误采或回退已定案。本地验证工具：`src/bin/verify_p1.rs`（`cargo run --release --bin verify_p1`，
> 样本 `data/replay_samples/`）。
>
> 跨仓协作现状（本仓为上游解析方；WASM 四入口 + 静态资产面；对方契约
> `contracts/agent/replay-facets-v2.md`、版本锁定 `deploy/agent/source.json`）见 §五。

## 一、本地验证裁决（P1 分歧组）

### B1. method27 (0x1b) args[21..33) —— **WotbTools 对，我方旧定名证伪** ✅已改码

该字段 = **弹道末段速度方向向量**。判据（5 场 92 包 / 79 配对）：方向说
`cos(seg, 发射速度) > 0.99` **79/79**（中位 1.000000）；位置说 `|seg − launch| < 0.5m`
**0/79**；norm 散布 0.229..248.7（方向向量量级，非位置）。旧"segmentStartPoint/弹跳点"
定名废弃。已落地：`TerrainImpactData.segment_start` → `terminal_dir`；viewer 弹跳线改为
出射方向线 + 出射偏角诊断（>2° 提示跳弹/减速）。

### B2. method27 (0x1b) args[4..8) —— **WotbTools 警告部分成立；掩码域已重裁** ✅已改码

92 包：高 16 位（byte2）非零 40/92，低 8 位 92/92 符合国家基数格式；去重
full=low24=low16=35 种，同 low16 多 byte2 冲突 = 0。**裁**：byte3（bits24-31）为噪声；
但 bits16-23 承载局部 id 高位（IS-7 AP=0x8250a、T57=0x8532a、T110E5 0x834）——旧
`& 0xFFFF` 掩码把这类 id 截断成 0x250a 必然查不中（B2 样本恰为局部 id ≤0xFF 的车，
掩盖了截断）。已落地：`& 0xFFFFFF` 掩码（`collect_terrain_impacts`）。

### B4. method35 (0x23) float1 —— **WotbTools 对，我方"倒计时"证伪**

float1 = **当前生效完整装填配置时长**（肾上腺素/弹药架/装填手状态联动），非倒计时。
判据：5 场 8 实体 43 事件（稀疏）；值在配置间跳变、不复原递减（样例 Type5H：
`12.494 → 10.679 ≈ ×0.855 → 复原 12.494`，12.494 = 该车满装填配置）。旧 §3.8"倒计时递减
确证"作废；本项目无代码消费，文档已修正（总集 §3.8）。

### B5'. avatar prop9（A2 复核）—— **WotbTools 对**

prop9 = **炮塔相对偏航镜像**（非俯仰、非角度制）。判据：5/5 场作者车 prop9(raw as rad)
vs 本车 prop2 coarse10 偏航：median |Δ| = 0.0015~0.0016 rad（0.09°）、值域恰 ±π。
A2 修复（俯仰回退改 method36 field2）成立。

### B6. 同一 shotId 多 interaction —— **Shot 计数按开火，不按装甲交互** ✅已改码

`unique shotId = 一次开火 / 一个 Shot`；同一 Shot 内可出现多个装甲交互。
`collect_launches` 按 shotId 去重，首条 method29 是 Shot 级 primary launch，后续同 shotId
method29 **不得增加射击数**。作者严格路径不能只按 `victim + time` 要求所有 type=32
segment 相同——优先用已验证的 `method8.hash6 ↔ type32.hash6` 同事件令牌关联，token
证据不足时保持 fail-fast。**PARTIAL / 禁猜**：该样本同 shotId 在命中附近出现第二条、
位置与速度均变化的 method29，强烈符合跳弹/续飞段，但单样本不足以把"后续 method29"
公开定名为 trajectory continuation——当前只保留第一条作为 primary launch，
不新增公开多段弹道 DTO。

### S1. 争霸空闲行判据 —— **本仓自有判据；对方只规定结果、未闭合 wire** ✅已改码

对方权威研究契约 `supremacy-base-state.md` 把「已归属基地被对方占领后中断」列为必须覆盖
的回归场景，并把 owner/capture lifecycle 与 "explicit clearing/invalidation semantics"
整体划归后端，但未给出「哪条 wire 行代表中断」。本仓据此采用自有判据 = **空闲整播**：
f3/f4/f5/f6 全缺省的行 = 该基地整条状态广播（零值被 proto3 省略），owner 按本行保留、
占领状态归零，中立与已归属统一处理。规则只读字段在场/缺省，不含统计阈值。**消费方同步
以对方实现为准**；本仓 `arena.rs` 重建器注释与契约 §1.1 已标注该口径偏离参考实现。

## 二、直接采纳（WotbTools PROVEN，按用户指示免验证）

| 项 | 结论 | 落地 |
|---|---|---|
| type31 | 录制者 arcade gun-marker/瞄准圈尺寸（连续值 6.75..54；`replayCtrl.setArcadeGunMarkerSize` 代码证据）。旧"FOV 三档·非瞄准圈"并档：三离散值=静止瞄准时刻子集 | 文档 |
| type=32 24/25B 族 | **消耗品生命周期**（wireCode+state 1/2/3/255；0x09 肾上腺素/0x0B MPRP/0x0C 急救包/0x0D 修理箱/0x69 钨芯弹等码表 PROVEN）。旧"模块损伤百分比流"降级 | 文档 |
| 穿透谓词 | `hit_flags & 0x1110`（+0x0100 内部模块穿；对结算 269/270，r≈0.9924）；他人路径 +`(result=2, cmp=0)` 模块-only 穿 case | **代码**（combat.rs 两处） |
| type26 | **来袭炮弹警告**（PROVEN），非"弹道清理"；开火后 0.2~0.6s 相关实为弹丸飞行时窗 | 文档 |
| battle_results tag125 | WotbTools #301 字段集无 125；开局血量改用公式 `max(field1,0)+field11` 交叉验证（我方 125 记录标注"待复核"） | 文档 |
| method38 位表 | 补名：0x0002 攻击时目标已死、0x0004 起火、0x0200 模块未被弹丸穿、0x2000/0x4000/0x8000 爆炸分支；headerHi16 保留 raw（不恒 0x0002，Maus 边界 0x0012/0x0028）；同钟批量需去重 | 文档（位常量已在码注） |
| 组件表 | +37 炮塔旋转机、39 车长/40 驾驶/41 炮手/**42 禁猜**/43 装填手；rawState：1=受损或乘员受伤、2=critical/禁用族（"摧毁"过强）、0=命中但无新负面（"无变化"过强） | 文档 |
| 死亡语义 | 哨兵族 {0,−1,−2,−3}；溺水 cause=5 正 HP 死亡（`dead ⇔ hp≤0` 被受控实验否定）；method1 source 按 cause 分域（0/1/2=对方、3/5=自身）；正确死亡面 = 哨兵族 ∨ prop1=00 ∨ wrapper6 ∨ 结算 field105 | **代码**（hp_terminal_normalized） |
| method36 全字段 | field2=车体系炮管俯仰（vs type39 f6 中位 0.0018 rad）、field3/4=水平/垂直角速度上限（火炮受损 ×0.675）、field5=瞄准时间（Reticle Cal ×0.70）、field6.f1=bloom（开火必正跳 326/326、火炮受损 ×2） | **代码**（AimSnapshot） |
| avatar prop9 | 炮塔相对偏航镜像（r=0.999993）——旧"瞄准角/俯仰回退"废弃 | **代码**（A2） |
| AoI 生命周期 | Type33→Type5→观测→Type4→硬黑屏（485/485 零更新）；Type4 ≠ 死亡；隐藏段禁止插值；OBSERVED/LAST_KNOWN/DEAD 三态 | 代码+文档（`collect_aoi_lifecycle`） |
| type39 | 28B=**7×f32**：f0/f1=世界系炮线 yaw/pitch（开火锚定 0.27°/0.45°）、f2-4=瞄准射线点、f5=相对偏航族（门控）、f6=车体系俯仰（0.17°）。撤销"排除于战斗分析" | 文档 |
| battle_results 字段 | 105=deathReason(−1 存活/1 火/2 撞/3 世界)、24=lifeTime、16=spotted、119=毁灭协助、120=炮印、117=挡伤、9/10=助攻两族、1=终局HP(−2=自动击毁✓与 parser.rs 现行为互证) | 文档 |
| Type5 尾部 loadout | 3 消耗品+3 给养（位置序 1037/1037）+9B 配件（字节=ID）；**敌方再物化亦带**（683/683）。解析已泛化：`0A 06`+6×14B 标准（观察者族 4 条拒收）与 XM551 受控场实测的 `0A 07` 变体（KK≥6 严丝合缝采纳）；扫描 `0B <n>`+nB（ID 域 100..=123 校验）。权威目录 `103=校准弹 CALIBRATED_SHELLS`、`110=强化装甲 ENHANCED_ARMOR`。`ShotReplayData.shooter/target_equipment` 注入双方搭载，viewer 自动穿深 ×1.06/1.07 与厚度 ×1.04（数据在场时禁用手动勾选框，缺失回退） | 代码+文档 |
| wrapper6 = 击杀播报 | victim/killer/deathReason + field3=>50% 伤害助攻者（46/46） | 代码（`collect_kill_feed`） |
| 其他 | prop4=engineMode TUPLE<u8,2>（&3 移动位）；type35=会话十分秒计数低字节（非"服务器 tick"；type36=u32/10.0 时间基锚）；avatar 0x0c=method12 累计伤害反馈（eventCode/count/value）；0x11=method17 弹药余弹递减；type10 [24..36]=滤波误差（禁当速度）、[8..12] parent≠0 时位置非世界坐标；1 位置单位=1 米；容器头 magic/totalLengthMinus8/variableHeaderLength 三字段互验；payloadLen==0 合法 | 文档 |

## 三、明确驳回（WotbTools 错误 / 其自标 GUESS 禁采）

1. **prop2 整 u16 偏航公式**（`raw*360/65536−180`）——其受控样本炮管贴极限（frac 恒定），
   分不出两模型；我方 coarse10|frac6 有 T110 极限钳位 + 弹道锚回归 0.997 支撑，保留。
   等价边界：frac 恒定时两模型同步。
2. **method8 result 0..4 任何符号命名**（含我方旧"4=跳弹"，已由 1436 交叉表证伪）——保留
   raw；行为数据：sub=4 中 231/310 掉血、(2,0) 属穿透族。
3. 我方被 B1/B4 证伪的旧条目（segmentStartPoint、0x23 倒计时）——见 §一，不再采信任何一方旧文本。
4. 一切 WotbTools 自标 UNKNOWN/GUESS/HYPOTHESIS 的：cause=4、method16 codeA 0/1/6/7、
   组件 42、prop7 元素命名（0x04=火 NOT PROVEN）、prop8 元素=method16 codeB 通用解码、
   bloom→UI 换算公式、field116 语义、field118（"占基地"被否）、"低 HP 保证精准火力"、
   历史 PC 位序移植（0x1000=旧火炮损伤等 REJECTED 项）、"Blitz 合并了某历史损伤位"（HYPOTHESIS）。
   （原列其中的 **method29 byte8 / 尾 f32** 已由我方定案：byte8 = 弹种编码——36 场 2440 发
   逐发携、137 弹种 (弹种→码) 零冲突；尾 f32 = 恒 6.2784 占位常量。见总集 §3.1；解析器已
   按其做弹种指纹回填，质量标记 `shell_from_launch_code` / `shell_from_velocity`。）
5. Version 门控：WotbTools 全部结论限定 11.19.0 China；按
   `(clientVersion, entityClass, methodId)` 三元组使用，跨版本数字 ID 可能漂移。

## 四、双方一致互证（无需改动）

type10 49B 布局/10Hz/米制；prop1=死亡边界；prop4 结构；prop3 0xFFFD 哨兵（扩展为四值族）；
type28=弹药槽；type32 hash6==method8 args[10..16]（对方 2,359/2,359 独立复证我方 86/86）；
method29/20 布局（对方补充：method20 31/34 场计数精确闭合、terminal-only shotId 勿伪造
发射源）；battle_results 25=killerID、102=team、无模块配置字段；Type13=流内结算；
容器无 XOR/无压缩；method36 PRE→发射→POST 三明治。

## 五、跨仓协作现状

WotbTools 的 canonical 流水线（Java `wotb-core` + 前端）需要 Agent 侧**只透出证据、
不下判断**，切面字段一律 additive（`AiReviewFacet` v1 / `PlaybackData` v2 版本不变）。
消费机制与版本记录/未完项见 [index.md](index.md)（版本记录表汇总 v0.3.4 起的每一版增量）
与 [AGENTS.md](../AGENTS.md)（pin bump、golden 重生成、Android `wotbVersion` 同步规则）。

本仓已落地、当前有效的能力面（详表见 [总集 §八](回放与射击逆向总集.md#八提取链消费地图字段级回退与质量标记)）：

- **结算字段**（`wargaming/battle_results_extra.rs`）：#301 两层嵌套（`{result_id@1, info@2}`，
  字段在 info 内：1=终局血量/16=点亮/24=寿命/25=击杀者/105=死因/119=毁灭协助/120=炮印）。
  `PlayerSummary` 含 death_reason/survived/life_time_secs/killer_id/n_enemies_spotted/
  destruction_assistance/gun_marks；CLI `single` 有 Settlement 块。未知字段诚实输出 "?"（unknown≠0）。
- **wrapper6 击杀播报**（`collect_kill_feed`）：subtype6 载荷 = root field6 剥壳，内层
  1=victim/2=killer/3=>50%助攻/4=死因；playback `kills` 增强（击杀者兜底归属 + assister_eid +
  death_reason，|t−death_t|≤5s 门控隔离开局初始化记录）。
- **AoI 生命周期**（`collect_aoi_lifecycle`）：Type33→Type5 开段/Type4 关段；Type4≠死亡。
  渲染侧插值防护由 playback coverage（采样间隙>2s 断开）承担、死亡面 = prop1——
  本收集器提供协议精确边界（0.094~2s 短隐藏段 coverage 不断）。
- **type39 作者炮线消费**（`collect_type39_frames`，7×f32 全字段）：消费 = `ShooterAimData.
  world_gun_yaw/pitch`（开火时刻 |dt|≤0.05s 锚定——326/326 三明治保证帧存在）。
- **method38 位常量结构化**（`combat::hit_flags_mod` 全 16 位命名 + PENETRATION_FAMILY 谓词）；
  同钟同受击者合并（0.05s 窗）；rawState 语义注释已按 §二 定案。
- **ReplayDataset**（`models/replay_dataset.rs` + CLI `dataset <file>`）：metadata/settlement/
  diagnostics（packet_types 直方图 + unsupported 盘点 + degradation 聚合）。
- **装填相位与有效时长**（v0.3.9）：见 §六。
- **车辆实际搭载配置与弹容**（`config_idx`/`burst_size`）：见 §六弹容裁决。
- **AoI 重入段化滤波**：位置滤波器按 AoI 在场段独立实例化（客户端 `onEnterAoI →
  setFilterOnEntity()` 同构）；`visibility[]` additive `pose`（Type5 物化快照位姿 = 段首种子）。

**剩余待办（本仓）**：

- ReplayDataset 阶段 2（observations/simulation 拆层，面向 Java 消费）
- method16/17/12 消费（模块/乘员时间线、弹药余弹、实时计数器——可选，视 UI 需求）
- AoI 协议边界收紧 coverage（0.094~2s 短隐藏段，前端消费）
- **对方点名要的字段**：`PlaybackData.damages[]`（数据已在 `timeline.hp_events`，加字段、
  facet 版本不变）、`coverage`（packet 计数与 `decodedPacketRatio`）、`finish_reason`、
  `unsupported_damage`（只有双方无数值），并实测确认 `Shot.game_hit_result` 与对方 Java
  `primaryResultRaw` 同义。其中"未钳零原始 HP"与"Visibility 当前 HP"已由 `hp_raw` 覆盖。

## §六 装填数据裁决

**协议面（本仓）**：装填的唯一数据源是 updateArena（m0x30）**subtype 15/16/17**，
条目 `{f1=eid, f2=相位码, f3=f32 秒, f4=计数}`，**仅本方全队**广播、由**相位转移**驱动推送
（非固定采样）。相位码与计数语义：

| 相位码 f2 | 语义 | 时长/计数 |
|---|---|---|
| 1 | 剩余弹数更新 | 无时长；与同车开火同刻（16/16 对齐） |
| 3 | 整夹重装开始 | f3 = 整夹时长；消费侧用 `reload_effective`（0x23）校准刻度 |
| 4 | 装填中途时长变更 | f3 = **新的完整有效时长**，不是倒计时 |
| 5 | 就绪 / 取消 | **f4=1 是就绪标志，不是剩余弹数** |
| 6 | 弹鼓逐发补槽 | f3 = 该发时长 |
| 7 | 夹内推弹上膛 | f3 = 上膛间隔；**不补弹、不增加已装发数** |
| 8 | **语义未定（禁猜）** | 无 f3/f4；样本 11 条全部来自 tank 21793「Sheridan Missile」，11/11 紧随该车 f2=3 前 0.5~3.2 s；渲染侧不解释 |

样本实测分布（9 场 527 条）：`{1:16, 3:251, 4:74, 5:77, 6:47, 7:62, 8:11}`（相位码 **2 未出现**）。

**除 f2=5 外，f4 = 该事件时刻的服务器剩余弹数快照**——与客户端 item_defs `<clip><count>`、
BlitzKit `burst_size` 三方一致（63 车互验）。**subtype 16 = 引擎 `ReloadTimeUpdate`**：载荷
`{field15:{eid, f2=1, f3=1|8}}`，与装填完成/开火**零相关**（110/140 条），原样透传、不消费
（"服务器下发每发装好通知"的旧猜测已证伪；也没有完成时刻的剩余数快照：f2=3 完成 0/345 条）。

**显示面（客户端行为，我们复刻的口径）**：满夹 `A|A|A`；开火 → 夹内推弹期为 `A|B|C`
（B = 正在推弹的那一格，**期间不补弹**）→ 完成 `A|A|C`；弹鼓另有 f2=6 补槽 `A|A|B` → 满
（期间再开火则取消）；空夹 → f2=3 整夹重装期间为**一整条不分割**（B）→ 完成 `A|A|A`；
**开火会取消进行中的装填**。客户端服务器侧只在转移点说话，中间进度是**本地外推**
（`reloadingShellTime0..5` × `gunReloadTimeFactor`/`reloadEqFactor`/`reloadBoost*`），
回放拖动时由 `ReloadScreenForRewind` 重建状态——与我们"服务器 f4 快照重锚 + 本地外推"同构。

**落地口径**：本仓切面 `reloads`（相位原样透传）+ `reload_effective`（0x23）；渲染在
WotbTools `frontend/src/scene/reloadBar.js`（含整夹/弹鼓/取消/f4 漂移纠正/方法 35 作用域回归），
本仓同构副本 `frontend/src/scene/reloadBar.js`。协议事实另记入
[回放与射击逆向总集.md](回放与射击逆向总集.md) 第一篇 §3.8、§3.10。

### 弹容 N 裁决

**弹容 N（装填条分格数）的唯一权威取值 = `vehicles[].burst_size`**——解析面按回放 comp
blob（ARENA_INFO subtype 1）证据链解析出的**实际搭载主炮配置**，直接给出该配置在坦克数据
里的弹夹容量（`configs[config_idx].burst_size` 原值，0 = 单发）。消费端无需再联表坦克数据、
无需等异步取数，也不存在取数失败时的降级歧义。

- **禁用旧推断**：不再用「消息流剩余弹数最大值 + 1」（f4 快照推断，`inferMagazineSize`）求 N——
  f4 快照继续用于**在膛发数**重锚（语义不变），只是不再参与 N。
- **禁用跨配置取最大**：`configs[].burst_size` 跨配置取最大在多炮坦克上取到的是未搭载炮的弹容
  （实测资产包 735 台中 52 台跨配置不一致；混合形态含"单发炮 + 弹夹炮"与"两门弹夹炮容量不同"
  ——T69 4/3、T54E1 4/3、ATAC 6/12、AC Wedge 0/6、Medium I 0/15 等）。
- **回退**：`burst_size` 缺失（坦克数据缺失或配置证据链未命中）→ 单发（1），不猜；
  configs 唯一时无歧义，直接给该唯一配置的原值（config_idx 仍不解析，与既有语义一致）。
- **客户端（WASM）路径**：浏览器解析无坦克数据注入，`vehicles[]` 另行透传 comp blob 的
  模块局部 id（`turret_local`/`gun_local`，纯回放证据）；消费方用资产面 `tank/{id}.json`
  联表 `configs[]` 按**同一三级证据链**（comp locals → 发射弹种 `shell_ids` → 初始血量
  `max_hp`）自行钉定实际搭载配置（WotbTools `reloadBar.resolveMountedConfig`，与
  `resolve_config_index` 同语义并有测试锁定）。
- **时长不受影响**：装填时长/进度基准仍全部来自相位 f3 + `reload_effective`（回放广播真值，
  一场只装一门炮，天然按实际炮正确），本裁决只改 N 的来源。
