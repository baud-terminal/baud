//! **What the picture under each text cell spans**: for every placement
//! drawn under the text, a table of its cells, each holding the range of
//! the pixels the placement draws there, composited over the page as the
//! frame shows it -- the darkest and the lightest luminance as two greys
//! ([`Spread::luminance`]), and the darkest and the lightest channel
//! ([`Spread::channels`]).
//!
//! **Built once a placement, never a frame**: a table is kept while its
//! [`Key`] holds -- the image, the placement, what it draws and how big,
//! the frame of an animation it shows, where the cell grid falls on it, and
//! the page it is composited over -- and while the image's pixels are the
//! ones it was built from ([`ImageRead::image_since`]). A frame whose
//! placements and theme are the last frame's builds none
//! ([`Counter::PictureRangesBuilt`]).
//!
//! **The pixels are the ones the frame draws**: each drawn pixel is the
//! texel [`ImageDraw::nearest`] names, the table every path samples
//! through, a fill word drawn as the placement's ground. A pixel below full
//! alpha is gathered over both ends of what can be under it; a cell the
//! picture covers only in part gathers those ends too, which the rest of it
//! shows.

use crate::color::{Argb, Backdrop, Spread};
use crate::counters::{Counter, Site};
use crate::renderer::{ImageComposite, ImagePlacement};
use crate::units::Tally;
use baud_domain::{
    Chan, Cols, FrameNo, ImageAge, ImageDraw, ImageGen, ImageId, ImageRead, ImageSince, Pixel,
    PlaceSerial, Rows,
};

/// What a table is built from: the image, the placement and what it draws,
/// the animation frame, the cell size and where the cells fall on the drawn
/// rectangle, how it meets what is under it, and the page as the frame
/// shows it.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Key {
    id: ImageId,
    born: ImageAge,
    serial: PlaceSerial,
    frame: FrameNo,
    draw: ImageDraw,
    /// The cell width and height, device pixels.
    cell: (u32, u32),
    /// How far into its first cell the drawn rectangle starts, across and
    /// down.
    phase: (u32, u32),
    /// What its fill words draw as.
    ground: Pixel,
    composite: ImageComposite,
    /// What the page shows under the picture: one colour, or two ends.
    under: Under,
}

/// **What is under a picture's pixels**: the page drawn opaque, or the
/// page at the window's dim over the two greys the window's material spans.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Under {
    Opaque(Argb),
    Glass {
        page: Argb,
        alpha: Chan,
        backdrop: Backdrop,
    },
}

impl Under {
    /// The colours the page can show: the page, or the page at its alpha
    /// over the darkest and over the lightest grey of the material.
    pub(crate) fn ends(self) -> [Argb; 2] {
        match self {
            Under::Opaque(page) => [page; 2],
            Under::Glass {
                page,
                alpha,
                backdrop,
            } => {
                let grey = |v: Chan| Argb::rgb(v, v, v);
                [
                    page.mix(grey(backdrop.dark()), alpha),
                    page.mix(grey(backdrop.light()), alpha),
                ]
            }
        }
    }

    /// The material's own two greys, under a placement that replaces the
    /// page.
    fn material(self) -> [Argb; 2] {
        match self {
            Under::Opaque(page) => [page; 2],
            Under::Glass { backdrop, .. } => {
                let grey = |v: Chan| Argb::rgb(v, v, v);
                [grey(backdrop.dark()), grey(backdrop.light())]
            }
        }
    }
}

/// One placement's table: a range a cell, `cols` by `rows`, row-major.
struct Table {
    key: Key,
    /// The store's generation it was built at, which the image is asked
    /// about ([`ImageRead::image_since`]).
    built: ImageGen,
    cols: Cols,
    rows: Rows,
    /// Each cell's luminance range and channel range.
    cells: Vec<(Backdrop, Backdrop)>,
}

