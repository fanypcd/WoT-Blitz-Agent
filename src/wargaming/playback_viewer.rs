//! 全场实时回放播放器：Three.js 战术场景（低模标记 + 可选 GLB 真实车模），
//! 数据来自 [`crate::replay::playback`] 的 0.1s 网格时间线（`/api/playback/data`）。
//!
//! 与 3D 装甲查看器（viewer.rs）共享的约定：
//! - Three.js 离线 vendor（web/vendor/three），importmap 按 base_prefix 拼接；
//! - 游戏系 → 场景系 = x 取负、yaw 取负（镜像，viewer world=1 同款；pitch 不取反）；
//! - 低模局部 forward = +Z（BW yaw = atan2(x,z)，推导与 negX/negAng 自洽）；
//! - GLB 姿态与 viewer world 模式同构：根 = poseFromYPR(−yaw, pitch, 0)
//!   （qFrame = Ry(π)·Rx(−π/2) 的 z-up→y-up 帧变换，GLB 内部系 x右/y前/z上）；
//!   炮塔/炮管 = 烘焙矩阵绕 models.pb 原点链枢轴旋转（turret Rz(−rel) / gun Rx(俯仰)），
//!   部件数据来自 `/api/tank/{id}`（standalone 挂 viewer::tank_data_handler）。
//!
//! 坐标换算（前端 applyPose）：
//!   pos = (−x, y, z)；hull.rotation = (pitch, −yaw, 0) 'YXZ'；
//!   turret.rotation.y = −(turret_abs − hull_yaw)；gunPivot.rotation.x = −gun_pitch。
//!   hull_yaw/turret_yaw 为后端解卷绕连续域（可超 ±π），直接线性插值即物理正确。

use crate::wargaming::tank_resolver::TankResolver;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// 数据响应缓存上限（gzip 后的完整 JSON，按回放绝对路径键）
const CACHE_MAX: usize = 4;

// ---------- 数据构建 + 缓存 ----------

type Cache = Mutex<Vec<(String, Arc<Vec<u8>>)>>;
static CACHE: OnceLock<Cache> = OnceLock::new();
/// gzip 响应缓存（与 JSON 缓存同键）。
/// 值为 Arc<Bytes>（克隆 = 引用计数，跨请求零拷贝；Body 本身非 Sync 不能入 static）
type GzCacheEntry = (String, Arc<axum::body::Bytes>);
static GZ_BYTES_CACHE: OnceLock<Mutex<Vec<GzCacheEntry>>> = OnceLock::new();

fn cache() -> &'static Cache {
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn cache_put(c: &'static Cache, key: String, v: Arc<Vec<u8>>) {
    let mut c = c.lock().unwrap();
    c.insert(0, (key, v));
    c.truncate(CACHE_MAX);
}

/// 解析回放 → PlaybackData → 未压缩 JSON 字节（缓存命中直接返回）。
/// TankResolver 由调用方注入（web = AppState mtime 缓存实例；standalone = 自建；
/// None 回退进程级 GLOBAL_RESOLVER）。
pub fn build_playback_json(
    path: &Path,
    resolver: Option<Arc<TankResolver>>,
) -> anyhow::Result<Arc<Vec<u8>>> {
    let key = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    if let Some((_, v)) = cache().lock().unwrap().iter().find(|(k, _)| k == &key) {
        return Ok(v.clone());
    }
    let json = build_playback_json_uncached(path, &key, resolver)?;
    let arc = Arc::new(json);
    cache_put(cache(), key, arc.clone());
    Ok(arc)
}

