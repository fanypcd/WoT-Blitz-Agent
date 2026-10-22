"""可破坏物 lka 选表/结构自检的回归（standalone，无 pytest 依赖）。

2026-10-22 回归背景：旧实现把分段表前缀硬编码成 `er*`（erlenberg），黄沙荒漠的
`dt1/dt2/dt3`、Dead Rail 的 `md1..` 等全部漏掉，回退到地图改版前的主表；主表把
同一 `(cell,slot)` 联到别的实体（desert_train 实测树倒事件联到 `bld_desert_house_*`）。
本文件锁三件事：①分段表发现是**任意前缀+序号**；②按 map_id 序数配对到变体；
③分段表必须通过「key 在场景 + 格子码等于实体位置」的结构自检。

用法：python tools/test_export_map_destructibles.py
"""
import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import export_map_destructibles as E  # noqa: E402
import export_map_glb as X  # noqa: E402


def _entry(map_id: int, key: str, space: str) -> X.MapEntry:
    return X.MapEntry(map_id, key, f"{space}/{space}.sc2", None, key)


def test_segment_discovery_any_prefix():
    with tempfile.TemporaryDirectory() as td:
        d = pathlib.Path(td)
        for name in ("02_desert_train_dt.dt1.lka.dvpl", "02_desert_train_dt.dt2.lka.dvpl",
                     "02_desert_train_dt.dt3.lka.dvpl", "02_desert_train_dt.lka.dvpl",
                     "02_desert_train_dt.mkm.dvpl"):
            (d / name).write_bytes(b"")
        segs = E._segment_tables(d, "02_desert_train_dt")
        assert [n for n, _, _ in segs] == [1, 2, 3]
        assert [pre for _, pre, _ in segs] == ["dt", "dt", "dt"]
        # 主表 / 非 lka 文件都不算分段表
        assert len(segs) == 3


def test_segment_discovery_rejects_drift():
    with tempfile.TemporaryDirectory() as td:
        d = pathlib.Path(td)
        (d / "m.dt1.lka.dvpl").write_bytes(b"")
        (d / "m.md2.lka.dvpl").write_bytes(b"")
        try:
            E._segment_tables(d, "m")
        except AssertionError:
            pass
        else:
            raise AssertionError("混合前缀应 fail-closed")
    with tempfile.TemporaryDirectory() as td:
        d = pathlib.Path(td)
        (d / "m.dt1.lka.dvpl").write_bytes(b"")
        (d / "m.dt1.lka.dvpl").with_name("m.dt01.lka.dvpl").write_bytes(b"")
        try:
            E._segment_tables(d, "m")
        except AssertionError:
            pass
        else:
            raise AssertionError("重复序号应 fail-closed")


def test_variant_pairing_by_map_id_ordinal():
    entries = [
        _entry(2, "desert_train", "02_desert_train_dt"),
        _entry(44, "desert_train_02", "02_desert_train_dt"),
        _entry(45, "desert_train_03", "02_desert_train_dt"),
        _entry(7, "medvedkovo", "04_medvedkovo_md"),
    ]
    segs = [(1, "dt", pathlib.Path("dt1")),
            (2, "dt", pathlib.Path("dt2")),
            (3, "dt", pathlib.Path("dt3"))]
    assert E.select_variant_table(entries[1], entries, segs)[0] == 2
    assert E.select_variant_table(entries[2], entries, segs)[0] == 3
    try:
        E.select_variant_table(entries[3], entries, segs)
    except AssertionError:
        pass
    else:
        raise AssertionError("同 space 数量不一致应 fail-closed")


def test_structural_check_accepts_correct_and_rejects_legacy_shape():
    scene_pos = {173: (234.3, 100.0), 434: (-202.1, -151.2)}
    ok = {
        "173": {"cell": [2, 1], "slot": 12},
        "434": {"cell": [-3, -2], "slot": 2},
    }
    E.validate_segmented_lka(ok, scene_pos, "ok")
    # 悬空 key = 遗留主表特征
    try:
        E.validate_segmented_lka({"999": {"cell": [0, 0], "slot": 1}}, scene_pos, "dangling")
    except AssertionError:
        pass
    else:
        raise AssertionError("悬空 key 应 fail-closed")
    # 格子码与实体位置不符 = 编号体系不对
    try:
        E.validate_segmented_lka({"173": {"cell": [3, 1], "slot": 12}}, scene_pos, "cell")
    except AssertionError:
        pass
    else:
        raise AssertionError("格子码不符应 fail-closed")


def main() -> None:
    tests = [
        test_segment_discovery_any_prefix,
        test_segment_discovery_rejects_drift,
        test_variant_pairing_by_map_id_ordinal,
        test_structural_check_accepts_correct_and_rejects_legacy_shape,
    ]
    for t in tests:
        t()
        print(f"PASS {t.__name__}")


if __name__ == "__main__":
    main()
