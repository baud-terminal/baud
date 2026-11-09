//! The wheel between AppKit's deltas and the child: travel kept until it is
//! a whole line, and the lines an application tracking the mouse asked for
//! turned into its reports. Pure -- a delta in, lines out; a `Term` and a
//! sink in, bytes out -- because `Driver::scroll` is AppKit-bound and no test
//! reaches it.

use baud_core::{
    AlternateScroll, Col, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind,
    MouseMods, MouseTracking, PointerPx, Row, Screen, ShiftEscape, Term,
};

/// Lines a single wheel event may turn into reports or keys: a trackpad
/// fling accumulates to many, and an application wants a bounded burst.
pub const WHEEL_MAX: u64 = 64;

/// What one wheel event says it moved, in AppKit's units.
#[derive(Clone, Copy, Debug)]
pub enum Delta {
    /// `scrollingDeltaY` with `hasPreciseScrollingDeltas` set -- a trackpad's
    /// or a Magic Mouse's -- in points; `scale` is the backing scale and
    /// `cell` the cell height in pixels.
    Points { dy: f64, scale: f64, cell: f64 },
    /// `scrollingDeltaY` without it -- a wheel's -- in lines.
    Lines(f64),
}

/// Whole lines of wheel, signed: positive is "show earlier lines", the way
/// AppKit delivers it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lines(i64);

impl Lines {
    pub fn get(self) -> i64 {
        self.0
    }
}

/// A line of travel, in the unit travel is kept in: millionths, as an
/// integer. **Not floating point**: ten tenths summed in `f64` are
/// 0.9999999999999999, and the truncation that makes whole lines of that
/// makes none -- the slow wheel this module fixes would have stayed one ulp
/// short of its first line. Rounding each delta to a millionth loses at most
/// half of one an event, a line in a million events.
const LINE: i64 = 1_000_000;

/// Wheel travel not yet a whole line, in millionths of one, signed like the
/// lines it becomes. Both kinds of delta accumulate here, under one rule.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Travel(i64);

impl Travel {
    /// The whole lines `d` completes, truncated toward zero, with the rest
    /// kept for the next event.
    ///
    /// **A wheel's lines accumulate the way a trackpad's points always did.**
    /// They were truncated an event at a time, so a delta under a line was
    /// dropped whole, and a mouse that reports a slow turn in small
    /// `scrollingDeltaY` values -- a tenth of a line a tick -- never scrolled.
    ///
    /// **A reversal starts from nothing**, on both paths. What one direction
    /// left over is dropped when a delta arrives in the other; carried, it
    /// had to be undone first, so half a line up and then down took a line
    /// and a half of travel to move one line. The precise path carried it
    /// until 2026-10-09, so this changes the trackpad as well as the wheel.
    pub fn lines(&mut self, d: Delta) -> Lines {
        let lines = match d {
            Delta::Points { dy, scale, cell } => dy * scale / cell,
            Delta::Lines(dy) => dy,
        };
        // `as` saturates and takes NaN to zero, so no delta wraps the sum.
        let parts = (lines * LINE as f64).round() as i64;
        if parts.signum() * self.0.signum() < 0 {
            self.0 = 0;
        }
        let total = self.0.saturating_add(parts);
        // Integer division truncates toward zero and the remainder keeps the
        // dividend's sign: the `trunc` this replaced, in exact arithmetic.
        self.0 = total % LINE;
        Lines(total / LINE)
    }

    /// Drop what is not yet a line.
    pub fn settle(&mut self) {
        self.0 = 0;
    }
}

/// Which way a wheel event scrolls: up and down the lines, or across the
/// columns -- a tilted wheel, Shift with a mouse wheel on macOS, a
/// trackpad's sideways swipe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Vertical,
    Horizontal,
}

