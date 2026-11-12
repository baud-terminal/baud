//! The activity lights: the pixels the title bar's panel shows
//! (`lights::draw_panel_led` and `lights::draw_panel_plate`, the pixel
//! oracle for a strip no GPU path draws -- each light is an image in a view
//! of its own over the plate's view, not part of the grid's frame) and the
//! lights' own line draws (`lights::draw_led`, `lights::draw` and
//! `lights::draw_plate`, here at a 28-point strip's height as well as the
//! line's), every lamp one of
//! the theme's sixteen palette entries exactly, and the clock that lights
//! them (`Activity`), held to WCAG 2.2's three flashes a second, to showing
//! everything it is told, and under Reduce Motion to one steady light
//! while the activity lasts.

mod support;

use baud_domain::{Chan, HostTime};
use baud_domain::{Cols, Rows};
use baud_font::{Face, Px};
use baud_render::lights::{
    DOT, GAP, HALO, HALO_GAP, LIT_CONTRAST, Level, PAIR, PLATE_PAD, PLATE_RADIUS_TITLE,
    PLATE_TITLE, TEXT, TEXT_CONTRAST, draw, draw_led, draw_panel_led, draw_panel_plate, draw_plate,
    full, layout, panel_layout, panel_letters, panel_margin, ring_entry, width,
};
use baud_render::{
    AREA_LIMIT, Activity, Ansi, Argb, Backdrop, Blend, ByLed, ByLight, DEFAULT_GLOW, Glow, Half,
    Halo, Led, Light, LightsColours, LightsImage, LightsLook, PIXELS_A_POINT, Pace, PxSize,
    READ_CEILING, Renderer, Shown, Stack, Theme, contrast_over,
};
use core::time::Duration;
use support::font;

/// The title strip's height in points, as the window's is.
const STRIP: f32 = 28.0;

/// The built-in themes, and xterm's sixteen on a black and on a white
/// page, the theme a palette file that names only them gives.
fn themes() -> Vec<(&'static str, Theme)> {
    let xterm = |fg: u32, bg: u32| {
        let mut t = Theme::dark();
        t.palette = baud_render::xterm_palette();
        (t.fg, t.bg) = (Argb(fg), Argb(bg));
        t
    };
    vec![
        ("dark", Theme::dark()),
        ("light", Theme::light()),
        ("xterm on black", xterm(0xFF_FF_FF_FF, 0xFF_00_00_00)),
        ("xterm on white", xterm(0xFF_00_00_00, 0xFF_FF_FF_FF)),
    ]
}

fn look_of(theme: &Theme, set: LightsColours, glow: Glow, halo: Halo) -> LightsLook {
    LightsLook::of(&theme.palette, theme.fg, theme.bg, set, glow, halo)
}

/// The default look: the dark theme in the meaning set, glowing.
fn look() -> LightsLook {
    look_of(
        &Theme::dark(),
        LightsColours::Meaning,
        Glow::Full,
        Halo::default(),
    )
}

/// Where `a` sits in the palette's sixteen.
fn index(a: Ansi) -> usize {
    Ansi::ALL.iter().position(|&x| x == a).unwrap()
}

/// The sixteen entries of `theme`, opaque.
fn sixteen(theme: &Theme) -> Vec<Argb> {
    Ansi::ALL.iter().map(|a| a.of(&theme.palette)).collect()
}

/// WCAG 2.2's contrast ratio of two opaque colours.
fn ratio(a: Argb, b: Argb) -> f64 {
    contrast_over(a, b, Chan::from_u8(0xFF), Backdrop::any())
}

/// A straight-alpha `src` over `dst`, as a view composites its picture
/// over what is under it.
fn over(dst: Argb, src: Argb) -> Argb {
    let sa = f64::from(src.a().get()) / 255.0;
    let da = f64::from(dst.a().get()) / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return Argb(0);
    }
    let ch = |s: Chan, d: Chan| {
        let v = (f64::from(s.get()) * sa + f64::from(d.get()) * da * (1.0 - sa)) / oa;
        Chan::from_unit((v / 255.0) as f32)
    };
    Argb::rgb(
        ch(src.r(), dst.r()),
        ch(src.g(), dst.g()),
        ch(src.b(), dst.b()),
    )
    .with_alpha(Chan::from_unit(oa as f32))
}

/// A picture as rows of its pixels' slice.
fn at_xy(img: &LightsImage, x: u32, y: u32) -> Argb {
    img.pixels[usize::try_from(y * img.width.value() + x).unwrap()]
}

/// **The title strip as the window shows it**: the plate's picture, the
/// strip's width and its pad at each end and [`PLATE_TITLE`] tall, centred
/// in the strip, and each light's picture at its slot over it; the canvas
/// is the plate's width and the strip's height, the lights `pad` in.
fn title(face: &Face<'_>, look: &LightsLook, shown: Shown, scale: f32) -> (LightsImage, u32) {
    let (at, w) = layout(face, scale).unwrap();
    let px = |pt: f32| u32::try_from((pt * scale).round() as i64).unwrap();
    let (pad, h) = (px(PLATE_PAD), px(STRIP));
    let cw = w.value() + 2 * pad;
    let mut canvas = vec![Argb(0); usize::try_from(cw * h).unwrap()];
    let mut plate = LightsImage::new();
    let ph = px(PLATE_TITLE);
    draw_plate(
        look,
        Px::of(cw),
        Px::of(ph),
        PLATE_RADIUS_TITLE * scale,
        &mut plate,
    )
    .unwrap();
    let top = (h - ph) / 2;
    for y in 0..ph {
        for x in 0..cw {
            let i = usize::try_from((top + y) * cw + x).unwrap();
            canvas[i] = over(canvas[i], at_xy(&plate, x, y));
        }
    }
    for (led, &light) in shown.iter() {
        let mut one = LightsImage::new();
        draw_led(face, look, led, light, scale, STRIP, &mut one).unwrap();
        let x0 = pad + at.get(led).start.value();
        for y in 0..one.height.value() {
            for x in 0..one.width.value() {
                let i = usize::try_from(y * cw + x0 + x).unwrap();
                canvas[i] = over(canvas[i], at_xy(&one, x, y));
            }
        }
    }
    let img = LightsImage {
        width: Px::of(cw),
        height: Px::of(h),
        pixels: canvas,
    };
    (img, pad)
}

/// **A light's lamp pixels at full coverage**, in its own picture: where
/// its dot, its half-size dot or its ring covers the pixel wholly, found
/// by drawing it with no letters and no glow, and the colour each must be.
fn lamp_mask(
    face: &Face<'_>,
    look: &LightsLook,
    led: Led,
    light: Light,
    scale: f32,
    height: f32,
) -> (Vec<(u32, u32)>, Argb) {
    let bare = LightsLook {
        label: Argb(0),
        glow: Glow::Dimmed,
        ..*look
    };
    let mut img = LightsImage::new();
    draw_led(face, &bare, led, light, scale, height, &mut img).unwrap();
    let want = match light {
        Light::Lit => *look.lit.get(led),
        Light::Dark => look.unlit,
        Light::Unread => look.ring,
    };
    let w = img.width.value();
    let pts = (0..img.pixels.len())
        .filter(|&i| img.pixels[i].a().get() == 0xFF)
        .map(|i| {
            let i = u32::try_from(i).unwrap();
            (i % w, i / w)
        })
        .collect();
    (pts, want)
}

/// What a scan of the full-coverage lamp pixels found: every pixel it
/// read, those that were not the colour they must be, and those that were
/// none of the theme's sixteen.
#[derive(Default, Debug)]
struct Scan {
    read: Vec<Argb>,
    wrong: Vec<Argb>,
    foreign: Vec<Argb>,
}

/// **Scan the title strip's lamps**: every full-coverage lamp pixel of
/// every light at `shown`, `shift` pixels right of where it is, against the
/// colour it must be and against the theme's sixteen.
fn scan_title(
    face: &Face<'_>,
    theme: &Theme,
    look: &LightsLook,
    shown: Shown,
    scale: f32,
    shift: Px,
) -> Scan {
    let (img, pad) = title(face, look, shown, scale);
    let (at, _) = layout(face, scale).unwrap();
    let ours = sixteen(theme);
    let mut s = Scan::default();
    for (led, &light) in shown.iter() {
        let (pts, want) = lamp_mask(face, look, led, light, scale, STRIP);
        let x0 = pad + at.get(led).start.value() + shift.value();
        for (x, y) in pts {
            let p = at_xy(&img, x0 + x, y);
            s.read.push(p);
            if p != want {
                s.wrong.push(p);
            }
            if !ours.contains(&p) {
                s.foreign.push(p);
            }
        }
    }
    s
}

