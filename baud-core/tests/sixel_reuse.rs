//! **A Sixel's unpainted pixels are its own, never a freed image's.**
//!
//! A Sixel decodes into a span of the image arena, and a span is reused once
//! the image that held it is freed -- by text written over its cells, which is
//! how a previewer replaces one preview with the next. The decoder paints
//! only set bits, so every position the new image leaves unpainted (an unset
//! bit, and every row of a band past the last set bit) showed whatever the
//! freed image had left there.
//!
//! What such a position shows is the text's. `[[source: vt3xx-sixel]]` lists
//! two meanings for `P2` (192-203): "0 or 2 (default) | Pixel positions
//! specified as 0 are set to the current background color" and "1 | Pixel
//! positions specified as 0 remain at their current color".
//! `[[source: dec-std-070]]` 070-9 §7.2 (PDF p. 918) gives the second
//! parameter "0 device default action", "1 no action (no change to zero bit
//! value grid positions)" and "2 set to background color - zero bit value grid
//! positions are set to the background color". Under `P2 = 1` the position
//! keeps what the screen shows under the image, which a transparent image
//! pixel (`ColorWord::DEFAULT`) composites to (a reading); under `P2 = 0` or
//! `2` baud draws it transparent too, its counted, standing departure
//! (`sixel_p2_ignored`, note S). Neither text, under any `P2`, lets the
//! position show a different image that was erased before this one arrived.

use baud_core::*;
use baud_domain::{Chan, ImageBudget, ImageGen, ImageId, ImagePx};

mod support;

/// The cell size every test here hands the core: an overwrite frees an image
/// only once the core knows how many cells it spans. 17x34 is the cell the
/// recording below was answered with (`CSI 6 ; 34 ; 17 t`).
const CELL: (u16, u16) = (17, 34);

/// Feed `data` as reads cut at `cuts` (ascending, each inside `data`).
fn feed_cut(t: &mut Term, data: &[u8], cuts: &[usize]) {
    let mut at = 0;
    for &end in cuts.iter().chain(core::iter::once(&data.len())) {
        if end > at {
            t.write_at(BytePos::ZERO, &data[at..end]);
            t.feed(BytePos::ZERO, ReadLen::from_index(end - at));
            at = end;
        }
    }
}

/// The one placed image's width, height and pixels, after `data` is fed as
/// reads cut at `cuts` into a term provisioned with `budget`.
fn placed_after(
    data: &[u8],
    cuts: &[usize],
    budget: ImageBudget,
) -> (usize, usize, Vec<ColorWord>, usize) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(budget);
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(CELL.0), ImagePx::of(CELL.1));
    feed_cut(&mut t, data, cuts);
    let imgs = t.images();
    assert_eq!(imgs.images_refused, 0, "cuts {cuts:?}: nothing refused");
    let placed: Vec<_> = imgs.placements().map(|(_, p)| p.image).collect();
    assert_eq!(placed.len(), 1, "cuts {cuts:?}: the second image alone");
    let v = imgs.image(placed[0]).expect("live");
    (
        usize::from(v.width.raw()),
        usize::from(v.height.raw()),
        v.pixels.to_vec(),
        imgs.sixel_p2_ignored,
    )
}

fn red() -> ColorWord {
    ColorWord::rgb(
        Chan::from_offset(255, 0),
        Chan::from_offset(0, 0),
        Chan::from_offset(0, 0),
    )
}

fn green() -> ColorWord {
    ColorWord::rgb(
        Chan::from_offset(0, 0),
        Chan::from_offset(255, 0),
        Chan::from_offset(0, 0),
    )
}

/// A red `w`-wide Sixel of `bands` full bands, text over its cell, then a
/// green one of the same size that paints only its top device row. The arena
/// holds exactly one such image, so the second can only land in the span the
/// first was freed from, and lands at all only if the overwrite freed it.
fn reuse_program(p2: &str, bands: usize) -> (Vec<u8>, usize) {
    let w = 10;
    let mut d = Vec::new();
    // `P1 = 9` and a `1;1` raster are 1:1, so a band is six device rows.
    let full: Vec<String> = (0..bands).map(|_| format!("!{w}~")).collect();
    d.extend(format!("\x1bP9;{p2}q\"1;1;{w};{}#1;2;100;0;0#1", 6 * bands).as_bytes());
    d.extend(full.join("-").as_bytes());
    d.extend(b"\x1b\\");
    // The red image spans one 17x34 cell at (0,0); `XX` over it frees it.
    d.extend(b"\x1b[HXX\x1b[H");
    // Band one paints bit 0 only (`@`); every later band paints nothing (`?`).
    let mut top: Vec<String> = vec![format!("!{w}@")];
    top.extend((1..bands).map(|_| format!("!{w}?")));
    d.extend(format!("\x1bP9;{p2}q\"1;1;{w};{}#2;2;0;100;0#2", 6 * bands).as_bytes());
    d.extend(top.join("-").as_bytes());
    d.extend(b"\x1b\\");
    (d, w * 6 * bands)
}

