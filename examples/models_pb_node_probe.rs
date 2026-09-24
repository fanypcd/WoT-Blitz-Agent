//! 临时探针：models.pb 中某坦克的 炮塔/主炮模块 → 模型节点号 映射
//! 用法：cargo run --release --example models_pb_node_probe -- <tank_id>
// blitzkit.rs 内部引用 crate::data::data_path——示例环境补同名模块
pub mod data {
    pub fn data_path(name: &str) -> std::path::PathBuf { std::path::Path::new("data").join(name) }
}
mod blitzkit_shim {
    #[path = "../../src/wargaming/blitzkit.rs"]
    pub mod blitzkit;
}
fn main() {
    let tank_id: u32 = std::env::args().nth(1).expect("usage: models_pb_node_probe <tank_id>").parse().unwrap();
    let buf = std::fs::read("data/models.pb").expect("data/models.pb");
    let all = blitzkit_shim::blitzkit::parse_models_pb(&buf).expect("parse");
    match all.iter().find(|t| t.tank_id == tank_id) {
        Some(info) => {
            println!("tank {tank_id}: {} 套炮塔", info.turrets.len());
            for t in &info.turrets {
                println!("  turret module={} -> node turret_0{}", t.module_id, t.model_node);
                for g in &t.guns {
                    println!("    gun module={} -> node gun_0{}", g.gun_module_id, g.model_node);
                }
            }
        }
        None => println!("tank {tank_id} 不在 models.pb"),
    }
}
