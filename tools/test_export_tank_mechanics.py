#!/usr/bin/env python3
"""`tools/export_tank_mechanics.py` 的独立回归测试（无 pytest 依赖）。

覆盖两层：
1. 纯解析/规范化：trayShell 结构 → ParamName1..10；富文本清理；机制枚举 → 文案键；
2. 已提交数据锚定：`data/tank_mechanics.json` 的 15 种机制、三语文案与 6 辆 TrayShell
   车的分段/参数必须与客户端语义一致（数值变化时测试失败，强制显式复核）。
"""
from __future__ import annotations

import json
import pathlib
import sys
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import export_tank_mechanics as M  # noqa: E402

TRAY_XML = """
<root>
  <turrets0>
    <Turret>
      <guns>
        <_107mm_Gun_M_75K>shared<maxAmmo>60</maxAmmo>
          <extras>
            <trayShell>
              <kinds>ARMOR_PIERCING_CR</kinds>
              <sectors>
                <sector><id>1</id><distance>50</distance><factor>1.4</factor></sector>
                <sector><id>2</id><distance>120</distance><factor>1.2</factor></sector>
                <sector><id>3</id><distance>200</distance><factor>1.0</factor></sector>
                <sector><id>4</id><distance>720</distance><factor>0.8</factor></sector>
              </sectors>
            </trayShell>
          </extras>
        </_107mm_Gun_M_75K>
      </guns>
    </Turret>
  </turrets0>
</root>
"""


def test_params_from_sectors() -> None:
    sectors = [
        {"distance": 50.0, "factor": 1.4},
        {"distance": 120.0, "factor": 1.2},
        {"distance": 200.0, "factor": 1.0},
        {"distance": 720.0, "factor": 0.8},
    ]
    assert M.params_from_sectors(sectors) == {
        "ParamName1": "50",
        "ParamName2": "140",
        "ParamName3": "50",
        "ParamName4": "120",
        "ParamName5": "120",
        "ParamName6": "120",
        "ParamName7": "200",
        "ParamName8": "100",
        "ParamName9": "200",
        "ParamName10": "80",
    }
    # 非整数边界保留小数（不猜整）
    assert M.params_from_sectors([
        {"distance": 50.5, "factor": 1.25},
        {"distance": 120.0, "factor": 1.0},
        {"distance": 200.0, "factor": 1.0},
        {"distance": 720.0, "factor": 0.8},
    ])["ParamName2"] == "125"


def test_parse_tray_shell_root() -> None:
    spec = M.parse_tray_shell_root(ET.fromstring(TRAY_XML), "fixture.xml")
    assert spec == {
        "kinds": ["ARMOR_PIERCING_CR"],
        "sectors": [
            {"distance": 50.0, "factor": 1.4},
            {"distance": 120.0, "factor": 1.2},
            {"distance": 200.0, "factor": 1.0},
            {"distance": 720.0, "factor": 0.8},
        ],
    }
    assert M.parse_tray_shell_root(ET.fromstring("<root/>"), "empty.xml") is None

    def expect_error(xml: str, needle: str) -> None:
        try:
            M.parse_tray_shell_root(ET.fromstring(xml), "bad.xml")
        except M.ExportError as exc:
            assert needle in str(exc), str(exc)
        else:
            raise AssertionError(f"应 fail-closed：{needle}")

    # 段数不是 4：文案模板固定 4 段
    expect_error(TRAY_XML.replace("<sector><id>4</id><distance>720</distance><factor>0.8</factor></sector>", ""),
                 "分段数")
    # 距离不递增
    expect_error(TRAY_XML.replace("<distance>120</distance>", "<distance>40</distance>"), "严格递增")
    # 未知 kind
    expect_error(TRAY_XML.replace("ARMOR_PIERCING_CR", "LASER"), "未知 trayShell kind")
    # 缺 kinds
    expect_error(TRAY_XML.replace("<kinds>ARMOR_PIERCING_CR</kinds>", ""), "缺 <kinds>")
    # 多炮副本定义不一致
    dup = TRAY_XML.replace(
        "</trayShell>",
        "</trayShell>\n<trayShell><kinds>ARMOR_PIERCING_CR</kinds><sectors>"
        "<sector><id>1</id><distance>50</distance><factor>1.5</factor></sector>"
        "<sector><id>2</id><distance>120</distance><factor>1.2</factor></sector>"
        "<sector><id>3</id><distance>200</distance><factor>1.0</factor></sector>"
        "<sector><id>4</id><distance>720</distance><factor>0.8</factor></sector>"
        "</sectors></trayShell>",
        1,
    )
    expect_error(dup, "定义不一致")


