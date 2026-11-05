//! The DEC 94-character sets baud translates, against the DEC texts and the
//! mapping they name, at the data level.
//!
//! `build/dec_sets.rs` and `build/dec_graphics.rs` compile the tables from
//! xterm's macros and the constants typed from DEC's texts, and never read
//! DEC's pages. This file reads DEC's tables and the mappings they name
//! itself and shares no code with the build, so a cell the build mis-reads
//! cannot pass on both sides -- the shape that let Swedish 4/0 go missing
//! while two tests of the national tables stayed green (note AF §2.3).
//!
//! Table 2-3 is read from `vt220_tables.tsv`, the cells read out of the
//! VT220 manual's chapter 2, fetched by its pinned URL and refused unless it
//! hashed to the manifest's SHA-256, as the manifest's comment on
//! `[[source: dec-special-graphics]]` records.
//!
//! Rendering through `Term` is `baud-core/tests/vt100/charsets.rs`'s.
use baud_domain::{AmbiguousWidth, Codepoint, DEC_ERROR_CHARACTER, DEC_GRAPHICS, DecSet, Width};
use std::collections::BTreeMap;

const TABLES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/vt220_tables.tsv"
));
const FOOT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/research/sources/foot-terminal.c"
));
const JIS0201: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/research/sources/JIS0201.TXT"
));

/// `[[source: dec-pplv2-pm]]` Figure A-2's hatched cells, DEC Supplemental's
/// reserved positions as a second DEC text draws them (note AF §3.1), typed
/// here from the note and not taken from the generator.
const FIGURE_A2_RESERVED: [u8; 13] = [
    0x24, 0x26, 0x2c, 0x2d, 0x2e, 0x2f, 0x34, 0x38, 0x3e, 0x50, 0x5e, 0x70, 0x7e,
];

fn table(set: DecSet) -> &'static [Codepoint; 94] {
    set.table()
        .unwrap_or_else(|| panic!("{set:?} has no table"))
}

fn at(set: DecSet, byte: u8) -> u32 {
    table(set)[usize::from(byte - 0x21)].value()
}

/// Table 2-3's right half, by the code's low seven bits: the glyph a cell
/// draws, or `None` for a cell that draws nothing.
///
/// `vt220_tables.tsv` holds each GR cell as `cell`, `T2-3b`, the low seven
/// bits in hex, the glyph (empty where the table draws nothing) and a shaded
/// field this table does not use.
fn table_2_3_right() -> BTreeMap<u8, Option<char>> {
    let mut cells = BTreeMap::new();
    for line in TABLES.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 5 || f[0] != "cell" || f[1] != "T2-3b" {
            continue;
        }
        let code = u8::from_str_radix(f[2], 16).expect("hex");
        let mut chars = f[3].chars();
        let glyph = match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            (None, _) => None,
            _ => panic!("0x{code:02x}: {:?} is more than one character", f[3]),
        };
        assert!(cells.insert(code, glyph).is_none(), "0x{code:02x} twice");
    }
    cells
}

/// **DEC Supplemental is `[[source: dec-special-graphics]]` Table 2-3's right
/// half**, glyph for glyph, and the error character in each cell the table
/// leaves empty -- the 13 `[[source: dec-pplv2-pm]]` Figure A-2 hatches.
///
/// Both finals reach the one table: `<` (VT220 Table 4-4) and `% 5`
/// (`[[source: dec-std-070]]` p. 5-117).
///
/// Positive control: the extraction must find all 94 cells and exactly 81
/// glyphs, or it is reading something else and agreeing with nothing.
#[test]
fn dec_supplemental_is_table_2_3() {
    let dec = table_2_3_right();
    assert_eq!(dec.len(), 94, "every GR cell of Table 2-3 parsed");
    let glyphs = dec.values().filter(|g| g.is_some()).count();
    assert_eq!(glyphs, 81, "Table 2-3 draws 81 characters");
    let empty: Vec<u8> = dec
        .iter()
        .filter(|(_, g)| g.is_none())
        .map(|(c, _)| *c)
        .collect();
    assert_eq!(
        empty, FIGURE_A2_RESERVED,
        "the empty cells are Figure A-2's reserved ones"
    );
    for set in [DecSet::Supplemental, DecSet::SupplementalGraphics] {
        for (code, glyph) in &dec {
            let want = glyph.map_or(DEC_ERROR_CHARACTER.value(), u32::from);
            assert_eq!(at(set, *code), want, "{set:?} 0x{code:02x}");
        }
    }
    // The one cell where xterm's table is not DEC's (note AF §3.1).
    assert_eq!(at(DecSet::Supplemental, 0x5f), 0x00df, "5/15 is ß, not `_`");
}

