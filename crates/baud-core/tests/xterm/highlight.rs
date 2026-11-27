//! `?1001` highlight tracking, built to `[[source: ctlseqs]]`, "Highlight
//! tracking":
//!
//! > Mouse highlight tracking notifies a program of a button press, receives
//! > a range of lines from the program, highlights the region covered by the
//! > mouse within that range until button release, and then sends the program
//! > the release coordinates. It is enabled by specifying parameter 1001 to
//! > DECSET. Highlighting is performed only for button 1, though other button
//! > events can be received.
//!
//! > On button press, the same information as for normal tracking is
//! > generated; xterm then waits for the program to send mouse tracking
//! > information. [...] `CSI Ps ; Ps ; Ps ; Ps ; Ps T`
//!
//! > When the button is released, xterm reports the ending position one of
//! > two ways: if the start and end coordinates are the same locations:
//! > `CSI t CxCy`; otherwise: `CSI T CxCyCxCyCxCy`. The parameters are
//! > startx, starty, endx, endy, mousex, and mousey.
//!
//! And the mouse section's frame: "There are two sets of mutually exclusive
//! modes" -- the protocol and the encoding -- and "The remaining five modes of
//! the mouse protocols are each enabled (or disabled) by a different
//! parameter": 9, 1000, **1001**, 1002 and 1003. So `?1001` is a tracking
//! level, as `?1000` is, and not a flag beside one.
//!
//! What xterm does that the text does not say, and baud does not do,
//! leaves questions open.

use crate::support;
use baud_core::*;

fn feed(t: &mut Term, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
}

fn ev(button: MouseButton, kind: MouseKind, col: usize, row: usize) -> MouseEvent {
    MouseEvent {
        button,
        mods: MouseMods::NONE,
        kind,
        col: Col::at_index(col),
        row: Row::from_index(row),
        px: None,
    }
}

fn press(col: usize, row: usize) -> MouseEvent {
    ev(MouseButton::Left, MouseKind::Press, col, row)
}

fn drag(col: usize, row: usize) -> MouseEvent {
    ev(MouseButton::Left, MouseKind::Motion, col, row)
}

fn release(col: usize, row: usize) -> MouseEvent {
    ev(MouseButton::Left, MouseKind::Release, col, row)
}

/// Run `f` on a fresh corpus terminal after `setup`, with the reply cleared.
fn with<R>(setup: &[u8], f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    support::with_corpus(|t| {
        feed(t, setup);
        t.clear_reply();
        f(t)
    })
}

/// Send `e` and return what it put in the reply buffer, clearing it.
fn sent(t: &mut Term, e: MouseEvent) -> Vec<u8> {
    t.mouse_report(e);
    let out = t.reply().to_vec();
    t.clear_reply();
    out
}

/// The program's answer, as a pty read: XTHIMOUSE with its five parameters.
fn answer(t: &mut Term, func: u16, x: u16, y: u16, first: u16, last: u16) {
    let s = format!("\x1b[{func};{x};{y};{first};{last}T");
    feed(t, s.as_bytes());
}

const ON: &[u8] = b"\x1b[?1001h";

// ── the level ──────────────────────────────────────────────────────────────

/// **A press under `?1001` is normal tracking's press.** "On button press,
/// the same information as for normal tracking is generated." On the base
/// `?1001` was a flag beside the level, the level stayed off, and the press
/// went nowhere: the program waiting for it to answer with a region never
/// heard of it.
#[test]
fn a_press_is_reported_as_normal_tracking_reports_it() {
    with(ON, |t| {
        // `CSI M Cb Cx Cy`, each `+ 32`: button 1 is 0, the home cell 1;1.
        assert_eq!(sent(t, press(0, 0)), b"\x1b[M !!");
        // Button 3 too: "other button events can be received".
        assert_eq!(
            sent(t, ev(MouseButton::Right, MouseKind::Press, 1, 0)),
            b"\x1b[M\"\"!"
        );
        // And its release, as normal tracking writes one: code 3.
        assert_eq!(
            sent(t, ev(MouseButton::Right, MouseKind::Release, 1, 0)),
            b"\x1b[M#\"!"
        );
    });
}

