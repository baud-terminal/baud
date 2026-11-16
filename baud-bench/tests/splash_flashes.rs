//! **The launch splash's flashes, counted on the frames it draws** (note
//! AJ; WCAG 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]:
//! its whole life at 60 frames a second on the CPU path -- the pixels the
//! Linux driver shows and the macOS pixel oracle holds the GPU arms to --
//! from the blank window it shows until the window is shown, through the
//! two plasmas that carry the logo in and out, gone at
//! [`baud_render::splash::GONE`], and after.
//!
//! **It passes the general and red flash thresholds as WCAG 2.2 defines
//! them**: content is below them if "there are no more than three general
//! flashes and / or no more than three red flashes within any one-second
//! period; or the combined area of flashes occurring concurrently occupies
//! no more than a total of .006 steradians within any 10 degree visual
//! field on the screen (25% of any 10 degree visual field on the screen)".
//! The census's verdict is that rule, the field Note 1's 341 by 256 pixels
//! read in points; this test held the splash to at most three flashes at
//! every pixel, which is the rule's first half alone, stricter than the
//! text. The two passes, at kitty's pace and a tenth of a second apart,
//! each spanning the lockup for a tenth of a second, the plasma in the
//! colour of the glyph it is over, put a fourth flash into one second at a
//! few pixels of the box they both cross; the most of any field that
//! flashes more than three times is under 0.2% of the limit on every
//! ground and size counted, and under 10% on the chained reading of a
//! flash (the census's strict verdict), which is asserted too. Both are printed with their margin.
//! This window is small, 64 by 20 cells at 1x, for the debug suite. At
//! this size the whole lockup covers less than the area limit, so the
//! verdict alone cannot fail here, and the margin assertion is what holds
//! it. The model is not scale-free (an ion's radius and a filament's
//! width have pixel floors), so the same census was taken at 200 by 50
//! cells at 2x, at 60 and 120 frames a second, and over a 6120 by 3330
//! pixel frame at 2x at 60 (note AJ).
//! The control: the same logo shown and taken down every 125 ms, four
//! times a second, does count more than three flashes at a pixel, so the
//! census sees the logo.

use baud_bench::flash::{Census, Flashes, Outcome, Rate, Scale};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Argb, Blend, Damage, FaceRole, FaceStyle, PxSize, Renderer, SplashLook, SplashPhase, Stack,
    Target, Theme,
};

const WS: WinSize = WinSize {
    ws_col: 64,
    ws_row: 20,
};

const RATE: u32 = 60;

