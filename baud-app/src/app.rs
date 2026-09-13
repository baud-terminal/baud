//! The window, the responder, and the frame.
#![forbid(unsafe_code)]

use crate::pty::{Pty, Stream};
use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Codepoint, Cols, FnKey, KeyCode, KeyEvent, Modifiers, ReadLen, Rows, WinSize,
};
use baud_font::{Face, FaceIndex, Px};
use baud_present_macos::{
    CellInput, Drawn, Gpu, GpuFrame, GridBuffer, InFlight, MapBuffer, Mode, Presenter, StylesBuffer,
};
use baud_render::{
    Blend, Damage, Frame, GlyphMap, PxSize, Renderer, Resolved, Stack, Tally, Target, Theme,
};
use macos_liquid_glass::drawable::{
    DrawableView, Focus, Handled, KeyPress, Mouse, Rect, Responder, Scroll, Waker,
};
use macos_liquid_glass::glass::{GlassStyle, GlassSurface};
use macos_liquid_glass::objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
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

/// The font size in points; pixels are this times the backing scale.
const POINTS: f32 = 13.0;
/// The largest grid the storage is provisioned for.
const MAX_GRID: WinSize = WinSize {
    ws_col: 400,
    ws_row: 150,
};
/// The default background's alpha: below one so the glass shows through
/// (D14, default background only).
const BG_ALPHA: u8 = 0xD8;

/// Which path draws: the CPU compositor, Metal over the converted
/// instances when `BAUD_GPU=1` is in the environment, or Metal over the raw
/// grid when it is `BAUD_GPU=cells`. The CPU path is the default until the
/// `frame_gpu` bench says which is faster on which recording.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
    Cpu,
    Instances,
    Cells,
}

fn wanted() -> Path {
    match std::env::var_os("BAUD_GPU") {
        Some(v) if v == "1" => Path::Instances,
        Some(v) if v == "cells" => Path::Cells,
        _ => Path::Cpu,
    }
}

/// The faces, by path, in fallback order: the system's mono, its emoji,
/// and its symbols -- braille and the box and block drawing Menlo lacks,
/// which is what btop's graphs are made of.
const FACES: [(&str, u32); 3] = [
    ("/System/Library/Fonts/Menlo.ttc", 0),
    ("/System/Library/Fonts/Apple Color Emoji.ttc", 0),
    ("/System/Library/Fonts/Apple Symbols.ttf", 0),
];

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
}

