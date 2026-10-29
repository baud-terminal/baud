//! **The launch splash's flashes, counted on the frames it draws** (note
//! AJ; WCAG 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]:
//! its whole life at 60 frames a second on the CPU path -- the pixels the
//! Linux driver shows and the macOS pixel oracle holds the GPU arms to --
//! from the blank window it shows until the window is shown, through the
//! plasma the logo comes out of and is swept out in, gone at
//! [`baud_render::splash::GONE`], and after.
//!
//! **No pixel flashes more than three times in any second**, general or
//! red, on the dark and the light theme and over the two red grounds the
//! trail census uses, `#CD0000` and `#1A0000`, so the verdict's area is
//! zero. This window is small, 64 by 20 cells at 1x, for the debug suite;
//! the model is not scale-free (an ion's radius and a filament's width
//! have pixel floors), and the same count was taken at 200 by 50 cells at
//! 2x, at 60 and 120 frames a second, and over a 6120 by 3330 pixel frame
//! at 60. One plasma crosses the box: two full-strength plasmas there put
//! a fourth flash into one second at some pixel on three of the four
//! grounds unless 0.4 s of hold kept them apart.
//! The control: the same logo shown and taken down every 125 ms, four
//! times a second, does count more than three flashes at a pixel, so the
//! census sees the logo.

use baud_bench::flash::{Census, Flashes, Rate, Scale};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
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
fn the_splash_flashes_no_pixel_more_than_three_times_a_second() {
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
            eprintln!(
                "{name} {form}: general {} red {} red-encoded {}, area {}, strict {}",
                r.general.flashes.0, r.red.flashes.0, r.red_encoded.flashes.0, r.area.0, r.strict.0
            );
            assert_eq!(r.translucent.value(), 0, "{name} {form}: an opaque ground");
            assert!(
                r.general.flashes <= Flashes(3),
                "{name} {form}: {:?}",
                r.general
            );
            assert!(r.red.flashes <= Flashes(3), "{name} {form}: {:?}", r.red);
            assert!(
                r.red_encoded.flashes <= Flashes(3),
                "{name} {form}: {:?}",
                r.red_encoded
            );
            assert_eq!(r.area.0, 0.0, "{name} {form}");
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
/// sweep's end, [`baud_render::splash::OUT`] plus
/// [`baud_render::splash::SWEEP_S`], to [`baud_render::splash::GONE`], the
/// frame's greatest difference from the frame with no splash never grows,
/// every frame from two 60 Hz frames before the end is within 0.10 of it
/// at every pixel, and a frame in the last 60 Hz interval differs by at
/// most a quarter of what the sweep's end does -- on the four grounds, at
/// 60 and 120 frames a second -- so the remainder fades rather than being
/// cut, and the splash's going is no transition by WCAG's definition even
/// after a dropped frame. The control: a twentieth of a second in, some
/// pixel is not within 0.10.
#[test]
fn the_plasmas_remainder_fades_into_the_page() {
    use baud_render::splash::{GONE, OUT, SWEEP_S};
    let red = |bg: u32| Theme {
        bg: Argb(bg),
        ..Theme::dark()
    };
    for rate in [60.0f32, 120.0] {
        let ages: Vec<f32> = (0u16..)
            .map(|f| OUT + SWEEP_S + f32::from(f) / rate)
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
