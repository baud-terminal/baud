//! **The launch splash**: baud's colour lockup, `baud ❯ █`, in the middle
//! of the window once it is shown -- carried in by the plasma from its
//! left edge to its right, whole at [`OUT`], 0.3 s in, then carried out by
//! a second plasma from its left edge to its right, and gone at [`GONE`],
//! 0.6 s in (note AJ). The lockup is
//! `assets/logo/baud-lockup-dark.svg`'s and `-light.svg`'s geometry and
//! colours, without the dark one's baked ground, which would be a
//! rectangle on the glass; the half is the theme's by the rule `CSI ? 996
//! n` answers with ([`crate::Theme::polarity`]).
//!
//! **Its plasma is the lockup's amber**: the logo carries the colour on the
//! splash, and the cursor trails keep the cursor's colour. It goes where it
//! stands rather than flying its block to the shell's cursor. It is drawn
//! as the lockup's own pixels, not as cells.
//!
//! **It comes out of the plasma trail's own model**, [`crate::plasma`]:
//! the lockup's block is a cursor, and the box from the wordmark's left to
//! the block collapses into it as kitty's quad collapses into the cell a
//! cursor reached ([`Segment`]), the tube filling it, the filaments
//! braiding into the block and the ions peeling off. **Two passes, at full
//! strength**: the first draws the logo, which stands faintly in it from
//! its first frame ([`GHOST`]), whole where the quad's trailing edge has
//! passed; the second lights over the whole logo on the frame it is whole
//! and takes it away where its own trailing edge has passed, the block
//! last each time, and what is left of it fades to nothing by [`GONE`].
//! Each runs at [`PACE`] of kitty's clock, and each trailing edge crosses
//! the lockup at one speed ([`IN_S`], [`SWEEP_S`]) rather than on kitty's
//! curve, so the plasma covers every glyph for as long as the next. The
//! two passes put a fourth flash into one second at a few pixels, an area
//! the general and red flash thresholds allow by a wide margin
//! (`baud-bench/tests/splash_flashes.rs`); no pass is capped, so nothing
//! waits on a flash cap and no frame stands still. No new shader: every
//! path draws it by the effect it already draws, and the pixel oracle
//! holds it by the reference it already has.
//!
//! **Everything is a pure function of the phase**: the driver hands
//! [`SplashLook`] -- so many seconds into the splash, the logo held, or so
//! far into the fade -- computed from the display link's timestamp, and
//! this derives the records and the effect. No clock and no state, as
//! [`crate::effect`].

use crate::color::Argb;
use crate::det::Det;
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

/// **The plasma's clock against kitty's**: each pass's tube, sheath,
/// filaments, knots and ions age at this share of the trail's own pace, so
/// it burns brighter for longer and braids more slowly.
pub const PACE: f32 = 0.7;

/// **Seconds the first pass takes to draw the logo**: its quad's trailing
/// edge crosses the lockup at one speed, from the wordmark's left to the
/// block's, rather than on kitty's curve, which halves its gap every 0.04 s
/// -- that drew the wordmark in about a tenth of a second and left the
/// plasma on the prompt and the block alone.
pub const IN_S: f32 = 0.30;

/// **Seconds into the splash at which the plasma that carries it out
/// lights**: the first pass's edge has reached the block, the logo is
/// whole, and the second pass lights over it on that frame, so nothing
/// stands still.
pub const OUT: f32 = IN_S;

/// **Seconds the second pass takes to carry the logo out**: its trailing
/// edge crosses the lockup at one speed, as the first pass's did, rather
/// than taking the first glyph in a frame and leaving the block standing
/// alone in a dim plasma.
pub const SWEEP_S: f32 = 0.30;

/// **Seconds into the splash at which the logo is gone**: the second
/// pass's trailing edge has crossed the lockup, the block last.
pub const LEFT: f32 = OUT + SWEEP_S;