/// **JIS-Katakana is JIS X 0201's katakana at 2/1-5/15**, through
/// `[[source: unicode-jis0201]]`'s 0xA1-0xDF, and the error character at the
/// 31 positions `[[source: dec-pplv2-pm]]` §A.12 reserves ("There are 63
/// characters defined in this character set").
#[test]
fn jis_katakana_is_jis_x_0201_and_reserves_columns_6_and_7() {
    let mut jis = BTreeMap::new();
    for line in JIS0201
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut f = line.split('\t');
        let byte = u8::from_str_radix(f.next().unwrap().trim_start_matches("0x"), 16).unwrap();
        let cp = u32::from_str_radix(f.next().unwrap().trim_start_matches("0x"), 16).unwrap();
        jis.insert(byte, cp);
    }
    assert_eq!(jis.len(), 158, "JIS0201.TXT's 158 mappings parsed");
    let mut defined = 0;
    for code in 0x21u8..=0x7e {
        let got = at(DecSet::JisKatakana, code);
        if code <= 0x5f {
            assert_eq!(
                got,
                jis[&(code | 0x80)],
                "0x{code:02x} is JIS X 0201's 0x{:02x}",
                code | 0x80
            );
            defined += 1;
        } else {
            assert_eq!(got, DEC_ERROR_CHARACTER.value(), "0x{code:02x} is reserved");
        }
    }
    assert_eq!(defined, 63);
    assert_eq!(
        at(DecSet::JisKatakana, 0x31),
        0xff71,
        "3/1 is HALFWIDTH KATAKANA LETTER A"
    );
}

/// **JIS-Roman is ASCII with the Yen sign at 5/12** (`[[source: vt382-rm]]`
/// §2.4.5). 7/14 is not asserted: "over line" or "Macron" is note AF's
/// Q-JIS-Roman, open, and the table keeps `~` there until it is answered.
#[test]
fn jis_roman_is_ascii_with_the_yen_sign() {
    for code in 0x21u8..=0x7d {
        let want = if code == 0x5c {
            0x00a5
        } else {
            u32::from(code)
        };
        assert_eq!(at(DecSet::JisRoman, code), want, "0x{code:02x}");
    }
}

/// **Every position a set translates to is one narrow cell**, by baud's own
/// width table: the bulk path writes every translated byte as
/// `Width::NARROW`, so a wide or zero-width codepoint here would desynchronise
/// the cursor from the cells.
#[test]
fn every_translated_position_is_one_narrow_cell() {
    for set in [DecSet::Supplemental, DecSet::JisKatakana, DecSet::JisRoman] {
        for (i, cp) in table(set).iter().enumerate() {
            assert!(
                Width::of(*cp, AmbiguousWidth::Narrow) == Width::NARROW,
                "{set:?} 0x{:02x}: U+{:04X}",
                i + 0x21,
                cp.value()
            );
        }
    }
}

/// The sets without a table, each for the reason on its `DecSet` variant.
#[test]
fn the_sets_baud_does_not_translate_have_no_table() {
    for set in [
        DecSet::Technical,
        DecSet::Hebrew,
        DecSet::Greek,
        DecSet::Turkish,
        DecSet::Cyrillic,
        DecSet::Undocumented,
    ] {
        assert!(set.table().is_none(), "{set:?}");
    }
}

/// **DEC Special Graphics is xterm's 31 choices, and foot's**: the table
/// the build compiles from `[[source: xterm-charsets]]`' macro is, codepoint
/// for codepoint, `[[source: foot-vt100-0]]`'s `vt100_0[]`, read here from
/// foot's source, `` ` `` through `~`. Two implementations' judgement of
/// what DEC's named glyphs are (`[[source: vt100-ug-ch3]]` Table 3-9 gives
/// no codepoint), and a change to either is red here.
///
/// Positive controls: foot's table is found and holds 31 characters, its
/// first the black diamond and its last the middle dot.
#[test]
fn dec_graphics_is_xterms_and_foots() {
    let at = FOOT
        .find("static const char32_t vt100_0[] = {")
        .expect("foot's vt100_0[]");
    let body = &FOOT[at..at + FOOT[at..].find("};").expect("its end")];
    let foot: Vec<u32> = body
        .split("U'")
        .skip(1)
        .map(|c| u32::from(c.chars().next().expect("a character")))
        .collect();
    assert_eq!(foot.len(), 31, "foot's vt100_0[] holds 31");
    assert_eq!((foot[0], foot[30]), (0x25c6, 0x00b7));
    assert_eq!(DEC_GRAPHICS.as_slice(), foot.as_slice());
}