fn font(p: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Counts `phases`, one a frame, over a blank screen in `theme`.
fn count(theme: &Theme, phases: &[Option<SplashPhase>]) -> baud_bench::flash::Report {
    let (frames, (w, h)) = draw(theme, phases);
    let mut census = Census::new(w, h, Scale(1), Rate(RATE));
    for px in &frames {
        census.frame(px);
    }
    census.finish()
}

/// `phases`, one a frame, drawn over a blank screen in `theme`, and the
/// frame's size.
fn draw(theme: &Theme, phases: &[Option<SplashPhase>]) -> (Vec<Vec<u32>>, (Px, Px)) {
    let (regular, bold) = (
        font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
        font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
    );
    let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
    let mut r = Renderer::new(
        Stack::with_roles(
            &faces,
            &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
        )
        .unwrap(),
        PxSize::of(14.0).unwrap(),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let hide = b"\x1b[?25l";
    t.write_at(BytePos::ZERO, hide);
    t.feed(BytePos::ZERO, ReadLen::from_index(hide.len()));
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let mut frames = Vec::with_capacity(phases.len());
    for &phase in phases {
        r.set_splash(phase.map(|phase| SplashLook { phase }));
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
        frames.push(px.clone());
    }
    (frames, (w, h))
}

/// A frame's time, seconds: one sixtieth a frame.
const FRAME_S: f32 = 1.0 / 60.0;

#[test]
fn the_splash_is_below_the_flash_thresholds() {
    // Nothing of the logo until the window is shown.
    let mut life: Vec<Option<SplashPhase>> = vec![None; 12];
    life.extend(
        (0u16..)
            .map(|f| f32::from(f) * FRAME_S)
            .take_while(|&a| a < baud_render::splash::GONE)
            .map(|a| Some(SplashPhase::Plasma(a))),
    );
    life.extend((0..12).map(|_| None));
    let fade: Vec<Option<SplashPhase>> = (0..12)
        .map(|_| Some(SplashPhase::Hold))
        .chain((0u16..12).map(|f| {
            Some(SplashPhase::Fade(
                f32::from(f) * FRAME_S / baud_render::splash::FADE_S,
            ))
        }))
        .chain((0..12).map(|_| None))
        .collect();
    let red = |bg: u32| Theme {
        bg: Argb(bg),
        ..Theme::dark()
    };
    for (name, theme) in [
        ("dark", Theme::dark()),
        ("light", Theme::light()),
        ("#CD0000", red(0xFFCD_0000)),
        ("#1A0000", red(0xFF1A_0000)),
    ] {
        for (form, phases) in [("plasma", &life), ("fade", &fade)] {
            let r = count(&theme, phases);
            let margin = |a: f64| 100.0 * (1.0 - a / r.threshold.0);
            eprintln!(
                "{name} {form}: general {} red {} red-encoded {}, area {} ({:.1}% margin), strict {} ({:.1}% margin)",
                r.general.flashes.0,
                r.red.flashes.0,
                r.red_encoded.flashes.0,
                r.area.0,
                margin(r.area.0),
                r.strict.0,
                margin(r.strict.0)
            );
            assert_eq!(r.translucent.value(), 0, "{name} {form}: an opaque ground");
            assert_eq!(r.outcome(), Outcome::Passes, "{name} {form}: {r:?}");
            assert_eq!(r.strict_outcome(), Outcome::Passes, "{name} {form}: {r:?}");
            assert!(margin(r.area.0) > 90.0, "{name} {form}: {r:?}");
        }
    }
    // The control: shown and taken down every 125 ms for two seconds.
    let toggled: Vec<Option<SplashPhase>> = (0..2 * RATE)
        .map(|f| {
            (f * 8 / RATE)
                .is_multiple_of(2)
                .then_some(SplashPhase::Hold)
        })
        .collect();
    let c = count(&Theme::dark(), &toggled);
    assert!(
        c.general.flashes > Flashes(3),
        "the control: {:?}",
        c.general
    );
}

/// The relative luminance of a pixel: WCAG 2.2's piecewise sRGB curve.
fn luminance(p: u32) -> f32 {
    let c = |v: u32| {
        let s = (v & 255) as f32 / 255.0;
        if s <= 0.040_45 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * c(p >> 16) + 0.7152 * c(p >> 8) + 0.0722 * c(p)
}

/// **What is left of the plasma fades into the bare page**: from the
/// logo's going, [`baud_render::splash::LEFT`], to
/// [`baud_render::splash::GONE`], the frame's greatest difference from the
/// frame with no splash never grows, every frame from two 60 Hz frames
/// before the end is within 0.10 of it at every pixel, and a frame in the
/// last 60 Hz interval differs by at most a quarter of what the logo's
/// going does -- on the four grounds, at 60 and 120 frames a second -- so
/// what is left after the logo only fades (a cut at that frame itself is
/// held by baud-render's
/// `the_plasma_is_full_strength_and_its_remainder_fades_out`), and the
/// splash's going is no transition by WCAG's definition even after a
/// dropped frame. The control: a twentieth of a second in, some
/// pixel is not within 0.10.
#[test]
fn the_plasmas_remainder_fades_into_the_page() {
    use baud_render::splash::{GONE, LEFT};
    let red = |bg: u32| Theme {
        bg: Argb(bg),
        ..Theme::dark()
    };
    for rate in [60.0f32, 120.0] {
        let ages: Vec<f32> = (0u16..)
            .map(|f| LEFT + f32::from(f) / rate)
            .take_while(|&a| a < GONE)
            .collect();
        let mut phases = vec![None, Some(SplashPhase::Plasma(0.05))];
        phases.extend(ages.iter().map(|&a| Some(SplashPhase::Plasma(a))));
        for (name, theme) in [
            ("dark", Theme::dark()),
            ("light", Theme::light()),
            ("#CD0000", red(0xFFCD_0000)),
            ("#1A0000", red(0xFF1A_0000)),
        ] {
            let (frames, _) = draw(&theme, &phases);
            let page: Vec<f32> = frames[0].iter().map(|&p| luminance(p)).collect();
            let most = |f: &[u32]| {
                f.iter()
                    .zip(&page)
                    .map(|(&p, &h)| (luminance(p) - h).abs())
                    .fold(0.0f32, f32::max)
            };
            assert!(most(&frames[1]) >= 0.10, "{name}: the control");
            let first = most(&frames[2]);
            let mut last = f32::INFINITY;
            for (&age, f) in ages.iter().zip(&frames[2..]) {
                let d = most(f);
                assert!(d <= last + 1e-3, "{name} at {rate} Hz: {age} s grew, {d}");
                last = d;
                if age >= GONE - 2.0 / 60.0 {
                    assert!(d < 0.10, "{name} at {rate} Hz: {age} s, {d}");
                }
                if age >= GONE - 1.0 / 60.0 {
                    assert!(
                        d <= first / 4.0,
                        "{name} at {rate} Hz: {age} s, {d} of {first}"
                    );
                }
            }
        }
    }
}

/// **How much of the plasma is seen, coming in and going out**: the
/// plasma's own light over the bare page -- each pixel's relative
/// luminance with each record's own colour laid over the page in order at
/// the alpha it draws there, against the page's -- summed over the pixels
/// and the frames, in points squared and seconds. Split where the logo
/// starts to leave, [`baud_render::splash::OUT`]: coming in before it,
/// going out from it to the last frame that draws any of the logo. At 64
/// by 20 cells at 1x, 60 frames a second, on the four grounds, the plasma
/// its filaments and ions alone, no tube in the box
/// ([`baud_render::effect::Body::Bare`]). Coming in is at least each
/// ground's floor below, and going out at least 0.7 of coming in: a second
/// plasma at the same pace and place. The floors are a bound on how much
/// is seen, not on how it flashes: less light is further from WCAG
/// 2.3.1's thresholds, which `the_splash_is_below_the_flash_thresholds`
/// holds the same frames to.
#[test]
fn the_plasma_is_seen_carrying_the_logo_in_and_out() {
    use baud_render::{Kind, Pt};
    let red = |bg: u32| Theme {
        bg: Argb(bg),
        ..Theme::dark()
    };
    let life: Vec<Option<SplashPhase>> = (0u16..)
        .map(|f| f32::from(f) * FRAME_S)
        .take_while(|&a| a < baud_render::splash::GONE)
        .map(|a| Some(SplashPhase::Plasma(a)))
        .collect();
    for (name, theme, floor) in [
        ("dark", Theme::dark(), 9.66),
        ("light", Theme::light(), 27.68),
        ("#CD0000", red(0xFFCD_0000), 8.05),
        ("#1A0000", red(0xFF1A_0000), 9.04),
    ] {
        let page = theme.bg;
        let (regular, bold) = (
            font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
            font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
        );
        let faces = [Face::parse(&regular).unwrap(), Face::parse(&bold).unwrap()];
        let mut r = Renderer::new(
            Stack::with_roles(
                &faces,
                &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)],
            )
            .unwrap(),
            PxSize::of(14.0).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let (w, h) = r.pixel_size();
        let (wu, hu) = (
            usize::try_from(w.value()).unwrap(),
            usize::try_from(h.value()).unwrap(),
        );
        let mut px = vec![0u32; wu * hu];
        let ground = luminance(page.0);
        // (age, light seen this frame, the lockup's ink)
        let mut frames: Vec<(f32, f64, f32)> = Vec::new();
        for &phase in &life {
            let Some(SplashPhase::Plasma(age)) = phase else {
                unreachable!("the life is the plasma's")
            };
            r.set_splash(phase.map(|phase| SplashLook { phase }));
            let mut target = Target::new(&mut px, w, w, h).unwrap();
            r.render(&mut t, &mut target, Damage::Full);
            // Each record's own colour, source-over in order over the page,
            // as the frame draws it: the word's ink over the word, the
            // amber over the mark, the steps between.
            let pg = [page.r(), page.g(), page.b()].map(|c| f32::from(c.get()));
            let mut mixed: Vec<[f32; 3]> = vec![pg; wu * hu];
            let mut lit = vec![false; wu * hu];
            let v = r.vfx().expect("the splash's plasma");
            for rec in r.trail() {
                let c = [rec.color.r(), rec.color.g(), rec.color.b()].map(|c| f32::from(c.get()));
                let (x0, y0) = (i32::from(rec.x.0), i32::from(rec.y.0));
                let (rw, rh) = (f32::from(rec.w.0), f32::from(rec.h.0));
                let a0 = f32::from(rec.color.a().get()) / 255.0;
                let (wi, hi) = (i32::try_from(wu).unwrap(), i32::try_from(hu).unwrap());
                for y in y0.max(0)..(y0 + i32::from(rec.h.0)).min(hi) {
                    for x in x0.max(0)..(x0 + i32::from(rec.w.0)).min(wi) {
                        let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                        let l = match rec.kind {
                            Kind::Particle => v.particle(Pt {
                                x: (cx - x0 as f32) / rw * 2.0 - 1.0,
                                y: (cy - y0 as f32) / rh * 2.0 - 1.0,
                            }),
                            _ => v.light(Pt { x: cx, y: cy }),
                        };
                        let a = (a0 * l).clamp(0.0, 1.0);
                        if a > 0.0 {
                            let i = usize::try_from(y).unwrap() * wu + usize::try_from(x).unwrap();
                            for k in 0..3 {
                                mixed[i][k] = c[k] * a + mixed[i][k] * (1.0 - a);
                            }
                            lit[i] = true;
                        }
                    }
                }
            }
            let seen: f64 = mixed
                .iter()
                .zip(&lit)
                .filter(|&(_, &on)| on)
                .map(|(m, _)| {
                    let q = |v: f32| Chan::from_unit(v / 255.0);
                    let m = Argb::rgb(q(m[0]), q(m[1]), q(m[2]));
                    f64::from((luminance(m.0) - ground).abs())
                })
                .sum();
            let ink = r
                .splash()
                .iter()
                .map(|i| f32::from(i.color.a().get()) / 255.0)
                .sum();
            frames.push((age, seen / f64::from(RATE), ink));
        }
        let out = baud_render::splash::OUT;
        let coming: f64 = frames.iter().filter(|f| f.0 < out).map(|f| f.1).sum();
        let going: f64 = frames
            .iter()
            .filter(|f| f.0 >= out)
            .take_while(|f| f.2 > 1e-4)
            .map(|f| f.1)
            .sum();
        eprintln!("{name}: coming in {coming:.2}, going out {going:.2} (floor {floor} coming in)");
        assert!(
            coming >= floor && going >= 0.7 * coming,
            "{name}: coming in {coming}, at least {floor}; going out {going}, at least {}",
            0.7 * coming
        );
    }
}
