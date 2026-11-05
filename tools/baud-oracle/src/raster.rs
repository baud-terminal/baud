//! **FreeType as the rasterisation oracle for `baud-font/tests/raster.rs`**:
//! every glyph of one face at the requested pixel sizes, through FreeType's
//! unhinted smooth rasteriser (`FT_LOAD_NO_HINTING | FT_LOAD_TARGET_NORMAL`,
//! `[[source: freetype-ftgrays]]`), each bitmap printed with the origin
//! FreeType gave it, so the test can rasterise the same glyph into the same
//! frame and compare pixel by pixel. The port of the freetype-py script
//! that did this, call for call: a face at its index, `FT_Set_Char_Size` at
//! 72 dpi so a point is a pixel, and `FT_LOAD_NO_BITMAP`, because a face
//! with embedded strikes (Monaco has them at small sizes) would otherwise
//! answer with the strike rather than the outline.
//!
//! `baud-oracle raster <font file> <face index> <px> [<px> ...]` prints,
//! per glyph and size, two lines:
//!
//! ```text
//! g <gid> <px> <bitmap_left> <bitmap_top> <width> <rows>
//! <width*rows bytes as hex, row-major, top row first>
//! ```
//!
//! and nothing at all for a face with no outlines to rasterise.

use std::path::Path;
use std::process::ExitCode;

use crate::ft;
use crate::pyfmt;

/// Every glyph of the face at `path`, `index`, at each size, as the
/// script printed it. `Err` says which call failed.
pub fn lines(path: &Path, index: i64, sizes: &[f64]) -> Result<String, String> {
    let lib = ft::Library::new();
    crate::pinned_freetype(&lib);
    let index = core::ffi::c_long::try_from(index).map_err(|e| e.to_string())?;
    let mut face = lib
        .face_at(path, index)
        .map_err(|e| format!("{}: {}", path.display(), ft::exception(e)))?;
    if face.flags() & ft::FACE_FLAG_SCALABLE == 0 {
        return Ok(String::new());
    }
    let flags = ft::LOAD_NO_HINTING | ft::LOAD_NO_BITMAP | ft::LOAD_RENDER | ft::LOAD_TARGET_NORMAL;
    let mut out = String::new();
    for &px in sizes {
        // 26.6 fixed point, `int(round(px * 64))`: ties to even.
        let size = (px * 64.0).round_ties_even() as core::ffi::c_long;
        face.set_char_size(size, 0, 72, 72)
            .map_err(|e| format!("set_char_size {px}: {}", ft::exception(e)))?;
        for gid in 0..face.num_glyphs() {
            let gid = u32::try_from(gid).map_err(|e| e.to_string())?;
            let b = face
                .load_bitmap(gid, flags)
                .map_err(|e| format!("load_glyph {gid}: {}", ft::exception(e)))?;
            let (w, h) = (b.width as usize, b.rows as usize);
            let pitch = usize::try_from(b.pitch)
                .ok()
                .filter(|&p| b.buffer.len() == h * p)
                .ok_or_else(|| {
                    format!(
                        "AssertionError: ({gid}, {w}, {h}, {}, {})",
                        b.pitch,
                        b.buffer.len()
                    )
                })?;
            out += &format!(
                "g {gid} {} {} {} {w} {h}\n",
                pyfmt::general(px),
                b.left,
                b.top
            );
            for r in 0..h {
                for byte in &b.buffer[r * pitch..r * pitch + w] {
                    out += &format!("{byte:02x}");
                }
            }
            out.push('\n');
        }
    }
    Ok(out)
}

/// `baud-oracle raster`'s arguments: the font, the face index and the
/// sizes, each parsed as Python's `int` and `float` read them.
pub fn main(args: &[String]) -> ExitCode {
    let [path, index, sizes @ ..] = args else {
        eprintln!("usage: baud-oracle raster <font file> <face index> <px> [<px> ...]");
        return ExitCode::FAILURE;
    };
    let Ok(index) = index.trim().parse::<i64>() else {
        eprintln!("baud-oracle raster: invalid literal for int(): {index:?}");
        return ExitCode::FAILURE;
    };
    let mut px = Vec::with_capacity(sizes.len());
    for s in sizes {
        match s.trim().parse::<f64>() {
            Ok(v) => px.push(v),
            Err(_) => {
                eprintln!("baud-oracle raster: could not convert string to float: {s:?}");
                return ExitCode::FAILURE;
            }
        }
    }
    match lines(Path::new(path), index, &px) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("baud-oracle raster: {e}");
            ExitCode::FAILURE
        }
    }
}
