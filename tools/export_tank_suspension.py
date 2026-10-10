"""逐车悬挂参数导出（客户端 `Parameters/<nation>/<stem>.yaml` → `tank_suspension/<tank_id>.json`）。

**只搬数据，不做单位换算与语义推断**：字段语义与未定项照原样透出，判据见
`docs/tank-suspension-client-re.md`（`{flag, a, b}` = 负重轮标记 + 上行/下行行程；
链折线 = 履带静止中心线，每点 `{x 沿车长, y 高, flag}`，两代键式都读）。
消费方（3D 回放）按该文档 §七 的分级路径使用；无 suspension 块的车 **不产文件**
（前端回落整台刚体，fail-closed）。

数据源经 `client_path()` 解析——DLC 覆盖层（`%LOCALAPPDATA%\\wotblitz\\packs`）优先于 `Data/`。
客户端 yaml 是机器生成、缩进固定，故与其它导出器一致走**行式解析**（工具链无 YAML 依赖）。

用法：
    python tools/export_tank_suspension.py                 # 全表（tanks.pb）
    python tools/export_tank_suspension.py --tank 7169 --tank 9489
    python tools/export_tank_suspension.py --list          # 只列 tank_id → 模型名
"""
from __future__ import annotations

import argparse
import base64
import json
import pathlib
import struct
import sys

_HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(_HERE))
sys.path.insert(0, str(_HERE / "wotbtools"))

from dlc_packs import client_path  # noqa: E402
from export_tank_glb import default_game_data, read_tank_table  # noqa: E402
from wotb_sc2 import decode_dvpl  # noqa: E402

BLOB_TERMINATOR = "\x0c"      # 二进制属性串的尾字节（空块整串就是一个 0x0C）
ROUND = 5                     # 源数据是 float32：5 位小数即可无歧义往返


class SuspensionParseError(RuntimeError):
    """结构不符合已知契约（fail-closed：跳过该车，不造缺省值）。"""


# ---------------------------------------------------------------------------
# 行式 YAML 解析（只覆盖客户端 Parameters 文件的实际形状）
# ---------------------------------------------------------------------------
def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _split_kv(line: str) -> tuple[str, str]:
    key, _, val = line.strip().partition(":")
    return key.strip(), val.strip()


def _unquote(s: str) -> str:
    if len(s) >= 2 and s[0] == s[-1] == '"':
        return s[1:-1]
    return s


def _parse_map(lines: list[str], base_indent: int) -> dict:
    """把一段同缩进层级的 `key[: value]` 行解析成 dict（值缺省 = 嵌套子表）。"""
    out: dict[str, object] = {}
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line.strip():
            i += 1
            continue
        ind = _indent(line)
        if ind != base_indent:
            raise SuspensionParseError(f"缩进层级异常: {line!r}")
        key, val = _split_kv(line)
        if val:
            if val == "{}":
                out[key] = {}
            elif val == "[]":
                out[key] = []
            else:
                out[key] = _unquote(val)
            i += 1
            continue
        child: list[str] = []
        j = i + 1
        while j < len(lines) and (not lines[j].strip() or _indent(lines[j]) > base_indent):
            child.append(lines[j])
            j += 1
        out[key] = _parse_map(child, base_indent + 4)
        i = j
    return out


def _section(lines: list[str], name: str, indent: int = 0) -> dict | None:
    """取顶层（或指定缩进）`name:` 段的子表；不存在返回 None。"""
    for i, line in enumerate(lines):
        if _indent(line) != indent or not line.strip().startswith(f"{name}:"):
            continue
        _, val = _split_kv(line)
        if val:
            raise SuspensionParseError(f"{name} 不是嵌套段: {line!r}")
        child: list[str] = []
        for ln in lines[i + 1:]:
            if not ln.strip():
                continue
            if _indent(ln) <= indent:
                break
            child.append(ln)
        return _parse_map(child, indent + 4)
    return None


# ---------------------------------------------------------------------------
# 数据结构
# ---------------------------------------------------------------------------
def _f(v) -> float:
    try:
        return round(float(v), ROUND)
    except (TypeError, ValueError) as e:
        raise SuspensionParseError(f"浮点解析失败: {v!r}") from e


