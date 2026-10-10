#!/usr/bin/env python3
"""逐图光照参数导出：`data/cache/maps/<space>.lighting.json`（打包器收进 `map/<key>/lighting.json`）。

数据源：`Data/3d/Maps/<space>/<space>.sc2` 的 **`#sceneComponentSets.Default`**
（⚠️ 不是 `#sceneComponents`——后者 `count: 0`）：

| 组件 | 用途 | 消费者 |
|---|---|---|
| `DirectionalLightComponent` | 太阳色/强度/环境色 + `transformNode.quaternion`（朝向） | 3D 回放场景的方向光与半球环境光 |
| `SceneRenderConfigComponent` | 距离雾 + 半空间雾 + 大气天/日色散射 | 待接（雾对齐，见 map-lighting-plan 期 2c） |
| `IBLComponent` | 逐图 diffuse/specular 立方图引用与倍率 | 待接（坦克 IBL，期 2d） |

**太阳方向约定（2026-10-09 本机全树实测，36 张可玩图自洽）**：把 `quaternion` 按 DAVA
`(x,y,z,w)` 解开后，**光源局部 −Y 轴**旋转到世界系 = 阳光**传播**方向（源→地面）。
36 图仰角全部落在 21.5°–65.7°；其余候选轴（±X/±Z/±Y 五种）在 8–12 张图上会算出
"太阳在地下"（负仰角），全部排除。
⚠️ **未定项**：该约定尚缺一次与客户端画面的 A/B 对照（见
[docs/feasibility-map-lighting.md](../docs/feasibility-map-lighting.md) §七 #1）；若日后证伪，
只需改本文件的 `sun_direction_from_quaternion` 与 `CONVENTION`，消费端读到的字段名不变。

坐标：客户端世界系 **z 上**；消费端（three.js 场景）经 `qFrame = Ry(π)·Rx(−π/2)`
映射 `(x,y,z) → (−x, z, y)`（WotbTools `playbackScene` 的 mapScenery 同式）。故本文件
**同时给出 `direction`（客户端世界系，留档）与 `direction_scene`（场景系，消费端直接用）**，
避免每个消费端各写一遍换算。

用法：
    python tools/export_map_lighting.py                 # 全部（按 maps.yaml 注册表）
    python tools/export_map_lighting.py --map himmelsdorf --map 17
    python tools/export_map_lighting.py --out-dir data/cache/maps
"""

from __future__ import annotations

import argparse
import json
import math
import pathlib
import sys

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_DIR))
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

import export_map_glb as E  # noqa: E402

CONVENTION = "dava-light-local-negative-y"

# 记录到 JSON 的组件内标量字段（逐字段白名单——客户端字段名一字不改，便于复核；
# 未列入的字段留在客户端文件里，需要时再补，不臆造）
FOG_SCALARS = (
    "FOG_LINEAR", "FOG_ATMOSPHERE", "FOG_HALFSPACE", "FOG_HALFSPACE_LINEAR", "VERTEX_FOG",
    "fogColor.r", "fogColor.g", "fogColor.b", "fogColor.a",
    "fogDensity", "fogStart", "fogEnd", "fogLimit",
    "fogAtmosphereColorSun.r", "fogAtmosphereColorSun.g", "fogAtmosphereColorSun.b",
    "fogAtmosphereColorSky.r", "fogAtmosphereColorSky.g", "fogAtmosphereColorSky.b",
    "fogAtmosphereDistance", "fogAtmosphereScattering",
    "fogHalfspaceHeight", "fogHalfspaceFalloff", "fogHalfspaceDensity", "fogHalfspaceLimit",
)


IBL_SCALARS = (
    "ibl.enableIBL", "ibl.class", "ibl.dimensions", "ibl.environmentGamma",
    "ibl.environmentMultiplier", "ibl.environmentGroundFactor",
    "ibl.diffuse", "ibl.specular", "ibl.source", "ibl.output",
    "ibl.near.clip.plane", "ibl.far.clip.plane",
)


def quat_matrix(q: list[float]) -> list[list[float]]:
    """DAVA `(x,y,z,w)` → 行主序 3×3（本机坐标，不做镜像）。"""
    x, y, z, w = q
    n = math.sqrt(x * x + y * y + z * z + w * w) or 1.0
    x, y, z, w = x / n, y / n, z / n, w / n
    return [
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ]


def sun_direction_from_quaternion(q: list[float]) -> list[float]:
    """阳光**传播**方向（客户端世界系，单位向量）= R(q) · (0,−1,0)。

    依据与候选轴排除见本文件 docstring；`docs/feasibility-map-lighting.md` §七 #1 记为
    待 A/B 的约定项。"""
    m = quat_matrix(q)
    v = [-m[0][1], -m[1][1], -m[2][1]]        # 局部 −Y 轴（列 1 取负）
    n = math.sqrt(sum(c * c for c in v)) or 1.0
    return [c / n for c in v]


