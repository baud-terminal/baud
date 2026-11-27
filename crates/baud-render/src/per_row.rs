//! **Each row's instances, and the frame's tallies of them** (FS-C7): how
//! many there are, how many are on the colour texture and how many are
//! solid -- the totals a frame reports (`Frame::instances`, `colored`,
//! `solids`).
//!
//! The totals are kept as the rows are written rather than counted each
//! frame: a row's tally is taken when its instances are set, from those
//! instances, and the totals move by the difference. So a frame that
//! converts one row tallies that row, and reads three numbers for the
//! whole screen, where it walked every instance of every row before.
//!
//! **The fields are this module's.** A row is written only through
//! [`PerRow::reset`], [`PerRow::set`] and [`PerRow::take`], each of which
//! tallies what the row then holds, so no write reaches a row without
//! its tally following -- the conversion's take and put, the leaving
//! line's save and restore around its own conversion, and the reset on a
//! resize alike.

use crate::Tally;
use crate::atlas::Kind;
use crate::instance::Instance;
use baud_domain::{Row, Rows};

/// **The tallies of some instances**: all of them, those on the colour
/// texture, and those solid.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Kinds {
    instances: Tally,
    colored: Tally,
    solids: Tally,
}

impl Kinds {
    /// `of`'s tallies, each instance read once.
    fn of(of: &[Instance]) -> Kinds {
        let mut k = Kinds {
            instances: Tally::of(of.len()),
            ..Kinds::default()
        };
        for inst in of {
            match inst.kind {
                Kind::Color => k.colored.bump(),
                Kind::Solid => k.solids.bump(),
                Kind::Mask | Kind::Quad | Kind::Band | Kind::Particle => {}
            }
        }
        k
    }

    /// These tallies with `k`'s added.
    fn plus(self, k: Kinds) -> Kinds {
        let mut t = self;
        t.instances.add(k.instances);
        t.colored.add(k.colored);
        t.solids.add(k.solids);
        t
    }

    /// These tallies with `k`'s taken away: `k` a part of them.
    fn less(self, k: Kinds) -> Kinds {
        let mut t = self;
        t.instances.remove(k.instances);
        t.colored.remove(k.colored);
        t.solids.remove(k.solids);
        t
    }

    /// All the instances.
    pub(crate) fn instances(self) -> Tally {
        self.instances
    }

    /// Those on the colour texture.
    pub(crate) fn colored(self) -> Tally {
        self.colored
    }

    /// Those solid.
    pub(crate) fn solids(self) -> Tally {
        self.solids
    }
}

/// **Every row's instances, each row's tally, and their totals.**
#[derive(Default)]
pub(crate) struct PerRow {
    rows: Vec<Vec<Instance>>,
    kinds: Vec<Kinds>,
    total: Kinds,
}

impl PerRow {
    /// `rows` rows, each empty: the totals nothing.
    pub(crate) fn reset(&mut self, rows: Rows) {
        self.rows = (0..rows.index()).map(|_| Vec::new()).collect();
        self.kinds = vec![Kinds::default(); rows.index()];
        self.total = Kinds::default();
    }

    /// Row `r`'s instances, now `instances`, tallied.
    pub(crate) fn set(&mut self, r: Row, instances: Vec<Instance>) {
        self.rows[r.index()] = instances;
        self.tally(r);
    }

    /// Row `r`'s instances, taken out and the row left empty, tallied as
    /// such: for a conversion that fills the same buffer and puts it back.
    pub(crate) fn take(&mut self, r: Row) -> Vec<Instance> {
        let out = core::mem::take(&mut self.rows[r.index()]);
        self.tally(r);
        out
    }

    /// The totals over every row.
    pub(crate) fn totals(&self) -> Kinds {
        self.total
    }

    /// The totals counted again from every row's instances, each read
    /// once, as every frame counted them before the tallies were kept: for
    /// the count harness to hold the kept totals to.
    #[cfg(feature = "counters")]
    pub(crate) fn recounted(&self) -> Kinds {
        self.rows
            .iter()
            .fold(Kinds::default(), |t, row| t.plus(Kinds::of(row)))
    }

    /// Every row's instances, top to bottom.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &[Instance]> + '_ {
        self.rows.iter().map(Vec::as_slice)
    }

    /// Row `r`'s tally taken again from what it holds, and the totals moved
    /// by the difference.
    fn tally(&mut self, r: Row) {
        let (was, now) = (self.kinds[r.index()], Kinds::of(&self.rows[r.index()]));
        crate::counters::add(
            crate::counters::Counter::InstancesTallied,
            crate::counters::Site::Totals,
            now.instances,
        );
        self.total = self.total.plus(now).less(was);
        self.kinds[r.index()] = now;
    }
}

impl core::ops::Index<Row> for PerRow {
    type Output = [Instance];

    fn index(&self, r: Row) -> &[Instance] {
        &self.rows[r.index()]
    }
}
