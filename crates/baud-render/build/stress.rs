//! The stress mark's tables, `OUT_DIR/stress.rs`: the codepoints `SGR
//! 64`'s mark skips, by `[[source: css-text-decor-3]]`'s rule for the
//! web's emphasis marks, which "are not drawn for":
//!
//! * "Word separators or other characters that belong to the Unicode
//!   separator classes (Z*)";
//! * "Punctuation--specifically, any characters that belong to the
//!   Unicode P* general category and do not NFKD normalize [UAX15] to any
//!   of the following symbols": # % U+2030 U+2031 U+066A U+0609 U+060A &
//!   U+204A @ U+00A7 U+00B6 U+204B U+2053 U+303D;
//! * "Characters belonging to the Unicode classes for control codes and
//!   unassigned characters (Cc, Cf, Cn)".
//!
//! A typographic character unit is classed by its base character, and the
//! Z* bullet's own exception -- "emphasis marks are drawn for a space that
//! combines with any combining characters" -- is the one way a cluster
//! whose base is skipped takes the mark. A combining character is gc M*
//! (`[[source: unicode17-ch3]]` D52), so the separators and the combining
//! characters are compiled beside the skip set.
//!
//! The general category is `[[source: gc-data]]`'s, which lists every
//! codepoint, Cn included. NFKD of one codepoint is its compatibility
//! decomposition (`[[source: unicode17-ch3]]` D65, D119): the mappings of
//! `[[source: unicode-data]]` field 5, tagged and untagged, applied until
//! nothing decomposes; no P* character is a Hangul syllable, and
//! reordering moves nothing in a decomposition to one symbol.
//!
//! Each set is a two-stage bit trie: stage 1 maps a codepoint's high bits
//! (`cp >> 8`) to a block, stage 2 holds each distinct 256-bit block as 32
//! bytes, low bit first, numbered in the order they are first seen. The
//! skip trie's two byte files are also what `baud-present-macos`'s build
//! writes into the cell shader, read from this build's `OUT_DIR`.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use baud_tablegen::{
    CODE_POINTS, Check, CodepointMap, Emit, Error, Errors, Input, Kind, PerLine, Source, Trie,
    ValueSet,
};
use std::collections::HashMap;

pub const ALIASES: Input = Input {
    file: "unicode-propertyvaluealiases.txt",
    key: "unicode-propertyvaluealiases",
    header: "# PropertyValueAliases-17.0.0.txt",
};

pub const GENERAL_CATEGORY: Input = Input {
    file: "unicode-derivedgeneralcategory.txt",
    key: "gc-data",
    header: "# DerivedGeneralCategory-17.0.0.txt",
};

pub const UNICODE_DATA: Input = Input {
    file: "unicode-unicodedata.txt",
    key: "unicode-data",
    header: "0000;<control>;Cc;0;BN;;;;;N;NULL;;;;",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 3] = [ALIASES, GENERAL_CATEGORY, UNICODE_DATA];

/// The `gc` short names, in PropertyValueAliases.txt's order.
const GC: [&str; 38] = [
    "C", "Cc", "Cf", "Cn", "Co", "Cs", "L", "LC", "Ll", "Lm", "Lo", "Lt", "Lu", "M", "Mc", "Me",
    "Mn", "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps", "S", "Sc", "Sk",
    "Sm", "So", "Z", "Zl", "Zp", "Zs",
];

/// The `gc` values that name a group of categories (UAX #44,
/// `[[source: uax44]]`), which no code point has as its own.
const GC_GROUPS: [&str; 8] = ["C", "L", "LC", "M", "N", "P", "S", "Z"];

/// CSS's exceptions: punctuation that NFKD-normalizes to one of these
/// keeps the mark.
pub const KEEPS: [u32; 15] = [
    0x0023, 0x0025, 0x2030, 0x2031, 0x066A, 0x0609, 0x060A, 0x0026, 0x204A, 0x0040, 0x00A7, 0x00B6,
    0x204B, 0x2053, 0x303D,
];

/// Positive controls, each a reading of the rule: a space, an ideographic
/// space, the paragraph separator, `!`, `,`, an ideographic comma, a
/// bullet, the soft hyphen, an unassigned codepoint and BEL are skipped.
pub const SKIPPED: [u32; 10] = [
    0x20, 0x3000, 0x2029, 0x21, 0x2C, 0x3001, 0x2022, 0xAD, 0x0378, 0x0007,
];

