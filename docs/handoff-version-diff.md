# 交接说明：相邻版本变更总览（`agent-version-diff`）

> 交接日期：2026-11-27  
> 涉及仓库：`D:\Class\Rust\Project`（数据/工具/资产）、`D:\Class\Rust\WotbTools`（前端/Android）  
> 当前状态：本地已打通并全部验证；**生产 `/diffs` 尚未发布**，WotbTools 尚未提交/合并，视觉验收未做。

## 0. 一句话目标

在 WotbTools 提供「版本变更」页，对比相邻主线 dataset（当前 `11.21.0 → 11.21.0_1 → 11.22.0`）的坦克参数、装甲、研发、物品与地图差异，并给出坦克参数弹窗、装甲小窗、地图前后对比。

## 1. 已完成

### 1.1 本地数据与线上对齐

- `release/asset_pack_11220`（11.22.0）从 r2 同步到 **`11.22.0-20261126-mechanics1`**：
  - 931 件 / 13.4 MB 逐文件 sha256 校验；
  - 全量 `verify --pack --full`：**7843 件 / 4.53 GB PASS**；
  - `content_digest = sha256:7edf65be25445d53e1edc11df13c69cac8075b112b936c76714b73dc8112a914`
  - `manifest_sha256 = 4976f44f110ab2144eb23fca313896367453317fd1fd48795de4db9444157bec`
- `release/asset_pack`（11.21.0）修复本地漂移：
  - 187 份陈旧 `tank/*.json` 从线上 `11.21.0-20261125-ui1` 回拉替换；
  - 移除多余的 `data/tankMechanics.json`（备份在 `release/_sync_11.21.0_ui1/`）；
  - 全量 **7810 件 / 4.51 GB PASS**。
- `release/versions.json` 已重新生成：default `11.22.0`（mechanics1）；`11.4.0.138-r5` 预览别名保留。
- 线上 11.22 的演进关系（已核实）：
  - r2 → **r3**：补 2x 坦克图（manifest 743 件）；
  - r3 → **mechanics1**：+`data/tankMechanics.json`，187 份 `tank/*.json` 增加 `mechanics` 字段；`delta.json` base=r3，1 added / 187 modified。

### 1.2 数据侧语义 diff 工具

新增：

- `D:\Class\Rust\Project\tools\version_diff.py`
- `D:\Class\Rust\Project\data\version_lineage.json`
- `D:\Class\Rust\Project\tools\test_version_diff.py`（14 例）

命令：

```bash
cd D:\Class\Rust\Project
python -I tools/version_diff.py \
  --lineage data/version_lineage.json \
  --datasets-root release/datasets \
  --out release/diffs --all --strict
```

输出（`release/diffs` 被 gitignore，仅本地生成）：

```text
release/diffs/index.json
release/diffs/11.21.0__11.21.0_1/11.21.0-20261125-ui1__11.21.0_1-20261125-ui1/
release/diffs/11.21.0_1__11.22.0/11.21.0_1-20261125-ui1__11.22.0-20261126-mechanics1/
  meta.json
  summary.json
  tanks/<id>.json
  maps/<id>.json
  items.json
```

关键语义（改动工具时必须保留）：

