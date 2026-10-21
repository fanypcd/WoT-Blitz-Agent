"""SpeedTree 顶点遮挡（COLOR_0）与 SPHERICAL_LIT 净染色单测（standalone）。

回归（2026-10-18，DesertSands 棕榈叶过亮）：
客户端 `speedtree-materials-vp.sl:236-285` 对**所有** SpeedTree 几何（不只锚定
叶卡）计算
    varVertexColor = 0.282094 × SH(L0) × vertexOcclusion × shOcclusionMult
    vertexOcclusion = aoContrast × (color0.r − 0.5) + 0.5 + aoBrightness
`palm1_dt/palm2_dt` 的叶片全是 w=0 固定叶（无锚定卡）⇒ 旧导出器走纯静态路径，
**不写 COLOR_0**；消费端回退 vOcc=1，且 st_tint 只写 SH 字面值（√π=1.7725）⇒
净值 1.7725×albedo，客户端只有 color0×albedo（叶均值 0.73）⇒ 约 2.4× 过曝。

本测试锁：
① COLOR_0 解码（位 2）与 fail-closed（无流/长度不齐 → None）；
② 客户端 vertexOcclusion 公式；
③ 客户端常数染色 0.282094×SH×aoMult（√π、aoMult=2 → 1.0）；
④ 组合净值 = 客户端 varVertexColor（棕榈/灌木两组实测参数）。
"""
import pathlib
import struct
import sys

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_map_glb import (  # noqa: E402
    apply_speedtree_vertex_ao,
    decode_speedtree_colors,
    speedtree_spherical_tint,
)

# 0x378f = 实测 palm/bush 的 SpeedTree 顶点掩码；COLOR0 在偏移 24、stride 92。
VF = 0x378F
STRIDE = 92


def group(rows, vf=VF, vc=None):
    payload = b"".join(rows)
    return {
        "vertexFormat": vf,
        "vertexCount": len(rows) if vc is None else vc,
        "vertices": {"$bytes": payload.hex()},
    }


def row(color):
    buf = bytearray(STRIDE)
    # POSITION @0（不影响 COLOR0 解码，仅保证行布局正确）
    struct.pack_into("<3f", buf, 0, 1.0, 2.0, 3.0)
    buf[24:28] = bytes(color)
    return bytes(buf)


def test_decode_colors_bit2():
    got = decode_speedtree_colors(group([row((255, 128, 0, 255)), row((64, 64, 64, 255))]))
    assert got is not None
    assert len(got) == 8
    assert abs(got[0] - 1.0) < 1e-6
    assert abs(got[1] - 128 / 255) < 1e-6
    assert abs(got[2] - 0.0) < 1e-6
    assert abs(got[3] - 1.0) < 1e-6
    assert abs(got[4] - 64 / 255) < 1e-6


def test_decode_colors_missing_stream_fails_closed():
    # 清掉位 2（COLOR0）→ off 无 2，必须 None（消费端回退 vOcc=1 的旧行为）
    assert decode_speedtree_colors(group([row((255, 255, 255, 255))], vf=VF & ~4)) is None


def test_decode_colors_length_mismatch_fails_closed():
    # 声明 2 顶点、实际 1 行 → None（不得按错误偏移取色）
    assert decode_speedtree_colors(group([row((255, 255, 255, 255))], vc=2)) is None


def test_vertex_ao_matches_client_formula():
    # 默认参数：vertexOcclusion = color0
    got = apply_speedtree_vertex_ao([0.5, 0.5, 0.5, 1.0], 1.0, 0.0)
    assert got == [0.5, 0.5, 0.5, 1.0]
    # bush 实测：contrast=1 / brightness=0.4
    got = apply_speedtree_vertex_ao([0.5, 0.5, 0.5, 1.0], 1.0, 0.4)
    assert abs(got[0] - 0.9) < 1e-6 and got[1] == got[0] and got[2] == got[0]
    # 对比度参与：3×(0.5−0.5)+0.5+0.5 = 1.0
    got = apply_speedtree_vertex_ao([0.5, 0.5, 0.5, 1.0], 3.0, 0.5)
    assert abs(got[0] - 1.0) < 1e-6
    # 保留 A 通道；RGB 三通道写同值（客户端 color0.rgb 同值）
    got = apply_speedtree_vertex_ao([0.25, 0.5, 0.75, 0.2], 1.0, 0.0)
    assert abs(got[0] - 0.25) < 1e-6
    assert got[1] == got[0] and got[2] == got[0]
    assert abs(got[3] - 0.2) < 1e-6


def test_spherical_tint_matches_client_constant():
    sqrt_pi = 1.772458791732788
    # √π 灰 + aoMult=2（HIGH/MEDIUM/LOW 默认）→ 1.0（旧口径直接写 1.7725，偏亮 77%）
    got = speedtree_spherical_tint(sqrt_pi, 2.0)
    assert all(abs(c - 1.0) < 1e-6 for c in got), got
    # 每树彩色 SH（karelia 实测）逐通道按 0.282094×2 缩放
    got = speedtree_spherical_tint((0.37, 0.50, 0.50), 2.0)
    assert abs(got[0] - 0.37 * 0.282094 * 2) < 1e-6
    assert abs(got[1] - 0.50 * 0.282094 * 2) < 1e-6
    # 标量 SH 展开成三通道；坏值护栏钳在 2.0
    got = speedtree_spherical_tint(3.0, 1.0)
    assert got == (2.0 * 0.282094,) * 3


def test_palm_and_bush_net_match_client():
    """消费端净值 = tint × vOcc，必须等于客户端 varVertexColor。"""
    sqrt_pi = 1.772458791732788
    tint = speedtree_spherical_tint(sqrt_pi, 2.0)[0]   # ≈ 1.0
    # 棕榈叶：默认 AO（brightness 0，contrast 1），color0 均值 0.732
    palm_occ = apply_speedtree_vertex_ao([0.732, 0.732, 0.732, 1.0], 1.0, 0.0)[0]
    palm_net = tint * palm_occ
    palm_client = 0.282094 * sqrt_pi * 0.732 * 2.0
    assert abs(palm_net - palm_client) < 1e-6
    assert abs(palm_net - 0.732) < 1e-6
    # 灌木：brightness 0.4，color0 均值 0.547
    bush_occ = apply_speedtree_vertex_ao([0.547, 0.547, 0.547, 1.0], 1.0, 0.4)[0]
    bush_net = tint * bush_occ
    bush_client = 0.282094 * sqrt_pi * (0.547 + 0.4) * 2.0
    assert abs(bush_net - bush_client) < 1e-6
    assert abs(bush_net - 0.947) < 1e-6


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
