//! 3D 装甲查看器入口（serve / start_viewer_server*）：组装路由（HTTP 面在
//! [`crate::web::assets`]）与领域数据（[`crate::wargaming::tank_configs`]），
//! 面向 CLI `view` 与 agent 工具 view_tank 的无头截图两类调用方。

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::routing::get;
use axum::Json;
use serde_json::json;

use crate::web::assets::wsl_ip;

use crate::wargaming::tank_configs::{resolve_config_index, resolve_shell_by_global_id};
use crate::wargaming::tank_resolver::TankResolver;
use crate::web::assets::build_viewer_router;

pub async fn serve(
    tank_resolver: TankResolver,
    tank_id: u32,
    shooter_id: Option<u32>,
) -> anyhow::Result<()> {
    let app = build_viewer_router(tank_resolver, tank_id, shooter_id, "");

    let addr = SocketAddr::from(([0, 0, 0, 0], 0));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    let url = format!("http://127.0.0.1:{}", local_addr.port());

    eprintln!("Server running at {}", url);
    eprintln!(
        "If browser doesn't open, try: http://localhost:{} or http://{}:{}",
        local_addr.port(),
        wsl_ip(),
        local_addr.port()
    );
    eprintln!("Opening browser...");

    if webbrowser::open(&url).is_err() {
        eprintln!("Please open {} in your browser manually.", url);
    }

    axum::serve(listener, app).await?;

    Ok(())
}

pub async fn start_viewer_server(
    tank_resolver: TankResolver,
    tank_id: u32,
    shooter_id: u32,
) -> anyhow::Result<u16> {
    start_viewer_server_with_data(tank_resolver, tank_id, Some(shooter_id), None).await
}

