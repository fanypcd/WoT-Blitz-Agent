//! wotb-agent 库形态入口：桌面 CLI（`main.rs`）复用全部业务模块。
//!
//! `data::set_base_dir()` 可把 data/（静态库/缓存/会话）等相对路径整体重定向到
//! 宿主应用的私有目录（本仓库 CLI 不调用，供需要私有数据目录的宿主复用）。

pub mod agent;
#[cfg(feature = "bundle")]
pub mod bundle;
pub mod data;
pub mod facets;
pub mod models;
pub mod replay;
pub mod wargaming;
pub mod web;
