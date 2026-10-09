#!/usr/bin/env python3
"""本机客户端解包 → `data/tanks.pb` / `data/models.pb`（保持 pb 格式，运行期零代码改动）。

⚠️ **未接线（2026-10-09 试接后回退）**：现役 `data/*.pb` 是 BlitzKit 版本；本地解包版本
验证完成前**不得**用本工具产物替换它们。试接批的差异与待验证项见
`docs/game-data-sources.md` §5.2b。

数据源：`data/cache/local_pb/{tanks,models}.json`（`tools/extract_vehicles.py` 产物，客户端
XML / `.sc2`/`.scg` 解包），只编码 `src/wargaming/blitzkit.rs` 运行期实际读取的字段
（`parse_tanks_pb` / `parse_models_pb` 的字段表），其余 pb 字段不生成。

补充表：`data/local_pb_supplement.json` —— 客户端**没有对应概念或数据**、暂沿用既有 BlitzKit
值的字段，全部集中在补充表里显式记录（换源不假装它们是客户端数据）：

  * `dev_name`     —— BK 的 slug；客户端无此概念（`game_data` 写产物用，丢它会全量重写 735 文件）
  * `tank_name` / `shell_name` / `gun_name` / `track_name` / `engine_name`
                   —— 客户端缺专名的 145 处（车名 24 / 弹 73 / 枪 17 / 履带 19 / 引擎 12）
  * `initial_turret_rotation` —— 4 辆；源在 `.sc2` 节点变换，两端尚未解析

用法：
    python tools/emit_vehicle_pb.py --emit-supplement   # 从现有 pb + local_pb 生成补充表
    python tools/emit_vehicle_pb.py                     # 编码写 data/tanks.pb + data/models.pb
    python tools/emit_vehicle_pb.py --out-dir /tmp/x    # 写到别处（默认 data/）
"""

from __future__ import annotations

import argparse
import json
import pathlib
import struct
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent


# ---------------------------------------------------------------------------
# protobuf 基元（手写编码；只写运行期读取的字段）
# ---------------------------------------------------------------------------
def varint(n: int) -> bytes:
    if n < 0:
        raise ValueError(f"varint 不支持负数: {n}")
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def f_bytes(f: int, payload: bytes) -> bytes:
    return varint((f << 3) | 2) + varint(len(payload)) + payload


def f_str(f: int, s: str) -> bytes:
    return f_bytes(f, s.encode("utf-8"))


def f_varint(f: int, n: int) -> bytes:
    return varint(f << 3) + varint(n)


def f_i32(f: int, n: float) -> bytes:
    return f_varint(f, int(round(n)))


def f_f32(f: int, x: float) -> bytes:
    return varint((f << 3) | 5) + struct.pack("<f", float(x))


def name_msg(name: str) -> bytes:
    """本地化名消息：`{1: {"en": name}}`——Rust `extract_name` 按 `\\x02en\\x12` 标记定位。"""
    entry = f_bytes(1, b"en") + f_str(2, name)
    return f_bytes(1, entry)


def vec3(v) -> bytes:
    out = b""
    for i, x in enumerate(v[:3]):
        out += f_f32(i + 1, x)
    return out


def is_zero_vec(v) -> bool:
    """全 0（或缺失）向量 = 无值：BK 对这类字段直接省略，写零向量会把 None 变成 [0,0,0]。"""
    return (not v) or all(float(x) == 0.0 for x in v[:3])


def is_hollow_bbox(b) -> bool:
    """全 0 包围盒 = 空心占位（43 辆无炮塔 TD 的炮塔盒），不写字段（= 无数据）。"""
    return (not b) or (all(x == 0 for x in b["min"]) and all(x == 0 for x in b["max"]))


def bbox_msg(b) -> bytes:
    return f_bytes(1, vec3(b["min"])) + f_bytes(2, vec3(b["max"]))


