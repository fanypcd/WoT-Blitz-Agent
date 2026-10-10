"""IBL 立方图 → 等距柱状投影 纯函数单测：python tools/test_export_ibl.py（standalone）。

锁的是**朝向**（最容易错、错了观感是"反射上下颠倒"）：依客户端权威面表
（`Data/Materials/Shaders/cubemap-faces.slh`）面序 `[+X,−X,+Y,−Y,+Z,−Z]`、坐标是
DAVA 世界系（**z 上**）⇒ **face4(+Z) = 天、face5(−Z) = 地**；输出在场景系（y 上）供
three 的 `EquirectangularReflectionMapping`（行 0 = 上）。
"""
import pathlib
import struct
import sys

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_glb import DDS_FOURCC_TO_BCN  # noqa: E402
from wotb_cube import FACE_N, FACE_U, FACE_V, cube_to_equirect, decode_dds_cube  # noqa: E402


def _faces(paint: int) -> list[np.ndarray]:
    fs = [np.zeros((8, 8, 3), np.float32) for _ in range(6)]
    fs[paint] = np.ones((8, 8, 3), np.float32)
    return fs


def test_client_face_table_is_z_up():
    """面表逐字对照客户端：face4/5 是 ±Z（天/地）——写成 ±Y 就会把反射上下颠倒。"""
    assert FACE_N == [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
    assert FACE_U[4] == (1, 0, 0) and FACE_V[4] == (0, -1, 0)
    assert FACE_U[0] == (0, 0, -1) and FACE_V[0] == (0, -1, 0)


def test_face_up_lands_on_top_row():
    """face4（+Z = 天）涂白 → 等距柱**顶行**亮、底行黑（场景系 y 上、行 0 = 上）。"""
    a = np.asarray(cube_to_equirect(_faces(4), 64), np.float32)
    h = a.shape[0]
    assert a[:h // 8].mean() > 200, f'顶部应亮（天），实际 {a[:h // 8].mean():.0f}'
    assert a[-(h // 8):].mean() < 20, f'底部应黑（地），实际 {a[-(h // 8):].mean():.0f}'


def test_face_down_lands_on_bottom_row():
    a = np.asarray(cube_to_equirect(_faces(5), 64), np.float32)
    h = a.shape[0]
    assert a[-(h // 8):].mean() > 200 and a[:h // 8].mean() < 20


def test_face_px_lands_on_equirect_seam():
    """face0（+X 游戏系）整面涂白 → 场景系里它是 −X（qFrame 取负）→ 经度 φ=π 即等距柱**缝**
    （列 0/末列）。判据取**中间行**（θ≈0，正对该面）；中列（φ=0，游戏 −X）应保持黑。
    注意竖轴是场景 up（= 游戏 +Z），故单列会跨天/地两面——不能整列求均值。"""
    faces = [np.zeros((16, 16, 3), np.float32) for _ in range(6)]
    faces[0][...] = 1.0
    a = np.asarray(cube_to_equirect(faces, 128), np.float32)
    h, w = a.shape[:2]
    mid = a[h // 2 - 1:h // 2 + 2].mean(axis=(0, 2))     # θ≈0 的那几行
    edge = max(mid[0], mid[-1])
    assert edge > 200, f'缝上（列 0/末列）应亮，实际 {edge:.0f}'
    assert mid[w // 2] < 20, f'中列（游戏 −X 方向）应黑，实际 {mid[w // 2]:.0f}'
    assert mid[w // 4] < 20 and mid[3 * w // 4] < 20, '±90° 方向也应黑'


# ---------------------------------------------------------------- DDS 立方图解码（DXT1/3/5）
def _dds_cube(fourcc: bytes, w: int = 4, h: int = 4, color565: int = 0x0000,
              cube: bool = True) -> bytes:
    """手搓最小 DDS 立方图：每面 1 个 BC 块、颜色端点 = color565、索引全 0（单色面）。

    DXT3/5 的 alpha 块都写成"全 255"：DXT3 = 8 B 显式 4bit（0xFF…）、DXT5 = 端点
    a0=a1=255 + 48bit 索引 0（解码取 a0）。"""
    header = bytearray(128)
    header[0:4] = b'DDS '
    struct.pack_into('<I', header, 4, 124)                            # dwSize
    struct.pack_into('<I', header, 8, 0x1 | 0x2 | 0x4 | 0x1000)       # CAPS|H|W|PIXELFORMAT
    struct.pack_into('<I', header, 12, h)
    struct.pack_into('<I', header, 16, w)
    struct.pack_into('<I', header, 28, 1)                             # mips = 1
    struct.pack_into('<I', header, 76, 32)                            # ddspf.dwSize
    struct.pack_into('<I', header, 80, 0x4)                           # DDPF_FOURCC
    header[84:88] = fourcc
    struct.pack_into('<I', header, 108, 0x1000 | 0x8)                 # caps: COMPLEX|TEXTURE
    struct.pack_into('<I', header, 112, (0x200 | 0xFC00) if cube else 0x1000)
    color = struct.pack('<HHI', color565, color565, 0)
    if fourcc == b'DXT1':
        block = color
    elif fourcc == b'DXT3':
        block = b'\xff' * 8 + color
    else:
        block = bytes([255, 255, 0, 0, 0, 0, 0, 0]) + color
    return bytes(header) + block * 6


def test_dds_cube_dxt5_decodes_as_bc3():
    """回归（2026-10-09）：DXT3/DXT5 → BCn 编号表曾写成 DXT 号本身（{DXT5: 5}），
    等于把 DXT5 当 **BC5（双通道）** 解 → `bcn_decode` 抛 "invalid shape for BC5"，
    异常冒到调用端 try/except ⇒ **所有 DXT3/DXT5 立方图静默失效**（italy 水面 cubemap、
    idle/plant 天空立方图实测全被丢弃）。"""
    # (200,100,50) → RGB565 = (25,25,6) = 0xCB26
    dec = decode_dds_cube(_dds_cube(b'DXT5', color565=0xCB26))
    assert dec is not None, 'DXT5 立方图必须能解（曾静默失效）'
    w, mips, faces = dec
    assert len(faces) == 6 and w == 4 and mips == 1
    for f, face in enumerate(faces):
        mean = face.reshape(-1, 3).mean(axis=0) * 255
        assert np.allclose(mean, (206, 101, 49), atol=8), f'face{f} 颜色 {mean.round(1)}'


def test_dds_cube_dxt1_and_dxt3_decode():
    for fourcc in (b'DXT1', b'DXT3'):
        dec = decode_dds_cube(_dds_cube(fourcc, color565=0xCB26))
        assert dec is not None, f'{fourcc} 立方图应能解'
        assert len(dec[2]) == 6


def test_dds_cube_requires_cubemap_flag_and_known_fourcc():
    """非立方图（无 DDSCAPS2_CUBEMAP）与未知 fourCC 一律 fail-closed 返回 None。"""
    assert decode_dds_cube(_dds_cube(b'DXT5', cube=False)) is None
    assert decode_dds_cube(_dds_cube(b'ATI2')) is None       # 表外格式
    assert decode_dds_cube(b'PVR\x03' + b'\x00' * 200) is None   # 非 DDS（PVR 走另一解码器）


def test_dds_fourcc_table_matches_2d_path():
    """立方图与 2D 两条 DDS 解码路径的 fourCC→BCn 表必须逐项一致——两张表分家正是
    上面那个静默失效 bug 的成因。"""
    from wotb_cube import decode_dds_cube as _  # noqa: F401
    src = pathlib.Path(__file__).resolve().parent / "wotbtools" / "wotb_cube.py"
    body = src.read_text(encoding="utf-8")
    assert '{"DXT1": 1, "DXT3": 2, "DXT5": 3}' in body
    assert DDS_FOURCC_TO_BCN == {"DXT1": 1, "DXT3": 2, "DXT5": 3}


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
