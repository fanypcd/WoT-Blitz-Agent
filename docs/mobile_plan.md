# 移动端 App 方案（Tauri 2）

> 状态：方案已评审方向，前期工作已完成（见 §7）。M1 尚未开始。
> 本文档是移动端工作的锚点：其他会话改动项目时，请对照 §8 的"将来会 touch 的文件"避让。

## 1. 目标

把现有 `wotb-agent`（Rust axum 后端 + 浏览器 Three.js 前端）发布为可安装到手机的 App：

- Android 优先（当前开发机 Windows 即可完成构建/签名/分发）；iOS 需 macOS，只交付工程。
- 资产全量本地化：GLB 车模、地图底图、图鉴、pb 数据不依赖运行时联网（GLB 允许首启下载补齐）。
- 功能范围：全场回放播放器（核心）、Agent 聊天、Tankopedia/装甲查看、战绩扫描、设置。

## 2. 技术选型

**Tauri 2**（移动端自 2.0 起为稳定特性）。后端已是 Rust：回放解析、axum 路由、
资产服务全部复用，改造量最小。淘汰项：Capacitor/TWA/PWA（后端无法上设备）、
Flutter/RN + FFI（前端整套重写）。

## 3. 架构

```
┌─ mobile/ (Tauri 壳 crate) ────────────────────────────┐
│  启动: paths::set_root(应用数据目录)                    │
│        → 复制随包只读资源到数据目录（版本感知增量复制）  │
│        → build_router() 构建与桌面版相同的 axum Router │
│        → 注册自定义协议桥: WebView 请求 oneshot 转发    │
└──────────────┬────────────────────────────────────────┘
               │ 依赖
┌─ wotb-agent (lib) ────────────────────────────────────┐
│  现有 src/* 全部模块；main.rs 保留为 CLI bin           │
└────────────────────────────────────────────────────────┘
```

关键决策：

- **协议桥而非 localhost 端口**：`register_uri_scheme_protocol` + `tower::ServiceExt::oneshot`
  把 WebView 请求转给 Router。免端口冲突、免 Android cleartext / iOS ATS 配置，双平台一致。
- **路径双根**：`paths::set_root()` 注入运行时根目录；资产查找顺序 = 可写数据目录 →
  随包只读资源 → （GLB 专属）CDN。现 `data.rs::data_path` 是 CWD 相对路径，
  手机上 CWD 无意义，必须改。需一并收编的硬编码路径：
  `viewer.rs` 的 `GLB_CACHE_DIR`/`VENDOR_DIR`、`playback_viewer.rs` 的 `VENDOR_DIR`、
  `map_assets.rs` 的 `MAP_DIR`、screenshots 路由、sessions 目录。

## 4. 资产策略

| 资产 | 体积 | 策略 |
|---|---|---|
| pb/tank_cache/game_data/图标/vendor | ~8MB | 随包只读资源 |
| 地图底图 26 张 @2x | 2.0MB | 随包（已导出，见 §7）|
| GLB 车模 ~735 辆 | ~2.1GB | **首启全量下载**到应用私有 glb_cache/（复用 glb_handler 下载链路 + 并发/续传/进度页）；运行时 `/glb/...` 缓存优先、CDN 兜底逻辑不变，跳过预下载也能用 |
| 可写数据 | — | 应用私有目录：glb_cache、sessions、config.toml、replays 导入 |

GLB 下载源做成可插拔（CDN / PC 局域网同步 / 打进安装包为后续可选）。

## 5. UI 排版（移动端单独重排，复用全部后端 API 与 JS 逻辑）

导航：顶部 6 Tab → **底部 Tab 栏**（Agent / 对局 / 图鉴 / 战绩 / 设置；Compare 收进战绩）。
实现：每页 HTML 骨架拆桌面/移动两个变体（`<style>`/`<script>` 拆成共享常量），
服务端按 User-Agent（Tauri WebView UA 含 Mobile）选模板。

设计规范：触控目标 ≥44px、基准字号 16px、`viewport-fit=cover` + `env(safe-area-inset-*)`、
去 hover 依赖改点选。

各页要点（详案见会话记录）：

- **回放播放器**（改动最大）：竖屏优先，画布全屏；topbar → 常驻状态条（计时/比分/存活）；
  左右 240px 队伍面板 → 底部抽屉（上滑呼出，我方/敌方程子页签，点玩家=跟随）；
  控制条只留 播放/进度/倍速/镜头 四高频项，GLB/标签/底图透明度收进 ⚙ 弹层；
  3 秒无操作沉浸模式；自由镜头手势 = 单指平移 + 双指缩放；loader 页取消
  （入口 = 对局 Tab 的导入列表）。