/// **Every full-coverage lamp pixel of the title strip is exactly the
/// palette entry its set gives it, and one of the theme's sixteen**: for
/// every set, the built-in themes and xterm's sixteen on black and on
/// white, every state, both glows and the default and strongest halo, at
/// 1x and 2x, on the plate as the window shows it. The controls: the same
/// scan with its mask moved 40 pixels finds pixels that are not the lamp's
/// colour, and a look whose lit colours are lifted off the palette -- the
/// lift this replaced -- finds pixels that are none of the sixteen, so the
/// scan can fire. Perturbation: draw a lamp at 254 of 255 coverage, or mix
/// the unlit lamp into the plate, and this goes red.
#[test]
fn every_lamp_pixel_on_the_title_strip_is_its_palette_entry() {
    let bold = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Bold.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let face = Face::parse(&bold).unwrap();
    let mut read = 0;
    for (name, theme) in themes() {
        for set in LightsColours::ALL {
            for glow in [Glow::Full, Glow::Dimmed] {
                for halo in [Halo::default(), Halo::of(1.0)] {
                    let look = look_of(&theme, set, glow, halo);
                    for light in Light::ALL {
                        for scale in [1.0_f32, 2.0] {
                            let s = scan_title(
                                &face,
                                &theme,
                                &look,
                                Shown::all(light),
                                scale,
                                Px::of(0),
                            );
                            assert_eq!(
                                (&s.wrong[..], &s.foreign[..]),
                                (&[][..], &[][..]),
                                "{name} {set:?} {glow:?} {light:?} {scale}x"
                            );
                            read += s.read.len();
                        }
                    }
                }
            }
            // The controls, at 2x lit.
            let look = look_of(&theme, set, Glow::Full, Halo::default());
            let moved = scan_title(
                &face,
                &theme,
                &look,
                Shown::all(Light::Lit),
                2.0,
                Px::of(40),
            );
            assert!(
                !moved.wrong.is_empty(),
                "{name} {set:?}: the moved mask fires"
            );
        }
        let lifted = LightsLook {
            lit: ByLed::of(|_| theme.palette[2].mix(Argb(0xFF_FF_FF_FF), Chan::from_u8(200))),
            ..look_of(&theme, LightsColours::Meaning, Glow::Full, Halo::default())
        };
        let s = scan_title(
            &face,
            &theme,
            &lifted,
            Shown::all(Light::Lit),
            2.0,
            Px::of(0),
        );
        assert!(!s.foreign.is_empty(), "{name}: the lifted control fires");
    }
    assert!(read > 50_000, "the scan read the lamps, {read}");
}

/// **The three sets are the tables they say, and the default is
/// meaning**: each light's entry in each set on each half, pinned to the
/// numbers the sets are defined by; the config's names; and a renderer
/// that was told nothing draws the meaning set, on the dark theme its
/// dark half and on the light theme its light one. Perturbation: default
/// to `Iec`, or give the light half the dark half's yellow, and this goes
/// red. CS is green, entry 2, in the meaning and iec sets: lit, it says
/// the line is clear, IEC 60073's normal condition, and no set lights a
/// lamp red for the state a session is in nearly all of the time but the
/// modem set, whose every lamp is red.
#[test]
fn the_sets_are_their_tables_and_meaning_is_the_default() {
    let nums = |set: LightsColours, half| Led::ALL.map(|l| index(*set.entries(half).get(l)));
    use LightsColours::{Iec, Meaning, Modem};
    assert_eq!(nums(Meaning, Half::Dark), [10, 11, 4, 6, 2, 3, 2, 5, 13]);
    assert_eq!(nums(Meaning, Half::Light), [10, 6, 4, 12, 2, 14, 2, 5, 13]);
    for half in [Half::Dark, Half::Light] {
        assert_eq!(nums(Iec, half), [2, 7, 2, 7, 7, 7, 2, 6, 6]);
        assert_eq!(nums(Modem, half), [1; 9]);
    }
    assert_eq!(LightsColours::default(), Meaning);
    assert_eq!(
        LightsColours::ALL.map(LightsColours::name),
        ["meaning", "iec", "modem"]
    );
    for set in LightsColours::ALL {
        assert_eq!(LightsColours::named(set.name()), Some(set));
    }
    assert_eq!(LightsColours::named("orange"), None);
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    for (theme, half) in [(Theme::dark(), Half::Dark), (Theme::light(), Half::Light)] {
        assert_eq!(Half::of(theme.fg, theme.bg), half);
        let r = renderer(&faces, theme.clone());
        let lit = r.lights_look(Glow::Full).lit;
        let want = ByLed::of(|l| Meaning.entries(half).get(l).of(&theme.palette));
        assert_eq!(lit, want, "{half:?}");
    }
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], theme: Theme) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(28.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(6),
        Rows::of_index(2),
    )
    .unwrap()
}

/// **The unlit lamp is entry 8 in every set, on every theme**: lit and
/// unlit already differ by size, so colour is not what tells them apart
/// (WCAG 2.2's SC 1.4.1, `[[source: wcag22]]`), and one entry for every
/// dark lamp keeps it visible on the plate wherever 8 is. The modem set's
/// on the dark theme had been entry 0, `#000000`, 1.10:1 on the `#101010`
/// plate, invisible; it is 8 now, 4.75:1. The ring is the entry the
/// foreground is, else 7. Perturbation: take entry 0 where the lit lamps
/// are quieter than 8, the old rule, and the dark modem set goes red.
#[test]
fn the_unlit_lamp_is_entry_eight_in_every_set() {
    let dark = Theme::dark();
    for (name, theme) in themes() {
        for set in LightsColours::ALL {
            let look = look_of(&theme, set, Glow::Full, Halo::default());
            assert_eq!(look.unlit, theme.palette[8], "{name} {set:?}");
        }
    }
    let modem = look_of(&dark, LightsColours::Modem, Glow::Full, Halo::default());
    let seen = contrast_over(
        modem.unlit,
        modem.plate,
        Chan::from_u8(0xFF),
        Backdrop::any(),
    );
    assert!(
        (seen - 4.75).abs() < 0.005,
        "the dark modem set's unlit lamp reads {seen:.2}:1 on its plate"
    );
    assert_eq!(
        ring_entry(&dark.palette, dark.fg),
        Ansi::White,
        "#D8D8D8 is no entry"
    );
    let mut fg = Theme::dark();
    fg.fg = fg.palette[15];
    assert_eq!(ring_entry(&fg.palette, fg.fg), Ansi::BrightWhite);
}

/// The widest run of pixels at least half covered, in the row through a
/// light's centre, from its picture drawn with no letters and no glow.
fn dot_width(face: &Face<'_>, light: Light, scale: f32) -> u32 {
    let bare = LightsLook {
        label: Argb(0),
        glow: Glow::Dimmed,
        ..look()
    };
    let mut img = LightsImage::new();
    draw_led(face, &bare, Led::Rd, light, scale, STRIP, &mut img).unwrap();
    let y = img.height.value() / 2;
    (0..img.width.value())
        .filter(|&x| at_xy(&img, x, y).a().get() >= 0x80)
        .count()
        .try_into()
        .unwrap()
}

/// **An unlit lamp is a dot half the lit one's diameter**, so lit and
/// unlit differ by size as well as colour and the state reads without
/// it: WCAG 2.2's SC 1.4.1, "Color is not used as the only visual means
/// of conveying information ... indicating an action, prompting a
/// response, or distinguishing a visual element" (`[[source: wcag22]]`).
/// At 1x, 2x and 3x the lit dot is the 7-point light across and the unlit
/// one half of it, to a pixel; both centred on the same point.
/// Perturbation: draw the unlit lamp at the lit one's radius and this goes
/// red.
#[test]
fn the_unlit_lamp_is_half_the_lit_ones_diameter() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    for scale in [1.0_f32, 2.0, 3.0] {
        let (lit, dark) = (
            dot_width(&face, Light::Lit, scale),
            dot_width(&face, Light::Dark, scale),
        );
        let d = DOT * scale;
        assert!((lit as f32 - d).abs() <= 1.0, "{scale}x lit {lit} of {d}");
        assert!(
            (dark as f32 - d / 2.0).abs() <= 1.0,
            "{scale}x unlit {dark} of {}",
            d / 2.0
        );
        assert!((2 * dark).abs_diff(lit) <= 2, "{scale}x: {dark} of {lit}");
    }
}

