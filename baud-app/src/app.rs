//! The window, the responder, and the frame.
#![forbid(unsafe_code)]

use crate::host::{
    FIRST_COMMIT, FIRST_DONE, FIRST_PRESENT, FIRST_TICK, MAX_GRID, describe, host_geometry,
    load_faces, startup_first, startup_mark,
};
use crate::pty::{Pty, Stream};
use baud_core::{Depth, Owned, Term, grid_stride};
use baud_domain::{
    BytePos, Codepoint, Cols, FnKey, KeyCode, KeyEvent, Modifiers, ReadLen, Rows, WinSize,
};
use baud_font::Px;
use baud_present_macos::{
    CellInput, Drawn, Gpu, GpuFrame, GridBuffer, InFlight, MapBuffer, Mode, Presented, Presenter,
    StylesBuffer,
};
use baud_render::{
    Blend, Blink, Blinking, Damage, Frame, GlyphMap, PxSize, Renderer, Resolved, Stack, Tally,
    Target, Theme,
};
use macos_liquid_glass::drawable::{
    DrawableView, Focus, Handled, KeyPress, Mouse, Rect, Responder, Scroll, Waker,
};
use macos_liquid_glass::glass::{GlassStyle, GlassSurface};
use macos_liquid_glass::objc2::MainThreadOnly;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSImage, NSView,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use macos_liquid_glass::window::GlassWindow;
use std::time::{Duration, Instant};

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
            eprintln!("BAUD_GPU={v:?} is not 0, 1 or cells; choosing automatically");
            Request::Auto
        }
    }
}

