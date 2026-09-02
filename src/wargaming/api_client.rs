use anyhow::Result;
use serde::{Deserialize, Serialize};

// =====================================================================
//  Wargaming 公共 API 客户端（仅用于战绩查询）
//  提供"按昵称搜玩家"和"查询玩家累计战绩"两个核心能力。
//  注意：坦克百科数据已全部来自 BlitzKit，不再走 WG 百科。
// =====================================================================

/// 一名玩家的累计战绩（随机战 + 排位战两套）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStats {
    pub nickname: String,
    pub account_id: u32,

    /// —— 随机战 ——
    pub random_battles: u32,
    pub random_wins: u32,
    pub random_losses: u32,
    pub random_damage_dealt: u64,
    pub random_frags: u64,
    pub random_shots: u64,
    pub random_hits: u64,
    pub random_xp: u64,
    pub random_spotted: u64,

    /// —— 排位战 ——
    pub rating_battles: u32,
    pub rating_wins: u32,
    pub rating_losses: u32,
    pub rating_damage_dealt: u64,
    pub rating_frags: u64,
    pub rating_shots: u64,
    pub rating_hits: u64,
    pub rating_xp: u64,
    pub rating_spotted: u64,
    /// 排位 mm 评级
    pub rating_mm_rating: Option<f32>,
    /// 排位显示评级（由 mm 换算：3000 + mm*10）
    pub rating_display_rating: Option<u32>,
    /// 当前赛季
    pub rating_season: Option<u32>,
}

/// WG API 客户端（保存 Application ID 与服务器分区对应的域名）。
pub struct WgApiClient {
    application_id: String,
    base_url: String,
}

impl WgApiClient {
    /// 构造客户端，按服务器分区选域名。
    pub fn new(application_id: &str, server: &str) -> Self {
        let base_url = match server {
            "asia" => "https://api.wotblitz.asia",
            "eu" => "https://api.wotblitz.eu",
            "na" | "com" => "https://api.wotblitz.com",
            _ => "https://api.wotblitz.asia",
        };
        Self {
            application_id: application_id.to_string(),
            base_url: base_url.to_string(),
        }
    }

    /// 按昵称搜索玩家，返回 `(昵称, 账号ID)` 列表。
    ///
    /// `exact=true` 用精确匹配，否则用前缀匹配。
    pub fn search_player(&self, nickname: &str, exact: bool) -> Result<Vec<(String, u32)>> {
        let search_type = if exact { "exact" } else { "startswith" };
        let url = format!(
            "{}/wotb/account/list/?application_id={}&search={}&type={}&limit=10",
            self.base_url, self.application_id, nickname, search_type
        );
        let resp: serde_json::Value = reqwest::blocking::get(&url)?.json()?;
        if resp["status"] != "ok" {
            anyhow::bail!("API error: {}", resp["error"]["message"]);
        }
        let mut results = Vec::new();
        if let Some(data) = resp["data"].as_array() {
            for item in data {
                let nick = item["nickname"].as_str().unwrap_or("?").to_string();
                let id = item["account_id"].as_u64().unwrap_or(0) as u32;
                results.push((nick, id));
            }
        }
        Ok(results)
    }

