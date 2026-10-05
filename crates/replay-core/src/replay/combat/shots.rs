//! 射击复现提取：ShotReplayData 数据面、ShotScanShared 共享扫描、
//! 作者严格路径与他人宽松路径（合并方案见 docs/architecture-debt.md 第 2 节）。

use super::*;
use crate::replay::filter::FilteredTimeline;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 单发射击的数据质量标注（宽松降级与快照陈旧度的可视化依据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotQuality {
    /// 射手状态快照距开火时刻的偏移（ms，负=早于开火；|值|大 = type=10 稀疏）
    pub shooter_state_dt_ms: i32,
    /// 射手位置为炮口坐标兜底（type=10 快照缺失，AoI 裁剪；见 ShooterAimData 注）
    #[serde(skip_serializing_if = "is_false")]
    pub shooter_pos_from_muzzle: bool,
    /// 目标状态采样距命中包时刻的偏移（ms，≤0 = 状态为命中批次前最后已知值）；脱靶弹无目标 = None。
    /// 命中弹的状态来源 = method8 通知处理时刻受击者的运行状态（WI 对齐锚点）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_state_dt_ms: Option<i32>,
    /// 炮塔朝向降级为车体朝向（type=7 prop2 缺失）："shooter" / "target"
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub turret_degraded: Vec<String>,
    /// 命中弹未在血量链找到伤害区间（服务器未记账 HP，damage 可能为 0）
    #[serde(skip_serializing_if = "is_false")]
    pub dmg_unattributed: bool,
    /// 锚点快照来源（仅降级/回退路径序列化，供 UI 徽章告警；正常路径不序列化）：
    /// - shooter："nearest"=最近包（AoI 稀疏）、"extrapolated"=段末速度外推；"filtered"（段内插值）=正常不序列化；
    /// - target："nearest"/"filtered"/"extrapolated" 均为 method8 命中通知缺失后的回退路径；
    ///   "wi_hit_state"（method8 通知状态）= 正常路径不序列化。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shooter_anchor_src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_anchor_src: Option<String>,
    /// 弹种来自 method0x07 开火广播兜底（type=32 命中通知未转发）；false = segment 权威来源
    #[serde(skip_serializing_if = "is_false")]
    pub shell_from_broadcast: bool,
    /// 弹种来自 method0x1b 地形命中广播兜底（0x07 亦未覆盖：他人脱靶弹/作者 0x07 空窗）；
    /// 同时意味着 terrain_impact 附带精确落点（撞静态物的弹无 0x1b，不适用）
    #[serde(default, skip_serializing_if = "is_false")]
    pub shell_from_terrain: bool,
    /// 射手炮管俯仰由发射速度向量推算（prop2 缺失回退；作者路径恒 false——作者回退走 method36 field2）
    #[serde(skip_serializing_if = "is_false")]
    pub shooter_pitch_from_velocity: bool,
    /// 射手炮管俯仰回退到 method36 field2（车体系炮管俯仰，WotbTools PROVEN；仅作者路径 prop2 缺失时）
    #[serde(default, skip_serializing_if = "is_false")]
    pub shooter_pitch_from_method36: bool,
    /// 炮管俯仰回退（prop2 frac 不可得）："shooter"=射手回退速度向量/method36 field2，"target"=受击方回退车体 pitch
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gun_pitch_degraded: Vec<String>,
    /// 俯仰采样流陈旧（prop2 断流 >2s，AoI 边界/补发簇；frac 恒定本身是炮管定点/贴
    /// 极限的如实上报，不算冻结）："shooter" / "target"
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pitch_frozen: Vec<String>,
}

/// 一次射击事件的"复现数据"：双方位置/朝向（type=10 实体状态包解码），供 3D 查看器复现热力图视角。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotReplayData {
    pub index: usize,
    pub time_s: f32,
    pub damage: u32,
    pub target_name: String,
    /// 受击方实体 id（作者 = method38 受击者，服务器权威；他人 = method8 直击通知）。
    /// 身份域与显示域（[`Self::target_name`]）解耦——投影/消费方联表一律用 eid；
    /// 名字缺失不作为提取失败条件（fail-soft）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_eid: Option<u32>,
    pub is_kill: bool,
    /// 射手实体 id（method29 shooterEntityId；作者路径恒为作者本人实体）
    pub shooter_eid: u32,
    /// 射手昵称（type=5 名册；未知为空串）
    #[serde(skip_serializing_if = "String::is_empty")]
    pub shooter_name: String,
    /// 是否回放作者本人的射击。false = 其他玩家：命中结果来自 method8 枚举 + 血量链伤害（无 method38 细节）
    #[serde(skip_serializing_if = "is_false")]
    pub is_author: bool,
    /// 射手（作者玩家实体）位置 [x, y(离地), z]
    pub shooter_pos: [f32; 3],
    /// 射手朝向（3 个浮点，语义为最可能猜测：偏航/俯仰/侧倾，弧度）
    pub shooter_ang: [f32; 3],
    pub target_pos: [f32; 3],
    pub target_ang: [f32; 3],
    /// 目标炮塔绝对朝向（弧度，与 hull yaw 同参考系，顺时针为正）；来源 type=7 sub=2 高 10 位粗值，
    /// ang = (u16>>6)/1024×2π − π（实测校准有 180° 偏移；低 6 位 = 炮管俯仰比例，不参与偏航）。
    pub target_turret_yaw: f32,
    /// 受击方炮管俯仰（弧度，炮塔系，正=仰角）；来源 prop2 低 6 位 frac：
    /// pitch = ele − frac/63×(dep+ele)（按车型极限锚定，frac=63 ↔ 俯角极限、0 ↔ 仰角极限）。
    /// 回退（无 prop2 采样/无车型极限）：车体 pitch（type10，正=车头下坡，语义不同仅兜底），
    /// 见 quality.gun_pitch_degraded。流断流 >2s 时 quality.pitch_frozen 提示陈旧。
    pub target_gun_pitch: f32,
    /// 存在 type=32 服务器解码的抵达成角（来向方位角校验通过，正=仰角）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub target_gun_pitch_server: bool,
    /// 恒 0（占位兼容字段）。type=32 通知包内**无**受击者炮塔角——26/27B 的
    /// 字节9=每实体序号、26B 字节11=cmpIndex、27B 字节10(7bit) 均非炮塔角；
    /// 主用 target_turret_yaw（prop2）。
    pub type32_turret_yaw: f32,
    /// 射手炮塔绝对朝向（弧度）= sub2_rel + shooter_hullYaw，用于精确入射方位角（替代位置差推算）。
    pub shooter_turret_yaw: f32,
    /// 射手炮管俯仰（弧度，炮塔系，正=仰角）；来源与受击方同源 = prop2 frac 解码（按射手车型极限）。
    /// 回退：作者 = method36 field2 车体系炮管俯仰（quality.shooter_pitch_from_method36）；
    /// 他人 = 发射速度向量反解（quality.shooter_pitch_from_velocity）。
    pub shooter_gun_pitch: f32,
    /// 弹着点相对【命中通知状态目标位置】的偏移 [x, y, z]（米）；来源 type=8 method20（shotId 配对）。
    /// 注意：method20 为弹道终点（穿透后出射/停止点，可在目标另一侧）。
    pub aim_point: [f32; 3],
    /// 炮口位置相对【命中通知状态目标位置】的偏移（与 aim_point 同基准）；viewer 与 launch_velocity 组合成弹道射线。
    pub launch_point_rel: [f32; 3],
    /// 弹道两点（回放世界系，米）：ball_a = 炮口发射位置（method29），ball_b = 弹道终点（method20，shotId 配对）；
    /// 两点确定弹道直线——viewer 用方向做 raycast，轴映射只需一次方向变换。
    pub ball_a: [f32; 3],
    pub ball_b: [f32; 3],
    /// 发射速度向量 [vx, vy, vz]（m/s）；method29 launchVelocity 服务器权威弹道方向（含俯仰），
    /// 与 launchPoint→终点连线夹角实测 <0.1°。
    pub launch_velocity: [f32; 3],
    /// 命中结果位图（u32 = flags16 | headerHi16<<16；wotinspector hit_flags 同源）。
    /// 全 16 位命名见 [`hit_flags_mod`]（WotbTools 全位 PROVEN）：0x0001 直接击杀 / 0x0002 目标已死 /
    /// 0x0004 起火 / 0x0008 跳弹 / 0x0010 材料击穿 / 0x0020 未击穿 / 0x0040/0x0080 间隙层穿与未穿 /
    /// 0x0100/0x0200 模块穿与未穿 / 0x0400 履带 / 0x0800 火炮 / 0x1000-0x8000 爆炸分支。
    /// headerHi 保留 raw（多数 0x0002=录像者关联位；Maus 边界 0x0012/0x0028，禁当命中位解码）。
    pub hit_flags: u32,
    /// 模块受损位掩码——wotinspector crit_modules 同源；bit = componentToken - 31（token 31..43 → bit 0..12；实测 token33 受损 → WI bit2=0x04 ✓）。
    pub crit_modules: u32,
    /// 模块摧毁位掩码（映射同上，state=2；实测 token35 履带摧毁 → bit4）。
    pub destroyed_modules: u32,
    /// 游戏原生命中段 u64（type=32 警告包尾 8 字节 LE）：`[result u8][shell_global_id u24 LE][0x00][X][Y][Z]`。
    /// - result：命中结果枚举（同 game_hit_result）。
    /// - shell_global_id（u24 LE）= (shells.xml 局部 id << 8) | 国家基数（nation_id×16+10：uk=0x5a、japan=0x6a、usa=0x2a）。
    /// - 字节4 恒 0x00；末 3 字节语义未解（服务器不下发片元编号）；服务器仅转发部分命中通知，0 = 未获取。
    pub segment: u64,
    /// 命中弹种全局 id（24 位，含国家基数字节；与 WI shell_id 同值同源；0 = 未获取）
    pub shell_id: u32,
    /// 弹种原始串（tanks.pb shells.xml：ap/ap_cr/heat/he 及 *_premium 修饰；ShellKindTable 按
    /// shell_id 全局 id 回填，兜底链各级均可命中；未识别为空串）。前端自行做标签映射与金弹判定，勿在此归一化。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shell_kind: String,
    /// segment[7]（B7）= 命中装甲板 plateId；viewer 片元验证 fragOk 用同源值
    pub armor_group: u8,
    /// segment[5..7] 组成的 u16 BE——非三角形索引：res=1 未穿时恒负值（可作未穿
    /// 判定冗余位）、res=0 恒 ~71-83；参照系未解，保留透传
    pub hit_triangle: u16,
    /// 游戏命中结果枚举（method8 b9 / type=32 segment 低字节同源）：
    /// 0=无命中结果 1=未击穿 2=间隙层止 3=有伤害（击穿/HE 爆炸）4=履带/模块交互
    /// （非跳弹——与内部模块穿旗标共存）；255 = 未获取。
    pub game_hit_result: u8,
    /// method8 ↔ type=32 同事件共享的 6 字节 = 游戏客户端 DecodeShotSegment 两点编码
    /// （出入点的部件 AABB 量化坐标，权威定义见《回放与射击逆向总集》第三篇 §二）；
    /// 3D 查看器据此标注出入点并构建 P1→P2 判定射线。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_token: Option<String>,
    /// 特殊弹药效果 ID（WotbTools PROVEN：1=精准火力 2=钨芯弹，可同发共存）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<u32>,
    /// 开火时刻（秒）——与 method29 发射包确定性匹配
    pub fire_time: f32,
    /// Shot 顶层身份：unique shotId = 一次开火。method29 发射 ↔ method20 终点以此确定性配对；
    /// 同一 shotId 的后续 method29 / 多次装甲交互不得展开成额外 Shot。
    pub shot_id: u32,
    /// 弹药槽位——type=28 选择状态在发射时刻的值（3D 视图弹种选择器索引用）
    pub shell_slot: u32,
    /// type=35 服务器竞技场 tick 计数器 @ 开火时刻（10Hz u8 递增，回放时钟秒×10）；开火 tick 判定 100/100 实测对齐。
    pub fire_tick: f32,
    /// 受击坦克 type=10 采样（命中 ±1s，位置相对命中通知状态锚点，世界系米；dt 锚定命中包时刻）。
    /// 末项可能为合成"游戏渲染位"采样（render=true，dt=0，滤波器命中帧输出，viewer ◎渲染位）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tick_samples: Vec<TickSample>,
    /// 射手坦克 type=10 采样（开火 ±0.2s，世界系绝对坐标）；末项可能为合成渲染位采样（render=true）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shooter_tick_samples: Vec<TickSample>,
    /// 地形命中数据（Avatar method 0x1b；全局广播含所有玩家脱靶弹，shotId 精确配对；
    /// 仅当该发未命中任何坦克且撞到地形时存在，约覆盖 2/3 地形弹）。
    /// args(34) = [shotId u32][shell_global_id u32][material u8][impactPoint 3×f32][terminalDir 3×f32][tail u8]。
    /// impact_point == method20 弹道终点；terminal_dir = 弹道末段速度方向向量（与发射速度同向）；
    /// material 落点材质类（观测 0/1/2/4/5，命名未定）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terrain_impact: Option<TerrainImpactData>,
    /// 开火时刻瞄准快照（Avatar method36，缺失时 None）。炮塔相对偏航为 f64 全精度（prop2 为 u16 量化）；
    /// state_before/after 为未定名状态常量（非扩散度，见 ShooterAimData 注）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shooter_aim: Option<ShooterAimData>,
    /// 射手车辆配件（Type5 物化 9 字节选择串；calib shells / enhanced armor 供查看器
    /// 自动穿深/厚度系数，替代手动勾选）。物化缺失（AoI 未覆盖）时 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shooter_equipment: Option<VehicleEquipment>,
    /// 受击方车辆配件（脱靶弹无目标时 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_equipment: Option<VehicleEquipment>,
    /// 兼容旧字段：= type32_turret_yaw（曾误标为"来袭方向"，实为受击者炮塔角）。
    pub incoming_yaw: f32,
    /// 兼容旧字段：= target_gun_pitch（曾误标为"来袭俯角"，实为受击者炮管俯仰）。
    pub incoming_pitch: f32,
    /// 数据质量标注（快照陈旧度/降级项；作者路径填快照偏移，他人路径另含降级标记）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<ShotQuality>,
    /// 服务器下发的受击部件索引 cmpIndex（method8 args[10]；0=底盘/履带 1=车体 2=炮塔 3=炮管，
    /// 命中高度分层实证）——游戏 showDamageFromShot/DecodeShotSegment 用它在指定部件上放
    /// 弹着点；与本地 raycast 的部件选择对照 = 命中位置偏差的校准基准。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_part_index: Option<u8>,
    /// 射手渲染层锚点（客户端位置滤波器输出；None = type=10 采样缺失不可构建时间线）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shooter_render: Option<RenderAnchorData>,
    /// 受击者渲染层锚点（仅命中弹；"玩家当时看到的"受击者位姿）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_render: Option<RenderAnchorData>,
    /// 受击方渲染时间线（滤波器输出 0.1s 降采样，命中 −3.0~+2.0s，pos 绝对世界坐标）——
    /// 滑块严格对齐游戏每帧实际显示位姿（含 latency 移位/误差盒钳位/外推）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_render_timeline: Vec<RenderTimelineSample>,
    /// 射手方渲染时间线（开火 −2.0~+2.0s，pos 绝对世界坐标）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shooter_render_timeline: Vec<RenderTimelineSample>,
    /// 受击方炮塔相对角时间线（prop2，命中 −3.0~+2.0s，[dt, rel_yaw 弧度]）——滑块实时驱动炮塔
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_turret_timeline: Vec<(f32, f32)>,
    /// 射手方炮塔相对角时间线（prop2，开火 −2.0~+2.0s，[dt, rel_yaw 弧度]）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shooter_turret_timeline: Vec<(f32, f32)>,
    /// 射手炮管俯仰时间线（prop2 frac 解码，开火 −2.0~+2.0s，[dt, 弧度，正=仰角]）；
    /// 无车型极限锚定时作者路径回退 method36 field2（车体系炮管俯仰，弧度）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shooter_gun_timeline: Vec<(f32, f32)>,
    /// 受击方炮管俯仰时间线（prop2 frac 解码，命中 −3.0~+2.0s，[dt, 弧度，正=仰角]）——
    /// 滑块实时驱动炮管（与 target_turret_timeline 配对）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_gun_timeline: Vec<(f32, f32)>,
}

