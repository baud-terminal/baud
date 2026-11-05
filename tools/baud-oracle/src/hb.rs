//! **HarfBuzz, declared as the extern lines it is** -- the calls the eight
//! oracles make and nothing else, and the four structs they read, laid out
//! as `hb-common.h`, `hb-buffer.h` and `hb-font.h` lay them out. The
//! `abi` tests compile a C probe against the installed headers and hold
//! each struct's size, the offset of every field read, and the constants
//! to them, because a wrong field order compiles, links and reads the
//! wrong member.
//!
//! No `-sys` crate: `harfbuzz-sys`'s `bundled` feature vendors HarfBuzz
//! 8.4.0, and a silently swapped engine is the one thing an oracle cannot
//! survive. The library linked is whatever `pkg-config` names, which
//! `tools/shell.nix` pins, and [`version`] is checked against
//! [`crate::HARFBUZZ`] before any oracle writes a line.

#![allow(unsafe_code)]

use core::ffi::{c_char, c_int, c_uint, c_void};
use std::ffi::CStr;
use std::path::Path;

/// `hb_codepoint_t`.
pub type Codepoint = u32;
/// `hb_position_t`.
pub type Position = i32;
/// `hb_tag_t`.
pub type Tag = u32;

/// `hb_variation_t`: an axis tag and a design-space value.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Variation {
    /// The axis.
    pub tag: Tag,
    /// The design-space value, a C `float`.
    pub value: f32,
}

/// `hb_feature_t`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Feature {
    /// The feature.
    pub tag: Tag,
    /// Its value; 0 turns it off.
    pub value: u32,
    /// The first cluster it applies to.
    pub start: c_uint,
    /// One past the last; `HB_FEATURE_GLOBAL_END` is `(unsigned) -1`.
    pub end: c_uint,
}

/// hb_glyph_info_t. `var1` and `var2` are a four-byte union, hb_var_int_t,
/// and private.
#[repr(C)]
pub struct GlyphInfo {
    /// After shaping, the glyph id.
    pub codepoint: Codepoint,
    mask: u32,
    /// The cluster the glyph belongs to.
    pub cluster: u32,
    var1: u32,
    var2: u32,
}

/// hb_glyph_position_t; `var` is a private hb_var_int_t.
#[repr(C)]
pub struct GlyphPosition {
    /// The pen's advance along x.
    pub x_advance: Position,
    /// The pen's advance along y.
    pub y_advance: Position,
    /// The glyph's offset along x from the pen.
    pub x_offset: Position,
    /// The glyph's offset along y from the pen.
    pub y_offset: Position,
    var: u32,
}

/// `HB_MEMORY_MODE_DUPLICATE`: the blob takes a copy.
pub const MEMORY_MODE_DUPLICATE: c_int = 0;

#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}

type MoveTo = extern "C" fn(*mut Opaque, *mut c_void, *mut c_void, f32, f32, *mut c_void);
type QuadTo = extern "C" fn(*mut Opaque, *mut c_void, *mut c_void, f32, f32, f32, f32, *mut c_void);
type CubicTo =
    extern "C" fn(*mut Opaque, *mut c_void, *mut c_void, f32, f32, f32, f32, f32, f32, *mut c_void);
type Close = extern "C" fn(*mut Opaque, *mut c_void, *mut c_void, *mut c_void);

