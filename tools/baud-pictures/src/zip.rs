//! One member of a zip archive, as `zipfile.ZipFile.read(name)` gives it:
//! found through the central directory, stored or deflated, and refused
//! unless its CRC-32 and size are the directory's.

use crate::zlib;

fn u16_at(b: &[u8], i: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(
        b.get(i..i + 2)?.try_into().ok()?,
    )))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// The bytes of `name` in the archive `zip`.
///
/// # Errors
/// When the archive has no such member or it does not read back whole.
pub fn read(zip: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let eocd = (0..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| zip[i..].starts_with(b"PK\x05\x06"))
        .ok_or("no end of central directory")?;
    let count = u16_at(zip, eocd + 10).ok_or("a short directory")?;
    let mut at = u32_at(zip, eocd + 16).ok_or("a short directory")? as usize;
    for _ in 0..count {
        if !zip.get(at..).is_some_and(|b| b.starts_with(b"PK\x01\x02")) {
            return Err("a bad central directory entry".into());
        }
        let bad = || "a short central directory entry".to_string();
        let method = u16_at(zip, at + 10).ok_or_else(bad)?;
        let crc = u32_at(zip, at + 16).ok_or_else(bad)?;
        let packed = u32_at(zip, at + 20).ok_or_else(bad)? as usize;
        let size = u32_at(zip, at + 24).ok_or_else(bad)? as usize;
        let (n, extra, comment) = (
            u16_at(zip, at + 28).ok_or_else(bad)?,
            u16_at(zip, at + 30).ok_or_else(bad)?,
            u16_at(zip, at + 32).ok_or_else(bad)?,
        );
        let local = u32_at(zip, at + 42).ok_or_else(bad)? as usize;
        let here = zip.get(at + 46..at + 46 + n).ok_or_else(bad)?;
        at += 46 + n + extra + comment;
        if here != name.as_bytes() {
            continue;
        }
        if !zip
            .get(local..)
            .is_some_and(|b| b.starts_with(b"PK\x03\x04"))
        {
            return Err(format!("{name}: no local header"));
        }
        let start = local
            + 30
            + u16_at(zip, local + 26).ok_or_else(bad)?
            + u16_at(zip, local + 28).ok_or_else(bad)?;
        let data = zip
            .get(start..start + packed)
            .ok_or("a member past the archive's end")?;
        let out = match method {
            0 => data.to_vec(),
            8 => zlib::inflate_all(data, -15, Some(size))?,
            m => return Err(format!("{name}: compression method {m}")),
        };
        if out.len() != size || zlib::crc(0, &out) != crc {
            return Err(format!("{name}: its CRC-32 or size is not the directory's"));
        }
        return Ok(out);
    }
    Err(format!("there is no item named {name:?} in the archive"))
}
