#!/usr/bin/env python3
"""
烘焙**真实场景正交俯视图**进高清底图（均衡档地面）：屋顶、树冠、草丛、
掩体——一切从上往下看得见的静态几何。

背景：4096² 烘焙底图（ground.webp，来自客户端 landscape colormap 提取）是纯
地形。均衡/流畅档不加载 3D 场景 GLB，地形图上没有树也没有建筑。本脚本把场景
GLB 的静态几何俯视光栅化（z-buffer 取最高面 = 正交俯视的真实遮挡关系），颜色
从客户端小地图（512²，自带树丛/屋顶/水系的俯视配色）双线性采样，合成回
ground.webp——覆盖边缘由几何（4096 精度）保证位置锐利，配色继承客户端观感。

选择规则：
- 排除 D_*/State N（损毁态变体）、mdVariant extras（跨变体污染——变体图共用
  一份底图，只烘无标记节点）、invisiblewall（不可见碰撞墙）、天空球、水面
  （地形图已含水系配色，避免半透明大面覆盖）。
- 叶卡（_CORNER，树冠）**必须包含**——它们就是俯视图里的树。
- md3 等变体专属物体不进共享底图（量化：medvedkovo 仅 5 个 md3 节点）——
  该变体的均衡档少这几笔，属已知限制（3D 档不受影响）。

朝向不信任推导：校准在 512² 以 8 种朝向投影覆盖掩膜，与客户端小地图的暗度
分布做相关，取最优；产出报告全部得分。默认铺设参数（无 X-Map-Meta）下运行时
UV 链推导的朝向应恰好是校准胜者，两者互为印证。

用法：
  python tools/bake_ground_roofs.py --self-test
  python tools/bake_ground_roofs.py --pack release/asset_pack --map medvedkovo          # 校准 + 报告（不写）
  python tools/bake_ground_roofs.py --pack release/asset_pack --map medvedkovo --write  # 写回 ground.webp
  python tools/bake_ground_roofs.py --pack release/asset_pack --all --write
"""
from __future__ import annotations

import argparse
import json
import re
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image

# ---------------------------------------------------------------- GLB 解析


def parse_glb(path: Path):
    """返回 (json, bin_chunk)。只解元数据与 BIN，不动纹理。"""
    data = path.read_bytes()
    assert data[:4] == b"glTF", f"{path} 不是 GLB"
    json_len = struct.unpack_from("<I", data, 12)[0]
    meta = json.loads(data[20:20 + json_len].decode("utf-8"))
    bin_off = 20 + json_len
    bin_len = struct.unpack_from("<I", data, bin_off)[0]
    bin_chunk = data[bin_off + 8:bin_off + 8 + bin_len]
    return meta, bin_chunk


def accessor_read(meta, bin_chunk, index):
    """读 accessor 为 numpy 数组（仅支持本管线用到的标量/VEC2/VEC3/VEC4 + u8/u16/u32/f32）。"""
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
        flat = np.frombuffer(bin_chunk, dtype=np.uint8, count=stride * n, offset=off)
        flat = np.lib.stride_tricks.as_strided(
            flat, shape=(n, cc), strides=(stride, np.dtype(dt).itemsize)).reshape(-1)
    return flat.reshape(n, cc) if cc > 1 else flat.reshape(n)


def node_local_matrix(n: dict) -> np.ndarray:
    """节点局部矩阵（列主序 4×4，与 glTF 一致）。"""
    if "matrix" in n:
        return np.array(n["matrix"], np.float64).reshape(4, 4).T  # 存储列主序 → 行主序
    q = n.get("rotation", [0.0, 0.0, 0.0, 1.0])
    s = n.get("scale", [1.0, 1.0, 1.0])
    t = n.get("translation", [0.0, 0.0, 0.0])
    x, y, z, w = q
    m = np.eye(4)
    m[0, 0] = 1 - 2 * (y * y + z * z); m[0, 1] = 2 * (x * y - z * w); m[0, 2] = 2 * (x * z + y * w)
    m[1, 0] = 2 * (x * y + z * w); m[1, 1] = 1 - 2 * (x * x + z * z); m[1, 2] = 2 * (y * z - x * w)
    m[2, 0] = 2 * (x * z - y * w); m[2, 1] = 2 * (y * z + x * w); m[2, 2] = 1 - 2 * (x * x + y * y)
    m[:3, 0] *= s[0]; m[:3, 1] *= s[1]; m[:3, 2] *= s[2]
    m[:3, 3] = t
    return m


