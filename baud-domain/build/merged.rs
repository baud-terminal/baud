//! The merged property table, `OUT_DIR/merged_data.rs`: what the
//! per-character path asks of a code point -- its Grapheme_Cluster_Break
//! class and lookback sets, and its width under the current tables and
//! Unicode 8's -- from one table, compiled from the BREAK trie
//! ([`crate::grapheme`]) and the WIDTH and WIDTH8 tries ([`crate::width`])
//! as the crate reads them.
//!
//! * **MERGED_PALETTE**, each distinct `(class, lookback, width codes)` of
//!   the code space, three bytes each, numbered in code point order of
//!   first use. The width codes are two, the current tables' in bits 0-1
//!   and Unicode 8's in bits 2-3, each 0 narrow, 1 zero, 2 wide and 3 East
//!   Asian Ambiguous: a code, not a width, since the ambiguous width is
//!   the embedder's policy.
//! * **MERGED_INDEX** and **MERGED_BLOCKS**, a two-stage trie of palette
//!   ids, `codepoint >> 8` to a block of 256, blocks numbered in order of
//!   first use and padded with zero blocks no index names to a power of
//!   two, so a block id's range is its bounds check.
//! * **BMP_DIRECT**, the Basic Multilingual Plane flat: a little-endian
//!   `u16` a code point, `class | lookback << 4 | codes << 8`.
//!
//! The values are `GraphemeClass::of_with_lookback`'s and `Width::of`'s
//! and `Width::of_unicode8`'s under both policies, and the three read more
//! than the tries: ASCII is classed without them, U+0000 is zero width and
//! the rest of ASCII narrow, and private use is narrow under either policy
//! where the tries give it ambiguous. A surrogate is no `Codepoint`: its
//! entry is `Other`, no lookback, narrow, and 0 in BMP_DIRECT.
//!
//! A pure function of the two tables: `main` writes what [`build`]
//! returns.

use crate::cluster::class;
use crate::width::packed_get;
use baud_tablegen::{CODE_POINTS, Check, Emit, Error, Errors, Kind};
use std::collections::HashMap;

const PATH: &str = "build/merged.rs";

/// The distinct `(class, lookback, codes)` of 17.0.0.
const PALETTE: usize = 33;

/// The trie values `Width::of` reads.
const NARROW: u8 = 0;
const ZERO: u8 = 1;
const AMBIGUOUS: u8 = 3;

/// `Codepoint::is_private_use`'s ranges, which East_Asian_Width gives `A`
/// and which take one column under either policy.
const PRIVATE_USE: [(usize, usize); 3] = [
    (0xE000, 0xF8FF),
    (0xF_0000, 0xF_FFFD),
    (0x10_0000, 0x10_FFFD),
];

/// The bytes of `emit`'s `<name>.bin`, or the failure.
fn bin<'a>(emit: &'a Emit, name: &str) -> Result<&'a [u8], Error> {
    let file = format!("{name}.bin");
    emit.files()
        .into_iter()
        .find(|(f, _)| *f == file)
        .map(|(_, b)| b)
        .ok_or_else(|| Error::new(Kind::Invariant, PATH, format!("no {file} to read")))
}

/// The class byte at `cp` of the BREAK trie, read as `of_with_lookback`
/// reads it: ASCII without the trie.
fn class_byte(index: &[u8], blocks: &[u8], cp: usize) -> u8 {
    match cp {
        0x0A => class("Lf"),
        0x0D => class("Cr"),
        0x00..=0x1F | 0x7F => class("Control"),
        0x20..=0x7E => class("Other"),
        _ => blocks[usize::from(index[cp >> 8]) * 256 + (cp & 0xFF)],
    }
}

/// The width code at `cp` of a width trie's value `v`, as `Width::of`
/// answers it under both policies: zero or narrow for ASCII, and narrow
/// for private use the trie calls ambiguous.
fn code(v: u8, cp: usize) -> u8 {
    if cp < 0x80 {
        return if cp == 0 { ZERO } else { NARROW };
    }
    let private = PRIVATE_USE.iter().any(|&(lo, hi)| (lo..=hi).contains(&cp));
    if v == AMBIGUOUS && private { NARROW } else { v }
}

