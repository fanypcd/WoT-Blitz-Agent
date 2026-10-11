"""解剖 tank .sc2 的状态开关子树：名字 / visibility / StateSwitcherComponent / 批次 lod+switch。

用法: python probe_switch.py <nation> <stem> [关键字]
例:   python probe_switch.py italy It08_Progetto_M40_mod_65 hide_elements
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from wotb_sc2 import decode_dvpl, read_sc2  # noqa: E402
import export_tank_glb as X  # noqa: E402


def main():
    nation, stem = sys.argv[1], sys.argv[2]
    key = sys.argv[3] if len(sys.argv) > 3 else ""
    res = X.resolve_tank(X.default_game_data(), nation, stem)
    print("sc2:", res["model_sc2"])
    scene = read_sc2(decode_dvpl(res["model_sc2"].read_bytes()))

    def walk(e, depth):
        comps = X.entity_components(e)
        name = e.get("name") or e.get("##name") or "?"
        rc = comps.get("RenderComponent")
        sw = comps.get("StateSwitcherComponent")
        bits = []
        if e.get("visibility", 0):
            bits.append(f"vis={e['visibility']}")
        if sw:
            bits.append(f"StateSwitcher(activeState={sw.get('ssc.activeState')!r})")
        if rc:
            ro = rc.get("rc.renderObj") or {}
            for bk in sorted(ro.get("ro.batches") or {}):
                bi = int(bk)
                lod = ro.get(f"rb{bi}.lodIndex", -1)
                swi = ro.get(f"rb{bi}.switchIndex", -1)
                bits.append(f"{bk}(lod={lod},sw={swi})")
        if key.lower() in name.lower() or (depth <= 2):
            print(f"{'  ' * depth}{name}  {' '.join(bits)}")
        for c in X.nested(e):
            walk(c, depth + 1)

    for e in X.nested(scene):
        walk(e, 0)


if __name__ == "__main__":
    main()
