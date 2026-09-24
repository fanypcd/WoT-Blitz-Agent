# WI（wotinspector）射击参数与命中位置分析

> 2026-09-24。方法：直接抓取 wotinspector 前端资源与公开 API 逐层分析
> （replays 站 React bundle → battle.json → Map Inspector WASM → shotsimulate blob 解码），
> 与本项目 combat.rs 提取链逐字段对照。
> 证据分级：**[实证]** = 抓包/字节解码直接证据；**[推断]** = 由字符串/架构证据推出，待函数级验证。
> 原始产物存 `tmp_wi_js/`（bundle、battle.json、wasm、wasm_strings.txt，未入库）。

---

## 一、总体架构 [实证]

WI 的"3D 查看器"不是网页 JS 渲染，而是**整个 Emscripten WASM 原生应用**
（Map Inspector，C++ 编译，自带 WebGL 渲染器）：

```
replays.wotinspector.com/en/blitz/view/<hash>/      ← React 摘要页（射击表格/战绩）
   ├─ battleDetailsUrl  api.wotinspector.com/v2/battles/<arena_unique_id>/?platform=blitz
   │     → 服务器预解析 JSON：shots 表 + chat（§二）
   ├─ "Watch online" → iframe: https://map.wotinspector.com/en/blitz11.18/
   │     ?url=<replay download_url>&frame&time=<t>
   │     → WASM (wotinspector-static.../mi/webapp1.4.0-3-blitz/map-inspector.wasm, 7.5MB)
   │        自行下载完整 .wotbreplay 并解析、加载游戏数据包后本地渲染（§三）
   └─ shotsimulate    api.wotinspector.com/v2/ai/shotsimulate/（装甲查看器复现弹道，§四）
```

关键结论：**网页 shots 表与 3D 查看器是两套数据流**。表格是服务器预解析的统计 JSON；
3D 查看器拿到的是**原始回放文件**（`--setting arg.replayUrl <download_url>`），在浏览器内
用 WASM 解析包流。因此查看器可用的射击参数 ⊇ 本项目 combat.rs 能提取的全部字段。

## 二、网页 shots 表（battle.json）字段清单 [实证]

`/v2/battles/<id>/` 每发命中弹一条（脱靶弹无记录）：

| 字段 | 实测样本 | 与本项目对照 |
|---|---|---|
| time / shooter / target | 36.32 / 4985030 / 4985027 | fire 时刻 + eid 配对（同源） |
| has_damage | true | method1 血量链归属 |
| shell_id | 1098 | type=32 segment u24 / 项目 `shell_id`（已 7/7 对齐） |
| turret_yaw / gun_pitch | −0.362 / +0.0737 rad | prop2 @命中通知时刻（项目 `target_turret_yaw/gun_pitch`，已对齐） |
| **segment** | "16748499983640690947"（u64 十进制串） | **[result u8][layer u8][hash6 6B]**（见下） |
| distance | 41.580047 m | 项目已 99/99 μ 级复现 |
| nominal_pen / nominal_damage | 170 / 135 | item_defs 弹种表 |
| damage / hit_flags / crit_modules / destroyed_modules | 130 / 0 / 0 / 0 | method38 位图（项目已对齐） |
| is_premium_ammo / ammo_type | true / 2 | type=28 + 弹种表 |

segment u64 小端解码（8 发实测）：`b0=result(0..4)`、`b1=layer(1..3)`、
`b2..b7 = hash6`（与 method8 hash6 同值）。
**layer** = 受击装甲层序号（间隙/多层装甲的第几层），与 type=32 的 B7=plateId 互补。
**整表没有任何命中坐标字段**——WI 与本项目收到的是同一份无坐标协议。

## 三、3D 查看器（WASM）内部 [实证字符串 + 架构推断]

map-inspector.wasm 解包字符串（wasm_strings.txt，13232 条唯一串）显示它内嵌：

- **游戏 BW 协议绑定**：`UpdateArenaGenerated.VehicleInfo.StuffAvatarEntry` 等——
  与项目 type=10/prop2 同源的实体位置/姿态流解析；
- **自有 Arena 事件流**（protobuf）：`ArenaTracerStartMsg` / `ArenaVehicleShotMsg` /
  `ArenaTracerStopMsg` / `ArenaExplodeProjectileMsg` / `ArenaVehicleGunAnglesMsg` /
  `ArenaEntityMoveMsg` / `ArenaVehicleHealthChangedMsg`——发射/终点/爆炸/炮塔角/移动；
- **完整装甲穿透模拟**：`piercingPower`、`piercingPowerLossFactorByDistance`、
  `getPiercingChance`、`armourPiercingFactor`、`primary_armor`/`damage_armor`、
  `use_armor_homogenization`、`ricochetAngle`、`isHEShell`；
- **碰撞模型与射线求交**：`hitTester`、`hitTester/collisionModelClient`、
  `protobuf.HitTester.modelFile`、模型路径
  `vehicles/{nation}/{tank}/collision_client/{gun}.model`、
  `vehicles/{}/{}/collision/{}.model`、`vehicles/{}/{}/{}/lod0/{gun}.model`
  （即游戏 `3d/Tanks/CollisionMeshes` 同源几何，项目已解格式、标注"可自建 HitTester"）；
- **弹道行为设置**：`continue_trace_if_no_hit`（未命中则示踪线继续延伸）、
  `damage_distance_limit`、`use_hit_angle`、`chance_to_hit_by_projectile/explosion`；
- **表现资源**：`sfx/tracer.cross`、`sfx/tracer.trail`、`sfx/ground_hit.ps`（弹着特效）。

**命中位置判定模型** [推断，证据链完整]：查看器不消费任何坐标型命中数据（协议中不存在），
它把发射参数（method29 launchPoint/launchVelocity 同源）沿弹道推进
（`continue_trace_if_no_hit`），在弹道与目标碰撞模型的交点放置弹着/特效，
再对交点做穿透模拟上色——**即本项目"红点"（弹道射线 × 部件约束）语义，
与游戏客户端第六轮逆向结论一致**。项目已复现同构管线（filter 时间线 + 部件盒求交）。

## 四、shotsimulate：解析器向 armor 查看器传递的信息 [实证]

3D 回放查看器（或摘要页）点开一发弹做装甲分析时，向
`GET /v2/ai/shotsimulate/` 发请求，模板在 map-inspector.wasm 内明文：

