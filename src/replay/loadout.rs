//! 玩家开局配置与弹种解析（loadout）
//!
//! - [`ShellKindTable`]：tanks.pb 全量展开的"全局弹种 id → shell_type 原始串"映射，
//!   为提取链的每发射击回填 `ShotReplayData.shell_kind`（作者+他人统一）。
//!   全局 id = (shells.xml 局部 id << 8) | 国家基数（nation_id×16+10，回放射击事件逆向分析 §5.1）。
//!   注意：shell_kind 存 tanks.pb 原始串（ap/ap_cr/heat/he/…含 premium 修饰）——
//!   前端 srShellBadge 自行做标签映射与金弹判定、并按原始串与槽位弹种比对，勿在此归一化。
//! - [`collect_player_loadouts`]：type=5 开局实体（eid/昵称/初始 HP）×
//!   battle_results（昵称→队伍/tank_id）× tanks.pb（基准 HP/弹药配置）
//!   → 每玩家开局配置；HP 基准 = 车体 health + 顶级炮塔 health（与 tank_resolver 同规），
//!   initial 高于基准即耐久加成装备（改进耐久 +12.5%）。

use std::collections::HashMap;

use serde::Serialize;
use wotbreplay_parser::models::battle_results::BattleResults;

use super::combat::ShotReplayData;
use crate::wargaming::blitzkit::load_tanks;

/// BigWorld 国家序（scripts/common/items 收录顺序）：基数 = 序号×16+10
/// （usa=0x2a、uk=0x5a、japan=0x6a 回放实测定标）
const NATION_ORDER: [&str; 7] = ["ussr", "germany", "usa", "china", "france", "uk", "japan"];

fn nation_base(nation: &str) -> Option<u32> {
    NATION_ORDER.iter()
        .position(|n| n.eq_ignore_ascii_case(nation))
        .map(|i| (i * 16 + 10) as u32)
}

/// 全局弹种 id → shell_type 原始串映射表（tanks.pb 全量坦克×炮塔×主炮展开；
/// 同一全局 id 多车重复时取首个非空值——同国同局部 id 即同弹种）。
pub struct ShellKindTable {
    by_global: HashMap<u32, String>,
}

impl ShellKindTable {
    pub fn from_tanks_pb() -> Self {
        let mut by_global: HashMap<u32, String> = HashMap::new();
        for tank in load_tanks().values() {
            let Some(base) = nation_base(&tank.nation) else { continue };
            for turret in &tank.turrets {
                for gun in &turret.guns {
                    for s in &gun.shells {
                        if s.id == 0 || s.shell_type.is_empty() { continue; }
                        let gid = ((s.id as u32) << 8) | base;
                        by_global.entry(gid).or_insert_with(|| s.shell_type.clone());
                    }
                }
            }
        }
        Self { by_global }
    }

    /// 回填每发射击的 shell_kind；已有非空值或 shell_id=0（未知弹种）保持原样。
    pub fn annotate(&self, shots: &mut [ShotReplayData]) {
        for s in shots.iter_mut() {
            if !s.shell_kind.is_empty() || s.shell_id == 0 { continue; }
            if let Some(t) = self.by_global.get(&s.shell_id) {
                s.shell_kind = t.clone();
            }
        }
    }

    pub fn kind_of(&self, global_id: u32) -> Option<&str> {
        self.by_global.get(&global_id).map(String::as_str)
    }
}

/// tanks.pb 局部弹种 id → 全局 id：`(局部 id << 8) | 国家基数`（nation_id×16+10）。
/// viewer 端弹种表/槽位反查与回放 shell_id 同域对齐的统一入口。
pub fn blitzkit_shell_global_id(nation: &str, local_id: u64) -> Option<u32> {
    nation_base(nation).map(|base| ((local_id as u32) << 8) | base)
}

/// 单个弹种的开局配置条目（顶级主炮全部弹种）。
#[derive(Debug, Clone, Serialize)]
pub struct ShellEntry {
    pub global_id: u32,
    pub kind: String,
    pub damage: u32,
    pub penetration: u32,
}

