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
//!     cargo run --release -p baud-bench --bin glyphs -- [rounds] [px]

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
    let mut args = std::env::args().skip(1);
    let rounds: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(7);
    let px: f32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(14.0);
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../baud-font/tests/fonts/");
    println!(
        "glyph masks at {px} px, {rounds} rounds, best settled round; on mains or it is noise"
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
                    let canvas = r.finish();
                    let cells = fit.extent.cells().unwrap();
                    canvas.coverage(&mut out[..cells]).unwrap();
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
