//! The alternate-screen transition: switching between the primary and the
//! alternate screen -- opening or leaving a full-screen program -- slides
//! the new screen in over the old one, sfte's `SFTE_TERM_ANIMATE_SCREEN`
//! ([[source: sfte]], `_sfte_render_get_anim_offsets()` and
//! `_sfte_render_prepare_passes()`). Off by default, as sfte's is.
//!
//! **The slide** ([`ScreenSlide`]) is sfte's, line for line: a quarter of
//! a second ([`SLIDE_S`], `SFTE_TERM_ANIM_DUR_MS` 250), eased out on a
//! cubic, `1 - (1 - p)^3`, over the frame's height, the offset truncated
//! to whole pixels as sfte's `(uint32_t)` cast does. Entering the
//! alternate screen the new screen rises from below; leaving it, the new
//! screen comes down from above. **The old screen stays where it is** and
//! the new one slides over it, which is what sfte draws with its default
//! smooth scroll on: its offsets function moves both screens, and then
//! `passes[0].y_off = (int32_t)ctx->term.scroll_y_offset` sets the
//! outgoing screen's offset to the smooth scroll's, nothing at rest.
//! Where the two screens are is [`offsets`]; [`compose`] draws a frame of
//! it from the live frame and the outgoing one.
//!
//! The live frame is drawn every frame of the slide as any frame is, so
//! output arriving while it slides is drawn at once, where the screen then
//! is: the slide moves pixels and holds nothing back. **The outgoing frame
//! is the last frame drawn before the switch, drawn again with its cursor
//! hidden**, as sfte's outgoing pass hides it (`hide_cursor = 1`), when
//! the driver sees the switch ([`crate::Renderer::hide_cursor`]): the
//! screen the eye was on, unslid, which on either screen stays as it was
//! for the length of the slide, because what a program writes goes to the
//! screen it switched to.
//!
//! **The new screen slides in faint** and comes up to full once it has
//! landed. sfte's slide at full contrast moves dense text past each pixel
//! several times within the quarter second: counted on the frames by the
//! WCAG 2.3.1 census ([[source: wcag22]]), one slide over box drawing or
//! rows in reverse video flashes more than three times a second over more
//! than a quarter of a ten-degree field, and a slide in and back within a
//! second does over most dense screens. So while it moves the new screen
//! is under a veil in the default background at the least alpha that
//! leaves none of the palette's first 16 colours, the foreground or the
//! colour cube's corners a transition from the background or from one
//! another ([`faint_alpha`]), and the veil fades off over 0.15 s once it
//! has landed. Truecolor reds those do not name are counted by the census
//! too, and pass.
//!
//! **The veil** ([`Transition`]) is one translucent full-screen solid in
//! the default background, fading to reveal the new screen, driven by a
//! `progress` the driver computes from the display link's timestamp the
//! way [`crate::Blink`] is: from the faint alpha after a slide, and from
//! its own peak alone as the switch's form under reduced motion, a
//! dissolve in place of the motion.
//!
//! A screen switch changes every cell, so unlike the cursor trail neither
//! is bounded to a band -- each covers the frame, and every row is drawn
//! while it runs. That is honest: the whole screen is what is changing.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use baud_domain::Chan;
use baud_font::Px;

/// A screen switch being veiled: how far along the fade the driver has
/// advanced it -- 0.0 fully veiled, 1.0 clear and the new screen settled
/// -- and which veil it fades from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Transition {
    pub progress: f32,
    pub from: VeilFrom,
}

impl Transition {
    /// The fade under reduced motion, `progress` of the way.
    pub const fn fade(progress: f32) -> Transition {
        Transition {
            progress,
            from: VeilFrom::Fade,
        }
    }

    /// The slide's faint veil, `progress` of the way to clear: held at 0
    /// while the screen slides, then faded off once it has landed.
    pub const fn faint(progress: f32) -> Transition {
        Transition {
            progress,
            from: VeilFrom::Faint,
        }
    }
}

/// **The alpha a veil starts from**: the fade's peak, or the slide's faint
/// veil -- the least that leaves none of the inks the renderer reads a
/// flash from the background or from one another ([`faint_alpha`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VeilFrom {
    Fade,
    Faint,
}

/// The veil for a frame: a full-screen rectangle at the alpha the fade is
/// at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Veil {
    pub w: Px,
    pub h: Px,
    pub alpha: Chan,
}

/// The alpha the veil starts at, before the fade.
const PEAK_ALPHA: u8 = 0xE0;

