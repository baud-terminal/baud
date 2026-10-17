//! `frame_gpu`: the CPU compositor against Metal on the same frames.
//!
//! Replays one recording through the core in `read(2)`-sized reads at
//! 200x50 cells, 14 px, and after every read converts the rows that
//! changed and then either paints them (the `cpu` arm, `Renderer::paint`)
//! or uploads and draws the whole frame on the GPU into an offscreen
//! texture, waiting for completion (the `gpu` arm, `Gpu::draw`). A third
//! arm, `cpu2`, is the CPU arm again -- the A/A control every sweep here
//! carries. The fourth, `cells`, converts nothing: the terminal's viewport
//! *is* a shared GPU buffer, the rendition table is resolved into another
//! and the glyph map lives in a third, and the shader resolves every cell
//! (`Gpu::draw_cells`); its `convert` column is the map walk and the style
//! resolve, and nothing is copied. It draws a smaller program (no
//! ligatures, no marks; see `baud_render::GlyphMap`), and the `marks`
//! column counts the cells whose marks it skipped. The fifth, `async`, is
//! the cell path as the app runs it -- committed with a completion handler
//! and never waited for -- so its `draw` column is the CPU's whole cost of
//! a frame, and its `wait` column is the time the *bench* idled until the
//! previous frame completed before it could feed the next read, which a
//! frame loop spends on other events. The five run interleaved per round,
//! a fresh renderer per arm per round, and every round is printed so the
//! reader discards the first. The `cells` arm prints as `waited`, because
//! that is what it is: the cell path with `waitUntilCompleted` after the
//! commit, kept so the bench can read the GPU's own start-to-end time --
//! a measurement arm, and not how the app runs.
//!
//! What it does not measure: the present. The CPU arm's frame is not
//! copied into a surface and the GPU arms' texture is not an IOSurface;
//! both are the same one memcpy away from the screen, and the `present`
//! column is a separate instrument.
//!
//! Usage: `frame_gpu <recording> [rounds] [full|incremental]`. macOS only.

