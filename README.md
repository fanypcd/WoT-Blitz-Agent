# WoTB Blitz Tactics Agent

AI 游戏分析助手 — 针对 World of Tanks Blitz（坦克世界闪击战）的回放分析和战术复盘工具。

## 功能

- **回放解析**：解析 `.wotbreplay` 二进制文件，提取 14 名玩家完整战绩
- **战斗事件时间线**：解码数据包流，追踪生命值变化、死亡事件、累计伤害曲线
- **射击事件推断**：关联伤害计数器和生命值变化，推断每发射击的目标/伤害/击杀
- **WG API 集成**：查询玩家累计战绩（25 个 API 端点，仅用于战绩查询；坦克数据全部来自 BlitzKit）
- **回放 vs API 对比**：量化近期表现 vs 历史平均的差异
- **API 数据快照**：定期采集快照实现时间序列分析（API 不支持按时间查询）
- **Web 图形界面**：`wotb-agent web` 启动独立 Web 应用——Agent 对话（SSE 流式进度 + 工具调用轨迹 + 打断）、玩家战绩查询、回放扫描报告、回放 vs API 对比、对局前瞻、模型配置编辑、Token 用量统计
- **3D 坦克查看器**：浏览器中加载 GLB 3D 模型，支持装甲分析、炮塔旋转、炮管俯仰、弹道轨迹可视化；可自由切换射击坦克（含对应口径/弹药）与受击坦克（按需缓存模型），图形化坦克筛选（封面图网格 + 等级/国家/类型筛选 + 名称搜索）
- **坦克配置切换**：多炮塔/多主炮坦克（如 E-100 的 12.8cm/15cm 双炮）可切换配置，同步更新火炮模型、装甲判定、弹种与穿深
- **多层穿透判定**：跳弹（>70°）、转正（AP 5°/APCR 2°）、overmatch（3x/2x 口径规则）、模块装甲（履带/炮管/炮盾 flat 抵消）、逐层消耗穿透力、spaced 附加装甲识别
- **统一击穿判定 + 伤害区分**：Rust `penetration.rs` 集中实现全部判定逻辑（跳弹/转正/overmatch/多层/HEAT 间隙衰减/HE 溅射/装备修正），前端通过 `POST /api/penetrate` 调用；区分**血量伤害（HP damage，穿透主装甲盒）**与**模块伤害（module damage，仅命中履带/炮管等外部模块）**
- **对局前瞻**：`prematch` 命令批量查询玩家战绩，分析阵容强度、识别威胁与薄弱点，可直接从回放文件提取双方阵容
- **BlitzKit 数据集成**：从 `tanks.pb` 提取 723 辆坦克完整数据（名称、tier/类型/国家、弹种、穿深、血量伤害、模块伤害、口径），从 `models.pb` 提取炮管俯仰角，从 `collision.glb` + `model.glb` 获取装甲板和模块几何，坦克封面图（big.webp）按需缓存
- **DVPL 游戏文件解码**：解析游戏本地文件提取装甲板厚度（含履带/炮管/spaced 附加装甲 `vehicleDamageFactor` 识别）和碰撞包围盒
- **LLM Agent**：自然语言对话，Agent 自主调用工具获取数据并生成分析报告（6 个工具，含打开 3D 装甲查看器）

## 快速开始

### 1. 编译

```bash
cargo build --release
```

编译产物在 `target/release/wotb-agent`（Windows: `target\release\wotb-agent.exe`）。
下文命令统一用 `cargo run --release -- <子命令>` 调用（首次会自动编译）；
也可把编译产物复制到项目根目录后用 `./wotb-agent <子命令>` 调用。

### 2. 配置

从仓库 clone 后，先把示例模板复制为配置文件（模板不含真实密钥）：

```bash
cp config.toml.example config.toml    # Windows: copy config.toml.example config.toml
```

然后编辑 `config.toml` 填入自己的密钥：

