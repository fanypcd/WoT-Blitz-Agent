#!/usr/bin/env python3
"""
把 GPU 渲染的俯视裸像素（frontend/scripts/bake-ground-overhead.mjs 产物）合成进
高清底图：8 朝向与 ground.webp 自身相关配准（渲染图内容丰富，相关信号极强，
不依赖小地图），alpha 通道即覆盖掩膜，out = ground×(1−0.92a) + rgb×0.92a。

用法：
  python tools/composite_overhead.py --pack release/asset_pack --map medvedkovo
  python tools/composite_overhead.py --pack release/asset_pack --all
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image


def grad_mag(gray: np.ndarray) -> np.ndarray:
    g = gray.astype(np.float32)
    gx = np.zeros_like(g); gy = np.zeros_like(g)
    gx[:, 1:-1] = g[:, 2:] - g[:, :-2]
    gy[1:-1, :] = g[2:, :] - g[:-2, :]
    return np.hypot(gx, gy)


def orient_uv(u, v, orient: str):
    return {
        "xy": (u, v), "yx": (v, u),
        "xY": (u, 1 - v), "Yx": (1 - v, u),
        "Xy": (1 - u, v), "yX": (v, 1 - u),
        "XY": (1 - u, 1 - v), "YX": (1 - v, 1 - u),
    }[orient]


def remap(img: np.ndarray, orient: str) -> np.ndarray:
    """按朝向重排图像（8 种 = 转置/镜像组合，纯索引重排零插值）。"""
    h, w = img.shape[:2]
    table = {
        "xy": lambda a: a, "YX": lambda a: a[::-1, ::-1],
        "yx": lambda a: a.transpose(1, 0, 2) if a.ndim == 3 else a.T,
        "XY": lambda a: a.transpose(1, 0, 2)[::-1, ::-1] if a.ndim == 3 else a.T[::-1, ::-1],
        "xY": lambda a: a[::-1], "yX": lambda a: a.transpose(1, 0, 2)[::-1] if a.ndim == 3 else a.T[::-1],
        "Xy": lambda a: a[:, ::-1], "Yx": lambda a: a.transpose(1, 0, 2)[:, ::-1] if a.ndim == 3 else a.T[:, ::-1],
    }
    return table[orient](img)


def best_orientation(overhead: np.ndarray, ground_small: np.ndarray):
    """渲染图与底图的相关：对 8 朝向把 overhead 重排到 ground 的 512 缩图，
    取梯度相关最高者（两图都是丰富纹理，正确朝向相关度一骑绝尘）。"""
    g_ground = grad_mag(np.asarray(Image.fromarray(ground_small).convert("L").resize((256, 256)))).ravel()
    g_ground = (g_ground - g_ground.mean()) / (g_ground.std() + 1e-6)
    scores = {}
    for orient in ("xy", "yx", "xY", "Yx", "Xy", "yX", "XY", "YX"):
        small = remap(overhead, orient)
        g_over = grad_mag(np.asarray(Image.fromarray(small).convert("L").resize((256, 256)))).ravel()
        g_over = (g_over - g_over.mean()) / (g_over.std() + 1e-6)
        scores[orient] = float((g_ground * g_over).mean())
    best = max(scores, key=scores.get)
    return best, scores


def composite_map(pack: Path, key: str, write: bool, quality: int) -> dict:
    mdir = pack / "map" / key
    ground_path = mdir / "ground.webp"
    raw_path = pack / "overhead" / f"{key}.rgba"
    meta_path = pack / "overhead" / f"{key}.meta.json"
    if not (raw_path.is_file() and meta_path.is_file() and ground_path.is_file()):
        return {"key": key, "skipped": "缺 overhead 原始渲染或底图"}
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    size = meta["size"]
    rgba = np.frombuffer(raw_path.read_bytes(), np.uint8).reshape(size, size, 4)
    ground = np.asarray(Image.open(ground_path).convert("RGB")).copy()
    if ground.shape[0] != size:
        return {"key": key, "skipped": f"底图 {ground.shape[0]}² 与渲染 {size}² 尺寸不一致"}

    small = rgba[..., :3][::8, ::8]
    best, scores = best_orientation(small, ground[::8, ::8])
    over = remap(rgba, best).astype(np.float32)
    a = (over[..., 3:4] / 255.0) * 0.92
    out = ground.astype(np.float32) * (1 - a) + over[..., :3] * a
    out_img = Image.fromarray(out.clip(0, 255).astype(np.uint8))

    report = {"key": key, "orient": best, "scores": {k: round(v, 4) for k, v in scores.items()},
              "cover_pct": round(100 * (rgba[..., 3] > 8).mean(), 2), "written": False}
    if write:
        out_img.save(ground_path, "WEBP", quality=quality, method=6)
        report["written"] = True
        report["out_kb"] = round(ground_path.stat().st_size / 1024)
    return report


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pack", default="release/asset_pack")
    ap.add_argument("--map")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--write", action="store_true", help="写回 ground.webp（缺省只报告）")
    ap.add_argument("--quality", type=int, default=82)
    args = ap.parse_args()
    pack = Path(args.pack)
    keys = ([d.name for d in (pack / "overhead").glob("*.meta.json")] if args.all
            else [args.map])
    keys = [Path(k).stem if k.endswith(".meta.json") else k for k in keys]
    for key in keys:
        try:
            print(json.dumps(composite_map(pack, key, args.write, args.quality), ensure_ascii=False))
        except Exception as e:  # noqa: BLE001
            print(json.dumps({"key": key, "error": str(e)[:300]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
