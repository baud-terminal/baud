//! The crate's small quantities, each a type so the audit sees a name
//! rather than a primitive: a counter, a coverage byte, two digests, and
//! a band of pixel rows.

use baud_font::Px;

/// A running count for the frame report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Tally(u64);

impl Tally {
    pub fn bump(&mut self) {
        self.0 = self.0.saturating_add(1);
    }

    pub fn add(&mut self, n: Tally) {
        self.0 = self.0.saturating_add(n.0);
    }

    pub fn of(n: usize) -> Tally {
        Tally(u64::try_from(n).unwrap_or(u64::MAX))
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// One more, saturating -- for a counter bumped in place.
    pub fn inc(self) -> Tally {
        Tally(self.0.saturating_add(1))
    }

    /// One fewer, saturating: a count of live things one of which went.
    pub fn lose(&mut self) {
        self.0 = self.0.saturating_sub(1);
    }
}

/// A mask texel: how much of the pixel the glyph covers, 0 to 255.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Coverage(pub u8);

/// An FNV-1a fold, zero before anything was folded.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Digest(u64);

impl Digest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub fn start() -> Digest {
        Digest(Digest::OFFSET)
    }

    pub fn fold(self, v: u64) -> Digest {
        Digest((self.0 ^ v).wrapping_mul(Digest::PRIME))
    }

    pub fn fold_bytes(self, bytes: &[u8]) -> Digest {
        bytes.iter().fold(self, |d, &b| d.fold(u64::from(b)))
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

/// The pixel rows `top..bottom` one cell row occupies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Band {
    pub top: Px,
    pub bottom: Px,
}

/// A pixel count as a slice index.
pub fn slots(p: Px) -> usize {
    usize::try_from(p.value()).unwrap_or(usize::MAX)
}
