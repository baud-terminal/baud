//! `frame`: what a frame costs, split into the conversion and the paint.
//!
//! Replays one recording through the core in `read(2)`-sized reads and
//! paints a frame after every read at 200x50 cells, 14 px, JetBrains
//! Mono, reporting the per-frame cost split into conversion (style
//! resolution, shaping through the run cache, atlas lookups) and
//! compositing, plus the counters that say two arms did equal work: rows
//! dirtied, instances emitted, cache hits and misses, glyphs rasterised.
//!
//! Usage: `frame <recording> [reps] [full|incremental]`.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let which = args.next().unwrap_or_else(|| "btop".to_string());
    let reps: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
    let damage = match args.next().as_deref() {
        Some("full") => Damage::Full,
        _ => Damage::Incremental,
    };
    let name = match which.as_str() {
        "btop" => "utf8/btop.raw".to_string(),
        "asciiquarium" => "ascii/asciiquarium.raw".to_string(),
        "cat-scroll" | "cat" => "ascii/cat-scroll.raw".to_string(),
        "git-log" => "ascii/git-log.raw".to_string(),
        "less-pager" | "less" => "ascii/less-pager.raw".to_string(),
        "vim" | "vim-unicode" => "utf8/vim-unicode.raw".to_string(),
        "cat-cjk" => "utf8/cat-cjk.raw".to_string(),
        "less-cjk" => "utf8/less-cjk.raw".to_string(),
        "cava" => "utf8/cava.raw".to_string(),
        other => other.to_string(),
    };
    let data = std::fs::read(format!(
        "{}/../tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("recording");
    // `FRAME_FACES=path[#index],...` stacks other faces -- on macOS,
    // `/System/Library/Fonts/Menlo.ttc,/System/Library/Fonts/Apple Color
    // Emoji.ttc` is the terminal's real stack and prices the colour path.
    let stack_spec = std::env::var("FRAME_FACES").unwrap_or_else(|_| {
        format!(
            "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let files: Vec<(Vec<u8>, u32)> = stack_spec
        .split(',')
        .map(|spec| {
            let (path, index) = spec.split_once('#').unwrap_or((spec, "0"));
            (
                std::fs::read(path).expect("face file"),
                index.parse().unwrap_or(0),
            )
        })
        .collect();
    let faces: Vec<Face<'_>> = files
        .iter()
        .map(|(data, i)| Face::parse_index(data, baud_font::FaceIndex::of(*i)).unwrap())
        .collect();
    let ws = WinSize {
        ws_row: 50,
        ws_col: 200,
    };
    // **A fresh renderer per rep.** The run cache and the atlas persist for
    // a session, so a renderer reused across reps reports the second rep
    // with every run already shaped and every glyph already rasterised --
    // the first version of this harness did, and its "best of three" was
    // a warm-cache number three times lower than a real first pass. Each
    // rep starts cold, which is what a recording's first replay costs.
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::from_stride(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap()
    };
    let mut r = fresh();
    let (w, h) = r.pixel_size();
    let mut pixels = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    println!(
        "{:<12} {:>7} {:>9} {:>9} {:>9} {:>8} {:>9} {:>8} {:>8} {:>8}",
        "recording",
        "frames",
        "us/frame",
        "convert",
        "paint",
        "rows",
        "inst",
        "hit%",
        "raster",
        "grown"
    );
    /// The best rep's sums.
    struct Rep {
        frames: usize,
        total: Duration,
        convert: Duration,
        paint: Duration,
        rows: u64,
        inst: u64,
    }
    let mut best: Option<Rep> = None;
    for rep in 0..reps {
        if rep > 0 {
            r = fresh();
        }
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let mut rep = Rep {
            frames: 0,
            total: Duration::ZERO,
            convert: Duration::ZERO,
            paint: Duration::ZERO,
            rows: 0,
            inst: 0,
        };
        let mut at = 0usize;
        let t0 = std::time::Instant::now();
        while at < data.len() {
            let n = 4096.min(data.len() - at);
            t.write_at(BytePos::ZERO, &data[at..at + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            at += n;
            let mut target = Target::new(&mut pixels, w, w, h).unwrap();
            let f = r.render(&mut t, &mut target, damage);
            rep.frames += 1;
            rep.convert += f.convert;
            rep.paint += f.composite;
            rep.rows += f.rows_dirty.value();
            rep.inst += f.instances.value();
        }
        rep.total = t0.elapsed();
        if best.as_ref().is_none_or(|b| rep.total < b.total) {
            best = Some(rep);
        }
    }
    let Rep {
        frames,
        total,
        convert,
        paint,
        rows,
        inst,
    } = best.unwrap();
    let a = r.atlas_stats();
    let (raster, coloured, grown) = (a.rasterised.value(), a.coloured.value(), a.grown.value());
    let c = r.cache_stats();
    let hit = 100.0 * c.hits.value() as f64 / (c.hits.value() + c.misses.value()).max(1) as f64;
    println!(
        "{:<12} {:>7} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>9.1} {:>7.1}% {:>8} {:>8} {:>8}",
        which,
        frames,
        total.as_secs_f64() * 1e6 / frames as f64,
        convert.as_secs_f64() * 1e6 / frames as f64,
        paint.as_secs_f64() * 1e6 / frames as f64,
        rows as f64 / frames as f64,
        inst as f64 / frames as f64,
        hit,
        raster,
        coloured,
        grown
    );
}
