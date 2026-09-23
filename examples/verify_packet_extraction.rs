//! 包提取完整性校验：独立 std 解析器直接走 data.wotreplay 包流（u32 len / u32 type / f32 clock / len B 载荷），
//! 与 crate `read_data()` 的结果逐包比对（数量 / wire 类型 / clock 位 / 原始载荷字节），
//! 并确认独立解析恰好消费到文件末尾（无残留、无截断）。
//! 用法：
//!   cargo run --release --example verify_packet_extraction -- <a.wotbreplay> <data.wotreplay 裸流>
//!   （裸流可用 `unzip -p a.wotbreplay data.wotreplay > data.wotreplay` 得到）
//!   只传 .wotbreplay 时仅输出 crate 侧包数与类型直方图；只传 .wotreplay 裸流时仅做独立解析。
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut replay_path: Option<std::path::PathBuf> = None;
    let mut raw_path: Option<std::path::PathBuf> = None;
    for a in &args {
        let p = std::path::PathBuf::from(a);
        match p.extension().and_then(|x| x.to_str()) {
            Some("wotbreplay") => replay_path = Some(p),
            Some("wotreplay") => raw_path = Some(p),
            _ => { eprintln!("unrecognized arg: {}", a); std::process::exit(2); }
        }
    }
    if replay_path.is_none() && raw_path.is_none() {
        eprintln!("usage: verify_packet_extraction <a.wotbreplay> [data.wotreplay]");
        std::process::exit(2);
    }

    let mut ok = true;

    // —— crate 路径：与项目内 5 个调用点（main/viewer/web/filter 等）完全一致的映射 ——
    let crate_packets: Vec<(u32, u32, Vec<u8>)> = replay_path.map(|path| {
        println!("=== {} ===", path.display());
        let f = std::fs::File::open(&path).unwrap_or_else(|e| panic!("open: {}", e));
        let mut replay = wotbreplay_parser::replay::Replay::open(f).unwrap_or_else(|e| panic!("Replay::open: {}", e));
        let data = replay.read_data().unwrap_or_else(|e| panic!("read_data: {}", e));
        let v: Vec<(u32, u32, Vec<u8>)> = data.packets.iter().map(|pkt| {
            let t = match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => *packet_type,
            };
            (t, pkt.clock_secs.to_bits(), pkt.raw_payload.clone())
        }).collect();
        println!("  client_version: {}", data.client_version);
        println!("  crate packets: {}", v.len());
        v
    }).unwrap_or_default();

    // —— 独立路径：纯 std 走裸流 ——
    if let Some(path) = raw_path {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
        println!("=== {} (independent std walk) ===", path.display());
        println!("  file bytes: {}", bytes.len());
        let (packets, hist, leftover) = walk_stream(&bytes);
        println!("  independent packets: {}", packets.len());
        println!("  type histogram: {:?}", hist);
        println!("  leftover bytes after last packet: {}", leftover);
        if leftover != 0 { ok = false; }

        // —— 双路径比对：数量 / 直方图 / 逐包（类型、clock 位、载荷字节）——
        if !crate_packets.is_empty() {
            let crate_hist: std::collections::BTreeMap<u32, usize> = crate_packets.iter()
                .fold(std::collections::BTreeMap::new(), |mut m, (t, _, _)| { *m.entry(*t).or_insert(0) += 1; m });
            println!("  crate histogram: {:?}", crate_hist);
            if packets.len() != crate_packets.len() || hist != crate_hist {
                println!("  [FAIL] count or histogram mismatch: independent={} crate={}", packets.len(), crate_packets.len());
                ok = false;
            } else {
                for (i, (ty, clock_bits, payload)) in packets.iter().enumerate() {
                    let (ct, cclock, cpayload) = &crate_packets[i];
                    if *ty != *ct || *clock_bits != *cclock || payload != cpayload {
                        println!("  [FAIL] packet #{} mismatch: wire(type={},clock_bits={:#x},len={}) vs crate(type={},clock_bits={:#x},len={})",
                            i, ty, clock_bits, payload.len(), ct, cclock, cpayload.len());
                        ok = false;
                        break;
                    }
                }
                if ok {
                    println!("  per-packet check: {} packets (type / clock bits / payload bytes) all identical", packets.len());
                }
            }
        }
    }

    if ok { println!("\nALL OK"); } else { std::process::exit(1); }
}

/// 独立解析 data.wotreplay 包流。返回 (逐包(type,clock位,载荷), 类型直方图, 末尾残留字节数)。
/// 任何一包长度越界即 panic（流损坏或格式假设错误时宁可失败，不静默少包）。
fn walk_stream(bytes: &[u8]) -> (Vec<(u32, u32, Vec<u8>)>, std::collections::BTreeMap<u32, usize>, usize) {
    let mut off = header_len(bytes);
    let mut hist = std::collections::BTreeMap::new();
    let mut packets = Vec::new();
    while off < bytes.len() {
        let remain = bytes.len() - off;
        assert!(remain >= 12, "truncated packet header at offset {} ({} bytes left)", off, remain);
        let len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let ty = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
        let clock = f32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap());
        assert!(off + 12 + len <= bytes.len(), "packet #{} (type={}, len={}) overruns file at offset {}", packets.len(), ty, len, off);
        let payload = bytes[off + 12..off + 12 + len].to_vec();
        off += 12 + len;
        *hist.entry(ty).or_insert(0) += 1;
        packets.push((ty, clock.to_bits(), payload));
    }
    (packets.clone(), hist, 0)
}

/// 跳过文件头：magic u32 + u64 + 1B 长度前缀哈希 + 1B 长度前缀版本串 + 1B
fn header_len(bytes: &[u8]) -> usize {
    let mut skip = 4 + 8;
    let hash_len = bytes[skip] as usize;
    skip += 1 + hash_len;
    let ver_len = bytes[skip] as usize;
    skip += 1 + ver_len + 1;
    skip
}