/// What the text gives the second image: its top row green, every other
/// position unpainted.
fn want(w: usize, h: usize) -> Vec<ColorWord> {
    let mut v = vec![ColorWord::DEFAULT; w * h];
    v[..w].fill(green());
    v
}

/// Every two-read split, then one-byte reads: the image must be the text's at
/// every boundary, and the red one the arena held must show nowhere.
fn holds_at_every_split(p2: &str, bands: usize) {
    let (prog, area) = reuse_program(p2, bands);
    let mut cuts: Vec<Vec<usize>> = (1..prog.len()).map(|k| vec![k]).collect();
    cuts.push(Vec::new());
    cuts.push((1..prog.len()).collect());
    // Every split is tried and the wrong ones counted, so a failure says how
    // many boundaries show the freed image, not only the first.
    let mut wrong = Vec::new();
    for c in &cuts {
        // An arena of exactly one image's words.
        let (w, h, px, ignored) = placed_after(&prog, c, ImageBudget::of(area));
        assert_eq!((w, h), (10, 6 * bands), "P2 {p2}, cuts {c:?}: the extent");
        // The positive control on the P2 reading: `P2 = 1` is the text's
        // transparent, `0` is baud's counted departure, both images counted.
        let counted = if p2 == "1" { 0 } else { 2 };
        assert_eq!(ignored, counted, "P2 {p2}: the opaque-background count");
        if px != want(w, h) {
            let reds = px.iter().filter(|&&p| p == red()).count();
            wrong.push((c.len(), c.first().copied(), reds));
        }
    }
    assert!(
        wrong.is_empty(),
        "P2 {p2}, {bands} band(s): {} of {} feeds show the freed image \
         (reads cut, first cut, red pixels): {:?}",
        wrong.len(),
        cuts.len(),
        &wrong[..wrong.len().min(4)]
    );
}

/// **The constructed reproduction, one band.** A 10x6 red Sixel, freed by
/// text, then a 10x6 one painting only its top row green into the same span.
/// The text's image is a green row over five unpainted rows; before the span
/// was cleared it was a green row over five red ones, at every split.
#[test]
fn a_sixel_in_a_freed_span_shows_none_of_the_freed_image() {
    holds_at_every_split("1", 1);
}

/// **Two bands, so a clear that stopped at the first band is seen.** The
/// second band is all `?`: it widens the extent to twelve rows and sets no
/// bit, and every one of its rows must be unpainted.
#[test]
fn a_freed_span_is_cleared_past_the_first_band() {
    holds_at_every_split("1", 2);
}

/// **The default `P2`, which asks for the background, draws no freed image
/// either.** baud draws an unset bit transparent here, its counted departure
/// (note S), and the freed red image is no part of that.
#[test]
fn under_the_default_p2_a_freed_span_shows_none_of_the_freed_image() {
    holds_at_every_split("0", 2);
}

/// The committed yazi recording: yazi 26.5.6 walking down seven synthetic
/// files and back, thirteen previews, each cleared with spaces and the next
/// drawn at the same cell (`tests/recordings/README.md`).
const WALK: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/yazi-walk.raw"
));

/// The replay geometry: 200x60 and the 17x34 cell the capture was answered
/// with. The budget is the app's 4 MiB arena; no body is held since
/// 2026-10-14, and until then this held every body (the largest is 815,682
/// bytes).
const WALK_WS: WinSize = WinSize {
    ws_row: 60,
    ws_col: 200,
};

fn walk_budget() -> ImageBudget {
    ImageBudget::of(1 << 20)
}

type Image = (u16, u16, Vec<ColorWord>);