def armor_msg(plates: dict, spaced) -> bytes:
    """`{1: map<uint32,float>, 2: repeated uint32}`；0 厚板省略（与 BlitzKit 序列化器一致）。"""
    out = b""
    for k in sorted(plates, key=lambda x: int(x)):
        v = float(plates[k])
        if v == 0.0:
            continue
        out += f_bytes(1, f_varint(1, int(k)) + f_f32(2, v))
    if spaced:
        # 逐项 (2,0) varint（与 BlitzKit 序列化器同形；packed (2,2) 运行期也支持，
        # 但 tools/compare_vehicle_data.py 的解析器只按逐项读）。保序不排序——
        # 运行期 SectionArmor.spaced 是 BTreeSet，顺序无消费方影响。
        for s in spaced:
            out += f_varint(2, int(s))
    return out


def pitch_msg(p: dict) -> bytes:
    out = f_f32(1, p.get("min", 0.0)) + f_f32(2, p.get("max", 0.0))
    for f, key in ((3, "front"), (4, "back")):
        e = p.get(key)
        if isinstance(e, dict):
            out += f_bytes(f, f_f32(1, e.get("min", 0.0)) + f_f32(2, e.get("max", 0.0))
                           + f_f32(3, e.get("range", 0.0)))
    if p.get("transition") is not None:
        out += f_f32(5, p["transition"])
    return out


CLASS_CODE = {"mediumTank": 1, "heavyTank": 2, "AT-SPG": 3}


# ---------------------------------------------------------------------------
# tanks.pb
# ---------------------------------------------------------------------------
def reload_msg(r: dict) -> bytes:
    if r.get("is_drum"):
        # 弹鼓：**field1 重复** N 次（各发装填）+ field2 间隔 + field3 容量
        # （Rust 按字段号读；顺序编号会全错位——曾是本编码器的 bug）
        body = b"".join(f_f32(1, float(x)) for x in (r.get("burst_reloads") or []))
        body += f_f32(2, r.get("burst_interval") or 0.0)
        body += f_f32(3, r.get("burst_size") or 0.0)
        return f_bytes(3, body)
    if r.get("is_burst"):
        return f_bytes(2, f_f32(1, r.get("reload") or 0.0)
                       + f_f32(2, r.get("burst_interval") or 0.0)
                       + f_f32(3, r.get("burst_size") or 0.0))
    return f_bytes(1, f_f32(1, r.get("reload") or 0.0))


def shell_msg(s: dict, name: str) -> bytes:
    out = f_varint(1, int(s["id"]))
    if name:
        out += f_bytes(2, name_msg(name))
    out += f_i32(3, s.get("velocity") or 0)
    out += f_i32(4, s.get("damage") or 0)
    out += f_i32(5, s.get("module_damage") or 0)
    out += f_f32(6, s.get("caliber") or 0.0)
    out += f_str(7, s.get("shell_type") or "")
    out += f_bytes(8, f_f32(1, s.get("penetration") or 0.0)
                   + f_f32(2, s.get("penetration_far") or 0.0))
    # field9 = 弹种权威枚举（客户端 shells.xml <kind>）；0=AP 按 proto3 零值省略（与 BK 同形）
    if s.get("shell_type_id"):
        out += f_varint(9, int(s["shell_type_id"]))
    out += f_f32(10, s.get("normalization") or 0.0)
    out += f_f32(11, s.get("ricochet") or 0.0)
    out += f_f32(12, s.get("explosion_radius") or 0.0)
    out += f_i32(13, s.get("range") or 0)
    return out


def gun_msg(g: dict, sup: dict) -> bytes:
    out = reload_msg(g.get("reload") or {})
    out += f_varint(4, int(g["module_id"]))
    out += f_f32(5, g.get("caliber_factor") or 0.0)  # 运行期 gun 有效性门槛（saw_rotation）
    gname = sup["gun_name"].get(str(g["module_id"])) or g.get("name") or ""
    if gname:
        out += f_bytes(8, name_msg(gname))
    out += f_i32(9, g.get("shell_count") or 0)
    for s in g.get("shells") or []:
        sname = sup["shell_name"].get(str(s["id"])) or s.get("name") or ""
        out += f_bytes(10, shell_msg(s, sname))
    out += f_f32(12, g.get("aim_time") or 0.0)
    out += f_f32(13, g.get("dispersion") or 0.0)
    return out


