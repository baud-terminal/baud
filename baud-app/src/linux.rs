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
//! 250 ms a phase until the desktop's settings are read).
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
use crate::pty::{Outbox, Pty, Ready, wait};
use baud_core::{Focus, Owned, PointerHides, Resized, Term};
use baud_domain::{Cols, DecodeProgress, Rows, WinSize};
use baud_font::Px;
use baud_present_wayland::{
    Board, BufferScale, Changed, Copied, Event, PointerEvent, Presented, Shaped, Window,
};
use baud_render::{Argb, Blend, Damage, PxSize, Renderer};
use baud_wayland::Connection;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

/// How long a synchronised update may hold the frame: two 60 Hz periods,
/// the bound the macOS loop keeps for the same reason -- long enough that
/// no real redraw is cut off, short enough that a stalled update is not a
/// frozen window.
const SYNC_MAX: Duration = Duration::from_millis(33);

/// How long the launch waits on the compositor for its globals and its
/// outputs: two round trips, each well under a millisecond on the same
/// machine and a network round trip apiece to a remote compositor, so only a
/// compositor that has stopped answering reaches it.
const LAUNCH_ANSWER: Duration = Duration::from_secs(10);

/// The grid before the compositor has said how large the window is.
const FIRST: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

/// Whose size the first window takes where its first configure leaves the
/// size to the client: the grid `--grid` asked for, or the window's own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstSize {
    Grid,
    Compositor,
}

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

/// Whether the window holds events already read ([`Window::has_events`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    Events,
    Nothing,
}

impl Held {
    fn of(w: &Window) -> Held {
        match w.has_events() {
            true => Held::Events,
            false => Held::Nothing,
        }
    }
}

/// **How long the loop's wait may last**: the soonest of `deadlines`, and
/// none at all while the window holds events already read, as the
/// launch's round trips read the first configure. Those are in the
/// window's queue, not on the socket, so the wait would not wake for them
/// and the first frame would wait for whatever woke it next.
fn wait_bound(deadlines: &[Option<Duration>], held: Held) -> Option<Duration> {
    let queued = match held {
        Held::Events => Some(Duration::ZERO),
        Held::Nothing => None,
    };
    deadlines.iter().copied().chain([queued]).flatten().min()
}

/// `[window] tearing`, as the window takes it.
fn tearing(t: crate::config::Tearing) -> baud_present_wayland::Tearing {
    match t {
        crate::config::Tearing::On => baud_present_wayland::Tearing::On,
        crate::config::Tearing::Off => baud_present_wayland::Tearing::Off,
    }
}

/// **An effect a handler asked for, run on the loop's next turn**: the
/// printed path's reveal over the session bus, its `[link.path]`
/// command, its copy. Nothing a handler does reaches the OS from inside
/// it; `Driver::run_later` performs each after `dispatch` returns, as
/// the macOS driver's main-queue turn does, and says what became of it.
enum Later {
    Reveal(String),
    Command(std::process::Command),
    Copy(String),
    /// A new baud running a link's window, in a folder, at the clicking
    /// window's grid (`windows::running_window`).
    Window(
        crate::rules::Run,
        Option<std::path::PathBuf>,
        crate::run_arg::Grid,
    ),
}

/// What a read was taken with: the writers sampled for it and whether it
/// carried a link (`probe::Probes::read_taken`).
type Taken = (crate::probe::Writers, crate::probe::ReadLinked);

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
    /// The gutter around the frame on every side, in points (`origin.rs`).
    padding: f32,
    /// Where the frame was last placed in the surface (`origin.rs`): every
    /// conversion between the surface and the frame adds it. Set only by
    /// [`Driver::place`].
    origin: crate::origin::Origin,
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
    /// Who was last read in front, for OH on the lights' line and on DEC's
    /// line alike: one read a frame (`own_line::FrontRead`).
    front: crate::own_line::FrontRead,
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
    /// deadline, which bounds the wait (`reads::paced`), each with what its
    /// read was taken with, so it is filed under its own verdict when fed.
    pace: crate::reads::Pace<(crate::probe::Writers, crate::probe::ReadLinked)>,
    /// The frames a screen switch's slide draws through, held only while
    /// it runs, and the surface's size (`SlideFrames`). The window copies
    /// each slide frame whole, and draws the frame after whole
    /// (`Window::draw_cpu`).
    slides: SlideFrames,
    /// Whether the slide's live frame holds the last frame drawn, or
    /// frames drawn in place since left it behind.
    frame_holds: FrameHolds,
    /// What the last lent draws changed, for the rows each buffer the
    /// window lends misses.
    since: Since,
    /// The frame's row pitch: the window's width.
    stride: Px,
    /// The default background at the window's opacity, for the margin
    /// past the last whole cell: the theme's, or `OSC 11`'s, or under `?5`
    /// the foreground. A change here draws the frame whole.
    margin: Argb,
    pending: Pending,
    /// What the frame owed is drawn for, until a frame is presented.
    frame_for: FrameFor,
    damage: Damage,
    /// The rows the last frame painted, as the window is told them: what
    /// it copies and damages (`Window::present`), or premultiplies in
    /// place (`Window::draw_cpu`).
    strips: Vec<baud_present_wayland::Strip>,
    /// The pixel rows the last frame drawn in place changed of the one
    /// before, which the compositor is told.
    changed: Vec<baud_present_wayland::Strip>,
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
    /// Whether `[font] dpi = "physical"` falling back to the desktop's
    /// point has been said.
    unmeasured: Said,
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
    /// The GPU arm, which draws a frame before the CPU arm is asked
    /// (`gpu_arm.rs`).
    gpu: crate::gpu_arm::GpuArm,
    /// When effect pacing last logged a change of divisor.
    paced_said: Option<Instant>,
    /// Whether the CPU arm's last build drew into the atlas, which effect
    /// pacing leaves out of its average.
    cpu_cold: baud_render::Cold,
    /// When the GPU arm last logged an effect frame's GPU times.
    gpu_said: Option<Instant>,
    /// The session bus, for the portal's appearance and the file
    /// manager's reveal; `None` once it is gone or was never there.
    bus: Option<crate::bus::Bus>,
    /// `[screen] printing`, the jobs taken from the core, and the print
    /// portal's dialog: one at a time (`print.rs`).
    printing: crate::config::Printing,
    prints: crate::print::Prints,
    print_dialog: PrintDialog,
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
    /// The desktop's two answers the preference is made of, as last heard:
    /// the portal's `reduced-motion` and `enable-animations`.
    heard_motion: (crate::bus::Motion, crate::bus::Animations),
    blink_animation: crate::config::BlinkAnimation,
    /// `[cursor] guide`: the cursor guide while no program has set it.
    cursor_guide: baud_domain::CursorGuide,
    /// `[screen] badge`: whether a program's badge is drawn.
    badge: crate::config::BadgeShown,
    /// `[window] status_line_lights`: whether DEC's indicator line also
    /// carries the state lights as words (`line_lamps.rs`).
    status_line_lights: baud_render::StatusLineLights,
    /// The hover text over DEC's line's words (`line_tip.rs`).
    line_tip: crate::line_tip::LineTip,
    /// What a copy says, and when it ends (`copied.rs`).
    copied: crate::copied::Signal<Instant>,
    /// The lines at rest the frame draws, and what reads them (`lined.rs`).
    lines: crate::lined::Lines,
    /// The view's link slots, which the scan and the link preview read.
    views: crate::lined::ViewLinks,
    uris: crate::lined::Uris,
    /// The worker that asks whether each printed path exists (`probe.rs`);
    /// the paths themselves are the seat's, which its pointer reads. The
    /// worker wakes the loop's wait through `woken` when it answers.
    probes: crate::probe::Probes<Instant>,
    woken: Option<crate::probe::Woken>,
    /// What a kitty file medium's reading thread wakes the loop's wait
    /// with (`reads::hold_kitty_files`).
    files_woken: Option<crate::probe::Woken>,
    /// When a read asks which program wrote it (`probe::Sampler`).
    groups: crate::probe::Sampler,
    link_config: crate::config::Link,
    /// The printed path a click asked about again, and the `:line:col`
    /// printed after it, for a `[link.path]` command when the answer comes.
    path_click: Option<(Vec<u8>, crate::paths::Asked)>,
    /// What the badge's variables read of the session that the core does
    /// not keep (`effects::Session`), brought up to date with the title.
    session: crate::effects::Session,
    /// The `xdg-open`s a link started, and the commands a `[link.*]` rule
    /// ran, reaped on each turn, and this machine's name for a
    /// `file://host/` link.
    opener: crate::opener::Opener,
    /// What a handler asked of the OS this turn, run at the top of the
    /// loop's next one, after `dispatch` has returned ([`Later`]).
    later: crate::next_turn::NextTurn<Later>,
    host: String,
    /// The notifications programs post, delivered over the session bus
    /// (`notify.rs`), and what the notification server said it supports.
    notes: crate::notify::Delivery<crate::bus::NotifyId>,
    /// The reader of the file iTerm2's `image` names, with every check
    /// kitty's file medium makes, off this thread: each piece and the end
    /// wake the loop's wait through `woken`, and the turn after the paths'
    /// takes them (`pictures_turn`).
    pictures: crate::kitty_files::Pictures,
    caps: baud_domain::FdoCaps,
    /// What the launcher was last told: a program's progress and its
    /// request for attention.
    launcher: baud_domain::LauncherEntry,
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
    /// What a point is here (`typesize.rs`): `[font] dpi` and what the
    /// portal said of the desktop's text scale. Typographic sizes read it
    /// through [`Driver::type_px`]; the backing scale never does.
    type_size: crate::typesize::TypeSnapshot,
    /// The five-second report (`report.rs`): the loop's turns, the frames
    /// committed to the compositor, what the last frame cost and the links
    /// activated, logged at `info` from the end of each turn, its deadline
    /// a bound on the wait while the log shows it.
    report: crate::report::Polled<Instant>,
    /// The keystroke instrument (`latency.rs`), on under `BAUD_KEYLAT=1`,
    /// and the last presentation it was told of: on Linux a key is shown
    /// when `wp_presentation` says its frame turned into light.
    latency: crate::latency::Latency,
    /// **The buffer each turn drains the master into** (`Pty::drain_into`),
    /// kept at `READ_BATCH` so a turn allocates nothing to read.
    read_buf: Vec<u8>,
    /// What the last read left of the window's title, so a read that
    /// changed none of it builds no title (`title::ReadTitle`).
    read_title: crate::title::ReadTitle,
    last_presented: Option<baud_present_wayland::Stamp>,
    /// The presented-frame log, on under `BAUD_PRESENTED=1`
    /// ([`PresentedLog`]).
    presented_log: PresentedLog,
    /// Whether a link's window is held open after its program failed
    /// (`held.rs`), with its line, until a key closes it.
    holding: crate::held::Holding,
    /// Whether a key in a held window asked the window to close.
    quit: Quit,
}

/// **The config's watch** (`reload.rs`), and the config the window last
/// took, as read, which a new reading is compared with (`live::between`).
struct Reloading {
    watch: crate::reload::Watch<Instant>,
    read: Config,
}

/// **The scrollbar's share of a pointer event** (`bar.rs`), after DEC's
/// locator: at the position the seat last had for a press or a release,
/// a move a drag while a button is down, its points `g.scale` pixels each,
/// the grid's corner in the surface at `g.pad`.
fn thumb(
    timed: &mut crate::timed::Timed,
    renderer: &Renderer<'_, '_>,
    pending: &mut Pending,
    term: &mut Term<'_>,
    beside: crate::seat::Beside,
    g: crate::seat::Geometry,
) -> crate::bar::Took {
    let pad = (f64::from(g.pad.0.value()), f64::from(g.pad.1.value()));
    let Some(e) = crate::bar::event_beside(beside, pad, g.scale) else {
        return crate::bar::Took::No;
    };
    let scale = g.scale;
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

/// Whether the window's loop ends at the end of this turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Quit {
    No,
    Yes,
}

/// Whether a one-time line has been logged.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Said {
    Not,
    Once,
}

/// Where a point's fallback is said from (`Driver::say_fallback`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Retyped {
    /// A text scale or an output size heard, and the point sized again.
    Heard,
    /// A configure, which runs on every resize.
    Configured,
}

