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
// kitty's `oklch(` and `lab(` colours take square roots
// (`baud-domain/src/themefile/oklab.rs`), and the hardware's is correctly rounded, as
// kitty's libm and Go's `math.Sqrt` are; `core` has it only behind this.
#![feature(core_float_math)]
#![forbid(unsafe_code)]

// `Pixel::zeroed_vec`, for a host with an allocator: the arena from zeroed
// memory. Off by default; `baud-core`'s `alloc` turns it on.
#[cfg(feature = "alloc")]
extern crate alloc;

mod base64;
mod bidi;
mod bidi_data {
    //! Bidirectional data, compiled by `build/bidi.rs` from the sources
    //! pinned in `docs/research/sources/MANIFEST.toml`
    //! (`[[source: unicode-derivedbidiclass]]`, `[[source: unicode-data]]`,
    //! `[[source: unicode-bidimirroring]]`,
    //! `[[source: unicode-bidibrackets]]`,
    //! `[[source: unicode-propertyvaluealiases]]`). BIDI is one byte a code
    //! point: the low five bits `BidiClass`, bit 5 Bidi_Mirrored, bits 6-7
    //! the bracket type; MIRROR is two, the mirroring glyph's index and the
    //! bracket pair; BOX_COUNTERPART is a Box Drawing character's left-right
    //! counterpart.
    include!(concat!(env!("OUT_DIR"), "/bidi_data.rs"));
}
mod brand;
mod cache;
mod caseless;
mod caseless_data {
    //! What a font family name compares, compiled by `build/caseless.rs`
    //! from `[[source: unicode-casefolding]]`'s full case folding (status C
    //! and F) pinned in `docs/research/sources/MANIFEST.toml`, for the
    //! default caseless match of `[[source: unicode17-ch3]]` D144 that
    //! `[[source: css-fonts-3]]` section 5.1 names.
    include!(concat!(env!("OUT_DIR"), "/caseless_data.rs"));
}
mod cell;
mod cluster_data;
mod colour_name;
pub mod contrast;
mod link_table;

