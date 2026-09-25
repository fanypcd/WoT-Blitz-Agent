//! GLB 模型全量预热（`fetch-models` 命令）：从 data/models.pb 枚举全部有模型的
//! 坦克 id，并发下载 model.glb + collision.glb 到 glb_cache/，完成后查看器/回放
//! 完全离线可用。单文件下载复用 viewer 的按需缓存逻辑（重试 + curl 回退 +
//! glTF magic 校验），本模块只做枚举、并发调度与进度汇总。
//!
//! 幂等可重跑：默认跳过已缓存文件（含断点续传语义——中断后重跑只补缺失项），
//! `--force` 强制全量重下。CDN 确定性 404（该车辆无模型）快速失败并计入失败清单。

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// 每辆坦克需要的模型文件（与 viewer 的 GLB_FILES 保持一致）。
const GLB_FILES: [&str; 2] = ["collision.glb", "model.glb"];

/// 进度打印步长（已完成任务数）。
const PROGRESS_STEP: usize = 25;

/// 失败清单最多记录条数（超出后省略，避免刷屏）。
const MAX_ERROR_LINES: usize = 50;

/// 全量预热全部坦克模型。返回 `(downloaded, cached, failed, downloaded_bytes)`。
pub async fn fetch_all_models(force: bool, concurrency: usize) -> Result<(usize, usize, usize, u64)> {
    let ids = crate::wargaming::blitzkit::load_model_ids();
    anyhow::ensure!(
        !ids.is_empty(),
        "models.pb 为空或缺失 —— 先运行 `fetch-blitzkit` / `update-data` 生成数据源"
    );
    // 单连接实测仅 ~40KB/s（CDN 逐连接限速），靠并发数堆聚合带宽
    let concurrency = concurrency.clamp(1, 64);

    // 任务队列：每辆坦克两个文件，展平后按原子下标分发到并发任务
    let jobs: Vec<(u32, &'static str)> = ids
        .iter()
        .flat_map(|&id| GLB_FILES.iter().map(move |&f| (id, f)))
        .collect();
    let total = jobs.len();

    println!("=== Fetch Models (full offline preload) ===");
    println!("  Tanks: {}  Files: {}  Concurrency: {}  Force: {}",
        ids.len(), total, concurrency, force);

    let downloaded = Arc::new(AtomicUsize::new(0));
    let cached = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let errors: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(Default::default());
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));

    let mut set = tokio::task::JoinSet::new();
    for (tank_id, filename) in jobs {
        let permit = sem.clone().acquire_owned().await
            .context("download semaphore closed")?;
        let (downloaded, cached, failed) =
            (downloaded.clone(), cached.clone(), failed.clone());
        let (total_bytes, done, errors) = (total_bytes.clone(), done.clone(), errors.clone());
        set.spawn(async move {
            let _permit = permit;

            let path = crate::wargaming::viewer::glb_cache_path(tank_id, filename);
            if force && path.exists() {
                let _ = std::fs::remove_file(&path);
            }
            if path.exists() {
                cached.fetch_add(1, Ordering::Relaxed);
            } else {
                match crate::wargaming::viewer::ensure_glb_bytes(tank_id, filename).await {
                    Ok(data) => {
                        downloaded.fetch_add(1, Ordering::Relaxed);
                        total_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                    }
                    Err(e) => {
                        failed.fetch_add(1, Ordering::Relaxed);
                        let mut errs = errors.lock().unwrap();
                        if errs.len() < MAX_ERROR_LINES {
                            errs.push(format!("{}/{}: {}", tank_id, filename, e));
                        } else if errs.len() == MAX_ERROR_LINES {
                            errs.push("...（其余错误省略）".into());
                        }
                    }
                }
            }

            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            if d % PROGRESS_STEP == 0 || d == total {
                eprintln!("  [{}/{}] downloaded={} cached={} failed={}",
                    d, total,
                    downloaded.load(Ordering::Relaxed),
                    cached.load(Ordering::Relaxed),
                    failed.load(Ordering::Relaxed));
            }
        });
    }
    while set.join_next().await.is_some() {}

    let (d, c, f) = (
        downloaded.load(Ordering::Relaxed),
        cached.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed),
    );
    let mb = total_bytes.load(Ordering::Relaxed) as f64 / 1024.0 / 1024.0;
    println!("  Done: downloaded={} ({:.0} MB) cached={} failed={}", d, mb, c, f);
    let errs = errors.lock().unwrap();
    if !errs.is_empty() {
        println!("  Failed items:");
        for e in errs.iter() {
            println!("    {}", e);
        }
        println!("  重跑 `fetch-models` 可只补失败项（已缓存的自动跳过）。");
    }
    Ok((d, c, f, total_bytes.load(Ordering::Relaxed)))
}

#[cfg(test)]
mod tests {
    /// load_model_ids 必须升序且去重；models.pb 缺失（纯净环境）时允许为空。
    #[test]
    fn model_ids_sorted_and_unique() {
        let ids = crate::wargaming::blitzkit::load_model_ids();
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "model ids must be strictly ascending");
    }
}
