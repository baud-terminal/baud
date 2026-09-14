//! The window, the responder, and the frame.
#![forbid(unsafe_code)]

use crate::host::{
    FIRST_BYTES, FIRST_COMMIT, FIRST_CONTENT, FIRST_DONE, FIRST_PRESENT, FIRST_TICK, MAX_GRID,
    describe, host_geometry, load_faces, startup_first, startup_mark, startup_note,
};
use crate::pty::{Pty, Stream};
use baud_core::{Depth, Owned, Screen, Term, grid_stride};
use baud_domain::{
    BytePos, Codepoint, Col, Cols, FnKey, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent,
    MouseKind, MouseMods, MouseTracking, ReadLen, Row, Rows, WinSize,
};
use baud_font::Px;
use baud_present_macos::clipboard;
use baud_present_macos::{
    CellInput, Drawn, Gpu, GpuFrame, GridBuffer, InFlight, MapBuffer, Mode, OverlayBuffer,
    Presented, Presenter, StylesBuffer, SurfaceSpace,
};
use baud_render::{
    Blend, Blink, Blinking, Candidate, Clip, Damage, Frame, GlyphMap, Grain, Instance, Overlay,
    PxSize, Renderer, Resolved, Scene, Scrolled, Selection, Span, Stack, Tally, Target, Theme,
    Window, back_for,
};
use macos_liquid_glass::drawable::{
    Button as ViewButton, DrawableView, Focus, Handled, KeyPress, Mouse,
    MouseKind as ViewMouseKind, Rect, Responder, Scroll, Waker,
};
use macos_liquid_glass::glass::{GlassStyle, GlassSurface};
use macos_liquid_glass::objc2::MainThreadOnly;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSImage, NSScreen,
    NSView, NSWindow, NSWindowButton, NSWindowStyleMask,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use macos_liquid_glass::window::GlassWindow;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// How a frame reaches the screen: through the GPU's completion and the
/// run loop, as every frame in the loop does, or on this thread with the
/// GPU waited for -- the first frame at launch, before a run loop exists.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Show {
    Event,
    Now,
}

/// Whether the screen needs painting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    Clean,
    Dirty,
}

/// The longest a synchronised update is honoured before a frame is drawn
/// anyway -- long enough that no real redraw is cut off, short enough that a
/// stalled update is not a frozen window. Two display-link periods at 60 Hz.
const SYNC_MAX: Duration = Duration::from_millis(33);
/// Half a blink cycle, seconds: text with `SGR 5` shows for this long, then
/// hides for this long. ~1.2 Hz, the middle of what terminals use.
const BLINK_HALF: f64 = 0.4;
/// Which path draws: the CPU compositor, Metal over the converted
/// instances, or Metal over the raw grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Path {
    Cpu,
    Instances,
    Cells,
}

/// What `BAUD_GPU` asks for.
///
/// **Unset is `Auto`: probe once and take the fastest path the machine
/// supports** -- the raw grid read by the shader where it lives, which costs
/// the CPU 15-20 us a frame on every recording; else Metal over the
/// converted instances; else the CPU. The cell path draws a smaller program
/// -- no cross-cell ligature and no attached mark, counted by
/// `MapStats::dropped_marks` at 55 cells a frame on cat-cjk -- and the owner
/// took that trade for the default (2026-09-13) until the hybrid lands;
/// `1` asks for the instances and their fidelity, `0` or `cpu` forces the
/// CPU for a comparison that needs it. A bench pins the path explicitly so
/// two arms never measure two tiers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Request {
    Auto,
    Force(Path),
}

fn wanted() -> Request {
    match std::env::var_os("BAUD_GPU") {
        None => Request::Auto,
        Some(v) if v == "1" || v == "gpu" || v == "instances" => Request::Force(Path::Instances),
        Some(v) if v == "cells" => Request::Force(Path::Cells),
        Some(v) if v == "0" || v == "cpu" => Request::Force(Path::Cpu),
        Some(v) => {
            crate::warn!("BAUD_GPU={v:?} is not 0, 1 or cells; choosing automatically");
            Request::Auto
        }
    }
}

/// The cell path's memory, all of it where the GPU reads it: the grid the
/// core writes, the styles the renderer resolves, the map the walk fills.
struct CellMemory {
    /// The overlay and the records that draw the cells it masks, and the
    /// cluster map the walk fills them from.
    overlay: OverlayBuffer,
    overlay_mem: &'static mut [u8],
    exceptions: Overlay,
    records: Vec<Instance>,
    candidates: Vec<Candidate>,
    /// The primary ring.
    grid: GridBuffer,
    /// The alternate screen's surface, bound while `Term::screen` says so.
    alt: GridBuffer,
    styles: StylesBuffer,
    styles_mem: &'static mut [Resolved],
    map_buffer: MapBuffer,
    map: GlyphMap<'static>,
}

/// What asked for a frame: the display link's tick, which is the vsync,
/// or a wake from another thread.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Tick,
    Wake,
}

/// Where the grid comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sizing {
    /// The window's size, through the view's resize callback.
    Window,
    /// `BAUD_GRID`: the window was sized to the grid, and the grid does
    /// not follow the window back. The callback fires with the view's
    /// default frame before it settles at the window's, and a pty resized
    /// through that transient is what a benchmark's child read as 120x40.
    Fixed,
}

struct App {
    term: Term<'static>,
    renderer: Renderer<'static, 'static>,
    presenter: Option<Presenter>,
    /// The GPU made before the terminal, on the cell path, so the grid
    /// could be lent from it; taken by the presenter when it exists.
    gpu: Option<Gpu>,
    cell: Option<CellMemory>,
    waker: Waker,
    pty: Pty,
    frame: Vec<u32>,
    frame_w: Px,
    frame_h: Px,
    /// The whole view in pixels: the surface the grid is centred in.
    view_px: (Px, Px),
    /// The letterbox margin the grid is centred by, so pointer math can
    /// take it off a view coordinate before naming a cell.
    pad_px: (Px, Px),
    /// The gutter in points, from the config; the grid leaves this on each
    /// side and the presenter clears it to the terminal background.
    padding: f64,
    scale: CGFloat,
    grid: WinSize,
    sizing: Sizing,
    /// Whether something changed since the last present.
    pending: Pending,
    /// How many lines above the live screen the window sits: zero is the
    /// live screen, and the bound is what the core retains. Anchored to
    /// content while output arrives, so the line being read stays put;
    /// a key press snaps it back to the bottom.
    back: Rows,
    /// The selection, over absolute lines of the live surface -- see
    /// `docs/research/notes/k-selection.md` -- and the rotation's advance
    /// since launch that numbers those lines.
    selection: Option<Selection>,
    scrolled: Scrolled,
    /// The screen the last pump saw: a switch is a different ring, and
    /// ends what a selection named.
    last_screen: Screen,
    /// Wheel travel not yet a whole line, in points.
    scroll_accum: f64,
    frames: Tally,
    last_report: std::time::Instant,
    stats: Frame,
    gpu_stats: GpuFrame,
    wanted: Path,
    /// What the surfaces' bytes mean; the presenter tags them with it.
    space: SurfaceSpace,
    /// The font size in points, for a rebuild at another backing scale,
    /// and the config's, which Actual Size returns to.
    font_size: f32,
    font_base: f32,
    /// When the last frame was committed, and how far apart the display
    /// link's ticks are: a wake draws at once when a frame's worth of time
    /// has passed, and leaves it to the tick otherwise, so a flood draws
    /// once a vsync and a keystroke draws now.
    last_commit: Instant,
    interval: Duration,
    last_tick: Option<f64>,
    /// When the terminal opened a synchronised update (`?2026`) that has not
    /// closed. A frame is held while one is open, so a half-drawn screen --
    /// btop's menu redraws inside one -- is never presented; but only up to
    /// [`SYNC_MAX`], so a program that opens one and stalls cannot freeze the
    /// window, which `RIS` does not clear.
    sync_since: Option<Instant>,
    /// The blink phase this frame draws with, whether any cell blinks, and
    /// whether the phase just flipped -- which forces a full repaint, since a
    /// blink changes no cell content and row damage would not catch it.
    blink: Blink,
    blink_present: Blinking,
    /// The damage the next frame draws with -- `Full` for the frame after a
    /// blink flip, since a blink changes no cell and row damage would miss it.
    next_damage: Damage,
}