/// **The one axis an event scrolls along: the larger of its two deltas**,
/// the vertical on a tie. AppKit's scroll views do it by default, "Some
/// content is scrollable in both the horizontal and vertical axes, but is
/// predominantly scrolled one axis at a time ... With scroll balls and touch
/// surfaces, it becomes more difficult to determine the user's intention"
/// (`usesPredominantAxisScrolling`, default true,
/// `[[source: apple-nsscrollview-usespredominantaxisscrolling]]`); alacritty
/// keeps one axis of a trackpad's deltas too
/// (`[[source: alacritty-0151-input-mod]]`). Without it a vertical swipe's
/// small sideways deltas reach a program tracking the mouse as buttons 6
/// and 7. Per event, since the event names no gesture to latch an axis to; a
/// wheel's event has one axis anyway.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn predominant(dx: f64, dy: f64) -> Axis {
    match dx.abs() > dy.abs() {
        true => Axis::Horizontal,
        false => Axis::Vertical,
    }
}

/// Where a wheel's lines go, decided from the terminal's state alone so a
/// test can reach it: `Driver::scroll` is AppKit-bound and no test does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// An application tracking the mouse: buttons 4 and 5, through [`report`].
    Report,
    /// The alternate screen under `?1007`: the cursor keys, through [`keys`].
    Keys,
    /// The primary screen: the window over the history moves.
    History,
    /// The alternate screen with `?1007` reset: nothing. It keeps no history
    /// to move, and the application has said it does not want the keys.
    Nothing,
}

/// Which [`Route`] a wheel takes now.
///
/// **Tracking first, whatever `?1007` says**: a wheel is buttons 4 and 5 to
/// an application tracking the mouse, `?9` included -- which reports neither,
/// "button 1, 2 or 3" being all X10 compatibility has, so the wheel there is
/// taken and sends nothing, as xterm's is (`EditorButton`, `[[source:
/// xterm-button]]`). Then the screen: the primary's wheel moves the window
/// over the history, and the alternate screen's sends the cursor keys while
/// `?1007` is set -- `[[source: ctlseqs]]`, "if Alternate Scroll mode is set,
/// then cursor up/down controls are sent when the terminal is displaying the
/// Alternate Screen Buffer" -- and nothing once an application resets it.
///
/// **Shift takes the wheel from tracking under XTSHIFTESCAPE 0**, the
/// default, as xterm's `ShiftOverride` takes every button event Shift
/// holds, the wheel's 4 and 5 included, to its own `scroll-back` and
/// `scroll-forw` (`[[source: xterm-button-c]]`): the wheel is then the
/// screen's, as it is with no tracking. Under `CSI > 1 s` the wheel binds
/// no selection, so it stays the program's, its Shift bit sent.
///
/// **DEC's locator takes no wheel**: 070-13 gives it four buttons and no
/// wheel, so under DECELR the wheel is the screen's, as with no tracking.
/// xterm reports X's buttons 4 and 5, its wheel, as the fourth button's
/// down and as event 10, "outside filter rectangle"
/// (`SendLocatorPosition`, `[[source: xterm-button-c]]`), a numbering
/// artifact not followed.
pub fn route(term: &Term<'_>, mods: MouseMods) -> Route {
    route_on(term, mods, Axis::Vertical)
}

/// [`route`] for a wheel along `axis`. **Across, only a program tracking
/// the mouse takes it**, as buttons 6 and 7; otherwise it does nothing, on
/// either screen: ctlseqs' Alternate Scroll sends "cursor up/down
/// controls" and nothing for the columns, xterm binds nothing to buttons 6
/// and 7 and foot nothing to `BTN_WHEEL_LEFT` and `BTN_WHEEL_RIGHT` by
/// default, and the history has no columns to move across. alacritty sends
/// cursor left and right on the alternate screen, practice not followed.
/// Shift's override takes it as it takes the vertical wheel, so Shift with
/// a mouse wheel, which macOS gives as a sideways scroll, reaches the
/// program only under `CSI > 1 s`.
pub fn route_on(term: &Term<'_>, mods: MouseMods, axis: Axis) -> Route {
    let shifted = mods.has(MouseMods::SHIFT) && term.shift_escape() == ShiftEscape::Override;
    let tracked = !matches!(
        term.mouse_tracking(),
        MouseTracking::Off | MouseTracking::Locator
    );
    if tracked && !shifted {
        return Route::Report;
    }
    if axis == Axis::Horizontal {
        return Route::Nothing;
    }
    match (term.screen(), term.alternate_scroll()) {
        (Screen::Normal, _) => Route::History,
        (Screen::Alternate, AlternateScroll::On) => Route::Keys,
        (Screen::Alternate, AlternateScroll::Off) => Route::Nothing,
    }
}