def turret_msg(t: dict, sup: dict) -> bytes:
    out = f_varint(1, int(t["module_id"]))
    out += f_i32(2, t.get("health") or 0)
    out += f_i32(3, t.get("view_range") or 0)
    out += f_f32(4, t.get("traverse_speed") or 0.0)
    out += f_i32(8, t.get("weight") or 0)
    for g in t.get("guns") or []:
        out += f_bytes(9, gun_msg(g, sup))
    return out


def tank_payload(t: dict, sup: dict) -> bytes:
    tid = str(t["tank_id"])
    out = b""
    dev = sup["dev_name"].get(tid)
    if dev:
        out += f_str(2, dev)
    out += f_i32(10, t.get("hp") or 0)
    out += f_str(11, t.get("nation") or "")
    name = (sup["tank_name"].get(tid) or t.get("name") or t.get("model_name") or "")
    out += f_bytes(12, name_msg(name))
    if t.get("is_premium"):
        out += f_varint(13, 1)
    elif t.get("is_collector"):
        out += f_varint(13, 2)
    out += f_i32(16, t.get("tier") or 0)
    if CLASS_CODE.get(t.get("tank_type") or ""):
        out += f_varint(17, CLASS_CODE[t["tank_type"]])
    for tu in t.get("turrets") or []:
        out += f_bytes(20, turret_msg(tu, sup))
    for i, e in enumerate(t.get("engines") or []):
        ename = sup["engine_name"].get(f"{t['tank_id']}:{i}") or e.get("name") or ""
        em = b""
        if ename:
            em += f_bytes(2, name_msg(ename))
        em += f_f32(5, e.get("fire_chance") or 0.0)
        em += f_i32(6, e.get("power") or 0)
        out += f_bytes(21, em)
    top_track = ((t.get("tracks") or [{}])[-1].get("module_id")) or 1
    for i, tr in enumerate(t.get("tracks") or []):
        tname = sup["track_name"].get(str(tr["module_id"])) or tr.get("name") or ""
        tm = f_varint(1, int(tr.get("module_id") or top_track))
        if tname:
            tm += f_bytes(3, name_msg(tname))
        tm += f_i32(4, tr.get("weight") or 0)
        tm += f_f32(5, tr.get("traverse_speed") or 0.0)
        if tr.get("resistance_hard") is not None:
            tm += f_f32(9, tr["resistance_hard"])
        if tr.get("resistance_medium") is not None:
            tm += f_f32(10, tr["resistance_medium"])
        out += f_bytes(22, tm)
    out += f_f32(25, t.get("speed_forward") or 0.0)
    out += f_f32(26, t.get("speed_reverse") or 0.0)
    if t.get("camouflage_still") is not None:  # field27 = 静止迷彩系数（客户端 invisibility.still）
        out += f_f32(27, t["camouflage_still"])
    out += f_i32(31, t.get("weight") or 0)
    out += f_str(32, t.get("model_name") or "")
    return out


def encode_tanks(tanks: list[dict], sup: dict) -> bytes:
    out = b""
    for t in sorted(tanks, key=lambda x: x["tank_id"]):
        entry = f_varint(1, int(t["tank_id"])) + f_bytes(2, tank_payload(t, sup))
        out += f_bytes(1, entry)
    return out


# ---------------------------------------------------------------------------
# models.pb
# ---------------------------------------------------------------------------
def model_gun_msg(g: dict) -> bytes:
    inner = f_bytes(1, armor_msg(g.get("gun_plates") or {}, g.get("gun_spaced") or []))
    if g.get("thickness") is not None:
        inner += f_f32(2, g["thickness"])
    inner += f_varint(3, int(g.get("model_node") or 0))
    if g.get("pitch_limits"):
        inner += f_bytes(4, pitch_msg(g["pitch_limits"]))
    if g.get("mask") is not None:
        inner += f_f32(5, g["mask"])
    return f_varint(1, int(g["gun_module_id"])) + f_bytes(2, inner)