/// **The plate is the theme's background, exactly, and the lamps read on
/// it**: every pixel of the title strip's plate that no light's picture
/// touches is the theme's background, opaque, in every set; every lit
/// lamp reads at 3:1 against it ([`LIT_CONTRAST`], SC 1.4.11) and the
/// letters at 4.5:1 ([`TEXT_CONTRAST`], SC 1.4.3) on the built-in themes.
/// The letters are the theme's foreground, which reads there as it is.
/// Perturbation: fill the plate with the page at its alpha, or lift it,
/// and this goes red.
#[test]
fn the_plate_is_the_background_and_the_lamps_read_on_it() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for set in LightsColours::ALL {
            let look = look_of(&theme, set, Glow::Full, Halo::default());
            assert_eq!(look.plate, theme.bg.with_alpha(Chan::from_u8(0xFF)));
            assert_eq!(look.label, theme.fg, "{name}: the foreground as it is");
            let (img, pad) = title(&face, &look, Shown::all(Light::Lit), 2.0);
            let mut bare = img.clone();
            bare.pixels.fill(Argb(0));
            let (blank, _) = title(
                &face,
                &LightsLook {
                    lit: ByLed::of(|_| Argb(0)),
                    unlit: Argb(0),
                    ring: Argb(0),
                    label: Argb(0),
                    ..look
                },
                Shown::all(Light::Lit),
                2.0,
            );
            let mut plate = 0;
            for (i, (&p, &b)) in img.pixels.iter().zip(&blank.pixels).enumerate() {
                if b.a().get() == 0xFF && p == b {
                    plate += 1;
                    assert_eq!(p, look.plate, "{name} {set:?}: pixel {i}");
                }
            }
            assert!(plate > 2000, "{name} {set:?}: the plate's inside, {plate}");
            let _ = pad;
            for led in Led::ALL {
                let c = ratio(*look.lit.get(led), look.plate);
                assert!(c >= LIT_CONTRAST, "{name} {set:?} {led:?}: {c:.2}");
            }
            let c = ratio(look.label, look.plate);
            assert!(c >= TEXT_CONTRAST, "{name}: letters {c:.2}");
        }
    }
}

/// **The glow's strength is 0 to 1, 0.4 by default, starting a point off
/// the dot**: `Halo::of` holds its strength to the range, 1 is a halo of
/// 0.8 where it starts and 0 none; at the strongest no pixel nearer the
/// dot's edge than [`HALO_GAP`] is touched, so the dot reads against the
/// plate as with no glow, and pixels past it are; at 0 the lit picture is
/// the picture under Dim Flashing Lights. Perturbation: start the halo at
/// the dot's edge and this goes red.
#[test]
fn the_glow_is_held_to_its_range_and_starts_off_the_dot() {
    assert_eq!(DEFAULT_GLOW, 0.4);
    assert_eq!(Halo::default(), Halo::of(0.4));
    assert_eq!(Halo::of(-1.0), Halo::of(0.0));
    assert_eq!(Halo::of(7.0), Halo::of(1.0));
    assert_eq!(Halo::of(1.0).alpha(), Chan::from_unit(0.8));
    assert_eq!(Halo::of(0.0).alpha().get(), 0);
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    let scale = 2.0;
    let draw_at = |halo: Halo, glow: Glow| {
        let l = LightsLook {
            label: Argb(0),
            glow,
            halo,
            ..look()
        };
        let mut img = LightsImage::new();
        draw_led(&face, &l, Led::Hs, Light::Lit, scale, STRIP, &mut img).unwrap();
        img
    };
    let strong = draw_at(Halo::of(1.0), Glow::Full);
    let none = draw_at(Halo::of(0.0), Glow::Full);
    assert_eq!(none, draw_at(Halo::of(1.0), Glow::Dimmed), "0 draws none");
    // The dot's centre: the letters' advance, the gap and half the light.
    let r = DOT * scale / 2.0;
    let cx = (0.6 * TEXT * 2.0 + GAP) * scale + r;
    let cy = STRIP * scale / 2.0;
    let (mut near, mut far) = (0, 0);
    for y in 0..strong.height.value() {
        for x in 0..strong.width.value() {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() - r;
            let (s, n) = (at_xy(&strong, x, y), at_xy(&none, x, y));
            if d > 1.0 && d < HALO_GAP * scale - 0.5 {
                assert_eq!(s, n, "({x}, {y}), {d:.2} off the dot: in the gap");
                near += 1;
            }
            if d > HALO_GAP * scale + 1.0 && d < HALO * scale - 1.0 && s != n {
                far += 1;
            }
        }
    }
    assert!(
        near > 10 && far > 100,
        "the probes reached both, {near} {far}"
    );
}

/// **Dim Flashing Lights draws no glow, and the dot as it is**: under
/// [`Glow::Dimmed`] a lit light's picture is the one drawn with no halo,
/// whatever the strength, and its lamp pixels are the lit entry, as at
/// full glow; at full glow the default halo draws pixels the dimmed one
/// does not, the control. Perturbation: draw the halo whatever the glow
/// and this goes red.
#[test]
fn dim_flashing_lights_draws_no_glow_and_the_dot_unchanged() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    for (name, theme) in themes() {
        for set in LightsColours::ALL {
            for led in Led::ALL {
                let pic = |glow, halo| {
                    let mut img = LightsImage::new();
                    let l = look_of(&theme, set, glow, halo);
                    draw_led(&face, &l, led, Light::Lit, 2.0, STRIP, &mut img).unwrap();
                    img
                };
                let dimmed = pic(Glow::Dimmed, Halo::of(1.0));
                assert_eq!(
                    dimmed,
                    pic(Glow::Full, Halo::of(0.0)),
                    "{name} {set:?} {led:?}"
                );
                assert_ne!(dimmed, pic(Glow::Full, Halo::default()), "the control");
                let l = look_of(&theme, set, Glow::Dimmed, Halo::default());
                let (pts, want) = lamp_mask(&face, &l, led, Light::Lit, 2.0, STRIP);
                assert!(!pts.is_empty());
                for (x, y) in pts {
                    assert_eq!(at_xy(&dimmed, x, y), want, "{name} {set:?} {led:?}");
                }
            }
        }
    }
}

/// WCAG 2.2's relative luminance of an opaque colour.
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

