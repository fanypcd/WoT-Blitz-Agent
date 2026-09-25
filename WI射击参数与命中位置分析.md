# WI（wotinspector）射击参数与命中位置分析

> 方法：抓取 wotinspector 前端资源与公开 API 逐层分析（replays 站 React bundle →
> battle.json → Map Inspector WASM → shotsimulate blob 解码 → armor 查看器活体取证），
> 并与本项目 combat.rs 提取链逐字段对照。本文为**整合后的终版结论**：协议事实、
> WI 消费方式、本项目复现实现与固有差异界限。原始产物存 `tmp_wi_js/`（未入库）。

---

## 一、总体架构

WI 的"3D 查看器"不是网页 JS 渲染，而是**整个 Emscripten WASM 原生应用**
（Map Inspector，C++ 编译，自带 WebGL 渲染器）：

```
replays.wotinspector.com/en/blitz/view/<hash>/      ← React 摘要页（射击表格/战绩）
   ├─ battleDetailsUrl  api.wotinspector.com/v2/battles/<arena_unique_id>/?platform=blitz
   │     → 服务器预解析 JSON：shots 表 + chat（§二）
   ├─ "Watch online" → iframe: map.wotinspector.com（WASM 自行下载完整 .wotbreplay
   │     解析包流、加载游戏数据包后本地渲染，§三）
   └─ shotsimulate    api.wotinspector.com/v2/ai/shotsimulate/（装甲查看器复现弹道，§四）
```

**网页 shots 表与 3D 查看器是两套数据流**：表格是服务器预解析统计 JSON；
3D 查看器拿到原始回放文件在浏览器内解析，可用参数 ⊇ 本项目 combat.rs 能提取的全部字段。

## 二、网页 shots 表（battle.json）字段清单

每发命中弹一条（脱靶无记录）：

| 字段 | 与本项目对照 |
|---|---|
| time / shooter / target | fire 时刻 + eid 配对（同源） |
| has_damage | method1 血量链归属 |
| shell_id | type=32 segment u24 / 项目 `shell_id`（已对齐） |
| turret_yaw / gun_pitch (rad) | prop2 @命中通知时刻（项目 `target_turret_yaw/gun_pitch`，已对齐） |
| **segment**（u64 十进制串） | **[result u8][layer u8][hash6 6B]**（§五） |
| distance | 流序 type10 欧氏距离，μ 级可复现 |
| nominal_pen / nominal_damage | item_defs 弹种表 |
| damage / hit_flags / crit_modules / destroyed_modules | method38 位图（已对齐） |
| is_premium_ammo / ammo_type | type=28 + 弹种表 |

**整表没有任何命中坐标字段**——WI 与本项目收到的是同一份无坐标协议。

## 三、3D 查看器（WASM）内部

wasm 字符串（13232 条唯一串）显示其内嵌：

- **BW 协议绑定**：与项目 type=10/prop2 同源的实体位置/姿态流解析；
- **自有 Arena 事件流**（protobuf）：发射/终点/爆炸/炮塔角/移动/血量变化；
- **完整装甲穿透模拟**：`piercingPower`（含距离衰减）、`getPiercingChance`、
  `armourPiercingFactor`、`use_armor_homogenization`、`ricochetAngle`、`isHEShell`；
- **碰撞模型与射线求交**：`hitTester/collisionModelClient`，模型路径
  `vehicles/{nation}/{tank}/collision_client/{gun}.model`（即游戏
  `3d/Tanks/CollisionMeshes` 同源几何，模块 id 决定炮塔/炮管变体）；
- **弹道行为设置**：`continue_trace_if_no_hit`、`use_hit_angle`、`chance_to_hit_by_projectile`。

## 四、shotsimulate：解析器向 armor 查看器传递的信息

模板（map-inspector.wasm 内明文）：

```
pkg={}&target.vehicle={}&target.chassis={}&target.turret={}&target.gun={}&
target.turret.yaw={}&target.gun.pitch={}&shooter.vehicle={}&shooter.chassis={}&
shooter.turret={}&shooter.gun={}&shooter.shell={}&segment={}&distance={}
```

