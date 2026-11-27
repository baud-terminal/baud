//! **An XKB keymap compiler for the keymap a compositor sends.**
//!
//! `wl_keyboard.keymap` hands a client the whole keymap as text in the
//! format `[[source: xkb-keymap-text-format]]` documents, and this crate
//! reads it and answers what a key produces under the modifier state the
//! compositor reports -- with no `libxkbcommon`, which is the register's
//! D13 taken strictly. `parse` reads the four sections, `compile` resolves
//! the virtual modifiers, the key types and the consumed modifiers, and
//! `state` holds the masks from `wl_keyboard.modifiers` and does the
//! lookup with the two transformations the library documents.
//!
//! The library is the oracle rather than the dependency: the fixtures under
//! `tests/` are its own answers over real keymaps, and `no_std` with
//! `alloc` is the shape the parser needs -- a keymap is thousands of names.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod compile;
pub mod compose;
pub mod keysym;
pub mod lex;
pub mod parse;
pub mod state;

pub use compile::{Keycode, Keymap, ModMask};
pub use keysym::Keysym;
pub use state::{Lookup, Mods, State};