/// WCAG 2.2's red flash between two opaque colours: either a saturated
/// red, and their CIE 1976 u'v' more than 0.2 apart (`[[source: wcag22]]`'s
/// definition), black taken at D65's white point, as `flash.rs` takes it.
fn red_flash(a: Argb, b: Argb) -> bool {
    let uv = |c: Argb| {
        let lin = |v: Chan| {
            let s = f64::from(v.get()) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (lin(c.r()), lin(c.g()), lin(c.b()));
        let x = 0.4124 * r + 0.3576 * g + 0.1805 * b;
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let z = 0.0193 * r + 0.1192 * g + 0.9505 * b;
        let d = x + 15.0 * y + 3.0 * z;
        if d <= 0.0 {
            (0.1978, 0.4683)
        } else {
            (4.0 * x / d, 9.0 * y / d)
        }
    };
    let ((ua, va), (ub, vb)) = (uv(a), uv(b));
    (red(a) || red(b)) && (ua - ub).hypot(va - vb) > 0.2
}

/// **The pixels that flash when every light changes at once**, in WCAG's
/// pixels: each pixel of the two pictures, as shown over the strip's
/// material (a pixel's alpha over black and over white), that is a
/// general flash -- 10% of relative luminance apart with the darker under
/// 0.80 -- or a red one, counted once, at `scale` device pixels a point
/// and [`PIXELS_A_POINT`] WCAG pixels a point.
fn flash_area(a: &LightsImage, b: &LightsImage, scale: f32) -> f32 {
    let black = Argb(0xFF_00_00_00);
    let white = Argb(0xFF_FF_FF_FF);
    let n = a
        .pixels
        .iter()
        .zip(&b.pixels)
        .filter(|&(&p, &q)| {
            [black, white].iter().any(|&g| {
                let (p, q) = (over(g, p), over(g, q));
                let (yp, yq) = (luminance(p), luminance(q));
                ((yp - yq).abs() >= 0.10 && yp.min(yq) < 0.80) || red_flash(p, q)
            })
        })
        .count();
    n as f32 / (scale * scale) * PIXELS_A_POINT * PIXELS_A_POINT
}

/// **Every light changing at once flashes under a quarter of WCAG's field,
/// so the lights meet 2.3.1 by the area clause, Dim Flashing Lights or
/// not**: "the combined area of flashes occurring concurrently occupies no
/// more than a total of .006 steradians within any 10 degree visual field
/// ... (25% of any 10 degree visual field on the screen)", the field
/// "a 341 x 256 pixel rectangle anywhere on the displayed screen area
/// when the content is viewed at 1024 x 768 pixels" (`[[source:
/// wcag22]]`), [`AREA_LIMIT`]. All nine lit against all nine dark on the
/// title strip as the window shows it, and each light's picture at the
/// line's height on the plate, at the strongest glow and the
/// default, in every set on the built-in themes and xterm's, and under Dim
/// Flashing Lights. A light is drawn in points, so the area does not grow
/// with the font. The control: the flash area is more than nothing, so the
/// probe fires. Perturbation: draw the plate in the lit colour and this
/// goes red.
#[test]
fn every_light_changing_at_once_flashes_under_the_area_clause() {
    let bold = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Bold.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let face = Face::parse(&bold).unwrap();
    let mut most: f32 = 0.0;
    for (name, theme) in themes() {
        for set in LightsColours::ALL {
            for (glow, halo) in [
                (Glow::Full, Halo::of(1.0)),
                (Glow::Full, Halo::default()),
                (Glow::Dimmed, Halo::of(1.0)),
            ] {
                let look = look_of(&theme, set, glow, halo);
                let (lit, _) = title(&face, &look, Shown::all(Light::Lit), 2.0);
                let (dark, _) = title(&face, &look, Shown::all(Light::Dark), 2.0);
                let area = flash_area(&lit, &dark, 2.0);
                // The line's: each light's picture at the line's height,
                // on the plate.
                let on_plate = |light| {
                    let mut all = LightsImage::new();
                    for led in Led::ALL {
                        let mut one = LightsImage::new();
                        let h = baud_render::lights::line_picture();
                        draw_led(&face, &look, led, light, 2.0, h, &mut one).unwrap();
                        all.pixels
                            .extend(one.pixels.iter().map(|&p| over(look.plate, p)));
                    }
                    all
                };
                let line = flash_area(&on_plate(Light::Lit), &on_plate(Light::Dark), 2.0);
                for (place, area) in [("title", area), ("line", line)] {
                    assert!(area > 0.0, "{name} {set:?} {place}: the control");
                    assert!(
                        area <= AREA_LIMIT,
                        "{name} {set:?} {glow:?} {place}: {area} of {AREA_LIMIT}"
                    );
                    most = most.max(area);
                }
            }
        }
    }
    assert!(most < AREA_LIMIT / 10.0, "the most, {most} WCAG pixels");
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

/// Whether any pixel in columns `from..to` and rows `rows` carries the
/// letters' colour.
fn ink(img: &LightsImage, label: Argb, from: f32, to: f32, rows: core::ops::Range<u32>) -> bool {
    (px(from)..px(to)).any(|x| {
        rows.clone().any(|y| {
            let p = img.pixels[usize::try_from(y * img.width.value() + x).unwrap()];
            p.a().get() > 0 && (p.r(), p.g(), p.b()) == (label.r(), label.g(), label.b())
        })
    })
}

/// **Each light shows its state in its own colour, and its letters sit to
/// its left**: at 1x and 2x, for each of the nine and each state, a lit
/// light's centre is its lit entry and a dark one's the unlit entry; an
/// unread one is a ring, its centre clear and its edge the ring's entry;
/// the letters put ink left of the light and none right of it; no light
/// has ink above its letters' caps, CS's no more than the rest, none of
/// the nine being inverted; the picture is its slot's width and the
/// strip's height, clear in its corner.
#[test]
fn each_light_is_drawn_in_its_state_with_its_letters_to_the_left() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    let look = look();
    for scale in [1.0_f32, 2.0] {
        let (slots, _) = layout(&face, scale).unwrap();
        for led in Led::ALL {
            for light in Light::ALL {
                let mut img = LightsImage::new();
                draw_led(&face, &look, led, light, scale, STRIP, &mut img).unwrap();
                assert_eq!(img.height.value(), px(STRIP * scale));
                assert_eq!(img.width, slots.get(led).width, "{led:?}");
                let (cx, cy) = centre(led, &img, scale);
                let p = at(&img, cx, cy);
                match light {
                    Light::Lit => assert_eq!(p, *look.lit.get(led), "{led:?}"),
                    Light::Dark => assert_eq!(p, look.unlit, "{led:?}"),
                    Light::Unread => {
                        assert_eq!(p.a().get(), 0, "{led:?} at {scale}x: a ring's centre");
                        let edge = at(&img, cx + (DOT / 2.0 - 0.6) * scale, cy);
                        assert!(
                            (edge.r(), edge.g(), edge.b())
                                == (look.ring.r(), look.ring.g(), look.ring.b())
                                && edge.a().get() > 0,
                            "{led:?} at {scale}x: the ring {edge:?}"
                        );
                    }
                }
                let left = cx - DOT * scale / 2.0;
                let all = 0..img.height.value();
                assert!(
                    ink(&img, look.label, 0.0, left, all.clone()),
                    "{led:?}'s letters"
                );
                assert!(
                    !ink(
                        &img,
                        look.label,
                        cx + DOT * scale / 2.0 + 1.0,
                        img.width.value() as f32,
                        all
                    ),
                    "{led:?}: no letters right of the light"
                );
                let caps_top = cy - 0.73 * TEXT * scale / 2.0;
                let bar = ink(&img, look.label, 0.0, left, 0..px(caps_top - 0.2 * scale));
                assert!(!bar, "{led:?} at {scale}x: ink above the caps");
                assert_eq!(at(&img, 0.0, 0.0).a().get(), 0, "the plate shows through");
            }
        }
    }
}

/// **The strip is its lights side by side**: the slots run left to right
/// in the 1988 Courier manual's order, a light's gap apart less the halo's
/// room and never overlapping, the last ending at the strip's width, which
/// `width` gives in points; and every pixel of the strip drawn at a state
/// is the pixel of that light's own picture at its slot, so the views
/// placed by the layout show the strip this file tests. Nine lights at 2x
/// are 522 pixels wide.
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
                assert!(
                    s.start.value() - end >= px((PAIR - HALO - 1.0) * scale),
                    "{led:?}"
                );
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
        draw(&face, &look(), shown, scale, STRIP, &mut strip).unwrap();
        assert_eq!(strip.width, w);
        for led in Led::ALL {
            let mut one = LightsImage::new();
            draw_led(&face, &look(), led, *shown.get(led), scale, STRIP, &mut one).unwrap();
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
    assert_eq!(w.value(), 522);
}

/// **The renderer draws the strip in the badge's face, in its own look**:
/// the same pixels as `draw` and `draw_led` over that face, the same
/// layout, and the look the theme and the set it was told give, so the
/// window shows what this file tests.
#[test]
fn the_renderer_draws_the_strip_the_oracle_holds() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut r = renderer(&faces, Theme::dark());
    r.set_lights_colours(LightsColours::Modem, Halo::of(1.0));
    let look = r.lights_look(Glow::Full);
    assert_eq!(
        look,
        look_of(
            &Theme::dark(),
            LightsColours::Modem,
            Glow::Full,
            Halo::of(1.0)
        )
    );
    let shown = ByLed {
        rd: Light::Lit,
        cd: Light::Unread,
        ..Shown::DARK
    };
    let (mut a, mut b) = (LightsImage::new(), LightsImage::new());
    r.activity_lights(&look, shown, 2.0, STRIP, &mut a).unwrap();
    draw(&faces[0], &look, shown, 2.0, STRIP, &mut b).unwrap();
    assert_eq!(a, b);
    for led in Led::ALL {
        r.activity_light(&look, led, Light::Lit, 2.0, STRIP, &mut a)
            .unwrap();
        draw_led(&faces[0], &look, led, Light::Lit, 2.0, STRIP, &mut b).unwrap();
        assert_eq!(a, b, "{led:?}");
    }
    assert_eq!(r.activity_lights_width(), width(&faces[0]));
    assert_eq!(r.activity_lights_layout(2.0), layout(&faces[0], 2.0));
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

/// **What `led` shows while its thing holds, and at rest**: lit and dark,
/// but CS the other way round -- lit while the line is clear, dark for the
/// hold after bytes were owed the program that the kernel had not taken,
/// as a modem's CS is on until the modem lowers CTS.
fn seen_rest(led: Led) -> (Light, Light) {
    match led {
        Led::Cs => (Light::Dark, Light::Lit),
        _ => (Light::Lit, Light::Dark),
    }
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
/// light lit within the dwell and a tick of it -- CS dark, its sense being
/// the other way round ([`seen_rest`]) -- a lit light goes dark only once
/// the last event's hold is over, and it is lit only within the hold plus
/// the dwell and a tick of an event. The control: a light that
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
            let (shows, rest) = seen_rest(led);
            let most = most_changes(&seen, Duration::from_secs(1));
            assert!(most <= 6, "seed {seed} {led:?} {pace:?}: {most}");
            if pace == Pace::Steady {
                let two = most_changes(&seen, Duration::from_secs(2));
                assert!(two <= 2, "seed {seed} {led:?}: {two} in two seconds");
            }
            for &b in events.iter().filter(|&&b| b + timing.dwell + slack < len) {
                assert!(
                    seen.iter()
                        .any(|&(at, l)| l == shows && at >= b && at <= b + timing.dwell + slack)
                        || seen
                            .iter()
                            .any(|&(at, l)| l == shows && at <= b && at + TICK > b),
                    "seed {seed} {led:?} {pace:?}: the event at {b:?} was never shown"
                );
            }
            for w in seen.windows(2) {
                let ((_, was), (at, l)) = (w[0], w[1]);
                let held = events
                    .iter()
                    .any(|&b| b + TICK <= at && at + TICK < b + timing.hold);
                assert!(
                    !(was == shows && l == rest && held),
                    "seed {seed} {led:?} {pace:?}: dark at {at:?} inside a hold"
                );
            }
            for &(at, l) in &seen {
                let quiet = !events
                    .iter()
                    .any(|&b| b <= at && at < b + timing.hold + timing.dwell + slack);
                assert!(
                    l == rest || !quiet,
                    "seed {seed} {led:?} {pace:?}: still shown at {at:?}"
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

/// **CS is lit while the line is clear, as a modem's is**: from the first
/// step, before anything is seen, CS is lit and the rest dark, and at rest
/// no light owes a change, so a driver with no display link sleeps; bytes
/// owed the program at a tick darken it at that step and keep it dark
/// while they are owed, for the hold after the last of them, the wait
/// bounded by that hold's end, and it lights again once the hold and the
/// dwell are over and owes nothing more. Owed
/// for less than a tick, it still goes dark, for the hold. Under Reduce
/// Motion the same, two seconds each. The control: SD fed the same is lit
/// for the same span, the sense CS has the other way round.
#[test]
fn cs_is_lit_while_the_line_is_clear() {
    let ms = |m: u64| t(Duration::from_millis(m));
    let mut a = Activity::new();
    let rest = a.step(ms(0), Pace::Full);
    for led in Led::ALL {
        let want = match led {
            Led::Cs => Light::Lit,
            _ => Light::Dark,
        };
        assert_eq!(*rest.get(led), want, "{led:?} at rest");
    }
    assert_eq!(
        a.next_change(ms(0), Pace::Full),
        None,
        "nothing to wake for"
    );
    // Owed from 100 to 500 ms, seen at each 8 ms tick.
    let mut cs = Vec::new();
    let mut sd = Vec::new();
    let mut b = Activity::new();
    for m in (0..2_000).step_by(8) {
        if (100..=500).contains(&m) {
            a.seen(Led::Cs, ms(m));
            b.seen(Led::Sd, ms(m));
        }
        cs.push((m, a.step(ms(m), Pace::Full).cs));
        sd.push((m, b.step(ms(m), Pace::Full).sd));
        if m == 504 {
            assert_eq!(
                a.next_change(ms(m), Pace::Full),
                Some(ms(696)),
                "dark, it wakes at the hold's end to light"
            );
        }
    }
    for &(m, l) in &cs {
        let want = match (104..696).contains(&m) {
            true => Light::Dark,
            false => Light::Lit,
        };
        assert_eq!(l, want, "CS at {m} ms");
    }
    for (&(m, c), &(_, s)) in cs.iter().zip(&sd) {
        assert_eq!(
            c == Light::Dark,
            s == Light::Lit,
            "SD the other way at {m} ms"
        );
    }
    assert_eq!(
        a.next_change(ms(2_000), Pace::Full),
        None,
        "clear again, at rest"
    );
    // Owed for under a tick: dark at the step after, for the hold.
    let mut a = Activity::new();
    a.step(ms(0), Pace::Full);
    a.seen(Led::Cs, ms(3));
    assert_eq!(
        a.step(ms(8), Pace::Full).cs,
        Light::Dark,
        "a refusal between ticks"
    );
    assert_eq!(a.step(ms(200), Pace::Full).cs, Light::Dark, "the hold");
    assert_eq!(a.step(ms(208), Pace::Full).cs, Light::Lit, "clear");
    // Reduce Motion: two seconds dark.
    let mut a = Activity::new();
    a.seen(Led::Cs, ms(0));
    assert_eq!(a.step(ms(0), Pace::Steady).cs, Light::Dark);
    assert_eq!(a.step(ms(1_999), Pace::Steady).cs, Light::Dark);
    assert_eq!(a.step(ms(2_000), Pace::Steady).cs, Light::Lit);
}

/// **The nine lamps of the panel in a picture of their own**, side by side
/// over `look`'s plate, opaque, at `scale`, the strip's height: as the
/// window composes the views.
fn panel_row(face: &Face<'_>, look: &LightsLook, shown: Shown, scale: f32) -> LightsImage {
    let (at, w) = panel_layout(face, scale).unwrap();
    let h = u32::try_from((STRIP * scale).round() as i64).unwrap();
    let mut out = LightsImage {
        width: w,
        height: Px::of(h),
        pixels: vec![look.plate; usize::try_from(w.value() * h).unwrap()],
    };
    for (led, &light) in shown.iter() {
        let mut one = LightsImage::new();
        draw_panel_led(face, look, led, light, scale, STRIP, &mut one).unwrap();
        let x0 = at.get(led).start.value();
        for y in 0..h {
            for x in 0..one.width.value() {
                let d = usize::try_from(y * w.value() + x0 + x).unwrap();
                out.pixels[d] = over(out.pixels[d], at_xy(&one, x, y));
            }
        }
    }
    out
}

/// A lamp's centre on the panel, in the picture's pixels: as far as the
/// picture says, read from its lit dot with nothing else drawn.
fn panel_centre(face: &Face<'_>, look: &LightsLook, led: Led, scale: f32) -> (f32, f32) {
    let bare = LightsLook {
        label: Argb(0),
        socket: Argb(0),
        glow: Glow::Dimmed,
        ..*look
    };
    let mut img = LightsImage::new();
    draw_panel_led(face, &bare, led, Light::Lit, scale, STRIP, &mut img).unwrap();
    let w = img.width.value();
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for y in 0..img.height.value() {
        for x in 0..w {
            let a = f32::from(at_xy(&img, x, y).a().get()) / 255.0;
            sx += (x as f32 + 0.5) * a;
            sy += (y as f32 + 0.5) * a;
            n += a;
        }
    }
    (sx / n, sy / n)
}

/// **The renderer draws the panel the oracle holds**: the same pixels as
/// `draw_panel_led` over the badge's face, and the same layout and letters.
#[test]
fn the_renderer_draws_the_panel_the_oracle_holds() {
    let bold = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Bold.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let faces = [Face::parse(&bold).unwrap()];
    let r = renderer(&faces, Theme::dark());
    let look = r.lights_look(Glow::Full);
    let (mut a, mut b) = (LightsImage::new(), LightsImage::new());
    for led in Led::ALL {
        r.panel_light(&look, led, Light::Lit, 2.0, STRIP, &mut a)
            .unwrap();
        draw_panel_led(&faces[0], &look, led, Light::Lit, 2.0, STRIP, &mut b).unwrap();
        assert_eq!(a, b, "{led:?}");
    }
    assert_eq!(r.panel_layout(2.0), panel_layout(&faces[0], 2.0));
    assert_eq!(r.panel_letters(), panel_letters(&faces[0]));
}

/// **The built-in themes' socket and bevel are the panel's numbers**: the
/// socket #050505 on the dark plate and #DADADA on the light; on the dark
/// the edge black at 55%, the highlight white at 13% and the bottom line
/// black at 80%, on the light 28%, 95% and 22%. Perturbation: the step of
/// 21 everywhere, or the alphas read the other way round, and this goes
/// red.
#[test]
fn the_built_in_plates_keep_their_socket_and_bevel() {
    let look = |t: &Theme| look_of(t, LightsColours::Meaning, Glow::Full, Halo::default());
    let (d, l) = (look(&Theme::dark()), look(&Theme::light()));
    assert_eq!(d.plate, Argb(0xFF_10_10_10));
    assert_eq!(l.plate, Argb(0xFF_EF_EF_EF));
    assert_eq!(d.socket, Argb(0xFF_05_05_05));
    assert_eq!(l.socket, Argb(0xFF_DA_DA_DA));
    let black = |a: f32| Argb(0xFF_00_00_00).with_alpha(Chan::from_unit(a));
    let white = |a: f32| Argb(0xFF_FF_FF_FF).with_alpha(Chan::from_unit(a));
    assert_eq!(d.bevel.edge, black(0.55));
    assert_eq!(d.bevel.top, white(0.13));
    assert_eq!(d.bevel.bottom, black(0.80));
    assert_eq!(l.bevel.edge, black(0.28));
    assert_eq!(l.bevel.top, white(0.95));
    assert_eq!(l.bevel.bottom, black(0.22));
}

/// **On any page the socket reads as a recess and the bevel as a bevel**,
/// made from the plate: on a dark page, a light one, a mid-grey, tinted ones
/// and the two extremes, the socket is at least 11 levels off the plate in
/// each channel it has room in and keeps the channels' order, so the hue;
/// the highlight over the plate is lighter than it and the edge and the
/// bottom line darker, where the page has room for them at all. The mid-grey's
/// socket is #707070, 16 under #808080; its bevel is between the two
/// built-in ones'. Perturbation: the socket a fixed #050505 and the
/// light page's goes black; or the alphas fixed at the dark page's and the
/// mid-grey's is not between.
#[test]
fn the_socket_and_the_bevel_hold_on_any_page() {
    let look_on = |bg: u32| {
        let mut t = Theme::dark();
        t.bg = Argb(bg);
        look_of(&t, LightsColours::Meaning, Glow::Full, Halo::default())
    };
    let rgb = |c: Argb| [c.r().get(), c.g().get(), c.b().get()];
    let luma = |c: Argb| {
        let [r, g, b] = rgb(c).map(f64::from);
        0.299 * r + 0.587 * g + 0.114 * b
    };
    for bg in [
        0xFF_00_00_00u32,
        0xFF_05_05_05,
        0xFF_1A_1B_26,
        0xFF_00_2B_36,
        0xFF_80_80_80,
        0xFF_A0_60_40,
        0xFF_FD_F6_E3,
        0xFF_FF_FF_FF,
    ] {
        let l = look_on(bg);
        let name = format!("{bg:08X}");
        assert_ne!(l.socket, l.plate, "{name}: the socket shows");
        assert_eq!(l.socket.a().get(), 0xFF, "{name}");
        let (p, s) = (rgb(l.plate), rgb(l.socket));
        let moved = p
            .iter()
            .zip(&s)
            .map(|(a, b)| i32::from(*a) - i32::from(*b))
            .collect::<Vec<_>>();
        let most = moved.iter().map(|d| d.abs()).max().unwrap();
        assert!(most >= 11, "{name}: {moved:?}");
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    p[i] < p[j] && s[i] <= s[j]
                        || p[i] > p[j] && s[i] >= s[j]
                        || p[i] == p[j] && s[i] == s[j],
                    "{name}: its hue: {p:?} to {s:?}"
                );
            }
        }
        let top = over(l.plate, l.bevel.top);
        let edge = over(l.plate, l.bevel.edge);
        let bottom = over(l.plate, l.bevel.bottom);
        if luma(l.plate) < 250.0 {
            assert!(luma(top) > luma(l.plate) + 8.0, "{name}: top {top:08X?}");
        }
        if luma(l.plate) > 5.0 {
            assert!(luma(edge) < luma(l.plate) - 8.0, "{name}: edge {edge:08X?}");
            assert!(luma(bottom) < luma(l.plate) - 8.0, "{name}: bottom");
        }
    }
    let grey = look_on(0xFF_80_80_80);
    assert_eq!(grey.socket, Argb(0xFF_70_70_70));
    let alpha = |c: Argb| c.a().get();
    let (d, li) = (look_on(0xFF_10_10_10).bevel, look_on(0xFF_EF_EF_EF).bevel);
    for (m, a, b) in [
        (grey.bevel.edge, d.edge, li.edge),
        (grey.bevel.top, d.top, li.top),
        (grey.bevel.bottom, d.bottom, li.bottom),
    ] {
        let (lo, hi) = (alpha(a).min(alpha(b)), alpha(a).max(alpha(b)));
        assert!(
            alpha(m) > lo && alpha(m) < hi,
            "{m:08X?} between {a:08X?} and {b:08X?}"
        );
    }
    // Past either end of the pages it is held there, and a black page has
    // no room below it, so its socket is the step above.
    assert_eq!(look_on(0xFF_00_00_00).bevel, d);
    assert_eq!(look_on(0xFF_00_00_00).socket, Argb(0xFF_0B_0B_0B));
    assert_eq!(look_on(0xFF_FF_FF_FF).bevel, li);
    assert_eq!(look_on(0xFF_FF_FF_FF).socket, Argb(0xFF_EA_EA_EA));
}

/// **Every full-coverage lamp pixel on the panel is its palette entry
/// exactly**: for every set, the built-in themes and xterm's on black and on
/// white, every state, at 1x and 2x, with its socket and letters, the mask
/// made with neither. The control: the same mask a column along finds pixels
/// that are not the lamp's colour. Perturbation: draw the socket over the
/// lamp, or mix the lamp into the socket, and this goes red.
#[test]
fn every_lamp_pixel_on_the_panel_is_its_palette_entry() {
    let bold = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&bold).unwrap();
    let mut read = 0;
    let mut control = 0;
    for (name, theme) in themes() {
        for set in LightsColours::ALL {
            let look = look_of(&theme, set, Glow::Full, Halo::default());
            let bare = LightsLook {
                label: Argb(0),
                socket: Argb(0),
                glow: Glow::Dimmed,
                ..look
            };
            for scale in [1.0, 2.0] {
                for led in Led::ALL {
                    for light in Light::ALL {
                        let want = match light {
                            Light::Lit => *look.lit.get(led),
                            Light::Dark => look.unlit,
                            Light::Unread => look.ring,
                        };
                        let (mut full, mut mask) = (LightsImage::new(), LightsImage::new());
                        draw_panel_led(&face, &look, led, light, scale, STRIP, &mut full).unwrap();
                        draw_panel_led(&face, &bare, led, light, scale, STRIP, &mut mask).unwrap();
                        let mut n = 0;
                        for (i, m) in mask.pixels.iter().enumerate() {
                            if m.a().get() == 0xFF {
                                assert_eq!(
                                    full.pixels[i], want,
                                    "{name} {set:?} {led:?} {light:?} at {scale}x: pixel {i}"
                                );
                                n += 1;
                                let moved = full.pixels.get(i + 3).copied().unwrap_or(want);
                                control += usize::from(moved != want);
                            }
                        }
                        // The ring is 1.25 points across: no pixel of it is
                        // whole at 1x.
                        let whole = light != Light::Unread || scale > 1.5;
                        assert!(
                            n > 0 || !whole,
                            "{name} {led:?} {light:?} at {scale}x: no lamp"
                        );
                        read += n;
                    }
                }
            }
        }
    }
    assert!(read > 10_000, "{read}");
    assert!(control > 1000, "the mask moved finds others: {control}");
}

