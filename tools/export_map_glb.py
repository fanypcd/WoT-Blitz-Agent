#!/usr/bin/env python3
"""导出回放页 3D 场景模型（建筑/桥/岩石等静态地物）为单文件 GLB + 高清地面贴图。

数据源：本机 WoTB 客户端 3d/Maps/<space>/<space>.sc2[.dvpl] + 同名 .scg[.dvpl]，
地面贴图取 landscape/ 下 colormap（2048² DXT5，分辨率约为小地图 4 倍）。
提取契约（可见性位 / LOD / switch 选择）沿用 WotbTools（MIT，见 tools/wotbtools/）的
export_map_geometry_poc 研究结论：SC2 RenderComponent → Mesh → ro.flags bit0 →
batch lodIndex/switchIndex（-1 通配）→ rb.datasource → SCG PolygonGroup。

输出：
    glb_cache/maps/<MapName>.glb          场景模型（游戏世界系，米，z 上、+y 北）
    glb_cache/maps/<MapName>.ground.webp  高清地面贴图（2048²，与底图同向：上=+z）
前端加载后用 qFrame（Ry(π)·Rx(-π/2)）旋转到回放场景系（与坦克 GLB 同一约定），
不做镜像烘焙——旋转是纯旋转，无绕序问题。

用法：
    python tools/export_map_glb.py                 # 导出全部 26 图
    python tools/export_map_glb.py --map WinterMalinovka --map MayanRuins
"""

from __future__ import annotations

import argparse
import colorsys
import hashlib
import io
import json
import math
import pathlib
import struct
import sys
from collections import Counter

import numpy as np

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

try:
    import imagecodecs
except ImportError as exc:  # 地面贴图导出需要
    imagecodecs = None

try:
    from PIL import Image, ImageEnhance
except ImportError as exc:
    Image = ImageEnhance = None

from wotb_sc2 import Reader, decode_dvpl, read_archive, read_sc2  # noqa: E402
from wotb_scg import (  # noqa: E402
    decode_bytes,
    decode_polygon_indices,
    decode_polygon_positions,
    polygon_groups_by_id,
    read_scg,
)
from export_map_geometry_poc import collect_instances, iter_entities_recursive  # noqa: E402

# 枚举名 → 3d/Maps 空间目录（与 src/wargaming/map_assets.rs MAP_SPACES 一致）
MAP_SPACES = {
    "DesertSands": "02_desert_train_dt",
    "Middleburg": "03_erlenberg_er",
    "Copperfield": "23_karieri_kr",
    "Alpenstadt": "31_lumber_lm",
    "Mines": "06_rudniki_rd",
    "DeadRail": "04_medvedkovo_md",
    "FortDespair": "07_fort_ft",
    "Himmelsdorf": "19_himmelsdorf_hm",
    "BlackGoldville": "21_mountain_mnt",
    "OasisPalms": "09_savanna_sv",
    "GhostFactory": "11_plant_pn",
    "Molendijk": "16_holland_hl",
    "PortBay": "14_port_pt",
    "WinterMalinovka": "12_malinovka_ma",
    "Castilla": "13_pliego_pl",
    "Canal": "18_canal_cn",
    "Vineyards": "22_italy_it",
    "YamatoHarbor": "24_milibase_mlb",
    "Canyon": "25_canyon_ca",
    "MayanRuins": "28_rock_rc",
    "DynastyPearl": "30_grossberg_sh",
    "NavalFrontier": "29_skit_sk",
    "FallsCreek": "05_amigosville_am",
    "NewBay": "34_forgecity_fc",
    "Normandy": "33_neptune_nt",
    "Wasteland": "26_holmeisk_hk",
}


def find_member(directory: pathlib.Path, space: str, suffix: str) -> pathlib.Path | None:
    """定位 <space>.sc2.dvpl / 兜底同后缀 glob（老图文件名不同）。"""
    exact = directory / f"{space}{suffix}"
    if exact.exists():
        return exact
    for p in sorted(directory.glob(f"*{suffix}")):
        return p
    return None


