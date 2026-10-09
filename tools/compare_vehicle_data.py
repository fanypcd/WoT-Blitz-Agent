#!/usr/bin/env python3
"""对照本机提取的车辆数据（`tools/extract_vehicles.py` 产物）与 BlitzKit pb。

流程：把 `data/tanks.pb` / `data/models.pb` 用**独立实现的 protobuf 解析**展开成与
`src/wargaming/blitzkit.rs` serde 输出同形的 dict（字段映射逐条对照 Rust 实现），
再与 `data/cache/local_pb/{tanks,models}.json` 逐字段 diff。

口径：
  * **按 tank_id 求交集**；BlitzKit 独有 = 多服务器聚合/未在本机客户端实装，属预期；
    本机独有 = 客户端磁盘上桥外车辆（开发/超测），也属预期。两者只计数不判错。
  * 交集内逐字段比对才是重点：浮点相对容差 1e-6（pb 是 f32，客户端 XML 十进制 → f64）；
    None/[]/{} 与"键缺失"视为相等（serde 的 skip_serializing_if 语义）。
  * 豁免字段：`dev_name`（BlitzKit 的 slug，客户端无此概念）、`hull_traverse`（field27
    是静止迷彩、与"车体转速"非同量）、`turrets[].name`（Rust 解析器跳过 pb 的炮塔名块，
    恒为空串）。`initial_turret_rotation` 自 2026-10-10 起有客户端源（XML
    `<turretInitialRotation>`），已纳入比对。
  * **空心包围盒 = 无数据**：min/max 全 0（或 pb 里的空 Vec3 消息，如 43 辆无炮塔 TD 的
    炮塔 bbox，`0a 00 12 00`）两侧都归一成 None，不算"零盒"（见 `is_hollow_bbox`）。

用法：
    python tools/compare_vehicle_data.py                 # 全量对照
    python tools/compare_vehicle_data.py --tank 9489     # 只看一辆的明细
"""

from __future__ import annotations

import argparse
import json
import pathlib
import struct
import sys
from collections import Counter, defaultdict

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent


# ---------------------------------------------------------------------------
# protobuf 基元
# ---------------------------------------------------------------------------
class R:
    def __init__(self, b: bytes):
        self.b, self.i = b, 0

    def varint(self) -> int:
        r = s = 0
        while True:
            x = self.b[self.i]
            self.i += 1
            r |= (x & 0x7F) << s
            if not x & 0x80:
                return r
            s += 7

    def bytes(self, n: int) -> bytes:
        d = self.b[self.i:self.i + n]
        self.i += n
        return d

    def f32(self) -> float:
        return struct.unpack_from("<f", self.bytes(4))[0]

    def tag(self):
        if self.i >= len(self.b):
            return None
        k = self.varint()
        return k >> 3, k & 7

    def skip(self, w: int) -> None:
        if w == 0:
            self.varint()
        elif w == 2:
            self.i += self.varint()
        elif w == 5:
            self.bytes(4)
        elif w == 1:
            self.bytes(8)
        else:
            raise ValueError(f"wire {w}")


def fields(b: bytes) -> list[tuple[int, int, object]]:
    r, out = R(b), []
    while (t := r.tag()) is not None:
        fn, wt = t
        if wt == 0:
            out.append((fn, wt, r.varint()))
        elif wt == 2:
            out.append((fn, wt, r.bytes(r.varint())))
        elif wt == 5:
            out.append((fn, wt, r.f32()))
        elif wt == 1:
            out.append((fn, wt, r.bytes(8)))
        else:
            raise ValueError(wt)
    return out


def extract_name(nb: bytes) -> str:
    """Rust extract_name 的复刻：定位 `\x02en\x12<len><name>` 取英文名。"""
    marker = b"\x02en\x12"
    pos = nb.find(marker)
    if pos < 0:
        return ""
    j = pos + len(marker)
    n = 0
    s = 0
    while True:
        x = nb[j]
        j += 1
        n |= (x & 0x7F) << s
        if not x & 0x80:
            break
        s += 7
    return nb[j:j + n].decode("utf-8", "replace")


