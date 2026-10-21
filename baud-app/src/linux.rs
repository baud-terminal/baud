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
//! **Type-checked on a Mac against the Linux target, and run only on a
//! headless compositor.** weston's headless backend accepted its frames, a
//! shell on its pty answered `stty size`, and the grid followed the
//! configure (`docs/record.md`, "The Linux app is wired and type-checked,
//! and it has not run"). A headless backend has no display, so nobody has
//! looked at its pixels, and nothing has typed into it on a compositor;
//! the protocol, the keymap compiler and the window are tested here over
//! a socket pair, and this file is the wiring between them and the core.
//! It says so at startup.
//!
//! **The timed effects are the macOS app's, on the CPU path** (`timed.rs`):
//! `SGR 5` and `SGR 6` text blink at their own rates, the cursor trail --
//! an effect such as the fire riding the quad, as the CPU path draws it --
//! the alternate screen's fade under `[screen] transition`, and the
//! scrollbar's thumb, AppKit's overlay style, with its hover, drag and jump
//! (`bar.rs`) and `?30`. A change on the clock with no event to bring it
//! bounds the one wait; motion in flight owes a frame on each turn, which
//! the compositor's frame events pace.
//!
//! **The desktop's appearance and links are the Linux desktop's**: under
//! `theme = "auto"` the palette follows the Settings portal's
//! `color-scheme` over the session bus, read at launch and followed as it
//! changes (`bus.rs`, `appearance.rs`); a Ctrl-click on a link opens it
//! with `xdg-open`, and a `file:` link is revealed by the file manager
//! (`opener.rs`); Ctrl+plus, Ctrl+minus and Ctrl+0 zoom. Tested over a
//! socket pair against another implementation's bytes, never on a desktop.
//!
//! **The desktop's accessibility settings are the macOS app's**: the
//! portal's `reduced-motion` acts as Reduce Motion does -- blinking text
//! held shown, the default trail off, the cursor's blink a hard switch --
//! and its `contrast` as Increase Contrast does, a stricter contrast
//! floor; `BAUD_MOTION` fixes the motion as on macOS (`access.rs`,
//! `appearance.rs`). The portal has no transparency key, so the dim stays
//! the half's.
//!
//! What the macOS app has and this does not, yet: the GPU paths, and with
//! them the smooth-scroll slide, which only the cell path draws -- the CPU
//! path this paints on jumps (`docs/record.md`, "Smooth scroll slides the
//! cell path on the ring, and the one-row cap is the latency answer"); the
//! system caret's rate (this keeps 333 ms a phase until the desktop's
//! settings are read); a backing scale other than one -- the window binds
//! no fractional-scale global; and the report every five seconds.
//!
//! **The pointer is the macOS app's, through the same rules** (`seat.rs`):
//! the wheel over the history, the alternate screen's cursor keys and a
//! tracking child's reports; every report encoding the core has; stream and
//! block selection with Shift's override of tracking; `?2001`'s click and
//! the `OSC 133` prompt's; and `OSC 22`'s shape through `cursor-shape-v1`
//! where the compositor offers it. Tested over a socket pair and through
//! `seat.rs`, never on a compositor with a mouse. **Open**: while the
//! scrollbar holds a press or a drag, its moves return before
//! `Seat::input`, so DEC's locator does not see them here; the macOS app
//! updates the locator first.
//!
//! **Copy and paste are the Linux desktop's** (`baud_present_wayland::clip`):
//! Ctrl+Shift+C copies the selection to the clipboard and Ctrl+Shift+V
//! pastes it; a finished selection goes to xterm's SELECT -- the primary
//! selection, or the clipboard under `?1041` (`clip::select_board`) -- and
//! the middle button's release and Shift+Insert paste SELECT; a program's
//! `OSC 52` write lands on each selection it names (`clip::boards`); a paste
//! goes through `paste.rs` as the macOS app's does. Every transfer is a
//! non-blocking pipe in the loop's one wait. Tested over a socket pair,
//! never against a compositor's clipboard.
//!
//! **Typing in every language**: dead keys and `Multi_key` sequences
//! through the locale's Compose table (`baud_present_wayland::compose`),
//! and an input method through `text-input-v3`, its composition drawn at
//! the cursor through the macOS app's rule (`marked.rs`). Tested over a
//! socket pair, never with an input method running.

use crate::bell::{Bell, Update, UpdateClock};
use crate::caret::{Blinks, Caret, Cycle, Gate, NonBlinking, Owes};
use crate::config::Config;
use crate::host::{
    FIRST_PRESENT, MAX_GRID, cell_px, describe, host_geometry, startup_first, startup_mark,
};
use crate::pty::{Chunk, Outbox, Pty, Ready, wait};
use baud_core::{Focus, Owned, PointerHides, Resized, Term};
use baud_domain::{Cols, DecodeProgress, Rows, WinSize};
use baud_font::Px;
use baud_present_wayland::{Board, Copied, Event, PointerEvent, Presented, Shaped, Window};
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
    /// The faces a program's `OSC 50` can name (`fonts.rs`), the size in
    /// force and the config's, in the pixels this driver draws at, and
    /// the window's last configured size, which a change of font re-lays
    /// the grid in.
    fonts: crate::fonts::Fonts,
    font_size: f32,
    font_base: f32,
    configured: Option<(baud_present_wayland::Px, baud_present_wayland::Px)>,
    /// The status line's rows as the window was last laid out for them
    /// (`Term::status_rows`, `App::status_line`).
    status_rows: Rows,
    pty: Pty,
    /// What the child is owed and the kernel has not taken: every write goes
    /// through it, in order (`pty.rs`).
    outbox: Outbox,
    /// A program's log (`?46`, `OSC 46`), written off this thread.
    hostlog: crate::hostlog::HostLog,
    /// The last page-width request seen (`width.rs`): this driver keeps its
    /// window and says so once.
    widths: crate::width::Widths,
    /// The reads a ReGIS delay holds, fed on the first turn at or after its
    /// deadline, which bounds the wait (`reads::paced`).
    pace: crate::reads::Pace,
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
    /// The image budget the core was provisioned with, so a refusal's log
    /// line names the limit it hit (`host::describe_refusals`).
    images: baud_domain::ImageBudget,
    /// Whether a kitty picture's decode is pending, so the wait does not
    /// block while it is (`reads::decode_wait`).
    decode: DecodeProgress,
    /// What the program's window operations did and did not do.
    performed: crate::winops::Performed,
    /// What the bound `xdg_wm_base` allows of `configure_bounds`, as the
    /// core was last told it for `OSC 61`.
    bounds_event: crate::winops::BoundsEvent,
    /// The pointer: its position, the press that owns a gesture, the
    /// wheel's travel and the selection (`seat.rs`).
    seat: crate::seat::Seat,
    /// The shape last set since the pointer entered, so a read sets it
    /// once; `None` until the first set after an enter.
    shape: Option<baud_present_wayland::CursorShape>,
    /// Whether a key hid the pointer, and what the core said when it was
    /// last asked (`hide.rs`).
    hiding: crate::hide::Hiding,
    /// Whether the missing cursor-shape global has been said.
    shapeless: Said,
    /// What a program's `OSC 52` copies came to (`clip.rs`), the policy,
    /// and the area the core was given.
    clips: crate::clip::Clips,
    clipboard: crate::config::Clipboard,
    /// Whether a copy the compositor could not take has been said.
    uncopied: Said,
    /// What the input method is composing (`marked.rs`), drawn at the
    /// cursor as the macOS app draws AppKit's marked text, and the hints
    /// on each of its codepoints.
    composing: Vec<baud_domain::Codepoint>,
    composing_hints: Vec<baud_render::Hints>,
    /// The find field while it is open (`find.rs`), as the macOS app
    /// keeps it: its current match is the seat's selection.
    finding: Option<crate::find::Finding>,
    /// Text blink, the cursor trail, the alternate screen's fade and the
    /// scrollbar's thumb, each owing a deadline or a frame (`timed.rs`).
    timed: crate::timed::Timed,
    /// The session bus, for the portal's appearance and the file
    /// manager's reveal; `None` once it is gone or was never there.
    bus: Option<crate::bus::Bus>,
    /// The config's `theme`, the half drawn, and what rebuilds a half: the
    /// theme files' schemes, `[glass]`'s dims and the contrast floor.
    theme_choice: crate::config::ThemeChoice,
    half: crate::palette::Half,
    halves: crate::palette::Halves,
    glass: crate::config::Glass,
    floor: baud_render::Floor,
    /// The desktop's contrast setting, as last heard: standard until the
    /// portal says otherwise.
    contrast: crate::access::Contrast,
    /// Where the motion preference comes from, `BAUD_MOTION` or the
    /// desktop, the preference in force, and the config's blink
    /// animation, which reduced motion makes a hard switch.
    motion_source: crate::access::MotionSource,
    motion: crate::config::SystemMotion,
    blink_animation: crate::config::BlinkAnimation,
    /// The `xdg-open`s a link started, reaped on each turn, and this
    /// machine's name for a `file://host/` link.
    opener: crate::opener::Opener,
    host: String,
}

