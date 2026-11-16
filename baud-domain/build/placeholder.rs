//! The Unicode placeholder diacritics, `OUT_DIR/placeholder_data.rs`
//! (`[[source: kitty-graphics]]`, "Unicode placeholders"): "The row and
//! column values are specified with diacritics listed in
//! rowcolumn-diacritics.txt", whose data line n, from zero, is the
//! diacritic for the number n ("U+305 is the diacritic corresponding to
//! the number 0 and U+30D corresponds to 1"). The list's codepoints are
//! pinned as `[[source: kitty-placeholder-diacritics]]`, read from
//! `[[source: kitty-rowcolumn-diacritics]]` by the sha256 its line 1 names.
//!
//! Two tables, both indexed and neither searched: the number to its
//! codepoint, and the codepoint to its number as a two-stage trie on
//! `codepoint >> 6`, which is what a mark after U+10EEEE asks.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{Check, Emit, Errors, Input, Kind, PerLine, Source};
use std::collections::HashMap;

/// Line 1 names the parent and its sha256, so a list read from another
/// copy of kitty's file is another header.
pub const DIACRITICS: Input = Input {
    file: "kitty-placeholder-diacritics.txt",
    key: "kitty-placeholder-diacritics",
    header: "# The Unicode placeholder diacritics, from [[source: kitty-rowcolumn-diacritics]] \
             at sha256 a80368b3272c41d8b50f3f640cf4305b6423e5a1aae6b72a405129bc29425f2c:",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 1] = [DIACRITICS];

/// How many the list holds.
const COUNT: usize = 297;

/// The text's own three: U+0305 is 0, U+030D 1 and U+030E 2.
const FIRST: [u32; 3] = [0x305, 0x30D, 0x30E];

/// A block is `1 << SHIFT` codepoints.
pub const SHIFT: u32 = 6;

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let src = Source::load(&DIACRITICS, read)?;
    let mut cps: Vec<u32> = Vec::new();
    let mut seen: HashMap<u32, usize> = HashMap::new();
    for row in src.rows() {
        if errs.take(row.expect(1)).is_none() {
            continue;
        }
        let Some(cp) = errs.take(row.code_point(0)) else {
            continue;
        };
        if let Some(line) = seen.get(&cp) {
            errs.push(row.error(
                Kind::Duplicate,
                0,
                format!("U+{cp:04X} is listed already on line {line}"),
            ));
            continue;
        }
        seen.insert(cp, row.line());
        cps.push(cp);
    }
    errs.count(src.path(), "diacritics", cps.len(), COUNT);
    for (n, want) in FIRST.iter().enumerate() {
        let got = cps.get(n).copied();
        errs.require(Check::from(got == Some(*want)), || {
            src.error(
                Kind::Control,
                format!("diacritic {n} is {got:X?}, U+{want:04X} expected"),
            )
        });
    }
    let Some(&max) = cps.iter().max() else {
        return Err(errs);
    };
    let block = 1usize << SHIFT;
    let pages = usize::try_from(max >> SHIFT).expect("a page") + 1;
    let by_cp: HashMap<u32, u16> = cps
        .iter()
        .enumerate()
        .map(|(n, &c)| (c, u16::try_from(n + 1).expect("a number below u16::MAX")))
        .collect();
    // Blocks numbered in first-seen order, block 0 the empty one.
    let mut blocks: Vec<Vec<u16>> = vec![vec![0; block]];
    let mut number: HashMap<Vec<u16>, usize> = HashMap::from([(vec![0; block], 0)]);
    let mut index: Vec<usize> = Vec::with_capacity(pages);
    for p in 0..pages {
        let base = u32::try_from(p << SHIFT).expect("a codepoint");
        let b: Vec<u16> = (0..block)
            .map(|i| {
                let cp = base + u32::try_from(i).expect("a codepoint");
                by_cp.get(&cp).copied().unwrap_or(0)
            })
            .collect();
        let next = blocks.len();
        let at = *number.entry(b.clone()).or_insert(next);
        if at == next {
            blocks.push(b);
        }
        index.push(at);
    }
    errs.require(Check::from(blocks.len() <= 256), || {
        src.error(
            Kind::Invariant,
            format!("{} blocks, more than a u8 names", blocks.len()),
        )
    });
    let mut out = Emit::new(
        "placeholder_data",
        "baud-domain/build.rs (build/placeholder.rs)",
        &[&src],
    );
    out.array(
        "DIACRITICS",
        "u32",
        &[&format!(
            "The diacritic for each number, `0..{}`: the list's data lines in order.",
            cps.len()
        )],
        &cps,
        PerLine(8),
        |v| format!("0x{v:05X}"),
    );
    out.code(&format!(
        "/// How far a codepoint is shifted for [`DIACRITIC_INDEX`]: blocks of {block}.\n\
         pub const DIACRITIC_SHIFT: u32 = {SHIFT};\n"
    ));
    out.array(
        "DIACRITIC_INDEX",
        "u8",
        &[
            &format!("Stage 1: `codepoint >> {SHIFT}` to its block in [`DIACRITIC_BLOCKS`], for"),
            &format!(
                "every codepoint below `0x{:05X}`; above it none is a diacritic.",
                pages << SHIFT
            ),
        ],
        &index,
        PerLine(16),
        usize::to_string,
    );
    let flat: Vec<u16> = blocks.concat();
    out.array(
        "DIACRITIC_BLOCKS",
        "u16",
        &[
            &format!(
                "Stage 2: {} blocks of {block}, each entry the number plus one, or",
                blocks.len()
            ),
            "zero for a codepoint that is not a diacritic. Block 0 is all zeros.",
        ],
        &flat,
        PerLine(16),
        u16::to_string,
    );
    errs.or(out)
}
