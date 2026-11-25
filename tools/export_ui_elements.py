#!/usr/bin/env python3
"""导出客户端 UI「单图」元素（国家旗帜、勋章、技能图标……）：DVPL 剥壳即裸 webp。

**不需要重编码**：本机客户端这些资源都是 `*.packed.webp.dvpl`，DVPL 壳里就是
RIFF/WEBP 裸字节（VP8X，含 alpha），校验 CRC/长度后原样落盘。图集（`texture0.packed.webp.dvpl`
+ `*.txt.dvpl` 切片描述，如 `InventoryIcons/`、`DamagedModules/`）不是单图，**不在本工具
范围**——需要切片的消费方另走图集解析（见 docs/game-data-sources.md §2.6）。

资源组（显式白名单，不做目录猜测）：

  * `flags`        `Gfx/Lobby/flags`         国家/子国家旗帜（9 国 + 7 子国家 + gold；
                                              9 种角色前缀，UI 绑定见下）
  * `medals`       `Gfx/Lobby/medals`        勋章/奖励图（180 个资源）
  * `crew-skills`  `Gfx/UI/CrewSkillIcons`   乘员技能图标
  * `ribbons`      `Gfx/UI/RibbonsLobby`     战斗结果缎带
  * `tournament`   `Gfx/UI/Tournament`       锦标赛界面元素（含 markOfMastery 1–3）
  * `lobby-icons`  `Gfx/Lobby/icons`         大厅通用图标（货币/设置/强化树等，递归含子目录）

旗帜的客户端消费点（实测 `Data/UI` 全量 3473 份 dvpl 扫描）：

  * `UI/Screens3/Lobby/Common/TankInfo/NationFlag.yaml.dvpl` →
    `~res:/Gfx/Lobby/flags/flag_text_<sprite>`（8 国 + 7 子国家 + other）；
  * `UI/Screens3/Lobby/Common/FlagButton.style.yaml.dvpl`（国家选择按钮，`bg-frame` 0–7）；
  * 其余 `flag_{carousel,event,filter,notification,profile-stat,tank-node,tank-node_I,
    tank-tree,tutor-tank}_*` 由各界面 yaml 直接按名引用（科技树/训练/通知/个人战绩等）。

输出布局（`--out` 默认 `data/cache/ui`，gitignored）：

    <out>/<group>/<客户端相对路径，去掉 .packed.webp.dvpl>.webp
    <out>/_export_status.json    # total/done/unchanged/failed/finished + 逐文件
                                 # src/out/bytes/sha256/width/height

`@2x` 变体是客户端独立资源，按原名保留（`foo@2x.webp`）。同输入同字节：已存在且内容
一致时不重写（`unchanged` 计数）。

用法：
    python tools/export_ui_elements.py --all
    python tools/export_ui_elements.py --group flags --group medals
    python tools/export_ui_elements.py --group flags --list          # 只打印解析结果
    python tools/export_ui_elements.py --all --data-root <Data 根> --out <目录>
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import sys
import time

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

from wotb_sc2 import decode_dvpl  # noqa: E402
from dlc_packs import client_path, packs_dir  # noqa: E402

DEFAULT_OUT = TOOLS_DIR.parent / "data" / "cache" / "ui"

GAME_DIR_CANDIDATES = [
    "D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
    "C:/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
    "/mnt/c/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
    "/mnt/d/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
]

GROUP_DIRS = {
    "flags": "Gfx/Lobby/flags",
    "medals": "Gfx/Lobby/medals",
    "crew-skills": "Gfx/UI/CrewSkillIcons",
    "ribbons": "Gfx/UI/RibbonsLobby",
    "tournament": "Gfx/UI/Tournament",
    "lobby-icons": "Gfx/Lobby/icons",
}

SUFFIX = ".packed.webp.dvpl"


class UiAssetError(ValueError):
    """资源不是可原样落盘的裸 webp，或尺寸头不可解析（fail-closed）。"""


def webp_dimensions(data: bytes) -> tuple[int, int]:
    """从裸 webp 头解析 (width, height)；不认识/截断即抛错，不猜。"""
    if len(data) < 30 or data[:4] != b"RIFF" or data[8:12] != b"WEBP":
        raise UiAssetError("not a RIFF/WEBP payload")
    riff_size = int.from_bytes(data[4:8], "little")
    if riff_size + 8 != len(data):
        raise UiAssetError(f"RIFF size mismatch: header={riff_size + 8}, actual={len(data)}")
    fourcc = data[12:16]
    if fourcc == b"VP8X":
        width = 1 + int.from_bytes(data[24:27], "little")
        height = 1 + int.from_bytes(data[27:30], "little")
    elif fourcc == b"VP8 ":
        if data[23:26] != b"\x9d\x01\x2a":
            raise UiAssetError("VP8 start code missing")
        width = int.from_bytes(data[26:28], "little") & 0x3FFF
        height = int.from_bytes(data[28:30], "little") & 0x3FFF
    elif fourcc == b"VP8L":
        if data[20] != 0x2F:
            raise UiAssetError("VP8L signature missing")
        bits = int.from_bytes(data[21:25], "little")
        width = (bits & 0x3FFF) + 1
        height = ((bits >> 14) & 0x3FFF) + 1
    else:
        raise UiAssetError(f"unsupported WEBP chunk {fourcc!r}")
    if width <= 0 or height <= 0:
        raise UiAssetError(f"invalid dimensions {width}x{height}")
    return width, height


def find_data_root(explicit: str | None) -> pathlib.Path:
    if explicit:
        root = pathlib.Path(explicit)
        if not root.is_dir():
            raise SystemExit(f"--data-root 不是目录: {root}")
        return root
    for candidate in GAME_DIR_CANDIDATES:
        root = pathlib.Path(candidate)
        if root.is_dir():
            return root
    raise SystemExit(
        "未找到 WoT Blitz 客户端 Data 目录；用 --data-root 指定（候选见 GAME_DIR_CANDIDATES）"
    )


def client_version(data_root: pathlib.Path) -> str | None:
    version_file = data_root / "version.txt.dvpl"
    if not version_file.is_file():
        return None
    try:
        return decode_dvpl(version_file.read_bytes()).decode("utf-8", "replace").strip()
    except Exception:
        return None


def collect(group: str, data_root: pathlib.Path) -> list[str]:
    """返回组内逻辑相对路径（`/` 分隔）；Data 与 DLC 覆盖层的并集。

    DLC 微更新可能**覆盖**同名文件或**新增**文件（见 docs/game-data-sources.md §5.4），
    因此枚举必须同时扫描 `packs/<组目录>`，不能只看 `Data/`。
    """
    rels: set[str] = set()
    base = data_root / GROUP_DIRS[group]
    if base.is_dir():
        rels.update(p.relative_to(base).as_posix() for p in base.rglob(f"*{SUFFIX}"))
    packs = packs_dir()
    if packs is not None:
        overlay = packs / GROUP_DIRS[group]
        if overlay.is_dir():
            rels.update(p.relative_to(overlay).as_posix() for p in overlay.rglob(f"*{SUFFIX}"))
    if not rels:
        raise SystemExit(f"资源目录为空（Data 与 DLC 覆盖层均无）: {GROUP_DIRS[group]}")
    return sorted(rels)


def source_path(data_root: pathlib.Path, group: str, rel: str) -> pathlib.Path:
    """客户端资源路径：`packs/<rel>` 优先，缺失回退 `Data/<rel>`（DLC 覆盖层契约）。"""
    return client_path(data_root, f"{GROUP_DIRS[group]}/{rel}")


def out_relative(group: str, rel: str) -> pathlib.Path:
    return pathlib.Path(group) / (rel[: -len(SUFFIX)] + ".webp")


def _sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _write_file(path: pathlib.Path, data: bytes) -> str:
    """原子写；内容相同不重写。返回 done / unchanged。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_file() and path.read_bytes() == data:
        return "unchanged"
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_bytes(data)
    os.replace(tmp, path)
    return "done"


