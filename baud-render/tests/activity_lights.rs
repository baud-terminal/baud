//! The activity lights: the pixels the title strip shows (`lights::draw`,
//! `lights::draw_led` and `lights::draw_plate`, the pixel oracle for a
//! strip no GPU path draws -- each light is an image in a view of its own
//! over the plate's view, not part of the grid's frame), every lamp one of
//! the theme's sixteen palette entries exactly, and the clock that lights
//! them (`Activity`), held to WCAG 2.2's three flashes a second, to showing
//! everything it is told, and under Reduce Motion to one steady light
//! while the activity lasts.

mod support;

use baud_domain::{Chan, HostTime};
use baud_domain::{Cols, Rows};
use baud_font::{Face, Px};
use baud_render::lights::{
    DOT, GAP, HALO, HALO_GAP, LIT_CONTRAST, PAIR, PLATE_PAD, PLATE_RADIUS_TITLE, PLATE_TITLE, TEXT,
    TEXT_CONTRAST, draw, draw_led, draw_plate, full, layout, ring_entry, unlit_entry, width,
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
/// red.
#[test]
fn the_sets_are_their_tables_and_meaning_is_the_default() {
    let nums = |set: LightsColours, half| Led::ALL.map(|l| index(*set.entries(half).get(l)));
    use LightsColours::{Iec, Meaning, Modem};
    assert_eq!(nums(Meaning, Half::Dark), [10, 11, 4, 6, 2, 3, 1, 5, 13]);
    assert_eq!(nums(Meaning, Half::Light), [10, 6, 4, 12, 2, 14, 1, 5, 13]);
    for half in [Half::Dark, Half::Light] {
        assert_eq!(nums(Iec, half), [2, 7, 2, 7, 7, 7, 3, 6, 6]);
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

/// **The unlit lamp is 8 in the meaning set, and 8 or 0 by the plate in
/// the other two**: whichever of the two is at rest -- visible against
/// the plate and no louder than the quietest lit lamp -- the more
/// visible; else the quieter visible one. On the dark theme the IEC set's
/// quietest lamp, green 2 at 8.8:1, leaves 8 at rest, and the modem set's
/// red at 4.5:1 does not, so the unlit lamp is 0; a theme whose entry 0
/// is its background gives 8. The meaning set's is 8 there although its
/// blue CD is quieter than 8. The ring is the entry the foreground is,
/// else 7. Perturbations: always take 8, and the modem set's unlit lamp
/// is louder than its lit ones; take the meaning set's by the plate, and
/// the default look's is 0.
#[test]
fn the_unlit_lamp_is_eight_or_zero_by_the_plate() {
    let dark = Theme::dark();
    let plate = dark.bg;
    let lit = |set: LightsColours| ByLed::of(|l| set.entries(Half::Dark).get(l).of(&dark.palette));
    assert_eq!(
        unlit_entry(&dark.palette, plate, &lit(LightsColours::Iec)),
        Ansi::BrightBlack
    );
    assert_eq!(
        unlit_entry(&dark.palette, plate, &lit(LightsColours::Modem)),
        Ansi::Black
    );
    assert_eq!(
        look_of(&dark, LightsColours::Meaning, Glow::Full, Halo::default()).unlit,
        dark.palette[8],
        "the default look's unlit lamp is 8 whatever its lit ones"
    );
    assert_eq!(
        look_of(&dark, LightsColours::Modem, Glow::Full, Halo::default()).unlit,
        dark.palette[0],
        "the modem set's look takes its entry by the plate"
    );
    let mut same = Theme::dark();
    same.palette[0] = same.bg;
    assert_eq!(
        unlit_entry(&same.palette, same.bg, &lit(LightsColours::Modem)),
        Ansi::BrightBlack,
        "an entry 0 that is the plate is not visible"
    );
    for (name, theme) in themes() {
        for set in LightsColours::ALL {
            let look = look_of(&theme, set, Glow::Full, Halo::default());
            assert!(
                [Ansi::Black, Ansi::BrightBlack]
                    .iter()
                    .any(|a| a.of(&theme.palette) == look.unlit),
                "{name} {set:?}"
            );
        }
    }
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
/// the letters put ink left of the light and none right of it; only CS
/// has ink above its letters' caps, its bar; the picture is its slot's
/// width and the strip's height, clear in its corner.
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
                assert_eq!(bar, led == Led::Cs, "{led:?} at {scale}x: the bar");
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
