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

use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

const VENDOR_DIR: &str = "web/vendor/three";
/// 数据响应缓存上限（gzip 后的完整 JSON，按回放绝对路径键）
const CACHE_MAX: usize = 4;

// ---------- 数据构建 + 缓存 ----------

type Cache = Mutex<Vec<(String, Arc<Vec<u8>>)>>;
static CACHE: OnceLock<Cache> = OnceLock::new();

fn cache() -> &'static Cache {
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

/// 解析回放 → PlaybackData → 未压缩 JSON 字节（缓存命中直接返回）。
/// TankResolver 用 viewer 的全局单例（web serve 启动时已 set；standalone 自行 set）。
pub fn build_playback_json(path: &Path) -> anyhow::Result<Arc<Vec<u8>>> {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy().to_string();
    if let Some((_, v)) = cache().lock().unwrap().iter().find(|(k, _)| k == &key) {
        return Ok(v.clone());
    }
    let json = build_playback_json_uncached(path, &key)?;
    let arc = Arc::new(json);
    let mut c = cache().lock().unwrap();
    c.insert(0, (key, arc.clone()));
    c.truncate(CACHE_MAX);
    Ok(arc)
}

fn build_playback_json_uncached(path: &Path, key: &str) -> anyhow::Result<Vec<u8>> {
    use wotbreplay_parser::replay::Replay;

    let mut replay = Replay::open(std::fs::File::open(path)?)?;
    let meta = replay.read_meta().ok();
    let data = replay.read_data()?;
    let packets: Vec<(u32, f32, &[u8])> = data.packets.iter().map(|pkt| {
        let t = match &pkt.payload {
            wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
            wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
            wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
        };
        (t, pkt.clock_secs, &pkt.raw_payload[..])
    }).collect();

    let br = replay.read_battle_results().ok();
    let resolver = crate::wargaming::viewer::global_resolver();
    let pitch_limits = br.as_ref()
        .map(|br| resolver.pitch_limits_from_battle_results(br))
        .unwrap_or_default();

    // battle_results → 玩家联表（昵称/队伍 × tank_id）+ 坦克名
    let winner_team = br.as_ref().and_then(|br| br.winner_team_number.as_ref())
        .map(|w| if *w == 1 { 1u8 } else if *w == 2 { 2u8 } else { 0u8 })
        .unwrap_or(0);
    let mut players = Vec::new();
    let mut tank_names = HashMap::new();
    if let Some(br) = br.as_ref() {
        for pr in &br.player_results {
            let joined = br.players.iter().find(|p| p.account_id == pr.info.account_id);
            let tank_id = pr.info.tank_id;
            players.push(crate::replay::playback::PlaybackPlayer {
                account_id: pr.info.account_id,
                nickname: joined.map(|p| p.info.nickname.clone()).unwrap_or_default(),
                team: joined.map(|p| if p.info.team == 1 { 1u8 } else { 2u8 }).unwrap_or(0),
                tank_id,
            });
            if let Some(name) = resolver.resolve(tank_id) {
                tank_names.insert(tank_id, name);
            }
        }
    }
    let author_account_id = br.as_ref().map(|b| b.author.account_id).unwrap_or(0);
    let map_id = br.as_ref().map(|b| b.mode_map_id & 0xFFFF).unwrap_or(0);
    let map_name = meta.as_ref()
        .map(|m| format!("{:?}", m.map_id))
        .unwrap_or_else(|| format!("map_{map_id}"));

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
    // 实际搭载：comp blob（确定性）优先，弹种/血量推断回退
    let valid_tanks: Vec<u32> = pb.vehicles.iter().map(|v| v.tank_id).collect();
    let comps = crate::replay::playback::collect_comp_descriptors(&packets, &valid_tanks);
    if !comps.is_empty() {
        eprintln!("[playback] comp 描述符: {} 条（updateArena subtype 1）", comps.len());
    }
    annotate_vehicle_configs(&mut pb, &comps);
    eprintln!("[playback] 完成: {} 车 / {} 发 / {:.0}s",
        pb.vehicles.len(), pb.shots.len(), pb.meta.duration);
    let json = serde_json::to_vec(&pb)?;
    Ok(json)
}

// ---------- 实际搭载配置推断（GLB 炮塔/主炮变体选择） ----------
//
// 回放不含直接的模块 id，但有两个可靠证据：
// 1. 发射弹种（shell_id 全局 id）：不同主炮弹表不同——已观测弹种必须 ⊆ 该炮弹表；
// 2. 初始血量：总 HP = 车体 health + 炮塔 health（tanks.pb），装备"改进耐久"= ×1.125。
// 弹种证据缺失（未开炮）时按血量；再缺失取顶级配置。多匹配取最后一档。

fn annotate_vehicle_configs(pb: &mut crate::replay::playback::PlaybackData,
                            comps: &HashMap<String, crate::replay::playback::CompDescriptor>) {
    // 实际搭载解析统一走 viewer::resolve_config_index（comp blob → 弹种 → 血量 三级证据链，
    // 与射击复现共享同一实现）；返回 dense (turret_index, gun_index)
    let mut cache: HashMap<u32, Option<(u32, u32)>> = HashMap::new();
    for v in &mut pb.vehicles {
        if v.tank_id == 0 { continue; }
        let pair = cache.entry(v.tank_id).or_insert_with(|| {
            let comp = comps.get(&v.nickname).and_then(|c| {
                ((c.tank_id & 0xFFFF) == (v.tank_id & 0xFFFF)).then_some((c.turret_local, c.gun_local))
            });
            crate::wargaming::viewer::resolve_config_index(v.tank_id, comp, &v.shell_ids, v.max_hp)
                .map(|(_, ti, gi)| (ti, gi))
        });
        if let Some((ti, gi)) = *pair {
            v.turret_index = Some(ti);
            v.gun_index = Some(gi);
        }
    }
}

fn gzip_bytes(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

/// 数据响应：gzip JSON（浏览器 fetch 按 Content-Encoding 透明解压）
pub async fn playback_data_response(path: &Path) -> Response {
    match build_playback_json(path) {
        Ok(json) => {
            let gz = gzip_bytes(&json);
            (
                [
                    (axum::http::header::CONTENT_TYPE, "application/json"),
                    (axum::http::header::CONTENT_ENCODING, "gzip"),
                ],
                gz,
            ).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("回放数据构建失败: {e:?}"),
        ).into_response(),
    }
}

/// POST /api/playback/data  body {file: "路径"}（与 /api/replay/shots 同款路径参数约定）
pub async fn playback_data_handler(Json(body): Json<serde_json::Value>) -> Response {
    let file = body["file"].as_str().unwrap_or("").trim().to_string();
    if file.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing file").into_response();
    }
    playback_data_response(Path::new(&file)).await
}

