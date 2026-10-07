#!/usr/bin/env python3
"""
烘焙**真实场景正交俯视图**进高清底图（均衡档地面）：带材质纹理的屋顶、树冠、
草丛——一切从上往下看得见的静态几何，按真实高度互相遮挡。

背景：4096² 烘焙底图（ground.webp，客户端 landscape colormap 提取）是纯地形。
均衡/流畅档不加载 3D 场景 GLB，地形图上没有树也没有建筑。本脚本把场景 GLB
做**带纹理的软光栅正交渲染**（z-buffer + 逐像素 UV 插值 + 材质纹理采样 +
alpha 裁切 + 顶点色/材质因子乘积），合成回 ground.webp——即真实场景的俯视
照片。客户端小地图只作朝向校准参照，不再是颜色源（风格化灰屋顶 ≠ 真实观感）。

选择规则：
- 排除 D_*/State N（损毁态变体）、mdVariant extras（跨变体污染——变体图共用
  一份底图，只烘无标记节点）、invisiblewall（不可见碰撞墙）、天空球、水面
  （地形图已含水系配色，避免半透明大面覆盖）。
- 叶卡（_CORNER，树冠）必须包含：底 position 是锚点（零面积），真实四边形由
  客户端 shader 按世界轴展开 _corner×scale 得到——烘焙做同款展开。
- md3 等变体专属物体不进共享底图（量化：medvedkovo 仅 5 个 md3 节点）。

朝向不信任推导：校准在 512² 以 8 种朝向投影覆盖掩膜，与客户端小地图的暗度
分布做相关取最优。medvedkovo 实测胜者 xY（北朝上）0.098，明确高于 UV 链
推导朝向 xy 的 0.057。

用法：
  python tools/bake_ground_roofs.py --self-test
  python tools/bake_ground_roofs.py --pack release/asset_pack --map medvedkovo          # 校准 + 报告（不写）
  python tools/bake_ground_roofs.py --pack release/asset_pack --map medvedkovo --write  # 写回 ground.webp
  python tools/bake_ground_roofs.py --pack release/asset_pack --all --write
"""
from __future__ import annotations

import argparse
import io
import json
import re
import struct
from pathlib import Path

import numpy as np
from PIL import Image

ALPHA_CUT = 0.33          # 与场景叶卡 shader 同一裁切阈值
TEX_MAX_DIM = 1024        # 纹理解码上限（俯视采样不需要原始精度，控内存）

_IMAGES: list = []        # 当前 GLB 的解码纹理（bake_map 装载，collect/render 消费）

# ---------------------------------------------------------------- GLB 解析


def parse_glb(path: Path):
    data = path.read_bytes()
    assert data[:4] == b"glTF", f"{path} 不是 GLB"
    json_len = struct.unpack_from("<I", data, 12)[0]
    meta = json.loads(data[20:20 + json_len].decode("utf-8"))
    bin_off = 20 + json_len
    bin_len = struct.unpack_from("<I", data, bin_off)[0]
    bin_chunk = data[bin_off + 8:bin_off + 8 + bin_len]
    return meta, bin_chunk


