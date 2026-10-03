//! 切面导出的服务端胶水：投影与互验在核心库（`wotb-replay-core::facets`），
//! 本模块只保留 CLI 导出与写盘 IO（路径/坦克名解析/地图显示名都是服务端能力）。

pub use wotb_replay_core::facets::{
    cross_check_author_counters, AiReviewFacet, CrossCheck, PlaybackFacet,
};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// CLI 导出（`wotb-agent facets <file>`）：模型扫描 → 切面 JSON 落盘 + 结算互验报告。
///
/// 注意：CLI 路径不做俯仰极限锚定（空锚定表，炮管俯仰走车体 pitch 兜底）与 GLB 变体
/// 标注——那两步是 viewer/在线链路的增值，不影响切面数据本身的正确性。
/// HoF 不是 Agent 能力（契约 v2）：由消费方从结算/Result 自行投影，此处不导出。
pub fn export_cli(
    file: &Path,
    parts: &str,
    out_dir: Option<&Path>,
    tank_cache: Option<&Path>,
) -> Result<()> {
    use crate::wargaming::tank_resolver::TankResolver;
    use wotb_replay_core::replay::model::{ReplayModel, ScanInput};
    use wotb_replay_core::replay::playback::PlaybackPlayer;

    let want: Vec<String> = parts
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    anyhow::ensure!(!want.is_empty(), "parts 为空（可选 playback/ai）");
    for p in &want {
        anyhow::ensure!(
            matches!(p.as_str(), "playback" | "ai"),
            "未知切面 `{p}`（可选 playback/ai）"
        );
    }

    // 结算（带可选坦克名解析）
    let resolver = tank_cache
        .filter(|p| p.exists())
        .and_then(|p| TankResolver::load_from_json_file(p).ok());
    let summary = match &resolver {
        Some(r) => crate::replay::parser::ReplayParser::with_resolver(r).parse_file(file)?,
        None => crate::replay::parser::ReplayParser::new().parse_file(file)?,
    };
    // 地图显示名以客户端注册表为准（parser 兜底是解析器枚举名）
    let map_name = crate::wargaming::map_assets::display_name(summary.map_id)
        .map(|s| s.to_string())
        .unwrap_or_else(|| summary.map_name.clone());

    // 包流（与 playback_probe 同款类型映射）
    // 原始分帧（不反序列化 payload，与 WASM 通道同源）
    let raw = crate::replay::packets::read_raw_packets(&std::fs::read(file)?)?;
    let packets: Vec<(u32, f32, &[u8])> = raw
        .iter()
        .map(|p| (p.packet_type, p.clock_secs, p.payload.as_slice()))
        .collect();

    let roster: Vec<PlaybackPlayer> = summary
        .players
        .iter()
        .map(|p| PlaybackPlayer {
            account_id: p.account_id,
            nickname: p.nickname.clone(),
            team: p.team,
            tank_id: p.tank_id,
        })
        .collect();
    let limits = crate::replay::combat::GunPitchLimits::new();
    let model = ReplayModel::scan(&ScanInput {
        packets: &packets,
        roster: &roster,
        author_account_id: summary.author_account_id,
        pitch_limits: &limits,
    })?;

    let dir: PathBuf = out_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| file.parent().map(|p| p.to_path_buf()).unwrap_or_default());
    std::fs::create_dir_all(&dir)?;
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("replay")
        .to_string();

    println!(
        "切面导出：{}（{} / {} 人 / 胜方 {}）",
        file.display(),
        map_name,
        summary.players.len(),
        summary.winner_team
    );

    for part in &want {
        match part.as_str() {
            "ai" => {
                let facet = AiReviewFacet::from_model_with_packets(&model, &summary, &packets);
                let path = write_json(&dir, &format!("{stem}.facet.ai.json"), &facet)?;
                println!(
                    "  评审切面   → {}（{} 事件 / {} 实体）",
                    path.display(),
                    facet.events.len(),
                    facet.rosters.len()
                );
            }
            "playback" => {
                let tank_names: HashMap<u32, String> = summary
                    .players
                    .iter()
                    .map(|p| (p.tank_id, p.tank_name.clone()))
                    .filter(|(_, n)| !n.is_empty())
                    .collect();
                let render = crate::replay::playback::PlaybackRenderInput {
                    winner_team: summary.winner_team,
                    map_id: summary.map_id,
                    map_name: map_name.clone(),
                    pitch_limits: &limits,
                    tank_names: &tank_names,
                };
                let facet = crate::replay::playback::from_model(&model, &render)?;
                let path = write_json(&dir, &format!("{stem}.facet.playback.json"), &facet)?;
                println!(
                    "  回放切面   → {}（{} 车 / {} 发 / 可见窗口 {}）",
                    path.display(),
                    facet.vehicles.len(),
                    facet.shots.len(),
                    facet.visibility.len()
                );
            }
            _ => unreachable!(),
        }
    }

    println!("结算互验（0x0c 过程计数 vs 结算总量，作者口径）：");
    for c in cross_check_author_counters(&model, &summary) {
        let verdict = match c.ok() {
            Some(true) => "OK",
            Some(false) => "MISMATCH（需人工判读）",
            None => "N/A（结算缺失）",
        };
        println!(
            "  {:<42} 结算 {:>5}  count {:>5}  value {:>5}  [{verdict}]",
            c.label,
            c.settlement.unwrap_or(0),
            c.counter_count,
            c.counter_value
        );
    }
    Ok(())
}

fn write_json<T: serde::Serialize>(dir: &Path, name: &str, value: &T) -> Result<PathBuf> {
    let path = dir.join(name);
    let w = std::io::BufWriter::new(std::fs::File::create(&path)?);
    serde_json::to_writer_pretty(w, value)?;
    Ok(path)
}
