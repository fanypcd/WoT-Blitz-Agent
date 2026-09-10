use anyhow::{Context, Result};
use std::path::Path;

// =====================================================================
//  BlitzKit tanks.pb 解析器（自实现 Protobuf）
//  逐字段解析 BlitzKit 的二进制坦克数据库，提取每辆坦克的
//  tier/类型/国家/名称/血量等元数据，并支持批量下载坦克封面图。
// =====================================================================

/// BlitzKit `tanks.pb` 数据文件的下载地址。
const PB_URL: &str = "https://api.blitzkit.app/definitions/tanks.pb";
/// BlitzKit `models.pb` 模型定义文件的下载地址（含炮塔/主炮→模型节点编号的权威映射）。
const MODELS_URL: &str = "https://api.blitzkit.app/definitions/models.pb";

/// 一个极简的 protobuf 读取器（只读，遍历字段）。
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// 读一个 varint（protobuf 的变长整数编码）。
    fn varint(&mut self) -> Result<u64> {
        let mut result: u64 = 0;
        let mut shift = 0u32;
        loop {
            let b = *self.buf.get(self.pos).context("pb truncated (varint)")?;
            self.pos += 1;
            result |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift > 63 {
                anyhow::bail!("varint too long");
            }
        }
    }

    /// 读 `len` 字节的裸数据。
    fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.pos + len;
        if end > self.buf.len() {
            anyhow::bail!("pb truncated (len)");
        }
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    /// 读下一个 tag；返回 `(字段号, wire_type)`，到达缓冲末尾返回 `None`。
    fn tag(&mut self) -> Option<Result<(u32, u8)>> {
        if self.pos >= self.buf.len() {
            return None;
        }
        Some((|| {
            let t = self.varint()? as u32;
            Ok((t >> 3, (t & 7) as u8))
        })())
    }

    /// 跳过某个 wire 类型字段。
    fn skip_field(&mut self, wire: u8) -> Result<()> {
        match wire {
            0 => { self.varint()?; }
            1 => { self.bytes(8)?; }
            2 => { let l = self.varint()? as usize; self.bytes(l)?; }
            5 => { self.bytes(4)?; }
            w => anyhow::bail!("unsupported wire type {}", w),
        }
        Ok(())
    }
}

/// 下载 BlitzKit `tanks.pb` + `models.pb` 到 data/ 数据目录（`fetch-blitzkit` 命令）。
///
/// tanks.pb 是项目的唯一坦克数据源：运行时直接解析它获取元数据/武器/装填；
/// models.pb 提供炮塔/主炮→模型节点编号的权威映射。均无需再拆分成多个中间 JSON。
pub async fn fetch_and_save(output: &Path) -> Result<usize> {
    // 保存 tanks.pb 并校验可解析
    eprintln!("Downloading {} ...", PB_URL);
    let resp = reqwest::get(PB_URL).await?.error_for_status()?;
    let data = resp.bytes().await?;
    std::fs::write(output, &data)?;
    let tanks = parse_tanks_pb(&data)?;
    eprintln!("Saved tanks.pb ({} bytes, {} tanks) to {}", data.len(), tanks.len(), output.display());

    // 保存 models.pb（炮塔/主炮→模型节点映射）到同目录
    let models_path = output.parent().map(|p| p.join("models.pb")).unwrap_or_else(|| Path::new("models.pb").to_path_buf());
    eprintln!("Downloading {} ...", MODELS_URL);
    let resp = reqwest::get(MODELS_URL).await?.error_for_status()?;
    let mdata = resp.bytes().await?;
    std::fs::write(&models_path, &mdata)?;
    let models = parse_models_pb(&mdata)?;
    eprintln!("Saved models.pb ({} bytes, {} tanks) to {}", mdata.len(), models.len(), models_path.display());

    Ok(tanks.len())
}

/// 一门炮的装填信息。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GunReload {
    /// 单发/弹夹/弹鼓的装填时间（秒）：单发=单发装填；弹夹=整夹装填；
    /// 弹鼓=各发装填的最大值。
    pub reload: f64,
    /// 是否弹夹/弹鼓炮。
    pub is_burst: bool,
    /// 是否弹鼓炮（每发独立装填）；false 表示弹夹（整夹装填）或单发。
    pub is_drum: bool,
    /// 弹夹容量（发）；非弹夹炮为 0。
    pub burst_size: f64,
    /// 弹夹/弹鼓内每发间隔（发射间隔/秒）。
    pub burst_interval: f64,
    /// 弹鼓各发装填时间列表（秒）；弹夹/单发为空。
    pub burst_reloads: Vec<f64>,
}

/// 一种弹药。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShellData {
    pub name: String,
    pub shell_type: String,
    pub damage: f64,
    pub penetration: f64,
    pub module_damage: f64,
    /// 弹速（m/s）。
    pub velocity: f64,
    /// 最大射程（m）。
    pub range: f64,
    /// 远距离穿深（穿深随距离线性衰减到该值）。
    pub penetration_far: f64,
    /// 弹径（mm）。
    pub caliber: f64,
    /// 转正角（度）。
    pub normalization: f64,
    /// 跳弹临界角（度）。
    pub ricochet: f64,
    /// HE 爆炸半径（m）；仅 HE 弹非零。
    pub explosion_radius: f64,
}

/// 一门主炮。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GunData {
    pub module_id: u32,
    pub name: String,
    /// 口径系数（如 26.25 对应 130mm；乘某因子得毫米）。
    pub caliber_factor: f64,
    pub shell_count: u32,
    /// 百米精度(散布)。
    pub dispersion: f64,
    /// 瞄准时间（秒）。
    pub aim_time: f64,
    pub shells: Vec<ShellData>,
    /// 装填信息（单发/弹夹/弹鼓）。
    pub reload: GunReload,
}

/// 一个引擎（来自坦克条目 field21）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EngineData {
    pub name: String,
    /// 引擎功率（马力）。
    pub power: f64,
    /// 起火率（0-1，如 0.082 = 8.2%）。
    pub fire_chance: f64,
}

/// 一个炮塔（含其主炮）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TurretData {
    pub module_id: u32,
    pub name: String,
    /// 炮塔血量（TurretDefinition.health，field2）——坦克总 HP = 车体 health + 炮塔 health
    pub health: u32,
    /// 炮塔重量（TurretDefinition.weight，field8）
    pub weight: f64,
    pub view_range: f64,
    pub traverse_speed: f64,
    pub guns: Vec<GunData>,
}

