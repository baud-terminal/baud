//! Bitmap strikes, layered colour and the PNG decoder, each against an
//! oracle that exists for no other reason than to disagree.
//!
//! **Three oracles.** `ttf-parser` for `sbix`, `CBLC`/`CBDT`, `COLR` v0
//! and `CPAL` -- every strike of every face, every glyph, compared on
//! presence, placement, `ppem` and the PNG bytes themselves; the `png`
//! crate for the decoder -- every PNG the strikes yield, decoded by both
//! and compared as RGBA8 byte for byte; and the specification for what the
//! two crates leave open.
//!
//! **Corpus.** The committed fixtures always: `NotoColorEmoji-subset.ttf`,
//! a `CBDT` face re-encoded across every index and image format the reader
//! implements (`tests/noto_subset.rs` says which glyphs are which),
//! `BungeeTint-Regular.ttf`, a `COLR` v0 face with eight palettes, and
//! `sbix-strikes.ttf`, six strikes holding every per-glyph case
//! (`SBIX` below lists them), seven unreadable PNGs among
//! them, which both decoders must refuse and which are counted. On
//! macOS every face under `/System/Library/Fonts` as well, which is where
//! Apple Color Emoji's 31,707 `sbix` PNGs come from; a face with no strikes
//! and no colour costs one parse and contributes nothing.
//!
//! **Normalisations, counted and pinned.** `ttf-parser` reports a `CBDT`
//! image's `y` as `bearingY - height` where the table (and baud) say
//! `bearingY`, so the comparison adds the height back and counts the
//! translation; a `COLR` version-1-only glyph is one baud answers `None`
//! for by decision D10, and is counted when the oracle paints it with
//! anything but solid layers. Both counts are asserted, one to be non-zero
//! where the corpus reaches it and one to be zero on the corpus as it is.
//!
//! **`'flip'` is read by the test, not the oracle.** `ttf-parser` answers
//! `None` for Apple's undocumented `'flip'` records, where baud answers the
//! target's image to be mirrored. So for every glyph baud says to mirror,
//! the test reads the redirections out of the raw `sbix` itself -- a second
//! spelling, which refuses a chain past ten redirections, one more than
//! baud's bound, so it never refuses what baud draws -- and requires a
//! `'flip'` on the way and baud's PNG to be the oracle's image of the glyph
//! the chain ends at, byte for byte and at its offsets. Each mirrored lookup
//! is counted: Apple Color Emoji's two faces give 972 each, one per `'flip'`
//! record, and the fixture 84 -- fourteen glyphs that resolve through a flip,
//! in each of six strikes, from 66 `'flip'` records. The count is asserted
//! exactly off macOS and as at least both on it.

use baud_domain::GlyphId;
use baud_font::bytes::Index;
use baud_font::{
    Face, FaceIndex, Mirror, Palette, PaletteEntry, Png, PngError, Rgba, Strike, faces,
};
use std::ops::Range;
use std::path::{Path, PathBuf};
use ttf_parser::RgbaColor;
use ttf_parser::colr::{ClipBox, CompositeMode, Paint, Painter};

mod sha256;
mod writer;
use writer::{Be, Payloads};

fn committed() -> Vec<PathBuf> {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"));
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("tests/fonts exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ttf"))
        .collect();
    v.sort();
    v
}

fn corpus() -> Vec<PathBuf> {
    let mut v = committed();
    if cfg!(target_os = "macos") {
        for dir in [
            "/System/Library/Fonts",
            "/System/Library/Fonts/Supplemental",
        ] {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    let ext = p
                        .extension()
                        .map(|e| e.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    if matches!(ext.as_str(), "ttf" | "ttc" | "otf") {
                        v.push(p);
                    }
                }
            }
        }
    }
    v.sort();
    v
}

/// A count the tally keeps, typed so the audit's floor need not move for it.
#[derive(Default, Debug, PartialEq, Eq)]
struct Counted(usize);

impl Counted {
    fn bump(&mut self) {
        self.0 += 1;
    }
}

#[derive(Default, Debug)]
struct Tally {
    faces: usize,
    strikes: usize,
    sbix_bitmaps: usize,
    sbix_flips: Counted,
    cbdt_bitmaps: usize,
    absent_agreed: usize,
    cbdt_y_translated: usize,
    pngs_decoded: usize,
    pixels: usize,
    refused_by_both: Counted,
    layered_glyphs: usize,
    layers_agreed: usize,
    colr_v1_only: usize,
    palettes: usize,
    colors_agreed: usize,
}

impl Tally {
    /// Adds another unit's tally to this one. Destructured whole, so a field
    /// added to `Tally` and not summed here does not compile -- a count left
    /// out would read zero, and a positive control on it would fail for the
    /// merge rather than for the fonts.
    fn absorb(&mut self, other: Tally) {
        let Tally {
            faces,
            strikes,
            sbix_bitmaps,
            sbix_flips: Counted(sbix_flips),
            cbdt_bitmaps,
            absent_agreed,
            cbdt_y_translated,
            pngs_decoded,
            pixels,
            refused_by_both: Counted(refused_by_both),
            layered_glyphs,
            layers_agreed,
            colr_v1_only,
            palettes,
            colors_agreed,
        } = other;
        self.faces += faces;
        self.strikes += strikes;
        self.sbix_bitmaps += sbix_bitmaps;
        self.sbix_flips.0 += sbix_flips;
        self.cbdt_bitmaps += cbdt_bitmaps;
        self.absent_agreed += absent_agreed;
        self.cbdt_y_translated += cbdt_y_translated;
        self.pngs_decoded += pngs_decoded;
        self.pixels += pixels;
        self.refused_by_both.0 += refused_by_both;
        self.layered_glyphs += layered_glyphs;
        self.layers_agreed += layers_agreed;
        self.colr_v1_only += colr_v1_only;
        self.palettes += palettes;
        self.colors_agreed += colors_agreed;
    }
}

/// What `ttf-parser` paints: for `COLR` v0 that is `outline_glyph` then
/// `paint(Solid)` per layer; anything else is version 1 and is recorded as
/// such.
#[derive(Default)]
struct Rec {
    pending: Option<u16>,
    layers: Vec<(u16, RgbaColor)>,
    v1: bool,
}

impl Painter<'_> for Rec {
    fn outline_glyph(&mut self, g: ttf_parser::GlyphId) {
        self.pending = Some(g.0);
    }
    fn paint(&mut self, paint: Paint<'_>) {
        match (self.pending.take(), paint) {
            (Some(g), Paint::Solid(c)) => self.layers.push((g, c)),
            _ => self.v1 = true,
        }
    }
    fn push_clip(&mut self) {
        self.v1 = true;
    }
    fn push_clip_box(&mut self, _: ClipBox) {
        self.v1 = true;
    }
    fn pop_clip(&mut self) {
        self.v1 = true;
    }
    fn push_layer(&mut self, _: CompositeMode) {
        self.v1 = true;
    }
    fn pop_layer(&mut self) {
        self.v1 = true;
    }
    fn push_transform(&mut self, _: ttf_parser::Transform) {
        self.v1 = true;
    }
    fn pop_transform(&mut self) {
        self.v1 = true;
    }
}

fn same(ours: Rgba, theirs: RgbaColor) -> bool {
    (ours.r, ours.g, ours.b, ours.a) == (theirs.red, theirs.green, theirs.blue, theirs.alpha)
}

/// The oracle's output, whatever channel layout `EXPAND | STRIP_16` left it
/// in, as RGBA8 -- which is the one layout baud's decoder answers in.
fn oracle_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().expect("the oracle reads the PNG");
    let mut buf = vec![0u8; reader.output_buffer_size().expect("the size fits")];
    let info = reader
        .next_frame(&mut buf)
        .expect("the oracle decodes the PNG");
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => px
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Rgb => px
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Indexed => panic!("EXPAND leaves no indexed output"),
    };
    (info.width, info.height, rgba)
}

fn decode_both(bytes: &[u8], t: &mut Tally, what: &str) {
    let png = Png::parse(bytes)
        .unwrap_or_else(|e| panic!("{what}: baud refuses a PNG the strike carries: {e:?}"));
    let mut raw = vec![0u8; png.raw_len().expect("a strike PNG has a bounded size")];
    let mut rgba = vec![0u8; png.rgba_len().unwrap()];
    let size = match png.decode(&mut raw, &mut rgba) {
        Ok(size) => size,
        Err(e) => {
            // A strike may carry a PNG nobody can read; `sbix-strikes.ttf`
            // does on purpose. The oracle must refuse it too.
            let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
            dec.set_transformations(png::Transformations::EXPAND);
            let theirs = dec.read_info().and_then(|mut r| {
                let mut buf = vec![0u8; r.output_buffer_size().expect("the size fits")];
                r.next_frame(&mut buf).map(|_| ())
            });
            assert!(
                theirs.is_err(),
                "{what}: baud refuses ({e:?}) a PNG the oracle reads"
            );
            t.refused_by_both.bump();
            return;
        }
    };
    let (w, h, theirs) = oracle_rgba(bytes);
    assert_eq!((size.width, size.height), (w, h), "{what}: size");
    assert!(rgba == theirs, "{what}: RGBA bytes differ");
    t.pngs_decoded += 1;
    t.pixels += rgba.len() / 4;
}

