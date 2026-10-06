"""导出地图可破坏物清单：.sc2 场景实例（StateSwitcher/SpeedTree + 碰撞/耐久）+
客户端全局类型库（Data/XML/destructibles.xml）→ data/cache/maps/<key>/destructibles.json。

数据链（客户端侧证据，见 docs/回放与射击逆向总集.md §5.4 / 第二篇 §4.4）：
  - 每个可破坏物 = StateSwitcher 实体：State0 完好 / State1（`_crash`）摧毁形态；
    CollisionTypeComponent { CollisionType, Density, Health, FallingType, MaterialKind }
    给出耐久与物理类别（与 destructibles.xml 同值）；ActionComponent 监听 11 号事件切换。
  - 树 = SpeedTreeObject 实体，倒伏由客户端 SpeedTree 系统按 destructibles.xml
    `<trees>` 条目（.spt 文件名联表）驱动。
  - 类型库 `<fragiles>`（.model）按实例名 stem 联表（fag_ln_fence_wood.sc2 →
    fag_ln_fence_wood.model）。

用法:
  python tools/export_map_destructibles.py --map lagoon --map malinovka
  python tools/export_map_destructibles.py --map lagoon --stdout   # 只打印不落盘
"""
import argparse
import json
import pathlib
import re
import sys
import xml.etree.ElementTree as ET

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "wotbtools"))

from wotb_sc2 import Reader, decode_dvpl, read_archive, read_sc2  # noqa: E402
import export_map_glb as X  # noqa: E402

DEFAULT_GAME_DATA = pathlib.Path(
    "D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data"
)


def name_stem(name: str) -> str:
    """实例名 → 类型联表键：去扩展名、去尾部 .lodN/序号（b2_3lod.sc2 → b2_3lod）。"""
    s = re.sub(r"\.(sc2|scg|model|spt)$", "", name, flags=re.I)
    s = re.sub(r"[\s_]*lod\d*$", "", s, flags=re.I)
    return s


def parse_destructibles_xml(game_data: pathlib.Path) -> dict[str, dict]:
    """全局类型库：键 = content/convert/<map>/… 的文件 stem，值 = health/density/段名。"""
    p = game_data / "XML" / "destructibles.xml.dvpl"
    root = ET.fromstring(decode_dvpl(p.read_bytes()).decode("utf-8", "replace"))
    out: dict[str, dict] = {}
    for section in ("trees", "fragiles", "fallingAtoms", "structures"):
        node = root.find(section)
        if node is None:
            continue
        for e in node.findall("entry"):
            fn = e.findtext("filename") or ""
            stem = name_stem(pathlib.PurePosixPath(fn).name)
            out.setdefault(
                stem,
                {
                    "section": section,
                    "file": fn,
                    "health": int(e.findtext("health") or 0),
                    "density": float(e.findtext("density") or 0) or None,
                },
            )
    return out


def collision_of(entity: dict) -> dict | None:
    for c in X.components_of(entity):
        if isinstance(c, dict) and c.get("comp.typename") == "CollisionTypeComponent":
            return {
                "collision_type": c.get("CollisionType"),
                "health": c.get("Health"),
                "density": c.get("Density"),
                "falling_type": c.get("FallingType"),
                "material_kind": c.get("MaterialKind"),
            }
    return None


def states_of(entity: dict) -> tuple[str, str] | None:
    for c in X.components_of(entity):
        if isinstance(c, dict) and c.get("comp.typename") == "StateSwitcherComponent":
            return (str(c.get("ssc.state0")), str(c.get("ssc.state1")))
    return None


def is_speedtree(entity: dict) -> bool:
    ro = X.component_by_type(entity, "RenderComponent")
    if not isinstance(ro, dict):
        return False
    obj = ro.get("rc.renderObj")
    return isinstance(obj, dict) and str(obj.get("##name", "")) == "SpeedTreeObject"


def _read_lka(path: pathlib.Path) -> dict:
    ka = read_archive(Reader(decode_dvpl(path.read_bytes())))
    out = {}
    for k, v in ka.items():
        if not str(k).isdigit() or not isinstance(v, int):
            continue
        out[str(k)] = {
            "serverId": v,
            "cell": [((v >> 24) & 0xFF) - 0x7F, ((v >> 16) & 0xFF) - 0x7F],
            "slot": v & 0xFFFF,
        }
    return out


def parse_lka(entry: X.MapEntry, game_data: pathlib.Path) -> dict:
    """blitz/<space>.lka（KeyedArchive）：场景实体 id → 服务器可破坏物 id。

    值 = u32：(cellX=floor(x/100)+0x7F, cellY=floor(z/100)+0x7F, slot u16)——
    100m 格子码 + 格内槽位（几何相关性 0.985/0.986 实测；与 devReplaysIgnores
    的 ignored_destructables 同域。客户端 DestructibleManager 按 key 查节点，
    exe 断言 "wrong lka key provided"/"Missed key:"）。覆盖子集：SpeedTree +
    小型 fragiles；temple/militaryBox/WatchTower/bush_liveOak 等不在表内。

    **slot 编号口径（2026-10-06 闭合）**：存在分段索引表 `<stem>.erN.lka` 时
    **整体替代主表**，不得合并——两套是不同的编号体系（erlenberg 实测：829 个
    公共键 455 个 serverId 不同、268 键仅在分段表；Middleburg 回放 11 事件
    地面真值判定 er0 表 11/11 命中，主表 4 MISS + 6 错联至远处实体/建筑）。
    分段表之间必须逐键一致，不一致 = 结构漂移，fail-closed 拒绝猜测。
    """
    lka_rel = entry.local_name.replace("/", "\\")
    lka_dir = (game_data / "3d" / "Maps" / lka_rel).parent / "blitz"
    stem = pathlib.PurePath(lka_rel).stem
    seg_paths = sorted(lka_dir.glob(stem + ".er*.lka.dvpl"))
    if seg_paths:
        tables = [(p, _read_lka(p)) for p in seg_paths]
        base_path, base = tables[0]
        for p, t in tables[1:]:
            if t != base:
                raise AssertionError(
                    f"{p.name} 与 {base_path.name} 逐键不一致（分段 lka 漂移，拒绝猜测）"
                )
        return base
    main_path = lka_dir / (stem + ".lka.dvpl")
    if not main_path.exists():
        return {}
    return _read_lka(main_path)


