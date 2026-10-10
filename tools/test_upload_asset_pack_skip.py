"""资产包上传器的跳过判定（standalone，无 pytest 依赖）。

2026-10-10 回归背景：跳过判定曾只比远端 `Content-Length`——"内容变了但字节数恰好相同"的
对象被静默跳过（2026-10-07 的 `map/lagoon/ground.webp`；2026-10-10 的坦克顶点烘焙：只改
顶点浮点、不改字节数）。修正后以**桶内 manifest.json 的逐文件 sha256** 为准，远端条目缺失
（首传/旧对象）才回退尺寸比对。本文件锁这条优先级链。

用法：python tools/test_upload_asset_pack_skip.py
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

from upload_asset_pack_cos import patch_manifest_entries, skip_decision  # noqa: E402

SHA_A = "a" * 64
SHA_B = "b" * 64


def test_hash_equal_skips():
    assert skip_decision("glb/1/model.glb", SHA_A, 100, "100", SHA_A) == "skip"
    # 哈希相等时即便远端长度字段异常也以哈希为准（长度只是回退判据）
    assert skip_decision("glb/1/model.glb", SHA_A, 100, "999", SHA_A) == "skip"


def test_same_size_changed_content_uploads():
    # 本仓的关键回归：同尺寸不同内容必须上传（顶点烘焙类修正）
    assert skip_decision("glb/28689/model.glb", SHA_B, 5211200, "5211200", SHA_A) == "upload-same-size"


def test_different_size_changed_content_uploads():
    assert skip_decision("glb/22385/model.glb", SHA_B, 5169232, "5169300", SHA_A) == "upload"


def test_missing_remote_uploads():
    assert skip_decision("glb/1/model.glb", SHA_A, 100, None, None) == "upload"
    assert skip_decision("glb/1/model.glb", SHA_A, 100, None, SHA_A) == "upload"


def test_legacy_fallback_without_remote_hash():
    # 远端 manifest 缺条目（旧对象）→ 回退 Content-Length
    assert skip_decision("map/x/ground.webp", SHA_A, 7080664, "7080664", None) == "skip-size"
    assert skip_decision("map/x/ground.webp", SHA_A, 7080664, "7080000", None) == "upload"


# ---------------------------------------------------------------------------
# --only 局部发布的 manifest 就地补丁（只更新被上传条目、其余照远端原样）
# ---------------------------------------------------------------------------
REMOTE = {
    "version": 1, "generated": "2026-10-09T16:08:27+00:00",
    "upstream_commit": "1b4841bc", "worktree_dirty": True,
    "files": [
        {"path": "glb/28689/model.glb", "bytes": 5211208, "sha256": SHA_A},
        {"path": "map/himmelsdorf/ground.webp", "bytes": 7080664, "sha256": "c" * 64},
        {"path": "glb/1/model.glb", "bytes": 1695724, "sha256": "d" * 64},
    ],
}


def test_patch_manifest_replaces_only_given_entries():
    man = patch_manifest_entries(REMOTE, {"glb/28689/model.glb": (5211200, SHA_B)}, note="partial")
    got = {f["path"]: f for f in man["files"]}
    assert got["glb/28689/model.glb"] == {"path": "glb/28689/model.glb", "bytes": 5211200, "sha256": SHA_B}
    # 未列出的条目逐字节不动（含合成前 ground 等历史状态）
    assert got["map/himmelsdorf/ground.webp"] == REMOTE["files"][1]
    assert got["glb/1/model.glb"] == REMOTE["files"][2]
    assert len(man["files"]) == 3
    assert man["note"] == "partial" and man["generated"] != REMOTE["generated"]
    # 入参不被修改（深拷贝）
    assert REMOTE["files"][0]["sha256"] == SHA_A and "note" not in REMOTE


def test_patch_manifest_registers_new_entries():
    man = patch_manifest_entries(REMOTE, {"glb/999/model.glb": (123, SHA_B)}, note="partial")
    got = {f["path"] for f in man["files"]}
    assert got == {"glb/28689/model.glb", "glb/1/model.glb", "map/himmelsdorf/ground.webp",
                   "glb/999/model.glb"}


def test_patch_manifest_rejects_broken_remote():
    try:
        patch_manifest_entries({"version": 1}, {}, note="x")
    except ValueError:
        return
    raise AssertionError("缺 files[] 应抛 ValueError")


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
