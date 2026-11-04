//! **The activity lights in the title strip**: AppKit's colours for them,
//! resolved under the window's appearance, and the picture an image view
//! shows, made from the pixels `baud_render::lights` draws.
//!
//! The colours are AppKit's dynamic system colours, by what each light
//! says of the line's direction -- `systemGreenColor` for what comes in
//! (RD, HS), `systemOrangeColor` for what goes out (SD, AA, CS),
//! `systemBlueColor` for the state of the session and the screen (CD, OH,
//! FAX, SYN), `secondaryLabelColor` for the letters -- resolved under the
//! appearance as the unemphasized selection is ([`crate::selection`]):
//! "Prefer system-defined colors. These colors
//! have their own accessible variants that automatically adapt when people
//! adjust their color preferences, such as enabling Increase Contrast or
//! toggling between the light and dark appearances"
//! (`[[source: apple-hig-accessibility]]`). They do not all read against
//! the title strip: under the light appearance, Increase Contrast's
//! included, AppKit's green, orange and blue read under WCAG's 3:1 against
//! the standard material's greys, so the strip lifts them there
//! (`baud_render::lights::strip_colours`), keeping each hue.
//!
//! `unsafe` is allowed here, as in [`crate::print`], for the one bitmap
//! constructor the bindings mark so and the buffer it hands back.
#![allow(unsafe_code)]

use baud_domain::Chan;
use baud_render::{Argb, ByLed, Led, LightsImage};
use block2::RcBlock;
use core::cell::Cell;
use macos_liquid_glass::objc2::AnyThread;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSAppearance, NSBitmapImageRep, NSColor, NSColorSpace, NSDeviceRGBColorSpace, NSImage,
};
use macos_liquid_glass::objc2_foundation::NSSize;

/// Which of the three colours a light takes: in, out or a state
/// ([`Led::direction`]).
pub use baud_render::lights::Direction;

/// `led`'s direction: RD and HS bring data in, SD, AA and CS send it out
/// or fail to, and the rest show a state ([`Led::direction`], which the
/// lights' own line in the frame takes its theme colours by too).
pub fn direction(led: Led) -> Direction {
    led.direction()
}

/// Every light's lit colour and the letters' colour, under `appearance`.
pub fn colours(appearance: &NSAppearance) -> (ByLed<Argb>, Argb) {
    let out = Cell::new(([Argb::default(); 3], Argb::default()));
    let resolve = RcBlock::new(|| {
        out.set((
            [
                srgb(&NSColor::systemGreenColor()),
                srgb(&NSColor::systemOrangeColor()),
                srgb(&NSColor::systemBlueColor()),
            ],
            srgb(&NSColor::secondaryLabelColor()),
        ));
    });
    appearance.performAsCurrentDrawingAppearance(&resolve);
    let ([inward, outward, state], label) = out.get();
    let lit = ByLed::of(|l| match direction(l) {
        Direction::In => inward,
        Direction::Out => outward,
        Direction::State => state,
    });
    (lit, label)
}

/// A colour in sRGB with its alpha, each channel rounded to eight bits; a
/// colour with no sRGB form is transparent.
fn srgb(c: &Retained<NSColor>) -> Argb {
    let Some(s) = c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
        return Argb(0);
    };
    let unit = |v: f64| Chan::from_unit(v as f32);
    Argb::rgb(
        unit(s.redComponent()),
        unit(s.greenComponent()),
        unit(s.blueComponent()),
    )
    .with_alpha(unit(s.alphaComponent()))
}

/// The strip's pixels as premultiplied RGBA bytes, row by row: what a
/// bitmap with an alpha channel holds.
pub fn rgba(img: &LightsImage) -> Vec<u8> {
    img.pixels
        .iter()
        .flat_map(|p| {
            let w = baud_domain::premultiply(p.0);
            let [a, r, g, b] = w.to_be_bytes();
            [r, g, b, a]
        })
        .collect()
}

