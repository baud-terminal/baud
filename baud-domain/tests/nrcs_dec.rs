//! The national replacement tables against DEC's own VT220 tables.
//!
//! `[[source: dec-special-graphics]]` is the vt100.net transcription of the
//! VT220 Programmer Reference Manual's chapter 2, and Tables 2-1 to 2-15 in it
//! are inline SVG: each glyph a `<text class="chname">` placed by x and y,
//! followed by a `<text class="chnums">` carrying its code, and each
//! substituted cell shaded by a `<rect fill="#ddd">`. This reads them at test
//! time and shares no code with `scripts/gen-nrcs-tables.py`, which writes
//! baud's tables from xterm's: the two tests that cited the tables before
//! compared baud with itself, and neither could see Swedish 4/0 go missing
//! (note AF §2.3, D2).
//!
//! **It compares tables. It never renders through `Term`.** Whether a
//! designated national set acts outside national mode is note AF's
//! Q-DECNRCM, open; whether a final takes effect at the level baud claims is
//! open too (§2.8). Both would be gates built in `Term`, after
//! `Charset::from_final`, which takes the byte and nothing else -- so the
//! content of a set and the set a final names are what can be checked now,
//! and all this file checks.
use baud_domain::{Charset, Nrcs, StreamByte, nrcs_table, uk_table};
use std::collections::{BTreeMap, BTreeSet};

const CHAPTER2: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/research/sources/vt220-rm-chapter2.html"
));
const CHAPTER4: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/research/sources/vt220-rm-chapter4.html"
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

/// The text between `<figure id="{id}">` and its `</figure>`.
fn figure(id: &str) -> &'static str {
    let open = format!("<figure id=\"{id}\">");
    let start = CHAPTER2
        .find(&open)
        .unwrap_or_else(|| panic!("{id}: no figure"));
    let rest = &CHAPTER2[start + open.len()..];
    &rest[..rest.find("</figure>").expect("figure closes")]
}

/// The value of `name="..."` in an element's attribute text.
fn attr<'a>(el: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let i = el.find(&key).unwrap_or_else(|| panic!("no {name} in {el}")) + key.len();
    &el[i..i + el[i..].find('"').expect("attribute closes")]
}

/// A coordinate in tenths, which every one in these figures is exact in.
fn tenths(s: &str) -> i64 {
    let v: f64 = s.parse().unwrap_or_else(|_| panic!("coordinate {s}"));
    let t = (v * 10.0).round();
    assert!(
        (v * 10.0 - t).abs() < 1e-6,
        "coordinate {s} is finer than tenths"
    );
    t as i64
}

/// The cell at `(x, y)` on a grid whose column 0, row 0 sits at
/// `(x0, y0)`, with columns 85 apart and rows 45 apart; `None` off the grid
/// or outside columns 2-7.
fn cell(x: i64, y: i64, x0: i64, y0: i64) -> Option<u8> {
    let (dx, dy) = (x - x0, y - y0);
    if dx < 0 || dy < 0 || dx % 850 != 0 || dy % 450 != 0 {
        return None;
    }
    let (col, row) = (dx / 850, dy / 450);
    ((2..=7).contains(&col) && row < 16).then(|| (col * 16 + row) as u8)
}

fn unescape(s: &str) -> String {
    let out = s
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    assert!(
        out == "&" || !out.contains('&'),
        "an entity this reader does not decode: {s}"
    );
    out
}

