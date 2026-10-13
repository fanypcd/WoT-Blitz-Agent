// data.wotreplay 原始包分帧：只切出 (type, clock, payload)，**不反序列化任何 payload**。
//
// 背景：crate `wotbreplay-parser` 的 `Replay::read_data()` 会把 type 0（BasePlayerCreate）等包的
// pickle payload 严格反序列化成结构体；个别真实回放里某个本应是 bool 的字段是整数 0
// （"invalid type: integer `0`, expected a boolean"），整场回放因此读不出包流——而本库的
// 解码器只需要原始 payload，从来不用 crate 的反序列化结果。这里自行分帧，单个 payload 的
// 形状偏差不再连坐整场。
//
// 线格式（与 `parser::read_client_version` 的头部注释同源）：
//   头部  magic u32(0x12345678) + u64 + [len u8 + client hash] + [len u8 + version] + u8
//   包体  连续排列 [payload_len u32][type u32][clock f32][payload]，全部小端；
//         type == 0xFFFF_FFFF 为流终止包（自身计入，之后不再读取）。

use std::io::Read;

use anyhow::{bail, Context, Result};

/// 流终止包类型
pub const TERMINATOR_TYPE: u32 = 0xFFFF_FFFF;
const MAGIC: u32 = 0x1234_5678;
const PACKET_HEADER_LEN: usize = 12;

/// 一个原始包：类型、回放时钟（秒）与未解码的 payload
#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub packet_type: u32,
    pub clock_secs: f32,
    pub payload: Vec<u8>,
}

/// 从 `.wotbreplay`（ZIP）原始字节读出 data.wotreplay 并分帧。
pub fn read_raw_packets(replay_zip: &[u8]) -> Result<Vec<RawPacket>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(replay_zip))
        .context("replay is not a zip archive")?;
    let mut entry = zip
        .by_name("data.wotreplay")
        .context("data.wotreplay missing")?;
    let mut data = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut data)
        .context("failed to read data.wotreplay")?;
    frame_packets(&data)
}

/// 对 data.wotreplay 字节分帧（头部校验 + 连续包；截断即报错，不做逐字节重同步）。
pub fn frame_packets(data: &[u8]) -> Result<Vec<RawPacket>> {
    let mut offset = packet_stream_offset(data)?;
    let mut packets = Vec::new();
    while offset + PACKET_HEADER_LEN <= data.len() {
        let payload_len = u32_le(data, offset) as usize;
        let packet_type = u32_le(data, offset + 4);
        let clock_secs = f32::from_bits(u32_le(data, offset + 8));
        let start = offset + PACKET_HEADER_LEN;
        let end = start.checked_add(payload_len).filter(|&e| e <= data.len());
        let Some(end) = end else {
            bail!("truncated packet at offset {offset} (payload_len {payload_len})");
        };
        packets.push(RawPacket {
            packet_type,
            clock_secs,
            payload: data[start..end].to_vec(),
        });
        offset = end;
        if packet_type == TERMINATOR_TYPE {
            break;
        }
    }
    Ok(packets)
}

/// 测试助手：把 crate `Replay::read_data()` 的反序列化产物映射为 `(type, clock, payload)`
/// 元组流（映射式与本库管线一致；仅 ignored 探针测试用——管线一律走 [`read_raw_packets`]，
/// 不吃 crate 反序列化，见本文件头注的连坐说明）。
#[cfg(test)]
pub(crate) fn test_data_tuples(data: &wotbreplay_parser::models::data::Data) -> Vec<(u32, f32, &[u8])> {
    data.packets
        .iter()
        .map(|pkt| {
            let t = match &pkt.payload {
                wotbreplay_parser::models::data::payload::Payload::BasePlayerCreate { .. } => 0,
                wotbreplay_parser::models::data::payload::Payload::EntityMethod(_) => 8,
                wotbreplay_parser::models::data::payload::Payload::Unknown { packet_type } => {
                    *packet_type
                }
            };
            (t, pkt.clock_secs, &pkt.raw_payload[..])
        })
        .collect()
}