```
pkg={}&target.vehicle={}&target.chassis={}&target.turret={}&target.gun={}&
target.turret.yaw={}&target.gun.pitch={}&shooter.vehicle={}&shooter.chassis={}&
shooter.turret={}&shooter.gun={}&shooter.shell={}&segment={}&distance={}
```

实测返回 armor.wotinspector.com 的 URL，其 `shot=` 参数为 base64url 的
**58 字节 shot blob**（armor 端经 `--setting arg.shot <blob>` 注入 WASM）。
用可控变量探针（逐字段替换观测）钉死的精确布局：

```
偏移   类型  字段                     （query 参数名）
[0]    u8    =0x02（版本/魔数）
[1]    u8    =0x01（恒定，语义未定）
[2..6)  u32  shooter.vehicle          shooter.vehicle
[6..10) u32  shooter.chassis 模块id    shooter.chassis
[10..14) u32 shooter.turret  模块id    shooter.turret
[14..18) u32 shooter.gun     模块id    shooter.gun
[18..22) u32 shooter.shell 全局弹种id  shooter.shell
[22..26) u32 target.vehicle            target.vehicle
[26..30) u32 target.chassis 模块id     target.chassis
[30..34) u32 target.turret  模块id     target.turret
[34..38) u32 target.gun     模块id     target.gun
[38..42) f32 target.turret.yaw (rad)   target.turret.yaw
[42..46) f32 target.gun.pitch (rad)    target.gun.pitch
[46..54) u64  segment 原样（[result][layer][hash6]）  segment
[54..58) f32 distance (m)              distance
```

探针记录：shooter→blob[2..22)、target→blob[22..38) 两块**严格按上述序**逐字段透传
（11/11 字段、5/5 f32 全部命中预期值）；segment@46 与 distance@54 原样回传。
**没有传递**：命中坐标、发射点/弹速、目标世界位置与车体朝向、任何时间线。

armor 查看器（`ai/webapp4.5.0/armor-inspector.wasm`，7.9MB）消费这些信息的方式
（字符串实证）：收到 blob 后设置 `scene.shotSimulatePlayerId/TargetId/TargetAngles/
Distance` 系列状态，调用 `simulateShot`（应用内嵌 Lua/Sol2 脚本函数名），对目标
`hitTester/collisionModelClient` 模型（`vehicles/{nation}/{tank}/collision_client/
{gun}.model`，模块 id 决定炮塔/炮管变体的装甲模型）做射线求交：
**来向 yaw/pitch（hash6 内）× distance 反推射线原点 → 求交得精确弹着点 →
layer（b1）选装甲层 → 穿透模拟上色**。辅助能力：`retraceLastShot`/
`resetCameraToLastShot`（相机重放到弹道）。

## 五、真实样本解码（T110E5 实战弹）与两处文档修正 [实证]

用户提供的实战链接（重放 7c6f1c4d…，10785 T110E5 打 T110E5）的 shot blob 解码：

```
shooter: vehicle=10785 chassis=22562 turret=19235 gun=271396 shell=537898(0x8352a = 局部2101|美国基数0x2a)
target : vehicle=10785 chassis=22562 turret=19235 gun=271396
target.turret.yaw = 44.296875 度   ← 单位是度！（44.296875 = 126 × 0.3515625，正是 prop2
                                      粗10位每步 360°/1024 的精确整数倍 → 来源即 prop2）
target.gun.pitch  = 2.158729 度    ← 连续值，即 prop2 frac6 按车型极限解码的结果
segment = 00 02 41 5a ff 41 77 00 → result=0（未造成伤害）、layer=2、hash6=41 5a ff 41 77 00
distance = 91.208 m
```

**修正 1：hash6 的三个 u16 是大端序**，不是小端。LE 解读给出 pitch=+89.7°（物理不可能）；
BE 解读：yaw=0xff41 → (65345−32768)/32768×180° = **+178.95°**（来向方位角），
pitch=0x7700 → (30464−32768)/32768×90° = **−6.33°**（抵达垂直角，91m 步战车交战完全合理）。
项目文档 §4.1 的解码公式角度系数正确但字节序需按 BE 复核（type=32 的 B5B6 本就证实过 BE）。

**修正 2：重放页拼接 armor 链接是浏览器端行为**（replays bundle 内 JS 明文）：
`turret_yaw/π×180`（弧度→度）、`chassis_id||-1`（花名册 players_data 的模块 id）、
`distance>0?distance:400`（兜底 400m）、`BigInt(segment)`，点击后 `window.open`。

## 六、"平行射线族"问题的消解：blob 字段集反推锚点约定（已被 §七 修订，存档）

> **2026-09-24 两轮修订**：本节的"车体局部系"参考系错误——hash6 方向实为**炮塔系**
> （§七终版），且 shotsimulate 会重编码 segment。核心构造（方向+姿态+distance → 模型
> raycast）方向正确，参考系修正为炮塔系。下文保留推导存档。

用户正确指出：只有方向+距离不能定位射线——缺锚点。但 blob 的字段集本身锁死了答案：

1. blob 里**没有**目标世界坐标、**没有**目标车体朝向（hull yaw）；
2. blob 里只有 `target.turret.yaw`（炮塔相对车体！）与 `target.gun.pitch`（相对炮塔）；
3. armor 查看器是单车场景：目标模型摆在本体局部系原点，唯一合理的构造是
   **一切在目标车体局部系中进行**。

即 WI 的构造为：目标车体前向 = 局部 +Z，模型（含履带/车体）不动、炮塔按
turret.yaw、炮管按 gun.pitch 旋转到位；**hash6 的来向 yaw 就是相对目标车体的方位角**
（若是世界系方位角，缺少 hull yaw 的 armor 查看器根本无法还原射线——字段集本身排除了
这种可能），pitch 为抵达垂直角；射线原点 = 目标位置锚点（局部原点）沿来向方位
退距离 distance；射线方向指向目标，对 `hitTester/collision_client` 模型求交得到弹着点。
"无数条平行射线"由两个约定钉死：**锚点 = 目标位置锚点（局部原点）；方位角基准 = 目标车体朝向**。

可证伪预测（供在 WI 查看器中目检）：该弹来向方位 +178.95° ≈ 正对目标车体**正后方**、
抵达角 −6.3°（射手高约 91×sin6.3°≈10m，打坡下目标）、result=0 显示为未击穿/弹开图标、
layer=2 为第二装甲层。若 WI 页面上弹着标记出现在目标车体后方部件而非前方，即证实
"车体相对系"构造。

