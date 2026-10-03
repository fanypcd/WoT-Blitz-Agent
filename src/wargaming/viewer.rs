//! 3D 装甲查看器入口（serve / start_viewer_server*）：组装路由（HTTP 面在
//! [`crate::web::assets`]）与领域数据（[`crate::wargaming::tank_configs`]），
//! 面向 CLI `view` 与 agent 工具 view_tank 的无头截图两类调用方。

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
) -> anyhow::Result<(u16, u32, Option<usize>, Option<usize>)> {
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

    let timeline = crate::replay::combat::CombatTimeline::parse_packets(&raw_packets);
    // 先把带 DamageCounter 事件的 entity_id 收进 HashSet，避免对每个候选实体
    // 重新全量扫描 events（O(N×M) → O(N+M)）
    let dmg_counter_eids: std::collections::HashSet<u32> = timeline
        .events
        .iter()
        .filter(|e| {
            matches!(
                e.event_type,
                crate::replay::combat::CombatEventType::DamageCounter { .. }
            )
        })
        .map(|e| e.entity_id)
        .collect();
    let author_eid = *timeline
        .entity_names
        .iter()
        .find(|(eid, _)| dmg_counter_eids.contains(eid))
        .map(|(eid, _)| eid)
        .unwrap_or(&0);
    let shots = timeline.infer_shots(author_eid);
    if shots.is_empty() {
        return Err(anyhow::anyhow!("No shot events detected in this replay."));
    }
    // 作者昵称 = battle_results 权威来源（meta.json 非 UTF-8 时 read_meta 整体失败，不可依赖；
    // meta.player_name 仅作兜底）
    let br = replay.read_battle_results().ok();
    let author_nickname = br
        .as_ref()
        .map(crate::replay::combat::author_nick_from_battle_results)
        .or_else(|| meta.as_ref().map(|m| m.player_name.clone()))
        .unwrap_or_default();
    let author_player_eid =
        crate::replay::combat::resolve_author_player_eid_by_nick(&raw_packets, &author_nickname);
    // 双方炮管俯仰的车型极限锚定表（昵称→俯角/仰角）——prop2 frac 比例解码用；
    // 实际搭载 comp blob 一并收集（锚定与每发配置下标共用）
    let valid_tanks: Vec<u32> = br
        .as_ref()
        .map(|br| br.player_results.iter().map(|pr| pr.info.tank_id).collect())
        .unwrap_or_default();
    let comps = crate::replay::playback::collect_comp_descriptors(&raw_packets, &valid_tanks);
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
    let mut replay_data = crate::replay::combat::extract_shot_replays_with_limits(
        &raw_packets,
        author_player_eid,
        &pitch_limits,
    )?;
    // 弹种回填：全局 shell_id → tanks.pb 原始弹种串（/api/replay_shot 透传给 3D 视图）
    crate::replay::loadout::ShellKindTable::from_tanks_pb().annotate(&mut replay_data);
    if shot_no == 0 || shot_no > replay_data.len() {
        return Err(anyhow::anyhow!(
            "shot {} out of range (1..={})",
            shot_no,
            replay_data.len()
        ));
    }
    let shot = &replay_data[shot_no - 1];
    let target_tank = tank_of(&shot.target_name);
    let author_nickname = meta
        .as_ref()
        .map(|m| m.player_name.clone())
        .unwrap_or_default();
    let shooter_tank = tank_of(&author_nickname)
        .or_else(|| meta.as_ref().map(|m| m.tank_id as u32).filter(|v| *v > 0));
    eprintln!("[replay_shot] shot={}_{} target_name={} target_tank={:?} shooter_tank={:?} target_ang={:?}",
        shot_no, shot.damage, shot.target_name, target_tank, shooter_tank, shot.target_ang);

    let viewed_tank = target_tank.or(shooter_tank).unwrap_or(0);
    let shell_slot = shot.shell_slot;
    // 实际搭载配置下标（目标/射手）：comp blob → 发射弹种 → 初始血量 证据链，注入每发数据
    //（comps 已在俯仰锚定表构建时收集）
    let initial_hp_all = crate::replay::combat::collect_initial_hp(&raw_packets);
    let mut player_shells: std::collections::HashMap<String, Vec<u32>> =
        std::collections::HashMap::new();
    for s in &replay_data {
        if s.shell_id == 0 {
            continue;
        }
        let v = player_shells.entry(s.shooter_name.clone()).or_default();
        if !v.contains(&s.shell_id) {
            v.push(s.shell_id);
        }
    }
    let mut nick_hp: std::collections::HashMap<String, u16> = std::collections::HashMap::new();
    for (eid, nick) in &timeline.entity_names {
        if let Some((_, hp)) = initial_hp_all.get(eid) {
            nick_hp.insert(nick.clone(), *hp);
        }
    }
    let cfg_of = |nick: &str, tank: u32| -> Option<usize> {
        if tank == 0 {
            return None;
        }
        let comp = comps.get(nick).and_then(|c| {
            ((c.tank_id & 0xFFFF) == (tank & 0xFFFF)).then_some((c.turret_local, c.gun_local))
        });
        let shells = player_shells.get(nick).map(|v| v.as_slice()).unwrap_or(&[]);
        let hp = nick_hp.get(nick).copied().unwrap_or(0);
        resolve_config_index(tank, comp, shells, hp).map(|(idx, _, _)| idx)
    };
    let viewed_cfg = if let Some(tt) = target_tank {
        cfg_of(&shot.target_name, tt)
    } else {
        shooter_tank.and_then(|st| cfg_of(&author_nickname, st))
    };
    let mut replay_json = serde_json::to_value(&replay_data)?;
    if let Some(arr) = replay_json.as_array_mut() {
        for s in arr.iter_mut() {
            let shooter_name = s["shooter_name"].as_str().unwrap_or("").to_string();
            let target_name = s["target_name"].as_str().unwrap_or("").to_string();
            let shooter_cfg = tank_of(&shooter_name).and_then(|t| cfg_of(&shooter_name, t));
            if let Some(idx) = shooter_cfg {
                s["shooter_config_idx"] = json!(idx);
            }
            if let Some(idx) = tank_of(&target_name).and_then(|t| cfg_of(&target_name, t)) {
                s["target_config_idx"] = json!(idx);
            }
            // 发射弹种解析注入（按射手实际搭载配置弹表；与 Web /api/replay/shots 同构）：
            // shell 数据 + cfg 域钉死的弹下标
            let shell_id = s["shell_id"].as_u64().unwrap_or(0) as u32;
            if let Some(st) = tank_of(&shooter_name) {
                if let Some((ci, si, sh)) =
                    crate::wargaming::tank_configs::resolve_shell_by_global_id(
                        st,
                        shell_id,
                        shooter_cfg,
                    )
                {
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
    // 与 3D 端 loadShooter 按 &scfg= 选定的弹表同域）；槽位仅作完全兜底
    let (shell_for_url, shooter_shell_cfg) = shooter_tank
        .and_then(|st| {
            let scfg = cfg_of(&shot.shooter_name, st);
            resolve_shell_by_global_id(st, shot.shell_id, scfg)
                .map(|(ci, si, _)| (si as u32, Some(ci)))
        })
        .unwrap_or((shell_slot, None));
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
