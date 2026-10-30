//! 录像者相机状态（轻量独立能力 `parseRecorderCamera`，契约见
//! `docs/recorder-camera.md`）。
//!
//! 证据边界：
//! - Type39 = 世界系瞄准方向 + 瞄准射线一点；抽稀为关键帧；
//! - Type23 = ARCADE/SNIPER 模式族，raw u32 无损透传，含语义未闭合的高位；
//! - Type31 = 开镜缩放 raw 值，全量透传。它可能是连续轨迹，也可能只有离散几档，
//!   单位未闭合，这里不解释为 FOV 度数或倍率；
//! - method36 field6.field1 = 动态扩散 bloom，稀疏样本；
//! - method49 `BackwardZoom` 按设备形态分段，回放里没有「本场生效分段」的标记，
//!   所以只在各分段取值一致时输出。
//!
//! 结构校验 fail-closed：包形状不符合已验证协议时整体返回 `None`，不按版本白名单放行，
//! 也不做数值分布判断。不构建 ReplayModel/PlaybackData，只做定向包扫描。

use std::io::Read;

use flate2::read::ZlibDecoder;
use serde::Serialize;

use super::author;
use super::combat;

/// 能力 DTO 版本。
pub const RECORDER_CAMERA_VERSION: u32 = 1;

const TYPE_MODE: u32 = 23;
const TYPE_ZOOM: u32 = 31;
const TYPE_AIM: u32 = 39;
const TYPE_ENTITY_METHOD: u32 = 8;

const AIM_PAYLOAD_LEN: usize = 28;
const MODE_PAYLOAD_LEN: usize = 4;
const ZOOM_PAYLOAD_LEN: usize = 4;

/// Type23 默认（ARCADE）raw 值：开局必为此值，两个开镜 session 之间必须经过它。
const MODE_DEFAULT_RAW: u32 = 1;
/// 低 31 位的模式码；高位语义未闭合，但属于同一开镜族。
const MODE_CODE_MASK: u32 = 0x7fff_ffff;

/// Type31 相对 session 边界的时钟容差：退出时刻的末样本与 Type23 同钟，
/// 进入时刻的首样本可能同钟，容差只吸收包序与 f32 时钟误差。
const ZOOM_SESSION_SLACK_S: f32 = 0.02;

/// 抽稀容差。开镜 FOV 最小 6.75°，0.02° 约等于 1080p 画面 3 像素。
const AIM_ANGLE_TOLERANCE_RAD: f64 = 0.02 * std::f64::consts::PI / 180.0;
const AIM_RAY_TOLERANCE_M: f64 = 2.0;

/// method49 单条解压上限（实测约 4–20 KiB），防止坏包触发超大分配。
const OPTIONS_MAX_DECOMPRESSED: u64 = 4 * 1024 * 1024;
const OPTIONS_METHOD: u32 = 49;
const OPTIONS_ZLIB_OFFSET: usize = 7;

/// 录像者相机能力输出。各列同组等长、按 clock 升序。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecorderCamera {
    pub version: u32,
    pub author_eid: u32,
    pub aim: AimTrack,
    pub mode: ModeTrack,
    pub zoom: ScalarTrack<f32>,
    pub bloom: ScalarTrack<f64>,
    pub client_options: ClientCameraOptions,
}

/// Type39 关键帧。`yaw` 弧度且已解卷绕；`pitch` 弧度，正为仰角；
/// `ray_*` 为原始 replay 世界坐标。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AimTrack {
    pub t: Vec<f32>,
    pub yaw: Vec<f32>,
    pub pitch: Vec<f32>,
    pub ray_x: Vec<f32>,
    pub ray_y: Vec<f32>,
    pub ray_z: Vec<f32>,
}

