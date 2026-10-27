#!/usr/bin/env python3
"""导出客户端研发/配置数据面（data/research/），供 WotbTools 配置器消费。

数据权威 = 本机客户端磁盘文件（与 tools/extract_vehicles.py 同源同语义，复用其
Names/Components/DefResolver/Extractor 设施）。fail-closed：缺文件/解析失败即报错，
不猜默认值。

用法：
    python tools/export_research_loadout.py [--out data/research] [--langs en,zh-Hans]
                                            [--with-research-graph]

输出（本期消费面）：
    meta.json                   game_version / generated_at / 逐文件 sha256 / 计数
    modules/{nation}.json       逐车模块数据（含 localId/tags/槽位数/预设名/共享组件；
                                hasTurret（<hull><fakeTurrets> 标记）；chassis stats 含
                                hysteresisHealth/hullPosition，gun stats 含 rotationSpeed；
                                车辆级 bbox chassis+hull、逐炮塔 bbox 与 turretOrigin 供尺寸计算）
    devices.json                24 件装备 + 9 预设（三语名/描述/script/display_params）
    consumables.json            消耗品（含 mode 标记与 vehicleFilter）
    provisions.json             补给（同构）
    consumables_default.json    客户端按 tier 默认消耗品表 + 单车覆盖
    params/structure.json       hangarParamsStructureConfig 分组树
    params/format.json          每参数 units/digits/biggerIsBetter
    strings/{lang}/research.json 参数名（tank_descr_param_*）+ 单位缩写
    camouflage.json             涂装隐蔽加成（camouflages.xml 的 <invisibility><categories>）
    icons/items/**              物品图标（客户端 UI 样式表 bg-sprite → 图集裁剪/整图，统一 96×96）
    icons/modules/*.webp        模块类型图标 5 张（ModulesTechTree，统一 96×96）

可选（--with-research-graph，本期消费面不用，供日后科技树展示）：
    tech_tree/{nation}.json     position 坐标 + premium_rows
    research/{nation}.json      <unlocks> 边 + 成本
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import sys
from datetime import datetime, timezone

import xml.etree.ElementTree as ET

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
sys.path.insert(0, str(TOOLS_DIR))
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

from extract_vehicles import (  # noqa: E402
    Components,
    DefResolver,
    Extractor,
    Names,
    default_game_data,
    last_child,
    parse_pitch,
    parse_vec_text,
)
from wotb_sc2 import decode_dvpl  # noqa: E402

NATIONS = ["ussr", "germany", "usa", "china", "france", "uk", "japan", "other", "european"]
PRESET_GROUPS = ("attack", "defence", "specialization")
PRESET_LEVELS = ("level0", "level1", "level2")

# 客户端基础 Strings 缺后期物品名（运行时本地化覆盖层有 zh-Hans，但 en 无覆盖层）：
# 仅补标准可装配物品的 en 显示名；zh-Hans 由覆盖层解析，ru 按既有约定回退 en。
# 来源：客户端 battle 文案（tungsten_tip_in_use = "Tungsten Rounds"）与官方物品名。
ITEM_NAME_SUPPLEMENT = {
    "#artefacts:tungstentip/name": {"en": "Tungsten Rounds"},
    "provisions/headOfCheese/Name": {"en": "Head of Cheese"},
    "provisions/cinnamonRoll/Name": {"en": "Cinnamon Roll"},
    "provisions/ration_czech_hq/Name": {"en": "Beef Goulash"},
    "provisions/ration_czech_regular/Name": {"en": "Fried Cheese"},
}

# 乘员技能（BlitzKit crew-skills.json 的 20 个车型技能；训练资格按车型，效果全局生效）。
SKILL_IDS = (
    "lucky_shot", "surrounded_by_enemy", "soft_recoil", "skilled_ranging", "fast_capture",
    "battle_rage", "armour_piercing", "tutor", "smooth_driving", "smooth_turret",
    "skilled_repairer", "close_combat_master", "fire_fighting", "desperado", "repair",
    "sniper", "deadly_accuracy", "camouflage", "virtuoso", "smooth_turn",
)
# 弹种图标（AmmoSlot.yaml 的 iconPath 映射；normal 大图）。
SHELL_TYPES = (
    "ap", "ap_premium", "ap_cr", "ap_cr_premium", "he",
    "he_premium", "hc", "hc_premium", "atgm_heat",
)


def load_dvpl_xml(path: pathlib.Path) -> ET.Element:
    return ET.fromstring(decode_dvpl(path.read_bytes()))


def ftext(el: ET.Element | None, tag: str) -> str | None:
    if el is None:
        return None
    v = el.findtext(tag)
    return v.strip() if v and v.strip() else None


def ffloat(el: ET.Element | None, tag: str) -> float | None:
    v = ftext(el, tag)
    if v is None:
        return None
    try:
        return float(v)
    except ValueError:
        return None


def fint(el: ET.Element | None, tag: str) -> int | None:
    v = ftext(el, tag)
    if v is None:
        return None
    try:
        return int(float(v))
    except ValueError:
        return None


def parse_floats(s: str | None) -> list[float] | None:
    if not s:
        return None
    parts = s.replace(",", " ").split()
    try:
        return [float(x) for x in parts]
    except ValueError:
        return None


def parse_dispersion_factors(el: ET.Element, shared: ET.Element | None) -> dict | None:
    """炮/底盘 `<shotDispersionFactors>` 子项（turretRotation/afterShot/vehicleMovement/...）。"""
    node = el.find("shotDispersionFactors")
    if node is None and shared is not None:
        node = shared.find("shotDispersionFactors")
    if node is None:
        return None
    out = {}
    for child in node:
        if not isinstance(child.tag, str):
            continue
        try:
            out[child.tag] = float((child.text or "").strip())
        except ValueError:
            continue
    return out or None


def parse_cost(raw: str | None) -> dict:
    raw = (raw or "").strip()
    if not raw:
        return {"xp": 0}
    if ":" in raw:
        cur, amt = raw.split(":", 1)
        try:
            return {"tokens": {"currency": cur, "amount": int(amt)}}
        except ValueError:
            return {"raw": raw}
    if raw.isdigit():
        return {"xp": int(raw)}
    return {"raw": raw}


def module_common_fields(el: ET.Element, shared: ET.Element | None, resolver_local_id: int | None,
                         names_by_lang: dict, tables_by_lang: dict, nation: str) -> dict:
    """模块公共字段：stem/localId/名称/等级/价格 + unlocks（last-wins 解析由 ElementTree 天然保证）。"""
    user_string = ftext(el, "userString") or (ftext(shared, "userString") if shared is not None else None)
    out = {
        "stem": el.tag,
        "localId": resolver_local_id,
        "level": fint(el, "level") if fint(el, "level") is not None else (fint(shared, "level") if shared is not None else None),
        "price": ffloat(el, "price") if ffloat(el, "price") is not None else (ffloat(shared, "price") if shared is not None else None),
    }
    for lang, names in names_by_lang.items():
        resolved = names.display(nation, user_string)
        if resolved is None and user_string:
            resolved = flat_lookup(tables_by_lang[lang], user_string)
        out[f"name_{lang}"] = resolved
    unlocks = []
    u = el.find("unlocks")
    if u is not None:
        for target in u:
            if not isinstance(target.tag, str) or target.tag == "cost":
                continue
            unlocks.append({
                "kind": target.tag,
                "stem": (target.text or "").strip(),
                "cost": parse_cost(target.findtext("cost")),
            })
    out["unlocks"] = unlocks
    return out


def flat_lookup(table: dict, key: str | None) -> str | None:
    """装备/物品键：Strings 表里 `#段:键`（含 # 原文）与平键并存，按原文→去#→段后序贯查。"""
    if not key:
        return None
    for k in (key, key[1:] if key.startswith("#") else None):
        if k and k in table:
            return table[k]
    k2 = key[1:] if key.startswith("#") else key
    if ":" in k2:
        v = table.get(k2.split(":", 1)[1])
        if v is not None:
            return v
    return None


