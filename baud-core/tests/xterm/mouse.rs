//! Mouse reporting: the tracking levels `?9` and `?1000/1002/1003`, and the
//! encodings X10, `?1005` UTF-8, `?1006` SGR, `?1015` urxvt and `?1016`
//! SGR-Pixels; and `?1007` alternate scroll, which the embedder's wheel reads.
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
//! **A position an encoding cannot name is refused and counted, not
//! truncated.** X10 encodes each ordinate as one byte at `value + 32`, capping
//! it at 223, and `?1005` at 2015; baud's grid goes to 2048 columns. A report
//! from column 400 under X10 is not truncated, it points at a different cell --
//! the wrong-shaped answer `docs/vt-coverage.md` forbids -- so it is dropped
//! and counted in `mouse_unencodable`. (This header said only SGR was
//! implemented, long after X10 was.)

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
        px: None,
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

/// Coordinates past the X10 limit of 223 are reported exactly: in X10 this
/// cell has no representation at all, and under SGR it is decimal.
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

// ── the modes that were counted: `?9`, `?1005`, `?1015`, `?1016`, `?1007` ──
//
// `[[source: ctlseqs]]`, "Mouse Tracking": "There are two sets of mutually
// exclusive modes: mouse protocol [and] protocol encoding." `?9` is a
// protocol, beside `?1000`/`?1002`/`?1003`; `?1005`, `?1006`, `?1015` and
// `?1016` are encodings. Until 2026-10-09 the four that are not `?1006` or a
// tracking level were `PrivateMode::Other`: counted in `unsupported_param`,
// answered `0` by DECRQM, and otherwise dropped, while herdr sends `?1015`
// before `?1006` and resets all eight on every start.

/// A fresh terminal, `setup` fed and the reply drained, then `e`: what went
/// to the host, and how many reports were refused as unencodable.
fn sent(setup: &[u8], e: MouseEvent) -> (Vec<u8>, usize) {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, setup);
    term.clear_reply();
    term.mouse_report(e);
    (term.reply().to_vec(), term.mouse_unencodable)
}

/// `e`, with the pointer at pixel `(x, y)` of the text area.
fn at_px(e: MouseEvent, x: ImagePx, y: ImagePx) -> MouseEvent {
    MouseEvent {
        px: Some(PointerPx { x, y }),
        ..e
    }
}

/// DECRQM's answers to `query` after `setup`, on a fresh terminal.
fn rqm(setup: &[u8], query: &[u8]) -> Vec<u8> {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, setup);
    term.clear_reply();
    feed(&mut term, query);
    term.reply().to_vec()
}

const ALL_MODES: &[u8] =
    b"\x1b[?9$p\x1b[?1000$p\x1b[?1005$p\x1b[?1006$p\x1b[?1007$p\x1b[?1015$p\x1b[?1016$p";

/// **`?9` is X10 compatibility: a press, the button, and nothing else.**
///
/// `[[source: ctlseqs]]`: "X10 compatibility mode sends an escape sequence
/// only on button press, encoding the location and the mouse button pressed
/// ... Cb is button-1, where button is 1, 2 or 3." No modifiers -- those
/// arrived with "Normal tracking mode", whose text is the first to say
/// "Modifier key (shift, ctrl, meta) information is also sent".
#[test]
fn x10_compatibility_reports_a_press_and_the_button_alone() {
    let press = |b| ev(b, MouseKind::Press, 0, 0);
    assert_eq!(
        sent(b"\x1b[?9h", press(MouseButton::Left)).0,
        b"\x1b[M\x20\x21\x21",
        "button 1 is Cb 0, plus 32"
    );
    assert_eq!(
        sent(b"\x1b[?9h", press(MouseButton::Middle)).0,
        b"\x1b[M\x21\x21\x21"
    );
    // Shift and Control held: normal tracking adds 4 and 16 (`CSI M 6 ! !`),
    // X10 compatibility adds nothing.
    let held = MouseEvent {
        mods: MouseMods::SHIFT.with(MouseMods::CONTROL),
        ..press(MouseButton::Right)
    };
    assert_eq!(sent(b"\x1b[?1000h", held).0, b"\x1b[M\x36\x21\x21");
    assert_eq!(sent(b"\x1b[?9h", held).0, b"\x1b[M\x22\x21\x21");
}