- 坦克按 `tank_id`；配置按 `(turret_local, gun_local)`（`turret_name` 为空，名字只作回退；重复键 fail-closed）。
- 弹种按 `type`；发动机按名字；研发模块按 `(category, name_en, level)`，unlock stem 解析成显示名；科技树 `sourceModule` 同法解析。
- 地图按仓库 `map_index.json` 的 `space` 聚合，**不能按 `variantKey`**：11.22 变体目录自指 `variantKey`，43 个 key 会被误判成新增 7 张图；真实 36 space。地图行带**实例计数**（2026-11-28 additive）：场景物件 = scenery.glb 带 mesh 节点数（节点名 = 来源 sc2 物件名，按名字多重集算逐名 added/removed）；草地 = grass.bin 每实例 12 字节。
- destructibles 逐实例比较 `collision.health` / `type.health` / `collision_type` / 原型 / 名称 / 位置；健康值变化与 `CollisionVolume` 改名分开计数。
- 内部 ID（`shell_global_ids`、`localId`、`stem`、`gameVersion`）不参与语义比较。
- 单侧字段进 `schema` 桶（如 `mechanics:187`、`stats.clip:249`、`skills.effects.lobbyIcon:20`）；展示层重生成（2x 图标 / `map2d` / cover / ground）和 3D 产物（模型 / 悬挂 / 履带）分桶；纯模型车进 `modelOnly`；未解析引用列 `unresolved`，`--strict` 失败。
- 配置 chip 已改为结构化：`changePath` + `config`，**不要把炮名重新拼进 `path`**（炮名可能含点，无法可靠拆分）。
- summary 的 `tanks[]` / `modelOnly[]` 每行带 `premium` / `collector`（照抄 `tank_cache` 的
  `is_premium`/`is_collector`，2026-11-27 additive；仅供前端百科同款车名配色，diff 语义不读它）。

当前结果（本地真实生成）：

| pair | 坦克 | 说明 |
|---|---|---|
| `11.21.0 → 11.21.0_1` | +1 / -0 / 0 changed | 仅 Relic D3；Rinoceronte 仅模型（modelOnly 1、technicalOnly 1） |
| `11.21.0_1 → 11.22.0` | +3 / -0 / 19 changed | 装甲 5、配置 10、modelOnly 7、technicalOnly 493、地图 36、schema `mechanics:187` + `stats.clip:249`、unresolved 0；物品 consumables 4 / provisions 9；科技树边 0 |

### 1.3 本地 `/diffs` 静态通道

`D:\Class\Rust\Project\scripts\serve_asset_pack.mjs` 已支持第 5 个参数：

```bash
cd D:\Class\Rust\Project
node scripts/serve_asset_pack.mjs 8129 release/asset_pack release/datasets release/diffs
```

已实测：

- `/diffs/index.json` 200
- 两个 pair 的 `summary.json` 200
- `/v/11.22.0/version.json`、`/v/11.21.0_1/version.json` 200

### 1.4 WotbTools 页面（2026-11-27 二轮改版后的最终形态）

新增（未提交）：

- `frontend/src/composables/useVersionDiff.js` + `.test.js`
  - catalog 校验/缓存、`datasetAssetUrl`、`datasetTankImageUrl`、`datasetMap2dUrl`、`datasetCoverUrl`、
    `fetchPairSummary` / `fetchTankDiff` / `fetchMapDiff`；
  - 二轮新增：`fetchDatasetTank`（按版本取 `tank/<id>.json`）、`fetchDatasetTankCrops`（`tank_images/crops.json`
    的 `icons`，缺失回退 `{}`）、`datasetTankModelUrl`（**白名单守卫**：只接受
    `/glb/<数字>/(collision|model).glb`，拒绝其他路径——装甲对比器用它加载两版碰撞/车体模型）。
- `frontend/src/components/AgentVersionDiff.vue` + `.test.js`
  - 最终版面是**变更日志式**：`新增/移除` 紧凑卡（百科同款 `VersionDiffVehicle`：8:5 封面 + 国籍布旗 +
    内容框裁剪 + 等级/类型 pill）、装甲章节**内嵌双版本装甲厚度视图**、参数变化卡（旧值 → 新值 + 差额，
    按配置分组）、地图折叠区（搜索 + 两版底图滑杆弹窗）、页尾折叠区（研发/物品/科技树/modelOnly/
    technicalOnly/schema/展示层/产物/未解析）。
  - 装甲章节对每个装甲有改动的车直接内嵌 `ArmorComparisonViewport`（不折叠、不弹窗）：一个 WebGL canvas
    用 scissor 分成左右两版，厚度着色沿用装甲查看器调色板，**白色轮廓 + polygonOffset 高亮改动板件**，
    左右滑杆（及左右两个版本角标按钮）切换两版可见比例，指针悬停显示所在版本与厚度；配置下拉在多炮车
    上切换对比配置。数据 = 两版 `tank/<id>.json`（configs/armor_model）+ diff 明细
    （`armor.model.*` 与配置级 `gun_plates`/`turret_plates`/`*_spaced`）→ `armorComparisonChoices`
    计算每个配置的高亮板件集合。
