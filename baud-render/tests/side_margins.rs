//! **A paragraph written inside DECSLRM side margins is laid out inside
//! them**: DECSLRM "sets the left and right margins to define the
//! scrolling region" (`[[source: vt510-decslrm]]`), and text that reaches
//! the right margin wraps to the left margin of the next line. The core
//! keeps on each row that wrapped so the margins it wrapped at
//! (`baud_domain::WrapMargins`), and UAX #9 (`[[source: uax9-bidi]]`)
//! resolves the paragraph over its rows' cells inside them, one paragraph
//! across its rows, and each run of cells outside them as a paragraph of
//! its own, ordered in its own columns, as a table row's cells are. That
//! the cells outside the margins are no part of the paragraph is baud's
//! reading, which the display's word wrap keeps too.
//!
//! **Only margins that end at the screen's right edge link the rows**,
//! since 2026-11-03: the core marks a row as continuing only where it
//! wrapped at the screen's edge, as ghostty marks only a wrap at its last
//! column. So the scene sets margins 11 to 30 on 30
//! columns, the cells outside them left of the left margin, and a
//! paragraph a program wraps at a right margin inside the screen is drawn
//! row by row (`a_paragraph_cut_at_a_right_margin_inside_the_screen_is_drawn_row_by_row`).

use baud_core::{Owned, Term};
use baud_domain::{BytePos, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_render::Reorder;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 30,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Twenty Hebrew letters: the whole width of margins 11 to 30.
fn twenty() -> String {
    (0..20)
        .map(|i| char::from_u32(0x5D0 + i).unwrap())
        .collect()
}

/// **The scene**: margins 11 to 30, twenty Hebrew letters and "12" from
/// column 11, the digits wrapped to the next line's left margin; and left
/// of the margins, columns 1 and 2, "xy" and "zw" on the two rows.
fn scene() -> String {
    format!(
        "\x1b[?25l\x1b[?69h\x1b[11;30s\x1b[1;11H{}12\x1b[1;1Hxy\x1b[2;1Hzw",
        twenty()
    )
}

/// Each row of the window `back` lines up as drawn, `.` for an erased
/// cell.
fn drawn(t: &Term<'_>, back: Rows) -> Vec<String> {
    let mut o = Reorder::new();
    o.lay_out(t, back);
    let vp = o.view(t, back);
    (0..vp.rows().index())
        .map(|y| {
            let r = Row::from_index(y);
            let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
            o.orders().permute(r, vp.row(r), &mut out);
            out.iter()
                .map(|c| match c.content.as_codepoint() {
                    Some(cp) if !c.content.is_empty() => char::from_u32(cp.value()).unwrap_or('?'),
                    _ => '.',
                })
                .collect()
        })
        .collect()
}

/// The two rows as the margins lay them out: the Hebrew right to left
/// inside columns 11 to 30, the digits against the margins' right edge on
/// the next line, since the paragraph its Hebrew began runs right to left,
/// and the letters outside the margins where they were written.
fn want() -> [String; 2] {
    let he: String = twenty().chars().rev().collect();
    [
        format!("xy{}{he}", ".".repeat(8)),
        format!("zw{}12", ".".repeat(26)),
    ]
}

/// **Inside its margins, one paragraph across its rows**: the digits on
/// the second row hold no strong letter and run right to left as the
/// paragraph the Hebrew on the first row began; "xy" and "zw" outside the
/// margins stay where they were written. Read as whole rows, the right
/// pane's letters were drawn across the screen at the Hebrew's left; read
/// row by row, the digits run left to right at the left margin.
#[test]
fn a_paragraph_inside_side_margins_is_laid_out_inside_them() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &scene());
    let got = drawn(&t, Rows::ZERO);
    assert_eq!(got[..2], want(), "{got:#?}");
}

/// **On the alternate screen too**, where programs that set side margins
/// draw.
#[test]
fn on_the_alternate_screen_too() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[?1049h");
    feed(&mut t, &scene());
    let got = drawn(&t, Rows::ZERO);
    assert_eq!(got[..2], want(), "{got:#?}");
}

