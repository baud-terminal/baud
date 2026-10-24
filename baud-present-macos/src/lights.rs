//! **The activity lights in the title strip**: AppKit's colours for them,
//! resolved under the window's appearance, and the picture an image view
//! shows, made from the pixels `baud_render::lights` draws.
//!
//! The colours are AppKit's dynamic system colours -- `systemGreenColor`
//! for RD, `systemOrangeColor` for SD, `secondaryLabelColor` for the
//! letters -- resolved under the appearance as the unemphasized selection
//! is ([`crate::selection`]): "Prefer system-defined colors. These colors
//! have their own accessible variants that automatically adapt when people
//! adjust their color preferences, such as enabling Increase Contrast or
//! toggling between the light and dark appearances"
//! (`[[source: apple-hig-accessibility]]`).
//!
//! `unsafe` is allowed here, as in [`crate::print`], for the one bitmap
//! constructor the bindings mark so and the buffer it hands back.
#![allow(unsafe_code)]

use baud_domain::Chan;
use baud_render::{Argb, LightsImage};
use block2::RcBlock;
use core::cell::Cell;
use macos_liquid_glass::objc2::AnyThread;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSAppearance, NSBitmapImageRep, NSColor, NSColorSpace, NSDeviceRGBColorSpace, NSImage,
};
use macos_liquid_glass::objc2_foundation::NSSize;

/// RD's and SD's lit colours and the letters' colour, under `appearance`.
pub fn colours(appearance: &NSAppearance) -> ([Argb; 2], Argb) {
    let out = Cell::new(([Argb::default(); 2], Argb::default()));
    let resolve = RcBlock::new(|| {
        out.set((
            [
                srgb(&NSColor::systemGreenColor()),
                srgb(&NSColor::systemOrangeColor()),
            ],
            srgb(&NSColor::secondaryLabelColor()),
        ));
    });
    appearance.performAsCurrentDrawingAppearance(&resolve);
    out.get()
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
    /// green and orange lit colours that differ between light and dark,
    /// opaque, and letters that are AppKit's secondary label, translucent.
    #[test]
    fn the_lights_take_appkits_colours_under_each_appearance() {
        let (lit_light, label_light) = colours(&named("NSAppearanceNameAqua"));
        let (lit_dark, label_dark) = colours(&named("NSAppearanceNameDarkAqua"));
        for lit in [lit_light, lit_dark] {
            let [rd, sd] = lit;
            assert_eq!((rd.a().get(), sd.a().get()), (0xFF, 0xFF));
            assert!(
                rd.g().get() > rd.r().get() && rd.g().get() > rd.b().get(),
                "{rd:?} is green"
            );
            assert!(
                sd.r().get() > sd.g().get() && sd.g().get() > sd.b().get(),
                "{sd:?} is orange"
            );
        }
        assert_ne!(lit_light, lit_dark, "each appearance its own");
        assert!(label_light.a().get() < 0xFF && label_dark.a().get() < 0xFF);
        assert!(label_light.r().get() < 0x80 && label_dark.r().get() > 0x80);
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