```toml
[wg_api]
application_id = "你的WG_API_KEY"    # https://developers.wargaming.net/applications/
server = "asia"                       # asia / eu / na

[llm]
endpoint = "https://api.openai.com/v1"
api_key = "你的API_KEY"
model = "glm-5"
context_length = 8192
thinking_mode = false
price_input_per_1k = 0.0
price_output_per_1k = 0.0
max_tokens = 4096
budget = 10.0                        # 预算上限（美元），超过自动中断

[replay]
# 回放目录：填入真实的 .wotbreplay 所在目录即可。
#   也可以直接用项目自带的测试回放（3 个示例文件，无需额外准备）:
# replay_dir = "replay_samples"
# Linux/WSL 回放目录示例:
# replay_dir = "/mnt/c/Users/你的用户名/AppData/Local/wotblitz/DAVAProject/replays"
# Windows 回放目录示例:
# replay_dir = "C:/Users/你的用户名/AppData/Local/wotblitz/DAVAProject/replays"
replay_dir = "replay_samples"
tank_cache_path = "data/tank_cache.json"
```

### 3. 准备数据

```bash
# 拉取/构建坦克数据（723 辆，来自 BlitzKit pb，无需 WG API）
cargo run --release -- fetch-blitzkit          # 下载 tanks.pb + models.pb → data/
cargo run --release -- fetch-tanks             # 从 BlitzKit 数据构建 tank_cache.json
cargo run --release -- fetch-icons             # 批量下载 723 辆坦克预览图 → tank_images/
```

数据文件统一放置在 `data/` 数据目录下。`tanks.pb` 是项目唯一坦克数据源（运行时直接解析获取元数据/武器/装填），`armor_cache.json`/`gun_angles.json` 来自 models.pb（装甲板/俯仰角），`tank_cache.json` 由 `fetch-tanks` 构建。

3D 查看器的模型与装甲数据已本地化，无需安装游戏即可运行：

- `data/game_data/` — 从游戏 DVPL 文件批量提取的 723 辆坦克装甲板 + 碰撞数据（`extract-game` 命令生成，已随项目分发）
- `glb_cache/` — 3D 模型本地缓存（首次访问某坦克时经 `/glb/` 代理从 BlitzKit CDN 下载并落盘）
- `tank_images/` — 坦克封面图（`fetch-icons` 批量下载，或经 `/api/tank_image/` 按需缓存）
- `web/vendor/` — Three.js 与 Chart.js 本地副本（离线可用，无需 CDN）

### 3b. 测试回放（可选）

项目在 `replay_samples/` 目录下提供了 **3 个示例 `.wotbreplay` 文件**，无需安装游戏即可测试回放相关功能：

| 文件 | 说明 |
|------|------|
| `20260902_2045__Anonyme_J39_Type_5_Exp_...wotbreplay` | Type 5 H Zetsu 对局（WinterMalinovka，重力模式） |
| `20260902_2053__Anonyme_J20_Type_2605_...wotbreplay` | Type 5 Heavy 对局（OasisPalms） |
| `20260902_2104__Anonyme_A116_XM551_...wotbreplay` | Sheridan Missile 对局（XM551 导弹坦） |

用法：把 `config.toml` 的 `replay_dir` 设为 `replay_samples`（如上），即可用 `scan` / `single` / `compare` 直接测试，例如：

```bash
# 扫描测试回放目录
cargo run --release -- scan replay_samples --mode all

# 解析其中单场
cargo run --release -- single replay_samples/20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay

# 网页版 Replay Scan
cargo run --release -- web
```

### 4. 使用