/// **A paragraph that began in the history reads its first row there
/// inside the margins it wrapped at**: the scene scrolled up a line, its
/// Hebrew row in the history, the digits on the window's first row still
/// run right to left against the margins' right edge.
#[test]
fn a_row_in_the_history_keeps_its_margins_as_context() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &format!("{}\x1b[?69l\x1b[4;1H\n", scene()));
    assert!(
        t.view(Rows::of_count(1)).margins(Row::ZERO).is_some(),
        "the premise: the Hebrew row is in the history, its margins kept"
    );
    let got = drawn(&t, Rows::ZERO);
    assert_eq!(got[0], want()[1], "{got:#?}");
}

/// **Beside a paragraph the display draws at its words**, the margins are
/// read from the display's own lines: below the scene, a finished English
/// paragraph the display draws on one line more pushes the scene's first
/// row out of the window, and the digits, now on the window's first line,
/// still run right to left inside the margins.
#[test]
fn the_display_keeps_the_margins_of_the_rows_it_draws_as_stored() {
    let ws = WinSize {
        ws_row: 8,
        ws_col: 30,
    };
    let history: String = (0..12).map(|i| format!("h{i}\r\n")).collect();
    let seven = "aaaaaaa bbbbbbb ccccccc ddddddd eeeeeee fffffff ggggggg";
    let bytes = format!(
        "{history}{}\x1b[?69l\x1b[4;1H{seven}\x1b[7;1H\x1b]133;A\x07$ ",
        scene()
    );
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, &bytes);
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    assert!(o.orders().display().is_some(), "the premise: re-broken");
    let got = drawn(&t, Rows::ZERO);
    assert_eq!(got[0], want()[1], "{got:#?}");
}

/// **A change of width that re-cuts the rows leaves no margins to read**:
/// the scene's paragraph alone widened to 40 columns and narrowed back is
/// re-cut as one line, its first row keeping the mark that it wrapped at
/// margins but not the margins, a pair the renderer does not take for
/// margins, so the row is read whole -- one right-to-left paragraph, its
/// Hebrew drawn from the left edge and the blanks left of it at the right
/// -- where read inside the margins the blanks would stay at the left, and
/// read as margins at column 1 alone every other cell a paragraph of its
/// own, the Hebrew in written order.
#[test]
fn a_change_of_width_that_re_cuts_the_rows_reads_them_whole() {
    let mut owned = Owned::new(&WinSize {
        ws_row: 4,
        ws_col: 40,
    });
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        &format!("\x1b[?25l\x1b[?69h\x1b[11;30s\x1b[1;11H{}12", twenty()),
    );
    let _ = t.resize(&WinSize {
        ws_row: 4,
        ws_col: 40,
    });
    let _ = t.resize(&WS);
    let m = t.view(Rows::ZERO).margins(Row::ZERO);
    assert_eq!(
        m.map(|m| (m.left().index(), m.right().index())),
        Some((0, 0)),
        "the premise: the mark kept, the margins not"
    );
    let got = drawn(&t, Rows::ZERO);
    let he: String = twenty().chars().rev().collect();
    assert_eq!(got[0], format!("{he}{}", ".".repeat(10)), "{got:#?}");
}

/// The first `n` Hebrew letters, `n` at most 27.
fn he(n: u32) -> String {
    (0..n).map(|i| char::from_u32(0x5D0 + i).unwrap()).collect()
}

/// A fresh terminal fed `bytes`, drawn.
fn drawn_after(bytes: &str) -> Vec<String> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    drawn(&t, Rows::ZERO)
}

