//! The terminal on Linux: a Wayland window through `baud-present-wayland`,
//! the pty polled beside the compositor's socket, and one loop -- wait on
//! the two, feed the core what the pty has, dispatch what the compositor
//! sent, paint the rows that changed on the CPU into a frame that keeps
//! the last one, and present it when no frame is in flight. Nothing waits
//! but the one wait, and nothing crosses a thread: the pty is read where
//! it is polled, which is the shape the macOS loop reaches through a
//! reader thread and a waker because its run loop cannot wait on a
//! descriptor.
//!
//! **Written and type-checked on a Mac against the Linux target, and not
//! yet run.** No compositor has shown this window; the protocol, the
//! keymap compiler and the window are tested here over a socket pair,
//! and this file is the wiring between them and the core. Until a Linux
//! box runs it, it is unverified, and it says so at startup.
//!
//! What the macOS app has and this does not, yet: the GPU paths, blink,
//! the wheel onto the scrollback, a backing scale other than one -- the
//! window binds no fractional-scale global -- and the report every five
//! seconds.

use crate::config::Config;
use crate::host::{
    FIRST_PRESENT, MAX_GRID, describe, host_geometry, load_faces, startup_first, startup_mark,
};
use crate::pty::{Chunk, Pty, Ready, wait};
use baud_core::{Owned, Resized, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::Px;
use baud_present_wayland::{Event, Presented, Window};
use baud_render::{Argb, Blend, Damage, PxSize, Renderer, Stack, Target, Theme};
use baud_wayland::Connection;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

/// How long a synchronised update may hold the frame: two 60 Hz periods,
/// the bound the macOS loop keeps for the same reason -- long enough that
/// no real redraw is cut off, short enough that a stalled update is not a
/// frozen window.
const SYNC_MAX: Duration = Duration::from_millis(33);

/// The grid before the compositor has said how large the window is.
const FIRST: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    Clean,
    Dirty,
}

struct Driver {
    conn: Connection,
    window: Window,
    term: Term<'static>,
    renderer: Renderer<'static, 'static>,
    pty: Pty,
    /// The frame the renderer paints into and the window copies out of:
    /// the window's size, so row damage has a target that still holds the
    /// last frame, with the cells' margin filled with the theme's
    /// background.
    frame: Vec<u32>,
    /// The frame's row pitch: the window's width.
    stride: Px,
    /// The theme's background at the window's opacity, for the margin
    /// past the last whole cell.
    margin: Argb,
    pending: Pending,
    damage: Damage,
    sync_since: Option<Instant>,
}

