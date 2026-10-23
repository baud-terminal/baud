//! A Sixel's background fill on the see-through window, on the CPU path:
//! `[glass] sixel_backgrounds`, the owner's call of 2026-10-22. The pixel
//! oracle holds the three paths to each other on a full frame; these hold
//! the CPU path's incremental frames to a full repaint, where a fill
//! blended over what is under it would be blended twice if its rows were
//! not repainted under each blit.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ImageBudget, ReadLen, Rows};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, SixelBg, Stack, Target, Theme};
use support::{WS, font};

/// A 40x40 Sixel with `P2` 0: its left half red stripes, the rest its fill.
const FILLED: &[u8] = b"\x1bP9;0;0q\"1;1;40;40#1;2;100;0;0#1!20i-!20i-!20i-!20A\x1b\\";

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn glass(mode: SixelBg) -> Theme {
    Theme {
        bg_alpha: Chan::from_u8(56),
        sixel_bg: mode,
        ..Theme::dark()
    }
}

/// A full frame of `t` by a fresh renderer under `theme`.
fn full(faces: &[Face<'_>], theme: &Theme, t: &mut Term<'_>) -> Vec<u32> {
    let mut r = renderer(faces, theme);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let mut target = Target::new(&mut px, w, w, h).unwrap();
    r.render(t, &mut target, Damage::Full);
    px
}

fn renderer<'f>(faces: &'f [Face<'f>], theme: &Theme) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

/// **Every incremental frame is the full repaint**, for each way the fill
/// meets the glass: drawn once over text, then again with nothing changed,
/// then with text written beside the image on a row it covers.
#[test]
fn each_fill_on_the_glass_draws_incrementally_as_a_full_repaint() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    for mode in [SixelBg::Opaque, SixelBg::Dimmed, SixelBg::Cleared] {
        let theme = glass(mode);
        let mut r = renderer(&faces, &theme);
        let m = r.metrics();
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 64));
        let mut t = Term::new(owned.storage(), &WS);
        let px16 = |v: u32| baud_domain::ImagePx::of(u16::try_from(v).unwrap());
        t.set_cell_size(px16(m.cell_w.value()), px16(m.cell_h.value()));
        feed(
            &mut t,
            b"\x1b[?25lEEEEEEEEEEEE\r\nEEEEEEEEEEEE\r\nEEEEEEEEEEEE\x1b[1;2H",
        );
        feed(&mut t, FILLED);
        let (w, h) = r.pixel_size();
        let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
        let mut frame = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, d: Damage| {
            {
                let mut target = Target::new(&mut px, w, w, h).unwrap();
                r.render(t, &mut target, d);
            }
            px.clone()
        };
        let first = frame(&mut r, &mut t, Damage::Full);
        assert_eq!(r.image_placements().len(), 1, "{mode:?}: the image placed");
        let again = frame(&mut r, &mut t, Damage::Incremental);
        assert_eq!(again, first, "{mode:?}: a frame with nothing changed");
        feed(&mut t, b"\x1b[2;12Hx");
        let beside = frame(&mut r, &mut t, Damage::Incremental);
        assert_ne!(beside, first, "{mode:?}: the control, the text drew");
        assert_eq!(
            beside,
            full(&faces, &theme, &mut t),
            "{mode:?}: as a full repaint"
        );
    }
}
