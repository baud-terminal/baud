//! The DEC 94-character sets baud translates beyond DEC Special Graphics
//! and the national replacement sets, `OUT_DIR/dec_sets_data.rs`.
//!
//! Each table is 94 codepoints, indexed by `byte - 0x21`, 2/1 through
//! 7/14, so the put path indexes it and never searches. A set's every
//! position is in the table: a defined position holds its character and a
//! reserved one DEC's error character, so nothing here falls back to the
//! byte.
//!
//! The standard is DEC's text and xterm's `charsets.h` is practice
//! (`[[source: xterm-charsets]]`). The build reads xterm's macros and the
//! constants below, each typed from a DEC text with its citation; it never
//! reads DEC's pages. Every cell is held to DEC's own tables by the tests:
//! `tests/build_tables.rs` runs [`named_exactly`] over Table 2-3 as
//! `tests/vt220_tables.tsv` transcribes it, so the cells where xterm's
//! macro and DEC's text differ are exactly the ones [`CONFORM`] and
//! [`XTERM_DEPARTS`] name, and xterm cannot drift silently.
//!
//! * **DEC Supplemental** (`ESC ( <` and `ESC ( % 5`): xterm's
//!   `map_DEC_Supp_Graphic`, under xterm's licence (X11), its 81 glyphs
//!   and its 13 `UNDEF` cells, which must be [`FIGURE_A2_RESERVED`], the
//!   cells `[[source: dec-pplv2-pm]]` Figure A-2 hatches.
//!   Where the macro departs from DEC's text, DEC's text is the standard
//!   and the cell is DEC's, listed in [`CONFORM`]: one cell, 5/15.
//! * **DEC Hebrew Supplemental** (`ESC ( " 4`): `[[source: xterm-charsets]]`'
//!   `map_DEC_Hebrew_Supp`, its 49 glyphs and its 45 `UNDEF` cells, which
//!   must be [`FIGURE_A22_RESERVED`], the cells `[[source: dec-pplv2-pm]]`
//!   Figure A-22 shades. The figure keeps DEC Supplemental's ¤, ª and º at
//!   2/8, 2/10 and 3/10, where the macro has ¨, × and ÷: three cells in
//!   [`CONFORM`].
//! * **DEC Greek Supplemental** (`ESC ( " ?`): `[[source: xterm-charsets]]`'
//!   `map_DEC_Greek_Supp`, its 81 glyphs and its 13 `UNDEF` cells, which
//!   must be [`FIGURE_A24_RESERVED`], the cells `[[source: dec-pplv2-pm]]`
//!   Figure A-24 shades. The macro and the figure agree at every cell.
//! * **JIS-Katakana** (`ESC ( I`): `[[source: dec-pplv2-pm]]` §A.12,
//!   "specified by JIS X 0201-1976. There are 63 characters defined",
//!   2/1-5/15, the rest reserved; the characters from
//!   `[[source: unicode-jis0201]]`, read here, 0xA1-0xDF.
//! * **JIS-Roman** (`ESC ( J`): `[[source: vt382-rm]]` §2.4.5, ASCII but
//!   5/12 the Yen sign and 7/14 the over line. 7/14 has no codepoint
//!   given yet, so it is listed in [`OPEN`] and stays `~` until
//!   answered.
//!
//! A reserved position draws `DEC_ERROR_CHARACTER`: "The error character
//! is printed when one of these reserved characters is received by the
//! device" (`[[source: dec-pplv2-pm]]` §A.2, §A.8, §A.9, §A.12), and on a video
//! terminal `[[source: dec-std-070]]` 070-3 §3.4.3 says "The character
//! code 10/0 in 8-bit environments is treated as an unassigned graphic
//! character code, and will cause the error character (usually imaged as
//! a REVERSE QUESTION MARK) to be displayed on conforming devices" (by
//! analogy, inferred), as `[[source: vt382-rm]]` §2.4.6 says of
//! JIS-Katakana's reserved codes.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use crate::xterm::{self, Cur, Digits, Line};
use baud_tablegen::{Check, Emit, Errors, Input, Kind, Source};
use std::collections::HashMap;
use std::fmt::Write as _;

pub const JIS0201: Input = Input {
    file: "JIS0201.TXT",
    key: "unicode-jis0201",
    header: "# JIS0201.TXT",
};

