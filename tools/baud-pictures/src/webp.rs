//! ```text
//! Pillow 12.3.0, parts of it transcribed into Rust in this file.
//! Copyright © 1997-2011 by Secret Labs AB
//! Copyright © 1995-2011 by Fredrik Lundh and contributors
//! Copyright © 2010 by Jeffrey 'Alex' Clark and contributors
//!
//! Licence: MIT-CMU. Its text is in LICENSES/pillow-MIT-CMU.txt.
//!
//! Ported: _webp.c's WebPEncode_wrapper (encode) and import_frame_libwebp,
//! with WebPImagePlugin's _save and the WebPAnimDecoder it opens a file
//! with (decode). Changed: written in Rust over libwebp's extern "C", for
//! one RGBA image with no ICC, EXIF or XMP, and a decode that returns the
//! first frame's canvas. The rest of this file is baud's.
//! ```
//!
//! ```text
//! libwebp 1.6.0 (encode.h, decode.h, mux.h, demux.h, mux_types.h), its
//! interface transcribed into Rust in this file.
//! Copyright (c) 2010, Google Inc. All rights reserved.
//!
//! Licence: BSD-3-Clause. Its text is in LICENSES/libwebp-BSD-3-Clause.txt.
//!
//! Ported: the layouts of WebPConfig, WebPPicture, WebPMemoryWriter,
//! WebPData, WebPBitstreamFeatures and WebPAnimInfo, the declarations of
//! the functions called, the four ABI version numbers and the enum values
//! used. Changed: each is written as a Rust #[repr(C)] struct, an extern
//! "C" item or a const.
//! ```
//!
//! **WebP through libwebp's own `extern "C"`**, as Pillow 12.3.0's
//! `_webp.c` calls it: the still-image encoder `WebPEncode_wrapper` and
//! the decoder `WebPAnimDecoder` that `WebPImagePlugin` opens every file
//! with.
//!
//! **The libwebp is the one Pillow linked: 1.6.0**, the build nixpkgs
//! `7dd199b0` gives libtiff (`libwebp.override { tiffSupport = false; }`),
//! which is the one Pillow's `_webp` names by its store path; the
//! revision's plain `libwebp` is a second build of the same source.
//! `tools/shell.nix` gives that build, and [`pinned`] is refused unless
//! the encoder, mux and demux all report [`LIBWEBP`].
//!
//! **What `im.save(buf, "WEBP", quality=q, alpha_quality=100, method=6)`
//! does**, read at the tag: `_save` (not `_save_all`: one image goes
//! through the legacy API) converts nothing for `RGBA` and calls
//! `WebPEncode(im, lossless=0, float(q), 100.0, icc="", method, exact=0,
//! exif=b"", xmp="")`, which
//! * `WebPConfigInit`s a config (the default preset at 75) and sets
//!   `lossless`, `quality`, `alpha_quality` (an `int`, so 100), `method`
//!   and `exact`, then `WebPValidateConfig`;
//! * `WebPPictureInit`s a picture, `use_argb = 1`, its size the image's,
//!   `WebPPictureAlloc`, and fills `argb` as `A << 24 | R << 16 | G << 8 |
//!   B` from each RGBA pixel;
//! * encodes it with `WebPEncode` into a `WebPMemoryWriter`;
//! * and puts the bitstream through the mux, `WebPMuxNew`,
//!   `WebPMuxSetImage(copy_data = 0)` and `WebPMuxAssemble`, with no ICC,
//!   EXIF or XMP chunk; the assembled bytes are the file.
//!
//! **What `Image.open(webp)` reads**: `WebPGetFeatures` sniffs the mode
//! (`RGBA`, or `RGBX` read as `RGB` when the bitstream has no alpha),
//! `WebPAnimDecoderNew(data, NULL)`, the default options (`MODE_RGBA`, no
//! threads), and the first `WebPAnimDecoderGetNext` canvas is the image.

#![allow(unsafe_code)]

use core::ffi::{c_float, c_int, c_void};

/// The libwebp Pillow 12.3.0 linked, `features.version("webp")` in the
/// pinned Python.
pub const LIBWEBP: &str = "1.6.0";