/// **The strip as a picture** `scale` device pixels a point, sized in
/// points so an image view at that size shows each pixel once; `None` for
/// an empty strip.
pub fn picture(img: &LightsImage, scale: f64) -> Option<Retained<NSImage>> {
    let w = crate::slots(img.width);
    let h = crate::slots(img.height);
    if w == 0 || h == 0 || scale <= 0.0 {
        return None;
    }
    let side = |v: usize| isize::try_from(v).ok();
    // SAFETY: no planes, so the rep allocates its own w by h by 4 bytes;
    // the colour space name is an extern static of the bindings.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            core::ptr::null_mut(),
            side(w)?,
            side(h)?,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            side(w * 4)?,
            32,
        )
    }?;
    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    // SAFETY: the buffer the rep allocated, w * h * 4 bytes in rows of
    // w * 4, which nothing else holds while it is written.
    let bytes = unsafe { core::slice::from_raw_parts_mut(data, w * h * 4) };
    bytes.copy_from_slice(&rgba(img));
    let points = NSSize::new(w as f64 / scale, h as f64 / scale);
    rep.setSize(points);
    let image = NSImage::initWithSize(NSImage::alloc(), points);
    image.addRepresentation(&rep);
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_font::Px;
    use macos_liquid_glass::objc2_foundation::NSString;

    fn named(name: &str) -> Retained<NSAppearance> {
        NSAppearance::appearanceNamed(&NSString::from_str(name)).expect("a system appearance")
    }

    /// **AppKit's own colours, resolved headless under each appearance**:
    /// green for what comes in, orange for what goes out and blue for a
    /// state, the same within each direction, lit colours that differ
    /// between light and dark, opaque, and letters that are AppKit's
    /// secondary label, translucent.
    #[test]
    fn the_lights_take_appkits_colours_under_each_appearance() {
        let (lit_light, label_light) = colours(&named("NSAppearanceNameAqua"));
        let (lit_dark, label_dark) = colours(&named("NSAppearanceNameDarkAqua"));
        for lit in [lit_light, lit_dark] {
            for (led, &c) in lit.iter() {
                assert_eq!(c.a().get(), 0xFF, "{led:?}");
                let (r, g, b) = (c.r().get(), c.g().get(), c.b().get());
                match direction(led) {
                    Direction::In => assert!(g > r && g > b, "{led:?} {c:?} is green"),
                    Direction::Out => assert!(r > g && g > b, "{led:?} {c:?} is orange"),
                    Direction::State => assert!(b > g && g > r, "{led:?} {c:?} is blue"),
                }
            }
            assert_eq!(lit.rd, lit.hs, "in, one colour");
            assert_eq!((lit.sd, lit.aa), (lit.cs, lit.cs), "out, one colour");
            assert_eq!((lit.cd, lit.oh, lit.fax), (lit.syn, lit.syn, lit.syn));
        }
        assert_ne!(lit_light, lit_dark, "each appearance its own");
        assert!(label_light.a().get() < 0xFF && label_dark.a().get() < 0xFF);
        assert!(label_light.r().get() < 0x80 && label_dark.r().get() > 0x80);
    }

    /// AppKit's accessibility appearances, light and dark: the ones a
    /// window takes while Increase Contrast is on, which no public name
    /// gives -- `appearanceNamed` with the accessibility names gives the
    /// standard colours headless -- so they come from AppKit's own class
    /// methods.
    fn accessible() -> (Retained<NSAppearance>, Retained<NSAppearance>) {
        use macos_liquid_glass::objc2::runtime::Bool;
        use macos_liquid_glass::objc2::{ClassType, msg_send};
        // SAFETY: the class methods take a BOOL and return an autoreleased
        // appearance or nil, which the Option holds.
        let (light, dark): (Option<Retained<NSAppearance>>, Option<_>) = unsafe {
            (
                msg_send![NSAppearance::class(), _aquaAppearanceWithAccessibility: Bool::YES],
                msg_send![NSAppearance::class(), _darkAquaAppearanceWithAccessibility: Bool::YES],
            )
        };
        let want = "AppKit's accessibility appearance";
        (light.expect(want), dark.expect(want))
    }

    /// **The strip's colours read on the standard material under every
    /// appearance AppKit resolves, Increase Contrast's too**: AppKit's
    /// colours resolved headless under Aqua and Dark Aqua and under their
    /// accessibility forms, each lit colour lifted to 3:1 and the letters
    /// to 4.5:1 against every grey of that half's material
    /// (`baud_render::lights::strip_colours`). The controls: the
    /// accessibility appearances are what they say -- named so, read as
    /// light or dark as the window's are, and giving colours the standard
    /// ones do not -- and AppKit's own colours read under 3:1 there, light
    /// with Increase Contrast's at about 2.6. Perturbation: draw AppKit's
    /// colours unlifted and this goes red.
    #[test]
    fn the_strips_colours_read_under_every_appearance() {
        use baud_render::lights::strip_colours;
        use baud_render::{Backdrop, GROUND_DARK, GROUND_LIGHT, contrast_over};
        let (aqua, dark) = (
            named("NSAppearanceNameAqua"),
            named("NSAppearanceNameDarkAqua"),
        );
        let (aqua_ic, dark_ic) = accessible();
        assert_eq!(
            aqua_ic.name().to_string(),
            "NSAppearanceNameAccessibilityAqua"
        );
        assert_eq!(
            dark_ic.name().to_string(),
            "NSAppearanceNameAccessibilityDarkAqua"
        );
        assert_ne!(
            colours(&aqua_ic),
            colours(&aqua),
            "the control: light's own"
        );
        assert_ne!(colours(&dark_ic), colours(&dark), "the control: dark's own");
        let mut short = 0;
        for (name, appearance, is_dark) in [
            ("light", &aqua, false),
            ("dark", &dark, true),
            ("light, Increase Contrast", &aqua_ic, false),
            ("dark, Increase Contrast", &dark_ic, true),
        ] {
            assert_eq!(macos_liquid_glass::is_dark(appearance), is_dark, "{name}");
            let (ground, backdrop) = if is_dark {
                (GROUND_DARK, Backdrop::under_window_dark())
            } else {
                (GROUND_LIGHT, Backdrop::under_window_light())
            };
            let (asked, label) = colours(appearance);
            let (lit, letters) = strip_colours(asked, label, ground, backdrop);
            let worst = |c: Argb| {
                backdrop
                    .greys()
                    .map(|g| contrast_over(c, g, Chan::from_u8(0xFF), Backdrop::any()))
                    .fold(f64::MAX, f64::min)
            };
            for (led, &c) in lit.iter() {
                assert!(worst(c) >= 3.0, "{name} {led:?} {c:?}: {:.3}", worst(c));
                short += usize::from(worst(*asked.get(led)) < 3.0);
            }
            assert!(
                worst(letters) >= 4.5,
                "{name} letters: {:.3}",
                worst(letters)
            );
        }
        assert!(short >= 18, "the control: AppKit's own fall short, {short}");
    }

    /// **The picture holds the strip's pixels, premultiplied, at its size
    /// in points**: a half-transparent red pixel and an opaque blue one
    /// become the bytes a bitmap with alpha holds, and the picture of two
    /// by one pixels at 2x is one point by half a point.
    #[test]
    fn the_picture_is_the_strips_pixels_at_its_size() {
        let img = LightsImage {
            width: Px::of(2),
            height: Px::of(1),
            pixels: vec![Argb(0x80_FF_00_00), Argb(0xFF_00_00_FF)],
        };
        assert_eq!(rgba(&img), vec![0x80, 0, 0, 0x80, 0, 0, 0xFF, 0xFF]);
        let image = picture(&img, 2.0).expect("a picture");
        assert_eq!(image.size(), NSSize::new(1.0, 0.5));
        assert!(picture(&LightsImage::new(), 2.0).is_none());
    }
}
