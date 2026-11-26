#!/usr/bin/env python3
"""导出逐车「车辆机制」清单与官方机制文案（TankMechanics）。

数据权威（全部来自本机客户端磁盘文件，不猜、不按内容统计）：

1. **逐车机制清单** = `Data/3d/Tanks/Parameters/<nation>/<stem>.yaml.dvpl` 的
   `mechanics:` 流式列表。客户端 hangar 的 `VehicleParamsPanel` 直接消费
   `vector<eGameplayMechanics>`；TrayShell 等机制的距离/倍率参数也在这里的
   `localizationValues`（ParamName1..10，客户端已算好）。
2. **TrayShell 结构参数** = 车辆 XML `<extras><trayShell>` 的 `<kinds>` /
   `<sectors><sector>{id,distance,factor}`；导出器把 XML 结构折算成同一组
   ParamName1..10 并与 Parameters 的 `localizationValues` 逐字比对，不一致即
   fail-closed（两处都是客户端数据，出现分歧说明解析或版本不匹配）。
3. **机制文案** = `Data/Strings/<lang>.yaml.dvpl` 的 `TankMechanics/<name>` /
   `...Description`，叠加运行时本地化覆盖层（客户端启动后从
   `https://stufficons.wgcdn.co/localizations/<lang>.yaml` 下载到
   `%LOCALAPPDATA%/wotblitz/DAVAProject/cache/localizations/`）。基础 Strings
   缺 MovableArmorDescription / TrayShellDescription，覆盖层补齐；任何语言缺
   标题或说明即 fail-closed（不用别的语言兜底）。

输出：`data/tank_mechanics.json`（随资产包分发为 `data/tankMechanics.json`）。

用法：
    python tools/export_tank_mechanics.py
    python tools/export_tank_mechanics.py --localizations-dir /path/to/overlay
    python tools/export_tank_mechanics.py --game-data <Data 根> --bridge <桥表>
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import sys
import urllib.request
from datetime import datetime, timezone

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
sys.path.insert(0, str(TOOLS_DIR))
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

from extract_vehicles import default_game_data  # noqa: E402
from wotb_sc2 import decode_dvpl  # noqa: E402

LANGUAGES = ("en", "zh-Hans", "ru")

# eGameplayMechanics 枚举名 → 客户端字符串键名。
# 依据 = 客户端 UI `TankMechanicsOverlay.yaml` / `TankMechanicsButton.yaml` 的
# `mechanicsTitle` 映射：只有 AlteredSpotDuration 的文案键名是 ReducedSpotDuration，
# 其余同名。新机制不在表内即 fail-closed（强制显式复核文案键）。
MECHANIC_TEXT_NAME = {
    "ImprovedDetection": "ImprovedDetection",
    "MovableArmor": "MovableArmor",
    "InternalTrack": "InternalTrack",
    "DesertPower": "DesertPower",
    "ReserveAmmo": "ReserveAmmo",
    "AlteredSpotDuration": "ReducedSpotDuration",
    "ShotDispersionStabilizator": "ShotDispersionStabilizator",
    "DraculaTrack": "DraculaTrack",
    "NotDamageModuls": "NotDamageModuls",
    "WithoutCrew": "WithoutCrew",
    "DoubleShot": "DoubleShot",
    "TripleShot": "TripleShot",
    "LTCamo": "LTCamo",
    "ATGM": "ATGM",
    "TrayShell": "TrayShell",
}

# 弹种 kind 闭集（客户端 shells.xml `<kind>`；与 tools/extract_vehicles.py 的 KIND_ID 同源）。
KIND_NAMES = {
    "ARMOR_PIERCING",
    "ARMOR_PIERCING_CR",
    "HOLLOW_CHARGE",
    "HIGH_EXPLOSIVE",
}

# TrayShell 文案模板固定 4 段（ParamName1..10）。客户端当前全量 6 辆都是 4 段；
# 出现别的段数说明模板/结构变了，必须显式扩展文案与导出器，不能猜。
TRAY_SHELL_SECTOR_COUNT = 4

OVERLAY_DIRS = (
    "%LOCALAPPDATA%/wotblitz/DAVAProject/cache/localizations",
    "~/wotblitz/DAVAProject/cache/localizations",
    "%LOCALAPPDATA%/wotblitz/DAVAProject/cache/dynamicContentLocalizations",
    "~/wotblitz/DAVAProject/cache/dynamicContentLocalizations",
)

OVERLAY_URL = "https://stufficons.wgcdn.co/localizations/{lang}.yaml"


class ExportError(RuntimeError):
    pass


def safe_load_yaml(text: str):
    """延迟导入 PyYAML：纯解析/已提交数据锚定测试不依赖该运行时依赖。"""
    try:
        import yaml
    except ImportError as exc:  # pragma: no cover - 环境缺少可选依赖
        raise ExportError("缺少 PyYAML：python -m pip install pyyaml") from exc
    return yaml.safe_load(text)


def load_bridge(path: pathlib.Path) -> dict[str, dict]:
    table = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(table, dict):
        raise ExportError(f"{path} 不是对象")
    return table


def stem_index(bridge: dict[str, dict]) -> dict[str, dict]:
    """stem → {tankId, nation, slug}。同一 stem 多 id 视为歧义，fail-closed。"""
    out: dict[str, dict] = {}
    for tid, row in bridge.items():
        stem = (row or {}).get("stem")
        if not stem:
            continue
        if stem in out:
            raise ExportError(f"桥表中 stem 重复：{stem}（{out[stem]['tankId']} / {tid}）")
        out[stem] = {"tankId": int(tid), "nation": row.get("nation", ""), "slug": row.get("slug", "")}
    return out


def decode_text(path: pathlib.Path) -> str:
    return decode_dvpl(path.read_bytes()).decode("utf-8", "replace")


def game_version(game_data: pathlib.Path) -> str | None:
    p = game_data / "version.txt.dvpl"
    if not p.is_file():
        return None
    try:
        return decode_text(p).strip().splitlines()[0].strip() or None
    except Exception:  # noqa: BLE001
        return None


def load_flat_strings(path: pathlib.Path) -> dict[str, str]:
    if not path.is_file():
        raise ExportError(f"缺少字符串表：{path}")
    text = decode_text(path)
    try:
        doc = safe_load_yaml(text)
    except Exception as exc:  # noqa: BLE001
        raise ExportError(f"{path} 不是合法 YAML：{exc}") from exc
    if not isinstance(doc, dict):
        raise ExportError(f"{path} 顶层不是映射")
    return {str(k): str(v) for k, v in doc.items() if isinstance(v, (str, int, float))}


def overlay_paths(lang: str, localizations_dir: pathlib.Path | None) -> list[pathlib.Path]:
    out: list[pathlib.Path] = []
    if localizations_dir is not None:
        out.append(localizations_dir / f"{lang}.yaml")
    localappdata = os.environ.get("LOCALAPPDATA")
    for raw in OVERLAY_DIRS:
        root = raw
        if "%LOCALAPPDATA%" in raw:
            if not localappdata:
                continue
            root = raw.replace("%LOCALAPPDATA%", localappdata)
        out.append(pathlib.Path(os.path.expanduser(os.path.expandvars(root))) / f"{lang}.yaml")
    # 去重保序
    seen: set[str] = set()
    uniq: list[pathlib.Path] = []
    for p in out:
        key = str(p)
        if key not in seen:
            seen.add(key)
            uniq.append(p)
    return uniq


def fetch_overlays(dest: pathlib.Path) -> None:
    """下载三语运行时本地化覆盖层到 dest（显式 `--fetch-localizations` 才联网）。"""
    dest.mkdir(parents=True, exist_ok=True)
    for lang in LANGUAGES:
        p = dest / f"{lang}.yaml"
        if p.is_file():
            continue
        url = OVERLAY_URL.format(lang=lang)
        print(f"下载本地化覆盖层：{url} → {p}")
        try:
            with urllib.request.urlopen(url, timeout=60) as resp:  # noqa: S310（固定官方 CDN）
                p.write_bytes(resp.read())
        except Exception as exc:  # noqa: BLE001
            raise ExportError(f"下载 {url} 失败：{exc}") from exc


def load_overlay(lang: str, localizations_dir: pathlib.Path | None) -> tuple[dict[str, str], dict | None]:
    for p in overlay_paths(lang, localizations_dir):
        if not p.is_file():
            continue
        raw = p.read_bytes()
        try:
            doc = safe_load_yaml(raw.decode("utf-8", "replace"))
        except Exception as exc:  # noqa: BLE001
            raise ExportError(f"本地化覆盖层不是合法 YAML：{p}：{exc}") from exc
        if not isinstance(doc, dict):
            raise ExportError(f"本地化覆盖层顶层不是映射：{p}")
        return (
            {str(k): str(v) for k, v in doc.items() if isinstance(v, str)},
            # 客户端缓存的来源是公开 CDN；只记录来源与字节指纹，不落本机绝对路径。
            {"source": f"https://stufficons.wgcdn.co/localizations/{lang}.yaml",
             "sha256": sha256_bytes(raw)},
        )
    return {}, None


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def normalize_localized_text(value: str) -> str:
    """客户端富文本 → 可渲染纯文本：去高亮标记、还原 `%%`，保留 ParamName 占位符。"""
    text = value.replace("%(highlight_start)", "").replace("%(highlight_end)", "")
    text = text.replace("%%", "%")
    for token in re.findall(r"%\(([^)]*)\)", text):
        if not re.fullmatch(r"ParamName\d+", token):
            raise ExportError(f"未知文案占位符 %（{token}）")
    return text


def fmt_number(value: float) -> str:
    """与客户端 localizationValues 同形：整数不带小数，非整数去尾零。"""
    if abs(value - round(value)) < 1e-9:
        return str(int(round(value)))
    return f"{value:.6f}".rstrip("0").rstrip(".")


def params_from_sectors(sectors: list[dict]) -> dict[str, str]:
    d = [s["distance"] for s in sectors]
    f = [s["factor"] for s in sectors]
    if len(sectors) != TRAY_SHELL_SECTOR_COUNT:
        raise ExportError(
            f"TrayShell 分段数 {len(sectors)} != {TRAY_SHELL_SECTOR_COUNT}；"
            "文案模板固定 4 段，需同步扩展 TankMechanics/TrayShellDescription 与前端渲染"
        )
    return {
        "ParamName1": fmt_number(d[0]),
        "ParamName2": fmt_number(f[0] * 100.0),
        "ParamName3": fmt_number(d[0]),
        "ParamName4": fmt_number(d[1]),
        "ParamName5": fmt_number(f[1] * 100.0),
        "ParamName6": fmt_number(d[1]),
        "ParamName7": fmt_number(d[2]),
        "ParamName8": fmt_number(f[2] * 100.0),
        "ParamName9": fmt_number(d[2]),
        "ParamName10": fmt_number(f[3] * 100.0),
    }


def parse_tray_shell_root(root, label: str) -> dict | None:
    """车辆 XML 根元素 → 规范化 trayShell（kinds + sectors）。"""
    nodes = list(root.iter("trayShell"))
    if not nodes:
        return None
    specs: list[dict] = []
    for node in nodes:
        kinds: list[str] = []
        for kinds_el in node.findall("kinds"):
            for token in re.split(r"[\s,]+", (kinds_el.text or "").strip()):
                if token:
                    if token not in KIND_NAMES:
                        raise ExportError(f"{label}: 未知 trayShell kind {token}")
                    if token not in kinds:
                        kinds.append(token)
        sectors: list[dict] = []
        for sec in node.findall("./sectors/sector"):
            try:
                sid = int((sec.findtext("id") or "").strip())
                dist = float((sec.findtext("distance") or "").strip())
                factor = float((sec.findtext("factor") or "").strip())
            except ValueError as exc:
                raise ExportError(f"{label}: trayShell sector 数值不可解析：{exc}") from exc
            if not (factor > 0.0):
                raise ExportError(f"{label}: trayShell factor 必须 > 0（id={sid}）")
            sectors.append({"id": sid, "distance": dist, "factor": factor})
        if not kinds:
            raise ExportError(f"{label}: trayShell 缺 <kinds>")
        if len(sectors) != TRAY_SHELL_SECTOR_COUNT:
            raise ExportError(
                f"{label}: trayShell 分段数 {len(sectors)} != {TRAY_SHELL_SECTOR_COUNT}；"
                "文案模板固定 4 段，需同步扩展"
            )
        ids = [s["id"] for s in sectors]
        if ids != list(range(1, len(ids) + 1)):
            raise ExportError(f"{label}: trayShell sector id 不是 1..N 顺序：{ids}")
        distances = [s["distance"] for s in sectors]
        if any(b <= a for a, b in zip(distances, distances[1:])):
            raise ExportError(f"{label}: trayShell distance 必须严格递增：{distances}")
        specs.append({
            "kinds": kinds,
            "sectors": [{"distance": s["distance"], "factor": s["factor"]} for s in sectors],
        })
    # 同一辆车的所有 trayShell 定义必须一致（多炮/多炮塔副本）；否则无法在车级展示。
    first = specs[0]
    for spec in specs[1:]:
        if spec != first:
            raise ExportError(f"{label}: 多个 trayShell 定义不一致：{specs}")
    return first


def parse_tray_shell(xml_path: pathlib.Path) -> dict | None:
    """车辆 XML 的 `<extras><trayShell>` → 规范化结构（kinds + sectors）。"""
    import xml.etree.ElementTree as ET

    root = ET.fromstring(decode_dvpl(xml_path.read_bytes()))
    return parse_tray_shell_root(root, xml_path.name)


def load_mechanic_texts(game_data: pathlib.Path, localizations_dir: pathlib.Path | None) -> tuple[dict, dict]:
    texts: dict[str, dict] = {}
    provenance: dict[str, dict] = {}
    for lang in LANGUAGES:
        strings_path = game_data / "Strings" / f"{lang}.yaml.dvpl"
        base = load_flat_strings(strings_path)
        overlay, overlay_used = load_overlay(lang, localizations_dir)
        merged = dict(base)
        merged.update(overlay)
        provenance[lang] = {
            "strings": str(strings_path.relative_to(game_data)).replace("\\", "/"),
            "stringsSha256": sha256_bytes(strings_path.read_bytes()),
            "localizationOverlay": overlay_used,
        }
        for enum_name, key in MECHANIC_TEXT_NAME.items():
            title_key = f"TankMechanics/{key}"
            desc_key = f"TankMechanics/{key}Description"
            missing = [k for k in (title_key, desc_key) if k not in merged]
            if missing:
                raise ExportError(
                    f"[{lang}] 缺少机制文案 {missing}（枚举 {enum_name}）。"
                    " 基础 Strings 不含时需提供运行时本地化覆盖层："
                    "https://stufficons.wgcdn.co/localizations/{lang}.yaml"
                )
            row = texts.setdefault(enum_name, {
                "titleKey": title_key,
                "descriptionKey": desc_key,
                "title": {},
                "description": {},
            })
            row["title"][lang] = normalize_localized_text(merged[title_key])
            row["description"][lang] = normalize_localized_text(merged[desc_key])
    return texts, provenance


def export(game_data: pathlib.Path, bridge_path: pathlib.Path, localizations_dir: pathlib.Path | None) -> dict:
    bridge = load_bridge(bridge_path)
    stems = stem_index(bridge)
    parameters_root = game_data / "3d" / "Tanks" / "Parameters"
    vehicle_root = game_data / "XML" / "item_defs" / "vehicles"
    if not parameters_root.is_dir():
        raise ExportError(f"缺少 {parameters_root}")
    if not vehicle_root.is_dir():
        raise ExportError(f"缺少 {vehicle_root}")

    tanks: dict[str, dict] = {}
    skipped: list[str] = []
    parsed = 0
    tray_seen: set[str] = set()
    for path in sorted(parameters_root.rglob("*.yaml.dvpl")):
        raw = decode_text(path)
        try:
            doc = safe_load_yaml(raw)
        except Exception as exc:  # noqa: BLE001
            raise ExportError(f"{path} 不是合法 YAML：{exc}") from exc
        if not isinstance(doc, dict):
            continue
        mechanics = doc.get("mechanics") or []
        if not isinstance(mechanics, list):
            raise ExportError(f"{path}: mechanics 不是列表")
        parsed += 1
        if not mechanics:
            continue
        stem = path.name[: -len(".yaml.dvpl")]
        info = stems.get(stem)
        if info is None:
            skipped.append(stem)
            continue
        out_rows: list[dict] = []
        seen_names: set[str] = set()
        for item in mechanics:
            if not isinstance(item, dict) or not item.get("name"):
                raise ExportError(f"{path}: mechanics 条目缺少 name：{item!r}")
            name = str(item["name"])
            if name in seen_names:
                raise ExportError(f"{path}: 机制 {name} 重复声明")
            seen_names.add(name)
            if name not in MECHANIC_TEXT_NAME:
                raise ExportError(
                    f"{path}: 未知机制 {name}（文案键未登记；请在 MECHANIC_TEXT_NAME 显式补表）"
                )
            row = {"name": name}
            # 保留客户端条目的其余字段（如 DesertPower.provision）；localizationValues 单独规范。
            for k, v in item.items():
                if k in ("name", "localizationValues"):
                    continue
                row[k] = v
            values = item.get("localizationValues")
            if values is not None:
                if not isinstance(values, dict):
                    raise ExportError(f"{path}: {name}.localizationValues 不是映射")
                row["localizationValues"] = {str(k): fmt_number(float(v)) if isinstance(v, (int, float)) else str(v)
                                             for k, v in values.items()}
            if name == "TrayShell":
                xml_path = vehicle_root / info["nation"] / f"{stem}.xml.dvpl"
                if not xml_path.is_file():
                    raise ExportError(f"{path}: TrayShell 需要车辆 XML，但缺少 {xml_path}")
                spec = parse_tray_shell(xml_path)
                if spec is None:
                    raise ExportError(f"{xml_path}: Parameters 声明 TrayShell，但 XML 无 <trayShell>")
                expected = params_from_sectors(spec["sectors"])
                actual = row.get("localizationValues")
                if actual != expected:
                    raise ExportError(
                        f"{xml_path.name}: trayShell 结构折算的文案参数与客户端 localizationValues 不一致："
                        f"expected={expected} actual={actual}"
                    )
                row["kinds"] = spec["kinds"]
                row["sectors"] = spec["sectors"]
                tray_seen.add(str(info["tankId"]))
            out_rows.append(row)
        tanks[str(info["tankId"])] = {"stem": stem, "mechanics": out_rows}

    texts, text_provenance = load_mechanic_texts(game_data, localizations_dir)
    # 覆盖完整性：客户端声明的每个机制都必须有文案（当前 15/15）。
    missing_text = sorted({row["name"] for t in tanks.values() for row in t["mechanics"]} - set(texts))
    if missing_text:
        raise ExportError(f"机制文案表缺少：{missing_text}")
    # TrayShell 声明与 XML 解析必须一一对应。
    declared_tray = {tid for tid, t in tanks.items()
                     if any(m["name"] == "TrayShell" for m in t["mechanics"])}
    if declared_tray != tray_seen:
        raise ExportError(
            f"TrayShell 车级清单不一致：declared={sorted(declared_tray)} parsed={sorted(tray_seen)}"
        )

    return {
        "meta": {
            "gameVersion": game_version(game_data),
            "generatedAt": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "bridge": str(bridge_path.relative_to(REPO_ROOT)).replace("\\", "/"),
            "parametersScanned": parsed,
            "tanksWithMechanics": len(tanks),
            "skippedStems": sorted(skipped),
            "languages": list(LANGUAGES),
            "mechanicCount": len(texts),
            "sources": {
                "parameters": "3d/Tanks/Parameters/<nation>/<stem>.yaml.dvpl",
                "vehicleXml": "XML/item_defs/vehicles/<nation>/<stem>.xml.dvpl",
                "textProvenance": text_provenance,
            },
        },
        "mechanics": {name: texts[name] for name in sorted(texts)},
        "tanks": {tid: tanks[tid] for tid in sorted(tanks, key=lambda x: int(x))},
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--game-data", type=pathlib.Path, default=default_game_data())
    ap.add_argument("--bridge", type=pathlib.Path, default=REPO_ROOT / "data" / "tank_id_bridge.json")
    ap.add_argument("--localizations-dir", type=pathlib.Path, default=None,
                    help="运行时本地化覆盖层目录（<lang>.yaml）；缺省读客户端缓存")
    ap.add_argument("--fetch-localizations", action="store_true",
                    help="缺覆盖层时从 stufficons.wgcdn.co 下载到 --localizations-dir"
                         "（缺省 data/cache/localizations）")
    ap.add_argument("--out", type=pathlib.Path, default=REPO_ROOT / "data" / "tank_mechanics.json")
    args = ap.parse_args()

    try:
        localizations_dir = args.localizations_dir
        if args.fetch_localizations:
            localizations_dir = localizations_dir or (REPO_ROOT / "data" / "cache" / "localizations")
            fetch_overlays(localizations_dir)
        payload = export(args.game_data, args.bridge, localizations_dir)
    except ExportError as exc:
        print(f"导出失败：{exc}", file=sys.stderr)
        return 1
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(payload, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(
        f"车辆机制：{payload['meta']['tanksWithMechanics']} 辆 / "
        f"{payload['meta']['mechanicCount']} 种机制 → {args.out}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())