def vec3(b: bytes) -> list[float]:
    v = [0.0, 0.0, 0.0]
    for fn, wt, val in fields(b):
        if wt == 5 and 1 <= fn <= 3:
            v[fn - 1] = round(float(val), 6)
    return v


def bbox(b: bytes) -> dict:
    mn, mx = None, None
    for fn, wt, v in fields(b):
        if wt == 2 and fn == 1:
            mn = vec3(v)
        elif wt == 2 and fn == 2:
            mx = vec3(v)
    return {"min": mn or [0.0] * 3, "max": mx or [0.0] * 3}


def armor(b: bytes) -> tuple[dict, list]:
    plates, spaced = {}, []
    for fn, wt, v in fields(b):
        if fn == 1 and wt == 2:
            kid = val = None
            for f2, w2, v2 in fields(v):
                if f2 == 1 and w2 == 0:
                    kid = v2
                elif f2 == 2 and w2 == 5:
                    val = round(float(v2), 6)
            if kid is not None and val:
                plates[str(kid)] = val
        elif fn == 2 and wt == 2:
            rr = R(v)
            while (t := rr.tag()) is not None:
                if t[1] == 0:
                    spaced.append(t[0] if False else rr.varint())
                elif t[1] == 2:
                    raise ValueError("unexpected spaced wire")
        elif fn == 2 and wt == 0:
            spaced.append(v)
    return plates, spaced


def pitch(b: bytes) -> dict:
    pl: dict = {"min": 0.0, "max": 0.0}  # Rust 结构体字段恒序列化（缺字段 = 默认 0.0）
    front = back = None
    for fn, wt, v in fields(b):
        if wt == 5 and fn == 1:
            pl["min"] = round(float(v), 6)
        elif wt == 5 and fn == 2:
            pl["max"] = round(float(v), 6)
        elif wt == 2 and fn == 3:
            front = _extrema(v)
        elif wt == 2 and fn == 4:
            back = _extrema(v)
        elif wt == 5 and fn == 5:
            pl["transition"] = round(float(v), 6)
    if front is not None:
        pl["front"] = front   # 空消息也要保留（Rust Some({0,0,0})）
    if back is not None:
        pl["back"] = back
    return pl


def _extrema(b: bytes) -> dict:
    e: dict = {"min": 0.0, "max": 0.0, "range": 0.0}
    for fn, wt, v in fields(b):
        if wt == 5 and fn == 1:
            e["min"] = round(float(v), 6)
        elif wt == 5 and fn == 2:
            e["max"] = round(float(v), 6)
        elif wt == 5 and fn == 3:
            e["range"] = round(float(v), 6)
    return e


