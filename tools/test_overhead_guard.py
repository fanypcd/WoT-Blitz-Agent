"""俯视合成链的世代护栏（standalone，无 pytest 依赖）。

2026-10-15 审计背景：合成器只按 `<key>.rgba/.meta.json` 是否存在读取，分不出渲染件世代；
"地面 pass 前的渲染件 + 文档流程"会**静默**把 36 张线上底图回退一代（实测：产物与线上逐张不同），
且 `--verify` 用它自洽重建也测不出来。加固 = ①`--write` 默认拒写 pre-ground-pass 世代
（`render_generation`/`write_refused`）；②归档助手 `archive_overhead_bake.py` 成对校验 +
世代体检。本文件锁这两处的判据（**只用语义标记，不做统计判定**）。

用法：python tools/test_overhead_guard.py
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from archive_overhead_bake import pair_up  # noqa: E402
from composite_overhead import render_generation, write_refused  # noqa: E402


def test_generation_semantic_marker():
    # 判据是 `ground` 键的**存在性与取值**，与其它字段无关
    assert render_generation({"ground": True, "coverPct": 100}) == "ground-pass"
    assert render_generation({"ground": False, "coverPct": 22.7}) == "no-terrain"
    # 缺键 = 地面 pass 前世代（本机 2026-10-14 20:52–21:33 那批归档件即此形态）
    assert render_generation({"coverPct": 22.73, "covered": 3814090}) == "pre-ground-pass"
    # 键存在但为假值以外的真值一律按地面 pass（bake 只写 true/false，不做宽松猜测）
    assert render_generation({"ground": True}) == "ground-pass"


def test_write_gate_refuses_legacy_only():
    legacy = "pre-ground-pass"
    # 拒写只发生在"要写 + 旧世代 + 未显式放行"三者同时成立时
    msg = write_refused(legacy, write=True, allow_legacy=False)
    assert msg and "--allow-legacy-render" in msg and "重烘" in msg
    assert write_refused(legacy, write=True, allow_legacy=True) is None
    assert write_refused(legacy, write=False, allow_legacy=False) is None  # 只报告（dry-run）不拦
    assert write_refused("ground-pass", write=True, allow_legacy=False) is None
    assert write_refused("no-terrain", write=True, allow_legacy=False) is None  # 合法无地形图


def test_pair_up_fail_closed():
    keys, errors, ignored = pair_up(["a.meta.json", "a.rgba", "b.meta.json", "upload.rgba", "c.rgba"])
    assert keys == ["a"]
    assert errors == ["b: 缺 b.rgba", "c: 缺 c.meta.json"]  # 缺任一半都要报（不部分归档）
    assert ignored == ["upload.rgba"]  # 上传中间件落盘点不参与配对
    keys, errors, ignored = pair_up(["k1.rgba", "k1.meta.json", "k2.rgba", "k2.meta.json"])
    assert keys == ["k1", "k2"] and errors == [] and ignored == []


if __name__ == "__main__":
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"[ok] {name}")
    print("全部通过")
