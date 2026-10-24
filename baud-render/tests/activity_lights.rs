//! The activity lights: the pixels the title strip shows (`lights::draw`,
//! the pixel oracle for a strip no GPU path draws -- it is an image in a
//! view of its own, not part of the grid's frame), and the clock that
//! lights them (`Activity`), held to WCAG 2.2's three flashes a second and
//! to showing every byte.

mod support;

use baud_domain::{Chan, HostTime};
use baud_domain::{Cols, Rows};
use baud_font::Face;
use baud_render::lights::{DOT, DWELL, HOLD, draw, width};
use baud_render::{
    Activity, Argb, Backdrop, Blend, ByShown, GROUND_DARK, GROUND_LIGHT, Glow, Light, LightsImage,
    LightsLook, PxSize, Renderer, Shown, Stack, Theme,
};
use core::time::Duration;
use support::font;

const GREEN: Argb = Argb(0xFF_30_D1_58);
const ORANGE: Argb = Argb(0xFF_FF_9F_0A);
const LABEL: Argb = Argb(0x8C_FF_FF_FF);

fn look() -> LightsLook {
    LightsLook::new(
        [GREEN, ORANGE],
        LABEL,
        GROUND_DARK,
        Glow::Full,
        Backdrop::under_window_dark(),
    )
}

/// A whole pixel count from a length in pixels, truncated.
fn px(v: f32) -> u32 {
    u32::try_from(v as i64).unwrap()
}

fn at(img: &LightsImage, x: f32, y: f32) -> Argb {
    img.pixels[usize::try_from(px(y) * img.width.value() + px(x)).unwrap()]
}

/// The centres of RD's light and SD's in device pixels, laid out by hand:
/// JetBrains Mono's advance is 600 of 1000 units, so "RD" at 8 pt is 9.6
/// pt, then the 3 pt gap and half the light; then the other half, the 9 pt
/// between the pairs, "SD" and the gap again.
fn centres(img: &LightsImage, scale: f32) -> [(f32, f32); 2] {
    let h = img.height.value() as f32 / 2.0;
    let rd = (9.6 + 3.0 + DOT / 2.0) * scale;
    [(rd, h), (rd + (DOT + 9.0 + 9.6 + 3.0) * scale, h)]
}

/// WCAG's relative luminance of an opaque colour.
fn luminance(c: Argb) -> f64 {
    let lin = |v: Chan| {
        let s = f64::from(v.get()) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// A straight-alpha pixel over an opaque ground.
fn on(p: Argb, ground: Argb) -> Argb {
    let a = f64::from(p.a().get()) / 255.0;
    let ch = |s: Chan, d: Chan| {
        Chan::from_unit(((f64::from(s.get()) * a + f64::from(d.get()) * (1.0 - a)) / 255.0) as f32)
    };
    Argb::rgb(
        ch(p.r(), ground.r()),
        ch(p.g(), ground.g()),
        ch(p.b(), ground.b()),
    )
}

/// **Each light shows its state in its own colour, and its letters sit to
/// its left**: at 1x and 2x, a lit light's centre is its lit colour and a
/// dark one's its dark colour, every state of the four; RD's letters put
/// ink left of RD's light and SD's between the two; the strip is as wide as
/// `width` says and as tall as the strip.
#[test]
fn each_light_is_drawn_in_its_state_with_its_letters_to_the_left() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    for scale in [1.0_f32, 2.0] {
        for shown in Shown::ALL {
            let mut img = LightsImage::new();
            draw(&face, &look(), shown, scale, 28.0, &mut img).unwrap();
            assert_eq!(img.height.value(), px(28.0 * scale));
            assert_eq!(img.width.value(), px((width(&face) * scale).ceil()));
            let [(rx, ry), (sx, sy)] = centres(&img, scale);
            let [rd_lit, sd_lit] = look().lit;
            let [rd_dark, sd_dark] = look().dark;
            let want = |light, lit, dark| match light {
                Light::Lit => lit,
                Light::Dark => dark,
            };
            // The colour, and the alpha a disc's inside reaches: the
            // rasteriser's sum can leave it a step short of 255.
            let same = |p: Argb, c: Argb| {
                (p.r(), p.g(), p.b()) == (c.r(), c.g(), c.b()) && p.a().get() >= 0xFE
            };
            let (p, w) = (at(&img, sx, sy), want(shown.sd, sd_lit, sd_dark));
            assert!(same(p, w), "SD at {scale}x, {shown:?}: {p:?} for {w:?}");
            let (p, w) = (at(&img, rx, ry), want(shown.rd, rd_lit, rd_dark));
            assert!(same(p, w), "RD at {scale}x, {shown:?}: {p:?} for {w:?}");
            assert!(rx + DOT * scale < sx, "RD left of SD");
            // Letters: ink in the label's colour left of each light.
            let ink = |from: f32, to: f32| {
                (px(from)..px(to)).any(|x| {
                    (0..img.height.value()).any(|y| {
                        let p = img.pixels[usize::try_from(y * img.width.value() + x).unwrap()];
                        p.a().get() > 0
                            && (p.r(), p.g(), p.b()) == (LABEL.r(), LABEL.g(), LABEL.b())
                    })
                })
            };
            assert!(ink(0.0, rx - DOT * scale / 2.0), "RD's letters at {scale}x");
            assert!(
                ink(rx + DOT * scale, sx - DOT * scale / 2.0),
                "SD's letters at {scale}x"
            );
            assert_eq!(at(&img, 0.0, 0.0).a().get(), 0, "the strip shows through");
        }
    }
}

/// **The renderer draws the strip in the badge's face**: the same pixels as
/// `draw` over that face, so the window shows what this file tests.
#[test]
fn the_renderer_draws_the_strip_the_oracle_holds() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(28.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(6),
        Rows::of_index(2),
    )
    .unwrap();
    let shown = Shown {
        rd: Light::Lit,
        sd: Light::Dark,
    };
    let (mut a, mut b) = (LightsImage::new(), LightsImage::new());
    r.activity_lights(&look(), shown, 2.0, 28.0, &mut a)
        .unwrap();
    draw(&faces[0], &look(), shown, 2.0, 28.0, &mut b).unwrap();
    assert_eq!(a, b);
    assert_eq!(r.activity_lights_width(), width(&faces[0]));
}

