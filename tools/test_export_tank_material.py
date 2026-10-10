"""坦克材质：**无 `metallicRoughnessTexture`** 时的显式因子（glTF 默认值陷阱）纯函数单测。

回归（2026-10-09 用户"真实坦克模型看起来发白"）：glTF 的 `metallicFactor`/`roughnessFactor`
默认值是 **1.0/1.0**——不写就是"全金属 + 全粗糙"，金属没有漫反射，整块只剩被 albedo 染色的
环境反射 ⇒ 在逐图 IBL 的场景里发白。约 270 辆老式车（内联槽名 legacy `albedo`/`normalmap`）
在客户端根本没有 `_RM`/`_MISC`（40 辆 / 2996 材质实测 0/2996 存在），作者属性是
`inGlossiness`（0.5/0.4/0.3）与 `inSpecularity 0.5`、且**无金属度属性**（0/2996）。
"""
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_tank_glb import missing_mr_factors  # noqa: E402


def prop(*vals):
    """NMaterial 属性字节：5B 头（type/count/3B 对齐）+ f32 序列。"""
    return {"$bytes": (bytes([0, len(vals), 0, 0, 0]) + struct.pack(f"<{len(vals)}f", *vals)).hex()}


def test_default_is_dielectric_mid_rough():
    # 无任何属性：金属度 0（涂装钢铁） + 粗糙度 0.5（= 1 − 缺省 glossiness 0.5）
    assert missing_mr_factors({}) == (0.0, 0.5)
    assert missing_mr_factors({"properties": {}}) == (0.0, 0.5)


def test_glossiness_maps_to_one_minus_roughness():
    mat = {"properties": {"inGlossiness": prop(0.3)}}
    assert abs(missing_mr_factors(mat)[1] - 0.7) < 1e-6   # 属性是 f32：1 − 0.3f = 0.699999988…
    mat = {"properties": {"inGlossiness": prop(0.4)}}
    assert abs(missing_mr_factors(mat)[1] - 0.6) < 1e-6


def test_explicit_metallic_is_honoured_and_clamped():
    mat = {"properties": {"metallic": prop(1.0), "inGlossiness": prop(0.5)}}
    assert missing_mr_factors(mat) == (1.0, 0.5)
    mat = {"properties": {"metalness": prop(2.0), "inGlossiness": prop(-1.0)}}
    assert missing_mr_factors(mat) == (1.0, 1.0)      # 越界一律钳到 [0,1]


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
