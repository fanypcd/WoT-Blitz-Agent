use anyhow::{Context, Result};
use std::fs::File;
use std::path::Path;
use wotbreplay_parser::replay::Replay;

use super::TankNames;
use crate::models::battle::{AuthorStats, BattleSummary, PlayerSummary};

// 回放解析（单场）：用 `wotbreplay-parser` 读取 `.wotbreplay` ZIP 包的
// meta 与 battle_results，提取成项目内部的 BattleSummary（含 14 名玩家战绩）。

/// 单场回放解析器。持有可选的 [`TankNames`]：有则可把 tank_id 解析成坦克名，否则退化为 `tank_{id}`。
pub struct ReplayParser<'a> {
    tank_resolver: Option<&'a dyn TankNames>,
}

/// 从回放**原始字节**读取 meta.json 原始 JSON。
///
/// 回放容器是 ZIP（`pk` 魔数），meta.json 为其中一条；上游 crate 的 `Meta` 只反序列化
/// 部分字段（playerName/arenaUniqueId/battleDuration/tank_id/mapId），且 crate 不暴露原始
/// 条目访问——故此处自行解 ZIP。meta.json 常含非 UTF-8 字节（昵称等，见 combat::shots），
/// 严格 UTF-8 会让整份 meta 失败，故按 lossy 解码：非法字节只影响所在字符串，不连坐其它字段。
fn read_meta_json(raw: &[u8]) -> Option<serde_json::Value> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(raw)).ok()?;
    let mut f = zip.by_name("meta.json").ok()?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut buf).ok()?;
    serde_json::from_str(&String::from_utf8_lossy(&buf)).ok()
}

fn meta_arena_bonus_type(meta: &serde_json::Value) -> Option<u32> {
    meta.get("arenaBonusType")
        .and_then(|x| x.as_u64())
        .map(|x| x as u32)
}

