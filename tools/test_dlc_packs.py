"""DLC 覆盖层（packs）路径解析单测：python tools/test_dlc_packs.py（standalone，无 pytest 依赖）。

锁 `dlc_packs.client_path` 的核心契约：**同相对路径下 `packs/` 优先于 `Data/`**，
缺失才回退。本机装有客户端时顺带对真实 packs 目录做一次抽查（无 packs 目录则跳过该段，
CI/纯净检出不会失败）。

背景：客户端把 DLC 微更新写到 `%LOCALAPPDATA%\\wotblitz\\packs` 并覆盖 `Data/` 同名路径
（本机实测 45 个 packs 文件里 5 个覆盖 Data、40 个 Data 里根本没有）。坦克 GLB 导出读的
`3d/Tanks/.../<model>.sc2/.scg` 正在被覆盖之列——不经本解析器就会读到 DLC 前的旧版。
"""
import pathlib
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

import dlc_packs  # noqa: E402


def _with_fake_packs(fn):
    """用临时目录冒充 packs 根，跑完恢复缓存。"""
    with tempfile.TemporaryDirectory() as td:
        fake = pathlib.Path(td)
        (fake / "3d" / "Tanks").mkdir(parents=True)
        (fake / "3d" / "Tanks" / "OnlyInPacks.sc2.dvpl").write_bytes(b"PACKS")
        (fake / "3d" / "Tanks" / "Both.sc2.dvpl").write_bytes(b"PACKS")
        data = fake / "Data"
        (data / "3d" / "Tanks").mkdir(parents=True)
        (data / "3d" / "Tanks" / "Both.sc2.dvpl").write_bytes(b"DATA")
        (data / "3d" / "Tanks" / "OnlyInData.sc2.dvpl").write_bytes(b"DATA")
        saved = dlc_packs._PACKS
        dlc_packs._PACKS = fake
        try:
            fn(fake, data)
        finally:
            dlc_packs._PACKS = saved


def test_same_path_prefers_packs():
    def run(fake, data):
        p = dlc_packs.client_path(data, "3d/Tanks/Both.sc2.dvpl")
        assert p.read_bytes() == b"PACKS", f"同名路径应取 packs，实际 {p}"

    _with_fake_packs(run)


def test_falls_back_to_data_when_absent_in_packs():
    def run(fake, data):
        p = dlc_packs.client_path(data, "3d/Tanks/OnlyInData.sc2.dvpl")
        assert p.read_bytes() == b"DATA", f"packs 缺失应回退 Data，实际 {p}"
        assert p.parent.parent.parent == data

    _with_fake_packs(run)


def test_finds_packs_only_file():
    def run(fake, data):
        p = dlc_packs.client_path(data, "3d/Tanks/OnlyInPacks.sc2.dvpl")
        assert p.read_bytes() == b"PACKS", f"仅 packs 有的文件应命中，实际 {p}"

    _with_fake_packs(run)


def test_accepts_parts_sequence_and_backslashes():
    def run(fake, data):
        a = dlc_packs.client_path(data, ("3d", "Tanks", "Both.sc2.dvpl"))
        b = dlc_packs.client_path(data, "3d\\Tanks\\Both.sc2.dvpl")
        assert a == b and a.read_bytes() == b"PACKS"

    _with_fake_packs(run)


def test_missing_everywhere_returns_data_path():
    """两边都没有时返回 Data 下的候选路径（调用方自行判 exists，保持 fail-closed）。"""
    def run(fake, data):
        p = dlc_packs.client_path(data, "3d/Tanks/Nope.sc2.dvpl")
        assert not p.exists() and p.parent.parent.parent == data

    _with_fake_packs(run)


def test_real_client_packs_if_present():
    """真实客户端抽查：packs 存在时，被覆盖的坦克模型必须解析到 packs。"""
    packs = dlc_packs.packs_dir()
    if packs is None:
        print("  （跳过：本机无 %LOCALAPPDATA%/wotblitz/packs）")
        return
    data = pathlib.Path(
        r"D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data")
    if not data.is_dir():
        print("  （跳过：本机无 Steam Data 目录）")
        return
    rel = "3d/Tanks/German/Ferdinand.scg.dvpl"
    p = dlc_packs.client_path(data, rel)
    assert "packs" in str(p).lower(), f"被 DLC 覆盖的模型应解析到 packs，实际 {p}"
    assert p.stat().st_size != (data / rel).stat().st_size, "packs 与 Data 应确为不同版本"


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
