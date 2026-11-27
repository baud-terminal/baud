//! **The README's header logo, filmed with baud's own renderer and
//! encoded**: `baud-pictures logo` writes `docs/assets/readme/logo-dark.gif`,
//! `logo-light.gif`, `logo-dark.svg` and `logo-light.svg`; `logo --check`
//! films and encodes all four again and fails unless each equals the
//! committed file byte for byte, writing no tracked file; `logo --out DIR`
//! writes all four to `DIR` in place of the tree, and holds those.
//!
//! **`--mp4`**, with `--out DIR` and only with it, also writes each half's
//! film as an MP4 beside its GIF in `DIR`, `logo-dark.mp4` and `logo-light.mp4`, for a
//! video to cut it into: the same frames, each held its own hundredths at a
//! constant 100 frames a second, so every frame keeps its time exactly,
//! encoded by the pinned ffmpeg's libx264 with fixed settings, four threads,
//! no metadata and every bit-exact flag ([`MP4`]). It is encoded twice and
//! must be the same bytes both times, as long as the film and as many
//! frames as its hundredths. Nothing commits the MP4s, so they are never
//! written into the tree, where an untracked file makes the demo's
//! `--record` refuse it as dirty: without `--out`, `--mp4` is refused.
//!
//! **`--ufo`** films the UFO logo instead ([`crate::ufo`]): the same
//! lockup, walls, encoder, checks and census, the saucer threading the
//! letters in place of the plasma, into `docs/assets/demo/`.
//!
//! **What it shows**: the lockup, `baud ❯ █`, as the launch splash holds
//! it and the logo files draw it, with baud's white plasma moving round
//! and through it as if each glyph were a solid wall, its block blinking
//! as baud's cursor blinks by default, the app's own caret asked for each
//! frame. It plays the plasma's whole loop, twenty blink cycles at the
//! default 250 ms shown and 250 hidden, at full light, and repeats, so the
//! plasma never rests: no frame is the bare logo. `crates/baud-app/src/logo_film.rs`
//! draws each frame; [`crate::plasma`] decides what the plasma does.
//!
//! **How the random path closes.** A random walk does not come back, so the
//! plasma's is built to: its moves are drawn from a seeded random number
//! and the last one is laid back onto the first's start with its depth's
//! parity kept, its current is curl noise of waves each periodic in the loop
//! (Bridson's construction made tileable in time), and its surges and events
//! wrap, so the frame after the last is the first. No fade or cross-dissolve
//! hides a seam, because there is none.
//! [`crate::plasma::Plasma::first_unrepeated`] holds that each frame's strands
//! equal those a whole loop later, which tests the paths that read raw time.
//! The current reads time already reduced to the loop, so this test cannot see
//! a current that does not close; the strand digests pin the waves, and the
//! decoded film's wrap step is measured against every other step. The file
//! then only says to repeat:
//! the GIF's NETSCAPE2.0 block, a count of zero
//! (`[[source: netscape-2-0-extension]]`), and the SVG's discrete SMIL
//! animations, `repeatCount="indefinite"` (`[[source: smil-animation]]`).
//!
//! **Where it departs from a standard, knowingly**: WCAG 2.2's 2.2.2 Pause,
//! Stop, Hide (Level A) asks that moving or blinking content which starts by
//! itself, lasts more than five seconds and is shown beside other content
//! can be paused, stopped or hidden (`[[source: wcag22]]`), and
//! `[[source: wcag22-understanding-pause-stop-hide]]` counts content that
//! does not as interfering with the whole page. A README carries no control
//! of its own, and GitHub's setting to pause animated images does not reach
//! an image inside `<picture>`, so a logo that repeats does not meet it
//! there. So the README's `<picture>` serves a reader who asked for reduced
//! motion the still lockup in place of the film, by
//! `prefers-reduced-motion` on its first two sources, which
//! `the_readmes_logo_gives_reduced_motion_a_still` holds; a reader who did
//! not ask gets the film and no control. WCAG 2.3.1 is a separate matter and
//! is met: the census counts the repeat and its join.
//!
//! **How it is made**, for each half, on GitHub's page colour, `#0d1117`
//! for the dark theme and `#ffffff` for the light:
//!  1. baud-app's logo film draws the bare logo, the block shown;
//!  2. the dark half's bare logo is held to `docs/assets/readme/logo-walls.png`,
//!     the drawing the plasma's path was chosen against, which
//!     [`WALLS_SHA256`] pins: today's renderer must draw the word within
//!     [`WALLS_OFF`] of it, and the mark moved up as [`MARK_MOVED`] says,
//!     or the lockup has moved and the path must be chosen again;
//!  3. [`crate::geometry`] reads the walls' signed distance field, normals
//!     and counters off it, and [`crate::plasma`] writes each frame's
//!     strands against them, in arithmetic that calls none of the
//!     platform's `sin`, `exp` and the rest, so a run is the same on every
//!     platform;
//!  4. the film draws each frame through the renderer's splash layers and
//!     slabs, cut against today's bare logo and its own distance field, the
//!     block at the caret's phase, in release's optimisation without LTO (a
//!     test, not an example: the blink is the app's, and the app is a binary
//!     whose modules no example reaches);
//!  5. every pixel within [`SNAP`] levels of the page is set to it, so the
//!     encoder's error cannot haze the page round the logo;
//!  6. gifski encodes the GIF at [`QUALITY`], its motion and lossy passes
//!     whole, the page a fixed colour of its palette, repeating for ever;
//!     every palette entry within [`crate::gif::PALETTE_SNAP`] levels of the
//!     page is then set to it, and each frame's delay to the film's, which
//!     gifski's frame rate cannot say for frames of two lengths;
//!  7. the SVG is the bare logo as a PNG with each frame over it as a WebP
//!     of what it changes, each shown and hidden by one discrete SMIL
//!     `<animate>` at the GIF's own times, repeating over its length.
//!
//! **What it checks before it writes**, each from the files it made:
//!  - gifski and the SVG's encoder, run twice on the same frames, write
//!    the same bytes;
//!  - the GIF lasts the film's length, 840 by 580, repeats, under
//!    [`LIMIT`] bytes; each of the SVG's frame images ends where the next
//!    frame starts;
//!  - the plasma never rests and the wrap is no jump: every frame moves
//!    pixels off the block against the bare logo, and the step from the last
//!    frame to the first moves no more than the film's busiest ([`wrap_holds`]);
//!  - the haze: of the pixels the film drew as the page, the share the
//!    decoded GIF shows off it, and the worst level (the GIF decoded by
//!    baud-bench's `gif_frames` example);
//!  - the word's contrast against the ground round it, every frame of both
//!    files, at least [`CONTRAST`] to one;
//!  - WCAG 2.3.1's census of the SVG's frames as they decode, at 100
//!    samples a second and 2 pixels a point, fed twice over so every second
//!    across the join is counted: both verdicts pass (baud-bench's
//!    `flash_census` example).
//!
//! Then, on the files it wrote -- or with `--check` the committed ones -- it
//! runs the tests that hold them, and prints what they read: baud-app's
//! `logo_film::tests::the_readme_logos_repeat_and_the_plasma_never_rests`
//! and `logo_film::tests::the_readme_svgs_repeat_at_the_gifs_times`, and
//! baud-bench's `readme_logo_flashes`, the census of the GIFs, each pointed
//! at `DIR` by `BAUD_README_FILES` under `--out`.
//!
//! **The environment is pinned whole.** zlib 1.3.2 and libwebp 1.6.0,
//! linked through `extern "C"` ([`crate::zlib`], [`crate::webp`]), come
//! from nixpkgs `7dd199b0`, `tools/shell.nix`'s pin for the pictures, the
//! builds Pillow 12.3.0 carried and linked there, and the run refuses
//! others; gifski 1.34.0 is the binary `readme-film.sh`'s nixpkgs builds,
//! run as a program and never linked (it is AGPL-3.0-or-later). The frames
//! come from the repository's renderer, built by the cargo on PATH. The
//! files were made on aarch64-darwin, and `--check` runs by hand, not in
//! `devenv test`.

use crate::json::{self, Value};
use crate::png::{self, Image, Mode};
use crate::{geometry, gif, numpy, plasma, py, webp};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The nixpkgs revision gifski is built from, `readme-film.sh`'s.
pub const GIFSKI_NIXPKGS: &str = "github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a";
/// `gifski --version` there.
pub const GIFSKI: &str = "gifski 1.34.0";
/// The first line of `ffmpeg -version` from the same nixpkgs, whose
/// `ffmpeg-headless.bin` encodes the MP4s.
pub const FFMPEG: &str = "ffmpeg version 9.0.1";

/// The MP4's frame rate: a hundredth a frame, the films' own unit.
pub const MP4_FPS: u32 = 100;

