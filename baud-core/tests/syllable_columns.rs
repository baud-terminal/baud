//! **How many columns a syllable takes**, against what programs count.
//!
//! `docs/research/notes/g/devanagari-columns.tsv` holds 404 units of
//! Devanagari -- the README's Hindi line and its words, the Devanagari
//! examples of Microsoft's specification and every word of two Premchand
//! pages -- with the columns each program measured: glibc's `wcswidth`,
//! unicode-width, the shells on Linux and on macOS, and baud's own cursor
//! before it counted spacing pieces. The cursor here must land where the
//! program writing the text expects:
//!
//! * by default, `?2027` reset, where glibc and unicode-width count, each
//!   letter and each spacing mark a column ([`ClusterCells::Legacy`]);
//! * with spacing marks joined, where the macOS shells count;
//! * with `?2027` set, one cell a cluster, where baud counted before.
//!
//! Every unit is fed at read sizes 1, 2, 3 and whole, since a part happens
//! on the codepoint after a join a read boundary can separate.
mod support;
use baud_core::{Owned, Term};
use baud_domain::{Cells, ClusterCells, Col, GraphemeClass, Row, SpacingMarks, WinSize};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 80,
};

/// One unit of the measured table: its codepoints and each program's count.
struct Unit {
    cps: Vec<u32>,
    one_cell: Cells,
    glibc: Cells,
    unicode_width: Cells,
    bash_macos: Cells,
}

fn units() -> Vec<Unit> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/notes/g/devanagari-columns.tsv"
    ))
    .expect("devanagari-columns.tsv");
    let mut lines = src.lines().filter(|l| !l.starts_with('#'));
    let head: Vec<&str> = lines.next().expect("a header").split('\t').collect();
    let col = |name: &str| head.iter().position(|h| *h == name).expect(name);
    let (cps, one, glibc, uw, bash) = (
        col("codepoints"),
        col("baud_one_cell"),
        col("glibc_2.36_2.41_2.43"),
        col("unicode_width_0.2.2"),
        col("bash_3.2_macos"),
    );
    let out: Vec<Unit> = lines
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let n = |i: usize| Cells::of(f[i].parse().expect("a count"));
            Unit {
                cps: f[cps]
                    .split(' ')
                    .map(|h| u32::from_str_radix(h, 16).expect("hex"))
                    .collect(),
                one_cell: n(one),
                glibc: n(glibc),
                unicode_width: n(uw),
                bash_macos: n(bash),
            }
        })
        .collect();
    assert_eq!(out.len(), 404, "the table's units");
    out
}

fn utf8(cps: &[u32]) -> Vec<u8> {
    cps.iter()
        .map(|&c| char::from_u32(c).expect("a scalar value"))
        .collect::<String>()
        .into_bytes()
}

/// The column the cursor stands at after `unit` is written from column 0,
/// on a terminal set up by `setup`, at every read size; they must agree.
fn column_after(t: &mut Term<'_>, unit: &[u32], setup: &[u8]) -> Cells {
    let text = utf8(unit);
    let mut at = None;
    for chunk in [1, 2, 3, text.len()] {
        support::feed(t, b"\x1bc");
        support::feed(t, setup);
        for piece in text.chunks(chunk) {
            support::feed(t, piece);
        }
        let c = Cells::of(t.cursor().1.index());
        assert!(
            at.is_none_or(|a| a == c),
            "read size {chunk} moved the cursor"
        );
        at = Some(c);
    }
    at.unwrap_or(Cells::of(0))
}

/// A lone combining mark, alone in its unit, at column 0: a defective
/// combining character sequence (`[[source: unicode18-ch3]]` D57), which
/// baud draws on a no-break space in a cell of its own (`Content::drawn`,
/// TUS 5.13), so it takes the column the space would.
fn lone_mark(unit: &Unit) -> bool {
    unit.cps.len() == 1
        && baud_domain::Codepoint::new(unit.cps[0]).is_some_and(|c| {
            matches!(
                GraphemeClass::of(c),
                GraphemeClass::Extend | GraphemeClass::SpacingMark
            )
        })
}

