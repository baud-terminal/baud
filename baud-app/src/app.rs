//! The window, the responder, and the frame.
#![forbid(unsafe_code)]

use crate::pty::{Pty, Stream};
use baud_core::{Owned, Term, grid_stride};
use baud_domain::{
    BytePos, Codepoint, Cols, FnKey, KeyCode, KeyEvent, Modifiers, ReadLen, Rows, WinSize,
};
use baud_domain::{CacheGeometry, CacheLevel};
use baud_font::{Face, FaceIndex, Px};
use baud_present_macos::{
    CellInput, Drawn, Gpu, GpuFrame, GridBuffer, InFlight, MapBuffer, Mode, Presenter, StylesBuffer,
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
/// The largest grid the storage is provisioned for.
const MAX_GRID: WinSize = WinSize {
    ws_col: 400,
    ws_row: 150,
};
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
/// **Unset is `Auto`: probe once and take the best path the machine
/// supports** -- Metal over the converted instances, else the CPU. The cell
/// path stays opt-in (`cells`), because it draws a smaller program: no
/// cross-cell ligature and no attached mark, counted by
/// `MapStats::dropped_marks` at 55 cells a frame on cat-cjk. That is a
/// fidelity trade a user makes, not one a probe makes for them. `0` or `cpu`
/// forces the CPU, for a comparison that needs it; a bench pins the path
/// explicitly so two arms never measure two tiers.
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

/// The cache hierarchy of this machine, from the per-arch probe: the grid's
/// row pitch is padded against its line by `store_stride`, so rows alias no
/// way and spread over every set. `UNKNOWN` where the probe cannot ask,
/// which still resolves to a safe pitch.
fn host_geometry() -> CacheGeometry {
    #[cfg(target_arch = "aarch64")]
    {
        <baud_isa_aarch64::Host as baud_isa::CacheProbe>::detect()
    }
    #[cfg(target_arch = "x86_64")]
    {
        <baud_isa_x86_64::Host as baud_isa::CacheProbe>::detect()
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        CacheGeometry::UNKNOWN
    }
}

/// One line for the log: what the probe found, level by level, and what
/// was assumed where it found nothing.
fn describe(geom: &CacheGeometry) -> String {
    let level = |l: Option<CacheLevel>| match l {
        Some(l) => match l.way() {
            Some(w) => format!(
                "{} KiB {}-way, {} KiB a way",
                l.size.bytes() / 1024,
                l.ways.count(),
                w.bytes() / 1024
            ),
            None => format!("{} KiB", l.size.bytes() / 1024),
        },
        None => "unknown".to_string(),
    };
    let line = match geom.line_reported() {
        Some(l) => format!("{} B", l.bytes()),
        None => format!("unknown, {} B assumed", geom.line().bytes()),
    };
    format!(
        "line {line}; L1d {}; L2 {}; L3 {}",
        level(geom.l1d()),
        level(geom.l2()),
        level(geom.l3())
    )
}

/// The cell path's memory, all of it where the GPU reads it: the grid the
/// core writes, the styles the renderer resolves, the map the walk fills.
struct CellMemory {
    grid: GridBuffer,
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
        let pitch = grid_stride(&geom);
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
        let own_grid = |ws: &WinSize| {
            let owned: &'static mut Owned =
                Box::leak(Box::new(Owned::with_geometry(&MAX_GRID, &geom)));
            Term::new(owned.storage(), ws)
        };
        let rows = Rows::of_index(usize::from(MAX_GRID.ws_row));
        let (mut wanted, gpu, term, grid) = match (request, probe) {
            (Request::Force(Path::Cells), Some(gpu)) => match gpu.grid(rows, pitch) {
                Some((grid, cells)) => {
                    let owned: &'static mut Owned =
                        Box::leak(Box::new(Owned::without_viewport(&MAX_GRID, &geom)));
                    let term = Term::new(owned.storage_over(cells), &ws);
                    (Path::Cells, Some(gpu), term, Some(grid))
                }
                None => {
                    eprintln!("BAUD_GPU=cells but no shared grid; drawing the instances");
                    (Path::Instances, Some(gpu), own_grid(&ws), None)
                }
            },
            (Request::Force(Path::Cells), None) => {
                eprintln!("BAUD_GPU=cells but no Metal device or shaders; drawing on the CPU");
                (Path::Cpu, None, own_grid(&ws), None)
            }
            (Request::Force(Path::Instances) | Request::Auto, Some(gpu)) => {
                (Path::Instances, Some(gpu), own_grid(&ws), None)
            }
            (Request::Force(Path::Instances), None) => {
                eprintln!("BAUD_GPU=1 but no Metal device or shaders; drawing on the CPU");
                (Path::Cpu, None, own_grid(&ws), None)
            }
            (Request::Auto, None) => {
                eprintln!("no Metal device or shaders; drawing on the CPU");
                (Path::Cpu, None, own_grid(&ws), None)
            }
            (Request::Force(Path::Cpu), _) => (Path::Cpu, None, own_grid(&ws), None),
        };
        let cfg = crate::config::Config::load();
        // Read and leak each face's bytes, then parse; the faces borrow them
        // for the life of the window. A face that will not read or parse is
        // fatal with its path, because a terminal with no font draws nothing.
        let data: Vec<&'static [u8]> = cfg
            .faces
            .iter()
            .map(|f| {
                let bytes =
                    std::fs::read(&f.path).unwrap_or_else(|e| panic!("font {}: {e}", f.path));
                &*Box::leak(bytes.into_boxed_slice())
            })
            .collect();
        let faces: &'static [Face<'static>] = Box::leak(
            data.iter()
                .zip(&cfg.faces)
                .map(|(d, f)| {
                    Face::parse_index(d, FaceIndex::of(f.index))
                        .unwrap_or_else(|_| panic!("font {}: not a face", f.path))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
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
        let reader_waker = waker;
        let pty = Pty::spawn(&ws, move || reader_waker.wake()).expect("pty");
        let (frame_w, frame_h) = renderer.pixel_size();
        let cell = match (&gpu, grid) {
            (Some(gpu), Some(grid)) => match (gpu.styles(&term), gpu.map()) {
                (Some((styles, styles_mem)), Some((map_buffer, entries))) => Some(CellMemory {
                    grid,
                    styles,
                    styles_mem,
                    map_buffer,
                    map: GlyphMap::over(entries),
                }),
                _ => {
                    eprintln!("BAUD_GPU=cells but no shared styles or map; drawing the instances");
                    // The terminal's viewport lives in this buffer for the
                    // life of the process, whichever path draws it.
                    std::mem::forget(grid);
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
                cell.map.ensure(self.term.cells(), &mut self.renderer);
                let cursor = self.term.cursor_visible().then(|| {
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
                    grid: &cell.grid,
                    styles: &cell.styles,
                    map: &cell.map_buffer,
                    stride: self.term.stride(),
                    rotation: self.term.rotation(),
                    cursor,
                };
                if let Some(p) = &mut self.presenter {
                    match p.draw_cells(waker, &mut self.renderer, &input) {
                        Some(Drawn::Frame(g)) => self.gpu_stats = g,
                        // Still in flight: the completion wakes us.
                        Some(Drawn::Busy) => return,
                        None => {}
                    }
                }
            }
            (Some(Mode::Gpu), None) => {
                let converted = self.renderer.convert(&mut self.term, damage);
                self.stats = Frame {
                    rows_dirty: converted.rows_dirty,
                    rows: converted.rows,
                    convert: converted.convert,
                    ..Frame::default()
                };
                if let Some(p) = &mut self.presenter {
                    match p.draw(waker, &mut self.renderer) {
                        Some(Drawn::Frame(g)) => self.gpu_stats = g,
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
                    p.present(view, &self.frame, w);
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
        for chunk in &drained.chunks {
            self.app.term.write_at(BytePos::ZERO, chunk);
            self.app
                .term
                .feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
            self.app.pending = Pending::Dirty;
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

    fn scroll(&mut self, _event: &Scroll) {}

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
    let icon = icon();
    if let Some(img) = &icon {
        macos_liquid_glass::set_application_icon(&ns_app, img);
    }
    let size = NSSize::new(960.0, 600.0);
    let window = GlassWindow::new(mtm, size, "baud");
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
    window.make_first_responder(&view);
    view.start_display_link();
    ns_app.activate();
    ns_app.run();
}