/// GET /api/playback/map?name=WinterMalinovka —— 地图底图（v1 数据源未定，恒 404，
/// 前端回退程序生成网格；接入在线源/游戏提取后在此返回 PNG）
pub async fn playback_map_handler(
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Response {
    let _name = q.get("name").cloned().unwrap_or_default();
    (axum::http::StatusCode::NOT_FOUND, "map image not available").into_response()
}

/// 播放器页面（web 模式 asset prefix = /armor_view：GLB/vendor 复用 armor_view 路由）
pub async fn playback_page_handler() -> Html<String> {
    Html(playback_index_html("/armor_view"))
}

// ---------- 独立服务（CLI `playback <replay>`） ----------

pub async fn serve_standalone(replay_path: &Path) -> anyhow::Result<()> {
    // 全局 resolver（GLB 车模名/俯仰极限表用）
    let resolver = crate::wargaming::tank_resolver::TankResolver::load_from_json_file(
        crate::data::data_path("tank_cache.json").as_path(),
    ).unwrap_or_default();
    crate::wargaming::viewer::set_global_resolver(resolver);

    // 预热缓存（启动即构建，首开页面零等待；失败不退出——页面仍可显示错误）
    if let Err(e) = build_playback_json(replay_path) {
        eprintln!("[playback] 预构建失败（页面请求时将重试）: {e:?}");
    }

    let index_html = playback_index_html("");
    let app = Router::new()
        .route("/", get(move || {
            let html = index_html.clone();
            async move { Html(html) }
        }))
        .route("/api/playback/data", post(playback_data_handler))
        .route("/api/playback/map", get(playback_map_handler))
        .route("/api/tank/{tank_id}", get(crate::wargaming::viewer::tank_data_handler))
        .route("/vendor/three/{*path}", get(crate::wargaming::viewer::vendor_handler))
        .route("/glb/{tank_id}/{filename}", get(crate::wargaming::viewer::glb_handler))
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

// ---------- 内嵌前端 ----------

pub fn playback_index_html(base_prefix: &str) -> String {
    let vendor_local = Path::new(VENDOR_DIR).join("three.module.js").exists();
    let importmap = if vendor_local {
        format!(
            r#"{{ "imports": {{ "three": "{base_prefix}/vendor/three/three.module.js", "three/addons/": "{base_prefix}/vendor/three/addons/" }} }}"#
        )
    } else {
        r#"{ "imports": { "three": "https://cdn.jsdelivr.net/npm/three@0.169.0/build/three.module.js", "three/addons/": "https://cdn.jsdelivr.net/npm/three@0.169.0/examples/jsm/" } }"#.to_string()
    };
    INDEX_HTML
        .replace("__IMPORTMAP__", &importmap)
        .replace("__ASSET_PREFIX__", base_prefix)
}

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>实时回放 · wotb-agent</title>
<style>
  :root { --panel: rgba(16,20,26,.82); --line: #2c3542; --fg: #d8dee7; --dim: #8a94a3;
          --ally: #3fa66a; --enemy: #c05046; --unknown: #8a94a3; --accent: #e8b23c; }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  html, body { width: 100%; height: 100%; overflow: hidden; background: #0d1117;
               color: var(--fg); font: 13px/1.45 "Segoe UI", "Microsoft YaHei", sans-serif; }
  #scene { position: absolute; inset: 0; }
  .panel { position: absolute; background: var(--panel); border: 1px solid var(--line);
           border-radius: 8px; backdrop-filter: blur(4px); }
  #topbar { top: 10px; left: 50%; transform: translateX(-50%); padding: 6px 18px;
            display: flex; gap: 16px; align-items: center; white-space: nowrap; }
  #topbar .timer { font-size: 18px; font-weight: 600; font-variant-numeric: tabular-nums; }
  #topbar .score { font-size: 16px; font-weight: 600; }
  #topbar .score .t1 { color: var(--ally); } #topbar .score .t2 { color: var(--enemy); }
  #topbar .map { color: var(--dim); }
  .team { top: 60px; width: 240px; padding: 6px; max-height: calc(100% - 190px); overflow-y: auto; }
  #team1 { left: 10px; } #team2 { right: 10px; }
  .team h3 { font-size: 12px; color: var(--dim); margin: 2px 4px 6px; font-weight: 500; }
  .pl { display: flex; align-items: center; gap: 6px; padding: 3px 6px; border-radius: 5px;
        cursor: pointer; }
  .pl:hover { background: rgba(255,255,255,.06); }
  .pl.dead { opacity: .42; }
  .pl.dead .nick { text-decoration: line-through; }
  .pl.followed { outline: 1px solid var(--accent); }
  .pl .dot { width: 8px; height: 8px; border-radius: 2px; flex: none; }
  .pl .nick { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pl .tank { color: var(--dim); font-size: 11px; max-width: 86px; overflow: hidden;
              text-overflow: ellipsis; white-space: nowrap; }
  .pl .hpbar { width: 52px; height: 5px; background: #222a34; border-radius: 3px; flex: none; }
  .pl .hpbar i { display: block; height: 100%; border-radius: 3px; background: var(--ally); }
  .pl.enemy .hpbar i { background: var(--enemy); } .pl.unknown .hpbar i { background: var(--unknown); }
  #killfeed { position: absolute; top: 60px; left: 50%; transform: translateX(-50%);
              display: flex; flex-direction: column; align-items: center; gap: 4px; pointer-events: none; }
  .kf { background: var(--panel); border: 1px solid var(--line); border-radius: 6px;
        padding: 3px 12px; font-size: 12px; animation: kfin .18s ease-out; white-space: nowrap; }
  .kf .k { color: var(--accent); font-weight: 600; }
  @keyframes kfin { from { opacity: 0; transform: translateY(-6px); } }
  #controls { bottom: 10px; left: 50%; transform: translateX(-50%); width: min(880px, 94%);
              padding: 8px 14px; display: flex; flex-direction: column; gap: 6px; }
  #controls .row { display: flex; gap: 8px; align-items: center; }
  #controls input[type=range] { flex: 1; accent-color: var(--accent); }
  #controls .time { font-variant-numeric: tabular-nums; color: var(--dim); min-width: 96px; text-align: center; }
  button, select { background: #1d242e; color: var(--fg); border: 1px solid var(--line);
                   border-radius: 6px; padding: 4px 10px; cursor: pointer; font-size: 12px; }
  button:hover { border-color: var(--accent); }
  button.on { background: var(--accent); color: #14181e; border-color: var(--accent); font-weight: 600; }
  #playBtn { width: 74px; font-weight: 600; }
  label.toggle { display: flex; gap: 4px; align-items: center; color: var(--dim); cursor: pointer; }
  #banner { position: absolute; top: 38%; left: 50%; transform: translate(-50%,-50%);
            font-size: 42px; font-weight: 700; padding: 14px 44px; display: none;
            background: var(--panel); border: 1px solid var(--line); border-radius: 12px; }
  #loader { position: absolute; inset: 0; background: rgba(10,13,17,.94); z-index: 10;
            display: flex; flex-direction: column; gap: 14px; align-items: center;
            justify-content: center; }
  #loader h2 { font-weight: 500; }
  #loader .row { display: flex; gap: 8px; }
  #loader input[type=text] { width: 420px; background: #141a22; color: var(--fg);
      border: 1px solid var(--line); border-radius: 6px; padding: 6px 10px; }
  #loader .hint { color: var(--dim); max-width: 560px; text-align: center; }
  #err { color: #e07b7b; max-width: 640px; white-space: pre-wrap; }
  #speeds button { min-width: 38px; }