对项目的两点含义：
- hash6 方向是**受击车体系**量——弹着点复现应把射线先变换到目标局部系再与部件盒求交
  （现行判定层锚点若在世界系直接求交，对偏车体朝向的样本会引入系统性偏差）；
- WI 全链同样没有精确弹着坐标，armor 端的"精确弹孔"= 目标局部系几何重建 + 模型 raycast，
  物理上限依旧是碰撞模型精度（CollisionMeshes 路线）。

## 七、2026-09-24 深挖：hash6 参考系破解（炮塔系来向）+ WI 服务端改写 segment [实证]

拿到用户样本的**原始回放**（replays 站公开下载，ZIP 容器，无需登录）后，
用项目解析器（`examples/wi_align_probe.rs`，新增）对 11 发直击弹做了
hash6 原始字节 × 几何真值（method29 炮口、method8 通知状态锚点、type=10 车体角、
launch_velocity 发射俯仰）的系统对照：

1. **`hash6 = [shell u16][来向 yaw u16][抵达 pitch u16]` 的世界系/车体系解读不成立，
   但 [2..4] 确为方向字节——参考系是受击者炮塔系**（2026-09-24 深挖终版）：
   - 字节对 [2..4] 在 11 发弹里"恒为 0xFF41/0xFF44/…/0xF92D"的表象，原因是
     **炮塔始终瞄着射手**：按受击者炮塔系计算来向方位 tframe = worldB − (hullYaw+turretRel)
     后，其反向（弹道行进方向）与 hash6 解码值 [BE u16, (u16−32768)/32768×180°] 对比：
     **8/11 发误差 ≤2.8°（中位 −0.35°）**；3 个离群（−20.7/−8.6/−8.4°）分别对应跳弹弹
     （result=4，特效方向可能为弹开方向）与车体姿态/锚点时序偏差；
   - 即旧文档的"来向 yaw"字段真实存在，但**参考系 = 受击者炮塔**（非世界/车体），
     字节序 = 大端，比例尺 = (u16−32768)/32768×180°（半圆）。旧 J39 校验的偶合成功
     与本样本"恒 0xFFxx"的表象由同一机制解释；
   - [4..6]（"抵达 pitch"）语义仍未定：与几何抵达角的炮塔系/世界系差值均差 10°~30°
     （疑似零点含车体 pitch/roll 姿态，本样本目标始终在坡地）；
   - hash6[0..2] 对同一弹种（537898）取不同值（415a vs 3bb3）→ 不是弹种（旧解作废）。
2. **游戏 type=32 尾 8 字节的项目语义同样存疑**：本样本实测尾 8 字节
   `02 3d ff 00 08 35 2a 01`，shell 537898=0x08352A 以 **大端 u24** 出现在 [4..7)、
   result 在 [7]；与项目文档 `[result][shell u24 LE][00][X][Y][Z]` 的位置/序均不符。
3. **WI 服务端改写 segment**：battle.json 的 segment（=[result][layer][method8 hash6]，
   hash6 与本探针直读 method8 逐字节一致 ✓）与 shotsimulate 返回 blob 里的 segment 不同：
   `01 02 41 5a ff 41 61 c2 → 00 02 41 5a ff 41 77 00`（result 1→0、末字节对 61c2→7700）。
   即 shotsimulate 不是透传——服务端按自己的装甲模拟结果**重新编码** segment 再交给
   armor 查看器。armor 端展示的弹着 = WI 自己模拟管线的输出，不是游戏弹着的重建。

**结论修订（终版，2026-09-24）**：hash6[2..4] = 受击者炮塔系弹道来向方位角（BE、半圆
比例尺）+ blob 的 target.turret.yaw/gun.pitch（目标姿态）+ distance → **弹道射线相对
目标模型完全确定**，armor 查看器据此对碰撞模型 raycast 即得精确弹着点（用户实测
"WI 能准确给出命中位置"与此一致）。WI 服务端 shotsimulate 另行重编码 segment（模拟
方向微调，61c2→7700 等）。此前"WI 只能重打模拟弹、不还原游戏弹着"的中间结论作废。
对项目的直接启示：弹着复现应采用 **炮塔系来向 + 姿态角 → 局部系射线 → 部件盒求交**
的 WI 同构管线；hash6 的 [2..4] 字节是比弹道反解更贴近服务器判定时刻的来向真值
（服务器在命中判定时刻编码，不受本端滤波/时序误差影响）。

### §七补4（取样时刻考古：method8 流序快照，21/21 双锁定）[实证]

用 [`examples/wi_pose_timing_probe.rs`]（新增）把 battle.json 的 turret_yaw/distance 与回放内
受击方 prop2 / type10 流在候选时刻逐一对照（用户提供回放，21 发命中弹）：

1. **时刻 = method8 直击通知包在包流中出现的时刻**。本回放 method29 与 method8 同钟
   （服务器 hitscan：开火即判定即通知），两个候选在本样本不可区分——但下面的流序证据
   本身锁定的是"处理 method8 时的状态"。
2. **取样语义 = 单遍流式解析器的当前实体状态**（流序，非时钟序）：
   - 距离 = method8 包索引之前**流序最后已知**的双方 type10 位置欧氏距离：
     **21/21 μ 级**（|ΔD| ≤ 8e-6 m）；项目 combat 管线（同样流序锚定）11/11 μ 级复验；
   - 炮塔角 = 同一流序最后已知受击者 prop2 的 coarse10（量化 0.3515625°）：
     **21/21 精确匹配**（如 44.296875° = 638/1024）；
   - 炮管俯仰 = 同一 prop2 的 frac6 按扇区极限解码：11 发中 8 发与项目解码**精确同值、
     符号相反**（WI 正值 = 下压/俯角，项目正 = 仰角；如 frac63 → WI +8.000° = 项目 −8.000°）；
     3 发差异（176.07/267.96/285.76）为扇区过渡区极限数据细节（models.pb vs WI 服务器表），
     非取样时刻问题；
   - **hull 车体角完全不取样**——blob/armor 查看器中车体恒默认朝向，这就是 WI 姿态
     只有炮塔/炮管两个角的原因。
3. **时钟序 vs 流序的对照实验**（关键方法学结论）：按时钟排序取"≤t8 最后采样"仅
   8/21 匹配；按包序取"method8 之前最后采样"21/21——**同 tick 内 type10/prop2 与
   method8 的包序位置决定取值**，任何复现实现必须用流序。
