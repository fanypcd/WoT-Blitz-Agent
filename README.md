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
| BlitzKit 数据集成 | tanks.pb 735 辆（弹种/穿深/血量）+ models.pb（spaced 权威 + **逐板装甲厚度/车体炮塔碰撞盒/俯仰极限**，唯一来源）+ GLB 几何 |
| LLM Agent | 自然语言对话，10 个工具自主获取数据并生成分析（含热力图截图） |
| 消费方切面导出 | `dataset`（权威数据集：metadata/settlement/diagnostics）与 `facets`（消费方切面：playback / ai-review）；WASM 四入口（`parseResult`/`parsePlayback`/`parseShotReplays`/`parseAiReview`）供浏览器/消费方直接投影（契约 v2，见 [docs/replay-contract-v2-supremacy-type39.md](docs/replay-contract-v2-supremacy-type39.md)） |
| 射击复现（实验性） | 从回放提取射击事件链，3D 查看器按射手视角复现弹道与判定；**游戏弹孔解码点**（服务器 segment 按游戏 `DecodeShotSegment` 同构公式解出的部件 AABB 量化点，橙色 ◆ 标记，与本地 raycast 弹着点对照）；目标/射手模型按**实际搭载配置**选炮塔/主炮变体 |
| 全场实时回放 | 14 车连续播放整场战斗：客户端滤波位姿（AvatarFilter 移植，60Hz→0.1s 网格）+ prop2 炮塔/炮管随动 + 弹道飞行动画 + 实时血量/击杀流/计分板；播放/暂停/0.5~16x 倍速/进度拖拽，自由/俯视/跟随镜头（**跟随**进入即贴到目标近距、旋转中心锁在车身；**俯视**对准该图可玩区中心），可选 GLB 真实车模；**装填进度条**与客户端逐状态复刻（整夹一条不分割 / 夹内推弹不补弹 / 弹鼓逐发补槽 / 开火取消 / 服务器剩余弹数快照重锚，见 `frontend/src/scene/reloadBar.js`）。多配置坦克按 **实际搭载**（ARENA_INFO 组成 blob → 发射弹种 → 初始血量 三级证据）自动选炮塔/主炮变体。3D 场景叠加（离线导出真贴图场景 + 分层地表实时合成）；场上标签（昵称+坦克名+血量条，恒定屏幕占比/半透明）；队伍色弹道轨迹线。**画质三档**（低/中/高，加载时选择，移动 WebView 默认低）：低=客户端小地图地面（保留 3D 起伏）+ 尖首盒子代理车模 + 无建筑 + 抗锯齿关，中=烘焙底图+建筑，高=分层地表+建筑+抗锯齿；档位取 `?q=` > localStorage > 设备默认 |

## 功能依赖一览

各模块运行所需的数据文件与外部服务（数据文件均已内置，标 ★ 的项需要联网或本机游戏客户端）：

| 模块 | 数据文件依赖 | 外部服务 |
|------|------|------|
| 回放解析（single/scan/combat/loadout/playback） | 仅 `.wotbreplay` 文件本身 | 无 |
| 射击复现（3D 查看器） | `tanks.pb`（comp blob 局部 id→配置对号）· `models.pb`（俯仰极限/部件盒/原点，按实际搭载 comp 对号）· `game_data/`（炮管/底盘碰撞盒）· `data/cache/models/`（模型）★ | 无（模型缓存后离线可用） |
| 全场实时回放 | 同射击复现（真实车模开关关闭时仅需 `.wotbreplay`） | 同上（仅 GLB 开关） |
| 3D 装甲查看器 / 穿透热力图 | `models.pb`（逐板装甲/车体炮塔碰撞盒/原点/变体映射）· `game_data/`（炮管/底盘碰撞盒）· `data/cache/models/` ★ | 无（缓存后离线可用） |
| WG API 集成（Player/Compare/Prematch/Snapshot） | `tank_cache.json`（昵称→tank_id 联表） | WG API（application_id） |
| LLM Agent | `tank_cache.json` + 各工具自身依赖 | LLM API 端点 |

补充说明：

