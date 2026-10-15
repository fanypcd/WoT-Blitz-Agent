#!/usr/bin/env python3
"""把包内 `overhead/` 的烘焙渲染件归档到缓存目录：`<pack>/overhead/` → `release/overhead-bake/`。

**为什么需要这一步、为什么必须由脚本做**：烘焙（WotbTools `frontend/scripts/bake-ground-overhead.mjs`）
把 `<key>.rgba`/`<key>.meta.json` 写进**包内** `overhead/`，合成器默认就从那里读；"合成完即清包内
`overhead/`"是惯例，而**归档是唯一的长期留存路径**。2026-10-15 审计实证过漏掉它的后果：地面 pass
那批渲染件（新世代）进了包、被合成、被清，归档停在地面 pass 之前 → 本机再也造不出线上那一代底图
（只能从 COS 回拉成品）。故归档必须**紧跟烘焙、且在清包内 `overhead/` 之前**。

护栏（fail-closed）：
  · 每张图必须 `<key>.rgba` + `<key>.meta.json` **成对**存在——缺任一半即整体拒绝（不部分归档，
    否则重合成时会把"缺件"与"上一轮旧件"混成一代）；
  · `upload.rgba`（/pixel-upload 中间件落盘点）不归档、不计入配对；
  · 拷贝后逐件**字节数核对**，不一致即报错退出；
  · 逐图打印世代（meta 的 `ground` 键：true / false / **缺**）——"缺"= 地面 pass 前的渲染件，
    即警告（这类件合成时会被 `composite_overhead.py` 默认拒写）。

用法：
    python tools/archive_overhead_bake.py                        # 默认 pack/overhead → release/overhead-bake
    python tools/archive_overhead_bake.py --verify-only          # 只体检配对与世代，不拷贝
    python tools/archive_overhead_bake.py --clear-pack           # 归档核对通过后，清掉包内这两类文件
"""
from __future__ import annotations

import argparse
import json
import pathlib
import shutil
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from composite_overhead import render_generation  # noqa: E402  （世代判据单一实现）

IGNORED = {"upload.rgba"}  # /pixel-upload 落盘点：上传中间件所用，非逐图产物


def pair_up(names: list[str]) -> tuple[list[str], list[str], list[str]]:
    """文件名清单 → (成对的 key 升序, errors, ignored)。

    errors 形如 `b: 缺 b.rgba` / `c: 缺 c.meta.json`；`upload.rgba` 等中间件文件进 ignored。
    """
    metas = {n[: -len(".meta.json")] for n in names if n.endswith(".meta.json")}
    rawes = {n[: -len(".rgba")] for n in names if n.endswith(".rgba") and n not in IGNORED}
    ignored = sorted(n for n in names if n in IGNORED)
    keys = sorted(metas & rawes)
    # 错误按 key 排序输出（两种缺件混排），报告顺序确定
    error_of = {k: f"{k}: 缺 {k}.rgba" for k in metas - rawes}
    error_of.update({k: f"{k}: 缺 {k}.meta.json" for k in rawes - metas})
    errors = [error_of[k] for k in sorted(error_of)]
    return keys, errors, ignored


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pack", type=pathlib.Path, default=pathlib.Path("release/asset_pack"))
    ap.add_argument("--dest", type=pathlib.Path, default=pathlib.Path("release/overhead-bake"))
    ap.add_argument("--verify-only", action="store_true", help="只体检配对与世代，不拷贝")
    ap.add_argument("--clear-pack", action="store_true",
                    help="归档核对通过后，清掉包内 overhead/ 的 .rgba/.meta.json（含 upload.rgba 残留）")
    args = ap.parse_args()

    src = args.pack / "overhead"
    if not src.is_dir():
        print(f"包内 overhead 目录不存在：{src}（先跑烘焙 bake-ground-overhead.mjs）", file=sys.stderr)
        return 1
    names = sorted(p.name for p in src.iterdir() if p.is_file())
    keys, errors, ignored = pair_up(names)
    if errors:
        print("成对校验失败，**整体拒绝归档**（不部分拷贝）：", file=sys.stderr)
        for e in errors:
            print(f"  {e}", file=sys.stderr)
        return 1
    if not keys:
        print(f"{src} 里没有 <key>.rgba + <key>.meta.json 产物（只有 {ignored or '空'}）", file=sys.stderr)
        return 1

    gens: dict[str, list[str]] = {}
    for key in keys:
        meta = json.loads((src / f"{key}.meta.json").read_text(encoding="utf-8"))
        gens.setdefault(render_generation(meta), []).append(key)
    for gen in ("ground-pass", "no-terrain", "pre-ground-pass"):
        if gens.get(gen):
            note = "  ⚠ 地面 pass 前世代：合成时会被默认拒写，先重烘" if gen == "pre-ground-pass" else ""
            print(f"{gen:16s} {len(gens[gen]):3d} 张{note}")

    if args.verify_only:
        print(f"体检完成：{len(keys)} 张成对、0 缺件；未拷贝。")
        return 0

    args.dest.mkdir(parents=True, exist_ok=True)
    copied = 0
    for key in keys:
        for name in (f"{key}.rgba", f"{key}.meta.json"):
            dst = args.dest / name
            shutil.copyfile(src / name, dst)
            if dst.stat().st_size != (src / name).stat().st_size:
                print(f"字节数核对失败：{name}", file=sys.stderr)
                return 1
            copied += 1
    mb = sum((args.dest / f"{k}.rgba").stat().st_size for k in keys) / 1024 / 1024
    print(f"已归档 {copied} 件（{len(keys)} 张图，rgba 合计 {mb:.1f}MB）→ {args.dest}")

    if args.clear_pack:
        for key in keys:
            (src / f"{key}.rgba").unlink()
            (src / f"{key}.meta.json").unlink()
        for name in ignored:
            (src / name).unlink()
        print(f"已清包内 overhead/ 的归档件与中间件残留（{len(keys)} 张）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
