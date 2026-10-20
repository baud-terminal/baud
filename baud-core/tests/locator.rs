//! **DEC's text locator, driven by the pointer**: `Term::locator` given
//! moves, presses and releases, and the DECLRP reports DECELR, DECSLE,
//! DECEFR and DECRQLP make of them (`[[source: dec-std-070]]` 070-13).
//! The controls alone, before any pointer, are `tests/vt420/locator.rs`.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

fn show(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

/// The cell at zero-based `col`, `row`, its pixel ten by twenty a cell
/// plus `(3, 4)`.
fn cell(col: usize, row: usize) -> Option<LocatorAt> {
    Some(LocatorAt {
        col: Col::at_index(col),
        row: Row::from_index(row),
        px: Some(PointerPx {
            x: ImagePx::of(u16::try_from(col * 10 + 3).unwrap()),
            y: ImagePx::of(u16::try_from(row * 20 + 4).unwrap()),
        }),
    })
}

fn moved(at: Option<LocatorAt>, held: LocatorButtons) -> LocatorInput {
    LocatorInput {
        kind: LocatorKind::Moved,
        at,
        held,
    }
}

fn press(b: LocatorButton, at: Option<LocatorAt>, held: LocatorButtons) -> LocatorInput {
    LocatorInput {
        kind: LocatorKind::Press(b),
        at,
        held: held.pressed(b),
    }
}

fn release(b: LocatorButton, at: Option<LocatorAt>, held: LocatorButtons) -> LocatorInput {
    LocatorInput {
        kind: LocatorKind::Release(b),
        at,
        held: held.released(b),
    }
}

const NONE: LocatorButtons = LocatorButtons::NONE;

/// A terminal after `setup`, then each step -- bytes or a pointer event --
/// and the reply after each, the buffer cleared between.
enum Step {
    Bytes(&'static [u8]),
    Pointer(LocatorInput),
}

fn run(setup: &[u8], steps: &[Step]) -> Vec<String> {
    support::with_term(&WS, |t| {
        support::feed(t, setup);
        t.clear_reply();
        steps
            .iter()
            .map(|s| {
                match s {
                    Step::Bytes(b) => support::feed(t, b),
                    Step::Pointer(e) => {
                        let sent = t.locator(*e);
                        assert_eq!(sent, !t.reply().is_empty(), "the return says");
                    }
                }
                let r = show(t.reply());
                t.clear_reply();
                r
            })
            .collect()
    })
}

use LocatorButton::{Fourth, Left, Middle, Right};
use Step::{Bytes, Pointer};

/// **DECSLE selects which transitions report, in order**: none at
/// power-up ("0-Respond only to explicit host requests (default)"), 1 and
/// 3 add the down and up transitions, 2 and 4 take them away, and one
/// sequence carries several.
#[test]
fn decsle_selects_which_button_transitions_report_a_declrp() {
    let at = cell(4, 2);
    let r = run(
        b"\x1b[1'z",
        &[
            Pointer(press(Left, at, NONE)),
            Pointer(release(Left, at, NONE)),
            Bytes(b"\x1b[1'{"),
            Pointer(press(Left, at, NONE)),
            Pointer(release(Left, at, NONE)),
            Bytes(b"\x1b[3'{"),
            Pointer(release(Left, at, NONE)),
            Bytes(b"\x1b[2;4'{"),
            Pointer(press(Left, at, NONE)),
            Pointer(release(Left, at, NONE)),
            Bytes(b"\x1b[1;3'{"),
            Pointer(press(Left, at, NONE)),
            Pointer(release(Left, at, NONE)),
        ],
    );
    let down = show(b"\x1b[2;4;3;5&w");
    let up = show(b"\x1b[3;0;3;5&w");
    assert_eq!(
        r,
        ["", "", "", &down, "", "", &up, "", "", "", "", &down, &up]
    );
}

/// **Each button's codes and the held mask**: "2-Left button down ...
/// 9-Fourth button up", and "Pb ... 1-Right 2-Middle 4-Left 8-Fourth",
/// the state after the transition, as xterm reports it.
#[test]
fn declrp_names_each_button_and_the_buttons_held_after_it() {
    let at = cell(0, 0);
    let left = NONE.pressed(Left);
    let r = run(
        b"\x1b[1'z\x1b[1;3'{",
        &[
            Pointer(press(Left, at, NONE)),
            Pointer(press(Middle, at, left)),
            Pointer(press(Right, at, left)),
            Pointer(press(Fourth, at, left)),
            Pointer(release(Fourth, at, left.pressed(Fourth))),
            Pointer(release(Right, at, left.pressed(Right))),
            Pointer(release(Middle, at, left.pressed(Middle))),
            Pointer(release(Left, at, left)),
        ],
    );
    let want = [
        "\x1b[2;4;1;1&w",
        "\x1b[4;6;1;1&w",
        "\x1b[6;5;1;1&w",
        "\x1b[8;12;1;1&w",
        "\x1b[9;4;1;1&w",
        "\x1b[7;4;1;1&w",
        "\x1b[5;4;1;1&w",
        "\x1b[3;0;1;1&w",
    ];
    assert_eq!(r, want.map(|s| show(s.as_bytes())));
}

/// **DECRQLP reports where the pointer last was and what it holds**, in
/// cells or, under `Pu` 1, in device pixels, both from 1:1 -- "The origin
/// is coordinate pair 1:1 in the upper-left corner". The pointer is kept
/// while the locator is off, so a request right after DECELR knows it.
#[test]
fn decrqlp_reports_the_pointer_and_its_buttons_in_cells_or_pixels() {
    let at = cell(4, 2);
    let r = run(
        b"",
        &[
            Pointer(moved(at, NONE.pressed(Right))),
            Bytes(b"\x1b[1'z\x1b['|"),
            Bytes(b"\x1b[1;1'z\x1b[1'|"),
            Bytes(b"\x1b[1;2'z\x1b[0'|"),
        ],
    );
    assert_eq!(
        r,
        [
            String::new(),
            show(b"\x1b[1;1;3;5&w"),
            show(b"\x1b[1;1;45;44&w"),
            show(b"\x1b[1;1;3;5&w"),
        ]
    );
}

/// **A position outside the text area is undefined**: "Pressing a button
/// on the locator shall generate a report with omitted coordinates"
/// (070-13 § 3.1), the event and buttons kept; xterm sends `CSI 0 & w`
/// there. A pixel the embedder did not give is undefined in pixel units,
/// as `?1016` refuses one.
#[test]
fn a_press_with_the_position_undefined_omits_the_coordinates() {
    let no_px = Some(LocatorAt {
        px: None,
        ..cell(1, 1).unwrap()
    });
    let r = run(
        b"\x1b[1'z\x1b[1'{",
        &[
            Pointer(press(Left, None, NONE)),
            Bytes(b"\x1b['|"),
            Bytes(b"\x1b[1;1'z"),
            Pointer(press(Right, no_px, NONE)),
            Bytes(b"\x1b[1;2'z"),
            Pointer(press(Right, no_px, NONE)),
            Pointer(press(Right, cell(25, 1), NONE)),
        ],
    );
    assert_eq!(
        r,
        [
            show(b"\x1b[2;4&w"),
            show(b"\x1b[1;4&w"),
            String::new(),
            show(b"\x1b[6;1&w"),
            String::new(),
            show(b"\x1b[6;1;2;2&w"),
            show(b"\x1b[6;1&w"),
        ]
    );
}

/// **Under DECOM, cells count from the margins and outside them the
/// position is undefined**: § 3.1's example, "the input cursor is outside
/// the active scrolling region, and the origin mode is set to relative".
/// Pixels stay counted from the text area (Jev, PIXELS_ABSOLUTE, 0.96 and
/// 0.94). xterm ignores DECOM.
#[test]
fn under_decom_cells_count_from_the_margins_and_pixels_do_not() {
    // Rows 3-6 and, under DECLRMM, columns 4-10, one-based.
    let margins = b"\x1b[3;6r\x1b[?69h\x1b[4;10s\x1b[?6h\x1b[1'z\x1b[1'{";
    let r = run(
        margins,
        &[
            Pointer(press(Left, cell(3, 2), NONE)),
            Pointer(press(Left, cell(9, 5), NONE)),
            Pointer(press(Left, cell(3, 1), NONE)),
            Pointer(press(Left, cell(10, 2), NONE)),
            Bytes(b"\x1b[1;1'z"),
            Pointer(press(Left, cell(3, 1), NONE)),
            Bytes(b"\x1b[?6l\x1b[1'z"),
            Pointer(press(Left, cell(3, 1), NONE)),
        ],
    );
    assert_eq!(
        r,
        [
            show(b"\x1b[2;4;1;1&w"),
            show(b"\x1b[2;4;4;7&w"),
            show(b"\x1b[2;4&w"),
            show(b"\x1b[2;4&w"),
            String::new(),
            show(b"\x1b[2;4;25;34&w"),
            String::new(),
            show(b"\x1b[2;4;2;4&w"),
        ]
    );
}

/// **DECEFR reports once, when the pointer leaves the rectangle**, "the
/// rectangle is de-activated"; inside, the boundaries included, nothing.
#[test]
fn decefr_reports_once_when_the_pointer_leaves_the_rectangle() {
    let r = run(
        b"\x1b[1'z",
        &[
            Pointer(moved(cell(4, 2), NONE)),
            Bytes(b"\x1b[2;2;6;8'w"),
            Pointer(moved(cell(7, 5), NONE)),
            Pointer(moved(cell(1, 1), NONE)),
            Pointer(moved(cell(8, 5), NONE.pressed(Left))),
            Pointer(moved(cell(12, 7), NONE)),
        ],
    );
    assert_eq!(
        r,
        [
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            show(b"\x1b[10;4;6;9&w"),
            String::new(),
        ]
    );
}

/// **Omitted corners are the pointer's own**, so DECEFR with none reports
/// "any locator movement" -- into another cell, or in pixel units another
/// pixel; a rectangle that does not contain the pointer reports at once,
/// "immediately and de-activate it" -- and so does one whose top is below
/// its bottom, which contains no position (Jev's lean, AS_GIVEN, 0.89 and
/// 0.85).
#[test]
fn decefr_takes_omitted_corners_from_the_pointer_and_reports_at_once_outside() {
    let r = run(
        b"\x1b[1'z",
        &[
            Pointer(moved(cell(4, 2), NONE)),
            Bytes(b"\x1b['w"),
            Pointer(moved(cell(4, 2), NONE)),
            Pointer(moved(cell(5, 2), NONE)),
            Bytes(b"\x1b[5;1;8;20'w"),
            Bytes(b"\x1b[3;9;1;1'w"),
            Bytes(b"\x1b[1;1'z\x1b['w"),
            Pointer(moved(
                Some(LocatorAt {
                    px: Some(PointerPx {
                        x: ImagePx::of(54),
                        y: ImagePx::of(44),
                    }),
                    ..cell(5, 2).unwrap()
                }),
                NONE,
            )),
        ],
    );
    assert_eq!(
        r,
        [
            String::new(),
            String::new(),
            String::new(),
            show(b"\x1b[10;0;3;6&w"),
            show(b"\x1b[10;0;3;6&w"),
            show(b"\x1b[10;0;3;6&w"),
            String::new(),
            show(b"\x1b[10;0;45;55&w"),
        ]
    );
}

/// **Leaving the window reports with the position undefined, and a
/// rectangle sent while the pointer is away waits for it** (DECEFR note
/// 2): "If a filter rectangle lies on the edge of the defined coordinate
/// space ... and the locator crosses that edge, the rectangle shall be
/// triggered to send a report with omitted coordinates"; with every corner
/// omitted, "the rectangle shall be triggered the next time the locator is
/// within the defined coordinate space".
#[test]
fn decefr_reports_the_pointer_leaving_and_waits_for_it_to_come_back() {
    let r = run(
        b"\x1b[1'z",
        &[
            Pointer(moved(cell(4, 2), NONE)),
            Bytes(b"\x1b[1;1;8;20'w"),
            Pointer(moved(None, NONE)),
            Bytes(b"\x1b['w"),
            Pointer(moved(None, NONE)),
            Pointer(moved(cell(0, 7), NONE)),
            Pointer(moved(cell(1, 7), NONE)),
        ],
    );
    assert_eq!(
        r,
        [
            String::new(),
            String::new(),
            show(b"\x1b[10;0&w"),
            String::new(),
            String::new(),
            show(b"\x1b[10;0;8;1&w"),
            String::new(),
        ]
    );
}

/// "0-Respond only to explicit host requests (default, also cancels any
/// pending filter rectangle)" (DECSLE), and "DECELR always cancels any
/// previous rectangle definition" (DECEFR note 1).
#[test]
fn decsle_0_and_decelr_cancel_the_filter_rectangle() {
    for cancel in [&b"\x1b[0'{"[..], b"\x1b['{", b"\x1b[1'z", b"\x1b[1;1'z"] {
        let v: &'static [u8] = cancel.to_vec().leak();
        let r = run(
            b"\x1b[1'z",
            &[
                Pointer(moved(cell(4, 2), NONE)),
                Bytes(b"\x1b['w"),
                Bytes(v),
                Pointer(moved(cell(9, 6), NONE)),
            ],
        );
        assert_eq!(r, ["", "", "", ""], "{}", show(cancel));
    }
}

/// **A press outside the rectangle reports the press, then the
/// rectangle**: "In the rare situation where two events occur
/// simultaneously ... The order of reporting shall be by increasing event
/// code number" (070-13 § 3).
#[test]
fn a_press_outside_the_rectangle_reports_the_press_first() {
    let r = run(
        b"\x1b[1'z\x1b[1'{",
        &[
            Pointer(moved(cell(4, 2), NONE)),
            Bytes(b"\x1b['w"),
            Pointer(press(Left, cell(6, 3), NONE)),
        ],
    );
    assert_eq!(r[2], show(b"\x1b[2;4;4;7&w\x1b[10;4;4;7&w"));
}

/// **A one-shot DECELR reports once and is off**: "the next trigger event
/// that occurs generates a single locator report. No further locator
/// reports occur (the locator is disabled), until another DECELR sequence
/// is received" -- and off is off, so the pointer goes back to the
/// selection.
#[test]
fn a_one_shot_decelr_reports_one_press_and_disables_the_locator() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2'z\x1b[1;3'{");
        assert_eq!(t.mouse_tracking(), MouseTracking::Locator);
        assert!(t.locator(press(Left, cell(0, 0), NONE)));
        assert_eq!(show(t.reply()), show(b"\x1b[2;4;1;1&w"));
        assert_eq!(t.mouse_tracking(), MouseTracking::Off);
        t.clear_reply();
        assert!(!t.locator(release(Left, cell(0, 0), NONE)));
        assert!(t.reply().is_empty());
    });
}

