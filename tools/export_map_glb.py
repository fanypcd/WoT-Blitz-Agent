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
    decode_polygon_indices,
    decode_polygon_positions,
    polygon_groups_by_id,
    read_scg,
)
from export_map_geometry_poc import collect_instances  # noqa: E402

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
    避免墙/顶/门各自深浅不一的碎裂感。未识别名则退回冷灰哈希。"""
    jitter_seed = name if name else str(group_id)
    digest = hashlib.sha256(jitter_seed.encode()).digest()
    jitter = 0.95 + digest[3] / 255.0 * 0.10  # 亮度 0.95-1.05（收紧，减少碎裂感）
    base = None
    if name:
        low = name.lower()
        for keywords, color in CATEGORY_PALETTES:
            if any(k in low for k in keywords):
                base = color
                break
    def to_linear(c: float) -> float:
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    if base is None:
        hue = digest[0] / 255.0 * (230 - 200) + 200      # 200-230° 蓝青
        sat = 0.05 + digest[1] / 255.0 * 0.10
        light = (0.38 + digest[2] / 255.0 * 0.30) * jitter
        r, g, b = colorsys.hls_to_rgb(hue / 360.0, min(light, 0.72), sat)
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
    """按 DAVA ePrimitiveType 转为三角形列表；非三角类图元返回 None（跳过）。
    DAVA 枚举：0=TRIANGLELIST 1=TRIANGLESTRIP 2=LINESTRIP 3=LINELIST 4=TRIANGLEFAN 5=POINTLIST。"""
    ptype = group.get("rhi_primitiveType")
    if ptype in (None, 0):
        return indices
    if ptype == 1:
        return strip_to_triangles(indices)
    if ptype == 4:
        return fan_to_triangles(indices)
    return None


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

    scene = read_sc2(load_payload(sc2_path))
    scg = read_scg(load_payload(scg_path))
    groups_by_id = polygon_groups_by_id([g for g in scg.get("polygonGroups", []) if isinstance(g, dict)])

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

    # 过滤巨型环境资产（周边山体背景/冰面/体积雾等，非战术建筑）
    # 判据：世界半径 > 150m，或实体名命中环境资产命名（env_* / fog* / mountain*）
    MAX_RADIUS_M = 150.0

    def is_env_asset(inst: dict) -> bool:
        t = inst["worldTransform"]
        scale = max(t["scale"]) if t["scale"] else 1.0
        if group_radius.get(inst["datasourceId"], 0.0) * scale > MAX_RADIUS_M:
            return True
        name = (inst.get("entityName") or "").lower()
        return name.startswith(("env_", "fog", "mountain"))

    instances = [it for it in instances if not is_env_asset(it)]

    # 悬浮/图外过滤：
    # 1) 天空渲染件（SkyFlattenSphere 天空球等）按名排除——它们是游戏天空着色器
    #    的投影面，故意悬在 60m 海拔上限的高空，导出成实体材质就是一颗暗球；
    # 2) 底面高于地形 >40m 的大件剔除（真正的天空/月亮类遗留）；
    # 3) 平移在 ±320m 图外的剔除（边界装饰会悬在虚空）。
    # 阈值 40m 是因为正确摆放的"屋顶组"会悬空 ~8m、部分地标（如马拉诺夫卡
    # 风车 mill+screw，悬 17-24m，用户要求保留）也在此列。高度图不可用（老图）
    # 时跳过 2) 3)。
    # 天空渲染件与图外装饰无条件排除（不依赖高度图）
    instances = [it for it in instances
                 if "sky" not in (it.get("entityName") or "").lower()
                 and abs(it["worldTransform"]["translation"][0]) <= 320
                 and abs(it["worldTransform"]["translation"][1]) <= 320]

    # 悬浮过滤：
    # 底面高于地形 >40m 的大件剔除（真正的天空/月亮类遗留）。
    # 阈值 40m 是因为正确摆放的"屋顶组"会悬空 ~8m、部分地标（如马拉诺夫卡
    # 风车 mill+screw，悬 17-24m，用户要求保留）也在此列。高度图不可用（老图）
    # 时跳过。
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

    gltf_meshes, gltf_materials = [], []
    mesh_index_by_id: dict[int, int] = {}
    buffer: bytearray = bytearray()
    buffer_views: list[dict] = []
    accessors: list[dict] = []
    total_tris = 0

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
        r, g, b, a = category_color(building, group_id)
        gltf_materials.append({
            "name": (building or f"group_{group_id:x}")[:60],
            "pbrMetallicRoughness": {"baseColorFactor": [r, g, b, a], "metallicFactor": 0.0, "roughnessFactor": 1.0},
            "doubleSided": True,
        })
        mesh_index_by_id[group_id] = len(gltf_meshes)
        gltf_meshes.append({"primitives": [{"attributes": {"POSITION": pos_acc, "NORMAL": nrm_acc},
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

    gltf = {
        "asset": {"version": "2.0", "generator": "wotb-agent export_map_glb (WotbTools contract)"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": gltf_materials,
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
    img = Image.frombytes("RGBA", (width, height), rgba)
    img = img.transpose(Image.FLIP_TOP_BOTTOM).convert("RGB")
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