/// What launch prepares before a window or a grid exists -- the cache
/// geometry, the GPU, the config, the faces and the renderer -- so the
/// child shell can be spawned and AppKit started with all of it done.
/// Under six milliseconds together; the point is what it lets overlap.
struct Boot {
    request: Request,
    geom: baud_domain::CacheGeometry,
    pitch: baud_domain::Stride,
    probe: std::thread::JoinHandle<Option<Gpu>>,
    renderer: Renderer<'static, 'static>,
    /// What the surfaces' bytes mean, from the config.
    space: SurfaceSpace,
    /// The font size in points; pixels are this times the backing scale.
    font_size: f32,
    /// The gutter in points around the grid, from the config.
    padding: f64,
}

impl Boot {
    fn prepare(scale: CGFloat) -> Boot {
        // **One probe, before the terminal.** Every path but the CPU's needs
        // the same device, `Gpu::new` compiles the shaders, and the cell path
        // has to know before the storage is placed whether the grid can live
        // where the shader reads it -- so the request is resolved here, once,
        // and `present` takes the device from `App::gpu` rather than asking
        // again. Each fallback says why on stderr: a probe that silently
        // draws a different program is the failure the counters exist to
        // catch.
        //
        // The storage, the font files and the faces live as long as the
        // process: the renderer borrows them for the life of the window. On
        // the cell path the viewport is not the storage's own: it is lent by
        // the GPU, so the grid the core writes is the grid the shader reads,
        // in place -- at the pitch the cache geometry chose, which is the
        // one pitch every allocator here agrees on through `grid_stride`.
        let request = wanted();
        let geom = host_geometry();
        startup_mark("geometry");
        let pitch = grid_stride(&geom, Cols::of_index(usize::from(MAX_GRID.ws_col)));
        crate::info!(
            "cache: {}; grid pitch {} cells, {} B a row",
            describe(&geom),
            pitch.index(),
            pitch.bytes()
        );
        // The Metal device on its own thread: creating it is 10-30 ms on
        // this box, and AppKit's initialisation on the main thread is what
        // it overlaps -- the probe is joined once the window exists.
        let probe = std::thread::spawn(move || {
            let g = match request {
                Request::Force(Path::Cpu) => None,
                _ => Gpu::new(),
            };
            if let Some(g) = &g {
                g.warm();
            }
            g
        });
        startup_mark("gpu probing");
        let cfg = crate::config::Config::load();
        startup_mark("config");
        match &cfg.source {
            Some(p) => crate::info!("config: {}", p.display()),
            None => crate::info!("config: none found; the built-in stack"),
        }
        for f in &cfg.faces {
            crate::info!("face: {}", f.path);
        }
        let faces = load_faces(&cfg);
        startup_mark("faces");
        let mut theme = Theme::dark();
        theme.bg_alpha = baud_domain::Chan::from_u8(cfg.background_alpha);
        let space = match cfg.colorspace {
            crate::config::Space::Srgb => SurfaceSpace::Srgb,
            crate::config::Space::DisplayP3 => SurfaceSpace::DisplayP3,
        };
        // At a placeholder grid: the renderer follows the terminal
        // (`Renderer::fit`) once the grid is known, and the cell metrics --
        // what the grid is computed from -- do not depend on it.
        let renderer = Renderer::new(
            Stack::new(faces).expect("faces"),
            PxSize::of(cfg.font_size * scale as f32).expect("size"),
            theme,
            Blend::Native,
            Cols::of_index(80),
            Rows::of_index(24),
        )
        .expect("cell metrics");
        startup_mark("renderer");
        Boot {
            request,
            geom,
            pitch,
            probe,
            renderer,
            space,
            font_size: cfg.font_size,
            padding: f64::from(cfg.padding),
        }
    }

    fn metrics_cell(&self) -> (f64, f64) {
        let m = self.renderer.metrics();
        (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()))
    }
}