/// 一辆坦克的完整数据（元数据 + 炮塔/主炮/弹种 + 装填）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TankFullData {
    pub tank_id: u32,
    pub dev_name: String,
    pub name: String,
    pub nation: String,
    pub tier: u32,
    pub tank_type: String,
    pub hp: u32,
    pub is_premium: bool,
    pub speed_forward: f64,
    pub speed_reverse: f64,
    pub hull_traverse: f64,
    pub weight: f64,
    pub turrets: Vec<TurretData>,
    /// 可用引擎列表（通常 1 个，部分车多档）。
    pub engines: Vec<EngineData>,
}

/// 炮管俯仰极值（PitchLimitsExtrema，度）：朝向某方向的俯仰范围。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PitchExtremaInfo {
    pub min: f32,
    pub max: f32,
    pub range: f32,
}

/// 炮管俯仰限制（GunModelDefinition.pitch，度，对齐 BlitzKit applyPitchYawLimits）：
/// min = −仰角上限、max = 俯角上限（lower=−max、upper=−min）；front/back = 炮塔朝向
/// 车头/车尾方向的极值（range 为该朝向的覆盖角度），transition = 朝向过渡角度。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PitchLimitsInfo {
    pub min: f32,
    pub max: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front: Option<PitchExtremaInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<PitchExtremaInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<f32>,
}

/// 炮塔水平射界（TurretModelDefinition.yaw，度；使用时钳制到 [−max, −min]，对齐 BlitzKit）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct YawLimitsInfo {
    pub min: f32,
    pub max: f32,
}

/// 初始炮塔姿态（ModelDefinition.initial_turret_rotation，度）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InitialRotationInfo {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

/// 单个炮塔的模型节点信息（来自 models.pb）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TurretModelInfo {
    /// 炮塔模块 id（如 Tiger II 的 8209）。
    pub module_id: u32,
    /// 该炮塔的模型节点编号（`turret_0X` 的 X）。
    pub model_node: u32,
    /// 炮塔装甲板 spaced 列表（TurretModelDefinition.armor.spaced，BlitzKit 分类权威）。
    #[serde(default)]
    pub turret_spaced: Vec<u32>,
    /// 火炮原点（TurretModelDefinition.gun_origin，DAVA 坐标）——炮管装甲板的定位基准。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gun_origin: Option<[f32; 3]>,
    /// 炮塔水平射界（TurretModelDefinition.yaw，度）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yaw_limits: Option<YawLimitsInfo>,
    /// 该炮塔下各主炮的模型节点编号（`gun_0X` 的 X）。
    pub guns: Vec<GunModelInfo>,
}

/// 单个主炮的模型节点信息（来自 models.pb）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GunModelInfo {
    /// 主炮模块 id（如 Tiger II 的 2321）。
    pub gun_module_id: u32,
    /// 该主炮的模型节点编号（`gun_0X` 的 X）。
    pub model_node: u32,
    /// 炮管外部模块厚度（GunModelDefinition.thickness，mm）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<f32>,
    /// 炮根/炮盾掩体位置（GunModelDefinition.mask）。None → 外部模块只取精确 gun_0X 节点；
    /// Some(v) → 含 gun_0X* 全部网格并按掩体平面裁剪（对齐 BlitzKit）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<f32>,
    /// 炮管装甲板 spaced 列表（GunModelDefinition.armor.spaced）。
    /// BlitzKit 数据中炮管安装甲普遍为 spaced（间隙甲）：穿透它不算击穿坦克，
    /// 炮弹必须继续穿透后面的车体/炮塔主装甲（实测 T-34 [1,2,3]、E 100 [1,2,3,4,5] 等）。
    #[serde(default)]
    pub gun_spaced: Vec<u32>,
    /// 炮管俯仰限制（GunModelDefinition.pitch，含 front/back 极值，度）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_limits: Option<PitchLimitsInfo>,
}

/// 一辆坦克的模型节点信息（来自 models.pb）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TankModelInfo {
    pub tank_id: u32,
    /// 车体装甲板 spaced 列表（ModelDefinition.armor.spaced）。
    #[serde(default)]
    pub hull_spaced: Vec<u32>,
    /// 炮塔原点（ModelDefinition.turret_origin，DAVA 坐标）——炮塔装甲板的定位基准。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turret_origin: Option<[f32; 3]>,
    /// 履带原点（第一个 TrackModelDefinition.origin，DAVA 坐标）——车体装甲板的定位基准。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_origin: Option<[f32; 3]>,
    /// 初始炮塔姿态（ModelDefinition.initial_turret_rotation，度；部分车辆才有）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_turret_rotation: Option<InitialRotationInfo>,
    pub turrets: Vec<TurretModelInfo>,
}

/// 解码坦克类别码：1=mediumTank、2=heavyTank、3=AT-SPG、无则 lightTank。
fn decode_class_code(code: u32) -> String {
    match code {
        1 => "mediumTank".to_string(),
        2 => "heavyTank".to_string(),
        3 => "AT-SPG".to_string(),
        _ => "lightTank".to_string(),
    }
}

/// 解析 `tanks.pb` 的完整数据（元数据+炮塔/主炮/弹种+装填），供运行时直接使用。
///
/// 一次遍历即可得到每辆坦克的全部数据，无需拆成多个中间 JSON。
/// 字段语义（经 multi-tank 校验）：
///   坦克级：field1=id, field2=dev_name, field10=hp, field11=nation, field16=tier,
///           field17=类别码, field25=前速, field26=倒速, field27=车体旋(rad/s), field31=重量, field32=名称
///   炮塔级(field20)：field1=module, field2=重量, field3=视野, field4=转速, field8=? 主炮在 field9 内
///   主炮级：field4=module, field5=口径系数, field9=弹数, field10=弹种, field12=瞄准, field13=百米精度；
///           field1(内嵌)=单发装填, field2(内嵌)=弹夹[装填,间隔,容量], field3(内嵌)=弹鼓[各发...,间隔,容量]
///   弹种级(field10)：field2=名称, field4=伤害, field5=模块伤害, field6=口径, field7=类型, field8.内嵌1=穿透
pub fn parse_tanks_pb(buf: &[u8]) -> Result<Vec<TankFullData>> {
    let mut r = Reader { buf, pos: 0 };
    let mut out = Vec::new();

    while let Some(res) = r.tag() {
        let (f, w) = res?;
        if f != 1 || w != 2 {
            anyhow::bail!("unexpected top-level field {} wire {}", f, w);
        }
        let len = r.varint()? as usize;
        let entry = r.bytes(len)?;
        let mut er = Reader { buf: entry, pos: 0 };

        let mut tank_id: u32 = 0;
        while let Some(res2) = er.tag() {
            let (f2, w2) = res2?;
            match (f2, w2) {
                (1, 0) => tank_id = er.varint()? as u32,
                (2, 2) => {
                    let slen = er.varint()? as usize;
                    let sub = er.bytes(slen)?;
                    if let Some(tank) = parse_tank_main(sub, tank_id)? {
                        out.push(tank);
                    }
                }
                _ => er.skip_field(w2)?,
            }
        }
    }
    Ok(out)
}