/// One `sbix` record's graphic type and, for a redirection, the glyph it
/// names: read straight from the table, so the test does not ask baud.
fn sbix_record(sbix: &[u8], strike: Index, glyph: GlyphId) -> Option<([u8; 4], GlyphId)> {
    let be32 = |at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(sbix.get(at..at + 4)?.try_into().ok()?))
    };
    let at = usize::try_from(be32(8 + 4 * usize::try_from(strike.value()).ok()?)?).ok()?;
    let g = glyph.index();
    let start = usize::try_from(be32(at + 4 + 4 * g)?).ok()?;
    let end = usize::try_from(be32(at + 4 + 4 * (g + 1))?).ok()?;
    if end < start + 10 {
        return None;
    }
    let rec = at + start;
    let kind: [u8; 4] = sbix.get(rec + 4..rec + 8)?.try_into().ok()?;
    let target = u16::from_be_bytes(sbix.get(rec + 8..rec + 10)?.try_into().ok()?);
    Some((kind, GlyphId::from_shaper(target)))
}

/// What one unit of work compares of a face: a run of one strike's glyphs,
/// or the face's layers and palettes, which also counts the face.
enum Part {
    Strike(Index, Range<u32>),
    Colour,
}

fn compare_face(path: &Path, data: &[u8], i: u32, part: &Part, t: &mut Tally) {
    let Ok(ours) = Face::parse_index(data, FaceIndex::of(i)) else {
        return;
    };
    let Ok(theirs) = ttf_parser::Face::parse(data, i) else {
        return;
    };
    let what = format!("{}#{i}", path.display());
    let glyphs = u32::from(ours.glyph_count().value());

    // Strikes: every glyph of every strike, by bytes, one run of a strike's
    // glyphs at a time. The run that starts a strike counts it.
    if let Part::Strike(s, run) = part {
        if run.start == 0 {
            t.strikes += 1;
        }
        let s = *s;
        let ppem = ours
            .strike_ppem(s)
            .unwrap_or_else(|| panic!("{what}: strike {s:?} has no ppem"));
        for g in run.clone() {
            let gid = GlyphId::from_shaper(u16::try_from(g).unwrap());
            let mine = ours.bitmap(s, gid);
            let img = theirs
                .glyph_raster_image(ttf_parser::GlyphId(u16::try_from(g).unwrap()), ppem.value());
            match (mine, img) {
                (None, None) => t.absent_agreed += 1,
                (Some(b), Some(img)) => {
                    assert_eq!(b.mirror, Mirror::AsStored, "{what} g{g}: mirrored");
                    assert_eq!(
                        img.format,
                        ttf_parser::RasterImageFormat::PNG,
                        "{what} g{g}"
                    );
                    assert_eq!(img.pixels_per_em, ppem.value(), "{what} g{g}: ppem");
                    assert!(b.png.as_slice() == img.data, "{what} g{g}: PNG bytes");
                    assert_eq!(b.placement.x, img.x, "{what} g{g}: x");
                    match b.strike {
                        Strike::Sbix => {
                            assert_eq!(b.placement.y, img.y, "{what} g{g}: y");
                            t.sbix_bitmaps += 1;
                        }
                        Strike::Cbdt => {
                            // The oracle answers the image's bottom edge; the
                            // table gives the top bearing.
                            assert_eq!(
                                b.placement.y,
                                img.y + i16::try_from(img.height).unwrap(),
                                "{what} g{g}: y"
                            );
                            t.cbdt_y_translated += 1;
                            t.cbdt_bitmaps += 1;
                        }
                    }
                    decode_both(
                        b.png.as_slice(),
                        t,
                        &format!("{what} g{g} @{}", ppem.value()),
                    );
                }
                (Some(b), None) if b.mirror == Mirror::LeftRight => {
                    let sbix = theirs
                        .raw_face()
                        .table(ttf_parser::Tag::from_bytes(b"sbix"))
                        .unwrap_or_else(|| panic!("{what} g{g}: mirrored without sbix"));
                    let (mut h, mut flipped) = (gid, false);
                    let mut hops = 0;
                    let img = loop {
                        assert!(hops < 10, "{what} g{g}: chain past ten hops");
                        hops += 1;
                        let (kind, target) = sbix_record(sbix, s, h)
                            .unwrap_or_else(|| panic!("{what} g{g}: no record for {h:?}"));
                        match &kind {
                            b"flip" => flipped = true,
                            b"dupe" => {}
                            other => panic!("{what} g{g}: {other:?} is no redirection"),
                        }
                        h = target;
                        let th = ttf_parser::GlyphId(u16::try_from(h.index()).unwrap());
                        if let Some(img) = theirs.glyph_raster_image(th, ppem.value()) {
                            break img;
                        }
                    };
                    assert!(flipped, "{what} g{g}: mirrored with no flip on the way");
                    assert!(b.png.as_slice() == img.data, "{what} g{g}: PNG bytes");
                    assert_eq!(
                        (b.placement.x, b.placement.y),
                        (img.x, img.y),
                        "{what} g{g}"
                    );
                    t.sbix_flips.bump();
                    decode_both(b.png.as_slice(), t, &format!("{what} g{g} flip"));
                }
                (mine, img) => panic!(
                    "{what} g{g} @{}: baud {:?}, oracle {:?}",
                    ppem.value(),
                    mine.map(|b| b.placement),
                    img.map(|i| (i.x, i.y, i.width, i.height))
                ),
            }
        }
        return;
    }
    t.faces += 1;

    // Layers: every glyph, every palette.
    let palettes = ours.palettes().value();
    let foreground = RgbaColor::new(0x12, 0x34, 0x56, 0x78);
    for g in 0..glyphs {
        let gid = GlyphId::from_shaper(u16::try_from(g).unwrap());
        let tg = ttf_parser::GlyphId(u16::try_from(g).unwrap());
        let mine: Option<Vec<_>> = ours.layers(gid).map(|l| l.collect());
        for p in 0..palettes.max(1) {
            let mut rec = Rec::default();
            let painted =
                theirs.paint_color_glyph(tg, u16::try_from(p).unwrap(), foreground, &mut rec);
            match (&mine, painted) {
                (None, None) => {}
                (None, Some(())) if rec.v1 => t.colr_v1_only += 1,
                (Some(layers), Some(())) => {
                    assert!(
                        !rec.v1,
                        "{what} g{g}: the oracle painted version 1 for a glyph with v0 layers"
                    );
                    assert_eq!(layers.len(), rec.layers.len(), "{what} g{g}: layer count");
                    for (l, (tg, tc)) in layers.iter().zip(&rec.layers) {
                        assert_eq!(
                            u16::try_from(l.glyph.index()).unwrap(),
                            *tg,
                            "{what} g{g}: layer glyph"
                        );
                        let mine = if l.color == PaletteEntry::FOREGROUND {
                            Rgba {
                                r: foreground.red,
                                g: foreground.green,
                                b: foreground.blue,
                                a: foreground.alpha,
                            }
                        } else {
                            ours.color(Palette::of(u16::try_from(p).unwrap()), l.color)
                                .unwrap_or_else(|| {
                                    panic!("{what} g{g}: entry {:?} has no colour", l.color)
                                })
                        };
                        assert!(
                            same(mine, *tc),
                            "{what} g{g} palette {p}: {mine:?} vs {tc:?}"
                        );
                        t.layers_agreed += 1;
                        if l.color != PaletteEntry::FOREGROUND {
                            t.colors_agreed += 1;
                        }
                    }
                    if p == 0 {
                        t.layered_glyphs += 1;
                    }
                }
                (mine, painted) => panic!(
                    "{what} g{g} palette {p}: baud {mine:?}, oracle {painted:?} v1={}",
                    rec.v1
                ),
            }
        }
    }

    // Palettes: the oracle exposes `CPAL` only through the layers it paints,
    // which the loop above compared entry by entry; what it does expose is
    // the count, and the edges are the specification's.
    match theirs.color_palettes() {
        Some(n) => {
            assert_eq!(u32::from(n.get()), palettes, "{what}: palette count");
            t.palettes += usize::try_from(palettes).unwrap();
            let past = u16::try_from(palettes).unwrap();
            assert!(
                ours.color(Palette::of(past), PaletteEntry::of(0)).is_none(),
                "{what}: one palette past the end"
            );
            assert!(
                ours.color(Palette::of(0), PaletteEntry::FOREGROUND)
                    .is_none(),
                "{what}: the foreground is not a table entry"
            );
        }
        None => assert_eq!(
            palettes, 0,
            "{what}: baud sees palettes the oracle does not"
        ),
    }
}

/// Glyphs in one strike piece. By the profile's per-strike times each of
/// Apple Color Emoji's two 160 ppem strikes is 3.29-3.30 s over 3,844
/// glyphs, so 128 of them are ~0.11 s, against the ~0.77 s each of
/// eighteen workers gets of the whole 13.9 s; the 20 ppem pieces, which
/// go last, are ~3-4 ms.
const PIECE: u32 = 128;