</style>
<script type="importmap">__IMPORTMAP__</script>
</head>
<body>
<div id="scene"></div>

<div id="topbar" class="panel">
  <span class="map" id="mapName"></span>
  <span class="timer" id="timer">--:--</span>
  <span class="score"><span class="t1" id="score1">0</span> : <span class="t2" id="score2">0</span></span>
</div>

<div id="team1" class="team panel"><h3>队伍 1</h3><div class="roster"></div></div>
<div id="team2" class="team panel"><h3>队伍 2</h3><div class="roster"></div></div>
<div id="killfeed"></div>
<div id="banner"></div>

<div id="controls" class="panel">
  <div class="row">
    <button id="playBtn">▶ 播放</button>
    <span id="speeds"></span>
    <input type="range" id="seek" min="0" max="1000" value="0">
    <span class="time" id="timeLabel">0.0s / 0.0s</span>
  </div>
  <div class="row">
    <span style="color:var(--dim)">镜头</span>
    <button data-cam="free" class="on">自由</button>
    <button data-cam="top">俯视</button>
    <button data-cam="follow">跟随</button>
    <span style="flex:1"></span>
    <label class="toggle"><input type="checkbox" id="glbToggle"> 真实车模（GLB）</label>
    <label class="toggle"><input type="checkbox" id="labelToggle" checked> 昵称标签</label>
  </div>
</div>

<div id="loader">
  <h2>全场实时回放</h2>
  <div class="row">
    <input type="text" id="filePath" placeholder=".wotbreplay 文件路径（或 URL 加 ?file=）">
    <button id="loadBtn">加载</button>
  </div>
  <div class="hint">14 车全场连续回放：滤波渲染位姿 + 炮塔/炮管随动 + 弹道飞行动画 + 实时血量/击杀流。<br>
  数据由本机解析（AvatarFilter 渲染层 + prop2 炮塔角），加载需数秒。</div>
  <div id="err"></div>
</div>

<script type="module">
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

const P = "__ASSET_PREFIX__";
const $ = (id) => document.getElementById(id);
const GRID_DT = 0.1;

// ---------- 全局状态 ----------
let DATA = null;                 // PlaybackData
let V = [];                      // 车辆运行时 {def, group, turretG, gunPivot, label, meshHull, glb}
let T = 0, PLAYING = false, SPEED = 2;
let CAM = 'free', FOLLOW_EID = 0;
let shotPtr = 0, killPtr = 0;
const tracers = [], impacts = [];
let renderer, scene, camera, controls, clock, raycaster;
let glbCache = new Map(), glbOn = false;

// ---------- 工具 ----------
const fmtTime = (s) => { s = Math.max(0, s); const m = Math.floor(s / 60);
  return String(m).padStart(2, '0') + ':' + String(Math.floor(s % 60)).padStart(2, '0'); };
const wrapPi = (a) => { while (a > Math.PI) a -= 2 * Math.PI; while (a < -Math.PI) a += 2 * Math.PI; return a; };

function idxOf(t) { return Math.floor((t - DATA.meta.t_start) / GRID_DT); }

function posAt(v, t, out) {
  const i = idxOf(t), n = DATA.meta.samples;
  const i0 = Math.max(0, Math.min(i, n - 1)), i1 = Math.min(i0 + 1, n - 1);
  const f = Math.max(0, Math.min(1, (t - DATA.meta.t_start) / GRID_DT - i0));
  const p = v.def.pos;
  out.set(-(p[i0*3] + (p[i1*3] - p[i0*3]) * f),
           p[i0*3+1] + (p[i1*3+1] - p[i0*3+1]) * f,
           p[i0*3+2] + (p[i1*3+2] - p[i0*3+2]) * f);
  return out;
}
function yawAt(v, t) { const a = arrAt(v.def.hull_yaw, t); return a; }
function turretAbsAt(v, t) { return arrAt(v.def.turret_yaw, t); }
function gunPitchAt(v, t) { return arrAt(v.def.gun_pitch, t); }
function arrAt(arr, t) {
  const i = idxOf(t), n = DATA.meta.samples;
  const i0 = Math.max(0, Math.min(i, n - 1)), i1 = Math.min(i0 + 1, n - 1);
  const f = Math.max(0, Math.min(1, (t - DATA.meta.t_start) / GRID_DT - i0));
  return arr[i0] + (arr[i1] - arr[i0]) * f;
}
function hpAt(v, t) {
  const hp = v.def.hp; let cur = v.def.max_hp;
  for (const [ht, hv] of hp) { if (ht <= t) cur = hv; else break; }
  return cur;
}
function deathAt(v, t) { return v.def.death_t == null ? false : t >= v.def.death_t; }
function visibleAt(v, t) {
  const c = v.def.coverage;
  for (let k = 0; k + 1 < c.length; k += 2) if (t >= c[k] && t <= c[k+1]) return true;
  return false;
}
function gameTimerLabel(t) {
  const ps = DATA.periods; let p = null;
  for (const pe of ps) { if (pe.clock <= t) p = pe; else break; }
  if (!p || p.period < 3) return p ? (p.period === 1 ? '准备' : '倒计时') : fmtTime(t);
  const elapsed = (t - p.clock) + (p.duration_s - p.remaining_s);
  return fmtTime(Math.max(0, p.duration_s - elapsed));
}