/// No motion is reported: normal tracking reports none, and highlight
/// tracking's text adds none -- the program hears of the drag in the release
/// report.
#[test]
fn motion_is_not_reported() {
    with(ON, |t| {
        sent(t, press(0, 0));
        assert_eq!(sent(t, drag(5, 5)), b"");
        assert_eq!(sent(t, ev(MouseButton::None, MouseKind::Motion, 6, 6)), b"");
    });
}

/// **`?1001` is one of the five exclusive protocol modes.** Setting it
/// replaces `?1000`, setting `?1000` replaces it, and resetting either turns
/// tracking off -- the rule every other level already follows. DECRQM
/// answers set for the one in force and reset for the rest.
#[test]
fn it_is_a_level_exclusive_with_the_other_four() {
    let rqm = |setup: &[u8]| {
        with(setup, |t| {
            feed(t, b"\x1b[?1000$p\x1b[?1001$p\x1b[?1002$p");
            t.reply().to_vec()
        })
    };
    assert_eq!(
        rqm(b"\x1b[?1000h\x1b[?1001h"),
        b"\x1b[?1000;2$y\x1b[?1001;1$y\x1b[?1002;2$y",
        "the later DECSET selects the level"
    );
    assert_eq!(
        rqm(b"\x1b[?1001h\x1b[?1002h"),
        b"\x1b[?1000;2$y\x1b[?1001;2$y\x1b[?1002;1$y",
        "and replaces it"
    );
    assert_eq!(
        rqm(b"\x1b[?1001h\x1b[?1000l"),
        b"\x1b[?1000;2$y\x1b[?1001;2$y\x1b[?1002;2$y",
        "a reset of any level turns tracking off"
    );
    // And a reset of a level not in force, `?1002 l` under `?1001`, turns
    // tracking off: no press is reported.
    with(b"\x1b[?1000h\x1b[?1001h\x1b[?1002l", |t| {
        assert_eq!(sent(t, press(0, 0)), b"", "and off means off");
    });
}

/// **XTSAVE and XTRESTORE keep it as they keep the other levels**: saved
/// as DECRQM answers it, and restored through DECSET, so `?1000` set in
/// between is replaced by `?1001` again.
#[test]
fn xtsave_and_xtrestore_keep_it_as_a_level() {
    with(b"\x1b[?1001h\x1b[?1001s\x1b[?1000h", |t| {
        assert_eq!(t.mouse_tracking(), MouseTracking::Normal, "the control");
        feed(t, b"\x1b[?1001r");
        assert_eq!(t.mouse_tracking(), MouseTracking::Highlight);
        assert_eq!(sent(t, press(0, 0)), b"\x1b[M !!");
    });
}

// ── the handshake ──────────────────────────────────────────────────────────

fn at(col: usize, row: usize) -> CellAt {
    CellAt::new(Col::at_index(col), Row::from_index(row))
}

/// **What the embedder draws**: nothing until the program answers, then the
/// region from its start to the end that "tracks the mouse", held to its
/// rows, and nothing again after the release -- "highlights the region
/// covered by the mouse within that range until button release".
#[test]
fn the_region_is_there_to_draw_from_the_answer_to_the_release() {
    with(ON, |t| {
        assert_eq!(t.highlight(), None, "nothing before a press");
        sent(t, press(4, 2));
        assert_eq!(t.highlight(), None, "nor while the answer is owed");
        // The pointer moves before the answer arrives: the end starts there.
        sent(t, drag(6, 3));
        answer(t, 1, 5, 3, 2, 6);
        assert_eq!(
            t.highlight(),
            Some(Highlighted {
                start: at(4, 2),
                end: at(6, 3),
            }),
            "the answer starts it where the pointer is"
        );
        sent(t, drag(1, 1));
        assert_eq!(
            t.highlight(),
            Some(Highlighted {
                start: at(4, 2),
                end: at(1, 1),
            }),
            "it tracks the mouse, upward too, and is not sorted"
        );
        sent(t, drag(9, 20));
        assert_eq!(
            t.highlight().map(|h| h.end),
            Some(at(9, 4)),
            "held above row lastrow (6): one-based 5 is the last"
        );
        sent(t, drag(9, 0));
        assert_eq!(
            t.highlight().map(|h| h.end),
            Some(at(9, 1)),
            "and never above row firstrow (2)"
        );
        sent(t, release(9, 0));
        assert_eq!(t.highlight(), None, "and gone at the release");
    });
}

