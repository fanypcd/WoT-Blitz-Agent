//! 真实样本冒烟（契约 v2 能力边界）：客户端路径（无 resolver/俯仰锚定/地图注册表）
//! 产出的独立能力满足基本不变量。样本取 data/replay_samples 最大的 .wotbreplay
//!（整场对战）；无样本环境跳过。
//!
//! 评审验收对映：
//! 1. Result-only parse 不物化 Playback、不依赖 HoF facet —— `result_smoke`：
//!    输出无 vehicles/时序键、体积比 Playback 小两个量级；
//! 2. Agent 公开面无 HoF —— 编译级保证（`HofFacet` 已删除，giant envelope 已拆除）。

use std::path::PathBuf;

fn largest_sample() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/replay_samples");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "wotbreplay").unwrap_or(false))
        .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
}

/// 结果能力：BattleSummary 齐备，且**不含**时序物化（毫秒级通道的边界不变量）。
#[test]
fn result_smoke() {
    let Some(path) = largest_sample() else {
        eprintln!("无回放样本，跳过");
        return;
    };
    eprintln!("样本: {}", path.display());
    let bytes = std::fs::read(&path).unwrap();

    let result: serde_json::Value = serde_json::from_str(
        &wotb_replay_wasm::result_json(&bytes, None).expect("结果能力构建成功"),
    )
    .unwrap();
    let players = result["players"].as_array().unwrap().len();
    assert!((8..=28).contains(&players), "花名册 {players}");
    assert!(
        result["author_account_id"].as_u64().unwrap() > 0,
        "作者在册"
    );
    assert!(result["winner_team"]
        .as_u64()
        .map(|w| w == 1 || w == 2)
        .unwrap_or(false));
    // 时序物化键不得出现在结果通道（PlaybackData 专属键）
    for key in ["vehicles", "shots", "kills", "periods", "visibility"] {
        assert!(result.get(key).is_none(), "结果能力不得物化时序键 {key}");
    }
}

/// 时序能力：PlaybackData 齐备；体积与结果通道的量级差证明两者独立（非信封捆绑）。
#[test]
fn playback_smoke() {
    let Some(path) = largest_sample() else {
        eprintln!("无回放样本，跳过");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();

    let playback_str = wotb_replay_wasm::playback_json(&bytes, None, None).expect("时序能力构建成功");
    let pb: serde_json::Value = serde_json::from_str(&playback_str).unwrap();
    assert_eq!(pb["version"], 2, "contract v2（版本门禁；消费端拒绝错版）");
    // contract v2 新键：非争霸场为空数组也必须安全序列化在场（skip_serializing_if 语义）
    for key in ["supremacy_bases", "supremacy_points"] {
        assert!(
            pb.get(key).is_none_or(|v| v.is_array()),
            "v2 键 {key} 须为数组或缺省"
        );
    }
    // aim_frames 已从契约移除（无消费方；见契约文档 §3）
    assert!(
        pb.get("aim_frames").is_none(),
        "aim_frames 已从契约移除，不得再序列化"
    );
    let nv = pb["vehicles"].as_array().unwrap().len();
    assert!((8..=28).contains(&nv), "车辆数 {nv}");
    assert!(
        pb["meta"]["samples"].as_u64().unwrap() > 600,
        "整场网格过短"
    );

    // 结果通道（毫秒级）输出体积必须比全场时序小两个量级——Result-only 消费
    // 不被迫物化 ~MB 级 Playback（契约 v2 拆分动机）
    let result_str = wotb_replay_wasm::result_json(&bytes, None).unwrap();
    assert!(
        result_str.len() * 100 < playback_str.len(),
        "result {}B vs playback {}B——量级分离失效",
        result_str.len(),
        playback_str.len()
    );
}

/// 跨 facet 身份不变量：Playback 中每个有昵称的 observed vehicle 必须能在
/// Result 花名册找到同名玩家，且 account_id/team/tank_id 完全一致（Result 为
/// oracle——昵称联表 SSOT 在模型 scan 一次完成；ASCII 过滤时代中文昵称车辆
/// team=0 且无身份，本测试即回归锚）。不做全局车辆数断言：single-POV/AoI 下
/// 整场未观察到的敌人合法缺席。
#[test]
fn playback_result_identity_invariant() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/replay_samples");
    let Some(rd) = std::fs::read_dir(&dir).ok() else {
        return;
    };
    let mut checked = 0;
    for path in rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "wotbreplay").unwrap_or(false))
    {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(result_str) = wotb_replay_wasm::result_json(&bytes, None) else {
            continue;
        };
        let Ok(playback_str) = wotb_replay_wasm::playback_json(&bytes, None, None) else {
            eprintln!("跳过（非整场/片段）: {}", path.display());
            continue;
        };
        let result: serde_json::Value = serde_json::from_str(&result_str).unwrap();
        let pb: serde_json::Value = serde_json::from_str(&playback_str).unwrap();

        let roster: std::collections::HashMap<&str, &serde_json::Value> = result["players"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["nickname"].as_str().unwrap_or(""), p))
            .collect();
        let mut named = 0;
        for v in pb["vehicles"].as_array().unwrap() {
            let Some(nick) = v["nickname"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let player = roster.get(nick).unwrap_or_else(|| {
                panic!(
                    "{}: observed 昵称 {nick:?} 不在 Result 花名册",
                    path.display()
                )
            });
            assert_eq!(
                v["account_id"], player["account_id"],
                "{nick} account_id 联表一致"
            );
            assert_eq!(v["team"], player["team"], "{nick} team 联表一致");
            assert_eq!(v["tank_id"], player["tank_id"], "{nick} tank_id 联表一致");
            assert!(
                v["team"].as_u64().unwrap() == 1 || v["team"].as_u64().unwrap() == 2,
                "{nick} team 必须是 1/2（联表成功），不得为 0"
            );
            named += 1;
        }
        eprintln!(
            "--- {}: {} 车全部通过（具名 {named}）",
            path.display(),
            pb["vehicles"].as_array().unwrap().len()
        );
        checked += 1;
    }
    assert!(checked > 0, "至少一个可解析样本");
}

