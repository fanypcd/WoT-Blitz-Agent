// P1 分歧验证工具（一次性诊断，不进入提取链）：
//   B1  method27 (0x1b) args[21..33)：terminal_dir = 速度方向向量（对照"位置点"假设）
//   B2  method27 (0x1b) args[4..8)：shell_global_id 稳定性（高位/格式/同射手多值）
//   B4  method35 (0x23)：float1 = 当前生效配置值（对照"倒计时"假设）
//   B5' avatar prop9：炮塔相对偏航镜像（对照"瞄准角/俯仰"假设）
// 用法：cargo run --release --bin verify_p1 -- [回放目录，默认 data/replay_samples]

use std::fs::File;
use wotbreplay_parser::replay::Replay;

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn le_f32(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn vec3(b: &[u8]) -> [f32; 3] {
    [le_f32(b), le_f32(&b[4..]), le_f32(&b[8..])]
}
fn norm(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cos(a: [f32; 3], b: [f32; 3]) -> Option<f32> {
    let (na, nb) = (norm(a), norm(b));
    (na > 1e-9 && nb > 1e-9).then(|| dot(a, b) / (na * nb))
}
fn median(mut v: Vec<f32>) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}
/// prop2 打包角 → 炮塔相对车体偏航 rad（coarse10）
fn prop2_yaw(u: u16) -> f32 {
    ((u >> 6) as f32) / 1024.0 * std::f32::consts::TAU - std::f32::consts::PI
}

fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/replay_samples".into());
    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("wotbreplay"))
        .collect();
    files.sort();

    // ===== 聚合累加器 =====
    let mut b1 = B1::default();
    let mut b2 = B2::default();
    let mut b4 = B4::default();
    let mut b5 = B5::default();

    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let mut replay = Replay::open(File::open(f)?)?;
        let data = replay.read_data()?;
        // (type, clock, payload)
        let pkts: Vec<(u32, f32, &[u8])> = data
            .packets
            .iter()
            .map(|pkt| {
                let t = match &pkt.payload {
                    wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                    wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate {
                        ..
                    } => 0,
                    wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => {
                        *packet_type
                    }
                };
                (t, pkt.clock_secs, &pkt.raw_payload[..])
            })
            .collect();

        // ---- 收集 method29 发射：shotId → (shooter, launchPoint, vel) ----
        let mut launches: std::collections::HashMap<u32, (u32, [f32; 3], [f32; 3])> =
            Default::default();
        // ---- method20 终点：shotId → endPoint ----
        let mut ends: std::collections::HashMap<u32, [f32; 3]> = Default::default();
        // ---- 0x1b 地形命中：shotId → (gid, impact, seg) ----
        let mut terrain: Vec<(u32, u32, [f32; 3], [f32; 3])> = Vec::new();
        // ---- 0x23 装填：entity → [(clock, f1, f2)] ----
        let mut reload35: std::collections::HashMap<u32, Vec<(f32, f32, f32)>> = Default::default();
        // ---- type=7 prop2 / prop9 ----
        let mut prop2: std::collections::HashMap<u32, Vec<(f32, u16)>> = Default::default();
        let mut prop9: Vec<(f32, f32)> = Vec::new(); // avatar (clock, raw f32)

        for (t, clock, p) in &pkts {
            match *t {
                7 if p.len() >= 14 => {
                    let sub = le_u32(&p[4..8]);
                    let eid = le_u32(&p[0..4]);
                    match sub {
                        2 => prop2
                            .entry(eid)
                            .or_default()
                            .push((*clock, u16::from_le_bytes([p[12], p[13]]))),
                        9 if p.len() >= 16 && le_u32(&p[8..12]) == 4 => {
                            prop9.push((*clock, le_f32(&p[12..16])));
                        }
                        _ => {}
                    }
                }
                8 => {
                    if p.len() < 12 {
                        continue;
                    }
                    let m = le_u32(&p[4..8]);
                    let alen = le_u32(&p[8..12]) as usize;
                    if 12 + alen > p.len() {
                        continue;
                    }
                    let a = &p[12..12 + alen];
                    match m {
                        0x1d if alen >= 37 => {
                            launches.insert(
                                le_u32(&a[4..8]),
                                (le_u32(&a[0..4]), vec3(&a[9..21]), vec3(&a[21..33])),
                            );
                        }
                        0x14 if alen >= 16 => {
                            ends.insert(le_u32(&a[0..4]), vec3(&a[4..16]));
                        }
                        0x1b if alen >= 34 => {
                            terrain.push((
                                le_u32(&a[0..4]),
                                le_u32(&a[4..8]),
                                vec3(&a[9..21]),
                                vec3(&a[21..33]),
                            ));
                        }
                        0x23 if alen >= 13 => {
                            reload35.entry(le_u32(&a[0..4])).or_default().push((
                                *clock,
                                le_f32(&a[4..8]),
                                le_f32(&a[8..12]),
                            ));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        println!(
            "=== {}  (launches={} endpoints={} terrain0x1b={} reload0x23={} props2={} props9={})",
            name,
            launches.len(),
            ends.len(),
            terrain.len(),
            reload35.values().map(|v| v.len()).sum::<usize>(),
            prop2.values().map(|v| v.len()).sum::<usize>(),
            prop9.len()
        );

        // ===== B1：seg 是位置还是方向 =====
        let mut impact_eq_end = 0usize;
        for (shot, _gid, impact, seg) in &terrain {
            b1.n += 1;
            b1.norms.push(norm(*seg));
            if let Some(&(shooter, launch, vel)) = launches.get(shot) {
                b1.paired += 1;
                let d_launch = norm(sub(*seg, launch)); // 位置判据：直线弹 ≈ 0
                let d_impact = norm(sub(*seg, *impact)); // 位置判据：直线弹 = 发射点 → seg==launch
                b1.d_launch.push(d_launch);
                if d_launch < 0.5 {
                    b1.pos_like += 1;
                }
                if let Some(c) = cos(*seg, vel) {
                    // 方向判据：cos(seg, 发射速度) ≈ 1
                    b1.cos_vel.push(c);
                    if c > 0.99 {
                        b1.dir_like_vel += 1;
                    }
                }
                if let Some(c) = cos(*seg, sub(*impact, launch)) {
                    b1.cos_ray.push(c);
                    if c > 0.99 {
                        b1.dir_like_ray += 1;
                    }
                }
                let _ = shooter;
                let _ = d_impact;
            }
            if let Some(end) = ends.get(shot) {
                if norm(sub(*impact, *end)) < 0.01 {
                    impact_eq_end += 1;
                }
            }
        }
        b1.impact_eq_end_total += impact_eq_end;
        b1.impact_eq_end_all += 1;

        // ===== B2：gid 稳定性 =====
        for (_shot, gid, _, _) in &terrain {
            b2.n += 1;
            let hi16 = gid >> 16;
            if hi16 != 0 {
                b2.hi16_nonzero.push(*gid);
            }
            let lo8 = gid & 0xFF;
            // 我方 shell_global_id 格式：(局部 id << 8) | 国家基数( nation*16+10 )
            if lo8 % 16 == 10 && (lo8 / 16) <= 8 {
                b2.canonical24 += 1;
            } else {
                b2.non_canonical.push(*gid);
            }
            b2.gids.push(*gid);
        }
        // 同射手 gid 集合（经 method29 联表）
        let mut per_shooter: std::collections::HashMap<u32, std::collections::BTreeSet<u32>> =
            Default::default();
        for (shot, gid, _, _) in &terrain {
            if let Some(&(shooter, _, _)) = launches.get(shot) {
                per_shooter.entry(shooter).or_default().insert(*gid);
            }
        }
        for (sh, gids) in &per_shooter {
            if gids.len() > 1 {
                b2.shooters_multi_gids += 1;
                if b2.sample_multi.is_none() {
                    b2.sample_multi = Some((*sh, gids.iter().copied().collect()));
                }
            }
        }

        // ===== B4：0x23 float1 倒计时 vs 配置值 =====
        for (eid, seq) in &reload35 {
            let mut s = seq.clone();
            s.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            b4.entities += 1;
            b4.events += s.len();
            let distinct: std::collections::BTreeSet<u32> =
                s.iter().map(|x| (x.1 * 1e4) as u32).collect();
            b4.distinct_vals.push(distinct.len());
            // 相邻事件间隔 <0.5s 的对中，float1 递减的比例
            let mut dec = 0usize;
            let mut tot = 0usize;
            for w in s.windows(2) {
                if (w[1].0 - w[0].0) < 0.5 {
                    tot += 1;
                    if w[1].1 < w[0].1 {
                        dec += 1;
                    }
                }
            }
            b4.decreasing_pairs.push((dec, tot));
            if b4.sample.is_none() && s.len() > 3 {
                b4.sample = Some((*eid, s.iter().take(8).map(|x| (x.0, x.1)).collect()));
            }
        }

        // ===== B5'：avatar prop9 = prop2 偏航镜像（rad）还是角度制？ =====
        if !prop9.is_empty() {
            prop9.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            // 每个有 prop2 的实体，与 avatar prop9 最近采样比对（|dt|≤0.05s）
            let mut best: Option<(f32, u32, usize)> = None; // (median|Δ|, eid, n)
            for (eid, series) in &prop2 {
                let mut diffs: Vec<f32> = Vec::new();
                for (c, u) in series.iter() {
                    let nearest = prop9
                        .iter()
                        .min_by_key(|(c9, _)| (((c9 - c).abs()) * 1000.0) as u32);
                    if let Some(&(c9, raw)) = nearest {
                        if (c9 - c).abs() <= 0.05 {
                            let yaw = prop2_yaw(*u);
                            // 两种解释：raw 视作 rad（WotbTools）/ 视作角度→rad（我方旧 to_radians 路径）
                            diffs.push((raw - yaw).abs().rem_euclid(std::f32::consts::TAU).min(
                                std::f32::consts::TAU
                                    - (raw - yaw).abs().rem_euclid(std::f32::consts::TAU),
                            ));
                        }
                    }
                }
                if diffs.len() >= 50 {
                    let n = diffs.len();
                    let m = median(diffs);
                    if best.is_none_or(|(bm, _, _)| m < bm) {
                        best = Some((m, *eid, n));
                    }
                }
            }
            if let Some((m, eid, n)) = best {
                b5.best_median.push(m);
                println!("  B5' 最匹配实体 eid={eid} n={n} prop9(raw as rad) vs prop2yaw median|Δ|={:.4} rad ({:.2}°)", m, m.to_degrees());
            }
            // 角度制解释的量纲 sanity：raw 值域
            let mut raws: Vec<f32> = prop9.iter().map(|x| x.1).collect();
            let (mn, mx) = raws
                .len()
                .checked_sub(1)
                .map(|i| {
                    raws.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    (raws[0], raws[i])
                })
                .unwrap_or((0.0, 0.0));
            println!(
                "  B5' prop9 raw 值域 [{:.4}, {:.4}]（rad 解释值域应为 ±π≈±3.14；角度制应为 ±180）",
                mn, mx
            );
        }

        // ===== 提取链冒烟：无锚定表 → 作者俯仰全走 method36 field2 回退链 =====
        let br = replay.read_battle_results().ok();
        let author_nick = br
            .as_ref()
            .and_then(|br| {
                br.players
                    .iter()
                    .find(|p| p.account_id == br.author.account_id)
                    .map(|p| p.info.nickname.clone())
            })
            .or_else(|| replay.read_meta().ok().map(|m| m.player_name.clone()))
            .unwrap_or_default();
        let pitch_limits: std::collections::HashMap<
            String,
            wotb_agent::replay::combat::GunPitchRange,
        > = Default::default();
        let author_shots: Vec<wotb_agent::replay::combat::ShotReplayData> =
            match wotb_agent::replay::combat::extract_shot_replays_auto_with_limits(
                &pkts,
                &author_nick,
                &pitch_limits,
            ) {
                Ok(shots) => shots,
                Err(e) => {
                    println!("  冒烟: 作者路径失败: {e}");
                    Vec::new()
                }
            };
        {
            let shots = &author_shots;
            let with_aim_pitch = shots
                .iter()
                .filter(|s| s.shooter_aim.as_ref().and_then(|a| a.gun_pitch).is_some())
                .count();
            let m36_fallback = shots
                .iter()
                .filter(|s| {
                    s.quality
                        .as_ref()
                        .map(|q| q.shooter_pitch_from_method36)
                        .unwrap_or(false)
                })
                .count();
            println!(
                "  冒烟: 作者路径 {} 发（shooter_aim 带俯仰 {}/{}，method36 回退 {} 发）",
                shots.len(),
                with_aim_pitch,
                shots.len(),
                m36_fallback
            );
            let eq = shots
                .iter()
                .filter(|s| s.shooter_equipment.is_some())
                .count();
            let tgt_eq = shots
                .iter()
                .filter(|s| s.target_equipment.is_some())
                .count();
            let cal = shots
                .iter()
                .filter(|s| {
                    s.shooter_equipment
                        .as_ref()
                        .map(|e| e.calibrated_shells)
                        .unwrap_or(false)
                })
                .count();
            let enh = shots
                .iter()
                .filter(|s| {
                    s.target_equipment
                        .as_ref()
                        .map(|e| e.enhanced_armor)
                        .unwrap_or(false)
                })
                .count();
            println!(
                "  C4: 射手配件 {}/{} 发（校准弹 {}）· 目标配件 {}/{} 发（强化装甲 {}）",
                eq,
                shots.len(),
                cal,
                tgt_eq,
                shots.len(),
                enh
            );
            if let Some(ex) = shots.iter().find(|s| s.shooter_equipment.is_some()) {
                println!(
                    "  C4: 作者 9 槽 = {:?}",
                    ex.shooter_equipment.as_ref().unwrap().raw
                );
            }
        }
        let author_eid =
            wotb_agent::replay::combat::resolve_author_player_eid_by_nick(&pkts, &author_nick);
        let others = wotb_agent::replay::combat::extract_other_shot_replays_with_limits(
            &pkts,
            author_eid,
            &pitch_limits,
        );
        println!(
            "  冒烟: 他人路径 {} 发（总发射 {}，跳过无终点 {}）",
            others.shots.len(),
            others.total_launches,
            others.skipped_no_endpoint
        );

        // C4 诊断：作者实体 type=5 包逐条检查 loadout framing
        let eqmap = wotb_agent::replay::combat::collect_vehicle_equipment(&pkts);
        println!(
            "  C4诊断: 作者eid={:#x} 装备={}, 全场type5含loadout的实体数={}",
            author_eid,
            eqmap.contains_key(&author_eid),
            eqmap.len()
        );
        for (_, clock, p) in pkts.iter().filter(|(t, _, _)| *t == 5) {
            let eid = le_u32(&p[0..4]);
            if eid != author_eid {
                continue;
            }
            let has_a6 = p.windows(2).any(|w| w[0] == 0x0A && w[1] == 0x06);
            let has_b9 = p.windows(2).any(|w| w[0] == 0x0B && w[1] == 0x09);
            println!(
                "  C4诊断: type5 clock={:.2} len={} 含0A06={} 含0B09={}",
                clock,
                p.len(),
                has_a6,
                has_b9
            );
            // 结构深挖：0B 09 前后各 dump 若干字节；找 0A XX 计数标记
            if let Some(pos) = p.windows(2).position(|w| w[0] == 0x0B && w[1] == 0x09) {
                let s0 = pos.saturating_sub(24);
                let hex: Vec<String> = p[s0..(pos + 11).min(p.len())]
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect();
                println!(
                    "  C4诊断: 0B09@{} 上下文(前24B..后9B): {}",
                    pos,
                    hex.join(" ")
                );
                for k in 0..pos {
                    if p[k] == 0x0A && k + 1 < pos && p[k + 1] <= 10 && p[k + 1] >= 3 {
                        println!(
                            "  C4诊断: 候选计数标记 @{}: 0A {:02X} ({} items?)",
                            k,
                            p[k + 1],
                            p[k + 1]
                        );
                    }
                }
            }
        }

        // P2-1 结算补充字段验证（crate 未暴露的 #301 字段）
        if let Ok(dat) = replay.read_battle_results_dat() {
            let st =
                wotb_agent::wargaming::battle_results_extra::parse_settlement_extras(&dat.buffer);
            let alive = st.iter().filter(|s| s.death_reason == Some(-1)).count();
            let fired = st.iter().filter(|s| s.death_reason == Some(1)).count();
            let rammed = st.iter().filter(|s| s.death_reason == Some(2)).count();
            let world = st.iter().filter(|s| s.death_reason == Some(3)).count();
            let normal = st.iter().filter(|s| s.death_reason.is_none()).count();
            let with_marks = st.iter().filter(|s| s.gun_marks.unwrap_or(0) > 0).count();
            let sample = st.iter()
                .find(|s| s.death_reason.is_some() && s.death_reason != Some(-1))
                .map(|s| format!("acct={} death={:?} life={:?} killer={:?} spotted={:?} dstAssist={:?} marks={:?}",
                    s.account_id, s.death_reason, s.life_time_secs, s.killer_id, s.n_enemies_spotted, s.destruction_assistance, s.gun_marks))
                .unwrap_or_else(|| "（全场无阵亡条目）".into());
            println!("  P2-1: 结算 {} 条（存活 {alive} / 火 {fired} / 撞 {rammed} / 世界 {world} / 普通击毁 {normal}），有炮印 {with_marks} 人", st.len());
            println!("  P2-1: 阵亡样例 {sample}");
        }

        // P2-2 wrapper6 击杀播报验证：kill feed vs 结算死亡数互验
        {
            let ups = wotb_agent::replay::combat::collect_arena_updates(&pkts);
            let mut hist = std::collections::BTreeMap::new();
            for u in &ups {
                *hist.entry(u.subtype).or_insert(0usize) += 1;
            }
            println!(
                "  P2-2调试: arena updates {} 条子类型分布 {:?}",
                ups.len(),
                hist
            );
        }
        for u in wotb_agent::replay::combat::collect_arena_updates(&pkts)
            .iter()
            .filter(|u| u.subtype == 6)
            .take(3)
        {
            let hex: String = u.payload.iter().map(|b| format!("{b:02x}")).collect();
            println!("  P2-2调试: s6 t={:.2} payload={}", u.clock, hex);
        }
        let kf = wotb_agent::replay::combat::collect_kill_feed(&pkts);
        let post = kf.iter().filter(|k| k.clock > 10.0).count();
        let with_assist = kf.iter().filter(|k| k.assister_eid.is_some()).count();
        let with_reason = kf.iter().filter(|k| k.death_reason.is_some()).count();
        println!("  P2-2: 击杀播报 {} 条（clock>10s: {post}，带助攻 {with_assist}，带死因 {with_reason}）", kf.len());
        for k in kf.iter().filter(|k| k.death_reason.is_some()).take(2) {
            println!(
                "  P2-2: 样例 victim={:#x} killer={:#x} assist={:?} reason={:?} t={:.2}s",
                k.victim_eid, k.killer_eid, k.assister_eid, k.death_reason, k.clock
            );
        }

        // P2-3 AoI 生命周期验证：Type33:Type5 配对 / Type4 重入 / 段统计
        let aoi = wotb_agent::replay::combat::collect_aoi_lifecycle(&pkts);
        let closed = aoi.iter().filter(|a| a.t_out.is_some()).count();
        let mut eids: std::collections::BTreeSet<u32> = Default::default();
        for a in &aoi {
            eids.insert(a.eid);
        }
        let multi = eids
            .iter()
            .filter(|e| aoi.iter().filter(|a| a.eid == **e).count() > 1)
            .count();
        println!(
            "  P2-3: AoI 段 {}（Type4 关闭 {}，重入实体 {}），在场实体 {}",
            aoi.len(),
            closed,
            multi,
            eids.len()
        );

        // P2-4 type39 作者炮线验证：f0/f1 vs method29 发射方向；f6 vs method36 field2
        {
            let frames = wotb_agent::replay::combat::collect_type39_frames(&pkts);
            let mut errs_yaw: Vec<f32> = Vec::new();
            let mut errs_pitch: Vec<f32> = Vec::new();
            let mut errs_f6: Vec<f32> = Vec::new();
            let mut f6_sign_flip = 0usize;
            let mut f6_total = 0usize;
            for sh in author_shots.iter() {
                let ft = sh.time_s;
                // 射线方向：ball_a→ball_b（miss 用 terrain/endpoint；ball_b 恒有）
                let dir = [
                    sh.ball_b[0] - sh.ball_a[0],
                    sh.ball_b[1] - sh.ball_a[1],
                    sh.ball_b[2] - sh.ball_a[2],
                ];
                let dnorm = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
                if dnorm < 1.0 {
                    continue;
                }
                let want_yaw = dir[0].atan2(dir[2]);
                let want_pitch = (dir[1] / dnorm).asin();
                let Some(fr) = frames
                    .iter()
                    .min_by_key(|f2| (((f2.clock - ft).abs()) * 1000.0) as u32)
                else {
                    continue;
                };
                if (fr.clock - ft).abs() > 0.15 {
                    continue;
                }
                let dy = (fr.gun_yaw - want_yaw).rem_euclid(std::f32::consts::TAU);
                let dy = dy.min(std::f32::consts::TAU - dy);
                errs_yaw.push(dy);
                errs_pitch.push((fr.gun_pitch_world - want_pitch).abs());
                if let Some(aim) = &sh.shooter_aim {
                    if let Some(p2) = aim.gun_pitch {
                        f6_total += 1;
                        let d_same = (fr.gun_pitch_local - p2 as f32).abs();
                        let d_flip = (fr.gun_pitch_local + p2 as f32).abs();
                        if d_flip < d_same {
                            f6_sign_flip += 1;
                        }
                        errs_f6.push(d_same.min(d_flip));
                    }
                }
            }
            let med = |mut v: Vec<f32>| {
                if v.is_empty() {
                    return f32::NAN;
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v[v.len() / 2]
            };
            println!("  P2-4: type39 帧数 {}；作者射击对齐 {} 发：f0 yaw 误差 median {:.2}°、f1 pitch 误差 median {:.2}°（WotbTools 锚定 0.27°/0.45°）",
                frames.len(), errs_yaw.len(), med(errs_yaw).to_degrees(), med(errs_pitch).to_degrees());
            println!(
                "  P2-4: f6 vs method36 field2：{} 对，median {:.4} rad（翻转占 {}/{}）",
                errs_f6.len(),
                med(errs_f6),
                f6_sign_flip,
                f6_total
            );
        }
    }

    // ===== 汇总裁决 =====
    println!("\n================ B1: 0x1b args[21..33) 位置 vs 方向 ================");
    println!("0x1b 总数 {}，与 method29 配对 {}", b1.n, b1.paired);
    println!(
        "位置判据 |seg − launch| < 0.5m：{}/{} 配对样本",
        b1.pos_like, b1.paired
    );
    println!(
        "方向判据 cos(seg, vel) > 0.99：{}/{}；cos(seg, end−launch) > 0.99：{}/{}",
        b1.dir_like_vel, b1.paired, b1.dir_like_ray, b1.paired
    );
    println!("norm(seg)：median={:.3} min={:.3} max={:.3}（方向说预测 0.18..218 量级散布；位置说 ≈ |launch| 同量级）",
        median(b1.norms.clone()),
        b1.norms.iter().cloned().fold(f32::INFINITY, f32::min),
        b1.norms.iter().cloned().fold(f32::NEG_INFINITY, f32::max));
    println!(
        "|seg−launch| median={:.4} m；cos(seg,vel) median={:.6}",
        median(b1.d_launch.clone()),
        median(b1.cos_vel.clone())
    );
    println!(
        "impact_point == method20 终点（<1cm）：{}/{} 文件合计",
        b1.impact_eq_end_total, b1.impact_eq_end_all
    );

    println!("\n================ B2: 0x1b args[4..8) gid 稳定性 ================");
    println!("gid 总数 {}", b2.n);
    println!(
        "高 16 位非零：{}（WotbTools 语料存在 0x0008/0x0001 漂移）",
        b2.hi16_nonzero.len()
    );
    println!(
        "低 8 位符合国家基数格式（×16+10）：{}/{}",
        b2.canonical24, b2.n
    );
    if !b2.hi16_nonzero.is_empty() {
        println!(
            "  高位非零样例：{:08X?}...",
            &b2.hi16_nonzero[..b2.hi16_nonzero.len().min(5)]
        );
    }
    if !b2.non_canonical.is_empty() {
        println!(
            "  非规范格式样例：{:08X?}...",
            &b2.non_canonical[..b2.non_canonical.len().min(5)]
        );
    }
    println!("同射手出现 >1 种 gid 的射手数：{}", b2.shooters_multi_gids);
    if let Some((sh, gids)) = &b2.sample_multi {
        println!("  样例 shooter={sh:#x} gids={:08X?}", gids);
    }
    let mut uniq = b2.gids.clone();
    uniq.sort();
    uniq.dedup();
    println!("全部 gid 去重后 {} 种", uniq.len());
    let mut low24: Vec<u32> = b2.gids.iter().map(|g| g & 0xFF_FFFF).collect();
    low24.sort();
    low24.dedup();
    let mut low16: Vec<u32> = b2.gids.iter().map(|g| g & 0xFFFF).collect();
    low16.sort();
    low16.dedup();
    let mut byte2_of_low16: std::collections::HashMap<u32, std::collections::BTreeSet<u32>> =
        Default::default();
    for g in &b2.gids {
        byte2_of_low16
            .entry(g & 0xFFFF)
            .or_default()
            .insert((g >> 16) & 0xFF);
    }
    let multi_byte2 = byte2_of_low16.values().filter(|s| s.len() > 1).count();
    println!("去重对比：full={} low24={} low16={}；同 low16 出现多个 byte2 的 low16 数 = {}（=0 则 byte2 纯噪声，&0xFFFF 掩码安全）",
        uniq.len(), low24.len(), low16.len(), multi_byte2);

    println!("\n================ B4: 0x23 float1 倒计时 vs 配置值 ================");
    println!(
        "实体数 {}，事件总数 {}（平均 {:.1}/实体）",
        b4.entities,
        b4.events,
        if b4.entities > 0 {
            b4.events as f32 / b4.entities as f32
        } else {
            0.0
        }
    );
    println!(
        "每实体 distinct 值数 median={:.0}（配置说：少数几个离散值反复；倒计时说：≈事件数）",
        median(b4.distinct_vals.iter().map(|x| *x as f32).collect())
    );
    let (mut dec, mut tot) = (0usize, 0usize);
    for (d, t) in &b4.decreasing_pairs {
        dec += d;
        tot += t;
    }
    println!(
        "<0.5s 相邻事件对中 float1 递减占比：{}/{} = {:.1}%（倒计时说应≈100%）",
        dec,
        tot,
        if tot > 0 {
            dec as f32 / tot as f32 * 100.0
        } else {
            0.0
        }
    );
    if let Some((eid, s)) = &b4.sample {
        println!(
            "样例 entity={eid:#x} 前 8 条 (clock, f1)：{:?}",
            s.iter()
                .map(|(c, v)| format!("({:.2}, {:.3})", c, v))
                .collect::<Vec<_>>()
        );
    }
    println!("\n（B1 判据：pos_like 占多数且 norm 同 |launch| 量级 → 我方位置说；dir_like 占多数且 norm 散布 0.18..218 → WotbTools 方向说。");
    println!("  B4 判据：递减占比高 + distinct≈事件数 → 我方倒计时说；distinct 少 + 事件稀疏 → WotbTools 配置说。）");
    Ok(())
}

#[derive(Default)]
struct B1 {
    n: usize,
    paired: usize,
    pos_like: usize,
    dir_like_vel: usize,
    dir_like_ray: usize,
    norms: Vec<f32>,
    d_launch: Vec<f32>,
    cos_vel: Vec<f32>,
    cos_ray: Vec<f32>,
    impact_eq_end_total: usize,
    impact_eq_end_all: usize,
}

#[derive(Default)]
struct B2 {
    n: usize,
    canonical24: usize,
    hi16_nonzero: Vec<u32>,
    non_canonical: Vec<u32>,
    gids: Vec<u32>,
    shooters_multi_gids: usize,
    sample_multi: Option<(u32, Vec<u32>)>,
}

#[derive(Default)]
struct B4 {
    entities: usize,
    events: usize,
    distinct_vals: Vec<usize>,
    decreasing_pairs: Vec<(usize, usize)>,
    sample: Option<(u32, Vec<(f32, f32)>)>,
}

#[derive(Default)]
struct B5 {
    best_median: Vec<f32>,
}
