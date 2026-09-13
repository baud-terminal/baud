//! The window, the responder, and the frame.
#![forbid(unsafe_code)]

use crate::pty::Pty;
use baud_core::{Owned, Term};
use baud_domain::PackedCell;
use baud_domain::{
    BytePos, Codepoint, Cols, FnKey, KeyCode, KeyEvent, Modifiers, ReadLen, Rows, WinSize,
};
use baud_font::{Face, FaceIndex, Px};
use baud_present_macos::{CellInput, GpuFrame, Mode, Presenter};
use baud_render::{Blend, Damage, Frame, GlyphMap, PxSize, Renderer, Stack, Tally, Target, Theme};
use macos_liquid_glass::drawable::{
    DrawableView, Focus, Handled, KeyPress, Mouse, Rect, Responder, Scroll,
};
use macos_liquid_glass::glass::{GlassStyle, GlassSurface};
use macos_liquid_glass::objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use macos_liquid_glass::window::GlassWindow;

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

struct App {
    term: Term<'static>,
    renderer: Renderer<'static, 'static>,
    presenter: Option<Presenter>,
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
    /// The cell path's glyph map and its copy of the grid: the copy exists
    /// because `Term::viewport` borrows the terminal exclusively while the
    /// styles still have to be resolved against it.
    map: GlyphMap,
    cells: Vec<PackedCell>,
}

impl App {
    fn new(ws: WinSize, scale: CGFloat) -> App {
        // The storage, the font files and the faces live as long as the
        // process: the renderer borrows them for the life of the window.
        let owned: &'static mut Owned = Box::leak(Box::new(Owned::new(&MAX_GRID)));
        let term = Term::new(owned.storage(), &ws);
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
        let pty = Pty::spawn(&ws).expect("pty");
        let (frame_w, frame_h) = renderer.pixel_size();
        App {
            term,
            renderer,
            presenter: None,
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
            wanted: wanted(),
            map: GlyphMap::new(),
            cells: Vec::new(),
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
            self.presenter = match self.wanted {
                Path::Instances | Path::Cells => Presenter::with_gpu(w, h).or_else(|| {
                    eprintln!("BAUD_GPU set but no Metal device or shaders; drawing on the CPU");
                    Presenter::new(w, h)
                }),
                Path::Cpu => Presenter::new(w, h),
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
        match self.presenter.as_ref().map(Presenter::mode) {
            Some(Mode::Gpu) if self.wanted == Path::Cells => {
                let t0 = std::time::Instant::now();
                self.cells.clear();
                self.cells.extend_from_slice(self.term.viewport());
                self.map.ensure(&self.cells, &mut self.renderer);
                let cursor = self.term.cursor_visible().then(|| {
                    let (r, c) = self.term.cursor();
                    (r, c, self.term.cursor_style())
                });
                let styles = self.renderer.resolve_styles(&self.term).to_vec();
                self.stats = Frame {
                    convert: t0.elapsed(),
                    ..Frame::default()
                };
                let input = CellInput {
                    cells: &self.cells,
                    styles: &styles,
                    map: &self.map,
                    cursor,
                };
                if let Some(p) = &mut self.presenter {
                    if let Some(g) = p.draw_cells(view, &self.renderer, &input) {
                        self.gpu_stats = g;
                    }
                }
            }
            Some(Mode::Gpu) => {
                let converted = self.renderer.convert(&mut self.term, damage);
                self.stats = Frame {
                    rows_dirty: converted.rows_dirty,
                    rows: converted.rows,
                    convert: converted.convert,
                    ..Frame::default()
                };
                if let Some(p) = &mut self.presenter {
                    if let Some(g) = p.draw(view, &self.renderer) {
                        self.gpu_stats = g;
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
        self.frames.bump();
        if self.last_report.elapsed().as_secs() >= 5 {
            match self.presenter.as_ref().map(Presenter::mode) {
                Some(Mode::Gpu) => eprintln!(
                    "{} frames; last: {} rows, {} instances, convert {:?}, upload {:?}, submit {:?}, gpu {:?}",
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

    fn frame(&mut self, _target: f64) {
        for chunk in self.app.pty.drain() {
            self.app.term.write_at(BytePos::ZERO, &chunk);
            self.app
                .term
                .feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
            self.app.pending = Pending::Dirty;
        }
        self.app.flush_reply();
        if self.app.pending == Pending::Dirty {
            let view = self.view.clone();
            self.app.present(&view);
        }
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
    );
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
