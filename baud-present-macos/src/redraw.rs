//! **What a surface is missing**, for a GPU frame drawn into a surface that
//! still holds an older one. Three surfaces rotate, so the one a frame goes
//! to holds the frame from three commits back; a frame that changed only
//! some rows of the one before it ([`Redraw`]) lets the next draw load the
//! surface and draw the rows every frame since changed, the union of their
//! changes, as the Wayland presenter brings a released buffer up to date
//! with every row changed since it was shown (`baud-present-wayland`'s
//! `damage.rs`). A surface no frame has been drawn into, and a frame whose
//! change was not recorded, are missing everything.

use baud_render::{Instance, Redraw, RowSet};

/// How many surfaces rotate: one showing, one the compositor may still be
/// reading for the previous transaction, one being drawn.
pub const SURFACES: usize = 3;

/// One of the [`SURFACES`], by its place in the rotation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SurfaceIx(pub(crate) usize);

impl SurfaceIx {
    pub const FIRST: SurfaceIx = SurfaceIx(0);

    /// The surface drawn after this one.
    pub fn next(self) -> SurfaceIx {
        SurfaceIx((self.0 + 1) % SURFACES)
    }
}

/// A set of the [`SURFACES`], one bit each.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Surfaces(pub(crate) u64);

impl Surfaces {
    pub const NONE: Surfaces = Surfaces(0);

    /// This set and `s`.
    pub fn with(self, s: SurfaceIx) -> Surfaces {
        Surfaces(
            self.0
                | u32::try_from(s.0)
                    .ok()
                    .and_then(|b| 1u64.checked_shl(b))
                    .unwrap_or(0),
        )
    }

    /// Whether `s` is in it.
    pub fn holds(self, s: SurfaceIx) -> bool {
        Surfaces::NONE.with(s).0 & self.0 != 0
    }
}

/// Which committed frame each surface holds, and what the last frames
/// committed changed of the one before.
#[derive(Clone, Debug)]
pub struct Ages {
    /// The frame each surface holds, by its count among the commits.
    held: Vec<Option<Commit>>,
    /// The changes of the frames committed last, oldest first, at most as
    /// many as there are surfaces: a surface is never more than that many
    /// commits behind and still recorded.
    recent: Vec<(Commit, Redraw)>,
    next: Commit,
}

/// A frame's place among the commits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Commit(u64);

impl Default for Ages {
    fn default() -> Ages {
        Ages::new()
    }
}

impl Ages {
    /// The [`SURFACES`], holding nothing.
    pub fn new() -> Ages {
        Ages {
            held: vec![None; SURFACES],
            recent: Vec::with_capacity(SURFACES),
            next: Commit(0),
        }
    }

    /// What `surface` lacks of the last frame committed: the union of every
    /// change since the frame it holds, or the whole when it holds none or a
    /// change between is no longer recorded.
    pub fn missing(&self, surface: SurfaceIx) -> Redraw {
        let Some(Some(held)) = self.held.get(surface.0).copied() else {
            return Redraw::Whole;
        };
        let after = Commit(held.0 + 1);
        match self.recent.first() {
            // A frame after the one held that the record no longer has.
            Some(&(oldest, _)) if oldest > after && after < self.next => Redraw::Whole,
            _ => self
                .recent
                .iter()
                .filter(|(c, _)| *c > held)
                .fold(Redraw::NOTHING, |d, &(_, r)| d.union(r)),
        }
    }

    /// **`surface` holds no frame**: the one committed into it failed, so
    /// its content is undefined, and it is missing everything until a
    /// frame is drawn into it again.
    pub fn forget(&mut self, surface: SurfaceIx) {
        if let Some(h) = self.held.get_mut(surface.0) {
            *h = None;
        }
    }

