"""烘焙光照图导出侧纯函数单测：python tools/test_export_lightmap_data.py（standalone）。

覆盖两处易错点（都不依赖客户端文件，合成数据即可）：
  1. `_prop_floats` 对 `uvScale`/`uvOffset` 的字节解析——NMaterial 属性 blob 是
     `[type u8][count u8][3B 对齐]` + f32 序列（实测 UV 变换在 offset 5），解析错位会
     让光照图整体采错区域（此前"UV1 落在图集暗区"的第二次踩坑即此类）。
  2. `GlbBuilder.add_node` 的 extras 合并：光照图逐实例变换（`lm`）与变体标签
     （`mdVariant`）必须**共存**——早先 extras 是整体赋值，新增一类会覆盖另一类。
"""
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_glb import (ClientMaterialFamily, GlbBuilder, _prop_floats,  # noqa: E402
                          detail_capable)
import tempfile  # noqa: E402
import pathlib as _p  # noqa: E402


def _vec2_blob(x: float, y: float) -> dict:
    """NMaterial 属性 blob：[type=1][count=1][3B 对齐] + 2×f32（与真机一致）。"""
    return {"$bytes": (bytes([1, 1, 0, 0, 0]) + struct.pack("<2f", x, y)).hex()}


def test_prop_floats_decodes_uv_transform():
    md = {"properties": {"uvScale": _vec2_blob(0.0625, 0.0625),
                         "uvOffset": _vec2_blob(0.06298828125, 0.31494140625)}}
    sc = _prop_floats(md, "uvScale", (0.0, 0.0))
    of = _prop_floats(md, "uvOffset", (0.0, 0.0))
    assert abs(sc[0] - 0.0625) < 1e-9 and abs(sc[1] - 0.0625) < 1e-9, sc
    assert abs(of[0] - 0.06298828125) < 1e-9 and abs(of[1] - 0.31494140625) < 1e-9, of
    # 缺属性 → 默认值，不抛
    assert _prop_floats({}, "uvScale", (1.0, 1.0)) == (1.0, 1.0)


def test_add_node_merges_lm_and_variant_extras():
    glb = GlbBuilder()
    transform = {"translation": [1.0, 2.0, 3.0], "rotation": [0.0, 0.0, 0.0, 1.0],
                 "scale": [1.0, 1.0, 1.0]}
    glb.add_node(0, transform, "ent.sc2", "md1", [0.0625, 0.03125, 0.0693, 0.4414])
    extras = glb.nodes[-1]["extras"]
    assert extras["mdVariant"] == "md1", extras
    assert extras["lm"] == [0.0625, 0.03125, 0.0693, 0.4414], extras
    # 无 lm 时不得凭空写键（旧包/非光照图节点逐字节不变）
    glb.add_node(0, transform, "ent2.sc2", None, None)
    assert "extras" not in glb.nodes[-1] or "lm" not in glb.nodes[-1].get("extras", {})