/// **A start off the screen is held to it**, so the drawn cell and the
/// reported one exist: column 900 on a 200-column screen is column 200.
#[test]
fn a_start_off_the_screen_is_held_to_it() {
    with(ON, |t| {
        sent(t, press(4, 2));
        answer(t, 1, 900, 3, 1, 50);
        assert_eq!(t.highlight().map(|h| h.start), Some(at(199, 2)));
    });
}

/// **A shrink under a drag holds every cell to the new screen.**
#[test]
fn a_resize_under_a_drag_holds_the_region_to_the_screen() {
    with(ON, |t| {
        sent(t, press(150, 40));
        answer(t, 1, 151, 41, 1, 51);
        sent(t, drag(180, 45));
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 20,
                ws_col: 80,
            }),
            Resized::Ok
        );
        assert_eq!(
            t.highlight(),
            Some(Highlighted {
                start: at(79, 19),
                end: at(79, 19),
            })
        );
    });
}

/// **The release after an answer is the long form**, `CSI T` and six
/// positions, when the region's start and end differ: startx, starty, endx,
/// endy, mousex, mousey -- here the press at 5;3, a drag, and the release at
/// 10;5, the end and the mouse in one cell.
#[test]
fn a_tracked_release_reports_the_region_in_the_long_form() {
    with(ON, |t| {
        assert_eq!(sent(t, press(4, 2)), b"\x1b[M %#");
        answer(t, 1, 5, 3, 1, 50);
        assert_eq!(t.reply(), b"", "the answer is not echoed");
        assert_eq!(sent(t, drag(9, 4)), b"");
        // 5 3 10 5 10 5, each `+ 32`.
        assert_eq!(sent(t, release(9, 4)), b"\x1b[T%#*%*%");
    });
}

/// **The short form when the start and end are the same location**:
/// `CSI t CxCy`, one position. The plain reading of "if the start and end
/// coordinates are the same locations"; xterm's own test differs, an open
/// question.
#[test]
fn a_release_where_it_started_reports_the_short_form() {
    with(ON, |t| {
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 1, 50);
        sent(t, drag(9, 4));
        assert_eq!(sent(t, release(4, 2)), b"\x1b[t%#");
    });
}

/// **The end never leaves the rows the program allowed**: "will never be
/// above row firstrow and will always be above row lastrow". Rows 2 to 4
/// here; a release on row 9 ends the region on row 4 and reports the mouse
/// where it was, "which may not be over a character" of the region.
#[test]
fn the_end_is_held_to_the_rows_the_program_gave() {
    with(ON, |t| {
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 2, 5);
        // Below lastrow - 1: held at row 4.
        assert_eq!(sent(t, release(7, 8)), b"\x1b[T%#($()");
    });
    with(ON, |t| {
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 2, 5);
        // Above firstrow: held at row 2.
        assert_eq!(sent(t, release(7, 0)), b"\x1b[T%#(\"(!");
    });
}

/// **`func` 0 aborts**: "func is non-zero to initiate highlight tracking and
/// zero to abort." Nothing is tracked, and the release is normal tracking's.
#[test]
fn func_zero_aborts_and_the_release_is_normal() {
    with(ON, |t| {
        sent(t, press(4, 2));
        answer(t, 0, 5, 3, 1, 50);
        assert_eq!(sent(t, release(9, 4)), b"\x1b[M#*%");
    });
}

/// **An answer nobody asked for starts nothing.** The text has the program
/// answer a press; one sent with no press waiting for it is not an answer,
/// and a later press and release are normal tracking's.
#[test]
fn an_unsolicited_answer_is_ignored() {
    with(ON, |t| {
        answer(t, 1, 5, 3, 1, 50);
        assert_eq!(t.highlight(), None, "nothing is tracked");
        // A release with no press before it -- the press landed elsewhere --
        // is normal tracking's, not a region's.
        assert_eq!(sent(t, release(9, 4)), b"\x1b[M#*%");
        sent(t, press(4, 2));
        assert_eq!(sent(t, release(9, 4)), b"\x1b[M#*%");
    });
}

