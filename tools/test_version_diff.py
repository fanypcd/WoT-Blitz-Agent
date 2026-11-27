#!/usr/bin/env python3
"""`tools/version_diff.py` 的独立回归测试（无 pytest 依赖）。

覆盖：
- 配置按 turret_local/gun_local 配对；重复键 fail-closed；
- 仅 shell_global_ids 变化不产生语义变化；
- 装甲板增/删/厚度变化；
- mechanics / stats.clip / skills.effects.lobbyIcon 单侧字段进 schema 桶；
- 研发模块 stem 重命名 + unlock 目标解析后不误报；
- 地图变体按 map_id/space 聚合，不把 variantKey 自指的目录当新增地图；
- destructibles 健康变化 vs 名称重命名分开计数；
- lineage 校验与 build_pair 集成；
- 配置 chip 同时携带可展示字段路径与配置身份，避免 UI 解析含点的炮名。
"""
from __future__ import annotations

import hashlib
import json
import pathlib
import struct
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import version_diff as vd  # noqa: E402


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _write_json(path: pathlib.Path, data) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")


def write_pack(root: pathlib.Path, *, dataset: str, release: str, caches: dict,
               tanks: dict, modules: list | None = None, tree: dict | None = None,
               maps: dict | None = None, destructibles: dict | None = None,
               extra_files: dict | None = None, skills: dict | None = None) -> None:
    root.mkdir(parents=True, exist_ok=True)
    _write_json(root / "version.json", {
        "schema": 1,
        "dataset_id": dataset,
        "label": dataset,
        "game_version": "release/11.21.0",
        "variant": None,
        "release": release,
        "content_digest": "sha256:" + "0" * 64,
        "manifest_sha256": "0" * 64,
        "tank_count": len(caches),
    })
    _write_json(root / "manifest.json", {
        "version": 1,
        "generated": "2026-01-01T00:00:00+00:00",
        "upstream_commit": "a" * 40,
        "worktree_dirty": False,
        "files": [],
    })
    _write_json(root / "data" / "tank_cache.json", caches)
    for tank_id, tank in tanks.items():
        _write_json(root / "tank" / f"{tank_id}.json", tank)
    if modules is not None:
        _write_json(root / "research" / "modules" / "ussr.json", {
            "meta": {"nation": "ussr", "count": len(modules)},
            "tanks": {module.get("name") or module.get("tankId"): module for module in modules},
        })
    if tree is not None:
        _write_json(root / "research" / "tech_tree" / "ussr.json", tree)
    if skills is not None:
        _write_json(root / "research" / "skills.json", skills)
    if maps is not None:
        _write_json(root / "index.json", {"version": 1, "maps": maps})
    if destructibles is not None:
        _write_json(root / "map" / "rudniki" / "destructibles.json", destructibles)
    for rel, data in (extra_files or {}).items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data if isinstance(data, bytes) else json.dumps(data, ensure_ascii=False).encode("utf-8"))


def base_cache(tank_id: str = "1") -> dict:
    return {
        tank_id: {
            "name": "T-34", "tier": 5, "type": "mediumTank", "nation": "ussr",
            "is_premium": False, "is_collector": False,
            "armor": {"turret_front": 60, "turret_sides": 60, "turret_rear": 55,
                      "hull_front": 50, "hull_sides": 50, "hull_rear": 45},
            "hp": 680, "speed_forward": 48, "speed_reverse": 18,
            "hull_traverse": 46.0, "view_range": 240.0, "turret_traverse_speed": 49.0,
            "gun_depression": 8.0, "gun_elevation": 26.0,
            "shells": [
                {"shell_type": "ap", "penetration": 125, "damage": 160},
                {"shell_type": "he", "penetration": 39, "damage": 200},
            ],
        }
    }


