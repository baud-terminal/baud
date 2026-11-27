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
        ..CursorPhase::HIDDEN
    }
}

fn glowing(glow: u8) -> CursorPhase {
    CursorPhase {
        glow: Chan::from_u8(glow),
        ..CursorPhase::HIDDEN
    }
}

fn burning(at: u8) -> CursorPhase {
    CursorPhase::fire(Chan::from_u8(at))
}

fn beamed(at: u8) -> CursorPhase {
    CursorPhase::ufo(Chan::from_u8(at))
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
        burning(40),
        burning(130),
        burning(200),
        beamed(40),
        beamed(128),
        beamed(200),
        faded(255),
    ] {
        r.set_cursor_blink(phase);
        let f = support::paint(&mut r, &mut t, &mut canvas, Damage::Incremental);
        assert_eq!(canvas.digest(), frame(bytes, phase), "{phase:?}");
        assert_eq!(f.rows_dirty.value(), 1, "{phase:?}");
    }
}

/// **The fire and the ufo land on the cursor's cell and nowhere else**, in
/// each shape: part way each is neither the shown frame nor the hidden one,
/// it differs from the shown one only inside the cell, and a steady cursor
/// has none.
#[test]
fn the_fire_and_the_ufo_land_on_the_cursor_cell_alone() {
    for (shape, effect) in [b"1", b"3", b"5"]
        .into_iter()
        .flat_map(|s| [(s, burning as fn(u8) -> CursorPhase), (s, beamed)])
    {
        let mut bytes = b"ab\x1b[".to_vec();
        bytes.extend_from_slice(shape);
        bytes.extend_from_slice(b" q\x1b[2;3H");
        let (on, cell) = pixels(&bytes, CursorPhase::SHOWN);
        let off = frame(&bytes, Blink::Hidden);
        for at in [30u8, 90, 140, 200, 240] {
            let (lit, _) = pixels(&bytes, effect(at));
            assert_ne!(
                lit.digest(),
                on.digest(),
                "Ps {} at {at}",
                char::from(shape[0])
            );
            assert_ne!(lit.digest(), off, "Ps {} at {at}", char::from(shape[0]));
            for (i, (a, b)) in on.pixels.iter().zip(&lit.pixels).enumerate() {
                if a != b {
                    let (x, y) = (i % on.width, i / on.width);
                    assert!(
                        (2 * cell.0..3 * cell.0).contains(&x) && (cell.1..2 * cell.1).contains(&y),
                        "{:?} at ({x}, {y}), outside the cursor's cell",
                        effect(at)
                    );
                }
            }
        }
        let mut steady = b"ab\x1b[".to_vec();
        steady.push(shape[0] + 1);
        steady.extend_from_slice(b" q\x1b[2;3H");
        assert_eq!(frame(&steady, effect(140)), frame(&steady, Blink::Shown));
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

/// **Every blink animation draws the same pixels on every OS**: the
/// plasma's glow at four pulses, a fade at two points, and the fire and the
/// ufo part way through, in the block, the underline and the bar (the glow
/// in the block alone: it is drawn over the cell, whatever the shape), on
/// an opaque page and on the glass at alpha 56, over a blank screen -- no
/// glyph, so what is pinned is the blink's -- is the frame pinned here,
/// taken on macOS and required of every other target: the Linux driver
/// draws with this path. The glow's exponential and the fire's sines are
/// `baud_render::det`'s, whose bits are the same on every machine
/// (`the_trails_call_no_platform_math` bars std's from `caret.rs`), and
/// the bits they are drawn from are pinned in `caret.rs` as well
/// (`every_blink_is_drawn_from_the_same_bits_on_every_os`): with std's
/// math in their place these frames read the same on macOS and on glibc
/// 2.36 on aarch64, and only the bits part. A change to a blink's look
/// changes its line, which the failure prints whole. Every line moved
/// when the cursor became baud's amber, the colour each blink is drawn in.
#[test]
fn every_blink_draws_the_same_pixels_on_every_os() {
    // Per page, opaque then the glass: the glow, then per shape the fade,
    // the fire and the ufo.
    const PINNED: &[u64] = &[
        0x5e9e547fa32b01f5,
        0x2be6a5bc45daefa5,
        0x5f83bc7db3232465,
        0xe1139bec5aaf87c5,
        0xa807e4ad8d237c25,
        0xc47d901c7d47c6a5,
        0x004d3f37f1bc8e65,
        0xd8ee2d98c3809e21,
        0xab2b397452a8f33e,
        0x0963d110ccf30785,
        0x0e1ed6ccabb49aa5,
        0x49c0f5abf1dbdb09,
        0xc794b02dbb847195,
        0x794a19b761f5dd4e,
        0x12c68b778c67f3dd,
        0xa83e767ade19c425,
        0x354ff490fd35faa5,
        0xb98c013720eb5c65,
        0xd67dd6616f1dcefe,
        0x4e95e809920ac38c,
        0xfaf60e75ce60bf85,
        0xb4b23435ddae8ea5,
        0xabacd5599b067699,
        0x9e938919b5c0a6b5,
        0x97e6f13d3baf77ee,
        0x8a1cd5e98c52eff1,
        0x205475130fa31a65,
        0x0076d5d87c264d05,
        0xe4442c2ddf655fd5,
        0x20da4cf1166e6a94,
        0xb7b7e10daa425876,
        0x0e38e781e92cc7ed,
        0xe31901afe79b5b05,
        0x8b5a118098551e28,
        0xdae6b10c2eab9dea,
        0xb21771c0dca14cc9,
        0xa70f17fe89c17ffb,
        0x1b787c0e1493fdb5,
        0xb4681dbf0472c565,
        0xcee75b247081e865,
        0xbee99efaf0f3a185,
        0xee9658f07e73aba5,
        0xb1e93824c4e289a5,
        0xba57b168c404fe65,
        0xa34b6090238868b5,
        0xea4f4ef415cf4b25,
        0x216f7d02971e4745,
        0x09da0a3f6c58a385,
        0x23b4f6170ade3841,
        0x364b3c5c8de7f27d,
        0x092556422d79aff3,
        0x933c468a5d5577ed,
        0x37930d5d938d8de5,
        0x83e66fac226744a5,
        0x3aff94adcbb68165,
        0x1c31fdbe157028c1,
        0xfde7339324ceabd8,
        0xba8c0fd4de8e9885,
        0x99606c81293c8585,
        0xab2339c37a140dcd,
        0xee4ae1d6a8a2d4a5,
        0xd129e909c7d31aeb,
        0xf59840bcf2715fe5,
        0x411aa413f40a5fd5,
        0xacec1c41c9965485,
        0x221e6c73e5c99bd5,
        0x4d990d47a6aec35a,
        0x945f028f2af826a5,
        0x72a8fe84fb9d919d,
        0xca1a11d5516a253d,
        0x5e1812b49f3d7c53,
        0xf0623fa68fe860a0,
        0x04046df2020a12b6,
        0x73117a590ce3a66a,
    ];
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut got = Vec::new();
    for alpha in [0xFF, 56] {
        let mut r = renderer(&faces);
        r.set_theme(Theme {
            bg_alpha: Chan::from_u8(alpha),
            ..Theme::dark()
        });
        let mut canvas = support::Canvas::for_renderer(&r);
        let mut draw = |shape: &[u8], phases: &[CursorPhase]| {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut bytes = b"\x1b[".to_vec();
            bytes.extend_from_slice(shape);
            bytes.extend_from_slice(b" q\x1b[2;3H");
            feed(&mut t, &bytes);
            for &p in phases {
                r.set_cursor_blink(p);
                support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
                got.push(canvas.digest());
            }
        };
        draw(b"1", &[40, 128, 200, 255].map(glowing));
        for shape in [b"1", b"3", b"5"] {
            draw(shape, &[128, 40].map(faded));
            draw(shape, &[10, 70, 135, 205, 240].map(burning));
            draw(shape, &[10, 70, 128, 190].map(beamed));
        }
    }
    // The positive control: no two of a page's frames are one.
    for page in got.chunks(got.len() / 2) {
        let mut seen = page.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            page.len(),
            "a blink drew another's frame: {page:x?}"
        );
    }
    let table: String = got
        .iter()
        .map(|d| format!("        0x{d:016x},\n"))
        .collect();
    assert!(
        got == PINNED,
        "the blinks' frames moved; the table this target draws:\n{table}"
    );
}
