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
//! **Type-checked on a Mac against the Linux target, and run only on
//! headless compositors.** Under weston's headless backend its frames
//! equal, on every pixel, the CPU renderer's on macOS set up as this
//! driver sets it up, and under sway's a virtual keyboard types into it
//! and its clipboard and wl-copy's trade text (`scripts/linux-pixels.sh`,
//! note BE); under sway 1.7 and 1.10 a virtual pointer selects, scrolls,
//! clicks links and drives a program's mouse reports, and the pointer
//! images it asks for are read off the compositor
//! (`scripts/linux-pointer.sh`, note BE). No screen has shown it; the
//! protocol, the keymap compiler and the window are tested here over a
//! socket pair, and this file is the wiring between them and the core. It
//! says so at startup.
//!
//! **The timed effects are the macOS app's, on the CPU path** (`timed.rs`):
//! `SGR 5` and `SGR 6` text blink at their own rates, the cursor trail --
//! every one the macOS app draws, the fire, the plasma and the rest of the
//! effects included, with the pixels its CPU path draws, under the same
//! flash cap -- the alternate screen's slide under `[screen] transition`, and the
//! scrollbar's thumb, AppKit's overlay style, with its hover, drag and jump
//! (`bar.rs`) and `?30`. A change on the clock with no event to bring it
//! bounds the one wait; motion in flight owes a frame on each turn, which
//! the compositor's frame events pace, and that frame is drawn for the
//! moment it is first seen and, where the compositor offers
//! `commit-timing-v1`, held to it (`Window::draw_for`); a frame a key or
//! the program's output owes is shown as soon as it can be.
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
//! **The frame is drawn at the scale the compositor prefers**, as the
//! macOS app draws at its backing scale (`baud_present_wayland::scale`): a
//! fraction through `wp-fractional-scale-v1` and `wp_viewporter`, else the
//! integer `preferred_buffer_scale` or the outputs' scale. The font keeps
//! its size in points and is drawn at the pixels the scale gives it, the
//! grid laid out again in the frame's pixels when the scale changes, and
//! the pointer's points are mapped to cells at it (`seat.rs`). Under sway's
//! headless backend at 1, 1.5 and 2 the window's buffer, its scale and
//! the cells a click lands on are read off the compositor
//! (`scripts/linux-pointer.sh`, note BE).
//!
//! What the macOS app has and this does not, yet: the GPU paths, and with
//! them the smooth-scroll slide, which only the cell path draws -- the CPU
//! path this paints on jumps; and the system caret's rate (this keeps
//! 333 ms a phase until the desktop's settings are read).
//!
//! **The report every five seconds is the macOS app's** (`report.rs`), at
//! `BAUD_LOG=info`, in the same shape: the frames committed to the
//! compositor and what the last one cost, then the clipboard's, the page
//! width's and the links' counts once each moves. It counts the loop's
//! turns where macOS counts display-link ticks, which this has none of,
//! and leaves out what only the GPU paths and the key-latency instrument
//! measure. Its deadline bounds the one wait while the log shows it, so an
//! idle window reports; at the default level it schedules nothing. Its
//! clock and its counting are tested on every platform, and its wiring
//! here is read off this file by a test.
//!
//! **The pointer is the macOS app's, through the same rules** (`seat.rs`):
//! the wheel over the history, the alternate screen's cursor keys and a
//! tracking child's reports; every report encoding the core has; stream and
//! block selection with Shift's override of tracking; `?2001`'s click and
//! the `OSC 133` prompt's; and `OSC 22`'s shape through `cursor-shape-v1`
//! where the compositor offers it. Tested over a socket pair, through
//! `seat.rs`, and with a virtual pointer on two headless sways. Each
//! pointer event is read with the modifiers held when it came, which the
//! window hands with it, not with what is held once the queue is read.
//!
//! **Copy and paste are the Linux desktop's** (`baud_present_wayland::clip`):
//! Ctrl+Shift+C copies the selection to the clipboard and Ctrl+Shift+V
//! pastes it; a finished selection goes to xterm's SELECT -- the primary
//! selection, or the clipboard under `?1041` (`clip::select_board`) -- and
//! the middle button's release and Shift+Insert paste SELECT; a program's
//! `OSC 52` write lands on each selection it names (`clip::boards`); a paste
//! goes through `paste.rs` as the macOS app's does. Every transfer is a
//! non-blocking pipe in the loop's one wait. Tested over a socket pair,
//! and against sway's clipboard and primary selection.
//!
//! **Typing in every language**: dead keys and `Multi_key` sequences
//! through the locale's Compose table (`baud_present_wayland::compose`),
//! and an input method through `text-input-v3`, its composition drawn at
//! the cursor through the macOS app's rule (`marked.rs`). Tested over a
//! socket pair, never with an input method running.

use crate::bell::{Bell, Rings, Update, UpdateClock};
use crate::caret::{Blinks, Caret, Cycle, Gate, NonBlinking, Owes};
use crate::config::Config;
use crate::host::{
    FIRST_PRESENT, MAX_GRID, cell_px, describe, host_geometry, startup_first, startup_mark,
};
use crate::pty::{Chunk, Outbox, Pty, Ready, wait};
use baud_core::{Focus, Owned, PointerHides, Resized, Term};
use baud_domain::{Cols, DecodeProgress, Rows, WinSize};
use baud_font::Px;
use baud_present_wayland::{
    Board, BufferScale, Changed, Copied, Event, PointerEvent, Presented, Shaped, Window,
};
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

/// What the frame owed is drawn for: the moment it is first seen, when a
/// timed effect owed it -- a trail, a slide, a fade, the splash, the
/// thumb, a blink flip, a kitty animation -- whose picture is a function
/// of that moment and which the compositor's commit timer then holds to
/// its refresh (`Window::draw_for`); else the soonest the compositor can
/// show it, which a key, the pointer and the program's output are owed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameFor {
    Moment,
    Soonest,
}

/// `[window] tearing`, as the window takes it.
fn tearing(t: crate::config::Tearing) -> baud_present_wayland::Tearing {
    match t {
        crate::config::Tearing::On => baud_present_wayland::Tearing::On,
        crate::config::Tearing::Off => baud_present_wayland::Tearing::Off,
    }
}

struct Driver {
    conn: Connection,
    window: Window,
    term: Term<'static>,
    renderer: Renderer<'static, 'static>,
    /// The faces a program's `OSC 50` can name (`fonts.rs`), the size in
    /// force and the config's, in points, and the window's last configured
    /// size, in the surface's points, which a change of font re-lays the
    /// grid in.
    fonts: crate::fonts::Fonts,
    font_size: f32,
    font_base: f32,
    configured: Option<(baud_present_wayland::Px, baud_present_wayland::Px)>,
    /// The scale the frame is laid out and drawn at, the window's when it
    /// was last configured: the font's pixels a point, and the frame's
    /// pixels to the surface's point that a pointer's position is read at.
    scale: BufferScale,
    /// The status line's rows as the window was last laid out for them
    /// (`Term::status_rows`, `App::status_line`).
    status_rows: Rows,
    /// `[window] activity_lights` as this system resolves it -- a line of
    /// their own unless the config says otherwise, the window having no
    /// title bar -- and the lights' clocks (`own_line.rs`).
    activity_lights: baud_render::ActivityLights,
    lights: crate::own_line::LineLights,
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
    /// The old screen kept at a screen switch for its slide, drawn with its
    /// cursor hidden, and the frame each slide frame is composed into:
    /// `frame` stays the live frame, which the renderer paints by rows.
    /// `slid` says the window holds what `frame` does not -- a composed
    /// frame, or the one before its cursor was hidden -- so the next frame
    /// goes to it whole.
    kept: Vec<u32>,
    composed: Vec<u32>,
    slid: crate::effects::Composed,
    /// The frame's row pitch: the window's width.
    stride: Px,
    /// The default background at the window's opacity, for the margin
    /// past the last whole cell: the theme's, or `OSC 11`'s, or under `?5`
    /// the foreground. The renderer paints only the grid, so a change here
    /// refills the frame and repaints it whole.
    margin: Argb,
    pending: Pending,
    /// What the frame owed is drawn for, until a frame is presented.
    frame_for: FrameFor,
    damage: Damage,
    /// The rows the last frame painted, as the window is told them: what
    /// it copies and damages (`Window::present`).
    strips: Vec<baud_present_wayland::Strip>,
    /// When the frames held for a synchronised update began to be held,
    /// for `SYNC_MAX`.
    updates: UpdateClock<Instant>,
    /// The visual bell no frame drew, ended on the `poll` timeout
    /// (`bell.rs`).
    bell: Bell<Instant>,
    /// The bells each read brought, rung once a frame interval, and what
    /// the config's `bell` says a ring does (`bell.rs`, `ring`).
    ringer: crate::bell::Ringer<Instant>,
    bell_style: crate::config::BellStyle,
    /// The cursor's blink, ended and restarted on the `poll` timeout
    /// (`caret.rs`) -- which also wakes for `[cursor] stop_blinking_after` --
    /// and whether the keyboard is ours -- the blink stops in a window
    /// without it.
    caret: Caret<Instant>,
    focus: Focus,
    /// How the cursor is drawn while the keyboard is another window's,
    /// from `[cursor] unfocused_hollow`, handed to the renderer at each
    /// `wl_keyboard.leave` and taken back at each enter.
    unfocused_cursor: baud_render::CursorLook,
    /// The title the toplevel was last given, so a program's `OSC 0` or
    /// `OSC 2` reaches it once rather than every read.
    titles: crate::title::Shown,
    /// The folder and program a title is derived from when a program set
    /// none (`title.rs`), checked where a frame is presented when a read
    /// or a key owes it, and the home directory a folder is spelled from.
    derive: crate::title::Derive,
    home: Option<Vec<u8>>,
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
    /// The board a finished selection went to as SELECT, and the source
    /// that offers it, given up when output takes the selection while
    /// `?1040` is reset (`seat::Select`).
    select_offer: Option<(Board, baud_wayland::ObjectId)>,
    /// What the input method is composing (`marked.rs`), drawn at the
    /// cursor as the macOS app draws AppKit's marked text, and the hints
    /// on each of its codepoints.
    composing: Vec<baud_domain::Codepoint>,
    composing_hints: Vec<baud_render::Hints>,
    /// The find field while it is open (`find.rs`), as the macOS app
    /// keeps it: its current match is the seat's selection.
    finding: Option<crate::find::Finding>,
    /// The presses the program was sent, until their releases
    /// (`pressed.rs`): a release goes where its press went.
    pressed: crate::pressed::Pressed,
    /// Where the input method puts the cursor in it (`marked::caret`).
    composing_caret: baud_render::Caret,
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
    /// `[cursor] guide`: the cursor guide while no program has set it.
    cursor_guide: baud_domain::CursorGuide,
    /// `[screen] badge`: whether a program's badge is drawn.
    badge: crate::config::BadgeShown,
    /// What the badge's variables read of the session that the core does
    /// not keep (`effects::Session`), brought up to date with the title.
    session: crate::effects::Session,
    /// The `xdg-open`s a link started, reaped on each turn, and this
    /// machine's name for a `file://host/` link.
    opener: crate::opener::Opener,
    host: String,
    /// The notifications programs post, delivered over the session bus
    /// (`notify.rs`), and what the notification server said it supports.
    notes: crate::notify::Delivery<Instant, crate::bus::NotifyId>,
    /// The reader of the file iTerm2's `image` names, with every check
    /// kitty's file medium makes.
    pictures: crate::kitty_files::LocalFiles,
    caps: crate::notify::ServerCaps,
    /// What the launcher was last told: a program's progress and its
    /// request for attention.
    launcher: crate::bus::Launcher,
    /// The config's files watched and the config as last read, while
    /// `config_reload` is on (`reload.rs`): the inotify descriptor in the
    /// loop's `poll`, the settle's end in its timeout.
    reload: Option<Reloading>,
    /// What the window says about the config, on a row of its own
    /// (`live::Notice`).
    notice: crate::live::Notice,
    /// The desktop's colour scheme as the portal last said it, for a
    /// reload's `theme = "auto"`.
    scheme: Option<crate::bus::Scheme>,
    /// The five-second report (`report.rs`): the loop's turns, the frames
    /// committed to the compositor, what the last frame cost and the links
    /// activated, logged at `info` from the end of each turn, its deadline
    /// a bound on the wait while the log shows it.
    report: crate::report::Polled<Instant>,
}

