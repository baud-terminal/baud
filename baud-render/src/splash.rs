//! **The launch splash**: baud's colour lockup, `baud ❯ █`, in the middle
//! of the window once it is shown -- coming out of the plasma from its
//! left edge to its right, whole about a quarter of a second in, then
//! swept out from its left edge to its right at [`OUT`], in the plasma's
//! fading tail, and gone at [`GONE`], half a second in (note AJ). The
//! lockup is
//! `assets/logo/baud-lockup-dark.svg`'s and `-light.svg`'s geometry and
//! colours, without the dark one's baked ground, which would be a
//! rectangle on the glass; the half is the theme's by the rule `CSI ? 996
//! n` answers with ([`crate::Theme::polarity`]).
//!
//! **Its plasma is the lockup's amber**: the logo carries the colour on the
//! splash, and the cursor trails keep the cursor's colour. It goes where it
//! stands rather than flying its block to the shell's cursor, and at
//! kitty's pace, as every trail's, rather than a slower collapse. It is
//! drawn as the lockup's own pixels, not as cells.
//!
//! **It comes out of the plasma trail's own model**, [`crate::plasma`]:
//! the lockup's block is a cursor, and the box from the wordmark's left to
//! the block collapses into it as kitty's quad collapses into the cell a
//! cursor reached ([`Segment`]), the tube filling it, the filaments
//! braiding into the block and the ions peeling off. The logo stands
//! faintly in it from its first frame ([`GHOST`]) and is drawn whole where
//! the quad's trailing edge has passed, the block last. **Then an even
//! sweep takes it out**, left to right over [`SWEEP_S`], the block last,
//! while what is left of the plasma burns out, and that remainder fades to
//! nothing by [`GONE`]. **One plasma, at full strength**: no second pass
//! crosses the box the emerge lit, so nothing waits on a flash cap and no
//! frame stands still. No new shader: every path draws it by the effect it
//! already draws, and the pixel oracle holds it by the reference it already
//! has.
//!
//! **Everything is a pure function of the phase**: the driver hands
//! [`SplashLook`] -- so many seconds into the splash, the logo held, or so
//! far into the fade -- computed from the display link's timestamp, and
//! this derives the records and the effect. No clock and no state, as
//! [`crate::effect`].

use crate::color::Argb;
use crate::effect::{Effect, Ignition, Seed, Vfx, smoothstep};
use crate::flash::Strength;
use crate::trail::{Pt, Rect, Segment, Shape, Smear};
use baud_domain::Chan;

/// **Where the splash is**, as the driver hands it each frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SplashPhase {
    /// So many seconds into the splash under full motion, from its first
    /// frame: the logo coming out of the plasma, then swept out of it.
    Plasma(f32),
    /// The logo, whole: from the first frame under reduced motion.
    Hold,
    /// So far through the fade, 0 to 1: the form under reduced motion.
    Fade(f32),
}

/// The splash for a frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SplashLook {
    pub phase: SplashPhase,
}

/// The lockup's box, in the units of its SVG.
pub const BOX: (f32, f32) = (300.0, 130.0);

/// A line of the lockup's text: its pen's start, its baseline, its size
/// and the tracking between its characters, in the SVG's units.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Line {
    pub x: f32,
    pub baseline: f32,
    pub size: f32,
    pub tracking: f32,
}

/// `baud`, at `x="40" y="88" font-size="62" letter-spacing="-2.8"`.
pub const WORDMARK: Line = Line {
    x: 40.0,
    baseline: 88.0,
    size: 62.0,
    tracking: -2.8,
};

/// `❯`, at `x="192" y="85" font-size="54"`.
pub const PROMPT: Line = Line {
    x: 192.0,
    baseline: 85.0,
    size: 54.0,
    tracking: 0.0,
};

/// The block, `<rect x="238" y="45" width="22" height="46">`.
pub const BLOCK: Rect = Rect {
    left: 238.0,
    top: 45.0,
    right: 260.0,
    bottom: 91.0,
};

/// **The box the plasma fills at its start**: from the wordmark's pen to
/// the block's right edge, the block's height -- the lockup's cursor line.
/// The ascenders of `b` and `d` stand a little above it, inside the
/// sheath's reach of a block's height.
pub const EMITTER: Rect = Rect {
    left: WORDMARK.x,
    top: BLOCK.top,
    right: BLOCK.right,
    bottom: BLOCK.bottom,
};

