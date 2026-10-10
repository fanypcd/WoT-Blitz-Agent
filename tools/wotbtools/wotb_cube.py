"""DAVA/WoTB 立方图工具：DDS 立方图解码 + 立方图 → 等距柱状投影（场景系）。

面表**逐字取自客户端** `Data/Materials/Shaders/cubemap-faces.slh`（即客户端 `texCUBE`
与 IBL 卷积用的同一张表）：面序 = DDS 标准 `[+X,−X,+Y,−Y,+Z,−Z]`，但坐标是 **DAVA 世界系
（z 上）** ⇒ **face4(+Z) = 天、face5(−Z) = 地**。方向 = `N + (2u−1)·U + (2v−1)·V`。
输出等距柱状投影响场景系（y 上、行 0 = 上），直接喂 three 的
`EquirectangularReflectionMapping`。

消费方：`tools/export_map_ibl.py`（逐图 IBL 环境贴图）、`tools/export_map_glb.py`
（环境反射遮罩材质的 cubemap 槽）。
"""
from __future__ import annotations

import math
import struct

import numpy as np
from PIL import Image


def _bcn(bcn: int) -> int:
    return {1: 1, 3: 3, 5: 5}[bcn]


def decode_dds_cube(raw: bytes) -> tuple[int, int, list[np.ndarray]] | None:
    """DDS 立方图 → (w, mips, [6 个 HxWx3 float 0..1 面]，mip0)。非立方图返回 None。"""
    if raw[:4] != b"DDS ":
        return None
    h = struct.unpack_from("<I", raw, 12)[0]
    w = struct.unpack_from("<I", raw, 16)[0]
    mips = struct.unpack_from("<I", raw, 28)[0] or 1
    fourcc = raw[84:88].decode(errors="replace")
    caps2 = struct.unpack_from("<I", raw, 112)[0]
    if not (caps2 & 0x200):
        return None
    # fourCC → **BCn 编号**（imagecodecs 的 `bcn_decode` 用的是 BC 编号，不是 DXT 号：
    # DXT1=BC1=1、DXT3=BC2=2、DXT5=BC3=3）。旧表写成 {DXT1:1, DXT3:3, DXT5:5} ⇒ 把 DXT5
    # 当 **BC5**（双通道）解，`bcn_decode` 抛 "invalid shape=(h,w,4) for BC5"，异常冒到
    # 调用端的 try/except ⇒ **所有 DXT3/DXT5 立方图静默失效**（2026-10-09 实测：italy 水面
    # cubemap 与 idle/plant 的天空立方图全被丢弃，水面的 cubemap 槽与 2 个环境反射静默降级）。
    # 与 2D 路径 `export_map_glb.DDS_FOURCC_TO_BCN` 必须逐项一致（有单测锁）。
    bcn = {"DXT1": 1, "DXT3": 2, "DXT5": 3}.get(fourcc)
    if bcn is None:
        return None
    block = 8 if bcn == 1 else 16
    import imagecodecs
    # DDS 立方图布局：**每面各自带完整 mip 链**（face0 mip0..N, face1 mip0..N, …）——
    # 按"逐 mip 逐面"读会把 face0 的 mip1 当成 face1（早期实测：+Y 面亮度 31 而天空应为亮）。
    face_stride = sum(max(1, max(1, w >> m) // 4) * max(1, max(1, h >> m) // 4) * block
                      for m in range(mips))
    faces = []
    for f in range(6):
        off = 128 + f * face_stride
        size = max(1, w // 4) * max(1, h // 4) * block
        data = raw[off:off + size]
        try:
            rgba = imagecodecs.bcn_decode(data, bcn, shape=(h, w, 4))
        except Exception:   # 载荷截断/未知位深 → fail-closed（调用端按"无立方图"回落）
            return None
        faces.append(rgba[..., :3].astype(np.float32) / 255.0)
    return w, mips, faces


# 客户端权威面表（Data/Materials/Shaders/cubemap-faces.slh，逐字）：
#   faceNormals = [+X, −X, +Y, −Y, +Z, −Z]（面序 = DDS 标准）
#   faceUs / faceVs 给出面内 u/v 基向量；方向 = N + (2u−1)·U + (2v−1)·V
# ⚠️ 坐标是 **DAVA 世界系（z 上）**：故 **face4(+Z) = 天、face5(−Z) = 地**
#（实测佐证：himmelsdorf face4=125 最亮 / face5=41 最暗；holland face4=61 / face5=23）。
# 旧实现按"Y 上是 up"猜 → 反射上下颠倒。
FACE_N = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
FACE_U = [(0, 0, -1), (0, 0, 1), (1, 0, 0), (1, 0, 0), (1, 0, 0), (-1, 0, 0)]
FACE_V = [(0, -1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0), (0, -1, 0)]


DEFAULT_OUT_W = 1024          # 等距柱状投影默认宽度（高度取其半）


def cube_to_equirect(faces: list[np.ndarray], out_w: int = DEFAULT_OUT_W) -> Image.Image:
    """6 面立方图 → 等距柱状投影（H = W/2），输出在**场景系（y 上）**，直接喂
    three 的 `EquirectangularReflectionMapping`（行 0 = 上、u → atan2(z, x)）。
    采样最近邻（源 256² 小图，够用）。"""
    out_h = max(1, out_w // 2)
    size = faces[0].shape[0]
    u_img = (np.arange(out_w) + 0.5) / out_w
    v_img = (np.arange(out_h) + 0.5) / out_h
    phi = (u_img - 0.5) * 2.0 * math.pi
    theta = (0.5 - v_img) * math.pi
    ct, st = np.cos(theta)[:, None], np.sin(theta)[:, None]
    cp, sp = np.cos(phi)[None, :], np.sin(phi)[None, :]
    # 场景系方向（three 约定：x = cosθcosφ, y = sinθ, z = cosθsinφ）
    ds = np.stack([ct * cp, np.broadcast_to(st, (out_h, out_w)), ct * sp], axis=-1)
    # 场景系 → 游戏系（qFrame (−x, z, y) 的逆）：x_g = −x_s, y_g = z_s, z_g = y_s
    dg = np.stack([-ds[..., 0], ds[..., 2], ds[..., 1]], axis=-1)
    axis = np.argmax(np.abs(dg), axis=-1)
    sign = np.take_along_axis(dg, axis[..., None], -1)[..., 0] >= 0
    out = np.zeros((out_h, out_w, 3), np.float32)
    for f, (n, u_ax, v_ax) in enumerate(zip(FACE_N, FACE_U, FACE_V)):
        axis_id = 0 if n[0] else (1 if n[1] else 2)
        m = (axis == axis_id) & (sign == (n[axis_id] > 0))
        if not m.any():
            continue
        dn = np.where(m, dg[..., axis_id] * n[axis_id], 1.0)          # d·N（面内为正）
        a = (dg[..., 0] * u_ax[0] + dg[..., 1] * u_ax[1] + dg[..., 2] * u_ax[2]) / dn   # d·U/(d·N)
        b = (dg[..., 0] * v_ax[0] + dg[..., 1] * v_ax[1] + dg[..., 2] * v_ax[2]) / dn   # d·V/(d·N)
        px = np.clip(((np.clip(a * 0.5 + 0.5, 0, 1)) * (size - 1)).astype(np.int64), 0, size - 1)
        py = np.clip(((np.clip(b * 0.5 + 0.5, 0, 1)) * (size - 1)).astype(np.int64), 0, size - 1)
        out[m] = faces[f][py[m], px[m]]
    return Image.fromarray((np.clip(out, 0, 1) * 255).astype(np.uint8), "RGB")


# ---------------------------------------------------------------- PVR3 立方图
_MAGIC_MARKER = b"PVR" + bytes([3]) + b"CRC_"


def decode_pvr3_cube(raw: bytes):
    """DAVA PVR3 立方图 → (w, mips, [6 个 HxWx3 float 0..1])；非立方图/不支持格式返回 None。

    头部布局同 `export_map_glb.decode_pvr3`（52B：'rgba'@8 / bits@12-16 / **h@24 / w@28** /
    depth@32 / surfaces@36 / **faces@40** / mips@44 / metaSize@48）；头后是版本字节 0x03 +
    'CRC_' 子块（len+crc，8B）+ 像素。像素布局与 DDS 立方图一致：**每面各带完整 mip 链**
    （实测 erlenberg `ErlenbergCubemap` 256²/faces=6/mips=9 载荷 6×174762 ✔）。
    位深实测只有 RGBA4444（bpp=2，r 在高半字节）。"""
    if raw[:4] != b"PVR" + bytes([3]):
        return None
    fmt = raw[8:12]
    bits = tuple(raw[12:16])
    h = struct.unpack_from("<I", raw, 24)[0]
    w = struct.unpack_from("<I", raw, 28)[0]
    faces = struct.unpack_from("<I", raw, 40)[0]
    mips = max(1, struct.unpack_from("<I", raw, 44)[0])
    # 位深实测两种：RGB565（5,6,5,0，erlenberg 水面）与 RGBA4444（4,4,4,4）；
    # 其余（含单通道）立方图未见过，fail-closed 返回 None（调用端回落旧口径）
    if faces != 6 or fmt[:1] != b"r" or bits not in ((5, 6, 5, 0), (4, 4, 4, 4)):
        return None
    idx = raw.find(_MAGIC_MARKER)
    if idx < 0:
        return None
    start = idx + len(_MAGIC_MARKER) + 8
    bpp = 2
    face_stride = 0
    cw, ch = w, h
    for _ in range(mips):
        face_stride += cw * ch * bpp
        if cw == 1 and ch == 1:
            break
        cw, ch = max(1, cw // 2), max(1, ch // 2)
    out = []
    for f in range(6):
        off = start + f * face_stride
        chunk = raw[off:off + w * h * bpp]
        if len(chunk) < w * h * bpp:
            return None
        arr = np.frombuffer(chunk, dtype="<u2").reshape(h, w)
        if bits == (5, 6, 5, 0):
            r = (((arr >> 11) & 0x1F) * 255 + 15) // 31
            g = (((arr >> 5) & 0x3F) * 255 + 31) // 63
            bl = ((arr & 0x1F) * 255 + 15) // 31
            rgba = np.stack([r, g, bl, np.full_like(r, 255)], axis=-1).astype(np.uint8)
        else:
            rgba = np.stack([((arr >> s) & 0xF) * 17 for s in (12, 8, 4, 0)], axis=-1).astype(np.uint8)
        out.append(Image.frombytes("RGBA", (w, h), rgba.tobytes()).transpose(Image.FLIP_TOP_BOTTOM)
                   .convert("RGB"))
    return w, mips, [np.asarray(im, np.float32) / 255.0 for im in out]


def decode_cube(raw: bytes):
    """DDS 立方图或 PVR3 立方图 → (w, mips, faces)；都不是则 None。

    ⚠️ 逐图水面 cubemap 两种容器都有（erlenberg/malinovka/port 是 PVR3，
    rudniki/holland/plant 是 DDS），只认 DDS 会让水面静默退回旧口径。"""
    dec = decode_dds_cube(raw)
    if dec is not None:
        return dec
    return decode_pvr3_cube(raw)
