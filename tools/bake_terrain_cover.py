"""地形"让位"掩码烘焙（贴地结构覆盖处的地形天花板）。

用法：
    python tools/bake_terrain_cover.py --map forgecity [--map ...] \
        [--pack release/asset_pack] [--cache data/cache/maps] [--stats]

背景（为什么是数据面、不是渲染端补偿）：铺装/地形与挡土墙顶、建筑基础、铁轨基板等在客户端是
**吸附齐平**的（实测沿缝 92% |Δ|≤1 cm，压顶 4% 高出 5–7 cm、铁轨一带真值恒定 0 覆盖）。客户端
的地形是按视距自适应 LOD 的补片网格（容许误差 = 0.014·距离·tan(fov/2)，最大 3 m），我们的观看
相机离目标远，同一处地形会被渲染成粗格插值，**合法地**拱到这些薄构件之上（实测 +35~44 cm@300–450 m）。
渲染端任何"偏移/抬升"都是手工补偿且与客户端无关；这里改为把作者意图显式化：**结构面就是这里的地面
高度**，逐 texel 记下来，前端只把**渲染用**高度场夹到该天花板之下（查询/放置用的高度场不动）。

烘焙口径（可与客户端对照，不含需要目视调参的量）：
  1. 只取**朝上**面片（世界上下分量 > 0.5×法线长）：竖直墙面、桥腹等不参与；
  2. 逐 texel 取该处**最高**的朝上面高度（同一 texel 多面片取最高者）；
  3. **贴地判定**：|结构面 − 地形| ≤ `--band`（默认 0.8 m）——实测要修的是 ≤0.5 m 的戳出，
     而离地 ≥1 m 的桥/高架不在此列（两者之间取 0.8 m 分开）；
  4. **单向天花板 + 边缘坡道**：覆盖处天花板 = 结构面；覆盖外 `--margin` 个 texel 内（默认 8 =
     网格最粗层一格 = 8 texel）按距离线性升到原地形高度；**只压不抬**（地形本来低于结构面时
     天花板不生效）⇒ 不会在低处开槽，也不会改动离地结构下方的地形；
  5. 量化与地形一致（u16 / zmin..zmax），存 `ceil + 1`（**0 = 无覆盖**，避免真值 0 与哨兵冲突）。

输出：`<pack>/map/<key>/cover.u16.bin`（512² u16 LE）+ `terrain.json` 增加 `cover` 字段；
同时写 `<cache>/<space>.cover.u16.bin`（供 export_asset_pack.py 随包复制）。**COS 同步由发布者执行。**
"""

from __future__ import annotations

import argparse
import json
import pathlib
import struct
import sys

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

BAND_LOW_DEFAULT = 0.15   # 结构面可比地形**低**多少（米）仍算"吸附齐平"：实测齐平缝 |Δ|≤1 cm，
                          # 略低者 75% ≤2 cm；0.15 m 容下"吸附设计"而排除"埋在铺装下 ≥0.3 m 的箱体"
BAND_HIGH_DEFAULT = 0.5   # 结构面可比地形**高**多少（米）仍算贴地低矮构件（铁轨基板实测高出 0.27 m）
PRESS_DEFAULT = 0.5       # 压低上限（米）：降低量恒 ≤ 此值 ⇒ 不会在陡坡旁挖沟、不会在大范围显出凹陷
CLEARANCE_DEFAULT = 0.1   # **几何间隙**（米）：压到"结构面下方"而不是压到结构面上——压到面上会让
                          # 地形与结构**共面**，深度测试逐像素抢胜 ⇒ 交叠闪烁（2026-10-10 用户
                          # "交叠闪烁更严重了"）。留 0.1 m ⇒ 带内地形处处干净低于结构面。
MARGIN_DEFAULT = 20     # 收尾带长度（texel）：≈2.5 个最粗层格（8 texel/格）⇒ 粗格插值也能被压低
UP_DOT = 0.5            # 朝上判定：世界上下分量 > UP_DOT × |法线|


# ---------------------------------------------------------------- 纯函数（单测看护）

