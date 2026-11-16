//! UAX #29 extended grapheme clusters, against Unicode's own conformance suite.
//!
//! `[[source: uax29]]` ships `GraphemeBreakTest.txt`: every case is a codepoint
//! sequence annotated with `÷` at each boundary and `×` where there is none.
//! That is a ready-made suite of several hundred cases written by the people
//! who wrote the rules, which is a far better test than anything invented here
//! — and exactly the kind of "generate valid sequences, do not guess" the
//! property suites already use.
//!
//! Why this exists at all: the text-encoding research deferred
//! grapheme clustering because its cost was **unmeasured**. A cost cannot be
//! measured without an implementation, so this is the implementation, correct
//! against the official suite, so the deferral becomes a decision made on a
//! number rather than on an assumption.

use crate::support;
use baud_core::*;

/// One case: the codepoints, and whether a boundary precedes each.
struct Case {
    cps: Vec<u32>,
    breaks: Vec<bool>,
    line: usize,
}

fn cases() -> Vec<Case> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/unicode-graphemebreaktest.txt"
    ))
    .expect("GraphemeBreakTest.txt");
    let mut out = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let body = line.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let (mut cps, mut breaks) = (Vec::new(), Vec::new());
        let mut pending: Option<bool> = None;
        let mut ok = true;
        for tok in body.split_whitespace() {
            match tok {
                "÷" => pending = Some(true),
                "×" => pending = Some(false),
                hex => match u32::from_str_radix(hex, 16) {
                    Ok(cp) => {
                        cps.push(cp);
                        breaks.push(pending.unwrap_or(true));
                    }
                    Err(_) => ok = false,
                },
            }
        }
        if ok && !cps.is_empty() {
            out.push(Case {
                cps,
                breaks,
                line: n + 1,
            });
        }
    }
    out
}

/// **Every case in Unicode's conformance suite.**
#[test]
fn matches_the_unicode_grapheme_break_suite() {
    let all = cases();
    assert!(
        all.len() > 700,
        "only {} cases parsed -- the suite parser stopped matching",
        all.len()
    );
    let mut failures = Vec::new();
    for c in &all {
        let mut st = ClusterBreak::new();
        let mut got = Vec::with_capacity(c.cps.len());
        for cp in &c.cps {
            let Some(code) = Codepoint::new(*cp) else {
                got.push(true);
                continue;
            };
            got.push(st.is_break(code));
        }
        if got != c.breaks {
            failures.push(format!(
                "line {}: {:04x?}\n      want {:?}\n      got  {:?}",
                c.line, c.cps, c.breaks, got
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} conformance cases failed:\n  {}",
        failures.len(),
        all.len(),
        failures
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// The cases baud actually cares about, named so a regression says what broke.
#[test]
fn the_clusters_a_terminal_meets() {
    let clusters = |s: &str| {
        let mut st = ClusterBreak::new();
        s.chars()
            .filter_map(|c| Codepoint::new(c as u32))
            .filter(|c| st.is_break(*c))
            .count()
    };
    assert_eq!(clusters("a"), 1, "one character");
    assert_eq!(
        clusters("e\u{301}"),
        1,
        "e + combining acute is one cluster"
    );
    assert_eq!(
        clusters("\u{1f469}\u{200d}\u{1f4bb}"),
        1,
        "woman + ZWJ + laptop is one cluster (GB11)"
    );
    assert_eq!(
        clusters("\u{1f1ef}\u{1f1f5}"),
        1,
        "a regional-indicator pair is one flag (GB12/13)"
    );
    assert_eq!(
        clusters("\u{1f1ef}\u{1f1f5}\u{1f1ef}\u{1f1f5}"),
        2,
        "two flags, not one run"
    );
    assert_eq!(
        clusters("\u{915}\u{94d}\u{937}"),
        1,
        "Devanagari conjunct ka + virama + ssa (GB9c)"
    );
    assert_eq!(clusters("\r\n"), 1, "CRLF is one cluster (GB3)");
    assert_eq!(clusters("a\r\nb"), 3, "and does not swallow its neighbours");
}

/// **Every code point has the class and the lookback sets the pinned files
/// give**, through `GraphemeClass::of_with_lookback` and
/// `is_spacing_extend`, from GraphemeBreakProperty.txt, emoji-data.txt's
/// Extended_Pictographic, DerivedCoreProperties.txt's InCB and
/// DerivedGeneralCategory.txt, read by `width.rs`'s own reader rather than
/// the build's. A class is matched by its name, so the table's order is
/// not what this checks.
#[test]
fn every_code_point_has_the_grapheme_class_the_pinned_files_give() {
    use crate::width::{ucd_has, ucd_values};
    let gcb = ucd_values("unicode-graphemebreakproperty.txt", "Other");
    let gc = ucd_values("unicode-derivedgeneralcategory.txt", "Cn");
    let pictographic = ucd_has("unicode-emoji-data.txt", "Extended_Pictographic");
    let dcp = "unicode-derivedcoreproperties.txt";
    let linker = ucd_has(dcp, "InCB;Linker");
    let consonant = ucd_has(dcp, "InCB;Consonant");
    let extend = ucd_has(dcp, "InCB;Extend");
    let name = |gcb: &str| {
        match gcb {
            "CR" => "Cr",
            "LF" => "Lf",
            "ZWJ" => "Zwj",
            "Regional_Indicator" => "RegionalIndicator",
            "LV" => "Lv",
            "LVT" => "Lvt",
            other => other,
        }
        .to_string()
    };
    let mut wrong = Vec::new();
    for cp in 0..0x11_0000u32 {
        let Some(c) = Codepoint::new(cp) else {
            continue;
        };
        let i = usize::try_from(cp).expect("index");
        let (class, lookback) = GraphemeClass::of_with_lookback(c);
        let got = (
            format!("{class:?}"),
            lookback.pictographic(),
            lookback.linker(),
            lookback.consonant(),
            lookback.incb_extend(),
            baud_domain::is_spacing_extend(c),
        );
        let want = (
            name(&gcb[i]),
            pictographic[i],
            linker[i],
            consonant[i],
            extend[i],
            gc[i] == "Mc" && gcb[i] == "Extend",
        );
        if got != want {
            wrong.push(format!("U+{cp:04X} {got:?} for {want:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} wrong: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(20)]
    );
}

/// **A combining mark is attached, not discarded.**
///
/// `baud-core2`'s `put_cp` returned early on `Width::ZERO`, so every combining
/// mark was silently thrown away: `e` + U+0301 stored `e` and the accent was
/// *gone* — not mis-measured, absent, and unrecoverable by a renderer, a
/// selection or a copy. A terminal must not lose a character it was sent.
///
/// The cell keeps the base and the marks live beside the grid, because
/// `PackedCell` is eight measured bytes with no room: `Paint::attach` puts
/// them in `ClusterStore`, keyed on the cluster's own content, and the slot's
/// index rides in the cell's `CellFlags`. Found by position was `baud-core2`'s
/// design.
#[test]
fn where_a_combining_mark_ends_up() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let bytes = "e\u{301}".as_bytes();
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    let cols = t.cursor().1.index();
    let cells: Vec<u32> = t.viewport()[..cols.max(1)]
        .iter()
        .map(|c| t.base_of(*c).as_codepoint().map_or(0, |c| c.value()))
        .collect();
    assert_eq!(cells, vec![0x65], "one cell, holding the base `e`");
    let marks: Vec<u32> = t.marks(Row::from_index(0), Col::ZERO).collect();
    assert_eq!(marks, vec![0x301], "and the acute is attached to it");
    assert_eq!(t.marks_dropped, 0, "nothing was lost");
}

/// Several marks stack, and a cell with none reports none.
#[test]
fn marks_stack_and_do_not_leak_between_cells() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    // `a` + acute + diaeresis, then a plain `b`.
    let bytes = "a\u{301}\u{308}b".as_bytes();
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    let at = |c: usize| -> Vec<u32> { t.marks(Row::from_index(0), Col::at_index(c)).collect() };
    assert_eq!(at(0), vec![0x301, 0x308], "both marks on the `a`");
    assert_eq!(at(1), Vec::<u32>::new(), "and none on the `b`");
}

/// **A mark with no base character takes a cell of its own.**
///
/// `[[source: uax29]]` GB1 puts a boundary at start of text, so a leading mark
/// is its own cluster, a defective combining character sequence (TUS D57, "at
/// the start of a string"), stored alone and drawn on a no-break space (TUS
/// 5.13). Nothing is dropped, though the name says counted, and the `x`
/// after it takes the next cell.
#[test]
fn a_leading_mark_has_no_base_and_is_counted() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let bytes = "\u{301}x".as_bytes();
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    assert_eq!(t.marks_dropped, 0, "the mark is kept");
    let row = t.row(Row::ZERO).expect("a row");
    assert_eq!(
        t.base_of(row[0]),
        Content::DEFECTIVE,
        "a cell of its own, with no base"
    );
    let marks: Vec<u32> = t.marks(Row::ZERO, Col::ZERO).collect();
    assert_eq!(marks, vec![0x301], "holding the mark");
    assert_eq!(
        row[1].content.as_codepoint().map(|c| c.value()),
        Some(0x78),
        "the x after it"
    );
}

// ── one cluster, one cell ───────────────────────────────────────────────────

fn columns(s: &str) -> usize {
    columns_after(b"", s)
}

/// The columns `s` takes once `setup` has been fed.
fn columns_after(setup: &[u8], s: &str) -> usize {
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    support::feed(&mut t, setup);
    support::feed(&mut t, s.as_bytes());
    t.cursor().1.index()
}

/// **A ZWJ emoji sequence is one cluster and takes the columns of one.**
///
/// Width alone cannot get this right: `U+1F469 U+200D U+1F4BB` is three
/// codepoints of width 2, 0 and 2, so measuring each independently gives four
/// columns for a single glyph — and every column of drift desynchronises the
/// cursor against an application counting the same text with its own rules.
#[test]
fn a_zwj_sequence_occupies_one_cluster_of_columns() {
    assert_eq!(
        columns("\u{1f469}\u{200d}\u{1f4bb}"),
        2,
        "woman technologist"
    );
    assert_eq!(columns("\u{1f469}"), 2, "the base emoji alone, for scale");
}

/// A flag is two regional indicators and one cluster.
#[test]
fn a_flag_is_one_cluster() {
    assert_eq!(columns("\u{1f1ef}\u{1f1f5}"), 2, "one flag");
    assert_eq!(
        columns("\u{1f1ef}\u{1f1f5}\u{1f1ef}\u{1f1f5}"),
        4,
        "two flags, not one run of four indicators"
    );
}

/// An Indic conjunct stays whole (GB9c) while `?2027` is set, one cell;
/// reset, its legacy clusters take a column each, as glibc counts them
/// (`tests/syllable_columns.rs`).
#[test]
fn an_indic_conjunct_is_one_cluster() {
    assert_eq!(
        columns_after(b"\x1b[?2027h", "\u{915}\u{94d}\u{937}"),
        1,
        "ka + virama + ssa"
    );
    assert_eq!(columns("\u{915}\u{94d}\u{937}"), 2, "ka + virama, ssa");
}

/// Clusters do not run together across unrelated text.
///
/// The run scanner blits printable ASCII without ever feeding `ClusterBreak`,
/// so without a reset the emoji after `a` would be judged against the emoji
/// before it and swallowed into its cluster.
#[test]
fn ascii_between_emoji_breaks_the_cluster() {
    assert_eq!(columns("\u{1f469}a\u{1f469}"), 5, "2 + 1 + 2");
}

/// And the ordinary cases still measure as they did.
#[test]
fn clustering_does_not_disturb_ordinary_text() {
    assert_eq!(columns("hello"), 5);
    assert_eq!(columns("中文"), 4);
    assert_eq!(columns("e\u{301}"), 1, "base plus mark is still one column");
}

/// **The trie's four-bit values and `GraphemeClass`'s declaration order agree.**
///
/// `GraphemeClass::of` reads a nibble from a generated table and indexes
/// `GraphemeClass::FROM_INDEX` with it. That correspondence is the one thing
/// here that would fail *silently*: reorder the enum, and every codepoint gets
/// a different class while the code still compiles and runs. The build
/// pins the order on its side; this asserts it on the code's side, so the
/// two have to be changed together.
///
/// Not a `transmute` for the same reason. A cast would make the bug
/// unobservable rather than merely quiet.
#[test]
fn the_break_trie_agrees_with_the_class_enum() {
    use baud_core::GraphemeClass::*;
    let want = [
        Other,
        Cr,
        Lf,
        Control,
        Extend,
        Zwj,
        RegionalIndicator,
        Prepend,
        SpacingMark,
        L,
        V,
        T,
        Lv,
        Lvt,
    ];
    for (i, c) in want.iter().enumerate() {
        assert_eq!(
            GraphemeClass::FROM_INDEX[i],
            *c,
            "index {i} of the trie's value space must be {c:?}"
        );
    }
    // The two spare slots exist so a four-bit value cannot leave the array.
    assert_eq!(GraphemeClass::FROM_INDEX.len(), 16);
    // And a spot check that the table is actually consulted: these are the
    // classes the cluster rules turn on, and none of them is `Other`.
    let of = |c: char| GraphemeClass::of(Codepoint::new(c as u32).expect("a char is a codepoint"));
    assert_eq!(of('\u{200d}'), Zwj);
    assert_eq!(of('\u{0301}'), Extend);
    assert_eq!(of('\u{1f1e6}'), RegionalIndicator);
    assert_eq!(of('\u{ac00}'), Lv);
    assert_eq!(of('\u{1100}'), L);
    assert_eq!(of('\u{4e00}'), Other);
}

// ── every line of the suite, through the terminal ───────────────────────────

/// What one column of a row holds: a cell's base and the marks attached to it
/// (`Term::marks`), a wide character's tail, or nothing.
#[derive(Clone, PartialEq, Eq)]
enum Column {
    Blank,
    Tail,
    Glyph(Vec<Codepoint>),
    /// A combining mark with no base, stored alone (`Content::DEFECTIVE`),
    /// and what joined it.
    Defective(Vec<Codepoint>),
}

/// `61+903` for a cell, `tail` and `_`, so a failure reads as a row.
impl std::fmt::Debug for Column {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Column::Blank => f.write_str("_"),
            Column::Tail => f.write_str("tail"),
            Column::Glyph(g) => {
                let hex: Vec<String> = g.iter().map(|c| format!("{:X}", c.value())).collect();
                f.write_str(&hex.join("+"))
            }
            Column::Defective(g) => {
                let hex: Vec<String> = g.iter().map(|c| format!("{:X}", c.value())).collect();
                write!(f, "()+{}", hex.join("+"))
            }
        }
    }
}

/// What reaches the grid before a line, and which writer it reaches it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Before {
    /// Nothing: the line is the start of the text, so GB1 is the boundary
    /// before its first codepoint.
    Nothing,
    /// `xy`, a two-letter run, which `Paint::piece` writes a character at a
    /// time -- unless the line begins with ASCII and lengthens it.
    TwoLetters,
    /// `xyz`, a three-letter run, which `Paint::piece` hands to
    /// `Paint::blit`.
    ThreeLetters,
}

impl Before {
    const ALL: [Before; 3] = [Before::Nothing, Before::TwoLetters, Before::ThreeLetters];

    fn letters(self) -> &'static str {
        match self {
            Before::Nothing => "",
            Before::TwoLetters => "xy",
            Before::ThreeLetters => "xyz",
        }
    }
}