/// `WEBP_ENCODER_ABI_VERSION`, `WEBP_DECODER_ABI_VERSION`,
/// `WEBP_MUX_ABI_VERSION` and `WEBP_DEMUX_ABI_VERSION` in its headers.
pub const ENCODER_ABI: c_int = 0x0210;
pub const DECODER_ABI: c_int = 0x0210;
pub const MUX_ABI: c_int = 0x0109;
pub const DEMUX_ABI: c_int = 0x0107;

/// `WEBP_PRESET_DEFAULT`, the preset `WebPConfigInit` passes, at 75.
pub const PRESET_DEFAULT: c_int = 0;
/// `WEBP_MUX_OK`.
pub const MUX_OK: c_int = 1;
/// `VP8_STATUS_OK`.
pub const STATUS_OK: c_int = 0;
/// `MODE_RGBA`.
pub const MODE_RGBA: c_int = 1;

/// `struct WebPConfig`, as `encode.h` lays it out.
#[repr(C)]
pub struct Config {
    pub lossless: c_int,
    pub quality: c_float,
    pub method: c_int,
    pub image_hint: c_int,
    pub target_size: c_int,
    pub target_psnr: c_float,
    pub segments: c_int,
    pub sns_strength: c_int,
    pub filter_strength: c_int,
    pub filter_sharpness: c_int,
    pub filter_type: c_int,
    pub autofilter: c_int,
    pub alpha_compression: c_int,
    pub alpha_filtering: c_int,
    pub alpha_quality: c_int,
    pub pass: c_int,
    pub show_compressed: c_int,
    pub preprocessing: c_int,
    pub partitions: c_int,
    pub partition_limit: c_int,
    pub emulate_jpeg_size: c_int,
    pub thread_level: c_int,
    pub low_memory: c_int,
    pub near_lossless: c_int,
    pub exact: c_int,
    pub use_delta_palette: c_int,
    pub use_sharp_yuv: c_int,
    pub qmin: c_int,
    pub qmax: c_int,
}

/// `struct WebPPicture`, as `encode.h` lays it out.
#[repr(C)]
pub struct Picture {
    pub use_argb: c_int,
    pub colorspace: c_int,
    pub width: c_int,
    pub height: c_int,
    pub y: *mut u8,
    pub u: *mut u8,
    pub v: *mut u8,
    pub y_stride: c_int,
    pub uv_stride: c_int,
    pub a: *mut u8,
    pub a_stride: c_int,
    pub pad1: [u32; 2],
    pub argb: *mut u32,
    pub argb_stride: c_int,
    pub pad2: [u32; 3],
    pub writer: Option<unsafe extern "C" fn(*const u8, usize, *const Picture) -> c_int>,
    pub custom_ptr: *mut c_void,
    pub extra_info_type: c_int,
    pub extra_info: *mut u8,
    pub stats: *mut c_void,
    pub error_code: c_int,
    pub progress_hook: *mut c_void,
    pub user_data: *mut c_void,
    pub pad3: [u32; 3],
    pub pad4: *mut u8,
    pub pad5: *mut u8,
    pub pad6: [u32; 8],
    pub memory: *mut c_void,
    pub memory_argb: *mut c_void,
    pub pad7: [*mut c_void; 2],
}

/// `struct WebPMemoryWriter`.
#[repr(C)]
pub struct MemoryWriter {
    pub mem: *mut u8,
    pub size: usize,
    pub max_size: usize,
    pub pad: [u32; 1],
}

/// `struct WebPData`.
#[repr(C)]
pub struct Data {
    pub bytes: *const u8,
    pub size: usize,
}

/// `struct WebPBitstreamFeatures`.
#[repr(C)]
pub struct Features {
    pub width: c_int,
    pub height: c_int,
    pub has_alpha: c_int,
    pub has_animation: c_int,
    pub format: c_int,
    pub pad: [u32; 5],
}

/// `struct WebPAnimInfo`.
#[repr(C)]
pub struct AnimInfo {
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub loop_count: u32,
    pub bgcolor: u32,
    pub frame_count: u32,
    pub pad: [u32; 4],
}

