"""导出器**贴花类（Decal.material）**行为回归单测：python tools/test_decal_export.py（standalone）。

背景（2026-10-14 环形轨道黑斑事件）：`Decal.material` 材质此前走 UV1 覆盖烘焙
（`bake_uv1_overlays` 把 `decal槽(UV1)×2` 乘进 albedo）。该烘焙对 UV0 重叠网格
（faust 环形轨道逐像素最多 35 重覆盖）逐覆盖就地连乘，把底色**精确压到 0**；
且消费端同时按 extras.decal 逐片元再乘一次 → 双重叠加。修复：decal 类一律不烘焙，
逐片元交消费端（`albedo(UV0) × decal槽(UV1) × [α] × 2`，客户端同式）。

本测试锁两条接口：
① 槽 = 地图地表色图（`landscape/<map>_colormap.tex`）⇒ 仅 `extras.decal`，
   **不随导** decalTexture（消费端用逐图 colormap，省体积）；
② 槽 = 材质自有贴图（投影贴花/阴影图/瓷砖等）⇒ 随导 `extras.decalTexture`
   （glTF texture 下标），消费端优先采它。
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from PIL import Image  # noqa: E402

from export_map_glb import GlbBuilder, _build_material  # noqa: E402


def _img(v: int) -> Image.Image:
    return Image.new("RGBA", (4, 4), (v, v, v, 255))


class _TexStub:
    """提供 decal 槽贴图解码结果；其余走 avg_color 兜底。"""

    avg_color: dict = {}

    def __init__(self, decal_px: int = 90):
        self._decal_px = decal_px

    def get(self, tex_path: str):
        if tex_path and "decal" in tex_path:
            return _img(self._decal_px), False
        return _img(200), False


def _desc(decal_slot: str):
    return {"materialName": "Instance-0",
            "textures": {"albedo": "a/rails.tex", "decal": decal_slot},
            "properties": {}, "flags": {"SHADOW_RECEIVER": 1}}


def test_colormap_slot_not_embedded():
    glb = GlbBuilder()
    i = _build_material(glb, _TexStub(), "a/rails.tex",
                        _desc("landscape/fa_colormap.tex"), _img(150), False,
                        decal=True)
    ex = glb.materials[i]["extras"]
    assert ex.get("decal") is True
    assert "decalTexture" not in ex, "地表色图槽不得随导（消费端持有逐图 colormap）"


def test_material_slot_embedded_as_decal_texture():
    glb = GlbBuilder()
    i = _build_material(glb, _TexStub(90), "a/rails.tex",
                        _desc("environment/images/env_fa_projector_decal.tex"),
                        _img(150), False, decal=True)
    ex = glb.materials[i]["extras"]
    assert ex.get("decal") is True
    ti = ex.get("decalTexture")
    assert isinstance(ti, int), "材质自有贴花槽必须随导 decalTexture（glTF texture 下标）"
    # 嵌入图内容 = 槽图（解码值 90），非 albedo（150）
    src = glb.textures[ti]["source"]
    assert glb.images[src]["mimeType"] == "image/jpeg"


def test_material_slot_dedup_by_slot_path():
    glb = GlbBuilder()
    i0 = _build_material(glb, _TexStub(90), "a/rails.tex",
                         _desc("environment/images/env_fa_projector_decal.tex"),
                         _img(150), False, decal=True)
    i1 = _build_material(glb, _TexStub(90), "a/walls.tex",
                         _desc("environment/images/env_fa_projector_decal.tex"),
                         _img(130), False, decal=True)
    t0 = glb.materials[i0]["extras"]["decalTexture"]
    t1 = glb.materials[i1]["extras"]["decalTexture"]
    assert t0 == t1, "同槽图跨材质必须复用同一嵌入图（按槽路径去重）"


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
