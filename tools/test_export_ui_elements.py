#!/usr/bin/env python3
"""`export_ui_elements.py` 的独立回归测试（无 pytest 依赖，不需要客户端）。

覆盖：
1. webp 头解析（VP8X / VP8 / VP8L）与 RIFF 长度 fail-closed；
2. DVPL 剥壳 → 同字节落盘 + `@2x` 原名保留 + 子目录相对路径；
3. 坏 payload（非 webp）记 failed、不产文件、整体非零退出；
4. 幂等：第二次运行全部 unchanged，字节不变；
5. 资源组白名单显式且目录不重叠。
"""

from __future__ import annotations

import json
import pathlib
import struct
import sys
import tempfile
import zlib
from contextlib import contextmanager

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

import export_ui_elements as U  # noqa: E402

SUFFIX = U.SUFFIX


def _dvpl(payload: bytes) -> bytes:
    footer = struct.pack("<III4s4s", len(payload), len(payload),
                         zlib.crc32(payload) & 0xFFFFFFFF, b"\x00\x00\x00\x00", b"DVPL")
    return payload + footer


def _vp8x(width: int, height: int) -> bytes:
    chunk = (b"VP8X" + struct.pack("<I", 10) + b"\x10\x00\x00\x00"
             + (width - 1).to_bytes(3, "little") + (height - 1).to_bytes(3, "little"))
    body = b"WEBP" + chunk
    return b"RIFF" + struct.pack("<I", len(body)) + body


def _vp8(width: int, height: int) -> bytes:
    chunk = (b"VP8 " + struct.pack("<I", 10) + b"\x00\x00\x00"
             + b"\x9d\x01\x2a" + struct.pack("<HH", width, height))
    body = b"WEBP" + chunk
    return b"RIFF" + struct.pack("<I", len(body)) + body


def _vp8l(width: int, height: int) -> bytes:
    bits = (width - 1) | ((height - 1) << 14)
    chunk = b"VP8L" + struct.pack("<I", 12) + b"\x2f" + struct.pack("<I", bits) + b"\x00" * 7
    body = b"WEBP" + chunk
    return b"RIFF" + struct.pack("<I", len(body)) + body


def test_dimensions() -> None:
    assert U.webp_dimensions(_vp8x(256, 128)) == (256, 128)
    assert U.webp_dimensions(_vp8(320, 240)) == (320, 240)
    assert U.webp_dimensions(_vp8l(63, 31)) == (63, 31)


def test_bad_payload_fails_closed() -> None:
    for raw, needle in [
        (b"not an image", "RIFF"),
        (_vp8x(1, 1)[:-1], "RIFF"),          # 截断后 <30 字节，走头部形状门禁
        (_vp8x(1, 1) + b"\x00", "RIFF size"),  # 头声明与实际长度不一致
    ]:
        try:
            U.webp_dimensions(raw)
        except U.UiAssetError as exc:
            assert needle in str(exc), (needle, exc)
        else:
            raise AssertionError(f"must reject: {raw[:12]!r}")


def test_group_dirs_explicit() -> None:
    dirs = list(U.GROUP_DIRS.values())
    assert len(dirs) == len(set(dirs)), "资源组目录不能重叠"
    for name, rel in U.GROUP_DIRS.items():
        assert rel.startswith("Gfx/") and not rel.endswith("/"), (name, rel)
    assert U.GROUP_DIRS["flags"] == "Gfx/Lobby/flags"


def _make_client(root: pathlib.Path) -> None:
    flags = root / U.GROUP_DIRS["flags"]
    (flags / "upgrade").mkdir(parents=True)
    (flags / ("flag_text_usa" + SUFFIX)).write_bytes(_dvpl(_vp8x(32, 16)))
    (flags / ("flag_text_usa@2x" + SUFFIX)).write_bytes(_dvpl(_vp8x(64, 32)))
    (flags / "upgrade" / ("flag_tank-node_ussr" + SUFFIX)).write_bytes(_dvpl(_vp8x(256, 128)))
    (flags / ("broken" + SUFFIX)).write_bytes(_dvpl(b"\x89PNG\r\n\x1a\n" + b"\x00" * 32))
    # 图集描述文件不在 glob 范围
    (flags / "atlas.txt.dvpl").write_bytes(_dvpl(b"whatever"))


