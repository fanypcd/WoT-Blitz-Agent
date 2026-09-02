use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use chrono::Utc;

use crate::wargaming::api_client::PlayerStats;

// =====================================================================
//  API 数据快照
//  WG API 不支持按时间查询，因此"定期采集 + 快照差值"来实现阶段性分析。
//  每次 `take` 存一份当时累计战绩，`diff` 比较新旧两次的增减。
// =====================================================================

/// 一份战绩快照：采集时间 + 当时的玩家累计战绩。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// 采集时间戳（秒）
    pub timestamp: i64,
    /// 采集时间（格式化字符串）
    pub datetime: String,
    /// 当时的累计战绩
    pub player: PlayerStats,
}

/// 两次快照之间的差值（阶段性表现变化）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDiff {
    pub from_time: String,
    pub to_time: String,
    pub from_battles: u32,
    pub to_battles: u32,
    pub battles_played: u32,
    pub wins_diff: i64,
    pub losses_diff: i64,
    pub win_rate_from: f64,
    pub win_rate_to: f64,
    pub damage_diff: i64,
    pub avg_damage_from: f64,
    pub avg_damage_to: f64,
    pub frags_diff: i64,
    pub avg_frags_from: f64,
    pub avg_frags_to: f64,
    pub shots_diff: i64,
    pub hits_diff: i64,
    pub hit_rate_from: f64,
    pub hit_rate_to: f64,
    pub xp_diff: i64,
    pub mm_rating_from: Option<f32>,
    pub mm_rating_to: Option<f32>,
    pub rating_delta: Option<f32>,
}

impl Snapshot {
    /// 用当前时间 + 给定战绩构造一份快照。
    pub fn from_player_stats(stats: PlayerStats) -> Self {
        let now = Utc::now();
        Self {
            timestamp: now.timestamp(),
            datetime: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            player: stats,
        }
    }
}

/// 快照存储：在指定目录下读写 `snapshot_{timestamp}.json` 文件。
pub struct SnapshotStore {
    dir: std::path::PathBuf,
}