def _write_status(path: pathlib.Path, state: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    state["updated_at"] = time.strftime("%H:%M:%S")
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, ensure_ascii=False, indent=1), encoding="utf-8")
    tmp.replace(path)


def run(groups: list[str], data_root: pathlib.Path, out: pathlib.Path,
        list_only: bool) -> int:
    state: dict = {
        "client_version": client_version(data_root),
        "data_root": data_root.as_posix(),
        "groups": {},
        "total": 0,
        "done": 0,
        "unchanged": 0,
        "failed": 0,
        "finished": False,
    }
    status_path = out / "_export_status.json"
    for group in groups:
        entries: list[dict] = []
        failure_reasons: list[str] = []
        counts = {"done": 0, "unchanged": 0, "failed": 0}
        for rel in collect(group, data_root):
            rel_src = f"{GROUP_DIRS[group]}/{rel}"
            rel_out = out_relative(group, rel)
            src = source_path(data_root, group, rel)
            overlay = packs_dir() is not None and src.is_relative_to(packs_dir())
            try:
                data = decode_dvpl(src.read_bytes())
                width, height = webp_dimensions(data)
                base_entry = {"src": rel_src, "out": rel_out.as_posix()}
                if overlay:
                    base_entry["overlay"] = True
                if list_only:
                    base_entry.update({"bytes": len(data), "width": width, "height": height})
                    entries.append(base_entry)
                    print(f"{width}x{height}  {rel_src}" + ("  [packs]" if overlay else ""))
                    continue
                action = _write_file(out / rel_out, data)
                counts[action] += 1
                base_entry.update({
                    "bytes": len(data),
                    "sha256": hashlib.sha256(data).hexdigest(),
                    "width": width,
                    "height": height,
                })
                entries.append(base_entry)
            except Exception as exc:  # fail-closed：记录并继续，末尾以非零退出
                counts["failed"] += 1
                failure_reasons.append(f"{rel_src}: {exc}")
                entries.append({"src": rel_src, "out": rel_out.as_posix(),
                                "error": str(exc)})
        group_state = {
            "dir": GROUP_DIRS[group],
            "files": len(entries),
            "done": counts["done"],
            "unchanged": counts["unchanged"],
            "failed": counts["failed"],
            "entries": entries,
        }
        if failure_reasons:
            group_state["errors"] = failure_reasons
        state["groups"][group] = group_state
        state["total"] += len(entries)
        state["done"] += counts["done"]
        state["unchanged"] += counts["unchanged"]
        state["failed"] += counts["failed"]
        if not list_only:
            _write_status(status_path, state)  # 逐组落盘，中断可续查
        print(f"== {group}: files={len(entries)} done={counts['done']} "
              f"unchanged={counts['unchanged']} failed={counts['failed']}")
    state["finished"] = True
    if not list_only:
        _write_status(status_path, state)
        print(f"status: {status_path}")
    if state["failed"]:
        print(f"FAIL: {state['failed']} 个资源未导出（见 _export_status.json 的 errors）",
              file=sys.stderr)
        return 1
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="导出客户端 UI 单图元素（DVPL 剥壳裸 webp）",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="资源组: " + ", ".join(sorted(GROUP_DIRS)),
    )
    parser.add_argument("--group", action="append", choices=sorted(GROUP_DIRS),
                        help="资源组（可重复）；默认 flags")
    parser.add_argument("--all", action="store_true", help="全部资源组")
    parser.add_argument("--data-root", help="客户端 Data 目录（默认自动探测）")
    parser.add_argument("--out", default=str(DEFAULT_OUT), help="输出目录")
    parser.add_argument("--list", action="store_true", help="只打印解析结果，不写盘")
    args = parser.parse_args(argv)

    groups = sorted(GROUP_DIRS) if args.all else (args.group or ["flags"])
    data_root = find_data_root(args.data_root)
    return run(groups, data_root, pathlib.Path(args.out), args.list)


if __name__ == "__main__":
    raise SystemExit(main())