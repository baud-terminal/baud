//! The activity lights: the pixels the title strip shows (`lights::draw`
//! and `lights::draw_led`, the pixel oracle for a strip no GPU path draws
//! -- each light is an image in a view of its own, not part of the grid's
//! frame), and the clock that lights them (`Activity`), held to WCAG 2.2's
//! three flashes a second, to showing everything it is told, and under
//! Reduce Motion to one steady light while the activity lasts.

mod support;

use baud_domain::{Chan, HostTime};
use baud_domain::{Cols, Rows};
use baud_font::Face;
use baud_render::lights::{DOT, GAP, PAIR, TEXT, draw, draw_led, full, layout, width};
use baud_render::{
    Activity, Argb, Backdrop, Blend, ByLed, ByLight, GROUND_DARK, GROUND_LIGHT, Glow, Led, Light,
    LightsImage, LightsLook, Pace, PxSize, READ_CEILING, Renderer, Shown, Stack, Theme,
};
use core::time::Duration;
use support::font;

const GREEN: Argb = Argb(0xFF_30_D1_58);
const ORANGE: Argb = Argb(0xFF_FF_92_30);
const BLUE: Argb = Argb(0xFF_00_91_FF);
const LABEL: Argb = Argb(0x8C_FF_FF_FF);

/// The three colours by direction, as the macOS window resolves them.
fn colours() -> ByLed<Argb> {
    ByLed::of(|l| match l {
        Led::Rd | Led::Hs => GREEN,
        Led::Sd | Led::Aa | Led::Cs => ORANGE,
        Led::Cd | Led::Oh | Led::Fax | Led::Syn => BLUE,
    })
}