def glb_world_xyz(scene_xyz: np.ndarray) -> np.ndarray:
    """GLB/场景系 → 世界系：世界 x = −场景 x、世界高度 = 场景 z、世界 z = 场景 y。

    与前端根组 `mapScenery.rotation.set(-π/2, π, 0)` 及 `Terrain.json` 的 worldBounds 一致
    （拣选报告里 `world:` 与 `scene:` 的换算关系）。"""
    out = np.empty_like(scene_xyz)
    out[..., 0] = -scene_xyz[..., 0]
    out[..., 1] = scene_xyz[..., 2]
    out[..., 2] = scene_xyz[..., 1]
    return out


def up_faces(world_tris: np.ndarray) -> np.ndarray:
    """朝上面片掩码：面法线的世界上下分量 > UP_DOT×|法线|。world_tris: (N,3,3) 世界 (x,y,z)。"""
    a = world_tris[:, 1] - world_tris[:, 0]
    b = world_tris[:, 2] - world_tris[:, 0]
    n = np.cross(a, b)
    len_ = np.linalg.norm(n, axis=1)
    ok = len_ > 1e-9
    out = np.zeros(len(world_tris), dtype=bool)
    out[ok] = n[ok, 1] > UP_DOT * len_[ok]
    return out


def texel_index(world_x: float, world_z: float, n: int, span: float) -> tuple[float, float]:
    """世界 (x,z) → texel 浮点索引（列 j ↔ 世界 x、行 i ↔ 世界 z；与 terrainMesh.js 同式）。"""
    return (world_z / span + 0.5) * n, (world_x / span + 0.5) * n


def classify_ground_lying(surface: np.ndarray, terrain: np.ndarray,
                          band_low: float = BAND_LOW_DEFAULT, band_high: float = BAND_HIGH_DEFAULT) -> np.ndarray:
    """贴地掩码（非对称）：结构面在 [地形 − band_low, 地形 + band_high] 内。

    非对称的理由：**吸附齐平**设计的结构面在地形之上下 1 cm 量级（band_low 取 0.15 m 足够），
    而"埋在铺装下 0.5 m 的箱体/管道"若也算贴地，压低铺装会把它挖出来 ⇒ 用紧的 band_low 排除。"""
    return np.isfinite(surface) & (surface >= terrain - band_low) & (surface <= terrain + band_high)


def envelope(nat: np.ndarray, mask_h: np.ndarray, covered: np.ndarray, margin: int, press_max: float,
             clearance: float = CLEARANCE_DEFAULT):
    """单向天花板（把地形压低，绝不抬高）：

      · 覆盖 texel（`covered`）：压低量 = min(press_max, nat − 结构面 + clearance) —— 落到**结构面下方
        clearance**（共面会 z-fight 闪烁，故留几何间隙）；
      · 覆盖外 margin 个 texel 内：同一压低量按 (1 − d/margin) 线性收尾到 0（边界连续，无台阶）。

    压低量**恒 ≤ press_max**（0.4 m）⇒ 不会在陡坡旁挖出沟槽（首版按"升到原地形"的坡道在陡坡旁
    实测可压低 3.6 m，已被此式取代）；地形本就低于结构面处压低量 = 0 ⇒ 只压不抬。

    返回 ceil（应由前端夹到 ≤ ceil；无影响处为 +inf）。"""
    h, w = nat.shape
    inf = np.inf
    ceil = np.full((h, w), inf, dtype=np.float64)
    if not covered.any():
        return ceil
    # 到最近覆盖 texel 的距离（texel 计）与该处结构面高度：两遍 chamfer 近似
    dist = np.full((h, w), inf, dtype=np.float64)
    near = np.zeros((h, w), dtype=np.float64)
    dist[covered] = 0.0
    near[covered] = mask_h[covered]
    for ys, xs in ((range(h), range(w)), (range(h - 1, -1, -1), range(w - 1, -1, -1))):
        for y in ys:
            for x in xs:
                best, bh = dist[y, x], near[y, x]
                for dy in (-1, 0, 1):
                    for dx in (-1, 0, 1):
                        yy, xx = y + dy, x + dx
                        if yy < 0 or xx < 0 or yy >= h or xx >= w:
                            continue
                        d2 = dist[yy, xx] + (1.0 if dx == 0 or dy == 0 else 1.4142135623730951)
                        if d2 < best:
                            best, bh = d2, near[yy, xx]
                dist[y, x], near[y, x] = best, bh
    act = dist <= margin
    if act.any():
        press = np.clip(nat[act] - near[act] + clearance, 0.0, press_max)
        taper = 1.0 - np.clip(dist[act] / float(margin), 0.0, 1.0)
        ceil[act] = nat[act] - press * taper
    ceil = np.where(ceil < nat - 1e-9, ceil, inf)     # 只压不抬
    return ceil


