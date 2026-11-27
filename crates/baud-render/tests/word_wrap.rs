//! **A finished paragraph drawn at its words** (`[screen] wrap_words`, on
//! by default): no word is split across two lines, and in a paragraph that
//! mixes directions a run of the other direction the wrap would split is
//! given lines of its own in its own direction.
//!
//! Every expectation is the window as drawn: each display row's cells in
//! visual order, `.` for an erased cell. A right-to-left line is written
//! here by [`rtl`], its characters reversed against the right edge, which
//! is UAX #9's order for a line of Hebrew letters, spaces and a closing
//! period (L2 reverses the level-1 run; the period, between an R and the
//! paragraph's end, is at level 1 by N1-N2).

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_render::{Reorder, Scrolled, Selection, Span, WordWrap};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The shell's prompt mark: what makes the text above it finished.
const PROMPT: &str = "\x1b]133;A\x07$ ";

/// Lines that scroll into the history first, so a re-broken paragraph has
/// lines above it to push up.
fn history(rows: u16) -> String {
    (0..rows).map(|i| format!("h{i}\r\n")).collect()
}

/// A line drawn left to right: its text, then erased cells to `cols`.
fn ltr(s: &str, cols: usize) -> String {
    let n = s.chars().count();
    format!("{s}{}", ".".repeat(cols - n))
}

/// A line drawn right to left against the right edge: erased cells, then
/// its characters reversed.
fn rtl(s: &str, cols: usize) -> String {
    let n = s.chars().count();
    format!(
        "{}{}",
        ".".repeat(cols - n),
        s.chars().rev().collect::<String>()
    )
}

/// Each display row of the window `back` lines up, as drawn, after `bytes`
/// with the display's wrap `wrap`; and the layout.
fn drawn_as(ws: &WinSize, bytes: &str, back: Rows, wrap: WordWrap) -> (Vec<String>, Reorder) {
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    o.set_word_wrap(wrap);
    o.lay_out(&t, back);
    let vp = o.view(&t, back);
    let rows = (0..vp.rows().index())
        .map(|y| {
            let r = Row::from_index(y);
            let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
            o.orders().permute(r, vp.row(r), &mut out);
            out.iter()
                .map(|c| match c.content {
                    x if x.is_empty() => '.',
                    x if x.is_wide_tail() => '~',
                    x => x
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or('?'),
                })
                .collect()
        })
        .collect();
    (rows, o)
}

fn drawn(ws: &WinSize, bytes: &str) -> Vec<String> {
    drawn_as(ws, bytes, Rows::ZERO, WordWrap::On).0
}

const W44: WinSize = WinSize {
    ws_row: 6,
    ws_col: 44,
};

/// The reported paragraph: Hebrew, an English run the terminal's wrap
/// cut as "mid|dle", Hebrew.
const REPORTED: &str = "\u{5D6}\u{5D5} \u{5E4}\u{5E1}\u{5E7}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA} with English words in the middle, \u{5E9}\u{5DE}\u{5DE}\u{5E9}\u{5D9}\u{5DB}\u{5D4} \u{5DC}\u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D4}\u{5D1}\u{5D0}\u{5D4}.";

fn reported_parts() -> (&'static str, &'static str, &'static str) {
    (
        "\u{5D6}\u{5D5} \u{5E4}\u{5E1}\u{5E7}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA} ",
        "with English words in the middle, ",
        "\u{5E9}\u{5DE}\u{5DE}\u{5E9}\u{5D9}\u{5DB}\u{5D4} \u{5DC}\u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D4}\u{5D1}\u{5D0}\u{5D4}.",
    )
}

/// **The reported case**: the English run the wrap split gets a line of
/// its own, flush left and left to right with its comma at its end; the
/// Hebrew before it and after it each on lines of their own against the
/// right edge; the prompt below keeps its row, the line above is pushed
/// into the history.
#[test]
fn the_run_the_wrap_splits_gets_a_line_of_its_own_in_its_own_direction() {
    let (he1, en, he2) = reported_parts();
    let bytes = format!("{}{REPORTED}\r\n{PROMPT}", history(8));
    let got = drawn(&W44, &bytes);
    assert_eq!(
        got,
        [
            ltr("h6", 44),
            ltr("h7", 44),
            rtl(he1, 44),
            ltr(en, 44),
            rtl(he2, 44),
            ltr("$ ", 44),
        ]
    );
}