/// The files, under `data/`, that [`build`] and `build/dec_graphics.rs`
/// read.
pub const INPUTS: [Input; 2] = [xterm::CHARSETS, JIS0201];

/// What xterm's `UNDEF` and a reserved position draw: DEC's error
/// character, U+2426, as `DEC_ERROR_CHARACTER` holds it. The tables write
/// the constant, `E`, and never this number.
pub const ERROR: u32 = 0x2426;

/// A set this module or `build/dec_graphics.rs` builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Set {
    Graphics,
    Supplemental,
    JisKatakana,
    JisRoman,
    Hebrew,
    Greek,
}

/// A position a DEC text defines and no answer has given a codepoint yet:
/// it draws its byte until the question is answered.
#[derive(Clone, Copy)]
pub struct Open {
    pub set: Set,
    pub byte: u8,
    pub why: &'static [&'static str],
}

/// A cell taken from xterm's macro where the macro departs from DEC's
/// text: the codepoint DEC's text gives, which the table takes because the
/// text is the standard, and why.
#[derive(Clone, Copy)]
pub struct Conform {
    pub set: Set,
    pub byte: u8,
    pub dec: u32,
    pub why: &'static [&'static str],
}

/// Where xterm's macro differs from a table built from DEC's text: what
/// xterm draws, and why the table does not.
#[derive(Clone, Copy)]
pub struct Departs {
    pub set: Set,
    pub byte: u8,
    pub xterm: u32,
    pub why: &'static [&'static str],
}

/// `[[source: dec-pplv2-pm]]` Figure A-2 (PDF p. 182), its hatched cells,
/// read by stroke detection from the hash-checked research copy: DEC
/// Supplemental's reserved positions, as a second DEC text
/// draws them.
pub const FIGURE_A2_RESERVED: [u8; 13] = [
    0x24, 0x26, 0x2C, 0x2D, 0x2E, 0x2F, 0x34, 0x38, 0x3E, 0x50, 0x5E, 0x70, 0x7E,
];

/// `[[source: dec-pplv2-pm]]` Figure A-22, "DEC Hebrew Supplemental
/// Character Set", its shaded cells, which §A.8 says print the error
/// character: Figure A-2's in columns 2 and 3, all of columns 4 and 5, and
/// 7/11-7/14.
pub const FIGURE_A22_RESERVED: [u8; 45] = [
    0x24, 0x26, 0x2C, 0x2D, 0x2E, 0x2F, 0x34, 0x38, 0x3E, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46,
    0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56,
    0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x5F, 0x7B, 0x7C, 0x7D, 0x7E,
];

/// `[[source: dec-pplv2-pm]]` Figure A-24, "DEC Greek Supplemental
/// Character Set", its shaded cells, which §A.9 says print the error
/// character: the cells Figure A-2 hatches.
pub const FIGURE_A24_RESERVED: [u8; 13] = FIGURE_A2_RESERVED;

/// `[[source: dec-pplv2-pm]]` §A.12: 63 defined, 2/1-5/15; 6/0-7/14
/// reserved, as `[[source: vt382-rm]]` Figure 2-10 draws them in GR (note
/// AF §3.4).
pub const KATAKANA_DEFINED: std::ops::RangeInclusive<u8> = 0x21..=0x5F;

/// `[[source: vt382-rm]]` §2.4.5, p. 2-17 [img]: "This set differs from
/// the ASCII Character Set only in that the Yen sign(¥)replaces the
/// backslash(\\) in column 5, row 12 (5/12) and the over line (¯) replaces
/// the tilde (˜) in column 7, row 14 (7/14)."
pub const JIS_ROMAN_YEN: (u8, u32) = (0x5C, 0x00A5);

/// How many positions of DEC Supplemental, DEC Hebrew Supplemental, DEC
/// Greek Supplemental and JIS-Katakana are defined.
pub const SUPPLEMENTAL_DEFINED: usize = 81;
pub const HEBREW_DEFINED: usize = 49;
pub const GREEK_DEFINED: usize = 81;
pub const KATAKANA_COUNT: usize = 63;

