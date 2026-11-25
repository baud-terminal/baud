//! **The rows of a surface a frame is drawn into**: the grid's rows and the
//! bands around them that are not grid rows, as one `Copy` set
//! ([`SurfaceRows`]). It names both what a lent buffer is missing ([`Age`])
//! and what a draw painted into it ([`Painted`]), and
//! [`crate::Renderer::bands`] turns it into pixel rows through the
//! renderer's layout.
//!
//! Top to bottom a surface is: the band above the whole frame
//! ([`ChromeBands::TOP`]), the whole frame -- the activity lights' line
//! ([`ChromeBands::LIGHTS`]) above or below the grid's rows, and below the
//! main display's rows the status line's row and the notice row
//! ([`ChromeBands::STATUS`]) -- and the band below it to the surface's
//! bottom edge ([`ChromeBands::BOTTOM`]). Every band is the surface's whole
//! width, the margins, the strip, the gutter and the letterbox columns
//! beside the rows included.

use baud_domain::{DirtyRows, Row, Rows};
use baud_font::Px;

use crate::units::Band;

/// A set of the surface's bands that are not rows of the main display.
///
/// Its bits are private: a set is built only from the named bands and their
/// `union`, so no bit outside the four is ever set.
///
/// ```compile_fail,E0423
/// let _ = baud_render::ChromeBands(1);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_render::ChromeBands;
/// let chrome = ChromeBands::TOP.union(ChromeBands::STATUS);
/// assert!(chrome.contains(ChromeBands::STATUS));
/// assert!(!chrome.contains(ChromeBands::LIGHTS));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChromeBands(u8);

impl ChromeBands {
    /// No band.
    pub const NONE: ChromeBands = ChromeBands(0);
    /// The band above the whole frame: the gutter and the letterbox above.
    pub const TOP: ChromeBands = ChromeBands(1);
    /// The activity lights' line, above or below the grid's rows.
    pub const LIGHTS: ChromeBands = ChromeBands(1 << 1);
    /// The status line's row and the notice row, below the main display.
    pub const STATUS: ChromeBands = ChromeBands(1 << 2);
    /// The band below the whole frame: the gutter and the letterbox below.
    pub const BOTTOM: ChromeBands = ChromeBands(1 << 3);

    /// Every band in either set.
    pub fn union(self, other: ChromeBands) -> ChromeBands {
        ChromeBands(self.0 | other.0)
    }

    /// Whether every band in `other` is in the set.
    pub fn contains(self, other: ChromeBands) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Rows of a surface: rows of the main display by index, and bands that are
/// not.
///
/// A grid row at or past the main display's rows names the frame row it
/// indexes, the status line's or the notice's; one past the frame names
/// nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurfaceRows {
    pub grid: DirtyRows,
    pub chrome: ChromeBands,
}

impl SurfaceRows {
    /// No row.
    pub const NONE: SurfaceRows = SurfaceRows {
        grid: DirtyRows::NONE,
        chrome: ChromeBands::NONE,
    };

    /// Every row in either set.
    pub fn union(self, other: SurfaceRows) -> SurfaceRows {
        SurfaceRows {
            grid: self.grid.union(other.grid),
            chrome: self.chrome.union(other.chrome),
        }
    }
}

/// What a lent buffer holds of the frame before: nothing to keep
/// (`Whole`), or every pixel but the rows named (`Stale`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Age {
    /// The buffer is new, or what it holds is unknown: every pixel is
    /// painted.
    Whole,
    /// The buffer holds the last frame but these rows.
    Stale(SurfaceRows),
}

/// **The rows a draw into a lent buffer painted**, every pixel of each at
/// the surface's whole width: what a presenter premultiplies, damages and
/// keeps to age the buffer by. Only the renderer's draw builds one, so a
/// presenter cannot name rows the draw did not paint.
///
/// ```compile_fail,E0423
/// let _ = baud_render::Painted(baud_render::SurfaceRows::NONE);
/// ```
///
/// ```compile_fail,E0599
/// let _ = baud_render::Painted::NONE;
/// ```
///
/// The legal path, reading one:
///
/// ```
/// use baud_render::{Band, Painted, Renderer};
/// fn to_premultiply(r: &Renderer<'_, '_>, painted: Painted) -> Vec<Band> {
///     r.bands(&painted.rows()).collect()
/// }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Painted(SurfaceRows);

impl Painted {
    /// What the draw painted.
    #[cfg(test)]
    pub(crate) fn of(rows: SurfaceRows) -> Painted {
        Painted(rows)
    }

    /// Every row either draw painted.
    pub fn union(self, other: Painted) -> Painted {
        Painted(self.0.union(other.0))
    }

    /// The rows painted.
    pub fn rows(self) -> SurfaceRows {
        self.0
    }
}

/// The surface a lent buffer is: its size, and where the whole frame's top
/// left corner is in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Surface {
    pub(crate) size: (Px, Px),
    pub(crate) origin: (Px, Px),
}