def game_to_scene(v: tuple[float, float, float] | list[float]) -> list[float]:
    """客户端世界系（z 上）→ 场景系（three.js，y 上）：qFrame = Ry(π)·Rx(−π/2) ⇒ (−x, z, y)。

    与 WotbTools `playbackScene` 的 `mapScenery.rotation.set(-π/2, π, 0)` 逐式一致。"""
    x, y, z = v[0], v[1], v[2]
    return [-x, z, y]


def _scalars(src: dict, names: tuple[str, ...]) -> dict:
    out = {}
    for name in names:
        if name in src:
            out[name] = src[name]
    return out


def _components(scene: dict) -> dict:
    """#sceneComponentSets.Default → {comp.typename: node}。"""
    sets = scene.get("#sceneComponentSets") or {}
    out: dict[str, dict] = {}
    for items in sets.values():
        if not isinstance(items, list):
            continue
        for item in items:
            if isinstance(item, dict) and item.get("comp.typename"):
                out[item["comp.typename"]] = item
    return out


def extract_lighting(scene: dict, space: str) -> dict:
    """纯函数：sc2 实体树 → lighting.json 结构（缺件记 warnings，不臆造默认值）。"""
    comps = _components(scene)
    warnings: list[str] = []
    out: dict = {"space": space, "warnings": warnings}

    dl = comps.get("DirectionalLightComponent")
    if dl is None:
        warnings.append("DirectionalLightComponent 缺失：太阳参数未导出")
    else:
        light = dl.get("light") or {}
        transform = ((dl.get("transformNode") or {}).get("transform")) or {}
        q = transform.get("quaternion")
        sun = {
            "color": [light.get("color.r"), light.get("color.g"), light.get("color.b")],
            "intensity": light.get("intensity"),
            "ambient": [light.get("ambColor.r"), light.get("ambColor.g"), light.get("ambColor.b")],
            "type": light.get("type"),
            "position": transform.get("position"),
            "quaternion": q,
            "convention": CONVENTION,
        }
        if q:
            direction = sun_direction_from_quaternion(q)
            sun["direction"] = direction
            sun["direction_scene"] = game_to_scene(direction)
        else:
            warnings.append("太阳 quaternion 缺失：direction 未导出")
        out["sun"] = sun

    src = comps.get("SceneRenderConfigComponent")
    if src is not None:
        fog = _scalars(src, FOG_SCALARS)
        if fog:
            fog["raw"] = {k: src[k] for k in src if k.startswith("fog") or k.startswith("FOG")
                          or k.startswith("VERTEX")}
            out["fog"] = fog
    else:
        warnings.append("SceneRenderConfigComponent 缺失：雾参数未导出")

    ibl = comps.get("IBLComponent")
    if ibl is not None:
        out["ibl"] = _scalars(ibl, IBL_SCALARS)

    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--map", action="append", help="显示名/键/回放地图 id（可重复）")
    ap.add_argument("--game-data", type=pathlib.Path,
                    default=pathlib.Path("D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data"))
    ap.add_argument("--out-dir", type=pathlib.Path, default=pathlib.Path("data/cache/maps"))
    ap.add_argument("--jobs", type=int, default=4, help="（占位，导出为纯解析、开销极小）")
    args = ap.parse_args()

    entries = E.load_registry(args.game_data)
    if args.map:
        picked = []
        for name in args.map:
            e = E.resolve_entry(entries, name)
            if e is None:
                print(f"[skip] 未在注册表命中：{name}", file=sys.stderr)
                continue
            picked.append(e)
    else:
        picked = entries

    args.out_dir.mkdir(parents=True, exist_ok=True)
    ok = fail = 0
    for entry in picked:
        sc2 = E.find_member(args.game_data / "3d" / "Maps" / entry.space, entry.space, ".sc2.dvpl") \
            or E.find_member(args.game_data / "3d" / "Maps" / entry.space, entry.space, ".sc2")
        if sc2 is None:
            print(f"[skip] {entry.space}: 场景文件缺失")
            fail += 1
            continue
        scene = E.read_sc2(E.load_payload(sc2))
        data = extract_lighting(scene, entry.space)
        data["map_id"] = entry.map_id
        data["key"] = entry.key
        (args.out_dir / f"{entry.space}.lighting.json").write_text(
            json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")
        sun = data.get("sun") or {}
        d = sun.get("direction_scene")
        elev = None
        if d:
            elev = math.degrees(math.asin(max(-1.0, min(1.0, -d[1]))))  # 场景系 y 上：−dy = 仰角
        print(f"[ok] {entry.space:24s} sun i={sun.get('intensity')} "
              f"dir={[round(c, 3) for c in (d or [])]} 仰角={elev if elev is None else round(elev, 1)}° "
              f"warn={len(data['warnings'])}")
        ok += 1
    print(f"完成：{ok} 张，失败 {fail}")
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