- **实际搭载配置**（射击复现/实时回放的炮塔/主炮变体选择与**俯仰锚定**）三级证据链：
  ① ARENA_INFO 组成 blob（回放内嵌，确定性）；② 发射弹种 ⊆ 炮弹表；③ 初始血量 = 车体+炮塔
  health（×1.125 改进耐久）。依次回退，均不命中 → 顶级配置。prop2 俯仰 frac 按锚定
  范围解码（扇区化：随炮塔朝向 front/back 分段）。
- `data/cache/models/` 首次访问自动从 BlitzKit CDN 下载（reqwest 失败自动回退系统 curl）；
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
# replay_dir = "data/replay_samples"
# Linux/WSL 回放目录示例:
# replay_dir = "/mnt/c/Users/你的用户名/AppData/Local/wotblitz/DAVAProject/replays"
# Windows 回放目录示例:
# replay_dir = "C:/Users/你的用户名/AppData/Local/wotblitz/DAVAProject/replays"
replay_dir = "data/replay_samples"
tank_cache_path = "data/tank_cache.json"
```

### 3. 数据（已内置，无需额外准备）

所有数据已随仓库分发在 `data/` 目录下，**克隆后即可直接运行，无需执行任何数据准备命令**：

| 数据 | 位置 | 说明 |
|------|------|------|
| 坦克数据源 | `data/tanks.pb` | BlitzKit 坦克数据库（运行时解析元数据/武器/装填，唯一数据源） |
| 模型定义 | `data/models.pb` | 炮塔/主炮→`gun/turret_0X` 模型节点映射 + spaced 分类权威 + 逐板装甲厚度/车体炮塔碰撞盒/俯仰极限（唯一来源） |
| 坦克缓存 | `data/tank_cache.json` | 由 `fetch-tanks` 构建的缓存 |
| 装甲/碰撞数据 | `data/game_data/`（700+ 个 JSON） | 从游戏 DVPL 提取：primaryArmor 装甲摘要、炮管/底盘碰撞盒（BlitzKit 缺项）及调试字段 |
| 数据版本清单 | `data/data_version.json` | 各数据文件对应的游戏版本与更新时间（`update-data` 维护） |

其他按需缓存（首次访问自动下载，无需手动准备，均在 `data/cache/` 下）：3D 模型
`data/cache/models/`、坦克封面图 `data/cache/tank_images/`、地形高度场
`data/cache/terrain/`（`fetch-terrain` 预提取）；前端为 `frontend/` Vue 3 SPA（Vite 构建、经 rust-embed 编译期嵌入 exe，天然离线）。
如需**完全离线**（查看器/回放不再联网拉模型），运行 `fetch-models` 一次性全量预下载
全部坦克 GLB（约 2 GB，可断点续跑）。

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

注意：`armor_cache.json` 是静态回退数据（仓库内无生成器），不随 `update-data` 刷新；
仅在 `game_data/` 缺失时兜底装甲摘要，优先级更低。

回放 3D 场景（建筑/树木真贴图 GLB、分层地表、地形尺度元数据）同样
源自本机客户端，按客户端自身管线离线导出，游戏更新后建议重跑：

```bash
python tools/export_map_glb.py                       # 全部地图 → data/cache/maps/<space>.*
python tools/export_map_glb.py --map 19              # 只导指定图（回放数字 id / 显示名 / 键）
python tools/export_map_glb.py --ground-only         # 只重导地面（整图烘焙+分层），跳过场景 GLB
python tools/export_map_glb.py --jobs 8              # 并行进程数（默认 4；进度见 _export_status.json）
```

导出器与客户端同链：`maps.yaml` 数字 id → space 场景（`.sc2` 实体树 + `.scg` 几何 +
NMaterial 材质树贴图），LOD/可见性/开关态语义镜像 DAVA `RenderObject` 批次规则；
地表着色逐分支复刻客户端 `Landscape/tilemask-fp.sl`（GLOBAL_TINT / SEPARATE_LM /
SCALED_TILES / HEIGHT_BLEND）。

坦克模型（`model.glb` / `collision.glb`）目前仍以 **BlitzKit 缓存**
`data/cache/models/` 为运行期来源；仓库另备一条**本机客户端自产**的并行管线，
产物写到 `data/cache/local_models/`（不替换数据源，两者可逐辆对照）：

```bash
python tools/export_tank_glb.py --all                 # 全量 735 辆 → data/cache/local_models/<id>/
python tools/export_tank_glb.py --tank 9489 --tank 7169
python tools/export_tank_glb.py --all --texture-mode none   # 只比几何（最快）
python tools/compare_tank_glb.py --all --audit        # 逐字节数值等价回归
python tools/compare_tank_glb.py --tank 9489 --render # 并排渲染三联图（可直接看图判"效果"）
```

对照结论（2026-10-01 实测，735 辆全量）：`collision.glb` **735/735**、`model.glb`
**733/735** 与 BlitzKit 产物逐字节等价（含节点顺序与 POSITION/NORMAL/TEXCOORD_0/1/2/索引
原始字节）；并排渲染逐辆轮廓 IoU ≈ 1.000。贴图**槽位集合与 BlitzKit 完全对齐**
（731/735 图片数相同、无一辆缺槽位；baseColor 逐像素 89.8% 一致；alphaMode 一致 99.1%）。
两处按设计不同、且**逐通道实测推翻了"BlitzKit 指派不成立"的笼统说法**：`normal` 槽 BlitzKit
取旧法线图（`images/<T>_NM`，DXT1），我们取 PBR 法线图（`images_pbr/<T>_NM`，BC5 + 重建 z）；
`metallicRoughness` 槽必须把 `baseRMMap` 的 ch0/ch1 **搬到 G/B**（glTF 规定 G=粗糙度、
B=金属度，原样返回会把金属度当粗糙度）——搬通道后 G 与 BlitzKit 1010/1011 一致，B（金属度）
只有我们取到真值；`occlusion` 两边同取 `miscMap.R`（1011/1011 逐像素相同）。
详见 [docs/local-model-export.md](docs/local-model-export.md) §4。

#### 地图资产清单（`data/cache/maps/<space>.*`，运行时缓存）

| 文件 | 说明 |
|------|------|
| `<space>.glb` | 静态场景（建筑/桥/岩石/树木真贴图；客户端 LOD0 批次语义，天空盒不导出） |
| `<space>.ground.webp` | 整图烘焙地面 4096²（客户端着色公式逐像素；前端回退与 2D 模式用） |
| `<space>.ground.cm/lm/tile0/tile1/mask0/mask1[/hmap0/hmap1].webp` | 分层地表贴图（全部无 alpha——Chrome 把带 alpha 的 webp 预乘解码会压暗 GPU 侧权重） |
| `<space>.ground.layers.json` | 分层合成参数（textureTiling/tileScale/tileColor/HeightBlend 等逐图旗标） |
| `<space>.json` | 地图元数据（世界包围盒/地形尺度/导出统计） |

前端 3D 地形优先用分层贴图按客户端同款公式实时合成（tile 细节纹理以原生分辨率
按 `textureTiling` 平铺，清晰度等同客户端、不受整图分辨率限制），分层缺失时回退
整图烘焙。个别图铺设参数异常时可用 `data/maps/<key>.json`
（`{"size_m":..,"x":..,"z":..,"rot90":..}`）微调，不动代码。

### 4. 测试回放（可选）

`data/replay_samples/` 提供 3 个示例 `.wotbreplay` 文件（无游戏也可测试）。
把 `config.toml` 的 `replay_dir` 设为 `data/replay_samples` 即可。

## 分发形态

本仓库的产物面已收敛为**回放解析核心**：Rust 库（`cargo test --workspace`）＋
`v*` tag 触发的 `.github/workflows/release.yml` 构建出的 **WASM 发行产物**
（`wotb-replay-wasm-<tag>.zip`）。该产物由
[WoTBTools](https://github.com/A158Coke/WotbTools) 按 `deploy/agent/source.json`
锁定的 release + sha256 直取，**前端开发在 WoTBTools 仓库进行**；本仓库
`frontend/` 保留但不再维护（本机调试 Web GUI 仍可 `cargo run --release -- web`）。

**已移除（2026-10，不再维护）**：

| 曾有的分发形态 | 涉及文件 |
|------|------|
| Windows 免安装便携包（轻量/全量 zip，产物落 `dist/`） | `scripts/package.ps1`、`scripts/build-all.ps1`、`scripts/zipdir.py`、`scripts/asset_manifest.py` |
| Android App（Tauri 2 壳，轻量/离线双 APK） | `mobile/`、`mobile_assets/`、`scripts/export_mobile_maps.py`、`docs/mobile_plan.md` |

仍保留的构建/导出脚本：

| 脚本 | 用途 |
|------|------|
| `scripts/build-wasm.ps1` | 本机构建 WASM 产物到 `frontend/public/wasm/`（release.yml 走同一套 cargo + wasm-bindgen 步骤） |
| `scripts/export_asset_pack.py` | 导出静态资产包到 `release/asset_pack/`（对象存储整目录直传，供前端 `?assets=` 取用） |
| `scripts/serve_asset_pack.mjs` | **本机联调**：把 `release/asset_pack/` 按消费方布局带 CORS 伺服（`node scripts/serve_asset_pack.mjs 8123`），供 WotbTools dev server 作资产源。WotbTools 前端测试的标准姿势见其 `docs/frontend/local-production-dev.md` |

静态资产包的导出流程（消费方为 WoTBTools 前端）：

```bash
cargo run --release -- dump-map-index > map_index.json
python scripts/export_asset_pack.py --map-index map_index.json
```

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

全部 735 辆坦克的图鉴网格。支持中英文模糊搜索（`e100`、`is7`、`def…`、中文车名如
`星际猎人`；支持子串与紧凑子序列匹配）、
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
  镜头：自由环绕 / 全局俯视 / 点击名册或场上车辆跟随（跟随模式相机位置随车
  刚性平移，方位/距离/俯仰完全由鼠标控制）；「真实车模」开关懒加载
  GLB 模型。场上每车悬浮昵称+血量条（恒定屏幕占比、半透明、不被场景遮挡，
  悬浮高度随距离自适应）；开火显示全弹道轨迹线（友军蓝/敌军红纯色，与弹着点
  特效同步淡出）；3D 地形叠加离线导出的真贴图场景与分层地表（天空盒不渲染）。
  未侦察车辆（回放数据不含其位置流，即作者客户端当时看不到的车）
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
loadout        # 单回放开局配置解析（队伍/坦克/初始血量/耐久加成/弹种表）
dataset        # 权威回放数据集导出（metadata/settlement/diagnostics JSON）
facets         # 消费方切面导出（playback / ai-review JSON；--parts 选择，供 WotbTools 等消费方投影）
playback       # 全场实时回放（浏览器连续播放整场战斗）
player         # WG API 玩家查询
snapshot       # API 数据快照（take/diff）
view           # 3D 装甲查看器
prematch       # 对局前瞻（--replay 可从回放提取阵容）
parse-game     # 解析单个游戏 DVPL 文件
extract-game   # 批量提取装甲/碰撞数据到 game_data/
fetch-terrain  # 预提取全部地图高度场缓存
fetch-minimaps # 预提取全部地图小地图缓存（低画质回放地面）
fetch-blitzkit # 重新下载 BlitzKit 数据源
fetch-tanks    # 重建 tank_cache.json
fetch-icons    # 批量下载坦克封面图
fetch-models   # 全量预下载坦克 GLB 模型到 data/cache/models/（约 2GB，完全离线）
update-data    # 游戏版本更新后一键刷新全部数据（版本感知增量更新）
config         # 查看/编辑配置
usage          # Token 用量统计
dump-map-index # 导出地图注册表 JSON（id → key/space/display，资产打包入口）
dump-tank-data # 导出逐车 tank/{id}.json（装甲/配置/原点，资产打包入口）
dump-shell-kinds # 导出全局弹种 id → 弹种表 JSON（射击复现弹种反解，静态资产面常量）
dump-methods   # 逆向工具：转储 method38/8/type=32 原始包字节
dump-entity    # 逆向工具：转储指定实体时间窗口内全部包
```

