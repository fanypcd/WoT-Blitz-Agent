"""草系统 / 地图边界导出纯函数单测：python tools/test_export_grass_border.py

覆盖 2026-11-17 审计 H1 / I5 的导出实现（tools/export_map_glb.py）：
  - `decode_cgd_lod`：cgd 平行数组（pos/tex/index）解码；任一件缺失/越界 fail-closed；
  - `_strip_to_triangles`：三角形带交替绕序；
  - `_hash32`：确定性（固定夹具，换实现即失配）；
  - `collect_grass_scene`：密度 alpha 按比例分配 60000 实例（最大余数法）、翻转位随档、
    fail-closed（缺密度图/缺翻转掩码/密度全零 → None）；
  - `collect_map_border`：三角形带 + 世界变换烘焙 + 组件视觉字段。
"""
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

from export_map_glb import (  # noqa: E402
    GRASS_GRID, GRASS_INSTANCE_STRUCT, GRASS_TARGET_INSTANCES,
    _hash32, _strip_to_triangles, collect_grass_scene, collect_map_border,
    decode_cgd_lod,
)


# ---------------------------------------------------------------- cgd 解码

def _lod(verts, idx):
    out = {"cgd.lod.posCount": len(verts), "cgd.lod.indexCount": len(idx)}
    for i, (x, y, z) in enumerate(verts):
        out[f"cgd.lod.pos.{i}"] = [x, y, z]
        out[f"cgd.lod.tex.{i}"] = [i * 0.1, i * 0.2]
    for i, v in enumerate(idx):
        out[f"cgd.lod.index.{i}"] = v
    return out


def test_decode_cgd_lod_ok():
    geo = decode_cgd_lod(_lod([(0, 0, 0), (0, 0, 1), (0.2, 0, 1)], [0, 1, 2]))
    assert geo is not None
    assert len(geo["positions"]) == 3 and geo["positions"][2] == (0.2, 0.0, 1.0)
    assert geo["uvs"][1] == (0.1, 0.2)
    assert geo["indices"] == [0, 1, 2]


def test_decode_cgd_lod_fail_closed():
    base = _lod([(0, 0, 0), (0, 0, 1), (0, 0, 2)], [0, 1, 2])
    # 缺 posCount
    broken = dict(base)
    broken.pop("cgd.lod.posCount")
    assert decode_cgd_lod(broken) is None
    # 索引越界
    bad_idx = dict(base)
    bad_idx["cgd.lod.index.1"] = 99
    assert decode_cgd_lod(bad_idx) is None
    # 顶点缺分量
    bad_pos = dict(base)
    bad_pos["cgd.lod.pos.0"] = [0.0, 0.0]
    assert decode_cgd_lod(bad_pos) is None
    # 顶点数不足
    assert decode_cgd_lod(_lod([(0, 0, 0)], [0])) is None


# ---------------------------------------------------------------- 三角形带

def test_strip_to_triangles_alternating_winding():
    # 4 顶点条带 → 2 三角形；第二个三角形绕序翻转（DAVA TRIANGLESTRIP 同）
    assert _strip_to_triangles([0, 1, 2, 3]) == [0, 1, 2, 1, 3, 2]
    # 3 顶点 → 1 三角形；2 顶点 → 空
    assert _strip_to_triangles([5, 6, 7]) == [5, 6, 7]
    assert _strip_to_triangles([5, 6]) == []


# ---------------------------------------------------------------- 哈希

def test_hash32_stable_fixture():
    # 固定夹具（客户端/JS 侧同式复算用；换实现即失配）
    vals = [_hash32(i) for i in range(6)]
    assert vals == [3232319850, 663891101, 3329832309, 2278584254, 3427349084, 3322605197], vals
    assert _hash32(0xFFFFFFFF) == _hash32(-1)   # 掩码到 32 位


# ---------------------------------------------------------------- 草提取

class _FakeMaterials:
    def __init__(self, albedo):
        self.albedo = albedo

    def resolve(self, _mid):
        return {"textures": {"albedo": self.albedo}}


class _FakeTextures:
    def __init__(self, images):
        self.images = images

    def get(self, path):
        img = self.images.get(path)
        return (img, False) if img is not None else (None, False)


