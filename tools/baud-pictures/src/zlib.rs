//! **zlib, declared as the extern lines it is**: the deflate calls
//! Pillow's PNG encoder makes, the inflate calls that read the release
//! archive and the PNGs back, and `crc32`, against the zlib
//! `tools/shell.nix` pins.
//!
//! **The zlib is the one Pillow 12.3.0 carried: 1.3.2.** Pillow's
//! `_imaging` in the pinned nixpkgs links no `libz`; it holds zlib 1.3.2
//! itself (its text says `deflate 1.3.2 Copyright 1995-2026 Jean-loup
//! Gailly and Mark Adler`). `tools/shell.nix` gives the same revision's
//! `zlib` 1.3.2, and [`version`] is refused unless it is [`ZLIB`]. No
//! `-sys` crate, and not `miniz_oxide`: another deflater writes other
//! bytes. The `abi` test compiles a C probe against `zlib.h` and holds
//! [`ZStream`]'s size and every field's offset to it.

#![allow(unsafe_code)]

use core::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
use std::ffi::CStr;

/// The zlib the pictures were made with, `zlib.ZLIB_RUNTIME_VERSION` in
/// the pinned Python.
pub const ZLIB: &str = "1.3.2";

/// `z_stream`, as `zlib.h` lays it out.
#[repr(C)]
pub struct ZStream {
    pub next_in: *const u8,
    pub avail_in: c_uint,
    pub total_in: c_ulong,
    pub next_out: *mut u8,
    pub avail_out: c_uint,
    pub total_out: c_ulong,
    pub msg: *const c_char,
    pub state: *mut c_void,
    pub zalloc: *const c_void,
    pub zfree: *const c_void,
    pub opaque: *mut c_void,
    pub data_type: c_int,
    pub adler: c_ulong,
    pub reserved: c_ulong,
}

pub const Z_NO_FLUSH: c_int = 0;
pub const Z_FINISH: c_int = 4;
pub const Z_OK: c_int = 0;
pub const Z_STREAM_END: c_int = 1;
pub const Z_DEFLATED: c_int = 8;
pub const Z_FILTERED: c_int = 1;
pub const Z_BEST_COMPRESSION: c_int = 9;

#[link(name = "z")]
unsafe extern "C" {
    fn zlibVersion() -> *const c_char;
    fn deflateInit2_(
        strm: *mut ZStream,
        level: c_int,
        method: c_int,
        window_bits: c_int,
        mem_level: c_int,
        strategy: c_int,
        version: *const c_char,
        stream_size: c_int,
    ) -> c_int;
    fn deflate(strm: *mut ZStream, flush: c_int) -> c_int;
    fn deflateEnd(strm: *mut ZStream) -> c_int;
    fn inflateInit2_(
        strm: *mut ZStream,
        window_bits: c_int,
        version: *const c_char,
        stream_size: c_int,
    ) -> c_int;
    fn inflate(strm: *mut ZStream, flush: c_int) -> c_int;
    fn inflateEnd(strm: *mut ZStream) -> c_int;
    fn crc32(crc: c_ulong, buf: *const u8, len: c_uint) -> c_ulong;
}

/// The linked zlib's `zlibVersion()`.
#[must_use]
pub fn version() -> String {
    // SAFETY: zlibVersion returns a static NUL-terminated string.
    unsafe { CStr::from_ptr(zlibVersion()) }
        .to_string_lossy()
        .into_owned()
}

/// `ZLIB_VERSION`, which every init call passes.
const HEADER_VERSION: &[u8] = b"1.3.2\0";

fn stream_size() -> c_int {
    c_int::try_from(size_of::<ZStream>()).expect("z_stream's size fits an int")
}

fn len(n: usize) -> c_uint {
    c_uint::try_from(n).expect("a buffer zlib can take")
}

fn empty() -> ZStream {
    ZStream {
        next_in: core::ptr::null(),
        avail_in: 0,
        total_in: 0,
        next_out: core::ptr::null_mut(),
        avail_out: 0,
        total_out: 0,
        msg: core::ptr::null(),
        state: core::ptr::null_mut(),
        zalloc: core::ptr::null(),
        zfree: core::ptr::null(),
        opaque: core::ptr::null_mut(),
        data_type: 0,
        adler: 0,
        reserved: 0,
    }
}

/// A deflate stream, `deflateInit2(level, Z_DEFLATED, 15, 9, strategy)`
/// as `ZipEncode.c` opens it. Boxed, because zlib keeps a pointer to it.
pub struct Deflate(Box<ZStream>);

