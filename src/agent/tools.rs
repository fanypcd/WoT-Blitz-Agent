use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

use crate::agent::llm_client::ToolDefinition;
use crate::agent::llm_client::ToolFunction;
use crate::wargaming::api_client::WgApiClient;
use crate::wargaming::tank_resolver::{TankResolver, TankInfo};
use crate::replay::parser::ReplayParser;
use crate::replay::scanner::{ReplayScanner, ScanFilter};
use crate::models::report::AggregatedReport;

// =====================================================================
//  Agent 工具集
//  定义注册给 LLM 的工具 + 工具执行逻辑。LLM 通过 JSON 参数调用这些工具，
//  Agent Loop 调用 execute() 执行并把结果回填为 tool 消息。
// =====================================================================

/// Agent 工具执行器：持有 WG API 客户端、可选坦克解析器、回放目录。
pub struct AgentTools {
    pub wg_client: WgApiClient,
    pub tank_resolver: Option<TankResolver>,
    pub replay_dir: String,
}

impl AgentTools {
    /// 构造工具执行器。
    ///
    /// `tank_cache` 若存在则加载成坦克解析器（用于把 tank_id 翻译成名称）。
    pub fn new(app_id: &str, server: &str, replay_dir: &str, tank_cache: Option<&Path>) -> Self {
        let tank_resolver = tank_cache
            .filter(|p| p.exists())
            .and_then(|p| TankResolver::load_from_json_file(p).ok());
        Self {
            wg_client: WgApiClient::new(app_id, server),
            tank_resolver,
            replay_dir: replay_dir.to_string(),
        }
    }