#[cfg(target_os = "macos")]
fn main() {
    // Imported here rather than at the top of the file: this bench is macOS
    // only, so a top-level `use` is unused on every other target and `-D
    // warnings` makes that an error. CI lints on ubuntu; a Mac cannot see it.
    use baud_core::{Depth, Owned, Term, grid_stride};
    use baud_domain::{
        BytePos, Cols, FrameGen, ReadLen, Row, RowBlock, RowRot, Rows, SwapEpoch, WinSize,
    };
    use baud_font::{Face, Px};
    use baud_present_macos::journal::{FrameTag, Journal, SetIx};
    use baud_present_macos::{CellInput, Done, Drawn, Gpu, InFlight, Landing};
    use baud_render::{
        Blend, Candidate, Damage, GlyphMap, Instance, Overlay, PxSize, Renderer, Scene, Stack,
        StyleStamp, Tally, Target, Theme,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    let mut args = baud_bench::args();
    let which = args.next().unwrap_or_else(|| "btop".to_string());
    let rounds: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);
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
    // An absolute path is read as it is, so a payload that is not a
    // recording -- a text-drawn picture placed the way a file manager
    // places it -- can be replayed without joining the corpus.
    let path = if name.starts_with('/') {
        name.clone()
    } else {
        format!("{}/../tests/recordings/{name}", env!("CARGO_MANIFEST_DIR"))
    };
    let data = std::fs::read(&path).expect("recording");
    // `FRAME_GLASS`: the window's dim, which `Theme::dark()` leaves at full
    // alpha -- where `[glass] pictures`'s finder never runs, so no default
    // arm can price it. `dim` is the app's default until 2026-10-17 (0.22
    // of 255, every background dimmed, pictures found and kept solid; the
    // default is 0.70 since), kept so its recorded figures re-run; `dim-nopictures`
    // is the same with `pictures = "dim"`, the finder off.
    let glass = std::env::var("FRAME_GLASS").unwrap_or_else(|_| "off".to_string());
    let theme = match glass.as_str() {
        "off" => Theme::dark(),
        "dim" | "dim-nopictures" => Theme {
            bg_alpha: baud_domain::Chan::from_u8(56),
            app_bg: baud_render::AppBg::Dimmed,
            picture_bg: if glass == "dim" {
                baud_render::PictureBg::Opaque
            } else {
                baud_render::PictureBg::Dimmed
            },
            ..Theme::dark()
        },
        other => panic!("FRAME_GLASS={other:?} is not off, dim or dim-nopictures"),
    };
    println!("glass: {glass}");
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
    // `FRAME_GRID=400x150`: provision the storage, and so the rendition
    // table and the GPU's grid, at that size while the screen stays 200x50.
    // The app provisions `MAX_GRID`, 400x150, which sizes the table to
    // 65,536 slots where the screen alone gives 16,384 -- so without this the
    // bench prices a quarter of the app's style resolve.
    let prov = std::env::var("FRAME_GRID").map_or(WinSize { ..ws }, |g| {
        let (c, r) = g.split_once('x').expect("FRAME_GRID is COLSxROWS");
        WinSize {
            ws_row: r.parse().expect("rows"),
            ws_col: c.parse().expect("cols"),
        }
    });
    // `FRAME_PX`: the size in device pixels, 14 by default; the app on a
    // Retina panel draws 14 pt at 28.
    let px: f32 =
        std::env::var("FRAME_PX").map_or(14.0, |v| v.parse().expect("FRAME_PX is a size"));
    println!("px: {px}");
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(px).unwrap(),
            theme.clone(),
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap()
    };
    let mut gpu = Gpu::new().expect("a Metal device");
    let probe = fresh();
    let (w, h) = probe.pixel_size();
    let target = gpu.offscreen(w, h).expect("offscreen target");
    let mut pixels = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    println!("memory: {:?}", gpu.unified_memory());
    println!(
        "{:<5} {:<6} {:>7} {:>9} {:>9} {:>9} {:>9} {:>9} {:>8} {:>9} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>9}",
        "round",
        "arm",
        "frames",
        "us/frame",
        "convert",
        "draw",
        "bind",
        "gpu",
        "rows",
        "inst",
        "miss",
        "ptest",
        "cand",
        "win",
        "wcells",
        "wmiss",
        "rec",
        "pict",
        "wait"
    );
    /// One arm's sums over a replay.
    #[derive(Default)]
    struct Sums {
        frames: usize,
        /// The first `FRAME_FIRST` frames of a replay on their own: their
        /// convert, and the map's misses when they ended -- row 8 of note N
        /// is about the first frames of a page, and a replay's median hides
        /// them.
        first_frames: Tally,
        first_convert: Duration,
        first_miss: Tally,
        total: Duration,
        convert: Duration,
        /// The paint on the CPU arm; bind plus commit on the GPU arms.
        draw: Duration,
        bind: Duration,
        gpu: Duration,
        rows: u64,
        inst: u64,
        miss: Tally,
        ptest: Tally,
        cand: Tally,
        win: Tally,
        wcells: Tally,
        wmiss: Tally,
        rec: Tally,
        /// The cells `[glass] pictures`'s finder marked as a picture, which
        /// is what says it ran: zero under `FRAME_GLASS=off`.
        pict: Tally,
        wait: Duration,
        /// Where the cell arms' CPU goes, a frame after a read and a frame
        /// with nothing read (`FRAME_IDLE`), apart.
        parts: [Parts; 2],
    }
    /// One kind of cell-path frame, split by what it does: the glyph-map
    /// walk, the rendition table synced (`Renderer::sync_styles`, the slots
    /// written since the last frame), the overlay walk, the window
    /// copied (the `copied` arm only; the app copies every frame), and the
    /// bind plus commit.
    #[derive(Default, Clone, Copy)]
    struct Parts {
        frames: Tally,
        map: Duration,
        resolve: Duration,
        overlay: Duration,
        copy: Duration,
        draw: Duration,
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Arm {
        Cpu,
        Gpu,
        Cpu2,
        Cells,
        Async,
        /// The cell path with the grid copied into a second shared buffer
        /// before each draw: the arm that says whether reading the core's
        /// grid in place costs the GPU anything.
        Copied,
        /// The cell path committed asynchronously with the surface set
        /// replaced **while a frame is in flight** -- what a live resize or
        /// a zoom does, because `relayout` reaches `present`, whose rebuild
        /// branch fires before the busy check.
        ///
        /// It answers the question the other arms cannot: how many frames
        /// reach the layer carrying an image drawn for surfaces that have
        /// since been freed and rebuilt. Real GPU completions on real
        /// hardware, arriving when the hardware says so; only the rebuild
        /// is scripted, and it is scripted to the shape the app produces.
        Present,
        /// The `async` cell frame plus the extra map walk the app's smooth-
        /// scroll slide does: `present` ensures the map over the view one
        /// line up (the leaving row's glyphs) *and* over the screen, where a
        /// steady frame ensures only the screen. So `slide - async` is the
        /// per-frame CPU cost a slide adds, which is dominated by that
        /// redundant walk; the leaving-row copy (one row) and the +1-row
        /// draw ((rows+1)/rows) are bounded-negligible by construction and
        /// not modelled here.
        Slide,
    }
    let arms = [
        Arm::Cpu,
        Arm::Gpu,
        Arm::Cpu2,
        Arm::Cells,
        Arm::Async,
        Arm::Copied,
        Arm::Present,
        Arm::Slide,
    ];
    // `FRAME_ARMS=async,cells` runs only the arms named, so a profile or a
    // sweep sees one path.
    let wanted = std::env::var("FRAME_ARMS").ok();
    // `FRAME_FIRST=n` reports the first n frames of every replay apart:
    // their convert a frame and the map misses they took, beside the rest.
    // `FRAME_IDLE=n`: after the replay, n more frames on the cell arms with
    // nothing read -- the frame a caret blink or a trail step draws, where
    // nothing in the grid, the table or the palette changed. Reported apart.
    let idle_n: usize = std::env::var("FRAME_IDLE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let first_n: usize = std::env::var("FRAME_FIRST")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let arms: Vec<Arm> = arms
        .into_iter()
        .filter(|a| {
            wanted.as_deref().is_none_or(|w| {
                let name = match a {
                    Arm::Cpu => "cpu",
                    Arm::Gpu => "gpu",
                    Arm::Cpu2 => "cpu2",
                    Arm::Cells => "cells",
                    Arm::Async => "async",
                    Arm::Copied => "copied",
                    Arm::Present => "present",
                    Arm::Slide => "slide",
                };
                w.split(',').any(|n| n.trim() == name)
            })
        })
        .collect();
    // The pitch every grid here is laid out at, the storage's and the GPU's,
    // so the copied arm's second buffer is the shape of the first.
    let geom = baud_bench::host_geometry();
    let stride = grid_stride(&geom, Cols::of_index(usize::from(prov.ws_col)));
    // The primary ring is the screen plus the scrollback; the copied arm's
    // buffer holds the screen alone, in screen order, because that is what
    // its copy is: the window, contiguous, under no rotation.
    let rows = Rows::of_index(usize::from(ws.ws_row));
    let ring = Owned::without_surfaces(&prov, &geom, Depth::DEFAULT).ring();
    let (copy_grid, copy_mem) = gpu.grid(rows, stride).expect("a second shared grid");
    let (map_buffer, map_mem) = gpu.map().expect("a shared map");
    let mut map = GlyphMap::over(map_mem);
    let (overlay, overlay_mem) = gpu
        .overlay(Cols::of_index(usize::from(ws.ws_col)), rows)
        .expect("a shared overlay");
    let mut records: Vec<Instance> = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    // The asynchronous arm's GPU time arrives with the completion, so the
    // handler is where it is taken. A no-op here would report zero for the
    // one arm the app actually runs -- which is exactly what the app did
    // until the completion started carrying the number. The landing also
    // carries *when* the frame ended on the host clock; that is the
    // keystroke instrument's business and this bench, which times frames
    // and not keys, reads only the duration.
    let async_gpu = Arc::new(AtomicU64::new(0));
    let sink = {
        let acc = Arc::clone(&async_gpu);
        let f: Arc<dyn Fn(Landing) + Send + Sync> = Arc::new(move |landed: Landing| {
            acc.fetch_add(
                u64::try_from(landed.gpu.as_nanos()).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
        });
        f
    };
    // The present arm's state. Shared with every completion, exactly as a
    // presenter shares them with the frames it has in flight.
    let epoch = Arc::new(SwapEpoch::new());
    let journal = Arc::new(Journal::default());
    let mut frame_no = FrameGen::FIRST;
    // How often the surfaces are replaced under a frame in flight. A drag
    // rebuilds on most steps, so every frame is the honest shape; the
    // divisor exists so the arm can also be run at a gentler rate.
    let rebuild_every: usize = std::env::var("FRAME_REBUILD_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    for round in 0..rounds {
        for &arm in &arms {
            let mut r = fresh();
            // The cell arms lend the terminal its viewport from the GPU;
            // the others take the storage's own.
            let cell_arm = matches!(
                arm,
                Arm::Cells | Arm::Async | Arm::Copied | Arm::Present | Arm::Slide
            );
            let (grid, shared) = if cell_arm {
                let (g, cells) = gpu.grid(ring, stride).expect("a shared grid");
                let prows = Rows::of_index(usize::from(prov.ws_row));
                let (_, alt) = gpu.grid(prows, stride).expect("a shared alternate");
                (Some(g), Some((cells, alt)))
            } else {
                (None, None)
            };
            let mut owned = if cell_arm {
                Owned::without_surfaces(&prov, &geom, Depth::DEFAULT)
            } else {
                Owned::with_geometry(&prov, &geom)
            };
            let mut t = match shared {
                Some((cells, alt)) => Term::new(owned.storage_over(cells, alt), &ws),
                None => Term::new(owned.storage(), &ws),
            };
            let (styles, styles_mem) = gpu.styles(&t).expect("shared styles");
            // The resolved table kept as the app keeps a set's: synced from
            // the core's log, so a frame resolves only what was written.
            let mut stamp = StyleStamp::default();
            // A fresh map per arm per round, like the renderer: the entries
            // live in the shared buffer, so the map is remade over it.
            map = GlyphMap::over(map.into_entries());
            let mut exceptions = Overlay::new();
            let mut s = Sums::default();
            let mut at = 0usize;
            let t0 = Instant::now();
            let mut idle_left = if cell_arm { idle_n } else { 0 };
            while at < data.len() || idle_left > 0 {
                let idle = at >= data.len();
                if idle {
                    idle_left -= 1;
                }
                let part = usize::from(idle);
                if cell_arm {
                    // The bench's stand-in for the frame loop's gate: no
                    // feed while the GPU reads the grid.
                    let tw = Instant::now();
                    while gpu.in_flight() == InFlight::Busy {
                        std::hint::spin_loop();
                    }
                    s.wait += tw.elapsed();
                }
                if !idle {
                    let n = 4096.min(data.len() - at);
                    t.write_at(BytePos::ZERO, &data[at..at + n]);
                    t.feed(BytePos::ZERO, ReadLen::from_index(n));
                    at += n;
                }
                if cell_arm {
                    // The styles first, as the app syncs them: the map walk
                    // reads each cell's face style from them.
                    let tr = Instant::now();
                    r.sync_styles(&t, styles_mem, &mut stamp);
                    s.parts[part].resolve += tr.elapsed();
                    let tc = Instant::now();
                    // Faithful to `present`'s slide branch: ensure the map
                    // over the view one line up (the leaving row) first, so
                    // the redundant walk it does is priced. `ensure` clears
                    // `candidates` and refills, so the screen ensure below
                    // still leaves them as the screen's -- the app's order.
                    if arm == Arm::Slide {
                        map.ensure(
                            &t,
                            t.view(Rows::of_index(1)),
                            styles_mem,
                            &mut r,
                            &mut candidates,
                        );
                    }
                    map.ensure(&t, t.viewport(), styles_mem, &mut r, &mut candidates);
                    s.parts[part].map += tc.elapsed();
                    let cursor = t.cursor_visible().then(|| {
                        let (row, col) = t.cursor();
                        (row, col, t.cursor_style())
                    });
                    let to = Instant::now();
                    exceptions.ensure(
                        &Scene {
                            term: &t,
                            view: t.viewport(),
                            styles: styles_mem,
                            cursor,
                            highlight: None,
                            map: &map,
                            candidates: &candidates,
                            // The bench prices the overlay walk; a settled
                            // frame's records do not slide, and folding a
                            // zero pan costs what folding a real one does.
                            pan: Px::of(0),
                            composition: None,
                        },
                        &mut r,
                        overlay_mem,
                        &mut records,
                    );
                    s.parts[part].overlay += to.elapsed();
                    s.cand.add(Tally::of(candidates.len()));
                    s.win.add(exceptions.stats().windows);
                    s.wcells.add(exceptions.stats().window_cells);
                    s.wmiss.add(exceptions.stats().window_misses);
                    s.rec.add(exceptions.stats().records);
                    s.pict.add(exceptions.stats().pictured);
                    let tp = Instant::now();
                    if arm == Arm::Copied {
                        let vp = t.viewport();
                        for y in 0..rows.index() {
                            RowBlock::nth(y, t.stride()).of_mut(copy_mem)[..vp.cols().index()]
                                .copy_from_slice(vp.row(Row::from_index(y)));
                        }
                    }
                    s.parts[part].copy += tp.elapsed();
                    let dt = tc.elapsed();
                    s.parts[part].frames.bump();
                    if !idle {
                        s.convert += dt;
                    }
                    if s.frames < first_n {
                        s.first_frames.bump();
                        s.first_convert += dt;
                        s.first_miss = map.stats().misses;
                    }
                    let (rotation, ring) = match arm == Arm::Copied {
                        true => (RowRot::NONE, rows),
                        false => (t.rotation(), t.ring()),
                    };
                    let input = CellInput {
                        grid: if arm == Arm::Copied {
                            &copy_grid
                        } else {
                            grid.as_ref().expect("grid")
                        },
                        styles: &styles,
                        map: &map_buffer,
                        stride: t.stride(),
                        rotation,
                        ring,
                        cursor,
                        cursor_color: None,
                        highlight: None,
                        overlay: &overlay,
                        records: &records,
                        slide: None,
                        // The bench drives one frame at a time, so a set a
                        // frame would say nothing; the presenter's wheel is
                        // what decides in the app.
                        set: SetIx(0),
                    };
                    let done = match arm {
                        Arm::Async | Arm::Slide => Done::Notify(Arc::clone(&sink)),
                        Arm::Present => {
                            // The identity the frame carries, captured the
                            // way `Presenter::on_done` captures it.
                            frame_no = frame_no.next();
                            let tag = FrameTag {
                                frame: frame_no,
                                swap: epoch.current(),
                                set: None,
                            };
                            let j = Arc::clone(&journal);
                            let e = Arc::clone(&epoch);
                            Done::Notify(Arc::new(move |landed| {
                                j.land(tag, &e, landed);
                            }))
                        }
                        _ => Done::Wait,
                    };
                    let Some(Drawn::Frame(g)) = gpu.draw_cells(&mut r, &input, &target, done)
                    else {
                        panic!("draw_cells");
                    };
                    // `FRAME_REBUILD_EVERY=0` never rebuilds: the control
                    // that says the counter can read zero. Without it a
                    // counter that only ever reported "superseded" would be
                    // making a claim rather than reporting one.
                    if arm == Arm::Present && rebuild_every != 0 && s.frames % rebuild_every == 0 {
                        // **The rebuild, taken while the frame is still in
                        // flight.** This is one line because that is all the
                        // app does: `Presenter::build` makes new surfaces and
                        // bumps the generation, and every frame committed
                        // against the old set is superseded from here.
                        epoch.bump();
                    }
                    s.parts[part].draw += g.upload + g.submit;
                    if idle {
                        // An idle frame's GPU time and the idle wait are not
                        // the replay's: they are reported with the parts.
                        async_gpu.swap(0, Ordering::Relaxed);
                        continue;
                    }
                    s.frames += 1;
                    s.draw += g.upload + g.submit;
                    s.bind += g.upload;
                    // A waited frame carries its own GPU time back; an
                    // asynchronous one cannot, because it has not finished
                    // -- its handler has the number and the arm idles until
                    // the completion has run, so what the sink holds here is
                    // the previous frame's. Over a round that costs the last
                    // frame and nothing else.
                    s.gpu += g.gpu.unwrap_or_else(|| {
                        Duration::from_nanos(async_gpu.swap(0, Ordering::Relaxed))
                    });
                    s.inst += g.instances.value();
                    continue;
                }
                let converted = r.convert(&t, Rows::ZERO, damage);
                s.frames += 1;
                s.convert += converted.convert;
                s.rows += converted.rows_dirty.value();
                match arm {
                    Arm::Cpu | Arm::Cpu2 => {
                        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
                        let f = r.paint(&mut target, converted);
                        s.draw += f.composite;
                        s.inst += f.instances.value();
                    }
                    Arm::Gpu => {
                        let Some(Drawn::Frame(g)) = gpu.draw(&mut r, &target, Done::Wait) else {
                            panic!("draw");
                        };
                        s.draw += g.upload + g.submit;
                        s.bind += g.upload;
                        s.gpu += g.gpu.unwrap_or_default();
                        s.inst += g.instances.value();
                    }
                    Arm::Cells | Arm::Async | Arm::Copied | Arm::Present | Arm::Slide => {
                        unreachable!("the cell arms continue above")
                    }
                }
            }
            while gpu.in_flight() == InFlight::Busy {
                std::hint::spin_loop();
            }
            s.total = t0.elapsed();
            if arm == Arm::Present {
                let c = journal.counts();
                println!(
                    "      present: {} landed, {} shown, {} superseded  \
(surfaces replaced under a frame in flight every {} frames)",
                    c.landed, c.shown, c.superseded, rebuild_every
                );
            }
            s.miss = map.stats().misses;
            s.ptest = map.stats().pair_tests;
            let per = |d: Duration| d.as_secs_f64() * 1e6 / s.frames as f64;
            println!(
                "{:<5} {:<6} {:>7} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>9.1} {:>7} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1} {:>9.1}",
                round,
                match arm {
                    Arm::Cpu => "cpu",
                    Arm::Gpu => "gpu",
                    Arm::Cpu2 => "cpu2",
                    Arm::Cells => "waited",
                    Arm::Async => "async",
                    Arm::Copied => "copied",
                    Arm::Present => "present",
                    Arm::Slide => "slide",
                },
                s.frames,
                per(s.total),
                per(s.convert),
                per(s.draw),
                per(s.bind),
                per(s.gpu),
                s.rows as f64 / s.frames as f64,
                s.inst as f64 / s.frames as f64,
                s.miss.value(),
                s.ptest.value() as f64 / s.frames as f64,
                s.cand.value() as f64 / s.frames as f64,
                s.win.value() as f64 / s.frames as f64,
                s.wcells.value() as f64 / s.frames as f64,
                s.wmiss.value() as f64 / s.frames as f64,
                s.rec.value() as f64 / s.frames as f64,
                s.pict.value() as f64 / s.frames as f64,
                per(s.wait),
            );
            for (kind, p) in ["after a read", "nothing read"].iter().zip(s.parts) {
                if p.frames.value() == 0 {
                    continue;
                }
                let n = u32::try_from(p.frames.value()).unwrap_or(u32::MAX);
                let us = |d: Duration| (d / n).as_secs_f64() * 1e6;
                println!(
                    "      {kind}: {} frames; map {:.1}, styles {:.1}, overlay {:.1}, copy {:.1}, draw {:.1}; cpu {:.1} us/frame",
                    p.frames.value(),
                    us(p.map),
                    us(p.resolve),
                    us(p.overlay),
                    us(p.copy),
                    us(p.draw),
                    us(p.map + p.resolve + p.overlay + p.copy + p.draw),
                );
            }
            let first_frames = usize::try_from(s.first_frames.value()).unwrap_or(usize::MAX);
            if first_n > 0 && first_frames > 0 && s.frames > first_frames {
                // Ratios without a cast: a `Duration` divides by a `u32`, and a
                // `u32` widens to `f64` by `From`; a count past `u32::MAX` is
                // saturated, which no replay here approaches.
                let n32 = |n| u32::try_from(n).unwrap_or(u32::MAX);
                let us_per = |d: Duration, n| (d / n32(n)).as_secs_f64() * 1e6;
                let per = |a, n| f64::from(n32(a)) / f64::from(n32(n));
                let rest = s.frames - first_frames;
                let first_miss = usize::try_from(s.first_miss.value()).unwrap_or(usize::MAX);
                let rest_miss = usize::try_from(s.miss.value())
                    .unwrap_or(usize::MAX)
                    .saturating_sub(first_miss);
                println!(
                    "      first {} frames: convert {:.1} us/frame, {} map misses ({:.1} a frame); \
the other {}: {:.1} us/frame, {} misses ({:.2} a frame)",
                    first_frames,
                    us_per(s.first_convert, first_frames),
                    first_miss,
                    per(first_miss, first_frames),
                    rest,
                    us_per(s.convert - s.first_convert, rest),
                    rest_miss,
                    per(rest_miss, rest),
                );
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("frame_gpu draws through Metal and runs on macOS only");
}