#[link(name = "webp")]
unsafe extern "C" {
    fn WebPGetEncoderVersion() -> c_int;
    fn WebPGetDecoderVersion() -> c_int;
    fn WebPConfigInitInternal(
        config: *mut Config,
        preset: c_int,
        quality: c_float,
        version: c_int,
    ) -> c_int;
    fn WebPValidateConfig(config: *const Config) -> c_int;
    fn WebPPictureInitInternal(picture: *mut Picture, version: c_int) -> c_int;
    fn WebPPictureAlloc(picture: *mut Picture) -> c_int;
    fn WebPPictureFree(picture: *mut Picture);
    fn WebPMemoryWriterInit(writer: *mut MemoryWriter);
    fn WebPMemoryWriterClear(writer: *mut MemoryWriter);
    fn WebPMemoryWrite(data: *const u8, size: usize, picture: *const Picture) -> c_int;
    fn WebPEncode(config: *const Config, picture: *mut Picture) -> c_int;
    fn WebPFree(ptr: *mut c_void);
    fn WebPGetFeaturesInternal(
        data: *const u8,
        size: usize,
        features: *mut Features,
        version: c_int,
    ) -> c_int;
}

#[link(name = "webpmux")]
unsafe extern "C" {
    fn WebPGetMuxVersion() -> c_int;
    fn WebPNewInternal(version: c_int) -> *mut c_void;
    fn WebPMuxSetImage(mux: *mut c_void, bitstream: *const Data, copy_data: c_int) -> c_int;
    fn WebPMuxAssemble(mux: *mut c_void, assembled: *mut Data) -> c_int;
    fn WebPMuxDelete(mux: *mut c_void);
}

#[link(name = "webpdemux")]
unsafe extern "C" {
    fn WebPGetDemuxVersion() -> c_int;
    fn WebPAnimDecoderNewInternal(
        data: *const Data,
        options: *const c_void,
        version: c_int,
    ) -> *mut c_void;
    fn WebPAnimDecoderGetInfo(dec: *const c_void, info: *mut AnimInfo) -> c_int;
    fn WebPAnimDecoderGetNext(dec: *mut c_void, buf: *mut *mut u8, timestamp: *mut c_int) -> c_int;
    fn WebPAnimDecoderDelete(dec: *mut c_void);
}

fn dotted(v: c_int) -> String {
    format!("{}.{}.{}", v >> 16, (v >> 8) % 0x100, v % 0x100)
}

/// The encoder's, decoder's, mux's and demux's versions, as
/// `WebPDecoderVersion_str` prints them.
#[must_use]
pub fn versions() -> [String; 4] {
    // SAFETY: each takes nothing and returns an int.
    unsafe {
        [
            dotted(WebPGetEncoderVersion()),
            dotted(WebPGetDecoderVersion()),
            dotted(WebPGetMuxVersion()),
            dotted(WebPGetDemuxVersion()),
        ]
    }
}

/// Whether every part of the linked libwebp is [`LIBWEBP`]. The version
/// string cannot tell this build from the revision's plain `libwebp`, which
/// is also 1.6.0; `tools/shell.nix` is what pins the build.
#[must_use]
pub fn pinned() -> bool {
    versions().iter().all(|v| v == LIBWEBP)
}

/// The encoder settings `_save` passes: lossy, `quality`, `alpha_quality`
/// and `method` as given, `exact` off.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub quality: f32,
    pub alpha_quality: f32,
    pub method: c_int,
}

