# WoTB Agent 开发进度总结

## 项目概述

**项目名称**: World of Tanks Blitz 游戏助手 Agent

**课程**: 程序设计训练（Rust）— 清华大学 2026 夏季学期

**目标**: 为 WoTB 玩家提供基于回放分析和 API 数据的个性化战术复盘、战绩诊断和提升建议

**服务器**: Asia (ASIA)
**测试玩家**: Anonyme (account_id: 2033684170)

---

## 一、已完成功能

### 1.1 回放解析

- **单场回放解析**（`single`）：解析 `.wotbreplay` ZIP 包，提取 `meta.json` 和 `battle_results.dat`，输出 14 名玩家完整战绩（伤害/命中/穿透/格挡/击杀/评级变化）
- **战斗事件解码**（`combat`）：自主逆向解码 `data.wotreplay` 数据包流中 `Unknown(type=7)` 包的 6 种事件子类型（生命值变化 sub=3、死亡 sub=1、累计伤害 sub=10 等），重建战斗时间线
- **射击事件推断**：关联作者伤害计数器递增和敌方生命值下降，推断每发射击的目标/伤害/击杀（已验证精确匹配）
- **批量扫描**（`scan`）：扫描回放目录，生成聚合报告（胜率/伤害/评级/坦克统计/地图统计），支持按模式和时间筛选
- **回放 vs API 对比**（`compare`）：扫描本地回放获取近期数据，与 WG API 累计数据对比，量化近期表现 vs 历史平均差异
- **坦克数据源切换**：坦克数据不再从 WG API 获取，改为从 BlitzKit pb 数据构建（`TankResolver::from_blitzkit`，723 辆）；WG API 仅用于战绩查询

### 1.2 WG API 集成（仅战绩查询）

- 25 个 API 端点封装（玩家统计/坦克百科/军团/赛事/单车统计/成就）；坦克百科端点已停用（坦克数据全部来自 BlitzKit）
- API 数据快照系统（`snapshot`）：定期采集累计数据，通过快照差值实现阶段性分析
- 玩家查询（`player`）：随机战+排位战完整统计

### 1.3 BlitzKit 数据集成

坦克数据全部来自 BlitzKit（WG API 仅用于战绩查询）。自实现 Rust Protobuf 解析器（`blitzkit.rs`）：

| 数据文件 | 内容 | 输出文件 |
|----------|------|----------|
| `tanks.pb` | 723 辆坦克 tier/类型/国家/血量/名称，炮塔/主炮/弹种/装填（弹夹/弹鼓） | **坦克数据源，运行时直接解析**（`fetch-blitzkit` 命令） |
| `models.pb` | 炮塔/主炮→模型节点（gun/turret_0X）权威映射，多炮塔/多配置正确切换 | **模型数据源，运行时直接解析**（`fetch-blitzkit` 命令） |
| `models.pb` | 723 辆坦克 hull+turret+gun 三段装甲板厚度 + 炮管俯仰角 | `armor_cache.json` + `gun_angles.json` |
| `icons/big.webp` | 723 辆坦克封面图 | `tank_images/`（`fetch-icons` 批量下载 / `/api/tank_image/` 按需缓存） |

- **伤害字段区分**：pb 的 shell 消息含两个伤害字段——field 4 = 血量伤害（HP/alpha，如 E-100 12.8cm AP 460）、field 5 = 模块伤害（履带/炮管，180）。曾误用 field 5 当作血量伤害（显示 180），已重新解析修正（585 辆精确覆盖，138 辆低级多炮塔车保持原值）
- `TankResolver::from_blitzkit`：从本地 BlitzKit 数据构建完整坦克解析器（含装甲摘要 front/side/rear、弹种、俯仰角），无需 WG API、无需联网
- 验证：IS-6 AP pen=175 ✓、E-100 12.8cm AP dmg=460 ✓、IS-7 dmg=460 ✓

### 1.4 DVPL 游戏文件解码