4. prop2 采样钟滞后通知钟 0.09~0.33s（变化驱动广播的自然节流，即"最后已知"的年龄）；
   WI 时间戳 = 回放钟 − 8.777s（战斗开始锚点，21 发恒定）。
5. 对项目的落点：combat.rs 现行"method8 通知状态锚点"与 WI 完全同构
   （本回放 11/11 μ 级距离复现再次验证）；唯一可补的是 gun_pitch 符号约定对齐
   （WI = 压角为正）与扇区过渡区表细节。
6. **修复已落地（同日）**：combat.rs 姿态快照改用 method8/29 流序 prop2 快照
   （`DirectHit8::victim_prop2` / `LaunchEntry::shooter_prop2`，时钟序回退保留）。
   修复后本回放对照：**炮塔角 11/11 逐位（0 coarse 步）、俯仰 11/11 逐位反号**
   （此前 3 发"扇区差异"实为扇区选择的偏航取样差异，随流序修复一并消失）、
   距离 μ 级保持；cargo test 37/37、存量 4 样本回放发数不变。

### §七补3（终终结案：差分实验破译 blob[48..50) = 弹道方向，游戏两点编码复用）

对运行中的查看器做**输入篡改差分实验**（改 blob 单字段 → 重载 → Lua 读 C++ trace 射线）：

| 变体 | blob[48..50) | 实测射线（炮塔系） |
|---|---|---|
| V1 基线 | 41 5a | 方位 0.00°、仰 −2.25°（= 炮管轴） |
| V2 yaw=0 | 41 5a | 方位 0.00°（跟随炮塔姿态） |
| V3 | ea aa | 方位 +23.57°、仰 +3.62° |
| A | C0 00 | 方位 +18.13°、仰 −8.70° |

**精确线性拟合**（V1/V3 定参，A 独立验证：预测 a=+0.3278/b=−0.1611 vs 实测 +0.3274/−0.1610）：

```
射线方向（炮管系） = normalize( kx·(b[48]−b[51]),  ky·(b[49]−b[52]),  1 )
kx = 0.00258, ky = 0.00135   （b[n] = blob 第 n 字节）
```

即：**hash6 六字节 = 游戏客户端 DecodeShotSegment 的两点编码（P1=b2b3b4, P2=b5b6b7，
部件 AABB 量化）**——WI 查看器取 P1−P2 的差值作为弹道方向斜率（Δz 归一），
叠加在按 turret.yaw/gun.pitch 摆位的炮管系上，锚定模型节点 → 对 collision_client
碰撞模型 raycast → 弹着标记/等效装甲/穿透率。

**一举 reconcile 全部历史疑点**：
- "hash6[0..2] 逐发变化、同弹种不同值" → 它是 P1 的 (x,y) 量化字节，非弹种；
- "b4/b7（z 字节）40% 压 0x00/0xff" → 入/出点 z 恒跨满盒深（从前到后），分布合理；
- 本发 415a−ff41 的 x 差恰为 0 → 弹道几乎正对炮管 → 此前"射线=炮管轴线"是特例假象；
- 服务端 shotsimulate 重写 61c2→7700 = 重编码 P2（模拟后自己的出点）。
- 基线 y 差 (0x5a−0x77)=−29 → −2.25° ≈ −gun.pitch：垂直方向同样数据驱动。

**精度边界**：方向量化粒度 ≈ 0.15°/单位（水平），方位复现 ±2~3°（与回放几何对照）；
射线的**横向锚定**在模型节点（耳轴附近），真实弹道的横向偏移仍不传递——但方向
（决定打中哪块板）由服务器编码的 P1−P2 真值驱动。这就是"精准弹着显示"的机制。

### §七补2（管线终局：活体读出的射线 = 检视视线，非游戏弹道）

应用完整启动后，从 Lua 全局 `ConfrontationInfoPanel`（RML 面板实例）直接读出
C++ armorTraced 信号交付的 trace 几何 [实证]：

```
ray.origin    = (0.6314, 1.7455, 1.8365)   ← 已裁剪到模型包围盒的入点
ray.direction = (−0.6978, +0.0392, −0.7152) ← 单位向量
```

- 反向水平方位 = atan2(0.6978, 0.7152) = **44.29°**（与 target.turret.yaw 44.2969° 差
  0.01°）、反向俯角 −2.25°——即**相机被放在目标炮管轴线的反向延长线上方，俯视模型**。
- **2026-09-24 用户纠错定案**：这条射线是**检视视线（相机→准星→模型的观察/trace 线），
  不是游戏弹道**。真实射击弹道不经过目标炮管耳轴/炮口线；WI 的 armor 链路从头到尾
  没有收到游戏弹道的任何几何（无位置、无方向、无横向偏移），因此它的弹着标记 =
  **沿自己选定的检视射线所看到的装甲剖面**，只是"准星所指处的装甲读数 + 该处模拟
  弹的结果"，与游戏真实弹着之间隔着一个假设（来向 = 炮管反向）。
- 交叉验证：hash6[2..4]（炮塔系来向，服务器按真实弹道编码）与炮管轴线在本样本差
  ~1°，在其他样本差至 20°——这个差值正是 WI 标记与游戏真实弹着之间的系统性偏差，
  且**协议层面不可消除**（弹道的横向偏移不在任何字段里）。
- 旁证链：面板 Lua 证实散布公式 `dispersion = ShotDispersionRadius × 0.01 × scene.distance`
  （0.297m 即基线散布按 91.2m 折算）；穿透 `shell_properties:getEffectivePiercingPower(distance)`
  按距离衰减；`scene_service:retraceLastShot()` 为面板晚连接/刷新场景准备的"重放最后一次
  trace"入口——**这正是"刷新时好时坏"的机制位置**：arg.shot 一次性消费 + trace 状态
  依赖启动时序，竞态失败即落回默认姿态。
- hash6[2..4]（炮塔系来向 ≈179°）与本射线（炮塔系 180°）在本样本中简并（差 1°），
  无法区分 C++ 用哪一个；但 hash6[4..6] 俯仰（−21°/−42°）与射线仰角（−2.25°）明显
  不符 → **射线取自炮管轴线而非 hash6**。hash6 更可能只服务游戏客户端自身的
  受击方向指示，WI 侧未消费。