/// `WebPEncode_wrapper` on an RGBA image `w` by `h`, row-major: the
/// assembled file.
///
/// # Errors
/// Where the wrapper raises: an invalid config, a picture it cannot
/// allocate, an encoding error, or a mux that assembles nothing.
pub fn encode(rgba: &[u8], w: usize, h: usize, s: Settings) -> Result<Vec<u8>, String> {
    assert_eq!(rgba.len(), w * h * 4, "an RGBA image of its size");
    let width = c_int::try_from(w).map_err(|_| "too wide".to_string())?;
    let height = c_int::try_from(h).map_err(|_| "too tall".to_string())?;
    // SAFETY: an all-zero Config and Picture are plain data, which the
    // init calls overwrite whole.
    let mut config: Config = unsafe { core::mem::zeroed() };
    // SAFETY: as above.
    let mut pic: Picture = unsafe { core::mem::zeroed() };
    // SAFETY: config is live; 75.f and the ABI are WebPConfigInit's.
    if unsafe { WebPConfigInitInternal(&raw mut config, PRESET_DEFAULT, 75.0, ENCODER_ABI) } == 0 {
        return Err("failed to initialize config!".into());
    }
    config.lossless = 0;
    config.quality = s.quality;
    // `config.alpha_quality = alpha_quality_factor;`, a float to an int.
    #[allow(clippy::cast_possible_truncation)]
    {
        config.alpha_quality = s.alpha_quality as c_int;
    }
    config.method = s.method;
    config.exact = 0;
    // SAFETY: config is initialised.
    if unsafe { WebPValidateConfig(&raw const config) } == 0 {
        return Err("invalid configuration".into());
    }
    // SAFETY: pic is live; the ABI is WebPPictureInit's.
    if unsafe { WebPPictureInitInternal(&raw mut pic, ENCODER_ABI) } == 0 {
        return Err("could not initialise picture".into());
    }
    pic.width = width;
    pic.height = height;
    pic.use_argb = 1;
    // SAFETY: pic is initialised with its size set.
    if unsafe { WebPPictureAlloc(&raw mut pic) } == 0 {
        return Err("can't allocate picture frame".into());
    }
    let stride = usize::try_from(pic.argb_stride).expect("a stride");
    for y in 0..h {
        // SAFETY: WebPPictureAlloc gave `height` rows of `argb_stride`
        // pixels, `width` of them used.
        let row = unsafe { core::slice::from_raw_parts_mut(pic.argb.add(stride * y), w) };
        for (x, d) in row.iter_mut().enumerate() {
            let p = &rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
            *d = u32::from(p[2])
                | (u32::from(p[1]) << 8)
                | (u32::from(p[0]) << 16)
                | (u32::from(p[3]) << 24);
        }
    }
    // SAFETY: an all-zero writer, which WebPMemoryWriterInit sets.
    let mut writer: MemoryWriter = unsafe { core::mem::zeroed() };
    // SAFETY: writer is live.
    unsafe { WebPMemoryWriterInit(&raw mut writer) };
    pic.writer = Some(WebPMemoryWrite);
    pic.custom_ptr = (&raw mut writer).cast();
    // SAFETY: config validated, pic allocated, its writer the memory
    // writer pointing at a live MemoryWriter.
    let ok = unsafe { WebPEncode(&raw const config, &raw mut pic) };
    let error = pic.error_code;
    // SAFETY: pic was allocated by WebPPictureAlloc.
    unsafe { WebPPictureFree(&raw mut pic) };
    if ok == 0 {
        // SAFETY: the writer's buffer is its own.
        unsafe { WebPMemoryWriterClear(&raw mut writer) };
        return Err(format!("encoding error {error}"));
    }
    let image = Data {
        bytes: writer.mem,
        size: writer.size,
    };
    // SAFETY: the ABI is WebPMuxNew's.
    let mux = unsafe { WebPNewInternal(MUX_ABI) };
    if mux.is_null() {
        // SAFETY: as above.
        unsafe { WebPMemoryWriterClear(&raw mut writer) };
        return Err("could not create mux object".into());
    }
    let mut out = Data {
        bytes: core::ptr::null(),
        size: 0,
    };
    // SAFETY: mux is live; image points at the writer's bytes, which stay
    // live until the writer is cleared after the mux is deleted (copy_data
    // 0 keeps a pointer to them, as the wrapper does).
    unsafe {
        let _ = WebPMuxSetImage(mux, &raw const image, 0);
        let _ = WebPMuxAssemble(mux, &raw mut out);
        WebPMuxDelete(mux);
        WebPMemoryWriterClear(&raw mut writer);
    }
    if out.size == 0 {
        return Err("cannot write file as WebP (encoder returned None)".into());
    }
    // SAFETY: WebPMuxAssemble gave `size` bytes at `bytes`, owned by the
    // caller and freed with WebPFree (WebPDataClear).
    let file = unsafe { core::slice::from_raw_parts(out.bytes, out.size) }.to_vec();
    // SAFETY: as above.
    unsafe { WebPFree(out.bytes.cast_mut().cast()) };
    Ok(file)
}

