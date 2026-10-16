"""2D 底图的**几何契约**（standalone，无 pytest 依赖）：索引 schemaVersion 2 的 worldBounds /
orientation 必须与注册表一致，且与真实像素、交接件字节三方自洽。

回归背景（2026-10-16）：坦克位置与底图错位的成因只可能是**取景**（裁剪/补边/换框）——2D 渲染把
回放坐标按注册表 `coordinateBounds`（±300 m）线性映射到整幅图，**栅格分辨率不参与映射**，所以
sha256/尺寸全对也校不出取景变化（尺寸可以不变）。本文件锁：
  1. `map2d/index.json` 逐图带几何契约，且与 WotbTools 注册表 `coordinateBounds` 逐值相等；
  2. 逐图声明的像素尺寸 == 真实 WebP 尺寸，且为方形（方形栅格 + ±300 方框 = 各向同性）；
  3. `map2d-source.json`（交接件）与索引同源：contentGeneratedAt == generated、mapsDigest 自洽、
     逐图 sha256 与 canonical staging / map2d 字节一致。

只读、不做图像重编码（Pillow 仅读头部）；不查 COS。

用法：python tools/test_2d_basemap_geometry.py
"""
import hashlib
import json
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / "tools"))

from PIL import Image  # noqa: E402

INDEX = REPO / "release/asset_pack/map2d/index.json"
STAGING = REPO / "release/2d-basemaps"
STAMP = STAGING / "map2d-source.json"

# `release/` 是 gitignored 的构建产物：全新检出（CI/他人机器）上这些用例无物可验。
# 与跨仓那条一致地**显式跳过**而不是报错——判据属于"打包后自检"，不是源码契约。
ARTIFACTS = INDEX.is_file() and STAMP.is_file()


def _load():
    idx = json.loads(INDEX.read_text(encoding="utf-8"))
    stamp = json.loads(STAMP.read_text(encoding="utf-8"))
    return idx, stamp


def test_index_schema_and_geometry_contract():
    if not ARTIFACTS:
        print('skip：本机没有 release/ 产物（gitignored）——打包后自检用例跳过')
        return
    idx, _ = _load()
    assert idx["schemaVersion"] == 2, "索引必须声明几何契约（schemaVersion 2）"
    maps = idx["maps"]
    assert len(maps) == 29, f"注册表 29 张，索引 {len(maps)} 张"
    for code, e in maps.items():
        b = e.get("worldBounds")
        assert b == {"xMin": -300.0, "xMax": 300.0, "yMin": -300.0, "yMax": 300.0}, f"{code}: worldBounds {b}"
        assert e.get("orientation") == "top=+Z,left=+X", f"{code}: orientation {e.get('orientation')}"
        assert e.get("width") == e.get("height"), f"{code}: 非方形 {e.get('width')}×{e.get('height')}"


def test_declared_dims_match_real_pixels():
    if not ARTIFACTS:
        print('skip：本机没有 release/ 产物（gitignored）——打包后自检用例跳过')
        return
    idx, _ = _load()
    for code, e in idx["maps"].items():
        with Image.open(REPO / "release/asset_pack/map2d" / f"{code}.webp") as im:
            assert im.size == (e["width"], e["height"]), f"{code}: 像素 {im.size} ≠ 索引 {e['width']}×{e['height']}"
            assert im.size[0] == im.size[1], f"{code}: 栅格非方形 {im.size}"


def test_stamp_is_same_generation_and_byte_bound():
    if not ARTIFACTS:
        print('skip：本机没有 release/ 产物（gitignored）——打包后自检用例跳过')
        return
    idx, stamp = _load()
    assert stamp["schemaVersion"] == 2
    assert stamp["contentGeneratedAt"] == idx["generated"], "交接件与索引必须同一代内容"
    assert stamp["indexSha256"] == hashlib.sha256(INDEX.read_bytes()).hexdigest(), "交接件 indexSha256 与索引实际字节不符"
    maps = stamp["maps"]
    assert len(maps) == len(idx["maps"]) == 29
    lines = []
    for code, entry in sorted(maps.items()):
        canonical = STAGING / entry["file"]
        digest = hashlib.sha256(canonical.read_bytes()).hexdigest()
        assert entry["sha256"] == digest, f"{code}: 交接件 sha256 与 canonical 字节不符（换图未更新交接件？）"
        assert idx["maps"][code]["sha256"] == digest, f"{code}: 索引 / 交接件 / canonical 三方 sha256 不一致"
        lines.append(f"{code}\t{digest}")
    assert stamp["mapsDigest"] == hashlib.sha256("\n".join(lines).encode("utf-8")).hexdigest(), "mapsDigest 自洽性失败"


def test_registry_bounds_are_the_same_contract_constant():
    """WotbTools 注册表的 coordinateBounds 必须与索引声明的世界框一致（跨仓契约，读源码常量）。"""
    reg = pathlib.Path("D:/Class/Rust/WotbTools/frontend/src/data/mapImages.js")
    if not reg.is_file():
        print("skip：本机没有 WotbTools 检出（跨仓校验跳过）")
        return
    text = reg.read_text(encoding="utf-8")
    assert re.search(r"WORLD_BOUNDS_300\s*=\s*\{\s*xMin:\s*-300,\s*xMax:\s*300,\s*yMin:\s*-300,\s*yMax:\s*300\s*\}", text), \
        "注册表的 WORLD_BOUNDS_300 常量变了——两侧世界框必须同步"


if __name__ == "__main__":
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    for fn in fns:
        fn()
        print(f"PASS {fn.__name__}")
    print(f"{len(fns)} tests passed")