/// A set read from its macro and held to a DEC figure: the macro's `UNDEF`
/// cells must be the figure's shaded ones, its defined positions number
/// `defined`, and a cell [`CONFORM`] names for the set takes DEC's
/// codepoint.
pub struct Charted {
    pub set: Set,
    pub name: &'static str,
    pub figure: &'static str,
    pub reserved: &'static [u8],
    pub defined: usize,
}

/// The sets [`Charted`] describes.
pub const CHARTED: [Charted; 3] = [
    Charted {
        set: Set::Supplemental,
        name: "DEC Supplemental",
        figure: "Figure A-2",
        reserved: &FIGURE_A2_RESERVED,
        defined: SUPPLEMENTAL_DEFINED,
    },
    Charted {
        set: Set::Hebrew,
        name: "DEC Hebrew Supplemental",
        figure: "Figure A-22",
        reserved: &FIGURE_A22_RESERVED,
        defined: HEBREW_DEFINED,
    },
    Charted {
        set: Set::Greek,
        name: "DEC Greek Supplemental",
        figure: "Figure A-24",
        reserved: &FIGURE_A24_RESERVED,
        defined: GREEK_DEFINED,
    },
];

/// The positions open until their question is answered.
pub const OPEN: [Open; 1] = [Open {
    set: Set::JisRoman,
    byte: 0x7E,
    why: &[
        "Q-JIS-Roman (note AF): `[[source: vt382-rm]]` §2.4.5 names it the",
        "\"over line\" and `[[source: dec-pplv2-pm]]` Table A-2 \"Macron\";",
        "U+203E or U+00AF",
    ],
}];

/// The cells taken from xterm's macro where the macro departs from DEC's
/// text. `named_exactly` fails unless these and [`XTERM_DEPARTS`] are
/// exactly the cells where the macro and DEC's text differ.
pub const CONFORM: [Conform; 5] = [
    Conform {
        set: Set::Supplemental,
        byte: 0x5F,
        dec: 0x00DF,
        why: &[
            "xterm's `UNI(0x5F, 0x005F)` draws `_`; DEC's Table 2-3,",
            "`[[source: vt320-uu-appe]]` and `[[source: dec-pplv2-pm]]` Figure A-2",
            "all have ß, so the table has ß (note AF §3.1)",
        ],
    },
    Conform {
        set: Set::Hebrew,
        byte: 0x28,
        dec: 0x00A4,
        why: &[
            "xterm's `UNI(0x28, 0x00A8)` draws ¨; `[[source: dec-pplv2-pm]]`",
            "Figure A-22 has ¤, as DEC Supplemental does, so the table has ¤",
        ],
    },
    Conform {
        set: Set::Hebrew,
        byte: 0x2A,
        dec: 0x00AA,
        why: &[
            "xterm's `UNI(0x2A, 0x00D7)` draws ×; `[[source: dec-pplv2-pm]]`",
            "Figure A-22 has ª, as DEC Supplemental does, so the table has ª",
        ],
    },
    Conform {
        set: Set::Hebrew,
        byte: 0x3A,
        dec: 0x00BA,
        why: &[
            "xterm's `UNI(0x3A, 0x00F7)` draws ÷; `[[source: dec-pplv2-pm]]`",
            "Figure A-22 has º, as DEC Supplemental does, so the table has º",
        ],
    },
    Conform {
        set: Set::Graphics,
        byte: 0x5F,
        dec: 0x0020,
        why: &[
            "the macro leaves it `UNDEF`; DEC names it \"Blank\"",
            "(`[[source: vt100-ug-ch3]]` Table 3-9) and gives no codepoint, and",
            "`[[source: dec-std-070]]` §3.4.1 gives SPACE \"the visual",
            "representation of the absence of a graphic symbol\", so the table",
            "has U+0020: a drawn Blank shows as an erased cell and copies as a space",
        ],
    },
];

/// Where the macro differs from a table built from DEC's text.
pub const XTERM_DEPARTS: [Departs; 2] = [
    Departs {
        set: Set::JisKatakana,
        byte: 0x7E,
        xterm: 0x007E,
        why: &[
            "xterm's macro lists no 0x7E and so draws `~`; DEC reserves 7/14",
            "(§A.12; `[[source: vt382-rm]]` Figure 2-10)",
        ],
    },
    Departs {
        set: Set::JisRoman,
        byte: 0x7E,
        xterm: 0x203E,
        why: &["xterm's `UNI(0x7E, 0x203E)` is one answer to Q-JIS-Roman, open"],
    },
];