// 解析主体消息（含全部元数据 + 炮塔/主炮）
fn parse_tank_main(sub: &[u8], tank_id: u32) -> Result<Option<TankFullData>> {
    let mut sr = Reader { buf: sub, pos: 0 };
    let mut tank = TankFullData {
        tank_id, dev_name: String::new(), name: String::new(),
        nation: String::new(), tier: 0, tank_type: String::new(),
        hp: 0, is_premium: false,
        speed_forward: 0.0, speed_reverse: 0.0, hull_traverse: 0.0,
        weight: 0.0, turrets: Vec::new(), engines: Vec::new(),
    };
    let mut has_name = false;
    let mut localized_name = String::new();
    while let Some(res) = sr.tag() {
        let (f, w) = res?;
        match (f, w) {
            (1, 0) => { let _ = sr.varint()?; },   // tank_id 重复
            (2, 2) => { let l = sr.varint()? as usize; tank.dev_name = String::from_utf8_lossy(sr.bytes(l)?).into_owned(); },
            (10, 0) => tank.hp = sr.varint()? as u32,
            (11, 2) => { let l = sr.varint()? as usize; tank.nation = String::from_utf8_lossy(sr.bytes(l)?).into_owned(); },
            (12, 2) => { let l = sr.varint()? as usize; localized_name = extract_name(sr.bytes(l)?); },
            (13, 0) => tank.is_premium = sr.varint()? == 1,
            (16, 0) => tank.tier = sr.varint()? as u32,
            (17, 0) => tank.tank_type = decode_class_code(sr.varint()? as u32),
            (21, 2) => {
                // 引擎: field2=名字, field6=功率(马力), field7=起火率(万分比)
                let l = sr.varint()? as usize;
                let eb = sr.bytes(l)?;
                let mut name = String::new();
                let mut power = 0.0f64;
                let mut fire = 0.0f64;
                let mut er = Reader { buf: eb, pos: 0 };
                while let Some(res) = er.tag() {
                    let (f, w) = res?;
                    match (f, w) {
                        (2, 2) => {
                            let nl = er.varint()? as usize;
                            let nb = er.bytes(nl)?;
                            name = extract_name(nb);
                        },
                        (6, 0) => power = er.varint()? as f64,
                        (7, 0) => fire = er.varint()? as f64 / 10000.0,
                        _ => er.skip_field(w)?,
                    }
                }
                if !name.is_empty() || power > 0.0 {
                    tank.engines.push(EngineData { name, power, fire_chance: fire });
                }
            },
            (25, 5) => { tank.speed_forward = f32::from_le_bytes(sr.bytes(4)?.try_into().unwrap()) as f64; },
            (26, 5) => { tank.speed_reverse = f32::from_le_bytes(sr.bytes(4)?.try_into().unwrap()) as f64; },
            (27, 5) => { tank.hull_traverse = f32::from_le_bytes(sr.bytes(4)?.try_into().unwrap()) as f64; },
            (31, 0) => tank.weight = sr.varint()? as f64,
            (32, 2) => { let l = sr.varint()? as usize; tank.name = String::from_utf8_lossy(sr.bytes(l)?).into_owned(); has_name = true; },
            (20, 2) => {
                let tlen = sr.varint()? as usize;
                let turret_bytes = sr.bytes(tlen)?;
                if let Some(tur) = parse_turret(&turret_bytes)? {
                    tank.turrets.push(tur);
                }
            }
            _ => sr.skip_field(w)?,
        }
    }
    if !has_name { return Ok(None); }
    // 优先本地化 display 名（en）；若为空则用 field32 短名。
    if !localized_name.is_empty() { tank.name = localized_name; }
    // 类别码 field17 缺失 → 轻坦（Blitz 约定：lightTank 为枚举默认值，
    // 仅 medium/heavy/AT-SPG 显式编码；实测 119 辆车无此字段，均为轻坦）。
    if tank.tank_type.is_empty() { tank.tank_type = "lightTank".to_string(); }
    Ok(Some(tank))
}