fn build_playback_json_uncached(
    path: &Path,
    key: &str,
    resolver: Option<Arc<TankResolver>>,
) -> anyhow::Result<Vec<u8>> {
    use wotbreplay_parser::replay::Replay;

    let mut replay = Replay::open(std::fs::File::open(path)?)?;
    let meta = replay.read_meta().ok();
    let data = replay.read_data()?;
    let packets: Vec<(u32, f32, &[u8])> = data
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

    let br = replay.read_battle_results().ok();
    let resolver = resolver.unwrap_or_else(crate::wargaming::tank_configs::global_resolver);
    // 实际搭载 comp blob（俯仰锚定与变体标注共用一份收集）
    let valid_tanks: Vec<u32> = br
        .as_ref()
        .map(|br| br.player_results.iter().map(|pr| pr.info.tank_id).collect())
        .unwrap_or_default();
    let comps = crate::replay::playback::collect_comp_descriptors(&packets, &valid_tanks);
    let pitch_limits = br
        .as_ref()
        .map(|br| resolver.pitch_limits_from_battle_results(br, &comps))
        .unwrap_or_default();

    // battle_results → 玩家联表（昵称/队伍 × tank_id）+ 坦克名
    let winner_team = br
        .as_ref()
        .and_then(|br| br.winner_team_number.as_ref())
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
    let mut players = Vec::new();
    let mut tank_names = HashMap::new();
    if let Some(br) = br.as_ref() {
        for pr in &br.player_results {
            let joined = br
                .players
                .iter()
                .find(|p| p.account_id == pr.info.account_id);
            let tank_id = pr.info.tank_id;
            players.push(crate::replay::playback::PlaybackPlayer {
                account_id: pr.info.account_id,
                nickname: joined.map(|p| p.info.nickname.clone()).unwrap_or_default(),
                team: joined
                    .map(|p| if p.info.team == 1 { 1u8 } else { 2u8 })
                    .unwrap_or(0),
                tank_id,
            });
            if let Some(name) = resolver.resolve(tank_id) {
                tank_names.insert(tank_id, name);
            }
        }
    }
    let author_account_id = br.as_ref().map(|b| b.author.account_id).unwrap_or(0);
    let map_id = br.as_ref().map(|b| b.mode_map_id & 0xFFFF).unwrap_or(0);
    // 显示名以客户端注册表为准（wotbreplay-parser 的 MapId 枚举个别判别值与
    // 客户端数据不一致，见 map_assets 模块注释）；无注册表时退回解析器名
    let map_name = crate::wargaming::map_assets::display_name(map_id)
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            meta.as_ref()
                .map(|m| format!("{:?}", m.map_id))
                .unwrap_or_else(|| format!("map_{map_id}"))
        });

    eprintln!("[playback] 构建全场时间线: {key}");
    let input = crate::replay::playback::PlaybackInput {
        packets: &packets,
        players,
        author_account_id,
        winner_team,
        map_id,
        map_name,
        pitch_limits: &pitch_limits,
        tank_names,
    };
    let mut pb = crate::replay::playback::build_playback_data(&input)?;
    // 实际搭载：comp blob（确定性）优先，弹种/血量推断回退（comps 已在锚定表构建时收集）
    if !comps.is_empty() {
        eprintln!(
            "[playback] comp 描述符: {} 条（updateArena subtype 1）",
            comps.len()
        );
    }
    annotate_vehicle_configs(&mut pb, &comps);
    eprintln!(
        "[playback] 完成: {} 车 / {} 发 / {:.0}s",
        pb.vehicles.len(),
        pb.shots.len(),
        pb.meta.duration
    );
    let json = serde_json::to_vec(&pb)?;
    Ok(json)
}

// ---------- 实际搭载配置推断（GLB 炮塔/主炮变体选择） ----------
//
// 回放不含直接的模块 id，但有两个可靠证据：
// 1. 发射弹种（shell_id 全局 id）：不同主炮弹表不同——已观测弹种必须 ⊆ 该炮弹表；
// 2. 初始血量：总 HP = 车体 health + 炮塔 health（tanks.pb），装备"改进耐久"= ×1.125。
// 弹种证据缺失（未开炮）时按血量；再缺失取顶级配置。多匹配取最后一档。

fn annotate_vehicle_configs(
    pb: &mut crate::replay::playback::PlaybackData,
    comps: &HashMap<String, crate::replay::playback::CompDescriptor>,
) {
    annotate_vehicle_config_slice(&mut pb.vehicles, comps);
}