/// Why a line this core renders differently from the policy below does so.
///
/// **No line is refused by the store any more.** Lines 196 (after `xyz`) and
/// 788 (in all three) were, while `ClusterStore` probed two of 63 slots from
/// an unfinalized hash: a fourth or a sixth prefix found both its slots
/// taken on a fresh store. With eight probes of 1024 slots and a finalized
/// hash they render as the standard says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Why {
    /// Printable ASCII straight after a Prepend. GB9b joins them; the Prepend
    /// is zero-width and joins the cell before it (`ZERO_WIDTH_STARTS_ALONE`),
    /// and the letter goes through `Paint::piece`, which never asks the
    /// machine whether it continues anything, so it takes a cell of its own.
    /// Note AB §7.1's "Arabic Prepend" row, an open departure.
    AsciiAfterPrepend,
}

/// **Every line where this core's grid departs from the cells the policy
/// below makes of the standard's clusters**, and why. A line that starts
/// passing fails this pin as surely as a new one failing does: the list moves
/// when the departure does.
const KNOWN: &[(usize, &[Before], Why)] = &[
    // `06DD 0020`: U+06DD ARABIC END OF AYAH is a Prepended_Concatenation_Mark,
    // a column of its own, so alone at column 0 it takes a
    // cell as after a letter, and the space after it one more.
    (365, &Before::ALL, Why::AsciiAfterPrepend),
    // `0061 0600 0062`, the file's own GB9b case.
    (764, &Before::ALL, Why::AsciiAfterPrepend),
];

