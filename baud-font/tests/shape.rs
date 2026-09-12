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

/// One glyph of an `hb-shape` line with positions on:
/// `gid=cluster[@dx,dy]+ax`.
struct HbGlyph {
    id: u16,
    cluster: u16,
    dx: i32,
    dy: i32,
    ax: i32,
}

fn parse_positioned(body: &str) -> Vec<HbGlyph> {
    body.trim_start_matches('[')
        .trim_end_matches(']')
        .split('|')
        .map(|g| {
            let (id, rest) = g.split_once('=').unwrap();
            let (cluster_and_off, ax) = rest.split_once('+').unwrap();
            let (cluster, off) = match cluster_and_off.split_once('@') {
                Some((c, o)) => (c, Some(o)),
                None => (cluster_and_off, None),
            };
            let (dx, dy) = match off {
                Some(o) => {
                    let (x, y) = o.split_once(',').unwrap();
                    (x.parse().unwrap(), y.parse().unwrap())
                }
                None => (0, 0),
            };
            HbGlyph {
                id: id.parse().unwrap(),
                cluster: cluster.parse().unwrap(),
                dx,
                dy,
                ax: ax.parse().unwrap(),
            }
        })
        .collect()
}

/// Whether to rename the face's `GPOS` table away before parsing, so
/// the shaper takes the same fallback path HarfBuzz took for the fixture.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GposTable {
    Kept,
    Stripped,
}

/// Renames the `GPOS` directory entry to `GPOZ` -- a tag that keeps the
/// directory sorted for a binary search -- the same edit
/// `scripts/gen-shape-fixtures.sh` makes for the fallback fixtures.
fn strip_gpos(data: &mut [u8]) -> usize {
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let mut renamed = 0;
    for i in 0..n {
        let at = 12 + 16 * i;
        if &data[at..at + 4] == b"GPOS" {
            data[at..at + 4].copy_from_slice(b"GPOZ");
            renamed += 1;
        }
    }
    renamed
}

struct Agreement {
    fixtures: usize,
    positioned: usize,
    raised: usize,
}

/// Compares every line of a positioning fixture file against the shaper.
/// HarfBuzz offsets a glyph from its own pen position, which for a mark is
/// one base advance past the cell origin baud offsets from -- so baud's dx
/// is HarfBuzz's plus the advances of the glyphs before it in the same
/// cluster. dy is the same frame in both.
fn positions_agree(file: &str, gpos: GposTable) -> Agreement {
    let text =
        std::fs::read_to_string(format!("{}/tests/shape/{file}", env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    let mut faces = std::collections::HashMap::new();
    let mut agreement = Agreement {
        fixtures: 0,
        positioned: 0,
        raised: 0,
    };
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let cps: Vec<u32> = f[1]
            .split_whitespace()
            .map(|h| u32::from_str_radix(h, 16).unwrap())
            .collect();
        let expected = parse_positioned(f[2]);
        let data = faces.entry(f[0].to_string()).or_insert_with(|| {
            let mut d = std::fs::read(format!(
                "{}/tests/fonts/{}",
                env!("CARGO_MANIFEST_DIR"),
                f[0]
            ))
            .unwrap();
            if gpos == GposTable::Stripped {
                assert_eq!(strip_gpos(&mut d), 1, "{}: no GPOS to strip", f[0]);
            }
            d
        });
        let face = Face::parse(data).unwrap();
        assert_eq!(
            face.gpos().is_none(),
            gpos == GposTable::Stripped,
            "{}",
            f[0]
        );
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 128];
        let (buf, shaped) = shape(&face, &cps, &mut storage);
        assert!(shaped.declined.is_zero());
        let got = buf.as_slice();
        assert_eq!(got.len(), expected.len(), "{} {:04X?}", f[0], cps);
        let mut pen_in_cluster = 0i32;
        let mut cluster_start: Option<u16> = None;
        for (g, e) in got.iter().zip(&expected) {
            assert_eq!(
                u16::try_from(g.id.index()).unwrap(),
                e.id,
                "{} {:04X?}",
                f[0],
                cps
            );
            assert_eq!(g.cluster.0, e.cluster, "{} {:04X?}", f[0], cps);
            if cluster_start != Some(e.cluster) {
                cluster_start = Some(e.cluster);
                pen_in_cluster = 0;
            }
            let want_dx = e.dx + pen_in_cluster;
            assert_eq!(
                (i32::from(g.dx), i32::from(g.dy)),
                (want_dx, e.dy),
                "{} {:04X?} glyph {}: baud ({}, {}) against hb-shape ({}, {}) + pen {}",
                f[0],
                cps,
                e.id,
                g.dx,
                g.dy,
                e.dx,
                e.dy,
                pen_in_cluster
            );
            if e.dx != 0 || e.dy != 0 {
                agreement.positioned += 1;
            }
            if e.dy != 0 {
                agreement.raised += 1;
            }
            pen_in_cluster += e.ax;
        }
        agreement.fixtures += 1;
    }
    agreement
}

#[test]
fn every_gpos_fixture_positions_marks_as_harfbuzz_does() {
    let a = positions_agree("gpos.txt", GposTable::Kept);
    eprintln!(
        "{} positioning fixtures, {} marks moved by a lookup",
        a.fixtures, a.positioned
    );
    assert!(a.fixtures >= 40, "{}", a.fixtures);
    // Fira stacks a macron on a diaeresis and JetBrains Mono anchors an
    // acute over a dotted a; a lookup must have moved something.
    assert!(a.positioned >= 5, "{}", a.positioned);
}

#[test]
fn every_fallback_fixture_places_marks_as_harfbuzz_does() {
    // No committed face lacks GPOS, so the fixtures were shaped on copies
    // with the table renamed away, and the same rename is made here: what
    // is compared is the geometric fallback, on both sides.
    let a = positions_agree("fallback.txt", GposTable::Stripped);
    eprintln!(
        "{} fallback fixtures, {} marks placed, {} of them vertically",
        a.fixtures, a.positioned, a.raised
    );
    assert!(a.fixtures >= 40, "{}", a.fixtures);
    // A stacked acute rides a diaeresis and a capital lifts its mark, so
    // the vertical half of the placer must have moved something.
    assert!(a.positioned >= 20, "{}", a.positioned);
    assert!(a.raised >= 10, "{}", a.raised);
}
