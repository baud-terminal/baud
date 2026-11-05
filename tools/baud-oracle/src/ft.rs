//! **FreeType, declared as the extern lines it is**: the six calls the
//! oracles make, the prefix of `FT_FaceRec` and `FT_GlyphSlotRec` up to the
//! one field each is read for (`glyph`, `outline`), and `FT_Outline`. The
//! structs are only ever reached through FreeType's own pointers, so a
//! prefix is all that has to be laid out, and the `abi` tests hold the two
//! offsets read and `FT_Outline`'s whole layout to `freetype.h`.
//!
//! What freetype-py did is reproduced call for call: one library, a face
//! opened at index 0 with `FT_New_Face`, design coordinates as 16.16, and
//! `FT_Load_Glyph` with `FT_LOAD_NO_SCALE | FT_LOAD_NO_HINTING`.

#![allow(unsafe_code)]

use core::ffi::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ushort, c_void};
use std::path::Path;

/// `FT_Pos`.
pub type Pos = c_long;
/// `FT_Fixed`, 16.16.
pub type Fixed = c_long;
/// `FT_Error`.
pub type Error = c_int;

/// `FT_LOAD_NO_SCALE`.
pub const LOAD_NO_SCALE: i32 = 1 << 0;
/// `FT_LOAD_NO_HINTING`.
pub const LOAD_NO_HINTING: i32 = 1 << 1;
/// `FT_CURVE_TAG_ON`, a tag's on-curve bit.
pub const CURVE_TAG_ON: u8 = 0x01;
/// `FT_LOAD_RENDER`.
pub const LOAD_RENDER: i32 = 1 << 2;
/// `FT_LOAD_NO_BITMAP`.
pub const LOAD_NO_BITMAP: i32 = 1 << 3;
/// `FT_LOAD_TARGET_NORMAL`.
pub const LOAD_TARGET_NORMAL: i32 = 0;
/// `FT_FACE_FLAG_SCALABLE`.
pub const FACE_FLAG_SCALABLE: c_long = 1 << 0;

/// `FT_Vector`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Vector {
    /// x, in font units under `FT_LOAD_NO_SCALE`.
    pub x: Pos,
    /// y.
    pub y: Pos,
}

/// `FT_Outline`.
#[repr(C)]
pub struct Outline {
    /// The number of contours.
    pub n_contours: c_ushort,
    /// The number of points.
    pub n_points: c_ushort,
    /// `n_points` points.
    pub points: *const Vector,
    /// `n_points` tags.
    pub tags: *const c_uchar,
    /// `n_contours` contour end indices.
    pub contours: *const c_ushort,
    /// The outline's flags.
    pub flags: c_int,
}

#[repr(C)]
struct Generic {
    data: *mut c_void,
    finalizer: *mut c_void,
}

#[repr(C)]
struct BBox {
    x_min: Pos,
    y_min: Pos,
    x_max: Pos,
    y_max: Pos,
}

/// `FT_FaceRec`, to `glyph`.
#[repr(C)]
struct FaceRec {
    num_faces: c_long,
    face_index: c_long,
    face_flags: c_long,
    style_flags: c_long,
    num_glyphs: c_long,
    family_name: *mut c_char,
    style_name: *mut c_char,
    num_fixed_sizes: c_int,
    available_sizes: *mut c_void,
    num_charmaps: c_int,
    charmaps: *mut c_void,
    generic: Generic,
    bbox: BBox,
    units_per_em: c_ushort,
    ascender: c_short,
    descender: c_short,
    height: c_short,
    max_advance_width: c_short,
    max_advance_height: c_short,
    underline_position: c_short,
    underline_thickness: c_short,
    glyph: *mut GlyphSlotRec,
}

/// `FT_Glyph_Metrics`.
#[repr(C)]
struct Metrics {
    width: Pos,
    height: Pos,
    hori_bearing_x: Pos,
    hori_bearing_y: Pos,
    hori_advance: Pos,
    vert_bearing_x: Pos,
    vert_bearing_y: Pos,
    vert_advance: Pos,
}