// 解析炮塔消息
fn parse_turret(tb: &[u8]) -> Result<Option<TurretData>> {
    let mut tr = Reader { buf: tb, pos: 0 };
    let mut turret = TurretData {
        module_id: 0, name: String::new(),
        health: 0, weight: 0.0, view_range: 0.0, traverse_speed: 0.0, guns: Vec::new(),
    };
    let mut has_gun = false;
    while let Some(res) = tr.tag() {
        let (f, w) = res?;
        match (f, w) {
            (1, 0) => turret.module_id = tr.varint()? as u32,
            // TurretDefinition.health（field2）——炮塔血量；旧实现误当 weight
            (2, 0) => turret.health = tr.varint()? as u32,
            (3, 0) => turret.view_range = tr.varint()? as f64,
            (4, 5) => turret.traverse_speed = f32::from_le_bytes(tr.bytes(4)?.try_into().unwrap()) as f64,
            // TurretDefinition.weight（field8）——真正的炮塔重量
            (8, 0) => turret.weight = tr.varint()? as f64,
            (6, 2) => { let l = tr.varint()? as usize; let _ = tr.bytes(l)?; },
            (9, 2) => {
                let glen = tr.varint()? as usize;
                let gb = tr.bytes(glen)?;
                if let Some(g) = parse_gun(&gb)? {
                    turret.guns.push(g); has_gun = true;
                }
            }
            _ => tr.skip_field(w)?,
        }
    }
    if has_gun { Ok(Some(turret)) } else { Ok(None) }
}
/// 解析一个主炮子块。
fn parse_gun(gb: &[u8]) -> Result<Option<GunData>> {
    let mut gr = Reader { buf: gb, pos: 0 };
    let mut gun = GunData {
        module_id: 0, name: String::new(), caliber_factor: 0.0,
        shell_count: 0, dispersion: 0.0, aim_time: 0.0,
        shells: Vec::new(), reload: GunReload { reload: 0.0, is_burst: false, is_drum: false, burst_size: 0.0, burst_interval: 0.0, burst_reloads: Vec::new() },
    };
    let mut saw_caliber = false;
    let mut name_bytes: Vec<u8> = Vec::new();
    while let Some(res5) = gr.tag() {
        let (f5, w5) = res5?;
        match (f5, w5) {
            (1, 2) => { let ilen = gr.varint()? as usize; let inner = gr.bytes(ilen)?;
                // 单发装填
                let mut ir = Reader { buf: inner, pos: 0 };
                while let Some(res6) = ir.tag() {
                    let (f6, w6) = res6?;
                    if f6 == 1 && w6 == 5 {
                        gun.reload.reload = f32::from_le_bytes(ir.bytes(4)?.try_into().unwrap()) as f64;
                    } else { ir.skip_field(w6)?; }
                }
                // 单发：is_burst 保持 false（除非后续 field2/field3 覆盖为弹夹/弹鼓）
            },
            (2, 2) => { let ilen = gr.varint()? as usize; let inner = gr.bytes(ilen)?;
                // 弹夹(magazine)：field1=整夹装填, field2=间隔, field3=容量
                let mut ir = Reader { buf: inner, pos: 0 };
                let mut mag_reload = None; let mut interval = 0.0; let mut size = 0.0;
                while let Some(res6) = ir.tag() {
                    let (f6, w6) = res6?;
                    if w6 == 5 {
                        let v = f32::from_le_bytes(ir.bytes(4)?.try_into().unwrap()) as f64;
                        if f6 == 1 { mag_reload = Some(v); } else if f6 == 2 { interval = v; } else if f6 == 3 { size = v; }
                    } else { ir.skip_field(w6)?; }
                }
                gun.reload.is_burst = true; gun.reload.is_drum = false;
                gun.reload.reload = mag_reload.unwrap_or(0.0);
                gun.reload.burst_interval = interval;
                gun.reload.burst_size = size;
                gun.reload.burst_reloads = Vec::new();
            },
            (3, 2) => { let ilen = gr.varint()? as usize; let inner = gr.bytes(ilen)?;
                // 弹鼓(drum)：field1(repeated)=各发装填, field2=间隔, field3=容量
                let mut ir = Reader { buf: inner, pos: 0 };
                let mut shots = Vec::new(); let mut interval = 0.0; let mut size = 0.0;
                while let Some(res6) = ir.tag() {
                    let (f6, w6) = res6?;
                    if w6 == 5 {
                        let v = f32::from_le_bytes(ir.bytes(4)?.try_into().unwrap()) as f64;
                        if f6 == 1 { shots.push(v); } else if f6 == 2 { interval = v; } else if f6 == 3 { size = v; }
                    } else { ir.skip_field(w6)?; }
                }
                gun.reload.is_burst = true; gun.reload.is_drum = true;
                gun.reload.burst_interval = interval;
                gun.reload.burst_size = size;
                gun.reload.burst_reloads = shots.clone();
                gun.reload.reload = shots.iter().cloned().fold(0.0f64, f64::max);
            },
            (4, 0) => gun.module_id = gr.varint()? as u32,
            (5, 5) => { gun.caliber_factor = f32::from_le_bytes(gr.bytes(4)?.try_into().unwrap()) as f64; if gun.caliber_factor > 1.0 { saw_caliber = true; } },
            (8, 2) => { let l = gr.varint()? as usize; name_bytes = gr.bytes(l)?.to_vec(); },
            (9, 0) => gun.shell_count = gr.varint()? as u32,
            (10, 2) => { let l = gr.varint()? as usize; let sb = gr.bytes(l)?; if let Some(s) = parse_shell(&sb)? { gun.shells.push(s); } },
            (12, 5) => { gun.aim_time = f32::from_le_bytes(gr.bytes(4)?.try_into().unwrap()) as f64; },
            (13, 5) => { gun.dispersion = f32::from_le_bytes(gr.bytes(4)?.try_into().unwrap()) as f64; },
            _ => gr.skip_field(w5)?,
        }
    }
    if !saw_caliber { return Ok(None); }
    // 从 name_bytes 提取主炮名（第一个 en 条目）
    gun.name = extract_name(&name_bytes);
    Ok(Some(gun))
}

/// 解析一个弹种子块。
fn parse_shell(sb: &[u8]) -> Result<Option<ShellData>> {
    let mut sr = Reader { buf: sb, pos: 0 };
    let mut shell = ShellData { name: String::new(), shell_type: String::new(), damage: 0.0, penetration: 0.0, module_damage: 0.0, velocity: 0.0, range: 0.0, penetration_far: 0.0, caliber: 0.0, normalization: 0.0, ricochet: 0.0, explosion_radius: 0.0 };
    let mut name_bytes: Vec<u8> = Vec::new();
    let mut type_bytes: Vec<u8> = Vec::new();
    while let Some(res) = sr.tag() {
        let (f, w) = res?;
        match (f, w) {
            (1, 0) => { let _ = sr.varint()?; },
            (2, 2) => { let l = sr.varint()? as usize; name_bytes = sr.bytes(l)?.to_vec(); },
            (3, 0) => shell.velocity = sr.varint()? as f64,          // 弹速 m/s
            (4, 0) => shell.damage = sr.varint()? as f64,
            (5, 0) => shell.module_damage = sr.varint()? as f64,
            (7, 2) => { let l = sr.varint()? as usize; type_bytes = sr.bytes(l)?.to_vec(); },
            (8, 2) => { let l = sr.varint()? as usize; let pb = sr.bytes(l)?;
                // 穿透在 field8 内嵌：field1(float)=近距穿深、field2(float)=远距穿深
                let mut pr = Reader { buf: pb, pos: 0 };
                while let Some(res2) = pr.tag() {
                    let (f2, w2) = res2?;
                    if f2 == 1 && w2 == 5 { shell.penetration = f32::from_le_bytes(pr.bytes(4)?.try_into().unwrap()) as f64; }
                    else if f2 == 2 && w2 == 5 { shell.penetration_far = f32::from_le_bytes(pr.bytes(4)?.try_into().unwrap()) as f64; }
                    else { pr.skip_field(w2)?; }
                }
            },
            (13, 0) => shell.range = sr.varint()? as f64,            // 射程 m
            (6, 5) => shell.caliber = f32::from_le_bytes(sr.bytes(4)?.try_into().unwrap()) as f64,      // 弹径 mm
            (10, 5) => shell.normalization = f32::from_le_bytes(sr.bytes(4)?.try_into().unwrap()) as f64, // 转正角度
            (11, 5) => shell.ricochet = f32::from_le_bytes(sr.bytes(4)?.try_into().unwrap()) as f64,   // 跳弹临界角
            (12, 5) => shell.explosion_radius = f32::from_le_bytes(sr.bytes(4)?.try_into().unwrap()) as f64, // HE 爆炸半径 m
            _ => sr.skip_field(w)?,
        }
    }
    shell.shell_type = String::from_utf8_lossy(&type_bytes).into_owned();
    shell.name = extract_name(&name_bytes);
    Ok(Some(shell))
}