def _grass_scene(density_path="grass/d.tex", flip_ones=4, alpha_value=128):
    chunk = {"cgd.materialId": 7}
    for vi in range(4):
        chunk[f"cgd.variation.{vi}"] = {"cgd.lod.0": _lod(
            [(0, 0, 0), (0.1, 0, 0), (0, 0.1, 1.1)], [0, 1, 2])}
    flip = bytearray(GRASS_GRID * GRASS_GRID)
    for i in range(flip_ones):
        flip[i] = 1
    veg = {
        "##name": "VegetationRenderObject",
        "vro.geometryData": {"cgd.chunkSet": chunk},
        "vro.lightmap": density_path,
        "vro.flippedDensityMap": {"$bytes": flip.hex()},
        "vro.scaleVariation": [0.2, 0.2, 0.2, 0.2],
        "vro.rotationVariation": [180.0, 0.0, 0.0, 0.0],
        "vro.baseColorMultilplier": 2.0,
        "vro.layerAnimationAmplitude": [0.2, 0.2, 0.2, 0.2],
        "vro.layersAnimationDrug": [1.4, 1.4, 1.4, 1.4],
        "vro.layersAnimationSpring": [15.0, 15.0, 15.0, 15.0],
        "vro.visibilityDistance": [2500.0, 1600.0],
        "vro.lodRanges": [0.0, 2.0, 6.0],
        "vro.isLit": False,
    }
    scene = {"#hierarchy": [{
        "name": "grass.sc2",
        "components": {"0001": {"comp.typename": "RenderComponent", "rc.renderObj": veg}},
    }]}
    density = Image.new("RGBA", (16, 16), (0, 0, 0, alpha_value))
    albedo = Image.new("RGBA", (8, 8), (128, 128, 128, 255))
    return scene, _FakeMaterials("grass/grass120.tex"), _FakeTextures({
        density_path: density, "grass/grass120.tex": albedo})


def test_collect_grass_scene_allocates_budget():
    scene, mats, texs = _grass_scene()
    grass = collect_grass_scene(scene, mats, texs, {"min": [-300, -300, 0], "max": [300, 300, 0]})
    assert grass is not None
    meta = grass["meta"]
    assert meta["count"] == GRASS_TARGET_INSTANCES
    assert len(grass["instances"]) == GRASS_TARGET_INSTANCES * GRASS_INSTANCE_STRUCT.size
    assert meta["variations"] == 4
    assert grass["params"]["baseColorMultiplier"] == 2.0
    # 实例字段合法：变体 < 4、位置落在 bounds 内
    xq, zq, rq, sq, variation, flipped = GRASS_INSTANCE_STRUCT.unpack_from(grass["instances"], 0)
    assert 0 <= variation < 4
    assert 0 <= sq <= 255
    assert xq <= 65535 and zq <= 65535


def test_collect_grass_scene_flipped_cells_recorded():
    # 前 4 个格翻转 ⇒ 至少一条实例带 flipped 位（网格序：cy=0, cx=0..3）
    scene, mats, texs = _grass_scene(flip_ones=4)
    grass = collect_grass_scene(scene, mats, texs, {"min": [-300, -300, 0], "max": [300, 300, 0]})
    flipped_seen = any(GRASS_INSTANCE_STRUCT.unpack_from(grass["instances"], off)[5]
                       for off in range(0, len(grass["instances"]), GRASS_INSTANCE_STRUCT.size))
    assert flipped_seen


def test_collect_grass_scene_fail_closed():
    bounds = {"min": [-300, -300, 0], "max": [300, 300, 0]}
    # 缺密度图
    scene, mats, texs = _grass_scene()
    assert collect_grass_scene(scene, mats, _FakeTextures({}), bounds) is None
    # 密度全零
    scene, mats, texs = _grass_scene(alpha_value=0)
    assert collect_grass_scene(scene, mats, texs, bounds) is None
    # 缺翻转掩码
    scene, mats, texs = _grass_scene()
    scene["#hierarchy"][0]["components"]["0001"]["rc.renderObj"].pop("vro.flippedDensityMap")
    assert collect_grass_scene(scene, mats, texs, bounds) is None
    # 无 VegetationRenderObject
    assert collect_grass_scene({"#hierarchy": []}, mats, texs, bounds) is None