/// No release, no motion, and no wheel: "only on button press", and of
/// "button 1, 2 or 3". xterm drops a button past 3 under `X10_MOUSE`
/// (`EditorButton`, `[[source: xterm-button]]`), so the wheel is not a
/// position that failed to encode and is not counted as one.
#[test]
fn x10_compatibility_sends_no_release_no_motion_and_no_wheel() {
    let none = |e| {
        let (bytes, refused) = sent(b"\x1b[?9h", e);
        assert!(bytes.is_empty(), "{e:?} sent {bytes:02x?}");
        assert_eq!(refused, 0, "{e:?} is not an unencodable position");
    };
    none(ev(MouseButton::Left, MouseKind::Release, 0, 0));
    none(ev(MouseButton::Left, MouseKind::Motion, 3, 3));
    none(ev(MouseButton::None, MouseKind::Motion, 3, 3));
    none(ev(MouseButton::WheelUp, MouseKind::Press, 0, 0));
    none(ev(MouseButton::None, MouseKind::Press, 0, 0));
    // Positive control: the same terminal reports a press.
    assert!(
        !sent(b"\x1b[?9h", ev(MouseButton::Left, MouseKind::Press, 0, 0))
            .0
            .is_empty()
    );
}

/// **`?9` is a protocol and the encodings still apply to it.** xterm builds
/// X10 compatibility's `Cb` as `' ' + button` and then writes it through the
/// encoding in force (`EmitButtonCode`, `[[source: xterm-button]]`), so under
/// `?1006` it is `CSI < 0 ; x ; y M`, with the modifiers still dropped.
#[test]
fn x10_compatibility_takes_the_encoding_in_force() {
    let held = MouseEvent {
        mods: MouseMods::SHIFT,
        ..ev(MouseButton::Left, MouseKind::Press, 9, 4)
    };
    assert_eq!(sent(b"\x1b[?9h\x1b[?1006h", held).0, b"\x1b[<0;10;5M");
    assert_eq!(sent(b"\x1b[?9h\x1b[?1015h", held).0, b"\x1b[32;10;5M");
}

/// **Any tracking reset turns tracking off, `?9` included.** xterm's
/// `really_set_mousemode` stores `enabled ? mode : MOUSE_OFF` for all five
/// protocols (`[[source: xterm-reset]]`'s file), so `?9 l` stops `?1000`
/// and `?1000 l` stops `?9` -- the rule `?1000/1002/1003` already follow.
#[test]
fn any_tracking_reset_turns_x10_compatibility_off_and_on_is_a_level() {
    let press = ev(MouseButton::Left, MouseKind::Press, 0, 0);
    assert!(sent(b"\x1b[?9h\x1b[?1000l", press).0.is_empty());
    assert!(sent(b"\x1b[?1000h\x1b[?9l", press).0.is_empty());
    // A later level replaces it: `?9` then `?1000` reports the release.
    let release = ev(MouseButton::Left, MouseKind::Release, 0, 0);
    assert_eq!(
        sent(b"\x1b[?9h\x1b[?1000h", release).0,
        b"\x1b[M\x23\x21\x21"
    );
}

/// **An X10 release says "release", not which button.** `[[source:
/// ctlseqs]]`: "The low two bits of Cb encode button information: 0=MB1
/// pressed, 1=MB2 pressed, 2=MB3 pressed, and 3=release." `MouseEvent::cb`
/// kept the released button, which is SGR's rule ("A different final
/// character is used for button release to resolve the X10 ambiguity"), so
/// under plain `?1000` a left release went out byte for byte as a left
/// press -- an application read every click as two presses.
#[test]
fn an_x10_release_says_release_not_which_button() {
    let release = ev(MouseButton::Left, MouseKind::Release, 0, 0);
    assert_eq!(
        sent(b"\x1b[?1000h", release).0,
        b"\x1b[M\x23\x21\x21",
        "3 (release) + 32"
    );
    // The modifiers stay: 3 + 4 (Shift) + 32.
    let shifted = MouseEvent {
        mods: MouseMods::SHIFT,
        ..release
    };
    assert_eq!(sent(b"\x1b[?1000h", shifted).0, b"\x1b[M\x27\x21\x21");
    // SGR keeps the button and says release with its final byte.
    assert_eq!(sent(SGR, release).0, b"\x1b[<0;1;1m");
}

