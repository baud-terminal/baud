//! Mouse reporting: `?1000/1002/1003` tracking and the `?1006` SGR encoding.
//!
//! `[[source: ctlseqs]]`:
//! * "The low two bits of Cb encode button information: 0=MB1 pressed,
//!   1=MB2 pressed, 2=MB3 pressed, and 3=release."
//! * "The next three bits encode the modifiers ... 4=Shift, 8=Meta, and
//!   16=Control."
//! * "On button-motion events, xterm adds 32 to the event code."
//! * Wheel buttons are "represented by the same event codes as buttons 1 and 2
//!   respectively, except that 64 is added to the event code."
//! * SGR (1006): "CSI < followed by semicolon-separated encoded button value,
//!   Px and Py ordinates and a final character which is M for button press and
//!   m for button release."
//!
//! **Only the SGR encoding is implemented, and that is a correctness decision.**
//! The X10 scheme encodes each ordinate as one byte at `value + 32`, capping it
//! at 223; baud's grid goes to 2048 columns. A report from column 400 in that
//! scheme is not truncated, it points at a different cell -- the wrong-shaped
//! answer `docs/vt-coverage.md` forbids. So without `?1006`, baud reports
//! nothing and counts the drop.

use crate::support;
use baud_core::*;

struct T {
    s: Owned,
}

fn feed(t: &mut Term, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
}

fn ev(button: MouseButton, kind: MouseKind, col: usize, row: usize) -> MouseEvent {
    MouseEvent {
        button,
        mods: MouseMods::NONE,
        kind,
        col: Col::at_index(col),
        row: Row::from_index(row),
    }
}

/// Enable `setup`, send `e`, return what went to the host.
fn report(setup: &[u8], e: MouseEvent) -> Vec<u8> {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, setup);
    term.clear_reply();
    term.mouse_report(e);
    term.reply().to_vec()
}

const SGR: &[u8] = b"\x1b[?1000h\x1b[?1006h";

// ── encoding ────────────────────────────────────────────────────────────────

/// A left-button press at the home cell.
///
/// Coordinates are 1-based like CUP, so the top-left cell is `1;1`.
#[test]
fn a_left_press_encodes_as_the_spec_says() {
    assert_eq!(
        report(SGR, ev(MouseButton::Left, MouseKind::Press, 0, 0)),
        b"\x1b[<0;1;1M"
    );
}

/// Release uses a lowercase `m`, which is the whole point of the SGR scheme:
/// "A different final character is used for button release to resolve the X10
/// ambiguity regarding which button was released."
#[test]
fn release_uses_a_lowercase_final_byte() {
    assert_eq!(
        report(SGR, ev(MouseButton::Left, MouseKind::Release, 0, 0)),
        b"\x1b[<0;1;1m"
    );
}

/// The button bits are 0, 1, 2 for left, middle, right.
#[test]
fn the_three_buttons_have_the_documented_codes() {
    for (b, code) in [
        (MouseButton::Left, "0"),
        (MouseButton::Middle, "1"),
        (MouseButton::Right, "2"),
    ] {
        let want = format!("\x1b[<{code};1;1M");
        assert_eq!(report(SGR, ev(b, MouseKind::Press, 0, 0)), want.as_bytes());
    }
}

/// Wheel buttons add 64.
#[test]
fn wheel_buttons_add_sixty_four() {
    assert_eq!(
        report(SGR, ev(MouseButton::WheelUp, MouseKind::Press, 0, 0)),
        b"\x1b[<64;1;1M"
    );
    assert_eq!(
        report(SGR, ev(MouseButton::WheelDown, MouseKind::Press, 0, 0)),
        b"\x1b[<65;1;1M"
    );
}

/// Motion adds 32, and it is a *different* 32 from the X10 printability offset
/// the SGR scheme drops.
#[test]
fn motion_adds_thirty_two() {
    let mut e = ev(MouseButton::Left, MouseKind::Motion, 0, 0);
    e.kind = MouseKind::Motion;
    assert_eq!(
        report(b"\x1b[?1002h\x1b[?1006h", e),
        b"\x1b[<32;1;1M",
        "0 (button 1) + 32 (motion)"
    );
}

/// Modifiers add 4, 8 and 16 and combine.
#[test]
fn modifiers_add_their_documented_values() {
    let mk = |m: MouseMods| MouseEvent {
        mods: m,
        ..ev(MouseButton::Left, MouseKind::Press, 0, 0)
    };
    assert_eq!(report(SGR, mk(MouseMods::SHIFT)), b"\x1b[<4;1;1M");
    assert_eq!(report(SGR, mk(MouseMods::META)), b"\x1b[<8;1;1M");
    assert_eq!(report(SGR, mk(MouseMods::CONTROL)), b"\x1b[<16;1;1M");
    assert_eq!(
        report(SGR, mk(MouseMods::SHIFT.with(MouseMods::CONTROL))),
        b"\x1b[<20;1;1M",
        "4 + 16"
    );
}