返回 armor.wotinspector.com URL，其 `shot=` 参数为 base64url 的 **58 字节 shot blob**
（armor 端经 `--setting arg.shot <blob>` 注入）。逐字段可控变量探针钉死的布局：

```
偏移    类型  字段
[0]     u8    =0x02（版本/魔数）
[1]     u8    =0x01（恒定）
[2..22) u32×5 shooter.{vehicle, chassis, turret, gun, shell}（模块/弹种 id）
[22..38) u32×4 target.{vehicle, chassis, turret, gun}
[38..42) f32  target.turret.yaw（度；= prop2 粗10位 × 0.3515625°，实测 126 步精确）
[42..46) f32  target.gun.pitch（度；prop2 frac6 按车型极限解码的连续值）
[46..54) u64  segment 原样（[result][layer][hash6]；服务端可能重编码，见 §5.2）
[54..58) f32  distance (m)
```

**没有传递**：命中坐标、发射点/弹速、目标世界位置与车体朝向、任何时间线。
armor 查看器收到 blob 后设置 `scene.shotSimulate{PlayerId,TargetId,TargetAngles,
Distance}`，调 `simulateShot` 对目标 `collision_client/{gun}.model` 射线求交。

**姿态取样语义**（用原始回放逐发对照 21/21 钉死）：blob 的姿态 = method8 直击通知
包在包流中出现时刻的**流序最后已知**受击者 prop2 快照（单遍流式解析器状态，非时钟序）：
距离 = 双方 type10 欧氏距离（μ 级）、炮塔角 = prop2 粗10位（逐位）、炮管俯仰 = frac6
按扇区极限解码（与项目解码等值反号，WI 压角为正）。**hull 车体角完全不取样**——
armor 查看器中车体恒默认朝向。项目 combat.rs 已按流序落地同构快照
（`DirectHit8::victim_prop2` / `LaunchEntry::shooter_prop2`），炮塔角/俯仰/距离逐位复现。

**服务端重编码**：shotsimulate 返回的 segment 与 battle.json 原值不同
（实测 `01 02 41 5a ff 41 61 c2 → 00 02 41 5a ff 41 77 00`：result 与末字节对被改写）
——armor 端展示的是 WI 自己模拟管线的输出，非游戏弹着的原样重建。

## 五、hash6 / DecodeShotSegment：命中位置的编码与消费

### 5.1 编码格式（游戏客户端反汇编 @0x1334E30，逐指令核对 + 穷举校准）

method8 直击元素 u64 = `[result b0][cmpIndex b1][hash6 = b2..b7]`，hash6 即
**DecodeShotSegment 两点编码**——服务器在命中判定时刻把出入点写入部件 AABB 量化坐标：

```
box = partBlock[cmpIndex]（部件 AABB，部件枢轴系）
P1 = min + (max−min) × (b2, b4, b3)/255      轴序：x右←b2、y前←b4、z高←b3（交错）
P2 = min + (max−min) × (b5, b7, b6)/255
```

- b4/b7（前向轴）40% 压 0xff 等端点值 = 入点钉盒前界面的真实特征；
- `out1 == out2` 单点分支 = 点射终止（未穿/跳弹），双点 = 贯穿段；
- 客户端把 P1/P2 经部件矩阵变换后放置受击特效——协议以此替代下发精确几何；
- 穷举校准（8 发 × 48 种排列镜像，对真实弹着测距）定案上述轴序：hull 三发
  0.09~0.28m、炮塔 0.41m；游戏内存部件块为 {右,高,前} 存储序，交错映射即其忠实反映。

### 5.2 WI armor 查看器的消费方式

- blob 姿态角（turret.yaw/gun.pitch，度）摆位目标模型；**判定射线 = 受击者炮管轴反向**
  （方位 = 180°−turret.yaw、俯仰 ≈ gun.pitch），横向锚定在炮耳轴附近；
- hash6 六字节对射线做**差分斜率微调**：`dir(炮塔系) = normalize(kx·Δb2, ky·Δb3, 1)`，
  kx=0.00258、ky=0.00135（活体差分实验三点拟合，V3/A 方位 23.57°/18.13° 逐位复现）。
  注意该公式丢弃前向轴 Δb4−b7 分量，只是 WI 的方向近似；
