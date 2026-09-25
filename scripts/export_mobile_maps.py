#!/usr/bin/env python3
"""独立导出 WoTB 全部地图小地图底图到 mobile_assets/maps/，供移动端 App 随包分发。

背景：桌面版底图由 src/wargaming/map_assets.rs 在请求时从本机游戏客户端提取
（缓存于 data/maps/_cache/），移动端没有游戏目录，必须改为随包资源。
本脚本零依赖（仅 Python 标准库）、只读游戏目录，可独立于 Rust 代码运行，
输出文件名与 Rust 端缓存命名一致（<MapIdDebugName>.webp），后续直接打进 App 资源包。

数据源规则（与 map_assets.rs 一致，改表时需同步）：
- 贴图：Data/Gfx/UI/BattleScreenHUD/minimap/<内部名>/MiniMapSmall@2x.packed.webp.dvpl
  （优先 @2x，退 MiniMapSmall.packed.webp.dvpl）；DVPL 壳 = 数据 + 20 字节 footer，
  压缩类型 0=未压缩（小地图实测均为 0）/ 1,2=LZ4 块 / 3=zlib；
- 名称映射：解析器 MapId Debug 名 → 游戏内 minimap 目录名，权威表在
  src/wargaming/map_assets.rs 的 MAP_DIRS，下表由其复制。

用法：
  python scripts/export_mobile_maps.py                # 导出全部缺失的图
  python scripts/export_mobile_maps.py --force        # 全部重新导出
  python scripts/export_mobile_maps.py --into-cache   # 同时写入 data/maps/_cache/（桌面版提速）
  python scripts/export_mobile_maps.py --game-dir X   # 显式指定游戏 Data 目录
"""

import argparse
import hashlib
import json
import struct
import sys
import zlib
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUT = PROJECT_ROOT / "mobile_assets" / "maps"
CACHE_DIR = PROJECT_ROOT / "data" / "maps" / "_cache"

# 与 resolve_game_dir 的 DEFAULT_GAME_DIRS 保持一致
DEFAULT_GAME_DIRS = [
    "D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
    "C:/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
]

# 复制自 src/wargaming/map_assets.rs::MAP_DIRS（同步注意：那边是权威）
MAP_DIRS = [
    ("DesertSands", "desert_train"),
    ("Middleburg", "erlenberg"),
    ("Copperfield", "karieri"),
    ("Alpenstadt", "lumber"),
    ("Mines", "rudniki"),
    ("DeadRail", "medvedkovo"),
    ("FortDespair", "fort"),
    ("Himmelsdorf", "himmelsdorf"),
    ("BlackGoldville", "mountain"),
    ("OasisPalms", "savanna"),
    ("GhostFactory", "plant"),
    ("Molendijk", "holland"),
    ("PortBay", "port"),
    ("WinterMalinovka", "malinovka"),
    ("Castilla", "pliego"),
    ("Canal", "canal"),
    ("Vineyards", "italy"),
    ("YamatoHarbor", "milbase"),
    ("Canyon", "canyon"),
    ("MayanRuins", "rock"),
    ("DynastyPearl", "grossberg"),
    ("NavalFrontier", "skit"),
    ("FallsCreek", "amigosville"),
    ("NewBay", "forgecity"),
    ("Normandy", "neptune"),
    ("Wasteland", "holmeisk"),
]

TEXTURE_CANDIDATES = [
    "MiniMapSmall@2x.packed.webp.dvpl",
    "MiniMapSmall.packed.webp.dvpl",
]

# ---------------------------------------------------------------- DVPL 解码

def lz4_block_decompress(src: bytes, output_size: int) -> bytes:
    """纯 Python LZ4 块解压（与 src/wargaming/dvpl.rs::lz4_decompress 同算法）。"""
    dst = bytearray(output_size)
    si = di = 0
    n = len(src)
    while si < n and di < output_size:
        token = src[si]; si += 1
        lit_len = (token >> 4) & 0x0F
        if lit_len == 15:
            while si < n:
                b = src[si]; si += 1
                lit_len += b
                if b != 255:
                    break
        lit_len = min(lit_len, output_size - di, n - si)
        dst[di:di + lit_len] = src[si:si + lit_len]
        si += lit_len; di += lit_len
        if si >= n or di >= output_size:
            break
        if si + 2 > n:
            break
        offset = src[si] | (src[si + 1] << 8); si += 2
        match_len = (token & 0x0F) + 4
        if (token & 0x0F) == 15:
            while si < n:
                b = src[si]; si += 1
                match_len += b
                if b != 255:
                    break
        match_len = min(match_len, output_size - di)
        for _ in range(match_len):
            dst[di] = dst[di - offset]
            di += 1
    if di != output_size:
        raise ValueError(f"LZ4 解压不完整: {di}/{output_size}")
    return bytes(dst)


