//! **The activity lights in the title strip**: the picture an image view
//! shows, made from the pixels `baud_render::lights` draws.
//!
//! Every colour in them is the theme's -- each lamp one of its sixteen
//! palette entries, the plate its background, the letters its foreground
//! (`baud_render::LightsLook::of`) -- so the strip draws what the lights'
//! own line draws on Linux, and no AppKit colour is read for it. On the
//! plate, which is opaque, a lamp reads against the theme's background
//! whatever the window's material shows behind the strip.
//!
//! `unsafe` is allowed here, as in [`crate::print`], for the one bitmap
//! constructor the bindings mark so and the buffer it hands back.
#![allow(unsafe_code)]

use baud_render::LightsImage;
use macos_liquid_glass::objc2::AnyThread;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage};
use macos_liquid_glass::objc2_foundation::NSSize;

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
    use baud_render::Argb;

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