def load_payload(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return decode_dvpl(raw) if path.name.lower().endswith(".dvpl") else raw


# ---------- 顶点布局（SCG 交错顶点） ----------
# 由 7 种实测 vertexFormat 反推并数值验证（bit 求和 == stride，UV 落在 [0,1]）：
#   bit0=位置12B、bit1=法线12B、bit2=颜色4B、bit3=UV0 8B、bit4=UV1 8B、
#   bit7=切线12B、bit8=副切线12B、bit9/12/13=其他扩展；按位序顺序排列。
VERTEX_LAYOUT_BITS = {0: 12, 1: 12, 2: 4, 3: 8, 4: 8, 7: 12, 8: 12, 9: 16, 10: 8, 12: 12, 13: 16}


def decode_group_uvs(group: dict) -> list[tuple[float, float]] | None:
    """解出 diffuse UV0（float2/顶点）；布局未知或该格式无 UV0 时返回 None。"""
    vf = group.get("vertexFormat")
    vc = group.get("vertexCount")
    payload = decode_bytes(group.get("vertices"))
    if not isinstance(vf, int) or not isinstance(vc, int) or vc <= 0 or payload is None:
        return None
    stride, ok = divmod(len(payload), vc)
    if ok or stride < 12:
        return None
    offsets = {}
    o = 0
    for b in range(16):
        if vf >> b & 1:
            sz = VERTEX_LAYOUT_BITS.get(b)
            if sz is None:
                return None  # 未知扩展位：布局不可靠
            if b == 3:
                offsets["uv"] = o
            o += sz
    if o != stride or "uv" not in offsets:
        return None
    uv_off = offsets["uv"]
    arr = np.frombuffer(payload, dtype=np.uint8).reshape(vc, stride)
    out = []
    for i in range(vc):
        u, v = struct.unpack_from("<ff", arr[i].tobytes(), uv_off)
        if not (math.isfinite(u) and math.isfinite(v)):
            return None
        out.append((u, v))
    # 合理性：UV 应集中在有限范围（平铺贴图一般 <8）
    u_arr = np.array([p[0] for p in out]); v_arr = np.array([p[1] for p in out])
    if abs(u_arr).max() > 16 or abs(v_arr).max() > 16:
        return None
    return out


def extract_string_table(raw: bytes) -> dict[int, str]:
    """从 SC2 KeyedArchive 头部提取 fastname 字符串表（id → 字符串）。"""
    reader = Reader(raw)
    reader.take(4)
    reader.u32(); reader.u32()
    read_archive(reader)
    desc = reader.u32()
    reader.take(desc)
    reader.take(2)
    ver = reader.u16()
    if ver != 2:
        return {}
    n = reader.u32()
    strings = [reader.text(reader.u16()) for _ in range(n)]
    ids = [reader.u32() for _ in range(n)]
    return dict(zip(ids, strings, strict=True))


def read_dds_image(path: pathlib.Path, max_dim: int = 1024) -> Image.Image | None:
    """解码 .dx11.dds.dvpl（DXT1/3/5）为 PIL RGB，长边超限则等比缩小。"""
    d = decode_dvpl(path.read_bytes())
    if d[:4] != b"DDS ":
        return None
    height = struct.unpack_from("<I", d, 12)[0]
    width = struct.unpack_from("<I", d, 16)[0]
    bcn = {"DXT1": 1, "DXT3": 2, "DXT5": 3}.get(d[84:88].decode(errors="replace"))
    if bcn is None:
        return None
    block = 8 if bcn == 1 else 16
    data = d[128:128 + (width // 4) * (height // 4) * block]
    rgba = imagecodecs.bcn_decode(data, bcn, shape=(height, width, 4))
    img = Image.frombytes("RGBA", (width, height), rgba)
    img = img.transpose(Image.FLIP_TOP_BOTTOM)
    if max(img.size) > max_dim:
        img.thumbnail((max_dim, max_dim), Image.LANCZOS)
    return img.convert("RGB")


def build_texture_index(roots) -> dict:
    """扫描客户端 3d/Maps 树，建 贴图基础名 → 文件 索引（小写归一，去 .dvpl/.dds 后缀）。"""
    idx = {}
    suffixes = (".dx11.dds.dvpl", ".dx11.pvr.dvpl", ".dds.dvpl", ".pvr.dvpl")
    for root in roots:
        root = pathlib.Path(root)
        if not root.exists():
            continue
        for p in root.rglob("*"):
            if not p.is_file() or p.suffix != ".dvpl":
                continue
            low = p.name.lower()
            if ".dds" not in low and ".pvr" not in low:
                continue
            for suf in suffixes:
                if low.endswith(suf):
                    idx.setdefault(low[:-len(suf)], p)
                    break
    return idx


_TEX_INDEX_CACHE: dict = {}


def get_texture_index(game_data) -> dict:
    key = str(game_data)
    if key not in _TEX_INDEX_CACHE:
        _TEX_INDEX_CACHE[key] = build_texture_index([pathlib.Path(game_data) / "3d" / "Maps"])
    return _TEX_INDEX_CACHE[key]


def find_texture_file(strings, building: str, maps_root, tex_index=None):
    """按建筑名（bld_12_barn.sc2 → bld_12_barn）找贴图：
    1) 字符串表的 .tex 路径 → 实际文件变体；
    2) 全局贴图索引精确名 / 前缀匹配（覆盖字符串表没登记的道具贴图）。"""
    stem = building.removesuffix(".sc2").lower()
    want = f"/{stem}.tex"
    rel = None
    values = strings if isinstance(strings, list) else strings.values()
    for v in values:
        if isinstance(v, str) and v.lower().endswith(want):
            rel = v
            break
    if rel is not None:
        sub = rel.replace("../", "")
        base_dir = maps_root / pathlib.Path(sub).parent
        base_name = pathlib.Path(sub).stem  # bld_12_barn
        for variant in (f"{base_name}.dx11.dds.dvpl", f"{base_name}.dds.dvpl",
                        f"{base_name}.dx11.pvr.dvpl", f"{base_name}.tex.dvpl"):
            cand = base_dir / variant
            if cand.exists():
                return cand
    if tex_index:
        if stem in tex_index:
            return tex_index[stem]
        for key, path in tex_index.items():
            if key.startswith(stem) or stem.startswith(key):
                return path
    return None


def strip_to_triangles(seq: list[int]) -> list[int]:
    """三角条带 → 三角形列表（标准交替绕序转换；退化三角形自然无害）。"""
    out: list[int] = []
    for i in range(2, len(seq)):
        a, b, c = seq[i - 2], seq[i - 1], seq[i]
        if a == b or b == c or a == c:
            continue  # 条带拼接产生的退化三角形
        if i % 2 == 0:
            out.extend((a, b, c))
        else:
            out.extend((a, c, b))
    return out


def build_path_index(scene: dict) -> dict[str, dict]:
    """把 #hierarchy 树展开为 entityPath → 实体字典 的索引。

    entityPath 与 collect_instances 输出同格式：'$.#hierarchy[110].#hierarchy[1]'
    （实体内 #hierarchy 为子实体列表，递归）。
    """
    index: dict[str, dict] = {}

    def walk(node: dict, key: str) -> None:
        index[key] = node
        hierarchy = node.get("#hierarchy")
        if not isinstance(hierarchy, list):
            return
        for i, child in enumerate(hierarchy):
            if isinstance(child, dict):
                walk(child, f"{key}.#hierarchy[{i}]")

    walk(scene, "$")
    return index


def resolve_building_name(path: str, index: dict[str, dict]) -> str | None:
    """实例路径向上找最近的具名祖先（SwitchNode 层不具名，建筑实体在父级）。"""
    parts = path.split(".")
    while parts:
        entity = index.get(".".join(parts))
        if entity is not None:
            name = entity.get("name")
            if isinstance(name, str) and name and name != "SwitchNode State 0":
                return name
        parts.pop()
    return None


# 建筑类目 → 暖色系基色（sRGB）。按实体名关键词归类，比随机蓝灰更接近真实观感。
CATEGORY_PALETTES: list[tuple[tuple[str, ...], tuple[float, float, float]]] = [
    (("izba", "house", "piggery", "kennel", "farm"), (0.64, 0.53, 0.40)),   # 木屋暖木色
    (("barn", "mill", "wooden", "lumber", "forge", "windmill"), (0.58, 0.42, 0.32)),  # 红棕
    (("church", "castle", "fort", "tower", "townhall"), (0.70, 0.66, 0.58)),          # 石造暖灰
    (("hangar", "plant", "factory", "warehouse", "depot"), (0.55, 0.55, 0.54)),       # 工业灰
    (("bridge", "rail", "platform", "pier"), (0.48, 0.45, 0.42)),                     # 桥梁深灰
    (("rock", "stone", "cliff"), (0.52, 0.50, 0.47)),                                 # 岩石
]

# 高度场 zMax（与 src/wargaming/map_assets.rs MAP_ZMAX 一致），悬浮过滤用
MAP_ZMAX = {
    "DesertSands": 100.0, "Middleburg": 150.0, "Copperfield": 120.0, "Alpenstadt": 180.0,
    "Mines": 135.0, "DeadRail": 70.0, "FortDespair": 70.0, "Himmelsdorf": 70.0,
    "BlackGoldville": 120.0, "OasisPalms": 80.0, "GhostFactory": 80.0, "Molendijk": 80.0,
    "PortBay": 70.0, "WinterMalinovka": 60.0, "Castilla": 50.0, "Canal": 140.0,
    "Vineyards": 150.0, "YamatoHarbor": 80.0, "Canyon": 100.0, "MayanRuins": 80.0,
    "DynastyPearl": 150.0, "NavalFrontier": 100.0, "FallsCreek": 50.0, "NewBay": 100.0,
    "Normandy": 70.0, "Wasteland": 120.0,
}


def load_heightmap(game_data: pathlib.Path, space: str, zmax: float) -> np.ndarray | None:
    """解码高度图为米制 ndarray（行 0=南、列 0=西，600m 方框）；与 Rust 侧约定一致。"""
    files = sorted((game_data / "3d" / "Maps" / space / "landscape").glob("*heightmap*.dvpl"))
    if not files:
        return None
    raw = decode_dvpl(files[0].read_bytes())
    if len(raw) < 8:
        return None
    size, tile = struct.unpack_from("<II", raw)
    if size != 512 or tile != 16 or len(raw) != 8 + size * size * 2:
        return None  # 老图变体（如 Himmelsdorf），不做悬浮过滤
    vals = np.frombuffer(raw[8:], dtype="<u2")
    blocks = size // tile
    grid = np.empty((size, size), dtype=np.uint16)
    i = 0
    for by in range(blocks):
        for bx in range(blocks):
            blk = vals[i:i + tile * tile].reshape(tile, tile)
            grid[by * tile:(by + 1) * tile, bx * tile:(bx + 1) * tile] = blk
            i += tile * tile
    return grid.astype(np.float64)[:, ::-1] * zmax / 65535.0


def category_color(name: str | None, group_id: int) -> tuple[float, float, float, float]:
    """按建筑类目给基色；亮度抖动取自建筑名哈希——同一栋建筑的所有部件同色，
    避免墙/顶/门各自深浅不一的碎裂感。未识别名则用亮中性灰（不再用暗蓝灰，
    否则建筑在深色地面上糊成一片黑色矩形）。"""
    jitter_seed = name if name else str(group_id)
    digest = hashlib.sha256(jitter_seed.encode()).digest()
    jitter = 0.95 + digest[3] / 255.0 * 0.10  # 亮度 0.95-1.05（收紧，减少碎裂感）
    base = None
    if name:
        low = name.lower()
        if low.startswith("bld") or "house" in low or "shed" in low:
            base = (0.70, 0.60, 0.46)      # 建筑 → 暖木色（bld_ 前缀是各图通用命名）
        elif low.startswith("stn") or "rock" in low:
            base = (0.60, 0.58, 0.55)      # 岩石 → 亮灰
        else:
            for keywords, color in CATEGORY_PALETTES:
                if any(k in low for k in keywords):
                    base = color
                    break
    def to_linear(c: float) -> float:
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    if base is None:
        # 亮中性灰（少量冷/暖偏移），sRGB 亮度 0.52-0.68
        warm = (digest[0] / 255.0 - 0.5) * 0.06
        light = 0.52 + digest[2] / 255.0 * 0.16
        r, g, b = light + warm, light, light - warm * 0.6
    else:
        r, g, b = (min(1.0, c * jitter) for c in base)
    return (to_linear(r), to_linear(g), to_linear(b), 1.0)

def fan_to_triangles(seq: list[int]) -> list[int]:
    """三角扇 → 三角形列表。"""
    out: list[int] = []
    for i in range(1, len(seq) - 1):
        out.extend((seq[0], seq[i], seq[i + 1]))
    return out


def group_triangles(group: dict, indices: list[int]) -> list[int] | None:
    """把图元索引统一转成三角形列表；无法判定时返回 None（跳过）。

    判据用 primitiveCount 算术自洽，而不是盲信 rhi_primitiveType：
      primitiveCount == indexCount/3  → 三角形列表；
      primitiveCount == indexCount-2  → 三角条带（标准交替绕序转换）；
    两者都不满足的组说明该组语义异常，按列表原样兜底（仍然可渲染）。
    """
    ic = len(indices)
    pc = group.get("primitiveCount")
    ptype = group.get("rhi_primitiveType")
    if isinstance(pc, int):
        if pc == ic // 3 and ic % 3 == 0:
            return indices
        if pc == ic - 2:
            return strip_to_triangles(indices)
    if ptype in (None, 0):
        return indices
    if ptype == 1 and ic >= 3:
        return strip_to_triangles(indices)
    if ptype == 4 and ic >= 3:
        return fan_to_triangles(indices)
    return indices if ic % 3 == 0 else None


def compute_normals(positions: list[tuple[float, float, float]],
                    indices: list[int]) -> list[tuple[float, float, float]]:
    """平滑顶点法线（面积加权）。"""
    normals = [[0.0, 0.0, 0.0] for _ in positions]
    for i in range(0, len(indices) - 2, 3):
        a, b, c = (indices[i], indices[i + 1], indices[i + 2])
        ax, ay, az = positions[a]
        bx, by, bz = positions[b]
        cx, cy, cz = positions[c]
        ux, uy, uz = bx - ax, by - ay, bz - az
        vx, vy, vz = cx - ax, cy - ay, cz - az
        nx, ny, nz = uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx
        for j in (a, b, c):
            normals[j][0] += nx
            normals[j][1] += ny
            normals[j][2] += nz
    out = []
    for n in normals:
        length = math.sqrt(n[0] ** 2 + n[1] ** 2 + n[2] ** 2)
        out.append((n[0] / length, n[1] / length, n[2] / length) if length > 1e-12 else (0.0, 1.0, 0.0))
    return out


def make_conifer(height: float = 12.0):
    """程序化针叶树：棕色树干 + 三层锥形深绿树冠。返回 (positions, indices, colors)。"""
    positions, indices, colors = [], [], []

    def add_ring(y0, y1, r0, r1, seg, color):
        base = len(positions)
        for i in range(seg + 1):
            a = i / seg * math.tau
            positions.extend([(math.cos(a) * r0, y0, math.sin(a) * r0),
                              (math.cos(a) * r1, y1, math.sin(a) * r1)])
            colors.extend([color, color])
        for i in range(seg):
            k = i * 2
            indices.extend([base + k, base + k + 2, base + k + 1])
            indices.extend([base + k + 1, base + k + 2, base + k + 3])

    trunk = (0.42, 0.32, 0.22); leaf = (0.16, 0.32, 0.18)
    add_ring(0, height * 0.12, height * 0.035, height * 0.03, 6, trunk)          # 树干
    for k, (ya, yb, ra, rb) in enumerate([
            (0.10, 0.45, 0.30, 0.10), (0.30, 0.68, 0.24, 0.06), (0.55, 0.92, 0.16, 0.0)]):
        add_ring(height * ya, height * yb, height * ra, height * rb, 7, leaf)
    return positions, indices, colors


def make_bush(height: float = 2.2):
    """程序化灌木/阔叶：树干短柱 + 两层叠放低模球冠，灰绿色。"""
    positions, indices, colors = [], [], []
    seg = 8
    trunk = (0.42, 0.32, 0.22); leaf = (0.30, 0.40, 0.22)
    for i in range(seg + 1):
        a = i / seg * math.tau
        positions.extend([(math.cos(a) * height * 0.06, 0, math.sin(a) * height * 0.06),
                          (math.cos(a) * height * 0.05, height * 0.45, math.sin(a) * height * 0.05)])
        colors.extend([trunk, trunk])
    for i in range(seg):
        k = i * 2
        k2 = (i + 1) % seg * 2
        indices.extend([k, k2, k + 1, k + 1, k2, k2 + 1])  # 树干侧面
    base = len(positions)
    for ring, (ry, rr) in enumerate([(0.42, 0.55), (0.78, 0.36)]):
        for i in range(seg):
            a = i / seg * math.tau
            positions.extend([(math.cos(a) * rr * height * 0.3, height * ry, math.sin(a) * rr * height * 0.3)])
            colors.extend([leaf])
    top = len(positions); positions.append([0.0, height * 1.0, 0.0]); colors.append(list(leaf))
    ring0 = base; ring1 = base + seg
    for i in range(seg):
        j = (i + 1) % seg
        indices.extend([ring0 + i, ring1 + i, ring0 + j])
        indices.extend([ring0 + j, ring1 + i, ring1 + j])
        indices.extend([ring1 + i, top - 1, ring1 + j])
    return positions, indices, colors


def collect_vegetation(scene, lod: int, switch: int):
    """收集 SpeedTreeObject / VegetationRenderObject 实体（树/灌木摆放数据，几何运行时生成）。
    返回 [(名称, worldTransform)]，与 collect_instances 同源逻辑。"""
    out = []
    for path, ent in iter_entities_recursive(scene):
        r = None
        for c in (ent.get('components') or {}).values():
            if isinstance(c, dict) and c.get('comp.typename') == 'RenderComponent':
                r = c; break
        if r is None: continue
        ro = r.get('rc.renderObj')
        if not isinstance(ro, dict): continue
        cls = str(ro.get('##name'))
        if cls not in ('SpeedTreeObject', 'VegetationRenderObject'): continue
        flags = ro.get('ro.flags')
        if isinstance(flags, int) and not (flags & 1): continue
        transform = None
        for c in (ent.get('components') or {}).values():
            if isinstance(c, dict) and c.get('comp.typename') == 'TransformComponent':
                t = c
                transform = {
                    "translation": [float(x) for x in t.get('tc.worldTranslation', [0, 0, 0])],
                    "rotationQuaternionXYZW": [float(x) for x in t.get('tc.worldRotation', [0, 0, 0, 1])],
                    "scale": [float(x) for x in t.get('tc.worldScale', [1, 1, 1])],
                }
                break
        if transform is None: continue
        out.append({"entityName": ent.get('name'), "worldTransform": transform, "renderClass": cls})
    return out


def export_map(game_data: pathlib.Path, map_name: str, space: str,
               output: pathlib.Path, lod: int = 0, switch: int = 0,
               heightmap: np.ndarray | None = None) -> dict:
    directory = game_data / "3d" / "Maps" / space
    sc2_path = find_member(directory, space, ".sc2.dvpl")
    if sc2_path is None:
        sc2_path = find_member(directory, space, ".sc2")
    if sc2_path is None:
        raise FileNotFoundError(f"{space}: 未找到场景文件（{directory}）")
    scg_ext = ".scg.dvpl" if sc2_path.name.lower().endswith(".dvpl") else ".scg"
    scg_path = directory / (sc2_path.stem.replace(".sc2", "") + scg_ext)
    if not scg_path.exists():
        scg_path = directory / (sc2_path.stem + scg_ext)
    if not scg_path.exists():
        raise FileNotFoundError(f"{space}: 未找到伴随 SCG（{scg_path}）")

    sc2_raw = load_payload(sc2_path)
    scene = read_sc2(sc2_raw)
    string_table = extract_string_table(sc2_raw)
    scg = read_scg(load_payload(scg_path))
    groups_by_id = polygon_groups_by_id([g for g in scg.get("polygonGroups", []) if isinstance(g, dict)])
    maps_root = directory.parent  # '../00_global_content/...' 相对空间目录的上一级

    instances, _skipped = collect_instances(scene, lod, switch)
    if not instances:
        raise RuntimeError(f"{space}: lod={lod} switch={switch} 无可见网格实例")

    # 共享网格：按 datasourceId 解码一次；同时算局部包围半径（供环境巨型资产过滤）
    mesh_ids = sorted({inst["datasourceId"] for inst in instances})
    group_radius: dict[int, float] = {}
    group_min_z: dict[int, float] = {}
    group_tri: dict[int, tuple[list, list]] = {}
    for group_id in mesh_ids:
        group = groups_by_id.get(group_id)
        if group is None:
            print(f"  [warn] {map_name}: datasource {group_id:#x} 无对应 PolygonGroup，跳过")
            continue
        try:
            positions = decode_polygon_positions(group)
            raw_indices = decode_polygon_indices(group)
        except Exception as exc:  # 个别组顶点格式异常：跳过不影响整体
            print(f"  [warn] {map_name}: 组 {group_id:#x} 解码失败（{exc}），跳过")
            continue
        indices = group_triangles(group, raw_indices)
        if not indices or len(indices) < 3:
            ptype = group.get("rhi_primitiveType")
            if ptype not in (None, 0, 1, 4):
                print(f"  [warn] {map_name}: 组 {group_id:#x} primitiveType={ptype} 非三角图元，跳过")
            continue
        group_tri[group_id] = (positions, indices)
        group_radius[group_id] = max(math.sqrt(x * x + y * y + z * z) for x, y, z in positions)
        group_min_z[group_id] = min(z for _, _, z in positions)

    # 过滤巨型环境资产（周边山体背景/大片冰面/体积雾等，非战术建筑）。
    # 判据只用世界半径 > 150m——不要按 env_ 前缀排除：不同地图作者对 env_* 的
    # 用法不同（马拉诺夫卡的 env_ma_* 是巨型冰面，其他图的 env_* 只是普通
    # 管道/石块等小道具，按前缀排除会误杀大量正常内容）。
    MAX_RADIUS_M = 150.0

    def is_env_asset(inst: dict) -> bool:
        t = inst["worldTransform"]
        scale = max(t["scale"]) if t["scale"] else 1.0
        return group_radius.get(inst["datasourceId"], 0.0) * scale > MAX_RADIUS_M

    instances = [it for it in instances if not is_env_asset(it)]

    # 悬浮/天空过滤：天空投影球按名排除；底面高于地形 >40m 且半径 >8m 的大件剔除
    # （正确摆放建筑的屋顶组会悬空 ~8m、地标风车悬 17-24m，均保留）；±320m 图外剔除。
    instances = [it for it in instances
                 if not any(k in (it.get("entityName") or "").lower() for k in ("sky", "smoke"))
                 and abs(it["worldTransform"]["translation"][0]) <= 320
                 and abs(it["worldTransform"]["translation"][1]) <= 320]
    if heightmap is not None:
        hn = heightmap.shape[0]

        def terrain_h(x: float, y: float) -> float:
            fx = (x / 600.0 + 0.5) * (hn - 1)
            fy = (y / 600.0 + 0.5) * (hn - 1)
            x0 = min(max(int(fx), 0), hn - 2)
            y0 = min(max(int(fy), 0), hn - 2)
            tx, ty = min(max(fx - x0, 0.0), 1.0), min(max(fy - y0, 0.0), 1.0)
            top = heightmap[y0, x0] * (1 - tx) + heightmap[y0, x0 + 1] * tx
            bot = heightmap[y0 + 1, x0] * (1 - tx) + heightmap[y0 + 1, x0 + 1] * tx
            return top * (1 - ty) + bot * ty

        kept = []
        for it in instances:
            t = it["worldTransform"]
            tr = t["translation"]
            scale = max(t["scale"]) if t["scale"] else 1.0
            local_min_z = group_min_z.get(it["datasourceId"])
            if local_min_z is None:
                kept.append(it)
                continue
            gap = (tr[2] + local_min_z * scale) - terrain_h(tr[0], tr[1])
            radius = group_radius.get(it["datasourceId"], 0.0) * scale
            if gap > 40.0 and radius > 8.0:
                print(f"  [filter] {map_name}: 悬空实例 {it.get('entityName')!r} "
                      f"底面高于地形 {gap:.1f}m（半径 {radius:.0f}m），剔除")
                continue
            kept.append(it)
        instances = kept
    mesh_ids = sorted({it["datasourceId"] for it in instances if it["datasourceId"] in group_tri})

    # 建筑名解析：SwitchNode 实例沿层级向上找具名祖先（bld_xx.sc2 等），
    # 每组取多数名 → 类目色（比随机冷灰更接近真实建筑观感）
    path_index = build_path_index(scene)
    names_by_group: dict[int, Counter] = {}
    for it in instances:
        building = resolve_building_name(it["entityPath"], path_index)
        if building:
            names_by_group.setdefault(it["datasourceId"], Counter())[building] += 1

    # 植被（SpeedTreeObject/VegetationRenderObject）：几何由引擎运行时生成、
    # 文件里只有摆放数据——无法精确提取树形，改用程序化近似（真实位置/缩放）：
    # fir=锥形针叶树，bush/其余=球状灌木，顶点色区分树干/树冠。
    veg = collect_vegetation(scene, lod, switch)
    veg = [v for v in veg
           if not any(k in (v.get("entityName") or "").lower() for k in ("sky", "smoke"))
           and abs(v["worldTransform"]["translation"][0]) <= 320
           and abs(v["worldTransform"]["translation"][1]) <= 320]

    gltf_meshes, gltf_materials = [], []
    mesh_index_by_id: dict[int, int] = {}
    buffer: bytearray = bytearray()
    buffer_views: list[dict] = []
    accessors: list[dict] = []
    gltf_images: list[dict] = []
    gltf_textures: list[dict] = []
    texture_index_by_path: dict[pathlib.Path, int] = {}
    total_tris = 0

    def texture_jpeg(tpath: pathlib.Path) -> bytes | None:
        try:
            img = read_dds_image(tpath, max_dim=1024)
        except Exception as exc:
            print(f"  [warn] {map_name}: 贴图解码失败 {tpath.name}（{exc}）")
            return None
        if img is None:
            return None
        buf = io.BytesIO()
        img.save(buf, "JPEG", quality=85)
        return buf.getvalue()

    def register_image(tpath: pathlib.Path, jpeg: bytes) -> int:
        if tpath in texture_index_by_path:
            return texture_index_by_path[tpath]
        while len(buffer) % 4:
            buffer.append(0)
        offset = len(buffer)
        buffer.extend(jpeg)
        buffer_views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(jpeg)})
        gltf_images.append({"bufferView": len(buffer_views) - 1, "mimeType": "image/jpeg"})
        idx = len(gltf_images) - 1
        gltf_textures.append({"source": idx})
        texture_index_by_path[tpath] = idx
        return idx

    def add_view(payload: bytes) -> int:
        # 4 字节对齐
        while len(buffer) % 4:
            buffer.append(0)
        offset = len(buffer)
        buffer.extend(payload)
        buffer_views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(payload)})
        return len(buffer_views) - 1

    for group_id in mesh_ids:
        positions, indices = group_tri[group_id]
        normals = compute_normals(positions, indices)

        pos_payload = struct.pack(f"<{len(positions) * 3}f", *[v for p in positions for v in p])
        nrm_payload = struct.pack(f"<{len(normals) * 3}f", *[v for n in normals for v in n])
        idx_payload = struct.pack(f"<{len(indices)}I", *indices)
        pos_view = add_view(pos_payload)
        nrm_view = add_view(nrm_payload)
        idx_view = add_view(idx_payload)

        mins = [min(p[i] for p in positions) for i in range(3)]
        maxs = [max(p[i] for p in positions) for i in range(3)]
        accessors.append({"bufferView": pos_view, "componentType": 5126, "count": len(positions),
                          "type": "VEC3", "min": mins, "max": maxs})
        pos_acc = len(accessors) - 1
        accessors.append({"bufferView": nrm_view, "componentType": 5126, "count": len(normals), "type": "VEC3"})
        nrm_acc = len(accessors) - 1
        accessors.append({"bufferView": idx_view, "componentType": 5125, "count": len(indices), "type": "SCALAR"})
        idx_acc = len(accessors) - 1

        building_names = names_by_group.get(group_id)
        building = building_names.most_common(1)[0][0] if building_names else None

        # 真贴图：该建筑组在字符串表中有对应 .tex 路径且客户端存在实际文件时，
        # 解码为 JPEG 内嵌 GLB，材质走 baseColorTexture；否则退回类目色。
        uvs = decode_group_uvs(groups_by_id[group_id]) if group_id in groups_by_id else None
        tex_file = find_texture_file(string_table, building, maps_root,
                                     get_texture_index(game_data)) if (building and uvs) else None
        r, g, b, a = category_color(building, group_id)

        gltf_materials.append({
            "name": (building or f"group_{group_id:x}")[:60],
            "pbrMetallicRoughness": {"baseColorFactor": [r, g, b, a], "metallicFactor": 0.0, "roughnessFactor": 1.0},
            "doubleSided": True,
        })
        attrs = {"POSITION": pos_acc, "NORMAL": nrm_acc}

        if uvs and tex_file is not None:
            jpeg = texture_jpeg(tex_file)
            if jpeg:
                uv_payload = struct.pack(f"<{len(uvs) * 2}f", *[v for uv in uvs for v in uv])
                uv_view = add_view(uv_payload)
                uv_mins = [min(p[0] for p in uvs), min(p[1] for p in uvs)]
                uv_maxs = [max(p[0] for p in uvs), max(p[1] for p in uvs)]
                accessors.append({"bufferView": uv_view, "componentType": 5126,
                                  "count": len(uvs), "type": "VEC2", "min": uv_mins, "max": uv_maxs})
                attrs["TEXCOORD_0"] = len(accessors) - 1
                tex_idx = register_image(tex_file, jpeg)
                gltf_materials[-1]["pbrMetallicRoughness"]["baseColorTexture"] = {"index": tex_idx}
                gltf_materials[-1]["pbrMetallicRoughness"]["baseColorFactor"] = [1.0, 1.0, 1.0, 1.0]

        mesh_index_by_id[group_id] = len(gltf_meshes)
        gltf_meshes.append({"primitives": [{"attributes": attrs,
                                            "indices": idx_acc, "material": len(gltf_materials) - 1}]})
        total_tris += len(indices) // 3

    # 实例节点（世界变换直映射 glTF TRS；纯游戏系坐标）
    nodes = []
    for inst in instances:
        mesh_idx = mesh_index_by_id.get(inst["datasourceId"])
        if mesh_idx is None:
            continue
        t = inst["worldTransform"]
        nodes.append({
            "mesh": mesh_idx,
            "translation": t["translation"],
            "rotation": t["rotationQuaternionXYZW"],
            "scale": t["scale"],
            "name": inst.get("entityName") or None,
        })

    # 植被（SpeedTree 几何由引擎运行时生成，无法精确提取）——程序化近似：
    # 按 SC2 里的真实摆放位置/缩放铺设针叶树（标称 12m）与灌木（3m），
    # 单位几何共享 + COLOR_0 顶点色区分树干/树冠。
    if veg:
        conifer_mesh = len(gltf_meshes)
        bush_mesh = conifer_mesh + 1
        for kind, nominal in (("conifer", 12.0), ("bush", 3.0)):
            vp, vi, vc = (make_conifer(1.0) if kind == "conifer" else make_bush(1.0))
            colors = [[c[0] * 0.9, c[1] * 0.9, c[2] * 0.9, 1.0] for c in vc]
            pos_payload = struct.pack(f"<{len(vp) * 3}f", *[v for pt in vp for v in pt])
            col_payload = struct.pack(f"<{len(colors) * 4}f", *[v for c in colors for v in c])
            idx_payload = struct.pack(f"<{len(vi)}I", *vi)
            pos_view = add_view(pos_payload)
            col_view = add_view(col_payload)
            idx_view = add_view(idx_payload)
            mins = [min(pt[i] for pt in vp) for i in range(3)]
            maxs = [max(pt[i] for pt in vp) for i in range(3)]
            accessors.append({"bufferView": pos_view, "componentType": 5126, "count": len(vp),
                              "type": "VEC3", "min": mins, "max": maxs})
            accessors.append({"bufferView": col_view, "componentType": 5126, "count": len(colors),
                              "type": "VEC4"})
            accessors.append({"bufferView": idx_view, "componentType": 5125,
                              "count": len(vi), "type": "SCALAR"})
            gltf_materials.append({
                "name": f"veg_{kind}",
                "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1],
                                         "metallicFactor": 0.0, "roughnessFactor": 1.0},
                "doubleSided": True,
            })
            gltf_meshes.append({"primitives": [{
                "attributes": {"POSITION": len(accessors) - 3, "COLOR_0": len(accessors) - 2},
                "indices": len(accessors) - 1, "material": len(gltf_materials) - 1}]})

        for v in veg:
            nm = (v.get("entityName") or "").lower()
            is_conifer = any(k in nm for k in ("fir", "pine", "spruce", "tree"))
            t = v["worldTransform"]
            s = t["scale"]
            nominal = 12.0 if is_conifer else 3.0
            nodes.append({
                "mesh": conifer_mesh if is_conifer else bush_mesh,
                "translation": t["translation"],
                "rotation": t["rotationQuaternionXYZW"],
                "scale": [s[0] * nominal, s[1] * nominal, s[2] * nominal],
                "name": v.get("entityName") or None,
            })
        veg_count = len(veg)

    gltf = {
        "asset": {"version": "2.0", "generator": "wotb-agent export_map_glb (WotbTools contract)"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": gltf_materials,
        "textures": gltf_textures,
        "images": gltf_images,
        "samplers": [{"wrapS": 10497, "wrapT": 10497, "magFilter": 9729, "minFilter": 9987}],
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{"byteLength": len(buffer)}],
    }
    json_chunk = json.dumps(gltf, separators=(",", ":")).encode()
    while len(json_chunk) % 4:
        json_chunk += b" "
    bin_chunk = bytes(buffer)
    while len(bin_chunk) % 4:
        bin_chunk += b"\x00"
    total = 12 + 8 + len(json_chunk) + 8 + len(bin_chunk)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(
        struct.pack("<III", 0x46546C67, 2, total)
        + struct.pack("<II", len(json_chunk), 0x4E4F534A) + json_chunk
        + struct.pack("<II", len(bin_chunk), 0x004E4942) + bin_chunk
    )
    return {"instances": len(nodes), "meshes": len(gltf_meshes), "triangles": total_tris,
            "bytes": total, "path": str(output)}