# ---------------------------------------------------------------------------
# tanks.pb → TankFullData 形状
# ---------------------------------------------------------------------------
def parse_tanks(buf: bytes) -> list[dict]:
    out = []
    for fn, wt, v in fields(buf):
        if fn != 1 or wt != 2:
            continue
        tid = 0
        main = None
        for f2, w2, v2 in fields(v):
            if f2 == 1 and w2 == 0:
                tid = v2
            elif f2 == 2 and w2 == 2:
                main = v2
        if main is None:
            continue
        t = {"tank_id": tid, "dev_name": "", "model_name": "", "name": "", "nation": "",
             "tier": 0, "tank_type": "", "hp": 0, "is_premium": False, "is_collector": False,
             "speed_forward": 0.0, "speed_reverse": 0.0, "hull_traverse": 0.0, "weight": 0.0,
             "turrets": [], "engines": [], "tracks": []}
        localized = ""
        for f, w, v2 in fields(main):
            if (f, w) == (2, 2):
                t["dev_name"] = v2.decode("utf-8", "replace")
            elif (f, w) == (10, 0):
                t["hp"] = v2
            elif (f, w) == (11, 2):
                t["nation"] = v2.decode("utf-8", "replace")
            elif (f, w) == (12, 2):
                localized = extract_name(v2)
            elif (f, w) == (13, 0):
                t["is_premium"] = v2 == 1
                t["is_collector"] = v2 == 2
            elif (f, w) == (16, 0):
                t["tier"] = v2
            elif (f, w) == (17, 0):
                t["tank_type"] = {1: "mediumTank", 2: "heavyTank", 3: "AT-SPG"}.get(v2, "lightTank")
            elif (f, w) == (20, 2):
                tu = _pb_turret(v2)
                if tu is not None:
                    t["turrets"].append(tu)
            elif (f, w) == (21, 2):
                e = {"name": "", "power": 0.0, "fire_chance": 0.0}
                for f2, w2, v3 in fields(v2):
                    if (f2, w2) == (2, 2):
                        e["name"] = extract_name(v3)
                    elif (f2, w2) == (5, 5):
                        e["fire_chance"] = round(float(v3), 8)
                    elif (f2, w2) == (6, 0):
                        e["power"] = float(v3)
                t["engines"].append(e)
            elif (f, w) == (22, 2):
                tr = {"module_id": 0, "name": "", "weight": 0.0, "traverse_speed": 0.0,
                      "resistance_hard": None, "resistance_medium": None}
                for f2, w2, v3 in fields(v2):
                    if (f2, w2) == (1, 0):
                        tr["module_id"] = v3
                    elif (f2, w2) == (3, 2):
                        tr["name"] = extract_name(v3)
                    elif (f2, w2) == (4, 0):
                        tr["weight"] = float(v3)
                    elif (f2, w2) == (5, 5):
                        tr["traverse_speed"] = round(float(v3), 6)
                    elif (f2, w2) == (9, 5):
                        tr["resistance_hard"] = round(float(v3), 6)
                    elif (f2, w2) == (10, 5):
                        tr["resistance_medium"] = round(float(v3), 6)
                if tr["name"]:
                    t["tracks"].append(tr)
            elif (f, w) == (25, 5):
                t["speed_forward"] = round(float(v2), 6)
            elif (f, w) == (26, 5):
                t["speed_reverse"] = round(float(v2), 6)
            elif (f, w) == (27, 5):
                # field27 的真身是静止迷彩系数（旧实现误当转速；见 blitzkit.rs 注释）
                t["camouflage_still"] = round(float(v2), 6)
            elif (f, w) == (31, 0):
                t["weight"] = float(v2)
            elif (f, w) == (32, 2):
                t["model_name"] = v2.decode("utf-8", "replace")
        if localized:
            t["name"] = localized
        if not t["name"]:
            t["name"] = t["model_name"]
        if not t["tank_type"]:
            t["tank_type"] = "lightTank"  # Rust：类别码缺失 → 轻坦
        out.append(t)
    return out


def _pb_turret(b: bytes) -> dict | None:
    tu = {"module_id": 0, "name": "", "health": 0, "weight": 0.0, "view_range": 0.0,
          "traverse_speed": 0.0, "guns": []}
    has_gun = False
    for f, w, v in fields(b):
        if (f, w) == (1, 0):
            tu["module_id"] = v
        elif (f, w) == (2, 0):
            tu["health"] = v
        elif (f, w) == (3, 0):
            tu["view_range"] = float(v)
        elif (f, w) == (4, 5):
            tu["traverse_speed"] = round(float(v), 6)
        elif (f, w) == (8, 0):
            tu["weight"] = float(v)
        elif (f, w) == (9, 2):
            g = _pb_gun(v)
            if g is not None:
                tu["guns"].append(g)
                has_gun = True
    return tu if has_gun else None