## 仓库结构

| 路径 | 说明 |
|------|------|
| `src/main.rs` / `src/lib.rs` | 入口：CLI（`main.rs`）与库形态（`lib.rs`，业务模块全集） |
| `src/agent/` | LLM Agent：工具编排与自然语言对话 |
| `crates/replay-core/` | **回放解析核心库**（零网络依赖，原生/WASM 双目标）：内部领域模型、事件解码、实时回放时间线、数据投影（结果/回放/评审，架构契约 v2：名人堂是消费方投影，非 Agent 能力）。消费方切面（`crates/replay-core/src/facets/`）**只做投影与互验、不下判断**——AI 切面附带原始未滤波证据（type=10 原始位姿与 prop2 炮塔、prop3 血量广播、method8 原始命中通知全变体、未钳制 HP）供消费方自建口径 |
| `crates/replay-wasm/` | **浏览器通道入口**（契约第 6 节纯客户端回放）：.wotbreplay 字节 → 核心库 → 独立能力 JSON（`parseResult`/`parsePlayback`/`parseShotReplays`/`parseAiReview`）；前两者可选注入 `tankNamesJson`（车型名表），射击复现可选注入俯仰锚定表与弹种反解表；`scripts/build-wasm.ps1` 构建到 `frontend/public/wasm/` |
| `src/replay/` | 兼容垫片（re-export 核心库）+ 服务端增值标注（loadout 弹种表，依赖 BlitzKit 坦克表 IO） |
| `src/wargaming/` | WG API、坦克/模型/地图资产、3D 装甲查看器与实时回放前端 |
| `src/web/` | Web GUI（axum 路由 + 内嵌前端 + 离线 Three.js vendor） |
| `src/models/`、`src/data.rs` | 服务端数据模型（report/config）；运行时路径层（数据目录可重定向） |
| `docs/` | 项目文档：[索引](docs/index.md)、[数据来源与 BlitzKit 依赖评估](docs/game-data-sources.md)、[数据面清单](docs/data-inventory.md)、解耦进度总览（[decoupling-status](docs/decoupling-status.md)）、本地模型自产（[local-model-export](docs/local-model-export.md)）、回放契约 v2（[replay-contract-v2](replay-contract-v2-supremacy-type39.md)）、Vue 迁移方案（已完成留档）、WotbTools 交叉引用裁决 |
| `tools/export_map_glb.py` | 回放 3D 场景/地表离线导出器（DAVA 解析库在 `tools/wotbtools/`） |
| `tools/export_tank_glb.py` | 坦克 GLB 的**本机客户端**自产管线（并行于 BlitzKit 缓存，见上） |
| `tools/compare_tank_glb.py` | 两来源坦克模型的对照器：数值等价回归 + 并排渲染差异图 |
| `tools/export_tank_icons.py` | 坦克封面图的**本机客户端**自产管线（`Gfx/UI/BigTankIcons`，剥 DVPL 得裸 webp）→ `data/cache/local_tank_icons/` |
| `data/` | 内置数据（tanks.pb / models.pb / tank_cache / game_data / 版本清单） |
| `data/cache/` | 运行时缓存（gitignore）：`models/` 坦克 GLB、`maps/` 地图资产、`tank_images/` 封面、`terrain/` 地形高度场、`screenshots/` 截图 |
| `data/replay_samples/` | 示例回放（仓库内置 3 个） |
| `scripts/` | 构建脚本：`build-wasm.ps1`（WASM 产物到 `frontend/public/wasm/`）、`export_asset_pack.py`（静态资产包到 `release/asset_pack/`）、`serve_asset_pack.mjs`（本机联调伺服该资产包） |

## 环境要求

- Rust 1.70+
- LLM API Key（OpenAI / 清华 AI 平台 / 其他 OpenAI 兼容 API）
- WG API Application ID（可选，项目自带公开 key；免费注册：https://developers.wargaming.net/applications/）
- WoTB 游戏安装（可选，用于 DVPL 游戏文件解析和真实回放文件）
- 网络（按需）：BlitzKit CDN（GLB 模型/数据首次下载，之后走本地缓存）、
  WG API、LLM API 端点；系统 curl（GLB 下载的自动回退通道）

## 许可证

本项目代码以 [MIT License](LICENSE) 发布。

引用开源库：
- `wotbreplay-parser` (MIT) — https://github.com/eigenein/wotbreplay-parser
- 其他 Rust crate 均为 MIT 或 Apache-2.0 许可

注意：MIT 许可仅覆盖本项目自研代码。运行中下载/生成的第三方与游戏内容
（BlitzKit 数据、坦克 GLB 模型、Wargaming 游戏贴图、`.wotbreplay` 回放等）
归其各自权利人（Wargaming 等）所有，不在本许可范围内。
