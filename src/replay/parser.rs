use std::fs::File;
use std::path::Path;
use anyhow::{Result, Context};
use wotbreplay_parser::replay::Replay;
use wotbreplay_parser::models::battle_results::TeamNumber;

use crate::models::battle::{BattleSummary, AuthorStats, PlayerSummary};
use crate::wargaming::tank_resolver::TankResolver;

// =====================================================================
//  回放解析（单场战斗）
//  通过第三方 crate `wotbreplay-parser` 读取 `.wotbreplay` ZIP 包，
//  解析其 meta（元信息）与 battle_results（战斗结果），
//  再提取成项目内部的 BattleSummary（含 14 名玩家战绩）。
// =====================================================================

/// 单场回放解析器。
///
/// 持有可选的 `TankResolver`（用于把 tank_id 解析成坦克名）。
/// 若解析器存在则坦克名可读；否则退化为 `tank_{id}`。
pub struct ReplayParser<'a> {
    tank_resolver: Option<&'a TankResolver>,
}

impl<'a> ReplayParser<'a> {
    /// 构造不带解析器的解析器（坦克名无法翻译，只能显示 id）。
    pub fn new() -> Self {
        Self { tank_resolver: None }
    }

    /// 构造带 TankResolver 的解析器（可把 tank_id 翻译成坦克名）。
    pub fn with_resolver(resolver: &'a TankResolver) -> Self {
        Self { tank_resolver: Some(resolver) }
    }

    /// 解析单个回放文件，返回该场战斗的汇总结构。
    pub fn parse_file(&self, path: &Path) -> Result<BattleSummary> {
        // 回放文件名（如 `20260730_1916__Anonyme_E-100_xxx.wotbreplay`）
        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        // 打开回放 ZIP 包
        let mut replay = Replay::open(File::open(path)?)
            .with_context(|| format!("Failed to open replay: {}", path.display()))?;

        // meta 可能缺失（.ok() 吞掉错误），battle_results 则必须存在
        let meta = replay.read_meta().ok();
        let br = replay.read_battle_results()
            .with_context(|| format!("Failed to parse battle_results: {}", path.display()))?;

        // 对局模式：用 Debug 格式化枚举（如 Rating / Regular）
        let room_type = format!("{:?}", br.room_type());
        let winner_team = match br.winner_team_number() {
            TeamNumber::One => 1u8,
            TeamNumber::Two => 2u8,
        };

        // 地图 ID 取低 16 位（高 16 位是模式标记）
        let map_id = br.mode_map_id & 0xFFFF;
        let map_name = meta.as_ref()
            .map(|m| format!("{:?}", m.map_id))
            .unwrap_or_else(|| format!("map_{}", map_id));

        let battle_duration = meta.as_ref()
            .map(|m| m.battle_duration_secs)
            .unwrap_or(0.0);

        // 作者（当前玩家）的信息
        let author = &br.author;
        let author_account_id = author.account_id;
        let author_team = if author.team_number == 1 { 1u8 } else { 2u8 };
        let author_won = author_team == winner_team;

        // 作者坦克 ID：优先取 meta，缺失时从玩家结果里找
        let mut author_tank_id = meta.as_ref().map(|m| m.tank_id as u32).unwrap_or(0);
        if author_tank_id == 0 {
            if let Some(pr) = br.player_results.iter().find(|pr| pr.info.account_id == author_account_id) {
                author_tank_id = pr.info.tank_id;
            }
        }

        let author_tank_name = self.resolve_tank_name(author_tank_id);

        // hitpoints_left == -2 表示"自动击毁"（溺水、坠桥、友军击杀等）
        let is_auto_destroyed = author.hitpoints_left == -2;

        let author_stats = AuthorStats {
            hitpoints_left: author.hitpoints_left,
            total_credits: author.total_credits,
            total_xp: author.total_xp,
            n_shots: author.n_shots,
            n_hits: author.n_hits,
            n_splashes: author.n_splashes,
            n_penetrations: author.n_penetrations,
            damage_dealt: author.damage_dealt,
            is_auto_destroyed,
        };

        // 作者昵称：meta 有则用，否则从玩家列表里按账号 ID 查
        let author_nickname = meta.as_ref()
            .map(|m| m.player_name.clone())
            .unwrap_or_else(|| {
                br.players.iter()
                    .find(|p| p.account_id == author_account_id)
                    .map(|p| p.info.nickname.clone())
                    .unwrap_or_default()
            });

        // —— 遍历全部 14 名玩家 —— team/platoon/clan/nickname 都要去 players 里按账号 ID 关联
        let mut players = Vec::with_capacity(br.player_results.len());
        for pr in &br.player_results {
            let info = &pr.info;
            let tank_name = self.resolve_tank_name(info.tank_id);

            let player_team = br.players.iter()
                .find(|p| p.account_id == info.account_id)
                .map(|p| if p.info.team == 1 { 1u8 } else { 2u8 })
                .unwrap_or(0);

            let platoon_id = br.players.iter()
                .find(|p| p.account_id == info.account_id)
                .and_then(|p| p.info.platoon_id);

            let clan_tag = br.players.iter()
                .find(|p| p.account_id == info.account_id)
                .and_then(|p| p.info.clan_tag.clone());

            let nickname = br.players.iter()
                .find(|p| p.account_id == info.account_id)
                .map(|p| p.info.nickname.clone())
                .unwrap_or_default();

            players.push(PlayerSummary {
                account_id: info.account_id,
                nickname,
                team: player_team,
                platoon_id,
                clan_tag,
                tank_id: info.tank_id,
                tank_name,
                base_xp: info.base_xp,
                credits_earned: info.credits_earned,
                n_shots: info.n_shots,
                n_hits_dealt: info.n_hits_dealt,
                n_penetrations_dealt: info.n_penetrations_dealt,
                damage_dealt: info.damage_dealt,
                damage_blocked: info.damage_blocked,
                damage_assisted_1: info.damage_assisted_1,
                damage_assisted_2: info.damage_assisted_2,
                n_hits_received: info.n_hits_received,
                n_penetrations_received: info.n_penetrations_received,
                n_enemies_damaged: info.n_enemies_damaged,
                n_enemies_destroyed: info.n_enemies_destroyed,
                mm_rating: info.mm_rating,
                display_rating: info.display_rating(),
            });
        }

        // —— 组装最终 BattleSummary ——
        let mut summary = BattleSummary::from_naive(br.timestamp_secs);
        summary.file_name = file_name;
        summary.room_type = room_type;
        summary.map_id = map_id;
        summary.map_name = map_name;
        summary.battle_duration_secs = battle_duration;
        summary.winner_team = winner_team;
        summary.author_account_id = author_account_id;
        summary.author_nickname = author_nickname;
        summary.author_tank_id = author_tank_id;
        summary.author_tank_name = author_tank_name;
        summary.author_team = author_team;
        summary.author_won = author_won;
        summary.author = author_stats;
        summary.players = players;

        Ok(summary)
    }

    /// 把坦克 ID 翻译成名称（有 resolver 时），否则返回 `tank_{id}`。
    fn resolve_tank_name(&self, tank_id: u32) -> String {
        if let Some(resolver) = self.tank_resolver {
            if let Some(name) = resolver.resolve(tank_id) {
                return name;
            }
        }
        format!("tank_{}", tank_id)
    }
}

impl<'a> Default for ReplayParser<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// 列出目录下所有后缀为 `.wotbreplay` 的回放文件（按文件名排序）。
pub fn list_replays_in_dir(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("wotbreplay") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

