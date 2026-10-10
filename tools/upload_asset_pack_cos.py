"""资产包 → 腾讯 COS 同步上传（一次性工具）。

- 凭据从环境变量 COS_SECRET_ID / COS_SECRET_KEY 读取，不落盘。
- 跳过逻辑：**以桶内 manifest.json 的逐文件 sha256 为准**——内容变了就必须传，
  哪怕尺寸相同（2026-10-10 修正：顶点烘焙只改内容不改尺寸，旧逻辑按 Content-Length
  比对会把这类修正静默跳过，修正永远上不了线）。远端 manifest 缺该条目（首传/旧对象）
  时回退 Content-Length 比对，取不到远端 manifest 则整体回退并告警。
- `--only <prefix>`（可重复）= **局部发布**：只处理匹配前缀的文件；此时 manifest.json
  不上传本机整份，而是拉远端清单**就地补丁**（只更新本次上传的条目）——多会话共用一个
  包目录时，不会把他人在飞的改动一并宣布为线上内容。
- 并发 12 线程；完整发布时 manifest.json 最后单独强传（完整性锚点）。

用法:
  COS_SECRET_ID=... COS_SECRET_KEY=... python tools/upload_asset_pack_cos.py \
      [--local release/asset_pack] [--dry-run]
  # 局部发布（多会话共用一个包目录时）：只传匹配前缀的文件，manifest 就地补丁
  COS_SECRET_ID=... COS_SECRET_KEY=... python tools/upload_asset_pack_cos.py --only glb/
"""
import argparse
import hashlib
import json
import os
import queue
import sys
import threading
import time
from pathlib import Path

from qcloud_cos import CosConfig, CosS3Client

BUCKET = "wotbtools-assets-1478073677"
REGION = "ap-shanghai"


def cache_control_for(key: str) -> str:
    """缓存策略：json 是前端一次装载的元数据/清单（index/manifest/terrain sidecar），
    内容会随包更新变化 → no-cache 强制回源 304 验证；二进制资产（glb/webp/bin/rgba）
    URL 稳定无内容哈希 → max-age=3600 折中更新延迟与重复拉取。COS 缺省不返回
    Cache-Control，浏览器按启发式（寿命 10%）缓存，会把旧资产扣在手里数小时。"""
    if key.endswith(".json"):
        return "no-cache"
    return "max-age=3600"


def local_sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def remote_manifest_raw(client) -> dict:
    """桶内 manifest.json 的解析结果；取不到（首传/损坏/网络）返回 {}。"""
    try:
        body = client.get_object(Bucket=BUCKET, Key="manifest.json")["Body"].get_raw_stream().read()
        man = json.loads(body.decode("utf-8"))
        return man if isinstance(man, dict) else {}
    except Exception as e:  # noqa: BLE001
        print(f"  ⚠ 桶内 manifest 不可用（{type(e).__name__}），跳过判定整体回退 Content-Length", flush=True)
        return {}


def remote_manifest_hashes(client) -> dict:
    """桶内 manifest.json → {path: sha256}（`remote_manifest_raw` 的薄封装）。"""
    man = remote_manifest_raw(client)
    return {f["path"]: f["sha256"] for f in man.get("files", [])
            if isinstance(f, dict) and f.get("path") and f.get("sha256")}


def skip_decision(key: str, local_sha256: str, local_size: int,
                  remote_len: str | None, remote_hash: str | None) -> str:
    """单文件跳过判定 → 'skip' | 'skip-size' | 'upload' | 'upload-same-size'（纯函数，单测看护）。

    优先级：**桶内 manifest 的逐文件 sha256** > `Content-Length` 回退（远端 manifest 缺该
    条目：首传/旧对象）。同尺寸不同内容必须传——顶点烘焙只改内容不改尺寸，旧逻辑（只比
    Content-Length）会把这类修正静默跳过、永远上不了线（2026-10-10 报障链的最后一环）。
    """
    if remote_len is None:
        return "upload"                      # 远端不存在
    if remote_hash is not None:
        if remote_hash == local_sha256:
            return "skip"
        return "upload-same-size" if remote_len == str(local_size) else "upload"
    return "skip-size" if remote_len == str(local_size) else "upload"


