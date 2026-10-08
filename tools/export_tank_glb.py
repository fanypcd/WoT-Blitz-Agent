#!/usr/bin/env python3
"""用本机 WoT Blitz 客户端自行导出坦克 `model.glb` / `collision.glb`（BlitzKit 的可替代来源）。

**不替换运行期数据源**：产物写到 `data/cache/local_models/<tank_id>/`，与 BlitzKit 缓存
`data/cache/models/<tank_id>/` 并存；本仓库运行期（Web/查看器）仍读 BlitzKit 那份。
两者并用 `tools/compare_tank_glb.py` 逐辆对照。

管线（与客户端加载方式一致，依据见各函数 docstring）：

  1. tank_id → 游戏模型名：读 `data/tanks.pb` 的 field1(tank_id) / field11(nation) /
     field32(游戏模型名)。field2 是 BlitzKit 的 slug（`m4a3e2`），**不能用于文件定位**——
     2026-09 前后 BlitzKit 把模型名从 field2 挪到了 field32。
  2. 权威路径：`3d/Tanks/Parameters/<nation>/<模型名>.yaml` 的 `resourcesPath.blitzModelPath`
     / `collisionMesh`；两者缺失时按目录约定回退（`Tanks/<Nation>/` 与 `CollisionMeshes/`）。
  3. 几何：`.sc2`（DAVA SceneFileV2 + KeyedArchive 实体树）+ 同名 `.scg`（SCPG PolygonGroup）。
     可视化顶点流 56B = 14 floats（pos/nrm/uv/tangent/binormal）；碰撞 32B = 8 floats，
     **末尾第 8 个 float 是装甲板号**。
  4. 材质：`rb.nmatname` → `.sc2` 的 NMaterial 节点，下钻 `configArchive_N`（皮肤变体，
     取 `configName=='Default'`）后沿 `parentMaterialKey` 继承链合并；材质名取继承链**根**。
  5. 贴图：`.tex` → `.dx11.dds.dvpl`，自写 BC1/2/3/5 解码（含 DAVA 把 BC5 装在 DXT5
     fourcc 里的情形，`pfFlags` bit31 置位）；编码 `image/webp`。

BlitzKit 契约要点（对照基准）：
  * 所有节点变换为 identity（姿态由前端运行时写矩阵）；
  * mesh 挂在名为批次号（`0000`/`0001`）的子节点上；mesh 名恒为 `RenderBatch`；
  * 排除 `HP_*` 特效锚点、`*_crash_*`、`chassis_chassis_*`、`*lod<d>ummy*` 烘焙占位；
  * 每个部件的 LOD0 批次里**丢弃 `Shadow_Material` 烘焙阴影**，再取剩余候选；
  * `collision.glb` 按**装甲板号连续 run** 切节点（命名 `<part>_armor_<N>`），与客户端三角序一致；
  * NaN 规范化为 `0x7fc00000`；索引按最大索引自适应 uint16/uint32。

贴图槽位口径（`--texture-mode`）：
  * `semantic`（缺省，推荐）：按 PBR 语义正确装配——
    baseColor←`baseColorMap`（老式车退 `albedo`）、normal←`baseNormalMap`（老式车退 `normalmap`，
    BC5 双通道重建 z）、metallicRoughness←`baseRMMap`（**搬通道**：ch0→G 粗糙度、ch1→B 金属度；
    glTF 采样 G/B，原样返回会把金属度当粗糙度）、occlusion←`miscMap.R`；
  * `none`：不导出贴图（只比几何时用，最快）。
  注：**不提供"复刻 BlitzKit 指派"的口径**——其指派在 PBR 语义上不成立，且逐通道实测与
  报告 B §3.3 的描述也**不符**（BK 的 normal 实为 legacy `normalmap`、MR.G 实为 `baseRMMap.ch0`、
  occlusion 与我们同为 `miscMap.R`），复刻只会误导；见 docs/local-model-export.md §4。`

用法：
    python tools/export_tank_glb.py --tank 9489 --tank 7169      # 指定若干 tank_id
    python tools/export_tank_glb.py --all                        # 全量 735 辆
    python tools/export_tank_glb.py --all --texture-mode none --jobs 8
    python tools/export_tank_glb.py --list                       # 列出 tank_id → 模型名

进度：原子更新 `<out>/_export_status.json`（total/done/failed/finished/results），
与 `tools/export_map_glb.py` 同一约定。
"""

from __future__ import annotations

import argparse
import concurrent.futures
import io
import json
import os
import pathlib
import re
import struct
import sys
import time

import numpy as np

TOOLS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent
sys.path.insert(0, str(TOOLS_DIR / "wotbtools"))

from dlc_packs import client_path  # noqa: E402
from wotb_sc2 import decode_dvpl, decode_bytes, read_sc2  # noqa: E402
from wotb_scg import read_scg  # noqa: E402

try:
    import imagecodecs
except ImportError:  # 贴图解码需要
    imagecodecs = None

try:
    from PIL import Image, ImageFilter
except ImportError:
    Image = None

# 客户端 Data 目录：与 src/wargaming/game_extract.rs 的 DEFAULT_GAME_DIRS 保持同一组
GAME_DIR_CANDIDATES = [
    "D:/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
    "C:/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
    "/mnt/c/Program Files (x86)/Steam/steamapps/common/World of Tanks Blitz/Data",
    "/mnt/d/SteamLibrary/steamapps/common/World of Tanks Blitz/Data",
]

# tanks.pb 的 nation 值 → 客户端 `3d/Tanks/<dir>` 目录名
NATION_DIR = {"germany": "German", "ussr": "USSR", "usa": "USA", "uk": "GB",
              "china": "China", "japan": "Japan", "france": "France",
              "european": "European", "other": "Other"}


def default_game_data() -> pathlib.Path:
    for c in GAME_DIR_CANDIDATES:
        p = pathlib.Path(c)
        if p.is_dir():
            return p
    return pathlib.Path(GAME_DIR_CANDIDATES[0])


# ---------------------------------------------------------------------------
# protobuf：tank_id → 游戏模型名（field32）
# ---------------------------------------------------------------------------
def _pb_varint(b: bytes, i: int) -> tuple[int, int]:
    r = 0
    s = 0
    while True:
        x = b[i]
        i += 1
        r |= (x & 0x7F) << s
        if not x & 0x80:
            return r, i
        s += 7


def _pb_fields(b: bytes) -> list[tuple[int, int, object]]:
    i = 0
    out: list[tuple[int, int, object]] = []
    while i < len(b):
        key, i = _pb_varint(b, i)
        fn, wt = key >> 3, key & 7
        if wt == 0:
            v, i = _pb_varint(b, i)
            out.append((fn, wt, v))
        elif wt == 2:
            ln, i = _pb_varint(b, i)
            out.append((fn, wt, b[i:i + ln]))
            i += ln
        elif wt == 5:
            out.append((fn, wt, struct.unpack_from("<f", b, i)[0]))
            i += 4
        elif wt == 1:
            out.append((fn, wt, b[i:i + 8]))
            i += 8
        else:
            raise ValueError(f"unsupported wire type {wt}")
    return out