/// `[[source: ctlseqs]]`, "Wheel mice": "Release events for the wheel
/// buttons are not reported." The wheel is a press a line; a release of
/// button 4 or 5 has nothing to say in any encoding.
#[test]
fn wheel_releases_are_not_reported() {
    for setup in [SGR, b"\x1b[?1000h".as_slice(), b"\x1b[?1000h\x1b[?1015h"] {
        let (bytes, refused) = sent(setup, ev(MouseButton::WheelUp, MouseKind::Release, 0, 0));
        assert!(bytes.is_empty(), "{setup:02x?} sent {bytes:02x?}");
        assert_eq!(refused, 0);
    }
    // Positive control: the press is.
    assert_eq!(
        sent(SGR, ev(MouseButton::WheelUp, MouseKind::Press, 0, 0)).0,
        b"\x1b[<64;1;1M"
    );
}

/// **`?1005` is X10 with its bytes UTF-8 encoded.** `[[source: ctlseqs]]`,
/// "UTF-8 (1005)": "This enables UTF-8 encoding for Cx and Cy under all
/// tracking modes, expanding the maximum encodable position from 223 to
/// 2015. For positions less than 95, the resulting output is identical under
/// both modes." xterm writes a zero-based position under 95 as one byte and
/// anything else as the two-byte UTF-8 form of `value + 33`
/// (`EmitMousePosition`, `[[source: xterm-button]]`): one-based 95 is `0x7F`
/// and 96 is the first two-byte position.
#[test]
fn utf8_encoding_is_x10_below_ninety_six_and_two_bytes_from_it() {
    let at = |col| ev(MouseButton::Left, MouseKind::Press, col, 0);
    let s = b"\x1b[?1000h\x1b[?1005h";
    assert_eq!(sent(s, at(94)).0, b"\x1b[M\x20\x7f\x21", "one-based 95");
    assert_eq!(sent(s, at(95)).0, b"\x1b[M\x20\xc2\x80\x21", "96 is U+0080");
    // The row too: 300 is 332, U+014C.
    assert_eq!(
        sent(s, ev(MouseButton::Left, MouseKind::Press, 0, 299)).0,
        b"\x1b[M\x20\x21\xc5\x8c"
    );
    // A release is X10's: 3 + 32, one byte.
    assert_eq!(
        sent(s, ev(MouseButton::Left, MouseKind::Release, 95, 0)).0,
        b"\x1b[M\x23\xc2\x80\x21"
    );
}

/// **2015 is the last position, and 2016 is refused and counted** -- the
/// rule X10 keeps at 223, and the same departure from xterm, which clamps a
/// position past its limit and sends a `0` byte as a "past-end marker"
/// (`EmitMousePosition`, `[[source: xterm-button]]`). A `0` names no cell,
/// and a clamp names a cell the pointer is not over.
#[test]
fn utf8_encoding_reaches_2015_and_refuses_past_it() {
    let s = b"\x1b[?1000h\x1b[?1005h";
    let at = |col| ev(MouseButton::Left, MouseKind::Press, col, 0);
    assert_eq!(
        sent(s, at(2014)).0,
        b"\x1b[M\x20\xdf\xbf\x21",
        "2015 + 32 is 2047, U+07FF"
    );
    let (bytes, refused) = sent(s, at(2015));
    assert!(bytes.is_empty(), "2016 has no two-byte form: {bytes:02x?}");
    assert_eq!(refused, 1, "refused and counted");
}

/// **`?1005` writes UTF-8 whatever the input encoding is.** The mode *is*
/// the encoding -- "UTF-8 Mouse Mode" -- and `[[source: ctlseqs]]` names the
/// cost rather than a fallback: "the encoded coordinates will not pass
/// through luit(1) unchanged, e.g., for locales using non-UTF-8 encoding."
/// xterm's emit path reads no encoding (`EmitMousePosition`,
/// `[[source: xterm-button]]`). So under `ESC % @` the bytes are the same.
#[test]
fn utf8_encoding_ignores_the_input_encoding() {
    let at96 = ev(MouseButton::Left, MouseKind::Press, 95, 0);
    let utf8 = sent(b"\x1b[?1000h\x1b[?1005h", at96).0;
    let latin1 = sent(b"\x1b%@\x1b[?1000h\x1b[?1005h", at96).0;
    assert_eq!(utf8, b"\x1b[M\x20\xc2\x80\x21");
    assert_eq!(latin1, utf8);
}

