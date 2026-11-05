//! The Indic tables, `OUT_DIR/indic_data.rs`: each character's class in an
//! Indic syllable and its position, which the Indic shaper reads
//! (`crate::indic`), from IndicSyllabicCategory.txt
//! (`[[source: unicode-indicsyllabiccategory]]`),
//! IndicPositionalCategory.txt (`[[source: unicode-indicpositionalcategory]]`)
//! and Blocks.txt (`[[source: unicode-blocks]]`), Unicode 17.0.0.
//!
//! **The mapping is HarfBuzz's.** The Unicode categories are finer than
//! the classes the Indic syllable grammar is written in (Microsoft's
//! "Developing OpenType Fonts for Devanagari Script",
//! `[[source: ms-devanagari-script-development]]`: C, V, N, H, ZWNJ, ZWJ,
//! M, SM, VD, A and NBSP), and a shaper has to say which class each one
//! is. HarfBuzz 14.5.1's gen-indic-table.py
//! (`[[source: harfbuzz-14-5-1-gen-indic-table]]`) says it -- its
//! category_map, position_map, its overrides and its matra positions per
//! script, re-expressed here for the nine scripts its Indic shaper takes
//! (Khmer and Myanmar, which it shapes elsewhere, are left out, and so are
//! the variation selectors it classes for Myanmar alone). The fixtures in
//! `tests/shape` are hb-shape's output, so the classes have to be the ones
//! it shaped with.
//!
//! A class and a position are each a byte, the numbers of
//! `crate::syllabic`'s `Category` and `Position`. A pure function of its
//! inputs: `main` reads them and writes what [`build`] returns.

use baud_tablegen::{Check, CpRange, Emit, Errors, Input, Kind, Source};
use std::collections::{BTreeMap, BTreeSet};

pub const SYLLABIC_CATEGORY: Input = Input {
    file: "unicode-indicsyllabiccategory.txt",
    key: "unicode-indicsyllabiccategory",
    header: "# IndicSyllabicCategory-17.0.0.txt",
};

pub const POSITIONAL_CATEGORY: Input = Input {
    file: "unicode-indicpositionalcategory.txt",
    key: "unicode-indicpositionalcategory",
    header: "# IndicPositionalCategory-17.0.0.txt",
};

pub const BLOCKS_TXT: Input = Input {
    file: "unicode-blocks.txt",
    key: "unicode-blocks",
    header: "# Blocks-17.0.0.txt",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 3] = [SYLLABIC_CATEGORY, POSITIONAL_CATEGORY, BLOCKS_TXT];

/// The shaper's classes, in the order of `syllabic::Category`.
pub const CATEGORIES: [&str; 20] = [
    "X",
    "C",
    "V",
    "N",
    "H",
    "ZWNJ",
    "ZWJ",
    "M",
    "SM",
    "A",
    "PLACEHOLDER",
    "DOTTEDCIRCLE",
    "RS",
    "MPst",
    "Repha",
    "Ra",
    "CM",
    "Symbol",
    "CS",
    "SMPst",
];

/// The positions in a syllable, left to right, in the order of
/// `syllabic::Position`.
pub const POSITIONS: [&str; 15] = [
    "START",
    "RA_TO_BECOME_REPH",
    "PRE_M",
    "PRE_C",
    "BASE_C",
    "AFTER_MAIN",
    "ABOVE_C",
    "BEFORE_SUB",
    "BELOW_C",
    "AFTER_SUB",
    "BEFORE_POST",
    "POST_C",
    "AFTER_POST",
    "SMVD",
    "END",
];

/// The blocks whose characters the shaper classes.
pub const BLOCKS: [&str; 15] = [
    "Basic Latin",
    "Latin-1 Supplement",
    "Devanagari",
    "Bengali",
    "Gurmukhi",
    "Gujarati",
    "Oriya",
    "Tamil",
    "Telugu",
    "Kannada",
    "Malayalam",
    "Vedic Extensions",
    "General Punctuation",
    "Superscripts and Subscripts",
    "Devanagari Extended",
];

/// And two single characters: NO-BREAK SPACE and DOTTED CIRCLE.
const SINGLES: [u32; 2] = [0x00A0, 0x25CC];

