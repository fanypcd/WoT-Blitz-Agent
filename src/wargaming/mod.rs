// Wargaming / BlitzKit / 游戏文件 集成层：WG API 客户端、坦克解析器、
// BlitzKit pb 解析、快照、3D 查看器、DVPL 解码、击穿判定、对局前瞻、数据提取。
pub mod api_client;
pub mod blitzkit;
pub mod data_version;
pub mod dvpl;
pub mod game_extract;
pub mod heatmap_ready;
pub mod map_assets;
pub mod model_fetch;
pub mod penetration;
pub mod playback_viewer;
pub mod prematch;
pub mod snapshot;
pub mod tank_configs;
pub mod tank_resolver;
pub mod viewer;
pub use wotb_replay_core::wargaming::battle_results_extra;
