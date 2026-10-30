# 录像者相机能力（`parseRecorderCamera`）

> 状态：v0.4.21 additive 能力，与其余五个 WASM 入口相互独立。
> 实现：`crates/replay-core/src/replay/recorder_camera.rs`；WASM 入口：`crates/replay-wasm/src/lib.rs`。

## 1. 能力定义

```text
parseRecorderCamera(bytes) -> JSON object | null
```

```json
{
  "version": 1,
  "author_eid": 1961229,
  "aim": { "t": [], "yaw": [], "pitch": [], "ray_x": [], "ray_y": [], "ray_z": [] },
  "mode": { "t": [], "raw": [] },
  "zoom": { "t": [], "raw": [] },
  "bloom": { "t": [], "raw": [] },
  "client_options": { "backward_zoom": null }
}
```

所有 `t` 都是原始 replay clock（秒），与 `pose_kf` 同一时钟；同组各列等长，按时钟升序。

| 字段 | 来源 | 语义 |
|---|---|---|
| `aim.yaw` | Type39 f0 | 世界系瞄准 yaw，弧度，已解卷绕；`yaw = atan2(dx, dz)`（游戏系） |
| `aim.pitch` | Type39 f1 取负 | 世界系瞄准 pitch，弧度，正为仰角 |
| `aim.ray_*` | Type39 f2..f4 | 瞄准射线上一点，原始 replay 世界坐标。不是弹着点，也不是目标位置 |
| `mode.raw` | Type23 | u32 无损。`raw & 0x7fffffff`：1 = ARCADE，0 = SNIPER。高位语义未闭合，属于开镜族 |
| `zoom.raw` | Type31 | 开镜缩放 raw 值，越小越深。不是 FOV 度数，也不是倍率 |
| `bloom.raw` | method36 field6.field1 | 动态扩散 bloom，稀疏样本 |
| `backward_zoom` | method49 | 见 §3 |

## 2. 消费规则

- Type39 在作者死亡或观战切换后不再代表原车视角，消费方必须用死亡时间门控。
- `zoom` 只能在同一开镜 session 内取最近样本。禁止线性插值，禁止跨 session。
- `zoom` 形态不唯一：多数回放约 120Hz 连续，`11.20.0` 非 `_china` 样本每 session 只有 2–8 个样本，集中在 27 / 16.88 / 6.75。这些稀疏样本里含 `SmartZoom=false` 分段，成因尚未闭合。
- `bloom` 中位间隔 1.3–3.2 s，集中在开火前后，不能当作连续缩圈曲线。
- 全程未开镜的回放是合法输入：`zoom` 为空，`mode` 只有默认值。

## 3. `BackwardZoom`

method49 的选项按设备形态分段（`Desktop` / `Tablet` / `Phone` / `MiniTablet`），回放里没有「本场生效分段」的标记。因此只在所有含该键的分段取值一致时输出 bool；分段不一致、键缺失或没有 method49 时输出 `null`。

## 4. 结构校验

下列任一情况返回 `null`，不按版本白名单放行，也不做数值分布判断：

- 作者车辆 eid 无法解析；
- Type23 缺失、长度不是 4、首值不是 1、低 31 位不在 {0,1}，或两个非默认值直接相邻；
- Type31 长度不是 4、值非有限，或落在开镜 session 外（容差 20 ms）；
- Type39 为空或长度不是 28。

## 5. 抽稀

`aim` 用多列 Ramer–Douglas–Peucker：角度容差 0.02°，射线点容差 2 m。`zoom`、`mode`、`bloom` 不抽稀。

## 6. 证据范围

46 份本地回放零拒绝：11.18.0_china_apple、11.19.0_china / china_apple、11.20.0 / 11.20.0_china。入库样本的 Type23/Type31 计数由 `crates/replay-core/tests/recorder_camera.rs` 与独立分帧脚本结果锁定。全量语料核对：

```bash
WOTB_REPLAY_CORPUS='<目录1>;<目录2>' cargo test -p wotb-replay-core --release --test recorder_camera -- --include-ignored --nocapture
```

单场 JSON 最大约 560 KiB，原生解析约 40 ms。