def load_localization_overlay(lang: str) -> dict:
    """运行时本地化覆盖层（客户端语言缓存）：基础 Strings 缺的后期物品名在这里。

    只读本机缓存；缺失/不可解析返回空表（fail-closed，不影响其余导出）。键是原文
    （含 `#artefacts:...` / `provisions/...`），与 flat_lookup 的查找口径一致。
    """
    for d in Names.OVERLAY_DIRS:
        p = pathlib.Path(os.path.expandvars(os.path.expanduser(d))) / f"{lang}.yaml"
        if p.exists():
            try:
                import yaml as _yaml
                data = _yaml.safe_load(p.read_text(encoding="utf-8", errors="replace"))
                return data if isinstance(data, dict) else {}
            except Exception:  # noqa: BLE001
                return {}
    return {}


def export_module_kind(root: ET.Element, kind: str, comp: Components,
                       names_by_lang: dict, tables_by_lang: dict, nation: str,
                       ids_map: dict[str, int] | None = None,
                       shared_map: dict[str, ET.Element] | None = None) -> list[dict]:
    """内联（chassis/turrets）或共享引用（engines/radios/fuelTanks）的模块列表。

    radios/fuelTanks 不在 Components 覆盖内（提取链的 pb 不需要），由 ids_map/shared_map
    传入；其 userString 在共享定义上，名称解析优先 flat_lookup。"""
    out = []
    container = last_child(root, kind if kind != "turrets" else "turrets0")
    if container is None:
        return out
    for entry in container:
        if not isinstance(entry.tag, str):
            continue
        shared = comp.shared_def(kind, entry.tag) if shared_map is None else shared_map.get(entry.tag)
        local = comp.local_id(kind, entry.tag) if ids_map is None else ids_map.get(entry.tag)
        m = module_common_fields(entry, shared, local, names_by_lang, tables_by_lang, nation)
        if kind == "chassis":
            m["stats"] = {
                "rotationSpeed": ffloat(entry, "rotationSpeed") or ffloat(shared, "rotationSpeed"),
                "terrainResistance": parse_floats(ftext(entry, "terrainResistance") or (ftext(shared, "terrainResistance") if shared is not None else None)),
                "maxLoad": ffloat(entry, "maxLoad") or ffloat(shared, "maxLoad"),
                "weight": ffloat(entry, "weight") or ffloat(shared, "weight"),
                "maxHealth": ffloat(entry, "maxHealth") or ffloat(shared, "maxHealth"),
                "bulkHealthFactor": ffloat(entry, "bulkHealthFactor") or ffloat(shared, "bulkHealthFactor"),
                # 履带耐久：客户端 hangarParams 的 chassisRepairTime = hysteresisHealth /
                # healthRegenPerSec（BI 11.21 同值：T-34 24/3.5 = 6.86 s、116-F3 66/9.625）。
                "hysteresisHealth": ffloat(entry, "hysteresisHealth") or ffloat(shared, "hysteresisHealth"),
                "healthRegenPerSec": ffloat(entry, "healthRegenPerSec") or ffloat(shared, "healthRegenPerSec"),
                # 车体在底盘上的挂点（车辆 XML <hullPosition>，顺序 [x, 上, 前]；
                # 尺寸并集换算为 [x, 前, 上] 见前端 compute）。
                "hullPosition": parse_vec_text(ftext(entry, "hullPosition") or (ftext(shared, "hullPosition") if shared is not None else None)),
                # 移动/车体转向扩圈系数（客户端 chassis <shotDispersionFactors>）
                "dispersionFactors": parse_dispersion_factors(entry, shared),
            }
        elif kind == "turrets":
            m["stats"] = {
                "rotationSpeed": ffloat(entry, "rotationSpeed") or ffloat(shared, "rotationSpeed"),
                "circularVisionRadius": ffloat(entry, "circularVisionRadius") or ffloat(shared, "circularVisionRadius"),
                "maxHealth": ffloat(entry, "maxHealth") or ffloat(shared, "maxHealth"),
                "weight": ffloat(entry, "weight") or ffloat(shared, "weight"),
            }
            yaw = parse_floats(ftext(entry, "yawLimits") or (ftext(shared, "yawLimits") if shared is not None else None))
            m["stats"]["yawLimits"] = yaw if yaw and len(yaw) == 2 else None
            # 每个炮塔自己的主装甲板（front sides rear，如 armor_1 armor_3 armor_4）：
            # 装甲表的 front/side/rear 随所选炮塔变，不能复用顶层 armor_model（顶级炮塔）映射。
            primary = ftext(entry, "primaryArmor") or (ftext(shared, "primaryArmor") if shared is not None else None)
            if primary:
                parts = primary.split()
                if len(parts) >= 3:
                    m["primaryArmor"] = {"front": parts[0], "sides": parts[1], "rear": parts[2]}
            guns = []
            guns_el = entry.find("guns")
            if guns_el is not None:
                for g in guns_el:
                    if not isinstance(g.tag, str):
                        continue
                    gshared = comp.shared_def("guns", g.tag)
                    glocal = comp.local_id("guns", g.tag)
                    gm = module_common_fields(g, gshared, glocal, names_by_lang, tables_by_lang, nation)
                    gm["stats"] = {
                        "reloadTime": ffloat(g, "reloadTime") or ffloat(gshared, "reloadTime"),
                        "aimingTime": ffloat(g, "aimingTime") or ffloat(gshared, "aimingTime"),
                        "shotDispersionRadius": ffloat(g, "shotDispersionRadius") or ffloat(gshared, "shotDispersionRadius"),
                        "maxAmmo": ffloat(g, "maxAmmo") or ffloat(gshared, "maxAmmo"),
                        "weight": ffloat(gshared, "weight") if gshared is not None else ffloat(g, "weight"),
                        "invisibilityFactorAtShot": ffloat(g, "invisibilityFactorAtShot") or ffloat(gshared, "invisibilityFactorAtShot"),
                        # 火炮转向速度（无炮塔车的“炮塔转速”替代值）：客户端 gun XML
                        # <rotationSpeed>，与炮塔 <rotationSpeed> 同单位/同系数（BK gunTraverseSpeed）。
                        "rotationSpeed": ffloat(g, "rotationSpeed") or ffloat(gshared, "rotationSpeed"),
                        # 炮塔转向/射击后/炮管损坏扩圈系数（客户端 gun <shotDispersionFactors>）
                        "dispersionFactors": parse_dispersion_factors(g, gshared),
                    }
                    # pitchLimits 是元素型（<min>/<max>[+front/back/transition]，重复标签
                    # last-wins）——复用提取链 parse_pitch 的权威解析，不自行猜文本格式。
                    gm["stats"]["pitchLimits"] = parse_pitch(DefResolver(g, gshared))
                    guns.append(gm)
            m["guns"] = guns
        elif kind == "engines":
            m["stats"] = {
                "power": ffloat(entry, "power") or ffloat(shared, "power"),
                "fireStartingChance": ffloat(entry, "fireStartingChance") or ffloat(shared, "fireStartingChance"),
                "weight": ffloat(entry, "weight") or ffloat(shared, "weight"),
                "tags": (ftext(entry, "tags") or (ftext(shared, "tags") if shared is not None else None) or "").split(),
            }
        elif kind == "radios":
            m["stats"] = {
                "distance": ffloat(entry, "distance") or ffloat(shared, "distance"),
                "weight": ffloat(entry, "weight") or ffloat(shared, "weight"),
            }
        elif kind == "fuelTanks":
            m["stats"] = {"weight": ffloat(entry, "weight") or ffloat(shared, "weight")}
        out.append(m)
    return out