/// 为回放射击复现启动无头查看器：解析回放 → 挂载 /api/replay_shot →
/// (端口, shell 弹表下标, 目标实际配置下标, 射手实际配置下标)。shell 参数 =
/// 发射弹种 shell_id 在射手实际搭载配置（`&scfg=` 返回值 4）弹表中的确定性下标
/// （type=28 槽位快照存在切弹竞态，仅作兜底）；配置下标供 `&config=`/`&scfg=`
/// 选择目标/射手模型的炮塔/主炮变体与射手弹表。
pub async fn start_viewer_server_for_replay(
    replay_path: &std::path::Path,
    tank_resolver: TankResolver,
    shot_no: usize,
) -> anyhow::Result<(u16, Option<u32>, Option<usize>, Option<usize>)> {
    use wotbreplay_parser::replay::Replay;
    let mut replay = Replay::open(std::fs::File::open(replay_path)?)?;
    let meta = replay.read_meta().ok();
    let data = replay.read_data()?;
    let raw_packets: Vec<(u32, f32, &[u8])> = data
        .packets
        .iter()
        .map(|pkt| {
            let t = match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => {
                    *packet_type
                }
            };
            (t, pkt.clock_secs, &pkt.raw_payload[..])
        })
        .collect();

    // 作者昵称 = battle_results 权威来源（meta.json 非 UTF-8 时 read_meta 整体失败，不可依赖；
    // meta.player_name 仅作兜底）
    let br = replay.read_battle_results().ok();
    let author_nickname = br
        .as_ref()
        .map(crate::replay::combat::author_nick_from_battle_results)
        .or_else(|| meta.as_ref().map(|m| m.player_name.clone()))
        .unwrap_or_default();
    // 双方炮管俯仰的车型极限锚定表（昵称→俯角/仰角）——prop2 frac 比例解码用；
    // 实际搭载 comp blob 一并收集（锚定与每发配置下标共用）
    let valid_tanks: Vec<u32> = br
        .as_ref()
        .map(|br| br.player_results.iter().map(|pr| pr.info.tank_id).collect())
        .unwrap_or_default();
    let comps = crate::replay::playback::collect_comp_descriptors(&raw_packets, &valid_tanks);
    // 作者实体 = comps 条目 account_id 精确匹配（P3 定案，重名/匿名免疫），退回昵称匹配
    let author_player_eid = br
        .as_ref()
        .and_then(|br| {
            comps
                .values()
                .find(|c| c.account_id != 0 && c.account_id == br.author.account_id as u64)
                .map(|c| c.eid)
                .filter(|e| *e != 0)
        })
        .unwrap_or_else(|| {
            crate::replay::combat::resolve_author_player_eid_by_nick(&raw_packets, &author_nickname)
        });
    let pitch_limits = br
        .as_ref()
        .map(|br| tank_resolver.pitch_limits_from_battle_results(br, &comps))
        .unwrap_or_default();
    let tank_of = |nick: &str| -> Option<u32> {
        let br = br.as_ref()?;
        br.players
            .iter()
            .find(|p| p.info.nickname == nick)
            .and_then(|p| {
                br.player_results
                    .iter()
                    .find(|pr| pr.info.account_id == p.account_id)
            })
            .map(|pr| pr.info.tank_id)
    };
    // 身份联表 eid 主键（P3 定案）：comps 条目 eid → account_id → 花名册 tank_id；
    // 退回昵称匹配。cfg 同构（eid → comp，退昵称）+ tank 低 16 位护栏。
    let comp_by_eid: HashMap<u32, &crate::replay::playback::CompDescriptor> = comps
        .values()
        .filter(|c| c.eid != 0)
        .map(|c| (c.eid, c))
        .collect();
    let account_of_eid = |eid: u32| -> Option<u64> {
        comp_by_eid
            .get(&eid)
            .filter(|c| c.account_id != 0)
            .map(|c| c.account_id)
    };
    let tank_of_eid = |eid: u32, nick: &str| -> Option<u32> {
        let br = br.as_ref()?;
        account_of_eid(eid).and_then(|aid| {
            br.player_results
                .iter()
                .find(|pr| pr.info.account_id as u64 == aid)
                .map(|pr| pr.info.tank_id)
        }).or_else(|| tank_of(nick))
    };
    let mut replay_data = crate::replay::combat::extract_shot_replays_with_limits(
        &raw_packets,
        author_player_eid,
        &pitch_limits,
    )?;
    // 弹种回填：全局 shell_id → tanks.pb 原始弹种串（/api/replay_shot 透传给 3D 视图）
    crate::replay::loadout::ShellKindTable::from_tanks_pb().annotate(&mut replay_data);
    if replay_data.is_empty() {
        return Err(anyhow::anyhow!("No shot events detected in this replay."));
    }
    if shot_no == 0 || shot_no > replay_data.len() {
        return Err(anyhow::anyhow!(
            "shot {} out of range (1..={})",
            shot_no,
            replay_data.len()
        ));
    }
    let shot = &replay_data[shot_no - 1];
    let target_tank = shot
        .target_eid
        .and_then(|te| tank_of_eid(te, &shot.target_name))
        .or_else(|| tank_of(&shot.target_name));
    let shooter_tank = tank_of_eid(shot.shooter_eid, &shot.shooter_name)
        .or_else(|| tank_of(&shot.shooter_name))
        .or_else(|| meta.as_ref().map(|m| m.tank_id as u32).filter(|v| *v > 0));
    eprintln!("[replay_shot] shot={}_{} target_name={} target_tank={:?} shooter_tank={:?} target_ang={:?}",
        shot_no, shot.damage, shot.target_name, target_tank, shooter_tank, shot.target_ang);

    let viewed_tank = target_tank.or(shooter_tank).unwrap_or(0);
    // 实际搭载配置下标（目标/射手）：comp blob 精确对号（弹种/血量启发式证据已退役，
    // fail-closed）；comps 已在俯仰锚定表构建时收集
    let cfg_of_eid = |eid: u32, nick: &str, tank: u32| -> Option<usize> {
        if tank == 0 {
            return None;
        }
        let comp = comp_by_eid
            .get(&eid)
            .copied()
            .or_else(|| comps.get(nick))
            .filter(|c| (c.tank_id & 0xFFFF) == (tank & 0xFFFF))
            .map(|c| (c.turret_local, c.gun_local));
        resolve_config_index(tank, comp).map(|(idx, _, _)| idx)
    };
    let viewed_cfg = if let Some(tt) = target_tank {
        shot.target_eid
            .and_then(|te| cfg_of_eid(te, &shot.target_name, tt))
            .or_else(|| cfg_of_eid(0, &shot.target_name, tt))
    } else {
        shooter_tank.and_then(|st| cfg_of_eid(shot.shooter_eid, &shot.shooter_name, st))
    };
    // 逐发注入：与类型化 replay_data 按下标对齐（serde_json::to_value 保序），
    // 射手/目标联表走 eid 主键
    let mut replay_json = serde_json::to_value(&replay_data)?;
    if let Some(arr) = replay_json.as_array_mut() {
        for (typed, s) in replay_data.iter().zip(arr.iter_mut()) {
            let shooter_cfg = tank_of_eid(typed.shooter_eid, &typed.shooter_name)
                .and_then(|t| cfg_of_eid(typed.shooter_eid, &typed.shooter_name, t));
            if let Some(idx) = shooter_cfg {
                s["shooter_config_idx"] = json!(idx);
            }
            let target_cfg = typed
                .target_eid
                .and_then(|te| {
                    tank_of_eid(te, &typed.target_name)
                        .and_then(|t| cfg_of_eid(te, &typed.target_name, t))
                })
                .or_else(|| {
                    tank_of(&typed.target_name).and_then(|t| cfg_of_eid(0, &typed.target_name, t))
                });
            if let Some(idx) = target_cfg {
                s["target_config_idx"] = json!(idx);
            }
            // 发射弹种解析注入（按射手实际搭载配置弹表；与 Web /api/replay/shots 同构）：
            // shell 数据 + cfg 域钉死的弹下标
            if let Some(st) = tank_of_eid(typed.shooter_eid, &typed.shooter_name) {
                if let Some((ci, si, sh)) = crate::wargaming::tank_configs::resolve_shell_by_global_id(
                    st,
                    typed.shell_id,
                    shooter_cfg,
                ) {
                    s["shooter_shell_cfg_idx"] = json!(ci);
                    s["shooter_shell_idx"] = json!(si);
                    s["shell"] = sh;
                }
            }
        }
    }
    eprintln!(
        "[replay_shot] 配置下标注入完成（shot={}，viewed_cfg={:?}）",
        shot_no, viewed_cfg
    );
    // URL 的 shell 参数 = 发射弹种在射手实际搭载配置弹表中的下标（scfg 域，
    // 与 3D 端 loadShooter 按 &scfg= 选定的弹表同域）。弹种链解析失败 → None：
    // type=28 槽位快照存在切弹竞态，不再作兜底（fail-closed，禁猜）。
    let (shell_for_url, shooter_shell_cfg) = shooter_tank
        .and_then(|st| {
            let scfg = cfg_of_eid(shot.shooter_eid, &shot.shooter_name, st);
            resolve_shell_by_global_id(st, shot.shell_id, scfg)
                .map(|(ci, si, _)| (Some(si as u32), Some(ci)))
        })
        .unwrap_or((None, None));
    let port =
        start_viewer_server_with_data(tank_resolver, viewed_tank, shooter_tank, Some(replay_json))
            .await?;
    Ok((port, shell_for_url, viewed_cfg, shooter_shell_cfg))
}

pub async fn start_viewer_server_with_data(
    tank_resolver: TankResolver,
    tank_id: u32,
    shooter_id: Option<u32>,
    replay_shots: Option<serde_json::Value>,
) -> anyhow::Result<u16> {
    let mut app = build_viewer_router(tank_resolver, tank_id, shooter_id, "");
    if let Some(shots) = replay_shots {
        app = app.route(
            "/api/replay_shot",
            get(move || {
                let shots = shots.clone();
                async move { Json(shots) }
            }),
        );
    }
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    eprintln!(
        "[viewer] serving tank {} (shooter {:?}) on http://127.0.0.1:{} (headless)",
        tank_id, shooter_id, port
    );
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(port)
}