/// **Where each band lies down a surface**, in its pixels: the whole
/// frame's height and its top in the surface, and in the whole frame the
/// grid's top, the lights' line and the frame's rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Layout {
    pub(crate) height: Px,
    pub(crate) top: Px,
    pub(crate) whole: Px,
    pub(crate) grid_top: Px,
    pub(crate) lights: Band,
    pub(crate) cell_h: Px,
    pub(crate) main: Rows,
    pub(crate) rows: Rows,
}

/// One piece of a surface's height, top to bottom: the band above, the
/// lights' line above the grid, each frame row, the lights' line below it
/// and the band below.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Piece {
    Top,
    LightsAbove,
    Frame(Row),
    LightsBelow,
    Bottom,
}

impl Piece {
    /// The piece after this one in a frame of `rows` rows, or none past
    /// the band below.
    fn next(self, rows: Rows) -> Option<Piece> {
        match self {
            Piece::Top => Some(Piece::LightsAbove),
            Piece::LightsAbove if rows.index() > 0 => Some(Piece::Frame(Row::ZERO)),
            Piece::LightsAbove => Some(Piece::LightsBelow),
            Piece::Frame(r) if r.index() + 1 < rows.index() => {
                Some(Piece::Frame(Row::from_index(r.index() + 1)))
            }
            Piece::Frame(_) => Some(Piece::LightsBelow),
            Piece::LightsBelow => Some(Piece::Bottom),
            Piece::Bottom => None,
        }
    }
}

impl Layout {
    /// A pixel offset `by` down from `at`, held to the surface.
    fn down(self, at: Px, by: Px) -> Px {
        Px::of(
            at.value()
                .saturating_add(by.value())
                .min(self.height.value()),
        )
    }

    /// Where `piece` lies, in the surface's pixels: empty when it is not
    /// drawn or lies past the surface's bottom.
    fn band(self, piece: Piece) -> Band {
        let at = |top: Px, h: Px| {
            let top = self.down(self.top, top);
            Band {
                top,
                bottom: self.down(top, h),
            }
        };
        let none = Px::of(0);
        let above = self.grid_top.value() > self.lights.top.value();
        let lights = Px::of(
            self.lights
                .bottom
                .value()
                .saturating_sub(self.lights.top.value()),
        );
        match piece {
            Piece::Top => Band {
                top: none,
                bottom: self.down(none, self.top),
            },
            Piece::LightsAbove if above => at(self.lights.top, lights),
            Piece::LightsBelow if !above => at(self.lights.top, lights),
            Piece::LightsAbove | Piece::LightsBelow => at(self.lights.top, none),
            Piece::Frame(r) => {
                let ch = self.cell_h.value();
                let y = u32::try_from(r.index())
                    .unwrap_or(u32::MAX)
                    .saturating_mul(ch);
                at(Px::of(self.grid_top.value().saturating_add(y)), self.cell_h)
            }
            Piece::Bottom => Band {
                top: self.down(self.top, self.whole),
                bottom: self.height,
            },
        }
    }

    /// Whether `rows` names `piece`.
    fn named(self, piece: Piece, rows: SurfaceRows) -> bool {
        match piece {
            Piece::Top => rows.chrome.contains(ChromeBands::TOP),
            Piece::LightsAbove | Piece::LightsBelow => rows.chrome.contains(ChromeBands::LIGHTS),
            Piece::Frame(r) => {
                rows.grid.contains(r)
                    || (r.index() >= self.main.index() && rows.chrome.contains(ChromeBands::STATUS))
            }
            Piece::Bottom => rows.chrome.contains(ChromeBands::BOTTOM),
        }
    }