/// HarfBuzz's category_map: each Indic_Syllabic_Category the shaper's
/// class. A value of a character in [`BLOCKS`] that is not here stops the
/// build.
pub const CATEGORY_MAP: [(&str, &str); 36] = [
    ("Other", "X"),
    ("Avagraha", "Symbol"),
    ("Bindu", "SM"),
    ("Brahmi_Joining_Number", "PLACEHOLDER"),
    ("Cantillation_Mark", "A"),
    ("Consonant", "C"),
    ("Consonant_Dead", "C"),
    ("Consonant_Final", "CM"),
    ("Consonant_Head_Letter", "C"),
    ("Consonant_Initial_Postfixed", "C"),
    ("Consonant_Killer", "M"),
    ("Consonant_Medial", "CM"),
    ("Consonant_Placeholder", "PLACEHOLDER"),
    ("Consonant_Preceding_Repha", "Repha"),
    ("Consonant_Prefixed", "X"),
    ("Consonant_Subjoined", "CM"),
    ("Consonant_Succeeding_Repha", "CM"),
    ("Consonant_With_Stacker", "CS"),
    ("Gemination_Mark", "SM"),
    ("Invisible_Stacker", "H"),
    ("Joiner", "ZWJ"),
    ("Modifying_Letter", "X"),
    ("Non_Joiner", "ZWNJ"),
    ("Nukta", "N"),
    ("Number", "PLACEHOLDER"),
    ("Number_Joiner", "PLACEHOLDER"),
    ("Pure_Killer", "M"),
    ("Register_Shifter", "RS"),
    ("Syllable_Modifier", "SM"),
    ("Tone_Letter", "X"),
    ("Tone_Mark", "N"),
    ("Virama", "H"),
    ("Visarga", "SM"),
    ("Vowel", "V"),
    ("Vowel_Dependent", "M"),
    ("Vowel_Independent", "V"),
];

/// HarfBuzz's position_map: each Indic_Positional_Category the position
/// it starts from.
pub const POSITION_MAP: [(&str, &str); 15] = [
    ("Not_Applicable", "END"),
    ("Left", "PRE_C"),
    ("Top", "ABOVE_C"),
    ("Bottom", "BELOW_C"),
    ("Right", "POST_C"),
    ("Bottom_And_Right", "POST_C"),
    ("Left_And_Right", "POST_C"),
    ("Top_And_Bottom", "BELOW_C"),
    ("Top_And_Bottom_And_Left", "BELOW_C"),
    ("Top_And_Bottom_And_Right", "POST_C"),
    ("Top_And_Left", "ABOVE_C"),
    ("Top_And_Left_And_Right", "POST_C"),
    ("Top_And_Right", "POST_C"),
    ("Overstruck", "AFTER_MAIN"),
    ("Visual_order_left", "PRE_M"),
];

/// HarfBuzz's category overrides, applied over the mapped classes.
pub const CATEGORY_OVERRIDES: [(u32, &str); 55] = [
    (0x2015, "PLACEHOLDER"),
    (0x2022, "PLACEHOLDER"),
    (0x25FB, "PLACEHOLDER"),
    (0x25FC, "PLACEHOLDER"),
    (0x25FD, "PLACEHOLDER"),
    (0x25FE, "PLACEHOLDER"),
    (0x0930, "Ra"),
    (0x09B0, "Ra"),
    (0x09F0, "Ra"),
    (0x0A30, "Ra"),
    (0x0AB0, "Ra"),
    (0x0B30, "Ra"),
    (0x0BB0, "Ra"),
    (0x0C30, "Ra"),
    (0x0CB0, "Ra"),
    (0x0D30, "Ra"),
    (0x0953, "SM"),
    (0x0954, "SM"),
    (0x0A40, "MPst"),
    (0x0A72, "C"),
    (0x0A73, "C"),
    (0x1CE2, "A"),
    (0x1CE3, "A"),
    (0x1CE4, "A"),
    (0x1CE5, "A"),
    (0x1CE6, "A"),
    (0x1CE7, "A"),
    (0x1CE8, "A"),
    (0x1CED, "A"),
    (0xA8F2, "Symbol"),
    (0xA8F3, "Symbol"),
    (0xA8F4, "Symbol"),
    (0xA8F5, "Symbol"),
    (0xA8F6, "Symbol"),
    (0xA8F7, "Symbol"),
    (0x1CE9, "Symbol"),
    (0x1CEA, "Symbol"),
    (0x1CEB, "Symbol"),
    (0x1CEC, "Symbol"),
    (0x1CEE, "Symbol"),
    (0x1CEF, "Symbol"),
    (0x1CF0, "Symbol"),
    (0x1CF1, "Symbol"),
    (0x0A51, "M"),
    (0x11301, "SM"),
    (0x11302, "SM"),
    (0x11303, "SM"),
    (0x1133B, "N"),
    (0x1133C, "N"),
    (0x0AFB, "N"),
    (0x0B55, "N"),
    (0x09FC, "PLACEHOLDER"),
    (0x0C80, "PLACEHOLDER"),
    (0x0D04, "PLACEHOLDER"),
    (0x25CC, "DOTTEDCIRCLE"),
];