- `frontend/src/components/version-diff/`
  - `VersionDiffVehicle.vue`（百科同款紧凑身份卡，`compact` 单列 80px 图；`premium`/`collector`
    与百科列表卡同一套车名配色——金币黄、收藏蓝，premium 优先）
  - `ArmorComparisonViewport.vue` + `.test.js`（懒加载 IntersectionObserver、release 校验门、错误重试、
    卸载释放 WebGL/模型；**滑杆整合在画面分割线上**：分割线手柄可拖拽（pointer capture）也支持键盘
    （←/→ 步进 2%，Home=只看新版、End=只看旧版），左右两个版本角标一键切单版；**滚轮缩放模型**
    （OrbitControls enableZoom，min/maxDistance 按模型半径钳制；**默认取景与装甲查看器厚度模式
    同口径**：target = 车体原点上的炮线（`armorPivotGun.z`）、方位 0、距离 ≈ 0.95×模型最长边
    ——用户先后要求"近一半"再"稍微远一些"，现值介于两者之间；**旁注 + 引线由场景内的 2D 叠加画布
    逐帧绘制**（与 WebGL 同帧同步，旋转/缩放零延迟；DOM 只保留版本角标与悬停读数；引线/端点用
    `--color-accent` 高对比色）；**旁注芯片可点击**（pointerup 位移 ≤5px 判定为点击，命中芯片 →
    与文字行点击同一隔离语义；悬停芯片时光标切 pointer）；每块改动装甲面**一条**旁注、只标厚度变化（`旧值 → 新值 mm`，单侧存在标
    `+新值`/`−旧值`，间隙标类别；不带板号），引线连到板件**表面采样点中朝向相机的最近点**；
    **展示炮管与履带**（visual model 总是加载，炮管/炮塔组可见性 = rig `applyConfigVisible`
    同口径：gun_N 组按 N 排序、gun/turret_index 为 0 基组序号——此前拿组号 N 直接比 index
    导致主炮消失；无 mask 隐藏 mask 部分 / 履带按 chassis 厚度着色）；**文字行点选 →
    `isolateKeys` 传入场景 `setIsolated`**，
    隔离数值与查看器 `applyArmorHighlight` 完全一致（选中板 opacity 1，其余装甲板 0.18、
    模块网格 0.15 且 depthWrite off——"几乎透明"），再点或切 pair 复原；隔离激活时非选中板件的
    旁注/引线同步淡出（overlay globalAlpha 0.3，选中板旁注保持全亮）；配置下拉在多炮车上切换对比配置）
  - **文字版去重**：`armorRows` 对「model 级已覆盖的板件」跳过配置级行（`gun_plates` 等在
    armor.model 与配置 diff 各出现一次），并修正 model 级 spaced 行（old/new 是板件 id 列表，
    展开为逐板 新增/移除，不再当厚度值展示）
  - **厚度着色 = 连续渐变**（2026-11-28 用户指令，先改装甲查看器）：`armorThicknessPalette.ts`
    重写为 `THICKNESS_STOPS` 锚点线性插值（30/60/100/200/320mm，语义色同前），<30 标定绿、
    ≥320 深红平段；**绝对域**（同一厚度任意坦克/对比两侧同色），装甲查看器的碰撞/厚度模式
    （collisionView/tankArmor）与本对比器共用同一函数；对比场景的渲染管线也与查看器对齐——
    `useFaceNormals`（非索引化 + 顶点法线）、`MeshStandardMaterial`（metalness 0.4 / roughness 0.6 /
    opacity 0.95）、ACES 色调映射 + 曝光 1.15、同一套半球（主题地面色）/环境/双聚光灯光、
    pixelRatio 上限 2；source 契约测试锁死「查看器厚度着色唯一来源 = 共享调色板」）
  - `ArmorChangeList.vue`（**完整文字版装甲 diff**：六向 + 板件表（含配置级 `gun_plates`/`turret_plates`
    与 `*_spaced` 增删）+ 结构装甲 + 履带；页面装甲章节内嵌与 `ArmorDiffDialog` 弹窗共用同一组件；
    页面内嵌**默认收起**（details「改动明细」，用户指令））
  - `TankDiffDialog.vue`（参数表 + 配置下拉 + 研发/配置增删；装甲标签页仍是六向数值 + 跳转装甲小窗）
  - `ArmorDiffDialog.vue`（表格版装甲明细，从坦克弹窗进入；页面级装甲展示由内嵌对比器承担）
  - `MapDiffDialog.vue`（前后底图滑杆 + 实例计数 chips（场景物件/草地实例 旧→新 与逐名增删）+ 健康值表 + 失败重试）
  - `versionDiffFormat.js` + `.test.js`（`armorRows(armor, detail)` 会把配置级板件/间隙展开成
    `configPlate`/`configSpaced` 行）、`version-diff.css`
