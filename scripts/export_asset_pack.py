#!/usr/bin/env python3
"""导出纯静态资产包（架构契约 §13；供对象存储/静态托管直传）。

把 Agent 资产端点的"文件选择逻辑"在打包时执行一次，产物是傻瓜静态主机
都能伺服的目录树——前端配置 `?assets=<部署域名>` 后按静态路径取用：

    <包根>/
    ├── index.json                      # 数字 id → key/space/display（前端一次性装载）
    ├── glb/{tank_id}/model.glb, collision.glb
    ├── tank_images/{id}.webp
    ├── data/{tanks.pb, models.pb, tank_cache.json, data_version.json}
    └── map/{key}/
        ├── ground.webp                 # /api/playback/map 的高清底图答案
        ├── mini.webp                   # res=mini 的答案
        ├── terrain.u16.bin             # /api/playback/terrain 的答案
        ├── terrain.json                # X-Terrain-Meta 头的 sidecar 化 {size,zmax,zmin,span}
        ├── scenery.glb                 # 场景 GLB
        ├── ground.layers.json          # groundmeta 的答案
        └── ground/{cm,lm,tile0,tile1,mask0,mask1,hmap0,hmap1}.webp  # groundtex 的答案

用法：
    wotb-agent dump-map-index > map_index.json      # 需游戏客户端在场（注册表源）
    python scripts/export_asset_pack.py --map-index map_index.json
    # --out 缺省为仓库根 release/asset_pack；如显式指定请用绝对路径——
    # 相对路径按 cwd 解析，曾在 frontend/dist 下误落 1.3GB 副本。

仅打包已存在的文件（缺卷积贴图/场景的地图自动缩量）；manifest.json 附全量 sha256。
"""

import argparse
import hashlib
import json
import shutil
import subprocess
from datetime import datetime, timezone
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
GROUND_LAYERS = ["cm", "lm", "tile0", "tile1", "mask0", "mask1", "hmap0", "hmap1"]


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def copy_first(srcs: list[Path], dst: Path) -> bool:
    """按优先级拷贝首个存在的来源（镜像服务端回退链）；返回是否命中。"""
    for s in srcs:
        if s.is_file():
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(s, dst)
            return True
    return False