/// HarfBuzz's position overrides, applied last.
pub const POSITION_OVERRIDES: [(u32, &str); 2] = [(0x0A51, "BELOW_C"), (0x0B01, "BEFORE_SUB")];

/// The classes that keep the position their character's category gives.
const POSITIONED: [&str; 6] = ["CM", "SM", "RS", "H", "M", "MPst"];
/// The classes that sit at the base.
const CONSONANTS: [&str; 7] = ["C", "CS", "Ra", "CM", "V", "PLACEHOLDER", "DOTTEDCIRCLE"];
/// The classes whose position is the matra's, by script.
const MATRAS: [&str; 2] = ["M", "MPst"];
/// The classes that go with the syllable modifiers and Vedic signs.
const SMVD: [&str; 5] = ["SM", "SMPst", "VD", "A", "Symbol"];

/// The first of the dense table's characters, and the end of its range:
/// U+0900..U+0D7F, the nine scripts' blocks.
pub const DENSE: (u32, u32) = (0x0900, 0x0D80);

/// The counts the tables were compiled with: classed characters in the
/// dense table and in Latin-1, the sparse table's records, and the blocks
/// they are in.
const COUNTS: [(&str, usize); 4] = [
    ("classed characters in U+0900..U+0D7F", 798),
    ("classed characters in U+0000..U+00FF", 15),
    ("sparse records", 91),
    ("sparse blocks", 5),
];

/// Positive controls, each a character and the class and position the
/// tables give it: KA a consonant at the base, VOWEL SIGN I a matra before
/// it, RA the letter that can become reph, NO-BREAK SPACE and DOTTED
/// CIRCLE bases, a Latin letter unclassed.
pub const CONTROLS: [(u32, &str, &str); 6] = [
    (0x0915, "C", "BASE_C"),
    (0x093F, "M", "PRE_M"),
    (0x0930, "Ra", "BASE_C"),
    (0x00A0, "PLACEHOLDER", "BASE_C"),
    (0x25CC, "DOTTEDCIRCLE", "BASE_C"),
    (0x0041, "X", "END"),
];

/// Where a dependent vowel sign goes, by script: HarfBuzz's
/// matra_pos_left/right/top/bottom. `None` for a matra at a position none
/// of them takes.
fn matra_position(u: u32, pos: &str, block: &str) -> Option<&'static str> {
    Some(match pos {
        "PRE_C" => "PRE_M",
        "POST_C" => match block {
            "Devanagari" => "AFTER_SUB",
            "Bengali" | "Gurmukhi" | "Gujarati" | "Oriya" | "Tamil" | "Malayalam" => "AFTER_POST",
            "Telugu" if u <= 0x0C42 => "BEFORE_SUB",
            "Kannada" if !(0x0CC3..=0x0CD6).contains(&u) => "BEFORE_SUB",
            _ => "AFTER_SUB",
        },
        "ABOVE_C" => match block {
            "Gurmukhi" => "AFTER_POST",
            "Oriya" => "AFTER_MAIN",
            "Telugu" | "Kannada" => "BEFORE_SUB",
            _ => "AFTER_SUB",
        },
        "BELOW_C" => match block {
            "Gurmukhi" | "Gujarati" | "Tamil" | "Malayalam" => "AFTER_POST",
            "Telugu" | "Kannada" => "BEFORE_SUB",
            _ => "AFTER_SUB",
        },
        _ => return None,
    })
}

/// A value a file gives a code point, and where.
#[derive(Clone, Copy, Debug)]
struct Listed<'a> {
    value: &'a str,
    line: usize,
    col: usize,
}