# ---------- 高清地面贴图（landscape colormap, 2048² DXT5 → WebP） ----------

# 命中排除词的文件一律不作为地表色图（法线/遮罩/细节/草皮/水体/辅助阴影等）
GROUND_EXCLUDE_KEYWORDS = (
    "tilemask", "normal", "roughness", "height", "grass", "water", "lightmap",
    "fake", "edge_map", "seabottom", "detail_", "thumbnail", "tile", "pbr",
    "koevaja", "kroevaja", "tread", "decals", "hm_", "minimap",
)
# 优先级关键词（高分在前）；都不命中时仍可作为兜底候选（如 BlackGoldville 的 2DSands2）
GROUND_PRIORITY_KEYWORDS = ("colormap", "colortexture", "colortexture", "wint", "landscape")
# DDS fourCC → imagecodecs BCn 格式码
DDS_FOURCC_TO_BCN = {"DXT1": 1, "DXT3": 2, "DXT5": 3}


def select_ground_file(landscape: pathlib.Path) -> pathlib.Path | None:
    best = None  # (score, pixels, path)
    for p in sorted(landscape.glob("*.dds.dvpl")):
        low = p.name.lower()
        if any(k in low for k in GROUND_EXCLUDE_KEYWORDS):
            continue
        score = 10
        for i, kw in enumerate(GROUND_PRIORITY_KEYWORDS):
            if kw in low:
                score = 100 - i
                break
        rank = (score, p.stat().st_size)
        if best is None or rank > best[0]:
            best = (rank, p)
    return best[1] if best else None