/// One of xterm's code-page macros, `begin_CODEPAGE(94)` around a
/// `switch`: each `UNI(byte, codepoint)` and `XXX(byte, UNDEF)` entry, and
/// whether a `default:` arm stands for the bytes it does not list.
pub struct CodePage {
    pub entries: HashMap<u8, u32>,
    pub default: bool,
}

/// The lines a code-page macro holds besides its entries.
const STRUCTURE: [&str; 6] = [
    "",
    "switch (code) {",
    "}",
    "} \\",
    "begin_CODEPAGE(94)",
    "end_CODEPAGE()",
];

/// The first `#define <name>(` with a body, read as a [`CodePage`]: any
/// line that is not one of its entries, a `default:` arm or [`STRUCTURE`]
/// is refused, as is a byte given twice.
pub fn code_page(src: &Source, name: &str, digits: Digits, errs: &mut Errors) -> CodePage {
    let mut page = CodePage {
        entries: HashMap::new(),
        default: false,
    };
    let Some((_, body)) = xterm::bodies(src, name).into_iter().next() else {
        errs.push(src.error(Kind::Malformed, format!("no {name} with a body")));
        return page;
    };
    for line in body {
        let text = line.text();
        let entry = uni(text, digits).or_else(|| undef(text, digits));
        if let Some((byte, cp)) = entry {
            if page.entries.insert(byte, cp).is_some() {
                errs.push(line.error(
                    src.path(),
                    Kind::Duplicate,
                    format!("{name}: 0x{byte:02X} twice"),
                ));
            }
        } else if text.starts_with("default:") {
            page.default = true;
        } else if !STRUCTURE.contains(&text) {
            errs.push(line.error(
                src.path(),
                Kind::Malformed,
                format!("{name}: an entry the build cannot read"),
            ));
        }
    }
    page
}

/// `UNI(0xHH, 0xHHHH)`, then a comment or nothing.
fn uni(text: &str, digits: Digits) -> Option<(u8, u32)> {
    let mut c = Cur::new(text);
    if !c.lit("UNI(") {
        return None;
    }
    let byte = c.hex(2, 2, digits)?;
    if !c.lit(", ") {
        return None;
    }
    let cp = c.hex(4, 4, digits)?;
    if !c.lit(")") {
        return None;
    }
    c.end()?;
    Some((u8::try_from(byte).ok()?, cp))
}

/// `XXX(0xHH, UNDEF)`, then a comment or nothing: the error character.
fn undef(text: &str, digits: Digits) -> Option<(u8, u32)> {
    let mut c = Cur::new(text);
    if !c.lit("XXX(") {
        return None;
    }
    let byte = c.hex(2, 2, digits)?;
    if !c.lit(", UNDEF)") {
        return None;
    }
    c.end()?;
    Some((u8::try_from(byte).ok()?, ERROR))
}

/// What xterm draws at each of 2/1-7/14 through `page`: a listed byte its
/// entry; an unlisted one the default arm, `dft`, which is GR's Latin-1
/// code in the one macro that has it (`charsets.c:443` passes
/// `chr = eight`), and the byte itself in a macro without one.
pub fn cells(page: &CodePage) -> [u32; 94] {
    std::array::from_fn(|i| {
        let c = u8::try_from(0x21 + i).expect("a byte");
        match page.entries.get(&c) {
            Some(&cp) => cp,
            None if page.default => u32::from(c | 0x80),
            None => u32::from(c),
        }
    })
}