/// `FT_Bitmap`.
#[repr(C)]
struct Bitmap {
    rows: c_uint,
    width: c_uint,
    pitch: c_int,
    buffer: *mut c_uchar,
    num_grays: c_ushort,
    pixel_mode: c_uchar,
    palette_mode: c_uchar,
    palette: *mut c_void,
}

/// `FT_GlyphSlotRec`, to `outline`.
#[repr(C)]
struct GlyphSlotRec {
    library: *mut c_void,
    face: *mut c_void,
    next: *mut c_void,
    glyph_index: c_uint,
    generic: Generic,
    metrics: Metrics,
    linear_hori_advance: Fixed,
    linear_vert_advance: Fixed,
    advance: Vector,
    format: c_uint,
    bitmap: Bitmap,
    bitmap_left: c_int,
    bitmap_top: c_int,
    outline: Outline,
}

#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}

#[link(name = "freetype")]
unsafe extern "C" {
    fn FT_Init_FreeType(library: *mut *mut Opaque) -> Error;
    fn FT_Library_Version(
        library: *mut Opaque,
        major: *mut c_int,
        minor: *mut c_int,
        patch: *mut c_int,
    );
    fn FT_New_Face(
        library: *mut Opaque,
        path: *const c_char,
        index: c_long,
        face: *mut *mut FaceRec,
    ) -> Error;
    fn FT_Done_Face(face: *mut FaceRec) -> Error;
    fn FT_Set_Var_Design_Coordinates(face: *mut FaceRec, n: c_uint, coords: *const Fixed) -> Error;
    fn FT_Load_Glyph(face: *mut FaceRec, glyph: c_uint, flags: i32) -> Error;
    fn FT_Set_Char_Size(
        face: *mut FaceRec,
        width: c_long,
        height: c_long,
        horz_resolution: c_uint,
        vert_resolution: c_uint,
    ) -> Error;
}

/// One `FT_Library`, as freetype-py keeps one for the process. It is never
/// freed: the oracles run once and exit.
pub struct Library(*mut Opaque);

impl Library {
    /// `FT_Init_FreeType`.
    pub fn new() -> Library {
        let mut l = core::ptr::null_mut();
        // SAFETY: an out-pointer; the error is checked.
        let e = unsafe { FT_Init_FreeType(&mut l) };
        assert_eq!(e, 0, "FT_Init_FreeType: {}", message(e));
        Library(l)
    }

    /// `FT_Library_Version`, as freetype-py's `version()` joined it.
    pub fn version(&self) -> String {
        let (mut a, mut b, mut c) = (0, 0, 0);
        // SAFETY: a live library and three out-pointers.
        unsafe { FT_Library_Version(self.0, &mut a, &mut b, &mut c) };
        format!("{a}.{b}.{c}")
    }

    /// `FT_New_Face(path, 0)`.
    pub fn face(&self, path: &Path) -> Face {
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).expect("a path");
        let mut f = core::ptr::null_mut();
        // SAFETY: a live library, a NUL-terminated path and an out-pointer.
        let e = unsafe { FT_New_Face(self.0, c.as_ptr(), 0, &mut f) };
        assert_eq!(e, 0, "FT_New_Face {}: {}", path.display(), message(e));
        Face(f)
    }
}

impl Library {
    /// `FT_New_Face(path, index)`, the error returned rather than asserted.
    pub fn face_at(&self, path: &Path, index: c_long) -> Result<Face, Error> {
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).expect("a path");
        let mut f = core::ptr::null_mut();
        // SAFETY: a live library, a NUL-terminated path and an out-pointer.
        match unsafe { FT_New_Face(self.0, c.as_ptr(), index, &mut f) } {
            0 => Ok(Face(f)),
            e => Err(e),
        }
    }
}

