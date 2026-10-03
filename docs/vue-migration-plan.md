# Vue 3 全量迁移方案 ✅ 已完成（2026-09-28）

> **2026-10-03 注记（读本文前先看）**：本仓前端已**冻结**——桌面便携包与 Android
> 分发形态整体移除，后续前端开发在 WotbTools 仓库进行（见 [README §与 WotbTools 的关系](../README.md)）。
> 因此本文提到的 `scripts/package.ps1`、`scripts/build-all.ps1`、`mobile/`（Tauri 壳）、
> `sync-android-assets.sh`、`prepare-assets.py` **均已删除**，相关段落只作历史记录；
> 现今本仓前端仅用于本机调试（`cd frontend && npm ci && npm run build` +
> `cargo run --release -- web`）。

> 状态：Phase 0–5 全部落地。四个页面（主 GUI 六 Tab、坦克详情、实时回放、3D 装甲检视器）
> 均已切流至 Vue 3 SPA（`frontend/`，Vite 构建、rust-embed 编译期嵌入二进制），
> 嵌入 HTML 字符串与 `web/vendor/` 目录全部退役。以下为原始方案，留档。

> 目标：将全部 4 个前端页面从「嵌在 Rust 源码里的 vanilla HTML/JS」迁移到 Vue 3 + Vite，
> 参考架构：[WotbTools](https://github.com/A158Coke/WotbTools)（Vue 3 + Vite + three.js 回放可视化）。
> 后端 API、数据层、回放解析全部不动，纯前端迁移。

## 一、现状盘点

### 页面清单（全部为 Rust 内嵌字符串）

| 页面 | 路由 | 位置 | 规模 | 三方依赖 |
|---|---|---|---|---|
| 主 GUI（6 Tab） | `/` | `src/web/index.html`（`include_str!`） | 1589 行 | markdown-it、KaTeX、Chart.js |
| 坦克详情页 | `/tank/{id}` | `src/web/tank_detail.html`（`include_str!`） | 533 行 | 同上 |
| 实时回放查看器 | `/playback` | `src/wargaming/playback_viewer.rs` 内 `INDEX_HTML` | ~1450 行 | three.js |
| 3D 装甲检视器 | `/armor_view/view/{id}` | `src/wargaming/viewer.rs` 内 `INDEX_HTML`（`viewer_index_html()` 动态生成） | ~4100 行 | three.js（import map + CDN 回退） |

主 GUI 六个 Tab：Agent（聊天）/ Tankopedia / Player / Replay / Compare / Settings，靠 `data-tab` 手动切换。

### 服务与分发形态（迁移的约束条件）

- **桌面**：`wotb-agent serve` → Axum 监听 18999，`build_router()` 承载全部 ~40 个端点。
- **移动**：Tauri 壳注册自定义 `tauri://` 协议，把 WebView 的**所有**请求桥接进同一个
  `build_router()`（`mobile/src-tauri/src/lib.rs:265`）。→ 前端迁移对 APK **零改动**。
- **打包**：桌面 `scripts/package.ps1`（便携 zip，内含 `web/vendor/` 散文件）；
  APK `prepare-assets.py` + `sync-android-assets.sh`（`web/vendor/` 一并打进 assets）。
- **三方库**：vendored 于 `web/vendor/`（three 0.169 / Chart.js / KaTeX / markdown-it），运行时从磁盘读。

## 二、目标架构

```
frontend/                      # 新增：Vue 3 + Vite 项目
├── src/
│   ├── api/                   # 全部后端端点的 fetch 封装（唯一 API 层，按域分文件）
│   ├── composables/           # useChat / usePlayback / useModelsProgress ...
│   ├── views/                 # ChatView / TankopediaView / PlayerView / ReplayView
│   │                          # / CompareView / SettingsView / TankDetailView
│   │                          # / PlaybackView / ArmorView
│   ├── components/            # TankCard / TeamRoster / Killfeed / ControlBar / MdContent ...
│   ├── scene/                 # three.js 命令式场景类：PlaybackScene / TankScene（不响应式化）
│   └── styles/                # 现有 CSS 变量平移为 design tokens
├── dist/                      # Vite 构建产物 → 嵌入二进制
└── vite.config.js             # dev proxy → 127.0.0.1:18999
```

- **单二进制分发不破坏**：`rust-embed` 嵌入 `frontend/dist`；debug 构建读盘（配合 Vite 产物热替换），
  release 构建编译期嵌入。
- **web/vendor 目录退役**：three/chart.js/katex/markdown-it 全部改 npm 依赖，由 Vite 打包，
  天然离线，`package.ps1` 与 `prepare-assets.py` 相应瘦身。
- **移动端零改动**：Tauri 桥接的是 `build_router`，Axum 换掉页面后 APK 自动跟上。

## 三、技术选型

| 项 | 选择 | 理由 |
|---|---|---|
| 框架 | Vue 3.5，`<script setup>` 组合式 API | 与 WotbTools 同栈；单文件组件适合逐页搬迁 |
| 语言 | **JavaScript 起步** | 现前端 ~9000 行无类型 JS，全量 TS 标注会显著拖慢迁移；后端返回多为 `serde_json::Value` 动态结构，TS 收益有限。后续可渐进加 `lang="ts"` |
| 构建 | Vite 6 | 事实标准；dev proxy 直连 Axum |
| 路由 | vue-router 4 | Tab → 真路由，URL 可直达/刷新保持（现 `data-tab` 切换做不到） |
| UI 库 | **不引入** | 现有整套暗色主题 CSS 直接平移，视觉零变化、迁移面最小 |
| three.js | npm 锁 `0.169` | 与现 vendored 版本一致，零升级风险 |
| 其他 | chart.js / katex / markdown-it（npm），新增 dompurify | 替代 vendor 文件；dompurify 补 markdown 渲染的 XSS 防护 |
| 嵌入 | `rust-embed` | debug 读盘 / release 嵌入；编译期需要 `frontend/dist` 存在（见风险表） |

**核心原则（WotbTools 同款分工）：three.js 场景保持命令式，不做响应式化。**
渲染循环、位姿滤波、建筑 instanced mesh 等原样搬进 `scene/` 类；Vue 只负责面板 UI，
通过 `scene.onTick()` 回调 → reactive 状态、控件事件 → `scene` 方法 两条单向通道桥接。

## 四、分阶段实施（每阶段独立可发布、可回退）

### Phase 0 · 地基（~1 天）

- `frontend/` 脚手架（Vite + Vue3 + vue-router）；`styles/tokens.css` 平移现有 `:root` CSS 变量。
- Axum 接入 `rust-embed`：`/assets/*` 服务静态产物；`index_handler` 改返回 Vue 版 SPA
  （带 history fallback）。旧主页面临时挂到 `/legacy` 保留。
- `vite.config.js`：proxy `/api`、`/armor_view`、`/glb`、`/vendor`、`/screenshots` → `127.0.0.1:18999`。
- 新增 `scripts/build-all.ps1`：`npm ci && npm run build` → `cargo build --release`；
  `package.ps1` 前置检查 `frontend/dist` 存在。
- **验收**：Vue 空壳在桌面浏览器 + APK 双端可访问，dev 热更新链路可用。

### Phase 1 · TankDetail 试点（~0.5 天）

- `views/TankDetailView.vue` + `api/tank.js` + `TankCard.vue`（Tankopedia 弹窗后续复用）。
- **验收点**：独立详情页、armor_view iframe 集成、封面图代理缓存链路。
- 切流：`/tank/{id}` 路由改 SPA fallback，删除 `tank_detail.html` 的 `include_str!`。

### Phase 2 · 主 GUI 六 Tab（~2–3 天）

- 路由：`/`(Agent)、`/tankopedia`、`/player`、`/replay`、`/compare`、`/settings`。
- 组件划分：
  - `ChatView`：事件轮询 → `useChat` composable；markdown-it + KaTeX 渲染管线 → `renderMarkdown()` 工具 + `MdContent` 组件；
  - `TankopediaView`：列表/筛选 + 详情弹窗（复用 `TankCard` + armor_view iframe）；
  - `PlayerView`：snapshot/diff + chart.js 图表；
  - `ReplayView`：扫描/聚合报告 + 跳转回放（`?file=` 参数照旧）；
  - `CompareView`、`SettingsView`（config 表单 + token usage 图 + 模型批量下载进度轮询）。
- 切流后删除 `src/web/index.html` 及其 handler。

### Phase 3 · Playback 回放查看器（最难，~2–3 天）

- `scene/PlaybackScene.js`：three.js 场景逻辑**原样搬出**（14 车位姿滤波、炮塔/炮管随动、
  弹道飞行动画、地形/烘焙底图、建筑加载、画质三档、res=mini 伺服链路——不改渲染语义）。
- Vue 壳：`LoaderPanel`（文件路径 + 画质选择）、`TopBar`（地图/计时/比分）、`TeamRoster`×2、
  `Killfeed`、`Banner`、`ControlBar`（播放/倍速/seek/画质徽标/开关）。
- 桥接：`onTick` 状态节流后进 reactive store；列表点选 → `scene.focusPlayer(id)`。
- 切流后删除 `playback_viewer.rs` 内 `INDEX_HTML` 字符串（**保留全部数据端点**）。

### Phase 4 · Armor Viewer 3D 检视器（最大，~3–4 天）

- `scene/TankScene.js`：GLB 加载、碰撞体、剖面/着色模式、穿透模拟可视化原样迁移。
- Vue 壳：车型筛选、弹表、穿透结果面板、`replay_shot` 集成、hold/ready 握手。
- 删除 `viewer_index_html()` 动态拼 HTML 与 import map CDN 回退——base path（`/armor_view` 前缀）
  改由构建期 `base` 或运行时注入常量解决。
- tank_detail 弹窗的 iframe 集成方式保留。

### Phase 5 · 收尾（~0.5 天）

- 删除全部残留嵌入 HTML 与 `include_str!`。
- `web/vendor/` 退役：`package.ps1`、`prepare-assets.py`、`sync-android-assets.sh` 同步瘦身。
- `viewer.rs` 的独立 serve 路由（若仍使用）指向新前端。
- **双端回归**：便携 zip、全量 zip、APK 轻量/全量。

## 五、风险与对策

| 风险 | 对策 |
|---|---|
| `rust-embed` 编译期需要 `frontend/dist` 存在，直接 `cargo build` 会挂 | `build.rs` 检查缺失时报明确错误（提示先跑 `npm run build`）；日常用 `scripts/build-all.ps1` 统一入口 |
| three.js 升级引入回归 | 锁定 0.169 不追新（与现 vendored 完全一致） |
| KaTeX 字体路径 | Vite 自动打包 `@font-face` 资源，Phase 2 联网验证一次公式渲染 |
| APK WebView 兼容 | `build.target: 'es2020'`；现有代码已用 ESM/可选链，无新增风险 |
| 单二进制体积 | three+Vue gzip 后 ~700KB，且便携包不再拷 `web/vendor`（~5MB 散文件），总体积下降 |
| 回归风险 | 阶段共存 + 路由级切流，任一阶段可独立回退到旧页面 |
| 长迁移期的双栈混乱 | 每阶段切流即删旧源（包括字符串常量），不留第二份真相 |

## 六、工作量汇总

| 阶段 | 内容 | 估算 |
|---|---|---|
| 0 | 脚手架 + 嵌入 + 构建链 | ~1 天 |
| 1 | TankDetail 试点 | ~0.5 天 |
| 2 | 主 GUI 六 Tab | ~2–3 天 |
| 3 | Playback 回放 | ~2–3 天 |
| 4 | Armor Viewer | ~3–4 天 |
| 5 | 收尾退役 | ~0.5 天 |
| **合计** | | **~9–12 人日** |
