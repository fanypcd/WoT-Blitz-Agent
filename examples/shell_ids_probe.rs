//! tanks.pb ShellData.field1 低字节形式核查：field1 是否恒为 (局部 id << 8) | 国家相关低字节。
//! 用法：cargo run --release --example shell_ids_probe
mod data {
    use std::path::PathBuf;
    pub fn data_path(name: &str) -> PathBuf { PathBuf::from("data").join(name) }
}
mod replay_shim {
    #[path = "../../src/wargaming/blitzkit.rs"]
    pub mod blitzkit;
}
use replay_shim::blitzkit;

use std::collections::BTreeMap;

fn main() {
    let tanks = blitzkit::load_tanks();
    let mut by_nation: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    for t in tanks.values() {
        let Some(top) = t.turrets.last() else { continue };
        let Some(gun) = top.guns.last() else { continue };
        let entry = by_nation.entry(t.nation.clone()).or_default();
        if entry.len() >= 8 { continue; }
        for s in &gun.shells {
            if s.id == 0 { continue; }
            entry.push((t.dev_name.clone(), s.id));
        }
    }
    for (n, v) in &by_nation {
        for (name, id) in v.iter().take(4) {
            println!("{:10} {:22} field1={:#010x} low={:#04x} local={:#08x}", n, name, id, id & 0xff, id >> 8);
        }
    }
}