- `frontend/src/scene/viewer/`
  - `armorComparisonModel.ts` + `.test.ts`：配置配对（`turret_local/gun_local`）、高亮集合计算
    （厚度变化按版本分侧、spaced 增删、primary 重指、chassis 履带）、`armorCompareWidth`
  - `armorComparisonScene.ts`：GLB 拉取/释放、单版本厚度场景（复用 `armorPlateModel` 的
    tagging/pivots/visibility；板件着色 = 共享连续渐变调色板）、双版本 scissor 渲染、
    OrbitControls（滚轮缩放钳制）、指针拾取、**板件表面采样锚点**（三角形质心 + 法线 ≤48 点/板，
    逐帧取朝向相机的最近点作为引线端点）、2D 叠加画布旁注绘制、`setIsolated` 板件隔离
- `frontend/src/components/tank-catalog-identity.css`（新）：把 `TankCatalogMenu` 的 `tp-media /
  tp-card-name / tp-badges / tp-pill` 身份样式抽成共享文件，`TankCatalogMenu.vue` 与
  `VersionDiffVehicle.vue` 都 `<style scoped src>` 引用；`AgentTankopedia` 的 `tankopedia.css` 仍保留自己
  的一份（历史平行实现，本次未合并）。

修改：

- `frontend/src/app/navigation.js` / `.test.js`：新增 `agent-version-diff`、导航项 `version-diff`（资料组，科技树与地图之间）、`from/to` query 键清理。
- `frontend/src/app/viewRegistry.js`：懒加载 `AgentVersionDiff.vue`。
- `frontend/src/locales/feature-messages.json`：`nav.versionChanges` + `versionDiff.*`（含装甲对比器全部
  文案），zh/en/ru 三语键数完全一致。
- `frontend/src/data-version-wiring.test.js`：允许 `composables/useVersionDiff.js` 显式按对比 dataset 构造
  `/v/<id>/...`（不能走当前选中 dataset 的 `assetProvider`）。
- `frontend/typescript.legacy.json`：把新增 JS/Vue 文件登记进 `p2` 基线。
- `frontend/src/components/TankCatalogMenu.vue`：身份样式迁移到共享 CSS（模板不变）。
- `frontend/src/components/AgentArmorView.viewer-state-contract.test.js`：断言改读共享 CSS。
- `frontend/src/styles/tokens/color-contrast.test.ts`：pill 对比度组合从共享 CSS 提取。
- `android/gradle.properties`：`wotbVersion=5.7.4 → 5.8.0`（按 Version-as-Code minor）。
- `docs/android/release-process.md`：当前源码版本改为 5.8.0。
- `docs/android/releases/5.8.0.md`：新增 release note，含 `/diffs` 发布前置（阻塞项）。