/// 一名玩家的开局配置（type=5 开局实体 × battle_results 花名册 × tanks.pb 基准数据）。
#[derive(Debug, Clone, Serialize)]
pub struct PlayerLoadout {
    pub team: i32,
    pub nickname: String,
    pub tank_name: String,
    pub entity_id: u32,
    /// 基准血量（车体 + 顶级炮塔 health）
    pub hp_base: u32,
    /// 开局实际血量（type=5 偏移 51 的 u16 满血锚点）
    pub hp_initial: u32,
    /// (hp_initial/hp_base − 1)×100；无基准或无加成为 None
    pub hp_bonus_pct: Option<f64>,
    /// 耐久加成装备标签（+12.5% = 改进耐久；无加成为空串）
    pub durability_equipment: String,
    pub shells: Vec<ShellEntry>,
}

/// 提取全场玩家开局配置：
/// type=5 数据包（eid=[0..4]、HP u16=[51..53]、昵称长度前缀串@57）
/// 联表 battle_results（昵称→队伍/tank_id）与 tanks.pb（基准 HP/弹种表）。
/// 未联上花名册的实体（观察者等）跳过；花名册玩家缺 type=5 时按缺数据输出。
pub fn collect_player_loadouts(packets: &[(u32, f32, &[u8])], br: &BattleResults) -> Vec<PlayerLoadout> {
    // type=5 开局实体：eid → (昵称, 初始 HP)
    let mut entities: HashMap<u32, (String, u32)> = HashMap::new();
    for (pkt_type, _, p) in packets {
        if *pkt_type != 5 || p.len() < 60 { continue; }
        let eid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let hp = u16::from_le_bytes([p[51], p[52]]) as u32;
        let entry = entities.entry(eid).or_insert((String::new(), hp));
        if entry.1 == 0 { entry.1 = hp; }
        if entry.0.is_empty() {
            let str_len = p[57] as usize;
            if (3..=30).contains(&str_len) && 58 + str_len <= p.len() {
                if let Ok(s) = std::str::from_utf8(&p[58..58 + str_len]) {
                    if s.chars().all(|c| c.is_ascii_graphic()) {
                        entry.0 = s.to_string();
                    }
                }
            }
        }
    }

    // 花名册：昵称 → (队伍, account_id)；account_id → tank_id
    let nick_team: HashMap<&str, (i32, u32)> = br.players.iter()
        .map(|p| (p.info.nickname.as_str(), (p.info.team, p.account_id)))
        .collect();
    let tank_of: HashMap<u32, u32> = br.player_results.iter()
        .map(|pr| (pr.info.account_id, pr.info.tank_id))
        .collect();

    let tanks = load_tanks();
    let mut out: Vec<PlayerLoadout> = Vec::new();
    for (eid, (nickname, hp_initial)) in entities {
        let Some(&(team, account_id)) = nick_team.get(nickname.as_str()) else { continue };
        let Some(tank_id) = tank_of.get(&account_id).copied() else { continue };
        let tank = tanks.get(&tank_id);
        let hp_base = tank.map(|t| t.hp + t.turrets.last().map(|tr| tr.health).unwrap_or(0)).unwrap_or(0);
        let hp_bonus_pct = if hp_base > 0 && hp_initial > hp_base {
            Some((hp_initial as f64 / hp_base as f64 - 1.0) * 100.0)
        } else { None };
        let durability_equipment = match hp_bonus_pct {
            Some(pct) if (pct - 12.5).abs() < 0.5 => "改进耐久".to_string(),
            Some(pct) if pct >= 1.0 => format!("耐久加成 +{pct:.1}%"),
            _ => String::new(),
        };
        let shells = tank.map(|t| {
            let base = nation_base(&t.nation);
            let mut v: Vec<ShellEntry> = Vec::new();
            if let Some(top) = t.turrets.last() {
                if let Some(gun) = top.guns.last() {
                    for s in &gun.shells {
                        if s.id == 0 { continue; }
                        v.push(ShellEntry {
                            global_id: ((s.id as u32) << 8) | base.unwrap_or(0),
                            kind: s.shell_type.clone(),
                            damage: s.damage as u32,
                            penetration: s.penetration as u32,
                        });
                    }
                }
            }
            v
        }).unwrap_or_default();
        out.push(PlayerLoadout {
            team,
            nickname,
            tank_name: tank.map(|t| if t.name.is_empty() { t.dev_name.clone() } else { t.name.clone() })
                .unwrap_or_else(|| format!("tank_{}", tank_id)),
            entity_id: eid,
            hp_base,
            hp_initial,
            hp_bonus_pct,
            durability_equipment,
            shells,
        });
    }
    out.sort_by(|a, b| a.team.cmp(&b.team).then(a.nickname.cmp(&b.nickname)));
    out
}
