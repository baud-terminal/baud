//! **Every trail in baud's amber, counted for WCAG 2.3.1's flashes**
//! (`[[source: wcag22]]`), by [`baud_bench::flash::Census`]: each of the
//! ten trails -- the seven effects riding the quad, the ghost, the sweep
//! and the quad -- across the window's diagonal on the CPU path, the
//! pixels the Linux driver shows, at 60 frames a second at 1x, on the dark
//! theme and the light. A run is one full pass and then the same pass
//! twice more at once, as repeats under the flash cap, which is what the
//! app draws for a band swept again inside a second (`baud-app`'s
//! `trail.rs`); a frame before the first and a sixth of a second after
//! the last are counted too.
//!
//! **Each passes on both readings**, the verdict's and the chained one,
//! and makes no red transition at all: the amber is not a saturated red,
//! its R/(R+G+B) 0.70 on the linear values on the dark theme and 0.72 on
//! the light, under the census's 0.8, and nothing it is drawn over makes
//! it one. The counts are printed. **The controls**: every run's frames
//! hold pixels the trail drew in the amber, so the census counted the
//! amber; and a plasma trail a program's `OSC 12` makes saturated red does
//! make red transitions, so the red reading can fire on a trail.

use baud_bench::flash::{Census, Outcome, Rate, Report, Scale, Transitions};
use baud_core::{CursorStyle, Owned, Term};
use baud_domain::{BytePos, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{
    Blend, Damage, Pt, PxSize, Renderer, Segment, Shape, Smear, Stack, Strength, Target, Theme,
    Trail, TrailLook,
};

const WS: WinSize = WinSize {
    ws_col: 64,
    ws_row: 20,
};

const RATE: Rate = Rate(60);

/// The seconds a ghost takes to land: the app's `GHOST_S`.
const GHOST_S: f32 = 0.1;

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Effect(Effect),
    Ghost,
    Smear(Shape),
}

const KINDS: [(&str, Kind); 10] = [
    ("fire", Kind::Effect(Effect::Fire)),
    ("plasma", Kind::Effect(Effect::Plasma)),
    ("lightning", Kind::Effect(Effect::Lightning)),
    ("ice", Kind::Effect(Effect::Ice)),
    ("ufo", Kind::Effect(Effect::Ufo)),
    ("orb", Kind::Effect(Effect::Orb)),
    ("liquid", Kind::Effect(Effect::Liquid)),
    ("ghost", Kind::Ghost),
    ("sweep", Kind::Smear(Shape::Sweep)),
    ("quad", Kind::Smear(Shape::Quad)),
];

/// Whether a pixel is amber: red over green over blue, each by a margin.
fn amber(p: u32) -> bool {
    let [_, r, g, b] = p.to_be_bytes();
    r > g.saturating_add(20) && g > b.saturating_add(20)
}

