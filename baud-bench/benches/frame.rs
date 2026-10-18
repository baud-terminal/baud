//! `frame`: what a frame costs, split into the conversion and the paint.
//!
//! Replays one recording through the core in `read(2)`-sized reads and
//! paints a frame after every read at 200x50 cells, 14 px, JetBrains
//! Mono, reporting the per-frame cost split into conversion (style
//! resolution, shaping through the run cache, atlas lookups) and
//! compositing, plus the counters that say two arms did equal work: rows
//! dirtied, instances emitted, cache hits and misses, glyphs rasterised,
//! and the codepoints left out of a cluster no face draws whole because
//! the face that keeps it lacks them (`unmapped`, the last rep's total;
//! `Renderer::marks_unmapped` says what it counts), and the outlines the
//! atlas and the shaper's glyph boxes refused for want of scratch
//! (`refused` and `inkref`, `AtlasStats::refused` and
//! `Renderer::ink_refused`; zero unless a face is read at a variation).
//!
//! Usage: `frame <recording> [reps] [full|incremental]`.
//!
//! `FRAME_BG_ALPHA=<0-255>` paints over a page dimmed to that alpha, the
//! glass (`[glass] dim_dark = 0.22` is 56), where the default is
//! `Theme::dark()`'s opaque page: the compositor blends the two pages by
//! two loops (`baud_render::Page`).
//!
//! `FRAME_BACKDROP=standard` reads the contrast floor's second pass
//! against the see-through standard material's dark greys
//! (`Backdrop::under_window_dark`), which the app draws by default, where
//! the default is the glass's black to white. It matters only below
//! alpha 255.
//!
//! `FRAME_BLINK=none|fade|plasma|fire|ufo` prices instead what an idle blinking
//! cursor's frame costs over the recording's last screen: 2000 incremental
//! frames alternating two phases of that animation -- the hard switch's
//! two, two opacities of a fade, two pulses of the glow, two steps of the
//! fire's burn or the ufo's lift -- reporting the
//! cost, rows and instances a frame. Frames a cycle are `caret.rs`'s
//! `frames_a_cycle_for_each_animation`; the two multiply.
//!
//! `FRAME_IMAGE=<keys>|none` prices instead what an image on screen costs a
//! frame: over the recording's last screen a 256x192 kitty image placed at
//! row 6, column 11 with the `a=p` keys given (`c=40`, `z=-1`, ...), or none,
//! then 500 full frames, reporting the cost a frame. The instructions the
//! whole run retires, `/usr/bin/time -l`, less those of `none`'s, over 500,
//! are the image's a frame.
//!
//! `FRAME_PERCELL=<cols>x<rows>` prices instead a picture placed one cell
//! at a time, yazi 26.9.1's shape (an `a=p,i,p,x,y,w,h,c=1,r=1,z=-1,C=1`
//! per cell): over the recording's last screen, a `cols` by `rows` pixel
//! image placed from the top-left, a source pixel a cell, then a full
//! frame and `FRAME_PERCELL_FRAMES` (default 20) incremental ones over the
//! still screen, reporting the cost a frame, the rows dirtied and an
//! FNV-1a of the last frame's pixels. `FRAME_PERCELL_CHURN=1` re-puts
//! placement 1 with another source pixel before every frame, so each
//! frame has a placement gone and one arrived. `FRAME_WS=<cols>x<rows>`
//! sets the screen, 200x50 by default.

use baud_core::{Owned, Term};
use baud_domain::Chan;
use baud_domain::{BytePos, Cols, ImageBudget, ImagePx, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Backdrop, Blend, CursorPhase, Damage, PxSize, Renderer, Stack, Target, Theme};
use std::time::Duration;

