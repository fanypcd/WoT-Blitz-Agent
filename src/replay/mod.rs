// 回放解析层已上移至核心库 `wotb-replay-core`（crates/replay-core，架构契约第 4 节的
// Rust Core 载体：零网络依赖，可编译原生与 wasm32 双目标）。本模块是兼容垫片：
// 存量调用点（`crate::replay::combat::…`）零改动；新代码建议直接引 `wotb_replay_core`。
pub use wotb_replay_core::replay::{combat, filter, model, packets, parser, playback, scanner};

// 玩家开局配置与弹种映射（loadout）：依赖服务端 BlitzKit 坦克表静态缓存（crate::data IO），
// 属服务端增值标注而非解析核心，留在本 crate。
pub mod loadout;

/// `Replay::read_data()` 产物 → `(type, clock, payload)` 元组流的统一映射（各管线消费方共用）。
/// `base_player_create_type`：type0 包的口径——loadout 管线传 5（Type5 载荷语义），其余管线传 0。
///
/// 与 [`packets::read_raw_packets`]（自原始字节分帧）互补：本函数吃 crate 的反序列化产物，
/// 个别真实回放的 pickle 形状偏差会在 `read_data()` 连坐整场（见 packets.rs 头注），
/// 调用方按各自容错需求选路。
pub fn raw_packet_tuples(
    data: &wotbreplay_parser::models::data::Data,
    base_player_create_type: u32,
) -> Vec<(u32, f32, &[u8])> {
    data.packets
        .iter()
        .map(|pkt| {
            let t = match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => {
                    base_player_create_type
                }
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => {
                    *packet_type
                }
            };
            (t, pkt.clock_secs, &pkt.raw_payload[..])
        })
        .collect()
}