/// 射击复现通道：弹种反解注入不变量——`shells_json`（dump-shell-kinds 富表）
/// 注入后带 shell_id 的弹全部补齐 `shell_kind` 与 `shell`（type/穿深一致）；
/// 缺省表时 shell_kind 为空串、无 shell 字段（数据可得性边界）。
#[test]
fn shot_replays_shell_injection_smoke() {
    // 样本须含已知全局弹种 id 的对局：GB13_FV215b 场（作者/他人均发 18010 APCR）
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/replay_samples");
    let path = std::fs::read_dir(&dir).ok().and_then(|rd| {
        rd.flatten().map(|e| e.path()).find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().contains("FV215b"))
                .unwrap_or(false)
        })
    });
    let Some(path) = path else {
        eprintln!("无 FV215b 样本，跳过");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();

    // 表来源与 dump-shell-kinds 同形状（现取 tanks.pb 全量展开；wasm 测试环境
    // 无 CLI，直接用上游 annotate 同款构建入口——src 侧 ShellKindTable 不可达，
    // 此处以最小内联表覆盖断言即可，全域覆盖由 dump CLI 产物保证）
    let bare: serde_json::Value =
        serde_json::from_str(&wotb_replay_wasm::shot_replays_json(&bytes, None, None).unwrap())
            .unwrap();
    // 契约 v0.1.9：包装对象 + fail-visible 诊断（作者严格路径健康时无 author_error）
    let shots = bare["shots"].as_array().expect("shots 数组");
    assert_eq!(bare["author_path"], "ok", "健康样本作者路径应 ok");
    assert!(
        bare.get("author_error").is_none(),
        "ok 态不得携带 author_error"
    );
    assert!(
        bare["author_eid"].as_u64().unwrap_or(0) > 0,
        "作者 eid 已解析"
    );
    assert!(
        bare["others"]["total_launches"].is_u64(),
        "他人路径统计在场"
    );
    assert!(!shots.is_empty(), "样本应含射击事件");
    for s in shots {
        assert!(s.get("shell").is_none(), "缺省表不得输出 shell 字段");
        if s["shell_id"].as_u64().unwrap_or(0) > 0 {
            assert_eq!(
                s["shell_kind"].as_str().unwrap_or(""),
                "",
                "缺省表 kind 恒空"
            );
        }
    }

    // 内联最小富表（FV215b APCR = 0x465a = 18010；样本含该弹——GB13_FV215b 场）
    let table = r#"{"18010":{"type":"ap_cr_premium","penetration":326,"damage":340,"module_damage":165,"explosion_radius":0}}"#;
    let injected: serde_json::Value = serde_json::from_str(
        &wotb_replay_wasm::shot_replays_json(&bytes, None, Some(table)).unwrap(),
    )
    .unwrap();
    let mut resolved = 0;
    for s in injected["shots"].as_array().unwrap() {
        if s["shell_id"].as_u64() != Some(18010) {
            continue;
        }
        resolved += 1;
        assert_eq!(s["shell_kind"], "ap_cr_premium", "kind 反解");
        assert_eq!(s["shell"]["penetration"], 326, "穿深注入");
        assert_eq!(s["shell"]["damage"], 340, "伤害注入");
    }
    assert!(resolved > 0, "样本应含 18010 弹（表注入生效的先验）");
}