impl App {
    fn new(ws: WinSize, sizing: Sizing, scale: CGFloat, waker: Waker, pty: Pty, boot: Boot) -> App {
        let Boot {
            request,
            geom,
            pitch,
            probe,
            mut renderer,
            space,
            font_size,
            padding,
        } = boot;
        let probe = probe.join().unwrap_or(None);
        startup_mark("gpu joined");
        let own_grid = |ws: &WinSize| {
            let owned: &'static mut Owned =
                Box::leak(Box::new(Owned::with_geometry(&MAX_GRID, &geom)));
            Term::new(owned.storage(), ws)
        };
        // **Both surfaces are the GPU's on the cell path.** The primary is
        // the ring -- the screen's capacity plus the scrollback -- and the
        // alternate screen is its own buffer of the capacity, so entering
        // it swaps which buffer the shader reads and copies nothing.
        let (mut wanted, gpu, term, grid) = match (request, probe) {
            (Request::Force(Path::Cells) | Request::Auto, Some(gpu)) => {
                let owned: &'static mut Owned = Box::leak(Box::new(Owned::without_surfaces(
                    &MAX_GRID,
                    &geom,
                    Depth::DEFAULT,
                )));
                match (gpu.grid(owned.ring(), pitch), gpu.grid(owned.rows(), pitch)) {
                    (Some((grid, cells)), Some((alt, alt_cells))) => {
                        let term = Term::new(owned.storage_over(cells, alt_cells), &ws);
                        (Path::Cells, Some(gpu), term, Some((grid, alt)))
                    }
                    _ => {
                        crate::warn!("no shared grid; drawing the instances");
                        (Path::Instances, Some(gpu), own_grid(&ws), None)
                    }
                }
            }
            (Request::Force(Path::Instances), Some(gpu)) => {
                (Path::Instances, Some(gpu), own_grid(&ws), None)
            }
            (Request::Force(Path::Cells | Path::Instances) | Request::Auto, None) => {
                crate::warn!("no Metal device or shaders; drawing on the CPU");
                (Path::Cpu, None, own_grid(&ws), None)
            }
            (Request::Force(Path::Cpu), _) => (Path::Cpu, None, own_grid(&ws), None),
        };
        renderer.fit(&term);
        let (frame_w, frame_h) = renderer.pixel_size();
        let cell = match (&gpu, grid) {
            (Some(gpu), Some((grid, alt))) => match (gpu.styles(&term), gpu.map()) {
                (Some((styles, styles_mem)), Some((map_buffer, entries))) => {
                    let (overlay, overlay_mem) = gpu
                        .overlay(
                            Cols::of_index(usize::from(MAX_GRID.ws_col)),
                            Rows::of_index(usize::from(MAX_GRID.ws_row)),
                        )
                        .expect("a shared overlay");
                    Some(CellMemory {
                        overlay,
                        overlay_mem,
                        exceptions: Overlay::new(),
                        records: Vec::new(),
                        candidates: Vec::new(),
                        grid,
                        alt,
                        styles,
                        styles_mem,
                        map_buffer,
                        map: GlyphMap::over(entries),
                    })
                }
                _ => {
                    crate::warn!("no shared styles or map; drawing the instances");
                    // The terminal's surfaces live in these buffers for the
                    // life of the process, whichever path draws them.
                    std::mem::forget(grid);
                    std::mem::forget(alt);
                    wanted = Path::Instances;
                    None
                }
            },
            _ => None,
        };
        crate::info!(
            "renderer: {wanted:?}{}",
            match request {
                Request::Auto => " (chosen)",
                Request::Force(_) => " (requested)",
            }
        );
        App {
            term,
            renderer,
            presenter: None,
            gpu,
            cell,
            waker,
            pty,
            frame: vec![0; slots(frame_w) * slots(frame_h)],
            frame_w,
            frame_h,
            view_px: (frame_w, frame_h),
            pad_px: (Px::of(0), Px::of(0)),
            padding,
            scale,
            grid: ws,
            sizing,
            pending: Pending::Dirty,
            back: Rows::ZERO,
            selection: None,
            scrolled: Scrolled::START,
            last_screen: Screen::Normal,
            scroll_accum: 0.0,
            frames: Tally::default(),
            last_report: std::time::Instant::now(),
            stats: Frame::default(),
            gpu_stats: GpuFrame::default(),
            wanted,
            space,
            font_size,
            font_base: font_size,
            last_commit: Instant::now(),
            interval: Duration::from_micros(8333),
            last_tick: None,
            sync_since: None,
            blink: Blink::default(),
            blink_present: Blinking::default(),
            next_damage: Damage::Incremental,
        }
    }

    fn metrics_cell(&self) -> (f64, f64) {
        let m = self.renderer.metrics();
        (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()))
    }

    /// Sends a key to the core and whatever it answers to the shell.
    fn send(&mut self, e: KeyEvent) {
        self.term.key(e);
        self.flush_reply();
    }

    /// Where the window sits in the ring, for the scrollbar.
    fn window(&self) -> Window {
        Window {
            retained: Rows::of_count(self.term.scrollback().len()),
            back: self.back,
            rows: self.term.viewport().rows(),
        }
    }

    fn flush_reply(&mut self) {
        let reply = self.term.reply().to_vec();
        if !reply.is_empty() {
            self.pty.write(&reply);
            self.term.clear_reply();
        }
    }

    fn present(&mut self, screen: &DrawableView, show: Show) {
        // The surface is the renderer's size, so the renderer follows the
        // terminal first; the cell path never converts and would otherwise
        // keep the grid it started with.
        self.renderer.fit(&self.term);
        // The selection's share of this view, for every path's one rule.
        let highlight = {
            let view = self.term.view(self.back);
            let top = self.scrolled.top(self.back);
            self.selection
                .and_then(|s| s.highlight(top, view.rows(), view.cols()))
        };
        self.renderer.set_highlight(highlight);
        // The scrollbar's thumb, where the window sits in the ring; none on
        // the live screen.
        self.renderer.set_scrollbar(Some(self.window()));
        // The grid, and the whole surface it is centred in; the letterbox
        // margin between them is the gap.
        let (w, h) = self.renderer.pixel_size();
        let (sw, sh) = self.view_px;
        self.pad_px = (
            Px::of(sw.value().saturating_sub(w.value()) / 2),
            Px::of(sh.value().saturating_sub(h.value()) / 2),
        );
        let surface_stale = self.presenter.as_ref().map(Presenter::size) != Some((sw, sh));
        if (w, h) != (self.frame_w, self.frame_h) || surface_stale || self.presenter.is_none() {
            self.frame_w = w;
            self.frame_h = h;
            self.frame = vec![0; slots(w) * slots(h)];
            self.presenter = match (self.wanted, self.gpu.take(), self.presenter.take()) {
                // A presenter that exists keeps its GPU across a resize.
                (Path::Instances | Path::Cells, _, Some(mut p)) if p.mode() == Mode::Gpu => {
                    p.resize(sw, sh).map(|()| p)
                }
                (Path::Instances | Path::Cells, Some(gpu), _) => {
                    Presenter::over(gpu, sw, sh, self.space)
                }
                // The device was taken by a presenter that then declined
                // its targets; probe once more, and say so if that fails.
                (Path::Instances | Path::Cells, None, _) => Presenter::with_gpu(sw, sh, self.space)
                    .or_else(|| {
                        crate::warn!("no Metal device, shaders or targets; drawing on the CPU");
                        Presenter::new(sw, sh, self.space)
                    }),
                (Path::Cpu, _, _) => Presenter::new(sw, sh, self.space),
            };
            if let Some(p) = &self.presenter {
                crate::info!("presenting through {:?}, memory {:?}", p.mode(), p.memory());
                if std::env::var_os("BAUD_DEBUG_GEOMETRY").is_some() {
                    let (cw, ch) = self.metrics_cell();
                    let (rc, rr) = self.renderer.grid();
                    let (tc, tr) = self.term.geometry();
                    eprintln!(
                        "geometry: cell {cw}x{ch} px, renderer {}x{} = {}x{} px, term {}x{}, pty {}x{}, scale {}",
                        rc.index(),
                        rr.index(),
                        w.value(),
                        h.value(),
                        tc.index(),
                        tr.index(),
                        self.grid.ws_col,
                        self.grid.ws_row,
                        self.scale
                    );
                }
            }
            self.pending = Pending::Dirty;
        }
        if self.pending == Pending::Clean {
            return;
        }
        self.renderer.set_blink(self.blink);
        let damage = self.next_damage;
        self.next_damage = Damage::Incremental;
        let waker = self.waker;
        match (
            self.presenter.as_ref().map(Presenter::mode),
            self.cell.as_mut(),
        ) {
            (Some(Mode::Gpu), Some(cell)) => {
                let t0 = Instant::now();
                // The window `back` lines up the ring: the shader's rotation
                // retreats by that and reads the retained rows where they
                // are, so a scrolled-back frame costs what a live one costs.
                let view = self.term.view(self.back);
                cell.map
                    .ensure(view, &mut self.renderer, &mut cell.candidates);
                let cursor = (self.term.cursor_visible() && self.back.index() == 0).then(|| {
                    let (r, c) = self.term.cursor();
                    (r, c, self.term.cursor_style())
                });
                self.renderer
                    .resolve_styles_into(&self.term, cell.styles_mem);
                cell.exceptions.ensure(
                    &Scene {
                        term: &self.term,
                        view,
                        styles: cell.styles_mem,
                        cursor,
                        highlight,
                        map: &cell.map,
                        candidates: &cell.candidates,
                    },
                    &mut self.renderer,
                    cell.overlay_mem,
                    &mut cell.records,
                );
                // The thumb rides with the records, drawn after the cell passes.
                cell.records.extend_from_slice(self.renderer.scrollbar());
                self.stats = Frame {
                    convert: t0.elapsed(),
                    ..Frame::default()
                };
                if !FIRST_COMMIT.load(Ordering::Relaxed)
                    || (FIRST_BYTES.load(Ordering::Relaxed)
                        && !FIRST_CONTENT.load(Ordering::Relaxed))
                {
                    startup_note(&format!(
                        "walk: {} map misses, {} window misses, {:.1} ms",
                        cell.map.stats().misses.value(),
                        cell.exceptions.stats().window_misses.value(),
                        self.stats.convert.as_secs_f64() * 1e3
                    ));
                }
                let input = CellInput {
                    grid: match self.term.screen() {
                        baud_core::Screen::Normal => &cell.grid,
                        baud_core::Screen::Alternate => &cell.alt,
                    },
                    styles: &cell.styles,
                    map: &cell.map_buffer,
                    stride: self.term.stride(),
                    rotation: view.rotation(),
                    ring: self.term.ring(),
                    cursor,
                    highlight,
                    overlay: &cell.overlay,
                    records: &cell.records,
                };
                if let Some(p) = &mut self.presenter {
                    let drawn = match show {
                        Show::Event => p.draw_cells(waker, &mut self.renderer, &input),
                        Show::Now => p.draw_cells_now(screen, &mut self.renderer, &input),
                    };
                    match drawn {
                        Some(Drawn::Frame(g)) => {
                            startup_first("first commit", &FIRST_COMMIT);
                            if show == Show::Now {
                                startup_first("first present", &FIRST_PRESENT);
                            }
                            if FIRST_BYTES.load(Ordering::Relaxed) {
                                startup_first("first content", &FIRST_CONTENT);
                            }
                            self.gpu_stats = g;
                        }
                        // Still in flight: the completion wakes us.
                        Some(Drawn::Busy) => return,
                        None => {}
                    }
                }
            }
            (Some(Mode::Gpu), None) => {
                let converted = self.renderer.convert(&self.term, self.back, damage);
                self.stats = Frame {
                    rows_dirty: converted.rows_dirty,
                    rows: converted.rows,
                    convert: converted.convert,
                    ..Frame::default()
                };
                if let Some(p) = &mut self.presenter {
                    let drawn = match show {
                        Show::Event => p.draw(waker, &mut self.renderer),
                        Show::Now => p.draw_now(screen, &mut self.renderer),
                    };
                    match drawn {
                        Some(Drawn::Frame(g)) => {
                            startup_first("first commit", &FIRST_COMMIT);
                            if show == Show::Now {
                                startup_first("first present", &FIRST_PRESENT);
                            }
                            if FIRST_BYTES.load(Ordering::Relaxed) {
                                startup_first("first content", &FIRST_CONTENT);
                            }
                            self.gpu_stats = g;
                        }
                        Some(Drawn::Busy) => return,
                        None => {}
                    }
                }
            }
            _ => {
                let stats = {
                    let mut target = Target::new(&mut self.frame, w, w, h).expect("target");
                    self.renderer.render(&mut self.term, &mut target, damage)
                };
                self.stats = stats;
                let clear = self.renderer.clear_color();
                if let Some(p) = &mut self.presenter {
                    if p.present(screen, &self.frame, w, clear) == Presented::Yes {
                        startup_first("first present", &FIRST_PRESENT);
                        if FIRST_BYTES.load(Ordering::Relaxed) {
                            startup_first("first content", &FIRST_CONTENT);
                        }
                    }
                }
            }
        }
        self.blink_present = self.renderer.blink_seen();
        self.pending = Pending::Clean;
        self.last_commit = Instant::now();
        self.frames.bump();
        if self.last_report.elapsed().as_secs() >= 5 {
            match self.presenter.as_ref().map(Presenter::mode) {
                Some(Mode::Gpu) => crate::info!(
                    "{} frames; last: {} rows, {} instances, convert {:?}, bind {:?}, commit {:?}, gpu {:?}",
                    self.frames.value(),
                    self.stats.rows_dirty.value(),
                    self.gpu_stats.instances.value(),
                    self.stats.convert,
                    self.gpu_stats.upload,
                    self.gpu_stats.submit,
                    self.gpu_stats.gpu
                ),
                _ => crate::info!(
                    "{} frames; last: {} rows, {} instances, convert {:?}, paint {:?}, cache {:?}",
                    self.frames.value(),
                    self.stats.rows_dirty.value(),
                    self.stats.instances.value(),
                    self.stats.convert,
                    self.stats.composite,
                    self.stats.cache
                ),
            }
            self.last_report = std::time::Instant::now();
        }
    }
}

