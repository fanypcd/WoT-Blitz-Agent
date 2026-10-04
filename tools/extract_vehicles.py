#!/usr/bin/env python3
"""从本机 WoT Blitz 客户端提取 `tanks.pb` / `models.pb` 的等价数据（同构 JSON）。

对照基准是 `src/wargaming/blitzkit.rs` 的 serde 输出形状（`TankFullData` / `TankModelInfo`），
`tools/compare_vehicle_data.py` 据此做逐字段 diff。

数据来源（全部读磁盘，**不读 `item_defs.zip`**——那是旧快照，869 条 vs 磁盘 881 条）：
  * `XML/item_defs/vehicles/<nation>/<stem>.xml.dvpl`  车辆定义（数值权威）
  * `XML/item_defs/vehicles/<nation>/list.xml.dvpl`    tier/类别/稀有度/本地化键
  * `XML/item_defs/vehicles/<nation>/components/*.xml.dvpl`  共享模块 + `<ids>` 局部 id 表
  * `Strings/en.yaml.dvpl`                             本地化显示名
  * `3d/Tanks/Parameters/<nation>/<stem>.yaml.dvpl`    碰撞盒 / maskSlice（models 侧）

tank_id 桥：`--emit-bridge` 从 `data/tanks.pb` 一次性导出
`{tank_id: {nation, stem, slug}}` → `data/tank_id_bridge.json`（735 条，入库后提取链
不再依赖 BlitzKit）。客户端磁盘上还有 ~146 个桥外的车辆 XML（开发/超测/未实装），
默认跳过、计数上报；`--all-local` 才提取它们（tank_id 记 -文件数序号，仅供对照）。

实现铁律（来自 docs/feasibility-pb-local-extraction.md §4，违反会静默出错）：
  1. 重复 XML 标签 **last-wins**（`<armor_N>`/`<pitchLimits>`/`<transition>` 会重复）；
  2. 0/缺 = 省略语义：`<weight>`/`<gunPosition>`/`<hullPosition>`/全 0 bbox ⇔ pb 字段缺失；
     0 厚板不入 `plates`，但 `spaced` 保留（零厚板可带 vehicleDamageFactor）；
  3. 模块 id = `(局部 id << 8) | 国家低位`，国家低位 ussr=1 germany=17 usa=33 china=49
     france=65 uk=81 japan=97 other=113 european=129（模块局部 id 来自 components 的 `<ids>`，
     弹种局部 id 来自 shells.xml 条目的 `<id>`）；
  4. 弹夹/弹鼓判定**只认车辆内联** `<clip>`/`<pumpGunMode>`（共享定义里的 clip 不算，
     否则 6 辆车假弹夹）；`<burst>` 是表现参数，禁止用来判 is_burst；
  5. `mask` = yaml `maskSlice.<node>.planePosition[1]`（+Y 分量），`enabled:false` ⇒ 省略；
  6. `pitchLimits` 首值=min 次值=max（不取负）；`extraPitchLimits` 的 front/back 为
     `min max range` 三值；`<transition>` 重复时 last-wins；
  7. `-180 180` 的 yawLimits ⇒ pb 省略（输出 null）；
  8. `caliber_factor`（pb 注释里的"口径系数"）实为炮定义的 `<rotationSpeed>`。

用法：
    python tools/extract_vehicles.py --emit-bridge              # 生成 data/tank_id_bridge.json
    python tools/extract_vehicles.py                            # 全量提取 → data/cache/local_pb/
    python tools/extract_vehicles.py --tank 9489 --tank 7169
    python tools/extract_vehicles.py --all-local                # 连桥外车辆一起提取
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import time
import xml.etree.ElementTree as ET

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

from wotb_sc2 import decode_dvpl  # noqa: E402

GAME_DIR_CANDIDATES = [
    "D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
    "C:/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
    "/mnt/c/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
    "/mnt/d/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
]

# 模块 id 的国家低位（= 报告 A §4.2 的"国家序×16+1"）
NATION_LOW = {"ussr": 1, "germany": 17, "usa": 33, "china": 49, "france": 65,
              "uk": 81, "japan": 97, "other": 113, "european": 129}
CLASS_WORDS = ("lightTank", "mediumTank", "heavyTank", "AT-SPG")


def default_game_data() -> pathlib.Path:
    for c in GAME_DIR_CANDIDATES:
        p = pathlib.Path(c)
        if p.is_dir():
            return p
    return pathlib.Path(GAME_DIR_CANDIDATES[0])


def mid(nation: str, local: int) -> int:
    return (local << 8) | NATION_LOW[nation]


# ---------------------------------------------------------------------------
# protobuf（仅 --emit-bridge 用：tanks.pb field1/field11/field2/field32）
# ---------------------------------------------------------------------------
def _pb_varint(b: bytes, i: int) -> tuple[int, int]:
    r = s = 0
    while True:
        x = b[i]
        i += 1
        r |= (x & 0x7F) << s
        if not x & 0x80:
            return r, i
        s += 7


def _pb_fields(b: bytes) -> list[tuple[int, int, object]]:
    i, out = 0, []
    while i < len(b):
        key, i = _pb_varint(b, i)
        fn, wt = key >> 3, key & 7
        if wt == 0:
            v, i = _pb_varint(b, i)
            out.append((fn, wt, v))
        elif wt == 2:
            ln, i = _pb_varint(b, i)
            out.append((fn, wt, b[i:i + ln]))
            i += ln
        elif wt == 5:
            out.append((fn, wt, struct_unpack_f(b, i)))
            i += 4
        elif wt == 1:
            out.append((fn, wt, b[i:i + 8]))
            i += 8
        else:
            raise ValueError(f"wire {wt}")
    return out


def struct_unpack_f(b: bytes, i: int) -> float:
    import struct
    return struct.unpack_from("<f", b, i)[0]


def emit_bridge(pb_path: pathlib.Path, out_path: pathlib.Path) -> int:
    buf = pb_path.read_bytes()
    table: dict[str, dict] = {}
    for fn, wt, v in _pb_fields(buf):
        if fn != 1 or wt != 2:
            continue
        entry = _pb_fields(v)
        tid = next((x[2] for x in entry if x[0] == 1 and x[1] == 0), None)
        main = next((x[2] for x in entry if x[0] == 2 and x[1] == 2), None)
        if tid is None or main is None:
            continue
        nation = stem = slug = ""
        for f2, w2, v2 in _pb_fields(main):
            if f2 == 11 and w2 == 2:
                nation = v2.decode("utf-8", "replace")
            elif f2 == 32 and w2 == 2:
                stem = v2.decode("utf-8", "replace")
            elif f2 == 2 and w2 == 2:
                slug = v2.decode("utf-8", "replace")
        table[str(tid)] = {"nation": nation, "stem": stem, "slug": slug}
    out_path.write_text(json.dumps(table, ensure_ascii=False, sort_keys=True, indent=1),
                        encoding="utf-8")
    print(f"桥接表 {len(table)} 条 → {out_path}")
    return len(table)


# ---------------------------------------------------------------------------
# 基础设施：last-wins 标量、共享定义合并、本地化
# ---------------------------------------------------------------------------
def last_child(parent: ET.Element | None, tag: str) -> ET.Element | None:
    """直接子元素中该标签的**最后一个**（重复标签 last-wins）。"""
    if parent is None:
        return None
    found = None
    for ch in parent:
        if ch.tag == tag:
            found = ch
    return found


def last_text(parent: ET.Element | None, tag: str) -> str | None:
    ch = last_child(parent, tag)
    if ch is None:
        return None
    return (ch.text or "").strip() or None


def ffloat(parent: ET.Element | None, tag: str, default: float = 0.0) -> float:
    t = last_text(parent, tag)
    try:
        return float(t) if t is not None else default
    except ValueError:
        return default


def fint(parent: ET.Element | None, tag: str, default: int = 0) -> int:
    t = last_text(parent, tag)
    try:
        return int(float(t)) if t is not None else default
    except ValueError:
        return default


class DefResolver:
    """车辆内联条目 + components 共享定义的合并视图（内联覆盖共享，逐标签 last-wins）。"""

    def __init__(self, inline: ET.Element | None, shared: ET.Element | None):
        self.inline, self.shared = inline, shared

    def has_shared_marker(self) -> bool:
        return bool(self.inline is not None and (self.inline.text or "").strip() == "shared")

    def elem(self, tag: str) -> ET.Element | None:
        """合并后该标签的元素（内联优先，否则共享）。"""
        el = last_child(self.inline, tag) if self.inline is not None else None
        if el is not None:
            return el
        return last_child(self.shared, tag) if self.shared is not None else None

    def text(self, tag: str) -> str | None:
        el = self.elem(tag)
        return (el.text or "").strip() if el is not None else None

    def floats(self, tag: str, default=0.0) -> float:
        el = self.elem(tag)
        try:
            return float((el.text or "").strip()) if el is not None else default
        except ValueError:
            return default

    def ints(self, tag: str, default=0) -> int:
        el = self.elem(tag)
        try:
            return int(float((el.text or "").strip())) if el is not None else default
        except ValueError:
            return default

    def inline_elem(self, tag: str) -> ET.Element | None:
        return last_child(self.inline, tag) if self.inline is not None else None

    def user_key(self) -> str | None:
        """userString 的 KEY（`#germany_vehicles:Turret_2_E-100` → `Turret_2_E-100`）。"""
        for tag in ("userString", "shortUserString"):
            t = self.text(tag)
            if t:
                return t.split(":", 1)[1] if ":" in t else t
        return None


class Components:
    """一个国家的 components/*.xml：`<ids>` 局部 id 表 + `<shared>` 定义。"""

    def __init__(self, comp_dir: pathlib.Path):
        self.ids: dict[str, dict[str, int]] = {}
        self.shared: dict[str, dict[str, ET.Element]] = {}
        for kind in ("guns", "shells", "engines", "chassis", "turrets", "radios", "fuelTanks"):
            self.ids[kind] = {}
            self.shared[kind] = {}
        for kind in ("guns", "engines", "chassis", "turrets"):
            p = comp_dir / f"{kind}.xml.dvpl"
            if not p.exists():
                continue
            root = ET.fromstring(decode_dvpl(p.read_bytes()))
            ids_el = last_child(root, "ids")
            if ids_el is not None:
                for ch in ids_el:
                    m = re.fullmatch(r"\d+", (ch.text or "").strip())
                    if m is not None:
                        self.ids[kind][ch.tag] = int(m.group(0))
            shared_el = last_child(root, "shared")
            if shared_el is not None:
                for ch in shared_el:
                    self.shared[kind][ch.tag] = ch
        # 弹种：顶层条目自带 <id>（无 ids/shared 分节）
        p = comp_dir / "shells.xml.dvpl"
        if p.exists():
            root = ET.fromstring(decode_dvpl(p.read_bytes()))
            self.shared["shells"] = {ch.tag: ch for ch in root
                                     if ch.tag not in ("icons", "nextAvailableId")
                                     and last_child(ch, "id") is not None}
            for name, el in self.shared["shells"].items():
                v = last_text(el, "id")
                if v is not None:
                    self.ids["shells"][name] = int(float(v))

    def local_id(self, kind: str, name: str) -> int | None:
        return self.ids.get(kind, {}).get(name)

    def shared_def(self, kind: str, name: str) -> ET.Element | None:
        return self.shared.get(kind, {}).get(name)


class Names:
    """`Strings/en.yaml` 的 `#<nation>_vehicles:<KEY>` → 显示名。"""

    def __init__(self, game_data: pathlib.Path, lang: str = "en"):
        p = game_data / "Strings" / f"{lang}.yaml.dvpl"
        self.map: dict[tuple[str, str], str] = {}
        if not p.exists():
            return
        txt = decode_dvpl(p.read_bytes()).decode("utf-8", "replace")
        self.bare: dict[str, str] = {}
        for m in re.finditer(r'"#(\w+)_vehicles:([A-Za-z0-9_.\-]+)"\s*:\s*"([^"]*)"', txt):
            if m.group(3):
                self.map[(m.group(1), m.group(2))] = m.group(3)
                self.bare.setdefault(m.group(2), m.group(3))

    def display(self, nation: str, key: str | None) -> str | None:
        if not key:
            return None
        # list.xml 的 userString 段前缀（uk 车是 gb_vehicles）与 en.yaml 的段名（uk_vehicles）
        # 可能不同，故 (nation,key) 未命中时回退裸键（车键跨系基本唯一）
        return self.map.get((nation, key)) or self.bare.get(key)


# ---------------------------------------------------------------------------
# 装甲（plates/spaced）与俯仰
# ---------------------------------------------------------------------------
def parse_armor(armor_el: ET.Element | None) -> tuple[dict[str, float], list[int]]:
    """`<armor>` → (plates, spaced)。0 厚板不入 plates（last-wins：后值 0 会删掉前值）；
    带 vehicleDamageFactor 的板（含 0 厚）入 spaced。"""
    plates: dict[str, float] = {}
    spaced: list[int] = []
    if armor_el is None:
        return plates, spaced
    for ch in armor_el:
        m = re.fullmatch(r"armor_(\d+)", ch.tag)
        if not m:
            continue
        n = int(m.group(1))
        val_text = (ch.text or "").strip()
        try:
            val = float(val_text) if val_text else 0.0
        except ValueError:
            val = 0.0
        key = str(n)
        if val != 0.0:
            plates[key] = val
        else:
            plates.pop(key, None)  # last-wins：后值 0 覆盖前值非 0
        if last_child(ch, "vehicleDamageFactor") is not None and n not in spaced:
            spaced.append(n)
    return plates, spaced


def parse_pitch(gun: DefResolver) -> dict | None:
    """`<pitchLimits>` + `<extraPitchLimits>` → PitchLimitsInfo 形状（缺省省略键）。"""
    pl = gun.elem("pitchLimits")
    if pl is None:
        return None
    vals = (pl.text or "").split()
    if len(vals) < 2:
        return None
    out: dict = {"min": float(vals[0]), "max": float(vals[1])}
    ext = gun.elem("extraPitchLimits")
    if ext is not None:
        for side in ("front", "back"):
            el = last_child(ext, side)
            if el is not None:
                v = (el.text or "").split()
                if len(v) >= 3:
                    out[side] = {"min": float(v[0]), "max": float(v[1]), "range": float(v[2])}
        tr = last_child(ext, "transition")
        if tr is not None:
            try:
                out["transition"] = float((tr.text or "").strip())
            except ValueError:
                pass
    return out


def parse_vec_text(s: str | None) -> list[float] | None:
    if not s:
        return None
    v = [float(x) for x in s.split()]
    return v if len(v) == 3 else None


# ---------------------------------------------------------------------------
# 主提取
# ---------------------------------------------------------------------------
class Extractor:
    def __init__(self, game_data: pathlib.Path):
        self.gd = game_data
        self.veh_dir = game_data / "XML" / "item_defs" / "vehicles"
        self.params_dir = game_data / "3d" / "Tanks" / "Parameters"
        self.names = Names(game_data)
        self._comp: dict[str, Components] = {}
        self._list: dict[str, dict[str, ET.Element]] = {}
        self.failures: list[str] = []

    def components(self, nation: str) -> Components:
        if nation not in self._comp:
            self._comp[nation] = Components(self.veh_dir / nation / "components")
        return self._comp[nation]

    def list_entries(self, nation: str) -> dict[str, ET.Element]:
        if nation not in self._list:
            p = self.veh_dir / nation / "list.xml.dvpl"
            out: dict[str, ET.Element] = {}
            if p.exists():
                root = ET.fromstring(decode_dvpl(p.read_bytes()))
                for ch in root:
                    if ch.tag not in ("nextAvailableId",):
                        out[ch.tag] = ch
            self._list[nation] = out
        return self._list[nation]

    # -- 名字 -----------------------------------------------------------------
    def disp(self, nation: str, r: DefResolver) -> str | None:
        return self.names.display(nation, r.user_key())

    # -- tanks.json（TankFullData 形状）---------------------------------------
    def extract_tank(self, nation: str, stem: str) -> dict:
        low = NATION_LOW[nation]
        comp = self.components(nation)
        veh_path = self.veh_dir / nation / f"{stem}.xml.dvpl"
        root = ET.fromstring(decode_dvpl(veh_path.read_bytes()))
        list_el = self.list_entries(nation).get(stem)
        hull = last_child(root, "hull")

        # 稀有度：金价 ∧ (¬)collectible（list.xml）
        gold = False
        collectible = False
        tier = 0
        if list_el is not None:
            price = last_child(list_el, "price")
            gold = price is not None and last_child(price, "gold") is not None
            tags = (last_text(list_el, "tags") or "").split()
            collectible = "collectible" in tags
            tier = fint(list_el, "level")
            cls = next((w for w in CLASS_WORDS if any(w in t for t in tags)), None)
        else:
            cls = None
        tank_type = cls or "lightTank"

        name = (self.names.display(nation, _key_of(list_el, "shortUserString"))
                or self.names.display(nation, _key_of(list_el, "userString"))
                or stem)  # pb 车名 = 短名（'Pz. IV G'），非全名

        turrets: list[dict] = []
        t0 = last_child(root, "turrets0")
        if t0 is not None:
            for t_el in t0:
                tname = t_el.tag
                tshared = comp.shared_def("turrets", tname)
                tr = DefResolver(t_el, tshared)
                tlocal = comp.local_id("turrets", tname)
                guns: list[dict] = []
                guns_el = tr.elem("guns")
                if guns_el is not None:
                    for g_el in guns_el:
                        gshared = comp.shared_def("guns", g_el.tag)
                        gr = DefResolver(g_el, gshared)
                        guns.append(self.extract_gun(nation, low, comp, gr, g_el))
                    # pb 的 guns[] 按模块 id 升序（E-100: 150mm(1050) 在 128mm(1052) 前，与 XML 文档序相反）
                    guns.sort(key=lambda g: g["module_id"])
                turrets.append({
                    "module_id": mid(nation, tlocal) if tlocal is not None else 0,
                    "name": self.disp(nation, tr) or tname,
                    "health": tr.ints("maxHealth"),
                    "weight": tr.floats("weight"),
                    "view_range": tr.floats("circularVisionRadius"),
                    "traverse_speed": tr.floats("rotationSpeed"),
                    "guns": guns,
                })

        engines: list[dict] = []
        e_el = last_child(root, "engines")
        if e_el is not None:
            for entry in e_el:
                er = DefResolver(entry, comp.shared_def("engines", entry.tag))
                engines.append({
                    "name": self.disp(nation, er) or entry.tag,
                    "power": er.floats("power"),
                    "fire_chance": er.floats("fireStartingChance"),
                })

        tracks: list[dict] = []
        c_el = last_child(root, "chassis")
        if c_el is not None:
            for entry in c_el:
                cr = DefResolver(entry, comp.shared_def("chassis", entry.tag))
                res = (cr.text("terrainResistance") or "").split()
                tracks.append({
                    "module_id": mid(nation, comp.local_id("chassis", entry.tag) or 0),
                    "name": self.disp(nation, cr) or entry.tag,
                    "weight": cr.floats("weight"),
                    "traverse_speed": cr.floats("rotationSpeed"),
                    "resistance_hard": float(res[0]) if res else None,
                    "resistance_medium": float(res[1]) if len(res) > 1 else None,
                })

        speed = last_child(root, "speedLimits")
        hp = fint(hull, "maxHealth")  # pb field10 = 车体 maxHealth（E-100 2200，非车体+炮塔之和）
        return {
            "tank_id": 0,  # 由调用方按桥接表填
            "dev_name": "",  # BlitzKit 的 slug，客户端无此概念，比较时跳过
            "model_name": stem,
            "name": name,
            "nation": nation,
            "tier": tier,
            "tank_type": tank_type,
            "hp": hp,
            "is_premium": bool(gold and not collectible),
            "is_collector": bool(gold and collectible),
            "speed_forward": ffloat(speed, "forward"),
            "speed_reverse": ffloat(speed, "backward"),
            "hull_traverse": 0.0,  # field27 与客户端非同量，不提取（比较时跳过）
            "weight": ffloat(hull, "weight"),
            "turrets": turrets,
            "engines": engines,
            "tracks": tracks,
        }

    # -- 单门炮 ---------------------------------------------------------------
    def extract_gun(self, nation: str, low: int, comp: Components,
                    gr: DefResolver, g_el: ET.Element) -> dict:
        # 弹夹/弹鼓判定**只认车辆内联**（铁律 4）
        inline_clip = gr.inline_elem("clip")
        inline_pump = gr.inline_elem("pumpGunMode")
        reload_time = gr.floats("reloadTime")
        if inline_pump is not None:
            times = [float(x) for x in (gr.text("pumpGunReloadTimes") or "").split()]
            clip = gr.inline_elem("clip")
            rate = ffloat(clip, "rate") if clip is not None else 0.0
            reload = {
                "reload": max(times) if times else reload_time,
                "is_burst": True, "is_drum": True,
                "burst_size": ffloat(clip, "count") if clip is not None else 0.0,
                "burst_interval": (60.0 / rate) if rate else 0.0,
                "burst_reloads": times,
            }
        elif inline_clip is not None:
            count = ffloat(inline_clip, "count")
            rate = ffloat(inline_clip, "rate")
            reload = {
                "reload": reload_time,
                "is_burst": True, "is_drum": False,
                "burst_size": count,
                "burst_interval": (60.0 / rate) if rate else 0.0,
                "burst_reloads": [],
            }
        else:
            reload = {"reload": reload_time, "is_burst": False, "is_drum": False,
                      "burst_size": 0.0, "burst_interval": 0.0, "burst_reloads": []}

        shells: list[dict] = []
        # shots 的两处来源：**共享 gun 定义**的条目带弹道参数（speed/piercingPower/...），
        # **车辆内联**的 `<shots>` 决定这炮装哪些弹、顺序如何（条目是 `<shell>shared<price>` 引用，
        # 无弹道字段）。逐弹合并：键序取内联（缺则共享），弹道字段内联 last-wins 回退共享。
        inline_shots = gr.inline_elem("shots")
        shared_shots = last_child(gr.shared, "shots") if gr.shared is not None else None
        order = inline_shots if inline_shots is not None else shared_shots
        if order is not None:
            for shot in order:
                if inline_shots is not None:
                    counterpart = last_child(shared_shots, shot.tag) if shared_shots is not None else None
                    traj = DefResolver(shot, counterpart)
                else:
                    traj = DefResolver(None, shot)
                shell_def = comp.shared_def("shells", shot.tag)
                s_el = shell_def
                sid = comp.local_id("shells", shot.tag)
                pp = (traj.text("piercingPower") or "").split()
                shells.append({
                    "id": mid(nation, sid) if sid is not None else 0,
                    "name": (self.names.display(nation, _key_of_el(s_el, "userString"))
                             or shot.tag),
                    "shell_type": last_text(s_el, "icon") or "",
                    "damage": 0.0,
                    "penetration": float(pp[0]) if pp else 0.0,
                    "module_damage": 0.0,
                    "velocity": traj.floats("speed"),
                    "range": traj.floats("maxDistance"),
                    "penetration_far": float(pp[1]) if len(pp) > 1 else 0.0,
                    "caliber": ffloat(s_el, "caliber"),
                    "normalization": ffloat(s_el, "normalizationAngle"),
                    "ricochet": ffloat(s_el, "ricochetAngle"),
                    "explosion_radius": ffloat(s_el, "explosionRadius"),
                    "_damage_armor": last_text(last_child(s_el, "damage"), "armor") if s_el is not None else None,
                    "_damage_devices": last_text(last_child(s_el, "damage"), "devices") if s_el is not None else None,
                })
        for s in shells:
            s["damage"] = float(s.pop("_damage_armor") or 0)
            s["module_damage"] = float(s.pop("_damage_devices") or 0)
        # 不排序：pb 的 shells/guns 顺序来自 BlitzKit 自身管线（与其 XML 文档序、id 序都不同），
        # 属非规范差异；对照器的键配对已消除顺序影响

        return {
            "module_id": mid(nation, comp.local_id("guns", g_el.tag) or 0),
            "name": self.disp(nation, gr) or g_el.tag,
            "caliber_factor": gr.floats("rotationSpeed"),  # pb"口径系数"实为炮 rotationSpeed
            "shell_count": gr.ints("level"),  # pb field9 = 炮的 <level>（"shell_count"是 Rust 侧误名；maxAmmo 在 pb field18，Rust 不读）
            "dispersion": gr.floats("shotDispersionRadius"),
            "aim_time": gr.floats("aimingTime"),
            "shells": shells,
            "reload": reload,
        }

    # -- models.json（TankModelInfo 形状）-------------------------------------
    def extract_model(self, nation: str, stem: str) -> dict:
        comp = self.components(nation)
        veh_path = self.veh_dir / nation / f"{stem}.xml.dvpl"
        root = ET.fromstring(decode_dvpl(veh_path.read_bytes()))
        hull = last_child(root, "hull")

        import yaml as pyyaml
        ypath = self.params_dir / nation / f"{stem}.yaml.dvpl"
        ydata = {}
        if ypath.exists():
            ydata = pyyaml.safe_load(decode_dvpl(ypath.read_bytes()).decode("utf-8", "replace")) or {}
        collision = ydata.get("collision") or {}
        mask_slice = ydata.get("maskSlice") or {}

        def bbox_of(key: str) -> dict | None:
            el = collision.get(key)
            if not isinstance(el, dict):
                return None
            bb = el.get("bbox")
            if not isinstance(bb, dict):
                return None
            mn, mx = bb.get("min"), bb.get("max")
            if not (isinstance(mn, list) and isinstance(mx, list)):
                return None
            # 注意：全 0 bbox **不**省略——实测 43 个炮塔的 pb 带全零 bbox
            return {"min": [float(x) for x in mn], "max": [float(x) for x in mx]}

        hull_plates, hull_spaced = parse_armor(last_child(hull, "armor"))
        turret_origin = parse_vec_text(last_text(last_child(hull, "turretPositions"), "turret"))

        turrets: list[dict] = []
        t0 = last_child(root, "turrets0")
        if t0 is not None:
            for t_el in t0:
                tr = DefResolver(t_el, comp.shared_def("turrets", t_el.tag))
                node = _node_from_model_path(tr, "Turret")
                guns_el = tr.elem("guns")
                guns: list[dict] = []
                if guns_el is not None:
                    for g_el in guns_el:
                        gr = DefResolver(g_el, comp.shared_def("guns", g_el.tag))
                        gnode = _node_from_model_path(gr, "Gun")
                        armor = gr.elem("armor")
                        gplates, gspaced = parse_armor(armor)
                        thick_el = last_child(armor, "gun") if armor is not None else None
                        thickness = float((thick_el.text or "0").strip()) if thick_el is not None else None
                        if thickness == 0.0:
                            thickness = None  # 0 厚度省略
                        ms = mask_slice.get(f"gun_{gnode:02d}")
                        mask = None
                        if isinstance(ms, dict) and ms.get("enabled"):
                            pp = ms.get("planePosition")
                            if isinstance(pp, list) and len(pp) >= 2:
                                mask = float(pp[1])
                        yl_text = tr.text("yawLimits")
                        yaw = None
                        if yl_text:
                            v = [float(x) for x in yl_text.split()]
                            if len(v) == 2 and not (v[0] == -180 and v[1] == 180):
                                yaw = {"min": v[0], "max": v[1]}
                        tlocal = comp.local_id("turrets", t_el.tag)
                        glocal = comp.local_id("guns", g_el.tag)
                        guns.append({
                            "gun_module_id": mid(nation, glocal) if glocal is not None else 0,
                            "model_node": gnode,
                            "thickness": thickness,
                            "mask": mask,
                            "gun_spaced": gspaced,
                            "gun_plates": gplates,
                            "pitch_limits": parse_pitch(gr),
                        })
                turrets.append({
                    "module_id": mid(nation, tlocal) if tlocal is not None else 0,
                    "model_node": node,
                    "turret_spaced": parse_armor(tr.elem("armor"))[1],
                    "turret_plates": parse_armor(tr.elem("armor"))[0],
                    "bbox": bbox_of(f"turret_{node:02d}"),
                    "gun_origin": parse_vec_text(tr.text("gunPosition")),
                    "yaw_limits": yaw,
                    "guns": guns,
                })

        # 履带：第一条 chassis 的 leftTrack 厚度 + hullPosition 原点
        track_thickness = None
        track_origin = None
        c_el = last_child(root, "chassis")
        if c_el is not None:
            first = next(iter(c_el), None)
            if first is not None:
                cr = DefResolver(first, comp.shared_def("chassis", first.tag))
                armor = cr.elem("armor")
                lt = last_text(armor, "leftTrack") if armor is not None else None
                if lt:
                    track_thickness = float(lt)
                track_origin = parse_vec_text(cr.text("hullPosition")) or [0.0, 0.0, 0.0]

        return {
            "tank_id": 0,
            "hull_spaced": hull_spaced,
            "hull_plates": hull_plates,
            "hull_bbox": bbox_of("hull"),
            "track_thickness": track_thickness,
            "turret_origin": turret_origin,
            "track_origin": track_origin,
            "initial_turret_rotation": None,  # 在 .sc2 节点变换内，客户端 XML 无
            "turrets": turrets,
        }


def _key_of(list_el: ET.Element | None, tag: str) -> str | None:
    t = last_text(list_el, tag) if list_el is not None else None
    return t.split(":", 1)[1] if t and ":" in t else t


def _key_of_el(el: ET.Element | None, tag: str) -> str | None:
    return _key_of(el, tag)


def _node_from_model_path(r: DefResolver, kind: str) -> int:
    """`models.undamaged` 路径里的 `Turret_02.model` / `Gun_04.model` → 2 / 4。"""
    models = r.elem("models")
    p = last_text(models, "undamaged") if models is not None else None
    if not p:
        return 0
    m = re.search(rf"{kind}_(\d+)\.model", p, re.IGNORECASE)
    return int(m.group(1)) if m else 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def _write_status(path: pathlib.Path, state: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    state["updated_at"] = time.strftime("%H:%M:%S")
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, ensure_ascii=False, indent=1), encoding="utf-8")
    tmp.replace(path)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--game-data", type=pathlib.Path, default=None)
    ap.add_argument("--pb", type=pathlib.Path, default=REPO_ROOT / "data" / "tanks.pb")
    ap.add_argument("--bridge", type=pathlib.Path, default=REPO_ROOT / "data" / "tank_id_bridge.json")
    ap.add_argument("--out", type=pathlib.Path, default=pathlib.Path("data/cache/local_pb"))
    ap.add_argument("--tank", action="append", default=[])
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--all-local", action="store_true",
                    help="连桥外车辆（开发/超测/未实装）一起提取，tank_id 记负数序号")
    ap.add_argument("--emit-bridge", action="store_true")
    args = ap.parse_args()

    game_data = args.game_data or default_game_data()
    if args.emit_bridge:
        emit_bridge(args.pb, args.bridge)
        return 0

    if not args.bridge.exists():
        print(f"!! 桥接表不存在: {args.bridge}（先跑 --emit-bridge）", file=sys.stderr)
        return 2
    bridge = json.loads(args.bridge.read_text(encoding="utf-8"))

    ex = Extractor(game_data)
    # 磁盘上的全部车辆 stem（按国家）
    disk: dict[str, list[str]] = {}
    for nation in NATION_LOW:
        d = ex.veh_dir / nation
        if d.is_dir():
            disk[nation] = sorted(p.stem.replace(".xml", "") for p in d.glob("*.xml.dvpl")
                                  if p.name != "list.xml.dvpl")

    targets: list[tuple[int, str, str]] = []
    if args.all or args.tank:
        want = {int(x) for x in args.tank} if args.tank else None
        for tid_s, info in sorted(bridge.items(), key=lambda kv: int(kv[0])):
            if want is not None and int(tid_s) not in want:
                continue
            targets.append((int(tid_s), info["nation"], info["stem"]))
    else:
        print("!! 需要 --tank <id> 或 --all", file=sys.stderr)
        return 2

    extra_by_stem: dict[tuple[str, str], int] = {}
    if args.all_local:
        bridged = {(info["nation"], info["stem"]) for info in bridge.values()}
        seq = -1
        for nation in NATION_LOW:
            for stem in disk.get(nation, []):
                if (nation, stem) not in bridged:
                    extra_by_stem[(nation, stem)] = seq
                    seq -= 1

    args.out.mkdir(parents=True, exist_ok=True)
    tanks_out: list[dict] = []
    models_out: list[dict] = []
    extra_out: list[dict] = []
    failures: list[dict] = []
    started = time.time()
    for i, (tid, nation, stem) in enumerate(targets):
        try:
            tk = ex.extract_tank(nation, stem)
            tk["tank_id"] = tid
            md = ex.extract_model(nation, stem)
            md["tank_id"] = tid
            tanks_out.append(tk)
            models_out.append(md)
        except Exception as e:  # noqa: BLE001
            failures.append({"tank_id": tid, "nation": nation, "stem": stem,
                             "error": f"{type(e).__name__}: {e}"})
        if (i + 1) % 100 == 0:
            print(f"  {i + 1}/{len(targets)}", flush=True)

    for (nation, stem), seq in sorted(extra_by_stem.items(), key=lambda kv: kv[1]):
        try:
            tk = ex.extract_tank(nation, stem)
            tk["tank_id"] = seq
            md = ex.extract_model(nation, stem)
            md["tank_id"] = seq
            tanks_out.append(tk)
            models_out.append(md)
            extra_out.append({"seq": seq, "nation": nation, "stem": stem})
        except Exception as e:  # noqa: BLE001
            failures.append({"tank_id": seq, "nation": nation, "stem": stem,
                             "error": f"{type(e).__name__}: {e}"})

    (args.out / "tanks.json").write_text(
        json.dumps(tanks_out, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    (args.out / "models.json").write_text(
        json.dumps(models_out, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    (args.out / "local_extra.json").write_text(
        json.dumps({"count": len(extra_out), "vehicles": extra_out}, ensure_ascii=False, indent=1),
        encoding="utf-8")
    if failures:
        (args.out / "failures.json").write_text(
            json.dumps(failures, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"完成：提取 {len(tanks_out)} 辆（桥外 {len(extra_out)}），失败 {len(failures)}，"
          f"耗时 {round(time.time() - started, 1)}s → {args.out}")
    for f in failures[:10]:
        print(f"   [fail] {f['tank_id']} {f['stem']}: {f['error']}")
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
