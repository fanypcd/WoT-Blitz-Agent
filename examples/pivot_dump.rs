//! models.pb 枢轴链转储：track_origin / turret_origin（DAVA 原始序 + GLB correctZY）
mod data {
    pub fn data_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new("data").join(name)
    }
}
mod wargaming_shim {
    #[path = "../../src/wargaming/blitzkit.rs"]
    pub mod blitzkit;
}
fn main() {
    let tid: u32 = std::env::args().nth(1).unwrap().parse().unwrap();
    match wargaming_shim::blitzkit::model_info(tid) {
        Some(m) => {
            println!("tank {}", tid);
            println!("track_origin (DAVA): {:?}", m.track_origin);
            println!("turret_origin (DAVA): {:?}", m.turret_origin);
            println!("GLB correctZY (x,z,y): track={:?} turret={:?}",
                m.track_origin.map(|t| [t[0], t[2], t[1]]),
                m.turret_origin.map(|t| [t[0], t[2], t[1]]));
            println!("initial_turret_rotation: {:?}", m.initial_turret_rotation);
        }
        None => println!("tank {} not found in models.pb", tid),
    }
}