def quantize(height: float, zmin: float, zmax: float) -> int:
    """高度 → u16（与地形同量化）。"""
    if not np.isfinite(height):
        return 0
    k = (zmax - zmin) / 65535.0
    v = int(round((height - zmin) / max(k, 1e-9))) + 1     # +1：0 留给"无覆盖"
    return max(1, min(65535, v))


# ---------------------------------------------------------------- GLB 读取

def read_glb(path: pathlib.Path):
    raw = path.read_bytes()
    assert raw[:4] == b"glTF", f"{path} 不是 GLB"
    off, chunks = 12, {}
    while off < len(raw):
        ln, ty = struct.unpack_from("<II", raw, off)
        chunks[ty] = raw[off + 8:off + 8 + ln]
        off += 8 + ln
    J = json.loads(chunks[0x4E4F534A])
    BIN = chunks.get(0x004E4942, b"")
    return J, BIN


def accessor(J, BIN, idx):
    a = J["accessors"][idx]
    bv = J["bufferViews"][a["bufferView"]]
    comp = {5120: ("b", 1), 5121: ("B", 1), 5122: ("h", 2), 5123: ("H", 2), 5125: ("I", 4), 5126: ("f", 4)}[a["componentType"]]
    ncomp = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
    start = (bv.get("byteOffset") or 0) + (a.get("byteOffset") or 0)
    stride = bv.get("byteStride") or comp[1] * ncomp
    out = np.empty((a["count"], ncomp), dtype=np.float64)
    for k in range(a["count"]):
        vals = struct.unpack_from("<" + comp[0] * ncomp, BIN, start + k * stride)
        out[k] = vals
    return out if ncomp > 1 else out[:, 0]


def node_matrix(nd, out=None):
    if "matrix" in nd:
        m = np.array(nd["matrix"], dtype=np.float64).reshape(4, 4).T
        return m
    t = np.array(nd.get("translation") or (0, 0, 0), dtype=np.float64)
    q = np.array(nd.get("rotation") or (0, 0, 0, 1), dtype=np.float64)
    s = np.array(nd.get("scale") or (1, 1, 1), dtype=np.float64)
    x, y, z, w = q
    r = np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ], dtype=np.float64)
    m = np.eye(4)
    m[:3, :3] = r * s[None, :]
    m[:3, 3] = t
    return m


def collect_triangles(J, BIN, material_flags=None) -> np.ndarray:
    """场景系三角形（N,3,3）。`material_flags` 传入 `(mat_idx) -> bool` 时按材质打标，
    返回值变为 `(tris, flags)`（flags 长度 = 三角形数，逐三角形）。"""
    tris, flags = [], []
    stack = [(i, np.eye(4)) for i in J["scenes"][0]["nodes"]]
    while stack:
        ni, parent = stack.pop()
        nd = J["nodes"][ni]
        world = parent @ node_matrix(nd)
        for c in nd.get("children", []):
            stack.append((c, world))
        if "mesh" not in nd:
            continue
        for prim in J["meshes"][nd["mesh"]]["primitives"]:
            attrs = prim.get("attributes") or {}
            if "POSITION" not in attrs or "indices" not in prim:
                continue
            P = accessor(J, BIN, attrs["POSITION"])
            I = accessor(J, BIN, prim["indices"]).astype(np.int64)
            ph = np.hstack([P, np.ones((len(P), 1))]) @ world.T
            V = ph[:, :3]
            tri = V[I.reshape(-1, 3)]
            tris.append(tri)
            if material_flags is not None:
                flags.append(np.full(len(tri), bool(material_flags(prim.get("material")))))
    if not tris:
        if material_flags is not None:
            return np.zeros((0, 3, 3)), np.zeros((0,), dtype=bool)
        return np.zeros((0, 3, 3))
    T = np.concatenate(tris, axis=0)
    if material_flags is not None:
        return T, np.concatenate(flags)
    return T