def patch_manifest_entries(manifest: dict, updates: dict, note: str) -> dict:
    """**局部发布**：只把 `updates`（{key: (bytes, sha256)}）的条目写进远端清单副本。

    `--only` 模式下用它替代整份上传——整份上传会把本机包内**其他进行中改动**（多会话
    共用一个包目录时）一并宣布为线上内容，而它们并没有被上传。未列入 `updates` 的条目
    原样保留（含既有的合成前 ground 条目等历史状态）；`generated` 更新为本次时刻并附
    `note` 说明范围。纯函数（单测看护）。
    """
    man = json.loads(json.dumps(manifest))          # 深拷贝，不动入参
    files = man.get("files")
    if not isinstance(files, list):
        raise ValueError("远端 manifest 缺 files[]，无法局部补丁")
    out_files = []
    seen = set()
    for f in files:
        key = f.get("path") if isinstance(f, dict) else None
        if key in updates:
            size, digest = updates[key]
            out_files.append({"path": key, "bytes": size, "sha256": digest})
            seen.add(key)
        else:
            out_files.append(f)
    for key, (size, digest) in updates.items():     # 远端清单里没有的新条目也登记
        if key not in seen:
            out_files.append({"path": key, "bytes": size, "sha256": digest})
    man["files"] = out_files
    man["generated"] = datetime_now_utc()
    man["note"] = note
    return man