/// Face `i`'s strikes cut into pieces of `PIECE` glyphs, each with `ppem`
/// squared times its glyphs, which ranks the pieces by the pixels they
/// decode. A strike of a face with no glyphs is still one piece, so it is
/// still counted and its `ppem` still asked for; a face either parser
/// refuses has none, as `compare_face` returns for it.
fn cut(data: &[u8], i: u32) -> Vec<(Part, u64)> {
    let Ok(ours) = Face::parse_index(data, FaceIndex::of(i)) else {
        return Vec::new();
    };
    if ttf_parser::Face::parse(data, i).is_err() {
        return Vec::new();
    }
    let glyphs = u32::from(ours.glyph_count().value());
    let mut pieces = Vec::new();
    for s in 0..ours.strikes().value() {
        let s = Index::of(s);
        let px = ours
            .strike_ppem(s)
            .map_or(0, |p| u64::from(p.value()).pow(2));
        let mut start = 0;
        loop {
            let end = glyphs.min(start + PIECE);
            pieces.push((Part::Strike(s, start..end), px * u64::from(end - start)));
            start = end;
            if start == glyphs {
                break;
            }
        }
    }
    pieces
}

/// A unit's result, or the panic `quietly` kept from it.
type Kept<T> = std::thread::Result<T>;

/// One face as its file's reader leaves it: its strike pieces, cut but not
/// yet compared, and its layers and palettes, compared.
type FaceRead = (Kept<Vec<(Part, u64)>>, Kept<Tally>);

/// One file, read once, by whichever worker took it: its faces, or the
/// panic `quietly` kept from counting them, and its bytes, kept only when a
/// piece will need them.
fn open(path: &Path) -> Option<(Vec<u8>, Kept<Vec<FaceRead>>)> {
    let data = std::fs::read(path).ok()?;
    let per_face = quietly(|| faces(&data).map(|n| n.value()).unwrap_or(1)).map(|n| {
        (0..n)
            .map(|i| {
                let pieces = quietly(|| cut(&data, i));
                let colour = quietly(|| {
                    let mut t = Tally::default();
                    compare_face(path, &data, i, &Part::Colour, &mut t);
                    t
                });
                (pieces, colour)
            })
            .collect::<Vec<FaceRead>>()
    });
    let shared = per_face
        .iter()
        .flatten()
        .any(|(p, _)| p.as_ref().is_ok_and(|p| !p.is_empty()));
    Some((if shared { data } else { Vec::new() }, per_face))
}

/// Runs `work` on every unit across the cores and answers each result at
/// its unit's index, so they are read in corpus order whichever thread ran
/// which. Workers take the next unit off one counter, the largest `cost`
/// first, so the big units start at once rather than last and alone. A
/// panic outside `quietly` is raised again here.
fn spread<U: Sync, R: Send>(
    units: &[U],
    cost: impl Fn(&U) -> u64,
    work: impl Fn(&U) -> R + Sync,
) -> Vec<R> {
    let mut order: Vec<usize> = (0..units.len()).collect();
    order.sort_by_cached_key(|&k| core::cmp::Reverse(cost(&units[k])));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(1, std::num::NonZero::get)
        .min(units.len().max(1));
    let mut slots: Vec<Option<R>> = (0..units.len()).map(|_| None).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    while let Some(&k) =
                        order.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                    {
                        done.push((k, work(&units[k])));
                    }
                    done
                })
            })
            .collect();
        for h in handles {
            for (k, r) in h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)) {
                slots[k] = Some(r);
            }
        }
    });
    slots
        .into_iter()
        .map(|r| r.expect("every unit ran"))
        .collect()
}

thread_local! {
    /// Set while this thread runs a unit inside `quietly`.
    static QUIET: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// `f`'s result, or its panic, kept rather than printed. Several units can
/// fail in one run, and which printed first would be the scheduler's
/// choice; `settle` runs the first in corpus order again on the test
/// thread instead, where it prints as one thread's did. The hook is quiet
/// only on a thread inside this call, and passes every other panic on.
fn quietly<R>(f: impl FnOnce() -> R) -> Kept<R> {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let loud = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.get() {
                loud(info);
            }
        }));
    });
    QUIET.set(true);
    let kept = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    QUIET.set(false);
    kept
}

/// A unit's result; or, for the first unit in corpus order that panicked,
/// that unit run again on this thread, so the failure reported is the one a
/// single thread would have stopped at, with its own message and location.
/// A unit that panicked on a worker and passes here would be a comparison
/// that is not deterministic, and fails as one.
fn settle<T>(kept: Kept<T>, again: impl FnOnce()) -> T {
    kept.unwrap_or_else(|e| {
        again();
        let why = e
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| e.downcast_ref::<&str>().copied())
            .unwrap_or("a panic with no message");
        panic!("passed on the test thread, after panicking on a worker: {why}")
    })
}

#[test]
fn every_strike_layer_and_pixel_agrees_with_the_oracles() {
    // **Read once, then cut where the time is.** On one thread an
    // instrumented run spent 13.94 s in Apple Color Emoji's eighteen
    // strikes, 3.29-3.30 s in each 160 ppem one, and under 5 ms in every
    // other file: split by file, one worker holds nearly all of it, and by
    // strike, a quarter. So each file is read once, by whichever worker takes
    // it, largest first, which counts its faces, compares their layers and
    // palettes and cuts their strikes into `PIECE`-glyph pieces; then the
    // workers take the pieces, the most pixels first, over bytes they all
    // share. Each piece and each face's colour is its own tally, summed
    // below in the order one thread visits them, so every count is one
    // thread's -- and so is a failure, because the first in that order, a
    // file's face count among them, is run again on this thread (`settle`).
    //
    // Peak memory is the first pass's files in flight: on this Mac's
    // eighteen cores the eighteen largest are 625 of the corpus's 814 MB,
    // and a counting allocator put the live heap's peak at 592-596 MiB,
    // against 184 MiB on one thread. The pieces then share Apple Color
    // Emoji's 183 MiB and add ~7 MiB between the eighteen workers, each
    // holding one PNG's buffers at a time, a 160 ppem image's the largest.
    let paths = corpus();
    let opened = spread(
        &paths,
        |p| std::fs::metadata(p).map_or(0, |m| m.len()),
        |p| open(p),
    );
    let pieces: Vec<(&Path, &[u8], u32, &Part, u64)> = paths
        .iter()
        .zip(&opened)
        .filter_map(|(p, o)| Some((p.as_path(), o.as_ref()?)))
        .flat_map(|(p, (data, per_face))| {
            per_face
                .iter()
                .flatten()
                .zip(0u32..)
                .flat_map(move |((pieces, _), i)| {
                    pieces
                        .iter()
                        .flatten()
                        .map(move |(part, cost)| (p, data.as_slice(), i, part, *cost))
                })
        })
        .collect();
    let done = spread(
        &pieces,
        |p| p.4,
        |&(path, data, i, part, _)| {
            quietly(|| {
                let mut t = Tally::default();
                compare_face(path, data, i, part, &mut t);
                t
            })
        },
    );
    let mut done = done.into_iter();
    let mut t = Tally::default();
    let mut files = 0;
    for (path, o) in paths.iter().zip(opened) {
        let Some((_, per_face)) = o else {
            continue;
        };
        let reread = || std::fs::read(path).expect("a file read once reads again");
        let per_face = settle(per_face, || {
            let _ = faces(&reread()).map(|n| n.value()).unwrap_or(1);
        });
        for ((pieces, colour), i) in per_face.into_iter().zip(0u32..) {
            for (part, _) in settle(pieces, || {
                cut(&reread(), i);
            }) {
                let kept = done.next().expect("a result for every piece");
                t.absorb(settle(kept, || {
                    compare_face(path, &reread(), i, &part, &mut Tally::default());
                }));
            }
            t.absorb(settle(colour, || {
                compare_face(path, &reread(), i, &Part::Colour, &mut Tally::default());
            }));
        }
        files += 1;
    }
    assert!(done.next().is_none(), "a piece no face accounts for");
    eprintln!("{files} files: {t:#?}");

    // Positive controls, so a corpus that reaches nothing cannot pass.
    // The committed CBDT subset: 42 bitmaps across five subtables, four
    // glyphs without, and every PNG decoded.
    assert!(t.cbdt_bitmaps >= 42, "{}", t.cbdt_bitmaps);
    assert!(t.absent_agreed >= 4, "{}", t.absent_agreed);
    assert!(t.pngs_decoded >= 42, "{}", t.pngs_decoded);
    assert_eq!(t.cbdt_y_translated, t.cbdt_bitmaps);
    // Bungee Tint: 1,063 layered glyphs over 8 palettes of 2 entries.
    assert!(t.layered_glyphs >= 1063, "{}", t.layered_glyphs);
    assert!(t.layers_agreed >= 2126 * 8, "{}", t.layers_agreed);
    assert!(t.palettes >= 8, "{}", t.palettes);
    assert!(t.colors_agreed >= 2126 * 8, "{}", t.colors_agreed);
    // Apple Color Emoji on macOS: nine sbix strikes, tens of thousands of
    // PNGs.
    if cfg!(target_os = "macos") {
        assert!(t.strikes >= 10, "{}", t.strikes);
        assert!(t.sbix_bitmaps >= 30_000, "{}", t.sbix_bitmaps);
        // 108 people facing right in each of nine strikes, in both of
        // Apple Color Emoji's faces, and the fixture's 84.
        assert!(t.sbix_flips.0 >= 1_944 + 84, "{:?}", t.sbix_flips);
        assert!(t.pngs_decoded >= 30_000, "{}", t.pngs_decoded);
    }
    // `sbix-strikes.ttf`'s unreadable PNGs: E's at 40 ppem and Q's in all
    // six strikes; none elsewhere.
    assert_eq!(t.refused_by_both, Counted(7));
    // The fixture mirrors C, D, G, K, P, R2 to R9 and U in each of six
    // strikes.
    if !cfg!(target_os = "macos") {
        assert_eq!(t.sbix_flips, Counted(84));
    }
    assert_eq!(
        t.colr_v1_only, 0,
        "a COLR v1-only face joined the corpus; count it here"
    );
}

