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
//! implements (`scripts/gen-cbdt-fixture.py` says which glyphs are which),
//! and `BungeeTint-Regular.ttf`, a `COLR` v0 face with eight palettes. On
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

use baud_domain::GlyphId;
use baud_font::bytes::Index;
use baud_font::{Face, FaceIndex, Palette, PaletteEntry, Png, PngError, Rgba, Strike, faces};
use std::path::{Path, PathBuf};
use ttf_parser::RgbaColor;
use ttf_parser::colr::{ClipBox, CompositeMode, Paint, Painter};

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

#[derive(Default, Debug)]
struct Tally {
    faces: usize,
    strikes: usize,
    sbix_bitmaps: usize,
    cbdt_bitmaps: usize,
    absent_agreed: usize,
    cbdt_y_translated: usize,
    pngs_decoded: usize,
    pixels: usize,
    interlaced_refused: usize,
    layered_glyphs: usize,
    layers_agreed: usize,
    colr_v1_only: usize,
    palettes: usize,
    colors_agreed: usize,
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
    let mut dec = png::Decoder::new(bytes);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().expect("the oracle reads the PNG");
    let mut buf = vec![0u8; reader.output_buffer_size()];
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
    let png = match Png::parse(bytes) {
        Ok(p) => p,
        Err(PngError::Interlaced) => {
            t.interlaced_refused += 1;
            return;
        }
        Err(e) => panic!("{what}: baud refuses a PNG the strike carries: {e:?}"),
    };
    let mut raw = vec![0u8; png.raw_len().expect("a strike PNG has a bounded size")];
    let mut rgba = vec![0u8; png.rgba_len().unwrap()];
    let size = png
        .decode(&mut raw, &mut rgba)
        .unwrap_or_else(|e| panic!("{what}: {e:?}"));
    let (w, h, theirs) = oracle_rgba(bytes);
    assert_eq!((size.width, size.height), (w, h), "{what}: size");
    assert!(rgba == theirs, "{what}: RGBA bytes differ");
    t.pngs_decoded += 1;
    t.pixels += rgba.len() / 4;
}

fn compare_face(path: &Path, data: &[u8], i: u32, t: &mut Tally) {
    let Ok(ours) = Face::parse_index(data, FaceIndex::of(i)) else {
        return;
    };
    let Ok(theirs) = ttf_parser::Face::parse(data, i) else {
        return;
    };
    t.faces += 1;
    let what = format!("{}#{i}", path.display());
    let glyphs = u32::from(ours.glyph_count().value());

    // Strikes: every glyph of every strike, by bytes.
    for s in 0..ours.strikes().value() {
        t.strikes += 1;
        let s = Index::of(s);
        let ppem = ours
            .strike_ppem(s)
            .unwrap_or_else(|| panic!("{what}: strike {s:?} has no ppem"));
        for g in 0..glyphs {
            let gid = GlyphId::from_shaper(u16::try_from(g).unwrap());
            let mine = ours.bitmap(s, gid);
            let img = theirs
                .glyph_raster_image(ttf_parser::GlyphId(u16::try_from(g).unwrap()), ppem.value());
            match (mine, img) {
                (None, None) => t.absent_agreed += 1,
                (Some(b), Some(img)) => {
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
                (mine, img) => panic!(
                    "{what} g{g} @{}: baud {:?}, oracle {:?}",
                    ppem.value(),
                    mine.map(|b| b.placement),
                    img.map(|i| (i.x, i.y, i.width, i.height))
                ),
            }
        }
    }

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

#[test]
fn every_strike_layer_and_pixel_agrees_with_the_oracles() {
    let mut t = Tally::default();
    let mut files = 0;
    for path in corpus() {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let n = faces(&data).map(|n| n.value()).unwrap_or(1);
        for i in 0..n {
            compare_face(&path, &data, i, &mut t);
        }
        files += 1;
    }
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
    // PNGs, none interlaced.
    if cfg!(target_os = "macos") {
        assert!(t.strikes >= 10, "{}", t.strikes);
        assert!(t.sbix_bitmaps >= 30_000, "{}", t.sbix_bitmaps);
        assert!(t.pngs_decoded >= 30_000, "{}", t.pngs_decoded);
    }
    assert_eq!(t.interlaced_refused, 0);
    assert_eq!(
        t.colr_v1_only, 0,
        "a COLR v1-only face joined the corpus; count it here"
    );
}

#[test]
fn the_subset_fixture_answers_every_arrangement_the_generator_wrote() {
    // `scripts/gen-cbdt-fixture.py` lays glyphs 4..45 out across index
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