```bash
# Web 图形界面（Agent 对话 + 各功能面板，推荐）
cargo run --release -- web

# Agent 交互式对话（CLI）
cargo run --release -- chat

# 查询玩家战绩
cargo run --release -- player Anonyme --app-id 你的KEY --server asia

# 扫描回放目录生成报告
cargo run --release -- scan "C:/.../replays/" --tank-cache data/tank_cache.json --mode rating

# 回放 vs API 累计对比
cargo run --release -- compare Anonyme "C:/.../replays/" --app-id 你的KEY --server asia --tank-cache data/tank_cache.json

# 解析单场回放
cargo run --release -- single replay.wotbreplay --tank-cache data/tank_cache.json

# 战斗事件时间线 + 射击推断
cargo run --release -- combat replay.wotbreplay

# 3D 坦克查看器（浏览器）
cargo run --release -- view 28689 --tank-cache data/tank_cache.json

# API 数据快照
cargo run --release -- snapshot Anonyme --app-id 你的KEY --server asia --action take
cargo run --release -- snapshot Anonyme --app-id 你的KEY --server asia --action diff

# 解析游戏 DVPL 文件（游戏目录自动探测，WSL/Windows 均可；也可 --game-dir 显式指定）
cargo run --release -- parse-game R132_T100LT

# 批量提取游戏装甲/碰撞数据到 game_data/（723 辆，可移植）
cargo run --release -- extract-game

# 查看 Token 用量
cargo run --release -- usage

# 查看/编辑配置
cargo run --release -- config --show
```

### 5. Agent 对话示例

```
> 查询Anonyme的排位战绩
[Agent] Calling LLM (step 1/5)...
[Agent] Tool call: search_player ({"nickname":"Anonyme"})
[Agent]   Done (388 chars)
[Agent] Calling LLM (step 2/5)...
[Agent] Tool call: get_player_stats ({"account_id":2033684170})
[Agent]   Done (279 chars)
[Agent] Calling LLM (step 3/5)...

以下是玩家 Anonyme 的排位战绩信息：
排位场次: 9,852 | 胜率: 58.4% | 场均伤害: 2,746 | 显示评级: 6,754

> 分析Anonyme最近的排位回放，和API累计数据对比
[Agent] Calling LLM (step 1/5)...
[Agent] Tool call: compare_replay_vs_api ({"nickname":"Anonyme","mode":"rating"})
...
近期 106 场排位 vs API 累计 9852 场：
胜率: 68.9% vs 58.4% (+10.5%)
场均伤害: 3670 vs 2746 (+33.7%)
评级: 4638 → 6754 (+2116)

> 查看一下E100的装甲模型
[Agent] Tool call: view_tank ({"tank":"E 100"})
[Agent]   Done (238 chars)

已为你打开 **E 100** 的3D装甲查看器！🛡️
（浏览器自动打开，可旋转视角、点击装甲测试穿透）
```

### 6. 3D 查看器操作

| 操作 | 功能 |
|------|------|
| Tank Filter 筛选 | 图形化坦克选择器（弹窗网格 + 封面图 + 等级/国家/类型筛选 + 名称搜索），点击卡片选择 Shooter/Target 坦克 |
| Config 选择器 | 切换坦克的炮塔/火炮配置（如 E100 的 12.8cm / 15cm 主炮），同步更新火炮模型、装甲判定、弹种与穿深 |
| Shooter 选择器 | 选择射击坦克，确定口径与弹药（对应弹种数据） |
| Target 选择器 | 选择受击坦克，切换 3D 模型并重新加载装甲数据 |
| 左键拖拽 | 旋转视角 |
| 滚轮 | 缩放 |
| 左键点击 | 多层穿透判定（用射手坦克弹药 + 受击坦克装甲；弹道轨迹线、接触点标记、信息窗口、跳弹/转正/overmatch） |
| 右键水平拖拽 | 旋转炮塔 |
| 右键垂直拖拽 | 调整炮管俯仰角 |
| 弹种选择器 | 选择当前弹种（随射手坦克改变，如 AP/APCR/HE/HEAT），多弹种切换 |
| Show Collision 按钮 | 按厚度着色显示装甲板 |

## CLI 命令一览

