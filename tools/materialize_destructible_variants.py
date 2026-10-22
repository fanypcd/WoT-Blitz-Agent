#!/usr/bin/env python3
"""为可破坏物映射**确有差异**的地图变体物化独立 `map/<key>/` 目录。

背景（2026-10-22）：多变体 space 的 lka 是**每变体一张**分段表（`<stem>.<前缀><序号>.lka`），
同一 space 的各 key 在 `(cell,slot) → 实体` 上可能不同（rudniki/amigosville/canal/neptune/
medvedkovo 实测）。打包器按 space 只物化首个 key 的目录、index 把所有变体 id 指向它，
会让变体局（如 Mines - Hill）的树倒事件联到基础变体的物体。

本工具在**完整打包链之后**运行（composite/2D 已完成，overhead 底图已烘进基础目录）：
  - 对同 space 的每个非基础 key，比较 `instances`（含 serverId 的完整实例表）；
  - 相同 → 不物化，index 继续指向基础 key；
  - 不同 → `map/<key>/` 用**硬链接**指向基础目录的全部文件，只写入本变体的
    `destructibles.json`，并把 index 的该 map_id 指向本 key。
硬链接失败（跨卷/文件系统不支持）时回退复制；共享资产字节仍一致，manifest 自洽。

用法（在打包链末端、`--refresh-manifest` 之前）：
  python tools/materialize_destructible_variants.py \
      --pack release/asset_pack --map-index map_index.json
  python scripts/export_asset_pack.py --out release/asset_pack --refresh-manifest
"""
import argparse
import json
import os
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent


def _load_json(p: Path) -> dict:
    return json.loads(p.read_text(encoding="utf-8"))


def _instances_signature(doc: dict) -> list[dict]:
    """实例表比对口径：忽略 mapId/key/display/lkaVariant 等文档级身份字段。"""
    return sorted((doc.get("instances") or []), key=lambda i: (i.get("id") is None,
                                                               i.get("id") or 0))


def _same_mapping(a: dict, b: dict) -> bool:
    if _instances_signature(a) != _instances_signature(b):
        return False

    def rev(doc):
        out = []
        for i in doc.get("instances") or []:
            srv = i.get("serverId")
            if srv:
                out.append((tuple(srv["cell"]), srv["slot"], i.get("id")))
        return sorted(out)

    return rev(a) == rev(b)


def _link_or_copy(src: Path, dst: Path) -> str:
    dst.parent.mkdir(parents=True, exist_ok=True)
    try:
        os.link(src, dst)
        return "link"
    except OSError:
        shutil.copy2(src, dst)
        return "copy"


def materialize(pack: Path, map_index: Path, cache_maps: Path, dry_run: bool) -> int:
    maps = _load_json(map_index)
    base_by_space: dict[str, str] = {}
    groups: dict[str, list[dict]] = {}
    for e in maps:
        groups.setdefault(e["space"], []).append(e)
    for space, entries in groups.items():
        existing = [e["key"] for e in entries if (pack / "map" / e["key"]).is_dir()]
        if len(existing) > 1:
            print(f"!! {space}: 多个已存在目录 {existing}，打包器状态不干净", file=sys.stderr)
            return 2
        if existing:
            base_by_space[space] = existing[0]

    index_path = pack / "index.json"
    index = _load_json(index_path)
    overlays = []
    skipped = []
    for space, entries in groups.items():
        base_key = base_by_space.get(space)
        if base_key is None:
            continue
        base_doc_path = cache_maps / base_key / "destructibles.json"
        if not base_doc_path.is_file():
            print(f"!! {base_key}: 缺 destructibles.json", file=sys.stderr)
            return 2
        base_doc = _load_json(base_doc_path)
        base_dir = pack / "map" / base_key
        for e in entries:
            key = e["key"]
            if key == base_key:
                continue
            var_doc_path = cache_maps / key / "destructibles.json"
            if not var_doc_path.is_file():
                print(f"!! {key}: 缺 destructibles.json（先跑 export_map_destructibles.py）",
                      file=sys.stderr)
                return 2
            var_doc = _load_json(var_doc_path)
            mid = str(e["map_id"])
            if _same_mapping(base_doc, var_doc):
                index["maps"][mid]["key"] = base_key
                skipped.append(key)
                continue
            dst_dir = pack / "map" / key
            if dst_dir.exists():
                print(f"!! {dst_dir} 已存在；本工具只创建缺失目录（避免覆盖）",
                      file=sys.stderr)
                return 2
            if dry_run:
                print(f"  [dry-run] {key}: 会物化（{sum(1 for _ in base_dir.rglob('*') if _.is_file())} 文件，硬链接）")
            else:
                n_link = n_copy = 0
                for src in sorted(base_dir.rglob("*")):
                    if not src.is_file() or src.name == "destructibles.json":
                        continue
                    how = _link_or_copy(src, dst_dir / src.relative_to(base_dir))
                    n_link += how == "link"
                    n_copy += how == "copy"
                dst_dir.mkdir(parents=True, exist_ok=True)
                shutil.copy2(var_doc_path, dst_dir / "destructibles.json")
                print(f"  {key}: 物化变体目录（共享 {n_link} 硬链 + {n_copy} 复制，"
                      f"自有 destructibles.json）")
            index["maps"][mid]["key"] = key
            overlays.append(key)
    if not dry_run:
        index["generated"] = datetime.now(timezone.utc).isoformat(timespec="seconds")
        index_path.write_text(json.dumps(index, ensure_ascii=False, indent=1),
                              encoding="utf-8")
        print(f"变体目录 {len(overlays)} 个: {', '.join(overlays) if overlays else '（无）'}")
        print(f"共用基础目录 {len(skipped)} 个变体: {', '.join(skipped) if skipped else '（无）'}")
        print(f"index.json 已更新 → {index_path}")
    return 0


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--pack", type=Path, default=PROJECT_ROOT / "release" / "asset_pack")
    ap.add_argument("--map-index", type=Path, default=PROJECT_ROOT / "map_index.json")
    ap.add_argument("--cache-maps", type=Path, default=PROJECT_ROOT / "data" / "cache" / "maps")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    raise SystemExit(materialize(args.pack, args.map_index, args.cache_maps, args.dry_run))


if __name__ == "__main__":
    main()
