//! The national replacement tables against DEC's own VT220 tables.
//!
//! `[[source: dec-special-graphics]]` is the vt100.net transcription of the
//! VT220 Programmer Reference Manual's chapter 2, and Tables 2-1 to 2-15 in it
//! are inline SVG: each glyph a `<text class="chname">` placed by x and y,
//! followed by a `<text class="chnums">` carrying its code, and each
//! substituted cell shaded by a `<rect fill="#ddd">`.
//! `scripts/extract-vt220-tables.py` fetches the page by its pinned URL,
//! refuses it unless it hashes to the manifest's SHA-256, reads each glyph's
//! cell from its geometry, fails unless that cell is the code printed after
//! it, and writes every cell of columns 2-7 with whether it is shaded to
//! `vt220_tables.tsv`, with Table 4-4's finals from `[[source: vt220-rm-ch4]]`.
//! This file reads that table and shares no code with
//! `scripts/gen-nrcs-tables.py`, which writes baud's tables from xterm's: the
//! two tests that cited the tables before compared baud with itself, and
//! neither could see Swedish 4/0 go missing (note AF §2.3, D2).
//!
//! **It compares tables. It never renders through `Term`.** Whether a
//! designated national set acts outside national mode was note AF's
//! Q-DECNRCM, decided (c) on 2026-10-24: it does. Gates of that kind are
//! built in `Term`, after `Charset::from_final`, which takes the byte and
//! nothing else -- so the content of a set and the set a final names are
//! what this file checks.
use baud_domain::{Charset, DEC_ERROR_CHARACTER, Nrcs, StreamByte, nrcs_table, uk_table};
use std::collections::{BTreeMap, BTreeSet};

const TABLES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/vt220_tables.tsv"
));
const UNICODE_DATA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/research/sources/unicode-unicodedata.txt"
));

/// The VT220's national sets, by DEC's name as Table 4-4 and each table's
/// caption spell it, with the table and the set baud names for it.
///
/// **Norwegian/Danish, Table 2-12, is extracted and not asserted.** Its finals
/// `E` and `6` name the DEC repertoire here (Ä Ü ä ü), and the VT420 and
/// EK-PPLV2-PM chart an ISO one under `` ` ``; which repertoire each final
/// names is note AF's Q-ND-backtick, the owner's (§2.6). Asserting Table 2-12
/// against what `E` reaches would build in two of its three options.
const VT220: [(&str, &str, Charset); 10] = [
    ("British", "T2-5", Charset::Uk),
    ("Dutch", "T2-6", Charset::National(Nrcs::Dutch)),
    ("Finnish", "T2-7", Charset::National(Nrcs::Finnish)),
    ("French", "T2-8", Charset::National(Nrcs::French)),
    (
        "French Canadian",
        "T2-9",
        Charset::National(Nrcs::FrenchCanadian),
    ),
    ("German", "T2-10", Charset::National(Nrcs::German)),
    ("Italian", "T2-11", Charset::National(Nrcs::Italian)),
    ("Spanish", "T2-13", Charset::National(Nrcs::Spanish)),
    ("Swedish", "T2-14", Charset::National(Nrcs::Swedish)),
    ("Swiss", "T2-15", Charset::National(Nrcs::Swiss)),
];

/// Tables 2-5 to 2-15 and the number of cells each shades, in order: UK,
/// Dutch, Finnish, French, French Canadian, German, Italian,
/// Norwegian/Danish, Spanish, Swedish, Swiss (note AF §1).
const SHADED: [(&str, usize); 11] = [
    ("T2-5", 1),
    ("T2-6", 9),
    ("T2-7", 9),
    ("T2-8", 9),
    ("T2-9", 10),
    ("T2-10", 8),
    ("T2-11", 10),
    ("T2-12", 10),
    ("T2-13", 8),
    ("T2-14", 10),
    ("T2-15", 12),
];

/// **Where the transcription is not DEC's printed table:** Dutch 7/12-7/14,
/// as (position, what the transcription draws, what the test expects).
/// Note AF §2.5.
///
/// * 7/12: the VT220 table, printed and transcribed alike, draws `f`. The test
///   expects ƒ, U+0192: `[[source: dec-std-070]]` p. 7-20 gives the florin as
///   a Dutch character of older devices with fallback presentation `f`
///   [img], and `[[source: dec-pplv2-pm]]` Figure A-17 draws ƒ, its NOTE
///   reading "7/12 Florin Sign (ƒ)", approximated as "Lowercase f (f)"
///   [img]. Not a transcription error: the VT220's `f` is read as the
///   florin's fallback, resolved (inferred).
/// * 7/13: the transcription has `£`; DEC has ¼, U+00BC
///   (`[[source: vt220-pocket-guide]]` p. 8 [img]; `[[source: dec-std-070]]`
///   p. 6-109, "one-qrtr (7/13)" [img]). A transcription error.
/// * 7/14: the transcription has `¼`; DEC has ´, U+00B4
///   (`[[source: vt220-pocket-guide]]` p. 8 [img]; `[[source: dec-pplv2-pm]]`
///   Table A-2, "Acute accent", and Figure A-17). A transcription error: its
///   7/14 is the 7/13 value shifted down a row.
const DUTCH: [(u8, &str, char); 3] = [
    (0x7c, "f", '\u{0192}'),
    (0x7d, "£", '\u{00bc}'),
    (0x7e, "¼", '\u{00b4}'),
];