def _b(v) -> bool:
    s = str(v).strip().lower()
    if s in ("true", "false"):
        return s == "true"
    raise SuspensionParseError(f"布尔解析失败: {v!r}")


def decode_blob(text: str) -> bytes:
    """二进制属性串 → 原始字节。

    文件里的形状是 YAML 双引号标量：`"<base64>\\f"`——`\\f` 是 **0x0C 终止符的转义写法**
    （两字符，不是真字节；空块整串就是 `"\\f"`）。故先把转义还原成真字节再剥离，
    最后补 base64 padding。
    """
    core = text.strip().replace("\\f", "\x0c").strip()
    core = core.rstrip(BLOB_TERMINATOR).strip()
    return base64.b64decode(core + "=" * (-len(core) % 4))


def parse_wheels(text: str) -> list[list]:
    """`wheels` 块 = 每侧逐轮数组，12 B/条 `{u32 flag, f32 a, f32 b}`。"""
    raw = decode_blob(text)
    if len(raw) % 12:
        raise SuspensionParseError(f"wheels 块长度不是 12 的倍数: {len(raw)}")
    out = []
    for i in range(len(raw) // 12):
        flag, a, b = struct.unpack_from("<Iff", raw, i * 12)
        out.append([int(flag), _f(a), _f(b)])
    return out


def parse_chain(text: str) -> list[list]:
    """链折线 = 每点 12 B `{f32 x, f32 y, u32 flag}`。"""
    raw = decode_blob(text)
    if len(raw) % 12:
        raise SuspensionParseError(f"链折线长度不是 12 的倍数: {len(raw)}")
    out = []
    for i in range(len(raw) // 12):
        x, y, flag = struct.unpack_from("<ffI", raw, i * 12)
        out.append([_f(x), _f(y), int(flag)])
    return out


def parse_suspension(txt: str) -> dict | None:
    """Parameters yaml 文本 → 悬挂块（无 suspension 段 → None）。"""
    lines = txt.splitlines()
    sus = _section(lines, "suspension")
    if sus is None:
        return None
    out: dict[str, object] = {}
    # enabled / chunkPrototypeExtending 在个别车（如 Oth10_WarDuck）缺省——按"默认启用、
    # 不外扩"处理；wheels 与链缺失/为空则视为**无悬挂可仿**（前端回落刚体）。
    out["enabled"] = _b(sus["enabled"]) if "enabled" in sus else True
    out["wheels_reaction_speed"] = _f(sus.get("wheelsReactionSpeed", 0.0))
    out["chunk_prototype_extending"] = _f(sus.get("chunkPrototypeExtending", 0.0))
    if not sus.get("wheels"):
        raise SuspensionParseError("empty_suspension: wheels 为空块")
    out["wheels"] = parse_wheels(str(sus["wheels"]))
    # 履带链：两代键式（复数 map / 单数串）；键序保留（多段模型 = 多个 key）
    if "leftTrackChains" in sus or "rightTrackChains" in sus:
        keys: list[str] = []
        chains: dict[str, list] = {"left": [], "right": []}
        for side, plural in (("left", "leftTrackChains"), ("right", "rightTrackChains")):
            tbl = sus.get(plural)
            if not isinstance(tbl, dict):
                raise SuspensionParseError(f"缺 {plural}")
            for k in sorted(tbl, key=lambda s: int(s)):
                chains[side].append(parse_chain(str(tbl[k])))
                if side == "left":
                    keys.append(k)
        out["chain_keys"] = keys
    else:
        chains = {"left": [parse_chain(str(sus["leftTrackChain"]))],
                  "right": [parse_chain(str(sus["rightTrackChain"]))]}
        out["chain_keys"] = ["0"]
    if not (chains["left"] or chains["right"]):
        raise SuspensionParseError("empty_suspension: 履带链为空")
    out["chains"] = chains
    bend = sus["trackBendingInfo"]
    out["track_bending"] = {
        "front_drive_wheel": _b(bend["frontDriveWheel"]),
        "upper_min": _f(bend["upperMin"]),
        "upper_factor": _f(bend["upperFactor"]),
        "front_factor": _f(bend["frontFactor"]),
        "back_factor": _f(bend["backFactor"]),
        "length_power": _f(bend["lengthPower"]),
        "speed": _f(bend["speed"]),
    }
    lay = sus["trackLayingInfo"]
    out["track_laying"] = {
        "bending_factor": _f(lay["bendingFactor"]),
        "length_power": _f(lay["lengthPower"]),
        "point_count_power": _f(lay["pointCountPower"]),
        "pressure_power": _f(lay["pressurePower"]),
        "primary_power": _f(lay["primaryPower"]),
    }
    chas = _section(lines, "chassis") or {}
    if "textureScale" in chas:
        out["texture_scale"] = _f(chas["textureScale"])
    return out


# ---------------------------------------------------------------------------
# 逐车导出
# ---------------------------------------------------------------------------
def export_one(game_data: pathlib.Path, tank_id: int, nation: str, stem: str,
               out_dir: pathlib.Path) -> dict:
    st: dict[str, object] = {"tank_id": tank_id, "stem": stem, "nation": nation}
    rel = f"3d/Tanks/Parameters/{nation}/{stem}.yaml.dvpl"
    p = client_path(game_data, rel)
    if not p.exists():
        st["skip"] = "no_yaml"
        return st
    txt = decode_dvpl(p.read_bytes()).decode("utf-8", "replace")
    try:
        sus = parse_suspension(txt)
    except SuspensionParseError as e:
        st["skip"] = str(e) if str(e).startswith("empty_suspension") else f"parse_error: {e}"
        return st
    if sus is None:
        st["skip"] = "no_suspension_block"
        return st
    if not sus["enabled"]:
        st["skip"] = "disabled"
        return st
    doc = {"tank_id": tank_id, "nation": nation, "stem": stem, "source": rel, **sus}
    (out_dir / f"{tank_id}.json").write_text(
        json.dumps(doc, ensure_ascii=False, sort_keys=True, separators=(",", ":")),
        encoding="utf-8")
    st["wheels"] = len(sus["wheels"])                       # type: ignore[arg-type]
    st["chain_pts"] = [len(c) for c in sus["chains"]["left"]]  # type: ignore[index]
    return st


def main() -> int:
    ap = argparse.ArgumentParser(description="逐车悬挂参数导出（客户端 yaml → 资产包）")
    ap.add_argument("--game-data", type=pathlib.Path, default=None)
    ap.add_argument("--tanks-pb", type=pathlib.Path,
                    default=_HERE.parent / "data" / "tanks.pb")
    ap.add_argument("--out", type=pathlib.Path,
                    default=_HERE.parent / "data" / "tank_suspension")
    ap.add_argument("--tank", type=int, action="append", default=[],
                    help="tank_id（可重复）；缺省 = 全表")
    ap.add_argument("--list", action="store_true", help="只列 tank_id → 模型名")
    args = ap.parse_args()

    game_data = args.game_data or default_game_data()
    if not game_data.is_dir():
        print(f"!! 客户端 Data 目录不存在: {game_data}（用 --game-data 指定）", file=sys.stderr)
        return 2
    table = read_tank_table(args.tanks_pb)
    if args.list:
        for tid in sorted(table):
            print(f"{tid}\t{table[tid]['nation']}\t{table[tid]['stem']}")
        return 0

    ids = sorted(args.tank) if args.tank else sorted(table)
    args.out.mkdir(parents=True, exist_ok=True)
    ok = 0
    skipped: dict[str, int] = {}
    stats = {"wheels_min": 10**9, "wheels_max": 0, "pts_min": 10**9, "pts_max": 0}
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
        stats["wheels_min"] = min(stats["wheels_min"], st["wheels"])
        stats["wheels_max"] = max(stats["wheels_max"], st["wheels"])
        pt = st["chain_pts"][0]
        stats["pts_min"] = min(stats["pts_min"], pt)
        stats["pts_max"] = max(stats["pts_max"], pt)
    total = sum(f.stat().st_size for f in args.out.glob("*.json"))
    print(f"导出 {ok} 辆 → {args.out}（{total/1024:.0f} KB）")
    if ok:
        print(f"  每侧轮数 {stats['wheels_min']}..{stats['wheels_max']}；链点数 {stats['pts_min']}..{stats['pts_max']}")
    if skipped:
        print("  跳过: " + ", ".join(f"{k}×{v}" for k, v in sorted(skipped.items())))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
