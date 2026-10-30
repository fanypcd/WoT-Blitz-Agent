//! 录像者相机能力：真实样本回归。
//!
//! 计数来自独立于本实现的原始包分帧脚本，用于防止采集或校验静默丢样本。

use std::path::PathBuf;
use std::time::Instant;

use wotb_replay_core::replay::recorder_camera::{self, RecorderCamera};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/replay_samples")
        .join(name)
}

fn scoped_sessions(camera: &RecorderCamera) -> usize {
    camera.mode.raw.iter().filter(|raw| **raw != 1).count()
}

/// (样本, Type23 包数, 开镜 session, Type31 样本)
const ORACLE: [(&str, usize, usize, usize); 4] = [
    ("20260902_2045__Anonyme_J39_Type_5_Exp_3354568815024678.wotbreplay", 71, 35, 186),
    ("20260902_2053__Anonyme_J20_Type_2605_3354220922674233.wotbreplay", 53, 26, 113),
    ("20260902_2104__Anonyme_A116_XM551_Exp_3355505117896350.wotbreplay", 11, 5, 10),
    ("20261013_1615__Anonyme_Oth71_Titan_Strife_3316571242883186.wotbreplay", 19, 9, 35),
];

#[test]
fn tracked_samples_match_independent_packet_counts() {
    for (name, mode_packets, sessions, zoom_samples) in ORACLE {
        let path = fixture(name);
        if !path.exists() {
            eprintln!("无样本 {name}，跳过");
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let camera = recorder_camera::from_replay_bytes(&bytes)
            .expect("能力构建成功")
            .unwrap_or_else(|| panic!("{name} 不应被结构校验拒绝"));
        assert_eq!(camera.mode.t.len(), mode_packets, "{name} Type23 包数");
        assert_eq!(scoped_sessions(&camera), sessions, "{name} 开镜 session");
        assert_eq!(camera.zoom.t.len(), zoom_samples, "{name} Type31 样本");
        assert_eq!(camera.mode.raw[0], 1, "{name} 开局为默认模式");
        assert!(camera.zoom.raw.iter().all(|v| (6.0..=54.5).contains(v)), "{name}");

        let aim = &camera.aim;
        assert_eq!(aim.t.len(), aim.yaw.len());
        assert_eq!(aim.t.len(), aim.pitch.len());
        assert_eq!(aim.t.len(), aim.ray_x.len());
        assert_eq!(aim.t.len(), aim.ray_y.len());
        assert_eq!(aim.t.len(), aim.ray_z.len());
        assert!(aim.t.windows(2).all(|w| w[0] <= w[1]), "{name} aim 时钟升序");
        assert_eq!(camera.bloom.t.len(), camera.bloom.raw.len());
        assert!(camera.zoom.t.windows(2).all(|w| w[0] <= w[1]), "{name} zoom 时钟升序");
    }
}

/// 全量语料核对：`WOTB_REPLAY_CORPUS` 为分号分隔的目录。
/// 打印体积与耗时；任一回放被拒绝即失败。
#[test]
#[ignore = "需要本地语料"]
fn corpus_is_accepted_within_budget() {
    let dirs = std::env::var("WOTB_REPLAY_CORPUS").expect("设置 WOTB_REPLAY_CORPUS");
    let mut files = Vec::new();
    for dir in dirs.split(';') {
        collect(&PathBuf::from(dir), &mut files);
    }
    assert!(!files.is_empty());
    let (mut max_kib, mut max_ms) = (0usize, 0u128);
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let started = Instant::now();
        let camera = recorder_camera::from_replay_bytes(&bytes).expect("能力构建成功");
        let elapsed = started.elapsed().as_millis();
        let name = path.file_name().unwrap().to_string_lossy();
        let camera = camera.unwrap_or_else(|| panic!("{name} 被结构校验拒绝"));
        let kib = serde_json::to_string(&camera).unwrap().len() / 1024;
        println!(
            "{name:.36} {kib} KiB {elapsed} ms keys={} zoom={} bloom={} backward_zoom={:?}",
            camera.aim.t.len(),
            camera.zoom.t.len(),
            camera.bloom.t.len(),
            camera.client_options.backward_zoom,
        );
        max_kib = max_kib.max(kib);
        max_ms = max_ms.max(elapsed);
    }
    println!("files={} max {max_kib} KiB, {max_ms} ms", files.len());
}

fn collect(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "wotbreplay") {
            out.push(path);
        }
    }
}