- **Agent 聊天**：252px 会话侧栏 → 抽屉/下拉；输入区固定底部 + 键盘避让；
  stat-grid 自适应网格字号提到 15-16px 后自然 2 列，基本不动。
- **图鉴**：卡片 1-2 列全宽；**装甲查看器**：3D 全屏，侧面板 → 底部弹层三页签
  （信息/穿深/配置），选中部位自动收起。
- **对比**：双栏 → 竖向 A/B 逐行交错。

## 6. 里程碑

| # | 内容 | 状态 |
|---|---|---|
| M1 | 路径运行时化（paths 模块）+ `build_router()` 拆分 + 移动模板选择骨架（桌面行为零变化） | 未开始 |
| M2 | Tauri 壳跑通：协议桥 + 随包资源 + 桌面验证 | 未开始 |
| M3 | Android 真机：文件导入、设置页、移动模板（静态结构页先行） | 未开始 |
| M4 | GLB 首启下载管理器 + 播放器/装甲查看器移动排版 + 性能降级 | 未开始 |
| M5 | 签名出包（APK/AAB）+ iOS 工程交付 | 未开始 |

（GLB 全量拉取由另一会话执行中，进度可用 §7 清单脚本查询。）

## 7. 前期工作产物（已完成，均不触碰现有源码）

| 产物 | 说明 |
|---|---|
| `mobile_assets/maps/` | 26 张地图底图（@2x 512²，共 2.0MB）+ `maps_manifest.json`（含 sha256/尺寸），已验证解码正确 |
| `scripts/export_mobile_maps.py` | 底图导出器：零依赖纯 Python，独立 DVPL 解码（0/3/zlib + 纯 Python LZ4 块解压），`--into-cache` 可同时喂桌面版缓存，`--force` 重导 |
| `scripts/asset_manifest.py` | 资产清单生成器 → `mobile_assets/manifest.json`：各资产计数/体积 + GLB 拉取进度（含缺失清单）。M2 打包前以它做齐备性检查 |
| 环境体检 | 见 §8 |

移动端打包最终资产构成（基于当前实测）：随包资源 ≈ 12MB（pb 2MB + 图标 3.8MB +
game_data 1MB + vendor 1.4MB + 底图 2MB + 杂项），GLB 首启下载 ≈ 2.1GB。

## 8. Android 构建环境（已安装，2026-09-25）

| 组件 | 版本/路径 |
|---|---|
| JDK | Microsoft OpenJDK 17.0.20.1（winget MSI），`JAVA_HOME=C:\Program Files\Microsoft\jdk-17.0.20.101-hotspot` |
| Android SDK | `D:\Android\Sdk`（platform-tools + platforms;android-35 + build-tools;35.0.0 + cmdline-tools/latest，共 2.7GB），`ANDROID_HOME` |
| NDK | 27.0.12077973，`NDK_HOME=D:\Android\Sdk\ndk\27.0.12077973` |
| tauri-cli | 2.11.5（npm 全局 `@tauri-apps/cli`，非 cargo 版） |
| Rust targets | aarch64-linux-android / armv7-linux-androideabi / x86_64-linux-android / i686-linux-android |

三个环境变量均已 `setx` 持久化（对新开终端生效）。cmdline-tools 与 SDK 包走
腾讯镜像（`mirrors.cloud.tencent.com/AndroidSDK/`），Google 官方源在本机可达但慢。

iOS：需 macOS + Xcode + Apple 开发者账号（$99/年），Windows 上不可构建。

## 9. 与并行会话的协调（移动端将来会 touch 的文件）

- `Cargo.toml`：加 `[lib]` 段 + tauri 依赖（workspace 化）
- 新增 `src/lib.rs`；`src/data.rs` 或新增 `src/paths.rs`：根目录运行时化
- `src/web/mod.rs`：`serve()` 拆出 `build_router()`（路由表不动）
- `src/wargaming/viewer.rs`：抽公共 GLB 下载函数（glb_handler 行为不变）
- 每页 HTML 模板拆分（`playback_viewer.rs`/`viewer.rs`/`web` 内嵌 HTML）
- 其余全部为新增目录（`mobile/`、`mobile_assets/`、`scripts/`）

在 M1 动手前，以上文件的重叠改动尽量与移动端会话对齐时间点。