**注意**：`frontend/src/composables/useAuth.ts` 的修改是工作区原有内容，不属于本任务，不要带入提交。

### 1.5 文档

本仓已改：

- `docs/index.md`：2026-11-27 条目（数据同步、工具、diff 结果、发布状态、波及面）。
- `docs/game-data-sources.md` §5.8：`/diffs` 契约、语义规则、发布约束。
- `docs/data-inventory.md`：§2.3 `/diffs` 通道、2026-11-27 的 mechanics1 现状、工具索引。
- `AGENTS.md`：本地 server 第 5 参数、刷新链新增 diff 生成命令。

## 2. 当前运行中的本地服务

- 资产/差异服务：`http://127.0.0.1:8129`
  - 由 `D:\Class\Rust\Project` 根目录启动：
    `node scripts/serve_asset_pack.mjs 8129 release/asset_pack release/datasets release/diffs`
- WotbTools dev server：`http://localhost:5199`
  - 启动命令：
    ```bash
    cd D:\Class\Rust\WotbTools\frontend
    VITE_ASSET_BASE_URL=http://127.0.0.1:8129 npm run dev -- --port 5199 --strictPort
    ```
- 验收 URL：
  - `http://localhost:5199/?view=agent-version-diff&assets=http://127.0.0.1:8129`
  - 指定 pair 示例：
    `http://localhost:5199/?view=agent-version-diff&assets=http://127.0.0.1:8129&from=11.21.0&to=11.21.0_1`

## 3. 验证记录

本仓：

- `python -I tools/test_version_diff.py`：**14/14 PASS**。
- 本地两个 pair 的 `/diffs` summary 均可 200 读取。
- 2026-11-27 重新生成 diff 后计数与上表一致。

WotbTools：

- `npx vitest run`：**603 文件 / 7389 用例通过，2 skipped**。
  - 首轮全量曾出现一次无关的 `AgentArmorView.dom-contract.test.js` 10s 超时；单独重跑 15/15 通过，随后全量重跑也全绿。
- `npm run typecheck`（`vue-tsc --noEmit -p tsconfig.app.json`）：0 错误。
- `npm run build`：成功。
- `npx stylelint "src/components/version-diff/*.css" "src/components/tank-catalog-identity.css"`：通过（含 `corner-shape: round` 配对规则）。
- `npm run test:jscpd`：smoke 通过（jscpd 格式表不含 css，身份样式抽取不会触发重复门禁）。
- 页面/组合式/格式化/装甲对比模型/视口/导航/语言/registry 相关定向测试全部通过。
- **真实数据端到端抽查**：用本机两代 `tank/16257.json`（Outcast）+ diff 明细跑
  `armorComparisonChoices`，能配出配置并把新增炮板 `gun:2` 高亮在新版侧、旧版不高亮，且
  `changes` 里带每侧厚度（标注口径）；临时脚本已删，结论已并入 `armorComparisonModel.test.ts` 的单测覆盖。

## 4. 剩余工作（按顺序）

### 4.1 【需用户明确授权】发布 `/diffs` 到生产资产 origin

**当前未发布**。这是对外操作，执行前必须再次取得用户确认。

发布前先确认（不要凭猜测）：