/// **The MP4's encoder settings**: RGB to BT.709 4:2:0 in TV range, H.264
/// High at CRF 18, `slow`, four threads, no audio, nothing of the run in
/// the file -- the demo's `--record` MP4's settings
/// (`crates/baud-app/examples/demo/film.rs`), the conversion added.
pub const MP4: [&str; 31] = [
    "-vf",
    "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p",
    "-an",
    "-c:v",
    "libx264",
    "-preset",
    "slow",
    "-crf",
    "18",
    "-pix_fmt",
    "yuv420p",
    "-colorspace",
    "bt709",
    "-color_primaries",
    "bt709",
    "-color_trc",
    "bt709",
    "-threads",
    "4",
    "-fflags",
    "+bitexact",
    "-flags:v",
    "+bitexact",
    "-map_metadata",
    "-1",
    "-map_chapters",
    "-1",
    "-movflags",
    "+faststart",
    "-f",
    "mp4",
];

/// Whether `logo` writes the MP4s too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mp4 {
    Yes,
    No,
}

/// **`--mp4` taken out of `logo`'s arguments**, wherever it stands, and
/// the rest for [`args`].
#[must_use]
pub fn mp4_asked(argv: &[String]) -> (Mp4, Vec<String>) {
    let rest: Vec<String> = argv.iter().filter(|a| *a != "--mp4").cloned().collect();
    match rest.len() == argv.len() {
        true => (Mp4::No, rest),
        false => (Mp4::Yes, rest),
    }
}
/// nixpkgs' revision for zlib and libwebp, `tools/shell.nix`'s `pictures`.
pub const NIXPKGS_REV: &str = "7dd199b0e2993e37b4775ed66b1c291699608c9f";

/// The GIF: gifski at 90, where 80 left the page round the logo off by up
/// to 5 levels in its frames, its motion and lossy passes whole.
pub const QUALITY: u32 = 90;
pub const MOTION: u32 = 100;
pub const LOSSY: u32 = 100;
/// Pixels this many levels of the page or nearer are set to it before
/// the encoders see them: 1.2% of white, which no eye sees over the page.
pub const SNAP: u8 = 3;
/// The SVG's frames: WebP at this quality, their alpha lossless.
pub const WEBP_QUALITY: f32 = 90.0;
/// The word against the ground round it, the worst frame: WCAG 1.4.3 asks
/// nothing of a logotype ("Text that is part of a logo or brand name has
/// no contrast requirement", `[[source: wcag22]]`); the floor borrows the
/// 4.5 to 1 it asks of text, so the plasma never washes the word out.
pub const CONTRAST: f64 = 4.5;

/// Each half: its files, its page and its word's ink.
pub struct Half {
    pub name: &'static str,
    pub gif: &'static str,
    pub svg: &'static str,
    pub page: [u8; 3],
    pub word: [u8; 3],
}

pub const HALVES: [Half; 2] = [
    Half {
        name: "dark",
        gif: "docs/assets/readme/logo-dark.gif",
        svg: "docs/assets/readme/logo-dark.svg",
        page: [0x0D, 0x11, 0x17],
        word: [0xF3, 0xF4, 0xF6],
    },
    Half {
        name: "light",
        gif: "docs/assets/readme/logo-light.gif",
        svg: "docs/assets/readme/logo-light.svg",
        page: [0xFF, 0xFF, 0xFF],
        word: [0x1E, 0x21, 0x28],
    },
];

/// The canvas.
pub const W: usize = 840;
pub const H: usize = 580;
/// A README loads its header on every visit.
pub const LIMIT: usize = 5_000_000;
/// A channel this far off another counts as moved, past [`SNAP`]: the snap
/// sets a pixel this near the page to it, and the plasma's faintest light
/// is not a thing the eye reads as motion.
pub const MOVED: u8 = 4;

pub const WALLS: &str = "docs/assets/readme/logo-walls.png";
pub const WALLS_SHA256: &str = "931c0a901594ee8834194315ce1e36094e4a7dc00a24ea3a4670f48c6dab8638";

/// **The canvas's columns the word stands in**, left of the ❯'s, and of the
/// mark's reach on the word's distance field.
pub const WORD_COLUMNS: usize = 510;

/// How far today's word may be from the walls' over [`WORD_COLUMNS`], each
/// at the count measured when the mark moved: pixels differing and the most a
/// channel moves, pixels of the ink's half-coverage mask differing, and
/// the most the signed distance moves, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Off {
    pub pixels: u64,
    pub levels: u64,
    pub mask: u64,
    pub sdf: f64,
}

pub const WALLS_OFF: Off = Off {
    pixels: 3873,
    levels: 2,
    mask: 2,
    sdf: 1.0,
};

/// **A glyph of the mark as it stands against the walls**: its columns of
/// the canvas, how many rows it stands higher, and how far it may then be
/// off them, each at the count measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    pub columns: (usize, usize),
    pub rows: usize,
    pub pixels: u64,
    pub levels: u64,
}

/// **The mark moved up against the word**: 2.26 of the lockup's units, at
/// the film's two pixels a unit, the ❯'s baseline and the block's rows each
/// rounded to their own whole pixel. The path keeps the walls' mark, and
/// stays the path that was chosen: every chain from the walls of the moved
/// mark is another path, since each candidate of the search shifts when one
/// does. The word is where the path was chosen, pixel for pixel.
pub const MARK_MOVED: [Moved; 2] = [
    Moved {
        columns: (WORD_COLUMNS, 562),
        rows: 4,
        pixels: 57,
        levels: 2,
    },
    Moved {
        columns: (562, crate::geometry::W),
        rows: 5,
        pixels: 0,
        levels: 0,
    },
];

impl Off {
    /// The dict's `repr`, as the walls line prints it.
    #[must_use]
    pub fn repr(&self) -> String {
        format!(
            "{{'pixels': {}, 'levels': {}, 'mask': {}, 'sdf': {}}}",
            self.pixels,
            self.levels,
            self.mask,
            py::repr_float(self.sdf)
        )
    }

    /// `any(off[k] > WALLS_OFF[k] for k in WALLS_OFF)`.
    #[must_use]
    pub fn past(&self, bound: &Off) -> bool {
        self.pixels > bound.pixels
            || self.levels > bound.levels
            || self.mask > bound.mask
            || self.sdf > bound.sdf
    }
}

/// release's optimisation without LTO, for the film and the census.
pub(crate) const FAST: [&str; 8] = [
    "--config",
    "profile.fast.inherits=\"release\"",
    "--config",
    "profile.fast.lto=false",
    "--config",
    "profile.fast.codegen-units=16",
    "--profile",
    "fast",
];

/// The tests that hold the files in the tree.
const TESTS: [&[&str]; 2] = [
    &[
        "-p",
        "baud",
        "--bin",
        "baud",
        "--",
        "logo_film::tests::the_readme_logos_repeat_and_the_plasma_never_rests",
        "logo_film::tests::the_readme_svgs_repeat_at_the_gifs_times",
    ],
    &["-p", "baud-bench", "--test", "readme_logo_flashes"],
];

/// What stops the run, printed after `baud-pictures logo: `.
pub type Fail = String;

/// A frame image of the SVG: its x and y, its WebP's base64 and where it
/// ends, in hundredths.
type Shown<'a> = (usize, usize, &'a str, u32);

/// A GIF's frames, RGB, with their hundredths; whether it loops; its size.
pub type Decoded = (Vec<(Vec<u8>, u32)>, bool, (usize, usize));

/// One half's GIF, its SVG and the lines that report them.
type Made = (Vec<u8>, Vec<u8>, Vec<String>);

// ---------------------------------------------------------------- pure parts

/// The snap: every pixel of an RGB frame whose channels are all within
/// [`SNAP`] of `page` set to `page`.
pub fn snap(frame: &mut [u8], page: [u8; 3]) {
    for p in frame.as_chunks_mut::<3>().0 {
        if (0..3).all(|j| p[j].abs_diff(page[j]) <= SNAP) {
            p.copy_from_slice(&page);
        }
    }
}

/// The film's frames out of `frames.rgb`, each snapped.
///
/// # Errors
/// "frames.rgb holds more than the film's frames", and a file too short.
pub fn snapped(raw: &[u8], frames: usize, page: [u8; 3]) -> Result<Vec<Vec<u8>>, Fail> {
    let n = W * H * 3;
    if raw.len() < n * frames {
        return Err(format!(
            "frames.rgb holds {} bytes, under {frames} frames",
            raw.len()
        ));
    }
    if raw.len() > n * frames {
        return Err("frames.rgb holds more than the film's frames".into());
    }
    Ok(raw
        .chunks_exact(n)
        .map(|c| {
            let mut f = c.to_vec();
            snap(&mut f, page);
            f
        })
        .collect())
}

