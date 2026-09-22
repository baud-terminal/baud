//! `?2048` — in-band resize notifications.
//!
//! `SIGWINCH` arrives from the kernel on its own schedule, on another
//! thread, with no defined position in the application's input: an
//! application cannot tell which of its own reads the new size belongs
//! to, and intermediate sizes coalesce. A report written into the stream
//! has a position, and the race is gone for anything that reads it.
//!
//! This is **not** the answer to a shell's prompt duplicating on a
//! resize -- no shell consumes it, and that is `OSC 133` and `Zone`.
//! `docs/research/notes/m-resize-and-reflow.md` keeps the two apart.
mod support;
use baud_domain::WinSize;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

#[test]
fn enabling_the_mode_reports_the_current_size_at_once() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2048h");
        // `CSI 48 ; rows ; cols ; height_px ; width_px t`, characters
        // before pixels, and the pixels zero because this harness never
        // tells the core its cell size -- which `[[source: inband-resize]]`
        // defines as the answer of a terminal that cannot report them. With
        // a cell size they are the text area:
        // `the_inband_report_carries_pixels_once_the_cell_is_known`.
        assert_eq!(t.reply(), b"\x1b[48;24;80;0;0t");
    });
}

#[test]
fn a_resize_reports_what_the_terminal_has() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2048h");
        t.clear_reply();
        let _ = t.resize(&WinSize {
            ws_col: 60,
            ws_row: 18,
        });
        assert_eq!(t.reply(), b"\x1b[48;18;60;0;0t");
    });
}

#[test]
fn a_clamped_resize_reports_the_clamp_and_not_the_request() {
    // **The property this mode is for.** `Term::resize` clamps to what
    // was provisioned, and an application that is told the number it
    // asked for rather than the number it got draws for a screen that
    // does not exist -- which is the same defect an embedder produces by
    // sending the child the requested `WinSize` instead of the settled
    // one. The harness provisions exactly `WS`, so asking for more is
    // clamped by construction.
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2048h");
        t.clear_reply();
        let _ = t.resize(&WinSize {
            ws_col: 400,
            ws_row: 200,
        });
        let (cols, rows) = t.geometry();
        assert!(
            rows.index() < 200 && cols.index() < 400,
            "the control: the request was clamped, so there is something to report"
        );
        let want = format!("\x1b[48;{};{};0;0t", rows.index(), cols.index());
        assert_eq!(t.reply(), want.as_bytes());
    });
}

#[test]
fn nothing_is_reported_until_the_mode_is_asked_for_and_nothing_after_it_is_dropped() {
    support::with_term(&WS, |t| {
        // The control: a resize with the mode off says nothing at all, so
        // the test above is about the mode rather than about resizing.
        let _ = t.resize(&WinSize {
            ws_col: 90,
            ws_row: 26,
        });
        assert!(t.reply().is_empty(), "a report arrived unasked for");

        support::feed(t, b"\x1b[?2048h");
        t.clear_reply();
        support::feed(t, b"\x1b[?2048l");
        let _ = t.resize(&WinSize {
            ws_col: 70,
            ws_row: 20,
        });
        assert!(
            t.reply().is_empty(),
            "a report arrived after the mode was dropped"
        );
    });
}

#[test]
fn the_report_follows_the_resize_rather_than_preceding_it() {
    // The specification requires that the terminal not report until its
    // own resize is complete. The observable form of that here is the
    // *content*: a report carrying the new size can only have been
    // written after the geometry settled.
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2048h");
        t.clear_reply();
        let _ = t.resize(&WinSize {
            ws_col: 40,
            ws_row: 10,
        });
        let (cols, rows) = t.geometry();
        let want = format!("\x1b[48;{};{};0;0t", rows.index(), cols.index());
        assert_eq!(
            t.reply(),
            want.as_bytes(),
            "the report does not match the geometry the resize settled on"
        );
    });
}

#[test]
fn a_re_enable_reports_again() {
    // "If mode already enabled and re-enabled, report immediately" --
    // an application that lost track must be able to ask.
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2048h");
        t.clear_reply();
        support::feed(t, b"\x1b[?2048h");
        assert_eq!(t.reply(), b"\x1b[48;24;80;0;0t");
    });
}

#[test]
fn decrqm_answers_truthfully() {
    support::with_term(&WS, |t| {
        // Not recognised as set before it is set: `1` is set, `2` is reset.
        support::feed(t, b"\x1b[?2048$p");
        assert_eq!(t.reply(), b"\x1b[?2048;2$y");
        t.clear_reply();
        support::feed(t, b"\x1b[?2048h");
        t.clear_reply();
        support::feed(t, b"\x1b[?2048$p");
        assert_eq!(t.reply(), b"\x1b[?2048;1$y");
    });
}
