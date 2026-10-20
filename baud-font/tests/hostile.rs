//! The hostile-input contract: no byte string panics.
//!
//! A font file is the same class of input as bytes from a pty -- whatever the
//! other end sends -- and the crate is `forbid(unsafe_code)`, so the only
//! failure a malformed file can produce is a panic, and the property here is
//! that it does not. Two generators: arbitrary bytes, which exercise the
//! directory and the version checks, and a real face with a handful of bytes
//! flipped, which reaches the table parsers with offsets and counts that lie.
//! The PNG decoder gets the same pair: arbitrary bytes, and a real strike
//! PNG mutated and truncated, because an inflater is the deepest state
//! machine in the crate and the one a hostile file can steer furthest.
//!
//! `PROPTEST_CASES=20000` before trusting a change here; the default is what
//! CI can afford.

use baud_domain::Codepoint;
use baud_domain::GlyphId;
use baud_font::bytes::Index;
use baud_font::{
    Cluster, Face, FaceIndex, Features, Glyph, Ignorable, Palette, Png, Shaper, faces,
};
use proptest::prelude::*;

const JETBRAINS: &[u8] = include_bytes!("fonts/JetBrainsMono-Regular.ttf");
const NOTO_SUBSET: &[u8] = include_bytes!("fonts/NotoColorEmoji-subset.ttf");
/// Six `sbix` strikes of `png `, `dupe` and `flip` records, loops and a
/// chain at the hop bound among them: a flipped byte lands in a redirection
/// or an offset, which is where a hop bound that misses a kind would spin.
const SBIX: &[u8] = include_bytes!("fonts/sbix-strikes.ttf");
/// A `CBLC` size for each way the lookup refuses or reads one but a nested
/// record, which `cblc.rs`'s surgeries and H1 cover (`scripts/gen-cblc-fixture.py`).
const CBLC_CASES: &[u8] = include_bytes!("bitmap/cblc-cases.ttf");
/// 2.5 KB of `gvar`-heavy font: a flipped byte lands in the variation data
/// far more often than in a real face.
const GVAR_CASES: &[u8] = include_bytes!("var/gvar-cases.ttf");
/// A hand-written `HVAR` and a phantom-point font: a flipped byte lands in
/// the item variation store, the maps and the composite chains.
const HVAR_CASES: &[u8] = include_bytes!("var/hvar/cases.ttf");
const PHANTOM_CASES: &[u8] = include_bytes!("var/hvar/phantom.ttf");
const JETBRAINS_VAR: &[u8] = include_bytes!("fonts/JetBrainsMono-Variable.ttf");
const BAKE_CASES: &[u8] = include_bytes!("var/bake/cases.ttf");
/// Composites placed by matched points and by scaled offsets, which the
/// default instance draws only through scratch: a flipped byte lands in a
/// point number, a flag word or a phantom's metrics.
const COMPONENT_CASES: &[u8] = include_bytes!("var/component-cases.ttf");

/// The two CFF2 faces: hints, flex, both subroutine kinds, two Font DICTs
/// and three data sets in 1.4 KB, and a real variable font's charstrings.
const CFF2_CASES: &[u8] = include_bytes!("var/cff2-cases.otf");
const SOURCE_CODE_VF: &[u8] = include_bytes!("fonts/SourceCodeVF-Upright.otf");

/// The positive control for the CFF2 properties: unmutated, both faces
/// parse and draw their `A` at the default and at a variation, so a flip
/// lands in a table the reader walks rather than one it refused at once.
#[test]
fn the_cff2_corpus_reaches_the_reader() {
    for data in [CFF2_CASES, SOURCE_CODE_VF] {
        let mut face = Face::parse(data).expect("parses");
        let g = face.glyph(Codepoint::new(0x41).unwrap()).expect("an A");
        assert_eq!(face.outline(g, &mut Null), baud_font::Drawn::Outline);
        face.set_variations(&[baud_font::var::Variation {
            tag: baud_font::Tag::from_bytes(*b"wght"),
            value: baud_font::var::Fixed(700 << 16),
        }]);
        assert!(!face.coords().is_default());
        assert_eq!(face.outline(g, &mut Null), baud_font::Drawn::Outline);
    }
}

