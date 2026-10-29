"""逐车地面履带印参数导出（客户端 `treadParameters` → `tank_tread/<tank_id>.json` + 贴图解码）。

客户端 TreadSystem（车开过后地面上的暗色碾压痕）的作者数据有三处、全部在可得数据面：
- `Parameters/<nation>/<stem>.yaml` 的 `treadParameters:` 块（5 字段，88/764 辆有）：
  `forceManualWidth/manualWidth`（印记条带宽，m）、`forceManualSpace/manualSpace`（沿路
  前进多少米铺一段）、`texturePath`（`~res:/3d/Tanks/Common/images/<Nation>_NN_tread.tex`）。
- 贴图本体：`3d/Tanks/Common/images/<stem>.dx11.dds.dvpl`（DDS BC1/2/3/5，256²），
  经 export_tank_glb.decode_dds 解码后转 webp（**无损**，保客户端像素）。
- 本导出器**只搬数据不做语义推断**：force*=false 时客户端如何派生宽度/间距未逆向，
  照原样透出 flag + authored 值；无 treadParameters 块的车**不产文件**（数据面口径见
  docs/data-inventory.md 履带印行，fail-closed）。

贴图导出与逐车参数解耦：对 `3d/Tanks/Common/images/*tread*.dx11.dds.dvpl` 全量解码
（含未被任何 yaml 引用的 `Tread`/`Japan_01` 等回落候选），幂等可重跑。

用法：
    python tools/export_tank_tread.py                 # 全表（tanks.pb）+ 全部贴图
    python tools/export_tank_tread.py --tank 7169
    python tools/export_tank_tread.py --textures      # 只重导贴图
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys

_HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))
sys.path.insert(0, str(_HERE / "wotbtools"))

from dlc_packs import client_path  # noqa: E402
from export_tank_glb import decode_dds, default_game_data, read_tank_table  # noqa: E402
from export_tank_suspension import SuspensionParseError, _b as _sus_bool  # noqa: E402
from export_tank_suspension import _section  # noqa: E402  (行式解析共用件)
from wotb_sc2 import decode_dvpl  # noqa: E402

ROUND = 6                     # yaml 字面量 6 位小数，round-trip 无损
IMAGES_REL = "3d/Tanks/Common/images"
TEX_FIELDS = ("force_manual_width", "manual_width", "force_manual_space", "manual_space")

# 无 treadParameters 块的车辆（424/735）的**国别主贴图**回落（2026-10-11 用户报
# "215b 比 Kranvagn 黑"：无块车此前回落浅色通用张 Tread.webp（α 64.4、可见亮度 82.7
# 的浅棕）+ 固定宽 0.6，与有块车的深色国别张（α ~100-128、亮度 30）墨量差约 3 倍；
# 客户端对无块车的行为未逆向，国别主贴图是"同族深色"的最佳可得代理，属产品近似）。
# 注：authored 面的贴图是**逐车（底盘来源）**而非按国别（普查：uk 国亦有 USSR_01 等），
# 但无块车无从取底盘来源 ⇒ 按国别回落。
NATION_DEFAULT_TEXTURE = {
    "ussr": "USSR_01_tread", "germany": "German_01_tread", "usa": "USA_01_tread",
    "uk": "GB_01_tread", "china": "China_01_tread", "japan": "Japan_01_tread",
    "france": "France_01_tread", "european": "European_01_tread", "other": "Other_01_tread",
}
# alpha 归一化（2026-10-11 用户裁示"统一颜色深浅"）：13 张 authored 贴图的整图 alpha
# 均值极差 4.4×（USSR 127.6 → STT 28.8；RGB/色调本已一致，可见亮度全 ≈30），
# 按中位数为目标缩放每张的 alpha 权重，前端以 per-texture 系数乘入印记不透明度。
# 钳位 [0.65, 1.8]：避免近全透明张（STT 0.40 覆盖）被推到失真。
ALPHA_SCALE_CLAMP = (0.65, 1.8)


class TreadParseError(RuntimeError):
    """结构不符合已知契约（fail-closed：跳过该车，不造缺省值）。"""


def _b(v) -> bool:
    """布尔解析：复用 suspension 的行式解析件，异常统一成本模块的 TreadParseError。"""
    try:
        return _sus_bool(v)
    except SuspensionParseError as e:
        raise TreadParseError(str(e)) from e


def _f(v) -> float:
    try:
        return round(float(v), ROUND)
    except (TypeError, ValueError) as e:
        raise TreadParseError(f"浮点解析失败: {v!r}") from e


def parse_tread_parameters(txt: str) -> dict | None:
    """Parameters yaml 文本 → treadParameters 块（无段 → None）。

    88/764 辆的块字段形状完全一致（5 字段全带）；缺 texturePath 或数值坏 → fail-closed。
    """
    try:
        blk = _section(txt.splitlines(), "treadParameters")
    except SuspensionParseError as e:
        # 形如 `treadParameters: {}` 的内联值段：同为契约外形状，fail-closed
        raise TreadParseError(str(e)) from e
    if blk is None:
        return None
    if "texturePath" not in blk:
        raise TreadParseError("缺 texturePath")
    return {
        "texture_path": str(blk["texturePath"]).strip(),
        "force_manual_width": _b(blk["forceManualWidth"]),
        "manual_width": _f(blk["manualWidth"]),
        "force_manual_space": _b(blk["forceManualSpace"]),
        "manual_space": _f(blk["manualSpace"]),
    }


def texture_stem(texture_path: str) -> str:
    """`~res:/3d/Tanks/Common/images/USSR_01_tread.tex` → `USSR_01_tread`。

    客户端 `.tex` 在数据面的实体是 `<stem>.dx11.dds.dvpl`；已普查 88/88 全是
    `~res:/3d/Tanks/Common/images/<stem>.tex` 这一形状，其它一律拒绝（fail-closed，
    防止 stem 与贴图导出目录脱钩）。
    """
    p = texture_path.strip()
    prefix = "~res:/" + IMAGES_REL + "/"
    if not p.startswith(prefix) or not p.endswith(".tex"):
        raise TreadParseError(f"texturePath 形状不符: {texture_path!r}")
    stem = p[len(prefix):][:-len(".tex")]
    if not stem or "/" in stem:
        raise TreadParseError(f"texturePath 形状不符: {texture_path!r}")
    return stem


def export_textures(game_data: pathlib.Path, out_dir: pathlib.Path) -> dict:
    """`images/*tread*.dx11.dds.dvpl` 全量解码 → `<stem>.webp`（无损）+ `textures.json`。

    `textures.json` = per-stem `{alpha_mean, scale}`：scale = 目标(全库中位)/该张 alpha_mean
    （钳位见 ALPHA_SCALE_CLAMP），前端乘入印记不透明度以**统一跨贴图的深浅**（用户裁示）。
    幂等可重跑。"""
    import statistics

    from PIL import Image
    src = client_path(game_data, IMAGES_REL)
    out_dir.mkdir(parents=True, exist_ok=True)
    n, total = 0, 0
    entries: dict[str, dict] = {}
    arrs: list[tuple[str, "np.ndarray"]] = []
    for f in sorted(src.glob("*tread*.dx11.dds.dvpl")):
        stem = f.name[: -len(".dx11.dds.dvpl")]
        arr, _tag = decode_dds(decode_dvpl(f.read_bytes()))
        Image.fromarray(arr.astype("uint8")).save(out_dir / f"{stem}.webp", lossless=True)
        arrs.append((stem, arr))
        n += 1
        total += (out_dir / f"{stem}.webp").stat().st_size
    alpha_means = {stem: float(arr[..., 3].astype("float64").mean()) for stem, arr in arrs}
    target = statistics.median(alpha_means.values()) if alpha_means else 0.0
    lo, hi = ALPHA_SCALE_CLAMP
    for stem, m in alpha_means.items():
        scale = 1.0 if m <= 1e-6 else min(hi, max(lo, target / m))
        entries[stem] = {"alpha_mean": round(m, 2), "scale": round(scale, 4)}
    meta = {"target_alpha_mean": round(target, 2), "clamp": list(ALPHA_SCALE_CLAMP),
            "textures": {k: entries[k] for k in sorted(entries)}}
    (out_dir / "textures.json").write_text(
        json.dumps(meta, ensure_ascii=False, sort_keys=True, separators=(",", ":")),
        encoding="utf-8")
    total += (out_dir / "textures.json").stat().st_size
    return {"count": n, "bytes": total, "target": meta["target_alpha_mean"]}


def export_one(game_data: pathlib.Path, tank_id: int, nation: str, stem: str,
               out_dir: pathlib.Path) -> dict:
    """有块 → authored json；无块/解析失败 → **国别主贴图回落** json（2026-10-11 起
    全部 735 辆都产文件：统一深浅的前提是每辆车都有同族深色贴图，此前无块车回落通用
    浅色张导致"215b 比 Kranvagn 黑"）。回落车宽度置 `null`（无 authored 宽度；客户端
    forceManualWidth=false 的语义务是"从履带几何现算"，前端从模型履带盒实测）。"""
    st: dict[str, object] = {"tank_id": tank_id, "stem": stem, "nation": nation}
    rel = f"3d/Tanks/Parameters/{nation}/{stem}.yaml.dvpl"
    p = client_path(game_data, rel)
    if p.exists():
        txt = decode_dvpl(p.read_bytes()).decode("utf-8", "replace")
        try:
            tread = parse_tread_parameters(txt)
        except TreadParseError as e:
            tread = None
            st["note"] = f"parse_error: {e}"
        if tread is not None:
            doc = {"tank_id": tank_id, "nation": nation, "stem": stem, "source": rel,
                   "fallback": False,
                   "texture": f"tread/textures/{texture_stem(tread['texture_path'])}.webp",
                   **{k: tread[k] for k in TEX_FIELDS}}
            (out_dir / f"{tank_id}.json").write_text(
                json.dumps(doc, ensure_ascii=False, sort_keys=True, separators=(",", ":")),
                encoding="utf-8")
            st["width"] = tread["manual_width"]
            st["authored"] = True
            return st
    # 回落：国别主贴图 + width=null（前端从模型履带盒实测）+ space 1.0
    dflt = NATION_DEFAULT_TEXTURE.get(nation)
    if dflt is None:
        st["skip"] = f"no_nation_default: {nation}"
        return st
    doc = {"tank_id": tank_id, "nation": nation, "stem": stem, "source": None,
           "fallback": True,
           "texture": f"tread/textures/{dflt}.webp",
           "manual_width": None, "manual_space": 1.0}
    (out_dir / f"{tank_id}.json").write_text(
        json.dumps(doc, ensure_ascii=False, sort_keys=True, separators=(",", ":")),
        encoding="utf-8")
    st["width"] = None
    st["authored"] = False
    return st


def main() -> int:
    ap = argparse.ArgumentParser(description="逐车履带印参数导出（客户端 yaml + 贴图 → 资产包）")
    ap.add_argument("--game-data", type=pathlib.Path, default=None)
    ap.add_argument("--tanks-pb", type=pathlib.Path,
                    default=_HERE.parent / "data" / "tanks.pb")
    ap.add_argument("--out", type=pathlib.Path,
                    default=_HERE.parent / "data" / "tank_tread")
    ap.add_argument("--textures-out", type=pathlib.Path,
                    default=_HERE.parent / "data" / "tank_tread_textures")
    ap.add_argument("--tank", type=int, action="append", default=[],
                    help="tank_id（可重复）；缺省 = 全表")
    ap.add_argument("--textures", action="store_true", help="只导贴图")
    args = ap.parse_args()

    game_data = args.game_data or default_game_data()
    if not game_data.is_dir():
        print(f"!! 客户端 Data 目录不存在: {game_data}（用 --game-data 指定）", file=sys.stderr)
        return 2

    tex = export_textures(game_data, args.textures_out)
    print(f"贴图 {tex['count']} 张 → {args.textures_out}（{tex['bytes']/1024:.0f} KB, webp 无损；"
          f"alpha 归一化目标 {tex['target']:.1f}）")
    if args.textures:
        return 0

    table = read_tank_table(args.tanks_pb)
    ids = sorted(args.tank) if args.tank else sorted(table)
    args.out.mkdir(parents=True, exist_ok=True)
    ok = authored = 0
    skipped: dict[str, int] = {}
    widths: list[float] = []
    for tid in ids:
        info = table.get(tid)
        if not info:
            continue
        st = export_one(game_data, tid, info["nation"], info["stem"], args.out)
        if "skip" in st:
            key = str(st["skip"]).split(":")[0]
            skipped[key] = skipped.get(key, 0) + 1
            continue
        ok += 1
        if st.get("authored"):
            authored += 1
            widths.append(float(st["width"]))  # type: ignore[arg-type]
        if st.get("note"):
            skipped[str(st["note"]).split(":")[0]] = skipped.get(str(st["note"]).split(":")[0], 0) + 1
    total = sum(f.stat().st_size for f in args.out.glob("*.json"))
    print(f"导出 {ok} 辆（authored {authored} / 国别回落 {ok - authored}）→ {args.out}（{total/1024:.0f} KB）")
    if widths:
        print(f"  authored manualWidth {min(widths):.3f}..{max(widths):.3f} m")
    if skipped:
        print("  备注: " + ", ".join(f"{k}×{v}" for k, v in sorted(skipped.items())))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
