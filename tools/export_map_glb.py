#!/usr/bin/env python3
"""按 WoT Blitz 客户端自身的加载方式导出地图静态场景（GLB + 地面贴图 + 元数据）。

管线与客户端一致（证据见各函数 docstring）：
  1. 注册表：Data/maps.yaml（回放数字 id → localName=space/sc2 路径）联查
     Data/Strings/en.yaml 的 `#maps:<dir>:<space>/<space>.sc2: "<显示名>"` 得到
     minimap 目录与本地化显示名——与客户端 arenaTypeID → maps.yaml 同一条链；
  2. 场景：3d/Maps/<space>/<space>.sc2（DAVA SceneFileV2 + KeyedArchive）实体树；
     实例收集镜像 RenderObject::UpdateActiveRenderBatchesFromCollection 语义
     （ro.flags bit0 可见位、rbN.lodIndex/rbN.switchIndex 与目标 LOD/开关态匹配，
     -1 为通配；开关态取 StateSwitcherComponent.activeState 初始值）；
  3. 几何：rb.datasource → 同名 .scg（SCPG）PolygonGroup 顶点/索引流；
  4. 材质：rb.nmatname → .sc2 #dataNodes 的 NMaterial 节点，沿 parentMaterialKey
     继承链合并贴图槽（albedo/lightmap/colorTexture/...）——贴图按客户端真实
     路径解析，不做任何名字猜测；
  5. 贴图解码：.dx11.dds.dvpl（BC1/2/3）与 .dx11.pvr.dvpl（DAVA 自有 PVR3 容器，
     未压缩 RGBA4444 + mipmap 链，含 CRC_ 子块）；
  6. 地形尺度：Landscape 渲染对象 bbox（世界包围盒）给出 span/zmin/zmax，
     连同注册表信息写入 <space>.json sidecar，供后端 terrain 接口使用。

输出（output-dir，默认 data/cache/maps/，按 space 目录命名——同一 space 的多张
变体地图共用一份场景）：
    <space>.glb           静态场景（游戏世界系，米，z 上；材质含真贴图/透明树叶）
    <space>.ground.webp   地面烘焙贴图（4096²，客户端 tilemask-fp.sl 公式，上=+z/北）
    <space>.ground.{cm,tile,mask,hmap}.webp + .layers.json
                          分层地表（原始行序）：前端按客户端着色器实时合成，
                          tile 原生分辨率平铺——清晰度对齐客户端
    <space>.json          元数据（注册表 + 地形尺度 + 统计）
前端用 qFrame（Ry(π)·Rx(-π/2)）旋到回放场景系，与坦克 GLB 同一约定。

用法：
    python tools/export_map_glb.py                     # 导出全部（按 space 去重，并行 jobs=4）
    python tools/export_map_glb.py --map WinterMalinovka --map 5
    python tools/export_map_glb.py --ground-only --map Mines
    python tools/export_map_glb.py --jobs 8            # 调整并行进程数

进度：每张图完成即实时打印（含剩余时间估算），并原子更新
<output-dir>/_export_status.json（total/done/failed/finished/results）——
外部可随时读取该文件查询导出是否完成，无需等待进程退出。
"""

from __future__ import annotations

import argparse
import bisect
import concurrent.futures
import io
import json
import math
import pathlib
import random
import re
import struct
import sys
import os
import time

import numpy as np

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

try:
    import imagecodecs
except ImportError:  # 地面贴图/DDS 解码需要
    imagecodecs = None

try:
    from PIL import Image
except ImportError:
    Image = None

from wotb_sc2 import Reader, decode_dvpl, read_archive, read_sc2  # noqa: E402
from wotb_scg import (  # noqa: E402
    decode_bytes,
    decode_polygon_indices,
    decode_polygon_positions,
    read_scg,
)

# ---------------------------------------------------------------------------
# 注册表：maps.yaml（id → localName）+ en.yaml #maps:（dir → sc2/显示名）
# ---------------------------------------------------------------------------


class MapEntry:
    """一张可玩地图的客户端注册信息。"""

    def __init__(self, map_id: int, key: str, local_name: str,
                 minimap_dir: str | None, display: str | None):
        self.map_id = map_id
        self.key = key
        self.local_name = local_name              # "12_malinovka_ma/12_malinovka_ma.sc2"
        self.space = local_name.split("/")[0]     # 3d/Maps 下的目录
        self.minimap_dir = minimap_dir            # Gfx/UI/BattleScreenHUD/minimap/<dir>
        self.display = display or key

    def __repr__(self) -> str:
        return f"MapEntry({self.map_id}, {self.key!r}, {self.space})"


def _parse_maps_yaml(text: str) -> dict[int, tuple[str, str]]:
    """maps.yaml 最小行解析：`    <key>:` 下的 `id:` / `localName:`。返回 id → (key, localName)。"""
    out: dict[int, tuple[str, str]] = {}
    key = None
    entry_id = None
    for line in text.splitlines():
        m = re.match(r"^    ([A-Za-z0-9_]+):\s*$", line)
        if m:
            key, entry_id = m.group(1), None
            continue
        m = re.match(r"^\s+id:\s*(\d+)\s*$", line)
        if m and key:
            entry_id = int(m.group(1))
            continue
        m = re.match(r"^\s+localName:\s*\"?([^\"\s]+)\"?\s*$", line)
        if m and key and entry_id is not None:
            out[entry_id] = (key, m.group(1))
    return out


def _parse_en_yaml_maps(text: str) -> dict[str, list[tuple[str, str]]]:
    """en.yaml 的 `\"#maps:<dir>:<space>/<space>.sc2\": \"<显示名>\"` 条目。

    同一 sc2 路径可能有多条（rudniki / rudniki_01/02/03 出生点变体），全部收集。
    """
    out: dict[str, list[tuple[str, str]]] = {}
    for m in re.finditer(r'"#maps:([^:]+):([^"]+)":\s*"([^"]*)"', text):
        out.setdefault(m.group(2), []).append((m.group(1), m.group(3)))
    return out


def load_registry(game_data: pathlib.Path) -> list[MapEntry]:
    """联查两张客户端表得到全量注册信息（与客户端 arenaTypeID→maps.yaml 同链）。

    显示名取 en.yaml 中 dir 与 maps.yaml 键一致的条目（基础变体），无则取首条。
    """
    maps_text = decode_dvpl((game_data / "maps.yaml.dvpl").read_bytes()).decode("utf-8", "replace")
    en_text = decode_dvpl(
        (game_data / "Strings" / "en.yaml.dvpl").read_bytes()).decode("utf-8", "replace")
    by_sc2 = _parse_en_yaml_maps(en_text)
    entries = []
    for map_id, (key, local_name) in sorted(_parse_maps_yaml(maps_text).items()):
        variants = by_sc2.get(local_name, [])
        pick = next((v for v in variants if v[0] == key), variants[0] if variants else None)
        dir_name, display = pick if pick else ("", "")
        entries.append(MapEntry(map_id, key, local_name, dir_name or None, display or None))
    return entries


def resolve_entry(entries: list[MapEntry], name: str) -> MapEntry | None:
    """解析 CLI/请求参数：纯数字=回放地图 id；否则显示名/键（去空白归一）。"""
    s = name.strip()
    if s.isdigit():
        for e in entries:
            if e.map_id == int(s):
                return e
        return None

    def norm(x: str) -> str:
        return re.sub(r"[^A-Za-z0-9]", "", x).lower()

    for e in entries:
        if norm(e.display) == norm(s):
            return e
    for e in entries:
        if norm(e.key) == norm(s):
            return e
    return None


# ---------------------------------------------------------------------------
# 场景/几何/材质（客户端格式，解析复用 tools/wotbtools）
# ---------------------------------------------------------------------------


MAGIC_PREFIX = b"PVR" + bytes([3])
MAGIC_MARKER = MAGIC_PREFIX + b"CRC_"


def extract_string_table(raw: bytes) -> dict[int, str]:
    """SC2 KeyedArchive 头部的 fastname 字符串表（id → 字符串）。"""
    reader = Reader(raw)
    reader.take(4)
    reader.u32(), reader.u32()
    read_archive(reader)
    desc = reader.u32()
    reader.take(desc)
    reader.take(2)
    ver = reader.u16()
    if ver != 2:
        return {}
    n = reader.u32()
    strings = [reader.text(reader.u16()) for _ in range(n)]
    ids = [reader.u32() for _ in range(n)]
    return dict(zip(ids, strings, strict=True))


def iter_entities_recursive(container: dict, path: str = "$"):
    """深先序遍历实体树（含嵌套 #hierarchy），镜像客户端场景节点遍历。"""
    hierarchy = container.get("#hierarchy")
    if not isinstance(hierarchy, list):
        return
    for index, entity in enumerate(hierarchy):
        if not isinstance(entity, dict):
            continue
        entity_path = f"{path}.#hierarchy[{index}]"
        yield entity_path, entity
        yield from iter_entities_recursive(entity, entity_path)


def components_of(entity: dict) -> list[dict]:
    comps = entity.get("components")
    if not isinstance(comps, dict):
        return []
    return [c for c in comps.values() if isinstance(c, dict)]


def component_by_type(entity: dict, type_name: str) -> dict | None:
    return next((c for c in components_of(entity) if c.get("comp.typename") == type_name), None)


def map_border_bounds(scene: dict) -> tuple[float, float, float, float] | None:
    """游戏内实际战场边界：场景实体的 MapBorderComponent → mbc.rect（16B = 4×f32：
    xMin, yMin, xMax, yMax，米；与回放坐标同系）。

    来源同 WotBTools map-semanticizer 的 map_border()（common/map-semantics/*.semantic.json
    的 playableBoundsMeters 即此值）——比真实地图 worldBounds(±300) 小，是战场可玩区。
    缺组件/长度异常时返回 None，调用方回退 worldBounds。
    """
    for _path, entity in iter_entities_recursive(scene):
        comp = component_by_type(entity, "MapBorderComponent")
        if comp is None:
            continue
        rect = decode_bytes(comp.get("mbc.rect"))
        if rect is not None and len(rect) == 16:
            x_min, y_min, x_max, y_max = struct.unpack("<4f", rect)
            return (x_min, y_min, x_max, y_max)
    return None


def world_transform(entity: dict) -> dict:
    """TransformComponent 里客户端已烘焙的世界变换（与父链累乘等价）。"""
    t = component_by_type(entity, "TransformComponent") or {}
    vec = lambda key, n, default: ([float(x) for x in t[key]]
                                   if isinstance(t.get(key), list) and len(t[key]) == n
                                   else list(default))
    return {
        "translation": vec("tc.worldTranslation", 3, (0.0, 0.0, 0.0)),
        "scale": vec("tc.worldScale", 3, (1.0, 1.0, 1.0)),
        "rotation": vec("tc.worldRotation", 4, (0.0, 0.0, 0.0, 1.0)),
    }


RENDER_OBJECT_VISIBLE_FLAG = 1 << 0
SHARED_BATCH_INDEX = -1
# 导出的 LOD/开关目标：初始可见态（客户端进场即 LOD0、StateSwitcher 处于 activeState）
TARGET_LOD = 0
# SpeedTree 远景 billboard（crossed-plane impostor）贴图命名约定（全图库扫描证实）：
# *_planes.tex / *_bb_*.tex / planelod*.tex
IMPOSTOR_TEX_RE = re.compile(r"(?i)planes|_bb_|planelod")
# SpeedTree 远景 LOD 平贴卡片的贴图命名（a_bush1_low1-4 / bush02_low1-2 等实测）
LOD_CARD_TEX_RE = re.compile(r"(?i)_low\d|_low\.")


class MaterialLibrary:
    """.sc2 #dataNodes 的 NMaterial 节点库 + parentMaterialKey 继承链解析。"""

    def __init__(self, scene: dict):
        self.by_id: dict[int, dict] = {}
        for node in scene.get("#dataNodes") or []:
            if not isinstance(node, dict) or node.get("##name") != "NMaterial":
                continue
            raw = node.get("#id")
            if not (isinstance(raw, dict) and isinstance(raw.get("$bytes"), str)):
                continue
            try:
                node_id = struct.unpack("<Q", bytes.fromhex(raw["$bytes"]))[0]
            except ValueError:
                continue
            self.by_id[node_id] = node
        self._cache: dict[int, dict] = {}

    def resolve(self, material_id: int | None) -> dict:
        """沿 parentMaterialKey 向上合并：贴图槽/属性子覆盖父，fxName 取最近非空。"""
        if material_id is None:
            return {}
        if material_id in self._cache:
            return self._cache[material_id]
        merged: dict = {"textures": {}, "properties": {}, "flags": {}, "fxName": None, "materialName": None}
        seen: set[int] = set()
        cur: int | None = material_id
        while isinstance(cur, int) and cur not in seen:
            seen.add(cur)
            node = self.by_id.get(cur)
            if node is None:
                break
            merged["textures"].update(node.get("textures") or {})
            merged["properties"].update(node.get("properties") or {})
            merged["flags"].update(node.get("flags") or {})
            if merged["fxName"] is None and node.get("fxName"):
                merged["fxName"] = node["fxName"]
            if merged["materialName"] is None and node.get("materialName"):
                merged["materialName"] = node["materialName"]
            cur = node.get("parentMaterialKey") if isinstance(node.get("parentMaterialKey"), int) else None
        self._cache[material_id] = merged
        return merged