1. 生产 Caddy 的静态根与 `/diffs` 实际映射（预期在 `/srv/wotb-assets/diffs/`，但必须先看 `Caddyfile` / 主机目录）。
2. CORS：页面从 WotbTools 的 origin 跨域取 `/diffs`，必须带 `Access-Control-Allow-Origin`（现有资产 origin 应已配置，需用 `curl -I -H "Origin: https://wotbtools.com"` 实测）。
3. 两个 dataset 的 `/v/<id>/version.json` 已经是 diff 中记录的 release：
   - `11.21.0` → `11.21.0-20261125-ui1`
   - `11.21.0_1` → `11.21.0_1-20261125-ui1`
   - `11.22.0` → `11.22.0-20261126-mechanics1`

建议发布顺序（`/diffs` 不属于 dataset manifest / 对象库 release 树，不要跑 `asset_release.py stamp`）：

1. 重新生成一遍 `release/diffs`，确保 `index.json` 记录的是当前线上 release。
2. 先上传两个 pair 目录（不可变；目录名已含两边 release id），**不要删除旧 pair**。
3. **最后原子替换 `index.json`**（临时文件 + rename），避免客户端看到尚未上传的 pair。
4. 逐文件回读校验：
   - `https://assets.wotbtools.com/diffs/index.json`
   - 每个 pair 的 `summary.json`、随机 `tanks/<id>.json`、`maps/<id>.json`
   - 与本地 sha256 一致、HTTP 200、CORS 头存在。
5. 用页面实测 release 校验不再告警。
6. 更新文档中的发布状态：
   - `docs/index.md` 2026-11-27 条目
   - `docs/game-data-sources.md` §5.8
   - `docs/data-inventory.md` §2.3
   - WotbTools `docs/android/releases/5.8.0.md` 的“发布前置”改为已完成（并记录发布时间/release id）。

如果没有授权发布，**不要合并 WotbTools**：页面会显示加载失败。

### 4.2 WotbTools 提交 / PR

- 只提交本任务文件；排除 `frontend/src/composables/useAuth.ts`。
- 运行（已通过，提交前再跑一次）：
  ```bash
  cd D:\Class\Rust\WotbTools\frontend
  npx vitest run
  npm run typecheck
  npm run build
  npx stylelint "src/components/version-diff/*.css" --allow-empty-input
  ```
- 如果 `/diffs` 已发布，可直接按正常 PR 流程；如果尚未发布，PR 必须明确标注 **blocked on `/diffs` publish**，不要合并。
- `wotbVersion=5.8.0` 已写入；按 WotbTools 规则，bundled frontend 变更需要该 bump。合入 main 会触发 Android stage/publish，所以顺序必须是：**先发布 `/diffs`，再合并**。
- 若用户决定暂不发布 `/diffs` 且不想要 Android 5.8.0，回退项：
  - `android/gradle.properties` 改回 `5.7.4`；
  - `docs/android/release-process.md` 当前源码版本改回 5.7.4；
  - 删除 `docs/android/releases/5.8.0.md`；
  - 并考虑在 `navigation.js` / `viewFromRoute` 中把该页暂时隐藏/重定向，避免用户进入空页面。

### 4.3 视觉验收（用户）

按仓库规则，**不要由 agent 截图或判定 3D/视觉结果**。把上面的本地 URL 交给用户，明确请用户检查：

- 时间线/下拉切换、总数卡；
- 坦克列表图标/筛选/搜索/国家/等级；
- 坦克弹窗各标签、配置下拉、装甲小窗、旧/新百科深链；
- 地图列表、前后 2D 底图滑杆、可破坏物表、旧/新地图深链；
- 三语切换（zh/en/ru）。

### 4.4 发布后的最终检查

- 公网 `https://assets.wotbtools.com/diffs/index.json` 与本地一致。
- 随机 pair 的 summary/tank/map 文件 200 且 sha256 一致。
- `https://assets.wotbtools.com/v/11.22.0/version.json` 的 release 与 diff 内记录一致。
- WotbTools 页面加载无 releaseMismatch 告警。
- 如果 Android 5.8.0 已发布，APK 内 `/agent-assets/diffs/...` 经 `AgentAssetProxy` 代理到同一 origin，也需抽查一次。