/// One entry of the UAX #29 transition table: bit 7 is the break decision,
/// bits 0..7 the next state. Exposed so
/// `the_cluster_table_agrees_with_the_rules` can check the artefact against
/// the rules that generated it.
pub fn cluster_next(state: u8, input: u8) -> u8 {
    cluster_data::CLUSTER_NEXT[(state as usize) * 256 + input as usize]
}
mod dec_graphics;
mod dec_sets_data;
mod decode;
mod expression;
mod font;
mod frame;
mod frozen;
pub mod gif;
mod grapheme;
mod grapheme_data {
    //! Grapheme-cluster data, compiled by `build/grapheme.rs` from the
    //! sources pinned in `docs/research/sources/MANIFEST.toml`
    //! (`[[source: gbp-data]]`, `[[source: emoji-data]]`,
    //! `[[source: dcp-data]]`, `[[source: gc-data]]`,
    //! `[[source: unicode-propertyvaluealiases]]`). BREAK is one byte a code
    //! point: the low four bits `GraphemeClass`, the high four `Lookback`.
    //! The class and the lookback sets are one table because the cluster
    //! rules need them together: 32,768 bytes against the 21,032 of a class
    //! trie and four range tables, and a probe that added a whole table
    //! lookup to `Width::of` measured flat, so the loads were never the cost
    //! and the binary searches' branches were. SPACING_EXTEND is the 46
    //! ranges of spacing marks UAX #29 classes Extend.
    include!(concat!(env!("OUT_DIR"), "/grapheme_data.rs"));
}
mod grid;
mod history;
mod ignorable_data {
    //! `Default_Ignorable_Code_Point`, compiled by `build/ignorable.rs` from
    //! `[[source: dcp-data]]`.
    include!(concat!(env!("OUT_DIR"), "/ignorable_data.rs"));
}
mod image;
mod input;
mod interpolate;
mod iso96_data;
mod joining;
mod joining_data {
    //! Cursive-joining data, compiled by `build/joining.rs` from the
    //! sources pinned in `docs/research/sources/MANIFEST.toml`
    //! (`[[source: unicode-arabicshaping]]`, `[[source: gc-data]]`,
    //! `[[source: unicode-derivedjoiningtype]]`,
    //! `[[source: unicode-propertyvaluealiases]]`).
    include!(concat!(env!("OUT_DIR"), "/joining_data.rs"));
}
pub mod jpeg;
mod kitty;
mod layout;
mod line;
mod linebreak;
mod linebreak_data {
    //! Line-breaking data, compiled by `build/linebreak.rs` from the
    //! sources pinned in `docs/research/sources/MANIFEST.toml`
    //! (`[[source: unicode-linebreak]]`, `[[source: gc-data]]`,
    //! `[[source: eaw-data]]`, `[[source: emoji-data]]`,
    //! `[[source: unicode-propertyvaluealiases]]`). LINE is one byte a code
    //! point: the low six bits `LineClass` as UAX #14's LB1 resolves it
    //! (`[[source: uax14]]`), bit 6 East_Asian_Width F, W or H, bit 7
    //! Extended_Pictographic and unassigned.
    include!(concat!(env!("OUT_DIR"), "/linebreak_data.rs"));
}
mod merged;
mod merged_data;
mod nrcs_data;
mod placeholder;
mod placeholder_data {
    //! The Unicode placeholder diacritics, compiled by `build/placeholder.rs`
    //! from the codepoints of kitty's list
    //! (`[[source: kitty-placeholder-diacritics]]`, read from
    //! `[[source: kitty-rowcolumn-diacritics]]`).
    include!(concat!(env!("OUT_DIR"), "/placeholder_data.rs"));
}
pub mod png;
mod pos;
mod premultiply;
mod prompts;
mod quantity;
mod regis;
mod scan;
mod search;
#[cfg(test)]
mod table_identity;
mod search_data {
    //! What a search compares, compiled by `build/search.rs` from the
    //! sources pinned in `docs/research/sources/MANIFEST.toml`:
    //! `[[source: unicode-data]]`'s canonical decompositions and combining
    //! classes and `[[source: unicode-casefolding]]`'s full case folding
    //! (status C and F), for the canonical caseless match of
    //! `[[source: unicode17-ch3]]` D145.
    include!(concat!(env!("OUT_DIR"), "/search_data.rs"));
}
mod sequence;
mod sequence_data {
    //! The sequences the Unicode Standard names, as a trie, compiled by
    //! `build/sequence.rs` from UTS #51's RGI emoji sequences
    //! (`[[source: emoji17-sequences]]`, `[[source: emoji17-zwj-sequences]]`)
    //! and the standardized emoji variation sequences
    //! (`[[source: emoji-variation-sequences]]`).
    include!(concat!(env!("OUT_DIR"), "/sequence_data.rs"));
}
mod sixel;
mod sixel_dump;
mod source;
mod store;
mod style;
mod substitute;
pub mod terminfo;
pub mod themefile;
mod time;
pub mod unchecked;
mod variables;
mod variants;
pub mod vs16;
mod vt;
mod width;
mod width_data {
    //! Width data, compiled by `build/width.rs` from the sources pinned in
    //! `docs/research/sources/MANIFEST.toml` (`[[source: eaw-data]]`,
    //! `[[source: eaw-data-8]]`, `[[source: gc-data]]`,
    //! `[[source: unicode-proplist-17]]`, `[[source: emoji-data]]`,
    //! `[[source: unicode-propertyvaluealiases]]`). WIDTH and WIDTH8 are two
    //! bits a code point, four a byte: 0 narrow, 1 zero, 2 wide, 3
    //! ambiguous, under Unicode 17.0.0's East_Asian_Width and 8.0.0's. A
    //! two-stage trie is consulted once per **non-ASCII codepoint** and
    //! never per byte: printable ASCII, the ~94% path, never reaches it.
    include!(concat!(env!("OUT_DIR"), "/width_data.rs"));
}
pub mod zlib;

pub use base64::*;
pub use bidi::*;
pub use brand::*;
pub use cache::*;
pub use caseless::*;
pub use cell::*;
pub use dec_graphics::*;
pub use decode::*;
pub use expression::*;
pub use font::*;
pub use frame::*;
pub use frozen::*;
pub use grapheme::*;
pub use grid::*;
pub use history::*;
pub use image::*;
pub use input::*;
pub use interpolate::*;
pub use joining::*;
pub use kitty::*;
pub use layout::*;
pub use line::*;
pub use linebreak::*;
pub use link_table::*;
pub use merged::*;
pub use nrcs_data::*;
pub use placeholder::*;
pub use pos::*;
pub use premultiply::*;
pub use prompts::*;
pub use quantity::*;
pub use regis::*;
pub use scan::*;
pub use search::*;
pub use sixel::*;
pub use sixel_dump::*;
pub use source::*;
pub use store::*;
pub use style::*;
pub use substitute::*;
pub use time::*;
pub use variables::*;
pub use variants::*;
pub use vs16::Vs16Width;
pub use vt::*;
pub use width::*;
