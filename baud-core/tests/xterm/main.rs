//! xterm extensions — the ones applications actually depend on.
//!
//! Not DEC protocol, and mostly not in any standard: the alternate screen,
//! mouse reporting, OSC, hyperlinks, bracketed paste, synchronised output and
//! XTVERSION. `docs/protocols/xterm.md` records which are implemented and which
//! are declined, with reasons.

mod alt_screen;
mod embedder_modes;
mod hyperlinks;
mod keypad_version;
mod mouse;
mod osc;
