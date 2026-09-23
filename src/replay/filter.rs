//! 客户端位置滤波器移植（逆向文档 §七）：BigWorld OSS `AvatarFilterHelper`（14.4.1）逐行语义移植，
//! 与 wotblitz.exe 中 WGVehicleFilter2/KineticsFilter 共享的核心实现同构——
//! 二进制实证：helper 对象 0x2A8 = 8×StoredInput(56B) + 2×Waypoint(96B) + 状态字段，
//! 延迟参数静态区 {1.0, 0.10, 2.0, 2.0, true} 原样编译（VA 0x4055388）。
//!
//! 用途：按文件序把 per-entity type=10 流喂进 [`AvatarFilter`]，以 60Hz 帧网格推进
//! [`AvatarFilter::output`]（游戏客户端每帧渲染一次），得到任意事件时刻的
//! **渲染层锚点** = 游戏画面里模型实际呈现的位置（滞后 latency ≈ 0.1~0.2s）。
//! 注意与判定层锚点（method8 通知状态，WI 对齐）分属两层语义：装甲命中几何必须用判定层。
//!
//! 与 OSS 的有意差异（均不改位姿算法主干）：
//! - `resolveOnGroundPosition`（地形吸附）与 `changeCoordinateSystem`（载具坐标系）
//!   在离线回放域为 no-op（type=10 的 y 已是接地高度；WoTB 无载具搭乘）；
//! - spaceID/vehicleID 通道省略（单空间）。

use super::combat::St10Sample;

/// OSS `AvatarFilterSettings` 编译期默认值（二进制实证原样保留，逆向文档 §7.3）
const LATENCY_VELOCITY: f32 = 1.0;
const LATENCY_MINIMUM: f32 = 0.10;
const LATENCY_FRAMES: f32 = 2.0;
const LATENCY_CURVE_POWER: f32 = 2.0;
const NUM_STORED_INPUTS: usize = 8;
/// reset 重填历史时的非零时间步（OSS NONZERO_TIME_DIFFERENCE，避免零时间差除法）
const NONZERO_TIME_DIFFERENCE: f64 = 0.01;

/// 一条滤波器输入（OSS StoredInput：double 时间 + float 几何）。
#[derive(Debug, Clone, Copy)]
struct StoredInput {
    time: f64,
    pos: [f32; 3],
    pos_error: [f32; 3],
    yaw: f32,
    pitch: f32,
}

/// OSS Waypoint：extract() 在 prev/next 两路径点间插值；stored 保留其锚定的原始输入供误差盒钳位。
#[derive(Debug, Clone, Copy)]
struct Waypoint {
    time: f64,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    stored: StoredInput,
}

impl Waypoint {
    fn zero() -> Self {
        Self {
            time: 0.0,
            pos: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            stored: StoredInput::zero(),
        }
    }
}

impl StoredInput {
    fn zero() -> Self {
        Self { time: 0.0, pos: [0.0; 3], pos_error: [0.0; 3], yaw: 0.0, pitch: 0.0 }
    }
}

/// 一帧的滤波输出（渲染层位姿；roll 恒 0 = 游戏滤波层语义，视觉侧倾来自物理层）。
#[derive(Debug, Clone, Copy)]
pub struct FramePose {
    /// 帧时刻（回放时钟域，秒）
    #[allow(dead_code)] // 公开数据 API 字段：viewer 渲染锚点消费前由测试/探针使用
    pub time: f64,
    /// 滤波器输出时刻 = time − latency（渲染在此时刻的"真实"位置）
    #[allow(dead_code)] // 公开数据 API 字段：viewer 渲染锚点消费前由测试/探针使用
    pub output_time: f64,
    /// 当前延迟（秒；自适应趋向 2 个更新帧、下限 0.10s）
    pub latency: f32,
    pub pos: [f32; 3],
    /// yaw/pitch 最短弧插值；roll 恒 0
    pub ang: [f32; 3],
    /// 路径点斜率 = 滤波层速度估计（m/s）
    #[allow(dead_code)]
    pub velocity: [f32; 3],
}

/// OSS AvatarFilter + AvatarFilterHelper 合体移植（helper 内嵌于滤波器对象，接口槽语义一致）。
#[derive(Debug, Clone)]
pub struct AvatarFilter {
    inputs: [StoredInput; NUM_STORED_INPUTS],
    current_input_index: usize,
    input_count: u32,
    next_waypoint: Waypoint,
    previous_waypoint: Waypoint,
    latency: f32,
    ideal_latency: f32,
    time_of_last_output: f64,
    got_new_input: bool,
    reset_flag: bool,
}