/// **The lockup's width as a share of the window's**, and the most of the
/// window's height it may take: 0.30, rather than 0.20 or 0.45. No text
/// gives a size.
pub const FRACTION: (f32, f32) = (0.30, 0.40);

/// The ignition's seed: one look, every launch.
pub const SEED: Seed = Seed(0xBA0D_276F);

/// Seconds the fade lasts: the screen transition's (`TRANSITION_S`).
pub const FADE_S: f32 = 0.15;

/// **Seconds into the splash at which the sweep out starts**: the logo is
/// whole about 0.25 s in (the block, drawn last, at 0.99 of its opacity
/// at 0.27 s), so it stands whole for a frame or two and every frame
/// moves.
pub const OUT: f32 = 0.27;

/// **Seconds the sweep out takes**, from the wordmark's left edge to the
/// block's right: an even edge, so the block, last, goes as fast as the
/// letters rather than slowing as the plasma's own edge does.
pub const SWEEP_S: f32 = 0.15;

/// **Seconds into the splash at which it is gone**: the logo is swept out
/// by [`OUT`] plus [`SWEEP_S`], 0.42 s, and what is left of the plasma
/// fades to nothing from there to here, so its end is no step at any
/// frame rate. The plasma's own end is later ([`end`]), but what is left of
/// it at 0.50 s is within 0.081 of the bare page's relative luminance at
/// every pixel on the four grounds the census counts
/// (`baud-bench/tests/splash_flashes.rs`).
pub const GONE: f32 = 0.50;

/// The lockup's two colours on a half: the wordmark's and the amber of the
/// prompt and the block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Colours {
    pub wordmark: Argb,
    pub amber: Argb,
}

/// The dark lockup's, `#F3F4F6` and `#F4A32C`.
pub const DARK: Colours = Colours {
    wordmark: Argb(0xFFF3_F4F6),
    amber: Argb(0xFFF4_A32C),
};

/// The light lockup's, `#1E2128` and `#C8801F`: "Light mode uses the deeper
/// amber deliberately" (`assets/logo/README.md`).
pub const LIGHT: Colours = Colours {
    wordmark: Argb(0xFF1E_2128),
    amber: Argb(0xFFC8_801F),
};

/// **Device pixels a lockup unit** over a frame `w` by `h`: the lockup
/// [`FRACTION`] of the window wide, or less where the window is short.
pub fn scale(frame: (f32, f32)) -> f32 {
    (frame.0 * FRACTION.0 / BOX.0)
        .min(frame.1 * FRACTION.1 / BOX.1)
        .max(0.0)
}

/// Where the lockup's box starts, centred in the frame, at `scale`.
pub fn origin(frame: (f32, f32), scale: f32) -> Pt {
    Pt {
        x: ((frame.0 - BOX.0 * scale) * 0.5).round(),
        y: ((frame.1 - BOX.1 * scale) * 0.5).round(),
    }
}

/// A lockup rectangle in frame pixels.
pub fn placed(r: Rect, at: Pt, scale: f32) -> Rect {
    Rect {
        left: at.x + r.left * scale,
        top: at.y + r.top * scale,
        right: at.x + r.right * scale,
        bottom: at.y + r.bottom * scale,
    }
}

/// **The ignition the vanish is**: from the emitter's left, on its centre
/// line, to the block, its unit the block's height.
pub fn ignition(at: Pt, scale: f32) -> Ignition {
    let e = placed(EMITTER, at, scale);
    let block = placed(BLOCK, at, scale);
    Ignition::ignite(
        Pt {
            x: e.left,
            y: (e.top + e.bottom) * 0.5,
        },
        block,
        SEED,
        (block.bottom - block.top).max(1.0),
    )
}

/// **The plasma `age` seconds into the splash**: the emitter collapsing
/// from its box into the block on kitty's constants, the block its hole,
/// the axis the move's, at full strength -- the logo's trailing edge
/// crossing it from left to right.
pub fn vfx(at: Pt, scale: f32, age: f32) -> Vfx {
    let e = placed(EMITTER, at, scale);
    let block = placed(BLOCK, at, scale);
    let f = ignition(at, scale).at(age.max(0.0));
    Vfx {
        effect: Effect::Plasma,
        emitter: Smear {
            corners: Segment::new(e.corners(), block).at(age.max(0.0)),
            hole: block,
            shape: Shape::Quad,
            strength: Strength::Full,
        },
        ignition: f,
        axis: f.dir,
    }
}

