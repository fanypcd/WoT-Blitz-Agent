//! 轻量作者初始位姿能力：真实回放回归 + 跨源证据（Type5 战斗车辆物化位姿）。
//!
//! 证据边界：本测试只断言 **replay 观测到的首条可信世界位姿**，不涉及地图
//! spawn 语义 / NORTH/SOUTH / WOTBTOOLS 坐标转换。

use std::path::PathBuf;

use wotb_replay_core::replay::author;
use wotb_replay_core::replay::packets::{read_raw_packets, RawPacket};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/replay_samples")
        .join(name)
}

fn all_fixtures() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/replay_samples");
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wotbreplay"))
        .collect();
    v.sort();
    v
}

/// 同场 Type5 战斗车辆物化位姿（世界系；offset 14..26）——与 type=10 独立的
/// 初始位姿证据源，用于交叉验证本能力取到的样本确实是作者车辆的世界坐标。
fn type5_materialization_pose(packets: &[RawPacket], eid: u32) -> Option<[f32; 3]> {
    packets.iter().find_map(|pkt| {
        let p = pkt.payload.as_slice();
        if pkt.packet_type != 5
            || p.len() < 34
            || u32::from_le_bytes([p[0], p[1], p[2], p[3]]) != eid
            || u16::from_le_bytes([p[4], p[5]]) != 2
        {
            return None;
        }
        let f = |o: usize| f32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]]);
        Some([f(14), f(18), f(22)])
    })
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// 去掉 battle_results.dat 的副本（模拟未结算/截断回放）：只剩 meta + 包流。
/// 身份必须退回 meta 昵称 → Type5 精确匹配；仍应返回首条作者世界位姿。
fn copy_without_battle_results(path: &PathBuf) -> Option<Vec<u8>> {
    use std::io::{Read, Write};

    let raw = std::fs::read(path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&raw)).ok()?;
    let mut meta = Vec::new();
    zip.by_name("meta.json").ok()?.read_to_end(&mut meta).ok()?;
    let mut data = Vec::new();
    zip.by_name("data.wotreplay")
        .ok()?
        .read_to_end(&mut data)
        .ok()?;

    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let options =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file("meta.json", options).ok()?;
        writer.write_all(&meta).ok()?;
        writer.start_file("data.wotreplay", options).ok()?;
        writer.write_all(&data).ok()?;
        writer.finish().ok()?;
    }
    Some(out)
}

#[test]
fn real_fixture_established_author_initial_position() {
    let path = fixture("2.wotbreplay");
    if !path.exists() {
        eprintln!("无 2.wotbreplay 样本，跳过");
        return;
    }
    let bytes = std::fs::read(&path).unwrap();
    let got = author::from_replay_bytes(&bytes)
        .expect("能力构建成功")
        .expect("作者世界位姿应存在");

    // 既有调查钉定的值（该场作者唯一 Type5 class=2 实体；首条 type=10 世界采样）。
    assert_eq!(got.author_eid, 1_961_229, "作者车辆 eid");
    assert!(
        (got.clock - 0.410_819).abs() < 1e-5,
        "最早采样 clock = {}, 期望 0.410819",
        got.clock
    );
    let expected: [f32; 3] = [-138.756_04, 31.585_73, -165.303_02];
    for (i, (actual, want)) in got.position.iter().zip(expected).enumerate() {
        assert!(
            (*actual - want).abs() < 1e-3,
            "position[{i}] = {actual}, 期望 {want}"
        );
    }
    assert!(got.position.iter().all(|v| v.is_finite()));

    // 独立协议证据：Type5 物化位姿应在同一出生位置附近（首条 type10 已是
    // 录制端到达的观测值，允许厘米级差异）。
    let packets = read_raw_packets(&bytes).unwrap();
    let mat = type5_materialization_pose(&packets, got.author_eid)
        .expect("作者战斗车辆应有 Type5 物化包");
    assert!(
        dist(mat, got.position) < 0.05,
        "Type5 物化位姿 {mat:?} 与 type10 首观测 {:?} 距离过大",
        got.position
    );
}

#[test]
fn returns_position_without_battle_results() {
    let path = fixture("2.wotbreplay");
    let Some(bytes) = copy_without_battle_results(&path) else {
        eprintln!("无法构造无结算副本，跳过");
        return;
    };
    let got = author::from_replay_bytes(&bytes)
        .expect("无 battle_results 时能力不应整体失败")
        .expect("meta 昵称回退应解析出作者位姿");
    assert_eq!(got.author_eid, 1_961_229, "昵称回退与结算路径同一实体");
    assert!((got.clock - 0.410_819).abs() < 1e-5);
    assert!(got.position.iter().all(|v| v.is_finite()));
}

#[test]
fn all_fixtures_author_position_is_finite_and_cross_source_consistent() {
    let mut checked = 0;
    for path in all_fixtures() {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let got = match author::from_replay_bytes(&bytes) {
            Ok(Some(p)) => p,
            Ok(None) => {
                eprintln!("跳过（无作者世界位姿）: {}", path.display());
                continue;
            }
            Err(e) => {
                eprintln!("跳过（能力不适用）: {}: {e:#}", path.display());
                continue;
            }
        };
        assert!(got.author_eid > 0);
        assert!(got.clock.is_finite() && got.clock >= 0.0);
        assert!(
            got.position.iter().all(|v| v.is_finite()),
            "{}: 非有限坐标 {:?}",
            path.display(),
            got.position
        );
        let packets = read_raw_packets(&bytes).unwrap();
        if let Some(mat) = type5_materialization_pose(&packets, got.author_eid) {
            assert!(
                dist(mat, got.position) < 0.1,
                "{}: Type5 {mat:?} vs type10 {:?}",
                path.display(),
                got.position
            );
        } else {
            eprintln!(
                "{}: 无 Type5 class=2 物化包（无法跨源核对，仅锁定 finite）",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "至少一个真实样本可产出作者初始位姿");
}