    /// 查询玩家的累计战绩（随机 + 排位 + 评级信息）。
    pub fn get_player_stats(&self, account_id: u32) -> Result<PlayerStats> {
        let url = format!(
            "{}/wotb/account/info/?application_id={}&account_id={}&extra=statistics.rating",
            self.base_url, self.application_id, account_id
        );
        let resp: serde_json::Value = reqwest::blocking::get(&url)?.json()?;
        if resp["status"] != "ok" {
            anyhow::bail!("API error: {}", resp["error"]["message"]);
        }

        let data = &resp["data"][&account_id.to_string()];
        let stats = &data["statistics"];
        let all = &stats["all"];        // 随机战统计
        let rating = &stats["rating"];  // 排位战统计

        // 显示评级 = 3000 + mm*10（WG 的换算公式）
        let mm_rating = rating["mm_rating"].as_f64().map(|v| v as f32);
        let display_rating = mm_rating.map(|v| (3000.0 + v * 10.0) as u32);

        Ok(PlayerStats {
            nickname: data["nickname"].as_str().unwrap_or("?").to_string(),
            account_id,
            random_battles: all["battles"].as_u64().unwrap_or(0) as u32,
            random_wins: all["wins"].as_u64().unwrap_or(0) as u32,
            random_losses: all["losses"].as_u64().unwrap_or(0) as u32,
            random_damage_dealt: all["damage_dealt"].as_u64().unwrap_or(0),
            random_frags: all["frags"].as_u64().unwrap_or(0),
            random_shots: all["shots"].as_u64().unwrap_or(0),
            random_hits: all["hits"].as_u64().unwrap_or(0),
            random_xp: all["xp"].as_u64().unwrap_or(0),
            random_spotted: all["spotted"].as_u64().unwrap_or(0),

            rating_battles: rating["battles"].as_u64().unwrap_or(0) as u32,
            rating_wins: rating["wins"].as_u64().unwrap_or(0) as u32,
            rating_losses: rating["losses"].as_u64().unwrap_or(0) as u32,
            rating_damage_dealt: rating["damage_dealt"].as_u64().unwrap_or(0),
            rating_frags: rating["frags"].as_u64().unwrap_or(0),
            rating_shots: rating["shots"].as_u64().unwrap_or(0),
            rating_hits: rating["hits"].as_u64().unwrap_or(0),
            rating_xp: rating["xp"].as_u64().unwrap_or(0),
            rating_spotted: rating["spotted"].as_u64().unwrap_or(0),
            rating_mm_rating: mm_rating,
            rating_display_rating: display_rating,
            rating_season: rating["current_season"].as_u64().map(|v| v as u32),
        })
    }

    /// 打印玩家战绩（`player` 命令用）。
    pub fn print_stats(stats: &PlayerStats) {
        println!();
        println!("========================================================");
        println!("  Player: {} (id={})", stats.nickname, stats.account_id);
        println!("========================================================");
        println!();
        println!("--- Random Battles ---");
        Self::print_section(stats.random_battles, stats.random_wins, stats.random_losses,
            stats.random_damage_dealt, stats.random_frags, stats.random_shots,
            stats.random_hits, stats.random_xp, stats.random_spotted);
        println!();
        println!("--- Rating Battles ---");
        Self::print_section(stats.rating_battles, stats.rating_wins, stats.rating_losses,
            stats.rating_damage_dealt, stats.rating_frags, stats.rating_shots,
            stats.rating_hits, stats.rating_xp, stats.rating_spotted);
        if let Some(mm) = stats.rating_mm_rating {
            println!();
            println!("  mm_rating:     {:.2}", mm);
            if let Some(dr) = stats.rating_display_rating {
                println!("  Display rating: {}", dr);
            }
        }
        if let Some(s) = stats.rating_season {
            println!("  Season:        {}", s);
        }
        println!();
        println!("========================================================");
    }

    /// 打印单个模式（随机/排位）的战绩小节。
    fn print_section(battles: u32, wins: u32, losses: u32, dmg: u64, frags: u64,
        shots: u64, hits: u64, xp: u64, spotted: u64)
    {
        let n = battles.max(1) as f64;
        println!("  Battles:       {}", battles);
        println!("  Win/Loss:       {}/{}  ({:.1}%)", wins, losses, wins as f64 / n * 100.0);
        println!("  Total damage:   {} (avg {:.0})", dmg, dmg as f64 / n);
        println!("  Total frags:    {} (avg {:.2})", frags, frags as f64 / n);
        println!("  Shots/Hits:     {}/{} ({:.1}%)", shots, hits, if shots > 0 { hits as f64 / shots as f64 * 100.0 } else { 0.0 });
        println!("  Total XP:       {} (avg {:.0})", xp, xp as f64 / n);
        println!("  Spotted:        {} (avg {:.2})", spotted, spotted as f64 / n);
    }
}