fn annotate_vehicle_config_slice(
    vehicles: &mut [crate::replay::playback::VehicleTrack],
    comps: &HashMap<String, crate::replay::playback::CompDescriptor>,
) {
    // 解析结果 = (build_configs 数组下标, turret_index, gun_index)；None = 证据不足
    type ResolvedConfig = (u32, u32, u32);
    type CompCache = HashMap<(u32, Option<(u16, u16)>), Option<ResolvedConfig>>;
    // 实际搭载解析统一走 tank_configs::resolve_config_index（comp blob 精确对号，
    // 与射击复现共享同一实现；旧弹种/血量启发式证据已退役）；返回 (build_configs
    // 数组下标, turret_index, gun_index)。config_idx 与 shots 的 shooter_config_idx
    // 同域；burst_size 直接给出实际搭载主炮的弹夹容量（装填条弹容 N 的权威值）。
    // 缓存键 = (tank_id, comp 局部 id 对)：同 tank_id 的不同玩家可搭载不同配置
    //（实测多炮车 52 台），逐车对号。
    let mut cache: CompCache = HashMap::new();
    // comp 匹配 = eid 优先（条目 field1 精确键，P3 定案），退回昵称键
    let comp_by_eid: HashMap<u32, &crate::replay::playback::CompDescriptor> = comps
        .values()
        .filter(|c| c.eid != 0)
        .map(|c| (c.eid, c))
        .collect();
    for v in vehicles.iter_mut() {
        if v.tank_id == 0 {
            continue;
        }
        let comp = comp_by_eid
            .get(&v.eid)
            .copied()
            .or_else(|| comps.get(&v.nickname))
            .and_then(|c| {
                ((c.tank_id & 0xFFFF) == (v.tank_id & 0xFFFF)).then_some((c.turret_local, c.gun_local))
            });
        let resolved = *cache
            .entry((v.tank_id, comp))
            .or_insert_with(|| {
                crate::wargaming::tank_configs::resolve_config_index(v.tank_id, comp)
                    .map(|(ci, ti, gi)| (ci as u32, ti, gi))
            });
        // 弹夹容量随配置走：命中 → 该配置的 burst_size；configs 唯一（无歧义）→ 该唯一配置；
        // 其余（坦克数据缺失/comp 未命中）保持 None，消费端按单发处理、不猜。
        let configs = crate::wargaming::tank_configs::build_configs(v.tank_id);
        let burst_at = |i: usize| {
            configs.get(i).and_then(|c| c.get("burst_size")).and_then(|b| b.as_f64()).map(|b| b.round() as u32)
        };
        if let Some((ci, ti, gi)) = resolved {
            v.config_idx = Some(ci);
            v.turret_index = Some(ti);
            v.gun_index = Some(gi);
            v.burst_size = burst_at(ci as usize);
        } else if configs.len() == 1 {
            v.burst_size = burst_at(0);
        }
    }
    // comp blob 局部 id 透传（纯回放证据）：消费端（含 WASM 客户端路径）联表
    // configs[].turret_local/gun_local 即可钉定实际搭载配置
    crate::replay::playback::annotate_vehicle_comp_locals(vehicles, comps);
}

fn gzip_bytes(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

/// 数据响应：gzip JSON（浏览器 fetch 按 Content-Encoding 透明解压）。
/// 全场时间线构建（多 MB JSON）与 gzip 压缩都是 CPU 重活——整体 spawn_blocking
/// （map/terrain/groundtex 系列同此纪律）。
pub async fn playback_data_response(
    path: &Path,
    resolver: Option<std::sync::Arc<TankResolver>>,
) -> Response {
    let path = path.to_path_buf();
    match tokio::task::spawn_blocking(move || playback_gzip_blocking(&path, resolver)).await {
        Ok(Ok(gz)) => (
            [
                (axum::http::header::CONTENT_TYPE, "application/json"),
                (axum::http::header::CONTENT_ENCODING, "gzip"),
            ],
            gz,
        )
            .into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("回放数据构建失败: {e:?}"),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("playback task failed: {e}"),
        )
            .into_response(),
    }
}