class ClientMaterialFamily:
    """材质 fx 名 → 客户端 shader 族与 define（决定叶卡/树材质的复刻方式）。

    判定依据是**材质文件**（Data/Materials/<fx>.material，YAML），不是实体类：
    .sc2 里 SpeedTreeObject 类只描述顶点流形态；上屏用哪套着色器由材质的
    `Shader:` 行决定（客户端证据 = Data/Materials/Shaders/Default/speedtree-materials-*.sl
    与 materials-*.sl 两份源码）：

      speedtree-materials + SPHERICAL_LIT/PBR_SPEEDTREE：
          varVertexColor = 0.282094 × SH(L0) × vertexOcclusion × aoMult
      speedtree-materials 其余（legacy for old tree lighting）：
          varVertexColor = color0 × treeLeafColorMul × treeLeafOcclusionMul
                          + treeLeafOcclusionOffset          （**SH 完全不参与**）
      materials（Textured.material 等）：普通受光材质——客户端按 lit 路线渲染
          （如 skit 的芦苇/蕨：SpeedTreeObject 类 + Textured.material，颜色 =
          albedo × flatColor × 场景光照），不适用叶卡的 unlit/SH 染色。

    StandardSpeedTreeAllQualities 之类的模板（MaterialTemplate: ULTRA/HIGH/… 引用）
    沿引用链逐档解析并取并集。
    """

    def __init__(self, game_data: pathlib.Path):
        self.base = game_data / "Materials"
        self._cache: dict[str | None, dict | None] = {}

    def resolve(self, fx_path: str | None) -> dict | None:
        name = (fx_path or "").rsplit("/", 1)[-1] or None
        if name not in self._cache:
            self._cache[name] = self._resolve(name, 0)
        return self._cache[name]

    def _resolve(self, name: str | None, depth: int) -> dict | None:
        if not name or depth > 4:
            return None
        for cand in (self.base / (name + ".dvpl"), self.base / name):
            if not cand.exists():
                continue
            try:
                raw = cand.read_bytes()
                text = (decode_dvpl(raw) if cand.suffix == ".dvpl" else raw).decode("utf-8", "replace")
            except Exception:
                return None
            m = re.search(r"Shader:\s*(\S+)", text)
            # Define 只看 UniqueDefines / QualityDependentUniqueDefines：IgnoreDefines
            # 是"关闭"语义（SpeedTree.material 的 ShadowMap pass 就写着
            # IgnoreDefines: [SPHERICAL_LIT, …]），全文匹配会把它误判成启用。
            defines_text = "\n".join(
                ln for ln in text.splitlines()
                if "Defines" in ln and "IgnoreDefines" not in ln)
            info = {"shader": m.group(1).rsplit("/", 1)[-1] if m else None,
                    "speedtree": bool(m and "speedtree" in m.group(1).lower()),
                    "spherical": "SPHERICAL_LIT" in defines_text}
            if info["shader"] is None:   # 模板：沿 ULTRA/HIGH/… 的材质引用链取并集
                for ref in re.findall(r"(~res:/Materials/[\w.\-]+\.material)", text):
                    sub = self._resolve(ref.rsplit("/", 1)[-1], depth + 1)
                    if sub is not None:
                        info["speedtree"] |= sub["speedtree"]
                        info["spherical"] |= sub["spherical"]
                        info["shader"] = info["shader"] or sub["shader"]
            return info
        return None


VARIANT_LABEL_RE = re.compile(r"^([a-z]+)(\d+)$")


def entity_variant_label(entity: dict) -> str | None:
    """实体 LabelComponent 的变体标签（形如 md1/dt2/er0：字母前缀+数字序号）。

    多变体地图（Dead Rail/.Middleburg 等 9 图）按变体把出生点/边界/专属布景
    打成 mdN 组，客户端一局只激活一组——按变体过滤的实现见
    resolve_variant_map（打标随包，前端按对局 map_id 剔除非本组节点）。
    非单一标签 / 不匹配字母+数字 → None（恒导出）。
    """
    for c in components_of(entity):
        if isinstance(c, dict) and c.get("comp.typename") == "LabelComponent":
            labels = c.get("lc.labels") or []
            if len(labels) == 1:
                m = VARIANT_LABEL_RE.match(str(labels[0]))
                return m.group(0) if m else None
            return None
    return None


def scene_variant_labels(scene: dict) -> list[str]:
    """场景中出现的变体标签，按数字序号升序（组序 ↔ 变体序，见 resolve_variant_map）。"""
    labels = set()
    for _path, ent in iter_entities_recursive(scene):
        lab = entity_variant_label(ent)
        if lab:
            labels.add(lab)
    return sorted(labels, key=lambda s: int(VARIANT_LABEL_RE.match(s).group(2)))


def resolve_variant_map(space_entries: list, scene: dict) -> dict[int, str]:
    """同 space 的注册表条目 ↔ 变体组 按序配对 → {map_id: label}。

    组序 = 标签数字升序（er0/er1/er2、md1/md2/md3）；key 序 = 基础键（无 _NN
    后缀）在前、其余按后缀数字升序。9 张多变体图全部吻合（erlenberg 的 _01↔er1
    组内 spawn 名却是 Spawn_02_*，证明按序数而非字面后缀配对）。组数与 key 数
    不一致 = 口径外结构，返回 {}（不写映射，前端不裁剪——fail-open 保持现状）。
    """

    def key_order(e) -> tuple:
        m = re.search(r"_0*(\d+)$", e.key)
        return (0, -1) if m is None else (1, int(m.group(1)))

    labels = scene_variant_labels(scene)
    ordered = sorted(space_entries, key=key_order)
    if not labels or len(labels) != len(ordered):
        return {}
    return {e.map_id: lab for e, lab in zip(ordered, labels)}


def collect_renderables(scene: dict) -> dict:
    """按客户端批次激活规则收集场景内容。

    返回 {
      instances: [(entity_name, world_transform, datasource, material_id, path, cls)],
      landscape: {...} | None,      # bbox/heightmap/matname
      variant_labels: {entity_path: "mdN"} | {},
    }
    """
    instances: list = []
    landscape = None
    lod_batch_dropped = 0
    variant_labels: dict = {}

    for entity_path, entity in iter_entities_recursive(scene):
        _vlab = entity_variant_label(entity)
        if _vlab:
            variant_labels[entity_path] = _vlab
        render = component_by_type(entity, "RenderComponent")
        if render is None:
            continue
        ro = render.get("rc.renderObj")
        if not isinstance(ro, dict):
            continue
        cls = str(ro.get("##name", ""))

        # 摧毁态/变体形态（StateSwitcher 高状态子树 State1 与独立 *_crash/_crush/
        # *_broken 兄弟实体）：按 (cell,slot) 事件切换渲染的目标网格——以 "D_"
        # 前缀节点名随包导出（前端：事件到达时隐藏同位置完好节点、显示 D_ 节点）。
        # 树倒（prop=3）无替换网格（倒伏动画作用于原节点），不在此列。
        nm = str(entity.get("name") or "")
        _nm_dest = bool(re.search(r"State ?[1-9]+$", nm) or re.search(r"_(crash|crush|broken)$", nm))
        if not _nm_dest:
            # 实体级初始可见性：bit0/bit1 为隐藏（blocking_volume=1、摧毁态 State-1=2）；
            # 值 4（草地系统实体）客户端照常渲染；摧毁态变体（_nm_dest）跳过此检
            # ——隐藏正是其初始态，几何仍需导出
            if entity.get("visibility", 0) & 3:
                continue
            # 实体级初始可见性：bit0/bit1 为隐藏（blocking_volume=1、摧毁态 State-1=2）；
            # 值 4（草地系统实体）客户端照常渲染
            if entity.get("visibility", 0) & 3:
                continue

        # 雾体积（如 malinovka fog_ma01.sc2）：客户端经雾管线渲染成半透明，
        # 静态导出会变成罩住半张图的白色实体网格——跳过
        if "fog" in str(name := entity.get("name") or "").lower():
            continue

        if cls == "Landscape":
            bbox = ro.get("bbox")
            hmap = ro.get("hmap")
            matname = ro.get("matname")
            entry = {}
            if isinstance(bbox, dict) and isinstance(bbox.get("$bytes"), str):
                try:
                    vals = struct.unpack("<6f", bytes.fromhex(bbox["$bytes"]))
                    entry["worldBounds"] = {"min": list(vals[:3]), "max": list(vals[3:])}
                except ValueError:
                    pass
            if isinstance(hmap, str):
                entry["heightmap"] = hmap
            if isinstance(matname, int):
                entry["matname"] = matname
            landscape = entry or None
            continue

        if cls not in ("Mesh", "SpeedTreeObject"):
            continue  # MapBorderRenderObject 等调试/边框对象

        # RenderObject 可见位（缺省即可见，镜像 RenderObject::Load 序列化缺省）；
        # 摧毁态变体初始隐藏（bit0=0、仅 bit13）——_nm_dest 放行（隐藏正是其语义）
        if not _nm_dest:
            flags = ro.get("ro.flags")
            if isinstance(flags, int) and not (flags & RENDER_OBJECT_VISIBLE_FLAG):
                continue
            if ro.get("ro.notShadowOnly") is False:
                continue

        switcher = component_by_type(entity, "StateSwitcherComponent")
        active_switch = switcher.get("ssc.activeState", 0) if switcher else 0
        if not isinstance(active_switch, int):
            active_switch = 0

        name = entity.get("name")
        transform = world_transform(entity)
        batches = ro.get("ro.batches")
        if not isinstance(batches, dict):
            continue
        batch_items = sorted(batches.items())
        # 客户端 RenderObject 按 rbN.lodIndex（N 为去零填充的批号，如 rb0/rb7，
        # 与批字典键 "0000" 不同——此前误用补零键导致恒读不到、全按通配处理）
        # 距离切换 LOD；实测 lod0 = 最高精度（fir: b0-2=完整组；电线杆
        # 210v=lod0/144v=lod1/28v=lod2）。非 SpeedTree 实体只保留 LOD0 批；
        # SpeedTree 的 lod 组选择交由 export_map 的顶点量择优。
        if cls != "SpeedTreeObject":
            lod_comp = component_by_type(entity, "LodComponent")
            if lod_comp is not None and len(batch_items) > 1:
                lod_filtered = [(k, b) for k, b in batch_items
                                if ro.get(f"rb{int(k)}.lodIndex", SHARED_BATCH_INDEX)
                                in (TARGET_LOD, SHARED_BATCH_INDEX)]
                batch_items = lod_filtered or batch_items[:1]
                lod_batch_dropped += max(0, len(batches) - len(batch_items))
        for batch_index, batch in batch_items:
            if not isinstance(batch, dict):
                continue
            bi_num = int(batch_index)
            lod = ro.get(f"rb{bi_num}.lodIndex", SHARED_BATCH_INDEX)
            switch = ro.get(f"rb{bi_num}.switchIndex", SHARED_BATCH_INDEX)
            # SpeedTree 的 lod 不过滤（_low 变体的卡片组在 lod1 而非 lod0），
            # 全部放行给 export_map 的"总顶点量最大 lod 组"择优；非 SpeedTree
            # 按 LOD0/通配过滤
            if cls != "SpeedTreeObject" and lod not in (TARGET_LOD, SHARED_BATCH_INDEX):
                continue
            # 摧毁态子树的批次 switchIndex 指向高状态（父 switcher activeState=0），
            # 按 _nm_dest 放行（其几何就是要导出的损毁形态）
            if not _nm_dest and switch not in (active_switch, SHARED_BATCH_INDEX):
                continue
            datasource = batch.get("rb.datasource")
            if not isinstance(datasource, int):
                continue
            material_id = batch.get("rb.nmatname")
            # SpeedTree 的 SH 环境光 L0（speedtree-materials-fp.sl:
            # baseColor *= varVertexColor）→ 材质染色。SH L0 = RGB 三通道：
            # 常见图 = √π 灰（erlenberg/medvedkovo 恒 1.772454）；karelia/neptune
            # 等图为真实每树彩色环境（karelia (0.37,0.50,0.50) 冷调黄昏）。
            # **按字面值导出，不做 √π 归一化、只取 R**（2026-10-07 勘误：
            # 归一化理论源于把地面 colormap 误当树冠烘焙真值——对照实验证明
            # colormap 无树冠信号；客户端按字面 SH 乘，钳制/归一化才是发灰主因）
            sh_l0 = None
            if cls == "SpeedTreeObject":
                sh = ro.get("sto.SHCoeff")
                if isinstance(sh, dict) and isinstance(sh.get("$bytes"), str):
                    try:
                        raw = bytes.fromhex(sh["$bytes"])[:12]
                        if len(raw) == 12:
                            sh_l0 = struct.unpack("<3f", raw)
                        else:
                            sh_l0 = struct.unpack("<f", bytes.fromhex(sh["$bytes"])[:4])[0]
                    except (ValueError, struct.error):
                        pass
            out_name = ("D_" + name) if _nm_dest else name
            instances.append((out_name, transform, datasource,
                              material_id if isinstance(material_id, int) else None,
                              entity_path, cls, lod, sh_l0))
    return {"instances": instances, "landscape": landscape,
            "lod_batches_dropped": lod_batch_dropped,
            "variant_labels": variant_labels}



# 顶点布局（SCG 交错顶点，实测 7 种 vertexFormat 反推，bit 求和 == stride）
# DAVA 顶点位表（权威来源：dava.engine `Sources/Internal/Render/RenderBase.h`
# `EVF_*` 位定义 :158-177 + `GetVertexSize` :213-261）。逐位语义：
#   0 VERTEX 12 | 1 NORMAL 12 | 2 COLOR 4       | 3-6 TEXCOORD0-3 各 8
#   7 TANGENT 12 | 8 BINORMAL 12 | 9 HARD_JOINTINDEX 4 | 10 PIVOT4 16
#   11 PIVOT_DEPRECATED 12 | 12 FLEXIBILITY 4 | 13 ANGLE_SIN_COS 8
#   14 JOINTINDEX 16 | 15 JOINTWEIGHT 16 | 16-19 CUBETEXCOORD0-3 各 12
# 此前本表 bit9/10/12/13 写成 16/8/12/16（四位全错）、并缺 5/6/11/14/15/16-19。
# 之所以未曾出错：偏移累加只遍历 bit0-2（见 decode_group_uvs），恰好落在两表一致的
# 区间。按权威表收口后，本表可用于 stride 校验与高位流定位。
VERTEX_LAYOUT_BITS = {0: 12, 1: 12, 2: 4, 3: 8, 4: 8, 5: 8, 6: 8, 7: 12, 8: 12,
                      9: 4, 10: 16, 11: 12, 12: 4, 13: 8, 14: 16, 15: 16,
                      16: 12, 17: 12, 18: 12, 19: 12}