/// **Seconds from the splash's start to when nothing of its plasma would
/// be left**: the plasma's end -- its life, or its last ion's death --
/// which [`GONE`] comes before. The ion count reads the path's length in
/// units, so this is the same at every size.
pub fn end() -> f32 {
    Effect::Plasma.end(&ignition(Pt { x: 0.0, y: 0.0 }, 1.0))
}

/// **The logo's opacity before the emerge's edge reaches it**: drawn
/// faintly inside the plasma from the first frame, so the logo is on
/// screen at once and the plasma draws it whole.
pub const GHOST: f32 = 0.25;

/// **How much of the plasma's own light is drawn `age` seconds in**: all
/// of it until the logo is swept out, then less in step with the time
/// left, none at [`GONE`] -- so the frame before the splash is gone already
/// differs from the bare page by less than the remainder does, whenever it
/// falls.
pub fn glow(age: f32) -> f32 {
    let from = OUT + SWEEP_S;
    ((GONE - age) / (GONE - from)).clamp(0.0, 1.0)
}

/// **Where the sweep out stands `age` seconds in**, in frame pixels: left
/// of the wordmark before [`OUT`], then moving evenly to the block's right
/// edge, a pixel past it, over [`SWEEP_S`].
fn sweep(at: Pt, scale: f32, age: f32) -> f32 {
    let e = placed(EMITTER, at, scale);
    let b = placed(BLOCK, at, scale);
    let k = ((age - OUT) / SWEEP_S).clamp(0.0, 1.0);
    match age < OUT {
        true => f32::NEG_INFINITY,
        false => e.left + (b.right + 1.0 - e.left) * k,
    }
}

/// The edge of the plasma's quad the ink follows: its trailing edge, the
/// corners' leftmost.
fn edge(v: &Vfx) -> f32 {
    v.emitter
        .corners
        .0
        .iter()
        .map(|p| p.x)
        .fold(f32::INFINITY, f32::min)
}

/// **A glyph's opacity**, `left` to `right` its box in frame pixels: whole
/// while held; drawn as the plasma's trailing edge crosses it, from
/// [`GHOST`], and gone as the sweep out crosses it; and the fade's.
pub fn ink(look: SplashLook, at: Pt, scale: f32, left: f32, right: f32) -> f32 {
    let swept = |x: f32| smoothstep(left, right.max(left + 1.0), x);
    match look.phase {
        SplashPhase::Hold => 1.0,
        SplashPhase::Fade(p) => 1.0 - p.clamp(0.0, 1.0),
        SplashPhase::Plasma(age) => {
            (GHOST + (1.0 - GHOST) * swept(edge(&vfx(at, scale, age))))
                * (1.0 - swept(sweep(at, scale, age)))
        }
    }
}

/// **The block's opacity**: whole while held; drawn last, as the plasma's
/// edge closes the block's own width short of it, and gone last, as the
/// sweep out crosses it; and the fade's.
pub fn block(look: SplashLook, at: Pt, scale: f32) -> f32 {
    let b = placed(BLOCK, at, scale);
    // Arrived within half a pixel, as a trail's corner is.
    let near = |x: f32| smoothstep(b.left - (b.right - b.left), b.left - 0.5, x);
    match look.phase {
        SplashPhase::Hold => 1.0,
        SplashPhase::Fade(p) => 1.0 - p.clamp(0.0, 1.0),
        SplashPhase::Plasma(age) => {
            (GHOST + (1.0 - GHOST) * near(edge(&vfx(at, scale, age))))
                * (1.0 - smoothstep(b.left, b.right, sweep(at, scale, age)))
        }
    }
}