fn slots(p: Px) -> usize {
    usize::try_from(p.value()).unwrap_or(0)
}

fn mods(m: macos_liquid_glass::drawable::Modifiers) -> Modifiers {
    let mut out = Modifiers::NONE;
    if m.shift {
        out = out.with(Modifiers::SHIFT);
    }
    if m.control {
        out = out.with(Modifiers::CTRL);
    }
    if m.option {
        out = out.with(Modifiers::ALT);
    }
    if m.command {
        out = out.with(Modifiers::SUPER);
    }
    out
}

/// The keys a macOS virtual key code names outright, before any text.
fn special(key: &KeyPress) -> Option<KeyCode> {
    Some(match key.key_code {
        0x7E => KeyCode::Up,
        0x7D => KeyCode::Down,
        0x7C => KeyCode::Right,
        0x7B => KeyCode::Left,
        0x73 => KeyCode::Home,
        0x77 => KeyCode::End,
        0x74 => KeyCode::PageUp,
        0x79 => KeyCode::PageDown,
        0x75 => KeyCode::Delete,
        0x72 => KeyCode::Insert,
        0x33 => KeyCode::Backspace,
        0x24 | 0x4C => KeyCode::Enter,
        0x30 => KeyCode::Tab,
        0x35 => KeyCode::Escape,
        0x7A => KeyCode::Function(FnKey::new(1)?),
        0x78 => KeyCode::Function(FnKey::new(2)?),
        0x63 => KeyCode::Function(FnKey::new(3)?),
        0x76 => KeyCode::Function(FnKey::new(4)?),
        0x60 => KeyCode::Function(FnKey::new(5)?),
        0x61 => KeyCode::Function(FnKey::new(6)?),
        0x62 => KeyCode::Function(FnKey::new(7)?),
        0x64 => KeyCode::Function(FnKey::new(8)?),
        0x65 => KeyCode::Function(FnKey::new(9)?),
        0x6D => KeyCode::Function(FnKey::new(10)?),
        0x67 => KeyCode::Function(FnKey::new(11)?),
        0x6F => KeyCode::Function(FnKey::new(12)?),
        _ => return None,
    })
}

/// The view's responder, with the view it reports to.
struct Driver {
    app: App,
    view: macos_liquid_glass::objc2::rc::Retained<DrawableView>,
    /// The Dock icon, set on the first display-link tick: vImage converts
    /// the image for the Dock on this thread, ~10 ms, and nothing on
    /// screen waits for it.
    dock_icon: Option<Retained<NSImage>>,
    /// The button a drag reports: the one that went down, until it comes
    /// up. AppKit's motion events name no button.
    held: Option<MouseButton>,
    /// A drag of the scrollbar's thumb: where it was grabbed, in frame
    /// pixels, and where its top was.
    bar_drag: Option<BarDrag>,
}

#[derive(Clone, Copy)]
struct BarDrag {
    grab_y: Px,
    top: Px,
}

/// Whether the scrollbar took a mouse event.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Took {
    Yes,
    No,
}

impl Driver {
    /// One turn of the loop, on any event: while a frame is in flight the
    /// bytes stay in the channel, because the grid they would write is
    /// the grid the GPU is reading, and the completion wakes us. Otherwise
    /// feed everything that arrived, then draw -- at once on a tick, and
    /// on a wake only if a frame's worth of time has passed since the last
    /// commit, so a flood draws once a vsync and a keystroke draws now.
    fn pump(&mut self, trigger: Trigger) {
        if self.app.presenter.as_ref().map(Presenter::in_flight) == Some(InFlight::Busy) {
            return;
        }
        let drained = self.app.pty.drain();
        if !drained.chunks.is_empty() {
            startup_first("first bytes", &FIRST_BYTES);
        }
        let before = self.app.term.rotation();
        for chunk in &drained.chunks {
            self.app.term.write_at(BytePos::ZERO, chunk);
            self.app
                .term
                .feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
            self.app.pending = Pending::Dirty;
        }
        // Looking back while output scrolls: the window stays on the lines
        // it was showing, which means it moves up the ring by however many
        // rows the screen scrolled -- the rotation's advance -- and no
        // further than what is retained.
        let ring = self.app.term.ring().index();
        let advanced = (self.app.term.rotation().index() + ring - before.index()) % ring;
        let retained = self.app.term.scrollback().len();
        if self.app.back.index() > 0 {
            self.app.back = Rows::of_count((self.app.back.index() + advanced).min(retained));
        }
        // The selection names absolute lines; the count that numbers them
        // advances with the screen. A surface switch is another ring, and a
        // line the ring recycled is clipped away.
        self.app.scrolled = self.app.scrolled.advanced(Rows::of_count(advanced));
        let screen = self.app.term.screen();
        if screen != self.app.last_screen {
            self.app.last_screen = screen;
            self.app.selection = None;
        }
        let oldest = self.app.scrolled.oldest(Rows::of_count(retained));
        let gone = self
            .app
            .selection
            .as_mut()
            .is_some_and(|s| s.clip(oldest) == Clip::Cleared);
        if gone {
            self.app.selection = None;
        }
        self.app.flush_reply();
        if drained.stream == Stream::Closed {
            // The child is gone: the window has nothing left to show.
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm).terminate(None);
            }
        }
        if self.app.pending != Pending::Dirty {
            return;
        }
        // Hold the frame while the application is mid synchronised update, so
        // a partial redraw is never shown -- btop wraps its menu redraw in
        // one, and presenting between the `?2026h` and the `?2026l` flickered
        // 89 times over a ten-second run. Not past SYNC_MAX, or an update that
        // opens and stalls would freeze the window, which `RIS` does not
        // clear. The closing `?2026l` feeds in and wakes us, and this call
        // then presents the finished screen.
        if self.app.term.synchronized_output() {
            let since = *self.app.sync_since.get_or_insert_with(Instant::now);
            if since.elapsed() < SYNC_MAX {
                return;
            }
        } else {
            self.app.sync_since = None;
        }
        let due = trigger == Trigger::Tick || self.app.last_commit.elapsed() >= self.app.interval;
        if due {
            let view = self.view.clone();
            self.app.present(&view, Show::Event);
        }
    }
}

impl Responder for Driver {
    fn key(&mut self, key: &KeyPress) -> Handled {
        if key.modifiers.command {
            // Menu territory: a key equivalent the menu carries never gets
            // here, and one it does not sends nothing to the child.
            return Handled::Consumed;
        }
        self.snap_to_live();
        let m = mods(key.modifiers);
        if let Some(code) = special(key) {
            self.app.send(KeyEvent::press(code, m));
            return Handled::Consumed;
        }
        if key.modifiers.control || key.modifiers.option {
            // A control or alt chord: the key's identity, folded by the core.
            let Some(c) = key
                .unmodified
                .chars()
                .next()
                .and_then(|c| Codepoint::new(u32::from(c)))
            else {
                return Handled::Consumed;
            };
            let mut e = KeyEvent::press(KeyCode::Char(c), m);
            e.text = key
                .characters
                .chars()
                .next()
                .filter(|ch| !ch.is_control())
                .and_then(|ch| Codepoint::new(u32::from(ch)));
            self.app.send(e);
            return Handled::Consumed;
        }
        Handled::Interpret
    }