/// **Off draws every row as the terminal cut it**, exactly as before the
/// setting: "mid" at the left of the first row, "dle" at the right of the
/// second, and no display is made.
#[test]
fn with_the_setting_off_every_row_is_drawn_as_the_terminal_cut_it() {
    let bytes = format!("{}{REPORTED}\r\n{PROMPT}", history(8));
    let (off, o) = drawn_as(&W44, &bytes, Rows::ZERO, WordWrap::Off);
    assert!(o.orders().display().is_none(), "no display with it off");
    let mut owned = Owned::new(&W44);
    let mut t = Term::new(owned.storage(), &W44);
    feed(&mut t, &bytes);
    let mut base = Reorder::new();
    base.lay_out(&t, Rows::ZERO);
    let vp = t.view(Rows::ZERO);
    for (y, row) in off.iter().enumerate() {
        let r = Row::from_index(y);
        let mut out = vec![PackedCell::EMPTY; 44];
        base.orders().permute(r, vp.row(r), &mut out);
        assert_eq!(
            row.chars().count(),
            out.len(),
            "row {y} is the window's width"
        );
    }
    assert!(
        off[3].starts_with("with English words in the mid "),
        "{off:#?}"
    );
    assert!(off[4].ends_with(",dle"), "{off:#?}");
}

/// **An all-Hebrew paragraph wraps at its words**: the word the terminal's
/// wrap split moves whole to the next line.
#[test]
fn an_all_hebrew_paragraph_wraps_at_its_words() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 10,
    };
    // "אבג דהו זחטי כל": the cut at 10 falls inside זחטי.
    let line =
        "\u{5D0}\u{5D1}\u{5D2} \u{5D3}\u{5D4}\u{5D5} \u{5D6}\u{5D7}\u{5D8}\u{5D9} \u{5DB}\u{5DC}";
    let bytes = format!("{}{line}\r\n{PROMPT}", history(6));
    let got = drawn(&ws, &bytes);
    assert_eq!(
        got,
        [
            ltr("h5", 10),
            rtl("\u{5D0}\u{5D1}\u{5D2} \u{5D3}\u{5D4}\u{5D5} ", 10),
            rtl("\u{5D6}\u{5D7}\u{5D8}\u{5D9} \u{5DB}\u{5DC}", 10),
            ltr("$ ", 10),
        ]
    );
}

/// **An all-English paragraph wraps at its words**, its trailing space left
/// hanging on the line it ends.
#[test]
fn an_all_english_paragraph_wraps_at_its_words() {
    let ws = WinSize {
        ws_row: 5,
        ws_col: 12,
    };
    let bytes = format!("{}the quick brown fox jumps\r\n{PROMPT}", history(6));
    let got = drawn(&ws, &bytes);
    assert_eq!(
        got,
        [
            ltr("h5", 12),
            ltr("the quick ", 12),
            ltr("brown fox ", 12),
            ltr("jumps", 12),
            ltr("$ ", 12),
        ]
    );
}

/// **An English paragraph with a Hebrew phrase the wrap splits**: the
/// phrase on a line of its own against the right edge, right to left.
#[test]
fn a_hebrew_phrase_in_an_english_paragraph_sits_at_the_right_edge() {
    let he = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5D5}\u{5D1}\u{5E8}\u{5D5}\u{5DB}\u{5D9}\u{5DD} \u{5D4}\u{5D1}\u{5D0}\u{5D9}\u{5DD} ";
    let text = format!("The sign at the old shop door said {he}in big blue letters.");
    let bytes = format!("{}{text}\r\n{PROMPT}", history(8));
    let got = drawn(&W44, &bytes);
    assert_eq!(
        got,
        [
            ltr("h6", 44),
            ltr("h7", 44),
            ltr("The sign at the old shop door said ", 44),
            rtl(he, 44),
            ltr("in big blue letters.", 44),
            ltr("$ ", 44),
        ]
    );
}

/// **Only the run the wrap would split moves**: "ה-SDK" and "Apple" fit
/// where they are and stay inline; "npm install", which the wrap would
/// split, gets its line, flush left, and the Hebrew resumes on a fresh
/// line (`bd/owner-runs/6-many.png`, B with own direction).
#[test]
fn a_short_run_that_fits_stays_inline() {
    let he = |s: &str| s.to_string();
    let head = he(
        "\u{5DC}\u{5D4}\u{5EA}\u{5D7}\u{5D9}\u{5DC} \u{5DE}\u{5EA}\u{5E7}\u{5D9}\u{5E0}\u{5D9}\u{5DD} \u{5D0}\u{5EA} \u{5D4}-SDK \u{5E9}\u{5DC} Apple \u{5D5}\u{5DE}\u{5E8}\u{5D9}\u{5E6}\u{5D9}\u{5DD} ",
    );
    let tail = "\u{5D1}\u{5EA}\u{5D9}\u{5E7}\u{5D9}\u{5D9}\u{5D4} \u{5E9}\u{5DC} \u{5D4}\u{5E4}\u{5E8}\u{5D5}\u{5D9}\u{5E7}\u{5D8}.";
    let text = format!("{head}npm install {tail}");
    let bytes = format!("{}{text}\r\n{PROMPT}", history(8));
    let got = drawn(&W44, &bytes);
    assert_eq!(got[3], ltr("npm install ", 44), "{got:#?}");
    assert_eq!(got[4], rtl(tail, 44), "{got:#?}");
    assert!(
        got[2].contains("SDK") && got[2].contains("Apple"),
        "{got:#?}"
    );
}