- **交点与穿甲计算实测**（同一活体，trace 点序 3 个穿越点，沿射线 0.482/1.550/3.564）：
  第一交点（准星处，模型局部系）= origin + 0.4819×direction =
  `(0.295, 1.764, 1.492)`；该点 `material.armor = 203.0mm`（主装甲，primary）、
  `cos_angle = 0.479`（法向夹角 ≈61.4°）→ 等效厚度 203/0.479 = **423.7mm**，
  与面板渲染的 424mm/pen chance 0.0%（HEAT pen 340mm）逐位闭环。
  （注意：这是**检视射线上的读数**，即"这条视线穿过的装甲"，不是游戏弹着点。）
- **"平行射线族"最终回答**：用户的质疑成立且贯穿始终——blob 参数（姿态+方向+距离）
  只能确定一个**方向**，弹道的横向偏移（即真实弹着点）不在协议里，因此"游戏那一发
  的交点"从 armor 链路原理上不可重建。WI 的解法是回避：它展示的是**自己选定的检视
  射线**（shot 模式下预置于目标炮管反向延长线，用户可用十字准星拖动）所穿过的装甲
  与该处模拟弹结果。唯一能算出真实弹着的是完整回放路径：method29/20 弹道线（世界系
  精确直线）× 目标模型求交——即本项目现行方案，严格强于 WI 的 armor 链路。

### §七补（终局确认：`c=` 分享码解码 + 活体状态读取 + 渲染终态）

1. **`c=` 分享码解码** [实证]：`base64url(raw-deflate(JSON))`（codec = share-codec
   chunk：`Te(ce(JSON.stringify(state)))`）。schema：
   `{v:1, m:viewMode, tab, p:{v,ch,hu,tu,gu,en,ra,sh}, cmp:{sets:[…]}, orig}`
   ——v/ch/hu/tu/gu/en/ra/sh = vehicle/chassis/hull/turret/gun/engine/radio/shell
   **模块 id**。用户样本 `c=` 解出：`m:3`（shot 视图模式）+ T110E5 全套配置
   （ch=22562, tu=19235, gu=271396, en=13093, ra=2855）。**无任何坐标/方位字段**。
2. **`c=` 不进 C++** [实证]：带 `&c=` 的 URL 的 SSR 页面，`Module.arguments` 仅含
   `--targetVehicleId 10785` + `--setting arg.shot <blob>` + pkg/staticHost/archiveHost/
   platform/language——`c=` 只驱动 React 层（模块选择器/对比视图），且 `shot=` 存在时
   React 显式忽略 `p`（`playerVehicle: hasShot ? null : rr(a.p)`）。
3. **活体状态读取**（完整启动后 `_execute` Lua 控制台）[实证]：
   `scene.shotSimulateDistance = 91.208442687988`（= blob distance 原样）、
   `scene.shotSimulateTargetId = 10785`、`scene.shotSimulatePlayerId = 10785`——
   blob → C++ 模拟状态逐字段落地。坑：settings:get* 需冒号语法 + 默认值三参、
   键类型错配会触发 wasm 陷阱（pcall 拦不住）。
4. **渲染终态**（截图实证）：confrontation 视图 = Monte-Carlo 对抗模拟——
   射手 HEAT 340dmg/120mm/pen 340mm、dispersion 0.297m、distance 91.2m，
   打 6 发模拟弹（"pen by 6/6 shots"），弹着标记处等效装甲 424mm →
   pen chance 0.0%，并给出 "total dmg received 2400 / dead in 48s" 预后。
   弹着点 = **WI 自己的模拟弹落点**（UI 十字准星可拖动改变瞄准），非游戏真实弹着。
5. **blob 字段消费去向（最终版）**：
   | blob 字段 | 去向 |
   |---|---|
   | shooter.{vehicle,chassis,turret,gun} | 射手火炮属性（散布 0.297m、弹速） |
   | shooter.shell | 弹种（HEAT 340dmg/120mm/pen 340mm） |
   | target.{vehicle,chassis,turret,gun} | 目标碰撞模型 `collision_client/{gun}.model` 选择 |
   | target.turret.yaw / gun.pitch（度） | 目标模型炮塔/炮管摆位 |
   | distance | 射击距离（穿透衰减、散布投影、预后计算） |
   | segment.result / layer | 击穿/弹开着色与装甲层 |
   | segment.hash6 | **未消费**（游戏特效字节，无几何语义，且已被服务端重写） |

### §七补5（轴序终局：穷举校准推翻"补偿论"，hash6 位置语义恢复）

对 8 发命中弹做**轴映射穷举校准**（48 种 排列×镜像 解码 P1 → 与真实弹着点测距）：

| 发 | 部件 | 直序(012) | 交错序(021) |
|---|---|---|---|
| 8 | hull | 3.49m | **0.088m** |
| 10 | hull | 4.26m | **0.278m** |
| 12 | hull | 4.28m | **0.253m** |
| 4 | 炮塔 | 2.52m | **0.409m** |
| 9/11/13 | 底盘(hull回退盒) | 1.8~2.4m | 0.6~1.1m（盒源不符） |

**定案：客户端反汇编的"字面交错序"才是正确映射**——x右←b2/b5、y前←b4/b7、
z高←b3/b6（§2.3 当年发明的"游戏块 {右,高,前} 补偿论"把它翻反了）。修正后
viewer 实测：shot8 P1 与弹着差 **0.1m 内**、shot4（炮塔）差 **0.25m**。

**重大翻案——hash6 位置语义恢复**：
- **b4 恒 255 的"异常分布"恰是真实入射点的特征**：穿盒前线段入点的前向坐标
  必在盒前界面（=max）；四轮报告的"字节压端点证伪坐标论"是错误轴映射下的
  假象（b4 被当成了高度字节）；
- 即 method8 hash6 = **服务器编码的真实入点/出点**（部件盒 1/255 量化）——
  协议其实下发弹着位置（量化精度），游戏客户端弹孔解码链（§2.1/§2.5）语义
  完全成立；"死路清单"中"hash6 坐标语义证伪"条目应撤销；
- 侧向/高度字节（b2/b3）与真实弹着的横向/高度吻合（hull 三发 0.09~0.28m 含
  量化步长 ~1.5cm×范围 + 盒源差）；
- §七补3 的 WI 结论不变：blob 端消费 (b2−b5, b3−b6) 差值作方向斜率——但现在
  可解读为"入出点连线方向"的自然结果，而非独立语义。

### §七补4c（浮点精度定量化 + 首版盒膨胀 bug 修复）