/// **The socket is behind every lamp, in every state, a socket's radius
/// about its centre, and the lamp is over it**: 3.9 points from the centre,
/// the socket's colour inside it and nothing past it; the lit dot leaves a
/// ring of socket, the small dark dot and the unread ring's hole a wider one.
/// Perturbation: draw no socket, one of radius 3.5, or one only behind a lit
/// lamp, and this goes red.
#[test]
fn the_socket_is_behind_lit_unlit_and_unread() {
    let bold = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&bold).unwrap();
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let look = look_of(
            &theme,
            LightsColours::Meaning,
            Glow::Dimmed,
            Halo::default(),
        );
        for scale in [1.0, 2.0] {
            for led in Led::ALL {
                let (cx, cy) = panel_centre(&face, &look, led, scale);
                for light in Light::ALL {
                    let mut img = LightsImage::new();
                    draw_panel_led(&face, &look, led, light, scale, STRIP, &mut img).unwrap();
                    let at = format!("{name} {led:?} {light:?} at {scale}x");
                    // A pixel wholly between the lamp's edge and the socket's,
                    // to the left of the centre; only the small dot leaves
                    // one a whole pixel wide at 1x.
                    let lamp_edge = match light {
                        Light::Lit | Light::Unread => 3.5,
                        Light::Dark => 1.75,
                    };
                    for y in 0..img.height.value() {
                        for x in 0..img.width.value() {
                            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                            let d = (dx * dx + dy * dy).sqrt() / scale;
                            let half = 0.75 / scale;
                            let p = at_xy(&img, x, y);
                            if d > lamp_edge + half && d < 3.9 - half {
                                assert_eq!(p, look.socket, "{at}: {x},{y} at {d}");
                            }
                            if d > 3.9 + half && dy.abs() < 6.0 * scale {
                                assert_eq!(p.a().get(), 0, "{at}: past it, {x},{y} at {d}");
                            }
                        }
                    }
                }
                // The ring of socket past the lit dot shows at 2x as its
                // colour where the dot's edge has gone.
                if scale > 1.5 {
                    let mut lit = LightsImage::new();
                    let none = LightsLook {
                        socket: Argb(0),
                        ..look
                    };
                    draw_panel_led(&face, &look, led, Light::Lit, 2.0, STRIP, &mut lit).unwrap();
                    let mut bare = LightsImage::new();
                    draw_panel_led(&face, &none, led, Light::Lit, 2.0, STRIP, &mut bare).unwrap();
                    assert_ne!(lit, bare, "{name} {led:?}: a socket behind a lit lamp");
                }
            }
        }
    }
}