/// Coordinates past the X10 limit of 223 are reported exactly.
///
/// This is why the SGR encoding is the only one implemented: in X10 this cell
/// has no representation at all.
#[test]
fn coordinates_past_the_x10_limit_are_exact() {
    assert_eq!(
        report(SGR, ev(MouseButton::Left, MouseKind::Press, 399, 41)),
        b"\x1b[<0;400;42M"
    );
}

// ── gating ──────────────────────────────────────────────────────────────────

/// With no tracking enabled, nothing is reported.
#[test]
fn nothing_is_reported_without_tracking() {
    assert!(report(b"", ev(MouseButton::Left, MouseKind::Press, 0, 0)).is_empty());
    assert!(
        report(
            b"\x1b[?1006h",
            ev(MouseButton::Left, MouseKind::Press, 0, 0)
        )
        .is_empty()
    );
}

/// **Tracking without `?1006` reports X10, which is the original protocol.**
///
/// This asserted that nothing was reported at all, on the reasoning that X10
/// caps each ordinate at 223 against a 2048-column grid and a report from
/// column 400 would name a different cell. True of column 400, and not a
/// reason to refuse the encoding: an application speaking the original
/// protocol on an 80x24 screen was getting silence where every byte was
/// encodable.
///
/// `[[source: ctlseqs]]`: "The original X10 mouse protocol limits the Cx and Cy
/// ordinates to 223 (=255 - 32)." Positions within it are sent; the rest are
/// refused, below.
#[test]
fn tracking_without_sgr_reports_x10() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, b"\x1b[?1000h");
    assert!(term.mouse_report(ev(MouseButton::Left, MouseKind::Press, 0, 0)));
    // `ESC [ M` then Cb, Cx, Cy, each biased by 32. Row and column are
    // one-based on the wire, so (0,0) is (1,1) is byte 33.
    assert_eq!(term.reply(), b"\x1b[M\x20\x21\x21", "X10 press at 1,1");
    assert_eq!(term.mouse_unencodable, 0, "nothing was dropped");
}

/// **A position X10 cannot name is refused, not truncated.**
///
/// The byte for column 400 would wrap to a small value and point at a
/// different cell, which is worse than saying nothing: the application would
/// act on a confident wrong answer. Dropped and counted.
#[test]
fn x10_refuses_a_position_it_cannot_encode() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, b"\x1b[?1000h");
    assert!(!term.mouse_report(ev(MouseButton::Left, MouseKind::Press, 0, 400)));
    assert!(term.reply().is_empty(), "no wrong-shaped report");
    assert_eq!(term.mouse_unencodable, 1, "the drop is visible");

    // The same position under `?1006` is fine: SGR writes decimal.
    feed(&mut term, b"\x1b[?1006h");
    assert!(term.mouse_report(ev(MouseButton::Left, MouseKind::Press, 0, 400)));
    assert!(!term.reply().is_empty(), "SGR has no such limit");
}

/// The boundary is 223 inclusive, one-based, as the specification states.
#[test]
fn x10_encodes_up_to_223_inclusive() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, b"\x1b[?1000h");
    // Zero-based 222 is one-based 223: the last encodable column.
    assert!(term.mouse_report(ev(MouseButton::Left, MouseKind::Press, 0, 222)));
    term.clear_reply();
    assert!(
        !term.mouse_report(ev(MouseButton::Left, MouseKind::Press, 0, 223)),
        "one-based 224 is past the limit"
    );
}

/// `?1000` reports presses and releases but not motion.
#[test]
fn normal_tracking_excludes_motion() {
    let s = b"\x1b[?1000h\x1b[?1006h";
    assert!(
        report(s, ev(MouseButton::Left, MouseKind::Motion, 0, 0)).is_empty(),
        "motion must not be reported under ?1000"
    );
    assert!(
        !report(s, ev(MouseButton::Left, MouseKind::Press, 0, 0)).is_empty(),
        "a press must be"
    );
}

/// `?1002` adds motion while a button is held, but not bare motion.
#[test]
fn button_event_tracking_reports_drags_only() {
    let s = b"\x1b[?1002h\x1b[?1006h";
    assert!(
        !report(s, ev(MouseButton::Left, MouseKind::Motion, 0, 0)).is_empty(),
        "a drag is reported"
    );
    assert!(
        report(s, ev(MouseButton::None, MouseKind::Motion, 0, 0)).is_empty(),
        "bare motion is not"
    );
}