浮点误差预算（解码全链 JS 双精度）：盒插值/矩阵乘/量化除法合成误差 ≤1e-13m（皮米级），可忽略；GLB 顶点 float32 ≈2e-7m，可忽略。真正误差源 = 解码盒本身：
1. 首版用 几何AABB×rest矩阵 并集，旋转几何的 AABB 再变换会虚胀（炮塔盒实测 x 3.69→2.81、y 4.54→4.11，虚胀 0.4~0.9m）——已改顶点级紧致盒（遍历真实顶点变换取 min/max）；
2. 修正后 shot5 解码盒 (−1.29..1.30, −2.13..1.80, 1.57..2.62) 与 WI trace 板级数据吻合；判定 ✗不一致（本地 PEN 254 vs 服务器 未击穿）为游戏 ±随机化的边际判定（HEAT 254 pen vs 板 256 eff），本地模拟不掷骰、显示期望结果，属固有差异非 bug。

### §七补4b（解码盒源修正：游戏原生部件盒优先）

按用户指令复核解码盒来源，发现首版装甲网格紧致并集盒是自建代理而非游戏原生数据，且存在旋转 AABB 虚胀（已改顶点级紧致盒）。终版实现改为游戏原生 collision.*_bbox 部件盒优先（game_data JSON 的 collision 段 = 从游戏文件提取的部件节点包围盒，坐标系 x右/y前/z上 与装甲模型一致；hull/chassis 位于模型原点、turret/gun 位于部件枢轴，放置时按枢轴+当前位姿角旋转），缺失时回退网格紧致盒。定位/判定路径同步审计：装甲厚度、枢轴、散布公式、armorTraced 均直用游戏/BlitzKit 数据；遗留硬编码仅枢轴缺数据兜底 (0,0,1.7)/(0,0,2.0)（数据缺失分支，不影响正常路径）。

**提取链修复（2026-09-25）**：早期 turret/gun bbox 大面积为 null 的根源 = 解析器硬编码段名 `turret_01:`/`gun_01:`，而游戏 YAML 段名带**模型节点号且零填充**（T110E5 的游戏文件 usa/T110.yaml 内唯一炮塔/主炮段名为 `turret_02:`/`gun_06:`），且 hull 段 averageThickness 内含 `turret_02: 186.08` 引用行（非段头）需排除。修复后全量收集 `turret_NN:`/`gun_NN:` 段（头行判定 = 段名+数字后紧跟冒号+行尾；段名按原始零填充数字串查找），再经 **models.pb 模块id→节点号映射**（`TankModelInfo.turrets[].model_node` / `GunModelInfo.model_node`，顶级配置 = tanks.pb turrets/guns 末位模块 id 匹配，缺失时回退末位/首段）挑选本车顶级配置写入 turret_bbox/gun_bbox。枢轴交叉验证：YAML `points` = −部件枢轴（模型系），与 models.pb track_origin+turret_origin 逐位一致（T110E5: (0,0.288,1.639)）→ 碰撞盒帧 = viewer 枢轴帧，placePt 逻辑无需改动。覆盖率（重提取后实测 728 车）：hull/chassis 100%、**turret 98%（715 车）、gun 95%（694 车）**，此前 T110E5 炮塔件"网格盒回退"的板级偏差随之消除。

### §七补4（项目侧落地：射击复现界面已标注 P1/P2）

`src/wargaming/viewer.rs` world 模式新增 DecodeShotSegment 两点标注（2026-09-24）：
- 数据源 = shots JSON 既有 `hit_token`（method8 hash6 十六进制），无 Rust 侧改动；
- 解码 = 部件 AABB 静止盒并（`turret_N_armor_`/`gun_N_armor_`/hull 网格，rest 矩阵）
  × 量化字节。**轴序（§七补5 穷举校准定案）**：字面交错序 x右←b2/b5、y前←b4/b7、
  z高←b3/b6；armorModel 局部系 x右/y前/z高（z-up）。P1 ◆橙（入点，实测距真实
  弹着 0.1~0.4m）/ P2 ○绿线框（盒内出点）+ 虚线段；炮塔/炮管部件经
  `node.matrix × rest⁻¹` 随滑块位姿联动；part 0（底盘）装甲模型无网格 →
  回退 hull 盒并标注（盒源不符时 P1 偏差 ~1m）；
- 调试行 `DecodeShotSegment`：两点世界坐标 + 量化差 Δb2−b5/Δb3−b6/Δb4−b7（WI 方向源）；
- 验证：T110E5 样本 shot#4（415a，炮塔）：交错序修正后 P1 距真实弹着 0.25m、
  shot#8（hull）0.1m 内 ✓；shot#9（2d97，底盘）hull 回退 ~1m（盒源不符）。
- **判定基准切换（2026-09-24 用户指令）**：击穿判定射线与弹着红点改用
  **P1→P2 线段**（原点 = P1 沿弹向回退 1m，方向 = P2−P1；ctxLaunch/ctxLvDir/
  ctxRayFar 覆盖 → 自动判定/滑块重跑/换弹重跑全线切换）；弦射线仅作无 token
  兜底。命中弹的轨迹可视化（弦线/发射点/速度虚线/服务器终点/来向射线）按
  `SHOW_TRAJ_ANNO`（命中弹=false，脱靶弹保留——弹道是其核心复现内容）隐藏。
  实测 shot#4：判定"✓ 一致 · 本地 BLOCKED · 服务器 NO PENETRATION"，
  场内仅剩 红点(=P1)+橙◆P1+绿○P2+段线。**点击维持锁定**（用户指令：不接受
  点击判定，仅初始 raycast 换 P1→P2 基准；onClick 恢复拦截）。
- **判定射线实现要点**（两层时序坑）：① armorModel 异步加载，seg 块（覆盖判定
  射线的代码）晚于 600ms 弦判定计时器——初始判定会先用旧弦跑一遍；② 覆盖写
  `ctxLaunch` 变量会被摆位链重赋值冲掉。终版实现：seg 块内先 `updSeg()` 摆点
  （此前 mkP1/mkP2 还是零向量），再存**独立全局 `window.__segRay`**
  {origin=P1−dir×1m, dir=(P2−P1)归一, far=max(2.5×|P2−P1|,6)}，
  `doPenetrationCheck` 射线选择优先级 = `__segRay` > 弦 > 相机——对初始/滑块/
  换弹所有路径生效且免疫重赋值。seg 块内随即以 `__segRay` 重跑判定（penSeq
  递增丢弃旧响应），trajGroup 判定管从跨场弦（x −44→+44）缩短为 P1→P2 短管
  （x 42.6→43.7）。
