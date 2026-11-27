use crate::*;
use core::marker::PhantomData;

mod sealed {
    pub trait State {}
}

/// Still being appended to.
pub enum Building {}

/// Immutable: runs final, width cached.
pub enum Complete {}

/// Bytes copied to the cold ring. The one mandatory copy in the design —
/// memcpy at 5948 MB/s against 280 MB/s ingest, ~5% of a core.
pub enum Drained {}

impl sealed::State for Building {}

impl sealed::State for Complete {}

impl sealed::State for Drained {}

/// A line of text as the *application* produced it — delimited by `\n`, not by
/// the screen edge. This is the unit that survives resize, and keeping it is
/// why reflow is re-derivation rather than reconstruction from a wrapped flag.
pub struct Line<S: sealed::State> {
    first_run: RunIdx,
    run_count: RunCount,
    width: DisplayWidth,
    _s: PhantomData<S>,
}

impl Line<Building> {
    pub fn new(first_run: RunIdx) -> Self {
        Line {
            first_run,
            run_count: RunCount::ZERO,
            width: DisplayWidth::ZERO,
            _s: PhantomData,
        }
    }
    /// Runs live in a shared pool, so there is no fixed cap. A previous design
    /// used `[Run; 8]` and silently promoted any line exceeding it.
    pub fn push_run(&mut self) {
        self.run_count = self.run_count.inc();
    }

    /// The only transition out of `Building`. Consumes `self`, so the mutable
    /// handle is gone.
    pub fn complete(self, width: DisplayWidth) -> Line<Complete> {
        Line {
            first_run: self.first_run,
            run_count: self.run_count,
            width,
            _s: PhantomData,
        }
    }
}

impl Line<Complete> {
    /// Only exists once the line is final, so a width can never be read
    /// mid-append.
    pub fn width(&self) -> DisplayWidth {
        self.width
    }
    pub fn runs(&self) -> RunRange {
        RunRange {
            start: self.first_run,
            count: self.run_count,
        }
    }
    pub fn drained(self) -> Line<Drained> {
        Line {
            first_run: self.first_run,
            run_count: self.run_count,
            width: self.width,
            _s: PhantomData,
        }
    }
}

impl Line<Drained> {
    pub fn width(&self) -> DisplayWidth {
        self.width
    }
}
