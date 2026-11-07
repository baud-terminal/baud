//! Compares frames pixel for pixel: the `pixels` bench's PAMs and a
//! compositor's PNG screenshots (`scripts/linux-pixels.sh` drives it), and
//! reads what else that script asks of a screenshot and of sway.
//!
//! The usage below is [`USAGE`], what an unknown command prints.
//!
//! Alpha is compared too: a PAM's fourth channel against a PNG's, where a PNG
//! with no alpha reads as opaque.
//!
//! The chunk walk and the filters are here rather than in
//! `baud_domain::png` because that decoder refuses what this one reads: a
//! chunk whose CRC is wrong and a filter byte above 4, which here means none.
//! A frame compared is any bytes a compositor wrote, and a refusal would hide
//! the comparison. The stream goes through `baud_domain::zlib`.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::Path;

use baud_domain::zlib::{Inflate, Inflated, ZlibError};

use super::Done;
use crate::capture::deflate;
use crate::key_latency::py;

/// What an unknown command prints, with a line after it.
pub const USAGE: &str = "Compare frames pixel for pixel: the `pixels` bench's PAMs and a
compositor's PNG screenshots (scripts/linux-pixels.sh drives it).

Built by the host's cargo and the container's alike.

  linux-pixels over-black <in.pam> <out.pam>
      the frame as a compositor shows it over a black output: each channel
      premultiplied, rounded as both presenters round, (c a + 127) / 255
  linux-pixels crop <in.png|pam> <x> <y> <w> <h> <out.pam>
  linux-pixels png <in.pam|png> <out.png> [scale]
  linux-pixels diff <a> <b> [<diff.png>]
      prints the count of pixels that differ, the largest channel
      difference, the per-channel histogram of differences and the box
      that holds them; writes the differing pixels in white over a dark
      copy of <a>, scaled up four times, when a path is given. Exits 1 when
      any pixel differs or the sizes differ, 0 when the two are identical.
  linux-pixels control <in.png|pam>
      yes when every pixel is the control's one value, (130, 26, 74, 255):
      rgb(185, 37, 105) at alpha 179, premultiplied over black and rounded
  linux-pixels png-size <in.png>
      a PNG's width and height, as WxH
  linux-pixels window-rect < tree.json
      the rect of each node of `swaymsg -t get_tree` that is the app's
      window, as X,Y WxH

Alpha is compared too: a PAM's fourth channel against a PNG's, where a PNG
with no alpha reads as opaque.
";

/// The app's id on sway.
const APP_ID: &str = "org.baud.terminal";

/// A frame: its size as the file says it and four bytes a pixel, though a
/// stream short by less than a pixel leaves it short.
#[derive(Debug, PartialEq, Eq)]
struct Image {
    w: i128,
    h: i128,
    px: Vec<u8>,
}

type Res<T> = Result<T, String>;

fn read_file(path: &Path) -> Res<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A slice as `px[start:stop]` takes one: from the end when negative, and
/// clamped.
fn slice(px: &[u8], start: i128, stop: i128) -> &[u8] {
    let len = i128::try_from(px.len()).unwrap_or(i128::MAX);
    let at = |i: i128| {
        let i = if i < 0 { i + len } else { i };
        usize::try_from(i.clamp(0, len)).unwrap_or(0)
    };
    let (a, b) = (at(start), at(stop));
    if a < b { &px[a..b] } else { &[] }
}