/// **`?1015` is X10's button value in decimal, and `M` every time.**
/// `[[source: ctlseqs]]`, "URXVT (1015)": "CSI followed by semicolon-separated
/// encoded button value, the Px and Py ordinates and final character M. This
/// uses the same button encoding as X10, but printing it as a decimal integer
/// rather than as a single byte." The same button encoding keeps X10's `+ 32`
/// and its `3` for a release (`EmitButtonCode` falls through to the decimal
/// with the 32 still on, `[[source: xterm-button]]`).
#[test]
fn urxvt_is_the_x10_button_value_in_decimal() {
    let s = b"\x1b[?1002h\x1b[?1015h";
    assert_eq!(
        sent(s, ev(MouseButton::Left, MouseKind::Press, 0, 0)).0,
        b"\x1b[32;1;1M"
    );
    assert_eq!(
        sent(s, ev(MouseButton::Left, MouseKind::Release, 0, 0)).0,
        b"\x1b[35;1;1M",
        "a release is 3 + 32, and the final is still M"
    );
    assert_eq!(
        sent(s, ev(MouseButton::Left, MouseKind::Press, 399, 41)).0,
        b"\x1b[32;400;42M",
        "no 223 limit"
    );
    assert_eq!(
        sent(s, ev(MouseButton::Left, MouseKind::Motion, 2, 2)).0,
        b"\x1b[64;3;3M",
        "a drag adds 32"
    );
    assert_eq!(
        sent(s, ev(MouseButton::WheelDown, MouseKind::Press, 0, 0)).0,
        b"\x1b[97;1;1M"
    );
}

/// **`?1016` is `?1006` with the position in pixels.** `[[source: ctlseqs]]`,
/// "SGR-Pixels (1016)": "Use the same mouse response format as the 1006
/// control, but report position in pixels rather than character cells."
/// One-based, as xterm writes it: `value + 1` of the pointer less the border
/// (`EditorButton`, `EmitMousePosition`, `[[source: xterm-button]]`) -- ctlseqs
/// says nothing of where pixel one is.
#[test]
fn sgr_pixels_reports_the_pointer_in_pixels() {
    let s = b"\x1b[?1000h\x1b[?1016h";
    let press = ev(MouseButton::Left, MouseKind::Press, 4, 5);
    assert_eq!(
        sent(s, at_px(press, ImagePx::of(37), ImagePx::of(81))).0,
        b"\x1b[<0;38;82M"
    );
    let release = ev(MouseButton::Left, MouseKind::Release, 4, 5);
    assert_eq!(
        sent(s, at_px(release, ImagePx::of(37), ImagePx::of(81))).0,
        b"\x1b[<0;38;82m",
        "SGR's release final"
    );
    assert_eq!(
        sent(s, at_px(press, ImagePx::ZERO, ImagePx::ZERO)).0,
        b"\x1b[<0;1;1M",
        "the first pixel is 1"
    );
}

/// **No pixel position, no `?1016` report** -- refused and counted rather
/// than made up from the cell. And a pixel the field cannot carry one-based,
/// `u16::MAX` -- which is where `ImagePx` saturates, "at least this far" --
/// is refused the same way.
#[test]
fn sgr_pixels_refuses_an_event_with_no_pixel_position() {
    let s = b"\x1b[?1000h\x1b[?1016h";
    let press = ev(MouseButton::Left, MouseKind::Press, 4, 5);
    let (bytes, refused) = sent(s, press);
    assert!(bytes.is_empty(), "no pixel, no report: {bytes:02x?}");
    assert_eq!(refused, 1);
    let (bytes, refused) = sent(s, at_px(press, ImagePx::of(u16::MAX), ImagePx::ZERO));
    assert!(bytes.is_empty(), "{bytes:02x?}");
    assert_eq!(refused, 1);
}

/// **Setting an encoding replaces the one in force**: "mutually exclusive"
/// (`[[source: ctlseqs]]`). The later one wins whichever it is.
#[test]
fn setting_an_encoding_replaces_the_one_in_force() {
    let press = ev(MouseButton::Left, MouseKind::Press, 0, 0);
    assert_eq!(
        sent(b"\x1b[?1000h\x1b[?1015h\x1b[?1006h", press).0,
        b"\x1b[<0;1;1M"
    );
    assert_eq!(
        sent(b"\x1b[?1000h\x1b[?1006h\x1b[?1015h", press).0,
        b"\x1b[32;1;1M"
    );
    assert_eq!(
        sent(b"\x1b[?1000h\x1b[?1006h\x1b[?1005h", press).0,
        b"\x1b[M\x20\x21\x21"
    );
}