fn packet_stream_offset(b: &[u8]) -> Result<usize> {
    if b.len() < 14 || u32_le(b, 0) != MAGIC {
        bail!("data.wotreplay header: bad magic");
    }
    // 客户端读路径对 +4 的 restLen 有 0x1E00000（30MB）上限校验（ReplayFile 头解析，
    // wotblitz.exe 0x13c6311/0x13c6395）；录制期写 0 占位、收尾回填。超限 = 非本项目
    // 支持的形态，fail-fast（现语料最大单场 ~3.4MB，距上限两个数量级）。
    // 另：客户端魔数还接受 0x12343210（旧格式、无头块、无版本串）——本项目不支持，
    // 按坏魔数拒绝是**有意行为**而非数据损坏。
    let rest_len = u32_le(b, 4);
    if rest_len > 0x01E0_0000 {
        bail!(
            "data.wotreplay header: restLen {} exceeds client cap (30MB)",
            rest_len
        );
    }
    let mut o = 12usize; // magic(4) + u64(8)
    let hash_len = *b.get(o).context("data.wotreplay header truncated")? as usize;
    o += 1 + hash_len;
    let version_len = *b.get(o).context("data.wotreplay header truncated")? as usize;
    o += 1 + version_len + 1; // version + 1 字节填充
    if o > b.len() {
        bail!("data.wotreplay header truncated");
    }
    Ok(o)
}

fn u32_le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Vec<u8> {
        let mut h = MAGIC.to_le_bytes().to_vec();
        h.extend_from_slice(&[0u8; 8]);
        h.push(2);
        h.extend_from_slice(b"ab");
        h.push(7);
        h.extend_from_slice(b"11.20.0");
        h.push(0);
        h
    }

    fn packet(out: &mut Vec<u8>, packet_type: u32, clock: f32, payload: &[u8]) {
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&packet_type.to_le_bytes());
        out.extend_from_slice(&clock.to_le_bytes());
        out.extend_from_slice(payload);
    }

    #[test]
    fn frames_packets_without_decoding_payloads_and_stops_at_terminator() {
        let mut data = header();
        // type 0 payload 是任意字节：分帧不关心它是不是合法 pickle
        packet(&mut data, 0, 0.0, &[0xde, 0xad]);
        packet(&mut data, 8, 1.5, &[]);
        packet(&mut data, TERMINATOR_TYPE, 0.0, &[0u8; 16]);
        data.extend_from_slice(&[1, 2, 3]); // 终止包之后的残留不得被读成包

        let packets = frame_packets(&data).unwrap();
        assert_eq!(packets.len(), 3);
        assert_eq!(
            packets[0],
            RawPacket {
                packet_type: 0,
                clock_secs: 0.0,
                payload: vec![0xde, 0xad]
            }
        );
        assert_eq!(packets[1].packet_type, 8);
        assert_eq!(packets[1].clock_secs, 1.5);
        assert!(packets[1].payload.is_empty());
        assert_eq!(packets[2].packet_type, TERMINATOR_TYPE);
    }

    #[test]
    fn truncated_packet_and_bad_magic_are_errors() {
        let mut data = header();
        packet(&mut data, 8, 1.0, &[1, 2, 3, 4]);
        data.truncate(data.len() - 2);
        assert!(frame_packets(&data).is_err());
        assert!(frame_packets(&[0u8; 32]).is_err());
    }

    /// restLen 超客户端 30MB 上限（0x1E00000）→ fail-fast（与 wotblitz.exe 读路径同界限）。
    #[test]
    fn rest_len_over_client_cap_is_rejected() {
        let mut data = header();
        data[4..8].copy_from_slice(&0x0200_0000u32.to_le_bytes()); // 32MB > 30MB
        let err = frame_packets(&data).unwrap_err().to_string();
        assert!(err.contains("exceeds client cap"), "err={err}");
    }
}
