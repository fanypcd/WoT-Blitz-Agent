"""变体标签解析纯函数单测：python tools/test_export_variants.py（standalone，无 pytest 依赖）。

覆盖 resolve_variant_map / scene_variant_labels / entity_variant_label ——
多变体地图（Dead Rail 等 9 图）的 mdN 组 ↔ 注册表 key 序配对口径。
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_glb import (  # noqa: E402
    MapEntry, entity_variant_label, resolve_variant_map, scene_variant_labels,
)


def ent(label=None):
    e = {"name": "x.sc2"}
    if label is not None:
        e["components"] = {"0000": {
            "comp.typename": "LabelComponent", "lc.labels": label}}
    return e


def scene(*entities):
    return {"#hierarchy": list(entities)}


def entry(map_id, key):
    return MapEntry(map_id, key, "04_x_xx", "x", "X")


def test_entity_variant_label():
    assert entity_variant_label(ent(["md3"])) == "md3"
    assert entity_variant_label(ent(["dt2"])) == "dt2"
    assert entity_variant_label(ent(["ordeal"])) is None      # 无数字尾缀 = 非变体组
    assert entity_variant_label(ent(["md1", "md2"])) is None  # 多标签口径外
    assert entity_variant_label(ent()) is None


def test_scene_variant_labels_sorted_by_number():
    s = scene(ent(["md2"]), ent(["md1"]), ent(["md3"]), ent(["ordeal"]), ent())
    assert scene_variant_labels(s) == ["md1", "md2", "md3"]
    # erlenberg 系从 0 起
    s2 = scene(ent(["er2"]), ent(["er0"]), ent(["er1"]))
    assert scene_variant_labels(s2) == ["er0", "er1", "er2"]


def test_resolve_variant_map_medvedkovo():
    # 基础键无后缀在前，其余按后缀数字；组按标签数字——两列按序 zip
    keys = [entry(7, "medvedkovo"), entry(46, "medvedkovo_02"), entry(47, "medvedkovo_03")]
    s = scene(ent(["md3"]), ent(["md1"]), ent(["md2"]))
    assert resolve_variant_map(keys, s) == {7: "md1", 46: "md2", 47: "md3"}


def test_resolve_variant_map_erlenberg_ordinal():
    # erlenberg：_01↔er1（组内 spawn 名是 Spawn_02_* —— 按序数而非字面后缀配对）
    keys = [entry(3, "erlenberg"), entry(54, "erlenberg_01"), entry(55, "erlenberg_02")]
    s = scene(ent(["er1"]), ent(["er2"]), ent(["er0"]))
    assert resolve_variant_map(keys, s) == {3: "er0", 54: "er1", 55: "er2"}


def test_resolve_variant_map_milbase_five():
    keys = [entry(25, "milbase"), entry(50, "milbase_02"), entry(51, "milbase_03"),
            entry(52, "milbase_04"), entry(53, "milbase_05")]
    s = scene(*[ent([f"mlb{i}"]) for i in (3, 1, 5, 2, 4)])
    assert resolve_variant_map(keys, s) == {
        25: "mlb1", 50: "mlb2", 51: "mlb3", 52: "mlb4", 53: "mlb5"}


def test_resolve_variant_map_fail_open():
    # 组数 ≠ key 数 → {}（不写映射，前端不裁剪，fail-open 保持现状）
    keys = [entry(7, "medvedkovo"), entry(46, "medvedkovo_02"), entry(47, "medvedkovo_03")]
    assert resolve_variant_map(keys, scene(ent(["md1"]), ent(["md2"]))) == {}
    assert resolve_variant_map(keys, scene(ent())) == {}
    # 单变体图：组=1、key=1 → 自映射（前端裁剪为空操作）
    assert resolve_variant_map([entry(19, "lagoon")], scene(ent(["lg0"]))) == {19: "lg0"}


def test_decode_group_uvs_tiling_not_rejected():
    """平铺 UV（|v|>64）是合法 TEXCOORD0，不得按幅值拒绝回退到位 4 备用对
    （Dead Rail 铁轨灰带实测根因）。vf=411 布局：pos12+normal12+uv0(8)+uv1(8)+…"""
    import struct as _s
    import numpy as np
    from export_map_glb import decode_group_uvs

    vf = 411
    vc = 2
    row = [1.0, 2.0, 3.0,       # pos
           0.0, 1.0, 0.0,       # normal
           0.38, -118.5,        # TEXCOORD0（平铺，v 超过旧 64 幅值守卫）
           0.48, 0.99,          # TEXCOORD1（图集备用对）
           0.0, 0.0, 0.0, 0.0,  # 位 7/8 区（解码只读低偏移，此处仅凑足 stride）
           0.0, 0.0]            # 补齐 64B（16 floats）
    assert len(row) * 4 == 64
    payload = b''.join(_s.pack('<16f', *row) for _ in range(vc))
    group = {"vertexFormat": vf, "vertexCount": vc,
             "vertices": {"$bytes": payload.hex()}}
    dec = decode_group_uvs(group)
    assert dec is not None, '平铺 UV 不应被整组拒绝'
    uv0, uv1, ch = dec
    assert ch == 3, f'应优先位 3（客户端 TEXCOORD0），实际 {ch}'
    assert abs(uv0[0][0] - 0.38) < 1e-5 and abs(uv0[0][1] + 118.5) < 1e-5, f'UV0 应为平铺对: {uv0[0]}'
    assert abs(uv1[0][0] - 0.48) < 1e-5, f'UV1 应为位 4 备用对: {uv1[0]}'


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
