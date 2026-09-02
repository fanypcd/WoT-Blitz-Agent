// =====================================================================
//  WoTB 回放分析 Agent — CLI 入口
//  用 clap 解析子命令并分发执行。部分命令（view/web）需要异步 Tokyo 运行时，
//  其余为同步单步操作。
// =====================================================================

mod models;
mod replay;
mod wargaming;
mod agent;
mod web;
mod data;

use std::path::PathBuf;
use std::io::{self, Write, BufRead};
use clap::{Parser as ClapParser, Subcommand};
use anyhow::Result;

use crate::models::report::AggregatedReport;
use crate::models::config::{Config, TokenUsage};
use crate::replay::scanner::{ReplayScanner, ScanFilter};
use crate::wargaming::tank_resolver::TankResolver;
use crate::wargaming::api_client::WgApiClient;
use crate::wargaming::snapshot::SnapshotStore;
use crate::replay::combat::{CombatTimeline, CombatEventType};
use crate::agent::Agent;

/// CLI 顶层入口：解析出的子命令。
#[derive(ClapParser)]
#[command(name = "wotb-agent", version = "0.1.0", about = "WoTB Replay Analysis Agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// 全部子命令（各含自己的参数，由 clap 生成 --help）。
#[derive(Subcommand)]
enum Commands {
    /// Parse a single replay file
    Single {
        /// Path to the .wotbreplay file
        file: PathBuf,
        /// Output JSON instead of text
        #[arg(short, long)]
        json: bool,
        /// Tank cache file path (JSON)
        #[arg(long)]
        tank_cache: Option<PathBuf>,
    },
    /// Scan a directory for replays and aggregate stats
    Scan {
        /// Directory containing .wotbreplay files
        dir: PathBuf,
        /// Filter by room type: all, rating, regular, training
        #[arg(short, long, default_value = "all")]
        mode: String,
        /// Only include replays from the last N days
        #[arg(short, long)]
        days: Option<i64>,
        /// Output JSON report to file
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Tank cache file path (JSON)
        #[arg(long)]
        tank_cache: Option<PathBuf>,
        /// Fetch tank data from WG API (requires application_id)
        #[arg(long)]
        fetch_tanks: bool,
        /// WG API application_id
        #[arg(long, env = "WG_APPLICATION_ID")]
        app_id: Option<String>,
        /// WG API server: asia, eu, na
        #[arg(long, default_value = "asia")]
        server: String,
    },
    /// Build tank resolver from local BlitzKit pb data (no WG API needed)
    FetchTanks {
        /// Output file path
        #[arg(short, long, default_value = "data/tank_cache.json")]
        output: PathBuf,
    },
    /// Analyze lineup strength of players for pre-match planning
    Prematch {
        /// Comma-separated player nicknames, or a .wotbreplay path to extract both teams
        nicknames: Option<String>,
        /// File containing player nicknames (one per line)
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Parse a .wotbreplay replay and analyze both teams (same as nicknames path)
        #[arg(short, long)]
        replay: Option<PathBuf>,
        /// WG API application_id
        #[arg(long, env = "WG_APPLICATION_ID")]
        app_id: String,
        /// WG API server
        #[arg(long, default_value = "asia")]
        server: String,
    },
    /// Take or view API data snapshots for time-series analysis
    Snapshot {
        /// WG API application_id
        #[arg(long, env = "WG_APPLICATION_ID")]
        app_id: String,
        /// WG API server
        #[arg(long, default_value = "asia")]
        server: String,
        /// Player nickname
        nickname: String,
        /// Action: take (new snapshot), list (show all), diff (compare oldest vs latest)
        #[arg(short, long, default_value = "take")]
        action: String,
        /// Snapshot storage directory
        #[arg(long, default_value = "snapshots")]
        dir: PathBuf,
    },
    /// Parse game data files (DVPL format) for armor/collision data
    ParseGame {
        /// Tank dev name (e.g. R132_T100LT)
        dev_name: String,
        /// Game data directory (auto-detected if omitted)
        #[arg(long)]
        game_dir: Option<PathBuf>,
    },
    /// Batch-extract armor/collision data from game DVPL files into game_data/ (portable)
    ExtractGame {
        /// Game data directory (auto-detected if omitted)
        #[arg(long)]
        game_dir: Option<PathBuf>,
        /// Output directory for extracted per-tank JSON files
        #[arg(long, default_value = "data/game_data")]
        output: PathBuf,
        /// Re-extract even if output file already exists
        #[arg(long)]
        force: bool,
    },
    /// Download BlitzKit tanks.pb (唯一坦克数据源) to data/
    FetchBlitzkit {
        /// Output file path（tanks.pb）
        #[arg(long, default_value = "data/tanks.pb")]
        output: PathBuf,
    },
    /// Batch-download all tank preview icons from BlitzKit into tank_images/
    FetchIcons {
        /// Output directory for tank icons
        #[arg(long, default_value = "tank_images")]
        dir: PathBuf,
        /// Re-download icons even if already present
        #[arg(long)]
        force: bool,
    },
    /// View 3D tank model in browser
    View {
        /// Tank ID
        tank_id: u32,
        /// Tank cache file
        #[arg(long)]
        tank_cache: Option<PathBuf>,
    },
    /// Launch the Web Agent GUI (independent web app)
    Web {
        /// Config file path
        #[arg(short, long, default_value = "config.toml")]
        config: PathBuf,
        /// Tank cache file
        #[arg(long)]
        tank_cache: Option<PathBuf>,
    },
    /// Start interactive Agent chat (R5/R6)
    Chat {
        /// Config file path
        #[arg(short, long, default_value = "config.toml")]
        config: PathBuf,
        /// Save session to JSON on exit
        #[arg(long)]
        save: Option<PathBuf>,
        /// Load session from JSON on start
        #[arg(long)]
        load: Option<PathBuf>,
    },
    /// Show or create config file (R3)
    Config {
        /// Config file path
        #[arg(short, long, default_value = "config.toml")]
        file: PathBuf,
        /// Show current config
        #[arg(long)]
        show: bool,
    },
    /// Show token usage statistics (R6)
    Usage {
        /// Usage file path
        #[arg(short, long, default_value = "token_usage.json")]
        file: PathBuf,
    },
    /// Query player stats from WG API
    Player {
        /// Player nickname (exact match)
        nickname: String,
        /// WG API application_id
        #[arg(long, env = "WG_APPLICATION_ID")]
        app_id: String,
        /// WG API server
        #[arg(long, default_value = "asia")]
        server: String,
    },
    /// Compare replay stats vs WG API cumulative stats
    Compare {
        /// Player nickname
        nickname: String,
        /// Replay directory
        dir: PathBuf,
        /// WG API application_id
        #[arg(long, env = "WG_APPLICATION_ID")]
        app_id: String,
        /// WG API server
        #[arg(long, default_value = "asia")]
        server: String,
        /// Tank cache file
        #[arg(long)]
        tank_cache: Option<PathBuf>,
        /// Filter by mode: rating, regular, all
        #[arg(short, long, default_value = "rating")]
        mode: String,
    },
    /// Analyze combat events from a replay
    Combat {
        /// Path to the .wotbreplay file
        file: PathBuf,
        /// Output JSON instead of text
        #[arg(short, long)]
        json: bool,
    },
}