// ---------- 场景 ----------
function initScene() {
  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x11161d);
  camera = new THREE.PerspectiveCamera(55, innerWidth / innerHeight, 0.5, 4000);
  camera.position.set(0, 180, 220);
  renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setSize(innerWidth, innerHeight);
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  $('scene').appendChild(renderer.domElement);
  controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true; controls.maxPolarAngle = Math.PI / 2 - 0.02;
  clock = new THREE.Clock();
  raycaster = new THREE.Raycaster();
  scene.add(new THREE.HemisphereLight(0xbfd4e8, 0x2a2f36, 0.9));
  const sun = new THREE.DirectionalLight(0xffffff, 1.1); sun.position.set(120, 260, 80); scene.add(sun);
  addEventListener('resize', () => {
    camera.aspect = innerWidth / innerHeight; camera.updateProjectionMatrix();
    renderer.setSize(innerWidth, innerHeight);
  });
  // 点选车辆 → 跟随
  renderer.domElement.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    const nd = new THREE.Vector2((e.clientX / innerWidth) * 2 - 1, -(e.clientY / innerHeight) * 2 + 1);
    raycaster.setFromCamera(nd, camera);
    const hits = raycaster.intersectObjects(V.map(v => v.group), true);
    if (hits.length) {
      let o = hits[0].object;
      while (o && !o.userData.eid) o = o.parent;
      if (o) setFollow(o.userData.eid);
    }
  });
}

function buildWorld() {
  // 数据范围（直接取自 DATA；去 2% 离群后取整百）。注意必须在 buildVehicles 之前也可用——
  // 运行时数组 V 此时尚未填充
  const xs = [], zs = [];
  for (const def of DATA.vehicles)
    for (let i = 0; i < def.pos.length; i += 9) { xs.push(def.pos[i]); zs.push(def.pos[i+2]); }
  xs.sort((a,b)=>a-b); zs.sort((a,b)=>a-b);
  const q = (a, f) => a.length ? a[Math.floor((a.length-1) * f)] : 0;
  const ex = Math.max(q(xs, .98) - q(xs, .02), 200) * 0.65;
  const ez = Math.max(q(zs, .98) - q(zs, .02), 200) * 0.65;
  const m = Math.max(ex, ez);
  const ext = isFinite(m) && m > 0 ? Math.ceil(m / 50) * 50 : 300;
  const cx = (q(xs, .98) + q(xs, .02)) / 2, cz = (q(zs, .98) + q(zs, .02)) / 2;

  const ground = new THREE.Mesh(
    new THREE.PlaneGeometry(ext * 2 + 100, ext * 2 + 100),
    new THREE.MeshLambertMaterial({ color: 0x202a36 }));
  ground.rotation.x = -Math.PI / 2; ground.position.set(cx, 0, cz);
  scene.add(ground);
  const grid = new THREE.GridHelper(ext * 2 + 100, Math.floor((ext * 2 + 100) / 50), 0x3a4a5e, 0x273140);
  grid.position.set(cx, 0.02, cz); scene.add(grid);
  WORLD_CENTER = { cx, cz, ext };
  camera.position.set(cx, ext * 1.1, cz + ext * 1.2);
  controls.target.set(cx, 0, cz);
}
let WORLD_CENTER = { cx: 0, cz: 0, ext: 300 };

function teamColor(v) {
  const f = DATA.meta.friendly_team, t = v.def.team;
  if (t === 0 || f === 0) return 0x8a94a3;
  return t === f ? 0x3fa66a : 0xc05046;
}

function makeLabel(v) {
  const cv = document.createElement('canvas'); cv.width = 256; cv.height = 64;
  v.labelCanvas = cv;
  const tex = new THREE.CanvasTexture(cv);
  const sp = new THREE.Sprite(new THREE.SpriteMaterial({ map: tex, depthTest: false }));
  sp.scale.set(10, 2.5, 1); sp.position.y = 6.2;
  v.label = sp; v.labelHp = null; v.labelDead = null;
  drawLabel(v);
  return sp;
}
// 昵称 + 实时血量条（当前/上限数字）+ 击毁状态。
// 变化检测必须在清空画布之前——先 clear 再早退会得到永久空白标签。
function drawLabel(v) {
  const hp = hpAt(v, T), dead = deathAt(v, T);
  if (hp === v.labelHp && dead === v.labelDead) return;
  v.labelHp = hp; v.labelDead = dead;
  const cv = v.labelCanvas, ctx = cv.getContext('2d');
  ctx.clearRect(0, 0, 256, 64);
  ctx.fillStyle = 'rgba(8,11,15,.62)';
  ctx.fillRect(14, 2, 228, 60);
  ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
  // 昵称（击毁置灰）
  ctx.font = '600 23px "Segoe UI", "Microsoft YaHei", sans-serif';
  ctx.fillStyle = dead ? 'rgba(150,158,168,.72)' : '#e6ebf2';
  ctx.fillText(dead ? '✝ ' + (v.def.nickname || 'Unknown') : (v.def.nickname || 'Unknown'), 128, 17, 208);
  // 血量条
  const frac = v.def.max_hp > 0 ? hp / v.def.max_hp : 0;
  const bx = 34, bw = 188, by = 36, bh = 14;
  ctx.fillStyle = 'rgba(0,0,0,.55)'; ctx.fillRect(bx, by, bw, bh);
  const col = dead ? '#3a424c'
    : v.def.team === 0 ? '#8a94a3'
    : (v.def.team === DATA.meta.friendly_team ? '#3fa66a' : '#c05046');
  ctx.fillStyle = col; ctx.fillRect(bx + 1, by + 1, Math.max(0, (bw - 2) * frac), bh - 2);
  // 血量数字（描边保证条上可读）
  ctx.font = '600 14px "Segoe UI", sans-serif';
  ctx.lineWidth = 3; ctx.strokeStyle = 'rgba(0,0,0,.85)';
  const txt = v.def.max_hp > 0 ? (hp + ' / ' + v.def.max_hp) : '—';
  ctx.strokeText(txt, bx + bw / 2, by + bh / 2 + 1);
  ctx.fillStyle = '#fff';
  ctx.fillText(txt, bx + bw / 2, by + bh / 2 + 1);
  v.label.material.map.needsUpdate = true;
}

