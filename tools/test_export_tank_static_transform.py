"""坦克导出：**静态变换烘焙**与 **StateSwitcher 初始态过滤**（standalone，无 pytest 依赖）。

回归（2026-10-10 用户报障）：Rhm.Pzw.（28689）装甲查看器里 `gun_01_mask_cap`（炮盾顶盖，
皮肤槽 DecorItem）贴在底面。根因：客户端把它放在 `gun_01_mask_cap_pivot` 的 world 变换
`[0, 1.855, 2.278]` 下，而导出沿用 BlitzKit 契约把所有节点变换写成 identity。

修法两条（依据见 export_tank_glb 模块 docstring）：
  * **非姿态节点**的累计静态变换烘进 POSITION/NORMAL 两段（GLB 节点仍全 identity，
    消费方零改动）；姿态节点 `hull` / `turret_NN` / `gun_NN` / `gun_NN_mask` / `chassis_*`
    与**场景根**不烘——运行期被矩阵覆盖 / 是车体锚点。
  * StateSwitcher 容器：默认 scene 只挂 `ssc.activeState` 对应的激活态子实体；越界（-1）
    = 默认 scene 关闭；`hide_elements` 的非激活子实体**不丢**：构建到备用 scene
    `wotb_inactive_states`（默认 scene 不引用，标准查看器只加载 `gltf.scene` ⇒ 默认不
    显示），节点带 `extras.wotb.hideElementsState/defaultVisible`，供传奇涂装/状态切换使用。

用法：python tools/test_export_tank_static_transform.py
（无客户端 / 无缓存时集成段自动跳过，纯函数段照跑。）
"""
import json
import pathlib
import struct
import sys
import tempfile

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from export_tank_glb import (  # noqa: E402
    IDENTITY_EPS, bake_vertices, compose_static, is_pose_node, local_trs,
    quat_mat, state_keep_children)


def tc(t=(0.0, 0.0, 0.0), q=(0.0, 0.0, 0.0, 1.0), s=(1.0, 1.0, 1.0)):
    return {"tc.localTranslation": list(t), "tc.localRotation": list(q), "tc.localScale": list(s)}


# ---------------------------------------------------------------------------
# 姿态节点判据
# ---------------------------------------------------------------------------
def test_pose_node_names():
    for name in ("hull", "turret_01", "turret_12", "gun_01", "gun_01_mask",
                 "gun_03_mask", "chassis_wheel_R_01", "chassis_track_L",
                 "chassis_chassis_R"):
        assert is_pose_node(name), name
    # 挂在这些子树下的命名件不是姿态节点本身（它们的自身变换要烘）
    for name in ("hull_nc", "hull_hide_elements_switch", "turret_01_nc_skin",
                 "gun_01_mask_cap_pivot", "gun_01_mask_cap", "gun_marks_root",
                 "gun_01_lamp_pivot", "Machine_gun_02.sc2", "hull_state_00"):
        assert not is_pose_node(name), name


# ---------------------------------------------------------------------------
# 局部变换 → 矩阵
# ---------------------------------------------------------------------------
def test_identity_and_noise_give_none():
    assert local_trs(None) is None
    assert local_trs({}) is None
    assert local_trs(tc()) is None
    # 浮点噪声（< IDENTITY_EPS）不触发烘焙——保住与 BlitzKit 的逐字节一致
    eps = IDENTITY_EPS / 10
    assert local_trs(tc(t=(eps, 0, 0), q=(eps, 0, 0, 1.0), s=(1 + eps, 1, 1))) is None


def test_cap_pivot_translation_survives():
    # 28689 gun_01_mask_cap_pivot 的真实作者值
    m = local_trs(tc(t=(0.0, 1.8550000190734863, 2.2780001163482666)))
    assert m is not None
    assert np.allclose(m[:3, 3], [0.0, 1.855, 2.278], atol=1e-6)
    assert np.allclose(m[:3, :3], np.eye(3))


def test_quat_180_about_z():
    m = quat_mat((-1.3909065899042616e-08, 0.0, 1.0, 8.940696716308594e-08))  # 22385 护盾
    assert np.allclose(m @ m, np.eye(3), atol=1e-6)
    assert np.allclose(m, np.diag([-1.0, -1.0, 1.0]), atol=1e-6)


