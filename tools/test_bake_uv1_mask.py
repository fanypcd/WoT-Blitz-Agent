"""UV1 覆盖烘焙的掩码通道单测：python tools/test_bake_uv1_mask.py（standalone，无 pytest 依赖）。

覆盖 bake_uv1_overlays 的 alphamask 取值：
  客户端 materials-fp.sl 用 `FP_A8(tex2D(alphamask, uv1))` 取掩码贴图的**单通道值**，
  而这些掩码多为 PVR3 8bpp 单通道（L8/A8），本仓 decode_pvr3 把值放在 RGB、A 恒 255；
  RGBA 掩码才把真值放在 A。
  ⚠️ 回归：旧实现恒取 `.a` → 单通道掩码恒等于 1 → 烘焙 alpha 变二值剪影（0/255、零中间调），
  himmelsdorf 烟囱烟雾等 15 图 144 个 mask 批次渲染成硬边卡片（2026-10-09 用户报障）。
"""
import pathlib
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_glb import bake_uv1_overlays, mask_channel_values  # noqa: E402

N = 16
# 全覆盖四边形（UV0 = UV1 = 单位方），两个三角形
UVS = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
IDX = [0, 1, 2, 3, 4, 5]


def _gradient_mask_1ch():
    """单通道掩码：值落在 RGB（A=255）——本仓 decode_pvr3 的 L8/A8 落点。"""
    v = np.linspace(0, 255, N).astype(np.uint8)
    rgb = np.tile(v, (N, 1))
    a = np.full((N, N), 255, np.uint8)
    return Image.fromarray(np.stack([rgb, rgb, rgb, a], -1), "RGBA")


def _gradient_mask_rgba():
    """RGBA 掩码：真值在 A，RGB 无信息（恒 255）。"""
    v = np.linspace(0, 255, N).astype(np.uint8)
    a = np.tile(v, (N, 1))
    rgb = np.full((N, N), 255, np.uint8)
    return Image.fromarray(np.stack([rgb, rgb, rgb, a], -1), "RGBA")


def _base():
    """albedo：alpha 恒 255（这些材质的 alpha 只能来自掩码）。"""
    rgb = np.full((N, N, 3), 128, np.uint8)
    a = np.full((N, N, 1), 255, np.uint8)
    return Image.fromarray(np.concatenate([rgb, a], -1), "RGBA")


def _baked_alpha(mask_img, tint=(1.0, 1.0, 1.0)):
    out = bake_uv1_overlays(_base(), None, mask_img, tint, 1.0, UVS, UVS, IDX)
    return np.asarray(out.convert("RGBA"), np.float32)[..., 3]


def test_single_channel_mask_uses_luminance():
    """单通道掩码（值在 RGB、A=255）→ 烘焙 alpha 取该值，而不是恒 1。"""
    alpha = _baked_alpha(_gradient_mask_1ch())
    assert alpha.max() > 240, f'掩码亮端应烘出高 alpha，实际 max={alpha.max()}'
    assert alpha.min() < 16, f'掩码暗端应烘出低 alpha，实际 min={alpha.min()}'
    soft = ((alpha > 25) & (alpha < 230)).mean()
    assert soft > 0.3, f'应为软渐变（中间调占比 >30%），实际 {soft * 100:.1f}%（二值剪影 = 旧 bug）'


def test_rgba_mask_uses_alpha_channel():
    """RGBA 掩码 → 仍取 A 通道（与客户端 FP_A8 同源）。"""
    alpha = _baked_alpha(_gradient_mask_rgba())
    assert alpha.max() > 240 and alpha.min() < 16
    soft = ((alpha > 25) & (alpha < 230)).mean()
    assert soft > 0.3, f'应为软渐变，实际 {soft * 100:.1f}%'


def test_flat_tint_applied_without_decal():
    """无 decal 的 FLATCOLOR：RGB 整图染色（alpha 不受影响）。"""
    out = bake_uv1_overlays(_base(), None, _gradient_mask_1ch(),
                            (0.5, 0.25, 1.0), 1.0, UVS, UVS, IDX)
    arr = np.asarray(out.convert("RGBA"), np.float32)
    assert abs(arr[..., 0].mean() - 128 * 0.5) < 3, 'R 通道应乘 0.5'
    assert abs(arr[..., 1].mean() - 128 * 0.25) < 3, 'G 通道应乘 0.25'


def test_mask_channel_values_ssot():
    """mask_channel_values 与烘焙同口径（SSOT）：单通道取亮度、RGBA 取 A。"""
    v = np.linspace(0, 255, N).astype(np.uint8)
    row = np.tile(v, (N, 1))
    got1 = mask_channel_values(_gradient_mask_1ch())
    got2 = mask_channel_values(_gradient_mask_rgba())
    for got, img in ((got1, None), (got2, None)):
        assert got.min() < 0.05 and got.max() > 0.95
        assert np.all(np.diff(got[N // 2]) > 0), '应随梯度单调递增'
    assert np.allclose(got1, got2, atol=1e-6), '两种掩码应得同一取值'


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
