"""导出 2D 战术底图（canonical 1254²）+ picker 封面（cover 512²）——由 asset_pack 的 ground.webp 生成。

链条（每一步都可复现，参数是文件内常量）：
  release/asset_pack/map/<key>/ground.webp（4096²，±300 m 世界框，顶行 = +Z_scene、左列 = +X_scene）
    → LANCZOS 等比缩放到 1254²
    → 全局对比曲线 y' = softclip(128 + (y−128)·GAIN)（输出两端各 SOFT_M 级指数软肩）
    → 饱和 ×SAT
    → 逐通道软肩（越过 [0,255] 的部分压成渐近线，无硬裁切）
    → USM 配方 B：1.0px / 80% / thr2（半径口径 = 1254 尺度，对应 1024 尺度的 0.8px）
    → WebP q90 / method 6 ⇒ canonical 1254² + 同图重采样 512² cover（q80）——
      同一调色管线两次采样，观感一致；cover 供 picker 封面（44–112KB 级）。

**无需翻转/裁剪**：ground.webp 与 2D 渲染在同一坐标系（推导见 docs/game-data-sources.md「2D 底图」）；
两侧都是固定契约，不按相关性搜索朝向（同 tools/composite_overhead.py 的纪律）。

清单来源（v3）：**资产包 `index.json` 全量枚举**（去重后的全部地图 key）——不再依赖前端
`mapImages.js` 注册表（手维护、曾经只有 29 张，新图/变体空间/老版复刻进不了循环）。
pack 里每张图都有 ground.webp（打包链保证），原料齐备即全覆盖。

纪律：
- 配方参数是常量、**全部地图同一套**，不做逐图统计决策（AGENTS.md：同类数据同一变换）；
- fail closed：pack index 读不到 / 源尺寸不是 4096² / worldBounds 不是 ±300 m / 输出目录含无关 webp /
  派生尺寸不符 —— 任一不成立直接失败，不"生成了就当成功"；
- **几何契约随件落盘**（索引 schemaVersion 2）：逐图 `worldBounds` + `orientation` 写进
  `map2d/index.json`，`--verify` 回读真实像素尺寸并要求方形且与索引一致 —— 只校验 sha256/字节
  校不出"换了图但世界范围/取景变了"（裁剪、补边后尺寸可以不变），那正是坦克位置与底图错位的成因；
- 确定性：同输入 + 同 Pillow 版本 → 逐字节相同；`--verify` 全量重算 sha256 复核。

用法：
  python tools/export_2d_basemaps.py --self-test
  python tools/export_2d_basemaps.py --out release/2d-basemaps            # 全部 43 张 + manifest + covers
  python tools/export_2d_basemaps.py --out release/2d-basemaps --map canal
  python tools/export_2d_basemaps.py --verify --out release/2d-basemaps   # 只读：重算并逐张比对 sha256
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

REPO = Path(__file__).resolve().parents[1]
TOOL_VERSION = 3   # v3：清单源改 pack index 全量枚举（不再依赖前端注册表）+ cover 512² 输出

# ---- 冻结的配方（改这里就是改产品决策；任何改动都会改变全部 43 张输出）----
CANON_DIM = 1254
CANON_Q, CANON_M = 90, 6
COVER_DIM, COVER_Q, COVER_M = 512, 80, 4   # picker 封面：低分辨率轻量（44–112KB 级）
GAIN = 1.17          # 全局对比曲线斜率（用户选定 2026-10-16）
SOFT_M = 24.0        # 软肩宽度（输出级数）
SAT = 0.9748         # 饱和修正（= 2026-10-16 拟合值 ref_chroma/cand_chroma，四位小数；勿四舍五入到 0.975——
                     #   那会改变全部输出字节，与用户审阅的方案不再逐字节一致）
USM = (1.0, 80, 2)   # radius(px@1254) / percent / threshold
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
EXPECTED_SRC_DIM = 4096
EXPECTED_WORLD_SPAN = 600.0


class ExportError(RuntimeError):
    """可复现的失败：消息里必须能一眼看出是哪张图、哪个前置条件不成立。"""


def now_iso() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def registry_all(pack: Path) -> list[dict]:
    """全部地图（pack `index.json` 全量去重枚举）：pack key、canonical 文件名、展示名。fail closed。

    v3：清单源从 WotbTools `mapImages.js` 换成 pack index —— 手维护注册表曾经只有 29 张，
    新图/变体空间/老版复刻永远进不了循环（43 张里缺 14 的根因）。pack 里每张图都有
    ground.webp（打包链保证），原料齐备即全覆盖。canonical 文件名 = `<key>.webp`
    （与 pack map2d/ 现行命名一致）。
    """
    idx_path = pack / "index.json"
    if not idx_path.is_file():
        raise ExportError(f"缺 {idx_path}——先跑资产打包（export_asset_pack.py）")
    idx = json.loads(idx_path.read_text(encoding="utf-8")).get("maps", {})
    if not idx:
        raise ExportError("pack index.json 里没有 maps 条目")
    rows, seen = [], set()
    for _mid, m in idx.items():
        if not isinstance(m, dict):
            continue
        key = str(m.get("key") or "")
        if not key or key in seen:
            continue   # 变体空间归并基础图（首个出现的 id = 基础 id）
        seen.add(key)
        rows.append({"code": key, "file": f"{key}.webp", "display": m.get("display") or key})
    if not rows:
        raise ExportError("pack index.json 枚举结果为空")
    return rows


def soft_clip(v: np.ndarray, m: float = SOFT_M) -> np.ndarray:
    """逐通道指数软肩：越界部分压成渐近线（单调、无硬裁切）。"""
    hi, lo = 255.0 - m, m
    return np.where(
        v > hi, hi + m * (1.0 - np.exp(-(v - hi) / m)),
        np.where(v < lo, lo - m * (1.0 - np.exp(-(lo - v) / m)), v),
    )


def build_lut(gain: float = GAIN, m: float = SOFT_M) -> np.ndarray:
    v = np.arange(256, dtype=np.float64)
    y = 128.0 + (v - 128.0) * gain
    out = soft_clip(y, m)
    out = np.maximum.accumulate(out)
    out[0], out[-1] = 0.0, 255.0
    return out


LUT = build_lut()


def luma(a: np.ndarray) -> np.ndarray:
    return a @ LUMA


def grade(a: np.ndarray) -> np.ndarray:
    """对比曲线（按亮度加增量，色相不动）→ 饱和 → 逐通道软肩。"""
    y = np.clip(luma(a), 0, 255)
    y2 = np.interp(y, np.arange(256), LUT).astype(np.float32)
    g = a + (y2 - y)[..., None]
    y2 = luma(g)
    g = y2[..., None] + (g - y2[..., None]) * SAT
    return soft_clip(g)


def export_one(pack: Path, row: dict) -> tuple[bytes, bytes, dict]:
    key, name = row["code"], row["file"]
    ground_path = pack / "map" / key / "ground.webp"
    terrain_path = pack / "map" / key / "terrain.json"
    if not ground_path.is_file():
        raise ExportError(f"{key}: 缺 {ground_path}")
    if not terrain_path.is_file():
        raise ExportError(f"{key}: 缺 {terrain_path}")
    terrain = json.loads(terrain_path.read_text(encoding="utf-8"))
    wb = terrain.get("worldBounds") or {}
    lo, hi = wb.get("min"), wb.get("max")
    if not (isinstance(lo, list) and isinstance(hi, list) and len(lo) == 3 and len(hi) == 3):
        raise ExportError(f"{key}: terrain.json 缺 worldBounds")
    for axis, i in (("x", 0), ("z", 1)):
        span = float(hi[i]) - float(lo[i])
        if abs(span - EXPECTED_WORLD_SPAN) > 1e-6 or abs(float(lo[i]) + 300.0) > 1e-6:
            raise ExportError(f"{key}: worldBounds {axis} 不是 ±300 m（{lo[i]}..{hi[i]}）——"
                              "2D 渲染契约假定 ±300，先确认为何偏离")
    src = Image.open(ground_path)
    if src.size != (EXPECTED_SRC_DIM, EXPECTED_SRC_DIM):
        raise ExportError(f"{key}: 源 ground.webp 尺寸 {src.size} ≠ {EXPECTED_SRC_DIM}²")

    base = np.asarray(src.convert("RGB").resize((CANON_DIM, CANON_DIM), Image.LANCZOS)).astype(np.float32)
    img = Image.fromarray(np.clip(grade(base), 0, 255).astype(np.uint8))
    img = img.filter(ImageFilter.UnsharpMask(radius=USM[0], percent=USM[1], threshold=USM[2]))
    buf = io.BytesIO()
    img.save(buf, format="WEBP", quality=CANON_Q, method=CANON_M)
    data = buf.getvalue()
    if img.size != (CANON_DIM, CANON_DIM):
        raise ExportError(f"{key}: 输出尺寸 {img.size} 不符")
    # cover 512²：同一调色管线第二次采样（picker 封面，44–112KB 级）
    cover_img = img.resize((COVER_DIM, COVER_DIM), Image.LANCZOS)
    cover_buf = io.BytesIO()
    cover_img.save(cover_buf, format="WEBP", quality=COVER_Q, method=COVER_M)
    cover_data = cover_buf.getvalue()
    return data, cover_data, {"code": key, "file": name, "display": row["display"],
                              # 世界框随件落盘（contract，不搜索）：顶行 = +Z、左列 = +X，覆盖 ±300 全幅。
                              # OTA 消费侧（WotbTools `map2dOverlay.js`）据此校验"换了图但世界范围变了"。
                              "worldBounds": {"xMin": -300.0, "xMax": 300.0, "yMin": -300.0, "yMax": 300.0},
                              "orientation": "top=+Z,left=+X",
                              "source": {"path": str(ground_path.relative_to(REPO)) if ground_path.is_relative_to(REPO) else str(ground_path),
                                         "bytes": ground_path.stat().st_size, "sha256": sha256_file(ground_path)},
                              "output": {"width": CANON_DIM, "height": CANON_DIM,
                                         "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()},
                              "cover": {"width": COVER_DIM, "height": COVER_DIM,
                                        "bytes": len(cover_data), "sha256": hashlib.sha256(cover_data).hexdigest()}}


def preflight_out(out: Path, names: set[str]) -> None:
    """输出目录只允许写注册表里的文件名；出现无关 webp ⇒ 立刻失败（防指错目录）。"""
    if out.exists():
        stray = sorted(p.name for p in out.glob("*.webp") if p.name not in names)
        if stray:
            raise ExportError(f"输出目录 {out} 含注册表之外的 webp：{stray[:5]}{'…' if len(stray) > 5 else ''}"
                              "——拒绝写入，请换目录或清理")


INDEX_SCHEMA = 2
SOURCE_NAME = "map2d-source.json"


def write_map2d(map2d_dir: Path, records: list[dict], now_iso: str) -> dict:
    """COS 免发版通道：`map2d/<code>.webp`（与 canonical 同字节）+ `map2d/index.json`（App 的版本判据）。

    索引是**唯一判据**：App 只按它决定"包内那份是不是最新"，字节必须与索引逐件 sha256 对齐
    （fail closed）。上传由 `tools/upload_asset_pack_cos.py` 承担（json → no-cache、webp → max-age=3600）。

    schemaVersion 2（2026-10-16）：逐图增 `worldBounds` + `orientation` —— 消费侧据此校验
    "换了图但世界范围变了"（只校验 sha256/尺寸是校不出来的：裁剪/补边后尺寸可以不变）。
    """
    map2d_dir.mkdir(parents=True, exist_ok=True)
    maps_entries = {}
    for rec in records:
        code = rec["code"]
        src = Path(rec["_out_path"])
        dst = map2d_dir / f"{code}.webp"
        data = src.read_bytes()
        if hashlib.sha256(data).hexdigest() != rec["output"]["sha256"]:
            raise ExportError(f"{code}: canonical 字节与 manifest 记录不一致——先重跑导出")
        dst.write_bytes(data)
        maps_entries[code] = {"path": f"map2d/{code}.webp", "sha256": rec["output"]["sha256"],
                              "bytes": len(data), "width": CANON_DIM, "height": CANON_DIM,
                              "worldBounds": rec["worldBounds"], "orientation": rec["orientation"]}
    index = {
        "schemaVersion": INDEX_SCHEMA,
        "generated": now_iso,
        "recipe": {"canonDim": CANON_DIM, "quality": CANON_Q, "method": CANON_M, "gain": GAIN,
                   "softShoulder": SOFT_M, "saturation": SAT, "usm": list(USM)},
        "maps": maps_entries,
    }
    (map2d_dir / "index.json").write_text(json.dumps(index, ensure_ascii=False, indent=1), encoding="utf-8")
    return index


def write_covers(pack: Path, records: list[dict], now_iso: str) -> dict:
    """picker 封面索引：`cover/<key>.webp`（512²，与 canonical 同一调色管线采样）+ `cover/index.json`。

    schemaVersion 1：逐图 `worldBounds` 随件（与 map2d 同源同框），消费侧（地图预览 picker）
    按 key 取 `cover/<key>.webp`，sha256 校验字节。
    """
    cover_dir = pack / "cover"
    cover_dir.mkdir(parents=True, exist_ok=True)
    maps_entries = {}
    for rec in records:
        code = rec["code"]
        src = Path(rec["_cover_path"])
        data = src.read_bytes()
        if hashlib.sha256(data).hexdigest() != rec["cover"]["sha256"]:
            raise ExportError(f"{code}: cover 字节与 manifest 记录不一致")
        maps_entries[code] = {"path": f"cover/{code}.webp", "sha256": rec["cover"]["sha256"],
                              "bytes": rec["cover"]["bytes"], "width": rec["cover"]["width"],
                              "height": rec["cover"]["height"], "worldBounds": rec["worldBounds"]}
    index = {
        "schemaVersion": 1,
        "generated": now_iso,
        "recipe": {"coverDim": COVER_DIM, "coverQ": COVER_Q, "coverM": COVER_M},
        "maps": maps_entries,
    }
    (cover_dir / "index.json").write_text(json.dumps(index, ensure_ascii=False, indent=1), encoding="utf-8")
    return index


def maps_digest(maps: dict) -> str:
    """29 张的聚合指纹：对排序后的 `code\\tsha256` 行做 sha256（确定性、与文件顺序无关）。"""
    lines = "\n".join(f"{code}\t{maps[code]['sha256']}" for code in sorted(maps))
    return hashlib.sha256(lines.encode("utf-8")).hexdigest()


def write_source_stamp(out_dir: Path, records: list[dict], index: dict, index_sha256: str) -> Path:
    """交接给 WotbTools 的**内容元数据**：生成时刻 + 与 29 张 canonical 字节的确定性绑定。

    - `contentGeneratedAt`：内容的生成时刻（= 索引 `generated`），不是构建时间。运行时用它做
      "远端是否有更新"的单调比较（`index.generated <= contentGeneratedAt ⇒ 包内就是最新的`）。
      若换成 CI wall-clock：① Android stage 晚于 COS 发布时构建，会把真正更新的底图判成更旧而
      **永久拒绝升级**；② 同一 commit 重跑构建会产出不同字节的 bundle，破坏 stage 可重跑性。
    - `maps`（逐图 `file`/`sha256`）+ `mapsDigest`：把元数据**绑定到具体字节**。消费侧必须用
      随包/仓库里的真实字节重算并逐条比对（fail closed）——否则"换了图但忘了更新交接件"会让
      旧时间戳配上新图，运行时据索引判断时可能把包内更新的图 OTA 降级。
    """
    maps = {rec["code"]: {"file": rec["file"], "sha256": rec["output"]["sha256"]} for rec in records}
    payload = {"schemaVersion": 2, "contentGeneratedAt": index["generated"],
               "indexSha256": index_sha256, "mapsDigest": maps_digest(maps),
               "recipe": index["recipe"], "maps": maps}
    stamp = out_dir / SOURCE_NAME
    stamp.write_text(json.dumps(payload, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    return stamp


def cmd_export(args) -> int:
    pack, out = Path(args.pack), Path(args.out)
    rows = registry_all(pack)
    if args.map:
        rows = [r for r in rows if r["code"] == args.map]
        if not rows:
            raise ExportError(f"--map {args.map} 不在清单里")
    preflight_out(out, {r["file"] for r in rows})
    out.mkdir(parents=True, exist_ok=True)

    records = []
    for row in rows:
        data, cover_data, rec = export_one(pack, row)
        target = out / row["file"]
        target.write_bytes(data)
        rec["_out_path"] = str(target)
        # cover 直接写进 pack `cover/<key>.webp`（picker 封面，前端 /cover/<key>.webp 取用）
        cover_dir = pack / "cover"
        cover_dir.mkdir(parents=True, exist_ok=True)
        cover_path = cover_dir / f"{row['code']}.webp"
        cover_path.write_bytes(cover_data)
        rec["_cover_path"] = str(cover_path)
        records.append(rec)
        print(f"{row['code']:14s} {row['file']:24s} {len(data)/1024:6.0f} KB "
              f"(cover {len(cover_data)/1024:.0f} KB)  {rec['output']['sha256'][:12]}")

    total = sum(r["output"]["bytes"] for r in records)
    print(f"合计 {len(records)} 张 / {total/1024/1024:.2f} MiB")

    map2d_info = None
    if not args.skip_map2d:
        if args.map:
            raise ExportError("--map 单张模式不写 map2d/covers 索引（索引必须覆盖全部地图）；"
                              "要么去掉 --map，要么加 --skip-map2d 只出 staging 图")
        map2d_dir = Path(args.map2d_dir)
        index = write_map2d(map2d_dir, records, args.generated or now_iso())
        idx_bytes = (map2d_dir / "index.json").read_bytes()
        index_sha = hashlib.sha256(idx_bytes).hexdigest()
        stamp = write_source_stamp(Path(args.out), records, index, index_sha)
        map2d_info = {"dir": str(map2d_dir), "index": "map2d/index.json",
                      "index_sha256": index_sha,
                      "source_stamp": SOURCE_NAME,
                      "source_stamp_sha256": sha256_file(stamp),
                      "schemaVersion": INDEX_SCHEMA, "entries": len(index["maps"]),
                      "generated": index["generated"]}
        print(f"map2d → {map2d_dir}（{len(index['maps'])} 张 + index.json，generated {index['generated']}）")
        print(f"内容时间戳 → {stamp}（contentGeneratedAt = {index['generated']}）")
        cover_index = write_covers(pack, records, args.generated or now_iso())
        cover_info = {"dir": str(pack / "cover"), "index": "cover/index.json",
                      "entries": len(cover_index["maps"]), "generated": cover_index["generated"]}
        print(f"covers → {pack / 'cover'}（{len(cover_index['maps'])} 张 + index.json）")

    if args.manifest:
        man_path = Path(args.manifest)
        man_path.parent.mkdir(parents=True, exist_ok=True)
        pack_man = pack / "manifest.json"
        provenance = {}
        if pack_man.is_file():
            pm = json.loads(pack_man.read_text(encoding="utf-8"))
            provenance = {"upstream_commit": pm.get("upstream_commit"), "pack_generated": pm.get("generated")}
        man_path.write_text(json.dumps({
            "tool": "tools/export_2d_basemaps.py", "tool_version": TOOL_VERSION,
            "recipe": {"canonDim": CANON_DIM, "quality": CANON_Q, "method": CANON_M,
                       "gain": GAIN, "softShoulder": SOFT_M, "saturation": SAT, "usm": list(USM)},
            "provenance": provenance, "map2d": map2d_info, "covers": cover_info,
            "files": [{k: v for k, v in r.items() if k != "_out_path"} for r in records],
        }, ensure_ascii=False, indent=1), encoding="utf-8")
        print("manifest →", man_path)
    return 0


def cmd_verify(args) -> int:
    """只读：按 manifest 逐张重算并与磁盘字节比对（确定性 + 保真）。"""
    pack, out = Path(args.pack), Path(args.out)
    rows = {r["code"]: r for r in registry_all(pack)}
    man = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
    recipe = man["recipe"]
    frozen = {"canonDim": CANON_DIM, "quality": CANON_Q, "method": CANON_M, "gain": GAIN,
              "softShoulder": SOFT_M, "saturation": SAT, "usm": list(USM)}
    if recipe != frozen:
        raise ExportError(f"manifest 配方 {recipe} ≠ 工具常量 {frozen}——先确认用哪套配方")
    bad = []
    for rec in man["files"]:
        code = rec["code"]
        if code not in rows:
            bad.append(f"{code}: manifest 有但注册表没有")
            continue
        disk = out / rec["file"]
        if not disk.is_file():
            bad.append(f"{code}: 缺输出 {disk}")
            continue
        if sha256_file(disk) != rec["output"]["sha256"]:
            bad.append(f"{code}: 磁盘 sha256 与 manifest 不符")
            continue
        cover_disk = pack / "cover" / f"{code}.webp"
        data, cover_data, fresh = export_one(pack, rows[code])
        if fresh["output"]["sha256"] != rec["output"]["sha256"] or data != disk.read_bytes():
            bad.append(f"{code}: 重算结果与已发布字节不一致（配方或依赖版本变了？）")
            continue
        if not cover_disk.is_file() or sha256_file(cover_disk) != fresh["cover"]["sha256"]:
            bad.append(f"{code}: pack cover.webp 缺失或 sha256 不符")
    if not args.skip_map2d:
        bad += verify_map2d(Path(args.map2d_dir), out, rows, len(rows))
    if "covers" in man:
        cover_idx_path = pack / "cover" / "index.json"
        if not cover_idx_path.is_file():
            bad.append("pack cover/index.json 缺失（manifest 声明了 covers）")
        else:
            cidx = json.loads(cover_idx_path.read_text(encoding="utf-8"))
            for code, entry in cidx["maps"].items():
                disk = pack / entry["path"]
                if not disk.is_file():
                    bad.append(f"cover/{code}: 缺 {entry['path']}")
                elif sha256_file(disk) != entry["sha256"]:
                    bad.append(f"cover/{code}: sha256 与索引不符")
    if bad:
        raise ExportError("verify 失败：\n  - " + "\n  - ".join(bad))
    tail = "" if args.skip_map2d else " + map2d 索引/字节/canonical 三方对齐"
    print(f"verify ok：{len(man['files'])} 张逐张重算一致{tail}"
          f"（Pillow {Image.__version__ if hasattr(Image, '__version__') else 'n/a'}）")
    return 0


def verify_map2d(map2d_dir: Path, out: Path, rows: dict, expect: int) -> list[str]:
    """索引是 App 的唯一判据 ⇒ 索引、map2d 字节、canonical staging 三者必须逐件对齐。"""
    idx_path = map2d_dir / "index.json"
    if not idx_path.is_file():
        return [f"缺 map2d 索引 {idx_path}（先跑一次导出）"]
    idx = json.loads(idx_path.read_text(encoding="utf-8"))
    bad: list[str] = []
    if idx.get("schemaVersion") != INDEX_SCHEMA:
        bad.append(f"index schemaVersion {idx.get('schemaVersion')} ≠ {INDEX_SCHEMA}")
    recipe = {k: idx.get("recipe", {}).get(k) for k in
              ("canonDim", "quality", "method", "gain", "softShoulder", "saturation", "usm")}
    if recipe["usm"] is not None:
        recipe["usm"] = list(recipe["usm"])
    if recipe != {"canonDim": CANON_DIM, "quality": CANON_Q, "method": CANON_M, "gain": GAIN,
                  "softShoulder": SOFT_M, "saturation": SAT, "usm": list(USM)}:
        bad.append(f"index.recipe {recipe} 与工具常量不符")
    stamp_path = out / SOURCE_NAME
    if not stamp_path.is_file():
        bad.append(f"缺内容元数据交接件 {stamp_path}（WotbTools 构建会 fail closed）")
    else:
        stamp = json.loads(stamp_path.read_text(encoding="utf-8"))
        if stamp.get("schemaVersion") != 2:
            bad.append(f"{SOURCE_NAME} schemaVersion {stamp.get('schemaVersion')} ≠ 2")
        if stamp.get("contentGeneratedAt") != idx.get("generated"):
            bad.append(f"{SOURCE_NAME} contentGeneratedAt {stamp.get('contentGeneratedAt')!r} ≠ 索引 generated {idx.get('generated')!r}")
        if stamp.get("indexSha256") != sha256_file(idx_path):
            bad.append(f"{SOURCE_NAME} indexSha256 与 map2d/index.json 实际哈希不符")
        # **字节绑定**：交接件里的逐图 sha256 必须等于 canonical staging 的真实字节，
        # 且与 map2d 索引逐条一致——三者（索引/交接件/canonical）同源才算通过。
        stamp_maps = stamp.get("maps") if isinstance(stamp.get("maps"), dict) else {}
        idx_maps = idx.get("maps") if isinstance(idx.get("maps"), dict) else {}
        for code in sorted(set(stamp_maps) | set(idx_maps)):
            entry = stamp_maps.get(code)
            if not isinstance(entry, dict) or not entry.get("sha256") or not entry.get("file"):
                bad.append(f"{SOURCE_NAME} 缺 {code} 的绑定（file/sha256）")
                continue
            canonical = out / entry["file"]
            if not canonical.is_file():
                bad.append(f"{code}: 交接件指向的 canonical 文件不存在：{entry['file']}")
                continue
            actual = sha256_file(canonical)
            if entry["sha256"] != actual:
                bad.append(f"{code}: 交接件 sha256 与 canonical 字节不符（换图未更新交接件？）")
            if (idx_maps.get(code) or {}).get("sha256") != actual:
                bad.append(f"{code}: 交接件/index 与 canonical 三方的 sha256 不一致")
        if stamp.get("mapsDigest") != maps_digest(stamp_maps):
            bad.append(f"{SOURCE_NAME} mapsDigest 与 maps 条目不符")
    maps = idx.get("maps") or {}
    if len(maps) != expect:
        bad.append(f"index 条目数 {len(maps)} ≠ 注册表 {expect}")
    for code, entry in maps.items():
        if code not in rows:
            bad.append(f"{code}: index 有但注册表没有")
            continue
        p = map2d_dir / f"{code}.webp"
        if not p.is_file():
            bad.append(f"{code}: 缺 map2d 图")
            continue
        if sha256_file(p) != entry.get("sha256") or p.stat().st_size != entry.get("bytes"):
            bad.append(f"{code}: map2d 字节与 index 记录不符")
        # 几何契约（schemaVersion 2）：世界框/取向随件落盘，且与**真实像素**一致。
        # 只校验 sha256/尺寸是校不出"裁剪/补边/换框"的（尺寸可以不变）——这里补齐。
        if entry.get("orientation") != "top=+Z,left=+X":
            bad.append(f"{code}: index orientation 缺失/不符（{entry.get('orientation')!r}）")
        wb = entry.get("worldBounds") if isinstance(entry.get("worldBounds"), dict) else {}
        if any(abs(float(wb.get(k, 1e9)) - v) > 1e-6 for k, v in
               (("xMin", -300.0), ("xMax", 300.0), ("yMin", -300.0), ("yMax", 300.0))):
            bad.append(f"{code}: index worldBounds 不是 ±300（{wb or '缺'}）")
        try:
            size = Image.open(p).size
        except Exception as exc:                                   # noqa: BLE001 —— 只报错不改判
            bad.append(f"{code}: 读不出像素尺寸（{exc}）")
        else:
            if size[0] != size[1]:
                bad.append(f"{code}: 像素 {size} 非方形（与 ±300 方框不同比 ⇒ 各向异性拉伸）")
            if size != (entry.get("width"), entry.get("height")):
                bad.append(f"{code}: 像素 {size} ≠ index {entry.get('width')}×{entry.get('height')}")
        canonical = out / rows[code]["file"]
        if not canonical.is_file():
            bad.append(f"{code}: 缺 canonical staging {canonical}")
        elif canonical.read_bytes() != p.read_bytes():
            bad.append(f"{code}: map2d 与 canonical staging 不同字节")
    return bad


def self_test() -> None:
    # 1) LUT：单调、端点、软肩不越界
    lut = build_lut()
    assert np.all(np.diff(lut) >= -1e-9), "LUT 非单调"
    assert lut[0] == 0.0 and lut[-1] == 255.0
    assert 100 < lut[128] < 160, f"中点映射异常：{lut[128]}"
    # 2) 对比曲线确实放大中段（相对恒等）
    assert lut[192] - lut[64] > 192 - 64, "曲线没有放大中段对比"
    # 3) 软肩：越界不产生硬裁切
    v = np.array([-40.0, -1.0, 0.0, 128.0, 255.0, 300.0, 400.0])
    sc = soft_clip(v)
    assert sc.min() > -1e-6 and sc.max() < 255.0, f"软肩越界：{sc}"
    assert np.all(np.diff(sc) >= -1e-9)
    # 4) grade：灰度渐变保持单调、色相不动、无贴顶
    grad = np.repeat(np.arange(256, dtype=np.float32).reshape(1, 256, 1), 3, axis=2)
    grad = np.repeat(grad, 8, axis=0)
    out = grade(grad)
    assert out.max() < 255.0 and out.min() > 0.0, "grade 产生硬裁切"
    row = out[0, :, 0]
    assert np.all(np.diff(row) >= -1e-4), "grade 破坏单调"
    # 5) 彩色像素：软肩可能压缩某通道，但不得反转通道次序（= 不得改变色相）
    warm = np.array([[[200.0, 60.0, 40.0]]], dtype=np.float32)
    cool = np.array([[[40.0, 90.0, 200.0]]], dtype=np.float32)
    gw, gc = grade(warm)[0, 0], grade(cool)[0, 0]
    assert gw[0] > gw[1] > gw[2], f"暖色通道次序被破坏：{gw}"
    assert gc[2] > gc[1] > gc[0], f"冷色通道次序被破坏：{gc}"
    # 6) 清单枚举（pack index 缺失时跳过）
    try:
        rows = registry_all(REPO / "release/asset_pack")
        assert len(rows) >= 20 and all(r["code"] and r["file"].endswith(".webp") for r in rows)
        print(f"self-test: ok（清单 {len(rows)} 张）")
    except ExportError as e:
        print(f"self-test: ok（跳过清单检查：{e}）")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pack", default="release/asset_pack")
    ap.add_argument("--out", default="release/2d-basemaps")
    ap.add_argument("--manifest", default="release/2d-basemaps-manifest.json")
    ap.add_argument("--map2d-dir", default="release/asset_pack/map2d",
                    help="COS 免发版通道目录：map2d/<code>.webp + index.json（包内，随上传器发布）")
    ap.add_argument("--skip-map2d", action="store_true", help="只出 canonical staging，不写 map2d 索引")
    ap.add_argument("--generated", help="覆盖 index.json 的 generated 时间戳（ISO-8601Z；复现/测试用）")
    ap.add_argument("--map", help="只导出单张（地图 key，如 canal）")
    ap.add_argument("--wotbtools", help="WotbTools 检出根目录（默认 ../WotbTools 或 WOTBTOOLS_DIR）")
    ap.add_argument("--verify", action="store_true", help="只读校验：按 manifest 逐张重算比对")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    try:
        if args.self_test:
            self_test()
        elif args.verify:
            return cmd_verify(args)
        else:
            return cmd_export(args)
    except ExportError as e:
        print(f"2d-basemaps: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
