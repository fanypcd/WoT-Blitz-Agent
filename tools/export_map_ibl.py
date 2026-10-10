#!/usr/bin/env python3
"""逐图 IBL 环境贴图导出：`data/cache/maps/<space>.ibl.webp`（等距柱状投影，供 three 的
`scene.environment`；动态物/坦克的环境反射来源）。

数据源：`Data/3d/Maps/<space>/IBL/{diffuse,specular}.dx11.dds.dvpl`（逐图**齐备**，见
`lighting.json` 的 `ibl` 块）：
  - `diffuse`  = 64² 立方图（漫反射辐照度，BC1，无 mip）；
  - `specular` = 256² 立方图 + 9 级 mip（**预滤**反射链，BC1）。
两者都是 DDS 立方图（`caps2 & 0x200`，面序 +X,−X,+Y,−Y,+Z,−Z；参考实现同序）。

取 **specular 的 mip0** 作环境源：它是预滤链里最锐的一级（≈粗糙度 0），喂给 three 的
PMREM 后由引擎生成粗糙度链，观感最接近客户端的 `MipFromRoughness`（客户端口径见
docs/feasibility-map-lighting.md §1.3；精确复刻属期 3）。`ibl.environmentMultiplier` 由
消费端经 `scene.environmentIntensity` 施加（该值逐图 0.8–4.0）。

立方图 → 等距柱状投影的坐标约定（**待视觉校准项**）：采用 **OpenGL/three 右手法**
（+Y 上、−Z 前），即与 three 的 `EquirectangularReflectionMapping` 一致；DDS 侧为 D3D
左手法时 ±Z 两面会前后互换——若反射里的地标左右/前后相反，flip 本文件的 `_face_dir`
中 z 分量符号即可（一行）。

用法：
    python tools/export_map_ibl.py                     # 全部（按 maps.yaml 注册表）
    python tools/export_map_ibl.py --map himmelsdorf --map 17
"""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import struct
import sys

import numpy as np
from PIL import Image

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_DIR))
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

import export_map_glb as E  # noqa: E402
from wotb_sc2 import decode_dvpl  # noqa: E402
from wotb_cube import cube_to_equirect, decode_dds_cube  # noqa: E402

OUT_W = 1024          # 等距柱状投影宽度（高度 = W/2）；1024×512 对漫反射/中粗糙度足够
JPEG_QUALITY = 90     # webp 质量（写成 webp，体积 ~100KB/图）


def resolve_ibl_file(map_dir: pathlib.Path, rel: str) -> pathlib.Path | None:
    stem = rel[:-4] if rel.lower().endswith(".tex") else rel
    base = (map_dir / stem).resolve() if stem.startswith("../") else map_dir / stem
    for suf in (".dx11.dds.dvpl", ".dds.dvpl", ".dx11.pvr.dvpl", ".pvr.dvpl"):
        cand = base.with_name(base.name + suf)
        if cand.exists():
            return cand
    return None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--map", action="append", help="显示名/键/回放地图 id（可重复）")
    ap.add_argument("--game-data", type=pathlib.Path,
                    default=pathlib.Path("D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data"))
    ap.add_argument("--out-dir", type=pathlib.Path, default=pathlib.Path("data/cache/maps"))
    ap.add_argument("--width", type=int, default=OUT_W, help="等距柱状投影宽度（高度取其半）")
    args = ap.parse_args()

    entries = E.load_registry(args.game_data)
    picked = []
    for name in (args.map or []):
        e = E.resolve_entry(entries, name)
        if e is None:
            print(f"[skip] 未在注册表命中：{name}", file=sys.stderr)
        else:
            picked.append(e)
    if not args.map:
        picked = entries

    args.out_dir.mkdir(parents=True, exist_ok=True)
    ok = skip = fail = 0
    for entry in picked:
        map_dir = args.game_data / "3d" / "Maps" / entry.space
        lj = args.out_dir / f"{entry.space}.lighting.json"
        ibl = {}
        if lj.exists():
            ibl = (json.loads(lj.read_text(encoding="utf-8")) or {}).get("ibl") or {}
        src_rel = ibl.get("ibl.specular") or ibl.get("ibl.diffuse")
        src = resolve_ibl_file(map_dir, src_rel) if src_rel else None
        if src is None:
            for cand in ("IBL/specular", "IBL/diffuse"):
                src = resolve_ibl_file(map_dir, cand)
                if src:
                    break
        if src is None:
            print(f"[skip] {entry.space}: IBL 立方图缺失")
            skip += 1
            continue
        try:
            raw = decode_dvpl(src.read_bytes()) if src.suffix == ".dvpl" else src.read_bytes()
            dec = decode_dds_cube(raw)
            if dec is None:
                print(f"[skip] {entry.space}: {src.name} 非 DDS 立方图")
                skip += 1
                continue
            w, mips, faces = dec
            img = cube_to_equirect(faces, args.width)
            out = args.out_dir / f"{entry.space}.ibl.webp"
            img.save(out, "WEBP", quality=JPEG_QUALITY)
            print(f"[ok] {entry.space:24s} {src.name} {w}² mips={mips} → {img.size} "
                  f"{out.stat().st_size/1024:.0f}KB")
            ok += 1
        except Exception as exc:  # noqa: BLE001
            print(f"[fail] {entry.space}: {type(exc).__name__}: {exc}", file=sys.stderr)
            fail += 1
    print(f"完成：{ok} 张，跳过 {skip}，失败 {fail}")
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