    /// 返回注册给 LLM 的工具定义（OpenAI tools 格式，共 6 个）。
    pub fn definitions() -> Vec<ToolDefinition> {
        vec![
            // 工具 1：按昵称搜索玩家
            ToolDefinition {
                def_type: "function".to_string(),
                function: ToolFunction {
                    name: "search_player".to_string(),
                    description: "Search for a WoTB player by nickname. Returns account_id and nickname.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "nickname": {"type": "string", "description": "Player nickname to search"}
                        },
                        "required": ["nickname"]
                    }),
                },
            },
            // 工具 2：查询玩家累计战绩
            ToolDefinition {
                def_type: "function".to_string(),
                function: ToolFunction {
                    name: "get_player_stats".to_string(),
                    description: "Get player cumulative stats from WG API, including random and rating battle stats, win rate, avg damage, hit rate, rating mm_rating.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "account_id": {"type": "integer", "description": "Player account ID"}
                        },
                        "required": ["account_id"]
                    }),
                },
            },
            // 工具 3：批量扫描回放生成报告
            ToolDefinition {
                def_type: "function".to_string(),
                function: ToolFunction {
                    name: "scan_replays".to_string(),
                    description: "Scan replay files in the replay directory and generate an aggregated report. Can filter by mode (rating/regular/all) and days. Returns battle count, win rate, avg damage, tank usage, map stats, rating changes.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "mode": {"type": "string", "description": "Filter by game mode: rating, regular, or all", "default": "all"},
                            "days": {"type": "integer", "description": "Only include replays from last N days. Omit for all."}
                        }
                    }),
                },
            },
            // 工具 4：解析单场回放
            ToolDefinition {
                def_type: "function".to_string(),
                function: ToolFunction {
                    name: "parse_replay".to_string(),
                    description: "Parse a single replay file and return detailed battle results including all 14 players' stats, author stats, map, mode, duration, winner.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "file_path": {"type": "string", "description": "Path to the .wotbreplay file"}
                        },
                        "required": ["file_path"]
                    }),
                },
            },
            // 工具 5：对比近期回放 vs API 累计
            ToolDefinition {
                def_type: "function".to_string(),
                function: ToolFunction {
                    name: "compare_replay_vs_api".to_string(),
                    description: "Compare recent replay stats vs WG API cumulative stats. Shows how the player is performing recently vs their all-time average.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "nickname": {"type": "string", "description": "Player nickname"},
                            "mode": {"type": "string", "description": "Filter: rating, regular, or all", "default": "rating"}
                        },
                        "required": ["nickname"]
                    }),
                },
            },
            // 工具 6：打开 3D 装甲查看器（可指定射击车辆 + 受击车辆）
            ToolDefinition {
                def_type: "function".to_string(),
                function: ToolFunction {
                    name: "view_tank".to_string(),
                    description: "Open the 3D armor viewer for one or two tanks so the user can inspect armor, rotate the turret/gun, and simulate shell penetration. Use when the user asks to see/visualize a tank's armor, compare two tanks, or simulate a specific shooter (attacking) tank vs a target (defending) tank. Pass the tank name or numeric tank ID for `target` (the tank being viewed/inspected) and optionally `shooter` (the tank firing the shell). Both are fuzzy-matched: if a name matches multiple tanks, a numbered candidate list is returned for the user to disambiguate.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "target": {"type": "string", "description": "Tank to view/inspect (defending side). Name (e.g. 'E 100') or numeric ID (e.g. 7169). Fuzzy match."},
                            "shooter": {"type": "string", "description": "Shooting/attacking tank whose shell & caliber are used for penetration (optional). Name or numeric ID, fuzzy match. Omit to default to the target."},
                            "port": {"type": "integer", "description": "Preferred port (optional, default 0 = auto-assign)"}
                        },
                        "required": ["target"]
                    }),
                },
            },
        ]
    }

    /// 按工具名分发执行，返回人类可读的结果文本（会回填为 tool 消息）。
    pub fn execute(&self, tool_name: &str, args: &Value) -> Result<String> {
        match tool_name {
            "search_player" => {
                let nickname = args["nickname"].as_str().unwrap_or("");
                let results = self.wg_client.search_player(nickname, false)?;
                if results.is_empty() {
                    Ok("No players found.".to_string())
                } else {
                    let lines: Vec<String> = results.iter()
                        .map(|(n, id)| format!("  {} (account_id={})", n, id))
                        .collect();
                    Ok(format!("Found {} players:\n{}", results.len(), lines.join("\n")))
                }
            }
            "get_player_stats" => {
                let account_id = args["account_id"].as_u64().unwrap_or(0) as u32;
                let stats = self.wg_client.get_player_stats(account_id)?;
                let n_r = stats.random_battles.max(1) as f64;
                let n_t = stats.rating_battles.max(1) as f64;
                Ok(format!(
                    "Player: {} (id={})\n\
                     \n--- Random Battles ---\n\
                     Battles: {}, WR: {:.1}%, Avg Dmg: {:.0}, Avg Frags: {:.2}, Hit Rate: {:.1}%\n\
                     \n--- Rating Battles ---\n\
                     Battles: {}, WR: {:.1}%, Avg Dmg: {:.0}, Avg Frags: {:.2}, Hit Rate: {:.1}%\n\
                     mm_rating: {:.2}, Display Rating: {}, Season: {}",
                    stats.nickname, stats.account_id,
                    stats.random_battles, stats.random_wins as f64 / n_r * 100.0,
                    stats.random_damage_dealt as f64 / n_r, stats.random_frags as f64 / n_r,
                    if stats.random_shots > 0 { stats.random_hits as f64 / stats.random_shots as f64 * 100.0 } else { 0.0 },
                    stats.rating_battles, stats.rating_wins as f64 / n_t * 100.0,
                    stats.rating_damage_dealt as f64 / n_t, stats.rating_frags as f64 / n_t,
                    if stats.rating_shots > 0 { stats.rating_hits as f64 / stats.rating_shots as f64 * 100.0 } else { 0.0 },
                    stats.rating_mm_rating.unwrap_or(0.0),
                    stats.rating_display_rating.unwrap_or(0),
                    stats.rating_season.unwrap_or(0),
                ))
            }
            "scan_replays" => {
                let mode = args["mode"].as_str().unwrap_or("all");
                let days = args["days"].as_i64();

                let scanner = if let Some(ref r) = self.tank_resolver {
                    ReplayScanner::with_resolver(r)
                } else {
                    ReplayScanner::new()
                };

                let filter = ScanFilter::from_mode(mode, days);

                let dir = Path::new(&self.replay_dir);
                let battles = scanner.scan_dir(dir, &filter, |_| {})?;

                if battles.is_empty() {
                    return Ok("No replays found matching the filter.".to_string());
                }

                let room_type = battles.first().map(|b| b.room_type.as_str()).unwrap_or("Unknown");
                let report = AggregatedReport::from_battles(&battles, room_type);

                let mut result = format!(
                    "Replay Scan Results:\n\
                     Player: {}, Mode: {}, Date: {}\n\
                     Battles: {}, WR: {:.1}%, Avg Dmg: {:.0}, Avg Frags: {:.2}\n\
                     Hit Rate: {:.1}%, Avg Block: {:.0}, Avg Assist: {:.0}\n\
                     Rating: {:.2} -> {:.2} ({:+.2})\n\
                     \nTop Tanks:\n",
                    report.author_name, report.room_type, report.date_range,
                    report.total_battles, report.win_rate, report.avg_damage, report.avg_frags,
                    report.hit_rate, report.avg_damage_blocked, report.avg_assisted,
                    report.rating_start.unwrap_or(0.0), report.rating_end.unwrap_or(0.0),
                    report.rating_delta.unwrap_or(0.0),
                );

                for t in report.tank_usage.iter().take(5) {
                    result.push_str(&format!("  {} - {}b, WR {:.0}%, avg_dmg {:.0}\n",
                        t.tank_name, t.battles, t.win_rate, t.avg_damage));
                }

                Ok(result)
            }
            "parse_replay" => {
                let file_path = args["file_path"].as_str().unwrap_or("");
                let parser = if let Some(ref r) = self.tank_resolver {
                    ReplayParser::with_resolver(r)
                } else {
                    ReplayParser::new()
                };
                let summary = parser.parse_file(Path::new(file_path))?;

                Ok(format!(
                    "Replay: {}\nPlayer: {} (tank: {})\nMap: {}, Mode: {}, Duration: {:.0}s\n\
                     Winner: Team {}\nAuthor: Team {} ({})\n\
                     Shots: {}/{}, Pens: {}, Damage: {}, Kills: {}",
                    summary.file_name, summary.author_nickname, summary.author_tank_name,
                    summary.map_name, summary.room_type, summary.battle_duration_secs,
                    summary.winner_team, summary.author_team,
                    if summary.author_won { "WON" } else { "LOST" },
                    summary.author.n_shots, summary.author.n_hits,
                    summary.author.n_penetrations, summary.author.damage_dealt,
                    summary.players.iter()
                        .find(|p| p.account_id == summary.author_account_id)
                        .map(|p| p.n_enemies_destroyed).unwrap_or(0),
                ))
            }
            "compare_replay_vs_api" => {
                let nickname = args["nickname"].as_str().unwrap_or("");
                let mode = args["mode"].as_str().unwrap_or("rating");
                let results = self.wg_client.search_player(nickname, true)?;
                if results.is_empty() {
                    return Ok(format!("Player not found: {}", nickname));
                }
                let account_id = results[0].1;
                let api_stats = self.wg_client.get_player_stats(account_id)?;

                let scanner = if let Some(ref r) = self.tank_resolver {
                    ReplayScanner::with_resolver(r)
                } else {
                    ReplayScanner::new()
                };

                let filter = ScanFilter::from_mode(mode, None);

                let battles = scanner.scan_dir(Path::new(&self.replay_dir), &filter, |_| {})?;
                if battles.is_empty() {
                    return Ok("No replays found.".to_string());
                }

                let room_type = battles.first().map(|b| b.room_type.as_str()).unwrap_or("Unknown");
                let report = AggregatedReport::from_battles(&battles, room_type);

                let is_rating = mode == "rating";
                let (api_battles, api_wins, api_dmg, api_frags, api_shots, api_hits) = if is_rating {
                    (api_stats.rating_battles, api_stats.rating_wins,
                     api_stats.rating_damage_dealt, api_stats.rating_frags,
                     api_stats.rating_shots, api_stats.rating_hits)
                } else {
                    (api_stats.random_battles, api_stats.random_wins,
                     api_stats.random_damage_dealt, api_stats.random_frags,
                     api_stats.random_shots, api_stats.random_hits)
                };

                let an = api_battles.max(1) as f64;
                Ok(format!(
                    "Comparison: {} ({} mode)\n\
                     \n  Metric          Replay     API Total   Difference\n\
                     Battles:         {}          {}          \n\
                     Win Rate:        {:.1}%      {:.1}%      {:+.1}%\n\
                     Avg Damage:      {:.0}        {:.0}        {:+.0}\n\
                     Avg Frags:       {:.2}        {:.2}        {:+.2}\n\
                     Hit Rate:        {:.1}%      {:.1}%      {:+.1}%\n\
                     \nReplay Rating: {:.2} -> {:.2} ({:+.2})\n\
                     API Rating: {:.2} (display {})\n\
                     Coverage: {:.1}%",
                    nickname, mode,
                    report.total_battles, api_battles,
                    report.win_rate, api_wins as f64 / an * 100.0, report.win_rate - api_wins as f64 / an * 100.0,
                    report.avg_damage, api_dmg as f64 / an, report.avg_damage - api_dmg as f64 / an,
                    report.avg_frags, api_frags as f64 / an, report.avg_frags - api_frags as f64 / an,
                    report.hit_rate, if api_shots > 0 { api_hits as f64 / api_shots as f64 * 100.0 } else { 0.0 },
                    report.hit_rate - (if api_shots > 0 { api_hits as f64 / api_shots as f64 * 100.0 } else { 0.0 }),
                    report.rating_start.unwrap_or(0.0), report.rating_end.unwrap_or(0.0), report.rating_delta.unwrap_or(0.0),
                    api_stats.rating_mm_rating.unwrap_or(0.0), api_stats.rating_display_rating.unwrap_or(0),
                    report.total_battles as f64 / api_battles.max(1) as f64 * 100.0,
                ))
            }
            "view_tank" => self.execute_view_tank(args),
            _ => Ok(format!("Unknown tool: {}", tool_name)),
        }
    }

    /// 处理 `view_tank` 工具：解析射击/受击坦克，后台启动 3D 查看器并打开浏览器。
    ///
    /// `target`（受击/查看方）与可选 `shooter`（射击方）都做模糊匹配：
    /// - 数字 ID 直接用；
    /// - 名称不区分大小写，优先精确、其次"最短子串"；
    /// - 命中多个候选时返回编号列表让用户选择（不直接启动）。
    ///
    /// 在独立线程用 tokio runtime 启动 viewer::serve（阻塞服务），因此不会卡住 Agent 循环。
    fn execute_view_tank(&self, args: &Value) -> Result<String> {
        let target_ref = args["target"].as_str().unwrap_or("").trim();
        let shooter_ref = args["shooter"].as_str().map(|s| s.trim()).filter(|s| !s.is_empty());

        if target_ref.is_empty() {
            return Ok("Please provide a tank name or ID to view (e.g. target: 'E 100').".to_string());
        }

        // 装载坦克解析器（tank_cache.json 含 723 辆坦克），用于模糊搜索 + 名称解析
        let resolver = TankResolver::load_from_json_file(&crate::data::data_path("tank_cache.json"))
            .or_else(|_| self.tank_resolver.clone().ok_or_else(|| anyhow::anyhow!("no resolver")))?;

        // 解析受击车辆
        let target = self.resolve_tank(&resolver, target_ref);
        let target = match target {
            ResolveTank::One(id) => id,
            ResolveTank::Ambiguous(cands) => {
                return Ok(format!(
                    "\"\u{9}The name '{}' matches multiple tanks. Please pick one:\n{}",
                    target_ref,
                    format_candidates(&cands)
                ));
            }
            ResolveTank::None => {
                return Ok(format!(
                    "Could not find a tank matching '{}'. Ask for a specific tank name (e.g. IS-7, E 100) or numeric ID.",
                    target_ref
                ));
            }
        };

        // 解析射击车辆（可选）
        let shooter = if let Some(sref) = shooter_ref {
            match self.resolve_tank(&resolver, sref) {
                ResolveTank::One(id) => Some(id),
                ResolveTank::Ambiguous(cands) => {
                    return Ok(format!(
                        "\u{9}The shooter name '{}' matches multiple tanks. Please pick one:\n{}",
                        sref,
                        format_candidates(&cands)
                    ));
                }
                ResolveTank::None => {
                    return Ok(format!(
                        "Could not find a shooter tank matching '{}'. Ask for a specific tank name (e.g. IS-7, E 100) or numeric ID.",
                        sref
                    ));
                }
            }
        } else {
            None
        };

        let target_name = resolver.resolve(target).unwrap_or_else(|| format!("tank_{}", target));
        let shooter_name = shooter.map(|id| resolver.resolve(id).unwrap_or_else(|| format!("tank_{}", id)));

        // 后台线程启动查看器，避免阻塞 Agent 循环
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new();
            if let Ok(rt) = rt {
                let _ = rt.block_on(crate::wargaming::viewer::serve(resolver, target, shooter));
            }
        });

        let desc = match (&shooter_name, shooter) {
            (Some(sname), Some(_)) => format!("**{}** (attacking) → **{}** (defending)", sname, target_name),
            (_, _) => format!("**{}**", target_name),
        };
        Ok(format!(
            "Opened the 3D armor viewer for {}. A browser window should have appeared showing the tank model, armor plates, and penetration analysis. The user can rotate the view, select a shell, switch shooter/target via the tank picker, and click on armor to test penetration.",
            desc
        ))
    }

    /// 模糊解析一辆坦克：`ResolveTank::One(id)` 唯一命中，`Ambiguous` 多候选，`None` 未命中。
    fn resolve_tank(&self, resolver: &TankResolver, tank_ref: &str) -> ResolveTank {
        // 数字 ID 直接用（即使 tank_cache 里没有该 ID，也透传给查看器）
        if let Ok(id) = tank_ref.parse::<u32>() {
            return ResolveTank::One(id);
        }

        // 名称归一：-/·/./ 全部视为空格，使 "E 100" 与 "E-100"、"IS-7" 与 "IS 7" 等可互相命中
        let norm = |s: &str| s.chars().map(|c| if c=='-'||c=='·'||c=='.'||c=='_' {' '} else {c}).collect::<String>().to_lowercase();
        let needle = norm(tank_ref);
        if needle.is_empty() { return ResolveTank::None; }
        let mut exacts: Vec<(u32, &TankInfo)> = Vec::new();
        let mut subs: Vec<(u32, &TankInfo, usize)> = Vec::new(); // (id, info, score)
        for (id, info) in resolver.iter() {
            let cand = norm(&info.name);
            if cand == needle {
                exacts.push((id, info));
            } else if let Some(pos) = cand.find(&needle) {
                // 命中位置越靠前越好；同名不同短名长度带来惩罚
                let score = pos + cand.len().saturating_sub(needle.len());
                subs.push((id, info, score));
            }
        }

        // 优先精确集；否则用子串集
        let base: Vec<(u32, &TankInfo)> = if !exacts.is_empty() {
            exacts
        } else {
            subs.sort_by_key(|(_, _, s)| *s);
            subs.into_iter().map(|(id, info, _)| (id, info)).collect()
        };

        match base.as_slice() {
            [] => ResolveTank::None,
            [single] => ResolveTank::One(single.0),
            many => {
                // 同一名称多个实体（如不同国家/等级的同类坦克）→ 交给用户选
                let candidates: Vec<TankCandidate> = many.iter()
                    .map(|(id, info)| TankCandidate {
                        id: *id,
                        name: info.name.clone(),
                        tier: info.tier as u32,
                        nation: info.nation.clone(),
                        tank_type: info.tank_type.clone(),
                    })
                    .collect();
                ResolveTank::Ambiguous(candidates)
            }
        }
    }
}

