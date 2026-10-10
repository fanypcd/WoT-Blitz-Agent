"""贴地结构脚印烘焙的纯函数契约（standalone：python tools/test_bake_ground_structs.py）。"""

import pathlib
import sys

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import bake_ground_structs as B  # noqa: E402


def test_world_mapping():
    # GLB/场景系 (x,y,z) → 世界 (x=−sx, y=sz, z=sy)；与前端 mapScenery.rotation(-π/2, π, 0) 同式
    p = np.array([[1.0, 2.0, 3.0], [-4.0, 5.0, 6.0]])
    w = B.glb_world_xyz(p)
    assert np.allclose(w, [[-1.0, 3.0, 2.0], [4.0, 6.0, 5.0]])


def test_up_faces():
    # 缠绕按右手（GLB 正面）取：法线 = (b−a)×(c−a)
    up = np.array([[[0, 0, 0], [0, 0, 1], [1, 0, 0]]], dtype=float)         # 法线 +y
    side = np.array([[[0, 0, 0], [0, 0, 1], [0, 1, 0]]], dtype=float)       # 法线 ±x
    down = np.array([[[0, 0, 0], [1, 0, 0], [0, 0, 1]]], dtype=float)       # 法线 −y
    ramp = np.array([[[0, 0, 0], [0, 0, 1], [1, 1, 0]]], dtype=float)       # 45° 斜面（n·up≈0.707）
    assert bool(B.up_faces(up)[0])
    assert not bool(B.up_faces(side)[0])
    assert not bool(B.up_faces(down)[0])
    assert bool(B.up_faces(ramp)[0])
    # 退化面片（零面积）必须被排除，不能因除零崩掉
    assert not bool(B.up_faces(np.array([[[0, 0, 0], [0, 0, 0], [0, 0, 0]]], dtype=float))[0])


def test_texel_index():
    # 世界 (0,0) 在 512/span600 下 = 网格中心
    i, j = B.texel_index(0.0, 0.0, 512, 600.0)
    assert (i, j) == (256.0, 256.0)
    # 西边沿 = 列 0；南边沿 = 行 0
    i, j = B.texel_index(-300.0, -300.0, 512, 600.0)
    assert (i, j) == (0.0, 0.0)


def test_classify_ground_lying():
    nat = np.array([[20.0, 20.0, 20.0, 20.0, 20.0]])
    surf = np.array([[20.3, 20.6, 19.95, 19.5, np.nan]])
    m = B.classify_ground_lying(surf, nat, 0.15, 0.5)
    assert bool(m[0, 0])          # 高出 0.3 ⇒ 贴地低矮构件
    assert not bool(m[0, 1])      # 高出 0.6 > band_high ⇒ 离地结构（桥/高架）
    assert bool(m[0, 2])          # 低 0.05 ⇒ 吸附齐平
    assert not bool(m[0, 3])      # 低 0.5 > band_low ⇒ 埋在铺装下的箱体（不细分）
    assert not bool(m[0, 4])      # 无结构面


def test_dilate_footprint():
    """膨胀 = chamfer 距离（正交 1 / 对角 √2）≤ dilate；false positive 只费三角形、不改高度。"""
    cov = np.zeros((7, 7), dtype=bool)
    cov[3, 3] = True
    f = B.dilate_footprint(cov, 2)
    assert f[3, 3]
    assert f[3, 5] and f[5, 3] and f[1, 3] and f[3, 1]      # 正交 d=2
    assert f[2, 3] and f[3, 2]                              # 正交 1
    assert f[2, 2] and f[2, 4] and f[4, 2] and f[4, 4]      # 对角 √2
    assert not f[1, 4]                                      # √2+1 = 2.414 > 2
    assert not f[0, 0]                                      # 3·√2 > 2
    # dilate=0 / 空输入：原样
    assert np.array_equal(B.dilate_footprint(cov, 0), cov)
    empty = np.zeros((4, 4), dtype=bool)
    assert not B.dilate_footprint(empty, 4).any()
    # 连通条带：正交邻域全膨胀（dilate=1 的菱形不含对角）
    bar = np.zeros((9, 9), dtype=bool)
    bar[4, 1:8] = True
    fb = B.dilate_footprint(bar, 1)
    assert fb[4].all() and fb[3, 4] and fb[5, 4] and fb[3, 1] and fb[3, 7] and fb[5, 1] and fb[5, 7]
    assert fb[4, 0] and fb[4, 8]                            # 条带端点的正交外扩
    assert not fb[3, 0] and not fb[5, 8]                    # 端点的对角 1.414 > 1
    assert not fb[2, 4] and not fb[6, 4]                    # 纵向仅 ±1
    assert not fb[0, 0]


def test_footprint_u8():
    cov = np.array([[False, True], [False, False]])
    out = B.footprint_u8(B.dilate_footprint(cov, 0))
    assert out.dtype == np.uint8
    assert out.tolist() == [[0, 1], [0, 0]]


def test_water_surfaces_and_submerged():
    """水面片自身与水下薄板必须被排除出"贴地结构"（脚印铺满水体会触发整片无谓的岸上细分；
    让位掩码时代 2026-10-10 同一教训）。"""
    # 一片水面（6×6 m 高 12.26）+ 其下 1 cm 的不透明冰（12.25）+ 岸上铁轨板（12.25，但不在水下）
    water = np.array([[[0, 12.26, 0], [6, 12.26, 0], [6, 12.26, 6]],
                      [[0, 12.26, 0], [6, 12.26, 6], [0, 12.26, 6]]], dtype=float)
    ice = np.array([[[0, 12.25, 0], [6, 12.25, 0], [6, 12.25, 6]],
                    [[0, 12.25, 0], [6, 12.25, 6], [0, 12.25, 6]]], dtype=float)
    rails = np.array([[[20, 12.25, 20], [22, 12.25, 20], [22, 12.25, 22]]], dtype=float)
    all_tris = np.concatenate([water, ice, rails])
    surf = B.water_surfaces_of(water)
    assert len(surf) == 2 and abs(surf[0][0] - 12.26) < 1e-9
    sub = B.submerged_faces(all_tris, surf)
    assert sub[2] and sub[3]                  # 水下冰：整个在水面占地内且低 1 cm ⇒ 淹没
    assert not sub[4]                         # 岸上的铁轨板：不在水面占地内 ⇒ 不淹没
    # 烘焙口径（bake_map 同式）：水面片按**材质**排除、水下件按淹没排除 ⇒ 只有岸上铁轨板留下
    is_water = np.array([True, True, False, False, False])
    kept = np.ones(len(all_tris), dtype=bool) & ~is_water & ~sub
    assert not kept[0] and not kept[1] and not kept[2] and not kept[3] and kept[4]
    # 水面**下方但只在占地外**的不淹没；高出水面的不淹没
    outside = np.array([[[20, 12.0, 20], [21, 12.0, 20], [21, 12.0, 21]]], dtype=float)
    higher = np.array([[[0, 13.0, 0], [6, 13.0, 0], [6, 13.0, 6]]], dtype=float)
    assert not B.submerged_faces(outside, surf)[0]
    assert not B.submerged_faces(higher, surf)[0]
    # 无水面 ⇒ 全不淹没（旧包/无水图的图行为不变）
    assert not B.submerged_faces(all_tris, []).any()


if __name__ == "__main__":
    fails = 0
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            try:
                fn()
                print(f"  ok  {name}")
            except Exception as e:  # noqa: BLE001
                fails += 1
                print(f"  FAIL {name}: {e}")
    print("失败", fails)
    raise SystemExit(1 if fails else 0)