/// **By default the cursor lands where glibc and unicode-width count**,
/// on 401 of the 404 units: each letter, each consonant joined after a
/// virama and each spacing vowel sign a column. The three it misses are a
/// lone virama, anusvara and candra E at column 0, which glibc counts 0 and
/// baud gives a cell, as it gives every defective mark one.
#[test]
fn the_cursor_lands_where_glibc_counts() {
    let units = units();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    assert_eq!(t.cluster_cells(), ClusterCells::Legacy);
    let mut misses = Vec::new();
    for u in &units {
        assert_eq!(u.glibc, u.unicode_width, "{:04X?}: the two agree", u.cps);
        let got = column_after(&mut t, &u.cps, b"");
        if got != u.glibc {
            misses.push((u.cps.clone(), got, u.glibc));
        }
    }
    assert_eq!(
        misses,
        [
            (vec![0x094D], Cells::of(1), Cells::of(0)),
            (vec![0x0902], Cells::of(1), Cells::of(0)),
            (vec![0x0945], Cells::of(1), Cells::of(0))
        ],
        "every unit but the three lone marks"
    );
}

/// **With spacing marks joined, the cursor lands where the macOS shells
/// count**, bash 3.2 and 5.3 alike, on 400 of 404: a consonant after a
/// virama a column, a spacing vowel sign or a visarga none. The four it
/// misses are the lone marks at column 0, the spacing ि among them.
#[test]
fn with_spacing_marks_joined_the_cursor_lands_where_the_mac_shells_count() {
    let units = units();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.set_spacing_marks(SpacingMarks::Joined);
    assert_eq!(t.cluster_cells(), ClusterCells::LegacyMarksJoined);
    let mut misses = Vec::new();
    for u in &units {
        let got = column_after(&mut t, &u.cps, b"");
        if got != u.bash_macos {
            assert!(
                lone_mark(u),
                "{:04X?}: {got:?} against {:?}",
                u.cps,
                u.bash_macos
            );
            misses.push(u.cps[0]);
        }
    }
    assert_eq!(misses, [0x093F, 0x094D, 0x0902, 0x0945]);
}

/// **With `?2027` set a cluster takes one cell of its first character's
/// width**, where baud counted before: all 404 units, the setting
/// whichever it is.
#[test]
fn with_grapheme_cluster_mode_set_a_cluster_takes_one_cell() {
    let units = units();
    for marks in [SpacingMarks::Column, SpacingMarks::Joined] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_spacing_marks(marks);
        for u in &units {
            assert_eq!(
                column_after(&mut t, &u.cps, b"\x1b[?2027h"),
                u.one_cell,
                "{:04X?}",
                u.cps
            );
        }
        assert_eq!(t.cluster_cells(), ClusterCells::Extended);
    }
}

fn row(t: &Term<'_>, y: Row) -> Vec<String> {
    let r = t.row(y).expect("a row");
    (0..r.len())
        .map(|x| {
            let c = r[x];
            if c.content.is_empty() {
                return String::new();
            }
            let mut s: String = t
                .base_of(c)
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .into_iter()
                .collect();
            s.extend(t.marks(y, Col::at_index(x)).filter_map(char::from_u32));
            s
        })
        .collect()
}

fn cells_of(text: &str, setup: &[u8], marks: SpacingMarks) -> (Vec<String>, (usize, usize)) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.set_spacing_marks(marks);
    support::feed(&mut t, setup);
    support::feed(&mut t, text.as_bytes());
    let mut r = row(&t, Row::from_index(0));
    while r.last().is_some_and(String::is_empty) {
        r.pop();
    }
    let (y, x) = t.cursor();
    (r, (y.index(), x.index()))
}

/// **What each cell holds**: र्मि is र with its virama, म and ि by default,
/// र् and मि with spacing marks joined, and one cell with `?2027` set. A
/// copy of the cells is the text either way.
#[test]
fn each_mode_puts_a_syllable_in_its_cells() {
    let s = "र्मि";
    assert_eq!(
        cells_of(s, b"", SpacingMarks::Column),
        (vec!["र्".into(), "म".into(), "ि".into()], (0, 3))
    );
    assert_eq!(
        cells_of(s, b"", SpacingMarks::Joined),
        (vec!["र्".into(), "मि".into()], (0, 2))
    );
    assert_eq!(
        cells_of(s, b"\x1b[?2027h", SpacingMarks::Column),
        (vec!["र्मि".into()], (0, 1))
    );
}