/// Each code point `src` lists and the value it gives it; a code point two
/// rows list is refused.
fn listed<'a>(src: &'a Source, errs: &mut Errors) -> BTreeMap<u32, Listed<'a>> {
    let mut out: BTreeMap<u32, Listed<'a>> = BTreeMap::new();
    for row in src.rows() {
        let Some(r) = errs.take(row.expect(2).and_then(|()| row.range(0))) else {
            continue;
        };
        if let Some(cp) = r.iter().find(|cp| out.contains_key(cp)) {
            let kind = if r.is_one() {
                Kind::Duplicate
            } else {
                Kind::Overlap
            };
            errs.push(row.error(
                kind,
                0,
                format!("U+{cp:04X} is already given on line {}", out[&cp].line),
            ));
            continue;
        }
        let col = row.get(1).map_or(1, |f| f.col);
        for cp in r.iter() {
            out.insert(
                cp,
                Listed {
                    value: row.field(1),
                    line: row.line(),
                    col,
                },
            );
        }
    }
    out
}

/// Blocks.txt's blocks, in order; two that overlap are refused.
fn blocks<'a>(src: &'a Source, errs: &mut Errors) -> Vec<(CpRange, &'a str)> {
    let mut out: Vec<(CpRange, &'a str)> = Vec::new();
    for row in src.rows() {
        let Some(r) = errs.take(row.expect(2).and_then(|()| row.range(0))) else {
            continue;
        };
        if let Some((prev, name)) = out.last().filter(|(p, _)| r.lo <= p.hi) {
            errs.push(row.error(Kind::Overlap, 0, format!("{r} overlaps {name}, {prev}")));
            continue;
        }
        out.push((r, row.field(1)));
    }
    out
}

fn block_of<'a>(blocks: &[(CpRange, &'a str)], cp: u32) -> &'a str {
    let i = blocks.partition_point(|(r, _)| r.hi < cp);
    match blocks.get(i) {
        Some((r, name)) if r.lo <= cp => name,
        _ => "No_Block",
    }
}

fn index(set: &[&str], name: &str) -> u8 {
    let i = set
        .iter()
        .position(|v| *v == name)
        .expect("a declared name");
    u8::try_from(i).expect("fewer than 256 values")
}

/// A value the map does not have, at the row that gives it.
fn unknown(src: &Source, at: Listed<'_>, what: &str) -> baud_tablegen::Error {
    src.error(
        Kind::UnknownValue,
        format!("{what} `{}` has no class in HarfBuzz's map", at.value),
    )
    .at(
        baud_tablegen::Pos {
            line: at.line,
            col: at.col,
        },
        src.line(at.line),
    )
}