- **本质 = 检视视线**：弹道的横向偏移不在任何字段里，WI 用"来向 ≈ 炮管反向"假设
  展示准星处的装甲剖面与该处模拟弹结果——其弹着标记与游戏真实弹着之间的系统偏差
  协议层面不可消除；面板数据流：`armorTraced(ray, points, …)` →
  `ShotResult.computeShotResult` → 穿透率着色与预后（散布 = ShotDispersionRadius×0.01×distance，
  穿深按 `getEffectivePiercingPower(distance)` 距离衰减）；
- `retraceLastShot` / `resetCameraToLastShot` 为刷新重放入口——`arg.shot` 一次性消费 +
  trace 状态依赖启动时序，竞态失败即落回默认姿态（"刷新时好时坏"的机制位置）。

### 5.3 与服务器判定的一致性

同发对照（T110E5 打 T110E5，hash 4bb9ff4bcdc7）：WI 射线命中 **84mm 主装甲 @78°**
→ 等效 403mm → HEAT 340 判 BLOCKED，与服务器 result=1（未穿）一致；本项目 P1→P2
弦判定命中同板（服务器 segment B7 装甲组 = 7 亦同），判未穿——**板级一致**。
入射角与穿/弹类别级的残余差异见 §6.3。

## 六、本项目复现实现（射击复现 3D 视图）

### 6.1 数据链：碰撞盒提取（`extract-game` / game_data）

- 来源 = 游戏 YAML `3d/Tanks/Parameters/*.yaml.dvpl` 的 collision 段；
  段名带模型节点号且**零填充**（T110E5 的游戏文件内唯一炮塔/主炮段名为
  `turret_02:`/`gun_06:`），全量收集 `turret_NN:`/`gun_NN:` 段（头行判定排除
  averageThickness 引用行）；
- 顶级配置经 **models.pb 模块id→节点号映射**挑选（tanks.pb 末位炮塔/主炮模块
  匹配 `TurretModelInfo.model_node`/`GunModelInfo.model_node`，缺失回退末位/首段）；
- 枢轴交叉验证：YAML `points` = −部件枢轴（模型系），与 models.pb
  track_origin+turret_origin 逐位一致 → 碰撞盒帧与查看器枢轴帧同一；
- 覆盖率（728 车）：hull/chassis 100%、turret 98%、gun 95%。

### 6.2 判定管线（viewer world 模式 seg 块）

- **出入点标注**：P1/P2 = hash6 按 5.1 公式解码（游戏原生盒 1/255 量化点），
  摆放经**部件网格世界矩阵**（网格局部系 = 枢轴系，实测 origPos=0），由位姿链驱动、
  随滑块炮塔/炮管转角联动；段线 = P1→P2（游戏编码命中线段）；
- **判定射线 `__segRay`** = {origin: P1 − 0.5·dir, dir: P1→P2 解码弦}：
  raycast 与入射角**同源此射线**（服务器编码的位置 + 弹向），世界系，优先于
  弦/相机射线源；初始判定/滑块重跑/换弹重跑全线走它；
- **入射角参数**：`/api/penetrate` 的 view_dir = 该射线方向（世界系，与命中法线
  同坐标系）；命中法线 = 世界系面法线（classifyHit）；
- 弹着点红点 = P1；盒缺失回退网格盒并在调试行标注。

### 6.3 固有差异界限

- **横向偏移不传递**：hash6 两点在部件盒内的横向位置即服务器编码的全部位置信息，
  与真实弹着间存在量化 + 盒源偏差（0.1~0.4m 级）；
- **板源差异**：本项目求交用 itemDefs 装甲网格，游戏/WI 用 collision_client 模型——
  同一块板的面法线/边界有差，入射角与穿/弹类别级（跳弹 vs 未穿）可能不同
  （shot#5 实测：同板 84mm，本地弦向掠射触发跳弹规则，服务器记未穿）；
