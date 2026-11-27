//! **Pinned data files compiled into typed tables, at build time.**
//!
//! A crate's `build.rs` reads its tables' sources with this crate -- the
//! Unicode Character Database files above all -- and writes the tables into
//! `OUT_DIR`, where the crate `include!`s them. Nothing generated is
//! committed, so nothing can drift from the data: the table is a function
//! of the pinned files, and the build computes it.
//!
//! What the data says is checked as it is read, and a failure stops the
//! build at the file, line and column of the row: a code point that is not
//! one (UAX #44 section 4.2.2, `[[source: uax44]]`) or is past U+10FFFF, a
//! row with the wrong number of fields, a UnicodeData.txt `First` with no
//! `Last`, a value outside the property's set, a key given twice, two rows
//! covering one code point, a count the table was compiled with, and a
//! header that is not the pinned file's. Every failure of a run is
//! reported, not only the first.
//!
//! It is a build-dependency, so it uses `std` alone and depends on nothing.

#![forbid(unsafe_code)]

mod emit;
mod error;
mod hash;
mod map;
mod ranges;
mod source;
mod trie;
mod values;

pub use emit::{Emit, PerLine};
pub use error::{Check, Error, Errors, Kind, Pos};
pub use hash::{Buckets, PerfectHash, Slot, Slots, fnv1a, perfect_hash, place};
pub use map::CodepointMap;
pub use ranges::binary_ranges;
pub use source::{CpRange, Entry, Field, Input, Row, Source};
pub use trie::Trie;
pub use values::ValueSet;

/// The last code point.
pub const LAST: u32 = 0x10_FFFF;

/// How many code points there are, U+0000 to U+10FFFF.
pub const CODE_POINTS: usize = 0x11_0000;