/// **Each lamp stands over its letters at the panel's numbers**: its centre
/// 8.5 points down the strip, within half a device pixel of the middle of
/// its letters across; its dot 7 across and an unlit one half that; the
/// letters' caps ending on the baseline at 22 and their top under the lamp's
/// halo's reach; at 1x and 2x, in a strip of 28 and centred in one of 32.
/// And the columns' layout: the nine in order, side by side. Perturbation: a
/// lamp centre at 14, a baseline at 20, or letters beside the lamp, and this
/// goes red.
#[test]
fn each_lamp_stands_over_its_letters_at_the_panels_numbers() {
    let bold = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Bold.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let face = Face::parse(&bold).unwrap();
    let look = look_of(
        &Theme::dark(),
        LightsColours::Meaning,
        Glow::Full,
        Halo::default(),
    );
    let letters = panel_letters(&face);
    for scale in [1.0_f32, 2.0] {
        let (slots, end) = panel_layout(&face, scale).unwrap();
        let mut next = 0;
        for led in Led::ALL {
            let s = slots.get(led);
            assert_eq!(s.start.value(), next, "{led:?} at {scale}x");
            next = s.start.value() + s.width.value();
        }
        assert_eq!(next, end.value());
        for led in Led::ALL {
            let (cx, cy) = panel_centre(&face, &look, led, scale);
            let (x, w) = *letters.get(led);
            let start = slots.get(led).start.value() as f32;
            let wanted = (x + w / 2.0) * scale - start;
            let at = format!("{led:?} at {scale}x");
            assert!((cx - wanted).abs() <= 0.5 + 1e-3, "{at}: {cx} and {wanted}");
            assert!((cy - 8.5 * scale).abs() < 0.01, "{at}: {cy}");
            // Whole pixels at the dot's left, so the nine are alike.
            let left = cx - DOT * scale / 2.0;
            assert!((left - left.round()).abs() < 0.01, "{at}: {left}");
            // The letters' ink, alone.
            let only = LightsLook {
                lit: ByLed::of(|_| Argb(0)),
                unlit: Argb(0),
                ring: Argb(0),
                socket: Argb(0),
                glow: Glow::Dimmed,
                ..look
            };
            let mut img = LightsImage::new();
            draw_panel_led(&face, &only, led, Light::Dark, scale, STRIP, &mut img).unwrap();
            let (w_px, h_px) = (img.width.value(), img.height.value());
            let inked: Vec<(u32, u32)> = (0..h_px)
                .flat_map(|y| (0..w_px).map(move |x| (x, y)))
                .filter(|&(x, y)| at_xy(&img, x, y).a().get() > 64)
                .collect();
            let bottom = inked.iter().map(|p| p.1).max().unwrap() + 1;
            let top = inked.iter().map(|p| p.1).min().unwrap();
            let (lo, hi) = (
                inked.iter().map(|p| p.0).min().unwrap() as f32,
                inked.iter().map(|p| p.0).max().unwrap() as f32 + 1.0,
            );
            assert!(
                (bottom as f32 - 22.0 * scale).abs() <= 1.0,
                "{at}: the baseline, {bottom}"
            );
            assert!(top as f32 >= 15.0 * scale, "{at}: under the halo, {top}");
            assert!(
                ((lo + hi) / 2.0 - cx).abs() <= 1.5,
                "{at}: letters {lo}..{hi} about {cx}"
            );
        }
    }
    // In a taller strip everything keeps its place about the middle.
    let mut img = LightsImage::new();
    draw_panel_led(
        &face,
        &LightsLook {
            label: Argb(0),
            socket: Argb(0),
            glow: Glow::Dimmed,
            ..look
        },
        Led::Rd,
        Light::Lit,
        2.0,
        32.0,
        &mut img,
    )
    .unwrap();
    let rows: Vec<u32> = (0..img.height.value())
        .filter(|&y| (0..img.width.value()).any(|x| at_xy(&img, x, y).a().get() == 0xFF))
        .collect();
    let mid = (rows[0] + rows[rows.len() - 1] + 1) as f32 / 2.0;
    assert!((mid - 2.0 * (8.5 + 2.0)).abs() < 0.01, "{mid}");
}

