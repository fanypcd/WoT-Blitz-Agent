"""变体目录物化工具的回归（standalone，无 pytest 依赖）。

2026-10-22：多变体 space 的 lka 每变体一张，映射确有差异的变体必须有独立
`map/<key>/`；共享资产硬链接、index 的 map_id 指向本 key。相同映射不得多建目录。
本文件锁这两条（含"文档级身份字段不同但 instances 相同 ⇒ 共用基础目录"）。

用法：python tools/test_materialize_destructible_variants.py
"""
import json
import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from materialize_destructible_variants import materialize  # noqa: E402


def _doc(key, map_id, instances):
    return {"mapId": map_id, "key": key, "display": key, "instances": instances}


def _write(path: pathlib.Path, doc):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc), encoding="utf-8")


def test_only_differing_mapping_gets_variant_dir():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        pack = root / "pack"
        cache = root / "cache"
        (pack / "map" / "base").mkdir(parents=True)
        (pack / "map" / "base" / "scenery.glb").write_bytes(b"GLB")
        (pack / "map" / "base" / "ground.webp").write_bytes(b"WEBP")
        base_inst = [{"id": 1, "name": "palm", "kind": "tree",
                      "serverId": {"cell": [0, 0], "slot": 1}},
                     {"id": 2, "name": "house", "kind": "switcher",
                      "serverId": {"cell": [0, 0], "slot": 2}}]
        same_inst = list(base_inst)
        diff_inst = [dict(base_inst[0], serverId={"cell": [0, 0], "slot": 3}),
                     dict(base_inst[1])]
        _write(cache / "base" / "destructibles.json", _doc("base", 1, base_inst))
        _write(cache / "same" / "destructibles.json", _doc("same", 2, same_inst))
        _write(cache / "diff" / "destructibles.json", _doc("diff", 3, diff_inst))
        idx = {"version": 1, "maps": {
            "1": {"key": "base", "display": "Base"},
            "2": {"key": "base", "display": "Same"},
            "3": {"key": "base", "display": "Diff"},
        }}
        (pack / "index.json").write_text(json.dumps(idx), encoding="utf-8")
        mi = [
            {"map_id": 1, "key": "base", "space": "space"},
            {"map_id": 2, "key": "same", "space": "space"},
            {"map_id": 3, "key": "diff", "space": "space"},
        ]
        mi_path = root / "map_index.json"
        mi_path.write_text(json.dumps(mi), encoding="utf-8")
        rc = materialize(pack, mi_path, cache, dry_run=False)
        assert rc == 0
        assert not (pack / "map" / "same").exists()
        assert (pack / "map" / "diff" / "destructibles.json").is_file()
        # 共享资产字节一致（硬链接成功则同 inode，失败回退复制也能通过）
        assert (pack / "map" / "diff" / "scenery.glb").read_bytes() == b"GLB"
        out = json.loads((pack / "index.json").read_text(encoding="utf-8"))
        assert out["maps"]["2"]["key"] == "base"
        assert out["maps"]["3"]["key"] == "diff"


def test_refuses_existing_variant_dir():
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        pack = root / "pack"
        cache = root / "cache"
        (pack / "map" / "base").mkdir(parents=True)
        (pack / "map" / "base" / "scenery.glb").write_bytes(b"GLB")
        (pack / "map" / "var").mkdir(parents=True)   # 已存在 = 状态不干净
        _write(cache / "base" / "destructibles.json",
               _doc("base", 1, [{"id": 1, "serverId": {"cell": [0, 0], "slot": 1}}]))
        _write(cache / "var" / "destructibles.json",
               _doc("var", 2, [{"id": 1, "serverId": {"cell": [0, 0], "slot": 2}}]))
        (pack / "index.json").write_text(json.dumps({"version": 1, "maps": {
            "1": {"key": "base"}, "2": {"key": "base"}}}), encoding="utf-8")
        mi_path = root / "map_index.json"
        mi_path.write_text(json.dumps([
            {"map_id": 1, "key": "base", "space": "space"},
            {"map_id": 2, "key": "var", "space": "space"},
        ]), encoding="utf-8")
        assert materialize(pack, mi_path, cache, dry_run=False) == 2


def main() -> None:
    for t in (test_only_differing_mapping_gets_variant_dir, test_refuses_existing_variant_dir):
        t()
        print(f"PASS {t.__name__}")


if __name__ == "__main__":
    main()