/// How far `today`'s bare logo is from the `walls`, both RGB canvases,
/// with each one's signed distance field.
#[must_use]
pub fn walls_off(walls: &[u8], today: &[u8], sdf_walls: &[f32], sdf_today: &[f32]) -> Off {
    let (mut pixels, mut levels) = (0u64, 0u64);
    for (a, b) in walls
        .as_chunks::<3>()
        .0
        .iter()
        .zip(today.as_chunks::<3>().0)
    {
        let d = (0..3).map(|j| a[j].abs_diff(b[j])).max().unwrap_or(0);
        if d > 0 {
            pixels += 1;
        }
        levels = levels.max(u64::from(d));
    }
    let mask = sdf_walls
        .iter()
        .zip(sdf_today)
        .filter(|(a, b)| (**a < 0.0) != (**b < 0.0))
        .count() as u64;
    let sdf = sdf_walls
        .iter()
        .zip(sdf_today)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    Off {
        pixels,
        levels,
        mask,
        sdf: py::round_to(f64::from(sdf), 3),
    }
}

/// The columns `x0..x1` of `w` by `h` values, each `n` long, row by row.
fn columns<T: Copy>(v: &[T], (w, h): (usize, usize), n: usize, (x0, x1): (usize, usize)) -> Vec<T> {
    (0..h)
        .flat_map(|y| v[(y * w + x0) * n..(y * w + x1) * n].iter().copied())
        .collect()
}

/// **What keeps `today`'s word off the `walls`' and its mark from being
/// theirs moved up**, both RGB canvases, or nothing, with each one's signed
/// distance field: how far the word is, and each glyph of the mark past its
/// [`MARK_MOVED`].
#[must_use]
pub fn lockup_faults(
    walls: &[u8],
    today: &[u8],
    sdf_walls: &[f32],
    sdf_today: &[f32],
) -> (Off, Vec<String>) {
    let size = (geometry::W, geometry::H);
    let word = (0, WORD_COLUMNS);
    let off = walls_off(
        &columns(walls, size, 3, word),
        &columns(today, size, 3, word),
        &columns(sdf_walls, size, 1, word),
        &columns(sdf_today, size, 1, word),
    );
    let mut bad = Vec::new();
    for m in MARK_MOVED {
        let (mut pixels, mut levels) = (0u64, 0u64);
        for y in m.rows..geometry::H {
            for x in m.columns.0..m.columns.1 {
                let at = |c: &[u8], row: usize| -> [u8; 3] {
                    let i = (row * geometry::W + x) * 3;
                    [c[i], c[i + 1], c[i + 2]]
                };
                let (a, b) = (at(walls, y), at(today, y - m.rows));
                let d = (0..3).map(|j| a[j].abs_diff(b[j])).max().unwrap_or(0);
                pixels += u64::from(d > 0);
                levels = levels.max(u64::from(d));
            }
        }
        if pixels > m.pixels || levels > m.levels {
            bad.push(format!(
                "columns {:?} are {pixels} pixels, {levels} levels off the walls' {} rows up, at most {} and {}",
                m.columns, m.rows, m.pixels, m.levels
            ));
        }
    }
    (off, bad)
}

/// The haze: of the pixels the film drew as the page, the share in
/// percent (rounded to 4 places) the decoded GIF shows off it, and the
/// worst level; a decoded frame stands for each of the film's frames it
/// merged.
#[must_use]
pub fn haze(
    rendered: &[Vec<u8>],
    got: &[(Vec<u8>, u32)],
    page: [u8; 3],
    delays: &[u32],
) -> (f64, u8) {
    let (mut n, mut off, mut worst, mut i) = (0u64, 0u64, 0u8, 0usize);
    for (g, cs) in got {
        let dv: Vec<u8> = g
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| (0..3).map(|j| p[j].abs_diff(page[j])).max().unwrap_or(0))
            .collect();
        let mut span = 0;
        while span < *cs {
            for (p, &d) in rendered[i].as_chunks::<3>().0.iter().zip(&dv) {
                if *p == page {
                    n += 1;
                    if d >= 1 {
                        off += 1;
                        worst = worst.max(d);
                    }
                }
            }
            span += delays[i];
            i += 1;
        }
    }
    #[allow(clippy::cast_precision_loss, reason = "counts of pixels")]
    let share = 100.0 * off as f64 / n.max(1) as f64;
    (py::round_to(share, 4), worst)
}

/// WCAG's relative luminance of an sRGB pixel, as the driver's NumPy
/// computed it: each channel over 255.0, linearised (`c / 12.92` at or
/// under 0.04045, else `((c + 0.055) / 1.055) ** 2.4`, the platform's
/// `pow`, as NumPy's), weighted 0.2126, 0.7152 and 0.0722 and added left
/// to right.
#[must_use]
pub fn luminance(p: &[u8]) -> f64 {
    let lin = |v: u8| {
        let c = f64::from(v) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    lin(p[0]) * 0.2126 + lin(p[1]) * 0.7152 + lin(p[2]) * 0.0722
}

/// `mask` grown by `r` pixels in every direction, a square.
fn grown(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if !mask[y * w + x] {
                continue;
            }
            for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                    out[yy * w + xx] = true;
                }
            }
        }
    }
    out
}

/// The word's contrast in each of `frames` (RGB canvases), sorted: the
/// mean luminance of the pixels `bare` draws in the word's own ink against
/// the ring 2 to 7 pixels round its ink, inside the `word` box, WCAG's
/// `(L1 + 0.05) / (L2 + 0.05)`. The means are NumPy's, pairwise over the
/// row-major selection.
#[must_use]
pub fn ratios(
    frames: &[&[u8]],
    bare: &[u8],
    word_box: [usize; 4],
    page: [u8; 3],
    word: [u8; 3],
) -> Vec<f64> {
    let [x0, y0, x1, y1] = word_box;
    let (bw, bh) = (x1 - x0, y1 - y0);
    let px = |img: &[u8], x: usize, y: usize| {
        let i = ((y0 + y) * W + x0 + x) * 3;
        [img[i], img[i + 1], img[i + 2]]
    };
    let mut ink = vec![false; bw * bh];
    let mut solid = vec![false; bw * bh];
    for y in 0..bh {
        for x in 0..bw {
            let p = px(bare, x, y);
            ink[y * bw + x] = (0..3).any(|j| p[j] != page[j]);
            solid[y * bw + x] = p == word;
        }
    }
    let (g7, g2) = (grown(&ink, bw, bh, 7), grown(&ink, bw, bh, 2));
    let ring: Vec<bool> = g7.iter().zip(&g2).map(|(a, b)| *a && !*b).collect();
    let mut rs: Vec<f64> = frames
        .iter()
        .map(|f| {
            let (mut sa, mut sb) = (Vec::new(), Vec::new());
            for y in 0..bh {
                for x in 0..bw {
                    let l = luminance(&px(f, x, y));
                    if solid[y * bw + x] {
                        sa.push(l);
                    }
                    if ring[y * bw + x] {
                        sb.push(l);
                    }
                }
            }
            let (a, b) = (numpy::mean(&sa), numpy::mean(&sb));
            (py::max(a, b) + 0.05) / (py::min(a, b) + 0.05)
        })
        .collect();
    rs.sort_by(f64::total_cmp);
    rs
}

/// The worst and the median of [`ratios`], each rounded to 2 places.
#[must_use]
pub fn contrast(
    frames: &[&[u8]],
    bare: &[u8],
    word_box: [usize; 4],
    page: [u8; 3],
    word: [u8; 3],
) -> (f64, f64) {
    let rs = ratios(frames, bare, word_box, page, word);
    (py::round_to(rs[0], 2), py::round_to(rs[rs.len() / 2], 2))
}

/// **The film's moved pixels**: how many of two canvases' pixels differ by
/// more than [`MOVED`] in a channel, outside `block`, which the blink alone
/// moves.
#[must_use]
pub fn moved(a: &[u8], b: &[u8], block: [usize; 4]) -> usize {
    let [x0, y0, x1, y1] = block;
    a.as_chunks::<3>()
        .0
        .iter()
        .zip(b.as_chunks::<3>().0)
        .enumerate()
        .filter(|(i, (p, q))| {
            let (x, y) = (i % W, i / W);
            !((x0..x1).contains(&x) && (y0..y1).contains(&y))
                && (0..3).any(|j| p[j].abs_diff(q[j]) > MOVED)
        })
        .count()
}

/// **The plasma never rests and the wrap is no jump**: every frame moves
/// pixels against the bare logo, and the step from the last frame to the
/// first moves no more than the film's busiest step. The least a frame moves
/// against the bare logo, the wrap's and the busiest step's.
///
/// # Errors
/// When a frame is the bare logo, or the wrap jumps.
pub fn wrap_holds(
    half: &str,
    frames: &[Vec<u8>],
    still: &[u8],
    block: [usize; 4],
) -> Result<(usize, usize, usize), Fail> {
    let (mut least, mut busiest) = (usize::MAX, 0);
    for (k, f) in frames.iter().enumerate() {
        let lit = moved(f, still, block);
        if lit == 0 {
            return Err(format!("{half}: frame {k} is the bare logo"));
        }
        least = least.min(lit);
        if k > 0 {
            busiest = busiest.max(moved(f, &frames[k - 1], block));
        }
    }
    let wrap = moved(&frames[0], &frames[frames.len() - 1], block);
    if wrap > busiest {
        return Err(format!(
            "{half}: the wrap moves {wrap} pixels, past the film's busiest step's {busiest}"
        ));
    }
    Ok((least, wrap, busiest))
}