def read_tank_table(pb_path: pathlib.Path) -> dict[int, dict]:
    """`data/tanks.pb` → {tank_id: {nation, stem, slug}}。stem 即游戏模型名（field32）。"""
    buf = pb_path.read_bytes()
    table: dict[int, dict] = {}
    for fn, wt, v in _pb_fields(buf):
        if fn != 1 or wt != 2:
            continue
        entry = _pb_fields(v)
        tid = next((x[2] for x in entry if x[0] == 1 and x[1] == 0), None)
        main = next((x[2] for x in entry if x[0] == 2 and x[1] == 2), None)
        if tid is None or main is None:
            continue
        nation = stem = slug = ""
        for f2, w2, v2 in _pb_fields(main):
            if f2 == 11 and w2 == 2:
                nation = v2.decode("utf-8", "replace")
            elif f2 == 32 and w2 == 2:
                stem = v2.decode("utf-8", "replace")
            elif f2 == 2 and w2 == 2:
                slug = v2.decode("utf-8", "replace")
        table[int(tid)] = {"nation": nation, "stem": stem or slug, "slug": slug}
    return table


# ---------------------------------------------------------------------------
# DAVA / DDS 解码
# ---------------------------------------------------------------------------
BC_BLOCK = {1: 8, 2: 16, 3: 16, 4: 8, 5: 16}
DXGI_TO_BCN = {70: 1, 71: 1, 72: 1, 73: 2, 74: 2, 75: 2, 76: 3, 77: 3, 78: 3,
               79: 4, 80: 4, 81: 4, 82: 5, 83: 5, 84: 5}
FOURCC_TO_BCN = {"DXT1": 1, "DXT3": 2, "DXT5": 3}


