"""资产包 → 腾讯 COS 同步上传（一次性工具）。

- 凭据从环境变量 COS_SECRET_ID / COS_SECRET_KEY 读取，不落盘。
- 跳过逻辑：远端已存在且 Content-Length 相同 → 跳过；否则上传（大文件分块）。
- 并发 12 线程；manifest.json 最后单独强传（完整性锚点）。

用法:
  COS_SECRET_ID=... COS_SECRET_KEY=... python tools/upload_asset_pack_cos.py \
      [--local release/asset_pack] [--dry-run]
"""
import argparse
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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--local", type=Path, default=Path("release/asset_pack"))
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--patch-headers", action="store_true",
                    help="不传字节：对远端已存在对象 copy+Replaced 补 Cache-Control 元数据")
    args = ap.parse_args()

    sid = os.environ.get("COS_SECRET_ID")
    skey = os.environ.get("COS_SECRET_KEY")
    if not sid or not skey:
        print("缺少 COS_SECRET_ID / COS_SECRET_KEY 环境变量", file=sys.stderr)
        return 2

    client = CosS3Client(CosConfig(Region=REGION, SecretId=sid, SecretKey=skey, Scheme="https"))

    files = [p for p in args.local.rglob("*") if p.is_file()]
    total_bytes = sum(p.stat().st_size for p in files)
    print(f"本地 {len(files)} 文件 / {total_bytes/1e6:.1f}MB，开始比对远端…", flush=True)

    tasks = []
    skipped = 0
    manifest_task = None
    t0 = time.time()
    for p in files:
        key = p.relative_to(args.local).as_posix()
        size = p.stat().st_size
        try:
            head = client.head_object(Bucket=BUCKET, Key=key)
            remote_ok = head["Content-Length"] == str(size)
        except Exception:
            head = None
            remote_ok = False
        if args.patch_headers:
            if head is None:
                skipped += 1  # 远端不存在，无从补头（补头不传字节）
                continue
            tasks.append((p, key, size))
            continue
        if key == "manifest.json":
            manifest_task = (p, key, size)  # 完整性锚点：所有文件传完后再强传
            continue
        if head is not None and remote_ok:
            skipped += 1
            continue
        tasks.append((p, key, size))
    up_bytes = sum(t[2] for t in tasks)
    print(f"远端相同跳过 {skipped}，待上传 {len(tasks)} 文件 / {up_bytes/1e6:.1f}MB（比对耗时 {time.time()-t0:.0f}s）", flush=True)

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

    if manifest_task is not None:
        p, key, size = manifest_task
        try:
            with open(p, 'rb') as f:
                client.put_object(Bucket=BUCKET, Key=key, Body=f,
                                  CacheControl=cache_control_for(key))
            print("manifest.json 强传完成（完整性锚点）", flush=True)
        except Exception as e:
            err.append((key, str(e)))

    print(f"完成：成功 {done[0]}，失败 {len(err)}，跳过 {skipped}", flush=True)
    for key, e in err[:10]:
        print(f"  FAIL {key}: {e}", flush=True)
    return 1 if err else 0


if __name__ == "__main__":
    sys.exit(main())
