"""DLC 覆盖层（packs）路径解析。

客户端把 DLC 微更新写到 `%LOCALAPPDATA%\\wotblitz\\packs`，**以相同的相对路径覆盖**
游戏 `Data/` 下的同名文件（第三方 mod 工具文档明写 "its files override the base ones"）。

本机实测（2026-10-08）：packs 下 45 个 `.dvpl` 中 **5 个与 Data 同名不同内容**——
`3d/Tanks/German/Ferdinand.sc2.dvpl`（10458 vs 7832 B）、同车 `.scg.dvpl`
（922191 vs 791385 B）、`XML/item_defs/vehicles/common/camouflages.xml.dvpl`、
`camouflages.yaml.dvpl`、`3d/Customization.yaml.dvpl`；另 **40 个 Data 里根本没有**
（全是 `G37_Ferdinand_skin` 皮肤资产）。坦克可视模型被覆盖这一条直接命中本仓的
坦克 GLB 导出链。

**所有对客户端资源的读取都应经 `client_path()`**：直接拼 `game_data / rel` 会读到
DLC 应用前的旧版本。非 Windows / 无该目录时自动退回只读 `Data/`。

（Rust 侧同义实现见 `src/wargaming/game_extract.rs` 的 `packs_dir` / `resolve_client_path`。）
"""
from __future__ import annotations

import os
import pathlib

_PACKS: pathlib.Path | None | bool = False  # False = 尚未探测


def packs_dir() -> pathlib.Path | None:
    """`%LOCALAPPDATA%\\wotblitz\\packs`；不存在则 None（结果缓存）。"""
    global _PACKS
    if _PACKS is False:
        base = os.environ.get("LOCALAPPDATA")
        cand = pathlib.Path(base) / "wotblitz" / "packs" if base else None
        _PACKS = cand if (cand is not None and cand.is_dir()) else None
    return _PACKS  # type: ignore[return-value]


def client_path(game_data: pathlib.Path, rel) -> pathlib.Path:
    """客户端资源路径：优先 `packs/<rel>`，缺失回退 `game_data/<rel>`。

    `rel` 传 `/` 分隔的相对路径（与 DVPL 内部路径同形），或若干段组成的序列。
    """
    if isinstance(rel, (str, pathlib.PurePath)):
        parts = pathlib.PurePosixPath(str(rel).replace("\\", "/")).parts
    else:
        parts = tuple(str(p) for p in rel)
    packs = packs_dir()
    if packs is not None:
        cand = packs.joinpath(*parts)
        if cand.exists():
            return cand
    return game_data.joinpath(*parts)