/// **A narrow window**: the English run wraps at a word onto as many lines
/// as it needs, flush left, then the Hebrew resumes on a fresh line
/// (`bd/owner-runs/3b-narrow.png`).
#[test]
fn a_run_longer_than_the_line_wraps_at_its_words_on_lines_of_its_own() {
    let ws = WinSize {
        ws_row: 7,
        ws_col: 28,
    };
    let head = "\u{5D4}\u{5E4}\u{5E7}\u{5D5}\u{5D3}\u{5D4} \u{5D4}\u{5D6}\u{5D0}\u{5EA} \u{5DE}\u{5D3}\u{5E4}\u{5D9}\u{5E1}\u{5D4} \u{5D0}\u{5EA} \u{5D4}\u{5DE}\u{5E9}\u{5E4}\u{5D8} ";
    let tail = "\u{5D5}\u{5D0}\u{5D6} \u{5D9}\u{5D5}\u{5E6}\u{5D0}\u{5EA}.";
    let text = format!("{head}the quick brown fox jumps over the lazy dog {tail}");
    let bytes = format!("{}{text}\r\n{PROMPT}", history(8));
    let got = drawn(&ws, &bytes);
    assert_eq!(
        got,
        [
            ltr("h6", 28),
            ltr("h7", 28),
            rtl(head, 28),
            ltr("the quick brown fox jumps ", 28),
            ltr("over the lazy dog ", 28),
            rtl(tail, 28),
            ltr("$ ", 28),
        ]
    );
}

/// **A number and a URL**: the number, level 2 and no letter, stays on the
/// Hebrew line; the URL the wrap would split -- "https://" fits the rest
/// of the Hebrew line, the whole URL does not -- gets its line, left to
/// right.
#[test]
fn a_url_the_wrap_would_split_gets_its_line_and_a_number_stays() {
    let head = "\u{5D4}\u{5D4}\u{5D6}\u{5DE}\u{5E0}\u{5D4} \u{5DE}\u{5E1}\u{5E4}\u{5E8} 48213 \u{5E0}\u{5E9}\u{5DC}\u{5D7}\u{5D4}. \u{5D1}\u{5DB}\u{5EA}\u{5D5}\u{5D1}\u{5EA} ";
    let tail = "\u{5D5}\u{5D9}\u{5D2}\u{5D9}\u{5E2}\u{5D5} \u{5EA}\u{5D5}\u{5DA} 3 \u{5D9}\u{5DE}\u{5D9}\u{5DD}.";
    let text = format!("{head}https://example.com/orders/48213 {tail}");
    let bytes = format!("{}{text}\r\n{PROMPT}", history(8));
    let got = drawn(&W44, &bytes);
    assert_eq!(
        got[3],
        ltr("https://example.com/orders/48213 ", 44),
        "{got:#?}"
    );
    assert!(
        got[2].contains("48213"),
        "the number on the Hebrew line: {got:#?}"
    );
}

/// **The line being typed is never re-broken**: a paragraph at or below the
/// latest prompt is drawn as the terminal cut it.
#[test]
fn the_paragraph_at_the_prompt_is_drawn_as_stored() {
    let (_, o) = drawn_as(
        &W44,
        &format!("{}{PROMPT}{REPORTED}", history(8)),
        Rows::ZERO,
        WordWrap::On,
    );
    assert!(
        o.orders().display().is_none(),
        "the typed line keeps its cut"
    );
}

/// **Without the shell's marks the live screen is a program's**: nothing on
/// it is re-broken; scrolled back, a paragraph in the history is.
#[test]
fn without_prompt_marks_only_the_history_is_re_broken() {
    let on_screen = format!("{}{REPORTED}\r\nafter\r\n", history(8));
    let (_, live) = drawn_as(&W44, &on_screen, Rows::ZERO, WordWrap::On);
    assert!(
        live.orders().display().is_none(),
        "the live screen as stored"
    );
    let scrolled = format!("{}{REPORTED}\r\n{}", history(2), history(6));
    let (_, back) = drawn_as(&W44, &scrolled, Rows::of_index(3), WordWrap::On);
    assert!(back.orders().display().is_some(), "the history re-broken");
}

/// **The alternate screen is a program's**: never re-broken.
#[test]
fn the_alternate_screen_is_drawn_as_stored() {
    let bytes = format!(
        "{}{REPORTED}\r\n{PROMPT}\x1b[?1049h{REPORTED}\r\n",
        history(8)
    );
    let (_, o) = drawn_as(&W44, &bytes, Rows::ZERO, WordWrap::On);
    assert!(o.orders().display().is_none());
}

/// **At the top of what is kept, a re-broken paragraph that would push a
/// line where nothing scrolls to it keeps the terminal's cut.**
#[test]
fn at_the_top_of_the_history_a_paragraph_keeps_its_cut_rather_than_lose_a_line() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 44,
    };
    let bytes = format!("{REPORTED}\r\n{PROMPT}");
    let (got, o) = drawn_as(&ws, &bytes, Rows::ZERO, WordWrap::On);
    assert!(o.orders().display().is_none(), "{got:#?}");
}