def model_turret_msg(t: dict) -> bytes:
    inner = b""
    if not is_hollow_bbox(t.get("bbox")):
        inner += f_bytes(1, bbox_msg(t["bbox"]))
    inner += f_bytes(2, armor_msg(t.get("turret_plates") or {}, t.get("turret_spaced") or []))
    inner += f_varint(3, int(t.get("model_node") or 0))
    if not is_zero_vec(t.get("gun_origin")):  # 全 0 = 无值（BK 省略，勿写零向量）
        inner += f_bytes(4, vec3(t["gun_origin"]))
    for g in t.get("guns") or []:
        inner += f_bytes(5, model_gun_msg(g))
    yl = t.get("yaw_limits")
    if isinstance(yl, dict):
        inner += f_bytes(6, f_f32(1, yl.get("min", 0.0)) + f_f32(2, yl.get("max", 0.0)))
    return f_varint(1, int(t["module_id"])) + f_bytes(2, inner)


def model_payload(m: dict, track_module: int, sup: dict) -> bytes:
    out = f_bytes(1, armor_msg(m.get("hull_plates") or {}, m.get("hull_spaced") or []))
    if not is_zero_vec(m.get("turret_origin")):
        out += f_bytes(2, vec3(m["turret_origin"]))
    rot = m.get("initial_turret_rotation") or sup["initial_turret_rotation"].get(str(m["tank_id"]))
    if rot:
        out += f_bytes(3, f_f32(1, rot.get("yaw", 0.0)) + f_f32(2, rot.get("pitch", 0.0))
                       + f_f32(3, rot.get("roll", 0.0)))
    for t in m.get("turrets") or []:
        out += f_bytes(4, model_turret_msg(t))
    if m.get("track_thickness") is not None:
        tr = f_varint(1, int(track_module))
        tr += f_bytes(2, f_f32(1, m["track_thickness"])
                      + (f_bytes(2, vec3(m["track_origin"])) if m.get("track_origin") else b""))
        out += f_bytes(5, tr)
    if m.get("hull_bbox"):
        out += f_bytes(6, bbox_msg(m["hull_bbox"]))
    return out


def encode_models(models: list[dict], tanks: list[dict], sup: dict) -> bytes:
    top_track = {t["tank_id"]: ((t.get("tracks") or [{}])[-1].get("module_id") or 1)
                 for t in tanks}
    out = b""
    for m in sorted(models, key=lambda x: x["tank_id"]):
        entry = f_varint(1, int(m["tank_id"])) + f_bytes(2, model_payload(
            m, top_track.get(m["tank_id"], 1), sup))
        out += f_bytes(1, entry)
    return out


# ---------------------------------------------------------------------------
# 补充表
# ---------------------------------------------------------------------------
SUPPLEMENT_SECTIONS = ("dev_name", "tank_name", "shell_name", "gun_name", "track_name",
                       "engine_name", "initial_turret_rotation")


def empty_supplement() -> dict:
    return {k: {} for k in SUPPLEMENT_SECTIONS}