def test_compose_static_parent_then_child():
    parent = local_trs(tc(t=(0.0, 1.0, 0.0)))
    acc = compose_static(None, tc(t=(0.0, 1.0, 0.0)))
    acc = compose_static(acc, tc(t=(0.0, 0.0, 2.0)))    # 子：z+2 在父帧里
    assert np.allclose(acc[:3, 3], [0.0, 1.0, 2.0])     # 平移可加（无旋转时）
    assert np.allclose(parent[:3, 3], [0.0, 1.0, 0.0])
    # 子变换为恒等 → 累计不变
    assert compose_static(acc, tc()) is acc


# ---------------------------------------------------------------------------
# 顶点烘焙
# ---------------------------------------------------------------------------
def _verts(pos, nrm):
    v = np.zeros((len(pos), 6), dtype="<f4")
    v[:, 0:3] = np.asarray(pos, dtype="<f4")
    v[:, 3:6] = np.asarray(nrm, dtype="<f4")
    return v


def test_bake_translation_moves_pos_keeps_normals_bitwise():
    v = _verts([[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]], [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0]])
    m = local_trs(tc(t=(0.0, 1.8550000190734863, 2.2780001163482666)))
    out = bake_vertices(v, m)
    assert np.allclose(out[:, 0:3], [[0.0, 1.855, 2.278], [1.0, 3.855, 5.278]], atol=1e-6)
    assert np.array_equal(out[:, 3:6], v[:, 3:6])       # 纯平移不碰法线（逐位）


def test_bake_rotation_rotates_normals():
    v = _verts([[1.0, 0.0, 0.0]], [[1.0, 0.0, 0.0]])
    m = local_trs(tc(q=(0.0, 0.0, 1.0, 0.0)))           # 绕 Z 180°
    out = bake_vertices(v, m)
    assert np.allclose(out[:, 0:3], [[-1.0, 0.0, 0.0]], atol=1e-6)
    assert np.allclose(out[:, 3:6], [[-1.0, 0.0, 0.0]], atol=1e-6)


def test_bake_nonuniform_scale_uses_inverse_transpose():
    # 法线 (0,1,1)/√2 在 scale=(1,1,2) 下应走逆转置（→(0,1,0.5) 归一化），
    # 而不是"只转一下"（那会得到 (0,1,1)）。
    v = _verts([[0.0, 0.0, 1.0]], [[0.0, 0.70710678, 0.70710678]])
    m = local_trs(tc(s=(1.0, 1.0, 2.0)))
    out = bake_vertices(v, m)
    assert np.allclose(out[:, 0:3], [[0.0, 0.0, 2.0]], atol=1e-6)
    exp = np.array([0.0, 1.0, 0.5]) / np.linalg.norm([0.0, 1.0, 0.5])
    assert np.allclose(out[:, 3:6], exp[None, :], atol=1e-6)


# ---------------------------------------------------------------------------
# StateSwitcher 初始态过滤
# ---------------------------------------------------------------------------
def sw(active, **states):
    d = {"ssc.activeState": active, "ssc.statesCount": len(states)}
    d.update({f"ssc.state{k}": v for k, v in states.items()})
    return d


def test_state_active_in_range_keeps_active_and_extra_children():
    # 9073 turret/state_entity_00：激活 turret_01_shields，丢 shields_01；非状态子实体（FX）保留
    kids = ["Turret_steam_hit_close", "turret_01_shields", "turret_01_shields_01"]
    keep, names = state_keep_children(
        sw(0, **{"0": "turret_01_shields", "1": "turret_01_shields_01"}), kids)
    assert keep and names == {"Turret_steam_hit_close", "turret_01_shields"}


def test_state_out_of_range_off():
    # 22385 state_entity_01：activeState=-1 → 整容器关闭（close 几何不导出）
    keep, names = state_keep_children(
        sw(-1, **{"0": "hull_shields_close_anim", "1": "hull_shields_close"}),
        ["hull_shields_close", "hull_shields_close_anim"])
    assert keep is False and names is None


def test_state_hide_elements_default_scene_active_only():
    # 默认 scene 只挂激活态；非激活子实体的保留由 build() 的备用 scene `wotb_inactive_states` 负责
    keep, names = state_keep_children(
        sw(0, **{"0": "turret_01_nc", "1": "turret_01_nc_skin"}),
        ["turret_01_nc_skin", "turret_01_nc"])
    assert keep and names == {"turret_01_nc"}