/// **The tables of this frame's placements under the text**.
pub(crate) struct PictureRanges {
    tables: Vec<Table>,
    /// The last frame's, which a placement drawn alike takes again.
    spare: Vec<Table>,
    /// Which table each of the frame's placements reads, in their order:
    /// none for one over the text, or whose image is gone.
    of_place: Vec<Option<usize>>,
    /// The page's own colours, which a cell shows where the picture leaves
    /// it, as ranges.
    page: (Backdrop, Backdrop),
}

/// A rectangle of device pixels, its left and top inclusive and its right
/// and bottom exclusive.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PxBox {
    pub(crate) x: (i64, i64),
    pub(crate) y: (i64, i64),
}

impl Default for PictureRanges {
    fn default() -> PictureRanges {
        PictureRanges {
            tables: Vec::new(),
            spare: Vec::new(),
            of_place: Vec::new(),
            page: (Backdrop::any(), Backdrop::any()),
        }
    }
}

impl PxBox {
    fn meets(self, o: PxBox) -> bool {
        self.x.0 < o.x.1 && o.x.0 < self.x.1 && self.y.0 < o.y.1 && o.y.0 < self.y.1
    }

    fn inside(self, o: PxBox) -> bool {
        o.x.0 <= self.x.0 && self.x.1 <= o.x.1 && o.y.0 <= self.y.0 && self.y.1 <= o.y.1
    }
}

impl PictureRanges {
    /// **Brings the tables up to `places`**, those under the text: each
    /// keeps the table it had while its key holds and its image holds the
    /// pixels the table was built from, and is built one otherwise; a table
    /// no placement takes goes. `cell` is the cell width and height and
    /// `under` what the page shows.
    pub(crate) fn update(
        &mut self,
        imgs: &impl ImageRead,
        places: &[ImagePlacement],
        cell: (u32, u32),
        under: Under,
    ) {
        core::mem::swap(&mut self.tables, &mut self.spare);
        self.tables.clear();
        self.of_place.clear();
        let mut page = Spread::EMPTY;
        for c in under.ends() {
            page.add(c);
        }
        self.page = (page.luminance(), page.channels());
        let (cw, ch) = (cell.0.max(1), cell.1.max(1));
        let phase = |v: i32, side: u32| {
            u32::try_from(i64::from(v).rem_euclid(i64::from(side))).unwrap_or(0)
        };
        for p in places {
            if !p.layer.under_text() {
                self.of_place.push(None);
                continue;
            }
            let key = Key {
                id: p.id,
                born: p.born,
                serial: p.serial,
                frame: p.frame,
                draw: p.draw,
                cell: (cw, ch),
                phase: (phase(p.at.0, cw), phase(p.at.1, ch)),
                ground: p.ground,
                composite: p.composite,
                under,
            };
            if let Some(i) = self.tables.iter().position(|t| t.key == key) {
                self.of_place.push(Some(i));
                continue;
            }
            let kept = self.spare.iter().position(|t| {
                t.key == key && imgs.image_since(p.id, p.born, t.built) == ImageSince::Same
            });
            let table = match kept {
                Some(i) => Some(self.spare.swap_remove(i)),
                None => {
                    let t = Table::build(imgs, key, p.epoch);
                    if t.is_some() {
                        crate::counters::add(
                            Counter::PictureRangesBuilt,
                            Site::Images,
                            Tally::of(1),
                        );
                    }
                    t
                }
            };
            let at = table.map(|t| {
                self.tables.push(t);
                self.tables.len() - 1
            });
            self.of_place.push(at);
        }
        self.spare.clear();
    }

