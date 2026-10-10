"""悬挂导出器契约单测：python tools/test_export_tank_suspension.py（standalone，无 pytest 依赖）。

锁 `tools/export_tank_suspension.py` 的口径：
- **blob 解码**：YAML 双引号里的 `\\f` 是 0x0C 终止符的**转义写法**（两字符），空块整串就是
  `"\\f"`；解码后长度必须是 12 的倍数（`wheels` = `{u32 flag, f32 a, f32 b}`，链折线 =
  `{f32 x, f32 y, u32 flag}`）。
- **两代键式**：复数 map（`leftTrackChains`，键 = 段序号，多段模型多键）与单数串
  （`leftTrackChain`，BT-7 一类旧文件）。
- **fail-closed**：无 suspension 段 → None；`wheels` 空块 / 链为空（Oth10_WarDuck 形状）
  → `empty_suspension` 而非造缺省值；块长度不是 12 的倍数 → 解析错误。
- **行式解析**的缩进边界：只取顶层 `suspension:`/`chassis:` 段，段外同名键（嵌套子表的键）
  不得串入。

真实客户端 yaml 的抽样回归在文件末尾（客户端目录缺失时自动跳过）。
"""
import base64
import pathlib
import struct
import sys

_HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))
sys.path.insert(0, str(_HERE / "wotbtools"))

from export_tank_suspension import (  # noqa: E402
    SuspensionParseError, decode_blob, parse_chain, parse_suspension, parse_wheels,
)


def _b64(raw: bytes) -> str:
    return base64.b64encode(raw).decode("ascii")


def _wheels_blob(recs: list[tuple[int, float, float]]) -> str:
    return _b64(b"".join(struct.pack("<Iff", *r) for r in recs))


def _chain_blob(pts: list[tuple[float, float, int]]) -> str:
    return _b64(b"".join(struct.pack("<ffI", *p) for p in pts))


YAML_PLURAL = """
resourcesPath:
    blitzModelPath: "Tanks/USSR/IS-7.sc2"
textureTransform:
    hull:
        scale: [1.1, 1.1]
treadParameters:
    manualWidth: 0.835616
suspension:
    enabled: true
    wheelsReactionSpeed: 1.100000
    chunkPrototypeExtending: 0.100000
    wheels: "%(wheels)s\\f"
    leftTrackChains:
        00: "%(chain)s\\f"
    rightTrackChains:
        00: "%(chain)s\\f"
    trackBendingInfo:
        frontDriveWheel: false
        upperMin: 0.280000
        upperFactor: 0.590000
        frontFactor: 0.400000
        backFactor: 0.250000
        lengthPower: 0.500000
        speed: 0.900000
    trackLayingInfo:
        bendingFactor: 0.744000
        lengthPower: 0.920000
        pointCountPower: 0.918000
        pressurePower: 0.260000
        primaryPower: 0.670000
chassis:
    textureScale: -0.548000
""" % {"wheels": _wheels_blob([(0, 0.13, 0.13), (1, 0.13, 0.13), (1, 0.0723, 0.0542)]),
       "chain": _chain_blob([(-2.44, 0.0016, 1), (-1.60, 0.0016, 1), (3.13, 0.245, 0)])}

def test_blob_escape_and_padding():
    raw = struct.pack("<Iff", 1, 0.5, 0.25)
    txt = _b64(raw) + "\\f"           # 文件里的形状：base64 + 转义终止符（两字符）
    assert decode_blob(txt) == raw
    assert decode_blob(_b64(raw) + "\x0c") == raw     # 真字节形式也接受
    assert decode_blob("\\f") == b""                  # 空块整串
    assert decode_blob("") == b""


def test_wheels_records():
    recs = [(0, 0.0556, 0.0839), (1, 0.0556, 0.0839)]
    got = parse_wheels(_wheels_blob(recs) + "\\f")
    assert got == [[0, 0.0556, 0.0839], [1, 0.0556, 0.0839]], got
    # 长度不是 12 的倍数 → fail-closed
    try:
        parse_wheels(_b64(b"\x01\x02\x03") + "\\f")
    except SuspensionParseError:
        pass
    else:
        raise AssertionError("非 12 倍数长度必须拒绝")


def test_chain_points():
    pts = [(-2.44, 0.00158, 1), (3.13, 0.245, 0)]
    assert parse_chain(_chain_blob(pts) + "\\f") == [[-2.44, 0.00158, 1], [3.13, 0.245, 0]]


