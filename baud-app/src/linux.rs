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
//! What the macOS app has and this does not, yet: the GPU paths, `SGR 5`
//! text blink, the system caret's rate (this keeps xterm's 600/300 until
//! the desktop's settings are read), the wheel onto the scrollback, a backing scale other than one -- the
//! window binds no fractional-scale global -- and the report every five
//! seconds.

use crate::bell::{Bell, Update, UpdateClock};
use crate::caret::{Blinks, Caret, Cycle, Gate, NonBlinking, Owes};
use crate::config::Config;
use crate::host::{
    FIRST_PRESENT, MAX_GRID, cell_px, describe, host_geometry, load_faces, startup_first,
    startup_mark,
};
use crate::pty::{Chunk, Outbox, Pty, Ready, wait};
use baud_core::{Focus, Owned, Resized, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Px;
use baud_present_wayland::{Event, Presented, Window};
use baud_render::{Argb, Blend, Damage, PxSize, Renderer, Target};
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
    /// What the child is owed and the kernel has not taken: every write goes
    /// through it, in order (`pty.rs`).
    outbox: Outbox,
    /// The frame the renderer paints into and the window copies out of:
    /// the window's size, so row damage has a target that still holds the
    /// last frame, with the cells' margin filled with the default
    /// background the screen draws.
    frame: Vec<u32>,
    /// The frame's row pitch: the window's width.
    stride: Px,
    /// The default background at the window's opacity, for the margin
    /// past the last whole cell: the theme's, or `OSC 11`'s, or under `?5`
    /// the foreground. The renderer paints only the grid, so a change here
    /// refills the frame and repaints it whole.
    margin: Argb,
    pending: Pending,
    damage: Damage,
    /// When the synchronised update open now opened, for `SYNC_MAX`.
    updates: UpdateClock<Instant>,
    /// The visual bell no frame drew, ended on the `poll` timeout
    /// (`bell.rs`).
    bell: Bell<Instant>,
    /// The cursor's blink, ended and restarted on the `poll` timeout
    /// (`caret.rs`) -- which also wakes for `[cursor] stop_blinking_after` --
    /// and whether the keyboard is ours -- the blink stops in a window
    /// without it.
    caret: Caret<Instant>,
    focus: Focus,
    /// The title the toplevel was last given, so a program's `OSC 0` or
    /// `OSC 2` reaches it once rather than every read.
    titles: crate::title::Shown,
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
    crate::info!("{}", crate::host::describe_images(&cfg.images));
    let faces = load_faces(&cfg);
    startup_mark("faces");
    let geom = host_geometry();
    crate::info!("cache: {}", describe(&geom));
    let owned: &'static mut Owned = Box::leak(Box::new(
        Owned::with_geometry(&MAX_GRID, &geom).with_images(cfg.images),
    ));
    let mut term = Term::new(owned.storage(), &FIRST);
    // **No appearance signal is wired here, so `Auto` resolves to dark.**
    // The desktop's preference lives behind `org.freedesktop.appearance`
    // on the portal's settings interface, which this driver does not speak
    // yet; naming the key is how a reader gets light until it does.
    let half = match cfg.theme {
        crate::config::ThemeChoice::Light => crate::palette::Half::Light,
        _ => crate::palette::Half::Dark,
    };
    // Both halves are read, so a mistake in the one not drawn is still
    // reported; only the half drawn is built.
    let (halves, said) = crate::palette::load_halves(
        cfg.palette_light.as_deref(),
        cfg.palette_dark.as_deref(),
        &crate::palette::Dirs::from_env(cfg.source.as_deref()),
    );
    for s in said {
        crate::warn!("{s}");
    }
    // The theme `half_theme` builds is the one macOS builds for the same
    // half, dim included: `[glass]`'s material has no counterpart here,
    // and the dim follows the half drawn.
    let theme = crate::palette::half_theme(half, &cfg.glass, cfg.floor, &halves);
    let margin = theme.bg.with_alpha(theme.bg_alpha);
    let mut renderer = Renderer::new(
        crate::host::stack(&cfg, faces),
        PxSize::of(cfg.font_size).expect("size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(FIRST.ws_col)),
        Rows::of_index(usize::from(FIRST.ws_row)),
    )
    .expect("cell metrics");
    renderer.set_emoji_default(cfg.emoji);
    crate::host::tell_defaults(&mut term, renderer.theme(), baud_core::Overrides::Keep);
    // The cell height for the Sixel cursor advance, before the first feed;
    // re-set on every `configure`.
    term.set_cell_size(
        cell_px(renderer.metrics().cell_w),
        cell_px(renderer.metrics().cell_h),
    );
    term.set_sixel_claim(cfg.sixel);
    term.set_vs16_width(cfg.vs16_width);
    term.set_initial_cursor_style(baud_core::CursorStyle::of(
        cfg.cursor_shape,
        cfg.cursor_blink.power_on(),
    ));
    startup_mark("renderer");
    let pty = Pty::spawn_polled(&FIRST, renderer.metrics().cell_w, renderer.metrics().cell_h)
        .expect("pty");
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
    let bell = Bell::new(&term, crate::bell::LENGTH);
    let mut d = Driver {
        bell,
        // xterm's rate, and no switch: `auto` blinks here. Nothing on Linux
        // reads the desktop's caret yet, so the choice is not kept.
        caret: Caret::new(
            Instant::now(),
            Cycle::XTERM,
            Gate::of(cfg.cursor_blink, NonBlinking::Off),
            cfg.cursor_stop,
        ),
        // Until `wl_keyboard.enter`: a window the compositor has not given
        // the keyboard holds a steady cursor.
        focus: Focus::Lost,
        conn,
        window,
        term,
        renderer,
        pty,
        outbox: Outbox::default(),
        frame: Vec::new(),
        stride: Px::of(0),
        margin,
        pending: Pending::Clean,
        damage: Damage::Full,
        updates: UpdateClock::new(SYNC_MAX),
        titles: crate::title::Shown::new(),
    };
    if let Err(e) = d.conn.flush() {
        crate::warn!("wayland: {e}");
        return;
    }
    loop {
        // The loop wakes for a held key's next repeat as well as for the
        // synchronised update's cap and a flash's end, whichever is sooner --
        // each only while it is ahead. One already reached is owed a frame,
        // and what stops that frame -- a frame in flight, a held update, no
        // free buffer -- ends on an event this wait already has; bounding the
        // wait by a past deadline would spin `poll(0)` until it did. The
        // update's cap did exactly that, from before the bell, once an
        // update outlived it.
        let now = Instant::now();
        let sync = d
            .updates
            .deadline()
            .filter(|&t| t > now)
            .map(|t| t.saturating_duration_since(now));
        let repeat = d
            .window
            .repeat_deadline()
            .map(|t| t.saturating_duration_since(now));
        let flash = d
            .bell
            .deadline()
            .filter(|&t| t > now)
            .map(|t| t.saturating_duration_since(now));
        // The cursor's next flip, while it blinks: always ahead of `now`.
        let caret = d
            .caret
            .next_change(now, d.blinks())
            .map(|t| t.saturating_duration_since(now));
        let timeout = [sync, repeat, flash, caret].into_iter().flatten().min();
        let (compositor, pty) = match wait(d.conn.as_fd(), d.pty.as_fd(), d.outbox.owed(), timeout)
        {
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
        // After a timeout as much as after readiness: a repeat is due on
        // the clock, not on an event, and so is a flash's end.
        let now = Instant::now();
        d.window.tick(now);
        if d.bell.is_due(now) {
            d.pending = Pending::Dirty;
        }
        let blinks = d.blinks();
        if d.caret.due(now, blinks) == Owes::Frame {
            d.pending = Pending::Dirty;
        }
        while let Some(e) = d.window.next_event() {
            match e {
                Event::Configured { width, height } => d.configure(width, height),
                Event::Frame => {}
                Event::Key(k) => {
                    d.term.key(k);
                    // A key shows the cursor, restarts its blink, and is
                    // input to its stop.
                    d.caret.input(Instant::now());
                }
                // Reported to the child only if it asked with `?1004`; the
                // reply is flushed with the keys' below. Kept for the blink,
                // which stops without the keyboard: a cursor hidden when it
                // leaves is owed the frame below that shows it.
                Event::Focus(f) => {
                    d.term.focus(f);
                    d.focus = f;
                    if f == Focus::Gained {
                        d.caret.input(Instant::now());
                    }
                    d.pending = Pending::Dirty;
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
    /// Whether the cursor blinks now: a style that asks to, the keyboard, and
    /// a cursor on the screen (`caret.rs`).
    fn blinks(&self) -> Blinks {
        crate::caret::blinks(
            self.term.cursor_style(),
            self.focus,
            self.term.cursor_visible().then(|| self.term.cursor()),
        )
    }

    /// One read's worth of the child's output into the core.
    fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.term.write_at(BytePos::ZERO, bytes);
        self.term
            .feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        self.pending = Pending::Dirty;
        // Output shows the cursor and restarts its blink (`caret.rs`).
        self.caret.restart(Instant::now());
        if let Some(t) = self
            .titles
            .next(self.term.title(), self.term.input_encoding(), "baud")
        {
            self.window.set_title(&mut self.conn.sender(), &t);
        }
    }

    /// Whatever the core answered -- a key, a query -- to the child, behind
    /// whatever the kernel has not yet taken. What it cannot take now stays
    /// owed, and the wait wakes for room to write it (`Outbox::owed`).
    fn flush_reply(&mut self) {
        self.outbox.push(self.term.reply());
        self.term.clear_reply();
        if !self.outbox.is_empty() {
            self.pty.write(&mut self.outbox);
        }
    }

    /// The compositor's size for the window: the grid that fits it, the
    /// core and the renderer resized to that, a fresh frame at the
    /// window's size, and the child told.
    fn configure(&mut self, width: baud_present_wayland::Px, height: baud_present_wayland::Px) {
        let m = self.renderer.metrics();
        // The core advances the cursor past a Sixel by cell rows: give it the
        // height. Set on every configure, which is where a cell size could
        // change (a font or scale change would arrive as one).
        self.term
            .set_cell_size(cell_px(m.cell_w), cell_px(m.cell_h));
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
        // The grid the terminal *has*, not the one asked for -- `resize`
        // clamps, and a child told the request draws for a screen that does
        // not exist, which the macOS driver learned from btop -- and the text
        // area in pixels from the cell, not the window's, which carries the
        // padding and the part of a cell the grid does not fill.
        let (has_cols, has_rows) = self.term.geometry();
        let has = WinSize {
            ws_col: u16::try_from(has_cols.index()).unwrap_or(u16::MAX),
            ws_row: u16::try_from(has_rows.index()).unwrap_or(u16::MAX),
        };
        let m = self.renderer.metrics();
        self.pty.resize(&has, m.cell_w, m.cell_h);
        // Then the replies: the `?2048` report `resize` or `set_cell_size`
        // queued goes out with the `SIGWINCH`, after it and not before.
        self.flush_reply();
    }

    /// Paint what changed and hand the frame to the window, unless a
    /// frame is already in flight, the window has no size yet, or a
    /// synchronised update is open and younger than `SYNC_MAX`.
    fn present(&mut self) {
        if self.pending == Pending::Clean || self.window.in_flight() || self.window.size().is_none()
        {
            return;
        }
        // Each update is timed from its own opening (`UpdateClock`).
        let update = self.updates.observe(&self.term, Instant::now());
        if update == Update::Honoured {
            // Honoured: what the update changes of `?5` is its to hide.
            self.bell.hold(&self.term);
            return;
        }
        // The visual bell, before the margin: the flash moves it too. Kept
        // only if the frame reaches the compositor, below.
        let bell = self.bell.plan(&self.term, Instant::now(), update);
        self.renderer.set_flash(bell.flash);
        let blinks = self.blinks();
        let caret = self.caret.plan(Instant::now(), blinks);
        self.renderer.set_cursor_blink(caret);
        let margin = self.renderer.clear_color_of(&self.term);
        if margin != self.margin {
            self.margin = margin;
            self.frame.fill(margin.0);
            self.damage = Damage::Full;
        }
        let (w, h) = self.renderer.frame_size();
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
            self.bell.commit(bell);
            self.caret.commit(caret);
            self.pending = Pending::Clean;
        }
    }
}