def parse_filter(el: ET.Element | None) -> dict | None:
    """vehicleFilter：一个 <include>/<exclude> 内的全部条件是 **AND**；多个块之间 OR。

    客户端示例（ration）：`<include><vehicle><minLevel>6</minLevel>…</vehicle><nations>ussr</nations></include>`
    = 6-10 级 **且** 苏联车。逐条件拆成独立规则会把 AND 变成 OR（苏系食物对所有国家出现）。
    """
    if el is None:
        return None
    out = {}

    def merge(rule: dict, key: str, values: list[str]) -> None:
        rule.setdefault(key, []).extend(values)

    for side in ("include", "exclude"):
        rules = []
        for group in el.findall(side):
            rule: dict = {}
            for child in group:
                if not isinstance(child.tag, str):
                    continue
                min_level = fint(child, "minLevel")
                max_level = fint(child, "maxLevel")
                if min_level is not None or max_level is not None:
                    rule["tiers"] = {
                        "min": min_level if min_level is not None else 1,
                        "max": max_level if max_level is not None else 10,
                    }
                names = ftext(child, "name")
                if names:
                    merge(rule, "ids", names.split())
                if child.tag == "nations":
                    nations = (child.text or "").strip()
                else:
                    nations = ftext(child, "nations")
                if nations:
                    merge(rule, "nations", nations.split())
                if child.tag == "extendedTags":
                    tags = (child.text or "").strip()
                else:
                    tags = ftext(child, "extendedTags")
                if tags:
                    merge(rule, "extendedTags", tags.split())
            if rule:
                rules.append(rule)
        if rules:
            out[side] = rules
    return out or None


def parse_item_script(el: ET.Element) -> dict:
    s = el.find("script")
    if s is None:
        return {"type": None, "params": {}}
    params: dict = {}
    for c in s:
        if not isinstance(c.tag, str):
            continue
        if len(c):
            sub = {}
            for g in c:
                if not isinstance(g.tag, str):
                    continue
                sub[g.tag] = (g.text or "").strip() if not len(g) else {
                    gg.tag: (gg.text or "").strip() for gg in g if isinstance(gg.tag, str)}
            params[c.tag] = sub
        else:
            params[c.tag] = (c.text or "").strip()
    return {"type": s.tag, "params": params}


def parse_display_params(el: ET.Element) -> dict | None:
    d = el.find("display_params")
    if d is None:
        return None
    out = {}
    for c in d:
        if not isinstance(c.tag, str):
            continue
        if len(c):
            out[c.tag] = {g.tag: (g.text or "").strip() for g in c if isinstance(g.tag, str)}
        else:
            out[c.tag] = (c.text or "").strip()
    return out


def load_component(comp_dir: pathlib.Path, kind: str) -> tuple[dict[str, int], dict[str, ET.Element]]:
    """Components 未覆盖 radios/fuelTanks（提取链的 pb 不需要）；导出器自加载。"""
    ids: dict[str, int] = {}
    shared: dict[str, ET.Element] = {}
    p = comp_dir / f"{kind}.xml.dvpl"
    if not p.exists():
        return ids, shared
    root = load_dvpl_xml(p)
    ids_el = last_child(root, "ids")
    if ids_el is not None:
        import re
        for ch in ids_el:
            m = re.fullmatch(r"\d+", (ch.text or "").strip())
            if m is not None:
                ids[ch.tag] = int(m.group(0))
    shared_el = last_child(root, "shared")
    if shared_el is not None:
        for ch in shared_el:
            shared[ch.tag] = ch
    return ids, shared


def export_items(game_data: pathlib.Path, kind: str, tables_by_lang: dict[str, dict]) -> list[dict]:
    list_path = game_data / f"XML/item_defs/vehicles/common/{kind}/list.xml.dvpl"
    root = load_dvpl_xml(list_path)
    items = []
    for ref in root.findall("items"):
        mode = pathlib.PurePosixPath(ref.get("path")).stem  # common / madgames / ...
        item_root = load_dvpl_xml(game_data / f"XML/item_defs/vehicles/common/{kind}" / (ref.get("path") + ".dvpl"))
        for el in item_root:
            if not isinstance(el.tag, str) or fint(el, "id") is None:
                continue
            user = ftext(el, "userString")
            descr = ftext(el, "description")
            item = {
                "id": fint(el, "id"),
                "stem": el.tag,
                "mode": mode,
                "icon": ftext(el, "icon"),
                "category": ftext(el, "category"),
                "tags": (ftext(el, "tags") or "").split(),
                "extraEffectDuration": ffloat(el, "extraEffectDuration"),
                "script": parse_item_script(el),
                "displayParams": parse_display_params(el),
                "filter": parse_filter(el.find("vehicleFilter")),
                # 战斗内技能（isAbility）：不是机库装配项（如 desertPower 还有 geometryFilter），
                # 配置器物品池必须排除；语义标记来自客户端 XML，不按名称/缺少文案判定。
                "isAbility": el.find("isAbility") is not None,
            }
            for lang, table in tables_by_lang.items():
                item[f"name_{lang}"] = flat_lookup(table, user)
                item[f"description_{lang}"] = flat_lookup(table, descr)
                if item[f"name_{lang}"] is None and user in ITEM_NAME_SUPPLEMENT:
                    item[f"name_{lang}"] = ITEM_NAME_SUPPLEMENT[user].get(lang)
            items.append(item)
    items.sort(key=lambda x: (x["mode"] != "common", x["id"]))
    return items