impl Deflate {
    /// # Panics
    /// When zlib refuses the parameters.
    #[must_use]
    pub fn new(level: c_int, strategy: c_int) -> Deflate {
        let mut z = Box::new(empty());
        // SAFETY: z is a zeroed z_stream; the version and size are zlib.h's.
        let err = unsafe {
            deflateInit2_(
                &raw mut *z,
                level,
                Z_DEFLATED,
                15,
                9,
                strategy,
                HEADER_VERSION.as_ptr().cast(),
                stream_size(),
            )
        };
        assert!(err >= 0, "deflateInit2 refused: {err}");
        Deflate(z)
    }

    /// Point the output at `out`, `out.len()` bytes free.
    pub fn set_out(&mut self, out: &mut [u8]) {
        self.0.next_out = out.as_mut_ptr();
        self.0.avail_out = len(out.len());
    }

    /// Point the input at `input`. It must outlive every `deflate` call
    /// that reads it.
    pub fn set_in(&mut self, input: &[u8]) {
        self.0.next_in = input.as_ptr();
        self.0.avail_in = len(input.len());
    }

    #[must_use]
    pub fn avail_in(&self) -> c_uint {
        self.0.avail_in
    }

    #[must_use]
    pub fn has_in(&self) -> bool {
        !self.0.next_in.is_null() && self.0.avail_in > 0
    }

    #[must_use]
    pub fn avail_out(&self) -> c_uint {
        self.0.avail_out
    }

    /// `deflate(&z_stream, flush)`.
    pub fn deflate(&mut self, flush: c_int) -> c_int {
        // SAFETY: the stream was opened by deflateInit2 and its buffers
        // are the caller's live slices.
        unsafe { deflate(&raw mut *self.0, flush) }
    }
}

impl Drop for Deflate {
    fn drop(&mut self) {
        // SAFETY: opened by deflateInit2; deflateEnd on an ended stream
        // returns an error and does nothing.
        unsafe { deflateEnd(&raw mut *self.0) };
    }
}

/// Inflate `data` (zlib-wrapped for `window_bits` 15, raw for -15),
/// expecting `size` bytes out when given.
///
/// # Errors
/// When the stream is not one zlib can finish.
pub fn inflate_all(
    data: &[u8],
    window_bits: c_int,
    size: Option<usize>,
) -> Result<Vec<u8>, String> {
    let mut z = Box::new(empty());
    // SAFETY: as above.
    let err = unsafe {
        inflateInit2_(
            &raw mut *z,
            window_bits,
            HEADER_VERSION.as_ptr().cast(),
            stream_size(),
        )
    };
    if err != Z_OK {
        return Err(format!("inflateInit2 refused: {err}"));
    }
    z.next_in = data.as_ptr();
    z.avail_in = len(data.len());
    let mut out = vec![0u8; size.unwrap_or(data.len() * 4 + 64)];
    let mut done = 0usize;
    let result = loop {
        if done == out.len() {
            out.resize(out.len() * 2, 0);
        }
        z.next_out = out[done..].as_mut_ptr();
        z.avail_out = len(out.len() - done);
        let before = z.avail_out;
        // SAFETY: the stream's buffers are the live `data` and `out`.
        let err = unsafe { inflate(&raw mut *z, Z_NO_FLUSH) };
        done += (before - z.avail_out) as usize;
        if err == Z_STREAM_END {
            break Ok(());
        }
        if err != Z_OK {
            break Err(format!("inflate stopped: {err}"));
        }
    };
    // SAFETY: opened by inflateInit2.
    unsafe { inflateEnd(&raw mut *z) };
    result?;
    out.truncate(done);
    if size.is_some_and(|n| n != done) {
        return Err(format!("inflated {done} bytes, not {}", size.unwrap_or(0)));
    }
    Ok(out)
}

/// `crc32(crc, buf)`.
#[must_use]
pub fn crc(crc: u32, buf: &[u8]) -> u32 {
    // SAFETY: buf is a live slice of the length given.
    let c = unsafe { crc32(c_ulong::from(crc), buf.as_ptr(), len(buf.len())) };
    u32::try_from(c).expect("a CRC-32 is 32 bits")
}

#[cfg(test)]
mod abi {
    use super::ZStream;
    use std::collections::HashMap;
    use std::process::Command;

