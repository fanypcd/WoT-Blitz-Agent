"""贴地结构脚印烘焙（渲染端"贴地结构带细分"判据的输入）。

用法：
    python tools/bake_ground_structs.py --map plant [--map ...] \
        [--pack release/asset_pack] [--cache data/cache/maps] [--stats]

背景（2026-10-11 撤销让位掩码后的替代机制）：贴地薄构件（铁轨基板/压顶/铺装）在远视距下会被
自适应 LOD 的粗格插值"合法地"盖住——客户端靠**贴地相机**（3–15 m，网格天然 1 texel 级）让这
不显形，回放相机在数百米外就会看到。此前 v0.4.2 的做法是把渲染用高度场逐 texel 压到结构面
之下（数据面造高度，36 图波及 249 万 texel、四个人工常数，且 450 m 残余 78 点）——已整体撤销。
现行做法**只标位置、不改高度**：逐 texel 记下"此处有贴地结构"，前端把与脚印相交的地形补片
**强制细分到 1 texel**（terrainMesh.js 第五判据）——细分不造高度，1 texel 网格 = 真值高度场
（细真值在铁轨带上恒 0 覆盖），与"岸线特征细分"（第四判据）同类：细分终点 = 数据自身分辨率。
false positive 的代价只是三角形，不会错改任何几何。

烘焙口径（与让位掩码同源的扫描，去掉全部造高度逻辑）：
  1. 只取**朝上**面片（世界上下分量 > 0.5×法线长）：竖直墙面、桥腹等不参与；
  2. 逐 texel 取该处**最高**的朝上面高度（同一 texel 多面片取最高者）；
  3. **贴地判定**：|结构面 − 地形| ≤ 带（--band-low 0.15 / --band-high 0.5）——齐平缝实测
     ≤1 cm、铁轨基板高出 0.27 m 入列、埋在铺装下 ≥0.3 m 的箱体不入列（误入列只费三角形，
     不会改几何）；
  4. **水面片与水下薄板排除**（水面片自身、整个被水面盖住且不高于其标高的件）——它们会把
     脚印铺满水体，触发无谓的全岸细分；
  5. **膨胀**（--dilate，默认 4 texel）：脚印向四周扩 4 texel——①细/粗分区缝合边上的顶点取
     粗侧直线值，结构必须离缝合边 ≥1 细格才不被缝合值抬过；②贴地判定的分类边缘误差。
     膨胀只扩大"强制细分"范围，不影响任何高度。

输出：`<pack>/map/<key>/structs.u8.bin`（512² u8，1 = 脚印）+ `terrain.json` 增 `structs` /
`structsStats` 字段（并**清除已撤销的 cover / coverStats**）；同时写
`<cache>/<space>.structs.u8.bin`（供 export_asset_pack.py 随包复制）。**COS 同步由发布者执行。**
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
DILATE_DEFAULT = 4        # 脚印膨胀（texel）：缝合边顶点取粗侧值 ⇒ 结构须离缝合边 ≥1 细格；
                          # 4 texel ≈ 4.7 m（span 600 / 512）同时盖住分类边缘的半 texel 误差
UP_DOT = 0.5              # 朝上判定：世界上下分量 > UP_DOT × |法线|


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
    而"埋在铺装下 0.5 m 的箱体/管道"若也算贴地，会铺满不需要细分的区域 ⇒ 用紧的 band_low 排除。
    本工具的判定误差只影响三角形预算（细分范围），不影响任何高度。"""
    return np.isfinite(surface) & (surface >= terrain - band_low) & (surface <= terrain + band_high)