/// **The Thai and Lao SARA AM take a column**, as every other spacing mark:
/// ก and ำ two columns by default, one with spacing marks joined or `?2027`
/// set.
#[test]
fn sara_am_takes_a_column_by_default() {
    for s in ["กำ", "ກຳ"] {
        assert_eq!(cells_of(s, b"", SpacingMarks::Column).1, (0, 2), "{s}");
        assert_eq!(cells_of(s, b"", SpacingMarks::Joined).1, (0, 1), "{s}");
        assert_eq!(
            cells_of(s, b"\x1b[?2027h", SpacingMarks::Column).1,
            (0, 1),
            "{s}"
        );
    }
}

/// **A spacing vowel sign UAX #29 classes Extend takes a column too**, as
/// glibc's `wcwidth` and vim count it: Bengali কা, Tamil கா, Kannada ಕೀ and
/// Malayalam കാ are two columns by default, the vowel sign in a cell of its
/// own, and one with spacing marks joined or `?2027` set. Bengali কো
/// written decomposed, U+09C7 U+09BE, is three columns and precomposed,
/// U+09CB, two, as glibc counts them.
#[test]
fn a_spacing_vowel_sign_classed_extend_takes_a_column() {
    for s in ["কা", "கா", "ಕೀ", "കാ"] {
        assert_eq!(cells_of(s, b"", SpacingMarks::Column).1, (0, 2), "{s}");
        assert_eq!(cells_of(s, b"", SpacingMarks::Joined).1, (0, 1), "{s}");
        assert_eq!(
            cells_of(s, b"\x1b[?2027h", SpacingMarks::Column).1,
            (0, 1),
            "{s}"
        );
    }
    assert_eq!(
        cells_of("\u{995}\u{9c7}\u{9be}", b"", SpacingMarks::Column).1,
        (0, 3)
    );
    assert_eq!(
        cells_of("\u{995}\u{9cb}", b"", SpacingMarks::Column).1,
        (0, 2)
    );
}

/// **A Prepended_Concatenation_Mark takes a column, and what follows it
/// another while `?2027` is reset**: a space, U+0600 ARABIC NUMBER SIGN and
/// U+0661 are three columns, as unicode-width and glibc count them, where
/// the sign took none and the digit joined it. With `?2027` set the digit
/// joins the sign by GB9b, one cell of the sign's width: two columns.
#[test]
fn a_number_sign_takes_a_column_and_the_digit_after_it_another() {
    let s = " \u{0600}\u{0661}";
    for marks in [SpacingMarks::Column, SpacingMarks::Joined] {
        assert_eq!(
            cells_of(s, b"", marks),
            (
                vec![" ".into(), "\u{0600}".into(), "\u{0661}".into()],
                (0, 3)
            )
        );
    }
    assert_eq!(
        cells_of(s, b"\x1b[?2027h", SpacingMarks::Column),
        (vec![" ".into(), "\u{0600}\u{0661}".into()], (0, 2))
    );
    // U+110BD KAITHI NUMBER SIGN, the other measured.
    assert_eq!(
        cells_of(" \u{110BD}\u{0661}", b"", SpacingMarks::Column).1,
        (0, 3)
    );
}

/// **A lone mark at column 0 takes a cell in every mode**: a defective
/// combining character sequence (`[[source: unicode18-ch3]]` D57), drawn on
/// a no-break space (TUS 5.13), where glibc counts a nonspacing one 0 and
/// a spacing one 1.
#[test]
fn a_lone_mark_at_column_0_takes_a_cell_in_every_mode() {
    for s in ["\u{094D}", "\u{093F}"] {
        for (setup, marks) in [
            (&b""[..], SpacingMarks::Column),
            (b"", SpacingMarks::Joined),
            (b"\x1b[?2027h", SpacingMarks::Column),
        ] {
            assert_eq!(cells_of(s, setup, marks).1, (0, 1), "{s:?} {setup:?}");
        }
    }
}