/// **The scrollbar's share of a pointer event** (`bar.rs`), after DEC's
/// locator: at the position the seat last had for a press or a release,
/// a move a drag while a button is down.
fn thumb(
    timed: &mut crate::timed::Timed,
    renderer: &Renderer<'_, '_>,
    pending: &mut Pending,
    term: &mut Term<'_>,
    beside: crate::seat::Beside,
) -> crate::bar::Took {
    use crate::bar::{Event as Bar, Left};
    use crate::gesture::Pressed;
    use crate::seat::Input;
    let e = match (beside.input, beside.at) {
        (Input::Entered(x, y), _) => Bar::Entered { x, y },
        (Input::Moved(x, y), _) => Bar::Moved {
            x,
            y,
            held: beside.held,
        },
        (Input::Pressed { button, .. }, Some((x, y))) => Bar::Pressed {
            x,
            y,
            left: match button {
                Pressed::Left => Left::Yes,
                _ => Left::No,
            },
        },
        (Input::Released { .. }, Some((x, y))) => Bar::Released { x, y },
        (Input::Left, _) => Bar::Left,
        _ => return crate::bar::Took::No,
    };
    let m = renderer.metrics();
    let f = crate::bar::Frame {
        size: renderer.frame_size(),
        cell_h: m.cell_h,
        widths: crate::reveal::widths(crate::reveal::Scale(1.0)),
    };
    let now = timed.clock.host(Instant::now());
    let (took, owes) = timed.bar.input(term, e, f, now);
    if owes == crate::bar::Owes::Frame {
        *pending = Pending::Dirty;
    }
    took
}

/// Whether a one-time line has been logged.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Said {
    Not,
    Once,
}