def dvpl_decode(raw: bytes) -> bytes:
    """DVPL 壳解码：末尾 20 字节 footer = input_size + comp_size + crc32 + type + 'DVPL'。"""
    if len(raw) < 20 or raw[-4:] != b"DVPL":
        raise ValueError("不是 DVPL 文件（footer/magic 不符）")
    input_size, comp_size, _crc, comp_type = struct.unpack_from("<IIII", raw, len(raw) - 20)
    compressed = raw[:comp_size]
    if comp_type == 0:
        data = compressed
    elif comp_type in (1, 2):
        data = lz4_block_decompress(compressed, input_size)
    elif comp_type == 3:
        data = zlib.decompress(compressed)
    else:
        raise ValueError(f"未知 DVPL 压缩类型 {comp_type}")
    if len(data) != input_size:
        raise ValueError(f"解压后长度 {len(data)} != footer 声明 {input_size}")
    return data

# ---------------------------------------------------------------- WebP 尺寸解析（尽力而为）

def webp_dimensions(data: bytes):
    if len(data) < 30 or data[:4] != b"RIFF" or data[8:12] != b"WEBP":
        return None
    fourcc = data[12:16]
    try:
        if fourcc == b"VP8X":
            w = int.from_bytes(data[24:27], "little") + 1
            h = int.from_bytes(data[27:30], "little") + 1
            return w, h
        if fourcc == b"VP8 ":
            w = struct.unpack_from("<H", data, 26)[0] & 0x3FFF
            h = struct.unpack_from("<H", data, 28)[0] & 0x3FFF
            return w, h
        if fourcc == b"VP8L":
            bits = int.from_bytes(data[21:25], "little")
            return (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1
    except (IndexError, struct.error):
        return None
    return None

# ---------------------------------------------------------------- 导出

def find_game_dir(explicit: str | None) -> Path:
    candidates = [Path(explicit)] if explicit else [Path(d) for d in DEFAULT_GAME_DIRS]
    for p in candidates:
        if p.is_dir():
            return p
    joined = " , ".join(str(c) for c in candidates)
    sys.exit(f"[export-maps] 找不到游戏 Data 目录（尝试过: {joined}），用 --game-dir 指定")


def export_all(game_dir: Path, out_dir: Path, force: bool, into_cache: bool) -> int:
    out_dir.mkdir(parents=True, exist_ok=True)
    results, failed = [], []
    for name, internal in MAP_DIRS:
        out_path = out_dir / f"{name}.webp"
        if out_path.exists() and not force:
            data = out_path.read_bytes()
            results.append({"name": name, "internal": internal, "source": "(已有)",
                            "variant": None, "bytes": len(data),
                            "sha256": hashlib.sha256(data).hexdigest(),
                            "dimensions": webp_dimensions(data)})
            continue
        got = None
        for variant, fname in zip(("@2x", "1x"), TEXTURE_CANDIDATES):
            p = game_dir / "Gfx" / "UI" / "BattleScreenHUD" / "minimap" / internal / fname
            if not p.exists():
                continue
            try:
                data = dvpl_decode(p.read_bytes())
            except (OSError, ValueError) as e:
                print(f"  [warn] {name}: {p.name} 解码失败 {e}")
                continue
            if data[:4] != b"RIFF" or data[8:12] != b"WEBP":
                print(f"  [warn] {name}: {p.name} 解码后不是 WebP")
                continue
            got = (variant, p, data)
            break
        if got is None:
            failed.append(name)
            print(f"  [miss] {name}（{internal}）: 游戏目录内无可用贴图")
            continue
        variant, src_path, data = got
        out_path.write_bytes(data)
        if into_cache:
            CACHE_DIR.mkdir(parents=True, exist_ok=True)
            (CACHE_DIR / f"{name}.webp").write_bytes(data)
        results.append({"name": name, "internal": internal, "source": str(src_path),
                        "variant": variant, "bytes": len(data),
                        "sha256": hashlib.sha256(data).hexdigest(),
                        "dimensions": webp_dimensions(data)})
        dims = results[-1]["dimensions"]
        dims_s = f" {dims[0]}x{dims[1]}" if dims else ""
        print(f"  [ok]   {name}.webp  {variant}  {len(data)/1024:.0f}KB{dims_s}")

    manifest = {"maps": results,
                "summary": {"total": len(MAP_DIRS), "exported": len(results),
                            "failed": failed,
                            "total_bytes": sum(r["bytes"] for r in results)}}
    (out_dir / "maps_manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")

    print(f"\n完成: {len(results)}/{len(MAP_DIRS)} 张 -> {out_dir}"
          f"（合计 {manifest['summary']['total_bytes']/1024/1024:.1f}MB）")
    if failed:
        print(f"失败清单: {', '.join(failed)}")
        return 1
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--game-dir", help="游戏 Data 目录（默认自动探测）")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT, help=f"输出目录（默认 {DEFAULT_OUT}）")
    ap.add_argument("--force", action="store_true", help="已存在的也重新导出")
    ap.add_argument("--into-cache", action="store_true",
                    help="同时写入 data/maps/_cache/（桌面版直接命中缓存）")
    args = ap.parse_args()

    game_dir = find_game_dir(args.game_dir)
    print(f"[export-maps] 游戏目录: {game_dir}")
    print(f"[export-maps] 输出目录: {args.out}\n")
    sys.exit(export_all(game_dir, args.out, args.force, args.into_cache))


if __name__ == "__main__":
    main()