/// A rendered glyph: the slot's bitmap origin and size, and its rows,
/// `width` bytes each, top row first.
pub struct Rendered {
    /// `bitmap_left`.
    pub left: c_int,
    /// `bitmap_top`.
    pub top: c_int,
    /// `bitmap.width`.
    pub width: u32,
    /// `bitmap.rows`.
    pub rows: u32,
    /// `bitmap.pitch`.
    pub pitch: i32,
    /// `rows * |pitch|` bytes, as freetype-py's `buffer` returned them.
    pub buffer: Vec<u8>,
}

impl Default for Library {
    fn default() -> Self {
        Library::new()
    }
}

/// `FT_Face`.
pub struct Face(*mut FaceRec);

/// One contour of a loaded outline: its points and their tags.
pub struct Contour<'a> {
    /// The points, in font units.
    pub points: &'a [Vector],
    /// Each point's tag.
    pub tags: &'a [u8],
}

impl Face {
    /// `FT_Set_Var_Design_Coordinates`, each value as freetype-py passed
    /// it: `int(round(c * 65536.0))`.
    pub fn set_var_design(&mut self, coords: &[f64]) -> Result<(), Error> {
        let v: Vec<Fixed> = coords.iter().map(|&c| fixed(c)).collect();
        let n = c_uint::try_from(v.len()).expect("a count");
        // SAFETY: a live face and `n` coordinates.
        match unsafe { FT_Set_Var_Design_Coordinates(self.0, n, v.as_ptr()) } {
            0 => Ok(()),
            e => Err(e),
        }
    }

    /// `FT_Load_Glyph(glyph, FT_LOAD_NO_SCALE | FT_LOAD_NO_HINTING)`, then
    /// the slot's outline split at its contour ends.
    pub fn load_unscaled(&mut self, glyph: u32) -> Result<Vec<Contour<'_>>, Error> {
        // SAFETY: a live face; the error is checked before the slot is read.
        let e = unsafe { FT_Load_Glyph(self.0, glyph, LOAD_NO_SCALE | LOAD_NO_HINTING) };
        if e != 0 {
            return Err(e);
        }
        // SAFETY: after a successful load the slot's outline holds
        // `n_points` points and tags and `n_contours` ends, valid until the
        // next load, which the `&mut self` borrow excludes.
        unsafe {
            let o = &(*(*self.0).glyph).outline;
            let n = usize::from(o.n_points);
            let m = usize::from(o.n_contours);
            if m == 0 {
                return Ok(Vec::new());
            }
            let points = core::slice::from_raw_parts(o.points, n);
            let tags = core::slice::from_raw_parts(o.tags, n);
            let ends = core::slice::from_raw_parts(o.contours, m);
            let mut out = Vec::with_capacity(m);
            let mut start = 0;
            for &end in ends {
                let end = usize::from(end) + 1;
                out.push(Contour {
                    points: &points[start..end],
                    tags: &tags[start..end],
                });
                start = end;
            }
            Ok(out)
        }
    }
}

impl Face {
    /// `face_flags`.
    pub fn flags(&self) -> c_long {
        // SAFETY: a live face.
        unsafe { (*self.0).face_flags }
    }

    /// `num_glyphs`.
    pub fn num_glyphs(&self) -> c_long {
        // SAFETY: a live face.
        unsafe { (*self.0).num_glyphs }
    }

    /// `FT_Set_Char_Size(width, height, hres, vres)`, 26.6 sizes.
    pub fn set_char_size(
        &mut self,
        width: c_long,
        height: c_long,
        h: u32,
        v: u32,
    ) -> Result<(), Error> {
        // SAFETY: a live face.
        match unsafe { FT_Set_Char_Size(self.0, width, height, h, v) } {
            0 => Ok(()),
            e => Err(e),
        }
    }