#[link(name = "harfbuzz")]
unsafe extern "C" {
    fn hb_version_string() -> *const c_char;
    fn hb_blob_create_from_file_or_fail(path: *const c_char) -> *mut Opaque;
    fn hb_blob_create(
        data: *const c_char,
        length: c_uint,
        mode: c_int,
        user_data: *mut c_void,
        destroy: Option<extern "C" fn(*mut c_void)>,
    ) -> *mut Opaque;
    fn hb_blob_destroy(blob: *mut Opaque);
    fn hb_face_create(blob: *mut Opaque, index: c_uint) -> *mut Opaque;
    fn hb_face_destroy(face: *mut Opaque);
    fn hb_face_get_glyph_count(face: *const Opaque) -> c_uint;
    fn hb_face_get_upem(face: *const Opaque) -> c_uint;
    fn hb_font_create(face: *mut Opaque) -> *mut Opaque;
    fn hb_font_destroy(font: *mut Opaque);
    fn hb_font_set_scale(font: *mut Opaque, x: c_int, y: c_int);
    fn hb_font_set_variations(font: *mut Opaque, v: *const Variation, n: c_uint);
    fn hb_font_get_var_coords_normalized(font: *mut Opaque, n: *mut c_uint) -> *const c_int;
    fn hb_font_get_glyph_h_advance(font: *mut Opaque, glyph: Codepoint) -> Position;
    fn hb_font_get_glyph_name(
        font: *mut Opaque,
        glyph: Codepoint,
        name: *mut c_char,
        size: c_uint,
    ) -> c_int;
    fn hb_font_draw_glyph(font: *mut Opaque, glyph: Codepoint, f: *mut Opaque, data: *mut c_void);
    fn hb_draw_funcs_create() -> *mut Opaque;
    fn hb_draw_funcs_destroy(f: *mut Opaque);
    fn hb_draw_funcs_set_move_to_func(f: *mut Opaque, g: MoveTo, u: *mut c_void, d: *mut c_void);
    fn hb_draw_funcs_set_line_to_func(f: *mut Opaque, g: MoveTo, u: *mut c_void, d: *mut c_void);
    fn hb_draw_funcs_set_quadratic_to_func(
        f: *mut Opaque,
        g: QuadTo,
        u: *mut c_void,
        d: *mut c_void,
    );
    fn hb_draw_funcs_set_cubic_to_func(f: *mut Opaque, g: CubicTo, u: *mut c_void, d: *mut c_void);
    fn hb_draw_funcs_set_close_path_func(f: *mut Opaque, g: Close, u: *mut c_void, d: *mut c_void);
    fn hb_buffer_create() -> *mut Opaque;
    fn hb_buffer_destroy(b: *mut Opaque);
    fn hb_buffer_add_codepoints(
        b: *mut Opaque,
        text: *const Codepoint,
        len: c_int,
        offset: c_uint,
        item_len: c_int,
    );
    fn hb_buffer_guess_segment_properties(b: *mut Opaque);
    fn hb_buffer_get_length(b: *mut Opaque) -> c_uint;
    fn hb_buffer_get_glyph_infos(b: *mut Opaque, n: *mut c_uint) -> *const GlyphInfo;
    fn hb_buffer_get_glyph_positions(b: *mut Opaque, n: *mut c_uint) -> *const GlyphPosition;
    fn hb_feature_from_string(s: *const c_char, len: c_int, f: *mut Feature) -> c_int;
    fn hb_tag_from_string(s: *const c_char, len: c_int) -> Tag;
    fn hb_shape(font: *mut Opaque, b: *mut Opaque, f: *const Feature, n: c_uint);
}

/// The linked library's own `hb_version_string`.
pub fn version() -> String {
    // SAFETY: a static NUL-terminated string the library owns.
    unsafe { CStr::from_ptr(hb_version_string()) }
        .to_string_lossy()
        .into_owned()
}

/// A tag from its four letters, as `hb_tag_from_string` reads it.
pub fn tag(name: &str) -> Tag {
    // SAFETY: the pointer and length describe `name`.
    unsafe { hb_tag_from_string(name.as_ptr().cast(), len(name.len())) }
}

fn len(n: usize) -> c_int {
    c_int::try_from(n).expect("a length that fits a C int")
}

/// `hb_blob_t`.
pub struct Blob(*mut Opaque);

impl Blob {
    /// `hb_blob_create_from_file_or_fail`, which is what uharfbuzz's
    /// `Blob.from_file_path` called.
    pub fn from_file(path: &Path) -> Blob {
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).expect("a path");
        // SAFETY: a NUL-terminated path; a null return is checked.
        let b = unsafe { hb_blob_create_from_file_or_fail(c.as_ptr()) };
        assert!(!b.is_null(), "HarfBuzz could not open {}", path.display());
        Blob(b)
    }

    /// `hb_blob_create` with `HB_MEMORY_MODE_DUPLICATE`, the mode
    /// uharfbuzz's `Blob(bytes)` used.
    pub fn copy_of(data: &[u8]) -> Blob {
        let n = c_uint::try_from(data.len()).expect("a blob that fits a C uint");
        // SAFETY: the library copies `n` bytes from `data` before returning.
        Blob(unsafe {
            hb_blob_create(
                data.as_ptr().cast(),
                n,
                MEMORY_MODE_DUPLICATE,
                core::ptr::null_mut(),
                None,
            )
        })
    }
}