| 命令 | 功能 | 说明 |
|------|------|------|
| `web` | Web 图形界面 | 独立 Web 应用：Agent 对话（流式进度/工具轨迹/打断）+ 玩家查询 + 回放扫描 + 对比 + 前瞻 + 配置 + Token 用量 |
| `chat` | Agent 对话 | CLI 交互式多轮对话，Agent 自主调用工具 |
| `single <file>` | 单回放解析 | 提取 14 名玩家完整战绩 |
| `scan <dir>` | 批量扫描 | 聚合报告（胜率/伤害/评级/坦克/地图） |
| `combat <file>` | 战斗事件 | 生命值时间线 + 射击推断 + 死亡事件 |
| `player <name>` | 玩家查询 | WG API 累计战绩（随机+排位） |
| `compare <name> <dir>` | 对比分析 | 回放近期 vs API 累计 |
| `snapshot <name>` | 数据快照 | 定期采集 + 差值对比 |
| `view <tank_id>` | 3D 查看器 | 浏览器中显示 3D 模型 + 装甲分析 + 穿透判定 + 坦克/配置切换 |
| `parse-game <name>` | 游戏文件解析 | DVPL 解码 + 碰撞包围盒 + 装甲板（游戏目录自动探测） |
| `extract-game` | 游戏数据提取 | 批量解析全部 723 辆 DVPL → `data/game_data/*.json`（可移植） |
| `fetch-blitzkit` | BlitzKit 数据源 | 下载 `tanks.pb` + `models.pb` → `data/` |
| `prematch <names\|--file>` | 对局前瞻 | 批量查玩家战绩，分析阵容强度；`--replay` 解析回放自动提取双方阵容 |
| `fetch-tanks` | 坦克数据 | 从本地 tanks.pb 数据构建 `data/tank_cache.json`（723 辆，无需 WG API） |
| `fetch-icons` | 坦克预览图 | 批量下载 723 辆坦克封面图（BlitzKit big.webp）→ `tank_images/` |
| `config --show` | 配置管理 | 查看/编辑 config.toml |
| `usage` | Token 统计 | 调用次数/Token/费用/明细 |

## Agent 工具

Agent 可自主调用以下工具获取数据：

| 工具 | 功能 |
|------|------|
| `search_player` | 按昵称搜索玩家 |
| `get_player_stats` | 获取 WG API 累计战绩 |
| `scan_replays` | 批量扫描回放生成报告 |
| `parse_replay` | 解析单场回放详情 |
| `compare_replay_vs_api` | 回放 vs API 对比 |
| `view_tank` | 打开 3D 装甲查看器（`target`=受击/查看方，`shooter`=射击方，均为模糊查询；多匹配时返回细化列表供选择） |

## 数据源

| 数据源 | 内容 | 获取方式 |
|--------|------|----------|
| 本地 `.wotbreplay` | 每场战斗完整数据 | 游戏自动保存 |
| WG Public API | 玩家累计战绩（随机+排位）/ 军团 / 赛事 / 成就 | 25 个 API 端点（**仅战绩查询**） |
| 游戏 DVPL 文件 | 装甲板 + 碰撞包围盒 + 部件位置偏移 | LZ4_HC 解压 |
| BlitzKit `tanks.pb` | 723 辆坦克名称、tier、类型、国家、弹种、穿深、伤害、装填（弹夹/弹鼓） | 运行时 Protobuf 解析（唯一数据源）→ `data/tanks.pb` |
| BlitzKit `models.pb` | 723 辆坦克装甲板厚度 + 炮管俯仰角 | Protobuf 解析 → `data/armor_cache.json` + `data/gun_angles.json` |
| BlitzKit CDN | `collision.glb`（装甲板几何）+ `model.glb`（视觉模型 + 履带/炮管模块 mesh） | HTTP 下载，经 `/glb/` 代理缓存到 `glb_cache/` |
| BlitzKit tank icons | 坦克封面图（`/tanks/{id}/icons/big.webp`） | 经 `/api/tank_image/` 代理缓存到 `tank_images/` |

## 项目结构

