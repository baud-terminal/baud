//! The caseless tables, `OUT_DIR/caseless_data.rs`: what a font family
//! name compares. CSS Fonts 3 section 5.1 (`[[source: css-fonts-3]]`)
//! matches a family name "case insensitively, using the \"Default Caseless
//! Matching\" algorithm" of the Unicode Standard, "applied without
//! normalizing the strings involved", with "the case mappings with status
//! field 'C' or 'F' in the CaseFolding.txt file". That is D144 of
//! `[[source: unicode17-ch3]]`: toCasefold(X) = toCasefold(Y), R4's
//! toCasefold mapping each character to its full case folding
//! (`[[source: unicode-casefolding]]`) and nothing else. The search tables
//! (`build/search.rs`) fold and then decompose, the canonical caseless
//! match of D145, which would match a precomposed letter against its
//! decomposition -- the pair CSS says "will not match" -- so this is its
//! own table.
//!
//! Per code point the table holds one thing: its full case folding, as an
//! index into a pool of sequences, index 0 meaning "the code point
//! itself", numbered in code point order. A two-stage trie on
//! `codepoint >> 8`, read back through the pool at every code point before
//! it is returned.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::ucd::{self, at};
use baud_tablegen::{CODE_POINTS, Check, Emit, Errors, Input, Kind, PerLine, Source, Trie};
use std::collections::HashMap;

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 1] = [ucd::CASE_FOLDING];

const SHIFT: u32 = 8;

/// The sequence at pool index `i`, `cp` itself at 0.
pub fn pooled(pool: &[u32], i: u16, cp: u32) -> Vec<u32> {
    let i = usize::from(i);
    if i == 0 {
        return vec![cp];
    }
    let n = usize::try_from(pool[i]).unwrap_or(0);
    pool[i + 1..i + 1 + n].to_vec()
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let cf = Source::load(&ucd::CASE_FOLDING, read)?;
    let fold = ucd::case_folding(&cf, &mut errs);
    // Positive controls: sharp s folds to "ss", dotted capital I to "i" and
    // a combining dot (the Turkic T row left out), KELVIN SIGN to "k",
    // A with ring to one code point rather than a decomposition, and I to
    // i.
    for (cp, want) in [
        (0xDF, &[0x73, 0x73][..]),
        (0x130, &[0x69, 0x307]),
        (0x212A, &[0x6B]),
        (0xC5, &[0xE5]),
        (0x49, &[0x69]),
    ] {
        let got = fold.get(&cp);
        errs.require(Check::from(got.map(Vec::as_slice) == Some(want)), || {
            cf.error(
                Kind::Control,
                format!("U+{cp:04X} folds to {got:X?}, {want:X?} expected"),
            )
        });
    }

    let mut pool: Vec<u32> = vec![0];
    let mut seqs: HashMap<&[u32], u16> = HashMap::new();
    let mut entry = vec![0u16; CODE_POINTS];
    for (&cp, to) in &fold {
        let i = match seqs.get(to.as_slice()) {
            Some(&i) => i,
            None => {
                let Ok(i) = u16::try_from(pool.len()) else {
                    errs.push(cf.error(Kind::Invariant, "the pool does not fit a u16 index"));
                    return Err(errs);
                };
                seqs.insert(to, i);
                pool.push(u32::try_from(to.len()).expect("a short folding"));
                pool.extend(to);
                i
            }
        };
        entry[at(cp)] = i;
    }
    errs.require(Check::from(pool.len() <= 0xFFFF), || {
        cf.error(
            Kind::Invariant,
            format!("{} pool words do not fit a u16", pool.len()),
        )
    });
    let Some(trie) = errs.take(Trie::build("CASELESS", &entry, SHIFT)) else {
        return Err(errs);
    };
    let wrong = (0..=baud_tablegen::LAST).find(|&cp| {
        let i = trie.get(at(cp), SHIFT).unwrap_or(0);
        let want = fold.get(&cp).cloned().unwrap_or_else(|| vec![cp]);
        pooled(&pool, i, cp) != want
    });
    if let Some(cp) = wrong {
        errs.push(cf.error(
            Kind::Invariant,
            format!("the trie disagrees with the foldings at U+{cp:04X}"),
        ));
    }

    let mut out = Emit::new(
        "caseless_data",
        "baud-domain/build.rs (build/caseless.rs)",
        &[&cf],
    );
    out.code(&format!(
        "/// The trie's block size, as a shift: `codepoint >> CASELESS_SHIFT` names\n\
         /// a block in [`CASELESS_INDEX`].\n\
         pub const CASELESS_SHIFT: u32 = {SHIFT};"
    ));
    out.array(
        "CASELESS_INDEX",
        "u8",
        &[&format!(
            "Stage 1: `codepoint >> {SHIFT}` to one of {} distinct blocks.",
            trie.distinct
        )],
        &trie.index,
        PerLine(24),
        u8::to_string,
    );
    out.array(
        "CASELESS_BLOCKS",
        "u16",
        &[
            &format!(
                "Stage 2: {} blocks of {}, each entry an index into",
                trie.distinct,
                1 << SHIFT
            ),
            "[`CASELESS_POOL`], 0 for the code point itself.",
        ],
        &trie.blocks,
        PerLine(16),
        u16::to_string,
    );
    out.array(
        "CASELESS_POOL",
        "u32",
        &["The foldings: at an index, a length and then that many code points."],
        &pool,
        PerLine(10),
        |v| format!("0x{v:04X}"),
    );
    errs.or(out)
}