/// **The cells where xterm and DEC differ are exactly the cells named**,
/// for `set`: at each of 2/1-7/14, a cell in `conform` must be one where
/// `xterm` departs from `dec` and takes `dec`'s codepoint, a cell in
/// `departs` one where `xterm` departs and draws what the entry records,
/// and every other cell must be the same in both. `dec` is the table built
/// from DEC's text: the build passes its own, and the tests DEC's pages as
/// transcribed. Returns each cell that breaks the rule.
pub fn named_exactly(
    set: Set,
    xterm: &[u32; 94],
    dec: &[u32; 94],
    conform: &[Conform],
    departs: &[Departs],
) -> Vec<String> {
    let mut out = Vec::new();
    for (i, (&x, &d)) in xterm.iter().zip(dec).enumerate() {
        let byte = u8::try_from(0x21 + i).expect("a byte");
        let at = format!("{set:?} 0x{byte:02X}");
        let c = conform.iter().find(|c| c.set == set && c.byte == byte);
        let p = departs.iter().find(|p| p.set == set && p.byte == byte);
        match (c, p) {
            (Some(_), Some(_)) => out.push(format!("{at}: named in CONFORM and XTERM_DEPARTS")),
            (Some(c), None) if x == d || c.dec != d => out.push(format!(
                "{at}: CONFORM gives U+{:04X}, DEC's text U+{d:04X}, xterm U+{x:04X}",
                c.dec
            )),
            (None, Some(p)) if x == d || p.xterm != x => out.push(format!(
                "{at}: XTERM_DEPARTS records U+{:04X}, xterm U+{x:04X}, DEC's text U+{d:04X}",
                p.xterm
            )),
            (None, None) if x != d => out.push(format!(
                "{at}: xterm U+{x:04X}, DEC's text U+{d:04X}, and neither CONFORM nor XTERM_DEPARTS names it"
            )),
            _ => {}
        }
    }
    out
}

/// The macro each set is read from.
const MACROS: [(Set, &str); 5] = [
    (Set::Supplemental, "map_DEC_Supp_Graphic"),
    (Set::JisKatakana, "map_JIS_Katakana"),
    (Set::JisRoman, "map_JIS_Roman"),
    (Set::Hebrew, "map_DEC_Hebrew_Supp"),
    (Set::Greek, "map_DEC_Greek_Supp"),
];