def _pb_gun(b: bytes) -> dict | None:
    g = {"module_id": 0, "name": "", "caliber_factor": 0.0, "shell_count": 0,
         "dispersion": 0.0, "aim_time": 0.0, "shells": [],
         "reload": {"reload": 0.0, "is_burst": False, "is_drum": False,
                    "burst_size": 0.0, "burst_interval": 0.0, "burst_reloads": []}}
    saw_caliber = False
    name_b = b""
    for f, w, v in fields(b):
        if (f, w) == (8, 2):
            name_b = v
        elif (f, w) == (1, 2):   # 单发装填 {1: f32 秒}
            for f2, w2, v2 in fields(v):
                if (f2, w2) == (1, 5):
                    g["reload"]["reload"] = round(float(v2), 8)
        elif (f, w) == (2, 2):   # 弹夹 {装填, 间隔, 容量}（全 f32，无缩放）
            vals = [x[2] for x in fields(v)]
            g["reload"].update({"reload": round(float(vals[0]), 8) if len(vals) > 0 else 0.0,
                                "is_burst": True, "is_drum": False,
                                "burst_interval": round(float(vals[1]), 8) if len(vals) > 1 else 0.0,
                                "burst_size": float(vals[2]) if len(vals) > 2 else 0.0,
                                "burst_reloads": []})
        elif (f, w) == (3, 2):   # 弹鼓 [各发装填..., 间隔, 容量]
            vals = [x[2] for x in fields(v)]
            if len(vals) >= 3:
                g["reload"].update({"reload": max(vals[:-2]) if vals[:-2] else 0.0,
                                    "is_burst": True, "is_drum": True,
                                    "burst_interval": round(float(vals[-2]), 8),
                                    "burst_size": float(vals[-1]),
                                    "burst_reloads": [round(float(x), 8) for x in vals[:-2]]})
        elif (f, w) == (4, 0):
            g["module_id"] = v
        elif (f, w) == (5, 5):
            g["caliber_factor"] = round(float(v), 6)
            saw_caliber = True
        elif (f, w) == (9, 0):
            g["shell_count"] = v
        elif (f, w) == (10, 2):
            g["shells"].append(_pb_shell(v))
        elif (f, w) == (12, 5):
            g["aim_time"] = round(float(v), 8)
        elif (f, w) == (13, 5):
            g["dispersion"] = round(float(v), 8)
    g["name"] = extract_name(name_b)
    return g if saw_caliber else None


def _pb_shell(b: bytes) -> dict:
    s = {"id": 0, "name": "", "shell_type": "", "shell_type_id": None, "damage": 0.0,
         "penetration": 0.0, "module_damage": 0.0, "velocity": 0.0, "range": 0.0,
         "penetration_far": 0.0, "caliber": 0.0, "normalization": 0.0, "ricochet": 0.0,
         "explosion_radius": 0.0}
    name_b = b""
    for f, w, v in fields(b):
        if (f, w) == (1, 0):
            s["id"] = v
        elif (f, w) == (2, 2):
            name_b = v
        elif (f, w) == (3, 0):
            s["velocity"] = float(v)
        elif (f, w) == (4, 0):
            s["damage"] = float(v)
        elif (f, w) == (5, 0):
            s["module_damage"] = float(v)
        elif (f, w) == (6, 5):
            s["caliber"] = round(float(v), 6)
        elif (f, w) == (7, 2):
            s["shell_type"] = v.decode("utf-8", "replace")
        elif (f, w) == (8, 2):   # 穿深 {近距 f32, 远距 f32}
            for f2, w2, v2 in fields(v):
                if (f2, w2) == (1, 5):
                    s["penetration"] = round(float(v2), 6)
                elif (f2, w2) == (2, 5):
                    s["penetration_far"] = round(float(v2), 6)
        elif (f, w) == (10, 5):
            s["normalization"] = round(float(v), 6)
        elif (f, w) == (11, 5):
            s["ricochet"] = round(float(v), 6)
        elif (f, w) == (12, 5):
            s["explosion_radius"] = round(float(v), 6)
        elif (f, w) == (13, 0):
            s["range"] = float(v)
        elif (f, w) == (9, 0):
            s["shell_type_id"] = v
    s["name"] = extract_name(name_b)
    return s


