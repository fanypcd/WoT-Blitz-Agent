"""履带印导出器契约单测：python tools/test_export_tank_tread.py（standalone，无 pytest 依赖）。

锁 `tools/export_tank_tread.py` 的口径：
- **块形状**：`treadParameters:` 顶层段、5 字段（forceManualWidth/manualWidth/
  forceManualSpace/manualSpace/texturePath）；无段 → None；缺 texturePath / 数值坏
  → fail-closed（TreadParseError），不造缺省值。
- **texturePath 形状**：只接受 `~res:/3d/Tanks/Common/images/<stem>.tex`（已普查 88/88），
  stem 是贴图文件名的唯一真源；其它形状拒绝。
- **与 suspension 块的边界**：行式解析只取顶层 `treadParameters:` 段，段外/段内嵌套键不得串。
- **产物自洽**（本地已导出时）：每个 `tank_tread/<id>.json` 的 `texture` 都指向已导出的贴图。

真实客户端抽样回归在文件末尾（客户端目录缺失时自动跳过）。
"""
import base64
import pathlib
import struct
import sys
import zlib

_HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))
sys.path.insert(0, str(_HERE / "wotbtools"))

from export_tank_tread import (  # noqa: E402
    TreadParseError, parse_tread_parameters, texture_stem,
)

YAML_FULL = """
resourcesPath:
    blitzModelPath: "Tanks/USSR/IS-7.sc2"
treadParameters:
    forceManualWidth: true
    manualWidth: 0.835616
    forceManualSpace: false
    manualSpace: 1.000000
    texturePath: "~res:/3d/Tanks/Common/images/USSR_01_tread.tex"
gunRecoil:
    gun_01:
        enabled: true
        texturePath: "~res:/elsewhere.tex"
suspension:
    enabled: true
    wheelsReactionSpeed: 1.100000
"""


def test_parse_full_block():
    doc = parse_tread_parameters(YAML_FULL)
    assert doc == {
        "texture_path": "~res:/3d/Tanks/Common/images/USSR_01_tread.tex",
        "force_manual_width": True,
        "manual_width": 0.835616,
        "force_manual_space": False,
        "manual_space": 1.0,
    }, doc


def test_no_block():
    assert parse_tread_parameters("resourcesPath:\n    blitzModelPath: x\n") is None
    # 空段（无子键）→ _section 返回 {}，缺 texturePath → fail-closed 而非 None
    try:
        parse_tread_parameters("treadParameters: {}\n")
    except TreadParseError:
        pass
    else:
        raise AssertionError("空块必须 fail-closed")


def test_fail_closed_shapes():
    bad_float = ("treadParameters:\n    forceManualWidth: true\n"
                 "    manualWidth: abc\n"
                 "    forceManualSpace: false\n    manualSpace: 1.0\n"
                 '    texturePath: "~res:/3d/Tanks/Common/images/X.tex"\n')
    try:
        parse_tread_parameters(bad_float)
    except TreadParseError:
        pass
    else:
        raise AssertionError("坏浮点必须拒绝")
    bad_bool = ("treadParameters:\n    forceManualWidth: yes-please\n"
                "    manualWidth: 1.0\n    forceManualSpace: false\n    manualSpace: 1.0\n"
                '    texturePath: "~res:/3d/Tanks/Common/images/X.tex"\n')
    try:
        parse_tread_parameters(bad_bool)
    except TreadParseError:
        pass
    else:
        raise AssertionError("坏布尔必须拒绝")
    no_tex = ("treadParameters:\n    forceManualWidth: true\n    manualWidth: 1.0\n"
              "    forceManualSpace: false\n    manualSpace: 1.0\n")
    try:
        parse_tread_parameters(no_tex)
    except TreadParseError:
        pass
    else:
        raise AssertionError("缺 texturePath 必须拒绝")


def test_texture_stem_shape():
    ok = "~res:/3d/Tanks/Common/images/USSR_01_tread.tex"
    assert texture_stem(ok) == "USSR_01_tread"
    for bad in ["3d/Tanks/Common/images/X.tex",                # 缺 ~res: 前缀
                "~res:/3d/Tanks/Common/images/X.dds",          # 非 .tex
                "~res:/3d/Tanks/Common/images/.tex",           # 空 stem
                "~res:/a/b/c.tex"]:                            # 不在 images 目录
        try:
            texture_stem(bad)
        except TreadParseError:
            continue
        raise AssertionError(f"必须拒绝: {bad!r}")


def test_pack_artifact_consistency():
    """本地已跑过导出时：json 引用的贴图必须存在（数据面自洽）。"""
    out = _HERE.parent / "data" / "tank_tread"
    tex = _HERE.parent / "data" / "tank_tread_textures"
    if not out.is_dir() or not tex.is_dir():
        print("  (跳过：本地无导出产物)")
        return
    n = 0
    for f in sorted(out.glob("*.json")):
        import json
        doc = json.loads(f.read_text(encoding="utf-8"))
        assert doc["texture"].startswith("tread/textures/"), doc
        name = doc["texture"].split("/")[-1]
        assert (tex / name).is_file(), f"{f.name} 引用的贴图缺失: {name}"
        n += 1
    print(f"  {n} 个 json 的贴图引用全部自洽")


def test_real_client_sample():
    """真实客户端抽样：IS-7 的 5 字段块 + 贴图实体存在 + DDS 可解码（alpha 软边带）。"""
    try:
        from dlc_packs import client_path
        from export_tank_glb import decode_dds, default_game_data
        from wotb_sc2 import decode_dvpl
    except ImportError:
        print("  (跳过：工具链依赖不可用)")
        return
    gd = default_game_data()
    if not gd.is_dir():
        print("  (跳过：本机无客户端目录)")
        return
    p = client_path(gd, "3d/Tanks/Parameters/USSR/IS-7.yaml.dvpl")
    if not p.exists():
        print("  (跳过：样本车 yaml 不存在)")
        return
    doc = parse_tread_parameters(decode_dvpl(p.read_bytes()).decode("utf-8", "replace"))
    assert doc is not None, "IS-7 应有 treadParameters 块"
    assert doc["texture_path"].endswith("USSR_01_tread.tex"), doc
    assert doc["force_manual_width"] is True
    assert abs(doc["manual_width"] - 0.835616) < 1e-6
    assert doc["force_manual_space"] is False and doc["manual_space"] == 1.0
    tex = client_path(gd, "3d/Tanks/Common/images/USSR_01_tread.dx11.dds.dvpl")
    assert tex.is_file(), "贴图实体必须在数据面"
    arr, _tag = decode_dds(decode_dvpl(tex.read_bytes()))
    assert arr.shape == (256, 256, 4), arr.shape
    a = arr[..., 3].astype(float)
    col = a.mean(axis=0)
    assert col[0] < 16 and col[255] < 16, "贴图横向两端必须渐隐（软边带）"
    assert col[128] > 128, "中心不透明度应显著"
    print(f"  IS-7: width {doc['manual_width']} m / 贴图 {arr.shape[1]}x{arr.shape[0]} "
          f"alpha 中心 {int(col[128])} 边缘 {int(col[0])}")


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