def base_tank(config: dict | None = None, armor_model: dict | None = None) -> dict:
    cfg = {
        "id": 0, "turret_name": "", "turret_local": 0, "gun_local": 0,
        "label": "76 mm L-11", "caliber": 76, "reload_time": 8.3,
        "aim_time": 2.5, "dispersion": 0.47, "dpm": 1012.0,
        "standard_damage": 160.0, "pitch_limits": {"min": -8.0, "max": 26.0},
        "shells": [
            {"type": "ap", "penetration": 125, "damage": 160},
            {"type": "he", "penetration": 39, "damage": 200},
        ],
        "engines": [{"name": "V-2", "power": 400.0, "fire_chance": 0.15}],
    }
    if config:
        cfg.update(config)
    return {
        "tank_id": 1, "name": "T-34", "tier": 5, "type": "mediumTank", "nation": "ussr",
        "hp": 680, "speed_forward": 48, "speed_reverse": 18, "hull_traverse": 46.0,
        "view_range": 240.0, "gun_depression": 8.0, "gun_elevation": 26.0,
        "armor": {"hull": {"front": 50, "sides": 50, "rear": 45},
                  "turret": {"front": 60, "sides": 60, "rear": 55}},
        "armor_model": armor_model or {
            "hull": {"plates": {"1": 50.0, "3": 50.0}, "primary": {"front": "armor_1", "sides": "armor_3"}, "spaced": []},
            "turret": {"plates": {"1": 60.0}, "primary": {"front": "armor_1"}, "spaced": []},
            "gun": {"plates": {"1": 52.0}, "primary": {"front": "armor_1"}, "spaced": ["1"]},
            "chassis": {"left_track": 20.0, "right_track": 20.0},
        },
        "configs": [cfg],
    }


def base_modules() -> list:
    return [{
        "tankId": 1, "name": "T-34", "tier": 5,
        "chassis": [{"stem": "T-34_mod_1941", "name_en": "T-34 mod. 1941", "level": 4, "price": 0,
                     "stats": {"rotationSpeed": 43.0}, "unlocks": []}],
        "turrets": [{
            "stem": "T-34_mod_1940", "name_en": "T-34 mod. 1940", "level": 4, "price": 0,
            "stats": {"rotationSpeed": 40.0},
            "guns": [{"stem": "_76mm_L-11", "name_en": "76 mm L-11", "level": 4, "price": 0,
                      "stats": {"reloadTime": 8.3}, "unlocks": []}],
            "unlocks": [],
        }],
        "engines": [{
            "stem": "V-2", "name_en": "V-2", "level": 1, "price": 0,
            "stats": {"power": 400.0},
            "unlocks": [{"kind": "engine", "stem": "V-2", "cost": {"xp": 100}}],
        }],
    }]


def base_tree() -> dict:
    return {
        "meta": {"gameVersion": "x"},
        "nodes": [{"stem": "T-34", "tankId": 1, "tier": 5, "price": 0, "tags": [], "x": 1, "y": 1}],
        "pending": [],
        "edges": [],
    }


def test_config_pairing_by_local_id() -> None:
    old = base_tank()["configs"][0]
    new = dict(old, reload_time=9.0)
    result = vd.diff_configs([old], [new])
    assert not result["unpairedOld"] and not result["unpairedNew"]
    assert any(change["path"] == "reload_time" for change in result["configs"][0]["changes"])


def test_config_duplicate_is_unpaired() -> None:
    old = base_tank()["configs"][0]
    result = vd.diff_configs([old, dict(old)], [old])
    assert result["unpairedOld"], "重复 localId 必须进 unpaired，不能按下标配对"


def test_shell_global_ids_only_is_ignored() -> None:
    old = base_tank()["configs"][0]
    new = dict(old, shell_global_ids=[999, 1000])
    result = vd.diff_configs([old], [new])
    assert not result["changed"]
    assert not result["configs"][0]["changes"]


def test_armor_plate_changes() -> None:
    old_model = base_tank()["armor_model"]
    new_model = json.loads(json.dumps(old_model))
    new_model["hull"]["plates"]["3"] = 72.0
    new_model["hull"]["plates"]["2"] = 60.0
    result = vd.diff_armor_model(old_model, new_model)
    assert any(item["plate"] == "3" and item["old"] == 50.0 and item["new"] == 72.0 for item in result["plates"])
    assert any(item["plate"] == "2" for item in result["addedPlates"])
    assert result["changed"]