def decode_dds(d: bytes, max_dim: int = 0):
    """DDS → (RGB/RGBA ndarray, 格式标签)。覆盖 DX10 头 / 真 DXTn / DAVA 的 DXT5-包-BC5。"""
    if Image is None or imagecodecs is None or d[:4] != b"DDS ":
        return None, "no-decoder"
    h = struct.unpack_from("<I", d, 12)[0]
    w = struct.unpack_from("<I", d, 16)[0]
    pf_flags = struct.unpack_from("<I", d, 80)[0]
    fourcc = d[84:88].decode("latin1")
    off, tag = 128, fourcc
    if fourcc == "DX10":
        # DAVA 写头整体 +4：dxgi 码在 128（标准 124），像素自 148
        bcn = None
        for dxgi_off, doff in ((128, 148), (124, 144)):
            val = struct.unpack_from("<I", d, dxgi_off)[0]
            if val in DXGI_TO_BCN:
                bcn, off, tag = DXGI_TO_BCN[val], doff, f"DX10/dxgi{val}"
                break
    else:
        bcn = FOURCC_TO_BCN.get(fourcc)
        if bcn == 3 and (pf_flags & 0x80000000):
            bcn, tag = 5, "DXT5-wrapped-BC5"  # DAVA 双通道数据
    if bcn is None:
        return None, "unsupported"
    need = (w // 4) * (h // 4) * BC_BLOCK[bcn]
    if off + need > len(d):
        return None, "short"
    ncomp = 2 if bcn == 5 else (1 if bcn == 4 else 4)
    try:
        arr = np.asarray(imagecodecs.bcn_decode(d[off:off + need], bcn, shape=(h, w, ncomp)))
    except Exception:
        return None, "decode-fail"
    arr = arr.reshape(h, w, ncomp)
    if ncomp == 2:  # BC5：R/G 双通道
        arr = np.dstack([arr[..., 0], arr[..., 1], np.zeros_like(arr[..., 0]),
                         np.full_like(arr[..., 0], 255)])
    elif ncomp == 1:
        arr = np.dstack([arr[..., 0]] * 3 + [np.full_like(arr[..., 0], 255)])
    out = arr.astype(np.uint8)
    if max_dim and max(w, h) > max_dim:
        img = Image.fromarray(out, "RGBA").resize((max_dim, max_dim), Image.LANCZOS)
        out = np.asarray(img)
    return out, tag


# ---------------------------------------------------------------------------
# sc2 / 几何
# ---------------------------------------------------------------------------
def ka_id(v):
    """KeyedArchive 的 #id（int 或 $bytes）→ int。"""
    if isinstance(v, int):
        return v
    if isinstance(v, dict) and isinstance(v.get("$bytes"), str):
        return int.from_bytes(bytes.fromhex(v["$bytes"])[:8], "little", signed=False)
    return None


def entity_components(e: dict) -> dict:
    return {c.get("comp.typename"): c for c in (e.get("components") or {}).values()
            if isinstance(c, dict)}


def nested(container: dict) -> list[dict]:
    return [e for e in (container.get("#hierarchy") or []) if isinstance(e, dict)]


# 顶点布局：SCG 交错顶点的 vertexFormat 位掩码 → 各功能位的字节数（7 种格式实测反推，
# bit 求和 == stride）。positions/normal 恒在 floats 0:6；**UV0 的偏移随格式变化**——
# bits{0,1} → 24B（UV 在 float 6:8，如 E-100）；bits{0,1,2} → 28B（UV 在 float 7:9，
# 中间那个 4 字节通道实测是 NaN，如 P44_Pantera）。硬编码 6:8 会整列错位。
# DAVA 顶点位表（权威来源：dava.engine RenderBase.h `EVF_*` :158-177 + `GetVertexSize`
# :213-261）。此前 bit9/10/12/13 写成 16/8/12/16（四位全错）、并缺 11/14/15/16-19。
# 与 export_map_glb.py 的同名表保持逐项一致。
VERTEX_LAYOUT_BITS = {0: 12, 1: 12, 2: 4, 3: 8, 4: 8, 5: 8, 6: 8, 7: 12, 8: 12,
                      9: 4, 10: 16, 11: 12, 12: 4, 13: 8, 14: 16, 15: 16,
                      16: 12, 17: 12, 18: 12, 19: 12}
# UV 集对应的 vertexFormat 位（0/1/2 是位置/法线/一个 4 字节通道，UV 从 bit3 起每集 8 字节）
UV_BITS = (3, 4, 5)


class Geometry:
    """SCPG：datasource id → 顶点/索引/UV。"""

    def __init__(self, scg_path: pathlib.Path):
        g = read_scg(decode_dvpl(scg_path.read_bytes()))
        self.groups = {ka_id(p["#id"]): p for p in g["polygonGroups"]}

    def verts(self, ds: int) -> np.ndarray:
        p = self.groups[ds]
        raw = decode_bytes(p["vertices"])
        return np.frombuffer(raw, dtype="<f4").reshape(p["vertexCount"], -1)

    def indices(self, ds: int) -> list[int]:
        p = self.groups[ds]
        raw = decode_bytes(p["indices"])
        return list(np.frombuffer(raw, dtype="<H" if p["indexFormat"] == 0 else "<I"))

    def uv_sets(self, ds: int) -> list[np.ndarray | None]:
        """按 vertexFormat 推导出的**全部 UV 集**：[UV0, UV1, UV2]，缺失位为 None。

        偏移由格式位确定性给出（UV 集从 bit3 起、每集 8 字节），正常直接采信；唯一要防的是
        少数格式高位语义不同导致落点跑进别的通道，那时该列会整列非有限值。不要加"数值幅度
        上限"守卫——UV 平铺会合法地超过 1（实测有 89.9），也不要求全有限（实测有网格 UV0 含
        NaN，后续 canon_nan 规范化）。

        BlitzKit 会把这些集分别导成 TEXCOORD_0/1/2（实测 139 辆有 UV1、18 辆有 UV2），
        只导 UV0 会在这批车上少 accessor。
        """
        p = self.groups[ds]
        vf, vc = p.get("vertexFormat"), p.get("vertexCount")
        raw = decode_bytes(p.get("vertices"))
        if not isinstance(vf, int) or not isinstance(vc, int) or vc <= 0 or raw is None:
            return [None, None, None]
        stride, rem = divmod(len(raw), vc)
        if rem or stride < 12:
            return [None, None, None]
        low = 0
        for b in (0, 1, 2):  # 低于 UV 位的功能位累计偏移
            if vf >> b & 1:
                sz = VERTEX_LAYOUT_BITS.get(b)
                if sz is None:
                    return [None, None, None]
                low += sz
        arr = np.frombuffer(raw, dtype=np.uint8).reshape(vc, stride).view("<f4")
        out: list[np.ndarray | None] = []
        off_bytes = low
        for i, bit in enumerate(UV_BITS):
            if not (vf >> bit) & 1:
                out.append(None)
                continue
            off = off_bytes // 4
            if (off + 2) * 4 > stride or off_bytes + 8 > stride:
                out.append(None)
            else:
                uv = arr[:, off:off + 2]
                out.append(uv if np.isfinite(uv).mean() >= 0.5 else None)
            off_bytes += VERTEX_LAYOUT_BITS[bit]
        return (out + [None, None, None])[:3]

    def uvs(self, ds: int) -> np.ndarray | None:
        """UV0（兼容旧调用）。"""
        return self.uv_sets(ds)[0]


class MaterialLib:
    """NMaterial 库：下钻 configArchive_N（取 Default 变体）后沿 parentMaterialKey 合并。"""

    def __init__(self, scene: dict):
        self.by_id = {}
        for n in scene.get("#dataNodes") or []:
            if isinstance(n, dict) and n.get("##name") == "NMaterial":
                i = ka_id(n.get("#id"))
                if i is not None:
                    self.by_id[i] = n
        self._cache: dict[int, dict] = {}

    def _flatten(self, node: dict) -> dict:
        inst = dict(node)
        if inst.get("configCount"):
            cfgs = [inst.get(f"configArchive_{k}") for k in range(inst["configCount"])]
            cfgs = [c for c in cfgs if isinstance(c, dict)]
            pick = next((c for c in cfgs if c.get("configName") == "Default"),
                        cfgs[0] if cfgs else None)
            if pick:
                inst["textures"] = pick.get("textures") or {}
                inst["properties"] = pick.get("properties") or {}
                # customCullMode 可能只在皮肤变体里（`F34_ARL_V39_BP_flag_R_mtr`、`It115_Rinoceronte_mtr` 等）
                if "customCullMode" in pick:
                    inst["customCullMode"] = pick["customCullMode"]
        return inst

    def resolve(self, mid: int | None) -> dict:
        if mid is None:
            return {}
        if mid in self._cache:
            return self._cache[mid]
        out = {"textures": {}, "properties": {}, "materialName": None, "customCullMode": None}
        seen, cur = set(), mid
        while isinstance(cur, int) and cur not in seen:
            seen.add(cur)
            node = self.by_id.get(cur)
            if node is None:
                break
            inst = self._flatten(node)
            out["textures"].update(inst.get("textures") or {})
            out["properties"].update(inst.get("properties") or {})
            if inst.get("materialName"):  # 名字取继承链根（BlitzKit 口径）
                out["materialName"] = inst["materialName"]
            # 子材质优先（继承链由近到远），只在未取值时填
            if out["customCullMode"] is None and inst.get("customCullMode") is not None:
                out["customCullMode"] = inst["customCullMode"]
            cur = node.get("parentMaterialKey") if isinstance(node.get("parentMaterialKey"), int) else None
        self._cache[mid] = out
        return out


def prop_floats(mat: dict, key: str, default=(0.0,)) -> tuple:
    v = (mat.get("properties") or {}).get(key)
    if not (isinstance(v, dict) and isinstance(v.get("$bytes"), str)):
        return tuple(default)
    b = bytes.fromhex(v["$bytes"])
    n = min(4, (len(b) - 5) // 4) if len(b) >= 9 else 0
    if n <= 0:
        return tuple(default)
    return struct.unpack_from(f"<{n}f", b, 5) + tuple(default)[n:]


# ---------------------------------------------------------------------------
# GLB 写入
# ---------------------------------------------------------------------------
def canon_nan(a: np.ndarray) -> np.ndarray:
    """任意 NaN 位型 → 规范 quiet NaN 0x7fc00000（BlitzKit 的 JS 管线口径）。

    客户端源数据里确实存在 0xffc00000（E-100.scg 54 处），不规范化会与目标逐字节不等。
    """
    f = np.asarray(a, dtype="<f4")
    bits = f.view("<u4").copy()
    exp_full = (bits & 0x7F800000) == 0x7F800000
    mant_nonzero = (bits & 0x007FFFFF) != 0
    mask = exp_full & mant_nonzero
    if mask.any():
        bits[mask] = 0x7FC00000
    return bits.view("<f4")


class Glb:
    """最小 GLB 2.0 写入器：identity 变换、单一 buffer、可共享顶点 accessor。"""

    def __init__(self):
        self.buf = bytearray()
        self.views: list[dict] = []
        self.accessors: list[dict] = []
        self.meshes: list[dict] = []
        self.materials: list[dict] = []
        self.images: list[dict] = []
        self.textures: list[dict] = []
        self.samplers = [{"wrapS": 10497, "wrapT": 10497, "magFilter": 9729, "minFilter": 9987}]
        self.nodes: list[dict] = []
        self._img_key: dict = {}
        self._attr_key: dict = {}

    def _view(self, payload: bytes) -> int:
        while len(self.buf) % 4:
            self.buf.append(0)
        o = len(self.buf)
        self.buf.extend(payload)
        self.views.append({"buffer": 0, "byteOffset": o, "byteLength": len(payload)})
        return len(self.views) - 1

    def image(self, key, data: bytes, mime: str) -> int:
        if key in self._img_key:
            return self._img_key[key]
        v = self._view(data)
        self.images.append({"bufferView": v, "mimeType": mime})
        self.textures.append({"source": len(self.images) - 1, "sampler": 0})
        self._img_key[key] = len(self.images) - 1
        return len(self.images) - 1

    def mesh(self, pos: np.ndarray, nrm, uvs, idx: list[int], material, share_key=None) -> int:
        """`uvs`：UV 集列表（[UV0, UV1, UV2]，元素可为 None），按序写成 TEXCOORD_0/1/2。"""
        attrs = dict(self._attr_key[share_key]) if (share_key is not None and share_key in self._attr_key) else {}
        if not attrs:
            p = canon_nan(pos)
            pv = self._view(p.astype("<f4").tobytes())
            self.accessors.append({"bufferView": pv, "componentType": 5126, "count": len(p),
                                   "type": "VEC3", "min": p.min(0).tolist(), "max": p.max(0).tolist()})
            attrs["POSITION"] = len(self.accessors) - 1
            if nrm is not None:
                n = canon_nan(nrm)
                nv = self._view(n.astype("<f4").tobytes())
                self.accessors.append({"bufferView": nv, "componentType": 5126, "count": len(n), "type": "VEC3"})
                attrs["NORMAL"] = len(self.accessors) - 1
            for i, uv in enumerate(uvs or ()):
                if uv is None:
                    continue
                uv = canon_nan(np.asarray(uv, dtype="<f4"))
                vv = self._view(uv.tobytes())
                self.accessors.append({"bufferView": vv, "componentType": 5126, "count": len(uv), "type": "VEC2"})
                attrs[f"TEXCOORD_{i}"] = len(self.accessors) - 1
            if share_key is not None:
                self._attr_key[share_key] = dict(attrs)
        # 索引按最大索引自适应（BlitzKit/glTF-Transform 产 uint16）
        dtype, ctype = ("<u2", 5123) if (idx and max(idx) < 65536) else ("<u4", 5125)
        iv = self._view(np.asarray(idx, dtype=dtype).tobytes())
        self.accessors.append({"bufferView": iv, "componentType": ctype, "count": len(idx), "type": "SCALAR"})
        prim = {"attributes": attrs, "indices": len(self.accessors) - 1, "mode": 4}
        if material is not None:
            prim["material"] = material
        self.meshes.append({"primitives": [prim], "name": "RenderBatch"})
        return len(self.meshes) - 1

    def node(self, name: str, mesh=None, children=None) -> int:
        n = {"name": name}
        if mesh is not None:
            n["mesh"] = mesh
        if children:
            n["children"] = children
        self.nodes.append(n)
        return len(self.nodes) - 1

    def finish(self, roots: list[int], generator: str) -> bytes:
        gltf = {"asset": {"version": "2.0", "generator": generator},
                "scene": 0, "scenes": [{"nodes": roots}], "nodes": self.nodes,
                "meshes": self.meshes, "accessors": self.accessors,
                "bufferViews": self.views, "buffers": [{"byteLength": len(self.buf)}]}
        if self.materials:
            gltf["materials"] = self.materials
        if self.textures:
            gltf["textures"] = self.textures
            gltf["images"] = self.images
            gltf["samplers"] = self.samplers
        js = json.dumps(gltf, separators=(",", ":")).encode()
        while len(js) % 4:
            js += b" "
        bn = bytes(self.buf)
        while len(bn) % 4:
            bn += b"\x00"
        total = 12 + 8 + len(js) + 8 + len(bn)
        return (struct.pack("<III", 0x46546C67, 2, total)
                + struct.pack("<II", len(js), 0x4E4F534A) + js
                + struct.pack("<II", len(bn), 0x004E4942) + bn)


# ---------------------------------------------------------------------------
# 贴图
# ---------------------------------------------------------------------------
# 贴图槽位解析：`--texture-mode semantic` 按 PBR 语义正确装配（推荐）。
#  * 新式车（306 辆）：材质内联 `configArchive_N.textures` 有 baseColorMap / baseNormalMap /
#    baseRMMap / miscMap 四个 PBR 槽；
#  * 老式车（429 辆）：内联槽名是 **legacy 的 `albedo` / `normalmap`**（无 RM/MISC），
#    缺内联时再走命名约定 `3d/Tanks/<Nation>/images/<材质名去 _mtr>`（`T_34_mtr` → `images/T-34`、
#    `T_34_track_mtr` → `images/T-34_track`），后缀 `_NM` 法线、`_RM`/`_MISC`/`_MASK`。
#
# 关于 `--texture-mode blitzkit`（复刻 BlitzKit 现行指派）—— **已移除**：其指派在 PBR 语义上
# 不成立，且逐通道实测（E-100 / `E_100_mtr`）显示报告 B §3.3 的描述也不准：BK 的 `normal`
# 实为 **legacy `normalmap`（旧法线图 DXT1）**、`MR.G` 实为 `baseRMMap` 的 **ch0**、
# `occlusion` 与我们一样取 `miscMap.R`（全量 1011/1011 一致）；真正对不拢的是 **BK 的 MR.B
# （金属度位）**——与客户端任一张贴图的任一通道都差 26+。复刻这种指派没有意义；要看 BlitzKit
# 的实际贴图直接看它的产物（`tools/compare_tank_glb.py` 的并排渲染就是干这个的）。


def _legacy_base(material_name: str | None, stem: str) -> str:
    """材质名 → legacy 贴图文件名主干：去 `_mtr` 后缀，再还原模型名的 `-`/`_` 差异。

    材质名是模型名把 `-` 写成 `_` 后加子部件与 `_mtr`（`T-34` → `T_34_mtr`、
    `T-34_track` → `T_34_track_mtr`），故按 stem 前缀还原而非全局替换
    （`M6E2BP_grill` 里的 `_` 是真下划线，全局替换会错）。
    """
    mn = material_name or ""
    if mn.endswith("_mtr"):
        mn = mn[:-4]
    mangled = stem.replace("-", "_")
    if stem and mn.startswith(mangled):
        return stem + mn[len(mangled):]
    return mn


def _slot_candidates(m: dict, stem: str) -> dict[str, list[str]]:
    """glTF 槽 → 按序尝试的客户端贴图路径（`.tex` 约定；扩展名由 TexStore 拼）。"""
    t = m.get("textures") or {}
    base = _legacy_base(m.get("materialName"), stem)
    img = f"images/{base}" if base else ""
    return {
        # 内联 PBR 槽 → 老式内联 legacy 槽(albedo) → 命名约定 _BC → 命名约定无后缀
        "baseColorTexture": [t.get("baseColorMap"), t.get("albedo"), f"{img}_BC", img],
        "normalTexture": [t.get("baseNormalMap"), t.get("normalmap"), f"{img}_NM"],
        "metallicRoughnessTexture": [t.get("baseRMMap"), f"{img}_RM"],
        "occlusionTexture": [t.get("miscMap"), f"{img}_MISC", f"{img}_MASK"],
    }


def decode_pvr(d: bytes, max_dim: int = 0):
    """DAVA PVR3 容器 → (RGBA ndarray, 标签)。解码器**复用** `tools/export_map_glb.py` 的实现。

    为什么必须回退到它：部分槽位在 PC 包内**只有** `.dx11.pvr.dvpl` 而没有 dds 版
    （如 BT-2 的 `images/BT-2_track` 仅 pvr、而其 `_NM` 有 dds），实测 160 辆车的
    导出槽位属此列——症状是"法线解出来了、baseColor 没有"。

    ⚠️ **保留 `decode_pvr3` 的垂直翻转，不要"对齐" DDS 路径**：坦克侧 DDS 解码不翻转、
    PVR3 解码翻转，两者朝向相反。这不是笔误——本机实测 13 个 PVR 来源的 baseColor 材质
    逐个与 BlitzKit 比对，**13/13 都在翻转后更接近**（如 545 T1_track：原样 MAD 43.6 /
    翻转 23.1）。改动这里会让所有 PVR 贴图上下颠倒。
    """
    try:
        from export_map_glb import decode_pvr3
    except Exception:  # noqa: BLE001
        return None, "no-pvr-decoder"
    img = decode_pvr3(d, max_dim or 100000)
    if img is None:
        return None, "pvr-fail"
    return np.asarray(img.convert("RGBA")), "PVR3"


class TexStore:
    """客户端 `.tex` 引用 → 解码后的 RGBA。路径按 **base 目录相对解析**。"""

    def __init__(self, base_dir: pathlib.Path, max_dim: int):
        self.base, self.max_dim, self.cache = base_dir, max_dim, {}

    def resolve(self, tex_path: str) -> pathlib.Path:
        """`.tex` 路径 → 磁盘主干（不含扩展名）。

        `../` 是**相对 .sc2 所在目录**的跨目录引用（日本联动车复用德国贴图写作
        `../German/images/Hetzer_GuP.tex`），必须按 base 归一化到真实目录；早期实现把
        前缀 `../` 直接剥掉，会拼出 `<Nation>/German/images/...` 这种不存在的路径，
        导致 196 辆跨目录引用贴图的车静默丢槽位。
        """
        stem = tex_path[:-4] if tex_path.lower().endswith(".tex") else tex_path
        return pathlib.Path(os.path.normpath(os.path.join(str(self.base), stem.lstrip("/"))))

    def load(self, tex_path: str):
        if tex_path in self.cache:
            return self.cache[tex_path]
        base = self.resolve(tex_path)
        out = None
        # 扩展名回退链：PC 的 BCn dds → 无前缀 dds → 移动端 PVR3（部分槽位只有它）
        for suf, dec in ((".dx11.dds.dvpl", decode_dds), (".dds.dvpl", decode_dds),
                         (".dx11.pvr.dvpl", decode_pvr)):
            p = pathlib.Path(str(base) + suf)
            if p.exists():
                arr, tag = dec(decode_dvpl(p.read_bytes()), self.max_dim)
                out = (arr, tag, p.name)
                break
        self.cache[tex_path] = out or (None, "missing", "")
        return self.cache[tex_path]


def _has_varying_alpha(arr: np.ndarray) -> bool:
    """贴图是否真的带"有变化的 alpha"（=存在镂空）。恒定不透明的 alpha 不参与判定。"""
    return arr.ndim == 3 and arr.shape[2] == 4 and int(arr[:, :, 3].min()) < 255


def _encode_webp(arr: np.ndarray, keep_alpha: bool = False) -> bytes:
    """编码 webp。`keep_alpha` 时保留 RGBA，alpha 有两个消费方，都不能丢：
    ① 履带等 alphaTest 材质的镂空就在 alpha 里，丢掉会渲染成实心板；
    ② WotbTools 场景运行时把 baseColor 的 alpha 当 DAVA gloss 消费——alpha 一旦
    整列变 1.0（丢 alpha 的 RGB webp 解码即如此），PBR 粗糙度跳到 0 变纯镜面，
    没有环境反射可取，整车发黑（实测全车系变暗）。
    但 libwebp 对 a=0 的像素不保 RGB（视为不可见优化掉，实测该区 RGB 均值偏差
    39.2、不透明区仅 2~3，负重轮轮面整片 a=0 → 横向白条即此）。故保留 alpha 时
    把 alpha 钳到 ≥1/255：gloss 语义不变（1/255≈0），RGB 全图保真（实测偏差 2.2）。"""
    if keep_alpha and arr.ndim == 3 and arr.shape[2] == 4:
        arr = arr.copy()
        arr[:, :, 3] = np.maximum(arr[:, :, 3], 1)
        img = Image.fromarray(arr, "RGBA")
    else:
        img = Image.fromarray(arr, "RGBA" if arr.ndim == 3 and arr.shape[2] == 4 else "RGB").convert("RGB")
    b = io.BytesIO()
    img.save(b, "WEBP", quality=92, method=4)
    return b.getvalue()


def _prep_texture(arr: np.ndarray, slot: str) -> np.ndarray | None:
    """把客户端贴图的通道装配成 glTF 槽期望的布局（PBR 语义口径）。"""
    rgb = arr[:, :, :3]
    if slot == "baseColor":
        return arr  # 保留 alpha（是否真用由 _has_varying_alpha 决定）
    if slot == "normal" and arr.shape[2] >= 3 and arr[:, :, 2].max() == 0:
        # BC5 只存 x/y：按 glTF 约定重建 z = sqrt(1 - x² - y²)（x/y 由 [0,1] 映到 [-1,1]）
        x = rgb[:, :, 0].astype(np.float32) / 255 * 2 - 1
        y = rgb[:, :, 1].astype(np.float32) / 255 * 2 - 1
        z = np.sqrt(np.clip(1 - x * x - y * y, 0, 1))
        return np.dstack([rgb[:, :, 0], rgb[:, :, 1], (z * 127.5 + 127.5).astype(np.uint8)])
    if slot == "metallicRoughness":
        # DAVA 的 `baseRMMap` 是 BC5 **双通道**：ch0=粗糙度、ch1=金属度（解码后落在 R/G 位、
        # B 位补零）。glTF 的 `metallicRoughnessTexture` 规定 **G=粗糙度、B=金属度**。
        # 粗糙度直接线性搬运（corr( 本解码, BlitzKit 产物 )=+0.996）。
        # 金属度**不能**线性搬运：BlitzKit 的 VFS 把 RM 解析到 PVR 无压缩源（authored
        # 通道，逐块对照证明 DDS 色块无法复原它），其分布 p50≈5；而 DDS 硬件 BC5 ch1
        # p50≈78——线性搬运会整车金属化、PBR 发黑（用户实测）。用双车合并拟合的单调
        # LUT 做**实证标定**（使导出分布对齐 BlitzKit 可见输出），非规范语义。
        if arr.shape[2] >= 3 and arr[:, :, 2].max() == 0:
            out = np.zeros_like(rgb)
            out[:, :, 1] = arr[:, :, 0]                                       # ch0 → G（粗糙度）
            # 金属度标定：DDS 里没有金属度（色块是占位常数——BC1-G 解码恒为 ~28 且逐块
            # 无结构；BlitzKit 的 VFS 把 RM 解析到 PVR 无压缩源才有 authored 通道）。硬件
            # BC5 ch1 只是索引字节噪声（p25=0 的块级量化散点），线性/查表搬运都会渲染成
            # "细碎杂乱、轮盘深浅不一"。做法：LUT 标定分布 + 高斯平滑压掉块级噪声。
            metal_lut_x = (8, 24, 40, 56, 72, 88, 104, 120, 136, 152, 168, 184, 200, 216, 232, 248)
            metal_lut_y = (3, 4, 5, 13, 13, 32, 41, 68, 79, 79, 82, 85, 87, 93, 132, 184)
            metal = np.interp(arr[:, :, 1], metal_lut_x, metal_lut_y).astype(np.uint8)
            radius = max(2, arr.shape[0] // 512)
            metal = np.asarray(Image.fromarray(metal, "L").filter(ImageFilter.GaussianBlur(radius)))
            out[:, :, 2] = metal
            return out
        return rgb  # 三通道来源（老式车的 images/<T>_RM）：通道语义未证实，原样保留
    if slot == "occlusion":
        return np.dstack([rgb[:, :, 0]] * 3)  # AO 在 R 通道
    return rgb


def _gltf_slot_name(glsl: str) -> str:
    return {"baseColorTexture": "baseColor", "normalTexture": "normal",
            "metallicRoughnessTexture": "metallicRoughness", "occlusionTexture": "occlusion"}[glsl]


# ---------------------------------------------------------------------------
# 导出
# ---------------------------------------------------------------------------
def _excluded(name: str) -> bool:
    """BlitzKit 三套产物都不含的源节点（特效锚点/烘焙占位/开关态子树）。"""
    if name.startswith("HP_"):
        return True
    if "_crash_" in name or "chassis_chassis" in name:
        return True
    low = name.lower()
    if "dummy" in low:
        return True
    # 烘焙 LOD 变体实体（chassis_track_R_lod2 / gun_01_lod_nc_lod2）：客户端按距离切换，
    # BlitzKit 产物不含。注意**不能**推广到 `_anim_*`——骨架子树（gun_03_mask_anim_tower、
    # umbrella_anim、..._fish_anim 等）BlitzKit 是保留的。
    if _LOD_SUFFIX_RE.search(name):
        return True
    # StateSwitcher 的高状态子树（损毁/变体形态）：初始态为 0，其 batch switchIndex
    # 缺省通配时会漏进来叠在完整形态上（与 tools/export_map_glb.py 同一判据）
    if _STATE_RE.search(name):
        return True
    return False


_STATE_RE = re.compile(r"State ?[1-9]")
_LOD_SUFFIX_RE = re.compile(r"_lod\d")


def export_visual(sc2_path: pathlib.Path, glb: Glb, tex: TexStore, mode: str, stem: str) -> tuple[list[int], dict]:
    scene = read_sc2(decode_dvpl(sc2_path.read_bytes()))
    mats = MaterialLib(scene)
    geo = Geometry(sc2_path.with_name(sc2_path.name.replace(".sc2.dvpl", ".scg.dvpl")))
    stats = {"nodes": 0, "meshes": 0, "tris": 0, "materials": 0, "tex_fail": []}
    mat_cache: dict[str, int] = {}

    def material_for(mid):
        m = mats.resolve(mid)
        name = m.get("materialName") or "mat"
        if name in mat_cache:
            return mat_cache[name]
        mat = {"name": name, "pbrMetallicRoughness": {}}
        pbr = mat["pbrMetallicRoughness"]
        # 色贴图的 alpha 是否真有镂空（None = 未导出贴图，无从判断）
        base_alpha_varies: bool | None = None if mode == "none" else False
        if mode != "none":
            for glsl, cands in _slot_candidates(m, stem).items():
                arr = fname = None
                wrapped_normal = None  # 法线槽的 wrapped-BC5 垫底候选
                for cp in cands:
                    if not cp:
                        continue
                    arr, tag, fname = tex.load(cp)
                    if arr is None:
                        continue
                    # DXT5-wrapped-BC5 的 NM 没有 Y 通道：alpha 块=X（与 legacy 法线
                    # corr +0.999），色块是占位常数（BC1-G 恒 ~28、无结构）——游戏/BK
                    # 的 Y 来自我们 PC 包里没有的数据。硬件 BC5 读出的 ch1 是索引噪声，
                    # 渲染出来就是"材质细碎杂乱、轮盘深浅不一"。优先回退 legacy
                    # `images/<T>_NM`（完整 DXT1 XYZ）；连它都没有时才垫底并拍平 Y。
                    if glsl == "normalTexture" and tag == "DXT5-wrapped-BC5":
                        if wrapped_normal is None:
                            wrapped_normal = (arr, tag, fname)
                        arr = None
                        continue
                    break
                if arr is None and wrapped_normal is not None:
                    arr = wrapped_normal[0].copy()
                    arr[:, :, 1] = 128  # Y 拍平：保留真实 X + 重建 Z，不渲染噪声
                    fname = wrapped_normal[2]
                if arr is None:
                    if any(cands):
                        stats["tex_fail"].append(f"{glsl}:{cands[0]} (missing)")
                    continue
                slot = _gltf_slot_name(glsl)
                out = _prep_texture(arr, slot)
                keep_alpha = _has_varying_alpha(out)
                if glsl == "baseColorTexture":
                    base_alpha_varies = keep_alpha
                i = glb.image((fname, glsl, mode, keep_alpha),
                              _encode_webp(out, keep_alpha), "image/webp")
                target = pbr if glsl in ("baseColorTexture", "metallicRoughnessTexture") else mat
                target[glsl] = {"index": i}
        # alphaTest 材质（履带/镂空）→ MASK + cutoff。判据是**两个条件同时成立**：
        #   ① 材质带 `alphatestThreshold` 属性（车体没有、履带有）；cutoff 直接取该属性值
        #      （实测与 BlitzKit 逐值相等：0.3/0.05/0.5/0.03 全对）；
        #   ② 其色贴图的 alpha 真的有变化——仅凭 ① 会把 24% 的材质误判成 MASK（实测 169 例），
        #      加上 ② 后 166/169 吻合。
        # 注：早期实现用 `attenuationBoxPosition` 判据，导致 407 个 BlitzKit 标为 MASK 的材质
        # 被输出成不透明（履带渲染成实心板）。
        props = m.get("properties") or {}
        if "alphatestThreshold" in props and base_alpha_varies is not False:
            mat["alphaMode"] = "MASK"
            mat["alphaCutoff"] = float(prop_floats(m, "alphatestThreshold", (0.0,))[0] or 0.0)
        # doubleSided 与 alphaMode **无关**，判据是客户端的 `customCullMode`：
        # 0 = DAVA cull mode NONE（不剔除）→ 双面。全量实测 **1598/1598 材质零反例**
        # （810 个 cullMode=0 全为双面；786 个缺省 + 2 个 =2 全为单面）。
        # 注意该字段可能在 NMaterial 顶层，也可能只在其皮肤变体 `configArchive_N` 里。
        if m.get("customCullMode") == 0:
            mat["doubleSided"] = True
        glb.materials.append(mat)
        mat_cache[name] = len(glb.materials) - 1
        stats["materials"] = len(glb.materials)
        return mat_cache[name]

    def build(e: dict):
        comps = entity_components(e)
        name = e.get("name") or e.get("##name") or "node"
        rc = comps.get("RenderComponent")
        mesh_nodes: list[int] = []
        # 注：坦克**不做**实体可见位过滤——客户端进场隐藏的皮肤/开关态子树（visibility=2）
        # BlitzKit 产物里是保留的（已实证 hull_hide_elements_skin3 vis=2）。这与
        # tools/export_map_glb.py 的地图规则不同，属两套产物的目标差异。
        # 但**必须有 LodComponent**：那是"参与 LOD 系统的渲染实体"的判据，主体部件全部具备；
        # 少数游离对象（Object117 / polySurface2）、动画骨持有节点（hull_anim_bore）与
        # 引用的子场景（EagleSpirit2.sc2）都没有，BlitzKit 产物同样不含。
        if rc and not _excluded(name) and comps.get("LodComponent") is not None:
            ro = rc.get("rc.renderObj") or {}
            batches = ro.get("ro.batches") or {}
            switcher = comps.get("StateSwitcherComponent")
            active = switcher.get("ssc.activeState", 0) if switcher else 0
            if not isinstance(active, int):
                active = 0
            # 客户端批次激活规则（同 tools/export_map_glb.py）：lodIndex 取 LOD0 或通配(-1)、
            # switchIndex 取 activeState 或通配；**所有通过筛选的批次都导出**（同一部件可能有
            # 0000/0001 两个 LOD0 批次，只取一个会漏几何），另外丢弃 Shadow_Material 烘焙阴影。
            for bk in sorted(batches):
                b = batches[bk]
                if not isinstance(b, dict):
                    continue
                bi = int(bk)
                if ro.get(f"rb{bi}.lodIndex", -1) not in (0, -1):
                    continue
                if ro.get(f"rb{bi}.switchIndex", -1) not in (active, -1):
                    continue
                nm = ka_id(b.get("rb.nmatname"))
                if (mats.resolve(nm).get("materialName") or "") == "Shadow_Material":
                    continue
                ds = ka_id(b.get("rb.datasource"))
                if ds is None or ds not in geo.groups:
                    continue
                v = geo.verts(ds)
                idx = geo.indices(ds)
                mesh_nodes.append(glb.node(bk, mesh=glb.mesh(
                    v[:, 0:3], v[:, 3:6], geo.uv_sets(ds), idx, material_for(nm))))
                stats["meshes"] += 1
                stats["tris"] += len(idx) // 3
        kids = [c for c in (build(k) for k in nested(e)) if c is not None]
        if not mesh_nodes and not kids:
            return None
        stats["nodes"] += 1
        return glb.node(name, children=mesh_nodes + kids)

    roots = [n for n in (build(e) for e in nested(scene)) if n is not None]
    return roots, stats


def export_collision(sc2_path: pathlib.Path, scg_path: pathlib.Path, glb: Glb) -> tuple[list[int], dict]:
    scene = read_sc2(decode_dvpl(sc2_path.read_bytes()))
    geo = Geometry(scg_path)
    roots, stats = [], {"plates": 0, "tris": 0}
    for e in nested(scene):
        rc = entity_components(e).get("RenderComponent")
        if not rc:
            continue
        part = e.get("name")
        for b in (rc.get("rc.renderObj") or {}).get("ro.batches", {}).values():
            ds = ka_id(b.get("rb.datasource"))
            if ds is None or ds not in geo.groups:
                continue
            v = geo.verts(ds)
            pos, nrm = v[:, 0:3], v[:, 3:6]
            plate_of = np.rint(v[:, v.shape[1] - 1]).astype(int)
            idx = geo.indices(ds)
            # 按装甲板号切连续 run（客户端三角序本就按板分组 → 与 BlitzKit 逐节点一致）
            runs, cur = [], None
            for t in range(len(idx) // 3):
                a = int(plate_of[idx[3 * t]])
                if cur is None or a != cur[0]:
                    if cur:
                        runs.append(cur)
                    cur = [a, []]
                cur[1].extend(idx[3 * t:3 * t + 3])
            if cur:
                runs.append(cur)
            for plate, ridx in runs:
                mi = glb.mesh(pos, nrm, None, ridx, None, share_key=("collision", part, ds))
                roots.append(glb.node(f"{part}_armor_{plate}", mesh=mi))
                stats["plates"] += 1
                stats["tris"] += len(ridx) // 3
    return roots, stats


# ---------------------------------------------------------------------------
# 路径解析 / 单辆导出
# ---------------------------------------------------------------------------
def read_params_yaml(game_data: pathlib.Path, nation: str, stem: str) -> dict:
    p = client_path(game_data, f"3d/Tanks/Parameters/{nation}/{stem}.yaml.dvpl")
    if not p.exists():
        return {}
    txt = decode_dvpl(p.read_bytes()).decode("utf-8", "replace")
    out = {}
    for line in txt.splitlines():
        ls = line.strip()
        if ls.startswith("blitzModelPath:"):
            out["blitzModelPath"] = ls.split(":", 1)[1].strip().strip('"')
        elif ls.startswith("collisionMesh:"):
            out["collisionMesh"] = ls.split(":", 1)[1].strip().strip('"')
    return out


def resolve_tank(game_data: pathlib.Path, nation: str, stem: str) -> dict:
    """权威路径：yaml 的 blitzModelPath / collisionMesh；缺失时按目录约定回退。

    所有路径经 `client_path()` 解析——DLC 覆盖层（`packs/`）同名路径优先于 `Data/`。
    `.sc2` 与 `.scg` **各自独立解析**：客户端可能只覆盖其中之一。
    """
    nd = NATION_DIR.get(nation, nation)
    info = read_params_yaml(game_data, nation, stem)
    res = {"nation": nation, "stem": stem, "nation_dir": nd, "yaml": bool(info)}
    bm = info.get("blitzModelPath")
    sc2_rel = (f"3d/{bm}.dvpl") if bm else f"3d/Tanks/{nd}/{stem}.sc2.dvpl"
    res["model_sc2"] = client_path(game_data, sc2_rel)
    res["model_scg"] = client_path(game_data, sc2_rel.replace(".sc2.dvpl", ".scg.dvpl"))
    cm = info.get("collisionMesh")
    csc2_rel = (f"3d/Tanks/{cm}.dvpl") if cm else \
        f"3d/Tanks/CollisionMeshes/{nation}-{stem}.sc2.dvpl"
    res["coll_sc2"] = client_path(game_data, csc2_rel)
    res["coll_scg"] = client_path(game_data, csc2_rel.replace(".sc2.dvpl", ".scg.dvpl"))
    return res


def export_tank(game_data: pathlib.Path, tank_id: int, nation: str, stem: str,
                out_root: pathlib.Path, mode: str = "semantic", max_tex: int = 0) -> dict:
    """导出单辆 → <out_root>/<tank_id>/{model.glb,collision.glb}。不抛出缺文件异常。"""
    res = resolve_tank(game_data, nation, stem)
    out = out_root / str(tank_id)
    out.mkdir(parents=True, exist_ok=True)
    st = {"tank_id": tank_id, "nation": nation, "stem": stem,
          "model_sc2": str(res["model_sc2"]), "yaml": res["yaml"]}
    if not res["model_sc2"].exists():
        st["error"] = "visual .sc2 missing"
        return st
    try:
        glb = Glb()
        tex = TexStore(res["model_sc2"].parent, max_tex)
        roots, s = export_visual(res["model_sc2"], glb, tex, mode, stem)
        data = glb.finish(roots, "wotb-agent local sc2 exporter")
        (out / "model.glb").write_bytes(data)
        st.update({k: s[k] for k in ("nodes", "meshes", "tris", "materials")})
        st["tex_fail"] = s["tex_fail"]
        st["model_bytes"] = len(data)
    except Exception as e:  # noqa: BLE001
        st["error"] = f"visual: {type(e).__name__}: {e}"
        return st
    if res["coll_sc2"].exists() and res["coll_scg"].exists():
        try:
            g2 = Glb()
            croots, cs = export_collision(res["coll_sc2"], res["coll_scg"], g2)
            cd = g2.finish(croots, "wotb-agent local sc2 exporter")
            (out / "collision.glb").write_bytes(cd)
            st["coll_plates"], st["coll_tris"], st["coll_bytes"] = cs["plates"], cs["tris"], len(cd)
        except Exception as e:  # noqa: BLE001
            st["coll_error"] = f"{type(e).__name__}: {e}"
    else:
        st["coll_error"] = "collision files missing"
    return st


def _export_one(payload: tuple) -> dict:
    """ProcessPool 的模块级 worker（Windows spawn 下闭包不可 pickle，参数一律走可序列化值）。"""
    game_data, tank_id, nation, stem, out_root, mode, max_tex = payload
    return export_tank(pathlib.Path(game_data), tank_id, nation, stem,
                       pathlib.Path(out_root), mode, max_tex)


def _write_status(path: pathlib.Path, state: dict) -> None:
    state["updated_at"] = time.strftime("%H:%M:%S")
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, ensure_ascii=False, indent=1), encoding="utf-8")
    tmp.replace(path)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--game-data", type=pathlib.Path, default=None,
                    help="客户端 Data 目录（缺省按常见 Steam 路径自动探测）")
    ap.add_argument("--pb", type=pathlib.Path, default=REPO_ROOT / "data" / "tanks.pb")
    ap.add_argument("--out", type=pathlib.Path, default=pathlib.Path("data/cache/local_models"))
    ap.add_argument("--tank", action="append", default=[],
                    help="tank_id（可重复）；与 --all 互斥")
    ap.add_argument("--all", action="store_true", help="导出 tank 表全部车辆")
    ap.add_argument("--texture-mode", choices=["semantic", "none"], default="semantic")
    ap.add_argument("--max-tex", type=int, default=0, help="贴图最长边上限（0=不缩放，保持客户端原始尺寸）")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--list", action="store_true", help="只列出 tank_id → 模型名")
    args = ap.parse_args()

    table = read_tank_table(args.pb)
    if args.list:
        for tid in sorted(table):
            t = table[tid]
            print(f"{tid}\t{t['nation']}\t{t['stem']}")
        return 0

    game_data = args.game_data or default_game_data()
    if not game_data.is_dir():
        print(f"!! 客户端 Data 目录不存在: {game_data}（用 --game-data 指定）", file=sys.stderr)
        return 2

    if args.all:
        targets = [(tid, t["nation"], t["stem"]) for tid, t in sorted(table.items())]
    elif args.tank:
        targets = []
        by_stem = {t["stem"]: tid for tid, t in table.items()}
        for s in args.tank:
            tid = int(s) if s.isdigit() else by_stem.get(s)
            if tid is None or tid not in table:
                print(f"!! 未知坦克: {s}", file=sys.stderr)
                return 2
            targets.append((tid, table[tid]["nation"], table[tid]["stem"]))
    else:
        print("!! 需要 --tank <id> 或 --all", file=sys.stderr)
        return 2

    args.out.mkdir(parents=True, exist_ok=True)
    status = args.out / "_export_status.json"
    state = {"total": len(targets), "done": 0, "failed": 0, "finished": False,
             "texture_mode": args.texture_mode, "results": [], "failures": []}
    started = time.time()
    _write_status(status, state)
    print(f"客户端: {game_data}\n输出:   {args.out}（BlitzKit 缓存 data/cache/models/ 不受影响）"
          f"\n贴图口径: {args.texture_mode}  车辆数: {len(targets)}", flush=True)

    def run(t):
        return export_tank(game_data, t[0], t[1], t[2], args.out, args.texture_mode, args.max_tex)

    payloads = [(str(game_data), tid, nat, stem, str(args.out), args.texture_mode, args.max_tex)
                for tid, nat, stem in targets]
    with concurrent.futures.ProcessPoolExecutor(max_workers=args.jobs) as pool:
        for st in pool.map(_export_one, payloads):
            if st.get("error"):
                state["failed"] += 1
                state["failures"].append(st)
                print(f"[fail] {st['tank_id']} {st['stem']}: {st['error']}", flush=True)
            else:
                state["done"] += 1
                state["results"].append(st)
            if (state["done"] + state["failed"]) % 25 == 0:
                state["elapsed_sec"] = round(time.time() - started, 1)
                _write_status(status, state)
                print(f"  {state['done'] + state['failed']}/{len(targets)}", flush=True)

    state["finished"] = True
    state["elapsed_sec"] = round(time.time() - started, 1)
    _write_status(status, state)
    print(f"完成：成功 {state['done']}，失败 {state['failed']}，耗时 {state['elapsed_sec']}s")
    return 0 if state["failed"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