def export_devices(game_data: pathlib.Path, names_by_lang: dict[str, Names],
                   tables_by_lang: dict[str, dict]) -> dict:
    root = load_dvpl_xml(game_data / "XML/item_defs/vehicles/common/optional_devices.xml.dvpl")
    devices = []
    for el in root:
        if not isinstance(el.tag, str) or fint(el, "id") is None:
            continue
        user = ftext(el, "userString")
        descr = ftext(el, "description")
        d = {
            "id": fint(el, "id"),
            "stem": el.tag,
            "icon": ftext(el, "icon"),
            "script": parse_item_script(el),
            "displayParams": parse_display_params(el),
        }
        for lang, table in tables_by_lang.items():
            d[f"name_{lang}"] = flat_lookup(table, user)
            d[f"description_{lang}"] = flat_lookup(table, descr)
        devices.append(d)
    devices.sort(key=lambda x: x["id"])

    slots_root = load_dvpl_xml(game_data / "XML/item_defs/vehicles/common/optional_device_slots.xml.dvpl")
    presets = {}
    for pr in slots_root.find("presets") or []:
        if not isinstance(pr.tag, str):
            continue
        slots = []
        for level in PRESET_LEVELS:
            level_el = pr.find(level)
            if level_el is None:
                continue
            for group in PRESET_GROUPS:
                g = level_el.find(group)
                if g is None:
                    continue
                left = (g.findtext("device0") or "").strip()
                right = (g.findtext("device1") or "").strip()
                slots.append({
                    "level": int(level[-1]),
                    "group": group,
                    "left": left or None,
                    "right": right or None,
                })
        presets[pr.tag] = slots
    return {"devices": devices, "presets": presets}


def export_params(game_data: pathlib.Path) -> tuple[dict, dict]:
    import yaml
    structure_raw = yaml.safe_load(decode_dvpl(
        (game_data / "hangarParamsStructureConfig.yaml.dvpl").read_bytes()).decode("utf-8"))
    format_raw = yaml.safe_load(decode_dvpl(
        (game_data / "hangarParamsConfig.yaml.dvpl").read_bytes()).decode("utf-8"))["keyedArchive"]

    def unwrap(node):
        if isinstance(node, dict) and set(node) == {"keyedArchive"}:
            return {k: unwrap(v) for k, v in node["keyedArchive"].items()}
        if isinstance(node, dict):
            return {k: unwrap(v) for k, v in node.items()}
        if isinstance(node, list):
            return [unwrap(x) for x in node]
        return node

    structure = unwrap(structure_raw)
    fmt = {}
    for key, node in (format_raw or {}).items():
        inner = unwrap(node.get("keyedArchive", {})) if isinstance(node, dict) else {}
        entry = {}
        if "units" in inner and isinstance(inner["units"], dict) and "string" in inner["units"]:
            entry["units"] = inner["units"]["string"]
        if "digits" in inner and isinstance(inner["digits"], dict):
            entry["digits"] = inner["digits"].get("int32")
        if "biggerIsBetter" in inner and isinstance(inner["biggerIsBetter"], dict):
            entry["biggerIsBetter"] = inner["biggerIsBetter"].get("bool", True)
        fmt[key] = entry
    return {"types": structure.get("types"), "mainSections": structure.get("mainSections"),
            "sections": structure.get("sections")}, fmt


def export_strings(game_data: pathlib.Path, langs: list[str], param_keys: list[str]) -> dict:
    out = {}
    for lang in langs:
        text = decode_dvpl((game_data / f"Strings/{lang}.yaml.dvpl").read_bytes()).decode("utf-8")
        import yaml
        table = yaml.safe_load(text) or {}
        entries = {}
        for key in param_keys:
            k = f"tank_descr_param_{key}"
            if k in table:
                entries[key] = table[k]
        for uk in [k for k in table if k.startswith("unit_measure_short_")]:
            entries[uk] = table[uk]
        out[lang] = entries
    return out


def export_camouflage(game_data: pathlib.Path) -> dict:
    """涂装隐蔽加成（客户端 camouflages.xml）：按涂装类别的逐车型系数。

    权威 = `XML/item_defs/vehicles/common/camouflages.xml` 的
    `<invisibility><categories>`；配置器的“涂装”开关用 `free` 类别（标准免费涂装，
    与 disposable/common-camo-credits 等常规涂装同值）。
    """
    root = load_dvpl_xml(game_data / "XML/item_defs/vehicles/common/camouflages.xml.dvpl")
    container = root.find("invisibility/categories")
    if container is None:
        raise RuntimeError("camouflages.xml 缺少 <invisibility><categories>")
    categories: dict[str, dict[str, float]] = {}
    for category in container:
        if not isinstance(category.tag, str):
            continue
        values = {}
        for child in category:
            if not isinstance(child.tag, str):
                continue
            raw = (child.text or "").strip()
            if not raw:
                continue
            values[child.tag] = float(raw)
        if values:
            categories[category.tag] = values
    if not categories:
        raise RuntimeError("camouflages.xml 的涂装类别为空")
    return {"defaultCategory": "free", "categories": categories}


# 图标来源（权威 = 客户端 UI 样式表的 bg-sprite 映射，不按文件名猜）：
#   - 装备/消耗品/补给：InventoryBigStyles（inventory 面板）+ 事件面板样式
#   - 装备：OptionalDevicesItemImage.style（装备页专用，含新版 supply 树图标）
#   - Normal 档兜底：少数物品只在 Normal 样式登记（如 desertPower）
ICON_STYLE_MAPS = (
    "UI/Styles/Lobby/Inventory/InventoryBigStyles.yaml.dvpl",
    "UI/Styles/Lobby/Inventory/Event/InventoryBigStyles.yaml.dvpl",
    "UI/Screens3/Lobby/Inventory/OptionalDevices/OptionalDevicesItemImage.style.yaml.dvpl",
    "UI/Styles/Lobby/Inventory/InventoryNormalStyles.yaml.dvpl",
    "UI/Styles/Lobby/Inventory/Event/InventoryNormalStyles.yaml.dvpl",
)
ICON_KINDS = (
    ("devices", "OptionalDevices"),
    ("consumables", "Consumables"),
    ("provisions", "Provisions"),
)
MODULE_ICON_KINDS = ("chassis", "engine", "gun", "hull", "turret")