/// **Lines whose standard clustering starts a cluster with a zero-width
/// codepoint**, which this core's cell model cannot give a cell of its own.
///
/// Alone, that is every line that begins with a mark or a joiner; after an
/// ASCII letter none are left, because GB9 joins them to it. And in both,
/// every line where one follows a control, after which GB4 starts a cluster
/// whatever comes: the even lines 28-140, a control then `U+0308`, and a
/// control then a mark or a joiner, 33-40 after CR, 71-78 after LF, 109-116
/// after NUL; and 749, which ends `LF U+0308`. Alone, the lines that begin
/// with a mark or a joiner and then hold a control join the rest: 141-146
/// and the like inside 140-292. The suite's Prepends, U+0600 and U+06DD,
/// are Prepended_Concatenation_Marks, a column each, so no line starts a
/// cluster with a zero-width Prepend.
const ZERO_WIDTH_STARTS_ALONE: &[(usize, usize)] = &[
    (28, 28),
    (30, 30),
    (32, 40),
    (42, 42),
    (44, 44),
    (46, 46),
    (48, 48),
    (50, 50),
    (52, 52),
    (54, 54),
    (56, 56),
    (58, 58),
    (60, 60),
    (62, 62),
    (64, 64),
    (66, 66),
    (68, 68),
    (70, 78),
    (80, 80),
    (82, 82),
    (84, 84),
    (86, 86),
    (88, 88),
    (90, 90),
    (92, 92),
    (94, 94),
    (96, 96),
    (98, 98),
    (100, 100),
    (102, 102),
    (104, 104),
    (106, 106),
    (108, 116),
    (118, 118),
    (120, 120),
    (122, 122),
    (124, 124),
    (126, 126),
    (128, 128),
    (130, 130),
    (132, 132),
    (134, 134),
    (136, 136),
    (138, 138),
    (140, 292),
    (749, 749),
];
const ZERO_WIDTH_STARTS_AFTER_LETTERS: &[(usize, usize)] = &[
    (28, 28),
    (30, 30),
    (32, 40),
    (42, 42),
    (44, 44),
    (46, 46),
    (48, 48),
    (50, 50),
    (52, 52),
    (54, 54),
    (56, 56),
    (58, 58),
    (60, 60),
    (62, 62),
    (64, 64),
    (66, 66),
    (68, 68),
    (70, 78),
    (80, 80),
    (82, 82),
    (84, 84),
    (86, 86),
    (88, 88),
    (90, 90),
    (92, 92),
    (94, 94),
    (96, 96),
    (98, 98),
    (100, 100),
    (102, 102),
    (104, 104),
    (106, 106),
    (108, 116),
    (118, 118),
    (120, 120),
    (122, 122),
    (124, 124),
    (126, 126),
    (128, 128),
    (130, 130),
    (132, 132),
    (134, 134),
    (136, 136),
    (138, 138),
    (140, 140),
    (749, 749),
];

/// Wide enough that no line of the suite wraps, and deep enough that none
/// scrolls: the most line feeds a line holds is two (67 and 749), so three
/// rows keep every line's output on the screen it was written to.
const SUITE_WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 64,
};

fn width_of(c: Codepoint) -> Width {
    Width::of(c, AmbiguousWidth::Narrow)
}

/// The standard's clusters for `before` followed by the line.
///
/// The file's boundaries hold as written after nothing. After an ASCII
/// letter, the boundary before the line's first codepoint is decided by the
/// only rules that can join anything to a letter: it is class Other with no
/// lookback, so of `[[source: uax29]]`'s rules just GB9 (`× (Extend | ZWJ)`)
/// and GB9a (`× SpacingMark`) remove it. Every later boundary is the file's,
/// because the rules that look further back -- GB9c, GB11 and GB12/13 --
/// look back through classes a letter is not, and GB12's `^` is "start of
/// text or a non-indicator".
fn standard_clusters(before: Before, c: &Case) -> Vec<Vec<Codepoint>> {
    let letters = before.letters().chars().map(u32::from);
    let mut out: Vec<Vec<Codepoint>> = Vec::new();
    let line = c.cps.iter().copied().zip(c.breaks.iter().copied());
    let all = letters
        .map(|l| (l, true))
        .chain(line.enumerate().map(|(i, (v, brk))| {
            let brk = match (i, before) {
                (0, Before::Nothing) => true,
                (0, _) => !matches!(
                    GraphemeClass::of(Codepoint::new(v).expect("a scalar value")),
                    GraphemeClass::Extend | GraphemeClass::Zwj | GraphemeClass::SpacingMark
                ),
                _ => brk,
            };
            (v, brk)
        }));
    for (v, brk) in all {
        let cp = Codepoint::new(v).expect("the suite names scalar values");
        match out.last_mut() {
            Some(k) if !brk => k.push(cp),
            _ => out.push(vec![cp]),
        }
    }
    out
}

/// Whether `cp` is general category Cc -- C0, DEL or C1 -- which is what
/// the break property calls Control, CR or LF.
fn is_control(cp: Codepoint) -> bool {
    char::from_u32(cp.value()).is_some_and(char::is_control)
}

/// Write one cluster's cell at `(r, c)`, over whatever is there, and return
/// how many columns it took.
///
/// **The model refuses what it does not need**: no line of the suite, and no
/// probe below, writes over a wide character or past the row's end, so
/// rather than guess what the core does to an orphaned half it says so and
/// stops.
fn put_cell(screen: &mut [Vec<Column>], (r, c): (usize, usize), k: &[Codepoint]) -> usize {
    let w = match width_of(k[0]) {
        Width::WIDE => 2,
        _ => 1,
    };
    let row = &mut screen[r];
    assert!(c + w <= row.len(), "the model does not wrap");
    for cell in &row[c..c + w] {
        let wide = matches!(cell, Column::Glyph(g) if width_of(g[0]) == Width::WIDE);
        assert!(
            *cell != Column::Tail && !wide,
            "the model does not overwrite a wide character"
        );
    }
    row[c] = Column::Glyph(k.to_vec());
    if w == 2 {
        row[c + 1] = Column::Tail;
    }
    w
}

/// The screen and cursor this core's documented policy makes of
/// `clusters`, fed as the terminal receives them.
///
/// **A control is a cluster of its own** (`[[source: uax29]]` GB4 and GB5
/// break before and after Control, CR and LF; GB3 keeps CR LF together), and
/// the terminal acts on it rather than writing it: CR returns to column 0
/// and LF moves down a row and keeps the column (`[[source: ecma48]]` 8.3.15
/// and 8.3.74; LNM, which would add the return, is reset). NUL changes
/// nothing on the screen -- the parser discards it, and 8.3.88 says it "may
/// be inserted into, or removed from, a data stream without affecting the
/// information content" -- but it is still a Control to GB4 and GB5, so the
/// cluster before it ends there. That reading is the face value of both
/// texts, and treating NUL as absent instead is the alternative.
///
/// Every other cluster takes one cell, holding all its codepoints and taking
/// its first one's width. **The policy is what happens to a cluster that
/// starts with a zero-width codepoint.** A combining mark there, an Extend
/// that is not default-ignorable, is a defective combining character
/// sequence (TUS D57) and takes one cell with no base, the mark and the rest
/// of the cluster in it (`Content::DEFECTIVE`). A
/// default-ignorable Extend or ZWJ draws nothing and is dropped, and what
/// follows it starts afresh, as after a control. Any other zero-width head,
/// a Prepend, joins the written cell before the cursor on the cursor's row,
/// and the rest of the cluster follows it there; at column 0 it is dropped
/// and the first codepoint with a width takes a cell and the rest join that,
/// and where the cell before the cursor is empty the cluster is dropped
/// whole. Until 2026-10-23 every zero-width head went the Prepend's way.
fn policy_screen(clusters: &[Vec<Codepoint>]) -> (Vec<Vec<Column>>, (Row, Col)) {
    let (rows, cols) = (usize::from(SUITE_WS.ws_row), usize::from(SUITE_WS.ws_col));
    let mut screen = vec![vec![Column::Blank; cols]; rows];
    let (mut r, mut c) = (0usize, 0usize);
    for k in clusters {
        if is_control(k[0]) {
            for cp in k {
                match cp.value() {
                    0x0d => c = 0,
                    0x0a => {
                        r += 1;
                        assert!(r < rows, "the model does not scroll");
                    }
                    0x00 => {}
                    v => panic!("the suite holds no control U+{v:04X}"),
                }
            }
            continue;
        }
        // A combining mark heads a cell of its own; a default-ignorable
        // head is dropped and the rest goes on as a cluster of its own. At
        // column 0 any other zero-width head is dropped too, and what follows
        // it starts again there, as `glyph` restarts the machine at column 0.
        let mut k: &[Codepoint] = k;
        while let Some(&head) = k.first() {
            let extend = matches!(
                GraphemeClass::of(head),
                GraphemeClass::Extend | GraphemeClass::Zwj
            );
            let dropped = match extend {
                true => baud_domain::is_default_ignorable(head),
                false => c == 0,
            };
            if width_of(head) != Width::ZERO || !dropped {
                break;
            }
            k = &k[1..];
        }
        let Some(&head) = k.first() else {
            continue;
        };
        if width_of(head) == Width::ZERO
            && matches!(
                GraphemeClass::of(head),
                GraphemeClass::Extend | GraphemeClass::Zwj
            )
        {
            assert!(c < cols, "the model does not wrap");
            screen[r][c] = Column::Defective(k.to_vec());
            c += 1;
            continue;
        }
        let lead = k.iter().position(|cp| width_of(*cp) != Width::ZERO);
        // The written cell that owns the column before the cursor.
        let before = match c {
            0 => None,
            _ => {
                let j = match screen[r][c - 1] {
                    Column::Tail => c - 2,
                    _ => c - 1,
                };
                Some((j, screen[r][j] != Column::Blank))
            }
        };
        match (lead, before) {
            // A cluster with a width at its head takes a cell; so does one
            // whose zero-width head fell at column 0, from its first codepoint
            // with a width.
            (Some(0), _) | (Some(_), None) => {
                let rest = &k[lead.unwrap_or(0)..];
                c += put_cell(&mut screen, (r, c), rest);
            }
            // A zero-width head joins the written cell before, and the
            // cluster with it.
            (_, Some((j, true))) => {
                if let Column::Glyph(g) | Column::Defective(g) = &mut screen[r][j] {
                    g.extend(k);
                }
            }
            // No written cell before it, or nothing with a width at column 0:
            // all dropped.
            (_, Some((_, false))) | (None, None) => {}
        }
    }
    (screen, (Row::from_index(r), Col::at_index(c)))
}

