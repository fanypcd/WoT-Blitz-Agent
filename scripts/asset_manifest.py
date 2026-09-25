#!/usr/bin/env python3
"""生成移动端打包资产清单 mobile_assets/manifest.json，并打印人读摘要。

用途：
1. 盘点移动端 App 将随包/下载的全部资产现状（pb 数据、图鉴图标、vendor、底图、GLB）；
2. 跟踪 GLB 全量拉取进度（另一会话正在执行）：已缓存/缺失清单、总体积，
   为「打进安装包 vs 首启下载」的决策提供体积依据；
3. 后续 Tauri 打包流水线以本清单为资产齐备性检查（打包前跑一次，缺失即报错）。

零依赖（仅标准库）、只读扫描，不改任何文件。

用法：python scripts/asset_manifest.py [--json OUT]
"""

import argparse
import json
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_JSON = PROJECT_ROOT / "mobile_assets" / "manifest.json"


def dir_stats(p: Path, pattern: str = "*"):
    files = [f for f in p.rglob(pattern) if f.is_file()] if p.is_dir() else []
    return {"path": str(p), "files": len(files),
            "bytes": sum(f.stat().st_size for f in files)}


def file_info(p: Path):
    return {"path": str(p), "exists": p.is_file(),
            "bytes": p.stat().st_size if p.is_file() else 0}


def scan() -> dict:
    root = PROJECT_ROOT
    data = root / "data"
    report = {}

    # ---- 核心 pb/JSON 数据 ----
    report["core_data"] = {
        "tanks.pb": file_info(data / "tanks.pb"),
        "models.pb": file_info(data / "models.pb"),
        "tank_cache.json": file_info(data / "tank_cache.json"),
        "gun_angles.json": file_info(data / "gun_angles.json"),
        "data_version.json": file_info(data / "data_version.json"),
    }

    # ---- game_data / tank_images / vendor ----
    report["game_data"] = dir_stats(data / "game_data", "*.json")
    report["tank_images"] = dir_stats(root / "tank_images", "*.webp")
    report["vendor_three"] = dir_stats(root / "web" / "vendor" / "three")

    # ---- 地图底图（export_mobile_maps.py 产物优先，退化看桌面缓存）----
    maps_dir = root / "mobile_assets" / "maps"
    if (maps_dir / "maps_manifest.json").is_file():
        m = json.loads((maps_dir / "maps_manifest.json").read_text(encoding="utf-8"))
        report["maps"] = {"source": "mobile_assets/maps",
                          "count": m["summary"]["exported"],
                          "expected": m["summary"]["total"],
                          "bytes": m["summary"]["total_bytes"],
                          "failed": m["summary"]["failed"]}
    else:
        st = dir_stats(data / "maps" / "_cache", "*.webp")
        report["maps"] = {"source": "data/maps/_cache（未执行 export_mobile_maps.py）",
                          "count": st["files"], "expected": 26, "bytes": st["bytes"],
                          "failed": []}

    # ---- GLB 全量进度 ----
    glb_cache = root / "glb_cache"
    expected_ids = []
    tc = data / "tank_cache.json"
    if tc.is_file():
        expected_ids = sorted(json.loads(tc.read_text(encoding="utf-8")).keys(), key=int)

    have_model, have_collision, total_bytes, missing = 0, 0, 0, []
    if glb_cache.is_dir():
        for tid in expected_ids:
            d = glb_cache / tid
            mglb, cglb = d / "model.glb", d / "collision.glb"
            # 另一会话可能正在并发下载/替换，size 用 try 兜底（读到即算，读不到重试一次）
            def size_or(p: Path):
                for _ in range(2):
                    try:
                        return p.stat().st_size if p.is_file() else 0
                    except OSError:
                        continue
                return 0
            m_sz, c_sz = size_or(mglb), size_or(cglb)
            m_ok, c_ok = m_sz > 0, c_sz > 0
            have_model += m_ok
            have_collision += c_ok
            total_bytes += m_sz + c_sz
            if not m_ok:
                missing.append(tid)

    # 缓存目录里可能还有 expected 之外的 id（已下架车等），一并统计
    extra = 0
    if glb_cache.is_dir():
        extra = sum(1 for d in glb_cache.iterdir()
                    if d.is_dir() and d.name not in set(expected_ids)
                    and (d / "model.glb").is_file())

    report["glb"] = {
        "expected_tanks": len(expected_ids),
        "have_model_glb": have_model,
        "have_collision_glb": have_collision,
        "bytes": total_bytes,
        "missing_count": len(missing),
        "missing_sample": missing[:20],
        "extra_cached_ids": extra,
    }
    return report


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", type=Path, default=DEFAULT_JSON)
    args = ap.parse_args()

    r = scan()
    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(r, ensure_ascii=False, indent=2), encoding="utf-8")

    g = r["glb"]
    mb = lambda b: f"{b/1024/1024:.1f}MB"
    print("=" * 62)
    print(" 移动端打包资产清单")
    print("=" * 62)
    for k in ("tanks.pb", "models.pb", "tank_cache.json"):
        info = r["core_data"][k]
        print(f"  {k:<18} {'OK' if info['exists'] else '缺失!':<4} {mb(info['bytes'])}")
    print(f"  {'game_data/':<18} {r['game_data']['files']} 个文件  {mb(r['game_data']['bytes'])}")
    print(f"  {'tank_images/':<18} {r['tank_images']['files']} 个图标  {mb(r['tank_images']['bytes'])}")
    print(f"  {'web/vendor/three':<18} {r['vendor_three']['files']} 个文件  {mb(r['vendor_three']['bytes'])}")
    m = r["maps"]
    print(f"  {'地图底图':<15} {m['count']}/{m['expected']} 张  {mb(m['bytes'])}  [{m['source']}]")
    print("-" * 62)
    pct = g["have_model_glb"] / max(g["expected_tanks"], 1) * 100
    bar = "█" * int(pct // 5) + "░" * (20 - int(pct // 5))
    print(f"  GLB 全量进度  {g['have_model_glb']}/{g['expected_tanks']}  {pct:.0f}%  {bar}")
    print(f"  已缓存体积    {mb(g['bytes'])}（collision.glb 齐备 {g['have_collision_glb']} 辆）")
    if g["missing_count"]:
        print(f"  缺失 {g['missing_count']} 辆，示例: {', '.join(g['missing_sample'][:8])}")
    else:
        print("  缺失 0 辆 —— 全量齐备，可打包")
    print("-" * 62)
    print(f"  清单已写入 {args.json}")
    if r["maps"]["count"] < r["maps"]["expected"]:
        print("  [提示] 底图不全，运行: python scripts/export_mobile_maps.py")


if __name__ == "__main__":
    sys.exit(main())
