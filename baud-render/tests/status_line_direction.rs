//! **A status line in Hebrew reads right to left**: the host-writable
//! status line (DECSSDT 2, written after DECSASD 1) is laid out as a line
//! of its own, as every line of the main display is -- ECMA-48's IMPLICIT,
//! "All bi-directional aspects of data are handled by the device itself"
//! (`[[source: ecma48]]` 7.2.1) -- under the line properties SCP and
//! `?2501` gave it while it was selected, the line's own, as DEC STD 070
//! gives it "its own ANSI text state and settings" (`[[source:
//! dec-std-070]]` 070-14 § 14.2; Jev OWN_PARAGRAPH, 0.94/0.97).
//!
//! Every expectation is a visual order worked by hand from UAX #9's rules;
//! `.` is an erased cell.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Reorder, Stack, Theme};
use support::{Canvas, font};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 12,
};

const HEBREW: &str = "\u{5D0}\u{5D1}\u{5D2}!";

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn text(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content {
            x if x.is_empty() => '.',
            x => x
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('?'),
        })
        .collect()
}

/// The status line and the main display's first row as the layout draws
/// them, after `bytes` from power-on.
fn drawn(bytes: &str) -> (String, String) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let vp = t.view(Rows::ZERO);
    let line = t.status_line().expect("a status line shown");
    let mut out = vec![PackedCell::EMPTY; line.cells.len()];
    o.orders()
        .permute(Row::from_index(vp.rows().index()), line.cells, &mut out);
    let mut first = vec![PackedCell::EMPTY; vp.cols().index()];
    o.orders().permute(Row::ZERO, vp.row(Row::ZERO), &mut first);
    (text(&out), text(&first))
}

/// The host-writable line opened and selected, `body` written on it, and
/// the main display selected again.
fn on_status(body: &str) -> String {
    format!("\x1b[2$~\x1b[1$}}{body}\x1b[0$}}")
}

/// **A Hebrew status line runs from the right edge**, as the same text on
/// a line of the main display does: its first strong letter is R (UAX #9
/// P2-P3), so the Hebrew run is reversed (L2) and the `!` after it, a
/// neutral between an R and the paragraph's end, takes the paragraph's
/// direction and is drawn at its left.
#[test]
fn a_hebrew_status_line_runs_from_the_right_edge() {
    let (status, main) = drawn(&format!("{HEBREW}{}", on_status(HEBREW)));
    assert_eq!(
        main, "........!\u{5D2}\u{5D1}\u{5D0}",
        "the control: a main row"
    );
    assert_eq!(status, main, "the status line as the main row");
    // The second control: a Latin status line keeps its order.
    let (status, _) = drawn(&on_status("abc!"));
    assert_eq!(status, "abc!........");
}

/// **The line's own properties, not the main display's**: `?2501` reset
/// and SCP 1 sent while DECSASD selects the status line keep a Hebrew
/// status line left to right (UAX #9 HL1), while a Hebrew line of the
/// main display, whose properties nobody changed, runs from the right.
#[test]
fn the_status_line_takes_the_direction_set_on_it() {
    let (status, main) = drawn(&format!(
        "{HEBREW}{}",
        on_status(&format!("\x1b[?2501l\x1b[1 k{HEBREW}"))
    ));
    assert_eq!(status, "\u{5D2}\u{5D1}\u{5D0}!........");
    assert_eq!(main, "........!\u{5D2}\u{5D1}\u{5D0}");
}

/// **With BDSM reset a status line whose path is right to left is drawn
/// in reverse**, terminal-wg's explicit RTL, as a line of the main display
/// is (`[[source: terminal-wg-bidi-basic-modes]]`).
#[test]
fn with_bdsm_reset_a_right_to_left_status_line_is_drawn_reversed() {
    let (status, main) = drawn(&format!("\x1b[8labc{}", on_status("\x1b[2 kabc")));
    assert_eq!(status, ".........cba");
    assert_eq!(main, "abc.........", "the control: the main display's path");
}

/// **The CPU path paints the status line as it paints the same text on a
/// line of the main display**: the status line's band of pixels equals
/// the band of a main-display row holding the same Hebrew, in the same
/// rendition.
#[test]
fn the_cpu_path_paints_a_hebrew_status_line_as_a_main_row() {
    let data = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&mono).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let line = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} 42";
    feed(&mut t, &format!("\x1b[?25l{line}{}", on_status(line)));
    r.fit(&t);
    let mut c = Canvas::for_renderer(&r);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let band = |row: usize| c.pixels[row * ch * c.width..(row + 1) * ch * c.width].to_vec();
    let status_row = t.geometry().1.index();
    assert_eq!(
        c.height,
        (status_row + 1) * ch,
        "the frame holds the status line"
    );
    let bg = Theme::dark().bg.0;
    assert!(band(0).iter().any(|&p| p != bg), "the main row drew");
    assert!(
        band(status_row) == band(0),
        "the status line paints as the main row"
    );
}

/// **No line of the main display is the status line's context**: a
/// Hebrew paragraph wrapped from the history into the window's first row
/// does not reach it, so a status line with no strong letter keeps the
/// power-on direction, left to right (UAX #9 P3), and stays where written.
#[test]
fn the_status_line_reads_no_line_of_the_main_display_as_context() {
    let ws = WinSize {
        ws_row: 3,
        ws_col: 6,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    // The status line opened below the main display, then four rows of
    // one Hebrew paragraph on its three, the first scrolled into the
    // history, and digits on the status line.
    let hebrew = "\u{5D0}".repeat(20);
    feed(&mut t, &format!("\x1b[2$~{hebrew}{}", on_status("12 !")));
    let vp = t.view(Rows::ZERO);
    assert_eq!(vp.rows().index(), 3, "the premise: three rows");
    assert_eq!(
        t.view(Rows::of_count(1)).wrapped(Row::ZERO),
        baud_domain::Wrap::Continues,
        "the premise: the history's line continues into the window"
    );
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let line = t.status_line().expect("shown");
    let mut out = vec![PackedCell::EMPTY; line.cells.len()];
    o.orders()
        .permute(Row::from_index(vp.rows().index()), line.cells, &mut out);
    assert_eq!(text(&out), "12 !..");
}

/// **With BDSM reset, after the renderer has shaped a letter that takes a
/// joining form** -- the frames that keep the last frame's joins -- a
/// right-to-left status line is still drawn in reverse: its band of
/// pixels equals a main-display row's holding the same text on the same
/// path.
#[test]
fn a_reversed_status_line_paints_as_a_main_row_once_joins_are_kept() {
    let data = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&mono).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // An Arabic beh on the third row, which the first frame shapes.
    feed(
        &mut t,
        &format!(
            "\x1b[?25l\x1b[8l\x1b[2 kabc\x1b[3;1H\u{628}{}",
            on_status("\x1b[2 kabc")
        ),
    );
    r.fit(&t);
    let mut c = Canvas::for_renderer(&r);
    for _ in 0..2 {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let band = |row: usize| c.pixels[row * ch * c.width..(row + 1) * ch * c.width].to_vec();
    let status_row = t.geometry().1.index();
    let bg = Theme::dark().bg.0;
    assert!(band(0).iter().any(|&p| p != bg), "the main row drew");
    assert!(
        band(status_row) == band(0),
        "the status line paints as the main row"
    );
}