fn look() -> LightsLook {
    LightsLook::new(
        colours(),
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

/// A light's centre in its own picture, in device pixels, laid out by
/// hand: JetBrains Mono's advance is 600 of 1000 units, so two letters at 8
/// pt are 9.6 pt and three 14.4, then the 3 pt gap and half the light.
fn centre(led: Led, img: &LightsImage, scale: f32) -> (f32, f32) {
    let letters = 0.6 * TEXT * led.letters().len() as f32;
    let h = img.height.value() as f32 / 2.0;
    ((letters + GAP + DOT / 2.0) * scale, h)
}

/// The colour, and the alpha a disc's inside reaches: the rasteriser's sum
/// can leave it a step short of 255.
fn same(p: Argb, c: Argb) -> bool {
    (p.r(), p.g(), p.b()) == (c.r(), c.g(), c.b()) && p.a().get() >= 0xFE
}

/// Whether any pixel in columns `from..to` and rows `rows` is the label's
/// colour.
fn ink(img: &LightsImage, from: f32, to: f32, rows: core::ops::Range<u32>) -> bool {
    (px(from)..px(to)).any(|x| {
        rows.clone().any(|y| {
            let p = img.pixels[usize::try_from(y * img.width.value() + x).unwrap()];
            p.a().get() > 0 && (p.r(), p.g(), p.b()) == (LABEL.r(), LABEL.g(), LABEL.b())
        })
    })
}

/// **Each light shows its state in its own colour, and its letters sit to
/// its left**: at 1x and 2x, for each of the nine and each state, a lit
/// light's centre is its lit colour and a dark one's its dark colour; an
/// unread one is a ring, its centre clear and its edge the letters'
/// colour; the letters put ink left of the light and none right of it;
/// only CS has ink above its letters' caps, its bar; the picture is its
/// slot's width and the strip's height.
#[test]
fn each_light_is_drawn_in_its_state_with_its_letters_to_the_left() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    for scale in [1.0_f32, 2.0] {
        let (slots, _) = layout(&face, scale).unwrap();
        for led in Led::ALL {
            for light in Light::ALL {
                let mut img = LightsImage::new();
                draw_led(&face, &look(), led, light, scale, 28.0, &mut img).unwrap();
                assert_eq!(img.height.value(), px(28.0 * scale));
                assert_eq!(img.width, slots.get(led).width, "{led:?}");
                let (cx, cy) = centre(led, &img, scale);
                let p = at(&img, cx, cy);
                match light {
                    Light::Lit => assert!(same(p, *look().lit.get(led)), "{led:?} {p:?}"),
                    Light::Dark => assert!(same(p, *look().dark.get(led)), "{led:?} {p:?}"),
                    Light::Unread => {
                        assert_eq!(p.a().get(), 0, "{led:?} at {scale}x: a ring's centre");
                        let edge = at(&img, cx + (DOT / 2.0 - 0.6) * scale, cy);
                        assert!(
                            (edge.r(), edge.g(), edge.b()) == (LABEL.r(), LABEL.g(), LABEL.b())
                                && edge.a().get() > 0,
                            "{led:?} at {scale}x: the ring {edge:?}"
                        );
                    }
                }
                let left = cx - DOT * scale / 2.0;
                let all = 0..img.height.value();
                assert!(ink(&img, 0.0, left, all.clone()), "{led:?}'s letters");
                assert!(
                    !ink(
                        &img,
                        cx + DOT * scale / 2.0 + 1.0,
                        img.width.value() as f32,
                        all
                    ),
                    "{led:?}: nothing right of the light"
                );
                // Above the caps: the letters' caps are centred on the
                // light, 0.73 of 8 pt tall.
                let caps_top = cy - 0.73 * TEXT * scale / 2.0;
                let bar = ink(&img, 0.0, left, 0..px(caps_top - 0.2 * scale));
                assert_eq!(bar, led == Led::Cs, "{led:?} at {scale}x: the bar");
                assert_eq!(at(&img, 0.0, 0.0).a().get(), 0, "the strip shows through");
            }
        }
    }
}

/// **The strip is its lights side by side**: the slots run left to right
/// in the 1988 Courier manual's order, a light's gap apart and never
/// overlapping, the last ending at the strip's width, which `width` gives
/// in points; and every pixel of the strip drawn at a state is the pixel
/// of that light's own picture at its slot, so the views placed by the
/// layout show the strip this file tests. Nine lights at 2x are 516 pixels
/// wide.
#[test]
fn the_strip_is_its_lights_side_by_side() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    assert_eq!(
        Led::ALL.map(Led::letters),
        ["HS", "AA", "CD", "OH", "RD", "SD", "CS", "FAX", "SYN"]
    );
    for scale in [1.0_f32, 2.0] {
        let (slots, w) = layout(&face, scale).unwrap();
        let mut end = 0;
        for led in Led::ALL {
            let s = slots.get(led);
            assert!(s.start.value() >= end, "{led:?} after the last");
            if led != Led::Hs {
                assert!(s.start.value() - end >= px((PAIR - 1.0) * scale), "{led:?}");
            }
            end = s.start.value() + s.width.value();
        }
        assert_eq!(end, w.value());
        assert!((w.value() as f32 - width(&face) * scale).abs() <= 1.0);
        let shown = ByLed::of(|l| match l {
            Led::Hs | Led::Oh | Led::Sd | Led::Fax => Light::Lit,
            Led::Cd => Light::Unread,
            _ => Light::Dark,
        });
        let mut strip = LightsImage::new();
        draw(&face, &look(), shown, scale, 28.0, &mut strip).unwrap();
        assert_eq!(strip.width, w);
        for led in Led::ALL {
            let mut one = LightsImage::new();
            draw_led(&face, &look(), led, *shown.get(led), scale, 28.0, &mut one).unwrap();
            let x0 = slots.get(led).start.value();
            for y in 0..one.height.value() {
                for x in 0..one.width.value() {
                    let a = one.pixels[usize::try_from(y * one.width.value() + x).unwrap()];
                    let b = strip.pixels[usize::try_from(y * w.value() + x0 + x).unwrap()];
                    assert_eq!(a, b, "{led:?} at ({x}, {y}), {scale}x");
                }
            }
        }
    }
    let (_, w) = layout(&face, 2.0).unwrap();
    assert_eq!(w.value(), 516);
}

/// **The renderer draws the strip in the badge's face**: the same pixels as
/// `draw` and `draw_led` over that face, and the same layout, so the window
/// shows what this file tests.
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
    let shown = ByLed {
        rd: Light::Lit,
        cd: Light::Unread,
        ..Shown::DARK
    };
    let (mut a, mut b) = (LightsImage::new(), LightsImage::new());
    r.activity_lights(&look(), shown, 2.0, 28.0, &mut a)
        .unwrap();
    draw(&faces[0], &look(), shown, 2.0, 28.0, &mut b).unwrap();
    assert_eq!(a, b);
    for led in Led::ALL {
        r.activity_light(&look(), led, Light::Lit, 2.0, 28.0, &mut a)
            .unwrap();
        draw_led(&faces[0], &look(), led, Light::Lit, 2.0, 28.0, &mut b).unwrap();
        assert_eq!(a, b, "{led:?}");
    }
    assert_eq!(r.activity_lights_width(), width(&faces[0]));
    assert_eq!(r.activity_lights_layout(2.0), layout(&faces[0], 2.0));
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