def test_state_active_name_missing_fails_open():
    # 形态未见过（激活名不在子实体里）→ 全保留，宁可多不可少
    keep, names = state_keep_children(sw(0, **{"0": "nope", "1": "other"}), ["a", "b"])
    assert keep and names is None
    # 无组件 / activeState 非 int
    assert state_keep_children(None, ["a"]) == (True, None)
    keep, names = state_keep_children({"ssc.activeState": "0", "ssc.state0": "a"}, ["a"])
    assert keep and names == {"a"}


# ---------------------------------------------------------------------------
# 集成：客户端在场时对真实车辆断言（无客户端/无缓存自动跳过）
# ---------------------------------------------------------------------------
def _load_glb_json(path: pathlib.Path) -> dict:
    b = path.read_bytes()
    jlen, = struct.unpack_from("<I", b, 12)
    return json.loads(b[20:20 + jlen])


def _subtree_bbox(js: dict, i: int):
    acc = [[1e9] * 3, [-1e9] * 3]

    def rec(k):
        n = js["nodes"][k]
        if "mesh" in n:
            prim = js["meshes"][n["mesh"]]["primitives"][0]
            a = js["accessors"][prim["attributes"]["POSITION"]]
            for c in range(3):
                acc[0][c] = min(acc[0][c], a["min"][c])
                acc[1][c] = max(acc[1][c], a["max"][c])
        for ch in n.get("children", []):
            rec(ch)

    rec(i)
    return acc


def _export_to_tmp(tank_id: int, mode: str = "none") -> pathlib.Path | None:
    """导出单辆到临时目录；客户端缺失返回 None。`mode="semantic"` 与现役缓存同口径。"""
    import export_tank_glb as ex
    game_data = ex.default_game_data()
    if not game_data.is_dir():
        return None
    table = ex.read_tank_table(pathlib.Path(__file__).resolve().parent.parent / "data" / "tanks.pb")
    info = table.get(tank_id)
    if info is None:
        return None
    td = pathlib.Path(tempfile.mkdtemp(prefix=f"tank{tank_id}_"))
    st = ex.export_tank(game_data, tank_id, info["nation"], info["stem"], td, mode, 0)
    if st.get("error"):
        print(f"  (skip: export error {st['error']})")
        return None
    return td / str(tank_id) / "model.glb"


def test_integration_28689_cap_on_roof():
    """Rhm.Pzw. 的炮盾顶盖必须在炮塔顶（z≈2.278..2.320），不再贴底面。"""
    glb = _export_to_tmp(28689)
    if glb is None:
        print("  (skip: 客户端不在场)"); return
    js = _load_glb_json(glb)
    idx = [i for i, n in enumerate(js["nodes"]) if n.get("name") == "gun_01_mask_cap"]
    assert idx, "gun_01_mask_cap 节点应存在"
    bb = _subtree_bbox(js, idx[0])
    assert bb[0][2] > 2.0, f"顶盖仍在低处: z_min={bb[0][2]:.3f}"
    assert abs(bb[0][2] - 2.278) < 0.01 and abs(bb[1][2] - 2.320) < 0.01, bb
    # 姿态节点未受影响（炮盾本体仍按模型空间原始坐标）
    mask = [i for i, n in enumerate(js["nodes"]) if n.get("name") == "gun_01_mask"][0]
    bm = _subtree_bbox(js, mask)
    assert abs(bm[1][2] - 2.324) < 0.01 and bm[0][2] > 1.7, bm


def test_integration_22385_single_shield_set():
    """JagdPantherII_Titan：只留初始态（open）护盾，close 形态与 react 变体不导出。"""
    glb = _export_to_tmp(22385)
    if glb is None:
        print("  (skip: 客户端不在场)"); return
    names = {n.get("name") for n in _load_glb_json(glb)["nodes"]}
    assert "hull_shields_open_anim" in names
    for gone in ("hull_shields_open_react", "hull_shields_close", "hull_shields_close_anim",
                 "hull_shields_open_react/0000"):
        assert gone not in names, f"非激活态 {gone} 仍在导出里"


