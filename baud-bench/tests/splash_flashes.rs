//! **The launch splash's flashes, counted on the frames it draws** (note
//! AJ; WCAG 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]:
//! its whole life at 60 frames a second on the CPU path -- the pixels the
//! Linux driver shows and the macOS pixel oracle holds the GPU arms to --
//! from the blank window through the held logo, the plasma and after.
//!
//! **No pixel flashes more than three times in any second**, general or
//! red, on the dark and the light theme and over the two red grounds the
//! trail census uses, `#CD0000` and `#1A0000`, so the verdict's area is
//! zero. This window is small, 64 by 20 cells at 1x, for the debug suite;
//! the model is not scale-free (an ion's radius and a filament's width
//! have pixel floors), and the record gives the same count taken at 200 by
//! 50 cells at 2x, at 60 and 120 frames a second. The control: the
//! same logo shown and taken down every 125 ms, four times a second, does
//! count more than three flashes at a pixel, so the census sees the logo.

use baud_bench::flash::{Census, Flashes, Rate, Scale};
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
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
    let mut census = Census::new(w, h, Scale(1), Rate(RATE));
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    for &phase in phases {
        r.set_splash(phase.map(|phase| SplashLook { phase }));
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(&mut t, &mut target, Damage::Full);
        census.frame(&px);
    }
    census.finish()
}

/// A frame's time, seconds: one sixtieth a frame.
const FRAME_S: f32 = 1.0 / 60.0;

#[test]
fn the_splash_flashes_no_pixel_more_than_three_times_a_second() {
    let end = baud_render::splash::end();
    let mut life: Vec<Option<SplashPhase>> = vec![None; 12];
    life.extend((0..12).map(|_| Some(SplashPhase::Hold)));
    let ages = (0u16..).map(|f| f32::from(f) * FRAME_S);
    life.extend(
        ages.take_while(|&a| a <= end + 2.0 * FRAME_S)
            .map(|a| Some(SplashPhase::Vanish(a))),
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