#[test]
fn the_subset_fixture_answers_every_arrangement_the_generator_wrote() {
    // `tests/noto_subset.rs` lays glyphs 4..45 out across index
    // formats 1, 3, 4, 5 and 2 in that order, so every glyph answering is
    // every format read. Glyphs 0..3 carry nothing.
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/NotoColorEmoji-subset.ttf"
    ))
    .unwrap();
    let face = Face::parse(&data).unwrap();
    assert_eq!(face.strikes().value(), 1);
    assert_eq!(face.strike_ppem(Index::ZERO).unwrap().value(), 109);
    for g in 0..4u16 {
        assert!(
            face.bitmap(Index::ZERO, GlyphId::from_shaper(g)).is_none(),
            "glyph {g}"
        );
    }
    for g in 4..46u16 {
        let b = face
            .bitmap(Index::ZERO, GlyphId::from_shaper(g))
            .unwrap_or_else(|| panic!("glyph {g}"));
        assert_eq!(b.strike, Strike::Cbdt);
        assert_eq!(&b.png.as_slice()[..8], b"\x89PNG\r\n\x1a\n", "glyph {g}");
    }
    assert!(face.bitmap(Index::ZERO, GlyphId::from_shaper(46)).is_none());
    assert!(
        face.bitmap(Index::of(1u32), GlyphId::from_shaper(4))
            .is_none()
    );
}

#[test]
fn a_png_the_decoder_refuses_is_named() {
    // The signature, the CRC, and a truncation each refuse with their own
    // reason, so a caller can say what was wrong rather than "no image".
    assert_eq!(Png::parse(b"not a png").err(), Some(PngError::Malformed));
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/NotoColorEmoji-subset.ttf"
    ))
    .unwrap();
    let face = Face::parse(&data).unwrap();
    let mut png = face
        .bitmap(Index::ZERO, GlyphId::from_shaper(4))
        .unwrap()
        .png
        .as_slice()
        .to_vec();
    assert!(Png::parse(&png).is_ok());
    png[20] ^= 0xFF; // inside IHDR, so its CRC no longer matches
    assert_eq!(Png::parse(&png).err(), Some(PngError::Crc));
    let short = &png[..png.len() / 2];
    // A truncated stream may fail at the chunk walk or inside inflate;
    // either way it fails, and never with a panic.
    let p = Png::parse(short);
    if let Ok(p) = p {
        let mut raw = vec![0u8; p.raw_len().unwrap()];
        let mut rgba = vec![0u8; p.rgba_len().unwrap()];
        assert!(p.decode(&mut raw, &mut rgba).is_err());
    }
}

#[test]
fn the_sbix_fixture_answers_every_case_in_every_strike() {
    // `SBIX`: strikes at 10, 20, 26, 40, 52 and 80 ppem.
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/sbix-strikes.ttf"
    ))
    .unwrap();
    let face = Face::parse(&data).unwrap();
    let ppems: Vec<u16> = (0..face.strikes().value())
        .map(|s| face.strike_ppem(Index::of(s)).unwrap().value())
        .collect();
    assert_eq!(ppems, [10, 20, 26, 40, 52, 80]);
    let g = GlyphId::from_shaper;
    for s in 0..6u32 {
        let i = Index::of(s);
        let ppem = ppems[usize::try_from(s).unwrap()];
        let a = face.bitmap(i, g(1)).expect("A is in every strike");
        assert_eq!(a.mirror, Mirror::AsStored);
        assert_eq!((a.placement.x, a.placement.y), (0, 0));
        // B is in the 80 strike only, H in the 10 strike only.
        assert_eq!(face.bitmap(i, g(2)).is_some(), ppem == 80, "B @{ppem}");
        assert_eq!(face.bitmap(i, g(8)).is_some(), ppem == 10, "H @{ppem}");
        // C flips A, D flips a dupe of A, G flips C: each is A's PNG,
        // mirrored once, at A's offsets rather than C's own (7, 3).
        for (glyph, name) in [(3, "C"), (4, "D"), (7, "G")] {
            let b = face
                .bitmap(i, g(glyph))
                .unwrap_or_else(|| panic!("{name} @{ppem}"));
            assert_eq!(b.mirror, Mirror::LeftRight, "{name} @{ppem}");
            assert!(b.png.as_slice() == a.png.as_slice(), "{name} @{ppem}");
            assert_eq!((b.placement.x, b.placement.y), (0, 0), "{name} @{ppem}");
        }
        // J is a dupe of A, as stored.
        let j = face.bitmap(i, g(10)).unwrap();
        assert_eq!(j.mirror, Mirror::AsStored);
        assert!(j.png.as_slice() == a.png.as_slice());
        // E is a record in every strike; at 40 its PNG does not decode.
        let e = face.bitmap(i, g(5)).unwrap();
        let png = Png::parse(e.png.as_slice()).unwrap();
        let mut raw = vec![0u8; png.raw_len().unwrap()];
        let mut rgba = vec![0u8; png.rgba_len().unwrap()];
        assert_eq!(
            png.decode(&mut raw, &mut rgba).is_err(),
            ppem == 40,
            "E @{ppem}"
        );
        // F holds nothing; I is a dupe of itself and L a flip of itself,
        // each refused at the bound.
        assert!(face.bitmap(i, g(6)).is_none(), "F @{ppem}");
        assert!(face.bitmap(i, g(9)).is_none(), "I @{ppem}");
        assert!(face.bitmap(i, g(12)).is_none(), "L @{ppem}");
        // K is a dupe of C: the flip comes second and still mirrors.
        let k = face.bitmap(i, g(11)).unwrap();
        assert_eq!(k.mirror, Mirror::LeftRight, "K @{ppem}");
        assert!(k.png.as_slice() == a.png.as_slice(), "K @{ppem}");
        // M is in the 20 and 52 strikes only.
        assert_eq!(
            face.bitmap(i, g(13)).is_some(),
            ppem == 20 || ppem == 52,
            "M @{ppem}"
        );
        // N's image sits at its own offsets; P flips N and sits at N's,
        // not its own (1, 1).
        let n = face.bitmap(i, g(14)).unwrap();
        assert_eq!(
            (n.mirror, n.placement.x, n.placement.y),
            (Mirror::AsStored, 5, -4),
            "N @{ppem}"
        );
        let p = face.bitmap(i, g(15)).unwrap();
        assert_eq!(
            (p.mirror, p.placement.x, p.placement.y),
            (Mirror::LeftRight, 5, -4),
            "P @{ppem}"
        );
        assert!(p.png.as_slice() == n.png.as_slice(), "P @{ppem}");
        // Q has a record everywhere and none of its PNGs decodes.
        let q = face.bitmap(i, g(16)).unwrap();
        let png = Png::parse(q.png.as_slice()).unwrap();
        let mut raw = vec![0u8; png.raw_len().unwrap()];
        let mut rgba = vec![0u8; png.rgba_len().unwrap()];
        assert!(png.decode(&mut raw, &mut rgba).is_err(), "Q @{ppem}");
        // R1 to R10: ten redirections alternating flip and dupe, then A.
        // The bound is ten records between both kinds, so R1, which needs
        // eleven, is refused and R2, which needs ten, is A's image mirrored.
        // R10 is a dupe of A alone, as stored.
        assert!(face.bitmap(i, g(17)).is_none(), "R1 @{ppem}");
        for r in 2..=10u16 {
            let b = face
                .bitmap(i, g(16 + r))
                .unwrap_or_else(|| panic!("R{r} @{ppem}"));
            let want = if r < 10 {
                Mirror::LeftRight
            } else {
                Mirror::AsStored
            };
            assert_eq!(b.mirror, want, "R{r} @{ppem}");
            assert!(b.png.as_slice() == a.png.as_slice(), "R{r} @{ppem}");
        }
        // U flips T's ramp; S stores the ramp mirrored, as its own image.
        let (t, u, sm) = (
            face.bitmap(i, g(27)).unwrap(),
            face.bitmap(i, g(28)).unwrap(),
            face.bitmap(i, g(29)).unwrap(),
        );
        assert_eq!(
            (t.mirror, u.mirror, sm.mirror),
            (Mirror::AsStored, Mirror::LeftRight, Mirror::AsStored),
            "T, U, S @{ppem}"
        );
        assert!(u.png.as_slice() == t.png.as_slice(), "U @{ppem}");
        assert!(sm.png.as_slice() != t.png.as_slice(), "S @{ppem}");
    }
    // Colour is a question about every strike: B and M are colour though the
    // first strike lacks them. F, the loops I and L and the chain R1 that
    // runs past the bound are not. Q is, though no image of it decodes: a
    // gap, pinned (the PNG is not decoded to answer the question).
    for (glyph, colour) in [
        (1, true),
        (2, true),
        (3, true),
        (4, true),
        (5, true),
        (6, false),
        (7, true),
        (8, true),
        (9, false),
        (10, true),
        (11, true),
        (12, false),
        (13, true),
        (14, true),
        (15, true),
        (16, true),
        (17, false),
        (18, true),
        (26, true),
        (27, true),
        (28, true),
        (29, true),
    ] {
        assert_eq!(face.is_colour(g(glyph)), colour, "glyph {glyph}");
    }
}

