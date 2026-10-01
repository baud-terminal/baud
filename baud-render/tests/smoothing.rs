//! macOS font smoothing on the CPU and instance paths (note J): every
//! outline glyph is drawn grown for the luminance of the colour it is
//! finally painted in, and nothing else changes.
//!
//! The dilation itself is `baud-font`'s, held to CoreText's masks by
//! `baud-font/tests/smooth.rs`. What is held here is the renderer's half:
//! which glyphs take a look, from which colour, and that turning it off
//! leaves the frame exactly what it was.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, SmoothLevel};
use baud_render::{
    Argb, Blend, Blink, BlockCursor, Damage, FaceStyle, GlyphLook, GlyphSource, Ink, Instance,
    Kind, Pictured, PxSize, Renderer, Resolved, Selected, Shapes, Smoothing, Stack, Swaps, Theme,
};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(16.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// One full frame of `bytes` under `smoothing`: its digest and the glyph
/// instances, with the coverage each one samples from the atlas.
fn frame(smoothing: Smoothing, bytes: &[u8]) -> (u64, Vec<(Instance, u64)>) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    r.set_smoothing(smoothing);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    let side = usize::try_from(r.atlas().side().value()).unwrap();
    let px = r.atlas().pixels();
    let glyphs = r
        .instances()
        .filter(|i| i.kind == Kind::Mask)
        .map(|i| {
            let (x0, y0) = (usize::from(i.u.0), usize::from(i.v.0));
            let sum = (y0..y0 + usize::from(i.h.0))
                .flat_map(|y| (x0..x0 + usize::from(i.w.0)).map(move |x| (y, x)))
                .map(|(y, x)| u64::from(px[y * side + x]))
                .sum();
            (*i, sum)
        })
        .collect();
    (canvas.digest(), glyphs)
}

/// White, then black on white, then a mid green: three colours at three
/// levels, the same letter in each, the cursor parked on row 2.
const THREE: &[u8] = b"\x1b[38;2;255;255;255mH\x1b[38;2;0;0;0;48;2;255;255;255mH\
\x1b[0m\x1b[38;2;0;205;0mH\x1b[2;24H";

/// **Off draws exactly what a renderer never told draws**, so every pinned
/// frame in the repository is unmoved by the change.
#[test]
fn off_is_the_frame_from_before() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let r = renderer(&faces);
    assert_eq!(
        r.smoothing(),
        Smoothing::Off,
        "off unless the embedder says"
    );
    let (plain, glyphs) = frame(Smoothing::Off, THREE);
    // Off, the three letters share one slot whatever their colour.
    let rect = |i: &Instance| (i.u.0, i.v.0, i.w.0, i.h.0);
    assert_eq!(glyphs.len(), 3);
    assert!(glyphs.iter().all(|(i, _)| rect(i) == rect(&glyphs[0].0)));
    // And on, then off again, is the off frame.
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    r.set_smoothing(Smoothing::CoreText);
    r.set_smoothing(Smoothing::Off);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, THREE);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    assert_eq!(canvas.digest(), plain);
}

/// **On, each letter is drawn for its own colour's level**: white at 4,
/// black at 0, green (0,205,0) at 2 -- three slots, the brighter the colour
/// the more ink, and black's mask is the plain one's coverage or more,
/// never less (level 0 still dilates by its 0.0058 ppem).
#[test]
fn each_colour_draws_its_own_level() {
    let (_, plain) = frame(Smoothing::Off, THREE);
    let (_, on) = frame(Smoothing::CoreText, THREE);
    assert_eq!(on.len(), 3);
    let rect = |i: &Instance| (i.u.0, i.v.0);
    assert_ne!(rect(&on[0].0), rect(&on[1].0), "white and black, two slots");
    assert_ne!(rect(&on[1].0), rect(&on[2].0), "black and green, two slots");
    assert_ne!(rect(&on[0].0), rect(&on[2].0), "white and green, two slots");
    let (white, black, green) = (on[0].1, on[1].1, on[2].1);
    assert!(
        white > green && green > black,
        "ink grows with the level: white {white}, green {green}, black {black}"
    );
    assert!(
        black > plain[1].1,
        "even level 0 grows: {black} > {}",
        plain[1].1
    );
    // The colours each glyph is painted in are the ones asked for.
    assert_eq!(on[0].0.color.0 & 0xFF_FFFF, 0xFF_FFFF);
    assert_eq!(on[1].0.color.0 & 0xFF_FFFF, 0x00_0000);
}

/// **The level is taken from the colour finally drawn**: reverse video
/// draws the letter in the background's colour, and the cursor's block
/// swaps it again -- so the same white-on-black letter, reversed, is drawn
/// at black's level.
#[test]
fn the_level_follows_the_drawn_colour() {
    let (_, normal) = frame(
        Smoothing::CoreText,
        b"\x1b[38;2;255;255;255;48;2;0;0;0mH\x1b[2;24H",
    );
    let (_, reversed) = frame(
        Smoothing::CoreText,
        b"\x1b[7;38;2;255;255;255;48;2;0;0;0mH\x1b[2;24H",
    );
    // Black on white: on the dark default background the contrast floor
    // would lift black to a grey and its level with it.
    let (_, black) = frame(
        Smoothing::CoreText,
        b"\x1b[38;2;0;0;0;48;2;255;255;255mH\x1b[2;24H",
    );
    eprintln!(
        "normal {:08x} {} reversed {:08x} {} black {:08x} {}",
        normal[0].0.color.0,
        normal[0].1,
        reversed[0].0.color.0,
        reversed[0].1,
        black[0].0.color.0,
        black[0].1
    );
    assert_eq!(reversed[0].0.color.0 & 0xFF_FFFF, 0);
    assert_eq!(
        reversed[0].1, black[0].1,
        "reversed white is drawn as black"
    );
    assert_ne!(reversed[0].1, normal[0].1);
}

