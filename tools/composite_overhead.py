#!/usr/bin/env python3
"""
把 GPU 渲染的俯视裸像素（frontend/scripts/bake-ground-overhead.mjs 产物）合成进
高清底图：**朝向由坐标契约唯一确定**（常量 ORIENT，见其注释的推导），与底图自身
的 8 朝向相关只作诊断输出，不参与判决。alpha 通道即覆盖掩膜，
out = ground×(1−0.92a) + rgb×0.92a。

用法：
  python tools/composite_overhead.py --pack release/asset_pack --map medvedkovo
  python tools/composite_overhead.py --pack release/asset_pack --all
  # 重烘（读原始底图与缓存渲染，不经包内 overhead/，也避免二次合成）：
  python tools/composite_overhead.py --pack release/asset_pack --map himmelsdorf \
      --render-dir release/overhead-bake --base-dir data/cache/maps --write
  # 读回校验：反解包内成品实际烘入的朝向，应恒为 ORIENT
  python tools/composite_overhead.py --pack release/asset_pack --render-dir release/overhead-bake \
      --base-dir data/cache/maps --all --verify
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image

# 朝向 = 常量，不搜索。两侧都是固定契约（与地图无关），逐项可溯源：
#   渲染侧（frontend/scripts/bake-ground-overhead.mjs）：
#     group.rotation = qFrame Ry(π)·Rx(−π/2)，把游戏系 (x,y,z) 映到场景系 (−x, z, y)；
#     正交相机 position(0,1000,0) + up(0,0,−1) 俯视 ⇒ 屏幕右 = +X_scene、
#     屏幕上 = −Z_scene，readPixels 再翻成图像行序 ⇒ 渲染图左列 = −X_scene、
#     顶行 = −Z_scene。
#   底图侧（ground.webp 契约）：export_map_glb.py 分层注释「前端采样
#     uv = (0.5−X/s, 0.5−Z/s)」（playbackScene 的 ShaderMaterial 与均衡档
#     PlaneGeometry+TextureLoader(flipY=true) 两路同式）⇒ 底图左列 = +X_scene、
#     顶行 = +Z_scene（北；即「上=+z/北」）。
#   两者相差 180°：两轴各翻一次 ⇒ 恒为 "YX"，任何地图都不需要特殊处理。
# 历史教训（2026-10-09，勿再回退成相关性判决）：旧版对 8 朝向做梯度相关取 argmax，
#   在低信号图上退化为噪声 argmax——himmelsdorf 的 8 个候选全落在 |score|≤0.12 的
#   噪声带（winner margin 0.0124），把俯视层烘成了上下镜像（xY），均衡档地面
#   因此呈现镜像的建筑层（3D 档不受影响：它不读这张图）。同一病灶当时距翻车
#   仅 0.0054（erlenberg_old），只是碰巧选中了正确候选。
ORIENT = "YX"
VERIFY_MIN_PX = 4096   # 读回校验的最小覆盖像素数（512² 网格），低于则无法判读


def grad_mag(gray: np.ndarray) -> np.ndarray:
    g = gray.astype(np.float32)
    gx = np.zeros_like(g); gy = np.zeros_like(g)
    gx[:, 1:-1] = g[:, 2:] - g[:, :-2]
    gy[1:-1, :] = g[2:, :] - g[:-2, :]
    return np.hypot(gx, gy)


def remap(img: np.ndarray, orient: str) -> np.ndarray:
    """按朝向重排图像（8 种 = 转置/镜像组合，纯索引重排零插值）。"""
    table = {
        "xy": lambda a: a, "YX": lambda a: a[::-1, ::-1],
        "yx": lambda a: a.transpose(1, 0, 2) if a.ndim == 3 else a.T,
        "XY": lambda a: a.transpose(1, 0, 2)[::-1, ::-1] if a.ndim == 3 else a.T[::-1, ::-1],
        "xY": lambda a: a[::-1], "yX": lambda a: a.transpose(1, 0, 2)[::-1] if a.ndim == 3 else a.T[::-1],
        "Xy": lambda a: a[:, ::-1], "Yx": lambda a: a.transpose(1, 0, 2)[:, ::-1] if a.ndim == 3 else a.T[:, ::-1],
    }
    return table[orient](img)


def audit_orientations(overhead: np.ndarray, ground_small: np.ndarray) -> dict:
    """**诊断项，不参与判决**（朝向取常量 ORIENT）：对 8 朝向把 overhead 重排到与
    ground 的 256 缩图做梯度相关，报告其 argmax 与 margin。正常情况应选中 ORIENT
    （两图结构都能对上时锐利）；选中别的候选说明该图这一对照本身无信号
    （如 himmelsdorf 的城市地面底图 vs 屋顶渲染），或上游契约变了——后者要查
    bake-ground-overhead.mjs 的相机/qFrame 与 export_map_glb.py 的底图契约。"""
    g_ground = grad_mag(np.asarray(Image.fromarray(ground_small).convert("L").resize((256, 256)))).ravel()
    g_ground = (g_ground - g_ground.mean()) / (g_ground.std() + 1e-6)
    scores = {}
    for orient in ("xy", "yx", "xY", "Yx", "Xy", "yX", "XY", "YX"):
        small = remap(overhead, orient)
        g_over = grad_mag(np.asarray(Image.fromarray(small).convert("L").resize((256, 256)))).ravel()
        g_over = (g_over - g_over.mean()) / (g_over.std() + 1e-6)
        scores[orient] = float((g_ground * g_over).mean())
    order = sorted(scores, key=scores.get, reverse=True)
    return {"best": order[0], "margin": round(scores[order[0]] - scores[order[1]], 4),
            "scores": {k: round(v, 4) for k, v in scores.items()}}


def space_of(key: str, map_index: Path) -> str:
    """key → space（变体图共用 space 的底图）；缺注册表/未命中回退 key 本身。"""
    try:
        idx = json.loads(map_index.read_text(encoding="utf-8"))
    except Exception:  # noqa: BLE001
        return key
    for e in idx if isinstance(idx, list) else []:
        if isinstance(e, dict) and e.get("key") == key:
            return e.get("space") or key
    return key


def _corr(a: np.ndarray, b: np.ndarray, m: np.ndarray) -> float:
    x = a[m].astype(np.float64); y = b[m].astype(np.float64)
    x = x - x.mean(); y = y - y.mean()
    d = float(np.sqrt((x * x).sum() * (y * y).sum()))
    return float((x * y).sum() / d) if d > 0 else 0.0


def read_back_orientation(rgba: np.ndarray, base: np.ndarray, ground: np.ndarray) -> dict | None:
    """按合成式反解**实际烘入**的朝向：对每个候选朝向建重建图
    recon_o = base×(1−w_o) + remap(render,o)×w_o 与包内成品在覆盖区比对——
    被采用的朝向重建相关 ≈1（仅 webp 重编码噪声），其余候选显著低。"""
    b = base[::8, ::8].astype(np.float32)
    g = ground[::8, ::8].astype(np.float32)
    scores = {}
    for o in ("xy", "yx", "xY", "Yx", "Xy", "yX", "XY", "YX"):
        r = remap(rgba, o)
        rs = r[::8, ::8, :3].astype(np.float32)
        w = (r[::8, ::8, 3:4] / 255.0) * 0.92
        m = w[..., 0] > 0.3
        if int(m.sum()) < VERIFY_MIN_PX:
            return None
        scores[o] = round(_corr(b * (1 - w) + rs * w, g, m), 5)
    order = sorted(scores, key=scores.get, reverse=True)
    return {"applied": order[0], "margin": round(scores[order[0]] - scores[order[1]], 4),
            "corr": scores}


def composite_map(pack: Path, key: str, write: bool, quality: int,
                  render_dir: Path | None = None, base_dir: Path | None = None,
                  map_index: Path = Path("map_index.json")) -> dict:
    mdir = pack / "map" / key
    ground_path = mdir / "ground.webp"
    rdir = render_dir or (pack / "overhead")
    raw_path = rdir / f"{key}.rgba"
    meta_path = rdir / f"{key}.meta.json"
    base_path = (base_dir / f"{space_of(key, map_index)}.ground.webp") if base_dir else ground_path
    if not (raw_path.is_file() and meta_path.is_file() and base_path.is_file()):
        return {"key": key, "skipped": "缺 overhead 原始渲染或底图"}
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    size = meta["size"]
    rgba = np.frombuffer(raw_path.read_bytes(), np.uint8).reshape(size, size, 4)
    ground = np.asarray(Image.open(base_path).convert("RGB")).copy()
    if ground.shape[0] != size:
        return {"key": key, "skipped": f"底图 {ground.shape[0]}² 与渲染 {size}² 尺寸不一致"}

    audit = audit_orientations(np.ascontiguousarray(rgba[..., :3][::8, ::8]), ground[::8, ::8])
    over = remap(rgba, ORIENT).astype(np.float32)
    a = (over[..., 3:4] / 255.0) * 0.92
    out = ground.astype(np.float32) * (1 - a) + over[..., :3] * a
    out_img = Image.fromarray(out.clip(0, 255).astype(np.uint8))

    report = {"key": key, "orient": ORIENT, "audit": audit, "base": str(base_path),
              "cover_pct": round(100 * (rgba[..., 3] > 8).mean(), 2), "written": False}
    if audit["best"] != ORIENT:
        report["audit_disagrees"] = (f"{audit['best']}(margin {audit['margin']})——"
                                     "仅诊断：该图底图↔渲染对照无信号属正常（见 audit_orientations），"
                                     "若本应锐利的图也如此则查上游契约")
    if write:
        out_img.save(ground_path, "WEBP", quality=quality, method=6)
        report["written"] = True
        report["out_kb"] = round(ground_path.stat().st_size / 1024)
    return report


def verify_map(pack: Path, key: str, render_dir: Path | None = None,
               base_dir: Path = Path("data/cache/maps"),
               map_index: Path = Path("map_index.json")) -> dict:
    """只读校验：反解包内 ground.webp 实际烘入的朝向，ok = 是否恒等于 ORIENT。"""
    rdir = render_dir or (pack / "overhead")
    raw_path = rdir / f"{key}.rgba"
    meta_path = rdir / f"{key}.meta.json"
    ground_path = pack / "map" / key / "ground.webp"
    base_path = base_dir / f"{space_of(key, map_index)}.ground.webp"
    for p in (raw_path, meta_path, ground_path, base_path):
        if not p.is_file():
            return {"key": key, "skipped": f"缺 {p.name}"}
    size = json.loads(meta_path.read_text(encoding="utf-8"))["size"]
    rgba = np.frombuffer(raw_path.read_bytes(), np.uint8).reshape(size, size, 4)
    base = np.asarray(Image.open(base_path).convert("RGB"))
    ground = np.asarray(Image.open(ground_path).convert("RGB"))
    back = read_back_orientation(rgba, base, ground)
    if back is None:
        return {"key": key, "skipped": "覆盖太小，无法判读"}
    return {"key": key, **back, "ok": back["applied"] == ORIENT}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pack", default="release/asset_pack")
    ap.add_argument("--map")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--write", action="store_true", help="写回 ground.webp（缺省只报告）")
    ap.add_argument("--quality", type=int, default=82)
    ap.add_argument("--render-dir", type=Path,
                    help="渲染 .rgba/.meta.json 所在目录（缺省 <pack>/overhead；缓存常驻 <pack>/../overhead-bake）")
    ap.add_argument("--base-dir", type=Path,
                    help="合成前底图 <space>.ground.webp 所在目录（缺省读包内 ground.webp；重烘须给"
                         "data/cache/maps，否则会在已合成的图上二次合成）")
    ap.add_argument("--map-index", type=Path, default=Path("map_index.json"))
    ap.add_argument("--verify", action="store_true",
                    help="只读校验：反解包内成品实际烘入的朝向（应恒为 ORIENT）")
    args = ap.parse_args()
    pack = Path(args.pack)
    rdir = args.render_dir or (pack / "overhead")
    keys = ([f.stem[: -len(".meta")] for f in rdir.glob("*.meta.json")] if args.all else [args.map])
    for key in keys:
        try:
            if args.verify:
                print(json.dumps(verify_map(pack, key, args.render_dir, args.base_dir or Path("data/cache/maps"),
                                            args.map_index), ensure_ascii=False))
            else:
                print(json.dumps(composite_map(pack, key, args.write, args.quality, args.render_dir,
                                               args.base_dir, args.map_index), ensure_ascii=False))
        except Exception as e:  # noqa: BLE001
            print(json.dumps({"key": key, "error": str(e)[:300]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
