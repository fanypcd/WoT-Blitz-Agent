#!/usr/bin/env python3
"""从本机客户端 item_defs 提取动态装甲（`<extras><armorsStatesController>`）声明。

产物 `data/armor_states.json`：逐车状态表（armorsStates propID=0 的状态语义源）。
运行时门控契约见 `crates/replay-core/src/facets/ai_review.rs` 的 RawArmorStateTrack
文档：值 ∉ 该车状态 id 集的更新丢弃。

用法：
    python tools/extract_armor_states.py --game "D:\\SteamLibrary\\steamapps\\common\\World of Tanks Blitz"
    python tools/extract_armor_states.py --game <...> --out /tmp/armor_states.json
"""

from __future__ import annotations

import argparse
import json
import lz4.block
import pathlib
import re
import struct
import zipfile

DEFAULT_OUT = pathlib.Path(__file__).resolve().parent.parent / "data" / "armor_states.json"


def dvpl(data: bytes) -> bytes:
    assert data[-4:] == b"DVPL", "not a DVPL payload"
    decoded, _enc, _crc, typ = struct.unpack("<IIII", data[-20:-4])
    payload = data[:-20]
    if typ == 0:
        return payload
    if typ == 2:
        return lz4.block.decompress(payload, uncompressed_size=decoded)
    raise ValueError(f"unsupported DVPL type {typ}")


def parse_states(xml: str) -> list[dict] | None:
    """行级解析 `<extras><armorsStatesController>`（客户端产出格式固定，与
    extract_vehicles.py / export_tank_glb.py 的行级解析同一纪律）。"""
    m = re.search(r"<armorsStatesController>(.*?)</armorsStatesController>", xml, re.S)
    if not m:
        return None
    states: list[dict] = []
    for block in re.finditer(r"<state>(.*?)</state>", m.group(1), re.S):
        body = block.group(1)

        def scalar(key: str) -> str | None:
            sm = re.search(rf"<{key}>(.*?)</{key}>", body, re.S)
            return sm.group(1).strip() if sm else None

        armors = scalar("armors")
        if armors is None:
            raise ValueError("state block without <armors> (fail-closed)")
        states.append(
            {
                "id": int(scalar("id")),
                "armors": armors.split(),
                "type": scalar("type") or "",
                "enabled": (scalar("enabled") or "").lower() == "true",
                "default": "<default/>" in body,
            }
        )
    return states


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--game", required=True, help="WoT Blitz 安装根目录")
    ap.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)
    args = ap.parse_args()

    zip_path = pathlib.Path(args.game) / "Data" / "XML" / "item_defs.zip"
    zf = zipfile.ZipFile(zip_path)

    out: dict[str, dict] = {}
    for name in sorted(zf.namelist()):
        if not re.fullmatch(r"vehicles/[^/]+/[^/]+\.xml\.dvpl", name):
            continue
        xml = dvpl(zf.read(name)).decode("utf-8", "replace")
        states = parse_states(xml)
        if states is None:
            continue
        nation, stem = name.split("/")[1], name.split("/")[2].removesuffix(".xml")
        if nation == "common":
            # 通用模板（vehicle.xml）不对应实车，跳过
            continue
        ids = [s["id"] for s in states]
        assert len(ids) == len(set(ids)), f"{name}: duplicate state ids {ids}"
        out[stem] = {"file": name.removesuffix(".dvpl"), "states": states}

    args.out.write_text(json.dumps(out, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    total_states = sum(len(v["states"]) for v in out.values())
    print(f"wrote {args.out}: {len(out)} vehicles, {total_states} states")


if __name__ == "__main__":
    main()
