"""DVPL 外壳 / LZ4 解码契约单测：python tools/test_dvpl_decode.py（standalone，无 pytest 依赖）。

锁 `wotb_sc2.py` 的 fail-closed 口径，与 Rust 侧 `src/wargaming/dvpl.rs` 的
`DvplFile::parse` / `lz4_decompress` 同一套不变量——两侧口径必须一致，否则同一个
客户端文件在两套解码器上会得出相反结论（2026-10-08 之前正是这种状态：Rust 三项全
不校验、Python 三项全查）。

其中**零偏移**一项是特例：它不是规范 LZ4，但客户端确实产出（全树 45016 个 .dvpl 里
1 例，`F114_Projet_4_1_skin_MISC.dx11.dds.dvpl`），参考实现（lz4 C）解成 match_length
个零字节，故两侧都按零处理而非拒绝。上面那个文件同时比对过 sha256 一致。
"""
import pathlib
import struct
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from wotb_sc2 import Sc2ParseError, decode_dvpl, lz4_block_decompress  # noqa: E402


def lz4_literals(data: bytes) -> bytes:
    """仅字面量的 LZ4 块（合法的最小序列：末段不带匹配部分）。"""
    out = bytearray()
    if len(data) < 15:
        out.append(len(data) << 4)
    else:
        out.append(0xF0)
        rest = len(data) - 15
        while rest >= 255:
            out.append(255)
            rest -= 255
        out.append(rest)
    out.extend(data)
    return bytes(out)


