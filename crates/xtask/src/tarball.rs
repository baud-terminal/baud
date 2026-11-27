//! A `.crate` read as `tarfile.open(crate).extractfile(name).read()` read
//! it: a gzip member (RFC 1952) holding a tar archive, of which one file
//! is wanted.
//!
//! The deflate stream is inflated by `baud-domain`'s inflater, which reads
//! the zlib framing (RFC 1950) around it. So the gzip header is read here,
//! a zlib header (`78 01`, a 32 KiB window and no dictionary) is fed in its
//! place, and once the final block is inflated the Adler-32 of what it
//! wrote is fed as the trailer that framing ends with: the inflater checks
//! the deflate data, and the gzip trailer's CRC-32 and length are checked
//! here, as `gzip` checks them. The inflated length is the trailer's
//! ISIZE, so the buffer is exactly as long as the stream.
//!
//! The archive is read as POSIX ustar, with the two extensions cargo's
//! `tar` writer uses for a path past 100 bytes: a GNU `L` record, whose
//! data is the next entry's name, and a pax `x` record's `path`.

use std::fmt;

use baud_domain::zlib::{Inflate, Inflated, ZlibError};

/// Why a `.crate` could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not a gzip member, or one this reader does not take.
    Gzip(&'static str),
    /// The deflate stream inside it.
    Deflate(ZlibError),
    /// The trailer's CRC-32 or length does not match.
    Trailer(&'static str),
    /// A tar header that does not parse.
    Tar(&'static str),
    /// No member of that name.
    Missing(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Gzip(w) => write!(f, "not a gzip file: {w}"),
            Refusal::Deflate(e) => write!(f, "the deflate stream: {e:?}"),
            Refusal::Trailer(w) => write!(f, "the gzip trailer: {w}"),
            Refusal::Tar(w) => write!(f, "the tar archive: {w}"),
            Refusal::Missing(n) => write!(f, "\"filename {n} not found\""),
        }
    }
}

/// A CRC-32 as gzip computes it (ISO 3309, reflected, `0xEDB88320`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Crc(pub u32);

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    // Two counters in step, so the index needs no cast.
    let (mut i, mut n) = (0, 0u32);
    while i < 256 {
        let mut c = n;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
        n += 1;
    }
    t
}

const CRC: [u32; 256] = crc_table();

/// The CRC-32 of `bytes`.
pub fn crc32(bytes: &[u8]) -> Crc {
    let mut c = !0u32;
    for &b in bytes {
        c = CRC[usize::from((c ^ u32::from(b)).to_le_bytes()[0])] ^ (c >> 8);
    }
    Crc(!c)
}

/// The Adler-32 of `bytes`, big-endian as zlib's trailer stores it.
fn adler32(bytes: &[u8]) -> [u8; 4] {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    ((b << 16) | a).to_be_bytes()
}

/// The bytes a gzip member holds.
pub fn gunzip(gz: &[u8]) -> Result<Vec<u8>, Refusal> {
    const FHCRC: u8 = 2;
    const FEXTRA: u8 = 4;
    const FNAME: u8 = 8;
    const FCOMMENT: u8 = 16;
    if gz.len() < 18 || gz[0] != 0x1f || gz[1] != 0x8b {
        return Err(Refusal::Gzip("no gzip magic"));
    }
    if gz[2] != 8 {
        return Err(Refusal::Gzip("a method other than deflate"));
    }
    let flg = gz[3];
    let mut at = 10;
    let past = |at: usize| at > gz.len() - 8;
    if flg & FEXTRA != 0 {
        let xlen = usize::from(u16::from_le_bytes([gz[at], gz[at + 1]]));
        at += 2 + xlen;
    }
    for flag in [FNAME, FCOMMENT] {
        if flg & flag != 0 {
            while !past(at) && gz[at] != 0 {
                at += 1;
            }
            at += 1;
        }
    }
    if flg & FHCRC != 0 {
        at += 2;
    }
    if past(at) {
        return Err(Refusal::Gzip("a header that runs into the trailer"));
    }
    let tail = gz.len() - 8;
    let crc = u32::from_le_bytes([gz[tail], gz[tail + 1], gz[tail + 2], gz[tail + 3]]);
    let isize = u32::from_le_bytes([gz[tail + 4], gz[tail + 5], gz[tail + 6], gz[tail + 7]]);
    let len = usize::try_from(isize).map_err(|_| Refusal::Gzip("too long"))?;
    let mut out = vec![0u8; len];
    let mut z = Inflate::NEW;
    let deflate = Refusal::Deflate;
    z.feed(&[0x78, 0x01], &mut out).map_err(deflate)?;
    if z.feed(&gz[at..tail], &mut out).map_err(deflate)? == Inflated::More {
        // The final block is in: what is left is the zlib trailer.
        if z.written() != len {
            return Err(Refusal::Deflate(ZlibError::Truncated));
        }
        let trailer = adler32(&out);
        z.feed(&trailer, &mut out).map_err(deflate)?;
    }
    z.finish().map_err(deflate)?;
    if z.excess() != 0 {
        return Err(Refusal::Trailer("data after the deflate stream"));
    }
    if crc32(&out) != Crc(crc) {
        return Err(Refusal::Trailer("CRC-32 mismatch"));
    }
    Ok(out)
}

