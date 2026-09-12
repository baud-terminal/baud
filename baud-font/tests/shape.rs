//! Shaping against HarfBuzz's golden fixtures.
//!
//! `tests/shape/gsub.txt` is what `hb-shape` produced for each (face,
//! text) with the grid feature set -- `ccmp` and `calt` on, the rest off --
//! written once by `scripts/gen-shape-fixtures.sh` and committed. Each line
//! is compared glyph for glyph: the ids and the cluster each glyph carries.
//!
//! **Clusters follow HarfBuzz's level 0**: a combining mark or a
//! default-ignorable joins the cluster before it, and a ligature keeps its
//! first cluster. That is also the terminal's rule, because a mark lives in
//! its base's cell, so the fixture and the shaper agree on what a cluster
//! is without translation.

use baud_domain::{AmbiguousWidth, Classified, Codepoint, GlyphId, Width};
use baud_font::{Buffer, Cluster, Face, Features, Glyph, Ignorable, Shaper};

struct Fixture {
    face: String,
    text: Vec<u32>,
    expected: Vec<(u16, u16)>,
}

fn fixtures() -> Vec<Fixture> {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/shape/gsub.txt"))
            .unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let text = f[1]
                .split_whitespace()
                .map(|h| u32::from_str_radix(h, 16).unwrap())
                .collect();
            let body = f[2].trim_start_matches('[').trim_end_matches(']');
            let expected = body
                .split('|')
                .map(|g| {
                    let (id, cl) = g.split_once('=').unwrap();
                    (id.parse().unwrap(), cl.parse().unwrap())
                })
                .collect();
            Fixture {
                face: f[0].to_string(),
                text,
                expected,
            }
        })
        .collect()
}

/// Shapes `text` through the shaper, clustering marks and ignorables with
/// the codepoint before them, the way HarfBuzz's level 0 and the terminal
/// both do.
fn shape<'b>(
    face: &Face<'_>,
    text: &[u32],
    storage: &'b mut [Glyph],
) -> (Buffer<'b>, baud_font::Shaped) {
    let cps: Vec<Codepoint> = text.iter().map(|&v| Codepoint::new(v).unwrap()).collect();
    let mut shaper = Shaper::new(face, storage);
    let mut start = 0usize;
    for i in 1..=cps.len() {
        let boundary = i == cps.len() || {
            let c = cps[i];
            let zero = Classified::of(c).width(AmbiguousWidth::Narrow) == Width::ZERO;
            !(zero || Ignorable::of(c) == Ignorable::Yes)
        };
        if boundary {
            shaper
                .push_cluster(&cps[start..i], Cluster(u16::try_from(start).unwrap()))
                .unwrap();
            start = i;
        }
    }
    shaper.finish(Features::GRID)
}

#[test]
fn every_gsub_fixture_shapes_as_harfbuzz_shaped_it() {
    let fixtures = fixtures();
    assert!(fixtures.len() >= 150, "{}", fixtures.len());
    let mut faces = std::collections::HashMap::new();
    let mut ligatures = 0usize;
    let mut glyphs = 0usize;
    for fx in &fixtures {
        let data = faces.entry(fx.face.clone()).or_insert_with(|| {
            std::fs::read(format!(
                "{}/tests/fonts/{}",
                env!("CARGO_MANIFEST_DIR"),
                fx.face
            ))
            .unwrap()
        });
        let face = Face::parse(data).unwrap();
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 128];
        let before = fx.text.len();
        let (buf, shaped) = shape(&face, &fx.text, &mut storage);
        assert!(shaped.declined.is_zero(), "{}: lookups declined", fx.face);
        let got: Vec<(u16, u16)> = buf
            .as_slice()
            .iter()
            .map(|g| (u16::try_from(g.id.index()).unwrap(), g.cluster.0))
            .collect();
        assert_eq!(
            got, fx.expected,
            "{} {:04X?}: baud {:?} against hb-shape {:?}",
            fx.face, fx.text, got, fx.expected
        );
        ligatures += before.saturating_sub(got.len());
        glyphs += got.len();
    }
    eprintln!(
        "{} fixtures, {glyphs} glyphs, {ligatures} glyphs consumed by ligatures",
        fixtures.len()
    );
    // Positive controls: Fira Code's `->` and `!=` are chained singles that
    // keep the glyph count, Bungee Tint has no GSUB and its lines must pass
    // untouched, and something must have formed a ligature somewhere.
    assert!(glyphs > 800, "{glyphs}");
    assert!(ligatures > 0, "{ligatures}");
}

#[test]
fn a_ligature_spans_the_cells_of_its_components() {
    // `Cluster` and `last` are what the cell mapper reads; a ligature must
    // report the span of every cell it consumed, and a chained single
    // substitution -- Fira's whole scheme -- must not widen anything.
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/JetBrainsMono-Regular.ttf"
    ))
    .unwrap();
    let face = Face::parse(&data).unwrap();
    let text: Vec<u32> = "a->b".chars().map(|c| c as u32).collect();
    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 16];
    let (buf, _) = shape(&face, &text, &mut storage);
    let spans: Vec<(u16, u16)> = buf
        .as_slice()
        .iter()
        .map(|g| (g.cluster.0, g.last.0))
        .collect();
    // Every cell 0..4 is covered exactly once, whatever the glyph count.
    let mut covered = [0u8; 4];
    for (a, b) in &spans {
        for c in *a..=*b {
            covered[usize::from(c)] += 1;
        }
    }
    assert_eq!(covered, [1, 1, 1, 1], "{spans:?}");
}