/// `?1003` reports motion with no button down.
///
/// `[[source: ctlseqs]]`: "all motion events are reported, even if no mouse
/// button is down."
#[test]
fn any_event_tracking_reports_bare_motion() {
    let s = b"\x1b[?1003h\x1b[?1006h";
    assert!(!report(s, ev(MouseButton::None, MouseKind::Motion, 0, 0)).is_empty());
}

/// Resetting the mode stops reporting.
#[test]
fn resetting_tracking_stops_reports() {
    let s = b"\x1b[?1000h\x1b[?1006h\x1b[?1000l";
    assert!(report(s, ev(MouseButton::Left, MouseKind::Press, 0, 0)).is_empty());
}

/// The tracking level is observable, and the modes are exclusive.
#[test]
fn tracking_level_is_observable_and_exclusive() {
    let level = |b: &[u8]| {
        let mut t = T {
            s: Owned::new(&support::CORPUS),
        };
        let mut term = Term::new(t.s.storage(), &support::CORPUS);
        feed(&mut term, b);
        term.mouse_tracking()
    };
    assert_eq!(level(b""), MouseTracking::Off);
    assert_eq!(level(b"\x1b[?1000h"), MouseTracking::Normal);
    assert_eq!(level(b"\x1b[?1003h"), MouseTracking::AnyEvent);
    assert_eq!(
        level(b"\x1b[?1000h\x1b[?1003h"),
        MouseTracking::AnyEvent,
        "the later DECSET selects the level"
    );
}

/// DECRQM reports the modes as implemented and tracks them.
///
/// It must say "set" only for the level actually in force -- claiming all three
/// would tell an application it is getting motion events it is not.
#[test]
fn decrqm_reports_the_mouse_modes() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, b"\x1b[?1002h");
    term.clear_reply();
    feed(&mut term, b"\x1b[?1000$p\x1b[?1002$p\x1b[?1006$p");
    assert_eq!(term.reply(), b"\x1b[?1000;2$y\x1b[?1002;1$y\x1b[?1006;2$y");
}

/// RIS turns tracking off, so a crashed program cannot leave the next one
/// receiving mouse reports it never asked for.
#[test]
fn ris_stops_tracking() {
    let s = b"\x1b[?1003h\x1b[?1006h\x1bc";
    assert!(report(s, ev(MouseButton::Left, MouseKind::Press, 0, 0)).is_empty());
}

/// **`CSI Ps ; Ps ; Ps ; Ps ; Ps T` is XTHIMOUSE, not SD.**
///
/// `[[source: ctlseqs]]` gives the final byte `T` to two operations and
/// separates them by *arity*: one parameter is "Scroll down Ps lines (SD)",
/// five are "Initiate highlight mouse tracking (XTHIMOUSE)".
///
/// baud read the five-parameter form as SD, so an application asking to
/// highlight a region **scrolled the screen instead** — a visible corruption,
/// and a silent one: nothing was counted, because SD is implemented and the
/// sequence dispatched to a real handler.
///
/// Found by re-reading the `?1001` refusal against its scope. The stated
/// hazard is xterm's event loop — "all X events are ignored until the proper
/// escape sequence is received" — and baud has no event loop to block. What
/// baud must not do is act on the request as though it were a different
/// operation.
#[test]
fn five_parameter_t_is_not_scroll_down() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, b"one\r\ntwo\r\nthree\r\n");
    let rows = |term: &Term| -> Vec<u32> {
        (0..3)
            .map(|y| {
                term.row(Row::from_index(y))[0]
                    .content
                    .as_codepoint()
                    .map_or(0, |c| c.value())
            })
            .collect()
    };
    let before = rows(&term);
    feed(&mut term, b"\x1b[1;2;3;4;5T");
    assert_eq!(rows(&term), before, "XTHIMOUSE must not scroll");
    assert_eq!(
        term.highlight_region(),
        HighlightRegion {
            func: 1,
            start_x: 2,
            start_y: 3,
            first_row: 4,
            last_row: 5,
        },
        "the region is recorded for an embedder that can highlight"
    );
}