/// **Under Dim Flashing Lights no change of a light is a flash**: every
/// pixel of all nine lights, each of the three changes a light can make --
/// lit against dark, unread against dark, unread against lit -- composited
/// over every grey of either appearance's material, and over every grey
/// from black to white for the glass, with that appearance's letters,
/// differs by less than WCAG's 10% of relative luminance (or both are 0.80
/// or lighter) and is no saturated red; at full glow lit against dark and
/// unread against dark are general flashes, the controls that this probe
/// can fire. A look with three colours scans each once, and the lights
/// that share a colour share its caps.
#[test]
fn dimmed_lights_change_under_the_flash_threshold() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    const LABEL_LIGHT: Argb = Argb(0x7F_00_00_00);
    for (ground, backdrop, label) in [
        (GROUND_DARK, Backdrop::under_window_dark(), LABEL),
        (GROUND_LIGHT, Backdrop::under_window_light(), LABEL_LIGHT),
        (GROUND_DARK, Backdrop::any(), LABEL),
        (GROUND_LIGHT, Backdrop::any(), LABEL_LIGHT),
    ] {
        for glow in [Glow::Dimmed, Glow::Full] {
            let look = LightsLook::new(colours(), label, ground, glow, backdrop);
            assert_eq!(look.dim.rd, look.dim.hs, "one cap a colour");
            assert_eq!(look.dim.cd, look.dim.syn, "one cap a colour");
            let drawn = |light| {
                let mut img = LightsImage::new();
                draw(&face, &look, Shown::all(light), 2.0, 28.0, &mut img).unwrap();
                img
            };
            let (lit, dark, unread) = (drawn(Light::Lit), drawn(Light::Dark), drawn(Light::Unread));
            for (name, a, b, control) in [
                ("lit/dark", &lit, &dark, true),
                ("unread/dark", &unread, &dark, true),
                ("unread/lit", &unread, &lit, false),
            ] {
                let mut any = false;
                for g in backdrop.greys() {
                    for (l, d) in a.pixels.iter().zip(&b.pixels) {
                        let (l, d) = (on(*l, g), on(*d, g));
                        let (yl, yd) = (luminance(l), luminance(d));
                        let general = (yl - yd).abs() >= 0.10 && yl.min(yd) < 0.80;
                        if glow == Glow::Dimmed {
                            assert!(!general, "{name} {glow:?} over {g:?}: {l:?} against {d:?}");
                            assert!(!red(l) && !red(d), "no saturated red: {l:?} {d:?}");
                        }
                        any |= general;
                    }
                }
                if glow == Glow::Full && control {
                    assert!(any, "the control: {name} at full glow over {ground:?}");
                }
            }
        }
    }
}

const TICK: Duration = Duration::from_nanos(8_333_333);

fn t(d: Duration) -> HostTime {
    HostTime::ZERO.plus(Duration::from_secs(1) + d)
}

/// A deterministic train of event times: bursts and gaps of every length a
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

/// The state `led` shows on every tick across `len`, at `pace`, fed
/// `events`.
fn run(led: Led, events: &[Duration], len: Duration, pace: Pace) -> Vec<(Duration, Light)> {
    let mut a = Activity::new();
    let mut i = 0;
    let mut now = Duration::ZERO;
    let mut out = Vec::new();
    while now < len {
        while i < events.len() && events[i] <= now {
            a.seen(led, t(events[i]));
            i += 1;
        }
        out.push((now, *a.step(t(now), pace).get(led)));
        now += TICK;
    }
    out
}

/// The most changes in any closed window of `span`.
fn most_changes(seen: &[(Duration, Light)], span: Duration) -> usize {
    let changes: Vec<Duration> = seen
        .windows(2)
        .filter(|w| w[0].1 != w[1].1)
        .map(|w| w[1].0)
        .collect();
    (0..changes.len())
        .map(|i| {
            changes[i..]
                .iter()
                .take_while(|&&c| c <= changes[i] + span)
                .count()
        })
        .max()
        .unwrap_or(0)
}