/// 从本地化 name 块提取英文名（在 `en` 本地化子消息中，格式 `\x02en\x12<len><name>`）。
fn extract_name(name_bytes: &[u8]) -> String {
    let marker: [u8; 4] = [0x02, b'e', b'n', 0x12];
    if let Some(pos) = name_bytes.windows(4).position(|w| w == &marker) {
        let mut j = pos + 4;
        if let Some(nlen) = read_varint(name_bytes, &mut j) {
            if j + nlen as usize <= name_bytes.len() {
                return String::from_utf8_lossy(&name_bytes[j..j + nlen as usize]).into_owned();
            }
        }
    }
    String::new()
}

/// 简单 varint 读取（返回 value 并推进 i）。
fn read_varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut r = 0u64; let mut s = 0u32;
    while *i < b.len() {
        let x = b[*i]; *i += 1;
        r |= ((x & 0x7f) as u64) << s;
        if x & 0x80 == 0 { return Some(r); }
        s += 7;
        if s > 63 { return None; }
    }
    None
}

/// 坦克封面图 URL 模板（big.webp）。
pub const ICON_URL: &str = "https://api.blitzkit.app/tanks/{}/icons/big.webp";

/// 批量下载全部坦克封面图到 `tank_images/{id}.webp`（阻塞）。
///
/// 已存在的文件跳过（除非 `force`），返回 `(下载数, 缓存数, 失败数)`。
pub fn download_all_icons(dir: &Path, force: bool) -> Result<(usize, usize, usize)> {
    std::fs::create_dir_all(dir)?;
    // 用本地 tanks.pb（唯一数据源）枚举 tank_id，无需重复下载。
    let ids: Vec<u32> = load_tanks().keys().copied().collect();
    let total = ids.len();
    let mut downloaded = 0usize;
    let mut cached = 0usize;
    let mut failed = 0usize;

    for (i, tid) in ids.iter().enumerate() {
        let path = dir.join(format!("{}.webp", tid));
        if path.exists() && !force {
            cached += 1;
        } else {
            let url = ICON_URL.replace("{}", &tid.to_string());
            match reqwest::blocking::get(&url) {
                Ok(resp) if resp.status().is_success() => {
                    match resp.bytes() {
                        Ok(bytes) if !bytes.is_empty() => {
                            if std::fs::write(&path, &bytes).is_ok() {
                                downloaded += 1;
                            } else {
                                failed += 1;
                            }
                        }
                        _ => failed += 1,
                    }
                }
                _ => failed += 1,
            }
        }
        if (i + 1) % 50 == 0 {
            eprintln!("  [{}/{}] downloaded={} cached={} failed={}", i + 1, total, downloaded, cached, failed);
        }
    }
    eprintln!("Icons done: downloaded={} cached={} failed={}", downloaded, cached, failed);
    Ok((downloaded, cached, failed))
}

/// 读取并解析 tanks.pb，返回 tank_id → TankFullData 映射。
/// 结果用 OnceLock 缓存，进程内只解析一次（解析成本较高）。
/// 读取并解析 tanks.pb，返回 tank_id → TankFullData 映射。
/// 结果用 OnceLock 缓存，进程内只解析一次（解析成本较高）。
pub fn load_tanks() -> std::collections::HashMap<u32, TankFullData> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<Vec<TankFullData>>> = OnceLock::new();
    let vec = CACHE.get_or_init(|| {
        let bytes = std::fs::read(crate::data::data_path("tanks.pb")).ok()?;
        parse_tanks_pb(&bytes).ok().map(|v| v.into_iter().filter(|t| !t.name.is_empty()).collect())
    });
    let mut out = std::collections::HashMap::new();
    if let Some(v) = vec { for t in v { out.insert(t.tank_id, t.clone()); } }
    out
}

/// 读取单辆坦克的完整数据。
pub fn tank_full(tank_id: u32) -> Option<TankFullData> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<Vec<TankFullData>>> = OnceLock::new();
    let vec = CACHE.get_or_init(|| {
        let bytes = std::fs::read(crate::data::data_path("tanks.pb")).ok()?;
        parse_tanks_pb(&bytes).ok().map(|v| v.into_iter().filter(|t| !t.name.is_empty()).collect())
    });
    vec.as_ref().and_then(|v| v.iter().find(|t| t.tank_id == tank_id)).cloned()
}

/// 解析 BlitzKit `models.pb`，返回 tank_id → 模型节点信息（炮塔/主炮→gun/turret_0X 编号）。
///
/// 该文件给出每辆坦克每套炮塔+主炮绑定到模型节点的权威映射，用于把配置正确切换到
/// 对应的 gun_0X / turret_0X 节点（多炮塔共享炮、多配置共享节点等情况均能正确处理）。
///
/// 结构（经实测解码）：
///   顶层 field1(repeated) = 每个坦克条目，其 field1=tank_id, field2=模型内容
///   模型内容 field4(repeated) = 每个炮塔：
///     field1 = 炮塔模块 id
///     field2 = 炮塔内容：field3=炮塔模型节点号, field5(repeated)=每个主炮：
///       主炮 field1 = 主炮模块 id
///       主炮 field2 = field3 = 主炮模型节点号
pub fn parse_models_pb(buf: &[u8]) -> Result<Vec<TankModelInfo>> {
    let mut result = Vec::new();
    let mut top = Reader { buf, pos: 0 };
    while let Some(res) = top.tag() {
        let (f, w) = res?;
        match (f, w) {
            (1, 2) => {
                let tlen = top.varint()? as usize;
                let tb = top.bytes(tlen)?;
                if let Some(info) = parse_model_tank_entry(tb)? {
                    result.push(info);
                }
            }
            _ => top.skip_field(w)?,
        }
    }
    Ok(result)
}