impl AvatarFilter {
    pub fn new() -> Self {
        Self {
            inputs: [StoredInput::zero(); NUM_STORED_INPUTS],
            current_input_index: 0,
            input_count: 0,
            next_waypoint: Waypoint::zero(),
            previous_waypoint: Waypoint::zero(),
            latency: 0.0,
            ideal_latency: 0.0,
            time_of_last_output: 0.0,
            got_new_input: false,
            reset_flag: true,
        }
    }

    /// index 0 = 最新（OSS getStoredInput 语义）
    fn input_at(&self, index: usize) -> &StoredInput {
        &self.inputs[(self.current_input_index + index) % NUM_STORED_INPUTS]
    }

    /// OSS reset(time)：只置标志，实际清史推迟到下一输入（reset StoredInputs 重填 8 份 -i*0.01s）。
    #[allow(dead_code)] // OSS 接口完整性；当前时间线构建从首个输入隐式 reset
    pub fn reset(&mut self, _time: f64) {
        self.reset_flag = true;
    }

    /// OSS input()：乱序样本（time ≤ 最新）直接丢弃；reset 后首输入整批重填。
    pub fn input(&mut self, time: f64, pos: [f32; 3], pos_error: [f32; 3], yaw: f32, pitch: f32) {
        if self.reset_flag {
            self.reset_stored_inputs(time, pos, pos_error, yaw, pitch);
            self.reset_flag = false;
            return;
        }
        if time <= self.input_at(0).time {
            return;
        }
        self.current_input_index =
            (self.current_input_index + NUM_STORED_INPUTS - 1) % NUM_STORED_INPUTS;
        let si = &mut self.inputs[self.current_input_index];
        si.time = time;
        si.pos = pos;
        si.pos_error = pos_error;
        si.yaw = yaw;
        si.pitch = pitch;
        self.got_new_input = true;
        self.input_count += 1;
    }

    /// OSS resetStoredInputs：全部 8 槽重填为同一状态（时间错开 0.01s 防零除）。
    fn reset_stored_inputs(
        &mut self,
        time: f64,
        pos: [f32; 3],
        pos_error: [f32; 3],
        yaw: f32,
        pitch: f32,
    ) {
        self.current_input_index = 0;
        self.input_count = 0;
        self.got_new_input = true;
        self.time_of_last_output = time;
        for (i, slot) in self.inputs.iter_mut().enumerate() {
            *slot = StoredInput {
                time: time - (i as f64) * NONZERO_TIME_DIFFERENCE,
                pos,
                pos_error,
                yaw,
                pitch,
            };
        }
        self.latency = (LATENCY_FRAMES as f64 * NONZERO_TIME_DIFFERENCE) as f32;
        self.next_waypoint = Waypoint {
            time: time - NONZERO_TIME_DIFFERENCE,
            pos,
            yaw,
            pitch,
            stored: StoredInput { time: time - NONZERO_TIME_DIFFERENCE, pos, pos_error, yaw, pitch },
        };
        self.previous_waypoint = self.next_waypoint;
        self.previous_waypoint.time -= NONZERO_TIME_DIFFERENCE;
    }

    /// OSS output(time) + extract()：推进延迟状态机并返回本帧渲染位姿。
    /// 返回的 output_time = time − latency（≤0 无历史可渲染的边界由调用方按帧网格自然规避）。
    pub fn output(&mut self, time: f64) -> FramePose {
        // 延迟理想值自适应（有新输入时重估）——WG 二进制 0x276E0C0 实测公式：
        // ratio = (7 − latencyFrames) / 7【maxLatencyFrame 硬编码 7，非 OSS 的动态
        // min(count,8)−1】，older 槽 = (cur−1)&7（满历史时 = 7 个间隔前，与 OSS 等价；
        // 部分历史时 WG 为渐变爬升，OSS 为 count≥3 即跳满——瞬态差异，跟随二进制）。
        if self.got_new_input {
            self.got_new_input = false;
            let newest_time = self.input_at(0).time;
            let older_time = self.input_at(7).time;   // (cur+7)%8 ≡ (cur−1)&7，与二进制同槽
            let latency_frames = LATENCY_FRAMES.min(7.0).max(0.0);
            let ratio = (7.0 - latency_frames) / 7.0;
            self.ideal_latency = (time - (older_time + (newest_time - older_time) * ratio as f64)) as f32;
            self.ideal_latency = self.ideal_latency.max(LATENCY_MINIMUM);
        }
        // latency 以 1.0×|Δ|² s/s 二次缓动逼近理想值
        let d_time = (time - self.time_of_last_output) as f32;
        let d_latency = (LATENCY_VELOCITY * d_time)
            * (1.0f32.min((self.ideal_latency - self.latency).abs().powf(LATENCY_CURVE_POWER)));
        if self.ideal_latency > self.latency {
            self.latency = (self.latency + d_latency).min(self.ideal_latency);
        } else {
            self.latency = (self.latency - d_latency).max(self.ideal_latency);
        }
        self.time_of_last_output = time;

        let output_time = time - self.latency as f64;
        self.extract(output_time, time, self.latency)
    }

