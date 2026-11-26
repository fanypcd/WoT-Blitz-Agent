#!/usr/bin/env python3
"""封面图导出契约（standalone，无 pytest 依赖）：档位选择 + 内容框。

锁定 2026-11-26 起的口径：
  * 同一画作同时有 1x / `@2x` 时默认选 `@2x`（客户端高 DPI 资源）；`use_2x=False`
    显式回退 1x；`@2x` 缺失时仍回退 1x（不因缺高清档丢图）。
  * `crops.json` 逐车记录 `canvas_w/canvas_h`——两种档位的画布尺寸可以不同
    （方形 128×128/256×256、紧裁画布），消费方不得写死 256×128。

运行：python tools/test_export_tank_icons.py
"""
from __future__ import annotations

import json
import pathlib
import re
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import export_tank_icons as E  # noqa: E402


def _entry(name: str, sub: str = "BigTankIcons") -> dict:
    return {"file": name, "sub": sub, "x2": "@2x" in name,
            "skin": bool(re.search(r"_skin\d*(@2x)?\.packed", name)), "derived": False}


def _index(files: list[str]) -> dict:
    index: dict = {"key": {}, "name": {}}
    for name in files:
        key = E.icon_key(name)
        ent = _entry(name)
        index["key"].setdefault(key, []).append(ent)
        index["name"].setdefault(E.base_name(name), []).append(ent)
    return index


def test_prefers_2x_when_present() -> None:
    index = _index(["ussr-T-34.packed.webp.dvpl", "ussr-T-34@2x.packed.webp.dvpl"])
    declared = [("iconPath", "ussr-T-34")]
    picked = E.pick_icon(index, "T-34", "", True, False, declared)
    assert picked is not None and picked["file"] == "ussr-T-34@2x.packed.webp.dvpl", picked
    assert E.pick_icon(index, "T-34", "", False, False, declared)["file"] == (
        "ussr-T-34.packed.webp.dvpl")


def test_2x_missing_falls_back_to_1x() -> None:
    index = _index(["ussr-IS-7.packed.webp.dvpl"])
    picked = E.pick_icon(index, "IS-7", "", True, False, [("iconPath", "ussr-IS-7")])
    assert picked is not None and picked["file"] == "ussr-IS-7.packed.webp.dvpl"


def test_build_index_registers_both_tiers_under_one_name() -> None:
    with tempfile.TemporaryDirectory() as td:
        ui = pathlib.Path(td) / "Gfx" / "UI"
        big = ui / "BigTankIcons"
        big.mkdir(parents=True)
        for name in ("china-Type59.packed.webp.dvpl", "china-Type59@2x.packed.webp.dvpl"):
            (big / name).write_bytes(b"")
        index = E.build_index(ui)
        entries = index["name"]["china-Type59"]
        assert len(entries) == 2, entries
        assert sorted(e["x2"] for e in entries) == [False, True]


def test_emit_crops_records_each_icon_own_canvas() -> None:
    from PIL import Image

    with tempfile.TemporaryDirectory() as td:
        out = pathlib.Path(td) / "icons"
        out.mkdir()
        # 256×128 常规画布；512×256 紧裁画布；内容框位置各不相同。
        for tid, size, box in (("1", (256, 128), (10, 5, 110, 85)),
                               ("2", (512, 256), (20, 10, 220, 170))):
            im = Image.new("RGBA", size, (0, 0, 0, 0))
            im.paste((200, 30, 30, 255), box)
            im.save(out / f"{tid}.webp", lossless=True, quality=100)
        crops = pathlib.Path(td) / "crops.json"
        assert E.emit_crops(out, crops) == 0
        doc = json.loads(crops.read_text(encoding="utf-8"))
        assert doc["icons"]["1"] == {"canvas_w": 256, "canvas_h": 128,
                                     "x": 10, "y": 5, "w": 100, "h": 80}
        assert doc["icons"]["2"] == {"canvas_w": 512, "canvas_h": 256,
                                     "x": 20, "y": 10, "w": 200, "h": 160}


def main() -> None:
    test_prefers_2x_when_present()
    test_2x_missing_falls_back_to_1x()
    test_build_index_registers_both_tiers_under_one_name()
    test_emit_crops_records_each_icon_own_canvas()
    print("export tank icons tests: PASS")


if __name__ == "__main__":
    main()