def accessor_read(meta, bin_chunk, index):
    acc = meta["accessors"][index]
    bv = meta["bufferViews"][acc["bufferView"]]
    comp_counts = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}
    comp_types = {5120: np.int8, 5121: np.uint8, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
    n, cc = acc["count"], comp_counts[acc["type"]]
    dt = comp_types[acc["componentType"]]
    off = bv.get("byteOffset", 0) + acc.get("byteOffset", 0)
    stride = bv.get("byteStride") or (np.dtype(dt).itemsize * cc)
    if stride == np.dtype(dt).itemsize * cc:
        flat = np.frombuffer(bin_chunk, dtype=dt, count=n * cc, offset=off)
    else:  # 交错布局
        raw = np.frombuffer(bin_chunk, dtype=np.uint8, count=stride * n, offset=off)
        flat = np.lib.stride_tricks.as_strided(
            raw, shape=(n, cc), strides=(stride, np.dtype(dt).itemsize)).reshape(-1)
    return flat.reshape(n, cc) if cc > 1 else flat.reshape(n)


def node_local_matrix(n: dict) -> np.ndarray:
    if "matrix" in n:
        return np.array(n["matrix"], np.float64).reshape(4, 4).T  # 存储列主序 → 行主序
    q = n.get("rotation", [0.0, 0.0, 0.0, 1.0])
    s = n.get("scale", [1.0, 1.0, 1.0])
    x, y, z, w = q
    m = np.eye(4)
    m[0, 0] = 1 - 2 * (y * y + z * z); m[0, 1] = 2 * (x * y - z * w); m[0, 2] = 2 * (x * z + y * w)
    m[1, 0] = 2 * (x * y + z * w); m[1, 1] = 1 - 2 * (x * x + z * z); m[1, 2] = 2 * (y * z - x * w)
    m[2, 0] = 2 * (x * z - y * w); m[2, 1] = 2 * (y * z + x * w); m[2, 2] = 1 - 2 * (x * x + y * y)
    m[:3, 0] *= s[0]; m[:3, 1] *= s[1]; m[:3, 2] *= s[2]
    m[:3, 3] = n.get("translation", [0.0, 0.0, 0.0])
    return m


def load_images(meta, bin_chunk):
    """解码全部 GLB image 到 ≤TEX_MAX_DIM 的 np.ndarray(RGBA float32/255)。"""
    out = []
    for img in meta.get("images", []):
        bv = meta["bufferViews"][img["bufferView"]]
        raw = bin_chunk[bv.get("byteOffset", 0):bv.get("byteOffset", 0) + bv["byteLength"]]
        im = Image.open(io.BytesIO(raw)).convert("RGBA")
        if max(im.size) > TEX_MAX_DIM:
            im.thumbnail((TEX_MAX_DIM, TEX_MAX_DIM), Image.BILINEAR)
        out.append(np.asarray(im, np.float32) / 255.0)
    return out


def material_factor(meta, mi):
    mats = meta.get("materials", [])
    if mi is None or mi >= len(mats):
        return np.array([1, 1, 1, 1], np.float32)
    pbr = mats[mi].get("pbrMetallicRoughness", {})
    return np.array(pbr.get("baseColorFactor", [1, 1, 1, 1]), np.float32)


def material_tex(meta, mi):
    mats = meta.get("materials", [])
    if mi is None or mi >= len(mats):
        return None
    ti = mats[mi].get("pbrMetallicRoughness", {}).get("baseColorTexture", {}).get("index")
    if ti is None:
        return None
    src = meta["textures"][ti].get("source")
    return _IMAGES[src] if src is not None else None


# ---------------------------------------------------------------- 几何收集


def collect_primitives(meta, bin_chunk, exclude_re, max_side_m: float):
    """收集选中 primitive 的世界系数据。返回 (prims, flat_tris, stats)：
    prims: [{tris(T,3,3), uv|None, vcol|None, factor, tex}]（叶卡已做 _corner 展开）
    flat_tris: 同数据的展开三角形（校准/覆盖统计用）。"""
    meshes = meta.get("meshes", [])
    nodes = meta.get("nodes", [])
    roots = meta["scenes"][meta.get("scene", 0)]["nodes"]
    stats = {"nodes_seen": 0, "mesh_used": 0, "mesh_skipped": 0, "tris": 0}
    prims, flat = [], []
    stack = [(ri, np.eye(4)) for ri in reversed(roots)]
    while stack:
        ni, parent = stack.pop()
        n = nodes[ni]
        world = parent @ node_local_matrix(n)
        for c in reversed(n.get("children", [])):
            stack.append((c, world))
        if n.get("mesh") is None:
            continue
        stats["nodes_seen"] += 1
        name = n.get("name") or ""
        if exclude_re.search(name) or (n.get("extras") or {}).get("mdVariant"):
            stats["mesh_skipped"] += 1
            continue
        ws = np.linalg.norm(world[:3, :3], axis=0)   # 模型缩放（叶卡 _corner 世界展开用）
        mi = n.get("mesh")
        for prim in meshes[mi]["primitives"]:
            attrs = prim["attributes"]
            pos = accessor_read(meta, bin_chunk, attrs["POSITION"]).astype(np.float64)
            idx = accessor_read(meta, bin_chunk, prim["indices"]) if "indices" in prim else np.arange(len(pos))
            tri = pos[idx].reshape(-1, 3, 3)
            tri = tri @ world[:3, :3].T + world[:3, 3]
            if "_CORNER" in attrs:
                cattr = accessor_read(meta, bin_chunk, attrs["_CORNER"]).astype(np.float64)
                cc = cattr.shape[1]                       # VEC3/VEC4 都有（w 分量弃用）
                corner = cattr[idx].reshape(-1, 3, cc)[..., :3]
                tri = tri + ws * corner                   # 客户端 shader 同款世界轴展开
            side = np.stack([np.linalg.norm(tri[:, i] - tri[:, j], axis=1)
                             for i, j in ((0, 1), (1, 2), (2, 0))], axis=1)
            keep = (side.max(axis=1) <= max_side_m) & (side.min(axis=1) > 1e-4)
            if not keep.any():
                continue
            tri = tri[keep]
            uv = None
            if "TEXCOORD_0" in attrs:
                uv = accessor_read(meta, bin_chunk, attrs["TEXCOORD_0"]).astype(np.float32)[idx].reshape(-1, 3, 2)[keep]
            vcol = None
            if "COLOR_0" in attrs:
                cv = accessor_read(meta, bin_chunk, attrs["COLOR_0"]).astype(np.float32)[idx].reshape(-1, 3, 4)[keep]
                if cv.max() > 1.001:                         # ubyte 归一化兜底
                    cv = cv / 255.0
                vcol = cv
            prims.append({"tris": tri, "uv": uv, "vcol": vcol,
                          "factor": material_factor(meta, prim.get("material")),
                          "tex": material_tex(meta, prim.get("material"))})
            flat.append(tri)
            stats["tris"] += len(tri)
            stats["mesh_used"] += 1
    return prims, (np.concatenate(flat) if flat else np.zeros((0, 3, 3))), stats


# ---------------------------------------------------------------- 朝向与光栅化


def world_to_uv(x, y, orient: str, span: float):
    """世界 (x,y) → 图像归一化 (u,v)（8 候选朝向；校准择优，不信任推导）。"""
    u = (x + span / 2) / span
    v = (y + span / 2) / span
    return {
        "xy": (u, v), "yx": (v, u),
        "xY": (u, 1 - v), "Yx": (1 - v, u),
        "Xy": (1 - u, v), "yX": (v, 1 - u),
        "XY": (1 - u, 1 - v), "YX": (1 - v, 1 - u),
    }[orient]


def rasterize_tris(tris, size: int, span: float, orient: str):
    """无纹理覆盖掩膜（校准用）。返回 (mask bool, zbuf float32)。
    mask 必须保持 bool——uint8 会被 numpy 当行索引而非布尔掩膜。"""
    zbuf = np.full((size, size), -np.inf, np.float32)
    mask = np.zeros((size, size), bool)
    if len(tris) == 0:
        return mask, zbuf
    uv = np.empty((len(tris), 3, 2))
    for i in range(3):
        u, v = world_to_uv(tris[:, i, 0], tris[:, i, 1], orient, span)
        uv[:, i, 0] = u * size
        uv[:, i, 1] = v * size
    depth = tris[:, :, 2]
    for t in range(len(tris)):
        p = uv[t]
        x0 = max(int(np.floor(p[:, 0].min())), 0)
        x1 = min(int(np.ceil(p[:, 0].max())) + 1, size)
        y0 = max(int(np.floor(p[:, 1].min())), 0)
        y1 = min(int(np.ceil(p[:, 1].max())) + 1, size)
        if x0 >= x1 or y0 >= y1:
            continue
        gx, gy = np.meshgrid(np.arange(x0, x1) + 0.5, np.arange(y0, y1) + 0.5)
        a, b, c = p[0], p[1], p[2]
        det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
        if abs(det) < 1e-9:
            continue
        w0 = ((b[1] - c[1]) * (gx - c[0]) + (c[0] - b[0]) * (gy - c[1])) / det
        w1 = ((c[1] - a[1]) * (gx - c[0]) + (a[0] - c[0]) * (gy - c[1])) / det
        w2 = 1 - w0 - w1
        inside = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
        if not inside.any():
            continue
        z = w0 * depth[t, 2] + w1 * depth[t, 0] + w2 * depth[t, 1]
        sub = zbuf[y0:y1, x0:x1]
        take = inside & (z > sub)
        sub[take] = z[take]
        mask[y0:y1, x0:x1][take] = True
    return mask, zbuf


def bilinear_sample(img: np.ndarray, u: np.ndarray, v: np.ndarray) -> np.ndarray:
    """u/v ∈ [0,1] 双线性采样（img: H×W×C）；1×1 纹理退化为常量取样。"""
    h, w = img.shape[:2]
    fx = np.clip(u, 0.0, 1.0) * (w - 1)
    fy = np.clip(v, 0.0, 1.0) * (h - 1)
    x0 = np.floor(fx).astype(np.int64); y0 = np.floor(fy).astype(np.int64)
    x1 = np.minimum(x0 + 1, w - 1); y1 = np.minimum(y0 + 1, h - 1)
    tx = (fx - x0)[..., None]; ty = (fy - y0)[..., None]
    return (img[y0, x0] * (1 - tx) * (1 - ty) + img[y0, x1] * tx * (1 - ty)
            + img[y1, x0] * (1 - tx) * ty + img[y1, x1] * tx * ty)


def render_textured(prims, size: int, span: float, orient: str):
    """带纹理正交软渲染：返回 (mask bool, colorbuf H×W×3 float32)。
    逐像素：z-buffer + UV 插值纹理双线性采样 × 材质因子 × 顶点色；
    alpha < ALPHA_CUT 不落缓冲（叶冠间隙透出地面）。无光照模拟（albedo 直出）。"""
    zbuf = np.full((size, size), -np.inf, np.float32)
    colorbuf = np.zeros((size, size, 3), np.float32)
    mask = np.zeros((size, size), bool)
    for pr in prims:
        tris, uv, vcol, factor, tex = pr["tris"], pr["uv"], pr["vcol"], pr["factor"], pr["tex"]
        untextured = uv is None or tex is None
        for t in range(len(tris)):
            p = np.empty((3, 2))
            for i in range(3):
                u, v = world_to_uv(tris[t, i, 0], tris[t, i, 1], orient, span)
                p[i] = (u * size, v * size)
            x0 = max(int(np.floor(p[:, 0].min())), 0)
            x1 = min(int(np.ceil(p[:, 0].max())) + 1, size)
            y0 = max(int(np.floor(p[:, 1].min())), 0)
            y1 = min(int(np.ceil(p[:, 1].max())) + 1, size)
            if x0 >= x1 or y0 >= y1:
                continue
            gx, gy = np.meshgrid(np.arange(x0, x1) + 0.5, np.arange(y0, y1) + 0.5)
            a, b, c = p[0], p[1], p[2]
            det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
            if abs(det) < 1e-9:
                continue
            w0 = ((b[1] - c[1]) * (gx - c[0]) + (c[0] - b[0]) * (gy - c[1])) / det
            w1 = ((c[1] - a[1]) * (gx - c[0]) + (a[0] - c[0]) * (gy - c[1])) / det
            w2 = 1 - w0 - w1
            inside = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
            if not inside.any():
                continue
            z = w0 * tris[t, 2, 2] + w1 * tris[t, 0, 2] + w2 * tris[t, 1, 2]
            n_px = gx.size
            if untextured:
                rgba = np.ones(gx.shape + (4,), np.float32)
            else:
                iu = w0 * uv[t, 0, 0] + w1 * uv[t, 1, 0] + w2 * uv[t, 2, 0]
                iv = w0 * uv[t, 0, 1] + w1 * uv[t, 1, 1] + w2 * uv[t, 2, 1]
                rgba = bilinear_sample(tex, iu, 1.0 - iv)   # glTF UV 原点在左下
            if vcol is None:
                vc = np.ones(gx.shape + (4,), np.float32)
            else:
                vc = w0[..., None] * vcol[t, 0] + w1[..., None] * vcol[t, 1] + w2[..., None] * vcol[t, 2]
            rgb = rgba[..., :3] * factor[:3] * vc[..., :3]
            alpha = rgba[..., 3] * factor[3] * vc[..., 3]
            valid = inside & (alpha >= ALPHA_CUT)
            if not valid.any():
                continue
            sub_z = zbuf[y0:y1, x0:x1]
            take = valid & (z > sub_z)
            if not take.any():
                continue
            sub_z[take] = z[take]
            colorbuf[y0:y1, x0:x1][take] = rgb[take]
            mask[y0:y1, x0:x1][take] = True
    return mask, colorbuf


# ---------------------------------------------------------------- 校准与烘焙


def calibrate(tris, mini_gray: np.ndarray, span: float):
    """8 朝向掩膜暗度 vs 小地图暗度的均值差（越高 = 覆盖落点越对）。"""
    small = np.asarray(Image.fromarray(mini_gray).resize((512, 512)), np.float32) / 255.0
    scores = {}
    for orient in ("xy", "yx", "xY", "Yx", "Xy", "yX", "XY", "YX"):
        mask, _ = rasterize_tris(tris, 512, span, orient)
        if mask.sum() < 64:
            scores[orient] = float("nan")
            continue
        scores[orient] = float(small[~mask].mean() - small[mask].mean())
    return scores


def span_of(meta) -> float:
    sp = (meta.get("extras") or {}).get("space", "")
    cand = Path("data/cache/maps") / f"{sp}.json"
    wb = None
    if cand.is_file():
        wb = json.loads(cand.read_text(encoding="utf-8")).get("worldBounds")
    wb = wb or {"min": [-300, -300, 0], "max": [300, 300, 70]}
    return max(wb["max"][0] - wb["min"][0], wb["max"][1] - wb["min"][1])


def bake_map(pack: Path, key: str, write: bool, quality: int, min_score: float = 0.02) -> dict:
    mdir = pack / "map" / key
    glb_path, ground_path, mini_path = mdir / "scenery.glb", mdir / "ground.webp", mdir / "mini.webp"
    for p in (glb_path, ground_path, mini_path):
        if not p.is_file():
            return {"key": key, "skipped": f"缺 {p.name}"}
    meta, bin_chunk = parse_glb(glb_path)
    global _IMAGES
    _IMAGES = load_images(meta, bin_chunk)

    exclude = re.compile(r"^D_|State ?[1-9]|invisible|sky|water|sea|river|lake", re.I)
    prims, flat_tris, stats = collect_primitives(meta, bin_chunk, exclude, max_side_m=200.0)
    span = span_of(meta)

    mini = np.asarray(Image.open(mini_path).convert("RGB"))
    scores = calibrate(flat_tris, np.asarray(Image.fromarray(mini).convert("L")), span)
    valid_scores = {k: v for k, v in scores.items() if v == v}
    best = max(valid_scores, key=valid_scores.get) if valid_scores else None
    # 置信门限：最高分低于阈值 = 没有任何朝向能与客户端小地图对上——
    # 多半是小地图配色对比度不足（暗屋顶配暗地形等），此时**不写**（fail-closed：
    # 错朝向的屋顶比没有屋顶糟），保留原底图待人工复核。
    if write and (best is None or valid_scores[best] < min_score):
        return {"key": key, "skipped": f"校准弱（best={best}:{round(valid_scores.get(best, float('nan')), 4) if valid_scores else 'nan'} < {min_score}），未写", "scores": {k: round(v, 4) for k, v in scores.items()}}

    ground = np.asarray(Image.open(ground_path).convert("RGB")).copy()
    size = ground.shape[0]
    mask, colorbuf = render_textured(prims, size, span, best)

    out = ground.astype(np.float32)
    ys, xs = np.nonzero(mask)
    out[ys, xs] = colorbuf[ys, xs] * 255.0 * 0.92 + out[ys, xs] * 0.08
    out_img = Image.fromarray(out.clip(0, 255).astype(np.uint8))

    report = {"key": key, "orient": best, "scores": {k: round(v, 4) for k, v in scores.items()},
              "tris": stats["tris"], "mesh_used": stats["mesh_used"], "mesh_skipped": stats["mesh_skipped"],
              "cover_pct": round(100 * mask.sum() / mask.size, 2), "written": False}
    if write:
        out_img.save(ground_path, "WEBP", quality=quality, method=6)
        report["written"] = True
        report["out_kb"] = round(ground_path.stat().st_size / 1024)
    return report


def self_test():
    span = 100.0
    tris = np.array([[[0, 0, 5], [10, 0, 5], [0, 10, 5]]], np.float64)
    mask, zbuf = rasterize_tris(tris, 100, span, "xy")
    assert 20 < mask.sum() < 60
    assert zbuf[mask].min() > 4.9
    lo = np.array([[[0, 0, 1], [10, 0, 1], [0, 10, 1]]], np.float64)
    hi = np.array([[[2, 2, 9], [10, 2, 9], [2, 10, 9]]], np.float64)
    m1, z1 = rasterize_tris(lo, 100, span, "xy")
    m2, z2 = rasterize_tris(hi, 100, span, "xy")
    z1[m2] = np.maximum(z1[m2], z2[m2])
    assert z1[53, 53] == 9.0 and z1[50, 50] == 1.0   # 交集高者胜 / 非交集保持
    for orient in ("xy", "yx", "xY", "Yx", "Xy", "yX", "XY", "YX"):
        A = np.array([[world_to_uv(1.0, 0.0, orient, 1.0)[0] - world_to_uv(0.0, 0.0, orient, 1.0)[0],
                       world_to_uv(0.0, 1.0, orient, 1.0)[0] - world_to_uv(0.0, 0.0, orient, 1.0)[0]],
                      [world_to_uv(1.0, 0.0, orient, 1.0)[1] - world_to_uv(0.0, 0.0, orient, 1.0)[1],
                       world_to_uv(0.0, 1.0, orient, 1.0)[1] - world_to_uv(0.0, 0.0, orient, 1.0)[1]]])
        o = np.array([world_to_uv(0.0, 0.0, orient, 1.0)])
        for wx, wy in ((0.3, 0.7), (0.9, 0.1), (0.5, 0.5)):
            u, v = world_to_uv(wx, wy, orient, 1.0)
            sol = np.linalg.solve(A, np.array([u, v]) - o[0])
            assert abs(sol[0] - wx) < 1e-9 and abs(sol[1] - wy) < 1e-9
    # 带纹理渲染：纯红 1×1 纹理三角形 → 覆盖像素为红；alpha=0 纹理 → 不覆盖
    red = np.zeros((1, 1, 4), np.float32); red[0, 0] = [1, 0, 0, 1]
    tr = np.array([[[0, 0, 5], [10, 0, 5], [0, 10, 5]]], np.float64)
    uvf = np.array([[[0, 0], [1, 0], [0, 1]]], np.float32)
    m, cb = render_textured([{"tris": tr, "uv": uvf, "vcol": None,
                              "factor": np.array([1, 1, 1, 1], np.float32), "tex": red}], 100, span, "xy")
    assert m.any() and abs(cb[m].mean(axis=0)[0] - 1.0) < 1e-5 and cb[m].mean(axis=0)[1] < 1e-5
    clear = np.zeros((1, 1, 4), np.float32); clear[0, 0] = [1, 0, 0, 0]   # 全透明
    m2, _ = render_textured([{"tris": tr, "uv": uvf, "vcol": None,
                              "factor": np.array([1, 1, 1, 1], np.float32), "tex": clear}], 100, span, "xy")
    assert not m2.any()
    print("self-test: ok")


def _bake_worker(job):
    """multiprocessing 工作函数（--jobs > 1 时按图并行；各图写各自目录，无竞争）。"""
    pack, key, write, quality, min_score = job
    try:
        return bake_map(Path(pack), key, write, quality, min_score)
    except Exception as e:  # noqa: BLE001 — 单图失败不断链
        return {"key": key, "error": str(e)}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pack", default="release/asset_pack")
    ap.add_argument("--map", help="单张地图 key")
    ap.add_argument("--all", action="store_true", help="遍历包内全部地图")
    ap.add_argument("--write", action="store_true", help="写回 ground.webp（缺省只校准+报告）")
    ap.add_argument("--quality", type=int, default=82, help="webp 质量（默认 82）")
    ap.add_argument("--jobs", type=int, default=1, help="并行进程数（按图分片；每 worker 峰值 ~1GB）")
    ap.add_argument("--min-score", type=float, default=0.02, help="朝向校准置信门限（低于则不写，fail-closed）")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        self_test()
        return
    pack = Path(args.pack)
    keys = ([d.name for d in (pack / "map").iterdir() if d.is_dir()] if args.all
            else ([args.map] if args.map else []))
    jobs = [(str(pack), k, args.write, args.quality, args.min_score) for k in keys]
    if args.jobs > 1 and len(jobs) > 1:
        from multiprocessing import Pool
        with Pool(min(args.jobs, len(jobs))) as pool:
            for r in pool.imap_unordered(_bake_worker, jobs):
                print(json.dumps(r, ensure_ascii=False), flush=True)
        return
    for job in jobs:
        print(json.dumps(_bake_worker(job), ensure_ascii=False))


if __name__ == "__main__":
    main()