/// **The config's watch** (`reload.rs`), and the config the window last
/// took, as read, which a new reading is compared with (`live::between`).
struct Reloading {
    watch: crate::reload::Watch<Instant>,
    read: Config,
}

/// **The scrollbar's share of a pointer event** (`bar.rs`), after DEC's
/// locator: at the position the seat last had for a press or a release,
/// a move a drag while a button is down, its points `scale` pixels each.
fn thumb(
    timed: &mut crate::timed::Timed,
    renderer: &Renderer<'_, '_>,
    pending: &mut Pending,
    term: &mut Term<'_>,
    beside: crate::seat::Beside,
    scale: f64,
) -> crate::bar::Took {
    let strip = f64::from(renderer.strip().value());
    let top = f64::from(renderer.grid_top().value());
    let Some(e) = crate::bar::event_beside(beside, (strip, top), scale) else {
        return crate::bar::Took::No;
    };
    let f = bar_frame(renderer, scale);
    let now = timed.clock.host(Instant::now());
    let (took, owes) = timed.bar.input(term, e, f, now);
    if owes == crate::bar::Owes::Frame {
        *pending = Pending::Dirty;
    }
    took
}

/// The frame the thumb is drawn in: the grid's, its widths AppKit's points
/// at `scale` pixels each, as the macOS app's are.
fn bar_frame(renderer: &Renderer<'_, '_>, scale: f64) -> crate::bar::Frame {
    crate::bar::Frame {
        size: renderer.frame_size(),
        cell_h: renderer.metrics().cell_h,
        widths: crate::reveal::widths(crate::reveal::Scale(scale)),
    }
}

/// Whether a read brought a reply the core queued.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Answered {
    No,
    Yes,
}

/// Whether a one-time line has been logged.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Said {
    Not,
    Once,
}