fn read_pam(path: &Path) -> Res<Image> {
    let data = read_file(path)?;
    const END: &[u8] = b"ENDHDR\n";
    let end = data
        .windows(END.len())
        .position(|w| w == END)
        .map(|at| at + END.len())
        .ok_or_else(|| format!("{}: no ENDHDR", path.display()))?;
    let head = std::str::from_utf8(&data[..end]).map_err(|e| format!("{}: {e}", path.display()))?;
    let lines = py::splitlines(head);
    let mut w = None;
    let mut h = None;
    let mut depth = None;
    for line in lines.iter().take(lines.len().saturating_sub(1)).skip(1) {
        if let Some((key, value)) = line.split_once(' ') {
            match key {
                "WIDTH" => w = Some(value),
                "HEIGHT" => h = Some(value),
                "DEPTH" => depth = Some(value),
                _ => {}
            }
        }
    }
    let field = |name: &str, v: Option<&str>| {
        v.and_then(py::int)
            .ok_or_else(|| format!("{}: no {name}", path.display()))
    };
    let w = field("WIDTH", w)?;
    let h = field("HEIGHT", h)?;
    let depth = field("DEPTH", depth)?;
    let body = &data[end..];
    let want = w.checked_mul(h).and_then(|n| n.checked_mul(depth));
    if want != i128::try_from(body.len()).ok() {
        return Err(format!(
            "{}: {} bytes for {w}x{h}x{depth}",
            path.display(),
            body.len()
        ));
    }
    let px = match depth {
        4 => body.to_vec(),
        3 => body
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 0xff])
            .collect(),
        other => return Err(format!("{}: depth {other}", path.display())),
    };
    Ok(Image { w, h, px })
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i32::from(a), i32::from(b), i32::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// `zlib.decompress`: a stream of any length, and what comes after it left
/// alone. [`Inflate`] wants a buffer the stream's exact length, and says when
/// it was not.
fn inflate(stream: &[u8], guess: usize) -> Res<Vec<u8>> {
    let mut size = guess;
    loop {
        let mut raw = vec![0u8; size];
        let mut z = Inflate::NEW;
        match z.feed(stream, &mut raw) {
            Ok(Inflated::Done) => return Ok(raw),
            Err(ZlibError::Short) => size = z.written(),
            Err(ZlibError::Overrun) => size = size.max(1 << 16) * 2,
            Ok(Inflated::More) => return Err("the stream is truncated".to_string()),
            Err(e) => return Err(format!("the stream: {e:?}")),
        }
    }
}

/// Big-endian `u32` at `at`, the file being long enough or not.
fn be32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_png(path: &Path) -> Res<Image> {
    let data = read_file(path)?;
    let name = path.display();
    if data.get(..8) != Some(b"\x89PNG\r\n\x1a\n") {
        return Err(format!("{name}: not a PNG"));
    }
    let mut at = 8usize;
    let mut idat = Vec::new();
    let mut header = None;
    while at < data.len() {
        let n = be32(&data, at).ok_or_else(|| format!("{name}: a chunk with no length"))?;
        let n = usize::try_from(n).map_err(|e| format!("{name}: {e}"))?;
        let kind = data.get(at + 4..at + 8).unwrap_or(&[]);
        let start = (at + 8).min(data.len());
        let body = &data[start..start.saturating_add(n).min(data.len())];
        at = at.saturating_add(n).saturating_add(12);
        if kind == b"IHDR" {
            if body.len() != 13 {
                return Err(format!("{name}: an IHDR of {} bytes", body.len()));
            }
            let (w, h) = (be32(body, 0), be32(body, 4));
            let (depth, ctype, interlace) = (body[8], body[9], body[12]);
            if depth != 8 || !(ctype == 2 || ctype == 6) || interlace != 0 {
                return Err(format!("{name}: depth {depth}, colour type {ctype}"));
            }
            header = Some((w.unwrap_or(0), h.unwrap_or(0), ctype));
        } else if kind == b"IDAT" {
            idat.extend_from_slice(body);
        }
    }
    // Deflate shrinks by 1032 at the most, so a header that lies about its
    // size costs no more than the stream can fill.
    let bound = idat.len().saturating_mul(1032).saturating_add(1024);
    let guess = header.map_or(0, |(w, h, c)| {
        let bpp = if c == 6 { 4u64 } else { 3 };
        let row = u64::from(w) * bpp + 1;
        usize::try_from(row.saturating_mul(u64::from(h))).unwrap_or(usize::MAX)
    });
    let raw = inflate(&idat, guess.min(bound))?;
    let (w, h, ctype) = header.ok_or_else(|| format!("{name}: no IHDR"))?;
    let wide = usize::try_from(w).map_err(|e| format!("{name}: {e}"))?;
    let high = usize::try_from(h).map_err(|e| format!("{name}: {e}"))?;
    let bpp = if ctype == 6 { 4 } else { 3 };
    let stride = wide * bpp;
    // The last row's filter byte is the least a frame of this height needs.
    let needed = (high.saturating_sub(1) as u128) * (stride as u128 + 1) + 1;
    if high > 0 && (raw.len() as u128) < needed {
        return Err(format!("{name}: the stream ends before its last row"));
    }
    let size = wide
        .checked_mul(high)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| format!("{name}: {w}x{h}"))?;
    let mut out = vec![0u8; size];
    let mut prev = vec![0u8; stride];
    for y in 0..high {
        let start = y * (stride + 1);
        let f = *raw
            .get(start)
            .ok_or_else(|| format!("{name}: the stream ends at row {y}"))?;
        let end = (start + 1 + stride).min(raw.len());
        let mut line = raw[start + 1..end].to_vec();
        let got = line.len();
        let reads = |i: usize, line: &[u8]| -> Res<u8> {
            line.get(i)
                .copied()
                .ok_or_else(|| format!("{name}: the stream ends in row {y}"))
        };
        for i in 0..stride {
            let a = if i >= bpp { reads(i - bpp, &line)? } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let add = match f {
                1 => a,
                2 => b,
                3 => a / 2 + b / 2 + (a & b & 1),
                4 => paeth(a, b, c),
                _ => continue,
            };
            let v = line
                .get_mut(i)
                .ok_or_else(|| format!("{name}: the stream ends in row {y}"))?;
            *v = v.wrapping_add(add);
        }
        let row_at = y * wide * 4;
        for x in 0..wide {
            let px = &line[(x * bpp).min(got)..(x * bpp + bpp).min(got)];
            let to = &mut out[row_at + x * 4..row_at + x * 4 + 4];
            to[..px.len()].copy_from_slice(px);
            if bpp == 3 && px.len() == 3 {
                to[3] = 0xff;
            }
            if px.len() < bpp {
                // A row short by less than a pixel, its filter 0, is the
                // stream's last: the pixel it cuts shortens the frame, an
                // RGB row's by its opaque byte too.
                let kept = if bpp == 3 {
                    to[px.len()] = 0xff;
                    px.len() + 1
                } else {
                    px.len()
                };
                out.truncate(row_at + x * 4 + kept);
                break;
            }
        }
        prev = line;
        prev.resize(stride, 0);
    }
    Ok(Image {
        w: i128::from(w),
        h: i128::from(h),
        px: out,
    })
}