/// Type23 无损序列。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ModeTrack {
    pub t: Vec<f32>,
    pub raw: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScalarTrack<T> {
    pub t: Vec<f32>,
    pub raw: Vec<T>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct ClientCameraOptions {
    /// 各设备形态分段取值一致时的 `BackwardZoom`；不一致或缺失 = `None`。
    pub backward_zoom: Option<bool>,
}

/// 从完整 .wotbreplay 字节构建。`Ok(None)` = 作者未解析或结构校验未通过。
pub fn from_replay_bytes(bytes: &[u8]) -> anyhow::Result<Option<RecorderCamera>> {
    let raw = super::packets::read_raw_packets(bytes)?;
    let packets: Vec<(u32, f32, &[u8])> = raw
        .iter()
        .map(|p| (p.packet_type, p.clock_secs, p.payload.as_slice()))
        .collect();
    let author_eid = author::author_eid_from_replay(bytes, &packets)?;
    Ok(from_packets(author_eid, &packets))
}

/// 包流 → 能力结果（纯逻辑，便于构造负例）。
pub fn from_packets(author_eid: u32, packets: &[(u32, f32, &[u8])]) -> Option<RecorderCamera> {
    if author_eid == 0 {
        return None;
    }
    let mode = collect_mode(packets)?;
    let zoom = collect_zoom(packets, &mode)?;
    let aim = collect_aim(packets)?;
    let bloom = collect_bloom(packets);
    Some(RecorderCamera {
        version: RECORDER_CAMERA_VERSION,
        author_eid,
        aim,
        mode,
        zoom,
        bloom,
        client_options: ClientCameraOptions {
            backward_zoom: collect_backward_zoom(packets),
        },
    })
}

/// Type23：开局必须是默认值，所有迁移都经过默认值，模式码只有 {0,1}。
fn collect_mode(packets: &[(u32, f32, &[u8])]) -> Option<ModeTrack> {
    let mut samples: Vec<(f32, u32)> = Vec::new();
    for (kind, clock, payload) in packets {
        if *kind != TYPE_MODE {
            continue;
        }
        let bytes: [u8; MODE_PAYLOAD_LEN] = (*payload).try_into().ok()?;
        if !clock.is_finite() {
            return None;
        }
        samples.push((*clock, u32::from_le_bytes(bytes)));
    }
    samples.sort_by(|a, b| a.0.total_cmp(&b.0));

    let first = samples.first()?;
    if first.1 != MODE_DEFAULT_RAW {
        return None;
    }
    for pair in samples.windows(2) {
        if pair[0].1 != MODE_DEFAULT_RAW && pair[1].1 != MODE_DEFAULT_RAW {
            return None;
        }
    }
    if samples.iter().any(|(_, raw)| raw & MODE_CODE_MASK > 1) {
        return None;
    }
    Some(ModeTrack {
        t: samples.iter().map(|s| s.0).collect(),
        raw: samples.iter().map(|s| s.1).collect(),
    })
}

/// 开镜 session 列表 `[进入, 退出)`；末尾未退出的 session 延伸到无穷。
fn scoped_sessions(mode: &ModeTrack) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    let mut open: Option<f32> = None;
    for (t, raw) in mode.t.iter().zip(&mode.raw) {
        match (*raw == MODE_DEFAULT_RAW, open) {
            (false, None) => open = Some(*t),
            (true, Some(start)) => {
                out.push((start, *t));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        out.push((start, f32::INFINITY));
    }
    out
}

/// Type31：全量透传，但每个样本都必须落在开镜 session 内。
fn collect_zoom(packets: &[(u32, f32, &[u8])], mode: &ModeTrack) -> Option<ScalarTrack<f32>> {
    let mut samples: Vec<(f32, f32)> = Vec::new();
    for (kind, clock, payload) in packets {
        if *kind != TYPE_ZOOM {
            continue;
        }
        let bytes: [u8; ZOOM_PAYLOAD_LEN] = (*payload).try_into().ok()?;
        let value = f32::from_le_bytes(bytes);
        if !clock.is_finite() || !value.is_finite() {
            return None;
        }
        samples.push((*clock, value));
    }
    samples.sort_by(|a, b| a.0.total_cmp(&b.0));

    let sessions = scoped_sessions(mode);
    let mut next = 0usize;
    for (clock, _) in &samples {
        while next < sessions.len() && *clock > sessions[next].1 + ZOOM_SESSION_SLACK_S {
            next += 1;
        }
        let inside = sessions
            .get(next)
            .is_some_and(|(start, _)| *clock >= *start - ZOOM_SESSION_SLACK_S);
        if !inside {
            return None;
        }
    }
    Some(ScalarTrack {
        t: samples.iter().map(|s| s.0).collect(),
        raw: samples.iter().map(|s| s.1).collect(),
    })
}

/// Type39：长度必须全部合法；空流 = 不支持。方向解卷绕后按容差抽稀。
fn collect_aim(packets: &[(u32, f32, &[u8])]) -> Option<AimTrack> {
    let wrong_shape = packets
        .iter()
        .any(|(kind, _, payload)| *kind == TYPE_AIM && payload.len() != AIM_PAYLOAD_LEN);
    if wrong_shape {
        return None;
    }
    let mut frames = combat::collect_type39_frames(packets);
    frames.retain(|f| {
        f.clock.is_finite()
            && f.gun_yaw.is_finite()
            && f.gun_pitch_world.is_finite()
            && f.ray_point.iter().all(|v| v.is_finite())
    });
    frames.sort_by(|a, b| a.clock.total_cmp(&b.clock));
    // 同钟保留包流最后一帧（sort_by 稳定）。
    frames.reverse();
    frames.dedup_by(|later, earlier| later.clock == earlier.clock);
    frames.reverse();
    if frames.is_empty() {
        return None;
    }

    let t: Vec<f64> = frames.iter().map(|f| f.clock as f64).collect();
    let yaw = unwrap_radians(frames.iter().map(|f| f.gun_yaw as f64));
    let pitch: Vec<f64> = frames.iter().map(|f| f.gun_pitch_world as f64).collect();
    let ray = |axis: usize| -> Vec<f64> {
        frames.iter().map(|f| f.ray_point[axis] as f64).collect()
    };
    let (ray_x, ray_y, ray_z) = (ray(0), ray(1), ray(2));
    let keep = simplify(
        &t,
        &[
            (&yaw, AIM_ANGLE_TOLERANCE_RAD),
            (&pitch, AIM_ANGLE_TOLERANCE_RAD),
            (&ray_x, AIM_RAY_TOLERANCE_M),
            (&ray_y, AIM_RAY_TOLERANCE_M),
            (&ray_z, AIM_RAY_TOLERANCE_M),
        ],
    );
    let column = |values: &[f64], scale: f64| -> Vec<f32> {
        keep.iter()
            .map(|&i| ((values[i] * scale).round() / scale) as f32)
            .collect()
    };
    Some(AimTrack {
        t: column(&t, 1e4),
        yaw: column(&yaw, 1e5),
        pitch: column(&pitch, 1e5),
        ray_x: column(&ray_x, 1e1),
        ray_y: column(&ray_y, 1e1),
        ray_z: column(&ray_z, 1e1),
    })
}

fn unwrap_radians(values: impl Iterator<Item = f64>) -> Vec<f64> {
    let tau = std::f64::consts::TAU;
    let mut out: Vec<f64> = Vec::new();
    for value in values {
        let next = match out.last() {
            Some(previous) => previous + (value - previous + tau / 2.0).rem_euclid(tau) - tau / 2.0,
            None => value,
        };
        out.push(next);
    }
    out
}

/// 多列 Ramer–Douglas–Peucker：保留点使每列线性插值误差都不超过该列容差。
fn simplify(t: &[f64], columns: &[(&[f64], f64)]) -> Vec<usize> {
    let last = t.len() - 1;
    let mut keep = vec![false; t.len()];
    keep[0] = true;
    keep[last] = true;
    let mut stack = vec![(0usize, last)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let span = t[b] - t[a];
        let mut worst = 1.0;
        let mut split = None;
        for i in a + 1..b {
            let ratio = (t[i] - t[a]) / span;
            for (values, tolerance) in columns {
                let expected = values[a] + (values[b] - values[a]) * ratio;
                let error = (values[i] - expected).abs() / tolerance;
                if error > worst {
                    worst = error;
                    split = Some(i);
                }
            }
        }
        if let Some(i) = split {
            keep[i] = true;
            stack.push((a, i));
            stack.push((i, b));
        }
    }
    (0..t.len()).filter(|&i| keep[i]).collect()
}

fn collect_bloom(packets: &[(u32, f32, &[u8])]) -> ScalarTrack<f64> {
    let mut samples: Vec<(f32, f64)> = combat::collect_aim_snapshots(packets)
        .into_iter()
        .filter_map(|(clock, snapshot)| Some((clock, snapshot.bloom?)))
        .filter(|(clock, bloom)| clock.is_finite() && bloom.is_finite())
        .collect();
    samples.sort_by(|a, b| a.0.total_cmp(&b.0));
    ScalarTrack {
        t: samples.iter().map(|s| s.0).collect(),
        raw: samples.iter().map(|s| s.1).collect(),
    }
}

/// method49 恰好一条时，解压其 pickle 并读取全部 `BackwardZoom` 分段。
fn collect_backward_zoom(packets: &[(u32, f32, &[u8])]) -> Option<bool> {
    let mut messages = packets.iter().filter(|(kind, _, payload)| {
        *kind == TYPE_ENTITY_METHOD
            && payload.len() >= 12
            && u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]])
                == OPTIONS_METHOD
    });
    let (_, _, payload) = messages.next()?;
    if messages.next().is_some() {
        return None;
    }
    let args_len = u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]) as usize;
    let args = payload.get(12..12usize.checked_add(args_len)?)?;
    let compressed = args.get(OPTIONS_ZLIB_OFFSET..)?;
    let mut options = Vec::new();
    ZlibDecoder::new(compressed)
        .take(OPTIONS_MAX_DECOMPRESSED)
        .read_to_end(&mut options)
        .ok()?;
    uniform_backward_zoom(&options)
}