/// **A syllable may part at a soft wrap**: on an 8-column row, र् in the
/// last column, म and ि wrap to the next row by default, and मि with
/// spacing marks joined; with `?2027` set the syllable is one cell in the
/// last column and the wrap waits.
#[test]
fn a_syllable_parts_at_a_soft_wrap() {
    let ws = WinSize {
        ws_row: 3,
        ws_col: 8,
    };
    let run = |setup: &[u8], marks: SpacingMarks| {
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        t.set_spacing_marks(marks);
        support::feed(&mut t, setup);
        support::feed(&mut t, "abcdefgर्मि".as_bytes());
        let (y, x) = t.cursor();
        (
            row(&t, Row::from_index(0))[7].clone(),
            row(&t, Row::from_index(1)),
            (y.index(), x.index()),
        )
    };
    let (last, next, at) = run(b"", SpacingMarks::Column);
    assert_eq!(
        (last.as_str(), &next[..2], at),
        ("र्", &["म".to_string(), "ि".to_string()][..], (1, 2))
    );
    let (last, next, at) = run(b"", SpacingMarks::Joined);
    assert_eq!((last.as_str(), next[0].as_str(), at), ("र्", "मि", (1, 1)));
    let (last, next, at) = run(b"\x1b[?2027h", SpacingMarks::Column);
    assert_eq!((last.as_str(), next[0].as_str(), at.0), ("र्मि", "", 0));
}

const DECRQM: &[u8] = b"\x1b[?2027$p";

fn answer(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    support::feed(t, bytes);
    t.clear_reply();
    support::feed(t, DECRQM);
    t.reply().to_vec()
}

/// **`?2027` is a switch**: `DECRQM` answers 2 at power-on, 1 once set, and
/// 2 again after `RIS` or `DECSTR`, as the draft's "Terminal Reset" says
/// (`[[source: mode2027]]`). XTSAVE keeps it and XTRESTORE brings it back.
#[test]
fn grapheme_cluster_mode_is_a_switch_reset_by_both_resets() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    assert_eq!(answer(&mut t, b""), b"\x1b[?2027;2$y", "power-on");
    assert_eq!(answer(&mut t, b"\x1b[?2027h"), b"\x1b[?2027;1$y", "set");
    assert_eq!(answer(&mut t, b"\x1bc"), b"\x1b[?2027;2$y", "after RIS");
    assert_eq!(
        answer(&mut t, b"\x1b[?2027h\x1b[!p"),
        b"\x1b[?2027;2$y",
        "after DECSTR"
    );
    assert_eq!(
        answer(&mut t, b"\x1b[?2027h\x1b[?2027l"),
        b"\x1b[?2027;2$y",
        "reset"
    );
    assert_eq!(
        answer(&mut t, b"\x1b[?2027h\x1b[?2027s\x1b[?2027l\x1b[?2027r"),
        b"\x1b[?2027;1$y",
        "saved set and restored"
    );
    assert_eq!(t.cluster_cells(), ClusterCells::Extended);
}

/// **A `U+FE0F` widens its base whichever state `?2027` is in**: ❤️ takes
/// two cells reset and set alike.
#[test]
fn the_emoji_selector_widens_under_both_states() {
    for setup in [&b""[..], b"\x1b[?2027h"] {
        let (cells, at) = cells_of("\u{2764}\u{fe0f}x", setup, SpacingMarks::Column);
        assert_eq!(at, (0, 3), "{setup:?}");
        assert_eq!(cells[0], "\u{2764}\u{fe0f}");
    }
}

/// **The mode in force when a character arrives decides**, and the text
/// already written keeps its cells: a control sequence ends a cluster
/// (`[[source: uax29]]` GB4, GB5), so `?2027` set or reset between two
/// syllables measures the second alone.
#[test]
fn the_mode_measures_the_next_character() {
    let (cells, at) = cells_of("र्मि\x1b[?2027hर्मि\x1b[?2027lर्मि", b"", SpacingMarks::Column);
    assert_eq!(cells, ["र्", "म", "ि", "र्मि", "र्", "म", "ि"]);
    assert_eq!(at, (0, 7));
}