/// Every glyph placed by x and y, and every shaded cell.
///
/// A glyph sits at the centre of the 55-wide box its code shares the cell
/// with: column 0's glyphs at x 137.5, its boxes at x 110, row 0's glyphs at
/// y 127 and its boxes at y 100. The `chnums` after each glyph carries the
/// code as octal, decimal and hex, and the hex has to be the cell the
/// geometry placed it in -- the check on the geometry. Off-grid names (the
/// row and column labels, column 1's DC1 and DC3, the key's ESC) fall out.
fn extract(id: &str) -> DecTable {
    let fig = figure(id);
    let caption = fig
        .split("<figcaption>")
        .nth(1)
        .and_then(|s| s.split("</figcaption>").next())
        .expect("caption")
        .to_string();
    let mut glyphs = BTreeMap::new();
    let mut rest = fig;
    while let Some(i) = rest.find("<text class=\"chname\"") {
        rest = &rest[i..];
        let open = &rest[..rest.find('>').expect("tag closes")];
        let body = &rest[open.len() + 1..];
        let text = &body[..body.find("</text>").expect("text closes")];
        rest = &body[text.len()..];
        let Some(code) = cell(tenths(attr(open, "x")), tenths(attr(open, "y")), 1375, 1270) else {
            continue;
        };
        let after = rest["</text>".len()..].trim_start();
        assert!(
            after.starts_with("<text class=\"chnums\""),
            "{id}: the glyph at {code:#04x} has no code after it"
        );
        let hex = after
            .split("<tspan")
            .nth(3)
            .and_then(|s| s.split('>').nth(1))
            .and_then(|s| s.split('<').next())
            .expect("a third tspan");
        assert_eq!(
            u8::from_str_radix(hex, 16).ok(),
            Some(code),
            "{id}: the glyph placed at {code:#04x} carries code {hex}"
        );
        assert!(
            glyphs.insert(code, unescape(text)).is_none(),
            "{id}: two glyphs at {code:#04x}"
        );
    }
    let mut shaded = BTreeSet::new();
    let mut rest = fig;
    while let Some(i) = rest.find("<rect ") {
        rest = &rest[i..];
        let el = &rest[..rest.find("/>").expect("rect closes")];
        rest = &rest[el.len()..];
        if !el.contains("fill=\"#ddd\"") {
            continue;
        }
        let code = cell(tenths(attr(el, "x")), tenths(attr(el, "y")), 1100, 1000)
            .unwrap_or_else(|| panic!("{id}: a shaded box off the grid: {el}"));
        shaded.insert(code);
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

/// Table 4-4's national sets: DEC's name, and the finals with the column/row
/// each is printed beside.
fn table_4_4() -> Vec<(String, Vec<u8>)> {
    let start = CHAPTER4.find("<table id=\"T4-4\">").expect("Table 4-4");
    let table = &CHAPTER4[start..start + CHAPTER4[start..].find("</table>").unwrap()];
    let nrcs = table
        .find("National replacement character sets")
        .expect("the national sets' heading");
    let mut sets = Vec::new();
    for row in table[nrcs..].split("<tr>").skip(1) {
        let cells: Vec<&str> = row
            .split("<td")
            .skip(1)
            .map(|c| {
                let c = &c[c.find('>').unwrap() + 1..];
                c[..c.find("</td>").unwrap()].trim()
            })
            .collect();
        let Some(pre) = cells.iter().find_map(|c| c.strip_prefix("<pre>")) else {
            continue;
        };
        let name = cells
            .iter()
            .find(|c| !c.is_empty() && !c.starts_with("<pre>"))
            .expect("a set's name");
        let pre = unescape(pre.trim_end_matches("</pre>"));
        let mut lines = pre.lines();
        let codes: Vec<u8> = lines
            .next()
            .unwrap()
            .split_whitespace()
            .map(|cr| {
                let (c, r) = cr.split_once('/').expect("column/row");
                c.parse::<u8>().unwrap() * 16 + r.parse::<u8>().unwrap()
            })
            .collect();
        let finals: Vec<u8> = lines
            .next()
            .unwrap()
            .split_whitespace()
            .filter(|t| *t != "or")
            .map(|t| {
                assert_eq!(t.len(), 1, "{name}: final {t:?}");
                t.as_bytes()[0]
            })
            .collect();
        assert_eq!(
            codes, finals,
            "{name}: the column/row and the final disagree"
        );
        sets.push((name.to_string(), finals));
    }
    sets
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