impl App {
    fn new(ws: WinSize, scale: CGFloat, waker: Waker) -> App {
        let wanted = wanted();
        // The storage, the font files and the faces live as long as the
        // process: the renderer borrows them for the life of the window.
        // On the cell path the viewport is not the storage's own: it is
        // lent by the GPU, so the grid the core writes is the grid the
        // shader reads, in place.
        let (gpu, term, grid) = match wanted {
            Path::Cells => match Gpu::new() {
                Some(gpu) => {
                    let (grid, cells) = gpu
                        .grid(
                            Rows::of_index(usize::from(MAX_GRID.ws_row)),
                            Cols::from_stride(baud_core::MAX_COLS),
                        )
                        .expect("a shared grid");
                    let owned: &'static mut Owned =
                        Box::leak(Box::new(Owned::without_viewport(&MAX_GRID)));
                    let term = Term::new(owned.storage_over(cells), &ws);
                    (Some(gpu), term, Some(grid))
                }
                None => {
                    eprintln!("BAUD_GPU=cells but no Metal device; drawing on the CPU");
                    let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&MAX_GRID)));
                    (None, Term::new(owned.storage(), &ws), None)
                }
            },
            _ => {
                let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&MAX_GRID)));
                (None, Term::new(owned.storage(), &ws), None)
            }
        };
        let data: Vec<&'static [u8]> = FACES
            .iter()
            .map(|(path, _)| {
                let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
                &*Box::leak(bytes.into_boxed_slice())
            })
            .collect();
        let faces: &'static [Face<'static>] = Box::leak(
            data.iter()
                .zip(FACES)
                .map(|(d, (_, i))| Face::parse_index(d, FaceIndex::of(i)).expect("system face"))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let mut theme = Theme::dark();
        theme.bg_alpha = baud_domain::Chan::from_u8(BG_ALPHA);
        let renderer = Renderer::new(
            Stack::new(faces).expect("faces"),
            PxSize::of(POINTS * scale as f32).expect("size"),
            theme,
            Blend::Native,
            Cols::from_stride(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .expect("cell metrics");
        let reader_waker = waker;
        let pty = Pty::spawn(&ws, move || reader_waker.wake()).expect("pty");
        let (frame_w, frame_h) = renderer.pixel_size();
        let cell = match (&gpu, grid) {
            (Some(gpu), Some(grid)) => {
                let (styles, styles_mem) = gpu.styles(&term).expect("shared styles");
                let (map_buffer, entries) = gpu.map().expect("a shared map");
                Some(CellMemory {
                    grid,
                    styles,
                    styles_mem,
                    map_buffer,
                    map: GlyphMap::over(entries),
                })
            }
            _ => None,
        };
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
            pending: Pending::Dirty,
            frames: Tally::default(),
            last_report: std::time::Instant::now(),
            stats: Frame::default(),
            gpu_stats: GpuFrame::default(),
            wanted,
            last_commit: Instant::now(),
            interval: Duration::from_micros(8333),
            last_tick: None,
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
                (Path::Instances | Path::Cells, None, _) => {
                    Presenter::with_gpu(w, h).or_else(|| {
                        eprintln!(
                            "BAUD_GPU set but no Metal device or shaders; drawing on the CPU"
                        );
                        Presenter::new(w, h)
                    })
                }
                (Path::Cpu, _, _) => Presenter::new(w, h),
            };
            if let Some(p) = &self.presenter {
                eprintln!("presenting through {:?}, memory {:?}", p.mode(), p.memory());
            }
            self.pending = Pending::Dirty;
        }
        if self.pending == Pending::Clean {
            return;
        }
        let damage = Damage::Incremental;
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
            self.app.pending = Pending::Dirty;
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

pub fn run() {
    let mtm = MainThreadMarker::new().expect("run() on the main thread");
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let size = NSSize::new(960.0, 600.0);
    let window = GlassWindow::new(mtm, size, "baud");
    let scale = window.ns_window().backingScaleFactor();
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let glass = GlassSurface::new(mtm, frame, GlassStyle::Regular, 16.0).expect("glass");
    let view = DrawableView::new(mtm, frame);
    // The initial grid from the window's points and the cell size at this
    // scale; the view's first resize callback settles it.
    let probe = App::new(
        WinSize {
            ws_col: 80,
            ws_row: 24,
        },
        scale,
        view.waker(),
    );
    // `BAUD_GRID=COLSxROWS` sizes the window to a grid, in points from the
    // cell metrics at this scale, so a benchmark runs every terminal at one
    // geometry.
    if let Some(grid) = std::env::var("BAUD_GRID").ok().and_then(|g| {
        let (c, r) = g.split_once('x')?;
        Some((c.parse::<f64>().ok()?, r.parse::<f64>().ok()?))
    }) {
        let (cw, ch) = probe.metrics_cell();
        let wanted = NSSize::new(grid.0 * cw / scale, grid.1 * ch / scale);
        window.ns_window().setContentSize(wanted);
    }
    let driver = Driver {
        app: probe,
        view: view.clone(),
    };
    view.set_responder(Box::new(driver));
    glass.set_content_view(&view);
    window.set_content_view(glass.view());
    window.set_accepts_mouse_moved(true);
    window.set_min_size(NSSize::new(200.0, 100.0));
    window.center();
    window.show();
    window.make_first_responder(&view);
    view.start_display_link();
    ns_app.activate();
    ns_app.run();
}
