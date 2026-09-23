use anyhow::Result;
use serde::{Deserialize, Serialize};

// 数据版本清单（data/data_version.json）：记录各派生数据文件最后一次更新时对应的
// 游戏版本与时间戳，供 `update-data` 判断游戏是否更新过（决定全量重提取还是增量补缺）。

/// data/data_version.json 的结构（清单缺失/损坏时 load() 返回默认值，版本号为空串）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataVersionManifest {
    /// 上次更新时本机游戏的版本号（version.txt.dvpl 中的 release/X.Y.Z 段）。
    #[serde(default)]
    pub game_version: String,
    /// 上次下载 BlitzKit pb 的时间（RFC3339）。
    #[serde(default)]
    pub blitzkit_updated_at: Option<String>,
    /// 上次重建 tank_cache.json 的时间。
    #[serde(default)]
    pub tank_cache_updated_at: Option<String>,
    /// 上次提取 game_data 的时间。
    #[serde(default)]
    pub game_data_updated_at: Option<String>,
    /// 上次 BlitzKit 坦克总数。
    #[serde(default)]
    pub tank_count: Option<usize>,
    /// 上次 game_data 覆盖的文件数（新提取 + 已缓存）。
    #[serde(default)]
    pub game_data_files: Option<usize>,
}

impl DataVersionManifest {
    /// 读取清单；文件不存在或解析失败时返回默认值（game_version 为空串表示从未记录）。
    pub fn load() -> Self {
        let path = crate::data::data_path("data_version.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 是否记录过任何更新（game_version 非空）。
    pub fn has_version(&self) -> bool {
        !self.game_version.is_empty()
    }

    pub fn save(&self) -> Result<()> {
        let path = crate::data::data_path("data_version.json");
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

/// 当前 UTC 时间的 RFC3339 字符串（清单时间戳用）。
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
