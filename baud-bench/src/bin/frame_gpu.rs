//! `frame_gpu`: the CPU compositor against Metal on the same frames.
//!
//! Replays one recording through the core in `read(2)`-sized reads at
//! 200x50 cells, 14 px, and after every read converts the rows that
//! changed and then either paints them (the `cpu` arm, `Renderer::paint`)
//! or uploads and draws the whole frame on the GPU into an offscreen
//! texture, waiting for completion (the `gpu` arm, `Gpu::draw`). A third
//! arm, `cpu2`, is the CPU arm again -- the A/A control every sweep here
//! carries. The fourth, `cells`, converts nothing: it uploads the raw grid
//! and the resolved rendition table and lets the shader resolve every
//! cell (`Gpu::draw_cells`), keeping only the glyph map complete on the
//! CPU -- its `convert` column is that walk. It draws a smaller program
//! (no ligatures, no marks; see `baud_render::GlyphMap`), and the `marks`
//! column counts the cells whose marks it skipped. The four run
//! interleaved per round, a fresh renderer per arm per round, and every
//! round is printed so the reader discards the first.
//!
//! What it does not measure: the present. The CPU arm's frame is not
//! copied into a surface and the GPU arm's texture is not an IOSurface;
//! both are the same one memcpy away from the screen, and the `present`
//! column is a separate instrument.
//!
//! Usage: `frame_gpu <recording> [rounds] [full|incremental]`. macOS only.

#[cfg(target_os = "macos")]
fn main() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
    use baud_font::Face;
    use baud_present_macos::{CellInput, Gpu};
    use baud_render::{Blend, Damage, GlyphMap, PxSize, Renderer, Stack, Target, Theme};
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
            Cols::from_stride(usize::from(ws.ws_col)),
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
        "{:<5} {:<6} {:>7} {:>9} {:>9} {:>9} {:>9} {:>9} {:>8} {:>9} {:>7}",
        "round",
        "arm",
        "frames",
        "us/frame",
        "convert",
        "draw",
        "upload",
        "gpu",
        "rows",
        "inst",
        "marks"
    );
    /// One arm's sums over a replay.
    #[derive(Default)]
    struct Sums {
        frames: usize,
        total: Duration,
        convert: Duration,
        /// The paint on the CPU arm; upload plus submit on the GPU arm.
        draw: Duration,
        upload: Duration,
        gpu: Duration,
        rows: u64,
        inst: u64,
        marks: u64,
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Arm {
        Cpu,
        Gpu,
        Cpu2,
        Cells,
    }
    let arms = [Arm::Cpu, Arm::Gpu, Arm::Cpu2, Arm::Cells];
    let mut cells: Vec<baud_domain::PackedCell> = Vec::new();
    for round in 0..rounds {
        for arm in arms {
            let mut r = fresh();
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let mut s = Sums::default();
            let mut map = GlyphMap::new();
            let mut at = 0usize;
            let t0 = Instant::now();
            while at < data.len() {
                let n = 4096.min(data.len() - at);
                t.write_at(BytePos::ZERO, &data[at..at + n]);
                t.feed(BytePos::ZERO, ReadLen::from_index(n));
                at += n;
                if arm == Arm::Cells {
                    let tc = Instant::now();
                    cells.clear();
                    cells.extend_from_slice(t.viewport());
                    map.ensure(&cells, &mut r);
                    let cursor = t.cursor_visible().then(|| {
                        let (row, col) = t.cursor();
                        (row, col, t.cursor_style())
                    });
                    let styles = r.resolve_styles(&t).to_vec();
                    s.convert += tc.elapsed();
                    let input = CellInput {
                        cells: &cells,
                        styles: &styles,
                        map: &map,
                        cursor,
                    };
                    let g = gpu.draw_cells(&r, &input, &target).expect("draw_cells");
                    s.frames += 1;
                    s.draw += g.upload + g.submit;
                    s.upload += g.upload;
                    s.gpu += g.gpu;
                    s.inst += g.instances.value();
                    continue;
                }
                let converted = r.convert(&mut t, damage);
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
                        let g = gpu.draw(&r, &target).expect("draw");
                        s.draw += g.upload + g.submit;
                        s.upload += g.upload;
                        s.gpu += g.gpu;
                        s.inst += g.instances.value();
                    }
                    Arm::Cells => unreachable!("the cell arm continues above"),
                }
            }
            s.total = t0.elapsed();
            s.marks = map.stats().dropped_marks.value();
            let per = |d: Duration| d.as_secs_f64() * 1e6 / s.frames as f64;
            println!(
                "{:<5} {:<6} {:>7} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>9.1} {:>7}",
                round,
                match arm {
                    Arm::Cpu => "cpu",
                    Arm::Gpu => "gpu",
                    Arm::Cpu2 => "cpu2",
                    Arm::Cells => "cells",
                },
                s.frames,
                per(s.total),
                per(s.convert),
                per(s.draw),
                per(s.upload),
                per(s.gpu),
                s.rows as f64 / s.frames as f64,
                s.inst as f64 / s.frames as f64,
                s.marks,
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("frame_gpu draws through Metal and runs on macOS only");
}
