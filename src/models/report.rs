use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::models::battle::BattleSummary;


/// 多场回放聚合后的战绩报告（`scan` 命令、Agent 的 `scan_replays` 工具、Web 的 `/api/scan` 共用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedReport {
    pub total_battles: usize,
    /// 时间范围（如 `2026-07-30 ~ 2026-08-28`）
    pub date_range: String,
    /// 对局模式（Rating / Regular）
    pub room_type: String,
    pub author_name: String,

    pub wins: usize,
    pub losses: usize,
    /// 胜率（百分比）
    pub win_rate: f64,

    pub avg_damage: f64,
    pub avg_frags: f64,
    pub total_shots: u64,
    pub total_hits: u64,
    /// 命中率（百分比）
    pub hit_rate: f64,
    pub total_penetrations: u64,
    /// 穿透/开炮比例（百分比）
    pub penetration_rate: f64,
    pub avg_damage_blocked: f64,
    pub avg_assisted: f64,
    pub avg_xp: f64,
    /// 平均战斗时长（秒）
    pub avg_battle_duration: f64,
    /// 自动击毁（溺水/坠桥等）的场数
    pub auto_destroyed_count: usize,

    /// 每场战斗的排位评级变化记录
    pub rating_changes: Vec<RatingChange>,
    pub rating_start: Option<f32>,
    pub rating_end: Option<f32>,
    pub rating_delta: Option<f32>,

    /// 每辆坦克的使用统计（按场数排序）
    pub tank_usage: Vec<TankUsage>,
    /// 每张地图的统计（按场数排序）
    pub map_stats: Vec<MapStat>,
    /// 全部逐场战斗明细（用于展开/导出）
    pub battle_summaries: Vec<BattleSummary>,
}

/// 单场排位评级的增减记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingChange {
    pub datetime: String,
    pub timestamp: i64,
    /// 该场结束后的 mm 评级
    pub mm_rating: Option<f32>,
    /// 该场结束后的显示评级
    pub display_rating: Option<u32>,
    pub won: bool,
    pub tank_name: String,
    pub damage_dealt: u32,
}

/// 单辆坦克的聚合使用统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TankUsage {
    pub tank_id: u32,
    pub tank_name: String,
    pub battles: usize,
    pub wins: usize,
    /// 该坦克的胜率（百分比）
    pub win_rate: f64,
    pub total_damage: u64,
    pub avg_damage: f64,
    pub total_frags: u64,
    pub avg_frags: f64,
}

/// 单张地图的聚合统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapStat {
    pub map_id: u32,
    pub map_name: String,
    pub battles: usize,
    pub wins: usize,
    /// 该地图的胜率（百分比）
    pub win_rate: f64,
    /// 该地图的场均伤害
    pub avg_damage: f64,
}

