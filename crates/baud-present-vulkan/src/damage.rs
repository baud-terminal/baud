//! **What each target lacks of the frame drawn last.** A window draws into
//! [`TARGETS`] images in turn, each keeping the frame last drawn into it
//! while the compositor shows another. A target lacks every row changed
//! since its frame -- the union of each later frame's change, whichever
//! target or arm drew it -- and a frame drawn into it draws those rows and
//! its own change, and hands the compositor the same rows. A target that
//! holds no frame lacks the whole.
//!
//! [`Stale`] keeps that account and nothing else: it takes each frame's
//! change ([`Redraw`]) and the target it went to, a frame the compositor did
//! not take, and a frame drawn on the CPU arm, and answers what a frame into
//! a target draws. No GPU is needed to hold it to that.

use baud_domain::{Row, Rows};
use baud_render::{Instance, Redraw, RowSet};

/// How many targets a window draws into in turn: one shown, one drawn.
pub const TARGETS: usize = 2;

/// The most bands a partial frame draws, each its own scissor over the
/// cell passes: runs past it are joined, the closest first.
pub const BANDS: usize = 4;

/// One of the [`TARGETS`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetIx(pub usize);

/// **Each target's stale rows**: what it lacks of the frame drawn last,
/// and what the frame drawn into it last drew.
#[derive(Clone, Copy, Debug)]
pub struct Stale {
    owed: [Redraw; TARGETS],
    drew: [Redraw; TARGETS],
}

impl Default for Stale {
    fn default() -> Stale {
        Stale::new()
    }
}

impl Stale {
    /// Every target holding no frame: each lacks the whole.
    pub fn new() -> Stale {
        Stale {
            owed: [Redraw::Whole; TARGETS],
            drew: [Redraw::NOTHING; TARGETS],
        }
    }

    /// What `k` lacks of the frame drawn last; the whole for a target
    /// outside the [`TARGETS`].
    pub fn owed(&self, k: TargetIx) -> Redraw {
        self.owed.get(k.0).copied().unwrap_or(Redraw::Whole)
    }

    /// **What a frame that changed `changed` of the last draws into `k`**:
    /// the rows `k` lacks and the frame's own change.
    pub fn draws(&self, k: TargetIx, changed: Redraw) -> Redraw {
        self.owed(k).union(changed)
    }

    /// **A frame that changed `changed` of the last, drawn into `k`**: `k`
    /// holds it and lacks nothing. Returns what it drew.
    /// Every other target lacks `changed` as well as what it lacked.
    pub fn drew(&mut self, k: TargetIx, changed: Redraw) -> Redraw {
        let drawn = self.draws(k, changed);
        for (j, o) in self.owed.iter_mut().enumerate() {
            *o = match j == k.0 {
                true => Redraw::NOTHING,
                false => o.union(changed),
            };
        }
        if let Some(d) = self.drew.get_mut(k.0) {
            *d = drawn;
        }
        drawn
    }

    /// **The frame drawn into `k` was not taken by the compositor**, which
    /// still shows an earlier one: the rows that frame drew are owed by
    /// `k` again, so the next frame into it draws them and hands them to
    /// the compositor with its own.
    pub fn refused(&mut self, k: TargetIx) {
        let drew = self.drew.get(k.0).copied().unwrap_or(Redraw::Whole);
        if let Some(o) = self.owed.get_mut(k.0) {
            *o = o.union(drew);
        }
    }

    /// **A frame that changed `changed` of the last, drawn where no target
    /// is**: on the CPU arm, or into an image outside the [`TARGETS`].
    /// Every target lacks it.
    pub fn elsewhere(&mut self, changed: Redraw) {
        for o in &mut self.owed {
            *o = o.union(changed);
        }
    }

    /// **`k` holds no frame**: a new image, or one whose frame failed.
    pub fn forget(&mut self, k: TargetIx) {
        if let Some(o) = self.owed.get_mut(k.0) {
            *o = Redraw::Whole;
        }
    }
}

/// **The grid rows a record covers**, for cells `cell` pixels big, or
/// `None` when it reaches past the grid's `cols` by `rows` cells: a record
/// there draws pixels no row's band holds.
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
    RowSet::NONE.span(Row::from_index(first), Row::from_index(last + 1))
}

/// The grid rows every record of `records` covers, or `None` when one
/// reaches past the grid ([`record_rows`]).
pub fn records_rows<'a>(
    records: impl IntoIterator<Item = &'a Instance>,
    cell: (u32, u32),
    grid: (u32, u32),
) -> Option<RowSet> {
    records.into_iter().try_fold(RowSet::NONE, |set, r| {
        record_rows(r, cell, grid).map(|rs| set.union(rs))
    })
}

/// **The bands a partial frame draws `rows` in**, into `out`: each run of
/// rows, the two runs with the fewest rows between them joined until at
/// most [`BANDS`] are left. `out` keeps its capacity, so a frame that
/// finds it large enough allocates nothing.
pub fn bands(rows: RowSet, out: &mut Vec<(Row, Rows)>) {
    out.clear();
    out.extend(rows.runs());
    while out.len() > BANDS {
        let gap = |i: usize| out[i + 1].0.index() - (out[i].0.index() + out[i].1.index());
        let Some(i) = (0..out.len() - 1).min_by_key(|&i| gap(i)) else {
            break;
        };
        let (a, b) = (out[i], out[i + 1]);
        out[i] = (a.0, Rows::of_index(b.0.index() + b.1.index() - a.0.index()));
        out.remove(i + 1);
    }
}