/// **What a pointer, a selection and a copy see**: the stored cells. A
/// pointer on the English line names the stored cell drawn there; the
/// selection between two of them copies the stored text in its logical
/// order; each stored cell maps to the display cell that draws it.
#[test]
fn a_selection_on_a_re_broken_line_copies_the_stored_text() {
    let bytes = format!("{}{REPORTED}\r\n{PROMPT}", history(8));
    let mut owned = Owned::new(&W44);
    let mut t = Term::new(owned.storage(), &W44);
    feed(&mut t, &bytes);
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let orders = o.orders();
    // "with" begins display row 3; it is stored on the paragraph's first
    // row (window row 3) at column 15.
    let (r, c) = orders.pointer(Row::from_index(3), Col::ZERO);
    assert_eq!((r, c), (Row::from_index(3), Col::at_index(15)));
    // "middle," ends at stored row 4, column 3 ("dle,"), drawn at x 32.
    let (r2, c2) = orders.pointer(Row::from_index(3), Col::at_index(32));
    assert_eq!((r2, c2), (Row::from_index(4), Col::at_index(3)));
    assert_eq!(
        orders.shown_at(Row::from_index(4), Col::at_index(3)),
        Some((Row::from_index(3), Col::at_index(32)))
    );
    let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
    let line = |r: Row| scrolled.line(Rows::ZERO, r);
    let row = |r: Row| t.view(Rows::ZERO).row(r);
    let mut sel = Selection::at(Span::cell(row(r), line(r), c), baud_render::Grain::Cell);
    sel.extend(Span::cell(row(r2), line(r2), c2));
    assert_eq!(sel.text(&t, scrolled), "with English words in the middle,");
}

/// **A find mark on a re-broken line is drawn where its cells are**: the
/// stored match "middle," across the stored rows is one run on the
/// English line.
#[test]
fn a_match_across_the_stored_cut_is_one_run_on_its_display_line() {
    let bytes = format!("{}{REPORTED}\r\n{PROMPT}", history(8));
    let mut owned = Owned::new(&W44);
    let mut t = Term::new(owned.storage(), &W44);
    feed(&mut t, &bytes);
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let orders = o.orders();
    // "mid" ends stored row 3 at columns 41..=43; "dle," starts row 4.
    let a = orders.stored_runs(Row::from_index(3), Col::at_index(41), Col::at_index(43));
    let b = orders.stored_runs(Row::from_index(4), Col::ZERO, Col::at_index(3));
    let line = Row::from_index(3);
    assert_eq!(
        a,
        [(line, Col::at_index(26), baud_domain::Cols::of_index(3))]
    );
    assert_eq!(
        b,
        [(line, Col::at_index(29), baud_domain::Cols::of_index(4))]
    );
}

/// **BDSM reset leaves every row as stored**: a program that lays out its
/// own right-to-left text draws the presentation lines it addresses
/// (`[[source: ecma48]]` 7.2.1, EXPLICIT).
#[test]
fn with_bdsm_reset_nothing_is_re_broken() {
    let bytes = format!("\x1b[8l{}{REPORTED}\r\n{PROMPT}", history(8));
    let (_, o) = drawn_as(&W44, &bytes, Rows::ZERO, WordWrap::On);
    assert!(o.orders().display().is_none());
}

/// **An erased cell between characters is a place a line may end**, as a
/// space is: the gap a cursor move left takes the break, and the word after
/// it moves whole.
#[test]
fn an_erased_gap_is_where_the_line_breaks() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 10,
    };
    // "abcd", one cell skipped, "efghijkl": the cut at 10 falls in the word.
    let bytes = format!("{}abcd\x1b[Cefghijkl\r\n{PROMPT}", history(6));
    let got = drawn(&ws, &bytes);
    assert_eq!(
        got,
        [
            ltr("h5", 10),
            ltr("abcd", 10),
            ltr("efghijkl", 10),
            ltr("$ ", 10)
        ]
    );
}

/// **A prompt's exit-status mark moves with its row** where a paragraph
/// below it is re-broken: the earlier prompt's row is pushed up one line,
/// and its mark with it.
#[test]
fn an_exit_status_mark_moves_with_its_prompt() {
    use baud_font::{Face, Px};
    use baud_render::{Blend, Leaving, PxSize, Renderer, Stack, StatusMarks, Theme};
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        baud_domain::Cols::of_index(44),
        Rows::of_index(6),
    )
    .unwrap();
    r.set_status_marks(StatusMarks::On);
    let mut owned = Owned::new(&W44);
    let mut t = Term::new(owned.storage(), &W44);
    // The first prompt on row 2, its command's output the
    // reported paragraph on rows 3 and 4, the open prompt on
    // row 5.
    feed(
        &mut t,
        &format!(
            "{}\x1b]133;A\x07$ \x1b]133;B\x07cat\x1b]133;C\x07\r\n{REPORTED}\r\n\x1b]133;D;0\x07{PROMPT}",
            history(6)
        ),
    );
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let rows = |r: &Renderer<'_, '_>| -> Vec<i32> {
        r.strip_records()
            .iter()
            .map(|i| i32::from(i.y.0).div_euclid(ch))
            .collect()
    };
    // Placed before the frame lays the window out, as the app places them.
    r.set_word_wrap(WordWrap::Off);
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert_eq!(rows(&r), [2], "as stored");
    r.set_word_wrap(WordWrap::On);
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert_eq!(rows(&r), [1], "pushed up with its row");
}