    fn text(&mut self, text: &str) {
        for ch in text.chars() {
            let Some(c) = Codepoint::new(u32::from(ch)) else {
                continue;
            };
            let lower = ch.to_lowercase().next().unwrap_or(ch);
            let identity = Codepoint::new(u32::from(lower)).unwrap_or(c);
            let mut e = KeyEvent::press(KeyCode::Char(identity), Modifiers::NONE);
            e.text = Some(c);
            self.app.send(e);
        }
    }

    fn command(&mut self, selector: &str) {
        match selector {
            "copy:" => return self.copy(),
            "paste:" => return self.paste(),
            "zoomIn:" => return self.zoom(self.app.font_size + 1.0),
            "zoomOut:" => return self.zoom(self.app.font_size - 1.0),
            "zoomActual:" => return self.zoom(self.app.font_base),
            _ => {}
        }
        let code = match selector {
            "insertNewline:" | "insertLineBreak:" => KeyCode::Enter,
            "deleteBackward:" => KeyCode::Backspace,
            "deleteForward:" => KeyCode::Delete,
            "insertTab:" => KeyCode::Tab,
            "insertBacktab:" => KeyCode::Tab,
            "cancelOperation:" => KeyCode::Escape,
            "moveUp:" => KeyCode::Up,
            "moveDown:" => KeyCode::Down,
            "moveLeft:" => KeyCode::Left,
            "moveRight:" => KeyCode::Right,
            "scrollPageUp:" => KeyCode::PageUp,
            "scrollPageDown:" => KeyCode::PageDown,
            _ => return,
        };
        let m = if selector == "insertBacktab:" {
            Modifiers::SHIFT
        } else {
            Modifiers::NONE
        };
        self.app.send(KeyEvent::press(code, m));
    }

    fn marked_text(&mut self, _text: &str) {}

    /// A press, release or motion, reported to the application when it
    /// asked for that kind: the core keeps the tracking level and the
    /// encoding, refuses what the level does not cover and what X10 cannot
    /// encode, and this only turns the view's pixels into a cell.
    fn mouse(&mut self, event: &Mouse) {
        if self.scrollbar(event) == Took::Yes {
            return;
        }
        // The child's, when it tracks the mouse -- unless Shift is held,
        // which is a selection whatever the child asked for: xterm's
        // default, ctlseqs' XTSHIFTESCAPE at 0.
        if self.app.term.mouse_tracking() == MouseTracking::Off || event.modifiers.shift {
            self.select(event);
            return;
        }
        let (row, col) = self.cell_under(event.x, event.y);
        let pressed = match event.button {
            ViewButton::Left => MouseButton::Left,
            ViewButton::Middle => MouseButton::Middle,
            ViewButton::Right => MouseButton::Right,
            ViewButton::Other(_) => MouseButton::None,
        };
        let (kind, button) = match event.kind {
            ViewMouseKind::Down => {
                self.held = Some(pressed);
                (MouseKind::Press, pressed)
            }
            ViewMouseKind::Up => {
                self.held = None;
                (MouseKind::Release, pressed)
            }
            ViewMouseKind::Dragged => (MouseKind::Motion, self.held.unwrap_or(pressed)),
            ViewMouseKind::Moved => (MouseKind::Motion, MouseButton::None),
            ViewMouseKind::Entered | ViewMouseKind::Exited => return,
        };
        let e = MouseEvent {
            button,
            mods: mouse_mods(event.modifiers),
            kind,
            col,
            row,
        };
        if self.app.term.mouse_report(e) {
            self.app.flush_reply();
        }
    }

    /// The wheel moves the window over the history. A trackpad sends
    /// points and a wheel sends lines; both accumulate to whole rows of the
    /// cell height, positive `dy` being "show earlier lines" the way AppKit
    /// delivers it. The alternate screen keeps no history and gets nothing
    /// here; a pager or an editor on it will want the wheel as keys, which
    /// is a separate protocol.
    fn scroll(&mut self, event: &Scroll) {
        let (_, ch) = self.app.metrics_cell();
        let lines = match event.precise {
            true => {
                self.app.scroll_accum += event.dy * self.app.scale;
                let whole = (self.app.scroll_accum / ch).trunc();
                self.app.scroll_accum -= whole * ch;
                whole
            }
            false => event.dy.trunc(),
        };
        // Whole lines, signed; the retained count bounds the window above
        // and the live screen bounds it below.
        let step = lines as i64;
        if step == 0 {
            return;
        }
        // An application tracking the mouse gets the wheel as buttons 4
        // and 5, one press a line, at the pointer; one on the alternate
        // screen without tracking gets it as the arrow keys, which is what
        // a pager or an editor there reads; the primary screen without
        // tracking is the window over the history.
        if self.app.term.mouse_tracking() != MouseTracking::Off {
            let (row, col) = self.cell_under(event.x, event.y);
            let button = if step > 0 {
                MouseButton::WheelUp
            } else {
                MouseButton::WheelDown
            };
            let mut sent = false;
            for _ in 0..step.unsigned_abs().min(WHEEL_MAX) {
                sent |= self.app.term.mouse_report(MouseEvent {
                    button,
                    mods: MouseMods::NONE,
                    kind: MouseKind::Press,
                    col,
                    row,
                });
            }
            if sent {
                self.app.flush_reply();
            }
            return;
        }
        if self.app.term.screen() != baud_core::Screen::Normal {
            let code = if step > 0 { KeyCode::Up } else { KeyCode::Down };
            for _ in 0..step.unsigned_abs().min(WHEEL_MAX) {
                self.app.send(KeyEvent::press(code, Modifiers::default()));
            }
            return;
        }
        let retained = i64::try_from(self.app.term.scrollback().len()).unwrap_or(i64::MAX);
        let now = i64::try_from(self.app.back.index()).unwrap_or(0);
        let back = Rows::of_count(usize::try_from((now + step).clamp(0, retained)).unwrap_or(0));
        if back.index() == self.app.back.index() {
            return;
        }
        self.app.back = back;
        self.app.pending = Pending::Dirty;
        self.pump(Trigger::Wake);
    }

    fn resized(&mut self, size: NSSize, scale: CGFloat) {
        if scale != self.app.scale {
            // A different scale is a different pixel size for the font: the
            // renderer's metrics, scales, run cache and atlas are rebuilt,
            // and the cell path's map and overlay rebuild on the atlas's
            // grow count. The comment that stood here said this happened
            // and it did not: the glyphs stayed at the old scale, half-size
            // on a 2x surface after a resize.
            self.app.scale = scale;
            if let Some(px) = PxSize::of(self.app.font_size * scale as f32) {
                if self.app.renderer.rescale(px).is_some() {
                    self.app.next_damage = Damage::Full;
                    self.app.pending = Pending::Dirty;
                }
            }
        }
        self.relayout(size, scale);
    }

    fn frame(&mut self, target: f64) {
        startup_first("first tick", &FIRST_TICK);
        if let (Some(img), Some(mtm)) = (self.dock_icon.take(), MainThreadMarker::new()) {
            macos_liquid_glass::set_application_icon(&NSApplication::sharedApplication(mtm), &img);
            startup_mark("dock icon");
        }
        if let Some(last) = self.app.last_tick {
            let dt = target - last;
            if dt > 0.001 && dt < 0.1 {
                self.app.interval = Duration::from_secs_f64(dt);
            }
        }
        self.app.last_tick = Some(target);
        // Blink is driven off the display link's timestamp -- the hardware
        // vsync clock -- so the phase is accurate without a timer of its own.
        // A full cycle is two BLINK_HALF periods; the phase flips a repaint
        // only when a cell actually blinks.
        let phase = if (target / BLINK_HALF).rem_euclid(2.0) < 1.0 {
            Blink::Shown
        } else {
            Blink::Hidden
        };
        if phase != self.app.blink {
            self.app.blink = phase;
            if self.app.blink_present == Blinking::Seen {
                self.app.next_damage = Damage::Full;
                self.app.pending = Pending::Dirty;
            }
        }
        self.pump(Trigger::Tick);
    }

    fn wake(&mut self) {
        if self.app.frames.value() > 0
            && self.app.presenter.as_ref().map(Presenter::in_flight) != Some(InFlight::Busy)
        {
            startup_first("first completion", &FIRST_DONE);
        }
        self.pump(Trigger::Wake);
    }

    fn focus(&mut self, _focus: Focus) {}

    fn cursor_rect(&self) -> Rect {
        let (r, c) = self.app.term.cursor();
        let (cw, ch) = self.app.metrics_cell();
        let s = self.app.scale;
        Rect {
            x: c.index() as f64 * cw / s,
            y: r.index() as f64 * ch / s,
            w: cw / s,
            h: ch / s,
        }
    }
}