/// **Seconds into the splash at which it is gone**: the second pass fades
/// to nothing over [`DIM`] up to here, so its end is no step at any frame
/// rate. The plasma's own end is later ([`end`]), but what is left of it
/// two 60 Hz frames before here is within 0.10 of the bare page's
/// relative luminance at every pixel on the four grounds the census counts
/// (`baud-bench/tests/splash_flashes.rs`).
pub const GONE: f32 = LEFT + 0.08;

/// **Seconds the second pass takes to fade out**, ending at [`GONE`]: it
/// starts as the block goes, the last of the logo, since at 0.7 of kitty's
/// pace the plasma is still bright when the logo is out -- a fade over
/// 0.08 s from [`LEFT`] left 0.12 of relative luminance on the light page
/// two frames before the end.
pub const DIM: f32 = 0.12;

/// The second pass's seed: its filaments and ions are its own, not the
/// first pass's played again.
pub const SEED_OUT: Seed = Seed(0x5EED_0A7F);

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

/// **The ignition a pass is**: from the emitter's left, on its centre
/// line, to the block, its unit the block's height.
pub fn ignition(at: Pt, scale: f32) -> Ignition {
    lit(at, scale, SEED)
}

fn lit(at: Pt, scale: f32, seed: Seed) -> Ignition {
    let e = placed(EMITTER, at, scale);
    let block = placed(BLOCK, at, scale);
    Ignition::ignite(
        Pt {
            x: e.left,
            y: (e.top + e.bottom) * 0.5,
        },
        block,
        seed,
        (block.bottom - block.top).max(1.0),
    )
}

/// One pass `t` seconds of its own clock in: the emitter collapsing from
/// its box into the block on kitty's constants, the block its hole, the
/// axis the move's, at full strength.
fn pass(at: Pt, scale: f32, seed: Seed, t: f32, quad: f32) -> Vfx {
    let e = placed(EMITTER, at, scale);
    let block = placed(BLOCK, at, scale);
    let t = t.max(0.0);
    let f = lit(at, scale, seed).at(t);
    Vfx {
        effect: Effect::Plasma,
        emitter: Smear {
            corners: Segment::new(e.corners(), block).at(quad.max(0.0)),
            hole: block,
            shape: Shape::Quad,
            strength: Strength::Full,
        },
        ignition: f,
        axis: f.dir,
    }
}

/// **The plasma `age` seconds into the splash**: two passes at [`PACE`],
/// each the trailing edge of its quad crossing the logo evenly from left
/// to right -- the first drawing it, from the first frame, over [`IN_S`];
/// the second, lit over it at [`OUT`], taking it away over [`SWEEP_S`].
pub fn vfx(at: Pt, scale: f32, age: f32) -> Vfx {
    match age < OUT {
        true => pass(at, scale, SEED, age * PACE, even(age, 0.0, IN_S)),
        false => pass(
            at,
            scale,
            SEED_OUT,
            (age - OUT) * PACE,
            even(age, OUT, SWEEP_S),
        ),
    }
}

/// **A pass's quad time `age` seconds into the splash**, the pass starting
/// at `from` and crossing in `span`: the time on kitty's curve at which its
/// trailing edge -- both left corners, which alone travel and so chase the
/// block's on the slow constant -- has closed the share of its gap an even
/// crossing has, `2^(-10 t / TAU_SLOW)` being the gap left. So the edge
/// moves at one speed, and is within `2^-12` of the block at its end.
fn even(age: f32, from: f32, span: f32) -> f32 {
    let left = (1.0 - (age - from) / span).max(1.0 / 4096.0);
    -crate::trail::TAU_SLOW / 10.0 * left.det_log2()
}

