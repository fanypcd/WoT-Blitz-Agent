"""逐图光照提取（tools/export_map_lighting.py）纯函数单测：python tools/test_export_lighting.py。

覆盖 extract_lighting 的太阳方向约定与缺件 fail-soft：
  - 方向 = R(quaternion) · (0,−1,0)（光源**局部 −Y** = 阳光传播方向，2026-10-09 全树
    实测 36 图自洽，见 feasibility-map-lighting.md §七 #1）；
  - direction_scene = qFrame 映射 (−x, z, y)，与 WotbTools playbackScene 同式；
  - 缺件只记 warnings、不臆造默认值（消费端据此回落兜底灯光）。
"""
import math
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_lighting import (  # noqa: E402
    CONVENTION, extract_lighting, game_to_scene, sun_direction_from_quaternion,
)

# himmelsdorf 实测原始四元数与其解出方向（回归夹具：换实现即失配）
HIM_Q = [0.9606305360794067, -0.0423382967710495, -0.007296621799468994, 0.2744868993759155]
HIM_DIR = [0.0773372783319908, 0.8457287993096372, -0.527978923252131]


def _scene(q=HIM_Q, *, with_dl=True, with_transform=True, with_q=True):
    comps = []
    if with_dl:
        dl = {"comp.typename": "DirectionalLightComponent",
              "light": {"color.r": 1.0, "color.g": 1.0, "color.b": 1.0,
                        "ambColor.r": 0.88, "ambColor.g": 0.86, "ambColor.b": 1.0,
                        "intensity": 5.0, "type": 0}}
        if with_transform:
            t = {"position": [27.5, -3.7, 138.4]}
            if with_q:
                t["quaternion"] = q
            dl["transformNode"] = {"transform": t}
        comps.append(dl)
    comps.append({"comp.typename": "SceneRenderConfigComponent", "fogDensity": 0.004,
                  "FOG_ATMOSPHERE": True})
    comps.append({"comp.typename": "IBLComponent", "ibl.enableIBL": True, "ibl.dimensions": 256,
                  "ibl.environmentMultiplier": 1.0})
    return {"#sceneComponentSets": {"Default": comps}}


def test_quaternion_to_direction_matches_client_sample():
    d = sun_direction_from_quaternion(HIM_Q)
    for got, want in zip(d, HIM_DIR):
        assert abs(got - want) < 1e-9, f"方向应与实测一致：{d} vs {HIM_DIR}"


def test_scene_mapping_is_qframe():
    # qFrame = Ry(π)·Rx(−π/2)：客户端世界系（z 上）→ 场景系（y 上）
    assert game_to_scene((0.0, 0.0, 1.0)) == [0.0, 1.0, 0.0]      # 客户端"上" → 场景"上"
    assert game_to_scene((1.0, 0.0, 0.0)) == [-1.0, 0.0, 0.0]     # x 取负
    assert game_to_scene((0.0, 1.0, 0.0)) == [0.0, 0.0, 1.0]      # 客户端北(+y) → 场景 +z


def test_extract_sun_and_elevation():
    data = extract_lighting(_scene(), "19_himmelsdorf_hm")
    assert not data["warnings"], data["warnings"]
    sun = data["sun"]
    assert sun["convention"] == CONVENTION
    assert sun["intensity"] == 5.0
    assert sun["color"] == [1.0, 1.0, 1.0]
    assert [round(c, 3) for c in sun["ambient"]] == [0.88, 0.86, 1.0]
    ds = sun["direction_scene"]
    # 场景系 y 上：−dy = 仰角；himmelsdorf 实测 31.9°
    elev = math.degrees(math.asin(max(-1.0, min(1.0, -ds[1]))))
    assert 31.5 < elev < 32.5, f"仰角应 ≈31.9°，实际 {elev:.1f}°"
    # 方向必须在地平线以上（传播方向朝下 ⇒ 场景系 dy < 0）
    assert ds[1] < 0, f"阳光传播方向必须朝下，实际 {ds}"


def test_missing_components_fail_soft():
    data = extract_lighting({"#sceneComponentSets": {"Default": []}}, "x")
    assert "sun" not in data
    assert any("DirectionalLightComponent" in w for w in data["warnings"])
    assert any("SceneRenderConfigComponent" in w for w in data["warnings"])
    assert "fog" not in data and "ibl" not in data


def test_missing_quaternion_marks_warning():
    data = extract_lighting(_scene(with_q=False), "x")
    assert "sun" in data and "direction" not in data["sun"]
    assert any("quaternion" in w for w in data["warnings"])


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
