//! Domain model as a type system.
//!
//! Every rule here is enforced by the compiler rather than by a comment. The
//! doc comments say *which measurement* made each rule necessary, but the rule
//! itself is structural — the wrong program does not build.
//!
//! Four techniques, in order of how much they buy:
//!
//! 1. **Generative brands** for anything whose validity depends on a mutable
//!    context. A `RowIdx` is meaningless once `cols` changes; a branded
//!    lifetime makes reusing one a type error rather than a silent wrong answer.
//! 2. **Typestate** for one-way lifecycles. `push_run` simply does not exist on
//!    a completed line.
//! 3. **Opaque newtypes with no arithmetic.** You cannot add a `LineIdx` to a
//!    `RowIdx` because neither exposes its integer.
//! 4. **Constructor narrowing.** `DisplayWidth` can only be built from a
//!    `WidthTable`, and no font type exists in this crate to pass instead.

#![no_std]
// `put_ascii_run` writes 27% of the cells on the streaming payload and costs
// 5.3x per cell what `slice::fill` does for the other 73%, because it was a
// scalar loop -- `otool -tv` showed no vector register in it at all. This
// feature is what lets it store eight cells at a time.
#![feature(portable_simd)]
// `ModeBit`'s bound on the `u32` bitset counts the variants with
// `core::mem::variant_count` rather than naming the last one, which went stale
// twice (`vt/mode.rs`).
#![feature(variant_count)]
#![forbid(unsafe_code)]

mod brand;
mod cache;
mod cell;
mod cluster_data;

/// One entry of the UAX #29 transition table: bit 7 is the break decision,
/// bits 0..7 the next state. Exposed so
/// `the_cluster_table_agrees_with_the_rules` can check the artefact against
/// the rules that generated it.
pub fn cluster_next(state: u8, input: u8) -> u8 {
    cluster_data::CLUSTER_NEXT[(state as usize) * 256 + input as usize]
}
mod dec_graphics;
mod decode;
mod font;
mod frame;
mod grapheme;
mod grapheme_data;
mod grid;
mod history;
mod image;
mod input;
mod layout;
mod line;
mod merged;
mod merged_data;
mod nrcs_data;
mod pos;
mod quantity;
mod scan;
mod sixel;
mod source;
mod store;
mod style;
pub mod terminfo;
pub mod themefile;
mod time;
pub mod unchecked;
mod vt;
mod width;
mod width_data;

pub use brand::*;
pub use cache::*;
pub use cell::*;
pub use dec_graphics::*;
pub use decode::*;
pub use font::*;
pub use frame::*;
pub use grapheme::*;
pub use grid::*;
pub use history::*;
pub use image::*;
pub use input::*;
pub use layout::*;
pub use line::*;
pub use merged::*;
pub use nrcs_data::*;
pub use pos::*;
pub use quantity::*;
pub use scan::*;
pub use sixel::*;
pub use source::*;
pub use store::*;
pub use style::*;
pub use time::*;
pub use vt::*;
pub use width::*;