/// Every row, the cursor and `marks_dropped` after feeding `bytes` in reads
/// of at most `read`.
fn through_term(bytes: &[u8], read: ReadLen) -> (Vec<Vec<Column>>, (Row, Col), usize) {
    through_term_in(Cells::Extended, bytes, read)
}

/// Which clusters a cell holds while a test feeds it: `?2027` set, the
/// extended ones `[[source: gbtest]]` lists, or reset, the legacy ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cells {
    Extended,
    Legacy,
}

/// [`through_term`] under `cells`: `?2027` set first for the extended
/// clusters, a control the machine starts again after.
fn through_term_in(
    cells: Cells,
    bytes: &[u8],
    read: ReadLen,
) -> (Vec<Vec<Column>>, (Row, Col), usize) {
    let mut owned = Owned::new(&SUITE_WS);
    let mut t = Term::new(owned.storage(), &SUITE_WS);
    if cells == Cells::Extended {
        support::feed(&mut t, b"\x1b[?2027h");
    }
    for piece in bytes.chunks(read.index()) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let screen = (0..usize::from(SUITE_WS.ws_row))
        .map(Row::from_index)
        .map(|y| {
            t.row(y)
                .expect("a row on the grid")
                .iter()
                .enumerate()
                .map(
                    |(x, c)| match (c.content.is_empty(), c.content.is_wide_tail()) {
                        (true, _) => Column::Blank,
                        (_, true) => Column::Tail,
                        _ => {
                            let marks = t
                                .marks(y, Col::at_index(x))
                                .map(|m| Codepoint::new(m).expect("a mark is a scalar value"));
                            if t.base_of(*c) == Content::DEFECTIVE {
                                return Column::Defective(marks.collect());
                            }
                            let base = t
                                .base_of(*c)
                                .as_codepoint()
                                .expect("a written cell holds one");
                            Column::Glyph(std::iter::once(base).chain(marks).collect())
                        }
                    },
                )
                .collect()
        })
        .collect();
    (screen, t.cursor(), t.marks_dropped)
}

fn spread(ranges: &[(usize, usize)]) -> Vec<usize> {
    ranges.iter().flat_map(|(a, b)| *a..=*b).collect()
}