def test_normalize_localized_text() -> None:
    assert M.normalize_localized_text("%(highlight_start)+50%%%(highlight_end)") == "+50%"
    assert M.normalize_localized_text("x %(ParamName1) m") == "x %(ParamName1) m"
    try:
        M.normalize_localized_text("bad %(other) token")
    except M.ExportError:
        pass
    else:
        raise AssertionError("未知占位符应 fail-closed")


def test_mechanic_text_table() -> None:
    assert len(M.MECHANIC_TEXT_NAME) == 15
    assert M.MECHANIC_TEXT_NAME["AlteredSpotDuration"] == "ReducedSpotDuration"
    assert M.MECHANIC_TEXT_NAME["TrayShell"] == "TrayShell"


def test_committed_data() -> None:
    path = ROOT / "data" / "tank_mechanics.json"
    data = json.loads(path.read_text(encoding="utf-8"))
    meta = data["meta"]
    assert meta["languages"] == ["en", "zh-Hans", "ru"]
    assert meta["mechanicCount"] == 15
    assert meta["tanksWithMechanics"] > 0
    assert not any(":\\" in str(v) or str(v).startswith("/") for v in meta["sources"]["textProvenance"].values()), \
        "provenance 不应记录本机绝对路径"

    # 15 种机制 × 三语标题/说明齐全，且高亮标记已清理
    assert set(data["mechanics"]) == set(M.MECHANIC_TEXT_NAME)
    for name, row in data["mechanics"].items():
        for lang in ("en", "zh-Hans", "ru"):
            title = row["title"][lang]
            desc = row["description"][lang]
            assert title.strip() and desc.strip(), (name, lang)
            assert "%(highlight" not in title + desc, (name, lang)

    # TrayShell 六辆（波兰 TD 五辆 + KV-4 KTTS）与分段值锚定
    tray_ids = {
        tid for tid, tank in data["tanks"].items()
        if any(m["name"] == "TrayShell" for m in tank["mechanics"])
    }
    assert tray_ids == {"19073", "19329", "19585", "19841", "23169", "28417"}
    kv4 = data["tanks"]["28417"]["mechanics"][0]
    assert kv4["name"] == "TrayShell"
    assert kv4["kinds"] == ["ARMOR_PIERCING_CR"]
    assert kv4["sectors"] == [
        {"distance": 50.0, "factor": 1.4},
        {"distance": 120.0, "factor": 1.2},
        {"distance": 200.0, "factor": 1.0},
        {"distance": 720.0, "factor": 0.8},
    ]
    assert M.params_from_sectors(kv4["sectors"]) == kv4["localizationValues"]

    # 其它机制也要有逐车清单（不能只导出 TrayShell）
    names = {m["name"] for tank in data["tanks"].values() for m in tank["mechanics"]}
    assert {"ImprovedDetection", "AlteredSpotDuration", "LTCamo", "DoubleShot", "ATGM"} <= names


def main() -> int:
    test_params_from_sectors()
    test_parse_tray_shell_root()
    test_normalize_localized_text()
    test_mechanic_text_table()
    test_committed_data()
    print("export_tank_mechanics: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())