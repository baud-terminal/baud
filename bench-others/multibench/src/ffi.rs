/// ghostty's terminal core via libghostty-vt's C ABI: create, configure, feed,
/// destroy.
///
/// **`set` is not optional, and leaving it out made the comparison unfair.**
/// `ghostty_terminal_new` starts with "reasonable defaults e.g. around
/// scrollback limits", and queried from the library those are *unlimited
/// lines* and **10,000 bytes** -- with pruning at a page granularity ghostty
/// documents as ~400 KB, which retains essentially no history. alacritty and
/// vt100 are handed `scrolling_history = 10_000` **lines** here and baud
/// carries `SB = 10_000` lines, so three cores were retaining ten thousand
/// lines and the fourth was retaining none.
///
/// This replaced bench-others/ghostty-bench, a separate Zig harness (deleted
/// 2026-10-13) that stalled on ghostty's build-time-generated unicode_tables. libghostty-vt is now a
/// shipped artifact built by ghostty's own build.zig, which runs that codegen
/// itself — so ghostty measures here, in the same harness, through the same PTY,
/// against the same payloads as every other core. A separate harness was the
/// confound MEASUREMENTS.md warns about.
#[cfg(have_ghostty)]
pub(crate) mod ghostty {
    use std::ffi::c_void;
    pub type Terminal = *mut c_void;
    unsafe extern "C" {
        /// `allocator` may be NULL for ghostty's default. Returns 0 on success.
        pub fn ghostty_terminal_new(
            allocator: *const c_void,
            out: *mut Terminal,
            cols: u16,
            rows: u16,
        ) -> i32;
        pub fn ghostty_terminal_free(t: Terminal);
        /// Set one option. `value` may be NULL where the option documents it
        /// -- for the scrollback byte limit, NULL means unlimited.
        pub fn ghostty_terminal_set(t: Terminal, opt: i32, value: *const c_void) -> i32;
        /// Documented as infallible: malformed input is logged, never fatal.
        pub fn ghostty_terminal_vt_write(t: Terminal, data: *const u8, len: usize);
    }
}