/// The GIF's own checks: it lasts the film's length, repeats, is the canvas's
/// size and under [`LIMIT`] bytes.
///
/// # Errors
/// The driver's words for whichever fails first.
pub fn gif_holds(
    half: &str,
    lasts: u32,
    total: u32,
    loops: bool,
    size: (usize, usize),
    bytes: usize,
) -> Result<(), Fail> {
    if lasts != total {
        return Err(format!(
            "{half}: the GIF lasts {lasts} cs, not the film's {total}"
        ));
    }
    if !loops || size != (W, H) || bytes >= LIMIT {
        let l = if loops { "True" } else { "False" };
        return Err(format!(
            "{half}: loops {l}, {}x{}, {bytes} bytes",
            size.0, size.1
        ));
    }
    Ok(())
}

/// `logo`'s usage line.
const USAGE: &str = "usage: baud-pictures logo [--check | --out DIR] [--keep DIR]";

/// What `logo` does with the files it makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Files {
    /// Writes them over the tree's.
    Write,
    /// Compares them with the tree's, writing nothing tracked.
    Check,
    /// Writes them to this directory, and holds those.
    Out(PathBuf),
}

/// `logo`'s arguments: what it does with the files, and where it keeps its
/// work.
///
/// # Errors
/// The usage line.
pub fn args(argv: &[String]) -> Result<(Files, Option<PathBuf>), Fail> {
    let (mut files, mut keep, mut it) = (Files::Write, None, argv.iter());
    while let Some(a) = it.next() {
        match (a.as_str(), it.as_slice().first(), &files) {
            ("--check", _, Files::Write) => files = Files::Check,
            ("--out", Some(d), Files::Write) => {
                files = Files::Out(PathBuf::from(d));
                it.next();
            }
            ("--keep", Some(d), _) => {
                keep = Some(PathBuf::from(d));
                it.next();
            }
            _ => return Err(USAGE.into()),
        }
    }
    Ok((files, keep))
}

/// The walls are the drawing the path was chosen against.
///
/// # Errors
/// When `data` is another file.
pub fn walls_pinned(data: &[u8]) -> Result<(), Fail> {
    if crate::sha256::hex(data) != WALLS_SHA256 {
        return Err(format!(
            "{WALLS} is not the walls the path was chosen against"
        ));
    }
    Ok(())
}

/// `--check` on one file: its line, and the failure when `made` is not
/// the bytes `committed`.
#[must_use]
pub fn compared(path: &str, committed: Option<&[u8]>, made: &[u8]) -> (String, Option<String>) {
    if committed == Some(made) {
        (format!("same    {path}"), None)
    } else {
        (
            format!("DIFFERS {path}"),
            Some(format!("{path} is not what the film and encoders make")),
        )
    }
}

/// The word's floor.
///
/// # Errors
/// When the worst frame is under [`CONTRAST`].
pub fn contrast_holds(half: &str, worst: f64) -> Result<(), Fail> {
    if worst < CONTRAST {
        return Err(format!(
            "{half}: the word's contrast falls to {} to 1",
            py::repr_float(worst)
        ));
    }
    Ok(())
}

/// WCAG 2.3.1's two verdicts on the SVG's frames.
///
/// # Errors
/// When either is not `passes`, the report printed as Python prints it.
pub fn census_holds(half: &str, c: &Value) -> Result<(), Fail> {
    if c.get("outcome")?.text()? != "passes" || c.get("strict_outcome")?.text()? != "passes" {
        return Err(format!("{half}: the SVG's census fails: {}", repr(c)));
    }
    Ok(())
}

/// A film's report names its half and the canvas.
///
/// # Errors
/// "{half}: {what} is {w}x{h} of {its half}".
pub fn size_half(m: &Value, half: &str, what: &str) -> Result<(), Fail> {
    let (w, h) = (m.get("width")?, m.get("height")?);
    let mh = m.get("half")?;
    #[allow(clippy::cast_precision_loss, reason = "the canvas's sides")]
    if (w.num()?, h.num()?) != (W as f64, H as f64) || mh.text()? != half {
        return Err(format!(
            "{half}: {what} is {}x{} of {}",
            w.py(),
            h.py(),
            mh.py()
        ));
    }
    Ok(())
}

