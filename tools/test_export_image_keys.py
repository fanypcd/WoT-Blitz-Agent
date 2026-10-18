"""导出器**图片注册表键**回归单测：python tools/test_export_image_keys.py（standalone）。

事故（2026-10-09，用户"新湾桥面变黑"）：`GlbBuilder.add_texture` 按 key 去重，而 albedo 的
嵌入像素并不只由路径决定——`FLATCOLOR` 染色与 UV1 覆盖烘焙都会改写它。此前 key = `("jpg",
albedo_path)`：同一 albedo 的两个材质（Textured.material 实例 flatColor≈0.08 先建近黑图、
Detail.material 实例 flatColor=0.76 应得 0.258）**互相顶替** ⇒ 挡土墙沥青片渲染成黑块。
全树普查 54 个图条目 / 788 个变体受同类顶替。同族问题：动画层掩码 key 曾按 albedo_path
去重（内容实际来自 alphamask 文件）。

本测试锁：① 同 albedo、不同 `img_variant` ⇒ 两张图、两个 texture 下标；
② 同 albedo、同变体 ⇒ 仍去重（不浪费体积）；③ 掩码按 alphamask 路径去重。
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from PIL import Image  # noqa: E402

from export_map_glb import GlbBuilder, _build_material, is_sky_material  # noqa: E402


class _TexStub:
    """`_build_material` 只在 img=None 时用 textures.avg_color 兜底；本测试恒有 img。"""
    avg_color = {}


def _img(v: int) -> Image.Image:
    return Image.new("RGBA", (4, 4), (v, v, v, 255))


def _desc(name="Instance-0"):
    return {"materialName": name, "textures": {"albedo": "a/x.tex", "alphamask": "a/m0.tex"},
            "properties": {}, "flags": {}}


def test_albedo_variants_not_deduped():
    glb = GlbBuilder()
    d = _desc()
    i0 = _build_material(glb, _TexStub(), "a/x.tex", d, _img(20), False,
                         img_variant=("tint", (0.08, 0.08, 0.08)))
    i1 = _build_material(glb, _TexStub(), "a/x.tex", d, _img(200), False,
                         img_variant=("tint", (0.76, 0.76, 0.76)))
    t0 = glb.materials[i0]["pbrMetallicRoughness"]["baseColorTexture"]["index"]
    t1 = glb.materials[i1]["pbrMetallicRoughness"]["baseColorTexture"]["index"]
    assert t0 != t1, "不同染色变体必须各自成图（曾互相顶替 ⇒ 黑块事故）"
    assert len(glb.images) == 2


def test_same_variant_still_deduped():
    glb = GlbBuilder()
    d = _desc()
    a = _build_material(glb, _TexStub(), "a/x.tex", d, _img(20), False,
                        img_variant=("tint", (0.5, 0.5, 0.5)))
    b = _build_material(glb, _TexStub(), "a/x.tex", d, _img(20), False,
                        img_variant=("tint", (0.5, 0.5, 0.5)))
    assert glb.materials[a]["pbrMetallicRoughness"]["baseColorTexture"]["index"] == \
           glb.materials[b]["pbrMetallicRoughness"]["baseColorTexture"]["index"]
    assert len(glb.images) == 1, "同变体必须继续去重（否则包体积无谓膨胀）"


def test_raw_albedo_and_variant_coexist():
    glb = GlbBuilder()
    d = _desc()
    a = _build_material(glb, _TexStub(), "a/x.tex", d, _img(20), False)          # 原图
    b = _build_material(glb, _TexStub(), "a/x.tex", d, _img(200), False,
                        img_variant=("bake", "d/y.tex", None, (1.0, 1.0, 1.0)))   # 烘焙变体
    assert glb.materials[a]["pbrMetallicRoughness"]["baseColorTexture"]["index"] != \
           glb.materials[b]["pbrMetallicRoughness"]["baseColorTexture"]["index"]


def test_mask_keyed_by_alphamask_path():
    glb = GlbBuilder()
    d0 = {"materialName": "m", "textures": {"albedo": "a/x.tex", "alphamask": "a/m0.tex"},
          "properties": {}, "flags": {}}
    d1 = {"materialName": "m", "textures": {"albedo": "a/x.tex", "alphamask": "a/m1.tex"},
          "properties": {}, "flags": {}}
    i0 = _build_material(glb, _TexStub(), "a/x.tex", d0, _img(20), True,
                         blend_layer=True, mask_img=_img(10))
    i1 = _build_material(glb, _TexStub(), "a/x.tex", d1, _img(20), True,
                         blend_layer=True, mask_img=_img(250))
    t0 = glb.materials[i0]["extras"]["maskTexture"]
    t1 = glb.materials[i1]["extras"]["maskTexture"]
    assert t0 != t1, "不同 alphamask 必须各自成图（曾按 albedo_path 去重互相顶替）"


def test_sky_material_semantic_flag():
    """天空穹不得靠节点名猜：导出器按材质模板/着色器写 extras.sky。"""
    assert is_sky_material({"fxName": "Skyobject.material"}) is True
    assert is_sky_material({"fxName": "WaterAllQualities.material"}) is False
    assert is_sky_material({"materialName": "Instance-1"}, {"shader": "skyobject-materials"}) is True
    assert is_sky_material({"materialName": "SkySeaSurface"}) is False


def test_sky_material_writes_extras():
    glb = GlbBuilder()
    d = _desc()
    i = _build_material(glb, _TexStub(), "a/x.tex", d, _img(20), False, sky=True)
    assert glb.materials[i]["extras"]["sky"] is True


def test_blend_material_writes_alpha_blend_semantic_flag():
    """所有 BLEND 材质都带 alphaBlend 语义；MASK 不得误标。"""
    glb = GlbBuilder()
    d = _desc()
    blend = _build_material(glb, _TexStub(), "a/x.tex", d, _img(20), True,
                            blend=True, opacity=1.0)
    mask = _build_material(glb, _TexStub(), "a/x.tex", d, _img(20), True, alpha_cut=True)
    assert glb.materials[blend]["alphaMode"] == "BLEND"
    assert glb.materials[blend]["extras"]["alphaBlend"] is True
    assert glb.materials[mask]["alphaMode"] == "MASK"
    assert "alphaBlend" not in glb.materials[mask].get("extras", {})


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
