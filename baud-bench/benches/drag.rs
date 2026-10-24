//! `drag`: what a window drag builds on the cell path, and what of it is
//! drawn.
//!
//! Replays a recorded drag -- `tests/resize/<name>.raw` and `.steps`,
//! the sizes a shell was dragged through and the bytes it sent back at
//! each -- through the core, the renderer and a presenter on Metal, with
//! no window: the surfaces are real `IOSurface`s and the completions are
//! the GPU's own, arriving when the hardware says so. Only the pointer is
//! scripted, one step every `period` milliseconds.
//!
//! Each step does what `relayout` does: the core resized, the bytes fed,
//! the frame owed, and `present` asked for one. Between steps the loop
//! does what `pump` does on a completion's wake and on the display link's
//! tick -- build the owed frame once none is in flight, the wake only once
//! a display period has passed since the last commit. Two rules for the
//! step's own `present`, each run twice as its own A/A:
//!
//! * `build`: the frame is built whatever is in flight, and `draw_cells`
//!   refuses it when the GPU is still reading the last one -- the rule
//!   before 2026-10-24;
//! * `skip`: a step that finds a frame in flight builds nothing and leaves
//!   the frame owed, for the completion or the tick to build at the size
//!   the window has by then (`App::present`).
//!
//! Per arm and round it prints the frames built, the ones committed, the
//! ones refused as busy, the ones skipped, the ones that landed for a
//! surface set already replaced (never shown), the GPU time summed over
//! the landings, the main thread's build time summed over the frames
//! built, and whether the last frame shown is the drag's last size. The
//! first round is printed and to be discarded, as every bench here does.
//!
//! Usage: `drag [name] [period ms] [rounds]`, `drag`, `8` and `5` by
//! default; `BAUD_DRAG_PX` the cell font's size in device pixels, 28 by
//! default -- 14 points on a Retina panel. macOS only.

