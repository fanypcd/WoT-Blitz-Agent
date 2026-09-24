//! 临时探针：tanks.pb T110E5 条目未消费字段枚举（找碰撞盒数据）
//! 用法：cargo run --release --example tanks_pb_bbox_probe -- <tank_id>
use std::fs;

struct Reader<'a> { buf: &'a [u8], pos: usize }
impl<'a> Reader<'a> {
    fn tag(&mut self) -> Option<anyhow::Result<(u32, u32)>> {
        if self.pos >= self.buf.len() { return None; }
        Some(self.read_tag_inner())
    }
    fn read_tag_inner(&mut self) -> anyhow::Result<(u32, u32)> {
        let key = self.varint()?;
        Ok(((key >> 3) as u32, (key & 7) as u32))
    }
    fn varint(&mut self) -> anyhow::Result<u64> {
        let mut v = 0u64; let mut s = 0;
        loop {
            let b = self.buf[self.pos]; self.pos += 1;
            v |= ((b & 0x7f) as u64) << s;
            if b < 0x80 { return Ok(v); }
            s += 7; if s > 63 { anyhow::bail!("varint overflow"); }
        }
    }
    fn bytes(&mut self, l: usize) -> anyhow::Result<&'a [u8]> {
        let b = &self.buf[self.pos..self.pos + l]; self.pos += l; Ok(b)
    }
    fn skip_field(&mut self, w: u32) -> anyhow::Result<()> {
        match w {
            0 => { let _ = self.varint()?; },
            1 => { let _ = self.bytes(8)?; },
            2 => { let l = self.varint()? as usize; let _ = self.bytes(l)?; },
            5 => { let _ = self.bytes(4)?; },
            _ => anyhow::bail!("wire {}", w),
        }
        Ok(())
    }
}

fn floats(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn dump_fields(label: &str, buf: &[u8], depth: usize) {
    let pad = "  ".repeat(depth);
    let mut r = Reader { buf, pos: 0 };
    while let Some(Ok((f, w))) = r.tag() {
        match w {
            0 => { let v = r.varint().unwrap(); println!("{pad}{label} f{f} varint={v}"); },
            1 => { let b = r.bytes(8).unwrap(); println!("{pad}{label} f{f} f64={:.3} bytes={:02x?}", f64::from_le_bytes(b.try_into().unwrap()), &b[..]); },
            5 => { let b = r.bytes(4).unwrap(); let fl = f32::from_le_bytes(b.try_into().unwrap()); println!("{pad}{label} f{f} f32={fl:.4} raw={:02x?}", &b[..]); },
            2 => {
                let l = r.varint().unwrap() as usize;
                let b = r.bytes(l).unwrap();
                // 判断是否为可打印文本
                let printable = !b.is_empty() && b.iter().all(|&c| c == b'\n' || (0x20..0x7f).contains(&c));
                if printable {
                    println!("{pad}{label} f{f} str({l})=\"{}\"", String::from_utf8_lossy(&b[..l.min(80)]));
                } else {
                    // 是否像 packed floats（长度为 4 的倍数且首字节模式合适）
                    let looks_f32 = l >= 8 && l % 4 == 0 && {
                        let fl = floats(b);
                        fl.iter().all(|v| v.is_finite() && v.abs() < 100.0)
                    };
                    if looks_f32 {
                        println!("{pad}{label} f{f} packed_f32({l})={:?}", floats(b).iter().map(|v| (v * 1000.0).round() / 1000.0).collect::<Vec<_>>());
                    } else if l > 4 {
                        // 尝试作为嵌套消息： dump 子字段（限深）
                        if depth < 3 {
                            println!("{pad}{label} f{f} msg({l}):");
                            let sub = b.to_vec();
                            dump_fields("", &sub, depth + 1);
                        } else {
                            println!("{pad}{label} f{f} msg({l}) bytes={:02x?}", &b[..l.min(24)]);
                        }
                    } else {
                        println!("{pad}{label} f{f} bytes({l})={:02x?}", b);
                    }
                }
            },
            _ => { println!("{pad}{label} f{f} wire{w} ?"); break; }
        }
    }
}

fn main() {
    let tank_id: u64 = std::env::args().nth(1).expect("usage: tanks_pb_bbox_probe <tank_id>").parse().unwrap();
    let buf = fs::read("data/tanks.pb").unwrap();
    let mut r = Reader { buf: &buf, pos: 0 };
    while let Some(Ok((f, w))) = r.tag() {
        if f == 1 && w == 2 {
            let l = r.varint().unwrap() as usize;
            let b = r.bytes(l).unwrap().to_vec();
            // 条目内 field1 = tank_id
            let mut tr = Reader { buf: &b, pos: 0 };
            let mut id = 0u64;
            let mut found = false;
            while let Some(Ok((ff, ww))) = tr.tag() {
                if ff == 1 && ww == 0 { id = tr.varint().unwrap(); found = true; }
                else { tr.skip_field(ww).unwrap(); }
            }
            if found && id == tank_id {
                println!("=== tank {tank_id} entry ({l}B) ===");
                dump_fields("t", &b, 0);
                break;
            }
        } else {
            r.skip_field(w).unwrap();
        }
    }
}