/// **A release before the answer is normal tracking's**, and an answer that
/// arrives after it is unsolicited: the button is up, so there is no
/// "region covered by the mouse ... until button release" left to highlight.
#[test]
fn a_release_before_the_answer_is_normal_and_a_late_answer_is_ignored() {
    with(ON, |t| {
        sent(t, press(4, 2));
        assert_eq!(sent(t, release(9, 4)), b"\x1b[M#*%");
        answer(t, 1, 5, 3, 1, 50);
        sent(t, press(4, 2));
        assert_eq!(sent(t, release(9, 4)), b"\x1b[M#*%");
    });
}

/// **A mode change and both resets end the handshake.** A program that left
/// the mode, or a terminal reset under it, has not asked for this release's
/// region; the release is reported by whatever is in force after.
#[test]
fn a_mode_change_or_a_reset_ends_the_handshake() {
    for between in [&b"\x1b[?1001l\x1b[?1001h"[..], b"\x1b[!p"] {
        with(ON, |t| {
            sent(t, press(4, 2));
            answer(t, 1, 5, 3, 1, 50);
            feed(t, between);
            t.clear_reply();
            assert_eq!(sent(t, release(9, 4)), b"\x1b[M#*%", "{between:?}");
        });
    }
    with(ON, |t| {
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 1, 50);
        feed(t, b"\x1bc");
        assert_eq!(t.highlight(), None, "RIS clears what is drawn");
        assert_eq!(sent(t, release(9, 4)), b"", "RIS turns tracking off");
    });
    // `?1001 l` alone: the drawing goes, and with tracking off the release
    // is reported by nothing.
    with(ON, |t| {
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 1, 50);
        assert!(t.highlight().is_some(), "the control");
        feed(t, b"\x1b[?1001l");
        assert_eq!(t.highlight(), None, "?1001 l clears what is drawn");
        assert_eq!(sent(t, release(9, 4)), b"", "and tracking is off");
    });
}

/// **The answer lands the same at every read split**: a pty read can end
/// anywhere inside XTHIMOUSE.
#[test]
fn the_answer_lands_the_same_at_every_read_split() {
    let ans = b"\x1b[1;5;3;1;50T";
    for k in 1..ans.len() {
        with(ON, |t| {
            sent(t, press(4, 2));
            feed(t, &ans[..k]);
            feed(t, &ans[k..]);
            assert_eq!(
                t.highlight().map(|h| h.start),
                Some(at(4, 2)),
                "split at {k}"
            );
            assert_eq!(sent(t, release(9, 4)), b"\x1b[T%#*%*%", "split at {k}");
        });
    }
}

// ── the encodings ──────────────────────────────────────────────────────────

/// **SGR (1006)**: "The highlight tracking responses are also modified to an
/// SGR-like format, using the same SGR-style scheme" -- `CSI <`, decimal
/// positions separated by `;`, and the final byte the X10 form has.
#[test]
fn under_sgr_the_reports_are_decimal() {
    with(b"\x1b[?1001h\x1b[?1006h", |t| {
        assert_eq!(sent(t, press(4, 2)), b"\x1b[<0;5;3M");
        answer(t, 1, 5, 3, 1, 50);
        assert_eq!(sent(t, release(9, 4)), b"\x1b[<5;3;10;5;10;5T");
    });
    with(b"\x1b[?1001h\x1b[?1006h", |t| {
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 1, 50);
        assert_eq!(sent(t, release(4, 2)), b"\x1b[<5;3t");
    });
}

/// A pixel position, zero-based as the embedder hands it over.
fn px(x: u16, y: u16) -> Option<PointerPx> {
    Some(PointerPx {
        x: ImagePx::of(x),
        y: ImagePx::of(y),
    })
}