fn main() {
    let mut args = baud_bench::args();
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
    let ws = std::env::var("FRAME_WS")
        .ok()
        .and_then(|v| {
            let (c, r) = v.split_once('x')?;
            Some(WinSize {
                ws_row: r.parse().ok()?,
                ws_col: c.parse().ok()?,
            })
        })
        .unwrap_or(WinSize {
            ws_row: 50,
            ws_col: 200,
        });
    // **A fresh renderer per rep.** The run cache and the atlas persist for
    // a session, so a renderer reused across reps reports the second rep
    // with every run already shaped and every glyph already rasterised --
    // the first version of this harness did, and its "best of three" was
    // a warm-cache number three times lower than a real first pass. Each
    // rep starts cold, which is what a recording's first replay costs.
    let theme = Theme {
        bg_alpha: std::env::var("FRAME_BG_ALPHA")
            .ok()
            .and_then(|s| s.parse().ok())
            .map_or(Theme::dark().bg_alpha, Chan::from_u8),
        backdrop: match std::env::var("FRAME_BACKDROP").as_deref() {
            Ok("standard") => Backdrop::under_window_dark(),
            _ => Backdrop::any(),
        },
        ..Theme::dark()
    };
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap()
    };
    let mut r = fresh();
    let (w, h) = r.pixel_size();
    let mut pixels = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    println!(
        "{:<12} {:>7} {:>9} {:>9} {:>9} {:>8} {:>9} {:>8} {:>8} {:>8} {:>8} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>8} {:>7} {:>7}",
        "recording",
        "frames",
        "us/frame",
        "convert",
        "paint",
        "rows",
        "inst",
        "hit%",
        "raster",
        "coloured",
        "grown",
        "differs",
        "span",
        "marked",
        "start",
        "shaped",
        "morx",
        "bigram",
        "unmapped",
        "refused",
        "inkref"
    );
    /// The best rep's sums.
    struct Rep {
        frames: usize,
        total: Duration,
        convert: Duration,
        paint: Duration,
        rows: u64,
        inst: u64,
        exc: [u64; 7],
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
            exc: [0; 7],
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
            let e = f.exceptions;
            for (slot, t) in rep.exc.iter_mut().zip([
                e.differs, e.spanning, e.marked, e.starters, e.shaped, e.morx, e.bigram,
            ]) {
                *slot += t.value();
            }
        }
        rep.total = t0.elapsed();
        if best.as_ref().is_none_or(|b| rep.total < b.total) {
            best = Some(rep);
        }
    }
    // `FRAME_RESCALE=<points>` prices a zoom: warm the atlas on the whole
    // recording at 14 px, then rescale to the new size and time the one
    // full-damage frame that follows -- the frame a font change forces, in
    // which every visible glyph re-shapes and re-rasterises into an atlas
    // that restarts at its smallest sheet. The grow and raster deltas say
    // whether the cost is the rasterising or the atlas growing under it.
    if let Ok(kind) = std::env::var("FRAME_BLINK") {
        let phase = |opacity: u8, glow: u8| CursorPhase {
            opacity: Chan::from_u8(opacity),
            glow: Chan::from_u8(glow),
            ..CursorPhase::HIDDEN
        };
        let pair = match kind.as_str() {
            "fade" => [phase(128, 0), phase(96, 0)],
            "plasma" => [phase(0, 128), phase(0, 192)],
            "fire" => [
                CursorPhase::fire(Chan::from_u8(70)),
                CursorPhase::fire(Chan::from_u8(80)),
            ],
            "ufo" => [
                CursorPhase::ufo(Chan::from_u8(70)),
                CursorPhase::ufo(Chan::from_u8(80)),
            ],
            _ => [CursorPhase::HIDDEN, CursorPhase::SHOWN],
        };
        let mut rb = fresh();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let mut tail = data.clone();
        tail.extend_from_slice(b"\x1b[1 q\x1b[?25h");
        t.write_at(BytePos::ZERO, &tail);
        t.feed(BytePos::ZERO, ReadLen::from_index(tail.len()));
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        rb.render(&mut t, &mut target, Damage::Full);
        let n = 2000u32;
        let (mut rows, mut inst, mut glow) = (0u64, 0u64, 0u64);
        let start = std::time::Instant::now();
        for i in 0..n {
            rb.set_cursor_blink(pair[usize::from(i % 2 == 1)]);
            let f = rb.render(&mut t, &mut target, Damage::Incremental);
            rows += f.rows_dirty.value();
            inst += f.instances.value();
            glow += u64::try_from(rb.cursor_glow().len()).unwrap_or(0);
        }
        let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        // `instances` is the rows' converted records; the glow's solids
        // ride beside them and are counted on their own.
        println!(
            "blink {kind} over {which}: {us:.1} us/frame, {:.2} rows/frame, {:.1} instances/frame, {:.2} glow solids/frame",
            rows as f64 / f64::from(n),
            inst as f64 / f64::from(n),
            glow as f64 / f64::from(n),
        );
        return;
    }
    if let Ok(keys) = std::env::var("FRAME_IMAGE") {
        let mut ri = fresh();
        let m = ri.metrics();
        let cell = |p: baud_font::Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(256 * 192));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(cell(m.cell_w), cell(m.cell_h));
        let mut tail = data.clone();
        if keys != "none" {
            const B64: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut rgba = Vec::new();
            for y in 0..192u32 {
                for x in 0..256u32 {
                    let [_, _, r, g] = (x * 256 + y).to_be_bytes();
                    rgba.extend_from_slice(&[r, g, 160, 255]);
                }
            }
            tail.extend_from_slice(b"\x1b[6;11H\x1b_Ga=T,f=32,s=256,v=192,i=1,q=2,C=1,");
            tail.extend_from_slice(keys.as_bytes());
            tail.push(b';');
            for q in rgba.chunks(3) {
                let mut b = [0u8; 3];
                b[..q.len()].copy_from_slice(q);
                let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
                for k in 0..4 {
                    tail.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap_or(0)]);
                }
            }
            tail.extend_from_slice(b"\x1b\\");
        }
        t.write_at(BytePos::ZERO, &tail);
        t.feed(BytePos::ZERO, ReadLen::from_index(tail.len()));
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        ri.render(&mut t, &mut target, Damage::Full);
        let n = 500u32;
        let start = std::time::Instant::now();
        for _ in 0..n {
            ri.render(&mut t, &mut target, Damage::Full);
        }
        let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        println!(
            "image {keys} over {which}: {us:.1} us/frame, {} placements",
            ri.image_placements().len()
        );
        return;
    }
    if let Ok(size) = std::env::var("FRAME_PERCELL") {
        let (pc, pr) = size.split_once('x').expect("FRAME_PERCELL=<cols>x<rows>");
        let (pc, pr): (u32, u32) = (pc.parse().unwrap(), pr.parse().unwrap());
        let frames: u32 = std::env::var("FRAME_PERCELL_FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let churn = std::env::var("FRAME_PERCELL_CHURN").is_ok_and(|v| v == "1");
        let mut ri = fresh();
        let m = ri.metrics();
        let cell = |p: baud_font::Px| ImagePx::of(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let area = usize::try_from(pc * pr).unwrap();
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(area + 64));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(cell(m.cell_w), cell(m.cell_h));
        const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut rgb = Vec::new();
        for y in 0..pr {
            for x in 0..pc {
                let [_, _, a, b] = (x * 7 + y * 13).to_be_bytes();
                rgb.extend_from_slice(&[a, b, 90]);
            }
        }
        let mut tail = data.clone();
        tail.extend_from_slice(format!("\x1b_Gq=2,a=t,i=1,f=24,s={pc},v={pr};").as_bytes());
        for q in rgb.chunks(3) {
            let mut b = [0u8; 3];
            b[..q.len()].copy_from_slice(q);
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for k in 0..4 {
                tail.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap_or(0)]);
            }
        }
        tail.extend_from_slice(b"\x1b\\");
        for y in 0..pr {
            for x in 0..pc {
                tail.extend_from_slice(
                    format!(
                        "\x1b[{};{}H\x1b_Gq=2,a=p,i=1,p={},x={x},y={y},w=1,h=1,c=1,r=1,z=-1,C=1\x1b\\",
                        y + 1,
                        x + 1,
                        y * pc + x + 1
                    )
                    .as_bytes(),
                );
            }
        }
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            for c in b.chunks(4096) {
                t.write_at(BytePos::ZERO, c);
                t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
            }
        };
        feed(&mut t, &tail);
        let mut rows = 0u64;
        let us = {
            let mut target = Target::new(&mut pixels, w, w, h).unwrap();
            ri.render(&mut t, &mut target, Damage::Full);
            let start = std::time::Instant::now();
            for k in 0..frames {
                if churn {
                    let put = format!(
                        "\x1b[1;1H\x1b_Gq=2,a=p,i=1,p=1,x={},y=0,w=1,h=1,c=1,r=1,z=-1,C=1\x1b\\",
                        (k + 1) % 2
                    );
                    feed(&mut t, put.as_bytes());
                }
                rows += ri
                    .render(&mut t, &mut target, Damage::Incremental)
                    .rows_dirty
                    .value();
            }
            start.elapsed().as_secs_f64() * 1e6 / f64::from(frames)
        };
        let fnv = pixels.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| {
            (h ^ u64::from(p)).wrapping_mul(0x0000_0100_0000_01b3)
        });
        println!(
            "percell {pc}x{pr} over {which} at {}x{}: {us:.1} us/frame, {} placements drawn, {} live, {rows} rows dirtied in {frames} frames, pixels {fnv:016x}",
            ws.ws_col,
            ws.ws_row,
            ri.image_placements().len(),
            t.images().placements().count(),
        );
        return;
    }
    if let Ok(pts) = std::env::var("FRAME_RESCALE") {
        let pts: f32 = pts.parse().unwrap_or(24.0);
        let mut rz = fresh();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let (w0, h0) = rz.pixel_size();
        let mut px0 = vec![0u32; usize::try_from(w0.value() * h0.value()).unwrap()];
        let mut at = 0usize;
        while at < data.len() {
            let n = 4096.min(data.len() - at);
            t.write_at(BytePos::ZERO, &data[at..at + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            at += n;
            let mut target = Target::new(&mut px0, w0, w0, h0).unwrap();
            rz.render(&mut t, &mut target, Damage::Full);
        }
        let a0 = rz.atlas_stats();
        let c0 = rz.cache_stats();
        if rz.rescale(PxSize::of(pts).unwrap()).is_some() {
            let (w1, h1) = rz.pixel_size();
            let mut px1 = vec![0u32; usize::try_from(w1.value() * h1.value()).unwrap()];
            let mut target = Target::new(&mut px1, w1, w1, h1).unwrap();
            let start = std::time::Instant::now();
            let f = rz.render(&mut t, &mut target, Damage::Full);
            let elapsed = start.elapsed();
            let a1 = rz.atlas_stats();
            let c1 = rz.cache_stats();
            println!(
                "rescale 14->{pts}px: frame {} us (convert {} us, paint {} us); raster {}, grown {}, misses {}",
                elapsed.as_micros(),
                f.convert.as_micros(),
                f.composite.as_micros(),
                a1.rasterised.value() - a0.rasterised.value(),
                a1.grown.value() - a0.grown.value(),
                c1.misses.value() - c0.misses.value(),
            );
        }
        return;
    }

    let Rep {
        frames,
        total,
        convert,
        paint,
        rows,
        inst,
        exc,
    } = best.unwrap();
    let a = r.atlas_stats();
    let (raster, coloured, grown) = (a.rasterised.value(), a.coloured.value(), a.grown.value());
    let c = r.cache_stats();
    let hit = 100.0 * c.hits.value() as f64 / (c.hits.value() + c.misses.value()).max(1) as f64;
    println!(
        "{:<12} {:>7} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>9.1} {:>7.1}% {:>8} {:>8} {:>8} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>8} {:>7} {:>7}",
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
        grown,
        exc[0] as f64 / frames as f64,
        exc[1] as f64 / frames as f64,
        exc[2] as f64 / frames as f64,
        exc[3] as f64 / frames as f64,
        exc[4] as f64 / frames as f64,
        exc[5] as f64 / frames as f64,
        exc[6] as f64 / frames as f64,
        r.marks_unmapped().value(),
        a.refused.value(),
        r.ink_refused().value()
    );
}