## 5. 关键坑与注意事项

1. **不要把炮名拼回 `configs.<gun>.<field>` 路径**：炮名含点（`7,5 cm Kw.K. L/100`），旧解析会产生错误字段。配置 chip 用 `changePath` + `config`；坦克弹窗用 `row.configLabel` 单独渲染。
2. **地图不能按 `variantKey` 分组**：11.22 变体目录 `variantKey` 自指，会把 43 个 key 当成新增地图。必须读仓库 `map_index.json` 的 `space`。
3. **配置本地 ID 优先**：`turret_name` 为空，按名字配对会产生大量 unpaired；`(turret_local, gun_local)` 是唯一可靠键。
4. **schema 桶不是变更**：`mechanics:187`、`stats.clip:249` 是单侧字段；不要把它们展开成 187/249 辆“玩法改动”。
5. **`/diffs` 不是 dataset manifest 内容**：不要跑 `asset_release.py stamp`，不要加进 pack，不要进对象库 release 树。
6. **`index.json` 最后写**：先传 pair 目录，再原子替换 index，避免竞态。
7. **WotbTools 的 `data-version-wiring` 规则**：版本对比必须按目标 dataset 显式构造 URL，不能走当前选中 dataset 的 `assetProvider`；允许名单已加 `composables/useVersionDiff.js`，不要扩大到组件层。
8. **TypeScript 基线**：新增 JS/Vue 必须登记到 `typescript.legacy.json` 的 `p2`，否则 `typescript-ownership.test.ts` 失败。
9. **CSS 规则**：`border-radius: var(--radius-full)` 必须紧跟 `corner-shape: round`；媒体查询用 `@media (width < 768px)`；禁止 hex/命名颜色与裸 `z-index`。
10. **不要动 `frontend/src/composables/useAuth.ts`**：它是工作区原有改动，与本任务无关。
11. **本地 preview dataset**：`11.4.0.138` 目录的 `version.json.dataset_id` 是 `11.4.0.138-r5`，目录名不一致；生成 catalog 时曾用临时 junction 根绕过，当前 `release/versions.json` 已手工合并好，不要随意重新 catalog 覆盖掉预览别名。
12. **全量测试偶发超时**：`AgentArmorView.dom-contract.test.js` 在并行负载下可能 10s 超时；单独重跑可过，不要误判为本次改动回归。
13. **身份样式已抽共享**：`TankCatalogMenu.vue` 与 `version-diff/VersionDiffVehicle.vue` 都 `<style scoped src="../tank-catalog-identity.css">`；`AgentArmorView.viewer-state-contract.test.js` 与 `styles/tokens/color-contrast.test.ts` 的断言读的是这个共享文件。不要把 `tp-*` 样式搬回组件内，否则这两处测试会挂。`AgentTankopedia` 的 `tankopedia.css` 仍保留自己的一份 `tp-*`（历史平行实现），本次刻意未合并。金币/收藏车名配色也在共享文件里（`.vd-identity.is-premium/.is-collector`，与百科列表卡 `.tp-card.is-*` 同一套 color-mix 口径）；身份字段来自 diff summary 的 `premium`/`collector`，不要在前端从名字猜身份。
14. **装甲对比器的模型 URL 有白名单**：`datasetTankModelUrl` 只接受 `/glb/<数字>/(collision|model).glb`；车体网格（履带）走 `visual_model_url`，且仅当 diff 高亮里有 `chassis:leftTrack/rightTrack` 时才加载。
15. **装甲高亮/旁注的归属规则**：车体（hull）板件变化对所有配置生效；炮塔/火炮板件变化只有单配置车才从 `armor.model` 归属，多配置车一律走配置级 `gun_plates`/`turret_plates`/`*_spaced` diff（fail-closed，避免把改动标到错误配置上）。只有六向 `armor.summary` 变化而没有板件级变化的车，对比器会正常渲染但不高亮（显示"此配置没有可高亮的板件"）——这是有意的，不要从六向数值猜板件。旁注**每块板一条、同时带前后值**（`changes` 的 `left/right` + `kind` thickness/added/removed/primary/spaced/chassis 决定文案；同一块板多类改动命中时保留语义更强的 kind：thickness > added/removed > primary/chassis > spaced）；引线端点在每帧对板件做射线取相机可见表面命中点——不要改回 bbox 中心（会埋进模型体积里，看起来"没附着"）。
16. **条目名必须走百科参数键**：diff 行标签统一经 `versionDiffFormat.js` 的 `diffPathLabel` → `fieldLabel`，映射到坦克百科参数面板同一批 `agentTanks.params.*` 键（装填时间/每分钟伤害/平均穿透…，三语齐全）。配置级 `change.path` 是**裸字段**（`reload_time`、`dpm`、`aim_time`、`standard_damage`、`weight`…），`diffPathLabel` 末尾必须落回 `fieldLabel`，不能 `return path`（曾因此整批配置条目回显代码字段）；**研究子树**（`research.gun:<名>.stats.reloadTime`、`hull.hp`、`bbox.*`、`invisibility.moving`、`tags`）由 `researchPathLabel` 拆"模块身份 · 条目名"。新增 diff 路径类型时补映射并加单测，否则会原样显示路径。
17. **每辆装甲车一个 WebGL 上下文**：`ArmorComparisonViewport` 用 IntersectionObserver（rootMargin 200px）懒挂载并在离屏/卸载时销毁；页面上装甲车数量增加时留意浏览器 WebGL 上下文上限。滚轮缩放开启后，光标悬停在视口上时滚轮不再滚动页面（与装甲查看器主场景一致）。