/// JIS0201.TXT's mappings, byte to codepoint: every data line is
/// `0xHH\t0xHHHH\t#`, the rest of it the name.
fn jis0201(src: &Source, errs: &mut Errors) -> HashMap<u8, u32> {
    let mut out = HashMap::new();
    for (i, line) in src.text().lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let row = Line {
            n: i + 1,
            raw: line,
        };
        let mut c = Cur::new(line);
        let read = (|| {
            let b = c.hex(2, 2, Digits::Upper)?;
            c.lit("\t").then_some(())?;
            let cp = c.hex(4, 4, Digits::Upper)?;
            c.lit("\t#").then_some(())?;
            Some((u8::try_from(b).ok()?, cp))
        })();
        match read {
            Some((b, cp)) => {
                if out.insert(b, cp).is_some() {
                    errs.push(row.error(src.path(), Kind::Duplicate, format!("0x{b:02X} twice")));
                }
            }
            None => {
                errs.push(row.error(src.path(), Kind::Malformed, "a line the build cannot read"))
            }
        }
    }
    out
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let charsets = errs.take(xterm::load(read));
    let jis_src = errs.take(Source::load(&JIS0201, read));
    let (Some(charsets), Some(jis_src)) = (charsets, jis_src) else {
        return Err(errs);
    };

    // JIS X 0201, through Unicode's mapping.
    let jis = jis0201(&jis_src, &mut errs);
    for c in KATAKANA_DEFINED {
        errs.require(Check::from(jis.contains_key(&(c | 0x80))), || {
            jis_src.error(
                Kind::Control,
                format!(
                    "JIS-Katakana 0x{c:02X}: JIS0201.TXT maps no 0x{:02X}",
                    c | 0x80
                ),
            )
        });
    }
    let katakana: [u32; 94] = std::array::from_fn(|i| {
        let c = u8::try_from(0x21 + i).expect("a byte");
        if KATAKANA_DEFINED.contains(&c) {
            jis.get(&(c | 0x80)).copied().unwrap_or(ERROR)
        } else {
            ERROR
        }
    });
    let defined = katakana.iter().filter(|&&v| v != ERROR).count();
    errs.count(
        jis_src.path(),
        "JIS-Katakana positions defined",
        defined,
        KATAKANA_COUNT,
    );
    let (yen_byte, yen) = JIS_ROMAN_YEN;
    errs.require(Check::from(jis.get(&yen_byte) == Some(&yen)), || {
        jis_src.error(
            Kind::Control,
            format!(
                "JIS0201.TXT does not map 0x{yen_byte:02X} to U+{yen:04X}, which VT382 §2.4.5 names"
            ),
        )
    });
    let roman: [u32; 94] = std::array::from_fn(|i| {
        let c = u8::try_from(0x21 + i).expect("a byte");
        if c == yen_byte { yen } else { u32::from(c) }
    });

    // xterm's macros.
    let xterm: Vec<(Set, [u32; 94], CodePage)> = MACROS
        .iter()
        .map(|&(set, name)| {
            let page = code_page(&charsets, name, Digits::Upper, &mut errs);
            (set, cells(&page), page)
        })
        .collect();
    if !errs.is_empty() {
        return Err(errs);
    }
    let of = |set: Set| xterm.iter().find(|(s, ..)| *s == set).expect("a macro");

    // The charted sets: each set's macro, where DEC's text is the standard
    // and the macro departs from it.
    let mut charted: Vec<(Set, [u32; 94])> = Vec::new();
    for c in &CHARTED {
        let (_, x, page) = of(c.set);
        let macro_name = MACROS
            .iter()
            .find(|(s, _)| *s == c.set)
            .map_or("", |(_, n)| *n);
        let mut undef: Vec<u8> = page
            .entries
            .iter()
            .filter(|(_, cp)| **cp == ERROR)
            .map(|(b, _)| *b)
            .collect();
        undef.sort_unstable();
        errs.require(Check::from(undef == c.reserved), || {
            charsets.error(
                Kind::Control,
                format!(
                    "{macro_name}'s UNDEF cells {undef:02X?} are not {}'s reserved ones {:02X?}",
                    c.figure, c.reserved
                ),
            )
        });
        let table: [u32; 94] = std::array::from_fn(|i| {
            let byte = u8::try_from(0x21 + i).expect("a byte");
            CONFORM
                .iter()
                .find(|k| k.set == c.set && k.byte == byte)
                .map_or(x[i], |k| k.dec)
        });
        let defined = table.iter().filter(|&&v| v != ERROR).count();
        errs.count(
            charsets.path(),
            &format!("{} positions defined", c.name),
            defined,
            c.defined,
        );
        charted.push((c.set, table));
    }
    let table_of = |set: Set| {
        charted
            .iter()
            .find(|(s, _)| *s == set)
            .map(|(_, t)| *t)
            .expect("a charted set")
    };

    let tables = [
        (Set::Supplemental, table_of(Set::Supplemental)),
        (Set::JisKatakana, katakana),
        (Set::JisRoman, roman),
        (Set::Hebrew, table_of(Set::Hebrew)),
        (Set::Greek, table_of(Set::Greek)),
    ];
    for (set, table) in &tables {
        let (_, x, _) = of(*set);
        for e in named_exactly(*set, x, table, &CONFORM, &XTERM_DEPARTS) {
            errs.push(charsets.error(Kind::Control, e));
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    let mut out = Emit::new(
        "dec_sets_data",
        "crates/baud-domain/build.rs (build/dec_sets.rs)",
        &[&charsets, &jis_src],
    );
    out.code(
        "use crate::{Codepoint, DEC_ERROR_CHARACTER};\n\n\
         /// DEC's error character, at a reserved position.\n\
         const E: Codepoint = DEC_ERROR_CHARACTER;\n",
    );
    for (set, table) in &tables {
        out.code(&table_rs(*set, table));
    }
    Ok(out)
}

/// The doc lines of each table, before its open, conformed and departing
/// cells.
fn doc(set: Set) -> (&'static str, &'static [&'static str]) {
    match set {
        Set::Supplemental => (
            "DEC_SUPPLEMENTAL",
            &[
                "DEC Supplemental, `ESC ( <` and `ESC ( % 5`: `[[source: xterm-charsets]]`'",
                "`map_DEC_Supp_Graphic`, its 81 glyphs and, in its 13 `UNDEF` cells, the",
                "error character, the 13 cells `[[source: dec-pplv2-pm]]` Figure A-2",
                "hatches and §A.2 says print the error character; held cell for cell to",
                "`[[source: dec-special-graphics]]` Table 2-3's right half by the tests.",
            ],
        ),
        Set::JisKatakana => (
            "JIS_KATAKANA",
            &[
                "JIS-Katakana, `ESC ( I`: 2/1-5/15 are JIS X 0201's katakana, which",
                "`[[source: dec-pplv2-pm]]` §A.12 names, through `[[source: unicode-jis0201]]`;",
                "6/0-7/14 are reserved and draw the error character (§A.12;",
                "`[[source: vt382-rm]]` §2.4.6 and Figure 2-10).",
            ],
        ),
        Set::JisRoman => (
            "JIS_ROMAN",
            &[
                "JIS-Roman, `ESC ( J`: ASCII with 5/12 the Yen sign (`[[source: vt382-rm]]`",
                "§2.4.5; `[[source: unicode-jis0201]]`).",
            ],
        ),
        Set::Hebrew => (
            "DEC_HEBREW_SUPPLEMENTAL",
            &[
                "DEC Hebrew Supplemental, `ESC ( \" 4`: `[[source: dec-pplv2-pm]]` Figure A-22,",
                "through `[[source: xterm-charsets]]`' `map_DEC_Hebrew_Supp`: columns 2 and 3",
                "DEC Supplemental's, 6/0-7/10 the 27 letters U+05D0-U+05EA, and in the 45",
                "cells the figure shades the error character, as §A.8 says.",
            ],
        ),
        Set::Greek => (
            "DEC_GREEK_SUPPLEMENTAL",
            &[
                "DEC Greek Supplemental, `ESC ( \" ?`: `[[source: dec-pplv2-pm]]` Figure A-24,",
                "through `[[source: xterm-charsets]]`' `map_DEC_Greek_Supp`: columns 2 and 3",
                "DEC Supplemental's, columns 4-7 the Greek letters, and in the 13 cells the",
                "figure shades the error character, as §A.9 says.",
            ],
        ),
        Set::Graphics => ("DEC_GRAPHICS", &[]),
    }
}

/// `/// * \`0xhh\`: why` for each cell, the reason's lines indented under
/// it and the last ending in a full stop.
pub fn cell_docs(rs: &mut String, title: &str, cells: &[(u8, &[&str])]) {
    if cells.is_empty() {
        return;
    }
    let _ = writeln!(rs, "///\n/// {title}");
    for (b, why) in cells {
        let _ = writeln!(rs, "///");
        for (i, w) in why.iter().enumerate() {
            let lead = if i == 0 {
                format!("* `0x{b:02x}`: ")
            } else {
                "  ".to_string()
            };
            let end = if i + 1 == why.len() { "." } else { "" };
            let _ = writeln!(rs, "/// {lead}{w}{end}");
        }
    }
}

/// The open, conformed and departing cells of `set`, as doc lines.
pub fn named_docs(rs: &mut String, set: Set) {
    let open: Vec<(u8, &[&str])> = OPEN
        .iter()
        .filter(|o| o.set == set)
        .map(|o| (o.byte, o.why))
        .collect();
    let conform: Vec<(u8, &[&str])> = CONFORM
        .iter()
        .filter(|c| c.set == set)
        .map(|c| (c.byte, c.why))
        .collect();
    let departs: Vec<(u8, &[&str])> = XTERM_DEPARTS
        .iter()
        .filter(|d| d.set == set)
        .map(|d| (d.byte, d.why))
        .collect();
    cell_docs(rs, "Open, drawing its byte until answered:", &open);
    cell_docs(
        rs,
        "Where the macro departs from DEC's text, the standard, the cell is DEC's:",
        &conform,
    );
    cell_docs(
        rs,
        "Where `[[source: xterm-charsets]]`' macro, practice, differs:",
        &departs,
    );
}

fn table_rs(set: Set, table: &[u32; 94]) -> String {
    let (ident, lines) = doc(set);
    let mut rs = String::new();
    for l in lines {
        let _ = writeln!(rs, "/// {l}");
    }
    named_docs(&mut rs, set);
    let _ = writeln!(rs, "pub(crate) static {ident}: [Codepoint; 94] = [");
    for (i, &v) in table.iter().enumerate() {
        let c = 0x21 + i;
        if v == ERROR {
            let _ = writeln!(rs, "    E, // 0x{c:02x} reserved");
        } else {
            let _ = writeln!(rs, "    Codepoint(0x{v:04x}), // 0x{c:02x}");
        }
    }
    rs.push_str("];\n");
    rs
}