pub fn run() {
    crate::warn!(
        "baud on Wayland: run only on a headless compositor, so nobody has looked at its pixels, typed into it, moved a mouse over it or pasted into it"
    );
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
    let data = crate::host::map_faces(&cfg);
    let faces = crate::host::parse_faces(&cfg, &data);
    let fonts = crate::fonts::Fonts::new(&cfg, data, faces);
    startup_mark("faces");
    let geom = host_geometry();
    crate::info!("cache: {}", describe(&geom));
    let owned: &'static mut Owned = Box::leak(Box::new(
        Owned::with_geometry(&MAX_GRID, &geom)
            .with_images(cfg.images)
            .with_clipboard(cfg.clipboard.budget),
    ));
    let mut term = Term::new(owned.storage(), &FIRST);
    // The kitty file mediums, read with the text's checks
    // (`kitty_files.rs`); leaked as the storage is.
    term.set_kitty_files(Box::leak(Box::new(crate::kitty_files::LocalFiles::new())));
    // ReGIS draws its built-in alphabet with the embedded face
    // (`regis_font.rs`).
    term.set_regis_font(crate::regis_font::glyph);
    // A ReGIS delay stops the read, and the loop feeds the rest on the turn
    // its deadline wakes (`reads::paced`).
    term.set_regis_hold(baud_core::RegisHold::On);
    // A deleted picture's pages go back to the system (`pages.rs`).
    if let Some(release) = crate::pages::release() {
        term.set_page_release(release);
    }
    // **The desktop's appearance, from the Settings portal over the session
    // bus** (`bus.rs`, `appearance.rs`): asked now, before the window opens,
    // so the answer usually lands before the first frame and never holds
    // it. Until it does, and with no bus, `auto` draws dark, as on macOS
    // before the system has said anything.
    let bus = match crate::bus::Bus::connect() {
        Ok(b) => Some(b),
        Err(e) => {
            crate::info!("{e}: theme = \"auto\" draws dark, and a file: link is not revealed");
            None
        }
    };
    let half = crate::appearance::half_for(cfg.theme, None);
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
    // and the dim follows the half drawn. Only the backdrop differs: with
    // no material modelled, the floor reads text against black to white.
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
    crate::host::set_widths(&mut term, cfg.vs16_width, cfg.ambiguous_width);
    crate::host::set_checksum(&mut term, cfg.checksum);
    crate::host::set_window_ops(&mut term, cfg.window_ops);
    crate::host::set_window_abilities(
        &mut term,
        crate::winops::wayland_abilities(crate::winops::BoundsEvent::Never),
        cfg.clipboard.osc52,
    );
    crate::host::set_term_name(&mut term);
    crate::host::set_logging(&mut term, cfg.logging);
    // **No print panel in this driver**: the core is told there is no
    // printer, so DSR `?15` answers "no printer" and a print is ignored and
    // counted, whatever `[screen] printing` says (`print.rs`; note AW).
    crate::print::set_printer(&mut term, cfg.printing, crate::print::Printers::None);
    term.set_graphics_print(cfg.graphics_print);
    crate::host::set_pointer_mode(&mut term, cfg.pointer_mode);
    crate::host::set_font_changes(&mut term, cfg.font_changes);
    crate::host::tell_font(&mut term, &fonts, cfg.font_size, cfg.font_size);
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
            crate::warn!("{}", crate::log::no_wayland_session(e));
            std::process::exit(1);
        }
    };
    startup_mark("wayland");
    let mut window = match Window::open(&mut conn, "baud", "org.baud.terminal") {
        Ok(w) => w,
        Err(e) => {
            crate::warn!("wayland: {e:?}");
            std::process::exit(1);
        }
    };
    // Dead keys and `Multi_key` sequences, from the table the session's
    // locale names, found where libxkbcommon finds it.
    match baud_present_wayland::compose::from_session(&crate::pty::locale_installed) {
        Some(f) => match f.table {
            Ok(t) => {
                crate::info!("compose: {}, {} sequences", f.path, t.entries().len());
                window.set_compose(Some(t));
            }
            Err(e) => crate::warn!(
                "compose: {} makes no table ({e:?}); dead keys type nothing",
                f.path
            ),
        },
        None => crate::info!("compose: no Compose file found; dead keys type nothing"),
    }
    let bell = Bell::new(&term, crate::bell::LENGTH);
    // `BAUD_MOTION` fixes the motion preference as on macOS; otherwise it
    // is full until the portal says, which the answer to the launch's
    // read usually does before the first frame.
    let motion_source =
        crate::access::motion_override(std::env::var("BAUD_MOTION").ok().as_deref());
    let widths = crate::width::Widths::new(&term);
    let clips = crate::clip::Clips::for_wayland(&term);
    let hiding = crate::hide::Hiding::new(&term);
    let mut d = Driver {
        bell,
        // The config's interval, else 333 ms a phase, and no switch: `auto`
        // blinks here. Nothing on Linux reads the desktop's caret yet, so
        // the choice is not kept; the animation is the config's until the
        // motion preference says otherwise (`set_motion`).
        caret: Caret::new(
            Instant::now(),
            Cycle::of_config(cfg.blink_interval),
            Gate::of(cfg.cursor_blink, NonBlinking::Off),
            cfg.cursor_stop,
            cfg.blink_animation,
        ),
        // Until `wl_keyboard.enter`: a window the compositor has not given
        // the keyboard holds a steady cursor.
        focus: Focus::Lost,
        conn,
        window,
        term,
        renderer,
        fonts,
        font_size: cfg.font_size,
        font_base: cfg.font_size,
        configured: None,
        status_rows: Rows::ZERO,
        pty,
        outbox: Outbox::default(),
        hostlog: crate::hostlog::HostLog::new(crate::pty::start_dir()),
        widths,
        pace: crate::reads::Pace::new(),
        frame: Vec::new(),
        stride: Px::of(0),
        margin,
        pending: Pending::Clean,
        damage: Damage::Full,
        updates: UpdateClock::new(SYNC_MAX),
        titles: crate::title::Shown::new(),
        images: cfg.images,
        decode: DecodeProgress::Idle,
        performed: crate::winops::Performed::new(),
        bounds_event: crate::winops::BoundsEvent::Never,
        seat: crate::seat::Seat::new(),
        shape: None,
        hiding,
        shapeless: Said::Not,
        clips,
        clipboard: cfg.clipboard,
        uncopied: Said::Not,
        composing: Vec::new(),
        composing_hints: Vec::new(),
        finding: None,
        timed: crate::timed::Timed::new(Instant::now(), cfg.cursor_trail, cfg.screen_transition),
        bus,
        theme_choice: cfg.theme,
        half,
        halves,
        glass: cfg.glass,
        floor: cfg.floor,
        contrast: crate::access::Contrast::Standard,
        motion_source,
        motion: crate::config::SystemMotion::Full,
        blink_animation: cfg.blink_animation,
        opener: crate::opener::Opener::default(),
        host: crate::opener::host_name(),
    };
    if let crate::access::MotionSource::Fixed(m) = motion_source {
        d.set_motion(m);
    }
    if let Err(e) = d.conn.flush() {
        crate::warn!("wayland: {e}");
        return;
    }
    'run: loop {
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
        // A kitty picture's decode, a step a turn between reads: the next
        // turn at once while one is pending (`reads::decode_wait`).
        let decode = crate::reads::decode_wait(d.decode);
        // A paste whose owner has gone quiet, abandoned at its deadline.
        let paste = d
            .window
            .transfer_deadline()
            .filter(|&t| t > now)
            .map(|t| t.saturating_duration_since(now));
        // The reads a ReGIS delay holds, owed at its deadline, and those a
        // one-shot ReGIS input held, owed at once when it has ended.
        let delay = match d.pace.released(&d.term) {
            true => Some(std::time::Duration::ZERO),
            false => d.pace.deadline().map(|t| t.saturating_duration_since(now)),
        };
        // The next blink flip of text on screen, a trail's dwell, the
        // thumb's hold: always ahead of `now`. Motion in flight owes a
        // frame on each turn instead, paced by the compositor's frame
        // events (`timed.rs`).
        let effects = d
            .timed
            .deadline(now)
            .map(|t| t.saturating_duration_since(now));
        let timeout = [sync, repeat, flash, caret, decode, paste, delay, effects]
            .into_iter()
            .flatten()
            .min();
        // The clipboard's pipes, each waited on for what it owes, and the
        // session bus, for what it sends and the room to send it.
        let mut transfers: Vec<_> = d
            .window
            .transfers()
            .map(|(fd, want)| {
                let i = match want {
                    baud_present_wayland::Want::Read => crate::pty::Interest::Read,
                    baud_present_wayland::Want::Write => crate::pty::Interest::Write,
                };
                (fd, i)
            })
            .collect();
        if let Some(bus) = &d.bus {
            transfers.push((bus.as_fd(), crate::pty::Interest::Read));
            if bus.wants_write() {
                transfers.push((bus.as_fd(), crate::pty::Interest::Write));
            }
        }
        let waited = wait(
            d.conn.as_fd(),
            d.pty.as_fd(),
            d.outbox.owed(),
            &transfers,
            timeout,
        );
        drop(transfers);
        let (compositor, pty) = match waited {
            Ok(r) => r,
            Err(e) => {
                crate::warn!("poll: {e}");
                break 'run;
            }
        };
        if compositor == Ready::Readable {
            match d.conn.receive() {
                Ok(0) => {
                    crate::err!("the compositor closed the connection");
                    break 'run;
                }
                Ok(_) => {}
                Err(e) => {
                    crate::warn!("wayland: {e}");
                    break 'run;
                }
            }
            if let Err(e) = d.conn.dispatch(&mut d.window) {
                crate::warn!("wayland: {e:?}");
                break 'run;
            }
        }
        if pty == Ready::Readable {
            match d.pty.read_chunk() {
                Chunk::Closed => break 'run,
                Chunk::Bytes(bytes) => d.feed(&bytes),
            }
        }
        // A ReGIS delay's held reads, at its deadline, read or not, and a
        // one-shot input's once it has ended.
        if d.pace.due(Instant::now()) || d.pace.released(&d.term) {
            d.feed(&[]);
        }
        // What the session bus sent, never waiting, and the openers a link
        // started that have ended.
        d.pump_bus();
        d.opener.reap();
        // After a timeout as much as after readiness: a repeat is due on
        // the clock, not on an event, and so is a flash's end. The
        // transfers move as far as their pipes allow, a paste finished
        // arriving as an event below.
        let now = Instant::now();
        d.window.tick(now);
        d.window.pump_transfers(now);
        if d.bell.is_due(now) {
            d.pending = Pending::Dirty;
        }
        let blinks = d.blinks();
        if d.caret.due(now, blinks) == Owes::Frame {
            d.pending = Pending::Dirty;
        }
        match d.timed.turn(now, &mut d.term) {
            crate::timed::Owes::Nothing => {}
            crate::timed::Owes::Frame => d.pending = Pending::Dirty,
            // A blink flip moves no cell word, so row damage cannot see it.
            crate::timed::Owes::Full => {
                d.damage = Damage::Full;
                d.pending = Pending::Dirty;
            }
        }
        while let Some(e) = d.window.next_event() {
            match e {
                Event::Configured { width, height } => d.configure(width, height),
                Event::Frame => {}
                // The find field takes every key while it is open, before a
                // key could snap the window away from the match it shows.
                Event::Key(k) if d.finding.is_some() => {
                    if k.kind != baud_domain::KeyEventKind::Release {
                        d.find_key(crate::find::field_key(k.code, k.mods, k.text));
                    }
                }
                Event::Key(k) => {
                    // A key snaps the window back to the live screen,
                    // where its effect is, and that reveals the thumb.
                    if d.seat.key(&mut d.term) {
                        d.pending = Pending::Dirty;
                        d.reveal_thumb();
                    }
                    // Into an empty buffer, so it always fits and a `false`
                    // is a key that sends nothing (`reads::input`). It was
                    // queued behind the read's answers and its `false`
                    // ignored, so a key after a reply-heavy read was lost.
                    // A full buffer here would be that again, and is said.
                    let overflows = d.term.reply_overflows;
                    let drawn = d.term.images().generation();
                    let queued =
                        crate::reads::input(&mut d.term, |t| t.key(k), |b| d.outbox.push(b));
                    // An arrow key moved a ReGIS input cursor, or a key
                    // ended the input and took it away: a frame.
                    if d.term.images().generation() != drawn {
                        d.pending = Pending::Dirty;
                    }
                    if !queued && d.term.reply_overflows != overflows {
                        crate::warn!("a key was dropped: the reply buffer had no room for it");
                    }
                    // A key that sent bytes hides the pointer as XTSMPOINTER
                    // says (`hide.rs`).
                    if queued {
                        let act = d.hiding.key(&d.term);
                        d.hide_pointer(act);
                    }
                    // A key shows the cursor, restarts its blink, and is
                    // input to its stop.
                    d.caret.input(Instant::now());
                    // SRM reset: the core drew the key itself, a frame owed
                    // now rather than at the next read.
                    if d.term.local_echo() {
                        d.pending = Pending::Dirty;
                    }
                }
                // Reported to the child only if it asked with `?1004`, into
                // an empty buffer like a key, and written with the keys'
                // below. Kept for the blink, which stops without the
                // keyboard: a cursor hidden when it leaves is owed the frame
                // below that shows it.
                Event::Focus(f) => {
                    crate::reads::input(&mut d.term, |t| t.focus(f), |b| d.outbox.push(b));
                    d.focus = f;
                    d.seat.focused(f);
                    if f == Focus::Gained {
                        d.caret.input(Instant::now());
                    } else {
                        // Losing the keyboard shows a pointer a key hid, but
                        // at mode 3, as xterm's `unselectwindow`.
                        let act = d.hiding.crossed(&d.term);
                        d.hide_pointer(act);
                    }
                    d.pending = Pending::Dirty;
                }
                Event::Pointer(p) => d.pointer(p),
                Event::Shortcut(s) => d.shortcut(s),
                Event::Pasted(_, text) => d.paste(&text),
                Event::PasteAbandoned(_) => crate::warn!(
                    "paste: the selection's owner sent nothing for two seconds; nothing was pasted"
                ),
                // The input method's composition changed: drawn now, and
                // gone when it commits, whose text arrives as keys.
                Event::Preedit(p) => {
                    let composing = crate::marked::codepoints(&p.text);
                    let ranges: Vec<_> = p
                        .hints
                        .iter()
                        .map(|h| (h.start.0, h.end.0, hint_of(h.hint)))
                        .collect();
                    let hints = crate::marked::hints(&p.text, &ranges);
                    if composing != d.composing || hints != d.composing_hints {
                        d.composing = composing;
                        d.composing_hints = hints;
                        d.pending = Pending::Dirty;
                    }
                }
                Event::CopyCancelled(_) => {
                    if let Some(line) = d.clips.cancelled_unfocused() {
                        crate::warn!("{line}");
                    }
                }
                Event::Closed => break 'run,
            }
        }
        // The seat's pointer comes and goes with its capabilities: told
        // after the events, before the next read is parsed.
        crate::host::set_locator_device(&mut d.term, d.window.pointer_device());
        // **Once the registry offers `xdg_wm_base`, `OSC 61` answers what
        // its version allows**: below 4 no bounds ever come, and a resize
        // in pixels or to the screen's size is listed as not done
        // (`winops::BoundsEvent`).
        let bounds = match d.window.configure_bounds() {
            baud_present_wayland::ConfigureBounds::Offered => crate::winops::BoundsEvent::Offered,
            baud_present_wayland::ConfigureBounds::Never => crate::winops::BoundsEvent::Never,
        };
        if bounds != d.bounds_event {
            d.bounds_event = bounds;
            d.term
                .set_window_abilities(crate::winops::wayland_abilities(bounds));
        }
        // The step the macOS display link takes a tick, here a turn; its
        // reply goes with the flush below.
        let (decode, shown) = crate::reads::decode_step(&mut d.term);
        d.decode = decode;
        if shown == crate::reads::Shown::Changed {
            d.pending = Pending::Dirty;
        }
        d.flush_reply();
        d.present();
        if let Err(e) = d.conn.flush() {
            crate::warn!("wayland: {e}");
            break 'run;
        }
    }
    // **The session's end, however it came**: the log writes what the
    // last reads queued before the process ends (`HostLog::quit`).
    d.quit_log();
}