#[cfg(target_os = "macos")]
fn main() {
    // Imported here rather than at the top of the file: the bench is macOS
    // only, so a top-level `use` is unused elsewhere and `-D warnings` makes
    // that an error on the Linux lint.
    use baud_core::{Owned, Term, grid_stride};
    use baud_domain::{BytePos, Cols, ReadLen, Row, RowRot, Rows, WinSize};
    use baud_font::{Face, Px};
    use baud_present_macos::{CellInput, Drawn, Gpu, InFlight, Presenter, SurfaceSpace, Wake};
    use baud_render::{
        Blend, Candidate, GlyphMap, Instance, Overlay, PxSize, Renderer, Scene, Stack, StyleStamp,
        Tally, Theme,
    };
    use macos_liquid_glass::drawable::SurfaceHandle;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    /// The completions' wakes, counted: the loop's stand-in for the run
    /// loop delivering `wake:`.
    #[derive(Clone, Default)]
    struct Watch(Arc<AtomicU64>);
    impl Wake for Watch {
        fn present(&self, _: &SurfaceHandle) {}
        fn wake(&self) {
            self.0.fetch_add(1, Ordering::Release);
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Rule {
        Build,
        Skip,
    }

    /// One arm's counts over a drag.
    #[derive(Default)]
    struct Sums {
        built: Tally,
        committed: Tally,
        busy: Tally,
        skipped: Tally,
        gpu: Duration,
        build: Duration,
    }

    let mut args = baud_bench::args();
    let name = args.next().unwrap_or_else(|| "drag".to_string());
    let period_ms: f64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(8.0);
    let rounds: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5);
    let px: f32 = std::env::var("BAUD_DRAG_PX").map_or(28.0, |v| v.parse().expect("a size"));
    let dir = format!("{}/../tests/resize", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read(format!("{dir}/{name}.raw")).expect("the recording");
    let text = std::fs::read_to_string(format!("{dir}/{name}.steps")).expect("its steps");
    let mut steps: Vec<(WinSize, &[u8])> = Vec::new();
    let mut at = 0;
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let (size, len) = line.split_once(' ').expect("COLSxROWS <len>");
        let (c, r) = size.split_once('x').expect("COLSxROWS");
        let len: usize = len.parse().expect("a length");
        let ws = WinSize {
            ws_col: c.parse().expect("columns"),
            ws_row: r.parse().expect("rows"),
        };
        steps.push((ws, &raw[at..at + len]));
        at += len;
    }
    assert_eq!(at, raw.len(), "the steps account for the recording");
    let period = Duration::from_secs_f64(period_ms / 1e3);
    // The display period the wake's gate and the tick read: 120 Hz.
    let interval = Duration::from_secs_f64(1.0 / 120.0);

    let font = std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the test font");
    let faces = [Face::parse(&font).expect("a face")];
    // Provisioned as the app provisions, `MAX_GRID`.
    let prov = WinSize {
        ws_col: 400,
        ws_row: 150,
    };
    let geom = baud_bench::host_geometry();
    let stride = grid_stride(&geom, Cols::of_index(usize::from(prov.ws_col)));
    let first = WinSize { ..steps[0].0 };
    let last_ws = &steps[steps.len() - 1].0;
    println!(
        "drag: {name}, {} steps from {}x{} to {}x{}, one every {period_ms} ms, {px} px, {rounds} rounds",
        steps.len(),
        first.ws_col,
        first.ws_row,
        last_ws.ws_col,
        last_ws.ws_row
    );
    println!(
        "{:<5} {:<6} {:>6} {:>9} {:>5} {:>7} {:>10} {:>8} {:>9} {:>5}",
        "round",
        "arm",
        "built",
        "committed",
        "busy",
        "skipped",
        "superseded",
        "gpu ms",
        "build ms",
        "last"
    );
    let arms = [
        ("build", Rule::Build),
        ("skip", Rule::Skip),
        ("build2", Rule::Build),
        ("skip2", Rule::Skip),
    ];
    for round in 0..rounds {
        // Rotated each round, so no rule always runs first.
        for k in 0..arms.len() {
            let (label, rule) = arms[(k + round) % arms.len()];
            let mut owned = Owned::with_geometry(&prov, &geom);
            let mut t = Term::new(owned.storage(), &first);
            let mut r = Renderer::new(
                Stack::new(&faces).expect("a stack"),
                PxSize::of(px).expect("a size"),
                Theme::dark(),
                Blend::Native,
                Cols::of_index(usize::from(first.ws_col)),
                Rows::of_index(usize::from(first.ws_row)),
            )
            .expect("a renderer");
            let gpu = Gpu::new().expect("a Metal device");
            // Two of each, the presenter's two sets, as the app lends them.
            let styled: Vec<_> = (0..2).map(|_| gpu.styles(&t).expect("styles")).collect();
            let overlaid: Vec<_> = (0..2)
                .map(|_| {
                    gpu.overlay(
                        Cols::of_index(usize::from(prov.ws_col)),
                        Rows::of_index(usize::from(prov.ws_row)),
                    )
                    .expect("an overlay")
                })
                .collect();
            let copied: Vec<_> = (0..2)
                .map(|_| {
                    gpu.grid(Rows::of_index(usize::from(prov.ws_row) + 1), stride)
                        .expect("a copy")
                })
                .collect();
            let (map_buffer, map_mem) = gpu.map().expect("a map");
            let (styles, mut styles_mem): (Vec<_>, Vec<_>) = styled.into_iter().unzip();
            let (overlay, mut overlay_mem): (Vec<_>, Vec<_>) = overlaid.into_iter().unzip();
            let (copies, mut copies_mem): (Vec<_>, Vec<_>) = copied.into_iter().unzip();
            let mut stamps = [StyleStamp::default(); 2];
            let mut map = GlyphMap::over(map_mem);
            let mut exceptions = Overlay::new();
            let mut records: Vec<Instance> = Vec::new();
            let mut candidates: Vec<Candidate> = Vec::new();
            r.fit(&t);
            let (w, h) = r.whole_size();
            let mut p = Presenter::over(gpu, w, h, SurfaceSpace::Srgb).expect("a presenter");
            let watch = Watch::default();
            let mut s = Sums::default();
            let mut dirty = true;
            let mut last_commit = Instant::now() - interval;
            let mut committed_at: Option<(Px, Px)> = None;
            let mut landed_seen = 0u64;
            let mut wakes_seen = 0u64;

            // `present`, one rule or the other at a step and `pump`'s at a
            // wake or a tick: the frame built and committed, or not.
            let mut present = |t: &mut Term<'_>,
                               r: &mut Renderer<'_, '_>,
                               p: &mut Presenter,
                               s: &mut Sums,
                               step: bool|
             -> bool {
                r.fit(t);
                let (w, h) = r.whole_size();
                if p.size() != (w, h) {
                    p.resize(w, h).expect("surfaces");
                }
                if step && rule == Rule::Skip && p.in_flight() == InFlight::Busy {
                    s.skipped.bump();
                    return false;
                }
                let tb = Instant::now();
                let Some(at) = p.building() else {
                    return false;
                };
                let set = at.0;
                r.sync_styles(t, &mut styles_mem[set][..], &mut stamps[set]);
                let view = t.viewport();
                map.ensure(t, view, &styles_mem[set][..], r, &mut candidates);
                let (vc, vr) = (view.cols().index(), view.rows().index());
                let pitch = t.stride().index();
                for y in 0..vr {
                    let src = view.row(Row::from_index(y));
                    copies_mem[set][y * pitch..y * pitch + vc].copy_from_slice(&src[..vc]);
                }
                let cursor = t.cursor_visible().then(|| {
                    let (row, col) = t.cursor();
                    (row, col, t.cursor_style())
                });
                exceptions.ensure(
                    &Scene {
                        term: t,
                        view,
                        styles: &styles_mem[set][..],
                        cursor,
                        highlight: None,
                        map: &map,
                        candidates: &candidates,
                        pan: Px::of(0),
                        composition: None,
                    },
                    r,
                    &mut overlay_mem[set][..],
                    &mut records,
                );
                let input = CellInput {
                    grid: &copies[set],
                    styles: &styles[set],
                    map: &map_buffer,
                    stride: t.stride(),
                    rotation: RowRot::NONE,
                    ring: Rows::of_index(vr),
                    cursor,
                    highlight: None,
                    overlay: &overlay[set],
                    records: &records,
                    grounds: &[],
                    under: exceptions.under(),
                    chrome: Tally::default(),
                    slide: None,
                    set: at,
                    table: stamps[set],
                    map_gen: map.generation(),
                    reach: map.reach(),
                    moving: (records.len(), records.len()),
                };
                let drawn = p.draw_cells(watch.clone(), r, &input);
                s.build += tb.elapsed();
                s.built.bump();
                match drawn {
                    Some(Drawn::Frame(_)) => {
                        s.committed.bump();
                        committed_at = Some((w, h));
                        true
                    }
                    Some(Drawn::Busy) => {
                        s.busy.bump();
                        false
                    }
                    None => panic!("draw_cells made no resource"),
                }
            };

            let t0 = Instant::now();
            let mut next_tick = t0 + interval;
            let mut i = 0;
            let mut settle_until = None;
            loop {
                let now = Instant::now();
                // A landing's GPU time, read once a landing is counted: one
                // frame flies at a time, so each is seen before the next.
                let c = p.counts();
                if c.landed > landed_seen {
                    landed_seen = c.landed;
                    s.gpu += c.gpu;
                }
                let woke = watch.0.load(Ordering::Acquire);
                if woke != wakes_seen {
                    wakes_seen = woke;
                    if dirty
                        && last_commit.elapsed() >= interval
                        && p.in_flight() == InFlight::Idle
                        && present(&mut t, &mut r, &mut p, &mut s, false)
                    {
                        dirty = false;
                        last_commit = Instant::now();
                    }
                }
                if now >= next_tick {
                    next_tick += interval;
                    if dirty
                        && p.in_flight() == InFlight::Idle
                        && present(&mut t, &mut r, &mut p, &mut s, false)
                    {
                        dirty = false;
                        last_commit = Instant::now();
                    }
                }
                if i < steps.len() && now >= t0 + period.mul_f64(i as f64) {
                    let (ws, bytes) = (&steps[i].0, steps[i].1);
                    let _ = t.resize(ws);
                    for chunk in bytes.chunks(4096) {
                        t.write_at(BytePos::ZERO, chunk);
                        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
                    }
                    dirty = true;
                    if present(&mut t, &mut r, &mut p, &mut s, true) {
                        dirty = false;
                        last_commit = Instant::now();
                    }
                    i += 1;
                    continue;
                }
                if i == steps.len() {
                    let until = *settle_until.get_or_insert(now + Duration::from_secs(1));
                    let c = p.counts();
                    if !dirty && p.in_flight() == InFlight::Idle && c.landed == landed_seen {
                        break;
                    }
                    assert!(now < until, "{label}: the drag never settled");
                }
                std::hint::spin_loop();
            }
            let c = p.counts();
            r.fit(&t);
            let last = match committed_at == Some(r.whole_size()) && p.size() == r.whole_size() {
                true => "yes",
                false => "NO",
            };
            println!(
                "{:<5} {:<6} {:>6} {:>9} {:>5} {:>7} {:>10} {:>8.2} {:>9.2} {:>5}",
                round,
                label,
                s.built.value(),
                s.committed.value(),
                s.busy.value(),
                s.skipped.value(),
                c.superseded,
                s.gpu.as_secs_f64() * 1e3,
                s.build.as_secs_f64() * 1e3,
                last
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("drag: macOS only");
}
