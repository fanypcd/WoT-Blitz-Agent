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
  - lka 选表：多变体 space 有 `<stem>.<前缀><序号>.lka` 分段表，**按 map_id 序数取本变体
    那一张**（与场景变体组 `variantByMapId` 同一配对）；主表是改版前遗留编号体系，只在
    没有分段表时使用。分段表导出前做结构自检（key 必须在场景、格子码必须等于实体位置）。

用法:
  python tools/export_map_destructibles.py --map lagoon --map malinovka
  python tools/export_map_destructibles.py --map lagoon --stdout   # 只打印不落盘
"""
import argparse
import json
import math
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


def _segment_tables(lka_dir: pathlib.Path, stem: str) -> list[tuple[int, str, pathlib.Path]]:
    """分段索引表 = `<stem>.<前缀><序号>.lka.dvpl`，按序号升序。

    命名是**地图变体前缀 + 序号**（erlenberg=er0/er1/er2、desert_train=dt1/dt2/dt3、
    rudniki=rd0..rd3、milbase=mlb1..mlb5……），不是固定前缀；只按 `er*` 匹配会漏掉
    除 Middleburg 外的全部多变体图（2026-10-22 实测 9 个 space 都有分段表）。
    同一 space 内前缀必须唯一、序号不得重复；不满足即结构漂移，拒绝猜测。
    """
    pat = re.compile(re.escape(stem) + r"\.([a-z]+)(\d+)\.lka\.dvpl$")
    segs: list[tuple[int, str, pathlib.Path]] = []
    for p in sorted(lka_dir.glob(stem + ".*.lka.dvpl")):
        m = pat.fullmatch(p.name)
        if m:
            segs.append((int(m.group(2)), m.group(1), p))
    if not segs:
        return []
    prefixes = {pre for _, pre, _ in segs}
    if len(prefixes) != 1:
        raise AssertionError(f"{stem}: 分段表前缀不唯一 {sorted(prefixes)}，拒绝猜测")
    nums = [n for n, _, _ in segs]
    if len(set(nums)) != len(nums):
        raise AssertionError(f"{stem}: 分段表序号重复 {nums}，拒绝猜测")
    return sorted(segs)


def select_variant_table(
    entry: X.MapEntry,
    registry: list[X.MapEntry],
    segs: list[tuple[int, str, pathlib.Path]],
) -> tuple[int, str, pathlib.Path]:
    """按**序数**把分段表配对到同 space 的地图 key。

    客户端一变体一张表；配对规则与场景变体组一致（`export_map_glb` 的
    `variantByMapId`）：同 space 的 key 按 map_id 升序 ↔ 分段表按序号升序。
    例：desert_train(2)/_02(44)/_03(45) ↔ dt1/dt2/dt3；rudniki(6)/_01(60)/_02(61)/
    _03(62) ↔ rd0/rd1/rd2/rd3。数量不等或当前 key 不在其中 = 结构漂移，fail-closed。
    """
    siblings = sorted((e for e in registry if e.space == entry.space),
                      key=lambda e: e.map_id)
    if len(siblings) != len(segs):
        raise AssertionError(
            f"{entry.space}: 分段表 {len(segs)} 张 ≠ 注册表变体 {len(siblings)} 个"
            f"（{ [e.key for e in siblings] }），拒绝猜测"
        )
    idx = next((i for i, e in enumerate(siblings) if e.map_id == entry.map_id), None)
    if idx is None:
        raise AssertionError(f"{entry.space}: 当前 key {entry.key} 不在同 space 变体组内")
    return segs[idx]


def parse_lka(entry: X.MapEntry, game_data: pathlib.Path,
              registry: list[X.MapEntry]) -> tuple[dict, dict]:
    """blitz/<space>.lka（KeyedArchive）：场景实体 id → 服务器可破坏物 id。

    值 = u32：(cellX=floor(x/100)+0x7F, cellY=floor(z/100)+0x7F, slot u16)——
    100m 格子码 + 格内槽位（几何相关性 0.985/0.986 实测；与 devReplaysIgnores
    的 ignored_destructables 同域。客户端 DestructibleManager 按 key 查节点，
    exe 断言 "wrong lka key provided"/"Missed key:"）。覆盖子集：SpeedTree +
    小型 fragiles；temple/militaryBox/WatchTower/bush_liveOak 等不在表内。

    **选表口径（2026-10-22 修正）**：存在分段表（`<stem>.<前缀><序号>.lka`）时
    **按 map_id 序数取本变体那一张**，整体替代主表，不得合并——两套是不同的编号
    体系。主表是地图改版前的遗留表：desert_train 实测 652 条中 307 条指向当前
    .sc2 已不存在的实体、81 条格子码与实体实际位置不符；分段表 976/976 全部可解析
    且格子码 100% 一致。旧实现对前缀硬编码 `er*`，导致其余 8 个多变体 space 全部
    回退到这张遗留主表，把树倒事件联到房屋等错误物体（2026-10-22 用户报告）。
    """
    lka_rel = entry.local_name.replace("/", "\\")
    lka_dir = (game_data / "3d" / "Maps" / lka_rel).parent / "blitz"
    stem = pathlib.PurePath(lka_rel).stem
    segs = _segment_tables(lka_dir, stem)
    if segs:
        num, prefix, path = select_variant_table(entry, registry, segs)
        table = _read_lka(path)
        keys = [(v["cell"][0], v["cell"][1], v["slot"]) for v in table.values()]
        if len(set(keys)) != len(keys):
            raise AssertionError(f"{path.name}: (cell,slot) 不唯一，服务器寻址会有歧义")
        if any(k[2] > 0xFF for k in keys):
            raise AssertionError(f"{path.name}: slot > 255，事件末字节无法表示")
        return table, {"file": path.name, "variant": f"{prefix}{num}", "segmented": True}
    main_path = lka_dir / (stem + ".lka.dvpl")
    if not main_path.exists():
        return {}, {"file": None, "variant": None, "segmented": False}
    return _read_lka(main_path), {
        "file": main_path.name, "variant": None, "segmented": False
    }


def validate_segmented_lka(lka: dict, scene_pos: dict[int, tuple[float, float]],
                           table_name: str) -> None:
    """分段表的结构自检（语义不变量，非统计阈值）：

    - 每个 key 必须解析到当前 .sc2 的实体（遗留主表的特征就是大量悬空 key）；
    - 值里的 100m 格子码必须等于该实体位置所在格子（格子码错 = 编号体系不是这张图）。

    不满足即 fail-closed——宁可不出包，也不把事件联到错误物体。
    """
    for sid, ref in lka.items():
        eid = int(sid)
        if eid not in scene_pos:
            raise AssertionError(f"{table_name}: lka key {eid} 在当前 .sc2 中不存在")
        cx, cy = ref["cell"]
        need = (math.floor(scene_pos[eid][0] / 100), math.floor(scene_pos[eid][1] / 100))
        if (cx, cy) != need:
            raise AssertionError(
                f"{table_name}: lka key {eid} 格子码 {(cx, cy)} ≠ 实体位置格 {need}"
            )


def export_map(entry: X.MapEntry, game_data: pathlib.Path, types: dict,
               registry: list[X.MapEntry]) -> dict:
    sc2_rel = entry.local_name.replace("/", "\\")
    sc2_path = (game_data / "3d" / "Maps" / sc2_rel).with_name(
        pathlib.PurePath(sc2_rel).name + ".dvpl"
    )
    scene = read_sc2(decode_dvpl(sc2_path.read_bytes()))

    # 自检基准：所有实体 id → 场景水平位置（与 lka 值的格子码同域）
    scene_pos: dict[int, tuple[float, float]] = {}
    for _, ent in X.iter_entities_recursive(scene):
        eid = ent.get("id")
        if eid is None:
            continue
        t = (X.world_transform(ent).get("translation") or [0, 0, 0])
        scene_pos[int(eid)] = (float(t[0]), float(t[1]))

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
    lka, lka_meta = parse_lka(entry, game_data, registry)
    if lka_meta["segmented"]:
        validate_segmented_lka(lka, scene_pos, lka_meta["file"])

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
        # 选表溯源：变体前缀+序号（如 dt1）；主表回退时为 null。
        # additive 字段，消费方忽略未知键。
        "lkaVariant": lka_meta["variant"],
        "lkaFile": lka_meta["file"],
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
        doc = export_map(entry, args.game_data, types, registry)
        kinds = {}
        for i in doc["instances"]:
            kinds[i["kind"]] = kinds.get(i["kind"], 0) + 1
        print(f"{doc['key']}: instances={doc['instanceCount']} {kinds} "
              f"typeTable={doc['typeTableSize']} lka={doc['lkaSize']}"
              f"({doc['lkaVariant'] or 'main'})")
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