/// The byte offset of table `tag` in a single-face sfnt.
fn table_at(data: &[u8], tag: &[u8; 4]) -> usize {
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&r| &data[r..r + 4] == tag)
        .map(|r| {
            usize::try_from(u32::from_be_bytes(data[r + 8..r + 12].try_into().unwrap())).unwrap()
        })
        .unwrap_or_else(|| panic!("no {tag:?}"))
}

#[test]
fn a_strike_count_past_the_table_or_the_bound_reads_as_no_strikes() {
    // Every renderer walks the strikes for every glyph, so a hostile count is
    // a hang rather than a malformed answer. Each table is refused whole when
    // its count overflows the arithmetic, when it passes the 255 bound -- 256
    // offsets fit the fixture's 53,565-byte `sbix`, so only the bound refuses
    // that one -- and when the table is cut short of its own records. The
    // unpatched files are the controls.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts/");
    for (file, tag) in [
        ("sbix-strikes.ttf", b"sbix"),
        ("NotoColorEmoji-subset.ttf", b"CBLC"),
    ] {
        let clean = std::fs::read(format!("{dir}{file}")).unwrap();
        assert!(
            Face::parse(&clean).unwrap().strikes().value() > 0,
            "{file}: control"
        );
        let refused = |data: &[u8], what: &str| {
            let face = Face::parse(data).unwrap();
            assert_eq!(face.strikes().value(), 0, "{file}: {what}");
            assert!(!face.is_colour(GlyphId::from_shaper(1)), "{file}: {what}");
        };
        for count in [u32::MAX, 256] {
            let mut data = clean.clone();
            let t = table_at(&data, tag) + 4;
            data[t..t + 4].copy_from_slice(&count.to_be_bytes());
            refused(&data, &format!("count {count}"));
        }
        // The table's own length cut to 12 bytes: its count stands, its
        // records do not fit.
        let mut data = clean.clone();
        let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
        let r = (0..n)
            .map(|i| 12 + 16 * i)
            .find(|&r| &data[r..r + 4] == tag)
            .unwrap();
        data[r + 12..r + 16].copy_from_slice(&12u32.to_be_bytes());
        refused(&data, "cut to 12 bytes");
    }
    // The bound's other side: 255 strikes are read, so it is the bound that
    // refuses 256 and not the offsets running out.
    let mut data = std::fs::read(format!("{dir}sbix-strikes.ttf")).unwrap();
    let t = table_at(&data, b"sbix") + 4;
    data[t..t + 4].copy_from_slice(&255u32.to_be_bytes());
    assert_eq!(Face::parse(&data).unwrap().strikes().value(), 255);
    // The Noto subset's 288-byte `CBLC` cannot hold 256 size records, so the
    // records check refuses that count before the bound is asked. A `CBLC`
    // grown to hold them -- the first size record repeated, appended past the
    // file's end and pointed at from the table directory -- is refused by the
    // bound alone at 256 and read at 255.
    let noto = std::fs::read(format!("{dir}NotoColorEmoji-subset.ttf")).unwrap();
    for (count, strikes) in [(255u32, 255u32), (256, 0)] {
        let data = with_cblc_of(&noto, count);
        assert_eq!(
            Face::parse(&data).unwrap().strikes().value(),
            strikes,
            "CBLC grown to {count} size records"
        );
    }
}

/// `data` with its `CBLC` replaced by one of `count` size records, each a
/// copy of the original's first: the new table is appended, four-aligned,
/// and its directory record repointed. Checksums are not read.
fn with_cblc_of(data: &[u8], count: u32) -> Vec<u8> {
    const SIZE_RECORD: usize = 48;
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let r = (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&r| &data[r..r + 4] == b"CBLC")
        .unwrap();
    let at = table_at(data, b"CBLC");
    let mut table = data[at..at + 4].to_vec();
    table.extend_from_slice(&count.to_be_bytes());
    let first = &data[at + 8..at + 8 + SIZE_RECORD];
    for _ in 0..count {
        table.extend_from_slice(first);
    }
    let mut out = data.to_vec();
    out.resize(out.len().next_multiple_of(4), 0);
    let offset = u32::try_from(out.len()).unwrap();
    let length = u32::try_from(table.len()).unwrap();
    out.extend_from_slice(&table);
    out[r + 8..r + 12].copy_from_slice(&offset.to_be_bytes());
    out[r + 12..r + 16].copy_from_slice(&length.to_be_bytes());
    out
}

// ------------------------------------------------------------------ sbix-strikes.ttf

/// **`sbix-strikes.ttf`: every case a per-glyph strike choice and Apple's
/// undocumented `'flip'` have to get right**, written by [`sbix_font`].
///
/// Apple Color Emoji is where these cases live -- 177 glyphs empty in some
/// of its nine strikes and present in others, 108 `'flip'` records a strike
/// -- but it is a system file on one platform, so the tests that must run
/// everywhere read this instead. Six strikes, at 10, 20, 26, 40, 52 and 80
/// ppem; each strike's images -- all but T's and S's ramps and the
/// unreadable PNGs -- paint their left half in that strike's colour and
/// their right half white, so a rendering says which strike it came from
/// and whether it was mirrored. 26 and 52 are there because a half pixel
/// above either is where an 800-unit em's f32 scale round trip lands just
/// under the half (26.5 px comes back 26.499998), which a strike choice
/// that rounds half up has to survive.
///
/// | glyph | codepoint | what each strike holds |
/// |---|---|---|
/// | 1 A | U+E001 | `png ` in every strike |
/// | 2 B | U+E002 | `png ` at 80 only |
/// | 3 C | U+E003 | `flip` to A in every strike, at offsets (7, 3) A does not have |
/// | 4 D | U+E004 | `flip` to J, which is a `dupe` of A |
/// | 5 E | U+E005 | `png ` everywhere, but at 40 a PNG whose zlib stream is invalid |
/// | 6 F | U+E006 | nothing anywhere |
/// | 7 G | U+E007 | `flip` to C: two flips on the way to A |
/// | 8 H | U+E008 | `png ` at 10 only |
/// | 9 I | U+E009 | `dupe` to itself |
/// | 10 J | U+E00A | `dupe` to A |
/// | 11 K | U+E00B | `dupe` to C: a dupe, then a flip |
/// | 12 L | U+E00C | `flip` to itself |
/// | 13 M | U+E00D | `png ` at 20 and 52 only |
/// | 14 N | U+E00E | `png ` in every strike, at offsets (5, -4) |
/// | 15 P | U+E00F | `flip` to N, at offsets (1, 1) N does not have |
/// | 16 Q | U+E010 | `png ` everywhere, and every one's zlib stream invalid |
/// | 17-26 R1-R10 | U+E011-U+E01A | R*k* names R*k+1*, R10 names A; odd *k* `flip`, even `dupe` |
/// | 27 T | U+E01B | `png ` everywhere: a ramp, red across and blue down |
/// | 28 U | U+E01C | `flip` to T |
/// | 29 S | U+E01D | `png ` everywhere: T's ramp stored mirrored |
///
/// M is held in two strikes either side of 26, so from 26 px to under 36 px
/// the nearest holder is 20 while the smallest at or above is 52. R1 to R10
/// are one chain of ten redirections that alternate kinds: R1 needs eleven
/// records to reach its image and R2 ten, so the bound -- ten records,
/// shared by both kinds -- refuses R1 and draws R2. T's image has no
/// symmetry and S holds it mirrored, so U, mirrored by the renderer, must
/// draw as S does at every size: the mirror is of the image the font names,
/// before any scaling.
///
/// Every record but C's, N's and P's sits at offsets (0, 0); every image is
/// ppem x ppem. The corrupt PNGs' chunks carry correct CRCs, so they parse
/// and only their inflate fails. `head`, `hhea`, `maxp`, `hmtx`, `cmap`
/// (format 12) and `post` (version 3) are written from scratch; there are
/// no outlines.
const SBIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts/sbix-strikes.ttf");