pub fn run() {
    crate::warn!("baud on Wayland: type-checked against Linux and not yet run on a compositor");
    let cfg = Config::load();
    startup_mark("config");
    match &cfg.source {
        Some(p) => crate::info!("config: {}", p.display()),
        None => crate::info!("config: none found; the built-in stack"),
    }
    if cfg.faces.is_empty() {
        crate::warn!(
            "no font: none of the usual DejaVu Sans Mono paths exists; name one in a config"
        );
        std::process::exit(1);
    }
    for f in &cfg.faces {
        crate::info!("face: {}", f.path);
    }
    let faces = load_faces(&cfg);
    startup_mark("faces");
    let geom = host_geometry();
    crate::info!("cache: {}", describe(&geom));
    let owned: &'static mut Owned = Box::leak(Box::new(Owned::with_geometry(&MAX_GRID, &geom)));
    let mut term = Term::new(owned.storage(), &FIRST);
    // **No appearance signal is wired here, so `Auto` resolves to dark.**
    // The desktop's preference lives behind `org.freedesktop.appearance`
    // on the portal's settings interface, which this driver does not speak
    // yet; naming the key is how a reader gets light until it does.
    let mut theme = match cfg.theme {
        crate::config::ThemeChoice::Light => Theme::light(),
        _ => Theme::dark(),
    };
    // The same absence decides the dim: `[glass]`'s pairs are resolved by
    // the appearance, and `dim_light` is unreachable here for exactly as
    // long as `Auto` is. The rest of `[glass]` -- the material, its
    // darkening, the shadow -- is `NSGlassEffectView`'s and has no
    // counterpart on this driver at all.
    theme.bg_alpha = Chan::from_unit(cfg.glass.dim_dark);
    let margin = theme.bg.with_alpha(theme.bg_alpha);
    let renderer = Renderer::new(
        Stack::new(faces).expect("faces"),
        PxSize::of(cfg.font_size).expect("size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(FIRST.ws_col)),
        Rows::of_index(usize::from(FIRST.ws_row)),
    )
    .expect("cell metrics");
    crate::host::tell_defaults(&mut term, renderer.theme());
    startup_mark("renderer");
    let pty = Pty::spawn_polled(&FIRST).expect("pty");
    startup_mark("pty");
    let mut conn = match Connection::connect() {
        Ok(c) => c,
        Err(e) => {
            crate::warn!("wayland: {e}");
            std::process::exit(1);
        }
    };
    startup_mark("wayland");
    let window = match Window::open(&mut conn, "baud", "org.baud.terminal") {
        Ok(w) => w,
        Err(e) => {
            crate::warn!("wayland: {e:?}");
            std::process::exit(1);
        }
    };
    let mut d = Driver {
        conn,
        window,
        term,
        renderer,
        pty,
        frame: Vec::new(),
        stride: Px::of(0),
        margin,
        pending: Pending::Clean,
        damage: Damage::Full,
        sync_since: None,
    };
    if let Err(e) = d.conn.flush() {
        crate::warn!("wayland: {e}");
        return;
    }
    loop {
        let timeout = d.sync_since.map(|s| SYNC_MAX.saturating_sub(s.elapsed()));
        let (compositor, pty) = match wait(d.conn.as_fd(), d.pty.as_fd(), timeout) {
            Ok(r) => r,
            Err(e) => {
                crate::warn!("poll: {e}");
                return;
            }
        };
        if compositor == Ready::Readable {
            match d.conn.receive() {
                Ok(0) => {
                    crate::err!("the compositor closed the connection");
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    crate::warn!("wayland: {e}");
                    return;
                }
            }
            if let Err(e) = d.conn.dispatch(&mut d.window) {
                crate::warn!("wayland: {e:?}");
                return;
            }
        }
        if pty == Ready::Readable {
            match d.pty.read_chunk() {
                Chunk::Closed => return,
                Chunk::Bytes(bytes) => d.feed(&bytes),
            }
        }
        while let Some(e) = d.window.next_event() {
            match e {
                Event::Configured { width, height } => d.configure(width, height),
                Event::Frame => {}
                Event::Key(k) => {
                    d.term.key(k);
                }
                // Reported to the child only if it asked with `?1004`; the
                // reply is flushed with the keys' below.
                Event::Focus(f) => {
                    d.term.focus(f);
                }
                Event::Closed => return,
            }
        }
        d.flush_reply();
        d.present();
        if let Err(e) = d.conn.flush() {
            crate::warn!("wayland: {e}");
            return;
        }
    }
}

impl Driver {
    /// One read's worth of the child's output into the core.
    fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.term.write_at(BytePos::ZERO, bytes);
        self.term
            .feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        self.pending = Pending::Dirty;
    }

    /// Whatever the core answered -- a key, a query -- to the child.
    fn flush_reply(&mut self) {
        let reply = self.term.reply().to_vec();
        if !reply.is_empty() {
            self.pty.write(&reply);
            self.term.clear_reply();
        }
    }

    /// The compositor's size for the window: the grid that fits it, the
    /// core and the renderer resized to that, a fresh frame at the
    /// window's size, and the child told.
    fn configure(&mut self, width: baud_present_wayland::Px, height: baud_present_wayland::Px) {
        let m = self.renderer.metrics();
        let cols = (width.0 / m.cell_w.value().max(1)).clamp(1, u32::from(MAX_GRID.ws_col));
        let rows = (height.0 / m.cell_h.value().max(1)).clamp(1, u32::from(MAX_GRID.ws_row));
        let ws = WinSize {
            ws_col: u16::try_from(cols).unwrap_or(u16::MAX),
            ws_row: u16::try_from(rows).unwrap_or(u16::MAX),
        };
        if let Resized::Truncated { cols, rows } = self.term.resize(&ws) {
            crate::warn!(
                "the window is larger than the grid; {}x{} of it is drawn",
                cols.index(),
                rows.index()
            );
        }
        self.renderer.fit(&self.term);
        let words = usize::try_from(u64::from(width.0) * u64::from(height.0)).unwrap_or(0);
        self.frame = vec![self.margin.0; words];
        self.stride = Px::of(width.0);
        self.damage = Damage::Full;
        self.pending = Pending::Dirty;
        let (pw, ph) = self.renderer.pixel_size();
        self.pty.resize(&ws, pw, ph);
    }

    /// Paint what changed and hand the frame to the window, unless a
    /// frame is already in flight, the window has no size yet, or a
    /// synchronised update is open and younger than `SYNC_MAX`.
    fn present(&mut self) {
        if self.pending == Pending::Clean || self.window.in_flight() || self.window.size().is_none()
        {
            return;
        }
        if self.term.synchronized_output() {
            let since = *self.sync_since.get_or_insert_with(Instant::now);
            if since.elapsed() < SYNC_MAX {
                return;
            }
        } else {
            self.sync_since = None;
        }
        let (w, h) = self.renderer.pixel_size();
        let Some(mut target) = Target::new(&mut self.frame, self.stride, w, h) else {
            return;
        };
        self.renderer
            .render(&mut self.term, &mut target, self.damage);
        self.damage = Damage::Incremental;
        let pitch = usize::try_from(self.stride.value()).unwrap_or(0);
        if self
            .window
            .present(&mut self.conn.sender(), &self.frame, pitch)
            == Presented::Yes
        {
            startup_first("first present", &FIRST_PRESENT);
            self.pending = Pending::Clean;
        }
    }
}