def test_parse_plural_block():
    doc = parse_suspension(YAML_PLURAL)
    assert doc is not None
    assert doc["enabled"] is True
    assert doc["wheels_reaction_speed"] == 1.1
    assert doc["chunk_prototype_extending"] == 0.1
    assert len(doc["wheels"]) == 3 and doc["wheels"][0] == [0, 0.13, 0.13]
    assert doc["chain_keys"] == ["00"]
    assert len(doc["chains"]["left"][0]) == 3
    assert doc["chains"]["left"] == doc["chains"]["right"]
    assert doc["track_bending"]["front_drive_wheel"] is False
    assert doc["track_bending"]["length_power"] == 0.5
    assert doc["track_laying"]["point_count_power"] == 0.918
    assert doc["texture_scale"] == -0.548
    # 段外同名键不得串入（treadParameters 的 manualWidth 不是悬挂字段）
    assert "manual_width" not in doc


def test_parse_singular_keys():
    cfg = "suspension:\n    enabled: true\n    wheelsReactionSpeed: 1.0\n"
    cfg += '    wheels: "%s\\f"\n' % _wheels_blob([(1, 0.05, 0.05)] * 4)
    cfg += '    leftTrackChain: "%s\\f"\n' % _chain_blob([(0.0, 0.0, 1), (1.0, 1.0, 1)])
    cfg += '    rightTrackChain: "%s\\f"\n' % _chain_blob([(0.0, 0.0, 1), (1.0, 1.0, 1)])
    cfg += "    trackBendingInfo:\n        frontDriveWheel: true\n        upperMin: 0.0\n"
    cfg += "        upperFactor: 0.0\n        frontFactor: 0.1\n        backFactor: 0.1\n"
    cfg += "        lengthPower: 0.5\n        speed: 1.0\n"
    cfg += "    trackLayingInfo:\n        bendingFactor: 0.5\n        lengthPower: 0.9\n"
    cfg += "        pointCountPower: 0.9\n        pressurePower: 0.2\n        primaryPower: 0.6\n"
    doc = parse_suspension(cfg)
    assert doc is not None and doc["chain_keys"] == ["0"]
    assert len(doc["chains"]["left"]) == 1 and len(doc["chains"]["left"][0]) == 2
    assert doc["track_bending"]["front_drive_wheel"] is True
    assert "texture_scale" not in doc          # 无 chassis 段 → 不造缺省


def test_fail_closed_shapes():
    assert parse_suspension("resourcesPath:\n    blitzModelPath: x\n") is None
    empty = ("suspension:\n    wheelsReactionSpeed: 0.000000\n"
             '    wheels: "\\f"\n    leftTrackChains: {}\n    rightTrackChains: {}\n')
    try:
        parse_suspension(empty)            # Oth10_WarDuck 形状
    except SuspensionParseError as e:
        assert str(e).startswith("empty_suspension"), e
    else:
        raise AssertionError("空 wheels 必须 fail-closed")
    # 有 wheels 但链为空 → 同样 fail-closed
    nolink = ("suspension:\n    wheelsReactionSpeed: 1.0\n"
              '    wheels: "%s\\f"\n    leftTrackChains: {}\n    rightTrackChains: {}\n'
              % _wheels_blob([(1, 0.1, 0.1)]))
    try:
        parse_suspension(nolink)
    except SuspensionParseError as e:
        assert str(e).startswith("empty_suspension"), e
    else:
        raise AssertionError("空链必须 fail-closed")


def test_real_client_sample():
    """真实客户端 yaml 抽样：IS-7 的 9 轮 / 63 点链、{flag,a,b} 与逆向结论一致。"""
    try:
        from dlc_packs import client_path
        from export_tank_glb import default_game_data
        from wotb_sc2 import decode_dvpl
    except ImportError:
        print("  (跳过：工具链依赖不可用)")
        return
    gd = default_game_data()
    if not gd.is_dir():
        print("  (跳过：本机无客户端目录)")
        return
    p = client_path(gd, "3d/Tanks/Parameters/ussr/IS-7.yaml.dvpl")
    if not p.exists():
        print("  (跳过：样本车 yaml 不存在)")
        return
    doc = parse_suspension(decode_dvpl(p.read_bytes()).decode("utf-8", "replace"))
    assert doc is not None, "IS-7 应有 suspension 块"
    assert len(doc["wheels"]) == 9, len(doc["wheels"])
    flags = [w[0] for w in doc["wheels"]]
    assert flags == [0] + [1] * 7 + [0], flags          # 首尾 = 诱导/主动轮
    assert all(w[1] == 0.13 and w[2] == 0.13 for w in doc["wheels"])
    chain = doc["chains"]["left"][0]
    assert 60 <= len(chain) <= 65, len(chain)
    ys = [pt[1] for pt in chain]
    assert max(ys) > 1.0 and min(ys) < 0.01              # 折线跨整个履带环
    assert doc["texture_scale"] == -0.548
    print(f"  IS-7: {len(doc['wheels'])} 轮 / 链 {len(chain)} 点 / textureScale {doc['texture_scale']}")


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