    /// OSS extract()：路径点对插值；请求时刻超前最新输入时 chooseNextWaypoint 推测外推。
    fn extract(&mut self, time: f64, frame_time: f64, latency: f32) -> FramePose {
        if time > self.next_waypoint.time {
            self.choose_next_waypoint(time);
        }
        let span = (self.next_waypoint.time - self.previous_waypoint.time) as f32;
        let proportion = if span > 0.0 {
            ((time - self.previous_waypoint.time) as f32) / span
        } else {
            0.0
        };
        let prev = &self.previous_waypoint;
        let next = &self.next_waypoint;
        let pos = [
            prev.pos[0] + (next.pos[0] - prev.pos[0]) * proportion,
            prev.pos[1] + (next.pos[1] - prev.pos[1]) * proportion,
            prev.pos[2] + (next.pos[2] - prev.pos[2]) * proportion,
        ];
        let velocity = if span > 0.0 {
            [
                (next.pos[0] - prev.pos[0]) / span,
                (next.pos[1] - prev.pos[1]) / span,
                (next.pos[2] - prev.pos[2]) / span,
            ]
        } else {
            [0.0; 3]
        };
        FramePose {
            time: frame_time,
            output_time: time,
            latency,
            pos,
            ang: [
                lerp_angle(prev.yaw, next.yaw, proportion),
                prev.pitch + (next.pitch - prev.pitch) * proportion,
                0.0,
            ],
            velocity,
        }
    }

    /// OSS chooseNextWaypoint()：推测外推——把当前路径点对沿运动方向投影到下一已收采样时刻，
    /// 并钳位进该采样 pos±posError 误差盒（误差盒重叠 = 纯误差调整，保持原地）。
    fn choose_next_waypoint(&mut self, time: f64) {
        // 无更新输入：原地站一帧（prev=cur，cur.time 推进到 time）
        if self.input_at(0).time < time {
            self.previous_waypoint = self.next_waypoint;
            self.next_waypoint.time = time;
            return;
        }
        let mut next_input_index = NUM_STORED_INPUTS - 1;
        while next_input_index > 0 {
            if self.input_at(next_input_index).time > time {
                break;
            }
            next_input_index -= 1;
        }
        let look_ahead = *self.input_at(0);
        let next_input = *self.input_at(next_input_index);
        let cur = self.next_waypoint;

        let mut new_wp = Waypoint {
            time: next_input.time,
            pos: [0.0; 3],
            yaw: next_input.yaw,
            pitch: next_input.pitch,
            stored: next_input,
        };

        // lookAhead 时刻的路径点外插 + 钳进 lookAhead 输入误差盒
        let la_rdit = if cur.time > self.previous_waypoint.time {
            ((look_ahead.time - self.previous_waypoint.time)
                / (cur.time - self.previous_waypoint.time)) as f32
        } else {
            0.0
        };
        let prev_pos = self.previous_waypoint.pos;
        let mut look_ahead_pos = [
            prev_pos[0] + (cur.pos[0] - prev_pos[0]) * la_rdit,
            prev_pos[1] + (cur.pos[1] - prev_pos[1]) * la_rdit,
            prev_pos[2] + (cur.pos[2] - prev_pos[2]) * la_rdit,
        ];
        clamp_box(&mut look_ahead_pos, look_ahead.pos, look_ahead.pos_error);

        // 误差盒重叠（纯误差调整）→ 保持原地；否则沿运动方向推进
        let new_box = (next_input.pos, next_input.pos_error);
        let cur_box = (cur.stored.pos, cur.stored.pos_error);
        if boxes_overlap(new_box, cur_box) {
            new_wp.pos = cur.pos;
        } else {
            let prop = if look_ahead.time > cur.time {
                ((next_input.time - cur.time) / (look_ahead.time - cur.time)) as f32
            } else {
                0.0
            };
            new_wp.pos = [
                cur.pos[0] + (look_ahead_pos[0] - cur.pos[0]) * prop,
                cur.pos[1] + (look_ahead_pos[1] - cur.pos[1]) * prop,
                cur.pos[2] + (look_ahead_pos[2] - cur.pos[2]) * prop,
            ];
        }

        // 约束新路径点进入 nextInput 误差盒：越界时把位移投影回运动方向再钳位
        if !box_contains(new_box, new_wp.pos) {
            let mut clamped = new_wp.pos;
            clamp_box(&mut clamped, next_input.pos, next_input.pos_error);
            let look_ahead_vector = [
                new_wp.pos[0] - cur.pos[0],
                new_wp.pos[1] - cur.pos[1],
                new_wp.pos[2] - cur.pos[2],
            ];
            let clamped_vector = [
                clamped[0] - cur.pos[0],
                clamped[1] - cur.pos[1],
                clamped[2] - cur.pos[2],
            ];
            let lsq = dot(look_ahead_vector, look_ahead_vector);
            if lsq > 0.0 {
                let t = dot(clamped_vector, look_ahead_vector) / lsq;
                new_wp.pos = [
                    cur.pos[0] + look_ahead_vector[0] * t,
                    cur.pos[1] + look_ahead_vector[1] * t,
                    cur.pos[2] + look_ahead_vector[2] * t,
                ];
            } else {
                new_wp.pos = cur.pos;
            }
            clamp_box(&mut new_wp.pos, next_input.pos, next_input.pos_error);
        }

        self.previous_waypoint = cur;
        self.next_waypoint = new_wp;
    }