/// One extracted table: the glyph names by code, columns 2-7, and the shaded
/// cells.
struct DecTable {
    caption: String,
    glyphs: BTreeMap<u8, String>,
    shaded: BTreeSet<u8>,
}

/// The tab-separated fields of each `vt220_tables.tsv` line of `kind`.
fn rows(kind: &str) -> impl Iterator<Item = Vec<&'static str>> + '_ {
    TABLES
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(move |f| f[0] == kind)
}

/// One table as `vt220_tables.tsv` holds it: its caption, its glyph at each
/// code of columns 2-7, and its shaded cells.
fn extract(id: &str) -> DecTable {
    let mut captions = rows("caption").filter(|f| f[1] == id);
    let caption = captions
        .next()
        .unwrap_or_else(|| panic!("{id}: no caption"))[2]
        .to_string();
    assert!(captions.next().is_none(), "{id}: two captions");
    let mut glyphs = BTreeMap::new();
    let mut shaded = BTreeSet::new();
    for f in rows("cell").filter(|f| f[1] == id) {
        assert_eq!(f.len(), 5, "{id}: a cell line of {} fields", f.len());
        let code = u8::from_str_radix(f[2], 16).expect("hex");
        assert!(
            glyphs.insert(code, f[3].to_string()).is_none(),
            "{id}: two glyphs at {code:#04x}"
        );
        match f[4] {
            "1" => {
                shaded.insert(code);
            }
            "0" => {}
            other => panic!("{id} {code:#04x}: shaded is {other:?}"),
        }
    }
    DecTable {
        caption,
        glyphs,
        shaded,
    }
}

/// Positions 2/1-7/14 that differ from ASCII.
fn differing(t: &DecTable) -> BTreeSet<u8> {
    (0x21u8..=0x7e)
        .filter(|b| t.glyphs[b] != char::from(*b).to_string())
        .collect()
}

/// A table, read through both of note AF §1's controls: 96 cells, `SP` at
/// 2/0 and `DEL` at 7/15, and every differing cell shaded and every shaded
/// cell differing.
fn checked(id: &str) -> DecTable {
    let t = extract(id);
    assert_eq!(t.glyphs.len(), 96, "{id}: cells extracted");
    assert_eq!(t.glyphs[&0x20], "SP", "{id}: 2/0");
    assert_eq!(t.glyphs[&0x7f], "DEL", "{id}: 7/15");
    assert_eq!(
        differing(&t),
        t.shaded,
        "{id}: the cells that differ from ASCII are not the cells shaded"
    );
    t
}

/// What a byte is under a baud table: listed, or itself (`with_nrcs`).
fn baud(table: &[(u8, u32)], b: u8) -> u32 {
    table
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(u32::from(b), |(_, cp)| *cp)
}

fn baud_table(set: Charset) -> &'static [(u8, u32)] {
    match set {
        Charset::Uk => uk_table(),
        Charset::National(n) => nrcs_table(n),
        other => panic!("{other:?} has no replacement table"),
    }
}

/// **The extraction is itself checked.** Table 2-1, ASCII, reads back as
/// ASCII with nothing shaded, and each national table shades exactly the
/// cells that differ, as many as note AF §1 counted.
#[test]
fn the_vt220_tables_extract_with_both_controls() {
    let ascii = checked("T2-1");
    assert!(differing(&ascii).is_empty(), "Table 2-1 differs from ASCII");
    for (id, n) in SHADED {
        let t = checked(id);
        assert_eq!(t.shaded.len(), n, "{id}: shaded cells");
    }
}

