//! The national replacement tables, held to the source they are generated
//! from, at the data level.
//!
//! `nrcs_tables_match_the_pinned_source` runs the generator with `--check`,
//! which compares the generator with its own output: a line it skips never
//! enters either side. That is how Swedish 4/0 went missing -- the generator
//! read a `MAP` line's codepoint from its `/* U+ */` comment, and
//! `MAP(0x40, XK_Eacute)` has none -- with both tests that cite the tables
//! green (note AF §2.3, D1 and D2). This file counts the lines of the pinned
//! `[[source: xterm-charsets]]` itself and shares no code with the generator.
//!
//! Nothing here renders through `Term`. Whether a national set acts outside
//! national mode was note AF's Q-DECNRCM, decided (c) on 2026-10-24, and
//! the tests that draw a designated set are in `baud-core`.
use baud_domain::{Nrcs, nrcs_table};

const CHARSETS_H: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/research/sources/xterm-charsets.h"
));

/// xterm's macro, baud's set, and how far baud's table is from one entry per
/// `MAP`, `UNI` or `XXX` line of the macro.
///
/// Two sets are off by design, and each is named here rather than absorbed:
///
/// * **Greek, -2.** `XXX(0x79, UNDEF)` and `XXX(0x7a, UNDEF)`, which xterm
///   marks "unused" and draws as U+2426. What an undefined position draws is
///   note AF's Q-UNDEF, the owner's, so neither line is emitted until it is
///   answered.
/// * **Turkish, +1.** 2/1, which `[[source: dec-pplv2-pm]]` Figure A-26 draws
///   as a dotless small i, U+0131, and xterm's macro leaves as `!` although
///   its comment cites the same figure. The value is typed into the
///   generator's DEC-override table with its citation, because that manual
///   is `external` and nothing can extract it.
const SETS: [(&str, Nrcs, i32); 14] = [
    ("Dutch", Nrcs::Dutch, 0),
    ("Finnish", Nrcs::Finnish, 0),
    ("French", Nrcs::French, 0),
    ("French_Canadian", Nrcs::FrenchCanadian, 0),
    ("German", Nrcs::German, 0),
    ("Italian", Nrcs::Italian, 0),
    ("Norwegian_Danish", Nrcs::NorwegianDanish, 0),
    ("Portuguese", Nrcs::Portuguese, 0),
    ("Spanish", Nrcs::Spanish, 0),
    ("Swedish", Nrcs::Swedish, 0),
    ("Swiss", Nrcs::Swiss, 0),
    ("Greek", Nrcs::Greek, -2),
    ("Hebrew", Nrcs::Hebrew, 0),
    ("Turkish", Nrcs::Turkish, 1),
];

/// The entry lines of `map_NRCS_<name>`, from the one definition that has a
/// body.
///
/// The header defines Greek, Hebrew and Turkish twice: with a `switch` under
/// `#if OPT_WIDE_CHARS`, and as `/* nothing */` under its `#else`
/// (`charsets.h:2533-2541`). A definition is its `#define` line and every
/// line a trailing backslash continues; the `#else` ones continue nothing.
fn entry_lines(name: &str) -> Vec<&'static str> {
    let head = format!("#define map_NRCS_{name}(code)");
    let lines: Vec<&'static str> = CHARSETS_H.lines().collect();
    let mut bodies = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !l.starts_with(&head) {
            continue;
        }
        let mut body = Vec::new();
        let mut j = i;
        while lines[j].trim_end().ends_with('\\') {
            j += 1;
            body.push(lines[j]);
        }
        if !body.is_empty() {
            bodies.push(body);
        }
    }
    assert_eq!(
        bodies.len(),
        1,
        "map_NRCS_{name}: {} definitions with a body, where one was expected",
        bodies.len()
    );
    bodies
        .pop()
        .unwrap()
        .into_iter()
        .filter(|l| ["MAP(", "UNI(", "XXX("].iter().any(|k| l.contains(k)))
        .collect()
}

/// **Every set carries every line of its source, but for the two named in
/// `SETS`.**
///
/// Red on the dropped line and the missing position, and on nothing else:
/// Swedish carried 9 entries for 10 lines, and Turkish 11 where xterm's 11
/// lines and DEC's 2/1 make 12.
#[test]
fn every_set_carries_every_line_of_its_source() {
    let mut total = 0;
    let mut wrong = Vec::new();
    for (name, set, adjust) in SETS {
        let lines = entry_lines(name).len() as i32;
        assert!(lines > 0, "map_NRCS_{name}: no entry lines found");
        total += lines;
        let have = nrcs_table(set).len() as i32;
        if have != lines + adjust {
            wrong.push(format!(
                "{set:?}: {have} entries, for {lines} lines in map_NRCS_{name} {adjust:+}"
            ));
        }
    }
    // Positive control: the fourteen macros hold 165 entry lines at the
    // pinned commit -- 162 the old generator emitted, the Swedish line it
    // dropped and Greek's two `UNDEF` lines. A reader that stopped finding
    // lines would make every comparison above vacuous.
    assert_eq!(total, 165, "entry lines across the fourteen macros");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **Swedish 4/0 is É, U+00C9.**
///
/// `[[source: dec-special-graphics]]` Table 2-14 shades it and draws É, and
/// `[[source: vt220-pocket-guide]]` p. 16, `[[source: vt420-rm]]` Table 2-1
/// and `[[source: dec-pplv2-pm]]` Figure A-15 agree (note AF §2.3). xterm has
/// it as `MAP(0x40, XK_Eacute)` (`charsets.h:393`), and `XK_Eacute` is
/// `0x00c9` in `[[source: xorg-keysymdef]]` (line 769).
#[test]
fn swedish_4_0_is_e_with_acute() {
    let sv = nrcs_table(Nrcs::Swedish);
    assert!(
        sv.contains(&(0x40, 0x00c9)),
        "Swedish 0x40 must be U+00C9, not absent: {sv:x?}"
    );
    assert_eq!(sv.len(), 10, "Swedish replaces ten positions");
}

/// **Turkish 2/1 is a dotless small i, U+0131.**
///
/// `[[source: dec-pplv2-pm]]` Figure A-26 draws a Helvetica-Bold `i` there
/// with its dot covered by a white rectangle, the only white shape on the
/// page; the `i` at 6/9 has none (note AF §2.4). xterm's macro has no 2/1.
#[test]
fn turkish_2_1_is_a_dotless_i() {
    let tr = nrcs_table(Nrcs::Turkish);
    assert!(
        tr.contains(&(0x21, 0x0131)),
        "Turkish 0x21 must be U+0131, not absent: {tr:x?}"
    );
}
