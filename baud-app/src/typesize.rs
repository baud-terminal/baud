//! **How many pixels a point is on Linux**: 96 dots to the inch times the
//! desktop's text-scaling factor, so baud's `font_size` draws the size the
//! same number draws in the desktop's other windows.
//!
//! The factor is `org.gnome.desktop.interface text-scaling-factor`, read
//! through the settings portal; with no portal or no key it is 1, so a
//! point is 96 dpi, and never the 72 a macOS point is. A point is
//! `size * dpi / 72` pixels.
//!
//! [`px_per_point`] is pure: the Linux driver fills a [`TypeSnapshot`] from
//! the portal and the output, and tests and the headless harnesses pass
//! fixed ones; nothing here reads the host. The factor reaches typographic
//! sizes only -- the font, the lights' line and the cursor guide -- never
//! the backing scale the core answers `ReportCellSize` from, nor the
//! flash clause the line lamps' words meet, which measure what is on
//! screen at the compositor's scale.

/// A point at 96 dots to the inch, where a macOS point is 72: the factor a
/// desktop that reports nothing gets.
pub(crate) const DESKTOP_DPI: f64 = 96.0;

use crate::config::FontDpi;

/// **What the desktop said**, as the driver has it now: the portal's
/// text-scaling factor, and the output's physical dots to the inch, each
/// `None` until known or where there is none.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct TypeSnapshot {
    pub(crate) text_scale: Option<f64>,
    pub(crate) physical: Option<f64>,
    pub(crate) mode: FontDpi,
}

impl TypeSnapshot {
    /// Before the portal or the output has answered: the desktop mode with
    /// nothing known, which is 96 dpi.
    pub(crate) const UNTOLD: TypeSnapshot = TypeSnapshot {
        text_scale: None,
        physical: None,
        mode: FontDpi::Desktop,
    };
}

/// The physical dots to the inch an output can plausibly have: a size in
/// millimetres outside this is a television's or a projector's guess, and
/// the desktop's answer is used instead.
pub(crate) const PHYSICAL_SANE: core::ops::RangeInclusive<f64> = 50.0..=500.0;

/// A text-scaling factor outside this is not a setting a desktop offers:
/// the key ranges from 0.5 to 3. Out of it, the factor is taken as 1.
pub(crate) const TEXT_SCALE_SANE: core::ops::RangeInclusive<f64> = 0.5..=3.0;

/// What [`px_per_point`] fell back from, for the config notice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Fallback {
    /// Nothing fell back.
    None,
    /// `"physical"` was asked and the output's size is unknown or not
    /// plausible: the desktop's answer is used.
    Physical,
    /// The portal's factor is not plausible: 1 is used.
    TextScale,
}