def submerged_faces(world_tris: np.ndarray, surfaces: list, margin: float = 0.5,
                    clearance: float = 0.05) -> np.ndarray:
    """逐三角形：是否**整个被某片水面盖住且不高于其标高**（水下件）。

    用途（2026-10-10 用户"岸线拉近看还是锯齿"的定案）：让位掩码的"贴地判定"必须**排除水面片
    自身与水下薄板**（冰面等）——把地形压到冰面之下会把**可见岸线**整体挪走（实测 malinovka
    岸线位移 2–4 texel、局部 5 格≈5.9 m，并留下台阶），而生产环境（无掩码）的岸线是平滑的
    数据等高线。水下件的粗格保护已由前端"岸线特征细分"（与水面标高相交的补片细到 1 texel）
    承担，不需要掩码再压。

    surfaces: `[(level, x0, x1, z0, z1), ...]`（世界系）。判定与前端 `underWater` 同口径：
    顶点全部落在某片水面的占地内（`margin` 米余量）且三角形最高点 ≤ 标高 + `clearance`。"""
    if not surfaces or len(world_tris) == 0:
        return np.zeros(len(world_tris), dtype=bool)
    out = np.zeros(len(world_tris), dtype=bool)
    for k, tri in enumerate(world_tris):
        x0, x1 = tri[:, 0].min(), tri[:, 0].max()
        z0, z1 = tri[:, 2].min(), tri[:, 2].max()
        ymax = tri[:, 1].max()
        for level, sx0, sx1, sz0, sz1 in surfaces:
            if (sx0 - margin <= x0 and x1 <= sx1 + margin
                    and sz0 - margin <= z0 and z1 <= sz1 + margin
                    and ymax <= level + clearance):
                out[k] = True
                break
    return out


def water_surfaces_of(world_tris: np.ndarray) -> list:
    """水面三角形 → `[(level, x0, x1, z0, z1), ...]`（逐三角形，避免大片的占地把空洞也圈进去）。"""
    out = []
    for tri in world_tris:
        out.append((float(tri[:, 1].max()), float(tri[:, 0].min()), float(tri[:, 0].max()),
                    float(tri[:, 2].min()), float(tri[:, 2].max())))
    return out