impl SnapshotStore {
    pub fn new(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf() }
    }

    /// 保存一份快照，返回写入的路径。
    pub fn save(&self, snapshot: &Snapshot) -> Result<std::path::PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let filename = format!("snapshot_{}.json", snapshot.timestamp);
        let path = self.dir.join(&filename);
        let json = serde_json::to_string_pretty(snapshot)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }

    /// 列出目录下全部快照（按时间升序）。
    pub fn list(&self) -> Result<Vec<Snapshot>> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut snapshots = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if !path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("snapshot_")).unwrap_or(false) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(snap) = serde_json::from_str::<Snapshot>(&content) {
                    snapshots.push(snap);
                }
            }
        }
        snapshots.sort_by_key(|s| s.timestamp);
        Ok(snapshots)
    }

    /// 取最新一份快照。
    pub fn latest(&self) -> Result<Option<Snapshot>> {
        let snaps = self.list()?;
        Ok(snaps.into_iter().last())
    }

    /// 取最早一份快照。
    pub fn oldest(&self) -> Result<Option<Snapshot>> {
        let snaps = self.list()?;
        Ok(snaps.into_iter().next())
    }

    /// 计算两份快照的差值（期间场次/胜率/伤害/评级等变化）。
    pub fn diff(&self, from: &Snapshot, to: &Snapshot) -> SnapshotDiff {
        let from_p = &from.player;
        let to_p = &to.player;

        // 用 saturating_sub 避免无符号减法下溢（新快照场次通常 ≥ 旧快照）
        let battles_played = to_p.rating_battles.saturating_sub(from_p.rating_battles);
        let wins_diff = to_p.rating_wins as i64 - from_p.rating_wins as i64;
        let losses_diff = to_p.rating_losses as i64 - from_p.rating_losses as i64;
        let damage_diff = to_p.rating_damage_dealt as i64 - from_p.rating_damage_dealt as i64;
        let frags_diff = to_p.rating_frags as i64 - from_p.rating_frags as i64;
        let shots_diff = to_p.rating_shots as i64 - from_p.rating_shots as i64;
        let hits_diff = to_p.rating_hits as i64 - from_p.rating_hits as i64;
        let xp_diff = to_p.rating_xp as i64 - from_p.rating_xp as i64;

        let n_from = from_p.rating_battles.max(1) as f64;
        let n_to = to_p.rating_battles.max(1) as f64;

        let hit_rate_from = if from_p.rating_shots > 0 {
            from_p.rating_hits as f64 / from_p.rating_shots as f64 * 100.0
        } else { 0.0 };
        let hit_rate_to = if to_p.rating_shots > 0 {
            to_p.rating_hits as f64 / to_p.rating_shots as f64 * 100.0
        } else { 0.0 };

        let rating_delta = match (to_p.rating_mm_rating, from_p.rating_mm_rating) {
            (Some(t), Some(f)) => Some(t - f),
            _ => None,
        };

        SnapshotDiff {
            from_time: from.datetime.clone(),
            to_time: to.datetime.clone(),
            from_battles: from_p.rating_battles,
            to_battles: to_p.rating_battles,
            battles_played,
            wins_diff,
            losses_diff,
            win_rate_from: from_p.rating_wins as f64 / n_from * 100.0,
            win_rate_to: to_p.rating_wins as f64 / n_to * 100.0,
            damage_diff,
            avg_damage_from: from_p.rating_damage_dealt as f64 / n_from,
            avg_damage_to: to_p.rating_damage_dealt as f64 / n_to,
            frags_diff,
            avg_frags_from: from_p.rating_frags as f64 / n_from,
            avg_frags_to: to_p.rating_frags as f64 / n_to,
            shots_diff,
            hits_diff,
            hit_rate_from,
            hit_rate_to,
            xp_diff,
            mm_rating_from: from_p.rating_mm_rating,
            mm_rating_to: to_p.rating_mm_rating,
            rating_delta,
        }
    }

    /// 打印快照列表（人类可读）。
    pub fn print_list(&self, snapshots: &[Snapshot]) {
        if snapshots.is_empty() {
            println!("No snapshots found.");
            return;
        }
        println!();
        println!("  Snapshots ({} total):", snapshots.len());
        println!("  {:<22} {:>8} {:>8} {:>8} {:>8} {:>10} {:>8}",
            "Time", "Bat", "Win%", "Dmg%", "Frag%", "Rating", "mmRat");
        println!("  {}", "-".repeat(80));
        for s in snapshots {
            let p = &s.player;
            let n = p.rating_battles.max(1) as f64;
            let wr = p.rating_wins as f64 / n * 100.0;
            let avg_dmg = p.rating_damage_dealt as f64 / n;
            let avg_frags = p.rating_frags as f64 / n;
            println!("  {:<22} {:>8} {:>7.1}% {:>7.0} {:>7.2} {:>10} {:>8.2}",
                s.datetime, p.rating_battles, wr, avg_dmg, avg_frags,
                p.rating_display_rating.unwrap_or(0),
                p.rating_mm_rating.unwrap_or(0.0));
        }
    }

    /// 打印快照差值（人类可读）。
    pub fn print_diff(&self, diff: &SnapshotDiff) {
        println!();
        println!("========================================================");
        println!("  Snapshot Diff: {} -> {}", diff.from_time, diff.to_time);
        println!("========================================================");
        println!();
        println!("  Battles played:    {}", diff.battles_played);
        println!("  Wins/Losses:        {}/{}", diff.wins_diff, diff.losses_diff);
        println!();
        println!("  {:<20} {:>12} {:>12} {:>10}", "Metric", "From", "To", "Change");
        println!("  {}", "-".repeat(58));

        let wr_change = diff.win_rate_to - diff.win_rate_from;
        let avg_dmg_change = diff.avg_damage_to - diff.avg_damage_from;
        let avg_frags_change = diff.avg_frags_to - diff.avg_frags_from;
        let hit_rate_change = diff.hit_rate_to - diff.hit_rate_from;

        println!("  {:<20} {:>12.1}% {:>12.1}% {:>+9.1}%", "Win rate", diff.win_rate_from, diff.win_rate_to, wr_change);
        println!("  {:<20} {:>12.0} {:>12.0} {:>+9.0}", "Avg damage", diff.avg_damage_from, diff.avg_damage_to, avg_dmg_change);
        println!("  {:<20} {:>12.2} {:>12.2} {:>+9.2}", "Avg frags", diff.avg_frags_from, diff.avg_frags_to, avg_frags_change);
        println!("  {:<20} {:>12.1}% {:>12.1}% {:>+9.1}%", "Hit rate", diff.hit_rate_from, diff.hit_rate_to, hit_rate_change);
        println!("  {:<20} {:>12} {:>12} {:>+9}", "Total damage", diff.from_battles, diff.to_battles, diff.damage_diff);

        if let Some(delta) = diff.rating_delta {
            println!();
            println!("  mm_rating:         {:.2} -> {:.2} ({:+.2})", 
                diff.mm_rating_from.unwrap_or(0.0),
                diff.mm_rating_to.unwrap_or(0.0),
                delta);
            let dr_from = 3000.0 + diff.mm_rating_from.unwrap_or(0.0) * 10.0;
            let dr_to = 3000.0 + diff.mm_rating_to.unwrap_or(0.0) * 10.0;
            println!("  Display rating:    {:.0} -> {:.0} ({:+.0})", dr_from, dr_to, dr_to - dr_from);
        }

        println!();
        println!("========================================================");
    }
}