/// method 0x1b 地形命中数据（字段语义见 [`ShotReplayData::terrain_impact`]）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainImpactData {
    /// 落点材质类（0/1/2/4/5 观测，具体命名未定）
    pub material: u8,
    /// 精确落点（回放世界系，米；== method20 弹道终点）
    pub impact_point: [f32; 3],
    /// 弹道末段速度方向向量（世界系；与发射速度同向，norm 非单位长度，非位置点）
    pub terminal_dir: [f32; 3],
}

/// Avatar method36 (0x24) 开火时刻瞄准快照（可选，无快照则 None）；args = [payloadLen u8][protobuf]。
/// field1(f64)=炮塔相对车体偏航（与 prop2 同语义）；开火时刻成对出现（射击前/后各一条，f1 恒同）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShooterAimData {
    pub turret_rel_yaw: f64,
    /// 成对快照 field6.field1（射击前；语义未定，透传）
    pub state_before: f64,
    /// 成对快照 field6.field1（射击后；无成对快照时 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_after: Option<f64>,
    /// root.field2 = 炮管俯仰（车体系 rad，WotbTools PROVEN）——作者俯仰权威来源（prop2 锚定缺失时的回退也走它）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gun_pitch: Option<f64>,
    /// type39 f0 世界系炮线 yaw（rad）——viewer 画真实 3D 炮线
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world_gun_yaw: Option<f32>,
    /// type39 f1 世界系炮线 pitch（rad，存储取负，还原时取反）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world_gun_pitch: Option<f32>,
}

/// method36 (0x24) 单快照解码结果（WotbTools PROVEN 全字段；根标量均为 fixed64）。
#[derive(Debug, Clone, Default)]
pub struct AimSnapshot {
    /// root.field1 = 炮塔/炮管相对车体偏航（rad；与 prop2 同语义 f64 全精度）
    pub turret_rel_yaw: Option<f64>,
    /// root.field2 = 炮管俯仰（车体系 rad）
    pub gun_pitch: Option<f64>,
    /// root.field3 = 水平炮塔最大角速度（rad/s 车型常量）
    pub yaw_speed_limit: Option<f64>,
    /// root.field4 = 垂直炮管最大角速度（rad/s 车型常量；火炮受损 ×0.675，修复复原）
    pub pitch_speed_limit: Option<f64>,
    /// root.field5 = 瞄准时间物理标量（Reticle Calibration 恰 ×0.70）
    pub aim_time: Option<f64>,
    /// field6.field1 = 动态扩散/开花 bloom（开火后正跳；火炮受损 ×2）
    pub bloom: Option<f64>,
}

/// 解析 method36 args → [`AimSnapshot`]。初始化变体（payloadLen=73）缺 root.field1/field2，
/// 相应字段为 None；格式不合法返回全 None（fail-soft，调用方忽略）。
fn parse_method36(args: &[u8]) -> AimSnapshot {
    let mut out = AimSnapshot::default();
    if args.is_empty() {
        return out;
    }
    // args[0] = payload 长度前缀（= args.len()-1），容错取 min
    let end = (args[0] as usize + 1).min(args.len());
    let payload = &args[1..end];
    let fields = match proto_fields(payload) {
        Some(f) => f,
        None => return out,
    };
    let fixed64 = |b: &[u8], f: &(u32, u8, usize, usize)| {
        f64::from_le_bytes(b[f.2..f.2 + 8].try_into().unwrap())
    };
    for f in &fields {
        if f.1 != 1 {
            continue;
        }
        match f.0 {
            1 => out.turret_rel_yaw = Some(fixed64(payload, f)),
            2 => out.gun_pitch = Some(fixed64(payload, f)),
            3 => out.yaw_speed_limit = Some(fixed64(payload, f)),
            4 => out.pitch_speed_limit = Some(fixed64(payload, f)),
            5 => out.aim_time = Some(fixed64(payload, f)),
            _ => {}
        }
    }
    out.bloom = fields.iter().find(|f| f.0 == 6 && f.1 == 2).and_then(|f| {
        let sub = &payload[f.2..f.2 + f.3];
        proto_fields(sub)?
            .iter()
            .find(|s| s.0 == 1 && s.1 == 1)
            .map(|s| fixed64(sub, s))
    });
    out
}

/// method38 (0x26) 命中反馈（作者 Avatar 专属）：args 布局（WotbTools PROVEN）：
/// [victimVehicleId u32][resultFlags16 u16][headerHi16 u16][resultCount u8][resultCount × (componentToken u8 + rawState u8)]
/// [modifierCount u8][modifierCount × modifierId u32]；rawState（WotbTools PROVEN）：1=受损或乘员受伤
/// 2=critical/禁用族（"摧毁"过强，是否=摧毁 PARTIAL）0=命中/参与但无新持久负面（"无变化"过强）；
/// modifierId 加性叠加可并存 [1,2]；组件号 31=引擎 32=弹药架 33=油箱 34/35=右/左履带 36=火炮
/// 37=炮塔旋转机 38=观察装置 39=车长 40=驾驶员 41=炮手 42=UNKNOWN 禁猜 43=装填手（WotbTools 枚举）
#[derive(Debug, Clone)]
pub(crate) struct HitFeedback {
    t: f32,
    victim: u32,
    flags: u32,
    crit_modules: u32,
    destroyed_modules: u32,
    components: Vec<(u8, u8)>,
    modifiers: Vec<u32>,
}

/// 两条射击提取路径（作者严格 / 他人宽松）的共享预分析产物：一次收集、两路复用，
/// `ReplayModel::scan` 亦从此取位姿/血量/名字索引，整场分析只此一份。
pub(crate) struct ShotScanShared {
    /// 全部 Shot 的 primary method29（作者 + 他人；按发射时刻排序，shotId 全局去重）。
    /// unique shotId = 一次开火；同 shotId 的后续 method29 不增加 Shot 数。
    pub launches: Vec<LaunchEntry>,
    /// 每射手首个 args<37 包的 args_len（作者路径 fail-fast 证据；正常回放为空表）
    pub short_args: HashMap<u32, usize>,
    /// method20 弹道终点（shotId 配对，含 miss 的空地终点）
    pub endpoints: HashMap<u32, (f32, [f32; 3])>,
    /// method8 直击通知（全局广播；按时钟排序）
    pub direct_hits8: Vec<DirectHit8>,
    /// type=32 命中通知（AoI 广播含他人；保持包序——hash6 令牌配对与顺序无关）
    pub warnings32: Vec<ArenaWarning32>,
    /// 0x1b 地形命中（shotId 配对，含全部玩家脱靶弹）
    pub terrain_impacts: HashMap<u32, (u32, TerrainImpactData)>,
    /// type=5 昵称表（eid → 昵称）
    pub names: HashMap<u32, String>,
    /// Type5 物化 9 字节配件选择
    pub vehicle_equipment: HashMap<u32, [u8; 9]>,
    /// type=35 tick 计数器（保持包序）
    pub tick_timeline: Vec<(f32, u8)>,
    /// per-entity type=10 状态采样（**已按 clock 稳定排序**——锚点选择与 roll 线性插值依赖时序）
    pub st10: HashMap<u32, Vec<St10Sample>>,
    /// per-entity prop2 打包角流（**已按 clock 稳定排序**，等钟保持包序——prop2_at 的
    /// 夹逼插值语义本就假设时钟序，排序同时让二分查找成立）
    pub prop2: HashMap<u32, Vec<(f32, f32, u16)>>,
    /// type=7 刷新簇时钟（AoI 补发/通道切换签名）
    pub refresh_clusters: HashMap<u32, Vec<f32>>,
    /// method1 血量事件（全实体、按时钟排序）
    pub hp_events: Vec<HpEvent>,
    /// type=5 满血锚点（血量链 seed）
    pub initial_hp: HashMap<u32, (f32, u16)>,
    // —— 以下为作者路径（Avatar 专属包）数据，他人路径不消费 ——
    /// method38 命中结果（同钟同受击者合并后，按时钟排序）
    pub hit_results38: Vec<HitFeedback>,
    /// type=28 弹药槽位选择时间线（按时钟排序）
    pub ammo_selects: Vec<(f32, u32)>,
    /// method 0x07 弹种广播时间线（按时钟排序；a0=0/1 恒成对同值，a0=18 非 弹种数据已排除）
    pub shell_broadcasts: Vec<(f32, u32)>,
    /// method36 瞄准快照（布局合法帧，按时钟排序）
    pub aim_snapshots: Vec<(f32, AimSnapshot)>,
    /// 作者俯仰回退序列（method36 field2 车体系俯仰，rad）——prop2/锚定缺失时的俯仰源
    pub aim_pitch_series: Vec<(f32, f32)>,
    /// type=39 作者瞄准/炮线帧（世界系 yaw/pitch）
    pub type39_frames: Vec<Type39Frame>,
    /// 作者伤害计数器（type=7 sub=10）增量序列（已剔除非弹伤害污染 tick）
    pub dc_increments: Vec<(f32, u32)>,
}

