# WoTB Blitz Tactics Agent

AI 游戏分析助手 — 针对 World of Tanks Blitz（坦克世界闪击战）的回放分析和战术复盘工具。
以 Rust 解析回放与游戏数据为底座，LLM Agent 通过 10 个工具自主完成战绩查询、
回放复盘、装甲穿透分析与 3D 可视化。

## 功能总览

| 模块 | 说明 |
|------|------|
| 回放解析 | 解析 `.wotbreplay` 二进制文件，提取全部玩家完整战绩 |
| WG API 集成 | 查询玩家累计战绩 |
| 回放 vs API 对比 | 量化近期表现 vs 历史平均（胜率/场均伤害/评级） |
| API 数据快照 | 定期采集快照实现时间序列分析 |
| 对局前瞻 | 批量查询阵容战绩，分析强度、识别威胁与薄弱点 |
| 3D 装甲查看器 | GLB 双模型（视觉+碰撞）+ 炮塔/炮管交互 + 多层穿透判定 + 实时穿透热力图 |
| 坦克配置切换 | 多炮塔/多主炮坦克（如 E-100 双炮）切换配置，同步模型/装甲/弹种 |
| 击穿判定内核 | Rust 统一实现：跳弹/转正/overmatch/间隙甲/HEAT 间隙衰减/HE 溅射/装备修正 |
| BlitzKit 数据集成 | tanks.pb 735 辆（弹种/穿深/血量）+ models.pb（spaced 权威）+ GLB 几何 |
| LLM Agent | 自然语言对话，10 个工具自主获取数据并生成分析（含热力图截图） |
| 射击复现（实验性） | 从回放提取射击事件链，3D 查看器按射手视角复现弹道与判定；**游戏弹孔解码点**（服务器 segment 按游戏 `DecodeShotSegment` 同构公式解出的部件 AABB 量化点，橙色 ◆ 标记，与本地 raycast 弹着点对照）；目标/射手模型按**实际搭载配置**选炮塔/主炮变体 |
| 全场实时回放 | 14 车连续播放整场战斗：客户端滤波位姿（AvatarFilter 移植，60Hz→0.1s 网格）+ prop2 炮塔/炮管随动 + 弹道飞行动画 + 实时血量/击杀流/计分板；播放/暂停/0.5~16x 倍速/进度拖拽，自由/俯视/跟随镜头，可选 GLB 真实车模。多配置坦克按 **实际搭载**（ARENA_INFO 组成 blob → 发射弹种 → 初始血量 三级证据）自动选炮塔/主炮变体 |

## 功能依赖一览

各模块运行所需的数据文件与外部服务（数据文件均已内置，标 ★ 的项需要联网或本机游戏客户端）：

| 模块 | 数据文件依赖 | 外部服务 |
|------|------|------|
| 回放解析（single/scan/combat/loadout/playback） | 仅 `.wotbreplay` 文件本身 | 无 |
| 射击复现（3D 查看器） | `tank_cache.json`（昵称→tank_id/俯仰极限）· `tanks.pb`（comp blob 局部 id→配置对号）· `models.pb`（部件盒/原点）· `glb_cache/`（模型）★ | 无（模型缓存后离线可用） |
| 全场实时回放 | 同射击复现（真实车模开关关闭时仅需 `.wotbreplay`） | 同上（仅 GLB 开关） |
| 3D 装甲查看器 / 穿透热力图 | `game_data/`（装甲逐板厚度）· `models.pb`（原点/变体映射）· `glb_cache/` ★ | 无（缓存后离线可用） |
| WG API 集成（Player/Compare/Prematch/Snapshot） | `tank_cache.json`（昵称→tank_id 联表） | WG API（application_id） |
| LLM Agent | `tank_cache.json` + 各工具自身依赖 | LLM API 端点 |

补充说明：

- **实际搭载配置**（射击复现/实时回放的炮塔/主炮变体选择）三级证据链：
  ① ARENA_INFO 组成 blob（回放内嵌，确定性）；② 发射弹种 ⊆ 炮弹表；③ 初始血量 = 车体+炮塔
  health（×1.125 改进耐久）。依次回退，均不命中 → 顶级配置。
- `glb_cache/` 首次访问自动从 BlitzKit CDN 下载（reqwest 失败自动回退系统 curl）；
  下载完成后离线可用。
- 非调试模式下射击复现常显内容：入射延长射线（900m）+ 命中点标记 + 轨迹管；
  P1/P2 解码标记与移动标注归调试层（`debug=1`）。

