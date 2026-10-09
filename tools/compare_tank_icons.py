#!/usr/bin/env python3
"""对照 BlitzKit 与客户端导出的坦克封面图（`tank_images` vs `local_tank_icons`）。

`export_tank_icons.py` 的 docstring 一直引用本工具，但此前**并未入库**——封面图因此
没有像 GLB 那样（`compare_tank_glb.py`）的对照手段，32 辆"缺图"长期没人能一眼分清
是**客户端真没有**还是**匹配规则没覆盖**。本工具补上这一环。

两条对照通道：
  1. **覆盖**（默认）——逐辆列出两侧有没有图，分出「两侧都有 / 只有 BlitzKit /
     只有客户端 / 两侧都缺」四类，并把导出器记录的 `src`（尤其是走了人工别名表的那些）
     一并列出，便于复核别名挂得对不对。
  2. **尺寸**（`--audit`）——两侧都有的车，比对像素尺寸、宽高比与字节数。两侧为**同源同画**
     （2026-10-10 复核：1:1 像素、左上角对齐；BK 画布是裁剪后的较小画布），尺寸差异来自
     画布裁剪、残留为亚像素/压缩级，均属预期，故只做**报告**不做等价判定；`--render` 可并排
     导出看得更直观。

用法：
    python tools/compare_tank_icons.py --all                 # 覆盖对照
    python tools/compare_tank_icons.py --all --audit         # 覆盖 + 尺寸报告
    python tools/compare_tank_icons.py --tank 10753 --render # 并排导出对比图
    python tools/compare_tank_icons.py --all --strict        # 任一缺口即非零退出（CI 门禁用）

口径提醒：`local_tank_icons` 是**对照产物、不接入运行期**（见 export_tank_icons.py 头部），
所以本工具只报差异，不主张谁对谁错；BlitzKit 那份才是当前分发使用的那份。
"""

import argparse
import pathlib
import sys
import json

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent


def read_status(status_path: pathlib.Path) -> dict:
    """导出器写的 `_export_status.json`：{tank_id: 记录}。缺失时返回空表。"""
    if not status_path.is_file():
        return {}
    try:
        d = json.loads(status_path.read_text(encoding="utf-8"))
    except Exception:
        return {}
    return {r["tank_id"]: r for r in d.get("results", []) if isinstance(r, dict) and "tank_id" in r}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--tank", action="append", default=[])
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--pb", type=pathlib.Path, default=REPO_ROOT / "data" / "tanks.pb")
    ap.add_argument("--blitzkit-dir", type=pathlib.Path,
                    default=REPO_ROOT / "data/cache/tank_images")
    ap.add_argument("--local-dir", type=pathlib.Path,
                    default=REPO_ROOT / "data/cache/local_tank_icons")
    ap.add_argument("--out-dir", type=pathlib.Path,
                    default=REPO_ROOT / "data/cache/local_compare")
    ap.add_argument("--audit", action="store_true", help="两侧都有的车比对尺寸/宽高比/字节")
    ap.add_argument("--render", action="store_true", help="并排导出对比图到 --out-dir")
    ap.add_argument("--strict", action="store_true", help="存在缺口时非零退出")
    args = ap.parse_args()

    sys.path.insert(0, str(REPO_ROOT / "tools"))
    import export_tank_icons as E

    table = E.read_tank_table(args.pb)
    status = read_status(args.local_dir / "_export_status.json")

    if args.tank:
        targets = [(int(x), table[int(x)]["stem"]) for x in args.tank]
    elif args.all:
        targets = sorted((tid, v["stem"]) for tid, v in table.items())
    else:
        print("!! 需要 --tank <id> 或 --all", file=sys.stderr)
        return 2

    def bk_path(tid: int) -> pathlib.Path:
        return args.blitzkit_dir / f"{tid}.webp"

    def lc_path(tid: int) -> pathlib.Path:
        return args.local_dir / f"{tid}.webp"

    both, bk_only, lc_only, neither = [], [], [], []
    aliased = []
    for tid, stem in targets:
        b, l = bk_path(tid).is_file(), lc_path(tid).is_file()
        if b and l:
            both.append((tid, stem))
            src = (status.get(tid) or {}).get("src")
            if src and stem in E.ICON_ALIAS:
                aliased.append((tid, stem, src))
        elif b:
            bk_only.append((tid, stem))
        elif l:
            lc_only.append((tid, stem))
        else:
            neither.append((tid, stem))

    print(f"=== 封面图覆盖对照（{len(targets)} 辆）===")
    print(f"  两侧都有            : {len(both)}")
    print(f"  只有 BlitzKit       : {len(bk_only)}   ← 客户端解包缺（迁移时是缺口）")
    print(f"  只有客户端          : {len(lc_only)}   ← 客户端可补")
    print(f"  两侧都缺            : {len(neither)}")
    if aliased:
        print(f"\n  走人工别名表命中的 {len(aliased)} 辆（请复核是否挂对）：")
        for tid, stem, src in aliased:
            print(f"    {tid:>7} {stem:34} -> {src}")
    for label, rows in (("只有 BlitzKit（客户端没有）", bk_only),
                        ("只有客户端", lc_only),
                        ("两侧都缺", neither)):
        if rows:
            print(f"\n  --- {label} ---")
            for tid, stem in rows:
                note = (status.get(tid) or {}).get("status", "")
                print(f"    {tid:>7} {stem:34} {note}")

    if args.audit and both:
        from PIL import Image
        print("\n=== 尺寸对照（两侧都有的车）===")
        diff_size, diff_ratio = [], []
        for tid, stem in both:
            try:
                with Image.open(bk_path(tid)) as a, Image.open(lc_path(tid)) as c:
                    aw, ah, cw, ch = a.width, a.height, c.width, c.height
            except Exception as e:
                print(f"    !! {tid} 读取失败: {e}")
                continue
            if (aw, ah) != (cw, ch):
                diff_size.append((tid, stem, f"{aw}x{ah}", f"{cw}x{ch}"))
            ra, rc = aw / ah if ah else 0, cw / ch if ch else 0
            if abs(ra - rc) > 0.01:
                diff_ratio.append((tid, stem, round(ra, 3), round(rc, 3)))
        print(f"  尺寸不同的: {len(diff_size)} / {len(both)}（两侧来源不同，属预期）")
        for tid, stem, a, c in diff_size[:15]:
            print(f"    {tid:>7} {stem:34} blitzkit={a:10} client={c}")
        if len(diff_size) > 15:
            print(f"    … 另 {len(diff_size) - 15} 辆")
        print(f"  宽高比不同的: {len(diff_ratio)}")
        for tid, stem, a, c in diff_ratio[:10]:
            print(f"    {tid:>7} {stem:34} blitzkit={a} client={c}")

    if args.render and targets:
        from PIL import Image
        args.out_dir.mkdir(parents=True, exist_ok=True)
        n = 0
        for tid, stem in targets:
            b, l = bk_path(tid), lc_path(tid)
            if not (b.is_file() and l.is_file()):
                continue
            with Image.open(b) as ai, Image.open(l) as ci:
                a, c = ai.convert("RGBA"), ci.convert("RGBA")
                h = 256
                a = a.resize((max(1, int(a.width * h / a.height)), h))
                c = c.resize((max(1, int(c.width * h / c.height)), h))
                out = Image.new("RGBA", (a.width + c.width + 8, h), (24, 24, 28, 255))
                out.paste(a, (0, 0)); out.paste(c, (a.width + 8, 0))
                dest = args.out_dir / f"icon_{tid}.png"
                out.save(dest)
                n += 1
        print(f"\n并排对比图已导出 {n} 张 → {args.out_dir}（左 BlitzKit / 右 客户端）")

    gap = len(bk_only) + len(neither)
    if args.strict and gap:
        print(f"\n!! 存在 {gap} 辆客户端侧缺口（--strict）", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