# ---------------------------------------------------------------------------
# models.pb → TankModelInfo 形状
# ---------------------------------------------------------------------------
def parse_models(buf: bytes) -> list[dict]:
    out = []
    for fn, wt, v in fields(buf):
        if (fn, wt) != (1, 2):
            continue
        tid, content = 0, None
        for f2, w2, v2 in fields(v):
            if (f2, w2) == (1, 0):
                tid = v2
            elif (f2, w2) == (2, 2):
                content = v2
        if content is None:
            continue
        m: dict = {"tank_id": tid, "hull_spaced": [], "hull_plates": {}, "hull_bbox": None,
                   "track_thickness": None, "turret_origin": None, "track_origin": None,
                   "initial_turret_rotation": None, "turrets": []}
        for f, w, v2 in fields(content):
            if (f, w) == (1, 2):
                m["hull_plates"], m["hull_spaced"] = armor(v2)
            elif (f, w) == (2, 2):
                m["turret_origin"] = vec3(v2)
            elif (f, w) == (3, 2):
                rot = {"yaw": 0.0, "pitch": 0.0, "roll": 0.0}
                for f2, w2, v3 in fields(v2):
                    if w2 == 5 and f2 in (1, 2, 3):
                        rot[["yaw", "pitch", "roll"][f2 - 1]] = round(float(v3), 6)
                m["initial_turret_rotation"] = rot
            elif (f, w) == (4, 2):
                m["turrets"].append(_pb_model_turret(v2))
            elif (f, w) == (5, 2):
                k, th, org = None, None, None
                for f2, w2, v3 in fields(v2):
                    if (f2, w2) == (1, 0):
                        k = v3
                    elif (f2, w2) == (2, 2):
                        for f3, w3, v4 in fields(v3):
                            if (f3, w3) == (1, 5):
                                th = round(float(v4), 6)
                            elif (f3, w3) == (1, 0):
                                th = float(v4)
                            elif (f3, w3) == (2, 2):
                                org = vec3(v4)
                if k is not None:
                    if m["track_origin"] is None:
                        m["track_origin"] = org or [0.0, 0.0, 0.0]
                    if m["track_thickness"] is None:
                        m["track_thickness"] = th
            elif (f, w) == (6, 2):
                m["hull_bbox"] = bbox(v2)
        out.append(m)
    return out


def _pb_model_turret(b: bytes) -> dict:
    tu: dict = {"module_id": 0, "model_node": 0, "turret_spaced": [], "turret_plates": {},
                "bbox": None, "gun_origin": None, "yaw_limits": None, "guns": []}
    for f, w, v in fields(b):
        if (f, w) == (1, 0):
            tu["module_id"] = v
        elif (f, w) == (2, 2):
            content = v
            for f2, w2, v3 in fields(content):
                if (f2, w2) == (1, 2):
                    tu["bbox"] = bbox(v3)
                elif (f2, w2) == (2, 2):
                    tu["turret_plates"], tu["turret_spaced"] = armor(v3)
                elif (f2, w2) == (3, 0):
                    tu["model_node"] = v3
                elif (f2, w2) == (4, 2):
                    tu["gun_origin"] = vec3(v3)
                elif (f2, w2) == (5, 2):
                    tu["guns"].append(_pb_model_gun(v3))
                elif (f2, w2) == (6, 2):
                    y = {"min": 0.0, "max": 0.0}
                    for f3, w3, v4 in fields(v3):
                        if w3 == 5 and f3 in (1, 2):
                            y[["min", "max"][f3 - 1]] = round(float(v4), 6)
                    tu["yaw_limits"] = y
    return tu


def _pb_model_gun(b: bytes) -> dict:
    g: dict = {"gun_module_id": 0, "model_node": 0, "thickness": None, "mask": None,
               "gun_spaced": [], "gun_plates": {}, "pitch_limits": None}
    for f, w, v in fields(b):
        if (f, w) == (1, 0):
            g["gun_module_id"] = v
        elif (f, w) == (2, 2):
            for f2, w2, v3 in fields(v):
                if (f2, w2) == (1, 2):
                    g["gun_plates"], g["gun_spaced"] = armor(v3)
                elif (f2, w2) == (2, 5):
                    g["thickness"] = round(float(v3), 6)
                elif (f2, w2) == (3, 0):
                    g["model_node"] = v3
                elif (f2, w2) == (4, 2):
                    g["pitch_limits"] = pitch(v3)
                elif (f2, w2) == (5, 5):
                    g["mask"] = round(float(v3), 6)
    return g


# ---------------------------------------------------------------------------
# diff
# ---------------------------------------------------------------------------
SKIP_PATHS = {"dev_name", "hull_traverse"}  # initial_turret_rotation 自 2026-10-10 有客户端源，纳入比对


def is_emptyish(v) -> bool:
    return v is None or v == [] or v == {}