/// **A find mark is drawn on the display cell its match is drawn in**,
/// on a paragraph that holds no right-to-left character, where no row is
/// reordered: the stored match "brown" moved to the next line by the
/// word wrap is washed there, and not at its stored cells.
#[test]
fn a_find_mark_on_an_all_english_paragraph_follows_the_word_wrap() {
    use baud_font::Face;
    use baud_render::{Blend, Current, Find, FindLook, Mark, PxSize, Renderer, Stack, Theme};
    let data = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let faces = [Face::parse(&data).unwrap()];
    let ws = WinSize {
        ws_row: 5,
        ws_col: 12,
    };
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        baud_domain::Cols::of_index(12),
        Rows::of_index(5),
    )
    .unwrap();
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    // "the quick br|own fox jump|s" stored on rows 1-3; drawn "the quick ",
    // "brown fox ", "jumps". "brown" is stored at row 1 cols 10-11 and row 2
    // cols 0-2; it is drawn on display row 2, columns 0-4.
    feed(
        &mut t,
        &format!("{}the quick brown fox jumps\r\n{PROMPT}", history(6)),
    );
    // The marks set before the frame lays the window out, as a driver sets
    // them: the layout that makes the display draws them through it.
    let field = Find::new().field(baud_domain::Cols::of_index(12));
    let mark = |row: usize, from: usize, to: usize| Mark {
        line: baud_render::WindowLine::of(Row::from_index(row)),
        from: Col::at_index(from),
        to: Col::at_index(to),
        current: Current::No,
    };
    r.set_find(
        &t,
        Some(FindLook {
            marks: vec![mark(1, 10, 11), mark(2, 0, 2)],
            field,
        }),
    );
    r.lay_out_bidi(&t, Rows::ZERO);
    let m = r.metrics();
    let (cw, ch) = (
        i32::try_from(m.cell_w.value()).unwrap(),
        i32::try_from(m.cell_h.value()).unwrap(),
    );
    let mut at: Vec<(i32, i32, i32)> = r
        .find_solids()
        .iter()
        .map(|i| {
            (
                i32::from(i.y.0) / ch,
                i32::from(i.x.0) / cw,
                i32::from(i.w.0) / cw,
            )
        })
        .collect();
    at.sort();
    assert_eq!(at, [(2, 0, 2), (2, 2, 3)], "on display row 2, columns 0-4");
}

/// Twenty columns: a Hebrew line of eighteen, its last space hanging.
const W20: WinSize = WinSize {
    ws_row: 6,
    ws_col: 20,
};

/// "אבגדה וזחטי כלמנס ": eighteen columns, so a word of three or more
/// after it does not fit the line.
const HE18: &str = "\u{5D0}\u{5D1}\u{5D2}\u{5D3}\u{5D4} \u{5D5}\u{5D6}\u{5D7}\u{5D8}\u{5D9} \u{5DB}\u{5DC}\u{5DE}\u{5E0}\u{5E1} ";

/// A Hebrew paragraph line whose logical text starts with the English
/// `en` and goes on with the Hebrew `he`: right to left against the
/// right edge, the English run kept in its own order at the line's
/// start, which is its right end.
fn rtl_led_by(en: &str, he: &str, cols: usize) -> String {
    let n = en.chars().count() + he.chars().count();
    format!(
        "{}{}{en}",
        ".".repeat(cols - n),
        he.chars().rev().collect::<String>()
    )
}

/// **A single foreign word is never moved**: "Apple", which does not fit
/// the rest of the Hebrew line, drops to the next line inline as any word
/// does, and the Hebrew goes on after it on that line.
#[test]
fn a_one_word_run_that_does_not_fit_drops_inline() {
    let he2 = " \u{5E2}\u{5E4}\u{5E6} \u{5E7}\u{5E8}\u{5E9}.";
    let bytes = format!("{}{HE18}Apple{he2}\r\n{PROMPT}", history(8));
    let (got, o) = drawn_as(&W20, &bytes, Rows::ZERO, WordWrap::On);
    assert_eq!(
        got,
        [
            ltr("h5", 20),
            ltr("h6", 20),
            ltr("h7", 20),
            rtl(HE18, 20),
            rtl_led_by("Apple", he2, 20),
            ltr("$ ", 20),
        ]
    );
    let moved = o.orders().display().map(|d| d.counts().1);
    assert_eq!(moved, Some(baud_render::Tally::default()), "no run moved");
}