def rasterize_max_height(tris_world: np.ndarray, nat: np.ndarray, span: float) -> np.ndarray:
    """逐 texel 取最高的朝上面高度（无覆盖 = NaN）。tris_world: 世界 (x,y,z)，只喂朝上面。"""
    n = nat.shape[0]
    surface = np.full((n, n), np.nan, dtype=np.float64)
    tex = span / n
    # 粗地形 min/max（64×64 块）用于跳过大范围无关面片
    blk = max(1, n // 64)
    nb = (n + blk - 1) // blk
    pad = np.pad(nat, ((0, nb * blk - n), (0, nb * blk - n)), constant_values=np.nan)
    tmin = np.nanmin(pad.reshape(nb, blk, nb, blk).transpose(0, 2, 1, 3).reshape(nb, nb, -1), axis=2)
    tmax = np.nanmax(pad.reshape(nb, blk, nb, blk).transpose(0, 2, 1, 3).reshape(nb, nb, -1), axis=2)
    band = 0.5
    for tri in tris_world:
        ylo, yhi = tri[:, 1].min(), tri[:, 1].max()
        x0 = int(np.floor((tri[:, 0].min() / span + 0.5) * n))
        x1 = int(np.ceil((tri[:, 0].max() / span + 0.5) * n))
        z0 = int(np.floor((tri[:, 2].min() / span + 0.5) * n))
        z1 = int(np.ceil((tri[:, 2].max() / span + 0.5) * n))
        x0, x1 = max(0, x0), min(n - 1, x1)
        z0, z1 = max(0, z0), min(n - 1, z1)
        if x1 < x0 or z1 < z0:
            continue
        bi0, bi1 = z0 // blk, z1 // blk
        bj0, bj1 = x0 // blk, x1 // blk
        if yhi < np.nanmin(tmin[bi0:bi1 + 1, bj0:bj1 + 1]) - band or ylo > np.nanmax(tmax[bi0:bi1 + 1, bj0:bj1 + 1]) + band:
            continue
        # 该面片的三顶点（世界 x,z,y）
        ax, az, ay = tri[0, 0], tri[0, 2], tri[0, 1]
        bx, bz, by = tri[1, 0], tri[1, 2], tri[1, 1]
        cx, cz, cy = tri[2, 0], tri[2, 2], tri[2, 1]
        den = (bz - cz) * (ax - cx) + (cx - bx) * (az - cz)
        if abs(den) < 1e-12:
            continue
        for zi in range(z0, z1 + 1):
            zt = (zi / n - 0.5) * span
            for xi in range(x0, x1 + 1):
                xt = (xi / n - 0.5) * span
                u = ((bz - cz) * (xt - cx) + (cx - bx) * (zt - cz)) / den
                v = ((cz - az) * (xt - cx) + (ax - cx) * (zt - cz)) / den
                w = 1 - u - v
                if u < -1e-9 or v < -1e-9 or w < -1e-9:
                    continue
                y = u * ay + v * by + w * cy
                cur = surface[zi, xi]
                if not (cur >= y):          # NaN 也算
                    surface[zi, xi] = y
    return surface


# ---------------------------------------------------------------- 单图烘焙

def bake_map(map_dir: pathlib.Path, glb_path: pathlib.Path, band_low: float, band_high: float, press: float,
             margin: int, clearance: float = CLEARANCE_DEFAULT, stats: bool = False):
    meta = json.loads((map_dir / "terrain.json").read_text(encoding="utf-8"))
    n = int(meta.get("size") or 512)
    span = float(meta.get("span") or 600.0)
    zmin = float(meta.get("zmin") or 0.0)
    zmax = float(meta.get("zmax") or 100.0)
    raw = (map_dir / "terrain.u16.bin").read_bytes()
    nat = np.frombuffer(raw, dtype="<u2").astype(np.float64).reshape(n, n) * ((zmax - zmin) / 65535.0) + zmin

    J, BIN = read_glb(glb_path)
    tris, is_water = collect_triangles(
        J, BIN,
        material_flags=lambda mi: bool(mi is not None
                                       and ((J["materials"][mi].get("extras") or {}).get("water"))))
    if len(tris) == 0:
        return None
    tris_world = glb_world_xyz(tris.reshape(-1, 3)).reshape(-1, 3, 3)
    # 水面片自身 + 水下薄板都**不参与**"贴地结构"（见 submerged_faces 注释：它们会扰动可见岸线；
    # 水下件的粗格保护已由前端"岸线特征细分"承担）。
    water_surfaces = water_surfaces_of(tris_world[is_water])
    keep = up_faces(tris_world)
    submerged = submerged_faces(tris_world, water_surfaces)
    keep = keep & ~is_water & ~submerged
    up = tris_world[keep]
    surface = rasterize_max_height(up, nat, span)
    covered0 = classify_ground_lying(surface, nat, band_low, band_high)
    tex = span / n
    mask_h = np.where(np.isfinite(surface), surface, 0.0)
    ceil = envelope(nat, surface, covered0, margin, press, clearance)
    # 写盘：ceil+1（0 = 无覆盖）
    out = np.zeros((n, n), dtype="<u2")
    act = np.isfinite(ceil)
    k = (zmax - zmin) / 65535.0
    q = np.zeros((n, n), dtype=np.int64)
    q[act] = np.round((ceil[act] - zmin) / max(k, 1e-9)).astype(np.int64) + 1
    np.clip(q, 1, 65535, out=q)
    out[act] = q[act].astype("<u2")
    (map_dir / "cover.u16.bin").write_bytes(out.tobytes())
    meta["cover"] = "cover.u16.bin"
    # 覆盖处置信：**被压低的** texel 上，地形与结构面的最小间隙（应 ≥ clearance；共面风险由此暴露）
    pressed = covered0 & np.isfinite(ceil)
    gap = float(np.min(mask_h[pressed] - ceil[pressed])) if pressed.any() else None
    meta["coverStats"] = {
        # ⚠️ 一律写成有限值（None 可以）：json.dumps 默认会写 -Infinity/NaN ⇒ 前端 JSON.parse 直接失败
        "minClearance": gap if (gap is not None and np.isfinite(gap)) else None,
        "bandLow": band_low, "bandHigh": band_high, "pressMax": press, "marginTexels": margin,
        "triangles": int(len(tris)), "upFacing": int(len(up)),
        "excludedWaterFaces": int((is_water & up_faces(tris_world)).sum()),
        "excludedSubmergedFaces": int((submerged & up_faces(tris_world)).sum()),
        "texelsGroundLying": int(covered0.sum()),
        "texelsCeiling": int(act.sum()),
        "clearance": clearance,
        "maxLowering": float(np.nanmax(np.where(act, nat - ceil, np.nan))) if act.any() else 0.0,
    }
    blob = json.dumps(meta, ensure_ascii=False, allow_nan=False)   # 非有限值会在此抛错（fail-closed）
    (map_dir / "terrain.json").write_text(blob, encoding="utf-8")
    if stats:
        print(f"  {map_dir.name}: 三角形 {len(tris)}（朝上 {len(up)}）"
              f" 贴地 texel {int(covered0.sum())} → 天花板 texel {int(act.sum())}"
              f" 最大压低 {meta['coverStats']['maxLowering']:.3f} m")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--map", action="append", help="只烘指定图（可重复）；缺省全部有 scenery.glb 的图")
    ap.add_argument("--pack", type=pathlib.Path, default=pathlib.Path("release/asset_pack"))
    ap.add_argument("--cache", type=pathlib.Path, default=pathlib.Path("data/cache/maps"))
    ap.add_argument("--band-low", type=float, default=BAND_LOW_DEFAULT)
    ap.add_argument("--band-high", type=float, default=BAND_HIGH_DEFAULT)
    ap.add_argument("--press", type=float, default=PRESS_DEFAULT)
    ap.add_argument("--clearance", type=float, default=CLEARANCE_DEFAULT)
    ap.add_argument("--margin", type=int, default=MARGIN_DEFAULT)
    ap.add_argument("--stats", action="store_true")
    args = ap.parse_args()

    map_root = args.pack / "map"
    keys = args.map or sorted(p.name for p in map_root.iterdir() if (p / "scenery.glb").is_file())
    done = 0
    for key in keys:
        d = map_root / key
        glb = args.cache / f"{json.loads((d / 'terrain.json').read_text(encoding='utf-8')).get('space', key)}.glb"
        if not glb.is_file():
            glb = d / "scenery.glb"
        if not (d / "terrain.json").is_file() or not (d / "terrain.u16.bin").is_file() or not glb.is_file():
            print(f"跳过 {key}（缺 terrain.json / terrain.u16.bin / scenery.glb）")
            continue
        res = bake_map(d, glb, args.band_low, args.band_high, args.press, args.margin, args.clearance, args.stats)
        if res is None:
            print(f"跳过 {key}（无三角形）")
            continue
        space = json.loads((d / "terrain.json").read_text(encoding="utf-8")).get("space", key)
        (args.cache / f"{space}.cover.u16.bin").write_bytes(res.tobytes())
        done += 1
    print(f"完成 {done} 张图；cover.u16.bin 已写入包内并与 export_asset_pack.py 同步复制（COS 同步由发布者执行）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