/// baud's icon, rasterised from `assets/logo/` and embedded, so it needs no
/// bundle. The dock wants the mark with its own padding, the way an app icon
/// sits in the Dock; the titlebar wants it snug against its box, or the strip
/// makes it tiny -- so the two read from different crops of the same mark.
fn icon() -> Option<Retained<NSImage>> {
    // The bare mark on a transparent ground -- just the glyph, no square behind
    // it. The SVG itself, not a raster, so NSImage renders it as a vector with
    // real transparency. This one keeps the 512-box padding, for the Dock.
    const BYTES: &[u8] = include_bytes!("../../assets/logo/baud-mark-amber.svg");
    macos_liquid_glass::image_from_bytes(BYTES)
}

/// The same mark, cropped snug to its own bounds. In the padded 512 box the
/// glyph is a third of the height, so a titlebar-sized box drew a mark half
/// the size of a traffic light; this viewBox is tight to the strokes, so the
/// box the icon is given is the size the mark is drawn at.
fn icon_titlebar() -> Option<Retained<NSImage>> {
    const BYTES: &[u8] = include_bytes!("../../assets/logo/baud-mark-amber-tight.svg");
    macos_liquid_glass::image_from_bytes(BYTES)
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("run() on the main thread");
    let size = NSSize::new(960.0, 600.0);
    // What the grid needs before a window exists: the display's scale, and
    // the titlebar's height -- the window is FullSizeContentView, so its
    // content spans the frame behind the transparent titlebar and the
    // traffic lights, and the terminal leaves that strip so row 0 clears
    // the lights. Both are class-level answers, so the child shell can be
    // spawned at its real grid before AppKit starts, and its own startup
    // overlaps AppKit's rather than following it.
    let early_scale = crate::pty::display_scale();
    let inset = {
        let content = NSRect::new(NSPoint::new(0.0, 0.0), size);
        let frame =
            NSWindow::frameRectForContentRect_styleMask(content, NSWindowStyleMask::Titled, mtm);
        (frame.size.height - size.height).max(0.0)
    };
    startup_mark("screen");
    let scale = match early_scale {
        Some(s) => s,
        None => {
            // No display CoreGraphics will name: AppKit's answer, at
            // AppKit's price.
            NSApplication::sharedApplication(mtm);
            NSScreen::mainScreen(mtm).map_or(1.0, |s| s.backingScaleFactor())
        }
    };
    let boot = Boot::prepare(scale);
    // The gutter in points: the grid floats inset from all four sides -- below
    // the titlebar and in from the left, right and bottom -- with the glass in
    // the margin. The pty and the renderer follow the inset area.
    let padding = boot.padding;
    // `BAUD_GRID=COLSxROWS` runs at a grid rather than at the window's
    // default, so a benchmark runs every terminal at one geometry. The pty
    // is *spawned* at that grid: sized afterwards, the child sometimes read
    // the default before the window's resize landed -- one row in fifteen.
    let grid = std::env::var("BAUD_GRID").ok().and_then(|g| {
        let (c, r) = g.split_once('x')?;
        Some(WinSize {
            ws_col: c.parse().ok()?,
            ws_row: r.parse().ok()?,
        })
    });
    let (cw, ch) = boot.metrics_cell();
    let ws = grid.as_ref().map_or_else(
        || {
            grid_for(
                NSSize::new(
                    (size.width - 2.0 * padding).max(0.0),
                    (size.height - inset - 2.0 * padding).max(0.0),
                ),
                scale,
                (cw, ch),
            )
        },
        |ws| WinSize {
            ws_col: ws.ws_col,
            ws_row: ws.ws_row,
        },
    );
    // The reader wakes the main thread through the view, which does not
    // exist yet; bytes that arrive before it does wait in the channel and
    // the first wake after the window is shown drains them.
    let late: Arc<OnceLock<Waker>> = Arc::new(OnceLock::new());
    let wake_late = Arc::clone(&late);
    let pty = Pty::spawn(&ws, move || {
        if let Some(w) = wake_late.get() {
            w.wake();
        }
    })
    .expect("pty");
    startup_mark("pty");
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    startup_mark("nsapp");
    let icon = icon();
    startup_mark("icon");
    let window = GlassWindow::new(mtm, size, "baud");
    startup_mark("glass window");
    // The inset predicted above, checked against the window that exists
    // now; the window's own value is what the layout uses.
    let measured = {
        let w = window.ns_window();
        let content_h = w
            .contentView()
            .map_or(size.height, |v| v.frame().size.height);
        (content_h - w.contentLayoutRect().size.height).max(0.0)
    };
    if (measured - inset).abs() > 0.5 {
        startup_note(&format!(
            "titlebar inset: predicted {inset:.1}, measured {measured:.1}"
        ));
    }
    let inset = measured;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let glass = GlassSurface::new(mtm, frame, GlassStyle::Regular, 16.0).expect("glass");
    let view = DrawableView::new(mtm, frame);
    startup_mark("window");
    let _ = late.set(view.waker());
    if grid.is_some() {
        // The window sized to the grid in points, from the cell metrics at
        // this scale, before it is shown; its first resize callback then
        // computes the same grid and changes nothing. The grid sits below
        // the titlebar inset, so the window is that much taller than the
        // grid; otherwise the inset would steal a row from the fixed
        // geometry a benchmark asked for.
        let wanted = NSSize::new(
            f64::from(ws.ws_col) * cw / scale + 2.0 * padding,
            f64::from(ws.ws_row) * ch / scale + inset + 2.0 * padding,
        );
        window.ns_window().setContentSize(wanted);
    }
    let probe = App::new(
        ws,
        if grid.is_some() {
            Sizing::Fixed
        } else {
            Sizing::Window
        },
        scale,
        view.waker(),
        pty,
        boot,
    );
    // A transparent container fills the glass; the terminal view sits inside it
    // inset below the titlebar, so the glass still fills the strip the traffic
    // lights float over while row 0 clears them. Autoresizing keeps the inset
    // fixed as the window grows.
    let full = frame.size;
    let container = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), full),
    );
    container.setAutoresizesSubviews(true);
    view.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(full.width, (full.height - inset).max(0.0)),
    ));
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    container.addSubview(&view);
    // The icon in the titlebar strip, just right of the traffic lights and
    // centred in the inset. Pinned to the top-left so it stays as the window
    // grows; the glass shows through around it.
    // The icon in the titlebar strip is placed after `show()`, below,
    // once the traffic-light buttons have been laid out.
    // The container is the window's content until the first tick, when
    // it moves under the glass; see `LateGlass`.
    window.set_content_view(&container);
    startup_mark("views");
    window.set_accepts_mouse_moved(true);
    window.set_min_size(NSSize::new(200.0, 100.0));
    window.center();
    startup_mark("centered");
    window.show();
    startup_mark("shown");
    if let Some(img) = &icon {
        // Placed against the real traffic lights rather than a hand-picked x.
        // The window is FullSizeContentView, so `container` (the content view)
        // spans the whole frame and shares the theme frame's bottom-left
        // origin -- so a standard button's `frame` is already in container
        // coordinates, no conversion (which would be `unsafe` here) needed.
        let w = window.ns_window();
        let lights = (
            w.standardWindowButton(NSWindowButton::CloseButton),
            w.standardWindowButton(NSWindowButton::MiniaturizeButton),
            w.standardWindowButton(NSWindowButton::ZoomButton),
        );
        // Read after `show()`, so the buttons are laid out; a zero-height
        // frame means they are not, and the fallback places it by the strip.
        let measured = match lights {
            (Some(close), Some(mini), Some(zoom)) => {
                let (c, m, z) = (close.frame(), mini.frame(), zoom.frame());
                (z.size.height >= 1.0).then(|| {
                    // Horizontal comes from the buttons: x is shared with the
                    // container (the titlebar view spans the width from x=0).
                    // Vertical does NOT -- a button's `frame` is in the
                    // titlebar view's own coordinates, so its y is near that
                    // view's bottom, not the window's top. Convert would be
                    // `unsafe`, forbidden here, so the icon is centred in the
                    // strip from geometry we already know: it runs from
                    // `full.height - inset` to `full.height`.
                    let gap = (m.origin.x - (c.origin.x + c.size.width)).max(0.0);
                    // A peer of the lights: nearly the full strip height, so
                    // the snug mark reads at a traffic light's size or larger.
                    let s = (z.size.height * 1.8).clamp(18.0, inset - 2.0);
                    (s, z.origin.x + z.size.width + gap)
                })
            }
            _ => None,
        };
        let (s, left) = measured.unwrap_or_else(|| {
            let s = (inset - 2.0).clamp(18.0, 28.0);
            (s, 74.0)
        });
        // Centred in the titlebar strip, whose top is the window top.
        let cy = full.height - inset / 2.0;
        // The snug crop, not the padded dock mark, so the box is the size.
        let iv = match icon_titlebar() {
            Some(tight) => macos_liquid_glass::icon_view(mtm, &tight),
            None => macos_liquid_glass::icon_view(mtm, img),
        };
        iv.setFrame(NSRect::new(
            NSPoint::new(left, cy - s / 2.0),
            NSSize::new(s, s),
        ));
        iv.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewMaxXMargin,
        );
        container.addSubview(&iv);
    }
    window.make_first_responder(&view);
    let mut driver = Driver {
        app: probe,
        view: view.clone(),
        dock_icon: icon.clone(),
        held: None,
        bar_drag: None,
    };
    // The first frame now, on this thread, before the run loop is entered:
    // the geometry the view's first resize callback would report, then one
    // draw. Its completion wakes the responder through the run loop, which
    // is where the surface reaches the layer.
    Responder::resized(&mut driver, view.frame().size, scale);
    let v = driver.view.clone();
    driver.app.present(&v, Show::Now);
    startup_mark("first frame");
    // The glass under the terminal now that the first frame is on the
    // layer: ordering a window whose content is a glass effect view costs
    // 10-20 ms more than ordering one whose content is a plain view, so
    // the window went up bare and the material lands here, before the run
    // loop -- and before a responder exists, because moving the container
    // re-enters the view's resize callback, which a responder callback
    // holding the view's borrow cannot survive.
    window.set_content_view(glass.view());
    glass.set_content_view(&container);
    startup_mark("glass installed");
    // The menu after the first frame, before the run loop: key equivalents
    // need it from the first event, and nothing on screen waits for it.
    macos_liquid_glass::menu::install_main_menu(&ns_app, mtm, "baud");
    startup_mark("menu");
    view.set_responder(Box::new(driver));
    view.start_display_link();
    view.waker().wake();
    ns_app.activate();
    ns_app.run();
}