impl AggregatedReport {
    /// 从一组已按模式/日期过滤的战斗聚合出战绩报告；`room_type` 会写入报告。
    /// 接收 `Vec` 并在报告中 move 复用为逐场明细，避免整体深拷贝。
    pub fn from_battles(battles: Vec<BattleSummary>, room_type: &str) -> Self {
        let total = battles.len();
        let wins = battles.iter().filter(|b| b.author_won).count();
        let losses = total - wins;

        // 逐项聚合作者（当前玩家）在全部场次里的累计值
        let total_damage: u64 = battles.iter().map(|b| b.author.damage_dealt as u64).sum();
        let total_shots: u64 = battles.iter().map(|b| b.author.n_shots as u64).sum();
        let total_hits: u64 = battles.iter().map(|b| b.author.n_hits as u64).sum();
        let total_pen: u64 = battles.iter().map(|b| b.author.n_penetrations as u64).sum();
        let total_xp: u64 = battles.iter().map(|b| b.author.total_xp as u64).sum();
        let total_duration: f64 = battles.iter().map(|b| b.battle_duration_secs).sum();
        let auto_destroyed = battles.iter().filter(|b| b.author.is_auto_destroyed).count();

        // 击毁/格挡/助攻与评级不在 author 里，需按作者账号 ID 从该场玩家列表找到
        // 作者记录；每场只查找一次，同一遍循环里完成累计、评级记录与坦克/地图分组
        let mut total_frags: u64 = 0;
        let mut total_block: u64 = 0;
        let mut total_assist: u64 = 0;
        let mut rating_changes: Vec<RatingChange> = Vec::new();
        let mut tank_map: HashMap<u32, TankUsage> = HashMap::new();
        let mut map_map: HashMap<u32, MapStat> = HashMap::new();
        let mut map_damage: HashMap<u32, f64> = HashMap::new();
        for b in &battles {
            let author_rec = b.players.iter().find(|p| p.account_id == b.author_account_id);
            if let Some(p) = author_rec {
                total_frags += p.n_enemies_destroyed as u64;
                total_block += p.damage_blocked as u64;
                total_assist += (p.damage_assisted_1 + p.damage_assisted_2) as u64;
                // —— 排位评级 —— 逐场抽取 mm_rating 变化，首场为起始、末场为结束
                rating_changes.push(RatingChange {
                    datetime: b.datetime.clone(),
                    timestamp: b.timestamp,
                    mm_rating: p.mm_rating,
                    display_rating: p.display_rating,
                    won: b.author_won,
                    tank_name: b.author_tank_name.clone(),
                    damage_dealt: b.author.damage_dealt,
                });
            }

            let entry = tank_map.entry(b.author_tank_id).or_insert_with(|| {
                TankUsage {
                    tank_id: b.author_tank_id,
                    tank_name: b.author_tank_name.clone(),
                    battles: 0,
                    wins: 0,
                    win_rate: 0.0,
                    total_damage: 0,
                    avg_damage: 0.0,
                    total_frags: 0,
                    avg_frags: 0.0,
                }
            });
            entry.battles += 1;
            if b.author_won {
                entry.wins += 1;
            }
            entry.total_damage += b.author.damage_dealt as u64;
            if let Some(p) = author_rec {
                entry.total_frags += p.n_enemies_destroyed as u64;
            }

            let mentry = map_map.entry(b.map_id).or_insert_with(|| {
                MapStat {
                    map_id: b.map_id,
                    map_name: b.map_name.clone(),
                    battles: 0,
                    wins: 0,
                    win_rate: 0.0,
                    avg_damage: 0.0,
                }
            });
            mentry.battles += 1;
            if b.author_won {
                mentry.wins += 1;
            }
            *map_damage.entry(b.map_id).or_insert(0.0) += b.author.damage_dealt as f64;
        }

        // 用 a.max(1) 避免分母为 0（无场次时各 "场均" 皆 0）
        let n = total.max(1) as f64;

        let rating_start = rating_changes.first().and_then(|r| r.mm_rating);
        let rating_end = rating_changes.last().and_then(|r| r.mm_rating);
        let rating_delta = match (rating_start, rating_end) {
            (Some(s), Some(e)) => Some(e - s),
            _ => None,
        };

        let mut tank_usage: Vec<TankUsage> = tank_map.into_values().collect();
        for t in &mut tank_usage {
            let n = t.battles as f64;
            t.win_rate = t.wins as f64 / n * 100.0;
            t.avg_damage = t.total_damage as f64 / n;
            t.avg_frags = t.total_frags as f64 / n;
        }
        tank_usage.sort_by(|a, b| b.battles.cmp(&a.battles));

        let mut map_stats: Vec<MapStat> = map_map.into_values().collect();
        for m in &mut map_stats {
            m.win_rate = m.wins as f64 / m.battles as f64 * 100.0;
            m.avg_damage = map_damage.remove(&m.map_id).unwrap_or(0.0) / m.battles as f64;
        }
        map_stats.sort_by(|a, b| b.battles.cmp(&a.battles));

        let date_range = if total > 0 {
            let min_ts = battles.iter().map(|b| b.timestamp).min().unwrap_or(0);
            let max_ts = battles.iter().map(|b| b.timestamp).max().unwrap_or(0);
            let min_dt = chrono::DateTime::from_timestamp(min_ts, 0)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| min_ts.to_string());
            let max_dt = chrono::DateTime::from_timestamp(max_ts, 0)
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| max_ts.to_string());
            format!("{} ~ {}", min_dt, max_dt)
        } else {
            "N/A".to_string()
        };

        let author_name = battles
            .first()
            .map(|b| b.author_nickname.clone())
            .unwrap_or_default();

        // —— 组装最终报告 —— 各项占比都用 "累计值 / 场数"
        AggregatedReport {
            total_battles: total,
            date_range,
            room_type: room_type.to_string(),
            author_name,
            wins,
            losses,
            win_rate: wins as f64 / n * 100.0,
            avg_damage: total_damage as f64 / n,
            avg_frags: total_frags as f64 / n,
            total_shots,
            total_hits,
            hit_rate: if total_shots > 0 {
                total_hits as f64 / total_shots as f64 * 100.0
            } else {
                0.0
            },
            total_penetrations: total_pen,
            penetration_rate: if total_shots > 0 {
                total_pen as f64 / total_shots as f64 * 100.0
            } else {
                0.0
            },
            avg_damage_blocked: total_block as f64 / n,
            avg_assisted: total_assist as f64 / n,
            avg_xp: total_xp as f64 / n,
            avg_battle_duration: total_duration / n,
            auto_destroyed_count: auto_destroyed,
            rating_changes,
            rating_start,
            rating_end,
            rating_delta,
            tank_usage,
            map_stats,
            battle_summaries: battles,
        }
    }

    /// 把聚合报告以人类可读的表格形式打印到 stdout（`scan` 命令的文本输出）。
    pub fn print_summary(&self) {
        println!();
        println!("========================================================");
        println!("  WoTB Replay Analysis Report");
        println!("========================================================");
        println!();
        println!("  Player:     {}", self.author_name);
        println!("  Mode:       {}", self.room_type);
        println!("  Date range: {}", self.date_range);
        println!("  Battles:    {}", self.total_battles);
        println!();

        println!("--- Overall ---");
        println!("  Wins / Losses:   {} / {}", self.wins, self.losses);
        println!("  Win rate:        {:.1}%", self.win_rate);
        println!("  Avg damage:      {:.0}", self.avg_damage);
        println!("  Avg frags:       {:.2}", self.avg_frags);
        println!("  Hit rate:        {:.1}%  ({}/{})", self.hit_rate, self.total_hits, self.total_shots);
        println!("  Pen rate:        {:.1}%  ({})", self.penetration_rate, self.total_penetrations);
        println!("  Avg block:       {:.0}", self.avg_damage_blocked);
        println!("  Avg assisted:    {:.0}", self.avg_assisted);
        println!("  Avg XP:          {:.0}", self.avg_xp);
        println!("  Avg duration:    {:.0}s", self.avg_battle_duration);
        println!("  Auto-destroyed:  {}", self.auto_destroyed_count);
        println!();

        // —— 排位评级 —— mm_rating → 显示评级换算：3000 + mm*10
        if let Some(delta) = self.rating_delta {
            println!("--- Rating ---");
            println!("  Start mm_rating: {:.2}", self.rating_start.unwrap_or(0.0));
            println!("  End mm_rating:   {:.2}", self.rating_end.unwrap_or(0.0));
            println!("  Delta:            {:+.2}", delta);
            let display_start = self.rating_start.map(|r| (3000.0 + r * 10.0) as u32);
            let display_end = self.rating_end.map(|r| (3000.0 + r * 10.0) as u32);
            if let (Some(s), Some(e)) = (display_start, display_end) {
                println!("  Display rating:   {} -> {} ({:+})", s, e, e as i64 - s as i64);
            }
            println!();
        }

        if !self.tank_usage.is_empty() {
            println!("--- Tank Usage (top {}) ---", self.tank_usage.len().min(10));
            println!("  {:<30} {:>4} {:>5} {:>6} {:>8} {:>8} {:>6}",
                "Tank", "Bat", "WR%", "Frags", "TotalDmg", "AvgDmg", "AvgFr");
            println!("  {}", "-".repeat(75));
            for t in self.tank_usage.iter().take(10) {
                println!("  {:<30} {:>4} {:>4.0}% {:>6} {:>8} {:>8.0} {:>6.2}",
                    t.tank_name, t.battles, t.win_rate, t.total_frags, t.total_damage, t.avg_damage, t.avg_frags);
            }
            println!();
        }

        // —— 地图统计 —— 场均伤害已在聚合阶段算好，直接读取
        if !self.map_stats.is_empty() {
            println!("--- Map Stats ---");
            println!("  {:<25} {:>4} {:>5} {:>6}", "Map", "Bat", "WR%", "AvgDmg");
            println!("  {}", "-".repeat(50));
            for m in self.map_stats.iter() {
                println!("  {:<25} {:>4} {:>4.0}% {:>8.0}", m.map_name, m.battles, m.win_rate, m.avg_damage);
            }
            println!();
        }

        if !self.rating_changes.is_empty() {
            println!("--- Rating Changes (last 10) ---");
            println!("  {:<20} {:>8} {:>6} {:>25} {:>8}",
                "DateTime", "Display", "W/L", "Tank", "Damage");
            println!("  {}", "-".repeat(75));
            for rc in self.rating_changes.iter().rev().take(10) {
                let dr = rc.display_rating.map(|d| d.to_string()).unwrap_or("-".into());
                let wl = if rc.won { "W" } else { "L" };
                println!("  {:<20} {:>8} {:>6} {:>25} {:>8}",
                    rc.datetime, dr, wl, rc.tank_name, rc.damage_dealt);
            }
            println!();
        }

        println!("========================================================");
    }
}