/// P0 结算字段（WotBTools 名人堂/联赛评分阻断项）：arena_id（**字符串**，值可超
/// JS 安全整数）、arena_bonus_type（meta.json 原始数值）、damage_received（玩家级，
/// #301 f11，缺省 0 为真实语义）。
#[test]
fn result_p0_settlement_fields_smoke() {
    let Some(path) = largest_sample() else {
        eprintln!("无回放样本，跳过");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let r: serde_json::Value =
        serde_json::from_str(&wotb_replay_wasm::result_json(&bytes, None).unwrap()).unwrap();

    // arena_id：必须是字符串（u64 超 JS 安全整数，禁止走 number）
    let aid = r.get("arena_id").and_then(|v| v.as_str());
    assert!(
        aid.is_some(),
        "arena_id 应为字符串（got {:?}）",
        r.get("arena_id")
    );
    assert!(!aid.unwrap().is_empty());
    assert!(
        aid.unwrap().chars().all(|c| c.is_ascii_digit()),
        "arena_id 应为十进制数字串"
    );

    // arena_bonus_type：meta.json 原始数值
    assert!(
        r.get("arena_bonus_type")
            .map(|v| v.is_u64())
            .unwrap_or(false),
        "arena_bonus_type 应为数值（got {:?}）",
        r.get("arena_bonus_type")
    );

    // damage_received：每个玩家都有（缺省 0）
    let players = r["players"].as_array().unwrap();
    assert!(!players.is_empty());
    for p in players {
        assert!(
            p.get("damage_received")
                .map(|v| v.is_u64())
                .unwrap_or(false),
            "玩家 {} 缺 damage_received",
            p.get("nickname").unwrap_or(&serde_json::json!("?"))
        );
    }
    eprintln!(
        "P0 字段: arena_id={:?} bonus_type={:?} 玩家数={}",
        aid.unwrap(),
        r.get("arena_bonus_type"),
        players.len()
    );
}

/// P1 结算字段：finish_reason / result_duration_secs / client_version（结算级）
/// 与 victory_points_earned/seized / hitpoints_left / rank（玩家级）。
#[test]
fn result_p1_settlement_fields_smoke() {
    let Some(path) = largest_sample() else {
        eprintln!("无回放样本，跳过");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let r: serde_json::Value =
        serde_json::from_str(&wotb_replay_wasm::result_json(&bytes, None).unwrap()).unwrap();

    // 结算级
    let fr = r.get("finish_reason").and_then(|v| v.as_u64());
    assert!(fr.is_some(), "finish_reason 应存在（root f4）");
    let dur = r.get("result_duration_secs").and_then(|v| v.as_u64());
    assert!(dur.is_some(), "result_duration_secs 应存在（root f5）");
    assert!(
        (1..=3600).contains(&dur.unwrap()),
        "结算时长为整秒且应在合理区间（got {dur:?}）"
    );
    let cv = r.get("client_version").and_then(|v| v.as_str());
    assert!(cv.is_some(), "client_version 应存在（data.wotreplay 头）");
    assert!(
        cv.unwrap().chars().next().unwrap().is_ascii_digit(),
        "版本串应形如 11.20.0（got {cv:?}）"
    );

    // 玩家级：hitpoints_left 全玩家在册（f1）
    let players = r["players"].as_array().unwrap();
    for p in players {
        assert!(
            p.get("hitpoints_left").is_some(),
            "玩家缺 hitpoints_left（应全玩家输出）"
        );
        // victory_points / rank 允许缺失（非相关模式/版本），但键必须存在（Option 序列化为 null）
        assert!(p.get("victory_points_earned").is_some());
        assert!(p.get("rank").is_some());
    }
    let vp = players
        .iter()
        .filter(|p| p["victory_points_earned"].is_u64())
        .count();
    let rk = players.iter().filter(|p| p["rank"].is_u64()).count();
    eprintln!(
        "P1: finish_reason={fr:?} duration={dur:?}s client_version={cv:?} \
               玩家={} 有点数者={vp} 有段位者={rk}",
        players.len()
    );
}

/// P2 注入与第 4 入口（契约 v0.3.1）：
/// - `parseResult` / `parsePlayback` 的 `tankNamesJson` 可选注入；
/// - `parseAiReview` 从弹道自带 `target_eid` 取受击方身份（不按昵称反查）。
#[test]
fn p2_tank_names_injection_and_ai_review_entry() {
    let Some(path) = largest_sample() else {
        eprintln!("无回放样本，跳过");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();

    // 取作者 tank_id，构造 {tank_id: name} 注入表
    let base: serde_json::Value =
        serde_json::from_str(&wotb_replay_wasm::result_json(&bytes, None).unwrap()).unwrap();
    let tid = base["author_tank_id"].as_u64().unwrap();
    assert!(tid > 0, "作者坦克 id 应非零");
    let table = format!(r#"{{"{tid}":"INJECTED_TANK"}}"#);

    // 结果通道：注入后作者与玩家行的 tank_name 命中项被替换
    let inj: serde_json::Value =
        serde_json::from_str(&wotb_replay_wasm::result_json(&bytes, Some(&table)).unwrap())
            .unwrap();
    assert_eq!(
        inj["author_tank_name"], "INJECTED_TANK",
        "注入应替换作者车型名"
    );
    let hits = inj["players"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["tank_id"].as_u64() == Some(tid))
        .all(|p| p["tank_name"] == "INJECTED_TANK");
    assert!(hits, "同 tank_id 的玩家行也应替换");
    // 未注入时是 `tank_{id}` 而非空串（文档与实现统一后的实测行为）
    let miss = inj["players"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["tank_id"].as_u64() != Some(tid))
        .map(|p| p["tank_name"].as_str().unwrap_or("").to_string())
        .unwrap_or_default();
    assert!(
        miss.starts_with("tank_"),
        "未注入项在客户端路径（无 resolver）应为 tank_{{id}} 而非空串，实测：{miss:?}"
    );

    // 时序通道：注入后 vehicles[].tank_name 命中项被替换
    let pb: serde_json::Value =
        serde_json::from_str(&wotb_replay_wasm::playback_json(&bytes, Some(&table), None).unwrap())
            .unwrap();
    let vhit = pb["vehicles"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["tank_id"].as_u64() == Some(tid))
        .all(|v| v["tank_name"] == "INJECTED_TANK");
    assert!(vhit, "playback 注入应替换对应车辆的 tank_name");

    // 第 4 入口：AiReviewFacet 形状 + Shot 身份域
    let ai: serde_json::Value =
        serde_json::from_str(&wotb_replay_wasm::ai_review_json(&bytes).unwrap()).unwrap();
    assert_eq!(ai["version"], 1, "DTO 冻结 v1");
    for key in ["battle", "rosters", "events", "settlements"] {
        assert!(ai.get(key).is_some(), "缺键 {key}");
    }
    let events = ai["events"].as_array().unwrap();
    assert!(!events.is_empty(), "事件流非空");
    // 事件按时钟升序（Shot 用 t）
    let mut last = f64::NEG_INFINITY;
    for e in events {
        let t = e["t"]
            .as_f64()
            .or_else(|| e["t_in"].as_f64())
            .unwrap_or(0.0);
        assert!(t >= last - 1e-6, "事件流应按时钟升序");
        last = t;
    }
    // Shot 事件的 hit 由 target_eid 存在性决定（身份域与显示域解耦）
    for e in events.iter().filter(|e| e["type"] == "shot") {
        assert_eq!(
            e["hit"].as_bool().unwrap(),
            e.get("target_eid").is_some(),
            "hit 应与 target_eid 存在性一致"
        );
    }
}