/// The font sizes a zoom may reach, in points: below six nothing reads,
/// and past seventy-two a cell is most of a small window.
const ZOOM_MIN: f32 = 6.0;
const ZOOM_MAX: f32 = 72.0;

/// Lines a single wheel event may turn into reports or keys: a trackpad
/// fling accumulates to many, and an application wants a bounded burst.
const WHEEL_MAX: u64 = 64;

impl Driver {
    /// Typing, and a paste, snap the window back to the live screen, where
    /// the keystroke's effect is.
    fn snap_to_live(&mut self) {
        if self.app.back.index() != 0 {
            self.app.back = Rows::ZERO;
            self.app.scroll_accum = 0.0;
            self.app.pending = Pending::Dirty;
        }
    }

    /// A press, drag or release as a selection: one click a cell, two a
    /// word, three a line, a drag extending by the press's grain, Shift
    /// moving the head and keeping the anchor, and a click that never
    /// moved clearing what was there.
    fn select(&mut self, event: &Mouse) {
        let (row, col) = self.cell_under(event.x, event.y);
        let scrolled = self.app.scrolled;
        let back = self.app.back;
        let view = self.app.term.view(back);
        let top = scrolled.top(back);
        let line = scrolled.line(back, row);
        let span_at = |grain: Grain| match grain {
            Grain::Cell => Span::cell(view.row(row), line, col),
            Grain::Word => Span::word(view.row(row), line, col),
            Grain::Line => Span::line(&view, top, row),
        };
        match event.kind {
            ViewMouseKind::Down if event.button == ViewButton::Left => {
                let grain = match event.clicks {
                    0 | 1 => Grain::Cell,
                    2 => Grain::Word,
                    _ => Grain::Line,
                };
                self.app.selection = match self.app.selection {
                    Some(mut sel) if event.modifiers.shift => {
                        sel.extend(span_at(sel.grain()));
                        Some(sel)
                    }
                    _ => Some(Selection::at(span_at(grain), grain)),
                };
                self.held = Some(MouseButton::Left);
            }
            ViewMouseKind::Dragged if self.held == Some(MouseButton::Left) => {
                if let Some(sel) = self.app.selection.as_mut() {
                    sel.extend(span_at(sel.grain()));
                }
            }
            ViewMouseKind::Up => {
                self.held = None;
                if self.app.selection.is_some_and(|s| s.is_click()) {
                    self.app.selection = None;
                }
            }
            _ => return,
        }
        self.app.pending = Pending::Dirty;
        self.pump(Trigger::Wake);
    }

    /// The scrollbar's strip, while the thumb is visible: a press on the
    /// thumb starts a drag, a press off it in the strip jumps the thumb to
    /// the pointer, and the window follows through the inverse geometry.
    fn scrollbar(&mut self, event: &Mouse) -> Took {
        let scale = self.app.scale;
        let at = |v: f64| Px::of(u32::try_from((v * scale).max(0.0) as i64).unwrap_or(0));
        // The grid is centred in the surface; the thumb's geometry is in
        // grid pixels, so the pointer comes in from view pixels less the
        // letterbox margin.
        let (px, py) = self.app.pad_px;
        let x = Px::of(at(event.x).value().saturating_sub(px.value()));
        let y = Px::of(at(event.y).value().saturating_sub(py.value()));
        let Some(t) = self.app.renderer.thumb() else {
            self.bar_drag = None;
            return Took::No;
        };
        let win = self.app.window();
        let size = self.app.renderer.pixel_size();
        let cell_h = self.app.renderer.metrics().cell_h;
        let top = match event.kind {
            ViewMouseKind::Down if event.button == ViewButton::Left && t.in_strip(x) => {
                if t.covers_y(y) {
                    self.bar_drag = Some(BarDrag {
                        grab_y: y,
                        top: t.top,
                    });
                    return Took::Yes;
                }
                Px::of(y.value().saturating_sub(t.height.value() / 2))
            }
            ViewMouseKind::Dragged => {
                let Some(d) = self.bar_drag else {
                    return Took::No;
                };
                let moved = i64::from(y.value()) - i64::from(d.grab_y.value());
                Px::of(u32::try_from((i64::from(d.top.value()) + moved).max(0)).unwrap_or(0))
            }
            ViewMouseKind::Up if self.bar_drag.is_some() => {
                self.bar_drag = None;
                return Took::Yes;
            }
            _ => return Took::No,
        };
        let back = back_for(win, size, cell_h, top);
        if back.index() != self.app.back.index() {
            self.app.back = back;
            self.app.pending = Pending::Dirty;
            self.pump(Trigger::Wake);
        }
        Took::Yes
    }

