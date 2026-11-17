"""DeferredDecal 投影贴花提取（tools/export_map_glb.py::collect_deferred_decals）纯函数单测。

覆盖 2026-11 审计 B9（plant 490 个投影贴花此前整类被跳过）：
  - 组件字段 → 契约记录（blending/blendingName/uvOffset/uvScale/box/flatColor）；
  - 隐藏实体（visibility bit0/bit1）跳过；
  - atlas 槽解析不到实体文件 ⇒ fail-closed 丢弃该贴花（不猜 dec_N 兜底）；
  - blending 枚举名与客户端 blending.slh 逐字对齐。
"""
import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_glb import collect_deferred_decals, DECAL_BLENDING_NAMES  # noqa: E402

# 存在性判据需要真实文件：桩根目录里预置 atlas0（missing_* 不预置）
_STUB_ROOT = pathlib.Path(tempfile.mkdtemp(prefix="wotb_decals_test_"))
(_STUB_ROOT / "decals").mkdir(parents=True)
(_STUB_ROOT / "decals" / "atlas0.tex").write_bytes(b"stub")


class _StubTextures:
    """textures._candidates 的最小桩：候选路径指向预置的真实/缺失文件。"""

    def _candidates(self, path: str):
        return [_STUB_ROOT / path.replace("~res:/", "")]


def _entity(name: str, *, visibility: int = 0, blending: int = 5,
            flat: bool = False, tex: str = "decals/atlas0.tex") -> dict:
    dec = {
        "comp.typename": "DeferredDecalComponent",
        "backSideFade": False,
        "blending": blending,
        "decalComponent.atlasInfo": {
            "atlasInfo.albedo": tex,
            "atlasInfo.uvOffset": [0.9296875, 0.20556640625],
            "atlasInfo.uvScale": [0.0625, 0.068359375],
        },
        "decalComponent.decal": {"atlasInfo.albedo": "decals/dec_21.tex",
                                 "atlasInfo.uvOffset": [0.0, 0.0],
                                 "atlasInfo.uvScale": [1.0, 1.0]},
        "fadeDistance": 0.0,
        "fogBehaviour": 2,
        "isShadowReceiver": False,
        "layer": 0,
        "verticalFadeWidth": 0.0,
    }
    if flat:
        dec["flatColor"] = [1.3726707696914673, 1.3726707696914673, 1.3726707696914673, 1.0]
    return {
        "name": name,
        "visibility": visibility,
        "components": {"0000": dec,
                       "0001": {"comp.typename": "TransformComponent",
                                "tc.worldTranslation": [1.0, 2.0, 3.0],
                                "tc.worldScale": [2.0, 4.0, 6.0],
                                "tc.worldRotation": [0.0, 0.0, 0.0, 1.0]}},
    }


def test_blending_names_match_client_slh():
    # Materials/Shaders/blending.slh 的 #define 序列（逐字）
    assert DECAL_BLENDING_NAMES[1] == "alphaBlend"
    assert DECAL_BLENDING_NAMES[2] == "additive"
    assert DECAL_BLENDING_NAMES[5] == "multiplicative"


def test_collect_records_fields():
    scene = {"#hierarchy": [_entity("angle_1", blending=2, flat=True)]}
    records, texs = collect_deferred_decals(scene, _StubTextures())
    assert len(records) == 1
    rec = records[0]
    assert rec["name"] == "angle_1"
    assert rec["blending"] == 2 and rec["blendingName"] == "additive"
    assert rec["texture"] == "decals/atlas0.tex"
    assert rec["uvOffset"] == [0.9296875, 0.20556640625]
    assert rec["uvScale"] == [0.0625, 0.068359375]
    assert rec["box"] == {"translation": [1.0, 2.0, 3.0],
                          "scale": [2.0, 4.0, 6.0],
                          "rotation": [0.0, 0.0, 0.0, 1.0]}
    # FLATCOLOR define 未在 DeferredDecal.material 声明 ⇒ 客户端不采；数据如实随档
    assert rec["flatColor"][0] == 1.372671
    assert texs == {"decals/atlas0.tex": "decal_atlas0.webp"}


def test_hidden_entity_skipped():
    scene = {"#hierarchy": [_entity("a", visibility=1), _entity("b", visibility=2),
                            _entity("c", visibility=0)]}
    records, _ = collect_deferred_decals(scene, _StubTextures())
    assert [r["name"] for r in records] == ["c"]


def test_missing_atlas_texture_dropped():
    scene = {"#hierarchy": [_entity("ok"),
                            _entity("bad", tex="decals/missing_atlas.tex")]}
    records, texs = collect_deferred_decals(scene, _StubTextures())
    assert [r["name"] for r in records] == ["ok"]
    assert "decals/missing_atlas.tex" not in texs


def test_no_decals_returns_empty():
    records, texs = collect_deferred_decals({"#hierarchy": []}, _StubTextures())
    assert records == [] and texs == {}


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