/// The veil for `t` over a frame `w` by `h`: full-screen, at an alpha that
/// fades from where `t` starts -- the fade's peak, or `faint` for the
/// slide's veil -- to zero as the new screen settles.
pub fn veil(t: Transition, w: Px, h: Px, faint: Chan) -> Veil {
    let p = t.progress.clamp(0.0, 1.0);
    let peak = match t.from {
        VeilFrom::Fade => PEAK_ALPHA,
        VeilFrom::Faint => faint.get(),
    };
    let fade = (f32::from(peak) * (1.0 - p)).round().max(0.0) as i64;
    Veil {
        w,
        h,
        alpha: Chan::from_u8(u8::try_from(fade).unwrap_or(0)),
    }
}

/// The veil as one translucent solid every path draws, in the default
/// background `bg` at its faded alpha, at the frame origin.
pub fn instance(v: Veil, bg: Argb) -> Instance {
    Instance {
        x: Screen(0),
        y: Screen(0),
        w: Tex(u16::try_from(v.w.value()).unwrap_or(u16::MAX)),
        h: Tex(u16::try_from(v.h.value()).unwrap_or(u16::MAX)),
        u: Tex(0),
        v: Tex(0),
        color: bg.with_alpha(v.alpha),
        kind: Kind::Solid,
        curve: baud_domain::contrast::CurveRow::default(),
    }
}

/// **Seconds the slide takes**: sfte's `SFTE_TERM_ANIM_DUR_MS`, 250.
pub const SLIDE_S: f32 = 0.25;

/// Which screen a switch brought up: the alternate screen, which the new
/// screen rises from below to cover, or the primary, which comes down from
/// above -- sfte's `anim_dir` 1 and -1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toward {
    Alternate,
    Primary,
}

/// A screen switch being slid: which way, and how far along the driver
/// has advanced it -- 0.0 the old screen still in place, 1.0 the new one
/// settled.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ScreenSlide {
    pub progress: f32,
    pub toward: Toward,
}

/// Where a slide's two frames are, in pixel rows down from where each
/// settles: the outgoing frame and the incoming one, drawn over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Offsets {
    pub outgoing: i32,
    pub incoming: i32,
}

/// **How far the slide has moved, in pixel rows of a frame `h` tall**:
/// sfte's `ease * height` on `1 - (1 - p)^3`, truncated.
pub fn travel(s: ScreenSlide, h: Px) -> i32 {
    let p = s.progress.clamp(0.0, 1.0);
    let q = 1.0 - p;
    let ease = 1.0 - q * q * q;
    let h = i32::try_from(h.value()).unwrap_or(i32::MAX);
    (ease * h as f32) as i32
}

/// **The two frames' places for `s` over a frame `h` tall**, sfte's
/// `_sfte_render_get_anim_offsets()` for the new frame -- toward the
/// alternate screen it rises from `h` below, toward the primary it comes
/// down from `h` above -- and the old frame in place, as sfte's passes
/// leave it with smooth scroll on.
pub fn offsets(s: ScreenSlide, h: Px) -> Offsets {
    let slide = travel(s, h);
    let h = i32::try_from(h.value()).unwrap_or(i32::MAX);
    let incoming = match s.toward {
        Toward::Alternate => h - slide,
        Toward::Primary => slide - h,
    };
    Offsets {
        outgoing: 0,
        incoming,
    }
}

/// **The row of the frame row `y` a slide draws from**, of `h` rows: the
/// incoming frame's where it lies, which is drawn over the outgoing one,
/// else the outgoing frame's. `None` where neither lies, which no
/// [`offsets`] leaves.
pub fn source(o: Offsets, y: Px, h: Px) -> Option<(Frame, Px)> {
    let (y, h) = (i64::from(y.value()), i64::from(h.value()));
    let inside = |v: i64| {
        (0..h)
            .contains(&v)
            .then(|| Px::of(u32::try_from(v).unwrap_or(0)))
    };
    inside(y - i64::from(o.incoming))
        .map(|r| (Frame::Incoming, r))
        .or_else(|| inside(y - i64::from(o.outgoing)).map(|r| (Frame::Outgoing, r)))
}

/// Which of a slide's two frames a row is drawn from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Frame {
    Outgoing,
    Incoming,
}

