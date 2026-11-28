#!/usr/bin/env python3
"""版本变更语义差异（dataset release → dataset release）。

输入两个 asset pack（`release/<release>` 或 `release/datasets/<id>`），输出给
WotbTools 版本变更页的静态 diff 工件：

  <out>/<fromDataset>__<toDataset>/<fromRelease>__<toRelease>/
    meta.json
    summary.json
    tanks/<tankId>.json
    maps/<mapId>.json
    items.json
  <out>/index.json

设计约束（与 AGENTS.md 一致）：
- 语义主键匹配，不按文件哈希/数组下标判定；内部标识（shell_global_ids、localId、
  stem、gameVersion）不参与变化计数。
- 单侧出现的字段归入 schema 桶，不逐车展开（例如 11.22 的 mechanics / clip）。
- 派生资源（map2d/cover/ground.webp、2x tank_images）进 presentation 桶，
  不作为玩法变更。
- 配对歧义 fail-closed：`--strict` 下任何 unpaired / unresolved 直接失败。

用法：
  python tools/version_diff.py --base release/datasets/11.21.0_1 \
      --target release/datasets/11.22.0 --out release/diffs --strict
  python tools/version_diff.py --lineage data/version_lineage.json \
      --datasets-root release/datasets --out release/diffs --all --strict
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import pathlib
import re
import sys
from collections import Counter, defaultdict
from typing import Any

SCHEMA = 1
TOOL_VERSION = "0.1.0"
TOL = 1e-6


class DiffError(RuntimeError):
    pass


# ---------------------------------------------------------------------------
# 基础工具
# ---------------------------------------------------------------------------
def load_json(path: pathlib.Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise DiffError(f"缺少文件: {path}") from exc
    except json.JSONDecodeError as exc:
        raise DiffError(f"JSON 解析失败: {path}: {exc}") from exc


def write_json(path: pathlib.Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + f".tmp-{os.getpid()}")
    tmp.write_text(json.dumps(data, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    os.replace(tmp, path)


def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def is_num(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def num_eq(a: Any, b: Any) -> bool:
    return math.isclose(float(a), float(b), rel_tol=TOL, abs_tol=1e-9)


def same_value(a: Any, b: Any) -> bool:
    """结构化相等；浮点带 1e-6 容差。dict 键集必须一致，list 顺序敏感。"""
    if is_num(a) and is_num(b):
        return num_eq(a, b)
    if isinstance(a, dict) and isinstance(b, dict):
        if set(a) != set(b):
            return False
        return all(same_value(a[key], b[key]) for key in a)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(same_value(x, y) for x, y in zip(a, b))
    return a == b


def add_change(changes: list[dict], path: str, old: Any, new: Any, category: str,
               note: str | None = None) -> None:
    item = {"path": path, "category": category, "old": old, "new": new}
    if note:
        item["note"] = note
    changes.append(item)


def schema_bucket() -> dict[str, Counter]:
    return {"added": Counter(), "removed": Counter()}


def compare_into(old: Any, new: Any, path: str, category: str,
                 changes: list[dict], schema: dict[str, Counter],
                 schema_path: str | None = None) -> None:
    """递归比较；单侧出现的键只进 schema，不产生逐项 change。

    schema_path 用于把逐模块路径（`gun:X/Y:10.stats.clip`）汇总成字段路径
    （`stats.clip`），避免 260 个模块各占一个 schema 键。
    """
    schema_path = schema_path or path
    if same_value(old, new):
        return
    if isinstance(old, dict) and isinstance(new, dict):
        for key in sorted(set(old) | set(new), key=str):
            child = f"{path}.{key}" if path else str(key)
            child_schema = f"{schema_path}.{key}" if schema_path else str(key)
            if key not in old:
                schema["added"][child_schema] += 1
            elif key not in new:
                schema["removed"][child_schema] += 1
            else:
                compare_into(old[key], new[key], child, category, changes, schema, child_schema)
        return
    add_change(changes, path, old, new, category)


def schema_to_json(schema: dict[str, Counter]) -> dict:
    return {
        "added": dict(sorted(schema["added"].items())),
        "removed": dict(sorted(schema["removed"].items())),
    }


def merge_schema(dst: dict[str, Counter], src: dict[str, Counter]) -> None:
    for kind in ("added", "removed"):
        for key, count in src[kind].items():
            dst[kind][key] += count


# ---------------------------------------------------------------------------
# pack 读取
# ---------------------------------------------------------------------------
def dataset_identity(pack: pathlib.Path) -> dict:
    version = load_json(pack / "version.json")
    manifest = load_json(pack / "manifest.json")
    identity = {
        "dataset": version.get("dataset_id"),
        "label": version.get("label"),
        "release": version.get("release"),
        "gameVersion": version.get("game_version"),
        "variant": version.get("variant"),
        "tankCount": version.get("tank_count"),
        "contentDigest": version.get("content_digest"),
        "manifestSha256": version.get("manifest_sha256"),
        "manifestGenerated": manifest.get("generated"),
        "upstreamCommit": manifest.get("upstream_commit"),
        "worktreeDirty": manifest.get("worktree_dirty"),
    }
    if not identity["dataset"] or not identity["release"]:
        raise DiffError(f"{pack}: version.json 缺 dataset_id/release")
    return identity


def manifest_index(pack: pathlib.Path) -> dict[str, dict]:
    path = pack / "manifest.json"
    if not path.is_file():
        return {}
    return {entry["path"]: entry for entry in load_json(path).get("files", [])}


def file_sha(pack: pathlib.Path, manifest: dict[str, dict], rel: str) -> str | None:
    entry = manifest.get(rel)
    if entry:
        return entry.get("sha256")
    path = pack / rel
    return sha256_file(path) if path.is_file() else None


def tank_cache(pack: pathlib.Path) -> dict[str, dict]:
    data = load_json(pack / "data" / "tank_cache.json")
    if not isinstance(data, dict):
        raise DiffError(f"{pack}: tank_cache.json 不是对象")
    return data


def map_index(pack: pathlib.Path) -> dict[str, dict]:
    data = load_json(pack / "index.json")
    maps = data.get("maps")
    if not isinstance(maps, dict):
        raise DiffError(f"{pack}: index.json 缺 maps")
    return maps


# ---------------------------------------------------------------------------
# 坦克概要（tank_cache）
# ---------------------------------------------------------------------------
SUMMARY_FIELDS: tuple[tuple[str, str], ...] = (
    ("hp", "survivability"),
    ("speed_forward", "mobility"),
    ("speed_reverse", "mobility"),
    ("hull_traverse", "mobility"),
    ("turret_traverse_speed", "firepower"),
    ("view_range", "firepower"),
    ("gun_depression", "firepower"),
    ("gun_elevation", "firepower"),
    ("is_premium", "identity"),
    ("is_collector", "identity"),
    ("tier", "identity"),
    ("type", "identity"),
    ("nation", "identity"),
    ("name", "identity"),
)

SUMMARY_SHELL_FIELDS = (
    "penetration", "penetration_top", "damage", "damage_top",
    "module_damage", "explosion_radius", "caliber", "normalization", "ricochet",
)


def paired_by_key(items: list[dict], key_fn) -> tuple[dict, list]:
    """按语义键配对；重复键不配对（fail-closed），返回 (pairs, duplicates)。"""
    buckets: dict[Any, list[dict]] = defaultdict(list)
    for item in items or []:
        buckets[key_fn(item)].append(item)
    pairs = {key: values[0] for key, values in buckets.items() if len(values) == 1}
    duplicates = sorted(key for key, values in buckets.items() if len(values) > 1)
    return pairs, duplicates


def diff_summary(base: dict, target: dict) -> tuple[list[dict], dict[str, Counter], list[str]]:
    changes: list[dict] = []
    schema = schema_bucket()
    unresolved: list[str] = []
    for field, category in SUMMARY_FIELDS:
        old = base.get(field)
        new = target.get(field)
        if old is None and new is None:
            continue
        if old is None or new is None:
            schema["removed" if new is None else "added"][field] += 1
            continue
        if not same_value(old, new):
            add_change(changes, field, old, new, category)

    for module in ("hull", "turret"):
        old_mod = (base.get("armor") or {}).get(module) or {}
        new_mod = (target.get("armor") or {}).get(module) or {}
        for face in ("front", "sides", "rear"):
            path = f"armor.{module}.{face}"
            old = old_mod.get(face)
            new = new_mod.get(face)
            if old is None and new is None:
                continue
            if old is None or new is None:
                schema["removed" if new is None else "added"][path] += 1
                continue
            if not same_value(old, new):
                add_change(changes, path, old, new, "armor")

    old_shells, old_dup = paired_by_key(base.get("shells") or [], lambda s: s.get("shell_type"))
    new_shells, new_dup = paired_by_key(target.get("shells") or [], lambda s: s.get("shell_type"))
    if old_dup or new_dup:
        unresolved.extend(f"shells.duplicate:{x}" for x in sorted(set(old_dup) | set(new_dup)))
    for shell_type in sorted(set(old_shells) | set(new_shells), key=str):
        path = f"shells.{shell_type}"
        old = old_shells.get(shell_type)
        new = new_shells.get(shell_type)
        if old is None or new is None:
            schema["removed" if new is None else "added"][f"shells.{shell_type}"] += 1
            continue
        for field in SUMMARY_SHELL_FIELDS:
            old_value = old.get(field)
            new_value = new.get(field)
            if old_value is None and new_value is None:
                continue
            if old_value is None or new_value is None:
                schema["removed" if new_value is None else "added"][f"{path}.{field}"] += 1
                continue
            if not same_value(old_value, new_value):
                add_change(changes, f"{path}.{field}", old_value, new_value, "firepower")
    return changes, schema, unresolved


# ---------------------------------------------------------------------------
# 装甲
# ---------------------------------------------------------------------------
def armor_section(module: dict, plate: str) -> str | None:
    primary = module.get("primary") or {}
    names = sorted(side for side, value in primary.items() if value == f"armor_{plate}")
    return "/".join(names) if names else None


def normalize_list(value: Any) -> list:
    if value is None:
        return []
    if isinstance(value, list):
        return sorted(str(item) for item in value)
    return [str(value)]


def diff_armor_model(old: dict | None, new: dict | None) -> dict:
    old = old or {}
    new = new or {}
    out = {
        "summary": {},
        "plates": [],
        "addedPlates": [],
        "removedPlates": [],
        "primary": [],
        "spaced": [],
        "chassis": [],
        "changed": False,
    }
    for module in ("hull", "turret", "gun"):
        old_mod = old.get(module) or {}
        new_mod = new.get(module) or {}
        old_plates = old_mod.get("plates") or {}
        new_plates = new_mod.get("plates") or {}
        for plate in sorted(set(old_plates) | set(new_plates), key=lambda x: (len(str(x)), str(x))):
            old_value = old_plates.get(plate)
            new_value = new_plates.get(plate)
            section = armor_section(new_mod if plate in new_plates else old_mod, plate)
            item = {"module": module, "plate": str(plate), "section": section}
            if old_value is None:
                item["thickness"] = new_value
                out["addedPlates"].append(item)
            elif new_value is None:
                item["thickness"] = old_value
                out["removedPlates"].append(item)
            elif not num_eq(old_value, new_value):
                item.update({"old": old_value, "new": new_value})
                out["plates"].append(item)
        old_primary = old_mod.get("primary") or {}
        new_primary = new_mod.get("primary") or {}
        for side in sorted(set(old_primary) | set(new_primary)):
            old_value = old_primary.get(side)
            new_value = new_primary.get(side)
            if old_value != new_value:
                out["primary"].append({"module": module, "side": side, "old": old_value, "new": new_value})
        old_spaced = normalize_list(old_mod.get("spaced"))
        new_spaced = normalize_list(new_mod.get("spaced"))
        if old_spaced != new_spaced:
            out["spaced"].append({"module": module, "old": old_spaced, "new": new_spaced})
    for field in ("left_track", "right_track"):
        old_value = (old.get("chassis") or {}).get(field)
        new_value = (new.get("chassis") or {}).get(field)
        if old_value is None and new_value is None:
            continue
        if old_value is None or new_value is None or not num_eq(old_value, new_value):
            out["chassis"].append({"field": field, "old": old_value, "new": new_value})
    out["changed"] = any(out[key] for key in ("plates", "addedPlates", "removedPlates", "primary", "spaced", "chassis"))
    return out


def diff_tank_armor_summary(old: dict | None, new: dict | None) -> dict[str, dict]:
    """方向装甲表：hull/turret × front/sides/rear，只返回变化项。"""
    out: dict[str, dict] = {}
    old = old or {}
    new = new or {}
    for module in ("hull", "turret"):
        for face in ("front", "sides", "rear"):
            old_value = (old.get(module) or {}).get(face)
            new_value = (new.get(module) or {}).get(face)
            if old_value is None and new_value is None:
                continue
            if not same_value(old_value, new_value):
                out[f"{module}.{face}"] = {"old": old_value, "new": new_value}
    return out


# ---------------------------------------------------------------------------
# 配置
# ---------------------------------------------------------------------------
CONFIG_FIELDS: dict[str, str] = {
    "reload_time": "firepower",
    "aim_time": "firepower",
    "dispersion": "firepower",
    "dpm": "firepower",
    "standard_damage": "firepower",
    "caliber": "firepower",
    "gun_thickness": "armor",
    "turret_traverse_speed": "firepower",
    "weight": "mobility",
    "hull_hp": "survivability",
    "turret_health": "survivability",
    "turret_weight": "survivability",
    "is_drum": "firepower",
    "is_burst": "firepower",
    "burst_group_count": "firepower",
    "burst_group_interval": "firepower",
    "burst_interval": "firepower",
    "burst_size": "firepower",
    "burst_reloads": "firepower",
    "pitch_limits": "firepower",
    "yaw_limits": "firepower",
    "tracks": "mobility",
    "clip": "firepower",
}

CONFIG_IGNORED = {
    "id", "shell_global_ids", "gun_index", "turret_index", "turret_local", "gun_local",
    "model_gun_count", "model_turret_count", "gun_mask", "gun_origin", "turret_name", "label",
}

CONFIG_ARMOR_FIELDS = {
    "gun_plates": "gun_plates",
    "turret_plates": "turret_plates",
    "gun_spaced": "gun_spaced",
    "turret_spaced": "turret_spaced",
}

CONFIG_SHELL_FIELDS = (
    "penetration", "penetration_far", "damage", "velocity",
    "module_damage", "explosion_radius", "normalization", "ricochet", "caliber", "range",
)


def config_key(config: dict) -> tuple:
    """配置身份：优先客户端 turret/gun localId（同一车内稳定），缺失时退回名字+口径。

    `turret_name` 在 dump-tank-data 产物里通常为空，不能作为主键；WarDuck 这类
    同炮名多配置也只有 localId 能区分。跨版本 localId 若漂移，配对会失败并进
    unresolved，而不是按数组下标猜。
    """
    turret_local = config.get("turret_local")
    gun_local = config.get("gun_local")
    if turret_local is not None and gun_local is not None:
        return ("local", str(turret_local), str(gun_local))
    turret = str(config.get("turret_name") or "").strip()
    gun = str(config.get("label") or "").strip()
    caliber = config.get("caliber")
    return ("names", turret, gun, round(float(caliber), 6) if is_num(caliber) else None)


def config_descriptor(config: dict) -> dict:
    return {
        "turret": str(config.get("turret_name") or ""),
        "turretLocal": config.get("turret_local"),
        "gun": str(config.get("label") or ""),
        "gunLocal": config.get("gun_local"),
        "caliber": config.get("caliber"),
    }


def diff_configs(old_configs: list[dict] | None, new_configs: list[dict] | None) -> dict:
    old_all = {config_key(config): config for config in old_configs or []}
    new_all = {config_key(config): config for config in new_configs or []}
    old_map, old_dup = paired_by_key(old_configs or [], config_key)
    new_map, new_dup = paired_by_key(new_configs or [], config_key)
    out = {
        "changed": False,
        "configs": [],
        "unpairedOld": [config_descriptor(old_all[key]) for key in old_dup],
        "unpairedNew": [config_descriptor(new_all[key]) for key in new_dup],
        "added": [],
        "removed": [],
    }
    aggregate_schema = schema_bucket()
    for key in sorted(set(old_map) | set(new_map), key=str):
        old = old_map.get(key)
        new = new_map.get(key)
        if old is None or new is None:
            entry = config_descriptor(new if old is None else old)
            out["added" if old is None else "removed"].append(entry)
            out["changed"] = True
            continue
        changes: list[dict] = []
        schema = schema_bucket()
        for field, category in CONFIG_FIELDS.items():
            old_value = old.get(field)
            new_value = new.get(field)
            if old_value is None and new_value is None:
                continue
            if old_value is None or new_value is None:
                schema["removed" if new_value is None else "added"][field] += 1
                continue
            if not same_value(old_value, new_value):
                add_change(changes, field, old_value, new_value, category)
        for field in CONFIG_ARMOR_FIELDS:
            # 配置级装甲板：进 armor 分类，但装甲小窗口仍读顶层 armor_model。
            if field in ("gun_spaced", "turret_spaced"):
                old_value = normalize_list(old.get(field))
                new_value = normalize_list(new.get(field))
            else:
                old_value = old.get(field)
                new_value = new.get(field)
            if old_value is None and new_value is None:
                continue
            if old_value is None or new_value is None:
                schema["removed" if new_value is None else "added"][field] += 1
                continue
            if not same_value(old_value, new_value):
                add_change(changes, field, old_value, new_value, "armor")
        # engines: 按显示名配对；功率/起火率变化算 mobility。
        old_engines, old_engine_dup = paired_by_key(old.get("engines") or [], lambda e: e.get("name"))
        new_engines, new_engine_dup = paired_by_key(new.get("engines") or [], lambda e: e.get("name"))
        if old_engine_dup or new_engine_dup:
            out.setdefault("unresolved", []).extend(
                f"engines.duplicate:{name}" for name in sorted(set(old_engine_dup) | set(new_engine_dup))
            )
        for name in sorted(set(old_engines) | set(new_engines), key=str):
            old_engine = old_engines.get(name)
            new_engine = new_engines.get(name)
            if old_engine is None or new_engine is None:
                changes.append({
                    "path": f"engines.{name}",
                    "category": "mobility",
                    "old": old_engine,
                    "new": new_engine,
                })
                continue
            for field in ("power", "fire_chance"):
                old_value = old_engine.get(field)
                new_value = new_engine.get(field)
                if old_value is None and new_value is None:
                    continue
                if old_value is None or new_value is None:
                    schema["removed" if new_value is None else "added"][f"engines.{field}"] += 1
                elif not same_value(old_value, new_value):
                    add_change(changes, f"engines.{name}.{field}", old_value, new_value, "mobility")
        # shells: 按 type 配对；同 type 重复时用 type_id 细分。
        old_shells, old_shell_dup = paired_by_key(old.get("shells") or [], lambda s: s.get("type"))
        new_shells, new_shell_dup = paired_by_key(new.get("shells") or [], lambda s: s.get("type"))
        if old_shell_dup or new_shell_dup:
            out.setdefault("unresolved", []).extend(
                f"configs.shells.duplicate:{shell}" for shell in sorted(set(old_shell_dup) | set(new_shell_dup))
            )
        for shell_type in sorted(set(old_shells) | set(new_shells), key=str):
            old_shell = old_shells.get(shell_type)
            new_shell = new_shells.get(shell_type)
            if old_shell is None or new_shell is None:
                changes.append({
                    "path": f"shells.{shell_type}",
                    "category": "firepower",
                    "old": old_shell,
                    "new": new_shell,
                })
                continue
            for field in CONFIG_SHELL_FIELDS:
                old_value = old_shell.get(field)
                new_value = new_shell.get(field)
                if old_value is None and new_value is None:
                    continue
                if old_value is None or new_value is None:
                    schema["removed" if new_value is None else "added"][f"shells.{shell_type}.{field}"] += 1
                elif not same_value(old_value, new_value):
                    add_change(changes, f"shells.{shell_type}.{field}", old_value, new_value, "firepower")
        # 未识别字段：双方都有且变化 → other；单侧 → schema。
        for field in sorted(set(old) | set(new)):
            if field in CONFIG_FIELDS or field in CONFIG_ARMOR_FIELDS or field in CONFIG_IGNORED:
                continue
            if field in ("engines", "shells"):
                continue
            old_value = old.get(field)
            new_value = new.get(field)
            if field not in old:
                schema["added"][field] += 1
            elif field not in new:
                schema["removed"][field] += 1
            else:
                compare_into(old_value, new_value, field, "other", changes, schema)
        if changes or out.get("unresolved"):
            out["changed"] = True
        out["configs"].append({
            "key": config_descriptor(new),
            "changes": changes,
            "schema": schema_to_json(schema),
        })
    for entry in out["configs"]:
        for kind in ("added", "removed"):
            for field_name, count in (entry.get("schema") or {}).get(kind, {}).items():
                aggregate_schema[kind][field_name] += count
    out["schema"] = schema_to_json(aggregate_schema)
    if old_dup or new_dup or out["added"] or out["removed"]:
        out["changed"] = True
    return out


# ---------------------------------------------------------------------------
# 研发模块
# ---------------------------------------------------------------------------
RESEARCH_TANK_FIELDS = (
    "tags", "consumableSlots", "provisionSlots", "crewRoles", "hull", "invisibility",
    "speedLimits", "hasTurret", "optDevicePreset", "vehicleAbilities", "bbox", "turretOrigin",
)


def research_modules(pack: pathlib.Path) -> dict[str, dict]:
    out: dict[str, dict] = {}
    modules_dir = pack / "research" / "modules"
    if not modules_dir.is_dir():
        return out
    for path in sorted(modules_dir.glob("*.json")):
        doc = load_json(path)
        for name, entry in (doc.get("tanks") or {}).items():
            tank_id = entry.get("tankId")
            key = str(tank_id) if tank_id is not None else str(name)
            if key in out:
                raise DiffError(f"{pack}: research/modules 中 tankId 重复: {key}")
            out[key] = entry
    return out


def vehicle_stem_index(pack: pathlib.Path) -> dict[str, dict]:
    out: dict[str, dict] = {}
    tree_dir = pack / "research" / "tech_tree"
    if not tree_dir.is_dir():
        return out
    for path in sorted(tree_dir.glob("*.json")):
        doc = load_json(path)
        for node in doc.get("nodes") or []:
            stem = node.get("stem")
            if stem:
                out[str(stem)] = {"tankId": node.get("tankId"), "name": stem}
    return out


def module_flat(entry: dict) -> tuple[dict, list]:
    flat: dict[tuple, dict] = {}
    duplicates: list[tuple] = []
    buckets: dict[tuple, list[dict]] = defaultdict(list)
    for category in ("chassis", "engines"):
        for module in entry.get(category) or []:
            buckets[("module", category, module.get("name_en"), module.get("level"))].append(module)
    for turret in entry.get("turrets") or []:
        buckets[("turret", None, turret.get("name_en"), turret.get("level"))].append(turret)
        for gun in turret.get("guns") or []:
            buckets[("gun", turret.get("name_en"), gun.get("name_en"), gun.get("level"))].append(gun)
    for key, values in buckets.items():
        if len(values) > 1:
            duplicates.append(key)
        else:
            flat[key] = values[0]
    return flat, duplicates


def resolve_unlocks(unlocks: list[dict] | None, module_index: dict, vehicle_index: dict) -> tuple[list, list]:
    resolved: list[list] = []
    unresolved: list[str] = []
    for unlock in unlocks or []:
        kind = unlock.get("kind")
        stem = unlock.get("stem")
        cost = unlock.get("cost")
        target = None
        if stem in module_index:
            target = module_index[stem]
        elif stem in vehicle_index:
            target = vehicle_index[stem].get("name") or stem
        else:
            unresolved.append(str(stem))
            target = stem
        resolved.append([kind, target, cost])
    resolved.sort(key=lambda item: (str(item[0]), str(item[1]), str(item[2])))
    return resolved, unresolved


def stem_module_index(entry: dict) -> dict[str, str]:
    """stem → 显示名，用于把 unlock 目标从 stem 归一到名字。"""
    out: dict[str, str] = {}
    for category in ("chassis", "engines"):
        for module in entry.get(category) or []:
            out[str(module.get("stem"))] = module.get("name_en")
    for turret in entry.get("turrets") or []:
        out[str(turret.get("stem"))] = turret.get("name_en")
        for gun in turret.get("guns") or []:
            out[str(gun.get("stem"))] = gun.get("name_en")
    return out


def diff_research(old: dict | None, new: dict | None,
                  old_vehicles: dict, new_vehicles: dict) -> dict:
    out = {
        "changed": False,
        "changes": [],
        "modulesAdded": [],
        "modulesRemoved": [],
        "unpaired": [],
        "unresolved": [],
        "schema": schema_bucket(),
    }
    if old is None and new is None:
        return out
    if old is None or new is None:
        out["changed"] = True
        out["changes"].append({
            "path": "research",
            "category": "research",
            "old": None if old is None else "present",
            "new": None if new is None else "present",
        })
        return out
    changes = out["changes"]
    schema = out["schema"]
    for field in RESEARCH_TANK_FIELDS:
        old_value = old.get(field)
        new_value = new.get(field)
        if old_value is None and new_value is None:
            continue
        if old_value is None or new_value is None:
            schema["removed" if new_value is None else "added"][field] += 1
            continue
        compare_into(old_value, new_value, field, "research", changes, schema)
    old_modules, old_dup = module_flat(old)
    new_modules, new_dup = module_flat(new)
    if old_dup or new_dup:
        out["unpaired"].extend(str(key) for key in sorted(set(old_dup) | set(new_dup)))
    old_stems = stem_module_index(old)
    new_stems = stem_module_index(new)
    for key in sorted(set(old_modules) | set(new_modules), key=str):
        old_module = old_modules.get(key)
        new_module = new_modules.get(key)
        label = f"{key[0]}:{key[1] + '/' if key[1] else ''}{key[2]}:{key[3]}"
        if old_module is None or new_module is None:
            out["modulesAdded" if old_module is None else "modulesRemoved"].append(label)
            out["changed"] = True
            continue
        fields = ["price", "primaryArmor", "stats", "unlocks"]
        for field in fields:
            old_value = old_module.get(field)
            new_value = new_module.get(field)
            if field == "unlocks":
                old_unlocks, old_unresolved = resolve_unlocks(old_value, old_stems, old_vehicles)
                new_unlocks, new_unresolved = resolve_unlocks(new_value, new_stems, new_vehicles)
                out["unresolved"].extend(old_unresolved)
                out["unresolved"].extend(new_unresolved)
                if not same_value(old_unlocks, new_unlocks):
                    add_change(changes, f"{label}.unlocks", old_unlocks, new_unlocks, "research")
                continue
            if old_value is None and new_value is None:
                continue
            if old_value is None or new_value is None:
                schema["removed" if new_value is None else "added"][f"{label}.{field}"] += 1
                continue
            compare_into(old_value, new_value, f"{label}.{field}", "research", changes, schema, schema_path=field)
    out["changed"] = bool(changes or out["modulesAdded"] or out["modulesRemoved"])
    return out


# ---------------------------------------------------------------------------
# 科技树 / 物品
# ---------------------------------------------------------------------------
def tech_tree_docs(pack: pathlib.Path) -> dict[str, dict]:
    out: dict[str, dict] = {}
    tree_dir = pack / "research" / "tech_tree"
    if not tree_dir.is_dir():
        return out
    for path in sorted(tree_dir.glob("*.json")):
        out[path.stem] = load_json(path)
    return out


def all_module_stems(entries: dict[str, dict]) -> dict[str, str]:
    out: dict[str, str] = {}
    for entry in entries.values():
        out.update(stem_module_index(entry))
    return out


def diff_tech_tree(old_docs: dict[str, dict], new_docs: dict[str, dict],
                   old_stems: dict[str, str] | None = None,
                   new_stems: dict[str, str] | None = None) -> dict:
    old_stems = old_stems or {}
    new_stems = new_stems or {}
    out = {
        "addedNodes": [], "removedNodes": [], "changedNodes": [],
        "addedEdges": [], "removedEdges": [], "changedEdges": [],
        "layoutChanged": 0,
    }
    for nation in sorted(set(old_docs) | set(new_docs)):
        old = old_docs.get(nation) or {}
        new = new_docs.get(nation) or {}
        old_nodes = {
            str(node.get("tankId") if node.get("tankId") is not None else node.get("stem")): node
            for node in old.get("nodes") or []
        }
        new_nodes = {
            str(node.get("tankId") if node.get("tankId") is not None else node.get("stem")): node
            for node in new.get("nodes") or []
        }
        for key in sorted(set(old_nodes) | set(new_nodes)):
            old_node = old_nodes.get(key)
            new_node = new_nodes.get(key)
            if old_node is None or new_node is None:
                (out["addedNodes"] if old_node is None else out["removedNodes"]).append(
                    {"nation": nation, "key": key, "node": new_node or old_node}
                )
                continue
            fields = {}
            for field in ("tier", "price", "tags"):
                if not same_value(old_node.get(field), new_node.get(field)):
                    fields[field] = {"old": old_node.get(field), "new": new_node.get(field)}
            if not same_value((old_node.get("x"), old_node.get("y")), (new_node.get("x"), new_node.get("y"))):
                out["layoutChanged"] += 1
            if fields:
                out["changedNodes"].append({"nation": nation, "key": key, "fields": fields})
        old_edges = {(edge.get("from"), edge.get("to")): edge for edge in old.get("edges") or []}
        new_edges = {(edge.get("from"), edge.get("to")): edge for edge in new.get("edges") or []}
        for edge_key in sorted(set(old_edges) | set(new_edges)):
            old_edge = old_edges.get(edge_key)
            new_edge = new_edges.get(edge_key)
            if old_edge is None or new_edge is None:
                (out["addedEdges"] if old_edge is None else out["removedEdges"]).append(
                    {"nation": nation, "from": edge_key[0], "to": edge_key[1]}
                )
                continue
            fields = {}
            for field in ("cost", "sourceKind"):
                if not same_value(old_edge.get(field), new_edge.get(field)):
                    fields[field] = {"old": old_edge.get(field), "new": new_edge.get(field)}
            old_source = old_stems.get(old_edge.get("sourceModule"), old_edge.get("sourceModule"))
            new_source = new_stems.get(new_edge.get("sourceModule"), new_edge.get("sourceModule"))
            if not same_value(old_source, new_source):
                fields["sourceModule"] = {"old": old_source, "new": new_source}
            if fields:
                out["changedEdges"].append({
                    "nation": nation, "from": edge_key[0], "to": edge_key[1], "fields": fields,
                })
    return out


def item_key(item: Any) -> str:
    if isinstance(item, dict):
        for field in ("id", "stem", "name_en"):
            if item.get(field) is not None:
                return str(item[field])
    return json.dumps(item, ensure_ascii=False, sort_keys=True)


ITEM_CONTAINERS = {"items", "devices", "presets", "classes", "categories"}


def diff_doc(old: Any, new: Any) -> dict:
    schema = schema_bucket()
    out = _diff_doc(old, new, "", schema, "")
    out["schema"] = schema_to_json(schema)
    return out


def _merge_nested(out: dict, nested: dict) -> None:
    for kind in ("added", "removed", "changed"):
        out[kind].extend(nested[kind])


def _diff_doc(old: Any, new: Any, prefix: str,
              schema: dict[str, Counter], schema_prefix: str) -> dict:
    """物品文档差异：item 级增删是真实变化；item 内单侧字段进 schema。"""
    out = {"added": [], "removed": [], "changed": []}
    if isinstance(old, dict) and isinstance(new, dict):
        for key in sorted(set(old) | set(new), key=str):
            if key == "meta":
                continue
            path = f"{prefix}.{key}" if prefix else str(key)
            schema_path = f"{schema_prefix}.{key}" if schema_prefix else str(key)
            if key not in old:
                if prefix in ("", *ITEM_CONTAINERS):
                    out["added"].append(path)
                else:
                    schema["added"][schema_path] += 1
            elif key not in new:
                if prefix in ("", *ITEM_CONTAINERS):
                    out["removed"].append(path)
                else:
                    schema["removed"][schema_path] += 1
            elif isinstance(old[key], (dict, list)) and isinstance(new[key], (dict, list)):
                child_schema_prefix = schema_path
                if prefix in ITEM_CONTAINERS and isinstance(old[key], dict) and isinstance(new[key], dict):
                    child_schema_prefix = ""
                _merge_nested(out, _diff_doc(old[key], new[key], path, schema, child_schema_prefix))
            elif not same_value(old[key], new[key]):
                out["changed"].append(path)
        return out
    if isinstance(old, list) and isinstance(new, list):
        # item 容器按 id/stem/name 配对；其他位置的 list（filter/ids 等）整体比较，
        # 避免把 filter 内部 dict 的每个 key 展开成噪声。
        if prefix in ITEM_CONTAINERS and all(isinstance(item, dict) for item in old + new):
            old_map = {item_key(item): item for item in old}
            new_map = {item_key(item): item for item in new}
            for key in sorted(set(old_map) | set(new_map), key=str):
                path = f"{prefix}.{key}" if prefix else str(key)
                old_item = old_map.get(key)
                new_item = new_map.get(key)
                if old_item is None:
                    out["added"].append(path)
                elif new_item is None:
                    out["removed"].append(path)
                else:
                    _merge_nested(out, _diff_doc(old_item, new_item, path, schema, ""))
            return out
        if not same_value(old, new):
            out["changed"].append(prefix or "value")
        return out
    if not same_value(old, new):
        out["changed"].append(prefix or "value")
    return out


def diff_items(old_pack: pathlib.Path, new_pack: pathlib.Path) -> dict:
    names = ("consumables", "provisions", "devices", "skills", "camouflage", "consumables_default")
    out: dict[str, dict] = {}
    for name in names:
        old_path = old_pack / "research" / f"{name}.json"
        new_path = new_pack / "research" / f"{name}.json"
        old = load_json(old_path) if old_path.is_file() else None
        new = load_json(new_path) if new_path.is_file() else None
        if old is None and new is None:
            continue
        out[name] = diff_doc(old, new)
    return out


# ---------------------------------------------------------------------------
# 地图
# ---------------------------------------------------------------------------
MAP_FAMILIES: dict[str, set[str]] = {
    "terrain": {"terrain.json", "terrain.u16.bin"},
    "groundLayers": {"ground.layers.json"},
    "scenery": {"scenery.glb"},
    "grass": {"grass.bin", "grass.glb", "grass.json"},
    "lighting": {"lighting.json", "ibl.webp"},
    "destructibles": {"destructibles.json"},
}

MAP_PRESENTATION = {"ground.webp", "mini.webp"}


def repo_map_spaces() -> dict[int, str]:
    """map_index.json（打包器输入）的 map_id → space 映射；用于把变体并回 base 图。"""
    path = pathlib.Path(__file__).resolve().parents[1] / "map_index.json"
    if not path.is_file():
        return {}
    try:
        data = load_json(path)
    except DiffError:
        return {}
    out: dict[int, str] = {}
    for entry in data if isinstance(data, list) else []:
        try:
            out[int(entry["map_id"])] = str(entry.get("space") or entry.get("key"))
        except (KeyError, TypeError, ValueError):
            continue
    return out


def map_groups(index: dict[str, dict], spaces: dict[int, str] | None = None) -> dict[str, list[tuple[int, dict]]]:
    spaces = spaces or {}
    all_keys = {str(entry.get("key")) for entry in index.values() if entry.get("key")}
    groups: dict[str, list[tuple[int, dict]]] = defaultdict(list)
    for raw_id, entry in index.items():
        try:
            map_id = int(raw_id)
        except (TypeError, ValueError):
            continue
        key = str(entry.get("key") or "")
        base = spaces.get(map_id)
        if not base:
            base = entry.get("space")
        if not base:
            variant_key = str(entry.get("variantKey") or "")
            if variant_key and variant_key != key and variant_key in all_keys:
                base = variant_key
        if not base and key:
            match = re.match(r"^(.*)_\d+$", key)
            if match and match.group(1) in all_keys:
                base = match.group(1)
        if not base:
            base = key or str(entry.get("variantKey") or "")
        groups[str(base)].append((map_id, entry))
    for values in groups.values():
        values.sort(key=lambda item: item[0])
    return groups


def diff_destructibles(old_path: pathlib.Path, new_path: pathlib.Path) -> dict:
    out = {
        "instancesAdded": 0,
        "instancesRemoved": 0,
        "healthChanged": 0,
        "collisionTypeChanged": 0,
        "prototypeChanged": 0,
        "renamed": 0,
        "moved": 0,
        "healthChanges": [],
    }
    if not old_path.is_file() or not new_path.is_file():
        return out
    old = load_json(old_path)
    new = load_json(new_path)
    old_instances = {item.get("id"): item for item in old.get("instances") or []}
    new_instances = {item.get("id"): item for item in new.get("instances") or []}
    out["instancesAdded"] = len(set(new_instances) - set(old_instances))
    out["instancesRemoved"] = len(set(old_instances) - set(new_instances))
    for instance_id in sorted(set(old_instances) & set(new_instances), key=str):
        old_item = old_instances[instance_id]
        new_item = new_instances[instance_id]
        old_collision = old_item.get("collision") or {}
        new_collision = new_item.get("collision") or {}
        for field in ("health",):
            old_value = old_collision.get(field)
            new_value = new_collision.get(field)
            if old_value is not None and new_value is not None and not same_value(old_value, new_value):
                out["healthChanged"] += 1
                if len(out["healthChanges"]) < 200:
                    out["healthChanges"].append({
                        "id": instance_id,
                        "field": f"collision.{field}",
                        "position": new_item.get("pos") or old_item.get("pos"),
                        "old": old_value,
                        "new": new_value,
                    })
        old_type = old_item.get("type") or {}
        new_type = new_item.get("type") or {}
        for field in ("health",):
            old_value = old_type.get(field)
            new_value = new_type.get(field)
            if old_value is not None and new_value is not None and not same_value(old_value, new_value):
                out["healthChanged"] += 1
                if len(out["healthChanges"]) < 200:
                    out["healthChanges"].append({
                        "id": instance_id,
                        "field": f"type.{field}",
                        "position": new_item.get("pos") or old_item.get("pos"),
                        "old": old_value,
                        "new": new_value,
                    })
        if not same_value(old_collision.get("collision_type"), new_collision.get("collision_type")):
            out["collisionTypeChanged"] += 1
        if not same_value(old_type.get("file"), new_type.get("file")) or not same_value(old_type.get("section"), new_type.get("section")):
            out["prototypeChanged"] += 1
        if old_item.get("name") != new_item.get("name"):
            out["renamed"] += 1
        if not same_value(old_item.get("pos"), new_item.get("pos")):
            out["moved"] += 1
    return out


def file_changed_pair(old_pack: pathlib.Path, new_pack: pathlib.Path,
                      old_manifest: dict, new_manifest: dict,
                      old_rel: str, new_rel: str) -> bool:
    return file_sha(old_pack, old_manifest, old_rel) != file_sha(new_pack, new_manifest, new_rel)


def glb_scene_instances(path: pathlib.Path) -> dict | None:
    """场景物件实例统计（导出器契约：GLB 每个实例 = 一个带 mesh 的节点，
    节点名 = 来源 sc2 物件名）。返回 {count, names(Counter)}；缺失/非 GLB 返回 None。"""
    try:
        data = path.read_bytes()
    except OSError:
        return None
    if len(data) < 20 or data[:4] != b"glTF":
        return None
    try:
        # GLB 头：magic(4) version(4) length(4) → JSON chunk: length(4) type(4 "JSON") data。
        json_len = int.from_bytes(data[12:16], "little")
        if data[16:20] != b"JSON":
            return None
        doc = json.loads(data[20:20 + json_len].decode("utf-8"))
    except (ValueError, UnicodeDecodeError):
        # 非 GLB/截断/伪 GLB：计数口径不适用，返回 None（fail-closed）。
        return None
    names = Counter(str(node.get("name") or "") for node in doc.get("nodes", []) if "mesh" in node)
    return {"count": sum(names.values()), "names": names}


def grass_instance_count(path: pathlib.Path) -> int | None:
    """草地实例数：grass.bin = 逐实例 12 字节（导出器契约）；异常尺寸返回 None。"""
    try:
        size = path.stat().st_size
    except OSError:
        return None
    if size <= 0 or size % 12 != 0:
        return None
    return size // 12


def diff_maps(old_pack: pathlib.Path, new_pack: pathlib.Path,
              old_manifest: dict, new_manifest: dict) -> dict:
    old_index = map_index(old_pack)
    new_index = map_index(new_pack)
    spaces = repo_map_spaces()
    old_groups = map_groups(old_index, spaces)
    new_groups = map_groups(new_index, spaces)
    out = {
        "added": [],
        "removed": [],
        "variantAdded": [],
        "variantRemoved": [],
        "maps": [],
        "presentationOnly": [],
    }
    old_ids = {map_id for values in old_groups.values() for map_id, _ in values}
    new_ids = {map_id for values in new_groups.values() for map_id, _ in values}
    old_bases = set(old_groups)
    new_bases = set(new_groups)
    for base in sorted(new_bases - old_bases):
        values = new_groups[base]
        base_id, entry = values[0]
        base_key = min((str(e.get("key")) for _, e in values if e.get("key")), key=lambda k: (len(k), k), default=base)
        out["added"].append({
            "mapId": base_id,
            "key": base_key,
            "display": entry.get("display") or base_key,
            "mapIds": [map_id for map_id, _ in values],
        })
    for base in sorted(old_bases - new_bases):
        values = old_groups[base]
        base_id, entry = values[0]
        base_key = min((str(e.get("key")) for _, e in values if e.get("key")), key=lambda k: (len(k), k), default=base)
        out["removed"].append({
            "mapId": base_id,
            "key": base_key,
            "display": entry.get("display") or base_key,
            "mapIds": [map_id for map_id, _ in values],
        })
    for map_id in sorted(new_ids - old_ids):
        found = next((e for values in new_groups.values() for mid, e in values if mid == map_id), None)
        base = spaces.get(map_id) or (str(found.get("variantKey") or found.get("key")) if found else None)
        if found and base in old_bases:
            out["variantAdded"].append({"mapId": map_id, "key": found.get("key"), "display": found.get("display")})
    for map_id in sorted(old_ids - new_ids):
        found = next((e for values in old_groups.values() for mid, e in values if mid == map_id), None)
        base = spaces.get(map_id) or (str(found.get("variantKey") or found.get("key")) if found else None)
        if found and base in new_bases:
            out["variantRemoved"].append({"mapId": map_id, "key": found.get("key"), "display": found.get("display")})

    for group in sorted(set(old_groups) & set(new_groups)):
        old_values = old_groups[group]
        new_values = new_groups[group]
        old_keys = {str(e.get("key")) for _, e in old_values if e.get("key")}
        new_keys = {str(e.get("key")) for _, e in new_values if e.get("key")}
        old_base_key = min(old_keys, key=lambda k: (len(k), k), default=group)
        new_base_key = min(new_keys, key=lambda k: (len(k), k), default=group)
        base_id = next((mid for mid, e in new_values if str(e.get("key")) == new_base_key),
                       min(map_id for map_id, _ in new_values))
        new_display = next((e.get("display") for mid, e in new_values if mid == base_id), None)
        old_base_id = next((mid for mid, e in old_values if str(e.get("key")) == old_base_key),
                           min(map_id for map_id, _ in old_values))
        old_display = next((e.get("display") for mid, e in old_values if mid == old_base_id), None)
        variant_ids = [map_id for map_id, _ in new_values]
        categories: list[str] = []
        files: dict[str, bool] = {}
        other_changed = False
        for family, names in MAP_FAMILIES.items():
            changed = False
            for name in names:
                if file_changed_pair(
                    old_pack, new_pack, old_manifest, new_manifest,
                    f"map/{old_base_key}/{name}", f"map/{new_base_key}/{name}",
                ):
                    changed = True
                    break
            files[family] = changed
            if changed:
                categories.append(family)
        # 场景物件/草地实例计数：GLB 每实例一个节点、grass.bin 每实例 12 字节（导出器契约）。
        scenery_diff: dict = {"old": None, "new": None, "added": {}, "removed": {}}
        grass_diff: dict = {"old": None, "new": None}
        if files.get("scenery"):
            old_scenery = glb_scene_instances(old_pack / "map" / old_base_key / "scenery.glb")
            new_scenery = glb_scene_instances(new_pack / "map" / new_base_key / "scenery.glb")
            if old_scenery is not None and new_scenery is not None:
                added = new_scenery["names"] - old_scenery["names"]
                removed = old_scenery["names"] - new_scenery["names"]
                scenery_diff = {
                    "old": old_scenery["count"],
                    "new": new_scenery["count"],
                    "added": dict(sorted(added.items())),
                    "removed": dict(sorted(removed.items())),
                }
        if files.get("grass"):
            old_grass = grass_instance_count(old_pack / "map" / old_base_key / "grass.bin")
            new_grass = grass_instance_count(new_pack / "map" / new_base_key / "grass.bin")
            if old_grass is not None and new_grass is not None:
                grass_diff = {"old": old_grass, "new": new_grass}
        # 变体目录按 variant 标签配对；11.22 新物化的变体目录没有旧侧对应项，
        # 不当作内容变化（其共享资产已由 base key 覆盖）。
        old_variants = {
            str(e.get("variant")): str(e.get("key"))
            for _, e in old_values
            if e.get("variant") and str(e.get("key")) != old_base_key
        }
        new_variants = {
            str(e.get("variant")): str(e.get("key"))
            for _, e in new_values
            if e.get("variant") and str(e.get("key")) != new_base_key
        }
        for tag in sorted(set(old_variants) & set(new_variants)):
            for name in ("destructibles.json", "terrain.json", "scenery.glb", "lighting.json"):
                if file_changed_pair(
                    old_pack, new_pack, old_manifest, new_manifest,
                    f"map/{old_variants[tag]}/{name}", f"map/{new_variants[tag]}/{name}",
                ):
                    other_changed = True
                    break
        if other_changed:
            files["variantContent"] = True
            if "variantContent" not in categories:
                categories.append("variantContent")
        destructibles = diff_destructibles(
            old_pack / "map" / old_base_key / "destructibles.json",
            new_pack / "map" / new_base_key / "destructibles.json",
        )
        presentation = {
            "ground": file_changed_pair(
                old_pack, new_pack, old_manifest, new_manifest,
                f"map/{old_base_key}/ground.webp", f"map/{new_base_key}/ground.webp"),
            "mini": file_changed_pair(
                old_pack, new_pack, old_manifest, new_manifest,
                f"map/{old_base_key}/mini.webp", f"map/{new_base_key}/mini.webp"),
            "map2d": file_changed_pair(
                old_pack, new_pack, old_manifest, new_manifest,
                f"map2d/{old_base_key}.webp", f"map2d/{new_base_key}.webp"),
            "cover": file_changed_pair(
                old_pack, new_pack, old_manifest, new_manifest,
                f"cover/{old_base_key}.webp", f"cover/{new_base_key}.webp"),
        }
        gameplay_changed = any(files.get(name) for name in MAP_FAMILIES)
        presentation_changed = any(presentation.values())
        if not gameplay_changed and not presentation_changed:
            continue
        counts = {
            "destructibleHealthChanged": destructibles["healthChanged"],
            "destructibleInstancesAdded": destructibles["instancesAdded"],
            "destructibleInstancesRemoved": destructibles["instancesRemoved"],
            "destructibleCollisionTypeChanged": destructibles["collisionTypeChanged"],
            "destructibleRenamed": destructibles["renamed"],
            "variantDirsChanged": 1 if files.get("variantContent") else 0,
            "sceneryOld": scenery_diff["old"],
            "sceneryNew": scenery_diff["new"],
            "sceneryAdded": sum(scenery_diff["added"].values()),
            "sceneryRemoved": sum(scenery_diff["removed"].values()),
            "grassOld": grass_diff["old"],
            "grassNew": grass_diff["new"],
        }
        detail_name = f"maps/{base_id}.json"
        row = {
            "mapId": base_id,
            "key": new_base_key,
            "display": new_display or old_display or new_base_key,
            "displayOld": old_display,
            "variantIds": variant_ids,
            "categories": categories,
            "counts": counts,
            "detail": detail_name,
        }
        if gameplay_changed:
            out["maps"].append(row)
        else:
            out["presentationOnly"].append(row)
        detail = {
            "schema": SCHEMA,
            "mapId": base_id,
            "from": {"key": old_base_key, "display": old_display, "variantIds": [mid for mid, _ in old_values]},
            "to": {"key": new_base_key, "display": new_display, "variantIds": variant_ids},
            "categories": categories,
            "files": files,
            "destructibles": destructibles,
            "scenery": scenery_diff,
            "grass": grass_diff,
            "presentation": presentation,
            "preview": {"fromKey": old_base_key, "toKey": new_base_key},
        }
        out.setdefault("_details", {}).setdefault(base_id, detail)
    out["added"].sort(key=lambda item: item["mapId"])
    out["removed"].sort(key=lambda item: item["mapId"])
    out["variantAdded"].sort(key=lambda item: item["mapId"])
    out["variantRemoved"].sort(key=lambda item: item["mapId"])
    out["maps"].sort(key=lambda item: item["mapId"])
    out["presentationOnly"].sort(key=lambda item: item["mapId"])
    return out


# ---------------------------------------------------------------------------
# manifest / 表现资源
# ---------------------------------------------------------------------------
def diff_manifest(old_manifest: dict, new_manifest: dict) -> dict:
    old_paths = set(old_manifest)
    new_paths = set(new_manifest)
    added = sorted(new_paths - old_paths)
    removed = sorted(old_paths - new_paths)
    changed = sorted(path for path in old_paths & new_paths if old_manifest[path].get("sha256") != new_manifest[path].get("sha256"))
    families = Counter(path.split("/", 1)[0] if "/" in path else path for path in changed)
    model_changed: set[str] = set()
    suspension_changed: set[str] = set()
    tread_changed: set[str] = set()
    for path in changed:
        parts = path.split("/")
        if len(parts) == 3 and parts[0] == "glb" and parts[2] in ("model.glb", "collision.glb"):
            model_changed.add(parts[1])
        elif len(parts) == 2 and parts[0] == "suspension":
            suspension_changed.add(pathlib.Path(parts[1]).stem)
        elif len(parts) == 2 and parts[0] == "tread":
            tread_changed.add(pathlib.Path(parts[1]).stem)
    tank_images = [path for path in changed if path.startswith("tank_images/") and path != "tank_images/crops.json"]
    return {
        "added": added,
        "removed": removed,
        "changed": changed,
        "families": dict(sorted(families.items())),
        "modelChanged": sorted(model_changed),
        "suspensionChanged": sorted(suspension_changed),
        "treadChanged": sorted(tread_changed),
        "tankImagesChanged": len(tank_images),
        "tankImageCropsChanged": "tank_images/crops.json" in changed,
        "map2dChanged": sum(1 for path in changed if path.startswith("map2d/")),
        "coverChanged": sum(1 for path in changed if path.startswith("cover/")),
        "groundChanged": sum(1 for path in changed if path.endswith("/ground.webp")),
        "dataFilesAdded": [path for path in added if path.startswith("data/")],
        "dataFilesRemoved": [path for path in removed if path.startswith("data/")],
    }


# ---------------------------------------------------------------------------
# 坦克明细
# ---------------------------------------------------------------------------
def tank_identity(cache: dict, tank: dict | None) -> dict:
    return {
        "name": cache.get("name") or (tank or {}).get("name"),
        "tier": cache.get("tier"),
        "nation": cache.get("nation"),
        "type": cache.get("type"),
    }


def tank_flags(cache: dict) -> dict:
    # 金币/收藏身份只用于前端展示（百科同款车名配色）；判定语义在提取链，
    # 这里照抄 tank_cache 的布尔值，不做二次推断。
    return {
        "premium": bool(cache.get("is_premium")),
        "collector": bool(cache.get("is_collector")),
    }


def tank_chips(changes: list[dict], armor_model: dict | None = None,
               configs: dict | None = None, limit: int = 4) -> list[dict]:
    priority = (
        "hp", "armor.hull.front", "armor.turret.front", "armor.hull.sides", "armor.turret.sides",
        "shells.ap.damage", "shells.ap.penetration", "shells.ap_cr_premium.damage",
        "shells.ap_cr_premium.penetration", "shells.he.damage", "speed_forward",
        "hull_traverse", "view_range", "gun_depression", "gun_elevation",
    )
    by_path = {change["path"]: change for change in changes}
    chips: list[dict] = []
    seen: set[str] = set()

    def push(path: str, old: Any, new: Any, *, change_path: str | None = None,
             config: dict | None = None) -> None:
        if len(chips) >= limit or path in seen:
            return
        seen.add(path)
        entry = {"path": path, "old": old, "new": new}
        # 配置 chip 的 path 里嵌了炮名（可能含点），单靠字符串拆不出字段；
        # 把可展示的字段路径与配置身份分开携带，UI 不再解析拼接串。
        if change_path is not None:
            entry["changePath"] = change_path
        if config is not None:
            entry["config"] = config
        chips.append(entry)

    for path in priority:
        change = by_path.get(path)
        if change:
            push(path, change["old"], change["new"])
        if len(chips) >= limit:
            return chips
    for change in changes:
        push(change["path"], change["old"], change["new"])
        if len(chips) >= limit:
            return chips
    # 概要没有变化时，用装甲板/配置变化补足卡片 chip。
    if armor_model:
        for plate in armor_model.get("plates") or []:
            push(f"armorModel.{plate['module']}.{plate['plate']}", plate.get("old"), plate.get("new"))
        for plate in armor_model.get("addedPlates") or []:
            push(f"armorModel.{plate['module']}.{plate['plate']}.added", None, plate.get("thickness"))
        for plate in armor_model.get("removedPlates") or []:
            push(f"armorModel.{plate['module']}.{plate['plate']}.removed", plate.get("thickness"), None)
    if configs:
        for entry in configs.get("configs") or []:
            key = entry.get("key") or {}
            prefix = f"configs.{key.get('turret')}|{key.get('gun')}"
            for change in entry.get("changes") or []:
                push(f"{prefix}.{change['path']}", change.get("old"), change.get("new"),
                     change_path=change["path"],
                     config={"turret": key.get("turret"), "gun": key.get("gun")})
                if len(chips) >= limit:
                    return chips
    return chips


def diff_tank(
    tank_id: str,
    old_cache: dict,
    new_cache: dict,
    old_tank: dict | None,
    new_tank: dict | None,
    old_manifest: dict,
    new_manifest: dict,
    old_research: dict | None,
    new_research: dict | None,
    old_vehicles: dict,
    new_vehicles: dict,
) -> dict | None:
    changes: list[dict] = []
    schema = schema_bucket()
    unresolved: list[str] = []
    summary_changes, summary_schema, summary_unresolved = diff_summary(old_cache, new_cache)
    changes.extend(summary_changes)
    merge_schema(schema, summary_schema)
    unresolved.extend(summary_unresolved)

    armor_summary = diff_tank_armor_summary((old_tank or {}).get("armor"), (new_tank or {}).get("armor"))
    armor_model = diff_armor_model((old_tank or {}).get("armor_model"), (new_tank or {}).get("armor_model"))
    configs = diff_configs((old_tank or {}).get("configs"), (new_tank or {}).get("configs"))
    unresolved.extend(configs.get("unresolved") or [])
    for kind in ("unpairedOld", "unpairedNew"):
        for key in configs.get(kind) or []:
            unresolved.append(f"configs.{kind}:{key}")
    merge_schema(schema, {
        "added": Counter(configs["schema"]["added"]),
        "removed": Counter(configs["schema"]["removed"]),
    })

    geometry: dict[str, bool] = {}
    for field in ("collision_boxes", "gun_collision", "model_origins"):
        old_value = (old_tank or {}).get(field)
        new_value = (new_tank or {}).get(field)
        if old_value is None and new_value is None:
            continue
        if old_value is None or new_value is None:
            schema["removed" if new_value is None else "added"][field] += 1
            continue
        changed = not same_value(old_value, new_value)
        geometry[field] = changed
        if changed:
            changes.append({"path": field, "category": "geometry", "old": "changed", "new": "changed"})
    for field in ("hull_spaced", "initial_turret_rotation"):
        old_value = (old_tank or {}).get(field)
        new_value = (new_tank or {}).get(field)
        if old_value is None and new_value is None:
            continue
        if old_value is None or new_value is None:
            schema["removed" if new_value is None else "added"][field] += 1
        elif not same_value(old_value, new_value):
            changes.append({"path": field, "category": "armor" if field == "hull_spaced" else "firepower",
                            "old": old_value, "new": new_value})
    # 未识别顶层字段
    handled = {
        "configs", "armor_model", "armor", "shells", "hp", "speed_forward", "speed_reverse",
        "hull_traverse", "turret_traverse_speed", "view_range", "gun_depression", "gun_elevation",
        "is_premium", "is_collector", "tier", "type", "nation", "name", "tank_id",
        "model_url", "visual_model_url", "collision_boxes", "gun_collision", "model_origins",
        "hull_spaced", "initial_turret_rotation",
    }
    for field in sorted(set(old_tank or {}) | set(new_tank or {})):
        if field in handled:
            continue
        old_value = (old_tank or {}).get(field)
        new_value = (new_tank or {}).get(field)
        if field not in (old_tank or {}):
            schema["added"][field] += 1
        elif field not in (new_tank or {}):
            schema["removed"][field] += 1
        elif not same_value(old_value, new_value):
            changes.append({"path": field, "category": "other", "old": old_value, "new": new_value})

    research = diff_research(old_research, new_research, old_vehicles, new_vehicles)
    unresolved.extend(research.get("unresolved") or [])

    model_changed = any(
        old_manifest.get(f"glb/{tank_id}/{name}", {}).get("sha256")
        != new_manifest.get(f"glb/{tank_id}/{name}", {}).get("sha256")
        for name in ("model.glb", "collision.glb")
    )
    suspension_changed = old_manifest.get(f"suspension/{tank_id}.json", {}).get("sha256") != new_manifest.get(f"suspension/{tank_id}.json", {}).get("sha256")
    tread_changed = old_manifest.get(f"tread/{tank_id}.json", {}).get("sha256") != new_manifest.get(f"tread/{tank_id}.json", {}).get("sha256")
    image_changed = old_manifest.get(f"tank_images/{tank_id}.webp", {}).get("sha256") != new_manifest.get(f"tank_images/{tank_id}.webp", {}).get("sha256")

    categories: list[str] = []
    for change in changes:
        if change["category"] not in categories:
            categories.append(change["category"])
    for entry in configs.get("configs") or []:
        for change in entry.get("changes") or []:
            if change["category"] not in categories:
                categories.append(change["category"])
    if armor_model["changed"] and "armor" not in categories:
        categories.append("armor")
    if research["changed"] and "research" not in categories:
        categories.append("research")
    config_structure_changed = bool(configs.get("added") or configs.get("removed"))
    if config_structure_changed and "firepower" not in categories:
        categories.append("firepower")
    chips = tank_chips(changes, armor_model, configs)
    if not chips and research["changed"]:
        chips = [{"path": "research", "old": "changed", "new": "changed"}]
    elif not chips and config_structure_changed:
        chips = [{"path": "configs", "old": "changed", "new": "changed"}]
    if not changes and not armor_model["changed"] and not research["changed"] and not config_structure_changed:
        has_schema = bool(
            schema["added"] or schema["removed"]
            or research["schema"]["added"] or research["schema"]["removed"]
        )
        if has_schema or configs.get("unpairedOld") or configs.get("unpairedNew"):
            return {
                "_schemaOnly": True,
                "schema": schema_to_json(schema),
                "research": research,
                "unresolved": sorted(set(unresolved)),
            }
        return None

    return {
        "schemaVersion": SCHEMA,
        "tankId": int(tank_id) if tank_id.isdigit() else tank_id,
        "identity": tank_identity(new_cache, new_tank),
        "categories": categories,
        "changes": changes,
        "chips": chips,
        "armor": {
            "summary": armor_summary,
            "model": armor_model,
        },
        "configs": configs,
        "research": research,
        "geometry": geometry,
        "artifacts": {
            "glb": model_changed,
            "suspension": suspension_changed,
            "tread": tread_changed,
            "tankImage": image_changed,
        },
        "schema": schema_to_json(schema),
        "unresolved": sorted(set(unresolved)),
    }


# ---------------------------------------------------------------------------
# 单对构建
# ---------------------------------------------------------------------------
def build_pair(base_dir: pathlib.Path, target_dir: pathlib.Path, out_root: pathlib.Path,
               strict: bool = False) -> dict:
    base_id = dataset_identity(base_dir)
    target_id = dataset_identity(target_dir)
    if base_id["variant"] or target_id["variant"]:
        raise DiffError("版本差异只接受正式 dataset（variant 必须为空）")
    if base_id["dataset"] == target_id["dataset"]:
        raise DiffError("base 与 target dataset 相同")
    old_manifest = manifest_index(base_dir)
    new_manifest = manifest_index(target_dir)
    old_cache = tank_cache(base_dir)
    new_cache = tank_cache(target_dir)
    old_research = research_modules(base_dir)
    new_research = research_modules(target_dir)
    old_vehicles = vehicle_stem_index(base_dir)
    new_vehicles = vehicle_stem_index(target_dir)
    old_tree = tech_tree_docs(base_dir)
    new_tree = tech_tree_docs(target_dir)

    old_ids = set(old_cache)
    new_ids = set(new_cache)
    added_ids = sorted(new_ids - old_ids, key=lambda x: int(x) if x.isdigit() else x)
    removed_ids = sorted(old_ids - new_ids, key=lambda x: int(x) if x.isdigit() else x)

    tanks: list[dict] = []
    details: dict[str, dict] = {}
    schema = schema_bucket()
    unresolved: list[str] = []
    model_only: list[dict] = []
    technical_only = 0
    config_changed = 0
    armor_changed = 0

    def model_row(tank_id: str) -> dict:
        cache = new_cache[tank_id]
        return {
            "id": int(tank_id) if tank_id.isdigit() else tank_id,
            "name": cache.get("name"),
            "tier": cache.get("tier"),
            "nation": cache.get("nation"),
            "type": cache.get("type"),
            **tank_flags(cache),
        }

    for tank_id in sorted(old_ids & new_ids, key=lambda x: int(x) if x.isdigit() else x):
        old_tank = load_json(base_dir / "tank" / f"{tank_id}.json") if (base_dir / "tank" / f"{tank_id}.json").is_file() else None
        new_tank = load_json(target_dir / "tank" / f"{tank_id}.json") if (target_dir / "tank" / f"{tank_id}.json").is_file() else None
        detail = diff_tank(
            tank_id, old_cache[tank_id], new_cache[tank_id], old_tank, new_tank,
            old_manifest, new_manifest,
            old_research.get(tank_id), new_research.get(tank_id),
            old_vehicles, new_vehicles,
        )
        model_changed = any(
            old_manifest.get(f"glb/{tank_id}/{name}", {}).get("sha256")
            != new_manifest.get(f"glb/{tank_id}/{name}", {}).get("sha256")
            for name in ("model.glb", "collision.glb")
        )
        if detail is None:
            if model_changed:
                model_only.append(model_row(tank_id))
            if old_tank != new_tank or old_research.get(tank_id) != new_research.get(tank_id) or model_changed:
                technical_only += 1
            continue
        if detail.get("_schemaOnly"):
            merge_schema(schema, {
                "added": Counter(detail["schema"]["added"]),
                "removed": Counter(detail["schema"]["removed"]),
            })
            merge_schema(schema, detail["research"]["schema"])
            unresolved.extend(detail["unresolved"])
            technical_only += 1
            if model_changed:
                model_only.append(model_row(tank_id))
            continue
        merge_schema(schema, {
            "added": Counter(detail["schema"]["added"]),
            "removed": Counter(detail["schema"]["removed"]),
        })
        merge_schema(schema, detail["research"]["schema"])
        unresolved.extend(detail["unresolved"])
        if detail["configs"]["changed"]:
            config_changed += 1
        if detail["armor"]["model"]["changed"]:
            armor_changed += 1
        row = {
            "id": int(tank_id) if tank_id.isdigit() else tank_id,
            "name": detail["identity"].get("name"),
            "tier": detail["identity"].get("tier"),
            "nation": detail["identity"].get("nation"),
            "type": detail["identity"].get("type"),
            **tank_flags(new_cache[tank_id]),
            "status": "changed",
            "categories": detail["categories"],
            "chips": detail["chips"],
            "detail": f"tanks/{tank_id}.json",
        }
        tanks.append(row)
        details[tank_id] = detail

    for tank_id in added_ids:
        cache = new_cache[tank_id]
        tanks.append({
            "id": int(tank_id) if tank_id.isdigit() else tank_id,
            "name": cache.get("name"), "tier": cache.get("tier"),
            "nation": cache.get("nation"), "type": cache.get("type"),
            **tank_flags(cache),
            "status": "added", "categories": ["added"], "chips": [], "detail": None,
        })
    for tank_id in removed_ids:
        cache = old_cache[tank_id]
        tanks.append({
            "id": int(tank_id) if tank_id.isdigit() else tank_id,
            "name": cache.get("name"), "tier": cache.get("tier"),
            "nation": cache.get("nation"), "type": cache.get("type"),
            **tank_flags(cache),
            "status": "removed", "categories": ["removed"], "chips": [], "detail": None,
        })

    map_diff = diff_maps(base_dir, target_dir, old_manifest, new_manifest)
    manifest_diff = diff_manifest(old_manifest, new_manifest)
    tech_tree = diff_tech_tree(old_tree, new_tree, all_module_stems(old_research), all_module_stems(new_research))
    items = diff_items(base_dir, target_dir)

    pair_dir = out_root / f"{base_id['dataset']}__{target_id['dataset']}" / f"{base_id['release']}__{target_id['release']}"
    pair_dir.mkdir(parents=True, exist_ok=True)
    for tank_id, detail in details.items():
        write_json(pair_dir / "tanks" / f"{tank_id}.json", detail)
    for map_id, detail in (map_diff.get("_details") or {}).items():
        write_json(pair_dir / "maps" / f"{map_id}.json", detail)
    items_doc = {"schema": SCHEMA, "items": items, "techTree": tech_tree}
    write_json(pair_dir / "items.json", items_doc)

    counts = {
        "tanks": {
            "added": len(added_ids),
            "removed": len(removed_ids),
            "changed": len(details),
            "armorChanged": armor_changed,
            "configChanged": config_changed,
            "modelOnly": len(model_only),
            "technicalOnly": technical_only,
        },
        "maps": {
            "added": len(map_diff["added"]),
            "removed": len(map_diff["removed"]),
            "variantAdded": len(map_diff["variantAdded"]),
            "variantRemoved": len(map_diff["variantRemoved"]),
            "changed": len(map_diff["maps"]),
            "presentationOnly": len(map_diff["presentationOnly"]),
            "byFamily": dict(sorted(Counter(
                family for row in map_diff["maps"] for family in row["categories"]
            ).items())),
        },
        "items": {
            "files": {
                name: {
                    "added": len(doc.get("added") or []),
                    "removed": len(doc.get("removed") or []),
                    "changed": len(doc.get("changed") or []),
                    "schema": doc.get("schema") or {"added": {}, "removed": {}},
                }
                for name, doc in items.items()
            },
            "techTree": {
                "addedNodes": len(tech_tree["addedNodes"]),
                "removedNodes": len(tech_tree["removedNodes"]),
                "changedNodes": len(tech_tree["changedNodes"]),
                "addedEdges": len(tech_tree["addedEdges"]),
                "removedEdges": len(tech_tree["removedEdges"]),
                "changedEdges": len(tech_tree["changedEdges"]),
            },
        },
        "schema": schema_to_json(schema),
        "presentation": {
            "tankImagesChanged": manifest_diff["tankImagesChanged"],
            "tankImageCropsChanged": manifest_diff["tankImageCropsChanged"],
            "map2dChanged": manifest_diff["map2dChanged"],
            "coverChanged": manifest_diff["coverChanged"],
            "groundChanged": manifest_diff["groundChanged"],
        },
        "artifacts": {
            "modelChanged": len(manifest_diff["modelChanged"]),
            "suspensionChanged": len(manifest_diff["suspensionChanged"]),
            "treadChanged": len(manifest_diff["treadChanged"]),
            "dataFilesAdded": manifest_diff["dataFilesAdded"],
            "dataFilesRemoved": manifest_diff["dataFilesRemoved"],
        },
    }

    summary = {
        "schema": SCHEMA,
        "pair": {
            "from": base_id,
            "to": target_id,
        },
        "meta": {
            "tool": "version_diff.py",
            "toolVersion": TOOL_VERSION,
            "generated": utc_now(),
            "pipelineChanged": base_id.get("upstreamCommit") != target_id.get("upstreamCommit"),
        },
        "counts": counts,
        "tanks": tanks,
        "maps": map_diff["maps"],
        "mapsPresentationOnly": map_diff["presentationOnly"],
        "mapsAdded": map_diff["added"],
        "mapsRemoved": map_diff["removed"],
        "mapsVariantAdded": map_diff["variantAdded"],
        "mapsVariantRemoved": map_diff["variantRemoved"],
        "modelOnly": model_only,
        "unresolved": sorted(set(unresolved)),
    }
    write_json(pair_dir / "summary.json", summary)
    meta = {
        "schema": SCHEMA,
        "tool": "version_diff.py",
        "toolVersion": TOOL_VERSION,
        "generated": summary["meta"]["generated"],
        "from": base_id,
        "to": target_id,
        "pipelineChanged": summary["meta"]["pipelineChanged"],
    }
    write_json(pair_dir / "meta.json", meta)

    if strict and summary["unresolved"]:
        raise DiffError(f"strict: 存在未解析配对: {summary['unresolved'][:10]}")
    return {
        "id": f"{base_id['dataset']}__{target_id['dataset']}",
        "from": base_id,
        "to": target_id,
        "path": pair_dir.relative_to(out_root).as_posix(),
        "counts": counts,
    }


def utc_now() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


# ---------------------------------------------------------------------------
# lineage / index / check
# ---------------------------------------------------------------------------
def load_lineage(path: pathlib.Path) -> dict:
    doc = load_json(path)
    if doc.get("schema") != SCHEMA:
        raise DiffError(f"{path}: lineage schema 不支持")
    entries = doc.get("lineage")
    if not isinstance(entries, list) or not entries:
        raise DiffError(f"{path}: lineage 为空")
    seen: set[str] = set()
    for index, entry in enumerate(entries):
        dataset = entry.get("dataset")
        predecessor = entry.get("predecessor")
        if not dataset or dataset in seen:
            raise DiffError(f"{path}: dataset 缺失或重复: {dataset}")
        if index == 0:
            if predecessor:
                raise DiffError(f"{path}: 首个 dataset 不应有 predecessor")
        elif predecessor != entries[index - 1].get("dataset"):
            raise DiffError(f"{path}: {dataset} 的 predecessor 必须紧邻上一条")
        seen.add(dataset)
    return doc


def validate_dataset_dir(root: pathlib.Path, dataset: str) -> pathlib.Path:
    path = root / dataset
    if not path.exists():
        raise DiffError(f"dataset 不存在: {path}")
    identity = dataset_identity(path)
    if identity["dataset"] != dataset:
        raise DiffError(f"dataset_id 与目录名不一致: {identity['dataset']!r} vs {dataset!r}")
    return path


def build_index(out_root: pathlib.Path, entries: list[dict]) -> dict:
    index = {
        "schema": SCHEMA,
        "generated": utc_now(),
        "pairs": sorted(entries, key=lambda item: (str(item["from"].get("gameVersion")), str(item["to"].get("gameVersion")))),
    }
    write_json(out_root / "index.json", index)
    return index


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--base", help="旧 dataset/pack 目录")
    ap.add_argument("--target", help="新 dataset/pack 目录")
    ap.add_argument("--lineage", help="data/version_lineage.json")
    ap.add_argument("--datasets-root", help="release/datasets")
    ap.add_argument("--out", required=True, help="release/diffs")
    ap.add_argument("--all", action="store_true", help="按 lineage 生成全部相邻对")
    ap.add_argument("--strict", action="store_true", help="unpaired/unresolved 直接失败")
    args = ap.parse_args(argv)

    out_root = pathlib.Path(args.out).resolve()
    out_root.mkdir(parents=True, exist_ok=True)
    if args.all:
        if not args.lineage or not args.datasets_root:
            raise DiffError("--all 需要 --lineage 与 --datasets-root")
        lineage = load_lineage(pathlib.Path(args.lineage))
        datasets_root = pathlib.Path(args.datasets_root)
        entries: list[dict] = []
        items = lineage["lineage"]
        for index in range(1, len(items)):
            base = validate_dataset_dir(datasets_root, items[index - 1]["dataset"])
            target = validate_dataset_dir(datasets_root, items[index]["dataset"])
            entry = build_pair(base, target, out_root, strict=args.strict)
            entries.append(entry)
            print(f"{entry['id']}: tanks +{entry['counts']['tanks']['added']} -{entry['counts']['tanks']['removed']} "
                  f"~{entry['counts']['tanks']['changed']} (armor {entry['counts']['tanks']['armorChanged']}), "
                  f"maps {entry['counts']['maps']['changed']}")
        build_index(out_root, entries)
        return 0
    if not args.base or not args.target:
        raise DiffError("单对模式需要 --base 与 --target")
    entry = build_pair(pathlib.Path(args.base), pathlib.Path(args.target), out_root, strict=args.strict)
    print(json.dumps(entry, ensure_ascii=False, indent=1))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except DiffError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        raise SystemExit(2)