def is_hollow_bbox(v) -> bool:
    """全 0 包围盒 = 空心占位（无数据），归一成 None 参与比较。

    实测 43 辆无炮塔 TD 的炮塔 bbox 在两侧都是空占位：客户端 YAML 无 turret 段、XML 为全 0，
    BlitzKit 的 pb 是 4 字节空消息（min/max 空 Vec3）；Rust 严格解析（parse_vec3 空 → None）
    落成 null，而本工具的宽松解析会把空 Vec3 读成 [0,0,0]。统计口径统一为
    「空消息 / 全 0 盒 = 无数据」，避免把空心算成"双方都是零盒"而掩盖它。
    """
    if not isinstance(v, dict) or set(v) != {"min", "max"}:
        return False
    mn, mx = v["min"], v["max"]
    return (isinstance(mn, list) and isinstance(mx, list)
            and all(x == 0 for x in mn) and all(x == 0 for x in mx))


def _is_zero_vec(v) -> bool:
    """全 0 三维向量 = 无值（BK 对全 0 的原点向量直接省略；`J24` 类 TD 的 gunPosition 即 0）。"""
    return (isinstance(v, list) and len(v) == 3
            and all(isinstance(x, (int, float)) and x == 0 for x in v))


def norm(v):
    """None/[]/{} 统一成 None；浮点按 pb f32 精度取整比较（1e-6 相对容差在树遍历时做）。"""
    if is_hollow_bbox(v):
        return None
    if isinstance(v, dict) and "shell_type_id" in v and v["shell_type_id"] is None:
        # proto3 零值省略：field9 缺失 == 0 == AP（penetration.rs 同口径）
        v = {**v, "shell_type_id": 0}
    if isinstance(v, dict):
        return {k: (None if k.endswith("origin") and _is_zero_vec(x) else norm(x))
                for k, x in v.items() if not is_emptyish(x)}
    if isinstance(v, list):
        return [norm(x) for x in v]
    return v


def diff(a, b, path: str, out: Counter, examples: list, tank_id):
    """a=BlitzKit 侧，b=本机侧。path 用 [] 泛化列表下标。"""
    if path == "turrets[].name":
        return  # Rust 解析器跳过 pb 炮塔名块，恒为空串
    if path.rsplit(".", 1)[-1] in SKIP_PATHS:
        return
    if isinstance(a, dict) and isinstance(b, dict):
        for k in set(a) | set(b):
            diff(a.get(k), b.get(k), f"{path}.{k}" if path else k, out, examples, tank_id)
        return
    if isinstance(a, list) and isinstance(b, list):
        if a and isinstance(a[0], dict) and "module_id" in a[0]:
            _keyed_diff(a, b, "module_id", path, out, examples, tank_id)
            return
        if a and isinstance(a[0], dict) and "gun_module_id" in a[0]:
            _keyed_diff(a, b, "gun_module_id", path, out, examples, tank_id)
            return
        if a and isinstance(a[0], dict) and "id" in a[0]:
            _keyed_diff(a, b, "id", path, out, examples, tank_id)
            return
        if len(a) != len(b):
            out[f"{path} (长度 {len(a)}≠{len(b)})"] += 1
            if len(examples) < 6000:
                examples.append((tank_id, f"{path} 长度", f"{len(a)} vs {len(b)}"))
        for x, y in zip(a, b):
            diff(x, y, f"{path}[]", out, examples, tank_id)
        return
    if isinstance(a, bool) or isinstance(b, bool):
        if a != b:
            out[path] += 1
            if len(examples) < 6000:
                examples.append((tank_id, path, f"{a!r} vs {b!r}"))
        return
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        if abs(a - b) > 1e-5 * max(1.0, abs(a), abs(b)):
            out[path] += 1
            if len(examples) < 6000:
                examples.append((tank_id, path, f"{a} vs {b}"))
        return
    if a != b:
        out[path] += 1
        if len(examples) < 6000:
            examples.append((tank_id, path, f"{a!r} vs {b!r}"))