function buildVehicles() {
  const hullGeo = new THREE.BoxGeometry(3.2, 1.05, 6.2);
  const trackGeo = new THREE.BoxGeometry(3.6, 0.75, 6.5);
  const turretGeo = new THREE.BoxGeometry(2.35, 0.85, 3.3);
  const gunGeo = new THREE.CylinderGeometry(0.14, 0.18, 5.4, 8);
  const trackMat = new THREE.MeshLambertMaterial({ color: 0x333a44 });
  for (const def of DATA.vehicles) {
    const color = teamColor({ def });
    const g = new THREE.Group(); g.userData.eid = def.eid;
    const hull = new THREE.Mesh(hullGeo, new THREE.MeshLambertMaterial({ color }));
    hull.position.y = 1.05;
    const tracks = new THREE.Mesh(trackGeo, trackMat); tracks.position.y = 0.42;
    const turretG = new THREE.Group(); turretG.position.y = 1.85;
    const turret = new THREE.Mesh(turretGeo, new THREE.MeshLambertMaterial({ color: new THREE.Color(color).multiplyScalar(1.15) }));
    turret.position.z = -0.25;
    const gunPivot = new THREE.Group(); gunPivot.position.set(0, 0.05, 1.5);
    const gun = new THREE.Mesh(gunGeo, new THREE.MeshLambertMaterial({ color: 0x59636f }));
    gun.rotation.x = Math.PI / 2; gun.position.z = 2.4;
    gunPivot.add(gun); turretG.add(turret); turretG.add(gunPivot);
    g.add(tracks); g.add(hull); g.add(turretG);
    if (def.is_author) {
      const ring = new THREE.Mesh(new THREE.RingGeometry(3.6, 4.3, 32),
        new THREE.MeshBasicMaterial({ color: 0xe8b23c, side: THREE.DoubleSide, transparent: true, opacity: .85 }));
      ring.rotation.x = -Math.PI / 2; ring.position.y = 0.06;
      ring.userData.keepWithGlb = true;   // GLB 模式下保留作者标记环
      g.add(ring);
    }
    const v = { def, group: g, turretG, gunPivot, meshHull: hull };
    g.add(makeLabel(v));
    scene.add(g);
    V.push(v);
  }
}

// ---------- GLB 真实车模 ----------
// 姿态处理与装甲查看器 world 模式（viewer.rs poseFromYPR / poseShooterTurretGun）同构：
// - GLB 内部系 x右/y前/z上（models.pb 原点已按 (x,z,y) 校正到该系）；根位姿 = qYaw·qPitch·qRoll·qFrame，
//   qFrame = Ry(π)·Rx(−π/2) 的 z-up→y-up 帧变换；yaw 传镜像值（−游戏 yaw），pitch 原值，roll 滤波层恒 0；
// - 炮塔/炮管 = 烘焙矩阵绕枢轴旋转：炮塔 Rz(−rel)（镜像系）@ tP=track+turret，炮管 Rx(俯仰)@ gP=tP+gun_origin；
// - 部件数据（model_origins / configs[].gun_origin / initial_turret_rotation）来自 /api/tank/{id}。

async function loadGlb(tankId) {
  if (glbCache.has(tankId)) return glbCache.get(tankId);
  const p = (async () => {
    try {
      const [{ GLTFLoader }, sd] = await Promise.all([
        import('three/addons/loaders/GLTFLoader.js'),
        fetch(P + '/api/tank/' + tankId).then(r => r.ok ? r.json() : null).catch(() => null),
      ]);
      const model = await new Promise((res) =>
        new GLTFLoader().load(`${P}/glb/${tankId}/model.glb`, g => res(g.scene), undefined, () => res(null)));
      if (!model) return null;
      model.scale.setScalar(1);
      // 拆件战斗渲染恒隐藏（否则随炮塔/炮盾转动暴露，装甲查看器同规则）
      model.traverse(n => {
        if (/^(gun_\d+|turret_\d+|hull)_hide_elements$/.test(n.name || ''))
          n.traverse(m => { if (m.isMesh) m.visible = false; });
      });
      // 缓存模板 + 部件数据（sd）；每车实例化时 clone 并重收集节点引用
      //（同 tank_id 多车共用一个实例会互相抢对象、位姿互覆盖）
      return { template: model, sd };
    } catch { return null; }
  })();
  glbCache.set(tankId, p);
  return p;
}

// 炮塔/炮管驱动部件收集 + 枢轴原点链（track+turret / +gun_origin，models.pb 数据）
function collectGlbParts(model, sd, sel) {
  // 节点分组（装甲查看器 collectConfigNodes 同式）：gun_XX(+_mask 等) 按编号分组升序、
  // turret_XX 升序——与 build_configs 的 dense 索引序一致
  const byGroup = new Map(); const turrets = [];
  model.traverse(n => {
    const nm = n.name || '';
    const gm = nm.match(/^gun_(\d+)/);
    const tm = nm.match(/^turret_(\d+)$/);
    if (gm) { const g = parseInt(gm[1], 10); if (!byGroup.has(g)) byGroup.set(g, []); byGroup.get(g).push(n); }
    else if (tm) turrets.push(n);
  });
  const gunGroups = Array.from(byGroup.keys()).sort((a, b) => a - b)
    .map(k => byGroup.get(k).sort((a, b) => ((a.name || '') < (b.name || '') ? -1 : 1)));
  turrets.sort((a, b) => ((a.name.match(/\d+/)?.[0] | 0) - (b.name.match(/\d+/)?.[0] | 0)));
  // 选中变体 = 回放证据推断的 dense 索引（缺省 = 顶级配置），其余隐藏
  const cfgs = (sd && sd.configs && sd.configs.length) ? sd.configs : null;
  const gi = (sel && sel.gun_index != null) ? sel.gun_index
    : (cfgs ? cfgs[cfgs.length - 1].gun_index : 0);
  const ti = (sel && sel.turret_index != null) ? sel.turret_index
    : (cfgs ? cfgs[cfgs.length - 1].turret_index : 0);
  gunGroups.forEach((grp, i) => grp.forEach(n => n.visible = (i === (gi % gunGroups.length))));
  turrets.forEach((n, i) => n.visible = (i === (ti % turrets.length)));
  const turretNode = turrets.length ? turrets[ti % turrets.length] : null;
  // 摆位只取炮管本体+炮盾（精确命名）；组内其余节点（gun_XX_mask_nc 等）是炮盾子树，
  // 直接摆位会与父节点继承叠加成双重旋转（装甲查看器 gunBarrelNodes 同款过滤）
  const grp = gunGroups.length ? gunGroups[gi % gunGroups.length] : [];
  const barrelNodes = grp.filter(n => /^gun_\d+(_mask)?$/.test(n.name || ''));
  const gunNodes = barrelNodes.length ? barrelNodes : grp;
  const mo = sd && sd.model_origins;
  if (!turretNode || !mo || !mo.track || !mo.turret) {
    console.warn('[playback] glb parts incomplete: turret=' + !!turretNode + ' origins=' + !!(mo && mo.track));
    return null;
  }
  const tP = [mo.track[0] + mo.turret[0], mo.track[1] + mo.turret[1], mo.track[2] + mo.turret[2]];
  // 火炮枢轴用选中配置的 gun_origin（非默认顶级）
  const cfg = cfgs ? (cfgs.find(c => c.turret_index === ti && c.gun_index === gi) || cfgs[cfgs.length - 1]) : null;
  const gP = (cfg && cfg.gun_origin)
    ? [tP[0] + cfg.gun_origin[0], tP[1] + cfg.gun_origin[1], tP[2] + cfg.gun_origin[2]]
    : tP.slice();
  return { turretNode, gunNodes, tP, gP, itr: sd.initial_turret_rotation || null };
}

