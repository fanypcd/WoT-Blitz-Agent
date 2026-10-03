# WoTB Blitz Tactics Agent

World of Tanks Blitz（坦克世界闪击战）的**回放解析核心**：以 Rust 解析 `.wotbreplay`
回放与游戏数据，向外输出权威语义数据（结算 / 全场回放时间线 / 射击链 / AI 评审切面），
并以 WASM 发行产物供产品前端消费。

> 本仓库不再维护面向终端用户的产品形态（Web GUI、桌面便携包、Android 壳均已退役）。
> 产品与前端在 [WoTBTools](https://github.com/A158Coke/WotbTools) 仓库开发。

## 能力

- **回放解析核心**（`crates/replay-core`，零网络依赖，原生 / WASM 双目标）
  - 协议层：`.wotbreplay` 数据段全量解码——实体位姿、炮塔/炮管俯仰、弹丸生命周期、
    命中通知与部件、装填相位、竞技场状态、血量链等；字节级语义与使用状态见
    [docs/回放与射击逆向总集.md](docs/回放与射击逆向总集.md)（唯一权威参考）；
  - 投影切面（facets，契约 v2）：`BattleSummary` 结算、`PlaybackData` 全场时间线
    （位姿网格 / 弹道 / 击杀 / 阶段 / 可见性 / 装填）、AI 评审事件流（附带原始未滤波
    证据）。fail-closed：证据缺失一律 Option / 哨兵保留 raw，不猜 0；
  - 射击复现数据：逐发弹道两端、双方姿态 / 炮塔 / 俯仰、命中结果与受击部件、
    弹种兜底链、逐发质量标记。
- **WASM 通道**（`crates/replay-wasm`）：四个独立入口
  `parseResult` / `parsePlayback` / `parseShotReplays` / `parseAiReview`，
  `.wotbreplay` 字节 → 能力 JSON，消费方按需取用（只要结果时不强制物化全场时序）。
- **数据与资产管线**（`src/wargaming/`、`tools/`、`scripts/`）：BlitzKit 数据源、
  本机客户端 DVPL 提取（装甲 / 碰撞盒 / 地图场景 GLB）、坦克 GLB 双来源对照管线、
  静态资产包导出（`release/asset_pack/`）。
- **质量门禁**：切面均为 additive（`PlaybackData` v2 / `AiReviewFacet` v1 版本不变）；
  CI 门禁 = wasm32 双目标编译 + `cargo test --workspace` + clippy 零告警。

## 与 WotbTools 的关系

本仓库是**上游回放解析器**；[WoTBTools](https://github.com/A158Coke/WotbTools)
是产品前端（回放 3D 场景 / 播放控制 / AI 复盘 UI），**前端开发与前端测试都在
WotbTools 仓库进行**，本仓只出 Rust 核心与 WASM 发行产物。

- **消费方式**：WotbTools 经 `deploy/agent/source.json` 锁定本仓 release tag + sha256，
  在浏览器端加载 WASM 本地解析（其服务端解析器已整体删除，本仓是它唯一的回放解析器）；
- **发行版本**：`Cargo.toml` 的 `[workspace.metadata.release].version` 是唯一版本源，
  该值变更合入 main 后 release workflow 自动测试、构建、打 `v<version>` tag 并发布
  `wotb-replay-wasm-v<version>.zip`；
- **AI 复盘**：WASM → canonical facts → WotbTools 侧 `ClientAiReviewProjection`，
  parity 由其 required CI 常驻看护；
- **3D 资产**：地图场景 / 模型等静态资产由本仓管线导出为 `release/asset_pack/`
  （对象存储分发），WotbTools 前端经 `?assets=` 指向；本机联调用
  `node scripts/serve_asset_pack.mjs 8123` 伺服；
- **协议分工**：解析 / 契约问题在本仓先修，WotbTools 再更新其 pin；
  逆向结论冲突时以 [docs/wotbtools-cross-reference.md](docs/wotbtools-cross-reference.md)
  的裁决记录为准；
- `frontend/` 在本仓已冻结留档（仅本机调试可 `cargo run --release -- web`），
  不再维护，3D 回放的视觉验收由用户执行。

## 仓库结构

| 路径 | 说明 |
|------|------|
| `crates/replay-core/` | 回放解析核心库（零网络依赖，原生 / WASM 双目标）：协议解码、领域模型、时间线、投影切面 |
| `crates/replay-wasm/` | 浏览器通道入口：四个独立能力 JSON 入口（契约第 6 节） |
| `src/` | 历史应用层（CLI / LLM Agent / Web GUI / 资产管线）：产品面退役后作为数据与资产管线的载体保留 |
| `docs/` | [文档索引](docs/index.md)：逆向总集、回放契约 v2、数据来源、WotbTools 交叉引用裁决等 |
| `tools/`、`scripts/` | 地图 / 坦克 GLB 导出、资产包导出与本机联调伺服脚本 |
| `data/` | 内置数据（tanks.pb / models.pb / game_data / 版本清单） |

## 许可证

本项目代码以 [MIT License](LICENSE) 发布。

引用开源库：
- `wotbreplay-parser` (MIT) — https://github.com/eigenein/wotbreplay-parser
- 其他 Rust crate 均为 MIT 或 Apache-2.0 许可

注意：MIT 许可仅覆盖本项目自研代码。运行中下载/生成的第三方与游戏内容
（BlitzKit 数据、坦克 GLB 模型、Wargaming 游戏贴图、`.wotbreplay` 回放等）
归其各自权利人（Wargaming 等）所有，不在本许可范围内。