- **端到端验证（shot#9，修复后）**：`__segRay.dir` 与 (P2−P1) 归一化点积 = **1.000**
  （判定方向严格沿 P1→P2）；触发重判后面板 "Hull Plate1 211mm / 260eff / pen BLOCKED"
  （254mm pen vs 260mm eff，差 6mm 的边际判定）。入射角按 P1→P2 与板防护法线计算
  ≈36°；几何面法线直算为 51~67° 的差异来自"板盒侧面 vs 防护法线"（装甲板保留原始
  防护法线，射入侧面时面法线 ⊥ 防护向，属正常）。残留不一致（服务器 PEN+410dmg vs
  本地 BLOCKED）为边际判定 + 游戏内 ±随机化 + part0 盒近似（hull 回退）的合成偏差，
  非变换错误。

### §七补6（终局实证：WI 活体 trace 数据 = 面板实例字段，与本发对照）

在 WI 官方 armor 查看器加载同发（battle#1，token 4bb9，未击穿）后，经 `_execute`
Lua 读取其面板实例 `ConfrontationInfoPanel` 的 `ray` / `points` 字段 [实证]：

```
ray.origin    = (−0.978, 2.251, 1.564)   （模型局部）
ray.direction = (0.311, 0.122, −0.942)
pt1: distance=0.382, cos_angle=0.2085（入射 78°）, material.armor=84mm
     （device_type=0 主装甲, vehicle_damage_factor=1.0）
pt2: distance=1.969, cos_angle=0.916, material.armor=0mm（device_type=9 间隔层）
```

判读：HEAT 340 打在 **84mm 主装甲、入射 78°** → 等效 84/0.2085 ≈ **403mm** →
HEAT 340 BLOCKED ✓ 与服务器未击穿一致。同一 hash6（4bb9ff4bcdc7）我们解析出的
通道在其模型上命中 84mm 板 @78°——**与游戏客户端语义完全同构**；我们本地判定
同发为 Turret Plate 6 140mm @57° = 256eff → 同判 BLOCKED，板级差异来自碰撞
模型源（collision_client vs itemDefs 装甲网格）。

同时确证面板数据流（confrontation_info_panel.rml Lua，解密件）：
`scene_service:connect_armorTracedSignal(onArmorTraced(ray, points, bounding_box))`
→ 面板存 `self.ray/self.points` → `ShotResult.computeShotResult(Ray.new(), points,
shell, …, shot_visitor)` 累计 `piercing_chance / effective_thickness / internal_travel_distance`
→ 穿透率着色与 "dead in 48s" 预后；`shell_properties:getEffectivePiercingPower(distance)`
按距离衰减；散布 = `ShotDispersionRadius × 0.01 × scene.distance`。

### §七补7（入射角定案：WI 射线 = 炮管轴 + b2/b3 角度偏移；双实现对照）

在 WI 官方查看器加载同发（battle#1）后，从其面板实例读出 **WI 自己的判定射线**：

```
ray.direction = (0.311, 0.122, −0.942)   （WI 模型局部系：x右/y上/z前）
  → 仰角 = asin(0.122) = +7.03°  ≈ 该发 gun.pitch +7.27°（炮管俯仰）
  → 方位 = atan2(0.311, −0.942) = 161.7° = 180° − 18.28° = 炮管轴反向
    （turret.yaw −18.28° 的反向延长线）
```

即 **WI 的判定射线 = 受击目标自己的炮管轴反向**（从炮口向炮塔打入），
方位/俯仰与 blob 姿态角（−18.28°/+7.27°）精确吻合——b2/b3 字节的差分斜率
（kx=0.00258、ky=0.00135，零点 b2=65/b3=90）就是相对炮管轴的角度偏移编码。

**与我们的对照（同发）**：我们当前按 P1→P2 量化差值构建射线，本地判 BLOCKED ✓
与服务器一致；WI 沿炮管轴反向 trace 判 403mm eff / BLOCKED ✓。两边结论一致但
**穿过的板与角度不同**（WI: 84mm 板@78°；我们: 车体板@51°）——根源是两者把
同一 hash6 通道映射到了各自的碰撞模型上（collision_client vs 我们的 itemDefs
装甲网格），板厚/法线存在模型源差异。

**对项目的修正方向**（待实现）：入射角应改按 **炮管轴反向 + b2/b3 角度偏移**
构建（与 WI 同构），弹着点取该射线与模型的交点——而非直接把 P1/P2 当世界坐标
摆放（盒源差异会传导）。炮管轴由 shot 数据的 target_turret_yaw/gun_pitch 给出，
无需额外数据。

> **✅ 已落地（2026-09-25，viewer world 模式 seg 块重写，终版=公式直译 + 位置锚定合成）**：
> ① 判定方向 = **§七补3 差分公式直译**：炮塔系 dir = normalize(0.00258·Δb2,
>   0.00135·Δb3, 1)（z = 炮管反水平轴）→ Rz(炮塔偏航) → 模型系 → 场景系。
>   方位/俯仰均由字节差完整编码（V1 基线 −2.25° 恰为炮管轴反向；V3/A 方位
>   23.57°/18.13° 逐位复现）——**勿再叠加炮管轴**（首版误按"炮管轴+偏移"实现，
>   俯仰重复计算，实测 shot#5 俯仰 −19.5° vs 公式 −1.55°，已修正）。
>   场景仰角再叠加车体坡度姿态（经 matrixWorld 带入，物理正确）。
> ② 判定位置 = 解码 P1（服务器编码真实入点，游戏原生盒 + 枢轴帧），判定射线
>   origin = P1 − 0.5·弹向（从表面外进入）。**耳轴长射线方案已废弃**——实测从
>   炮管轴线上回退的射线会先打中自家炮管板（Gun Plate 0eff 污染判定链）。
> ③ 弹着点红点 = P1；段线 = 解码 P1→P2（游戏编码出入点段，随位姿联动）；
>   初始判定/滑块重跑/换弹重跑全线走新射线（`__segRay` > 弦 > 相机优先级不变）。
> ④ 前置数据修复（同日）：collision turret/gun 盒提取链（零填充段名扫描 +
>   models.pb 节点映射，见 §七补4b 补记）。
> ⑤ 实测（T110E5 shot#5，部件 P2 炮塔）：模型系弹俯仰 −1.55°（= ky·Δb3，
>   字节编码值）；本地判 PEN（打中炮塔上部 784mm 板 @80° 掠射 → 129eff）vs
>   服务器 NO PEN——横向偏移不传递（§七补2）+ 板源差异（§七补6）的合成偏差，
>   在协议不可消除界限内。`__segDiag.ray()` 诊断钩子可读出方向/入出点/偏移角。

