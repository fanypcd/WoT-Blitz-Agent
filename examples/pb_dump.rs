//! 递归转储 tanks.pb / models.pb 中指定坦克的全部 protobuf 字段（含未解析字段），
//! 用于寻找未接入的碰撞盒/部件数据。用法：
//!   cargo run --release --example pb_dump -- <tanks.pb|models.pb> <tank_id|dev_name>
use std::io::Write;

struct R<'a> { buf: &'a [u8], pos: usize }

impl<'a> R<'a> {
    fn varint(&mut self) -> anyhow::Result<u64> {
        let mut v = 0u64; let mut s = 0u32;
        loop {
            let b = *self.buf.get(self.pos).ok_or(anyhow::anyhow!("eof"))?;
            self.pos += 1;
            v |= ((b & 0x7f) as u64) << s;
            if b & 0x80 == 0 { return Ok(v); }
            s += 7;
        }
    }
    fn bytes(&mut self, len: usize) -> anyhow::Result<&'a [u8]> {
        let b = self.buf.get(self.pos..self.pos+len).ok_or(anyhow::anyhow!("eof"))?;
        self.pos += len; Ok(b)
    }
    fn tag(&mut self) -> Option<anyhow::Result<(u32, u8)>> {
        if self.pos >= self.buf.len() { return None; }
        Some(self.varint().map(|v| (v as u32, (v & 7) as u8)))
    }
    fn skip(&mut self, w: u8) -> anyhow::Result<()> {
        match w {
            0 => { self.varint()?; }
            1 => { self.bytes(8)?; }
            2 => { let l = self.varint()? as usize; self.bytes(l)?; }
            5 => { self.bytes(4)?; }
            _ => anyhow::bail!("wire {}", w),
        }
        Ok(())
    }
}

fn dump(buf: &[u8], path: &str, depth: usize, out: &mut dyn Write) -> anyhow::Result<()> {
    if depth > 8 { return Ok(()); }
    let mut r = R { buf, pos: 0 };
    let pad = "  ".repeat(depth);
    while r.pos < buf.len() {
        let (f, w) = match r.tag() { Some(x) => x?, None => break };
        match w {
            0 => { let v = r.varint()?; writeln!(out, "{}f{} varint {}", pad, f, v)?; }
            1 => { let b = r.bytes(8)?; writeln!(out, "{}f{} fixed64 {}", pad, f, u64::from_le_bytes(b.try_into().unwrap()))?; }
            2 => {
                let l = r.varint()? as usize;
                let b = r.bytes(l)?;
                // 判断是否可进一步递归：试探性解析（>90% 字节可解且无非法 wire 才递归）
                let printable = b.iter().filter(|&&c| (32..127).contains(&c) || c == b'\n').count();
                let text_like = printable as f32 / l as f32 > 0.85;
                if depth < 8 && !text_like && l > 1 {
                    let sub_start = r.pos; // 无用占位
                    let _ = sub_start;
                    // 递归尝试
                    let mut ok = true;
                    {
                        let mut sr = R { buf: b, pos: 0 };
                        let mut cnt = 0;
                        while sr.pos < b.len() {
                            match sr.tag() { Some(x) => { let (_, w) = x?; if w > 5 { ok = false; break; } sr.skip(w)?; cnt += 1; } None => break }
                        }
                        if cnt == 0 { ok = false; }
                    }
                    if ok {
                        writeln!(out, "{}f{} msg[{}] {{", pad, f, l)?;
                        dump(b, path, depth + 1, out)?;
                        writeln!(out, "{}}}", pad)?;
                        continue;
                    }
                }
                if text_like {
                    writeln!(out, "{}f{} str[{}] {}", pad, f, l, String::from_utf8_lossy(b))?;
                } else {
                    writeln!(out, "{}f{} bytes[{}] {}", pad, f, l, b.iter().take(24).map(|x| format!("{:02x}", x)).collect::<String>())?;
                }
            }
            5 => { let b = r.bytes(4)?; let f32v = f32::from_le_bytes(b.try_into().unwrap()); writeln!(out, "{}f{} f32 {}", pad, f, f32v)?; }
            _ => { writeln!(out, "{}f{} wire{} <?>", pad, f, w)?; r.skip(w)?; }
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let want = &args[2];
    let buf = std::fs::read(path).unwrap();
    let mut out = std::io::stdout();

    // 顶层遍历，定位目标坦克条目
    let mut r = R { buf: &buf, pos: 0 };
    while let Some(res) = r.tag() {
        let (f, w) = res.unwrap();
        if f != 1 || w != 2 { r.skip(w).unwrap(); continue; }
        let len = r.varint().unwrap() as usize;
        let entry = r.bytes(len).unwrap();
        // 读 tank_id（field1 varint）与 dev_name（field2 str）
        let mut er = R { buf: entry, pos: 0 };
        let mut id = 0u32; let mut dev = String::new();
        while let Some(res2) = er.tag() {
            let (f2, w2) = res2.unwrap();
            match (f2, w2) {
                (1, 0) => id = er.varint().unwrap() as u32,
                (2, 2) => { let l = er.varint().unwrap() as usize; dev = String::from_utf8_lossy(er.bytes(l).unwrap()).into_owned(); }
                _ => er.skip(w2).unwrap(),
            }
        }
        if dev.contains(want.as_str()) || id.to_string() == *want {
            writeln!(out, "=== tank {} ({}) ===", id, dev).unwrap();
            let _ = dump(entry, path, 1, &mut out);
            break;
        }
    }
    out.flush().unwrap();
}