// GLB 根位姿（每刻；与 armor viewer applyPose 同式：pos 镜像 + poseFromYPR(−yaw, pitch, 0)）
function poseFromYPR(yaw, pitch, roll) {
  const qYpi = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), Math.PI);
  const qFrame = qYpi.multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), -Math.PI / 2));
  const qYaw = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), yaw || 0);
  const qPitch = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), pitch || 0);
  const qRoll = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), roll || 0);
  return qYaw.multiply(qPitch).multiply(qRoll).multiply(qFrame);
}
function poseGlb(v) {
  v.glb.position.copy(v.group.position);
  v.glb.quaternion.copy(poseFromYPR(-yawAt(v, T), arrAt(v.def.hull_pitch, T), 0));
  const p = v.glbParts;
  if (!p) return;
  const rel = wrapPi(turretAbsAt(v, T) - yawAt(v, T));
  const tr = -rel;                       // 镜像系节点旋转角 = −rel
  const gr = gunPitchAt(v, T);           // glb 系 Rx(θ)：θ>0 = 前向(+Y)抬向 +Z = 仰角
  let turretRot = new THREE.Matrix4().makeRotationZ(tr);
  if (p.itr) {
    turretRot = new THREE.Matrix4().makeRotationFromEuler(new THREE.Euler(
      -THREE.MathUtils.degToRad(p.itr.pitch || 0), -THREE.MathUtils.degToRad(p.itr.roll || 0),
      tr - THREE.MathUtils.degToRad(p.itr.yaw || 0), 'XYZ'));
  }
  const mT = new THREE.Matrix4().makeTranslation(p.tP[0], p.tP[1], p.tP[2]).multiply(turretRot)
    .multiply(new THREE.Matrix4().makeTranslation(-p.tP[0], -p.tP[1], -p.tP[2]));
  const mG = mT.clone().multiply(new THREE.Matrix4().makeTranslation(p.gP[0], p.gP[1], p.gP[2]))
    .multiply(new THREE.Matrix4().makeRotationX(gr))
    .multiply(new THREE.Matrix4().makeTranslation(-p.gP[0], -p.gP[1], -p.gP[2]));
  const tn = p.turretNode;
  tn.updateMatrix();
  if (!tn.userData.__bake) { tn.userData.__bake = tn.matrix.clone(); tn.matrixAutoUpdate = false; }
  tn.matrix.copy(mT.clone().multiply(tn.userData.__bake));
  for (const gn of p.gunNodes) {
    gn.updateMatrix();
    if (!gn.userData.__bake) { gn.userData.__bake = gn.matrix.clone(); gn.matrixAutoUpdate = false; }
    gn.matrix.copy(mG.clone().multiply(gn.userData.__bake));
  }
  v.glb.updateMatrixWorld(true);
}

async function applyGlbToggle(on) {
  glbOn = on;
  if (on) {
    // 并行加载：单个大模型慢解析不阻塞其余车辆；每车 clone 独立实例
    await Promise.all(V.filter(v => v.def.tank_id > 0).map(async (v) => {
      if (!v.glb) {
        const loaded = await loadGlb(v.def.tank_id);
        if (loaded && glbOn && !v.glb) {
          const inst = loaded.template.clone();
          inst.scale.setScalar(1);
          v.glb = inst;
          v.glbParts = collectGlbParts(inst, loaded.sd,
            { turret_index: v.def.turret_index ?? null, gun_index: v.def.gun_index ?? null });
          v.glb.visible = v.group.visible;
          scene.add(v.glb);
        }
      }
      if (v.glb) setLowPoly(v, false);
    }));
  } else {
    for (const v of V) {
      if (v.glb) { scene.remove(v.glb); v.glb = null; v.glbParts = null; }
      setLowPoly(v, true);
    }
  }
}
// 低模显隐（标签与作者标记环除外；GLB 根在 scene 上不经过 group）
function setLowPoly(v, show) {
  for (const c of v.group.children) {
    if (c === v.label || c.userData.keepWithGlb) continue;
    c.visible = show;
  }
}

// ---------- 弹道 ----------
const TRACER_LEN = 9;
function spawnShot(s) {
  const from = new THREE.Vector3(-s.from[0], s.from[1], s.from[2]);
  const to = new THREE.Vector3(-s.to[0], s.to[1], s.to[2]);
  const color = s.is_kill ? 0xff3355 : s.ricochet ? 0x9aa5b1
    : s.game_hit_result === 3 ? 0xffc94d : s.hit ? 0x6fb3ff : 0xd8dee7;
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(0.16, 0.16, TRACER_LEN),
    new THREE.MeshBasicMaterial({ color }));
  scene.add(mesh);
  // WoTB 弹速高、交战近，直飞常 <0.3s——最小显示 0.22s 保证可见性
  tracers.push({ mesh, from, to, t0: s.t_fire, t1: s.t_fire + Math.max(0.22, s.flight_secs), shot: s, color });
}
function spawnImpact(tr) {
  const s = tr.shot;
  const g = new THREE.Group();
  const ball = new THREE.Mesh(new THREE.SphereGeometry(0.55, 10, 10),
    new THREE.MeshBasicMaterial({ color: tr.color, transparent: true }));
  const ring = new THREE.Mesh(new THREE.RingGeometry(0.8, 1.15, 20),
    new THREE.MeshBasicMaterial({ color: tr.color, side: THREE.DoubleSide, transparent: true }));
  ring.rotation.x = -Math.PI / 2;
  g.add(ball); g.add(ring);
  g.position.copy(tr.to);
  scene.add(g);
  impacts.push({ g, until: tr.t1 + 2.2, ball, ring });
}
function updateTracers() {
  for (let i = tracers.length - 1; i >= 0; i--) {
    const tr = tracers[i];
    if (T < tr.t0) continue;
    const f = Math.min(1, (T - tr.t0) / (tr.t1 - tr.t0));
    const head = tr.from.clone().lerp(tr.to, f);
    const tail = tr.from.clone().lerp(tr.to, Math.max(0, f - TRACER_LEN / tr.from.distanceTo(tr.to)));
    tr.mesh.position.copy(head.clone().add(tail).multiplyScalar(0.5));
    tr.mesh.lookAt(head);
    if (f >= 1) {
      scene.remove(tr.mesh); tracers.splice(i, 1);
      spawnImpact(tr);
    }
  }
  for (let i = impacts.length - 1; i >= 0; i--) {
    const im = impacts[i];
    const left = im.until - T;
    if (left <= 0) { scene.remove(im.g); impacts.splice(i, 1); continue; }
    const op = Math.min(1, left / 1.2);
    im.ball.material.opacity = op; im.ring.material.opacity = op * 0.8;
    im.ring.scale.setScalar(1 + (1 - Math.min(1, left / 2.2)) * 1.6);
  }
}