/// `c` at `alpha` of its own.
pub fn at_alpha(c: Argb, alpha: f32) -> Argb {
    c.with_alpha(Chan::from_unit(
        alpha.clamp(0.0, 1.0) * f32::from(c.a().get()) / 255.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: (f32, f32) = (1600.0, 1000.0);

    /// **The lockup is the SVG's**: its box, the wordmark's and the
    /// prompt's lines, the block, and the two halves' colours, as
    /// `assets/logo/baud-lockup-dark.svg` and `-light.svg` spell them.
    #[test]
    fn the_lockup_is_the_svgs() {
        let dark = include_str!("../../assets/logo/baud-lockup-dark.svg");
        let light = include_str!("../../assets/logo/baud-lockup-light.svg");
        for (svg, c) in [(dark, DARK), (light, LIGHT)] {
            assert!(svg.contains(r#"viewBox="0 0 300 130""#));
            assert!(svg.contains(
                r#"<text x="40" y="88" font-size="62" font-family="JetBrains Mono, ui-monospace, SFMono-Regular, Menlo, monospace" font-weight="500" letter-spacing="-2.8""#
            ));
            assert!(svg.contains(r#"<text x="192" y="85" font-size="54""#));
            assert!(svg.contains(r#"<rect x="238" y="45" width="22" height="46""#));
            let hex = |a: Argb| format!("#{:06X}", a.0 & 0xFF_FFFF);
            assert!(svg.contains(&format!(r#"fill="{}">baud"#, hex(c.wordmark))));
            assert!(svg.contains(&format!(r#"fill="{}">❯"#, hex(c.amber))));
            assert!(svg.contains(&format!(
                r#"<rect x="238" y="45" width="22" height="46" fill="{}">"#,
                hex(c.amber)
            )));
        }
        assert_eq!(BOX, (300.0, 130.0));
        assert_eq!(
            (BLOCK.right - BLOCK.left, BLOCK.bottom - BLOCK.top),
            (22.0, 46.0)
        );
    }

    /// **It is centred, and its share of the window**: as wide as the
    /// fraction says on a wide window, held by the height on a short one.
    #[test]
    fn it_is_centred_and_its_share_of_the_window() {
        let s = scale(FRAME);
        assert!((BOX.0 * s - FRAME.0 * FRACTION.0).abs() < 1e-3);
        let o = origin(FRAME, s);
        assert!((o.x * 2.0 + BOX.0 * s - FRAME.0).abs() <= 1.0);
        assert!((o.y * 2.0 + BOX.1 * s - FRAME.1).abs() <= 1.0);
        let short = (1600.0, 200.0);
        let t = scale(short);
        assert!((BOX.1 * t - short.1 * FRACTION.1).abs() < 1e-3);
        assert!(BOX.0 * t < short.0 * FRACTION.0);
    }

    /// **The plasma collapses into the block**: at its start the emitter
    /// is the box from the wordmark to the block, the hole the block,
    /// drawn at full strength; it collapses into the block; and its own
    /// end, the same at every size, comes after the splash is gone.
    #[test]
    fn the_plasma_collapses_into_the_block() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let first = vfx(o, s, 0.0);
        let e = placed(EMITTER, o, s);
        let xs = first.emitter.corners.0.map(|p| p.x);
        assert!(xs.iter().any(|&x| (x - e.left).abs() < 1e-3));
        assert!(xs.iter().any(|&x| (x - e.right).abs() < 1e-3));
        assert_eq!(first.emitter.hole, placed(BLOCK, o, s));
        assert_eq!(first.effect, Effect::Plasma);
        assert_eq!(first.emitter.strength, Strength::Full);
        let late = vfx(o, s, 0.5).emitter.corners.0;
        let b = placed(BLOCK, o, s);
        assert!(
            late.iter()
                .all(|p| p.x >= b.left - 1.0 && p.x <= b.right + 1.0)
        );
        let out = end();
        assert!((crate::plasma::LIFE..=0.65).contains(&out), "{out}");
        assert!(GONE < out, "{GONE} {out}");
        assert_eq!(
            Effect::Plasma.end(&ignition(o, s)),
            out,
            "the same at another size"
        );
    }

    /// **The sweep out takes the logo, and the glow fades after it**: the
    /// logo whole just before [`OUT`]; the wordmark's first glyph gone
    /// before the prompt; nothing of the logo once the sweep has crossed
    /// the block; the plasma's light whole until then and none at
    /// [`GONE`]; the fade taking all of it together.
    #[test]
    fn the_sweep_takes_the_logo_and_the_glow_fades_after_it() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let b = (o.x + 40.0 * s, o.x + 75.0 * s);
        let p = (o.x + 192.0 * s, o.x + 224.0 * s);
        let held = SplashLook {
            phase: SplashPhase::Hold,
        };
        assert_eq!(ink(held, o, s, b.0, b.1), 1.0);
        assert_eq!(block(held, o, s), 1.0);
        let at = |t: f32| SplashLook {
            phase: SplashPhase::Plasma(t),
        };
        assert!(ink(at(OUT), o, s, b.0, b.1) > 1.0 - 1e-3);
        assert!(ink(at(OUT), o, s, p.0, p.1) > 1.0 - 1e-3);
        assert!(block(at(OUT), o, s) > 0.98, "{}", block(at(OUT), o, s));
        let t = OUT + 0.03;
        assert!(ink(at(t), o, s, b.0, b.1) < ink(at(t), o, s, p.0, p.1));
        let swept = OUT + SWEEP_S;
        assert_eq!(ink(at(swept), o, s, b.0, b.1), 0.0);
        assert_eq!(ink(at(swept), o, s, p.0, p.1), 0.0);
        assert_eq!(block(at(swept), o, s), 0.0);
        assert_eq!(glow(swept), 1.0);
        assert!(glow((swept + GONE) / 2.0) > 0.4 && glow((swept + GONE) / 2.0) < 0.6);
        assert_eq!(glow(GONE), 0.0);
        let half = SplashLook {
            phase: SplashPhase::Fade(0.5),
        };
        assert_eq!(ink(half, o, s, b.0, b.1), 0.5);
        assert_eq!(block(half, o, s), 0.5);
    }

    /// The lockup's five glyphs' boxes, left to right, in frame pixels:
    /// the wordmark's four at its advance less its tracking, and the
    /// prompt.
    fn glyphs(o: Pt, s: f32) -> [(f32, f32); 5] {
        let step = 62.0 * 0.6 + WORDMARK.tracking;
        let w = |i: f32| {
            let l = WORDMARK.x + i * step;
            (o.x + l * s, o.x + (l + step) * s)
        };
        [
            w(0.0),
            w(1.0),
            w(2.0),
            w(3.0),
            (o.x + 192.0 * s, o.x + 224.0 * s),
        ]
    }

    /// **Both sweeps run left to right, the block last**: at every age
    /// before the sweep out each glyph is at least as drawn as the one to
    /// its right and the block the least; at every age after it starts,
    /// each is at least as gone, the block the last to go. The first frame
    /// shows the logo partly drawn, and [`OUT`] shows it whole.
    #[test]
    fn both_sweeps_run_left_to_right_the_block_last() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let g = glyphs(o, s);
        let ms = (0u16..=700).map(|m| f32::from(m) / 1000.0);
        for age in ms {
            let look = SplashLook {
                phase: SplashPhase::Plasma(age),
            };
            let mut inks: Vec<f32> = g.iter().map(|&(l, r)| ink(look, o, s, l, r)).collect();
            inks.push(block(look, o, s));
            for w in inks.windows(2) {
                // Gone no sooner than the one to its left; the block, at
                // 0.99 of its opacity when the sweep starts, counts as
                // whole until the sweep reaches it.
                let ordered = match age < OUT {
                    true => w[0] + 1e-4 >= w[1],
                    false => w[0] <= w[1] + 1e-4 || w[1] >= 0.98,
                };
                assert!(ordered, "at {age}: {inks:?}");
            }
        }
        let first = SplashLook {
            phase: SplashPhase::Plasma(0.0),
        };
        let b = ink(first, o, s, g[0].0, g[0].1);
        assert!(b > 0.0 && b < 1.0, "the first frame, partly: {b}");
        let whole = SplashLook {
            phase: SplashPhase::Plasma(OUT),
        };
        for &(l, r) in &g {
            assert!(ink(whole, o, s, l, r) > 1.0 - 1e-3);
        }
        assert!(block(whole, o, s) > 0.98, "whole at the sweep's start");
        let midway = SplashLook {
            phase: SplashPhase::Plasma(0.05),
        };
        assert!(
            ink(midway, o, s, g[0].0, g[0].1) > ink(midway, o, s, g[4].0, g[4].1) + 0.2,
            "the left drawn before the right"
        );
    }
}