const SBIX_STRIKES: [u16; 6] = [10, 20, 26, 40, 52, 80];
const SBIX_GLYPHS: u16 = 30;
const SBIX_WHITE: [u8; 3] = [240, 240, 240];

/// A zlib header, then a block whose type is the reserved 0b11: the seven
/// unreadable streams, written as they are.
const SBIX_CORRUPT: [u8; 6] = [0x78, 0x9c, 0xff, 0xff, 0xff, 0xff];

fn sbix_colour(ppem: u16) -> [u8; 3] {
    match ppem {
        10 => [220, 40, 40],
        20 => [40, 200, 40],
        26 => [200, 60, 200],
        40 => [40, 80, 230],
        52 => [40, 200, 200],
        80 => [230, 210, 30],
        _ => unreachable!("{ppem} is no strike"),
    }
}

fn sbix_png(ppem: u16, corrupt: bool, z: &Payloads) -> Vec<u8> {
    let ppem32 = u32::from(ppem);
    if corrupt {
        return writer::png_rgba8(ppem32, &SBIX_CORRUPT);
    }
    let rows = writer::halves(ppem32, sbix_colour(ppem), SBIX_WHITE);
    writer::png_rgba8(ppem32, &z.deflate(&rows))
}

/// Red rising across, green falling across, blue rising down: no two
/// texels of a row alike, and no row like another.
fn sbix_ramp(ppem: u16, mirrored: bool, z: &Payloads) -> Vec<u8> {
    let top = u32::from(ppem) - 1;
    let mut rows = Vec::new();
    for y in 0..=top {
        rows.push(0);
        for x in 0..=top {
            let u = if mirrored { top - x } else { x };
            let level = |v: u32| u8::try_from(v * 255 / top).unwrap();
            rows.extend_from_slice(&[level(u), 255 - level(u), level(y), 255]);
        }
    }
    writer::png_rgba8(u32::from(ppem), &z.deflate(&rows))
}

fn sbix_write(kind: &[u8; 4], data: &[u8], x: i16, y: i16) -> Option<Vec<u8>> {
    Some(Be::default().i16(x).i16(y).bytes(kind).bytes(data).done())
}

fn glyph_ref(g: u16) -> [u8; 2] {
    g.to_be_bytes()
}

/// The record each glyph holds in this strike, or `None` for nothing.
fn sbix_glyph_records(ppem: u16, z: &Payloads) -> Vec<Option<Vec<u8>>> {
    let png = |corrupt| sbix_png(ppem, corrupt, z);
    let mut out = vec![None; usize::from(SBIX_GLYPHS)];
    out[1] = sbix_write(b"png ", &png(false), 0, 0);
    out[2] = if ppem == 80 {
        sbix_write(b"png ", &png(false), 0, 0)
    } else {
        None
    };
    out[3] = sbix_write(b"flip", &glyph_ref(1), 7, 3);
    out[4] = sbix_write(b"flip", &glyph_ref(10), 0, 0);
    out[5] = sbix_write(b"png ", &png(ppem == 40), 0, 0);
    out[7] = sbix_write(b"flip", &glyph_ref(3), 0, 0);
    out[8] = if ppem == 10 {
        sbix_write(b"png ", &png(false), 0, 0)
    } else {
        None
    };
    out[9] = sbix_write(b"dupe", &glyph_ref(9), 0, 0);
    out[10] = sbix_write(b"dupe", &glyph_ref(1), 0, 0);
    out[11] = sbix_write(b"dupe", &glyph_ref(3), 0, 0);
    out[12] = sbix_write(b"flip", &glyph_ref(12), 0, 0);
    out[13] = if ppem == 20 || ppem == 52 {
        sbix_write(b"png ", &png(false), 0, 0)
    } else {
        None
    };
    out[14] = sbix_write(b"png ", &png(false), 5, -4);
    out[15] = sbix_write(b"flip", &glyph_ref(14), 1, 1);
    out[16] = sbix_write(b"png ", &png(true), 0, 0);
    for k in 1..=10u16 {
        let kind = if k % 2 == 1 { b"flip" } else { b"dupe" };
        let to = if k < 10 { 17 + k } else { 1 };
        out[usize::from(16 + k)] = sbix_write(kind, &glyph_ref(to), 0, 0);
    }
    out[27] = sbix_write(b"png ", &sbix_ramp(ppem, false, z), 0, 0);
    out[28] = sbix_write(b"flip", &glyph_ref(27), 0, 0);
    out[29] = sbix_write(b"png ", &sbix_ramp(ppem, true, z), 0, 0);
    out
}

fn sbix_table(z: &Payloads) -> Vec<u8> {
    let mut strikes = Vec::new();
    for ppem in SBIX_STRIKES {
        let head = 4 + 4 * (usize::from(SBIX_GLYPHS) + 1);
        let mut strike = Be::default();
        strike.u16(ppem).u16(72);
        let mut data = Vec::new();
        for r in sbix_glyph_records(ppem, z) {
            strike.n32(head + data.len());
            data.extend(r.unwrap_or_default());
        }
        strike.n32(head + data.len()).bytes(&data);
        strikes.push(strike.done());
    }
    let mut out = Be::default();
    out.u16(1).u16(1).n32(SBIX_STRIKES.len());
    let mut at = 8 + 4 * SBIX_STRIKES.len();
    for s in &strikes {
        out.n32(at);
        at += s.len();
    }
    for s in &strikes {
        out.bytes(s);
    }
    out.done()
}

/// The whole of `sbix-strikes.ttf`, its zlib streams from `z`.
fn sbix_font(z: &Payloads) -> Vec<u8> {
    let groups: Vec<_> = (1..u32::from(SBIX_GLYPHS))
        .map(|g| (0xE000 + g, 0xE000 + g, g))
        .collect();
    writer::sfnt(vec![
        (*b"cmap", writer::cmap(&groups)),
        (*b"head", writer::head()),
        (*b"hhea", writer::hhea(SBIX_GLYPHS)),
        (*b"hmtx", writer::hmtx(SBIX_GLYPHS)),
        (*b"maxp", writer::maxp(SBIX_GLYPHS)),
        (*b"post", writer::post()),
        (*b"sbix", sbix_table(z)),
    ])
}

fn sbix_payloads() -> (Vec<u8>, Payloads) {
    let committed = std::fs::read(SBIX).unwrap();
    let mut z = Payloads::new();
    z.add(&committed);
    (committed, z)
}

#[test]
fn the_sbix_fixture_is_what_its_generator_writes() {
    // The generator's table is the fixture's documentation, so the
    // committed bytes must be what it writes; otherwise the table describes
    // another file. Every byte is the generator's but the 33 valid zlib
    // streams, which must inflate to its rows (`writer`).
    let (committed, z) = sbix_payloads();
    assert_eq!(z.valid, 33, "valid streams");
    assert_eq!(z.invalid, vec![SBIX_CORRUPT.to_vec(); 7], "invalid streams");
    writer::assert_written("sbix-strikes.ttf", &sbix_font(&z), &committed, &z);
}

#[test]
fn a_changed_row_or_byte_fails_the_sbix_generator_check() {
    // Positive controls for the check above: one generated row changed in
    // one byte finds no committed stream, and one byte of the file outside
    // every stream -- strike 20's ppem -- no longer compares.
    let (committed, z) = sbix_payloads();
    let z = z.perturbing(5);
    let fresh = sbix_font(&z);
    assert_eq!(z.missed(), 1);
    assert!(fresh != committed);
    let (_, z) = sbix_payloads();
    let fresh = sbix_font(&z);
    assert_eq!((z.missed(), &fresh), (0, &committed));
    let sbix = table_at(&committed, b"sbix");
    let strike = sbix
        + usize::try_from(u32::from_be_bytes(
            committed[sbix + 12..sbix + 16].try_into().unwrap(),
        ))
        .unwrap();
    let mut moved = committed.clone();
    moved[strike + 1] ^= 1;
    assert_eq!(&committed[strike..][..2], &[0, 20]);
    assert!(fresh != moved);
}

/// baud's decode of one PNG, as its size and RGBA8.
fn ours(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), PngError> {
    let png = Png::parse(bytes)?;
    let mut raw = vec![0u8; png.raw_len().ok_or(PngError::Buffer)?];
    let mut rgba = vec![0u8; png.rgba_len().ok_or(PngError::Buffer)?];
    let size = png.decode(&mut raw, &mut rgba)?;
    Ok((size.width, size.height, rgba))
}

fn adam7_fixtures() -> Vec<PathBuf> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/png");
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with("-i.png"))
        .collect();
    all.sort();
    all
}