// ---------- 击杀 feed / 计分 ----------
function feedEntry(k) {
  const name = (eid) => { const v = V.find((x) => x.def.eid === eid);
    return v ? (v.def.nickname || 'Unknown') : eid === 0 ? '环境' : String(eid); };
  const div = document.createElement('div');
  div.className = 'kf';
  const causeMap = { 0: '', 1: '（火焰）', 2: '（撞击）', 3: '（环境）', 5: '（溺水）' };
  div.innerHTML = k.killer_eid !== 0
    ? `<span class="k">${esc(name(k.killer_eid))}</span> 击毁 ${esc(name(k.victim_eid))}`
    : `${esc(name(k.victim_eid))}${causeMap[k.cause] || '阵亡'}`;
  $('killfeed').appendChild(div);
  setTimeout(() => div.remove(), 8000);
}
function esc(s) { const d = document.createElement('span'); d.textContent = s; return d.innerHTML; }
function advanceKills() {
  while (killPtr < DATA.kills.length && DATA.kills[killPtr].t <= T) {
    feedEntry(DATA.kills[killPtr]);
    killPtr++;
  }
}
function rebuildFeed() {
  $('killfeed').innerHTML = '';
  const recent = DATA.kills.filter((k) => k.t <= T).slice(-6);
  for (const k of recent) feedEntry(k);
  killPtr = 0;
  while (killPtr < DATA.kills.length && DATA.kills[killPtr].t <= T) killPtr++;
}
function updateScore() {
  let s1 = 0, s2 = 0;
  for (const k of DATA.kills) {
    if (k.t > T) continue;
    const victim = V.find((x) => x.def.eid === k.victim_eid);
    if (!victim) continue;
    if (victim.def.team === 1) s2++; else if (victim.def.team === 2) s1++;
  }
  $('score1').textContent = s1; $('score2').textContent = s2;
}

// ---------- 名册 ----------
function buildRoster() {
  for (const tid of ['team1', 'team2']) $(tid).querySelector('.roster').innerHTML = '';
  for (const v of V) {
    const d = v.def;
    const div = document.createElement('div');
    div.className = 'pl' + (d.team === 0 ? ' unknown' : '');
    div.dataset.eid = d.eid;
    div.innerHTML = `<span class="dot" style="background:#${new THREE.Color(teamColor(v)).getHexString()}"></span>
      <span class="nick">${esc(d.is_author ? '★ ' + (d.nickname || 'Unknown') : (d.nickname || 'Unknown'))}</span>
      <span class="tank">${esc(d.tank_name || (d.tank_id ? 'tank_' + d.tank_id : ''))}</span>
      <span class="hpbar"><i></i></span>`;
    div.addEventListener('click', () => setFollow(d.eid));
    v.rosterEl = div;
    $(d.team === 2 ? 'team2' : 'team1').querySelector('.roster').appendChild(div);
  }
}
function updateRoster() {
  for (const v of V) {
    const hp = hpAt(v, T), dead = deathAt(v, T);
    const frac = v.def.max_hp > 0 ? hp / v.def.max_hp : 0;
    const bar = v.rosterEl.querySelector('.hpbar i');
    const w = Math.round(100 * frac);
    if (bar.dataset.w !== String(w)) { bar.style.width = w + '%'; bar.dataset.w = String(w); }
    v.rosterEl.classList.toggle('dead', dead);
    v.rosterEl.classList.toggle('followed', FOLLOW_EID === v.def.eid);
  }
}

// ---------- 主循环 ----------
const tmpV = new THREE.Vector3();
function applyPose(v) {
  const dead = deathAt(v, T);
  const vis = visibleAt(v, T);
  v.group.visible = vis;
  if (v.glb) v.glb.visible = vis;
  if (!vis) { v.wasDead = false; return; }
  posAt(v, T, tmpV);
  v.group.position.copy(tmpV);
  if (v.glb) poseGlb(v);
  // 标签随镜头距离自适应缩放（近处不遮车、远处仍可读）
  if (v.label) {
    const d = camera.position.distanceTo(v.group.position);
    const s = Math.min(30, Math.max(6, d * 0.16));
    v.label.scale.set(s, s * 0.25, 1);
  }
  // 低模位姿（GLB 显示时保留低模位姿更新，切回低模无跳变）
  v.group.rotation.order = 'YXZ';
  v.group.rotation.y = -yawAt(v, T);
  v.group.rotation.x = arrAt(v.def.hull_pitch, T);
  const rel = wrapPi(turretAbsAt(v, T) - yawAt(v, T));
  v.turretG.rotation.y = -rel;
  v.gunPivot.rotation.x = -gunPitchAt(v, T);
  // 死亡：低模灰化（材质色乘 0.35；复活语义不存在，回放倒带时恢复）
  if (dead !== v.wasDead) {
    v.wasDead = dead;
    v.group.traverse((o) => {
      if (o.isMesh && o.material && o.material.color) {
        if (dead) { o.userData.__c = o.material.color.clone(); o.material.color.multiplyScalar(0.35); }
        else if (o.userData.__c) o.material.color.copy(o.userData.__c);
      }
    });
  }
  drawLabel(v);
}

let winnerShown = false;
function animate() {
  requestAnimationFrame(animate);
  const dt = Math.min(clock.getDelta(), 0.1);
  if (DATA && PLAYING) {
    T += dt * SPEED;
    if (T >= DATA.meta.duration) { T = DATA.meta.duration; setPlaying(false); }
    tick();
  }
  // 相机
  if (DATA && CAM === 'follow' && FOLLOW_EID) {
    const v = V.find((x) => x.def.eid === FOLLOW_EID);
    if (v && v.group.visible) {
      posAt(v, T, tmpV);
      const dir = camera.position.clone().sub(controls.target); dir.y = 0;
      if (dir.lengthSq() < 1) dir.set(0, 0, 1);
      dir.normalize().multiplyScalar(26);
      const target = tmpV.clone();
      controls.target.lerp(target, 0.18);
      const want = target.clone().add(dir).add(new THREE.Vector3(0, 12, 0));
      camera.position.lerp(want, 0.12);
    }
  }
  controls.update();
  renderer.render(scene, camera);
}