def parse_icon_style_map(game_data: pathlib.Path) -> dict[str, str]:
    """`<class>`（= 物品 XML 的 <icon>）→ bg-sprite 路径。"""
    import re
    pattern = re.compile(
        r'\.(?:equipment_item|optional_device_item)\.([A-Za-z0-9_\-]+)[^\n]*\n[^\n]*\n'
        r'\s+properties:\s*\n\s+bg-sprite:\s*"([^"]+)"')
    mapping: dict[str, str] = {}
    for rel in ICON_STYLE_MAPS:
        p = game_data / rel
        if not p.exists():
            continue
        text = decode_dvpl(p.read_bytes()).decode("utf-8")
        for cls, sprite in pattern.findall(text):
            mapping.setdefault(cls, sprite)
    if not mapping:
        raise RuntimeError("客户端 UI 样式表里没有找到任何 icon 映射")
    return mapping


def _sprite_frames(txt: str) -> list[tuple[int, int, int, int]]:
    lines = [line.strip() for line in txt.splitlines() if line.strip()]
    count = int(lines[3].split()[0])
    frames = []
    for line in lines[4:4 + count]:
        parts = line.split()
        frames.append((int(parts[0]), int(parts[1]), int(parts[2]), int(parts[3])))
    return frames


def _load_sprite(game_data: pathlib.Path, bases: list[str], cache: dict) -> "object":
    """按候选基名取图标：`<base>.txt.dvpl`（图集裁剪）优先，其次 `<base>.packed.webp.dvpl`（整图）。"""
    import io as _io
    from PIL import Image
    for base in bases:
        txt = game_data / f"{base}.txt.dvpl"
        if txt.exists():
            text = decode_dvpl(txt.read_bytes()).decode("utf-8")
            lines = [line.strip() for line in text.splitlines() if line.strip()]
            atlas = pathlib.PurePosixPath(base).parent / lines[1]
            atlas_path = game_data / f"{atlas}.dvpl"
            key = str(atlas_path)
            if key not in cache:
                cache[key] = Image.open(_io.BytesIO(decode_dvpl(atlas_path.read_bytes()))).convert("RGBA")
            image = cache[key]
            x, y, w, h = _sprite_frames(text)[0]
            if x + w > image.width or y + h > image.height:
                raise RuntimeError(f"icon frame out of atlas: {base} {x},{y},{w},{h} > {image.size}")
            return image.crop((x, y, x + w, y + h)).copy()
        packed = game_data / f"{base}.packed.webp.dvpl"
        if packed.exists():
            return Image.open(_io.BytesIO(decode_dvpl(packed.read_bytes()))).convert("RGBA")
    raise FileNotFoundError(f"icon sprite not found: {bases}")


