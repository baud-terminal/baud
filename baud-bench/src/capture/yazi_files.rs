//! The seven files `tests/recordings/sixel/yazi-walk.raw` walks over: the
//! `gen-yazi-files` example.
//!
//! ```text
//! gen-yazi-files <dir>
//! ```
//!
//! Deterministic test images and PDFs, seeded: photo.jpg, flat.png, alpha.png,
//! screenshot.png, small.png, sparse.pdf and text.pdf. Nothing here is derived
//! from any user file. The JPEG is written by macOS's sips, so it is made on
//! macOS.
//!
//! All but the JPEG are the same bytes on any host: the draws come from one
//! seeded generator, the pictures but the photo take no float from a libm,
//! and the deflate is [`super::deflate`]'s. The photo's noise goes through
//! the host's `log`, `sin` and `cos`, but it draws as many values whatever
//! they come to, so the other files do not depend on them.

use std::ffi::{OsStr, OsString};
use std::process::Command;

use super::deflate;
use super::draws::Draws;

const DOC: &str = "The seven files tests/recordings/sixel/yazi-walk.raw walks over.

    gen-yazi-files <dir>

Deterministic test images and PDFs, seeded: photo.jpg, flat.png, alpha.png,
screenshot.png, small.png, sparse.pdf and text.pdf. Nothing here is derived
from any user file. The JPEG is written by macOS's sips, so it is made on
macOS.";

/// The generator's seed.
pub const SEED: u32 = 20_261_009;

/// A PNG's colour type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Colour {
    /// Three bytes a pixel: colour type 2.
    Rgb,
    /// Four: colour type 6.
    Rgba,
}

impl Colour {
    fn bytes(self) -> usize {
        match self {
            Colour::Rgb => 3,
            Colour::Rgba => 4,
        }
    }

    fn png_type(self) -> u8 {
        match self {
            Colour::Rgb => 2,
            Colour::Rgba => 6,
        }
    }
}

/// `zlib.crc32`.
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for part in parts {
        for b in *part {
            c ^= u32::from(*b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
    }
    !c
}

fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
    let len = u32::try_from(data.len()).expect("a chunk fits a u32");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
}

/// A PNG of `px`: filter 0 on every row, one `IDAT` at zlib's level 6.
pub fn png(w: usize, h: usize, px: &[u8], colour: Colour) -> Vec<u8> {
    let n = colour.bytes();
    let mut raw = Vec::with_capacity((w * n + 1) * h);
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&px[y * w * n..(y + 1) * w * n]);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&u32::try_from(w).expect("a width").to_be_bytes());
    ihdr.extend_from_slice(&u32::try_from(h).expect("a height").to_be_bytes());
    ihdr.extend_from_slice(&[8, colour.png_type(), 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &deflate::compress(&raw));
    chunk(&mut out, b"IEND", b"");
    out
}

/// `int(v)` kept to a byte: `max(0, min(255, int(v)))`.
fn clamped(v: f64) -> u8 {
    u8::try_from((v.trunc() as i64).clamp(0, 255)).expect("clamped to a byte")
}

/// A colour component the arithmetic keeps in a byte: `int(v)`.
fn byte(v: f64) -> u8 {
    u8::try_from(v.trunc() as i64).expect("in a byte")
}

/// 1: photo-like, a smooth colour field plus per-pixel noise, as a camera's.
pub fn photo(rnd: &mut Draws) -> Vec<u8> {
    let (w, h) = (1200usize, 1600usize);
    let mut px = vec![0u8; w * h * 3];
    for y in 0..h {
        let yf = y as f64;
        for x in 0..w {
            let xf = x as f64;
            let r = 128.0 + 100.0 * (xf / 97.0).sin() * (yf / 131.0).cos();
            let g = 128.0 + 90.0 * ((xf + yf) / 173.0).sin();
            let b = 128.0 + 110.0 * (xf / 61.0 - yf / 89.0).cos();
            let i = (y * w + x) * 3;
            px[i] = clamped(r + rnd.gauss(0.0, 12.0));
            px[i + 1] = clamped(g + rnd.gauss(0.0, 12.0));
            px[i + 2] = clamped(b + rnd.gauss(0.0, 12.0));
        }
    }
    px
}

/// 2: flat, four colour blocks, the photo's size.
pub fn flat() -> Vec<u8> {
    let (w, h) = (1200usize, 1600usize);
    let blocks: [[u8; 3]; 4] = [[200, 40, 40], [40, 160, 60], [40, 60, 200], [230, 200, 40]];
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let index = usize::from(x >= w / 2) + 2 * usize::from(y >= h / 2);
            px.extend_from_slice(&blocks[index]);
        }
    }
    px
}

/// 3: small, a 64x64 gradient.
pub fn small() -> Vec<u8> {
    let mut px = Vec::with_capacity(64 * 64 * 3);
    for y in 0..64u8 {
        for x in 0..64u8 {
            px.extend_from_slice(&[x * 4, y * 4, 128]);
        }
    }
    px
}

