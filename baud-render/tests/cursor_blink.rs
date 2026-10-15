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
use baud_domain::Chan;
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Blink, CursorPhase, Damage, PxSize, Renderer, Stack, Theme};
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
fn frame(bytes: &[u8], phase: impl Into<CursorPhase>) -> u64 {
    pixels(bytes, phase.into()).0.digest()
}

/// One full frame of `bytes` drawn in `phase`, and the renderer's cell size.
fn pixels(bytes: &[u8], phase: CursorPhase) -> (support::Canvas, (usize, usize)) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    r.set_cursor_blink(phase);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    let m = r.metrics();
    let cell = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    (canvas, cell)
}

fn faded(opacity: u8) -> CursorPhase {
    CursorPhase {
        opacity: Chan::from_u8(opacity),
        glow: Chan::from_u8(0),
    }
}

fn glowing(glow: u8) -> CursorPhase {
    CursorPhase {
        opacity: Chan::from_u8(0),
        glow: Chan::from_u8(glow),
    }
}

/// **A fade part way is neither phase, and its ends are the two phases**:
/// a block over a letter, an underline and a bar at half opacity draw a
/// third frame, and at 255 and 0 the shown and hidden ones exactly.
#[test]
fn a_fade_part_way_is_between_the_two_phases_and_its_ends_are_them() {
    for shape in [b"1", b"3", b"5"] {
        let mut bytes = b"ab\x1b[".to_vec();
        bytes.extend_from_slice(shape);
        bytes.extend_from_slice(b" q\x1b[1;2H");
        let (on, off) = (frame(&bytes, Blink::Shown), frame(&bytes, Blink::Hidden));
        assert_ne!(on, off, "the control");
        assert_eq!(frame(&bytes, faded(255)), on);
        assert_eq!(frame(&bytes, faded(0)), off);
        let half = frame(&bytes, faded(128));
        assert!(half != on && half != off, "Ps {}", char::from(shape[0]));
        // A steady style ignores the phase.
        let mut steady = b"ab\x1b[".to_vec();
        steady.push(shape[0] + 1);
        steady.extend_from_slice(b" q\x1b[1;2H");
        assert_eq!(frame(&steady, faded(128)), frame(&steady, Blink::Shown));
    }
}

/// **The plasma glow lands on the cursor's cell and nowhere else**: it
/// differs from the hidden frame, only inside the cell, brighter at the
/// pulse's height than near its ends -- and a steady cursor has none.
#[test]
fn the_glow_lands_on_the_cursor_cell_alone() {
    let bytes = b"ab\x1b[1 q\x1b[2;3H";
    let (off, cell) = pixels(bytes, CursorPhase::HIDDEN);
    let mut moved = Vec::new();
    for g in [40u8, 255] {
        let (lit, _) = pixels(bytes, glowing(g));
        let mut n = 0usize;
        let mut sum = 0u64;
        for (i, (a, b)) in off.pixels.iter().zip(&lit.pixels).enumerate() {
            if a != b {
                let (x, y) = (i % off.width, i / off.width);
                assert!(
                    (2 * cell.0..3 * cell.0).contains(&x) && (cell.1..2 * cell.1).contains(&y),
                    "glow {g} at ({x}, {y}), outside the cursor's cell"
                );
                n += 1;
                sum += u64::from((b >> 8) & 0xff);
            }
        }
        assert!(n > 0, "glow {g} reached no pixel");
        moved.push(sum);
    }
    assert!(
        moved[1] > moved[0],
        "the pulse's height is brighter: {moved:?}"
    );
    let steady = b"ab\x1b[2 q\x1b[2;3H";
    assert_eq!(frame(steady, glowing(255)), frame(steady, Blink::Shown));
}

/// **A fade and a glow repaint incrementally to what a full frame paints**,
/// one row each: an opacity or a pulse that moves and nothing else still
/// dirties the cursor's row, or the CPU path would freeze mid-fade.
#[test]
fn a_fade_and_a_glow_repaint_incrementally() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let bytes = b"one\r\ntwo\r\nab";
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    for phase in [
        faded(200),
        faded(90),
        faded(0),
        glowing(30),
        glowing(250),
        glowing(0),
        faded(255),
    ] {
        r.set_cursor_blink(phase);
        let f = support::paint(&mut r, &mut t, &mut canvas, Damage::Incremental);
        assert_eq!(canvas.digest(), frame(bytes, phase), "{phase:?}");
        assert_eq!(f.rows_dirty.value(), 1, "{phase:?}");
    }
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

/// **A cursor a program placed and hid is not drawn.** The trail follows it
/// (`Term::placed_cursor`), and that is the only reader it has: helix draws its
/// own block there, and a second one drawn by the terminal would sit on it.
/// The frame is the one with the cursor hidden where no program placed it.
#[test]
fn a_placed_hidden_cursor_is_not_drawn() {
    let placed = b"ab\x1b[1;2H\x1b[?25l";
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, placed);
    assert!(
        t.placed_cursor().is_some(),
        "the control: the program placed it"
    );
    assert_eq!(
        frame(placed, Blink::Shown),
        frame(b"ab\x1b[?25l", Blink::Shown),
        "a placed cursor was drawn"
    );
    assert_ne!(
        frame(b"ab\x1b[1;2H", Blink::Shown),
        frame(placed, Blink::Shown),
        "the control: the same cursor shown is drawn"
    );
}
