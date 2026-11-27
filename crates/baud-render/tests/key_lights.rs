//! The keyboard lights DECLL loads (`lights::draw_key_led`): each drawn as
//! a light of the strip is, its letters and then its lamp, lit in SYN's
//! entry of the set exactly and dark a dot in the unlit entry.

mod support;

use baud_domain::{KeyLed, LedState};
use baud_font::Face;
use baud_render::lights::{
    DOT, GAP, TEXT, draw_key_led, draw_led, key_letters, key_picture_width, text_width,
};
use baud_render::{Argb, Glow, Halo, Led, Light, LightsColours, LightsImage, LightsLook, Theme};
use support::font;

/// The title strip's height in points, as the window's is.
const STRIP: f32 = 28.0;

/// The pixel at the lamp's centre in a picture of `led` drawn at `scale`.
fn centre(face: &Face<'_>, img: &LightsImage, led: KeyLed, scale: f32) -> Argb {
    let cx = (text_width(face, key_letters(led), TEXT) + GAP + DOT / 2.0) * scale;
    let cy = STRIP * scale / 2.0;
    let x = u32::try_from(cx.floor() as i64).unwrap();
    let y = u32::try_from(cy.floor() as i64).unwrap();
    img.pixels[usize::try_from(y * img.width.value() + x).unwrap()]
}

#[test]
fn a_lit_key_light_is_syns_entry_and_a_dark_one_the_unlit_entry() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    for theme in [Theme::dark(), Theme::light()] {
        for set in LightsColours::ALL {
            let look = LightsLook::of(
                &theme.palette,
                theme.fg,
                theme.bg,
                set,
                Glow::Full,
                Halo::default(),
            );
            for led in KeyLed::ALL {
                for scale in [1.0_f32, 2.0] {
                    let mut on = LightsImage::new();
                    draw_key_led(&face, &look, led, LedState::On, scale, STRIP, &mut on).unwrap();
                    let mut off = LightsImage::new();
                    draw_key_led(&face, &look, led, LedState::Off, scale, STRIP, &mut off).unwrap();
                    let wide = (key_picture_width(&face, led) * scale).ceil() as i64;
                    assert_eq!(i64::from(on.width.value()), wide, "{led:?} at {scale}");
                    assert_eq!(on.width, off.width);
                    assert_eq!(
                        centre(&face, &on, led, scale),
                        *look.lit.get(Led::Syn),
                        "{led:?} lit at {scale}, {set:?}"
                    );
                    assert_eq!(
                        centre(&face, &off, led, scale),
                        look.unlit,
                        "{led:?} dark at {scale}, {set:?}"
                    );
                    assert_ne!(on.pixels, off.pixels, "{led:?}: lit reads apart from dark");
                }
            }
        }
    }
}

/// The strip's lights draw as before: `draw_led` and `draw_key_led` share
/// one picture, so a strip light's lit centre is still its own entry.
#[test]
fn a_strip_light_keeps_its_own_entry() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&mono).unwrap();
    let theme = Theme::dark();
    let look = LightsLook::of(
        &theme.palette,
        theme.fg,
        theme.bg,
        LightsColours::Meaning,
        Glow::Full,
        Halo::default(),
    );
    let scale = 2.0_f32;
    for led in Led::ALL {
        let mut img = LightsImage::new();
        draw_led(&face, &look, led, Light::Lit, scale, STRIP, &mut img).unwrap();
        let cx = (text_width(&face, led.letters(), TEXT) + GAP + DOT / 2.0) * scale;
        let x = u32::try_from(cx.floor() as i64).unwrap();
        let y = u32::try_from((STRIP * scale / 2.0).floor() as i64).unwrap();
        let at = img.pixels[usize::try_from(y * img.width.value() + x).unwrap()];
        assert_eq!(at, *look.lit.get(led), "{led:?}");
    }
}
