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

use baud_core::{Depth, grid_stride};
use baud_domain::{Row, RowBlock, RowRot};
#[cfg(target_os = "macos")]
fn main() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
    use baud_font::Face;
    use baud_present_macos::{CellInput, Done, Drawn, Gpu, InFlight};
    use baud_render::{Blend, Damage, GlyphMap, PxSize, Renderer, Stack, Target, Theme};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    let mut args = std::env::args().skip(1);
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
    let data = std::fs::read(format!(
        "{}/../tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("recording");
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
    let fresh = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
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
        "{:<5} {:<6} {:>7} {:>9} {:>9} {:>9} {:>9} {:>9} {:>8} {:>9} {:>7} {:>9}",
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
        "marks",
        "wait"
    );
    /// One arm's sums over a replay.
    #[derive(Default)]
    struct Sums {
        frames: usize,
        total: Duration,
        convert: Duration,
        /// The paint on the CPU arm; bind plus commit on the GPU arms.
        draw: Duration,
        bind: Duration,
        gpu: Duration,
        rows: u64,
        inst: u64,
        marks: u64,
        wait: Duration,
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
    }
    let arms = [
        Arm::Cpu,
        Arm::Gpu,
        Arm::Cpu2,
        Arm::Cells,
        Arm::Async,
        Arm::Copied,
    ];
    // The pitch every grid here is laid out at, the storage's and the GPU's,
    // so the copied arm's second buffer is the shape of the first.
    let geom = baud_bench::host_geometry();
    let stride = grid_stride(&geom, Cols::of_index(usize::from(ws.ws_col)));
    // The primary ring is the screen plus the scrollback; the copied arm's
    // buffer holds the screen alone, in screen order, because that is what
    // its copy is: the window, contiguous, under no rotation.
    let rows = Rows::of_index(usize::from(ws.ws_row));
    let ring = Owned::without_surfaces(&ws, &geom, Depth::DEFAULT).ring();
    let (copy_grid, copy_mem) = gpu.grid(rows, stride).expect("a second shared grid");
    let (map_buffer, map_mem) = gpu.map().expect("a shared map");
    let mut map = GlyphMap::over(map_mem);
    let noop: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    for round in 0..rounds {
        for arm in arms {
            let mut r = fresh();
            // The cell arms lend the terminal its viewport from the GPU;
            // the others take the storage's own.
            let cell_arm = matches!(arm, Arm::Cells | Arm::Async | Arm::Copied);
            let (grid, shared) = if cell_arm {
                let (g, cells) = gpu.grid(ring, stride).expect("a shared grid");
                let (_, alt) = gpu.grid(rows, stride).expect("a shared alternate");
                (Some(g), Some((cells, alt)))
            } else {
                (None, None)
            };
            let mut owned = if cell_arm {
                Owned::without_surfaces(&ws, &geom, Depth::DEFAULT)
            } else {
                Owned::with_geometry(&ws, &geom)
            };
            let mut t = match shared {
                Some((cells, alt)) => Term::new(owned.storage_over(cells, alt), &ws),
                None => Term::new(owned.storage(), &ws),
            };
            let (styles, styles_mem) = gpu.styles(&t).expect("shared styles");
            // A fresh map per arm per round, like the renderer: the entries
            // live in the shared buffer, so the map is remade over it.
            map = GlyphMap::over(map.into_entries());
            let mut s = Sums::default();
            let mut at = 0usize;
            let t0 = Instant::now();
            while at < data.len() {
                if cell_arm {
                    // The bench's stand-in for the frame loop's gate: no
                    // feed while the GPU reads the grid.
                    let tw = Instant::now();
                    while gpu.in_flight() == InFlight::Busy {
                        std::hint::spin_loop();
                    }
                    s.wait += tw.elapsed();
                }
                let n = 4096.min(data.len() - at);
                t.write_at(BytePos::ZERO, &data[at..at + n]);
                t.feed(BytePos::ZERO, ReadLen::from_index(n));
                at += n;
                if cell_arm {
                    let tc = Instant::now();
                    map.ensure(t.cells(), &mut r);
                    let cursor = t.cursor_visible().then(|| {
                        let (row, col) = t.cursor();
                        (row, col, t.cursor_style())
                    });
                    r.resolve_styles_into(&t, styles_mem);
                    if arm == Arm::Copied {
                        let vp = t.viewport();
                        for y in 0..rows.index() {
                            RowBlock::nth(y, t.stride()).of_mut(copy_mem)[..vp.cols().index()]
                                .copy_from_slice(vp.row(Row::from_index(y)));
                        }
                    }
                    s.convert += tc.elapsed();
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
                    };
                    let done = match arm {
                        Arm::Async => Done::Notify(Arc::clone(&noop)),
                        _ => Done::Wait,
                    };
                    let Some(Drawn::Frame(g)) = gpu.draw_cells(&mut r, &input, &target, done)
                    else {
                        panic!("draw_cells");
                    };
                    s.frames += 1;
                    s.draw += g.upload + g.submit;
                    s.bind += g.upload;
                    s.gpu += g.gpu;
                    s.inst += g.instances.value();
                    continue;
                }
                let converted = r.convert(&t, damage);
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
                        s.gpu += g.gpu;
                        s.inst += g.instances.value();
                    }
                    Arm::Cells | Arm::Async | Arm::Copied => {
                        unreachable!("the cell arms continue above")
                    }
                }
            }
            while gpu.in_flight() == InFlight::Busy {
                std::hint::spin_loop();
            }
            s.total = t0.elapsed();
            s.marks = map.stats().dropped_marks.value();
            let per = |d: Duration| d.as_secs_f64() * 1e6 / s.frames as f64;
            println!(
                "{:<5} {:<6} {:>7} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>9.1} {:>7} {:>9.1}",
                round,
                match arm {
                    Arm::Cpu => "cpu",
                    Arm::Gpu => "gpu",
                    Arm::Cpu2 => "cpu2",
                    Arm::Cells => "waited",
                    Arm::Async => "async",
                    Arm::Copied => "copied",
                },
                s.frames,
                per(s.total),
                per(s.convert),
                per(s.draw),
                per(s.bind),
                per(s.gpu),
                s.rows as f64 / s.frames as f64,
                s.inst as f64 / s.frames as f64,
                s.marks,
                per(s.wait),
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("frame_gpu draws through Metal and runs on macOS only");
}