/// 一次性坦克解析结果。
#[derive(Debug)]
enum ResolveTank {
    One(u32),
    Ambiguous(Vec<TankCandidate>),
    None,
}

/// 供消歧列表展示的坦克候选。
#[derive(Debug)]
struct TankCandidate {
    id: u32,
    name: String,
    tier: u32,
    nation: String,
    tank_type: String,
}

/// 把候选列表格式化为带编号的文本（LLM 据此让用户选择）。
fn format_candidates(cands: &[TankCandidate]) -> String {
    let mut out = String::new();
    for (i, c) in cands.iter().enumerate() {
        out.push_str(&format!("  {}. {} (id={}) — Tier {}, {}, {}\n",
            i + 1, c.name, c.id, c.tier, c.nation, c.tank_type));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wargaming::tank_resolver::TankInfo;

    fn mk_resolver(tanks: &[(u32, &str, u8, &str, &str)]) -> TankResolver {
        let mut r = TankResolver::new();
        for (id, name, tier, nation, ttype) in tanks {
            r.add(*id, TankInfo {
                name: name.to_string(),
                tier: *tier,
                tank_type: ttype.to_string(),
                nation: nation.to_string(),
                is_premium: false,
                armor: None,
                shells: Vec::new(),
                hp: None,
                speed_forward: None,
                speed_reverse: None,
                hull_traverse: None,
                view_range: None,
                turret_traverse_speed: None,
                gun_depression: None,
                gun_elevation: None,
                turret_traverse_left: None,
                turret_traverse_right: None,
            });
        }
        r
    }

    fn tools() -> AgentTools {
        AgentTools::new("app", "asia", "", None)
    }

    fn resolve_ok(res: ResolveTank) -> u32 {
        match res {
            ResolveTank::One(id) => id,
            _ => panic!("expected One, got {:?}", res),
        }
    }

    #[test]
    fn exact_match_beats_substring() {
        let r = mk_resolver(&[
            (9489, "E 100", 10, "germany", "heavyTank"),
            (12049, "Jg.Pz. E 100", 10, "germany", "AT-SPG"),
        ]);
        let res = tools().resolve_tank(&r, "e 100");
        assert_eq!(resolve_ok(res), 9489);
    }

    #[test]
    fn numeric_id_passthrough() {
        let r = mk_resolver(&[(9489, "E 100", 10, "germany", "heavyTank")]);
        let res = tools().resolve_tank(&r, "7169");
        assert_eq!(resolve_ok(res), 7169);
    }

    #[test]
    fn ambiguous_returns_candidates() {
        let r = mk_resolver(&[
            (529, "Tiger I", 7, "germany", "heavyTank"),
            (5137, "Tiger II", 8, "germany", "heavyTank"),
            (10769, "Tiger (P)", 7, "germany", "heavyTank"),
        ]);
        let res = tools().resolve_tank(&r, "Tiger");
        match res {
            ResolveTank::Ambiguous(cands) => {
                assert_eq!(cands.len(), 3);
                let txt = format_candidates(&cands);
                assert!(txt.contains("Tiger I"));
                assert!(txt.contains("Tiger II"));
                assert!(txt.contains("Tiger (P)"));
            }
            _ => panic!("expected Ambiguous"),
        }
    }

    #[test]
    fn none_when_not_found() {
        let r = mk_resolver(&[(1, "T-34", 5, "ussr", "mediumTank")]);
        assert!(matches!(tools().resolve_tank(&r, "Maus"), ResolveTank::None));
    }

    // 用真实 tank_cache.json（723 辆）验证常见模糊查询的实际表现。
    // 若文件缺失则跳过（不依赖构建环境的静态数据）。
    #[test]
    fn real_cache_fuzzy_search() {
        let Ok(r) = TankResolver::load_from_json_file(&crate::data::data_path("tank_cache.json")) else {
            eprintln!("tank_cache.json missing, skipping real_cache test");
            return;
        };
        // "IS-7" 唯一命中 → One(7169)
        match tools().resolve_tank(&r, "IS-7") {
            ResolveTank::One(id) => assert_eq!(id, 7169),
            other => panic!("IS-7 should be unique, got {:?}", other),
        }
        // "E 100" 精确命中 → One(9489)
        match tools().resolve_tank(&r, "E 100") {
            ResolveTank::One(id) => assert_eq!(id, 9489),
            other => panic!("E 100 should be exact match, got {:?}", other),
        }
        // "Tiger" 多命中 → Ambiguous
        assert!(matches!(tools().resolve_tank(&r, "Tiger"), ResolveTank::Ambiguous(_)));
        // "Maus" 精确命中 → One(6929)
        match tools().resolve_tank(&r, "Maus") {
            ResolveTank::One(id) => assert_eq!(id, 6929),
            other => panic!("Maus should be unique, got {:?}", other),
        }
    }

    // 验证 view_tank 工具定义已改用 target/shooter 参数（而非旧 tank）。
    #[test]
    fn view_tank_tool_definition_uses_target_shooter() {
        let defs = AgentTools::definitions();
        let vt = defs.iter()
            .find(|d| d.function.name == "view_tank")
            .expect("view_tank tool must exist");
        let props = vt.function.parameters.get("properties").and_then(|p| p.as_object())
            .expect("parameters.properties");
        assert!(props.contains_key("target"), "must have target param");
        assert!(props.contains_key("shooter"), "must have shooter param");
        assert!(!props.contains_key("tank"), "old 'tank' param should be gone");
        let required = vt.function.parameters.get("required").and_then(|r| r.as_array())
            .expect("required array");
        assert!(required.iter().any(|r| r == "target"), "target must be required");
    }
}