def test_mechanics_is_schema_only() -> None:
    cache = base_cache()["1"]
    old_tank = base_tank()
    new_tank = dict(old_tank, mechanics=[{"name": "LTCamo"}])
    detail = vd.diff_tank("1", cache, cache, old_tank, new_tank, {}, {}, None, None, {}, {})
    assert detail is not None and detail.get("_schemaOnly")
    assert detail["schema"]["added"].get("mechanics") == 1


def test_research_clip_is_schema_only() -> None:
    old = base_modules()[0]
    new = json.loads(json.dumps(old))
    new["turrets"][0]["guns"][0]["stats"]["clip"] = {"count": 4, "rate": 10.0}
    result = vd.diff_research(old, new, {}, {})
    assert not result["changed"]
    assert result["schema"]["added"].get("stats.clip") == 1


def test_research_stem_rename_and_unlock_resolution() -> None:
    old = base_modules()[0]
    new = json.loads(json.dumps(old))
    new["engines"][0]["stem"] = "V-2_3"
    new["engines"][0]["unlocks"][0]["stem"] = "V-2_3"
    result = vd.diff_research(old, new, {}, {})
    assert not result["changed"], "stem 重命名但显示名/目标不变，不应算变化"
    assert not result["unresolved"]


def test_items_lobby_icon_is_schema_only() -> None:
    old = {"meta": {"gameVersion": "a"}, "items": {"lucky_shot": {"effects": {"chance": 0.01}}}}
    new = {"meta": {"gameVersion": "b"}, "items": {"lucky_shot": {"effects": {"chance": 0.01,
           "lobbyIcon": "~res:/x"}}}}
    result = vd.diff_doc(old, new)
    assert not result["changed"]
    assert result["schema"]["added"].get("effects.lobbyIcon") == 1


def test_map_variant_self_key_is_not_new_map() -> None:
    original = vd.repo_map_spaces
    vd.repo_map_spaces = lambda: {6: "06_rudniki_rd", 60: "06_rudniki_rd"}
    try:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            old = root / "old"
            new = root / "new"
            maps_old = {"6": {"key": "rudniki", "display": "Mines", "variantKey": "rudniki"}}
            maps_new = {
                "6": {"key": "rudniki", "display": "Mines", "variantKey": "rudniki"},
                "60": {"key": "rudniki_01", "display": "Mines - Hill", "variantKey": "rudniki_01", "variant": "rd1"},
            }
            write_pack(old, dataset="a", release="a-r1", caches=base_cache(), tanks={"1": base_tank()},
                       maps=maps_old, destructibles={"instances": []},
                       extra_files={"map/rudniki/scenery.glb": b"old"})
            write_pack(new, dataset="b", release="b-r1", caches=base_cache(), tanks={"1": base_tank()},
                       maps=maps_new,
                       destructibles={"instances": [{"id": 1, "name": "CollisionVolume",
                                                     "collision": {"health": 150, "collision_type": 10},
                                                     "type": {"health": 150}, "pos": [0, 0, 0]}]},
                       extra_files={"map/rudniki/scenery.glb": b"new"})
            # 旧侧 destructibles 也写入
            _write_json(old / "map" / "rudniki" / "destructibles.json",
                        {"instances": [{"id": 1, "name": "old", "collision": {"health": 5},
                                        "type": {"health": 5}, "pos": [0, 0, 0]}]})
            result = vd.diff_maps(old, new, {}, {})
            assert result["added"] == []
            assert len(result["variantAdded"]) == 1
            assert len(result["maps"]) == 1
            assert result["maps"][0]["key"] == "rudniki"
            assert result["maps"][0]["counts"]["destructibleHealthChanged"] == 2
    finally:
        vd.repo_map_spaces = original