fn meta_player_vehicle_name(meta: &serde_json::Value) -> Option<String> {
    meta.get("playerVehicleName")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 结算阵容完整性：花名册与战绩的账号集合完全一致；任一侧为空 → false。
pub fn roster_complete(roster_accounts: &[u32], result_accounts: &[u32]) -> bool {
    if roster_accounts.is_empty() || result_accounts.is_empty() {
        return false;
    }
    let a: std::collections::BTreeSet<u32> = roster_accounts.iter().copied().collect();
    let b: std::collections::BTreeSet<u32> = result_accounts.iter().copied().collect();
    a == b
}

fn meta_map_key(meta: &serde_json::Value) -> Option<String> {
    meta.get("mapName")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 从回放**原始字节**读取 meta.json 的 `arenaBonusType`（名人堂白名单 {1,7}、
/// 联赛模式 {2,4} 的判定依据；crate `Meta` 不含该字段）。非 ZIP/无该字段一律 None（unknown ≠ 0）。
pub fn read_arena_bonus_type(raw: &[u8]) -> Option<u32> {
    read_meta_json(raw).as_ref().and_then(meta_arena_bonus_type)
}

/// 从回放**原始字节**读取 meta.json 的原始 `mapName`（地图代号，如 `skit`）。
/// crate 的 `Meta` 把地图反序列化为枚举（未知地图丢失原名），底图/语义/i18n 需要原始代号。
/// 缺省/空串一律 None。
pub fn read_map_key(raw: &[u8]) -> Option<String> {
    read_meta_json(raw).as_ref().and_then(meta_map_key)
}

/// 把只能从容器原始字节取得的字段（arenaBonusType / 地图代号 / 客户端版本）写入汇总。
/// meta.json 只解压一次。文件路径与内存字节（WASM）两个宿主共用，新增此类字段只改这里。
pub fn apply_container_fields(summary: &mut BattleSummary, raw: &[u8]) {
    let meta = read_meta_json(raw);
    summary.arena_bonus_type = meta.as_ref().and_then(meta_arena_bonus_type);
    summary.map_key = meta.as_ref().and_then(meta_map_key);
    summary.author_vehicle_codename = meta.as_ref().and_then(meta_player_vehicle_name);
    summary.client_version = read_client_version(raw);
}

/// `killer_id` 是 result/entity ID：经同场 `result_id → account_id` 联表成击杀者账号。
/// 联不上（结算缺 result_id、击杀者非战斗者）保持 None。
fn resolve_killer_accounts(players: &mut [PlayerSummary]) {
    let account_by_result: std::collections::HashMap<u32, u32> = players
        .iter()
        .filter_map(|p| p.result_id.map(|r| (r, p.account_id)))
        .collect();
    for p in players.iter_mut() {
        p.killer_account_id = p.killer_id.and_then(|k| account_by_result.get(&k).copied());
    }
}

/// 从回放**原始字节**读取客户端版本串（`data.wotreplay` 头部）。
///
/// 布局（crate `Data::from_reader` 同源，实测 `11.20.0`）：
/// `magic u32(0x12345678) + u64 + [len u8 + client hash] + [len u8 + version] + u8 + packets`。
/// 只读头部前若干字节，**不解析整包**（data.wotreplay 可达数 MB，为版本解析全包不划算）。
/// 版本门禁（语义只在 11.19/11.20 验证）依赖此字段。
pub fn read_client_version(raw: &[u8]) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(raw)).ok()?;
    let mut f = zip.by_name("data.wotreplay").ok()?;
    let mut buf = [0u8; 128];
    let n = std::io::Read::read(&mut f, &mut buf).ok()?;
    let b = &buf[..n];
    if b.len() < 12 || u32::from_le_bytes([b[0], b[1], b[2], b[3]]) != 0x1234_5678 {
        return None;
    }
    let mut o = 12usize; // magic(4) + u64(8)
    let hlen = *b.get(o)? as usize; // 长度前缀 hash
    o += 1 + hlen;
    let vlen = *b.get(o)? as usize; // 长度前缀版本串
    o += 1;
    let v = b.get(o..o + vlen)?;
    let text = std::str::from_utf8(v).ok()?;
    // 版本串应可打印；否则视为布局不符（不猜）
    text.chars()
        .all(|c| c.is_ascii_graphic() || c == '.' || c == '_' || c == '-')
        .then(|| text.to_string())
}

impl<'a> ReplayParser<'a> {
    /// 构造不带解析器的解析器（坦克名无法翻译，只能显示 id）。
    pub fn new() -> Self {
        Self {
            tank_resolver: None,
        }
    }

    pub fn with_resolver<R: TankNames>(resolver: &'a R) -> Self {
        Self {
            tank_resolver: Some(resolver as &'a dyn TankNames),
        }
    }

    /// 解析单个回放文件，返回该场战斗的汇总结构。
    pub fn parse_file(&self, path: &Path) -> Result<BattleSummary> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        // 容器字段需从原始字节取（见 apply_container_fields），在打开 Replay（消费 reader）之前读
        let raw_bytes = std::fs::read(path).ok();

        let mut replay = Replay::open(File::open(path)?)
            .with_context(|| format!("Failed to open replay: {}", path.display()))?;
        let mut summary = self
            .parse_replay(&mut replay, &file_name)
            .with_context(|| format!("Failed to parse replay: {}", path.display()))?;
        if let Some(raw) = raw_bytes.as_deref() {
            apply_container_fields(&mut summary, raw);
        }
        Ok(summary)
    }

