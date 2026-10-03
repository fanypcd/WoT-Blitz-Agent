#!/usr/bin/env python3
"""用本机 WoT Blitz 客户端导出坦克封面图/图标（替代 BlitzKit `tanks/{id}/icons/big.webp`）。

**不替换运行期数据源**：产物写到 `data/cache/local_tank_icons/<tank_id>.webp`，与 BlitzKit 的
`data/cache/tank_images/<tank_id>.webp` 并存，可用 `tools/compare_tank_icons.py` 或肉眼对照。

客户端来源（实测路径与形态）：

  * `Data/Gfx/UI/BigTankIcons/<name>.packed.webp.dvpl`（2312 个文件，含 `@2x` 与 `_skinN` 变体）
    —— DVPL 壳里就是**裸 webp**（RIFF/WEBP），**剥壳即得，无需重编码**。
    分辨率：基础 256×128 RGBA / `@2x` 512×256。
  * `Data/Gfx/UI/BattleScreenHUD/SmallTankIcons/<name>.packed.webp.dvpl`（1474 个）128×32，作兜底。

⚠️ **图标名是第三套命名域，与 `tanks.pb` 的模型名不规则对应**（这是本工具最绕的一环）：
`Ch01_Type59` → `china-Type59`（丢 `Ch01_`）、`GB10_Black_Prince` → `britsh-BlackPrince`
（丢 `GB10_`、去下划线、且国家标签是错拼的 `britsh`）、`StuGIII` → `germany-StugIII`（大小写不同）、
`Oth08_WH_Vindicator` → `other-Oth08_Vindicator`（丢 `WH_`）、`S04_Lago-I` → `european-S04_Lago_I`
（`-`↔`_`）、`Oth10_WarDuck` → `WarDuck`（**无国家前缀**）。
因此实现不用"拼字符串"而是**归一化索引**：两侧都去掉国家标签、转小写、去掉所有非字母数字后比对；
候选名取"完整模型名"与"去掉 `XxNN_` 前缀的短名"（另加去掉 `WH_` 前缀）。

用法：
    python tools/export_tank_icons.py --all                    # 全量 735 辆
    python tools/export_tank_icons.py --all --use-2x           # 用 @2x（512×256）
    python tools/export_tank_icons.py --tank 9489 --tank 7169
    python tools/export_tank_icons.py --all --allow-small      # 允许退到 SmallTankIcons
    python tools/export_tank_icons.py --list                   # 只打印解析结果（不写盘）

进度：原子更新 `<out>/_export_status.json`（total/done/missing/finished/results），与其他导出器同约定。
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import pathlib
import re
import sys
import time

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

# 图标文件名里的国家标签（实测出现的全集；`britsh` 是客户端数据里的错拼，须一并接受）
ICON_TAGS = ("germany", "ussr", "usa", "british", "britsh", "china", "japan", "france",
             "european", "other")
# 模型名的子部件前缀：`Ch01_Type59` / `GB10_Black_Prince` / `Oth08_WH_Vindicator` / `S04_Lago-I`
STEM_PREFIX_RE = re.compile(r"^[A-Za-z]+[0-9]+_")
# 少数车的前缀不带数字（`Ch_WZ-112v2` → 图标 `china-WZ-112v2`、`GB_Mark_I` → `britsh-Mark_I`、
# `G_KpfPz_70` → `germany-KpfPz_70`）
STEM_PREFIX_NODIGIT_RE = re.compile(r"^[A-Za-z]{1,3}_")
# 少数车多带一层无意义前缀/中缀（实测 `Oth08_WH_Vindicator` → 图标 `other-Oth08_Vindicator`）
EXTRA_PREFIX_RE = re.compile(r"^WH_")
NOISE_INFIX = ("WH_",)

# 人工核对过的别名：models.pb 的模型名（field32）→ 客户端图标**文件名**。
#
# 客户端图标名有时用内部代号/缩写/另一套拼法，`candidate_keys` 的单侧拼接拼不出来。
# 这张表**只收实测确认是同一辆车的**，宁可缺失也不猜——错挂别的车的图标比缺图更糟
# （例：`M2_med` 的近期候选只有 `usa-M7_med`、`T1_hvy` 只有 `usa-T1`，两者都是别的车，
# 故都不收）。每条依据写在行尾，便于复核。
ICON_ALIAS = {
    # uk：客户端用 Medium_N / Cruiser_N / Churchill_GC 等短名，模型名走 GB 编号
    "GB01_Medium_Mark_I": "britsh-Medium_I.packed.webp.dvpl",
    "GB05_Vickers_Medium_Mk_II": "britsh-Medium_II.packed.webp.dvpl",
    "GB06_Vickers_Medium_Mk_III": "britsh-Medium_III.packed.webp.dvpl",
    "GB58_Cruiser_Mk_III": "britsh-Cruiser_III.packed.webp.dvpl",
    "GB59_Cruiser_Mk_IV": "britsh-Cruiser_IV.packed.webp.dvpl",
    "GB40_Gun_Carrier_Churchill": "britsh-Churchill_GC.packed.webp.dvpl",     # GC = Gun Carrier
    "GB68_Matilda_Black_Prince": "britsh-MatildaBP.packed.webp.dvpl",         # BP = Black Prince
    "GB24_Centurion_Mk3": "britsh-Centurion_7-1.packed.webp.dvpl",            # 显示名即 Centurion Mk. 7/1
    "GB116_Harry_Hopkins": "british-GB116_Harry_Hopkins_I.packed.webp.dvpl",  # 仅多尾部 "I"
    # usa
    "T1_Cunningham": "usa-T1.packed.webp.dvpl",
    "M2_med": "usa-M2_MT.packed.webp.dvpl",                                   # MT = Medium Tank
    "T2_med": "usa-T2_MT.packed.webp.dvpl",
    "T7_Combat_Car": "usa-T7-cc.packed.webp.dvpl",                            # cc = combat car
    # ussr
    "ST_I": "ussr-R63_ST_IBD.packed.webp.dvpl",                               # R63 = ST-I 内部代号
    "R132_T100LT": "ussr-R132_VNII_100LT.packed.webp.dvpl",
    "R116_ISU122C_Berlin": "ussr-ISU122_Berlin.packed.webp.dvpl",
    # germany
    "E50_Ausf_M": "germany-E-50M.packed.webp.dvpl",
    "Pro_Ag_A": "germany-Leopard_PT_A.packed.webp.dvpl",                      # Leopard Prototyp A
    "G114_Rheinmetall_Scorpion": "germany-Skorpion.packed.webp.dvpl",
    "G_VK3502_BP": "germany-G_VK3502H_BP.packed.webp.dvpl",
    "H39_captured": "germany-PzKpfw38H_735.packed.webp.dvpl",                 # 显示名 Pz.Kpfw. 38H 735 (f)
    "JagdTiger_SdKfz_185": "germany-JagdTiger8_8.packed.webp.dvpl",           # 8,8 cm Pak 43 Jagdtiger
    "JagdTiger_SdKfz_185_Snowstorm": "germany-JagdTiger8_8_Snowstorm.packed.webp.dvpl",
    "S01_Frankentank": "Frankentank_event.packed.webp.dvpl",                  # 万圣节 Tankenstein；文件名无国家标签
    # france
    "D1": "france-Renault_D1.packed.webp.dvpl",
    "F73_M4A1_Revalorise": "france-Revalorise.packed.webp.dvpl",
    "AMX_M4_1945": "france-AMX_M4-45.packed.webp.dvpl",                       # AMX M4 mle. 45
}


def default_game_data() -> pathlib.Path:
    for c in GAME_DIR_CANDIDATES:
        p = pathlib.Path(c)
        if p.is_dir():
            return p
    return pathlib.Path(GAME_DIR_CANDIDATES[0])


def normalize(name: str) -> str:
    """归一化比对键：去掉国家标签 → 只留小写字母数字。

    这样 `GB10_Black_Prince`(标签 `britsh` 在另一侧)、`StuGIII`/`StugIII`、`S04_Lago-I`/`S04_Lago_I`
    都能落在同一个键上。
    """
    low = name.lower()
    for tag in ICON_TAGS:
        for sep in ("-", "_"):
            if low.startswith(tag + sep):
                low = low[len(tag) + 1:]
                break
        else:
            continue
        break
    return re.sub(r"[^a-z0-9]", "", low)


def icon_key(filename: str) -> str:
    """图标文件名 → 归一化键（剥掉 `@2x` 与 `.packed.webp.dvpl`）。"""
    n = filename.replace("@2x", "")
    if n.endswith(".packed.webp.dvpl"):
        n = n[: -len(".packed.webp.dvpl")]
    return normalize(n)


def build_index(ui_dir: pathlib.Path) -> dict:
    """→ {归一化键: [(文件名, 子目录, 是否 @2x, 是否皮肤变体)]}。

    同一辆车在 BigTankIcons 与小图标目录里都可能命中，故键下是列表，由选择函数排序。
    """
    index: dict = {}
    for sub in ("BigTankIcons", "BattleScreenHUD/SmallTankIcons"):
        d = ui_dir / sub
        if not d.is_dir():
            continue
        for p in d.iterdir():
            n = p.name
            if not n.endswith(".packed.webp.dvpl"):
                continue
            key = icon_key(n)
            if not key:
                continue
            index.setdefault(key, []).append(
                {"file": n, "sub": sub, "x2": "@2x" in n,
                 "skin": bool(re.search(r"_skin\d*(@2x)?\.packed", n))})
    return index


def pick_icon(index: dict, stem: str, display: str, use_2x: bool, allow_small: bool) -> dict | None:
    """按优先级挑一张：大图标基础档 > 大图标 @2x > （可选）小图标，且非皮肤变体优先。"""
    got = []
    for k in candidate_keys(stem, display):
        got += index.get(k, [])
    if not got and stem in ICON_ALIAS:
        # 候选键拼不出的（客户端用了内部代号/缩写/别名），回退到人工别名表。
        got = list(index.get(icon_key(ICON_ALIAS[stem]), []))
    if not got:
        return None
    prio = {"BigTankIcons": 0, "BattleScreenHUD/SmallTankIcons": 1}
    want_x2 = 1 if use_2x else 0
    ranked = sorted(
        got,
        key=lambda e: (int(e["skin"]),                       # 皮肤变体最后
                       prio[e["sub"]] if allow_small else 0,  # 不允许小图标时不动它的位次
                       abs(int(e["x2"]) - want_x2)),          # 与请求档位越接近越好
    )
    if not allow_small:
        big = [e for e in ranked if e["sub"] == "BigTankIcons"]
        if not big:
            return None
        ranked = big
    return ranked[0]


def load_display_names(game_data: pathlib.Path, lang: str = "en") -> dict:
    """`{车辆行号键: 显示名}`：`Strings/<lang>.yaml` 的 `"#<nation>_vehicles:<KEY>": "<显示名>"`。

    图标名的**额外词**来自显示名而不是模型名（`Sherman_Jumbo` 的显示名是
    "M4A3E2 Sherman Jumbo"，图标就叫 `usa-M4A3E2_Sherman_Jumbo`；`M48A1` 的显示名是
    "M48A1 Patton"，小图标叫 `usa-M48A1_Patton`）。所以候选名里必须带上它。
    """
    p = game_data / "Strings" / f"{lang}.yaml.dvpl"
    if not p.exists():
        return {}
    txt = decode_dvpl(p.read_bytes()).decode("utf-8", "replace")
    out = {}
    for m in re.finditer(r'"#(\w+)_vehicles:([A-Za-z0-9_\-\.]+)"\s*:\s*"((?:[^"\\]|\\.)*)"', txt):
        nat, key, disp = m.group(1), m.group(2), m.group(3)
        if disp:
            out[(nat, key)] = disp
            out.setdefault(key, disp)      # 无国家限定的兜底
    return out


def candidate_keys(stem: str, display: str = "") -> list[str]:
    """模型名/显示名 → 归一化候选键（去国家标签、转小写、去非字母数字后比对）。

    候选来源：模型名本身 / 去 `XxNN_` 前缀的短名 / 去中缀噪声 / **显示名**。
    归一化能吸收客户端命名域里的全部已知不规则：大小写（`StuGIII`↔`StugIII`）、
    `-`↔`_`（`S04_Lago-I`↔`S04_Lago_I`）、去下划线（`Black_Prince`↔`BlackPrince`）、
    国家标签错拼（`british`↔`britsh`）、以及无国家前缀（`WarDuck`）。
    """
    outs = [stem, display]
    short = STEM_PREFIX_RE.sub("", stem)
    if short != stem:
        outs.append(short)
    short2 = STEM_PREFIX_NODIGIT_RE.sub("", stem)
    if short2 != stem:
        outs.append(short2)
    # 模型名 + 显示名里多出来的词：`M48A1` 的显示名是 "M48 Patton"、图标却叫 `M48A1_Patton`
    # ——"模型名做前缀、显示名补后缀"这种组合单看任一侧都拼不出。
    stem_norm = normalize(stem)
    extra = [t for t in re.split(r"[^A-Za-z0-9]+", display or "")
             if t and normalize(t) not in stem_norm]
    if extra and stem:
        outs.append(stem + "_" + "_".join(extra))
    for s in list(outs):
        if not s:
            continue
        s2 = EXTRA_PREFIX_RE.sub("", s)
        if s2 != s:
            outs.append(s2)
        for noise in NOISE_INFIX:
            if noise in s:
                outs.append(s.replace(noise, ""))
    seen, keys = set(), []
    for s in outs:
        k = normalize(s)
        if k and k not in seen:
            seen.add(k)
            keys.append(k)
    return keys


def read_tank_table(pb_path: pathlib.Path) -> dict[int, dict]:
    """`data/tanks.pb` → {tank_id: {nation, stem}}（与 export_tank_glb 同口径，自包含）。"""
    import struct

    def varint(b: bytes, i: int):
        r = s = 0
        while True:
            x = b[i]
            i += 1
            r |= (x & 0x7F) << s
            if not x & 0x80:
                return r, i
            s += 7

    def fields(b: bytes):
        i, out = 0, []
        while i < len(b):
            key, i = varint(b, i)
            fn, wt = key >> 3, key & 7
            if wt == 0:
                v, i = varint(b, i)
                out.append((fn, wt, v))
            elif wt == 2:
                ln, i = varint(b, i)
                out.append((fn, wt, b[i:i + ln]))
                i += ln
            elif wt == 5:
                out.append((fn, wt, struct.unpack_from("<f", b, i)[0]))
                i += 4
            elif wt == 1:
                out.append((fn, wt, b[i:i + 8]))
                i += 8
            else:
                raise ValueError(f"wire {wt}")
        return out

    table = {}
    for fn, wt, v in fields(pb_path.read_bytes()):
        if fn != 1 or wt != 2:
            continue
        entry = fields(v)
        tid = next((x[2] for x in entry if x[0] == 1 and x[1] == 0), None)
        main = next((x[2] for x in entry if x[0] == 2 and x[1] == 2), None)
        if tid is None or main is None:
            continue
        nation = stem = ""
        for f2, w2, v2 in fields(main):
            if f2 == 11 and w2 == 2:
                nation = v2.decode("utf-8", "replace")
            elif f2 == 32 and w2 == 2:
                stem = v2.decode("utf-8", "replace")
        table[int(tid)] = {"nation": nation, "stem": stem}
    return table


def _export_one(payload: tuple) -> dict:
    tid, stem, display, game_data, out_root, use_2x, allow_small, index = payload
    ent = pick_icon(index, stem, display, use_2x, allow_small)
    if ent is None:
        return {"tank_id": tid, "stem": stem, "status": "no-icon"}
    src_path = pathlib.Path(game_data) / "Gfx/UI" / ent["sub"] / ent["file"]
    if not src_path.exists():
        return {"tank_id": tid, "stem": stem, "status": "file-missing", "src": str(src_path)}
    data = decode_dvpl(src_path.read_bytes())
    if data[:4] != b"RIFF" or data[8:12] != b"WEBP":
        return {"tank_id": tid, "stem": stem, "status": "not-webp", "src": str(src_path)}
    out = pathlib.Path(out_root)
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{tid}.webp").write_bytes(data)
    return {"tank_id": tid, "stem": stem, "status": "ok", "src": ent["file"],
            "tier": "2x" if ent["x2"] else "1x", "small": ent["sub"].endswith("SmallTankIcons"),
            "skin": ent["skin"], "bytes": len(data)}


def _write_status(path: pathlib.Path, state: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)   # 首次写入时输出目录可能尚未创建
    state["updated_at"] = time.strftime("%H:%M:%S")
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, ensure_ascii=False, indent=1), encoding="utf-8")
    tmp.replace(path)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--game-data", type=pathlib.Path, default=None)
    ap.add_argument("--pb", type=pathlib.Path, default=REPO_ROOT / "data" / "tanks.pb")
    ap.add_argument("--out", type=pathlib.Path, default=pathlib.Path("data/cache/local_tank_icons"))
    ap.add_argument("--tank", action="append", default=[])
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--use-2x", action="store_true", help="优先用 @2x（512×256）")
    ap.add_argument("--allow-small", action="store_true", help="允许退到 SmallTankIcons（128×32）")
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--lang", default="en", help="用于取显示名的 Strings 语言（图标名依赖英文名）")
    ap.add_argument("--list", action="store_true", help="只打印解析结果，不写盘")
    args = ap.parse_args()

    game_data = args.game_data or default_game_data()
    ui = game_data / "Gfx/UI"
    if not ui.is_dir():
        print(f"!! 客户端 UI 目录不存在: {ui}（用 --game-data 指定）", file=sys.stderr)
        return 2
    table = read_tank_table(args.pb)
    index = build_index(ui)
    # 别名表自检：目标必须在客户端索引里真的存在。写错文件名要立刻炸——
    # 否则别名会静默退化成"缺图"，而缺图本来就是这张表想解决的问题。
    bad_alias = [(s, f) for s, f in ICON_ALIAS.items() if icon_key(f) not in index]
    if bad_alias:
        for s, f in bad_alias:
            print(f"!! ICON_ALIAS 目标不存在于客户端: {s} -> {f}", file=sys.stderr)
        return 2
    disp_map = load_display_names(game_data, args.lang)
    def display_for(nat: str, stem: str) -> str:
        # en.yaml 的段名**就是 pb 的国家名**（uk 也是 `uk_vehicles`，实测；报告 A 说 uk 用
        # `gb_vehicles` 是错的）。仍保留裸键兜底以防 list.xml 键与模型名不一致。
        return disp_map.get((nat, stem)) or disp_map.get(stem) or ""
    n_x2 = sum(1 for v in index.values() for e in v if e['x2'])
    print(f"图标索引：BigTankIcons/SmallTankIcons 归一化键 {len(index)} 个（条目 {sum(len(v) for v in index.values())}，其中 @2x {n_x2}）")

    if args.all:
        targets = [(tid, t["stem"]) for tid, t in sorted(table.items())]
    elif args.tank:
        targets = [(int(x), table[int(x)]["stem"]) for x in args.tank]
    else:
        print("!! 需要 --tank <id> 或 --all", file=sys.stderr)
        return 2

    if args.list:
        for tid, stem in targets:
            ent = pick_icon(index, stem, display_for(table[tid]['nation'], stem), args.use_2x, args.allow_small)
            tier = ("2x" if ent["x2"] else "1x") if ent else "-"
            print(f"{tid}\t{stem}\t{(ent['file'] if ent else '未命中')}\t{tier}")
        return 0

    payloads = [(tid, stem, display_for(table[tid]['nation'], stem), str(game_data), str(args.out),
                 args.use_2x, args.allow_small, index) for tid, stem in targets]
    state = {"total": len(targets), "done": 0, "missing": 0, "finished": False, "results": []}
    started = time.time()
    status = args.out / "_export_status.json"
    _write_status(status, state)
    with concurrent.futures.ProcessPoolExecutor(max_workers=args.jobs) as pool:
        for r in pool.map(_export_one, payloads):
            if r["status"] == "ok":
                state["done"] += 1
                state["results"].append(r)
            else:
                state["missing"] += 1
                state["results"].append(r)
            if (state["done"] + state["missing"]) % 100 == 0:
                state["elapsed_sec"] = round(time.time() - started, 1)
                _write_status(status, state)
    state["finished"] = True
    state["elapsed_sec"] = round(time.time() - started, 1)
    _write_status(status, state)
    ok = [r for r in state["results"] if r["status"] == "ok"]
    small = sum(1 for r in ok if r.get("small"))
    x2 = sum(1 for r in ok if r.get("tier") == "2x")
    print(f"完成：导出 {state['done']}/{state['total']}（@2x {x2}，小图标兜底 {small}），"
          f"未命中 {state['missing']}，耗时 {state['elapsed_sec']}s")
    miss = [r for r in state["results"] if r["status"] != "ok"]
    if miss:
        print("未命中清单（客户端无该车图标）:")
        for r in miss[:30]:
            print(f"   {r['tank_id']} {r['stem']} ({r['status']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