/// 解析 Armor 消息中的 spaced 列表（field2：packed 或逐项 varint repeated uint32）。
/// Armor = { map<uint32,float> thickness=1; repeated uint32 spaced=2; }。
fn parse_armor_spaced(ab: &[u8]) -> Result<Vec<u32>> {
    let mut ar = Reader { buf: ab, pos: 0 };
    let mut spaced = Vec::new();
    while let Some(res) = ar.tag() {
        let (f, w) = res?;
        match (f, w) {
            (2, 2) => {
                let l = ar.varint()? as usize;
                let pb = ar.bytes(l)?;
                let mut pr = Reader { buf: pb, pos: 0 };
                while pr.pos < pr.buf.len() {
                    spaced.push(pr.varint()? as u32);
                }
            }
            (2, 0) => spaced.push(ar.varint()? as u32),
            _ => ar.skip_field(w)?,
        }
    }
    Ok(spaced)
}

/// 解析 Vector3 消息（field1/2/3 均为 fixed32 float；缺失分量记 0）。
fn parse_vec3(vb: &[u8]) -> Result<Option<[f32; 3]>> {
    let mut vr = Reader { buf: vb, pos: 0 };
    let mut v = [0.0f32; 3];
    let mut any = false;
    while let Some(res) = vr.tag() {
        let (f, w) = res?;
        match (f, w) {
            (1, 5) => { let b = vr.bytes(4)?; v[0] = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); any = true; }
            (2, 5) => { let b = vr.bytes(4)?; v[1] = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); any = true; }
            (3, 5) => { let b = vr.bytes(4)?; v[2] = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); any = true; }
            _ => vr.skip_field(w)?,
        }
    }
    Ok(if any { Some(v) } else { None })
}