    /// 当前延迟（秒）——裁判探针与质量标注用。
    #[allow(dead_code)]
    pub fn latency(&self) -> f32 {
        self.latency
    }
}

/// 60Hz 帧网格时间线：按文件序喂入 type=10 采样，逐帧推进 output()（游戏每帧渲染一次），
/// 查询 = 取事件时刻所在帧的渲染位姿。帧率影响 latency 缓动的离散化，量级可忽略。
pub struct FilteredTimeline {
    start: f64,
    dt: f64,
    frames: Vec<FramePose>,
}

impl FilteredTimeline {
    /// 按时钟升序构建（内部兜底排序）。时间线覆盖 [首采样, 末采样+0.5s]。
    pub fn build(samples: &[St10Sample]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut sorted: Vec<&St10Sample> = samples.iter().collect();
        sorted.sort_by(|a, b| a.clock.partial_cmp(&b.clock).unwrap());
        const FRAME_DT: f64 = 1.0 / 60.0;
        let start = sorted[0].clock as f64;
        let end = sorted[sorted.len() - 1].clock as f64 + 0.5;
        let frames_cap = ((end - start) / FRAME_DT).ceil() as usize + 1;
        let mut filter = AvatarFilter::new();
        let mut frames = Vec::with_capacity(frames_cap);
        let mut next_input = 0usize;
        let mut t = start;
        while t <= end || next_input < sorted.len() {
            while next_input < sorted.len() && (sorted[next_input].clock as f64) <= t {
                let s = sorted[next_input];
                filter.input(s.clock as f64, s.pos, s.pos_error, s.yaw, s.pitch);
                next_input += 1;
            }
            frames.push(filter.output(t));
            t += FRAME_DT;
        }
        Some(Self { start, dt: FRAME_DT, frames })
    }

    /// 事件时刻的渲染位姿。`at_or_after=true` 取首个 ≥t 的帧（包在帧首网络泵处理后渲染于当帧）。
    pub fn pose_at(&self, t: f64, at_or_after: bool) -> Option<FramePose> {
        if self.frames.is_empty() {
            return None;
        }
        let idx = (t - self.start) / self.dt;
        let i = if at_or_after {
            idx.ceil()
        } else {
            idx.floor()
        };
        let i = (i.max(0.0) as usize).min(self.frames.len() - 1);
        Some(self.frames[i])
    }

    #[allow(dead_code)]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// 时间线实际覆盖的绝对时刻范围 [start, end]（首帧 = 首输入时刻，其后为 60Hz 外推/钳位）
    pub fn time_range(&self) -> (f64, f64) {
        (self.start, self.start + self.dt * (self.frames.len() as f64 - 1.0).max(0.0))
    }
}

// ---------- 几何助手（语义对应 OSS Vector3/BoundingBox/Angle） ----------

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 分量级钳位进 pos±err 误差盒
fn clamp_box(v: &mut [f32; 3], center: [f32; 3], err: [f32; 3]) {
    for i in 0..3 {
        v[i] = v[i].clamp(center[i] - err[i], center[i] + err[i]);
    }
}

