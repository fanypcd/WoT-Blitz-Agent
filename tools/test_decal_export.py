"""导出器**贴花类**行为回归单测：python tools/test_decal_export.py（standalone）。

背景（2026-10-14 环形轨道黑斑事件 + 同日晚全量审计）：
- decal 材质此前走 UV1 覆盖烘焙（`bake_uv1_overlays` 把 `decal槽(UV1)×2` 乘进 albedo）。
  该烘焙对 UV0 重叠网格逐覆盖就地连乘，把底色**精确压到 0**（faust 环形轨道 35 重覆盖）；
  且消费端同时按 extras.decal 逐片元再乘一次 → 双重叠加。
- 审计进一步发现：烘焙触发条件只看"槽位存在"，而客户端是否采样 decal 槽由**有效 define
  集**（材质文件链 ∪ 实例启用预设 ∪ **实例 flags**）决定——pliego 边境 3 例 `MATERIAL_DECAL`
  只在 flags 里，被旧判据漏掉后照烘（均值 5.08 重连乘）；plant 路面实例无任何 define 却
  也被烘（凭空多乘）。修复：decal **一律不烘焙**，define 生效者逐片元交消费端
  （`albedo(UV0) × decal槽(UV1) × [α] × 2`，客户端同式）；define 未生效者槽位忽略。

本测试锁接口：
① 槽 = 地面 `colorTexture` **逐字同一路径** ⇒ 仅 `extras.decal`，**不随导** decalTexture
   （消费端用逐图 colormap，省体积）；
② 槽 ≠ 地面色图 ⇒ 随导 `extras.decalTexture`（glTF texture 下标），消费端优先采它；
③ 身份按**路径**判、不按文件名启发式（'*colormap*' 字串曾把 pliego_colorTexture 误判）；
④ `extras.decalSeparateLm` 逐实例下发（LSC define；旧的逐图 separate_lm 猜测会错乘
   faust 10 个无 LSC 实例）；
⑤ `decal_capable` 的 define 集判据（文件链 ∪ flags）；`flatcolor_defined` 的
   `FLATCOLOR || FLATALBEDO` 判据。
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from PIL import Image  # noqa: E402

from export_map_glb import (GlbBuilder, _build_material,  # noqa: E402
                            decal_capable, flatcolor_defined)

FA_CM = "landscape/fa_colormap.tex"
OWN_SLOT = "environment/images/env_fa_projector_decal.tex"


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


def _desc(decal_slot: str, flags=None):
    return {"materialName": "Instance-0",
            "textures": {"albedo": "a/rails.tex", "decal": decal_slot},
            "properties": {}, "flags": flags or {"SHADOW_RECEIVER": 1}}


def _build(slot, ground_cm, flags=None):
    glb = GlbBuilder()
    i = _build_material(glb, _TexStub(), "a/rails.tex", _desc(slot, flags), _img(150),
                        False, decal=True, ground_cm_slot=ground_cm)
    return glb, i


def test_colormap_slot_not_embedded():
    glb, i = _build(FA_CM, FA_CM)
    ex = glb.materials[i]["extras"]
    assert ex.get("decal") is True
    assert "decalTexture" not in ex, "地面色图槽不得随导（消费端持有逐图 colormap）"


def test_material_slot_embedded_as_decal_texture():
    glb, i = _build(OWN_SLOT, FA_CM)
    ex = glb.materials[i]["extras"]
    assert ex.get("decal") is True
    ti = ex.get("decalTexture")
    assert isinstance(ti, int), "材质自有贴花槽必须随导 decalTexture（glTF texture 下标）"
    src = glb.textures[ti]["source"]
    assert glb.images[src]["mimeType"] == "image/jpeg"


def test_identity_not_name_heuristic():
    """身份按槽路径逐字相等判：名字含/不含 'colormap' 都不是依据。"""
    # 名字不含 colormap，但就是地面色图（pliego 实测口径）⇒ 不随导
    glb, i = _build("landscape/pliego_colorTexture.tex", "landscape/pliego_colorTexture.tex")
    assert "decalTexture" not in glb.materials[i]["extras"]
    # 名字像色图，但与地面色图不是同一文件 ⇒ 必须随导
    glb, i = _build("landscape/other_colormap.tex", FA_CM)
    assert isinstance(glb.materials[i]["extras"].get("decalTexture"), int)


def test_decal_separate_lm_explicit_boolean():
    """LSC define（逐实例 flags）⇒ extras.decalSeparateLm 显式下发（真/假都要有）。"""
    glb, i = _build(FA_CM, FA_CM, flags={"LANDSCAPE_SEPARATE_LIGHTMAP_CHANNEL": 1})
    assert glb.materials[i]["extras"].get("decalSeparateLm") is True
    glb, i = _build(FA_CM, FA_CM, flags={"SHADOW_RECEIVER": 1})
    assert glb.materials[i]["extras"].get("decalSeparateLm") is False, \
        "无 LSC 必须显式 False（缺失会让消费端回退逐图近似而错乘）"


def test_material_slot_dedup_by_slot_path():
    glb = GlbBuilder()
    i0 = _build_material(glb, _TexStub(90), "a/rails.tex", _desc(OWN_SLOT), _img(150),
                         False, decal=True, ground_cm_slot=FA_CM)
    i1 = _build_material(glb, _TexStub(90), "a/walls.tex", _desc(OWN_SLOT), _img(130),
                         False, decal=True, ground_cm_slot=FA_CM)
    t0 = glb.materials[i0]["extras"]["decalTexture"]
    t1 = glb.materials[i1]["extras"]["decalTexture"]
    assert t0 == t1, "同槽图跨材质必须复用同一嵌入图（按槽路径去重）"


def test_decal_capable_define_union():
    """decal 判据 = 文件链 ∪ 预设 ∪ 实例 flags（PBR_DECAL 同判）。"""
    family = {"decal": True, "decal_presets": set()}
    assert decal_capable(_desc(FA_CM), family), "文件链顶层 define 命中（faust 轨道口径）"
    assert decal_capable(_desc(FA_CM, {"MATERIAL_DECAL": 1}), None), \
        "实例 flags 的 MATERIAL_DECAL 命中（pliego 边境口径）"
    assert decal_capable(_desc(FA_CM, {"PBR_DECAL": 1}), None), "PBR_DECAL 同判"
    assert not decal_capable(_desc(FA_CM, {"SHADOW_RECEIVER": 1}), None), \
        "无任何 define 来源不得判 decal（plant 路面口径：槽位是弃用残留）"


def test_flatcolor_defined_both_defines():
    """客户端 `#if FLATCOLOR || FLATALBEDO`：两 define 同义。"""
    assert flatcolor_defined({"flags": {"FLATCOLOR": 1}})
    assert flatcolor_defined({"flags": {"FLATALBEDO": 1}}), \
        "FLATALBEDO-only（forgecity 边境 36 + plant 1）此前被丢染色"
    assert not flatcolor_defined({"flags": {"SHADOW_RECEIVER": 1}})
    assert not flatcolor_defined({})


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