    /// `FT_Load_Glyph(glyph, flags)`, then the slot's bitmap.
    pub fn load_bitmap(&mut self, glyph: u32, flags: i32) -> Result<Rendered, Error> {
        // SAFETY: a live face; the error is checked before the slot is read.
        let e = unsafe { FT_Load_Glyph(self.0, glyph, flags) };
        if e != 0 {
            return Err(e);
        }
        // SAFETY: after a successful load the slot's bitmap holds `rows`
        // rows of `|pitch|` bytes (none when `buffer` is null), valid until
        // the next load, which the `&mut self` borrow excludes.
        unsafe {
            let slot = &*(*self.0).glyph;
            let b = &slot.bitmap;
            let len = b.rows as usize * b.pitch.unsigned_abs() as usize;
            let buffer = if b.buffer.is_null() || len == 0 {
                Vec::new()
            } else {
                core::slice::from_raw_parts(b.buffer, len).to_vec()
            };
            Ok(Rendered {
                left: slot.bitmap_left,
                top: slot.bitmap_top,
                width: b.width,
                rows: b.rows,
                pitch: b.pitch,
                buffer,
            })
        }
    }
}

impl Drop for Face {
    fn drop(&mut self) {
        // SAFETY: the face this value owns.
        unsafe { FT_Done_Face(self.0) };
    }
}

/// Python's `int(round(c * 65536.0))`: round half to even.
fn fixed(c: f64) -> Fixed {
    (c * 65536.0).round_ties_even() as Fixed
}

/// What freetype-py's `FT_Exception` printed for an error, `str(e)`:
/// `"FT_Exception:  (<message>)"`, its message argument being empty.
pub fn exception(e: Error) -> String {
    format!("FT_Exception:  ({})", message(e))
}

/// An error code's text, from `fterrdef.h`; `"unknown error"` for a code it
/// does not define, as freetype-py answered one.
///
/// **freetype-py's own table is not used**: eight of its strings misquote
/// the header -- `"horizontal header (hhea, table missing"` for 0x8F, and
/// the same `,` for `)` at 0x90, 0x92, 0x93, 0x94, 0x96, 0x9A and 0x9B.
/// No committed row holds one of those codes; this is FreeType's text.
pub fn message(e: Error) -> &'static str {
    ERRORS
        .iter()
        .find(|&&(code, _)| code == e)
        .map_or("unknown error", |&(_, m)| m)
}