/// [`playback_data_response`] 的阻塞实现：JSON 构建（自带缓存）→ gzip（结果缓存）。
fn playback_gzip_blocking(
    path: &Path,
    resolver: Option<std::sync::Arc<TankResolver>>,
) -> anyhow::Result<axum::body::Bytes> {
    let key = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string();
    if let Some((_, b)) = GZ_BYTES_CACHE
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap()
        .iter()
        .find(|(k, _)| k == &key)
    {
        return Ok((**b).clone());
    }
    let json = build_playback_json(path, resolver)?;
    let gz = gzip_bytes(&json);
    // Bytes::from(Vec) 取所有权零拷贝；Bytes 克隆共享底层缓冲
    let bytes = Arc::new(axum::body::Bytes::from(gz));
    let out = (*bytes).clone();
    let mut c = GZ_BYTES_CACHE
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap();
    c.insert(0, (key, bytes));
    c.truncate(CACHE_MAX);
    Ok(out)
}

/// POST /api/playback/data  body {file: "路径"}（与 /api/replay/shots 同款路径参数约定）
pub async fn playback_data_handler(Json(body): Json<serde_json::Value>) -> Response {
    let file = body["file"].as_str().unwrap_or("").trim().to_string();
    if file.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing file").into_response();
    }
    playback_data_response(Path::new(&file), None).await
}

/// 地图参数：?id=<回放数字 id>（首选，与客户端 arenaTypeID 同链）或 ?name=<显示名|键>。
fn map_query_param(q: &HashMap<String, String>) -> String {
    if let Some(id) = q.get("id").filter(|s| !s.trim().is_empty()) {
        return id.trim().to_string();
    }
    q.get("name").cloned().unwrap_or_default()
}

/// GET /api/playback/map?id=19 —— 地图底图（提取/覆盖/缓存链路见
/// [`crate::wargaming::map_assets`]；不可用时仍 404，前端回退程序生成网格）。
/// `&res=mini` 伺服客户端小地图（低画质档地面：缓存→客户端提取→高清兜底）
pub async fn playback_map_handler(
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let map = map_query_param(&q);
    let mini = q.get("res").map(|s| s.as_str()) == Some("mini");
    tokio::task::spawn_blocking(move || {
        if mini {
            crate::wargaming::map_assets::map_minimap_response(&map)
        } else {
            crate::wargaming::map_assets::map_image_response(&map)
        }
    })
    .await
    .unwrap_or_else(|_| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "map task failed",
        )
            .into_response()
    })
}

/// GET /api/playback/terrain?id=19 —— 高度场地形（u16 LE + X-Terrain-Meta；
/// 不可用 404，前端回退 2D 底图平面）
pub async fn playback_terrain_handler(
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let map = map_query_param(&q);
    tokio::task::spawn_blocking(move || crate::wargaming::map_assets::terrain_response(&map))
        .await
        .unwrap_or_else(|_| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "terrain task failed",
            )
                .into_response()
        })
}

/// GET /api/playback/groundmeta?id=19 —— 地表分层合成参数（缺失 404，
/// 前端回退整图烘焙）
pub async fn playback_groundmeta_handler(
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let map = map_query_param(&q);
    tokio::task::spawn_blocking(move || {
        crate::wargaming::map_assets::ground_layers_meta_response(&map)
    })
    .await
    .unwrap_or_else(|_| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ground meta task failed",
        )
            .into_response()
    })
}

/// GET /api/playback/groundtex?id=19&k=cm|tile|mask|hmap —— 地表分层贴图
pub async fn playback_groundtex_handler(
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let map = map_query_param(&q);
    let layer = q.get("k").cloned().unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        crate::wargaming::map_assets::ground_layer_response(&map, &layer)
    })
    .await
    .unwrap_or_else(|_| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "ground tex task failed",
        )
            .into_response()
    })
}

/// GET /api/playback/scenery?id=19 —— 静态场景 GLB（客户端管线离线导出；
/// 缺失 404，前端静默跳过）
pub async fn playback_scenery_handler(
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let map = map_query_param(&q);
    tokio::task::spawn_blocking(move || crate::wargaming::map_assets::scenery_response(&map))
        .await
        .unwrap_or_else(|_| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "scenery task failed",
            )
                .into_response()
        })
}