fn box_contains(box_: ([f32; 3], [f32; 3]), p: [f32; 3]) -> bool {
    let (c, e) = box_;
    (0..3).all(|i| p[i] >= c[i] - e[i] && p[i] <= c[i] + e[i])
}

/// 两误差盒是否重叠（OSS BoundingBox::intersects(BoundingBox)：逐轴区间重叠）
fn boxes_overlap(a: ([f32; 3], [f32; 3]), b: ([f32; 3], [f32; 3])) -> bool {
    let (ca, ea) = a;
    let (cb, eb) = b;
    (0..3).all(|i| (ca[i] - cb[i]).abs() <= ea[i] + eb[i])
}

/// 角度最短弧插值（OSS Angle::lerp：差值归一化到 [−π, π]）
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = b - a;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    a + d * t
}

// ---------- 裁判探针（逆向文档 §7.4 三层判定协议的第二层：真实回放分布证据） ----------
//
// 运行：WOTB_FILTER_PROBE=replay_samples cargo test referee_probe -- --ignored --nocapture
// （env 给单个 .wotbreplay 路径或目录；缺省 replay_samples/）
//
// 判定项（预期已预登记，判据 = 分布形态而非单点）：
//   R1 隐含滞后：对机动目标（≥1.5 m/s），argmin_t' |render(t') − judgment| 的 t' − t
//      应集中在 [0.00, 0.45]s、中位 ≈0.2s（渲染滞后 + raw 采样滞后的合成）；
//   R2 latency 状态：命中帧滤波器 latency ∈ [0.02, 0.35]，且随战斗时间增长（缓动收敛）；
//   R3 静止目标：渲染-判定锚点距离中位 ≤ 0.5m（滞后对静止目标不可见）；
//   R4 物理连续性：渲染时间线帧间速度 ≤ 30 m/s（跳变被误差盒钳位吸收，无爆炸）。