/// **Margins a row wrapped at stop applying when it wraps again at the
/// page's edge**: a row that wrapped at margins 11 to 30 and is written
/// over, after `?69l` or a DECSLRM over the whole page, by a paragraph
/// that wraps at column 30, is drawn as the same cells written on a fresh
/// screen are, one right-to-left paragraph across the page.
#[test]
fn a_row_wrapped_again_at_the_page_edge_is_read_whole() {
    let margined = format!("\x1b[?25l\x1b[?69h\x1b[11;30s\x1b[1;11H{}", "a".repeat(25));
    let rest = format!("\x1b[1;1H{}", he(25) + &he(10));
    let control = drawn_after(&format!("\x1b[?25l\x1b[2;11Haaaaa{rest}"));
    for leave in ["\x1b[?69l", "\x1b[s"] {
        let got = drawn_after(&format!("{margined}{leave}{rest}"));
        assert_eq!(got, control, "{leave:?}");
    }
}

/// **A row that no longer wraps is read whole**: a newline that ends the
/// row, or an erase to its end that leaves it wrapping nowhere, makes the
/// margins it once wrapped at no paragraph's, and it is drawn as the same
/// cells written on a fresh screen.
#[test]
fn a_row_that_no_longer_wraps_is_read_whole() {
    let margined = format!(
        "\x1b[?25l\x1b[?69h\x1b[11;30s\x1b[1;11H{}\x1b[?69l",
        "a".repeat(25)
    );
    let ended = format!("\x1b[1;1H{}\r\n", he(25));
    let control = drawn_after(&format!("\x1b[?25l\x1b[1;26Haaaaa\x1b[2;11Haaaaa{ended}"));
    assert_eq!(
        drawn_after(&format!("{margined}{ended}")),
        control,
        "a newline"
    );
    let erased = format!("\x1b[1;1H{}\x1b[1;2H\x1b[K", he(1));
    let control = drawn_after(&format!("\x1b[?25l\x1b[2;11Haaaaa{erased}"));
    assert_eq!(
        drawn_after(&format!("{margined}{erased}")),
        control,
        "an erase"
    );
}

/// **A paragraph the rectangle scrolls keeps its margins**: the scene
/// written from the bottom margin, so the wrap to the digits scrolls the
/// rectangle a row up, is drawn on rows 3 and 4 as it is on rows 1 and 2
/// written from the top.
#[test]
fn a_paragraph_the_rectangle_scrolls_keeps_its_margins() {
    let got = drawn_after(&format!(
        "\x1b[?25l\x1b[?69h\x1b[11;30s\x1b[4;11H{}12\x1b[3;1Hxy\x1b[4;1Hzw",
        twenty()
    ));
    assert_eq!(got[2..], want(), "{got:#?}");
}

/// **An `IL` inside the rectangle carries the margins down with its
/// cells**: the scene's paragraph moves to rows 2 and 3, and "xy" and
/// "zw", outside the margins, stay on rows 1 and 2.
#[test]
fn an_il_inside_the_rectangle_carries_the_margins() {
    let got = drawn_after(&format!("{}\x1b[1;11H\x1b[L", scene()));
    let he: String = twenty().chars().rev().collect();
    let want = [
        format!("xy{}", ".".repeat(28)),
        format!("zw{}{he}", ".".repeat(8)),
        format!("{}12", ".".repeat(28)),
    ];
    assert_eq!(got[..3], want, "{got:#?}");
}

/// **A paragraph a program cuts at a right margin inside the screen is
/// drawn row by row**: margins 1 to 20 of 30 columns, the Hebrew filling
/// them and "12" wrapped to the next line, which the core leaves unmarked
/// as ghostty does, so the digits are a
/// paragraph of their own and run left to right from the left margin,
/// where the margins ending at the screen's edge draw them against the
/// right one (`want`).
#[test]
fn a_paragraph_cut_at_a_right_margin_inside_the_screen_is_drawn_row_by_row() {
    let got = drawn_after(&format!(
        "\x1b[?25l\x1b[?69h\x1b[1;20s\x1b[1;1H{}12",
        twenty()
    ));
    assert_eq!(got[1], format!("12{}", ".".repeat(28)), "{got:#?}");
}