/// **Every line of `[[source: gbtest]]`, fed through `Term`, at every read
/// size, alone and after printable ASCII taken by both of `Paint::piece`'s
/// branches.**
///
/// `matches_the_unicode_grapheme_break_suite` holds `ClusterBreak` to the file
/// and passes every line, which is a statement about the machine and not
/// about the terminal: the terminal decides when the machine is *asked*.
/// Printable ASCII is written by `Paint::piece` and `Paint::blit` without
/// asking, and they tell the machine what went past. Until 2026-10-10 they
/// told it "nothing came before this", so GB1 broke ahead of GB9 and GB9a and
/// a continuation with a width took a cell of its own after an ASCII letter:
/// lines 691, 763, 766 and 767 alone, and with a letter before them the
/// file's 32 control-free lines that begin with `U+0903`, failed here and
/// nowhere else.
///
/// **And the 211 lines that hold a control**, NUL, LF or CR, fed as the
/// terminal receives them: `policy_screen` says what each does to the
/// screen. The machine was never told of a control, so after a letter a
/// line feed left it saying a letter came before, and `x y LF U+0903` put
/// the visarga on the empty cell below the `y`, where `Paint::attach` found
/// no base and dropped it -- line 83 and its neighbours. GB4 and GB5 break
/// on both sides of a control, so every control now leaves the machine as
/// they do (`ClusterBreak::after_control`).
///
/// Per column of every row it compares the base and its marks, the wide tail
/// or a blank, and it compares the cursor. It expects one cell per cluster of
/// the standard's, holding the cluster and taking its first codepoint's width
/// (`Width::of`, which `width.rs` holds to East Asian Width), with the one
/// policy `policy_screen` states applied. Every line where that policy
/// departs from the standard is pinned by number, and so is every line where
/// the grid departs from the policy, with the reason; nothing is skipped.
///
/// **Every read size**, one byte to the whole line, so a character split
/// across reads, the bulk writer `put_run` and the single-character
/// `put_one` all have to agree.
#[test]
fn every_line_of_the_suite_clusters_through_the_terminal() {
    let all = cases();
    // **Every control in the file is NUL, LF or CR**, and those are exactly
    // the lines the break property calls Control, CR or LF. `is_control` is
    // general category Cc: C0, DEL and C1.
    let control = |v: &u32| is_control(Codepoint::new(*v).expect("a scalar value"));
    let held = all.iter().filter(|c| c.cps.iter().any(control)).count();
    assert_eq!(
        (all.len(), held),
        (766, 211),
        "the suite or the control filter moved"
    );
    for c in all.iter().flat_map(|c| &c.cps).filter(|v| control(v)) {
        assert!(
            matches!(c, 0x00 | 0x0a | 0x0d),
            "U+{c:04X}: a control `policy_screen` does not model"
        );
    }
    for c in &all {
        let by_class = c.cps.iter().any(|v| {
            matches!(
                GraphemeClass::of(Codepoint::new(*v).expect("a scalar value")),
                GraphemeClass::Cr | GraphemeClass::Lf | GraphemeClass::Control
            )
        });
        assert_eq!(
            by_class,
            c.cps.iter().any(control),
            "line {}: a control by one test and not the other",
            c.line
        );
    }

    let mut wrong = Vec::new();
    let mut zero_alone = Vec::new();
    let mut zero_after = Vec::new();
    let mut found: Vec<(usize, Before)> = Vec::new();
    for before in Before::ALL {
        for c in &all {
            let clusters = standard_clusters(before, c);
            if clusters
                .iter()
                .any(|k| !is_control(k[0]) && width_of(k[0]) == Width::ZERO)
            {
                match before {
                    Before::Nothing => zero_alone.push(c.line),
                    Before::TwoLetters => zero_after.push(c.line),
                    Before::ThreeLetters => {}
                }
            }
            let (want, at) = policy_screen(&clusters);
            let bytes: Vec<u8> = clusters
                .iter()
                .flatten()
                .map(|cp| char::from_u32(cp.value()).expect("a scalar value"))
                .collect::<String>()
                .into_bytes();
            let mut sizes = Vec::new();
            let mut first = None;
            for read in 1..=bytes.len() {
                let (screen, cursor, _) = through_term(&bytes, ReadLen::from_index(read));
                if screen != want || cursor != at {
                    sizes.push(read);
                    first.get_or_insert((screen, cursor));
                }
            }
            let Some((screen, cursor)) = first else {
                continue;
            };
            found.push((c.line, before));
            let known = KNOWN
                .iter()
                .find(|(l, b, _)| *l == c.line && b.contains(&before));
            match known {
                Some((_, _, Why::AsciiAfterPrepend)) => {
                    let after_prepend = clusters.iter().any(|k| {
                        k.windows(2).any(|w| {
                            GraphemeClass::of(w[0]) == GraphemeClass::Prepend
                                && char::from_u32(w[1].value())
                                    .is_some_and(|l| l.is_ascii() && !l.is_ascii_control())
                        })
                    });
                    assert!(
                        after_prepend,
                        "line {}: pinned as ASCII after a Prepend, and it has none",
                        c.line
                    );
                }
                None => {
                    // The used part of each row, so a failure reads as rows.
                    let used = |rows: &[Vec<Column>]| -> Vec<Vec<Column>> {
                        let n = want
                            .iter()
                            .chain(&screen)
                            .filter_map(|r| r.iter().rposition(|c| *c != Column::Blank))
                            .max()
                            .map_or(0, |p| p + 1);
                        rows.iter().map(|r| r[..n].to_vec()).collect()
                    };
                    wrong.push(format!(
                        "line {} {:?}, at read sizes {:?} of {}: {}\n      want {:?}, cursor {:?}\n      got  {:?}, cursor {:?}",
                        c.line,
                        before,
                        sizes,
                        bytes.len(),
                        c.cps.iter().map(|v| format!("{v:04X}")).collect::<Vec<_>>().join(" "),
                        used(&want),
                        at,
                        used(&screen),
                        cursor
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} line-and-prefix pairs render other than the standard's clusters:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
    let gone: Vec<_> = KNOWN
        .iter()
        .flat_map(|(l, bs, why)| bs.iter().map(move |b| (*l, *b, *why)))
        .filter(|(l, b, _)| !found.contains(&(*l, *b)))
        .collect();
    assert!(
        gone.is_empty(),
        "pinned departures that now render as the standard says -- move the pin: {gone:?}"
    );
    assert_eq!(
        zero_alone,
        spread(ZERO_WIDTH_STARTS_ALONE),
        "the lines starting a cluster with a zero-width codepoint, alone"
    );
    assert_eq!(
        zero_after,
        spread(ZERO_WIDTH_STARTS_AFTER_LETTERS),
        "the lines starting a cluster with a zero-width codepoint, after a letter"
    );
}

/// [`standard_clusters`] by the legacy rules instead: `[[source: uax29]]`'s
/// "legacy grapheme cluster", the extended rules without GB9a, GB9b and
/// GB9c, walked by `ClusterRules::is_break_in` from the start of text over
/// the letters before the line and the line, and baud's one tailoring of
/// them: a spacing mark UAX #29 classes Extend (`baud_domain::is_spacing_extend`)
/// starts a cluster of its own, the rules started again at it.
fn legacy_clusters(before: Before, c: &Case) -> Vec<Vec<Codepoint>> {
    let mut rules = baud_domain::ClusterRules::new();
    let mut out: Vec<Vec<Codepoint>> = Vec::new();
    let all = before
        .letters()
        .chars()
        .map(u32::from)
        .chain(c.cps.iter().copied());
    for v in all {
        let cp = Codepoint::new(v).expect("the suite names scalar values");
        let (class, lb) = GraphemeClass::of_with_lookback(cp);
        let mut brk = rules.is_break_in(baud_domain::ClusterCells::Legacy, class, lb);
        if !brk && class == GraphemeClass::Extend && baud_domain::is_spacing_extend(cp) {
            rules = baud_domain::ClusterRules::new();
            brk = rules.is_break_in(baud_domain::ClusterCells::Legacy, class, lb);
        }
        match out.last_mut() {
            Some(k) if !brk => k.push(cp),
            _ => out.push(vec![cp]),
        }
    }
    out
}

/// **While `?2027` is reset every line of the suite parts as the legacy
/// rules say**: one cell a legacy cluster, through the same policy and at
/// every read size, so a spacing mark, a consonant after a virama and a
/// character after a Prepend each take a cell. GB9b's departure for ASCII
/// after a Prepend, pinned for the extended clusters above, is none here:
/// the legacy rules break there too.
#[test]
fn every_line_of_the_suite_parts_as_the_legacy_rules_say() {
    let mut wrong = Vec::new();
    let mut parted = 0usize;
    for before in Before::ALL {
        for c in &cases() {
            let clusters = legacy_clusters(before, c);
            if clusters != standard_clusters(before, c) {
                parted += 1;
            }
            let (want, at) = policy_screen(&clusters);
            let bytes: Vec<u8> = clusters
                .iter()
                .flatten()
                .map(|cp| char::from_u32(cp.value()).expect("a scalar value"))
                .collect::<String>()
                .into_bytes();
            for read in 1..=bytes.len() {
                let (screen, cursor, _) =
                    through_term_in(Cells::Legacy, &bytes, ReadLen::from_index(read));
                if screen != want || cursor != at {
                    wrong.push(format!(
                        "line {} {before:?}, read {read}: {:04X?}",
                        c.line, c.cps
                    ));
                    break;
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} line-and-prefix pairs render other than the legacy clusters:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
    // A positive control: the legacy rules part some lines, so the two
    // tests do not compare the same clusters.
    assert!(parted > 100, "only {parted} lines part");
}

/// **A continuation with a width joins the ASCII letter before it**, by
/// either of `Paint::piece`'s branches: after `a`, a character at a time, and
/// after `abc`, through `Paint::blit`. GB9a joins a SpacingMark, `U+0903`;
/// GB9 joins an Extend, `U+09BE`, a vowel sign of category Mc, and `U+1F3FF`,
/// an emoji modifier two columns wide. Each took a cell of its own after the
/// letter while `piece` and `blit` told the machine nothing came before it.
/// After `é`, class Other too and written through the machine, each attached
/// throughout, which is the control. The suite has no `U+09BE` case, so this
/// is what holds it.
///
/// **And where the run ends.** After 40 letters the run's last byte falls
/// inside a sixteen-byte vector of `run_of_text`'s scan rather than at its
/// start. After 64 the row is full and the cursor sits on the pending-wrap
/// sentinel, so the mark has to find the letter in the last column. After 70
/// the run goes through `Paint::wrapped`, and the letter is on the next row.
#[test]
fn a_continuation_with_a_width_joins_the_ascii_letter_before_it() {
    let cp = |c: char| Codepoint::new(u32::from(c)).expect("a char is a codepoint");
    let letters = |n: usize| -> String { ('a'..='z').cycle().take(n).collect() };
    let cols = usize::from(SUITE_WS.ws_col);
    let befores = [
        "a".to_string(),
        "abc".to_string(),
        "\u{e9}".to_string(),
        letters(40),
        letters(cols),
        letters(70),
    ];
    let mut wrong = Vec::new();
    for mark in ['\u{903}', '\u{9be}', '\u{1f3ff}'] {
        for before in &befores {
            let text: String = before.chars().chain([mark]).collect();
            let letter = before.chars().last().expect("a letter");
            let n = before.chars().count();
            let (y, x) = ((n - 1) / cols, (n - 1) % cols);
            // The cursor after the letter, or on the last column when the
            // letter filled the row: `Term::cursor` clamps the pending-wrap
            // sentinel to it.
            let cursor = (Row::from_index(y), Col::at_index((x + 1).min(cols - 1)));
            let mut screen = vec![vec![Column::Blank; cols]; usize::from(SUITE_WS.ws_row)];
            for (i, l) in before.chars().enumerate() {
                screen[i / cols][i % cols] = Column::Glyph(vec![cp(l)]);
            }
            screen[y][x] = Column::Glyph(vec![cp(letter), cp(mark)]);
            let sizes: Vec<usize> = (1..=text.len())
                .filter(|read| {
                    let (got, at, _) = through_term(text.as_bytes(), ReadLen::from_index(*read));
                    (got, at) != (screen.clone(), cursor)
                })
                .collect();
            if !sizes.is_empty() {
                wrong.push(format!(
                    "{n} characters ending {letter:?} + {mark:?}, in reads of {sizes:?}"
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "not one cell with the mark on the letter:\n  {}",
        wrong.join("\n  ")
    );
}

/// Where a function leaves the cursor and what it does to the two letters
/// before it, by the text that defines it.
#[derive(Clone, Copy, Debug)]
enum Effect {
    /// The cursor moves to this row and column; nothing is erased.
    MovesTo(usize, usize),
    /// The cursor stays and these columns of its row are erased.
    Erases(usize, usize),
    /// The cursor stays and the screen scrolls down a line (RI at the top
    /// margin).
    ScrollsDown,
    /// Nothing on the screen changes.
    Stays,
}

/// **After a control function the next character starts a cluster of its
/// own**, for every C0 control in Ground and for every function that moves
/// the cursor or erases, after an ASCII letter and after a letter that is not
/// ASCII.
///
/// `[[source: uax29]]` GB4 and GB5 break after and before Control, CR and LF,
/// and every control function is introduced by one: a C0 byte, or `ESC`,
/// itself a C0 control. The machine was told of none of them, so it went on
/// saying a letter came before. A continuation with a width then reached
/// `Paint::attach`, which joins it to the cell before the cursor: after a
/// line feed, a tab, a forward move or an erase that cell is empty, and the
/// character was dropped and counted in `marks_dropped`; after a backward
/// move it joined a letter the function had stepped back over. After `é`,
/// which goes through the machine, that was so before printable ASCII told
/// the machine anything, which is why `é` is a probe and not a control.
///
/// The functions are every C0 control in Ground, `DEL`, a `CAN` cancelling a
/// sequence, the cursor moves and erasures by `CSI`, `IND`, `NEL` and `RI`
/// by `ESC`, VT52's cursor right and direct address, and a control string, a
/// mode change and a query, which move and erase nothing and are control
/// functions all the same. Three continuations, each of which the rules
/// join to a letter: `U+0903` (GB9a, a SpacingMark), `U+09BE` (GB9, an
/// Extend of category Mc) and `U+1F3FF` (GB9, an emoji modifier two columns
/// wide). Every read size. Each site that tells the machine has a case here
/// that fails without it; `CR` and `CR LF` land on column 0, where a
/// continuation takes a cell whatever the machine says, so theirs is
/// `a_carriage_return_ends_a_regional_indicator_pair`.
///
/// **REP is pinned the other way, in `after_rep_the_cluster_goes_on`**: it
/// writes a graphic character, so the
/// continuation joins the copy. A plain SGR is a control sequence like the
/// rest and ends the cluster (GB4 on its `ESC`).
#[test]
fn a_control_function_ends_the_cluster_before_it() {
    let cp = |c: char| Codepoint::new(u32::from(c)).expect("a char is a codepoint");
    let (rows, cols) = (usize::from(SUITE_WS.ws_row), usize::from(SUITE_WS.ws_col));
    // The letters are written at row 0, columns 0 and 1; the cursor is at 2.
    // Each function is the bytes before the letters, which only VT52 uses,
    // and the bytes after them.
    let ansi = |name: &str, f: &[u8], e: Effect| (name.to_string(), Vec::new(), f.to_vec(), e);
    let mut functions: Vec<(String, Vec<u8>, Vec<u8>, Effect)> = vec![
        ansi("LF", b"\n", Effect::MovesTo(1, 2)),
        ansi("VT", b"\x0b", Effect::MovesTo(1, 2)),
        ansi("FF", b"\x0c", Effect::MovesTo(1, 2)),
        ansi("CR", b"\r", Effect::MovesTo(0, 0)),
        ansi("CR LF", b"\r\n", Effect::MovesTo(1, 0)),
        ansi("HT", b"\t", Effect::MovesTo(0, 8)),
        ansi("BS", b"\x08", Effect::MovesTo(0, 1)),
        ansi("CHA 5", b"\x1b[5G", Effect::MovesTo(0, 4)),
        ansi("CUF 3", b"\x1b[3C", Effect::MovesTo(0, 5)),
        ansi("CUB 1", b"\x1b[D", Effect::MovesTo(0, 1)),
        ansi("CUP 2;4", b"\x1b[2;4H", Effect::MovesTo(1, 3)),
        ansi("ED 2", b"\x1b[2J", Effect::Erases(0, 2)),
        ansi("EL 1", b"\x1b[1K", Effect::Erases(0, 2)),
        // A designation, an intermediate and a final, which moves nothing.
        ansi("SCS G0 ASCII", b"\x1b(B", Effect::Stays),
        ansi("IND", b"\x1bD", Effect::MovesTo(1, 2)),
        ansi("NEL", b"\x1bE", Effect::MovesTo(1, 0)),
        ansi("RI", b"\x1bM", Effect::ScrollsDown),
        // A control string moves nothing; it is a control function, and
        // GB4 breaks on the `ESC` that opens it.
        ansi("OSC 0", b"\x1b]0;t\x07", Effect::Stays),
        ansi("OSC 0 ST", b"\x1b]0;t\x1b\\", Effect::Stays),
        // Nor does a mode change or a query, or a rendition: an `SGR` is the
        // commonest sequence there is and a control sequence all the same,
        // whether `fast_csi` takes it whole or the graph walks it.
        ansi("DECTCEM", b"\x1b[?25l", Effect::Stays),
        ansi("SGR 31", b"\x1b[31m", Effect::Stays),
        ansi("SGR", b"\x1b[m", Effect::Stays),
        // A sequence `CAN` cancels: nothing is performed but the `CAN`.
        ansi("CSI CAN", b"\x1b[\x18", Effect::Stays),
        // A sequence that breaks its own grammar is still a control sequence:
        // `[[source: williams]]`'s `csi ignore` swallows it to its final byte
        // and performs nothing. A marker after a parameter, an intermediate
        // before one, and an empty sub-parameter, which is an `SGR`.
        ansi("CSI 1 ? m", b"\x1b[1?m", Effect::Stays),
        ansi("CSI 1 < m", b"\x1b[1<m", Effect::Stays),
        ansi("CSI SP 1 m", b"\x1b[ 1m", Effect::Stays),
        ansi("CSI : m", b"\x1b[:m", Effect::Stays),
        // An escape sequence with a `DEL` in it, which it ignores and goes
        // on (`[[source: williams]]` 286): `ESC DEL ( B` designates ASCII.
        // `ESC DEL` alone ended the sequence until 2026-10-11, and now leaves
        // it open for the continuation's lead byte.
        ansi("ESC DEL ( B", b"\x1b\x7f(B", Effect::Stays),
        // VT52, entered before the letters so that nothing but the function
        // stands between them and the continuation: cursor right, and a
        // direct address to the first row and the sixth column.
        (
            "VT52 ESC C".into(),
            b"\x1b[?2l".to_vec(),
            b"\x1bC".to_vec(),
            Effect::MovesTo(0, 3),
        ),
        (
            "VT52 ESC Y".into(),
            b"\x1b[?2l".to_vec(),
            b"\x1bY\x20\x25".to_vec(),
            Effect::MovesTo(0, 5),
        ),
        // Entering VT52 and leaving it, the two functions that set the node
        // themselves rather than through an edge.
        ansi("DECANM", b"\x1b[?2l", Effect::Stays),
        (
            "VT52 ESC <".into(),
            b"\x1b[?2l".to_vec(),
            b"\x1b<".to_vec(),
            Effect::Stays,
        ),
    ];
    // **Every C0 control in Ground that moves nothing**, each by its byte:
    // NUL, which the parser discards, the bell, the locking shifts, which
    // change only what printable ASCII is drawn from, CAN and SUB, which have
    // nothing to cancel there, and the rest, which do nothing; and DEL, which
    // is ignored and which the break property calls Control too.
    for b in (0x00u8..=0x1f).chain([0x7f]) {
        if !matches!(b, 0x08..=0x0d | 0x1b) {
            functions.push((format!("{b:#04x}"), Vec::new(), vec![b], Effect::Stays));
        }
    }
    let mut wrong = Vec::new();
    for mark in ['\u{903}', '\u{9be}', '\u{1f3ff}'] {
        for letters in ["ab", "a\u{e9}"] {
            for (name, enter, f, effect) in &functions {
                let mut bytes = enter.clone();
                bytes.extend(letters.as_bytes());
                bytes.extend(f);
                bytes.extend(mark.to_string().as_bytes());
                let mut want = vec![vec![Column::Blank; cols]; rows];
                for (i, l) in letters.chars().enumerate() {
                    want[0][i] = Column::Glyph(vec![cp(l)]);
                }
                let (r, c) = match *effect {
                    Effect::MovesTo(r, c) => (r, c),
                    Effect::Erases(from, to) => {
                        for cell in &mut want[0][from..to] {
                            *cell = Column::Blank;
                        }
                        (0, 2)
                    }
                    Effect::ScrollsDown => {
                        want.rotate_right(1);
                        (0, 2)
                    }
                    Effect::Stays => (0, 2),
                };
                // Over a letter, where the function moved back: a wide one
                // takes the column after too.
                let w = put_cell(&mut want, (r, c), &[cp(mark)]);
                let at = (Row::from_index(r), Col::at_index(c + w));
                let sizes: Vec<usize> = (1..=bytes.len())
                    .filter(|read| {
                        let (got, cursor, _) = through_term(&bytes, ReadLen::from_index(*read));
                        (got, cursor) != (want.clone(), at)
                    })
                    .collect();
                if !sizes.is_empty() {
                    let (got, cursor, dropped) = through_term(&bytes, ReadLen::from_index(1));
                    let row = |s: &[Vec<Column>], y: usize| s[y][..10].to_vec();
                    wrong.push(format!(
                        "{letters:?} {name} {mark:?}, in reads of {sizes:?}: want {:?} / {:?} at {at:?}, got {:?} / {:?} at {cursor:?}, {dropped} dropped",
                        row(&want, 0),
                        row(&want, 1),
                        row(&got, 0),
                        row(&got, 1),
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} cases did not start a cluster after the function:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

/// **REP, after which the cluster goes on from the copy**, pinned.
///
/// REP writes the letter again, so the continuation joins the copy: `ab`
/// `CSI b` `U+0903` is `a`, `b`, and `b` with the visarga. `Paint::repeat`
/// writes each copy without asking the machine, then tells the machine one
/// copy went past, starting it from the state a control leaves, and
/// `csi_final` puts the parser back in `Node::Ground` rather than
/// `Node::AfterControl`, so nothing undoes that. After a line feed, which
/// REP's copy follows, the continuation joins the copy too.
#[test]
fn after_rep_the_cluster_goes_on() {
    let cp = |c: char| Codepoint::new(u32::from(c)).expect("a char is a codepoint");
    let (rows, cols) = (usize::from(SUITE_WS.ws_row), usize::from(SUITE_WS.ws_col));
    let mut wrong = Vec::new();
    for mark in ['\u{903}', '\u{9be}', '\u{1f3ff}'] {
        for letters in ["ab", "a\u{e9}"] {
            let (a, b) = {
                let mut it = letters.chars();
                (it.next().expect("two"), it.next().expect("two"))
            };
            let cell = |l: char| Column::Glyph(vec![cp(l)]);
            let joined = |l: char| Column::Glyph(vec![cp(l), cp(mark)]);
            let cases = [
                (
                    "REP",
                    &b"\x1b[b"[..],
                    vec![(0, 0, cell(a)), (0, 1, cell(b)), (0, 2, joined(b))],
                    (0, 3),
                ),
                (
                    "LF REP",
                    &b"\n\x1b[b"[..],
                    vec![(0, 0, cell(a)), (0, 1, cell(b)), (1, 2, joined(b))],
                    (1, 3),
                ),
            ];
            for (name, f, cells, (r, c)) in cases {
                let mut bytes = letters.as_bytes().to_vec();
                bytes.extend(f);
                bytes.extend(mark.to_string().as_bytes());
                let mut want = vec![vec![Column::Blank; cols]; rows];
                for (y, x, k) in cells {
                    want[y][x] = k;
                }
                let at = (Row::from_index(r), Col::at_index(c));
                let sizes: Vec<usize> = (1..=bytes.len())
                    .filter(|read| {
                        let (got, cursor, _) = through_term(&bytes, ReadLen::from_index(*read));
                        (got, cursor) != (want.clone(), at)
                    })
                    .collect();
                if !sizes.is_empty() {
                    wrong.push(format!(
                        "{letters:?} {name} {mark:?}, in reads of {sizes:?}"
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "the cluster did not go on:\n  {}",
        wrong.join("\n  ")
    );
}

/// **A REP with nothing to repeat is still a control, and the one before it
/// still counts.** After `RIS` there is no last graphic character, so
/// `Paint::repeat` writes nothing and tells the machine nothing; `csi_final`
/// must then leave the parser on `Node::AfterControl`, where the `CUP` put it,
/// and not move it to `Node::Ground` as it does after a copy, or the machine
/// judges the mark from the letter written before the reset. So `ab RIS
/// CUP 1;5 CSI b U+0903` is a blank screen and the mark in a cell of its own
/// at the fifth column.
#[test]
fn a_rep_with_nothing_to_repeat_leaves_the_control_before_it_in_force() {
    let cp = |c: char| Codepoint::new(u32::from(c)).expect("a char is a codepoint");
    let (rows, cols) = (usize::from(SUITE_WS.ws_row), usize::from(SUITE_WS.ws_col));
    let mut wrong = Vec::new();
    for mark in ['\u{903}', '\u{9be}', '\u{1f3ff}'] {
        for letters in ["ab", "a\u{e9}"] {
            let mut bytes = letters.as_bytes().to_vec();
            bytes.extend(b"\x1bc\x1b[1;5H\x1b[b");
            bytes.extend(mark.to_string().as_bytes());
            let mut want = vec![vec![Column::Blank; cols]; rows];
            let w = put_cell(&mut want, (0, 4), &[cp(mark)]);
            let at = (Row::from_index(0), Col::at_index(4 + w));
            let sizes: Vec<usize> = (1..=bytes.len())
                .filter(|read| {
                    let (got, cursor, _) = through_term(&bytes, ReadLen::from_index(*read));
                    (got, cursor) != (want.clone(), at)
                })
                .collect();
            if !sizes.is_empty() {
                wrong.push(format!("{letters:?} {mark:?}, in reads of {sizes:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **CR and CR LF end a regional-indicator pair**, which the column-0 test in
/// `glyph` cannot show for a mark: after a carriage return a continuation at
/// column 0 takes a cell whatever the machine says. A regional indicator is
/// not a continuation; whether it pairs is GB12/13's parity, which a control
/// resets. So `U+1F1E6 CR U+1F1E7 U+1F1E8` is one flag, `BC`, over the `A`:
/// with the parity carried across the CR the `B` closed `A`'s pair and the
/// `C` took a cell of its own. `CR` is the one control whose edge leads to
/// `Node::Ground` and not `Node::AfterControl`, so the machine is not told of
/// it; `Paint::restart_cluster` starts it again when a character the machine
/// would continue lands at column 0, and this is the test that fails without
/// it.
#[test]
fn a_carriage_return_ends_a_regional_indicator_pair() {
    let ri = |c: u32| Codepoint::new(c).expect("a regional indicator");
    let (rows, cols) = (usize::from(SUITE_WS.ws_row), usize::from(SUITE_WS.ws_col));
    let (a, b, c) = (ri(0x1f1e6), ri(0x1f1e7), ri(0x1f1e8));
    let utf8 = |v: Codepoint| char::from_u32(v.value()).expect("a scalar").to_string();
    let mut wrong = Vec::new();
    for (name, f, row) in [("CR", "\r", 0usize), ("CR LF", "\r\n", 1)] {
        let text = format!("{}{f}{}{}", utf8(a), utf8(b), utf8(c));
        let mut want = vec![vec![Column::Blank; cols]; rows];
        if row == 1 {
            put_cell(&mut want, (0, 0), &[a]);
        }
        put_cell(&mut want, (row, 0), &[b, c]);
        let at = (Row::from_index(row), Col::at_index(2));
        for read in 1..=text.len() {
            let (got, cursor, _) = through_term(text.as_bytes(), ReadLen::from_index(read));
            if (got.clone(), cursor) != (want.clone(), at) {
                wrong.push(format!(
                    "{name}, read {read}: got {:?} / {:?} at {cursor:?}",
                    &got[0][..6],
                    &got[1][..6]
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **A flag after a Prepend is still a flag**: `[[source: uax29]]`
/// § 3.1.1, "do not break between regional indicator (RI) symbols if there
/// is an odd number of RI characters before the break point. GB12 sot
/// (RI RI)* RI × RI GB13 [^RI] (RI RI)* RI × RI". U+0600 ARABIC NUMBER
/// SIGN is `[^RI]`. While `?2027` is set the sign and the flag are one
/// cluster, `÷ 0600 × 1F1E6 × 1F1E7 ÷` (GB9b, then GB13), so one cell holds
/// all three, and a third indicator takes a cell of its own; while it is
/// reset the legacy rules, without GB9b, give `÷ 0600 ÷ 1F1E6 × 1F1E7 ÷`,
/// the sign in one cell and the flag in the next. At every read size.
#[test]
fn a_flag_after_a_prepend_is_one_cluster() {
    let scalar = |v: u32| Codepoint::new(v).expect("a scalar value");
    let (p, a, b, c) = (
        scalar(0x0600),
        scalar(0x1f1e6),
        scalar(0x1f1e7),
        scalar(0x1f1e8),
    );
    let mut wrong = Vec::new();
    for (cells, clusters) in [
        (Cells::Extended, vec![vec![p, a, b]]),
        (Cells::Extended, vec![vec![p, a, b], vec![c]]),
        (Cells::Legacy, vec![vec![p], vec![a, b]]),
        (Cells::Legacy, vec![vec![p], vec![a, b], vec![c]]),
    ] {
        let (want, at) = policy_screen(&clusters);
        let bytes: Vec<u8> = clusters
            .iter()
            .flatten()
            .map(|cp| char::from_u32(cp.value()).expect("a scalar value"))
            .collect::<String>()
            .into_bytes();
        for read in 1..=bytes.len() {
            let (got, cursor, _) = through_term_in(cells, &bytes, ReadLen::from_index(read));
            if (got[0].clone(), cursor) != (want[0].clone(), at) {
                wrong.push(format!(
                    "{cells:?} {clusters:X?}, read {read}: got {:?} at {cursor:?}",
                    &got[0][..6]
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **A replacement character is class Other to the cluster after it**, after
/// a control as much as anywhere. `U+FFFD` is `Grapheme_Cluster_Break` Other
/// and not `Extended_Pictographic`, so GB9a joins a `U+0903` to it, GB999
/// breaks a regional indicator after it, and GB11 does not reach across it:
/// `é CUF 3 80 U+0903` is the replacement with the visarga in one cell, and
/// `U+1F1E6 80 U+1F1E7` and `U+1F468 FF U+200D U+1F469` are three cells each,
/// the joiner on the replacement.
///
/// The replacement is written by `Act::Bad` for a byte that is no character,
/// by `Act::Resync` for a character a read left unfinished, and by `chars`
/// for a lead whose character is cut short in the read. Each of the three
/// asks the machine about U+FFFD as about any character
/// (`a_replacement_for_ill_formed_bytes_clusters_as_a_literal_one`). Written
/// without asking, the next character would be judged from what came before
/// the replacement: from the control, so the visarga would take a cell of its
/// own, or from the indicator or the pictographic, so the second indicator
/// would pair with the first and the woman would join the man.
/// Every read size, so the unfinished character takes `Resync` as well as
/// `chars`.
#[test]
fn a_replacement_character_is_class_other_to_the_cluster_after_it() {
    let cp = |v: u32| Codepoint::new(v).expect("a scalar value");
    let (rows, cols) = (usize::from(SUITE_WS.ws_row), usize::from(SUITE_WS.ws_col));
    let fffd = 0xfffd;
    // Each case: its name, its bytes, and the clusters it writes, each at a
    // row and a column; the cursor ends after the last.
    type Cluster = (usize, usize, Vec<u32>);
    let mut cases: Vec<(String, Vec<u8>, Vec<Cluster>)> = Vec::new();
    for letter in ['a', '\u{e9}'] {
        let l = u32::from(letter);
        let mut text = String::new();
        text.push(letter);
        for (name, between, at) in [
            ("80", &b"\x80"[..], (0, 1)),
            ("CUF 3, 80", &b"\x1b[3C\x80"[..], (0, 4)),
            ("LF, FF", &b"\n\xff"[..], (1, 1)),
            ("CUF 3, C3", &b"\x1b[3C\xc3"[..], (0, 4)),
            ("CUF 3, E2 82", &b"\x1b[3C\xe2\x82"[..], (0, 4)),
        ] {
            let mut bytes = text.as_bytes().to_vec();
            bytes.extend(between);
            bytes.extend("\u{903}".as_bytes());
            cases.push((
                format!("{letter:?} {name} U+0903"),
                bytes,
                vec![(0, 0, vec![l]), (at.0, at.1, vec![fffd, 0x903])],
            ));
        }
    }
    for (name, between) in [
        ("80", &b"\x80"[..]),
        ("C3", &b"\xc3"[..]),
        ("E2 82", &b"\xe2\x82"[..]),
    ] {
        let mut bytes = "\u{1f1e6}".as_bytes().to_vec();
        bytes.extend(between);
        bytes.extend("\u{1f1e7}".as_bytes());
        cases.push((
            format!("U+1F1E6 {name} U+1F1E7"),
            bytes,
            vec![
                (0, 0, vec![0x1f1e6]),
                (0, 2, vec![fffd]),
                (0, 3, vec![0x1f1e7]),
            ],
        ));
        let mut bytes = "\u{1f468}".as_bytes().to_vec();
        bytes.extend(between);
        bytes.extend("\u{200d}\u{1f469}".as_bytes());
        cases.push((
            format!("U+1F468 {name} U+200D U+1F469"),
            bytes,
            vec![
                (0, 0, vec![0x1f468]),
                (0, 2, vec![fffd, 0x200d]),
                (0, 3, vec![0x1f469]),
            ],
        ));
    }
    let mut ff = "\u{1f468}".as_bytes().to_vec();
    ff.push(0xff);
    ff.extend("\u{200d}\u{1f469}".as_bytes());
    cases.push((
        "U+1F468 FF U+200D U+1F469".into(),
        ff,
        vec![
            (0, 0, vec![0x1f468]),
            (0, 2, vec![fffd, 0x200d]),
            (0, 3, vec![0x1f469]),
        ],
    ));
    let mut wrong = Vec::new();
    for (name, bytes, clusters) in &cases {
        let mut want = vec![vec![Column::Blank; cols]; rows];
        let mut at = (0, 0);
        for (r, c, k) in clusters {
            let k: Vec<Codepoint> = k.iter().map(|v| cp(*v)).collect();
            at = (*r, c + put_cell(&mut want, (*r, *c), &k));
        }
        let at = (Row::from_index(at.0), Col::at_index(at.1));
        let sizes: Vec<usize> = (1..=bytes.len())
            .filter(|read| {
                let (got, cursor, _) = through_term(bytes, ReadLen::from_index(*read));
                (got, cursor) != (want.clone(), at)
            })
            .collect();
        if !sizes.is_empty() {
            let (got, cursor, _) = through_term(bytes, ReadLen::from_index(bytes.len()));
            wrong.push(format!(
                "{name}, in reads of {sizes:?}: want {:?} / {:?} at {at:?}, got {:?} / {:?} at {cursor:?} whole",
                &want[0][..6],
                &want[1][..6],
                &got[0][..6],
                &got[1][..6],
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} cases judged the cluster across a replacement:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

/// **What a run of printable ASCII writes is class Other with no lookback,
/// under every character set a designation can name, but for three
/// codepoints**, which is what makes `ClusterBreak::after_ascii_run` the
/// truthful state after it. A run is written through the designated set, so
/// DEC graphics and the national and DEC sets put other codepoints in its
/// cells. Every one-byte final and every intermediate-and-final pair, every
/// printable byte; the control is `U+00A9`, which is `Extended_Pictographic`
/// and so carries a lookback this check would see.
///
/// **The three, since DEC Supplemental and JIS-Katakana are translated
/// (2026-10-12)**: DEC Supplemental 2/9 is U+00A9 itself, which opens GB11's
/// lookback, and JIS-Katakana 5/14 and 5/15 are U+FF9E and U+FF9F, the
/// halfwidth sound marks, which are `Extend`. Each is written as the
/// character at its position, in a cell of its own, and the state after the
/// run is still `after_ascii_run`: `U+00A9` then a `ZWJ` and a pictograph in
/// UTF-8 does not join as GB11 joins it, and a sound mark does not join the
/// katakana before it as GB9 does. Whether a translated character enters the
/// cluster machine as the same codepoint in UTF-8 would, or keeps DEC's one
/// code, one position, is open. This asserts the three by name, so a fourth
/// fails here.
#[test]
fn every_character_set_writes_printable_ascii_as_class_other() {
    use baud_domain::{Charset, PackedCell};
    let copyright = Codepoint::new(u32::from('\u{a9}')).expect("a char is a codepoint");
    let (_, lb) = GraphemeClass::of_with_lookback(copyright);
    assert!(!lb.is_none(), "the control: U+00A9 opens GB11's lookback");
    let mut sets = Vec::new();
    for f in b'0'..=b'~' {
        sets.push(Charset::from_final(StreamByte::from_stream(f)));
        for i in b' '..=b'/' {
            sets.push(Charset::from_multi(
                StreamByte::from_stream(i),
                StreamByte::from_stream(f),
            ));
        }
    }
    // The three a translated DEC set writes that are not class Other with no
    // lookback, each with its class and whether it opens a lookback.
    let open = [
        (0x00a9, GraphemeClass::Other, false),
        (0xff9e, GraphemeClass::Extend, false),
        (0xff9f, GraphemeClass::Extend, false),
    ];
    let mut substituted = std::collections::BTreeSet::new();
    let mut seen_open = std::collections::BTreeSet::new();
    for set in &sets {
        for b in b' '..=b'~' {
            let cp = PackedCell::translate(StreamByte::from_stream(b), *set);
            if cp.value() != u32::from(b) {
                substituted.insert(cp.value());
            }
            let (class, lb) = GraphemeClass::of_with_lookback(cp);
            if let Some(&(_, c, none)) = open.iter().find(|(v, _, _)| *v == cp.value()) {
                assert_eq!((class, lb.is_none()), (c, none), "U+{:04X}", cp.value());
                seen_open.insert(cp.value());
                continue;
            }
            assert_eq!(
                (class, lb.is_none()),
                (GraphemeClass::Other, true),
                "{set:?} writes byte {b:#04x} as U+{:04X}",
                cp.value()
            );
        }
    }
    assert_eq!(seen_open.len(), open.len(), "each of the three is written");
    // 132 before DEC Supplemental, JIS-Katakana and JIS-Roman were
    // translated; they add 103 more, counted from the generated tables too.
    assert_eq!(
        substituted.len(),
        235,
        "the sets substitute a different set of codepoints now"
    );
}