/// **A frame of the slide, row by row**: `out`'s `h` rows of `stride`
/// pixels, each from `live` -- the new screen as it is now -- or `kept`,
/// the old screen kept at the switch, at `o`. Every buffer is `h`
/// rows of `stride`; `false`, and nothing written, when one is short.
pub fn compose(live: &[u32], kept: &[u32], out: &mut [u32], stride: Px, h: Px, o: Offsets) -> bool {
    let slots = |p: Px| usize::try_from(p.value()).unwrap_or(usize::MAX);
    let (s, n) = (slots(stride), slots(stride).saturating_mul(slots(h)));
    if s == 0 || live.len() < n || kept.len() < n || out.len() < n {
        return false;
    }
    for (y, dst) in (0..h.value()).map(Px::of).zip(out.chunks_exact_mut(s)) {
        let (from, r) = match source(o, y, h) {
            Some((Frame::Incoming, r)) => (live, r),
            Some((Frame::Outgoing, r)) => (kept, r),
            None => (live, y),
        };
        let at = slots(r) * s;
        dst.copy_from_slice(&from[at..at + s]);
    }
    true
}

/// **The most of the screen's contrast a sliding screen keeps**: the
/// largest alpha `a` at which every ink in `inks`, drawn at `a` over the
/// default background `ground` -- the slide's veil at `255 - a` over it --
/// is no WCAG 2.3.1 transition from the ground nor from any other ink so
/// drawn ([`crate::flash`], the thresholds the trail's repeat passes are
/// drawn under). A slide of full contrast moves dense text past each pixel
/// several times within a quarter of a second, more than three flashes at
/// a pixel over more than a quarter of a ten-degree field on box drawing
/// and on rows in reverse video. The renderer finds it on the first slide
/// over a background and keeps it: about 0.3 ms on this Mac, once.
pub fn faint_alpha(ground: Argb, inks: &[Argb]) -> Chan {
    let cap = inks
        .iter()
        .map(|&i| crate::flash::under_threshold(i, ground).get())
        .min()
        .unwrap_or(u8::MAX);
    let drawn = |a: u8| -> Vec<Argb> {
        inks.iter()
            .map(|&i| crate::flash::over_ground(i, ground, f32::from(a) / 255.0))
            .collect()
    };
    let calm = |a: u8| {
        let d = drawn(a);
        d.iter()
            .enumerate()
            .all(|(k, &x)| d[k + 1..].iter().all(|&y| !crate::flash::flashes(x, y)))
    };
    let a = (0..=cap).rev().find(|&a| calm(a)).unwrap_or(0);
    Chan::from_u8(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn veil_of(t: Transition, w: Px, h: Px) -> Veil {
        veil(t, w, h, Chan::from_u8(0))
    }

    const W: Px = Px::of(800);
    const H: Px = Px::of(500);

    /// At the start the veil is nearly opaque and covers the frame; at the
    /// end it is gone; the size is the frame's throughout.
    #[test]
    fn the_veil_fades_from_the_peak_to_nothing() {
        let start = veil_of(Transition::fade(0.0), W, H);
        assert_eq!(start.alpha.get(), 0xE0);
        assert_eq!((start.w.value(), start.h.value()), (800, 500));

        let mid = veil_of(Transition::fade(0.5), W, H);
        assert_eq!(mid.alpha.get(), 0x70);

        let end = veil_of(Transition::fade(1.0), W, H);
        assert_eq!(end.alpha.get(), 0);
        assert_eq!((end.w.value(), end.h.value()), (800, 500));
    }

    /// sfte's curve over a 400-row frame: the ease-out cubic, truncated,
    /// so half way is 7/8 of the way, the two ways mirror each other, and
    /// the old frame stays put.
    #[test]
    fn the_slide_eases_out_over_the_frame_both_ways() {
        let up = |p| {
            offsets(
                ScreenSlide {
                    progress: p,
                    toward: Toward::Alternate,
                },
                Px::of(400),
            )
        };
        let down = |p| {
            offsets(
                ScreenSlide {
                    progress: p,
                    toward: Toward::Primary,
                },
                Px::of(400),
            )
        };
        assert_eq!(
            up(0.0),
            Offsets {
                outgoing: 0,
                incoming: 400
            }
        );
        assert_eq!(
            up(0.5),
            Offsets {
                outgoing: 0,
                incoming: 50
            }
        );
        assert_eq!(
            up(1.0),
            Offsets {
                outgoing: 0,
                incoming: 0
            }
        );
        assert_eq!(
            down(0.0),
            Offsets {
                outgoing: 0,
                incoming: -400
            }
        );
        assert_eq!(
            down(0.5),
            Offsets {
                outgoing: 0,
                incoming: -50
            }
        );
        assert_eq!(
            down(1.0),
            Offsets {
                outgoing: 0,
                incoming: 0
            }
        );
        // 1 - 0.9^3 = 0.271 of 400 is 108.4, truncated.
        assert_eq!(up(0.1).incoming, 400 - 108);
        assert_eq!(up(1.5), up(1.0));
    }

    /// Every row of a slid frame comes from one of the two frames, the
    /// incoming one where it lies, at the row the offsets name.
    #[test]
    fn a_composed_frame_takes_each_row_from_its_frame() {
        let (w, h) = (3usize, 8usize);
        let px = |v: usize| Px::of(u32::try_from(v).unwrap());
        let row = |i: usize| u32::try_from(i / w).unwrap();
        let live: Vec<u32> = (0..w * h).map(|i| 0x100 + row(i)).collect();
        let kept: Vec<u32> = (0..w * h).map(|i| 0x200 + row(i)).collect();
        let mut out = vec![0; w * h];
        let o = offsets(
            ScreenSlide {
                progress: 0.5,
                toward: Toward::Alternate,
            },
            Px::of(8),
        );
        assert_eq!(
            o,
            Offsets {
                outgoing: 0,
                incoming: 1
            }
        );
        assert!(compose(&live, &kept, &mut out, px(w), px(h), o));
        let rows: Vec<u32> = out.chunks(w).map(|r| r[0]).collect();
        assert_eq!(
            rows,
            [0x200, 0x100, 0x101, 0x102, 0x103, 0x104, 0x105, 0x106]
        );
        let o = offsets(
            ScreenSlide {
                progress: 0.5,
                toward: Toward::Primary,
            },
            Px::of(8),
        );
        assert!(compose(&live, &kept, &mut out, px(w), px(h), o));
        let rows: Vec<u32> = out.chunks(w).map(|r| r[0]).collect();
        assert_eq!(
            rows,
            [0x101, 0x102, 0x103, 0x104, 0x105, 0x106, 0x107, 0x207]
        );
        assert!(!compose(&live, &kept[1..], &mut out, px(w), px(h), o));
    }

    /// The inks the renderer reads the faint veil over: the palette's
    /// first 16, the foreground, the colour cube's corners.
    fn inks(theme: &crate::Theme) -> Vec<Argb> {
        let c = Chan::from_u8;
        let corners = (0u8..8).map(|k| {
            let on = |b: u8| c(if k & b != 0 { u8::MAX } else { 0 });
            Argb::rgb(on(1), on(2), on(4))
        });
        theme.palette[..16]
            .iter()
            .copied()
            .chain([theme.fg])
            .chain(corners)
            .map(|i| i.with_alpha(c(u8::MAX)))
            .collect()
    }

    /// **The faint veil keeps the most contrast that makes no flash**, on
    /// the dark and the light theme: no ink drawn at it is a transition
    /// from the background or another ink, it keeps some of every screen,
    /// and one step more makes a transition -- the control that the scan
    /// can fire.
    #[test]
    fn the_faint_veil_keeps_the_most_contrast_that_makes_no_flash() {
        for theme in [crate::Theme::dark(), crate::Theme::light()] {
            let ground = theme.bg.with_alpha(Chan::from_u8(u8::MAX));
            let inks = inks(&theme);
            let a = faint_alpha(ground, &inks).get();
            let drawn = |a: u8| -> Vec<Argb> {
                inks.iter()
                    .map(|&i| crate::flash::over_ground(i, ground, f32::from(a) / 255.0))
                    .chain([ground])
                    .collect()
            };
            let flashing = |a: u8| {
                let d = drawn(a);
                d.iter()
                    .enumerate()
                    .any(|(k, &x)| d[k + 1..].iter().any(|&y| crate::flash::flashes(x, y)))
            };
            eprintln!("ground {:08x}: keeps {a}", ground.0);
            assert!(
                a >= 8,
                "{:08x}: the slide keeps some of the screen, {a}",
                ground.0
            );
            assert!(!flashing(a), "{:08x}: no flash at {a}", ground.0);
            assert!(flashing(a + 1), "{:08x}: a flash at {}", ground.0, a + 1);
        }
    }

    /// Progress is clamped, so a driver that overshoots draws the settled,
    /// invisible veil rather than a negative alpha.
    #[test]
    fn progress_is_clamped() {
        let over = veil_of(Transition::fade(1.5), W, H);
        let end = veil_of(Transition::fade(1.0), W, H);
        assert_eq!(over, end);
    }
}