def emit_supplement(pb_dir: pathlib.Path, local_dir: pathlib.Path,
                    out_path: pathlib.Path) -> dict:
    """从**现有** pb（BlitzKit）+ local_pb 对照，抽客户端缺失/脏值处沿用的 BK 值。"""
    sys.path.insert(0, str(REPO_ROOT / "tools"))
    import compare_vehicle_data as C  # noqa: E402  （复用独立 pb 解析器）

    bk_t = {t["tank_id"]: t for t in C.parse_tanks((pb_dir / "tanks.pb").read_bytes())}
    bk_m = {m["tank_id"]: m for m in C.parse_models((pb_dir / "models.pb").read_bytes())}
    lc_t = {t["tank_id"]: t for t in json.loads((local_dir / "tanks.json").read_text("utf-8"))}
    lc_m = {m["tank_id"]: m for m in json.loads((local_dir / "models.json").read_text("utf-8"))}

    sup = empty_supplement()
    for tid, bt in bk_t.items():
        sup["dev_name"][str(tid)] = bt["dev_name"]
        lt = lc_t.get(tid) or {}
        if bt["name"] and bt["name"] != (lt.get("name") or ""):
            sup["tank_name"][str(tid)] = bt["name"]
        # 弹/枪/履带/引擎名：按（弹全局 id / 模块 id / tank:idx）对齐，BK 非空且与本地不同 → 补
        bk_shells = {s["id"]: s["name"] for tu in bt["turrets"] for g in tu["guns"]
                     for s in g["shells"] if s.get("name")}
        bk_guns = {g["module_id"]: g["name"] for tu in bt["turrets"] for g in tu["guns"]
                   if g.get("name")}
        bk_tracks = {t["module_id"]: t["name"] for t in bt["tracks"] if t.get("name")}
        bk_engines = {f"{tid}:{i}": e["name"] for i, e in enumerate(bt["engines"])
                      if e.get("name")}
        lc_shells = {s["id"]: s.get("name") for tu in (lt.get("turrets") or [])
                     for g in tu["guns"] for s in g["shells"]}
        lc_guns = {g["module_id"]: g.get("name") for tu in (lt.get("turrets") or [])
                   for g in tu["guns"]}
        lc_tracks = {t["module_id"]: t.get("name") for t in (lt.get("tracks") or [])}
        lc_engines = {f"{tid}:{i}": e.get("name")
                      for i, e in enumerate(lt.get("engines") or [])}
        for table, bk_v, lc_v in (("shell_name", bk_shells, lc_shells),
                                  ("gun_name", bk_guns, lc_guns),
                                  ("track_name", bk_tracks, lc_tracks),
                                  ("engine_name", bk_engines, lc_engines)):
            for key, name in bk_v.items():
                if name and name != (lc_v.get(key) or ""):
                    sup[table][str(key)] = name
        bm = bk_m.get(tid) or {}
        if bm.get("initial_turret_rotation") and not (lc_m.get(tid) or {}).get(
                "initial_turret_rotation"):
            sup["initial_turret_rotation"][str(tid)] = bm["initial_turret_rotation"]

    counts = {k: len(v) for k, v in sup.items()}
    payload = {"$comment": "客户端无对应概念/数据、暂沿用既有 BlitzKit 值的字段（见 "
                           "tools/emit_vehicle_pb.py 头部）。生成后随游戏版本人工维护。",
               "generated_from": "data/tanks.pb + data/models.pb（换源前）",
               **sup}
    out_path.write_text(json.dumps(payload, ensure_ascii=False, indent=1, sort_keys=True),
                        encoding="utf-8")
    print(f"补充表 → {out_path}")
    for k, v in counts.items():
        print(f"  {k}: {v}")
    return sup


def load_supplement(path: pathlib.Path) -> dict:
    if not path.is_file():
        print(f"!! 补充表不存在: {path}（先跑 --emit-supplement）", file=sys.stderr)
        sys.exit(2)
    raw = json.loads(path.read_text("utf-8"))
    sup = empty_supplement()
    for k in SUPPLEMENT_SECTIONS:
        sup[k] = raw.get(k) or {}
    return sup


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--local-dir", type=pathlib.Path,
                    default=REPO_ROOT / "data" / "cache" / "local_pb")
    ap.add_argument("--supplement", type=pathlib.Path,
                    default=REPO_ROOT / "data" / "local_pb_supplement.json")
    ap.add_argument("--out-dir", type=pathlib.Path, default=REPO_ROOT / "data")
    ap.add_argument("--emit-supplement", action="store_true",
                    help="从现有 pb + local_pb 生成补充表（换源前跑一次）")
    ap.add_argument("--tanks-only", action="store_true")
    ap.add_argument("--models-only", action="store_true")
    args = ap.parse_args()

    if args.emit_supplement:
        emit_supplement(args.out_dir, args.local_dir, args.supplement)
        return 0

    sup = load_supplement(args.supplement)
    tanks = json.loads((args.local_dir / "tanks.json").read_text("utf-8"))
    models = json.loads((args.local_dir / "models.json").read_text("utf-8"))

    if not args.models_only:
        blob = encode_tanks(tanks, sup)
        (args.out_dir / "tanks.pb").write_bytes(blob)
        print(f"tanks.pb  ← 客户端解包编码：{len(tanks)} 辆 / {len(blob)} 字节")
    if not args.tanks_only:
        blob = encode_models(models, tanks, sup)
        (args.out_dir / "models.pb").write_bytes(blob)
        print(f"models.pb ← 客户端解包编码：{len(models)} 辆 / {len(blob)} 字节")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
