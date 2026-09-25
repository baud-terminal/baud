//! DECRQM (mode reporting) and DECSCUSR (cursor style).
//!
//! Both are here because both are ways for an application to *find out* rather
//! than to command, and both fail in the same way if done carelessly: a wrong
//! answer is acted on as if it were right.
//!
//! `[[source: ctlseqs]]`:
//! * `CSI Ps $ p` — "Request ANSI mode (DECRQM) ... reply DECRPM is
//!   `CSI Ps ; Pm $ y` where Ps is the mode number as in SM/RM, and Pm is the
//!   mode value: 0 - not recognized, 1 - set, 2 - reset, 3 - permanently set,
//!   4 - permanently reset."
//! * `CSI ? Ps $ p` — the same for the private namespace, replying
//!   `CSI ? Ps ; Pm $ y`.
//! * `CSI Ps SP q` — "Set cursor style (DECSCUSR), VT520."

use crate::support;
use baud_core::*;

fn drive(bytes: &[u8]) -> (Vec<u8>, CursorStyle) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    (t.reply().to_vec(), t.cursor_style())
}

fn replies(bytes: &[u8]) -> Vec<u8> {
    drive(bytes).0
}

fn style(bytes: &[u8]) -> CursorStyle {
    drive(bytes).1
}

// ── DECRQM ──────────────────────────────────────────────────────────────────

/// A private mode that is set reports 1, and reset reports 2.
#[test]
fn decrqm_reports_set_and_reset() {
    assert_eq!(replies(b"\x1b[?7h\x1b[?7$p"), b"\x1b[?7;1$y", "DECAWM set");
    assert_eq!(
        replies(b"\x1b[?7l\x1b[?7$p"),
        b"\x1b[?7;2$y",
        "DECAWM reset"
    );
}

/// It tracks the mode rather than reporting a constant.
#[test]
fn decrqm_follows_the_mode() {
    assert_eq!(
        replies(b"\x1b[?25$p"),
        b"\x1b[?25;1$y",
        "cursor starts visible"
    );
    assert_eq!(replies(b"\x1b[?25l\x1b[?25$p"), b"\x1b[?25;2$y");
    assert_eq!(replies(b"\x1b[?25l\x1b[?25h\x1b[?25$p"), b"\x1b[?25;1$y");
}

/// The private and ANSI namespaces are distinct, and the reply says which.
///
/// `CSI 4` is IRM; `CSI ? 4` is DECSCLM, which baud does not implement. An
/// answer that dropped the `?` would be read as an answer about IRM.
#[test]
fn decrqm_keeps_the_namespaces_apart() {
    // `CSI 4` is IRM; `CSI ? 4` is DECSCLM. Both are implemented, and they are
    // unrelated modes that happen to share a number — so setting one must not
    // move the other. This used to assert `? 4` was unrecognised, which was
    // only true while DECSCLM was missing; asserting independence instead
    // survives implementing it and is the stronger property anyway.
    assert_eq!(replies(b"\x1b[4$p"), b"\x1b[4;2$y", "IRM starts reset");
    assert_eq!(
        replies(b"\x1b[?4$p"),
        b"\x1b[?4;2$y",
        "DECSCLM starts reset"
    );

    // Set the ANSI one; the private one must not move.
    assert_eq!(replies(b"\x1b[4h\x1b[4$p"), b"\x1b[4;1$y");
    assert_eq!(replies(b"\x1b[4h\x1b[?4$p"), b"\x1b[?4;2$y");

    // Set the private one; the ANSI one must not move.
    assert_eq!(replies(b"\x1b[?4h\x1b[?4$p"), b"\x1b[?4;1$y");
    assert_eq!(replies(b"\x1b[?4h\x1b[4$p"), b"\x1b[4;2$y");
}