impl Drop for Blob {
    fn drop(&mut self) {
        // SAFETY: the reference this value owns.
        unsafe { hb_blob_destroy(self.0) }
    }
}

/// `hb_face_t`, at index 0, which is uharfbuzz's `Face(blob)`.
pub struct Face(*mut Opaque);

impl Face {
    /// `hb_face_create(blob, 0)`.
    pub fn new(blob: &Blob) -> Face {
        // SAFETY: the face takes its own reference to the blob.
        Face(unsafe { hb_face_create(blob.0, 0) })
    }

    /// `hb_face_get_glyph_count`.
    pub fn glyph_count(&self) -> u32 {
        // SAFETY: a live face.
        unsafe { hb_face_get_glyph_count(self.0) }
    }

    /// `hb_face_get_upem`.
    pub fn upem(&self) -> u32 {
        // SAFETY: a live face.
        unsafe { hb_face_get_upem(self.0) }
    }
}

impl Drop for Face {
    fn drop(&mut self) {
        // SAFETY: the reference this value owns.
        unsafe { hb_face_destroy(self.0) }
    }
}

/// `hb_font_t`, made by `hb_font_create` and so at the face's default
/// scale until [`Font::set_scale`] says otherwise.
pub struct Font(*mut Opaque);

impl Font {
    /// `hb_font_create`.
    pub fn new(face: &Face) -> Font {
        // SAFETY: the font takes its own reference to the face.
        Font(unsafe { hb_font_create(face.0) })
    }

    /// `hb_font_set_scale`.
    pub fn set_scale(&mut self, x: u32, y: u32) {
        let (x, y) = (
            c_int::try_from(x).expect("a scale"),
            c_int::try_from(y).expect("a scale"),
        );
        // SAFETY: a live font.
        unsafe { hb_font_set_scale(self.0, x, y) }
    }

    /// `hb_font_set_variations`, in the order given, each value narrowed
    /// to the C `float` the struct holds, as uharfbuzz's assignment did.
    pub fn set_variations(&mut self, axes: &[(&str, f64)]) {
        let v: Vec<Variation> = axes
            .iter()
            .map(|&(name, value)| Variation {
                tag: tag(name),
                // A C `double` to `float` assignment rounds to nearest.
                value: value as f32,
            })
            .collect();
        let n = c_uint::try_from(v.len()).expect("a count");
        // SAFETY: `v` holds `n` variations for the call's duration.
        unsafe { hb_font_set_variations(self.0, v.as_ptr(), n) }
    }

    /// `hb_font_get_var_coords_normalized`: the 2.14 coordinates as the
    /// integers HarfBuzz stores.
    pub fn var_coords_normalized(&self) -> Vec<i32> {
        let mut n: c_uint = 0;
        // SAFETY: the array, `n` long, lives as long as the font's state.
        let p = unsafe { hb_font_get_var_coords_normalized(self.0, &mut n) };
        if p.is_null() || n == 0 {
            return Vec::new();
        }
        // SAFETY: `p` points at `n` ints.
        unsafe { core::slice::from_raw_parts(p, n as usize) }.to_vec()
    }

    /// `hb_font_get_glyph_h_advance`.
    pub fn h_advance(&self, glyph: u32) -> i32 {
        // SAFETY: a live font; any glyph id is accepted.
        unsafe { hb_font_get_glyph_h_advance(self.0, glyph) }
    }

    /// `hb_font_get_glyph_name` into a 64-byte buffer, as uharfbuzz's
    /// `get_glyph_name` did; `None` where HarfBuzz has no name.
    pub fn glyph_name(&self, glyph: u32) -> Option<String> {
        let mut buf = [0 as c_char; 64];
        // SAFETY: the buffer is 64 bytes and HarfBuzz writes at most that.
        let ok = unsafe { hb_font_get_glyph_name(self.0, glyph, buf.as_mut_ptr(), 64) };
        if ok == 0 {
            return None;
        }
        // SAFETY: HarfBuzz NUL-terminates within the buffer.
        let s = unsafe { CStr::from_ptr(buf.as_ptr()) };
        Some(s.to_string_lossy().into_owned())
    }