/// **Resetting an encoding undoes only that encoding.** ctlseqs says the
/// encodings are exclusive and not what a reset of one does while another is
/// in force; xterm decides: "Rather than choose an arbitrary precedence among
/// the coordinate modes, they are mutually exclusive. For consistency, a
/// reset is only effective against the matching mode" (`srm_EXT_MODE_MOUSE`
/// and its three neighbours, `[[source: xterm-reset]]`'s file). `?1006 l`
/// went to X10 whatever was in force, so `?1015 h ?1006 l` lost the urxvt
/// encoding an application had asked for.
#[test]
fn resetting_an_encoding_undoes_only_that_encoding() {
    let press = ev(MouseButton::Left, MouseKind::Press, 0, 0);
    assert_eq!(
        sent(b"\x1b[?1000h\x1b[?1015h\x1b[?1006l", press).0,
        b"\x1b[32;1;1M",
        "?1006 l does not undo ?1015"
    );
    assert_eq!(
        sent(b"\x1b[?1000h\x1b[?1006h\x1b[?1015l", press).0,
        b"\x1b[<0;1;1M",
        "?1015 l does not undo ?1006"
    );
    assert_eq!(
        sent(b"\x1b[?1000h\x1b[?1015h\x1b[?1015l", press).0,
        b"\x1b[M\x20\x21\x21",
        "and the matching reset returns to X10"
    );
}

/// **DECRQM answers every mouse mode.** Each tracking level and each encoding
/// is `1` only while it is the one in force, as xterm answers them
/// (`do_dec_rqm`: `send_mouse_pos == X10_MOUSE`, `extend_coords == params[0]`,
/// `[[source: xterm-misc]]`). They answered `0`, "not recognized", which
/// told an application the mode did not exist while `?1015` was in fact
/// counted and dropped.
#[test]
fn decrqm_answers_every_mouse_mode() {
    assert_eq!(
        rqm(b"", ALL_MODES),
        b"\x1b[?9;2$y\x1b[?1000;2$y\x1b[?1005;2$y\x1b[?1006;2$y\x1b[?1007;1$y\x1b[?1015;2$y\x1b[?1016;2$y",
        "power-on: X10 encoding, no tracking, alternate scroll on"
    );
    assert_eq!(
        rqm(b"\x1b[?9h", b"\x1b[?9$p\x1b[?1000$p"),
        b"\x1b[?9;1$y\x1b[?1000;2$y"
    );
    assert_eq!(
        rqm(b"\x1b[?9h\x1b[?1000h", b"\x1b[?9$p\x1b[?1000$p"),
        b"\x1b[?9;2$y\x1b[?1000;1$y"
    );
    let enc = b"\x1b[?1005$p\x1b[?1006$p\x1b[?1015$p\x1b[?1016$p";
    assert_eq!(
        rqm(b"\x1b[?1005h", enc),
        b"\x1b[?1005;1$y\x1b[?1006;2$y\x1b[?1015;2$y\x1b[?1016;2$y"
    );
    assert_eq!(
        rqm(b"\x1b[?1015h", enc),
        b"\x1b[?1005;2$y\x1b[?1006;2$y\x1b[?1015;1$y\x1b[?1016;2$y"
    );
    assert_eq!(
        rqm(b"\x1b[?1016h", enc),
        b"\x1b[?1005;2$y\x1b[?1006;2$y\x1b[?1015;2$y\x1b[?1016;1$y"
    );
    assert_eq!(rqm(b"\x1b[?1007l", b"\x1b[?1007$p"), b"\x1b[?1007;2$y");
    assert_eq!(
        rqm(b"\x1b[?1007l\x1b[?1007h", b"\x1b[?1007$p"),
        b"\x1b[?1007;1$y"
    );
}

/// **`RIS` clears the protocol and the encoding, `?9` and `?1015` with
/// them; `DECSTR` keeps both.** The rule `?1000` and `?1006` already follow:
/// xterm resets `send_mouse_pos` and `extend_coords` only in `ReallyReset`'s
/// `if (full)` branch (`[[source: xterm-reset]]`).
#[test]
fn ris_clears_x10_compatibility_and_the_encoding_and_decstr_keeps_them() {
    let press = ev(MouseButton::Left, MouseKind::Press, 0, 0);
    assert!(sent(b"\x1b[?9h\x1b[?1015h\x1bc", press).0.is_empty());
    assert_eq!(
        rqm(b"\x1b[?9h\x1b[?1015h\x1bc", b"\x1b[?9$p\x1b[?1015$p"),
        b"\x1b[?9;2$y\x1b[?1015;2$y"
    );
    assert_eq!(
        sent(b"\x1b[?9h\x1b[?1015h\x1b[!p", press).0,
        b"\x1b[32;1;1M",
        "X10 compatibility's button in urxvt's decimal, through the soft reset"
    );
}