fn read(path: &Path) -> Res<Image> {
    if path.to_string_lossy().ends_with(".png") {
        read_png(path)
    } else {
        read_pam(path)
    }
}

fn write_pam(path: &Path, w: i128, h: i128, px: &[u8]) -> Res<()> {
    let mut out =
        format!("P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n")
            .into_bytes();
    out.extend_from_slice(px);
    fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// The CRC-32 of PNG's chunks, bit by bit: the table would save nothing on
/// a frame's one IDAT against the deflate before it.
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = !0u32;
    for part in parts {
        for byte in *part {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) -> Res<()> {
    let n = u32::try_from(body.len()).map_err(|e| format!("a chunk's size: {e}"))?;
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&crc32(&[kind, body]).to_be_bytes());
    Ok(())
}

fn write_png(path: &Path, w: i128, h: i128, px: &[u8], scale: i128) -> Res<()> {
    let tall = h.checked_mul(scale).ok_or("a height past this tool")?;
    let wide = w.checked_mul(scale).ok_or("a width past this tool")?;
    let mut rows = Vec::new();
    for y in 0..tall {
        let sy = floor_div(y, scale);
        rows.push(0u8);
        for x in 0..w {
            let at = (sy * w + x) * 4;
            let p = slice(px, at, at + 3);
            for _ in 0..scale {
                rows.extend_from_slice(p);
            }
        }
    }
    // The file exists from here, whatever becomes of its contents.
    fs::write(path, []).map_err(|e| format!("{}: {e}", path.display()))?;
    let out_of_range = |v: i128| format!("{v} does not fit an IHDR");
    let packed_w = u32::try_from(wide).map_err(|_| out_of_range(wide))?;
    let packed_h = u32::try_from(tall).map_err(|_| out_of_range(tall))?;
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&packed_w.to_be_bytes());
    ihdr.extend_from_slice(&packed_h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr)?;
    chunk(
        &mut out,
        b"IDAT",
        &deflate::compress_at(&rows, deflate::Level::Nine),
    )?;
    chunk(&mut out, b"IEND", b"")?;
    fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// A channel at an alpha, rounded as both presenters round.
fn premultiplied(c: u8, a: u8) -> u8 {
    let v = (u32::from(c) * u32::from(a) + 127) / 255;
    u8::try_from(v).unwrap_or(u8::MAX)
}

/// Each channel premultiplied and rounded, alpha opaque.
fn over_black(px: &[u8]) -> Res<Vec<u8>> {
    if !px.len().is_multiple_of(4) {
        return Err("a frame cut inside a pixel".to_string());
    }
    let mut out = px.to_vec();
    for (to, from) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(px.as_chunks::<4>().0)
    {
        for k in 0..3 {
            to[k] = premultiplied(from[k], from[3]);
        }
        to[3] = 255;
    }
    Ok(out)
}

fn crop(image: &Image, x0: i128, y0: i128, cw: i128, ch: i128) -> Res<Vec<u8>> {
    if x0.saturating_add(cw) > image.w || y0.saturating_add(ch) > image.h {
        return Err(format!(
            "{}x{} does not hold {cw}x{ch} at {x0},{y0}",
            image.w, image.h
        ));
    }
    let mut out = Vec::new();
    for y in y0..y0 + ch {
        let row = y * image.w + x0;
        out.extend_from_slice(slice(&image.px, row * 4, (row + cw) * 4));
    }
    Ok(out)
}

/// `a // b`.
fn floor_div(a: i128, b: i128) -> i128 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// The comparison: what it prints and its code.
fn diff(a: &Path, b: &Path, out_png: Option<&Path>, out: &mut String) -> Res<i32> {
    let (first, second) = (read(a)?, read(b)?);
    let (wa, ha) = (first.w, first.h);
    if (wa, ha) != (second.w, second.h) {
        *out += &format!(
            "sizes differ: {wa}x{ha} against {}x{}\n",
            second.w, second.h
        );
        return Ok(1);
    }
    let (pa, pb) = (&first.px, &second.px);
    if !pa.len().is_multiple_of(4) || pb.len() < pa.len() {
        return Err("a frame cut inside a pixel".to_string());
    }
    let mut n = 0i128;
    let mut worst = 0u8;
    // In the order each difference first appears: ties keep it when sorted.
    let mut hist: Vec<(u8, usize)> = Vec::new();
    let mut slots = [usize::MAX; 256];
    let (mut left, mut top, mut right, mut bottom) = (wa, ha, -1i128, -1i128);
    let mut mark = vec![0u8; usize::try_from(wa * ha * 4).map_err(|e| e.to_string())?];
    for (i, (p, q)) in pa
        .as_chunks::<4>()
        .0
        .iter()
        .zip(pb.as_chunks::<4>().0)
        .enumerate()
    {
        let d = (0..4).map(|k| p[k].abs_diff(q[k])).max().unwrap_or(0);
        let to = &mut mark[i * 4..i * 4 + 4];
        if d != 0 {
            let index = i128::try_from(i).map_err(|e| e.to_string())?;
            let y = floor_div(index, wa);
            let x = index - y * wa;
            n += 1;
            worst = worst.max(d);
            let slot = &mut slots[usize::from(d)];
            if *slot == usize::MAX {
                *slot = hist.len();
                hist.push((d, 0));
            }
            hist[*slot].1 += 1;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
            to.fill(0xff);
        } else {
            for k in 0..3 {
                to[k] = p[k] / 4;
            }
            to[3] = 255;
        }
    }
    let total = wa * ha;
    *out += &format!("{n} of {total} pixels differ, largest channel difference {worst}\n");
    if n != 0 {
        hist.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let top8: Vec<String> = hist
            .iter()
            .take(8)
            .map(|(d, c)| format!("{d}: {c}"))
            .collect();
        *out += &format!("  by difference: {}\n", top8.join(", "));
        *out += &format!("  within x {left}..{right}, y {top}..{bottom}\n");
    }
    if let Some(path) = out_png {
        write_png(path, wa, ha, &mark, 4)?;
    }
    Ok(i32::from(n != 0))
}

/// The control's one value, computed here and not read from either arm:
/// rgb(185, 37, 105) at the dark dim's alpha, round(0.70 * 255) = 179,
/// premultiplied over black and rounded. Each channel's value is a rounding
/// up, so a truncation reads (129, 25, 73).
fn control_value() -> [u8; 4] {
    let channel = |c| premultiplied(c, 179);
    [channel(185), channel(37), channel(105), 255]
}

/// `yes` when every pixel is the control's value.
fn control(image: &Image) -> &'static str {
    let want = control_value();
    let size = image.w.checked_mul(image.h).map(|n| n * 4);
    let holds = size == i128::try_from(image.px.len()).ok()
        && image.px.as_chunks::<4>().0.iter().all(|p| *p == want);
    if holds { "yes" } else { "no" }
}

