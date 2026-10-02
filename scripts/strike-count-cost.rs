//! What `Face::is_colour` costs on hostile faces past baud's strike bound.
//! Note J, docs/research/notes/j-colour-bitmap-limits.md. Not a test of the
//! tree: it needs `MAX_STRIKES` lifted in baud-font/src/bitmap.rs, so it
//! lives here and is copied in for a run, then removed:
//!
//!   python3 scripts/strike-count-fonts.py OUT
//!   (set `const MAX_STRIKES: u32 = u32::MAX;` in baud-font/src/bitmap.rs)
//!   cp scripts/strike-count-cost.rs baud-font/tests/scratch_count_cost.rs
//!   STAGE2=OUT cargo test --release -p baud-font --test scratch_count_cost -- --nocapture
//!   (restore bitmap.rs and delete the copy)
//!
//! For the cblc-subs rows the per-strike column divides by one size; the
//! per-subtable cost is the per-call figure over the subtable count.
use baud_domain::GlyphId;
use baud_font::Face;
use std::hint::black_box;
use std::time::Instant;

fn time(face: &Face<'_>, g: u16, budget_calls: usize) -> f64 {
    let g = GlyphId::from_shaper(g);
    let mut best = f64::MAX;
    for _ in 0..3 {
        let t = Instant::now();
        for _ in 0..budget_calls {
            black_box(black_box(face).is_colour(black_box(g)));
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e9 / budget_calls as f64);
    }
    best
}

#[test]
fn count_cost() {
    let dir = std::env::var("STAGE2").expect("STAGE2");
    let cases: &[(&str, u32, &[(&str, u16, bool)])] = &[
        (
            "sbix-1",
            1,
            &[("miss", 0, false), ("ten-hop", 9, false), ("hit", 1, true)],
        ),
        (
            "sbix-255",
            255,
            &[("miss", 0, false), ("ten-hop", 9, false), ("hit", 1, true)],
        ),
        (
            "sbix-65535",
            65535,
            &[("miss", 0, false), ("ten-hop", 9, false), ("hit", 1, true)],
        ),
        (
            "sbix-1048574",
            1_048_574,
            &[("miss", 0, false), ("ten-hop", 9, false), ("hit", 1, true)],
        ),
        (
            "cblc-1",
            1,
            &[("out-of-range miss", 0, false), ("hit", 4, true)],
        ),
        (
            "cblc-255",
            255,
            &[("out-of-range miss", 0, false), ("hit", 4, true)],
        ),
        (
            "cblc-65535",
            65535,
            &[("out-of-range miss", 0, false), ("hit", 4, true)],
        ),
        (
            "cblc-range-1",
            1,
            &[("in-range miss, 5 subtables", 0, false)],
        ),
        (
            "cblc-range-255",
            255,
            &[("in-range miss, 5 subtables", 0, false)],
        ),
        (
            "cblc-range-65535",
            65535,
            &[("in-range miss, 5 subtables", 0, false)],
        ),
        (
            "cblc-subs-5",
            1,
            &[("miss over the subtables", 0, false), ("hit", 40, true)],
        ),
        (
            "cblc-subs-255",
            1,
            &[("miss over the subtables", 0, false), ("hit", 40, true)],
        ),
        (
            "cblc-subs-65535",
            1,
            &[("miss over the subtables", 0, false), ("hit", 40, true)],
        ),
    ];
    for (name, strikes, glyphs) in cases {
        let data = std::fs::read(format!("{dir}/{name}.ttf")).unwrap();
        let face = Face::parse(&data).unwrap();
        assert_eq!(face.strikes().value(), *strikes, "{name} reads its strikes");
        for (what, g, colour) in glyphs.iter() {
            assert_eq!(
                face.is_colour(GlyphId::from_shaper(*g)),
                *colour,
                "{name} {what}"
            );
            let calls = (4_000_000 / *strikes as usize).clamp(10, 200_000);
            let ns = time(&face, *g, calls);
            println!(
                "{name:>16} {what:>28}: {:12.1} ns a call, {:7.2} ns a strike, block of 256: {:9.3} ms",
                ns,
                ns / f64::from(*strikes),
                ns * 256.0 / 1e6
            );
        }
    }
}