def decode_group_uvs(group: dict):
    """解出 UV0（bit3）与 UV1（bit4）。

    客户端 materials-fp.sl：albedo 采样 texCoord0；ALPHA_MASK 材质的
    alphamask（效果层镂空）采样 texCoord1。偏移按低于 UV 位的功能位累计——
    普通网格位和恒等于 stride；SpeedTree 卡片组高位语义不同导致位和≠stride，
    但 UV 仍在低位累计偏移处。
    返回 (uv0, uv1或None, 使用的通道bit)；无有效 UV → None。
    """
    vf = group.get("vertexFormat")
    vc = group.get("vertexCount")
    payload = decode_bytes(group.get("vertices"))
    if not isinstance(vf, int) or not isinstance(vc, int) or vc <= 0 or payload is None:
        return None
    stride, rem = divmod(len(payload), vc)
    if rem or stride < 12:
        return None
    low_off = 0
    for b in range(3):
        if vf >> b & 1:
            sz = VERTEX_LAYOUT_BITS.get(b)
            if sz is None:
                return None
            low_off += sz
    arr = np.frombuffer(payload, dtype=np.uint8).reshape(vc, stride)
    floats = arr.view("<f4")
    found = {}
    # UV 偏移按"实际置位的 TEXCOORD 位"逐个累加（DAVA GetVertexSize 口径）。旧实现对
    # bit4 硬编码 `low_off + 8`、默认 bit3 必然存在：mask 0x11（只置 TEXCOORD1、无
    # TEXCOORD0，本机地图实测 20 个组）会把起点算到 stride 之外，被下面的长度守卫挡掉
    # → 该组唯一的 UV 通道被静默丢弃。坦克侧（export_tank_glb.py:307）本就只在位存在时
    # 累加，两侧口径现已一致。
    uv_off = low_off
    for bit in (3, 4):
        if not (vf >> bit) & 1:
            continue
        if uv_off + 8 <= stride:
            u = floats[:, uv_off // 4]
            v = floats[:, uv_off // 4 + 1]
            # 只拒非有限值（偏移错位才会出 NaN/inf）。不可按幅值拒绝：平铺 UV 的
            # 合法倍数没有上限（REPEAT 采样），铁轨条 v=-118 实测——旧幅值守卫
            # (>64) 把真 TEXCOORD0 错选成位 4 备用对，铁轨整条拉成一段灰带
            if np.isfinite(u).all() and np.isfinite(v).all():
                found[bit] = list(zip(u.tolist(), v.tolist()))
        uv_off += VERTEX_LAYOUT_BITS[bit]
    if 3 in found:
        return found[3], found.get(4), 3
    if 4 in found:
        return found[4], None, 4
    return None


def _vertex_offsets(vf: int) -> tuple[dict, int] | None:
    """掩码 → {位号: 字节偏移} 与每顶点总尺寸。

    DAVA 的 EVF 位分配**本身就是顶点流的内存顺序**：PolygonGroup.cpp:49-166 按
    V→N→C→TC0..TC3→TANGENT→BINORMAL→HARD_JOINTINDEX→PIVOT4→PIVOT_DEPRECATED→
    FLEXIBILITY→ANGLE_SIN_COS→JOINTINDEX→JOINTWEIGHT→CUBETEXCOORD0..3 逐位写入，
    与位号升序一致（另见 RenderBase.h `GetVertexSize` 的逐位求和）。故某位的偏移
    = 所有低于它的置位尺寸之和，任意掩码通用。
    """
    off = {}
    cur = 0
    for b in range(20):
        if not (vf >> b) & 1:
            continue
        sz = VERTEX_LAYOUT_BITS.get(b)
        if sz is None:
            return None
        off[b] = cur
        cur += sz
    return off, cur


def decode_speedtree_cards(group: dict):
    """SpeedTree 叶卡解算：**掩码驱动**，取代原先按 stride 硬编码的 56B / 92B 两条路径。

    判据只看掩码是否含 EVF_PIVOT4(bit10)，列偏移由掩码算出，故 56B(0x360d) / 92B(0x378f)
    只是其中两种。批内按 `pivot.w` 双峰切分：
      - w=1 → 锚定叶卡（billboard；客户端 speedtree-materials-vp.sl 在【视空间】用
        pivot + 角点偏移重建，叶卡恒面向相机）
      - w=0 → 固定朝向叶片 / 刚体，交静态路径照常导出
    本机地图实测：含 bit10 共 **668 组 / 10 种掩码**，旧的两条硬编码路径只覆盖
    0x360d 与 0x378f；其余（0x340d 94 组、0x358f 23、0x359f 4，以及 0x360d 里
    混有 w=0 的组）会整组掉进静态路径 → pivot 被忽略 → 叶片固定朝向。
    几何列（本机实测布局与位表推出的偏移一致）：
      pos@off[0] · COLOR0 u8x4@off[2] · uv0@off[3] · pivot.xyz@off[10] · pivot.w@off[10]+12

    返回 dict(anchors/corners/colors/occ_mean/uvs/indices/rigid)；rigid 为 None 表示
    整组都是卡片（纯叶卡批）。守卫 fail-closed：无 bit0/2/3/10、w 非严格 0/1 双峰、
    卡片顶点数为 0、三角形跨界（卡↔刚体共享顶点）、卡片无三角形 → None（整组走静态，
    与原行为一致）。
    """
    vf = group.get("vertexFormat")
    vc = group.get("vertexCount")
    payload = decode_bytes(group.get("vertices"))
    if not isinstance(vf, int) or not isinstance(vc, int) or vc <= 0 or payload is None:
        return None
    if not (vf >> 10) & 1:
        return None
    lo = _vertex_offsets(vf)
    if lo is None:
        return None
    off, total = lo
    if not all(b in off for b in (0, 2, 3, 10)):
        return None
    if len(payload) != vc * total:
        return None
    arr = np.frombuffer(payload, dtype=np.uint8).reshape(vc, total)
    f = arr.view("<f4")
    pos_off, col_off, uv_off, piv_off = off[0], off[2], off[3], off[10]
    pos = f[:, pos_off // 4:pos_off // 4 + 3]
    piv = f[:, piv_off // 4:piv_off // 4 + 3]
    w = f[:, (piv_off + 12) // 4]
    # pivot.w 实测严格双峰（0 或 1，无中间值）——非双峰说明偏移算错，fail-closed
    if not bool(((np.abs(w) < 1e-4) | (np.abs(w - 1) < 1e-4)).all()):
        return None
    card_idx = np.where(np.abs(w - 1) < 1e-4)[0]
    if len(card_idx) == 0:
        return None
    if not (np.isfinite(pos).all() and np.isfinite(piv).all()):
        return None
    tri = group_triangles(group, decode_polygon_indices(group))
    if not tri:
        return None
    card_set = set(card_idx.tolist())
    triplets = list(zip(tri[0::3], tri[1::3], tri[2::3]))
    # 三角形不得跨界：卡↔刚体若共享顶点，billboard 重建与静态几何会互相撕裂
    if any((a in card_set) != (b in card_set) or (b in card_set) != (c in card_set)
           for a, b, c in triplets):
        return None
    card_tri = [i for t in triplets if t[0] in card_set for i in t]
    if not card_tri:
        return None
    colors = arr[:, col_off:col_off + 4].astype(np.float32) / 255.0
    remap = {old: new for new, old in enumerate(card_idx.tolist())}
    # 锚点/角点：w=1 ⇒ anchor=pivot、corner=pos−pivot（w 混合时按 w 插值，纯卡批等价）
    anchor = pos * (1.0 - w[:, None]) + piv * w[:, None]
    corner = pos - anchor
    rigid_idx = np.where(np.abs(w) < 1e-4)[0]
    rigid = None
    if len(rigid_idx):
        rigid_rem = {old: new for new, old in enumerate(rigid_idx.tolist())}
        rigid = {
            "positions": pos[rigid_idx].tolist(),
            "indices": [rigid_rem[i] for t in triplets
                        if t[0] not in card_set for i in t],
            "uvs": f[rigid_idx][:, uv_off // 4:uv_off // 4 + 2].tolist(),
            "uvs1": None,
            # COLOR_0 顶点遮挡必须随刚体子集导出：混合组（如 Spruce w=1 占比仅
            # 0.25）的 w=0 子集是【固定朝向叶片】而非树枝（三角形尺寸与卡片叶
            # 同量级、贴图同为叶图集）——客户端对两种叶片都乘 varVertexColor；
            # 漏导则固定叶全亮、billboard 叶带 AO 偏暗 = 同树叶片双色
            # （2026-10-07 erlenberg 实测报障）。前端 ST| 静态材质按
            # vertexColors 消费（GLTFLoader 对带 COLOR_0 的几何自动置位）。
            "colors": colors[rigid_idx].reshape(-1).tolist(),
        }
    return {
        "anchors": anchor[card_idx].tolist(),
        "corners": np.concatenate([corner[card_idx], w[card_idx][:, None]], axis=1).reshape(-1).tolist(),
        "colors": colors[card_idx].reshape(-1).tolist(),
        "occ_mean": round(float(colors[card_idx, 0].mean()), 4),
        "uvs": f[card_idx][:, uv_off // 4:uv_off // 4 + 2].tolist(),
        "indices": [remap[i] for i in card_tri],
        "rigid": rigid,
    }


def strip_to_triangles(seq: list[int]) -> list[int]:
    """三角条带 → 三角形列表（标准交替绕序；退化三角形剔除）。"""
    out: list[int] = []
    for i in range(2, len(seq)):
        a, b, c = seq[i - 2], seq[i - 1], seq[i]
        if a == b or b == c or a == c:
            continue
        out.extend((a, b, c) if i % 2 == 0 else (a, c, b))
    return out


def fan_to_triangles(seq: list[int]) -> list[int]:
    """三角扇 → 三角形列表。"""
    return [x for i in range(1, len(seq) - 1) for x in (seq[0], seq[i], seq[i + 1])]


def group_triangles(group: dict, indices: list[int]) -> list[int] | None:
    """图元索引统一为三角形（primitiveCount 算术自洽判别条带/列表）。"""
    ic = len(indices)
    pc = group.get("primitiveCount")
    ptype = group.get("rhi_primitiveType")
    if isinstance(pc, int):
        if pc == ic // 3 and ic % 3 == 0:
            return indices
        if pc == ic - 2:
            return strip_to_triangles(indices)
    if ptype in (None, 0):
        return indices
    if ptype == 1 and ic >= 3:
        # 实测修正：DAVA 树/灌木等 group 的 rhi_primitiveType=1，但索引序列实际是
        # 三角列表结构（每 3 个成组，如 [0,1,2, 3,4,0, 5,6,3,…]；strip 应为连续
        # 递增 [0,1,2,3,4,…]）。误按 strip 解析会让相邻组共享边 → 产生横跨整个
        # 模型的长条三角形（撕裂），且面数虚高（Linden 68 顶点被解析出 1756 面，
        # 实际 656）。索引数能被 3 整除时按列表处理。
        if ic % 3 == 0:
            return indices
        return strip_to_triangles(indices)
    if ptype == 4 and ic >= 3:
        return fan_to_triangles(indices)
    return indices if ic % 3 == 0 else None


def compute_normals(positions, indices):
    """平滑顶点法线（面积加权）。"""
    normals = [[0.0, 0.0, 0.0] for _ in positions]
    for i in range(0, len(indices) - 2, 3):
        a, b, c = indices[i], indices[i + 1], indices[i + 2]
        ax, ay, az = positions[a]
        bx, by, bz = positions[b]
        cx, cy, cz = positions[c]
        nx, ny, nz = (by - ay) * (cz - az) - (bz - az) * (cy - ay), \
                     (bz - az) * (cx - ax) - (bx - ax) * (cz - az), \
                     (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
        for j in (a, b, c):
            normals[j][0] += nx
            normals[j][1] += ny
            normals[j][2] += nz
    out = []
    for n in normals:
        length = math.sqrt(n[0] ** 2 + n[1] ** 2 + n[2] ** 2)
        out.append((n[0] / length, n[1] / length, n[2] / length) if length > 1e-12 else (0.0, 1.0, 0.0))
    return out


# ---------------------------------------------------------------------------
# 贴图解码：DDS（BC1/2/3 + DX10 扩展头 BCn）与 DAVA PVR3（RGBA4444）
# ---------------------------------------------------------------------------

DDS_FOURCC_TO_BCN = {"DXT1": 1, "DXT3": 2, "DXT5": 3}
# DX10 扩展头：DXGI_FORMAT → BCn（含 typeless/sRGB 变体）
#
# 编号按 dxgiformat.h：70-72 BC1、73-75 BC2、76-78 BC3、79-81 BC4、82-84 BC5、
# **95-97 BC6H、98-100 BC7**。85-93 是 B5G6R5/B5G5R5A1/B8G8R8A8 等非 BCn 的
# 未压缩格式，曾在此被误映射成 BC6H/BC7——本机 15386 个 dds 实测 DX10 只用
# {71,72,74,75,77,78}，故该误映射从未触发；一旦出现 85-93 的 DX10 DDS，旧表会
# 拿 B5G6R5 当 BC6H 解出错误像素而非报错。现按位宽真值表收口，未知码一律
# fail-closed 返回 None（与 export_tank_glb.py 的口径一致）。
DDS_DXGI_TO_BCN = {70: 1, 71: 1, 72: 1, 73: 2, 74: 2, 75: 2, 76: 3, 77: 3, 78: 3,
                   79: 4, 80: 4, 81: 4, 82: 5, 83: 5, 84: 5,
                   95: 6, 96: 6, 97: 6, 98: 7, 99: 7, 100: 7}


def decode_dds(d: bytes, max_dim: int = 1024) -> Image.Image | None:
    """DDS（DXT1/3/5 + DX10 扩展头 BCn）→ PIL RGBA，保留 alpha，长边超限等比缩小。"""
    if d[:4] != b"DDS ":
        return None
    height = struct.unpack_from("<I", d, 12)[0]
    width = struct.unpack_from("<I", d, 16)[0]
    fourcc = d[84:88].decode(errors="replace")
    # 偏移说明（此前注释把标准布局误读成"DAVA 整体 +4"）：DDS 头在 4 字节 magic 之后，
    # 故 dwFourCC 的标准绝对偏移本就是 84（而非 80），DDS_HEADER 结束于 128，DX10 的
    # dxgiFormat 因此落在 128、像素落在 128+20=148——三者全是标准偏移，无任何 DAVA 偏移。
    # 保留 (124, 144) 兜底只为容忍非标准写入方；命中即用，不命中则 fail-closed。
    if fourcc == "DX10":
        bcn = data_off = None
        for dxgi_off, off in ((128, 148), (124, 144)):
            v = struct.unpack_from("<I", d, dxgi_off)[0]
            if v in DDS_DXGI_TO_BCN:
                bcn, data_off = DDS_DXGI_TO_BCN[v], off
                break
    else:
        bcn = DDS_FOURCC_TO_BCN.get(fourcc)
        data_off = 128
    if bcn is None or imagecodecs is None:
        return None
    block = 8 if bcn in (1, 4) else 16
    data = d[data_off:data_off + (width // 4) * (height // 4) * block]
    try:
        rgba = imagecodecs.bcn_decode(data, bcn, shape=(height, width, 4))
    except Exception:
        return None
    img = Image.frombytes("RGBA", (width, height), rgba).transpose(Image.FLIP_TOP_BOTTOM)
    if max(img.size) > max_dim:
        img.thumbnail((max_dim, max_dim), Image.LANCZOS)
    return img


def decode_pvr3(d: bytes, max_dim: int = 1024) -> Image.Image | None:
    """DAVA 自有 PVR3 容器 → PIL RGBA。

    布局：**52B 头**（PVR3/ver/'rgba'/bits[4]/colourSpace/channelType/**height@24**/
    **width@28**/depth/surfaces/faces/mips/**metaDataSize@48**）
    + PVR3 版本字节 0x03 + 'CRC_' 元数据块（len + crc）| 像素（未压缩 mip 链，mip0 在前）。
    像素起点按 CRC_ 子块定位（真字节含 0x03 版本字节，故匹配串是 `PVR` + 0x03 + `CRC_`）；
    尺寸/位深按"载荷长度与 mip 链总量精确相等"校验，头部尺寸字段与载荷不符时
    （如 TileTxRock 头写 512²、载荷实为 1024² 图集）以 2 倍尺寸重试。

    尺寸字段序见 PVRFormatHelper.h 结构体：@24 是 height、@28 是 width（旧实现读反，
    非方形贴图会被 reshape 成转置图；方形图宽度相同故长期未暴露）。
    """
    if d[:4] != b"PVR":
        return None
    fmt = d[8:12]
    bits = tuple(d[12:16])
    # PVR3 头字段序：**height@24、width@28**（PVRFormatHelper.h 结构体顺序 + writer/reader
    # 双向赋值确认）。旧实现读成 w@24/h@28——方形贴图无差别，非方形会按转置尺寸 reshape
    # 而错切。本机 1328 个 PVR 中 5 张非方形（palm_trunk / a_sosna01×2 / env_kr_cactus /
    # monument）；同一资产 palm_trunk 的 DDS 副本为高 512 × 宽 256，与 "PVR@24=512" 互证。
    h0 = struct.unpack_from("<I", d, 24)[0]
    w0 = struct.unpack_from("<I", d, 28)[0]
    mips = max(1, struct.unpack_from("<I", d, 44)[0])
    bpp = sum(bits) // 8 or 2

    def chain(tw, th):
        total, cw, ch = 0, tw, th
        for _ in range(mips):
            total += cw * ch * bpp
            if cw == 1 and ch == 1:
                break
            cw = max(1, cw // 2)
            ch = max(1, ch // 2)
        return total

    # 标准 PVR3 容器（52B 头 + metaSize 元数据，无 DAVA CRC 包装）的 RGB565
    # 无 alpha 格式（如 env_kr_cactus）：mip0 在元数据之后，解码为不透明
    if bits == (5, 6, 5, 0) and fmt[:3] == b"rgb":
        meta_size = struct.unpack_from("<I", d, 48)[0] if len(d) >= 52 else 0
        total0 = w0 * h0 * 2
        start = 52 + meta_size
        if start < 0 or start + total0 > len(d):
            return None
        arr16 = np.frombuffer(d[start:start + total0], dtype="<u2").reshape(h0, w0)
        r = (((arr16 >> 11) & 0x1F) * 255 + 15) // 31
        g = (((arr16 >> 5) & 0x3F) * 255 + 31) // 63
        b = ((arr16 & 0x1F) * 255 + 15) // 31
        rgba = np.stack([r, g, b, np.full_like(r, 255)], axis=-1).astype(np.uint8)
        img = Image.frombytes("RGBA", (w0, h0), rgba.tobytes()).transpose(Image.FLIP_TOP_BOTTOM)
        if max(img.size) > max_dim:
            img.thumbnail((max_dim, max_dim), Image.LANCZOS)
        return img

    starts = []
    marker = d.rfind(b"PVRCRC_")
    if marker != -1:
        head_len = struct.unpack_from("<I", d, marker + 8)[0]
        starts.append(marker + 12 + head_len)
    starts.append(len(d) - chain(w0, h0))

    px = None
    w = h = 0
    for tw, th in ((w0, h0), (w0 * 2, h0 * 2), (w0 * 4, h0 * 4)):
        total = chain(tw, th)
        for start in starts:
            if 0 <= start and len(d) - start == total:
                px = d[start:]
                w, h = tw, th
                break
        if px is not None:
            break
    if px is None or len(px) < w * h * bpp:
        return None

    if bits == (4, 4, 4, 4) and fmt == b"rgba":
        arr = np.frombuffer(px[:w * h * 2], dtype="<u2").reshape(h, w)
        channels = [((arr >> s) & 0xF) * 17 for s in (12, 8, 4, 0)]
    elif bits == (8, 8, 8, 8):
        arr = np.frombuffer(px[:w * h * 4], dtype=np.uint8).reshape(h, w, 4)
        channels = [arr[..., i] for i in range(4)]
    else:
        return None
    rgba = np.stack(channels, axis=-1).astype(np.uint8)
    img = Image.frombytes("RGBA", (w, h), rgba.tobytes()).transpose(Image.FLIP_TOP_BOTTOM)
    if max(img.size) > max_dim:
        img.thumbnail((max_dim, max_dim), Image.LANCZOS)
    return img


def decode_tiletx_planes(d: bytes):
    """解码 TileTx*.tex（四平面打包的地表细节纹理）→ [4 张 512² 灰度细节图]。

    载荷 = 4 个平面顺序存储，每平面 = 单通道 4444 mip 链（512² 时 699050B）。
    """
    if d[:4] != b"PVR":
        return None
    # 同 decode_pvr3：**height@24、width@28**（旧实现读反，非方形贴图会错切）
    h0 = struct.unpack_from("<I", d, 24)[0]
    w0 = struct.unpack_from("<I", d, 28)[0]
    mips = max(1, struct.unpack_from("<I", d, 44)[0])

    def chain(tw, th):
        total, cw, ch = 0, tw, th
        for _ in range(mips):
            total += cw * ch * 2
            if cw == 1 and ch == 1:
                break
            cw = max(1, cw // 2)
            ch = max(1, ch // 2)
        return total

    per = chain(w0, h0)
    marker = d.rfind(b"PVRCRC_")
    px_start = None
    if marker != -1:
        head_len = struct.unpack_from("<I", d, marker + 8)[0]
        cand = marker + 12 + head_len
        if len(d) - cand == per * 4:
            px_start = cand
    if px_start is None:
        cand = len(d) - per * 4
        px_start = cand if cand >= 0 else None
    if px_start is None:
        return None
    px = d[px_start:]
    planes = []
    for k in range(4):
        plane = np.frombuffer(px[per * k:per * k + chain(w0, h0)], dtype="<u2")
        arr = plane[:w0 * h0].reshape(h0, w0)
        v = ((arr >> 12) & 0xF) * 17  # 取高 4 位作灰度（4444 单通道展宽）
        planes.append(v.astype(np.uint8)[::-1].copy())
    return planes


class TextureStore:
    """`.tex` 逻辑路径 → 已解码图像 + alpha 判定（磁盘变体：dx11.dds / dx11.pvr）。"""

    def __init__(self, map_dir: pathlib.Path, global_dirs: list[pathlib.Path]):
        self.roots = [map_dir, *global_dirs]
        self._cache: dict[str, tuple[Image.Image | None, bool]] = {}
        # 颜色均值兜底（UV 缺失时给材质一个从贴图派生的底色）
        self.avg_color: dict[str, tuple[float, float, float] | None] = {}
        # 源容器为 DDS 的贴图（仅诊断/追溯用，不参与嵌入决策）：decode_dds 与
        # decode_pvr3 各自都在解码里翻过一次（消掉容器行序），嵌入 GLB 时统一
        # 再翻回"文件原始行序"——glTF v=0 ↔ 图像首行，与客户端 D3D v=0 =
        # authored top 对齐（DDS 侧地面烘焙链路已逐像素验证；PVR 侧 2026-10-07
        # 由叶卡 UV 窗口覆盖率与标牌贴图方向实测校正，见 _build_material）。
        self.from_dds: dict[str, bool] = {}

    def _candidates(self, tex_path: str) -> list[pathlib.Path]:
        stem = tex_path[:-4] if tex_path.lower().endswith(".tex") else tex_path
        rel = stem[3:] if stem.startswith("../") else stem.lstrip("/")
        rel_path = pathlib.Path(rel)
        suffixes = (".dx11.dds.dvpl", ".dds.dvpl", ".dx11.pvr.dvpl", ".pvr.dvpl")
        out = []
        for root in self.roots:
            base = root / rel_path.parent
            for suf in suffixes:
                p = base / (rel_path.name + suf)
                if p.exists():
                    out.append(p)
        return out

    def get(self, tex_path: str) -> tuple[Image.Image | None, bool]:
        """返回 (图像|None, has_alpha)。解码失败时 (None, False)。"""
        if tex_path in self._cache:
            return self._cache[tex_path]
        img, has_alpha = None, False
        for cand in self._candidates(tex_path):
            try:
                d = decode_dvpl(cand.read_bytes())
            except Exception:
                continue
            is_dds = d[:4] == b"DDS "
            img = decode_dds(d) if is_dds else decode_pvr3(d)
            if img is not None:
                alpha = img.getchannel("A")
                has_alpha = alpha.getextrema()[0] < 250
                self.from_dds[tex_path] = is_dds
                break
        self._cache[tex_path] = (img, has_alpha)
        if img is not None:
            small = img.convert("RGB").resize((1, 1))
            self.avg_color[tex_path] = tuple(v / 255 for v in small.getpixel((0, 0)))
        return self._cache[tex_path]


# ---------------------------------------------------------------------------
# GLB 组装
# ---------------------------------------------------------------------------


class GlbBuilder:
    """手写 GLB2 容器（与旧版一致：单 buffer + JPEG/PNG 内嵌贴图）。"""

    def __init__(self):
        self.buffer = bytearray()
        self.buffer_views: list[dict] = []
        self.accessors: list[dict] = []
        self.meshes: list[dict] = []
        self.materials: list[dict] = []
        self.images: list[dict] = []
        self.textures: list[dict] = []
        self.nodes: list[dict] = []
        self._tex_by_key: dict = {}
        self.total_tris = 0

    def add_view(self, payload: bytes) -> int:
        while len(self.buffer) % 4:
            self.buffer.append(0)
        offset = len(self.buffer)
        self.buffer.extend(payload)
        self.buffer_views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(payload)})
        return len(self.buffer_views) - 1

    def add_texture(self, key, jpeg_or_png: bytes, mime: str) -> int:
        if key in self._tex_by_key:
            return self._tex_by_key[key]
        view = self.add_view(jpeg_or_png)
        self.images.append({"bufferView": view, "mimeType": mime})
        self.textures.append({"source": len(self.images) - 1})
        self._tex_by_key[key] = len(self.images) - 1
        return self._tex_by_key[key]

    def add_shared_mesh(self, key, positions, indices, uvs, normals, material_index,
                        extra_attrs=None) -> int | None:
        """共享几何（按 datasource 去重）；key=datasource。
        extra_attrs: [{"name","type":"VEC4","data":[flat floats]}]（如叶卡 _CORNER/COLOR_0）。"""
        pos_view = self.add_view(struct.pack(f"<{len(positions) * 3}f", *[v for p in positions for v in p]))
        mins = [min(p[i] for p in positions) for i in range(3)]
        maxs = [max(p[i] for p in positions) for i in range(3)]
        self.accessors.append({"bufferView": pos_view, "componentType": 5126, "count": len(positions),
                               "type": "VEC3", "min": mins, "max": maxs})
        attrs = {"POSITION": len(self.accessors) - 1}
        if normals is not None:
            nrm_view = self.add_view(struct.pack(f"<{len(normals) * 3}f", *[v for n in normals for v in n]))
            self.accessors.append({"bufferView": nrm_view, "componentType": 5126, "count": len(normals),
                                   "type": "VEC3"})
            attrs["NORMAL"] = len(self.accessors) - 1
        if uvs is not None:
            uv_view = self.add_view(struct.pack(f"<{len(uvs) * 2}f", *[v for uv in uvs for v in uv]))
            self.accessors.append({"bufferView": uv_view, "componentType": 5126, "count": len(uvs),
                                   "type": "VEC2",
                                   "min": [min(p[0] for p in uvs), min(p[1] for p in uvs)],
                                   "max": [max(p[0] for p in uvs), max(p[1] for p in uvs)]})
            attrs["TEXCOORD_0"] = len(self.accessors) - 1
        for attr in (extra_attrs or []):
            flat = attr["data"]
            ncomp = 4 if attr["type"] == "VEC4" else 3
            view = self.add_view(struct.pack(f"<{len(flat)}f", *flat))
            self.accessors.append({"bufferView": view, "componentType": 5126,
                                   "count": len(flat) // ncomp, "type": attr["type"]})
            attrs[attr["name"]] = len(self.accessors) - 1
        idx_view = self.add_view(struct.pack(f"<{len(indices)}I", *indices))
        self.accessors.append({"bufferView": idx_view, "componentType": 5125,
                               "count": len(indices), "type": "SCALAR"})
        self.meshes.append({"primitives": [{"attributes": attrs,
                                            "indices": len(self.accessors) - 1,
                                            "material": material_index,
                                            "mode": 4}]})
        self.total_tris += len(indices) // 3
        return len(self.meshes) - 1

    def add_node(self, mesh_index: int, transform: dict, name: str | None,
                 variant_label: str | None = None) -> None:
        node = {
            "mesh": mesh_index,
            "translation": transform["translation"],
            "rotation": transform["rotation"],
            "scale": transform["scale"],
            "name": name or None,
        }
        if variant_label:
            # 变体标签随包（GLTFLoader 落 node.userData.mdVariant）：前端按对局
            # map_id 对照 asset.extras.variantByMapId 剔除非本组节点——多变体地图
            # （Dead Rail 等 9 图）的场景 GLB 按 space 共享，组外布景不剔除即成
            # 幻影物体（回放里立着游戏里没有的石头）
            node["extras"] = {"mdVariant": variant_label}
        self.nodes.append(node)

    def finish(self, generator: str, scene_extras: dict | None = None) -> bytes:
        asset: dict = {"version": "2.0", "generator": generator}
        if scene_extras:
            asset["extras"] = scene_extras
        gltf = {
            "asset": asset,
            "scene": 0,
            "scenes": [{"nodes": list(range(len(self.nodes)))}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": self.materials,
            "textures": self.textures,
            "images": self.images,
            "samplers": [{"wrapS": 10497, "wrapT": 10497, "magFilter": 9729, "minFilter": 9987}],
            "accessors": self.accessors,
            "bufferViews": self.buffer_views,
            "buffers": [{"byteLength": len(self.buffer)}],
        }
        if scene_extras:
            # scene 级 extras 由 GLTFLoader 落 gltf.scene.userData（前端主读入口；
            # asset.extras 为冗余备份）
            gltf["scenes"][0]["extras"] = scene_extras
        json_chunk = json.dumps(gltf, separators=(",", ":")).encode()
        while len(json_chunk) % 4:
            json_chunk += b" "
        bin_chunk = bytes(self.buffer)
        while len(bin_chunk) % 4:
            bin_chunk += b"\x00"
        total = 12 + 8 + len(json_chunk) + 8 + len(bin_chunk)
        return (struct.pack("<III", 0x46546C67, 2, total)
                + struct.pack("<II", len(json_chunk), 0x4E4F534A) + json_chunk
                + struct.pack("<II", len(bin_chunk), 0x004E4942) + bin_chunk)


# ---------------------------------------------------------------------------
# 导出主流程
# ---------------------------------------------------------------------------


def find_member(directory: pathlib.Path, space: str, suffix: str) -> pathlib.Path | None:
    exact = directory / f"{space}{suffix}"
    if exact.exists():
        return exact
    for p in sorted(directory.glob(f"*{suffix}")):
        return p
    return None


def load_payload(path: pathlib.Path) -> bytes:
    raw = path.read_bytes()
    return decode_dvpl(raw) if path.name.lower().endswith(".dvpl") else raw


def _border_doc(border: tuple[float, float, float, float] | None) -> dict | None:
    if border is None:
        return None
    x_min, y_min, x_max, y_max = border
    return {"xMin": round(x_min, 3), "yMin": round(y_min, 3),
            "xMax": round(x_max, 3), "yMax": round(y_max, 3)}


def export_map(game_data: pathlib.Path, entry: MapEntry, output_dir: pathlib.Path,
               scenery_only: bool = False) -> dict:
    space = entry.space
    directory = game_data / "3d" / "Maps" / space
    sc2_path = find_member(directory, space, ".sc2.dvpl") or find_member(directory, space, ".sc2")
    if sc2_path is None:
        raise FileNotFoundError(f"{space}: 未找到场景文件（{directory}）")
    scg_ext = ".scg.dvpl" if sc2_path.name.lower().endswith(".dvpl") else ".scg"
    scg_path = directory / (sc2_path.stem.replace(".sc2", "") + scg_ext)
    if not scg_path.exists():
        raise FileNotFoundError(f"{space}: 未找到伴随 SCG（{scg_path}）")

    sc2_raw = load_payload(sc2_path)
    scene = read_sc2(sc2_raw)
    scg = read_scg(load_payload(scg_path))
    groups_by_id = {}
    for g in scg.get("polygonGroups", []):
        if isinstance(g, dict):
            raw = g.get("#id")
            if isinstance(raw, dict) and isinstance(raw.get("$bytes"), str):
                try:
                    groups_by_id[struct.unpack("<Q", bytes.fromhex(raw["$bytes"]))[0]] = g
                except ValueError:
                    pass
    materials = MaterialLibrary(scene)
    renderables = collect_renderables(scene)
    instances = renderables["instances"]
    if not instances:
        raise RuntimeError(f"{space}: 无可见网格实例")
    # 变体映射（{map_id: "mdN"}，随 GLB extras 下发；多变体图 9 张，其余为 {}）
    try:
        sibling_entries = [e for e in load_registry(game_data) if e.space == space]
    except Exception:
        sibling_entries = [entry]
    variant_by_map_id = resolve_variant_map(sibling_entries, scene)
    variant_labels: dict = renderables.get("variant_labels") or {}

    # 00_global_content 装饰贴图索引（stem → 路径）：env_*/dec_* 装饰材质的
    # albedo 按约定与实体同名存放在全局内容目录（如 env_kr_cactus.pvr）
    global_tex_index: dict[str, str] = {}
    maps_root = game_data / "3d" / "Maps"
    gc_root = maps_root / "00_global_content"
    if gc_root.exists():
        for p in gc_root.rglob("*.dvpl"):
            rel = p.relative_to(maps_root)
            stem_path = (rel.parent / rel.name.split(".")[0]).as_posix() + ".tex"
            stem = rel.name.split(".")[0].lower()
            global_tex_index.setdefault(stem, stem_path)

    output_dir.mkdir(parents=True, exist_ok=True)
    # `.tex` 相对路径解析根：地图目录（landscape/... 等）与 3d/Maps 根
    # （`../00_global_content/...` 去掉 ../ 后即相对此根）
    textures = TextureStore(directory, [directory.parent])
    families = ClientMaterialFamily(game_data)
    glb = GlbBuilder()

    # ---- 几何按 datasource 去重（客户端同型物体共享 PolygonGroup）、材质按
    # 解析后的 albedo 去重（实例级 NMaterial 只是挂同一贴图树的空壳）----
    mesh_by_ds: dict[int, int] = {}
    alpha_bake_cache: dict[tuple, Image.Image] = {}
    flat_tint_cache: dict[tuple, Image.Image] = {}
    material_index_by_albedo: dict[tuple, int] = {}
    stats = {"decode_fail": 0, "no_group": 0, "no_uv": 0, "uv1": 0, "no_texture": 0,
             "impostor": 0, "flatcard": 0, "tree_lod_batch": 0, "st_cards": 0,
             "shadow_helper_skipped": 0,
             "lod_batch_dropped": renderables.get("lod_batches_dropped", 0)}

    # SpeedTree 实体的批次按 rbN.lodIndex 分 LOD 组（客户端按距离切组渲染）：
    # 同实体只保留"总顶点量最大"的组——lod0 通常最高精度（fir 完整组），但
    # _low 远景变体实体的卡片组在 lod1（lod0 是平板），按几何量择优统一处理。
    # 否则多组同时导出叠加成重影平面。
    lod_group_verts: dict[tuple, dict[int, int]] = {}
    for _n, _t, ds, mid, epath, cls, lod, _sh in instances:
        if cls != "SpeedTreeObject":
            continue
        g = groups_by_id.get(ds)
        if g is None:
            continue
        per = lod_group_verts.setdefault(epath, {})
        per[lod] = per.get(lod, 0) + (g.get("vertexCount") or 0)
    best_lod = {epath: max(per, key=per.get) if per else None
                for epath, per in lod_group_verts.items()}
    drop_lod: set[tuple] = set()
    for _n, _t, ds, mid, epath, cls, lod, _sh in instances:
        if cls == "SpeedTreeObject" and best_lod.get(epath) is not None and lod != best_lod[epath]:
            drop_lod.add((epath, lod))

    for name, transform, datasource, material_id, _path, cls, lod, sh_l0 in instances:
        inst_variant = variant_labels.get(_path)
        # 客户端不上屏的几何：ShadowVolume 材质（模板阴影体，只渲染进阴影贴图）
        # 与 *_helper 编辑器辅助体（材质无任何贴图槽）——导出只会成为屏幕上
        # 不存在的无贴图异物（如 dec_hm_stones01_shad 的阴影壳、env_nt_heinkel_helper）
        fx = (materials.resolve(material_id).get("fxName") or "")
        if fx.endswith("ShadowVolume.material") or "_helper" in str(name).lower():
            stats["shadow_helper_skipped"] += 1
            continue
        # SpeedTree 远景 billboard 交叉板：客户端由 SpeedTree 运行时按距离切换 LOD，
        # 静态场景里叶片卡片几何（同实体其他 batch）已完整呈现树形；这些
        # *planes*/*_bb_* 贴图的 batch 若一并导出会变成贯穿树冠的十字大板
        if cls == "SpeedTreeObject":
            if (_path, lod) in drop_lod:
                stats["tree_lod_batch"] += 1
                continue
            desc = materials.resolve(material_id)
            alb = (desc.get("textures") or {}).get("albedo") or ""
            if IMPOSTOR_TEX_RE.search(alb.rsplit("/", 1)[-1]):
                stats["impostor"] += 1
                continue

        group = groups_by_id.get(datasource)
        if group is None:
            stats["no_group"] += 1
            continue
        # 材质 shader 族（决定是否 SpeedTree 叶卡/树材质：billboard 重建 + 不受光染色）
        fam = families.resolve((materials.resolve(material_id).get("fxName")))
        # 材质文件缺失（Materials.custom 等）→ 回退旧口径：按实体类判 ST + SH 染色
        st_family = fam["speedtree"] if fam is not None else (cls == "SpeedTreeObject")
        st_spherical = fam["spherical"] if fam is not None else True
        try:
            positions = decode_polygon_positions(group)
            raw_indices = decode_polygon_indices(group)
        except Exception:
            stats["decode_fail"] += 1
            continue
        indices = group_triangles(group, raw_indices)
        if not indices or len(indices) < 3:
            stats["decode_fail"] += 1
            continue
        # SpeedTree 的远景 LOD 平贴卡片（z 向无厚度 + _low* LOD 贴图，如
        # a_bush1_low1-4/bush02_low1-2）：运行时按距离替换，静态导出会与 3D
        # 几何叠成毛团。仅当"平贴 且 贴图为 _low LOD 变体"时跳过——真水平
        # 叶簇（fir_leaves_2 顶盖等）虽平贴但非 LOD，必须保留
        if cls == "SpeedTreeObject":
            zs = [p[2] for p in positions]
            if max(zs) - min(zs) < 0.4:
                desc = materials.resolve(material_id)
                alb = (desc.get("textures") or {}).get("albedo") or ""
                if LOD_CARD_TEX_RE.search(alb.rsplit("/", 1)[-1]):
                    stats["flatcard"] += 1
                    continue
        decoded = decode_group_uvs(group)
        if decoded is None:
            uvs = uvs1 = None
            stats["no_uv"] += 1
        else:
            uvs, uvs1, channel = decoded
            if channel == 4:
                stats["uv1"] += 1
        # SpeedTree 叶卡 billboard 数据（客户端 speedtree-materials-vp.sl 同构）：
        # SCG 的 POSITION 是"锚点 pivot + 角点偏移"的展开态——pivot.w=1 时客户端
        # 渲染完全锚定 pivot，角点偏移每帧旋转风摆相位后在视空间加回（叶卡恒
        # 面向相机）。导出锚点为 POSITION、角点偏移/烘焙遮挡为自定义属性，
        # 前端着色器逐帧重建；非卡片布局（树干等）返回 None 走静态路径。
        # billboard 变换只存在于 speedtree-materials 着色器里——材质非该族
        # （Textured.material 等）时客户端就是静态几何，不做卡片重建
        # 掩码驱动：凡掩码含 PIVOT4 的组都按 pivot.w 双峰切分（见 decode_speedtree_cards）。
        # rigid 为 None = 整组纯叶卡；非 None = 混合批，刚体余量就地替换工作集
        # （下游 uv1 烘焙/材质/网格全部一致使用子集）。
        card = (decode_speedtree_cards(group)
                if (st_family and cls == "SpeedTreeObject") else None)
        if card is not None:
            if card["rigid"] is None:
                stats["st_cards"] = stats.get("st_cards", 0) + 1
            else:
                stats["st_cards_gen2"] = stats.get("st_cards_gen2", 0) + 1
                positions = card["rigid"]["positions"]
                indices = card["rigid"]["indices"]
                uvs = card["rigid"]["uvs"]
                uvs1 = card["rigid"]["uvs1"]
                stats["st_gen2_rigid"] = stats.get("st_gen2_rigid", 0) + 1

        mat_desc = materials.resolve(material_id)
        albedo_path = mat_desc["textures"].get("albedo")
        # env_*/dec_* 装饰材质可能只有 lightmap 泛型槽——其 albedo 按约定与
        # 实体同名存放在 00_global_content（如 env_kr_cactus.pvr），按名回退
        if albedo_path is None and cls in ("Mesh", "SpeedTreeObject"):
            stem = name.split(":")[0].strip().lower()
            if stem.endswith(".sc2"):
                stem = stem[:-4]
            albedo_path = global_tex_index.get(stem)
            if albedo_path is not None:
                stats["env_albedo_fallback"] = stats.get("env_albedo_fallback", 0) + 1
        if albedo_path and uvs is not None:
            img, has_alpha = textures.get(albedo_path)
        else:
            img, has_alpha = None, False
            if albedo_path is None:
                stats["no_texture"] += 1
        # UV1 覆盖烘焙（客户端 materials-fp.sl）：
        # - decal 槽（河道蓝瓷砖等）：RGB ×= decal(UV1) × flatColor × 2——
        #   可见纹理在 decal 里，不烘焙就是白色条带
        # - alphamask 槽（瀑布/波纹/烟雾效果层）：A ×= mask(UV1)——镂空+软透明
        # - FLATCOLOR：染色（瀑布水 flatColor 1.4+）
        # - 动画层（TEXTURE0_ANIMATION_SHIFT）用半透明混合（客户端 Translucent）
        mask_path = mat_desc["textures"].get("alphamask")
        # Water 着色器（海/河水面）的 decal 槽语义不同（water-fp.sl 深度染色），
        # 不做 MATERIAL_DECAL 烘焙；水面按客户端语义半透明渲染
        is_water = "water" in (mat_desc.get("fxName") or "").lower()
        decal_path = None if is_water else mat_desc["textures"].get("decal")
        flat_rgb = (1.0, 1.0, 1.0)
        if "flatColor" in (mat_desc.get("properties") or {}):
            flat_rgb = tuple(round(v, 4) for v in
                             _prop_floats(mat_desc, "flatColor", (1, 1, 1, 1))[:3])
        anim_layer = bool((mat_desc.get("flags") or {}).get("TEXTURE0_ANIMATION_SHIFT"))
        needs_bake = (decal_path or mask_path) and uvs1 is not None and img is not None
        flat_baked = False   # UV1 烘焙已含 flatColor 时不重复染色
        if needs_bake and (flat_rgb != (1.0, 1.0, 1.0) or anim_layer or decal_path):
            bake_key = (albedo_path, decal_path, mask_path, flat_rgb)
            if bake_key not in alpha_bake_cache:
                base_img, _ = textures.get(albedo_path)
                decal_img = textures.get(decal_path)[0] if decal_path else None
                mask_img = textures.get(mask_path)[0] if mask_path else None
                if base_img is not None:
                    mult = 2.0 if decal_path else 1.0   # ×2 仅 MATERIAL_DECAL 分支
                    alpha_bake_cache[bake_key] = bake_uv1_overlays(
                        base_img, decal_img, mask_img, flat_rgb, mult,
                        uvs, uvs1, indices)
                    stats["uv1_baked"] = stats.get("uv1_baked", 0) + 1
            baked = alpha_bake_cache.get(bake_key)
            if baked is not None:
                img = baked
                flat_baked = True
                if mask_path:
                    has_alpha = True
        # FLATCOLOR 整图染色（客户端 speedtree-fp / materials-fp 皆为采样后
        # baseColor *= flatColor）。此前只随 UV1 覆盖烘焙顺带应用，独立 FLATCOLOR
        # 材质（skit 芦苇叶 1.345/1.145/1.030、port 树皮 1.354/1.070/0.502、
        # holland 灌木 2.491/2.073/1.683 等）被静默丢弃。逐通道伽马空间相乘，
        # 与客户端同式；按 (albedo, 染色) 缓存，避免共享贴图被就地改写。
        if (img is not None and not flat_baked and flat_rgb != (1.0, 1.0, 1.0)
                and (mat_desc.get("flags") or {}).get("FLATCOLOR")):
            tint_key = (albedo_path, flat_rgb)
            if tint_key not in flat_tint_cache:
                flat_tint_cache[tint_key] = apply_flat_tint(img, flat_rgb)
            img = flat_tint_cache[tint_key]
            stats["flat_tinted"] = stats.get("flat_tinted", 0) + 1
        # SpeedTree 材质族（客户端 speedtree-materials.sl）：无场景光照，
        # color = albedo × varVertexColor，alpha discard 0.5——标记 ST| 让前端走
        # 不受光材质（我们此前的 Lambert+太阳让随机卡片亮度随朝向乱变，与客户端
        # 的均匀叶色完全不符）。
        # **族判定按材质、不按实体类（2026-10-07 勘误）**：.sc2 的 SpeedTreeObject
        # 类只决定顶点流形态，上屏着色器由材质文件 `Shader:` 行决定——芦苇/蕨
        # （skit Cattail/BostonFern）虽是 SpeedTreeObject 类但材质是 Textured.material，
        # 客户端按普通【受光】几何渲染（materials shader），旧口径把它们当 ST|
        # 不受光 + SH 加亮 = 芦苇/蕨异常偏亮发平的根因。
        # 染色按材质族分派（同一份客户端源码的两个分支，见 ClientMaterialFamily）：
        # - SPHERICAL_LIT/PBR_SPEEDTREE：SH(L0) 字面 RGB（2026-10-07 定版，erlenberg
        #   实测）；客户端另乘 0.282094(≈A0) 与 aoMult，属已知偏差，本前端未复刻。
        # - legacy（SpeedTree.material）：**SH 完全不参与**——客户端公式为
        #   color0 × treeLeafColorMul × treeLeafOcclusionMul + treeLeafOcclusionOffset；
        #   skit/karelia/canal 等图全体树木走这一支，skit 该组属性 = (1,1,1)/1.0
        #   ⇒ 叶色 = albedo × COLOR0，旧口径乘 SH(1.7725) 使叶片偏亮 77%（实测报障）。
        is_st = bool(st_family)
        if is_st:
            if st_spherical and sh_l0 is not None:
                if isinstance(sh_l0, tuple):
                    st_tint = tuple(min(c, 2.0) for c in sh_l0)
                else:
                    st_tint = (min(sh_l0, 2.0),) * 3
            elif st_spherical:
                st_tint = (1.0, 1.0, 1.0)   # 无 SHCoeff：等价只乘 color0（客户端同式）
            else:
                # 属性缺省 = shader 默认（treeLeafColorMul 0.5 / OcclusionMul 0.5 / Offset 0）
                tcm = _prop_floats(mat_desc, "treeLeafColorMul", (0.5, 0.5, 0.5, 0.5))
                tom = _prop_floats(mat_desc, "treeLeafOcclusionMul", (0.5,))
                too = _prop_floats(mat_desc, "treeLeafOcclusionOffset", (0.0,))
                st_tint = tuple(min(max(tcm[i] * tom[0] + too[0], 1e-3), 4.0) for i in range(3))
        else:
            st_tint = None
        card_occ = (card["occ_mean"] if card is not None
                    else (gen2["occ_mean"] if gen2 is not None else None))
        mat_key = (albedo_path, decal_path, mask_path, flat_rgb, has_alpha,
                   bool(img is not None), anim_layer and mask_path is not None,
                   is_water, st_tint, card_occ)
        if mat_key not in material_index_by_albedo:
            material_index_by_albedo[mat_key] = _build_material(
                glb, textures, albedo_path, mat_desc, img, has_alpha,
                blend=(anim_layer and mask_path is not None) or is_water,
                opacity=0.7 if is_water else None,
                st_tint=st_tint, occ_mean=1.0 if card_occ is not None else None)
        material_index = material_index_by_albedo[mat_key]

        # 网格按 (datasource, 材质) 去重：同型物体共享几何，但不同材质的
        # 共享几何（如 seabottom 海床/水面同 16 顶点四边形）必须各自建网格，
        # 否则先到的材质会套给全部实例（水面被渲染成海床）
        mesh_key = (datasource, material_index)
        if gen2 is not None:
            mesh_key = (datasource, material_index, "gen2")
        if mesh_key in mesh_by_ds:
            existing = mesh_by_ds[mesh_key]
            for mi in (existing if isinstance(existing, tuple) else (existing,)):
                glb.add_node(mi, transform, name, inst_variant)
            continue

        created = []
        if card is not None:
            # 叶卡：POSITION=锚点（树冠内固定点），_CORNER=角点偏移(+pivot.w)，
            # COLOR_0=烘焙遮挡（灰度）——前端视空间重建叶卡朝向
            mesh_index = glb.add_shared_mesh(
                datasource, card["anchors"], card["indices"], card["uvs"], None,
                material_index, extra_attrs=[
                    {"name": "_CORNER", "type": "VEC4", "data": card["corners"]},
                    {"name": "COLOR_0", "type": "VEC4", "data": card["colors"]},
                ])
            if mesh_index is not None:
                created.append(mesh_index)
        elif gen2 is not None:
            # 新一代混合批：刚体余量（静态网格）+ 锚定叶簇（卡片网格）各建一个，
            # 同一实例挂两节点（共享几何分别按 datasource+角色 去重）
            rig = gen2["rigid"]
            if rig:
                mi = glb.add_shared_mesh(
                    (datasource, "gen2rig"), rig["positions"], rig["indices"],
                    rig["uvs"], compute_normals(rig["positions"], rig["indices"]),
                    material_index,
                    extra_attrs=([{"name": "COLOR_0", "type": "VEC4", "data": rig["colors"]}]
                                 if rig.get("colors") else None))
                if mi is not None:
                    created.append(mi)
            mi = glb.add_shared_mesh(
                (datasource, "gen2card"), gen2["anchors"], gen2["indices"],
                gen2["uvs"], None, material_index, extra_attrs=[
                    {"name": "_CORNER", "type": "VEC4", "data": gen2["corners"]},
                    {"name": "COLOR_0", "type": "VEC4", "data": gen2["colors"]},
                ])
            if mi is not None:
                created.append(mi)
        else:
            mesh_index = glb.add_shared_mesh(datasource, positions, indices, uvs,
                                             compute_normals(positions, indices), material_index)
            if mesh_index is not None:
                created.append(mesh_index)
        if not created:
            continue
        mesh_by_ds[mesh_key] = tuple(created)
        for mi in created:
            glb.add_node(mi, transform, name, inst_variant)

    # ---- GLB + sidecar + 地面贴图 ----
    (output_dir / f"{space}.glb").write_bytes(
        glb.finish("wotb-agent export_map_glb (client-aligned)",
                   {"variantByMapId": variant_by_map_id} if variant_by_map_id else None))

    if scenery_only:
        # 只重导 GLB（变体打标等场景修正）；不触地面贴图与 sidecar——旧 sidecar
        # 的 ground/worldBounds 字段保持原样，避免部分写出造成消费端回退
        return {"meta": {"key": entry.key, "space": space,
                         "instances": len(glb.nodes),
                         "variantByMapId": variant_by_map_id},
                "bytes": (output_dir / f"{space}.glb").stat().st_size}

    landscape = renderables["landscape"] or {}
    land_mat = materials.resolve(landscape.get("matname"))
    meta = {
        "mapId": entry.map_id,
        "key": entry.key,
        "display": entry.display,
        "space": space,
        "sc2": entry.local_name,
        "worldBounds": landscape.get("worldBounds"),
        # 游戏内实际战场边界（MapBorderComponent.mbc.rect）；缺失为 None，消费端回退 worldBounds
        "playableBounds": _border_doc(map_border_bounds(scene)),
        "heightmap": landscape.get("heightmap"),
        "instances": len(glb.nodes),
        "meshes": len(glb.meshes),
        "materials": len(glb.materials),
        "triangles": glb.total_tris,
        "textures": len(glb.images),
        "warnings": {k: v for k, v in stats.items() if v},
    }
    ground = None
    color_tex = land_mat.get("textures", {}).get("colorTexture")
    if color_tex:
        tile_tex = land_mat.get("textures", {}).get("tileTexture0")
        tiling = _prop_floats(land_mat, "textureTiling", default=(50.0, 50.0))
        ground = export_ground(game_data, space, land_mat, directory, output_dir)
        meta["ground"] = {"source": color_tex, "tile": tile_tex,
                          "tiling": tiling[0], "size": ground["size"]}
    (output_dir / f"{space}.json").write_text(json.dumps(meta, indent=1), encoding="utf-8")
    return {"meta": meta, "bytes": (output_dir / f"{space}.glb").stat().st_size}


def _prop_floats(mat_desc: dict, key: str, default) -> tuple:
    """NMaterial properties 里 float/color 属性的字节解析：
    [type u8][count u8][3B 对齐] 后接 f32 序列，最多取 4 个（不足处用 default 补齐）。"""
    v = mat_desc.get("properties", {}).get(key)
    if not (isinstance(v, dict) and isinstance(v.get("$bytes"), str)):
        return tuple(default)
    b = bytes.fromhex(v["$bytes"])
    n = min(4, (len(b) - 5) // 4) if len(b) >= 9 else 0
    if n <= 0:
        return tuple(default)
    try:
        vals = struct.unpack_from(f"<{n}f", b, 5)
    except struct.error:
        return tuple(default)
    return vals + tuple(default)[n:]





def bake_uv1_overlays(base_img: Image.Image, decal_img, mask_img,
                       tint, mult, uvs, uvs1, idx: list) -> Image.Image:
    """UV1 覆盖烘焙（materials-fp.sl 客户端公式）：

    - decal 槽（MATERIAL_DECAL）：RGB ×= decal.rgb(UV1) × flatColor × 2.0
      （DRAW PHASE 的 color *= shadowColor × 2，阴影乘数受光态为 1）
    - alphamask 槽（ALPHA_MASK）：A ×= mask.a(UV1)（效果层镂空/软透明）
    - FLATCOLOR（非 decal 材质）：RGB ×= flatColor（无 ×2）
    按网格 UV0 三角形光栅化，UV1 重心插值采样（decal 平铺 wrap）。
    """
    canvas = base_img.convert("RGBA").copy()
    arr = np.asarray(canvas, np.float32)
    rgb = arr[..., :3].copy() / 255.0
    alpha = arr[..., 3].copy()
    orig_alpha = alpha.copy() / 255.0
    H, W = alpha.shape
    out_a = np.zeros((H, W), np.float32)
    covered = np.zeros((H, W), bool)
    decal = None
    if decal_img is not None:
        decal = np.asarray(decal_img.convert("RGB"), np.float32) / 255.0
    mask = None
    if mask_img is not None:
        mask = np.asarray(mask_img.convert("RGBA"), np.float32)[..., 3] / 255.0
    tint_arr = np.asarray(tint[:3], np.float32)

    for t in range(0, len(idx) - 2, 3):
        i0, i1, i2 = idx[t], idx[t + 1], idx[t + 2]
        xs = [uvs[i][0] * W for i in (i0, i1, i2)]
        ys = [uvs[i][1] * H for i in (i0, i1, i2)]
        xmin = max(0, int(min(xs))); xmax = min(W - 1, int(math.ceil(max(xs))))
        ymin = max(0, int(min(ys))); ymax = min(H - 1, int(math.ceil(max(ys))))
        if xmin > xmax or ymin > ymax:
            continue
        d = (xs[1] - xs[0]) * (ys[2] - ys[0]) - (xs[2] - xs[0]) * (ys[1] - ys[0])
        if abs(d) < 1e-12:
            continue
        gx, gy = np.meshgrid(np.arange(xmin, xmax + 1) + 0.5,
                             np.arange(ymin, ymax + 1) + 0.5)
        w0 = ((xs[1] - gx) * (ys[2] - gy) - (xs[2] - gx) * (ys[1] - gy)) / d
        w1 = ((xs[2] - gx) * (ys[0] - gy) - (xs[0] - gx) * (ys[2] - gy)) / d
        w2 = 1 - w0 - w1
        m = (w0 >= 0) & (w1 >= 0) & (w2 >= 0)
        if not m.any():
            continue
        u1 = w0 * uvs1[i0][0] + w1 * uvs1[i1][0] + w2 * uvs1[i2][0]
        v1 = w0 * uvs1[i0][1] + w1 * uvs1[i1][1] + w2 * uvs1[i2][1]
        if decal is not None:
            DH, DW = decal.shape[:2]
            di = (v1 * DH).astype(np.int64) % DH
            dj = (u1 * DW).astype(np.int64) % DW
            factor = decal[di, dj] * tint_arr * mult
            region = rgb[ymin:ymax + 1, xmin:xmax + 1]
            region[m] = np.clip(region[m] * factor[m], 0, 1)
        if mask is not None:
            MH, MW = mask.shape
            mi = (v1 * MH).astype(np.int64) % MH
            mj = (u1 * MW).astype(np.int64) % MW
            vals = mask[mi, mj]
            region_a = out_a[ymin:ymax + 1, xmin:xmax + 1]
            reg_c = covered[ymin:ymax + 1, xmin:xmax + 1]
            region_a[m] = np.maximum(region_a[m], vals[m])
            reg_c |= m

    if decal is None and not np.allclose(tint_arr, 1.0):
        # 非 decal 的 FLATCOLOR：整图染色（无逐像素变化）
        rgb *= tint_arr
    if mask is not None:
        out_a[covered] *= orig_alpha[covered]
        alpha = (np.clip(out_a, 0, 1) * 255)
    arr[..., :3] = np.clip(rgb, 0, 1) * 255.0
    arr[..., 3] = alpha
    return Image.fromarray(arr.astype(np.uint8), "RGBA")


def apply_flat_tint(img: Image.Image, tint) -> Image.Image:
    """FLATCOLOR 整图染色：RGB 逐通道乘 tint（伽马空间，与客户端 fragment 同式）。

    输入输出均 RGBA；返回新图（调用方按 (albedo, tint) 缓存，切勿就地改缓存贴图）。
    """
    arr = np.asarray(img.convert("RGBA"), np.float32).copy()
    arr[..., :3] = np.clip(arr[..., :3] / 255.0 * np.asarray(tint[:3], np.float32), 0.0, 1.0) * 255.0
    return Image.fromarray(arr.astype(np.uint8), "RGBA")


def _build_material(glb: GlbBuilder, textures: TextureStore, albedo_path: str | None,
                    mat_desc: dict, img: Image.Image | None, has_alpha: bool,
                    blend: bool = False, opacity: float | None = None,
                    st_tint=None, occ_mean: float | None = None) -> int:
    """albedo 贴图（含透明）→ GLB 材质；无贴图时用贴图均值色兜底。

    blend=True（TEXTURE0_ANIMATION_SHIFT 效果层：瀑布/波纹/烟雾）：客户端在
    Translucent 层做 alpha 混合，用 BLEND 模式近似；其余透明材质（树叶卡片）
    为 alpha test，用 MASK。
    st_tint：SpeedTree 材质族染色（RGB 三元组；SPHERICAL_LIT 族 = SH(L0) 字面值，
    legacy 族 = treeLeafColorMul×treeLeafOcclusionMul+Offset——**SH 不参与**）。
    """
    base_name = (mat_desc.get("materialName") or "mat")[:60]
    base = {"name": ("ST|" + base_name) if st_tint is not None else base_name,
            "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 1.0},
            "doubleSided": True}
    if st_tint is not None:
        base["pbrMetallicRoughness"]["baseColorFactor"] = [st_tint[0], st_tint[1], st_tint[2], 1.0]
    if occ_mean is not None:
        base["extras"] = {"occMean": occ_mean}   # 前端遮挡除数；1.0 = vOcc 直乘（客户端同式）
    if img is not None:
        # 源贴图统一翻回"文件原始行序"再嵌入——decode_dds / decode_pvr3 各自
        # 都在解码里翻过一次（消掉容器行序），此处再翻即为文件行序，对齐
        # glTF v=0 ↔ 图像首行与客户端 D3D v=0 = authored top。
        # **此前只对 DDS 源翻**：PVR 源（SpeedTree 叶卡图集/芦苇等）净效果上下
        # 颠倒，叶卡 UV 窗口全部落在图集空白区 → 树叶只剩零星小点（2026-10-07
        # 实测报障：skit skt_fir_leafs 叶卡主窗口覆盖率 5.4% → 翻正后 35.2%）。
        embed = img.transpose(Image.FLIP_TOP_BOTTOM)
        buf = io.BytesIO()
        if has_alpha:
            embed.save(buf, "PNG")
            tex_idx = glb.add_texture(("png", albedo_path), buf.getvalue(), "image/png")
        else:
            embed.convert("RGB").save(buf, "JPEG", quality=85)
            tex_idx = glb.add_texture(("jpg", albedo_path), buf.getvalue(), "image/jpeg")
        base["pbrMetallicRoughness"]["baseColorTexture"] = {"index": tex_idx}
        if opacity is not None:
            base["alphaMode"] = "BLEND"
            base["pbrMetallicRoughness"]["baseColorFactor"] = [1.0, 1.0, 1.0, opacity]
        elif has_alpha and blend:
            base["alphaMode"] = "BLEND"
        elif has_alpha:
            # alpha cutout：SpeedTree 材质（ST|）按客户端 discard 0.5，其余 0.33
            base["alphaMode"] = "MASK"
            base["alphaCutoff"] = 0.5 if st_tint is not None else 0.33
    else:
        r, g, b = textures.avg_color.get(albedo_path or "", (0.62, 0.60, 0.55)) or (0.62, 0.60, 0.55)
        base["pbrMetallicRoughness"]["baseColorFactor"] = [r, g, b, 1.0]
    glb.materials.append(base)
    return len(glb.materials) - 1


def _decode_landscape_tex(textures: TextureStore, tex_path: str) -> Image.Image | None:
    """按贴图槽路径解码（colormap/tilemask/tile 等，RGBA）。"""
    img, _ = textures.get(tex_path)
    return img


def decode_tiletx_planes_from_slot(tile_tex: str, map_dir: pathlib.Path):
    """TileTex 槽 -> [4 张灰度细节 ndarray]；文件缺失/结构不符返回 None。"""
    stem = tile_tex[:-4] if tile_tex.lower().endswith(".tex") else tile_tex
    rel = stem[3:] if stem.startswith("../") else stem.lstrip("/")
    base = map_dir / pathlib.Path(rel).parent / pathlib.Path(rel).name
    for suf in (".dx11.pvr.dvpl", ".pvr.dvpl", ".dx11.dds.dvpl", ".dds.dvpl"):
        cand = base.with_name(base.name + suf)
        if cand.exists():
            d = decode_dvpl(cand.read_bytes())
            return decode_tiletx_planes(d)
    return None


def _srgb_to_linear(arr):
    """sRGB -> 线性（客户端 shader 的硬件解码行为）。"""
    c = np.clip(np.asarray(arr, dtype=np.float32), 0.0, 1.0)
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def _linear_to_srgb(arr):
    """线性 -> sRGB（客户端输出编码）。"""
    c = np.clip(np.asarray(arr, dtype=np.float32), 0.0, None)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)


def _resize_channels(img: Image.Image, size: tuple[int, int]) -> np.ndarray:
    """RGBA 图逐通道重采样为 float32 HxWx4（0..1）。

    PIL 对 RGBA 图整体 resize 会把透明区 RGB 与可见区混算（实测 mask R 通道
    均值 0.585→0.168），必须逐通道缩放——与客户端 GPU 各通道独立采样一致。
    """
    return np.stack([np.asarray(img.getchannel(i).resize(size, Image.BILINEAR), dtype=np.float32)
                     for i in range(4)], axis=-1) / 255.0


def bake_ground(colormap: Image.Image, planes, mask_img, hmap_planes,
                cfg: dict) -> Image.Image:
    """按客户端 Landscape/tilemask-fp.sl 非 PBR 路径逐像素烘焙地表。

    着色器（Data/Materials/Shaders/Landscape/tilemask-fp.sl，按材质 flags 分支）：
        colorMapAlbedo *= globalFlatColor * 2            # 仅 FLATCOLOR/GLOBAL_TINT
        alpha 调整 (brightness,contrast,gamma)            # 仅 GLOBAL_TINT 内 SEPARATE_LIGHTMAP_CHANNEL
        shadowColor = colorMapAlbedo * colorMapFetch.a    # SEPARATE_LIGHTMAP_CHANNEL；
                                                          # SHADOW_RECEIVER 全光照态乘数为 1
        非 HeightBlend: out = (Σ_k tile.ch_k * mask.ch_k * tileColor_k) * shadowColor * 2
        HeightBlend:    mask2 = sat(tilemaskWeight*(mask*2-1) + hMap*scaleColor + offsetColor)
                        out = HeightBlend(tile.ch_k*tileColor_k, mask2) * shadowColor * 2
                        （HeightBlend: b = max(mask2 - (max(mask2)-softness), 0.001) 归一加权）
        SCALED_TILES:   每通道按各自 tileScale_k 平铺采样
    权重不做归一/整形、阴影不设下限——均为客户端原样（烘焙不做任何提亮）。
    输出做垂直翻转：贴图 t=0 对应世界 +y（北），WebP 契约上边=+z（北）。
    """
    # 4096²：清晰度主要来自 tile 细节纹理的平铺频率（colormap 本身低频，
    # 双线性放大不损失信息）——输出线性分辨率翻倍，tile 采样密度同步翻倍
    h = w = 4096
    cm = _resize_channels(colormap.convert("RGBA"), (w, h))
    albedo = cm[..., :3].copy()
    alpha = cm[..., 3:4].copy()
    if cfg["flatcolor"]:
        albedo *= np.asarray(cfg["flat_color"][:3], dtype=np.float32) * 2.0
        if cfg["separate_lm"]:
            br, ct, gm = cfg["lm_adjust"][:3]
            alpha = np.power(alpha, gm)
            alpha = (alpha - 0.5) * ct + 0.5
            alpha = alpha + br
    shadow = albedo * alpha if cfg["separate_lm"] else albedo

    def channel_fetch_k(planes4, scales, k):
        """tileTexture0/tileHeightTexture 第 k 通道的平铺采样（客户端单次 RGBA
        fetch 取 .r/.g/.b/.a；4 平面容器按"平面 k = 通道 k"展开；SCALED_TILES
        时每通道各自 tileScale_k）。按通道惰性取用控制 4096² 内存峰值。"""
        ph, pw = planes4[k].shape[:2]
        ys = ((np.arange(h) * cfg["tiling"][1] * scales[k]) % ph).astype(np.int64)
        xs = ((np.arange(w) * cfg["tiling"][0] * scales[k]) % pw).astype(np.int64)
        return planes4[k].astype(np.float32)[np.ix_(ys, xs)] / 255.0

    if planes is None or len(planes) != 4:
        planes = [np.full((512, 512), 115, np.uint8)] * 4

    if mask_img is not None:
        mask = _resize_channels(mask_img.convert("RGBA"), (w, h))
    else:
        mask = np.full((h, w, 4), 0.25, dtype=np.float32)

    if cfg["height_blend"] and hmap_planes is not None:
        hch = [channel_fetch_k(hmap_planes, cfg["tile_scale"], k) for k in range(4)]
        hmap = np.stack(hch, axis=-1)
        mask2 = cfg["tilemask_weight"] * (mask * 2.0 - 1.0) \
            + hmap * np.asarray(cfg["hb_scale"][:4], dtype=np.float32) \
            + np.asarray(cfg["hb_offset"][:4], dtype=np.float32)
        np.clip(mask2, 0.0, 1.0, out=mask2)
        start = mask2.max(axis=-1, keepdims=True) - np.asarray(cfg["hb_softness"][:4], dtype=np.float32)
        b = np.maximum(mask2 - start, 0.001)
        denom = b.sum(axis=-1, keepdims=True)
        detail = np.zeros((h, w, 3), dtype=np.float32)
        for k in range(4):
            tint = np.asarray(cfg["tile_colors"][k][:3], dtype=np.float32)
            detail += channel_fetch_k(planes, cfg["tile_scale"], k)[..., None] * tint * b[..., k:k + 1]
        detail /= denom
    else:
        detail = np.zeros((h, w, 3), dtype=np.float32)
        for k in range(4):
            tint = np.asarray(cfg["tile_colors"][k][:3], dtype=np.float32)
            detail += channel_fetch_k(planes, cfg["tile_scale"], k)[..., None] * tint * mask[..., k:k + 1]

    out = detail * shadow * 2.0
    img = Image.fromarray((np.clip(out, 0, 1) * 255).astype(np.uint8), "RGB")
    return img.transpose(Image.FLIP_TOP_BOTTOM)


def landscape_bake_cfg(land_mat: dict) -> dict:
    """从解析后的 Landscape NMaterial 提取 tilemask-fp.sl 所需 flags/属性。"""
    flags = land_mat.get("flags") or {}

    def fprop(key, default):
        return list(_prop_floats(land_mat, key, default))

    return {
        # FLATCOLOR/GLOBAL_TINT（32_faust_night 等）；着色器内 globalFlatColor 缺省 0.5 → ×2 后中性
        "flatcolor": any(flags.get(k) for k in ("FLATCOLOR", "GLOBAL_TINT")),
        "flat_color": fprop("globalFlatColor", (0.5, 0.5, 0.5)),
        "separate_lm": bool(flags.get("LANDSCAPE_SEPARATE_LIGHTMAP_CHANNEL")),
        "lm_adjust": fprop("landscapeLightmapAdjustment", (0.0, 1.0, 1.0)),
        # SCALED_TILES：仅此旗标下客户端才逐通道 tileScale 采样，否则单次 RGBA 取
        "scaled_tiles": bool(flags.get("LANDSCAPE_SCALED_TILES_NON_PBR")),
        "height_blend": bool(flags.get("LANDSCAPE_HEIGHT_BLEND")),
        "tilemask_weight": fprop("tilemaskWeight", (0.15,))[0],
        "hb_scale": fprop("heightMapScaleColor", (1.0, 1.0, 1.0, 1.0)),
        "hb_offset": fprop("heightMapOffsetColor", (0.0, 0.0, 0.0, 0.0)),
        "hb_softness": fprop("heightMapSoftnessColor", (0.15, 0.15, 0.15, 0.15)),
        "tile_scale": [fprop(f"tileScale{i}", (1.0,))[0] for i in range(4)],
        "tile_colors": [fprop(f"tileColor{i}", (1.0, 1.0, 1.0)) for i in range(4)],
        "tiling": fprop("textureTiling", (50.0, 50.0)),
    }


def export_ground(game_data: pathlib.Path, space: str, land_mat: dict,
                  map_dir: pathlib.Path, output_dir: pathlib.Path) -> dict:
    """客户端 Landscape 地表烘焙 → WebP（上=+z/北，与底图契约一致）。"""
    if Image is None or imagecodecs is None:
        raise RuntimeError("需要 pip install pillow imagecodecs")

    def resolve_file(tex_path: str) -> pathlib.Path | None:
        stem = tex_path[:-4] if tex_path.lower().endswith(".tex") else tex_path
        rel = stem[3:] if stem.startswith("../") else stem.lstrip("/")
        base = map_dir / pathlib.Path(rel).parent / pathlib.Path(rel).name
        for suf in (".dx11.dds.dvpl", ".dds.dvpl", ".dx11.pvr.dvpl", ".pvr.dvpl"):
            cand = base.with_name(base.name + suf)
            if cand.exists():
                return cand
        return None

    def decode_opt(tex_path):
        if not tex_path:
            return None
        tf = resolve_file(tex_path)
        if tf is None:
            return None
        td = decode_dvpl(tf.read_bytes())
        return decode_dds(td, max_dim=2048) if td[:4] == b"DDS " else decode_pvr3(td, max_dim=2048)

    tex = land_mat.get("textures", {})
    color_tex = tex.get("colorTexture")
    src = resolve_file(color_tex) if color_tex else None
    if src is None:
        raise FileNotFoundError(f"{space}: colormap 文件缺失（{color_tex}）")
    cfg = landscape_bake_cfg(land_mat)
    cmap = decode_opt(color_tex)

    def tile_channels(tex_path):
        """TileTx/Height 纹理 → 4 通道细节平面。

        客户端把 4 张细节图打包为单张 RGBA 纹理的四个通道（着色器单次 RGBA
        采样取 .r/.g/.b/.a）；少数变体是 DAVA 四平面容器（每平面单通道 4444
        mip 链，普通解码因载荷长度不符而失败），退回专用四平面解码。
        """
        if not tex_path:
            return None
        img = decode_opt(tex_path)
        if img is not None:
            arr = np.asarray(img.convert("RGBA"), dtype=np.uint8)
            return [arr[..., k].copy() for k in range(4)]
        return decode_tiletx_planes_from_slot(tex_path, map_dir)

    planes = tile_channels(tex.get("tileTexture0"))

    # HeightBlend 分支用 tileMaskHeightBlend + tileHeightTexture（着色器同名采样器）
    mask_tex = tex.get("tileMaskHeightBlend") if cfg["height_blend"] else None
    mask_tex = mask_tex or tex.get("tileMask")
    mask_img = decode_opt(mask_tex) if mask_tex else None
    hmap_planes = tile_channels(tex.get("tileHeightTexture")) if cfg["height_blend"] else None

    baked = bake_ground(cmap, planes, mask_img, hmap_planes, cfg)
    out = output_dir / f"{space}.ground.webp"
    baked.save(out, "WEBP", quality=90)

    # ---- 客户端同款分层导出：前端 ShaderMaterial 按 tilemask-fp.sl 实时合成，
    # tile/height 纹理以原生分辨率平铺重复（客户端 texCoordTiled = texCoord ×
    # textureTiling，约 30–120 次重复/全图），清晰度不再受烘焙网格分辨率限制。
    # 行序保持 colormap 原始空间（不翻转）；前端采样 uv = (0.5−X/s, 0.5−Z/s)。
    # tile/height/mask 是四个独立语义通道，必须无损（有损 RGB 会通道串色）。
    layers: dict = {
        "tiling": [round(v, 4) for v in cfg["tiling"]],
        "tile_scale": [round(v, 4) for v in cfg["tile_scale"]],
        "tile_colors": [[round(c, 4) for c in tc[:3]] for tc in cfg["tile_colors"]],
        "flatcolor": bool(cfg["flatcolor"]),
        "flat_color": [round(v, 4) for v in cfg["flat_color"][:3]],
        "separate_lm": bool(cfg["separate_lm"]),
        "lm_adjust": [round(v, 4) for v in cfg["lm_adjust"][:3]],
        "scaled_tiles": bool(cfg["scaled_tiles"]),
        "height_blend": bool(cfg["height_blend"]),
        "tilemask_weight": round(cfg["tilemask_weight"], 4),
        "hb_scale": [round(v, 4) for v in cfg["hb_scale"][:4]],
        "hb_offset": [round(v, 4) for v in cfg["hb_offset"][:4]],
        "hb_softness": [round(v, 4) for v in cfg["hb_softness"][:4]],
        "files": {},
    }

    def save_layer(name: str, img: Image.Image, lossless: bool = True) -> None:
        p = output_dir / f"{space}.ground.{name}.webp"
        if lossless:
            img.save(p, "WEBP", lossless=True)
        else:
            img.save(p, "WEBP", quality=92)
        layers["files"][name] = {"size": list(img.size), "bytes": p.stat().st_size}

    # 注意：所有载荷一律无 alpha（RGB/灰度 webp）——Chrome 把带 alpha 的 webp
    # 解码为预乘 RGB（three.js 原样上传后权重/细节被 alpha 压暗近黑，实测 mask
    # 的 4 通道被 0.043 的 alpha 压成近零 → 地面大片变黑）。第 4 通道拆独立灰度图。
    cm_rgba = cmap.convert("RGBA")
    save_layer("cm", cm_rgba.convert("RGB"), lossless=False)          # colorTexture RGB
    save_layer("lm", cm_rgba.getchannel("A"))                         # colorTexture.a = lightmap
    if planes is not None and len(planes) == 4:
        pl = np.stack(planes, axis=-1).astype(np.uint8)
        save_layer("tile0", Image.fromarray(pl[..., :3], "RGB"))      # tileTexture0 ch0-2
        save_layer("tile1", Image.fromarray(pl[..., 3]))              # tileTexture0 ch3
    if mask_img is not None:
        mk = np.asarray(mask_img.convert("RGBA"), dtype=np.uint8)
        save_layer("mask0", Image.fromarray(mk[..., :3], "RGB"))      # mask ch0-2
        save_layer("mask1", Image.fromarray(mk[..., 3]))              # mask ch3
    if cfg["height_blend"] and hmap_planes is not None and len(hmap_planes) == 4:
        hp = np.stack(hmap_planes, axis=-1).astype(np.uint8)
        save_layer("hmap0", Image.fromarray(hp[..., :3], "RGB"))      # tileHeight ch0-2
        save_layer("hmap1", Image.fromarray(hp[..., 3]))              # tileHeight ch3
    need = 8 if cfg["height_blend"] else 6
    if len(layers["files"]) >= need:
        lp = output_dir / f"{space}.ground.layers.json"
        lp.write_text(json.dumps(layers, ensure_ascii=False), encoding="utf-8")

    return {"size": baked.size, "bytes": out.stat().st_size, "source": src.name,
            "tile": tex.get("tileTexture0"), "mask": mask_tex}


def _export_map_worker(task: dict) -> dict:
    """并行导出的子进程入口：导出单张地图，返回结果摘要（异常转 failure）。"""
    game_data = pathlib.Path(task["game_data"])
    out_dir = pathlib.Path(task["output_dir"])
    e = task["entry"]
    entry = MapEntry(e["map_id"], e["key"], e["local_name"], e["minimap_dir"], e["display"])
    started = time.time()
    try:
        if task.get("scenery_only"):
            info = export_map(game_data, entry, out_dir, scenery_only=True)
            vb = info["meta"].get("variantByMapId") or {}
            return {"key": entry.key, "space": entry.space, "map_id": entry.map_id,
                    "ok": True, "sec": round(time.time() - started, 1),
                    "line": f"[ok] {entry.space:<20} GLB {info['bytes'] / 1e6:.2f}MB "
                            f"变体映射={vb or '无'}"}
        if task["ground_only"]:
            directory = game_data / "3d" / "Maps" / entry.space
            sc2_path = find_member(directory, entry.space, ".sc2.dvpl") or find_member(directory, entry.space, ".sc2")
            scene = read_sc2(load_payload(sc2_path))
            land_mat = MaterialLibrary(scene).resolve(collect_renderables(scene)["landscape"]["matname"])
            ground = export_ground(game_data, entry.space, land_mat, directory, out_dir)
            return {"key": entry.key, "space": entry.space, "map_id": entry.map_id,
                    "ok": True, "sec": round(time.time() - started, 1),
                    "line": f"[ok] {entry.space:<20} 地面 {ground['size'][0]}x{ground['size'][1]} "
                            f"{ground['bytes'] / 1e6:.2f}MB"}
        info = export_map(game_data, entry, out_dir)
        m = info["meta"]
        bound = m.get("worldBounds")
        zmax = f"{bound['max'][2]:.0f}m" if bound else "?"
        return {"key": entry.key, "space": entry.space, "map_id": entry.map_id,
                "ok": True, "sec": round(time.time() - started, 1),
                "line": f"[ok] id={entry.map_id:<3} {entry.key:<18} space={entry.space:<20} "
                        f"实例 {m['instances']:>5} 网格 {m['meshes']:>4} 三角形 {m['triangles']:>8} "
                        f"贴图 {m['textures']:>3} zmax={zmax} {info['bytes'] / 1e6:.1f}MB"
                        + (f" 警告{m['warnings']}" if m["warnings"] else "")}
    except Exception as exc:
        return {"key": entry.key, "space": entry.space, "map_id": entry.map_id,
                "ok": False, "sec": round(time.time() - started, 1),
                "line": f"[fail] id={entry.map_id:<3} {entry.key:<18} {entry.space}: {exc}"}


def _write_status(status_path: pathlib.Path, state: dict) -> None:
    """原子更新进度状态文件（外部可随时读取查询导出是否完成）。"""
    state["updated_at"] = time.strftime("%H:%M:%S")
    tmp = status_path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, ensure_ascii=False, indent=1), encoding="utf-8")
    tmp.replace(status_path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--map", action="append",
                        help="只导出指定图（回放 id / 显示名 / maps.yaml 键，可重复）；缺省全部")
    parser.add_argument("--game-data", type=pathlib.Path,
                        default=pathlib.Path("D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data"))
    parser.add_argument("--output-dir", type=pathlib.Path, default=pathlib.Path("data/cache/maps"))
    parser.add_argument("--ground-only", action="store_true",
                        help="跳过 GLB，只重新导出地面贴图（+元数据）")
    parser.add_argument("--scenery-only", action="store_true",
                        help="只重导 GLB（如变体打标修正）；不触地面贴图与 sidecar")
    parser.add_argument("--jobs", type=int, default=4,
                        help="并行导出进程数（缺省 4；每张图相互独立）")
    args = parser.parse_args()

    started = time.time()
    status_path = args.output_dir / "_export_status.json"
    state = {"total": 0, "done": 0, "failed": 0, "finished": False,
             "results": [], "failures": []}

    def put_status() -> None:
        state["elapsed_sec"] = round(time.time() - started, 1)
        _write_status(status_path, state)

    entries = load_registry(args.game_data)
    print(f"注册表：{len(entries)} 张地图（maps.yaml ∩ en.yaml）", flush=True)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    if args.map:
        wanted = []
        for name in args.map:
            e = resolve_entry(entries, name)
            if e is None:
                print(f"[skip] {name}: 注册表无此图", flush=True)
            else:
                wanted.append(e)
    else:
        wanted = entries

    # 按 space 去重（变体地图共享场景）
    seen_space: set[str] = set()
    targets = []
    for e in wanted:
        if e.space not in seen_space:
            seen_space.add(e.space)
            targets.append(e)

    state["total"] = len(targets)
    put_status()
    tasks = [{"game_data": str(args.game_data), "output_dir": str(args.output_dir),
              "entry": {"map_id": e.map_id, "key": e.key, "local_name": e.local_name,
                        "space": e.space, "minimap_dir": e.minimap_dir,
                        "display": e.display},
              "ground_only": bool(args.ground_only),
              "scenery_only": bool(args.scenery_only)}
             for e in targets]

    def on_result(res: dict) -> None:
        state["done"] += 1
        entry_rec = {"key": res["key"], "space": res["space"],
                     "ok": res["ok"], "sec": res["sec"]}
        if res["ok"]:
            state["results"].append(entry_rec)
        else:
            state["failed"] += 1
            state["failures"].append(res["space"])
            state["results"].append({**entry_rec, "error": res["line"]})
        put_status()
        elapsed = time.time() - started
        remain = elapsed / state["done"] * (state["total"] - state["done"])
        print(f"({state['done']}/{state['total']}，剩余约 {remain:.0f}s) {res['line']}", flush=True)

    jobs = max(1, min(int(args.jobs), len(targets)))
    if jobs > 1:
        print(f"并行导出 {len(targets)} 张图（jobs={jobs}），进度：{status_path}", flush=True)
        with concurrent.futures.ProcessPoolExecutor(max_workers=jobs) as pool:
            for res in pool.map(_export_map_worker, tasks):
                on_result(res)
    else:
        for task in tasks:
            on_result(_export_map_worker(task))

    state["finished"] = True
    put_status()
    if state["failed"]:
        print(f"\n失败 {state['failed']}：{', '.join(state['failures'])}", flush=True)
        return 1
    print(f"\n全部完成：{state['total']} 张，耗时 {time.time() - started:.0f}s", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