pub fn run(given: Option<(crate::rules::Run, Option<crate::run_arg::Grid>)>) {
    // `baud --grid COLSxROWS -e`: a link's window opens at the grid of the
    // window it was asked from, within the grid baud provisions; else the
    // grid before the compositor has said how large the window is.
    let (given, first, sized) = match given {
        Some((run, Some(g))) => (Some(run), g.within(&MAX_GRID), FirstSize::Grid),
        Some((run, None)) => (Some(run), FIRST, FirstSize::Compositor),
        None => (None, FIRST, FirstSize::Compositor),
    };
    // **The default config, before the config is read**: written once when
    // none is anywhere baud looks (`default_config.rs`), so the watch below
    // takes it as the file in force. A new window is a new baud, which
    // finds it there.
    crate::default_config::at_launch();
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
    let mut notice_said = launched.said;
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
    // **The outputs, before the storage is provisioned**: the registry's
    // round trip, and where it offered an output a second one after the
    // binds, which that output's `mode`, `scale` and `done` precede
    // (`Window::outputs_settled`). The provision is still `MAX_GRID`.
    let mut conn = match Connection::connect() {
        Ok(c) => c,
        Err(e) => {
            crate::warn!("{}", crate::log::no_wayland_session(e));
            std::process::exit(1);
        }
    };
    let mut window = match Window::open(&mut conn, "baud", "org.baud.terminal") {
        Ok(w) => w,
        Err(e) => {
            crate::warn!("wayland: {e:?}");
            std::process::exit(1);
        }
    };
    let answer_by = Instant::now() + LAUNCH_ANSWER;
    while !window.outputs_settled() {
        match conn.roundtrip_until(&mut window, answer_by) {
            Ok(_) => {}
            Err(baud_wayland::client::Error::TimedOut) => {
                crate::warn!(
                    "wayland: the compositor did not answer within {} s",
                    LAUNCH_ANSWER.as_secs()
                );
                std::process::exit(1);
            }
            Err(e) => {
                crate::warn!("wayland: {e:?}");
                std::process::exit(1);
            }
        }
    }
    for o in window.outputs() {
        crate::info!(
            "output: {} x {} px, scale {}",
            o.width.0,
            o.height.0,
            o.scale
        );
    }
    // **The window exported** (`xdg-foreign-unstable-v2`), so a print
    // portal's dialog opens on it: its handle comes with the next events
    // and names the dialog's parent (`print_parent`).
    window.export(&mut conn.sender());
    startup_mark("wayland");
    let geom = host_geometry();
    crate::info!("cache: {}", describe(&geom));
    let owned: &'static mut Owned = Box::leak(Box::new(
        Owned::with_geometry(&MAX_GRID, &geom)
            .with_images(cfg.images)
            .with_clipboard(cfg.clipboard.budget)
            .with_printer(crate::print::budget(cfg.printing))
            .with_notices(crate::notify::BUDGET),
    ));
    let mut term = Term::new(owned.storage(), &first);
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
    let mut bus = match crate::bus::Bus::connect() {
        Ok(b) => Some(b),
        Err(e) => {
            crate::info!("{e}: theme = \"auto\" draws dark, and a file: link is not revealed");
            None
        }
    };
    // **The printer is the print portal**: asked for now, and listed to the
    // core when it answers (`print.rs`); until then, and with no bus, a
    // print is "no printer".
    if let (Some(bus), crate::config::Printing::On) = (bus.as_mut(), cfg.printing) {
        bus.ask_print_portal();
    }
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
    // A glass raised so its text reads is said once, on the notice's row
    // and the startup line.
    for s in crate::palette::glass_raised(&cfg.glass, cfg.floor, &halves) {
        crate::info!("{s}");
        crate::host::startup_said(&s);
        notice_said.push(s);
    }
    let notice = crate::live::Notice::of(&notice_said);
    // The theme `half_theme` builds is the one macOS builds for the same
    // half, dim included: `[glass]`'s material has no counterpart here,
    // and the dim follows the half drawn. Only the backdrop differs: with
    // no material modelled, the floor reads text against black to white.
    let theme = crate::palette::half_theme(half, &cfg.glass, cfg.floor, &halves);
    let margin = theme.bg.with_alpha(theme.bg_alpha);
    // **A point at the desktop's size** (`typesize.rs`): until the portal
    // answers, the desktop's 96 dpi at scale 1.
    let type_size = crate::typesize::TypeSnapshot {
        mode: cfg.font_dpi,
        ..crate::typesize::TypeSnapshot::UNTOLD
    };
    let type_px = crate::typesize::px_per_point(type_size, 1.0).0 as f32;
    let mut renderer = Renderer::new(
        crate::host::stack(&fonts),
        PxSize::of(cfg.font_size * type_px).expect("size"),
        theme,
        Blend::Native,
        Cols::of_index(usize::from(first.ws_col)),
        Rows::of_index(usize::from(first.ws_row)),
    )
    .expect("cell metrics");
    crate::host::set_up_renderer(&mut renderer, &cfg, &notice);
    // The activity lights' own line, at a point's pixels until the first
    // configure sets the scale's (`configure`).
    renderer.set_lights_line(cfg.activity_lights.line(), type_px);
    // The window starts without the keyboard, until `wl_keyboard.enter`.
    renderer.set_cursor_look(crate::caret::look(Focus::Lost, cfg.unfocused_cursor));
    let term = crate::host::set_up_term(term, &cfg, &renderer, &fonts);
    // **A window asked for a grid opens at it** (`--grid`): the surface
    // that grid fills at the first cell, by the arithmetic a program's
    // resize in cells takes (`winops::wayland`). The first configure may
    // have come already, during the outputs' round trips: with no size yet
    // the window prefers this one for a configure that leaves the size to
    // the client; with one, a floating window takes it as a program's
    // resize would, and a tiled, maximised or full-screen one keeps the
    // compositor's.
    if sized == FirstSize::Grid {
        use crate::winops::{Shown, WaylandAction, WaylandWindow};
        let m = renderer.metrics();
        let at = WaylandWindow {
            size: (0, 0),
            bounds: None,
            shown: Shown::Floating,
            cell: (m.cell_w.value(), m.cell_h.value()),
            scale: crate::surface::Scale120(window.scale().0),
            grid: crate::width::Grid::of(&term),
            status: term.status_rows(),
            line: renderer.lights_height(),
            marks: renderer.status_marks(),
            pad: crate::origin::gutter(
                cfg.padding,
                crate::typesize::px_per_point(type_size, 1.0).0,
            ),
        };
        let ask = baud_core::WindowRequest::ResizeCells {
            rows: baud_core::SizeAsk::Exactly(first.ws_row),
            cols: baud_core::SizeAsk::Exactly(first.ws_col),
        };
        if let WaylandAction::Resize(w, h) = crate::winops::wayland(ask, &at) {
            let (w, h) = (baud_present_wayland::Px(w), baud_present_wayland::Px(h));
            match window.size() {
                None => window.prefer(w, h),
                Some(_) => {
                    let _ = window.resize(w, h);
                }
            }
        }
    }
    startup_mark("renderer");
    let pty = Pty::spawn_polled(
        &first,
        renderer.metrics().cell_w,
        renderer.metrics().cell_h,
        // `baud -e`'s program in place of the shell, when it was given.
        &crate::pty::ShellStart {
            run: given,
            ..crate::pty::ShellStart::of(&cfg)
        },
    )
    .expect("pty");
    let mut term = term;
    // The token baud handed the shell is the one each mark is read against
    // (`ran::ShellToken::hash`), before any of its bytes is fed.
    term.set_shell_token(pty.shell_token_hash());
    // A window the system gave no terminal says why (`pty::Refused`).
    crate::pty::say_refused(&mut term, &pty);
    startup_mark("pty");
    // The link sniff, which finds the shell's `D` by the token baud handed it.
    let groups = crate::probe::Sampler::for_shell(pty.shell_token());
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
    // The printed paths' worker writes a byte here after each answer, which
    // the loop's one wait hears (`probe::Woken`); with no pair, an answer is
    // read at the next turn the wait has. A notification's picture read
    // writes to the same pair at each piece and at its end, and is taken
    // after the paths' turn drains it (`pictures_turn`); with no pair, at
    // the next turn, or its deadline's.
    let (woken, mut probes, picture_wake) = match crate::probe::Woken::pair() {
        Ok((w, wake)) => {
            let shared = std::sync::Mutex::new(wake.clone());
            let pictures: crate::kitty_files::Wake = std::sync::Arc::new(move || {
                if let Ok(wake) = shared.lock() {
                    wake();
                }
            });
            (Some(w), crate::probe::Probes::system(wake), Some(pictures))
        }
        Err(e) => {
            crate::warn!("paths: no wake for the file checks ({e}); their answers wait for a turn");
            (None, crate::probe::Probes::system(|| {}), None)
        }
    };
    // No kind of link is asked about here (`Link::programs`): the list is
    // the `[link.*]` rules' commands alone.
    probes.set_programs(cfg.link.programs());
    // **A kitty file medium is read off the loop's thread**, which a pair
    // of its own wakes at each piece and at the read's end; the turn drains
    // it before the pieces are taken (`feed_released`). With no pair, the
    // files are read at the terminator by the reader lent above.
    let mut pace = crate::reads::Pace::new();
    let files_woken = match crate::probe::Woken::pair() {
        Ok((w, wake)) => {
            crate::reads::hold_kitty_files(&mut term, &mut pace, std::sync::Arc::new(wake));
            Some(w)
        }
        Err(e) => {
            crate::warn!(
                "kitty graphics: no wake for a file read ({e}); files are read in the feed"
            );
            None
        }
    };
    let mut d = Driver {
        bell,
        ringer: crate::bell::Ringer::new(&term),
        bell_style: cfg.bell,
        // The config's interval, else 250 ms a phase, and no switch: `auto`
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
        padding: cfg.padding,
        origin: crate::origin::Origin::CORNER,
        configured: None,
        scale: BufferScale::ONE,
        status_rows: Rows::ZERO,
        activity_lights: cfg.activity_lights,
        lights: crate::own_line::LineLights::new(),
        front: crate::own_line::FrontRead::new(),
        pty,
        outbox: Outbox::default(),
        hostlog: crate::hostlog::HostLog::new(crate::pty::start_dir()),
        widths,
        pace,
        slides: SlideFrames::new(),
        frame_holds: FrameHolds::Stale,
        since: Since::NONE,
        stride: Px::of(0),
        margin,
        pending: Pending::Clean,
        frame_for: FrameFor::Soonest,
        damage: Damage::Full,
        strips: Vec::new(),
        changed: Vec::new(),
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
            seat.set_at_rest(cfg.link.at_rest());
            seat.set_place(cfg.message.link.place);
            // `HOME` read once, here, and this machine's one name.
            use std::os::unix::ffi::OsStringExt;
            let home = std::env::var_os("HOME").map(std::ffi::OsString::into_vec);
            seat.paths = crate::paths::Paths::new(home, cfg.link.rules.forms());
            seat.paths.set_rules(cfg.link.rules.clone());
            seat.paths.set(cfg.link.paths_read());
            let _ = seat.paths.set_missing(cfg.link.missing_read());
            let _ = seat.paths.set_join(cfg.link.join_read());
            seat
        },
        shape: None,
        hiding,
        shapeless: Said::Not,
        clips,
        clipboard: cfg.clipboard,
        uncopied: Said::Not,
        unmeasured: Said::Not,
        select_offer: None,
        composing: Vec::new(),
        composing_hints: Vec::new(),
        finding: None,
        pressed: crate::pressed::Pressed::default(),
        composing_caret: baud_render::Caret::End,
        // `BAUD_GPU=off` keeps every frame on the CPU arm, read once.
        gpu: crate::gpu_arm::GpuArm::new(std::env::var("BAUD_GPU").ok().as_deref()),
        paced_said: None,
        cpu_cold: baud_render::Cold::No,
        gpu_said: None,
        timed: crate::timed::Timed::new(Instant::now(), cfg.cursor_trail, cfg.screen_transition)
            .with_trail_scroll(cfg.trail_scroll)
            // Every window's, a new window's process as much as the
            // launch's (`splash.rs`).
            .with_splash(crate::splash::splash_override(
                std::env::var("BAUD_SPLASH").ok().as_deref(),
                cfg.launch_splash,
            )),
        bus,
        printing: cfg.printing,
        prints: crate::print::Prints::new(),
        print_dialog: PrintDialog::Idle,
        theme_choice: cfg.theme,
        half,
        halves,
        glass: cfg.glass,
        floor: cfg.floor,
        contrast: crate::access::Contrast::Standard,
        motion_source,
        motion: crate::config::SystemMotion::Full,
        heard_motion: (crate::bus::Motion::NoPreference, crate::bus::Animations::On),
        blink_animation: cfg.blink_animation,
        cursor_guide: cfg.cursor_guide,
        badge: cfg.badge,
        status_line_lights: cfg.status_line_lights,
        line_tip: crate::line_tip::LineTip::default(),
        copied: crate::copied::Signal::new(cfg.message),
        lines: crate::lined::Lines::default(),
        views: crate::lined::ViewLinks::default(),
        uris: {
            let mut u = crate::lined::Uris::default();
            u.set_rules(cfg.link.rules.clone());
            for told in cfg.link.rules.told() {
                crate::info!("{told}");
            }
            u
        },
        probes,
        woken,
        files_woken,
        groups,
        link_config: cfg.link.clone(),
        path_click: None,
        session: crate::effects::Session::here(cfg.shell_program.as_deref()),
        opener: crate::opener::Opener::default(),
        later: crate::next_turn::NextTurn::default(),
        host: crate::opener::host_name(),
        notes,
        pictures: crate::kitty_files::Pictures::new(picture_wake),
        caps: baud_domain::FdoCaps::Unasked,
        launcher: baud_domain::LauncherEntry::IDLE,
        reload,
        notice,
        scheme: None,
        type_size,
        report: crate::report::Polled::new(Instant::now(), crate::report::Logged::now()),
        latency: crate::latency::Latency::from_env(),
        read_buf: Vec::with_capacity(crate::pty::READ_BATCH),
        read_title: crate::title::ReadTitle::new(),
        last_presented: None,
        presented_log: PresentedLog::from_env(),
        holding: crate::held::Holding::No,
        quit: Quit::No,
    };
    // This machine's name, one value for the links and the paths.
    let names = crate::link::MachineNames::one(&d.host);
    crate::link::hand_names(names, &mut d.uris, &mut d.seat.paths);
    d.timed.set_image_animation(cfg.image_animation);
    if let crate::access::MotionSource::Fixed(m) = motion_source {
        d.set_motion(m);
    }
    if let Err(e) = d.conn.flush() {
        crate::warn!("wayland: {e}");
        return;
    }
    'run: loop {
        // What the last turn's handlers asked of the OS, with none of them
        // on the stack.
        d.run_later();
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
        // The end of a copy's words or pulse, the same way (`copied.rs`).
        let copied = d
            .copied
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
        // does with `expire_timeout`, on the clock the notifications are
        // timed on.
        let host = d.timed.clock.host(now);
        let expiry = d
            .notes
            .slots
            .next_due()
            .map(|t| t.since(host).unwrap_or(Duration::ZERO));
        // A notification's picture read, given up at its deadline
        // (`Delivery::picture_due`).
        let pictures = d
            .notes
            .picture_due()
            .map(|t| t.since(host).unwrap_or(Duration::ZERO));
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
        // OH read again in DEC's 80 ms while its line shows the words.
        let words = d.line_lamps_wait(now);
        // The printed paths' next batch, once one is owed, and the worker's
        // hang, while a batch is out (`probe::Probes::deadline`).
        let paths = d
            .probes
            .deadline(&d.seat.paths, now)
            .map(|t| t.saturating_duration_since(now));
        // With no pidfd, the one more look a hung-up master's child is owed.
        let reap = d.pty.reap_owed().map(|t| t.saturating_duration_since(now));
        // A new device after one was lost (`gpu_fallback.rs`).
        let restart = d.gpu.deadline().map(|t| t.saturating_duration_since(now));
        // The deadlines, one list (`own_line::wait_bounds` reads it), and
        // the window's held events over them (`wait_bound`).
        let timeout = [
            sync, repeat, flash, copied, caret, decode, paste, delay, effects, frames, expiry,
            pictures, settle, report, lights, words, paths, reap, restart,
        ];
        let timeout = wait_bound(&timeout, Held::of(&d.window));
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
        // The printed paths' worker, which writes when it answers.
        if let Some(w) = &d.woken {
            transfers.push((w.as_fd(), crate::pty::Interest::Read));
        }
        // A kitty file read's thread, which writes at each piece and at
        // the read's end.
        if let Some(w) = &d.files_woken {
            transfers.push((w.as_fd(), crate::pty::Interest::Read));
        }
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
        // The child, as its pidfd, until it is reaped: readable once it has
        // ended (`held.rs`).
        let child = d.pty.child_fd().map(|fd| {
            transfers.push((fd, crate::pty::Interest::Read));
            transfers.len() - 1
        });
        // The GPU arm's pipeline builds: one finished owes a frame.
        let built = d.gpu.ready_fd().map(|fd| {
            transfers.push((fd, crate::pty::Interest::Read));
            transfers.len() - 1
        });
        // The GPU arm's frame in flight: its end, readable once finished,
        // frees what it reads and owes a frame asked for meanwhile.
        let flown = d.gpu.done_fd().map(|fd| {
            transfers.push((fd, crate::pty::Interest::Read));
            transfers.len() - 1
        });
        // The master until it hangs up, after which it would answer every
        // wait at once (`Pty::polled_master`).
        let waited = wait(
            d.conn.as_fd(),
            d.pty.polled_master(),
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
            // **The master drained in one turn** (`Pty::drain_into`): every
            // read it has, up to `READ_BATCH`, into the buffer the driver
            // keeps, and the work a feed does done once for all of them.
            // The keystroke instrument's echo is stamped at the first read,
            // and the clock read for it only while the instrument holds a
            // keystroke's record open.
            let mut stamp = match d.latency.on() == crate::latency::Switch::On
                && d.latency.open().value() > 0
            {
                true => crate::pty::Stamp::Asked,
                false => crate::pty::Stamp::Unasked,
            };
            let mut bytes = std::mem::take(&mut d.read_buf);
            let batch = d.pty.drain_into(&mut bytes, &mut stamp);
            match batch {
                // The child let go of its terminal: the master leaves the
                // poll, and the window decides below once the child is
                // reaped too (`held.rs`).
                crate::pty::Batch::Closed => d.pty.hung_up(),
                // A wake that found nothing to read: the paced and released
                // reads are `feed_released`'s, below, every turn.
                crate::pty::Batch::Bytes(0) => {}
                crate::pty::Batch::Bytes(_) => {
                    // Which program wrote it, asked at the read: it may be
                    // gone by the batch (`Probes::read_taken`), and a read
                    // carrying a link always asks.
                    let master = d.pty.reads().master;
                    // No process watch here: a job that writes and ends
                    // between two reads is the residual race
                    // (`probe::Writers`).
                    let (writers, linked) = match d.groups.read(Instant::now(), &bytes) {
                        crate::probe::GroupAsk::Yes(l) => (
                            d.groups.writers(
                                master.and_then(crate::fg::group),
                                crate::probe::Watched::Nothing,
                            ),
                            l,
                        ),
                        crate::probe::GroupAsk::No => {
                            (d.groups.unasked(), crate::probe::ReadLinked::No)
                        }
                    };
                    if let crate::pty::Stamp::At(at) = stamp {
                        d.latency.echoed([at]);
                    }
                    let began = d.seat.cursor_on(&d.term);
                    let took = d.feed(&bytes, (writers, linked));
                    d.latency.fed(crate::pty::host_now);
                    let ended = d.seat.cursor_on(&d.term);
                    // Filed under the reads this feed fed, a held one's own
                    // when its bytes go on, and none still held.
                    let lines = d.seat.counts().core_lines(&d.term);
                    d.probes.drain_taken(
                        &mut d.seat.paths,
                        &d.term,
                        (began, ended),
                        lines,
                        took.iter().copied(),
                        Instant::now(),
                    );
                }
            }
            d.read_buf = bytes;
        }
        // A GPU pipeline built: the frame it held back is owed, though
        // nothing else changed.
        if built.and_then(|i| extra.get(i).copied()) == Some(Ready::Readable) && d.gpu.woken() {
            d.pending = Pending::Dirty;
        }
        // The GPU arm's frame landed: a frame asked for while it flew is owed.
        if flown.and_then(|i| extra.get(i).copied()) == Some(Ready::Readable)
            && d.gpu.landed() == baud_present_vulkan::present::Owes::Frame
        {
            d.pending = Pending::Dirty;
        }
        // The child's end, by its pidfd, or with none the one more look the
        // hangup owes (`Pty::reap_owed`); and the window's decision once
        // both its ends are in, in either order.
        if child.and_then(|i| extra.get(i).copied()) == Some(Ready::Readable) {
            d.pty.reap();
        }
        if d.pty.reap_owed().is_some_and(|t| Instant::now() >= t) {
            d.pty.give_up();
        }
        if d.ended() == Quit::Yes || d.quit == Quit::Yes {
            break 'run;
        }
        // A ReGIS delay's held reads, at its deadline, read or not, a
        // one-shot input's once it has ended, and a kitty file read's pieces.
        d.feed_released();
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
        // The printed paths' answers, never waited for, and their next batch
        // when it is due.
        d.paths_turn();
        // The notifications' picture reads, after the paths' turn drained
        // the wake they share, so a piece that woke the wait is taken now.
        d.pictures_turn();
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
        if d.copied.is_due(at) {
            d.pending = Pending::Dirty;
        }
        // Nothing while a synchronised update holds the frame
        // (`Caret::tick`).
        let blinks = d.blinks();
        let update = d.updates.state(&d.term, Instant::now());
        if d.caret.tick(at, blinks, update) == Owes::Frame {
            d.pending = Pending::Dirty;
        }
        // The refresh effect pacing weighs a frame against (`pacer.rs`).
        d.timed.set_period(d.window.frame_interval());
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
                // Shift+Page Up and Shift+Page Down page the main screen's
                // history (`history_keys.rs`), settled now: the held reads
                // that may go on are fed first, and reads a wait still
                // holds leave the key the program's. A press taken here is
                // not remembered, so its release goes to nobody.
                Event::Key(k) => {
                    let history = match d.keys_to() {
                        crate::pressed::KeysTo::Program => {
                            d.feed_released();
                            let undecoded = match d.pace.holds() {
                                true => crate::history_keys::Undecoded::Held,
                                false => crate::history_keys::Undecoded::None,
                            };
                            crate::history_keys::decide(&k, d.term.screen(), undecoded)
                        }
                        crate::pressed::KeysTo::FindField => None,
                    };
                    match history {
                        Some(p) => d.page_history(p),
                        None => {
                            let route = d.pressed.key(k, d.keys_to());
                            d.route_key(route);
                        }
                    }
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
                        if d.caret
                            .input(Instant::now(), crate::caret::Input::FocusGain)
                            == crate::caret::Owes::Frame
                        {
                            d.pending = Pending::Dirty;
                        }
                        // Back from another app: what is struck is asked
                        // again.
                        d.seat.paths.recheck();
                        d.probes.owe_programs();
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
                        .map(|h| {
                            (
                                crate::marked::HintAt(h.start.0),
                                crate::marked::HintAt(h.end.0),
                                hint_of(h.hint),
                            )
                        })
                        .collect();
                    let hints = crate::marked::hints(&p.text, &ranges);
                    let cursor = (
                        crate::marked::CaretAt(p.cursor.begin.0),
                        crate::marked::CaretAt(p.cursor.end.0),
                    );
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
        // The output's size under `[font] dpi = "physical"`, after the
        // events: a `wl_output.geometry` past the last configure, or the
        // surface entering another output.
        d.follow_output();
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
        // DEC's line, after this turn's reads and writes (`line_lamps.rs`).
        d.show_line_lamps();
        d.present();
        // The frames the compositor has shown up to now, for the instrument.
        d.presentation_tick();
        d.log_presented();
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
            // The GPU arm's failures and restarts, once one moved
            // (`gpu_fallback.rs`).
            if let Some(line) = d.gpu.counts().line() {
                crate::info!("{line}");
            }
        }
    }
    // Every frame the window drew, by the arm that drew it (`gpu_arm.rs`).
    d.gpu.exit();
    // What the window copied into shared memory, on its own line.
    crate::info!("{}", ShmCopies(d.window.copies()));
    // The notifications still waiting for their pictures, posted with
    // their text alone (`Delivery::linux_closing`).
    d.pictures_at_close();
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
                Some(mine.as_fd()),
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

    /// **One read's worth of the child's output into the core**, `tag` what
    /// it was taken with, and the reads fed -- held ones when their bytes go
    /// on (`reads::paced`), each with its own tag, for the caller to file.
    fn feed(&mut self, bytes: &[u8], tag: Taken) -> Vec<Taken> {
        if bytes.is_empty() && !self.pace.due(Instant::now()) && !self.pace.released(&self.term) {
            return Vec::new();
        }
        let mut took = Vec::new();
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
        // A program's copies this read, said once after it (`copied.rs`).
        let mut copies = crate::copied::Batch::default();
        let mut asked = Vec::new();
        // A program's font changes, performed after the read (`fonts.rs`).
        let mut fonts_asked = Vec::new();
        // The watches on the selection's lines, armed before the read
        // (`keep::Before::watched`).
        let before = self.seat.before_read(&mut self.term);
        // A program's notifications, taken read by read: the core keeps
        // eight a read (`notify.rs`).
        let icons = &mut self.notes.icons;
        // A program's print jobs, taken part by part: the core keeps
        // `PRINT_QUEUE` of them (`print.rs`).
        let prints = &mut self.prints;
        let mut posted = Vec::new();
        // A reply the core queued while it read, which nobody typed: AA.
        let mut answered = Answered::No;
        crate::reads::paced(
            &mut self.term,
            pace,
            Instant::now(),
            [(bytes, tag)],
            |b| {
                if !b.is_empty() {
                    answered = Answered::Yes;
                }
                self.outbox.push(b);
            },
            |t, _| {
                if let Some((d, warn)) = clips.poll(t, osc52) {
                    if let Some(line) = warn {
                        crate::warn!("{line}");
                    }
                    let text = match d {
                        crate::clip::Decision::Set(text) => Some(text),
                        _ => None,
                    };
                    match program_copy(window, conn, t, d) {
                        Copied::Set => {
                            if let Some(text) = text {
                                copies.copied(text);
                            }
                        }
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
                prints.take(t);
                t.clear_notices();
            },
            &mut self.hostlog,
            &mut took,
        );
        // Before any key is encoded: a key `Term::key` sends as ISO 8859-1
        // must reach a line discipline whose `IUTF8` is clear.
        self.pty.follow(self.term.input_encoding());
        // The jobs this read finished, onto the print portal's dialog.
        self.print_next();
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
        if self.copied.program(copies, Instant::now()) == crate::copied::Owes::Frame {
            self.pending = Pending::Dirty;
        }
        // The selection after the read: renumbered, or dropped where the
        // read wrote over it, switched or reset (`seat.rs`). A switch takes
        // the trail away and fades the new screen in; a reset that brought
        // a looked-back window live reveals the thumb.
        let last = took.iter().rev().find_map(|(w, _)| w.record);
        let after = self.seat.after_read(&self.term, before, last.as_ref());
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
                // The slide's first frame is drawn whole: its live frame
                // may be new.
                let _ = self.slides.start(self.margin);
                self.frame_holds = FrameHolds::Stale;
                self.frame_from_window();
                let whole = self.renderer.whole_size();
                let (renderer, term) = (&mut self.renderer, &self.term);
                let _ = crate::origin::in_frame(
                    &mut self.slides.frame,
                    self.stride,
                    self.origin,
                    whole,
                    |target| renderer.render_hidden(term, target),
                );
                self.slides.kept.clone_from(&self.slides.frame);
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
        // The read owes the derived title a check, made at the next frame.
        self.derive.owe();
        // **The title once for each change, not once a read**
        // (`title::ReadTitle`): a read changes the program's title, its
        // encoding, the printer's run or the folder `OSC 7` names, and the
        // frame's check finds a change of the folder and program a derived
        // title names.
        let changed = self.read_title.changed(
            self.term.title(),
            self.term.title_encoding(),
            self.term.print_run(),
            self.term.working_directory(),
        );
        if changed == crate::title::Changed::Yes {
            self.send_title();
        }
        took
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
            |term, beside| thumb(timed, renderer, pending, term, beside, g),
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
        let host = self.uris.names();
        let views = self.views.fresh(&self.term, self.seat.scrolled());
        let hovered = self.seat.hover(&self.term, mods, g, host, views.as_ref());
        if hovered {
            self.pending = Pending::Dirty;
        }
        // The hover text over DEC's line's words and the lights' own line's
        // lights, where the pointer is now, the line's row included
        // (`line_tip.rs`).
        let over = self.seat.line_over(g, self.term.status_rows());
        let at = self.seat.frame_point(g.scale);
        self.line_tip.set_lights(self.lights_drawn());
        if self.line_tip.update_at(&self.term, over, at) == crate::line_tip::Changed::Yes {
            self.pending = Pending::Dirty;
        }
        if hovered || pressed {
            self.show_pointer();
        }
        match owed {
            Owed::Nothing => {}
            Owed::Frame => self.pending = Pending::Dirty,
            Owed::Keys => {
                if self
                    .caret
                    .input(Instant::now(), crate::caret::Input::WheelKeys)
                    == crate::caret::Owes::Frame
                {
                    self.pending = Pending::Dirty;
                }
            }
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
                if let Some((link, line)) = self.seat.take_link() {
                    self.open_link(&link, line);
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
        crate::seat::Geometry {
            pad: (
                Px::of(self.origin.x.value() + self.renderer.strip().value()),
                Px::of(self.origin.y.value() + self.renderer.grid_top().value()),
            ),
            cell: (f64::from(m.cell_w.value()), f64::from(m.cell_h.value())),
            grid: crate::width::Grid::of(&self.term),
            scale: self.scale.factor(),
        }
    }

    /// **The frame's pixels to one point** (`typesize.rs`): the desktop's
    /// point -- 96 dpi times its text scale, or what `[font] dpi` says --
    /// at the scale the frame is drawn at. Typographic sizes only: the
    /// backing scale the core reports in stays the compositor's.
    fn type_px(&self) -> f64 {
        crate::typesize::px_per_point(self.type_size, self.scale.factor()).0
    }

    /// **The padding's gutter** in the frame's pixels: `padding` points at
    /// the desktop's pixels to a point (`origin.rs`).
    fn gutter(&self) -> Px {
        crate::origin::gutter(self.padding, self.type_px())
    }

    /// **Places the frame in the surface** (`origin.rs`): the gutter, then
    /// what the surface leaves beyond the frame split between opposite
    /// sides. A frame that moved is drawn whole, the renderer painting the
    /// gutter and the leftover around it. The only writer of `origin`.
    fn place(&mut self) {
        let surface = self.slides.surface();
        let at = crate::origin::origin(surface, self.renderer.whole_size(), self.gutter());
        if at != self.origin {
            self.origin = at;
            self.damage = Damage::Full;
        }
    }

    /// **The font's pixel size at `points`**: the points at the desktop's
    /// pixels to a point ([`Driver::type_px`]).
    fn font_px(&self, points: f32) -> Option<PxSize> {
        PxSize::of(points * self.type_px() as f32)
    }

    /// **The modifiers changed**: Ctrl pressed or let go over a link shows
    /// or takes away its preview, the pointer resting.
    fn modifiers(&mut self, mods: baud_domain::Modifiers) {
        let g = self.seat_geometry();
        let host = self.uris.names();
        let views = self.views.fresh(&self.term, self.seat.scrolled());
        if self.seat.hover(&self.term, mods, g, host, views.as_ref()) {
            self.pending = Pending::Dirty;
            self.show_pointer();
        }
    }

    /// The grid the terminal has: what a link's window opens at.
    fn grid(&self) -> crate::run_arg::Grid {
        crate::windows::grid_of(&self.term)
    }

    /// **A link to the desktop, or refused and said why**, the macOS
    /// driver's rules (`link::admit`): what it opens goes to `xdg-open`,
    /// one argument and no shell (`opener.rs`), and a `file:` link, which
    /// opening could launch, is revealed by the file manager over the
    /// session bus, or refused where there is no bus.
    fn open_link(&mut self, link: &crate::link::Under, line: baud_render::AbsLine) {
        use crate::link::ClickOn;
        let shown = |uri: &[u8]| {
            uri.iter()
                .take(80)
                .copied()
                .collect::<Vec<u8>>()
                .escape_ascii()
                .to_string()
        };
        // Each activation counted for the five-second report: opened once
        // `xdg-open` has started, revealed once the reveal is sent over the
        // session bus, and refused otherwise, a failed start and a missing
        // bus included, as the macOS app counts a link the system did not
        // take. The one decision both drivers make (`link::click_on`): the
        // link takes its origin from the rows it covers, as the worker named
        // who wrote them; a remote program's `file://` link opens through
        // the ssh session the worker read for the group that printed it.
        let decided = crate::link::click_on(
            link,
            &self.term,
            self.uris.names(),
            self.uris.rules(),
            &self.seat.paths,
        );
        match decided {
            // Asked again off the frame, and revealed or said on a later turn.
            ClickOn::Path(p) => self.check_path(p),
            // A remote program's file link: a new baud running `ssh` to the
            // session's machine with `-e`, in this window's folder, started
            // through the opener on the loop's next turn (`Later`), as a
            // rule's window is.
            ClickOn::Remote(Ok(run), _) => {
                let dir = crate::windows::inherited(
                    self.term.working_directory(),
                    std::slice::from_ref(&self.host),
                    self.derive.dir(),
                );
                self.later.push(Later::Window(run, dir, self.grid()));
            }
            ClickOn::Remote(Err(why), uri) => {
                self.report.links.refused.bump();
                crate::warn!("link refused ({why:?}): {}", shown(uri));
            }
            ClickOn::Gone => {
                self.report.links.refused.bump();
                crate::warn!("link refused (gone)");
            }
            ClickOn::Refused(why, text) | ClickOn::Unreadable(why, text) => {
                self.report.links.refused.bump();
                crate::warn!("link refused ({why:?}): {}", shown(text));
            }
            ClickOn::Admitted(t, uri) => {
                let said = shown(uri);
                use crate::link::Click;
                match crate::link::click(&t, &self.seat.paths) {
                    // A file the worker found gone, asked about again as a
                    // printed path's click is -- after a rule's copy or
                    // command, which is the link's own action (`link::click`).
                    Click::Again(path) => {
                        self.path_click = Some((path.clone(), crate::paths::Asked::Link));
                        self.check_file(crate::paths::Candidate {
                            path,
                            form: crate::paths::Form::Absolute,
                            line,
                        });
                    }
                    // A rule's `copy`: the link's text to the clipboard, said as
                    // a copy of a selection is; nothing is opened.
                    Click::Copy => {
                        let text = t.as_str().to_string();
                        self.link_copied(text);
                    }
                    // A command of yours: no shell, null streams, reaped on
                    // each turn as `xdg-open` is.
                    Click::Command => {
                        let built = t
                            .rule()
                            .and_then(|ix| self.uris.rules().command(ix, &t.handed()));
                        match built.map(|b| b.map(|cmd| self.opener.start(cmd))) {
                            Some(Ok(Ok(()))) => {
                                self.report.links.opened.bump();
                                crate::info!("link handed to a [link.*] command: {said}");
                            }
                            Some(Ok(Err(e))) => {
                                self.report.links.refused.bump();
                                crate::warn!("link not sent: [link.*] command: {e}: {said}");
                            }
                            Some(Err(why)) => {
                                self.report.links.refused.bump();
                                crate::warn!("link refused ({why:?}): {said}");
                            }
                            None => {
                                self.report.links.refused.bump();
                                crate::warn!("link refused (no command): {said}");
                            }
                        }
                    }
                    // A window of yours: a new baud running the rule's argv
                    // with `-e`, in this window's folder, started through the
                    // opener on the loop's next turn (`Later`).
                    Click::Window => {
                        let built = t
                            .rule()
                            .and_then(|ix| self.uris.rules().window(ix, &t.handed()));
                        match built {
                            Some(Ok(run)) => {
                                let dir = crate::windows::inherited(
                                    self.term.working_directory(),
                                    std::slice::from_ref(&self.host),
                                    self.derive.dir(),
                                );
                                self.later.push(Later::Window(run, dir, self.grid()));
                            }
                            Some(Err(why)) => {
                                self.report.links.refused.bump();
                                crate::warn!("link refused ({why:?}): {said}");
                            }
                            None => {
                                self.report.links.refused.bump();
                                crate::warn!("link refused (no command): {said}");
                            }
                        }
                    }
                    Click::Open => match self.opener.start(crate::opener::command(&t.handed())) {
                        Ok(()) => {
                            self.report.links.opened.bump();
                            crate::info!("link opened ({:?}): {said}", t.scheme());
                        }
                        Err(e) => {
                            self.report.links.refused.bump();
                            crate::warn!("link not opened: xdg-open: {e}: {said}");
                        }
                    },
                    Click::Reveal => match self.bus.as_mut() {
                        Some(bus) => {
                            bus.reveal(t.reveal_uri().as_str());
                            self.report.links.revealed.bump();
                            crate::info!("link sent to the file manager: {said}");
                        }
                        None => {
                            self.report.links.refused.bump();
                            crate::warn!("link not revealed: no session bus: {said}");
                        }
                    },
                }
            }
        }
    }

    /// **A program's notifications, delivered** (`notify.rs`): each shown,
    /// closed or dropped as `baud_domain::notice_plan` says, and the
    /// launcher told of a change of progress or attention.
    fn deliver(&mut self, posted: Vec<crate::notify::Posted>) {
        use baud_domain::{LinuxSight, NoticePlan};
        let now = self.timed.clock.host(Instant::now());
        let sight = match self.focus {
            Focus::Gained => LinuxSight::Focused,
            Focus::Lost => LinuxSight::Unfocused,
        };
        for p in posted {
            let plan = baud_domain::notice_plan(
                p.action,
                p.occasion,
                self.notes.setting,
                sight,
                &mut self.notes.bucket,
                now,
            );
            match plan {
                // iTerm2's picture alone: `Notify` has no summary to give
                // it, the one text argument the text does not call
                // optional.
                NoticePlan::Show if crate::notify::untitled(&p) => {
                    self.notes.bare.add();
                    self.term.notice_dropped(p.token);
                }
                // Parked in the order it came while a picture is read off
                // this thread, its own or one ahead of it's
                // (`Delivery::linux`).
                NoticePlan::Show => match self.bus.is_some() {
                    true => {
                        let ready = self.notes.linux(p, &mut self.pictures, now);
                        self.notify(ready);
                    }
                    false => self.term.notice_dropped(p.token),
                },
                NoticePlan::Close => {
                    if let (Some(&id), Some(bus)) =
                        (self.notes.slots.id(p.token), self.bus.as_mut())
                    {
                        bus.close_notification(id);
                    }
                }
                NoticePlan::Drop(_) => self.term.notice_dropped(p.token),
            }
        }
        // Once a read at most, as on macOS, and once a burst.
        for e in self.notes.lines() {
            crate::notify::apply(&mut self.term, e);
        }
        let progress = self.notes.progress(&self.term);
        let attention = self.notes.attention(&self.term);
        if let (Some(l), Some(bus)) = (self.launcher.heard(progress, attention), self.bus.as_mut())
        {
            bus.launcher(crate::bus::Launcher::of(l));
        }
    }

    /// **Notifications posted on the session bus**, each with the
    /// `image-data` it sends, the server's answer awaited; with no bus, or
    /// not sent, dropped, as a refusal is: an update not sent leaves the
    /// one shown as it was.
    fn notify(&mut self, ready: Vec<crate::notify::LinuxPost>) {
        for (p, image) in ready {
            let Some(bus) = self.bus.as_mut() else {
                self.term.notice_dropped(p.token);
                continue;
            };
            let replaces = match p.action {
                baud_core::NoticeAction::Replace => self.notes.slots.id(p.token).copied(),
                _ => None,
            };
            let n = crate::bus::notification(&p, replaces, self.caps.known(), image);
            let sent = bus.notify(&n, p.token);
            let buttons = baud_domain::NoticeButtonSet::of(p.buttons.iter().map(|(n, _)| *n));
            self.notes.slots.posting(p.token, p.expiry, buttons);
            if let Err(e) = sent {
                self.notes.slots.refused(p.token);
                self.term.notice_dropped(p.token);
                crate::warn!("notification not shown: not sent, as {e}");
            }
        }
    }

    /// **The notifications' picture reads, on each turn of the loop**: what
    /// they brought taken, never waiting, and each notification ready at
    /// the front posted in order -- one whose read ended, one past its
    /// deadline with its text alone, and the ones that waited behind it
    /// (`Delivery::linux_reads`).
    fn pictures_turn(&mut self) {
        let ready = self
            .notes
            .linux_reads(self.timed.clock.host(Instant::now()));
        self.notify(ready);
        if let Some(line) = self.notes.image_line() {
            crate::warn!("{}", crate::notify::line_text(line));
        }
    }

    /// The notifications still waiting for their pictures when the window
    /// closes, posted with their text alone; nothing waits for a read.
    fn pictures_at_close(&mut self) {
        let ready = self.notes.linux_closing();
        self.notify(ready);
    }

    /// The notifications whose `w` has passed, closed: "guaranteed to be
    /// closed once the expiry time has passed". With no deadline held the
    /// clock is not read.
    fn expire_notices(&mut self) {
        let timed = &self.timed;
        let due = self.notes.slots.due(|| timed.clock.host(Instant::now()));
        if let Some(bus) = self.bus.as_mut() {
            let slots = &self.notes.slots;
            for i in due {
                if let Some(&id) = slots.token_at(i).and_then(|t| slots.id(t)) {
                    bus.close_notification(id);
                }
            }
        }
    }

    /// **The printed paths' worker, on each turn of the loop**: what it woke
    /// the wait with drained, its answers merged, its hang watched and a
    /// batch sent once one is due (`probe.rs`). A click's second look, once
    /// answered, reveals or says why not.
    fn paths_turn(&mut self) {
        if let Some(w) = &self.woken {
            w.drain();
        }
        let master = self.pty.reads().master;
        let seen = crate::probe::Seen {
            term: &self.term,
            scrolled: self.seat.scrolled(),
            lines: &mut self.lines,
            uris: &mut self.uris,
            urls: self.link_config.urls_read(),
            paths: &mut self.seat.paths,
        };
        let turn = self
            .probes
            .turn(Instant::now(), seen, || master.and_then(crate::fg::group));
        self.report.links.checks = self.probes.counts;
        if turn.frame == crate::paths::Owed::Yes {
            self.pending = Pending::Dirty;
        }
        if let Some(done) = turn.clicked {
            self.path_checked(done);
        }
    }

    /// **A click on a printed path**: asked again of the worker, off the
    /// frame, and answered on a later turn, or now with no worker to ask.
    fn check_path(&mut self, p: &crate::link::PrintedPath) {
        self.path_click = Some((
            p.path().to_vec(),
            crate::paths::Asked::Printed(p.line_col().clone()),
        ));
        self.check_file(crate::paths::Candidate {
            path: p.path().to_vec(),
            form: p.form(),
            line: p.line(),
        });
    }

    /// **A click on a file, asked again of the worker**: a printed path, or
    /// an `OSC 8` `file://` link the worker found gone.
    fn check_file(&mut self, c: crate::paths::Candidate) {
        let master = self.pty.reads().master;
        let seen = crate::probe::Seen {
            term: &self.term,
            scrolled: self.seat.scrolled(),
            lines: &mut self.lines,
            uris: &mut self.uris,
            urls: self.link_config.urls_read(),
            paths: &mut self.seat.paths,
        };
        if let Some(done) =
            self.probes
                .click(Instant::now(), seen, master.and_then(crate::fg::group), c)
        {
            self.path_checked(done);
        }
    }

    /// **What a click on a printed path comes to**: found again, its
    /// `file://` URI through `admit`, unchanged, and shown by the file
    /// manager over the session bus, never launched; gone, or not checked,
    /// the bar says so. No path is logged.
    fn path_checked(&mut self, done: crate::probe::Clicked) {
        use crate::copied::PathNote;
        use crate::link::Action;
        use crate::paths::Found;
        let clicked = self.path_click.take();
        let note = match done {
            crate::probe::Clicked::Exists(path) => {
                let asked = crate::paths::asked_for(clicked, &path);
                let does =
                    crate::paths::on_found(&asked, &path, self.uris.names(), self.uris.rules());
                // Each of the three on the loop's next turn (`Later`).
                if let Found::Copy = does {
                    // The worker's bytes moved, copied only when they are
                    // not UTF-8.
                    let text = String::from_utf8(path)
                        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                    self.later.push(Later::Copy(text));
                    return;
                }
                if let Found::Window(built) = does {
                    match built {
                        // In the file's own folder.
                        Ok(run) => {
                            use std::os::unix::ffi::OsStrExt;
                            let dir = std::path::Path::new(std::ffi::OsStr::from_bytes(&path))
                                .parent()
                                .map(std::path::Path::to_path_buf);
                            self.later.push(Later::Window(run, dir, self.grid()));
                        }
                        Err(why) => {
                            self.report.links.refused.bump();
                            crate::warn!("printed path refused for [link.path]'s window ({why:?})");
                        }
                    }
                    return;
                }
                if let Found::Command(built) = does {
                    match built {
                        Ok(cmd) => self.later.push(Later::Command(cmd)),
                        Err(why) => {
                            self.report.links.refused.bump();
                            crate::warn!(
                                "printed path refused for [link.path]'s command ({why:?})"
                            );
                        }
                    }
                    return;
                }
                let uri = crate::paths::file_uri(&path);
                match crate::link::admit_path_owned(uri, self.uris.names()) {
                    Ok(t) if t.action() == Action::Reveal => {
                        // `paths::file_uri` built it with no host, the form
                        // a file manager is handed.
                        self.later.push(Later::Reveal(t.into_uri().into_string()));
                        None
                    }
                    Ok(_) | Err(_) => {
                        self.report.links.refused.bump();
                        Some(PathNote::TooLong)
                    }
                }
            }
            crate::probe::Clicked::Missing => {
                self.report.links.refused.bump();
                Some(PathNote::Missing)
            }
            crate::probe::Clicked::Unchecked => {
                self.report.links.refused.bump();
                Some(PathNote::Unchecked)
            }
        };
        if let Some(n) = note {
            if self.copied.path(n) == crate::copied::Owes::Frame {
                self.pending = Pending::Dirty;
            }
        }
    }

    /// **What the last turn's handlers asked of the OS** ([`Later`]), done
    /// now, at the top of the loop, after `dispatch` returned and with no
    /// handler on the stack; each said and counted as it was when it ran
    /// in the handler. The queue's storage is kept for the next turn.
    fn run_later(&mut self) {
        let mut asked = self.later.take();
        for effect in asked.drain(..) {
            match effect {
                Later::Reveal(uri) => match self.bus.as_mut() {
                    Some(bus) => {
                        bus.reveal(&uri);
                        self.report.links.revealed.bump();
                        crate::info!("printed path sent to the file manager");
                    }
                    None => {
                        self.report.links.refused.bump();
                        crate::warn!("printed path not revealed: no session bus");
                    }
                },
                Later::Command(cmd) => match self.opener.start(cmd) {
                    Ok(()) => {
                        self.report.links.opened.bump();
                        crate::info!("printed path sent to [link.path]'s command");
                    }
                    Err(e) => {
                        self.report.links.refused.bump();
                        crate::warn!("printed path not sent: [link.path]'s command: {e}");
                    }
                },
                Later::Copy(text) => self.link_copied(text),
                Later::Window(run, dir, grid) => {
                    // A program that is gone opens no window: asked here, as
                    // the new baud would run it, before it starts.
                    if run.found(std::env::var_os("PATH").as_deref())
                        == crate::rules::Program::Missing
                    {
                        self.report.links.refused.bump();
                        crate::warn!(
                            "link not opened in a new window: {:?} is not a program that can run",
                            run.program()
                        );
                        continue;
                    }
                    let started = std::env::current_exe().and_then(|exe| {
                        self.opener.start(crate::windows::link_window(
                            &exe,
                            run,
                            dir.as_deref(),
                            grid,
                        ))
                    });
                    match started {
                        Ok(()) => {
                            self.report.links.opened.bump();
                            crate::info!("link opened in a new window");
                        }
                        Err(e) => {
                            self.report.links.refused.bump();
                            crate::warn!("link not opened in a new window: baud -e: {e}");
                        }
                    }
                }
            }
        }
        // Whatever the effects themselves asked for waits a turn.
        self.later.keep(asked);
    }

    /// **The next print job onto the portal's dialog**, while none is up:
    /// the dialog asked for, titled for the window, and the job kept for
    /// its answer.
    fn print_next(&mut self) {
        if !matches!(self.print_dialog, PrintDialog::Idle) {
            return;
        }
        let Some(bus) = self.bus.as_mut() else {
            return;
        };
        let Some(job) = self.prints.next(self.term.printer()) else {
            return;
        };
        let fallback = self
            .derive
            .title(
                self.term.working_directory(),
                std::slice::from_ref(&self.host),
                self.home.as_deref(),
            )
            .unwrap_or_else(|| "baud".to_string());
        let window =
            crate::title::window_title(self.term.title(), self.term.title_encoding(), &fallback);
        let title = crate::print::job_title(&window, job.cut, Default::default());
        let parent = print_parent(self.window.exported());
        crate::info!("print: the dialog, its parent {parent:?}");
        bus.prepare_print(&parent, &title);
        self.print_dialog = PrintDialog::Asked { job, title };
    }

    /// **The dialog's choices: the job as a PDF on its paper, printed**
    /// through `Print` with the token, the file unlinked so nothing is left
    /// behind it. A job that prints nothing, or a file that cannot be made,
    /// ends the exchange, said.
    fn print_ready(&mut self, token: crate::bus::PrintToken, setup: crate::bus::PageSetup) {
        let PrintDialog::Asked { job, title } =
            std::mem::replace(&mut self.print_dialog, PrintDialog::Idle)
        else {
            return;
        };
        let built = self.fonts.configured();
        let (regular, bold) = built.as_ref().map(print_faces).unwrap_or_default();
        let faces = baud_render::print_pdf::Faces {
            regular: &regular,
            bold: &bold,
        };
        let Some((pdf, sheets, stripped)) = crate::print::pdf(&job.bytes, setup, faces) else {
            crate::info!("print: the job prints nothing");
            return self.print_next();
        };
        crate::info!(
            "print: {sheets} sheets, {} bytes of PDF, {stripped} controls not printed, {} pictures reduced, {} left out",
            pdf.len(),
            stripped.reduced(),
            stripped.left_out()
        );
        let file = match print_file(&pdf) {
            Ok(f) => f,
            Err(e) => {
                crate::warn!("print: not printed: the document could not be written: {e}");
                return self.print_next();
            }
        };
        let parent = print_parent(self.window.exported());
        let sent = self
            .bus
            .as_mut()
            .is_some_and(|b| b.print(&parent, &title, file, token));
        match sent {
            true => self.print_dialog = PrintDialog::Printing,
            false => {
                crate::warn!("print: not printed: the session bus does not pass files");
                self.print_next();
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
            Heard::PrintPortal(Ok(v)) => {
                crate::info!("print: the print portal, version {}", v.0);
                crate::print::set_printer(
                    &mut self.term,
                    self.printing,
                    crate::print::Printers::Listed,
                );
            }
            Heard::PrintPortal(Err(e)) => {
                crate::info!("print: no print portal ({e}); a program's print is ignored");
            }
            Heard::PrintReady(token, setup) => self.print_ready(token, setup),
            Heard::PrintEnded(end) => {
                match end {
                    crate::bus::PrintEnd::Printed => crate::info!("print: printed"),
                    crate::bus::PrintEnd::Cancelled => crate::info!("print: cancelled"),
                    crate::bus::PrintEnd::Failed(e) => crate::warn!("print: not printed: {e}"),
                }
                self.print_dialog = PrintDialog::Idle;
                self.print_next();
            }
            Heard::Motion(m) => {
                self.heard_motion.0 = m;
                let (m, a) = self.heard_motion;
                self.set_motion(crate::appearance::motion_of(self.motion_source, m, a))
            }
            Heard::Animations(a) => {
                self.heard_motion.1 = a;
                let (m, a) = self.heard_motion;
                self.set_motion(crate::appearance::motion_of(self.motion_source, m, a))
            }
            Heard::TextScale(t) => {
                let factor = t.factor();
                if self.type_size.text_scale != Some(factor) {
                    self.type_size.text_scale = Some(factor);
                    crate::info!("text scale: {factor}");
                    self.retype();
                }
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
                self.caps = baud_domain::FdoCaps::Answered(caps);
                self.term.set_notice_support(baud_domain::notice_support(
                    self.notes.setting,
                    baud_domain::NoticePlatform::Linux(caps),
                ));
            }
            Heard::Notified(token, id) => {
                // An answer for a token whose slot a newer notification
                // took is not the one shown there: nothing is said.
                let now = self.timed.clock.host(Instant::now());
                if self.notes.slots.notified(token, id, now).is_some() {
                    crate::reads::input(
                        &mut self.term,
                        |t| t.notice_shown(token),
                        |b| self.outbox.push(b),
                    );
                }
            }
            Heard::NotNotified(token, e) => {
                // A refused update leaves the one on screen as it was.
                self.notes.slots.refused(token);
                self.term.notice_dropped(token);
                crate::warn!("notification not shown: the notification server answered {e}");
            }
            Heard::Invoked(id, key, activation) => {
                // "default" is a click on the notification itself, a number
                // its button, read back exactly as it was sent against the
                // buttons it showed: any other key is dropped, not reported.
                let clicked = self.notes.slots.token(&id).and_then(|token| {
                    let button = match self.notes.slots.action(token, &key)? {
                        baud_domain::FdoActionKey::Default => None,
                        baud_domain::FdoActionKey::Button(b) => Some(b),
                    };
                    Some((token, button))
                });
                if let Some((token, button)) = clicked {
                    // A click on the body of one that asked focus
                    // activates the window with the token the server
                    // sent for it, where it sent one: the compositor
                    // decides (`notify.rs`, `support`).
                    if let (None, true, Some(t)) =
                        (button, self.term.notice_focuses(token), activation)
                    {
                        let _ = self.window.activate_with(&mut self.conn.sender(), t);
                    }
                    crate::reads::input(
                        &mut self.term,
                        |t| t.notice_activated(token, button),
                        |b| self.outbox.push(b),
                    );
                }
            }
            Heard::NotifyClosed(id) => {
                if let Some(token) = self.notes.slots.token(&id) {
                    self.notes.forget(token);
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
    /// macOS driver's `apply_config` does: no scroll mode or glass here,
    /// which this driver does not draw. Returns what it could not act on.
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
                    // Before the server answers, the core is told nothing:
                    // its answer tells it.
                    if let baud_domain::FdoCaps::Answered(caps) = self.caps {
                        self.term.set_notice_support(baud_domain::notice_support(
                            c.notifications,
                            baud_domain::NoticePlatform::Linux(caps),
                        ));
                    }
                }
                Part::CopyOnSelect => {
                    self.clipboard.copy_on_select = c.clipboard.copy_on_select;
                }
                Part::Message => {
                    self.renderer.set_message_colours(c.message.colours);
                    self.copied.set(c.message);
                    self.seat.set_place(c.message.link.place);
                    self.pending = Pending::Dirty;
                }
                Part::Link => {
                    self.link_config = c.link.clone();
                    self.seat.set_at_rest(c.link.at_rest());
                    if self.uris.rules() != &c.link.rules {
                        for told in c.link.rules.told() {
                            crate::info!("{told}");
                        }
                    }
                    self.uris.set_rules(c.link.rules.clone());
                    self.seat.paths.set_rules(c.link.rules.clone());
                    self.seat.paths.set(c.link.paths_read());
                    let _ = self.seat.paths.set_missing(c.link.missing_read());
                    let _ = self.seat.paths.set_join(c.link.join_read());
                    self.probes.set_programs(c.link.programs());
                    self.probes.owe_programs();
                    self.lines.forget();
                    self.pending = Pending::Dirty;
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
                // The padding is not lent to the marks here, so they are
                // never drawn.
                Part::WrapMarker => self.renderer.set_wrap_marker(c.wrap_marker),
                // On their own line, the window having no title bar: its
                // pixels given or taken back now.
                Part::ActivityLights => {
                    let was = self.renderer.lights_height();
                    self.activity_lights = c.activity_lights;
                    self.renderer.set_lights_colours(
                        c.activity_lights_colours,
                        baud_render::Halo::of(c.activity_lights_glow),
                    );
                    self.pending = Pending::Dirty;
                    self.renderer
                        .set_lights_line(c.activity_lights.line(), self.type_px() as f32);
                    self.lights_line(was);
                }
                // DEC's line given the words, or none, at the turn's end.
                Part::StatusLineLights => self.status_line_lights = c.status_line_lights,
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
                // The gutter: fewer or more cells, laid out again.
                Part::Padding => {
                    self.padding = c.padding;
                    reconfigure = true;
                }
                Part::Scroll | Part::Reload => {}
                Part::ColourSpace
                | Part::GlassStyle
                | Part::Images
                | Part::Sixel
                | Part::Vs16Width
                | Part::Printing
                | Part::ClipboardArea
                | Part::ShellIntegration
                | Part::ShellSshEnv
                | Part::ShellProgram => {}
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
        let (mut halves, mut said) = crate::palette::load_halves(
            c.palette_light.as_deref(),
            c.palette_dark.as_deref(),
            &crate::palette::Dirs::from_env(c.source.as_deref()),
        );
        halves.selection = c.selection;
        halves.cursor = c.cursor;
        said.extend(crate::palette::glass_raised(&c.glass, c.floor, &halves));
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

    /// **The held reads now owed a feed, fed**: a ReGIS delay's at its
    /// deadline, read or not, a one-shot input's once it has ended, and a
    /// kitty file read's pieces and end while it runs (`reads::paced`). The
    /// loop calls it every turn, and a key before it decides against the
    /// screen (`history_keys::decide`), so the screen it reads is the one
    /// every byte that may be decoded made. The file read's wakes are
    /// drained first, so one its thread sends after is the next wait's.
    fn feed_released(&mut self) {
        if let Some(w) = &self.files_woken {
            w.drain();
        }
        if self.pace.due(Instant::now()) || self.pace.released(&self.term) {
            let began = self.seat.cursor_on(&self.term);
            let took = self.feed(&[], Default::default());
            let ended = self.seat.cursor_on(&self.term);
            let lines = self.seat.counts().core_lines(&self.term);
            self.probes.drain_taken(
                &mut self.seat.paths,
                &self.term,
                (began, ended),
                lines,
                took.iter().copied(),
                Instant::now(),
            );
        }
    }

    /// **Shift+Page Up or Shift+Page Down, taken for the history**
    /// (`history_keys::page`): the window moves a page and the thumb shows,
    /// as it does for the wheel.
    fn page_history(&mut self, p: crate::history_keys::HistoryPage) {
        if crate::history_keys::page(&mut self.term, p) == crate::history_keys::Paged::Moved {
            self.pending = Pending::Dirty;
            self.reveal_thumb();
        }
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
            let overflows = self.term.reply_counts().overflows;
            let outbox = &mut self.outbox;
            let queued = crate::reads::input(&mut self.term, |t| t.key(e), |b| outbox.push(b));
            if !queued && self.term.reply_counts().overflows != overflows {
                crate::warn!("a key's release was dropped: the reply buffer had no room for it");
            }
        }
    }

    /// **The window's child has ended, or let go of its terminal**: once
    /// both are in, in either order (`held::decide`), a shell's window and
    /// a link's that ended well close, and a link's that failed holds,
    /// showing its line below what the program printed (`held::written`,
    /// drawn in `present`) until a key closes it. Each turn after asks
    /// nothing more.
    fn ended(&mut self) -> Quit {
        if self.holding != crate::held::Holding::No {
            return Quit::No;
        }
        match self.pty.decide(self.pty.stream()) {
            crate::held::Decided::Wait => Quit::No,
            // As on macOS, not while a notification waits for its picture,
            // which its deadline bounds (`Delivery::checks`).
            crate::held::Decided::Close => match self.notes.checks() {
                crate::notify::Checks::Done => Quit::Yes,
                crate::notify::Checks::Waiting => Quit::No,
            },
            crate::held::Decided::Hold(line) => {
                self.holding = crate::held::Holding::Yes(line);
                // The line goes in behind any reads a delay held, which go on
                // with it and are filed under their own writers, as every
                // feed's are; the line itself, baud's own, under none.
                let began = self.seat.cursor_on(&self.term);
                let took = self.feed(crate::held::written(), Default::default());
                let ended = self.seat.cursor_on(&self.term);
                let lines = self.seat.counts().core_lines(&self.term);
                self.probes.drain_taken(
                    &mut self.seat.paths,
                    &self.term,
                    (began, ended),
                    lines,
                    took.iter().copied(),
                    Instant::now(),
                );
                self.pending = Pending::Dirty;
                Quit::No
            }
        }
    }

    /// A key event where `pressed.rs` sends it.
    /// **A frame the compositor showed, told to the keystroke instrument**:
    /// `wp_presentation`'s `presented` is the moment it "turned into light",
    /// on the presentation clock, so each new one is the vsync every shown
    /// key's record takes -- its early bound, and the next one's its late
    /// bound. A presentation clock other than `CLOCK_MONOTONIC`, the clock
    /// every other stamp is read on, refuses the run rather than mix two
    /// clocks in one leg. Nothing is read with the instrument off.
    fn presentation_tick(&mut self) {
        if self.latency.on() == crate::latency::Switch::Off {
            return;
        }
        let Some(p) = self.window.presented() else {
            return;
        };
        if self.last_presented == Some(p.at) {
            return;
        }
        self.last_presented = Some(p.at);
        if p.clock != baud_present_wayland::ClockId(crate::sys::CLOCK_MONOTONIC) {
            self.latency.refused("presentation clock");
            return;
        }
        let at = u64::try_from(p.at.0.as_nanos()).unwrap_or(u64::MAX);
        self.latency
            .tick(|| Some(baud_domain::HostTime::of_nanos(at)));
    }

    /// **A line for each turn in which the compositor accounted for
    /// frames**, under `BAUD_PRESENTED=1`: nothing is read with it off.
    fn log_presented(&mut self) {
        let PresentedLog::On(seen) = &mut self.presented_log else {
            return;
        };
        let now = self.window.frames();
        if now == *seen {
            return;
        }
        *seen = now;
        eprintln!("{}", presented_line(self.window.presented(), now));
    }

    fn route_key(&mut self, route: crate::pressed::Route) {
        // A key takes a copy's words down, and still goes on (`copied.rs`).
        if route.typed() && self.copied.dismiss() == crate::copied::Owes::Frame {
            self.pending = Pending::Dirty;
        }
        let k = match route {
            crate::pressed::Route::Field(k) => {
                self.find_key(crate::find::field_key(k.code, k.mods, k.text));
                return;
            }
            crate::pressed::Route::Nobody => return,
            crate::pressed::Route::Program(k) => k,
        };
        // A held window: any key but a modifier's own closes it, and goes
        // to no program; the shortcuts, Copy among them, came before this
        // as their own events.
        if self.holding != crate::held::Holding::No {
            if route.typed() && crate::held::closes(k.code) {
                self.quit = Quit::Yes;
            }
            return;
        }
        // A key dismisses the hover text over DEC's line, and still goes
        // on to the program (`line_tip.rs`, WCAG 2.2's 1.4.13).
        if self.line_tip.dismiss(&self.term) == crate::line_tip::Changed::Yes {
            self.pending = Pending::Dirty;
        }
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
        let overflows = self.term.reply_counts().overflows;
        let drawn = self.term.images().generation();
        // A record opens for a key typed as it is sent, and one that sent
        // nothing is closed as such; a release does neither
        // (`Latency::sent`).
        let (term, outbox) = (&mut self.term, &mut self.outbox);
        let queued = self.latency.sent(k.kind, crate::pty::host_now, || {
            crate::reads::input(term, |t| t.key(k), |b| outbox.push(b))
        });
        // An arrow key moved a ReGIS input cursor, or a key ended the input
        // and took it away: a frame.
        if self.term.images().generation() != drawn {
            self.pending = Pending::Dirty;
        }
        if !queued && self.term.reply_counts().overflows != overflows {
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
        if self.caret.input(Instant::now(), crate::caret::Input::of(k)) == crate::caret::Owes::Frame
        {
            self.pending = Pending::Dirty;
        }
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
            Copied::Set => self.selection_copied(&text),
            Copied::Unavailable => self.uncopied_once(),
            Copied::Unfocused => {
                if let Some(line) = self.clips.sent_unfocused() {
                    crate::warn!("{line}");
                }
            }
        }
    }

    /// **A link's text to the clipboard, for a `[link.*]` rule's `copy`**:
    /// the same write and the same words as a copy of the selection, and no
    /// process started.
    fn link_copied(&mut self, text: String) {
        match self
            .window
            .copy(&mut self.conn.sender(), Board::Clipboard, &text)
        {
            Copied::Set => {
                self.report.links.opened.bump();
                crate::info!("link copied by a [link.*] rule");
                self.selection_copied(&text);
            }
            Copied::Unavailable => self.uncopied_once(),
            Copied::Unfocused => {
                if let Some(line) = self.clips.sent_unfocused() {
                    crate::warn!("{line}");
                }
            }
        }
    }

    /// **A copy of the selection, said** (`copied.rs`): the words, whichever
    /// selection it went to, and the pulse where the settings and the flash
    /// budget allow it. Linux's desktop has no Dim Flashing Lights, and baud
    /// exposes no element to a screen reader on it, so there is nothing to
    /// announce through.
    fn selection_copied(&mut self, text: &str) {
        let count = crate::copied::characters(text);
        let lights = crate::access::FlashingLights::Full;
        let runs = count != crate::copied::Characters(0)
            && crate::copied::pulse_runs(self.copied.message(), self.motion, lights)
            && self.bell.pulse(
                self.window.next_shown(Instant::now()),
                crate::copied::PULSE_FOR,
            );
        let pulse = match runs {
            true => crate::copied::Pulse::Run,
            false => crate::copied::Pulse::Skip,
        };
        if self.copied.selection(count, pulse) == crate::copied::Owes::Frame {
            self.pending = Pending::Dirty;
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
        if self.caret.input(Instant::now(), crate::caret::Input::Paste) == crate::caret::Owes::Frame
        {
            self.pending = Pending::Dirty;
        }
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
            pad: self.gutter(),
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
            pad: self.gutter(),
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

    /// The words' clause at the cell drawn, the size and zoom in force.
    fn line_clause(&self) -> baud_render::FlashClause {
        let m = self.renderer.metrics();
        let cell = (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()));
        crate::line_lamps::clause(cell, self.scale.factor())
    }

    /// **DEC's indicator line given the state lights as they are**, at
    /// each turn after its reads and writes: FAX and SYN change only in a
    /// read, CS only at a write, so the turn that changes one gives it.
    /// OH is who is in front, the read the lights' line makes, taken only
    /// while the line shows the words; the wait wakes the loop to take it
    /// again in bound (`line_lamps_wait`). Linux has no reader for CD, and
    /// shows it unknown (`line_lamps.rs`).
    fn show_line_lamps(&mut self) {
        let clause = self.line_clause();
        let oh = match crate::line_lamps::shown(self.status_line_lights, &self.term, clause) {
            crate::line_lamps::Shown::Words => {
                crate::own_line::lamp(self.in_front(self.timed.clock.host(Instant::now())))
            }
            crate::line_lamps::Shown::None => baud_core::ReadLamp::default(),
        };
        let cd = baud_core::ReadLamp::Unknown;
        let lamps = crate::line_lamps::read(&self.term, &self.outbox, cd, oh);
        let given = crate::line_lamps::given(self.status_line_lights, clause, lamps);
        if crate::line_lamps::give(&mut self.term, given) == crate::line_lamps::Owes::Frame {
            self.pending = Pending::Dirty;
        }
        // And the hover text over them and over the lights' own line, its
        // word or light perhaps gone.
        self.line_tip.set_lights(self.lights_drawn());
        if self.line_tip.refresh(&self.term) == crate::line_tip::Changed::Yes {
            self.pending = Pending::Dirty;
        }
    }

    /// **The lights' own line as the last frame drew it**, for their hover
    /// text, which baud draws on Linux as on macOS: the whole frame is
    /// drawn at its origin in the buffer (`origin.rs`), so its pixels are
    /// the surface's at the scale, less the origin. None while the lights
    /// are elsewhere or off.
    fn lights_drawn(&self) -> Option<crate::line_tip::LightsAt> {
        let corner = (
            f64::from(self.origin.x.value()),
            f64::from(self.origin.y.value()),
        );
        crate::line_tip::LightsLine::drawn(&self.renderer).map(|l| l.at(corner))
    }

    /// **Who is in front at `now`**: the terminal's foreground group
    /// (`TIOCGPGRP`), read at most once a frame for whichever of the
    /// lights' line and DEC's line wants it (`own_line::FrontRead`).
    fn in_front(&mut self, now: baud_domain::HostTime) -> crate::own_line::InFront {
        let (master, shell) = (self.pty.reads().master, self.pty.child());
        self.front.at(now, || {
            crate::own_line::in_front(master.and_then(crate::fg::group), shell)
        })
    }

    /// **How long the wait may last for DEC's line**: while it shows the
    /// words, until OH is due again in 070-14's 80 ms at the compositor's
    /// refresh, a 60 Hz frame until one is known, so a job that prints
    /// nothing is drawn in bound; none while it shows none.
    fn line_lamps_wait(&self, now: Instant) -> Option<Duration> {
        let shown =
            crate::line_lamps::shown(self.status_line_lights, &self.term, self.line_clause());
        if shown == crate::line_lamps::Shown::None {
            return None;
        }
        let frame = self
            .window
            .frame_interval()
            .unwrap_or(crate::own_line::SAMPLE);
        self.front.line_wait(self.timed.clock.host(now), frame)
    }

    /// Whatever the core answered -- a key, a query -- to the child, behind
    /// whatever the kernel has not yet taken. What it cannot take now stays
    /// owed, and the wait wakes for room to write it (`Outbox::owed`).
    fn flush_reply(&mut self) {
        self.outbox.push(self.term.reply());
        self.term.clear_reply();
        if !self.outbox.is_empty() {
            let wrote = self.pty.write(&mut self.outbox);
            if self.outbox.is_empty() {
                self.latency.written(crate::pty::host_now);
            }
            // SD for what the kernel took, CS dark for what it refused.
            let now = self.timed.clock.host(Instant::now());
            self.lights.wrote(wrote, now);
        }
    }

    /// **The activity lights at this turn**, on their own line: OH, SYN and
    /// FAX read at most once a frame (`own_line::SAMPLE`), the clocks
    /// stepped at the motion preference's pace -- full
    /// glow, the portal having no Dim Flashing Lights -- and a frame owed
    /// when the line shows something else. Nothing but SYN's reading is
    /// taken while the lights are elsewhere or off.
    fn tick_lights(&mut self) {
        if self.activity_lights.line().is_none() {
            self.lights.away(&self.term);
            return;
        }
        let now = self.timed.clock.host(Instant::now());
        // OH, SYN and FAX at a frame's pace: the loop turns once a read.
        if self.lights.due(now) == crate::own_line::Sampled::Now {
            let front = self.in_front(now);
            self.lights.states(front, &self.term, &self.outbox, now);
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
            pad: self.gutter(),
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

    /// **What a point is, changed** -- the portal's text scale read after
    /// launch or changed by the user, or the output's size: the font at the
    /// new pixels a point, the lights' line with it, and the grid re-laid
    /// as a configure lays it. Before the first frame that is a re-layout
    /// nobody sees; after it, one resize. A factor out of the desktop's
    /// range is 1, and `"physical"` with no plausible output size is the
    /// desktop's point, each said in the log.
    fn retype(&mut self) {
        self.say_fallback(Retyped::Heard);
        let Some(px) = self.font_px(self.font_size) else {
            return;
        };
        if self.renderer.rescale(px).is_none() {
            crate::warn!("the font did not rescale to the desktop's point; it stays as it was");
            return;
        }
        // The lights' line follows in `configure`, which sets it at a
        // point's pixels on every configure.
        self.damage = Damage::Full;
        self.pending = Pending::Dirty;
        if let Some((w, h)) = self.configured {
            self.configure(w, h);
        }
    }

    /// **What fell back, said** (`typesize::Fallback`): a text scale out
    /// of range at the retype that hears it, and `"physical"` with no
    /// plausible output size once, at the configure or the retype that
    /// first finds it.
    fn say_fallback(&mut self, at: Retyped) {
        let (_, fell) = crate::typesize::px_per_point(self.type_size, self.scale.factor());
        match (fell, at) {
            (crate::typesize::Fallback::TextScale, Retyped::Heard) => crate::warn!(
                "the desktop's text scale {:?} is outside 0.5 to 3; a point is 96 dpi",
                self.type_size.text_scale
            ),
            (crate::typesize::Fallback::Physical, _) if self.unmeasured == Said::Not => {
                self.unmeasured = Said::Once;
                crate::warn!(
                    "[font] dpi = \"physical\": the output's size is not known or not \
                     plausible; a point is the desktop's"
                );
            }
            _ => {}
        }
    }

    /// **The output's size followed** under `[font] dpi = "physical"`: the
    /// size the window reads now (`Window::physical_dpi`) against the one
    /// the point was last sized at, and a retype where they differ.
    fn follow_output(&mut self) {
        if self.type_size.mode != crate::config::FontDpi::Physical {
            return;
        }
        let physical = self.window.physical_dpi();
        if physical != self.type_size.physical {
            self.type_size.physical = physical;
            self.retype();
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
        // So is a new output under `[font] dpi = "physical"`: its size in
        // millimetres is part of what a point is (`typesize.rs`). The
        // backing scale follows the compositor's scale alone.
        let scale = self.window.scale();
        let physical = self.window.physical_dpi();
        let moved = physical != self.type_size.physical
            && self.type_size.mode == crate::config::FontDpi::Physical;
        self.type_size.physical = physical;
        self.say_fallback(Retyped::Configured);
        if scale != self.scale || moved {
            let rescaled = scale != self.scale;
            self.scale = scale;
            match self.font_px(self.font_size) {
                Some(px) if self.renderer.rescale(px).is_some() => {}
                _ => crate::warn!(
                    "the font did not rescale to {} pixels a point; it stays as it was",
                    self.type_px()
                ),
            }
            if rescaled {
                self.term
                    .set_backing_scale(baud_domain::BackingScale::of(scale.factor()));
            }
            crate::info!(
                "scale: {}; a point is {} pixels",
                scale.factor(),
                self.type_px()
            );
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
        // compositor configured, in the frame's pixels, as the macOS driver
        // reports its window in device pixels. The window asks for
        // server-side decorations, and where the compositor draws them its
        // frame lies outside the surface it configured, so the size
        // reported is still the surface's. The rest is not known: xdg-shell
        // tells a client neither its position nor whether it is minimized,
        // and the outputs the window crate binds say only their scale, so
        // those reports stay silent rather than guessed.
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
            .set_lights_line(self.activity_lights.line(), self.type_px() as f32);
        // Inside the padding's gutter on every side (`origin.rs`).
        let (room_w, room_h) =
            crate::origin::room((Px::of(width.0), Px::of(height.0)), self.gutter());
        let text_w = room_w.value().saturating_sub(self.renderer.strip().value());
        let text_h = room_h
            .value()
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
        let was = (self.term.geometry().0, self.term.screen());
        let was_last = crate::paths::last_line(&self.term, self.seat.counts().primary());
        // The re-cut told line by line, into the pace's buffer, which every
        // resize reuses (`paths::Recuts`): no read is between here and the
        // filing below.
        self.pace.recuts.clear();
        let resized = self.pace.recuts.resize(&mut self.term, &ws);
        if let Resized::Truncated { cols, rows } = resized {
            crate::warn!(
                "the window is larger than the grid; {}x{} of it is drawn",
                cols.index(),
                rows.index()
            );
        }
        self.renderer.fit(&self.term);
        // A reflow re-cuts every line, so a selection's columns are gone;
        // a change of height alone on the primary keeps it (note V, V3).
        // The core's record of the resize, taken once, moves the counts and
        // files the paths.
        let rec = self.term.take_record();
        self.seat.resized(&self.term, was, &rec);
        // The printed paths' facts, carried at once where the resize re-cut
        // the lines (`Probes::resized`), before anything reads them.
        let counts = self.seat.counts();
        self.probes.resized(
            &mut self.seat.paths,
            (&rec, self.pace.recuts.last()),
            &self.term,
            was_last,
            &counts,
        );
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
        self.slides.resized((Px::of(width.0), Px::of(height.0)));
        self.stride = Px::of(width.0);
        self.place();
        // **The screen made ready to draw, before the first frame is asked
        // for**: the glyphs it shows drawn into the atlas and its runs
        // shaped now, so the frame's build is warm.
        let (seal, back) = (self.term.sealed(), self.term.window_back());
        self.gpu.prepare(&mut self.renderer, &self.term, seal, back);
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
        let job = self.derive.group();
        if self.derive.check(&self.pty.reads()) == crate::title::Changed::Yes {
            self.send_title();
            // A job that ended may have made what is struck, or installed
            // an app: asked again at the next batch, and the apps only
            // while one was found missing, as on macOS.
            if self.derive.group() != job {
                self.seat.paths.recheck();
                if self.seat.paths.programs_missing() {
                    self.probes.owe_programs();
                }
            }
        }
        // Each hold is timed from the opening that began it (`UpdateClock`).
        let update = self.updates.observe(&self.term, Instant::now());
        // A `?5` change the check found made past a hold's bound is shown
        // by the next frame drawn, whatever the update is by then.
        self.bell.past_bound(self.updates.held_past());
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
        // The cursor this frame presents restarts the cycle if it is not
        // the one the last frame did (`Caret::present`).
        self.caret
            .present(seen_at, crate::caret::PresentedCursor::of(&self.term));
        let caret = self.caret.plan(seen_at, blinks);
        self.renderer.set_cursor_blink(caret);
        // The selection's share of the view, or `?1001`'s region.
        let highlight = self.seat.highlight(&self.term);
        self.renderer.set_highlight(highlight);
        // iTerm2's cursor guide: what a program set, else `[cursor] guide`,
        // its one point at a point's pixels (`type_px`).
        let scale = self.scale.factor();
        let guide = self.term.cursor_guide().unwrap_or(self.cursor_guide);
        self.renderer
            .set_cursor_guide(crate::effects::guide_look(guide, self.type_px()));
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
        // The view's link slots, brought to the view shown now: the link
        // preview and the scan read the rows holding a slot from them.
        let scrolled = self.seat.scrolled();
        let views = self.views.sync(&self.term, scrolled);
        // The link preview, read again where the pointer is while Ctrl is
        // held: a read may have changed what is under it.
        let host = self.uris.names();
        let mods = self.window.modifiers();
        let previewed = self
            .seat
            .hover_refresh(&self.term, mods, host, Some(&views));
        // The lines at rest for this frame (`lined.rs`).
        match self.link_config.lines() {
            Some(style) => {
                if self.seat.paths.observe(&self.term, scrolled) == crate::paths::Moved::Yes {
                    self.lines.forget();
                }
                self.lines.scan(
                    &self.term,
                    scrolled,
                    &views,
                    &mut crate::paths::Links {
                        uris: &mut self.uris,
                        urls: self.link_config.urls_read(),
                        paths: &mut self.seat.paths,
                    },
                );
                self.renderer.set_link_lines(self.lines.lines(), style);
                self.renderer
                    .set_line_colour(self.link_config.line_colour());
                self.renderer
                    .set_link_strikes(self.lines.struck(), self.seat.hover_struck());
            }
            None => {
                self.lines.off();
                self.renderer.set_link_lines(&[], Default::default());
                self.renderer
                    .set_link_strikes(&[], self.seat.hover_struck());
            }
        }
        // The pointer shown for the preview read above, once the scan is done
        // with the view's slots.
        if previewed {
            self.show_pointer();
        }
        self.renderer
            .set_link_missing(self.link_config.missing_color);
        match self.seat.take_entered() {
            Some(crate::paths::Gone::File(path)) => self.seat.paths.recheck_path(&path),
            Some(crate::paths::Gone::NoApp(_) | crate::paths::Gone::NoCommand) => {
                self.probes.owe_programs()
            }
            None => {}
        }
        self.renderer.set_link(self.seat.hover_look().cloned());
        self.renderer.set_link_hover(self.seat.hover_over());
        self.renderer.set_tip(self.line_tip.look().cloned());
        // What a copy says, over that slot for its moment, and none under the
        // link preview, which keeps the bar (`copied.rs`).
        let (cols, rows) = self.term.geometry();
        let preview = crate::copied::Preview::of(self.seat.hover_look());
        let signal = self
            .copied
            .frame(seen_at, preview, highlight.as_ref(), cols, rows);
        // A held window's line, after whatever that slot shows (`held.rs`).
        let held = match &self.holding {
            crate::held::Holding::No => None,
            crate::held::Holding::Yes(line) => crate::held::on_view(
                line,
                self.term.cursor().0,
                self.term.window_back(),
                cols,
                rows,
                self.copied.message().held,
                crate::held::Address::of(preview, self.copied.message().link.place),
            ),
        };
        match (signal.look, held) {
            (Some(look), held) => self
                .renderer
                .set_tip(crate::held::with_line(Some(look), held)),
            (None, Some(held)) => self.renderer.set_tip(crate::held::with_line(
                self.line_tip.look().cloned(),
                Some(held),
            )),
            (None, None) => {}
        }
        self.renderer.set_selection_pulse(signal.pulse);
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
        let composed = composition.clone();
        self.renderer.set_composition(composition);
        // In the surface's points, "surface local coordinates": the grid
        // begins at the frame's origin past the exit-status strip, and the
        // frame's pixels are the scale's a point (`surface.rs`).
        let grid = (
            Px::of(self.origin.x.value() + self.renderer.strip().value()),
            Px::of(self.origin.y.value() + self.renderer.grid_top().value()),
        );
        let rect = rect.map(|r| {
            let r = crate::marked::in_frame(r, grid);
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
        self.timed.keyed(self.caret.last_key());
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
        // Over none of what the shell has drawn (`splash.rs`).
        self.renderer.splash_yields_to(&self.term, Px::of(0));
        let shows: Vec<_> = plan.fireworks.iter().flatten().copied().collect();
        self.renderer.set_fireworks(&shows);
        let margin = self.renderer.clear_color_of(&self.term);
        if margin != self.margin {
            self.margin = margin;
            self.damage = Damage::Full;
        }
        // **One write generation closed a frame, before its first ask of
        // the display** (`Term::seal_writes`), and after the link scan,
        // which keeps the seal it read under, as `App::present` seals: the
        // re-cut keeps its layout under the seal, so the frame's later asks
        // read no row. The draw (`draw_kept`) paints under the same seal.
        let seal = self.term.seal_writes();
        // The marks on the window shown, and the whole frame they are
        // drawn in: the strip at its left, then the grid.
        self.renderer.place_marks(
            &self.term,
            self.term.window_back(),
            baud_render::Leaving::None,
            Px::of(0),
        );
        // The frame at its origin in the surface (`origin.rs`), placed
        // again for a frame whose size changed.
        self.place();
        // **The GPU arm first** (`gpu_arm.rs`): drawn and shown, or owed
        // while the compositor holds both buffers; a frame it hands back,
        // and a slide, is the CPU arm's, below, repainted whole after a
        // GPU frame.
        if self.frame_for == FrameFor::Moment {
            self.window.draw_for(seen_at);
        }
        let inputs = baud_present_vulkan::present::Inputs {
            back: self.term.window_back(),
            highlight,
            composition: &composed,
            predictions: &[],
        };
        // What effect pacing weighs this frame by: whether it draws a trail
        // or the fireworks, and whether the frame before was shown late.
        let effect = paced(&self.renderer, self.timed.trail());
        let late = self.window.take_late();
        let slide = match plan.slide {
            Some(_) => crate::effects::Composed::Yes,
            None => crate::effects::Composed::No,
        };
        match self.gpu.frame(
            (&mut self.window, &mut self.conn.sender()),
            &mut self.renderer,
            &self.term,
            seal,
            &inputs,
            slide,
        ) {
            crate::gpu_arm::Arm::Shown => {
                if let (Some(drawn), Some((cost, cold))) = (effect, self.gpu.cost()) {
                    self.pace(crate::pacer::PacedArm::Gpu, drawn, (cost, cold), late);
                    if let Some(times) = self.gpu.gpu_times() {
                        self.say_gpu_times(drawn, cost, times);
                    }
                }
                self.seat.drawn(self.renderer.bidi());
                self.timed
                    .drawn(self.renderer.blink_seen(), self.term.regis_blink());
                startup_first("first present", &FIRST_PRESENT);
                self.report.presented();
                self.latency.shown_cpu(crate::pty::host_now);
                self.bell.commit(bell);
                self.caret.commit(caret);
                self.pending = Pending::Clean;
                self.frame_for = FrameFor::Soonest;
                return;
            }
            crate::gpu_arm::Arm::Owed => return,
            crate::gpu_arm::Arm::Cpu(_) => {}
        }
        if self.gpu.cpu_owes_whole() {
            self.damage = Damage::Full;
        }
        // The rows the window shows, `window_back` lines up the history
        // while the wheel or the thumb holds it there: the rows the
        // selection, the link under a click and the thumb already read.
        let back = self.term.window_back();
        // A frame a timed effect owed is drawn for the moment it is seen,
        // and held to it.
        if self.frame_for == FrameFor::Moment {
            self.window.draw_for(seen_at);
        }
        self.cpu_cold = baud_render::Cold::No;
        let painting = Instant::now();
        let shown = match plan.slide {
            Some(s) if self.slides.ready() => self.draw_slide(s, (seal, back)),
            _ => {
                // Settled: the slide's frames go, so an idle window holds
                // nothing for the slide.
                if plan.slide.is_none() {
                    self.slides.settle();
                }
                self.draw_in_place((seal, back))
            }
        };
        if shown == Presented::Yes {
            // The CPU arm's cost: the paint, the slide's composition and
            // the premultiply.
            let cost = painting.elapsed();
            startup_first("first present", &FIRST_PRESENT);
            if let Some(drawn) = effect {
                self.pace(
                    crate::pacer::PacedArm::Cpu,
                    drawn,
                    (cost, self.cpu_cold),
                    late,
                );
            }
            self.report.presented();
            self.latency.shown_cpu(crate::pty::host_now);
            self.bell.commit(bell);
            self.caret.commit(caret);
            self.pending = Pending::Clean;
            self.frame_for = FrameFor::Soonest;
            // An arm that failed is torn down now that the surface shows
            // this frame and none of its buffers (`gpu_arm.rs`).
            self.gpu
                .committed(&mut self.window, &mut self.conn.sender());
        }
    }

    /// **The frame last drawn in place, read back into the slide's live
    /// frame** for its old screen, straight again: the window's buffer
    /// that holds it, while one does and is the surface's size. The live
    /// frame then holds the last frame, and the slide's first live frame
    /// is still drawn whole.
    fn frame_from_window(&mut self) {
        let pitch = usize::try_from(self.stride.value()).unwrap_or(0);
        let Some((pixels, stride)) = self.window.last_drawn() else {
            return;
        };
        let frame = &mut self.slides.frame;
        if pitch == 0 || frame.len() / pitch * stride > pixels.len() {
            return;
        }
        for (to, from) in frame.chunks_mut(pitch).zip(pixels.chunks(stride)) {
            for (t, &f) in to.iter_mut().zip(from) {
                *t = straight(Argb(f)).0;
            }
        }
    }

    /// **What a CPU frame drew, kept**: what it cost for the report, the
    /// order each row was drawn in, which the pointer maps back through,
    /// and the blink rates on screen, which bound the next wait.
    fn cpu_drawn(&mut self, frame: baud_render::Frame) {
        self.report.drawn(frame);
        self.cpu_cold = frame.cold;
        self.seat.drawn(self.renderer.bidi());
        self.damage = Damage::Incremental;
        self.timed
            .drawn(self.renderer.blink_seen(), self.term.regis_blink());
    }

    /// **The CPU arm's frame, drawn in place** into a buffer the window
    /// lends (`Window::draw_cpu`), with the rows that buffer misses since
    /// it was last drawn (`Since`); no pixel is copied. A buffer of
    /// another size than the surface's is not drawn, and the frame is
    /// owed.
    fn draw_in_place(&mut self, (seal, back): (baud_domain::Seal, Rows)) -> Presented {
        let (renderer, term, since) = (&mut self.renderer, &self.term, &mut self.since);
        let (painted, changed) = (&mut self.strips, &mut self.changed);
        let (damage, at) = (self.damage, self.origin);
        let (width, rows) = self.slides.surface();
        let surface = (width.value(), rows.value());
        let mut cost = None;
        let drew = &mut cost;
        let shown = self.window.draw_cpu(&mut self.conn.sender(), move |lent| {
            if (lent.size.0.0, lent.size.1.0) != surface {
                return None;
            }
            let stride = Px::of(u32::try_from(lent.stride).ok()?);
            let (frame, rows) = draw_at(
                renderer,
                term,
                (seal, back),
                (lent.pixels, stride, Px::of(surface.0), at),
                (damage, since.age(lent.age)),
                (&mut *painted, &mut *changed),
            )?;
            since.drawn(rows);
            *drew = Some(frame);
            let painted: &Vec<baud_present_wayland::Strip> = painted;
            let changed: &Vec<baud_present_wayland::Strip> = changed;
            Some(baud_present_wayland::InPlace {
                painted,
                changed: match damage {
                    Damage::Full => Changed::Whole,
                    Damage::Incremental => Changed::Strips(changed),
                },
            })
        });
        if let Some(frame) = cost {
            self.cpu_drawn(frame);
            self.frame_holds = FrameHolds::Stale;
        }
        shown
    }

    /// **A slide's frame**: the slide's live frame drawn -- whole when the
    /// frames before were drawn in place -- the grid's rows composed from
    /// it and the kept old screen, the margin below them the live frame's,
    /// and copied into the window whole, as is the frame after the slide.
    fn draw_slide(
        &mut self,
        s: baud_render::ScreenSlide,
        (seal, back): (baud_domain::Seal, Rows),
    ) -> Presented {
        if self.frame_holds == FrameHolds::Stale {
            self.damage = Damage::Full;
        }
        let Some(drawn) = draw_kept(
            &mut self.renderer,
            &self.term,
            (seal, back),
            (&mut self.slides.frame, self.stride, self.origin),
            self.damage,
            &mut self.strips,
        ) else {
            return Presented::No;
        };
        self.frame_holds = FrameHolds::Last;
        self.cpu_drawn(drawn);
        let pitch = usize::try_from(self.stride.value()).unwrap_or(0);
        let SlideFrames {
            frame,
            kept,
            composed,
            ..
        } = &mut self.slides;
        composed.clone_from(frame);
        // The frame's own rows, not the activity lights' line, which
        // holds still as the margin does.
        let rows = self.renderer.frame_size().1;
        let span = crate::width::slide_span(
            Px::of(self.origin.y.value() + self.renderer.grid_top().value()),
            rows,
            self.stride,
            frame,
        );
        let o = baud_render::slide_offsets(s, rows);
        let shown = match baud_render::compose_slide(
            &frame[span.clone()],
            &kept[span.clone()],
            &mut composed[span],
            self.stride,
            rows,
            o,
        ) {
            true => &*composed,
            false => &*frame,
        };
        self.window
            .present(&mut self.conn.sender(), shown, pitch, Changed::Whole)
    }

    /// **An effect frame shown, fed to effect pacing** (`pacer.rs`): the arm
    /// that drew it, what it drew, what it cost and whether its build drew
    /// into the atlas, and whether the frame before was shown late -- which
    /// counts only on feedback synchronised to the vertical blank
    /// (`timing::late`). A change of divisor is logged, at most once a
    /// second.
    fn pace(
        &mut self,
        arm: crate::pacer::PacedArm,
        drawn: crate::pacer::Paced,
        (cost, cold): (Duration, crate::pacer::Cold),
        late: Option<baud_present_wayland::Late>,
    ) {
        let (w, h) = self.renderer.whole_size();
        let key = crate::pacer::Key {
            arm,
            drawn,
            size: (
                crate::pacer::Pixels(w.value()),
                crate::pacer::Pixels(h.value()),
            ),
            scale: self.scale.factor(),
        };
        let missed = match late {
            Some(baud_present_wayland::Late::Yes) => crate::pacer::Missed::Yes,
            _ => crate::pacer::Missed::No,
        };
        let Some(divisor) = self.timed.effect_frame(key, cost, missed, cold) else {
            return;
        };
        let now = Instant::now();
        if self
            .paced_said
            .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1))
        {
            self.paced_said = Some(now);
            crate::info!(
                "effects: {drawn:?} on the {arm:?} arm drawn {divisor:?}, a frame costing {cost:?} against {:?}",
                self.window.frame_interval()
            );
        }
    }

    /// **What an effect frame on the GPU arm cost the GPU**, at most once a
    /// second under `BAUD_LOG=debug`: the pass from the command buffer's
    /// start to its end, the trail's draws inside it and their fragment
    /// invocations (`baud_present_vulkan::timing`), beside the frame's cost
    /// from target to fence, whose remainder is recording, submission and
    /// the queue.
    fn say_gpu_times(
        &mut self,
        drawn: crate::pacer::Paced,
        cost: Duration,
        times: baud_present_vulkan::timing::GpuTimes,
    ) {
        let now = Instant::now();
        if self
            .gpu_said
            .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1))
        {
            self.gpu_said = Some(now);
            let (w, h) = self.renderer.whole_size();
            crate::debug!(
                "gpu times: {drawn:?} at {}x{}, pass {:?}, trail {:?} over {} draws of {} records, {:?} fragments, frame {cost:?}",
                w.value(),
                h.value(),
                times.pass,
                times.effect,
                times.effect_draws.0,
                self.renderer.trail().len(),
                times.effect_fragments.map(|f| f.0),
            );
        }
    }
}

/// **What an effect frame draws**, for effect pacing: the fireworks while a
/// show is up, else the trail when its records are an effect's or a
/// smear's -- a ghost's sweep is a plain fill, and not paced.
fn paced(
    renderer: &Renderer<'_, '_>,
    trail: crate::config::CursorTrail,
) -> Option<crate::pacer::Paced> {
    if renderer.fireworks_uniform([0.0; 2]).is_some() {
        return Some(crate::pacer::Paced::Fireworks);
    }
    renderer
        .trail()
        .iter()
        .any(|r| {
            matches!(
                r.kind,
                baud_render::Kind::Quad | baud_render::Kind::Band | baud_render::Kind::Particle
            )
        })
        .then_some(crate::pacer::Paced::Trail(trail))
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

/// **Where a print job is with the portal**: no dialog, a dialog asked for
/// with its job and title kept for the answer, or the document printing.
enum PrintDialog {
    Idle,
    Asked {
        job: crate::print::Job,
        title: String,
    },
    Printing,
}

/// **The face chains a print is drawn from**: the stack's fallback faces in
/// order for plain text, and its bold face ahead of them for bold.
fn print_faces(
    b: &crate::fonts::Built,
) -> (Vec<baud_font::Face<'static>>, Vec<baud_font::Face<'static>>) {
    use baud_render::{FaceRole, FaceStyle};
    let with = |want: fn(&FaceRole) -> bool| -> Vec<baud_font::Face<'static>> {
        b.faces
            .iter()
            .zip(&b.roles)
            .filter(|(_, r)| want(r))
            .map(|(f, _)| f.clone())
            .collect()
    };
    let plain = with(|r| matches!(r, FaceRole::Fallback));
    let mut bold = with(|r| matches!(r, FaceRole::Style(FaceStyle::Bold)));
    bold.extend(plain.iter().cloned());
    (plain, bold)
}

/// **The print dialog's parent window**, as the portal names one under
/// Wayland, `wayland:` and the window's exported handle (`[[source:
/// xdg-portal-window-identifiers]]`); none, `""`, before the handle comes or
/// where the compositor offers no export.
fn print_parent(handle: Option<&str>) -> String {
    handle.map_or_else(String::new, |h| format!("wayland:{h}"))
}

/// **A document the portal reads**: written to a file in the runtime
/// directory, unlinked at once so nothing is left behind, and rewound to
/// its start, "open for reading" (`[[source: xdg-portal-print]]`).
fn print_file(pdf: &[u8]) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::io::{Seek, SeekFrom, Write};
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("baud-print-{}.pdf", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    let _ = std::fs::remove_file(&path);
    file.write_all(pdf)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(file.into())
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

/// **The presented-frame log** (`BAUD_PRESENTED=1`): a line on stderr each
/// turn the compositor has said it presented or discarded a frame after
/// the last, for a pacing check on a compositor whose own log cannot be
/// read. Off, nothing is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentedLog {
    Off,
    /// The counts the last line gave.
    On(baud_present_wayland::Frames),
}

impl PresentedLog {
    fn from_env() -> PresentedLog {
        match std::env::var("BAUD_PRESENTED").as_deref() {
            Ok("1") => PresentedLog::On(baud_present_wayland::Frames::default()),
            _ => PresentedLog::Off,
        }
    }
}

/// **The CPU arm's frame, drawn into the slide's live frame**: the surface
/// `stride` pixels wide, its rows below one another, the renderer's whole
/// frame at `at` in it, drawn by [`draw_at`] with nothing stale -- `frame`
/// holds the last frame. `strips` are set to the surface's pixel rows
/// painted, at its whole width: what the window copies and damages. What
/// the frame cost, or `None` when the surface does not hold the whole
/// frame.
fn draw_kept(
    renderer: &mut Renderer<'_, '_>,
    term: &Term<'_>,
    (seal, back): (baud_domain::Seal, Rows),
    (frame, stride, at): (&mut [u32], Px, crate::origin::Origin),
    damage: Damage,
    strips: &mut Vec<baud_present_wayland::Strip>,
) -> Option<baud_render::Frame> {
    let mut changed = Vec::new();
    let stale = baud_render::Age::Stale(baud_render::SurfaceRows::NONE);
    draw_at(
        renderer,
        term,
        (seal, back),
        (frame, stride, stride, at),
        (damage, stale),
        (strips, &mut changed),
    )
    .map(|(frame, _)| frame)
}

/// **The CPU arm's frame, drawn into a surface**: `frame`, `width` pixels
/// wide and its rows `stride` pixels apart, the renderer's whole frame at
/// `at` in it. Drawn through the renderer's lent draw: every pixel for
/// [`Damage::Full`], else the damage the renderer reads off `term`'s
/// window `back` lines up, with the rows `age` says the surface misses;
/// the renderer paints the gutter and the leftover around the whole frame
/// on every band it paints. `painted` is set to the surface's pixel rows
/// painted, at its whole width, and `changed` to those it changed of the
/// frame drawn before. What the frame cost and the rows it changed, or
/// `None` when the surface does not hold the whole frame.
fn draw_at(
    renderer: &mut Renderer<'_, '_>,
    term: &Term<'_>,
    (seal, back): (baud_domain::Seal, Rows),
    (frame, stride, width, at): (&mut [u32], Px, Px, crate::origin::Origin),
    (damage, age): (Damage, baud_render::Age),
    (painted, changed): (
        &mut Vec<baud_present_wayland::Strip>,
        &mut Vec<baud_present_wayland::Strip>,
    ),
) -> Option<(baud_render::Frame, baud_render::SurfaceRows)> {
    let (w, h) = renderer.whole_size();
    let fits =
        |at: Px, whole: Px, room: Px| at.value().checked_add(whole.value()) <= Some(room.value());
    crate::origin::surface(frame, stride, width, |target, (sw, sh)| {
        if !fits(at.x, w, sw) || !fits(at.y, h, sh) {
            return None;
        }
        renderer.set_lent_surface((sw, sh), (at.x, at.y));
        let damage = match damage {
            Damage::Full => baud_domain::FrameDamage::Whole(baud_domain::WholeWhy::Requested),
            Damage::Incremental => renderer.damage(term, back),
        };
        let drawn = renderer.draw_lent(term, seal, back, &damage, target, age);
        let strip = |b: baud_render::Band| baud_present_wayland::Strip {
            top: baud_present_wayland::Px(b.top.value()),
            bottom: baud_present_wayland::Px(b.bottom.value()),
        };
        painted.clear();
        painted.extend(renderer.bands(&drawn.rows()).map(strip));
        changed.clear();
        changed.extend(renderer.bands(&drawn.changed()).map(strip));
        Some((renderer.lent_frame(), drawn.changed()))
    })
    .flatten()
}

/// Whether the driver's kept frame holds the last frame drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameHolds {
    Last,
    Stale,
}

/// **The frames a screen switch's slide draws through**, held only while a
/// slide runs: every other CPU frame is drawn in place into the window's
/// buffer (`draw_in_place`), so a window that never slides holds none of
/// them, and a resize makes none.
///
/// `frame` is the live frame the slide's frames are painted into and the
/// window copies out of, the surface's size, so row damage has a target
/// that still holds the last frame; the renderer paints all of it, the
/// gutter and the leftover around the whole frame included (`draw_kept`).
/// `kept` is the old screen kept at the switch, drawn with its cursor
/// hidden, and `composed` the frame each slide frame is composed into.
#[derive(Debug)]
struct SlideFrames {
    frame: Vec<u32>,
    kept: Vec<u32>,
    composed: Vec<u32>,
    /// The surface's size in pixels, the size `frame` is made at.
    size: (Px, Px),
    /// How many times `frame` was made, counted for the tests.
    #[cfg(test)]
    made: Made,
}

/// How many times a slide's live frame was made at the surface's size.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Made(u64);

impl SlideFrames {
    /// None held, for a surface not yet sized.
    fn new() -> Self {
        Self {
            frame: Vec::new(),
            kept: Vec::new(),
            composed: Vec::new(),
            size: (Px::of(0), Px::of(0)),
            #[cfg(test)]
            made: Made(0),
        }
    }

    /// The surface's pixels: its width and its rows.
    fn surface(&self) -> (Px, Px) {
        self.size
    }

    /// **The surface resized**: the frames go, a slide drawn through them
    /// ends, and the next slide makes its frame at the new size.
    fn resized(&mut self, size: (Px, Px)) {
        self.size = size;
        self.settle();
    }

    /// **A slide starts**: the live frame, made at the surface's size in
    /// `margin` unless it is held at that size already.
    fn start(&mut self, margin: Argb) -> &mut Vec<u32> {
        let (w, h) = self.size;
        let words = usize::try_from(u64::from(w.value()) * u64::from(h.value())).unwrap_or(0);
        if self.frame.len() != words {
            self.frame = vec![margin.0; words];
            #[cfg(test)]
            {
                self.made = Made(self.made.0 + 1);
            }
        }
        &mut self.frame
    }

    /// **The slide settled**: the three frames go.
    fn settle(&mut self) {
        self.frame = Vec::new();
        self.kept = Vec::new();
        self.composed = Vec::new();
    }

    /// Whether a slide frame can be drawn: the old screen is kept at the
    /// live frame's size.
    fn ready(&self) -> bool {
        !self.kept.is_empty() && self.kept.len() == self.frame.len()
    }

    /// Whether the three frames hold no byte.
    #[cfg(test)]
    fn holds_none(&self) -> bool {
        [&self.frame, &self.kept, &self.composed]
            .iter()
            .all(|f| f.capacity() == 0)
    }
}

/// How many lent draws [`Since`] keeps what they changed of: a buffer
/// further behind is drawn whole.
const SINCE: usize = 4;

/// **What the last lent draws changed**, newest first: a buffer `n` draws
/// behind misses the union of the `n` newest, and a buffer further behind
/// than those kept, or whose frame the window does not know, is drawn
/// whole.
struct Since {
    changed: [baud_render::SurfaceRows; SINCE],
    kept: baud_present_wayland::Draws,
}

impl Since {
    const NONE: Since = Since {
        changed: [baud_render::SurfaceRows::NONE; SINCE],
        kept: baud_present_wayland::Draws(0),
    };

    /// What a buffer of age `age` misses.
    fn age(&self, age: baud_present_wayland::BufferAge) -> baud_render::Age {
        use baud_present_wayland::BufferAge;
        match age {
            BufferAge::Behind(n) => match usize::try_from(n.0) {
                Ok(i) if n <= self.kept => baud_render::Age::Stale(
                    self.changed[..i]
                        .iter()
                        .fold(baud_render::SurfaceRows::NONE, |a, &c| a.union(c)),
                ),
                _ => baud_render::Age::Whole,
            },
            BufferAge::Whole => baud_render::Age::Whole,
        }
    }

    /// A lent draw changed `changed`.
    fn drawn(&mut self, changed: baud_render::SurfaceRows) {
        self.changed.copy_within(0..SINCE - 1, 1);
        self.changed[0] = changed;
        let most = baud_present_wayland::Draws(u32::try_from(SINCE).unwrap_or(u32::MAX));
        self.kept = baud_present_wayland::Draws(self.kept.0 + 1).min(most);
    }
}

/// **A shared-memory word back to straight alpha**: each colour channel
/// over the alpha, rounded to the nearest byte. Exact where the pixel is
/// opaque; a clear pixel is clear black.
fn straight(word: Argb) -> Argb {
    let (w, a) = (word.0, word.0 >> 24);
    match a {
        0xFF => word,
        0 => Argb(0),
        _ => {
            let ch = |shift: u32| ((((w >> shift) & 0xFF) * 255 + a / 2) / a).min(0xFF) << shift;
            Argb((a << 24) | ch(16) | ch(8) | ch(0))
        }
    }
}

/// The exit line of what the window copied into shared memory over its
/// life ([`baud_present_wayland::Window::copies`]), one line in a fixed
/// format: `shm copies: <bytes> bytes over <presents> presents`, each a
/// plain decimal.
struct ShmCopies(baud_present_wayland::Copies);

impl core::fmt::Display for ShmCopies {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let c = self.0;
        write!(
            f,
            "shm copies: {} bytes over {} presents",
            c.whole.0 + c.rows.0,
            c.presents.0
        )
    }
}

/// `presented: at=<ns> refresh=<ns> presented=<n> discarded=<n>`: the
/// latest frame's stamp on the presentation clock and the refresh it
/// predicts (0 where the compositor predicts none), and the frames the
/// compositor has accounted for so far, so frames presented within one
/// turn are counted though only the last is stamped. `at=None` before a
/// frame has been presented.
fn presented_line(
    p: Option<baud_present_wayland::Presentation>,
    f: baud_present_wayland::Frames,
) -> String {
    let (at, refresh) = match p {
        Some(p) => (
            p.at.0.as_nanos().to_string(),
            p.refresh.as_nanos().to_string(),
        ),
        None => ("None".to_string(), "None".to_string()),
    };
    format!(
        "presented: at={at} refresh={refresh} presented={} discarded={}",
        f.presented.0, f.discarded.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A resize makes no slide frame; a slide makes one**: resized from
    /// 3840x2160 to 1600x900, the slide's frames are not made and hold no
    /// byte; the slide's start makes its live frame once, at the surface's
    /// size in the margin, a second start in the same slide makes none,
    /// and the settle and a resize after it leave nothing held.
    #[test]
    fn a_resize_makes_no_slide_frame_and_a_slide_makes_one() {
        let mut s = SlideFrames::new();
        for (w, h) in [(3840, 2160), (1600, 900)] {
            s.resized((Px::of(w), Px::of(h)));
        }
        assert_eq!(s.surface(), (Px::of(1600), Px::of(900)));
        assert_eq!(s.made, Made(0), "a resize makes no frame");
        assert!(s.holds_none(), "a window that never slides holds none");
        assert!(!s.ready(), "no slide frame is drawn with nothing kept");
        let margin = Argb(0xff20_3040);
        let frame = s.start(margin);
        assert_eq!(frame.len(), 1600 * 900);
        assert!(frame.iter().all(|&w| w == margin.0));
        assert_eq!(s.made, Made(1), "a slide's start makes its frame");
        let _ = s.start(margin);
        assert_eq!(s.made, Made(1), "the frame held is used again");
        s.kept.clone_from(&s.frame);
        assert!(s.ready());
        s.settle();
        assert!(s.holds_none(), "a settled slide holds nothing");
        s.resized((Px::of(3840), Px::of(2160)));
        assert_eq!(s.made, Made(1));
        assert!(s.holds_none());
    }

    /// **A presented line gives the latest stamp, the refresh and the counts**,
    /// so a reader can tell frames presented within one turn; before any
    /// frame, the stamp is `None`.
    #[test]
    fn a_presented_line_carries_the_stamp_and_the_counts() {
        use baud_present_wayland::dmabuf::Count;
        use baud_present_wayland::{ClockId, Frames, Presentation, Stamp};
        let f = Frames {
            presented: Count(7),
            discarded: Count(1),
        };
        let p = Presentation {
            clock: ClockId(1),
            at: Stamp(std::time::Duration::new(2, 5)),
            refresh: std::time::Duration::from_nanos(4_166_822),
        };
        assert_eq!(
            presented_line(Some(p), f),
            "presented: at=2000000005 refresh=4166822 presented=7 discarded=1"
        );
        assert_eq!(
            presented_line(None, Frames::default()),
            "presented: at=None refresh=None presented=0 discarded=0"
        );
        assert_eq!(
            PresentedLog::from_env(),
            PresentedLog::Off,
            "off unless asked"
        );
    }

    /// **The wait ends at once while the window holds events read
    /// already**: with no deadline and with one five seconds out, events
    /// in the window's queue bound the wait at zero, and with none the
    /// wait is the soonest deadline, or unbounded with none. The launch's
    /// round trips read the first configure into that queue, where the
    /// socket's readiness does not show it.
    #[test]
    fn the_wait_ends_at_once_while_the_window_holds_events() {
        let five = Some(Duration::from_secs(5));
        assert_eq!(
            wait_bound(&[None, None], Held::Events),
            Some(Duration::ZERO)
        );
        assert_eq!(
            wait_bound(&[five, None], Held::Events),
            Some(Duration::ZERO)
        );
        // The controls: the deadlines alone.
        assert_eq!(wait_bound(&[five, None], Held::Nothing), five);
        assert_eq!(wait_bound(&[None, None], Held::Nothing), None);
        let one = Some(Duration::from_millis(1));
        assert_eq!(wait_bound(&[five, one], Held::Nothing), one);
    }

    /// **The launch says what the status page says**: Linux under Wayland
    /// is a platform ready to use, so the launch warns of nothing, and the
    /// page lists it beside macOS. Perturbation: a launch warning that the
    /// Wayland version is not ready -- this fails.
    #[test]
    fn the_launch_says_what_the_status_page_says() {
        const SRC: &str = include_str!("linux.rs");
        let code = SRC
            .rsplit_once(concat!("#[cfg(test)]\n", "mod tests {"))
            .map_or(SRC, |(c, _)| c);
        assert!(
            code.contains("pub fn run("),
            "the launch is in the code read"
        );
        assert!(!code.contains(concat!("no screen has ", "shown it")));
        assert!(!code.contains(concat!("not ready for ", "everyday use")));
        let status = include_str!("../../docs/guide/status.md");
        assert!(status.contains("\n## Ready to use: Linux under Wayland\n"));
        assert!(!status.contains(concat!("not ready for", "\n  everyday use")));
    }

    /// **The shell is spawned before anything is presented**: the launch
    /// opens the window and runs its round trips before the storage is
    /// provisioned, and the window's opening commit attaches no buffer
    /// (`a_window_opens_shows_a_frame_and_types`, baud-present-wayland),
    /// so the first frame is the loop's `present`. In `run`, no present
    /// comes before `Pty::spawn_polled`, and the driver whose `present`
    /// draws is built after it. The loop is bound to a compositor, so the
    /// source is what can fail.
    #[test]
    fn the_shell_is_spawned_before_anything_is_presented() {
        const SRC: &str = include_str!("linux.rs");
        let (_, run) = SRC
            .split_once(concat!("pub fn ", "run(given"))
            .expect("the launch");
        let (before, after) = run
            .split_once(concat!("Pty::", "spawn_polled("))
            .expect("the shell's spawn");
        assert!(
            before.contains(concat!("Window::", "open(")),
            "the control: the window is opened before the shell"
        );
        assert!(
            !before.contains(concat!(".", "present(")),
            "a present before the shell is spawned"
        );
        let built = after
            .find(concat!("let mut d = ", "Driver {"))
            .expect("the driver");
        let drawn = after
            .find(concat!("d.", "present();"))
            .expect("the loop's present");
        assert!(built < drawn, "the driver is built before it presents");
    }

    /// **The keystroke instrument costs nothing when it is off**: every
    /// stamp this driver hands it is the clock function itself, read only
    /// inside the instrument once it is on, but for the one read beside a
    /// pty read, which sits behind the instrument's own `on()` test on the
    /// line before it. Read off this file's code, its tests left out; the
    /// positive control is the count of hooks the driver carries.
    #[test]
    fn the_instrument_reads_no_clock_while_it_is_off() {
        let src = include_str!("linux.rs");
        let code = src
            .rsplit_once(concat!("#[cfg(test)]\n", "mod tests {"))
            .map_or(src, |(c, _)| c);
        let lines: Vec<&str> = code.lines().collect();
        let hooks = lines.iter().filter(|l| l.contains("latency.")).count();
        assert!(hooks >= 9, "the scan finds the driver's hooks ({hooks})");
        let called = concat!("host_now", "()");
        for (i, l) in lines.iter().enumerate() {
            if l.contains(called) {
                let guard = i.checked_sub(1).map_or("", |j| lines[j]);
                assert!(
                    guard.contains("latency.on() == crate::latency::Switch::On"),
                    "line {}: the clock is read with no `on()` test before it: {l}",
                    i + 1
                );
            }
        }
    }

    /// **The frame hands the plan the last key before its trail**: in the
    /// frame, `self.timed.keyed(self.caret.last_key());` comes straight
    /// before the `self.timed.plan(` call, so an effect lights on the first
    /// move after a key, a paste or the wheel's keys (`trail::FOLLOWS`).
    /// Without it every effect stays dark on Linux and nothing else here
    /// fails. The planted control: the same source with the line taken out
    /// fails the check.
    #[test]
    fn the_frame_hands_the_plan_the_last_key_before_its_trail() {
        const SRC: &str = include_str!("linux.rs");
        // Spelled apart, so this test does not match its own source.
        let line = concat!("self.timed.keyed(", "self.caret.last_key());");
        let call = concat!("let plan = ", "self.timed.plan(");
        let hands = |src: &str| {
            let bare: String = src.split_whitespace().collect();
            let call: String = call.split_whitespace().collect();
            let parts: Vec<&str> = bare.split(call.as_str()).collect();
            parts.len() == 2 && parts[0].ends_with(line)
        };
        assert!(hands(SRC), "the frame hands the plan the last key");
        assert!(
            !hands(&SRC.replacen(line, "", 1)),
            "the planted control: without the line"
        );
    }

    /// **The CPU arm presents what it painted**: a 20 x 4 grid at a 7 px
    /// gutter's corner in a surface whose every pixel is poison. Drawn
    /// whole, no poison is left, the gutter included, and the strips cover
    /// the surface. Poisoned again, with one cell written on the third row
    /// and the cursor hidden, the strips are that row's band at the
    /// surface's whole width -- the renderer's `bands` of it -- no poison
    /// is left in that band, the gutter's columns included, and every pixel
    /// outside it is still poison.
    /// **Each buffer is drawn with what it missed since it was last drawn,
    /// and a new one whole.** Three buffers the window would lend, each
    /// filled with poison at first, take sixteen frames in the order
    /// below, each frame writing a row of a 20 x 4 grid at a 7 px gutter;
    /// once, between frames, every buffer's frame becomes unknown, as at a
    /// new scale. Each draw's `BufferAge` comes from [`Ages`] and its
    /// `Age` from [`Since`]. A buffer new or unknown paints every pixel
    /// row. Otherwise it paints exactly the bands of the rows this frame
    /// changed with the rows each frame drawn into another buffer since it
    /// was last drawn changed -- no more, so the age decays. The painted
    /// rows are premultiplied in place, and after every frame the buffer
    /// equals a whole draw of the same screen by a second renderer,
    /// premultiplied: no poison and no row of an older frame is left.
    ///
    /// [`Ages`]: baud_present_wayland::Ages
    #[test]
    fn each_buffer_is_drawn_with_what_it_missed_since_it_was_drawn() {
        use baud_core::{BytePos, ReadLen};
        use baud_present_wayland::{Ages, BufferAge, Slot, Strip};
        use baud_render::{Stack, SurfaceRows, Theme};
        const POISON: u32 = 0xDEAD_BEEF;
        let ws = WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).expect("the face")];
        let mut store = Owned::new(&ws);
        let mut t = Term::new(store.storage(), &ws);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        feed(&mut t, b"\x1b[?25l");
        let (cols, rows) = t.geometry();
        let renderer = || {
            let mut r = Renderer::new(
                Stack::new(&faces).expect("a stack"),
                PxSize::of(14.0).expect("a size"),
                Theme::dark(),
                Blend::Native,
                cols,
                rows,
            )
            .expect("cell metrics");
            r.fit(&t);
            r
        };
        let (mut r, mut whole) = (renderer(), renderer());
        let (w, h) = r.whole_size();
        let g = 7;
        let stride = Px::of(w.value() + 2 * g);
        let height = h.value() + 2 * g;
        let at = crate::origin::Origin {
            x: Px::of(g),
            y: Px::of(g),
        };
        let pitch = usize::try_from(stride.value()).expect("a pitch");
        let words = pitch * usize::try_from(height).expect("a height");
        let mut buffers = vec![vec![POISON; words]; 3];
        let mut ages = Ages::<3>::NEW;
        let mut since = Since::NONE;
        // What each buffer missed, by the test's own count: the rows every
        // frame drawn into another buffer since changed.
        let mut missed = [SurfaceRows::NONE; 3];
        let (mut painted, mut changed) = (Vec::new(), Vec::new());
        let rows_of = |strips: &[Strip]| {
            let mut on = vec![false; usize::try_from(height).expect("a height")];
            for s in strips {
                for y in s.top.0..s.bottom.0 {
                    on[usize::try_from(y).expect("a row")] = true;
                }
            }
            on
        };
        let order = [0, 1, 2, 0, 0, 2, 1, 1, 0, 2, 2, 0, 1, 0, 2, 1];
        for (n, &i) in order.iter().enumerate() {
            if n == 9 {
                ages.lose();
            }
            let row = n % 4 + 1;
            feed(&mut t, format!("\x1b[{row};{}Hx{n}", n % 7 + 1).as_bytes());
            let age = ages.age(Slot(i));
            let seal = t.seal_writes();
            let damage = match n {
                0 => Damage::Full,
                _ => Damage::Incremental,
            };
            let (_, rows_changed) = draw_at(
                &mut r,
                &t,
                (seal, Rows::ZERO),
                (&mut buffers[i], stride, stride, at),
                (damage, since.age(age)),
                (&mut painted, &mut changed),
            )
            .expect("the frame fits");
            since.drawn(rows_changed);
            ages.drawn(Slot(i));
            let want = match age {
                BufferAge::Whole => vec![true; usize::try_from(height).expect("a height")],
                BufferAge::Behind(_) => {
                    let s = missed[i].union(rows_changed);
                    let bands: Vec<Strip> = r
                        .bands(&s)
                        .map(|b| Strip {
                            top: baud_present_wayland::Px(b.top.value()),
                            bottom: baud_present_wayland::Px(b.bottom.value()),
                        })
                        .collect();
                    rows_of(&bands)
                }
            };
            assert_eq!(rows_of(&painted), want, "frame {n}, buffer {i}, {age:?}");
            for (j, m) in missed.iter_mut().enumerate() {
                *m = match j == i {
                    true => SurfaceRows::NONE,
                    false => m.union(rows_changed),
                };
            }
            for s in &painted {
                let rows = usize::try_from(s.top.0).expect("a row")
                    ..usize::try_from(s.bottom.0).expect("a row");
                for p in &mut buffers[i][rows.start * pitch..rows.end * pitch] {
                    *p = baud_domain::premultiply(*p);
                }
            }
            let mut fresh = vec![POISON; words];
            let mut scratch = (Vec::new(), Vec::new());
            draw_at(
                &mut whole,
                &t,
                (seal, Rows::ZERO),
                (&mut fresh, stride, stride, at),
                (Damage::Full, baud_render::Age::Whole),
                (&mut scratch.0, &mut scratch.1),
            )
            .expect("the frame fits");
            for p in &mut fresh {
                *p = baud_domain::premultiply(*p);
            }
            assert!(
                buffers[i] == fresh,
                "frame {n}, buffer {i}: a whole draw's pixels"
            );
        }
    }

    /// **The exit line of the shared-memory copies** is the line the
    /// measuring harness reads: the bytes copied whole and by rows,
    /// summed, and the presents that copied them; and the driver's exit
    /// path prints it, on its own line, after the GPU arm's.
    #[test]
    fn the_exit_line_says_what_was_copied_into_shared_memory() {
        use baud_present_wayland::{Bytes, Copies, dmabuf::Count};
        let c = Copies {
            whole: Bytes(33_177_600),
            rows: Bytes(15_360),
            presents: Count(2),
        };
        assert_eq!(
            ShmCopies(c).to_string(),
            "shm copies: 33192960 bytes over 2 presents"
        );
        assert_eq!(
            ShmCopies(Copies::default()).to_string(),
            "shm copies: 0 bytes over 0 presents"
        );
        let src = include_str!("linux.rs");
        let gpu = src
            .find("\n    d.gpu.exit();\n")
            .expect("the GPU arm's exit line");
        let shm = src
            .find("\n    crate::info!(\"{}\", ShmCopies(d.window.copies()));\n")
            .expect("the shared-memory exit line");
        assert!(gpu < shm, "after the GPU arm's line");
    }

    /// **A slide's old screen read back is the screen it was**: a
    /// premultiplied word taken back to straight alpha and premultiplied
    /// again, as the slide copies it, is the word it was, at every alpha
    /// and every channel value the alpha allows, so a slide that only moves
    /// pixels moves them exactly, translucent ones included.
    #[test]
    fn a_word_read_back_to_straight_premultiplies_to_itself() {
        let mut words = Vec::new();
        for a in 0..=255u32 {
            for p in 0..=a {
                words.push(Argb((a << 24) | (p << 16) | ((a - p) << 8) | p));
            }
        }
        let back: Vec<u32> = words.iter().map(|&w| straight(w).0).collect();
        let mut row = vec![0u32; back.len()];
        baud_domain::premultiply_row(&back, &mut row);
        for ((w, b), r) in words.iter().zip(&back).zip(&row) {
            assert_eq!(baud_domain::premultiply(*b), w.0, "{:#010x}", w.0);
            assert_eq!(*r, w.0, "by rows, {:#010x}", w.0);
        }
    }

    #[test]
    fn the_cpu_arm_presents_what_it_painted() {
        use baud_core::{BytePos, ReadLen};
        use baud_domain::{DirtyRows, Row};
        use baud_present_wayland::Strip;
        use baud_render::{ChromeBands, Stack, SurfaceRows, Theme};
        const POISON: u32 = 0xDEAD_BEEF;
        let ws = WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).expect("the face")];
        let mut store = Owned::new(&ws);
        let mut t = Term::new(store.storage(), &ws);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        feed(&mut t, b"\x1b[?25l\x1b[3;5Hab");
        let (cols, rows) = t.geometry();
        let mut r = Renderer::new(
            Stack::new(&faces).expect("a stack"),
            PxSize::of(14.0).expect("a size"),
            Theme::dark(),
            Blend::Native,
            cols,
            rows,
        )
        .expect("cell metrics");
        r.fit(&t);
        let (w, h) = r.whole_size();
        let g = 7;
        let stride = Px::of(w.value() + 2 * g);
        let height = h.value() + 2 * g;
        let at = crate::origin::Origin {
            x: Px::of(g),
            y: Px::of(g),
        };
        let pitch = usize::try_from(stride.value()).expect("a pitch");
        let words = pitch * usize::try_from(height).expect("a height");
        let mut frame = vec![POISON; words];
        let mut strips = Vec::new();
        let seal = t.seal_writes();
        draw_kept(
            &mut r,
            &t,
            (seal, Rows::ZERO),
            (&mut frame, stride, at),
            Damage::Full,
            &mut strips,
        )
        .expect("the frame fits");
        assert!(!frame.contains(&POISON), "drawn whole, gutter included");
        let mut covered = vec![false; usize::try_from(height).expect("a height")];
        for s in &strips {
            for y in s.top.0..s.bottom.0 {
                covered[usize::try_from(y).expect("a row")] = true;
            }
        }
        assert!(covered.iter().all(|&c| c), "{strips:?}");
        frame.fill(POISON);
        feed(&mut t, b"c");
        let seal = t.seal_writes();
        draw_kept(
            &mut r,
            &t,
            (seal, Rows::ZERO),
            (&mut frame, stride, at),
            Damage::Incremental,
            &mut strips,
        )
        .expect("the frame fits");
        let cell = r.metrics().cell_h.value();
        assert_eq!(h.value(), 4 * cell, "the frame is the grid alone");
        let top = g + 2 * cell;
        let band = Strip {
            top: baud_present_wayland::Px(top),
            bottom: baud_present_wayland::Px(top + cell),
        };
        assert_eq!(strips, vec![band], "one row's band");
        let third = SurfaceRows {
            grid: DirtyRows::NONE.with(Row::from_index(2)).expect("a row"),
            chrome: ChromeBands::NONE,
        };
        let bands: Vec<Strip> = r
            .bands(&third)
            .map(|b| Strip {
                top: baud_present_wayland::Px(b.top.value()),
                bottom: baud_present_wayland::Px(b.bottom.value()),
            })
            .collect();
        assert_eq!(strips, bands, "the renderer's bands of what it painted");
        for (y, line) in (0u32..).zip(frame.chunks(pitch)) {
            match (top..top + cell).contains(&y) {
                true => assert!(!line.contains(&POISON), "row {y} painted whole"),
                false => assert!(line.iter().all(|&p| p == POISON), "row {y} untouched"),
            }
        }
    }
}