/// The cell path's memory, all of it where the GPU reads it: the grid the
/// core writes, the styles the renderer resolves, the map the walk fills.
struct CellMemory {
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
    /// Wheel travel not yet a whole line, in points.
    scroll_accum: f64,
    frames: Tally,
    last_report: std::time::Instant,
    stats: Frame,
    gpu_stats: GpuFrame,
    wanted: Path,
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

impl App {
    fn new(ws: WinSize, sizing: Sizing, scale: CGFloat, waker: Waker) -> App {
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
        eprintln!(
            "cache: {}; grid pitch {} cells, {} B a row",
            describe(&geom),
            pitch.index(),
            pitch.bytes()
        );
        let probe = match request {
            Request::Force(Path::Cpu) => None,
            _ => Gpu::new(),
        };
        startup_mark("gpu probed");
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
                        eprintln!("no shared grid; drawing the instances");
                        (Path::Instances, Some(gpu), own_grid(&ws), None)
                    }
                }
            }
            (Request::Force(Path::Instances), Some(gpu)) => {
                (Path::Instances, Some(gpu), own_grid(&ws), None)
            }
            (Request::Force(Path::Cells | Path::Instances) | Request::Auto, None) => {
                eprintln!("no Metal device or shaders; drawing on the CPU");
                (Path::Cpu, None, own_grid(&ws), None)
            }
            (Request::Force(Path::Cpu), _) => (Path::Cpu, None, own_grid(&ws), None),
        };
        let cfg = crate::config::Config::load();
        startup_mark("config");
        match &cfg.source {
            Some(p) => eprintln!("config: {}", p.display()),
            None => eprintln!("config: none found; the built-in stack"),
        }
        for f in &cfg.faces {
            eprintln!("face: {}", f.path);
        }
        let faces = load_faces(&cfg);
        startup_mark("faces");
        let mut theme = Theme::dark();
        theme.bg_alpha = baud_domain::Chan::from_u8(cfg.background_alpha);
        let renderer = Renderer::new(
            Stack::new(faces).expect("faces"),
            PxSize::of(cfg.font_size * scale as f32).expect("size"),
            theme,
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .expect("cell metrics");
        startup_mark("renderer");
        let reader_waker = waker;
        let pty = Pty::spawn(&ws, move || reader_waker.wake()).expect("pty");
        startup_mark("pty");
        let (frame_w, frame_h) = renderer.pixel_size();
        let cell = match (&gpu, grid) {
            (Some(gpu), Some((grid, alt))) => match (gpu.styles(&term), gpu.map()) {
                (Some((styles, styles_mem)), Some((map_buffer, entries))) => Some(CellMemory {
                    grid,
                    alt,
                    styles,
                    styles_mem,
                    map_buffer,
                    map: GlyphMap::over(entries),
                }),
                _ => {
                    eprintln!("no shared styles or map; drawing the instances");
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
        eprintln!(
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
            scale,
            grid: ws,
            sizing,
            pending: Pending::Dirty,
            back: Rows::ZERO,
            scroll_accum: 0.0,
            frames: Tally::default(),
            last_report: std::time::Instant::now(),
            stats: Frame::default(),
            gpu_stats: GpuFrame::default(),
            wanted,
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

    fn flush_reply(&mut self) {
        let reply = self.term.reply().to_vec();
        if !reply.is_empty() {
            self.pty.write(&reply);
            self.term.clear_reply();
        }
    }

    fn present(&mut self, view: &DrawableView) {
        // The surface is the renderer's size, so the renderer follows the
        // terminal first; the cell path never converts and would otherwise
        // keep the grid it started with.
        self.renderer.fit(&self.term);
        let (w, h) = self.renderer.pixel_size();
        if (w, h) != (self.frame_w, self.frame_h) || self.presenter.is_none() {
            self.frame_w = w;
            self.frame_h = h;
            self.frame = vec![0; slots(w) * slots(h)];
            self.presenter = match (self.wanted, self.gpu.take(), self.presenter.take()) {
                // A presenter that exists keeps its GPU across a resize.
                (Path::Instances | Path::Cells, _, Some(mut p)) if p.mode() == Mode::Gpu => {
                    p.resize(w, h).map(|()| p)
                }
                (Path::Instances | Path::Cells, Some(gpu), _) => Presenter::over(gpu, w, h),
                // The device was taken by a presenter that then declined
                // its targets; probe once more, and say so if that fails.
                (Path::Instances | Path::Cells, None, _) => {
                    Presenter::with_gpu(w, h).or_else(|| {
                        eprintln!("no Metal device, shaders or targets; drawing on the CPU");
                        Presenter::new(w, h)
                    })
                }
                (Path::Cpu, _, _) => Presenter::new(w, h),
            };
            if let Some(p) = &self.presenter {
                eprintln!("presenting through {:?}, memory {:?}", p.mode(), p.memory());
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
                cell.map.ensure(view, &mut self.renderer);
                let cursor = (self.term.cursor_visible() && self.back.index() == 0).then(|| {
                    let (r, c) = self.term.cursor();
                    (r, c, self.term.cursor_style())
                });
                self.renderer
                    .resolve_styles_into(&self.term, cell.styles_mem);
                self.stats = Frame {
                    convert: t0.elapsed(),
                    ..Frame::default()
                };
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
                };
                if let Some(p) = &mut self.presenter {
                    match p.draw_cells(waker, &mut self.renderer, &input) {
                        Some(Drawn::Frame(g)) => {
                            startup_first("first commit", &FIRST_COMMIT);
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
                    match p.draw(waker, &mut self.renderer) {
                        Some(Drawn::Frame(g)) => {
                            startup_first("first commit", &FIRST_COMMIT);
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
                if let Some(p) = &mut self.presenter {
                    if p.present(view, &self.frame, w) == Presented::Yes {
                        startup_first("first present", &FIRST_PRESENT);
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
                Some(Mode::Gpu) => eprintln!(
                    "{} frames; last: {} rows, {} instances, convert {:?}, bind {:?}, commit {:?}, gpu {:?}",
                    self.frames.value(),
                    self.stats.rows_dirty.value(),
                    self.gpu_stats.instances.value(),
                    self.stats.convert,
                    self.gpu_stats.upload,
                    self.gpu_stats.submit,
                    self.gpu_stats.gpu
                ),
                _ => eprintln!(
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
        if self.app.back.index() > 0 {
            let ring = self.app.term.ring().index();
            let advanced = (self.app.term.rotation().index() + ring - before.index()) % ring;
            let retained = self.app.term.scrollback().len();
            self.app.back = Rows::of_count((self.app.back.index() + advanced).min(retained));
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
            self.app.present(&view);
        }
    }
}

impl Responder for Driver {
    fn key(&mut self, key: &KeyPress) -> Handled {
        // Typing snaps the window back to the live screen, where the
        // keystroke's effect is.
        if self.app.back.index() != 0 {
            self.app.back = Rows::ZERO;
            self.app.scroll_accum = 0.0;
            self.app.pending = Pending::Dirty;
        }
        let m = mods(key.modifiers);
        if key.modifiers.command {
            // Menu territory; nothing to send.
            return Handled::Consumed;
        }
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

    fn mouse(&mut self, _event: &Mouse) {}

    /// The wheel moves the window over the history. A trackpad sends
    /// points and a wheel sends lines; both accumulate to whole rows of the
    /// cell height, positive `dy` being "show earlier lines" the way AppKit
    /// delivers it. The alternate screen keeps no history and gets nothing
    /// here; a pager or an editor on it will want the wheel as keys, which
    /// is a separate protocol.
    fn scroll(&mut self, event: &Scroll) {
        if self.app.term.screen() != baud_core::Screen::Normal {
            return;
        }
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
        let (cw, ch) = self.app.metrics_cell();
        if scale != self.app.scale {
            // A different scale is a different pixel size for the font;
            // the renderer is rebuilt with it.
            self.app.scale = scale;
        }
        if self.app.sizing == Sizing::Fixed {
            return;
        }
        let px_w = size.width * scale;
        let px_h = size.height * scale;
        let cols = ((px_w / cw).floor() as i64).clamp(2, i64::from(MAX_GRID.ws_col));
        let rows = ((px_h / ch).floor() as i64).clamp(1, i64::from(MAX_GRID.ws_row));
        let ws = WinSize {
            ws_col: u16::try_from(cols).unwrap_or(2),
            ws_row: u16::try_from(rows).unwrap_or(1),
        };
        if (ws.ws_col, ws.ws_row) != (self.app.grid.ws_col, self.app.grid.ws_row) {
            // Truncated only past MAX_GRID, which the clamp above prevents.
            let _ = self.app.term.resize(&ws);
            let px = |v: f64| Px::of(u32::try_from(v as i64).unwrap_or(u32::MAX));
            self.app.pty.resize(&ws, px(px_w), px(px_h));
            self.app.grid = ws;
            // A new grid is a new surface, so repaint every row of it rather
            // than trusting the last frame, whose rows were a different width.
            self.app.next_damage = Damage::Full;
            self.app.pending = Pending::Dirty;
            // Draw at the new size *now*. A live drag runs a modal resize loop
            // in which the display link does not tick, so without a present
            // here the layer stretches the last surface and the window looks
            // corrupted until the drag ends. `term.resize` kept the content --
            // the pitch is fixed, so each row was clipped or extended in place
            // -- so this paints what is there at the new geometry, and the
            // child redraws into it on `SIGWINCH`.
            self.app.present(&self.view);
        }
    }

    fn frame(&mut self, target: f64) {
        startup_first("first tick", &FIRST_TICK);
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
/// bundle: the dock icon and the titlebar mark both read from it.
fn icon() -> Option<Retained<NSImage>> {
    // The bare mark on a transparent ground -- just the glyph, no square behind
    // it -- for both the dock and the titlebar. The SVG itself, not a raster,
    // so NSImage renders it as a vector with real transparency.
    const BYTES: &[u8] = include_bytes!("../../assets/logo/baud-mark-amber.svg");
    macos_liquid_glass::image_from_bytes(BYTES)
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("run() on the main thread");
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    startup_mark("nsapp");
    let icon = icon();
    startup_mark("icon");
    if let Some(img) = &icon {
        macos_liquid_glass::set_application_icon(&ns_app, img);
    }
    let size = NSSize::new(960.0, 600.0);
    let window = GlassWindow::new(mtm, size, "baud");
    startup_mark("glass window");
    let scale = window.ns_window().backingScaleFactor();
    // The window is FullSizeContentView, so its content spans the frame behind
    // the transparent titlebar and the traffic lights. `contentLayoutRect` is
    // the area below the titlebar, so the difference is the titlebar height in
    // points -- the top inset the terminal must leave so row 0 clears the
    // lights. Constant for the life of the window.
    let inset = {
        let w = window.ns_window();
        let content_h = w
            .contentView()
            .map_or(size.height, |v| v.frame().size.height);
        (content_h - w.contentLayoutRect().size.height).max(0.0)
    };
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let glass = GlassSurface::new(mtm, frame, GlassStyle::Regular, 16.0).expect("glass");
    let view = DrawableView::new(mtm, frame);
    startup_mark("window");
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
    // Otherwise the initial grid is a placeholder the view's first resize
    // callback settles from the window's points and the cell size.
    let probe = App::new(
        grid.as_ref().map_or(
            WinSize {
                ws_col: 80,
                ws_row: 24,
            },
            |ws| WinSize {
                ws_col: ws.ws_col,
                ws_row: ws.ws_row,
            },
        ),
        if grid.is_some() {
            Sizing::Fixed
        } else {
            Sizing::Window
        },
        scale,
        view.waker(),
    );
    if let Some(ws) = grid {
        // The window sized to the grid in points, from the cell metrics at
        // this scale, before it is shown; its first resize callback then
        // computes the same grid and changes nothing.
        let (cw, ch) = probe.metrics_cell();
        // The grid sits below the titlebar inset, so the window is that much
        // taller than the grid; otherwise the inset would steal a row from the
        // fixed geometry a benchmark asked for.
        let wanted = NSSize::new(
            f64::from(ws.ws_col) * cw / scale,
            f64::from(ws.ws_row) * ch / scale + inset,
        );
        window.ns_window().setContentSize(wanted);
    }
    let driver = Driver {
        app: probe,
        view: view.clone(),
    };
    view.set_responder(Box::new(driver));
    window.set_content_view(glass.view());
    // A transparent container fills the glass; the terminal view sits inside it
    // inset below the titlebar, so the glass still fills the strip the traffic
    // lights float over while row 0 clears them. Autoresizing keeps the inset
    // fixed as the window grows.
    let full = glass.view().frame().size;
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
    if let Some(img) = &icon {
        let s = (inset * 0.62).max(14.0);
        let iv = macos_liquid_glass::icon_view(mtm, img);
        iv.setFrame(NSRect::new(
            NSPoint::new(78.0, full.height - inset + (inset - s) / 2.0),
            NSSize::new(s, s),
        ));
        iv.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewMaxXMargin,
        );
        container.addSubview(&iv);
    }
    glass.set_content_view(&container);
    window.set_accepts_mouse_moved(true);
    window.set_min_size(NSSize::new(200.0, 100.0));
    window.center();
    window.show();
    startup_mark("shown");
    window.make_first_responder(&view);
    view.start_display_link();
    ns_app.activate();
    ns_app.run();
}