/// 解析 models.pb 中单个坦克条目。
fn parse_model_tank_entry(tb: &[u8]) -> Result<Option<TankModelInfo>> {
    let mut tr = Reader { buf: tb, pos: 0 };
    let mut tank_id = 0u32;
    let mut content: Option<&[u8]> = None;
    while let Some(res) = tr.tag() {
        let (f, w) = res?;
        match (f, w) {
            (1, 0) => tank_id = tr.varint()? as u32,
            (2, 2) => { let l = tr.varint()? as usize; content = Some(tr.bytes(l)?); }
            _ => tr.skip_field(w)?,
        }
    }
    let Some(content) = content else { return Ok(None) };

    let mut hull_spaced = Vec::new();
    let mut turret_origin: Option<[f32; 3]> = None;
    let mut track_origin: Option<[f32; 3]> = None;
    let mut initial_turret_rotation: Option<InitialRotationInfo> = None;
    let mut turrets = Vec::new();
    let mut cr = Reader { buf: content, pos: 0 };
    while let Some(res) = cr.tag() {
        let (f, w) = res?;
        match (f, w) {
            // ModelDefinition.armor（field1）= 车体 Armor（thickness map + spaced）
            (1, 2) => {
                let l = cr.varint()? as usize;
                hull_spaced = parse_armor_spaced(cr.bytes(l)?)?;
            }
            // ModelDefinition.turret_origin（field2）——注意：全 0 的 origin 可能被序列化器
            // 省略为空消息，此时按 (0,0,0) 处理（否则 model_origins 整体失效、定位回退旧方案）
            (2, 2) => {
                let l = cr.varint()? as usize;
                turret_origin = Some(parse_vec3(cr.bytes(l)?)?.unwrap_or([0.0, 0.0, 0.0]));
            }
            // ModelDefinition.initial_turret_rotation（field3）= {yaw=1, pitch=2, roll=3}（度）
            (3, 2) => {
                let l = cr.varint()? as usize;
                let ib = cr.bytes(l)?;
                let mut ir = Reader { buf: ib, pos: 0 };
                let mut rot = InitialRotationInfo { yaw: 0.0, pitch: 0.0, roll: 0.0 };
                while let Some(res2) = ir.tag() {
                    let (f2, w2) = res2?;
                    match (f2, w2) {
                        (1, 5) => { let b = ir.bytes(4)?; rot.yaw = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                        (2, 5) => { let b = ir.bytes(4)?; rot.pitch = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                        (3, 5) => { let b = ir.bytes(4)?; rot.roll = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                        _ => ir.skip_field(w2)?,
                    }
                }
                initial_turret_rotation = Some(rot);
            }
            (4, 2) => {
                let tl = cr.varint()? as usize;
                let tb = cr.bytes(tl)?;
                if let Some(tur) = parse_model_turret(tb)? {
                    turrets.push(tur);
                }
            }
            // ModelDefinition.tracks（field5）= map<uint32, TrackModelDefinition{thickness=1, origin=2}>
            (5, 2) => {
                let l = cr.varint()? as usize;
                let kb = cr.bytes(l)?;
                let mut k = None;
                let mut origin: Option<[f32; 3]> = None;
                let mut kr = Reader { buf: kb, pos: 0 };
                while let Some(res2) = kr.tag() {
                    let (f2, w2) = res2?;
                    match (f2, w2) {
                        (1, 0) => { k = Some(kr.varint()? as u32); }
                        (2, 2) => {
                            let l2 = kr.varint()? as usize;
                            let trb = kr.bytes(l2)?;
                            let mut trr = Reader { buf: trb, pos: 0 };
                            while let Some(res3) = trr.tag() {
                                let (f3, w3) = res3?;
                                match (f3, w3) {
                                    (2, 2) => { let l3 = trr.varint()? as usize; origin = parse_vec3(trr.bytes(l3)?)?; }
                                    _ => trr.skip_field(w3)?,
                                }
                            }
                        }
                        _ => kr.skip_field(w2)?,
                    }
                }
                if k.is_some() && track_origin.is_none() {
                    // 空的 origin 消息（全 0 省略）按 (0,0,0) 处理——如 T28 Defender
                    track_origin = Some(origin.unwrap_or([0.0, 0.0, 0.0]));
                }
            }
            _ => cr.skip_field(w)?,
        }
    }
    Ok(Some(TankModelInfo { tank_id, hull_spaced, turret_origin, track_origin, initial_turret_rotation, turrets }))
}

/// 解析 models.pb 中单个炮塔条目。
fn parse_model_turret(tb: &[u8]) -> Result<Option<TurretModelInfo>> {
    let mut tr = Reader { buf: tb, pos: 0 };
    let mut tmod = 0u32;
    let mut content: Option<&[u8]> = None;
    while let Some(res) = tr.tag() {
        let (f, w) = res?;
        match (f, w) {
            (1, 0) => tmod = tr.varint()? as u32,
            (2, 2) => { let l = tr.varint()? as usize; content = Some(tr.bytes(l)?); }
            _ => tr.skip_field(w)?,
        }
    }
    let Some(content) = content else { return Ok(None) };

    let mut model_node = 0u32;
    let mut turret_spaced = Vec::new();
    let mut gun_origin: Option<[f32; 3]> = None;
    let mut yaw_limits: Option<YawLimitsInfo> = None;
    let mut guns = Vec::new();
    let mut cr = Reader { buf: content, pos: 0 };
    while let Some(res) = cr.tag() {
        let (f, w) = res?;
        match (f, w) {
            (3, 0) => model_node = cr.varint()? as u32,
            // TurretModelDefinition.armor（field2）= 炮塔 Armor（thickness map + spaced）
            (2, 2) => {
                let l = cr.varint()? as usize;
                turret_spaced = parse_armor_spaced(cr.bytes(l)?)?;
            }
            // TurretModelDefinition.gun_origin（field4）
            (4, 2) => { let l = cr.varint()? as usize; gun_origin = parse_vec3(cr.bytes(l)?)?; }
            // TurretModelDefinition.yaw（field6）= YawLimits{min=1, max=2}（度）
            (6, 2) => {
                let l = cr.varint()? as usize;
                let yb = cr.bytes(l)?;
                let mut yr = Reader { buf: yb, pos: 0 };
                let mut yl = YawLimitsInfo { min: 0.0, max: 0.0 };
                while let Some(res2) = yr.tag() {
                    let (f2, w2) = res2?;
                    match (f2, w2) {
                        (1, 5) => { let b = yr.bytes(4)?; yl.min = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                        (2, 5) => { let b = yr.bytes(4)?; yl.max = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                        _ => yr.skip_field(w2)?,
                    }
                }
                yaw_limits = Some(yl);
            }
            (5, 2) => {
                let gl = cr.varint()? as usize;
                let gb = cr.bytes(gl)?;
                if let Some(g) = parse_model_gun(gb)? {
                    guns.push(g);
                }
            }
            _ => cr.skip_field(w)?,
        }
    }
    Ok(Some(TurretModelInfo { module_id: tmod, model_node, turret_spaced, gun_origin, yaw_limits, guns }))
}

/// 解析 models.pb 中单个主炮条目（GunModelDefinition：1=armor 2=thickness 3=model_id 4=pitch 5=mask）。
fn parse_model_gun(gb: &[u8]) -> Result<Option<GunModelInfo>> {
    let mut gr = Reader { buf: gb, pos: 0 };
    let mut gmod = 0u32;
    let mut inner: Option<&[u8]> = None;
    while let Some(res) = gr.tag() {
        let (f, w) = res?;
        match (f, w) {
            (1, 0) => gmod = gr.varint()? as u32,
            (2, 2) => { let l = gr.varint()? as usize; inner = Some(gr.bytes(l)?); }
            _ => gr.skip_field(w)?,
        }
    }
    let Some(inner) = inner else {
        return Ok(Some(GunModelInfo { gun_module_id: gmod, model_node: 0, thickness: None, mask: None, gun_spaced: Vec::new(), pitch_limits: None }));
    };

    let mut ir = Reader { buf: inner, pos: 0 };
    let mut model_node = 0u32;
    let mut thickness = None;
    let mut mask = None;
    let mut gun_spaced = Vec::new();
    let mut pitch_limits: Option<PitchLimitsInfo> = None;
    while let Some(res) = ir.tag() {
        let (f, w) = res?;
        match (f, w) {
            (3, 0) => model_node = ir.varint()? as u32,
            // float 固定 32 位小端
            (2, 5) => { let b = ir.bytes(4)?; thickness = Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]])); }
            (5, 5) => { let b = ir.bytes(4)?; mask = Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]])); }
            // GunModelDefinition.armor（field1）= 炮管 Armor（thickness map + spaced）
            (1, 2) => { let l = ir.varint()? as usize; gun_spaced = parse_armor_spaced(ir.bytes(l)?)?; }
            // GunModelDefinition.pitch（field4）= PitchLimits{min=1, max=2, front=3, back=4, transition=5}
            (4, 2) => {
                let l = ir.varint()? as usize;
                let pb = ir.bytes(l)?;
                let mut pr = Reader { buf: pb, pos: 0 };
                let mut pl = PitchLimitsInfo { min: 0.0, max: 0.0, front: None, back: None, transition: None };
                let extrema = |b: &[u8]| -> Result<PitchExtremaInfo> {
                    let mut er = Reader { buf: b, pos: 0 };
                    let mut e = PitchExtremaInfo { min: 0.0, max: 0.0, range: 0.0 };
                    while let Some(res2) = er.tag() {
                        let (f2, w2) = res2?;
                        match (f2, w2) {
                            (1, 5) => { let b = er.bytes(4)?; e.min = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                            (2, 5) => { let b = er.bytes(4)?; e.max = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                            (3, 5) => { let b = er.bytes(4)?; e.range = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                            _ => er.skip_field(w2)?,
                        }
                    }
                    Ok(e)
                };
                while let Some(res2) = pr.tag() {
                    let (f2, w2) = res2?;
                    match (f2, w2) {
                        (1, 5) => { let b = pr.bytes(4)?; pl.min = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                        (2, 5) => { let b = pr.bytes(4)?; pl.max = f32::from_le_bytes([b[0], b[1], b[2], b[3]]); }
                        (3, 2) => { let l2 = pr.varint()? as usize; pl.front = Some(extrema(pr.bytes(l2)?)?); }
                        (4, 2) => { let l2 = pr.varint()? as usize; pl.back = Some(extrema(pr.bytes(l2)?)?); }
                        (5, 5) => { let b = pr.bytes(4)?; pl.transition = Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]])); }
                        _ => pr.skip_field(w2)?,
                    }
                }
                pitch_limits = Some(pl);
            }
            _ => ir.skip_field(w)?,
        }
    }
    Ok(Some(GunModelInfo { gun_module_id: gmod, model_node, thickness, mask, gun_spaced, pitch_limits }))
}

