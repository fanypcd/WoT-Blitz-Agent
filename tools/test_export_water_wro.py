"""WaterRenderObject 水面 UV 基捕获（tools/export_map_glb.py::wro_uv_basis）纯函数单测。

覆盖 2026-11 审计 F2（13 图 14 个 WaterRenderObject 水体主体此前整类被跳过）：
  - WRO 专属 UV 基两属性（texCoordTransform0 / inputTangent）齐全 ⇒ 全值捕获；
  - 缺任一 ⇒ None（调用方 fail-closed 整批不导，不产出错色平面）；
  - 属性字节解析用 neptune 实测原始 hex 作回归夹具（换实现即失配）；
  - MEDIUM 档水面属性（water_props）不受 WRO 改动影响（回归）。
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_glb import wro_uv_basis, water_props, WATER_PROP_DEFAULTS  # noqa: E402

# 33_neptune_nt.sc2 dataNodes[3750] 实测原始属性字节（回归夹具）
TCT_HEX = "0301000000487d943b607b0abbc07b0abb307d94bb"   # float4
TAN_HEX = "0201000000db03683f2661d8be00000080"           # float3


def _mat(props: dict) -> dict:
    return {"properties": props}


def test_wro_uv_basis_captures_both_neptune_values():
    mat = _mat({
        "texCoordTransform0": {"$bytes": TCT_HEX},
        "inputTangent": {"$bytes": TAN_HEX},
    })
    out = wro_uv_basis(mat)
    assert out is not None
    assert len(out["texCoordTransform0"]) == 4
    assert len(out["inputTangent"]) == 3
    # neptune 实测值（0x3b947d48 = 0.004532…；切线 = (0.9063, -0.4226, 0)）
    assert abs(out["texCoordTransform0"][0] - 0.004532) < 1e-5
    assert abs(out["texCoordTransform0"][1] - (-0.002113)) < 1e-5
    assert abs(out["inputTangent"][0] - 0.906309) < 1e-5


def test_wro_uv_basis_missing_either_prop_fails_closed():
    full = {
        "texCoordTransform0": {"$bytes": TCT_HEX},
        "inputTangent": {"$bytes": TAN_HEX},
    }
    only_tct = {"texCoordTransform0": full["texCoordTransform0"]}
    only_tan = {"inputTangent": full["inputTangent"]}
    for props in (only_tct, only_tan, {}):
        assert wro_uv_basis(_mat(props)) is None, props.keys()


def test_wro_uv_basis_degenerate_bytes_fail_closed():
    # 截短（解析回全零）/非 hex/非法字节：属性键存在但推导不出有效 UV 矩阵
    # ⇒ None（调用方整批不导）。全零矩阵在客户端同样不会出现（UV 恒 (0,0)）
    for tct_hex in ("00", "zz", "00000000000000000000"):
        mat = _mat({
            "texCoordTransform0": {"$bytes": tct_hex},
            "inputTangent": {"$bytes": TAN_HEX},
        })
        assert wro_uv_basis(mat) is None, tct_hex


def test_water_props_shape_unchanged():
    # neptune fresnelPow 实测字节（0x40c00000 = 6.0）
    mat = _mat({"fresnelPow": {"$bytes": "00010000000000c040"}})
    out = water_props(mat)
    assert set(out.keys()) == set(WATER_PROP_DEFAULTS.keys())
    assert out["fresnelPow"] == 6.0
    assert out["normal0Scale"] == 1.0    # 缺省回填


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
