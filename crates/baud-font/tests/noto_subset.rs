//! **`tests/fonts/NotoColorEmoji-subset.ttf`, subset from Noto Color Emoji.**
//!
//! Noto Color Emoji (`[[source: noto-color-emoji-cbdt]]`, 10.7 MB) is the
//! one widely-shipped CBDT face and it is too large to commit, so
//! [`subset`] takes its first [`N`] glyphs and writes a self-contained sfnt
//! that exercises every index format and every PNG image format the reader
//! in `crates/baud-font/src/bitmap.rs` implements:
//!
//! | glyph range | index format | image format |
//! |---|---|---|
//! | 4..15 | 1 (u32 offsets) | 17 (small metrics + PNG) |
//! | 16..27 | 3 (u16 offsets) | 18 (big metrics + PNG) |
//! | 28..33 | 4 (sparse glyph/offset pairs) | 17 |
//! | 34..39 | 5 (constant size, sparse ids) | 19 (metrics in the subtable) |
//! | 40..45 | 2 (constant size, dense) | 19 |
//!
//! Noto itself uses only index format 1 with image format 17, so the other
//! arrangements are re-encodings of its records rather than anything Noto
//! ships; the PNG bytes and the metrics are Noto's own. Glyphs 0..3 have no
//! bitmap in Noto and are kept so the reader's "no image" answer is
//! exercised.
//!
//! Constant-size subtables (formats 2 and 5) hold each record in
//! `imageSize` bytes, so every record in them is padded to the longest; the
//! `dataLen` field inside format 19 bounds the PNG, and the padding is never
//! read.
//!
//! `head`, `hhea`, `maxp`, `OS/2`, `post` and the horizontal metrics are
//! Noto's, truncated to the subset; `cmap` is rebuilt as one format 12
//! subtable over the codepoints Noto maps to the kept glyphs.
//!
//! The source is not committed, so the subsetter runs by hand, against the
//! pinned file, and holds the committed subset to what it writes:
//!
//! ```text
//! BAUD_NOTO_CBDT=/path/to/NotoColorEmoji.ttf \
//!   cargo test -p baud-font --test noto_subset -- --ignored
//! ```

use std::collections::{BTreeMap, HashMap};

mod sha256;
use sha256::sha256_hex;

/// Glyphs kept: 0..3 without bitmaps, then 42 that carry one.
const N: u16 = 46;

/// `noto-color-emoji-cbdt`'s SHA-256 in `MANIFEST.toml`.
const SOURCE_SHA256: &str = "72a635cb3d2f3524c51620cdde406b217204e8a6a06c6a096ff8ed4b5fd6e27b";

fn u16_at(d: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([d[at], d[at + 1]])
}

fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(d[at..at + 4].try_into().unwrap())
}

/// A 32-bit offset or length, as an index.
fn at32(d: &[u8], at: usize) -> usize {
    usize::try_from(u32_at(d, at)).unwrap()
}

fn tables(d: &[u8]) -> HashMap<[u8; 4], &[u8]> {
    (0..usize::from(u16_at(d, 4)))
        .map(|i| {
            let r = 12 + 16 * i;
            let off = at32(d, r + 8);
            let len = at32(d, r + 12);
            (d[r..r + 4].try_into().unwrap(), &d[off..off + len])
        })
        .collect()
}

/// A glyph's smallGlyphMetrics (height, width, bearingX, bearingY,
/// advance) and its PNG.
type Record = ([u8; 5], Vec<u8>);

/// The one size of a format-1/17 CBLC: its 48 bytes, and every glyph's
/// record.
fn read_cbdt(cblc: &[u8], cbdt: &[u8]) -> (Vec<u8>, BTreeMap<u32, Record>) {
    assert_eq!(u32_at(cblc, 4), 1, "Noto has one size");
    let size = cblc[8..56].to_vec();
    let (isa, nis) = (at32(&size, 0), at32(&size, 8));
    let mut out = BTreeMap::new();
    for j in 0..nis {
        let a = isa + 8 * j;
        let (fg, lg) = (u32::from(u16_at(cblc, a)), u32::from(u16_at(cblc, a + 2)));
        let h = isa + at32(cblc, a + 4);
        let formats = (u16_at(cblc, h), u16_at(cblc, h + 2));
        assert_eq!(formats, (1, 17));
        let ido = at32(cblc, h + 4);
        let off = |k: u32| at32(cblc, h + 8 + 4 * usize::try_from(k).unwrap());
        for g in fg..=lg {
            let (s, e) = (off(g - fg), off(g - fg + 1));
            if e == s {
                continue;
            }
            let rec = &cbdt[ido + s..ido + e];
            let len = at32(rec, 5);
            let png = rec[9..9 + len].to_vec();
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "glyph {g}");
            out.insert(g, (rec[..5].try_into().unwrap(), png));
        }
    }
    (size, out)
}