def _keyed_diff(a: list, b: list, key: str, path: str, out: Counter,
                examples: list, tank_id) -> None:
    """按键配对的列表 diff：配对成功的逐字段比；单侧多出的计入顺序/集合差。"""
    am = {x.get(key): (i, x) for i, x in enumerate(a)}
    bm = {x.get(key): (i, x) for i, x in enumerate(b)}
    for k in set(am) & set(bm):
        diff(am[k][1], bm[k][1], f"{path}[]", out, examples, tank_id)
    for k in sorted(set(am) - set(bm)):
        out[f"{path} 配对失败（仅 BlitzKit，键={k}）"] += 1
        if len(examples) < 6000:
            examples.append((tank_id, f"{path} 仅BK", f"{key}={k}"))
    for k in sorted(set(bm) - set(am)):
        out[f"{path} 配对失败（仅本机，键={k}）"] += 1
        if len(examples) < 6000:
            examples.append((tank_id, f"{path} 仅本机", f"{key}={k}"))
    # 顺序差：键集合相同但序列不同才计一次
    if set(am) == set(bm) and [x.get(key) for x in a] != [x.get(key) for x in b]:
        out[f"{path} 顺序不同"] += 1
        if len(examples) < 6000:
            examples.append((tank_id, f"{path} 顺序",
                             f"BK={[x.get(key) for x in a]} 本机={[x.get(key) for x in b]}"))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--pb-dir", type=pathlib.Path, default=REPO_ROOT / "data")
    ap.add_argument("--local-dir", type=pathlib.Path, default=pathlib.Path("data/cache/local_pb"))
    ap.add_argument("--tank", type=int, default=None, help="只看一辆的逐字段明细")
    args = ap.parse_args()

    bk_tanks = {t["tank_id"]: t for t in parse_tanks((args.pb_dir / "tanks.pb").read_bytes())}
    bk_models = {m["tank_id"]: m for m in parse_models((args.pb_dir / "models.pb").read_bytes())}
    lc_tanks = {t["tank_id"]: t for t in json.loads((args.local_dir / "tanks.json").read_text(encoding="utf-8"))
                if t["tank_id"] > 0}
    lc_models = {m["tank_id"]: m for m in json.loads((args.local_dir / "models.json").read_text(encoding="utf-8"))
                 if m["tank_id"] > 0}

    bk_ids = set(bk_tanks) | set(bk_models)
    lc_ids = set(lc_tanks) | set(lc_models)
    inter = sorted(bk_ids & lc_ids)
    print(f"BlitzKit {len(bk_ids)} 辆；本机 {len(lc_ids)} 辆；交集 {len(inter)}；"
          f"BlitzKit 独有 {len(bk_ids - lc_ids)}（多服务器/未实装，预期）；"
          f"本机独有 {len(lc_ids - bk_ids)}（客户端桥外车辆，预期）")

    out: Counter = Counter()
    examples: list = []
    name_mis: list = []
    compared = 0
    for tid in inter:
        if args.tank is not None and tid != args.tank:
            continue
        compared += 1
        diff(norm(bk_tanks.get(tid)), norm(lc_tanks.get(tid)), "", out, examples, tid)
        diff(norm(bk_models.get(tid)), norm(lc_models.get(tid)), "", out, examples, tid)
        bt, lt = bk_tanks.get(tid), lc_tanks.get(tid)
        if bt and lt and bt["name"] != lt["name"]:
            name_mis.append((tid, bt["name"], lt["name"]))

    print(f"\n=== 交集 {compared} 辆的逐字段差异（豁免 dev_name/hull_traverse/turrets[].name）===")
    total = sum(out.values())
    for path, n in out.most_common(40):
        print(f"  {n:6d}  {path}")
    print(f"  合计 {total} 处")
    if name_mis:
        print(f"\n=== 车名不一致 {len(name_mis)}（本地化回退，见 decoupling-status §2 C2）===")
        for tid, a, b in name_mis[:12]:
            print(f"   {tid}: BK={a!r} 本机={b!r}")
    if args.tank is not None:
        print(f"\n=== tank {args.tank} 的全部差异明细 ===")
        for tid, path, vals in examples:
            if tid == args.tank:
                print(f"   {path}: {vals}")
    else:
        print("\n=== 差异样例（每路径前 3 条）===")
        shown: Counter = Counter()
        for tid, path, vals in examples:
            key = path.split(" 长度")[0]
            if shown[key] >= 3:
                continue
            shown[key] += 1
            print(f"   [{tid}] {path}: {vals}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