/// The table, or every failure found compiling it from `grapheme`'s and
/// `width`'s output.
pub fn build(grapheme: &Emit, width: &Emit) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let tries = [
        bin(grapheme, "break_index"),
        bin(grapheme, "break_blocks"),
        bin(width, "width_index"),
        bin(width, "width_blocks"),
        bin(width, "width8_index"),
        bin(width, "width8_blocks"),
    ];
    let Ok([bi, bb, wi, wb, w8i, w8b]) = <[&[u8]; 6]>::try_from(
        tries
            .into_iter()
            .filter_map(|t| errs.take(t))
            .collect::<Vec<_>>(),
    ) else {
        return Err(errs);
    };
    let classes = u8::try_from(crate::grapheme::CLASSES.len()).expect("14 classes");
    if let Some(cp) = (0..CODE_POINTS).find(|&cp| class_byte(bi, bb, cp) & 0xF >= classes) {
        errs.push(Error::new(
            Kind::Invariant,
            PATH,
            format!("U+{cp:04X}'s class is past the 14 GraphemeClass has"),
        ));
        return Err(errs);
    }
    let other = class("Other");
    // The palette entry of every code point, each id a palette's place.
    let entry = |cp: usize| -> (u8, u8, u8) {
        if (0xD800..0xE000).contains(&cp) {
            return (other, 0, 0);
        }
        let b = class_byte(bi, bb, cp);
        let codes = code(packed_get(wi, wb, cp), cp) | code(packed_get(w8i, w8b, cp), cp) << 2;
        (b & 0xF, b >> 4, codes)
    };
    let mut palette: Vec<(u8, u8, u8)> = Vec::new();
    let mut place: HashMap<(u8, u8, u8), usize> = HashMap::new();
    let mut leaf = vec![0usize; CODE_POINTS];
    for (cp, id) in leaf.iter_mut().enumerate() {
        let t = entry(cp);
        let n = palette.len();
        *id = *place.entry(t).or_insert_with(|| {
            palette.push(t);
            n
        });
    }
    errs.count(PATH, "palette entries", palette.len(), PALETTE);
    errs.require(Check::from(palette.len() <= 256), || {
        Error::new(
            Kind::Invariant,
            PATH,
            format!("{} palette entries will not fit a byte", palette.len()),
        )
    });
    let mut blocks: Vec<u8> = Vec::new();
    let mut seen: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut index: Vec<u8> = Vec::new();
    for hi in 0..CODE_POINTS >> 8 {
        // A palette id past a byte is refused above; it is cut here only
        // so the walk ends, and nothing is emitted.
        let block: Vec<u8> = leaf[hi * 256..hi * 256 + 256]
            .iter()
            .map(|&id| u8::try_from(id).unwrap_or(u8::MAX))
            .collect();
        let n = seen.len();
        let id = *seen.entry(block.clone()).or_insert_with(|| {
            blocks.extend_from_slice(&block);
            n
        });
        index.push(u8::try_from(id).unwrap_or(u8::MAX));
    }
    errs.require(Check::from(seen.len() <= 256), || {
        Error::new(
            Kind::Invariant,
            PATH,
            format!("{} distinct blocks will not fit a byte", seen.len()),
        )
    });
    // At 128 blocks the id's sign bit is the bounds check (`ldrsb`,
    // `tbnz`); at 126, which private use staying narrow left, every
    // two-stage lookup took a compare more, +0.069% instructions on
    // `cat-cjk`. The padding is never read.
    blocks.resize(seen.len().next_power_of_two() * 256, 0);

    let mut bmp: Vec<u8> = Vec::with_capacity(0x10000 * 2);
    for cp in 0..0x10000 {
        let v = if (0xD800..0xE000).contains(&cp) {
            0
        } else {
            let (c, l, w) = entry(cp);
            u16::from(c) | u16::from(l) << 4 | u16::from(w) << 8
        };
        bmp.extend_from_slice(&v.to_le_bytes());
    }

    // Positive controls: a code point of each override, and one each of
    // the codes, read back through the trie and the flat plane.
    let wide = 2;
    for (cp, want, what) in [
        (
            0x0000,
            (class("Control"), 0, ZERO | ZERO << 2),
            "NULL, zero width",
        ),
        (0x0041, (other, 0, 0), "LATIN CAPITAL LETTER A"),
        (
            0x00A1,
            (other, 0, AMBIGUOUS | AMBIGUOUS << 2),
            "an ambiguous",
        ),
        (
            0xE000,
            (other, 0, 0),
            "private use, narrow under either policy",
        ),
        (0x4E00, (other, 0, wide | wide << 2), "a CJK ideograph"),
        (
            0x1F1E6,
            (class("RegionalIndicator"), 0, wide),
            "a regional indicator",
        ),
        (0xD800, (other, 0, 0), "a surrogate"),
    ] {
        let id = index.get(cp >> 8).map(|&b| {
            let at = usize::from(b) * 256 + (cp & 0xFF);
            usize::from(blocks.get(at).copied().unwrap_or(u8::MAX))
        });
        let got = id.and_then(|i| palette.get(i)).copied();
        let flat = (cp < 0x10000).then(|| u16::from_le_bytes([bmp[cp * 2], bmp[cp * 2 + 1]]));
        let (c, l, w) = want;
        let want_flat = match cp {
            0xD800..0xE000 => 0,
            _ => u16::from(c) | u16::from(l) << 4 | u16::from(w) << 8,
        };
        errs.require(
            Check::from(got == Some(want) && flat.is_none_or(|f| f == want_flat)),
            || {
                Error::new(
                    Kind::Control,
                    PATH,
                    format!("U+{cp:04X} ({what}) is {got:?} and {flat:?}, {want:?} expected"),
                )
            },
        );
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    let mut out = Emit::new(
        "merged_data",
        "baud-domain/build.rs (build/merged.rs) from the grapheme and width tables",
        &[],
    );
    let triples: Vec<u8> = palette.iter().flat_map(|&(c, l, w)| [c, l, w]).collect();
    let distinct = seen.len();
    out.bytes(
        "MERGED_PALETTE",
        &[&format!(
            "`(class, lookback, width codes)` per palette id: {} of them.",
            palette.len()
        )],
        &triples,
    );
    out.bytes(
        "MERGED_INDEX",
        &["`cp >> 8` to a block of [`MERGED_BLOCKS`]."],
        &index,
    );
    out.bytes(
        "MERGED_BLOCKS",
        &[
            &format!("{distinct} deduplicated blocks of 256 palette ids, padded with blocks no"),
            &format!(
                "index names to {}, a power of two, so a block id's range is its",
                blocks.len() / 256
            ),
            "bounds check.",
        ],
        &blocks,
    );
    out.bytes(
        "BMP_DIRECT",
        &[
            "The Basic Multilingual Plane, **flat**: one load, no stage-1",
            "indirection. Little-endian `u16` per codepoint,",
            "`class | lookback << 4 | width codes << 8`.",
        ],
        &bmp,
    );
    errs.or(out)
}