#[cfg(test)]
mod referee {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    fn collect_replay_files(root: &Path) -> Vec<std::path::PathBuf> {
        if root.is_file() {
            return vec![root.to_path_buf()];
        }
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                if e.path().extension().map(|x| x == "wotbreplay").unwrap_or(false) {
                    out.push(e.path());
                }
            }
        }
        out.sort();
        out
    }

    fn median(v: &mut Vec<f32>) -> f32 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() { 0.0 } else { v[v.len() / 2] }
    }
    fn percentile(v: &mut Vec<f32>, p: usize) -> f32 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() { 0.0 } else { v[v.len() * p / 100] }
    }

    #[test]
    #[ignore = "裁判探针：WOTB_FILTER_PROBE=<path|dir> cargo test referee_probe -- --ignored --nocapture"]
    fn referee_probe() {
        let root = std::env::var("WOTB_FILTER_PROBE").unwrap_or_else(|_| "replay_samples".into());
        let files = collect_replay_files(Path::new(&root));
        assert!(!files.is_empty(), "未找到回放文件：{root}");
        eprintln!("=== 渲染层锚点裁判实验（{} 个回放）===", files.len());

        // 跨回放聚合
        let mut lag_all: Vec<f32> = Vec::new();
        let mut lat_all: Vec<f32> = Vec::new();
        let mut stat_dist_all: Vec<f32> = Vec::new();
        let mut n_moving = 0usize;
        let mut n_stationary = 0usize;
        let mut max_frame_speed_all: f32 = 0.0;
        let mut frames_total = 0usize;
        let mut frames_over_30 = 0usize;
        let mut render_vs_raw_all: Vec<f32> = Vec::new();
        let mut frames_checked = 0usize;

        for file in &files {
            let f = std::fs::File::open(file).unwrap();
            let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap();
            let data = replay.read_data().unwrap();
            let u32le = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            let packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
                let t = match &pkt.payload {
                    wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
                    wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                    wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
                };
                (t, pkt.clock_secs, &pkt.raw_payload[..])
            }).collect();

            // per-entity type=10 流（含 pos_error）
            let mut st10: HashMap<u32, Vec<St10Sample>> = HashMap::new();
            // method8 命中（文件序状态机：J = 受击者最后已知 type=10 位姿，判定层锚点）
            let mut hits: Vec<(u32, u32, f32, [f32; 3])> = Vec::new();
            for (t2, clock, p) in &packets {
                if *t2 == 10 && p.len() >= 48 {
                    st10.entry(u32le(&p[0..4])).or_default().push(St10Sample {
                        clock: *clock,
                        pos: [f32le(&p[12..16]), f32le(&p[16..20]), f32le(&p[20..24])],
                        pos_error: [f32le(&p[24..28]), f32le(&p[28..32]), f32le(&p[32..36])],
                        yaw: f32le(&p[36..40]), pitch: f32le(&p[40..44]), roll: f32le(&p[44..48]),
                    });
                    continue;
                }
                if *t2 == 8 && p.len() >= 22 {
                    if u32le(&p[4..8]) != 0x08 { continue; }
                    let alen = u32le(&p[8..12]) as usize;
                    if alen < 10 || 12 + alen > p.len() { continue; }
                    let a = &p[12..12 + alen];
                    if a[8] != 0x01 { continue; }
                    let victim = u32le(&a[4..8]);
                    if let Some(s) = st10.get(&victim).and_then(|v| v.last()) {
                        hits.push((u32le(&a[0..4]), victim, *clock, s.pos));
                    }
                }
            }
            if hits.is_empty() { continue; }

            // 惰性时间线
            let mut timelines: HashMap<u32, FilteredTimeline> = HashMap::new();
            let mut lag_file: Vec<f32> = Vec::new();
            let mut lat_file: Vec<f32> = Vec::new();
            let mut stat_file: Vec<f32> = Vec::new();
            for (_shooter, victim, t_hit, judgment) in &hits {
                let Some(samples) = st10.get(victim) else { continue };
                if !timelines.contains_key(victim) {
                    if let Some(tl) = FilteredTimeline::build(samples) {
                        timelines.insert(*victim, tl);
                    }
                }
                let Some(tl) = timelines.get(victim) else { continue };
                let Some(pose) = tl.pose_at(*t_hit as f64, true) else { continue };

                // 目标速度（锚点 ±0.15s 邻近采样）
                let speed = {
                    let mut v: Option<f32> = None;
                    for w in samples.windows(2) {
                        let (a, b) = (&w[0], &w[1]);
                        if a.clock >= *t_hit - 0.35 && b.clock <= *t_hit + 0.35 && b.clock > a.clock {
                            let d = ((b.pos[0]-a.pos[0]).powi(2) + (b.pos[1]-a.pos[1]).powi(2)
                                + (b.pos[2]-a.pos[2]).powi(2)).sqrt();
                            v = Some(d / (b.clock - a.clock)).or(v);
                        }
                    }
                    v.unwrap_or(0.0)
                };

                lat_file.push(pose.latency);
                if speed < 0.5 {
                    stat_file.push(dist3(pose.pos, *judgment));
                    n_stationary += 1;
                } else if speed >= 1.5 {
                    // 隐含滞后：argmin_t' |render(t') − judgment|，t' ∈ [t−0.15, t+0.6]
                    let mut best: Option<(f32, f32)> = None;
                    for fr in &tl.frames {
                        let dt = (fr.time - *t_hit as f64) as f32;
                        if !(-0.15..=0.6).contains(&dt) { continue; }
                        let d = dist3(fr.pos, *judgment);
                        if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                            best = Some((d, dt));
                        }
                    }
                    if let Some((_, l)) = best {
                        lag_file.push(l);
                        n_moving += 1;
                    }
                } else {
                    n_stationary += 1;
                }
            }
            // 物理连续性（R4）：验证"渲染位 ≈ output_time 时刻的 raw 真值"全局收敛。
            // 注：不判帧间速度上限——实体中途进入 AoI 时自身 latency 未收敛，stand-still 后
            // 的追赶滑移（OSS 同款代码，游戏中新亮点坦克同样滑移）与掉崖等真实高速运动
            // 都会产生合法的高帧速，速度上限判据会误杀保真行为。
            for (eid, tl) in &timelines {
                let mut v: Vec<&St10Sample> = st10.get(eid).unwrap().iter().collect();
                v.sort_by(|a, b| a.clock.partial_cmp(&b.clock).unwrap());
                let clocks: Vec<f64> = v.iter().map(|s| s.clock as f64).collect();
                for fr in tl.frames.iter().step_by(10) {
                    // raw 真值参照 = 距 output_time 时间最近（非距离）的输入采样；>0.3s 视为数据间隙跳过
                    let idx = clocks.partition_point(|c| *c < fr.output_time);
                    let mut best_time_gap = f64::MAX;
                    let mut best_i = None;
                    for i in [idx.wrapping_sub(1), idx] {
                        if i < v.len() {
                            let gap = (clocks[i] - fr.output_time).abs();
                            if gap < best_time_gap {
                                best_time_gap = gap;
                                best_i = Some(i);
                            }
                        }
                    }
                    let Some(i) = best_i else { continue };
                    if best_time_gap > 0.3 { continue; }
                    render_vs_raw_all.push(dist3(v[i].pos, fr.pos));
                    frames_checked += 1;
                }
                for w in tl.frames.windows(2) {
                    let dt = (w[1].time - w[0].time) as f32;
                    if dt <= 0.0 { continue; }
                    let spd = dist3(w[1].pos, w[0].pos) / dt;
                    if spd > 30.0 { frames_over_30 += 1; }
                    max_frame_speed_all = max_frame_speed_all.max(spd);
                }
                frames_total += tl.frames.len();
            }
            let name = file.file_name().unwrap().to_string_lossy().to_string();
            eprintln!("--- {name}: 命中 {} 发，机动 {} / 静止 {}",
                hits.len(), lag_file.len(), stat_file.len());
            eprintln!("    latency@hit  med={:.3}s", median(&mut lat_file));
            if !lag_file.is_empty() {
                eprintln!("    隐含滞后     p25={:+.3} med={:+.3} p75={:+.3}s",
                    percentile(&mut lag_file, 25), median(&mut lag_file), percentile(&mut lag_file, 75));
            }
            if !stat_file.is_empty() {
                eprintln!("    静止偏差     med={:.3}m", median(&mut stat_file));
            }
            lag_all.extend(lag_file);
            lat_all.extend(lat_file);
            stat_dist_all.extend(stat_file);
        }

        eprintln!("=== 汇总（机动 {n_moving} / 静止 {n_stationary}）===");
        let mut verdicts = Vec::new();
        // R1 隐含滞后
        let lag_med = median(&mut lag_all);
        let r1 = (0.0..=0.45).contains(&lag_med);
        eprintln!("R1 隐含滞后 med={lag_med:+.3}s（预期 [0.00,0.45]，≈latency+采样滞后）→ {}", verdict(r1));
        verdicts.push(r1);
        // R2 latency 状态
        let lat_med = median(&mut lat_all);
        let r2 = (0.02..=0.35).contains(&lat_med);
        eprintln!("R2 latency@hit med={lat_med:.3}s（预期 [0.02,0.35]，随战斗时长收敛）→ {}", verdict(r2));
        verdicts.push(r2);
        // R3 静止目标偏差
        let stat_med = median(&mut stat_dist_all);
        let r3 = stat_med <= 0.5;
        eprintln!("R3 静止目标渲染-判定偏差 med={stat_med:.3}m（预期 ≤0.5）→ {}", verdict(r3));
        verdicts.push(r3);
        // R4 物理收敛：渲染位 ≈ output_time 时刻的 raw 真值（全局采样）
        let r4_med = median(&mut render_vs_raw_all);
        let r4_p99 = percentile(&mut render_vs_raw_all, 99);
        let r4 = r4_med <= 2.0 && r4_p99 <= 15.0;
        eprintln!("R4 渲染-vs-raw@output_time（{} 帧采样）med={r4_med:.3}m p99={r4_p99:.3}m（预期 med≤2 / p99≤15）→ {}",
            frames_checked, verdict(r4));
        eprintln!("    信息项：帧间最大速度 {:.1} m/s（含 AoI 进入追赶滑移/掉崖等保真高速，不作判定）",
            max_frame_speed_all);
        verdicts.push(r4);
        eprintln!("=== 总判定：{}/4 通过 ===", verdicts.iter().filter(|v| **v).count());
        assert!(verdicts.iter().all(|v| *v), "裁判实验存在未通过项，见上方报告");
    }

    fn verdict(ok: bool) -> &'static str {
        if ok { "PASS" } else { "FAIL" }
    }

    fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
        ((a[0]-b[0]).powi(2) + (a[1]-b[1]).powi(2) + (a[2]-b[2]).powi(2)).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(clock: f32, pos: [f32; 3], err: f32) -> St10Sample {
        St10Sample {
            clock,
            pos,
            pos_error: [err; 3],
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        }
    }

    /// 静止目标：任意时刻渲染位姿 == 输入位置（精确相等）
    #[test]
    fn stationary_target_exact() {
        let samples = vec![
            sample(10.0, [1.0, 2.0, 3.0], 0.05),
            sample(10.1, [1.0, 2.0, 3.0], 0.05),
            sample(10.2, [1.0, 2.0, 3.0], 0.05),
        ];
        let tl = FilteredTimeline::build(&samples).unwrap();
        for t in [10.0f64, 10.05, 10.15, 10.35, 10.65] {
            let pose = tl.pose_at(t, true).unwrap();
            assert_eq!(pose.pos, [1.0, 2.0, 3.0]);
            assert_eq!(pose.ang[2], 0.0);
        }
    }

    /// 匀速直线（10Hz 采样，5 m/s）：稳态渲染位置 ≈ t−latency 时刻的真实位置；
    /// latency 收敛后（>0.1s 输入间隔）推测外推全程平滑、速度估计贴真值；
    /// 输入停止后原地站住（推测外推无更新输入分支）。
    #[test]
    fn constant_velocity_tracks_lagged_truth() {
        let mut samples = Vec::new();
        for i in 0..600 {
            let t = 20.0 + i as f32 * 0.1;
            samples.push(sample(t, [100.0 + 5.0 * (t - 20.0), 50.0, 10.0], 0.05));
        }
        let tl = FilteredTimeline::build(&samples).unwrap();
        // 早期（t=23s）：latency 从 0.02 向 ~0.2 二次缓动中（OSS 特性：dLatency=|Δ|²，
        // Δ≈0.2 → 初速仅 0.04s/s，需数十秒收敛）；此阶段 output_time 可能越过最新输入
        // 触发 stand-still（原地等下一包）——只验位置跟踪，不验速度。
        let early = tl.pose_at(23.05, true).unwrap();
        let truth_early = (5.0 * (early.output_time - 20.0)) as f32;
        let err = (early.pos[0] - (100.0 + truth_early)).abs();
        assert!(err < 0.5, "早期渲染位置偏离 output_time 真值 {}m", err);
        // 收敛段（t=60s， latency ≈ 0.2 > 0.1 间隔）：速度估计贴真值
        let pose = tl.pose_at(60.05, true).unwrap();
        assert!(
            (0.15..=0.25).contains(&pose.latency),
            "收敛段 latency 应 ≈0.2，实际 {}", pose.latency
        );
        let truth_at_output_t = (5.0 * (pose.output_time - 20.0)) as f32;
        let err = (pose.pos[0] - (100.0 + truth_at_output_t)).abs();
        assert!(err < 0.5, "渲染位置偏离 output_time 真值 {}m（latency={}）", err, pose.latency);
        assert!(
            (pose.velocity[0] - 5.0).abs() < 0.3,
            "速度估计 {:?} 偏离 5.0",
            pose.velocity
        );
        // 输入停止（末采样 79.9s）后再查：应原地站住 = 最后输入位置
        let end_pose = tl.pose_at(80.35, true).unwrap();
        assert!((end_pose.pos[0] - (100.0 + 5.0 * 59.9)).abs() < 0.5);
    }

    /// 乱序/重复时间样本被丢弃（OSS input 越新丢弃规则）
    #[test]
    fn out_of_order_inputs_dropped() {
        let samples = vec![
            sample(10.0, [0.0; 3], 0.05),
            sample(10.1, [1.0, 0.0, 0.0], 0.05),
            sample(10.05, [2.0, 0.0, 0.0], 0.05), // 乱序：应被丢弃
        ];
        let tl = FilteredTimeline::build(&samples).unwrap();
        let pose = tl.pose_at(10.3, true).unwrap();
        assert!((pose.pos[0] - 1.0).abs() < 0.1, "乱序样本不得污染历史，pos={:?}", pose.pos);
    }

    /// 误差盒钳位：巨大 posError 突跳（AoI 通道切换式跳变）被钳位吸收，渲染轨迹仍连续
    #[test]
    fn teleport_jump_clamped() {
        let mut samples = Vec::new();
        for i in 0..30 {
            let t = 5.0 + i as f32 * 0.1;
            samples.push(sample(t, [0.0 + i as f32 * 2.0, 0.0, 0.0], 0.05)); // 20 m/s 行驶
        }
        // AoI 切换式大跳变（100m）+ 小误差盒
        samples.push(sample(8.0, [100.0, 0.0, 0.0], 0.05));
        let tl = FilteredTimeline::build(&samples).unwrap();
        // 跳变后帧间最大位移有界（钳位吸收，不瞬移）
        let mut max_step: f32 = 0.0;
        for w in tl.frames.windows(2) {
            let d = [
                w[1].pos[0] - w[0].pos[0],
                w[1].pos[1] - w[0].pos[1],
                w[1].pos[2] - w[0].pos[2],
            ];
            max_step = max_step.max(dot(d, d).sqrt());
        }
        assert!(max_step < 10.0, "跳变应被吸收，帧间最大位移 {:.1}m", max_step);
    }
}
