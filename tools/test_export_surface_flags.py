"""水面标记（extras.water / asset.extras.surfaceFlags）与 LOW 档水面属性纯函数单测。

回归：消费端一度按**节点名**判水面（/water|sea|lake|river|fountain/i），把含 "water" 的
**建筑**误判成水面（水塔 `bld_er_water_tower_pbr` → 退回受光材质 → 整体发白，2026-10-09 用户
报障）。现判据是**材质文件 fxName**，结果写进材质 extras，消费端只认标记。

水面档位（2026-10-09，用户选 A）：客户端 `WaterAllQualities.material` 四档 → LOW =
`WaterPerVertexCubemap.material`（`!PIXEL_LIT`）：水色 = 双层水贴图相乘 × 3 × decalTint ×
印花 + cubemap × reflectanceColor、不透明。`water_props` 是它的逐材质属性口径。
"""
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_glb import WATER_PROP_DEFAULTS, is_water_material, water_props  # noqa: E402


def md(fx):
    return {"fxName": fx}


def prop(*vals):
    """NMaterial 属性字节：5B 头（type/count/3B 对齐）+ f32 序列。"""
    return {"$bytes": (bytes([0, len(vals), 0, 0, 0])
                       + struct.pack(f"<{len(vals)}f", *vals)).hex()}


def test_water_materials_detected():
    assert is_water_material(md("~res:/Materials/WaterAllQualities.material")) is True
    assert is_water_material(md("~res:/Materials/WaterPerPixelCubemapAlphablend.material")) is True


def test_buildings_named_water_are_not_water():
    # 水塔/水厂这类**建筑**的材质是普通受光/光照图材质，名字里带 water 不算数
    assert is_water_material(md("~res:/Materials/StandardLightmapAllQualities.material")) is False
    assert is_water_material(md("~res:/Materials/TextureLightmap.material")) is False
    assert is_water_material(md("~res:/Materials/PBR.material")) is False
    assert is_water_material({}) is False


def test_water_props_defaults_follow_client_property_values():
    # 全写：无属性时逐键落客户端 property 缺省（water-fp.sl:126-128 / water-vp.sl:121-124）。
    # 不做"只写非缺省项"的省略——消费端据此把 MEDIUM 档公式逐项照抄。
    got = water_props({})
    assert got == {"normal0Scale": 1.0, "normal1Scale": 1.0,
                   "normal0ShiftPerSecond": [0.0, 0.0], "normal1ShiftPerSecond": [0.0, 0.0],
                   "fresnelBias": 0.0, "fresnelPow": 0.0,
                   "reflectionTintColor": [1.0, 1.0, 1.0]}
    assert sorted(got) == sorted(WATER_PROP_DEFAULTS)


def test_water_props_read_authored_values():
    # erlenberg seaplane 真值（Data 侧实测）：标量落标量、向量落定长列表
    mat = {"properties": {
        "normal0Scale": prop(7.7331671714782715),
        "normal1Scale": prop(7.0),
        "normal0ShiftPerSecond": prop(0.0, 0.015382000245153904),
        "normal1ShiftPerSecond": prop(-0.01, 0.01),
        "fresnelBias": prop(0.7198669910430908),
        "fresnelPow": prop(0.7107231616973877),
        "reflectionTintColor": prop(0.70099937915802, 0.70099937915802, 0.7236133217811584),
    }}
    got = water_props(mat)
    assert got["normal0Scale"] == 7.733167
    assert got["normal1Scale"] == 7.0
    assert got["normal0ShiftPerSecond"] == [0.0, 0.015382]
    assert got["normal1ShiftPerSecond"] == [-0.01, 0.01]
    assert got["fresnelBias"] == 0.719867
    assert got["fresnelPow"] == 0.710723
    assert got["reflectionTintColor"] == [0.700999, 0.700999, 0.723613]


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