impl Driver {
    /// **The log finished before the process ends**: the writer drops its
    /// end of a socket pair once everything before it is written and the
    /// file closed, and this one wait wakes for that or for the bound. No
    /// frame is owed by then; the compositor's socket waits.
    fn quit_log(&mut self) {
        let (mine, theirs) = match std::os::unix::net::UnixStream::pair() {
            Ok(pair) => pair,
            Err(e) => {
                crate::warn!("log: no socket to hear the writer finish: {e}");
                return;
            }
        };
        let mut theirs = Some(theirs);
        loop {
            let end = theirs.take();
            if self.hostlog.quit(Instant::now(), move || drop(end)) == crate::hostlog::Finished::Yes
            {
                return;
            }
            let left = self
                .hostlog
                .quit_by()
                .map(|by| by.saturating_duration_since(Instant::now()));
            if let Err(e) = wait(
                mine.as_fd(),
                mine.as_fd(),
                crate::pty::Owed::Nothing,
                &[],
                left,
            ) {
                crate::warn!("poll: {e}");
                return;
            }
        }
    }

    /// Whether the cursor blinks now: a style that asks to, the keyboard, and
    /// a cursor on the screen or, while DECSASD selects it, on the status
    /// line (`caret.rs`).
    fn blinks(&self) -> Blinks {
        let shown = self.term.cursor_visible()
            || self.term.status_line().is_some_and(|l| l.cursor.is_some());
        crate::caret::blinks(self.term.cursor_style(), self.focus, shown.then_some(()))
    }