def lz4_literals_then_match() -> tuple[bytes, bytes]:
    """8 字节字面量 + 回引 4088 字节的 LZ4 块，正好铺满 4096 字节。"""
    unit = b"ABCDEFGH"
    output_size = 4096
    match_len = output_size - len(unit)
    block = bytearray([(len(unit) << 4) | 0x0F])  # 低 4 位=15 → 匹配长度走扩展
    block.extend(unit)
    block.extend(len(unit).to_bytes(2, "little"))  # offset = 8
    extra = match_len - 19  # 19 = 低 4 位 15 + 基准 4
    while extra >= 255:
        block.append(255)
        extra -= 255
    block.append(extra)
    expected = (unit * (output_size // len(unit) + 1))[:output_size]
    return bytes(block), expected


def dvpl_blob(comp_type: int, decoded_size: int, payload: bytes) -> bytes:
    """按真实编码器的写法拼一个 DVPL：载荷 + footer。"""
    import zlib
    footer = struct.pack(
        "<III4s4s",
        decoded_size,
        len(payload),
        zlib.crc32(payload) & 0xFFFFFFFF,
        bytes([comp_type, 0, 0, 0]),
        b"DVPL",
    )
    return payload + footer


def expect_error(fn, needle: str):
    try:
        fn()
    except Sc2ParseError as err:
        assert needle in str(err), f"错误信息不含 {needle!r}: {err}"
        return
    raise AssertionError(f"应当报错（期望含 {needle!r}）")


def test_lz4_literals_roundtrip():
    text = bytes(b"abcdefgh"[i % 8] for i in range(300))
    assert lz4_block_decompress(lz4_literals(text), len(text)) == text


def test_lz4_match_roundtrip():
    block, expected = lz4_literals_then_match()
    assert lz4_block_decompress(block, len(expected)) == expected


def test_lz4_offset_zero_writes_zeros():
    """零偏移 → match_length 个零字节（参考实现口径，不得按非法偏移拒绝）。"""
    block = bytes([0x41]) + b"wxyz" + (0).to_bytes(2, "little")  # 4 字面量 + 匹配 5
    out = lz4_block_decompress(block, 9)
    assert out == b"wxyz" + bytes(5), out


def test_lz4_rejects_offset_beyond_produced():
    block = bytes([0x4F]) + b"wxyz" + (8).to_bytes(2, "little") + bytes([200])
    expect_error(lambda: lz4_block_decompress(block, 4 + 19 + 200), "Invalid LZ4 match offset")


def test_lz4_rejects_truncated():
    block = lz4_literals(b"A" * 300)[: 3 + 100]
    expect_error(lambda: lz4_block_decompress(block, 300), "literal")


def test_lz4_rejects_size_mismatch():
    text = b"B" * 64
    expect_error(lambda: lz4_block_decompress(lz4_literals(text), 128), "size mismatch")


def test_dvpl_decodes_and_rejects_crc():
    payload = b"payload bytes"
    blob = dvpl_blob(0, len(payload), payload)
    assert decode_dvpl(blob) == payload

    tampered = bytearray(blob)
    tampered[0] ^= 0xFF
    expect_error(lambda: decode_dvpl(bytes(tampered)), "CRC")


def test_dvpl_rejects_packed_size_mismatch():
    payload = b"payload bytes"
    blob = bytearray(dvpl_blob(0, len(payload), payload))
    n = len(blob)
    blob[n - 20 + 4 : n - 20 + 8] = (4096).to_bytes(4, "little")  # 谎报编码长度
    expect_error(lambda: decode_dvpl(bytes(blob)), "packed-size")


def test_dvpl_rejects_bad_footer():
    expect_error(lambda: decode_dvpl(b"too short"), "too small")
    blob = bytearray(dvpl_blob(0, 3, b"abc"))
    blob[-4:] = b"XXXX"  # magic 损坏
    expect_error(lambda: decode_dvpl(bytes(blob)), "Missing DVPL footer")


def test_dvpl_offset_zero_end_to_end():
    """零偏移走完整 DVPL 外壳（与真机那个文件同形态）。"""
    block = bytes([0x41]) + b"wxyz" + (0).to_bytes(2, "little")
    blob = dvpl_blob(2, 9, block)
    assert decode_dvpl(blob) == b"wxyz" + bytes(5)


def test_dvpl_decodes_zlib_type3():
    """type 3 = zlib（与 Rust 侧同口径）。此前 Python 直接抛 unsupported，两端对同一文件
    会给出相反结论；本机 45016 个文件实测无 type 3，故属潜在分歧。"""
    import zlib as _z
    text = b"zlib payload for type 3 round trip" * 4
    blob = dvpl_blob(3, len(text), _z.compress(text))
    assert decode_dvpl(blob) == text


def test_dvpl_rejects_unknown_type():
    import zlib as _z
    payload = _z.compress(b"x" * 10)
    expect_error(lambda: decode_dvpl(dvpl_blob(4, 10, payload)),
                 "Unsupported DVPL compression type 4")


def test_pvr3_single_channel_l8_a8_decodes():
    """PVR3 单通道（bits=(8,0,0,0)，'l' 亮度 / 'a' 纯 alpha）必须解出——本机 1328 张
    PVR 里 954 张属此类，此前一律 return None 被静默丢弃。无客户端时跳过。"""
    import pathlib as _pl
    import sys as _sys
    data = _pl.Path(r"D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data")
    if not data.is_dir():
        print("  （跳过：本机无客户端）")
        return
    _sys.path.insert(0, str(_pl.Path(__file__).resolve().parent))
    import export_map_glb as _M
    from wotb_sc2 import decode_dvpl as _dd
    n = 0
    for p in data.rglob("*.pvr.dvpl"):
        try:
            d = _dd(p.read_bytes())
        except Exception:
            continue
        if d[:4] != b"PVR" or tuple(d[12:16]) != (8, 0, 0, 0):
            continue
        img = _M.decode_pvr3(d)
        assert img is not None, f"单通道 PVR 应解出: {p}"
        assert img.size[0] == img.size[1], f"本批实测皆为方图: {img.size}"
        n += 1
    assert n > 100, f"样本量过少: {n}"
    print(f"  单通道 PVR 解出 {n} 张")


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
