//! **The launch splash**: baud's colour lockup, `baud ❯ █`, drawn in the
//! middle of the window from the first frame, and gone in the plasma once
//! the shell has drawn something (note AJ). The lockup is
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
//! **It vanishes in the plasma trail's own model**, [`crate::plasma`]: the
//! lockup's block is a cursor, and the box from the wordmark's left to the
//! block collapses into it as kitty's quad collapses into the cell a
//! cursor reached ([`Segment`]), the tube filling it, the filaments
//! braiding into the block and the ions peeling off. The wordmark and the
//! prompt go where the quad's trailing edge has passed, and the block
//! starves with the tube, as `(1 - k)^2`. So no new shader: every path
//! draws it by the effect it already draws, and the pixel oracle holds it
//! by the reference it already has.
//!
//! **Everything is a pure function of the phase**: the driver hands
//! [`SplashLook`] -- the logo held, so many seconds into the vanish, or so
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
    /// The logo, whole: from the first frame until the shell draws
    /// something or the cap passes.
    Hold,
    /// So many seconds into the plasma.
    Vanish(f32),
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

/// **The plasma `age` seconds into the vanish**: the emitter collapsing
/// from its box into the block on kitty's constants, the block its hole,
/// the axis the move's.
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

/// **Seconds from the vanish's start to when nothing of it is left**: the
/// plasma's end -- its life, or its last ion's death -- which the block,
/// starving with the tube, never outlasts. The ion count reads the path's
/// length in units, so this is the same at every size.
pub fn end() -> f32 {
    Effect::Plasma.end(&ignition(Pt { x: 0.0, y: 0.0 }, 1.0))
}

/// **A glyph's opacity**, `left` to `right` its box in frame pixels: whole
/// while held, gone as the plasma's emitter's trailing edge crosses it,
/// and the fade's.
pub fn ink(look: SplashLook, at: Pt, scale: f32, left: f32, right: f32) -> f32 {
    match look.phase {
        SplashPhase::Hold => 1.0,
        SplashPhase::Fade(p) => 1.0 - p.clamp(0.0, 1.0),
        SplashPhase::Vanish(age) => {
            let c = vfx(at, scale, age).emitter.corners.0;
            let trailing = c.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
            1.0 - smoothstep(left, right.max(left + 1.0), trailing)
        }
    }
}

/// **The block's opacity**: whole while held, starving with the tube as
/// `(1 - k)^2` over the plasma's life, and the fade's.
pub fn block(look: SplashLook) -> f32 {
    match look.phase {
        SplashPhase::Hold => 1.0,
        SplashPhase::Fade(p) => 1.0 - p.clamp(0.0, 1.0),
        SplashPhase::Vanish(age) => {
            let k = (age / crate::plasma::LIFE).clamp(0.0, 1.0);
            (1.0 - k) * (1.0 - k)
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

    /// **The vanish is the plasma's, into the block**: at its start the
    /// emitter is the box from the wordmark to the block, the hole the
    /// block; it collapses into the block; and it is out by the plasma's
    /// end, which is the same at every size.
    #[test]
    fn the_vanish_collapses_the_plasma_into_the_block() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let first = vfx(o, s, 0.0);
        let e = placed(EMITTER, o, s);
        let xs = first.emitter.corners.0.map(|p| p.x);
        assert!(xs.iter().any(|&x| (x - e.left).abs() < 1e-3));
        assert!(xs.iter().any(|&x| (x - e.right).abs() < 1e-3));
        assert_eq!(first.emitter.hole, placed(BLOCK, o, s));
        assert_eq!(first.effect, Effect::Plasma);
        let late = vfx(o, s, 0.5).emitter.corners.0;
        let b = placed(BLOCK, o, s);
        assert!(
            late.iter()
                .all(|p| p.x >= b.left - 1.0 && p.x <= b.right + 1.0)
        );
        let out = end();
        assert!((crate::plasma::LIFE..=0.65).contains(&out), "{out}");
        assert_eq!(
            Effect::Plasma.end(&ignition(o, s)),
            out,
            "the same at another size"
        );
        assert_eq!(vfx(o, s, out + 0.01).light(b.center()), 0.0);
    }

    /// **The ink goes where the plasma has passed, the block with the
    /// tube**: whole while held; the wordmark's first glyph gone before the
    /// prompt; the block dimming as the tube does and dark at the plasma's
    /// life; the fade taking all of it together.
    #[test]
    fn the_ink_goes_where_the_plasma_has_passed() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let b = (o.x + 40.0 * s, o.x + 75.0 * s);
        let p = (o.x + 192.0 * s, o.x + 224.0 * s);
        let held = SplashLook {
            phase: SplashPhase::Hold,
        };
        assert_eq!(ink(held, o, s, b.0, b.1), 1.0);
        assert_eq!(block(held), 1.0);
        let at = |t: f32| SplashLook {
            phase: SplashPhase::Vanish(t),
        };
        assert_eq!(ink(at(0.0), o, s, b.0, b.1), 1.0);
        let t = 0.02;
        assert!(ink(at(t), o, s, b.0, b.1) < ink(at(t), o, s, p.0, p.1));
        assert!(ink(at(0.3), o, s, b.0, b.1) < 1e-3);
        assert!(block(at(0.2)) < 1.0 && block(at(0.2)) > 0.0);
        assert_eq!(block(at(crate::plasma::LIFE)), 0.0);
        let half = SplashLook {
            phase: SplashPhase::Fade(0.5),
        };
        assert_eq!(ink(half, o, s, b.0, b.1), 0.5);
        assert_eq!(block(half), 0.5);
    }
}