/// Bakes `data` at two weights into a buffer that may be too small, and
/// then into one of the length it asked for; the answers are not the
/// property, not panicking is.
fn bake_any(data: &[u8]) {
    let mut scratch = vec![baud_font::BakePoint::ZERO; 1 << 12];
    for w in [250, 700] {
        let mut out = vec![0u8; 4096];
        let wght = baud_font::var::Fixed(w << 16);
        if let Err(baud_font::Refused::Out(need)) =
            baud_font::bake(data, wght, &mut scratch, &mut out)
        {
            let mut out = vec![0u8; usize::try_from(need.value()).unwrap()];
            let _ = baud_font::bake(data, wght, &mut scratch, &mut out);
        }
    }
}

/// Where table `tag` lies in an sfnt: its record's offset and length.
fn table_range(data: &[u8], tag: [u8; 4]) -> (usize, usize) {
    let be16 = |at: usize| usize::from(u16::from_be_bytes([data[at], data[at + 1]]));
    let be32 = |at: usize| {
        usize::try_from(u32::from_be_bytes(data[at..at + 4].try_into().unwrap())).unwrap()
    };
    (0..be16(4))
        .map(|i| 12 + 16 * i)
        .find(|&r| data[r..r + 4] == tag)
        .map(|r| (be32(r + 8), be32(r + 12)))
        .unwrap()
}

/// Every byte of `SBIX`'s `sbix` table but the two ramps' PNGs (glyphs 27
/// and 29), which are nine tenths of it and are the decoder's to survive --
/// `a_mutated_png_never_panics` reaches the decoder -- not the table's.
fn sbix_structure() -> Vec<usize> {
    let (start, len) = table_range(SBIX, *b"sbix");
    let be32 = |at: usize| {
        usize::try_from(u32::from_be_bytes(SBIX[at..at + 4].try_into().unwrap())).unwrap()
    };
    let mut ramps = Vec::new();
    for k in 0..be32(start + 4) {
        let strike = start + be32(start + 8 + 4 * k);
        for g in [27, 29] {
            ramps.push(strike + be32(strike + 4 + 4 * g)..strike + be32(strike + 4 + 4 * (g + 1)));
        }
    }
    let reach: Vec<usize> = (start..start + len)
        .filter(|i| !ramps.iter().any(|r| r.contains(i)))
        .collect();
    // The positive control: the ramps were found, and the rest is left.
    assert!(
        reach.len() > 4_000 && reach.len() < len / 5,
        "{}",
        reach.len()
    );
    reach
}

/// A sink that discards everything: the property is that emitting does not
/// panic, not what is emitted.
struct Null;

impl baud_font::Sink for Null {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {}
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
    fn close(&mut self) {}
}