/// A film's report lists a delay for each frame.
///
/// # Errors
/// "{half}: {n} frames and {k} delays".
pub fn frames_match(m: &Value, half: &str) -> Result<(), Fail> {
    let frames = m.get("frames")?;
    let delays = m.get("delays_cs")?.nums()?;
    #[allow(clippy::cast_precision_loss, reason = "a frame count")]
    if frames.num()? != delays.len() as f64 {
        return Err(format!(
            "{half}: {} frames and {} delays",
            frames.py(),
            delays.len()
        ));
    }
    Ok(())
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `base64.b64encode`.
#[must_use]
pub fn b64encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                out.push(char::from(B64[((n >> (18 - 6 * k)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `base64.b64decode` of what [`b64encode`] writes.
///
/// # Errors
/// A character outside the alphabet, or a length not a multiple of 4.
pub fn b64decode(text: &str) -> Result<Vec<u8>, Fail> {
    let b = text.as_bytes();
    if !b.len().is_multiple_of(4) {
        return Err("Incorrect padding".into());
    }
    let val = |c: u8| -> Result<u32, Fail> {
        B64.iter()
            .position(|&a| a == c)
            .map(|v| v as u32)
            .ok_or_else(|| format!("a base64 character {c:#x}"))
    };
    let mut out = Vec::with_capacity(b.len() / 4 * 3);
    for q in b.as_chunks::<4>().0 {
        let pad = q.iter().rev().take_while(|&&c| c == b'=').count();
        let mut n = 0u32;
        for &c in &q[..4 - pad] {
            n = (n << 6) | val(c)?;
        }
        n <<= 6 * pad;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Ok(out)
}

fn secs(cs: u32) -> String {
    format!("{}.{:02}s", cs / 100, cs % 100)
}

fn starts(delays: &[u32]) -> Vec<u32> {
    let mut s = vec![0u32];
    for cs in delays {
        s.push(s[s.len() - 1] + cs);
    }
    s
}

/// **A proportion of the loop's `total` hundredths at `cs`**, as a keyTimes
/// value.
#[must_use]
pub fn key_time(cs: u32, total: u32) -> String {
    format!("{:.5}", f64::from(cs) / f64::from(total))
}

/// **A frame's visibility over the loop, discrete**
/// (`[[source: smil-animation]]`, whose first discrete time must be 0, and
/// whose time "defines when the value is set"): the values and keyTimes that
/// show it from `on` to `off` of `total` hundredths.
fn shown(on: u32, off: u32, total: u32) -> (&'static str, String) {
    let (a, b) = (key_time(on, total), key_time(off, total));
    match (on, off == total) {
        (0, true) => ("visible", "0".to_string()),
        (0, false) => ("visible;hidden", format!("0;{b}")),
        (_, true) => ("hidden;visible", format!("0;{a}")),
        (_, false) => ("hidden;visible;hidden", format!("0;{a};{b}")),
    }
}

/// **The SVG**: `still`, the bare logo, as a PNG, and each frame that differs
/// from it over it as a WebP of the box it changes and two pixels round, its
/// unchanged pixels clear, shown from its start to the next frame's by a
/// discrete SMIL animation repeating over the film's length.
///
/// # Errors
/// The WebP encoder's refusal.
pub fn svg(frames: &[Vec<u8>], still: &[u8], delays: &[u32]) -> Result<Vec<u8>, Fail> {
    let base = still;
    let png = png::encode(&Image {
        w: W,
        h: H,
        mode: Mode::Rgb,
        px: base.to_vec(),
    });
    let st = starts(delays);
    let total = st[st.len() - 1];
    let mut out = vec![
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{W}\" height=\"{H}\" viewBox=\"0 0 {W} {H}\">"
        ),
        "<title>baud</title>".to_string(),
        format!(
            "<image width=\"{W}\" height=\"{H}\" href=\"data:image/png;base64,{}\"/>",
            b64encode(&png)
        ),
        "<g>".to_string(),
    ];
    for (k, f) in frames.iter().enumerate() {
        let changed: Vec<bool> = f
            .as_chunks::<3>()
            .0
            .iter()
            .zip(base.as_chunks::<3>().0)
            .map(|(a, b)| a != b)
            .collect();
        let (mut xmin, mut ymin, mut xmax, mut ymax) = (usize::MAX, usize::MAX, 0, 0);
        for (i, _) in changed.iter().enumerate().filter(|(_, c)| **c) {
            let (x, y) = (i % W, i / W);
            xmin = xmin.min(x);
            ymin = ymin.min(y);
            xmax = xmax.max(x);
            ymax = ymax.max(y);
        }
        if xmin == usize::MAX {
            continue;
        }
        let (x0, y0) = (xmin.saturating_sub(2), ymin.saturating_sub(2));
        let (x1, y1) = ((xmax + 3).min(W), (ymax + 3).min(H));
        let (cw, ch) = (x1 - x0, y1 - y0);
        let mut rgba = Vec::with_capacity(cw * ch * 4);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * W + x;
                rgba.extend_from_slice(&f[i * 3..i * 3 + 3]);
                rgba.push(if changed[i] { 255 } else { 0 });
            }
        }
        let settings = webp::Settings {
            quality: WEBP_QUALITY,
            alpha_quality: 100.0,
            method: 6,
        };
        let data = webp::encode(&rgba, cw, ch, settings)?;
        let (values, keys) = shown(st[k], st[k + 1], total);
        out.push(format!(
            "<image x=\"{x0}\" y=\"{y0}\" width=\"{cw}\" height=\"{ch}\" visibility=\"hidden\" \
             href=\"data:image/webp;base64,{}\"><animate attributeName=\"visibility\" \
             calcMode=\"discrete\" values=\"{values}\" keyTimes=\"{keys}\" dur=\"{}\" \
             repeatCount=\"indefinite\"/></image>",
            b64encode(&data),
            secs(total)
        ));
    }
    out.push("</g>".into());
    out.push("</svg>".into());
    out.push(String::new());
    Ok(out.join("\n").into_bytes())
}

/// Digits at `s[i..]`, and where they end.
fn digits(s: &str, i: usize) -> Option<(u32, usize)> {
    let n = s[i..].bytes().take_while(u8::is_ascii_digit).count();
    if n == 0 {
        return None;
    }
    Some((s[i..i + n].parse().ok()?, i + n))
}

fn lit(s: &str, i: usize, l: &str) -> Option<usize> {
    s[i..].starts_with(l).then_some(i + l.len())
}

/// `(\d+)\.(\d\d)s` at `i`: the hundredths.
fn time(s: &str, i: usize) -> Option<(u32, usize)> {
    let (whole, i) = digits(s, i)?;
    let i = lit(s, i, ".")?;
    let b = s.as_bytes();
    if b.get(i..i + 2)?.iter().all(u8::is_ascii_digit) && lit(s, i + 2, "s").is_some() {
        let cs: u32 = s[i..i + 2].parse().ok()?;
        Some((whole * 100 + cs, i + 3))
    } else {
        None
    }
}

/// The hundredths of a keyTimes value of a loop of `total`.
fn cs_of(key: &str, total: u32) -> Option<u32> {
    let k: f64 = key.parse().ok()?;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a proportion of a whole number of hundredths"
    )]
    Some((k * f64::from(total)).round() as u32)
}

/// One frame image at `i`: x, y, the WebP's base64, begin and end in
/// hundredths, and where it ends. `total` is the film's length, which its
/// animation must repeat over.
fn frame_at(s: &str, i: usize, total: u32) -> Option<(usize, usize, &str, u32, u32, usize)> {
    let i = lit(s, i, "<image x=\"")?;
    let (x, i) = digits(s, i)?;
    let i = lit(s, i, "\" y=\"")?;
    let (y, i) = digits(s, i)?;
    let i = lit(s, i, "\" width=\"")?;
    let (_, i) = digits(s, i)?;
    let i = lit(s, i, "\" height=\"")?;
    let (_, i) = digits(s, i)?;
    let i = lit(
        s,
        i,
        "\" visibility=\"hidden\" href=\"data:image/webp;base64,",
    )?;
    let n = s[i..].bytes().take_while(|&c| c != b'"').count();
    if n == 0 {
        return None;
    }
    let data = &s[i..i + n];
    let i = lit(
        s,
        i + n,
        "\"><animate attributeName=\"visibility\" calcMode=\"discrete\" values=\"",
    )?;
    let v = s[i..].bytes().take_while(|&c| c != b'"').count();
    let values = &s[i..i + v];
    let i = lit(s, i + v, "\" keyTimes=\"")?;
    let k = s[i..].bytes().take_while(|&c| c != b'"').count();
    let keys: Vec<&str> = s[i..i + k].split(';').collect();
    let i = lit(s, i + k, "\" dur=\"")?;
    let (dur, i) = time(s, i)?;
    let i = lit(s, i, "\" repeatCount=\"indefinite\"/></image>")?;
    if dur != total || keys.first() != Some(&"0") {
        return None;
    }
    let (begin, end) = match (values, keys.as_slice()) {
        ("visible", [_]) => (0, total),
        ("visible;hidden", [_, b]) => (0, cs_of(b, total)?),
        ("hidden;visible", [_, a]) => (cs_of(a, total)?, total),
        ("hidden;visible;hidden", [_, a, b]) => (cs_of(a, total)?, cs_of(b, total)?),
        _ => return None,
    };
    Some((x as usize, y as usize, data, begin, end, i))
}

/// The base image's base64, the first `BASE` pattern's.
fn base_at(s: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(k) = s[from..].find("<image width=\"") {
        let i = from + k;
        let parse = || {
            let j = lit(s, i, "<image width=\"")?;
            let (_, j) = digits(s, j)?;
            let j = lit(s, j, "\" height=\"")?;
            let (_, j) = digits(s, j)?;
            let j = lit(s, j, "\" href=\"data:image/png;base64,")?;
            let n = s[j..].bytes().take_while(|&c| c != b'"').count();
            (n > 0 && lit(s, j + n, "\"/>").is_some()).then(|| &s[j..j + n])
        };
        if let Some(d) = parse() {
            return Some(d);
        }
        from = i + 1;
    }
    None
}

/// **The SVG's picture at each of the film's frame times as a browser
/// composes it**: the base, and over it the frame image shown then, its
/// colour times its alpha over 255 plus the ground's times the rest, in
/// `f64`, rounded half to even, as the driver's NumPy did.
///
/// # Errors
/// "the SVG's frame at {at} cs ends at {end}, not {next}", "the SVG
/// shows frames at no frame's start: [...]", a frame image that does not
/// repeat over the film's length, and an undecodable image.
pub fn svg_frames(b: &[u8], delays: &[u32]) -> Result<Vec<Vec<u8>>, Fail> {
    let text = std::str::from_utf8(b).map_err(|e| e.to_string())?;
    let base = base_at(text).ok_or("the SVG has no base image")?;
    let ground = png::decode(&b64decode(base)?)?;
    let ground: Vec<f64> = match ground.mode {
        Mode::Rgb => ground.px.iter().map(|&v| f64::from(v)).collect(),
        Mode::Rgba => ground
            .px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| p[..3].iter().map(|&v| f64::from(v)))
            .collect(),
    };
    let total: u32 = delays.iter().sum();
    let mut shown: Vec<(u32, Shown<'_>)> = Vec::new();
    let mut i = 0;
    while let Some(k) = text[i..].find("<image x=\"") {
        let at = i + k;
        if let Some((x, y, data, begin, end, next)) = frame_at(text, at, total) {
            // A dict: a later frame at the same start replaces the earlier.
            shown.retain(|(b, _)| *b != begin);
            shown.push((begin, (x, y, data, end)));
            i = next;
        } else {
            return Err(format!(
                "the SVG's frame image at byte {at} does not repeat over the film's {total} cs"
            ));
        }
    }
    let mut out = Vec::with_capacity(delays.len());
    let mut at = 0u32;
    for &cs in delays {
        let mut f = ground.clone();
        if let Some(pos) = shown.iter().position(|(b, _)| *b == at) {
            let (_, (x, y, data, end)) = shown.remove(pos);
            if end != at + cs {
                return Err(format!(
                    "the SVG's frame at {at} cs ends at {end}, not {}",
                    at + cs
                ));
            }
            let (ww, hh, rgba) = webp::decode(&b64decode(data)?)?;
            for yy in 0..hh {
                for xx in 0..ww {
                    let s = &rgba[(yy * ww + xx) * 4..(yy * ww + xx) * 4 + 4];
                    let a = f64::from(s[3]) / 255.0;
                    let d = ((y + yy) * W + x + xx) * 3;
                    for c in 0..3 {
                        f[d + c] = f64::from(s[c]) * a + f[d + c] * (1.0 - a);
                    }
                }
            }
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "`astype(numpy.uint8)` of values in 0..=255"
        )]
        out.push(f.iter().map(|v| v.round_ties_even() as u8).collect());
        at += cs;
    }
    if !shown.is_empty() {
        let mut left: Vec<u32> = shown.iter().map(|(b, _)| *b).collect();
        left.sort_unstable();
        let left: Vec<String> = left.iter().map(u32::to_string).collect();
        return Err(format!(
            "the SVG shows frames at no frame's start: [{}]",
            left.join(", ")
        ));
    }
    Ok(out)
}

/// The GIF's frames as RGB canvases with their delays in hundredths,
/// whether it loops, and its size. The frames, delays and size are baud's
/// own decoder's, which composed both committed GIFs' 434 frames and delays
/// as Pillow 12.3.0 did, run as baud-bench's `gif_frames` example because
/// this workspace links no crate of the repository's; whether it loops is
/// [`gif::loops`], the rule Pillow reads it by.
///
/// # Errors
/// A GIF baud's decoder refuses, named on stderr, or a run that did not
/// write the frames it reports.
pub fn decoded(root: &Path, b: &[u8], d: &Path) -> Result<Decoded, Fail> {
    let (file, raw) = (d.join("decoded.gif"), d.join("decoded.rgb"));
    std::fs::write(&file, b).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--quiet", "--locked"])
        .args(FAST)
        .args(["-p", "baud-bench", "--example", "gif_frames", "--"])
        .arg(&file)
        .arg(&raw)
        .current_dir(root);
    let said = json::parse(&String::from_utf8_lossy(&run(&mut cmd, "gif_frames")?))?;
    let bytes = read(&raw)?;
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_file(&raw);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "whole numbers `gif_frames` printed"
    )]
    let (w, h, delays) = (
        said.get("width")?.num()? as usize,
        said.get("height")?.num()? as usize,
        said.get("delays")?
            .nums()?
            .iter()
            .map(|&v| v as u32)
            .collect::<Vec<u32>>(),
    );
    let frame = w * h * 3;
    if bytes.len() != delays.len() * frame {
        return Err(format!(
            "gif_frames wrote {} bytes for {} frames of {w} by {h}",
            bytes.len(),
            delays.len()
        ));
    }
    let frames = bytes
        .chunks_exact(frame)
        .map(<[u8]>::to_vec)
        .zip(delays)
        .collect();
    Ok((frames, gif::loops(b), (w, h)))
}