@contextmanager
def _no_dlc():
    """测试隔离：不扫描本机真实 DLC 覆盖层。"""
    old_packs, old_client = U.packs_dir, U.client_path
    U.packs_dir = lambda: None
    U.client_path = lambda root, rel: root / rel
    try:
        yield
    finally:
        U.packs_dir, U.client_path = old_packs, old_client


def test_dlc_overlay_and_additions() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp) / "Data"
        packs = pathlib.Path(tmp) / "packs"
        out = pathlib.Path(tmp) / "out"
        rel = f"{U.GROUP_DIRS['flags']}/flag_text_usa{SUFFIX}"
        added = f"{U.GROUP_DIRS['flags']}/flag_text_gold{SUFFIX}"
        (root / rel).parent.mkdir(parents=True)
        (root / rel).write_bytes(_dvpl(_vp8x(32, 16)))
        (packs / rel).parent.mkdir(parents=True)
        (packs / rel).write_bytes(_dvpl(_vp8x(64, 32)))       # 同名覆盖
        (packs / added).write_bytes(_dvpl(_vp8x(8, 8)))       # 覆盖层新增

        old_packs, old_client = U.packs_dir, U.client_path
        U.packs_dir = lambda: packs
        U.client_path = lambda data_root, rel: (packs / rel) if (packs / rel).exists() else (data_root / rel)
        try:
            code = U.run(["flags"], root, out, list_only=False)
        finally:
            U.packs_dir, U.client_path = old_packs, old_client

        assert code == 0, code
        assert (out / "flags" / "flag_text_usa.webp").read_bytes() == _vp8x(64, 32), "DLC 覆盖必须优先"
        assert (out / "flags" / "flag_text_gold.webp").read_bytes() == _vp8x(8, 8), "DLC 新增必须入集"
        status = json.loads((out / "_export_status.json").read_text(encoding="utf-8"))
        entries = {e["out"]: e for e in status["groups"]["flags"]["entries"]}
        assert entries["flags/flag_text_usa.webp"].get("overlay") is True
        assert entries["flags/flag_text_gold.webp"]["width"] == 8


def test_export_and_idempotency() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp) / "Data"
        out = pathlib.Path(tmp) / "out"
        _make_client(root)

        with _no_dlc():
            code = U.run(["flags"], root, out, list_only=False)
        assert code == 1, "坏 payload 必须整体非零退出"
        status = json.loads((out / "_export_status.json").read_text(encoding="utf-8"))
        flags = status["groups"]["flags"]
        assert flags["files"] == 4, flags
        assert flags["done"] == 3 and flags["failed"] == 1, flags
        assert status["finished"] is True and status["failed"] == 1
        assert "broken" in flags["errors"][0] and "RIFF" in flags["errors"][0]
        assert not (out / "flags" / "atlas.webp").exists()
        assert not (out / "flags" / "broken.webp").exists()

        usa = (out / "flags" / "flag_text_usa.webp").read_bytes()
        assert usa == _vp8x(32, 16), "必须与 DVPL 解包字节完全一致"
        assert (out / "flags" / "flag_text_usa@2x.webp").is_file()
        nested = out / "flags" / "upgrade" / "flag_tank-node_ussr.webp"
        assert nested.is_file(), "子目录相对路径必须保留"
        assert nested.read_bytes() == _vp8x(256, 128)

        entry = next(e for e in flags["entries"] if e["src"].endswith("flag_text_usa" + SUFFIX))
        assert entry["width"] == 32 and entry["height"] == 16
        assert entry["sha256"] == __import__("hashlib").sha256(usa).hexdigest()

        before = {p: p.read_bytes() for p in out.rglob("*.webp")}
        with _no_dlc():
            code = U.run(["flags"], root, out, list_only=False)
        assert code == 1
        status = json.loads((out / "_export_status.json").read_text(encoding="utf-8"))
        flags = status["groups"]["flags"]
        assert flags["done"] == 0 and flags["unchanged"] == 3, flags
        assert {p: p.read_bytes() for p in out.rglob("*.webp")} == before


def test_list_only_writes_nothing() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp) / "Data"
        out = pathlib.Path(tmp) / "out"
        _make_client(root)
        with _no_dlc():
            code = U.run(["flags"], root, out, list_only=True)
        assert code == 1
        assert not out.exists(), "list-only 不得创建输出目录"


def main() -> int:
    for name, fn in sorted(globals().items()):
        if name.startswith("test_") and callable(fn):
            fn()
            print(f"[ok] {name}")
    print("ui element export tests: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())