def export_ground(game_data: pathlib.Path, map_name: str, space: str, output: pathlib.Path) -> dict:
    """解码 landscape colormap DDS 为 WebP（上=+z，与底图同向； mild 对比度/饱和度增强）。"""
    if Image is None or imagecodecs is None:
        raise RuntimeError("需要 pip install pillow imagecodecs")
    landscape = game_data / "3d" / "Maps" / space / "landscape"
    src = select_ground_file(landscape)
    if src is None:
        raise FileNotFoundError(f"{space}: landscape/ 下无可用地表色图")

    d = decode_dvpl(src.read_bytes())
    if d[:4] != b"DDS ":
        raise ValueError(f"{src.name}: 非 DDS 容器")
    height = struct.unpack_from("<I", d, 12)[0]
    width = struct.unpack_from("<I", d, 16)[0]
    fourcc = d[84:88].decode(errors="replace")
    bcn = DDS_FOURCC_TO_BCN.get(fourcc)
    if bcn is None:
        raise ValueError(f"{src.name}: 暂不支持 fourCC {fourcc}（仅 DXT1/DXT3/DXT5）")
    block = 8 if bcn == 1 else 16
    data = d[128:128 + (width // 4) * (height // 4) * block]
    rgba = imagecodecs.bcn_decode(data, bcn, shape=(height, width, 4))
    arr = np.frombuffer(rgba, dtype=np.uint8).reshape(height, width, 4)

    # 保留游戏原始贴图像素（含脚印阴影/alpha 涂黑区域——用户要求不做清理，
    # 那些区域可能对应尚未正确处理的植被等地物）
    img = Image.frombytes("RGB", (width, height), arr[..., :3].astype(np.uint8))
    img = img.transpose(Image.FLIP_TOP_BOTTOM)
    # 地图原色偏灰白：轻微提升对比/饱和，接近小地图观感
    img = ImageEnhance.Contrast(img).enhance(1.18)
    img = ImageEnhance.Color(img).enhance(1.15)
    output.parent.mkdir(parents=True, exist_ok=True)
    img.save(output, "WEBP", quality=85)
    return {"size": (width, height), "bytes": output.stat().st_size, "source": src.name}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--map", action="append", help="只导出指定图（可重复）；缺省全部")
    parser.add_argument("--game-data", type=pathlib.Path,
                        default=pathlib.Path("D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data"))
    parser.add_argument("--output-dir", type=pathlib.Path, default=pathlib.Path("glb_cache/maps"))
    parser.add_argument("--ground-only", action="store_true",
                        help="跳过 GLB，只重新导出地面贴图（复用已生成的 GLB）")
    args = parser.parse_args()

    wanted = args.map or list(MAP_SPACES)
    failures = []
    for name in wanted:
        space = MAP_SPACES.get(name)
        if space is None:
            print(f"[skip] {name}: 不在映射表")
            continue
        if not args.ground_only:
            try:
                zmax = MAP_ZMAX.get(name, 100.0)
                hmap = load_heightmap(args.game_data, space, zmax)
                info = export_map(args.game_data, name, space, args.output_dir / f"{name}.glb",
                                  heightmap=hmap)
                print(f"[ok] {name:<16} 实例 {info['instances']:>4}  网格 {info['meshes']:>3}  "
                      f"三角形 {info['triangles']:>7}  {info['bytes'] / 1e6:.2f} MB")
            except Exception as exc:
                failures.append(name)
                print(f"[fail] {name:<16} GLB: {exc}")
                continue
        try:
            ground = export_ground(args.game_data, name, space,
                                   args.output_dir / f"{name}.ground.webp")
            print(f"[ok] {name:<16} 地面 {ground['size'][0]}x{ground['size'][1]}  "
                  f"来源 {ground['source']}  {ground['bytes'] / 1e6:.2f} MB")
        except Exception as exc:
            print(f"[warn] {name:<16} 地面贴图跳过: {exc}")
    if failures:
        print(f"\nGLB 失败 {len(failures)} 图：{', '.join(failures)}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