/// 程序入口：解析参数 → 分发到对应子命令。
fn main() -> Result<()> {
    let cli = Cli::parse();

    // View（3D 查看器）需要 async 运行时（axum 服务器）
    if let Commands::View { tank_id, tank_cache } = &cli.command {
        let tank_id = *tank_id;
        let tank_cache = tank_cache.clone();
        return tokio::runtime::Runtime::new()?.block_on(async {
            let resolver_path = tank_cache.unwrap_or_else(|| crate::data::data_path("tank_cache.json"));
            let resolver = TankResolver::load_from_json_file(&resolver_path)
                .map_err(|e| {
                    eprintln!("Failed to load tank cache: {}. Run `fetch-tanks` first.", e);
                    e
                })?;

            let name = resolver.resolve(tank_id).unwrap_or_else(|| format!("tank_{}", tank_id));
            eprintln!("Starting 3D viewer for {} (id={})...", name, tank_id);
            eprintln!("Models served via local cache proxy (/glb/{}/...), first fetch cached to glb_cache/.", tank_id);

            crate::wargaming::viewer::serve(resolver, tank_id, None).await
        });
    }

    // Web 图形界面同样需要 async 运行时
    if let Commands::Web { config, .. } = &cli.command {
        let config_path = config.clone();
        return tokio::runtime::Runtime::new()?.block_on(async {
            crate::web::serve(config_path).await
        });
    }

    // 其余命令按名称分发
    match cli.command {
        Commands::ParseGame { dev_name, game_dir } => {
        // parse-game：解析单辆坦克的游戏 DVPL（碰撞 YAML + 装甲 XML），打印到 stdout
            use crate::wargaming::dvpl::{DvplFile, CollisionData};

            let game_dir = crate::wargaming::game_extract::resolve_game_dir(game_dir.as_deref())?;
            let nations = ["ussr", "usa", "germany", "uk", "japan", "china", "france", "european", "other"];
            let mut found = false;

            for nation in &nations {
                let filepath = game_dir.join(format!("3d/Tanks/Parameters/{}/{}.yaml.dvpl", nation, dev_name));
                if filepath.exists() {
                    eprintln!("Found: {}", filepath.display());
                    let dvpl = DvplFile::read(&filepath)?;
                    let text = String::from_utf8_lossy(&dvpl.data);
                    eprintln!("Decompressed: {} bytes (compression type {})", dvpl.data.len(), dvpl.compression_type);

                    let collision = CollisionData::parse_from_yaml(&text);
                    println!("\n=== {} Collision Data ===\n", dev_name);
                    if let Some(ref c) = collision {
                        if let Some(ref hull) = c.hull_bbox {
                            println!("  Hull bbox:   min({:.2}, {:.2}, {:.2}) max({:.2}, {:.2}, {:.2})",
                                hull.min[0], hull.min[1], hull.min[2], hull.max[0], hull.max[1], hull.max[2]);
                        }
                        if let Some(ref turret) = c.turret_bbox {
                            println!("  Turret bbox: min({:.2}, {:.2}, {:.2}) max({:.2}, {:.2}, {:.2})",
                                turret.min[0], turret.min[1], turret.min[2], turret.max[0], turret.max[1], turret.max[2]);
                        }
                        if let Some(ref gun) = c.gun_bbox {
                            println!("  Gun bbox:    min({:.2}, {:.2}, {:.2}) max({:.2}, {:.2}, {:.2})",
                                gun.min[0], gun.min[1], gun.min[2], gun.max[0], gun.max[1], gun.max[2]);
                        }
                        if let Some(ref chassis) = c.chassis_bbox {
                            println!("  Chassis bbox: min({:.2}, {:.2}, {:.2}) max({:.2}, {:.2}, {:.2})",
                                chassis.min[0], chassis.min[1], chassis.min[2], chassis.max[0], chassis.max[1], chassis.max[2]);
                        }
                        if let Some(th) = c.average_thickness_hull {
                            println!("  Avg thickness hull:   {:.1}mm", th);
                        }
                        if let Some(th) = c.average_thickness_turret {
                            println!("  Avg thickness turret: {:.1}mm", th);
                        }
                        if let Some(ref p) = c.hull_points {
                            println!("  Hull points:   [{:.2}, {:.2}, {:.2}]", p[0], p[1], p[2]);
                        }
                        if let Some(ref p) = c.turret_points {
                            println!("  Turret points: [{:.2}, {:.2}, {:.2}]", p[0], p[1], p[2]);
                        }
                        if let Some(ref p) = c.gun_points {
                            println!("  Gun points:    [{:.2}, {:.2}, {:.2}]", p[0], p[1], p[2]);
                        }
                    }

                    found = true;
                    break;
                }
            }

            if !found {
                eprintln!("YAML file not found in any nation directory.");
            }

            // Also load and display XML DVPL armor data
            for nation in &nations {
                let xml_path = game_dir.join(format!("XML/item_defs/vehicles/{}/{}.xml.dvpl", nation, dev_name));
                if xml_path.exists() {
                    eprintln!("\nFound XML: {}", xml_path.display());
                    let dvpl = DvplFile::read(&xml_path)?;
                    let text = String::from_utf8_lossy(&dvpl.data);
                    let armor = crate::wargaming::dvpl::ArmorModel::parse_from_xml(&text);
                    if let Some(ref am) = armor {
                        println!("\n=== Armor Model ===");
                        println!("  Hull plates: {:?}", am.hull.plates);
                        if let Some(ref t) = am.turret { println!("  Turret plates: {:?}", t.plates); }
                        if let Some(ref g) = am.gun { println!("  Gun plates: {:?}", g.plates); }
                        if let Some(ref c) = am.chassis { println!("  Chassis: left={} right={}", c.left_track, c.right_track); }
                    }
                    break;
                }
            }
            return Ok(());
        }
        Commands::ExtractGame { game_dir, output, force } => {
        // extract-game：批量把全部坦克的装甲/碰撞数据提取到 game_data/（可移植）
            let stats = crate::wargaming::game_extract::extract_all(
                game_dir.as_deref(), &output, force,
            )?;
            println!("\n=== Game Data Extraction ===");
            println!("  Output dir:   {}", output.display());
            println!("  Extracted:    {}", stats.extracted);
            println!("  Cached:       {} (already present, use --force to re-extract)", stats.cached);
            println!("  Missing src:  {} (file absent in game dir)", stats.missing_files);
            println!("  Parse failed: {}", stats.parse_failed);
            println!("  No dev_name:  {}", stats.skipped_no_name);
            println!("  Write failed: {}", stats.write_failed);
            return Ok(());
        }
        Commands::FetchBlitzkit { output } => {
        // fetch-blitzkit：下载 tanks.pb（唯一数据源）到 data/tanks.pb
            let n = tokio::runtime::Runtime::new()?
                .block_on(crate::wargaming::blitzkit::fetch_and_save(&output))?;
            println!("Saved tanks.pb ({}) — parsed {} tanks -> {}", output.display(), n, output.display());
            return Ok(());
        }
        Commands::FetchIcons { dir, force } => {
        // fetch-icons：批量下载坦克封面图到 tank_images/
            let (downloaded, cached, failed) =
                crate::wargaming::blitzkit::download_all_icons(&dir, force)?;
            println!("Tank icons downloaded={} cached={} failed={} -> {}",
                downloaded, cached, failed, dir.display());
            return Ok(());
        }
        Commands::View { .. } => unreachable!(),
        Commands::Web { .. } => unreachable!(),
        Commands::Single { file, json, tank_cache } => {
        // single：解析单个回放文件，打印 14 名玩家战绩
            let resolver = tank_cache
                .filter(|p| p.exists())
                .and_then(|p| TankResolver::load_from_json_file(&p).ok());
            let parser = if let Some(ref r) = resolver {
                crate::replay::parser::ReplayParser::with_resolver(r)
            } else {
                crate::replay::parser::ReplayParser::new()
            };
            let summary = parser.parse_file(&file)?;

            if json {
                let json = serde_json::to_string_pretty(&summary)?;
                println!("{}", json);
            } else {
                print_single_replay(&summary);
            }
        }
        Commands::Scan { dir, mode, days, output, tank_cache, fetch_tanks, app_id: _, server: _ } => {
        // scan：批量扫描目录，输出聚合报告（胜率/伤害/坦克/地图）
            // Load or fetch tank resolver
            let resolver = if fetch_tanks {
                eprintln!("Building tank resolver from local BlitzKit data...");
                match TankResolver::from_blitzkit() {
                    Ok(r) => {
                        eprintln!("Loaded {} tanks from BlitzKit", r.len());
                        Some(r)
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to build BlitzKit resolver: {}", e);
                        None
                    }
                }
            } else if let Some(ref cache_path) = tank_cache {
                if cache_path.exists() {
                    eprintln!("Loading tank cache from {}...", cache_path.display());
                    match TankResolver::load_from_json_file(cache_path) {
                        Ok(r) => {
                            eprintln!("Loaded {} tanks", r.len());
                            Some(r)
                        }
                        Err(e) => {
                            eprintln!("Warning: failed to load tank cache: {}", e);
                            None
                        }
                    }
                } else {
                    eprintln!("Tank cache not found: {}", cache_path.display());
                    None
                }
            } else {
                None
            };

            let scanner = if let Some(ref r) = resolver {
                ReplayScanner::with_resolver(r)
            } else {
                ReplayScanner::new()
            };

            // Build filter
            let filter = ScanFilter::from_mode(&mode, days);

            eprintln!("Scanning: {}", dir.display());
            eprintln!("Filter: mode={}, days={:?}", mode, days);
            eprintln!();

            let battles = scanner.scan_dir(&dir, &filter, |p| {
                let status = if p.ok { "OK" } else { "ERR" };
                let detail = p.error.as_deref().map(|e| format!(": {}", e)).unwrap_or_default();
                eprint!("\r[{}/{}] {} {}{}", p.current, p.total, status, p.file_name, detail);
                if p.current == p.total {
                    eprintln!();
                }
            })?;

            if battles.is_empty() {
                eprintln!("No replays matched the filter.");
                return Ok(());
            }

            let room_type = battles.first().map(|b| b.room_type.as_str()).unwrap_or("Unknown");
            let report = AggregatedReport::from_battles(&battles, room_type);

            if let Some(output) = output {
                let json = serde_json::to_string_pretty(&report)?;
                std::fs::write(&output, json)?;
                eprintln!("Report saved to: {}", output.display());
            }

            report.print_summary();
        }
        Commands::Snapshot { app_id, server, nickname, action, dir } => {
        // snapshot：定期采集 API 快照（take/list/diff），实现阶段性分析
            let store = SnapshotStore::new(&dir);
            
            match action.as_str() {
                "take" => {
                    let client = WgApiClient::new(&app_id, &server);
                    eprintln!("Fetching stats for '{}'...", nickname);
                    let results = client.search_player(&nickname, true)?;
                    if results.is_empty() {
                        anyhow::bail!("Player not found: {}", nickname);
                    }
                    let account_id = results[0].1;
                    let stats = client.get_player_stats(account_id)?;
                    let snapshot = crate::wargaming::snapshot::Snapshot::from_player_stats(stats);
                    let path = store.save(&snapshot)?;
                    eprintln!("Snapshot saved: {}", path.display());
                    eprintln!("  Player: {} (id={})", snapshot.player.nickname, snapshot.player.account_id);
                    eprintln!("  Rating battles: {}, WR: {:.1}%", 
                        snapshot.player.rating_battles,
                        if snapshot.player.rating_battles > 0 {
                            snapshot.player.rating_wins as f64 / snapshot.player.rating_battles as f64 * 100.0
                        } else { 0.0 });
                    eprintln!("  mm_rating: {:.2} (display {})", 
                        snapshot.player.rating_mm_rating.unwrap_or(0.0),
                        snapshot.player.rating_display_rating.unwrap_or(0));
                }
                "list" => {
                    let snapshots = store.list()?;
                    store.print_list(&snapshots);
                }
                "diff" => {
                    let snapshots = store.list()?;
                    if snapshots.len() < 2 {
                        eprintln!("Need at least 2 snapshots to diff (found {}).", snapshots.len());
                        eprintln!("Take more snapshots first with: snapshot <name> --action take");
                        return Ok(());
                    }
                    let from = &snapshots[0];
                    let to = &snapshots[snapshots.len() - 1];
                    let diff = store.diff(from, to);
                    store.print_diff(&diff);
                }
                _ => {
                    anyhow::bail!("Unknown action: {} (use: take, list, diff)", action);
                }
            }
        }
        Commands::Chat { config: config_path, save, load } => {
        // chat：启动交互式 Agent 对话（R5 会话 / R6 token 统计）
            // Set up Ctrl+C handler (R4: interrupt)
            let _ = ctrlc::set_handler(|| {
                crate::agent::set_interrupted();
                eprintln!("\n[Interrupted] Finishing current step...");
            });

            eprintln!("Loading config from {}...", config_path.display());
            let mut agent = Agent::new(&config_path)?;

            if let Some(ref load_path) = load {
                if load_path.exists() {
                    eprintln!("Loading session from {}...", load_path.display());
                    match agent.load_session(load_path) {
                        Ok(_) => eprintln!("Session loaded."),
                        Err(e) => eprintln!("Failed to load session: {}", e),
                    }
                }
            }

            eprintln!("Agent ready! Model: {} (context: {}, thinking: {})",
                agent.config.llm.model,
                agent.config.llm.context_length,
                agent.config.llm.thinking_mode);
            eprintln!("Replay dir: {}", agent.replay_dir);
            if let Some(ref tc) = agent.tank_cache {
                eprintln!("Tank cache: {}", tc.display());
            }
            eprintln!("Type 'exit' to quit, 'history' to view conversation, 'usage' to see token stats.");
            eprintln!("Press Ctrl+C to interrupt long-running tasks.");
            eprintln!();

            let stdin = io::stdin();
            loop {
                print!("> ");
                io::stdout().flush()?;

                let mut input = String::new();
                if stdin.lock().read_line(&mut input)? == 0 {
                    break;
                }
                let input = input.trim();

                if input.is_empty() {
                    continue;
                }
                if input == "exit" || input == "quit" {
                    break;
                }
                if input == "history" {
                    agent.print_history();
                    continue;
                }
                if input == "usage" {
                    agent.print_usage();
                    continue;
                }

                match agent.chat(input) {
                    Ok(response) => {
                        println!("\n{}\n", response);
                    }
                    Err(e) => {
                        eprintln!("Error: {}\n", e);
                    }
                }
            }

            if let Some(ref save_path) = save {
                match agent.save_session(save_path) {
                    Ok(_) => eprintln!("Session saved to {}", save_path.display()),
                    Err(e) => eprintln!("Failed to save session: {}", e),
                }
            }
            agent.save_usage();
            eprintln!("Session saved. Goodbye!");
        }
        Commands::Config { file, show } => {
        // config：查看/编辑 config.toml
            if show {
                let config = Config::load_or_create(&file)?;
                let toml = toml::to_string_pretty(&config)?;
                println!("# {}\n\n{}", file.display(), toml);
            } else {
                let config = Config::load_or_create(&file)?;
                eprintln!("Config file: {}", file.display());
                eprintln!("  WG API: server={}, app_id={}", config.wg_api.server, 
                    if config.wg_api.application_id.is_empty() { "(empty)" } else { "configured" });
                eprintln!("  LLM: model={}, endpoint={}", config.llm.model, config.llm.endpoint);
                eprintln!("  Replay dir: {}", config.replay.replay_dir);
                eprintln!();
                eprintln!("Edit {} to configure your settings.", file.display());
            }
        }
        Commands::Usage { file } => {
        // usage：打印 Token 用量汇总
            let usage = TokenUsage::load_from_file(&file)?;
            usage.print_summary();
        }
        Commands::Player { nickname, app_id, server } => {
        // player：查询玩家累计战绩（WG API）
            let client = WgApiClient::new(&app_id, &server);
            
            eprintln!("Searching for player '{}' on {}...", nickname, server);
            let results = client.search_player(&nickname, true)?;
            if results.is_empty() {
                eprintln!("Player not found. Trying partial match...");
                let partial = client.search_player(&nickname, false)?;
                if partial.is_empty() {
                    eprintln!("No players found.");
                    return Ok(());
                }
                eprintln!("Found {} partial matches:", partial.len());
                for (nick, id) in &partial {
                    eprintln!("  {} (id={})", nick, id);
                }
                return Ok(());
            }
            
            let (_, account_id) = &results[0];
            eprintln!("Found: {} (id={})", results[0].0, account_id);
            eprintln!("Fetching stats...");
            
            let stats = client.get_player_stats(*account_id)?;
            WgApiClient::print_stats(&stats);
        }
        Commands::Compare { nickname, dir, app_id, server, tank_cache, mode } => {
        // compare：扫描近期回放 vs API 累计，量化近期表现
            // 1. Fetch API stats
            let client = WgApiClient::new(&app_id, &server);
            eprintln!("Fetching API stats for '{}'...", nickname);
            let results = client.search_player(&nickname, true)?;
            if results.is_empty() {
                anyhow::bail!("Player not found: {}", nickname);
            }
            let account_id = results[0].1;
            let api_stats = client.get_player_stats(account_id)?;

            // 2. Scan replays
            let resolver = tank_cache
                .filter(|p| p.exists())
                .and_then(|p| TankResolver::load_from_json_file(&p).ok());
            let scanner = if let Some(ref r) = resolver {
                ReplayScanner::with_resolver(r)
            } else {
                ReplayScanner::new()
            };

            let filter = ScanFilter::from_mode(&mode, None);

            eprintln!("Scanning replays in {} (mode={})...", dir.display(), mode);
            let battles = scanner.scan_dir(&dir, &filter, |p| {
                let detail = p.error.as_deref().map(|e| format!(" ERR: {}", e)).unwrap_or_default();
                eprint!("\r[{}/{}] {}{}", p.current, p.total, p.file_name, detail);
                if p.current == p.total { eprintln!(); }
            })?;

            if battles.is_empty() {
                eprintln!("No replays matched.");
                return Ok(());
            }

            let room_type = battles.first().map(|b| b.room_type.as_str()).unwrap_or("Unknown");
            let report = AggregatedReport::from_battles(&battles, room_type);

            // 3. Compare
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

            println!();
            println!("========================================================");
            println!("  Replay vs API Comparison: {}", nickname);
            println!("========================================================");
            println!();
            println!("  {:<25} {:>12} {:>12} {:>10}", "Metric", "Replays", "API Total", "Replay %");
            println!("  {}", "-".repeat(62));

            let an = api_battles.max(1) as u64 as f64;

            let rows: Vec<(&str, f64, f64, bool)> = vec![
                ("Battles", report.total_battles as f64, api_battles as f64, false),
                ("Wins", report.wins as f64, api_wins as f64, false),
                ("Win rate %", report.win_rate, api_wins as f64 / an * 100.0, true),
                ("Avg damage", report.avg_damage, api_dmg as f64 / an, true),
                ("Avg frags", report.avg_frags, api_frags as f64 / an, true),
                ("Hit rate %", report.hit_rate, if api_shots > 0 { api_hits as f64 / api_shots as f64 * 100.0 } else { 0.0 }, true),
                ("Avg XP", report.avg_xp, if is_rating { api_stats.rating_xp as f64 / an } else { api_stats.random_xp as f64 / an }, true),
                ("Avg block", report.avg_damage_blocked, 0.0, true),
                ("Avg assisted", report.avg_assisted, 0.0, true),
            ];

            for (label, replay_val, api_val, is_avg) in &rows {
                let pct = if *api_val > 0.0 && *is_avg {
                    format!("{:+.1}%", (replay_val - api_val) / api_val * 100.0)
                } else if !*is_avg && *api_val > 0.0 {
                    format!("{:.1}%", replay_val / api_val * 100.0)
                } else {
                    "-".to_string()
                };
                let replay_str = if *label == "Win rate %" || *label == "Hit rate %" {
                    format!("{:.1}%", replay_val)
                } else {
                    format!("{:.0}", replay_val)
                };
                let api_str = if *label == "Win rate %" || *label == "Hit rate %" {
                    format!("{:.1}%", api_val)
                } else {
                    format!("{:.0}", api_val)
                };
                println!("  {:<25} {:>12} {:>12} {:>10}", label, replay_str, api_str, pct);
            }

            println!();
            println!("  Replay battles: {}  |  API total: {}  |  Coverage: {:.1}%",
                report.total_battles, api_battles,
                report.total_battles as f64 / api_battles.max(1) as f64 * 100.0);
            
            if is_rating {
                println!();
                println!("  --- Rating ---");
                println!("  Replay mm_rating: {:.2} -> {:.2} (delta {:+.2})",
                    report.rating_start.unwrap_or(0.0),
                    report.rating_end.unwrap_or(0.0),
                    report.rating_delta.unwrap_or(0.0));
                println!("  API mm_rating:     {:.2} (display {})",
                    api_stats.rating_mm_rating.unwrap_or(0.0),
                    api_stats.rating_display_rating.unwrap_or(0));
            }

            println!();
            println!("  --- Top 5 Tanks (Replay) ---");
            for t in report.tank_usage.iter().take(5) {
                println!("    {:<28} {:>3}b  WR={:.0}%  avg_dmg={:.0}",
                    t.tank_name, t.battles, t.win_rate, t.avg_damage);
            }

            println!();
            println!("========================================================");
        }
        Commands::Prematch { nicknames, file, replay, app_id, server } => {
        // prematch：对局前瞻——分析双方阵容强度、识别威胁与薄弱点
            let client = WgApiClient::new(&app_id, &server);

            // If a replay path is provided, extract both teams and analyze each separately.
            if let Some(ref rp) = replay {
                let resolver = TankResolver::load_from_json_file(&crate::data::data_path("tank_cache.json")).ok();
                let parser = match &resolver {
                    Some(r) => crate::replay::parser::ReplayParser::with_resolver(r),
                    None => crate::replay::parser::ReplayParser::new(),
                };
                let summary = parser.parse_file(rp)?;
                let mut team_a = Vec::new();
                let mut team_b = Vec::new();
                for p in &summary.players {
                    if p.team == 1 { team_a.push(p.nickname.clone()); }
                    else { team_b.push(p.nickname.clone()); }
                }
                eprintln!("\n=== 回放: {} ===", summary.file_name);

                // Analyze both lineups and compare.
                let (report_a, report_b) = {
                    let mut pa = Vec::new();
                    for n in &team_a {
                        eprintln!("查询我方玩家 '{}'...", n);
                        if let Ok(results) = client.search_player(n, true) {
                            if !results.is_empty() {
                                if let Ok(stats) = client.get_player_stats(results[0].1) { pa.push(stats); }
                            }
                        }
                    }
                    let mut pb = Vec::new();
                    for n in &team_b {
                        eprintln!("查询敌方玩家 '{}'...", n);
                        if let Ok(results) = client.search_player(n, true) {
                            if !results.is_empty() {
                                if let Ok(stats) = client.get_player_stats(results[0].1) { pb.push(stats); }
                            }
                        }
                    }
                    (
                        crate::wargaming::prematch::analyze_lineup(pa)?,
                        crate::wargaming::prematch::analyze_lineup(pb)?,
                    )
                };

                eprintln!("\n--- 我方阵容 ---");
                crate::wargaming::prematch::print_report(&report_a);
                eprintln!("\n--- 敌方阵容 ---");
                crate::wargaming::prematch::print_report(&report_b);

                // Overall comparison
                let diff = report_a.avg_damage - report_b.avg_damage;
                eprintln!("\n=== 阵容对比 ===");
                eprintln!("我方场均伤害 {:.0} vs 敌方 {:.0} ({:+.0})",
                    report_a.avg_damage, report_b.avg_damage, diff);
                eprintln!("我方平均胜率 {:.1}% vs 敌方 {:.1}%",
                    report_a.avg_win_rate, report_b.avg_win_rate);
                if diff > 0.0 {
                    eprintln!("我方伤害占优，可利用火力压制敌方弱点。");
                } else if diff < 0.0 {
                    eprintln!("敌方伤害占优，注意规避其高威胁玩家，优先集火其薄弱点。");
                } else {
                    eprintln!("双方火力相当，比拼战术配合与走位。");
                }
                return Ok(());
            }

            // Otherwise: resolve the player list from inline names or a file.
            let mut names: Vec<String> = Vec::new();
            if let Some(ref f) = file {
                let text = std::fs::read_to_string(f)?;
                for line in text.lines() {
                    let n = line.trim().trim_matches(|c| c == '"' || c == '\'');
                    if !n.is_empty() { names.push(n.to_string()); }
                }
            }
            if let Some(ref inline) = nicknames {
                for s in inline.split(',') {
                    let n = s.trim();
                    if !n.is_empty() { names.push(n.to_string()); }
                }
            }
            if names.is_empty() {
                anyhow::bail!("No nicknames provided. Pass them inline, via --file, or via --replay.");
            }

            let mut players = Vec::new();
            for name in &names {
                eprintln!("查询玩家 '{}'...", name);
                match client.search_player(name, true) {
                    Ok(results) if !results.is_empty() => {
                        let account_id = results[0].1;
                        match client.get_player_stats(account_id) {
                            Ok(stats) => players.push(stats),
                            Err(e) => eprintln!("  {} 查询失败: {}", name, e),
                        }
                    }
                    _ => eprintln!("  未找到玩家 '{}'", name),
                }
            }

            let report = crate::wargaming::prematch::analyze_lineup(players)?;
            crate::wargaming::prematch::print_report(&report);
        }
        Commands::FetchTanks { output } => {
        // fetch-tanks：从本地 BlitzKit 数据构建 tank_cache.json
            eprintln!("Building tank resolver from local BlitzKit data (no WG API)...");
            let resolver = TankResolver::from_blitzkit()?;
            eprintln!("Built {} tanks from BlitzKit pb data", resolver.len());
            resolver.save_to_json_file(&output)?;
            eprintln!("Saved to: {}", output.display());
        }
        Commands::Combat { file, json } => {
        // combat：解码单场数据包事件流 + 推断每发射击
            use wotbreplay_parser::replay::Replay;
            use std::fs::File;

            let mut replay = Replay::open(File::open(&file)?)
                .map_err(|e| anyhow::anyhow!("Failed to open replay: {}", e))?;

            let data = replay.read_data()
                .map_err(|e| anyhow::anyhow!("Failed to read data: {}", e))?;

            let raw_packets: Vec<(u32, f32, &[u8])> = data.packets.iter()
                .map(|pkt| {
                    let pkt_type = match &pkt.payload {
                        wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
                        wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                        wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
                    };
                    (pkt_type, pkt.clock_secs, &pkt.raw_payload[..])
                })
                .collect();

            let timeline = CombatTimeline::parse_packets(&raw_packets);

            if json {
                let json = serde_json::to_string_pretty(&timeline)?;
                println!("{}", json);
            } else {
                println!("\n========================================================");
                println!("  Combat Event Analysis: {}", file.display());
                println!("========================================================");
                timeline.print_timeline();

                // Infer and print shot events
                let author_eid = *timeline.entity_names.iter()
                    .find(|(eid, _)| {
                        timeline.events.iter().any(|e| 
                            e.entity_id == **eid && matches!(e.event_type, CombatEventType::DamageCounter { .. }))
                    })
                    .map(|(eid, _)| eid)
                    .unwrap_or(&0);

                let shots = timeline.infer_shots(author_eid);
                timeline.print_shots(&shots);
                println!("\n========================================================");
            }
        }
    }

    Ok(())
}