    const PROBE: &str = r#"
#include <stddef.h>
#include <stdio.h>
#include <zlib.h>
#define AT(f) printf("z_stream." #f "=%lld\n", (long long)offsetof(z_stream, f))
#define VALUE(v) printf(#v "=%lld\n", (long long)(v))
int main(void) {
    printf("z_stream.size=%lld\n", (long long)sizeof(z_stream));
    AT(next_in); AT(avail_in); AT(total_in); AT(next_out); AT(avail_out);
    AT(total_out); AT(msg); AT(state); AT(zalloc); AT(zfree); AT(opaque);
    AT(data_type); AT(adler); AT(reserved);
    VALUE(Z_NO_FLUSH); VALUE(Z_FINISH); VALUE(Z_OK); VALUE(Z_STREAM_END);
    VALUE(Z_DEFLATED); VALUE(Z_FILTERED); VALUE(Z_BEST_COMPRESSION);
    printf("version=%s\n", ZLIB_VERSION);
    return 0;
}
"#;

    fn ask() -> HashMap<String, String> {
        let dir = std::env::temp_dir().join(format!("baud-pictures-abi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let (src, bin) = (dir.join("probe.c"), dir.join("probe"));
        std::fs::write(&src, PROBE).expect("write the probe");
        let flags = Command::new("pkg-config")
            .args(["--cflags", "zlib"])
            .output()
            .expect("pkg-config, which tools/shell.nix provides");
        assert!(flags.status.success(), "pkg-config found no zlib");
        let flags = String::from_utf8(flags.stdout).expect("utf-8 flags");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let out = Command::new(&cc)
            .args(flags.split_whitespace())
            .arg("-o")
            .arg(&bin)
            .arg(&src)
            .output()
            .unwrap_or_else(|e| panic!("{cc} did not run: {e}"));
        assert!(
            out.status.success(),
            "the probe did not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run it");
        let _ = std::fs::remove_dir_all(&dir);
        let text = String::from_utf8(run.stdout).expect("utf-8 output");
        let map: HashMap<String, String> = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        // The control: a probe that printed nothing passes every check.
        assert_eq!(map.len(), 23, "the probe's lines: {text}");
        map
    }

    #[test]
    fn z_stream_is_laid_out_as_zlib_h_lays_it_out() {
        let c = ask();
        let at = |f: &str| {
            c[&format!("z_stream.{f}")]
                .parse::<usize>()
                .expect("an offset")
        };
        assert_eq!(at("size"), size_of::<ZStream>());
        for (f, off) in [
            ("next_in", core::mem::offset_of!(ZStream, next_in)),
            ("avail_in", core::mem::offset_of!(ZStream, avail_in)),
            ("total_in", core::mem::offset_of!(ZStream, total_in)),
            ("next_out", core::mem::offset_of!(ZStream, next_out)),
            ("avail_out", core::mem::offset_of!(ZStream, avail_out)),
            ("total_out", core::mem::offset_of!(ZStream, total_out)),
            ("msg", core::mem::offset_of!(ZStream, msg)),
            ("state", core::mem::offset_of!(ZStream, state)),
            ("zalloc", core::mem::offset_of!(ZStream, zalloc)),
            ("zfree", core::mem::offset_of!(ZStream, zfree)),
            ("opaque", core::mem::offset_of!(ZStream, opaque)),
            ("data_type", core::mem::offset_of!(ZStream, data_type)),
            ("adler", core::mem::offset_of!(ZStream, adler)),
            ("reserved", core::mem::offset_of!(ZStream, reserved)),
        ] {
            assert_eq!(at(f), off, "z_stream.{f}");
        }
        for (name, v) in [
            ("Z_NO_FLUSH", super::Z_NO_FLUSH),
            ("Z_FINISH", super::Z_FINISH),
            ("Z_OK", super::Z_OK),
            ("Z_STREAM_END", super::Z_STREAM_END),
            ("Z_DEFLATED", super::Z_DEFLATED),
            ("Z_FILTERED", super::Z_FILTERED),
            ("Z_BEST_COMPRESSION", super::Z_BEST_COMPRESSION),
        ] {
            assert_eq!(c[name], v.to_string(), "{name}");
        }
        assert_eq!(c["version"], super::ZLIB, "the header's ZLIB_VERSION");
        assert_eq!(
            format!("{}\0", c["version"]).as_bytes(),
            super::HEADER_VERSION,
            "the version every init passes"
        );
    }

    #[test]
    fn the_linked_zlib_is_the_one_pillow_carried() {
        assert_eq!(super::version(), super::ZLIB);
    }
}