- 参考 [Tankerch/DVPL_Converter](https://github.com/Tankerch/DVPL_Converter) 格式规范，自实现纯 Rust LZ4_HC 解压
- 解析游戏本地 XML DVPL 文件提取装甲板厚度：
  - hull 装甲板（`<armor_N>VALUE</armor_N>`）
  - turret 装甲板（`<turrets0>` → `<Turret_N_...>` → `<armor>` 块）
  - gun 装甲板（`<guns>` → `<armor>` 块，含 `<gun>N</gun>` 炮管装甲值）
  - chassis 履带装甲（`<leftTrack>` / `<rightTrack>`）
  - spaced 附加装甲识别（`<vehicleDamageFactor>0.0</vehicleDamageFactor>` 标记的装甲板归类为外部装甲）
- 解析游戏本地 YAML DVPL 文件提取碰撞包围盒和 `points` 偏移数据（用于 3D 查看器中炮塔/炮管定位）
- 弹种类型适配：WG API 返回大写（`AP`/`APCR`/`HE`），代码统一 `.toLowerCase()` 处理
- dev_name 自动映射：从 `tanks.pb`（唯一数据源）的 dev_name 字段构建 tank_id → 游戏文件名映射，覆盖全部 723 辆坦克

### 1.5 统一击穿判定逻辑（Rust 模块）

**`src/wargaming/penetration.rs`** — 集中实现全部击穿判定机制，前端点击时通过 `POST /api/penetrate` 调用：

**装甲分类**：
| 类别 | section | 行为 |
|------|---------|------|
| 主装甲盒 | `hull`, `turret` | 角度等效装甲，可跳弹可转正 |
| 附加装甲 | `spaced` | flat 抵消，无角度/转正/跳弹（`vehicleDamageFactor=0` 标记） |
| 炮盾装甲 | `gun` | flat 抵消，无角度/转正/跳弹 |
| 履带 | `chassis` | flat 抵消（`<leftTrack>`/`<rightTrack>` 厚度） |
| 炮管/炮根 | `gunBarrel` | flat 抵消（`<gun>` 标签厚度，含 `gun_01` + `gun_01_mask` mesh） |

**判定机制**：
1. **主装甲盒概念**：炮弹必须穿透 hull 或 turret 装甲才算 `PENETRATION`；只命中外部装甲 → `BLOCKED`
2. **跳弹**：原始入射角 > 70°（AP/APCR），仅当炮弹未穿过外部装甲时检查
3. **转正**：AP -5°、APCR -2°，仅当炮弹未穿过外部装甲且未跳弹时施加
4. **Overmatch**：口径 > 3 倍厚度 → 强制不跳弹；口径 > 2 倍厚度 → 增强转正（`1.4 * norm * caliber / (2 * thickness)`）
5. **外部装甲**：flat 抵消穿透力，无角度/转正/跳弹；穿过外部装甲后主装甲不再跳弹不转正
6. **多层穿透**：逐层消耗 `remaining_pen -= effective`，穿透主装甲后停止（不计算穿出）
7. **HE 弹**：单层判定，无转正无跳弹

**API 端点**：

| 端点 | 方法 | 功能 |
|------|------|------|
| `GET /` | GET | HTML 页面（Three.js 前端） |
| `GET /api/tank/{tank_id}` | GET | 指定坦克数据（元数据、装甲、configs 配置列表、弹种、俯仰角、模型 URL） |
| `GET /api/tank_list` | GET | 723 辆坦克 ID→名称映射 |
| `GET /api/tank_filter` | GET | 坦克富元数据（id/name/tier/nation/type，供筛选器） |
| `GET /api/tank_image/{id}` | GET | 坦克封面图（本地缓存，缺则从 BlitzKit 下载） |
| `GET /api/shells/{tank_id}` | GET | 指定坦克弹种数据（穿深、HP 伤害、模块伤害、类型、口径） |
| `GET /glb/{tank_id}/{file}` | GET | 3D 模型代理（本地缓存，缺则从 BlitzKit CDN 下载） |
| `POST /api/penetrate` | POST | 统一击穿判定（raycast 命中列表 + 弹种参数，返回结果/层数/角度/伤害） |

### 1.6 3D 装甲查看器

**双模型绑定**：
- `model.glb`（可见）：带贴图的视觉模型，含履带（`chassis_track_L/R`）、负重轮、炮管（`gun_0X`）、炮根（`gun_0X_mask`）等模块 mesh
- `collision.glb`（隐藏）：每个 mesh = 一块装甲板，node 名含 plate ID（如 `hull_armor_3`），用于 raycast 和装甲计算
- raycast 目标为 `[armorModel, ...moduleMeshes]`，覆盖两个模型

**坦克与配置切换**：
- **Shooter/Target 独立选择**：射击坦克决定口径/弹药，受击坦克显示模型/装甲
- **图形化筛选器**：弹窗网格 + 封面图卡片 + 等级/国家/类型筛选 + 名称搜索（`/api/tank_filter` 富元数据）
- **Config 选择器**：多炮塔/多主炮坦克（290 辆，如 E-100 的 12.8cm/15cm 双炮）可切换配置，同步更新火炮模型（`gun_0X` 组显隐）、装甲判定（`gun_0X_armor_*` 分组显隐）、弹种与穿深
- 3D 模型按需缓存：`/glb/{id}/` 代理首次下载落盘到 `glb_cache/`

**模块装甲集成**：
- 从 `model.glb` 提取 `chassis_track_L/R`（履带）、`gun_0X`（炮管）、`gun_0X_mask`（炮根）子 mesh（前缀匹配支持多配置）
- 厚度来自游戏 XML DVPL（`<leftTrack>`/`<rightTrack>`/`<gun>` 标签）
- 同一 section+plateId 的多次命中去重（保留最近一次）

**炮塔旋转和炮管俯仰**：
- 矩阵旋转法：`translate(pivot) * rotate(angle) * translate(-pivot) * origMatrix`
- **枢轴从视觉模型几何推导**（`computePivots`：炮塔取组包围盒中心、炮管取离炮塔最近端即炮根），缺 `turret_points` 的坦克也能正确旋转不漂移；points 数据作为回退
- **炮根 mask 跟随**：`gun_0X` 前缀分组（炮管+炮根+变体），俯仰时整组同步旋转
- **装甲模块对齐**（`alignArmorModules`）：碰撞模型炮塔/炮管 armor 缺定位数据时（如 E-100），按视觉模型对应组的包围盒中心平移对齐
- 右键拖拽操作，visual 和 armor 模型同步旋转

**弹道可视化**：
- TubeGeometry 轨迹线（绿=穿透/红=未穿/橙=跳弹），`depthTest: false` + `renderOrder: 999`
- 每层接触点小球标记
- HTML 信息窗口（固定大小，通过 3D→屏幕投影定位，每帧跟随摄像机更新；下移偏置避免遮挡模型）
- 信息窗口显示伤害区分：穿透 → `HP Dmg 460`；仅命中履带/炮管 → `Module Dmg 180 (HP 460)`
- 拖曳检测：鼠标位移 >5px 判定为拖曳，不触发点击；纯点击空白处才清除轨迹

**Collision 模式**：
- Show Collision 按钮：按基础厚度着色显示装甲板（绿<30mm → 黄<60 → 橙<100 → 红<200 → 暗红≥200）

**实时穿透渲染（GPU shader，已弃用）**：
- 早期屏幕空间 RT 双 pass 方案因投影误差被放弃；后采用逐顶点路径预计算 + 片元着色
- 因 GPU 着色与 Rust `penetration.rs` 判定易失配、投影厚度≠射线路径厚度、法线来源不一致等缺陷，该功能已**完全移除**
- 穿透判定统一由 Rust `penetration.rs` 计算，前端仅保留「点击精判」与「弹道可视化」

### 1.7 LLM Agent

- Agent Loop 重构为 **async**（`Agent::chat_async`），通过 `AgentEvent` 枚举（StepStart/ToolCall/ToolResult/Text/Interrupted/Done/Error）流式上报事件；CLI 通过阻塞包装复用并镜像事件到 stderr
- 6 个 Agent 工具：search_player、get_player_stats、scan_replays、parse_replay、compare_replay_vs_api、**view_tank**（打开 3D 装甲查看器：解析坦克名称/ID → 后台线程启动查看器并自动开浏览器）
- Token 统计：精确记录每次 API 调用的输入/输出 Token 和费用，支持预算上限自动中断
- 会话保存/加载：完整上下文（含系统提示、对话、工具调用、Token 统计）存为 JSON

### 1.8 Web 图形界面（`wotb-agent web`）

独立 Web 应用（axum，`src/web/`），把 CLI 功能全部图形化：

- **Agent 对话页**：聊天气泡 + 实时进度（LLM 步骤 N/M）+ 工具调用轨迹（名称/参数/结果）+ 打断按钮；前端轮询 `/api/chat/events` 增量渲染 AgentEvent 事件流
- **功能面板**：玩家战绩查询、回放扫描聚合报告、回放 vs API 对比、对局前瞻、模型配置编辑（写回 config.toml）、Token 用量统计（明细 + 预算进度条）
- **架构**：会话按 `session_id` 区分，每会话独立 Agent 实例（tokio Mutex 跨 await 持锁保证串行）；业务逻辑与 CLI 共用服务层函数，阻塞调用经 `spawn_blocking` 包装

---

## 二、数据源

### 2.1 Wargaming Public API — ✅ 可用

- Base URL: `https://api.wotblitz.asia/wotb/`
- 25 个端点：Accounts(4) + Tankopedia(8) + Clans(4) + Tanks(2) + Tournaments(7)
- 限制：不支持按时间筛选，无认证接口，无排位排行榜

### 2.2 wotbreplay-parser（Rust crate）— ✅ 可用

- `wotbreplay-parser` v0.4.2 (MIT, 作者 eigenein)
- 解析 `.wotbreplay` ZIP 包结构（meta.json + battle_results.dat + data.wotreplay）
- 仅解码 BasePlayerCreate(type=0) 和 UpdateArena(type=8)，数据包事件解码为自主逆向

### 2.3 BlitzKit 数据 — ✅ 可用（坦克数据主源）

| 来源 | URL | 内容 |
|------|-----|------|
| `tanks.pb` | `https://api.blitzkit.app/definitions/tanks.pb` | 723 辆坦克完整数据（元数据 + 弹种血量/模块伤害） |
| `models.pb` | `https://api.blitzkit.app/definitions/models.pb` | 723 辆坦克装甲板厚度 + 俯仰角 |
| `collision.glb` | `https://api.blitzkit.app/tanks/{id}/collision.glb` | 装甲板 3D 几何（hull/turret/gun armor plates） |
| `model.glb` | `https://api.blitzkit.app/tanks/{id}/model.glb` | 视觉模型（含履带、负重轮、炮管等模块 mesh） |
| `icons/big.webp` | `https://api.blitzkit.app/tanks/{id}/icons/big.webp` | 坦克封面图（筛选器卡片） |

### 2.4 DVPL 游戏文件 — ✅ 可用（需游戏安装；`game_data/` 已随项目分发）

- XML DVPL：装甲板厚度（hull/turret/gun `<armor_N>` + chassis `<leftTrack>`/`<rightTrack>` + gun `<gun>` + `vehicleDamageFactor` 识别 spaced 装甲）
- YAML DVPL：碰撞包围盒 + `points` 偏移（炮塔/炮管位置）

---

## 三、项目文件

| 文件 | 内容 | 来源 |
|------|------|------|
| `tanks.pb` | BlitzKit 坦克数据库（723 辆，运行时解析元数据/武器/装填） | `fetch-blitzkit` 命令 |
| `models.pb` | BlitzKit 模型定义（炮塔/主炮→gun/turret_0X 节点映射，723 辆） | `fetch-blitzkit` 命令 |
| `tank_cache.json` | 坦克数据缓存（723 辆，由 tanks.pb 构建，含 HP/模块伤害/装甲摘要/俯仰角） | `fetch-tanks` 命令（`from_blitzkit`） |
| `armor_cache.json` | 装甲板厚度（723 辆，hull+turret+gun 三段） | Python 脚本解析 models.pb |
| `gun_angles.json` | 炮管俯仰角（723 辆） | Python 脚本解析 models.pb |
| `game_data/` | 游戏装甲/碰撞数据提取（723 辆 per-tank JSON，可移植） | `extract-game` 命令 |
| `glb_cache/` | 3D 模型本地缓存（按需下载落盘） | `/glb/` 代理 |
| `tank_images/` | 坦克封面图（723 辆 big.webp） | `fetch-icons` / `/api/tank_image/` 代理 |
| `web/vendor/` | 前端本地依赖（Three.js + Chart.js，离线可用） | 手动 vendored |

### 源码结构

```
src/
├── main.rs              # CLI 入口（18 个子命令）
├── agent/
│   ├── mod.rs           # Agent Loop（async + AgentEvent 流式事件 + CLI 阻塞包装）
│   ├── llm_client.rs    # LLM 客户端（OpenAI 兼容 API，async）
│   └── tools.rs         # Agent 工具（6 个，含 view_tank 打开 3D 查看器）
├── web/
│   ├── mod.rs           # Web GUI 服务器（axum：SSE 事件/会话/面板 API）
│   └── index.html       # Web 前端（对话 + 玩家 + 回放 + 对比 + 设置）
├── models/              # battle / report / config + Token 统计
├── replay/              # parser / scanner / combat
└── wargaming/
    ├── mod.rs
    ├── tank_resolver.rs # 坦克解析（from_blitzkit + 弹种 HP/模块伤害）
    ├── blitzkit.rs      # tanks.pb Protobuf 解析（元数据 + 图标批量下载）
    ├── api_client.rs    # WG API 客户端（仅战绩查询）
    ├── snapshot.rs      # API 数据快照存储
    ├── prematch.rs      # 对局前瞻（阵容强度/威胁/弱点）
    ├── dvpl.rs          # DVPL 解码（LZ4_HC + XML/YAML + 装甲/碰撞/points）
    ├── game_extract.rs  # 游戏数据批量提取（game_data/ 生成与加载）
    ├── penetration.rs   # 统一击穿判定（+ HP/模块伤害区分）
    └── viewer.rs        # 3D 查看器（坦克/配置切换 + 装甲穿透判定）
```

---

## 四、作业要求对照

| 要求 | 状态 | 说明 |
|------|------|------|
| R1 核心逻辑用 Rust | ✅ 完成 | 回放解析+战斗事件解码+API客户端+Agent Loop（async）+BlitzKit pb 解析+DVPL解码+3D查看器后端+Web GUI 后端+penetration.rs 统一击穿判定 |
| R2 用户交互界面 | ✅ 完成 | Web GUI（Agent 对话+功能面板）+ CLI 交互终端 + Web 3D 查看器（坦克/配置切换+装甲分析+弹道可视化） |
| R3 可自定义模型配置 | ✅ 完成 | config.toml（WG API + LLM + 回放路径）+ Web GUI 设置页直接编辑保存 |
| R4 实时进度渲染和打断 | ✅ 完成 | 批量扫描进度 + Agent 工具调用进度（CLI stderr / Web SSE 事件流）+ Ctrl+C / 打断按钮 |
| R5 上下文历史管理 | ✅ 完成 | 多轮对话 + history 查看 + JSON 保存/加载 + Web 会话管理 |
| R6 Token 用量与价格统计 | ✅ 完成 | 精确统计 + 价格换算 + 预算中断 + usage 命令 + Web 用量面板 |

---

## 五、待实现功能

### 5.1 已实现（原待办清单核对）

- ✅ **对局前瞻**（`prematch`）：批量查询 WG API 战绩，分析阵容强度（平均胜率/场均伤害）、识别高威胁玩家（伤害超均值 1.2 倍）和薄弱点（胜率低于均值 0.95 倍），生成开局指引与双方对比
- ✅ **击穿判定增强**：HEAT 间隙衰减、HE 溅射伤害、装备修正（Calibrated Shells/Enhanced Armor）
- ✅ **多炮塔/多主炮配置切换**：Config 选择器（见 1.6）
- ⛔ **实时穿透渲染**：已弃用（GPU 着色与 Rust 判定易失配，穿透统一由 `penetration.rs` 计算）
- ✅ **Web 图形界面**：Agent 对话 + 功能面板（见 1.8）
- ✅ **坦克数据源切换**：全部来自 BlitzKit，WG API 仅查战绩（见 1.1/1.3）
- ✅ **血量伤害/模块伤害区分**：tanks.pb field4/field5 分别解析（见 1.3）

### 5.2 3D 查看器增强（未实现）

- 自动旋转炮塔到最佳装甲角度建议
- 弹种穿深随距离衰减曲线

### 5.3 双层装甲精确计算（部分实现）

当前点击判定支持"外部模块 → spaced → 主装甲"的层序消耗；同 section 多 plate 克隆几何完全重叠（BlitzKit collision.glb 数据限制），无法按位置区分同 section 内不同 plate 的厚度归属。

---

## 六、时间节点

| 日期 | 任务 | 状态 |
|------|------|------|
| 8.30 23:59 | 网络学堂发布选题 | ⬜ 今天截止 |
| 9.1 23:59 | 给至少 3 位同学评论 | ⬜ 未完成 |
| 9.6 23:59 | 发布设计文档摘要和项目链接 | ⬜ 未完成 |
| 9.8 23:59 | 试用至少 3 位同学作品 | ⬜ 未完成 |
| 9.10 上午 | 分课堂展示与互评 | ⬜ 未完成 |
