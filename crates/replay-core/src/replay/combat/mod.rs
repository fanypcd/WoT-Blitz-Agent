//! 回放战斗事件域（原 combat.rs 单文件 ~3800 行拆分；公共路径 `crate::replay::combat::*` 不变）。
//!
//! - [`events`]：CombatTimeline/CombatEvent/HpEvent 事件层（含血量链收集）；
//! - [`pb`]：protobuf wire 解析助手（varint/字段遍历/hex——全库唯一一套）；
//! - [`arena`]：updateArena 流（subtype 过滤收集/PERIOD/击杀播报/AoI 在场/反馈计数/type39）；
//! - [`collect`]：射击路径的包收集器（launch/endpoint/direct8/warning32/地形命中/降幅/tick）；
//! - [`indexes`]：per-entity 索引与 prop2 时间线求值（st10/prop2/二分采样）；
//! - [`anchors`]：判定锚点选择与渲染层滤波时间线（AvatarFilter 输出）；
//! - [`pitch`]：炮管俯仰极限模型与 prop2 frac 解码；
//! - [`nickname`]：Type5 昵称域唯一解码器（UTF-8 全域 SSOT，三消费方共用）；
//! - [`shots`]：ShotReplayData/ShotScanShared 与作者/他人两条提取路径。
//!
//! 拆分为纯搬移（行为与拆分前逐位一致，见 docs/architecture-debt.md 第 1 节）。

mod anchors;
mod arena;
mod collect;
mod events;
mod indexes;
mod nickname;
mod pb;
mod pitch;
mod shots;

pub use anchors::*;
pub use arena::*;
pub use collect::*;
pub use events::*;
pub use indexes::*;
pub use nickname::*;
pub(crate) use pb::*;
pub use pitch::*;
pub use shots::*;

/// serde skip_serializing_if 助手：false 不序列化
fn is_false(b: &bool) -> bool {
    !*b
}