/// **The unlit lamp on the panel is half the lit one's diameter**: so the
/// state reads by size as well as colour, WCAG 2.2's SC 1.4.1, as on the
/// line. Perturbation: an unlit dot of the lit size, and this goes red.
#[test]
fn the_panels_unlit_lamp_is_half_the_lit_ones_diameter() {
    let bold = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&bold).unwrap();
    let look = LightsLook {
        label: Argb(0),
        socket: Argb(0),
        glow: Glow::Dimmed,
        ..look()
    };
    for scale in [2.0, 4.0] {
        let across = |light| {
            let mut img = LightsImage::new();
            draw_panel_led(&face, &look, Led::Cs, light, scale, STRIP, &mut img).unwrap();
            (0..img.width.value())
                .filter(|&x| (0..img.height.value()).any(|y| at_xy(&img, x, y).a().get() > 127))
                .count() as f32
                / scale
        };
        assert!((across(Light::Lit) - 7.0).abs() <= 0.5, "{scale}");
        assert!((across(Light::Dark) - 3.5).abs() <= 0.5, "{scale}");
    }
}

/// **The panel's glow starts a point off the dot and stays in the lamp's
/// column and above its letters, and Dim Flashing Lights takes it away**:
/// a lit lamp at the strongest glow against one under Glow::Dimmed differs
/// only from 4.5 points off the centre, a point off the dot's 3.5 past the
/// socket's 3.9, to 6.5; never in the letters' rows; and not at all at a
/// strength of 0. Perturbation: a halo from the dot's edge, or one reaching
/// the picture's height as the line's does, and this goes red.
#[test]
fn the_panels_glow_is_off_the_dot_and_over_the_letters() {
    let bold = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&bold).unwrap();
    let theme = Theme::dark();
    let lit = |glow, halo, scale| {
        let l = look_of(&theme, LightsColours::Meaning, glow, halo);
        let mut img = LightsImage::new();
        draw_panel_led(&face, &l, Led::Sd, Light::Lit, scale, STRIP, &mut img).unwrap();
        img
    };
    for scale in [1.0, 2.0] {
        let dim = lit(Glow::Dimmed, Halo::of(1.0), scale);
        assert_eq!(dim, lit(Glow::Full, Halo::of(0.0), scale), "{scale}");
        let full = lit(Glow::Full, Halo::of(1.0), scale);
        let look = look_of(
            &theme,
            LightsColours::Meaning,
            Glow::Dimmed,
            Halo::default(),
        );
        let (cx, cy) = panel_centre(&face, &look, Led::Sd, scale);
        let mut touched = 0;
        for y in 0..full.height.value() {
            for x in 0..full.width.value() {
                if at_xy(&full, x, y) == at_xy(&dim, x, y) {
                    continue;
                }
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let d = (dx * dx + dy * dy).sqrt() / scale;
                assert!(d >= 4.5 - 1.0 / scale, "{x},{y}: {d} at {scale}x");
                assert!(d <= 6.5 + 1.0 / scale, "{x},{y}: {d} at {scale}x");
                assert!((y as f32) < 16.0 * scale, "{x},{y} in the letters");
                touched += 1;
            }
        }
        assert!(touched > 20, "the glow draws: {touched} at {scale}x");
    }
}

/// **The lamps read against the socket they stand in, as they do on the
/// plate**: every set's lit lamps at 3:1 or better on the built-in themes
/// (SC 1.4.11), the unlit lamp too. Perturbation: a socket of the lamp's
/// lightness, #808080 on the dark theme, and this goes red.
#[test]
fn the_lamps_read_against_their_socket() {
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for set in LightsColours::ALL {
            let look = look_of(&theme, set, Glow::Full, Halo::default());
            for led in Led::ALL {
                let c = ratio(*look.lit.get(led), look.socket);
                assert!(c >= LIT_CONTRAST, "{name} {set:?} {led:?}: {c:.2}");
            }
            let c = ratio(look.unlit, look.socket);
            assert!(c >= LIT_CONTRAST, "{name} unlit: {c:.2}");
        }
    }
}