/// **A colour glyph and a box character take one slot whatever the
/// colour**: an image is not an outline, and baud's own masks are drawn to
/// the cell, so neither has anything for a level to grow.
#[test]
fn images_and_boxes_are_drawn_once() {
    let jb = font("JetBrainsMono-Regular.ttf");
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&jb).unwrap(), Face::parse(&noto).unwrap()];
    let emoji = (0x80..=0x10FFFF)
        .find(|&cp| {
            Codepoint::new(cp)
                .is_some_and(|c| faces[1].glyph(c).is_some() && faces[0].glyph(c).is_none())
        })
        .and_then(Codepoint::new)
        .unwrap();
    let mut r = renderer(&faces);
    r.set_smoothing(Smoothing::CoreText);
    let image = r.cluster(&[emoji], FaceStyle::Regular);
    assert_eq!(
        image[0].slot.unwrap().kind,
        Kind::Color,
        "the emoji face's image"
    );
    let line = r.cluster(&[Codepoint::new(0x2500).unwrap()], FaceStyle::Regular);
    let letter = r.cluster(
        &[Codepoint::new(u32::from('H')).unwrap()],
        FaceStyle::Regular,
    );
    assert_eq!(line[0].source, GlyphSource::Procedural);
    for g in [&image[0], &line[0]] {
        let slots: Vec<_> = SmoothLevel::ALL
            .iter()
            .map(|&l| r.restyle(g, GlyphLook::Smooth(l)).map(|s| (s.x.0, s.y.0)))
            .collect();
        assert!(slots[0].is_some());
        assert!(slots.iter().all(|s| *s == slots[0]), "one slot: {slots:?}");
    }
    // The control: an outline takes a slot a level.
    let a = r
        .restyle(&letter[0], GlyphLook::Smooth(SmoothLevel::L0))
        .unwrap();
    let b = r
        .restyle(&letter[0], GlyphLook::Smooth(SmoothLevel::L4))
        .unwrap();
    assert_ne!((a.x.0, a.y.0), (b.x.0, b.y.0));
    let plain = r.restyle(&letter[0], GlyphLook::Plain).unwrap();
    assert_eq!((plain.x.0, plain.y.0), {
        let s = letter[0].slot.unwrap();
        (s.x.0, s.y.0)
    });
}

/// **Turning it on repaints an incremental frame**: no cell changed, so row
/// damage alone would keep every row's old mask. The frame after the switch
/// is the frame a renderer that was always on draws.
#[test]
fn a_switch_repaints_incrementally_what_a_full_frame_paints() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, THREE);
    let mut r = renderer(&faces);
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    let off = canvas.digest();
    r.set_smoothing(Smoothing::CoreText);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Incremental);
    let (on, _) = frame(Smoothing::CoreText, THREE);
    assert_ne!(off, on, "the control: smoothing changes the frame");
    assert_eq!(canvas.digest(), on);
}

/// **The side the cell path keys a glyph on is the drawn foreground's.**
/// The shader picks the slot's foreground look or its background's by the
/// swaps' parity (`Swaps::ink`); the CPU path takes the level of the colour
/// `Resolved::drawn` hands back. Over every swap, an `OSC 12` fill and a
/// picture, the two must name one look -- or the map is keyed on a look
/// the glyph is not drawn in, and the cell arm draws nothing there.
#[test]
fn the_ink_is_the_drawn_foregrounds_side() {
    let (fg, bg) = (Argb(0xFF_FF_FF_FF), Argb(0x38_10_10_10));
    let smoothing = Smoothing::CoreText;
    let slot = Resolved {
        fg,
        bg,
        underline: fg,
        shapes: Shapes::default().with_looks(smoothing.look(fg), smoothing.look(bg)),
    };
    assert_ne!(
        smoothing.look(fg),
        smoothing.look(bg),
        "the control: two looks"
    );
    let mut swapped = 0;
    for selected in [Selected::No, Selected::Yes] {
        for block in [
            BlockCursor::No,
            BlockCursor::Yes(None),
            BlockCursor::Yes(Some(Argb(0xFF_FF_00_00))),
        ] {
            for pictured in [Pictured::No, Pictured::Yes] {
                let s = Swaps {
                    pictured,
                    selected,
                    block,
                    blink: Blink::Shown,
                };
                let drawn = slot.drawn(s);
                assert_eq!(slot.shapes.look(s.ink()), smoothing.look(drawn.fg), "{s:?}");
                swapped += usize::from(s.ink() == Ink::Swapped);
            }
        }
    }
    // Selected alone, twice (with and without a picture), and a block
    // cursor alone in its two fills, twice.
    assert_eq!(swapped, 6, "the odd swaps: selected or block, not both");
}