    /// 从已打开的回放对象构建结算汇总（与 [`Self::parse_file`] 同义；供 WASM/内存宿主
    /// 复用——浏览器路径只有字节没有文件路径）。
    pub fn parse_replay<R: std::io::Read + std::io::Seek>(
        &self,
        replay: &mut Replay<R>,
        file_name: &str,
    ) -> Result<BattleSummary> {
        // meta 可能缺失（.ok() 吞掉错误），battle_results 则必须存在
        let meta = replay.read_meta().ok();
        let br = replay
            .read_battle_results()
            .context("Failed to parse battle_results")?;
        // 结算补充字段（crate 未暴露的 #301 字段：死亡原因/寿命/点亮/毁灭协助/炮印/击杀者）
        // battle_results.dat（pickle 外层）：arenaUniqueId = 名人堂查重/去重键；
        // buffer = #301 补充字段。两者共用同一次读取。
        let br_dat = replay.read_battle_results_dat().ok();
        let arena_id = br_dat.as_ref().map(|d| d.arena_unique_id.to_string());
        // 结算根字段（finishReason / 整秒时长）与 #201 段位：同一 buffer 再走两遍，
        // 结构小、开销可忽略
        let root_fields = br_dat
            .as_ref()
            .map(|d| crate::wargaming::battle_results_extra::parse_root_fields(&d.buffer))
            .unwrap_or_default();
        let ranks = br_dat
            .as_ref()
            .map(|d| crate::wargaming::battle_results_extra::parse_rank_entries(&d.buffer))
            .unwrap_or_default();
        let settlements: std::collections::HashMap<
            u32,
            crate::wargaming::battle_results_extra::PlayerSettlement,
        > = br_dat
            .as_ref()
            .map(|dat| crate::wargaming::battle_results_extra::parse_settlement_extras(&dat.buffer))
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.account_id, s))
            .collect();

        let room_type = format!("{:?}", br.room_type());
        // 胜方取结算原始字段：crate 的 winner_team_number() 把「无胜方」（平局/未结算）映射成
        // TeamNumber::One，会把平局伪装成 1 队胜。缺省/非 1·2 一律 0 = 无胜方（与 playback_viewer 同口径）。
        let winner_team = br
            .winner_team_number
            .as_ref()
            .map(|w| {
                if *w == 1 {
                    1u8
                } else if *w == 2 {
                    2u8
                } else {
                    0u8
                }
            })
            .unwrap_or(0);

        // 地图 ID 取低 16 位（高 16 位是模式标记）
        let map_id = br.mode_map_id & 0xFFFF;
        let map_name = meta
            .as_ref()
            .map(|m| format!("{:?}", m.map_id))
            .unwrap_or_else(|| format!("map_{}", map_id));

        let battle_duration = meta.as_ref().map(|m| m.battle_duration_secs).unwrap_or(0.0);

        let author = &br.author;
        let author_account_id = author.account_id;
        let author_team = if author.team_number == 1 { 1u8 } else { 2u8 };
        let author_won = author_team == winner_team;

        // 作者坦克 ID：优先取 meta，缺失时从玩家结果里找
        let mut author_tank_id = meta.as_ref().map(|m| m.tank_id as u32).unwrap_or(0);
        if author_tank_id == 0 {
            if let Some(pr) = br
                .player_results
                .iter()
                .find(|pr| pr.info.account_id == author_account_id)
            {
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

        let author_nickname = meta
            .as_ref()
            .map(|m| m.player_name.clone())
            .unwrap_or_else(|| {
                br.players
                    .iter()
                    .find(|p| p.account_id == author_account_id)
                    .map(|p| p.info.nickname.clone())
                    .unwrap_or_default()
            });

        // —— 遍历全部 14 名玩家 —— team/platoon/clan/nickname 都要去 players 里按账号 ID 关联
        let mut players = Vec::with_capacity(br.player_results.len());
        for pr in &br.player_results {
            let info = &pr.info;
            let tank_name = self.resolve_tank_name(info.tank_id);

            // team/platoon/clan/nickname 同取自该账号在 players 里的记录，只查一次
            let joined = br.players.iter().find(|p| p.account_id == info.account_id);

            let player_team = joined
                .map(|p| if p.info.team == 1 { 1u8 } else { 2u8 })
                .unwrap_or(0);

            let platoon_id = joined.and_then(|p| p.info.platoon_id);

            let clan_tag = joined.and_then(|p| p.info.clan_tag.clone());

            let nickname = joined.map(|p| p.info.nickname.clone()).unwrap_or_default();

            let settlement = settlements.get(&info.account_id);

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
                death_reason: settlement.and_then(|s| s.death_reason),
                // death_reason 缺省 = 普通击毁（字段表见 battle_results_extra）；整条结算缺失才 None
                survived: settlement.map(|s| s.death_reason == Some(-1)),
                life_time_secs: settlement.and_then(|s| s.life_time_secs),
                killer_id: settlement.and_then(|s| s.killer_id),
                n_enemies_spotted: settlement.and_then(|s| s.n_enemies_spotted),
                destruction_assistance: settlement.and_then(|s| s.destruction_assistance),
                gun_marks: settlement.and_then(|s| s.gun_marks),
                damage_received: settlement.and_then(|s| s.damage_received),
                victory_points_earned: settlement.and_then(|s| s.victory_points_earned),
                victory_points_seized: settlement.and_then(|s| s.victory_points_seized),
                hitpoints_left: settlement.and_then(|s| s.hitpoints_left),
                rank: ranks.get(&info.account_id).copied(),
                xp: settlement.and_then(|s| s.xp),
                credits: settlement.and_then(|s| s.credits),
                result_id: settlement.and_then(|s| s.result_id),
                killer_account_id: None,
            });
        }
        resolve_killer_accounts(&mut players);

        let mut summary = BattleSummary::from_naive(br.timestamp_secs);
        summary.file_name = file_name.to_string();
        summary.room_type = room_type;
        summary.arena_id = arena_id; // arena_bonus_type 由 parse_file / 宿主入口后置填充
        summary.finish_reason = root_fields.finish_reason;
        let roster_accounts: Vec<u32> = br.players.iter().map(|p| p.account_id).collect();
        let result_accounts: Vec<u32> = br
            .player_results
            .iter()
            .map(|pr| pr.info.account_id)
            .collect();
        summary.roster_complete = Some(roster_complete(&roster_accounts, &result_accounts));
        summary.result_duration_secs = root_fields.duration_secs;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn killer_account_is_joined_through_result_id() {
        let mut victim = PlayerSummary::for_test(1, "victim");
        victim.result_id = Some(100);
        victim.killer_id = Some(200);
        let mut killer = PlayerSummary::for_test(2, "killer");
        killer.result_id = Some(200);
        let mut unresolved = PlayerSummary::for_test(3, "unresolved");
        unresolved.killer_id = Some(999);
        let mut players = vec![victim, killer, unresolved];

        resolve_killer_accounts(&mut players);

        assert_eq!(players[0].killer_account_id, Some(2));
        assert_eq!(players[1].killer_account_id, None);
        assert_eq!(players[2].killer_account_id, None);
    }

    #[test]
    fn meta_fields_survive_non_utf8_bytes() {
        // 昵称里的非法 UTF-8 不得让 arenaBonusType / mapName 一起丢失
        let mut json = br#"{"playerName":""#.to_vec();
        json.extend_from_slice(&[0xff, 0xfe]);
        json.extend_from_slice(br#"","arenaBonusType":2,"mapName":"skit"}"#);
        let mut zip_bytes = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            w.start_file("meta.json", zip::write::FileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut w, &json).unwrap();
            w.finish().unwrap();
        }
        assert_eq!(read_arena_bonus_type(&zip_bytes), Some(2));
        assert_eq!(read_map_key(&zip_bytes).as_deref(), Some("skit"));
    }

    /// 结算阵容完整性：账号集合（与顺序无关）完全一致才完整；多余 / 缺失 / 空 → 不完整。
    #[test]
    fn roster_complete_requires_identical_account_sets() {
        assert!(roster_complete(&[1, 2, 3], &[3, 2, 1]));
        assert!(!roster_complete(&[1, 2, 3, 4], &[1, 2, 3]));
        assert!(!roster_complete(&[1, 2], &[1, 2, 3]));
        assert!(!roster_complete(&[], &[1]));
        assert!(!roster_complete(&[1], &[]));
    }
}