/// Everything a caller can ask a face, on a fixed set of inputs.
fn exercise(data: &[u8]) {
    let n = faces(data).map(|n| n.value()).unwrap_or(1).min(8);
    for i in 0..n {
        let Ok(face) = Face::parse_index(data, FaceIndex::of(i)) else {
            continue;
        };
        let _ = face.units_per_em();
        let _ = face.line_metrics();
        let _ = face.typo_metrics();
        let _ = face.win_metrics();
        let _ = face.underline();
        let _ = face.pitch();
        if let Some(f) = face.family() {
            let _ = f.utf8(&mut [0u8; 64]);
            let _ = f.matches(b"JetBrains Mono");
        }
        let count = u32::from(face.glyph_count().value()).min(4096);
        for g in 0..count {
            let g = baud_domain::GlyphId::from_shaper(u16::try_from(g).unwrap());
            let _ = face.advance(g);
            let _ = face.side_bearing(g);
            let _ = face.outline(g, &mut Null);
            // A glyph that matches points needs scratch at the default
            // instance too, and the need must not be short.
            let need = face.outline_need(g).value();
            if need <= 1 << 16 {
                let mut scratch = vec![baud_font::VarPoint::EMPTY; usize::try_from(need).unwrap()];
                let drawn = face.outline_at(g, &mut scratch, &mut Null);
                assert!(
                    !matches!(drawn, baud_font::Drawn::NeedsScratch(_)),
                    "the default need was short: {need}"
                );
            }
        }
        for g in 0..count.min(256) {
            let _ = face.is_colour(baud_domain::GlyphId::from_shaper(u16::try_from(g).unwrap()));
        }
        let strikes = face.strikes().value().min(16);
        for st in 0..strikes {
            let st = Index::of(st);
            let _ = face.strike_ppem(st);
            let read = match face.strike(st) {
                Some(baud_font::StrikeState::Read(s)) => Some(s),
                _ => None,
            };
            for g in 0..count.min(256) {
                let g = baud_domain::GlyphId::from_shaper(u16::try_from(g).unwrap());
                let b = face.bitmap(st, g);
                if let Some(b) = b {
                    decode(b.png.as_slice());
                }
                // A checked strike answers what the per-call check answers.
                if let Some(s) = read {
                    let png =
                        |b: Option<baud_font::Bitmap<'_>>| b.map(|b| b.png.as_slice().to_vec());
                    assert_eq!(
                        png(face.bitmap_in(s, g)),
                        png(b),
                        "strike {}, glyph {}",
                        st.value(),
                        g.index()
                    );
                }
            }
        }
        for g in 0..count {
            let g = baud_domain::GlyphId::from_shaper(u16::try_from(g).unwrap());
            if let Some(layers) = face.layers(g) {
                for l in layers.take(64) {
                    let _ = face.color(Palette::FIRST, l.color);
                }
            }
        }
        let _ = face.palettes();
        shape_some(&face);
        for c in [
            0x20u32, 0x41, 0x7E, 0xE9, 0x4E2D, 0x1F600, 0x1F468, 0xFFFF, 0x10FFFF,
        ] {
            let Some(c) = Codepoint::new(c) else { continue };
            let _ = face.glyph(c);
            for vs in [0xFE0E_u32, 0xFE0F, 0xE0100] {
                let Some(vs) = Codepoint::new(vs) else {
                    continue;
                };
                let _ = face.variation(c, vs);
            }
        }
        varied(face, count);
    }
}

/// Every glyph again at a variation away from the default on both axes the
/// committed variable fonts use: the refusal, the need walk, and the varied
/// draw with exactly the scratch the need names -- the need being the one
/// number a caller sizes a buffer from, a hostile font must not make it
/// short. A need past a bound is refused, as a caller would.
fn varied(mut face: Face<'_>, count: u32) {
    const BOUND: u32 = 1 << 16;
    let at = |t: &[u8; 4], v: i32| baud_font::var::Variation {
        tag: baud_font::Tag::from_bytes(*t),
        value: baud_font::var::Fixed(v << 16),
    };
    face.set_variations(&[at(b"wght", 700), at(b"wdth", 150)]);
    // Mark anchors move at a variation: GDEF's store and GPOS's devices.
    shape_some(&face);
    for g in 0..count {
        let g = baud_domain::GlyphId::from_shaper(u16::try_from(g).unwrap());
        let _ = face.outline(g, &mut Null);
        let _ = face.advance(g);
        let need = face.outline_need(g).value();
        if need > BOUND {
            continue;
        }
        let mut scratch = vec![baud_font::VarPoint::EMPTY; usize::try_from(need).unwrap()];
        let drawn = face.outline_at(g, &mut scratch, &mut Null);
        assert!(
            !matches!(drawn, baud_font::Drawn::NeedsScratch(_)),
            "the need was short: {need}"
        );
    }
}