```
wotb-agent/
├── Cargo.toml
├── config.toml.example        # 配置模板（含 WG API + LLM 字段，不含密钥；复制为 config.toml）
├── data/                      # 数据目录（全部静态数据文件）
│   ├── tank_cache.json        #   坦克数据缓存（由 tanks.pb 构建, 723 辆，含 HP/模块伤害）
│   ├── tanks.pb               #   BlitzKit 坦克数据库（元数据/武器/装填, 运行时解析, 723 辆）
│   ├── models.pb              #   BlitzKit 模型定义（炮塔/主炮→模型节点映射, 运行时解析, 723 辆）
│   ├── armor_cache.json       #   装甲板厚度数据（BlitzKit models.pb, 723 辆）
│   ├── gun_angles.json        #   炮管俯仰角数据（BlitzKit models.pb, 723 辆）
│   └── game_data/             #   游戏装甲/碰撞数据提取（DVPL, 723 辆, 可移植）
├── glb_cache/                 # 3D 模型本地缓存（BlitzKit CDN, 按需下载落盘；不入库）
├── tank_images/               # 坦克封面图缓存（BlitzKit big.webp, 723 辆；不入库）
├── web/vendor/                # 前端本地依赖（three/ Three.js + chart.umd.min.js Chart.js）
├── src/
│   ├── main.rs              # CLI 入口（18 个子命令）
│   ├── agent/
│   │   ├── mod.rs           # Agent Loop（async，AgentEvent 流式事件 + CLI 阻塞包装）
│   │   ├── llm_client.rs    # LLM 客户端（OpenAI 兼容 API，async）
│   │   └── tools.rs         # Agent 工具定义和执行（含 view_tank 打开 3D 查看器）
│   ├── web/
│   │   ├── mod.rs           # Web GUI 服务器（axum：Agent SSE 事件/会话/各面板 API）
│   │   └── index.html       # Web 前端（对话 + 玩家 + 回放 + 对比 + 设置 五个标签页）
│   ├── models/
│   │   ├── battle.rs        # 单场战斗数据结构
│   │   ├── report.rs        # 多场聚合报告
│   │   └── config.rs        # 配置 + Token 统计
│   ├── replay/
│   │   ├── parser.rs        # 回放解析（meta + battle_results）
│   │   ├── scanner.rs       # 目录扫描 + 日期/模式筛选
│   │   └── combat.rs        # 战斗事件解码 + 射击推断
│   └── wargaming/
│       ├── tank_resolver.rs # 坦克解析（from_blitzkit 从 BlitzKit 数据构建 + 弹种 HP/模块伤害）
│       ├── api_client.rs    # WG API 客户端（仅战绩查询）
│       ├── blitzkit.rs      # BlitzKit tanks.pb Protobuf 解析（元数据/图标批量下载）
│       ├── snapshot.rs      # API 数据快照存储
│       ├── prematch.rs      # 对局前瞻（阵容强度/威胁/薄弱点分析）
│       ├── viewer.rs        # 3D 查看器（axum + Three.js：坦克/配置切换 + 装甲穿透判定）
│       ├── dvpl.rs          # DVPL 解码 + 装甲/碰撞解析 + spaced 识别 + 部件偏移
│       ├── game_extract.rs  # 游戏数据批量提取（data/game_data/ 生成与加载）
│       └── penetration.rs   # 统一击穿判定（跳弹/转正/overmatch/多层/HE + HP/模块伤害区分）
```

## 环境要求

- Rust 1.70+
- Wargaming API Application ID（免费注册：https://developers.wargaming.net/applications/）
- LLM API Key（OpenAI / 清华 AI 平台 / 其他 OpenAI 兼容 API）
- WoTB 游戏安装（可选，用于 DVPL 游戏文件解析和回放文件）

## 许可证

引用开源库：
- `wotbreplay-parser` (MIT) — https://github.com/eigenein/wotbreplay-parser
- 其他 Rust crate 均为 MIT 或 Apache-2.0 许可
