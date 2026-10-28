# 录像者初始世界位姿能力（`parseAuthorInitialPosition`）

> 状态：v0.4.15 additive 能力。属于 Agent 公开 WASM 面，但与
> `parseResult` / `parsePlayback` / `parseShotReplays` / `parseAiReview`
> 相互独立。契约实现：`crates/replay-core/src/replay/author.rs`；
> WASM 入口：`crates/replay-wasm/src/lib.rs`。

## 1. 能力定义

从 `.wotbreplay` 中取得**录像者车辆的第一条可信世界坐标观测**：

```text
parseAuthorInitialPosition(bytes) -> JSON object | null
```

返回：

```json
{
  "author_eid": 1961229,
  "clock": 0.41081899404525757,
  "position": [-138.75604248046875, 31.585731506347656, -165.3030242919922]
}
```

字段语义：

| 字段 | 含义 |
|---|---|
| `author_eid` | 包流中解析出的录像者**战斗车辆** entity ID（Type5 class=2 车辆的协议 eid） |
| `clock` | 被选中 type=10 采样的原始 replay clock（秒） |
| `position` | 原始 replay 世界坐标 `[x, y, z]`；`x/z` 为水平面，`y` 为高程 |

无可靠证据时返回 JSON `null`。**禁止**用 `[0,0,0]`、平均值、地图资源或文件名猜值。

## 2. 语义边界：观测，不是 spawn point

`initial position` 的严格定义是：

> replay 中录像者车辆的第一条可信 world-space position observation。

它不是地图 spawn point 的 bit-exact 坐标，原因包括首包到达时车辆已有微小移动、
包头 clock 是录制端到达时间、网络/记录时序等。消费方应把它当作
**最早观测位置**，并在自己的地图语义层做 nearest spawn cluster 分类。

Agent 只输出 **what the replay says**：

- 不输出 `NORTH` / `SOUTH` / `WEST` / `EAST`；
- 不输出 spawn A/B、出生侧、占领点或统计语义；
- 不做 replay 坐标 → 地图坐标转换（该变换属于 WotbTools）；
- 不根据 team / 文件名 / 地图资源推断位置。

## 3. 证据规则

### 3.1 身份

作者车辆 EID 解析与 `ReplayModel::scan` 共用同一函数（SSOT）：

1. ARENA_INFO comp blob 的 `account_id` 精确匹配（重名 / 匿名免疫）；
2. 未命中时退回既有 Type5 昵称精确解析。

作者账号来自 battle_results；昵称优先 meta.json `playerName`，缺失时退回
battle_results 花名册昵称。battle_results 缺失的回放仍可走 meta 昵称回退；
两面证据都没有 → `null`。

### 3.2 位姿样本

只接受同时满足以下条件的 type=10 包：

```text
payload 长度 >= 48B（共享解码器阈值）
packet eid == author_eid
payload[8..12] parent entity ID == 0（直接世界坐标；挂接实体不得当世界原点）
position 三轴 finite
clock finite
```

在全部合格样本中选择 replay clock 最小者（同钟保持包流首条）。
不取车辆平均值、不插值、不推算 spawn center。

> 坐标轴：Agent replay 世界坐标水平面为 `x, z`，`y` 为 elevation。
> WotbTools 地图坐标使用 `x, y` 水平面，`z` 为 elevation；转换在消费方完成。

## 4. 失败模式

| 情况 | 输出 |
|---|---|
| 作者 EID 未解析 | `null` |
| 作者无 type=10 采样 | `null` |
| 候选全部为 parent != 0 / NaN / inf / 截断载荷 | `null` |
| 未结算回放（battle_results 缺失），但已出现合格观测 | 正常返回该观测 |
| 包流结构性损坏 | capability 级错误（不静默伪造） |

未知永远不是 `[0,0,0]`。回放客户端版本兼容性沿用现有包解析器规则，
本能力不单独放行/拒绝版本。

## 5. 性能与架构边界

该能力**不构建**：

- `ReplayModel` / `PlaybackData`；
- 0.1s 位姿网格 / 滤波时间线；
- shot extraction / HP timeline / AiReviewFacet。

实现路径：

```text
open replay
  -> read meta + battle_results（身份）
  -> frame packets
  -> resolve author vehicle eid
  -> scan type=10 for that eid, keep earliest world sample
  -> serialize 3-field JSON
```

包流分帧仍复用 `replay::packets` SSOT；type=10 字段解码复用
`combat::indexes::decode_st10_sample`，不新增第三份 type=10 decoder。
目标是 O(packet scan)、小分配、tiny output，适合批量 replay 的二次轻量 probe。

## 6. 与 `parseResult` 的关系

`parseResult` 保持 settlement/meta-only：

- 不读 packet stream；
- 不建时序模型；
- `BattleSummary` 不新增本能力字段。

消费方按需组合：

```text
const result = parseResult(bytes)
if (eligibleForCollector(result)) {
  const initial = parseAuthorInitialPosition(bytes)
}
```

## 7. 回归与真实证据

- 合成单测：身份隔离、earliest clock、parent/NaN/short 样本跳过、未解析/无位姿 → null；
- 真实夹具 `data/replay_samples/2.wotbreplay`：作者 eid、clock 与坐标锁定到既有调查值
  的小容差；并与同场 Type5 战斗车辆物化位姿（独立证据源）交叉核对 < 0.05m；
- 全部真实夹具：返回值 finite，且与 Type5 物化位姿 < 0.1m（有 Type5 证据时）；
- 核心测试：unavailable → JSON `null`（不伪造 `[0,0,0]`）；
- WASM smoke：JSON shape、`parseResult` 不新增字段/时序键。

## 8. 版本与回滚

v0.4.15 起随 `wotb-replay-wasm-v<version>.zip` 发布。纯 additive：

- 既有 `parseResult` / `parsePlayback` / `parseShotReplays` / `parseAiReview` 不变；
- 回滚只需消费方 pin 回上一版 Agent release；
- 无存储数据/协议迁移。