## 八、对项目的落地建议（修订）

1. **路线正确性确认**：WI 独立实现选择了与本项目相同的解法——弹道 × 碰撞模型求交定弹着点；
   `decal_hit`（AABB 解码点）路线的废弃与 WI 行为一致。
2. **可借鉴的差距**（按收益排序）：
   a. WI 用游戏原始 `collision_client/{}.model`（CollisionMeshes）而非 itemDefs 轴对齐盒做求交——
      即项目六轮报告建议的"数据源升级"路线，是弹着点残差收敛到分米级的关键；
   b. segment b1 `layer`（装甲层序号）是项目 type=32 提取链尚未单独输出的字段
      （B7=plateId 之外的层信息），可补充间隙弹的层归属；
   c. 穿透模拟链（距离衰减 `piercingPowerLossFactorByDistance`、均质化
      `use_armor_homogenization`、跳弹角 `ricochetAngle`）可用于"复现 WI 式"的
      击穿/未穿着色，项目已有 result 字段可交叉验证；
   d. **方向语义对齐（终版）**：hash6[2..4] = 受击者炮塔系来向方位（BE、半圆比例尺）。
      项目弹着复现应实现同构管线：目标模型按 blob/prop2 姿态摆位 → hash6 方向在炮塔系
      转成世界系射线 → 部件盒求交。该来向是服务器命中判定时刻的编码真值，可作为
      弹道反解的交叉验证与坡地姿态修正的数据源；[4..6] 俯仰零点（疑含车体姿态）待控制
      实验定标。
3. **无需再找**：WI 侧也不存在坐标级命中数据；"向 WI 对齐弹着点精度"的物理上限
   就是碰撞模型精度，与本项目结论相同。

## 九、证据索引

| 结论 | 证据 |
|---|---|
| 查看器=WASM 原生应用 | map.wotinspector.com/en/blitz11.18/ 页面 `Module`+`map-inspector.wasm`(7,495,420B)，canvas WebGL |
| 查看器吃原始回放 | iframe `?url=<download_url>&frame&time=`；wasm 串 `data.wotreplay`/`wotbreplay`/"replay has been downloaded %s"/`arg.replayUrl` |
| shots 表字段 | `api.wotinspector.com/v2/battles/14954085777440988/?platform=blitz` 实测 JSON |
| segment=[result][layer][hash6] | 8 发 u64 小端字节解码（tmp_wi_js/battle.json） |
| shotsimulate 模板 | map-inspector.wasm 明文 `pkg={}&target.vehicle={}&…&segment={}&distance={}` |
| shot blob 布局 | 可控变量探针 16 字段逐一定位（tmp_wi_js/）：shooter 块@2、target 块@22、segment@46、distance f32@54=41.579 |
| armor 端消费链 | armor.html `--setting arg.shot <blob>`；armor-inspector.wasm 串 `scene.shotSimulate*`/`simulateShot`/`hitTester/collisionModelClient`/`vehicles/{}/{}/collision_client/{}.model` |
| hitTester/碰撞模型 | wasm 串 `hitTester/collisionModelClient`、`vehicles/{}/{}/collision_client/{}.model` 等 |
| 无坐标结论 | battle.json shots 全字段无坐标；shot blob 无坐标；hash6 坐标语义已被项目七轮逆向证伪 |
| 真实 blob 解码 | 用户提供链接实测：姿态为度、yaw=126×0.3515625°（prop2 粗位精确整数倍） |
| hash6 无方位角 | wi_align_probe 11 发对照：[2..4] 恒 0xFFxx 而方位角跨 ±180°；全组合扫描无方位相关（仅 [2..4]BE×90° vs 发射俯仰 MAE 3.2°，弱） |
| WI 重编码 segment | 同发 battle.json `01 02 415a ff41 61c2` → blob `00 02 415a ff41 7700`（result 1→0、末字节对改写） |
| 游戏原始回放 | replays 站公开下载 1MB ZIP（meta.json/battle_results.dat/data.wotreplay），项目解析器直读成功 |
| c= 分享码 | base64url(raw-deflate(JSON)) 实测解开：m=3 + T110E5 模块配置（en/ra 等），无坐标/方位；codec=share-codec chunk |
| c= 不进 C++ | 带 &c= 的 URL 的 SSR `Module.arguments` 仅含 arg.shot + targetVehicleId 等；React `playerVehicle: hasShot?null:rr(a.p)` |
| 活体状态 | 完整启动后 Lua 读取：scene.shotSimulateDistance=91.208442、TargetId/PlayerId=10785 |
| 渲染终态 | 截图：confrontation 视图 6 发模拟弹、HEAT 340 vs 等效 424mm、pen chance 0.0%、dispersion 0.297m、dead in 48s |
| 链接为浏览器端拼接 | replays bundle 明文：`turret_yaw/Math.PI*180`、`chassis_id||-1`、`distance>0?distance:400`、`BigInt(segment)` |
| 数据包解密 | ZipCrypto 已知明文攻击（bkcrack，PNG 头 16B）→ 全包 409/409 CRC 校验通过；UI Lua 仅调 `shots[i]:simulateShot()`，几何在 C++ |

## 十、遗留未定项

- **method8 hash6 的真实语义**：旧 [shell][yaw][pitch] 已证伪（§七）。候选：游戏客户端
  DecodeShotSegment 的 AABB 量化点（项目七轮逆向的"机械解码"结论，字节压边界特征吻合：
  本样本 [2..4] 恒 0xFFxx = y 上边界）；或是特效/种子类非几何字段。建议用 WI 全量
  battle.json + 本探针做跨回放大样本统计；
- **type=32 尾 8 字节真实布局**：本样本 `02 3d ff 00 08 35 2a 01`（shell 大端 u24@[4..7)、
  result@[7]）与项目文档 `[result][shell LE u24][00][X][Y][Z]` 冲突，需重推（注意 26/27B
  变体与 method 0x11/0x12 差异）；
- armor 端 C++ `simulateShot` 的模拟种子与锚点：WAT（armor.wat，85MB）已定位引用
  `scene.shotSimulate*` 设置的函数（func 8525 调用链），可继续静态分析；或修复启动竞态后
  用 `_execute` Lua 控制台活体取证（`tmp_wi_js/harness/` 本地 harness 可复用）；
- `armor_decrypted/`（全量解密 Lua/RML/YAML）中无弹着几何逻辑（UI 层），已核实。
