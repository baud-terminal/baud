//! ```text
//! Pillow 12.3.0, parts of it transcribed into Rust in this file.
//! Copyright © 1997-2011 by Secret Labs AB
//! Copyright © 1995-2011 by Fredrik Lundh and contributors
//! Copyright © 2010 by Jeffrey 'Alex' Clark and contributors
//!
//! Licence: MIT-CMU. Its text is in LICENSES/pillow-MIT-CMU.txt.
//!
//! Ported: GifImagePlugin._seek's NETSCAPE2.0 application extension
//! read, the "loop" it sets in im.info (loops). Changed: written in Rust
//! over the file's bytes, answering only whether the GIF loops. The rest of
//! this file is baud's.
//! ```
//!
//! **The byte edits made to gifski's GIF of the README logo**: every
//! colour-table entry within [`PALETTE_SNAP`] levels of the page set to
//! the page, and each Graphic Control Extension's delay set to the film's
//! (`[[source: gif89a]]`, 19, 21 and 23), which gifski's single frame rate
//! cannot say for frames of two lengths. They are the deleted driver's
//! block walk and its edit loop, line for line.

/// GIF palette entries this near the page are set to it after encoding.
pub const PALETTE_SNAP: u8 = 6;

/// A block the edits touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    /// A colour table: its offset and its entries.
    Table(usize, usize),
    /// A Graphic Control Extension: its delay's offset.
    Gce(usize),
}

fn at(b: &[u8], i: usize) -> Result<u8, String> {
    // Python indexes past the end with an IndexError; the walk here
    // refuses the same files with a message.
    b.get(i)
        .copied()
        .ok_or_else(|| format!("a GIF cut short at {i}"))
}

/// **The GIF's colour tables and Graphic Control Extensions, in order**:
/// the global table after the logical screen descriptor, then, block by
/// block to the trailer, each extension's sub-blocks skipped (a GCE
/// noted) and each image's local table noted and its data skipped.
///
/// # Errors
/// "a GIF block {c:#x} at {i}" for a block that is none of the three, "a
/// GIF with no trailer" when the bytes end first, as the driver failed.
pub fn blocks(b: &[u8]) -> Result<Vec<Block>, String> {
    let flags = at(b, 10)?;
    let (mut i, mut out) = (13usize, Vec::new());
    if flags & 0x80 != 0 {
        out.push(Block::Table(13, 1 << ((flags & 7) + 1)));
        i += 3 * (1 << ((flags & 7) + 1));
    }
    while i < b.len() {
        let c = b[i];
        if c == 0x3B {
            return Ok(out);
        }
        if c == 0x21 {
            let mut j = i + 2;
            if at(b, i + 1)? == 0xF9 {
                out.push(Block::Gce(j + 2));
            }
            while at(b, j)? != 0 {
                j += usize::from(b[j]) + 1;
            }
            i = j + 1;
        } else if c == 0x2C {
            let mut j = i + 10;
            let packed = at(b, i + 9)?;
            if packed & 0x80 != 0 {
                let k = 1 << ((packed & 7) + 1);
                out.push(Block::Table(j, k));
                j += 3 * k;
            }
            j += 1;
            while at(b, j)? != 0 {
                j += usize::from(b[j]) + 1;
            }
            i = j + 1;
        } else {
            return Err(format!("a GIF block {c:#x} at {i}"));
        }
    }
    Err("a GIF with no trailer".into())
}

/// **The edits**: in `b`, every table entry whose channels are each
/// within [`PALETTE_SNAP`] of `page` set to `page`, and each GCE's delay
/// the sum of the film's `delays_cs` over the frames it spans. gifski at
/// 50 frames a second merges a run of equal frames into one of their
/// summed length, so a GCE's delay over 2 is the count of film frames it
/// stands for.
///
/// # Errors
/// [`blocks`]' refusals, and "{half}: gifski's frames span {k} of the
/// film's {n}" when the GCEs do not cover the film exactly.
pub fn rewrite(
    gif: &[u8],
    delays_cs: &[u32],
    page: [u8; 3],
    half: &str,
) -> Result<Vec<u8>, String> {
    let mut b = gif.to_vec();
    let mut starts = vec![0u32];
    for cs in delays_cs {
        starts.push(starts[starts.len() - 1] + cs);
    }
    let mut k = 0usize;
    for blk in blocks(&b)? {
        match blk {
            Block::Table(o, entries) => {
                for e in 0..entries {
                    let c = o + 3 * e;
                    let near = (0..3).all(|j| b[c + j].abs_diff(page[j]) <= PALETTE_SNAP);
                    if near {
                        b[c..c + 3].copy_from_slice(&page);
                    }
                }
            }
            Block::Gce(o) => {
                let span = usize::from(u16::from_le_bytes([b[o], b[o + 1]]) / 2);
                let (from, to) = (k, k + span);
                // Python's list index raises past the film's end.
                let d = starts.get(to).ok_or_else(|| {
                    format!(
                        "{half}: gifski's frames span past the film's {}",
                        delays_cs.len()
                    )
                })? - starts[from];
                let d = u16::try_from(d).map_err(|_| format!("{half}: a delay of {d} cs"))?;
                b[o..o + 2].copy_from_slice(&d.to_le_bytes());
                k = to;
            }
        }
    }
    if k != delays_cs.len() {
        return Err(format!(
            "{half}: gifski's frames span {k} of the film's {}",
            delays_cs.len()
        ));
    }
    Ok(b)
}

/// **Whether the GIF loops**, as Pillow 12.3.0's `"loop" in im.info`
/// after opening it (`GifImagePlugin._seek(0)`): an application
/// extension before the first image whose first sub-block starts with
/// `NETSCAPE2.0` and whose second is at least 3 bytes starting with 1.
#[must_use]
pub fn loops(b: &[u8]) -> bool {
    let Some(&flags) = b.get(10) else {
        return false;
    };
    let mut i = 13usize;
    if flags & 0x80 != 0 {
        i += 3 * (1 << ((flags & 7) + 1));
    }
    let mut found = false;
    while b.get(i) == Some(&0x21) {
        let label = b.get(i + 1).copied();
        let mut j = i + 2;
        let mut nth = 0;
        let mut netscape = false;
        while let Some(&n) = b.get(j) {
            if n == 0 {
                break;
            }
            let sub = b.get(j + 1..j + 1 + usize::from(n)).unwrap_or(&[]);
            if label == Some(0xFF) && nth == 0 {
                netscape = sub.starts_with(b"NETSCAPE2.0");
            } else if netscape && nth == 1 && sub.len() >= 3 && sub[0] == 1 {
                found = true;
            }
            nth += 1;
            j += usize::from(n) + 1;
        }
        i = j + 1;
    }
    found
}