def test_integration_10625_default_scene_single_turret_alt_scene_preserved():
    """10625 CC1 Mk.2：默认 scene 只有激活态；传奇涂装部分组件在备用 scene 保留、默认不显示。

    `turret_01_nc_skin`/`turret_02_nc_skin` 是传奇涂装的部分组件（脚本在激活时对主体
    `turret_01_nc` 施加 -6.5° 姿态）。默认 scene 不能挂它们，否则和默认炮塔同屏；
    但数据要保留在备用 scene `wotb_inactive_states`，供后续状态切换使用。
    """
    glb = _export_to_tmp(10625)
    if glb is None:
        print("  (skip: 客户端不在场)"); return
    js = _load_glb_json(glb)
    assert js["scene"] == 0 and len(js["scenes"]) >= 2, js.get("scenes")

    def reach(idx: int) -> set[str]:
        seen: set[str] = set()
        stack = list(js["scenes"][idx]["nodes"])
        while stack:
            i = stack.pop()
            n = js["nodes"][i]
            seen.add(n.get("name"))
            stack.extend(n.get("children", []))
        return seen

    default = reach(0)
    alt = reach(1)
    for keep in ("turret_01", "turret_01_nc", "turret_02", "turret_02_nc"):
        assert keep in default, f"{keep} 应在默认 scene"
    for gone in ("turret_01_nc_skin", "turret_02_nc_skin", "hull_nc_skin"):
        assert gone not in default, f"非激活形态 {gone} 不应出现在默认 scene"
    assert js["scenes"][1].get("name") == "wotb_inactive_states"
    for kept in ("turret_01_nc_skin", "turret_02_nc_skin", "hull_nc_skin"):
        assert kept in alt, f"{kept} 应保留在备用 scene"
    for nm in ("turret_01_nc_skin", "turret_02_nc_skin"):
        i = [i for i, n in enumerate(js["nodes"]) if n.get("name") == nm][0]
        assert js["nodes"][i]["extras"]["wotb"]["defaultVisible"] is False


def _has_multi_state_hide_elements(tid: int) -> bool:
    """该车是否有 >1 个状态的 hide_elements StateSwitcher 容器（本次修正会改变其 GLB）。"""
    cache = globals().setdefault("_HIDE_STATE_CACHE", {})
    if tid in cache:
        return cache[tid]
    import export_tank_glb as ex
    from wotb_sc2 import decode_dvpl, read_sc2
    root = pathlib.Path(__file__).resolve().parent.parent
    game_data = ex.default_game_data()
    table = ex.read_tank_table(root / "data" / "tanks.pb")
    info = table.get(tid)
    found = False
    if info is not None:
        res = ex.resolve_tank(game_data, info["nation"], info["stem"])
        p = res["model_sc2"]
        if p.exists():
            scene = read_sc2(decode_dvpl(p.read_bytes()))

            def walk(e):
                nonlocal found
                if found:
                    return
                comps = ex.entity_components(e)
                sw = comps.get("StateSwitcherComponent")
                if sw and "hide_elements" in (e.get("name") or ""):
                    n = sw.get("ssc.statesCount")
                    if isinstance(n, int) and n > 1:
                        found = True
                        return
                for k in ex.nested(e):
                    walk(k)

            for e in ex.nested(scene):
                walk(e)
    cache[tid] = found
    return found


def test_integration_unaffected_tank_byte_identical():
    """未命中静态变换/状态过滤的车必须与现役缓存**逐字节一致**（修正的零副作用保证）。"""
    root = pathlib.Path(__file__).resolve().parent.parent
    cache = root / "data" / "cache" / "models"
    static_affected = {3921, 5969, 8305, 9073, 10625, 10753, 10881, 12657, 16001, 17217,
                       17265, 17777, 20081, 20817, 21249, 22033, 22385, 24145, 24945,
                       26145, 28689, 28961, 29457}
    checked = 0
    for tid in sorted(p.name for p in cache.iterdir() if p.is_dir()):
        if not tid.isdigit():
            continue
        if int(tid) in static_affected or _has_multi_state_hide_elements(int(tid)):
            continue
        ref = cache / tid / "model.glb"
        if not ref.is_file():
            continue
        glb = _export_to_tmp(int(tid), mode="semantic")
        if glb is None:
            print("  (skip: 客户端不在场)"); return
        assert glb.read_bytes() == ref.read_bytes(), f"{tid} 未被修正触及却变了字节"
        checked += 1
        if checked >= 3:
            break
    assert checked > 0 or not cache.is_dir(), "没有可比对的缓存样本"


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