/// **At most three flashes in any second, and everything shown, on every
/// light at either pace**: across sixty seeded trains of thirty seconds
/// each, ticked at 120 Hz, each fed to one of the nine in turn, no closed
/// one-second window holds more than six changes (three pairs of opposing
/// changes, WCAG 2.2's count), and under Reduce Motion no closed two-second
/// window more than two, so no second more than one; every event has its
/// light lit within the dwell and a tick of it; a lit light goes dark only
/// once the last event's hold is over; and it is lit only within the hold
/// plus the dwell and a tick of an event. The control: a light that
/// flashes 50 ms for each event, the Linux tty trigger's blink, makes more
/// than six on the same trains, so the count can fail.
#[test]
fn the_lights_flash_at_most_three_times_a_second_and_show_everything() {
    let len = Duration::from_secs(30);
    let slack = TICK + Duration::from_nanos(1);
    let mut naive_worst = 0;
    for seed in 1..=60_u64 {
        let events = train(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), len);
        let led = Led::ALL[usize::try_from(seed).unwrap() % Led::ALL.len()];
        for pace in [Pace::Full, Pace::Steady] {
            let timing = pace.timing();
            let seen = run(led, &events, len, pace);
            let most = most_changes(&seen, Duration::from_secs(1));
            assert!(most <= 6, "seed {seed} {led:?} {pace:?}: {most}");
            if pace == Pace::Steady {
                let two = most_changes(&seen, Duration::from_secs(2));
                assert!(two <= 2, "seed {seed} {led:?}: {two} in two seconds");
            }
            for &b in events.iter().filter(|&&b| b + timing.dwell + slack < len) {
                assert!(
                    seen.iter().any(|&(at, l)| l == Light::Lit
                        && at >= b
                        && at <= b + timing.dwell + slack)
                        || seen
                            .iter()
                            .any(|&(at, l)| l == Light::Lit && at <= b && at + TICK > b),
                    "seed {seed} {led:?} {pace:?}: the event at {b:?} was never shown"
                );
            }
            for w in seen.windows(2) {
                let ((_, was), (at, l)) = (w[0], w[1]);
                let held = events
                    .iter()
                    .any(|&b| b + TICK <= at && at + TICK < b + timing.hold);
                assert!(
                    !(was == Light::Lit && l == Light::Dark && held),
                    "seed {seed} {led:?} {pace:?}: dark at {at:?} inside a hold"
                );
            }
            for &(at, l) in &seen {
                let quiet = !events
                    .iter()
                    .any(|&b| b <= at && at < b + timing.hold + timing.dwell + slack);
                assert!(
                    l == Light::Dark || !quiet,
                    "seed {seed} {led:?} {pace:?}: still lit at {at:?}"
                );
            }
        }
        // The control: 50 ms lit for every event, as the kernel blinks.
        let mut i = 0;
        let mut until = Duration::ZERO;
        let mut now = Duration::ZERO;
        let mut naive = Vec::new();
        while now < len {
            while i < events.len() && events[i] <= now {
                until = events[i] + Duration::from_millis(50);
                i += 1;
            }
            naive.push((now, if now < until { Light::Lit } else { Light::Dark }));
            now += TICK;
        }
        naive_worst = naive_worst.max(most_changes(&naive, Duration::from_secs(1)));
    }
    assert!(naive_worst > 6, "the control flashed {naive_worst} times");
}

/// **A state that toggles at a frame rate is one steady light**: SYN fed
/// on every tick a program holds synchronized output open, which it does
/// for half of each 60 Hz frame for five seconds, is lit from the first
/// tick to the end and goes dark once; a state seen on every tick, OH's
/// job in front, does the same. The control: the same toggling drawn as
/// the state itself changes over a hundred times a second.
#[test]
fn a_state_that_toggles_fast_is_one_steady_light() {
    let mut a = Activity::new();
    let mut raw = Vec::new();
    let mut syn = Vec::new();
    let mut oh = Vec::new();
    let mut now = Duration::ZERO;
    let frame = Duration::from_micros(16_667);
    let len = Duration::from_secs(6);
    while now < len {
        let open = now < Duration::from_secs(5)
            && (now.as_nanos() % frame.as_nanos()) * 2 < frame.as_nanos();
        if open {
            a.seen(Led::Syn, t(now));
        }
        if now < Duration::from_secs(5) {
            a.seen(Led::Oh, t(now));
        }
        let s = a.step(t(now), Pace::Full);
        raw.push((now, if open { Light::Lit } else { Light::Dark }));
        syn.push((now, s.syn));
        oh.push((now, s.oh));
        now += Duration::from_nanos(4_166_667);
    }
    let changes = |v: &[(Duration, Light)]| v.windows(2).filter(|w| w[0].1 != w[1].1).count();
    assert_eq!((syn[0].1, oh[0].1), (Light::Lit, Light::Lit), "lit at once");
    assert_eq!(changes(&syn), 1, "off once it stops");
    assert_eq!(changes(&oh), 1);
    assert!(
        syn.iter()
            .filter(|(at, _)| *at > Duration::from_millis(250) && *at < Duration::from_secs(5))
            .all(|&(_, l)| l == Light::Lit),
        "lit throughout"
    );
    assert!(
        most_changes(&raw, Duration::from_secs(1)) > 100,
        "the control"
    );
}