def export_map(entry: X.MapEntry, game_data: pathlib.Path, types: dict) -> dict:
    sc2 = game_data / "3d" / "Maps" / entry.space
    sc2_rel = entry.local_name.replace("/", "\\")
    sc2_path = (game_data / "3d" / "Maps" / sc2_rel).with_name(
        pathlib.PurePath(sc2_rel).name + ".dvpl"
    )
    scene = read_sc2(decode_dvpl(sc2_path.read_bytes()))

    instances = []
    for path, ent in X.iter_entities_recursive(scene):
        coll = collision_of(ent)
        states = states_of(ent)
        tree = is_speedtree(ent)
        if coll is None and states is None and not tree:
            continue
        name = str(ent.get("name") or "")
        if re.search(r"State [1-9]|_crash|^touch$|^start$", name):
            continue  # 摧毁态子树/触发体不入清单（客户端初始态已在父实体上）
        t = (X.world_transform(ent).get("translation") or [0, 0, 0])
        stem = name_stem(name)
        type_ref = types.get(stem) or types.get(re.sub(r"[\d_]+$", "", stem))
        inst = {
            "id": ent.get("id"),
            "name": name,
            "pos": [round(t[0], 2), round(t[1], 2), round(t[2], 2)],
            "kind": "tree" if tree else "switcher",
            # 100m 区域格子（回放系 x/z；.sc2 x/y 水平、z 高度）
            "cell": [int(t[0] // 100) * 100, int(t[1] // 100) * 100],
            "scene_path": path,
        }
        if coll:
            inst["collision"] = coll
        if type_ref:
            inst["type"] = {
                k: type_ref[k] for k in ("section", "file", "health", "density")
            }
        instances.append(inst)
    instances.sort(key=lambda i: (i["cell"], str(i["name"]), i["id"] or 0))
    lka = parse_lka(entry, game_data)

    # 补收 lka 键指向但无标记组件的父实体（如 Big_live_oak：父实体只有
    # TransformComponent，碰撞/SpeedTree 标记在子实体上——服务器按父 id 寻址，
    # 逆向总集 §5.4 的 (cell, slot) 联表目标必须是父实体）
    have = {i["id"] for i in instances}
    for path, ent in X.iter_entities_recursive(scene):
        eid = ent.get("id")
        if eid is None or eid in have or str(eid) not in lka:
            continue
        t = (X.world_transform(ent).get("translation") or [0, 0, 0])
        # 类别从子树推断：含 SpeedTreeObject 即树
        is_tree = False
        for _, sub in X.iter_entities_recursive(ent):
            if is_speedtree(sub):
                is_tree = True
                break
        instances.append({
            "id": eid,
            "name": str(ent.get("name") or ""),
            "pos": [round(t[0], 2), round(t[1], 2), round(t[2], 2)],
            "kind": "tree" if is_tree else "switcher",
            "cell": [int(t[0] // 100) * 100, int(t[1] // 100) * 100],
            "scene_path": path,
        })

    for i in instances:
        ref = lka.get(str(i["id"]))
        if ref:
            i["serverId"] = ref

    return {
        "mapId": entry.map_id,
        "key": entry.key,
        "display": entry.display,
        "space": entry.local_name,
        "instanceCount": len(instances),
        "typeTableSize": len(types),
        "lkaSize": len(lka),
        "instances": instances,
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--map", action="append", required=True,
                    help="地图 key（可多次），如 lagoon / malinovka")
    ap.add_argument("--game-data", type=pathlib.Path, default=DEFAULT_GAME_DATA)
    ap.add_argument("--output-dir", type=pathlib.Path,
                    default=pathlib.Path("data/cache/maps"))
    ap.add_argument("--stdout", action="store_true", help="打印摘要，不写文件")
    args = ap.parse_args()

    registry = X.load_registry(args.game_data)
    types = parse_destructibles_xml(args.game_data)
    for key in args.map:
        entry = X.resolve_entry(registry, key)
        if entry is None:
            print(f"[skip] 未在注册表找到 {key}")
            continue
        doc = export_map(entry, args.game_data, types)
        kinds = {}
        for i in doc["instances"]:
            kinds[i["kind"]] = kinds.get(i["kind"], 0) + 1
        print(f"{doc['key']}: instances={doc['instanceCount']} {kinds} "
              f"typeTable={doc['typeTableSize']}")
        if args.stdout:
            for i in doc["instances"][:10]:
                print("  ", json.dumps(i, ensure_ascii=False))
            continue
        out_dir = args.output_dir / doc["key"]
        out_dir.mkdir(parents=True, exist_ok=True)
        out = out_dir / "destructibles.json"
        out.write_text(json.dumps(doc, ensure_ascii=False), encoding="utf-8")
        print(f"  → {out}")


if __name__ == "__main__":
    main()
