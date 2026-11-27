//! The eight functions `src/sfte_shim.c` exports, and nothing else.
//!
//! **No Rust mirror of `sfte_cell`.** Its layout is a function of eight
//! macros, so a mirror would desynchronise the day one changed and read
//! plausible garbage rather than fail. The shim answers the three facts the
//! comparison needs -- rune, is-a-wide-tail, background-is-default -- as
//! integers, and everything else stays on the C side.
//!
//! sfte allocates with `malloc`/`calloc` and links libc, where `baud-core` is
//! caller-provided and allocation-free: the same engineering-contract note
//! `multibench`'s `ffi.rs` makes for libghostty-vt, and the comparison is over
//! what each draws, not over what each costs.
use std::ffi::{c_char, c_void};

/// An opaque handle to the shim's state: the sfte context plus the reply
/// accumulator.
pub type Shim = *mut c_void;

unsafe extern "C" {
    /// Set `LC_CTYPE` to `C.UTF-8` and return the name that took, or an empty
    /// string if none did. Callers must compare the answer; see `main.rs`.
    pub fn sfte_shim_set_locale() -> *const c_char;
    /// NULL if sfte failed to initialise **or came up at any geometry other
    /// than 200x50** -- that second case is the positive control for the arm.
    pub fn sfte_shim_new() -> Shim;
    pub fn sfte_shim_free(s: Shim);
    pub fn sfte_shim_feed(s: Shim, data: *const u8, len: usize);
    pub fn sfte_shim_cols(s: Shim) -> i16;
    pub fn sfte_shim_rows(s: Shim) -> i16;
    pub fn sfte_shim_cursor_col(s: Shim) -> i16;
    pub fn sfte_shim_cursor_row(s: Shim) -> i16;
    pub fn sfte_shim_rune(s: Shim, col: i16, row: i16) -> u32;
    pub fn sfte_shim_is_wide_tail(s: Shim, col: i16, row: i16) -> u8;
    pub fn sfte_shim_bg_is_default(s: Shim, col: i16, row: i16) -> u8;
    pub fn sfte_shim_reply_len(s: Shim) -> usize;
    pub fn sfte_shim_reply(s: Shim) -> *const u8;
    pub fn sfte_shim_reply_dropped(s: Shim) -> usize;
    pub fn sfte_shim_clear_reply(s: Shim);
    pub fn sfte_shim_cell_bytes() -> usize;
}