    /// A frame committed into `surface`, which changed `changed` of the
    /// frame committed before it.
    pub fn drew(&mut self, surface: SurfaceIx, changed: Redraw) {
        let now = self.next;
        self.next = Commit(now.0 + 1);
        if self.recent.len() == self.held.len().max(1) {
            self.recent.remove(0);
        }
        self.recent.push((now, changed));
        if let Some(h) = self.held.get_mut(surface.0) {
            *h = Some(now);
        }
    }
}

/// The grid rows a record covers, given the cell height, or `None` when it
/// reaches past the grid's `cols` by `rows` cells of `cell` pixels: a record
/// there draws pixels no row's redraw holds.
pub fn record_rows(r: &Instance, cell: (u32, u32), (cols, rows): (u32, u32)) -> Option<RowSet> {
    let (cw, ch) = (i64::from(cell.0), i64::from(cell.1));
    let (x, y) = (i64::from(r.x.0), i64::from(r.y.0));
    let (w, h) = (i64::from(r.w.0), i64::from(r.h.0));
    if w == 0 || h == 0 {
        return Some(RowSet::NONE);
    }
    if x < 0 || y < 0 || x + w > i64::from(cols) * cw || y + h > i64::from(rows) * ch || ch == 0 {
        return None;
    }
    let first = usize::try_from(y / ch).ok()?;
    let last = usize::try_from((y + h - 1) / ch).ok()?;
    RowSet::NONE.span(
        baud_domain::Row::from_index(first),
        baud_domain::Row::from_index(last + 1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::Row;

    fn rows(r: &[usize]) -> Redraw {
        r.iter()
            .fold(Redraw::NOTHING, |d, &i| d.with(Row::from_index(i)))
    }

    const S0: SurfaceIx = SurfaceIx(0);
    const S1: SurfaceIx = SurfaceIx(1);
    const S2: SurfaceIx = SurfaceIx(2);

    #[test]
    fn a_surface_misses_every_change_since_its_frame() {
        let mut a = Ages::new();
        assert_eq!(a.missing(S0), Redraw::Whole, "it holds nothing");
        a.drew(S0, Redraw::Whole);
        a.drew(S1, rows(&[2]));
        a.drew(S2, rows(&[5]));
        // Surface 0 holds the first; the two after it changed rows 2 and 5.
        assert_eq!(a.missing(S0), rows(&[2, 5]));
        a.drew(S0, rows(&[7]));
        // Surface 1 holds the second; since, rows 5 and 7.
        assert_eq!(a.missing(S1), rows(&[5, 7]));
        // Surface 0 holds the last frame: nothing missing.
        assert_eq!(a.missing(S0), Redraw::NOTHING);
        a.drew(S1, Redraw::Whole);
        assert_eq!(a.missing(S2), Redraw::Whole, "a whole frame since");
    }

    /// **A surface whose frame failed misses everything**, though every
    /// frame since changed only rows. Break: `forget` leaving `held` as it
    /// was, and it misses row 1 alone.
    #[test]
    fn a_forgotten_surface_misses_everything() {
        let mut a = Ages::new();
        a.drew(S0, Redraw::Whole);
        a.drew(S1, rows(&[1]));
        a.forget(S1);
        a.drew(S2, rows(&[1]));
        assert_eq!(a.missing(S0), rows(&[1]), "a surface that drew");
        assert_eq!(a.missing(S1), Redraw::Whole, "the surface that failed");
        a.drew(S1, rows(&[2]));
        assert_eq!(a.missing(S1), Redraw::NOTHING, "drawn into again");
    }

    #[test]
    fn a_surface_past_the_record_misses_everything() {
        let mut a = Ages::new();
        a.drew(S0, Redraw::Whole);
        // Four frames into the other two: surface 0's frame is four back,
        // and the record keeps three.
        for _ in 0..2 {
            a.drew(S1, rows(&[1]));
            a.drew(S2, rows(&[1]));
        }
        assert_eq!(a.missing(S0), Redraw::Whole);
        assert_eq!(a.missing(S1), rows(&[1]));
    }
}
