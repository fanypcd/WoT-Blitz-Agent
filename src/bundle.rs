// 自包含分发模式（`cargo build --release --features bundle`）：
// data/、web/vendor/、replay_samples/ 与 config.toml.example 在编译期打包进 exe，
// 首次在目标机器运行时自释放到 exe 所在目录（不可写则回退到用户目录），
// 并把工作目录切过去——此后与便携包形态完全一致，
// update-data / Settings 在线改配置 / glb_cache 懒加载下载照常可用。
// 已在带 data/tanks.pb 的目录里运行（如仓库根 cargo run）时不做任何事。

use std::path::{Path, PathBuf};

use rust_embed::{Embed, RustEmbed};

#[derive(RustEmbed)]
#[folder = "data/"]
#[exclude = "sessions/*"]
#[exclude = "sessions/**"]
struct DataAssets;

#[derive(RustEmbed)]
#[folder = "web/vendor/"]
struct VendorAssets;

#[derive(RustEmbed)]
#[folder = "replay_samples/"]
struct ReplaySamples;

const CONFIG_EXAMPLE: &str = include_str!("../config.toml.example");

/// bundle 构建入口：确保运行目录就绪（必要时自释放资源并 chdir）。
pub fn bootstrap() -> anyhow::Result<()> {
    // 已处于数据完整的项目目录（仓库根 / 已释放过的目录）→ 直接使用
    if is_data_ready(Path::new(".")) {
        return Ok(());
    }

    let exe_dir = std::env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let base = match ensure_writable(&exe_dir) {
        Some(dir) => dir,
        None => {
            // exe 目录不可写（如 Program Files）→ 回退用户目录
            let fallback = user_data_dir()?;
            std::fs::create_dir_all(&fallback)?;
            eprintln!(
                "[bundle] exe 目录不可写，资源释放到用户目录: {}",
                fallback.display()
            );
            fallback
        }
    };

    if !is_data_ready(&base) {
        eprintln!("[bundle] 首次运行，释放内置数据到 {} ...", base.display());
        extract(&base)?;
        eprintln!("[bundle] 释放完成。之后可用 update-data 刷新数据，删除 data/ 可重置。");
    }
    ensure_config(&base)?;

    std::env::set_current_dir(&base)?;
    eprintln!("[bundle] 工作目录: {}", base.display());
    Ok(())
}

/// 目录是否已有可直接使用的数据（以 tanks.pb 为准，避免覆盖用户已刷新的数据）。
fn is_data_ready(dir: &Path) -> bool {
    dir.join("data/tanks.pb").is_file()
}

fn extract(base: &Path) -> anyhow::Result<()> {
    extract_assets::<DataAssets>(base, "data")?;
    extract_assets::<VendorAssets>(base, Path::new("web").join("vendor").to_str().unwrap())?;
    extract_assets::<ReplaySamples>(base, "replay_samples")?;
    Ok(())
}

fn extract_assets<A: Embed>(base: &Path, subdir: &str) -> anyhow::Result<()> {
    for rel in A::iter() {
        let Some(file) = A::get(&rel) else { continue };
        let target = base.join(subdir).join(&*rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, file.data.as_ref())?;
    }
    Ok(())
}

/// config.toml 缺失时用内置模板落一份（含可用的公开 WG key，LLM key 需用户自填）。
fn ensure_config(base: &Path) -> anyhow::Result<()> {
    let cfg = base.join("config.toml");
    if !cfg.exists() {
        std::fs::write(&cfg, CONFIG_EXAMPLE)?;
        eprintln!("[bundle] 已生成默认 config.toml（LLM key 请在 Settings 页或文件中填写）");
    }
    Ok(())
}

/// 目录可写探测：尝试创建并删除一个临时文件。
fn ensure_writable(dir: &Path) -> Option<PathBuf> {
    let probe = dir.join(format!(".wotb-write-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Some(dir.to_path_buf())
        }
        Err(_) => None,
    }
}

/// 用户级回退目录：%APPDATA%/wotb-agent（Windows）或 ~/.wotb-agent（其他平台）。
fn user_data_dir() -> anyhow::Result<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return Ok(Path::new(&appdata).join("wotb-agent"));
    }
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("无法确定用户目录"))?;
    Ok(Path::new(&home).join(".wotb-agent"))
}