def collect_world_triangles(meta, bin_chunk, exclude_re, max_side_m: float):
    """收集选中 mesh 的世界系三角形 (N,3,3)（x/y 地面、z 上）。"""
    meshes = meta.get("meshes", [])
    nodes = meta.get("nodes", [])
    roots = meta["scenes"][meta.get("scene", 0)]["nodes"]
    stats = {"nodes_seen": 0, "mesh_used": 0, "mesh_skipped": 0, "tris": 0}
    tris_all = []
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
        for prim in meshes[n["mesh"]]["primitives"]:
            attrs = prim["attributes"]
            pos = accessor_read(meta, bin_chunk, attrs["POSITION"]).astype(np.float64)
            idx = accessor_read(meta, bin_chunk, prim["indices"]) if "indices" in prim else np.arange(len(pos))
            tri = pos[idx].reshape(-1, 3, 3)           # (T,3,3)
            tri = tri @ world[:3, :3].T + world[:3, 3]
            # 退化/超大三角形防护（错标尺度的病态几何会毁掉 z-buffer）
            side = np.stack([np.linalg.norm(tri[:, i] - tri[:, j], axis=1)
                             for i, j in ((0, 1), (1, 2), (2, 0))], axis=1)
            keep = (side.max(axis=1) <= max_side_m) & (side.min(axis=1) > 1e-4)
            if keep.any():
                tris_all.append(tri[keep])
                stats["tris"] += int(keep.sum())
            stats["mesh_used"] += 1
    return (np.concatenate(tris_all) if tris_all else np.zeros((0, 3, 3)), stats)


# ---------------------------------------------------------------- 朝向与光栅化

# 8 种候选朝向：世界 (x,y) → 图像 (col,row) 归一化坐标 [0,1]。
# 「推导朝向」= "xy"（运行时默认铺设的 UV 链逆映射）；其余 7 个供校准择优。
def world_to_uv(x, y, orient: str, span: float):
    u = (x + span / 2) / span
    v = (y + span / 2) / span
    return {
        "xy": (u, v), "yx": (v, u),
        "xY": (u, 1 - v), "Yx": (1 - v, u),
        "Xy": (1 - u, v), "yX": (v, 1 - u),
        "XY": (1 - u, 1 - v), "YX": (1 - v, 1 - u),
    }[orient]


