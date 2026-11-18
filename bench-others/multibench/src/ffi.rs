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
        /// Resize to `cols` by `rows` cells of `cell_width_px` by
        /// `cell_height_px`; the pixel size is set even when the grid's is
        /// unchanged. Returns 0 on success.
        pub fn ghostty_terminal_resize(
            t: Terminal,
            cols: u16,
            rows: u16,
            cell_width_px: u32,
            cell_height_px: u32,
        ) -> i32;
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
        /// Read one datum of the terminal into `out`, of the type the
        /// header names for `data`. Returns 0 on success.
        pub fn ghostty_terminal_get(t: Terminal, data: i32, out: *mut c_void) -> i32;
        /// A reference to the cell at `point`, valid until the next
        /// mutating call.
        pub fn ghostty_terminal_grid_ref(t: Terminal, point: Point, out: *mut GridRef) -> i32;
        pub fn ghostty_grid_ref_cell(r: *const GridRef, out: *mut u64) -> i32;
        pub fn ghostty_cell_get(cell: u64, data: i32, out: *mut c_void) -> i32;
        /// One datum of the active screen's image storage.
        pub fn ghostty_kitty_graphics_get(g: Graphics, data: i32, out: *mut c_void) -> i32;
        /// The image with client id `id`, or NULL.
        pub fn ghostty_kitty_graphics_image(g: Graphics, id: u32) -> Image;
        pub fn ghostty_kitty_graphics_image_get(image: Image, data: i32, out: *mut c_void) -> i32;
        pub fn ghostty_kitty_graphics_placement_iterator_new(
            allocator: *const c_void,
            out: *mut Placements,
        ) -> i32;
        pub fn ghostty_kitty_graphics_placement_iterator_free(it: Placements);
        pub fn ghostty_kitty_graphics_placement_next(it: Placements) -> bool;
        pub fn ghostty_kitty_graphics_placement_get(
            it: Placements,
            data: i32,
            out: *mut c_void,
        ) -> i32;
        /// Where the current placement's anchor is on the viewport; not 0
        /// when it is virtual or wholly off the screen.
        pub fn ghostty_kitty_graphics_placement_viewport_pos(
            it: Placements,
            image: Image,
            t: Terminal,
            col: *mut i32,
            row: *mut i32,
        ) -> i32;
    }

    /// `GhosttyKittyGraphics`, borrowed from the terminal.
    pub type Graphics = *mut c_void;
    /// `GhosttyKittyGraphicsImage`, borrowed from the storage.
    pub type Image = *const c_void;
    /// `GhosttyKittyGraphicsPlacementIterator`.
    pub type Placements = *mut c_void;

    const _: () = assert!(std::mem::size_of::<Point>() == 24);
    const _: () = assert!(std::mem::size_of::<GridRef>() == 24);

    /// `GhosttyPoint`: an `int` tag, then a 16-byte union aligned to 8
    /// whose coordinate variant is `{ uint16_t x; uint32_t y; }`, so `x`
    /// is at byte 8 and `y` at 12.
    #[repr(C)]
    pub struct Point {
        pub tag: i32,
        pub _gap: u32,
        pub x: u16,
        pub _x_pad: u16,
        pub y: u32,
        pub _rest: u64,
    }

    /// `GhosttyGridRef`, a sized struct.
    #[repr(C)]
    pub struct GridRef {
        pub size: usize,
        pub node: *mut c_void,
        pub x: u16,
        pub y: u16,
    }

    pub const POINT_TAG_ACTIVE: i32 = 0;
    pub const CELL_DATA_CODEPOINT: i32 = 1;
    pub const CELL_DATA_WIDE: i32 = 3;
    pub const CELL_WIDE_SPACER_TAIL: i32 = 2;
    pub const TERMINAL_DATA_KITTY_GRAPHICS: i32 = 30;
    pub const GRAPHICS_DATA_PLACEMENT_ITERATOR: i32 = 1;
    pub const PLACEMENT_DATA_IMAGE_ID: i32 = 1;
    pub const PLACEMENT_DATA_IS_VIRTUAL: i32 = 3;
    pub const PLACEMENT_DATA_Z: i32 = 12;
    pub const IMAGE_DATA_WIDTH: i32 = 3;
    pub const IMAGE_DATA_HEIGHT: i32 = 4;
    pub const IMAGE_DATA_FORMAT: i32 = 5;
    pub const IMAGE_DATA_DATA_PTR: i32 = 7;
    pub const IMAGE_DATA_DATA_LEN: i32 = 8;
    pub const IMAGE_DATA_GENERATION: i32 = 9;
    pub const IMAGE_FORMAT_RGB: i32 = 0;
    pub const IMAGE_FORMAT_RGBA: i32 = 1;
    pub const IMAGE_FORMAT_GRAY_ALPHA: i32 = 3;
    pub const IMAGE_FORMAT_GRAY: i32 = 4;

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

    /// Whether the library is handed a PNG decoder: the process-wide hook,
    /// so it holds for every ghostty terminal until it is set again.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum PngDecoder {
        /// [`decode_png`].
        Registered,
        /// None: the library refuses an `f=100` transmit. Only the screen
        /// check's test clears the hook.
        #[cfg(test)]
        Cleared,
    }

    /// Registers [`decode_png`], or, in a test, clears the hook.
    pub fn set_png_decoder(decoder: PngDecoder) {
        type DecodePng = unsafe extern "C" fn(
            *mut c_void,
            *const c_void,
            *const u8,
            usize,
            *mut SysImage,
        ) -> bool;
        let f: DecodePng = decode_png;
        let value = match decoder {
            PngDecoder::Registered => f as *const c_void,
            #[cfg(test)]
            PngDecoder::Cleared => std::ptr::null(),
        };
        // SAFETY: `value` is NULL, which clears the hook, or `f`, which
        // matches `GhosttySysDecodePngFn` and is a static function the
        // library may keep for the process's life.
        let r = unsafe { ghostty_sys_set(SYS_OPT_DECODE_PNG, value) };
        assert_eq!(
            r, 0,
            "setting ghostty's PNG decoder to {decoder:?} failed with {r}"
        );
    }
}
