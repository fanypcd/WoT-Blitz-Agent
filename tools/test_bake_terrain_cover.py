"""地形让位掩码烘焙的纯函数契约（standalone：python tools/test_bake_terrain_cover.py）。"""

import pathlib
import sys

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import bake_terrain_cover as B  # noqa: E402


def expect_error(fn, needle):
    try:
        fn()
    except Exception as e:  # noqa: BLE001
        assert needle in str(e), f"期望错误含 {needle!r}，实际 {e!r}"
        return
    raise AssertionError(f"未抛错（期望 {needle!r}）")


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
    assert not bool(m[0, 3])      # 低 0.5 > band_low ⇒ 埋在铺装下的箱体（不得挖出来）
    assert not bool(m[0, 4])      # 无结构面


def test_envelope_one_sided_and_ramp():
    # 覆盖 texel：压低量 = min(press_max, nat − 结构面 + clearance) ⇒ 落到**结构面下方 clearance**
    nat = np.array([[20.0, 20.4, 24.0, 30.0]])
    surf = np.array([[np.nan, 20.35, np.nan, np.nan]])          # 低 0.05 ⇒ 吸附齐平（band_low 内）
    cov = B.classify_ground_lying(surf, nat, 0.15, 0.5)
    assert bool(cov[0, 1]) and not cov[0, 0]
    ceil = B.envelope(nat, surf, cov, margin=2, press_max=0.5, clearance=0.1)
    assert abs(ceil[0, 1] - 20.25) < 1e-9                        # = 结构面 − 0.1（不共面 ⇒ 不 z-fight）
    # d=1（margin=2）：该 texel 自己的压低量 = clip(24.0−20.35+0.1, 0, 0.5) = 0.5（封顶）×(1−1/2) ⇒ 24.0−0.25
    assert abs(ceil[0, 2] - 23.75) < 1e-9
    assert not np.isfinite(ceil[0, 3])                           # d ≥ margin：收尾到 0
    assert not np.isfinite(ceil[0, 0])                           # 地形本就在结构面之下 ⇒ 不动
    steep = np.array([[20.0, 23.6, 30.0]])
    surf2 = np.array([[20.3, np.nan, np.nan]])
    cov2 = B.classify_ground_lying(surf2, steep, 0.15, 0.5)
    c2 = B.envelope(steep, surf2, cov2, margin=2, press_max=0.5, clearance=0.1)
    assert steep[0, 1] - c2[0, 1] <= 0.5 * 0.5 + 1e-9            # 压低量 ≤ press_max×taper
    assert not np.isfinite(B.envelope(nat, np.full_like(nat, np.nan),
                                      np.zeros_like(nat, dtype=bool), 8, 0.5, 0.1)).any()


def test_envelope_clearance():
    """覆盖处地形必须落在结构面**下方 clearance**（共面 ⇒ 逐像素 z-fight 闪烁；2026-10-10 报障）。"""
    nat = np.full((3, 3), 30.0)
    surf = np.full((3, 3), 29.9)                                  # 结构面在地形下 0.1
    cov = B.classify_ground_lying(surf, nat, 0.15, 0.5)
    assert cov.all()
    ceil = B.envelope(nat, surf, cov, margin=1, press_max=0.5, clearance=0.1)
    gap = surf - ceil                                             # 地形在结构面之下的间隙
    assert np.all(gap >= 0.1 - 1e-9), f"最小间隙 {gap.min():.4f} < clearance"
    # 反向：结构面在地形**之上**（低矮构件）⇒ 不压（只压不抬），间隙本就为负（结构在上，正常）
    nat2 = np.array([[20.0]])
    surf2 = np.array([[20.4]])
    cov2 = B.classify_ground_lying(surf2, nat2, 0.15, 0.5)
    c2 = B.envelope(nat2, surf2, cov2, margin=1, press_max=0.5, clearance=0.1)
    assert not np.isfinite(c2[0, 0])


def test_quantize_sentinel():
    zmin, zmax = 0.0, 100.0
    assert B.quantize(float("nan"), zmin, zmax) == 0            # 无覆盖 = 0
    assert B.quantize(zmin, zmin, zmax) == 1                    # 真值 0 也 ≥1（哨兵不冲突）
    v = B.quantize(50.0, zmin, zmax)
    assert 32000 < v < 33500                                    # ≈ 50/100×65535 + 1
    assert B.quantize(zmax * 2, zmin, zmax) == 65535            # 夹取上限




def test_water_surfaces_and_submerged():
    """水面片自身与水下薄板必须被排除出"贴地结构"（2026-10-10 用户"岸线拉紧看还是锯齿"定案：
    掩码把地形压到冰面之下会把可见岸线整体挪走并留台阶；生产环境无掩码、岸线是平滑数据等高线）。"""
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
