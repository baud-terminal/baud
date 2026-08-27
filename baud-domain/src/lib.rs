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
#![forbid(unsafe_code)]

mod brand;
mod cell;
mod decode;
mod font;
mod grid;
mod layout;
mod line;
mod pos;
mod quantity;
mod scan;
mod source;
mod store;
mod style;
pub mod unchecked;
mod vt;
mod width;
mod width_data;

pub use brand::*;
pub use cell::*;
pub use decode::*;
pub use font::*;
pub use grid::*;
pub use layout::*;
pub use line::*;
pub use pos::*;
pub use quantity::*;
pub use scan::*;
pub use source::*;
pub use store::*;
pub use style::*;
pub use vt::*;
pub use width::*;