// ---------------------------------------------------------------- the run

/// The repository the tool is built in.
#[must_use]
pub fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

pub(crate) fn run(cmd: &mut Command, what: &str) -> Result<Vec<u8>, Fail> {
    let out = cmd
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{what} did not run: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed: {}", out.status));
    }
    Ok(out.stdout)
}

/// ffmpeg's `bin`, `ffmpeg-headless.bin` from gifski's nixpkgs, its
/// version checked.
///
/// # Errors
/// When nix cannot build it, or it is not [`FFMPEG`].
pub fn ffmpeg() -> Result<PathBuf, Fail> {
    let out = run(
        Command::new("nix").args([
            "build",
            "--no-link",
            "--print-out-paths",
            &format!("{GIFSKI_NIXPKGS}#ffmpeg-headless.bin"),
        ]),
        "nix build ffmpeg",
    )?;
    let bin = PathBuf::from(String::from_utf8_lossy(&out).trim()).join("bin");
    let v = run(
        Command::new(bin.join("ffmpeg")).args(["-hide_banner", "-version"]),
        "ffmpeg -version",
    )?;
    let v = String::from_utf8_lossy(&v);
    let first = v.lines().next().unwrap_or("");
    if !first.starts_with(FFMPEG) {
        return Err(format!(
            "the tools are not the pinned ones: ffmpeg is {first:?}, not {FFMPEG:?}"
        ));
    }
    Ok(bin)
}

/// **One half's MP4**: every frame held its hundredths at [`MP4_FPS`],
/// piped to ffmpeg as raw RGB and encoded with [`MP4`] into `d/name`.
pub(crate) fn mp4(
    ff: &Path,
    d: &Path,
    frames: &[Vec<u8>],
    delays: &[u32],
    name: &str,
) -> Result<Vec<u8>, Fail> {
    use std::io::Write;
    let out = d.join(name);
    let mut cmd = Command::new(ff.join("ffmpeg"));
    cmd.args(["-hide_banner", "-v", "error", "-y", "-f", "rawvideo"])
        .args(["-pix_fmt", "rgb24", "-s", &format!("{W}x{H}")])
        .args(["-r", &MP4_FPS.to_string(), "-i", "-"])
        .args(MP4)
        .arg(&out)
        .stdin(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("ffmpeg did not run: {e}"))?;
    let mut fed = Ok(());
    if let Some(mut pipe) = child.stdin.take() {
        'all: for (f, &cs) in frames.iter().zip(delays) {
            for _ in 0..cs {
                if let Err(e) = pipe.write_all(f) {
                    fed = Err(format!("ffmpeg's input: {e}"));
                    break 'all;
                }
            }
        }
    }
    let st = child.wait().map_err(|e| format!("ffmpeg: {e}"))?;
    fed?;
    if !st.success() {
        return Err(format!("ffmpeg failed: {st}"));
    }
    read(&out)
}

/// **The MP4 holds the film**: as many frames as its hundredths, playing
/// as long, by ffprobe.
pub(crate) fn mp4_holds(ff: &Path, half: &str, path: &Path, total: u32) -> Result<String, Fail> {
    let out = run(
        Command::new(ff.join("ffprobe"))
            .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
            .args([
                "-show_entries",
                "stream=nb_read_packets,duration,start_time",
            ])
            .args(["-of", "csv=p=0"])
            .arg(path),
        "ffprobe",
    )?;
    let said = String::from_utf8_lossy(&out).trim().to_string();
    let f: Vec<&str> = said.split(',').collect();
    let num = |k: usize| f.get(k).and_then(|v| v.trim().parse::<f64>().ok());
    let (start, lasts, frames) = (num(0), num(1), num(2));
    let want = f64::from(total) / f64::from(MP4_FPS);
    let held = matches!((start, lasts, frames),
        (Some(s), Some(l), Some(n)) if s.abs() < 1e-3 && (l - want).abs() < 1e-3
            && (n - f64::from(total)).abs() < 0.5);
    if !held {
        return Err(format!(
            "{half}: the MP4 reads {said:?} (start, length, frames), not 0, {want}, {total}"
        ));
    }
    Ok(said)
}

/// gifski, built from its pinned nixpkgs, and its version checked.
///
/// # Errors
/// When nix cannot build it, or it is not [`GIFSKI`].
pub fn gifski() -> Result<PathBuf, Fail> {
    let out = run(
        Command::new("nix").args([
            "build",
            "--no-link",
            "--print-out-paths",
            &format!("{GIFSKI_NIXPKGS}#gifski"),
        ]),
        "nix build gifski",
    )?;
    let bin = PathBuf::from(String::from_utf8_lossy(&out).trim()).join("bin/gifski");
    let v = run(Command::new(&bin).arg("--version"), "gifski --version")?;
    let v = String::from_utf8_lossy(&v).trim().to_string();
    if v != GIFSKI {
        return Err(format!(
            "the tools are not the pinned ones: gifski is {v:?}, not {GIFSKI:?}"
        ));
    }
    Ok(bin)
}

/// The pinned libraries: zlib and libwebp.
///
/// # Errors
/// When either is another version.
pub fn libraries() -> Result<(), Fail> {
    let z = crate::zlib::version();
    if z != crate::zlib::ZLIB {
        return Err(format!(
            "the zlib linked is {z}, not the pinned {}",
            crate::zlib::ZLIB
        ));
    }
    if !webp::pinned() {
        return Err(format!(
            "the libwebp linked is {:?}, not the pinned {}",
            webp::versions(),
            webp::LIBWEBP
        ));
    }
    Ok(())
}

pub(crate) fn cargo_test(
    root: &Path,
    args: &[&str],
    env: &[(&str, &Path)],
    what: &str,
) -> Result<(), Fail> {
    let mut cmd = Command::new("cargo");
    cmd.args(["test", "--quiet", "--locked", "-p", "baud", "--bin", "baud"])
        .args(FAST)
        .args(args)
        .current_dir(root)
        .stdout(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    run(&mut cmd, what).map(|_| ())
}

pub(crate) fn read(p: &Path) -> Result<Vec<u8>, Fail> {
    std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))
}

pub(crate) fn json_file(p: &Path) -> Result<Value, Fail> {
    let b = read(p)?;
    json::parse(&String::from_utf8_lossy(&b))
}

/// `repr()` of what `json.load` makes of `v`: a dict's keys and values
/// as Python prints them.
#[must_use]
pub fn repr(v: &Value) -> String {
    match v {
        Value::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}: {}", py::repr_str(k), repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::List(l) => format!("[{}]", l.iter().map(repr).collect::<Vec<_>>().join(", ")),
        other => other.repr(),
    }
}