def test_destructibles_health_vs_rename() -> None:
    old = {"instances": [{"id": 1, "name": "old", "collision": {"health": 5, "collision_type": 3},
                          "type": {"health": 5, "file": "a"}, "pos": [0, 0, 0]}]}
    new = {"instances": [{"id": 1, "name": "CollisionVolume", "collision": {"health": 150, "collision_type": 10},
                          "type": {"health": 150, "file": "a"}, "pos": [0, 0, 0]}]}
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        _write_json(root / "old.json", old)
        _write_json(root / "new.json", new)
        result = vd.diff_destructibles(root / "old.json", root / "new.json")
        assert result["healthChanged"] == 2
        assert result["collisionTypeChanged"] == 1
        assert result["renamed"] == 1


def test_lineage_validation() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        path = pathlib.Path(tmp) / "lineage.json"
        _write_json(path, {"schema": 1, "lineage": [
            {"dataset": "a"}, {"dataset": "b", "predecessor": "a"}, {"dataset": "c", "predecessor": "a"},
        ]})
        try:
            vd.load_lineage(path)
        except vd.DiffError:
            pass
        else:
            raise AssertionError("坏 lineage 必须失败")


def test_build_pair_added_and_changed() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        old = root / "old"
        new = root / "new"
        out = root / "out"
        cache_old = base_cache()
        cache_new = base_cache()
        cache_new["1"] = dict(cache_new["1"], hp=700)
        cache_new["2"] = dict(cache_new["1"], name="T-34-85", is_premium=True, is_collector=False)
        tank_new = base_tank()
        tank_new["hp"] = 700
        tank2 = base_tank()
        tank2.update({"tank_id": 2, "name": "T-34-85", "hp": 700})
        modules_new = base_modules() + [{
            **base_modules()[0], "tankId": 2, "name": "T-34-85",
        }]
        write_pack(old, dataset="a", release="a-r1", caches=cache_old, tanks={"1": base_tank()},
                   modules=base_modules(), tree=base_tree(), maps={})
        write_pack(new, dataset="b", release="b-r1", caches=cache_new, tanks={"1": tank_new, "2": tank2},
                   modules=modules_new, tree=base_tree(), maps={})
        entry = vd.build_pair(old, new, out)
        assert entry["counts"]["tanks"]["added"] == 1
        assert entry["counts"]["tanks"]["changed"] == 1
        summary_path = out / entry["path"] / "summary.json"
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
        assert any(item["status"] == "added" and item["id"] == 2 for item in summary["tanks"])
        added = next(item for item in summary["tanks"] if item["status"] == "added")
        changed = next(item for item in summary["tanks"] if item["status"] == "changed")
        assert added["premium"] is True and added["collector"] is False
        assert changed["premium"] is False and changed["collector"] is False
        assert any(change["path"] == "hp" for item in summary["tanks"]
                   if item["status"] == "changed" for change in item["chips"])


def test_config_chip_carries_structured_path_and_identity():
    configs = {
        "configs": [{
            "key": {"turret": "", "gun": "7,5 cm Kw.K. L/100"},
            "changes": [{
                "path": "shells.ap.penetration",
                "old": 200, "new": 210,
            }],
        }],
    }
    chips = vd.tank_chips([], None, configs)
    assert len(chips) == 1
    chip = chips[0]
    assert chip["path"].startswith("configs.|7,5 cm Kw.K. L/100.")
    assert chip["changePath"] == "shells.ap.penetration"
    assert chip["config"] == {"turret": "", "gun": "7,5 cm Kw.K. L/100"}