/// `Image.open(webp).convert("RGBA")`: the first canvas the animation
/// decoder gives, its size, RGBA; an image with no alpha has alpha 255.
///
/// # Errors
/// "could not create decoder object" or "failed to read next frame", as
/// the wrapper raises them.
pub fn decode(file: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    // SAFETY: an all-zero Features, which the call fills.
    let mut f: Features = unsafe { core::mem::zeroed() };
    // SAFETY: file is a live slice; the ABI is WebPGetFeatures'.
    let sniff =
        unsafe { WebPGetFeaturesInternal(file.as_ptr(), file.len(), &raw mut f, DECODER_ABI) };
    let alpha = !(sniff == STATUS_OK && f.has_alpha == 0);
    let data = Data {
        bytes: file.as_ptr(),
        size: file.len(),
    };
    // SAFETY: data points at the live file; NULL options are the default.
    let dec = unsafe { WebPAnimDecoderNewInternal(&raw const data, core::ptr::null(), DEMUX_ABI) };
    if dec.is_null() {
        return Err("could not create decoder object".into());
    }
    // SAFETY: an all-zero AnimInfo, which the call fills.
    let mut info: AnimInfo = unsafe { core::mem::zeroed() };
    let mut buf: *mut u8 = core::ptr::null_mut();
    let mut ts: c_int = 0;
    // SAFETY: dec is live; buf points into the decoder, valid until the
    // next call or its deletion, and is copied before either.
    let out = unsafe {
        if WebPAnimDecoderGetInfo(dec, &raw mut info) == 0 {
            WebPAnimDecoderDelete(dec);
            return Err("could not create decoder object".into());
        }
        if WebPAnimDecoderGetNext(dec, &raw mut buf, &raw mut ts) == 0 {
            WebPAnimDecoderDelete(dec);
            return Err("failed to read next frame".into());
        }
        let n = info.canvas_width as usize * info.canvas_height as usize * 4;
        let v = core::slice::from_raw_parts(buf, n).to_vec();
        WebPAnimDecoderDelete(dec);
        v
    };
    let mut out = out;
    if !alpha {
        for p in out.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
    }
    Ok((info.canvas_width as usize, info.canvas_height as usize, out))
}

#[cfg(test)]
mod abi {
    use super::{AnimInfo, Config, Data, Features, MemoryWriter, Picture};
    use std::collections::HashMap;
    use std::process::Command;

    const PROBE: &str = r#"
#include <stddef.h>
#include <stdio.h>
#include <webp/decode.h>
#include <webp/demux.h>
#include <webp/encode.h>
#include <webp/mux.h>
#define SIZE(t) printf("sizeof." #t "=%lld\n", (long long)sizeof(t))
#define AT(t, f) printf(#t "." #f "=%lld\n", (long long)offsetof(t, f))
#define VALUE(v) printf(#v "=%lld\n", (long long)(v))
int main(void) {
    SIZE(WebPConfig); AT(WebPConfig, lossless); AT(WebPConfig, quality);
    AT(WebPConfig, method); AT(WebPConfig, alpha_quality);
    AT(WebPConfig, exact); AT(WebPConfig, qmax);
    SIZE(WebPPicture); AT(WebPPicture, use_argb); AT(WebPPicture, width);
    AT(WebPPicture, height); AT(WebPPicture, argb);
    AT(WebPPicture, argb_stride); AT(WebPPicture, writer);
    AT(WebPPicture, custom_ptr); AT(WebPPicture, error_code);
    AT(WebPPicture, pad7);
    SIZE(WebPMemoryWriter); AT(WebPMemoryWriter, mem);
    AT(WebPMemoryWriter, size);
    SIZE(WebPData); AT(WebPData, size);
    SIZE(WebPBitstreamFeatures); AT(WebPBitstreamFeatures, has_alpha);
    SIZE(WebPAnimInfo); AT(WebPAnimInfo, canvas_height);
    AT(WebPAnimInfo, frame_count);
    VALUE(WEBP_ENCODER_ABI_VERSION); VALUE(WEBP_DECODER_ABI_VERSION);
    VALUE(WEBP_MUX_ABI_VERSION); VALUE(WEBP_DEMUX_ABI_VERSION);
    VALUE(WEBP_PRESET_DEFAULT); VALUE(WEBP_MUX_OK); VALUE(VP8_STATUS_OK);
    VALUE(MODE_RGBA);
    return 0;
}
"#;