/// A PNG of an RGB canvas for gifski to read: filter None on every row and
/// zlib at level 1, the speed Pillow's `compress_level=1` asked for; gifski
/// reads the pixels, and nothing keeps the bytes.
#[must_use]
pub fn quick_png(px: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(H * (W * 3 + 1));
    for row in px.as_chunks::<{ W * 3 }>().0 {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = crate::zlib::Deflate::new(1, 0);
    let mut buf = vec![0u8; raw.len() + raw.len() / 1000 + 64];
    z.set_in(&raw);
    z.set_out(&mut buf);
    let end = z.deflate(crate::zlib::Z_FINISH);
    assert_eq!(
        end,
        crate::zlib::Z_STREAM_END,
        "one buffer holds the stream"
    );
    let n = buf.len() - z.avail_out() as usize;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend(u32::try_from(data.len()).expect("a chunk").to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crate::zlib::crc(crate::zlib::crc(0, kind), data);
        out.extend(crc.to_be_bytes());
    };
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(u32::try_from(W).expect("a width").to_be_bytes());
    ihdr.extend(u32::try_from(H).expect("a height").to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &buf[..n]);
    chunk(b"IEND", b"");
    out
}

pub(crate) fn canvas(p: &Path) -> Result<Vec<u8>, Fail> {
    let raw = read(p)?;
    if raw.len() != W * H * 3 {
        return Err(format!("{} is not a canvas", p.display()));
    }
    Ok(raw)
}

/// The bare logo, the block shown, and the blink's phases.
pub(crate) fn bare(root: &Path, half: &str, d: &Path) -> Result<(Vec<u8>, Value), Fail> {
    cargo_test(
        root,
        &[
            "logo_film::film_the_readme_logo_bare",
            "--",
            "--ignored",
            "--exact",
        ],
        &[("BAUD_LOGO_OUT", d), ("BAUD_LOGO_HALF", Path::new(half))],
        "the bare film",
    )?;
    let m = json_file(&d.join("bare.json"))?;
    size_half(&m, half, "the bare canvas")?;
    Ok((canvas(&d.join("bare.rgb"))?, m))
}

fn f32s(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// Hold today's dark bare logo to the walls the path was chosen against,
/// by [`lockup_faults`]; the walls' geometry directory, which the path is
/// chosen from, and today's, which the film's slabs are cut from.
pub(crate) fn walls_hold(
    root: &Path,
    today: &[u8],
    tmp: &Path,
) -> Result<(PathBuf, PathBuf), Fail> {
    let data = read(&root.join(WALLS))?;
    walls_pinned(&data)?;
    let walls = png::decode(&data)?;
    let walls: Vec<u8> = match walls.mode {
        Mode::Rgb => walls.px,
        Mode::Rgba => walls
            .px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect(),
    };
    let mut sdf = Vec::new();
    let mut dirs = Vec::new();
    for (name, img) in [("walls", &walls), ("today", &today.to_vec())] {
        let g = tmp.join(format!("geom-{name}"));
        std::fs::create_dir_all(&g).map_err(|e| format!("{}: {e}", g.display()))?;
        let fields = geometry::fields(img);
        for (file, b) in geometry::files(&fields) {
            std::fs::write(g.join(file), &b).map_err(|e| format!("{file}: {e}"))?;
            if file == "sdf.f32" {
                sdf.push(f32s(&b));
            }
        }
        dirs.push(g);
    }
    let (off, bad) = lockup_faults(&walls, today, &sdf[0], &sdf[1]);
    println!(
        "walls: today's word is {} off them, at most {}; {} of the mark's glyphs off where it moved to",
        off.repr(),
        WALLS_OFF.repr(),
        bad.len()
    );
    for line in &bad {
        println!("walls: {line}");
    }
    if off.past(&WALLS_OFF) || !bad.is_empty() {
        return Err(
            "the lockup has moved off the walls the plasma's path was chosen against: choose the path again"
                .into(),
        );
    }
    let today = dirs.swap_remove(1);
    Ok((dirs.swap_remove(0), today))
}

/// The generator's strands for each frame, and its report.
fn strands(geom: &Path, half: &str, blink: &Value, d: &Path) -> Result<(PathBuf, Value), Fail> {
    let out = d.join("strands.txt");
    let args = [
        geom.display().to_string(),
        half.to_string(),
        blink.get("on_ms")?.py(),
        blink.get("off_ms")?.py(),
        out.display().to_string(),
    ];
    let report = plasma::run(&args).map_err(|e| match e {
        plasma::Stop::Exit(m) => m,
        plasma::Stop::Assert(m) => format!("AssertionError: {m}"),
    })?;
    Ok((out, json::parse(&report)?))
}

/// Film one half into `d`; its report.
fn film(root: &Path, half: &str, d: &Path, prims: &Path, geom: &Path) -> Result<Value, Fail> {
    cargo_test(
        root,
        &[
            "logo_film::film_the_readme_logo",
            "--",
            "--ignored",
            "--exact",
        ],
        &[
            ("BAUD_LOGO_OUT", d),
            ("BAUD_LOGO_HALF", Path::new(half)),
            ("BAUD_LOGO_PRIMS", prims),
            ("BAUD_LOGO_GEOM", geom),
        ],
        "the film",
    )?;
    let m = json_file(&d.join("film.json"))?;
    size_half(&m, half, "the film")?;
    frames_match(&m, half)?;
    Ok(m)
}

pub(crate) fn delays_of(m: &Value) -> Result<Vec<u32>, Fail> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "hundredths"
    )]
    Ok(m.get("delays_cs")?
        .nums()?
        .iter()
        .map(|&v| v as u32)
        .collect())
}

/// gifski's GIF of the frames, repeating for ever, then [`gif::rewrite`].
pub(crate) fn encode(
    gifski: &Path,
    d: &Path,
    delays: &[u32],
    names: &[PathBuf],
    h: &Half,
) -> Result<Vec<u8>, Fail> {
    let out = d.join("logo.gif");
    let fixed = format!("{:02X}{:02X}{:02X}", h.page[0], h.page[1], h.page[2]);
    let mut cmd = Command::new(gifski);
    cmd.args([
        "--quiet",
        "--fps",
        "50",
        "--quality",
        &QUALITY.to_string(),
        "--width",
        &W.to_string(),
    ])
    .args([
        "--motion-quality",
        &MOTION.to_string(),
        "--lossy-quality",
        &LOSSY.to_string(),
    ])
    .args(["--fixed-color", &fixed, "--repeat", "0", "-o"])
    .arg(&out)
    .args(names);
    run(&mut cmd, "gifski")?;
    gif::rewrite(&read(&out)?, delays, h.page, h.name)
}

/// The census of `frames`, each held its hundredths at 100 samples a
/// second and 2 pixels a point, fed twice over, so the frames either side of
/// the join are counted as the repeat shows them: `flash_census`'s report.
pub(crate) fn census(
    root: &Path,
    frames: &[Vec<u8>],
    delays: &[u32],
    d: &Path,
    name: &str,
) -> Result<Value, Fail> {
    let (raw, reps) = (
        d.join(format!("{name}.rgb")),
        d.join(format!("{name}.repeats")),
    );
    let w = |p: &Path, b: &[u8]| std::fs::write(p, b).map_err(|e| format!("{}: {e}", p.display()));
    w(&raw, &frames.concat())?;
    let lines: String = delays.iter().map(|cs| format!("{cs}\n")).collect();
    w(&reps, lines.as_bytes())?;
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--quiet", "--locked"])
        .args(FAST)
        .args(["-p", "baud-bench", "--example", "flash_census", "--"])
        .arg(&raw)
        .arg(&reps)
        .args([&W.to_string(), &H.to_string(), "2", "0", "100", "2"])
        .current_dir(root);
    let out = run(&mut cmd, "flash_census")?;
    let _ = std::fs::remove_file(&raw);
    json::parse(&String::from_utf8_lossy(&out))
}