/// **A run the wrap would not split stays inline**: "Apple pie", whose
/// first word misses the Hebrew line but which fits whole on the next,
/// drops there inline, the Hebrew going on after it.
#[test]
fn a_run_that_fits_whole_on_the_next_line_drops_inline() {
    let he2 = " \u{5E2}\u{5E4}\u{5E6}.";
    let bytes = format!("{}{HE18}Apple pie{he2}\r\n{PROMPT}", history(8));
    let (got, o) = drawn_as(&W20, &bytes, Rows::ZERO, WordWrap::On);
    assert_eq!(
        got,
        [
            ltr("h5", 20),
            ltr("h6", 20),
            ltr("h7", 20),
            rtl(HE18, 20),
            rtl_led_by("Apple pie", he2, 20),
            ltr("$ ", 20),
        ]
    );
    let moved = o.orders().display().map(|d| d.counts().1);
    assert_eq!(moved, Some(baud_render::Tally::default()), "no run moved");
}

/// **A paragraph whose words need fewer lines than it is stored in keeps
/// the terminal's cut**: "abcd efghi" and the three spaces the terminal
/// wrapped onto a row of their own would be one line, the spaces hanging,
/// and the window's last row would move up off the cursor's. As stored,
/// the prompt stays on the cursor's row, and a pointer on each display row
/// names the stored row drawn there.
#[test]
fn a_paragraph_that_needs_fewer_lines_keeps_the_prompt_on_the_cursor_row() {
    let ws = WinSize {
        ws_row: 5,
        ws_col: 10,
    };
    let bytes = format!("{}abcd efghi   \r\n{PROMPT}", history(6));
    let (got, o) = drawn_as(&ws, &bytes, Rows::ZERO, WordWrap::On);
    assert_eq!(
        got,
        [
            ltr("h4", 10),
            ltr("h5", 10),
            ltr("abcd efghi", 10),
            ltr("   ", 10),
            ltr("$ ", 10),
        ]
    );
    let last = Row::from_index(4);
    assert_eq!(o.orders().row_of(last), Some(last), "the prompt's row");
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, &bytes);
    let vp = t.view(Rows::ZERO);
    for (y, line) in got.iter().enumerate() {
        let (r, _) = o.orders().pointer(Row::from_index(y), Col::ZERO);
        let stored: String = vp.row(r)[..2]
            .iter()
            .map(|c| match c.content.as_codepoint() {
                Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                None => '.',
            })
            .collect();
        assert_eq!(stored, line[..2], "display row {y}");
    }
}

/// **A shrinking paragraph keeps its cut under a paragraph that runs on
/// from the history** too: "aaaa bbbb" sits one row above the window, so
/// counting every line of the window's first paragraph read the window as
/// full, and the gap "abcd efghi" and its wrapped spaces left was filled by
/// drawing that history row at the top, where a pointer named the window
/// row below it. As stored, every display row is its own window row.
#[test]
fn a_shrinking_paragraph_under_one_from_the_history_keeps_its_cut() {
    let ws = WinSize {
        ws_row: 5,
        ws_col: 10,
    };
    let bytes = format!(
        "{}aaaa bbbb cccc dddd eeee ffff\r\nabcd efghi   \r\n{PROMPT}",
        history(6)
    );
    let (got, o) = drawn_as(&ws, &bytes, Rows::ZERO, WordWrap::On);
    assert_ne!(got[0][..2], *"aa", "a history row drawn in the window");
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, &bytes);
    let vp = t.view(Rows::ZERO);
    for (y, line) in got.iter().enumerate() {
        let row = Row::from_index(y);
        assert_eq!(o.orders().row_of(row), Some(row), "row {y}");
        let (r, _) = o.orders().pointer(row, Col::ZERO);
        let stored: String = vp.row(r)[..2]
            .iter()
            .map(|c| match c.content.as_codepoint() {
                Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                None => '.',
            })
            .collect();
        assert_eq!(stored, line[..2], "display row {y}");
    }
}

/// **At the top of the history a paragraph that needs fewer lines keeps
/// the terminal's cut** too, rather than leave the prompt above the
/// cursor's row.
#[test]
fn at_the_top_of_the_history_a_paragraph_that_shrinks_keeps_its_cut() {
    let ws = WinSize {
        ws_row: 3,
        ws_col: 10,
    };
    let (got, o) = drawn_as(
        &ws,
        &format!("abcd efghi   \r\n{PROMPT}"),
        Rows::ZERO,
        WordWrap::On,
    );
    assert!(o.orders().display().is_none(), "{got:#?}");
    assert_eq!(got[2], ltr("$ ", 10));
}

/// The rows of the live screen as stored, read to the live width, `.`
/// for an erased cell.
fn stored_rows(t: &Term<'_>) -> Vec<String> {
    let vp = t.view(Rows::ZERO);
    (0..vp.rows().index())
        .map(|y| {
            vp.row(Row::from_index(y))
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|p| char::from_u32(p.value()))
                        .unwrap_or('.')
                })
                .collect()
        })
        .collect()
}