    /// **What the pictures under the text span behind `rect`**, a cell or
    /// the cells one character covers, on the default page: the union
    /// over every placement under the text that meets it of the ranges of
    /// the table cells it meets, and the page's own colours where it
    /// leaves a placement's visible part or its table. `places` are the
    /// frame's placements, in the order the tables were brought up to.
    /// None where no picture is behind it.
    pub(crate) fn behind(
        &self,
        places: &[ImagePlacement],
        rect: PxBox,
    ) -> Option<(Backdrop, Backdrop)> {
        let mut out: Option<(Backdrop, Backdrop)> = None;
        let mut add = |b: (Backdrop, Backdrop)| {
            out = Some(match out {
                None => b,
                Some(o) => (o.0.union(b.0), o.1.union(b.1)),
            });
        };
        for (p, t) in places.iter().zip(&self.of_place) {
            let Some(t) = t.and_then(|i| self.tables.get(i)) else {
                continue;
            };
            let (vx, vy) = (i64::from(p.x), i64::from(p.y));
            let shown = PxBox {
                x: (vx, vx + i64::from(p.w)),
                y: (vy, vy + i64::from(p.h)),
            };
            if !rect.meets(shown) {
                continue;
            }
            if !rect.inside(shown) {
                add(self.page);
            }
            let (cw, ch) = (i64::from(t.key.cell.0), i64::from(t.key.cell.1));
            let left = i64::from(p.at.0) - i64::from(t.key.phase.0);
            let top = i64::from(p.at.1) - i64::from(t.key.phase.1);
            let span = |(a, b): (i64, i64), from: i64, side: i64| {
                (a - from).div_euclid(side)..=(b - 1 - from).div_euclid(side)
            };
            for row in span(rect.y, top, ch) {
                for col in span(rect.x, left, cw) {
                    let r = usize::try_from(row).ok().filter(|&r| r < t.rows.index());
                    let c = usize::try_from(col).ok().filter(|&c| c < t.cols.index());
                    match r.zip(c) {
                        Some((r, c)) => add(t.cells[r * t.cols.index() + c]),
                        None => add(self.page),
                    }
                }
            }
        }
        out
    }
}

impl Table {
    /// The table of the placement `key` names, over its whole drawn
    /// rectangle; none when the store no longer holds its image.
    fn build(imgs: &impl ImageRead, key: Key, built: ImageGen) -> Option<Table> {
        let view = imgs.image(key.id)?;
        let (cw, ch) = key.cell;
        let (px, py) = key.phase;
        let (src, dest) = (key.draw.src, key.draw.dest);
        let cols = usize::try_from((px + dest.w).div_ceil(cw)).unwrap_or(0);
        let rows = usize::try_from((py + dest.h).div_ceil(ch)).unwrap_or(0);
        let mut spread = vec![Spread::EMPTY; cols * rows];
        let mut covered = vec![0u32; cols * rows];
        let width = usize::from(view.width.raw());
        let xs: Vec<usize> = (0..dest.w)
            .map(|i| usize::try_from(ImageDraw::nearest(i, dest.w, src.x, src.w)).unwrap_or(0))
            .collect();
        let pages = key.under.ends();
        let behind = match key.composite {
            ImageComposite::Over => pages,
            ImageComposite::Replace => key.under.material(),
        };
        for j in 0..dest.h {
            let ty = usize::try_from(ImageDraw::nearest(j, dest.h, src.y, src.h)).unwrap_or(0);
            let row = usize::try_from((py + j) / ch).unwrap_or(0);
            for (i, &tx) in (0..dest.w).zip(&xs) {
                let col = usize::try_from((px + i) / cw).unwrap_or(0);
                let Some(&p) = view.pixels.get(ty * width + tx) else {
                    continue;
                };
                let at = row * cols + col;
                let c = Argb(crate::renderer::filled(p, key.ground).bits());
                match c.a().get() {
                    0xFF => spread[at].add(c),
                    a => {
                        for b in behind {
                            spread[at].add(c.mix(b, Chan::from_u8(a)));
                        }
                    }
                }
                covered[at] += 1;
            }
        }
        // A cell the picture covers in part shows the page in the rest.
        for (s, &n) in spread.iter_mut().zip(&covered) {
            if n < cw * ch {
                for page in pages {
                    s.add(page);
                }
            }
        }
        Some(Table {
            key,
            built,
            cols: Cols::of_index(cols),
            rows: Rows::of_index(rows),
            cells: spread
                .iter()
                .map(|s| (s.luminance(), s.channels()))
                .collect(),
        })
    }
}