/// And a letter, a Han character, `#`, `@`, the fullwidth and small `#`,
/// a private-use character and an emoji take the mark.
pub const MARKED: [u32; 8] = [0x61, 0x4E2D, 0x23, 0x40, 0xFF03, 0xFE5F, 0xE000, 0x1F600];

/// An acute, a Hebrew point, a Devanagari vowel sign, an enclosing circle
/// and VS16 are combining (Mn, Mc, Me).
pub const COMBINING: [u32; 5] = [0x301, 0x5B4, 0x93E, 0x20DD, 0xFE0F];

/// ZWJ, ZWNJ, a halfwidth voiced mark, an emoji modifier, a tag and a
/// letter are not.
pub const NOT_COMBINING: [u32; 6] = [0x200D, 0x200C, 0xFF9E, 0x1F3FB, 0xE0041, 0x61];

/// The distinct blocks of the skip trie and of the combining trie, and the
/// separators, in the pinned files: positive-control counts, which a data
/// upgrade fails on purpose.
pub const SKIP_BLOCKS: usize = 147;
pub const COMBINING_BLOCKS: usize = 75;
pub const SEPARATORS: usize = 19;

/// The trie's block: 256 codepoints, 32 bytes of bits.
const SHIFT: u32 = 5;

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([aliases, gcs, ud]) = <[Source; 3]>::try_from(loaded) else {
        return Err(errs);
    };
    let values = match ValueSet::aliases(&aliases, "gc", &GC) {
        Ok(v) => v,
        Err(e) => {
            errs.extend(e);
            return Err(errs);
        }
    };
    let Some(gc) = general_category(&gcs, &values, &mut errs) else {
        return Err(errs);
    };
    let dec = decompositions(&ud, &mut errs);
    let class = |cp: u32| GC[usize::from(gc[at(cp)])];

    let skip: Vec<bool> = (0..CODE_POINTS)
        .map(|i| {
            let cp = u32::try_from(i).expect("a code point");
            let c = class(cp);
            if c.starts_with('Z') || matches!(c, "Cc" | "Cf" | "Cn") {
                true
            } else if c.starts_with('P') {
                let mut n = Vec::new();
                nfkd(cp, &dec, &mut n);
                !(n.len() == 1 && KEEPS.contains(&n[0]))
            } else {
                false
            }
        })
        .collect();
    let combining: Vec<bool> = (0..CODE_POINTS)
        .map(|i| class(u32::try_from(i).expect("a code point")).starts_with('M'))
        .collect();
    let separators: Vec<u32> = (0..=baud_tablegen::LAST)
        .filter(|&cp| class(cp).starts_with('Z'))
        .collect();

    for (cps, set, want, what) in [
        (&SKIPPED[..], &skip, true, "skipped"),
        (&MARKED, &skip, false, "skipped"),
        (&COMBINING, &combining, true, "combining"),
        (&NOT_COMBINING, &combining, false, "combining"),
    ] {
        for &cp in cps {
            let got = set[at(cp)];
            errs.require(Check::from(got == want), || {
                gcs.error(
                    Kind::Control,
                    format!("U+{cp:04X} is {what}: {got}, {want} expected"),
                )
            });
        }
    }
    errs.count(gcs.path(), "separators", separators.len(), SEPARATORS);
    let skips = errs.take(bit_trie("STRESS_SKIP", &skip));
    let marks = errs.take(bit_trie("COMBINING", &combining));
    let (Some(skips), Some(marks)) = (skips, marks) else {
        return Err(errs);
    };
    errs.count(
        gcs.path(),
        "stress skip blocks",
        skips.distinct,
        SKIP_BLOCKS,
    );
    errs.count(
        gcs.path(),
        "combining blocks",
        marks.distinct,
        COMBINING_BLOCKS,
    );

    let mut out = Emit::new(
        "stress",
        "crates/baud-render/build.rs (build/stress.rs)",
        &[&aliases, &gcs, &ud],
    );
    out.bytes(
        "STRESS_SKIP_INDEX",
        &["Stage 1: codepoint >> 8 to a block number, `0x1100` entries."],
        &skips.index,
    );
    let doc = format!(
        "Stage 2: {} distinct 256-codepoint blocks, 32 bytes each,",
        skips.distinct
    );
    out.bytes(
        "STRESS_SKIP_BLOCKS",
        &[
            &doc,
            "a set bit for a codepoint the mark skips, low bit first.",
        ],
        &skips.blocks,
    );
    out.array(
        "SEPARATORS",
        "u32",
        &[
            "The separator classes (Z*), ascending: a cluster on one of these",
            "takes the mark when a combining character attaches.",
        ],
        &separators,
        PerLine(12),
        |c| format!("0x{c:04X}"),
    );
    out.bytes(
        "COMBINING_INDEX",
        &["Stage 1 of the combining characters (gc M*, D52), as above."],
        &marks.index,
    );
    let doc = format!(
        "Stage 2: {} distinct blocks, a set bit for a combining",
        marks.distinct
    );
    out.bytes("COMBINING_BLOCKS", &[&doc, "character."], &marks.blocks);
    errs.or(out)
}