/// 4: screenshot-like, a white page with dark 'text' strokes and a coloured
/// band, as a capture of a window.
pub fn screenshot(rnd: &mut Draws) -> Vec<u8> {
    let (w, h) = (1600usize, 1000usize);
    let mut px = vec![0xffu8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 3;
            if y < 60 {
                px[i..i + 3].copy_from_slice(&[236, 236, 240]);
            } else if y % 40 < 18 && 40 < x && x < 1500 && rnd.random() < 0.55 {
                let v = u8::try_from(rnd.randint(20, 120)).expect("in a byte");
                px[i..i + 3].copy_from_slice(&[v, v, v]);
            }
        }
    }
    px
}

/// 5: alpha, RGBA with a transparent background and a soft disc.
pub fn alpha() -> Vec<u8> {
    let (w, h) = (800usize, 800usize);
    let mut px = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f64 - 400.0, y as f64 - 400.0);
            // `math.hypot`: the root of an exact integer, correctly rounded.
            let d = (dx * dx + dy * dy).sqrt();
            let mut a = if d > 250.0 {
                clamped(255.0 * (1.0 - (d - 250.0) / 60.0))
            } else {
                255
            };
            if d > 310.0 {
                a = 0;
            }
            let (wf, hf) = (w as f64, h as f64);
            let r = byte((x * 255) as f64 / wf);
            let g = byte((y * 255) as f64 / hf);
            px.extend_from_slice(&[r, g, 180, a]);
        }
    }
    px
}

/// A hand-written PDF of one page, Helvetica, a base-14 font, not embedded.
pub fn pdf(content: &[u8]) -> Vec<u8> {
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content);
    stream.extend_from_slice(b"\nendstream");
    let objs: [&[u8]; 5] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        &stream,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::with_capacity(objs.len());
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let x = out.len();
    let size = objs.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n").as_bytes(),
    );
    out
}

/// 6: the content stream of `text.pdf`, sixty lines of fourteen words.
pub fn text_content(rnd: &mut Draws) -> Vec<u8> {
    const WORDS: [&str; 19] = [
        "lorem",
        "ipsum",
        "dolor",
        "sit",
        "amet",
        "consectetur",
        "adipiscing",
        "elit",
        "sed",
        "do",
        "eiusmod",
        "tempor",
        "incididunt",
        "ut",
        "labore",
        "et",
        "dolore",
        "magna",
        "aliqua",
    ];
    let mut lines = Vec::with_capacity(60);
    for _ in 0..60 {
        let words: Vec<&str> = (0..14).map(|_| *rnd.choice(&WORDS)).collect();
        lines.push(format!("({}) '", words.join(" ")));
    }
    format!("BT /F1 10 Tf 12 TL 50 760 Td {} ET", lines.join(" ")).into_bytes()
}

/// Every file but the JPEG, in the order they are drawn, and the rows the
/// JPEG is made from.
pub struct Files {
    /// The photo's pixels, for `sips`.
    pub photo: Vec<u8>,
    /// Each other file's name and bytes.
    pub others: Vec<(&'static str, Vec<u8>)>,
}

/// The files, from the seed.
pub fn files() -> Files {
    let mut rnd = Draws::new(SEED);
    let photo = photo(&mut rnd);
    let mut others = Vec::with_capacity(6);
    others.push(("flat.png", png(1200, 1600, &flat(), Colour::Rgb)));
    others.push(("small.png", png(64, 64, &small(), Colour::Rgb)));
    let shot = screenshot(&mut rnd);
    others.push(("screenshot.png", png(1600, 1000, &shot, Colour::Rgb)));
    others.push(("alpha.png", png(800, 800, &alpha(), Colour::Rgba)));
    let content = text_content(&mut rnd);
    others.push(("text.pdf", pdf(&content)));
    others.push(("sparse.pdf", pdf(b"BT /F1 24 Tf 72 700 Td (A title) Tj ET")));
    Files { photo, others }
}

/// The example.
pub fn run(args: &[OsString]) -> i32 {
    let [dir] = args else {
        return super::died(DOC);
    };
    match write_all(dir) {
        Ok(()) => 0,
        Err(e) => super::died(&e),
    }
}

/// `dir` and everything in it.
fn write_all(dir: &OsStr) -> Result<(), String> {
    super::pty::coerce_locale();
    std::fs::create_dir_all(dir).map_err(|e| format!("makedirs: {e}"))?;
    let at = |name: &str| std::path::Path::new(dir).join(name);
    let Files { photo, others } = files();
    let photo_png = at("photo.png");
    std::fs::write(&photo_png, png(1200, 1600, &photo, Colour::Rgb))
        .map_err(|e| format!("open: {e}"))?;
    let sips = Command::new("sips")
        .args(["-s", "format", "jpeg", "-s", "formatOptions", "85"])
        .arg(&photo_png)
        .arg("--out")
        .arg(at("photo.jpg"))
        .output()
        .map_err(|e| format!("sips: {e}"))?;
    if !sips.status.success() {
        return Err(format!("sips: {}", sips.status));
    }
    std::fs::remove_file(&photo_png).map_err(|e| format!("remove: {e}"))?;
    for (name, bytes) in &others {
        std::fs::write(at(name), bytes).map_err(|e| format!("open: {e}"))?;
    }
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("listdir: {e}"))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    println!("{}", super::py::repr_list(&names));
    Ok(())
}