/// 读取单辆坦克的模型节点信息。
pub fn model_info(tank_id: u32) -> Option<TankModelInfo> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<Vec<TankModelInfo>>> = OnceLock::new();
    let vec = CACHE.get_or_init(|| {
        let bytes = std::fs::read(crate::data::data_path("models.pb")).ok()?;
        parse_models_pb(&bytes).ok().filter(|v| !v.is_empty())
    });
    vec.as_ref().and_then(|v| v.iter().find(|t| t.tank_id == tank_id)).cloned()
}

#[cfg(test)]
mod parse_tests {
    use super::*;
    #[test]
    fn parse_pb_smoke() {
        let bytes = std::fs::read(crate::data::data_path("tanks.pb")).unwrap();
        let tanks = parse_tanks_pb(&bytes).unwrap();
        assert!(tanks.len() > 700, "expect >700 tanks, got {}", tanks.len());
        let is7 = tanks.iter().find(|t| t.tank_id == 7169).expect("IS-7");
        assert_eq!(is7.name, "IS-7");
        assert_eq!(is7.tier, 10);
        assert_eq!(is7.tank_type, "heavyTank");
        assert_eq!(is7.hp, 2040);
        assert!(is7.speed_forward > 30.0);
        // gun/shell
        let gun = &is7.turrets[0].guns[0];
        let ap = gun.shells.iter().find(|s| s.shell_type == "ap").unwrap();
        assert_eq!(ap.penetration as i64, 251);
        assert_eq!(ap.damage as i64, 460);
        assert_eq!(ap.module_damage as i64, 180);
        assert!((gun.reload.reload - 12.4).abs() < 0.5, "reload {}", gun.reload.reload);
        assert!(!gun.reload.is_burst);
        // 引擎数据（IS-7: V-2-54SCP 900 马力）
        assert!(!is7.engines.is_empty(), "IS-7 should have engine");
        let eng = &is7.engines[0];
        assert!(eng.power > 0.0, "engine power");
        assert!(eng.fire_chance > 0.0 && eng.fire_chance < 0.5, "fire chance");
        // 弹速/射程/远距穿深
        assert!(ap.velocity > 0.0, "shell velocity");
        assert!(ap.range > 0.0, "shell range");
        assert!(ap.penetration_far > 0.0 && ap.penetration_far <= ap.penetration, "pen far");
        // T57 heavy = magazine
        let t57 = tanks.iter().find(|t| t.tank_id == 14881).expect("T57");
        let g0 = &t57.turrets[0].guns[0];
        assert!(g0.reload.is_burst && !g0.reload.is_drum, "T57 should be magazine");
        assert_eq!(g0.reload.burst_size as i64, 3);
        // Progetto 65 = drum
        let pr = tanks.iter().find(|t| t.tank_id == 385).expect("Progetto");
        let g0 = &pr.turrets[0].guns[0];
        assert!(g0.reload.is_burst && g0.reload.is_drum, "Progetto should be drum");
        assert_eq!(g0.reload.burst_reloads.len(), 3);
    }

    #[test]
    fn parse_models_pb_smoke() {
        let bytes = std::fs::read(crate::data::data_path("models.pb")).unwrap();
        let ms = parse_models_pb(&bytes).unwrap();
        assert!(ms.len() > 700, "expect >700 tanks, got {}", ms.len());
        // Tiger II: 两炮塔，炮塔 8209 → turret_01, 8465 → turret_02；
        // gun 2321→2 / 10513→7 / 10769→8 (炮塔 A)，2321→4 / 10513→9 / 10769→10 (炮塔 B)
        let tiger = ms.iter().find(|m| m.tank_id == 5137).expect("Tiger II");
        assert_eq!(tiger.turrets.len(), 2);
        let a = &tiger.turrets[0];
        assert_eq!(a.module_id, 8209);
        assert_eq!(a.model_node, 1);
        assert_eq!(a.guns.iter().map(|g| (g.gun_module_id, g.model_node)).collect::<Vec<_>>(),
                   vec![(2321, 2), (10513, 7), (10769, 8)]);
        let b = &tiger.turrets[1];
        assert_eq!(b.module_id, 8465);
        assert_eq!(b.model_node, 2);
        assert_eq!(b.guns.iter().map(|g| (g.gun_module_id, g.model_node)).collect::<Vec<_>>(),
                   vec![(2321, 4), (10513, 9), (10769, 10)]);
        // AC Celeno: V2/V3 共享模型节点 3
        let cel = ms.iter().find(|m| m.tank_id == 20081).expect("AC Celeno");
        let t2 = &cel.turrets[1];
        assert_eq!(t2.module_id, 58481);
        assert_eq!(t2.guns.iter().map(|g| (g.gun_module_id, g.model_node)).collect::<Vec<_>>(),
                   vec![(47217, 2), (48497, 3), (52849, 3)]);
    }

    /// Armor.spaced 解析（炮管/车体/炮塔装甲板的 spaced 分类）。
    /// 实测（2026-09）：炮管安装甲普遍为 spaced——穿透它不算击穿坦克。
    #[test]
    fn parse_models_pb_spaced() {
        let bytes = std::fs::read(crate::data::data_path("models.pb")).unwrap();
        let ms = parse_models_pb(&bytes).unwrap();
        // Jg.Pz. E 100: gun spaced = [1,2,3,4]（17cm Pak），hull spaced = [13,15]
        let jpz = ms.iter().find(|m| m.tank_id == 12049).expect("Jg.Pz. E 100");
        let gun = &jpz.turrets[0].guns[0];
        assert_eq!(gun.gun_spaced, vec![1, 2, 3, 4]);
        assert_eq!(jpz.hull_spaced, vec![13, 15]);
        // E 100: gun spaced = [1,2,3,4,5]；hull spaced 含 10/13；turret spaced 含 9
        let e100 = ms.iter().find(|m| m.tank_id == 9489).expect("E 100");
        assert_eq!(e100.turrets[0].guns[0].gun_spaced, vec![1, 2, 3, 4, 5]);
        assert!(e100.hull_spaced.contains(&10) && e100.hull_spaced.contains(&13));
        assert!(e100.turrets[0].turret_spaced.contains(&9));
        // T-34: 所有炮的 gun spaced = [1,2,3]
        let t34 = ms.iter().find(|m| m.tank_id == 1).expect("T-34");
        for t in &t34.turrets {
            for g in &t.guns {
                assert_eq!(g.gun_spaced, vec![1, 2, 3], "T-34 gun spaced");
            }
        }
    }
}



