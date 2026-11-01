//! **iTerm2's fireworks' flashes, counted on the frames baud draws** (WCAG
//! 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]: a whole
//! show on the CPU path -- the pixels the Linux driver shows and the macOS
//! pixel oracle holds the GPU arms to -- from the frame before it to the
//! frames after it is gone, the cursor's cell in the middle of a 64 by 20
//! cell window at 1x, small for the debug suite and wider than the 341 by
//! 256 point field the area is counted in.
//!
//! **baud's look passes the general and red flash thresholds, and iTerm2's
//! does not.** Content is below them if "there are no more than three
//! general flashes and / or no more than three red flashes within any
//! one-second period; or the combined area of flashes occurring
//! concurrently occupies no more than a total of .006 steradians within
//! any 10 degree visual field on the screen (25% of any 10 degree visual
//! field on the screen)". Drawn with every number iTerm2's
//! (`baud_render::fireworks::ITERM2`), the show flashes more than three
//! times a second over more than that 25% on the chained reading at 60
//! frames a second and on both at 120 (on the verdict's at 60, 20,598 of
//! the 21,824 square points) -- the positive control, so the census sees
//! the stars. Drawn with baud's (`baud_render::fireworks::BAUD`,
//! the slow stars at 40 points a second and half as many fast ones), it is
//! under the threshold on both the census's readings, the verdict's and the
//! chained one, at 60 and 120 frames a second, over the dark and light
//! pages and two red ones, each under 60% of the area; the margins are
//! printed. The worst was 9,665 square points, chained, on the light page
//! at 120. The still form Reduce Motion draws lights a pixel once and
//! fades it once, and is counted too.

use baud_bench::flash::{Census, Outcome, Rate, Report, Scale};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::effect::Seed;
use baud_render::fireworks::{self, BAUD, Form, HOLD, ITERM2, Look, ORIGINS, Show};
use baud_render::{Argb, Blend, Damage, Pt, PxSize, Renderer, Stack, Target, Theme};

const WS: WinSize = WinSize {
    ws_col: 64,
    ws_row: 20,
};

/// The window the long census draws: 128 by 45 cells, 1024 by 765 points.
const WIDE: WinSize = WinSize {
    ws_col: 128,
    ws_row: 45,
};

/// Where a census draws: its window, and device pixels a point.
struct Window {
    ws: WinSize,
    scale: Scale,
}

const SMALL: &Window = &Window {
    ws: WS,
    scale: Scale(1),
};

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// **What a run draws**: one show held a second after its request, as
/// one request starts; one held [`HELD`] seconds, as a request every
/// second or less keeps restarting its hold; or two, the second begun 1.1
/// s after the first, as a request in the first's fade starts another.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Run {
    One,
    Held,
    Two,
}

/// Seconds a [`Run::Held`] show holds.
const HELD: f32 = 4.0;

/// When the second show of a [`Run::Two`] begins, seconds in.
const SECOND: f32 = 1.1;

impl Run {
    /// Seconds from the first show's start to the last one's end.
    fn end(self) -> f32 {
        match self {
            Run::One => HOLD + fireworks::FADE,
            Run::Held => HELD + fireworks::FADE,
            Run::Two => SECOND + HOLD + fireworks::FADE,
        }
    }

    /// The shows `t` seconds in, each from its start as the driver times
    /// it, the older first.
    fn shows(self, t: f32, base: Show) -> Vec<Show> {
        let up = |s: Show| (s.age >= 0.0 && s.age < s.end()).then_some(s);
        match self {
            Run::One => up(Show { age: t, ..base }).into_iter().collect(),
            Run::Held => up(Show {
                age: t,
                hold: HELD,
                ..base
            })
            .into_iter()
            .collect(),
            Run::Two => [
                up(Show { age: t, ..base }),
                up(Show {
                    age: t - SECOND,
                    seed: Seed(base.seed.0 ^ 0x5EC0_4D00),
                    ..base
                }),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }
}

/// A run of shows in `look` and `form` at `rate` frames a second over a
/// blank page in `theme`, counted: a frame before it, its frames, and a
/// sixth of a second after its end.
fn count(theme: &Theme, look: Look, form: Form, rate: u32, win: &Window) -> Report {
    count_run(theme, (look, form, Run::One), Rate(rate), win)
}

fn count_run(
    theme: &Theme,
    (look, form, run): (Look, Form, Run),
    rate: Rate,
    win: &Window,
) -> Report {
    let ws = WinSize {
        ws_col: win.ws.ws_col,
        ws_row: win.ws.ws_row,
    };
    let k = f32::from(u16::try_from(win.scale.0).unwrap());
    let regular = font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&regular).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0 * k).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let hide = b"\x1b[?25l";
    t.write_at(BytePos::ZERO, hide);
    t.feed(BytePos::ZERO, ReadLen::from_index(hide.len()));
    let (w, h) = r.pixel_size();
    let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap());
    let at = Pt {
        x: (side(w) * 0.5).floor() + 0.5,
        y: (side(h) * 0.5).floor() + 0.5,
    };
    let mut census = Census::new(w, h, win.scale, rate);
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let frames =
        u32::try_from((f64::from(run.end() + 1.0 / 6.0) * f64::from(rate.0)).ceil() as i64)
            .unwrap();
    let base = Show {
        age: 0.0,
        hold: HOLD,
        origins: [at; ORIGINS],
        hole: at,
        scale: k,
        seed: Seed(0x0F1E_E7A5),
        look,
        form,
    };
    for f in 0..=frames {
        let shows: Vec<Show> = match f {
            0 => Vec::new(),
            _ => run.shows(
                f32::from(u16::try_from(f - 1).unwrap())
                    / f32::from(u16::try_from(rate.0).unwrap()),
                base,
            ),
        };
        r.set_fireworks(&shows);
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
        census.frame(&px);
    }
    census.finish()
}

