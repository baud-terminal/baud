//! VT100 / ANSI — the set every terminal must implement.
//!
//! One module per area rather than one flat file per test, so the protocol a
//! test belongs to is visible from its path: `tests/vt100/scrolling.rs` is
//! VT100 scrolling, and nothing else is.
//!
//! Conformance against the `vt100`, `vt102` and `ansi` terminfo profiles lives
//! in `tests/conformance/` — these are the behavioural tests behind it.

mod c0;
mod charsets;
mod cursor;
mod erase;
mod irm;
mod modes;
mod scrolling;
mod tabs;