/// A `rect` field as `print` writes the JSON value.
fn rect_field(rect: &serde_json::Value, key: &str) -> Res<String> {
    use serde_json::Value;
    match rect.get(key) {
        Some(Value::Number(n)) => Ok(n.to_string()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Bool(true)) => Ok("True".to_string()),
        Some(Value::Bool(false)) => Ok("False".to_string()),
        Some(Value::Null) => Ok("None".to_string()),
        other => Err(format!("rect[{key}] is {other:?}")),
    }
}

/// The app's windows in `node` and under it, its `nodes` before its
/// `floating_nodes`.
fn walk(node: &serde_json::Value, out: &mut String) -> Res<()> {
    let serde_json::Value::Object(fields) = node else {
        return Err(format!("a node that is not an object: {node}"));
    };
    if fields.get("app_id").and_then(|v| v.as_str()) == Some(APP_ID) {
        let rect = fields.get("rect").ok_or("the window has no rect")?;
        let (x, y) = (rect_field(rect, "x")?, rect_field(rect, "y")?);
        let (w, h) = (rect_field(rect, "width")?, rect_field(rect, "height")?);
        *out += &format!("{x},{y} {w}x{h}\n");
    }
    let empty = Vec::new();
    let kids = |key: &str| -> Res<&Vec<serde_json::Value>> {
        match fields.get(key) {
            None => Ok(&empty),
            Some(serde_json::Value::Array(a)) => Ok(a),
            Some(other) => Err(format!("{key} is {other}")),
        }
    };
    let (nodes, floating) = (kids("nodes")?, kids("floating_nodes")?);
    for kid in nodes.iter().chain(floating) {
        walk(kid, out)?;
    }
    Ok(())
}

