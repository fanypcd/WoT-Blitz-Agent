#!/usr/bin/env python3
"""导出回放页 3D 场景模型（建筑/桥/岩石等静态地物）为单文件 GLB。

数据源：本机 WoTB 客户端 3d/Maps/<space>/<space>.sc2[.dvpl] + 同名 .scg[.dvpl]。
提取契约（可见性位 / LOD / switch 选择）沿用 WotbTools（MIT，见 tools/wotbtools/）的
export_map_geometry_poc 研究结论：SC2 RenderComponent → Mesh → ro.flags bit0 →
batch lodIndex/switchIndex（-1 通配）→ rb.datasource → SCG PolygonGroup。

输出：glb_cache/maps/<MapName>.glb，坐标系 = 游戏世界（米，z 上、+y 北）。
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

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

from wotb_sc2 import decode_dvpl, read_sc2  # noqa: E402
from wotb_scg import (  # noqa: E402
    decode_polygon_indices,
    decode_polygon_positions,
    polygon_groups_by_id,
    read_scg,
)
from export_map_geometry_poc import (  # noqa: E402
    collect_instances,
    component_by_type,
    iter_entities_recursive,
)

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
    """定位 <space>.sc2.dvpl / 兜底 glob；suffix 形如 '.sc2.dvpl'。"""
    exact = directory / f"{space}{suffix}"
    if exact.exists():
        return exact
    plain = exact.with_suffix("") if suffix == ".sc2" else None
    if plain is not None and plain.exists():
        return plain
    for p in sorted(directory.glob(f"*{suffix}")):
        return p
    return None


def load_payload(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return decode_dvpl(raw) if path.name.lower().endswith(".dvpl") else raw


def group_color(group_id: int) -> tuple[float, float, float, float]:
    """按组 ID 哈希出低饱和蓝灰色（线性 RGB），战术沙盘风格。"""
    digest = hashlib.sha256(str(group_id).encode()).digest()
    hue = digest[0] / 255.0 * (230 - 200) + 200      # 200-230° 蓝青
    sat = 0.05 + digest[1] / 255.0 * 0.10            # 5-15%
    light = 0.35 + digest[2] / 255.0 * 0.35          # 35-70%
    r, g, b = colorsys.hls_to_rgb(hue / 360.0, light, sat)
    # sRGB 近似转线性（glTF baseColorFactor 为线性空间）
    def to_linear(c: float) -> float:
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    return (to_linear(r), to_linear(g), to_linear(b), 1.0)


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
               output: pathlib.Path, lod: int = 0, switch: int = 0) -> dict:
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
    mesh_ids = sorted({it["datasourceId"] for it in instances if it["datasourceId"] in group_tri})

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

        r, g, b, a = group_color(group_id)
        gltf_materials.append({
            "name": f"group_{group_id:x}",
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--map", action="append", help="只导出指定图（可重复）；缺省全部")
    parser.add_argument("--game-data", type=pathlib.Path,
                        default=pathlib.Path("D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data"))
    parser.add_argument("--output-dir", type=pathlib.Path, default=pathlib.Path("glb_cache/maps"))
    args = parser.parse_args()

    wanted = args.map or list(MAP_SPACES)
    failures = []
    for name in wanted:
        space = MAP_SPACES.get(name)
        if space is None:
            print(f"[skip] {name}: 不在映射表")
            continue
        try:
            info = export_map(args.game_data, name, space, args.output_dir / f"{name}.glb")
            print(f"[ok] {name:<16} 实例 {info['instances']:>4}  网格 {info['meshes']:>3}  "
                  f"三角形 {info['triangles']:>7}  {info['bytes'] / 1e6:.2f} MB")
        except Exception as exc:
            failures.append(name)
            print(f"[fail] {name:<16} {exc}")
    if failures:
        print(f"\n失败 {len(failures)} 图：{', '.join(failures)}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