/// **Under Dim Flashing Lights no change of a light is a flash**: every
/// pixel of both lights, lit and dark, composited over every grey of
/// either appearance's material, and over every grey from black to white
/// for the glass, differs by less than WCAG's 10% of
/// relative luminance (or both are 0.80 or lighter) and is no saturated
/// red; at full glow the same change is a general flash, the control that
/// this probe can fire.
#[test]
fn dimmed_lights_change_under_the_flash_threshold() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    for (ground, backdrop) in [
        (GROUND_DARK, Backdrop::under_window_dark()),
        (GROUND_LIGHT, Backdrop::under_window_light()),
        (GROUND_DARK, Backdrop::any()),
        (GROUND_LIGHT, Backdrop::any()),
    ] {
        for (glow, flashes) in [(Glow::Dimmed, false), (Glow::Full, true)] {
            let look = LightsLook::new([GREEN, ORANGE], LABEL, ground, glow, backdrop);
            let mut lit = LightsImage::new();
            let mut dark = LightsImage::new();
            draw(&face, &look, Shown::ALL[3], 2.0, 28.0, &mut lit).unwrap();
            draw(&face, &look, Shown::DARK, 2.0, 28.0, &mut dark).unwrap();
            let mut any = false;
            for g in backdrop.greys() {
                for (l, d) in lit.pixels.iter().zip(&dark.pixels) {
                    let (l, d) = (on(*l, g), on(*d, g));
                    let (yl, yd) = (luminance(l), luminance(d));
                    let general = (yl - yd).abs() >= 0.10 && yl.min(yd) < 0.80;
                    if !flashes {
                        assert!(!general, "{glow:?} over {g:?}: {l:?} against {d:?}");
                        assert!(!red(l) && !red(d), "no saturated red: {l:?} {d:?}");
                    }
                    any |= general;
                }
            }
            assert_eq!(any, flashes, "{glow:?} over {ground:?}");
        }
    }
}