/// A command-line argument as `int` reads it.
fn int_arg(a: &OsString) -> Res<i128> {
    let s = a.to_string_lossy();
    py::int(&s).ok_or_else(|| format!("invalid literal for int(): {}", py::repr_str(&s)))
}

/// Argument `n`, or the failure its absence is.
fn arg(args: &[OsString], n: usize) -> Res<&Path> {
    args.get(n)
        .map(Path::new)
        .ok_or_else(|| format!("argument {n} is missing: try `linux-pixels help`"))
}

fn dispatch(args: &[OsString], stdin: &mut dyn Read, out: &mut String) -> Res<i32> {
    let cmd = args.first().ok_or("no command: try `linux-pixels help`")?;
    match cmd.to_string_lossy().as_ref() {
        "over-black" => {
            let image = read(arg(args, 1)?)?;
            let px = over_black(&image.px)?;
            write_pam(arg(args, 2)?, image.w, image.h, &px)?;
        }
        "crop" => {
            let image = read(arg(args, 1)?)?;
            let nums = args.get(2..6).ok_or("crop wants x y w h")?;
            let [x0, y0, cw, ch] = [
                int_arg(&nums[0])?,
                int_arg(&nums[1])?,
                int_arg(&nums[2])?,
                int_arg(&nums[3])?,
            ];
            let to = arg(args, 6)?;
            let px = crop(&image, x0, y0, cw, ch)?;
            write_pam(to, cw, ch, &px)?;
        }
        "png" => {
            let image = read(arg(args, 1)?)?;
            let to = arg(args, 2)?;
            let scale = args.get(3).map_or(Ok(1), int_arg)?;
            write_png(to, image.w, image.h, &image.px, scale)?;
        }
        "diff" => {
            return diff(
                arg(args, 1)?,
                arg(args, 2)?,
                args.get(3).filter(|p| !p.is_empty()).map(Path::new),
                out,
            );
        }
        "control" => {
            let image = read(arg(args, 1)?)?;
            *out += control(&image);
            out.push('\n');
        }
        "png-size" => {
            let path = arg(args, 1)?;
            let data = read_file(path)?;
            let (Some(w), Some(h)) = (be32(&data, 16), be32(&data, 20)) else {
                return Err(format!("{}: shorter than an IHDR", path.display()));
            };
            *out += &format!("{w}x{h}\n");
        }
        "window-rect" => {
            let mut text = Vec::new();
            stdin
                .read_to_end(&mut text)
                .map_err(|e| format!("stdin: {e}"))?;
            let tree: serde_json::Value =
                serde_json::from_slice(&text).map_err(|e| format!("stdin: {e}"))?;
            walk(&tree, out)?;
        }
        _ => {
            *out += USAGE;
            out.push('\n');
            return Ok(2);
        }
    }
    Ok(0)
}

/// The whole tool.
pub fn run(args: &[OsString], mut stdin: impl Read) -> Done {
    let mut out = String::new();
    match dispatch(args, &mut stdin, &mut out) {
        Ok(code) => Done::with(out, code),
        Err(why) => Done::failed(out, why),
    }
}