/// `lines` of wheel as the cursor keys: Up for each line that shows earlier
/// lines and Down for each that shows later ones, at most `WHEEL_MAX`. Keys,
/// not bytes, so each goes through `Term::wheel_key` and is the cursor key
/// as DECCKM has it -- `CSI A` normally and `SS3 A` in application mode --
/// and not a keystroke, which KAM would lock and SRM draw.
pub fn keys(lines: Lines) -> impl Iterator<Item = KeyEvent> {
    let code = if lines.0 > 0 {
        KeyCode::Up
    } else {
        KeyCode::Down
    };
    (0..lines.0.unsigned_abs().min(WHEEL_MAX))
        .map(move |_| KeyEvent::press(code, Modifiers::default()))
}

/// `lines` of wheel to an application tracking the mouse: one press of
/// button 4 or 5 a line at `(row, col)`, at most `WHEEL_MAX` -- and every
/// byte of them handed to `sink` in order, behind whatever the core already
/// owed, because a report must not overtake an answer queued before it.
///
/// **The reply buffer is drained after every report**, not once after the
/// loop. It was 256 bytes then, and an SGR wheel report is ten to sixteen,
/// so the loop that drained it once sent the first 21 lines of a 40-line
/// fling at column 51 and dropped the other 19 -- herdr's state, 2026-10-09.
/// Each report lands in an empty buffer now and none can overflow, so a
/// `false` from `mouse_report` is the application not wanting the event,
/// which is the same answer for every line of it.
///
/// `px` is the same position in pixels, which `?1016` reports instead, and
/// `mods` the modifiers held, which every report carries as a button's does.
pub fn report(
    term: &mut Term<'_>,
    lines: Lines,
    (row, col, px): (Row, Col, PointerPx),
    mods: MouseMods,
    sink: impl FnMut(&[u8]),
) {
    report_on(term, Axis::Vertical, lines, (row, col, px), mods, sink)
}

/// [`report`] along `axis`: across, button 6 for each column toward the
/// start of the line and 7 for each toward its end, "encoded like the
/// wheel mice, by adding 64 (for buttons 6 and 7)" (`[[source: ctlseqs]]`),
/// 6 the left as the X convention numbers them and foot sends
/// `BTN_WHEEL_LEFT` for a step left (`[[source: foot-input]]`) -- ctlseqs' "button 7
/// (left) and button 6 (right) with a Logitech mouse" reads the other way,
/// a mouse's own numbering.
pub fn report_on(
    term: &mut Term<'_>,
    axis: Axis,
    lines: Lines,
    (row, col, px): (Row, Col, PointerPx),
    mods: MouseMods,
    mut sink: impl FnMut(&[u8]),
) {
    let button = match (axis, lines.0 > 0) {
        (Axis::Vertical, true) => MouseButton::WheelUp,
        (Axis::Vertical, false) => MouseButton::WheelDown,
        (Axis::Horizontal, true) => MouseButton::WheelLeft,
        (Axis::Horizontal, false) => MouseButton::WheelRight,
    };
    owed(term, &mut sink);
    for _ in 0..lines.0.unsigned_abs().min(WHEEL_MAX) {
        let queued = term.mouse_report(MouseEvent {
            button,
            mods,
            kind: MouseKind::Press,
            col,
            row,
            px: Some(px),
        });
        if !queued {
            break;
        }
        owed(term, &mut sink);
    }
}