/// WCAG's saturated red: R / (R + G + B) of 0.8 or more on the linear
/// values.
fn red(c: Argb) -> bool {
    let lin = |v: Chan| {
        let s = f64::from(v.get()) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let s = lin(c.r()) + lin(c.g()) + lin(c.b());
    s > 0.0 && lin(c.r()) / s >= 0.8
}

const TICK: Duration = Duration::from_nanos(8_333_333);

fn t(d: Duration) -> HostTime {
    HostTime::ZERO.plus(Duration::from_secs(1) + d)
}

/// A deterministic train of byte times: bursts and gaps of every length a
/// typist, a flood and an idle shell make, from a fixed seed.
fn train(seed: u64, len: Duration) -> Vec<Duration> {
    let mut x = seed | 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut at = Duration::ZERO;
    let mut out = Vec::new();
    while at < len {
        out.push(at);
        let gap = match next() % 4 {
            0 => next() % 20,
            1 => next() % 250,
            2 => next() % 600,
            _ => next() % 3_000,
        };
        at += Duration::from_millis(gap + 1);
    }
    out
}

/// The light a clock shows on every tick across `len`, fed `bytes`.
fn run(
    bytes: &[Duration],
    len: Duration,
    step: impl Fn(&mut Activity, HostTime) -> Light,
    feed: impl Fn(&mut Activity, HostTime),
) -> Vec<(Duration, Light)> {
    let mut a = Activity::new();
    let mut i = 0;
    let mut now = Duration::ZERO;
    let mut out = Vec::new();
    while now < len {
        while i < bytes.len() && bytes[i] <= now {
            feed(&mut a, t(bytes[i]));
            i += 1;
        }
        out.push((now, step(&mut a, t(now))));
        now += TICK;
    }
    out
}

/// The most changes in any closed one-second window.
fn most_changes(seen: &[(Duration, Light)]) -> usize {
    let changes: Vec<Duration> = seen
        .windows(2)
        .filter(|w| w[0].1 != w[1].1)
        .map(|w| w[1].0)
        .collect();
    (0..changes.len())
        .map(|i| {
            changes[i..]
                .iter()
                .take_while(|&&c| c <= changes[i] + Duration::from_secs(1))
                .count()
        })
        .max()
        .unwrap_or(0)
}

/// **At most three flashes in any second, and every byte shown, on both
/// lights**: across sixty seeded trains of thirty seconds each, ticked at
/// 120 Hz, no closed one-second window holds more than six changes (three
/// pairs of opposing changes, WCAG 2.2's count); every byte has its light
/// lit within `DWELL` and a tick of it; a lit light goes dark only once
/// the last byte's `HOLD` is over; and it is lit only within
/// `HOLD` plus `DWELL` and a tick of a byte, and dark once that long has
/// passed with none. The control: a light that flashes 50 ms for each
/// byte, the Linux tty trigger's blink, makes more than six on the same
/// trains, so the count can fail.
#[test]
fn the_lights_flash_at_most_three_times_a_second_and_show_every_byte() {
    let len = Duration::from_secs(30);
    let slack = TICK + Duration::from_nanos(1);
    let mut naive_worst = 0;
    for seed in 1..=60_u64 {
        let bytes = train(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), len);
        for which in [Light::Lit, Light::Dark] {
            // `which` picks the light: Lit runs RD, Dark runs SD.
            let seen = run(
                &bytes,
                len,
                |a, now| match which {
                    Light::Lit => a.step(now).rd,
                    Light::Dark => a.step(now).sd,
                },
                |a, at| match which {
                    Light::Lit => a.received(at),
                    Light::Dark => a.sent(at),
                },
            );
            assert!(
                most_changes(&seen) <= 6,
                "seed {seed}: {}",
                most_changes(&seen)
            );
            for &b in bytes.iter().filter(|&&b| b + DWELL + slack < len) {
                assert!(
                    seen.iter()
                        .any(|&(at, l)| l == Light::Lit && at >= b && at <= b + DWELL + slack)
                        || seen
                            .iter()
                            .any(|&(at, l)| l == Light::Lit && at <= b && at + TICK > b),
                    "seed {seed}: the byte at {b:?} was never shown"
                );
            }
            // A lit light goes dark only once the last byte's hold is
            // over: steady bytes keep it lit.
            for w in seen.windows(2) {
                let ((_, was), (at, l)) = (w[0], w[1]);
                let held = bytes
                    .iter()
                    .any(|&b| b + TICK <= at && at + TICK < b + HOLD);
                assert!(
                    !(was == Light::Lit && l == Light::Dark && held),
                    "seed {seed}: dark at {at:?} inside a byte's hold"
                );
            }
            for &(at, l) in &seen {
                let quiet = !bytes
                    .iter()
                    .any(|&b| b <= at && at < b + HOLD + DWELL + slack);
                assert!(
                    l == Light::Dark || !quiet,
                    "seed {seed}: still lit at {at:?}"
                );
            }
        }
        // The control: 50 ms lit for every byte, as the kernel blinks.
        let mut i = 0;
        let mut until = Duration::ZERO;
        let mut now = Duration::ZERO;
        let mut naive = Vec::new();
        while now < len {
            while i < bytes.len() && bytes[i] <= now {
                until = bytes[i] + Duration::from_millis(50);
                i += 1;
            }
            naive.push((now, if now < until { Light::Lit } else { Light::Dark }));
            now += TICK;
        }
        naive_worst = naive_worst.max(most_changes(&naive));
    }
    assert!(naive_worst > 6, "the control flashed {naive_worst} times");
}

/// **A cache of one picture a state hands back the state's own**.
#[test]
fn a_picture_a_state() {
    let by = ByShown::of(|s| s);
    for s in Shown::ALL {
        assert_eq!(*by.get(s), s);
    }
}