/// **Seconds from the splash's start to when nothing of its plasma would
/// be left**: the second pass's end -- its life, or its last ion's death,
/// on its own clock -- which [`GONE`] comes before. The ion count reads
/// the path's length in units, so this is the same at every size.
pub fn end() -> f32 {
    OUT + Effect::Plasma.end(&lit(Pt { x: 0.0, y: 0.0 }, 1.0, SEED_OUT)) / PACE
}

/// **The logo's opacity before the emerge's edge reaches it**: drawn
/// faintly inside the plasma from the first frame, so the logo is on
/// screen at once and the plasma draws it whole.
pub const GHOST: f32 = 0.25;

/// **How much of the second pass's light is drawn `age` seconds in**: all
/// of it until [`DIM`] before [`GONE`], then less in step with the time
/// left, none at [`GONE`] -- so the frame before the splash is gone already
/// differs from the bare page by less than the remainder does, whenever it
/// falls. The logo's own last ink, the block's, fades with it.
pub fn glow(age: f32) -> f32 {
    ((GONE - age) / DIM).clamp(0.0, 1.0)
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
/// while held; drawn as the first pass's trailing edge crosses it, from
/// [`GHOST`], and gone as the second pass's crosses it; and the fade's.
pub fn ink(look: SplashLook, at: Pt, scale: f32, left: f32, right: f32) -> f32 {
    let swept = |x: f32| smoothstep(left, right.max(left + 1.0), x);
    match look.phase {
        SplashPhase::Hold => 1.0,
        SplashPhase::Fade(p) => 1.0 - p.clamp(0.0, 1.0),
        SplashPhase::Plasma(age) if age < OUT => {
            GHOST + (1.0 - GHOST) * swept(edge(&vfx(at, scale, age)))
        }
        SplashPhase::Plasma(age) => (1.0 - swept(edge(&vfx(at, scale, age)))) * glow(age),
    }
}

/// **The block's opacity**: whole while held; drawn last, as the first
/// pass's edge closes the block's own width short of it, and gone last, as
/// the second pass's does; and the fade's.
pub fn block(look: SplashLook, at: Pt, scale: f32) -> f32 {
    let b = placed(BLOCK, at, scale);
    // Arrived within half a pixel, as a trail's corner is.
    let near = |x: f32| smoothstep(b.left - (b.right - b.left), b.left - 0.5, x);
    match look.phase {
        SplashPhase::Hold => 1.0,
        SplashPhase::Fade(p) => 1.0 - p.clamp(0.0, 1.0),
        SplashPhase::Plasma(age) if age < OUT => {
            GHOST + (1.0 - GHOST) * near(edge(&vfx(at, scale, age)))
        }
        SplashPhase::Plasma(age) => (1.0 - near(edge(&vfx(at, scale, age)))) * glow(age),
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

    /// **Both passes collapse into the block**: at each one's start the
    /// emitter is the box from the wordmark to the block, the hole the
    /// block, drawn at full strength, the second with a seed of its own;
    /// each has landed on the block, within a pixel, at its end; and the
    /// second's own end, the same at every size, comes after the splash is
    /// gone.
    #[test]
    fn both_passes_collapse_into_the_block() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let e = placed(EMITTER, o, s);
        let b = placed(BLOCK, o, s);
        for (start, landed, seed) in [(0.0, OUT - 1e-4, SEED), (OUT, LEFT - 1e-4, SEED_OUT)] {
            let first = vfx(o, s, start);
            let xs = first.emitter.corners.0.map(|p| p.x);
            assert!(xs.iter().any(|&x| (x - e.left).abs() < 1e-3), "{start}");
            assert!(xs.iter().any(|&x| (x - e.right).abs() < 1e-3), "{start}");
            assert_eq!(first.emitter.hole, b);
            assert_eq!(first.effect, Effect::Plasma);
            assert_eq!(first.emitter.strength, Strength::Full);
            assert_eq!(first.ignition.seed, seed);
            assert_eq!(first.ignition.age, 0.0, "lit at {start}");
            let late = vfx(o, s, landed).emitter.corners.0;
            assert!(
                late.iter()
                    .all(|p| p.x >= b.left - 1.0 && p.x <= b.right + 1.0),
                "{landed}: {late:?}"
            );
        }
        let out = end();
        assert!(GONE < out, "{GONE} {out}");
        assert_eq!(
            OUT + Effect::Plasma.end(&lit(o, s, SEED_OUT)) / PACE,
            out,
            "the same at another size"
        );
    }

    /// **Each trailing edge crosses at one speed**: the quad's leftmost
    /// corner, sampled every 5 ms through each pass, is where an even
    /// crossing from the wordmark's left to the block's left puts it, to
    /// within half a pixel; and each pass's plasma ages at [`PACE`] of the
    /// splash's clock. The control: on kitty's own curve, at the same
    /// clock, the edge is over two and a half times as far across a tenth
    /// of the way in.
    #[test]
    fn each_trailing_edge_crosses_at_one_speed() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let (e, b) = (placed(EMITTER, o, s), placed(BLOCK, o, s));
        for (from, span) in [(0.0, IN_S), (OUT, SWEEP_S)] {
            for k in 0u8..60 {
                let u = f32::from(k) / 60.0;
                let age = from + span * u;
                let v = vfx(o, s, age);
                let want = e.left + (b.left - e.left) * u;
                assert!(
                    (edge(&v) - want).abs() < 0.5,
                    "{age}: {} not {want}",
                    edge(&v)
                );
                assert!((v.ignition.age - (age - from) * PACE).abs() < 1e-5, "{age}");
            }
        }
        let kitty = Segment::new(e.corners(), b).at(IN_S * 0.1 * PACE);
        let x = kitty.0.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        assert!(
            x - e.left > 2.5 * 0.1 * (b.left - e.left),
            "the control: {x}"
        );
    }

    /// **The second pass takes the logo, and the glow fades after it**: the
    /// logo whole at [`OUT`]; the wordmark's first glyph gone before the
    /// prompt; nothing of the logo once the second pass's edge has crossed
    /// the block; the plasma's light whole until [`DIM`] before [`GONE`]
    /// and none at [`GONE`]; the fade taking all of it together.
    #[test]
    fn the_second_pass_takes_the_logo_and_the_glow_fades_after_it() {
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
        assert!(block(at(OUT), o, s) > 0.99, "{}", block(at(OUT), o, s));
        assert!(
            block(at(OUT - 1e-4), o, s) > 0.99,
            "whole as the first pass ends"
        );
        let t = OUT + 0.1;
        assert!(ink(at(t), o, s, b.0, b.1) < ink(at(t), o, s, p.0, p.1));
        assert_eq!(ink(at(LEFT), o, s, b.0, b.1), 0.0);
        assert_eq!(ink(at(LEFT), o, s, p.0, p.1), 0.0);
        assert!(block(at(LEFT), o, s) < 1e-3, "{}", block(at(LEFT), o, s));
        assert_eq!(block(at(GONE), o, s), 0.0);
        assert_eq!(glow(GONE - DIM), 1.0);
        assert!(glow(GONE - DIM / 2.0) > 0.4 && glow(GONE - DIM / 2.0) < 0.6);
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

    /// **Both passes run left to right, the block last**: at every age
    /// before the second pass each glyph is at least as drawn as the one to
    /// its right and the block the least; at every age after it lights,
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
                let ordered = match age < OUT {
                    true => w[0] + 1e-4 >= w[1],
                    false => w[0] <= w[1] + 1e-4,
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
        assert!(block(whole, o, s) > 0.99, "whole as the second pass lights");
        let midway = SplashLook {
            phase: SplashPhase::Plasma(0.1),
        };
        assert!(
            ink(midway, o, s, g[0].0, g[0].1) > ink(midway, o, s, g[4].0, g[4].1) + 0.2,
            "the left drawn before the right"
        );
    }
}
