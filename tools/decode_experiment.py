"""受控实验解码器：回放切面 + 可破坏物清单 → 每个事件时刻的候选物体表。

用法:
  python tools/decode_experiment.py <facet.playback.json> [notes]
  notes = 可选，"时刻,类型" 逗号分隔的碾压顺序备注（如 "18,栅栏; 25,树"）

对每个 AreaDestructibles 事件：
  - 事件时刻作者车的回放坐标（0.1s 网格插值，x 取反 = 场景系）
  - 候选物体 = 场景坐标距作者 ≤8m 的可破坏物（含 lka serverId 若有）
  - 无轨迹（未点亮视角）时回退：事件格子内的全部物体
"""
import json
import math
import pathlib
import sys

MAPS = pathlib.Path("data/cache/maps")


def load_map_by_id(map_id: int):
    for p in MAPS.glob("*/destructibles.json"):
        d = json.loads(p.read_text(encoding="utf-8"))
        if d.get("mapId") == map_id:
            return d
    return None


def author_track(facet):
    for v in facet["vehicles"]:
        if v.get("is_author"):
            pos = v["pos"]
            t0 = facet["meta"]["t_start"]
            pts = [(t0 + i / 10.0, pos[i], pos[i + 2]) for i in range(0, len(pos), 3)]
            return v.get("eid"), pts
    return None, []


def pos_at(pts, t):
    if not pts:
        return None
    lo, hi = 0, len(pts) - 1
    if t <= pts[0][0] or t >= pts[-1][0]:
        i = 0 if t <= pts[0][0] else hi
        return pts[i][1], pts[i][2]
    while hi - lo > 1:
        mid = (lo + hi) // 2
        if pts[mid][0] <= t:
            lo = mid
        else:
            hi = mid
    (t0, x0, z0), (t1, x1, z1) = pts[lo], pts[hi]
    f = (t - t0) / (t1 - t0) if t1 > t0 else 0
    return x0 + (x1 - x0) * f, z0 + (z1 - z0) * f


def main():
    facet = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
    notes = sys.argv[2] if len(sys.argv) > 2 else ""
    d = load_map_by_id(facet["meta"]["map_id"])
    if d is None:
        sys.exit(f"map {facet['meta']['map_id']} 无 destructibles.json，先跑 export_map_destructibles.py")
    print(f"地图: {d['key']} ({d['display']})  实例 {d['instanceCount']}  lka {d.get('lkaSize', 0)}")

    eid, pts = author_track(facet)
    print(f"作者车: eid={eid} 轨迹 {len(pts)} 点")

    # lka 逆表: (cellX, cellY, slot) -> 场景实体 id
    inv = {}
    for i in d["instances"]:
        srv = i.get("serverId")
        if srv:
            inv[(srv["cell"][0], srv["cell"][1], srv["slot"])] = i
    spos = {i["id"]: i for i in d["instances"]}
    for e in facet.get("destructible_events", []):
        a = next((a for a in facet.get("destructible_areas", []) if a["eid"] == e["area_eid"]), None)
        if a is None:
            continue
        # 直接坐标（无镜像）：cell = floor(x/100), floor(z/100)
        cell = (int(a["x"] // 100), int(a["z"] // 100))
        args = bytes(e["args"])
        prop = (args[0] >> 5) & 3
        slot = e.get("slot", args[-1])
        kind = {1: "fragile", 2: "column", 3: "tree"}.get(prop, "?")
        inst = inv.get((cell[0], cell[1], slot))
        if inst is None:
            # 锚点贴格边界兜底：±1 邻域
            for dx in (-1, 0, 1):
                for dz in (-1, 0, 1):
                    inst = inv.get((cell[0] + dx, cell[1] + dz, slot))
                    if inst:
                        break
                if inst:
                    break
        line = f"t={e['clock']:7.2f} cell={cell} [{kind:6}] slot={slot:2d}"
        if inst:
            line += (f" → {inst['name'][:34]:34} scene_id={inst['id']:6} "
                     f"@({inst['pos'][0]:7.1f},{inst['pos'][1]:7.1f})")
        else:
            line += " → 未命中"
        print(line)
    if notes:
        print(chr(10) + "== 碾压顺序备注 ==")
        for part in notes.replace("；", ";").split(";"):
            print("  ", part.strip())


if __name__ == "__main__":
    main()