def test_map_scenery_instance_counts():
    # 场景物件计数 = GLB 带 mesh 节点数（导出器契约），按名字多重集算增删。
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        old = root / "old"
        new = root / "new"
        out = root / "out"

        def scenery_glb(count, extra_names=()):
            nodes = [{"name": f"tree_{i}.sc2", "mesh": i} for i in range(count)]
            nodes += [{"name": name, "mesh": len(nodes) + i} for i, name in enumerate(extra_names)]
            doc = {
                "asset": {"version": "2.0"},
                "scene": 0,
                "scenes": [{"nodes": list(range(len(nodes)))}],
                "nodes": nodes,
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]} for _ in nodes],
                "accessors": [{"componentType": 5126, "count": 3, "type": "VEC3",
                               "min": [0, 0, 0], "max": [0, 0, 0]}],
                "buffers": [{"byteLength": 36}],
                "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
            }
            # GLB 布局：magic/version/total → chunk length/type("JSON")/data
            payload = json.dumps(doc, separators=(",", ":")).encode("utf-8")
            header = (b"glTF" + struct.pack("<II", 2, 20 + len(payload))
                      + struct.pack("<I", len(payload)) + b"JSON")
            return header + payload

        maps_fixture = {1: {"key": "karelia", "display": "Karelia", "space": "karelia"}}
        write_pack(old, dataset="a", release="a-r1", caches=base_cache(),
                   tanks={"1": base_tank()}, modules=base_modules(), tree=base_tree(), maps=maps_fixture,
                   extra_files={"map/karelia/scenery.glb": scenery_glb(3),
                                "map/karelia/grass.bin": bytes(36)})
        write_pack(new, dataset="b", release="b-r1", caches=base_cache(),
                   tanks={"1": base_tank()}, modules=base_modules(), tree=base_tree(), maps=maps_fixture,
                   extra_files={"map/karelia/scenery.glb": scenery_glb(3, ("bush02.sc2",)),
                                "map/karelia/grass.bin": bytes(48)})
        entry = vd.build_pair(old, new, out)
        summary = json.loads((out / entry["path"] / "summary.json").read_text(encoding="utf-8"))
        row = next(item for item in summary["maps"] if item["key"] == "karelia")
        assert row["counts"]["sceneryOld"] == 3
        assert row["counts"]["sceneryNew"] == 4
        assert row["counts"]["sceneryAdded"] == 1
        assert row["counts"]["sceneryRemoved"] == 0
        assert row["counts"]["grassOld"] == 3
        assert row["counts"]["grassNew"] == 4
        detail = json.loads((out / entry["path"] / "maps" / "1.json").read_text(encoding="utf-8"))
        assert detail["scenery"]["added"] == {"bush02.sc2": 1}


def test_config_added_produces_changed_tank():
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        old = root / "old"
        new = root / "new"
        out = root / "out"
        cfg = base_tank()["configs"][0]
        cfg2 = dict(cfg, gun_local=1, label="85 mm D-5T", caliber=85)
        new_tank = base_tank()
        new_tank["configs"] = [cfg, cfg2]
        write_pack(old, dataset="a", release="a-r1", caches=base_cache(),
                   tanks={"1": base_tank()}, modules=base_modules(), tree=base_tree(), maps={})
        write_pack(new, dataset="b", release="b-r1", caches=base_cache(),
                   tanks={"1": new_tank}, modules=base_modules(), tree=base_tree(), maps={})
        entry = vd.build_pair(old, new, out)
        summary = json.loads((out / entry["path"] / "summary.json").read_text(encoding="utf-8"))
        row = next(item for item in summary["tanks"] if item["id"] == 1)
        assert row["status"] == "changed"
        assert "firepower" in row["categories"]
        detail = json.loads((out / entry["path"] / "tanks" / "1.json").read_text(encoding="utf-8"))
        assert detail["configs"]["added"][0]["gun"] == "85 mm D-5T"
        assert any(chip["path"] == "configs" for chip in detail["chips"])


def main() -> int:
    tests = [
        test_config_pairing_by_local_id,
        test_config_duplicate_is_unpaired,
        test_shell_global_ids_only_is_ignored,
        test_armor_plate_changes,
        test_mechanics_is_schema_only,
        test_research_clip_is_schema_only,
        test_research_stem_rename_and_unlock_resolution,
        test_items_lobby_icon_is_schema_only,
        test_map_variant_self_key_is_not_new_map,
        test_destructibles_health_vs_rename,
        test_lineage_validation,
        test_build_pair_added_and_changed,
        test_config_chip_carries_structured_path_and_identity,
        test_config_added_produces_changed_tank,
        test_map_scenery_instance_counts,
    ]
    for test in tests:
        test()
        print(f"PASS {test.__name__}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())