#[test]
fn every_interlaced_fixture_decodes_as_its_twin_and_the_oracle() {
    // `ADAM7_DIR`: each picture with interlace method 1
    // and again with method 0, every colour type and depth PNG allows, at
    // 1x1, 2x3, 7x9 and 33x17. PNG 2003 13.10: "Decoders are required to
    // be able to read interlaced images." The two baud decodes and the two
    // oracle decodes must all be one RGBA image.
    let mut kinds = std::collections::BTreeSet::new();
    let mut sizes = std::collections::BTreeSet::new();
    let mut pairs = 0;
    for path in adam7_fixtures() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let laced = std::fs::read(&path).unwrap();
        let twin = std::fs::read(path.with_file_name(name.replace("-i.png", "-n.png"))).unwrap();
        // IHDR's interlace byte: the fixture is what its name says.
        assert_eq!((laced[28], twin[28]), (1, 0), "{name}");
        kinds.insert((laced[25], laced[24]));
        let (w, h, a) = ours(&laced).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let (_, _, b) = ours(&twin).unwrap_or_else(|e| panic!("{name} twin: {e:?}"));
        let (ow, oh, c) = oracle_rgba(&laced);
        let (_, _, d) = oracle_rgba(&twin);
        sizes.insert((w, h));
        assert_eq!((w, h), (ow, oh), "{name}: size");
        assert!(c == d, "{name}: the oracle reads the twins differently");
        assert!(
            b == d,
            "{name}: the non-interlaced twin differs from the oracle"
        );
        assert!(
            a == c,
            "{name}: the interlaced decode differs from the oracle"
        );
        pairs += 1;
    }
    // Positive controls: all fifteen (colour type, depth) pairs of table
    // 11.1, and the sizes that leave passes empty.
    let allowed = [
        (0, 1),
        (0, 2),
        (0, 4),
        (0, 8),
        (0, 16),
        (2, 8),
        (2, 16),
        (3, 1),
        (3, 2),
        (3, 4),
        (3, 8),
        (4, 8),
        (4, 16),
        (6, 8),
        (6, 16),
    ];
    assert_eq!(kinds.into_iter().collect::<Vec<_>>(), allowed);
    assert_eq!(
        sizes.into_iter().collect::<Vec<_>>(),
        [(1, 1), (2, 3), (7, 9), (33, 17)]
    );
    assert_eq!(pairs, 60);
}

