//! **Microseconds per glyph mask, per face, at the terminal's size.**
//!
//! The register's D6 decides that glyphs are rasterised once, on the CPU,
//! into an atlas -- so the cost that matters is per cache *miss*, and this
//! is what a miss costs: bounds, fit, the signed-area accumulate and the
//! prefix sum into an 8-bit mask, for every glyph of a face at 14 px.
//!
//! Best settled round of several, because it is a ceiling on one component
//! rather than a comparison of two programs: a slower round is the machine
//! doing something else. Run on mains. Two arms are compared here only when
//! both are built from this tree and interleaved -- the portable accumulate
//! against an ISA one, when one exists.
//!
//!     cargo bench -p baud-bench --bench glyphs -- [rounds] [px] [probe]
//!
//! `probe` prices one component additively by doing it twice: `bounds` (the
//! outline walk that sizes the mask), `raster` (the outline walk that
//! flattens and splats), or `coverage` (the prefix sum and quantise). It is
//! a runtime branch once per glyph -- coarse, and fine for deciding whether
//! a pass is worth an ISA arm before one is built.
//!
//! **Priced, on mains, three interleaved rounds with the first discarded,
//! Fira Code at 14 px, base 0.70 us a glyph:** the sizing walk is **+28%**,
//! the flatten-and-splat **+50%**, the prefix sum and quantise **+12%**
//! (85 ns for ~135 pixels -- a dependent `f32` add chain at the core's add
//! latency, which is what a vector scan would reassociate away, at the price
//! of the byte-exact agreement with ab_glyph that the scalar order keeps).
//! JetBrains Mono and Bungee Tint split the same way. So the accumulate
//! pass the register (D6) names as the SIMD surface is a tenth of a cost
//! that is paid once per glyph per size, and no arm is built for it until a
//! frame measurement says the miss path matters at all -- every shipping
//! GPU terminal is bound by the cell-to-instance conversion, not by
//! rasterisation.

use baud_domain::GlyphId;
use baud_font::{Bounds, Canvas, Face, Fit, Rasterizer, Scale, faces};
use std::hint::black_box;
use std::time::Instant;

const FACES: [&str; 4] = [
    "FiraCode-Regular.ttf",
    "JetBrainsMono-Regular.ttf",
    "BungeeTint-Regular.ttf",
    "NotoColorEmoji-subset.ttf",
];

fn main() {
    let mut args = baud_bench::args();
    let rounds: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(7);
    let px: f32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(14.0);
    let probe = args.next().unwrap_or_default();
    let (twice_bounds, twice_raster, twice_coverage) =
        (probe == "bounds", probe == "raster", probe == "coverage");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../baud-font/tests/fonts/");
    println!(
        "glyph masks at {px} px, {rounds} rounds, best settled round, probe {probe:?}; on mains or it is noise"
    );
    println!(
        "{:<28} {:>7} {:>7} {:>10} {:>9}",
        "face", "glyphs", "masks", "us/glyph", "px/mask"
    );
    let mut scratch = vec![0f32; 1 << 16];
    let mut out = vec![0u8; 1 << 16];
    for name in FACES {
        let data = std::fs::read(format!("{dir}{name}")).expect("committed fixture");
        let n = faces(&data).map(|n| n.value()).unwrap_or(1);
        for i in 0..n {
            let face = Face::parse_index(&data, baud_font::FaceIndex::of(i)).unwrap();
            let scale = Scale::of(px, face.units_per_em());
            let glyphs = u32::from(face.glyph_count().value());
            let mut best = f64::MAX;
            let mut masks = 0usize;
            let mut pixels = 0usize;
            for round in 0..rounds {
                masks = 0;
                pixels = 0;
                let t0 = Instant::now();
                for g in 0..glyphs {
                    let gid = GlyphId::from_shaper(u16::try_from(g).unwrap());
                    let mut b = Bounds::new();
                    face.outline(gid, &mut b);
                    if twice_bounds {
                        let mut b2 = Bounds::new();
                        face.outline(gid, &mut b2);
                        black_box(&b2);
                    }
                    let Some(fit) = Fit::of(&b, scale) else {
                        continue;
                    };
                    let need = fit.extent.scratch().unwrap();
                    if scratch.len() < need {
                        scratch.resize(need, 0.0);
                        out.resize(need, 0);
                    }
                    let canvas = Canvas::new(fit.extent, &mut scratch).unwrap();
                    let mut r = Rasterizer::new(canvas, fit.transform);
                    face.outline(gid, &mut r);
                    if twice_raster {
                        face.outline(gid, &mut r);
                    }
                    let canvas = r.finish();
                    let cells = fit.extent.cells().unwrap();
                    canvas.coverage(&mut out[..cells]).unwrap();
                    if twice_coverage {
                        canvas.coverage(&mut out[..cells]).unwrap();
                        black_box(&out[..cells]);
                    }
                    black_box(&out[..cells]);
                    masks += 1;
                    pixels += cells;
                }
                let secs = t0.elapsed().as_secs_f64();
                // The first round is warm-up and is not a candidate.
                if round > 0 {
                    best = best.min(secs);
                }
            }
            let us = best * 1e6 / f64::from(glyphs);
            println!(
                "{:<28} {:>7} {:>7} {:>10.3} {:>9.1}",
                format!("{name}#{i}"),
                glyphs,
                masks,
                us,
                pixels as f64 / masks.max(1) as f64
            );
        }
    }
}