/// smallGlyphMetrics as bigGlyphMetrics, the vertical fields mirroring
/// the horizontal.
fn big(small: &[u8; 5]) -> Vec<u8> {
    [&small[..], &small[2..5]].concat()
}

fn be16(v: usize) -> [u8; 2] {
    u16::try_from(v).unwrap().to_be_bytes()
}

fn be32(v: usize) -> [u8; 4] {
    u32::try_from(v).unwrap().to_be_bytes()
}

/// The CBDT data and the CBLC's subtable array and headers covering
/// glyphs 4..45 in the five arrangements.
fn build_cbdt(records: &BTreeMap<u16, Record>) -> (Vec<u8>, Vec<u8>, Vec<u8>, usize) {
    let mut cbdt = vec![0, 3, 0, 0];
    let mut subtables: Vec<(u16, u16, Vec<u8>)> = Vec::new();
    let mut emit = |cbdt: &mut Vec<u8>, first: u16, last: u16, index: u16, image: u16| {
        let ido = cbdt.len();
        let mut header = [index.to_be_bytes(), image.to_be_bytes()].concat();
        header.extend(be32(ido));
        let mut body = Vec::new();
        match index {
            // u32 offsets, record = small metrics + length + PNG
            1 => {
                for g in first..=last {
                    body.extend(be32(cbdt.len() - ido));
                    let (m, png) = &records[&g];
                    cbdt.extend(m);
                    cbdt.extend(be32(png.len()));
                    cbdt.extend(png);
                }
                body.extend(be32(cbdt.len() - ido));
            }
            // u16 offsets, record = big metrics + length + PNG
            3 => {
                let mut n = 0;
                for g in first..=last {
                    body.extend(be16(cbdt.len() - ido));
                    let (m, png) = &records[&g];
                    cbdt.extend(big(m));
                    cbdt.extend(be32(png.len()));
                    cbdt.extend(png);
                    n += 1;
                }
                body.extend(be16(cbdt.len() - ido));
                if (n + 1) % 2 == 1 {
                    // keep the next subtable 4-aligned
                    body.extend([0, 0]);
                }
            }
            // sparse (glyph, offset) pairs, then a sentinel
            4 => {
                let mut pairs = Vec::new();
                for g in first..=last {
                    pairs.push((g, cbdt.len() - ido));
                    let (m, png) = &records[&g];
                    cbdt.extend(m);
                    cbdt.extend(be32(png.len()));
                    cbdt.extend(png);
                }
                pairs.push((0xFFFF, cbdt.len() - ido));
                body.extend(be32(pairs.len() - 1));
                for (g, o) in pairs {
                    body.extend(g.to_be_bytes());
                    body.extend(be16(o));
                }
            }
            // constant imageSize, shared big metrics: 5 lists its ids, 2
            // is dense
            _ => {
                let size = (first..=last)
                    .map(|g| 4 + records[&g].1.len())
                    .max()
                    .unwrap();
                for g in first..=last {
                    let png = &records[&g].1;
                    let start = cbdt.len();
                    cbdt.extend(be32(png.len()));
                    cbdt.extend(png);
                    cbdt.resize(start + size, 0);
                }
                body.extend(be32(size));
                body.extend(big(&records[&first].0));
                if index == 5 {
                    body.extend(be32(usize::from(last - first) + 1));
                    for g in first..=last {
                        body.extend(g.to_be_bytes());
                    }
                }
            }
        }
        header.extend(body);
        subtables.push((first, last, header));
    };
    emit(&mut cbdt, 4, 15, 1, 17);
    emit(&mut cbdt, 16, 27, 3, 18);
    emit(&mut cbdt, 28, 33, 4, 17);
    emit(&mut cbdt, 34, 39, 5, 19);
    emit(&mut cbdt, 40, 45, 2, 19);
    let array_len = 8 * subtables.len();
    let (mut array, mut headers) = (Vec::new(), Vec::new());
    for (first, last, header) in &subtables {
        array.extend(first.to_be_bytes());
        array.extend(last.to_be_bytes());
        array.extend(be32(array_len + headers.len()));
        headers.extend(header);
        headers.resize(headers.len().next_multiple_of(4), 0);
    }
    (cbdt, array, headers, subtables.len())
}

