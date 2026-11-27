//! iTerm2's `OSC 1337` keys, compiled by `build/iterm_keys.rs` from the
//! names alone, `[[source: iterm2-osc1337-doc-keys]]` and
//! `[[source: iterm2-osc1337-source-keys]]`, which were read from the
//! pinned pages and source (external, GPL-2.0);
//! `every_iterm2_key_has_its_row` fails if the table in `iterm.rs` misses
//! a key listed here.

include!(concat!(env!("OUT_DIR"), "/iterm_keys.rs"));
