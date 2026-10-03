// 回放解析层：单场解析（parser）、批量扫描（scanner）、战斗事件解码与射击推断（combat）、
// 客户端位置滤波器移植（filter，渲染层锚点，逆向文档 §七）、玩家开局配置与弹种映射（loadout）、
// 全场实时回放时间线（playback，0.1s 网格位姿/炮塔/弹道/血量）、
// 内部领域模型（model，一次包扫描的权威产物，切面从这里投影）。
pub mod combat;
pub mod filter;
pub mod model;
pub mod packets;
pub mod parser;
pub mod playback;
pub mod scanner;

/// 坦克名解析的最小接口。核心库不绑定实现：服务端 `TankResolver`（读 tank_cache.json）
/// 实现它，未来 WASM 侧可由打包数据实现；parser/scanner 以 `dyn` 持有，
/// 调用点经泛型 `with_resolver::<R>` 保持零改动。
pub trait TankNames {
    fn resolve(&self, tank_id: u32) -> Option<String>;
}