/// The first format 12 subtable's groups.
fn read_cmap12(cmap: &[u8]) -> Vec<(u32, u32, u32)> {
    for i in 0..usize::from(u16_at(cmap, 2)) {
        let off = at32(cmap, 8 + 8 * i);
        if u16_at(cmap, off) == 12 {
            return (0..at32(cmap, off + 12))
                .map(|k| {
                    let at = off + 16 + 12 * k;
                    (u32_at(cmap, at), u32_at(cmap, at + 4), u32_at(cmap, at + 8))
                })
                .collect();
        }
    }
    panic!("no format 12 cmap");
}

/// A format 12 `cmap` over the code points Noto maps to a kept glyph,
/// coalesced into sequential groups, and how many code points it maps.
fn build_cmap(groups: &[(u32, u32, u32)], old_to_new: &HashMap<u32, u32>) -> (Vec<u8>, usize) {
    let mut mapped: Vec<(u32, u32)> = groups
        .iter()
        .flat_map(|&(s, e, g)| {
            (s..=e).filter_map(move |c| Some((c, *old_to_new.get(&(g + c - s))?)))
        })
        .collect();
    mapped.sort();
    let mut runs: Vec<(u32, u32, u32)> = Vec::new();
    for &(c, gid) in &mapped {
        match runs.last_mut() {
            Some(r) if r.1 + 1 == c && r.2 + (r.1 - r.0) + 1 == gid => r.1 = c,
            _ => runs.push((c, c, gid)),
        }
    }
    let mut out = [0u16, 1, 3, 10]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect::<Vec<u8>>();
    out.extend(12u32.to_be_bytes());
    out.extend([0, 12, 0, 0]);
    out.extend(be32(16 + 12 * runs.len()));
    out.extend(0u32.to_be_bytes());
    out.extend(be32(runs.len()));
    for (s, e, g) in runs {
        out.extend(s.to_be_bytes());
        out.extend(e.to_be_bytes());
        out.extend(g.to_be_bytes());
    }
    (out, mapped.len())
}

fn checksum(data: &[u8]) -> u32 {
    data.chunks(4)
        .map(|c| {
            let mut w = [0u8; 4];
            w[..c.len()].copy_from_slice(c);
            u32::from_be_bytes(w)
        })
        .fold(0, u32::wrapping_add)
}

/// An sfnt of `tables`, sorted by tag, each padded to a word. `head` is
/// Noto's, its checksumAdjustment as Noto has it.
fn sfnt(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|t| t.0);
    let n = tables.len();
    let mut es = 1;
    while es << 1 <= n {
        es <<= 1;
    }
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    let selector = usize::try_from(es.trailing_zeros()).unwrap();
    for v in [n, es * 16, selector, n * 16 - es * 16] {
        out.extend(be16(v));
    }
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend(tag);
        out.extend(checksum(data).to_be_bytes());
        out.extend(be32(12 + 16 * n + body.len()));
        out.extend(be32(data.len()));
        body.extend(data);
        body.resize(body.len().next_multiple_of(4), 0);
    }
    out.extend(body);
    out
}