## 快速开始

### 1. 编译

```bash
cargo build --release
```

编译产物在 `target/release/wotb-agent`。

### 2. 配置

从仓库 clone 后，先把示例模板复制为配置文件（模板不含真实密钥）：

```bash
cp config.toml.example config.toml    # Windows: copy config.toml.example config.toml
```

然后编辑 `config.toml` 填入自己的密钥：

```toml
[wg_api]
application_id = "9eeca6d62dfc4b1d8539ee5a76d0bf55"    # 项目自带可用的公开 key，可直接使用；也可申请自己的
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

### 3. 数据（已内置，无需额外准备）

所有数据已随仓库分发在 `data/` 目录下，**克隆后即可直接运行，无需执行任何数据准备命令**：

| 数据 | 位置 | 说明 |
|------|------|------|
| 坦克数据源 | `data/tanks.pb` | BlitzKit 坦克数据库（运行时解析元数据/武器/装填，唯一数据源） |
| 模型节点映射 | `data/models.pb` | 炮塔/主炮→`gun/turret_0X` 模型节点映射 + spaced 分类权威 |
| 坦克缓存 | `data/tank_cache.json` | 由 `fetch-tanks` 构建的缓存 |
| 装甲/碰撞数据 | `data/game_data/`（700+ 个 JSON） | 从游戏 DVPL 提取的可移植数据 |
| 数据版本清单 | `data/data_version.json` | 各数据文件对应的游戏版本与更新时间（`update-data` 维护） |

其他按需缓存（首次访问自动下载，无需手动准备）：3D 模型 `glb_cache/`、
坦克封面图 `tank_images/`、前端依赖 `web/vendor/`（Three.js/Chart.js，离线可用）。

### 3.1 游戏版本更新后如何更新数据

游戏客户端版本更新后（如 11.20 → 11.21），一条命令即可刷新全部派生数据：

```bash
cargo run --release -- update-data          # 下载最新 BlitzKit pb → 重建 tank_cache → 更新 game_data
cargo run --release -- update-data --check  # 只查看版本状态与将要执行的动作，不改任何文件
```

命令会读取游戏目录的 `version.txt.dvpl` 检测本机游戏版本，并与 `data/data_version.json`
清单比对：**版本变化 → 全量重提取 game_data；版本未变 → 只补新增坦克的缺失文件**。
常用参数：`--offline`（跳过联网下载，仅用现有 pb 重建）、`--force`（强制全量重提取）、
`--game-dir`（手动指定游戏目录，默认自动探测 Steam 安装路径）。

注意：`armor_cache.json` / `gun_angles.json` 是静态回退数据（仓库内无生成器），
不随 `update-data` 刷新；它们仅在 `game_data/` 缺失时兜底，优先级更低。

### 4. 测试回放（可选）

`replay_samples/` 提供 3 个示例 `.wotbreplay` 文件（无游戏也可测试）。
把 `config.toml` 的 `replay_dir` 设为 `replay_samples` 即可。

## Web UI 使用

```bash
cargo run --release -- web
```

启动后自动打开浏览器。界面为六个标签页：

### Agent（默认）

自然语言对话。直接输入问题，Agent 自主调用工具并给出分析：
"查询 Anonyme 的排位战绩"、"分析最近回放并和 API 对比"、"查看 E100 的装甲模型"、
"渲染 E100 正面的穿透热力图"、"复现 xxx.wotbreplay 的第 3 发射击"（*实验性）。
顶部按钮：Interrupt（打断当前分析，仅作用于当前会话）、History（对话历史）、Save（保存会话）。

**多会话管理**：左上下拉可切换会话，New 新建、Delete 删除（含磁盘持久化文件）、
Export 导出为 Markdown；每轮对话结束自动落盘 `data/sessions/<会话名>.json`，
服务重启后会话与历史自动恢复。同一会话同一时刻仅执行一轮对话（忙时后端返回 409）。

### Tankopedia

全部 723 辆坦克的图鉴网格。支持模糊搜索（`e100`、`is7`、`def…`）、
按等级/国家/类型筛选。点击卡片进入坦克详情：装甲汇总、逐板厚度（含 spaced 分类）、
弹种数据（正式名/穿深/伤害/HE 爆炸半径）。

### Player

WG API 玩家战绩查询。输入昵称搜索，返回随机/排位累计数据。

### Replay

- **Replay Scan Report**：批量扫描回放目录，生成聚合报告（胜率/场均伤害/坦克/地图统计）。
  支持模式筛选（All/Rating）和天数窗口。目录留空使用配置的 `replay_dir`；
  Windows 路径可直接粘贴（WSL 下自动转换）。
- **Shot Replay（射击复现，实验性）**：粘贴单场 `.wotbreplay` 路径 → Parse →
  得到该场全部射击事件列表（序号/时间/伤害/目标）。点击任一行在新窗口打开
  3D 查看器复现该发：相机按射手真实视角放置、受击坦克炮塔/炮管按回放时刻
  姿态呈现、热力图就绪后沿弹道自动穿透判定并在模型上标注弹着点。
  ⚠ 命中位置计算尚不完善，判定结果仅供参考。
- **实时回放**：同一路径输入旁的「▶ 实时回放」按钮（或 CLI `playback <file>`），
  新窗口整场连续播放：14 车按客户端滤波位姿实时移动、炮塔/炮管随动、弹道飞行
  与命中标记、实时血量/击杀流/计分板。播放/暂停/0.5~16x 倍速/进度条拖拽；
  镜头：自由环绕 / 全局俯视 / 点击名册或场上车辆跟随；「真实车模」开关懒加载
  GLB 模型。未侦察车辆（回放数据不含其位置流，即作者客户端当时看不到的车）
  在获得数据前自动隐藏。

### Compare

- **Compare (Replay vs API)**：近期回放表现 vs 历史累计对比（量化差异 + 图表）。
- **Prematch Lineup**：输入逗号分隔的昵称（或从回放提取），分析阵容强度、
  识别威胁与薄弱点。

### Settings

- **Model Config**：LLM 模型/端点/Key/上下文长度/Max Tokens/预算/思考模式，
  在线编辑并保存到 config.toml。
- **Token Usage**：调用次数/Token/费用统计 + 预算进度条 + 最近调用明细。

## 3D 装甲查看器操作

| 操作 | 功能 |
|------|------|
| 左键拖拽 | 旋转视角 |
| 滚轮 | 缩放 |
| 左键点击装甲 | 多层穿透判定（弹道轨迹线、接触点标记、信息窗口） |
| 右键水平拖拽 | 旋转炮塔 |
| 右键垂直拖拽 | 调整炮管俯仰角 |
| 弹种选择器 | 切换弹种（AP/APCR/HE/HEAT），判定与热力图同步 |
| Equip 开关 | Calib.Shells（+6%/+7% 穿深）/ Enh.Armor（+4% 装甲） |
| 穿透热力图按钮 | 逐像素击穿概率渐变（绿=稳定击穿 → 红=稳定抵挡，跳弹蓝紫高亮） |
| Show Collision 按钮 | 按厚度着色显示装甲板 |

## CLI 命令

以下命令均可用（详细参数见 `--help`）：

```
web            # Web 图形界面（推荐）
chat           # Agent 交互式对话（CLI）
single         # 单回放解析
scan           # 批量扫描回放目录
compare        # 回放 vs API 累计对比
combat         # 战斗事件时间线 + 射击推断（--json 含射击复现数据）
playback       # 全场实时回放（浏览器连续播放整场战斗）
player         # WG API 玩家查询
snapshot       # API 数据快照（take/diff）
view           # 3D 装甲查看器
prematch       # 对局前瞻（--replay 可从回放提取阵容）
parse-game     # 解析单个游戏 DVPL 文件
extract-game   # 批量提取装甲/碰撞数据到 game_data/
fetch-blitzkit # 重新下载 BlitzKit 数据源
fetch-tanks    # 重建 tank_cache.json
fetch-icons    # 批量下载坦克封面图
update-data    # 游戏版本更新后一键刷新全部数据（版本感知增量更新）
config         # 查看/编辑配置
usage          # Token 用量统计
```

## 环境要求

- Rust 1.70+
- LLM API Key（OpenAI / 清华 AI 平台 / 其他 OpenAI 兼容 API）
- WG API Application ID（可选，项目自带公开 key；免费注册：https://developers.wargaming.net/applications/）
- WoTB 游戏安装（可选，用于 DVPL 游戏文件解析和真实回放文件）
- 网络（按需）：BlitzKit CDN（GLB 模型/数据首次下载，之后走本地缓存）、
  WG API、LLM API 端点；系统 curl（GLB 下载的自动回退通道）

## 许可证

引用开源库：
- `wotbreplay-parser` (MIT) — https://github.com/eigenein/wotbreplay-parser
- 其他 Rust crate 均为 MIT 或 Apache-2.0 许可