/// **A narrowing at a marked prompt keeps the cut of the rows it did not
/// re-cut.** At a marked prompt a change of width keeps the shell's block
/// whole (`Term::resize`, note M): each of its rows keeps its cells past
/// the new edge, to show again. A line typed there and run is then a
/// finished paragraph whose rows hold text past the edge. Read from the
/// cells the window shows, its text joined each row's first part to the
/// next row's and drew words never written -- `$ the qu` and `brown fo`
/// read as `qubrown` -- so the paragraph keeps the terminal's cut, every
/// visible cell where it is. The controls: not narrowed, the same
/// paragraph is drawn at its words, and narrowed its rows hold text past
/// the edge.
#[test]
fn a_narrowing_at_a_marked_prompt_keeps_the_cut_of_rows_it_did_not_re_cut() {
    let wide = WinSize {
        ws_row: 5,
        ws_col: 12,
    };
    let narrow = WinSize {
        ws_row: 5,
        ws_col: 8,
    };
    let run = "\r\n\x1b]133;C\x07\x1b]133;D;0\x07";
    for narrowed in [false, true] {
        let mut owned = Owned::new(&wide);
        let mut t = Term::new(owned.storage(), &wide);
        feed(
            &mut t,
            &format!("{}{PROMPT}the quick brown fox jumps", history(6)),
        );
        if narrowed {
            let _ = t.resize(&narrow);
            let vp = t.view(Rows::ZERO);
            let at = stored_rows(&t)
                .iter()
                .position(|l| l.starts_with("$ the"))
                .expect("the typed line");
            assert_eq!(
                (at..at + 3)
                    .map(|y| vp.text_past_edge(Row::from_index(y)))
                    .collect::<Vec<_>>(),
                [
                    baud_domain::PastEdge::Held,
                    baud_domain::PastEdge::Held,
                    baud_domain::PastEdge::Clear
                ],
                "the control: the block kept whole, text held past the edge: {:?}",
                stored_rows(&t)
            );
        }
        feed(&mut t, &format!("{run}{PROMPT}"));
        let mut o = Reorder::new();
        o.set_word_wrap(WordWrap::On);
        o.lay_out(&t, Rows::ZERO);
        match narrowed {
            false => assert!(
                o.orders().display().is_some(),
                "the control: not narrowed, drawn at its words"
            ),
            true => {
                let rows = stored_rows(&t);
                let at = rows
                    .iter()
                    .position(|l| l.starts_with("$ the"))
                    .expect("the typed line");
                assert_eq!(
                    rows[at..at + 3],
                    ["$ the qu", "brown fo", "mps....."],
                    "the rows as the terminal cut them at twelve, shown to eight"
                );
                assert!(
                    o.orders().display().is_none(),
                    "every row drawn as the terminal cut it"
                );
            }
        }
    }
}

/// **A narrowing at a marked prompt re-cuts the output above it, and the
/// display draws it at its words**: the paragraph a command printed is
/// finished output, which a change of width re-cuts as it always did, so
/// its rows hold nothing past the edge and the word wrap reads them whole
/// -- `the`, `quick`, `brown`, `fox`, `jumps`, each on a line, where the
/// terminal's re-cut at eight split `the quic|k brown`. Before, the
/// narrowing kept every row and the paragraph its cut at twelve, clipped.
#[test]
fn a_narrowing_at_a_marked_prompt_draws_the_output_above_it_at_its_words() {
    let wide = WinSize {
        ws_row: 6,
        ws_col: 12,
    };
    let mut owned = Owned::new(&wide);
    let mut t = Term::new(owned.storage(), &wide);
    feed(
        &mut t,
        &format!("{}the quick brown fox jumps\r\n{PROMPT}", history(8)),
    );
    let _ = t.resize(&WinSize {
        ws_row: 6,
        ws_col: 8,
    });
    let vp = t.view(Rows::ZERO);
    assert!(
        (0..6).all(|y| vp.text_past_edge(Row::from_index(y)) == baud_domain::PastEdge::Clear),
        "the control: nothing held past the edge"
    );
    assert_eq!(
        stored_rows(&t)[1..5],
        ["the quic", "k brown ", "fox jump", "s......."],
        "the control: the terminal's re-cut at eight splits words"
    );
    let mut o = Reorder::new();
    o.set_word_wrap(WordWrap::On);
    o.lay_out(&t, Rows::ZERO);
    let vp = o.view(&t, Rows::ZERO);
    let drawn: Vec<String> = (0..vp.rows().index())
        .map(|y| {
            let r = Row::from_index(y);
            let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
            o.orders().permute(r, vp.row(r), &mut out);
            out.iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|p| char::from_u32(p.value()))
                        .unwrap_or('.')
                })
                .collect()
        })
        .collect();
    assert_eq!(
        drawn,
        [
            ltr("the ", 8),
            ltr("quick ", 8),
            ltr("brown ", 8),
            ltr("fox ", 8),
            ltr("jumps", 8),
            ltr("$ ", 8),
        ]
    );
}