/// `fterrdef.h`'s `FT_NOERRORDEF_` and `FT_ERRORDEF_` lines, code and
/// text, in its order. The `abi` tests expand the header's own macros and
/// hold this to them.
pub const ERRORS: [(Error, &str); 95] = [
    (0x00, "no error"),
    (0x01, "cannot open resource"),
    (0x02, "unknown file format"),
    (0x03, "broken file"),
    (0x04, "invalid FreeType version"),
    (0x05, "module version is too low"),
    (0x06, "invalid argument"),
    (0x07, "unimplemented feature"),
    (0x08, "broken table"),
    (0x09, "broken offset within table"),
    (0x0A, "array allocation size too large"),
    (0x0B, "missing module"),
    (0x0C, "missing property"),
    (0x10, "invalid glyph index"),
    (0x11, "invalid character code"),
    (0x12, "unsupported glyph image format"),
    (0x13, "cannot render this glyph format"),
    (0x14, "invalid outline"),
    (0x15, "invalid composite glyph"),
    (0x16, "too many hints"),
    (0x17, "invalid pixel size"),
    (0x18, "invalid SVG document"),
    (0x20, "invalid object handle"),
    (0x21, "invalid library handle"),
    (0x22, "invalid module handle"),
    (0x23, "invalid face handle"),
    (0x24, "invalid size handle"),
    (0x25, "invalid glyph slot handle"),
    (0x26, "invalid charmap handle"),
    (0x27, "invalid cache manager handle"),
    (0x28, "invalid stream handle"),
    (0x30, "too many modules"),
    (0x31, "too many extensions"),
    (0x40, "out of memory"),
    (0x41, "unlisted object"),
    (0x51, "cannot open stream"),
    (0x52, "invalid stream seek"),
    (0x53, "invalid stream skip"),
    (0x54, "invalid stream read"),
    (0x55, "invalid stream operation"),
    (0x56, "invalid frame operation"),
    (0x57, "nested frame access"),
    (0x58, "invalid frame read"),
    (0x60, "raster uninitialized"),
    (0x61, "raster corrupted"),
    (0x62, "raster overflow"),
    (0x63, "negative height while rastering"),
    (0x70, "too many registered caches"),
    (0x80, "invalid opcode"),
    (0x81, "too few arguments"),
    (0x82, "stack overflow"),
    (0x83, "code overflow"),
    (0x84, "bad argument"),
    (0x85, "division by zero"),
    (0x86, "invalid reference"),
    (0x87, "found debug opcode"),
    (0x88, "found ENDF opcode in execution stream"),
    (0x89, "nested DEFS"),
    (0x8A, "invalid code range"),
    (0x8B, "execution context too long"),
    (0x8C, "too many function definitions"),
    (0x8D, "too many instruction definitions"),
    (0x8E, "SFNT font table missing"),
    (0x8F, "horizontal header (hhea) table missing"),
    (0x90, "locations (loca) table missing"),
    (0x91, "name table missing"),
    (0x92, "character map (cmap) table missing"),
    (0x93, "horizontal metrics (hmtx) table missing"),
    (0x94, "PostScript (post) table missing"),
    (0x95, "invalid horizontal metrics"),
    (0x96, "invalid character map (cmap) format"),
    (0x97, "invalid ppem value"),
    (0x98, "invalid vertical metrics"),
    (0x99, "could not find context"),
    (0x9A, "invalid PostScript (post) table format"),
    (0x9B, "invalid PostScript (post) table"),
    (0x9C, "found FDEF or IDEF opcode in glyf bytecode"),
    (0x9D, "missing bitmap in strike"),
    (0x9E, "SVG hooks have not been set"),
    (0xA0, "opcode syntax error"),
    (0xA1, "argument stack underflow"),
    (0xA2, "ignore"),
    (0xA3, "no Unicode glyph name found"),
    (0xA4, "glyph too big for hinting"),
    (0xB0, "`STARTFONT' field missing"),
    (0xB1, "`FONT' field missing"),
    (0xB2, "`SIZE' field missing"),
    (0xB3, "`FONTBOUNDINGBOX' field missing"),
    (0xB4, "`CHARS' field missing"),
    (0xB5, "`STARTCHAR' field missing"),
    (0xB6, "`ENCODING' field missing"),
    (0xB7, "`BBX' field missing"),
    (0xB8, "`BBX' too big"),
    (0xB9, "Font header corrupted or missing fields"),
    (0xBA, "Font glyphs corrupted or missing fields"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::ask_the_headers;
    use core::mem::{offset_of, size_of};

    #[test]
    fn the_layouts_are_freetype_s_own() {
        let h = ask_the_headers();
        assert_eq!(h["FT_Vector.size"], size_of::<Vector>() as i128);
        assert_eq!(h["FT_Vector.y"], offset_of!(Vector, y) as i128);
        assert_eq!(h["FT_Outline.size"], size_of::<Outline>() as i128);
        assert_eq!(
            h["FT_Outline.n_contours"],
            offset_of!(Outline, n_contours) as i128
        );
        assert_eq!(
            h["FT_Outline.n_points"],
            offset_of!(Outline, n_points) as i128
        );
        assert_eq!(h["FT_Outline.points"], offset_of!(Outline, points) as i128);
        assert_eq!(h["FT_Outline.tags"], offset_of!(Outline, tags) as i128);
        assert_eq!(
            h["FT_Outline.contours"],
            offset_of!(Outline, contours) as i128
        );
        assert_eq!(h["FT_Outline.flags"], offset_of!(Outline, flags) as i128);
        assert_eq!(
            h["FT_Outline.n_contours.size"],
            size_of::<c_ushort>() as i128
        );
        assert_eq!(h["FT_Outline.contours.elem"], size_of::<c_ushort>() as i128);
        assert_eq!(h["FT_FaceRec.glyph"], offset_of!(FaceRec, glyph) as i128);
        assert_eq!(
            h["FT_FaceRec.face_flags"],
            offset_of!(FaceRec, face_flags) as i128
        );
        assert_eq!(
            h["FT_FaceRec.num_glyphs"],
            offset_of!(FaceRec, num_glyphs) as i128
        );
        assert_eq!(
            h["FT_GlyphSlotRec.bitmap"],
            offset_of!(GlyphSlotRec, bitmap) as i128
        );
        assert_eq!(
            h["FT_GlyphSlotRec.bitmap_left"],
            offset_of!(GlyphSlotRec, bitmap_left) as i128
        );
        assert_eq!(
            h["FT_GlyphSlotRec.bitmap_top"],
            offset_of!(GlyphSlotRec, bitmap_top) as i128
        );
        assert_eq!(h["FT_Bitmap.size"], size_of::<Bitmap>() as i128);
        assert_eq!(h["FT_Bitmap.rows"], offset_of!(Bitmap, rows) as i128);
        assert_eq!(h["FT_Bitmap.width"], offset_of!(Bitmap, width) as i128);
        assert_eq!(h["FT_Bitmap.pitch"], offset_of!(Bitmap, pitch) as i128);
        assert_eq!(h["FT_Bitmap.buffer"], offset_of!(Bitmap, buffer) as i128);
        assert_eq!(h["FT_LOAD_RENDER"], LOAD_RENDER as i128);
        assert_eq!(h["FT_LOAD_NO_BITMAP"], LOAD_NO_BITMAP as i128);
        assert_eq!(h["FT_LOAD_TARGET_NORMAL"], LOAD_TARGET_NORMAL as i128);
        assert_eq!(h["FT_FACE_FLAG_SCALABLE"], FACE_FLAG_SCALABLE as i128);
        assert_eq!(
            h["FT_GlyphSlotRec.outline"],
            offset_of!(GlyphSlotRec, outline) as i128
        );
        assert_eq!(h["FT_Pos.size"], size_of::<Pos>() as i128);
        assert_eq!(h["FT_Fixed.size"], size_of::<Fixed>() as i128);
        assert_eq!(h["FT_Error.size"], size_of::<Error>() as i128);
        assert_eq!(h["FT_LOAD_NO_SCALE"], LOAD_NO_SCALE as i128);
        assert_eq!(h["FT_LOAD_NO_HINTING"], LOAD_NO_HINTING as i128);
        assert_eq!(h["FT_CURVE_TAG_ON"], CURVE_TAG_ON as i128);
    }

    /// Every code the header defines, with its text, and no other.
    #[test]
    fn the_error_texts_are_the_header_s() {
        let want = crate::abi::error_texts();
        let have: Vec<(i32, String)> = ERRORS.iter().map(|&(c, m)| (c, m.to_string())).collect();
        assert_eq!(have, want);
    }

    #[test]
    fn the_linked_freetype_is_the_pinned_one() {
        let h = ask_the_headers();
        let header = format!(
            "{}.{}.{}",
            h["FREETYPE_MAJOR"], h["FREETYPE_MINOR"], h["FREETYPE_PATCH"]
        );
        let lib = Library::new();
        assert_eq!(header, lib.version());
        assert_eq!(lib.version(), crate::FREETYPE);
    }

    /// The one row the committed files hold, spelled as freetype-py
    /// spelled it, and an undefined code's fallback.
    #[test]
    fn an_error_prints_as_freetype_py_printed_it() {
        assert_eq!(exception(0x15), "FT_Exception:  (invalid composite glyph)");
        assert_eq!(exception(0x7fff), "FT_Exception:  (unknown error)");
    }

    #[test]
    fn a_design_coordinate_rounds_as_python_s_round() {
        assert_eq!(fixed(650.0), 650 << 16);
        assert_eq!(fixed(0.5 / 65536.0), 0);
        assert_eq!(fixed(1.5 / 65536.0), 2);
    }
}