/// Every image stored over a feed of `data` in reads of `chunk`, in order. A
/// store bumps the store's generation, so after each read whose generation
/// moved, a placed image that was not placed after the last such read is a
/// new one. yazi clears a preview in the 442-1,465 bytes between one Sixel's
/// terminator and the next one's `ESC P`, so at reads of 4096 a store and its
/// free can share a read and that image is never seen placed (1 of 13 was
/// seen there); at reads of 1 and 7 all 13 are, and the count of stores
/// observed is asserted so a read size that hid one fails rather than passes.
fn stored(data: &[u8], chunk: usize) -> Vec<Image> {
    let mut owned = Owned::new(&WALK_WS).with_images(walk_budget());
    let mut t = Term::new(owned.storage(), &WALK_WS);
    t.set_cell_size(ImagePx::of(CELL.0), ImagePx::of(CELL.1));
    let mut epoch: ImageGen = t.images().generation();
    let mut prev: Vec<ImageId> = Vec::new();
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let n = chunk.min(data.len() - at);
        t.write_at(BytePos::ZERO, &data[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
        let imgs = t.images();
        if imgs.generation() == epoch {
            continue;
        }
        epoch = imgs.generation();
        let cur: Vec<ImageId> = imgs.placements().map(|(_, p)| p.image).collect();
        for id in &cur {
            if !prev.contains(id) {
                let v = imgs.image(*id).expect("placed means live");
                out.push((v.width.raw(), v.height.raw(), v.pixels.to_vec()));
            }
        }
        prev = cur;
    }
    assert_eq!(t.images().images_refused, 0, "chunk {chunk}: none refused");
    out
}

/// Each Sixel in `data`, `ESC P` through `ESC \`, decoded alone into a fresh
/// store: the oracle. A fresh arena holds no earlier image, so whatever it
/// decodes is that body's and nothing else's.
fn alone(data: &[u8]) -> Vec<Image> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(s) = find(data, i, b"\x1bP") {
        let e = find(data, s, b"\x1b\\").expect("every Sixel is terminated") + 2;
        let mut owned = Owned::new(&WALK_WS).with_images(walk_budget());
        let mut t = Term::new(owned.storage(), &WALK_WS);
        feed_cut(&mut t, &data[s..e], &[]);
        let imgs = t.images();
        let placed: Vec<_> = imgs.placements().map(|(_, p)| p.image).collect();
        if let [id] = placed[..] {
            let v = imgs.image(id).expect("live");
            out.push((v.width.raw(), v.height.raw(), v.pixels.to_vec()));
        }
        i = e;
    }
    out
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// **The recorded yazi walk, replayed: every preview is the image its own body
/// draws.** yazi sends `P2 = 1` (`ESC P 9;1 q`), so an unset bit keeps what is
/// under the image. One preview is live at a time, so the second through the
/// thirteenth are each stored in the span the first was freed from. Before the
/// span was cleared, seven of them -- the fourth through the tenth, whose
/// extent runs past their raster's `Pv` to a whole band (375 to 378, 64 to 66,
/// 776 to 780 rows) -- carried an earlier preview's pixels in those rows, and
/// the screenshot's last three rows differed between the way down and the way
/// back.
#[test]
fn every_preview_in_a_yazi_walk_is_its_own_body_alone() {
    let oracle = alone(WALK);
    // The positive control on the oracle and the observer: thirteen Sixels,
    // each `P2 = 1`, and every one stored at both read sizes.
    assert_eq!(oracle.len(), 13, "the recording carries thirteen previews");
    assert_eq!(
        WALK.windows(6).filter(|w| w == b"\x1bP9;1q").count(),
        13,
        "every preview asks P2 = 1"
    );
    for chunk in [1usize, 7] {
        let got = stored(WALK, chunk);
        assert_eq!(got.len(), 13, "chunk {chunk}: thirteen stores observed");
        let wrong: Vec<usize> = (0..13)
            .filter(|&k| got[k] != oracle[k])
            .map(|k| k + 1)
            .collect();
        assert!(
            wrong.is_empty(),
            "chunk {chunk}: previews {wrong:?} differ from their body decoded alone"
        );
        // Walking down and back shows each file twice, preview k and 14 - k,
        // and a file's preview cannot depend on the way the walk came to it.
        for k in 0..13 {
            assert!(
                got[k] == got[12 - k],
                "chunk {chunk}: preview {} and preview {} are one file and differ",
                k + 1,
                13 - k
            );
        }
    }
}

/// FNV-1a 64 over an image's pixel words, little-endian: a digest to pin
/// thirteen 600x804 images by.
fn fnv(px: &[ColorWord]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for c in px {
        for b in c.bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// **The recorded yazi walk draws, pixel for pixel, what the buffered
/// decoder drew.** Each preview's size and FNV-1a digest, taken from the
/// buffered decoder at 2992a8e9 -- the body held whole, measured, and
/// decoded into a cleared span -- fed the same way, at reads of 1 and 7.
/// Since 2026-10-14 the body is decoded as it arrives, and the images are
/// those. The other recorded yazi runs, 41 Sixels in all, were held to the
/// same in the record's entry ("A Sixel is decoded as its body arrives").
#[test]
fn every_preview_in_a_yazi_walk_is_the_buffered_decoders_image() {
    const WANT: [(u16, u16, u64); 13] = [
        (600, 600, 0x6132_24e3_2195_be70),
        (600, 804, 0x9e81_e642_d3a8_4c4b),
        (600, 804, 0x4cbc_c35c_bfb6_4bef),
        (600, 378, 0xbdcf_bb00_d7f7_aee7),
        (64, 66, 0xaafa_e662_fca9_ff84),
        (600, 780, 0xe0c6_92e5_eff3_6eae),
        (600, 780, 0x6254_c5dc_a39c_0a8e),
        (600, 780, 0xe0c6_92e5_eff3_6eae),
        (64, 66, 0xaafa_e662_fca9_ff84),
        (600, 378, 0xbdcf_bb00_d7f7_aee7),
        (600, 804, 0x4cbc_c35c_bfb6_4bef),
        (600, 804, 0x9e81_e642_d3a8_4c4b),
        (600, 600, 0x6132_24e3_2195_be70),
    ];
    for chunk in [1usize, 7] {
        let got: Vec<(u16, u16, u64)> = stored(WALK, chunk)
            .iter()
            .map(|(w, h, px)| (*w, *h, fnv(px)))
            .collect();
        assert_eq!(got, WANT, "chunk {chunk}");
    }
}