#[test]
fn bauds_fireworks_are_below_the_flash_thresholds_and_iterm2s_are_not() {
    let red = |bg: u32| Theme {
        bg: Argb(bg),
        ..Theme::dark()
    };
    // The control first: iTerm2's own look fails -- the chained reading at
    // 60 frames a second, and the verdict at 120, where the verdict's count
    // at 60 is 20,598, just under.
    for (rate, verdict) in [(60, Outcome::Passes), (120, Outcome::Fails)] {
        let c = count(&Theme::dark(), ITERM2, Form::Moving, rate, SMALL);
        eprintln!(
            "iTerm2's look, dark, {rate} Hz: area {} of {} (contiguous {}), strict {}, general flashes {} red {}",
            c.area.0,
            c.threshold.0,
            c.contiguous.0,
            c.strict.0,
            c.general.flashes.0,
            c.red.flashes.0
        );
        assert_eq!(c.strict_outcome(), Outcome::Fails, "the control: {c:?}");
        assert_eq!(c.outcome(), verdict, "the control: {c:?}");
    }
    for (name, theme) in [
        ("dark", Theme::dark()),
        ("light", Theme::light()),
        ("#CD0000", red(0xFFCD_0000)),
        ("#1A0000", red(0xFF1A_0000)),
    ] {
        for (form, rate) in [(Form::Moving, 60), (Form::Moving, 120), (Form::Still, 60)] {
            let r = count(&theme, BAUD, form, rate, SMALL);
            let margin = |a: f64| 100.0 * (1.0 - a / r.threshold.0);
            eprintln!(
                "{name} {form:?} {rate} Hz: general {} red {} red-encoded {}, area {} ({:.1}% margin), strict {} ({:.1}% margin)",
                r.general.flashes.0,
                r.red.flashes.0,
                r.red_encoded.flashes.0,
                r.area.0,
                margin(r.area.0),
                r.strict.0,
                margin(r.strict.0)
            );
            assert_eq!(r.translucent.value(), 0, "{name}: an opaque ground");
            assert_eq!(
                r.outcome(),
                Outcome::Passes,
                "{name} {form:?} {rate}: {r:?}"
            );
            assert_eq!(
                r.strict_outcome(),
                Outcome::Passes,
                "{name} {form:?} {rate}: {r:?}"
            );
            assert!(margin(r.strict.0) > 40.0, "{name} {form:?} {rate}: {r:?}");
            if form == Form::Still {
                assert!(r.general.flashes.0 <= 1, "{name}: still, {r:?}");
            }
        }
    }
}

/// **Requests that keep a show up, or start a second, stay under the
/// thresholds too**: a show held four seconds -- a program asking again
/// every second or less, so every burst is up through any one second of
/// it -- at 60 and 120 frames a second, and two shows, the second begun in
/// the first's fade, at 120, over the dark and light pages, on both
/// readings, each under 80% of the area; the margins are printed. The
/// worst was the held show's, 16,602 square points, chained, on the dark
/// page at 120; two shows measured 10,422 at most, at 120.
#[test]
fn held_and_overlapping_fireworks_are_below_the_flash_thresholds() {
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for (run, rate) in [(Run::Held, 60), (Run::Held, 120), (Run::Two, 120)] {
            let r = count_run(&theme, (BAUD, Form::Moving, run), Rate(rate), SMALL);
            let margin = |a: f64| 100.0 * (1.0 - a / r.threshold.0);
            eprintln!(
                "{name} {run:?} {rate} Hz: general {} red {}, area {} ({:.1}% margin), strict {} ({:.1}% margin)",
                r.general.flashes.0,
                r.red.flashes.0,
                r.area.0,
                margin(r.area.0),
                r.strict.0,
                margin(r.strict.0)
            );
            assert_eq!(r.outcome(), Outcome::Passes, "{name} {run:?} {rate}: {r:?}");
            assert_eq!(
                r.strict_outcome(),
                Outcome::Passes,
                "{name} {run:?} {rate}: {r:?}"
            );
            assert!(margin(r.strict.0) > 20.0, "{name} {run:?} {rate}: {r:?}");
        }
    }
}

/// **The same census over a full window**, 1024 by 765 points at 1x and
/// 2x, iTerm2's look and baud's at 60 and 120 frames a second on the dark
/// and light pages: too long for the debug suite, so run by name
/// (`--ignored`); its counts are printed.
#[test]
#[ignore]
fn bauds_fireworks_pass_over_a_full_window() {
    for scale in [1, 2] {
        let win = &Window {
            ws: WIDE,
            scale: Scale(scale),
        };
        for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            for rate in [60, 120] {
                for (which, look) in [("iTerm2", ITERM2), ("baud", BAUD)] {
                    let r = count(&theme, look, Form::Moving, rate, win);
                    eprintln!(
                        "{scale}x {name} {rate} Hz {which}: area {} strict {} of {}",
                        r.area.0, r.strict.0, r.threshold.0
                    );
                    if which == "baud" {
                        assert_eq!(r.outcome(), Outcome::Passes, "{r:?}");
                        assert_eq!(r.strict_outcome(), Outcome::Passes, "{r:?}");
                    }
                }
            }
        }
    }
}