/// **xterm's DECterm quirks are not followed**: xterm turns the whole
/// locator off when DECRQLP arrives or a button is pressed while a
/// rectangle is armed, "This might be a bug, but I don't know, so I'll
/// emulate it anyway" (`[[source: xterm-button-c]]`); 070-13 says no such
/// thing, so the locator and its rectangle stay.
#[test]
fn a_request_or_a_press_inside_the_rectangle_leaves_the_locator_on() {
    let r = run(
        b"\x1b[1'z\x1b[1'{",
        &[
            Pointer(moved(cell(4, 2), NONE)),
            Bytes(b"\x1b[1;1;4;10'w"),
            Bytes(b"\x1b['|"),
            Pointer(press(Left, cell(5, 2), NONE)),
            Pointer(moved(cell(12, 2), NONE.pressed(Left))),
        ],
    );
    assert_eq!(
        r,
        [
            String::new(),
            String::new(),
            show(b"\x1b[1;0;3;5&w"),
            show(b"\x1b[2;4;3;6&w"),
            show(b"\x1b[10;4;3;13&w"),
        ]
    );
}

/// **Under the locator, `Term::mouse_report` sends nothing**: the two are
/// exclusive protocols, and the locator's reports are `Term::locator`'s.
/// With the locator off, `Term::locator` sends nothing.
#[test]
fn the_locator_and_the_xterm_mouse_reports_do_not_mix() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?1000h\x1b[1'{");
        assert!(!t.locator(press(Left, cell(0, 0), NONE)));
        support::feed(t, b"\x1b[1'z");
        let e = MouseEvent {
            button: MouseButton::Left,
            mods: MouseMods::NONE,
            kind: MouseKind::Press,
            col: Col::ZERO,
            row: Row::ZERO,
            px: None,
        };
        assert!(!t.mouse_report(e));
        assert!(t.reply().is_empty());
    });
}

/// **A report the buffer cannot take is dropped whole and counted**, and
/// a one-shot DECELR stays on for the next, as an unsent report told the
/// program nothing.
#[test]
fn a_report_that_does_not_fit_is_dropped_and_a_one_shot_waits() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2'z\x1b[1'{");
        let before = t.reply_overflows;
        while t.reply_overflows == before {
            support::feed(t, b"\x1b[5n");
        }
        let full = t.reply_overflows;
        assert!(!t.locator(press(Left, cell(0, 0), NONE)));
        assert_eq!(t.reply_overflows, full + 1, "counted");
        assert_eq!(t.mouse_tracking(), MouseTracking::Locator, "still on");
        t.clear_reply();
        assert!(t.locator(press(Left, cell(0, 0), NONE)));
        assert_eq!(show(t.reply()), show(b"\x1b[2;4;1;1&w"));
        assert_eq!(t.mouse_tracking(), MouseTracking::Off);
    });
}