/// Every code point's General_Category, as its place in [`GC`]. The file
/// lists every code point, Cn included, so a code point it leaves out is
/// refused rather than defaulted, and so is a row giving a group.
fn general_category(src: &Source, gc: &ValueSet, errs: &mut Errors) -> Option<Vec<u8>> {
    let mut map = CodepointMap::<u8>::new();
    for row in src.rows() {
        let r = errs.take(row.expect(2).and_then(|()| row.range(0)));
        let v = errs.take(gc.check(&row, 1));
        let (Some(r), Some(v)) = (r, v) else {
            continue;
        };
        if GC_GROUPS.contains(&row.field(1)) {
            errs.push(row.error(
                Kind::UnknownValue,
                1,
                format!(
                    "gc `{}` is a group of categories, not a code point's",
                    row.field(1)
                ),
            ));
            continue;
        }
        errs.take(map.assign(src, &row, r, u8::try_from(v).expect("38 values")));
    }
    errs.take(map.complete(src.path()))
}

/// UnicodeData.txt's decomposition mappings, field 5, canonical and
/// compatibility alike: the tag of a compatibility mapping is dropped.
fn decompositions(ud: &Source, errs: &mut Errors) -> HashMap<u32, Vec<u32>> {
    let mut dec = HashMap::new();
    for entry in ud.unicode_data() {
        let Some(e) = errs.take(entry) else {
            continue;
        };
        let d = e.row.field(5);
        if d.is_empty() {
            continue;
        }
        let from = if d.starts_with('<') {
            d.find('>').map_or(d.len(), |i| i + 1)
        } else {
            0
        };
        if let Some(to) = errs.take(e.row.code_points(5, from)) {
            dec.insert(e.range.lo, to);
        }
    }
    dec
}

/// `cp`'s full compatibility decomposition, appended to `out`.
fn nfkd(cp: u32, dec: &HashMap<u32, Vec<u32>>, out: &mut Vec<u32>) {
    match dec.get(&cp) {
        Some(to) => to.iter().for_each(|&c| nfkd(c, dec, out)),
        None => out.push(cp),
    }
}

/// `set`, a flag per code point, as a bit trie of 256-codepoint blocks,
/// read back through both stages at every code point before it is
/// returned.
pub fn bit_trie(name: &str, set: &[bool]) -> Result<Trie<u8>, Error> {
    let bytes: Vec<u8> = set
        .chunks(8)
        .map(|c| {
            c.iter()
                .enumerate()
                .fold(0u8, |b, (i, &on)| b | (u8::from(on) << i))
        })
        .collect();
    let trie = Trie::build(name, &bytes, SHIFT)?;
    let wrong = (0..set.len()).find(|&i| {
        trie.get(i >> 3, SHIFT)
            .map(|byte| (byte >> (i & 7)) & 1 == 1)
            != Some(set[i])
    });
    match wrong {
        Some(i) => Err(Error::new(
            Kind::Invariant,
            name,
            format!("the bit trie disagrees with its set at {i:#X}"),
        )),
        None => Ok(trie),
    }
}

fn at(cp: u32) -> usize {
    usize::try_from(cp).unwrap_or(usize::MAX)
}
