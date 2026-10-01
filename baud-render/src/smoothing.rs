//! macOS font smoothing, as the renderer is told to draw it (note J).
//!
//! Whether to smooth is the embedder's to say -- the app reads the
//! platform's setting and its own config -- never the renderer's to look
//! up, so a test or a bench draws the same pixels on every machine. When it
//! is on, every outline glyph is drawn grown for the luminance of the colour
//! it is finally painted in (`baud_font::Smoother`), which is that glyph's
//! [`crate::atlas::GlyphLook`].

use crate::atlas::GlyphLook;
use crate::color::Argb;
use baud_domain::Chan;
use baud_font::SmoothLevel;

/// Whether outline glyphs are drawn the way macOS's font smoothing draws
/// them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Smoothing {
    /// Each outline as it is: Linux, `AppleFontSmoothing = 0`, and every
    /// test that pins a frame.
    #[default]
    Off,
    /// Grown by CoreText's dilation for the text colour's level.
    CoreText,
}

impl Smoothing {
    /// The look an outline painted in `color` is drawn with.
    pub fn look(self, color: Argb) -> GlyphLook {
        match self {
            Smoothing::Off => GlyphLook::Plain,
            Smoothing::CoreText => GlyphLook::Smooth(level_of(color)),
        }
    }
}

/// The smoothing level of text painted in `color`: from the colour's linear
/// Rec.709 luminance, which `baud-font`'s `tests/smooth.rs` holds CoreText
/// to in 21 coloured fills. Alpha does not enter: CoreText keys on the fill.
pub fn level_of(color: Argb) -> SmoothLevel {
    // sRGB byte to linear light, exactly: a level boundary is a luminance
    // threshold, and the compositor's 12-bit table could put a colour near
    // one on the wrong side.
    static LINEAR: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    let l = LINEAR.get_or_init(|| {
        let mut t = [0.0f32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        t
    });
    let at = |c: Chan| l[usize::from(c.get())];
    SmoothLevel::of_luminance(
        0.2126 * at(color.r()) + 0.7152 * at(color.g()) + 0.0722 * at(color.b()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use SmoothLevel::*;

    /// A truecolour from its three bytes.
    fn rgb(c: [u8; 3]) -> Argb {
        Argb::rgb(
            Chan::from_u8(c[0]),
            Chan::from_u8(c[1]),
            Chan::from_u8(c[2]),
        )
    }

    /// The fills `baud-font`'s colour gate measured, at the levels CoreText
    /// drew them: Rec.709, where Rec.601 would put green at 2 and magenta
    /// at 2.
    #[test]
    fn the_level_is_rec709_as_coretext_drew_it() {
        let cases = [
            (rgb([0, 0, 0]), L0),
            (rgb([255, 255, 255]), L4),
            (rgb([0, 255, 0]), L3),
            (rgb([255, 0, 255]), L1),
            (rgb([0, 205, 0]), L2),
            (rgb([0, 0, 255]), L0),
            (rgb([255, 255, 0]), L4),
            (rgb([128, 0, 255]), L0),
            (rgb([0xd8, 0xd8, 0xd8]), L3),
        ];
        for (c, want) in cases {
            assert_eq!(level_of(c), want, "{:08x}", c.0);
        }
    }

    #[test]
    fn off_is_plain_whatever_the_colour() {
        assert_eq!(Smoothing::Off.look(rgb([255, 255, 255])), GlyphLook::Plain);
        assert_eq!(
            Smoothing::CoreText.look(rgb([255, 255, 255])),
            GlyphLook::Smooth(L4)
        );
    }
}