    /// `hb_font_draw_glyph`, every call the draw session makes handed to
    /// `pen` in order.
    pub fn draw(&self, glyph: u32, pen: &mut dyn Pen) {
        let mut pen: &mut dyn Pen = pen;
        let data: *mut c_void = (&raw mut pen).cast();
        // SAFETY: the funcs object is made and destroyed around one call,
        // and `data` points at `pen` for that call's whole duration.
        unsafe {
            let f = hb_draw_funcs_create();
            let none = core::ptr::null_mut();
            hb_draw_funcs_set_move_to_func(f, move_to, none, none);
            hb_draw_funcs_set_line_to_func(f, line_to, none, none);
            hb_draw_funcs_set_quadratic_to_func(f, quad_to, none, none);
            hb_draw_funcs_set_cubic_to_func(f, cubic_to, none, none);
            hb_draw_funcs_set_close_path_func(f, close_path, none, none);
            hb_font_draw_glyph(self.0, glyph, f, data);
            hb_draw_funcs_destroy(f);
        }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        // SAFETY: the reference this value owns.
        unsafe { hb_font_destroy(self.0) }
    }
}

/// What a draw session emits, each coordinate the C `float` HarfBuzz
/// passed.
pub trait Pen {
    /// `move_to`.
    fn move_to(&mut self, x: f32, y: f32);
    /// `line_to`.
    fn line_to(&mut self, x: f32, y: f32);
    /// `quadratic_to`.
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32);
    /// `cubic_to`.
    fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32);
    /// `close_path`.
    fn close(&mut self);
}

fn pen<'a>(data: *mut c_void) -> &'a mut dyn Pen {
    // SAFETY: `data` is the `&mut &mut dyn Pen` `Font::draw` passed.
    unsafe { &mut **data.cast::<&mut dyn Pen>() }
}

extern "C" fn move_to(
    _: *mut Opaque,
    d: *mut c_void,
    _: *mut c_void,
    x: f32,
    y: f32,
    _: *mut c_void,
) {
    pen(d).move_to(x, y);
}

extern "C" fn line_to(
    _: *mut Opaque,
    d: *mut c_void,
    _: *mut c_void,
    x: f32,
    y: f32,
    _: *mut c_void,
) {
    pen(d).line_to(x, y);
}

#[allow(clippy::too_many_arguments)]
extern "C" fn quad_to(
    _: *mut Opaque,
    d: *mut c_void,
    _: *mut c_void,
    cx: f32,
    cy: f32,
    x: f32,
    y: f32,
    _: *mut c_void,
) {
    pen(d).quad_to(cx, cy, x, y);
}

#[allow(clippy::too_many_arguments)]
extern "C" fn cubic_to(
    _: *mut Opaque,
    d: *mut c_void,
    _: *mut c_void,
    c1x: f32,
    c1y: f32,
    c2x: f32,
    c2y: f32,
    x: f32,
    y: f32,
    _: *mut c_void,
) {
    pen(d).cubic_to(c1x, c1y, c2x, c2y, x, y);
}

extern "C" fn close_path(_: *mut Opaque, d: *mut c_void, _: *mut c_void, _: *mut c_void) {
    pen(d).close();
}

/// `hb_buffer_t`.
pub struct Buffer(*mut Opaque);

impl Buffer {
    /// `hb_buffer_create`.
    pub fn new() -> Buffer {
        // SAFETY: a fresh buffer; allocation failure gives the inert one.
        Buffer(unsafe { hb_buffer_create() })
    }

    /// `hb_buffer_add_codepoints(text, len, 0, len)`, then
    /// `hb_buffer_guess_segment_properties`.
    pub fn add_codepoints(&mut self, text: &[u32]) {
        // SAFETY: `text` is `len` codepoints long.
        unsafe {
            hb_buffer_add_codepoints(self.0, text.as_ptr(), len(text.len()), 0, len(text.len()));
            hb_buffer_guess_segment_properties(self.0);
        }
    }

    /// `hb_shape` with `features`.
    pub fn shape(&mut self, font: &Font, features: &[Feature]) {
        let n = c_uint::try_from(features.len()).expect("a count");
        // SAFETY: live font and buffer; `features` is `n` long.
        unsafe { hb_shape(font.0, self.0, features.as_ptr(), n) }
    }