def test_lightmap_capability_criterion():
    """光照图判据：绑槽 ≠ 启用（回归：medvedkovo 外围群山被误当光照图网格 → 乘暗光图变黑斑）。

    客户端只在 `MATERIAL_LIGHTMAP` 编译期 define 下才采样光照图（materials-vp.sl:117 /
    materials-fp.sl:246；SETUP_LIGHTMAP 全库无人使用）。define 来源：① 材质文件顶层
    （TextureLightmap.material）；② 实例启用的预设（Textured/Detail + LightMap）；
    ③ 光照图模板族（*LightmapAllQualities*，审计既定口径）。
    """
    from export_map_glb import lightmap_capable, preset_defines
    tx = """Material:
    Layers: [ OpaqueRenderLayer ]
    Shader: ~res:/Materials/Shaders/Default/materials
    UniqueDefines: [ MATERIAL_TEXTURE ]
    Presets:
        LightMap:
            UniqueDefines: [ MATERIAL_LIGHTMAP ]
        AlphaBlend:
            UniqueDefines: [ ALPHABLEND, ALPHATEST ]
        AlphaTest:
            UniqueDefines: [ ALPHATEST ]
            RenderState:
                cullMode: NONE
"""
    assert preset_defines(tx, 'LightMap') == {'MATERIAL_LIGHTMAP'}
    assert preset_defines(tx, 'AlphaBlend') == {'ALPHABLEND', 'ALPHATEST'}
    assert preset_defines(tx, 'AlphaTest') == {'ALPHATEST'}      # 块边界：不含下一个预设
    assert preset_defines(tx, '不存在') == set()
    fam_txt = {'lm_top': False, 'lm_presets': {'LightMap'}}
    fam_tl = {'lm_top': True, 'lm_presets': {'LightMap'}}
    # ① 顶层 define（TextureLightmap.material 一族）
    assert lightmap_capable({'fxName': '~res:/Materials/TextureLightmap.material'}, fam_tl) is True
    # ② 实例启用了 LightMap 预设
    assert lightmap_capable({'fxName': '~res:/Materials/Textured.material',
                             'presets': {'LightMap': True}}, fam_txt) is True
    # ③ 模板族
    assert lightmap_capable({'fxName': '~res:/Materials/StandardLightmapAllQualities.material'},
                            fam_txt) is True
    # 绑槽但材质未启用 → 不得采样（群山 `mountains_001kl_` 场景）
    assert lightmap_capable({'fxName': '~res:/Materials/Textured.material'}, fam_txt) is False
    assert lightmap_capable({'fxName': '~res:/Materials/StandardAllQualities.material'},
                            {'lm_top': False, 'lm_presets': set()}) is False




def test_material_family_lm_top_excludes_presets(tmp_path=None):
    """`ClientMaterialFamily` 的 lm_top 只看 `Presets:` 之前的顶层段（回归：整文件扫描会把
    `Textured.material` 的 LightMap 预设计成"自带"，于是"绑槽即受光图"的老毛病复活）。"""
    import tempfile
    import pathlib as _p
    from export_map_glb import ClientMaterialFamily

    textured = "\n".join([
        "Material:",
        "    Layers: [ OpaqueRenderLayer ]",
        "    Shader: ~res:/Materials/Shaders/Default/materials",
        "    UniqueDefines: [ MATERIAL_TEXTURE ]",
        "    Presets:",
        "        LightMap:",
        "            UniqueDefines: [ MATERIAL_LIGHTMAP ]",
        "",
    ])
    tex_lm = "\n".join([
        "Material:",
        "    Layers: [ OpaqueRenderLayer ]",
        "    Shader: ~res:/Materials/Shaders/Default/materials",
        "    UniqueDefines: [ MATERIAL_TEXTURE, MATERIAL_LIGHTMAP ]",
        "    Presets:",
        "        AlphaTest:",
        "            UniqueDefines: [ ALPHATEST ]",
        "",
    ])
    template = "\n".join([
        "MaterialTemplate:",
        '    ULTRA: "~res:/Materials/TextureLightmap.material"',
        '    HIGH : "~res:/Materials/Textured.material"',
        "",
    ])
    with tempfile.TemporaryDirectory() as td:
        base = _p.Path(td) / "Materials"
        base.mkdir()
        (base / "Textured.material").write_text(textured, encoding="utf-8")
        (base / "TextureLightmap.material").write_text(tex_lm, encoding="utf-8")
        (base / "StandardLightmapAllQualities.material").write_text(template, encoding="utf-8")
        fam = ClientMaterialFamily(_p.Path(td))
        # 顶层 vs 预设的分界（本 bug 的要害）
        assert fam.resolve("~res:/Materials/Textured.material")["lm_top"] is False
        assert fam.resolve("~res:/Materials/Textured.material")["lm_presets"] == {"LightMap"}
        assert fam.resolve("~res:/Materials/TextureLightmap.material")["lm_top"] is True
        # 模板族：沿档位链取并集（ULTRA→TextureLightmap 的顶层 define 并进来）
        assert fam.resolve("~res:/Materials/StandardLightmapAllQualities.material")["lm_top"] is True