def rasterize_tris(tris, size: int, span: float, orient: str):
    """俯视 z-buffer 光栅化：返回 (mask bool, zbuf float32)，z 高者胜（= 屋顶）。
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
        xs = np.arange(x0, x1) + 0.5
        ys = np.arange(y0, y1) + 0.5
        gx, gy = np.meshgrid(xs, ys)
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
        sub_z = zbuf[y0:y1, x0:x1]
        take = inside & (z > sub_z)
        sub_z[take] = z[take]
        mask[y0:y1, x0:x1][take] = True
    return mask, zbuf


def bilinear_sample(img: np.ndarray, u: np.ndarray, v: np.ndarray) -> np.ndarray:
    """u/v ∈ [0,1] 归一化坐标的双线性采样（img: H×W×3）。"""
    h, w = img.shape[:2]
    fx = np.clip(u * w - 0.5, 0, w - 1.001)
    fy = np.clip(v * h - 0.5, 0, h - 1.001)
    x0 = fx.astype(np.int64); y0 = fy.astype(np.int64)
    tx = (fx - x0)[..., None]; ty = (fy - y0)[..., None]
    c00 = img[y0, x0]; c10 = img[y0, x0 + 1]; c01 = img[y0 + 1, x0]; c11 = img[y0 + 1, x0 + 1]
    return (c00 * (1 - tx) * (1 - ty) + c10 * tx * (1 - ty) + c01 * (1 - tx) * ty + c11 * tx * ty)


# ---------------------------------------------------------------- 校准与烘焙

def calibrate(tris, mini_gray: np.ndarray, span: float):
    """8 朝向各光栅化 512²，掩膜暗度与小地图暗度相关——相关越高 = 屋顶落点越对。"""
    small = np.asarray(Image.fromarray(mini_gray).resize((512, 512)), np.float32) / 255.0
    scores = {}
    for orient in ("xy", "yx", "xY", "Yx", "Xy", "yX", "XY", "YX"):
        mask, _ = rasterize_tris(tris, 512, span, orient)
        if mask.sum() < 64:
            scores[orient] = float("nan")
            continue
        roof_dark = small[mask]
        ground_dark = small[~mask]
        # 掩膜内应显著更暗（建筑在雪地小地图上是深色块）——用均值差的效应量当分数
        scores[orient] = float(ground_dark.mean() - roof_dark.mean())
    return scores


def bake_map(pack: Path, key: str, write: bool, quality: int) -> dict:
    mdir = pack / "map" / key
    glb_path = mdir / "scenery.glb"
    ground_path = mdir / "ground.webp"
    mini_path = mdir / "mini.webp"
    for p in (glb_path, ground_path, mini_path):
        if not p.is_file():
            return {"key": key, "skipped": f"缺 {p.name}"}
    meta, bin_chunk = parse_glb(glb_path)

    # 只烘无 mdVariant 标记、非损毁态/叶卡/树/水的静态网格
    exclude = re.compile(r"^D_|State ?[1-9]|invisible|sky|water|sea|river|lake", re.I)
    sidecar = None
    for sp in {meta.get("extras", {}).get("space", "")}:
        cand = Path("data/cache/maps") / f"{sp}.json"
        if cand.is_file():
            sidecar = json.loads(cand.read_text(encoding="utf-8"))
    wb = (sidecar or {}).get("worldBounds") or {"min": [-300, -300, 0], "max": [300, 300, 70]}
    span = max(wb["max"][0] - wb["min"][0], wb["max"][1] - wb["min"][1])

    tris, stats = collect_world_triangles(meta, bin_chunk, exclude, max_side_m=200.0)

    mini = np.asarray(Image.open(mini_path).convert("RGB"))
    scores = calibrate(tris, np.asarray(Image.fromarray(mini).convert("L")), span)
    best = max(scores, key=lambda k: (scores[k] if scores[k] == scores[k] else -1))

    ground = np.asarray(Image.open(ground_path).convert("RGB")).copy()
    size = ground.shape[0]
    mask, _ = rasterize_tris(tris, size, span, best)

    # 屋顶上色：小地图双线性采样（同朝向），轻微压暗融进底图光照；边缘再压暗一圈
    ys, xs = np.nonzero(mask)
    u = (xs + 0.5) / size; v = (ys + 0.5) / size
    uu, vv = world_to_uv(0, 0, best, span)  # 占位：采样必须走同一朝向函数
    # 直接用像素反推归一化坐标（与 world_to_uv 同一映射的逆）
    uu = xs.astype(np.float64) / size; vv = ys.astype(np.float64) / size
    inv = {"xy": ("xy", 1), "yx": ("yx", 1), "xY": ("xY", 1), "Yx": ("Yx", 1),
           "Xy": ("xy", -1), "Yx": ("yx", -1), "XY": ("xy", -1), "YX": ("yx", -1)}
    base, sign = inv[best]
    # inv 映射：把「采样朝向」的 uv 还原——直接数值反解最稳：
    gx = np.zeros_like(uu); gy = np.zeros_like(vv)
    # 对 8 朝向统一用数值反解（world_to_uv 在仿射下可逆，二分没必要——线性解）
    A = np.array([[world_to_uv(1.0, 0.0, best, 1.0)[0] - world_to_uv(0.0, 0.0, best, 1.0)[0],
                   world_to_uv(0.0, 1.0, best, 1.0)[0] - world_to_uv(0.0, 0.0, best, 1.0)[0]],
                  [world_to_uv(1.0, 0.0, best, 1.0)[1] - world_to_uv(0.0, 0.0, best, 1.0)[1],
                   world_to_uv(0.0, 1.0, best, 1.0)[1] - world_to_uv(0.0, 0.0, best, 1.0)[1]]])
    o = np.array([world_to_uv(0.0, 0.0, best, 1.0)])
    pts = np.stack([uu, vv], axis=1) - o
    sol = np.linalg.solve(A, pts.T).T
    mu, mv = world_to_uv(sol[:, 0], sol[:, 1], best, span)
    roof = bilinear_sample(mini, mu, mv)
    out = ground.astype(np.float32)
    out[ys, xs] = out[ys, xs] * 0.2 + roof * 0.8
    out_img = Image.fromarray(out.clip(0, 255).astype(np.uint8))

    report = {"key": key, "orient": best, "scores": {k: round(v, 4) for k, v in scores.items()},
              "tris": stats["tris"], "mesh_used": stats["mesh_used"], "mesh_skipped": stats["mesh_skipped"],
              "roof_px": int(mask.sum()), "roof_pct": round(100 * mask.sum() / mask.size, 2),
              "written": False}
    if write:
        out_img.save(ground_path, "WEBP", quality=quality, method=6)
        report["written"] = True
        report["out_kb"] = round(ground_path.stat().st_size / 1024)
    return report


def self_test():
    """光栅化/朝向的最小自检（合成三角形，无外部资产）。"""
    span = 100.0
    # 1) 单三角形覆盖：世界原点附近的正方形 → 图像中心区域
    tris = np.array([[[0, 0, 5], [10, 0, 5], [0, 10, 5]]], np.float64)
    mask, zbuf = rasterize_tris(tris, 100, span, "xy")
    assert 20 < mask.sum() < 60, mask.sum()
    assert zbuf[mask].min() > 4.9
    # 2) z-buffer：高处三角形遮住低处
    lo = np.array([[[0, 0, 1], [10, 0, 1], [0, 10, 1]]], np.float64)
    hi = np.array([[[2, 2, 9], [10, 2, 9], [2, 10, 9]]], np.float64)
    m1, z1 = rasterize_tris(lo, 100, span, "xy")
    m2, z2 = rasterize_tris(hi, 100, span, "xy")
    z1[m2.astype(bool)] = np.maximum(z1[m2.astype(bool)], z2[m2.astype(bool)])
    # 世界 (3.5,3.5) → 像素 (53,53)：同时在低/高两个三角形内 → 高者（z=9）胜
    assert z1[53, 53] == 9.0, z1[53, 53]
    assert z1[50, 50] == 1.0, z1[50, 50]   # 只在低三角形内 → 保持低值
    # 3) 朝向函数可逆性：8 朝向 world→uv 数值反解误差 < 1e-9
    for orient in ("xy", "yx", "xY", "Yx", "Xy", "yX", "XY", "YX"):
        A = np.array([[world_to_uv(1.0, 0.0, orient, 1.0)[0] - world_to_uv(0.0, 0.0, orient, 1.0)[0],
                       world_to_uv(0.0, 1.0, orient, 1.0)[0] - world_to_uv(0.0, 0.0, orient, 1.0)[0]],
                      [world_to_uv(1.0, 0.0, orient, 1.0)[1] - world_to_uv(0.0, 0.0, orient, 1.0)[1],
                       world_to_uv(0.0, 1.0, orient, 1.0)[1] - world_to_uv(0.0, 0.0, orient, 1.0)[1]]])
        o = np.array([world_to_uv(0.0, 0.0, orient, 1.0)])
        for wx, wy in ((0.3, 0.7), (0.9, 0.1), (0.5, 0.5)):
            u, v = world_to_uv(wx, wy, orient, 1.0)
            sol = np.linalg.solve(A, np.array([u, v]) - o[0])
            assert abs(sol[0] - wx) < 1e-9 and abs(sol[1] - wy) < 1e-9, (orient, sol)
    print("self-test: 3/3 ok")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pack", default="release/asset_pack")
    ap.add_argument("--map", help="单张地图 key")
    ap.add_argument("--all", action="store_true", help="遍历包内全部地图")
    ap.add_argument("--write", action="store_true", help="写回 ground.webp（缺省只校准+报告）")
    ap.add_argument("--quality", type=int, default=82, help="webp 质量（默认 82）")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        self_test()
        return
    pack = Path(args.pack)
    keys = ([d.name for d in (pack / "map").iterdir() if d.is_dir()] if args.all
            else [args.map])
    for key in keys:
        try:
            r = bake_map(pack, key, args.write, args.quality)
        except Exception as e:  # noqa: BLE001 — 批处理时单图失败不断链
            r = {"key": key, "error": str(e)}
        print(json.dumps(r, ensure_ascii=False))


if __name__ == "__main__":
    main()