    /// The shaped glyphs, each with its position.
    pub fn glyphs(&self) -> Vec<(&GlyphInfo, &GlyphPosition)> {
        let (mut a, mut b): (c_uint, c_uint) = (0, 0);
        // SAFETY: both arrays are the buffer's and `n` long while it lives
        // unchanged, which the shared borrow guarantees.
        unsafe {
            let n = hb_buffer_get_length(self.0) as usize;
            if n == 0 {
                return Vec::new();
            }
            let i = core::slice::from_raw_parts(hb_buffer_get_glyph_infos(self.0, &mut a), n);
            let p = core::slice::from_raw_parts(hb_buffer_get_glyph_positions(self.0, &mut b), n);
            i.iter().zip(p).collect()
        }
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer::new()
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: the reference this value owns.
        unsafe { hb_buffer_destroy(self.0) }
    }
}

/// A feature from its tag, global, at `value` -- uharfbuzz's
/// `hb_feature_from_string` then `feat.value = value`.
pub fn feature(name: &str, value: u32) -> Feature {
    let mut f = Feature {
        tag: 0,
        value: 0,
        start: 0,
        end: 0,
    };
    // SAFETY: the pointer and length describe `name`; `f` is written.
    let ok = unsafe { hb_feature_from_string(name.as_ptr().cast(), len(name.len()), &mut f) };
    assert!(ok != 0, "HarfBuzz did not parse the feature {name}");
    f.value = value;
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::ask_the_headers;
    use core::mem::{offset_of, size_of};

    #[test]
    fn the_layouts_are_harfbuzz_s_own() {
        let h = ask_the_headers();
        assert_eq!(h["hb_variation_t.size"], size_of::<Variation>() as i128);
        assert_eq!(h["hb_variation_t.tag"], offset_of!(Variation, tag) as i128);
        assert_eq!(
            h["hb_variation_t.value"],
            offset_of!(Variation, value) as i128
        );
        assert_eq!(h["hb_feature_t.size"], size_of::<Feature>() as i128);
        assert_eq!(h["hb_feature_t.tag"], offset_of!(Feature, tag) as i128);
        assert_eq!(h["hb_feature_t.value"], offset_of!(Feature, value) as i128);
        assert_eq!(h["hb_feature_t.start"], offset_of!(Feature, start) as i128);
        assert_eq!(h["hb_feature_t.end"], offset_of!(Feature, end) as i128);
        assert_eq!(h["hb_glyph_info_t.size"], size_of::<GlyphInfo>() as i128);
        assert_eq!(
            h["hb_glyph_info_t.codepoint"],
            offset_of!(GlyphInfo, codepoint) as i128
        );
        assert_eq!(
            h["hb_glyph_info_t.cluster"],
            offset_of!(GlyphInfo, cluster) as i128
        );
        assert_eq!(
            h["hb_glyph_position_t.size"],
            size_of::<GlyphPosition>() as i128
        );
        assert_eq!(
            h["hb_glyph_position_t.x_advance"],
            offset_of!(GlyphPosition, x_advance) as i128
        );
        assert_eq!(
            h["hb_glyph_position_t.x_offset"],
            offset_of!(GlyphPosition, x_offset) as i128
        );
        assert_eq!(
            h["hb_glyph_position_t.y_offset"],
            offset_of!(GlyphPosition, y_offset) as i128
        );
        assert_eq!(h["hb_codepoint_t.size"], size_of::<Codepoint>() as i128);
        assert_eq!(h["hb_position_t.size"], size_of::<Position>() as i128);
        assert_eq!(h["HB_MEMORY_MODE_DUPLICATE"], MEMORY_MODE_DUPLICATE as i128);
        assert_eq!(h["HB_FEATURE_GLOBAL_END"], c_uint::MAX as i128);
    }

    /// The header the probe compiled against is the library linked: the
    /// version macro and the call agree, and both are the pin.
    #[test]
    fn the_linked_harfbuzz_is_the_pinned_one() {
        let h = ask_the_headers();
        let header = format!(
            "{}.{}.{}",
            h["HB_VERSION_MAJOR"], h["HB_VERSION_MINOR"], h["HB_VERSION_MICRO"]
        );
        assert_eq!(header, version());
        assert_eq!(version(), crate::HARFBUZZ);
    }

    /// `uharfbuzz`'s `shape` read a dict `{"mark": True}` through
    /// `hb_feature_from_string`, which makes the feature global.
    #[test]
    fn a_feature_is_global_from_its_tag() {
        let f = feature("kern", 0);
        assert_eq!(f.tag, u32::from_be_bytes(*b"kern"));
        assert_eq!((f.value, f.start, f.end), (0, 0, c_uint::MAX));
    }
}