/// Every classed character's class and position, by character.
pub fn classes(
    cats: &Source,
    poss: &Source,
    blks: &Source,
    errs: &mut Errors,
) -> BTreeMap<u32, (&'static str, &'static str)> {
    let cat_of = listed(cats, errs);
    let pos_of = listed(poss, errs);
    let blocks = blocks(blks, errs);
    for name in BLOCKS {
        errs.require(Check::from(blocks.iter().any(|(_, n)| *n == name)), || {
            blks.error(Kind::Control, format!("no block is named {name:?}"))
        });
    }
    let category = |cp: u32, errs: &mut Errors| -> Option<&'static str> {
        let Some(&at) = cat_of.get(&cp) else {
            return Some("X");
        };
        let found = CATEGORY_MAP.iter().find(|(k, _)| *k == at.value);
        if found.is_none() {
            errs.push(unknown(cats, at, "Indic_Syllabic_Category"));
        }
        found.map(|&(_, v)| v)
    };
    let position = |cp: u32, errs: &mut Errors| -> Option<(&'static str, bool)> {
        let Some(&at) = pos_of.get(&cp) else {
            return Some(("END", true));
        };
        let found = POSITION_MAP.iter().find(|(k, _)| *k == at.value);
        if found.is_none() {
            errs.push(unknown(poss, at, "Indic_Positional_Category"));
        }
        found.map(|&(k, v)| (v, k == "Not_Applicable"))
    };
    let all: BTreeSet<u32> = cat_of.keys().chain(pos_of.keys()).copied().collect();
    let mut data: BTreeMap<u32, (&'static str, &'static str)> = BTreeMap::new();
    for u in all {
        if !SINGLES.contains(&u) && !BLOCKS.contains(&block_of(&blocks, u)) {
            continue;
        }
        let (Some(cat), Some((pos, unpositioned))) = (category(u, errs), position(u, errs)) else {
            continue;
        };
        let cat = if cat == "SM" && unpositioned {
            "SMPst"
        } else {
            cat
        };
        data.insert(u, (cat, pos));
    }
    for (u, cat) in CATEGORY_OVERRIDES {
        let pos = data.get(&u).map_or("END", |&(_, p)| p);
        data.insert(u, (cat, pos));
    }
    for (&u, v) in data.iter_mut() {
        let (cat, mut pos) = *v;
        if !POSITIONED.contains(&cat) {
            pos = "END";
        }
        if CONSONANTS.contains(&cat) {
            pos = "BASE_C";
        } else if MATRAS.contains(&cat) {
            match matra_position(u, pos, block_of(&blocks, u)) {
                Some(p) => pos = p,
                None => errs.push(blks.error(
                    Kind::Invariant,
                    format!("U+{u:04X}: a matra at {pos}, where no script places one"),
                )),
            }
        } else if SMVD.contains(&cat) {
            pos = "SMVD";
        }
        *v = (cat, pos);
    }
    for (u, pos) in POSITION_OVERRIDES {
        let cat = data.get(&u).map_or("X", |&(c, _)| c);
        data.insert(u, (cat, pos));
    }
    data
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let loaded: Vec<Source> = INPUTS
        .iter()
        .filter_map(|i| errs.take(Source::load(i, read)))
        .collect();
    let Ok([cats, poss, blks]) = <[Source; 3]>::try_from(loaded) else {
        return Err(errs);
    };
    let data = classes(&cats, &poss, &blks, &mut errs);
    let unclassed = ("X", "END");
    let at = |u: u32| data.get(&u).copied().unwrap_or(unclassed);
    let code = |(c, p): (&str, &str)| [index(&CATEGORIES, c), index(&POSITIONS, p)];
    let (lo, hi) = DENSE;
    let dense: Vec<u8> = (lo..hi).flat_map(|u| code(at(u))).collect();
    let latin1: Vec<u8> = (0..0x100).flat_map(|u| code(at(u))).collect();
    let sparse: Vec<(u32, (&str, &str))> = data
        .iter()
        .filter(|&(&u, &v)| !(lo..hi).contains(&u) && u >= 0x100 && v != unclassed)
        .map(|(&u, &v)| (u, v))
        .collect();
    let sparse_blocks: Vec<u32> = sparse
        .iter()
        .map(|&(u, _)| u >> 8)
        .collect::<BTreeSet<u32>>()
        .into_iter()
        .collect();
    let classed = |r: std::ops::Range<u32>| r.filter(|&u| at(u) != unclassed).count();
    let got = [
        classed(lo..hi),
        classed(0..0x100),
        sparse.len(),
        sparse_blocks.len(),
    ];
    for ((what, want), got) in COUNTS.into_iter().zip(got) {
        errs.count(cats.path(), what, got, want);
    }
    for (u, c, p) in CONTROLS {
        let got = at(u);
        errs.require(Check::from(got == (c, p)), || {
            cats.error(
                Kind::Control,
                format!("U+{u:04X} is {got:?}, ({c:?}, {p:?}) expected"),
            )
        });
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut out = Emit::new(
        "indic_data",
        "baud-font/build.rs (build/indic.rs)",
        &[&cats, &poss, &blks],
    );
    out.code(&format!(
        "/// The first character of [`DENSE`]: U+{lo:04X}.\n\
         pub const DENSE_FIRST: u32 = 0x{lo:04X};"
    ));
    out.bytes(
        "DENSE",
        &[
            &format!(
                "The class and position of U+{lo:04X}..U+{:04X}, two bytes a",
                hi - 1
            ),
            "character, indexed by the character less [`DENSE_FIRST`].",
        ],
        &dense,
    );
    out.bytes(
        "LATIN1",
        &[
            "The class and position of U+0000..U+00FF, as [`DENSE`]'s: the",
            "digits, NO-BREAK SPACE and the superscripts two and three are classed.",
        ],
        &latin1,
    );
    out.array(
        "SPARSE_BLOCKS",
        "u32",
        &[
            "The 256-character blocks [`SPARSE`] holds characters of, each a",
            "character shifted right by eight, in ascending order: a character",
            "in none of them is unclassed without a search.",
        ],
        &sparse_blocks,
        8,
        |b| format!("0x{b:03X}"),
    );
    let records: Vec<u8> = sparse
        .iter()
        .flat_map(|&(u, v)| {
            let [_, a, b, c] = u.to_be_bytes();
            let [cat, pos] = code(v);
            [a, b, c, cat, pos]
        })
        .collect();
    out.bytes(
        "SPARSE",
        &[
            "Every other classed character, five bytes each: the character in",
            "three, big-endian, then its class and position; in ascending order.",
        ],
        &records,
    );
    errs.or(out)
}