/// 一次包扫描构建 [`ShotScanShared`]（各收集器独立成 pass，语义与原逐路径收集逐位一致；
/// `author_player_eid` 仅用于 Avatar 专属流的作者过滤，0 = 未解析作者）。
pub(crate) fn build_shot_scan_shared(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
) -> ShotScanShared {
    let (launches, short_args) = collect_launches(packets, |_| true);
    let endpoints = collect_endpoints(packets);
    let direct_hits8 = collect_direct_hits8(packets);
    let warnings32 = collect_warnings32(packets);
    let terrain_impacts = collect_terrain_impacts(packets);
    let names = extract_entity_names(packets);
    let vehicle_equipment = collect_vehicle_equipment(packets);
    let tick_timeline = collect_tick_timeline(packets);
    let (mut st10, mut prop2) = build_entity_indexes(packets);
    for v in st10.values_mut() {
        v.sort_by(|a, b| a.clock.partial_cmp(&b.clock).unwrap());
    }
    for v in prop2.values_mut() {
        v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    }
    let refresh_clusters = collect_refresh_clusters(packets);
    let hp_events = parse_hp_events(packets);
    let initial_hp = collect_initial_hp(packets);

    // ③ method38 命中结果（Avatar 方法 = 仅作者自己的射击反馈）
    let mut hit_results: Vec<HitFeedback> = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 12 + 9 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x26 {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 9 || 12 + args_len > p.len() {
            continue;
        }
        let a = &p[12..12 + args_len];
        let mut crit_modules = 0u32;
        let mut destroyed_modules = 0u32;
        let mut components: Vec<(u8, u8)> = Vec::new();
        let mut off = 9usize;
        for _ in 0..a[8] {
            if off + 2 > args_len {
                break;
            }
            let (tok, state) = (a[off], a[off + 1]);
            components.push((tok, state));
            let bit = (tok as u32).checked_sub(31).map(|b| 1u32 << b).unwrap_or(0);
            match state {
                1 => crit_modules |= bit,
                2 => destroyed_modules |= bit,
                _ => {}
            }
            off += 2;
        }
        let mut modifiers: Vec<u32> = Vec::new();
        if off < args_len {
            let mcount = a[off];
            off += 1;
            for _ in 0..mcount {
                if off + 4 > args_len {
                    break;
                }
                modifiers.push(u32::from_le_bytes([
                    a[off],
                    a[off + 1],
                    a[off + 2],
                    a[off + 3],
                ]));
                off += 4;
            }
        }
        hit_results.push(HitFeedback {
            t: *clock,
            victim: u32::from_le_bytes([a[0], a[1], a[2], a[3]]),
            // 完整位图 = flags16 | headerHi<<16（wotinspector hit_flags 同源）
            flags: u32::from_le_bytes([a[4], a[5], a[6], a[7]]),
            crit_modules,
            destroyed_modules,
            components,
            modifiers,
        });
    }
    hit_results.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());
    // ③' 同钟同受击者合并：一发命中可产生多条结果消息（多次装甲交互，同钟重复计为一次命中事件）；
    // 位图取并集；组件按 token 取最大 state；modifiers 去重合并。
    let mut merged38: Vec<HitFeedback> = Vec::new();
    for h in hit_results {
        if let Some(last) = merged38.last_mut() {
            if (last.t - h.t).abs() <= 0.05 && last.victim == h.victim {
                last.flags |= h.flags;
                last.crit_modules |= h.crit_modules;
                last.destroyed_modules |= h.destroyed_modules;
                for (tok, st) in h.components {
                    match last.components.iter_mut().find(|(t, _)| *t == tok) {
                        Some(e) => e.1 = e.1.max(st),
                        None => last.components.push((tok, st)),
                    }
                }
                for m in h.modifiers {
                    if !last.modifiers.contains(&m) {
                        last.modifiers.push(m);
                    }
                }
                continue;
            }
        }
        merged38.push(h);
    }

    // ⑤' 弹药选择时间线（type=28，payload=u32 LE 槽位；录像者本人的选择状态）
    let mut ammo_selects: Vec<(f32, u32)> = Vec::new();
    for (t, clock, p) in packets {
        if *t != 28 || p.len() < 4 {
            continue;
        }
        ammo_selects.push((*clock, u32::from_le_bytes([p[0], p[1], p[2], p[3]])));
    }
    ammo_selects.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    // ⑤'' Avatar method 0x07 弹种广播时间线：args(5) = [a0 u8][shell_global_id u32 LE]。
    // 命中通知未转发时（含脱靶弹）以此兜底。
    let mut shell_broadcasts: Vec<(f32, u32)> = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 17 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x07 {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 5 || 12 + args_len > p.len() {
            continue;
        }
        if p[12] != 0 && p[12] != 1 {
            continue;
        }
        shell_broadcasts.push((*clock, u32::from_le_bytes([p[13], p[14], p[15], p[16]])));
    }
    shell_broadcasts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());

    // ⑤'''' Avatar method36 (0x24) 瞄准快照时间线（envelope = avatar = 录像者本人）；
    // args = [len u8][protobuf]，开火时刻成对；布局不合法的包 fail-soft 跳过（解码见 AimSnapshot）。
    let mut aim_snapshots: Vec<(f32, AimSnapshot)> = Vec::new();
    for (_, clock, p) in packets {
        if p.len() < 14 {
            continue;
        }
        if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 0x24 {
            continue;
        }
        let args_len = u32::from_le_bytes([p[8], p[9], p[10], p[11]]) as usize;
        if args_len < 2 || 12 + args_len > p.len() {
            continue;
        }
        let snap = parse_method36(&p[12..12 + args_len]);
        if snap.turret_rel_yaw.is_some() && snap.bloom.is_some() {
            aim_snapshots.push((*clock, snap));
        }
    }
    aim_snapshots.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
    let aim_pitch_series: Vec<(f32, f32)> = aim_snapshots
        .iter()
        .filter_map(|(c, s)| s.gun_pitch.map(|p| (*c, p as f32)))
        .collect();
    let type39_frames = collect_type39_frames(packets);

    // 作者伤害计数器（type=7 sub=10）增量序列——首次命中（血量链无前值）兜底；
    // 撞击/火伤等非弹伤害增量与 method1 cause≠0 且涉及作者的事件同批剔除
    let mut non_shell_ticks: Vec<f32> = hp_events
        .iter()
        .filter(|e| {
            e.cause != 0 && (e.source == author_player_eid || e.victim == author_player_eid)
        })
        .map(|e| e.clock)
        .collect();
    non_shell_ticks.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut dc_increments: Vec<(f32, u32)> = Vec::new();
    {
        let mut last_cum: u32 = 0;
        for (t, clock, p) in packets {
            if *t != 7 || p.len() < 16 {
                continue;
            }
            if u32::from_le_bytes([p[4], p[5], p[6], p[7]]) != 10 {
                continue;
            }
            let cum = u32::from_le_bytes([p[12], p[13], p[14], p[15]]);
            if cum > last_cum {
                // 二分判定 |clock - tc| <= 0.3 的污染 tick（non_shell_ticks 已排序）
                let lo = non_shell_ticks.partition_point(|tc| *tc < clock - 0.3);
                let polluted = lo < non_shell_ticks.len() && non_shell_ticks[lo] <= clock + 0.3;
                if !polluted {
                    dc_increments.push((*clock, cum - last_cum));
                }
                last_cum = cum;
            }
        }
    }

    ShotScanShared {
        launches,
        short_args,
        endpoints,
        direct_hits8,
        warnings32,
        terrain_impacts,
        names,
        vehicle_equipment,
        tick_timeline,
        st10,
        prop2,
        refresh_clusters,
        hp_events,
        initial_hp,
        hit_results38: merged38,
        ammo_selects,
        shell_broadcasts,
        aim_snapshots,
        aim_pitch_series,
        type39_frames,
        dc_increments,
    }
}

/// 作者路径 type=32 选择：method38 先确定 victim；同一 victim/命中时钟可能合法出现
/// 多个不同 segment（一次 Shot 的多次装甲交互）。method8 与 type=32 的 hash6 是同事件
/// 确定性令牌，因此有唯一 method8 token 时先按 token 收窄；重复 method8 广播按 hash6 去重。
/// token 缺失/不唯一/无法命中 type=32 时退回原 victim+time 严格判定，不猜选。
fn select_author_warning32<'a>(
    warnings32: &'a [ArenaWarning32],
    direct_hits8: &[DirectHit8],
    author_player_eid: u32,
    victim_eid: u32,
    end_time: f32,
) -> Result<Option<&'a ArenaWarning32>, usize> {
    let window: Vec<&ArenaWarning32> = warnings32
        .iter()
        .filter(|w| w.eid == victim_eid && (w.t - end_time).abs() <= 0.05)
        .collect();
    if window.is_empty() {
        return Ok(None);
    }

    let mut hashes: Vec<[u8; 6]> = Vec::new();
    for d in direct_hits8.iter().filter(|d| {
        d.shooter == author_player_eid && d.victim == victim_eid && (d.t - end_time).abs() <= 0.05
    }) {
        if !hashes.contains(&d.hash6) {
            hashes.push(d.hash6);
        }
    }

    let candidates: Vec<&ArenaWarning32> = if hashes.len() == 1 {
        let matched: Vec<&ArenaWarning32> = window
            .iter()
            .copied()
            .filter(|w| w.hash6 == hashes[0])
            .collect();
        if matched.is_empty() {
            window
        } else {
            matched
        }
    } else {
        window
    };

    let first = candidates[0];
    if candidates.iter().any(|w| w.segment != first.segment) {
        return Err(candidates.len());
    }
    Ok(Some(first))
}

// ---------- 作者/他人两路径的同构段共享实现 ----------
// 两条路径的本质差异只在"缺失时怎么办"（作者 bail / 他人跳过或兜底）；组装段收敛为
// 单份，时间窗常量（渲染受击 −3.0~+2.0 / 射手 −3.0~+2.0、炮塔角射手 −2.0~+2.0）集中在此。

/// 炮塔相对角：流序快照（method29/8 处理时刻，WI 同基准）优先 → 时钟序 prop2 兜底。
/// None = prop2 缺失（调用方按路径 bail 或降级为车体朝向）。
fn turret_rel_at(
    stream_snapshot: Option<(f32, u16)>,
    series: Option<&Vec<(f32, f32, u16)>>,
    t: f32,
) -> Option<f32> {
    match stream_snapshot {
        Some((_, v)) => Some(decode_prop2_u16(v).0),
        None => prop2_at_arrived(series, t).map(|(r, _)| r),
    }
}

/// prop2 frac 俯仰解码链（双方同构）：流序快照 × 车型锚定 → 时钟序 × 锚定。
/// None = prop2/锚定缺失——调用方按路径回退（作者射手回 method36 且仍缺失 fail-fast，
/// 他人射手回发射速度向量，受击方回车体 pitch），降级标记由调用方打。
fn pitch_from_prop2(
    stream_snapshot: Option<(f32, u16)>,
    series: Option<&Vec<(f32, f32, u16)>>,
    t: f32,
    limits: Option<&GunPitchRange>,
    pitch_frozen: &mut Vec<String>,
    who: &str,
) -> Option<f32> {
    if let Some(((_, v), lim)) = stream_snapshot.zip(limits) {
        let (y, fr) = decode_prop2_u16(v);
        if prop2_frac_frozen(series, t) {
            pitch_frozen.push(who.into());
        }
        return Some(decode_prop2_gun_pitch(fr, lim, y));
    }
    if let Some(((y, fr), lim)) = prop2_at_arrived(series, t).zip(limits) {
        if prop2_frac_frozen(series, t) {
            pitch_frozen.push(who.into());
        }
        return Some(decode_prop2_gun_pitch(fr, lim, y));
    }
    None
}

/// 单发射击的渲染层包（判定锚点 + 渲染锚点 + 四条时间线）。
struct ShotRenderPack {
    shooter_render: Option<RenderAnchorData>,
    target_render: Option<RenderAnchorData>,
    target_render_timeline: Vec<RenderTimelineSample>,
    shooter_render_timeline: Vec<RenderTimelineSample>,
    target_turret_timeline: Vec<(f32, f32)>,
    shooter_turret_timeline: Vec<(f32, f32)>,
    shooter_gun_timeline: Vec<(f32, f32)>,
    target_gun_timeline: Vec<(f32, f32)>,
}

/// 射手炮管俯仰时间线的无锚定回退：作者 = method36 field2 车体系俯仰序列；
/// 他人 = 空（发射速度向量只在开火帧存在，无时间线可言）。
enum ShooterGunFallback<'a> {
    Method36Series(&'a Vec<(f32, f32)>),
    None,
}