def dilate_footprint(covered: np.ndarray, dilate: int) -> np.ndarray:
    """脚印膨胀：到最近 covered texel 的 chamfer 距离（正交 1 / 对角 √2）≤ dilate ⇒ 入脚印。

    两遍扫描（左上→右下、右下→左上），与让位掩码 envelope 的距离场同式。dilate ≤ 0 时原样返回。"""
    if dilate <= 0 or not covered.any():
        return covered.copy()
    h, w = covered.shape
    inf = float(h + w) * 2.0
    dist = np.full((h, w), inf, dtype=np.float64)
    dist[covered] = 0.0
    for ys, xs in ((range(h), range(w)), (range(h - 1, -1, -1), range(w - 1, -1, -1))):
        for y in ys:
            row = dist[y]
            for x in xs:
                best = row[x]
                for dy in (-1, 0, 1):
                    for dx in (-1, 0, 1):
                        if dy == 0 and dx == 0:
                            continue
                        yy, xx = y + dy, x + dx
                        if yy < 0 or xx < 0 or yy >= h or xx >= w:
                            continue
                        d2 = dist[yy, xx] + (1.0 if dx == 0 or dy == 0 else 1.4142135623730951)
                        if d2 < best:
                            best = d2
                row[x] = best
    return dist <= float(dilate)


def footprint_u8(footprint: np.ndarray) -> np.ndarray:
    """脚印 → u8（1 = 脚印 texel，0 = 无）。"""
    return footprint.astype(np.uint8)


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

    用途：脚印的"贴地判定"必须**排除水面片自身与水下薄板**（冰面等）——否则脚印铺满水体，
    触发整片无谓的岸上细分（让位掩码时代 2026-10-10 同一教训：水体的粗格保护由前端
    "岸线特征细分"承担，不需要这里管）。

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

def bake_map(map_dir: pathlib.Path, glb_path: pathlib.Path, band_low: float, band_high: float,
             dilate: int, stats: bool = False):
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
    water_surfaces = water_surfaces_of(tris_world[is_water])
    keep = up_faces(tris_world)
    submerged = submerged_faces(tris_world, water_surfaces)
    keep = keep & ~is_water & ~submerged
    up = tris_world[keep]
    surface = rasterize_max_height(up, nat, span)
    covered = classify_ground_lying(surface, nat, band_low, band_high)
    footprint = dilate_footprint(covered, dilate)

    out = footprint_u8(footprint)
    (map_dir / "structs.u8.bin").write_bytes(out.tobytes())
    meta.pop("cover", None)          # 让位掩码已撤销（2026-10-11）：清掉旧字段与统计
    meta.pop("coverStats", None)
    meta["structs"] = "structs.u8.bin"
    meta["structsStats"] = {
        "bandLow": band_low, "bandHigh": band_high, "dilateTexels": dilate,
        "triangles": int(len(tris)), "upFacing": int(len(up)),
        "excludedWaterFaces": int((is_water & up_faces(tris_world)).sum()),
        "excludedSubmergedFaces": int((submerged & up_faces(tris_world)).sum()),
        "texelsGroundLying": int(covered.sum()),
        "texelsFootprint": int(footprint.sum()),
    }
    blob = json.dumps(meta, ensure_ascii=False, allow_nan=False)   # 非有限值会在此抛错（fail-closed）
    (map_dir / "terrain.json").write_text(blob, encoding="utf-8")
    if stats:
        print(f"  {map_dir.name}: 三角形 {len(tris)}（朝上 {len(up)}）"
              f" 贴地 texel {int(covered.sum())} → 膨胀脚印 {int(footprint.sum())}"
              f"（{footprint.sum() / footprint.size * 100:.1f}% 面积）")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--map", action="append", help="只烘指定图（可重复）；缺省全部有 scenery.glb 的图")
    ap.add_argument("--pack", type=pathlib.Path, default=pathlib.Path("release/asset_pack"))
    ap.add_argument("--cache", type=pathlib.Path, default=pathlib.Path("data/cache/maps"))
    ap.add_argument("--band-low", type=float, default=BAND_LOW_DEFAULT)
    ap.add_argument("--band-high", type=float, default=BAND_HIGH_DEFAULT)
    ap.add_argument("--dilate", type=int, default=DILATE_DEFAULT)
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
        res = bake_map(d, glb, args.band_low, args.band_high, args.dilate, args.stats)
        if res is None:
            print(f"跳过 {key}（无三角形）")
            continue
        space = json.loads((d / "terrain.json").read_text(encoding="utf-8")).get("space", key)
        (args.cache / f"{space}.structs.u8.bin").write_bytes(res.tobytes())
        done += 1
    print(f"完成 {done} 张图；structs.u8.bin 已写入包内并与 export_asset_pack.py 同步复制（COS 同步由发布者执行）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