def datetime_now_utc() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--local", type=Path, default=Path("release/asset_pack"))
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--patch-headers", action="store_true",
                    help="不传字节：对远端已存在对象 copy+Replaced 补 Cache-Control 元数据")
    ap.add_argument("--only", action="append", default=[], metavar="PREFIX",
                    help="局部发布：只处理 key 以该前缀开头的文件（可重复，如 --only glb/）；"
                         "此时 manifest.json 采取**远端清单就地补丁**（只更新被上传文件的条目），"
                         "不上传本机整份 manifest——多会话共用一个包目录时不会把他人在飞的改动"
                         "宣布为线上内容")
    args = ap.parse_args()

    sid = os.environ.get("COS_SECRET_ID")
    skey = os.environ.get("COS_SECRET_KEY")
    if not sid or not skey:
        print("缺少 COS_SECRET_ID / COS_SECRET_KEY 环境变量", file=sys.stderr)
        return 2

    client = CosS3Client(CosConfig(Region=REGION, SecretId=sid, SecretKey=skey, Scheme="https"))

    # `overhead/` = 俯视烘焙的**中间渲染件**（frontend/scripts/bake-ground-overhead.mjs 落进包目录，
    # 36×2 文件 ≈ 2.4 GB），不属于发布内容：manifest 侧已排除，这里也必须排除，否则差分上传会把
    # 中间件推上 COS（2026-10-09 事故"误传 overhead 2.4GB"的复现路径；2026-10-10 实测本扫描曾计入 5074 文件）。
    files = [p for p in args.local.rglob("*") if p.is_file()
             and "overhead" not in p.relative_to(args.local).parts[:-1]]
    if args.only:
        files = [p for p in files
                 if any(p.relative_to(args.local).as_posix().startswith(x) for x in args.only)]
    total_bytes = sum(p.stat().st_size for p in files)
    scope = f"（--only {' '.join(args.only)}）" if args.only else ""
    print(f"本地 {len(files)} 文件 / {total_bytes/1e6:.1f}MB{scope}，开始比对远端…", flush=True)

    remote_manifest = {} if args.patch_headers else remote_manifest_raw(client)
    remote_hashes = {f["path"]: f["sha256"] for f in remote_manifest.get("files", [])
                     if isinstance(f, dict) and f.get("path") and f.get("sha256")} \
                    if remote_manifest else {}
    if args.only and not remote_manifest and not args.patch_headers:
        print("!! --only 需要桶内 manifest 才能做清单补丁，但取不到（见上文告警）", file=sys.stderr)
        return 2
    tasks = []
    skipped = 0
    same_size_changed = 0
    hash_skipped = 0
    manifest_task = None
    t0 = time.time()
    for p in files:
        key = p.relative_to(args.local).as_posix()
        size = p.stat().st_size
        try:
            head = client.head_object(Bucket=BUCKET, Key=key)
            remote_len = head["Content-Length"]
        except Exception:
            head, remote_len = None, None
        if args.patch_headers:
            if head is None:
                skipped += 1  # 远端不存在，无从补头（补头不传字节）
                continue
            tasks.append((p, key, size))
            continue
        if key == "manifest.json":
            manifest_task = (p, key, size)  # 完整性锚点：所有文件传完后再强传
            continue
        if head is not None:
            verdict = skip_decision(key, local_sha256(p), size, remote_len, remote_hashes.get(key))
            if verdict == "skip":
                skipped += 1
                hash_skipped += 1
                continue
            if verdict == "skip-size":
                skipped += 1                    # 远端 manifest 无此条目（旧对象）：回退尺寸比对
                continue
            if verdict == "upload-same-size":
                same_size_changed += 1   # 同尺寸不同内容：旧逻辑会误跳，这里必须上传
        tasks.append((p, key, size))
    up_bytes = sum(t[2] for t in tasks)
    print(f"远端相同跳过 {skipped}（其中按 sha256 跳过 {hash_skipped}），待上传 {len(tasks)} 文件 / "
          f"{up_bytes/1e6:.1f}MB（比对耗时 {time.time()-t0:.0f}s）", flush=True)
    if same_size_changed:
        print(f"  ⚠ 同尺寸不同内容 {same_size_changed} 个——按旧 Content-Length 逻辑本会被静默跳过，"
              f"本次按 sha256 上传", flush=True)
    if args.only:
        print(f"  （--only 模式：manifest.json 走远端清单就地补丁，登记 {len(tasks)} 个待传条目"
              f"{'' if args.dry_run else ''}）", flush=True)

    if args.dry_run:
        return 0

    q = queue.Queue()
    for t in tasks:
        q.put(t)
    done = [0]
    err = []
    lock = threading.Lock()

    def worker():
        while True:
            try:
                p, key, size = q.get_nowait()
            except queue.Empty:
                return
            try:
                if args.patch_headers:
                    client.copy_object(
                        Bucket=BUCKET, Key=key,
                        CopySource={"Bucket": BUCKET, "Key": key, "Region": REGION},
                        CopyStatus="Replaced", StorageClass="STANDARD",
                        CacheControl=cache_control_for(key))
                elif size > 8 * 1024 * 1024:
                    client.upload_file(Bucket=BUCKET, Key=key, LocalFilePath=str(p),
                                       EnableMD5=False, PartSize=8, MAXThread=4,
                                       CacheControl=cache_control_for(key))
                else:
                    with open(p, 'rb') as f:
                        client.put_object(Bucket=BUCKET, Key=key, Body=f,
                                          CacheControl=cache_control_for(key))
                with lock:
                    done[0] += 1
                    if done[0] % 100 == 0 or done[0] == len(tasks):
                        el = time.time() - t0
                        print(f"  {done[0]}/{len(tasks)}（{el:.0f}s）", flush=True)
            except Exception as e:
                with lock:
                    err.append((key, str(e)))
            finally:
                q.task_done()

    threads = [threading.Thread(target=worker, daemon=True) for _ in range(12)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    if manifest_task is not None and not args.only:
        p, key, size = manifest_task
        try:
            with open(p, 'rb') as f:
                client.put_object(Bucket=BUCKET, Key=key, Body=f,
                                  CacheControl=cache_control_for(key))
            print("manifest.json 强传完成（完整性锚点）", flush=True)
        except Exception as e:
            err.append((key, str(e)))
    elif args.only:
        # 局部发布：远端清单就地补丁（只登记本次真正上传的条目）
        uploaded = {k: (s, local_sha256(p)) for (p, k, s) in tasks if k not in {e[0] for e in err}}
        try:
            man = patch_manifest_entries(
                remote_manifest, uploaded,
                note=(f"partial publish 2026-10-10: {len(uploaded)} object(s) re-uploaded by "
                      f"--only {' '.join(args.only)} (shell firing-table decision inputs: "
                      f"caliber/normalization/ricochet + tank-level caliber tier fix); other entries "
                      f"kept as the bucket had them"))
            body = json.dumps(man, ensure_ascii=False, indent=1).encode("utf-8")
            client.put_object(Bucket=BUCKET, Key="manifest.json", Body=body,
                              CacheControl=cache_control_for("manifest.json"))
            print(f"manifest.json 就地补丁完成：登记 {len(uploaded)} 条（其余条目照远端原样）", flush=True)
        except Exception as e:
            err.append(("manifest.json", str(e)))

    print(f"完成：成功 {done[0]}，失败 {len(err)}，跳过 {skipped}", flush=True)
    for key, e in err[:10]:
        print(f"  FAIL {key}: {e}", flush=True)
    return 1 if err else 0


if __name__ == "__main__":
    sys.exit(main())