#[allow(clippy::too_many_arguments)] // 两路径组装的公共入参，聚合结构反而不透明
fn shot_render_pack(
    render_cache: &mut HashMap<u32, FilteredTimeline>,
    st10: &St10Index,
    prop2: &Prop2Index,
    shooter_eid: u32,
    fire_time: f32,
    shooter_judgment_pos: [f32; 3],
    skip_shooter: bool,
    target_eid: Option<u32>,
    hit: bool,
    end_time: f32,
    target_judgment_pos: [f32; 3],
    shooter_limits: Option<&GunPitchRange>,
    target_limits: Option<&GunPitchRange>,
    gun_fallback: ShooterGunFallback,
) -> ShotRenderPack {
    // 渲染层锚点（客户端位置滤波器）：滤波器输出 = 游戏画面里模型实际呈现的位姿；
    // 判定层锚点（调用方 sp/tp）保持不动——装甲命中几何必须用判定层。
    // 他人路径炮口兜底（pos_from_muzzle）时 sp 非车体位姿，跳过射手侧渲染数据。
    let shooter_render = if !skip_shooter {
        render_anchor(
            render_cache,
            shooter_eid,
            st10.get(&shooter_eid),
            fire_time,
            shooter_judgment_pos,
        )
    } else {
        None
    };
    let target_render = if hit {
        target_eid.and_then(|teid| {
            render_anchor(
                render_cache,
                teid,
                st10.get(&teid),
                end_time,
                target_judgment_pos,
            )
        })
    } else {
        None
    };
    // 渲染时间线（滑块严格对齐游戏每帧显示位姿，0.1s 步长）：受击方命中 −3.0~+2.0s、
    // 射手方开火 −3.0~+2.0s。命中后数据保留：滤波器误差盒钳位会渐进滑向通道切换后
    // 的真相，即游戏当时渲染的画面。
    let target_render_timeline = if hit {
        target_eid
            .map(|teid| {
                render_timeline(
                    render_cache,
                    teid,
                    st10.get(&teid),
                    end_time,
                    -3.0,
                    2.0,
                    0.1,
                )
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let shooter_render_timeline = if !skip_shooter {
        render_timeline(
            render_cache,
            shooter_eid,
            st10.get(&shooter_eid),
            fire_time,
            -3.0,
            2.0,
            0.1,
        )
    } else {
        Vec::new()
    };
    // 炮塔/炮管实时时间线（prop2，客户端语义 0.1s 网格，与锚点同一求值器）：
    // 受击方命中 −3.0~+2.0s、射手方开火 −2.0~+2.0s
    let target_turret_timeline = if hit {
        target_eid
            .and_then(|teid| prop2.get(&teid))
            .map(|series| timeline_prop2_client(Some(series), end_time, -3.0, 2.0, None))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let shooter_turret_timeline =
        timeline_prop2_client(prop2.get(&shooter_eid), fire_time, -2.0, 2.0, None);
    let shooter_gun_timeline = match shooter_limits {
        Some(lim) => {
            timeline_prop2_client(prop2.get(&shooter_eid), fire_time, -2.0, 2.0, Some(lim))
        }
        None => match gun_fallback {
            ShooterGunFallback::Method36Series(series) => {
                timeline_1f(Some(series), fire_time, -2.0, 2.0)
            }
            ShooterGunFallback::None => Vec::new(),
        },
    };
    let target_gun_timeline = if hit {
        target_eid
            .and_then(|teid| prop2.get(&teid))
            .map(|series| timeline_prop2_client(Some(series), end_time, -3.0, 2.0, target_limits))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    ShotRenderPack {
        shooter_render,
        target_render,
        target_render_timeline,
        shooter_render_timeline,
        target_turret_timeline,
        shooter_turret_timeline,
        shooter_gun_timeline,
        target_gun_timeline,
    }
}

/// type=10 采样窗口（base_t ±1s，锚点/补发簇截断 + "游戏渲染位"合成）——
/// 受击方相对判定锚点（relative=true），射手世界系绝对坐标。
/// 窗口右端 +0.09 仅作锚点兜底：有真锚点（|dt|<0.05）时 trim 截断其后样本——
/// 命中/开火后数据包源切换（AoI 远端基线），其后首包含数米级瞬移；
/// 射手前窗取 −1.0s：过窄时靠前 tick 钳死在开火位置呈现"不动"假象。
#[allow(clippy::too_many_arguments)] // 两路径组装的公共入参（同 shot_render_pack）
fn tick_window_samples(
    st10: &St10Index,
    tick_timeline: &[(f32, u8)],
    refresh_clusters: &HashMap<u32, Vec<f32>>,
    eid: u32,
    base_t: f32,
    anchor_pos: [f32; 3],
    relative: bool,
    render: Option<&RenderAnchorData>,
) -> Vec<TickSample> {
    let mut out = Vec::new();
    if let Some(samples) = st10.get(&eid) {
        for s in samples {
            let dt = s.clock - base_t;
            if !(-1.0..=0.09).contains(&dt) {
                continue;
            }
            let pos = if relative {
                [
                    s.pos[0] - anchor_pos[0],
                    s.pos[1] - anchor_pos[1],
                    s.pos[2] - anchor_pos[2],
                ]
            } else {
                s.pos
            };
            out.push(TickSample::raw(
                dt,
                tick_at(tick_timeline, s.clock),
                pos,
                s.yaw,
                s.pitch,
                s.roll,
            ));
        }
    }
    trim_tick_samples(
        &mut out,
        refresh_cluster_after(refresh_clusters, eid, base_t),
    );
    if let Some(r) = render {
        let pos = if relative {
            [
                r.pos[0] - anchor_pos[0],
                r.pos[1] - anchor_pos[1],
                r.pos[2] - anchor_pos[2],
            ]
        } else {
            r.pos
        };
        out.push(TickSample::render_ghost(
            0.0, pos, r.ang[0], r.ang[1], r.ang[2],
        ));
    }
    out
}

/// 从射击事件抽取复现数据（WotbTools 权威弹丸生命周期，全部 shotId 确定性配对）：
/// method29 (0x1d) 发射 / method20 (0x14) 终点 / method38 (0x26) 命中结果（仅作者）；目标 = method38 victimVehicleId（服务器权威，无则 miss）；
/// 伤害 = method1 血量链差值（victim + source=作者 + cause=0）。数据完整性 fail-fast：任何缺失/歧义直接返回 Err，不做保守降级（零值掩盖问题）。
/// 便捷入口（作者 eid 版）外部调用面；当前管线走 [`extract_shot_replays_with_limits`]。
#[allow(dead_code)]
pub fn extract_shot_replays(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
) -> anyhow::Result<Vec<ShotReplayData>> {
    extract_shot_replays_with_limits(packets, author_player_eid, &GunPitchLimits::new())
}

/// [`extract_shot_replays`] 的完整形态：`pitch_limits` = 昵称→(俯角°,仰角°) 锚定表
/// （[`gun_pitch_limits_from`] 构建）。双方炮管俯仰主来源 = prop2 frac 比例解码；
/// 无锚定表时回退路径（作者 method36 field2 / 他人速度向量 / 受击方车体 pitch）并打质量标记。
pub fn extract_shot_replays_with_limits(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
    pitch_limits: &GunPitchLimits,
) -> anyhow::Result<Vec<ShotReplayData>> {
    let shared = build_shot_scan_shared(packets, author_player_eid);
    let mut render_cache: HashMap<u32, FilteredTimeline> = HashMap::new();
    extract_shot_replays_from_shared(&shared, author_player_eid, pitch_limits, &mut render_cache)
}

/// [`extract_shot_replays_with_limits`] 的共享扫描形态：预分析产物由调用方一次构建
/// （[`build_shot_scan_shared`]），作者/他人两路复用；`render_cache`（滤波时间线，
/// 按实体确定）亦可跨路复用。
pub(crate) fn extract_shot_replays_from_shared(
    shared: &ShotScanShared,
    author_player_eid: u32,
    pitch_limits: &GunPitchLimits,
    render_cache: &mut HashMap<u32, FilteredTimeline>,
) -> anyhow::Result<Vec<ShotReplayData>> {
    if author_player_eid == 0 {
        anyhow::bail!("无法解析作者实体：meta.player_name / battle_results 昵称与 type=5 实体昵称匹配失败（作者实体未在录像中出现或昵称无效）");
    }

    // ① 作者的 method29 发射事件（共享发射表按 shooter 过滤）；args<37 = 布局漂移，fail-fast
    let launches: Vec<LaunchEntry> = shared
        .launches
        .iter()
        .filter(|l| l.shooter == author_player_eid)
        .cloned()
        .collect();
    if let Some(&args_len) = shared.short_args.get(&author_player_eid) {
        anyhow::bail!(
            "作者的 method29 发射包 args 长度 {} < 37（回放版本布局漂移？）",
            args_len
        );
    }

    // ①' 段链分组：同 (shooter, shotId) 的后续 method29 = 同一发的跳弹/穿透续段。
    // 发射段 = 链首（唯一进入组装），续段首点 = 命中/跳弹点（渲染终点），续段本身不是独立射击。
    let (launch_is_cont, next_segment) = collect::shot_segments(&launches);
    let launches: Vec<LaunchEntry> = launches
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !launch_is_cont[*i])
        .map(|(_, l)| l)
        .collect();

    // ② method20 弹道终点（shotId 配对；含 miss 的空地终点）
    let endpoints = &shared.endpoints;

    // ③ method38 命中结果（Avatar 方法 = 仅作者自己的射击反馈；共享收集 + 同钟同受击者合并，
    // 布局见 ShotScanShared.hit_results38 文档）
    let mut hit_results = shared.hit_results38.clone();

    // ③'' type=32 来袭炮弹警告/命中通知（eid = 受击者，AoI 广播含他人命中）；segment u64 低字节 = 命中结果枚举（与 method8 b9 同域）。
    // 共享表保持包序，此处按时钟排序供时间窗扫描（filter 保序）
    let mut warnings32 = shared.warnings32.clone();
    warnings32.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap());

    // ③''' Vehicle method8 直击通知（全局广播，envelope eid = 受击者）
    let direct_hits8 = &shared.direct_hits8;

    let names = &shared.names;

    // ④' type=7 刷新簇时钟（AoI 补发/通道切换签名）——tick 采样窗口截断依据
    let refresh_clusters = &shared.refresh_clusters;

    // ⑤'' type=35 服务器竞技场 tick 计数器（u8 递增 @10Hz，自然回绕）。
    let tick_timeline = &shared.tick_timeline;

    // ⑥ per-entity 索引（共享一次预建，st10/prop2 均已按 clock 排序）：type=10 状态采样 / type=7 prop2 炮塔偏航。
    let (st10, prop2) = (&shared.st10, &shared.prop2);
    // 射手（作者）俯仰解码锚定：昵称 → (俯角°, 仰角°)；无锚定时俯仰回退 method36 field2
    let author_name = names.get(&author_player_eid).cloned().unwrap_or_default();
    let shooter_limits = pitch_limits.get(&author_name);

    // ⑤' 弹药选择时间线（type=28，payload=u32 LE 槽位；录像者本人的选择状态）
    let ammo_selects = &shared.ammo_selects;

    // ⑤'' Avatar method 0x07 弹种广播时间线：命中通知未转发（含脱靶弹）时的弹种兜底。
    let shell_broadcasts = &shared.shell_broadcasts;

    // ⑤''' Avatar method 0x1b 地形命中包（仅无坦克命中时广播）：shotId 配对；全局广播含
    // 所有玩家脱靶弹，args[4..8] 的 shell_global_id 是弹种兜底链第三级数据源。
    let terrain_impacts = &shared.terrain_impacts;

    // ⑤'''' Avatar method36 瞄准快照 + 作者俯仰回退序列（method36 field2 车体系俯仰）；
    // type=39 世界系炮线帧（仅开火时刻锚定消费，死亡后旋转不适用）
    let aim_snapshots = &shared.aim_snapshots;
    let aim_pitch_series = &shared.aim_pitch_series;
    let type39_frames = &shared.type39_frames;

    // method1 血量事件（全实体、按时钟排序）——降幅推导输入
    // Type5 物化 9 字节配件选择：射击双方搭载注入（viewer 自动穿深/厚度系数）
    let vehicle_equipment = &shared.vehicle_equipment;
    // ⑥' 确定性伤害降幅区间（WotbTools deriveLosses 同款；仅作者造成的 cause=0 炮弹直击降幅）
    // type=5 满血锚点补链：受害者首个 method1 已是掉血后血量时，首刀降幅才可归属
    let dmg_losses = derive_dmg_losses(
        &shared.hp_events,
        Some(author_player_eid),
        &shared.initial_hp,
    );
    // 降幅互斥消费标记（一段降幅只归属一发，防同区间多发重复计数）
    let mut dmg_losses_used: std::collections::HashSet<usize> = std::collections::HashSet::new();
    // 作者伤害计数器增量序列（共享收集，已剔除非弹伤害污染 tick）
    let dc_increments = &shared.dc_increments;
    let mut hr_cursor = 0usize; // method38 消费游标（按时间顺序，逐发消费）
    let mut dc_cursor = 0usize; // 计数器增量顺序游标（每个造成伤害的命中消费一条）

    // ⑦ 逐发处理（fail-fast：任何数据缺失/歧义直接报错）
    let mut out: Vec<ShotReplayData> = Vec::with_capacity(launches.len());
    // render_cache 由调用方传入（作者/他人两路共享——滤波时间线按实体确定）
    for (i, l) in launches.iter().enumerate() {
        let fire_time = l.t;
        let shot_id = l.shot_id;
        // ctx 仅在报错路径格式化（避免逐发无条件分配）
        let ctx = || format!("shot #{} (shotId={}, t={:.2}s)", i + 1, shot_id, fire_time);

        let (sp, sa, sp_dt, sp_src) = select_anchor_state(
            st10.get(&author_player_eid)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            refresh_clusters,
            author_player_eid,
            fire_time,
        )
        .ok_or_else(|| anyhow::anyhow!("{}: 射手 type=10 状态快照缺失", ctx()))?;

        // 弹道终点（shotId 精确配对）；ball_a = method29 炮口发射位置
        let ball_a = l.point;
        // 终点：有续段（命中/跳弹）→ 续段首点（画面里炮线拐弯处，与命中通知同刻）；
        // 无续段 → method20 终点（弹道最终停止点）
        let (end_time, ball_b) = next_segment
            .get(&(l.shooter, shot_id))
            .copied()
            .or_else(|| endpoints.get(&shot_id).copied())
            .ok_or_else(|| anyhow::anyhow!("{}: method20 弹道终点缺失（shotId 无配对）", ctx()))?;
        let fire_tick = tick_at(tick_timeline, fire_time);

        // ⑦' 弹药槽位：发射时刻的最后选择（type=28 时间线 ≤ fire_time 的最新值）
        let mut shell_slot: u32 = 0;
        for (t_sel, slot) in ammo_selects.iter() {
            if *t_sel <= fire_time {
                shell_slot = *slot;
            } else {
                break;
            }
        }

        // ⑧ 目标实体：method38 victimVehicleId（服务器权威，确定性）
        let mut target_eid: Option<u32> = None;
        let mut damage = 0u32;
        let mut target_name = String::new();
        let mut is_kill = false;
        let mut hit = false;
        let mut hit_flags: u32 = 0;
        let mut crit_modules: u32 = 0;
        let mut destroyed_modules: u32 = 0;
        let mut modifiers: Vec<u32> = Vec::new();

        let mut matched: Option<usize> = None;
        let mut cands = 0usize;
        for (j, hr) in hit_results.iter().enumerate().skip(hr_cursor) {
            if hr.t > end_time + 0.5 {
                break;
            } // 已排序，越过窗口即止
            if (hr.t - end_time).abs() < 0.5 && hr.t >= end_time - 0.05 {
                cands += 1;
                if matched.is_none() {
                    matched = Some(j);
                }
            }
        }
        if cands > 1 {
            anyhow::bail!(
                "{}: method38 配对歧义——命中窗口内出现 {} 条命中结果",
                ctx(),
                cands
            );
        }
        if let Some(j) = matched {
            let hr = &mut hit_results[j];
            target_eid = Some(hr.victim);
            hit_flags = hr.flags;
            crit_modules = hr.crit_modules;
            destroyed_modules = hr.destroyed_modules;
            // HitFeedback 恰被消费一次（hr_cursor 单调前进），modifiers 直接移走避免克隆
            modifiers = std::mem::take(&mut hr.modifiers);
            hit = true;
            hr_cursor = j + 1;
        }

        if let Some(teid) = target_eid {
            // fail-soft：名字是显示域，缺失（受击方昵称损坏等）不中止整条作者射击链——
            // 保留 shot，target_name 空串 + target_eid 照传（身份/弹道/判定不受影响）
            target_name = names.get(&teid).cloned().unwrap_or_default();
        }

        // ⑧' 游戏原生命中段 + 结果枚举（wotinspector segment 对齐）：type=32 警告包优先（segment 低字节=结果枚举），
        // method8 直击通知兜底；两者 hash6 命中令牌一致。歧义 fail-fast；服务器未转发时 segment=0 / result=255。
        let mut segment: u64 = 0;
        let mut shell_id: u32 = 0;
        let mut armor_group: u8 = 0;
        let mut hit_triangle: u16 = 0;
        let mut game_hit_result: u8 = 255;
        let mut hit_token: Option<String> = None;
        if let Some(teid) = target_eid {
            // 一次 Shot 可以在同一 victim/同钟产生多个装甲交互；只按 victim+time 会把
            // 合法的不同 type=32 segment 误判为歧义。作者路径与他人路径统一优先使用
            // method8.hash6 ↔ type32.hash6 的确定性事件令牌；证据不足时仍 fail-fast。
            match select_author_warning32(
                &warnings32,
                direct_hits8,
                author_player_eid,
                teid,
                end_time,
            ) {
                Ok(Some(first)) => {
                    segment = first.segment;
                    game_hit_result = first.result;
                    hit_token = Some(first.hash6.iter().map(|b| format!("{:02x}", b)).collect());
                    // segment 布局解码：[result][shell_global_id u24 LE（=(局部 id<<8)|国家基数）][00][X][Y][Z=armor_group]
                    let sb = segment.to_le_bytes();
                    // 全局弹种 id = B1B2B3 u24 LE（=(局部 id<<8)|国家基数），与 WI shell_id 同值
                    shell_id = (sb[1] as u32) | ((sb[2] as u32) << 8) | ((sb[3] as u32) << 16);
                    armor_group = sb[7];
                    hit_triangle = u16::from_be_bytes([sb[5], sb[6]]);
                }
                Ok(None) => {
                    // type=32 未转发：direct_hits8 已按 t 排序且 filter 保序，r8 天然有序
                    let r8: Vec<&DirectHit8> = direct_hits8
                        .iter()
                        .filter(|d| {
                            d.shooter == author_player_eid
                                && d.victim == teid
                                && (d.t - end_time).abs() <= 0.05
                        })
                        .collect();
                    if !r8.is_empty() {
                        let first = r8[0];
                        if r8.iter().any(|d| d.result != first.result) {
                            anyhow::bail!(
                                "{}: method8 结果枚举歧义——窗口内 {} 条互不一致",
                                ctx(),
                                r8.len()
                            );
                        }
                        game_hit_result = first.result;
                        hit_token =
                            Some(first.hash6.iter().map(|b| format!("{:02x}", b)).collect());
                    }
                }
                Err(n) => {
                    anyhow::bail!(
                        "{}: type=32 segment 歧义——窗口内 {} 条互不一致的命中段",
                        ctx(),
                        n
                    );
                }
            }
        }

        // ⑧'' 弹种三级兜底：type=32 segment 权威 → method 0x07 广播 @ 发射时刻（命中通知未转发，
        // 含脱靶弹；30/30 一致）→ 0x1b 地形命中广播（0x07 空窗时；args 自带 shell_global_id）。
        // 兜底发生时置对应质量标记供 UI 徽章提示
        let mut shell_from_broadcast = false;
        let mut shell_from_terrain = false;
        let terrain_hit = terrain_impacts.get(&shot_id);
        if shell_id == 0 {
            for (t_sel, sh) in shell_broadcasts.iter() {
                if *t_sel <= fire_time {
                    shell_id = *sh;
                    shell_from_broadcast = true;
                } else {
                    break;
                }
            }
            if shell_id == 0 {
                if let Some((gid, _)) = terrain_hit {
                    shell_id = *gid;
                    shell_from_terrain = true;
                }
            }
        }
        let terrain_impact = terrain_hit.map(|(_, d)| d.clone());

        // ⑧''' 开火时刻瞄准快照（method36 成对，|dt|≤0.05）：前=射击前，后=射击后。
        let shooter_aim = {
            let cands: Vec<&AimSnapshot> = aim_snapshots
                .iter()
                .filter(|(t2, _)| (*t2 - fire_time).abs() <= 0.05)
                .map(|(_, s)| s)
                .collect();
            // type39 世界系炮线：开火时刻锚定（|dt|≤0.05s，开火帧必被前后帧夹住）
            let gun_line = type39_frames
                .iter()
                .filter(|f2| (f2.clock - fire_time).abs() <= 0.05)
                .min_by_key(|f2| (((f2.clock - fire_time).abs()) * 1000.0) as u32);
            cands.first().and_then(|s| {
                s.turret_rel_yaw.map(|yaw| ShooterAimData {
                    turret_rel_yaw: yaw,
                    state_before: s.bloom.unwrap_or(0.0),
                    state_after: cands.get(1).and_then(|s2| s2.bloom),
                    gun_pitch: s.gun_pitch,
                    world_gun_yaw: gun_line.map(|f2| f2.gun_yaw),
                    world_gun_pitch: gun_line.map(|f2| f2.gun_pitch_world),
                })
            })
        };

        // 伤害归属（确定性，WotbTools deriveLosses 同款）：穿透族谓词 0x1110 = 材料击穿 0x0010 /
        // 内部模块穿 0x0100 / HE 爆炸 0x1000 → 互斥：一段降幅只归属一次（防同区间双发重复计数，
        // 见 assign_dmg_losses 注）
        let mut dmg_unattributed = false;
        if hit && hit_flags & hit_flags_mod::PENETRATION_FAMILY != 0 {
            let victim = target_eid.unwrap_or(0);
            let containing: Vec<(usize, &DmgLoss)> = dmg_losses
                .iter()
                .enumerate()
                .filter(|(li, l)| {
                    !dmg_losses_used.contains(li)
                        && l.victim == victim
                        && l.t_prev < end_time
                        && end_time <= l.t_cur + 1e-6
                })
                .collect();
            if containing.len() > 1 {
                anyhow::bail!(
                    "{}: 伤害归属歧义（{} 个血量降幅区间包含命中时刻）",
                    ctx(),
                    containing.len()
                );
            }
            let dc_delta = dc_increments.get(dc_cursor).map(|x| x.1);
            if dc_delta.is_some() {
                dc_cursor += 1;
            }
            match containing.first() {
                Some((li, l)) => {
                    dmg_losses_used.insert(*li);
                    damage = l.dmg;
                }
                None => {
                    // ② 计数器亦无增量 = 服务器未记账 HP 伤害（模块-only 击穿等）→ 0
                    damage = dc_delta.unwrap_or(0);
                    dmg_unattributed = true;
                }
            }
            // hp_cur 已按哨兵族归一化（{0,-1,-2,-3}→0），==0 即终态；溺水（cause=5，HP 可为正）
            // 不产生降幅、不进入本链——非炮弹击杀，正确地不计入 is_kill
            is_kill = containing
                .first()
                .map(|(_, l)| l.hp_cur == 0)
                .unwrap_or(false)
                || hit_flags & hit_flags_mod::DIRECT_KILL != 0;
        }

        // ⑨ 目标位置与姿态 @ 命中通知状态（WI 对齐确定性锚点）：
        // method8 命中通知包处理时刻（文件序）受击者的最后已知 type=10 姿态——判定批次内
        // 服务器对受击者的最新已知位置；method8 缺失时回退 end_time 插值状态。
        // miss 无目标基准，回退开火时刻。
        let (tp, ta, tp_dt, tp_src) = if hit {
            let teid = target_eid.unwrap_or(0);
            let d8state = direct_hits8
                .iter()
                .find(|d| {
                    d.shooter == author_player_eid
                        && d.victim == teid
                        && (d.t - end_time).abs() <= 0.05
                        && d.victim_state.is_some()
                })
                .and_then(|d| d.victim_state);
            match d8state {
                // 正常路径（UI 不告警）；回退路径保留 nearest/filtered/extrapolated 供徽章告警
                Some((pos, ang, state_clock)) => (pos, ang, state_clock - end_time, "wi_hit_state"),
                None => select_anchor_state(
                    st10.get(&teid).map(Vec::as_slice).unwrap_or(&[]),
                    refresh_clusters,
                    teid,
                    end_time,
                )
                .ok_or_else(|| anyhow::anyhow!("{}: 命中时刻目标 type=10 状态快照缺失", ctx()))?,
            }
        } else {
            ([0.0; 3], [0.0; 3], 0.0, "nearest")
        };

        // ⑨' 渲染层锚点 + 时间线（窗口语义与实现见 shot_render_pack，两路径共用；
        // 射手炮管俯仰时间线的无锚定回退 = method36 field2 车体系俯仰序列）
        let target_limits = pitch_limits.get(&target_name);
        let ShotRenderPack {
            shooter_render,
            target_render,
            target_render_timeline,
            shooter_render_timeline,
            target_turret_timeline,
            shooter_turret_timeline,
            shooter_gun_timeline,
            target_gun_timeline,
        } = shot_render_pack(
            render_cache,
            st10,
            prop2,
            author_player_eid,
            fire_time,
            sp,
            false,
            target_eid,
            hit,
            end_time,
            tp,
            shooter_limits,
            target_limits,
            ShooterGunFallback::Method36Series(aim_pitch_series),
        );

        // ⑨'' 服务器下发的受击部件索引（method8 args[10]，cmpIndex 0..3）：
        // 与本地 raycast 的部件选择对照 = 命中位置偏差的校准基准。
        let server_part_index = if hit {
            let teid = target_eid.unwrap_or(0);
            direct_hits8
                .iter()
                .find(|d| {
                    d.shooter == author_player_eid
                        && d.victim == teid
                        && (d.t - end_time).abs() <= 0.05
                })
                .and_then(|d| d.component_index)
        } else {
            None
        };

        // ⑨''' 受击者炮管俯仰：method8/type=32 的抵达成角 pitch
        // （来向方位角校验通过者，见 decoded_target_gun_pitch）——受击者被命中时的
        // 反向瞄准俯仰，viewer 渲染"炮口指向射手"的炮管俯角。无有效解码时回退车体 pitch。
        let bearing = if tp != [0.0; 3] && sp != [0.0; 3] {
            Some((sp[0] - tp[0]).atan2(sp[2] - tp[2]))
        } else {
            None
        };
        let server_gun_pitch = if hit {
            decoded_target_gun_pitch(&warnings32, target_eid.unwrap_or(0), end_time, bearing)
                .map(|(p, _)| p)
        } else {
            None
        };

        // ⑩ 目标炮塔朝向 = prop2 + hullYaw（命中弹必须有）
        let state_time = end_time; // 炮塔/炮管取样基准 = 命中通知时刻（与 WI turret_yaw 同域）
                                   // ⑩' method8 流序 prop2 快照：method8 包处理时刻受击者的最后已知
                                   // prop2——时钟序"≤t 最后采样"在同 tick 包序错位时会取到 method8
                                   // 之后的更新（炮塔转动中差 1~8 个 coarse 步），流序快照与 WI battle.json 逐位相等。
        let d8_prop2 = if hit {
            let teid = target_eid.unwrap_or(0);
            direct_hits8
                .iter()
                .find(|d| {
                    d.shooter == author_player_eid
                        && d.victim == teid
                        && (d.t - end_time).abs() <= 0.05
                })
                .and_then(|d| d.victim_prop2)
        } else {
            None
        };
        let turret_yaw = if hit {
            let teid = target_eid.unwrap();
            turret_rel_at(d8_prop2, prop2.get(&teid), state_time)
                .map(|rel| rel + ta[0])
                .ok_or_else(|| anyhow::anyhow!("{}: 目标炮塔朝向（type=7 prop2）缺失", ctx()))?
        } else {
            0.0
        };

        // ⑪ 射手炮塔朝向 = prop2 + 射手 hullYaw，@ 开火时刻（method29 流序快照优先，语义同 ⑩'）
        let shooter_turret_yaw =
            turret_rel_at(l.shooter_prop2, prop2.get(&author_player_eid), fire_time)
                .map(|rel| rel + sa[0])
                .ok_or_else(|| anyhow::anyhow!("{}: 射手炮塔朝向（type=7 prop2）缺失", ctx()))?;

        // ⑪' 受击方炮管俯仰 = prop2 frac 比例解码（车型极限锚定）@ 命中通知时刻；
        // 流序快照优先（扇区选择用同一快照的偏航角，与 WI 同基准）；prop2 采样或锚定缺失
        // → 回退车体 pitch（type10，语义不同仅兜底，质量标记 "target"）
        let mut gun_pitch_degraded: Vec<String> = Vec::new();
        let mut pitch_frozen: Vec<String> = Vec::new();
        let target_gun_pitch_val = if hit {
            let teid = target_eid.unwrap();
            pitch_from_prop2(
                d8_prop2,
                prop2.get(&teid),
                state_time,
                target_limits,
                &mut pitch_frozen,
                "target",
            )
            .unwrap_or_else(|| {
                gun_pitch_degraded.push("target".into());
                ta[1]
            })
        } else {
            ta[1]
        };

        // ⑫ 射手炮管俯仰 = prop2 frac 比例解码 @ 开火时刻（与受击方同源同锚定；method29 流序快照优先）；
        // prop2/锚定缺失 → 回退 method36 field2（车体系炮管俯仰，WotbTools PROVEN），仍缺则 fail-fast
        let (shooter_gun_pitch, shooter_pitch_from_method36) = match pitch_from_prop2(
            l.shooter_prop2,
            prop2.get(&author_player_eid),
            fire_time,
            shooter_limits,
            &mut pitch_frozen,
            "shooter",
        ) {
            Some(p) => (p, false),
            None => {
                gun_pitch_degraded.push("shooter".into());
                let snap_pitch = aim_snapshots
                    .iter()
                    .min_by_key(|(c, _)| (((*c - fire_time).abs()) * 1000.0) as u32)
                    .and_then(|(_, s)| s.gun_pitch);
                match snap_pitch {
                    Some(p) => (p as f32, true),
                    None => {
                        anyhow::bail!("{}: 射手炮管俯仰（prop2 与 method36 快照均缺失）", ctx())
                    }
                }
            }
        };

        // ⑬ aim_point / launch_point_rel = 相对【命中通知状态】目标位置（type10 接地高度）的偏移
        let (aim_point_val, launch_point_rel) = if ball_b != [0.0; 3] && target_eid.is_some() {
            let rel = |p: [f32; 3]| [p[0] - tp[0], p[1] - tp[1], p[2] - tp[2]];
            (rel(ball_b), rel(ball_a))
        } else {
            ([0.0; 3], [0.0; 3])
        };

        // ⑭/⑮ type=10 采样窗口（受击方相对锚点 / 射手世界系；窗口语义与截断规则见
        // tick_window_samples——两路径共用，注释里记录的实测依据不再逐路径复制）
        let tick_samples = if hit {
            target_eid
                .map(|victim| {
                    tick_window_samples(
                        st10,
                        tick_timeline,
                        refresh_clusters,
                        victim,
                        end_time,
                        tp,
                        true,
                        target_render.as_ref(),
                    )
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let shooter_tick_samples = tick_window_samples(
            st10,
            tick_timeline,
            refresh_clusters,
            author_player_eid,
            fire_time,
            [0.0; 3],
            false,
            shooter_render.as_ref(),
        );

        out.push(ShotReplayData {
            index: i + 1,
            time_s: fire_time,
            damage,
            target_name,
            target_eid,
            is_kill,
            shooter_eid: author_player_eid,
            shooter_name: names.get(&author_player_eid).cloned().unwrap_or_default(),
            is_author: true,
            shooter_pos: sp,
            shooter_ang: sa,
            target_pos: tp,
            target_ang: ta,
            target_turret_yaw: turret_yaw,
            target_gun_pitch: target_gun_pitch_val,
            target_gun_pitch_server: server_gun_pitch.is_some(),
            type32_turret_yaw: 0.0,
            shooter_turret_yaw,
            shooter_gun_pitch,
            aim_point: aim_point_val,
            launch_point_rel,
            ball_a,
            ball_b,
            launch_velocity: l.vel,
            hit_flags,
            crit_modules,
            destroyed_modules,
            segment,
            shell_id,
            shell_kind: String::new(),
            armor_group,
            hit_triangle,
            game_hit_result,
            hit_token,
            modifiers,
            shell_slot,
            fire_time,
            shot_id,
            incoming_yaw: 0.0,
            incoming_pitch: ta[1],
            tick_samples,
            shooter_tick_samples,
            fire_tick,
            terrain_impact,
            shooter_aim,
            shooter_equipment: vehicle_equipment
                .get(&author_player_eid)
                .map(VehicleEquipment::from_ids),
            target_equipment: target_eid
                .and_then(|t| vehicle_equipment.get(&t))
                .map(VehicleEquipment::from_ids),
            quality: Some(ShotQuality {
                shooter_state_dt_ms: (sp_dt * 1000.0).round() as i32,
                shooter_pos_from_muzzle: false,
                target_state_dt_ms: if hit {
                    Some((tp_dt * 1000.0).round() as i32)
                } else {
                    None
                },
                turret_degraded: Vec::new(), // 作者路径 prop2 缺失即 fail-fast，不存在降级
                dmg_unattributed,
                shell_from_broadcast,
                shell_from_terrain,
                shooter_pitch_from_velocity: false,
                shooter_pitch_from_method36,
                gun_pitch_degraded,
                pitch_frozen,
                shooter_anchor_src: if sp_src != "filtered" {
                    Some(sp_src.into())
                } else {
                    None
                },
                // 受击方 filtered 仅出现于 method8 缺失回退路径（正常路径为 wi_hit_state），需序列化提示
                target_anchor_src: if hit && tp_src != "wi_hit_state" {
                    Some(tp_src.into())
                } else {
                    None
                },
            }),
            shooter_render,
            target_render,
            target_render_timeline,
            shooter_render_timeline,
            target_turret_timeline,
            shooter_turret_timeline,
            shooter_gun_timeline,
            target_gun_timeline,
            server_part_index,
        });
    }

    // ⑧' method38 = 作者自己的命中反馈——每条都必须配对到一次发射
    if hr_cursor < hit_results.len() {
        anyhow::bail!("存在未被任何发射配对的 method38 命中结果（{} 条未消费，自 t={:.2}s 起）——发射/命中配对不完整",
            hit_results.len() - hr_cursor, hit_results[hr_cursor].t);
    }

    Ok(out)
}

/// 作者昵称（battle_results 权威来源）：author.account_id → 花名册昵称。
/// meta.json 常含非 UTF-8 字节导致 crate read_meta 整体失败（serde_json 严格 UTF-8），
/// 而 battle_results（pickle+protobuf）字符串恒为合法 UTF-8——作者身份以此为准。
pub fn author_nick_from_battle_results(
    br: &wotbreplay_parser::models::battle_results::BattleResults,
) -> String {
    br.players
        .iter()
        .find(|p| p.account_id == br.author.account_id)
        .map(|p| p.info.nickname.clone())
        .unwrap_or_default()
}

/// 从 type=5 实体创建包按昵称精确匹配作者玩家实体 eid。
/// 作者昵称来自回放自身：meta.player_name / battle_results author→花名册（不用文件名——
/// 文件名不属于回放数据，改名即失效）。昵称解码走
/// [`decode_type5_nickname`] SSOT（原始 UTF-8 全域，含非 ASCII），与作者昵称精确比较
///（有 battle_results 昵称作锚，无垃圾误配风险）。无匹配返回 0。
pub fn resolve_author_player_eid_by_nick(packets: &[(u32, f32, &[u8])], author_nick: &str) -> u32 {
    if author_nick.is_empty() {
        return 0;
    }
    packets
        .iter()
        .filter_map(|(t, _, p)| {
            if *t != 5 {
                return None;
            }
            decode_type5_nickname(p)
                .filter(|(_, s)| *s == author_nick)
                .map(|(eid, _)| eid)
        })
        .next()
        .unwrap_or(0)
}

/// 便捷入口：默认无俯仰极限锚定。
#[allow(dead_code)]
pub fn extract_shot_replays_auto(
    packets: &[(u32, f32, &[u8])],
    author_nick: &str,
) -> anyhow::Result<Vec<ShotReplayData>> {
    extract_shot_replays_auto_with_limits(packets, author_nick, &GunPitchLimits::new())
}

/// [`extract_shot_replays_auto`] 的完整形态（`pitch_limits` 语义见 [`extract_shot_replays_with_limits`]）。
/// `author_nick` = 作者玩家昵称（meta.player_name / battle_results author→花名册），非文件名。
pub fn extract_shot_replays_auto_with_limits(
    packets: &[(u32, f32, &[u8])],
    author_nick: &str,
    pitch_limits: &GunPitchLimits,
) -> anyhow::Result<Vec<ShotReplayData>> {
    let author_player_eid = resolve_author_player_eid_by_nick(packets, author_nick);
    extract_shot_replays_with_limits(packets, author_player_eid, pitch_limits)
}

/// 一次共享扫描完成"作者严格 + 他人宽松"两路提取（web `replay_shots_handler` /
/// CLI `replay` 子命令入口）：预分析产物与滤波渲染缓存两路复用。作者路径 fail
/// 语义与 [`extract_shot_replays_auto_with_limits`] 一致（错误原样上抛，由调用方
/// 决定降级策略）；`author_player_eid` 非零时直接采用，为 0 时按 `author_nick` 解析。
pub fn extract_all_shots_auto_with_limits(
    packets: &[(u32, f32, &[u8])],
    author_nick: &str,
    author_player_eid: u32,
    pitch_limits: &GunPitchLimits,
) -> anyhow::Result<(Vec<ShotReplayData>, OtherShotsExtraction)> {
    let author_eid = if author_player_eid != 0 {
        author_player_eid
    } else {
        resolve_author_player_eid_by_nick(packets, author_nick)
    };
    let shared = build_shot_scan_shared(packets, author_eid);
    let mut render_cache: HashMap<u32, FilteredTimeline> = HashMap::new();
    let author =
        extract_shot_replays_from_shared(&shared, author_eid, pitch_limits, &mut render_cache)?;
    let others = extract_other_shot_replays_from_shared(
        &shared,
        author_eid,
        pitch_limits,
        &mut render_cache,
    );
    Ok((author, others))
}

/// 他人路径提取结果：射击列表 + 回退/跳过统计（web 响应 notes 供用户了解数据边界）。
pub struct OtherShotsExtraction {
    pub shots: Vec<ShotReplayData>,
    /// method29 发射总数（含被跳过者）
    pub total_launches: usize,
    /// method20 弹道终点缺失 → 整发跳过（无复现基准）
    pub skipped_no_endpoint: usize,
    /// 受击方状态完全缺失（method8 无状态且 type=10 无采样）→ 整发跳过
    pub skipped_no_target_state: usize,
    /// 射手 type=10 缺失 → 炮口坐标兜底（未跳过，per-shot 另有 quality 标记）
    pub muzzle_fallback: usize,
}

/// 其他玩家（队友/敌方）射击的宽松提取：与 [`extract_shot_replays`] 同源数据，但 Avatar 专属包不可得，对应字段降级：
/// method38 命中反馈（作者专属）→ hit_flags/crit/destroyed/modifiers 恒空，结果 = method8 result 枚举；目标 = method8 就近匹配（±0.05s）；
/// segment/弹种/装甲组 = method8 hash6 令牌 ↔ type=32 精确配对（同源），未配对时弹种回退
/// 0x1b 地形命中广播的 shell_global_id（全局广播含所有玩家脱靶弹，shotId 配对）并置 shell_from_terrain；
/// 伤害 = 血量链降幅（source=射手，cause=0）；双方炮管俯仰 = prop2 frac 解码（无锚定时射手回退发射速度向量、受击方回退车体 pitch）；
/// method36 瞄准快照 / type=28 弹药槽 = Avatar 专属 → None / 0。
/// 宽松模式：数据缺失的射击跳过并计数，绝不 bail（AoI 裁剪致远端数据稀疏是预期，与作者路径 fail-fast 不同）。
/// 便捷入口：默认无俯仰极限锚定。
#[allow(dead_code)]
pub fn extract_other_shot_replays(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
) -> OtherShotsExtraction {
    extract_other_shot_replays_with_limits(packets, author_player_eid, &GunPitchLimits::new())
}

/// [`extract_other_shot_replays`] 的完整形态（`pitch_limits` 语义见 [`extract_shot_replays_with_limits`]）。
pub fn extract_other_shot_replays_with_limits(
    packets: &[(u32, f32, &[u8])],
    author_player_eid: u32,
    pitch_limits: &GunPitchLimits,
) -> OtherShotsExtraction {
    let shared = build_shot_scan_shared(packets, author_player_eid);
    let mut render_cache: HashMap<u32, FilteredTimeline> = HashMap::new();
    extract_other_shot_replays_from_shared(
        &shared,
        author_player_eid,
        pitch_limits,
        &mut render_cache,
    )
}

/// [`extract_other_shot_replays_with_limits`] 的共享扫描形态（预分析产物复用，见
/// [`ShotScanShared`]；`render_cache` 可与作者路径共享）。
pub(crate) fn extract_other_shot_replays_from_shared(
    shared: &ShotScanShared,
    author_player_eid: u32,
    pitch_limits: &GunPitchLimits,
    render_cache: &mut HashMap<u32, FilteredTimeline>,
) -> OtherShotsExtraction {
    // ① 全部非作者的 method29 发射事件（作者由严格路径处理；shotId 全局去重）；args<37 的包直接跳过
    let launches: Vec<LaunchEntry> = shared
        .launches
        .iter()
        .filter(|l| l.shooter != author_player_eid)
        .cloned()
        .collect();

    // ①' 段链分组（与作者路径同构）：链首 = 发射段；其余 = 同一发的跳弹/穿透续段
    let (launch_is_cont, next_segment) = collect::shot_segments(&launches);
    let launches: Vec<LaunchEntry> = launches
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !launch_is_cont[*i])
        .map(|(_, l)| l)
        .collect();

    // ② method20 弹道终点（shotId 配对，全量收集，与作者路径同构）
    let endpoints = &shared.endpoints;

    // ③ method8 直击通知（全局广播）
    let direct_hits8 = &shared.direct_hits8;

    // ③' type=32 命中通知（AoI 广播含他人；按 hash6 令牌与 method8 精确配对）
    let warnings32 = &shared.warnings32;

    // ③'' method 0x1b 地形命中广播（全局，含所有玩家脱靶弹）：
    // 脱靶弹的 terrain_impact 精确落点 + shell_id 兜底链第三级数据源
    let terrain_impacts = &shared.terrain_impacts;

    let names = &shared.names;

    // Type5 物化 9 字节配件选择（作者路径同款）：他人射击双方搭载注入
    let vehicle_equipment = &shared.vehicle_equipment;

    // ④ type=35 tick 计数器展开（与作者路径同构）
    let tick_timeline = &shared.tick_timeline;

    // ⑤ 血量链降幅区间（全 source 保留——作者路径 ⑥' 的无过滤版本，cause=0 炮弹直击；含 type=5 满血锚点）
    let dmg_losses = derive_dmg_losses(&shared.hp_events, None, &shared.initial_hp);
    // ⑤' 互斥预归属：每段降幅只归属区间内 end_time 最大的命中发射（同区间未穿弹不计入、
    // 不重复计数）。目标识别与循环内同式（method8 ±0.05s）。
    let shot_dmg_inputs: Vec<Option<(f32, u32, u32)>> = launches
        .iter()
        .map(|l| {
            let (end_time, _) = next_segment
                .get(&(l.shooter, l.shot_id))
                .copied()
                .or_else(|| endpoints.get(&l.shot_id).copied())?;
            let teid = direct_hits8
                .iter()
                .filter(|d| d.shooter == l.shooter && (d.t - end_time).abs() <= 0.05)
                .min_by(|x, y| {
                    (x.t - end_time)
                        .abs()
                        .partial_cmp(&(y.t - end_time).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|d| d.victim)?;
            Some((end_time, l.shooter, teid))
        })
        .collect();
    let dmg_assign = assign_dmg_losses(&shot_dmg_inputs, &dmg_losses);

    // ⑥ type=10 状态索引 + type=7 prop2 炮塔偏航索引（共享一次预建，均按 clock 排序）
    let (st10, prop2) = (&shared.st10, &shared.prop2);
    // 补发簇签名（AoI 通道切换点）——tick 采样截断 + 锚点选择依据
    let refresh_clusters = &shared.refresh_clusters;
    let anchor_at = |eid: u32, t: f32| -> Option<([f32; 3], [f32; 3], f32, &'static str)> {
        select_anchor_state(st10.get(&eid)?, refresh_clusters, eid, t)
    };

    // ⑦ 逐发组装（宽松：缺数据跳过 + 计数）
    let mut out: Vec<ShotReplayData> = Vec::with_capacity(launches.len());
    // render_cache 由调用方传入（作者/他人两路共享——滤波时间线按实体确定）
    let mut skipped_no_endpoint = 0usize;
    let mut skipped_no_target_state = 0usize;
    let mut muzzle_fallback = 0usize;
    for (li, l) in launches.iter().enumerate() {
        // ctx 仅在跳过/兜底打印时格式化（避免逐发无条件分配）
        let ctx = || {
            format!(
                "shotId={} shooter={:08x} t={:.2}s",
                l.shot_id, l.shooter, l.t
            )
        };
        let ball_a = l.point;
        // 终点：有续段（命中/跳弹）→ 续段首点；无续段 → method20 终点（与作者路径同构）
        let (end_time, ball_b) = match next_segment
            .get(&(l.shooter, l.shot_id))
            .copied()
            .or_else(|| endpoints.get(&l.shot_id).copied())
        {
            Some(x) => x,
            None => {
                skipped_no_endpoint += 1;
                eprintln!("[replay_others] 跳过 {}: method20 终点缺失", ctx());
                continue;
            }
        };
        // 射手状态快照；AoI 裁剪缺失时用炮口坐标兜底（ball_a = method29 服务器权威发射位置），朝向从速度向量推算，不整发跳过
        let (sp, sa, sp_dt, sp_src, pos_from_muzzle) = match anchor_at(l.shooter, l.t) {
            Some((pos, ang, dt, src)) => (pos, ang, dt, src, false),
            None => {
                let yaw = l.vel[2].atan2(l.vel[0]);
                let pitch = {
                    let horiz = (l.vel[0] * l.vel[0] + l.vel[2] * l.vel[2]).sqrt();
                    if horiz > 1e-6 {
                        l.vel[1].atan2(horiz)
                    } else {
                        0.0
                    }
                };
                muzzle_fallback += 1;
                eprintln!(
                    "[replay_others] {}: 射手 type=10 状态缺失，用炮口坐标兜底",
                    ctx()
                );
                (ball_a, [yaw, pitch, 0.0], 0.0, "nearest", true)
            }
        };
        let fire_tick = tick_at(tick_timeline, l.t);

        // 目标 = method8 就近匹配（窗口 ±0.05s，同作者路径命中窗口）
        let dhit = direct_hits8
            .iter()
            .filter(|d| d.shooter == l.shooter && (d.t - end_time).abs() <= 0.05)
            .min_by(|x, y| {
                (x.t - end_time)
                    .abs()
                    .partial_cmp(&(y.t - end_time).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let target_eid = dhit.map(|d| d.victim);
        let hit = target_eid.is_some();

        // segment/弹种/装甲组：method8 hash6 令牌 ↔ type=32 精确配对（优于时间窗）；
        // type=32 未配对（AoI 裁剪）时弹种回退 0x1b 地形命中广播（shotId 精确配对，含所有玩家脱靶弹）
        let mut segment: u64 = 0;
        let mut shell_id: u32 = 0;
        let mut shell_from_terrain = false;
        let mut armor_group: u8 = 0;
        let mut hit_triangle: u16 = 0;
        let mut game_hit_result: u8 = dhit.map(|d| d.result).unwrap_or(255);
        let mut hit_token: Option<String> = None;
        if let Some(d) = dhit {
            hit_token = Some(d.hash6.iter().map(|b| format!("{:02x}", b)).collect());
            if let Some(teid) = target_eid {
                if let Some(w) = warnings32
                    .iter()
                    .find(|w| w.eid == teid && w.hash6 == d.hash6)
                {
                    segment = w.segment;
                    game_hit_result = w.result;
                    let sb = segment.to_le_bytes();
                    shell_id = (sb[1] as u32) | ((sb[2] as u32) << 8) | ((sb[3] as u32) << 16);
                    armor_group = sb[7];
                    hit_triangle = u16::from_be_bytes([sb[5], sb[6]]);
                }
            }
        }
        let terrain_hit = terrain_impacts.get(&l.shot_id);
        if shell_id == 0 {
            if let Some((gid, _)) = terrain_hit {
                shell_id = *gid;
                shell_from_terrain = true;
            }
        }
        let terrain_impact = terrain_hit.map(|(_, d)| d.clone());

        // 伤害归属：互斥预归属结果（assign_dmg_losses：区间内 end_time 最大者得降幅）
        let mut damage = 0u32;
        let mut is_kill = false;
        let mut dmg_unattributed = false;
        let target_name = target_eid
            .and_then(|e| names.get(&e).cloned())
            .unwrap_or_default();
        if let Some(d) = dhit {
            if let Some(&(dm, hp_cur)) = dmg_assign.get(&li) {
                damage = dm;
                is_kill = hp_cur == 0; // 归属链已按哨兵族 {0,-1,-2,-3}→0 归一化
            } else if d.result == 3
                || d.result == 4
                || (d.result == 2 && d.component_index == Some(0))
            {
                // 应伤结果（3=击穿 / 4=履带·模块交互可带伤）却无降幅 = 服务器未记账 HP
                dmg_unattributed = true;
            }
            // result=1/2（未穿/间隙止）无降幅 = 正常零伤，不打标
        }

        // 目标状态 @ 命中通知状态（WI 对齐锚点，与作者路径同源：dhit 快照即受击者在
        // method8 处理时刻的运行状态；状态缺失回退 end_time 插值，仍缺则整发跳过）
        let (tp, ta, tp_dt, tp_src) = if let Some(d) = dhit {
            match d.victim_state {
                // 与作者路径同源：method8 通知状态 = WI 正常语义（不告警）
                Some((pos, ang, state_clock)) => (pos, ang, state_clock - end_time, "wi_hit_state"),
                None => match anchor_at(d.victim, end_time) {
                    Some(x) => x,
                    None => {
                        skipped_no_target_state += 1;
                        eprintln!(
                            "[replay_others] 跳过 {}: 命中时刻目标 type=10 状态缺失",
                            ctx()
                        );
                        continue;
                    }
                },
            }
        } else {
            ([0.0; 3], [0.0; 3], 0.0, "nearest")
        };

        // 渲染层锚点 + 时间线（与作者路径共用 shot_render_pack；射手炮口兜底时跳过
        // 射手侧渲染数据，炮管俯仰时间线无锚定回退 = 空）
        let shooter_name_val = names.get(&l.shooter).cloned().unwrap_or_default();
        let shooter_limits = pitch_limits.get(&shooter_name_val);
        let target_limits = pitch_limits.get(&target_name);
        let ShotRenderPack {
            shooter_render,
            target_render,
            target_render_timeline,
            shooter_render_timeline,
            target_turret_timeline,
            shooter_turret_timeline,
            shooter_gun_timeline,
            target_gun_timeline,
        } = shot_render_pack(
            render_cache,
            st10,
            prop2,
            l.shooter,
            l.t,
            sp,
            pos_from_muzzle,
            target_eid,
            hit,
            end_time,
            tp,
            shooter_limits,
            target_limits,
            ShooterGunFallback::None,
        );
        // 服务器受击部件索引（method8 args[10]）
        let server_part_index = dhit.and_then(|d| d.component_index);
        // 受击者炮管俯仰（原始解码恢复）：来向方位角校验通过的抵达成角 pitch
        let bearing = if tp != [0.0; 3] && sp != [0.0; 3] {
            Some((sp[0] - tp[0]).atan2(sp[2] - tp[2]))
        } else {
            None
        };
        let server_gun_pitch =
            decoded_target_gun_pitch(warnings32, target_eid.unwrap_or(0), end_time, bearing)
                .map(|(p, _)| p);

        // 炮塔朝向（prop2 相对角 @ 命中通知时刻；method8 流序快照优先 = WI 逐位同基准，
        // 见 DirectHit8::victim_prop2；AoI 裁剪缺失时降级为车体朝向，不跳过）
        let d8_prop2 = dhit.and_then(|d| d.victim_prop2);
        let mut turret_degraded: Vec<String> = Vec::new();
        let mut gun_pitch_degraded: Vec<String> = Vec::new();
        let mut pitch_frozen: Vec<String> = Vec::new();
        let target_turret_yaw = if hit {
            let teid = target_eid.unwrap_or(0);
            match turret_rel_at(d8_prop2, prop2.get(&teid), end_time) {
                Some(rel) => rel + ta[0],
                None => {
                    turret_degraded.push("target".into());
                    ta[0]
                }
            }
        } else {
            0.0
        };
        let shooter_turret_yaw = match turret_rel_at(l.shooter_prop2, prop2.get(&l.shooter), l.t) {
            Some(rel) => rel + sa[0],
            None => {
                if !pos_from_muzzle {
                    turret_degraded.push("shooter".into());
                }
                sa[0]
            }
        };

        // 炮管俯仰：prop2 frac 比例解码（双方同源，method29/8 流序快照优先）；prop2/锚定缺失
        // → 射手回退发射速度向量反解（垂直/水平分量），受击方回退车体 pitch，均打质量标记
        let shooter_gun_pitch = match pitch_from_prop2(
            l.shooter_prop2,
            prop2.get(&l.shooter),
            l.t,
            shooter_limits,
            &mut pitch_frozen,
            "shooter",
        ) {
            Some(p) => p,
            None => {
                gun_pitch_degraded.push("shooter".into());
                let horiz = (l.vel[0] * l.vel[0] + l.vel[2] * l.vel[2]).sqrt();
                if horiz > 1e-6 {
                    l.vel[1].atan2(horiz)
                } else {
                    0.0
                }
            }
        };
        let target_gun_pitch_val = if hit {
            let teid = target_eid.unwrap();
            pitch_from_prop2(
                d8_prop2,
                prop2.get(&teid),
                end_time,
                target_limits,
                &mut pitch_frozen,
                "target",
            )
            .unwrap_or_else(|| {
                gun_pitch_degraded.push("target".into());
                ta[1]
            })
        } else {
            ta[1]
        };

        // aim_point / launch_point_rel = 相对命中通知状态目标位置的偏移（与作者路径同式）
        let (aim_point_val, launch_point_rel) = if ball_b != [0.0; 3] && target_eid.is_some() {
            let rel = |p: [f32; 3]| [p[0] - tp[0], p[1] - tp[1], p[2] - tp[2]];
            (rel(ball_b), rel(ball_a))
        } else {
            ([0.0; 3], [0.0; 3])
        };

        // ⑭/⑮ type=10 采样窗口（与作者路径共用 tick_window_samples）
        let tick_samples = target_eid
            .map(|victim| {
                tick_window_samples(
                    st10,
                    tick_timeline,
                    refresh_clusters,
                    victim,
                    end_time,
                    tp,
                    true,
                    target_render.as_ref(),
                )
            })
            .unwrap_or_default();
        let shooter_tick_samples = tick_window_samples(
            st10,
            tick_timeline,
            refresh_clusters,
            l.shooter,
            l.t,
            [0.0; 3],
            false,
            shooter_render.as_ref(),
        );

        out.push(ShotReplayData {
            index: 0, // 合并后由调用方按 time_s 全局重编号
            time_s: l.t,
            damage,
            target_name,
            target_eid,
            is_kill,
            shooter_eid: l.shooter,
            shooter_name: names.get(&l.shooter).cloned().unwrap_or_default(),
            is_author: false,
            shooter_pos: sp,
            shooter_ang: sa,
            target_pos: tp,
            target_ang: ta,
            target_turret_yaw,
            target_gun_pitch: target_gun_pitch_val,
            target_gun_pitch_server: server_gun_pitch.is_some(),
            type32_turret_yaw: 0.0,
            shooter_turret_yaw,
            shooter_gun_pitch,
            aim_point: aim_point_val,
            launch_point_rel,
            ball_a,
            ball_b,
            launch_velocity: l.vel,
            hit_flags: 0, // method38 作者专属，他人不可得（结果看 game_hit_result）
            crit_modules: 0,
            destroyed_modules: 0,
            segment,
            shell_id,
            shell_kind: String::new(),
            armor_group,
            hit_triangle,
            game_hit_result,
            hit_token,
            modifiers: Vec::new(),
            shell_slot: 0, // type=28 弹药槽是作者本人的选择状态，对他人无意义
            fire_time: l.t,
            shot_id: l.shot_id,
            incoming_yaw: 0.0,
            incoming_pitch: ta[1],
            tick_samples,
            shooter_tick_samples,
            fire_tick,
            terrain_impact,    // 0x1b 全局广播：他人脱靶弹同样有精确落点
            shooter_aim: None, // method36 瞄准快照 = 作者 Avatar 专属
            shooter_equipment: vehicle_equipment
                .get(&l.shooter)
                .map(VehicleEquipment::from_ids),
            target_equipment: target_eid
                .and_then(|t| vehicle_equipment.get(&t))
                .map(VehicleEquipment::from_ids),
            quality: Some(ShotQuality {
                shooter_state_dt_ms: (sp_dt * 1000.0).round() as i32,
                shooter_pos_from_muzzle: pos_from_muzzle,
                target_state_dt_ms: if hit {
                    Some((tp_dt * 1000.0).round() as i32)
                } else {
                    None
                },
                turret_degraded,
                dmg_unattributed,
                shell_from_broadcast: false, // 0x07 弹种广播 = 作者 Avatar 弹药状态，他人不可得
                shell_from_terrain,
                shooter_pitch_from_velocity: gun_pitch_degraded.iter().any(|s| s == "shooter"),
                shooter_pitch_from_method36: false,
                gun_pitch_degraded,
                pitch_frozen,
                shooter_anchor_src: if sp_src != "filtered" {
                    Some(sp_src.into())
                } else {
                    None
                },
                // 受击方 filtered 仅出现于 method8 缺失回退路径（正常路径为 wi_hit_state），需序列化提示
                target_anchor_src: if hit && tp_src != "wi_hit_state" {
                    Some(tp_src.into())
                } else {
                    None
                },
            }),
            shooter_render,
            target_render,
            target_render_timeline,
            shooter_render_timeline,
            target_turret_timeline,
            shooter_turret_timeline,
            shooter_gun_timeline,
            target_gun_timeline,
            server_part_index,
        });
    }
    eprintln!("[replay_others] 其他玩家射击提取: {} 发（终点缺失跳过 {}、受击方状态缺失跳过 {}、射手状态炮口兜底 {}）",
        out.len(), skipped_no_endpoint, skipped_no_target_state, muzzle_fallback);
    OtherShotsExtraction {
        shots: out,
        total_launches: launches.len(),
        skipped_no_endpoint,
        skipped_no_target_state,
        muzzle_fallback,
    }
}

#[cfg(test)]
mod author_warning32_pairing_tests {
    use super::*;

    fn warning(t: f32, eid: u32, hash6: [u8; 6], segment: u64) -> ArenaWarning32 {
        ArenaWarning32 {
            t,
            eid,
            result: segment.to_le_bytes()[0],
            segment,
            hash6,
            inc_yaw: 0.0,
            inc_pitch: 0.0,
        }
    }

    fn direct(t: f32, shooter: u32, victim: u32, hash6: [u8; 6], result: u8) -> DirectHit8 {
        DirectHit8 {
            t,
            shooter,
            victim,
            result,
            component_index: Some(1),
            hash6,
            victim_state: None,
            victim_prop2: None,
        }
    }

    #[test]
    fn unique_method8_hash_disambiguates_multi_interaction_type32_window() {
        // 11.20 China Kranvagn 边界形状：同一 Shot / victim / 时钟出现两个不同
        // type=32 segment；method8 自身重复广播，但两份共享同一 hash6。
        let a = [0x0a, 0xa3, 0x6c, 0x50, 0xa1, 0x5c];
        let b = [0x23, 0xa8, 0x6c, 0x4c, 0x8f, 0x4d];
        let warnings = vec![
            warning(110.35225, 42, a, 0x0346_0000_0135_8a00),
            warning(110.35225, 42, b, 0x033a_b700_0135_8a01),
        ];
        let hits = vec![
            direct(110.35225, 7, 42, a, 0),
            direct(110.35225, 7, 42, a, 0),
        ];

        let picked = select_author_warning32(&warnings, &hits, 7, 42, 110.35225)
            .expect("唯一 method8 hash6 应消除 type32 窗口歧义")
            .expect("应选出命中段");
        assert_eq!(picked.hash6, a);
        assert_eq!(picked.segment, warnings[0].segment);
    }

    #[test]
    fn conflicting_type32_without_unique_method8_hash_remains_fail_fast() {
        let a = [1, 2, 3, 4, 5, 6];
        let b = [6, 5, 4, 3, 2, 1];
        let warnings = vec![warning(20.0, 42, a, 0x10), warning(20.0, 42, b, 0x20)];

        assert!(matches!(
            select_author_warning32(&warnings, &[], 7, 42, 20.0),
            Err(2)
        ));

        let hits = vec![direct(20.0, 7, 42, a, 0), direct(20.0, 7, 42, b, 1)];
        assert!(matches!(
            select_author_warning32(&warnings, &hits, 7, 42, 20.0),
            Err(2)
        ));
    }

    #[test]
    fn duplicate_same_token_type32_with_same_segment_is_not_ambiguous() {
        let hash = [1, 1, 2, 3, 5, 8];
        let warnings = vec![
            warning(30.0, 42, hash, 0x1234),
            warning(30.0, 42, hash, 0x1234),
        ];
        let hits = vec![direct(30.0, 7, 42, hash, 0)];

        let picked = select_author_warning32(&warnings, &hits, 7, 42, 30.0)
            .expect("相同 token + 相同 segment 的重复通知应去重语义")
            .expect("应选出命中段");
        assert_eq!(picked.segment, 0x1234);
    }
}