- **随机化**：游戏内 ±5% 边际判定，本地模拟显示期望结果；
- 收敛路线 = 换用 `3d/Tanks/CollisionMeshes/*.scg.dvpl` 自建 HitTester（格式已解）。

## 七、`c=` 分享码与活体状态

- `c=` = base64url(raw-deflate(JSON))：`{v, m(viewMode), tab, p:{v,ch,hu,tu,gu,en,ra,sh}, cmp, orig}`
  ——全模块 id，无任何坐标/方位；只驱动 React 层（`shot=` 存在时被忽略），不进 C++；
- 活体读取：`scene.shotSimulateDistance/TargetId/PlayerId` 逐字段落地 blob 值；
  注意 `settings:get*` 键类型错配会触发 wasm 陷阱。

## 八、证据索引

| 结论 | 证据 |
|---|---|
| 查看器 = WASM 原生应用、吃原始回放 | 页面 Module + map-inspector.wasm(7.5MB)；`arg.replayUrl` 等 wasm 串 |
| shots 表字段 / 无坐标 | battle.json 实测 JSON 全字段 |
| players_data.chassis/turret/gun_id = 按 tank_id 静态取**顶级配置**，非实际搭载 | UHMNO(T92E1) 发射弹只匹配 105mm 初级炮弹表（7df2a/7e12a/7e02a），WI `gun_id=259620` = 152mm 顶级炮 WG item id；回放全流模块 id varint 零命中（2026-09-25，见未解析清单死路表） |
| WG item id ↔ tanks.pb module_id | 同 24 位局部 id，低字节 = 国家序×16+类别码（chassis=2/turret=3/gun=4；BlitzKit 侧恒 +1）。BZ-75：turret 16433→16435、gun 19249→19252；T92E1：gun 259617→259364(105)/259620(152) 实测对号 |
| segment = [result][layer][hash6] | 8 发 u64 小端字节解码 |
| blob 布局 16 字段 | 可控变量探针逐一定位（shooter@2、target@22、segment@46、distance@54） |
| blob 姿态单位 = 度、来源 = prop2 | 126 × 0.3515625° 精确整步；21/21 流序 prop2 逐位匹配 |
| 服务端重编码 segment | 同发 battle.json vs blob 字节对照（61c2→7700） |
| hash6 = P1/P2 两点编码 | 客户端反汇编 @0x1334E30 + 48 种轴映射穷举校准（hull 0.09~0.28m） |
| WI 判定射线 = 炮管轴反向 | 活体面板 ray 反向方位/仰角与 blob 姿态角精确吻合（44.29°/−2.25° 等） |
| 字节差分斜率微调 | 输入篡改差分实验 V1/V3/A 线性拟合（方位 23.57°/18.13° 复现） |
| WI 射线 = 检视视线 | 相机位于炮管反向延长线上方俯视；横向偏移不在协议字段 |
| 游戏原始回放可得 | replays 站公开下载 ZIP，项目解析器直读成功 |
| `c=` 分享码 | base64url(raw-deflate(JSON)) 解码实测；React 层 hasShot 忽略 p |
| 活体状态 | `_execute` Lua 读 scene.shotSimulate* 实测 |
| 数据包解密 | ZipCrypto 已知明文攻击（bkcrack）409/409 CRC 通过；UI Lua 仅调 simulateShot |

## 九、遗留未定项

- **type=32 尾 8 字节布局**：一例实测 `02 3d ff 00 08 35 2a 01`（shell 大端 u24@[4..7)、
  result@[7]）与早前 `[result][shell u24 LE][00][X][Y][Z]` 读法冲突，需按 26/27B 变体重推；
- **armor 端 `simulateShot` 的模拟种子与锚点**：可继续静态分析 armor.wat（85MB，
  func 8525 调用链已定位）或修复启动竞态后活体取证（`tmp_wi_js/harness/` 可复用）；
- **WI 判定射线与 hash6 的精确关系**：差分实验证明字节影响射线，但活体样本中
  两者曾简并（差 1°），公式形态（全三点 or 差分斜率）未逐位定案——本项目按 5.1
  解码弦实现（服务器编码语义），与 WI 的近似无关。