function tick() {
  // 弹道推进
  const shots = DATA.shots;
  while (shotPtr < shots.length && shots[shotPtr].t_fire <= T) { spawnShot(shots[shotPtr]); shotPtr++; }
  updateTracers();
  advanceKills();
  for (const v of V) applyPose(v);
  updateRoster(); updateScore();
  // HUD
  $('timer').textContent = gameTimerLabel(T);
  const f = (T - DATA.meta.t_start) / Math.max(0.001, DATA.meta.duration - DATA.meta.t_start);
  if (document.activeElement !== $('seek')) $('seek').value = Math.round(f * 1000);
  $('timeLabel').textContent = `${T.toFixed(1)}s / ${DATA.meta.duration.toFixed(1)}s`;
  if (!winnerShown && T >= DATA.meta.duration - 1e-3 && DATA.meta.winner_team) {
    winnerShown = true;
    const b = $('banner');
    const w = DATA.meta.winner_team, fr = DATA.meta.friendly_team;
    b.textContent = w === 0 ? '平局' : (w === fr ? '胜利' : '失败');
    b.style.color = w === fr ? '#3fa66a' : '#c05046';
    b.style.display = 'block';
  }
}

// ---------- 控制 ----------
function setPlaying(p) {
  PLAYING = p;
  $('playBtn').textContent = p ? '⏸ 暂停' : '▶ 播放';
}
function seekTo(t) {
  T = Math.max(DATA.meta.t_start, Math.min(DATA.meta.duration, t));
  // 重置动态层
  for (const tr of tracers) scene.remove(tr.mesh);
  tracers.length = 0;
  for (const im of impacts) scene.remove(im.g);
  impacts.length = 0;
  shotPtr = 0;
  while (shotPtr < DATA.shots.length && DATA.shots[shotPtr].t_fire <= T) shotPtr++;
  rebuildFeed();
  winnerShown = false; $('banner').style.display = 'none';
  tick();
}
function setFollow(eid) {
  FOLLOW_EID = (FOLLOW_EID === eid) ? 0 : eid;
  if (FOLLOW_EID) { setCam('follow'); } else { setCam('free'); }
}
function setCam(mode) {
  CAM = mode;
  document.querySelectorAll('[data-cam]').forEach((b) =>
    b.classList.toggle('on', b.dataset.cam === mode));
  controls.enabled = true;
  if (mode === 'top') {
    FOLLOW_EID = 0;
    const { cx, cz, ext } = WORLD_CENTER;
    camera.position.set(cx, ext * 1.7, cz + 0.01);
    controls.target.set(cx, 0, cz);
  } else if (mode === 'free') {
    FOLLOW_EID = 0;
  }
}

function initControls() {
  $('playBtn').addEventListener('click', () => setPlaying(!PLAYING));
  const speeds = [0.5, 1, 2, 4, 8, 16];
  const sp = $('speeds');
  for (const s of speeds) {
    const b = document.createElement('button');
    b.textContent = s + 'x';
    if (s === SPEED) b.classList.add('on');
    b.addEventListener('click', () => {
      SPEED = s;
      sp.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x === b));
    });
    sp.appendChild(b);
  }
  $('seek').addEventListener('input', () => {
    if (!DATA) return;
    const f = $('seek').value / 1000;
    seekTo(DATA.meta.t_start + f * (DATA.meta.duration - DATA.meta.t_start));
  });
  document.querySelectorAll('[data-cam]').forEach((b) =>
    b.addEventListener('click', () => setCam(b.dataset.cam)));
  $('glbToggle').addEventListener('change', (e) => applyGlbToggle(e.target.checked));
  $('labelToggle').addEventListener('change', (e) => {
    for (const v of V) v.label.visible = e.target.checked;
  });
  addEventListener('keydown', (e) => {
    if (e.code === 'Space' && DATA) { e.preventDefault(); setPlaying(!PLAYING); }
  });
}

// ---------- 数据加载 ----------
async function loadData(file) {
  $('err').textContent = '';
  const btn = $('loadBtn'); btn.disabled = true; btn.textContent = '解析中…';
  try {
    // 数据端点固定在根路径（/api/playback/data，两模式一致）；
    // P 仅用于 GLB/vendor 静态资产（web 模式 = /armor_view，与装甲查看器共用挂载）
    const resp = await fetch('/api/playback/data', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ file }),
    });
    if (!resp.ok) throw new Error(await resp.text());
    DATA = await resp.json();
    startPlayback();
    $('loader').style.display = 'none';
  } catch (e) {
    $('err').textContent = '加载失败: ' + e.message;
  } finally {
    btn.disabled = false; btn.textContent = '加载';
  }
}
function startPlayback() {
  $('mapName').textContent = DATA.meta.map_name || ('map_' + DATA.meta.map_id);
  buildWorld();
  buildVehicles();
  buildRoster();
  T = DATA.meta.t_start;
  shotPtr = 0; killPtr = 0;
  window.__pbV = V;   // 调试钩子：控制台可查每车 GLB/位姿状态
  setPlaying(true);
  tick();
}

function init() {
  initScene();
  initControls();
  animate();
  const usp = new URLSearchParams(location.search);
  const f = usp.get('file');
  if (f) { $('filePath').value = f; loadData(f); }
  $('loadBtn').addEventListener('click', () => {
    const v = $('filePath').value.trim();
    if (v) loadData(v);
  });
  $('filePath').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') $('loadBtn').click();
  });
}
init();
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// 页面模板替换完整性：占位符不残留
    #[test]
    fn index_html_placeholders_replaced() {
        let html = playback_index_html("/armor_view");
        assert!(!html.contains("__IMPORTMAP__"));
        assert!(!html.contains("__ASSET_PREFIX__"));
        assert!(html.contains("/armor_view/vendor/three/three.module.js"));
        // GLB/vendor 路径由 JS 前缀常量拼接（${P}/glb/...），检查前缀注入
        assert!(html.contains("const P = \"/armor_view\""));
        let html2 = playback_index_html("");
        assert!(html2.contains("\"three\": \"/vendor/three/three.module.js\""));
        assert!(html2.contains("const P = \"\""));
    }
}