/// **`?1001` is recorded, with an accessor, like every input-side mode.**
///
/// It was refused outright, on the reading that highlight tracking "requires a
/// cooperating program, else xterm will hang". The hang is an event loop
/// ignoring events until a reply arrives, and baud has no event loop -- so
/// whether to wait is the embedder's decision, not baud's to pre-empt.
///
/// What baud lacks is a selection model to highlight *with*. That is a reason
/// not to draw the region, not a reason to discard the request: recording it is
/// the only channel a `no_std` core with no renderer has, and it is what every
/// other input-side mode gets.
#[test]
fn highlight_tracking_is_recorded_and_reported() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    assert_eq!(term.highlight_tracking(), HighlightTracking::Off, "default");

    feed(&mut term, b"\x1b[?1001h");
    assert_eq!(term.highlight_tracking(), HighlightTracking::On);
    // `baud-core` counts unsupported *finals*, behind `diag`, and `?1001` is
    // a private mode rather than a final -- so there is no counter to read.
    // What the assertion was for is the line above: the mode is recorded,
    // which is the difference between recognised and refused.

    // DECRQM tells the truth about it.
    feed(&mut term, b"\x1b[?1001$p");
    assert_eq!(term.reply(), b"\x1b[?1001;1$y", "DECRQM reports it set");
    term.clear_reply();

    feed(&mut term, b"\x1b[?1001l");
    assert_eq!(term.highlight_tracking(), HighlightTracking::Off);
    feed(&mut term, b"\x1b[?1001$p");
    assert_eq!(term.reply(), b"\x1b[?1001;2$y", "and reset");
}

/// One parameter is still SD.
#[test]
fn one_parameter_t_is_still_scroll_down() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, b"one\r\ntwo\r\n\x1b[1T");
    assert_eq!(
        term.row(Row::from_index(1))[0]
            .content
            .as_codepoint()
            .map(|c| c.value()),
        Some(u32::from(b'o')),
        "SD by one moves `one` down a row"
    );
    // `baud-core`'s unsupported counter is `diag`-gated and indexed by final
    // byte; the assertion that carries here is the one above -- `SD` moved the
    // row, which is what "implemented" means.
}

// ── a full reply buffer ─────────────────────────────────────────────────────

/// **A report the reply buffer cannot hold returns `false`, the way
/// [`Term::key`] already does.** `mouse_report` returned `true` for it: the
/// report was dropped whole and counted in `reply_overflows`, which is right,
/// and then the return value said it had been sent, which is not. An
/// embedder that asks `true` to mean "queued" -- the wheel loop draining the
/// buffer only when it fills -- could not tell the drop from the send.
///
/// Found closing the loop on herdr 0.9.3 (2026-10-09): a 40-line fling in
/// the state it leaves the terminal in -- the alternate screen, `?1003` any
/// event, `?1006` SGR -- queued 21 twelve-byte reports into the 256-byte
/// buffer and dropped 19, and every one of the 40 calls answered `true`.
#[test]
fn a_report_that_does_not_fit_says_so() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    // The mode bytes herdr sends, in its order: ?1015 arrives and ?1006 wins.
    feed(
        &mut term,
        b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h",
    );
    term.clear_reply();
    let wheel = ev(MouseButton::WheelUp, MouseKind::Press, 50, 15);
    let one = b"\x1b[<64;51;16M";
    let fit = REPLY / one.len();
    assert_eq!(fit, 21, "twelve-byte reports into REPLY bytes");
    for i in 0..fit {
        assert!(term.mouse_report(wheel), "report {i} fits and is queued");
    }
    assert_eq!(term.reply(), one.repeat(fit).as_slice());
    let before = term.reply_overflows;
    assert!(
        !term.mouse_report(wheel),
        "a report that does not fit is not queued, and must not answer true"
    );
    assert_eq!(term.reply().len(), fit * one.len(), "dropped whole");
    assert_eq!(term.reply_overflows, before + 1, "and counted");
    // Positive control: the same report into a drained buffer is queued, so
    // the `false` above meant "did not fit" and not "not wanted".
    term.clear_reply();
    assert!(term.mouse_report(wheel));
    assert_eq!(term.reply(), one);
}

/// The X10 arm queues through the same buffer and answers the same way.
#[test]
fn an_x10_report_that_does_not_fit_says_so() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, b"\x1b[?1000h");
    term.clear_reply();
    let press = ev(MouseButton::Left, MouseKind::Press, 0, 0);
    // `ESC [ M Cb Cx Cy`: six bytes, so 42 fit and four bytes are left over.
    let fit = REPLY / 6;
    for i in 0..fit {
        assert!(term.mouse_report(press), "report {i} fits");
    }
    let before = term.reply_overflows;
    assert!(!term.mouse_report(press), "the next does not fit");
    assert_eq!(term.reply_overflows, before + 1);
    assert_eq!(
        term.mouse_unencodable, 0,
        "a full buffer is not unencodable"
    );
}