/// One half's GIF and SVG bytes, checked, and the lines that report them.
fn make(
    root: &Path,
    (gifski, ff): (&Path, Option<&Path>),
    h: &Half,
    tmp: &Path,
    (walls, today): (&Path, &Path),
    blink: &Value,
) -> Result<(Made, Option<Vec<u8>>), Fail> {
    let half = h.name;
    let d = tmp.join(half);
    let (prims, report) = strands(walls, half, blink, &d)?;
    let m = film(root, half, &d, &prims, today)?;
    let delays = delays_of(&m)?;
    let frames = snapped(&read(&d.join("frames.rgb"))?, delays.len(), h.page)?;
    let mut still = canvas(&d.join("still.rgb"))?;
    snap(&mut still, h.page);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "`int(v)` of pixel coordinates"
    )]
    let block: Vec<usize> = m
        .get("block")?
        .nums()?
        .iter()
        .map(|&v| v as usize)
        .collect();
    let round = 2usize;
    let block = [
        block[0].saturating_sub(round),
        block[1].saturating_sub(round),
        block[2] + round,
        block[3] + round,
    ];
    let (least, wrap, busiest) = wrap_holds(half, &frames, &still, block)?;
    let mut names = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        let p = d.join(format!("{i:05}.png"));
        std::fs::write(&p, quick_png(f)).map_err(|e| format!("{}: {e}", p.display()))?;
        names.push(p);
    }
    let b = encode(gifski, &d, &delays, &names, h)?;
    if encode(gifski, &d, &delays, &names, h)? != b {
        return Err(format!(
            "{half}: gifski wrote different bytes from the same frames"
        ));
    }
    let (got, loops, size) = decoded(root, &b, &d)?;
    let total: u32 = delays.iter().sum();
    let movie = match ff {
        Some(ff) => {
            let m = mp4(ff, &d, &frames, &delays, "logo.mp4")?;
            if mp4(ff, &d, &frames, &delays, "again.mp4")? != m {
                return Err(format!(
                    "{half}: ffmpeg wrote different MP4s from the same frames"
                ));
            }
            let said = mp4_holds(ff, half, &d.join("logo.mp4"), total)?;
            println!(
                "{half}: the MP4, {} bytes, twice the same, reads {said}",
                m.len()
            );
            Some(m)
        }
        None => None,
    };
    let lasts: u32 = got.iter().map(|(_, cs)| cs).sum();
    gif_holds(half, lasts, total, loops, size, b.len())?;
    let s = svg(&frames, &still, &delays)?;
    if svg(&frames, &still, &delays)? != s {
        return Err(format!(
            "{half}: the SVG's encoder wrote different bytes from the same frames"
        ));
    }
    let seen = svg_frames(&s, &delays)?;
    let (share, worst_levels) = haze(&frames, &got, h.page, &delays);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "`int(v)` of pixel coordinates"
    )]
    let word: Vec<usize> = m.get("word")?.nums()?.iter().map(|&v| v as usize).collect();
    let word = [word[0], word[1], word[2], word[3]];
    let all: Vec<&[u8]> = got
        .iter()
        .map(|(f, _)| f.as_slice())
        .chain(seen.iter().map(Vec::as_slice))
        .collect();
    let (worst, median) = contrast(&all, &still, word, h.page, h.word);
    contrast_holds(half, worst)?;
    let c = census(root, &seen, &delays, &d, "svg")?;
    census_holds(half, &c)?;
    let (o, so) = (c.get("outcome")?.text()?, c.get("strict_outcome")?.text()?);
    let shows = s.windows(9).filter(|w| w == b"<animate ").count();
    let p = |k: &str| -> Result<String, Fail> { Ok(m.get(k)?.py()) };
    let r = |k: &str| -> Result<String, Fail> { Ok(report.get(k)?.py()) };
    let cp = |k: &str| -> Result<String, Fail> { Ok(c.get(k)?.py()) };
    let lines = vec![
        format!(
            "{}: {} bytes, {} frames on {}, repeating every {total} cs ({} cycles of {} ms shown and {} hidden)",
            h.gif,
            b.len(),
            got.len(),
            p("page")?,
            p("cycles")?,
            p("on_ms")?,
            p("off_ms")?
        ),
        format!(
            "{}: {} bytes, {shows} frame images over the logo, at the GIF's times",
            h.svg,
            s.len()
        ),
        format!(
            "  the plasma: seed {}, {} events in the loop; {} layers at most a frame; off the block, at least {least} pixels a frame away from the bare logo, the wrap moves {wrap} and the busiest step {busiest}",
            r("seed")?,
            r("events")?,
            p("most_layers")?
        ),
        format!(
            "  haze: {}% of the page's pixels off it in the GIF, by {worst_levels} levels at worst",
            py::repr_float(share)
        ),
        format!(
            "  the word's contrast: worst {} to 1, median {}",
            py::repr_float(worst),
            py::repr_float(median)
        ),
        format!(
            "  the SVG's census: general {}, red {} flashes at a pixel in a second; area {}, strict {} of {}: {o}, strict {so}",
            cp("general_flashes")?,
            cp("red_flashes")?,
            cp("area")?,
            cp("strict")?,
            cp("threshold")?
        ),
    ];
    Ok(((b, s, lines), movie))
}

/// The tests that hold the files, in the tree or in `out`, their output
/// shown.
fn tests(root: &Path, out: Option<&Path>) -> Result<(), Fail> {
    for t in TESTS {
        let mut cmd = Command::new("cargo");
        cmd.args(["test", "--quiet", "--locked"]).args(FAST).args(t);
        if let Some(d) = out {
            cmd.env("BAUD_README_FILES", d);
        }
        if !t.contains(&"--") {
            cmd.arg("--");
        }
        cmd.args(["--nocapture", "--test-threads", "1"])
            .current_dir(root);
        // One pipe for both, so the lines come in the order they were
        // written.
        let (mut r, w) = std::io::pipe().map_err(|e| format!("a pipe: {e}"))?;
        let w2 = w.try_clone().map_err(|e| format!("a pipe: {e}"))?;
        cmd.stdout(w2).stderr(w);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cargo test did not run: {e}"))?;
        // The parent's copies of the write end go with the Command.
        drop(cmd);
        let mut bytes = Vec::new();
        r.read_to_end(&mut bytes)
            .map_err(|e| format!("cargo test's output: {e}"))?;
        let status = child.wait().map_err(|e| format!("cargo test: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        for line in py::splitlines(&text) {
            if line.starts_with("logo-") || line.starts_with("test ") {
                println!("{line}");
            }
        }
        if !status.success() {
            println!("{text}");
            return Err(format!("the test {} fails", t.join(" ")));
        }
    }
    Ok(())
}

/// `logo [--check | --out DIR]`: the run, its lines printed; the files that
/// differ.
///
/// # Errors
/// What stops the run before the files are compared or written.
pub fn main(
    root: &Path,
    files: &Files,
    keep: Option<&Path>,
    movie: Mp4,
) -> Result<Vec<String>, Fail> {
    if movie == Mp4::Yes && !matches!(files, Files::Out(_)) {
        return Err(
            "--mp4 needs --out DIR: the MP4s are not committed, and left in the \
                    tree they make the demo's --record refuse it as dirty"
                .into(),
        );
    }
    libraries()?;
    let out = match files {
        Files::Out(d) => {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
            Some(
                d.canonicalize()
                    .map_err(|e| format!("{}: {e}", d.display()))?,
            )
        }
        _ => None,
    };
    eprintln!(
        "tools: zlib {} and libwebp {} from nixpkgs {NIXPKGS_REV}, gifski from {GIFSKI_NIXPKGS}",
        crate::zlib::ZLIB,
        webp::LIBWEBP
    );
    let gifski = gifski()?;
    let ff = match movie {
        Mp4::Yes => {
            let b = ffmpeg()?;
            eprintln!("tools: {FFMPEG} from {GIFSKI_NIXPKGS}, {}", b.display());
            Some(b)
        }
        Mp4::No => None,
    };
    let tmp = keep.map_or_else(
        || std::env::temp_dir().join(format!("baud-pictures-logo-{}", std::process::id())),
        Path::to_path_buf,
    );
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&tmp);
    } else if std::fs::read_dir(&tmp).is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!("{} is not empty", tmp.display()));
    }
    let result: Result<Vec<String>, Fail> = (|| {
        let mut today = Vec::new();
        let mut blink = Vec::new();
        for h in &HALVES {
            let d = tmp.join(h.name);
            std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
            let (t, m) = bare(root, h.name, &d)?;
            today.push(t);
            blink.push(m);
        }
        let (walls, today) = walls_hold(root, &today[0], &tmp)?;
        let mut bad = Vec::new();
        for (h, blink) in HALVES.iter().zip(&blink) {
            let ((g, s, lines), movie) = make(
                root,
                (&gifski, ff.as_deref()),
                h,
                &tmp,
                (&walls, &today),
                blink,
            )?;
            if let Some(m) = movie {
                let at = root.join(h.gif).with_extension("mp4");
                let name = at.file_name().ok_or("a file's name")?.to_owned();
                let to = out.as_ref().map_or(at, |d| d.join(name));
                std::fs::write(&to, &m).map_err(|e| format!("{}: {e}", to.display()))?;
                println!("wrote {}", to.display());
            }
            for line in lines {
                println!("{line}");
            }
            for (path, b) in [(h.gif, g), (h.svg, s)] {
                let at = root.join(path);
                if *files == Files::Check {
                    let committed = std::fs::read(&at).ok();
                    let (line, refusal) = compared(path, committed.as_deref(), &b);
                    println!("{line}");
                    bad.extend(refusal);
                } else {
                    let name = Path::new(path).file_name().ok_or("a file's name")?;
                    let to = out.as_ref().map_or(at, |d| d.join(name));
                    std::fs::write(&to, &b).map_err(|e| format!("{}: {e}", to.display()))?;
                    println!("wrote {}", to.display());
                }
            }
        }
        Ok(bad)
    })();
    if keep.is_none() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let bad = result?;
    tests(root, out.as_deref())?;
    Ok(bad)
}