/// 协议 0 pickle 形态：`BackwardZoom'\np<memo>\nS'true'`。
/// 任一分段形态不符或取值不一致 → `None`。
fn uniform_backward_zoom(pickle: &[u8]) -> Option<bool> {
    const KEY: &[u8] = b"BackwardZoom'\n";
    let mut found = None;
    let mut rest = pickle;
    while let Some(at) = rest.windows(KEY.len()).position(|w| w == KEY) {
        rest = &rest[at + KEY.len()..];
        let value_at = rest.windows(3).position(|w| w == b"\nS'")?;
        let memo = rest.get(..value_at)?;
        if memo.first() != Some(&b'p') || !memo[1..].iter().all(u8::is_ascii_digit) {
            return None;
        }
        rest = &rest[value_at + 3..];
        let value = if rest.starts_with(b"true'") {
            true
        } else if rest.starts_with(b"false'") {
            false
        } else {
            return None;
        };
        if found.is_some_and(|v| v != value) {
            return None;
        }
        found = Some(value);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode_packet(clock: f32, raw: u32) -> (u32, f32, Vec<u8>) {
        (TYPE_MODE, clock, raw.to_le_bytes().to_vec())
    }
    fn zoom_packet(clock: f32, raw: f32) -> (u32, f32, Vec<u8>) {
        (TYPE_ZOOM, clock, raw.to_le_bytes().to_vec())
    }
    fn aim_packet(clock: f32, yaw_deg: f32) -> (u32, f32, Vec<u8>) {
        let floats = [yaw_deg, 1.0, 10.0, 20.0, 30.0, 0.0, 0.0];
        let payload = floats.iter().flat_map(|v| v.to_le_bytes()).collect();
        (TYPE_AIM, clock, payload)
    }
    fn build(packets: &[(u32, f32, Vec<u8>)]) -> Option<RecorderCamera> {
        let borrowed: Vec<(u32, f32, &[u8])> = packets
            .iter()
            .map(|(kind, clock, payload)| (*kind, *clock, payload.as_slice()))
            .collect();
        from_packets(7, &borrowed)
    }
    fn valid() -> Vec<(u32, f32, Vec<u8>)> {
        vec![
            mode_packet(0.0, 1),
            aim_packet(1.0, 10.0),
            mode_packet(2.0, 0),
            zoom_packet(2.008, 54.0),
            zoom_packet(3.0, 27.0),
            mode_packet(3.0, 1),
            aim_packet(4.0, 20.0),
            mode_packet(5.0, 0x8000_0000),
            zoom_packet(5.0, 6.75),
            mode_packet(6.0, 1),
        ]
    }

    #[test]
    fn valid_stream_keeps_raw_mode_and_all_zoom_samples() {
        let camera = build(&valid()).expect("合法流");
        assert_eq!(camera.mode.raw, vec![1, 0, 1, 0x8000_0000, 1]);
        assert_eq!(camera.zoom.raw, vec![54.0, 27.0, 6.75]);
        assert_eq!(camera.author_eid, 7);
        assert_eq!(camera.client_options.backward_zoom, None);
    }

    #[test]
    fn arcade_only_replay_is_valid() {
        let camera = build(&[mode_packet(0.0, 1), aim_packet(1.0, 0.0)]).expect("全程未开镜");
        assert!(camera.zoom.t.is_empty());
    }

    #[test]
    fn unknown_author_is_rejected() {
        let packets = valid();
        let borrowed: Vec<(u32, f32, &[u8])> =
            packets.iter().map(|(k, c, p)| (*k, *c, p.as_slice())).collect();
        assert!(from_packets(0, &borrowed).is_none());
    }

    #[test]
    fn missing_mode_or_aim_stream_is_rejected() {
        assert!(build(&[aim_packet(1.0, 0.0)]).is_none());
        assert!(build(&[mode_packet(0.0, 1)]).is_none());
    }

    #[test]
    fn mode_stream_that_does_not_start_at_default_is_rejected() {
        assert!(build(&[mode_packet(0.0, 0), aim_packet(1.0, 0.0)]).is_none());
    }

    #[test]
    fn unknown_mode_code_is_rejected() {
        let mut packets = valid();
        packets.push(mode_packet(7.0, 2));
        assert!(build(&packets).is_none());
    }

    #[test]
    fn direct_transition_between_scoped_values_is_rejected() {
        let mut packets = valid();
        packets.push(mode_packet(7.0, 0));
        packets.push(mode_packet(8.0, 0x8000_0000));
        assert!(build(&packets).is_none());
    }

    #[test]
    fn zoom_sample_outside_scoped_session_is_rejected() {
        let mut packets = valid();
        packets.push(zoom_packet(4.0, 27.0));
        assert!(build(&packets).is_none());
    }

    #[test]
    fn wrong_payload_lengths_are_rejected() {
        let mut bad_aim = valid();
        bad_aim.push((TYPE_AIM, 7.0, vec![0; 24]));
        assert!(build(&bad_aim).is_none());
        let mut bad_mode = valid();
        bad_mode.push((TYPE_MODE, 7.0, vec![1, 0]));
        assert!(build(&bad_mode).is_none());
        let mut bad_zoom = valid();
        bad_zoom.push((TYPE_ZOOM, 5.5, vec![0; 8]));
        assert!(build(&bad_zoom).is_none());
    }

    #[test]
    fn open_ended_session_accepts_later_zoom() {
        let mut packets = valid();
        packets.push(mode_packet(7.0, 0));
        packets.push(zoom_packet(100.0, 27.0));
        assert!(build(&packets).is_some());
    }

    #[test]
    fn yaw_is_unwrapped_across_the_seam() {
        let yaw = unwrap_radians([3.1_f64, -3.1].into_iter());
        assert!((yaw[1] - (std::f64::consts::TAU - 3.1)).abs() < 1e-9);
    }

    #[test]
    fn simplify_keeps_corners_and_drops_collinear_points() {
        let t = [0.0, 1.0, 2.0, 3.0, 4.0];
        let line = [0.0, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(simplify(&t, &[(&line, 0.01)]), vec![0, 4]);
        let corner = [0.0, 0.0, 0.0, 5.0, 5.0];
        let kept = simplify(&t, &[(&corner, 0.01)]);
        assert!(kept.contains(&2) && kept.contains(&3), "{kept:?}");
    }

    #[test]
    fn backward_zoom_requires_every_section_to_agree() {
        let agree = b"a BackwardZoom'\np74\nS'true'\np75\ns b BackwardZoom'\np9\nS'true'\n";
        assert_eq!(uniform_backward_zoom(agree), Some(true));
        let differ = b"BackwardZoom'\np1\nS'true'\nBackwardZoom'\np2\nS'false'\n";
        assert_eq!(uniform_backward_zoom(differ), None);
        assert_eq!(uniform_backward_zoom(b"SmartZoom'\np1\nS'true'\n"), None);
        assert_eq!(uniform_backward_zoom(b"BackwardZoom'\nI01\n"), None);
    }
}
