//! A blinking cursor's off phase reaches the pixels on the CPU path, and only
//! a blinking cursor's.
//!
//! The driver owns the clock and hands the renderer a phase
//! ([`Renderer::set_cursor_blink`]); the renderer's one rule
//! ([`Renderer::cursor_drawn`]) hides a cursor whose style blinks in the off
//! phase and never a steady one. Every frame here is compared against a frame
//! with `DECTCEM` off, which draws no cursor by a different route: a hidden
//! phase must be exactly that frame, not merely different from the shown one.
//!
//! The pixel oracle in `baud-present-macos` holds the two GPU arms to each
//! other on the same rule; this holds the CPU path, which that oracle cannot
//! see, so a rule lost from every path at once is still caught.

mod support;

use baud_core::{CursorStyle, Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Blink, Damage, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 12,
};

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The digest of one full frame of `bytes` on a fresh terminal, drawn in
/// `phase`.
fn frame(bytes: &[u8], phase: Blink) -> u64 {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    r.set_cursor_blink(phase);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    canvas.digest()
}

/// Text with the cursor after it, in each shape: the off phase of a blinking
/// cursor is the frame with no cursor at all, and its on phase is not.
#[test]
fn a_blinking_cursors_off_phase_is_the_frame_without_a_cursor() {
    for shape in [b"1", b"3", b"5"] {
        let mut shown = b"ab\x1b[".to_vec();
        shown.extend_from_slice(shape);
        shown.extend_from_slice(b" q");
        let mut none = shown.clone();
        none.extend_from_slice(b"\x1b[?25l");
        let hidden = frame(&shown, Blink::Hidden);
        assert_eq!(
            hidden,
            frame(&none, Blink::Shown),
            "Ps {}: the off phase is not the frame without a cursor",
            char::from(shape[0])
        );
        assert_ne!(
            frame(&shown, Blink::Shown),
            hidden,
            "Ps {}: the on phase draws no cursor",
            char::from(shape[0])
        );
    }
}

/// A steady cursor is drawn in either phase: the rule hides only what blinks.
#[test]
fn a_steady_cursor_is_drawn_in_either_phase() {
    for shape in [b"2", b"4", b"6"] {
        let mut bytes = b"ab\x1b[".to_vec();
        bytes.extend_from_slice(shape);
        bytes.extend_from_slice(b" q");
        assert_eq!(
            frame(&bytes, Blink::Hidden),
            frame(&bytes, Blink::Shown),
            "Ps {}: a steady cursor blinked",
            char::from(shape[0])
        );
    }
}

/// The rule itself, as a driver reads it for the cell path.
#[test]
fn the_rule_hides_only_a_blinking_style_in_the_off_phase() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces);
    r.set_cursor_blink(Blink::Hidden);
    assert!(!r.cursor_drawn(CursorStyle::BlinkingBlock));
    assert!(!r.cursor_drawn(CursorStyle::BlinkingUnderline));
    assert!(!r.cursor_drawn(CursorStyle::BlinkingBar));
    assert!(r.cursor_drawn(CursorStyle::SteadyBlock));
    assert!(r.cursor_drawn(CursorStyle::SteadyBar));
    r.set_cursor_blink(Blink::Shown);
    assert!(r.cursor_drawn(CursorStyle::BlinkingBlock));
}

/// A flip draws nothing else: an incremental frame into each phase equals a
/// full repaint of it, so the cursor's row is repainted and no other row
/// needs to be -- the blink costs a row a flip on this path, not the screen.
#[test]
fn a_flip_repaints_incrementally_to_what_a_full_frame_paints() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let bytes = b"one\r\ntwo\r\nab";
    let full = |phase| frame(bytes, phase);

    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut canvas = support::Canvas::for_renderer(&r);
    r.set_cursor_blink(Blink::Shown);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    assert_eq!(canvas.digest(), full(Blink::Shown));
    for phase in [Blink::Hidden, Blink::Shown, Blink::Hidden] {
        r.set_cursor_blink(phase);
        let f = support::paint(&mut r, &mut t, &mut canvas, Damage::Incremental);
        assert_eq!(canvas.digest(), full(phase), "{phase:?}");
        assert_eq!(
            f.rows_dirty.value(),
            1,
            "{phase:?}: a flip repainted more than the cursor's row"
        );
    }
}
