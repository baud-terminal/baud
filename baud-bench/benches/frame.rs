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
//! `FRAME_BIDI=explicit` resets BDSM (`CSI 8 l`) before the recording, so
//! each frame draws its rows in stored order; the default leaves it set,
//! as a terminal starts, and each frame lays its rows out in the order
//! UAX #9 gives them.
//!
//! `FRAME_WRAP=off` draws every row as the terminal cut it
//! (`baud_render::WordWrap::Off`, `[screen] wrap_words = "off"`), where the
//! default draws a finished paragraph at its words; each run prints, after
//! its row, the paragraphs the display re-broke summed over the best rep's
//! frames (`Display::counts`), so an arm that re-broke nothing shows it.
//!
//! `FRAME_STRING=1` begins and ends an SDS string (`CSI 1 ]`, `CSI 0 ]`)
//! before the recording, which writes no cell in it: the arm of a
//! terminal that has seen a string, whose frames may read each cell's
//! string (`Term::strings_seen`), against the same recording's.
//!
//! `FRAME_BG_ALPHA=<0-255>` paints over a page dimmed to that alpha, the
//! glass (`[glass] dim_dark = 0.70`, the default, is 179, and
//! `dim_light = 0.60` is 153), where the default is
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
//! `FRAME_TRAIL=none|quad|fire|plasma|lightning|ice|ufo|orb|liquid` prices
//! instead what a cursor trail costs the CPU path a frame: over the
//! recording's last screen, 600 incremental frames 1/120 s apart of a
//! 30-column move down three rows (`FRAME_TRAIL_MOVE` sets another), the
//! trail drawn from its start until it
//! is out and then again -- the quad's chase, or an effect riding it --
//! reporting the cost, rows and trail records a frame. The instructions
//! the whole run retires, `/usr/bin/time -l`, less those of `none`'s, over
//! 600, are the trail's a frame.
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
//! frame has a placement gone and one arrived. `FRAME_PERCELL_OVER=1`
//! places a partly transparent 2x2 image over the text, `c` and `r` the
//! screen's, so every row is repainted every frame, and the picture's
//! placements under it with it. `FRAME_WS=<cols>x<rows>`
//! sets the screen, 200x50 by default.
//!
//! `FRAME_ZOOM=<px>` prices a zoom and a zoom back over the recording's
//! last screen: warm at 14 px, then `FRAME_ZOOM_STEPS` rescales to `px`
//! and back to 14 by turns, each with the one full frame it forces,
//! reporting each step's time, the glyphs drawn and the bytes a GPU copy
//! of the atlas would take. `FRAME_ZOOM=<px>,<px>,...` walks those sizes
//! instead, and `FRAME_ZOOM_PAINT=off` converts without painting, the
//! CPU's share of a zoom on the GPU path.

use baud_core::{Owned, Term};
use baud_domain::Chan;
use baud_domain::{BytePos, Cols, ImageBudget, ImagePx, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Backdrop, Blend, CursorPhase, Damage, PxSize, Renderer, Stack, Target, Theme};
use std::time::Duration;