    /// One read's worth of the child's output into the core.
    fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() && !self.pace.due(Instant::now()) && !self.pace.released(&self.term) {
            return;
        }
        let refused = self.term.images().refusals;
        // The read's answers go to the outbox now, before a key queues behind
        // them in the reply buffer (`reads::feed`); the loop's
        // `flush_reply` writes them.
        // **A program's `OSC 52` copy is acted on read by read**, as the
        // macOS pump does: the core keeps only the last write it finished.
        // **A program's page width is refused and said once** (`width.rs`):
        // nothing here resizes the window, so no width request holds the
        // read; a change of `?46` does, and the feed logs each part.
        let (widths, pace) = (&mut self.widths, &mut self.pace);
        let (clips, window, conn, osc52) = (
            &mut self.clips,
            &mut self.window,
            &mut self.conn,
            self.clipboard.osc52,
        );
        let mut uncopied = Copied::Set;
        let mut asked = Vec::new();
        // A program's font changes, performed after the read (`fonts.rs`).
        let mut fonts_asked = Vec::new();
        // The watches on the selection's lines, armed before the read
        // (`keep::Before::watched`).
        let before = self.seat.before_read(&mut self.term);
        crate::reads::paced(
            &mut self.term,
            pace,
            Instant::now(),
            [bytes],
            |b| self.outbox.push(b),
            |t| {
                if let Some((d, warn)) = clips.poll(t, osc52) {
                    if let Some(line) = warn {
                        crate::warn!("{line}");
                    }
                    match program_copy(window, conn, t, d) {
                        Copied::Set => {}
                        Copied::Unavailable => uncopied = Copied::Unavailable,
                        Copied::Unfocused => {
                            if let Some(line) = clips.sent_unfocused() {
                                crate::warn!("{line}");
                            }
                        }
                    }
                }
                if let Some((_, Some(line))) = widths.poll(t, crate::width::Host::NoResize) {
                    crate::warn!("{line}");
                }
                fonts_asked.extend_from_slice(t.font_requests());
                t.clear_font_requests();
                asked.extend_from_slice(t.window_requests());
                t.clear_window_requests();
            },
            &mut self.hostlog,
        );
        // Before any key is encoded: a key `Term::key` sends as ISO 8859-1
        // must reach a line discipline whose `IUTF8` is clear.
        self.pty.follow(self.term.input_encoding());
        if let Some(line) = self.clips.core_refusals(&self.term, self.clipboard.budget) {
            crate::warn!("{line}");
        }
        if uncopied == Copied::Unavailable {
            self.uncopied_once();
        }
        // The selection after the read: renumbered, or dropped where the
        // read wrote over it, switched or reset (`seat.rs`). A switch takes
        // the trail away and fades the new screen in; a reset that brought
        // a looked-back window live reveals the thumb.
        let after = self.seat.after_read(&self.term, before);
        // The find's matches after the read, the current one selected
        // again when it moved, as the macOS pump does.
        if let Some(f) = self.finding.as_mut() {
            if f.refresh(&self.term) == crate::find::Moved::Yes {
                let scrolled = self.seat.scrolled();
                self.seat.select(
                    f.find()
                        .current()
                        .and_then(|m| crate::find::selection_of(&self.term, scrolled, m)),
                );
            }
            self.pending = Pending::Dirty;
        }
        if after.surface == crate::seat::Surface::Switched {
            self.timed.switched(Instant::now());
        }
        if after.thumb == crate::seat::Thumb::Reveal {
            self.reveal_thumb();
        }
        // A program's pointer shape, shown at once while the pointer is
        // over the window; outside it, the next enter shows it.
        self.show_pointer();
        // Tracking turned on, a new shape or `RIS` shows a pointer a key
        // hid (`hide.rs`).
        let act = self.hiding.read(&self.term);
        self.hide_pointer(act);
        // The status line's size first: `Window::resize` takes it at once,
        // so a program's resize in the same read is measured on the window
        // that holds the line and wins.
        self.status_line();
        for r in asked {
            self.window_request(r);
        }
        for c in fonts_asked {
            let (at, base) = (self.font_size, self.font_base);
            let plan = crate::host::plan_font(&mut self.term, &self.fonts, &c, at, base);
            if let Some(plan) = plan {
                self.refont(plan.face, plan.points);
            }
        }
        self.pending = Pending::Dirty;
        // A refused image is said by the limit it hit, once a read -- the
        // macOS pump's rule (`host::describe_refusals`).
        if let Some(line) =
            crate::host::describe_refusals(&refused, &self.term.images().refusals, &self.images)
        {
            crate::warn!("{line}");
        }
        // Output shows the cursor and restarts its blink (`caret.rs`).
        self.caret.restart(Instant::now());
        if let Some(t) = self.titles.next(
            self.term.title(),
            self.term.title_encoding(),
            "baud",
            self.term.print_run(),
        ) {
            self.window.set_title(&mut self.conn.sender(), &t);
        }
    }

    /// **One pointer event** into the seat (`seat.rs`): the wheel, a
    /// button, a move, an enter or a leave, with the modifiers the keyboard
    /// holds. Bytes for the child go to the outbox, flushed at the loop's
    /// end with the core's reply.
    fn pointer(&mut self, p: PointerEvent) {
        use crate::gesture::Pressed;
        use crate::seat::{Input, Owed};
        use baud_present_wayland::{Button, EventCode, Press, Scroll};
        let input = match p {
            PointerEvent::Entered(at) => {
                self.shape = None;
                Input::Entered(at.x, at.y)
            }
            PointerEvent::Left => {
                self.shape = None;
                Input::Left
            }
            PointerEvent::Moved(at) => Input::Moved(at.x, at.y),
            PointerEvent::Button {
                button,
                press,
                time,
            } => {
                let button = match button {
                    Button::Left => Pressed::Left,
                    Button::Middle => Pressed::Middle,
                    Button::Right => Pressed::Right,
                    Button::Other(EventCode::SIDE) => Pressed::Fourth,
                    Button::Other(_) => Pressed::Other,
                };
                match press {
                    Press::Pressed => Input::Pressed { button, time },
                    Press::Released => Input::Released { button, time },
                }
            }
            PointerEvent::Scroll(Scroll::Steps(s)) => Input::Steps(s),
            PointerEvent::Scroll(Scroll::Distance(d)) => Input::Distance(d),
            PointerEvent::ScrollStopped => Input::Stopped,
            PointerEvent::ScrollAcross(Scroll::Steps(s)) => Input::StepsAcross(s),
            PointerEvent::ScrollAcross(Scroll::Distance(d)) => Input::DistanceAcross(d),
            PointerEvent::ScrollAcrossStopped => Input::StoppedAcross,
        };
        // A press and the wheel are input to the blink's stop, as on macOS.
        if matches!(
            input,
            Input::Pressed { .. }
                | Input::Steps(_)
                | Input::Distance(_)
                | Input::StepsAcross(_)
                | Input::DistanceAcross(_)
        ) {
            self.caret.quiet(Instant::now());
        }
        let m = self.renderer.metrics();
        let (cols, rows) = self.term.geometry();
        let g = crate::seat::Geometry {
            cell: (f64::from(m.cell_w.value()), f64::from(m.cell_h.value())),
            grid: (
                u16::try_from(cols.index()).unwrap_or(u16::MAX),
                u16::try_from(rows.index()).unwrap_or(u16::MAX),
            ),
        };
        let mods = self.window.modifiers();
        // DEC's locator, then the scrollbar's thumb (`bar.rs`), then the
        // rest of the seat, the macOS order: the locator sees every
        // position and the buttons held, a press the thumb takes keeps its
        // drags and its release, and a move it takes is no one else's.
        let back = self.term.primary_window_back();
        let outbox = &mut self.outbox;
        let (timed, renderer, pending) = (&mut self.timed, &self.renderer, &mut self.pending);
        let owed = self.seat.input_beside(
            &mut self.term,
            input,
            mods,
            g,
            |b| outbox.push(b),
            |term, beside| thumb(timed, renderer, pending, term, beside),
        );
        // The wheel that moved the window over the history reveals the
        // thumb, as AppKit's does.
        if self.term.primary_window_back() != back {
            self.reveal_thumb();
        }
        match owed {
            Owed::Nothing => {}
            Owed::Frame => self.pending = Pending::Dirty,
            Owed::Keys => self.caret.input(Instant::now()),
            // A selection made is SELECT's: the primary, or the clipboard
            // under `?1041`, as xterm's `select-end(SELECT, CUT_BUFFER0)`
            // (Jev's lean, PRIMARY_ONLY, 0.73/0.88: V13's copy-on-select is
            // this on Linux).
            Owed::Selected => {
                self.pending = Pending::Dirty;
                let board = board_of(crate::clip::select_board(&self.term));
                self.copy_selection(board);
            }
            Owed::PasteSelection => {
                let board = board_of(crate::clip::select_board(&self.term));
                self.window.paste(&mut self.conn.sender(), board);
            }
            Owed::Link => {
                if let Some(link) = self.seat.take_link() {
                    self.open_link(&link);
                }
            }
        }
        // A move or a press shows a pointer a key hid; a crossing shows it
        // but at mode 3, where an enter hides it again with the enter's serial, the
        // image being the client's to set on every enter (`hide.rs`).
        match input {
            Input::Entered(..) => match self.hiding.crossed(&self.term) {
                crate::hide::Act::Nothing if self.hiding.hidden() == PointerHides::Yes => {
                    let _ = self.window.hide_pointer(&mut self.conn.sender());
                }
                _ => self.show_pointer(),
            },
            Input::Left => {
                let act = self.hiding.crossed(&self.term);
                self.hide_pointer(act);
            }
            Input::Moved(..) => {
                let act = self.hiding.moved();
                self.hide_pointer(act);
            }
            Input::Pressed { .. } => {
                let act = self.hiding.pressed();
                self.hide_pointer(act);
            }
            _ => {}
        }
    }

    /// **A link to the desktop, or refused and said why**, the macOS
    /// driver's rules (`link::admit`): what it opens goes to `xdg-open`,
    /// one argument and no shell (`opener.rs`), and a `file:` link, which
    /// opening could launch, is revealed by the file manager over the
    /// session bus, or refused where there is no bus.
    fn open_link(&mut self, link: &crate::link::Under) {
        use crate::link::{Action, Host, Under};
        let shown = |uri: &[u8]| {
            uri.iter()
                .take(80)
                .copied()
                .collect::<Vec<u8>>()
                .escape_ascii()
                .to_string()
        };
        let uri = match link {
            Under::Osc8(id) => match self.term.link_uri(*id) {
                Some(uri) => uri,
                None => {
                    crate::warn!("link refused (gone)");
                    return;
                }
            },
            Under::Bare(bare) => match bare.uri() {
                Ok(uri) => uri,
                Err(why) => {
                    crate::warn!("link refused ({why:?}): {}", shown(bare.text()));
                    return;
                }
            },
        };
        let said = shown(uri);
        match crate::link::admit(uri, Host(&self.host)) {
            Err(why) => crate::warn!("link refused ({why:?}): {said}"),
            Ok(t) => match t.action() {
                Action::Open => match self.opener.start(crate::opener::command(t.as_str())) {
                    Ok(()) => crate::info!("link opened ({:?}): {said}", t.scheme()),
                    Err(e) => crate::warn!("link not opened: xdg-open: {e}: {said}"),
                },
                Action::Reveal => match self.bus.as_mut() {
                    Some(bus) => {
                        bus.reveal(&crate::opener::local(t.as_str()));
                        crate::info!("link sent to the file manager: {said}");
                    }
                    None => crate::warn!("link not revealed: no session bus: {said}"),
                },
            },
        }
    }

    /// **What the session bus sent** (`bus.rs`): the desktop's colour
    /// scheme, read or changed, a reveal's answer, or the bus gone.
    fn pump_bus(&mut self) {
        let Some(bus) = self.bus.as_mut() else {
            return;
        };
        for heard in bus.pump() {
            self.heard(heard);
        }
    }

    fn heard(&mut self, heard: crate::bus::Heard) {
        use crate::bus::Heard;
        match heard {
            Heard::Scheme(s, origin) => {
                if let Some(half) = crate::appearance::switch(self.theme_choice, self.half, s) {
                    self.show_half(half, crate::appearance::overrides(origin));
                }
            }
            Heard::NoScheme(e) if self.theme_choice == crate::config::ThemeChoice::Auto => {
                crate::info!(
                    "appearance: the Settings portal gave no color-scheme ({e}); theme = \"auto\" draws dark"
                );
            }
            Heard::NoScheme(_) => {}
            Heard::Motion(m) => {
                self.set_motion(crate::appearance::motion_of(self.motion_source, m))
            }
            Heard::Contrast(c) => {
                let contrast = crate::appearance::contrast_of(c);
                if contrast != self.contrast {
                    self.contrast = contrast;
                    crate::info!("contrast: {contrast:?}");
                    // The contrast moves no colour the core reports, so
                    // only the renderer is told, as on macOS.
                    let theme = self.theme_of(self.half);
                    self.renderer.set_theme(theme);
                    self.damage = Damage::Full;
                    self.pending = Pending::Dirty;
                }
            }
            Heard::Revealed => crate::info!("link revealed by the file manager"),
            Heard::NotRevealed(e) => {
                crate::warn!("link not revealed: the file manager answered {e}")
            }
            Heard::Closed(e) => {
                crate::warn!("{e}");
                self.bus = None;
            }
        }
    }

    /// **The palette's other half**, built as at launch, a program's
    /// colours kept or dropped as `appearance::overrides` says, and the
    /// frame repainted whole: a theme moves no cell word, which row damage
    /// compares.
    fn show_half(&mut self, half: crate::palette::Half, overrides: baud_core::Overrides) {
        self.half = half;
        let theme = self.theme_of(half);
        self.renderer.set_theme(theme);
        crate::host::tell_defaults(&mut self.term, self.renderer.theme(), overrides);
        self.damage = Damage::Full;
        self.pending = Pending::Dirty;
    }

    /// The theme `half` is drawn with under the desktop's contrast setting.
    fn theme_of(&self, half: crate::palette::Half) -> baud_render::Theme {
        crate::appearance::drawn_theme(half, &self.glass, self.floor, &self.halves, self.contrast)
    }

    /// **The motion preference moved**, from the portal or `BAUD_MOTION`:
    /// blinking text, the trail and the cursor's blink animation follow
    /// it, as `poll_motion` does on macOS -- reduced motion always means
    /// a hard blink, whatever the config named.
    fn set_motion(&mut self, m: crate::config::SystemMotion) {
        if m == self.motion {
            return;
        }
        self.motion = m;
        crate::info!("motion: {m:?}");
        self.caret.set_animation(self.blink_animation.effective(m));
        if self.timed.set_motion(m, Instant::now()) == crate::timed::Owes::Frame {
            self.pending = Pending::Dirty;
        }
        // Text a held phase hid is shown again on the next turn's flip
        // check, which repaints whole.
    }

    /// **The thumb shown, held and faded**: the window moved over the
    /// history by the wheel, a key, a paste or a reset.
    fn reveal_thumb(&mut self) {
        let now = self.timed.clock.host(Instant::now());
        self.timed.bar.reveal.reveal(now);
        self.pending = Pending::Dirty;
    }

    /// **What a key, a move, a press or a read did to the pointer**
    /// (`hide.rs`): hidden with no surface while it is over the window,
    /// shown by setting its shape again.
    fn hide_pointer(&mut self, act: crate::hide::Act) {
        match act {
            crate::hide::Act::Hide if self.seat.inside() => {
                let _ = self.window.hide_pointer(&mut self.conn.sender());
            }
            crate::hide::Act::Show => {
                self.shape = None;
                self.show_pointer();
            }
            _ => {}
        }
    }

    /// **A clipboard or a zoom key** (`baud_present_wayland::keys::shortcut`):
    /// Ctrl+Shift+C copies the selection to the clipboard, Ctrl+Shift+V
    /// pastes the clipboard, and Shift+Insert pastes SELECT. A copy with
    /// nothing selected copies nothing. Ctrl+plus and Ctrl+minus zoom a
    /// step of the configured size at a time and Ctrl+0 returns to it, as
    /// Cmd does on macOS (`zoom`).
    fn shortcut(&mut self, s: baud_present_wayland::Shortcut) {
        use baud_present_wayland::Shortcut;
        match s {
            Shortcut::Copy => self.copy_selection(Board::Clipboard),
            Shortcut::Paste => {
                self.window.paste(&mut self.conn.sender(), Board::Clipboard);
            }
            Shortcut::PasteSelection => {
                let board = board_of(crate::clip::select_board(&self.term));
                self.window.paste(&mut self.conn.sender(), board);
            }
            Shortcut::ZoomIn => self.zoom(crate::fonts::Zoom::In),
            Shortcut::ZoomOut => self.zoom(crate::fonts::Zoom::Out),
            Shortcut::ZoomReset => self.zoom(crate::fonts::Zoom::Reset),
            Shortcut::Find => {
                if self.finding.is_none() {
                    self.finding = Some(crate::find::Finding::open(&self.term, Vec::new()));
                }
                self.find_show();
            }
        }
    }

    /// A key in the open field: the query edited, the current match
    /// stepped, or the field closed, the selection left on what it found.
    fn find_key(&mut self, key: crate::find::FieldKey) {
        let Some(f) = self.finding.as_mut() else {
            return;
        };
        if f.key(&self.term, key) == crate::find::Open::Closed {
            self.finding = None;
            self.pending = Pending::Dirty;
            return;
        }
        self.find_show();
    }

    /// **The current match shown**: the window moved over the history to
    /// it, which reveals the thumb, and selected.
    fn find_show(&mut self) {
        let Some(f) = self.finding.as_ref() else {
            return;
        };
        let back = self.term.window_back();
        if let Some(sel) = f.show(&mut self.term, self.seat.scrolled()) {
            self.seat.select(Some(sel));
        }
        if self.term.window_back() != back {
            self.reveal_thumb();
        }
        self.pending = Pending::Dirty;
    }

    /// **A zoom key**: the font in force at the size the step lands on
    /// (`fonts::zoomed`), the grid re-laid in the window as a program's
    /// `OSC 50` lays it.
    fn zoom(&mut self, step: crate::fonts::Zoom) {
        let points = crate::fonts::zoomed(step, self.font_size, self.font_base);
        self.refont(None, points);
    }

    /// The selection's text to `board`, if there is a selection.
    fn copy_selection(&mut self, board: Board) {
        let Some(text) = self.seat.selected_text(&self.term) else {
            return;
        };
        match self.window.copy(&mut self.conn.sender(), board, &text) {
            Copied::Set => {}
            Copied::Unavailable => self.uncopied_once(),
            Copied::Unfocused => {
                if let Some(line) = self.clips.sent_unfocused() {
                    crate::warn!("{line}");
                }
            }
        }
    }

    /// The compositor could not take a copy, said once.
    fn uncopied_once(&mut self) {
        if self.uncopied == Said::Not {
            self.uncopied = Said::Once;
            crate::warn!(
                "clipboard: the compositor took no copy -- it offers no data device or primary selection for it, or no key or click has come yet to name"
            );
        }
    }

    /// **A paste to the child**, the macOS app's: the window back on the
    /// live screen, the cursor shown, and the text in the form the child's
    /// modes give it (`paste.rs`), behind whatever is owed.
    fn paste(&mut self, text: &str) {
        // A paste while the find field is open goes into the query.
        if let Some(f) = self.finding.as_mut() {
            f.typed(&self.term, &crate::marked::codepoints(text));
            self.find_show();
            return;
        }
        if self.seat.key(&mut self.term) {
            self.reveal_thumb();
        }
        self.caret.input(Instant::now());
        self.flush_reply();
        let pty = &self.pty;
        let form = crate::paste::form(&self.term, || pty.literal_next(), || pty.specials());
        self.outbox.push(&crate::paste::bytes(text, form));
        self.flush_reply();
    }

    /// **The pointer's shape for the program's `OSC 22`, or the I-beam**
    /// (`baud_present_wayland::pointer::shape`), set once a change while
    /// the pointer is over the window. Without the cursor-shape global the
    /// window draws the image itself, the I-beam or the arrow
    /// (`baud_present_wayland::cursor`), and that is said once.
    fn show_pointer(&mut self) {
        if !self.seat.inside() {
            return;
        }
        let want = baud_present_wayland::pointer::shape(self.term.pointer_shape());
        if self.shape == Some(want) {
            return;
        }
        match self.window.set_pointer_shape(&mut self.conn.sender(), want) {
            Shaped::Set => self.shape = Some(want),
            Shaped::Drawn => {
                self.shape = Some(want);
                if self.shapeless == Said::Not {
                    self.shapeless = Said::Once;
                    crate::info!(
                        "the compositor offers no cursor-shape global: baud draws its own pointer, an I-beam or an arrow, and a program's OSC 22 shape is the nearer of the two"
                    );
                }
            }
            Shaped::Unavailable => {}
        }
    }

    /// **A program's window operation, as xdg-shell can perform it**
    /// (`crate::winops::wayland`): minimise, maximise and restore, full
    /// screen, and a floating window's own size, which arrives as a
    /// `Configured` event and lays out as a compositor's would. What
    /// xdg-shell has no request for is counted and warned once by its
    /// reason.
    fn window_request(&mut self, r: baud_core::WindowRequest) {
        use crate::winops::{Set, Shown, Unbuilt, WaylandAction, WaylandWindow};
        use baud_present_wayland::{Resize, Toggle};
        let Some((w, h)) = self.window.size() else {
            return;
        };
        let m = self.renderer.metrics();
        let at = WaylandWindow {
            size: (w.0, h.0),
            bounds: self.window.bounds().map(|(bw, bh)| (bw.0, bh.0)),
            shown: match self.window.shown() {
                baud_present_wayland::Shown::Floating => Shown::Floating,
                baud_present_wayland::Shown::Maximized => Shown::Maximized,
                baud_present_wayland::Shown::FullScreen => Shown::FullScreen,
            },
            cell: (m.cell_w.value(), m.cell_h.value()),
            grid: crate::width::Grid::of(&self.term),
            status: self.term.status_rows(),
        };
        let toggle = |s: Set| match s {
            Set::On => Toggle::Set,
            Set::Off => Toggle::Unset,
        };
        let unbuilt = match crate::winops::wayland(r, &at) {
            WaylandAction::Minimize => {
                self.window.minimize(&mut self.conn.sender());
                None
            }
            WaylandAction::SetMaximized(s) => {
                self.window.maximize(&mut self.conn.sender(), toggle(s));
                None
            }
            WaylandAction::SetFullScreen(s) => {
                self.window.full_screen(&mut self.conn.sender(), toggle(s));
                None
            }
            WaylandAction::Resize(x, y) => match self
                .window
                .resize(baud_present_wayland::Px(x), baud_present_wayland::Px(y))
            {
                Resize::Taken => None,
                Resize::Bound => Some(Unbuilt::Bound),
            },
            WaylandAction::Redraw => {
                self.damage = Damage::Full;
                self.pending = Pending::Dirty;
                None
            }
            WaylandAction::Unbuilt(why) => Some(why),
        };
        match unbuilt {
            None => self.performed.did(),
            Some(why) => {
                if let Some(line) = self.performed.unbuilt(why) {
                    crate::warn!("{line}");
                }
            }
        }
    }

    /// **The status line shown or taken away** (DECSSDT), after the read
    /// that changed it and before its window operations: a floating window
    /// grows or shrinks by the line's
    /// row, the grid keeping its rows, and its `Configured` lays the grid
    /// out; where the window cannot change size the grid gives the row up
    /// or takes it back, laid out again at the size it has
    /// ([`crate::winops::wayland_status`]).
    fn status_line(&mut self) {
        use crate::winops::{Shown, StatusChange};
        let now = self.term.status_rows();
        if now == self.status_rows {
            return;
        }
        let was = core::mem::replace(&mut self.status_rows, now);
        self.damage = Damage::Full;
        self.pending = Pending::Dirty;
        let Some((w, h)) = self.configured else {
            return;
        };
        let shown = match self.window.shown() {
            baud_present_wayland::Shown::Floating => Shown::Floating,
            baud_present_wayland::Shown::Maximized => Shown::Maximized,
            baud_present_wayland::Shown::FullScreen => Shown::FullScreen,
        };
        let m = self.renderer.metrics();
        let at = crate::winops::WaylandWindow {
            size: (w.0, h.0),
            bounds: self.window.bounds().map(|(bw, bh)| (bw.0, bh.0)),
            shown,
            cell: (m.cell_w.value(), m.cell_h.value()),
            grid: crate::width::Grid::of(&self.term),
            status: now,
        };
        let resized = match crate::winops::wayland_status(shown, self.term.window_ops()) {
            StatusChange::Resize => {
                let (x, y) = crate::winops::wayland_status_window(at, was).size;
                self.window
                    .resize(baud_present_wayland::Px(x), baud_present_wayland::Px(y))
            }
            StatusChange::Relayout => baud_present_wayland::Resize::Bound,
        };
        if resized == baud_present_wayland::Resize::Bound {
            self.configure(w, h);
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

    /// **The font led by `face`, or the one in force, at `points`** (a
    /// program's `OSC 50`, `fonts.rs`): the renderer restacked or rescaled,
    /// the core told, and the grid re-laid in the window as a configure
    /// lays it.
    fn refont(&mut self, face: Option<crate::fonts::Slot>, points: f32) {
        let face = face.filter(|&f| !self.fonts.leads(f));
        if points == self.font_size && face.is_none() {
            return;
        }
        let Some(px) = PxSize::of(points) else {
            return;
        };
        let done = match face {
            Some(f) => self
                .fonts
                .stack(f)
                .and_then(|b| baud_render::Stack::with_roles(b.faces, &b.roles))
                .and_then(|stack| self.renderer.restack(stack, px))
                .map(|()| self.fonts.set(f)),
            None => self.renderer.rescale(px),
        };
        if done.is_none() {
            return;
        }
        self.font_size = points;
        crate::host::tell_font(&mut self.term, &self.fonts, points, self.font_base);
        if let Some((w, h)) = self.configured {
            self.configure(w, h);
        }
    }

    /// The compositor's size for the window: the grid that fits it, the
    /// core and the renderer resized to that, a fresh frame at the
    /// window's size, and the child told.
    fn configure(&mut self, width: baud_present_wayland::Px, height: baud_present_wayland::Px) {
        self.configured = Some((width, height));
        let m = self.renderer.metrics();
        // The core advances the cursor past a Sixel by cell rows: give it the
        // height. Set on every configure, which is where a cell size could
        // change (a font or scale change would arrive as one).
        self.term
            .set_cell_size(cell_px(m.cell_w), cell_px(m.cell_h));
        // XTWINOPS's window size, `CSI 14 ; 2 t`: the surface the
        // compositor configured, which is the whole window -- nothing here
        // asks for server-side decorations, so none frame it. The rest is
        // not known: xdg-shell tells a client neither its position nor
        // whether it is minimized, and no `wl_output` is bound, so those
        // reports stay silent rather than guessed.
        self.term.set_window(baud_core::WindowFacts {
            window: Some(baud_core::PixelSize {
                width: baud_core::ImagePx::of(u16::try_from(width.0).unwrap_or(u16::MAX)),
                height: baud_core::ImagePx::of(u16::try_from(height.0).unwrap_or(u16::MAX)),
            }),
            ..baud_core::WindowFacts::UNKNOWN
        });
        let cols = (width.0 / m.cell_w.value().max(1)).clamp(1, u32::from(MAX_GRID.ws_col));
        let rows = (height.0 / m.cell_h.value().max(1)).clamp(1, u32::from(MAX_GRID.ws_row));
        // The status line's row is the window's, below the grid: the grid
        // is what is left, and the child is told it (`Term::status_rows`).
        let ws = crate::width::less_status(
            WinSize {
                ws_col: u16::try_from(cols).unwrap_or(u16::MAX),
                ws_row: u16::try_from(rows).unwrap_or(u16::MAX),
            },
            self.term.status_rows(),
        );
        let was = (
            self.term.history_mark(),
            self.term.geometry().0,
            self.term.screen(),
        );
        if let Resized::Truncated { cols, rows } = self.term.resize(&ws) {
            crate::warn!(
                "the window is larger than the grid; {}x{} of it is drawn",
                cols.index(),
                rows.index()
            );
        }
        self.renderer.fit(&self.term);
        // A reflow re-cuts every line, so a selection's columns are gone;
        // a change of height alone on the primary keeps it (note V, V3).
        self.seat.resized(&self.term, was);
        // The find's matches on the re-laid grid, and its current match
        // selected again, as the macOS driver's resize does.
        if let Some(f) = self.finding.as_mut() {
            if let Some(sel) = f.after_resize(&self.term, self.seat.scrolled()) {
                self.seat.select(Some(sel));
            }
        }
        // A resize or a font change moves every cell's pixels under the
        // trail, which names them: the next sighting starts afresh.
        self.timed.snap();
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
        // The selection's share of the view, or `?1001`'s region.
        self.renderer.set_highlight(self.seat.highlight(&self.term));
        // The find field and every match's mark, on the window drawn.
        let look = self
            .finding
            .as_ref()
            .map(|f| f.look(&self.term, self.term.window_back()));
        self.renderer.set_find(&self.term, look);
        // The input method's composition at the live cursor, and the cell
        // its candidate window opens beside, sent when it moved.
        self.renderer.set_composition(crate::marked::composition(
            &self.term,
            &self.composing,
            &self.composing_hints,
        ));
        let m = self.renderer.metrics();
        let [x, y, w, h] = crate::marked::cursor_cell(&self.term, m.cell_w, m.cell_h);
        let px = |p: Px| baud_present_wayland::Px(p.value());
        self.window.set_text_cursor(
            &mut self.conn.sender(),
            baud_present_wayland::CursorRect {
                x: px(x),
                y: px(y),
                width: px(w),
                height: px(h),
            },
        );
        // The thumb, the trail, the fade and the blink phases at this
        // moment (`timed.rs`), in the macOS frame's order.
        let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let cell = crate::timed::Cell {
            px: (side(m.cell_w), side(m.cell_h)),
            height: baud_render::Points(side(m.cell_h)),
        };
        let renderer = &self.renderer;
        let plan = self.timed.plan(
            Instant::now(),
            &self.term,
            |r, c, style| renderer.cursor_rect(r, c, style),
            cell,
            crate::reveal::widths(crate::reveal::Scale(1.0)),
        );
        self.renderer.set_scrollbar(plan.scroll);
        self.renderer.set_screen_fade(plan.fade);
        self.renderer.set_blink(plan.blink);
        self.renderer.set_trail(plan.trail);
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
        // The blink rates on screen, which bound the next wait.
        self.timed
            .drawn(self.renderer.blink_seen(), self.term.regis_blink());
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

/// The renderer's spelling of one of `text-input-v3`'s preedit hints.
fn hint_of(h: baud_present_wayland::Hint) -> baud_render::Hints {
    use baud_present_wayland::Hint;
    use baud_render::Hints;
    match h {
        Hint::Whole => Hints::WHOLE,
        Hint::Selection => Hints::SELECTION,
        Hint::Prediction => Hints::PREDICTION,
        Hint::Prefix => Hints::PREFIX,
        Hint::Suffix => Hints::SUFFIX,
        Hint::SpellingError => Hints::SPELLING_ERROR,
        Hint::ComposeError => Hints::COMPOSE_ERROR,
    }
}

/// The presenter's name for a selection.
fn board_of(b: crate::clip::Board) -> Board {
    match b {
        crate::clip::Board::Clipboard => Board::Clipboard,
        crate::clip::Board::Primary => Board::Primary,
    }
}

/// **A program's `OSC 52` write on the seat's selections**: each it names
/// (`clip::boards`) set or emptied, or nothing where the policy refused
/// it. `Unavailable` when the compositor could take none of them, and
/// `Unfocused` when any went without the keyboard.
fn program_copy(
    window: &mut Window,
    conn: &mut baud_wayland::Connection,
    t: &Term<'_>,
    d: crate::clip::Decision<'_>,
) -> Copied {
    use crate::clip::Decision;
    let Some(targets) = t.clipboard().map(|w| w.targets) else {
        return Copied::Set;
    };
    let Some(boards) = crate::clip::boards(targets, crate::clip::select_board(t)) else {
        return Copied::Set;
    };
    let mut took = Copied::Unavailable;
    for b in boards.each().map(board_of) {
        let c = match d {
            Decision::Set(text) => window.copy(&mut conn.sender(), b, text),
            Decision::Clear => window.clear(&mut conn.sender(), b),
            Decision::Refused(_) => return Copied::Set,
        };
        took = match (took, c) {
            (_, Copied::Unfocused) | (Copied::Unfocused, _) => Copied::Unfocused,
            (_, Copied::Set) | (Copied::Set, _) => Copied::Set,
            _ => Copied::Unavailable,
        };
    }
    took
}