/// A tar header's numeric field: octal, NUL- or space-terminated, or GNU
/// base-256 when its first byte has the high bit set.
fn number(field: &[u8]) -> Result<u64, Refusal> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        let mut n = u64::from(field[0] & 0x7f);
        for &b in &field[1..] {
            n = n
                .checked_mul(256)
                .and_then(|n| n.checked_add(u64::from(b)))
                .ok_or(Refusal::Tar("a size past 64 bits"))?;
        }
        return Ok(n);
    }
    let mut n = 0u64;
    for &b in field {
        match b {
            b'0'..=b'7' => {
                n = n
                    .checked_mul(8)
                    .and_then(|n| n.checked_add(u64::from(b - b'0')))
                    .ok_or(Refusal::Tar("a size past 64 bits"))?;
            }
            0 | b' ' => {
                if n > 0 {
                    break;
                }
            }
            _ => return Err(Refusal::Tar("a size that is not octal")),
        }
    }
    Ok(n)
}

/// A NUL-terminated name field.
fn name(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// The `path` record of a pax header's data, if it has one.
fn pax_path(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    let mut rest: &str = &text;
    let mut found = None;
    while let Some((len, _)) = rest.split_once(' ') {
        let n: usize = len.parse().ok()?;
        let rec = rest.get(..n)?;
        let kv = rec.split_once(' ')?.1.strip_suffix('\n')?;
        if let Some(p) = kv.strip_prefix("path=") {
            found = Some(p.to_string());
        }
        rest = &rest[n..];
    }
    found
}

/// The bytes of the member named `want`: the last such member, as
/// `tarfile.getmember` takes it.
pub fn member(tar: &[u8], want: &str) -> Result<Vec<u8>, Refusal> {
    let mut at = 0;
    let mut long: Option<String> = None;
    let mut found = None;
    while at + 512 <= tar.len() {
        let h = &tar[at..at + 512];
        if h.iter().all(|&b| b == 0) {
            break;
        }
        let size = usize::try_from(number(&h[124..136])?).map_err(|_| Refusal::Tar("too long"))?;
        let start = at + 512;
        let end = start
            .checked_add(size)
            .filter(|&e| e <= tar.len())
            .ok_or(Refusal::Tar("a member that runs past the archive"))?;
        let data = &tar[start..end];
        at = start + size.div_ceil(512) * 512;
        match h[156] {
            b'L' => {
                long = Some(name(data));
                continue;
            }
            b'x' => {
                long = pax_path(data).or(long);
                continue;
            }
            b'g' => continue,
            _ => {}
        }
        let full = match long.take() {
            Some(n) => n,
            None => {
                let base = name(&h[0..100]);
                let prefix = if &h[257..262] == b"ustar" {
                    name(&h[345..500])
                } else {
                    String::new()
                };
                if prefix.is_empty() {
                    base
                } else {
                    format!("{prefix}/{base}")
                }
            }
        };
        if full == want {
            found = Some(data.to_vec());
        }
    }
    found.ok_or_else(|| Refusal::Missing(want.to_string()))
}

/// `tarfile.open(path).extractfile(want).read()` of a `.tar.gz`.
pub fn extract(gz: &[u8], want: &str) -> Result<Vec<u8>, Refusal> {
    member(&gunzip(gz)?, want)
}