/// Whether a zoom step paints its frame.
#[derive(Clone, Copy)]
enum Switch {
    On,
    Off,
}

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
        "pipes" => "ascii/pipes.raw".to_string(),
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
    let wrap = match std::env::var("FRAME_WRAP").as_deref() {
        Ok("off") => baud_render::WordWrap::Off,
        _ => baud_render::WordWrap::On,
    };
    let fresh = || {
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap();
        r.set_word_wrap(wrap);
        r
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
        /// Paragraphs the display re-broke, summed over the frames.
        rebroken: baud_render::Tally,
    }
    let mut best: Option<Rep> = None;
    for rep in 0..reps {
        if rep > 0 {
            r = fresh();
        }
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        // `FRAME_BIDI=explicit` resets BDSM before the recording, so no
        // frame lays its rows out by UAX #9: the arm without the
        // right-to-left layout, which a terminal starts with since BDSM is
        // set at power-on.
        if std::env::var("FRAME_BIDI").as_deref() == Ok("explicit") {
            let bdsm = b"\x1b[8l";
            t.write_at(BytePos::ZERO, bdsm);
            t.feed(BytePos::ZERO, ReadLen::from_index(bdsm.len()));
        }
        // `FRAME_STRING=1`: a string begun and ended, no cell in it.
        if std::env::var("FRAME_STRING").as_deref() == Ok("1") {
            let sds = b"\x1b[1]\x1b[0]";
            t.write_at(BytePos::ZERO, sds);
            t.feed(BytePos::ZERO, ReadLen::from_index(sds.len()));
            assert!(t.strings_seen(), "the string was begun");
        }
        let mut rep = Rep {
            frames: 0,
            total: Duration::ZERO,
            convert: Duration::ZERO,
            paint: Duration::ZERO,
            rows: 0,
            inst: 0,
            exc: [0; 7],
            rebroken: baud_render::Tally::default(),
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
            if let Some(d) = r.bidi().display() {
                rep.rebroken.add(d.counts().0);
            }
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
        let (mut conv, mut comp) = (Duration::ZERO, Duration::ZERO);
        let start = std::time::Instant::now();
        for i in 0..n {
            rb.set_cursor_blink(pair[usize::from(i % 2 == 1)]);
            let f = rb.render(&mut t, &mut target, Damage::Incremental);
            rows += f.rows_dirty.value();
            inst += f.instances.value();
            glow += u64::try_from(rb.cursor_glow().len()).unwrap_or(0);
            conv += f.convert;
            comp += f.composite;
        }
        let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        let per = |d: Duration| d.as_secs_f64() * 1e6 / f64::from(n);
        // `instances` is the rows' converted records; the glow's solids
        // ride beside them and are counted on their own.
        println!(
            "blink {kind} over {which}: {us:.1} us/frame (convert {:.1}, composite {:.1}), {:.2} rows/frame, {:.1} instances/frame, {:.2} glow solids/frame",
            per(conv),
            per(comp),
            rows as f64 / f64::from(n),
            inst as f64 / f64::from(n),
            glow as f64 / f64::from(n),
        );
        return;
    }
    if let Ok(kind) = std::env::var("FRAME_TRAIL") {
        use baud_render::effect::{Effect, Ignition, Seed, Vfx};
        use baud_render::{Pt, Segment, Shape, Smear, Strength, TrailLook};
        let effect = match kind.as_str() {
            "fire" => Some(Effect::Fire),
            "plasma" => Some(Effect::Plasma),
            "lightning" => Some(Effect::Lightning),
            "ice" => Some(Effect::Ice),
            "ufo" => Some(Effect::Ufo),
            "orb" => Some(Effect::Orb),
            "liquid" => Some(Effect::Liquid),
            _ => None,
        };
        let mut rt = fresh();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        t.write_at(BytePos::ZERO, &data);
        t.feed(BytePos::ZERO, ReadLen::from_index(data.len()));
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        rt.render(&mut t, &mut target, Damage::Full);
        let style = baud_core::CursorStyle::SteadyBlock;
        let cell = |row: usize, col: usize| {
            rt.cursor_rect(
                baud_domain::Row::from_index(row),
                baud_domain::Col::at_index(col),
                style,
            )
        };
        // `FRAME_TRAIL_MOVE=<row>,<col>,<row>,<col>` moves elsewhere: the
        // screen's diagonal is `0,0,49,199`.
        let mv: Vec<usize> = std::env::var("FRAME_TRAIL_MOVE")
            .ok()
            .map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect())
            .filter(|v: &Vec<usize>| v.len() == 4)
            .unwrap_or_else(|| vec![10, 40, 13, 70]);
        let (from, to) = (cell(mv[0], mv[1]), cell(mv[2], mv[3]));
        let seg = Segment::new(from.corners(), to);
        let unit = rt.metrics().cell_h.value() as f32;
        let tail = Pt {
            x: (from.left + from.right) * 0.5,
            y: (from.top + from.bottom) * 0.5,
        };
        let ignition = Ignition::ignite(tail, to, Seed(7), unit);
        let life = effect.map_or(seg.lasts(), |e| e.end(&ignition));
        let per = u32::try_from((life * 120.0).ceil().max(1.0) as i64).unwrap_or(1);
        let n = 600u32;
        let (mut rows, mut records) = (0u64, 0u64);
        let start = std::time::Instant::now();
        for i in 0..n {
            let age = (i % per) as f32 / 120.0;
            let emitter = Smear {
                corners: seg.at(age),
                hole: to,
                shape: Shape::Quad,
                strength: Strength::Full,
            };
            let look = match (kind.as_str(), effect) {
                ("none", _) => None,
                (_, Some(e)) => Some(TrailLook::Vfx(Vfx {
                    effect: e,
                    emitter,
                    ignition: ignition.at(age),
                    axis: Pt { x: 0.995, y: 0.1 },
                })),
                (_, None) => Some(TrailLook::Smear(emitter)),
            };
            rt.set_trail(look);
            let f = rt.render(&mut t, &mut target, Damage::Incremental);
            rows += f.rows_dirty.value();
            records += u64::try_from(rt.trail().len()).unwrap_or(0);
        }
        let us = start.elapsed().as_secs_f64() * 1e6 / f64::from(n);
        println!(
            "trail {kind} over {which}: {us:.1} us/frame, {:.2} rows/frame, {:.1} records/frame, {per} frames a life",
            rows as f64 / f64::from(n),
            records as f64 / f64::from(n),
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
        let over = std::env::var("FRAME_PERCELL_OVER").is_ok_and(|v| v == "1");
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
        if over {
            // Four RGBA pixels at half alpha: `f=32`, base64 of 16 bytes.
            let put = format!(
                "\x1b[1;1H\x1b_Gq=2,a=T,i=2,f=32,s=2,v=2,c={},r={},z=1,C=1;gEAggIBAIICAQCCAgEAggA==\x1b\\",
                ws.ws_col, ws.ws_row
            );
            feed(&mut t, put.as_bytes());
            assert_eq!(
                t.images().placements().count(),
                area + 1,
                "FRAME_PERCELL_OVER: the overlay was placed; leave it a slot"
            );
        }
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
    // `FRAME_ZOOM=<px>` prices a zoom and a zoom back: warm the atlas on
    // the whole recording at 14 px, then `FRAME_ZOOM_STEPS` (default 4)
    // rescales, to the new size and back by turns, each followed by the one
    // full-damage frame it forces, printing every step's wall time, convert
    // and paint, rasters, sheet sides and the bytes a GPU copy of the
    // sheets would take. `FRAME_ZOOM=<px>,<px>,...` walks those sizes in
    // order instead. Run under `/usr/bin/time -l` at 0, 1, 2 and more
    // steps: the instructions retired at `n` steps less those at `n - 1`
    // are step `n`'s, the first zoom and then the zoom back.
    // `FRAME_ZOOM_PAINT=off` converts each step's frame and paints
    // nothing, as the app's GPU path leaves the paint to the GPU, so the
    // count is the CPU's share of a zoom.
    if let Ok(list) = std::env::var("FRAME_ZOOM") {
        let paint = std::env::var("FRAME_ZOOM_PAINT").map_or(Switch::On, |v| match v.as_str() {
            "off" => Switch::Off,
            _ => Switch::On,
        });
        let walk: Vec<f32> = list.split(',').filter_map(|v| v.parse().ok()).collect();
        let walk = match walk.as_slice() {
            [one] => vec![*one, 14.0],
            _ => walk,
        };
        let pts = list;
        let steps: usize = std::env::var("FRAME_ZOOM_STEPS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(walk.len().max(4));
        let mut rz = fresh();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let (w0, h0) = rz.pixel_size();
        let mut px = vec![0u32; usize::try_from(w0.value() * h0.value()).unwrap()];
        let mut at = 0usize;
        while at < data.len() {
            let n = 4096.min(data.len() - at);
            t.write_at(BytePos::ZERO, &data[at..at + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            at += n;
            let mut target = Target::new(&mut px, w0, w0, h0).unwrap();
            rz.render(&mut t, &mut target, Damage::Full);
        }
        rz.atlas_mut().take_dirty();
        println!(
            "zoom {pts} px over {which}: warm at 14 px, sheets {}/{}",
            rz.atlas().side().value(),
            rz.atlas().color_side().value()
        );
        let mut fnv = 0xcbf2_9ce4_8422_2325u64;
        for step in 1..=steps {
            let to = walk[(step - 1) % walk.len()];
            let a0 = rz.atlas_stats();
            // The time is the rescale's and the frame's; the canvas
            // resized between them, and hashed after, is the harness's.
            let start = std::time::Instant::now();
            rz.rescale(PxSize::of(to).unwrap())
                .expect("a size the face takes");
            let rescaled = start.elapsed();
            let (convert, composite, framed) = match paint {
                Switch::On => {
                    let (w1, h1) = rz.pixel_size();
                    px.clear();
                    px.resize(usize::try_from(w1.value() * h1.value()).unwrap(), 0);
                    let mut target = Target::new(&mut px, w1, w1, h1).unwrap();
                    let start = std::time::Instant::now();
                    let f = rz.render(&mut t, &mut target, Damage::Full);
                    (f.convert, f.composite, start.elapsed())
                }
                Switch::Off => {
                    let start = std::time::Instant::now();
                    let c = rz.convert(&t, Rows::ZERO, Damage::Full);
                    (c.convert, Duration::ZERO, start.elapsed())
                }
            };
            let elapsed = rescaled + framed;
            let a1 = rz.atlas_stats();
            // What a GPU copy of the sheets would take for this frame: a
            // replaced sheet whole, else the rectangle written.
            let (dirty, mask_new, color_new) = rz.atlas_mut().take_dirty();
            let texels = |r: Option<baud_render::TexRect>| {
                r.map_or(0, |r| u64::from(r.w.0) * u64::from(r.h.0))
            };
            let side = |s: baud_render::Side| u64::from(s.value()) * u64::from(s.value());
            let upload = match mask_new {
                baud_render::Replaced::Yes => side(rz.atlas().side()),
                baud_render::Replaced::No => texels(dirty.mask),
            } + 4 * match color_new {
                baud_render::Replaced::Yes => side(rz.atlas().color_side()),
                baud_render::Replaced::No => texels(dirty.color),
            };
            if let Switch::On = paint {
                for p in &px {
                    fnv = (fnv ^ u64::from(*p)).wrapping_mul(0x100_0000_01b3);
                }
            }
            println!(
                "  step {step} to {to}px: {} us (convert {} us, paint {} us); raster {}, coloured {}, synthesised {}; sheets {}/{}, upload {upload} bytes",
                elapsed.as_micros(),
                convert.as_micros(),
                composite.as_micros(),
                a1.rasterised.value() - a0.rasterised.value(),
                a1.coloured.value() - a0.coloured.value(),
                a1.synthesised.value() - a0.synthesised.value(),
                rz.atlas().side().value(),
                rz.atlas().color_side().value(),
            );
        }
        println!("  pixels {fnv:016x}");
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
        rebroken,
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
    println!("  rebroken {}", rebroken.value());
}