#[test]
fn an_interlaced_scratch_counts_each_pass_its_own_filter_bytes() {
    // 2x3, 8-bit grey: passes 2 and 4 have no columns and pass 3 no rows,
    // so they carry no filter byte (9.1); 1 has one pixel, 5 one, 6 two rows
    // of one and 7 one row of two. 2 + 2 + 4 + 3 = 11, against 3 x 3 = 9.
    let raw = |f: &str| {
        let bytes = std::fs::read(format!("{}/tests/png/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        Png::parse(&bytes).unwrap().raw_len().unwrap()
    };
    assert_eq!(raw("g8-2x3-i.png"), 11);
    assert_eq!(raw("g8-2x3-n.png"), 9);
    // 33x17 at one bit: each reduced scanline is padded to a byte of its
    // own (7.2). Passes 1 to 7 are 5x3, 4x3, 9x2, 8x5, 17x4, 16x9 and
    // 33x8 pixels: 6 + 6 + 6 + 10 + 16 + 27 + 48 = 119, against 6 x 17.
    assert_eq!(raw("g1-33x17-i.png"), 119);
    assert_eq!(raw("g1-33x17-n.png"), 102);
    assert_eq!(raw("rgb8-1x1-i.png"), 4);
}

#[test]
fn an_interlace_method_past_adam7_is_a_header_error() {
    // 15.2.3: "Unexpected values in fields of known chunks ... are treated
    // as errors." Methods 0 and 1 are the two 8.2 defines.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/png/g8-2x3-i.png");
    let mut png = std::fs::read(path).unwrap();
    png[28] = 2;
    let crc = {
        let mut c = 0xFFFF_FFFFu32;
        for &b in &png[12..29] {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    };
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    assert_eq!(Png::parse(&png).err(), Some(PngError::Header));
    png[28] = 1;
    assert!(
        Png::parse(&png).is_err(),
        "the CRC was recomputed for 2, not 1"
    );
}

// ------------------------------------------------------------------ tests/png/

/// **`tests/png/`: interlaced PNGs and their non-interlaced twins, every
/// colour type and bit depth PNG allows**, written by [`adam7_files`] for
/// the Adam7 differential above.
///
/// Each picture is written twice from the same samples, once with
/// interlace method 1 (Adam7) and once with method 0, so a decoder's two
/// paths must agree with each other as well as with the oracle. The encoder
/// is this file's own, from `[[source: w3c-png-2003]]`: the pass lattice
/// from 8.2's 8-by-8 pattern, scanlines packed and padded per reduced image
/// (7.2), no filter byte for an empty pass (9.1, 13.10), and the prior
/// scanline taken as zero at the start of every reduced image (9.2).
///
/// The sizes leave passes empty: 1x1 has only pass 1; 2x3 has passes with
/// no columns (2 and 4) and one with no rows (3); 7x9 and 33x17 have every
/// pass, 33 leaving a partial byte below 8 bits. The first scanline of every
/// pass after the first non-empty one is filtered Up, Average or Paeth, so a
/// decoder that carries the previous pass's last row in as "prior" decodes
/// it wrongly; the generator counts the rows where that happens on a
/// would-be prior that is non-zero. The 7x9 pictures carry `tRNS`, and split
/// their zlib stream across `IDAT` chunks of five bytes, mid-pass; the 33x17
/// ones split it every 97 bytes.
const ADAM7_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/png");

/// 8.2, as the specification prints it.
const ADAM7_PATTERN: [[u8; 8]; 8] = [
    [1, 6, 4, 6, 2, 6, 4, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [5, 6, 5, 6, 5, 6, 5, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [3, 6, 4, 6, 3, 6, 4, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [5, 6, 5, 6, 5, 6, 5, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
];

/// (name, colour type, channels, depths): 11.2.2, table 11.1.
const ADAM7_COLOURS: [(&str, u8, usize, &[u8]); 5] = [
    ("g", 0, 1, &[1, 2, 4, 8, 16]),
    ("rgb", 2, 3, &[8, 16]),
    ("p", 3, 1, &[1, 2, 4, 8]),
    ("ga", 4, 2, &[8, 16]),
    ("rgba", 6, 4, &[8, 16]),
];
const ADAM7_SIZES: [(usize, usize); 4] = [(1, 1), (2, 3), (7, 9), (33, 17)];

/// (x0, y0, dx, dy) of pass `p`, read off the pattern.
fn adam7_lattice(p: u8) -> (usize, usize, usize, usize) {
    let cells: Vec<(usize, usize)> = (0..8)
        .flat_map(|r| (0..8).map(move |c| (r, c)))
        .filter(|&(r, c)| ADAM7_PATTERN[r][c] == p)
        .collect();
    let (y0, x0) = cells[0];
    let cols: std::collections::BTreeSet<usize> =
        cells.iter().filter(|c| c.0 == y0).map(|c| c.1).collect();
    let rows: std::collections::BTreeSet<usize> =
        cells.iter().filter(|c| c.1 == x0).map(|c| c.0).collect();
    let step = |s: &std::collections::BTreeSet<usize>| {
        let v: Vec<usize> = s.iter().copied().collect();
        if v.len() > 1 { v[1] - v[0] } else { 8 }
    };
    (x0, y0, step(&cols), step(&rows))
}

/// xorshift32.
struct Xorshift(u32);

impl Xorshift {
    fn new(seed: u32) -> Self {
        Self(if seed == 0 { 1 } else { seed })
    }
    fn next(&mut self) -> u32 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        s
    }
}

type Pixels = Vec<Vec<Vec<u32>>>;

/// One scanline, samples big-endian, sub-byte samples high bits first, the
/// last byte's unused low bits zero (7.2).
fn adam7_pack(row: &[Vec<u32>], depth: u8) -> Vec<u8> {
    let samples = row.iter().flatten().copied();
    let byte = |v: u32| u8::try_from(v).unwrap();
    match depth {
        16 => samples
            .flat_map(|s| u16::try_from(s).unwrap().to_be_bytes())
            .collect(),
        8 => samples.map(byte).collect(),
        _ => {
            let (mut out, mut acc, mut n) = (Vec::new(), 0u32, 0u8);
            for s in samples {
                acc = (acc << depth) | s;
                n += depth;
                if n == 8 {
                    out.push(byte(acc));
                    (acc, n) = (0, 0);
                }
            }
            if n > 0 {
                out.push(byte(acc << (8 - n)));
            }
            out
        }
    }
}

fn adam7_paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// One filtered scanline: its filter type byte, then each byte less its
/// prediction (9.2).
fn adam7_filter(kind: u8, line: &[u8], prior: &[u8], bpp: usize) -> Vec<u8> {
    let mut out = vec![kind];
    for (x, &o) in line.iter().enumerate() {
        let a = if x >= bpp { line[x - bpp] } else { 0 };
        let b = prior[x];
        let c = if x >= bpp { prior[x - bpp] } else { 0 };
        let pred = match kind {
            0 => 0,
            1 => a,
            2 => b,
            3 => u8::try_from((u16::from(a) + u16::from(b)) / 2).unwrap(),
            _ => adam7_paeth(a, b, c),
        };
        out.push(o.wrapping_sub(pred));
    }
    out
}

/// The reduced images a picture is stored as: itself, or Adam7's non-empty
/// passes.
fn adam7_reduced(px: &Pixels, interlace: u8) -> Vec<Pixels> {
    if interlace == 0 {
        return vec![px.clone()];
    }
    let (w, h) = (px[0].len(), px.len());
    (1..=7)
        .filter_map(|p| {
            let (x0, y0, dx, dy) = adam7_lattice(p);
            let rows: Pixels = (y0..h)
                .step_by(dy)
                .map(|y| (x0..w).step_by(dx).map(|x| px[y][x].clone()).collect())
                .collect();
            (!rows.is_empty() && !rows[0].is_empty()).then_some(rows)
        })
        .collect()
}

/// The filtered scanlines, counting in `reset` each pass start whose
/// wrongly carried prior would have been non-zero.
fn adam7_encode(px: &Pixels, depth: u8, interlace: u8, reset: &mut usize) -> Vec<u8> {
    let bpp = (usize::from(depth) * px[0][0].len() / 8).max(1);
    let mut data = Vec::new();
    let mut previous_last: Option<Vec<u8>> = None;
    for (k, image) in adam7_reduced(px, interlace).iter().enumerate() {
        let lines: Vec<Vec<u8>> = image.iter().map(|r| adam7_pack(r, depth)).collect();
        let mut prior = vec![0u8; lines[0].len()];
        for (j, line) in lines.iter().enumerate() {
            let kind = if j == 0 && k > 0 {
                if previous_last
                    .as_ref()
                    .is_some_and(|l| l.iter().any(|&b| b != 0))
                {
                    *reset += 1;
                }
                2 + k % 3
            } else {
                (j + k) % 5
            };
            data.extend(adam7_filter(u8::try_from(kind).unwrap(), line, &prior, bpp));
            prior.clone_from(line);
        }
        previous_last = lines.last().cloned();
    }
    data
}

/// A PNG: `ihdr`, the chunks that go before the image data, and `stream`
/// split into `IDAT`s of `split` bytes.
fn adam7_png(ihdr: &[u8], before: &[u8], stream: &[u8], split: Option<usize>) -> Vec<u8> {
    let mut out = writer::SIGNATURE.to_vec();
    out.extend(writer::chunk(b"IHDR", ihdr));
    out.extend_from_slice(before);
    for part in stream.chunks(split.unwrap_or(stream.len())) {
        out.extend(writer::chunk(b"IDAT", part));
    }
    out.extend(writer::chunk(b"IEND", b""));
    out
}

/// Every file of `tests/png/`, by name, and the pass starts that test the
/// prior reset.
fn adam7_files(z: &Payloads) -> (Vec<(String, Vec<u8>)>, usize) {
    let mut reset = 0;
    let mut seed = 0x9E37_79B9u32;
    let mut out = Vec::new();
    for (name, ctype, channels, depths) in ADAM7_COLOURS {
        for &depth in depths {
            for (w, h) in ADAM7_SIZES {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let mut rng = Xorshift::new(seed);
                let entries = (1u32 << depth).min(200);
                let top = if ctype == 3 { entries } else { 1 << depth };
                let px: Pixels = (0..h)
                    .map(|_| {
                        (0..w)
                            .map(|_| (0..channels).map(|_| rng.next() % top).collect())
                            .collect()
                    })
                    .collect();
                let mut before = Vec::new();
                if ctype == 3 {
                    let plte: Vec<u8> = (0..entries * 3)
                        .map(|_| rng.next().to_le_bytes()[0])
                        .collect();
                    before.extend(writer::chunk(b"PLTE", &plte));
                }
                let sample = |s: u32| u16::try_from(s).unwrap().to_be_bytes();
                let trns: Option<Vec<u8>> = match ctype {
                    _ if (w, h) != (7, 9) => None,
                    3 => Some(
                        (0..(entries / 2).max(1))
                            .map(|_| rng.next().to_le_bytes()[0])
                            .collect(),
                    ),
                    0 => Some(sample(px[0][0][0]).to_vec()),
                    2 => Some(px[0][0].iter().flat_map(|&s| sample(s)).collect()),
                    _ => None,
                };
                if let Some(trns) = trns {
                    before.extend(writer::chunk(b"tRNS", &trns));
                }
                let split = match (w, h) {
                    (7, 9) => Some(5),
                    (33, 17) => Some(97),
                    _ => None,
                };
                for (interlace, tag) in [(1, "i"), (0, "n")] {
                    let raw = adam7_encode(&px, depth, interlace, &mut reset);
                    let ihdr = Be::default()
                        .n32(w)
                        .n32(h)
                        .bytes(&[depth, ctype, 0, 0, interlace])
                        .done();
                    out.push((
                        format!("{name}{depth}-{w}x{h}-{tag}.png"),
                        adam7_png(&ihdr, &before, &z.deflate(&raw), split),
                    ));
                }
            }
        }
    }
    (out, reset)
}

fn adam7_committed() -> (Vec<(String, Vec<u8>)>, Payloads) {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(ADAM7_DIR)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    let mut z = Payloads::new();
    for (_, bytes) in &files {
        z.add(bytes);
    }
    (files, z)
}

#[test]
fn the_adam7_fixtures_are_what_their_generator_writes() {
    // Every byte is the generator's but the 120 zlib streams, one a file,
    // which must inflate to its filtered scanlines (`writer`).
    let (committed, z) = adam7_committed();
    assert_eq!((z.valid, z.invalid.len()), (120, 0));
    let (mut fresh, reset) = adam7_files(&z);
    fresh.sort();
    let names = |v: &[(String, Vec<u8>)]| v.iter().map(|f| f.0.clone()).collect::<Vec<_>>();
    assert_eq!(
        names(&committed),
        names(&fresh),
        "the generator writes another set of files"
    );
    let wrong: Vec<&str> = committed
        .iter()
        .zip(&fresh)
        .filter(|(a, b)| a.1 != b.1)
        .map(|(a, _)| a.0.as_str())
        .collect();
    if !wrong.is_empty() || z.missed() > 0 {
        let out = std::env::temp_dir().join(format!("baud-{}-png", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        for (name, bytes) in &fresh {
            std::fs::write(out.join(name), bytes).unwrap();
        }
        panic!(
            "{} file(s) are not what the generator writes ({wrong:?}), and {} scanline \
             buffer(s) match no committed stream; the generator's files are in {}",
            wrong.len(),
            z.missed(),
            out.display()
        );
    }
    assert_eq!(committed.len(), 120);
    // Every pass after the first starts on a row filtered against "prior";
    // 221 of those would have been non-zero had the last pass's row been
    // carried in.
    assert_eq!(reset, 221);
}

#[test]
fn a_changed_scanline_or_byte_fails_the_adam7_generator_check() {
    // Positive controls: one scanline buffer changed in its last byte finds
    // no committed stream, and one interlace byte flipped in a committed
    // header no longer compares.
    let (committed, z) = adam7_committed();
    let z = z.perturbing(17);
    let (mut fresh, _) = adam7_files(&z);
    fresh.sort();
    assert_eq!(z.missed(), 1);
    assert!(fresh != committed);
    let (committed, z) = adam7_committed();
    let (mut fresh, _) = adam7_files(&z);
    fresh.sort();
    assert!(fresh == committed && z.missed() == 0);
    let mut moved = committed.clone();
    moved[0].1[28] ^= 1;
    assert!(fresh != moved);
}

#[test]
fn the_interlaced_system_pngs_agree_with_the_oracle() {
    // macOS 27.0 ships 155 interlaced PNGs under /System/Library and
    // /Applications, every one 8-bit (149 RGBA, 5 RGB, 1 palette); these
    // two directories hold 137 of them. Every one found must agree; the
    // count is a positive control only where Skyglow, the interlaced PNG
    // `baud-bench/src/capture/kitten.rs` pins, has its macOS 27.0 bytes, so
    // another release that moves the files cannot fail it.
    if !cfg!(target_os = "macos") {
        return;
    }
    let skyglow = "/System/Library/PrivateFrameworks/GeoKit.framework/Versions/A/\
                   Resources/UTC-08.00-Skyglow@2x.png";
    let pinned = std::fs::read(skyglow).is_ok_and(|b| {
        sha256::sha256_hex(&b) == "e035d914fa659efae67108035fa97d31b756fc71cce807e7844d475702e74d7f"
    });
    let dirs = [
        "/System/Library/PrivateFrameworks/GeoKit.framework/Versions/A/Resources",
        "/System/Library/PrivateFrameworks/QuickLookSupport.framework/Versions/A/Resources",
    ];
    let mut decoded = 0;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries {
            let path = e.unwrap().path();
            if path.extension().is_none_or(|x| x != "png") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            if bytes.get(28) != Some(&1) || bytes.get(12..16) != Some(b"IHDR") {
                continue;
            }
            let (w, h, a) = ours(&bytes).unwrap_or_else(|e| panic!("{path:?}: {e:?}"));
            let (ow, oh, c) = oracle_rgba(&bytes);
            assert_eq!((w, h), (ow, oh), "{path:?}: size");
            assert!(a == c, "{path:?}: RGBA bytes differ");
            decoded += 1;
        }
    }
    if pinned {
        assert!(decoded >= 130, "{decoded}");
    }
}