/// The subset of `noto`, and a line saying what it holds.
fn subset(noto: &[u8]) -> (Vec<u8>, String) {
    let t = tables(noto);
    let (size, records) = read_cbdt(t[b"CBLC"], t[b"CBDT"]);
    assert!((0..4).all(|g| !records.contains_key(&g)));
    // Noto's bitmap range has holes, so the fixture's glyph k is the kth of
    // Noto's glyphs that carries a bitmap (after the four that do not), and
    // every table is remapped through keep[new] = old.
    let keep: Vec<u32> = (0..4)
        .chain(records.keys().copied().take(usize::from(N) - 4))
        .collect();
    assert_eq!(keep.len(), usize::from(N));
    let old_to_new: HashMap<u32, u32> = keep
        .iter()
        .enumerate()
        .map(|(k, &g)| (g, u32::try_from(k).unwrap()))
        .collect();
    let kept: BTreeMap<u16, Record> = keep[4..]
        .iter()
        .map(|g| (u16::try_from(old_to_new[g]).unwrap(), records[g].clone()))
        .collect();
    let (cbdt, array, headers, nsub) = build_cbdt(&kept);
    let isa = 8 + 48;
    // version 3.0, one size
    let mut cblc = [3u16, 0, 0, 1]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect::<Vec<u8>>();
    cblc.extend(be32(isa));
    cblc.extend(be32(array.len() + headers.len()));
    cblc.extend(be32(nsub));
    // colorRef, hori, vert
    cblc.extend(&size[12..40]);
    cblc.extend(4u16.to_be_bytes());
    cblc.extend((N - 1).to_be_bytes());
    // ppemX, ppemY, bitDepth, flags
    cblc.extend(&size[44..48]);
    cblc.extend(array);
    cblc.extend(headers);

    let mut maxp = t[b"maxp"][..6].to_vec();
    maxp[4..6].copy_from_slice(&N.to_be_bytes());
    let mut hhea = t[b"hhea"].to_vec();
    let n_hm = u32::from(u16_at(&hhea, 34));
    hhea[34..36].copy_from_slice(&N.to_be_bytes());
    let src = t[b"hmtx"];
    let mut hmtx = Vec::new();
    for &g in &keep {
        let g = usize::try_from(g).unwrap();
        let n_hm = usize::try_from(n_hm).unwrap();
        if g < n_hm {
            hmtx.extend(&src[4 * g..4 * g + 4]);
        } else {
            hmtx.extend(&src[4 * (n_hm - 1)..4 * (n_hm - 1) + 2]);
            hmtx.extend(&src[4 * n_hm + 2 * (g - n_hm)..4 * n_hm + 2 * (g - n_hm) + 2]);
        }
    }
    let (cmap, mapped) = build_cmap(&read_cmap12(t[b"cmap"]), &old_to_new);
    let cbdt_len = cbdt.len();
    let data = sfnt(vec![
        (*b"CBDT", cbdt),
        (*b"CBLC", cblc),
        (*b"OS/2", t[b"OS/2"].to_vec()),
        (*b"cmap", cmap),
        (*b"head", t[b"head"].to_vec()),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"maxp", maxp),
        (*b"post", t[b"post"][..32].to_vec()),
    ]);
    let line = format!(
        "{} bytes, {N} glyphs, {} bitmaps from Noto glyphs {}..{}, {mapped} codepoints mapped, CBDT {cbdt_len} B",
        data.len(),
        N - 4,
        keep[4],
        keep[keep.len() - 1]
    );
    (data, line)
}

#[test]
#[ignore = "a tool: needs the pinned NotoColorEmoji.ttf at BAUD_NOTO_CBDT"]
fn the_noto_subset_is_what_the_subsetter_writes() {
    let path = std::env::var("BAUD_NOTO_CBDT")
        .expect("BAUD_NOTO_CBDT names the pinned NotoColorEmoji.ttf");
    let noto = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(
        sha256_hex(&noto),
        SOURCE_SHA256,
        "{path} is not the pinned Noto"
    );
    let (fresh, line) = subset(&noto);
    println!("{line}");
    let committed = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/NotoColorEmoji-subset.ttf"
    ))
    .unwrap();
    if fresh != committed {
        let out = std::env::temp_dir().join(format!("baud-{}-noto-subset.ttf", std::process::id()));
        std::fs::write(&out, &fresh).unwrap();
        panic!(
            "NotoColorEmoji-subset.ttf is not what the subsetter writes; its file is {}",
            out.display()
        );
    }
}