/// A trail of `kind` across the window under `theme` after `bytes`: its
/// count, and the pixels it drew in the amber over the frame without it.
fn count(theme: &Theme, kind: Kind, bytes: &[u8]) -> (Report, usize) {
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut text = Vec::new();
    for i in 0..WS.ws_row / 2 {
        text.extend_from_slice(
            format!("total {i:>4}  drwxr-xr-x  baud  staff  src\r\n").as_bytes(),
        );
    }
    text.extend_from_slice(b"\x1b[?25l");
    text.extend_from_slice(bytes);
    t.write_at(BytePos::ZERO, &text);
    t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let bare = px.clone();
    let mut census = Census::new(w, h, Scale(1), RATE);
    census.frame(&px);
    let (rows, cols) = (usize::from(WS.ws_row), usize::from(WS.ws_col));
    let (from, to) = (
        (Row::from_index(2), Col::at_index(2)),
        (Row::from_index(rows - 3), Col::at_index(cols - 10)),
    );
    let style = CursorStyle::SteadyBlock;
    let (a, b) = (
        r.cursor_rect(from.0, from.1, style),
        r.cursor_rect(to.0, to.1, style),
    );
    let seg = match kind {
        Kind::Smear(Shape::Sweep) => Segment::sweep(a.corners(), b),
        _ => Segment::new(a.corners(), b),
    };
    let unit = u16::try_from(r.metrics().cell_h.value()).map_or(f32::MAX, f32::from);
    let tail = Pt {
        x: (a.left + a.right) * 0.5,
        y: (a.top + a.bottom) * 0.5,
    };
    let ignition = Ignition::ignite(tail, b, Seed(7), unit);
    let end = match kind {
        Kind::Effect(e) => e.end(&ignition),
        Kind::Ghost => GHOST_S,
        Kind::Smear(_) => seg.lasts(),
    };
    let frame_s = 1.0 / RATE.0 as f32;
    let mut drawn = 0usize;
    for strength in [
        Strength::Full,
        Strength::UnderThreshold,
        Strength::UnderThreshold,
    ] {
        for f in 0u16.. {
            let age = f32::from(f) * frame_s;
            if age > end {
                break;
            }
            let emitter = |shape| Smear {
                corners: seg.at(age),
                hole: b,
                shape,
                strength,
                opacity: Smear::OPAQUE,
            };
            r.set_trail(Some(match kind {
                Kind::Effect(effect) => TrailLook::Vfx(Vfx {
                    effect,
                    emitter: emitter(Shape::Quad),
                    ignition: ignition.at(age),
                    axis: ignition.dir,
                }),
                Kind::Ghost => TrailLook::Ghost(Trail {
                    from,
                    to,
                    progress: age / GHOST_S,
                    origin_dy: 0.0,
                    lift: Px::of(0),
                    strength,
                }),
                Kind::Smear(shape) => TrailLook::Smear(emitter(shape)),
            }));
            let mut target = Target::new(&mut px, w, w, h).unwrap();
            r.render(&mut t, &mut target, Damage::Full);
            drawn += px
                .iter()
                .zip(&bare)
                .filter(|&(p, q)| p != q && amber(*p))
                .count();
            census.frame(&px);
        }
    }
    r.set_trail(None);
    for _ in 0..RATE.0 / 6 {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
        census.frame(&px);
    }
    (census.finish(), drawn)
}

#[test]
fn every_amber_trail_is_below_the_flash_thresholds_and_never_red() {
    for (half, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for (name, kind) in KINDS {
            let (c, drawn) = count(&theme, kind, b"");
            eprintln!(
                "{half} {name}: general {:?} flashes, {:?} chained, over {:.0} pt2; red {:?} \
                 transitions, {:?} flashes; area {:.0}, strict {:.0} of {:.0}; {drawn} amber \
                 pixel-frames",
                c.general.flashes.0,
                c.general.chained.0,
                c.general.over.0,
                c.red.transitions.0,
                c.red.flashes.0,
                c.area.0,
                c.strict.0,
                c.threshold.0,
            );
            assert!(drawn > 0, "{half} {name}: the control, no amber drawn");
            assert_eq!(c.translucent.value(), 0, "{half} {name}: frames not opaque");
            assert_eq!(c.outcome(), Outcome::Passes, "{half} {name}");
            assert_eq!(
                c.strict_outcome(),
                Outcome::Passes,
                "{half} {name}: chained"
            );
            assert_eq!(
                (c.red.transitions, c.red_encoded.transitions),
                (Transitions(0), Transitions(0)),
                "{half} {name}: a red transition"
            );
        }
    }
    // The red control: a program's saturated red cursor.
    let (c, _) = count(
        &Theme::dark(),
        Kind::Effect(Effect::Plasma),
        b"\x1b]12;#ff0000\x07",
    );
    assert!(
        c.red.transitions > Transitions(0),
        "the control: a red plasma makes no red transition"
    );
}