/// Shape a handful of clusters that reach every substitution path: a
/// ligature trigger, a composed pair, a ZWJ sequence, a variation
/// selector, and an unmapped character. The property is that no table
/// contents can make the shaper panic, not what it emits.
fn shape_some(face: &Face<'_>) {
    const CLUSTERS: &[&[u32]] = &[
        &[0x61],
        &[0x2D],
        &[0x3E],
        &[0x65, 0x0301],
        &[0x61, 0x0308, 0x0301],
        &[0x6F, 0x0323, 0x0302],
        &[0x1F468, 0x200D, 0x1F469],
        &[0x1F468, 0x200D, 0x1F469, 0x200D, 0x1F467],
        &[0x1F1EC, 0x1F1E7],
        &[0x31, 0xFE0F, 0x20E3],
        &[0x263A, 0xFE0F],
        &[0x10FFFF],
        &[0xAC00],
    ];
    // A varied face's glyph boxes need scratch: shaped once without, and
    // again with exactly the need the refusals named, which must then
    // refuse nothing -- the renderer's protocol, so a hostile font must not
    // make that need short. A need past a bound is left refused.
    const BOUND: u32 = 1 << 16;
    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 64];
    let mut scratch: Vec<baud_font::VarPoint> = Vec::new();
    for pass in 0..2 {
        let mut shaper = Shaper::new(face, &mut storage);
        shaper.scratch(&mut scratch);
        for (i, cps) in CLUSTERS.iter().enumerate() {
            let cps: Vec<Codepoint> = cps.iter().filter_map(|&v| Codepoint::new(v)).collect();
            let _ = shaper.push_cluster(&cps, Cluster(u16::try_from(i).unwrap()));
        }
        let (_, shaped) = shaper.finish(Features::GRID, baud_font::Script::COMMON);
        let need = shaped.ink_need.value();
        if shaped.ink_refused.is_zero() || need > BOUND {
            return;
        }
        assert_eq!(pass, 0, "a box refused with the scratch it named: {need}");
        scratch.resize(usize::try_from(need).unwrap(), baud_font::VarPoint::EMPTY);
    }
}

/// Decode a PNG the way a renderer would, with the size-bomb check the
/// register's hazard 7 asks for: an `IHDR` may claim any size, so the
/// buffers are refused above a bound rather than allocated on trust.
fn decode(bytes: &[u8]) {
    const BOUND: usize = 1 << 22;
    let Ok(png) = Png::parse(bytes) else { return };
    let _ = png.size();
    let (Some(raw), Some(rgba)) = (png.raw_len(), png.rgba_len()) else {
        return;
    };
    if raw > BOUND || rgba > BOUND {
        return;
    }
    let mut raw = vec![0u8; raw];
    let mut rgba = vec![0u8; rgba];
    let _ = png.decode(&mut raw, &mut rgba);
}

/// One real PNG to mutate: the first bitmap of the committed CBDT subset.
fn a_real_png() -> Vec<u8> {
    let face = Face::parse(NOTO_SUBSET).unwrap();
    face.bitmap(Index::ZERO, baud_domain::GlyphId::from_shaper(4))
        .expect("glyph 4 of the subset carries a bitmap")
        .png
        .as_slice()
        .to_vec()
}

