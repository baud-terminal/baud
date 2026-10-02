//! What an index of which glyphs have a bitmap costs to build, against
//! walking the strikes per glyph as `Face::has_bitmap` does. Note J,
//! docs/research/notes/j-colour-bitmap-limits.md. Not a test of the tree:
//! the 4096 rows need `MAX_STRIKES` lifted, as for strike-count-cost.rs:
//!
//!   python3 scripts/strike-count-fonts.py OUT
//!   (set `const MAX_STRIKES: u32 = u32::MAX;` in baud-font/src/bitmap.rs)
//!   cp scripts/strike-count-build.rs baud-font/tests/scratch_build_cost.rs
//!   STAGE3=OUT NOTO=/path/to/NotoColorEmoji.ttf \
//!     cargo test --release -p baud-font --test scratch_build_cost -- --nocapture
//!   (restore bitmap.rs and delete the copy)
//!
//! The build is the simplest exact one: strike by strike, every glyph not
//! yet found, through `Face::bitmap` -- the call `has_bitmap` makes, so the
//! two agree by construction, and the test still holds them equal at every
//! glyph before timing anything. The first round is discarded; the range is
//! over the rest.
use baud_domain::GlyphId;
use baud_font::bytes::Index;
use baud_font::{Face, FaceIndex};
use std::hint::black_box;
use std::time::Instant;

fn build(face: &Face<'_>) -> Vec<bool> {
    let mut found = vec![false; usize::from(face.glyph_count().value())];
    let strikes = face.strikes();
    let mut i = Index::ZERO;
    while strikes.holds(i) {
        for (g, seen) in (0u16..).zip(found.iter_mut()) {
            if !*seen && face.bitmap(i, GlyphId::from_shaper(g)).is_some() {
                *seen = true;
            }
        }
        i = i.next();
    }
    found
}

fn walk(face: &Face<'_>) -> Vec<bool> {
    (0..face.glyph_count().value())
        .map(|g| face.has_bitmap(GlyphId::from_shaper(g)))
        .collect()
}

fn ms(f: impl FnOnce()) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1e3
}

#[test]
fn build_cost() {
    let stage = std::env::var("STAGE3").expect("STAGE3");
    let noto = std::env::var("NOTO").expect("NOTO");
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts");
    let cases: Vec<(String, String, u32)> = [
        (
            "Apple Color Emoji",
            "/System/Library/Fonts/Apple Color Emoji.ttc".to_string(),
        ),
        ("Noto Color Emoji", noto),
        ("sbix fixture", format!("{fixtures}/sbix-strikes.ttf")),
        (
            "Noto subset fixture",
            format!("{fixtures}/NotoColorEmoji-subset.ttf"),
        ),
    ]
    .into_iter()
    .map(|(n, p)| (n.to_string(), p, 0))
    .chain(
        [
            "sbix-wide-255",
            "sbix-wide-dupe-255",
            "sbix-wide-4096",
            "sbix-wide-dupe-4096",
            "cblc-wide-255x1",
            "cblc-wide-1x255",
            "cblc-wide-255x255",
            "cblc-wide-4096x1",
        ]
        .into_iter()
        .map(|n| (n.to_string(), format!("{stage}/{n}.ttf"), 0)),
    )
    .collect();
    for (name, path, index) in cases {
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let face = Face::parse_index(&data, FaceIndex::of(index)).unwrap();
        let strikes = face.strikes().value();
        let glyphs = u32::from(face.glyph_count().value());
        let mut want = Vec::new();
        let walk_ms = ms(|| want = walk(&face));
        let got = build(&face);
        assert_eq!(got, want, "{name}: the index differs from has_bitmap");
        let colour = got.iter().filter(|c| **c).count();
        let first = ms(|| {
            black_box(build(black_box(&face)));
        });
        let reps = if first > 1000.0 { 2 } else { 5 };
        let runs: Vec<f64> = (0..reps)
            .map(|_| {
                ms(|| {
                    black_box(build(black_box(&face)));
                })
            })
            .collect();
        let lo = runs.iter().copied().fold(f64::MAX, f64::min);
        let hi = runs.iter().copied().fold(0.0, f64::max);
        let pairs = f64::from(strikes) * f64::from(glyphs);
        println!(
            "{name:>20}: {strikes:>5} strikes x {glyphs:>5} glyphs, {colour:>5} with a bitmap | \
             build {lo:9.3}-{hi:9.3} ms ({:5.2} ns a strike-glyph pair, {reps} rounds) | \
             walk of every glyph {walk_ms:9.3} ms",
            lo * 1e6 / pairs
        );
    }
}