fn print_single_replay(summary: &crate::models::battle::BattleSummary) {
    println!();
    println!("========================================================");
    println!("  Single Replay: {}", summary.file_name);
    println!("========================================================");
    println!();
    println!("  Player:     {} (id={})", summary.author_nickname, summary.author_account_id);
    println!("  Tank:       {} (id={})", summary.author_tank_name, summary.author_tank_id);
    println!("  Map:        {} (id={:#06x})", summary.map_name, summary.map_id);
    println!("  Mode:       {:?}", summary.room_type);
    println!("  Duration:   {:.1}s", summary.battle_duration_secs);
    println!("  Time:       {}", summary.datetime);
    println!("  Winner:     Team {}", summary.winner_team);
    println!("  Author:     Team {} ({})", summary.author_team, if summary.author_won { "WON" } else { "LOST" });
    println!();

    let a = &summary.author;
    println!("--- Author Stats ---");
    println!("  HP left:       {} {}", a.hitpoints_left,
        if a.is_auto_destroyed { "(AUTO-DESTROYED)" } else { "" });
    println!("  Credits:       {}", a.total_credits);
    println!("  XP:            {}", a.total_xp);
    println!("  Shots/Hits:    {}/{} ({:.0}%)", a.n_shots, a.n_hits,
        if a.n_shots > 0 { a.n_hits as f64 / a.n_shots as f64 * 100.0 } else { 0.0 });
    println!("  Penetrations:  {}", a.n_penetrations);
    println!("  Splashes:      {}", a.n_splashes);
    println!("  Damage:        {}", a.damage_dealt);
    println!();

    println!("--- All Players ({} total) ---", summary.players.len());
    println!("  {:<3} {:<25} {:<5} {:<7} {:<6} {:>5} {:>5} {:>5} {:>7} {:>7} {:>5} {:>7}",
        "#", "Nickname", "Team", "Tank", "XP", "Shots", "Hits", "Pens", "Dmg", "Block", "Kills", "mmRat");
    println!("  {}", "-".repeat(95));
    for (i, p) in summary.players.iter().enumerate() {
        let is_author = p.account_id == summary.author_account_id;
        let marker = if is_author { "*" } else { " " };
        println!("{} {:<2} {:<25} {:<5} {:<7} {:<6} {:>5} {:>5} {:>5} {:>7} {:>7} {:>5} {:>7.1}",
            marker, i+1, p.nickname, p.team, p.tank_name, p.base_xp,
            p.n_shots, p.n_hits_dealt, p.n_penetrations_dealt,
            p.damage_dealt, p.damage_blocked, p.n_enemies_destroyed,
            p.mm_rating.unwrap_or(0.0));
    }
    println!("\n  (* = replay author)");
    println!("\n========================================================");
}