def _normalize_icon(image: "object", canvas: int = 96, pad: int = 6) -> "object":
    """统一视觉尺寸：裁到 alpha 内容框，按比例缩放后居中放进固定方形画布。

    客户端图集帧是逐图标紧裁的（内容 34~83px 不等），新版 supply 树单图还带大块透明边；
    直接按原画布 contain 会出现“同一个格子里的图标大小不一”。这是对同类资产的统一变换
    （不是逐图标判定），保证 68 张物品图与 5 张模块类型图同一光学尺寸。
    """
    from PIL import Image
    bbox = image.getchannel("A").getbbox()
    if bbox:
        image = image.crop(bbox)
    inner = canvas - 2 * pad
    scale = min(inner / image.width, inner / image.height)
    size = (max(1, round(image.width * scale)), max(1, round(image.height * scale)))
    resized = image.resize(size, Image.LANCZOS)
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    out.paste(resized, ((canvas - size[0]) // 2, (canvas - size[1]) // 2), resized)
    return out


def export_icons(game_data: pathlib.Path, out_dir: pathlib.Path, devices: dict,
                 consumables: list[dict], provisions: list[dict]) -> dict:
    """装备/消耗品/补给图标 + 模块类型图标（客户端图集裁剪；缺件记录在 manifest.missing）。

    只导出配置器会展示的物品（装备全部 + common 消耗品/补给）；事件模式图标不在本期消费面。
    """
    mapping = parse_icon_style_map(game_data)
    cache: dict = {}
    manifest: dict = {"modules": {}, "items": {}, "missing": []}
    missing: list[dict] = []

    # 图标目录由导出器全权生成：旧一代残留（如早期 vehicle*.webp）不进入新 manifest，
    # 先清空避免“包里有图但 manifest 没有”的歧义。
    import shutil
    icons_root = out_dir / "icons"
    if icons_root.exists():
        shutil.rmtree(icons_root)

    for kind, sub in ICON_KINDS:
        if kind == "devices":
            items = devices["devices"]
        elif kind == "consumables":
            items = [item for item in consumables if item["mode"] == "common"]
        else:
            items = [item for item in provisions if item["mode"] == "common"]
        exported: dict[str, dict] = {}
        for item in items:
            icon = item.get("icon")
            if not icon or icon in exported:
                continue
            bases: list[str] = []
            sprite = mapping.get(icon)
            if sprite:
                rel = sprite.replace("~res:/", "")
                bases.append(rel.rsplit(".", 1)[0] if rel.endswith((".txt", ".psd", ".png")) else rel)
            bases.append(f"Gfx/UI/InventoryIcons/Big/{sub}/{icon}")
            try:
                image = _load_sprite(game_data, bases, cache)
            except FileNotFoundError:
                missing.append({"kind": kind, "stem": item["stem"], "icon": icon})
                continue
            normalized = _normalize_icon(image)
            dest = out_dir / "icons" / "items" / kind / f"{icon}.webp"
            dest.parent.mkdir(parents=True, exist_ok=True)
            normalized.save(dest, "WEBP", lossless=True)
            exported[icon] = {"file": f"items/{kind}/{icon}.webp", "w": normalized.width, "h": normalized.height}
        manifest["items"][kind] = exported

    module_dir = game_data / "Gfx/UI/ModulesTechTree"
    for kind in MODULE_ICON_KINDS:
        src = module_dir / f"vehicle{kind.capitalize()}.packed.webp.dvpl"
        raw = decode_dvpl(src.read_bytes())
        if not raw.startswith((b"RIFF", b"\x89PNG")):
            raise RuntimeError(f"unexpected module icon payload: {src.name} head={raw[:8].hex()}")
        import io as _io
        from PIL import Image
        normalized = _normalize_icon(Image.open(_io.BytesIO(raw)).convert("RGBA"))
        dest = out_dir / "icons" / "modules" / f"{kind}.webp"
        dest.parent.mkdir(parents=True, exist_ok=True)
        normalized.save(dest, "WEBP", lossless=True)
        manifest["modules"][kind] = f"modules/{kind}.webp"

    manifest["missing"] = missing
    return manifest


def export_skills(game_data: pathlib.Path, out_dir: pathlib.Path,
                  tables_by_lang: dict[str, dict]) -> dict:
    """乘员技能：客户端 avatar.xml 的 class 归属/效果参数/图标 + Strings 文案。

    effect/tip 保留客户端原文（含 %(...) 参数占位）；`effects` 是 skill 元素的数值子项
    （per-level 小数，如 camouflageFactorPerLevel=0.03），前端/引擎不得另写常量。
    """
    root = load_dvpl_xml(game_data / "XML/item_defs/tankmen/avatar.xml.dvpl")
    skill_nodes = {el.tag: el for el in (root.find("skills") or []) if isinstance(el.tag, str)}
    class_map: dict[str, list[str]] = {}
    for cls in root.find("skillsByClasses") or []:
        if isinstance(cls.tag, str):
            class_map[cls.tag] = (cls.text or "").split()

    manifest: dict[str, dict] = {}
    missing: list[dict] = []
    for skill_id in SKILL_IDS:
        node = skill_nodes.get(skill_id)
        if node is None:
            missing.append({"kind": "skill-definition", "id": skill_id})
            continue
        icon_name = None
        for icon in node.findall("icon"):
            icon_name = ftext(icon, "name")
            if icon_name:
                break
        if not icon_name:
            missing.append({"kind": "skill-icon", "id": skill_id})
            continue
        base = icon_name.replace("~res:/", "")
        try:
            image = _load_sprite(game_data, [base], {})
        except FileNotFoundError:
            missing.append({"kind": "skill-icon", "id": skill_id})
            continue
        image = _normalize_icon(image)
        dest = out_dir / "icons" / "skills" / f"{skill_id}.webp"
        dest.parent.mkdir(parents=True, exist_ok=True)
        image.save(dest, "WEBP", lossless=True)

        effects: dict[str, object] = {}
        skill_type = None
        for child in node:
            if not isinstance(child.tag, str):
                continue
            if child.tag in ("userString", "effectDescription", "tipDescription", "featuresDescription"):
                continue
            if child.tag == "icon":
                continue
            raw = (child.text or "").strip()
            if child.tag == "type":
                skill_type = raw or None
                continue
            try:
                effects[child.tag] = float(raw)
            except ValueError:
                if raw:
                    effects[child.tag] = raw
        entry: dict[str, object] = {
            "icon": f"skills/{skill_id}.webp",
            "effects": effects,
        }
        if skill_type:
            entry["type"] = skill_type
        classes = [cls for cls, ids in class_map.items() if skill_id in ids]
        if classes:
            entry["classes"] = classes
        for lang, table in tables_by_lang.items():
            prefix = f"#crew_skills:{skill_id}/"
            for field in ("name", "effect", "tip", "features"):
                value = table.get(prefix + field)
                if value:
                    entry[f"{field}_{lang}"] = value.strip()
        manifest[skill_id] = entry
    return {"skills": manifest, "classes": class_map, "missing": missing}

def export_shell_icons(game_data: pathlib.Path, out_dir: pathlib.Path) -> dict:
    """弹种图标（AmmoSlot.yaml 的 iconPath → ammunition/normal/<type>_m）。"""
    manifest: dict[str, str] = {}
    missing: list[dict] = []
    cache: dict = {}
    for shell_type in SHELL_TYPES:
        try:
            image = _load_sprite(
                game_data, [f"Gfx/Shared/tank-supply/ammunition/normal/{shell_type}_m"], cache)
        except FileNotFoundError:
            missing.append({"kind": "shell-icon", "type": shell_type})
            continue
        image = _normalize_icon(image)
        dest = out_dir / "icons" / "shells" / f"{shell_type}.webp"
        dest.parent.mkdir(parents=True, exist_ok=True)
        image.save(dest, "WEBP", lossless=True)
        manifest[shell_type] = f"shells/{shell_type}.webp"
    return {"shells": manifest, "missing": missing}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="data/research")
    ap.add_argument("--langs", default="en,zh-Hans")
    ap.add_argument("--with-research-graph", action="store_true")
    ap.add_argument("--game-data", default=None,
                    help="客户端 Data 根目录（默认自动探测；预览客户端可显式指定）")
    ap.add_argument("--bridge", default=None,
                    help="tank-id 映射 JSON（默认 data/tank_id_bridge.json；"
                         "预览增量可给只含新增车的映射）")
    ap.add_argument("--modules-only", action="store_true",
                    help="只导出逐车 modules/*.json（预览客户端增量；不改装备/参数/图标）")
    ap.add_argument("--merge", action="store_true",
                    help="与 --modules-only 连用：把本次导出的车并入既有 modules/*.json，"
                         "保留其余车")
    args = ap.parse_args()
    if args.merge and not args.modules_only:
        ap.error("--merge 只能与 --modules-only 连用")
    langs = [x.strip() for x in args.langs.split(",") if x.strip()]
    out_root = REPO_ROOT / args.out
    out_root.mkdir(parents=True, exist_ok=True)
    game_data = pathlib.Path(args.game_data) if args.game_data else default_game_data()
    bridge_path = pathlib.Path(args.bridge) if args.bridge else REPO_ROOT / "data/tank_id_bridge.json"
    version = decode_dvpl((game_data / "version.txt.dvpl").read_bytes()).decode("utf-8").strip()
    generated_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    names_by_lang = {lang: Names(game_data, lang=lang) for lang in langs}
    en_names = names_by_lang[langs[0]]
    tables_by_lang = {}
    for lang in langs:
        import yaml as _y
        tables_by_lang[lang] = _y.safe_load(decode_dvpl(
            (game_data / f"Strings/{lang}.yaml.dvpl").read_bytes()).decode("utf-8")) or {}
        # 基础 Strings 之后补运行时覆盖层（只补缺键，基础表优先）
        for key, value in load_localization_overlay(lang).items():
            tables_by_lang[lang].setdefault(key, value)

    # ---------- 逐车模块 ----------
    # 模型碰撞盒 / 炮塔挂点：复用提取链的 Extractor（与 models.pb 同源同语义）。
    ex = Extractor(game_data)
    bridge = json.loads(bridge_path.read_text(encoding="utf-8"))
    by_key = {(v["nation"], v["stem"]): int(tid) for tid, v in bridge.items()}
    skipped_not_in_bridge: dict[str, int] = {}
    tank_counts = {}
    total_tanks = 0
    preview_by_nation: dict[str, list[tuple[int, str]]] = {}
    # 预览增量：bridge 只给新增车 → 只遍历这些国别（缺目录/文件的国别不猜、不写空）。
    nations = NATIONS
    if args.modules_only:
        nations = [n for n in NATIONS if any(v["nation"] == n for v in bridge.values())]
    for nation in nations:
        comp = Components(game_data / f"XML/item_defs/vehicles/{nation}/components")
        radio_ids, radio_shared = load_component(game_data / f"XML/item_defs/vehicles/{nation}/components", "radios")
        fuel_ids, fuel_shared = load_component(game_data / f"XML/item_defs/vehicles/{nation}/components", "fuelTanks")
        list_root = load_dvpl_xml(game_data / f"XML/item_defs/vehicles/{nation}/list.xml.dvpl")
        existing_meta: dict = {}
        if args.merge:
            mp = out_root / "modules" / f"{nation}.json"
            if mp.exists():
                existing = json.loads(mp.read_text(encoding="utf-8"))
                existing_meta = existing.get("meta") or {}
                tanks = dict(existing.get("tanks") or {})
            else:
                tanks = {}
        else:
            tanks = {}
        exported_here = 0
        skipped = 0
        for v in list_root:
            stem = v.tag
            tid = by_key.get((nation, stem))
            if tid is None:
                skipped += 1
                continue
            p = game_data / f"XML/item_defs/vehicles/{nation}/{stem}.xml.dvpl"
            if not p.exists():
                skipped += 1
                continue
            root = load_dvpl_xml(p)
            hull = root.find("hull")
            list_el = v
            name_key = ftext(list_el, "userString")
            tank = {
                "tankId": tid,
                "name": en_names.display(nation, name_key) or stem,
                "tier": fint(list_el, "level") or 0,
                "tags": (ftext(list_el, "tags") or "").split(),
                "consumableSlots": fint(root, "consumableSlots"),
                "provisionSlots": fint(root, "provisionSlots"),
                "optDevicePreset": ftext(root, "optDevicePreset"),
                "speedLimits": {
                    "forward": ffloat(root.find("speedLimits"), "forward"),
                    "backward": ffloat(root.find("speedLimits"), "backward"),
                },
                "invisibility": {
                    "still": ffloat(root.find("invisibility"), "still"),
                    "moving": ffloat(root.find("invisibility"), "moving"),
                    "firePenalty": ffloat(root.find("invisibility"), "firePenalty"),
                },
                "hull": {
                    "hp": fint(hull, "maxHealth") if hull is not None else None,
                    "weight": ffloat(hull, "weight") if hull is not None else None,
                },
                # 是否有可旋转炮塔：客户端 `<hull><fakeTurrets>` 元素**存在** ⇔ 无炮塔
                # （无炮塔车的“炮塔”是随火炮转动的固定战斗室，游戏把这类车标为
                # hasTurret=false，并用 gun <rotationSpeed> 作为转向速度）。
                # 738/738 与 BI 11.21 hasTurret 逐辆一致；缺 <hull> 时留 null（不猜有/无）。
                "hasTurret": (hull.find("fakeTurrets") is None) if hull is not None else None,
                # <crew> 角色表（子标签名；子标签有文本 = 兼职，如 commander 兼 gunner）。
                # 乘员角色加成（BK：有该角色 ×1.1，无 ×1.05）的判定输入。
                "crewRoles": [c.tag for c in (root.find("crew") or []) if isinstance(c.tag, str)],
                "chassis": export_module_kind(root, "chassis", comp, names_by_lang, tables_by_lang, nation),
                "turrets": export_module_kind(root, "turrets", comp, names_by_lang, tables_by_lang, nation),
                "engines": export_module_kind(root, "engines", comp, names_by_lang, tables_by_lang, nation),
                "radios": export_module_kind(root, "radios", comp, names_by_lang, tables_by_lang, nation,
                                             ids_map=radio_ids, shared_map=radio_shared),
                "fuelTanks": export_module_kind(root, "fuelTanks", comp, names_by_lang, tables_by_lang, nation,
                                                ids_map=fuel_ids, shared_map=fuel_shared),
            }
            if tank["radios"] is not None:
                for m in tank["radios"]:
                    sh = radio_shared.get(m["stem"])
                    if sh is not None:
                        m["stats"]["distance"] = m["stats"]["distance"] if m["stats"]["distance"] is not None else ffloat(sh, "distance")
                        m["stats"]["weight"] = m["stats"]["weight"] if m["stats"]["weight"] is not None else ffloat(sh, "weight")
            if tank["fuelTanks"] is not None:
                for m in tank["fuelTanks"]:
                    sh = fuel_shared.get(m["stem"])
                    if sh is not None and m["stats"]["weight"] is None:
                        m["stats"]["weight"] = ffloat(sh, "weight")
            # 车辆模型碰撞盒（尺寸参数）：车体/底盘盒在车辆级，炮塔盒挂在逐炮塔上
            # （随所选炮塔变）。turretOrigin 是 XML <turretPositions> 的 [x, 上, 前]，
            # 与包围盒坐标 [x, 前, 上] 的换算在前端 compute 中完成（见 §5 公式表）。
            try:
                model = ex.extract_model(nation, stem)
            except Exception as exc:  # noqa: BLE001 - 单辆车缺模型不应中断整国导出
                model = None
                print(f"   [bbox] {nation}/{stem} 跳过：{type(exc).__name__}: {exc}")
            if model:
                tank["turretOrigin"] = model.get("turret_origin")
                # 炮塔初始姿态（仅 4 辆意系 AT-SPG 非零）：pitch 计入有效俯仰上下限
                # （BI 11.21 同口径：Minotauro 18/7 + pitch 3 → 上 15 / 下 10）。
                tank["initialTurretRotation"] = model.get("initial_turret_rotation")
                tank["bbox"] = {
                    "hull": model.get("hull_bbox"),
                    "chassis": model.get("chassis_bbox"),
                }
                src_turrets = model.get("turrets") or []
                if len(src_turrets) == len(tank["turrets"]):
                    for dst, src in zip(tank["turrets"], src_turrets):
                        dst["stats"]["bbox"] = src.get("bbox")
                else:
                    # 顺序无法对号 = 不着色/不给错盒（fail-closed：尺寸整组隐藏）
                    print(f"   [bbox] {nation}/{stem} 炮塔数不匹配（{len(src_turrets)} vs {len(tank['turrets'])}），尺寸留空")
            if args.modules_only:
                # 预览增量数据来源逐车留痕：模块文件的 meta.gameVersion 仍描述主数据集。
                tank["gameVersion"] = version
            tanks[stem] = tank
            exported_here += 1
        skipped_not_in_bridge[nation] = skipped
        tank_counts[nation] = len(tanks)
        total_tanks += len(tanks)
        preview_ids = sorted(
            v["tankId"] for v in tanks.values() if v.get("gameVersion"))
        preview_entries = sorted(
            (v["tankId"], v["gameVersion"]) for v in tanks.values()
            if v.get("gameVersion"))
        if preview_entries:
            preview_by_nation[nation] = preview_entries
        if args.modules_only and args.merge and exported_here == 0:
            continue
        file_meta = {"nation": nation, "gameVersion": version, "count": len(tanks)}
        if args.merge and existing_meta:
            file_meta["gameVersion"] = existing_meta.get("gameVersion") or version
            versions = sorted({v["gameVersion"] for v in tanks.values() if v.get("gameVersion")})
            if versions:
                file_meta["preview"] = {"gameVersions": versions, "tanks": preview_ids}
        payload = {"meta": file_meta, "tanks": tanks}
        (out_root / "modules").mkdir(parents=True, exist_ok=True)
        (out_root / "modules" / f"{nation}.json").write_text(
            json.dumps(payload, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")

    if args.modules_only:
        # 预览增量只改 modules/*.json；meta.json 保留主数据集版本，追加预览车清单，
        # 并重算全量文件 sha256（资产包 manifest 的逐文件校验会读它）。
        meta_path = out_root / "meta.json"
        if args.merge and meta_path.exists():
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
            meta["previewGeneratedAt"] = generated_at
        else:
            meta = {"gameVersion": version, "generatedAt": generated_at, "langs": langs}
        counts = meta.setdefault("counts", {})
        by_nation = dict(counts.get("tanksByNation") or {})
        by_nation.update(tank_counts)
        counts["tanks"] = sum(by_nation.values())
        counts["tanksByNation"] = by_nation
        preview = []
        for nation, entries in preview_by_nation.items():
            for tid, gv in entries:
                preview.append({"tankId": tid, "nation": nation, "gameVersion": gv})
        meta["preview"] = sorted(preview, key=lambda x: x["tankId"])
        files = {}
        for p in sorted(out_root.rglob("*")):
            if not p.is_file() or p.name == "meta.json":
                continue
            rel = p.relative_to(out_root).as_posix()
            files[rel] = {"sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
                          "bytes": p.stat().st_size}
        meta["files"] = files
        meta_path.write_text(json.dumps(meta, ensure_ascii=False, separators=(",", ":")),
                             encoding="utf-8")
        print(f"game={version} modules-only preview tanks={len(preview)}")
        for nation, entries in preview_by_nation.items():
            print(f"  {nation}: {[tid for tid, _ in entries]}")
        print(f"out={out_root}")
        return

    # ---------- 装备 / 物品 / 默认表 / 参数 ----------
    devices = export_devices(game_data, names_by_lang, tables_by_lang)
    consumables = export_items(game_data, "consumables", tables_by_lang)
    provisions = export_items(game_data, "provisions", tables_by_lang)

    import yaml as _yaml
    defaults_raw = _yaml.safe_load(decode_dvpl(
        (game_data / "XML/item_defs/vehicles/common/consumablesDefault.yaml.dvpl").read_bytes()).decode("utf-8"))
    by_tier, overrides = {}, {}
    for k, v in (defaults_raw or {}).items():
        if isinstance(k, int) or (isinstance(k, str) and k.isdigit()):
            by_tier[int(k)] = v
        else:
            overrides[str(k)] = v

    structure, fmt = export_params(game_data)
    param_keys = sorted(fmt.keys())

    strings = export_strings(game_data, langs, param_keys)
    camouflage = export_camouflage(game_data)
    icons = export_icons(game_data, out_root, devices, consumables, provisions)
    skill_icons = export_skills(game_data, out_root, tables_by_lang)
    shell_icons = export_shell_icons(game_data, out_root)
    icons["skills"] = skill_icons["skills"]
    icons["shells"] = shell_icons["shells"]
    icons["missing"].extend(skill_icons["missing"])
    icons["missing"].extend(shell_icons["missing"])

    # ---------- 可选：研发图 / 科技树 ----------
    files: dict[str, dict] = {}

    def write(rel: str, obj) -> None:
        p = out_root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(obj, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")

    if args.with_research_graph:
        raise RuntimeError("--with-research-graph 尚未接常驻输出（本期消费面不用）；如需科技树数据先跑 tmp_analysis/scan_research.py 原型")

    write("devices.json", {"meta": {"gameVersion": version, "count": len(devices["devices"]),
                                    "presets": len(devices["presets"])}, **devices})
    write("consumables.json", {"meta": {"gameVersion": version, "count": len(consumables)}, "items": consumables})
    write("provisions.json", {"meta": {"gameVersion": version, "count": len(provisions)}, "items": provisions})
    write("consumables_default.json", {"meta": {"gameVersion": version},
                                       "byTier": by_tier, "overrides": overrides})
    write("params/structure.json", {"meta": {"gameVersion": version}, **structure})
    write("params/format.json", {"meta": {"gameVersion": version}, "params": fmt})
    for lang in langs:
        write(f"strings/{lang}/research.json", {"meta": {"gameVersion": version, "lang": lang},
                                                "entries": strings[lang]})
    write("camouflage.json", {"meta": {"gameVersion": version}, **camouflage})
    write("skills.json", {"meta": {"gameVersion": version, "count": len(skill_icons["skills"])},
                          "classes": skill_icons["classes"], "items": skill_icons["skills"]})
    write("icons/manifest.json", {"meta": {"gameVersion": version}, **icons})

    # 逐文件 sha256：覆盖全部产出的数据与图标（meta.json 自身除外）
    for p in sorted(out_root.rglob("*")):
        if not p.is_file() or p.name == "meta.json":
            continue
        rel = p.relative_to(out_root).as_posix()
        files[rel] = {"sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "bytes": p.stat().st_size}

    write("meta.json", {
        "gameVersion": version,
        "generatedAt": generated_at,
        "langs": langs,
        "counts": {
            "tanks": total_tanks,
            "tanksByNation": tank_counts,
            "devices": len(devices["devices"]),
            "devicePresets": len(devices["presets"]),
            "consumables": len(consumables),
            "provisions": len(provisions),
            "consumablesCommon": sum(1 for x in consumables if x["mode"] == "common"),
            "provisionsCommon": sum(1 for x in provisions if x["mode"] == "common"),
            "skills": len(skill_icons["skills"]),
            "shellIcons": len(shell_icons["shells"]),
        },
        "files": files,
    })
    print(f"game={version}")
    print(f"tanks={total_tanks} byNation={tank_counts}")
    print(f"devices={len(devices['devices'])} presets={len(devices['presets'])}")
    print(f"consumables={len(consumables)} (common {sum(1 for x in consumables if x['mode'] == 'common')})")
    print(f"provisions={len(provisions)} (common {sum(1 for x in provisions if x['mode'] == 'common')})")
    print(f"params={len(fmt)} strings({','.join(langs)}) icons={sum(len(v) for v in icons['items'].values())}+{len(icons['modules'])}")
    print(f"out={out_root}")


if __name__ == "__main__":
    main()