/// **The panel's plate, drawn**: its picture a margin past the plate, the
/// plate's own colour exactly inside, the highlight along the top inside and
/// the line along the bottom inside over the plate, the edge outside, and
/// its corners rounded: at 2x each a pixel across. At 1x each half a pixel's
/// coverage. On the dark and the light theme. The control: a plate with no
/// bevel's colours is one colour inside. Perturbation: the highlight on the
/// bottom, the edge inside the plate, or square corners, and this goes red.
#[test]
fn the_panels_plate_has_its_bevel_and_its_corners() {
    for theme in [Theme::dark(), Theme::light()] {
        let look = look_of(&theme, LightsColours::Meaning, Glow::Full, Halo::default());
        for scale in [1.0_f32, 2.0] {
            let (w, h) = (
                u32::try_from((221.0 * scale) as i64).unwrap(),
                u32::try_from((24.0 * scale) as i64).unwrap(),
            );
            let mut img = LightsImage::new();
            draw_panel_plate(&look, Px::of(w), Px::of(h), scale, &mut img).unwrap();
            let m = panel_margin(scale).unwrap().value();
            assert_eq!(m, 1, "a pixel past it, at either scale");
            assert_eq!(img.width.value(), w + 2 * m);
            assert_eq!(img.height.value(), h + 2 * m);
            let mid = w / 2 + m;
            let coverage = if scale > 1.5 { 1.0 } else { 0.5 };
            let part =
                |c: Argb| c.with_alpha(Chan::from_unit(f32::from(c.a().get()) / 255.0 * coverage));
            // Inside, on the middle row.
            assert_eq!(at_xy(&img, mid, m + h / 2), look.plate);
            // The highlight, the plate's top row.
            assert_eq!(at_xy(&img, mid, m), over(look.plate, part(look.bevel.top)));
            // The bottom line, its bottom row.
            assert_eq!(
                at_xy(&img, mid, m + h - 1),
                over(look.plate, part(look.bevel.bottom))
            );
            // The edge, the rows and columns just outside it.
            for (x, y) in [
                (mid, 0),
                (mid, h + 2 * m - 1),
                (0, h / 2),
                (w + 2 * m - 1, h / 2),
            ] {
                assert_eq!(
                    at_xy(&img, x, y),
                    part(look.bevel.edge),
                    "{x},{y} at {scale}x"
                );
            }
            // The first row inside the plate past the highlight is the plate.
            assert_eq!(at_xy(&img, mid, m + 1), look.plate, "at {scale}x");
            // The corners: nothing at the picture's corner, the plate a
            // radius in.
            assert_eq!(at_xy(&img, 0, 0).a().get(), 0, "at {scale}x");
            let r = 5 * u32::try_from(scale as i64).unwrap();
            assert_eq!(at_xy(&img, m + r, m + r), look.plate, "at {scale}x");
            assert_ne!(at_xy(&img, m, m), look.plate, "square at {scale}x");
            // The highlight stops where the corner starts.
            assert_eq!(at_xy(&img, m + r - 1, m), {
                let mut bare = LightsImage::new();
                let none = LightsLook {
                    bevel: baud_render::Bevel {
                        top: Argb(0),
                        ..look.bevel
                    },
                    ..look
                };
                draw_panel_plate(&none, Px::of(w), Px::of(h), scale, &mut bare).unwrap();
                at_xy(&bare, m + r - 1, m)
            });
        }
    }
}

/// **The panel meets WCAG 2.3.1 by the area clause, as the line does**:
/// every lamp changing at once, lit against dark, on the panel's columns, at
/// the strongest glow and the default and under Dim Flashing Lights, in every
/// set on the built-in themes and xterm's, under a quarter of the field and a
/// tenth of that. The clock's three flashes a second are the lights' own and
/// are held by `the_lights_flash_at_most_three_times_a_second_and_show_everything`.
/// The control: the flash area is more than nothing. Perturbation: draw the
/// plate in the lit colour and this goes red.
#[test]
fn every_panel_lamp_changing_at_once_flashes_under_the_area_clause() {
    let bold = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Bold.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let face = Face::parse(&bold).unwrap();
    let mut most: f32 = 0.0;
    for (name, theme) in themes() {
        for set in LightsColours::ALL {
            for (glow, halo) in [
                (Glow::Full, Halo::of(1.0)),
                (Glow::Full, Halo::default()),
                (Glow::Dimmed, Halo::of(1.0)),
            ] {
                let look = look_of(&theme, set, glow, halo);
                let lit = panel_row(&face, &look, Shown::all(Light::Lit), 2.0);
                let dark = panel_row(&face, &look, Shown::all(Light::Dark), 2.0);
                let area = flash_area(&lit, &dark, 2.0);
                assert!(area > 0.0, "{name} {set:?}: the control");
                assert!(
                    area <= AREA_LIMIT,
                    "{name} {set:?} {glow:?}: {area} of {AREA_LIMIT}"
                );
                most = most.max(area);
            }
        }
    }
    assert!(most < AREA_LIMIT / 10.0, "the most, {most} WCAG pixels");
}

/// **A state held asks for no step while it holds** (`Level`): OH held
/// from 0 is owed a step at once and lit by it, then asks for none, not at
/// its hold's end and not an hour later; ended at 3600 s it keeps its hold
/// from there and goes dark at its end. A refused read held is unread with
/// no step asked for while it holds. The control: OH seen once at 0 asks
/// for a step at the hold's end. Perturbations: a held light's hold runs
/// out in the step as a sighting's does -- an hour on fails; or in
/// `next_change` -- held and seen fails.
#[test]
fn a_state_held_asks_for_no_step_while_it_holds() {
    let ms = |m: u64| t(Duration::from_millis(m));
    let mut seen = Activity::new();
    seen.step(ms(0), Pace::Full);
    seen.seen(Led::Oh, ms(0));
    assert!(seen.due(ms(0), Pace::Full), "the control: owed at once");
    assert_eq!(seen.step(ms(0), Pace::Full).oh, Light::Lit);
    assert_eq!(
        seen.next_change(ms(1), Pace::Full),
        Some(ms(200)),
        "the control: a sighting's hold ends"
    );
    let mut a = Activity::new();
    a.step(ms(0), Pace::Full);
    a.hold(Led::Oh, Level::Held, ms(0));
    assert!(a.due(ms(0), Pace::Full), "owed at once");
    assert_eq!(a.step(ms(0), Pace::Full).oh, Light::Lit);
    assert!(!a.due(ms(1), Pace::Full));
    assert_eq!(
        a.next_change(ms(300), Pace::Full),
        None,
        "held: no hold to end"
    );
    // A sighting while held, as CS takes on a short write: still none.
    a.seen(Led::Oh, ms(1_000));
    assert_eq!(a.next_change(ms(1_001), Pace::Full), None, "held, seen");
    let hour = 3_600_000;
    assert!(!a.due(ms(hour), Pace::Full), "an hour on");
    assert_eq!(a.step(ms(hour), Pace::Full).oh, Light::Lit);
    a.hold(Led::Oh, Level::Held, ms(hour));
    assert!(
        !a.due(ms(hour), Pace::Full),
        "the same level again: nothing"
    );
    a.hold(Led::Oh, Level::Free, ms(hour + 10));
    assert!(
        !a.due(ms(hour + 10), Pace::Full),
        "ended: its hold from then"
    );
    assert_eq!(
        a.next_change(ms(hour + 10), Pace::Full),
        Some(ms(hour + 210))
    );
    assert_eq!(a.step(ms(hour + 210), Pace::Full).oh, Light::Dark);
    let mut r = Activity::new();
    r.step(ms(0), Pace::Full);
    r.hold(Led::Cd, Level::Refused, ms(0));
    assert!(r.due(ms(0), Pace::Full));
    assert_eq!(r.step(ms(0), Pace::Full).cd, Light::Unread);
    assert_eq!(r.next_change(ms(300), Pace::Full), None, "refused, held");
}

/// **A state held wakes a driver with no display link twice, and a
/// sighting fed at every wake wakes it every 200 ms** (`Level`): a driver
/// that wakes only at [`Activity::due`] and [`Activity::next_change`],
/// counting its wakes over an hour of OH held. Held, it wakes to light it
/// and once at the dwell's end; the control, OH fed as a sighting at each
/// wake, as the lights were fed at every tick, wakes at each hold's end
/// for the whole hour. Counted, not timed. Perturbation: a held light's
/// hold run out as a sighting's -- held wakes as often as the control.
#[test]
fn a_state_held_wakes_twice_in_an_hour() {
    let ms = |m: u64| t(Duration::from_millis(m));
    let hour = ms(3_600_000);
    let wakes = |held: bool| {
        let mut a = Activity::new();
        a.step(ms(0), Pace::Full);
        let mut now = ms(0);
        let mut woke = 0u32;
        if held {
            a.hold(Led::Oh, Level::Held, now);
        }
        loop {
            if !held {
                a.seen(Led::Oh, now);
            }
            woke += 1;
            if a.due(now, Pace::Full) {
                a.step(now, Pace::Full);
            }
            match a.next_change(now, Pace::Full) {
                Some(next) if next <= hour => now = next,
                _ => break,
            }
        }
        (woke, a.step(now, Pace::Full).oh)
    };
    assert_eq!(wakes(true), (2, Light::Lit), "held");
    let (fed, oh) = wakes(false);
    assert_eq!(oh, Light::Lit, "the control: lit too");
    assert!(fed > 10_000, "the control: a wake each hold, {fed}");
}