pub fn run() {
    crate::warn!(
        "baud on Wayland: run only on headless compositors, where its pixels, keys, pointer and clipboard are checked; no screen has shown it"
    );
    // **The watch before the read** (`reload::Watch::around`), so an edit
    // that lands while the config is read is heard; the themes and fonts
    // it names are read below, under it.
    let (launched, watch) =
        crate::reload::Watch::around(&crate::config::lookup(), Instant::now(), || {
            let l = Config::launch();
            let w = &l.watched;
            let files = crate::reload::files(&w.lookup, &w.themes, &w.faces);
            (l, files)
        });
    let notice = crate::live::Notice::of(&launched.said);
    let reload = match launched.config.config_reload {
        crate::config::ConfigReload::On => Some(Reloading {
            watch,
            read: launched.read,
        }),
        crate::config::ConfigReload::Off => None,
    };
    let cfg = launched.config;
    startup_mark("config");
    match &cfg.source {
        Some(p) => crate::info!("config: {}", p.display()),
        None => crate::info!("config: none found; the built-in stack"),
    }
    if cfg.faces.is_empty() {
        crate::warn!("no font: the stack is empty; name one in a config");
        std::process::exit(1);
    }
    for f in &cfg.faces {
        crate::info!("face: {}", f.path);
    }
    crate::info!("{}", crate::host::describe_images(&cfg.images));
    let (data, mut held) = crate::host::map_faces(&cfg);
    let faces = crate::host::parse_faces(&cfg, &data, &mut held);
    let fonts = crate::fonts::Fonts::new(&cfg, data, faces, held);
    startup_mark("faces");
    let geom = host_geometry();
    crate::info!("cache: {}", describe(&geom));
    let owned: &'static mut Owned = Box::leak(Box::new(
        Owned::with_geometry(&MAX_GRID, &geom)
            .with_images(cfg.images)
            .with_clipboard(cfg.clipboard.budget)
            .with_notices(crate::notify::BUDGET),
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
    // A read stops once its answers fill the reply buffer past one
    // control's worth, and the loop sends them before the rest is parsed.
    crate::reads::hold_replies(&mut term);
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
    let (mut halves, said) = crate::palette::load_halves(
        cfg.palette_light.as_deref(),
        cfg.palette_dark.as_deref(),
        &crate::palette::Dirs::from_env(cfg.source.as_deref()),
    );
    halves.selection = cfg.selection;
    halves.cursor = cfg.cursor;
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
        crate::host::stack(&fonts),
        PxSize::of(cfg.font_size).expect("size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(FIRST.ws_col)),
        Rows::of_index(usize::from(FIRST.ws_row)),
    )
    .expect("cell metrics");
    crate::host::set_up_renderer(&mut renderer, &cfg, &notice);
    // The activity lights' own line, at the first configure's scale once
    // that comes (`configure`).
    renderer.set_lights_line(cfg.activity_lights.line(), 1.0);
    // The window starts without the keyboard, until `wl_keyboard.enter`.
    renderer.set_cursor_look(crate::caret::look(Focus::Lost, cfg.unfocused_cursor));
    let term = crate::host::set_up_term(term, &cfg, &renderer, &fonts);
    startup_mark("renderer");
    let pty = Pty::spawn_polled(
        &FIRST,
        renderer.metrics().cell_w,
        renderer.metrics().cell_h,
        cfg.shell_integration,
    )
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
    // `[window] tearing`, taken once the surface and the compositor's
    // `tearing-control-v1` both exist.
    window.set_tearing(&mut conn.sender(), tearing(cfg.tearing));
    let bell = Bell::new(&term, crate::bell::LENGTH);
    // `BAUD_MOTION` fixes the motion preference as on macOS; otherwise it
    // is full until the portal says, which the answer to the launch's
    // read usually does before the first frame.
    let motion_source =
        crate::access::motion_override(std::env::var("BAUD_MOTION").ok().as_deref());
    let widths = crate::width::Widths::new(&term);
    let clips = crate::clip::Clips::for_wayland(&term);
    let hiding = crate::hide::Hiding::new(&term);
    // **A program's notifications go to the session bus's notification
    // server** (`notify.rs`, `bus.rs`): the core is told none is delivered
    // until the server answers `GetCapabilities`, so with no bus or no
    // server kitty's query goes unanswered.
    let notes = crate::notify::Delivery::new(cfg.notifications, &term, cfg.images);
    let mut d = Driver {
        bell,
        ringer: crate::bell::Ringer::new(&term),
        bell_style: cfg.bell,
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
        unfocused_cursor: cfg.unfocused_cursor,
        conn,
        window,
        term,
        renderer,
        fonts,
        font_size: cfg.font_size,
        font_base: cfg.font_size,
        configured: None,
        scale: BufferScale::ONE,
        status_rows: Rows::ZERO,
        activity_lights: cfg.activity_lights,
        lights: crate::own_line::LineLights::new(),
        pty,
        outbox: Outbox::default(),
        hostlog: crate::hostlog::HostLog::new(crate::pty::start_dir()),
        widths,
        pace: crate::reads::Pace::new(),
        frame: Vec::new(),
        kept: Vec::new(),
        composed: Vec::new(),
        slid: crate::effects::Composed::No,
        stride: Px::of(0),
        margin,
        pending: Pending::Clean,
        frame_for: FrameFor::Soonest,
        damage: Damage::Full,
        strips: Vec::new(),
        updates: UpdateClock::new(SYNC_MAX),
        titles: crate::title::Shown::new(),
        derive: crate::title::Derive::new(),
        home: std::env::var_os("HOME")
            .map(|h| std::os::unix::ffi::OsStrExt::as_bytes(h.as_os_str()).to_vec()),
        images: cfg.images,
        decode: DecodeProgress::Idle,
        performed: crate::winops::Performed::new(),
        bounds_event: crate::winops::BoundsEvent::Never,
        seat: {
            let mut seat = crate::seat::Seat::new();
            seat.set_word_wrap(cfg.wrap_words);
            seat
        },
        shape: None,
        hiding,
        shapeless: Said::Not,
        clips,
        clipboard: cfg.clipboard,
        uncopied: Said::Not,
        select_offer: None,
        composing: Vec::new(),
        composing_hints: Vec::new(),
        finding: None,
        pressed: crate::pressed::Pressed::default(),
        composing_caret: baud_render::Caret::End,
        timed: crate::timed::Timed::new(Instant::now(), cfg.cursor_trail, cfg.screen_transition)
            .with_trail_scroll(cfg.trail_scroll)
            .with_splash(
                match crate::windows::started(
                    std::env::var_os(crate::windows::NEW_WINDOW).as_deref(),
                ) {
                    // A new window's process: not a launch, so no logo.
                    crate::windows::Started::Window => crate::config::LaunchSplash::Off,
                    crate::windows::Started::Launch => crate::splash::splash_override(
                        std::env::var("BAUD_SPLASH").ok().as_deref(),
                        cfg.launch_splash,
                    ),
                },
            ),
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
        cursor_guide: cfg.cursor_guide,
        badge: cfg.badge,
        session: crate::effects::Session::here(),
        opener: crate::opener::Opener::default(),
        host: crate::opener::host_name(),
        notes,
        pictures: crate::kitty_files::LocalFiles::new(),
        caps: crate::notify::ServerCaps::NONE,
        launcher: crate::bus::Launcher::IDLE,
        reload,
        notice,
        scheme: None,
        report: crate::report::Polled::new(Instant::now(), crate::report::Logged::now()),
    };
    d.timed.set_image_animation(cfg.image_animation);
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
        // A kitty animation's next frame (`Timed::frame_deadline`).
        let frames = d
            .timed
            .frame_deadline(now, &d.term)
            .map(|t| t.saturating_duration_since(now));
        // A notification's `w`, closed at its deadline whatever the server
        // does with `expire_timeout`.
        let expiry = d
            .notes
            .shown
            .next_due()
            .map(|t| t.saturating_duration_since(now));
        // The config's settle, ended on the timeout (`reload.rs`).
        let settle = d
            .reload
            .as_ref()
            .and_then(|r| r.watch.deadline())
            .map(|t| t.saturating_duration_since(now));
        // The five-second report, while the log shows it: the one bound an
        // idle window's wait has (`report::Polled::wait`).
        let report = d.report.wait(now);
        // The soonest an activity light can change, on its own line.
        let lights = d.lights_wait(now);
        let timeout = [
            sync, repeat, flash, caret, decode, paste, delay, effects, frames, expiry, settle,
            report, lights,
        ]
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
        // The config's inotify descriptor, read when it is readable: its
        // place among the extra descriptors, whose readiness the wait says.
        let inotify = d.reload.as_ref().and_then(|r| r.watch.as_fd()).map(|fd| {
            transfers.push((fd, crate::pty::Interest::Read));
            transfers.len() - 1
        });
        let waited = wait(
            d.conn.as_fd(),
            d.pty.as_fd(),
            d.outbox.owed(),
            &transfers,
            timeout,
        );
        drop(transfers);
        let (compositor, pty, extra) = match waited {
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
        // The config's files: what inotify said, when the wait says it is
        // readable, and the reading at the settle's end (`follow_config`).
        let heard = inotify
            .and_then(|i| extra.get(i).copied())
            .unwrap_or(Ready::Idle);
        d.follow_config(heard);
        // The system's faces for what the last frame met and no face draws.
        d.follow_gaps();
        // What the session bus sent, never waiting, and the openers a link
        // started that have ended.
        d.pump_bus();
        d.expire_notices();
        d.opener.reap();
        // After a timeout as much as after readiness: a repeat is due on
        // the clock, not on an event, and so is a flash's end. The
        // transfers move as far as their pipes allow, a paste finished
        // arriving as an event below.
        let now = Instant::now();
        d.window.tick(now);
        d.window.pump_transfers(now);
        // What a frame owes is decided at the moment a frame drawn now is
        // first seen (`Window::next_shown`), the moment it is planned for
        // in `present`: one clock for both, as the macOS app decides and
        // plans by its display link's target.
        let at = d.window.next_shown(now);
        if d.bell.is_due(at) {
            d.pending = Pending::Dirty;
        }
        let blinks = d.blinks();
        if d.caret.due(at, blinks) == Owes::Frame {
            d.pending = Pending::Dirty;
        }
        match d.timed.turn(at, &mut d.term) {
            crate::timed::Owes::Nothing => {}
            crate::timed::Owes::Frame => {
                d.pending = Pending::Dirty;
                d.frame_for = FrameFor::Moment;
            }
            // A blink flip moves no cell word, so row damage cannot see it.
            crate::timed::Owes::Full => {
                d.damage = Damage::Full;
                d.pending = Pending::Dirty;
                d.frame_for = FrameFor::Moment;
            }
        }
        while let Some(e) = d.window.next_event() {
            match e {
                Event::Configured { width, height } => d.configure(width, height),
                // The first frame reached the screen: the compositor's
                // first frame callback, which follows the first commit
                // that attached a buffer, and the splash's clock starts on
                // the next frame (`Timed::shown`).
                Event::Frame => {
                    if d.timed.shown() != crate::timed::Owes::Nothing {
                        d.pending = Pending::Dirty;
                        startup_mark("splash shown");
                    }
                }
                // The find field takes every key while it is open, before a
                // key could snap the window away from the match it shows,
                // and a release goes where its press went (`pressed.rs`).
                Event::Key(k) => {
                    let route = d.pressed.key(k, d.keys_to());
                    d.route_key(route);
                }
                Event::Typed(k) => {
                    let route = crate::pressed::Pressed::typed(k, d.keys_to());
                    d.route_key(route);
                }
                // Reported to the child only if it asked with `?1004`, into
                // an empty buffer like a key, and written with the keys'
                // below. Kept for the blink, which stops without the
                // keyboard: a cursor hidden when it leaves is owed the frame
                // below that shows it.
                Event::Focus(f) => {
                    // The keys still down are let go first: after `leave`
                    // the compositor sends this window none of their
                    // releases (`pressed.rs`).
                    if f == Focus::Lost {
                        d.let_go();
                    }
                    crate::reads::input(&mut d.term, |t| t.focus(f), |b| d.outbox.push(b));
                    d.focus = f;
                    d.seat.focused(f);
                    d.renderer
                        .set_cursor_look(crate::caret::look(f, d.unfocused_cursor));
                    if f == Focus::Gained {
                        d.caret.input(Instant::now());
                    } else {
                        // Losing the keyboard shows a pointer a key hid, but
                        // at mode 3, as xterm's `unselectwindow`.
                        let act = d.hiding.crossed(&d.term);
                        d.hide_pointer(act);
                    }
                    // Without the keyboard no link is previewed, and the
                    // hand goes with it.
                    d.show_pointer();
                    d.pending = Pending::Dirty;
                }
                Event::Pointer(p, mods) => d.pointer(p, mods),
                Event::Modifiers(mods) => d.modifiers(mods),
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
                    let cursor = (p.cursor.begin.0, p.cursor.end.0);
                    let caret = crate::marked::caret(&p.text, cursor);
                    let same = composing == d.composing
                        && hints == d.composing_hints
                        && caret == d.composing_caret;
                    if !same {
                        d.composing = composing;
                        d.composing_hints = hints;
                        d.composing_caret = caret;
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
        // And whether its keymap has a Hebrew layout, which DECHEBM needs.
        crate::host::set_hebrew_layout(&mut d.term, d.window.hebrew_layout());
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
        d.tick_lights();
        d.present();
        if let Err(e) = d.conn.flush() {
            crate::warn!("wayland: {e}");
            break 'run;
        }
        // Every turn, not every frame, as the macOS report is taken every
        // tick: an idle window draws no frame and is the one it has to be
        // able to report on.
        if let Some(lines) = d.report.turn(Instant::now(), &d.clips, &d.widths) {
            for line in lines {
                crate::info!("{line}");
            }
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
        // A program's notifications, taken read by read: the core keeps
        // eight a read (`notify.rs`).
        let icons = &mut self.notes.icons;
        let mut posted = Vec::new();
        // A reply the core queued while it read, which nobody typed: AA.
        let mut answered = Answered::No;
        crate::reads::paced(
            &mut self.term,
            pace,
            Instant::now(),
            [bytes],
            |b| {
                if !b.is_empty() {
                    answered = Answered::Yes;
                }
                self.outbox.push(b);
            },
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
                posted.extend(t.notices().map(|n| crate::notify::Posted::of(&n, icons)));
                t.clear_notices();
            },
            &mut self.hostlog,
        );
        // Before any key is encoded: a key `Term::key` sends as ISO 8859-1
        // must reach a line discipline whose `IUTF8` is clear.
        self.pty.follow(self.term.input_encoding());
        // RD and HS for the read, AA for a reply it brought.
        let now = self.timed.clock.host(Instant::now());
        self.lights.read(bytes, now);
        if answered == Answered::Yes {
            self.lights.answered(now);
        }
        self.deliver(posted);
        // **The bells this read brought ring now**, drawn or not, once a
        // frame interval (`Ringer`): the compositor's, else the macOS
        // driver's starting one until the frame callbacks give one.
        let interval = self
            .window
            .frame_interval()
            .unwrap_or(std::time::Duration::from_micros(8333));
        if let Rings::Ring(volume) = self.ringer.hear(&self.term, Instant::now(), interval) {
            self.ring_bell(volume);
        }
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
        // The lines the whole screen retired ride a trail in flight up with
        // the text at the next frame, as on macOS (`timed.rs`).
        self.timed.scrolled(&self.term, before);
        // Output took the selection while `?1040` is reset: what SELECT
        // offered is given up, as xterm's `DisownSelection` does.
        if after.select == crate::seat::Select::GivenUp {
            self.give_up_select();
        }
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
            // The old screen kept for a slide: the frame last drawn, its
            // rows under the cursor painted again with the cursor hidden,
            // as sfte draws its outgoing screen, unslid. The frame after
            // goes to the window whole. Nothing is kept for no slide.
            let form = self.timed.switched(Instant::now(), self.term.screen());
            if let Some(crate::effects::SwitchForm::Slide(_)) = form {
                let (w, h) = self.renderer.whole_size();
                if let Some(mut target) = Target::new(&mut self.frame, self.stride, w, h) {
                    self.renderer.render_hidden(&self.term, &mut target);
                }
                self.kept.clone_from(&self.frame);
                self.slid = crate::effects::Composed::Yes;
            }
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
            let plan = crate::host::plan_font(&mut self.term, &mut self.fonts, &c, at, at, base);
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
        // The read owes the derived title a check, made at the next frame.
        self.derive.owe();
        self.send_title();
    }

    /// The window's title, sent when it changed (`title::Shown`): the
    /// program's, else the folder and program (`title::Derive::title`),
    /// else "baud". A report naming another host, refused, is logged once
    /// (`Derive::refusal`).
    fn send_title(&mut self) {
        let refused = self.derive.refused;
        let fallback = self
            .derive
            .title(
                self.term.working_directory(),
                std::slice::from_ref(&self.host),
                self.home.as_deref(),
            )
            .unwrap_or_else(|| "baud".to_string());
        if self.derive.refused != refused {
            crate::info!("{}", self.derive.refusal());
        }
        if let Some(t) = self.titles.next(
            self.term.title(),
            self.term.title_encoding(),
            &fallback,
            self.term.print_run(),
        ) {
            self.window.set_title(&mut self.conn.sender(), &t);
        }
        let was = self.session.clone();
        let s = &mut self.session;
        s.pid = self.pty.child();
        s.tty = self.pty.tty().map(|p| p.to_string_lossy().into_owned());
        s.job = self.derive.group();
        s.job_name = self.derive.name().map(str::to_string);
        s.dir = self.derive.dir().map(|d| d.to_string_lossy().into_owned());
        s.title = self.titles.sent().map(str::to_string);
        // A badge naming the job, its folder or the title is drawn again
        // when they change, though the screen did not.
        if self.session != was && self.term.badge_format().is_some() {
            self.pending = Pending::Dirty;
        }
    }

    /// **What a ring does here** (`bell.rs`, `ring`): the rule as the
    /// macOS driver's shim applies it, the window focused while the
    /// keyboard is ours -- xdg-shell tells a client nothing of a minimized
    /// window. The sound is the compositor's system bell, where it offers
    /// `xdg-system-bell-v1`, and nothing without it; the flash is the
    /// visual bell. `?1042`'s attention and `?1043`'s raise are one
    /// request, `xdg-activation-v1`'s `activate` with a token the window
    /// asks for, as foot marks a window urgent
    /// (`[[source: foot-2705e36-wayland]]` 2630-2658): no Wayland request
    /// tells the two apart, and the compositor decides whether the window
    /// is marked for attention or brought forward
    /// (`[[source: xdg-activation-v1]]`).
    fn ring_bell(&mut self, volume: baud_core::BellVolume) {
        use crate::bell::{Attention, Raise, Shown, Sound, Window};
        let state = match self.focus {
            Focus::Gained => Window::Focused,
            Focus::Lost => Window::Unfocused,
        };
        let ring = crate::bell::ring(
            self.bell_style,
            self.motion,
            state,
            self.term.bell_is_urgent(),
            self.term.pop_on_bell(),
            volume,
        );
        if ring.sound == Sound::Beep {
            let _ = self.window.ring_bell(&mut self.conn.sender());
        }
        if ring.shown == Shown::Flash {
            self.bell.ring();
            self.pending = Pending::Dirty;
        }
        if ring.attention == Attention::Request || ring.raise == Raise::Front {
            let _ = self.window.activate(&mut self.conn.sender());
        }
    }

    /// **One pointer event** into the seat (`seat.rs`): the wheel, a
    /// button, a move, an enter or a leave, with the modifiers the keyboard
    /// held when it came. Bytes for the child go to the outbox, flushed at
    /// the loop's end with the core's reply.
    fn pointer(&mut self, p: PointerEvent, mods: baud_domain::Modifiers) {
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
                    Button::Other(EventCode::SIDE) => Pressed::Button8,
                    Button::Other(EventCode::EXTRA) => Pressed::Button9,
                    Button::Other(EventCode::FORWARD) => Pressed::Button10,
                    Button::Other(EventCode::BACK) => Pressed::Button11,
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
        let g = self.seat_geometry();
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
            |term, beside| thumb(timed, renderer, pending, term, beside, g.scale),
        );
        // The wheel that moved the window over the history reveals the
        // thumb, as AppKit's does.
        if self.term.primary_window_back() != back {
            self.reveal_thumb();
        }
        // The link preview, under the pointer where it is now, and the
        // pointer's shape for it: over the scrollbar's strip while it takes
        // a left press, a click scrolls and opens nothing.
        let now = self.timed.clock.host(Instant::now());
        let f = bar_frame(&self.renderer, g.scale);
        let left = self.timed.bar.left_press(&self.term, f, now);
        let pressed = self.seat.hover_press(left);
        let host = crate::link::Host(&self.host);
        let hovered = self.seat.hover(&self.term, mods, g, host);
        if hovered {
            self.pending = Pending::Dirty;
        }
        if hovered || pressed {
            self.show_pointer();
        }
        match owed {
            Owed::Nothing => {}
            Owed::Frame => self.pending = Pending::Dirty,
            Owed::Keys => self.caret.input(Instant::now()),
            // A selection made is SELECT's: the primary, or the clipboard
            // under `?1041`, as xterm's `select-end(SELECT, CUT_BUFFER0)`
            // (note V's V13 copy-on-select is this on Linux).
            Owed::Selected => {
                self.pending = Pending::Dirty;
                let board = board_of(crate::clip::select_board(&self.term));
                self.copy_selection(board);
                self.select_offer = self.window.offering(board).map(|id| (board, id));
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

    /// The cell and grid geometry the seat reads pointer positions in, and
    /// the scale they are read at.
    fn seat_geometry(&self) -> crate::seat::Geometry {
        let m = self.renderer.metrics();
        let (cols, rows) = self.term.geometry();
        crate::seat::Geometry {
            pad: (
                self.renderer.strip().value(),
                self.renderer.grid_top().value(),
            ),
            cell: (f64::from(m.cell_w.value()), f64::from(m.cell_h.value())),
            grid: (
                u16::try_from(cols.index()).unwrap_or(u16::MAX),
                u16::try_from(rows.index()).unwrap_or(u16::MAX),
            ),
            scale: self.scale.factor(),
        }
    }

    /// **The font's pixel size at `points`**: the points times the scale
    /// the frame is drawn at, as the macOS driver sizes its font at the
    /// backing scale.
    fn font_px(&self, points: f32) -> Option<PxSize> {
        PxSize::of(points * self.scale.factor() as f32)
    }

    /// **The modifiers changed**: Ctrl pressed or let go over a link shows
    /// or takes away its preview, the pointer resting.
    fn modifiers(&mut self, mods: baud_domain::Modifiers) {
        let g = self.seat_geometry();
        let host = crate::link::Host(&self.host);
        if self.seat.hover(&self.term, mods, g, host) {
            self.pending = Pending::Dirty;
            self.show_pointer();
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
            Under::Osc8(link) => match link.uri(&self.term) {
                Some(uri) => uri,
                None => {
                    self.report.links.refused.bump();
                    crate::warn!("link refused (gone)");
                    return;
                }
            },
            Under::Bare(bare) => match bare.uri() {
                Ok(uri) => uri,
                Err(why) => {
                    self.report.links.refused.bump();
                    crate::warn!("link refused ({why:?}): {}", shown(bare.text()));
                    return;
                }
            },
        };
        // Each activation counted for the five-second report: opened once
        // `xdg-open` has started, revealed once the reveal is sent over the
        // session bus, and refused otherwise, a failed start and a missing
        // bus included, as the macOS app counts a link the system did not
        // take.
        let said = shown(uri);
        match crate::link::admit(uri, Host(&self.host)) {
            Err(why) => {
                self.report.links.refused.bump();
                crate::warn!("link refused ({why:?}): {said}");
            }
            Ok(t) => match t.action() {
                Action::Open => match self.opener.start(crate::opener::command(t.as_str())) {
                    Ok(()) => {
                        self.report.links.opened.bump();
                        crate::info!("link opened ({:?}): {said}", t.scheme());
                    }
                    Err(e) => {
                        self.report.links.refused.bump();
                        crate::warn!("link not opened: xdg-open: {e}: {said}");
                    }
                },
                Action::Reveal => match self.bus.as_mut() {
                    Some(bus) => {
                        bus.reveal(&crate::opener::local(t.as_str()));
                        self.report.links.revealed.bump();
                        crate::info!("link sent to the file manager: {said}");
                    }
                    None => {
                        self.report.links.refused.bump();
                        crate::warn!("link not revealed: no session bus: {said}");
                    }
                },
            },
        }
    }

    /// **A program's notifications, delivered** (`notify.rs`): each shown,
    /// closed or dropped as `notify::plan` says, and the launcher told of
    /// a change of progress or attention.
    fn deliver(&mut self, posted: Vec<crate::notify::Posted>) {
        use crate::notify::{Focus as F, Plan, Visible};
        let now = Instant::now();
        let focus = match self.focus {
            Focus::Gained => F::Focused,
            Focus::Lost => F::Unfocused,
        };
        for p in posted {
            match crate::notify::plan(
                &p,
                self.notes.setting,
                focus,
                Visible::Unknown,
                &mut self.notes.bucket,
                now,
            ) {
                // iTerm2's picture alone: `Notify` has no summary to give
                // it, the one text argument the text does not call
                // optional.
                Plan::Show if crate::notify::untitled(&p) => {
                    self.notes.bare.0 += 1;
                    self.term.notice_dropped(p.token);
                }
                Plan::Show => match self.bus.as_mut() {
                    Some(bus) => {
                        let replaces = match p.action {
                            baud_core::NoticeAction::Replace => self.notes.shown.id(p.token),
                            _ => None,
                        };
                        let image = self.notes.image_data(&p, &mut self.pictures);
                        let n = crate::bus::notification(&p, replaces, self.caps, image);
                        bus.notify(&n, p.token);
                        self.notes.waiting.push((p.token, p.expiry));
                    }
                    None => self.term.notice_dropped(p.token),
                },
                Plan::Close => {
                    if let (Some(id), Some(bus)) = (self.notes.shown.id(p.token), self.bus.as_mut())
                    {
                        bus.close_notification(id);
                    }
                }
                Plan::Drop(_) => self.term.notice_dropped(p.token),
            }
        }
        // Once a read at most, as on macOS, and once a burst.
        if let Some(line) = self.notes.rate_line() {
            crate::warn!("{line}");
        }
        if let Some(line) = self.notes.bare_line() {
            crate::warn!("{line}");
        }
        if let Some(line) = self.notes.image_line() {
            crate::warn!("{line}");
        }
        let progress = self.notes.progress(&self.term);
        let attention = self.notes.attention(&self.term);
        if progress.is_none() && attention.is_none() {
            return;
        }
        let mut l = self.launcher;
        if let Some(p) = progress {
            l.progress = crate::notify::launcher_progress(p);
        }
        if let Some(a) = attention {
            l.urgent = match a {
                baud_core::Attention::Yes | baud_core::Attention::Once => crate::bus::Truth::True,
                baud_core::Attention::No => crate::bus::Truth::False,
            };
        }
        if l != self.launcher {
            self.launcher = l;
            if let Some(bus) = self.bus.as_mut() {
                bus.launcher(l);
            }
        }
    }

    /// The notifications whose `w` has passed, closed: "guaranteed to be
    /// closed once the expiry time has passed".
    fn expire_notices(&mut self) {
        let due = self.notes.shown.due(Instant::now());
        if let Some(bus) = self.bus.as_mut() {
            for (_, id) in due {
                bus.close_notification(id);
            }
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
                self.scheme = Some(s);
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
            Heard::NoServer(e) => {
                crate::info!(
                    "notifications: no notification server on the session bus ({e}); programs' notifications are not shown"
                );
                self.term.set_notice_support(None);
            }
            Heard::Caps(caps) => {
                self.caps = caps;
                self.term.set_notice_support(crate::notify::support(
                    self.notes.setting,
                    crate::notify::Platform::Linux(caps),
                ));
            }
            Heard::Notified(token, id) => {
                let expiry = self.notes.answered(token);
                self.notes.shown.shown(token, id, expiry, Instant::now());
                crate::reads::input(
                    &mut self.term,
                    |t| t.notice_shown(token),
                    |b| self.outbox.push(b),
                );
            }
            Heard::NotNotified(token, e) => {
                self.notes.answered(token);
                self.term.notice_dropped(token);
                crate::warn!("notification not shown: the notification server answered {e}");
            }
            Heard::Invoked(id, key, activation) => {
                if let Some(token) = self.notes.shown.token(&id) {
                    // "default" is a click on the notification itself, a
                    // number its button, from 1.
                    let button = key
                        .parse::<usize>()
                        .ok()
                        .and_then(|n| n.checked_sub(1))
                        .and_then(baud_core::NoticeButton::of_index);
                    // A click on the body of one that asked focus
                    // activates the window with the token the server
                    // sent for it, where it sent one: the compositor
                    // decides (`notify.rs`, `support`).
                    if let (None, true, Some(t)) =
                        (button, self.term.notice_focuses(token), activation)
                    {
                        let _ = self.window.activate_with(&mut self.conn.sender(), &t);
                    }
                    crate::reads::input(
                        &mut self.term,
                        |t| t.notice_activated(token, button),
                        |b| self.outbox.push(b),
                    );
                }
            }
            Heard::NotifyClosed(id) => {
                if let Some(token) = self.notes.shown.token(&id) {
                    self.notes.shown.gone(token);
                    crate::reads::input(
                        &mut self.term,
                        |t| t.notice_closed(token),
                        |b| self.outbox.push(b),
                    );
                }
            }
            Heard::Revealed => crate::info!("link revealed by the file manager"),
            Heard::NotRevealed(e) => {
                crate::warn!("link not revealed: the file manager answered {e}")
            }
            Heard::Closed(e) => {
                crate::warn!("{e}");
                self.bus = None;
                // Nothing can show a notification now.
                self.term.set_notice_support(None);
            }
        }
    }

    /// **The config's files, followed** (`reload.rs`): what inotify said
    /// since the last turn, read only when the wait said it is `heard`,
    /// and at the settle's end the files read and, when one reads
    /// differently, the config read again.
    fn follow_config(&mut self, heard: Ready) {
        let Some(r) = self.reload.as_mut() else {
            return;
        };
        let now = Instant::now();
        let _ = r.watch.heard_if(heard, now);
        if r.watch.due(now) == crate::reload::Due::Changed {
            self.reload_config(now);
        }
    }

    /// **The system's faces for what no face draws** (`host::follow_gaps`),
    /// as the macOS driver's `follow_gaps` takes them: this driver paints on
    /// this thread, so nothing is reading the atlas.
    fn follow_gaps(&mut self) {
        let Some(px) = self.font_px(self.font_size) else {
            return;
        };
        match crate::host::follow_gaps(&mut self.renderer, &mut self.fonts, px) {
            None => {}
            Some(Ok(given)) => {
                crate::info!(
                    "fonts: a system face added; {}",
                    crate::host::given_back(given)
                );
                self.damage = Damage::Full;
                self.pending = Pending::Dirty;
            }
            Some(Err(e)) => crate::warn!("fonts: a system face did not load: {e}"),
        }
    }

    /// **The config read again, and what it changed applied** (`live.rs`),
    /// as the macOS driver's `reload_config` does.
    fn reload_config(&mut self, now: Instant) {
        let Some(r) = self.reload.as_ref() else {
            return;
        };
        let in_force = r.read.source.clone();
        let reading = match Config::reread(in_force.as_deref()) {
            crate::config::Reread::Read(reading) => reading,
            crate::config::Reread::Broken(sentence) => {
                crate::warn!("config: {sentence}");
                self.show_notice(crate::live::Notice::broken(sentence));
                return;
            }
            crate::config::Reread::Gone(path) => {
                let sentence = crate::config::gone(&path);
                crate::warn!("config: {sentence}");
                self.show_notice(crate::live::Notice::broken(sentence));
                return;
            }
        };
        let crate::config::Reading {
            config,
            mut said,
            watched,
        } = *reading;
        for s in &said {
            crate::warn!("{s}");
        }
        let faces = r.watch.faces_moved();
        let parts = crate::live::between(&r.read, &config, faces);
        // The files the new config names watched before they are read, as
        // the macOS driver does.
        if let Some(r) = self.reload.as_mut() {
            r.watch.rewatch(
                crate::reload::files(&watched.lookup, &watched.themes, &watched.faces),
                now,
            );
        }
        said.extend(self.apply_config(&config, &parts));
        if let Some(line) = crate::live::later(&parts) {
            crate::info!("{line}");
        }
        let off = config.config_reload == crate::config::ConfigReload::Off;
        if let Some(r) = self.reload.as_mut() {
            r.read = config;
        }
        self.show_notice(crate::live::Notice::of(&said));
        if off {
            crate::info!("config: config_reload is off; changes wait for the next launch");
            self.reload = None;
        }
    }

    /// **Each part of a config read again, applied to the window**, as the
    /// macOS driver's `apply_config` does: no padding, scroll mode or
    /// glass here, which this driver does not draw. Returns what it could
    /// not act on.
    fn apply_config(&mut self, c: &Config, parts: &[crate::live::Part]) -> Vec<String> {
        use crate::live::Part;
        let mut said = Vec::new();
        let mut reconfigure = false;
        for &p in parts {
            match p {
                Part::TermOptions => {
                    crate::live::term_options(
                        &mut self.term,
                        c,
                        crate::winops::wayland_abilities(self.bounds_event),
                    );
                    self.clipboard.osc52 = c.clipboard.osc52;
                    // `prompt_at_bottom` turned on at a prompt moves its
                    // rows in the core with no output to draw them.
                    self.damage = Damage::Full;
                }
                Part::Keyboard => self.term.change_set_up(c.set_up),
                Part::Bell => self.bell_style = c.bell,
                Part::Emoji => {
                    self.renderer.set_emoji_default(c.emoji);
                    self.damage = Damage::Full;
                }
                Part::Notifications => {
                    self.notes.setting = c.notifications;
                    if self.caps != crate::notify::ServerCaps::NONE {
                        self.term.set_notice_support(crate::notify::support(
                            c.notifications,
                            crate::notify::Platform::Linux(self.caps),
                        ));
                    }
                }
                Part::CopyOnSelect => {
                    self.clipboard.copy_on_select = c.clipboard.copy_on_select;
                }
                Part::AmbiguousWidth => self.term.set_ambiguous_width(c.ambiguous_width),
                Part::SpacingMarks => self.term.set_spacing_marks(c.join_spacing_marks),
                Part::Splash => {
                    let setting = crate::splash::splash_override(
                        std::env::var("BAUD_SPLASH").ok().as_deref(),
                        c.launch_splash,
                    );
                    if self.timed.set_splash(setting) != crate::timed::Owes::Nothing {
                        self.damage = Damage::Full;
                        self.pending = Pending::Dirty;
                    }
                }
                Part::Trail | Part::Transition => {
                    self.timed.set_trail_scroll(c.trail_scroll);
                    let owes =
                        self.timed
                            .set_config(c.cursor_trail, c.screen_transition, Instant::now());
                    if owes == crate::timed::Owes::Frame {
                        self.pending = Pending::Dirty;
                    }
                }
                Part::CursorStyle => {
                    self.term
                        .reconfigure_cursor_style(baud_core::CursorStyle::of(
                            c.cursor_shape,
                            c.cursor_blink.power_on(),
                        ));
                }
                Part::Caret => {
                    self.caret.set_system(
                        Cycle::of_config(c.blink_interval),
                        Gate::of(c.cursor_blink, NonBlinking::Off),
                    );
                    self.caret.set_stop(c.cursor_stop);
                    self.blink_animation = c.blink_animation;
                    self.caret
                        .set_animation(c.blink_animation.effective(self.motion));
                }
                Part::CursorGuide => self.cursor_guide = c.cursor_guide,
                Part::Badge => self.badge = c.badge,
                Part::WrapWords => {
                    self.renderer.set_word_wrap(c.wrap_words);
                    self.seat.set_word_wrap(c.wrap_words);
                }
                // On their own line, the window having no title bar: its
                // pixels given or taken back now.
                Part::ActivityLights => {
                    let was = self.renderer.lights_height();
                    self.activity_lights = c.activity_lights;
                    self.renderer
                        .set_lights_line(c.activity_lights.line(), self.scale.factor() as f32);
                    self.lights_line(was);
                }
                // The hint goes with the next frame.
                Part::Tearing => self
                    .window
                    .set_tearing(&mut self.conn.sender(), tearing(c.tearing)),
                Part::ImageAnimation => self.timed.set_image_animation(c.image_animation),
                Part::Hollow => {
                    self.unfocused_cursor = c.unfocused_cursor;
                    self.renderer
                        .set_cursor_look(crate::caret::look(self.focus, self.unfocused_cursor));
                }
                Part::StatusMarks => {
                    self.renderer.set_status_marks(c.exit_status);
                    reconfigure = true;
                }
                Part::Palette => said.extend(self.reload_palette(c)),
                Part::Fonts | Part::FontSize => {}
                Part::Padding | Part::Scroll | Part::Reload => {}
                Part::ColourSpace
                | Part::GlassStyle
                | Part::Images
                | Part::Sixel
                | Part::Vs16Width
                | Part::Printing
                | Part::ClipboardArea
                | Part::ShellIntegration => {}
            }
        }
        // A size the user zoomed to stays.
        let points = crate::live::size_after(self.font_size, self.font_base, c.font_size);
        self.font_base = c.font_size;
        if parts.contains(&Part::Fonts) {
            match self.reload_fonts(c, points) {
                Ok(()) => reconfigure = true,
                Err(e) => said.push(format!("config: {e}; the fonts stay as they were")),
            }
        } else if points != self.font_size {
            self.refont(None, points);
        }
        crate::host::tell_font(&mut self.term, &self.fonts, self.font_size, self.font_base);
        if let (true, Some((w, h))) = (reconfigure, self.configured) {
            self.configure(w, h);
        }
        self.damage = Damage::Full;
        self.pending = Pending::Dirty;
        said
    }

    /// **The palette from a config read again**: the theme files read
    /// anew, the half the desktop's scheme gives shown, and the core told
    /// the new defaults with a program's colours kept.
    fn reload_palette(&mut self, c: &Config) -> Vec<String> {
        let (mut halves, said) = crate::palette::load_halves(
            c.palette_light.as_deref(),
            c.palette_dark.as_deref(),
            &crate::palette::Dirs::from_env(c.source.as_deref()),
        );
        halves.selection = c.selection;
        halves.cursor = c.cursor;
        self.halves = halves;
        self.theme_choice = c.theme;
        self.glass = c.glass;
        self.floor = c.floor;
        let half = crate::appearance::half_for(c.theme, self.scheme);
        self.show_half(half, baud_core::Overrides::Keep);
        let theme = self.renderer.theme();
        self.margin = theme.bg.with_alpha(theme.bg_alpha);
        said
    }

    /// **The fonts a config read again names**, leading the chain at
    /// `points`, as the macOS driver's `reload_fonts` does.
    fn reload_fonts(&mut self, c: &Config, points: f32) -> Result<(), String> {
        let resolved = c.clone().resolved();
        let px = self.font_px(points).ok_or("no size")?;
        let given = crate::host::refont(&mut self.renderer, &mut self.fonts, &resolved, px)?;
        crate::info!("{}", crate::host::given_back(given));
        self.font_size = points;
        Ok(())
    }

    /// **The config's notice on its row, or none**, the grid giving the
    /// row up while one is shown.
    fn show_notice(&mut self, notice: crate::live::Notice) {
        if notice == self.notice {
            return;
        }
        let rows_moved = notice.rows() != self.notice.rows();
        self.renderer.set_notice(notice.codepoints());
        self.notice = notice;
        self.damage = Damage::Full;
        self.pending = Pending::Dirty;
        if let (true, Some((w, h))) = (rows_moved, self.configured) {
            self.configure(w, h);
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

    /// **A clipboard, zoom or window key** (`baud_present_wayland::keys::shortcut`):
    /// Ctrl+Shift+C copies the selection to the clipboard, Ctrl+Shift+V
    /// pastes the clipboard, Ctrl+Shift+N opens a new window in this
    /// one's directory (`windows::spawn_window`), and Shift+Insert pastes
    /// SELECT. A copy with
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
            Shortcut::NewWindow => {
                let dir = crate::windows::inherited(
                    self.term.working_directory(),
                    std::slice::from_ref(&self.host),
                    self.derive.dir(),
                );
                crate::windows::spawn_window(dir.as_deref());
            }
            Shortcut::Find => {
                if self.finding.is_none() {
                    self.finding = Some(crate::find::Finding::open(&self.term, Vec::new()));
                }
                self.find_show();
            }
        }
    }

    /// Whose the keys are now: the find field's while it is open.
    fn keys_to(&self) -> crate::pressed::KeysTo {
        match self.finding {
            Some(_) => crate::pressed::KeysTo::FindField,
            None => crate::pressed::KeysTo::Program,
        }
    }

    /// **The keys the program was told are down, let go**: the window lost
    /// the keyboard. Each is sent as its release, which the core sends only
    /// where kitty's event types asked for one.
    fn let_go(&mut self) {
        for e in self.pressed.let_go() {
            let overflows = self.term.reply_overflows;
            let outbox = &mut self.outbox;
            let queued = crate::reads::input(&mut self.term, |t| t.key(e), |b| outbox.push(b));
            if !queued && self.term.reply_overflows != overflows {
                crate::warn!("a key's release was dropped: the reply buffer had no room for it");
            }
        }
    }

    /// A key event where `pressed.rs` sends it.
    fn route_key(&mut self, route: crate::pressed::Route) {
        let k = match route {
            crate::pressed::Route::Field(k) => {
                self.find_key(crate::find::field_key(k.code, k.mods, k.text));
                return;
            }
            crate::pressed::Route::Nobody => return,
            crate::pressed::Route::Program(k) => k,
        };
        // A key snaps the window back to the live screen, where its effect
        // is, and that reveals the thumb.
        if self.seat.key(&mut self.term) {
            self.pending = Pending::Dirty;
            self.reveal_thumb();
        }
        // Into an empty buffer, so it always fits and a `false` is a key
        // that sends nothing (`reads::input`). It was queued behind the
        // read's answers and its `false` ignored, so a key after a
        // reply-heavy read was lost. A full buffer here would be that
        // again, and is said.
        let overflows = self.term.reply_overflows;
        let drawn = self.term.images().generation();
        let outbox = &mut self.outbox;
        let queued = crate::reads::input(&mut self.term, |t| t.key(k), |b| outbox.push(b));
        // An arrow key moved a ReGIS input cursor, or a key ended the input
        // and took it away: a frame.
        if self.term.images().generation() != drawn {
            self.pending = Pending::Dirty;
        }
        if !queued && self.term.reply_overflows != overflows {
            crate::warn!("a key was dropped: the reply buffer had no room for it");
        }
        // A key that sent bytes hides the pointer as XTSMPOINTER says
        // (`hide.rs`).
        if queued {
            let act = self.hiding.key(&self.term);
            self.hide_pointer(act);
            // A key can start a job that prints nothing.
            self.derive.owe();
        }
        // A key shows the cursor, restarts its blink, and is input to its
        // stop.
        self.caret.input(Instant::now());
        // SRM reset: the core drew the key itself, a frame owed now rather
        // than at the next read.
        if self.term.local_echo() {
            self.pending = Pending::Dirty;
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

    /// **What SELECT offered, given up** where its source still offers
    /// it; a copy since, ours or another client's, keeps its own.
    fn give_up_select(&mut self) {
        if let Some((board, source)) = self.select_offer.take() {
            let _ = self.window.give_up(&mut self.conn.sender(), board, source);
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

    /// **The pointer's shape: the hand over a link a click opens, else
    /// the program's `OSC 22`, else the I-beam** (`link::Hover::pointer`,
    /// `baud_present_wayland::pointer::shape`), set once a change while
    /// the pointer is over the window and shown. A hidden pointer stays
    /// hidden: a set shape shows it, so the shape waits for what shows it
    /// (`hide.rs`), which sets it then. Without the cursor-shape global the
    /// window draws the image itself, the I-beam, the hand or the arrow
    /// (`baud_present_wayland::cursor`), and that is said once.
    fn show_pointer(&mut self) {
        if !self.seat.inside() || self.hiding.hidden() == PointerHides::Yes {
            return;
        }
        let want = baud_present_wayland::pointer::shape(
            self.seat.pointer_shape(self.term.pointer_shape()),
        );
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
                        "the compositor offers no cursor-shape global: baud draws its own pointer, an I-beam, a pointing hand or an arrow, and a program's OSC 22 shape is the nearest of the three"
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
            scale: crate::surface::Scale120(self.scale.0),
            grid: crate::width::Grid::of(&self.term),
            status: self.term.status_rows(),
            line: self.renderer.lights_height(),
            marks: self.renderer.status_marks(),
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
            scale: crate::surface::Scale120(self.scale.0),
            grid: crate::width::Grid::of(&self.term),
            status: now,
            line: self.renderer.lights_height(),
            marks: self.renderer.status_marks(),
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
            let wrote = self.pty.write(&mut self.outbox);
            // SD for what the kernel took, CS for what it refused.
            let now = self.timed.clock.host(Instant::now());
            self.lights.wrote(wrote, now);
        }
    }

    /// **The activity lights at this turn**, on their own line: OH, SYN and
    /// FAX read at most once a frame (`own_line::SAMPLE`), the clocks
    /// stepped at the motion preference's pace -- full
    /// glow, the portal having no Dim Flashing Lights -- and a frame owed
    /// when the line shows something else. Nothing is read while the lights
    /// are elsewhere or off.
    fn tick_lights(&mut self) {
        if self.activity_lights.line().is_none() {
            return;
        }
        let now = self.timed.clock.host(Instant::now());
        // OH, SYN and FAX at a frame's pace: the loop turns once a read.
        if self.lights.due(now) == crate::own_line::Sampled::Now {
            let group = crate::fg::group(self.pty.reads().master);
            let front = crate::own_line::in_front(group, self.pty.child());
            self.lights.states(front, &self.term, now);
        }
        let pace = crate::access::lights_pace(self.motion);
        if let Some((shown, glow)) = self.lights.step(now, pace, baud_render::Glow::Full) {
            self.renderer.set_lights(shown, glow);
            self.pending = Pending::Dirty;
        }
    }

    /// **How long the wait may last for the lights**: until the soonest a
    /// light can change, so one goes dark when its hold ends; none while
    /// they hold, or are not on their line.
    fn lights_wait(&self, now: Instant) -> Option<Duration> {
        self.activity_lights.line()?;
        let pace = crate::access::lights_pace(self.motion);
        self.lights.wait(self.timed.clock.host(now), pace)
    }

    /// **The activity lights' own line put up, taken down or resized**
    /// (`[window] activity_lights`), its pixels `was` before: a floating
    /// window grows or shrinks by the difference and its `Configured` lays
    /// the grid out with its rows kept, as the status line's row does;
    /// where the window cannot change size the grid gives the pixels up or
    /// takes them back, laid out again at the size it has.
    fn lights_line(&mut self, was: Px) {
        use crate::winops::Shown;
        if self.renderer.lights_height() == was {
            return;
        }
        self.lights.stale();
        self.damage = Damage::Full;
        self.pending = Pending::Dirty;
        let Some((w, h)) = self.configured else {
            return;
        };
        let m = self.renderer.metrics();
        let at = crate::winops::WaylandWindow {
            size: (w.0, h.0),
            bounds: self.window.bounds().map(|(bw, bh)| (bw.0, bh.0)),
            shown: match self.window.shown() {
                baud_present_wayland::Shown::Floating => Shown::Floating,
                baud_present_wayland::Shown::Maximized => Shown::Maximized,
                baud_present_wayland::Shown::FullScreen => Shown::FullScreen,
            },
            cell: (m.cell_w.value(), m.cell_h.value()),
            scale: crate::surface::Scale120(self.scale.0),
            grid: crate::width::Grid::of(&self.term),
            status: self.term.status_rows(),
            line: self.renderer.lights_height(),
            marks: self.renderer.status_marks(),
        };
        let resized = match at.shown {
            Shown::Floating => {
                let (x, y) = crate::winops::wayland_line_window(at, was).size;
                self.window
                    .resize(baud_present_wayland::Px(x), baud_present_wayland::Px(y))
            }
            Shown::Maximized | Shown::FullScreen => baud_present_wayland::Resize::Bound,
        };
        if resized == baud_present_wayland::Resize::Bound {
            self.configure(w, h);
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
        let Some(px) = self.font_px(points) else {
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

    /// The compositor's size for the window, in the surface's points, or a
    /// new scale to draw it at: the font at the scale's pixels, the grid
    /// that fits the frame, the core and the renderer resized to that, a
    /// fresh frame at the frame's size, and the child told.
    fn configure(&mut self, width: baud_present_wayland::Px, height: baud_present_wayland::Px) {
        self.configured = Some((width, height));
        // **A new scale is a new pixel size for the font**, its size in
        // points kept, as the macOS driver's `resized` takes a new backing
        // scale: the renderer rescaled, and the points `OSC 1337 ;
        // ReportCellSize` answers in follow it.
        let scale = self.window.scale();
        if scale != self.scale {
            self.scale = scale;
            match self.font_px(self.font_size) {
                Some(px) if self.renderer.rescale(px).is_some() => {}
                _ => crate::warn!(
                    "the font did not rescale to {} pixels a point; it stays as it was",
                    scale.factor()
                ),
            }
            self.term
                .set_backing_scale(baud_domain::BackingScale::of(scale.factor()));
            crate::info!("scale: {} pixels a point", scale.factor());
        }
        // The frame: the surface's points at the scale, rounded halfway
        // away from zero, as the window's buffer is (`Window::buffer_size`).
        let (width, height) = (scale.buffer(width), scale.buffer(height));
        let m = self.renderer.metrics();
        // The core advances the cursor past a Sixel by cell rows: give it the
        // height. Set on every configure, which is where a cell size could
        // change (a font or scale change would arrive as one).
        self.term
            .set_cell_size(cell_px(m.cell_w), cell_px(m.cell_h));
        // XTWINOPS's window size, `CSI 14 ; 2 t`: the surface the
        // compositor configured, which is the whole window, in the frame's
        // pixels, as the macOS driver reports its window in device pixels
        // -- nothing here asks for server-side decorations, so none frame
        // it. The rest is not known: xdg-shell tells a client neither its
        // position nor whether it is minimized, and the outputs the window
        // crate binds say only their scale, so those reports stay silent
        // rather than guessed.
        self.term.set_window(baud_core::WindowFacts {
            window: Some(baud_core::PixelSize {
                width: baud_core::ImagePx::of(u16::try_from(width.0).unwrap_or(u16::MAX)),
                height: baud_core::ImagePx::of(u16::try_from(height.0).unwrap_or(u16::MAX)),
            }),
            ..baud_core::WindowFacts::UNKNOWN
        });
        // The exit-status strip, when kept, takes its column first, and the
        // activity lights' own line its pixels at this scale, no row.
        self.renderer
            .set_lights_line(self.activity_lights.line(), scale.factor() as f32);
        let text_w = width.0.saturating_sub(self.renderer.strip().value());
        let text_h = height
            .0
            .saturating_sub(self.renderer.lights_height().value());
        let cols = (text_w / m.cell_w.value().max(1)).clamp(1, u32::from(MAX_GRID.ws_col));
        let rows = (text_h / m.cell_h.value().max(1)).clamp(1, u32::from(MAX_GRID.ws_row));
        // The status line's row is the window's, below the grid: the grid
        // is what is left, and the child is told it (`Term::status_rows`).
        let ws = crate::width::less_status(
            WinSize {
                ws_col: u16::try_from(cols).unwrap_or(u16::MAX),
                ws_row: u16::try_from(rows).unwrap_or(u16::MAX),
            },
            Rows::of_count(self.term.status_rows().index() + self.renderer.notice_rows().index()),
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
        // **The derived title's check, once a frame and only when owed**
        // (`title::Derive`): the compositor's frame events pace it, as the
        // display link paces macOS's. Linux has no unprivileged fork
        // event, so a job that prints nothing is seen by the key that
        // started it and the reads around it.
        if self.derive.check(&self.pty.reads()) == crate::title::Changed::Yes {
            self.send_title();
        }
        // Each hold is timed from the opening that began it (`UpdateClock`).
        let update = self.updates.observe(&self.term, Instant::now());
        if update == Update::Honoured {
            // Honoured: what the update changes of `?5` is its to hide.
            self.bell.hold(&self.term);
            return;
        }
        // The moment the frame is first seen: the next refresh where the
        // compositor's presentation feedback says one, else now -- what
        // the macOS frame plans for with its display link's target.
        let seen_at = self.window.next_shown(Instant::now());
        // The visual bell, before the margin: the flash moves it too. Kept
        // only if the frame reaches the compositor, below.
        let bell = self.bell.plan(&self.term, seen_at, update);
        self.renderer.set_flash(bell.flash);
        self.renderer.set_bell_tint(bell.tint);
        let blinks = self.blinks();
        let caret = self.caret.plan(seen_at, blinks);
        self.renderer.set_cursor_blink(caret);
        // The selection's share of the view, or `?1001`'s region.
        self.renderer.set_highlight(self.seat.highlight(&self.term));
        // iTerm2's cursor guide: what a program set, else `[cursor] guide`,
        // its one point at the scale's pixels.
        let scale = self.scale.factor();
        let guide = self.term.cursor_guide().unwrap_or(self.cursor_guide);
        self.renderer
            .set_cursor_guide(crate::effects::guide_look(guide, scale));
        // A `selection` the badge names reads what a copy would copy.
        let selection = || self.seat.selected_text(&self.term);
        let page = self.renderer.background_of(&self.term);
        let badge = crate::effects::badge_look(
            &self.term,
            self.badge,
            &self.session,
            &selection,
            page,
            scale,
        );
        self.renderer.set_badge(badge);
        // The find field and every match's mark, on the window drawn, and
        // the input method's composition in the field while it is open.
        let look = self.finding.as_ref().map(|f| {
            f.look_typing(
                &self.term,
                self.term.window_back(),
                &self.composing,
                &self.composing_hints,
                self.composing_caret,
            )
        });
        let field = look.as_ref().map(|l| l.field.clone());
        self.renderer.set_find(&self.term, look);
        // The link preview, read again where the pointer is while Ctrl is
        // held: a read may have changed what is under it.
        let host = crate::link::Host(&self.host);
        let mods = self.window.modifiers();
        if self.seat.hover_refresh(&self.term, mods, host) {
            self.show_pointer();
        }
        self.renderer.set_link(self.seat.hover_look().cloned());
        // The input method's composition at the live cursor, and where
        // its candidate window opens -- at the input method's own cursor
        // in it, as drawn -- sent when it moved; while it hides its
        // cursor, the last stands. While the find field is open both are
        // the field's: the composition in it, the window at its caret.
        let m = self.renderer.metrics();
        let (composition, rect) = match &field {
            Some(f) => (None, Some(crate::marked::field_cell(f, m.cell_w, m.cell_h))),
            None => {
                let c = crate::marked::composition(
                    &self.term,
                    &self.composing,
                    &self.composing_hints,
                    self.composing_caret,
                );
                let r = crate::marked::candidate_rect(&self.term, c.as_ref(), m.cell_w, m.cell_h);
                (c, r)
            }
        };
        self.renderer.set_composition(composition);
        // In the surface's points, "surface local coordinates": the grid
        // begins past the exit-status strip, and the frame's pixels are
        // the scale's a point (`surface.rs`).
        let rect = rect.map(|r| {
            let r = crate::marked::in_frame(r, (self.renderer.strip(), self.renderer.grid_top()));
            crate::surface::Scale120(self.scale.0).cover(r)
        });
        let point = |p: crate::surface::Points| baud_present_wayland::Px(p.0);
        if let Some([x, y, w, h]) = rect {
            self.window.set_text_cursor(
                &mut self.conn.sender(),
                baud_present_wayland::CursorRect {
                    x: point(x),
                    y: point(y),
                    width: point(w),
                    height: point(h),
                },
            );
        }
        // The thumb, the trail, the fade and the blink phases at this
        // moment (`timed.rs`), in the macOS frame's order.
        let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let cell = crate::timed::Cell {
            px: (side(m.cell_w), side(m.cell_h)),
            height: baud_render::Points(side(m.cell_h) / scale as f32),
        };
        let renderer = &self.renderer;
        let plan = self.timed.plan(
            seen_at,
            &self.term,
            |r, c, style| renderer.cursor_rect(r, c, style),
            cell,
            crate::reveal::widths(crate::reveal::Scale(scale)),
        );
        self.renderer.set_scrollbar(plan.scroll);
        // The thumb this frame draws decides who a left press goes to, so
        // the pointer over a link under its strip is read again with it.
        let now = self.timed.clock.host(seen_at);
        let left = self
            .timed
            .bar
            .left_press(&self.term, bar_frame(&self.renderer, scale), now);
        if self.seat.hover_press(left) {
            self.show_pointer();
        }
        self.renderer.set_screen_fade(plan.fade);
        self.renderer.set_blink(plan.blink);
        self.renderer.set_trail(plan.trail);
        self.renderer.set_splash(plan.splash);
        let shows: Vec<_> = plan.fireworks.iter().flatten().copied().collect();
        self.renderer.set_fireworks(&shows);
        let margin = self.renderer.clear_color_of(&self.term);
        if margin != self.margin {
            self.margin = margin;
            self.frame.fill(margin.0);
            self.damage = Damage::Full;
        }
        // The marks on the window shown, and the whole frame they are
        // drawn in: the strip at its left, then the grid.
        self.renderer.place_marks(
            &self.term,
            self.term.window_back(),
            baud_render::Leaving::None,
            Px::of(0),
        );
        let (w, h) = self.renderer.whole_size();
        let Some(mut target) = Target::new(&mut self.frame, self.stride, w, h) else {
            return;
        };
        // A frame the driver repaints whole -- a new size, the margin
        // refilled -- changed pixels past the grid's rows too, which no
        // band the renderer paints covers: the whole buffer is damaged.
        let whole = self.damage;
        // The rows the window shows, `window_back` lines up the history
        // while the wheel or the thumb holds it there: the rows the
        // selection, the link under a click and the thumb already read.
        let back = self.term.window_back();
        let painted = self
            .renderer
            .render_at(&mut self.term, back, &mut target, self.damage);
        self.report.drawn(painted);
        // The order each row was drawn in, which the pointer maps back
        // through.
        self.seat.drawn(self.renderer.bidi());
        self.damage = Damage::Incremental;
        self.strips.clear();
        let px = |p: Px| baud_present_wayland::Px(p.value());
        self.strips.extend(
            self.renderer
                .painted()
                .map(|b| baud_present_wayland::Strip {
                    top: px(b.top),
                    bottom: px(b.bottom),
                }),
        );
        let pitch = usize::try_from(self.stride.value()).unwrap_or(0);
        // A sliding frame: the grid's rows composed from the live frame and
        // the kept one, the margin below them the live frame's; it and the
        // frame after it go whole.
        let slid = match plan.slide {
            Some(s) if self.kept.len() == self.frame.len() => {
                self.composed.clone_from(&self.frame);
                // The frame's own rows, not the activity lights' line, which
                // holds still as the margin does.
                let rows = self.renderer.frame_size().1;
                let span = crate::width::slide_span(
                    self.renderer.grid_top(),
                    rows,
                    self.stride,
                    &self.frame,
                );
                let o = baud_render::slide_offsets(s, rows);
                match baud_render::compose_slide(
                    &self.frame[span.clone()],
                    &self.kept[span.clone()],
                    &mut self.composed[span],
                    self.stride,
                    rows,
                    o,
                ) {
                    true => crate::effects::Composed::Yes,
                    false => crate::effects::Composed::No,
                }
            }
            _ => {
                // Settled: the two frames go, so an idle window holds
                // nothing for the slide.
                if plan.slide.is_none() {
                    self.kept = Vec::new();
                    self.composed = Vec::new();
                }
                crate::effects::Composed::No
            }
        };
        let changed = match (whole, slid, self.slid) {
            (Damage::Full, _, _)
            | (_, crate::effects::Composed::Yes, _)
            | (_, _, crate::effects::Composed::Yes) => Changed::Whole,
            (Damage::Incremental, crate::effects::Composed::No, crate::effects::Composed::No) => {
                Changed::Strips(&self.strips)
            }
        };
        self.slid = slid;
        let shown = match slid {
            crate::effects::Composed::Yes => &self.composed,
            crate::effects::Composed::No => &self.frame,
        };
        // The blink rates on screen, which bound the next wait.
        self.timed
            .drawn(self.renderer.blink_seen(), self.term.regis_blink());
        // A frame a timed effect owed is drawn for the moment it is seen,
        // and held to it.
        if self.frame_for == FrameFor::Moment {
            self.window.draw_for(seen_at);
        }
        if self
            .window
            .present(&mut self.conn.sender(), shown, pitch, changed)
            == Presented::Yes
        {
            startup_first("first present", &FIRST_PRESENT);
            self.report.presented();
            self.bell.commit(bell);
            self.caret.commit(caret);
            self.pending = Pending::Clean;
            self.frame_for = FrameFor::Soonest;
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
