//! **The default blink's flashes, counted on the frames it draws** (WCAG
//! 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]: a
//! blinking block switched hard, 250 ms shown and 250 hidden -- the app's
//! default, `caret.rs`'s `EVEN` and `blink_animation = "none"`, which its
//! own tests pin -- drawn at 60 frames a second on the CPU path, the pixels
//! the Linux driver shows and the macOS pixel oracle holds the GPU arms to.
//! At 13, 32, 46 and 95 pt, on 1x and 2x displays, on the dark and the
//! light theme.
//!
//! **It is under the general and red flash thresholds by the count alone**:
//! content passes if "there are no more than three general flashes and / or
//! no more than three red flashes within any one-second period". A hard
//! blink at two cycles a second is four opposing changes a second at the
//! cursor's pixels: two flashes paired disjointly, three on the chained
//! reading (the census's strict verdict), both "no more than three", so no
//! pixel counts toward the area clause at any size, and the area is zero
//! whatever the cell covers. The counts are asserted exactly, so a blink
//! the census did not see would fail too.
//!
//! The control: the same cursor switched every 125 ms, four cycles a
//! second, counts four flashes at its pixels, more than three, and the
//! area it covers is the area that fails the count.

use baud_bench::flash::{Census, Flashes, MOST, Rate, Report, Scale};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, CursorPhase, Damage, PxSize, Renderer, Stack, Target, Theme};

const RATE: u32 = 60;

/// The default phase, each of shown and hidden: `caret::EVEN`.
const PHASE_S: f64 = 0.250;

const WS: WinSize = WinSize {
    ws_col: 5,
    ws_row: 3,
};

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// **`seconds` of a blinking block, `phase` seconds shown and `phase`
/// hidden, switched hard**, at `pt` points on a `scale` display in
/// `theme`, counted.
fn count(theme: &Theme, pt: f32, scale: u32, phase: f64, seconds: f64) -> Report {
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(pt * scale as f32).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // A blinking block in the middle cell, beside a letter on each side.
    let b = b"\x1b[1 q\x1b[2;2Hab\x1b[2;3H";
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    let (w, h) = r.pixel_size();
    let mut census = Census::new(w, h, Scale(scale), Rate(RATE));
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let frames = (seconds * f64::from(RATE)).round() as i64;
    for k in 0..frames {
        let now = k as f64 / f64::from(RATE);
        let hidden = ((now / phase + 1e-9).floor() as i64) % 2 == 1;
        r.set_cursor_blink(match hidden {
            true => CursorPhase::HIDDEN,
            false => CursorPhase::SHOWN,
        });
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
        census.frame(&px);
    }
    census.finish()
}

#[test]
fn the_default_blink_is_below_the_flash_thresholds() {
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for pt in [13.0f32, 32.0, 46.0, 95.0] {
            for scale in [1u32, 2] {
                let r = count(&theme, pt, scale, PHASE_S, 3.0);
                eprintln!(
                    "{name} {pt} pt {scale}x: general {} (chained {}), red {}, area {}, \
                     strict {}, ever {:.0} pt^2",
                    r.general.flashes.0,
                    r.general.chained.0,
                    r.red.flashes.0,
                    r.area.0,
                    r.strict.0,
                    r.general.ever.0,
                );
                let at = format!("{name} {pt} pt {scale}x: {r:?}");
                assert_eq!(r.translucent.value(), 0, "an opaque ground: {at}");
                assert_eq!(r.general.flashes, Flashes(2), "{at}");
                assert_eq!(r.general.chained, Flashes(3), "{at}");
                assert!(r.general.chained <= MOST, "{at}");
                assert_eq!(r.red.flashes, Flashes(0), "{at}");
                assert_eq!(r.area.0, 0.0, "{at}");
                assert_eq!(r.strict.0, 0.0, "{at}");
                assert!(r.general.ever.0 > 0.0, "the census sees the cursor: {at}");
            }
        }
    }
    // The control: four cycles a second is four flashes at the cursor's
    // pixels, and they are the area over the count.
    let r = count(&Theme::dark(), 32.0, 1, PHASE_S / 2.0, 3.0);
    assert_eq!(r.general.flashes, Flashes(4), "the control: {r:?}");
    assert!(r.area.0 > 0.0, "the control: {r:?}");
}