/// A mode baud does not implement answers 0, "not recognized".
///
/// This is the correct answer, not a fallback. Answering 2 ("reset") would tell
/// the application the mode exists and is off, so it would set the mode and
/// believe it took effect.
#[test]
fn decrqm_says_not_recognized_for_unimplemented_modes() {
    // Modes with no number in any specification, which cannot graduate.
    //
    // Three examples have graduated out of this test: `?1000` when mouse
    // reporting landed, then `?1001` when highlight tracking was recorded
    // rather than refused. That is the point of the answer being *tracked*
    // rather than constant -- and a reason to pick examples here that are not
    // real modes waiting to be implemented.
    assert_eq!(replies(b"\x1b[?9999$p"), b"\x1b[?9999;0$y");
    assert_eq!(replies(b"\x1b[?31337$p"), b"\x1b[?31337;0$y");
    assert_eq!(replies(b"\x1b[99$p"), b"\x1b[99;0$y");
}

/// LNM is a real ANSI mode and is reset by default.
#[test]
fn decrqm_answers_the_ansi_namespace() {
    assert_eq!(replies(b"\x1b[20$p"), b"\x1b[20;2$y");
}

// ── DECSCUSR ────────────────────────────────────────────────────────────────

/// Every shape the spec lists maps to a distinct style.
#[test]
fn decscusr_sets_each_documented_shape() {
    assert_eq!(style(b"\x1b[2 q"), CursorStyle::SteadyBlock);
    assert_eq!(style(b"\x1b[3 q"), CursorStyle::BlinkingUnderline);
    assert_eq!(style(b"\x1b[4 q"), CursorStyle::SteadyUnderline);
    assert_eq!(style(b"\x1b[5 q"), CursorStyle::BlinkingBar);
    assert_eq!(style(b"\x1b[6 q"), CursorStyle::SteadyBar);
}

/// 0 and 1 are both the blinking block, and so is the power-on state.
#[test]
fn decscusr_zero_one_and_default_agree() {
    assert_eq!(style(b""), CursorStyle::BlinkingBlock, "power-on");
    assert_eq!(style(b"\x1b[0 q"), CursorStyle::BlinkingBlock);
    assert_eq!(style(b"\x1b[1 q"), CursorStyle::BlinkingBlock);
    assert_eq!(style(b"\x1b[ q"), CursorStyle::BlinkingBlock, "omitted");
}

/// `Ps = 7` restores the power-on style, which unconfigured is the default.
/// What it restores when the embedder configured another is the next test's.
#[test]
fn decscusr_seven_restores_the_default() {
    assert_eq!(style(b"\x1b[5 q\x1b[7 q"), CursorStyle::BlinkingBlock);
}

/// A terminal whose embedder configured a steady bar, fed `bytes`: the style
/// in force and the replies. Every test of what a reset restores runs here,
/// because under the default -- a blinking block, which `Ps = 1` also names
/// -- a reset that fell back to the blinking block and one that restored the
/// configured style read the same.
fn configured(bytes: &[u8]) -> (CursorStyle, Vec<u8>) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.set_initial_cursor_style(CursorStyle::SteadyBar);
    support::feed(&mut t, bytes);
    (t.cursor_style(), t.reply().to_vec())
}

/// The configured style is the power-on style, and the one the embedder can
/// read back.
#[test]
fn the_configured_style_is_the_power_on_style() {
    assert_eq!(configured(b"").0, CursorStyle::SteadyBar);
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    assert_eq!(t.initial_cursor_style(), CursorStyle::BlinkingBlock);
    t.set_initial_cursor_style(CursorStyle::BlinkingUnderline);
    assert_eq!(t.initial_cursor_style(), CursorStyle::BlinkingUnderline);
    assert_eq!(t.cursor_style(), CursorStyle::BlinkingUnderline);
}

