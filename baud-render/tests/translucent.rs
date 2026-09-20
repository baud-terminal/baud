//! **Every background carries the window's dim, not only the default
//! one**, and a cursor or selection hands back an *opaque* foreground.
//!
//! Nothing else in the workspace renders at a translucent `bg_alpha`:
//! every other gate uses `Theme::dark()`, whose alpha is `0xFF`, where
//! `with_alpha(0xFF)` is a no-op and this whole path is invisible. So
//! these are the only tests that can fail when the rule changes.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Argb, Blend, Damage, Kind, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 12,
};

/// `[glass] dim_dark`'s default, 0.22 of 255 rounded.
const DIM: u8 = 56;

fn alpha(c: Argb) -> u8 {
    c.0.to_be_bytes()[0]
}

fn dim_theme() -> Theme {
    let mut t = Theme::dark();
    t.bg_alpha = Chan::from_u8(DIM);
    t
}

fn render(bytes: &[u8], f: impl FnOnce(&Renderer<'_, '_>)) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        dim_theme(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);
    f(&r);
}

/// A default background, a background the application set, and a reverse
/// rendition -- all three at the window's alpha. Under the rule this
/// replaced only the first was translucent, which is why btop and
/// asciiquarium were opaque where a shell was not.
#[test]
fn every_background_carries_the_windows_dim() {
    // `A` default, `B` on red, `C` reversed, then the cursor parked off
    // the three so it cannot opaque one of them.
    render(b"A\x1b[41mB\x1b[0;7mC\x1b[0m\x1b[2;12H", |r| {
        let bg = r.background();
        for (i, name) in ["default", "SGR 41", "SGR 7 reverse"].iter().enumerate() {
            assert_eq!(
                alpha(bg[i]),
                DIM,
                "cell {i} ({name}) is {:#04x}, not the window's dim",
                alpha(bg[i])
            );
        }
        // And they are three *different* colours, so the assertion above
        // is about three renditions rather than one repeated.
        assert_ne!(bg[0].0 & 0x00FF_FFFF, bg[1].0 & 0x00FF_FFFF);
        assert_ne!(bg[1].0 & 0x00FF_FFFF, bg[2].0 & 0x00FF_FFFF);
    });
}

/// The block cursor is opaque on **both** sides of its swap. `bg` becomes
/// the old foreground and is the block a reader sees; `fg` becomes the old
/// background, which now carries the dim -- and a glyph drawn at 0.22 over
/// its own cursor block is barely there.
#[test]
fn a_cursor_is_opaque_on_both_sides_of_its_swap() {
    // One glyph, with the cursor homed back onto it, so every mask
    // instance in the frame is the cursor's own glyph.
    render(b"A\x1b[H", |r| {
        assert_eq!(alpha(r.background()[0]), 0xFF, "the cursor block is dimmed");
        let masks: Vec<_> = r
            .instances()
            .filter(|i| matches!(i.kind, Kind::Mask))
            .collect();
        assert!(!masks.is_empty(), "no glyph drawn: the test proves nothing");
        for m in masks {
            assert_eq!(
                alpha(m.color),
                0xFF,
                "a glyph under the cursor is drawn at the background's alpha"
            );
        }
    });
}

/// The margin keeps the same alpha, so the gutter and the cells are one
/// surface rather than a see-through edge around an opaque grid.
#[test]
fn the_margin_is_the_same_alpha_as_a_cell() {
    render(b"A", |r| {
        assert_eq!(alpha(r.clear_color()), DIM);
    });
}