# ---------------------------------------------------------------- 边界提取

def _scg_group(verts, idx):
    """合成 PolygonGroup（EVF_VERTEX、float3、u16 索引）。"""
    payload = b"".join(struct.pack("<3f", *v) for v in verts)
    idx_payload = struct.pack(f"<{len(idx)}H", *idx)
    return {
        "#id": {"$bytes": struct.pack("<Q", 42).hex()},
        "vertexFormat": 1,           # EVF_VERTEX
        "vertexCount": len(verts),
        "indexCount": len(idx),
        "indexFormat": 0,            # INDEX_FORMAT_UINT16
        "textureCoordCount": 0,
        "cubeTextureCoordCount": 0,
        "vertices": {"$bytes": payload.hex()},
        "indices": {"$bytes": idx_payload.hex()},
    }


def _border_scene(translation=(0.0, 0.0, 0.0)):
    return {"#hierarchy": [{
        "name": "MapBorder1",
        "visibility": 0,
        "components": {
            "0001": {"comp.typename": "MapBorderComponent",
                     "mbc.rect": {"$bytes": struct.pack("<4f", -300, -300, 300, 300).hex()},
                     "mbc.visualColor": [1.0, 0.156, 0.156, 1.0],
                     "mbc.visualHeight": 0.9,
                     "mbc.visualMinZ": -100.0, "mbc.visualMaxZ": 100.0,
                     "mbc.visualZFadeWidth": 2.0, "mbc.visualZOffset": -0.2,
                     "mbc.visualDistanceFadeEnabled": False},
            "0002": {"comp.typename": "RenderComponent",
                     "rc.renderObj": {"##name": "MapBorderRenderObject",
                                      "mro.distanceNearFarSq": [90000.0, 160000.0],
                                      "mro.distanceFadeEnabled": False,
                                      "ro.batches": {"0000": {"rb.datasource": 42}},
                                      "ro.flags": 1}},
            "0003": {"comp.typename": "TransformComponent",
                     "tc.worldTranslation": list(translation),
                     "tc.worldRotation": [0.0, 0.0, 0.0, 1.0],
                     "tc.worldScale": [1.0, 1.0, 1.0]},
        },
    }]}


def test_collect_map_border_geometry_and_visual():
    group = _scg_group([(0, 0, 0), (0, 0, 0.9), (1, 0, 0.1), (1, 0, 1.0)], [0, 1, 2, 3])
    border = collect_map_border(_border_scene(), {42: group})
    assert border is not None
    assert len(border["positions"]) == 4
    assert border["indices"] == [0, 1, 2, 1, 3, 2]     # 条带交替绕序 → 2 三角形
    v = border["visual"]
    assert v["color"][:3] == [1.0, 0.156, 0.156]
    assert v["height"] == 0.9 and v["minZ"] == -100.0 and v["maxZ"] == 100.0
    assert v["distanceNearFarSq"] == [90000.0, 160000.0]


def test_collect_map_border_applies_world_transform():
    group = _scg_group([(0, 0, 0), (0, 0, 0.9), (1, 0, 0.1), (1, 0, 1.0)], [0, 1, 2, 3])
    border = collect_map_border(_border_scene(translation=(10.0, 20.0, 30.0)), {42: group})
    p0, p1 = border["positions"][0], border["positions"][1]
    assert p0 == [10.0, 20.0, 30.0]
    assert abs(p1[0] - 10.0) < 1e-6 and abs(p1[1] - 20.0) < 1e-6 and abs(p1[2] - 30.9) < 1e-6


def test_collect_map_border_fail_closed():
    assert collect_map_border({"#hierarchy": []}, {}) is None
    group = _scg_group([(0, 0, 0), (0, 0, 0.9), (1, 0, 0.1), (1, 0, 1.0)], [0, 1, 2, 3])
    # datasource 不在 scg 里 ⇒ 无几何
    assert collect_map_border(_border_scene(), {}) is None


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