/// **Content, by set: each VT220 national table in baud is DEC's.**
///
/// Over 2/1-7/14, for the ten sets in `VT220`, UK included. Red at Swedish
/// 4/0, which baud drew as `@` where Table 2-14 shades É.
#[test]
fn each_vt220_national_table_is_decs() {
    let mut wrong = Vec::new();
    for (name, id, set) in VT220 {
        let dec = checked(id);
        assert!(
            dec.caption
                .starts_with(&format!("Table {} {name} NRC Set", &id[1..])),
            "{id}'s caption is {:?}, not {name}'s",
            dec.caption
        );
        let table = baud_table(set);
        for (k, _) in table {
            assert!(
                (0x21..=0x7e).contains(k),
                "{name}: baud lists {k:#04x}, outside the 2/1-7/14 compared"
            );
        }
        for b in 0x21u8..=0x7e {
            let drawn = &dec.glyphs[&b];
            let want = match DUTCH
                .iter()
                .find(|(p, ..)| set == Charset::National(Nrcs::Dutch) && *p == b)
            {
                Some((_, transcribed, decs)) => {
                    assert_eq!(
                        drawn, transcribed,
                        "Dutch {b:#04x}: the transcription no longer reads {transcribed}, so \
                         the exception recorded for it is stale"
                    );
                    *decs
                }
                None => {
                    let mut cs = drawn.chars();
                    match (cs.next(), cs.next()) {
                        (Some(c), None) => c,
                        _ => panic!("{name} {b:#04x}: {drawn:?} is not one character"),
                    }
                }
            };
            let got = baud(table, b);
            if got != u32::from(want) {
                wrong.push(format!(
                    "{name} {}/{}: DEC {want} U+{:04X}, baud U+{got:04X}",
                    b >> 4,
                    b & 15,
                    u32::from(want)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Table 4-4's national sets: DEC's name, and the finals printed beside it,
/// which the extraction checked against the column/row printed above each.
fn table_4_4() -> Vec<(String, Vec<u8>)> {
    rows("final")
        .map(|f| (f[1].to_string(), f[2].as_bytes().to_vec()))
        .collect()
}

/// **Finals, by byte: each final VT220 Table 4-4 lists names the set whose
/// table `each_vt220_national_table_is_decs` checked.**
///
/// `Charset::from_final` takes the byte and nothing else -- no mode, no level
/// -- so a gate either open question would add sits after it, and this
/// asserts names only. `E` and `6` wait on Q-ND-backtick with Table 2-12.
/// `f`, which no DEC table lists, is never sent: it waits on note AF's
/// Q-French-f and Q-Unlisted.
#[test]
fn each_vt220_national_final_names_the_set_checked() {
    let listed = table_4_4();
    let names: Vec<&str> = listed.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "British",
            "Dutch",
            "Finnish",
            "French",
            "French Canadian",
            "German",
            "Italian",
            "Norwegian/Danish",
            "Spanish",
            "Swedish",
            "Swiss"
        ],
        "Table 4-4's national sets"
    );
    let mut asserted = Vec::new();
    for (name, finals) in &listed {
        if name == "Norwegian/Danish" {
            assert_eq!(finals, b"E6", "Norwegian/Danish's finals");
            continue;
        }
        let (_, _, set) = VT220
            .iter()
            .find(|(n, ..)| n == name)
            .unwrap_or_else(|| panic!("{name} has no table here"));
        for f in finals {
            assert_eq!(
                Charset::from_final(StreamByte::from_stream(*f)),
                *set,
                "ESC ( {} must name {name}",
                char::from(*f)
            );
            asserted.push(*f);
        }
    }
    assert_eq!(asserted, b"A4C5RQKYZH7=", "the finals asserted");
}

/// The name `[[source: unicode-data]]` gives a scalar value, read from the
/// pinned file rather than typed here.
fn unicode_name(v: u32) -> Option<&'static str> {
    let key = format!("{v:04X};");
    UNICODE_DATA
        .lines()
        .find(|l| l.starts_with(&key))
        .and_then(|l| l.split(';').nth(1))
}

/// DEC's error character is stored as U+2426 SYMBOL FOR SUBSTITUTE FORM TWO
/// (note AE's AE8.1, decided by Jev at 1.00 twice, the second asking with the
/// options reordered). DEC names no codepoint: 070-3 §3.4.3 has the character
/// "usually imaged as a REVERSE QUESTION MARK", and U+2426 is ISO 2047's
/// picture of SUB, the control DEC ties it to, with a reversed question mark
/// for its reference glyph (`[[source: unicode18-chart-2400]]`, note AF
/// Q-UNDEF).
///
/// The value is checked by its name in the pinned database, so a constant
/// moved to any of the three other options asked -- U+FFFD, which departs
/// from DEC's description by name and by glyph, U+2E2E or U+2592 -- goes red
/// here, not only in review.
/// The `FFFD` lookup is the positive control: a probe that could find no
/// line at all would read as a mismatch.
#[test]
fn the_dec_error_character_is_symbol_for_substitute_form_two() {
    assert_eq!(
        unicode_name(0xFFFD),
        Some("REPLACEMENT CHARACTER"),
        "the positive control: the pinned database must be searchable"
    );
    assert_eq!(
        unicode_name(DEC_ERROR_CHARACTER.value()),
        Some("SYMBOL FOR SUBSTITUTE FORM TWO"),
        "DEC's error character is U+2426 (note AE's AE8.1), not U+{:04X}",
        DEC_ERROR_CHARACTER.value()
    );
}