    /// **The pixel rows `rows` names**, top to bottom, each run of named
    /// pieces as one band: no two overlap, and none is empty.
    pub(crate) fn bands(self, rows: SurfaceRows) -> impl Iterator<Item = Band> {
        let mut at = Some(Piece::Top);
        core::iter::from_fn(move || {
            let mut run: Option<Band> = None;
            while let Some(piece) = at {
                let band = self.band(piece);
                let empty = band.top == band.bottom;
                match (self.named(piece, rows) && !empty, run.as_mut()) {
                    (true, Some(open)) => open.bottom = band.bottom,
                    (true, None) => run = Some(band),
                    (false, Some(_)) if !empty => return run,
                    (false, _) => {}
                }
                at = piece.next(self.rows);
            }
            run
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Blend, PxSize, Renderer, Stack, Theme};
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Cols, ReadLen, WinSize};
    use baud_font::Face;

    const WS: WinSize = WinSize {
        ws_col: 16,
        ws_row: 6,
    };

    fn all() -> ChromeBands {
        ChromeBands::TOP
            .union(ChromeBands::LIGHTS)
            .union(ChromeBands::STATUS)
            .union(ChromeBands::BOTTOM)
    }

    /// The bands each row of the main display and each chrome band names
    /// alone, one list, sorted by their tops.
    fn pieces(r: &Renderer<'_, '_>) -> Vec<(String, Band)> {
        let one_row = |y: usize| SurfaceRows {
            grid: DirtyRows::NONE.with(Row::from_index(y)).unwrap(),
            chrome: ChromeBands::NONE,
        };
        let chrome = |c: ChromeBands| SurfaceRows {
            grid: DirtyRows::NONE,
            chrome: c,
        };
        let named = (0..r.main_rows().index())
            .map(|y| (format!("row {y}"), one_row(y)))
            .chain([
                ("top".to_string(), chrome(ChromeBands::TOP)),
                ("lights".to_string(), chrome(ChromeBands::LIGHTS)),
                ("status".to_string(), chrome(ChromeBands::STATUS)),
                ("bottom".to_string(), chrome(ChromeBands::BOTTOM)),
            ]);
        let mut out: Vec<(String, Band)> = named
            .flat_map(|(name, rows)| {
                r.bands(&rows)
                    .map(move |b| (name.clone(), b))
                    .collect::<Vec<_>>()
            })
            .collect();
        out.sort_by_key(|(_, b)| b.top.value());
        out
    }

    /// Asserts the bands run from the surface's top to its bottom edge
    /// with no gap and no overlap.
    fn tiles(what: &str, bands: &[(String, Band)], h: Px) {
        let mut y = Px::of(0);
        for (name, b) in bands {
            assert_eq!(b.top, y, "{what}: {name} starts where the band above ends");
            assert!(
                b.bottom.value() > b.top.value(),
                "{what}: {name} is not empty"
            );
            y = b.bottom;
        }
        assert_eq!(y, h, "{what}: the bands reach the surface's bottom edge");
    }

    /// **The bands of every row of the main display and every chrome band
    /// cover the surface's rows exactly once**: the gutter and letterbox
    /// above and below, the lights' line above the grid and then below
    /// it, and the status line's row, each named alone; and all of them
    /// named at once is the whole surface, one band.
    #[test]
    fn bands_tile_the_surface() {
        let data = std::fs::read(format!(
            "{}/../baud-app/fonts/built/Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let faces = [Face::parse(&data).unwrap()];
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let open = b"\x1b[2$~";
        t.write_at(BytePos::ZERO, open);
        t.feed(BytePos::ZERO, ReadLen::from_index(open.len()));
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(16),
            Rows::of_index(6),
        )
        .unwrap();
        r.fit(&t);
        assert_eq!(
            t.status_rows(),
            Rows::of_index(1),
            "the premise: a status line's row below the main display"
        );
        for edge in [crate::LineEdge::Top, crate::LineEdge::Bottom] {
            r.set_lights_line(Some(edge), 2.0);
            assert!(r.lights_height().value() > 0, "the premise: a lights line");
            let (w, h) = r.whole_size();
            let size = (Px::of(w.value() + 30), Px::of(h.value() + 23));
            r.set_surface(size, (Px::of(15), Px::of(9)));
            let what = format!("lights at {edge:?}");
            let bands = pieces(&r);
            assert_eq!(
                bands.len(),
                r.main_rows().index() + 4,
                "{what}: one band each: {bands:?}"
            );
            tiles(&what, &bands, size.1);
            let every = SurfaceRows {
                grid: DirtyRows::NONE
                    .span(Row::ZERO, Row::from_index(r.main_rows().index()))
                    .unwrap(),
                chrome: all(),
            };
            let whole: Vec<Band> = r.bands(&every).collect();
            assert_eq!(
                whole,
                [Band {
                    top: Px::of(0),
                    bottom: size.1
                }],
                "{what}: everything named is one band, the surface"
            );
        }
    }

    /// **A surface no taller than the whole frame has no band above or
    /// below**: with no surface set it is the whole frame, and the main
    /// display's rows, the status row and the lights' line tile it.
    #[test]
    fn the_whole_frame_is_the_surface_until_one_is_set() {
        let data = std::fs::read(format!(
            "{}/../baud-app/fonts/built/Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let faces = [Face::parse(&data).unwrap()];
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(16),
            Rows::of_index(6),
        )
        .unwrap();
        r.set_lights_line(Some(crate::LineEdge::Bottom), 2.0);
        let bands = pieces(&r);
        assert_eq!(bands.len(), r.main_rows().index() + 1, "{bands:?}");
        tiles("no surface set", &bands, r.whole_size().1);
        let rows = SurfaceRows {
            grid: DirtyRows::NONE,
            chrome: ChromeBands::TOP.union(ChromeBands::BOTTOM),
        };
        assert_eq!(r.bands(&rows).count(), 0, "no band above or below");
    }

    /// **What two draws painted is every row either did**, and reads back
    /// as rows.
    #[test]
    fn painted_unions_what_each_draw_painted() {
        let row = |y: usize| DirtyRows::NONE.with(Row::from_index(y)).unwrap();
        let a = Painted::of(SurfaceRows {
            grid: row(2),
            chrome: ChromeBands::TOP,
        });
        let b = Painted::of(SurfaceRows {
            grid: row(5),
            chrome: ChromeBands::STATUS,
        });
        let both = a.union(b).rows();
        assert!(both.grid.contains(Row::from_index(2)) && both.grid.contains(Row::from_index(5)));
        assert!(
            both.chrome
                .contains(ChromeBands::TOP.union(ChromeBands::STATUS))
        );
        assert!(!both.chrome.contains(ChromeBands::LIGHTS));
        assert_eq!(a.rows().union(b.rows()), both);
    }
}