    fn ask() -> HashMap<String, String> {
        let dir =
            std::env::temp_dir().join(format!("baud-pictures-webp-abi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let (src, bin) = (dir.join("probe.c"), dir.join("probe"));
        std::fs::write(&src, PROBE).expect("write the probe");
        let flags = Command::new("pkg-config")
            .args(["--cflags", "libwebp", "libwebpmux", "libwebpdemux"])
            .output()
            .expect("pkg-config, which tools/shell.nix provides");
        assert!(flags.status.success(), "pkg-config found no libwebp");
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
        assert_eq!(map.len(), 35, "the probe's lines: {text}");
        map
    }

    #[test]
    fn the_structs_are_laid_out_as_libwebps_headers_lay_them_out() {
        use core::mem::offset_of;
        let c = ask();
        let n = |k: &str| c[k].parse::<usize>().unwrap_or_else(|_| panic!("{k}"));
        for (k, v) in [
            ("sizeof.WebPConfig", size_of::<Config>()),
            ("WebPConfig.lossless", offset_of!(Config, lossless)),
            ("WebPConfig.quality", offset_of!(Config, quality)),
            ("WebPConfig.method", offset_of!(Config, method)),
            (
                "WebPConfig.alpha_quality",
                offset_of!(Config, alpha_quality),
            ),
            ("WebPConfig.exact", offset_of!(Config, exact)),
            ("WebPConfig.qmax", offset_of!(Config, qmax)),
            ("sizeof.WebPPicture", size_of::<Picture>()),
            ("WebPPicture.use_argb", offset_of!(Picture, use_argb)),
            ("WebPPicture.width", offset_of!(Picture, width)),
            ("WebPPicture.height", offset_of!(Picture, height)),
            ("WebPPicture.argb", offset_of!(Picture, argb)),
            ("WebPPicture.argb_stride", offset_of!(Picture, argb_stride)),
            ("WebPPicture.writer", offset_of!(Picture, writer)),
            ("WebPPicture.custom_ptr", offset_of!(Picture, custom_ptr)),
            ("WebPPicture.error_code", offset_of!(Picture, error_code)),
            ("WebPPicture.pad7", offset_of!(Picture, pad7)),
            ("sizeof.WebPMemoryWriter", size_of::<MemoryWriter>()),
            ("WebPMemoryWriter.mem", offset_of!(MemoryWriter, mem)),
            ("WebPMemoryWriter.size", offset_of!(MemoryWriter, size)),
            ("sizeof.WebPData", size_of::<Data>()),
            ("WebPData.size", offset_of!(Data, size)),
            ("sizeof.WebPBitstreamFeatures", size_of::<Features>()),
            (
                "WebPBitstreamFeatures.has_alpha",
                offset_of!(Features, has_alpha),
            ),
            ("sizeof.WebPAnimInfo", size_of::<AnimInfo>()),
            (
                "WebPAnimInfo.canvas_height",
                offset_of!(AnimInfo, canvas_height),
            ),
            (
                "WebPAnimInfo.frame_count",
                offset_of!(AnimInfo, frame_count),
            ),
        ] {
            assert_eq!(n(k), v, "{k}");
        }
        for (k, v) in [
            ("WEBP_ENCODER_ABI_VERSION", super::ENCODER_ABI),
            ("WEBP_DECODER_ABI_VERSION", super::DECODER_ABI),
            ("WEBP_MUX_ABI_VERSION", super::MUX_ABI),
            ("WEBP_DEMUX_ABI_VERSION", super::DEMUX_ABI),
            ("WEBP_PRESET_DEFAULT", super::PRESET_DEFAULT),
            ("WEBP_MUX_OK", super::MUX_OK),
            ("VP8_STATUS_OK", super::STATUS_OK),
            ("MODE_RGBA", super::MODE_RGBA),
        ] {
            assert_eq!(c[k], v.to_string(), "{k}");
        }
    }

    #[test]
    fn the_linked_libwebp_is_the_one_pillow_linked() {
        assert_eq!(super::versions(), [super::LIBWEBP; 4].map(String::from));
    }
}