/// **Reduce Motion: one steady light while the activity lasts**: keys
/// three a second for five seconds -- a light that flashes at full motion,
/// several times -- keep SD lit, steadily, under the steady pace from the
/// first key to two seconds after the last, dark once after it.
#[test]
fn reduce_motion_holds_a_light_steady_while_the_activity_lasts() {
    let keys: Vec<Duration> = (0..15).map(|k| Duration::from_millis(k * 333)).collect();
    let len = Duration::from_secs(10);
    let full = run(Led::Sd, &keys, len, Pace::Full);
    let steady = run(Led::Sd, &keys, len, Pace::Steady);
    let changes = |v: &[(Duration, Light)]| v.windows(2).filter(|w| w[0].1 != w[1].1).count();
    assert!(
        changes(&full) > 6,
        "the control: {} at full",
        changes(&full)
    );
    assert_eq!(steady[0].1, Light::Lit, "lit at the first key");
    assert_eq!(changes(&steady), 1, "dark once, two seconds after the last");
    let last = *keys.last().unwrap();
    for &(at, l) in &steady {
        let want = match at < last + Duration::from_secs(2) {
            true => Light::Lit,
            false => Light::Dark,
        };
        if at + TICK < last + Duration::from_secs(2) || at > last + Duration::from_secs(2) + TICK {
            assert_eq!(l, want, "at {at:?}");
        }
    }
}

/// **A refused read shows the ring, a seen thing wins over it, and it
/// fades like a light**: refused on every tick, CD is unread after the
/// first; seen as well, it is lit; when both stop, it goes dark once the
/// hold and dwell are over; and the ring obeys the same dwell, so a light
/// fed seeded trains whose events alternate between seen and refused
/// changes no more than six times in any second.
#[test]
fn a_refused_read_shows_a_ring_and_keeps_the_clock() {
    let mut a = Activity::new();
    let ms = |m: u64| t(Duration::from_millis(m));
    a.refused(Led::Cd, ms(0));
    assert_eq!(a.step(ms(0), Pace::Full).cd, Light::Unread);
    for m in (8..400).step_by(8) {
        a.refused(Led::Cd, ms(m));
        a.seen(Led::Cd, ms(m));
        a.step(ms(m), Pace::Full);
    }
    assert_eq!(a.shown().cd, Light::Lit, "seen wins");
    assert_eq!(a.step(ms(1000), Pace::Full).cd, Light::Dark, "faded");
    let len = Duration::from_secs(20);
    for seed in 1..=20_u64 {
        let events = train(seed.wrapping_mul(0x2545_F491_4F6C_DD1D), len);
        let mut b = Activity::new();
        let (mut i, mut now, mut seen) = (0, Duration::ZERO, Vec::new());
        let mut any_ring = false;
        while now < len {
            while i < events.len() && events[i] <= now {
                match i % 2 {
                    0 => b.seen(Led::Cd, t(events[i])),
                    _ => b.refused(Led::Cd, t(events[i])),
                }
                i += 1;
            }
            let l = b.step(t(now), Pace::Full).cd;
            any_ring |= l == Light::Unread;
            seen.push((now, l));
            now += TICK;
        }
        assert!(any_ring, "seed {seed}: the control, a ring shown");
        let most = most_changes(&seen, Duration::from_secs(1));
        assert!(most <= 6, "seed {seed}: {most}");
    }
}

/// **HS's read is one as long as the tty hands back**: Darwin's 1024 and
/// Linux's 4095, each on its own system, light it, one byte fewer does not.
#[test]
fn a_read_at_the_ceiling_is_full() {
    let ceiling = match cfg!(target_os = "linux") {
        true => 4095,
        false => 1024,
    };
    assert_eq!(READ_CEILING, ceiling);
    assert!(full(&vec![0; ceiling]));
    assert!(full(&[0; 4096]));
    assert!(!full(&vec![0; ceiling - 1]));
    assert!(!full(&[]));
}

/// **A cache of one thing a light, and one a state, hands back its own**.
#[test]
fn one_thing_a_light_and_a_state() {
    let by = ByLed::of(|l| l);
    for l in Led::ALL {
        assert_eq!(*by.get(l), l);
    }
    assert_eq!(by.iter().map(|(l, _)| l).collect::<Vec<_>>(), Led::ALL);
    let st = ByLight::of(|s| s);
    for s in Light::ALL {
        assert_eq!(*st.get(s), s);
    }
}