/// **The frame's pixels to one point**, at the compositor's `scale`, and
/// what fell back. Desktop: 96/72 times the text-scaling factor, times the
/// scale; fixed: the number over 72, times the scale; physical: the
/// output's dots over 72 and not times the scale, the output's dots being
/// its hardware pixels, which the buffer at the scale already is -- else
/// the desktop's.
pub(crate) fn px_per_point(s: TypeSnapshot, scale: f64) -> (f64, Fallback) {
    let desktop = || match s.text_scale {
        Some(f) if TEXT_SCALE_SANE.contains(&f) => (DESKTOP_DPI * f, Fallback::None),
        Some(_) => (DESKTOP_DPI, Fallback::TextScale),
        None => (DESKTOP_DPI, Fallback::None),
    };
    let logical = |(dpi, fell): (f64, Fallback)| (dpi / 72.0 * scale, fell);
    match s.mode {
        FontDpi::Fixed(d) => logical((d, Fallback::None)),
        FontDpi::Desktop => logical(desktop()),
        FontDpi::Physical => match s.physical {
            Some(d) if PHYSICAL_SANE.contains(&d) => (d / 72.0, Fallback::None),
            _ => logical((desktop().0, Fallback::Physical)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text_scale: Option<f64>, physical: Option<f64>, mode: FontDpi) -> TypeSnapshot {
        TypeSnapshot {
            text_scale,
            physical,
            mode,
        }
    }

    /// **The desktop's point, at every scale**: no portal answer is 96 dpi
    /// (never 72); a factor multiplies it; a fixed number and a plausible
    /// physical size are used as given; an implausible factor is 1 and an
    /// implausible or unknown physical size is the desktop's, each saying
    /// so. Perturbation: return 72 for an
    /// untold desktop, a macOS point, and the first row goes red.
    #[test]
    fn a_point_is_the_desktops_96_dpi_times_its_text_scale() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        for scale in [1.0, 1.5, 2.0] {
            let rows = [
                (TypeSnapshot::UNTOLD, 96.0, Fallback::None),
                (
                    snap(Some(1.25), None, FontDpi::Desktop),
                    120.0,
                    Fallback::None,
                ),
                (
                    snap(Some(1.0), None, FontDpi::Fixed(110.0)),
                    110.0,
                    Fallback::None,
                ),
                (
                    snap(Some(9.0), None, FontDpi::Desktop),
                    96.0,
                    Fallback::TextScale,
                ),
                (
                    snap(None, None, FontDpi::Physical),
                    96.0,
                    Fallback::Physical,
                ),
                (
                    snap(Some(1.5), Some(4000.0), FontDpi::Physical),
                    144.0,
                    Fallback::Physical,
                ),
            ];
            for (s, dpi, fell) in rows {
                let (px, got) = px_per_point(s, scale);
                assert!(close(px, dpi / 72.0 * scale), "{s:?} at {scale}: {px}");
                assert_eq!(got, fell, "{s:?}");
            }
        }
        // A desktop at 96 dpi: 13 pt at scale 1 is 17.3 px, a DejaVu Sans
        // Mono cell 10 px wide.
        let (px, _) = px_per_point(TypeSnapshot::UNTOLD, 1.0);
        assert!(close(13.0 * px, 13.0 * 96.0 / 72.0));
    }

    /// **A physical point is the output's own pixels, at any scale**: the
    /// output's dots to the inch are its hardware pixels, which the buffer
    /// at the compositor's scale already is, so the scale does not
    /// multiply them again. A 27-inch 4K output, 3840 pixels over 597.7
    /// mm, is 163.2 dpi: at scale 2 a point is 163.2/72 = 2.27 buffer
    /// pixels, its physical size, and 139 dpi is 139/72 at 1, 1.5 and 2.
    /// Perturbation: multiply by the scale -- the 4K point is 4.53 pixels,
    /// twice its physical size.
    #[test]
    fn a_physical_point_is_the_outputs_pixels_at_every_scale() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let uhd = 3840.0 * 25.4 / 597.7;
        let (px, fell) = px_per_point(snap(None, Some(uhd), FontDpi::Physical), 2.0);
        assert!(close(px, uhd / 72.0), "{px}");
        assert_eq!(fell, Fallback::None);
        for scale in [1.0, 1.5, 2.0] {
            let (px, _) = px_per_point(snap(None, Some(139.0), FontDpi::Physical), scale);
            assert!(close(px, 139.0 / 72.0), "at {scale}: {px}");
        }
    }

    /// **The Linux driver follows the output's size between configures**:
    /// after each turn's events it compares the size the window reads now
    /// with the one the point was sized at and retypes where they differ,
    /// so a `wl_output.geometry` past the last configure, or the surface
    /// entering another output, resizes the text. The window's own reading
    /// is the handshake test
    /// `the_physical_dpi_is_the_entered_outputs_mode_over_its_millimetres`.
    /// The physical fallback is said once. Perturbation: drop the call
    /// after the events -- the size is read only at the next configure.
    #[test]
    fn the_driver_follows_the_outputs_size_after_each_turns_events() {
        let linux = include_str!("linux.rs");
        assert!(linux.contains(concat!(
            "        crate::host::set_hebrew_layout(&mut d.term, d.window.hebrew_layout());\n",
            "        // The output's size under `[font] dpi = \"physical\"`, after the\n",
            "        // events: a `wl_output.geometry` past the last configure, or the\n",
            "        // surface entering another output.\n",
            "        d.follow_output();\n",
        )));
        assert!(linux.contains(concat!(
            "        let physical = self.window.physical_dpi();\n",
            "        if physical != self.type_size.physical {\n",
            "            self.type_size.physical = physical;\n",
            "            self.retype();\n",
        )));
        assert_eq!(linux.matches("self.unmeasured = Said::Once;").count(), 1);
    }
}