/// `Ps = 7`, "initial resources" (`[[source: ctlseqs]]`), restores the
/// configured style, and so does `Ps = 0` -- foot's reading, where xterm's is
/// a blinking block (`CursorPick` records both). `Ps = 1` stays the blinking
/// block the spec names, whatever is configured.
#[test]
fn zero_and_seven_restore_the_configured_style_and_one_is_the_blinking_block() {
    assert_eq!(configured(b"\x1b[3 q\x1b[7 q").0, CursorStyle::SteadyBar);
    assert_eq!(configured(b"\x1b[3 q\x1b[0 q").0, CursorStyle::SteadyBar);
    assert_eq!(
        configured(b"\x1b[3 q\x1b[ q").0,
        CursorStyle::SteadyBar,
        "omitted"
    );
    assert_eq!(configured(b"\x1b[1 q").0, CursorStyle::BlinkingBlock);
    assert_eq!(configured(b"\x1b[2 q").0, CursorStyle::SteadyBlock);
}

/// Both resets restore the configured style: xterm's `ReallyReset` restores
/// the shape and the blink from its resources on both (`[[source:
/// xterm-reset]]`'s file).
#[test]
fn both_resets_restore_the_configured_style() {
    assert_eq!(
        configured(b"\x1b[3 q\x1b[!p").0,
        CursorStyle::SteadyBar,
        "DECSTR"
    );
    assert_eq!(
        configured(b"\x1b[3 q\x1bc").0,
        CursorStyle::SteadyBar,
        "RIS"
    );
}

/// DECRQSS reports the style in force: the configured one before anything
/// asks for another, and after a reset to it.
#[test]
fn decrqss_reports_the_configured_style() {
    assert_eq!(configured(b"\x1bP$q q\x1b\\").1, b"\x1bP1$r6 q\x1b\\");
    assert_eq!(
        configured(b"\x1b[3 q\x1b[0 q\x1bP$q q\x1b\\").1,
        b"\x1bP1$r6 q\x1b\\"
    );
}

/// A configuration names a shape and a blink apart; together they are each of
/// the six styles, and none twice.
#[test]
fn a_shape_and_a_blink_name_each_style() {
    use CursorBlink::*;
    use CursorShape::*;
    let all = [
        (Block, Blinking, CursorStyle::BlinkingBlock),
        (Block, Steady, CursorStyle::SteadyBlock),
        (Underline, Blinking, CursorStyle::BlinkingUnderline),
        (Underline, Steady, CursorStyle::SteadyUnderline),
        (Bar, Blinking, CursorStyle::BlinkingBar),
        (Bar, Steady, CursorStyle::SteadyBar),
    ];
    for (shape, blink, want) in all {
        assert_eq!(CursorStyle::of(shape, blink), want, "{shape:?} {blink:?}");
    }
}

/// `?12` moves the blink axis of the configured style and leaves its shape.
#[test]
fn private_mode_twelve_moves_the_configured_styles_blink() {
    assert_eq!(configured(b"\x1b[?12h").0, CursorStyle::BlinkingBar);
}

/// The blink axis is recoverable even though the wire encodes it with shape.
#[test]
fn blink_is_readable_separately() {
    assert!(CursorStyle::BlinkingBar.blinks());
    assert!(!CursorStyle::SteadyBar.blinks());
    assert!(CursorStyle::BlinkingBlock.blinks());
    assert!(!CursorStyle::SteadyUnderline.blinks());
}

/// An out-of-range parameter leaves the style alone rather than resetting it.
#[test]
fn an_unknown_cursor_style_is_ignored() {
    assert_eq!(style(b"\x1b[5 q\x1b[99 q"), CursorStyle::BlinkingBar);
}

/// DECSCUSR must not be confused with DECSCA: both end in `q`, and only the
/// intermediate byte tells them apart.
#[test]
fn decscusr_and_decsca_do_not_collide() {
    // `CSI 2 " q` is DECSCA (erasable), not a steady block.
    assert_eq!(style(b"\x1b[2\"q"), CursorStyle::BlinkingBlock);
    // `CSI 2 SP q` is the steady block.
    assert_eq!(style(b"\x1b[2 q"), CursorStyle::SteadyBlock);
}

/// RIS returns the cursor style to power-on; configured, to the configured
/// style, which `both_resets_restore_the_configured_style` holds.
#[test]
fn ris_restores_the_cursor_style() {
    assert_eq!(style(b"\x1b[5 q\x1bc"), CursorStyle::BlinkingBlock);
}