proptest! {
    #[test]
    fn arbitrary_bytes_never_decode_into_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        decode(&bytes);
    }

    #[test]
    fn a_mutated_png_never_panics(
        at in 0usize..4096,
        byte in any::<u8>(),
        cut in 0usize..4096,
    ) {
        let mut png = a_real_png();
        let n = png.len();
        png[at % n] = byte;
        decode(&png);
        decode(&png[..cut % n]);
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        exercise(&bytes);
    }

    #[test]
    fn a_mutated_face_never_panics(
        flips in proptest::collection::vec((0usize..JETBRAINS.len(), any::<u8>()), 1..16)
    ) {
        let mut data = JETBRAINS.to_vec();
        for (at, byte) in flips {
            data[at] = byte;
        }
        exercise(&data);
    }

    #[test]
    fn a_mutated_sbix_face_never_panics(
        flips in proptest::collection::vec((any::<u32>(), any::<u8>()), 1..16)
    ) {
        // Into the `sbix` table's structure: the strike offsets, the glyph
        // offsets, the records' offsets and kinds and the redirections.
        let reach = sbix_structure();
        let mut data = SBIX.to_vec();
        for (at, byte) in flips {
            let i = reach[usize::try_from(at).unwrap_or(0) % reach.len()];
            data[i] = byte;
        }
        exercise(&data);
    }

    #[test]
    fn a_mutated_cblc_face_never_panics(
        cases in any::<bool>(),
        flips in proptest::collection::vec((any::<u32>(), any::<u8>()), 1..16),
        glyphs in proptest::option::of(any::<u16>()),
    ) {
        // Into a `CBLC` table -- the Noto subset's 288 bytes or the cases
        // fixture's 776 -- its size records, index subtable records and
        // subtables, and sometimes the face's glyph count as well.
        let mut data = if cases { CBLC_CASES } else { NOTO_SUBSET }.to_vec();
        let (at, len) = table_range(&data, *b"CBLC");
        for (i, byte) in flips {
            data[at + usize::try_from(i).unwrap_or(0) % len] = byte;
        }
        if let Some(n) = glyphs {
            let (maxp, _) = table_range(&data, *b"maxp");
            data[maxp + 4..maxp + 6].copy_from_slice(&n.to_be_bytes());
        }
        exercise(&data);
    }

    #[test]
    fn a_mutated_variable_face_never_panics(
        flips in proptest::collection::vec((0usize..GVAR_CASES.len(), any::<u8>()), 1..8)
    ) {
        let mut data = GVAR_CASES.to_vec();
        for (at, byte) in flips {
            data[at] = byte;
        }
        exercise(&data);
    }

    #[test]
    fn a_truncated_variable_face_never_panics(len in 0usize..GVAR_CASES.len()) {
        exercise(&GVAR_CASES[..len]);
    }

    /// Byte flips inside a `CFF2` table: the cases font's, whose 1.4 KB is
    /// every structure the reader walks, or Source Code VF's, whose
    /// charstrings and subroutines are a real font's. `exercise` draws every
    /// glyph at the default and at a variation, so the blends run too.
    /// Source Code VF is one case in eight: drawing its 1,568 glyphs twice
    /// is most of this property's time.
    #[test]
    fn a_mutated_cff2_face_never_panics(
        which in 0u8..8,
        flips in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..8),
    ) {
        let mut data = if which == 0 { SOURCE_CODE_VF } else { CFF2_CASES }.to_vec();
        let (start, len) = table_range(&data, *b"CFF2");
        for (at, byte) in flips {
            data[start + at % len] = byte;
        }
        exercise(&data);
    }

    #[test]
    fn a_truncated_cff2_face_never_panics(len in 0usize..CFF2_CASES.len()) {
        exercise(&CFF2_CASES[..len]);
    }

    #[test]
    fn a_mutated_component_face_never_panics(
        flips in proptest::collection::vec((0usize..COMPONENT_CASES.len(), any::<u8>()), 1..8)
    ) {
        let mut data = COMPONENT_CASES.to_vec();
        for (at, byte) in flips {
            data[at] = byte;
        }
        exercise(&data);
    }

    #[test]
    fn a_truncated_component_face_never_panics(len in 0usize..COMPONENT_CASES.len()) {
        exercise(&COMPONENT_CASES[..len]);
    }

    #[test]
    fn a_mutated_advance_face_never_panics(
        phantom in any::<bool>(),
        flips in proptest::collection::vec((0usize..HVAR_CASES.len(), any::<u8>()), 1..8)
    ) {
        let mut data = if phantom { PHANTOM_CASES } else { HVAR_CASES }.to_vec();
        for (at, byte) in flips {
            let at = at % data.len();
            data[at] = byte;
        }
        exercise(&data);
    }

    /// Byte flips inside the variable face's `GDEF` and `GPOS`, where its
    /// item variation store and 876 anchor devices are: a flip anywhere in
    /// 300 KB lands there too rarely to test them.
    #[test]
    fn a_mutated_variable_layout_never_panics(
        in_gdef in any::<bool>(),
        flips in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..8)
    ) {
        let mut data = JETBRAINS_VAR.to_vec();
        let (start, len) = table_range(JETBRAINS_VAR, if in_gdef { *b"GDEF" } else { *b"GPOS" });
        for (at, byte) in flips {
            data[start + at % len] = byte;
        }
        exercise(&data);
    }

    /// The bake writes a font from these bytes, so no byte string may
    /// panic it: the case font, which carries every table the bake
    /// rewrites in 1.5 KB, flipped anywhere, and JetBrains Mono flipped
    /// inside the tables the bake reads glyph by glyph.
    #[test]
    fn a_mutated_bake_never_panics(
        flips in proptest::collection::vec((0usize..BAKE_CASES.len(), any::<u8>()), 1..8)
    ) {
        let mut data = BAKE_CASES.to_vec();
        for (at, byte) in flips {
            data[at] = byte;
        }
        bake_any(&data);
    }

    #[test]
    fn a_truncated_bake_never_panics(len in 0usize..BAKE_CASES.len()) {
        bake_any(&BAKE_CASES[..len]);
    }

    #[test]
    fn a_mutated_variable_font_bakes_or_refuses(
        table in 0usize..5,
        flips in proptest::collection::vec((any::<usize>(), any::<u8>()), 1..8)
    ) {
        let mut data = JETBRAINS_VAR.to_vec();
        let tag = [*b"glyf", *b"gvar", *b"GPOS", *b"GDEF", *b"loca"][table];
        let (start, len) = table_range(JETBRAINS_VAR, tag);
        for (at, byte) in flips {
            data[start + at % len] = byte;
        }
        bake_any(&data);
    }

    #[test]
    fn a_truncated_advance_face_never_panics(len in 0usize..HVAR_CASES.len()) {
        exercise(&HVAR_CASES[..len]);
        exercise(&PHANTOM_CASES[..len.min(PHANTOM_CASES.len())]);
    }

    #[test]
    fn a_truncated_face_never_panics(len in 0usize..JETBRAINS.len()) {
        exercise(&JETBRAINS[..len]);
    }

    #[test]
    fn a_mutated_aat_face_never_panics(
        which in 0usize..2,
        flips in proptest::collection::vec((any::<u32>(), any::<u8>()), 1..16)
    ) {
        // No committed face carries `morx`, so the state-table driver is
        // mutated through the two macOS faces that do -- Menlo for the
        // ligature type, Geneva for the contextual and swash types -- and
        // this is absent elsewhere rather than red.
        let Some(original) = aat_face(which) else {
            return Ok(());
        };
        let mut data = original.to_vec();
        for (at, byte) in flips {
            let i = usize::try_from(at).unwrap_or(0) % data.len();
            data[i] = byte;
        }
        exercise(&data);
    }
}

/// The macOS `morx` faces, read once; `None` where they are not installed.
fn aat_face(which: usize) -> Option<&'static [u8]> {
    static FACES: std::sync::OnceLock<[Option<Vec<u8>>; 2]> = std::sync::OnceLock::new();
    let faces = FACES.get_or_init(|| {
        [
            std::fs::read("/System/Library/Fonts/Menlo.ttc").ok(),
            std::fs::read("/System/Library/Fonts/Geneva.ttf").ok(),
        ]
    });
    faces.get(which)?.as_deref()
}
