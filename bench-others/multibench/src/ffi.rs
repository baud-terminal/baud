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
        /// Install a process-wide hook; for the PNG decoder, `value` is the
        /// callback. Returns 0 on success.
        pub fn ghostty_sys_set(option: i32, value: *const c_void) -> i32;
        /// `len` bytes from the allocator the library hands a callback; the
        /// library frees them.
        pub fn ghostty_alloc(allocator: *const c_void, len: usize) -> *mut u8;
    }

    /// `GHOSTTY_SYS_OPT_DECODE_PNG`.
    const SYS_OPT_DECODE_PNG: i32 = 1;

    /// `GhosttySysImage`: the decoded image a PNG callback fills.
    #[repr(C)]
    pub struct SysImage {
        width: u32,
        height: u32,
        data: *mut u8,
        data_len: usize,
    }

    /// Decodes a PNG with baud-domain's decoder into RGBA bytes taken from
    /// the library's allocator, which then owns them. Any failure returns
    /// false, which the library reports as a refused image.
    unsafe extern "C" fn decode_png(
        _userdata: *mut c_void,
        allocator: *const c_void,
        data: *const u8,
        data_len: usize,
        out: *mut SysImage,
    ) -> bool {
        // SAFETY: the library passes `data_len` readable bytes at `data`.
        let bytes = unsafe { std::slice::from_raw_parts(data, data_len) };
        let Ok(png) = baud_domain::png::Png::parse(bytes) else {
            return false;
        };
        let (Some(raw_len), Some(rgba_len)) = (png.raw_len(), png.rgba_len()) else {
            return false;
        };
        let mut raw = vec![0u8; raw_len];
        // SAFETY: `ghostty_alloc` returns `rgba_len` writable bytes or null.
        let pixels = unsafe { ghostty_alloc(allocator, rgba_len) };
        if pixels.is_null() {
            return false;
        }
        // SAFETY: `pixels` holds `rgba_len` bytes, just allocated and unaliased.
        let rgba = unsafe { std::slice::from_raw_parts_mut(pixels, rgba_len) };
        let Ok(size) = png.decode(&mut raw, rgba) else {
            return false;
        };
        // SAFETY: the library passes a valid `out` to fill.
        unsafe {
            *out = SysImage {
                width: size.width,
                height: size.height,
                data: pixels,
                data_len: rgba_len,
            };
        }
        true
    }

    /// Registers [`decode_png`] once for the process.
    pub fn install_png_decoder() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            type DecodePng = unsafe extern "C" fn(
                *mut c_void,
                *const c_void,
                *const u8,
                usize,
                *mut SysImage,
            ) -> bool;
            let f: DecodePng = decode_png;
            // SAFETY: `f` matches `GhosttySysDecodePngFn`; the library keeps
            // the pointer for the process's life, and `decode_png` is static.
            let r = unsafe { ghostty_sys_set(SYS_OPT_DECODE_PNG, f as *const c_void) };
            assert_eq!(r, 0, "registering ghostty's PNG decoder failed with {r}");
        });
    }
}
