//! 临时探针：直接调用 dvpl 的 numbered bbox 扫描
//! 用法：cargo run --release --example yaml_collision_dump -- <file.yaml.dvpl> [prefix]
mod dvpl_shim {
    #[path = "../../src/wargaming/dvpl.rs"]
    pub mod dvpl;
}
fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap();
    let prefix = std::env::args().nth(2).unwrap_or_else(|| "turret_".into());
    let d = dvpl_shim::dvpl::DvplFile::read(std::path::Path::new(&path))?;
    let text = String::from_utf8_lossy(&d.data);
    let collision_text = &text[text.find("collision:").unwrap_or(0)..];
    let r = dvpl_shim::dvpl::parse_numbered_section_bboxes(collision_text, &prefix);
    println!("prefix={prefix} -> {} 段: {:?}", r.len(), r.iter().map(|n| n.node).collect::<Vec<_>>());
    if let Some(first) = r.first() {
        println!("首个 bbox: min={:?} max={:?}", first.bbox.min, first.bbox.max);
    }
    Ok(())
}