    /// The grid for a view of `size` points at `scale`, and the terminal,
    /// the child and the frame following it -- on a resize, and on a zoom.
    fn relayout(&mut self, size: NSSize, scale: CGFloat) {
        let (cw, ch) = self.app.metrics_cell();
        let px_w = size.width * scale;
        let px_h = size.height * scale;
        // The surface is the whole view; the grid is centred in it, so it
        // is computed from the view less a gutter on each side and the
        // presenter clears the margin the centring leaves to the terminal
        // background -- a gap, not a see-through edge or a border.
        self.app.view_px = (
            Px::of(u32::try_from(px_w.max(0.0) as i64).unwrap_or(0)),
            Px::of(u32::try_from(px_h.max(0.0) as i64).unwrap_or(0)),
        );
        if self.app.sizing == Sizing::Fixed {
            return;
        }
        let gutter = 2.0 * self.app.padding * scale;
        let cols =
            (((px_w - gutter).max(cw) / cw).floor() as i64).clamp(2, i64::from(MAX_GRID.ws_col));
        let rows =
            (((px_h - gutter).max(ch) / ch).floor() as i64).clamp(1, i64::from(MAX_GRID.ws_row));
        let ws = WinSize {
            ws_col: u16::try_from(cols).unwrap_or(2),
            ws_row: u16::try_from(rows).unwrap_or(1),
        };
        if (ws.ws_col, ws.ws_row) != (self.app.grid.ws_col, self.app.grid.ws_row) {
            let before = self.app.term.rotation();
            // Truncated only past MAX_GRID, which the clamp above prevents.
            let _ = self.app.term.resize(&ws);
            // Anchor the view to the content, not the grid. A shorter screen
            // keeps the cursor's row and retires the top into history (xterm's
            // rule); left alone the first line would scroll away on every drag
            // step. So the window moves up the ring by exactly what the resize
            // retired -- the same pin `pump` applies while output scrolls --
            // and the first visible line stays the first visible line. A
            // taller screen pulls rows back and the window follows down toward
            // the live screen; typing snaps all the way back to it, where the
            // cursor is. `scrolled` is left untouched: it numbers lines for a
            // selection, and a resize has already cleared the selection.
            let ring = self.app.term.ring().index();
            let fwd = (self.app.term.rotation().index() + ring - before.index()) % ring;
            let advanced = if fwd * 2 <= ring {
                i64::try_from(fwd).unwrap_or(0)
            } else {
                i64::try_from(fwd).unwrap_or(0) - i64::try_from(ring).unwrap_or(0)
            };
            let retained = i64::try_from(self.app.term.scrollback().len()).unwrap_or(0);
            let nb =
                (i64::try_from(self.app.back.index()).unwrap_or(0) + advanced).clamp(0, retained);
            self.app.back = Rows::of_count(usize::try_from(nb).unwrap_or(0));
            let px = |v: f64| Px::of(u32::try_from(v as i64).unwrap_or(u32::MAX));
            // The `SIGWINCH` is sent on the drag event itself, not deferred:
            // a full-screen program redraws into the new geometry as the
            // window moves, so it fills the rows the reflow opened rather
            // than leaving them blank until the drag stops -- the black gaps
            // a settle-delay left in a live resize.
            self.app.pty.resize(&ws, px(px_w), px(px_h));
            self.app.grid = ws;
            // A reflow re-cuts every line, so the columns a selection named
            // no longer name cells.
            self.app.selection = None;
            // A new grid is a new surface, so repaint every row of it rather
            // than trusting the last frame, whose rows were a different width.
            self.app.next_damage = Damage::Full;
            self.app.pending = Pending::Dirty;
            // Draw at the new size *now*, and synchronously. A live drag runs
            // a modal resize loop in which the display link does not tick and
            // the async draw's completion -- delivered through the main run
            // loop -- is never drained, so an async present issued here sees
            // the previous frame still in flight, returns `Drawn::Busy`, and
            // draws nothing: the layer keeps one stale surface stretched over
            // every new size for the whole drag, which is the freeze. A
            // synchronous draw waits for the GPU and sets the layer itself, so
            // each step shows a correctly sized frame. `term.resize` kept the
            // content -- the pitch is fixed, each row clipped or extended in
            // place -- and the child redraws into it on `SIGWINCH`.
            self.app.present(&self.view, Show::Now);
        }
    }

    /// The View menu's zoom: a new font size in points, bounded, the
    /// renderer rescaled to it at the current backing scale, the grid
    /// re-laid for the same window, and a frame drawn now. The size is not
    /// written back to the config; Actual Size returns to what it said.
    fn zoom(&mut self, points: f32) {
        let points = points.clamp(ZOOM_MIN, ZOOM_MAX);
        if points == self.app.font_size {
            return;
        }
        let scale = self.app.scale;
        let Some(px) = PxSize::of(points * scale as f32) else {
            return;
        };
        if self.app.renderer.rescale(px).is_none() {
            return;
        }
        self.app.font_size = points;
        self.relayout(self.view.frame().size, scale);
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        self.app.present(&self.view, Show::Event);
    }

    /// Cmd-C: the selection's text to the pasteboard, if there is one.
    fn copy(&mut self) {
        let Some(sel) = self.app.selection else {
            return;
        };
        let text = sel.text(&self.app.term, self.app.scrolled);
        if !text.is_empty() {
            clipboard::write_text(&text);
        }
    }

    /// Cmd-V: the pasteboard's string to the child, bracketed when it
    /// asked (`?2004`), through the channel keys take.
    fn paste(&mut self) {
        let Some(text) = clipboard::read_text() else {
            return;
        };
        let bracket = if self.app.term.bracketed_paste() {
            Bracket::On
        } else {
            Bracket::Off
        };
        self.snap_to_live();
        self.app.pty.write(&paste_bytes(&text, bracket));
    }

    /// The cell under a point in the view, clamped to the grid: the view
    /// is the terminal's area below the titlebar inset, in points, and the
    /// grid is in pixels of the cell size.
    fn cell_under(&self, x: f64, y: f64) -> (Row, Col) {
        cell_under(
            (x, y),
            self.app.scale,
            self.app.metrics_cell(),
            (self.app.grid.ws_col, self.app.grid.ws_row),
            (self.app.pad_px.0.value(), self.app.pad_px.1.value()),
        )
    }
}

/// Whether the child asked for bracketed paste.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bracket {
    On,
    Off,
}

/// The bytes a paste sends: bracketed as `?2004` specifies, with a forged
/// end inside the text removed, because the child would take it as the
/// end; or, unbracketed, with line endings as typed input carries them.
fn paste_bytes(text: &str, bracket: Bracket) -> Vec<u8> {
    match bracket {
        Bracket::On => {
            let body = text.replace("\x1b[201~", "");
            [b"\x1b[200~".as_slice(), body.as_bytes(), b"\x1b[201~"].concat()
        }
        Bracket::Off => text.replace("\r\n", "\r").replace('\n', "\r").into_bytes(),
    }
}

/// `cell_under` as a pure function of the point, the scale, the cell size
/// in pixels and the grid, so a test can pin the arithmetic.
fn cell_under(
    at: (f64, f64),
    scale: CGFloat,
    cell: (f64, f64),
    grid: (u16, u16),
    pad: (u32, u32),
) -> (Row, Col) {
    let col = (((at.0 * scale - f64::from(pad.0)) / cell.0).floor() as i64)
        .clamp(0, i64::from(grid.0).saturating_sub(1).max(0));
    let row = (((at.1 * scale - f64::from(pad.1)) / cell.1).floor() as i64)
        .clamp(0, i64::from(grid.1).saturating_sub(1).max(0));
    (
        Row::from_index(usize::try_from(row).unwrap_or(0)),
        Col::at_index(usize::try_from(col).unwrap_or(0)),
    )
}

/// The modifier bits a mouse report carries, from the view's modifiers:
/// Option is Meta, the way xterm reads the key.
fn mouse_mods(m: macos_liquid_glass::drawable::Modifiers) -> MouseMods {
    let mut out = MouseMods::NONE;
    if m.shift {
        out = out.with(MouseMods::SHIFT);
    }
    if m.control {
        out = out.with(MouseMods::CONTROL);
    }
    if m.option {
        out = out.with(MouseMods::META);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paste_is_bracketed_when_asked_and_its_line_endings_are_typed_otherwise() {
        let forged = "a\x1b[201~b\r\nc\n";
        assert_eq!(
            paste_bytes(forged, Bracket::On),
            b"\x1b[200~ab\r\nc\n\x1b[201~".to_vec()
        );
        assert_eq!(
            paste_bytes(forged, Bracket::Off),
            b"a\x1b[201~b\rc\r".to_vec()
        );
    }

    #[test]
    fn a_point_names_the_cell_under_it_and_the_edges_clamp() {
        // 2x scale, 20x40 px cells, a 200x50 grid: point (105, 21) is
        // 210x42 px, cell (10, 1).
        let (r, c) = cell_under((105.0, 21.0), 2.0, (20.0, 40.0), (200, 50), (0, 0));
        assert_eq!((r.index(), c.index()), (1, 10));
        let (r, c) = cell_under((-3.0, -3.0), 2.0, (20.0, 40.0), (200, 50), (0, 0));
        assert_eq!((r.index(), c.index()), (0, 0));
        let (r, c) = cell_under((9999.0, 9999.0), 2.0, (20.0, 40.0), (200, 50), (0, 0));
        assert_eq!((r.index(), c.index()), (49, 199));
    }
}

/// The grid a view of `size` points holds at `scale`, from the cell size
/// in pixels: the arithmetic of `Responder::resized`, for the spawn that
/// happens before the view exists.
fn grid_for(size: NSSize, scale: CGFloat, cell: (f64, f64)) -> WinSize {
    let cols = ((size.width * scale / cell.0).floor() as i64).clamp(2, i64::from(MAX_GRID.ws_col));
    let rows = ((size.height * scale / cell.1).floor() as i64).clamp(1, i64::from(MAX_GRID.ws_row));
    WinSize {
        ws_col: u16::try_from(cols).unwrap_or(2),
        ws_row: u16::try_from(rows).unwrap_or(1),
    }
}