/// **SGR-Pixels (1016)**: "Use the same mouse response format as the 1006
/// control, but report position in pixels rather than character cells" --
/// and of the long form's six fields the highlight paragraph calls four
/// "character positions" of the region and two "the location of the mouse
/// at button up, which may not be over a character". So the region is in
/// cells and the mouse in pixels, one-based on the wire as a 1016 press
/// writes its pixel: here the press at pixel 40;30, the region 5;3 to
/// 10;5, and the mouse at pixel 95;50.
#[test]
fn under_sgr_pixels_the_region_is_in_cells_and_the_mouse_in_pixels() {
    with(b"\x1b[?1001h\x1b[?1016h", |t| {
        let mut p = press(4, 2);
        p.px = px(40, 30);
        assert_eq!(sent(t, p), b"\x1b[<0;41;31M");
        answer(t, 1, 5, 3, 1, 50);
        let mut r = release(9, 4);
        r.px = px(95, 50);
        assert_eq!(sent(t, r), b"\x1b[<5;3;10;5;96;51T");
        assert_eq!(t.mouse_unencodable, 0);
    });
    // The short form writes the region's one location and no mouse
    // position: cells, as under 1006.
    with(b"\x1b[?1001h\x1b[?1016h", |t| {
        let mut p = press(4, 2);
        p.px = px(40, 30);
        sent(t, p);
        answer(t, 1, 5, 3, 1, 50);
        let mut r = release(4, 2);
        r.px = px(41, 31);
        assert_eq!(sent(t, r), b"\x1b[<5;3t");
    });
}

/// **Under 1016 a long-form release with no pixel is refused and counted**,
/// as a press with none is: the mouse field is a pixel the embedder did not
/// give, and baud does not make one up. One at `u16::MAX`, where `ImagePx`
/// saturates and whose one-based form the field cannot carry, the same.
/// The short form carries no mouse position, so it needs no pixel and is
/// sent.
#[test]
fn under_sgr_pixels_a_release_without_a_pixel_is_refused_where_one_is_written() {
    for missing in [None, px(u16::MAX, 50), px(95, u16::MAX)] {
        with(b"\x1b[?1001h\x1b[?1016h", |t| {
            let mut p = press(4, 2);
            p.px = px(40, 30);
            sent(t, p);
            answer(t, 1, 5, 3, 1, 50);
            let mut r = release(9, 4);
            r.px = missing;
            assert_eq!(sent(t, r), b"", "{missing:?}");
            assert_eq!(t.mouse_unencodable, 1, "{missing:?}");
            assert_eq!(t.highlight(), None, "the release still ends it");
        });
    }
    with(b"\x1b[?1001h\x1b[?1016h", |t| {
        let mut p = press(4, 2);
        p.px = px(40, 30);
        sent(t, p);
        answer(t, 1, 5, 3, 1, 50);
        assert_eq!(sent(t, release(4, 2)), b"\x1b[<5;3t");
        assert_eq!(t.mouse_unencodable, 0);
    });
}

/// **urxvt (1015)**: decimal with no `<` -- the form ctlseqs warns "can be
/// mistaken for SD (scroll down)".
#[test]
fn under_urxvt_the_reports_are_decimal_without_the_marker() {
    with(b"\x1b[?1001h\x1b[?1015h", |t| {
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 1, 50);
        assert_eq!(sent(t, release(9, 4)), b"\x1b[5;3;10;5;10;5T");
    });
}

/// **UTF-8 (1005)** writes each position `+ 32` as UTF-8: "UTF-8 encoding for
/// Cx and Cy under all tracking modes". Column 150 is 182, two bytes.
#[test]
fn under_utf8_a_wide_position_is_two_bytes() {
    with(b"\x1b[?1001h\x1b[?1005h", |t| {
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 1, 50);
        assert_eq!(
            sent(t, release(149, 4)),
            "\x1b[T%#\u{b6}%\u{b6}%".as_bytes()
        );
    });
}

/// **A position X10 cannot name refuses the whole report**, counted in
/// `mouse_unencodable`, as every other X10 report past 223 is: the byte
/// would name a different cell.
#[test]
fn an_unencodable_position_refuses_the_report_and_counts_it() {
    let wide = WinSize {
        ws_row: 50,
        ws_col: 300,
    };
    support::with_term(&wide, |t| {
        feed(t, ON);
        t.clear_reply();
        sent(t, press(4, 2));
        answer(t, 1, 5, 3, 1, 50);
        assert_eq!(sent(t, release(250, 4)), b"");
        assert_eq!(t.mouse_unencodable, 1);
    });
}