/// The core's reply buffer to `sink`, and emptied.
fn owed(term: &mut Term<'_>, sink: &mut impl FnMut(&[u8])) {
    if !term.reply().is_empty() {
        sink(term.reply());
        term.clear_reply();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{BytePos, ImagePx, Owned, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 40,
        ws_col: 120,
    };

    /// A pixel position for the reports whose encoding does not carry one.
    const PX: PointerPx = PointerPx {
        x: ImagePx::ZERO,
        y: ImagePx::ZERO,
    };

    /// The modes herdr 0.9.3 leaves the terminal in, in the order it sends
    /// them (captured 2026-10-09): the alternate screen, all three tracking
    /// levels with `?1003` last, `?1015` and then `?1006`, which wins.
    const HERDR: &[u8] =
        b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h\x1b[?2004h\x1b[?1004h";

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    /// **The wheel across reaches a program tracking the mouse as buttons
    /// 6 and 7**, toward the start of the line 6 and toward its end 7, one
    /// press a column; with no tracking, and under `?9` or Shift's override,
    /// it goes nowhere, on either screen, `?1007` set or not. Under
    /// XTSHIFTESCAPE 1 Shift leaves it the program's, its Shift bit sent.
    #[test]
    fn the_wheel_across_is_buttons_6_and_7_and_nothing_untracked() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let across = Axis::Horizontal;
        assert_eq!(route_on(&t, MouseMods::NONE, across), Route::Nothing);
        feed(&mut t, b"\x1b[?1049h");
        assert_eq!(t.alternate_scroll(), AlternateScroll::On, "the control");
        assert_eq!(route(&t, MouseMods::NONE), Route::Keys, "the control");
        assert_eq!(route_on(&t, MouseMods::NONE, across), Route::Nothing);
        feed(&mut t, b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(route_on(&t, MouseMods::NONE, across), Route::Report);
        assert_eq!(
            route_on(&t, MouseMods::SHIFT, across),
            Route::Nothing,
            "Shift"
        );
        feed(&mut t, b"\x1b[>1s");
        assert_eq!(
            route_on(&t, MouseMods::SHIFT, across),
            Route::Report,
            "Shift under XTSHIFTESCAPE 1"
        );
        let mut child = Vec::new();
        let at = (Row::from_index(2), Col::at_index(4), PX);
        report_on(&mut t, across, Lines(1), at, MouseMods::SHIFT, |b| {
            child.extend_from_slice(b)
        });
        assert_eq!(child, b"\x1b[<70;5;3M", "button 6 with Shift's 4");
        child.clear();
        feed(&mut t, b"\x1b[>0s");
        report_on(&mut t, across, Lines(2), at, MouseMods::NONE, |b| {
            child.extend_from_slice(b)
        });
        report_on(&mut t, across, Lines(-1), at, MouseMods::NONE, |b| {
            child.extend_from_slice(b)
        });
        assert_eq!(child, b"\x1b[<66;5;3M\x1b[<66;5;3M\x1b[<67;5;3M");
    }

    /// **One axis an event, the larger**: a wheel's event has one, a
    /// trackpad's swipe a little of the other, which goes.
    #[test]
    fn each_event_scrolls_along_its_larger_delta() {
        assert_eq!(predominant(0.0, 3.0), Axis::Vertical);
        assert_eq!(predominant(-2.0, 0.0), Axis::Horizontal);
        assert_eq!(predominant(0.4, -9.5), Axis::Vertical, "a vertical swipe");
        assert_eq!(predominant(-6.0, 1.5), Axis::Horizontal, "a sideways one");
        assert_eq!(predominant(2.0, 2.0), Axis::Vertical, "a tie");
    }

    /// **A 40-line fling reaches the child as 40 reports.** It reached it as
    /// 21: the loop queued a twelve-byte report a line into the core's
    /// 256-byte reply buffer and drained it once, after the loop, so the
    /// 22nd to the 40th were dropped -- counted in `ReplyCounts::overflows`, and
    /// answered `true` by a `mouse_report` that had not queued them.
    #[test]
    fn a_long_fling_under_herdr_reaches_the_child_whole() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, HERDR);
        t.clear_reply();
        let before = t.reply_counts().overflows;
        let mut child = Vec::new();
        report(
            &mut t,
            Lines(40),
            (Row::from_index(15), Col::at_index(50), PX),
            MouseMods::NONE,
            |b| child.extend_from_slice(b),
        );
        let one = b"\x1b[<64;51;16M";
        let reports = child.windows(one.len()).filter(|w| w == one).count();
        assert_eq!(reports, 40, "one report a line, and none dropped");
        assert_eq!(child, one.repeat(40), "and nothing else");
        assert_eq!(t.reply_counts().overflows, before, "nothing overflowed");
        assert!(t.reply().is_empty(), "nothing left behind");
    }

    /// The other direction is button 5, and the cap still holds: a fling
    /// past `WHEEL_MAX` sends `WHEEL_MAX`.
    #[test]
    fn a_fling_down_past_the_cap_sends_the_cap() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, HERDR);
        t.clear_reply();
        let mut child = Vec::new();
        report(
            &mut t,
            Lines(-100),
            (Row::from_index(0), Col::at_index(0), PX),
            MouseMods::NONE,
            |b| child.extend_from_slice(b),
        );
        let one = b"\x1b[<65;1;1M";
        assert_eq!(child, one.repeat(64));
    }

    /// **A reply the core already owed goes first.** A report must not
    /// overtake an answer queued before it: the application reads them in
    /// the order the terminal produced them.
    #[test]
    fn what_the_core_already_owed_goes_ahead_of_the_reports() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, HERDR);
        t.clear_reply();
        feed(&mut t, b"\x1b[5n");
        assert_eq!(t.reply(), b"\x1b[0n", "the control: a reply is owed");
        let mut child = Vec::new();
        report(
            &mut t,
            Lines(2),
            (Row::from_index(0), Col::at_index(0), PX),
            MouseMods::NONE,
            |b| child.extend_from_slice(b),
        );
        assert_eq!(child, b"\x1b[0n\x1b[<64;1;1M\x1b[<64;1;1M");
    }

    /// Without tracking nothing is sent, and the sink is not handed the
    /// empty buffer as if it were a report.
    #[test]
    fn nothing_is_sent_to_an_application_that_does_not_track() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut calls = 0;
        report(
            &mut t,
            Lines(3),
            (Row::from_index(0), Col::at_index(0), PX),
            MouseMods::NONE,
            |b| calls += usize::from(!b.is_empty()),
        );
        assert_eq!(calls, 0);
        assert_eq!(t.reply_counts().overflows, 0);
    }

    /// **`?1007 l` takes the wheel off the arrow keys.** `[[source:
    /// ctlseqs]]`, "Wheel mice": "if Alternate Scroll mode is set, then
    /// cursor up/down controls are sent when the terminal is displaying the
    /// Alternate Screen Buffer." Reset, the alternate screen keeps no history
    /// to move, so the wheel there does nothing -- and it sent the arrows
    /// whatever the mode said, because the mode was counted and dropped.
    #[test]
    fn after_1007l_the_wheel_on_the_alternate_screen_sends_nothing() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b[?1049h\x1b[?1007l");
        let r = route(&t, MouseMods::NONE);
        assert!(
            !matches!(r, Route::Report | Route::Keys | Route::History),
            "after ?1007 l the wheel on the alternate screen sends nothing and moves nothing: {r:?}"
        );
        assert_eq!(r, Route::Nothing);
        assert_eq!(t.alternate_scroll(), AlternateScroll::Off);
        // And `?1007 h` gives the arrows back.
        feed(&mut t, b"\x1b[?1007h");
        assert_eq!(route(&t, MouseMods::NONE), Route::Keys);
    }

    /// **The default is unchanged: arrows on the alternate screen.** baud
    /// sent them before `?1007` was read, and a user sees no difference until
    /// an application resets the mode. That is set at power-on, against
    /// xterm's `alternateScroll` resource, which is `False`
    /// (`[[source: xterm-reset]]`'s file); ctlseqs leaves the initial state
    /// to the resource.
    #[test]
    fn the_alternate_screen_sends_arrows_until_an_application_says_otherwise() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        assert_eq!(
            route(&t, MouseMods::NONE),
            Route::History,
            "the primary moves the history"
        );
        feed(&mut t, b"\x1b[?1049h");
        assert_eq!(route(&t, MouseMods::NONE), Route::Keys);
    }

    /// **Tracking takes the wheel whatever `?1007` says, and the primary
    /// screen moves the history whatever it says.** Alternate scroll is a
    /// rule for the alternate screen without tracking; xterm's wheel is a
    /// report first whenever a protocol is on (`EditorButton`'s callers,
    /// `[[source: xterm-button]]`).
    #[test]
    fn tracking_and_the_primary_screen_ignore_1007() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b[?1007l");
        assert_eq!(route(&t, MouseMods::NONE), Route::History);
        // An application tracking the mouse on the primary screen gets the
        // wheel too: tracking decides before the screen does.
        feed(&mut t, b"\x1b[?1000h");
        assert_eq!(
            route(&t, MouseMods::NONE),
            Route::Report,
            "tracking on the primary screen"
        );
        feed(&mut t, b"\x1b[?1000l\x1b[?1049h\x1b[?1000h");
        assert_eq!(route(&t, MouseMods::NONE), Route::Report);
        feed(&mut t, b"\x1b[?1000l\x1b[?9h");
        assert_eq!(
            route(&t, MouseMods::NONE),
            Route::Report,
            "X10 compatibility is tracking too"
        );
    }

    /// **Shift takes the wheel from tracking under XTSHIFTESCAPE 0, and
    /// leaves it the program's, Shift bit sent, under 1.** xterm's
    /// `ShiftOverride` takes a Shift button event from the protocol for
    /// every button at 0, the wheel's included; at 1 only for a button the
    /// selection binds, and the wheel binds none (`[[source:
    /// xterm-button-c]]`). The wheel went to the program, no bit set,
    /// whatever was held.
    #[test]
    fn shift_takes_the_wheel_from_tracking_only_under_xtshiftescape_0() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(route(&t, MouseMods::NONE), Route::Report, "the control");
        assert_eq!(route(&t, MouseMods::SHIFT), Route::History, "Shift, at 0");
        feed(&mut t, b"\x1b[?1049h");
        assert_eq!(
            route(&t, MouseMods::SHIFT),
            Route::Keys,
            "the alternate screen"
        );
        feed(&mut t, b"\x1b[>1s");
        assert_eq!(route(&t, MouseMods::SHIFT), Route::Report, "Shift, at 1");
        t.clear_reply();
        let mut child = Vec::new();
        report(
            &mut t,
            Lines(1),
            (Row::from_index(0), Col::at_index(0), PX),
            MouseMods::SHIFT,
            |b| child.extend_from_slice(b),
        );
        assert_eq!(child, b"\x1b[<68;1;1M", "button 4 with Shift's 4");
    }

    /// **The wheel's keys are the cursor keys as DECCKM has them**: through
    /// `Term::key`, so `CSI A` normally and `SS3 A` once an application sets
    /// `?1` -- ctlseqs' "cursor up/down controls", which are the keys.
    #[test]
    fn the_wheels_keys_are_the_cursor_keys_as_decckm_has_them() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let sent = |t: &mut Term<'_>, lines| {
            let mut out = Vec::new();
            for k in keys(lines) {
                crate::reads::input(t, |t| t.wheel_key(k), |b| out.extend_from_slice(b));
            }
            out
        };
        feed(&mut t, b"\x1b[?1049h");
        t.clear_reply();
        assert_eq!(sent(&mut t, Lines(2)), b"\x1b[A\x1b[A");
        assert_eq!(sent(&mut t, Lines(-1)), b"\x1b[B");
        feed(&mut t, b"\x1b[?1h");
        t.clear_reply();
        assert_eq!(sent(&mut t, Lines(1)), b"\x1bOA");
        assert_eq!(keys(Lines(-500)).count(), 64, "capped at WHEEL_MAX");
    }

    /// **Under `?1016` the wheel reports the pointer in pixels**: a wheel
    /// event is a button press like any other, and xterm writes it through
    /// the same `EditorButton` (`[[source: xterm-button]]`).
    #[test]
    fn a_wheel_under_1016_reports_the_pointer_in_pixels() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\x1b[?1049h\x1b[?1000h\x1b[?1016h");
        t.clear_reply();
        let mut child = Vec::new();
        let px = PointerPx {
            x: ImagePx::of(33),
            y: ImagePx::of(70),
        };
        report(
            &mut t,
            Lines(1),
            (Row::from_index(1), Col::at_index(2), px),
            MouseMods::NONE,
            |b| child.extend_from_slice(b),
        );
        assert_eq!(child, b"\x1b[<64;34;71M");
    }

    /// **A slow wheel scrolls.** A mouse that sends small `scrollingDeltaY`
    /// values without precise deltas -- kitty's issue 1238 is the same report
    /// from Cocoa -- had each one truncated to zero lines, so ten events of a
    /// tenth of a line moved nothing. Ten tenths are a line.
    #[test]
    fn ten_tenths_of_a_line_are_a_line() {
        let mut w = Travel::default();
        let lines: i64 = (0..10).map(|_| w.lines(Delta::Lines(0.1)).get()).sum();
        assert_eq!(lines, 1);
        // And the next tenth starts the next line, rather than finishing it.
        assert_eq!(w.lines(Delta::Lines(0.1)), Lines(0));
    }

    /// A whole line, and more than one, still lands at once.
    #[test]
    fn whole_lines_land_at_once() {
        let mut w = Travel::default();
        assert_eq!(w.lines(Delta::Lines(3.0)), Lines(3));
        assert_eq!(w.lines(Delta::Lines(-2.0)), Lines(-2));
        assert_eq!(w.lines(Delta::Lines(1.5)), Lines(1));
        assert_eq!(w.lines(Delta::Lines(0.5)), Lines(1), "the half carried");
    }

    /// **A reversal starts from nothing**, on both paths: travel left over
    /// in one direction does not have to be undone before the other moves.
    /// Half a line up, then two halves down, is one line down.
    #[test]
    fn a_reversal_drops_what_the_other_direction_left() {
        let mut w = Travel::default();
        assert_eq!(w.lines(Delta::Lines(0.5)), Lines(0));
        assert_eq!(w.lines(Delta::Lines(-0.5)), Lines(0));
        assert_eq!(w.lines(Delta::Lines(-0.5)), Lines(-1));
    }

    /// **And on a trackpad, which did not reset either.** The precise path
    /// carried the remainder across a reversal; a pure accumulator is one
    /// rule for both, so the reset is the trackpad's too. A trackpad's
    /// points at 2x with a 32-pixel cell: 8 points is half a line.
    #[test]
    fn a_trackpad_reversal_drops_it_too() {
        let mut w = Travel::default();
        let half = |dy| Delta::Points {
            dy,
            scale: 2.0,
            cell: 32.0,
        };
        assert_eq!(w.lines(half(8.0)), Lines(0));
        assert_eq!(w.lines(half(-8.0)), Lines(0));
        assert_eq!(w.lines(half(-8.0)), Lines(-1));
    }

    /// A trackpad's points still accumulate to rows of the cell height.
    #[test]
    fn points_accumulate_to_rows_of_the_cell() {
        let mut w = Travel::default();
        let d = Delta::Points {
            dy: 1.0,
            scale: 2.0,
            cell: 32.0,
        };
        let lines: i64 = (0..16).map(|_| w.lines(d).get()).sum();
        assert_eq!(lines, 1, "sixteen 2-pixel steps are one 32-pixel row");
        let lines: i64 = (0..15).map(|_| w.lines(d).get()).sum();
        assert_eq!(lines, 0, "fifteen are not");
    }

    /// `settle` drops the remainder: the keystroke's snap to the live screen
    /// starts the next wheel from nothing.
    #[test]
    fn settling_drops_the_remainder() {
        let mut w = Travel::default();
        assert_eq!(w.lines(Delta::Lines(0.9)), Lines(0));
        w.settle();
        assert_eq!(w.lines(Delta::Lines(0.2)), Lines(0));
    }
}
