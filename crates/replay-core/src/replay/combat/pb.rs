//! protobuf wire 解析助手：varint 读取与字段遍历（find_field/field_value 族）。
//! 全库唯一实现（溢出/截断策略统一，见 docs/architecture-debt.md）。

/// protobuf 最小遍历：varint / fixed64 / 定长子消息，返回 (field_no, wire_type, 内容偏移, 内容长)；仅用于 method36 快照，格式不合法返回 None（fail-soft 调用方忽略）。
pub(crate) fn proto_fields(b: &[u8]) -> Option<Vec<(u32, u8, usize, usize)>> {
    fn varint(b: &[u8], mut o: usize) -> Option<(u64, usize)> {
        let mut v = 0u64;
        let mut s = 0u32;
        loop {
            let x = *b.get(o)?;
            o += 1;
            v |= ((x & 0x7f) as u64) << s;
            if x & 0x80 == 0 {
                return Some((v, o));
            }
            s += 7;
            if s > 63 {
                return None;
            }
        }
    }
    let mut out = Vec::new();
    let mut o = 0usize;
    while o < b.len() {
        let (tag, o2) = varint(b, o)?;
        let (no, wt) = ((tag >> 3) as u32, (tag & 7) as u8);
        let (start, len) = match wt {
            0 => {
                let (_, o3) = varint(b, o2)?;
                (o2, o3 - o2)
            }
            1 => (o2, 8),
            5 => (o2, 4),
            2 => {
                let (l, o3) = varint(b, o2)?;
                (o3, l as usize)
            }
            _ => return None,
        };
        let end = start.checked_add(len)?;
        if end > b.len() {
            return None;
        }
        out.push((no, wt, start, len));
        o = end;
    }
    Some(out)
}

/// protobuf 最小 varint 读取（u64 实体 ID 安全），返回 None = 流不合法。
pub(crate) fn pb_varint(b: &[u8], o: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut s = 0u32;
    loop {
        let x = *b.get(*o)?;
        *o += 1;
        v |= ((x & 0x7f) as u64) << s;
        if x & 0x80 == 0 {
            return Some(v);
        }
        s += 7;
        if s > 63 {
            return None;
        }
    }
}

/// protobuf 最小读取助手（wire 解析；仅用于 ArenaPeriod，格式不合法返回 None）
pub(crate) fn find_field(b: &[u8], want: u32) -> Option<&[u8]> {
    let mut i = 0usize;
    while i < b.len() {
        let tag = b[i];
        i += 1;
        let field = (tag >> 3) as u32;
        let wire = tag & 7;
        match wire {
            0 => {
                while i < b.len() && b[i] & 0x80 != 0 {
                    i += 1;
                }
                i += 1;
            }
            1 => i += 8,
            2 => {
                let mut len = 0usize;
                let mut shift = 0u32;
                while i < b.len() {
                    len |= ((b[i] & 0x7f) as usize) << shift;
                    shift += 7;
                    let cont = b[i] & 0x80 != 0;
                    i += 1;
                    if !cont {
                        break;
                    }
                }
                if i + len > b.len() {
                    return None;
                }
                if field == want {
                    return Some(&b[i..i + len]);
                }
                i += len;
            }
            5 => i += 4,
            _ => return None,
        }
    }
    None
}

fn field_value(b: &[u8], want: u32) -> Option<(&[u8], u8)> {
    let mut i = 0usize;
    while i < b.len() {
        let tag = b[i];
        i += 1;
        let field = (tag >> 3) as u32;
        let wire = tag & 7;
        match wire {
            0 => {
                let start = i;
                while i < b.len() && b[i] & 0x80 != 0 {
                    i += 1;
                }
                i += 1;
                if field == want && i <= b.len() {
                    return Some((&b[start..i], 0));
                }
            }
            1 => {
                if i + 8 > b.len() {
                    return None;
                } else if field == want {
                    return Some((&b[i..i + 8], 1));
                } else {
                    i += 8;
                }
            }
            2 => {
                let mut len = 0usize;
                let mut shift = 0u32;
                while i < b.len() {
                    len |= ((b[i] & 0x7f) as usize) << shift;
                    shift += 7;
                    let cont = b[i] & 0x80 != 0;
                    i += 1;
                    if !cont {
                        break;
                    }
                }
                if i + len > b.len() {
                    return None;
                }
                if field == want {
                    return Some((&b[i..i + len], 2));
                }
                i += len;
            }
            5 => {
                if i + 4 > b.len() {
                    return None;
                } else if field == want {
                    return Some((&b[i..i + 4], 1));
                } else {
                    i += 4;
                }
            }
            _ => return None,
        }
    }
    None
}

pub(crate) fn read_varint(b: &[u8], field: u32) -> Option<u64> {
    let (v, wire) = field_value(b, field)?;
    if wire != 0 {
        return None;
    }
    let mut val = 0u64;
    let mut shift = 0u32;
    for &x in v {
        val |= ((x & 0x7f) as u64) << shift;
        shift += 7;
        if x & 0x80 == 0 {
            return Some(val);
        }
    }
    None
}

pub(crate) fn read_fixed64(b: &[u8], field: u32) -> Option<f64> {
    let (v, wire) = field_value(b, field)?;
    if wire != 1 || v.len() != 8 {
        return None;
    }
    Some(f64::from_le_bytes(v.try_into().ok()?))
}

pub(crate) fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

// **method8 元素后 6 字节（hash6/"命中令牌"）的结构**：
// [shell u16][来向 yaw u16][抵达 pitch u16]——结构化弹道数据，见
// decoded_target_gun_pitch（yaw/pitch 的 (u16−32768)/32768 比例尺解码 + 来向方位角
// ≤15° 校验 + |pitch|≤30° 有效域，校验通过者入 target_gun_pitch）。