def test_detail_capability_criterion():
    """B1：`detail_capable` = 材质链**顶层**声明 MATERIAL_DETAIL（绑槽不够）。

    客户端证据：`materials-vp.sl:122/233`（varDetailTexCoord = uv0 × detailTileCoordScale）
    + `materials-fp.sl:100/264/347`（采样后 DRAW PHASE 末尾 color *= detail × 2.0）。
    实测 36 图 453 实例绑 detail 槽、材质全部是 `Detail.material`
    （UniqueDefines: [MATERIAL_TEXTURE, MATERIAL_DETAIL]）——其中 398 同时是光照图批次。
    """
    detail_mat = "\n".join([
        "Material:",
        "    Layers: [ OpaqueRenderLayer ]",
        "    Shader: ~res:/Materials/Shaders/Default/materials",
        "    UniqueDefines: [ MATERIAL_TEXTURE, MATERIAL_DETAIL ]",
        "    Presets:",
        "        LightMap:",
        "            UniqueDefines: [ MATERIAL_LIGHTMAP ]",
        "",
    ])
    plain = "\n".join([
        "Material:",
        "    Layers: [ OpaqueRenderLayer ]",
        "    Shader: ~res:/Materials/Shaders/Default/materials",
        "    UniqueDefines: [ MATERIAL_TEXTURE ]",
        "    Presets:",
        "        LightMap:",
        "            UniqueDefines: [ MATERIAL_LIGHTMAP ]",
        "",
    ])
    tiered = "\n".join([
        "Material:",
        "    Layers: [ OpaqueRenderLayer ]",
        "    Shader: ~res:/Materials/Shaders/Default/materials",
        "    UniqueDefines: [ MATERIAL_TEXTURE ]",
        "    QualityDependentUniqueDefines:",
        "        ULTRA: [ MATERIAL_DETAIL ]",
        "        MEDIUM: [ MATERIAL_DETAIL ]",
        "        LOW: [ ]",
        "",
    ])
    with tempfile.TemporaryDirectory() as td:
        base = _p.Path(td) / "Materials"
        base.mkdir()
        (base / "Detail.material").write_text(detail_mat, encoding="utf-8")
        (base / "Plain.material").write_text(plain, encoding="utf-8")
        (base / "TieredDetail.material").write_text(tiered, encoding="utf-8")
        fam = ClientMaterialFamily(_p.Path(td))
        f_detail = fam.resolve("~res:/Materials/Detail.material")
        f_plain = fam.resolve("~res:/Materials/Plain.material")
        f_tiered = fam.resolve("~res:/Materials/TieredDetail.material")
        assert f_detail["detail"] is True and f_detail["lm_presets"] == {"LightMap"}
        assert f_plain["detail"] is False
        assert f_tiered["detail"] is True
        # 绑槽 + 材质声明 ⇒ 导出；绑槽 + 未声明 ⇒ 不导出
        assert detail_capable({"presets": {}}, f_detail) is True
        assert detail_capable({"presets": {}}, f_plain) is False
        assert detail_capable({}, None) is False


def test_decal_capability_criterion():
    """贴花判据（`decal_capable`）：材质链顶层 MATERIAL_DECAL ⇒ 走贴花路径。

    客户端证据见 `decal_capable` docstring（materials-fp/vp.sl 的 MATERIAL_DECAL 分支）：
    `albedo(UV0) × colormap(UV1) × 2.0`，不受光——判错就落到受光材质、整体偏亮
    （2026-10-10 用户"铁轨贴图看起来太亮"：forgecity `env_fs_rails_00X` fxName = `Decal.material`）。
    """
    import export_map_glb as emg
    decal_capable = emg.decal_capable
    f_decal = {"decal": True, "decal_presets": set(), "lm_top": False, "lm_presets": set()}
    f_plain = {"decal": False, "decal_presets": set(), "lm_top": True, "lm_presets": set()}
    f_preset = {"decal": False, "decal_presets": {"Decal"}, "lm_top": False, "lm_presets": set()}
    assert decal_capable({"presets": {}}, f_decal) is True
    assert decal_capable({"presets": {}}, f_plain) is False
    assert decal_capable({"presets": {"Decal": True}}, f_preset) is True
    assert decal_capable({"presets": {}}, f_preset) is False      # 未启用预设 ⇒ 不导出（同光照图口径）
    assert decal_capable({}, None) is False


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