// 播放器页面为 Vue SPA（/playback → crate::web::spa_index_handler，
// vue-router 路由 PlaybackView + scene/playbackScene.js 场景内核）。

// ---------- 独立服务（CLI `playback <replay>`） ----------

pub async fn serve_standalone(replay_path: &Path) -> anyhow::Result<()> {
    // 全局 resolver（GLB 车模名/俯仰极限表用）
    let resolver = crate::wargaming::tank_resolver::TankResolver::load_from_json_file(
        crate::data::data_path("tank_cache.json").as_path(),
    )
    .unwrap_or_default();
    crate::wargaming::tank_configs::set_global_resolver(resolver.clone());

    // 预热缓存（启动即构建，首开页面零等待；失败不退出——页面仍可显示错误）
    if let Err(e) = build_playback_json(replay_path, Some(Arc::new(resolver))) {
        eprintln!("[playback] 预构建失败（页面请求时将重试）: {e:?}");
    }

    // 页面为 Vue SPA（crate::web 嵌入产物）；根路径重定向到 /playback 路由
    let app = Router::new()
        .route(
            "/",
            get(|| async { axum::response::Redirect::temporary("/playback") }),
        )
        .route(
            "/playback",
            get(|| async { crate::web::spa_index_response() }),
        )
        .route(
            "/assets/{*path}",
            get(
                |axum::extract::Path(path): axum::extract::Path<String>| async move {
                    crate::web::spa_asset_response(&path)
                },
            ),
        )
        .route("/api/playback/data", post(playback_data_handler))
        .route("/api/playback/map", get(playback_map_handler))
        .route("/api/playback/terrain", get(playback_terrain_handler))
        .route("/api/playback/scenery", get(playback_scenery_handler))
        .route("/api/playback/groundmeta", get(playback_groundmeta_handler))
        .route("/api/playback/groundtex", get(playback_groundtex_handler))
        .route(
            "/api/tank/{tank_id}",
            get(crate::web::assets::tank_data_handler),
        )
        .route(
            "/glb/{tank_id}/{filename}",
            get(crate::web::assets::glb_handler),
        )
        .with_state(());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 0));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();
    let url = format!("http://127.0.0.1:{port}");
    eprintln!("Playback viewer running at {url}");
    if webbrowser::open(&url).is_err() {
        eprintln!("Please open {url} in your browser manually.");
    }
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod config_idx_tests {
    use super::*;
    use crate::replay::playback::{CompDescriptor, VehicleTrack};

    fn vt(tank_id: u32, nickname: &str) -> VehicleTrack {
        VehicleTrack {
            eid: 0,
            account_id: 0,
            nickname: nickname.to_string(),
            tank_id,
            tank_name: String::new(),
            team: 1,
            is_author: false,
            max_hp: 0,
            pos: Vec::new(),
            hull_yaw: Vec::new(),
            hull_pitch: Vec::new(),
            hull_roll: Vec::new(),
            turret_yaw: Vec::new(),
            gun_pitch: Vec::new(),
            hp: Vec::new(),
            death_t: None,
            killer_eid: 0,
            shell_ids: Vec::new(),
            loadout_items: Vec::new(),
            equipment: None,
            turret_index: None,
            gun_index: None,
            config_idx: None,
            burst_size: None,
            turret_local: None,
            gun_local: None,
            coverage: Vec::new(),
            pose_kf: None,
        }
    }

    fn comps(entries: &[(&str, u32, u16, u16)]) -> HashMap<String, CompDescriptor> {
        entries
            .iter()
            .map(|(nick, tid, tl, gl)| {
                (
                    nick.to_string(),
                    CompDescriptor {
                        nickname: nick.to_string(),
                        eid: 0,
                        account_id: 0,
                        tank_id: *tid,
                        turret_local: *tl,
                        gun_local: *gl,
                    },
                )
            })
            .collect()
    }

    /// T69（14625，两门弹夹炮 4 发/3 发）：comp blob 逐车钉定实际搭载配置——
    /// 装填条弹容 N = configs[config_idx].burst_size。缓存 key 必须含 comp：
    /// 同 tank_id 的两台车搭载不同炮时不得互相串值（旧行为按 tank_id 缓存会串）。
    #[test]
    fn config_idx_pins_mounted_gun_per_vehicle() {
        let tank_id = 14625;
        let configs = crate::wargaming::tank_configs::build_configs(tank_id);
        assert!(configs.len() >= 2, "测试前提：T69 多配置");
        let local = |i: usize| {
            (
                configs[i]["turret_local"].as_u64().unwrap() as u16,
                configs[i]["gun_local"].as_u64().unwrap() as u16,
            )
        };
        let bs = |i: usize| configs[i]["burst_size"].as_f64().unwrap_or(0.0);
        assert_ne!(bs(0), bs(1), "测试前提：两炮弹容不同");

        let (t0, g0) = local(0);
        let (t1, g1) = local(1);
        // 两个车辆顺序：结果必须只随各车自己的 comp 走（缓存键回归——旧代码按
        // tank_id 缓存，后到的车会复用先到车的配置）
        for vehicles in [
            vec![vt(tank_id, "A"), vt(tank_id, "B")],
            vec![vt(tank_id, "B"), vt(tank_id, "A")],
        ] {
            let mut vehicles = vehicles;
            annotate_vehicle_config_slice(
                &mut vehicles,
                &comps(&[("A", tank_id, t0, g0), ("B", tank_id, t1, g1)]),
            );
            for (pos, v) in vehicles.iter().enumerate() {
                let ci = if v.nickname == "A" { 0 } else { 1 };
                assert_eq!(
                    v.config_idx,
                    Some(ci),
                    "位置 {pos} 车辆 {} 应钉定配置 {ci}",
                    v.nickname
                );
                // burst_size = 实际搭载配置的弹夹容量（T69 两炮 4 发/3 发不同）
                assert_eq!(
                    v.burst_size,
                    Some(bs(ci as usize) as u32),
                    "位置 {pos} 车辆 {} 弹容应随配置 {ci}",
                    v.nickname
                );
            }
        }
    }

    /// 无 comp 证据：fail-closed → config_idx = None（消费端自选顶级配置为显示默认）。
    /// 2026-10 起"弹种 ⊆ 弹表 / 血量 ±2 容差"启发式回退已退役（P1 探针实证 comp blob
    /// 覆盖全部玩家，推断级证据不再必要——禁猜）。
    #[test]
    fn config_idx_stays_none_without_comp() {
        let tank_id = 14625;
        let configs = crate::wargaming::tank_configs::build_configs(tank_id);
        assert!(configs.len() > 1, "测试前提：多配置车");
        let mut vehicles = vec![vt(tank_id, "C")];
        annotate_vehicle_config_slice(&mut vehicles, &comps(&[]));
        assert_eq!(vehicles[0].config_idx, None);
        assert_eq!(vehicles[0].burst_size, None);
    }

    /// 单配置坦克（IS-7，单发炮）：无配置歧义，config_idx 不解析（与既有语义一致），
    /// 但 burst_size 仍给出该唯一配置的原值（0 = 单发）。
    #[test]
    fn burst_size_still_emitted_for_single_config_tank() {
        let configs = crate::wargaming::tank_configs::build_configs(7169);
        assert_eq!(configs.len(), 1, "测试前提：IS-7 单配置");
        let mut vehicles = vec![vt(7169, "E")];
        annotate_vehicle_config_slice(&mut vehicles, &comps(&[]));
        assert_eq!(vehicles[0].config_idx, None);
        assert_eq!(
            vehicles[0].burst_size,
            Some(configs[0]["burst_size"].as_f64().unwrap().round() as u32)
        );
    }

    /// 未知 tank_id（0 = 身份未知车）：不解析，config_idx/burst_size 保持 None。
    #[test]
    fn config_idx_stays_none_for_unknown_tank() {
        let mut vehicles = vec![vt(0, "D")];
        annotate_vehicle_config_slice(&mut vehicles, &comps(&[]));
        assert_eq!(vehicles[0].config_idx, None);
        assert_eq!(vehicles[0].burst_size, None);
    }
}