/// **`?1007` survives both resets**, as xterm keeps `alternateScroll`: the
/// flag is read from its resource in `VTInitialize` and written by
/// `srm_ALTERNATE_SCROLL` and nothing else -- `ReallyReset`, soft or hard,
/// never names it (`[[source: xterm-reset]]`'s file). Practice, not text:
/// ctlseqs gives the mode and its resource and says nothing about a reset.
#[test]
fn alternate_scroll_survives_both_resets() {
    assert_eq!(
        rqm(b"\x1b[?1007l\x1bc", b"\x1b[?1007$p"),
        b"\x1b[?1007;2$y",
        "RIS"
    );
    assert_eq!(
        rqm(b"\x1b[?1007l\x1b[!p", b"\x1b[?1007$p"),
        b"\x1b[?1007;2$y",
        "DECSTR"
    );
}

/// **The modes land the same at every read split.** A mode sequence cut
/// between `?` and its digits, or inside `1015`, must set what the whole
/// one sets. The program leaves every axis away from its default and ends
/// with a reset that does not match, so the order matters too.
#[test]
fn the_mouse_modes_land_the_same_at_every_read_split() {
    let program: &[u8] = b"\x1b[?9h\x1b[?1007l\x1b[?1016h\x1b[?1015h\x1b[?1016l";
    let want_rqm: &[u8] = b"\x1b[?9;1$y\x1b[?1000;2$y\x1b[?1005;2$y\x1b[?1006;2$y\x1b[?1007;2$y\x1b[?1015;1$y\x1b[?1016;2$y";
    for size in 1..=program.len() {
        let mut t = T {
            s: Owned::new(&support::CORPUS),
        };
        let mut term = Term::new(t.s.storage(), &support::CORPUS);
        for chunk in program.chunks(size) {
            feed(&mut term, chunk);
        }
        assert_eq!(term.unsupported_param, 0, "read size {size}: counted");
        feed(&mut term, ALL_MODES);
        assert_eq!(term.reply(), want_rqm, "read size {size}");
        term.clear_reply();
        term.mouse_report(ev(MouseButton::Right, MouseKind::Press, 0, 0));
        assert_eq!(term.reply(), b"\x1b[34;1;1M", "read size {size}");
    }
}

/// **herdr's start, as captured (2026-10-09), ends in SGR any-event, and
/// none of it is counted.** It resets all eight modes, `?1006` down to `?9`,
/// and then sets three tracking levels, `?1015` and `?1006`
/// (`.claude/research/scroll/scratch/modes.txt`). Each reset of an encoding
/// not in force is a no-op under xterm's rule; the last `?1006 h` replaces
/// `?1015`.
#[test]
fn herdrs_start_ends_in_sgr_any_event_and_nothing_is_counted() {
    let herdr: &[u8] = b"\x1b[?1006l\x1b[?1016l\x1b[?1015l\x1b[?1005l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?9l\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h";
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    feed(&mut term, herdr);
    assert_eq!(term.unsupported_param, 0, "every mode is acted on");
    assert_eq!(term.mouse_tracking(), MouseTracking::AnyEvent);
    term.clear_reply();
    feed(&mut term, ALL_MODES);
    assert_eq!(
        term.reply(),
        b"\x1b[?9;2$y\x1b[?1000;2$y\x1b[?1005;2$y\x1b[?1006;1$y\x1b[?1007;1$y\x1b[?1015;2$y\x1b[?1016;2$y"
    );
}

/// **The embedder reads both new states**: `?9` is a level
/// `mouse_tracking` names, and `?1007` is `alternate_scroll`, on at power-on,
/// off after `?1007 l`, and kept through `RIS`.
#[test]
fn the_new_states_are_observable() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    assert_eq!(term.alternate_scroll(), AlternateScroll::On);
    feed(&mut term, b"\x1b[?9h\x1b[?1007l");
    assert_eq!(term.mouse_tracking(), MouseTracking::X10Compat);
    assert_eq!(term.alternate_scroll(), AlternateScroll::Off);
    feed(&mut term, b"\x1bc");
    assert_eq!(term.mouse_tracking(), MouseTracking::Off);
    assert_eq!(
        term.alternate_scroll(),
        AlternateScroll::Off,
        "RIS keeps it"
    );
    feed(&mut term, b"\x1b[?1007h");
    assert_eq!(term.alternate_scroll(), AlternateScroll::On);
}