## 6. 关键文件索引

本仓：

- `tools/version_diff.py` — diff 引擎/CLI
- `tools/test_version_diff.py` — 15 例回归
- `data/version_lineage.json` — 主线链（不要猜前驱）
- `scripts/serve_asset_pack.mjs` — `/diffs` 本地伺服
- `docs/game-data-sources.md` §5.8 — 契约权威
- `docs/data-inventory.md` §2.3 / §6.4 — 通道与工具索引
- `docs/index.md` 2026-11-27 条目 — 状态与波及面
- `release/diffs/` — 生成物（gitignored）

WotbTools：

- `frontend/src/composables/useVersionDiff.js` — catalog/URL/fetch（含 `datasetTankModelUrl` 守卫）
- `frontend/src/components/AgentVersionDiff.vue` — 页面（变更日志式版面）
- `frontend/src/components/version-diff/VersionDiffVehicle.vue` — 百科同款紧凑身份卡
- `frontend/src/components/version-diff/ArmorComparisonViewport.vue` — 内嵌双版本装甲厚度对比器
- `frontend/src/scene/viewer/armorComparisonModel.ts` — 配置配对与高亮集合（纯函数，有单测）
- `frontend/src/scene/viewer/armorComparisonScene.ts` — WebGL 双版本渲染（scissor 分割 / 厚度着色 / 轮廓高亮）
- `frontend/src/components/version-diff/TankDiffDialog.vue` — 坦克参数弹窗
- `frontend/src/components/version-diff/ArmorDiffDialog.vue` — 装甲表格明细（从坦克弹窗进入）
- `frontend/src/components/version-diff/MapDiffDialog.vue` — 地图对比
- `frontend/src/components/version-diff/versionDiffFormat.js` — 纯展示辅助
- `frontend/src/components/tank-catalog-identity.css` — `tp-*` 身份样式共享文件
- `frontend/src/app/navigation.js` / `viewRegistry.js` — 路由与导航
- `frontend/src/locales/feature-messages.json` — 三语文案
- `docs/android/releases/5.8.0.md` — Android release note（含 `/diffs` 发布阻塞项）