/// The display lines of `text` written above a prompt in a window `cols`
/// wide, the history's lines and the prompt's left out, each trimmed of
/// its erased cells; and the words the layout broke at a line's edge.
fn path_lines(text: &str, cols: u16) -> (Vec<String>, baud_render::Tally) {
    let ws = WinSize {
        ws_row: 8,
        ws_col: cols,
    };
    let (got, o) = drawn_as(
        &ws,
        &format!("{}{text}\r\n{PROMPT}", history(8)),
        Rows::ZERO,
        WordWrap::On,
    );
    let emergency = o
        .orders()
        .display()
        .map(|d| d.counts().2)
        .unwrap_or_default();
    let lines = got
        .iter()
        .map(|l| l.trim_end_matches(['.', ' ']).to_string())
        .filter(|l| {
            let history = l
                .strip_prefix('h')
                .is_some_and(|n| n.parse::<u16>().is_ok());
            !history && l != "$"
        })
        .collect();
    (lines, emergency)
}

/// **A path wraps before its separators, and moves whole after a space**
/// (`baud_domain::line_breaks_in_paths`): `/home` then `/jordan`; `cd`
/// never left with a lone `/`; a dated log path broken at its separators
/// with no word broken at the edge, where UAX #14's default glues
/// `2026/11/04.log` (LB25) and breaks it at the edge; a Windows path at
/// its backslashes; a URL's host whole before its path.
#[test]
fn a_path_wraps_before_its_separators() {
    let none = baud_render::Tally::default();
    let mut wrong = Vec::new();
    for (text, cols, want) in [
        ("ls /home/jordan", 10, vec!["ls /home", "/jordan"]),
        ("cd /home", 7, vec!["cd", "/home"]),
        (
            "/var/log/2026/11/04.log",
            12,
            vec!["/var/log", "/2026/11", "/04.log"],
        ),
        (
            r"C:\Users\jordan\Code",
            10,
            vec![r"C:\Users", r"\jordan", r"\Code"],
        ),
        (
            "https://github.com/jordan",
            20,
            vec!["https://github.com", "/jordan"],
        ),
    ] {
        let (got, emergency) = path_lines(text, cols);
        if got != want || emergency != none {
            wrong.push(format!(
                "{text} at {cols}: {got:?}, {emergency:?} broken at the edge"
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **A fraction, a date and the abbreviations UAX #14 names wrap as its
/// default wraps them**: no path run, so no break before their slash.
#[test]
fn a_fraction_a_date_and_w_o_wrap_as_the_default() {
    let (got, _) = path_lines("half 1/2 and 06/07/99 w/o and/or", 10);
    assert_eq!(got, ["half 1/2", "and", "06/07/99", "w/o and/or"]);
}

/// **Spaces a program wrote after its last word go on lines of their own,
/// as the terminal laid them**, where hanging them would leave the
/// paragraph on fewer lines than it is stored in: an underlined header
/// padded past the width. Written at twelve, `ab cdefgh` and eleven
/// underlined spaces; narrowed to six, the re-cut splits `cde|fgh` over
/// four rows. At its words it needs two lines and the spaces, which fill
/// `cdefgh`'s line no further and go on two lines of their own, six and
/// five, so the paragraph keeps its four rows and every word is whole.
/// Hung, they left two lines for four rows, and the paragraph kept the
/// terminal's cut (the control below, with the wrap off).
#[test]
fn spaces_after_the_last_word_are_laid_on_lines_of_their_own() {
    let ws = WinSize {
        ws_row: 8,
        ws_col: 12,
    };
    let bytes = format!(
        "{}ab cdefgh\x1b[4m{}\x1b[24m\r\n{PROMPT}",
        history(10),
        " ".repeat(11)
    );
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, &bytes);
    let _ = t.resize(&WinSize {
        ws_row: 8,
        ws_col: 6,
    });
    let rows = |wrap: WordWrap| {
        let mut o = Reorder::new();
        o.set_word_wrap(wrap);
        o.lay_out(&t, Rows::ZERO);
        let vp = o.view(&t, Rows::ZERO);
        (0..vp.rows().index())
            .map(|y| {
                let r = Row::from_index(y);
                let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
                o.orders().permute(r, vp.row(r), &mut out);
                out.iter()
                    .map(|c| {
                        c.content
                            .as_codepoint()
                            .and_then(|p| char::from_u32(p.value()))
                            .unwrap_or('.')
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(WordWrap::Off)[3..7],
        ["ab cde", "fgh   ", "      ", "  ...."],
        "the control: the terminal's re-cut splits a word over four rows"
    );
    assert_eq!(
        rows(WordWrap::On)[3..7],
        ["ab ...", "cdefgh", "      ", "     ."],
        "every word whole on the paragraph's four rows"
    );
}