def git_provenance() -> dict:
    """包的来源：生成它的 upstream commit，以及生成时工作区是否干净。

    `worktree_dirty=True` 的包**不等于**该 commit——消费方据此判断能否用包内的
    sha256 去对照某个 commit 复现。取不到 git（非仓库/无 git）时返回空 dict，
    不因此中断打包（旧行为）。
    """
    try:
        sha = subprocess.run(["git", "rev-parse", "HEAD"], cwd=PROJECT_ROOT,
                             capture_output=True, text=True, check=True).stdout.strip()
        dirty = bool(subprocess.run(["git", "status", "--porcelain"], cwd=PROJECT_ROOT,
                                    capture_output=True, text=True, check=True).stdout.strip())
    except Exception:
        return {}
    return {"upstream_commit": sha, "worktree_dirty": dirty}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", type=Path, default=PROJECT_ROOT / "release" / "asset_pack")
    ap.add_argument("--data", type=Path, default=PROJECT_ROOT / "data")
    ap.add_argument("--map-index", type=Path, required=True,
                    help="wotb-agent dump-map-index 的 JSON 输出")
    args = ap.parse_args()

    maps = json.loads(args.map_index.read_text(encoding="utf-8"))
    data = args.data
    cache_maps = data / "cache" / "maps"
    terrain_cache = data / "cache" / "terrain"
    out = args.out
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    files: list[Path] = []

    def cp(src: Path, rel: str) -> bool:
        dst = out / rel
        if src.is_file():
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
            files.append(dst)
            return True
        return False

    # ---- 坦克 GLB（data/cache/models/{id}/，四级回退的最终答案已落盘） ----
    glb_n = 0
    models_dir = data / "cache" / "models"
    if models_dir.is_dir():
        for f in sorted(models_dir.rglob("*.glb")):
            if cp(f, f"glb/{f.parent.name}/{f.name}"):
                glb_n += 1

    # ---- 坦克封面 ----
    img_n = 0
    img_dir = data / "cache" / "tank_images"
    if img_dir.is_dir():
        for f in sorted(img_dir.rglob("*.webp")):
            if cp(f, f"tank_images/{f.name}"):
                img_n += 1

    # ---- 射击复现数据前提：碰撞盒 JSON（game_data/）+ per-tank 数据（tank/，dump-tank-data 产物） ----
    gd = data / "game_data"
    if gd.is_dir():
        for f in sorted(gd.rglob("*.json")):
            cp(f, f"game_data/{f.relative_to(gd).as_posix()}")
    td = data / "tank_data"
    if td.is_dir():
        for f in sorted(td.rglob("*.json")):
            cp(f, f"tank/{f.name}")

    # ---- 核心数据（前端展示名/弹种表的自足来源） ----
    core_n = 0
    for name in ["tanks.pb", "models.pb", "tank_cache.json", "data_version.json"]:
        if cp(data / name, f"data/{name}"):
            core_n += 1

    # 精简展示名表（id → 名称）：客户端路径 tank_name 的自足来源（wotbagent 数据，不依赖消费方 tankopedia）
    tc = data / "tank_cache.json"
    if tc.is_file():
        names = {}
        for tid, info in json.loads(tc.read_text(encoding="utf-8")).items():
            name = (info or {}).get("name")
            if name:
                names[str(tid)] = name
        (out / "data" / "tank_names.json").write_text(
            json.dumps(names, ensure_ascii=False, sort_keys=True), encoding="utf-8")
        files.append(out / "data" / "tank_names.json")
        core_n += 1

    # ---- 地图（按注册表逐张物化；对齐四个端点的文件选择语义） ----
    map_n = 0
    with_terrain = 0
    missing_scale = []
    seen_spaces: dict[str, str] = {}   # space → 首个物化 key（变体图共享资产）
    for e in maps:
        key, space = e["key"], e["space"]
        if space in seen_spaces:
            continue  # 同 space 变体：资产只物化一份，index 多 id 指向同一目录
        seen_spaces[space] = key
        mid = str(e["map_id"])
        mdir = out / "map" / key
        got = []

        # map?id= 高清底图：手动覆盖（data/maps/<name|key>.{webp,png,jpg,jpeg}）
        # → data/cache/maps/{space}.ground.webp
        overrides = [data / "maps" / f"{s}.{ext}"
                     for s in (e.get("_param", key), key)
                     for ext in ("webp", "png", "jpg", "jpeg")]
        if copy_first(overrides + [cache_maps / f"{space}.ground.webp"],
                      mdir / "ground.webp"):
            got.append("ground")

        # res=mini：提取缓存 {space}.minimap.webp（客户端提取/随包兜底为运行时链，包外）
        if (cache_maps / f"{space}.minimap.webp").is_file():
            cp(cache_maps / f"{space}.minimap.webp", f"map/{key}/mini.webp")
            got.append("mini")

        # terrain：手动覆盖 data/maps/{key}.heightmap.u16.bin
        #          → 缓存 data/cache/terrain/{key}.hm.u16.bin；
        # 尺度 = sidecar data/cache/maps/{space}.json 的 worldBounds（缺尺度不打包 terrain
        # ——与服务端"错标尺度比 404 更糟"同一裁决），sidecar 物化为 terrain.json
        scale_src = cache_maps / f"{space}.json"
        scale = None
        if scale_src.is_file():
            v = json.loads(scale_src.read_text(encoding="utf-8"))
            b = v.get("worldBounds") or {}
            mn, mx = b.get("min"), b.get("max")
            if isinstance(mn, list) and isinstance(mx, list) and len(mn) >= 3 and len(mx) >= 3:
                # span = 水平世界跨度 max(dx,dy)——与服务端 terrain_scale（map_assets.rs）
                # 同式；前端 sampleHeight/rebuildGround 以它做水平米制换算。
                # 勿写 zmax-zmin（那是垂直高度差）：错值会把地形压成 span×span 小块
                span_h = max(float(mx[0]) - float(mn[0]), float(mx[1]) - float(mn[1]))
                scale = {"size": 512,
                         "zmax": round(float(mx[2]), 1),
                         "zmin": round(float(mn[2]), 1),
                         "span": round(span_h, 1)}
        terrain_srcs = [data / "maps" / f"{key}.heightmap.u16.bin",
                        terrain_cache / f"{key}.hm.u16.bin"]
        hit = copy_first(terrain_srcs, mdir / "terrain.u16.bin")
        if hit and scale:
            # terrain.json = X-Terrain-Meta 头的 sidecar 化：顶层必须是前端消费的
            # size/zmax/zmin/span（worldBounds 等导出器原字段保留在子对象）
            got.append("terrain")
            with_terrain += 1
        if hit:
            side = {}
            if scale_src.is_file():
                try:
                    side = json.loads(scale_src.read_text(encoding="utf-8"))
                except Exception:
                    side = {}
            side.update(scale or {})
            (out / f"map/{key}/terrain.json").write_text(
                json.dumps(side, ensure_ascii=False), encoding="utf-8")
            files.append(out / f"map/{key}/terrain.json")
        elif hit:
            missing_scale.append(key)
            (mdir / "terrain.u16.bin").unlink(missing_ok=True)

        # scenery：data/cache/maps/{space}.glb
        if cp(cache_maps / f"{space}.glb", f"map/{key}/scenery.glb"):
            got.append("scenery")

        # groundmeta + groundtex：{space}.ground.layers.json + {space}.ground.{layer}.webp
        if cp(cache_maps / f"{space}.ground.layers.json", f"map/{key}/ground.layers.json"):
            got.append("layers")
        for k in GROUND_LAYERS:
            cp(cache_maps / f"{space}.ground.{k}.webp", f"map/{key}/ground/{k}.webp")

        if got:
            map_n += 1
            files.extend(p for p in mdir.rglob("*") if p.is_file())

    # ---- 包的来源（upstream commit + 生成时的数据版本清单） ----
    provenance = git_provenance()
    data_version = None
    dv_path = data / "data_version.json"
    if dv_path.is_file():
        try:
            data_version = json.loads(dv_path.read_text(encoding="utf-8"))
        except Exception:
            data_version = None

    # ---- index.json（前端一次性装载：数字 id → key） ----
    generated = datetime.now(timezone.utc).isoformat(timespec="seconds")
    index = {
        "version": 1,
        "generated": generated,
        **provenance,
        "maps": {str(e["map_id"]): {"key": seen_spaces.get(e["space"], e["key"]), "display": e["display"]} for e in maps},
    }
    (out / "index.json").write_text(json.dumps(index, ensure_ascii=False, indent=1),
                                    encoding="utf-8")
    files.append(out / "index.json")

    # ---- manifest（sha256，部署前校验包完整性） ----
    # 按路径去重（map 段 cp 与 rglob 会重复登记同一文件）
    uniq: dict[str, dict] = {}
    for f in sorted(files):
        rel = str(f.relative_to(out)).replace("\\", "/")
        uniq.setdefault(rel, {"path": rel, "bytes": f.stat().st_size, "sha256": sha256(f)})
    manifest_files = [uniq[k] for k in sorted(uniq)]
    manifest = {
        "version": 1,
        "generated": generated,
        **provenance,
        "data_version": data_version,
        "files": manifest_files,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1),
                                       encoding="utf-8")

    n_files = len(manifest["files"])
    mb = sum(f["bytes"] for f in manifest["files"]) / 1e6
    print(f"资产包已导出: {out}")
    print(f"  GLB {glb_n} 个文件 / 封面 {img_n} 张 / 核心数据 {core_n} 个 / 地图 {map_n} 张"
          f"（含地形 {with_terrain}）")
    print(f"  共 {n_files} 文件, {mb:.1f}MB（manifest.json 含逐文件 sha256）")
    if missing_scale:
        print(f"  ⚠ 缺地形尺度（sidecar worldBounds），terrain 未打包: {', '.join(missing_scale)}")
    print("  部署：整目录上传到静态主机/对象存储根路径；CORS 允许页面来源 GET/HEAD。")


if __name__ == "__main__":
    main()
