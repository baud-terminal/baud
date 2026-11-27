//! **iTerm2's cursor guide**: "a horizontal rule that indicates the
//! vertical position of the cursor" (`[[source: iterm2-doc-profiles-colors]]`),
//! which `OSC 1337 ; HighlightCursorLine=yes` shows and `=no` hides
//! (`[[source: iterm2-escape-codes]]`).
//!
//! **Drawn as iTerm2 draws it.** Its colour fills the frame's width one
//! cell high on the cursor's row, and a line one point high at the row's
//! top and at its bottom is filled again, so those two lines carry the
//! colour twice (`drawCursorGuideForColumns`, `[[source:
//! iterm2-7ce037e-text-drawing-helper]]`; the Metal path's texture the
//! same, `[[source: iterm2-964a75d-cursor-guide-renderer]]`). It lies over
//! the backgrounds and the images under the text, and under the cursor and
//! the text: Powerline glyphs are off in iTerm2's default profile, and
//! with them off it is drawn below the text (`shouldDrawCursorGuideBelowText`,
//! `[[source: iterm2-964a75d-metal-per-frame-state]]`), and the cursor
//! after it (`[[source: iterm2-964a75d-metal-driver]]`). A block cursor's
//! cell is left out of the band, since iTerm2's block covers the guide
//! there and baud draws a block under its records.
//!
//! Filling a colour twice at alpha `a` leaves the same pixel as filling it
//! once at `1 - (1 - a)^2`, so each line is one record at that alpha, and a
//! pixel is covered by one record.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use baud_domain::Chan;
use baud_domain::contrast::CurveRow;
use baud_font::Px;

/// **iTerm2's default guide colour**, calibrated RGB (0.650, 0.910, 1.000)
/// at 0.25 (`KEY_CURSOR_GUIDE_COLOR`, `[[source:
/// iterm2-964a75d-profile-preferences]]`), in sRGB through the calibrated
/// space's conversion `SetColors` uses (`themefile::gamut::to_srgb`), and
/// 0.25 of 255 to the nearest byte; pinned by
/// `the_guide_colour_is_iterm2s_default_in_srgb`.
pub const CURSOR_GUIDE: Argb = Argb(0x40B3_ECFF);

/// How the guide is drawn while it is shown: its colour, and the height in
/// device pixels of iTerm2's one-point line, the backing scale rounded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuideLook {
    pub colour: Argb,
    pub line: Px,
}

/// What the guide covers this frame, which decides the rows it repaints:
/// its row's top, the block cursor's columns it leaves out, and its look.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Placed {
    pub top: u32,
    pub hole: Option<(u32, u32)>,
    pub look: GuideLook,
}

/// The alpha of a colour filled twice at `a`, `1 - (1 - a)^2`, in bytes.
pub(crate) fn twice(a: Chan) -> Chan {
    let left = 255 - u32::from(a.get());
    let v = 255 - (left * left + 127) / 255;
    Chan::from_u8(u8::try_from(v).unwrap_or(u8::MAX))
}

/// The guide's records for a band `h` pixels high at `p.top`, across a
/// frame `width` pixels wide, `p.hole` left out: on each side of the hole
/// its top line, its middle and its bottom line.
pub(crate) fn records(p: Placed, h: Px, width: Px, out: &mut Vec<Instance>) {
    out.clear();
    let (h, width) = (h.value(), width.value());
    let line = p.look.line.value().clamp(1, h / 2);
    let (a, b) = (p.look.colour.a(), twice(p.look.colour.a()));
    let spans = match p.hole {
        Some((x, w)) => [(0, x.min(width)), (x.saturating_add(w).min(width), width)],
        None => [(0, width), (width, width)],
    };
    for (x0, x1) in spans {
        if x1 <= x0 {
            continue;
        }
        for (y, rh, alpha) in [
            (p.top, line, b),
            (p.top + line, h.saturating_sub(2 * line), a),
            (p.top + h.saturating_sub(line), line, b),
        ] {
            if rh == 0 {
                continue;
            }
            out.push(Instance {
                x: Screen(i16::try_from(x0).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
                w: Tex(u16::try_from(x1 - x0).unwrap_or(u16::MAX)),
                h: Tex(u16::try_from(rh).unwrap_or(u16::MAX)),
                u: Tex(0),
                v: Tex(0),
                color: p.look.colour.with_alpha(alpha),
                kind: Kind::Solid,
                curve: CurveRow::default(),
            });
        }
    }
}
