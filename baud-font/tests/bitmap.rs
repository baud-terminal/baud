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
//! `BungeeTint-Regular.ttf`, a `COLR` v0 face with eight palettes, and
//! `sbix-strikes.ttf`, six strikes holding every per-glyph case
//! (`scripts/gen-sbix-fixture.py` lists them), seven unreadable PNGs among
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
    interlaced_refused: usize,
    refused_by_both: Counted,
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
    let size = match png.decode(&mut raw, &mut rgba) {
        Ok(size) => size,
        Err(e) => {
            // A strike may carry a PNG nobody can read; `sbix-strikes.ttf`
            // does on purpose. The oracle must refuse it too.
            let mut dec = png::Decoder::new(bytes);
            dec.set_transformations(png::Transformations::EXPAND);
            let theirs = dec.read_info().and_then(|mut r| {
                let mut buf = vec![0u8; r.output_buffer_size()];
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
        // 108 people facing right in each of nine strikes, in both of
        // Apple Color Emoji's faces, and the fixture's 84.
        assert!(t.sbix_flips.0 >= 1_944 + 84, "{:?}", t.sbix_flips);
        assert!(t.pngs_decoded >= 30_000, "{}", t.pngs_decoded);
    }
    assert_eq!(t.interlaced_refused, 0);
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

#[test]
fn the_sbix_fixture_answers_every_case_in_every_strike() {
    // `scripts/gen-sbix-fixture.py`: strikes at 10, 20, 26, 40, 52 and 80 ppem.
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

#[test]
fn the_sbix_fixture_is_what_its_generator_writes() {
    // The generator's table is the fixture's documentation, so the committed
    // bytes must be what it writes; otherwise the table describes another
    // file. Both Pythons on the development Mac write the same bytes; a
    // zlib other than Mark Adler's (zlib-ng) deflates differently and would
    // fail this without the fixture having changed.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::env::temp_dir().join(format!("baud-sbix-{}.ttf", std::process::id()));
    let ran = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-sbix-fixture.py"))
        .arg(&out)
        .status()
        .expect("python3 to run the generator");
    assert!(ran.success(), "gen-sbix-fixture.py failed");
    let fresh = std::fs::read(&out).unwrap();
    std::fs::remove_file(&out).unwrap();
    let committed =
        std::fs::read(format!("{root}/baud-font/tests/fonts/sbix-strikes.ttf")).unwrap();
    assert!(
        fresh == committed,
        "sbix-strikes.ttf is not what scripts/gen-sbix-fixture.py writes: regenerate it \
         and re-pin font-sbix-strikes in MANIFEST.toml"
    );
}
