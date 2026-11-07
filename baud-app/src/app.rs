//! The window, the responder, and the frame.
#![forbid(unsafe_code)]

use crate::access::{Contrast, MotionSource, motion_override, system_motion};
use crate::bar::Took;
use crate::bell::{Bell, Rings, Update, UpdateClock};
use crate::caret::{Caret, Cycle, Gate, NonBlinking, Owes, PresentedCursor, live_cursor};
use crate::effects::{
    Composed, SwitchForm, TrailCursor, regis_phase, switch_at, switch_form, text_blink,
    trail_cursor, window_of,
};
use crate::gesture::{
    Kind, Owner, Pressed, Selected, Shift, To, held_after, locator_input, locator_under,
    pointer_route, pointer_under, shift_overrides,
};
use crate::host::{
    FIRST_BYTES, FIRST_COMMIT, FIRST_CONTENT, FIRST_DONE, FIRST_PRESENT, FIRST_SPLASH, FIRST_TICK,
    FIRST_TICK_DONE, FIRST_VISIBLE, FIRST_WAKE, MAX_GRID, cell_px, describe, host_geometry,
    startup_first, startup_mark, startup_note,
};
use crate::keep::{
    Before, KeptText, Ring, followed, kept_across_resize, live_moved, numbered, oldest, reset_in,
    scrolled_off_the_top, snap_live, stale_in_moved_rows, switched, tracked_highlight,
    whole_screen_scrolled,
};
use crate::latency::{Ended, Judged, Latency, Switch};
use crate::pty::{Outbox, Pty, Stream, Wrote, host_now};
use crate::reveal::{Reveal, Scale, shown_gutter};
use crate::trail::{GhostRide, ride_px};
use crate::wheel::{Delta, Route, Travel};
use baud_core::{
    Depth, Erasure, Looking, Owned, PromptJump, PromptsOpened, ReadLamp, Screen, Term, Zone,
    grid_stride,
};
use baud_domain::{
    CellFlags, Codepoint, Col, Cols, Content, FnKey, HostTime, ImagePx, KeyCode, KeyEvent,
    KeyEventKind, KeypadMode, LocatorAt, LocatorButtons, LocatorInput, LocatorKind, Modifiers,
    MouseButton, MouseEvent, MouseKind, MouseMods, PackedCell, PointerPx, RegisRepainted, Row,
    RowRot, Rows, WinSize,
};
use baud_font::Px;
use baud_present_macos::clipboard;
use baud_present_macos::settings::{AnimatedImages, Flashing, Heard, Setting};
use baud_present_macos::{
    CellInput, Drawn, Gpu, GpuFrame, GridBuffer, InFlight, MapBuffer, Mode, OverlayBuffer,
    Presented, Presenter, StylesBuffer, SurfaceSpace,
    journal::{Reshape, Wheel},
    screens::{Built, Compare, Screens},
};
use baud_render::Led;
use baud_render::Place;
use baud_render::{
    Blend, BlinkPhases, Blinking, Candidate, Clip, Damage, Frame, GlyphMap, Instance, Look,
    MapEntry, Overlay, Press, PxSize, Renderer, Resolved, Scene, Scroll as ScrollBar, Scrolled,
    Scroller, Selection, Slide, StyleStamp, Tally, Target, Theme, Trail, TrailLook, WalkKey,
    Window, gutter,
};
use macos_liquid_glass::drawable::{
    Appearance, Button as ViewButton, DrawableView, Focus, Handled, KeyPress, ModifierChange,
    Mouse, MouseKind as ViewMouseKind, Rect, Responder, Scroll, Waker,
};
use macos_liquid_glass::glass::{GlassError, GlassStyle, GlassSurface};
use macos_liquid_glass::material::MaterialSurface;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::{MainThreadOnly, Message};
use macos_liquid_glass::objc2_app_kit::{
    NSAppearanceCustomization, NSApplication, NSApplicationActivationPolicy,
    NSAutoresizingMaskOptions, NSBeep, NSColor, NSCursor, NSEvent, NSEventModifierFlags, NSImage,
    NSRequestUserAttentionType, NSScreen, NSScroller, NSScrollerStyle, NSView, NSWindow,
    NSWindowAnimationBehavior, NSWindowOcclusionState, NSWindowStyleMask,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{
    MainThreadMarker, NSBundle, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSSize,
    NSString, NSUserDefaults,
};
use macos_liquid_glass::window::{GlassWindow, TitlebarSide};
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

#[cfg(test)]
mod launch_tests;

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

/// An alternate-screen switch being animated: the display-link timestamp
/// it began at, and how it is drawn ([`switch_at`]), off the same clock
/// as the blink and the trail.
#[derive(Clone, Copy)]
struct TransitionState {
    start: f64,
    form: SwitchForm,
}

/// Whether the GPU path owes a screen switch's kept frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Keep {
    Done,
    Owed,
}

/// What [`App::draw_kept`] left the frame it was called from to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KeptDraw {
    /// Nothing is owed, or it could not be drawn: build the frame.
    Build,
    /// A frame is in flight, the kept one or another: the completion wakes
    /// the responder to build this one.
    Wait,
}

/// A cursor move being animated: the cell the cursor left, the cell it
/// reached, and the display-link timestamp the sweep began at. `progress`
/// is [`crate::trail::ghost_progress`]'s, computed off the tick's own clock the way
/// the blink phase is, so the renderer only interpolates.
#[derive(Clone, Copy, PartialEq, Debug)]
struct TrailState {
    from: (Row, Col),
    to: (Row, Col),
    start: f64,
    /// How far the cell it left has ridden the screen's scroll, pixels, y
    /// down (`ride_ghost`).
    origin_dy: f32,
    /// How far the ghost itself has ridden, and whether it has stopped.
    ride: GhostRide,
    /// How the sweep is drawn, weighed on the ghost's ledger when it
    /// started (`trail::Ledger`).
    strength: baud_render::Strength,
}

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
/// `MapStats::dropped_marks` at 55 cells a frame on cat-cjk -- and that
/// trade is the default (since 2026-09-13) until the hybrid lands; `1` asks
/// for the instances and their fidelity, `0` or `cpu` forces the CPU for a
/// comparison that needs it. A bench pins the path explicitly so two arms
/// never measure two tiers.
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

/// One of each, up to [`Wheel::SETS`]. Fallible the same way the single
/// one was, so a machine that cannot lend the memory falls back rather
/// than half-building a ring.
///
/// Which set a frame goes into is not decided here: the presenter's
/// [`Wheel`] owns that, because it is the structure both threads hold and
/// a set stops being free at the commit and starts again at the landing,
/// neither of which the app can see on its own.
fn sets<T>(mut one: impl FnMut() -> Option<T>) -> Option<Vec<T>> {
    let mut out = Vec::with_capacity(Wheel::SETS);
    for _ in 0..Wheel::SETS {
        out.push(one()?);
    }
    Some(out)
}

/// The cell path's memory, all of it where the GPU reads it: the grid the
/// core writes, the styles the renderer resolves, the map the walk fills.
struct CellMemory {
    /// The overlay and the records that draw the cells it masks, and the
    /// cluster map the walk fills them from.
    /// **Ringed, because a frame is built into the set the GPU is not
    /// reading.** `relayout` and `zoom` present without waiting for the
    /// frame in flight, so the write would otherwise land in the buffer a
    /// live draw is sampling. Writer and reader touch different memory
    /// instead, which is the property a ring has and a barrier buys by
    /// waiting.
    overlay: Vec<OverlayBuffer>,
    overlay_mem: Vec<&'static mut [u8]>,
    exceptions: Overlay,
    records: Vec<Instance>,
    /// The cursor guide's records, drawn under the glyphs.
    grounds: Vec<Instance>,
    candidates: Vec<Candidate>,
    /// **The window, copied, one per set -- research decision D4.**
    ///
    /// The shader used to read `grid` and `alt` where the core writes
    /// them, which D4 names for what it is: *"`bytesNoCopy` over the live
    /// grid is not zero-copy, it is a race"*. The only thing that made it
    /// safe was refusing to feed the parser while a frame was in flight,
    /// and that barrier is the freeze -- a resize or a burst of output
    /// stops the terminal reading its child for the whole of the GPU's
    /// round trip, every frame.
    ///
    /// So the frame gets its own copy of the *screen* -- not the ring, and
    /// not the scrollback behind it -- laid out flat in screen order, which
    /// is why the shader reads it at [`RowRot::NONE`] with a ring of
    /// `rows`. It is 80,000 bytes at 200x50 and about 3 us of CPU, against
    /// an 8,333 us frame at 120 Hz (note BC, section 3, where `frame_gpu`
    /// read the copy and the live grid in the same GPU time under load).
    /// The record of which screen each set holds and the copy itself are
    /// [`Screens::take`], the code `frame_gpu`'s `app` arm runs.
    ///
    /// One per set, for the reason the overlay and the styles are: the
    /// frame in flight is reading its own.
    copies: Vec<GridBuffer>,
    copies_mem: Vec<&'static mut [PackedCell]>,
    styles: Vec<StylesBuffer>,
    styles_mem: Vec<&'static mut [Resolved]>,
    /// Where each set's resolved table stands. A set is written only when no
    /// frame in flight reads it, so it falls a frame behind the others, and
    /// each is brought up to date from the core's log on its own
    /// (`Renderer::sync_styles`): only the slots written since that set was
    /// last built.
    style_stamps: Vec<StyleStamp>,
    /// **The slots each set's resolved table lent its last frame's frozen
    /// rows** (`baud_render::Lent`): a window scrolled back over history the
    /// core froze off the screen names ids in a generation's words, which
    /// its copy draws through slots no other row of the frame names, given
    /// back before the set is built again.
    lent: Vec<baud_render::Lent>,
    /// The lent frame's copy as a view, for the walk and the overlay, which
    /// read each cell's id in the same table the shader does.
    lent_cells: Vec<PackedCell>,
    lent_used: Vec<baud_domain::RowExtent>,
    /// **Two, ping-ponged on a rescale, and not a set a frame.** Rotating
    /// per frame would be three caches each seeing every third frame and
    /// the miss rate would roughly triple. Per frame it needs no ring
    /// anyway: `ensure` only ever writes an entry for a codepoint no
    /// committed frame draws, and linear probing is sound under insertion.
    ///
    /// What is not sound is the *clear*. A rescale invalidates every entry
    /// and `GlyphMap::over` fills all 16,384 slots, which is 384 KB written
    /// into the buffer the shader is probing for the frame in flight. So
    /// the next map is built over the other buffer and the binding moves --
    /// a pointer, no copy, no allocation and no wait.
    maps: Maps,
    /// **What the last frame was built from, so a frame built from the same
    /// screen does less** (`Screens`): the map walk is skipped and a set
    /// whose copy already holds the screen is not copied again.
    screens: Screens,
    /// **The window in the order UAX #9 gives each row**, while a row is
    /// drawn in another order (`baud_render::Orders::window`): what the map
    /// walk, the overlay and the copy read. Empty, and the core's own view
    /// read, while none is.
    bidi_cells: Vec<PackedCell>,
    bidi_used: Vec<baud_domain::RowExtent>,
    /// What the last frame was built from, besides its set's buffers and
    /// `records` and `grounds`, which hold it until the next is built.
    last: Option<CellBuilt>,
}

/// **What the cell path's last frame was built from**, so a screen switch
/// can build it again with no cursor for its slide's kept frame
/// ([`App::draw_kept`]): the set its window was copied into, each row's
/// mark as the walk read it, how many of `records` the walk wrote and
/// which are the trail's and the blink animation's, and the rest of its
/// input as it was handed over.
struct CellBuilt {
    at: baud_present_macos::journal::SetIx,
    cols: Cols,
    rows: Rows,
    used: Vec<baud_domain::RowExtent>,
    walked: Tally,
    moving: (usize, usize),
    stride: baud_domain::Stride,
    ring: Rows,
    highlight: Option<baud_render::Highlight>,
    composition: Option<baud_render::Composition>,
    chrome: Tally,
    slide: Option<Px>,
    table: StyleStamp,
    map_gen: baud_render::MapGeneration,
    reach: baud_render::Reach,
    screen: baud_present_macos::screens::ScreenGen,
}

/// The glyph map and the buffer it is built over, which cannot be told
/// apart.
///
/// They are one thing because binding one while the map is over the other
/// would have the shader probe a map nothing filled -- every cell a miss,
/// every glyph absent -- and nothing in a frame would say so. Only
/// [`Maps::rescaled`] moves either, and it moves both, so the pair cannot
/// come apart the way two fields can. The same reason [`baud_core`] parks
/// a surface's cells, marks, ring and rotation in one struct rather than
/// four.
struct Maps {
    /// Two, ping-ponged on a rescale. Allocated once: a rescale is a
    /// keypress, and allocating 384 KB on one is not realtime.
    buffers: [MapBuffer; 2],
    at: MapSide,
    live: GlyphMap<'static>,
    /// The buffer the live map is *not* over, waiting to become the next.
    spare: Option<&'static mut [MapEntry]>,
}

impl Maps {
    /// The buffer the shader must bind: the one the live map is over.
    fn buffer(&self) -> &MapBuffer {
        &self.buffers[self.at.0]
    }

    /// The next map, built over the other buffer.
    ///
    /// A rescale invalidates every entry, because each names an atlas
    /// rectangle at the old size, and `GlyphMap::over` fills all 16,384
    /// slots. Filling them in the buffer the shader is probing for the
    /// frame in flight is the write race; filling the other one and moving
    /// the binding is a pointer.
    fn rescaled(&mut self) {
        let Some(spare) = self.spare.take() else {
            return;
        };
        let live = core::mem::replace(&mut self.live, GlyphMap::over(spare));
        self.spare = Some(live.into_entries());
        self.at = self.at.other();
    }
}

/// Which of the two glyph-map buffers the live map is built over.
#[derive(Clone, Copy, PartialEq, Eq)]
struct MapSide(usize);

impl MapSide {
    const FIRST: MapSide = MapSide(0);

    fn other(self) -> MapSide {
        MapSide(1 - self.0)
    }
}

/// What asked for a frame: the display link's tick, which is the vsync,
/// a wake from another thread, or the window's opening, whose turn feeds
/// what the shell has written before the first frame is drawn
/// (`Opening::show`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Trigger {
    Tick,
    Wake,
    /// Everything a wake does but the frame: `show` draws the window's
    /// first one itself, at once, with the bytes this turn fed.
    Opening,
}

/// **What a turn's frame gate decides** ([`frame_gate`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Decided {
    /// Nothing is owed a frame.
    Clean,
    /// A synchronised update holds it (`?2026`).
    Held,
    /// Owed, and left to the tick, the frame in flight's completion, or
    /// -- at the opening -- the window's first frame.
    Later,
    /// Drawn now.
    Now,
}

/// **What the pump's frame gate reads**, as the turn left it.
#[derive(Clone, Copy)]
struct FrameGate {
    pending: Pending,
    /// The synchronised update, asked only when a frame is owed: the
    /// asking moves its clock (`UpdateClock::observe`).
    update: Option<Update>,
    trigger: Trigger,
    echo: Echo,
    guess: Guess,
    marked: Marked,
    /// Since the last frame was committed, and how far apart the ticks
    /// are.
    since_commit: Duration,
    interval: Duration,
    flight: InFlight,
}

/// **The pump's frame gate**: whether this turn draws. Nothing owed is
/// [`Decided::Clean`]; a synchronised update the clock still honours holds
/// the frame, so a partial redraw is never shown; at the opening the
/// window's first frame draws it. Otherwise a frame is due on a tick, on a
/// key's echo fed, a prediction or a composition owed, or once a frame's
/// time has passed since the last commit -- so a flood draws once a vsync
/// and a keystroke draws now -- and it is drawn unless one is in flight,
/// whose completion runs the turn again. The launch logo is no input: it
/// owes the ticks frames (`frame`), and nothing here asks about it.
fn frame_gate(g: FrameGate) -> Decided {
    if g.pending != Pending::Dirty {
        return Decided::Clean;
    }
    if g.update == Some(Update::Honoured) {
        return Decided::Held;
    }
    if g.trigger == Trigger::Opening {
        return Decided::Later;
    }
    let due = g.trigger == Trigger::Tick
        || g.echo == Echo::Fed
        || g.guess == Guess::Owed
        || g.marked == Marked::Owed
        || g.since_commit >= g.interval;
    match due && g.flight == InFlight::Idle {
        true => Decided::Now,
        false => Decided::Later,
    }
}

/// Where the grid comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sizing {
    /// The window's size, through the view's resize callback.
    Window,
    /// `BAUD_GRID`: the window was sized to the grid, and the grid does
    /// not follow the window back. The callback fires with the view's
    /// default frame before it settles at the window's, and a pty resized
    /// through that transient is what a benchmark's child read as 120x40.
    Fixed,
}

impl Sizing {
    /// What a program's page-width request may do: the window's grid
    /// follows one, and a pinned one does not.
    fn host(self) -> crate::width::Host {
        match self {
            Sizing::Window => crate::width::Host::Window,
            Sizing::Fixed => crate::width::Host::Pinned,
        }
    }
}

/// A page-width decision acted on inside the read that asked
/// (`reads::feed`'s `after`): its warning said, and on a resize the grid
/// resized, before the rest of the read is fed. Returns the grid the core
/// has after it -- `Term::resize` clamps, and the child is told what the
/// terminal has. **The window over the history goes to the live screen**,
/// at the request's byte: a program that changes the page width is
/// redrawing it.
fn honour(
    polled: Option<(crate::width::Decision, Option<String>)>,
    term: &mut Term<'_>,
) -> Option<crate::width::Grid> {
    let (d, warn) = polled?;
    if let Some(line) = warn {
        crate::warn!("{line}");
    }
    let crate::width::Decision::Resize(grid) = d else {
        return None;
    };
    // A clamp is read back below rather than from the answer: the child is
    // told the grid the terminal has.
    let _ = term.resize(&grid.ws());
    term.window_live();
    Some(crate::width::Grid::of(term))
}

/// What the terminal's surfaces live in when its storage does not hold
/// them: on the cell path, the two GPU buffers the primary ring and the
/// alternate screen are lent from (`Owned::storage_over`).
type Surfaces = Option<(GridBuffer, GridBuffer)>;

struct App {
    /// The terminal, with its storage, its kitty files reader and the GPU
    /// buffers its surfaces live in, all given back when the window closes
    /// (`lent::Screen`).
    term: crate::lent::Screen<Surfaces>,
    /// The renderer with the fonts it draws from -- the faces a program's
    /// `OSC 50` can name and the stacks they lead (`fonts.rs`) -- given
    /// back when the window closes (`lent::Drawing`).
    renderer: crate::lent::Drawing,
    presenter: Option<Presenter>,
    /// The GPU made before the terminal, on the cell path, so the grid
    /// could be lent from it; taken by the presenter when it exists.
    gpu: Option<Gpu>,
    cell: Option<CellMemory>,
    waker: Waker,
    pty: Pty,
    /// What the child is owed and the kernel has not taken (`pty.rs`):
    /// every write goes through it, in order, and the pump offers what is
    /// left on every turn -- the display link's tick among them -- so a
    /// queue that fills is retried on the clock the loop already has, and
    /// the window never waits in `write(2)` for a child that is not reading.
    outbox: Outbox,
    /// A program's log (`?46`, `OSC 46`), written off this thread.
    hostlog: crate::hostlog::HostLog,
    frame: Vec<u32>,
    /// The CPU path's old screen, kept at a screen switch for its slide
    /// with its cursor hidden, and the frame each slide frame is composed
    /// into: the frame buffer itself stays the live frame, which the
    /// renderer paints by rows. `slid` says the surfaces hold what the
    /// frame buffer does not -- a composed frame, or the one before its
    /// cursor was hidden -- so the next frame goes to them whole.
    kept_frame: Vec<u32>,
    composed: Vec<u32>,
    slid: Composed,
    /// The GPU path's kept frame, owed from a screen switch until the next
    /// frame draws it ([`App::draw_kept`]).
    keep: Keep,
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
    /// The selection, over absolute lines of the live surface, and the
    /// rotation's advance since launch that numbers those lines.
    selection: Option<Selection>,
    /// The selection's text, kept past its highlight while `?1040` is set
    /// (`keep::KeptText`).
    kept: KeptText,
    scrolled: Scrolled,
    /// The find field while it is open (`find.rs`): its matches, kept
    /// across reads, and the current one, which is the selection.
    finding: Option<crate::find::Finding>,
    /// The link preview (`link.rs`, `Hover`): the link under the pointer
    /// while Command is held, underlined, and its address in a bar.
    hover: crate::link::Hover,
    /// The hover text over DEC's line's words (`line_tip.rs`).
    line_tip: crate::line_tip::LineTip,
    /// The host a `file:` link is local to, for the preview: read once
    /// when Command goes down and kept while it is held (`preview_host`).
    hover_host: Option<String>,
    /// The pointer the window last showed for a program's `OSC 22` or a
    /// link a click opens (`pointer.rs`), unknown while the pointer is
    /// outside it.
    pointer_shown: crate::pointer::Shown,
    /// What the field left when it last closed, which Command-G reopens it
    /// on when the find pasteboard still holds its query; and the query
    /// last written to the find pasteboard.
    find_left: Option<crate::find::Left>,
    find_written: Vec<Codepoint>,
    /// When the scrollbar's thumb is seen, off the display link: revealed
    /// by a scroll, held, faded, widened while hovered (`reveal`).
    reveal: Reveal,
    /// The thumb's look as last drawn: the tick forces a frame only when the
    /// look it computes differs, so a held, hovered or hidden thumb builds
    /// nothing; and a press is tested against it, what is on the screen.
    drawn_look: Look,
    /// Where the pointer last was: the hover is re-evaluated from it on
    /// every tick, so a window that reaches the live screen under a still
    /// pointer -- a wheel, a key, a paste, a jump -- drops the hover there,
    /// and one that leaves it takes the hover up.
    pointer: Place,
    /// Where the pointer was at the last mouse event: what a frame tests a
    /// left press against. `pointer` is the
    /// scrollbar's, set only after the preview has read the event, so a
    /// frame the preview presents would otherwise test the press where the
    /// pointer was the event before.
    strip: Place,
    /// The scroller "Show scroll bars" asked for when the window was last
    /// laid out: under the legacy style the grid gives up a gutter to it.
    scroller: Scroller,
    /// `?30` as the window was last laid out for it
    /// (`Term::scrollbar_shown`): hidden, nothing is drawn, nothing takes
    /// a press and the legacy gutter is gone (`Driver::scrollbar_mode`).
    bar_shown: baud_core::ScrollbarShown,
    /// The status line's rows as the window was last laid out for them
    /// (`Term::status_rows`, `Driver::status_line`).
    status_rows: Rows,
    /// Wheel travel not yet a whole line (`wheel.rs`).
    travel: Travel,
    /// Sideways wheel travel not yet a whole column (`wheel.rs`).
    travel_across: Travel,
    frames: Tally,
    /// Display-link ticks, which `frames` is not: a tick that finds
    /// nothing pending returns from `present` before a frame is built,
    /// so on an idle screen `frames` stops and this does not. Row 11's
    /// metric is what those ticks cost, and it needs both numbers.
    ticks: Tally,
    /// Frames asked for while the GPU was still reading the last one, and
    /// left owed rather than built (`builds`).
    owed_in_flight: Tally,
    /// `OSC 8` activation, for the five-second report: links opened,
    /// revealed in Finder, and refused (`report::Links`).
    links: crate::report::Links,
    /// See [`Trace`].
    trace: Option<Trace>,
    /// When the five-second report is next due (`report.rs`).
    reported: crate::report::Every<std::time::Instant>,
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
    /// Where the last key sent to the child stands: its echo not yet read,
    /// read but not yet drawn, or drawn. A wake draws at once only when a
    /// frame's time has passed since the last commit, so a flood draws
    /// once a vsync -- and a keystroke landing inside that time waited for
    /// the tick. Row 6 of note N: the read that brings a key's echo is
    /// drawn as soon as no frame is in flight, whatever the clock says,
    /// and only that read; floods still coalesce.
    echo: Echo,
    /// Row 9: the echoes drawn before the pty answered, oldest first; the
    /// frame they are owed; whether a contradiction has closed prediction
    /// until the next prompt; and the zone the last read left the core in,
    /// so the next prompt is seen opening.
    predictions: Vec<Prediction>,
    guess: Guess,
    /// The input manager's composition -- `marked_text`'s marked text: a
    /// dead key's accent, an IME's unconverted text -- drawn at the cursor
    /// until it is committed or cancelled, and whether a frame is owed for a
    /// change to it.
    composing: Vec<Codepoint>,
    marked: Marked,
    hold: Hold,
    /// The core's prompt count at the last read, for [`hold_after`].
    last_prompts: PromptsOpened,
    last_tick: Option<f64>,
    /// When the frames held for a synchronised update (`?2026`) began to be
    /// held. A frame is held while one is open, so a half-drawn screen --
    /// btop's menu redraws inside one -- is never presented; but only up to
    /// [`SYNC_MAX`], so a program that opens one and stalls -- or dies inside
    /// it, leaving it open until a `?2026l`, `DECSTR` or `RIS` -- cannot
    /// freeze the window, nor can one that opens each update in the read
    /// that closes the last.
    updates: UpdateClock<Instant>,
    /// The blink phase this frame draws with, whether any cell blinks, and
    /// whether the phase just flipped -- which forces a full repaint, since a
    /// blink changes no cell content and row damage would not catch it.
    blink: BlinkPhases,
    blink_present: Blinking,
    /// The cursor's blink, on the display link's clock (`caret.rs`): the
    /// config's interval, else the system caret's keys where set, else 250
    /// ms a phase, read on the first tick and at every focus gain,
    /// only while the window is key, restarted by a key sent or output read,
    /// and stopped after `[cursor] stop_blinking_after` without input. `focus`
    /// is whether the window is key, as the last focus callback or poll said.
    /// `blink_choice` is what `[cursor] blink` said, which decides with the
    /// system's non-blinking switch whether the drawing is held steady;
    /// `warned_blink` keeps its warning to one.
    caret: Caret<HostTime>,
    focus: baud_core::Focus,
    blink_choice: crate::config::BlinkChoice,
    warned_blink: Done,
    /// What `[cursor] blink_animation` and `blink_interval_ms` said: the
    /// animation before Reduce Motion folds in (`poll_motion`), and the
    /// interval that wins over the system's keys at every read.
    blink_animation: crate::config::BlinkAnimation,
    blink_interval: crate::config::BlinkInterval,
    /// `[cursor] guide`: the cursor guide while no program has set it.
    cursor_guide: baud_domain::CursorGuide,
    /// `[screen] badge`: whether a program's badge is drawn.
    badge: crate::config::BadgeShown,
    /// What the badge's variables read of the session that the core does
    /// not keep (`effects::Session`), brought up to date with the title.
    session: crate::effects::Session,
    /// Whether the first tick has read the system's caret yet.
    caret_read: Done,
    /// The visual bell no frame drew: a `?5` pair in one read, shown as the
    /// other video until a deadline the display link's clock reaches
    /// (`bell.rs`).
    bell: Bell<HostTime>,
    /// What a `BEL` does in a focused window, from `bell`, and the bells
    /// the last read brought, rung once a frame interval (`bell.rs`).
    bell_style: crate::config::BellStyle,
    ringer: crate::bell::Ringer<Instant>,
    /// The damage the next frame draws with -- `Full` for the frame after a
    /// blink flip, since a blink changes no cell and row damage would miss it.
    next_damage: Damage,
    /// The trail drawn -- `[cursor] trail`, or under Reduce Motion none
    /// unless the config named one -- and what the config said and the
    /// system's motion preference, which decide it together; the ghost
    /// being animated, if any; the cursor cell the last frame drew, so a
    /// move can be detected; and the display link's timestamp this tick,
    /// so the ghost's progress is off the same clock as the blink. The
    /// ghost being `Some` forces a frame the way a blink flip does, so an
    /// idle terminal with a steady cursor builds nothing -- the
    /// `blink_seen` equivalent for this animation.
    cursor_trail: crate::config::CursorTrail,
    trail_pick: crate::config::TrailPick,
    motion: crate::config::SystemMotion,
    motion_source: MotionSource,
    /// Whether Dim Flashing Lights is on: blinking text's off phase is
    /// drawn faint while it is (`dim_off_phases`). Read at launch and when
    /// its notification arrives (`follow_settings`).
    flashing: Flashing,
    /// The activity lights' clocks, told of each read that brought bytes
    /// and each write the kernel took any of, stepped every tick
    /// (`baud_render::lights`), and whether they are shown, from `[window]
    /// activity_lights`.
    activity: baud_render::Activity,
    activity_lights: baud_render::ActivityLights,
    /// What the lights' own line last showed and how, when the setting
    /// puts them there: a tick that changes either owes a frame.
    line_shown: Option<(baud_render::Shown, baud_render::Glow)>,
    /// Whether DEC's indicator line also carries the state lights, from
    /// `[window] status_line_lights` (`line_lamps.rs`).
    status_line_lights: baud_render::StatusLineLights,
    /// Whether Animated Images is on, and `[images] animation`: a kitty
    /// animation plays or holds as the two and the motion preference say
    /// (`access::image_motion`). The first is read at launch and when its
    /// notification arrives (`follow_settings`).
    animated: AnimatedImages,
    image_animation: crate::config::ImageAnimation,
    trail: Option<TrailState>,
    /// The quad or sweep trail's state between frames (`trail.rs`), and
    /// the one ledger of full passes every trail weighs against, the
    /// ghost's sweeps included (`Chase::ledger`): one screen, one second,
    /// whichever trail drew them, so a change of trail starts no fresh one.
    chase: crate::trail::Chase<HostTime>,
    last_cursor_cell: Option<(Row, Col)>,
    /// The screen the last frame saw, from which a program's scroll that
    /// carried the cursor is taken away (`carried.rs`), and the lines the
    /// whole screen's scroll rode the trail since that frame
    /// (`ride_trail`), whose frame is the ride's.
    carried: crate::carried::Carried,
    ridden: Rows,
    tick_target: f64,
    /// **The launch splash's timing** (`splash.rs`): waiting for the
    /// window to be shown, emerging, then vanishing, read off
    /// `tick_target` like the trail.
    splash: crate::splash::Splash,
    /// What the logo's frames and the shell's first reads did, said under
    /// `BAUD_STARTUP=1` once the logo is gone (`splash::LaunchLog`).
    launch_log: crate::splash::LaunchLog,
    /// Whether a frame has reached the layer: one of what the tick's gate
    /// reads before it shows the window to the splash (`Driver::frame`),
    /// with the window visible and the plasma's pipeline built.
    layer_shown: Done,
    /// **The plasma's pipelines, building on a thread of their own** from
    /// the window's being shown (`App::build_plasma`), and taken by the
    /// tick once built (`App::plasma_ready`): the logo waits for them,
    /// and nothing waits for the logo.
    plasma: Option<std::thread::JoinHandle<baud_present_macos::EffectPipes>>,
    /// Whether an alternate-screen switch slides, from `[screen]
    /// transition`, and the switch being animated. `Some` forces a frame the
    /// way a trail does, so an idle terminal on either screen builds nothing.
    screen_transition: crate::config::ScreenTransition,
    transition: Option<TransitionState>,
    /// Whether output scrolls smoothly, from `[scroll] mode`, and the live
    /// slide it drives. The slide's offset is capped at one row, so a fast
    /// burst never accumulates lag; a non-zero pan forces a frame the way a
    /// trail does, and an idle screen -- whose pan is zero -- forces none.
    scroll: crate::config::ScrollMode,
    /// Whether a trail in flight rides a scroll of the whole screen, from
    /// `[cursor] trail_scroll`.
    trail_scroll: crate::config::TrailScroll,
    /// How the cursor is drawn while another window has the keyboard, from
    /// `[cursor] unfocused_hollow`: [`App::show_focus`] hands it to the
    /// renderer on a focus loss.
    unfocused_cursor: baud_render::CursorLook,
    slide: Slide,
    /// The keystroke-to-pixel instrument, on under `BAUD_KEYLAT=1` and one
    /// test per hook otherwise; see `latency.rs`.
    latency: Latency,
    /// The image budget the core was provisioned with, so a refusal's log
    /// line names the limit it hit (`host::describe_refusals`).
    images: baud_domain::ImageBudget,
    /// `[clipboard]`: the `OSC 52` policy and the area the core was given.
    clipboard: crate::config::Clipboard,
    /// The last `OSC 52` write acted on, and the counts (`clip.rs`).
    clips: crate::clip::Clips,
    /// The last page-width request acted on, and the counts (`width.rs`).
    widths: crate::width::Widths,
    /// The reads a ReGIS delay holds, fed on the first tick at or after
    /// its deadline, and those behind a DSR `?15` waiting for the printer
    /// list (`reads::paced`).
    pace: crate::reads::Pace,
    /// `[screen] printing`, read once at launch and passed with each
    /// read of the printer list (`print.rs`).
    printing: crate::config::Printing,
    /// The notifications programs post (`notify.rs`), the center that
    /// shows them -- `None` outside the app's bundle -- and the Dock's
    /// attention request a program's `RequestAttention=yes` or `once` made.
    notes: crate::notify::Delivery<Instant, String>,
    /// The reader of the file iTerm2's `image` names, with every check
    /// kitty's file medium makes.
    pictures: crate::kitty_files::LocalFiles,
    /// The thread that checks iTerm2's pictures before they are attached,
    /// off this one (`notify::Checker`).
    checker: crate::notify::Checker,
    center: Option<baud_present_macos::notify::Center>,
    attention: Option<baud_present_macos::notify::AttentionId>,
    /// iTerm2's fireworks: the shows the requests started (`fireworks.rs`).
    fireworks: crate::fireworks::Fireworks,
    /// When Notification Center's list was last asked for, and whether an
    /// answer is owed.
    delivered_asked: Option<Instant>,
    delivered_last: Option<Instant>,
    /// When the printer list is read again, and the thread that reads it
    /// (`print.rs`, `PrinterList`).
    watch: crate::print::Watch,
    printer_list: PrinterList,
    /// A program's print jobs waiting for the print panel (`print.rs`).
    prints: crate::print::Prints,
    /// Whether the print panel is up, shared with the block that shows it.
    panel: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

/// What launch prepares before a window or a grid exists -- the cache
/// geometry, the GPU path asked for, the config, the faces and the
/// renderer -- so the child shell can be spawned and AppKit started with
/// all of it done; the GPU's probe starts after the spawn (`start_probe`).
/// Under six milliseconds together; the point is what it lets overlap.
struct Boot {
    request: Request,
    geom: baud_domain::CacheGeometry,
    pitch: baud_domain::Stride,
    renderer: Renderer<'static, 'static>,
    /// The faces a program's `OSC 50` can name.
    fonts: crate::fonts::Fonts,
    /// Whether a program may change the font, from `[font] changes`.
    font_changes: baud_domain::FontChanges,
    /// What the surfaces' bytes mean, from the config.
    space: SurfaceSpace,
    /// The font size in points; pixels are this times the backing scale.
    font_size: f32,
    /// The gutter in points around the grid, from the config.
    padding: f64,
    /// Whether the palette follows the appearance, from the config.
    theme: crate::config::ThemeChoice,
    /// The material and its dimming, from the config's `[glass]`.
    glass: crate::config::Glass,
    /// The contrast floor, from `[palette] floor`.
    floor: baud_render::Floor,
    /// The schemes `[palette] light` and `dark` name, read at launch.
    halves: crate::palette::Halves,
    /// What `[cursor] trail` said: a trail by name, or the default, which
    /// Reduce Motion turns off.
    cursor_trail: crate::config::TrailPick,
    /// The cursor the terminal powers on with, from `[cursor] shape` and
    /// `blink`.
    cursor: baud_core::CursorStyle,
    /// What `[cursor] blink` said, and when a blink stops without input.
    blink_choice: crate::config::BlinkChoice,
    cursor_stop: crate::config::StopAfter,
    /// `[cursor] blink_animation` and `blink_interval_ms`.
    blink_animation: crate::config::BlinkAnimation,
    blink_interval: crate::config::BlinkInterval,
    /// `[cursor] guide`.
    cursor_guide: baud_domain::CursorGuide,
    /// `[screen] badge`.
    badge: crate::config::BadgeShown,
    /// `[images] animation`.
    image_animation: crate::config::ImageAnimation,
    /// Whether an alternate-screen switch slides, from `[screen] transition`.
    screen_transition: crate::config::ScreenTransition,
    /// Whether output scrolls smoothly, from `[scroll] mode`.
    scroll: crate::config::ScrollMode,
    /// Whether a trail in flight rides a scroll of the whole screen, from
    /// `[cursor] trail_scroll`.
    trail_scroll: crate::config::TrailScroll,
    /// How the cursor is drawn while another window has the keyboard,
    /// from `[cursor] unfocused_hollow`.
    unfocused_cursor: baud_render::CursorLook,
    /// How much caller memory the image store is given, from `[images]`.
    images: baud_domain::ImageBudget,
    /// Whether the DA claims Sixel, from `[images] advertise_sixel`.
    sixel: baud_domain::SixelClaim,
    /// Whether a `U+FE0F` widens its base, from `vs16_width`.
    vs16_width: baud_domain::Vs16Width,
    /// Whether DECRQCRA is answered, from `[screen] checksum`.
    checksum: baud_domain::ChecksumReport,
    /// The Set-up modes, from `[keyboard]`.
    set_up: baud_domain::SetUp,
    /// What a `SUB` draws, from `[screen] substitute_mark` and
    /// `substitute_character`.
    substitute: crate::config::Substitute,
    /// Whether a new prompt is drawn on the last row, from `[screen]
    /// prompt_at_bottom`.
    prompt_at_bottom: baud_domain::PromptAtBottom,
    /// Whether a program's window operations are performed, from `[window]
    /// operations`.
    window_ops: baud_domain::WindowOps,
    activity_lights: baud_render::ActivityLights,
    status_line_lights: baud_render::StatusLineLights,
    /// Whether the window shows the splash, from `[launch] splash`.
    launch_splash: crate::config::LaunchSplash,
    /// The shell a window starts and whether its prompts are marked, from
    /// `[shell]`.
    shell: crate::pty::ShellStart,
    /// Whether a program may log what it sends, from `[screen] logging`.
    logging: baud_domain::Logging,
    /// Whether a program's print opens the print panel, from `[screen]
    /// printing`.
    printing: crate::config::Printing,
    /// Whether a program's notification is delivered, from `[screen]
    /// notifications`.
    notifications: crate::config::Notifications,
    /// How a printed picture is sent, from `[screen] print_colour`,
    /// `print_rgb`, `print_background` and `print_rotated`.
    graphics_print: baud_domain::GraphicsPrint,
    /// When a key hides the pointer at power-on, from `[pointer]
    /// hide_while_typing`.
    pointer_mode: baud_domain::PointerMode,
    /// How many columns an ambiguous character takes, from `[text]
    /// ambiguous_width`.
    ambiguous_width: baud_domain::AmbiguousWidth,
    /// Whether a spacing mark takes a column, from `[text]
    /// join_spacing_marks`.
    join_spacing_marks: baud_domain::SpacingMarks,
    /// `[clipboard]`.
    clipboard: crate::config::Clipboard,
    /// What a `BEL` does in a focused window, from `bell`.
    bell: crate::config::BellStyle,
    /// What the window says about the config at launch (`live::Notice`).
    notice: crate::live::Notice,
    /// The config as read and the watch on the files it pulls in, taken
    /// before the config was read, while `config_reload` is on.
    reload: Option<(crate::config::Config, crate::reload::Watch<Instant>)>,
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
        // window: its terminal and renderer borrow them, and its close
        // gives them back (`lent.rs`). On
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
        // **The watch before the read** (`reload::Watch::around`), so an
        // edit that lands while the config is read is heard; the themes
        // and fonts it names are read below, under it.
        let (launched, watch) =
            crate::reload::Watch::around(&crate::config::lookup(), Instant::now(), || {
                let l = crate::config::Config::launch();
                let w = &l.watched;
                let files = crate::reload::files(&w.lookup, &w.themes, &w.faces);
                (l, files)
            });
        let notice = crate::live::Notice::of(&launched.said);
        let reload = match launched.config.config_reload {
            crate::config::ConfigReload::On => Some((launched.read, watch)),
            crate::config::ConfigReload::Off => None,
        };
        let cfg = launched.config;
        startup_mark("config");
        match &cfg.source {
            Some(p) => crate::info!("config: {}", p.display()),
            None => crate::info!("config: none found; the built-in stack"),
        }
        for f in &cfg.faces {
            crate::info!("face: {}", f.path);
        }
        crate::info!("{}", crate::host::describe_images(&cfg.images));
        let (data, mut held) = crate::host::map_faces(&cfg);
        let faces = crate::host::parse_faces(&cfg, &data, &mut held);
        let fonts = crate::fonts::Fonts::new(&cfg, data, faces, held);
        startup_mark("faces");
        // An explicit choice is the palette; `Auto` starts dark and is
        // corrected from the window's own appearance before the first
        // frame, because no window exists here to ask.
        // The theme files are read here, at launch, once: an appearance
        // flip swaps between two themes built from them and reads nothing.
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
        startup_mark("themes");
        // The appearance the config pins, or dark's until the window can be
        // asked -- this renderer draws no frame; `run` sets the palette
        // again from `window.is_dark()` before the first one.
        let appearance = match cfg.theme {
            crate::config::ThemeChoice::Light => Appearance::Light,
            _ => Appearance::Dark,
        };
        let theme = theme_for(appearance, &cfg.glass, cfg.theme, cfg.floor, &halves);
        let space = match cfg.colorspace {
            crate::config::Space::Srgb => SurfaceSpace::Srgb,
            crate::config::Space::DisplayP3 => SurfaceSpace::DisplayP3,
        };
        // At a placeholder grid: the renderer follows the terminal
        // (`Renderer::fit`) once the grid is known, and the cell metrics --
        // what the grid is computed from -- do not depend on it.
        let mut renderer = Renderer::new(
            crate::host::stack(&fonts),
            PxSize::of(cfg.font_size * scale as f32).expect("size"),
            theme,
            Blend::Native,
            Cols::of_index(80),
            Rows::of_index(24),
        )
        .expect("cell metrics");
        crate::host::set_up_renderer(&mut renderer, &cfg, &notice);
        // The lights' own line, when the setting asks for one, which the
        // window's arithmetic counts as it counts the strip.
        renderer.set_lights_line(cfg.activity_lights.line(), scale as f32);
        startup_mark("renderer");
        Boot {
            request,
            geom,
            pitch,
            renderer,
            fonts,
            font_changes: cfg.font_changes,
            space,
            font_size: cfg.font_size,
            padding: f64::from(cfg.padding),
            theme: cfg.theme,
            glass: cfg.glass,
            floor: cfg.floor,
            halves,
            cursor_trail: cfg.cursor_trail,
            cursor: baud_core::CursorStyle::of(cfg.cursor_shape, cfg.cursor_blink.power_on()),
            blink_choice: cfg.cursor_blink,
            cursor_stop: cfg.cursor_stop,
            blink_animation: cfg.blink_animation,
            blink_interval: cfg.blink_interval,
            cursor_guide: cfg.cursor_guide,
            badge: cfg.badge,
            image_animation: cfg.image_animation,
            screen_transition: cfg.screen_transition,
            scroll: cfg.scroll,
            trail_scroll: cfg.trail_scroll,
            unfocused_cursor: cfg.unfocused_cursor,
            images: cfg.images,
            sixel: cfg.sixel,
            vs16_width: cfg.vs16_width,
            checksum: cfg.checksum,
            set_up: cfg.set_up,
            substitute: cfg.substitute,
            prompt_at_bottom: cfg.prompt_at_bottom,
            window_ops: cfg.window_ops,
            activity_lights: cfg.activity_lights,
            status_line_lights: cfg.status_line_lights,
            launch_splash: cfg.launch_splash,
            shell: crate::pty::ShellStart::of(&cfg),
            logging: cfg.logging,
            printing: cfg.printing,
            notifications: cfg.notifications,
            graphics_print: cfg.graphics_print,
            pointer_mode: cfg.pointer_mode,
            ambiguous_width: cfg.ambiguous_width,
            join_spacing_marks: cfg.join_spacing_marks,
            clipboard: cfg.clipboard,
            bell: cfg.bell,
            notice,
            reload,
        }
    }

    /// **The GPU's probe, on its own thread**, started by `run` once the
    /// shell is spawned (note AJ section 20): a `fork` waits for any
    /// `dlopen` in flight, and the probe loads the GPU's driver.
    fn start_probe(request: Request) -> std::thread::JoinHandle<Option<Gpu>> {
        // The Metal device on its own thread: creating it is 10-30 ms on
        // this box, and AppKit's initialisation on the main thread is what
        // it overlaps -- the probe is joined once the window exists.
        let probe = std::thread::spawn(move || {
            let t0 = Instant::now();
            let mut g = match request {
                Request::Force(Path::Cpu) => None,
                _ => Gpu::new(),
            };
            if let Some(g) = &mut g {
                let b = g.build();
                let t1 = Instant::now();
                g.warm();
                // The shader-cache question of note N's row 1, as numbers:
                // the library compile and the five pipelines are what a
                // cold cache makes expensive, and the device and the warm
                // pass are the same on every launch.
                startup_note(&format!(
                    "gpu build: device {:.1} ms, library {:.1} ({:?}), pipelines {:.1}, warm pass {:.1}, probe {:.1}",
                    b.device.as_secs_f64() * 1e3,
                    b.library.as_secs_f64() * 1e3,
                    b.library_from,
                    b.pipelines.as_secs_f64() * 1e3,
                    t1.elapsed().as_secs_f64() * 1e3,
                    t0.elapsed().as_secs_f64() * 1e3,
                ));
                // **A second line, not a wider first one.** The five are the
                // answer to row 1's leftover +28.5 ms, and
                // `baud-bench/src/host/shader_cache.rs` reads the line above
                // field by field -- appending to it would take the gate with
                // the finding.
                startup_note(&format!(
                    "gpu pipelines: bg {:.1} ms, glyph {:.1}, cell_bg {:.1}, cell_glyph {:.1}, cell_shape {:.1}",
                    b.each[0].as_secs_f64() * 1e3,
                    b.each[1].as_secs_f64() * 1e3,
                    b.each[2].as_secs_f64() * 1e3,
                    b.each[3].as_secs_f64() * 1e3,
                    b.each[4].as_secs_f64() * 1e3,
                ));
                // The launch logo's plasma is not built here: this thread
                // is joined before the window's first frame, which the
                // logo must not hold (`App::build_plasma`).
            }
            g
        });
        startup_mark("gpu probing");
        probe
    }

    fn metrics_cell(&self) -> (f64, f64) {
        let m = self.renderer.metrics();
        (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()))
    }
}

impl App {
    fn new(
        ws: WinSize,
        sizing: Sizing,
        scale: CGFloat,
        waker: Waker,
        pty: Pty,
        boot: Boot,
        probe: std::thread::JoinHandle<Option<Gpu>>,
    ) -> App {
        // The display's wrap `prepare` gave the renderer, which a copy
        // under `?96` reads its lines by.
        let wrap_words = boot.renderer.word_wrap();
        let Boot {
            request,
            geom,
            pitch,
            mut renderer,
            fonts,
            font_changes,
            space,
            font_size,
            padding,
            // Both were consumed in `prepare`, which built the renderer's
            // palette; `run` reads them off `boot` for the driver.
            theme: _,
            glass: _,
            floor: _,
            halves: _,
            cursor_trail,
            cursor,
            blink_choice,
            cursor_stop,
            blink_animation,
            blink_interval,
            cursor_guide,
            badge,
            image_animation,
            screen_transition,
            scroll,
            trail_scroll,
            unfocused_cursor,
            images,
            sixel,
            vs16_width,
            checksum,
            set_up,
            substitute,
            prompt_at_bottom,
            window_ops,
            activity_lights,
            status_line_lights,
            launch_splash,
            shell,
            logging,
            printing,
            notifications,
            graphics_print,
            pointer_mode,
            ambiguous_width,
            join_spacing_marks,
            clipboard,
            bell: bell_style,
            // `run` takes both off `boot` for the driver.
            notice: _,
            reload: _,
        } = boot;
        let probe = probe.join().unwrap_or(None);
        startup_mark("gpu joined");
        // The motion preference decides the trail before the chase is
        // built, so a launch under Reduce Motion never lights one. Read here,
        // after `sharedApplication`, which is what keeps AppKit's cached
        // answer invalidated.
        let motion_source = motion_override(std::env::var("BAUD_MOTION").ok().as_deref());
        let motion = system_motion(motion_source, reduce_motion);
        let flashing = baud_present_macos::settings::flashing();
        let animated = baud_present_macos::settings::animated_images();
        startup_mark("reduce motion");
        let trail_pick = cursor_trail;
        let cursor_trail = trail_pick.effective(motion);
        crate::info!("motion: {motion:?}; trail: {cursor_trail:?}");
        // The process's first scroller-style read is not the 5 ns a wheel
        // pays: AppKit loads the answer the first time, and this is where
        // the launch path makes it. Its own mark, so it is a number.
        let scroller = scroller_style();
        startup_mark("scroller style");
        // **The storage, the kitty files reader and the GPU buffers are the
        // window's own**, held with the terminal they are lent to and given
        // back when the window closes (`lent::Screen`). The kitty file
        // mediums are read with the text's checks.
        let provisioned = |owned: Owned| {
            owned
                .with_images(images)
                .with_clipboard(clipboard.budget)
                .with_printer(crate::print::budget(printing))
                .with_notices(crate::notify::BUDGET)
        };
        let own_grid = |ws: &WinSize| {
            crate::lent::Screen::new(
                provisioned(Owned::with_geometry(&MAX_GRID, &geom)),
                crate::kitty_files::LocalFiles::new(),
                None,
                |owned| Term::new(owned.storage(), ws),
            )
        };
        // **Both surfaces live in shared memory on the cell path.** The
        // primary is the ring -- the screen's capacity plus the scrollback
        // -- and the alternate screen is its own buffer of the capacity.
        // The shader reads neither: each frame copies the window out of
        // whichever is up into its own set (research decision D4), so the
        // two buffers are an ownership role, held by the terminal's
        // `Screen` rather than bound.
        let (mut wanted, gpu, term) = match (request, probe) {
            (Request::Force(Path::Cells) | Request::Auto, Some(gpu)) => {
                let owned = provisioned(Owned::without_surfaces(&MAX_GRID, &geom, Depth::DEFAULT));
                match (gpu.grid(owned.ring(), pitch), gpu.grid(owned.rows(), pitch)) {
                    (Some((grid, cells)), Some((alt, alt_cells))) => {
                        let term = crate::lent::Screen::new(
                            owned,
                            crate::kitty_files::LocalFiles::new(),
                            Some((grid, alt)),
                            |owned| Term::new(owned.storage_over(cells, alt_cells), &ws),
                        );
                        (Path::Cells, Some(gpu), term)
                    }
                    _ => {
                        crate::warn!("no shared grid; drawing the instances");
                        (Path::Instances, Some(gpu), own_grid(&ws))
                    }
                }
            }
            (Request::Force(Path::Instances), Some(gpu)) => {
                (Path::Instances, Some(gpu), own_grid(&ws))
            }
            (Request::Force(Path::Cells | Path::Instances) | Request::Auto, None) => {
                crate::warn!("no Metal device or shaders; drawing on the CPU");
                (Path::Cpu, None, own_grid(&ws))
            }
            (Request::Force(Path::Cpu), _) => (Path::Cpu, None, own_grid(&ws)),
        };
        renderer.fit(&term);
        // The first frame is laid out for the scroller the setting asks for,
        // as every relayout after it is (`Driver::relayout`).
        renderer.set_gutter(gutter(scroller, crate::reveal::widths(Scale(scale))));
        let mut term = term;
        crate::host::tell_defaults(&mut term, renderer.theme(), baud_core::Overrides::Keep);
        let (frame_w, frame_h) = renderer.whole_size();
        let cell = match (&gpu, wanted) {
            (Some(gpu), Path::Cells) => match (sets(|| gpu.styles(&term)), gpu.map()) {
                (Some(styled), Some((first_map, entries))) => {
                    let overlaid = sets(|| {
                        gpu.overlay(
                            Cols::of_index(usize::from(MAX_GRID.ws_col)),
                            Rows::of_index(usize::from(MAX_GRID.ws_row)),
                        )
                    })
                    .expect("a shared overlay a frame in flight");
                    let (styles, styles_mem): (Vec<_>, Vec<_>) = styled.into_iter().unzip();
                    let (overlay, overlay_mem): (Vec<_>, Vec<_>) = overlaid.into_iter().unzip();
                    // The second map buffer, so a rescale never allocates
                    // and never fills the one being probed.
                    let (second_map, spare) = gpu.map().expect("a second glyph map");
                    // The window's own memory, at the provisioned height and
                    // the same pitch the core lays rows at, so a copy is one
                    // `copy_from_slice` a row and no arithmetic changes when
                    // the grid is resized under it.
                    let copied =
                        sets(|| gpu.grid(Rows::of_index(usize::from(MAX_GRID.ws_row) + 1), pitch))
                            .expect("a shared window copy a frame in flight");
                    let (copies, copies_mem): (Vec<_>, Vec<_>) = copied.into_iter().unzip();
                    Some(CellMemory {
                        overlay,
                        overlay_mem,
                        exceptions: Overlay::new(),
                        records: Vec::new(),
                        grounds: Vec::new(),
                        candidates: Vec::new(),
                        copies,
                        copies_mem,
                        style_stamps: vec![StyleStamp::default(); styles_mem.len()],
                        lent: (0..styles_mem.len())
                            .map(|_| baud_render::Lent::default())
                            .collect(),
                        lent_cells: Vec::new(),
                        lent_used: Vec::new(),
                        styles,
                        styles_mem,
                        screens: Screens::new(),
                        bidi_cells: Vec::new(),
                        bidi_used: Vec::new(),
                        last: None,
                        maps: Maps {
                            buffers: [first_map, second_map],
                            at: MapSide::FIRST,
                            live: GlyphMap::over(entries),
                            spare: Some(spare),
                        },
                    })
                }
                _ => {
                    crate::warn!("no shared styles or map; drawing the instances");
                    // The terminal's surfaces stay in the buffers its
                    // `Screen` holds, whichever path draws them.
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
        let trace = Trace::open(&ws);
        // Said at startup, because the one time it mattered the variable
        // was spelled `AUD_TRACE` and nothing said the files were not
        // being written. Said through `BAUD_LOG`, as the diagnostics beside
        // it are: the off and writing lines are diagnostics, `info` beside
        // `renderer:`, so a misspelling shows under `BAUD_LOG=info` and a
        // default launch stays quiet; a `BAUD_TRACE` that could not be
        // acted on is a `warn`, as a `BAUD_GPU` baud cannot read is.
        match (&trace, std::env::var_os("BAUD_TRACE")) {
            (Some(_), Some(p)) => {
                crate::info!("trace: writing {}.raw and .steps", p.to_string_lossy())
            }
            (None, Some(p)) => crate::warn!(
                "trace: BAUD_TRACE={} but the files could not be created",
                p.to_string_lossy()
            ),
            (None, None) => {
                crate::info!("trace: off (set BAUD_TRACE=<prefix> to record the session)")
            }
            (Some(_), None) => {}
        }
        // The smooth-scroll slide caps its offset at one row, so it needs the
        // row height in device pixels; `renderer` is moved into `App` below.
        let cell_h = renderer.metrics().cell_h;
        let cell_w = renderer.metrics().cell_w;
        // The core advances the cursor past a Sixel by cell rows, so it needs
        // the cell height in pixels; re-set on every `rescale`.
        term.set_cell_size(cell_px(cell_w), cell_px(cell_h));
        // `OSC 1337 ; ReportCellSize` answers in points: the cell over this.
        term.set_backing_scale(baud_domain::BackingScale::of(scale));
        term.set_sixel_claim(sixel);
        crate::host::set_widths(&mut term, vs16_width, ambiguous_width, join_spacing_marks);
        crate::host::set_checksum(&mut term, checksum);
        crate::host::set_up(&mut term, set_up);
        crate::host::set_substitute(&mut term, substitute);
        crate::host::set_prompt_at_bottom(&mut term, prompt_at_bottom);
        crate::host::set_window_ops(&mut term, window_ops);
        crate::host::set_window_abilities(
            &mut term,
            crate::winops::mac_abilities(),
            clipboard.osc52,
        );
        // AppKit always has a pointer to move: a Mac's trackpad, or the
        // cursor a mouse or a tablet drives.
        crate::host::set_locator_device(&mut term, baud_domain::LocatorDevice::Present);
        crate::host::set_hebrew_layout(&mut term, crate::layouts::hebrew_layout());
        crate::host::set_term_name(&mut term);
        crate::host::set_version_text(&mut term);
        crate::host::set_logging(&mut term, logging);
        // A printer only where one is listed (`print.rs`; note AW), and a
        // DSR `?15` held while the list is read again.
        crate::print::set_printer(&mut term, printing, printers_listed());
        crate::print::set_hold(&mut term, printing);
        term.set_graphics_print(graphics_print);
        crate::host::set_pointer_mode(&mut term, pointer_mode);
        crate::host::set_font_changes(&mut term, font_changes);
        crate::host::tell_font(&mut term, &fonts, font_size, font_size);
        term.set_initial_cursor_style(cursor);
        let bell = Bell::new(&term, crate::bell::LENGTH);
        let ringer = crate::bell::Ringer::new(&term);
        let clips = crate::clip::Clips::new(&term);
        // A read stops at a page-width request, and `pump` resizes the grid
        // before the rest of it is parsed (`width.rs`).
        term.set_width_hold(baud_core::WidthHold::On);
        // A read stops once its answers fill the reply buffer past one
        // control's worth, and `pump` sends them before the rest is parsed.
        crate::reads::hold_replies(&mut term);
        // ReGIS draws its built-in alphabet with the embedded face
        // (`regis_font.rs`).
        term.set_regis_font(crate::regis_font::glyph);
        // A ReGIS delay stops the read, and the pump feeds the rest on the
        // tick its deadline falls on (`reads::paced`).
        term.set_regis_hold(baud_core::RegisHold::On);
        // A deleted picture's pages go back to the system (`pages.rs`).
        if let Some(release) = crate::pages::release() {
            term.set_page_release(release);
        }
        let widths = crate::width::Widths::new(&term);
        // **A program's notifications go to the notification center**,
        // from the app's bundle only (`baud_present_macos::notify`), and
        // the core is told they are delivered only when they can be.
        let center = match MainThreadMarker::new().map(baud_present_macos::notify::Center::open) {
            Some(Ok(c)) => Some(c),
            Some(Err(baud_present_macos::notify::Unavailable::NoBundle)) => {
                crate::info!(
                    "notifications: baud runs outside its app bundle, so programs' notifications are not shown"
                );
                None
            }
            _ => None,
        };
        term.set_notice_support(match center {
            Some(_) => crate::notify::support(notifications, crate::notify::Platform::MacOs),
            None => None,
        });
        let notes = crate::notify::Delivery::new(notifications, &term, images);
        let fireworks = crate::fireworks::Fireworks::new();
        let last_prompts = term.prompts_opened();
        App {
            bell,
            bell_style,
            ringer,
            term,
            renderer: crate::lent::Drawing::new(renderer, fonts),
            presenter: None,
            gpu,
            cell,
            waker,
            pty,
            outbox: Outbox::default(),
            hostlog: crate::hostlog::HostLog::new(crate::pty::start_dir()),
            frame: vec![0; slots(frame_w) * slots(frame_h)],
            kept_frame: Vec::new(),
            composed: Vec::new(),
            slid: Composed::No,
            keep: Keep::Done,
            frame_w,
            frame_h,
            view_px: (frame_w, frame_h),
            pad_px: (Px::of(0), Px::of(0)),
            padding,
            scale,
            grid: ws,
            sizing,
            pending: Pending::Dirty,
            selection: None,
            kept: {
                let mut kept = KeptText::default();
                kept.set_word_wrap(wrap_words);
                kept
            },
            scrolled: Scrolled::START,
            finding: None,
            hover: crate::link::Hover::default(),
            line_tip: crate::line_tip::LineTip::default(),
            hover_host: None,
            pointer_shown: crate::pointer::Shown::Unknown,
            find_left: None,
            find_written: Vec::new(),
            reveal: Reveal::HIDDEN,
            drawn_look: Look::HIDDEN,
            pointer: Place::Grid,
            strip: Place::Grid,
            scroller,
            bar_shown: baud_core::ScrollbarShown::Shown,
            status_rows: Rows::ZERO,
            travel: Travel::default(),
            travel_across: Travel::default(),
            frames: Tally::default(),
            ticks: Tally::default(),
            owed_in_flight: Tally::default(),
            links: crate::report::Links::default(),
            trace,
            reported: crate::report::Every::new(std::time::Instant::now()),
            stats: Frame::default(),
            gpu_stats: GpuFrame::default(),
            wanted,
            space,
            font_size,
            font_base: font_size,
            last_commit: Instant::now(),
            interval: Duration::from_micros(8333),
            echo: Echo::Drawn,
            predictions: Vec::new(),
            guess: Guess::None,
            composing: Vec::new(),
            marked: Marked::Drawn,
            hold: Hold::Open,
            last_prompts,
            last_tick: None,
            updates: UpdateClock::new(SYNC_MAX),
            blink: BlinkPhases::SHOWN,
            blink_present: Blinking::default(),
            // AppKit's rate until the first tick reads the system's; the
            // cursor is not drawn blinking before then, since focus starts
            // lost.
            caret: Caret::new(
                host_now(),
                Cycle::of_config(blink_interval),
                Gate::Free,
                cursor_stop,
                blink_animation.effective(motion),
            ),
            blink_animation,
            blink_interval,
            cursor_guide,
            badge,
            session: crate::effects::Session::here(shell.program.as_deref()),
            focus: baud_core::Focus::Lost,
            blink_choice,
            warned_blink: Done::No,
            caret_read: Done::No,
            next_damage: Damage::Incremental,
            cursor_trail,
            trail_pick,
            motion,
            motion_source,
            flashing,
            activity: baud_render::Activity::new(),
            activity_lights,
            line_shown: None,
            status_line_lights,
            animated,
            image_animation,
            trail: None,
            chase: crate::trail::Chase::new(cursor_trail.style()),
            last_cursor_cell: None,
            carried: crate::carried::Carried::new(),
            ridden: Rows::ZERO,
            tick_target: 0.0,
            splash: crate::splash::Splash::new(crate::splash::splash_override(
                std::env::var("BAUD_SPLASH").ok().as_deref(),
                launch_splash,
            )),
            // Under the flag `startup_begin` reads.
            launch_log: crate::splash::LaunchLog::new(match std::env::var_os("BAUD_STARTUP") {
                Some(_) => crate::splash::Logging::On,
                None => crate::splash::Logging::Off,
            }),
            layer_shown: Done::No,
            plasma: None,
            screen_transition,
            transition: None,
            scroll,
            trail_scroll,
            unfocused_cursor,
            slide: Slide::new(cell_h),
            latency: Latency::from_env(),
            images,
            clipboard,
            clips,
            widths,
            pace: crate::reads::Pace::new(),
            printing,
            notes,
            pictures: crate::kitty_files::LocalFiles::new(),
            checker: crate::notify::Checker::new(move || waker.wake(), crate::notify::attaches),
            fireworks,
            center,
            attention: None,
            delivered_asked: None,
            delivered_last: None,
            watch: crate::print::Watch::new(),
            printer_list: PrinterList::new(waker),
            prints: crate::print::Prints::new(),
            panel: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(PANEL_IDLE)),
        }
    }

    /// **The cursor as the focus has it**: `[cursor] unfocused_hollow`'s
    /// look while another window has the keyboard, its shape while this one
    /// has it.
    ///
    /// A frame is owed on a change, as nothing else on screen moved.
    /// Nothing else reads the focus for the cursor: a trail runs as it
    /// does in front, and `?1004` reports the focus and changes nothing
    /// drawn.
    fn show_focus(&mut self, f: baud_core::Focus) {
        let look = crate::caret::look(f, self.unfocused_cursor);
        if look != self.renderer.cursor_look() {
            self.renderer.set_cursor_look(look);
            self.pending = Pending::Dirty;
        }
    }

    fn metrics_cell(&self) -> (f64, f64) {
        let m = self.renderer.metrics();
        (f64::from(m.cell_w.value()), f64::from(m.cell_h.value()))
    }

    /// The pixels beside the grid the window keeps, in device pixels: the
    /// legacy scroller's gutter `bar` and the exit-status strip
    /// ([`Renderer::strip`]), which every width the window is given or
    /// gives the grid counts.
    fn beside(&self, bar: Px) -> f64 {
        f64::from(bar.value().saturating_add(self.renderer.strip().value()))
    }

    /// Tells the child the grid the terminal *has* and the text area in
    /// pixels, from the cell the renderer draws -- the numbers `CSI 14 t`
    /// answers with, so a child that asks either way hears the same thing.
    /// A no-op when neither moved since the kernel was last told.
    ///
    /// **Then the replies, so `?2048`'s report goes out with the `SIGWINCH`,
    /// not a frame later.** `Term::resize` queues one, and so does
    /// `Term::set_cell_size` when a zoom moves the pixels with the grid
    /// unchanged. The point of an in-band notification is that it has a
    /// position in the stream the application is reading -- leaving it in the
    /// buffer until the next pump would put it after whatever the child wrote
    /// in between, which is the ordering the mode exists to fix. After the
    /// `ioctl` and never before it: `[[source: inband-resize]]` forbids a
    /// report before the resize is complete. The flush is here rather than
    /// at each caller because a same-grid zoom reaches `relayout`'s
    /// fall-through, which had none.
    fn tell_pty(&mut self) {
        let (cols, rows) = self.term.geometry();
        let ws = WinSize {
            ws_col: u16::try_from(cols.index()).unwrap_or(u16::MAX),
            ws_row: u16::try_from(rows.index()).unwrap_or(u16::MAX),
        };
        let m = self.renderer.metrics();
        self.pty.resize(&ws, m.cell_w, m.cell_h);
        self.flush_reply(Asked::Unasked);
    }

    /// Sends a key to the core and whatever it answers to the shell, and
    /// says whether anything did: a key the core encodes to nothing asks
    /// for no echo, and the instrument must not wait for one -- not even
    /// when the flush behind it writes bytes an earlier event left owed.
    fn send(&mut self, e: KeyEvent) -> Sent {
        // Into an empty buffer, so the key always fits and `false` is only a
        // key that encodes to nothing (`reads::input`).
        let drawn = self.term.images().generation();
        // A reply the terminal queued on its own goes out ahead of the key,
        // through `reads::input`: AA's, not the key's.
        if !self.term.reply().is_empty() {
            self.activity.seen(Led::Aa, host_now());
        }
        let encoded = crate::reads::input(&mut self.term, |t| t.key(e), |b| self.outbox.push(b));
        // An arrow key moved a ReGIS input cursor, or a key ended the input
        // and took it away: a frame, and the tick's pump feeds the reads
        // the input held (`reads::Pace`).
        if self.term.images().generation() != drawn {
            self.pending = Pending::Dirty;
        }
        // A key shows the cursor and restarts its blink, whether or not the
        // child echoes it back, and is input to the stop (`caret.rs`). The
        // wheel's arrow keys on the alternate screen do not come through
        // here: `Route::Keys` sends them through `Term::wheel_key`, which
        // KAM does not lock and SRM does not draw, and restarts the caret
        // itself.
        self.caret.input(host_now());
        match (encoded, self.flush_reply(Asked::Typed)) {
            (true, Sent::Bytes) => Sent::Bytes,
            _ => Sent::Nothing,
        }
    }

    /// Where the window sits in the ring, for the scrollbar.
    fn window(&self) -> Window {
        window_of(&self.term)
    }

    /// How the thumb is seen at `now`: the reveal's under the overlay style,
    /// and constant under the legacy one, where nothing fades or hovers --
    /// so the tick, which forces a frame only when this changes, forces none
    /// for it, and a wheel's reveal draws nothing new.
    fn look(&self, now: HostTime) -> Look {
        self.reveal.look_under(self.scroller, now)
    }

    /// A read of the printer list for `why`, off this thread, with printing
    /// on and none out (`print.rs`).
    fn read_printers(&mut self, why: crate::print::Ask) {
        if self.printing == crate::config::Printing::On && self.watch.ask(Instant::now(), why) {
            self.printer_list.read();
        }
    }

    /// Whatever the core answered to the child, behind whatever it is
    /// already owed, as much as the kernel takes now; `Bytes` only when that
    /// emptied the outbox. With nothing owed it makes no system call: the
    /// pump runs this every tick.
    ///
    /// The reply was copied out and cleared whether or not the write went,
    /// so a short write lost the rest; the outbox keeps it.
    fn flush_reply(&mut self, asked: Asked) -> Sent {
        let now = host_now();
        // AA: a reply the terminal queued on its own, at the flush that
        // sends it.
        match asked {
            Asked::Unasked if !self.term.reply().is_empty() => self.activity.seen(Led::Aa, now),
            Asked::Unasked | Asked::Typed => {}
        }
        self.outbox.push(self.term.reply());
        self.term.clear_reply();
        if self.outbox.is_empty() {
            return Sent::Nothing;
        }
        // SD lights for a write the kernel took any of, never for bytes
        // still owed; a write it refused any of darkens CS, the line no
        // longer clear.
        match self.pty.write(&mut self.outbox) {
            Wrote::All => {
                self.activity.seen(Led::Sd, now);
                Sent::Bytes
            }
            Wrote::Part => {
                self.activity.seen(Led::Sd, now);
                self.activity.seen(Led::Cs, now);
                Sent::Nothing
            }
            Wrote::Nothing => {
                self.activity.seen(Led::Cs, now);
                Sent::Nothing
            }
        }
    }

    /// **The link preview read again** while the pointer is over the
    /// text (`link::Hover::refresh`): on the view shown now, with the
    /// modifiers AppKit says are held now, `NSEvent.modifierFlags`. A
    /// press or release of a modifier key alone is read as it happens
    /// (`modifiers_changed`); this read, at each tick, follows the view as
    /// output moves it under a held chord. Whether what the frame draws
    /// changed.
    fn read_hover(&mut self) -> bool {
        if !self.hover.over_text() {
            return false;
        }
        let flags = NSEvent::modifierFlags_class();
        // Command, and not Shift, which is the selection's.
        let chord = match flags.contains(NSEventModifierFlags::Command)
            && !flags.contains(NSEventModifierFlags::Shift)
        {
            true => crate::link::Chord::Cmd,
            false => crate::link::Chord::Other,
        };
        self.read_hover_as(chord)
    }

    /// The link preview read again with `chord` held: what `read_hover`
    /// does with the modifiers it reads.
    fn read_hover_as(&mut self, chord: crate::link::Chord) -> bool {
        if !self.hover.over_text() {
            return false;
        }
        let host = self.preview_host(chord);
        let changed = self.hover.refresh(
            &self.term,
            self.scrolled,
            chord,
            self.focus,
            crate::link::Host(&host),
        );
        if changed {
            self.reshape_pointer();
        }
        changed
    }

    /// **The pointer over the text**: the pointing hand while the link
    /// preview shows a link a click opens, else the shape a program named
    /// with `OSC 22` (`link::Hover::pointer`), as the nearest system
    /// cursor (`pointer.rs`), set only when it changes and never for a
    /// session that names none and points at no link.
    fn show_pointer(&mut self) {
        use crate::pointer::{Shown, system_pointer, to_set};
        let want = self
            .hover
            .pointer(self.term.pointer_shape())
            .map(system_pointer);
        if let Some(p) = to_set(want, self.pointer_shown) {
            system_cursor(p).set();
        }
        self.pointer_shown = match want {
            Some(w) => Shown::Set(w),
            None => Shown::System,
        };
    }

    /// A view point in frame pixels: the frame is centred in the surface
    /// and the thumb's geometry is in frame pixels, so a point comes in
    /// from view pixels less the letterbox margin.
    fn frame_px(&self, x: f64, y: f64) -> (Px, Px) {
        let scale = self.scale;
        let at = |v: f64| Px::of(u32::try_from((v * scale).max(0.0) as i64).unwrap_or(0));
        let (px, py) = self.pad_px;
        (
            Px::of(at(x).value().saturating_sub(px.value())),
            Px::of(at(y).value().saturating_sub(py.value())),
        )
    }

    /// What the scrollbar's press rules read besides the event (`bar.rs`).
    fn bar_ctx(&self) -> crate::bar::Ctx {
        crate::bar::Ctx {
            scroller: self.scroller,
            shown: self.bar_shown,
            frame: crate::bar::Frame {
                size: self.renderer.frame_size(),
                cell_h: self.renderer.metrics().cell_h,
                widths: crate::reveal::widths(crate::reveal::Scale(self.scale)),
            },
            // A press is tested against what is on the screen: the look
            // the last frame drew, not the one the hover will draw.
            drawn: self.drawn_look,
            now: HostTime::of_secs(self.tick_target),
        }
    }

    /// The pointer shown again after what decides it changed -- a read, the
    /// link preview -- while the pointer is over the window; outside it,
    /// the next move in shows it.
    fn reshape_pointer(&mut self) {
        if self.pointer_shown != crate::pointer::Shown::Unknown {
            self.show_pointer();
        }
    }

    /// **The host the preview admits a `file:` link against**,
    /// `NSProcessInfo.hostName`, read once when Command goes down and kept
    /// while it is held: the display-link tick reads the preview on every
    /// frame while Command is held, and the host is asked once a press.
    fn preview_host(&mut self, chord: crate::link::Chord) -> String {
        match chord {
            crate::link::Chord::Cmd => self
                .hover_host
                .get_or_insert_with(baud_present_macos::link::host_name)
                .clone(),
            crate::link::Chord::Other => {
                self.hover_host = None;
                String::new()
            }
        }
    }

    /// **Keeps the old screen for a screen switch's slide**, drawn with its
    /// cursor hidden, as sfte draws its outgoing screen: the last frame
    /// drawn, unslid, with what the cursor brought taken off. On the CPU
    /// path its rows under the cursor are painted again now and the frame
    /// buffer copied; on the instance path they are converted again now,
    /// from the renderer's copy of them; on the GPU paths the frame is
    /// drawn before the next one ([`App::draw_kept`]).
    fn keep_frame(&mut self) {
        match (
            self.presenter.as_ref().map(Presenter::mode),
            self.cell.is_some(),
        ) {
            (Some(Mode::Gpu), cells) => {
                if !cells {
                    self.renderer.hide_cursor(&self.term);
                }
                self.keep = Keep::Owed;
            }
            _ => {
                let (w, h) = (self.frame_w, self.frame_h);
                if let Some(mut target) = Target::new(&mut self.frame, w, w, h) {
                    self.renderer.render_hidden(&self.term, &mut target);
                }
                self.kept_frame.clone_from(&self.frame);
                self.slid = Composed::Yes;
            }
        }
    }

    /// **The kept frame a screen switch owes, drawn before anything of the
    /// next frame is set** ([`App::keep_frame`]): the instance path's
    /// renderer as the switch left it, or the cell path's last frame built
    /// again from its set with no cursor, no trail, no blink animation and
    /// no cursor guide -- the window copied into the set read as a view of
    /// its own, walked again for the records the cursor's block swapped,
    /// and the records after the walk's the last frame's, but the trail's
    /// and the blink animation's. While a frame is in flight nothing is
    /// drawn and the completion wakes the responder, as for any frame.
    fn draw_kept(&mut self) -> KeptDraw {
        if self.keep == Keep::Done {
            return KeptDraw::Build;
        }
        let waker = self.waker;
        let Some(p) = self.presenter.as_mut().filter(|p| p.mode() == Mode::Gpu) else {
            self.keep = Keep::Done;
            return KeptDraw::Build;
        };
        if p.in_flight() == InFlight::Busy {
            self.owed_in_flight.bump();
            return KeptDraw::Wait;
        }
        let drawn = match self.cell.as_mut() {
            None => p.keep_frame(waker, &mut self.renderer),
            Some(cell) => match &cell.last {
                None => None,
                Some(b) => {
                    let set = b.at.0;
                    let n = b.rows.index() * b.stride.index();
                    let view = baud_domain::Viewport::new(
                        &cell.copies_mem[set][..n.min(cell.copies_mem[set].len())],
                        &b.used,
                        b.stride,
                        b.cols,
                        b.rows,
                        b.rows,
                        RowRot::NONE,
                    );
                    self.renderer.set_trail(None);
                    self.renderer.set_cursor_glow_at(None);
                    self.renderer.place_grounds(None, None);
                    let mut records = Vec::new();
                    cell.exceptions.ensure(
                        &Scene {
                            term: &self.term,
                            view,
                            styles: &cell.styles_mem[set][..],
                            cursor: None,
                            highlight: b.highlight,
                            map: &cell.maps.live,
                            candidates: &cell.candidates,
                            pan: b.slide.unwrap_or(Px::of(0)),
                            composition: b.composition.as_ref(),
                        },
                        &mut self.renderer,
                        &mut cell.overlay_mem[set][..],
                        &mut records,
                    );
                    let under = cell.exceptions.under();
                    let walked = usize::try_from(b.walked.value()).unwrap_or(usize::MAX);
                    let (ma, mb) = b.moving;
                    let tail = cell.records.get(walked..).unwrap_or(&[]);
                    let (ma, mb) = (ma.saturating_sub(walked), mb.saturating_sub(walked));
                    let moving = records.len() + ma.min(tail.len());
                    records.extend_from_slice(&tail[..ma.min(tail.len())]);
                    records.extend_from_slice(&tail[mb.min(tail.len())..]);
                    let grounds = self.renderer.badge().to_vec();
                    let input = CellInput {
                        grid: &cell.copies[set],
                        styles: &cell.styles[set],
                        map: cell.maps.buffer(),
                        stride: b.stride,
                        rotation: RowRot::NONE,
                        ring: b.ring,
                        cursor: None,
                        cursor_drawn: cell.exceptions.cursor_drawn(),
                        highlight: b.highlight,
                        overlay: &cell.overlay[set],
                        records: &records,
                        grounds: &grounds,
                        under,
                        chrome: b.chrome,
                        slide: b.slide,
                        set: b.at,
                        table: b.table,
                        map_gen: b.map_gen,
                        reach: b.reach,
                        moving: (moving, moving),
                        screen: b.screen,
                    };
                    p.keep_cells(waker, &mut self.renderer, &input)
                }
            },
        };
        match drawn {
            Some(Drawn::Busy) => KeptDraw::Wait,
            Some(Drawn::Frame(_)) => {
                self.keep = Keep::Done;
                KeptDraw::Wait
            }
            None => {
                self.keep = Keep::Done;
                KeptDraw::Build
            }
        }
    }

    /// **The plasma's pipelines started on a thread of their own**: the
    /// launch logo's, and a plasma trail's in front of the text, from the
    /// device and library the window's first frame was drawn with. Called
    /// once the window is shown, so nothing on the way to its first frame
    /// builds them; none on the CPU path, which has none to build.
    fn build_plasma(&mut self) {
        let build = self
            .presenter
            .as_ref()
            .and_then(|p| p.effect_build(baud_render::effect::Effect::Plasma));
        if let Some(b) = build {
            self.plasma = Some(std::thread::spawn(move || b.build()));
            startup_mark("plasma building");
        }
    }

    /// **Whether the plasma is ready for the logo**: its pipelines taken
    /// once their thread has ended, never waited for, so a tick before
    /// then starts no logo and draws everything else; ready at once with
    /// nothing building.
    fn plasma_ready(&mut self) -> bool {
        match self.plasma.take() {
            None => true,
            Some(h) if !h.is_finished() => {
                self.plasma = Some(h);
                false
            }
            Some(h) => {
                if let (Ok(built), Some(p)) = (h.join(), self.presenter.as_mut()) {
                    p.adopt(built);
                }
                startup_mark("plasma built");
                true
            }
        }
    }

    fn present(&mut self, screen: &DrawableView, show: Show) {
        // Row 11's ceiling arm: what an idle tick would cost if the
        // preamble below were free. Incorrect by construction -- the
        // preamble is where a geometry change is noticed -- so it is a
        // bound on the saving rather than a change.
        #[cfg(feature = "ceiling-idle-skip")]
        if self.pending == Pending::Clean {
            return;
        }
        // A screen switch's kept frame, before anything below sets what the
        // next frame draws over the renderer's state, which is still the
        // old screen's: drawn, it is in flight, and this frame is built
        // when it lands.
        if self.draw_kept() == KeptDraw::Wait {
            return;
        }
        // The surface is the renderer's size, so the renderer follows the
        // terminal first; the cell path never converts and would otherwise
        // keep the grid it started with.
        self.renderer.fit(&self.term);
        // The selection's share of this view, for every path's one rule --
        // or, while a program's `?1001` region tracks the mouse, that region
        // in its place (`tracked_highlight`).
        let highlight = {
            let back = self.term.window_back();
            let view = self.term.view(back);
            let top = self.scrolled.top(back);
            match self.term.highlight() {
                Some(_) => tracked_highlight(&self.term, self.scrolled, back),
                None => {
                    let wrap = self.renderer.word_wrap();
                    let beyond = crate::keep::beyond(&self.term, back, wrap);
                    self.selection
                        .and_then(|s| s.highlight_reaching(top, view.rows(), view.cols(), beyond))
                }
            }
        };
        self.renderer.set_highlight(highlight);
        // iTerm2's cursor guide: what a program set, else `[cursor] guide`.
        let guide = self.term.cursor_guide().unwrap_or(self.cursor_guide);
        self.renderer
            .set_cursor_guide(crate::effects::guide_look(guide, self.scale));
        // iTerm2's badge, a program's, unless `[screen] badge` is off; a
        // `selection` it names reads what a copy would copy.
        let wrap = self.renderer.word_wrap();
        let selection = || {
            crate::keep::copied(
                self.selection.as_ref(),
                &self.kept,
                &self.term,
                self.scrolled,
                wrap,
            )
        };
        let badge = crate::effects::badge_look(
            &self.term,
            self.badge,
            &self.session,
            &selection,
            self.renderer.background_of(&self.term),
            self.scale,
        );
        self.renderer.set_badge(badge);
        // The find field and every match's mark, on the window drawn, and
        // the input manager's composition in the field while it is open.
        // AppKit hands no hints, and its selected range does not reach
        // here (`marked.rs`), so the cursor is after the composition.
        let look = self.finding.as_ref().map(|f| {
            f.look_typing(
                &self.term,
                self.term.window_back(),
                &self.composing,
                &[],
                baud_render::Caret::End,
            )
        });
        self.renderer.set_find(&self.term, look);
        // The link preview, read again under the pointer on the view shown
        // now: a read or a wheel may have changed what is under it since
        // the tick read it (`Driver::poll_hover`).
        if self.read_hover() {
            self.pending = Pending::Dirty;
        }
        self.renderer.set_link(self.hover.look().cloned());
        // The hover text over DEC's line's words, read again on the line
        // as it is now (`line_tip.rs`).
        if self.line_tip.refresh(&self.term) == crate::line_tip::Changed::Yes {
            self.pending = Pending::Dirty;
        }
        self.renderer.set_tip(self.line_tip.look().cloned());
        // The scrollbar's thumb, where the window sits in the ring, seen as
        // much as the reveal says at the display link's last target.
        let look = HostTime::of_secs(self.tick_target).map_or(Look::HIDDEN, |now| self.look(now));
        self.drawn_look = look;
        // The thumb this frame draws decides who a left press goes to, so
        // the pointer over a link under its strip is read again with it.
        let left = crate::bar::left_press(&self.term, self.strip, self.bar_ctx());
        if self.hover.set_left_press(left) {
            self.reshape_pointer();
        }
        // None while a program has hidden it (`?30`).
        let bar = match self.bar_shown {
            baud_core::ScrollbarShown::Shown => Some(ScrollBar {
                window: self.window(),
                look,
                widths: crate::reveal::widths(Scale(self.scale)),
                scroller: self.scroller,
            }),
            baud_core::ScrollbarShown::Hidden => None,
        };
        self.renderer.set_scrollbar(bar);
        // The cursor trail, spawned here, where the post-feed cursor is
        // known and before any path converts; advanced and cleared off
        // `tick_target`, the display link's own clock, so the renderer only
        // draws what it is handed and the frame is a pure function of it.
        // The quad's rules are `trail.rs`'s; the ghost sweeps on any move.
        // The cursor it follows is `trail_cursor`'s: the one drawn, or where a
        // program placed a cursor it hid and draws its own.
        let ridden = core::mem::replace(&mut self.ridden, Rows::ZERO);
        let trail_look = frame_trail(
            (self.cursor_trail, self.trail_scroll),
            &self.term,
            &self.renderer,
            self.scale,
            self.tick_target,
            (&mut self.chase, &mut self.trail, &mut self.last_cursor_cell),
            (&mut self.carried, ridden),
        );
        // The alternate-screen switch: the new screen sliding in over the
        // old one, or the veil under Reduce Motion, advanced and cleared off
        // `tick_target` like the trail. Settled, this frame draws neither --
        // the veil's end repaints the screen once, the slide's goes to the
        // surfaces whole -- and the tick stops forcing frames.
        let switch = self
            .transition
            .and_then(|st| switch_at(st.start, st.form, self.tick_target));
        if switch.is_none() {
            self.transition = None;
        }
        let slide = switch.and_then(|f| f.slide);
        self.renderer.set_screen_fade(switch.and_then(|f| f.veil));
        if let Some(p) = self.presenter.as_mut() {
            p.set_slide(slide);
        }
        // The launch splash, off `tick_target` like the fade: waiting for
        // the window, emerging, held, vanishing or gone. The synchronous
        // first frame comes before the window is shown, so it draws nothing
        // of the logo -- or the held logo, under reduced motion -- and
        // starts no clock.
        let now = self.last_tick.map(|_| self.tick_target);
        let splash = self.splash.look(now, &self.term, self.motion);
        if let Some(baud_render::SplashLook {
            phase: baud_render::SplashPhase::Plasma(_),
        }) = splash
        {
            startup_first("splash first frame", &FIRST_SPLASH);
        }
        let logo = match splash {
            Some(_) => crate::splash::Logo::Drawn,
            None => crate::splash::Logo::None,
        };
        self.renderer.set_splash(splash);
        // iTerm2's fireworks, off `tick_target` like the trail: a show a
        // request started, held and gone on the display link's clock. A
        // frame before the first tick has no reading, so a request waits
        // for one, as the splash's clock does.
        let m = self.renderer.metrics();
        let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let cells = crate::fireworks::Cells {
            w: side(m.cell_w),
            h: side(m.cell_h),
            back: self.term.window_back().index(),
        };
        let shows = match self.last_tick {
            Some(_) => self.fireworks.frame(
                self.tick_target,
                &self.term,
                cells,
                self.scale as f32,
                self.motion,
            ),
            None => Vec::new(),
        };
        self.renderer.set_fireworks(&shows);
        // The input manager's composition, at the cursor of the live view:
        // the instance and CPU paths draw it in their convert, the cell path
        // through its overlay below -- but in the find field while it is
        // open, which takes the keys.
        // AppKit's selected range does not reach here (`marked.rs`), so
        // the cursor is after the composition.
        let composition = match self.finding {
            Some(_) => None,
            None => crate::marked::composition(
                &self.term,
                &self.composing,
                &[],
                baud_render::Caret::End,
            ),
        };
        self.renderer.set_composition(composition.clone());
        // The frame -- the grid, and a legacy scroller's gutter beside it --
        // and the whole surface it is centred in; the letterbox margin
        // between them is the gap.
        // The window's padding lent to the word wrap's marks, a band each
        // side (`Renderer::set_margins`): as wide as the padding, or what
        // the letterbox leaves where a pinned grid fills more of the view.
        // The grid keeps its place and every column.
        let (sw, sh) = self.view_px;
        let band = margin_band(sw, &self.renderer, self.padding, self.scale);
        self.renderer.set_margins(band);
        // The whole frame -- the left margin, the exit-status strip, when
        // kept, then the grid and the gutter, then the right margin -- is
        // centred; the pad is the grid's origin, the margin and the strip
        // in from the letterbox.
        let (w, h) = self.renderer.whole_size();
        self.pad_px = grid_origin(
            (sw, sh),
            (w, h),
            self.renderer.grid_left(),
            self.renderer.grid_top(),
        );
        let surface_stale = self.presenter.as_ref().map(Presenter::size) != Some((sw, sh));
        if (w, h) != (self.frame_w, self.frame_h) || surface_stale || self.presenter.is_none() {
            self.frame_w = w;
            self.frame_h = h;
            self.frame = vec![0; slots(w) * slots(h)];
            // **A new frame holds nothing, so it is painted whole.** The CPU
            // arm paints only the rows its damage names, and a sub-cell resize
            // reallocates the frame without changing the grid, which is the
            // one thing `relayout` forces a full frame for: every row that
            // did not change was presented as zeros, transparent. Read below
            // in this same call.
            self.next_damage = Damage::Full;
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
        // **A frame in flight: this one is owed, and nothing is built**
        // (`builds`). After the surfaces above, so a drag step that
        // replaced them has superseded the frame in flight, drawn for the
        // size the window left, and it is never shown; before the styles,
        // the map walk, the copy, the overlay and the convert below, which
        // `draw_cells` would refuse. `pending` stays set, and the
        // completion's wake or the next tick builds it at the size the
        // window has then. `pump` holds the same gate, so this is reached
        // by the callers of `present` outside it: `relayout`, `zoom` and
        // the rest.
        if builds(self.presenter.as_ref().map(Presenter::in_flight)) == Builds::Owed {
            self.owed_in_flight.bump();
            return;
        }
        self.renderer.set_blink(self.blink);
        // The visual bell, decided before any path reads the video, at the
        // moment the frame can first be seen: the display link's target if
        // it is still ahead, or now. Kept by `commit` if an arm shows the
        // frame, and in part by `refused` if its present fails, below. The
        // update state is the clock's as the pump last left it: a resize or a
        // zoom draws from outside the pump's hold check, and the clock is what
        // says whether the update open now is honoured.
        let now = host_now();
        let seen_at = HostTime::of_secs(self.tick_target).map_or(now, |t| t.max(now));
        let update = self.updates.state(&self.term, Instant::now());
        let bell = self.bell.plan(&self.term, seen_at, update);
        self.renderer.set_flash(bell.flash);
        self.renderer.set_bell_tint(bell.tint);
        // The cursor's blink phase at the same moment, kept with the bell's
        // plan when an arm shows the frame.
        let blinks = crate::caret::blinks(
            self.term.cursor_style(),
            self.focus,
            live_cursor(&self.term),
        );
        // The cursor this frame presents restarts the cycle if it is not
        // the one the last frame did (`Caret::present`).
        self.caret.present(seen_at, PresentedCursor::of(&self.term));
        let caret = self.caret.plan(seen_at, blinks);
        self.renderer.set_cursor_blink(caret);
        // Image placements for this view, for the two GPU arms: both read the
        // one list `set_images` builds, so an image lands where its line is.
        // The CPU arm's `render_at` places them itself, without the upload
        // snapshots a GPU needs and it does not -- a second converted copy of
        // every visible image, kept for nothing.
        if self.presenter.as_ref().map(Presenter::mode) == Some(Mode::Gpu) {
            self.renderer
                .set_images(&self.term, self.term.window_back());
        }
        let damage = self.next_damage;
        self.next_damage = Damage::Incremental;
        let waker = self.waker;
        // Whether this frame reached the screen, for the bell: a present
        // that failed drops the frame's content, and a flash it planned
        // with it.
        let mut shown = Presented::No;
        // **The slide's pan, read once**: the trail is lifted by it, and the
        // cell arm pans the grid, the overlay records and the leaving row by
        // it, so none of them can disagree. A trail is lifted only where the
        // grid slides -- the cell path, at the live screen, on the primary
        // (`slides`); where the grid jumps, the ride alone moved it
        // (`ride_trail`), and under `stay` it draws where the screen had it.
        let pan = self.slide.pan();
        let drawn = SlideDrawn::of(
            self.presenter.as_ref().map(Presenter::mode),
            self.cell.as_ref(),
        );
        let sliding = slides(
            self.scroll,
            self.term.window_back(),
            self.term.screen(),
            drawn,
            below_grid(&self.term, &self.renderer),
        ) && pan.value() > 0;
        let lift = if sliding { pan } else { Px::of(0) };
        // Over none of what the shell has drawn: its output is drawn as
        // with no logo from the frame it arrives on (`splash.rs`), where
        // the slide draws it, lowered by `lift`.
        self.renderer.splash_yields_to(&self.term, lift);
        // No trail while the screen slides, as sfte draws none.
        self.renderer.set_trail(
            trail_look
                .filter(|_| slide.is_none())
                .map(|l| l.lifted(trail_lift(self.trail_scroll, lift))),
        );
        // The exit-status marks on the window shown, for every path: on the
        // cell path's slide they ride with the grid, its leaving row too.
        let leaving = match sliding {
            true => baud_render::Leaving::Drawn,
            false => baud_render::Leaving::None,
        };
        self.renderer
            .place_marks(&self.term, self.term.window_back(), leaving, lift);
        // The activity lights on their own line, for every path; it holds
        // still while the grid slides.
        self.renderer.place_lights(&self.term);
        match (
            self.presenter.as_ref().map(Presenter::mode),
            self.cell.as_mut(),
        ) {
            (Some(Mode::Gpu), Some(cell)) => {
                let t0 = Instant::now();
                // The window `back` lines up the ring: the shader's rotation
                // retreats by that and reads the retained rows where they
                // are, so a scrolled-back frame costs what a live one costs.
                let back = self.term.window_back();
                // The window in the order each row is drawn in, laid out once
                // for this frame: the core's own view when no row is
                // reordered. With BDSM reset, once a letter that takes a
                // joining form has been drawn, the last frame's joins are
                // kept for the rows the copy's compare finds changed.
                // Row 9's predicted echoes, on the live view, are laid out
                // as written, so a prediction's row is drawn in the order
                // its echo will be.
                let joins_seen = self.renderer.joins_seen();
                self.renderer.set_predicted(
                    self.predictions
                        .iter()
                        .filter(|_| back.index() == 0)
                        .map(|p| (p.row, p.col, p.cp)),
                );
                let rejoin = self.renderer.lay_out_bidi_copied(&self.term, back);
                let view = self.renderer.bidi().window(
                    &self.term,
                    back,
                    &mut cell.bidi_cells,
                    &mut cell.bidi_used,
                );
                // Smooth scroll: at the live screen on the primary surface, a
                // non-zero pan draws one leaving row above the window.
                // The leaving row's glyphs are one row up, not in `view`, so
                // ensure the map over the view one line up too -- the
                // persistent map holds them from when the row was on screen,
                // and this covers a clear. It is ensured *first*, because
                // `ensure` clears `candidates` and refills them for the view
                // it walks (`cellmap`): the screen must be ensured last, or
                // the overlay's records name `above`'s rows -- the whole
                // screen one line up -- and every mark and ligature is drawn
                // a row off its cell for the length of the slide.
                // Which buffers this frame may write: any set no frame in
                // flight was committed with. `None` would mean every one is
                // held and there is nowhere to build that a live draw is not
                // reading -- which cannot happen while one frame is in
                // flight and three sets exist, and is a refusal rather than
                // a corruption if it ever does.
                let Some(at) = self.presenter.as_ref().and_then(Presenter::building) else {
                    return;
                };
                let set = at.0;
                // Whether this set's copy holds ids an earlier frame lent,
                // read before they are given back.
                let lent_before = cell.lent[set].lent().value() > 0;
                // The slots this set lent its last frame's frozen rows, put
                // back first: no frame in flight reads this set.
                self.renderer.give_back(
                    &mut cell.styles_mem[set][..],
                    &mut cell.style_stamps[set],
                    &mut cell.lent[set],
                );
                // The set's resolved table, brought up to date from the core's
                // log: only the slots written since this set was last built,
                // which on a steady screen is none. Before the map
                // walk, which reads each cell's face style from it.
                self.renderer.sync_styles(
                    &self.term,
                    &mut cell.styles_mem[set][..],
                    &mut cell.style_stamps[set],
                );
                // **The leaving row**: the line just above the window,
                // `view(back + 1).row(0)`, appended to the copy at index
                // `vrows`, where `slide_rows = 1` maps draw row 0 to it with
                // the identity rotation the copy is read under -- in the
                // order it was drawn in as the first row of the window one
                // line up, a marked cell given its base, as the shader reads
                // it (`Renderer::copy_leaving`). It is never selected and
                // never holds the cursor (the shader's row there is past
                // every one). The map is ensured over that row as copied,
                // and what that walk finds kept for the row's own records
                // (`Overlay::ensure_leaving`).
                if sliding {
                    let (vcols, vrows) = (view.cols().index(), view.rows().index());
                    let at = vrows * self.term.stride().index();
                    let row = &mut cell.copies_mem[set][at..at + vcols];
                    self.renderer.copy_leaving(&self.term, back, row);
                }
                let cursor_at =
                    live_cursor(&self.term).map(|(r, c)| (r, c, self.term.cursor_style()));
                // Row 9: the drawn cursor sits after the last predicted
                // glyph, for the shader and for the overlay's own paint.
                let cursor_at = match (cursor_at, self.predictions.last()) {
                    (Some((_, _, style)), Some(p)) => Some((p.row, p.col.after(), style)),
                    (c, _) => c,
                };
                // A composition puts the drawn cursor after itself, as the
                // instance and CPU paths do in their convert.
                let cursor_at = match (cursor_at, &composition) {
                    (Some((_, _, style)), Some(c)) => {
                        c.cursor(c.width_on(&view)).map(|(r, col)| (r, col, style))
                    }
                    (c, _) => c,
                };
                // **Whether this frame's screen is the last frame's**: its
                // rows, its status line and its line renditions against the
                // copy that frame left, row for row; never while sliding,
                // whose copy holds a leaving row and whose walk another
                // window. **D4: the window, copied into this frame's set**,
                // unless that set's copy holds the screen already -- a
                // frame that changed only the cursor, built into a set an
                // earlier one copied the same screen into. The rows the view
                // names, in screen order, so what the shader reads is a
                // still image of the screen rather than a grid the parser is
                // still writing -- which is what lets `pump` keep feeding
                // while this frame is in flight.
                let stride = self.term.stride().index();
                let (vcols, vrows) = (view.cols().index(), view.rows().index());
                // The status line in its drawn order, the row after the
                // window's in the renderer's maps.
                let mut status_cells = Vec::new();
                let status = self.term.status_line().map(|line| {
                    self.renderer.bidi().status_line(
                        Row::from_index(vrows),
                        line,
                        &mut status_cells,
                    )
                });
                let lined = usize::from(status.is_some());
                let screens = &mut cell.screens;
                screens.compare = Compare::for_rejoin(rejoin);
                // **Whether a cell of this frame names a row the core froze**
                // (`baud_render::Lent`): the window's, a display line's
                // among them, or the leaving row's. No row at all while the
                // core has frozen nothing, which one test answers.
                let leaving_source =
                    sliding.then(|| self.renderer.leaving_source(&self.term, back));
                let frozen = self.renderer.bidi().window_frozen(&self.term, back)
                    || leaving_source
                        .as_ref()
                        .is_some_and(|s| !s.words().is_live());
                // A window that shows a frozen row is copied afresh, its ids
                // the core's, and so is one whose copy an earlier frame lent
                // slots: that copy holds ids that named them, put back now.
                if frozen || lent_before {
                    screens.forget(at);
                }
                let built = screens.take(
                    &mut cell.copies_mem,
                    at,
                    &view,
                    status.as_ref(),
                    self.term.stride(),
                    leaving,
                );
                // **A frozen row of the window drawn in its own renditions**
                // (`baud_render::Lent`): its copied cells given slots no row
                // of the frame names, and the walk and the overlay reading the
                // copy, so all three resolve each id in the one table. A
                // frame with no frozen row asks one test a row.
                let lend = match frozen {
                    false => baud_render::Lend::Nothing,
                    true => {
                        // Each window row through the table each of its cells
                        // names, in the order the copy holds them; the row
                        // after the window's the leaving row while sliding,
                        // else the status line's, and the notice's after it.
                        let sources: Vec<baud_render::RowSource<'_>> = (0..vrows)
                            .map(|y| {
                                self.renderer.bidi().row_source(
                                    &self.term,
                                    back,
                                    Row::from_index(y),
                                )
                            })
                            .collect();
                        let mut rows: Vec<(&mut [PackedCell], baud_render::Words<'_>)> = Vec::new();
                        for (y, chunk) in cell.copies_mem[set].chunks_mut(stride).enumerate() {
                            let words = match (sources.get(y), &leaving_source) {
                                (Some(s), _) => s.words(),
                                (None, Some(s)) if y == vrows => s.words(),
                                _ => baud_render::Words::LIVE,
                            };
                            if y > vrows + 1 {
                                break;
                            }
                            let n = vcols.min(chunk.len());
                            rows.push((&mut chunk[..n], words));
                        }
                        self.renderer.lend_frozen(
                            &self.term,
                            &mut rows,
                            &mut cell.styles_mem[set][..],
                            &mut cell.style_stamps[set],
                            &mut cell.lent[set],
                        )
                    }
                };
                // What the walk, the overlay and a sized line read the copy
                // through: the words its lent slots were given.
                self.renderer.set_frame_lent(&cell.lent[set]);
                // A lent copy names other ids than the last frame's copy
                // did, so no compare of the two says the screen is the same.
                let built = match lend {
                    baud_render::Lend::Nothing => built,
                    _ => Built::New,
                };
                // **The leaving row's map ensured**, over the row as copied
                // and lent: before the screen's walk below, which clears the
                // candidates and refills them for the view it walks.
                if sliding {
                    let at = vrows * stride;
                    let row = &cell.copies_mem[set][at..at + vcols];
                    self.renderer.leaving_lent(row);
                    let used = [baud_domain::RowExtent::of(vcols)];
                    let leaving = baud_domain::Viewport::new(
                        row,
                        &used,
                        baud_domain::Stride::of_cells(vcols),
                        view.cols(),
                        Rows::of_count(1),
                        Rows::of_count(1),
                        RowRot::NONE,
                    );
                    cell.exceptions.ensure_leaving(
                        &mut cell.maps.live,
                        &self.term,
                        leaving,
                        &cell.styles_mem[set][..],
                        &mut self.renderer,
                    );
                }
                let view = match lend {
                    baud_render::Lend::Nothing => view,
                    _ => {
                        cell.lent_cells.clear();
                        cell.lent_cells
                            .extend_from_slice(&cell.copies_mem[set][..vrows * stride]);
                        cell.lent_used.clear();
                        for y in 0..vrows {
                            let r = Row::from_index(y);
                            let mut e = baud_domain::RowExtent::of(vcols)
                                .with_rendition(view.rendition(r), view.cols());
                            if view.wrapped(r) == baud_domain::Wrap::Continues {
                                e = e.wrapping(vcols);
                            }
                            cell.lent_used.push(e);
                        }
                        baud_domain::Viewport::new(
                            &cell.lent_cells,
                            &cell.lent_used,
                            self.term.stride(),
                            view.cols(),
                            view.rows(),
                            view.rows(),
                            RowRot::NONE,
                        )
                    }
                };
                // The kept joins found again on the rows that compare found
                // changed, before the walk and the records read the forms.
                if let Some(how) = rejoin {
                    self.renderer
                        .rejoin_bidi(&self.term, back, how, |r| screens.changed(r));
                }
                let key = WalkKey {
                    table: cell.style_stamps[set],
                    map: cell.maps.live.generation(),
                    grown: self.renderer.atlas_stats().grown,
                    clusters: self.term.cluster_epoch(),
                    back,
                    cols: view.cols(),
                    rows: view.rows(),
                };
                if built == Built::New || screens.walked != Some(key) {
                    cell.maps.live.ensure(
                        &self.term,
                        view,
                        &cell.styles_mem[set][..],
                        &mut self.renderer,
                        &mut cell.candidates,
                    );
                    screens.walked = (!sliding).then(|| WalkKey {
                        map: cell.maps.live.generation(),
                        grown: self.renderer.atlas_stats().grown,
                        ..key
                    });
                }
                // **The first letter that takes a joining form**, shaped by
                // the walk with BDSM reset, was laid out unjoined, since no
                // row is read for joins before one is drawn: laid out again,
                // before the records draw the forms. Once a renderer's life.
                self.renderer.join_after_walk(&self.term, back, joins_seen);
                let cursor = cursor_at.filter(|&(_, _, style)| self.renderer.cursor_drawn(style));
                // **The config's notice row, after the status line's**:
                // blank cells, whose default background the shader draws,
                // under the notice's field drawn with baud's own overlays
                // below (`Renderer::notice_bar`). No slide runs while it is
                // shown (`below_grid`). Filled on every frame, the copy of
                // the screen skipped or not: a set that holds the screen may
                // not hold the notice's row.
                let noticed = match self.renderer.notice_bar() {
                    Some(_) => {
                        let at = (vrows + lined) * stride;
                        match cell.copies_mem[set].get_mut(at..at + vcols) {
                            Some(row) => {
                                row.fill(PackedCell::EMPTY);
                                1
                            }
                            None => 0,
                        }
                    }
                    None => 0,
                };
                cell.exceptions.ensure(
                    &Scene {
                        term: &self.term,
                        view,
                        styles: &cell.styles_mem[set][..],
                        cursor,
                        highlight,
                        map: &cell.maps.live,
                        candidates: &cell.candidates,
                        // Smooth scroll: the records slide with the grid,
                        // gated on the same `sliding` as the grid's own pan
                        // so a mark or a ligature never lags a settled frame.
                        pan: if sliding { pan } else { Px::of(0) },
                        composition: composition.as_ref(),
                    },
                    &mut self.renderer,
                    &mut cell.overlay_mem[set][..],
                    &mut cell.records,
                );
                let walked = cell.records.len();
                // Row 9: the predicted echoes, each a masked cell and a
                // record in the rendition of the cell to its left, on the
                // live view only. A cell the walk above already drew a
                // record for -- a mark, or a glyph inside a ligature
                // window -- is left to that record: two records for one
                // cell would draw two glyphs on top of each other, and the
                // echo is a frame away.
                if back.index() == 0 {
                    for p in &self.predictions {
                        // The rendition is the cell to the left's, Mosh's
                        // heuristic: what was typed before this is what
                        // this will look like, where the cell under the
                        // cursor may hold the shell's own dimmed guess.
                        // The logical cell, read from the core's view: the
                        // window drawn may show another cell there.
                        let at = if p.col.index() > 0 {
                            p.col.index() - 1
                        } else {
                            p.col.index()
                        };
                        let style = self
                            .term
                            .view(back)
                            .row(p.row)
                            .get(at)
                            .map_or(PackedCell::EMPTY.style, |c| c.style);
                        cell.exceptions.predicted(
                            &mut self.renderer,
                            (p.row, p.col),
                            PackedCell {
                                content: Content::codepoint(p.cp),
                                style,
                                flags: CellFlags::NONE,
                            },
                            &cell.styles_mem[set][..],
                            cursor,
                            highlight,
                            view.cols(),
                            &mut cell.overlay_mem[set][..],
                            &mut cell.records,
                        );
                    }
                }
                // The composition: its cells masked off the shader and its
                // records -- glyphs and underline -- after the walk's, the
                // same records the instance and CPU paths draw.
                if let Some(c) = &composition {
                    let under = view
                        .row(c.row)
                        .get(self.renderer.bidi().visual_col(c.row, c.col).index())
                        .copied()
                        .unwrap_or(PackedCell::EMPTY);
                    cell.exceptions.composed(
                        &mut self.renderer,
                        &self.term,
                        (c, baud_render::Laid::AsWritten),
                        under,
                        &cell.styles_mem[set][..],
                        cursor,
                        view.cols(),
                        view.rendition(c.row),
                        if sliding { pan } else { Px::of(0) },
                        &mut cell.overlay_mem[set][..],
                        &mut cell.records,
                    );
                }
                // Every match's mark and the link preview's underline, with
                // the content; the find field and the link's bar are baud's
                // own and go over the images, below.
                cell.records.extend_from_slice(self.renderer.find_solids());
                cell.records.extend_from_slice(self.renderer.link_solids());
                // The leaving row: what the map does not draw of it drawn by
                // records, as on a row of the window, and a line that is not
                // single-width drawn whole by them, its row of the mask the
                // one after the screen's (`Overlay::leaving`).
                if sliding {
                    cell.exceptions.leaving(
                        &mut self.renderer,
                        &self.term,
                        (&cell.maps.live, &cell.styles_mem[set][..]),
                        back,
                        (view.cols(), view.rows()),
                        pan,
                        &mut cell.overlay_mem[set][..],
                        &mut cell.records,
                    );
                }
                // The status line's records, its row of the mask marked.
                cell.exceptions.status(
                    &mut self.renderer,
                    &self.term,
                    cursor,
                    (view.cols(), view.rows()),
                    &mut cell.overlay_mem[set][..],
                    &mut cell.records,
                );
                // The trail, thumb and screen fade ride with the records,
                // drawn after the cell passes: the trail under the thumb, and
                // the fade last so it veils everything during a switch.
                let moving_from = cell.records.len();
                cell.records.extend_from_slice(self.renderer.trail());
                // The plasma blink's glow over the cursor's cell, after the
                // trail as `Renderer::instances` has it: built from the
                // cursor before the phase's filter, since the glow is drawn
                // while the cursor is not.
                self.renderer.set_cursor_glow_at(cursor_at);
                cell.records.extend_from_slice(self.renderer.cursor_glow());
                // The badge, which stays where the view has it, and the
                // cursor guide from the same cursor, else on the cursor's
                // line while the window looks back, its records under the
                // glyphs (`CellInput::grounds`).
                self.renderer.place_grounds(
                    cursor_at,
                    baud_render::Renderer::cursor_line(&self.term, back),
                );
                cell.grounds.clear();
                cell.grounds.extend_from_slice(self.renderer.badge());
                self.renderer
                    .cursor_guide_lowered(if sliding { pan } else { Px::of(0) }, &mut cell.grounds);
                let moving = (moving_from, cell.records.len());
                // baud's own overlays, drawn over the images over the text
                // (`CellInput::chrome`): a `BEL`'s tint under the rest; the
                // find field over its cells, masked off the shader as a
                // composition is, and its caret; the link preview's bar the
                // same way; the thumb; the fade.
                let chrome_from = cell.records.len();
                cell.records.extend_from_slice(self.renderer.bell_tint());
                let tips = self
                    .renderer
                    .tip_bars()
                    .into_iter()
                    .map(|c| (Some(c), false));
                let fields = [
                    (self.renderer.find_field(), true),
                    (self.renderer.link_bar(), false),
                ]
                .into_iter()
                .chain(tips)
                .chain([(self.renderer.notice_bar(), false)]);
                for (c, caret) in fields {
                    let Some(c) = c else { continue };
                    cell.exceptions.composed(
                        &mut self.renderer,
                        &self.term,
                        (&c, baud_render::Laid::OnScreen),
                        PackedCell::EMPTY,
                        &cell.styles_mem[set][..],
                        None,
                        view.cols(),
                        baud_domain::LineRendition::Single,
                        Px::of(0),
                        &mut cell.overlay_mem[set][..],
                        &mut cell.records,
                    );
                    if caret {
                        cell.records.extend_from_slice(self.renderer.find_caret());
                    }
                }
                cell.records.extend_from_slice(self.renderer.splash());
                cell.records.extend_from_slice(self.renderer.scrollbar());
                cell.records.extend_from_slice(self.renderer.screen_fade());
                let chrome = Tally::of(cell.records.len() - chrome_from);
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
                        cell.maps.live.stats().misses.value(),
                        cell.exceptions.stats().window_misses.value(),
                        self.stats.convert.as_secs_f64() * 1e3
                    ));
                }
                // What this frame is built from, for a screen switch's kept
                // frame (`draw_kept`).
                let mut used = cell.last.take().map(|b| b.used).unwrap_or_default();
                used.clear();
                used.extend((0..view.rows().index()).map(|y| view.extent(Row::from_index(y))));
                cell.last = Some(CellBuilt {
                    at,
                    cols: view.cols(),
                    rows: view.rows(),
                    used,
                    walked: Tally::of(walked),
                    moving,
                    stride: self.term.stride(),
                    ring: Rows::of_index(vrows + lined + noticed + usize::from(sliding)),
                    highlight,
                    composition: composition.clone(),
                    chrome,
                    slide: sliding.then_some(pan),
                    table: cell.style_stamps[set],
                    map_gen: cell.maps.live.generation(),
                    reach: cell.maps.live.reach(),
                    screen: cell.screens.now(),
                });
                let input = CellInput {
                    // The copy, not the core's grid: flat, in screen order,
                    // so the shader's rotation is the identity and its ring
                    // is the screen. Nothing here depends on which surface
                    // is up or how far back the window is -- the view
                    // resolved both when it was copied.
                    grid: &cell.copies[set],
                    styles: &cell.styles[set],
                    map: cell.maps.buffer(),
                    stride: self.term.stride(),
                    rotation: RowRot::NONE,
                    // One more row when sliding: the appended leaving row.
                    ring: Rows::of_index(vrows + lined + noticed + usize::from(sliding)),
                    cursor,
                    cursor_drawn: cell.exceptions.cursor_drawn(),
                    highlight,
                    overlay: &cell.overlay[set],
                    records: &cell.records,
                    grounds: &cell.grounds,
                    under: cell.exceptions.under(),
                    chrome,
                    slide: sliding.then_some(pan),
                    set: at,
                    table: cell.style_stamps[set],
                    map_gen: cell.maps.live.generation(),
                    reach: cell.maps.live.reach(),
                    moving,
                    // The screen the copy holds, as the record that decided
                    // whether to copy it names it: the GPU's change is
                    // taken against the last frame it committed.
                    screen: cell.screens.now(),
                };
                if let Some(p) = &mut self.presenter {
                    let before = self.latency.stamp(host_now);
                    let drawn = match show {
                        Show::Event => p.draw_cells(waker, &mut self.renderer, &input),
                        Show::Now => p.draw_cells_now(screen, &mut self.renderer, &input),
                    };
                    match drawn {
                        Some(Drawn::Frame(g)) => {
                            shown = Presented::Yes;
                            self.latency.committed(|| before);
                            startup_first("first commit", &FIRST_COMMIT);
                            // Drawn now, the frame is on the layer and
                            // no completion follows it: marked here for
                            // the tick's splash gate (`frame`).
                            if show == Show::Now {
                                self.layer_shown = Done::Yes;
                                startup_first("first present", &FIRST_PRESENT);
                            }
                            if FIRST_BYTES.load(Ordering::Relaxed) {
                                startup_first("first content", &FIRST_CONTENT);
                            }
                            self.gpu_stats = g;
                        }
                        // Still in flight: the completion wakes us. The
                        // presenter gave this frame's tag back, so the set
                        // stays free and no reshape waits on it.
                        Some(Drawn::Busy) => return,
                        None => {}
                    }
                }
            }
            (Some(Mode::Gpu), None) => {
                let converted = self
                    .renderer
                    .convert(&self.term, self.term.window_back(), damage);
                self.stats = Frame {
                    rows_dirty: converted.rows_dirty,
                    rows: converted.rows,
                    convert: converted.convert,
                    ..Frame::default()
                };
                if let Some(p) = &mut self.presenter {
                    let before = self.latency.stamp(host_now);
                    let drawn = match show {
                        Show::Event => p.draw(waker, &mut self.renderer),
                        Show::Now => p.draw_now(screen, &mut self.renderer),
                    };
                    match drawn {
                        Some(Drawn::Frame(g)) => {
                            shown = Presented::Yes;
                            self.latency.committed(|| before);
                            startup_first("first commit", &FIRST_COMMIT);
                            // Drawn now, the frame is on the layer and
                            // no completion follows it: marked here for
                            // the tick's splash gate (`frame`).
                            if show == Show::Now {
                                self.layer_shown = Done::Yes;
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
                    let back = self.term.window_back();
                    self.renderer
                        .render_at(&mut self.term, back, &mut target, damage)
                };
                self.stats = stats;
                let clear = self.renderer.clear_color();
                // What the frame repainted, so the surface it goes to copies
                // only the rows it and the frames since that surface was
                // last copied changed.
                let painted: Vec<baud_render::Band> = self.renderer.painted().collect();
                // A sliding frame is composed from the live frame and the
                // kept one, and it and the frame after it go whole.
                let slid = match slide {
                    Some(s) if self.kept_frame.len() == self.frame.len() => {
                        self.composed.resize(self.frame.len(), 0);
                        // The rows that slide: the frame's own, not the
                        // activity lights' line, which holds still and is
                        // copied from the live frame.
                        let rows = self.renderer.frame_size().1;
                        let core::ops::Range { start: a, end: b } = crate::width::slide_span(
                            self.renderer.grid_top(),
                            rows,
                            w,
                            &self.frame,
                        );
                        self.composed[..a].copy_from_slice(&self.frame[..a]);
                        self.composed[b..].copy_from_slice(&self.frame[b..]);
                        let o = baud_render::slide_offsets(s, rows);
                        match baud_render::compose_slide(
                            &self.frame[a..b],
                            &self.kept_frame[a..b],
                            &mut self.composed[a..b],
                            w,
                            rows,
                            o,
                        ) {
                            true => Composed::Yes,
                            false => Composed::No,
                        }
                    }
                    _ => {
                        // Settled: the two frames go, so an idle window
                        // holds nothing for the slide.
                        if slide.is_none() && self.transition.is_none() {
                            self.kept_frame = Vec::new();
                            self.composed = Vec::new();
                        }
                        Composed::No
                    }
                };
                let changed = match (damage, slid, self.slid) {
                    (Damage::Full, _, _) | (_, Composed::Yes, _) | (_, _, Composed::Yes) => {
                        baud_present_macos::Changed::Whole
                    }
                    (Damage::Incremental, Composed::No, Composed::No) => {
                        baud_present_macos::Changed::Bands(&painted)
                    }
                };
                self.slid = slid;
                let frame = match slid {
                    Composed::Yes => &self.composed,
                    Composed::No => &self.frame,
                };
                if let Some(p) = &mut self.presenter {
                    if p.present_changed(screen, frame, w, clear, changed) == Presented::Yes {
                        shown = Presented::Yes;
                        // On the layer now, as the first frame drawn on
                        // the GPU is; no completion follows a CPU frame.
                        self.layer_shown = Done::Yes;
                        self.latency.shown_cpu(host_now);
                        startup_first("first present", &FIRST_PRESENT);
                        if FIRST_BYTES.load(Ordering::Relaxed) {
                            startup_first("first content", &FIRST_CONTENT);
                        }
                    }
                }
            }
        }
        self.blink_present = self.renderer.blink_seen();
        match shown {
            Presented::Yes => {
                self.bell.commit(bell);
                // The first frame after the logo says what its frames did.
                let said = self.launch_log.committed(host_now(), logo, &self.term);
                for line in said.into_iter().flatten() {
                    startup_note(&line);
                }
            }
            Presented::No => self.bell.refused(bell),
        }
        // The blink's phase is taken either way: not taking it would owe a
        // frame on every tick for as long as presents keep failing, the
        // defect the bell's review found in `refused`. What that costs: a
        // present that fails drops the frame, and while the cursor blinks the
        // next flip draws the phase again, but a failed frame that was to
        // stop the blink shown -- focus lost, a steady style, the system's
        // switch, the stop, a steady cycle -- leaves it hidden until another
        // frame draws for any reason.
        self.caret.commit(caret);
        self.pending = Pending::Clean;
        self.last_commit = Instant::now();
        // A fed echo is drawn by this commit; a wanted one is still owed,
        // whatever frame this was -- a tick's, or a prediction's.
        if self.echo == Echo::Fed {
            self.echo = Echo::Drawn;
        }
        self.guess = Guess::None;
        self.marked = Marked::Drawn;
        self.frames.bump();
    }

    /// The five-second report. **It is called from the tick and not from
    /// `present`, because an idle screen never reaches the end of
    /// `present`** -- the `Pending::Clean` gate returns first -- so the
    /// report this lived inside could not fire in exactly the state row
    /// 11 is about, and its silence read the same as a broken probe.
    ///
    /// The CPU path's first line and the counts after it are the Linux
    /// driver's too (`report.rs`); the GPU line and the latency line are
    /// this app's alone.
    fn report(&mut self) {
        if self.reported.due(std::time::Instant::now()) == crate::report::Due::No {
            return;
        }
        match self.presenter.as_ref().map(Presenter::mode) {
            Some(Mode::Gpu) => crate::info!(
                "{} ticks, {} frames; last: {} rows, {} instances, convert {:?}, bind {:?}, commit {:?}; {}",
                self.ticks.value(),
                self.frames.value(),
                self.stats.rows_dirty.value(),
                self.gpu_stats.instances.value(),
                self.stats.convert,
                self.gpu_stats.upload,
                self.gpu_stats.submit,
                self.present_counts()
            ),
            _ => crate::info!(
                "{}",
                crate::report::cpu_line(
                    crate::report::Wakes::Ticks(self.ticks),
                    self.frames,
                    &self.stats
                )
            ),
        }
        if let Some(line) = self.latency.report() {
            eprintln!("{line}");
        }
        for line in crate::report::counts(&self.clips, &self.widths, &self.links) {
            crate::info!("{line}");
        }
    }
}

/// What the present path has done, for the five-second report and for
/// `BAUD_FRAME_LOG`.
///
/// `superseded` is the one to read: a frame that finished after the
/// surfaces it drew into had been replaced. Shown, it was the doubled image
/// on a grow and the old font size on a zoom; since `a9c1a16` `admits`
/// drops it, so the count is of frames kept off the layer, not of the
/// artefact happening.
impl App {
    /// A new pixel size for the faces, and the glyph map that goes with
    /// it.
    ///
    /// Every entry names an atlas rectangle at the old size, so a rescale
    /// invalidates the map whole -- and filling it in place is 384 KB
    /// written into the buffer the shader is probing for the frame in
    /// flight. The next map is built over the other buffer and the binding
    /// moves: a pointer, no copy, no allocation, and nothing waits.
    fn rescale(&mut self, px: PxSize) -> Option<()> {
        self.renderer.rescale(px)?;
        self.rescaled();
        Some(())
    }

    /// Another face leads the chain, at `px`: the renderer restacked
    /// (`Renderer::restack`), then what a rescale owes.
    fn restack(&mut self, face: crate::fonts::Slot, px: PxSize) -> Option<()> {
        let built = self.renderer.fonts_mut().stack(face)?;
        let stack = baud_render::Stack::with_roles(built.faces, &built.roles)?;
        self.renderer.restack(stack, px)?;
        self.renderer.fonts_mut().set(face);
        self.rescaled();
        Some(())
    }

    /// What a new size or stack owes beside the renderer.
    fn rescaled(&mut self) {
        // The row height changed, so the slide's cap did too; keep an offset
        // in flight a valid fraction of the new row. The Sixel cursor advance
        // divides by the same height, so re-take it here or every image from
        // now mis-advances -- the stale-geometry trap this repo has hit before.
        self.slide.set_cell_h(self.renderer.metrics().cell_h);
        self.term.set_cell_size(
            cell_px(self.renderer.metrics().cell_w),
            cell_px(self.renderer.metrics().cell_h),
        );
        // A rescale is a move to a display of another scale as often as a
        // zoom: the points `ReportCellSize` answers in follow it.
        self.term
            .set_backing_scale(baud_domain::BackingScale::of(self.scale));
        if let Some(cell) = self.cell.as_mut() {
            cell.maps.rescaled();
        }
    }

    /// The instrument's `landed` and `shown` legs, from the view's
    /// `presented` -- fired where the `CATransaction` that set the surface
    /// as the layer's contents committed, so `shown` is that moment and not
    /// the wake that follows it, which the pty reader can also send and
    /// which a pump can run ahead of the present. `landed` is the GPU end
    /// the journal recorded for the frame the completion handed over; a
    /// stamp that is not a landing on this clock is refused, printed once
    /// per refusal, and the key still completes without it.
    fn presented(&mut self) {
        if self.latency.on() == Switch::Off {
            return;
        }
        let Some(ended) = self.presenter.as_ref().map(Presenter::presented_end) else {
            return;
        };
        let now = host_now();
        // A present with no landing queued behind it is not a frame this
        // instrument can pair; `ZERO` is what `presented` refuses.
        let ended = ended.unwrap_or(HostTime::ZERO);
        if self.latency.presented(ended, now) == Ended::Refused {
            eprintln!(
                "keylat: landing refused ended={} now={}",
                ended.nanos(),
                now.nanos()
            );
        }
    }

    fn present_counts(&self) -> String {
        match self.presenter.as_ref().map(Presenter::counts) {
            Some(c) => format!(
                "present: {} landed, {} shown, {} superseded, {} deferred, {} given back, {} owed to a live frame, last gpu {:?}",
                c.landed,
                c.shown,
                c.superseded,
                c.deferred,
                c.abandoned,
                self.owed_in_flight.value(),
                c.gpu
            ),
            None => "present: no presenter".to_string(),
        }
    }
}

/// When the key event being dispatched was generated, on the host clock:
/// `NSEvent.timestamp` of `NSApplication`'s `currentEvent`, which inside
/// `keyDown:` -- and inside the `insertText:` and `doCommandBySelector:` the
/// input manager runs from it -- is that key event. `None` off the main
/// thread, with no event being dispatched, or for a stamp that is not a
/// time; the instrument then takes the moment the responder ran. The one
/// place an `NSTimeInterval` crosses into the instrument's type.
fn current_event_time() -> Option<HostTime> {
    let mtm = MainThreadMarker::new()?;
    let event = NSApplication::sharedApplication(mtm).currentEvent()?;
    HostTime::of_secs(event.timestamp())
}

fn slots(p: Px) -> usize {
    usize::try_from(p.value()).unwrap_or(0)
}

/// **The modifiers a key is encoded with**: those the window crate's key
/// event carries, and Caps Lock from the event's own `modifierFlags`,
/// which the crate's modifiers have no field for. "If a modifier is
/// *active* when the key event occurs, i.e. if the key is pressed or the
/// lock (for caps lock/num lock) is enabled, the key event must have the
/// bit for that modifier set" (`[[source: kitty-keyboard]]`), as kitty's
/// macOS layer reads `NSEventModifierFlagCapsLock` from the event
/// (`translateFlags`, `[[source: kitty-glfw-cocoa-window-851cb80]]`). A
/// Mac has no NumLock, whose key is the keypad's Clear.
fn held_with_locks(key: &KeyPress, flags: NSEventModifierFlags) -> Modifiers {
    with_locks(mods(key.modifiers), flags)
}

/// `m` with Caps Lock's bit while an event's `flags` say the lock is on:
/// a key's press and release ([`held_with_locks`]) and a modifier key's
/// change ([`modifier_event`]) alike.
fn with_locks(m: Modifiers, flags: NSEventModifierFlags) -> Modifiers {
    match flags.contains(NSEventModifierFlags::CapsLock) {
        true => m.with(Modifiers::CAPS_LOCK),
        false => m,
    }
}

/// **The key event's flags**: `modifierFlags` of `NSApplication`'s
/// `currentEvent`, which inside `keyDown:`, `keyUp:` and `flagsChanged:`
/// is that event, as [`current_event_time`] reads its stamp, and inside
/// the monitor that hands the view a Command-held key-up is that key-up
/// (`a_key_up_a_monitor_is_handed_is_the_current_event`). AppKit sets
/// `NSEventModifierFlagCapsLock` there while the lock is on, "Set if Caps
/// Lock key is pressed" (`[[source: macos-sdk-nsevent-h]]` line 168;
/// `[[source: apple-nsevent-modifierflags]]`,
/// `[[source: apple-nsevent-modifierflags-capslock]]`). None off the main thread or
/// with no event being dispatched.
fn current_event_flags() -> NSEventModifierFlags {
    MainThreadMarker::new()
        .and_then(|mtm| NSApplication::sharedApplication(mtm).currentEvent())
        .map_or(NSEventModifierFlags(0), |e| e.modifierFlags())
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

/// The link's chord from the modifiers held: Command, and not Shift, which
/// is the selection's.
fn link_chord(m: macos_liquid_glass::drawable::Modifiers) -> crate::link::Chord {
    match m.command && !m.shift {
        true => crate::link::Chord::Cmd,
        false => crate::link::Chord::Other,
    }
}

/// What kind of key event AppKit delivered: a key the system repeats while it
/// is held is marked, and the core reports it as a repeat only where
/// `[[source: kitty-keyboard]]`'s event types asked for one -- everywhere else
/// the encoder sends it as the press it replays.
fn kind_of(key: &KeyPress) -> KeyEventKind {
    match key.repeat {
        true => KeyEventKind::Repeat,
        false => KeyEventKind::Press,
    }
}

/// **The presses the program got, by key code, until each one's release.**
/// kitty's event types report "key repeat and key release events"
/// (`[[source: kitty-keyboard]]`), and a release names the key its press
/// named: the identity, base layout key and text the press carried, with
/// the modifiers held when it comes up. A release whose press the program
/// never got is dropped, and so is a second one: a Command chord is the
/// menu's, a key the input manager takes into a composition or a dead key
/// types nothing yet, and the find field takes what is typed into it, so
/// none of those was sent and a release alone would tell the program of a
/// key it never saw go down. kitty pairs them the same way
/// (`[[source: kitty-glfw-input-c-851cb80]]`). The Mac's virtual key codes
/// are below `0x80` (`[[source: macos-sdk-events-h]]`).
struct Downs {
    keys: [Option<KeyEvent>; 128],
    /// Caps Lock's bit as the last key event the view was handed carried
    /// it, a press, a release or a modifier key's change, the lock's own
    /// among them: what the keys let go at a focus loss carry (`let_go`).
    lock: Modifiers,
}

/// A Mac's virtual key code, `NSEvent.keyCode`: a physical key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct VirtualKey(u16);

/// Whose the keys are: the program's, or the find field's while it is
/// open, which takes every key typed into it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KeysTo {
    Program,
    FindField,
}

/// The key that typed a commit of the input manager, `text`: the key
/// handed to it, `code`, when the commit is one character, and none for a
/// commit of more, which is no one key's, so no press is recorded for it
/// and no release follows.
fn commit_key(code: Option<VirtualKey>, text: &str) -> Option<VirtualKey> {
    code.filter(|_| text.chars().count() == 1)
}

/// **The key handed to the input manager, until what it typed arrives**:
/// set by `key()` for a key it answers `Handled::Interpret` for, and taken
/// by the `text()` or `command()` the input manager answers it with, whose
/// press `typed()` records in [`Downs`] under it, so its release follows.
/// The window crate's `insertText:` reports the composition ended,
/// `marked_text("")`, before every commit, a key's own character among
/// them, so an empty composition leaves the key handed: only a key that
/// made the composition something -- a dead key's accent, an input
/// method's first letter -- typed nothing itself and is dropped, and a key
/// pressed while a composition is open is never handed
/// (`composing_first`). The key coming up drops it too, so a commit after
/// it -- the character palette's, an input method's on a click -- is no
/// key's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Handed(Option<VirtualKey>);

impl Handed {
    const NONE: Handed = Handed(None);

    /// `key` went to the input manager.
    fn key(&mut self, key: VirtualKey) {
        self.0 = Some(key);
    }

    /// The composition is `text` now: a key that made it something typed
    /// nothing.
    fn marked(&mut self, text: &str) {
        if !text.is_empty() {
            self.0 = None;
        }
    }

    /// The key that typed the commit `text`, taken: the one handed when
    /// the commit is one character ([`commit_key`]).
    fn text(&mut self, text: &str) -> Option<VirtualKey> {
        commit_key(self.0.take(), text)
    }

    /// The key the input manager made a command of, taken.
    fn command(&mut self) -> Option<VirtualKey> {
        self.0.take()
    }

    /// `key` came up.
    fn up(&mut self, key: VirtualKey) {
        if self.0 == Some(key) {
            self.0 = None;
        }
    }
}

impl Downs {
    const NONE: Downs = Downs {
        keys: [None; 128],
        lock: Modifiers::NONE,
    };

    /// A key event with `flags` was handed to the view: Caps Lock as it
    /// says.
    fn saw(&mut self, flags: NSEventModifierFlags) {
        self.lock = with_locks(Modifiers::NONE, flags);
    }

    /// `e` went to the program for the key `key`.
    fn pressed(&mut self, key: VirtualKey, e: KeyEvent) {
        if let Some(slot) = self.keys.get_mut(usize::from(key.0)) {
            *slot = Some(e);
        }
    }

    /// The release of `key` with `mods` held, if its press went to the
    /// program; the slot is free again after.
    fn released(&mut self, key: VirtualKey, mods: Modifiers) -> Option<KeyEvent> {
        let press = self.keys.get_mut(usize::from(key.0))?.take()?;
        Some(KeyEvent {
            kind: KeyEventKind::Release,
            mods,
            ..press
        })
    }

    /// The release of the key that came up, `key`, with the modifiers
    /// held as it does and Caps Lock from its event's `flags`, if its
    /// press went to the program.
    fn up(&mut self, key: &KeyPress, flags: NSEventModifierFlags) -> Option<KeyEvent> {
        self.saw(flags);
        self.released(VirtualKey(key.key_code), held_with_locks(key, flags))
    }

    /// What a modifier key alone, `e` on `key`, sends: its release when
    /// its press went, and its press unless the find field has the keys,
    /// recorded so its release follows it.
    fn modifier(&mut self, key: VirtualKey, e: KeyEvent, to: KeysTo) -> Option<KeyEvent> {
        match (e.kind, to) {
            (KeyEventKind::Release, _) => self.released(key, e.mods),
            (_, KeysTo::FindField) => None,
            (_, KeysTo::Program) => {
                self.pressed(key, e);
                Some(e)
            }
        }
    }

    /// Every press still held, released, and the table empty after: the
    /// window losing the keyboard, whose key-ups go to the window that took
    /// it, so a program would hold those keys for good. No key is held
    /// after, and Caps Lock's bit is set while the lock is on, as the last
    /// key event said (`saw`): "if a modifier is active when the key event
    /// occurs, i.e. if the key is pressed or the lock (for caps lock/num
    /// lock) is enabled, the key event must have the bit for that modifier
    /// set" (`[[source: kitty-keyboard]]`). The Linux driver lets go the
    /// same way
    /// (`crate::pressed::Pressed::let_go`); Ghostty releases on a focus
    /// loss too (`[[source: ghostty-d9840f3-surface]]`, `focusCallback`),
    /// and kitty builds the releases and drops them before the program
    /// (`[[source: kitty-glfw-window-c-6d5d0c4]]`,
    /// `[[source: kitty-glfw-c-6d5d0c4]]` line 605).
    fn let_go(&mut self) -> impl Iterator<Item = KeyEvent> + '_ {
        let lock = self.lock;
        self.keys
            .iter_mut()
            .filter_map(Option::take)
            .map(move |press| KeyEvent {
                kind: KeyEventKind::Release,
                mods: lock,
                ..press
            })
    }
}

/// **A modifier key pressed or released alone**, as kitty's "report all
/// keys as escape codes" reports it, "events for pressing modifier keys"
/// (`[[source: kitty-keyboard]]`): its functional key, `LEFT_SHIFT` to
/// `RIGHT_SUPER` and `CAPS_LOCK`, Command being super and Option alt ("the
/// command key on mac keyboards", "the option key"). The modifiers are
/// those held after the change, "the modifier state including the effect
/// for the current event": Control set on a Control press, clear on its
/// release, and still set when the other Control key holds it. Caps Lock's
/// bit is set while the lock is on, read from the change's own `flags`
/// as a key's is ([`held_with_locks`]): "if ... the lock (for caps
/// lock/num lock) is enabled, the key event must have the bit for that
/// modifier set". The lock's own key is pressed when the change turns the
/// lock on and released when it turns it off, its bit set and clear with
/// it, as kitty's macOS layer reads it
/// (`[[source: kitty-glfw-cocoa-window-851cb80]]`, `flagsChanged:`). `fn`
/// is none: kitty's layer reports nothing for it,
/// and the protocol has no key for it. The core sends a modifier key only
/// under "report all keys".
fn modifier_event(
    key: Option<macos_liquid_glass::drawable::ModifierKey>,
    kind: KeyEventKind,
    held: macos_liquid_glass::drawable::Modifiers,
    flags: NSEventModifierFlags,
) -> Option<KeyEvent> {
    use baud_domain::PuaKey as P;
    use macos_liquid_glass::drawable::ModifierKey as K;
    let code = match key? {
        K::LeftShift => P::LeftShift,
        K::RightShift => P::RightShift,
        K::LeftControl => P::LeftControl,
        K::RightControl => P::RightControl,
        K::LeftOption => P::LeftAlt,
        K::RightOption => P::RightAlt,
        K::LeftCommand => P::LeftSuper,
        K::RightCommand => P::RightSuper,
        K::CapsLock => P::CapsLock,
        _ => return None,
    };
    let m = with_locks(mods(held), flags);
    // The lock's own key carries the lock as the change leaves it.
    let m = match (code, kind) {
        (P::CapsLock, KeyEventKind::Release) => m.without(Modifiers::CAPS_LOCK),
        (P::CapsLock, _) => m.with(Modifiers::CAPS_LOCK),
        _ => m,
    };
    let mut e = KeyEvent::press(KeyCode::Pua(code), m);
    e.kind = kind;
    Some(e)
}

/// Whose a click is: the program's and the terminal's as any click is, or
/// the one that brought the window forward.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Click {
    Ours,
    Activating,
}

/// **The left click into an inactive window activates it and does
/// nothing else**: not reported to a program tracking the mouse, starting
/// no selection, arming no link, and its drag and release go with it. The
/// window crate accepts the first mouse, so the press reaches the view;
/// Apple's guidance is that "most view objects refuse a click-through
/// attempt, so the event simply activates the window"
/// (`[[source: apple-nsview-acceptsfirstmouse]]`). Ghostty refuses it
/// because accepting it would "encode the event to the pty"
/// (`[[source: ghostty-d9840f3-surfaceview-appkit]]`), and iTerm2 by
/// default reports no first click to the program and runs no Cmd-click
/// action from it (`[[source: iterm2-7ce037e-advanced-settings]]`). A
/// left press is the activating one when the window did not have the
/// keyboard before it, `was` being the focus as last polled, read before
/// this event polls it; a left drag or release belongs to the press it
/// follows, `held`. The poll cannot see the window key before its press:
/// AppKit makes the window key inside the dispatch of that same
/// mouse-down, which Ghostty's monitor relies on ("If you return nil here
/// then nobody gets a windowDidBecomeKey event"), and no tick runs inside
/// a dispatch; that order is read from Ghostty's source, not measured.
///
/// **A right, middle or other press activates no window**, so it is the
/// program's and the terminal's in the background as in front: Apple
/// speaks of the "initial mouse-down event" a view is sent with
/// `mouseDown(with:)`, iTerm2 brings the window forward on those buttons
/// only under a preference that is off by default
/// (`[[source: iterm2-7ce037e-pty-mouse-handler]]`,
/// `[[source: iterm2-7ce037e-preferences]]`), and Ghostty's monitor
/// looks only at the left button.
fn click_of(held: Click, kind: ViewMouseKind, button: ViewButton, was: baud_core::Focus) -> Click {
    match (kind, button) {
        (ViewMouseKind::Down, ViewButton::Left) => match was {
            baud_core::Focus::Lost => Click::Activating,
            baud_core::Focus::Gained => Click::Ours,
        },
        (ViewMouseKind::Dragged | ViewMouseKind::Up, ViewButton::Left) => held,
        _ => Click::Ours,
    }
}

/// The left click held after `kind` of `button`, whose own click is
/// `click`: a left press sets it and a left release ends it, and another
/// button's press or release inside it leaves it as it was.
fn click_held(held: Click, kind: ViewMouseKind, button: ViewButton, click: Click) -> Click {
    match (kind, button) {
        (ViewMouseKind::Down, ViewButton::Left) => click,
        (ViewMouseKind::Up, ViewButton::Left) => Click::Ours,
        _ => held,
    }
}

/// The keys a macOS virtual key code names outright, before any text.
///
/// **The keypad is named by its key codes**, `[[source:
/// macos-sdk-events-h]]` lines 244-261, and not by
/// `NSEventModifierFlagNumericPad`, which AppKit sets on the arrow keys as
/// well. No keypad navigation key (`KeypadKey::Left` to `KeypadKey::Begin`)
/// is named, because a Mac keypad has none: each physical keypad key has
/// one code, by its digit, and the keypad has no NumLock state, so a digit
/// is always the digit. Linux names them from the keysym NumLock picks.
///
/// `0x47`, `kVK_ANSI_KeypadClear`, is NumLock: the HID usage tables name
/// the one key "Keypad Num Lock and Clear"
/// (`[[source: usb-hid-usage-tables-1-6]]`, Keyboard/Keypad page, usage
/// 0x53), and kitty's macOS layer names it `GLFW_FKEY_NUM_LOCK`
/// (`[[source: kitty-glfw-cocoa-window-851cb80]]`). It sends nothing on
/// the legacy layer, as a lock key does, and kitty's `57360` under
/// "report all keys".
fn special(key: &KeyPress) -> Option<KeyCode> {
    use baud_domain::KeypadKey as Kp;
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
        0x24 => KeyCode::Enter,
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
        0x41 => KeyCode::Keypad(decimal(key)),
        0x43 => KeyCode::Keypad(Kp::Multiply),
        0x45 => KeyCode::Keypad(Kp::Add),
        0x47 => KeyCode::Pua(baud_domain::PuaKey::NumLock),
        0x4B => KeyCode::Keypad(Kp::Divide),
        0x4C => KeyCode::Keypad(Kp::Enter),
        0x4E => KeyCode::Keypad(Kp::Minus),
        0x51 => KeyCode::Keypad(Kp::Equal),
        0x52 => KeyCode::Keypad(Kp::Digit0),
        0x53 => KeyCode::Keypad(Kp::Digit1),
        0x54 => KeyCode::Keypad(Kp::Digit2),
        0x55 => KeyCode::Keypad(Kp::Digit3),
        0x56 => KeyCode::Keypad(Kp::Digit4),
        0x57 => KeyCode::Keypad(Kp::Digit5),
        0x58 => KeyCode::Keypad(Kp::Digit6),
        0x59 => KeyCode::Keypad(Kp::Digit7),
        // `0x5A` is F20's.
        0x5B => KeyCode::Keypad(Kp::Digit8),
        0x5C => KeyCode::Keypad(Kp::Digit9),
        _ => return None,
    })
}

/// The keypad's decimal key, `0x41`, by what the layout types on it
/// with Control and Option ignored (`charactersIgnoringModifiers`), so
/// those chords do not change the key; Shift can, on the 52 layouts where
/// it types the other separator.
///
/// **A decimal key that types `,` is the keypad's comma**, as it is on a
/// German Linux keypad: xkeyboard-config makes it `KP_Separator`
/// (`[[source: xkeyboard-config-kpdl-193cc6c]]` lines 30-35, included by
/// the German layout, `[[source: xkeyboard-config-de-193cc6c]]` line 35),
/// so it takes line 2498's comma row, `SS3 l` under DECKPAM, and kitty's
/// `KP_SEPARATOR`. kitty's and iTerm2's macOS layers name it `KP_DECIMAL`
/// by its code alone; here the key is told by its text. U+066B, the
/// Arabic decimal separator, and every other character keep it the
/// decimal key.
fn decimal(key: &KeyPress) -> baud_domain::KeypadKey {
    match key.unmodified.starts_with(',') {
        true => baud_domain::KeypadKey::Comma,
        false => baud_domain::KeypadKey::Period,
    }
}

/// **A Control or Option chord**: the key's identity, folded by the core,
/// what it typed as its text, its PC-101 character as kitty's base layout
/// key, and under Control on a key whose identity is not ASCII the
/// character the layout's own Control layer gives (`control_of`).
fn chord(key: &KeyPress, m: Modifiers, unshifted: Option<Codepoint>) -> Option<KeyEvent> {
    let c = key
        .unmodified
        .chars()
        .next()
        .and_then(|c| Codepoint::new(u32::from(c)))?;
    // With Shift, `charactersIgnoringModifiers` is the key shifted: the key
    // is the one Shift was on, and that character its shifted key, as
    // kitty's macOS layer takes it (`add_alternate_keys`).
    let (c, shifted) = match key.modifiers.shift {
        true => shifted_key(c, unshifted).map_or((c, None), |(k, s)| (k, Some(s))),
        false => (c, None),
    };
    let mut e = KeyEvent::press(KeyCode::Char(c), m);
    e.shifted = shifted.filter(|&s| s != c);
    e.text = key
        .characters
        .chars()
        .next()
        .filter(|ch| !ch.is_control())
        .and_then(|ch| Codepoint::new(u32::from(ch)));
    // Sent where it differs from the key's own unshifted character, which
    // `charactersIgnoringModifiers` gives with Shift applied, and from the
    // key Shift was found to be on.
    let own = key.unmodified.chars().flat_map(char::to_lowercase).next();
    e.base_layout = pc101(key)
        .filter(|&b| Some(b) != own.and_then(|o| Codepoint::new(u32::from(o))))
        .filter(|&b| b != c);
    if key.modifiers.control && c.value() > 0x7f {
        e.control = control_of(&key.characters);
    }
    Some(e)
}

/// **The ASCII key behind what the layout types with Control**: on every
/// Apple layout probed -- US, Russian, Ukrainian, Hebrew, Greek, Arabic,
/// Armenian, Georgian, Thai -- the letter and bracket keys' Control layer
/// types the ASCII control code (Ctrl+С `0x03`), and this names the key
/// that folds to it:
/// `@` for NUL, the letter for `0x01`-`0x1a`, `[` to `_` for `0x1b`-`0x1f`,
/// `?` for DEL; a printable ASCII character is itself, and anything else
/// none. The XKB protocol leaves Control on a non-ASCII key
/// "application-specific"; this is the layout's own answer, as libX11's
/// group fallback is on Linux.
fn control_of(characters: &str) -> Option<Codepoint> {
    let c = characters.chars().next()?;
    let v = u32::from(c);
    let key = match v {
        0 => '@',
        0x01..=0x1a => char::from_u32(v + 0x60)?,
        0x1b..=0x1f => char::from_u32(v + 0x40)?,
        0x7f => '?',
        0x20..=0x7e => c,
        _ => return None,
    };
    Some(Codepoint::from_char(key))
}

/// **The standard PC-101 layout**, by the Mac's virtual key code: the
/// `kVK_ANSI_*` codes `[[source: macos-sdk-events-h]]` names by the US
/// key at each position (lines 197-243) and `kVK_Space` (268), "codes
/// [that] identify physical keys on a keyboard". kitty's base layout key
/// is "the key corresponding to the physical key in the standard PC-101
/// key layout" (`[[source: kitty-keyboard]]`).
fn pc101(key: &KeyPress) -> Option<Codepoint> {
    let c = match key.key_code {
        0x00 => 'a',
        0x01 => 's',
        0x02 => 'd',
        0x03 => 'f',
        0x04 => 'h',
        0x05 => 'g',
        0x06 => 'z',
        0x07 => 'x',
        0x08 => 'c',
        0x09 => 'v',
        0x0B => 'b',
        0x0C => 'q',
        0x0D => 'w',
        0x0E => 'e',
        0x0F => 'r',
        0x10 => 'y',
        0x11 => 't',
        0x12 => '1',
        0x13 => '2',
        0x14 => '3',
        0x15 => '4',
        0x16 => '6',
        0x17 => '5',
        0x18 => '=',
        0x19 => '9',
        0x1A => '7',
        0x1B => '-',
        0x1C => '8',
        0x1D => '0',
        0x1E => ']',
        0x1F => 'o',
        0x20 => 'u',
        0x21 => '[',
        0x22 => 'i',
        0x23 => 'p',
        0x25 => 'l',
        0x26 => 'j',
        0x27 => '\'',
        0x28 => 'k',
        0x29 => ';',
        0x2A => '\\',
        0x2B => ',',
        0x2C => '/',
        0x2D => 'n',
        0x2E => 'm',
        0x2F => '.',
        0x31 => ' ',
        0x32 => '`',
        _ => return None,
    };
    Some(Codepoint::from_char(c))
}

/// **The keystroke handed to the input manager**, until `text` answers:
/// what it typed, its PC-101 character, the modifiers held -- of which
/// only Shift and Caps Lock reach here, Command, Control and Option being
/// kept from the input manager -- and, with Shift, the character the
/// layout types on the key without it (`layouts::unshifted`).
struct Keyed {
    typed: String,
    base: Option<Codepoint>,
    mods: Modifiers,
    unshifted: Option<Codepoint>,
}

/// **The key a Shift-held keystroke names**: the character the layout
/// types on it without Shift, and the one it typed as kitty's shifted key
/// (`[[source: kitty-keyboard]]`: the key is "the lower-case (or more
/// technically, un-shifted) version", "the shifted key is simply the
/// upper-case version"), so Shift+1 is the `1` key with `!` shifted.
/// Where the layout gave nothing, a capital's lower case is its key; a
/// character with no case is then left as it was typed, with no Shift,
/// rather than as a key Shift would have to be on.
fn shifted_key(typed: Codepoint, unshifted: Option<Codepoint>) -> Option<(Codepoint, Codepoint)> {
    let lower = char::from_u32(typed.value())
        .and_then(|ch| ch.to_lowercase().next())
        .and_then(|l| Codepoint::new(u32::from(l)));
    unshifted
        .or(lower.filter(|&l| l != typed))
        .map(|key| (key, typed))
}

/// **What the input manager's text types**: a key per character, its
/// lower case as the identity and itself as the text. When the text is
/// exactly what the keystroke that went to the input manager typed, it is
/// that key's: its PC-101 character rides as kitty's base layout key, and
/// Shift held is reported beside the key Shift was on ([`shifted_key`]),
/// as kitty reports `shift+a`, `CSI 97 ; 2 u`, and Caps Lock on beside
/// it, `CSI 97 ; 65 u` for its `A`. An input method's commit,
/// a paste or a composed character is not that key's, and carries
/// neither.
fn typed_text(text: &str, keyed: Option<&Keyed>) -> Vec<KeyEvent> {
    let own = keyed.filter(|k| k.typed == text && text.chars().count() == 1);
    let base = own.and_then(|k| k.base);
    let shift = own.filter(|k| k.mods.has(Modifiers::SHIFT));
    let locks = own.map_or(Modifiers::NONE, |k| k.mods.without(Modifiers::SHIFT));
    text.chars()
        .filter_map(|ch| {
            let c = Codepoint::new(u32::from(ch))?;
            let lower = ch.to_lowercase().next().unwrap_or(ch);
            let identity = Codepoint::new(u32::from(lower)).unwrap_or(c);
            let mut e = match shift.and_then(|k| shifted_key(c, k.unshifted)) {
                Some((key, typed)) => {
                    let mut e = KeyEvent::press(KeyCode::Char(key), Modifiers::SHIFT.with(locks));
                    e.shifted = Some(typed).filter(|&t| t != key);
                    e
                }
                None => KeyEvent::press(KeyCode::Char(identity), locks),
            };
            e.text = Some(c);
            e.base_layout = base.filter(|&b| KeyCode::Char(b) != e.code);
            Some(e)
        })
        .collect()
}

/// **Whether a key's modifiers make it a chord**, whose bytes are not its
/// text: anything but Shift, whose capital is the text the base layer
/// sends, and Caps Lock, whose capital is too.
fn is_chord(held: Modifiers) -> bool {
    held.without(Modifiers::SHIFT).without(Modifiers::CAPS_LOCK) != Modifiers::NONE
}

/// The character the layout types on `key` without Shift, asked only
/// while Shift is held: the one case its own characters do not say.
fn shift_unshifted(key: &KeyPress) -> Option<Codepoint> {
    match key.modifiers.shift {
        true => crate::layouts::unshifted(key),
        false => None,
    }
}

/// The event for a key [`special`] named, carrying `m`.
///
/// **A keypad key carries the text it types**, as the Wayland driver's
/// does, which is how kitty's disambiguate flag tells a typed digit from
/// a key that typed nothing (`[[source: kitty-keyboard]]`, "all non text
/// keypad keys"): the layout's character, none for a control code --
/// keypad Enter is `U+0003` -- and none while Control or Option is held,
/// as kitty's keyboard layer gives none (`[[source: kitty-xkb-glfw-c]]`).
/// The numeric column sends that text (`keypad_numeric`); the application
/// column reads the key.
fn pressed(code: KeyCode, key: &KeyPress, m: Modifiers) -> KeyEvent {
    let mut e = KeyEvent::press(code, m);
    if let KeyCode::Keypad(_) = code {
        e.text = key
            .characters
            .chars()
            .next()
            .filter(|c| !c.is_control() && !key.modifiers.control && !key.modifiers.option)
            .and_then(|c| Codepoint::new(u32::from(c)));
    }
    e
}

/// Whether a key's text is what it sends, so that drawing it ahead of the
/// echo predicts the echo: a keypad key only from the numeric column
/// (`[[source: ctlseqs]]` line 2498), under DECKPAM `5` being `SS3 u`.
fn predicts(code: KeyCode, keypad: KeypadMode) -> bool {
    match code {
        KeyCode::Keypad(_) => keypad == KeypadMode::Numeric,
        _ => true,
    }
}

/// A style change the system has announced and the loop has not applied.
///
/// **The observer cannot touch the driver, which is why this is a cell.**
/// `StyleObserver`'s callback runs on the main thread and can land while a
/// responder callback holds the view's `RefCell` borrow -- the same
/// re-entrancy that made moving the glass container inside a tick panic
/// with "already borrowed". So the callback records what the system said
/// and `pump` applies it, which is the shape `Deferred` already has for a
/// resize that arrives mid-flight.
#[derive(Clone, Default)]
struct PendingStyle(std::rc::Rc<std::cell::Cell<Option<Appearance>>>);

/// **macOS carries two light/dark settings and they disagree, so baud
/// reads each one where it belongs.**
///
/// `AppleInterfaceStyle` is System Settings > Appearance, light/dark, and
/// it is the *readability* axis: it decides the palette, and therefore
/// the dim, which is the palette's own opacity. That is this function.
/// `AppleIconAppearanceTheme` is **Icon & widget style**, the system's own
/// "how should glass look" setting, and it decides the material -- see
/// [`material_appearance`].
///
/// They are independent. On the machine this was written on the first is
/// `Dark` and the second is `ClearLight`, which is why the crate's example
/// renders its *light* calibration under a dark system, and why reading
/// one key for both would have matched neither.
///
/// This one is the window's own appearance, which is the view's while
/// nothing assigns the window one. baud deliberately does not: forcing it
/// from the widget style is what painted a terminal light under a dark
/// system, and that measurement is why the palette is not on that key.
fn appearance_of(window: &GlassWindow) -> Appearance {
    if window.is_dark() {
        Appearance::Dark
    } else {
        Appearance::Light
    }
}

/// The appearance the **material** follows: Icon & widget style.
///
/// This is the axis the crate's example follows, and the one that was
/// being watched when the example was seen to change with the system
/// settings -- measured, because the example's body is byte-identical
/// under a light and a dark system while its own band reads "Clear >
/// Light" under both.
///
/// `ThemeMode::Auto` is the one token with no light/dark of its own, and
/// `WidgetStyle::is_dark` resolves it against an ambient appearance -- so
/// under Auto the material does follow the system after all, and every
/// caller gets that without having to know which of the two preference
/// keys fired. The observer watches both, so both routes re-apply and both
/// compute this same answer.
fn material_appearance(window: &GlassWindow) -> Appearance {
    let ambient = window.ns_window().effectiveAppearance();
    if macos_liquid_glass::icon_style::current().is_dark(&ambient) {
        Appearance::Dark
    } else {
        Appearance::Light
    }
}

/// HSB to an sRGB colour.
///
/// Built in sRGB explicitly, so the alpha has a defined colour space
/// rather than whatever `NSColor.black` happens to be -- Generic Gray
/// Gamma 2.2, which cannot carry a hue at all. The same conversion the
/// crate's example does by hand, for the same reason.
fn hsb(h: f32, sat: f32, b: f32, alpha: f32) -> Retained<NSColor> {
    let c = f64::from(b * sat);
    let hp = f64::from(h.rem_euclid(1.0)) * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    // Compared rather than cast to a sector index: `hp` is already
    // bounded to 0..6 by the `rem_euclid`, so the six arms are six range
    // tests -- and a cast here would be the only one in this crate
    // outside the kernel ABI in `sys.rs`.
    let (r1, g1, b1) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = f64::from(b) - c;
    NSColor::colorWithSRGBRed_green_blue_alpha(r1 + m, g1 + m, b1 + m, f64::from(alpha))
}

/// The material darkening and the shadow, for an appearance.
///
/// **The darkening goes on the MATERIAL, not as a layer over it**, so the
/// titlebar strip and the body share one surface -- which is what the
/// crate's example does with the same setter, and what baud did not do at
/// all. It is appearance-dependent because a black strong enough for dark
/// would wrongly darken light.
///
/// The colour is a dark, slightly saturated blue rather than pure black:
/// black scales every channel equally, so it cannot shift the material's
/// hue, only scale the colour down -- the harder it darkens, the less
/// colour survives. Zero strength means no tint at all rather than a
/// transparent one.
///
/// **What this does not carry.** The example also washes the surface with
/// the Icon & widget style's *theme* colour under Tinted, and it does so
/// by painting it in its own `drawRect:` -- explicitly not through this
/// setter, which "despite being documented as a bias floods the surface
/// to a flat saturated slab at full strength". baud has no equivalent of
/// that painting and no key for it, so a Tinted desktop gets the material
/// darkening and no theme colour.
fn apply_glass(
    body: &Body,
    window: &GlassWindow,
    cfg: &crate::config::Glass,
    appearance: Appearance,
) {
    let dark = appearance == Appearance::Dark;
    // A visual effect view has no tint: the darkening is the glass's.
    if let Body::Glass(glass) = body {
        let strength = if dark { cfg.tint_dark } else { cfg.tint_light };
        let color = (strength > 0.0).then(|| hsb(cfg.hue, cfg.saturation, 0.10, strength));
        glass.set_tint_color(color.as_deref());
    }
    window.set_has_shadow(match cfg.shadow {
        crate::config::Shadow::Always => true,
        crate::config::Shadow::Never => false,
        crate::config::Shadow::Auto => dark,
    });
}

/// The window's body, from `[glass] style`: the see-through standard
/// material, `.underWindowBackground`, by default -- Apple's material for
/// app backgrounds, where Liquid Glass belongs to controls
/// ([[source: apple-hig-materials]]; note AT) -- or Liquid Glass, regular
/// or clear. Both are retains: a clone addresses the one AppKit view.
#[derive(Clone)]
enum Body {
    Standard(MaterialSurface),
    Glass(GlassSurface),
}

impl Body {
    /// The body `style` names, at `frame`. Only the glass can fail, on a
    /// system without it, and that refuses the launch as the check in
    /// [`run`] does.
    fn of(mtm: MainThreadMarker, frame: NSRect, style: crate::config::Material) -> Body {
        let glass = |style| {
            GlassSurface::new(mtm, frame, style, 16.0)
                .unwrap_or_else(|e| refuse(refusal_message(&e)))
        };
        match style {
            crate::config::Material::Standard => Body::Standard(MaterialSurface::new(
                mtm,
                frame,
                macos_liquid_glass::material::Material::UnderWindowBackground,
            )),
            crate::config::Material::Regular => Body::Glass(glass(GlassStyle::Regular)),
            crate::config::Material::Clear => Body::Glass(glass(GlassStyle::Clear)),
        }
    }

    /// The body as a plain view, for the window's content.
    fn view(&self) -> &NSView {
        match self {
            Body::Standard(m) => m.view(),
            Body::Glass(g) => g.view(),
        }
    }

    /// Which kind of body this is, for [`drawn_in`].
    fn kind(&self) -> BodyKind {
        match self {
            Body::Standard(_) => BodyKind::Standard,
            Body::Glass(_) => BodyKind::Glass,
        }
    }

    /// The appearance this body draws in, which the shadow follows.
    fn appearance(&self, window: &GlassWindow) -> Appearance {
        drawn_in(
            self.kind(),
            appearance_of(window),
            material_appearance(window),
        )
    }

    /// Put the terminal's container on the material.
    fn set_content_view(&self, content: &NSView) {
        match self {
            Body::Standard(m) => m.set_content_view(content),
            Body::Glass(g) => g.set_content_view(content),
        }
    }
}

/// The two kinds of [`Body`], without the views.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BodyKind {
    Standard,
    Glass,
}

/// The appearance a body draws in: **the standard material follows the
/// view's effective appearance** ("their exact look depends on the view's
/// effectiveAppearance", [[source: sdk27-nsvisualeffectview-h]] line 18),
/// which baud leaves the system's, `system`; the glass follows Icon &
/// widget style, `widget` ([`material_appearance`]). A Dark system under
/// a ClearLight widget style draws a dark standard material, so `shadow =
/// "auto"` casts one.
fn drawn_in(kind: BodyKind, system: Appearance, widget: Appearance) -> Appearance {
    match kind {
        BodyKind::Standard => system,
        BodyKind::Glass => widget,
    }
}

/// What the floor reads text against under the page: the see-through
/// standard material's greys for the half, from note AT's model, while
/// the palette follows the system -- the material follows the window's
/// appearance, so a palette that follows it too sits on that half's
/// material. Black to white over the glass (note AS), and under a pinned
/// palette, which can sit on either half's material.
fn backdrop(
    style: crate::config::Material,
    half: crate::palette::Half,
    choice: crate::config::ThemeChoice,
) -> baud_render::Backdrop {
    use crate::config::{Material, ThemeChoice};
    use crate::palette::Half;
    use baud_render::Backdrop;
    match (style, choice, half) {
        (Material::Standard, ThemeChoice::Auto, Half::Dark) => Backdrop::under_window_dark(),
        (Material::Standard, ThemeChoice::Auto, Half::Light) => Backdrop::under_window_light(),
        _ => Backdrop::any(),
    }
}

/// The palette an appearance resolves to, at the configured opacity: the
/// built-in half, with the theme file named for it laid over it, read
/// against what the body can show under it ([`backdrop`]).
fn theme_for(
    appearance: Appearance,
    glass: &crate::config::Glass,
    choice: crate::config::ThemeChoice,
    floor: baud_render::Floor,
    halves: &crate::palette::Halves,
) -> Theme {
    let half = match appearance {
        Appearance::Dark => crate::palette::Half::Dark,
        Appearance::Light => crate::palette::Half::Light,
    };
    let mut theme = crate::palette::half_theme(half, glass, floor, halves);
    theme.backdrop = backdrop(glass.style, half, choice);
    theme
}

/// Both palettes, built once at launch from the config: an appearance flip
/// swaps between them and reads nothing (note Y, step 4).
struct Themes {
    dark: Theme,
    light: Theme,
}

impl Themes {
    fn of(
        glass: &crate::config::Glass,
        choice: crate::config::ThemeChoice,
        floor: baud_render::Floor,
        halves: &crate::palette::Halves,
    ) -> Themes {
        Themes {
            dark: theme_for(Appearance::Dark, glass, choice, floor, halves),
            light: theme_for(Appearance::Light, glass, choice, floor, halves),
        }
    }

    fn get(&self, a: Appearance) -> &Theme {
        match a {
            Appearance::Dark => &self.dark,
            Appearance::Light => &self.light,
        }
    }
}

/// The floors of the two themes `Themes::of` builds, for the config's test.
#[cfg(test)]
pub(crate) fn themes_for_test(
    glass: &crate::config::Glass,
    floor: baud_render::Floor,
) -> (baud_render::Floor, baud_render::Floor) {
    let t = Themes::of(
        glass,
        crate::config::ThemeChoice::Auto,
        floor,
        &crate::palette::Halves::default(),
    );
    (t.dark.floor, t.light.floor)
}

/// The two themes `Themes::of` builds from `halves`, for the palette test.
#[cfg(test)]
pub(crate) fn themes_with(halves: &crate::palette::Halves) -> (Theme, Theme) {
    let t = Themes::of(
        &crate::config::Glass::defaults(),
        crate::config::ThemeChoice::Auto,
        baud_render::Floor::DEFAULT,
        halves,
    );
    (t.dark, t.light)
}

/// What macOS's Accessibility Display settings ask of the cells, read on
/// the tick beside Reduce Motion (note Y, decision 9). Off, the default,
/// until the first read, which `run` makes before the first frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Display {
    transparency: Transparency,
    contrast: Contrast,
}

/// Reduce Transparency: "UI (mainly window) backgrounds should not be
/// semi-transparent; they should be opaque"
/// ([[source: macos-sdk-nsaccessibility-h]] line 99).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Transparency {
    #[default]
    Full,
    Reduced,
}

/// The settings from the system's getters.
fn display_settings(
    reduce_transparency: impl FnOnce() -> bool,
    increase_contrast: impl FnOnce() -> bool,
) -> Display {
    Display {
        transparency: match reduce_transparency() {
            true => Transparency::Reduced,
            false => Transparency::Full,
        },
        contrast: match increase_contrast() {
            true => Contrast::Increased,
            false => Contrast::Standard,
        },
    }
}

/// `theme` as `display` asks for it drawn. The renderer's style cache is
/// dropped by `set_theme`, which moves its theme generation, so a changed
/// floor reaches cells already resolved only through it, which
/// `show_theme` calls.
///
/// **Reduce Transparency draws every background at alpha 1.0**, the margin
/// included, since it clears to the default background at the theme's
/// alpha. That overrides the rule that application backgrounds stay dim,
/// and only while the setting is on (note Y, decision 9). The 32 pt
/// titlebar strip stays material, which the system itself frosts under the
/// setting.
fn accessible(mut theme: Theme, display: Display) -> Theme {
    if display.transparency == Transparency::Reduced {
        theme.bg_alpha = baud_domain::Chan::from_u8(0xFF);
    }
    theme.floor = display.contrast.floor(theme.floor);
    theme
}

/// The appearance whose theme is in force: the pinned one, or under `auto`
/// the one the palette was last set for -- dark before the first, as
/// `prepare` builds it.
fn in_force(choice: crate::config::ThemeChoice, set_for: Option<Appearance>) -> Appearance {
    match choice {
        crate::config::ThemeChoice::Light => Appearance::Light,
        crate::config::ThemeChoice::Dark => Appearance::Dark,
        crate::config::ThemeChoice::Auto => set_for.unwrap_or(Appearance::Dark),
    }
}

/// What an appearance callback does to the palette.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PaletteSwitch {
    /// Nothing: a pinned theme, or the palette already set for this
    /// appearance.
    Stay,
    /// Set the palette for this appearance and drop the application's colour
    /// overrides with it.
    To(Appearance),
}

/// **Only a change of the resolved appearance switches.** AppKit calls
/// `viewDidChangeEffectiveAppearance` whenever the view's effective appearance
/// object changes, which includes accessibility variants such as Increase
/// Contrast where light or dark is the same, and a switch drops every colour
/// an application set (note Y, decision 5). So a callback that resolves to the
/// appearance the palette is already set for changes nothing, and a pinned
/// theme never switches, rather than dropping them on every callback.
fn palette_switch(
    choice: crate::config::ThemeChoice,
    set_for: Option<Appearance>,
    now: Appearance,
) -> PaletteSwitch {
    match choice {
        crate::config::ThemeChoice::Auto if set_for != Some(now) => PaletteSwitch::To(now),
        _ => PaletteSwitch::Stay,
    }
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
    /// Every button physically down, for DEC's locator's `Pb`, kept from
    /// every press and release the view delivers (`held_after`).
    buttons: LocatorButtons,
    /// The kind of the key being handled, from `key()` to the `typed()` it
    /// reaches. A printable key goes through the input manager, whose
    /// `text()` and `command()` carry no repeat flag, so `key()` notes it
    /// here first and `typed()` takes it, so a repeat marks one event. A
    /// repeating key that types nothing leaves it set until the next `key()`
    /// resets it, and only a `text()` from outside a key press -- an input
    /// method committing on a click -- could take it in between.
    key_kind: KeyEventKind,
    /// What the keystroke handed to the input manager typed and its PC-101
    /// character, until `text` answers: the base layout key of a key the
    /// input manager types as itself (`typed_text`).
    keyed: Option<Keyed>,
    /// The key handed to the input manager, until the `text` or
    /// `command` it answers with (`Handed`).
    handed: Handed,
    /// The presses the program got, until their releases (`Downs`).
    downs: Downs,
    /// Whether the click in progress is the one that activated the window,
    /// which does nothing else (`click_of`).
    click: Click,
    /// A Cmd press over a link, until its release: the gesture is the
    /// terminal's, and `crate::link::gesture` says what each event means.
    armed: Option<crate::link::Armed>,
    /// The press the scrollbar holds: a drag of the thumb in flight, and
    /// whether the press in progress is the scrollbar's, whose drags and
    /// release are then the scrollbar's too, so neither the selection nor
    /// a tracking program sees a release without its press (`bar.rs`).
    bar_hold: crate::bar::Hold,
    /// Who owns the press in progress, the child or the selection, decided
    /// at the press and kept to its release (`pointer_route`).
    owner: Owner,
    /// What a resize or a zoom asked for while a frame was in flight,
    /// applied by the next `pump` the completion wakes.
    deferred: Deferred,
    /// What the style observer last saw, for `pump` to apply.
    style: PendingStyle,
    /// Whether the palette follows the appearance at all: a config that
    /// names `dark` or `light` pins it, and the observer still moves the
    /// window's material -- the glass is the window's, the palette is the
    /// terminal's, and only the second is the reader's choice.
    theme_choice: crate::config::ThemeChoice,
    /// The material and its dimming, from the config's `[glass]`. Kept
    /// so an appearance change can resolve the pairs without re-reading
    /// the config, and so a theme swap can rebuild the palette.
    glass_cfg: crate::config::Glass,
    /// Both palettes, built at launch.
    themes: Themes,
    /// The Accessibility Display settings the theme in force was drawn
    /// for, read on the tick (`poll_display`).
    display: Display,
    /// The observers of the settings AppKit announces a change of, whose
    /// marks the tick takes (`follow_settings`).
    settings: baud_present_macos::settings::Watch,
    /// The window and screen as last handed to the core for XTWINOPS's
    /// reports, read on the tick (`poll_window`).
    window_facts: baud_core::WindowFacts,
    /// The frame a program's maximise saved, which its restore returns to
    /// (`crate::winops`), as xterm's `RequestMaximize` keeps the frame
    /// it restores.
    saved_frame: Option<crate::winops::Rect>,
    /// How the status line took its row when it came, the window's or the
    /// grid's, which its going gives back (`crate::winops::mac_status_change`).
    status_took: crate::winops::StatusChange,
    /// What the program's window operations did and did not do.
    performed: crate::winops::Performed,
    /// The appearance the palette was last set for under `auto`: `None`
    /// before the first, and always under a pinned theme.
    palette_for: Option<Appearance>,
    /// The window's material, and the window itself, so a light/dark flip
    /// can re-tint the one and re-shadow the other. Both are retains: a
    /// clone of either addresses the one AppKit object.
    ///
    /// The window one closes a cycle -- the window owns the view, the
    /// view owns this driver, and this driver retains the window -- a
    /// cycle that a window's close breaks: `closed` swaps the driver for
    /// `Gone`, which drops this retain.
    glass: Body,
    window: GlassWindow,
    /// The title the window was last given, so a program's `OSC 0` or
    /// `OSC 2` reaches it once rather than on every read.
    titles: crate::title::Shown,
    /// The minimized window's title last set: the icon name, else the
    /// title (`title::Mini`).
    mini: crate::title::Mini,
    /// The folder and program a title is derived from when a program set
    /// none, read once a tick when a read, a key or a process event owes
    /// it (`title.rs`), and the process events (`fg::Watch`).
    derive: crate::title::Derive,
    watch: Option<crate::fg::Watch>,
    /// This machine's names, one of which an `OSC 7` must name to be
    /// local (`machine_names`), read on a thread of their own from launch
    /// (`Hosts`), and the home directory a folder is spelled from
    /// (`title.rs`).
    hosts: Hosts,
    title_waits: TitleWaits,
    home: Option<Vec<u8>>,
    /// Whether a key hid the pointer, and what the core said when it was
    /// last asked (`hide.rs`).
    hiding: crate::hide::Hiding,
    /// The titlebar mark (`mark.rs`), in its leading titlebar accessory,
    /// re-centred on every resize. A manual autoresizing mask lost it after
    /// a fast drag to a tiny window -- the frame clamped and never came
    /// back -- so the driver places it itself instead, in the height
    /// AppKit gave the accessory.
    icon: Option<crate::mark::Mark>,
    /// The title bar's height when the window opened, which the lights'
    /// pictures are drawn for.
    icon_inset: CGFloat,
    /// The activity lights at the strip's right, in a trailing titlebar
    /// accessory (`lights.rs`, `strip.rs`).
    lights: crate::lights::Strip,
    /// OH's and CD's sampler of the job in front, and the kernel's reads
    /// it makes, on the tick while the lights are on (`front.rs`).
    front: crate::front::Front,
    procs: crate::front::Live,
    /// SYN's reading of the synchronised update at the last tick, so one
    /// opened and closed between two ticks still lights it (`bell.rs`).
    synced: crate::bell::Synced,
    /// The config's files watched and the config as last read, while
    /// `config_reload` is on (`reload.rs`).
    reload: Option<Reloading>,
    /// What the window says about the config, on a row of its own
    /// (`live::Notice`).
    notice: crate::live::Notice,
    /// The `[glass]` settings the style observer applies on an appearance
    /// change, shared with it so a reload reaches the next one.
    glass_live: std::rc::Rc<std::cell::Cell<crate::config::Glass>>,
    /// The window's key in the open windows, given as it is shown.
    window_id: Option<crate::windows::WindowId>,
    /// Whether the window's close has been asked for, once its shell has
    /// gone, so the pump that sees it again asks nothing more.
    close_asked: CloseAsked,
}

/// Whether a window's close has been asked for (`Driver::close_later`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CloseAsked {
    No,
    Yes,
}

/// **An open window, as the app holds it**: the window, the view whose
/// responder is its driver, and the observers that go when it does --
/// the style observer that tints its glass, and the one that hears it
/// close (`baud_present_macos::closing`).
struct Open {
    window: GlassWindow,
    view: Retained<DrawableView>,
    _style: macos_liquid_glass::icon_style::StyleObserver,
    _closing: Option<baud_present_macos::closing::Closing>,
    /// The observer of the window's full screen, which moves the grid's
    /// top (`strip.rs`), removed with the window.
    _full_screen: Option<baud_present_macos::full_screen::FullScreen>,
    /// The monitor that hands the view the key-ups AppKit keeps while
    /// Command is held (`baud_present_macos::keyup`), removed with the
    /// window.
    _keyups: Option<baud_present_macos::keyup::KeyUps>,
}

thread_local! {
    /// **The open windows** (`windows.rs`), on the main thread, where every
    /// window is opened and closed.
    static OPEN: std::cell::RefCell<crate::windows::Windows<Open>> =
        const { std::cell::RefCell::new(crate::windows::Windows::new()) };
    /// **A New Window the File menu asked of one window**, until that
    /// window's driver takes it (`new_window_from_menu`).
    static ASKED: crate::windows::Asked = const { crate::windows::Asked::new() };
    /// **Every window's progress, for the one Dock tile** (`notify::DockBars`).
    static DOCK: std::cell::RefCell<crate::notify::DockBars<crate::windows::WindowId>> =
        const { std::cell::RefCell::new(crate::notify::DockBars::new()) };
}

/// The Dock tile's bar drawn again, when the windows' progress changed it.
fn show_dock(mtm: MainThreadMarker, changed: Option<Option<crate::notify::DockBar>>) {
    if let Some(bar) = changed {
        baud_present_macos::notify::show_bar(mtm, bar.map(dock_bar));
    }
}

/// **A window opened after the launch**, by Command-N: the same boot,
/// shell, window and first frame as the launch's own, on a main-queue turn
/// of its own -- outside the key's callback, which holds the view's
/// borrow that the new window's taking the keyboard re-enters -- with its
/// shell started in `at` and the window cascaded from the key window, the
/// one it was opened from. The Dock's icon is the launch's. The logo is
/// every window's: it greets the window rather than marking the end of
/// a launch, so this sets nothing of it and the boot carries `[launch]
/// splash` as the launch's does (`splash.rs`).
fn open_window(at: Option<std::path::PathBuf>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let after = NSApplication::sharedApplication(mtm)
        .keyWindow()
        .map(|w| w.frame());
    let scale = NSScreen::mainScreen(mtm).map_or(1.0, |s| s.backingScaleFactor());
    let inset = predicted_inset(mtm, WINDOW_SIZE);
    let shell = Shell::start(scale, WINDOW_SIZE, inset, at.as_deref());
    let opening = Opening::new(mtm, WINDOW_SIZE, scale, inset, shell, None);
    opening.show(mtm, scale, after);
}

/// **A window that has closed, taken down**: on a main-queue turn after
/// its close, never inside one of its view's callbacks, which hold the
/// borrow the responder is swapped under. Its display link stops, and the
/// driver is replaced by a responder that does nothing, which drops the
/// window's terminal, renderer and timers and its `Pty`, whose close hangs
/// up the shell (`pty.rs`). The terminal gives back its grid and history
/// storage with its image store, its kitty files reader and the GPU
/// buffers its surfaces lived in (`lent::Screen`); the renderer its parsed
/// faces and font files, each file unmapped once no other window's fonts
/// count it (`lent::Drawing`); the process watch ends its thread and
/// closes its kqueue (`fg::Watch`). What stays is the view, which the
/// window crate's waker retains, and the `Gone` responder in it. The other
/// windows go on, and when it was the last the app runs on with none, for
/// File > New Window or a Dock click to open one (`windows::LAST_WINDOW`).
/// A second close of the same window finds nothing.
fn closed(id: crate::windows::WindowId) {
    let Some((open, left)) = OPEN.with_borrow_mut(|o| o.close(id)) else {
        return;
    };
    open.view.stop_display_link();
    open.view.set_responder(Box::new(Gone));
    drop(open);
    // Its progress leaves the Dock tile with it.
    if let Some(mtm) = MainThreadMarker::new() {
        show_dock(mtm, DOCK.with_borrow_mut(|d| d.gone(id)));
    }
    match left {
        crate::windows::Left::Others(n) => crate::info!("a window closed; {n} still open"),
        crate::windows::Left::None => crate::info!("the last window closed"),
    }
    if crate::windows::after_close(left, crate::windows::LAST_WINDOW)
        == crate::windows::AfterClose::Quit
    {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).terminate(None);
        }
    }
}

/// **File > New Window**, and Command-N, which the menu's key equivalent
/// takes before the view does: the app delegate's `newWindow:`
/// (`macos_liquid_glass::app::AppDelegate`). The window of baud's it was
/// asked from -- the key window, else the main one, while a panel has the
/// keyboard -- opens one as its own Command-N does, in its directory
/// (`Driver::new_window`), at its view's next wake, outside the menu's
/// action. With no window of baud's in front, one opens where a launch
/// starts. The window crate calls this for a Dock click with no window
/// shown too, and a minimized window counts as shown there
/// (`[[source: apple-nsapplicationdelegate-should-handle-reopen]]`).
fn new_window_from_menu() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let asked = [app.keyWindow(), app.mainWindow()]
        .into_iter()
        .flatten()
        .find_map(|front| {
            OPEN.with_borrow(|o| {
                o.ids().into_iter().find_map(|id| {
                    let open = o.get(id)?;
                    std::ptr::eq(open.window.ns_window(), &*front).then(|| (id, open.view.waker()))
                })
            })
        });
    match asked {
        Some((id, waker)) => {
            ASKED.with(|a| a.ask(id));
            waker.wake();
        }
        None => dispatch2::DispatchQueue::main().exec_async(|| open_window(None)),
    }
}

/// What begins every notification identifier this process posts.
fn notice_base() -> String {
    format!("baud-{}", std::process::id())
}

/// **What begins the identifiers of the notifications a window posts**
/// (`windows::WindowId::notice_prefix`); a window not yet given its key,
/// which posts nothing before it is shown, gets the process's alone.
fn notice_prefix(window: Option<crate::windows::WindowId>) -> String {
    match window {
        Some(w) => w.notice_prefix(&notice_base()),
        None => notice_base(),
    }
}

/// **The responder a closed window's view keeps**: the crate's waker
/// retains the view for the life of the process, so a wake a thread still
/// sends after the close reaches this, which does nothing.
struct Gone;

impl Responder for Gone {
    fn key(&mut self, _: &KeyPress) -> Handled {
        Handled::Consumed
    }
    fn text(&mut self, _: &str) {}
    fn command(&mut self, _: &str) {}
    fn marked_text(&mut self, _: &str) {}
    fn mouse(&mut self, _: &Mouse) {}
    fn scroll(&mut self, _: &Scroll) {}
    fn resized(&mut self, _: NSSize, _: CGFloat) {}
    fn frame(&mut self, _: f64) {}
    fn focus(&mut self, _: Focus) {}
    fn cursor_rect(&self) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        }
    }
}

/// **The config's watch** (`reload.rs`): the kernel's events heard on the
/// main queue, the files and the settle, and the config the window last
/// took, as read, which a new reading is compared with (`live::between`).
/// Dropping it cancels the source, and the kqueue closes once the source's
/// cancel handler has run (`reload::Listening`).
struct Reloading {
    _heard: Option<crate::reload::Listening>,
    watch: crate::reload::Watch<Instant>,
    read: crate::config::Config,
}

/// Whether a read erased the primary's history -- `CSI 3 J` or `RIS`, from
/// either screen -- by the core's count, because nothing else across a read
/// says so: the erase scrolls nothing, and lines after it in the same read
/// refill the history to a count that hides it.
fn erased(term: &Term<'_>, before: Before) -> bool {
    term.history_erasures() != before.erased
}

/// What the live screen did across a read, for the slide and the ride:
/// `live_moved`'s answer, or nothing when a program's page width resized the
/// grid inside the read (`honour`). The reflow's
/// re-cut retires rows that no output scrolled, and `relayout`, which a
/// drag reaches outside the pump, starts no slide for one either; the
/// selection is gone with the resize, so nothing it numbers is left.
fn read_moved(term: &Term<'_>, before: Before, resized: Option<crate::width::Grid>) -> Ring {
    match resized {
        Some(_) => Ring::Retired(Rows::ZERO),
        None => live_moved(term, before),
    }
}

/// **The wheel over the history**: the primary's window `lines` further
/// back, or forward where they are negative, held between the live
/// screen and the oldest line the history keeps; and how far back it then
/// is, or `None` where it already was, and nothing moved. The scroll
/// handler's `Route::History`, where a test reaches it.
fn scroll_history(term: &mut Term<'_>, lines: crate::wheel::Lines) -> Option<Rows> {
    let step = lines.get();
    let retained = i64::try_from(term.scrollback().len()).unwrap_or(i64::MAX);
    let was = term.primary_window_back();
    let now = i64::try_from(was.index()).unwrap_or(0);
    let back = Rows::of_count(usize::try_from((now + step).clamp(0, retained)).unwrap_or(0));
    if back.index() == was.index() {
        return None;
    }
    term.set_window_back(back);
    Some(back)
}

/// **The trail a frame draws** at `now`, the display link's target, for
/// the cursor [`trail_cursor`] follows, under the trail `kind`: the ghost's
/// sweep, spawned on a move and advanced ([`ghost_frame`]), or the chase's
/// ([`chase_frame`]), with the cell the cursor was last seen in moved on.
/// `present` hands it to the renderer once the slide's pan is read, and a
/// test draws what it gives through the same renderer. **A cursor the
/// program's scroll took with its line** first moves what the trail holds
/// of the text with it, under `[cursor] trail_scroll` (`key`), unless the
/// whole screen's scroll rode the trail since the last frame (`ridden`),
/// so only the rest of its move trails (`carried.rs`).
fn frame_trail(
    (kind, key): (crate::config::CursorTrail, crate::config::TrailScroll),
    term: &Term<'_>,
    renderer: &Renderer<'_, '_>,
    scale: CGFloat,
    now: f64,
    (chase, trail, last_cursor_cell): (
        &mut crate::trail::Chase<HostTime>,
        &mut Option<TrailState>,
        &mut Option<(Row, Col)>,
    ),
    (carried, ridden): (&mut crate::carried::Carried, Rows),
) -> Option<TrailLook> {
    let followed = trail_cursor(term);
    let cursor_cell = followed.map(TrailCursor::cell);
    let m = renderer.metrics();
    let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
    let cell = (side(m.cell_w), side(m.cell_h));
    // The cell's height in points, the flash bound's size gate.
    let height = baud_render::Points(cell.1 / scale as f32);
    let carry = crate::carried::carry(
        carried,
        term,
        cursor_cell,
        (kind, key),
        ridden,
        (chase, last_cursor_cell),
        cell.1,
    );
    if let Some(c) = carry {
        *trail = trail.map(|st| TrailState {
            to: (c.row(st.to.0), st.to.1),
            origin_dy: st.origin_dy + c.px(cell.1),
            ..st
        });
    }
    if kind == crate::config::CursorTrail::Ghost {
        let rect =
            |(r, c): (Row, Col)| renderer.cursor_rect(r, c, baud_core::CursorStyle::SteadyBlock);
        let sweep = HostTime::of_secs(now).and_then(|now| {
            crate::trail::ghost_sweep(
                chase.ledger(),
                *last_cursor_cell,
                cursor_cell,
                now,
                rect,
                (cell, height),
            )
        });
        if let Some(sw) = sweep {
            *trail = Some(TrailState {
                from: sw.from,
                to: sw.to,
                start: now,
                origin_dy: 0.0,
                ride: GhostRide::Riding(0.0),
                strength: sw.strength,
            });
        }
    }
    *last_cursor_cell = cursor_cell;
    let ghost = ghost_frame(trail, term, now);
    match kind {
        crate::config::CursorTrail::Ghost => ghost.map(TrailLook::Ghost),
        crate::config::CursorTrail::Quad
        | crate::config::CursorTrail::Sweep
        | crate::config::CursorTrail::Fire
        | crate::config::CursorTrail::Plasma
        | crate::config::CursorTrail::Lightning
        | crate::config::CursorTrail::Ice
        | crate::config::CursorTrail::Ufo
        | crate::config::CursorTrail::Orb
        | crate::config::CursorTrail::Liquid => {
            let style = term.cursor_style();
            let rect = followed.map(|f| {
                let (r, c) = f.cell();
                renderer.cursor_rect(r, c, f.style(style))
            });
            chase_frame(chase, term, rect, HostTime::of_secs(now), (cell, height))
        }
        crate::config::CursorTrail::Off => None,
    }
}

/// **The ghost this frame draws**, from the sweep in flight at `now`, the
/// display link's clock: none once it has landed -- this frame draws
/// `set_trail(None)`, whose `trail_moved` dirties the ghost's last rows to
/// clear it, and the tick stops forcing frames -- and none while the window
/// looks back into the history, where the sweep's live rows are not what
/// is drawn: it is dropped there, as the chase arms snap.
fn ghost_frame(trail: &mut Option<TrailState>, term: &Term<'_>, now: f64) -> Option<Trail> {
    if term.window_back().index() != 0 {
        *trail = None;
    }
    let st = (*trail)?;
    let p = crate::trail::ghost_progress(st.start, now);
    if p >= 1.0 {
        *trail = None;
        return None;
    }
    Some(Trail {
        from: st.from,
        to: st.to,
        progress: p.max(0.0),
        origin_dy: st.origin_dy,
        lift: Px::of(0),
        strength: st.strength,
    })
}

/// **The chase trail this frame draws**, toward `cursor`, the rectangle
/// of the cursor `trail_cursor` follows, at `now`, the display link's
/// clock. Looking back forgets where the cursor was, as a resize does:
/// output may have moved it meanwhile, and a trail on the way back would
/// come from a place the reader never saw it leave, and a trail drawing is
/// gone at once. A hidden cursor keeps it, and a trail drawing fades out
/// -- `trail.rs`.
fn chase_frame(
    chase: &mut crate::trail::Chase<HostTime>,
    term: &Term<'_>,
    cursor: Option<baud_render::Rect>,
    now: Option<HostTime>,
    (cell, height): ((f32, f32), baud_render::Points),
) -> Option<TrailLook> {
    if term.window_back().index() != 0 {
        chase.snap();
    }
    now.and_then(|now| chase.frame(cursor, now, cell, height))
}

/// What the app keeps over the screen and a read can leave naming cells
/// that are gone: the selection, the echoes drawn ahead of the shell and
/// their frame, the slide, the wheel's travel not yet a line, and the
/// instrument the guesses' verdicts go to. Borrowed from `App` for one call,
/// so the rule is a plain function a test runs over a real `Term`.
struct Kept<'a> {
    selection: &'a mut Option<Selection>,
    predictions: &'a mut Vec<Prediction>,
    guess: &'a mut Guess,
    slide: &'a mut Slide,
    travel: &'a mut Travel,
    latency: &'a mut Latency,
}

/// Whether a read put another surface up, for what follows the cursor and
/// the fade the pump runs on one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Surface {
    Switched,
    Same,
}

/// Whether the scrollbar's thumb is revealed after a read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Thumb {
    Reveal,
    Leave,
}

/// **What a read that put another surface up, or reset the terminal, owes
/// `kept`**, and what the pump does after: `before` was taken with the
/// erasure watch armed ([`Before::watched`]).
///
/// Note V's V1: when the reset count moves, the selection goes -- it names
/// cells the power-on state does not have, as foot's hard reset and xterm's
/// `RIS` both drop theirs -- the guesses drawn ahead of the shell are
/// dropped as contradicted, the wheel's part-line settles, and a window over
/// the history that came to the live screen reveals its thumb, as a key's
/// snap does (V1's first fork). **The window is the one the reset took live,
/// at the reset's byte**, which the core keeps with the first erasure
/// (`Erasure::Reset`, note V, V2), so the answer is the same however the
/// bytes fell into pumps: the primary's window is drawn only while the
/// primary is up, and any erasure takes it live, so only the first erasure
/// in a pump can reveal, and only a reset made from the primary while the
/// window was off the live screen. Read at the pump's edges it was the
/// window drawn before the pump, and `?1049h` then `RIS` revealed in one
/// pump and not in two; read as the window at the pump's start, a
/// full-screen `SD`, `RI` at the top or a prompt redraw that took it live
/// earlier in the same pump still revealed. The slide is the pump's erase
/// arm's, which `RIS` moves too. **No fade on a primary reset** (V1's second
/// fork): it is not a switch, so [`Surface`] stays the epoch's; a reset from
/// the alternate screen is both, and fades as a switch does.
fn restart(term: &Term<'_>, before: Before, kept: Kept<'_>) -> (Surface, Thumb) {
    let surface = match switched(term, before) {
        true => Surface::Switched,
        false => Surface::Same,
    };
    let reset = reset_in(term, before);
    if surface == Surface::Switched || reset {
        *kept.selection = None;
        // Each with its verdict, as `judge` gives one: a bare clear left a
        // key's record under `BAUD_KEYLAT` waiting for the instrument's own
        // timeout.
        for p in kept.predictions.drain(..) {
            if let Some(seq) = p.seq {
                kept.latency.judged(seq, Judged::Contradicted);
            }
        }
        *kept.guess = Guess::None;
        kept.travel.settle();
    }
    if surface == Surface::Switched {
        // A surface switch: the slide's offset names a row the new surface
        // does not have (the alternate screen keeps no ring), so settle it
        // rather than carry it across. A reset's slide is the erase arm's.
        kept.slide.snap();
    }
    let thumb = match term.first_erasure(before.erased) {
        Some(Erasure::Reset(Screen::Normal, Looking::Back)) => Thumb::Reveal,
        _ => Thumb::Leave,
    };
    (surface, thumb)
}

#[cfg(test)]
mod ghost_tests {
    use super::{GhostRide, TrailState, ghost_frame};
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Col, ReadLen, Row, Rows, WinSize};

    /// **Looking back drops a ghost in flight**, as the chase arms snap:
    /// its sweep names live rows, and the window shows the history there.
    /// The control is the same sweep at the same moment on the live
    /// screen. Perturbation: keep the ghost while the window looks back --
    /// it is drawn over the history and the second assertion fails.
    #[test]
    fn looking_back_drops_the_ghost_in_flight() {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 10,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let lines: Vec<u8> = (0..30)
            .flat_map(|i| format!("{i}\r\n").into_bytes())
            .collect();
        t.write_at(BytePos::ZERO, &lines);
        t.feed(BytePos::ZERO, ReadLen::from_index(lines.len()));
        let sweep = Some(TrailState {
            from: (Row::from_index(9), Col::ZERO),
            to: (Row::from_index(9), Col::at_index(1)),
            start: 1.0,
            origin_dy: 0.0,
            ride: GhostRide::Riding(0.0),
            strength: baud_render::Strength::Full,
        });
        let mut live = sweep;
        assert!(ghost_frame(&mut live, &t, 1.05).is_some(), "the control");
        t.set_window_back(Rows::of_count(3));
        let mut back = sweep;
        assert_eq!(ghost_frame(&mut back, &t, 1.05), None, "over the history");
        assert!(back.is_none(), "and dropped");
        t.window_live();
        assert_eq!(ghost_frame(&mut back, &t, 1.06), None, "not back on return");
    }

    /// **Looking back drops a chase trail in flight, and the way back
    /// draws none stale**: the two smears and the seven effects, each
    /// caught drawing a long move when the window scrolls back three lines.
    /// While it looks back no frame draws one; output then moves the
    /// cursor, and on the live screen again neither the first sighting nor
    /// the frames after the dwell trail from where the cursor was before
    /// the reader looked away. The controls: each trail draws on the live
    /// screen, and the window is still back after the output.
    /// Perturbation: the snap removed -- the chase keeps the cell it last
    /// saw and trails from it once the dwell has passed on the way back.
    #[test]
    fn looking_back_snaps_the_chase_in_flight() {
        use super::{TrailCursor, chase_frame, trail_cursor};
        use crate::config::CursorTrail;
        use baud_domain::HostTime;
        let ws = WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let size = ((10.0, 20.0), baud_render::Points(20.0));
        let rect = |f: TrailCursor| {
            let (r, c) = f.cell();
            let (x, y) = (c.index() as f32 * 10.0, r.index() as f32 * 20.0);
            baud_render::Rect {
                left: x,
                top: y,
                right: x + 10.0,
                bottom: y + 20.0,
            }
        };
        let ms = |n: u64| Some(HostTime::of_nanos(n * 1_000_000));
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        for trail in [
            CursorTrail::Sweep,
            CursorTrail::Quad,
            CursorTrail::Fire,
            CursorTrail::Plasma,
            CursorTrail::Lightning,
            CursorTrail::Ice,
            CursorTrail::Ufo,
            CursorTrail::Orb,
            CursorTrail::Liquid,
        ] {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let lines: Vec<u8> = (0..30)
                .flat_map(|i| format!("{i}\r\n").into_bytes())
                .collect();
            feed(&mut t, &lines);
            let mut chase = crate::trail::Chase::new(trail.style());
            let mut frame = |t: &Term<'_>, n: u64| {
                let cursor = trail_cursor(t).map(rect);
                chase_frame(&mut chase, t, cursor, ms(n), size)
            };
            let _ = frame(&t, 1);
            feed(&mut t, b"\x1b[1;30H");
            let _ = frame(&t, 5);
            assert!(
                frame(&t, 20).is_some(),
                "{trail:?}: the control, drawn live"
            );
            t.set_window_back(Rows::of_count(3));
            feed(&mut t, b"\x1b[10;1H");
            assert_eq!(t.window_back().index(), 3, "{trail:?}: still back");
            for n in [30, 45, 60] {
                assert_eq!(frame(&t, n), None, "{trail:?}: none back at {n} ms");
            }
            t.window_live();
            for n in [70, 85, 100, 130] {
                assert_eq!(frame(&t, n), None, "{trail:?}: none returned at {n} ms");
            }
        }
    }
}

/// **A window over the history is the primary screen's.** The alternate
/// screen keeps none, so a window left scrolled back when a program enters
/// it is the live screen for everything that reads one, and the primary's
/// ring alone moves it, after foot, which keeps a view per grid.
#[cfg(test)]
mod alt_window_tests {
    use super::{
        Before, Hold, Ring, erased, hold_after, live_cursor, live_moved, numbered, oldest,
        snap_live, stale_in_moved_rows, switched, whole_screen_scrolled, window_of,
    };
    use baud_core::{Depth, Owned, Screen, Term};
    use baud_domain::{BytePos, ReadLen, Rows, WinSize};
    use baud_font::Px;
    use baud_render::Scrolled;
    use baud_render::{Hover, thumb};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 10,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// Forty lines on a ten-row screen: thirty-one retained, the cursor on
    /// the bottom row, where a newline retires one more.
    fn with_history(f: impl FnOnce(&mut Term<'_>)) {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let lines: Vec<u8> = (0..40)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        assert_eq!(t.scrollback().len(), 31);
        f(&mut t);
    }

    fn drawn(t: &Term<'_>) -> bool {
        let widths = crate::reveal::widths(crate::reveal::Scale(2.0));
        let size = (Px::of(800), Px::of(500));
        thumb(window_of(t), size, Px::of(10), widths, Hover::Rest).is_some()
    }

    /// The terminal's window put `back` lines up the primary's history,
    /// `bytes` fed as one read, and where the primary's window is after it.
    fn window_across(t: &mut Term<'_>, back: Rows, bytes: &[u8]) -> Rows {
        t.set_window_back(back);
        feed(t, bytes);
        t.primary_window_back()
    }

    /// The same across a resize to `ws`.
    fn window_resized(t: &mut Term<'_>, ws: &WinSize, back: Rows) -> Rows {
        t.set_window_back(back);
        let _ = t.resize(ws);
        t.primary_window_back()
    }

    #[test]
    fn on_the_alternate_screen_the_window_is_the_live_screen() {
        with_history(|t| {
            t.set_window_back(Rows::of_count(3));
            // The control, on the primary: three lines up, a thumb, and no
            // cursor -- it belongs to the live screen below the window.
            assert_eq!(t.window_back().index(), 3);
            assert!(drawn(t), "a thumb while looking back");
            assert_eq!(live_cursor(t), None);
            feed(t, b"\x1b[?1049h\x1b[Hvi");
            assert_eq!(t.window_back().index(), 0, "nothing behind the screen");
            assert_eq!(t.primary_window_back().index(), 3, "the primary's is kept");
            assert!(!drawn(t), "no thumb over a screen with no history");
            assert!(
                live_cursor(t).is_some(),
                "the program that owns the screen owns the cursor"
            );
        });
    }

    #[test]
    fn the_alternate_screens_output_does_not_move_the_primarys_window() {
        with_history(|t| {
            let mut back = Rows::of_count(3);
            t.set_window_back(back);
            let read = |t: &mut Term<'_>, bytes: &[u8], back: &mut Rows| {
                feed(t, bytes);
                *back = t.primary_window_back();
            };
            read(t, b"\x1b[?1049h\x1b[10;1H", &mut back);
            assert_eq!(back.index(), 3, "entering it moved nothing");
            read(t, b"a\r\nb\r\nc\r\nd\r\n", &mut back);
            assert_eq!(back.index(), 3, "its scrolling turned its own ring");
            read(t, b"\x1b[?1049l", &mut back);
            assert_eq!(back.index(), 3, "and leaving it finds the lines again");
            // The control: output on the primary still carries the window
            // up with the lines it shows.
            read(t, b"e\r\nf\r\n", &mut back);
            assert_eq!(back.index(), 5);
        });
    }

    #[test]
    fn a_read_that_switches_screens_anchors_by_what_the_primary_scrolled() {
        // Into the alternate screen within one read: two lines retired on
        // the primary before the switch, four on the alternate after it.
        with_history(|t| {
            assert_eq!(
                window_across(
                    t,
                    Rows::of_count(3),
                    b"a\r\nb\r\n\x1b[?1049h\x1b[10;1Hw\r\nx\r\ny\r\nz\r\n"
                )
                .index(),
                5
            );
        });
        // Out of it within one read: four on the alternate, one on the
        // primary after it.
        with_history(|t| {
            feed(t, b"\x1b[?1049h\x1b[10;1H");
            assert_eq!(
                window_across(
                    t,
                    Rows::of_count(3),
                    b"w\r\nx\r\ny\r\nz\r\n\x1b[?1049lc\r\n"
                )
                .index(),
                4
            );
        });
        // In and out within one read: the one case the live rotation
        // already answered, because both its ends are the primary's -- kept
        // so the history mark is held to it too.
        with_history(|t| {
            assert_eq!(
                window_across(
                    t,
                    Rows::of_count(3),
                    b"a\r\nb\r\n\x1b[?1049h\x1b[10;1Hw\r\nx\r\n\x1b[?1049lc\r\n"
                )
                .index(),
                6
            );
        });
    }

    /// The live surface's movement is the selection's line count and a
    /// slide's, and across a read that switches screens its two rotations
    /// are two rings': the primary's, hundreds of rows round a ring of
    /// thousands, against the alternate screen's ring of one screen. Read
    /// as one, that subtracted past zero -- a panic in a debug build and a
    /// meaningless count in a release one.
    #[test]
    fn the_live_movement_is_nothing_across_a_switch() {
        with_history(|t| {
            // The control: on one screen it is the rotation's advance.
            let before = Before::of(t);
            feed(t, b"a\r\nb\r\n");
            assert_eq!(live_moved(t, before), Ring::Retired(Rows::of_count(2)));
            let before = Before::of(t);
            feed(t, b"\x1b[?1049h\x1b[10;1Hw\r\n");
            assert_eq!(live_moved(t, before), Ring::Retired(Rows::ZERO));
            let before = Before::of(t);
            feed(t, b"x\r\n\x1b[?1049lc\r\n");
            assert_eq!(live_moved(t, before), Ring::Retired(Rows::ZERO));
        });
    }

    /// A region anchored at the top retires its line the way the whole screen
    /// does, and the rows under it stay: a slide must not pan them. The core
    /// counts it, so a read that sets a region, scrolls it and resets it
    /// before it ends is still seen -- the region at the read's end says
    /// nothing about what scrolled during it.
    #[test]
    fn a_region_scroll_is_not_a_whole_screen_scroll() {
        with_history(|t| {
            // The control: the whole screen scrolling.
            let before = Before::of(t);
            feed(t, b"a\r\nb\r\n");
            assert!(matches!(live_moved(t, before), Ring::Retired(n) if n.index() == 2));
            assert!(whole_screen_scrolled(t, before));
            let before = Before::of(t);
            feed(t, b"\x1b[1;5r\x1b[5;1Hc\r\nd\r\n\x1b[r");
            assert!(matches!(live_moved(t, before), Ring::Retired(n) if n.index() == 2));
            assert!(!whole_screen_scrolled(t, before), "reset inside the read");
            // A region in the middle retires nothing, so there is nothing to
            // slide either way.
            let before = Before::of(t);
            feed(t, b"\x1b[6;8r\x1b[8;1He\r\nf\r\n\x1b[r");
            assert!(whole_screen_scrolled(t, before));
        });
    }

    /// **A region retire keeps a selection on the rows under the region on
    /// their text, and one in the region moves with its rows.** The rows
    /// under it stayed while the count advanced, so the core carries the
    /// selection there against the count (since 2026-10-20; it was dropped
    /// until then); a whole-screen scroll moves every row; a read that
    /// resets its region after the scroll is judged by the scroll's own
    /// rows. At every read size and every split into two reads, each kept
    /// selection on its own text.
    #[test]
    fn a_region_retire_keeps_a_selection_on_the_rows_under_it() {
        let setup: &[u8] = b"\x1b[1;5r\x1b[7;1Hdash\x1b[3;1Hpane";
        let scroll: &[u8] = b"\x1b[5;1Hx\r\ny\r\n";
        with_history(|t| {
            feed(t, setup);
            let before = Before::of(t);
            feed(t, scroll);
            assert!(
                !whole_screen_scrolled(t, before),
                "the control: a region retired"
            );
        });
        let reset: &[u8] = b"\x1b[5;1Hw\r\n\x1b[r";
        // Line feeds alone, which write on no row a selection across the
        // region's bottom holds.
        let lfs: &[u8] = b"\x1b[5;1H\n\n";
        let mut wrong = Vec::new();
        for (prog, sel, what) in [
            (scroll, (6, 6), "under it"),
            (scroll, (2, 2), "in it"),
            (lfs, (2, 6), "across its bottom, the region's part kept"),
            (reset, (2, 2), "the region reset after it"),
            (reset, (6, 6), "and under it"),
        ] {
            for w in wrong_feeds(Depth::DEFAULT, setup, &[prog], sel, Want::Kept) {
                wrong.push(format!("{what}: {w}"));
            }
        }
        let whole: &[&[u8]] = &[b"\x1b[r", b"\x1b[10;1Hz\r\n"];
        assert!(!drops(setup, whole, (0, 6)), "every row moved");
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// **And the same on the alternate screen**, where vim's tall region
    /// rotates the ring and the status line under it stays. The core counted
    /// region scrolls on the primary only, so here the read once looked like
    /// a whole-screen scroll and a selection on the status line was
    /// renumbered onto the row above it; then it was dropped; now it stays.
    #[test]
    fn a_region_rotation_on_the_alternate_screen_keeps_a_selection_under_it() {
        // A tall region on this ten-row screen, `1;8`, so it rotates on
        // the alternate screen as vim's `1;45` does on fifty.
        let setup: &[u8] = b"\x1b[?1049h\x1b[1;8r\x1b[9;1Hstat\x1b[3;1Hpane";
        let scroll: &[u8] = b"\x1b[8;1Hx\r\ny\r\n";
        with_history(|t| {
            feed(t, setup);
            let (before, rot) = (Before::of(t), t.rotation());
            feed(t, scroll);
            assert!(
                t.rotation() != rot,
                "the control: the region rotated the alternate ring"
            );
            assert!(
                !whole_screen_scrolled(t, before),
                "and that was a region, not the whole screen"
            );
        });
        let mut wrong = Vec::new();
        for (sel, what) in [((8, 8), "under it"), ((2, 2), "in it")] {
            for w in wrong_feeds(Depth::DEFAULT, setup, &[scroll], sel, Want::Kept) {
                wrong.push(format!("{what}: {w}"));
            }
        }
        wrong.extend(wrong_feeds(
            Depth::DEFAULT,
            setup,
            &[b"\x1b[8;1H\r\n\r\n\r\n"],
            (0, 1),
            Want::Dropped,
        ));
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// Whether the moved-rows check drops a selection on screen row `y` of
    /// the view `back` lines up, made after `setup` and held across `reads`,
    /// each fed as the pump feeds one: the row watch armed on the selection
    /// as the count numbers it, the check after the feed, and the count
    /// advanced by what the read retired. The selection is numbered as the
    /// driver would have it after the thirty-one lines the history holds, so
    /// a line behind the screen has a number of its own rather than the
    /// floor's.
    fn drops(setup: &[u8], reads: &[&[u8]], at: (usize, usize)) -> bool {
        drops_to(setup, reads, at, at.1)
    }

    /// [`drops`], for a selection from screen row `y` to screen row `last`
    /// of the view `back` lines up.
    fn drops_to(setup: &[u8], reads: &[&[u8]], (back, y): (usize, usize), last: usize) -> bool {
        use baud_domain::{Col, Row};
        use baud_render::{Grain, Selection, Span};
        let mut dropped = false;
        with_history(|t| {
            let mut scrolled = Scrolled::START.advanced(Rows::of_count(31));
            let before = Before::of(t);
            feed(t, setup);
            if let Ring::Retired(n) = live_moved(t, before) {
                scrolled = scrolled.advanced(n);
            }
            let span = |t: &Term<'_>, y: usize| {
                let r = Row::from_index(y);
                Span::cell(
                    t.row(r).expect("a row on the grid"),
                    scrolled.line(Rows::of_count(back), r),
                    Col::ZERO,
                )
            };
            let mut sel = Selection::at(span(t, y), Grain::Cell);
            sel.extend(span(t, last));
            for bytes in reads {
                let before = Before::watched(t, Some(&sel), scrolled);
                feed(t, bytes);
                if stale_in_moved_rows(t) {
                    dropped = true;
                    return;
                }
                scrolled = match numbered(t, before, None) {
                    Ring::Retired(n) => scrolled.advanced(n),
                    Ring::Restored(n) => match scrolled.retreated(n) {
                        Some(s) => s,
                        None => {
                            dropped = true;
                            return;
                        }
                    },
                };
                let mut kept = Some(sel);
                super::followed(t, &mut kept, scrolled);
                if let Some(s) = kept {
                    sel = s;
                }
            }
        });
        dropped
    }

    /// **Rows a region moved in place carry a selection on them with their
    /// text** (since 2026-10-20, as xterm does; dropped until then). A
    /// region in the middle of the screen scrolls by copying its rows, so
    /// no ring moves and the count does not renumber the selection: the
    /// core's watch carries it a row up with the pane's line. Above the
    /// region and under it nothing moved, and the history is not the live
    /// screen. A selection from above the region into its first row keeps
    /// only that row, which the scroll takes out: dropped, as xterm
    /// disowns it. A read that copies and then resets the region is judged
    /// by the copy's rows; `IL` with no region drops one holding the
    /// cursor's row, as xterm's `InsertLine` does, and one on the row it
    /// pushes out, and moves the rest. At every read size and split.
    #[test]
    fn rows_moved_in_place_carry_a_selection_with_their_text() {
        use baud_domain::Row;
        let setup: &[u8] = b"\x1b[3;1Habove\x1b[5;1Hpane\x1b[8;1Hunder\x1b[4;6r";
        with_history(|t| {
            feed(t, setup);
            let before = Before::of(t);
            feed(t, b"\x1b[6;1H\n");
            assert_eq!(
                live_moved(t, before),
                Ring::Retired(Rows::ZERO),
                "the control: no ring moved"
            );
            assert_eq!(
                t.row(Row::from_index(3)).expect("a row on the grid")[0]
                    .content
                    .as_codepoint()
                    .map(|c| c.value()),
                Some(u32::from(b'p')),
                "and the text did: the pane's line is a row up"
            );
        });
        let copy: &[u8] = b"\x1b[6;1H\n";
        let reset: &[u8] = b"\x1b[6;1H\n\x1b[r";
        let il: &[u8] = b"\x1b[r\x1b[9;1H\x1b[L";
        let carried: &[u8] = b"\x1b[r\x1b[10;1H\n\n\x1b[4;6r\x1b[6;1H\n";
        let mut wrong = Vec::new();
        for (prog, sel, want, what) in [
            (copy, (4, 4), Want::Kept, "in the region"),
            (copy, (4, 5), Want::Kept, "the region's two lower rows"),
            (copy, (2, 2), Want::Kept, "above it"),
            (copy, (7, 7), Want::Kept, "under it"),
            (copy, (-5, -5), Want::Kept, "in the history"),
            (
                copy,
                (1, 3),
                Want::Dropped,
                "from above it into its first row",
            ),
            (
                copy,
                (0, 2),
                Want::Kept,
                "one that ends on the row above it",
            ),
            (reset, (4, 4), Want::Kept, "copied, then the region reset"),
            (reset, (2, 2), Want::Kept, "and the row above it"),
            (il, (8, 8), Want::Dropped, "IL, at the cursor's row"),
            (il, (9, 9), Want::Dropped, "IL, the row it pushes out"),
            (il, (7, 7), Want::Kept, "IL, the row above the cursor"),
            (
                carried,
                (7, 7),
                Want::Kept,
                "a row lines retired carried in",
            ),
            (carried, (3, 3), Want::Kept, "and one they carried above it"),
        ] {
            for w in wrong_feeds(Depth::DEFAULT, setup, &[prog], sel, want) {
                wrong.push(format!("{what}: {w}"));
            }
        }
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// **A write or an erase on a selected row drops the selection, whatever
    /// it writes, and one on another row keeps it**. An erase on row 0 and
    /// an `IL` on row 9 in one read drop a selection on row 0, which the
    /// move does not reach, read whole, at every read size and cut into two
    /// reads at every byte. `ICH`, `DCH`, `DECIC` and `DECDC` drop it with
    /// `?1040` set, the power-on state, where xterm shifts it (since
    /// 2026-10-22).
    #[test]
    fn a_write_or_an_erase_on_a_selected_row_drops_it() {
        let setup: &[u8] = b"\x1b[1;1Hhello\x1b[5;1Hworld\x1b[3;1H";
        let (first, other) = ((0, 0), (0, 4));
        let read: &[u8] = b"\x1b[1;1H\x1b[2K\x1b[10;1H\x1b[L";
        for k in 0..read.len() {
            let reads: Vec<&[u8]> = [&read[..k], &read[k..]]
                .into_iter()
                .filter(|r| !r.is_empty())
                .collect();
            assert!(drops(setup, &reads, first), "the erased row, cut at {k}");
            assert!(!drops(setup, &reads, other), "a row untouched, cut at {k}");
        }
        for size in 1..read.len() {
            let reads: Vec<&[u8]> = read.chunks(size).collect();
            assert!(drops(setup, &reads, first), "the erased row, {size} a read");
            assert!(
                !drops(setup, &reads, other),
                "a row untouched, {size} a read"
            );
        }
        for (read, what) in [
            (b"\x1b[1;1H\x1b[2K".as_slice(), "EL"),
            (b"\x1b[1;1HXY", "text over it"),
            (b"\x1b[1;1Hhello", "the same text"),
            (b"\x1b[1;1H\x1b[@", "ICH"),
            (b"\x1b[1;1H\x1b[P", "DCH"),
            // In a region of rows 1-3, so row 5 is out of their reach.
            (b"\x1b[1;3r\x1b[1;1H\x1b['}\x1b[r", "DECIC"),
            (b"\x1b[1;3r\x1b[1;1H\x1b['~\x1b[r", "DECDC"),
        ] {
            assert!(drops(setup, &[read], first), "{what}");
            assert!(!drops(setup, &[read], other), "{what}: another row");
        }
        assert!(
            !drops(setup, &[b"\x1b[5;1HXY\x1b[2K\x1b[1;1H"], first),
            "a write and an erase on another row, and a visit"
        );
        assert!(
            !drops(setup, &[b"\x1b[1;3r\x1b[1;1H\x1b[r"], first),
            "the control: the region set and reset, and no column edit"
        );
    }

    /// **A region set after the copy does not hide it.** A read that copies
    /// in one region and then sets another -- stacked panes, each drawn
    /// under its own -- ends with a region that never moved; the copy's own
    /// rows are what count, and the selection on the pane's line goes up a
    /// row with it, at every read size and split. The row above it and the
    /// history stay.
    #[test]
    fn a_region_set_after_the_copy_still_carries_a_selection_it_moved() {
        use baud_domain::Row;
        let setup: &[u8] = b"\x1b[5;1Hpane";
        let read: &[u8] = b"\x1b[4;6r\x1b[6;1H\n\x1b[8;9r";
        with_history(|t| {
            feed(t, setup);
            let before = Before::of(t);
            feed(t, read);
            assert_eq!(
                live_moved(t, before),
                Ring::Retired(Rows::ZERO),
                "the control: no ring moved"
            );
            assert_eq!(
                t.row(Row::from_index(3)).expect("a row on the grid")[0]
                    .content
                    .as_codepoint()
                    .map(|c| c.value()),
                Some(u32::from(b'p')),
                "and the pane's line is a row up"
            );
        });
        let mut wrong = Vec::new();
        for (sel, what) in [
            ((4, 4), "the pane's moved line"),
            ((2, 2), "the row above it"),
            ((-5, -5), "the history"),
        ] {
            for w in wrong_feeds(Depth::DEFAULT, setup, &[read], sel, Want::Kept) {
                wrong.push(format!("{what}: {w}"));
            }
        }
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// **A region set and set back within one read**: begun in 8;9, copied
    /// in 4;6 and ended in 8;9. The pane's line went up a row and its
    /// selection with it; the row above both regions and the end region's
    /// first row are where they were, and the history is not the live
    /// screen. The same at every read size and every split into two reads,
    /// each kept selection on its own text, so the bytes and not where the
    /// reads fall decide: judged by the region at the read's two ends, the
    /// read whole kept the pane's selection on the row above its text, and
    /// the reads that held the copy with only one of the two region changes
    /// dropped the row above.
    #[test]
    fn a_region_set_and_set_back_within_one_read_is_judged_as_in_separate_reads() {
        let setup: &[u8] = b"\x1b[3;1Habove\x1b[5;1Hpane\x1b[8;1Hunder\x1b[8;9r";
        let prog: &[u8] = b"\x1b[4;6r\x1b[6;1H\n\x1b[8;9r";
        let mut wrong = Vec::new();
        for (sel, what) in [
            ((4, 4), "the pane's moved line"),
            ((2, 2), "the row above both regions"),
            ((7, 7), "the end region's first row"),
            ((-5, -5), "the history"),
        ] {
            for w in wrong_feeds(Depth::DEFAULT, setup, &[prog], sel, Want::Kept) {
                wrong.push(format!("{what}: {w}"));
            }
        }
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// Every feed of `prog`: the read whole, every read size, and every
    /// split into two reads.
    fn every_feed(prog: &[u8]) -> Vec<(String, Vec<&[u8]>)> {
        let mut feeds: Vec<(String, Vec<&[u8]>)> = vec![("whole".into(), vec![prog])];
        for size in 1..prog.len() {
            feeds.push((format!("read size {size}"), prog.chunks(size).collect()));
        }
        for at in 1..prog.len() {
            let (a, b) = prog.split_at(at);
            feeds.push((format!("split at {at}"), vec![a, b]));
        }
        feeds
    }

    /// What the pump leaves of a selection on live screen row `r`, made
    /// after `setup` and held across `reads`: [`pump_keeps_lines`] on the
    /// default history. `None` when it was dropped; else the text the
    /// selection was made on and the text of the line it names now, on the
    /// screen or in the history.
    fn pump_keeps(
        setup: &[u8],
        reads: &[&[u8]],
        r: baud_domain::Row,
    ) -> Option<(String, Option<String>)> {
        let y = i32::try_from(r.index()).expect("a screen row");
        let mut lines = pump_keeps_lines(Depth::DEFAULT, setup, reads, (y, y))?;
        lines.pop()
    }

    /// **A region anchored at the top that scrolls keeps a selection under
    /// it on its text, whatever region the read ends in.** The scroll
    /// advances the ring and the rows under the region stay, so the core
    /// carries a selection there against the count (dropped until then).
    /// Begun in 1;9, scrolled in 1;5: then a copy in 8;9 and back to 1;9,
    /// or straight back to 1;9. Rows 5 to 9 were under 1;5 and stay on
    /// their own text, but for the first program's row 7, which the copy
    /// then takes out of 8;9's top. Rows 1 to 4 were in it and moved with
    /// the ring, and row 0 went to the history: kept on their own text, row
    /// 0's in the history. At every read size and every split into two
    /// reads, no feed keeps one on another row's text.
    #[test]
    fn a_top_regions_scroll_keeps_a_selection_under_it_at_every_read_size() {
        use baud_domain::Row;
        let setup: &[u8] = b"\x1b[1;9r";
        let progs: [&[u8]; 2] = [
            b"\x1b[1;5r\x1b[5;1H\n\x1b[8;9r\x1b[9;1H\n\x1b[1;9r",
            b"\x1b[1;5r\x1b[5;1H\n\x1b[1;9r",
        ];
        // The control: the selection's text is what the label says it is.
        assert_eq!(
            pump_keeps(setup, &[b"x"], Row::from_index(7)),
            Some(("line 38".into(), Some("line 38".into())))
        );
        let (mut wrong, mut wrong_feeds, mut fed) = (Vec::new(), 0, 0);
        for (p, prog) in progs.iter().enumerate() {
            for (name, reads) in every_feed(prog) {
                fed += 1;
                let before = wrong.len();
                for y in 0..10 {
                    let got = pump_keeps(setup, &reads, Row::from_index(y));
                    let right = match (p, y, &got) {
                        (0, 7, None) => true,
                        (_, _, Some((label, now))) => now.as_ref() == Some(label),
                        _ => false,
                    };
                    if !right {
                        wrong.push(format!("{:?} {name}: row {y} {got:?}", prog.len()));
                    }
                }
                wrong_feeds += usize::from(wrong.len() > before);
            }
        }
        assert!(
            wrong.is_empty(),
            "{} wrong answers on {wrong_feeds} of {fed} feeds: {wrong:?}",
            wrong.len()
        );
    }

    /// **A line the whole screen's scroll down takes back drops a selection
    /// on it, at every read size.** `SD` and `RI` at the top take a line back
    /// and blank it on the top row, and push the last row under the window,
    /// where a later line feed brings it back blank. A read that also
    /// retires lines can net none taken back, so the drop on lines taken
    /// back does not see it, and neither does the drop on rows moved in
    /// place unless the core tests those two rows. Five programs on the
    /// labelled screen, each at every read size and every split into two
    /// reads: no feed keeps a selection on text other than its own.
    #[test]
    fn a_line_taken_back_drops_a_selection_on_it_at_every_read_size() {
        use baud_domain::Row;
        let setup: Vec<u8> = (0..10)
            .flat_map(|y| format!("\x1b[{};1HP{y}", y + 1).into_bytes())
            .collect();
        let progs: [&[u8]; 5] = [
            b"\x1b[2S\x1b[2T",
            b"\x1b[2S\x1b[2T\x1b[10;1H\x1b[L",
            b"\x1b[2S\x1b[T\x1b[4;6r\x1b[6;1H\n",
            b"\n\n\n\x1b[2S\x1b[T\x1b[L",
            b"\x1b[T\x1b[10;1H\n",
        ];
        // The control: the selection's text is what the label says it is.
        assert_eq!(
            pump_keeps(&setup, &[b"x"], Row::from_index(1)),
            Some(("P1ne 32".into(), Some("P1ne 32".into())))
        );
        let (mut wrong, mut wrong_feeds, mut fed) = (Vec::new(), 0, 0);
        for prog in progs {
            for (name, reads) in every_feed(prog) {
                fed += 1;
                let before = wrong.len();
                for y in 0..10 {
                    let got = pump_keeps(&setup, &reads, Row::from_index(y));
                    let right = match &got {
                        None => true,
                        Some((label, now)) => now.is_none() || now.as_ref() == Some(label),
                    };
                    if !right {
                        let p = String::from_utf8_lossy(prog);
                        wrong.push(format!("{p:?} {name}: row {y} {got:?}"));
                    }
                }
                wrong_feeds += usize::from(wrong.len() > before);
            }
        }
        assert!(
            wrong.is_empty(),
            "{} wrong answers on {wrong_feeds} of {fed} feeds: {wrong:?}",
            wrong.len()
        );
    }

    /// **A read that moves the alternate screen further than half its ring
    /// drops a selection, at every read size.** The ring's rotation, read at
    /// a read's two ends, says one line where a ring and a line went by, so
    /// a selection renumbered by it lands on rows that are not its own; an
    /// `IL` in the same read, under the selection, used to drop it by the
    /// count of moves in place. On the test terminal's ten-row alternate
    /// ring, `IND`, `IL`, `3 IL`, two `2 SU` and six line feeds: no feed
    /// keeps a selection on rows 1 to 6 on text other than its own.
    #[test]
    fn a_read_past_half_the_alternate_ring_drops_a_selection_at_every_read_size() {
        use baud_domain::Row;
        let mut setup: Vec<u8> = b"\x1b[?1049h".to_vec();
        setup.extend((0..10).flat_map(|y| format!("\x1b[{};1HA{y}", y + 1).into_bytes()));
        let prog: &[u8] = b"\x1bD\x1b[L\x1b[3L\x1b[2S\x1b[2S\n\n\n\n\n\n";
        // The control: the selection's text is what the label says it is.
        assert_eq!(
            pump_keeps(&setup, &[b"x"], Row::from_index(1)),
            Some(("A1".into(), Some("A1".into())))
        );
        let (mut wrong, mut wrong_feeds, mut fed) = (Vec::new(), 0, 0);
        for (name, reads) in every_feed(prog) {
            fed += 1;
            let before = wrong.len();
            for y in 1..=6 {
                let got = pump_keeps(&setup, &reads, Row::from_index(y));
                let right = match &got {
                    None => true,
                    Some((label, now)) => now.is_none() || now.as_ref() == Some(label),
                };
                if !right {
                    wrong.push(format!("{name}: row {y} {got:?}"));
                }
            }
            wrong_feeds += usize::from(wrong.len() > before);
        }
        assert!(
            wrong.is_empty(),
            "{} wrong answers on {wrong_feeds} of {fed} feeds: {wrong:?}",
            wrong.len()
        );
    }

    /// Ten labelled rows over the forty lines `with_history` writes -- `P0`
    /// to `P9` over "line 31" to "line 40", or on the alternate screen `A0`
    /// to `A9` -- so a row's text says which row it was.
    fn labelled(screen: Screen) -> Vec<u8> {
        let (mut setup, mark): (Vec<u8>, _) = match screen {
            Screen::Alternate => (b"\x1b[?1049h".to_vec(), "A"),
            Screen::Normal => (Vec::new(), "P"),
        };
        setup.extend((0..10).flat_map(|y| format!("\x1b[{};1H{mark}{y}", y + 1).into_bytes()));
        setup
    }

    /// What the pump leaves of a selection on lines `a` to `b` of the live
    /// surface -- a screen row, or for a negative number a line of the
    /// history that many lines above the screen -- made after `setup` on the
    /// forty lines a terminal of `depth` saved lines holds, and held across
    /// `reads`, each judged as `pump` judges one and in its order: the watch
    /// on the selection's lines, the renumbering by the lines the read moved
    /// the surface, a switch or a reset, and the clip at the oldest line the
    /// ring holds. `None` when it was dropped; else, line by line, the text
    /// it was made on and the text of the line it names now, `None` for a
    /// line it no longer reaches.
    fn pump_keeps_lines(
        depth: Depth,
        setup: &[u8],
        reads: &[&[u8]],
        (a, b): (i32, i32),
    ) -> Option<Vec<(String, Option<String>)>> {
        use baud_domain::{Col, PackedCell, Row};
        use baud_render::{Clip, Grain, Selection, Span};
        let text = |cells: &[PackedCell]| -> String {
            let s: String = cells
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or(' ')
                })
                .collect();
            s.trim_end().to_string()
        };
        let mut owned = Owned::with_history(&WS, &baud_core::CacheGeometry::UNKNOWN, depth);
        let t = &mut Term::new(owned.storage(), &WS);
        let lines: Vec<u8> = (0..40)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        feed(t, &lines);
        let mut scrolled = Scrolled::START.advanced(Rows::of_count(31));
        let before = Before::of(t);
        feed(t, setup);
        if let Ring::Retired(n) = live_moved(t, before) {
            scrolled = scrolled.advanced(n);
        }
        // Line `y` as a view's distance back and its row.
        let at = |y: i32| match usize::try_from(y) {
            Ok(y) => (Rows::ZERO, Row::from_index(y)),
            Err(_) => (
                Rows::of_count(usize::try_from(y.unsigned_abs()).expect("a saved line")),
                Row::ZERO,
            ),
        };
        let start = scrolled;
        let labels: Vec<String> = (a..=b)
            .map(|y| text(t.view(at(y).0).row(at(y).1)))
            .collect();
        let span = |t: &Term<'_>, y: i32| {
            let (k, r) = at(y);
            Span::cell(t.view(k).row(r), scrolled.line(k, r), Col::ZERO)
        };
        let mut sel = Selection::at(span(t, a), Grain::Cell);
        sel.extend(span(t, b));
        let (mut down, mut up) = (Rows::ZERO, Rows::ZERO);
        for bytes in reads {
            let before = Before::watched(t, Some(&sel), scrolled);
            feed(t, bytes);
            if stale_in_moved_rows(t) {
                return None;
            }
            scrolled = match numbered(t, before, None) {
                Ring::Retired(n) => scrolled.advanced(n),
                Ring::Restored(n) => scrolled.retreated(n)?,
            };
            // How far the read carried the selection's text against the
            // count, so each line it was made on is looked for where its
            // text went.
            match t.watched_lines().map(|w| w.moved) {
                Some(baud_core::TextMoved::Down(k)) => {
                    down = Rows::of_count(down.index() + k.index())
                }
                Some(baud_core::TextMoved::Up(k)) => up = Rows::of_count(up.index() + k.index()),
                None => {}
            }
            let mut kept = Some(sel);
            super::followed(t, &mut kept, scrolled);
            sel = kept?;
            if switched(t, before) || super::reset_in(t, before) {
                return None;
            }
            if sel.clip(oldest(t, scrolled)) == Clip::Cleared {
                return None;
            }
        }
        let retained = Rows::of_count(t.scrollback().len());
        let now = |line| {
            let on = (0..10)
                .map(Row::from_index)
                .find(|&r| scrolled.line(Rows::ZERO, r) == line);
            if let Some(r) = on {
                return Some(text(t.row(r).expect("a row on the grid")));
            }
            (1..=t.looking_back(retained).index())
                .map(Rows::of_count)
                .find(|&k| scrolled.line(k, Row::ZERO) == line)
                .map(|k| text(t.view(k).row(Row::ZERO)))
        };
        Some(
            (a..=b)
                .zip(labels)
                .map(|(y, label)| {
                    let (k, r) = at(y);
                    let mut line = start.line(k, r);
                    for _ in 0..down.index() {
                        line = line.below();
                    }
                    for _ in 0..up.index() {
                        line = line.above().unwrap_or(line);
                    }
                    let kept = line >= oldest(t, scrolled) && sel.touches(line, line);
                    (label, kept.then(|| now(line)).flatten())
                })
                .collect(),
        )
    }

    /// What a test wants of a selection: dropped, or kept with every line
    /// it still reaches on its own text.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Want {
        Dropped,
        Kept,
    }

    /// The feeds of `progs` on which a selection on lines `a` to `b`, made
    /// after `setup` on a terminal of `depth` saved lines, is not what
    /// `want` says of it.
    fn wrong_feeds(
        depth: Depth,
        setup: &[u8],
        progs: &[&[u8]],
        sel: (i32, i32),
        want: Want,
    ) -> Vec<String> {
        let mut wrong = Vec::new();
        for prog in progs {
            for (name, reads) in every_feed(prog) {
                let got = pump_keeps_lines(depth, setup, &reads, sel);
                let right = match (&got, want) {
                    (None, Want::Dropped) => true,
                    (Some(lines), Want::Kept) => lines
                        .iter()
                        .all(|(label, now)| now.is_none() || now.as_ref() == Some(label)),
                    _ => false,
                };
                if !right {
                    let p = String::from_utf8_lossy(prog);
                    wrong.push(format!("{p:?} {name}: lines {sel:?} {got:?}"));
                }
            }
        }
        wrong
    }

    /// The feeds of `prog` on which a selection on lines `sel`, made after
    /// `setup`, does not end on exactly the lines `keep` of those it was
    /// made on, each on its own text: [`wrong_feeds`] for a selection a
    /// scroll trims.
    fn wrong_kept(setup: &[u8], prog: &[u8], sel: (i32, i32), keep: &[i32]) -> Vec<String> {
        let mut wrong = Vec::new();
        for (name, reads) in every_feed(prog) {
            let got = pump_keeps_lines(Depth::DEFAULT, setup, &reads, sel);
            let right = got.as_ref().is_some_and(|lines| {
                (sel.0..=sel.1)
                    .zip(lines)
                    .all(|(y, (label, now))| match keep.contains(&y) {
                        true => now.as_ref() == Some(label),
                        false => now.is_none(),
                    })
            });
            if !right {
                let p = String::from_utf8_lossy(prog);
                wrong.push(format!("{p:?} {name}: lines {sel:?} {got:?}"));
            }
        }
        wrong
    }

    /// **A scroll up and down in one read keeps the rows it brought back**,
    /// at every read size and split, on both screens. `ESC [ 2 S ESC [ 2 T`
    /// sends rows 0 and 1 off the top and takes them back blank, so a
    /// selection on either is dropped; rows 2 to 9 go up two and come back
    /// on their own text, so one there is kept. Until 2026-10-17 a feed
    /// split after `ESC [ 2 S` dropped those too, by the second read's two
    /// lines taken back, judged at its two ends. A selection from row 1 to
    /// row 9 is dropped on the primary, where row 1 waits in the history to
    /// be taken back blank; on the alternate screen, which keeps none, the
    /// scroll that takes row 1 off the top trims it there, as xterm
    /// truncates a selection a scroll moves out of the screen, and the rest
    /// is kept on its text at every feed. Until then it was trimmed only
    /// where a read ended with row 1 off the top, and dropped on the 4 of
    /// 15 feeds whose read took it back blank.
    #[test]
    fn a_scroll_up_and_down_in_one_read_keeps_the_rows_it_brought_back() {
        let prog: &[&[u8]] = &[b"\x1b[2S\x1b[2T"];
        let mut wrong = Vec::new();
        for screen in [Screen::Normal, Screen::Alternate] {
            let setup = labelled(screen);
            for y in 0..10 {
                let want = match y < 2 {
                    true => Want::Dropped,
                    false => Want::Kept,
                };
                wrong.extend(wrong_feeds(Depth::DEFAULT, &setup, prog, (y, y), want));
            }
            wrong.extend(wrong_feeds(
                Depth::DEFAULT,
                &setup,
                prog,
                (2, 9),
                Want::Kept,
            ));
            let want = match screen {
                Screen::Normal => Want::Dropped,
                Screen::Alternate => Want::Kept,
            };
            wrong.extend(wrong_feeds(Depth::DEFAULT, &setup, prog, (1, 9), want));
        }
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// **A scroll carries a selection with its text, at every read size and
    /// split, on both screens** -- the three cases open until 2026-10-20,
    /// settled as xterm does with `keepSelection` at its default. A scroll
    /// down keeps what it does not push under the screen (`SD 2` over rows 5
    /// to 9, `RI` at the top over rows 8 and 9); a region's scroll, `3;8`
    /// (rows 2 to 7), moves the rows inside it, keeps the part inside of one
    /// across its top or its bottom, and drops one it scrolls out whole;
    /// `IL` and `DL` there move the rows below the cursor, and drop one
    /// holding the cursor's row or a deleted one. Each was dropped until
    /// then.
    #[test]
    fn a_scroll_carries_a_selection_with_its_text_at_every_read_size() {
        let region: &[u8] = b"\x1b[3;8r";
        let mut wrong = Vec::new();
        for screen in [Screen::Normal, Screen::Alternate] {
            let plain = labelled(screen);
            let fenced = [plain.as_slice(), region].concat();
            for (setup, prog, sel, keep, what) in [
                (&plain, &b"\x1b[2T"[..], (5, 9), &[5, 6, 7][..], "SD 2"),
                (&plain, b"\x1b[1;1H\x1bM", (8, 9), &[8], "RI at the top"),
                (
                    &fenced,
                    b"\x1b[8;1H\n",
                    (4, 5),
                    &[4, 5],
                    "a region's line feed",
                ),
                (&fenced, b"\x1b[8;1H\n", (1, 4), &[3, 4], "across its top"),
                (
                    &fenced,
                    b"\x1b[8;1H\n",
                    (6, 9),
                    &[6, 7],
                    "across its bottom",
                ),
                (&fenced, b"\x1b[T", (4, 5), &[4, 5], "SD in the region"),
                (&fenced, b"\x1b[5;1H\x1b[2L", (5, 6), &[5], "IL 2 above it"),
                (&fenced, b"\x1b[4;1H\x1b[M", (5, 6), &[5, 6], "DL above it"),
            ] {
                for w in wrong_kept(setup, prog, sel, keep) {
                    wrong.push(format!("{screen:?} {what}: {w}"));
                }
            }
            for (setup, prog, sel, what) in [
                (&plain, &b"\x1b[2T"[..], (8, 9), "SD 2, all pushed under"),
                (&fenced, b"\x1b[8;1H\n", (2, 2), "its top row, out"),
                (&fenced, b"\x1b[2T", (6, 7), "SD 2, pushed out"),
                (&fenced, b"\x1b[6;1H\x1b[L", (5, 6), "IL in it"),
                (&fenced, b"\x1b[6;1H\x1b[M", (5, 6), "DL in it"),
            ] {
                for w in wrong_feeds(Depth::DEFAULT, setup, &[prog], sel, Want::Dropped) {
                    wrong.push(format!("{screen:?} {what}: {w}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// **A line taken back over a saved line drops a selection on it**, at
    /// every read size and split. A scroll down shows the newest saved line
    /// on the top row, blanked, and a line feed or `SU` sends it back, so a
    /// read can net no movement or lines retired while the saved line that
    /// held "line 30" holds nothing: a selection on it, or from it onto the
    /// screen, is dropped. One on the saved lines above it, which no line
    /// taken back reached, is kept on its own text at every feed -- until
    /// 2026-10-17 kept whole and dropped split after the scroll down, and a
    /// selection on "line 30" itself was kept on the blank, the watch armed
    /// on live rows alone.
    #[test]
    fn a_line_taken_back_over_a_saved_line_drops_a_selection_on_it() {
        let setup = labelled(Screen::Normal);
        let progs: &[&[u8]] = &[
            b"\x1b[T\x1b[10;1H\n",
            b"\x1b[2T\x1b[10;1H\n\n",
            b"\x1b[T\x1b[2S",
        ];
        let mut wrong = Vec::new();
        for (sel, want) in [
            ((-1, -1), Want::Dropped),
            ((-1, 1), Want::Dropped),
            ((-4, -2), Want::Kept),
            ((-3, -3), Want::Kept),
        ] {
            let progs = match sel {
                (-4, -2) => &progs[..1],
                _ => progs,
            };
            wrong.extend(wrong_feeds(Depth::DEFAULT, &setup, progs, sel, want));
        }
        // The control: the saved line held its text before the read.
        assert_eq!(
            pump_keeps_lines(Depth::DEFAULT, &setup, &[b"x"], (-1, -1)),
            Some(vec![("line 30".into(), Some("line 30".into()))])
        );
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// **A read past half the primary's ring numbers a selection by the
    /// lines it moved**, at every read size and split. With thirty saved
    /// lines the ring is forty rows, and the rotation read round it says
    /// twenty-one lines retired are nineteen taken back, and forty-one are
    /// one. Counted, twenty-one carry row 9 into the history, still on its
    /// own text, and a saved line three up further in, so both are kept;
    /// forty-one carry both past the oldest line the ring holds, so both are
    /// dropped. Until 2026-10-17 the twenty-one dropped them, by the watch's
    /// count of a read past half the ring, and the forty-one kept a saved
    /// line, which arms no watch, on the line one row from it.
    #[test]
    fn a_read_past_half_the_primary_ring_numbers_a_selection_by_its_lines() {
        let depth = Depth::of(30);
        let setup = labelled(Screen::Normal);
        let lf = |n: usize| [&b"\x1b[10;1H"[..], &b"\n".repeat(n)].concat();
        let (short, long) = (lf(21), lf(41));
        let mut wrong = Vec::new();
        for sel in [(9, 9), (-3, -3), (-3, 9)] {
            wrong.extend(wrong_feeds(depth, &setup, &[&short], sel, Want::Kept));
            wrong.extend(wrong_feeds(depth, &setup, &[&long], sel, Want::Dropped));
        }
        // The controls: the ring is forty rows, and twenty-one lines leave
        // row 9's text in the history, where the selection still names it.
        with_history(|t| assert!(t.ring().index() > 40, "the default ring is longer"));
        assert_eq!(
            pump_keeps_lines(depth, &setup, &[&short], (9, 9)),
            Some(vec![("P9".into(), Some("P9".into()))])
        );
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// **A read past half the alternate screen's ring and back keeps a
    /// selection it left alone**, at every read size and split. On its
    /// ten-row ring `ESC [ 6 S ESC [ 5 T` nets a line retired, row 8's text
    /// on row 7; six line feeds at the bottom and six `RI` at the top net
    /// none, rows 8 and 9 back on their text; `ESC [ 6 T ESC [ 6 S` nets
    /// none, row 1 back on its. No line the reads took back blanked those
    /// rows. Until 2026-10-17 the watch was reached by the distance alone,
    /// at the line that went past half the ring, and each was dropped.
    #[test]
    fn a_read_past_half_the_alternate_ring_and_back_keeps_a_selection_it_left_alone() {
        let setup = labelled(Screen::Alternate);
        let back: Vec<u8> = [
            &b"\x1b[10;1H"[..],
            &b"\n".repeat(6),
            b"\x1b[1;1H",
            &b"\x1bM".repeat(6),
        ]
        .concat();
        let mut wrong = Vec::new();
        for (prog, sel) in [
            (&b"\x1b[6S\x1b[5T"[..], (8, 8)),
            (&back[..], (8, 9)),
            (&b"\x1b[6T\x1b[6S"[..], (1, 1)),
        ] {
            wrong.extend(wrong_feeds(
                Depth::DEFAULT,
                &setup,
                &[prog],
                sel,
                Want::Kept,
            ));
        }
        // The control: a row the last of them pushed under the window.
        wrong.extend(wrong_feeds(
            Depth::DEFAULT,
            &setup,
            &[b"\x1b[6T\x1b[6S"],
            (9, 9),
            Want::Dropped,
        ));
        assert!(wrong.is_empty(), "{} wrong: {wrong:?}", wrong.len());
    }

    /// A read that leaves the alternate screen and enters it again ends on
    /// the screen it began on, so the screen read at its two ends cannot see
    /// the switch -- and the entry cleared the screen and reset its ring, so
    /// the selection a file manager left named rows of an editor's screen,
    /// and the live movement compared two incarnations of one ring. The
    /// hand-off from a file manager to an editor sends both in one read.
    #[test]
    fn leaving_and_entering_the_alternate_screen_in_one_read_is_a_switch() {
        with_history(|t| {
            feed(t, b"\x1b[?1049h\x1b[10;1Ha\r\nb\r\n");
            // The control: a read on one surface is not a switch.
            let before = Before::of(t);
            feed(t, b"c\r\n");
            assert!(!switched(t, before));
            assert_eq!(live_moved(t, before), Ring::Retired(Rows::of_count(1)));
            let before = Before::of(t);
            feed(t, b"\x1b[?1049l\x1b[?1049h\x1b[10;1Hd\r\n");
            assert!(switched(t, before), "the screen went and came back");
            assert_eq!(live_moved(t, before), Ring::Retired(Rows::ZERO));
        });
    }

    /// A selection on the alternate screen is clipped at that screen's top,
    /// which keeps nothing behind it -- not at the primary's oldest line,
    /// which is what the history count reports while the alternate screen
    /// is up, and which left a selection scrolled off the top alive to be
    /// copied as bare newlines.
    #[test]
    fn a_selection_on_the_alternate_screen_is_clipped_at_its_top() {
        with_history(|t| {
            let scrolled = Scrolled::START.advanced(Rows::of_count(40));
            // The control, on the primary: thirty-one lines behind the top.
            assert_eq!(oldest(t, scrolled), scrolled.top(Rows::of_count(31)));
            feed(t, b"\x1b[?1049h");
            assert_eq!(oldest(t, scrolled), scrolled.top(Rows::ZERO));
        });
    }

    /// A resize taken while the alternate screen is up moves the primary --
    /// a shorter screen retires the rows the cursor it saved needs -- and
    /// the window follows the primary's lines, where the live rotation, the
    /// alternate screen's, had not moved at all.
    #[test]
    fn a_resize_on_the_alternate_screen_anchors_by_the_primary() {
        with_history(|t| {
            feed(t, b"\x1b[?1049h");
            let ws = WinSize {
                ws_col: 20,
                ws_row: 8,
            };
            assert_eq!(window_resized(t, &ws, Rows::of_count(3)).index(), 5);
        });
    }

    /// **A change of width keeps a scrolled-back window on the line it was
    /// showing.** The window was anchored by how far the line at the
    /// primary window's top moved, and a reflow renumbers that line by the
    /// growth of every line above it -- including the lines above the one
    /// the window shows, which do not move it at all. So long lines *above*
    /// the viewed line put the window on another line; the control puts the
    /// same long lines *between* the viewed line and the live screen, where
    /// their growth is exactly how much further back the viewed line is.
    #[test]
    fn a_width_reflow_keeps_the_window_on_its_line() {
        let long = |i: usize| format!("long {i} {}\r\n", "x".repeat(22));
        let short = |i: usize| format!("s {i:02}\r\n");
        // (the stream, the viewed line, the arm's name)
        let above: String = (0..5).map(long).chain((0..30).map(short)).collect();
        let between: String = (0..10)
            .map(short)
            .chain((0..5).map(long))
            .chain((10..30).map(short))
            .collect();
        for (stream, viewed, arm) in [
            (
                &between,
                "s 05",
                "the control, long lines between it and the screen",
            ),
            (&above, "s 05", "long lines above the viewed line"),
        ] {
            for (from, to) in [(20u16, 12u16), (12, 20)] {
                let ws = |c: u16| WinSize {
                    ws_col: c,
                    ws_row: 10,
                };
                let mut owned = Owned::new(&ws(20));
                let mut t = Term::new(owned.storage(), &ws(from));
                feed(&mut t, stream.as_bytes());
                let text = |t: &Term<'_>, back: Rows| -> String {
                    t.view(back)
                        .row(baud_domain::Row::ZERO)
                        .iter()
                        .filter_map(|c| c.content.as_codepoint())
                        .filter_map(|cp| char::from_u32(cp.value()))
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                };
                let back = (1..=t.scrollback().len())
                    .map(Rows::of_count)
                    .find(|&b| text(&t, b) == viewed)
                    .expect("the viewed line is in the history");
                let back = window_resized(&mut t, &ws(to), back);
                assert_eq!(
                    text(&t, back),
                    viewed,
                    "{arm}, {from} to {to} columns: the window moved to another line"
                );
                // A live window stays live, whatever the re-cut did.
                assert_eq!(window_resized(&mut t, &ws(from), Rows::ZERO), Rows::ZERO);
            }
        }
    }

    /// **A full-screen program started from the prompt is not a prompt
    /// opening.** A contradicted prediction holds prediction until the
    /// shell's next prompt, and the hold is released when a prompt opens on
    /// the primary. Each screen has its own zone, so a program a line-editor
    /// widget opens with no `C` -- fzf full-screen, `edit-command-line` --
    /// turned the live zone to `Output` and back, which read as a prompt
    /// opening when it closed. The control is a real next prompt, which
    /// releases the hold.
    #[test]
    fn an_alternate_screen_round_trip_is_not_a_prompt_opening() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let before = t.prompts_opened();
        feed(&mut t, b"\x1b]133;P;k=i\x07$ ");
        let (mut hold, mut last) = hold_after(&t, Hold::UntilNextPrompt, before);
        assert!(hold == Hold::Open, "the control: a prompt opened");
        // A prediction the echo contradicted.
        hold = Hold::UntilNextPrompt;
        // The program's screen carries a mark of its own, which is the
        // program's and opens no prompt of the shell's.
        let program = b"\x1b[?1049h\x1b]133;A\x07\x1b[Hpicker";
        for read in [&program[..], b"\x1b[?1049l"] {
            (hold, last) = hold_after(&t, hold, last);
            feed(&mut t, read);
            (hold, last) = hold_after(&t, hold, last);
            assert!(
                hold == Hold::UntilNextPrompt,
                "{read:?}: the program's screen opened no prompt"
            );
        }
        // The command the user runs, then its next prompt.
        feed(&mut t, b"\r\n\x1b]133;C\x07");
        (hold, last) = hold_after(&t, hold, last);
        feed(&mut t, b"out\r\n\x1b]133;D\x07\x1b]133;P;k=i\x07$ ");
        (hold, _) = hold_after(&t, hold, last);
        assert!(hold == Hold::Open, "the next prompt released the hold");
    }

    /// **A command whose `C`, output and next prompt come in one read
    /// opens a prompt all the same.** The hold was released by the zone
    /// turning to `Prompt` between two reads, and a one-read command leaves
    /// it at `Prompt` on both sides, so prediction stayed held until a later
    /// prompt. At every split of the read in two the hold is released at its
    /// end; the control is the read with no prompt in it, which keeps it.
    #[test]
    fn a_command_in_one_read_releases_the_hold() {
        let prompt = b"\x1b]133;P;k=i\x07$ ";
        let read = b"\r\n\x1b]133;C\x07out\r\n\x1b]133;D\x07\x1b]133;P;k=i\x07$ ";
        let control = b"\r\n\x1b]133;C\x07out\r\n\x1b]133;D\x07";
        for (bytes, opens) in [(&read[..], true), (&control[..], false)] {
            for cut in 0..bytes.len() {
                let mut owned = Owned::new(&WS);
                let mut t = Term::new(owned.storage(), &WS);
                let (_, last) = hold_after(&t, Hold::Open, t.prompts_opened());
                feed(&mut t, prompt);
                let (_, mut last) = hold_after(&t, Hold::Open, last);
                // A prediction the echo contradicted, at this prompt.
                let mut hold = Hold::UntilNextPrompt;
                let (one, two) = bytes.split_at(cut);
                match cut == 0 {
                    // One read, the whole of it.
                    true => feed(&mut t, bytes),
                    // Two reads, the hold sampled after each.
                    false => {
                        feed(&mut t, one);
                        (hold, last) = hold_after(&t, hold, last);
                        feed(&mut t, two);
                    }
                }
                (hold, _) = hold_after(&t, hold, last);
                assert_eq!(
                    hold == Hold::Open,
                    opens,
                    "{bytes:?} cut at {cut}: a prompt opened or not"
                );
            }
        }
    }

    /// **The display-link tick reads the link preview's modifiers**, so a
    /// press or release of Command with the pointer resting shows on an
    /// idle screen even where no modifier change reaches the view, and a
    /// steady or hidden cursor draws no frame to read them in. The
    /// tick branch is AppKit-bound and no test drives it, so this holds
    /// the source: the pump's tick branch polls the hover after the focus
    /// (a focus lost clears it first), and the poll reads the modifiers
    /// held now, refreshes the hover and owes a frame when it changed.
    /// `link.rs`'s `a_press_of_command_alone_shows_on_the_next_frame` holds
    /// what the refresh does. Perturbations: delete the tick's call, or
    /// the frame it owes -- each fails here.
    #[test]
    fn the_display_link_tick_reads_the_link_preview() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart, so this test's own text is not what it finds.
        const PUMP: &str = concat!("    fn pump(&mut self, ", "trigger: Trigger) {");
        const TICK: &str = concat!("if let Trigger::", "Tick = trigger {");
        const FOCUS: &str = concat!("self.poll_", "focus();");
        const CALL: &str = concat!("self.poll_", "hover();");
        const POLL: &str = concat!("    fn poll_", "hover(&mut self) {");
        const READ: &str = concat!("    fn read_", "hover(&mut self) -> bool {");
        const READ_AS: &str = concat!(
            "    fn read_",
            "hover_as(&mut self, chord: crate::link::Chord) -> bool {"
        );
        let body = |head: &str| {
            assert_eq!(SRC.matches(head).count(), 1, "one {head}");
            let b = SRC.split(head).nth(1).expect(head);
            &b[..b.find("\n    }\n").expect("the function ends")]
        };
        let pump = body(PUMP);
        let tick = pump.split(TICK).nth(1).expect("the tick branch");
        let tick = &tick[..tick.find("\n        }\n").expect("the branch ends")];
        let (focus, call) = (tick.find(FOCUS), tick.find(CALL));
        assert!(call.is_some(), "the tick does not poll the hover");
        assert!(focus < call, "the hover is polled before the focus");
        assert_eq!(SRC.matches(CALL).count(), 1, "and nowhere else");
        let poll = body(POLL);
        assert!(
            poll.contains(concat!("self.app.read_", "hover()"))
                && poll.contains("self.app.pending = Pending::Dirty;"),
            "the poll owes no frame when the preview changes"
        );
        let read = body(READ);
        assert!(
            read.contains(concat!("NSEvent::modifier", "Flags_class()"))
                && read.contains(concat!("self.read_", "hover_as(chord)"))
                && body(READ_AS).contains(".refresh("),
            "the read does not refresh with the modifiers held now"
        );
    }

    /// **The `OSC 52` poll is the feed's per-read callback.** The pump is
    /// AppKit-bound and no test drives it, so this holds the source: the
    /// pump's one `reads::feed` call hands it `clips.poll`, which then runs
    /// after every read (`clip.rs` holds that `feed` does). Moved after the
    /// call, beside the title, the first of two reads that each finish a
    /// copy would be lost: the core keeps only the last write it finished.
    #[test]
    fn the_clipboard_is_polled_after_every_read() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart, so this test's own text is not what it finds.
        const PUMP: &str = concat!("    fn pump(&mut self, ", "trigger: Trigger) {");
        const FEED: &str = concat!("crate::reads::", "paced(");
        const POLL: &str = concat!("pasteboard(", "clips.poll(t, osc52));");
        assert_eq!(SRC.matches(PUMP).count(), 1, "one pump");
        let pump = SRC.split(PUMP).nth(1).expect("the pump");
        let pump = &pump[..pump.find("\n    }\n").expect("the pump ends")];
        assert_eq!(pump.matches(FEED).count(), 1, "the pump feeds once");
        let call = pump.split(FEED).nth(1).expect("the feed call");
        let call = &call[..call.find("\n        );").expect("the call ends")];
        assert!(
            call.contains(POLL),
            "the poll is not the feed's per-read callback"
        );
        assert_eq!(SRC.matches(POLL).count(), 1, "and nowhere else");
    }

    /// **The driver's calls into these are AppKit-bound**, so no test above
    /// reaches them: `present`, `select`, the link under a Cmd-click, the
    /// pump and the resize. This holds the source to them instead. **The app
    /// keeps no count of the window** (note V, V2): the core holds it, and a
    /// stored copy is the second numbering three recorded bugs came from, so
    /// no line names one. Every read asks the core for the window drawn
    /// (`Term::window_back`, zero on the alternate screen), and the
    /// primary's own, which the alternate screen does not clamp, is read
    /// only on the lines below, each on a path the alternate screen cannot
    /// reach -- the exact list, so a new raw reader fails here.
    #[test]
    fn every_reader_of_the_window_asks_the_clamp() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart so this test does not name them itself.
        let stored = [
            concat!("self.", "back"),
            concat!("app.", "back"),
            concat!("after", "_read("),
            concat!("resized", "_window("),
            concat!("resize", "_holding("),
        ];
        let names = |line: &str, tok: &str| {
            line.match_indices(tok).any(|(at, _)| {
                !line[at + tok.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
            })
        };
        let kept: Vec<&str> = SRC
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter(|l| stored.iter().any(|tok| names(l, tok)))
            .collect();
        assert!(kept.is_empty(), "a window the app keeps: {kept:?}");
        // The thumb's drag reads the primary's window in `bar::press`,
        // which both drivers call: a thumb is drawn only off a live
        // primary.
        const ALLOWED: [(&str, &str); 1] = [(
            "let was = term.primary_window_back();",
            "the wheel over the history (`scroll_history`): the alternate screen returned arrow keys before it",
        )];
        let raw_tokens = [
            concat!("self.app.term.", "primary_window_back("),
            concat!("self.term.", "primary_window_back("),
            concat!(" term.", "primary_window_back("),
        ];
        let mut raw: Vec<&str> = SRC
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && !l.starts_with('"') && !l.starts_with("(\""))
            .filter(|l| raw_tokens.iter().any(|tok| l.contains(tok)))
            .collect();
        raw.sort_unstable();
        let mut allowed: Vec<&str> = ALLOWED.iter().map(|(l, _)| *l).collect();
        allowed.sort_unstable();
        assert_eq!(
            raw, allowed,
            "a raw read of the primary's window that does not ask the clamp"
        );
        // And the CPU branch paints the window, where the live-screen form
        // names no window for the list above to see.
        assert!(
            !SRC.contains(concat!(".render(", "&mut self.term")),
            "the CPU path paints the live screen whatever the window"
        );
    }

    #[test]
    fn a_key_on_the_alternate_screen_leaves_the_primarys_window() {
        with_history(|t| {
            let back = Rows::of_count(3);
            t.set_window_back(back);
            assert!(snap_live(t), "on the primary, a key snaps");
            assert_eq!(t.window_back().index(), 0);
            t.set_window_back(back);
            feed(t, b"\x1b[?1049h");
            assert!(!snap_live(t));
            assert_eq!(
                t.primary_window_back().index(),
                3,
                "on the alternate screen it resets the live screen's view, \
                 which is already at the bottom"
            );
        });
    }

    /// **An erased history puts a window over it on the live screen, however
    /// the read falls.** `CSI 3 J` and `RIS` empty the saved lines and scroll
    /// nothing, so the anchor read no movement: an erase alone in a read
    /// clamped the window to an empty history and landed live, and an erase
    /// with lines after it in the same read clamped it to the lines that
    /// refilled the history -- the oldest of them, pinned there. The same
    /// bytes landed in two places by where the read ended.
    #[test]
    fn an_erased_history_puts_the_window_on_the_live_screen_however_the_read_falls() {
        let lines = |n: usize| -> Vec<u8> {
            (0..n)
                .flat_map(|i| format!("n {i}\r\n").into_bytes())
                .collect()
        };
        let mut reset_then_lines = b"\x1bc".to_vec();
        reset_then_lines.extend(lines(15));
        let mut erase_then_lines = b"\x1b[3J".to_vec();
        erase_then_lines.extend(lines(3));
        let mut lines_then_erase = lines(2);
        lines_then_erase.extend_from_slice(b"\x1b[3J");
        for (bytes, what) in [
            (b"\x1b[3J".to_vec(), "the erase alone"),
            (
                erase_then_lines,
                "the erase, then lines that refill the history",
            ),
            (lines_then_erase, "lines, then the erase"),
            (b"\x1bc".to_vec(), "RIS alone"),
            (reset_then_lines, "RIS, then lines that refill the history"),
        ] {
            with_history(|t| {
                assert_eq!(
                    window_across(t, Rows::of_count(3), &bytes),
                    Rows::ZERO,
                    "{what}: the window is not on the live screen"
                );
            });
        }
        // The control: a read with no erase carries the window up the
        // history with the lines it shows.
        with_history(|t| {
            assert_eq!(window_across(t, Rows::of_count(3), &lines(2)).index(), 5);
        });
    }

    /// **From the alternate screen it is the primary's window that goes
    /// live**, the one a key keeps while a program has the screen: the
    /// erase emptied the primary's history under it.
    #[test]
    fn an_erase_on_the_alternate_screen_puts_the_primarys_window_live() {
        with_history(|t| {
            let mut back = Rows::of_count(3);
            t.set_window_back(back);
            let read = |t: &mut Term<'_>, bytes: &[u8], back: &mut Rows| {
                feed(t, bytes);
                *back = t.primary_window_back();
            };
            read(t, b"\x1b[?1049h\x1b[10;1H", &mut back);
            assert_eq!(back.index(), 3, "the control: entering it moved nothing");
            read(t, b"\x1b[3J", &mut back);
            assert_eq!(back, Rows::ZERO, "the primary's history went from under it");
            read(t, b"\x1b[?1049l", &mut back);
            assert_eq!(back, Rows::ZERO, "and leaving finds the live screen");
        });
    }

    /// **A selection in the erased history goes, and one reaching into it
    /// from the screen is clipped to the screen's top**, by the clip the
    /// pump applies after every read: the oldest line a selection may name
    /// follows the history, and the lines that refill it are numbered after
    /// the erased ones, so none of them can be mistaken for an erased line.
    /// A selection on the screen stays, because the screen does.
    #[test]
    fn a_selection_in_the_erased_history_goes_and_one_reaching_into_it_is_clipped() {
        use baud_domain::{Col, Row};
        use baud_render::{Clip, Grain, Selection, Span};
        for (bytes, refilled) in [(&b"\x1b[3J"[..], 0), (b"\x1b[3Ja\r\nb\r\n", 2)] {
            with_history(|t| {
                // The forty lines before this read: thirty-one behind the
                // screen, numbered 0 to 30, and the screen's top at 31.
                let mut scrolled = Scrolled::START.advanced(Rows::of_count(31));
                // Line `n` counted from the first of them, which is a view
                // `31 - n` lines up from the screen's top -- negative past it.
                let span = |t: &Term<'_>, line: usize| {
                    let at = match line < 31 {
                        true => scrolled.top(Rows::of_count(31 - line)),
                        false => scrolled.line(Rows::ZERO, Row::from_index(line - 31)),
                    };
                    Span::cell(t.row(Row::ZERO).expect("a row on the grid"), at, Col::ZERO)
                };
                let make = |t: &Term<'_>, from: usize, to: usize| {
                    let mut s = Selection::at(span(t, from), Grain::Cell);
                    s.extend(span(t, to));
                    s
                };
                let (mut gone, mut reaching, mut shown) =
                    (make(t, 5, 10), make(t, 25, 35), make(t, 33, 35));
                let before = Before::of(t);
                feed(t, bytes);
                if let Ring::Retired(n) = live_moved(t, before) {
                    scrolled = scrolled.advanced(n);
                }
                let at = oldest(t, scrolled);
                assert_eq!(
                    at,
                    Scrolled::START.advanced(Rows::of_count(31)).top(Rows::ZERO),
                    "{refilled} refilled: the oldest line is the screen's top before the read"
                );
                assert!(
                    gone.clip(at) == Clip::Cleared,
                    "{refilled} refilled: in the history"
                );
                assert!(reaching.clip(at) == Clip::Kept, "{refilled} refilled");
                assert_eq!(reaching.ordered().0.line, at, "clipped to the screen's top");
                let whole = shown.ordered();
                assert!(
                    shown.clip(at) == Clip::Kept,
                    "{refilled} refilled: on the screen"
                );
                assert_eq!(shown.ordered(), whole, "and untouched");
            });
        }
    }

    /// **The pump settles a slide when the history was erased.** A slide
    /// draws the newest saved line above the screen as it pans, and after
    /// an erase there is none: the clamp makes it the screen's own first
    /// row, drawn twice for the rest of the slide. The pump is AppKit-bound,
    /// so the source is held to it, and `erased` to what moves it.
    #[test]
    fn an_erase_settles_the_slide() {
        const SRC: &str = include_str!("app.rs");
        assert!(
            SRC.contains(concat!(
                "if erased(&self.app.term, before) {\n",
                "            self.app.slide.snap();"
            )),
            "the pump settles the slide on an erase"
        );
        with_history(|t| {
            for (bytes, want) in [
                (&b"\x1b[2J"[..], false),
                (b"a\r\n", false),
                (b"\x1b[?3J", false),
                (b"\x1b[3J", true),
                (b"\x1bc", true),
            ] {
                let before = Before::of(t);
                feed(t, bytes);
                assert_eq!(erased(t, before), want, "{bytes:?}");
            }
        });
    }
}

#[cfg(test)]
mod button_tests {
    use super::{Pressed, ViewButton, mouse_button};

    /// **AppKit's buttons past the three are X's 8 to 11**, as XQuartz
    /// gives "additional mouse buttons (4 and higher in AppKit) to buttons
    /// 8 and higher in X11": `buttonNumber` 3 to 6, and nothing past them.
    #[test]
    fn appkits_buttons_past_the_three_are_xs_8_to_11() {
        let got = [3, 4, 5, 6, 7].map(|n| mouse_button(ViewButton::Other(n)));
        assert_eq!(
            got,
            [
                Pressed::Button8,
                Pressed::Button9,
                Pressed::Button10,
                Pressed::Button11,
                Pressed::Other
            ]
        );
        assert_eq!(mouse_button(ViewButton::Middle), Pressed::Middle);
    }
}

#[cfg(test)]
mod locator_tests {
    use super::tell_locator;
    use super::{LineFrame, LocatorHalf, LocatorOwed, Reply, To, ViewButton, ViewMouseKind};
    use baud_core::{LocatorDevice, Owned, Term};
    use baud_domain::{
        BytePos, Col, LocatorAt, LocatorButton, LocatorButtons, ReadLen, Row, WinSize,
    };

    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 6,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    fn cell(col: Col, row: Row) -> Option<LocatorAt> {
        Some(LocatorAt { col, row, px: None })
    }

    /// The indicator line's text.
    fn line(t: &Term<'_>) -> String {
        let cells = t.status_line().map_or(&[][..], |l| l.cells);
        cells
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or(' ')
            })
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    const NOTHING: LocatorOwed = LocatorOwed {
        reply: Reply::None,
        frame: LineFrame::None,
    };
    const FRAME: LocatorOwed = LocatorOwed {
        reply: Reply::None,
        frame: LineFrame::Owed,
    };
    const BOTH: LocatorOwed = LocatorOwed {
        reply: Reply::Queued,
        frame: LineFrame::Owed,
    };

    /// **The macOS driver's wiring of DEC's locator**: a move, a drag or a
    /// crossing is told in the unrouted half and a press or a release only
    /// in the routed one, so neither is told twice; a press the route gave
    /// the child reports under DECSLE and one it gave the selection is a
    /// move; a leave is the position undefined wherever it was, so the
    /// indicator line's field goes; and the line owes a frame whenever it
    /// is shown and the locator was told, a report the reply buffer.
    #[test]
    fn the_driver_tells_the_locator_each_half_once() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_locator_device(LocatorDevice::Present);
        let none = LocatorButtons::NONE;
        let left = none.pressed(LocatorButton::Left);
        let (moved, down) = (
            (ViewMouseKind::Moved, ViewButton::Left),
            (ViewMouseKind::Down, ViewButton::Left),
        );
        let un = LocatorHalf::Unrouted;
        // No line: no frame, and nothing reported before DECELR.
        assert_eq!(
            tell_locator(
                &mut t,
                moved,
                un,
                cell(Col::at_index(2), Row::from_index(4)),
                none
            ),
            NOTHING
        );
        feed(&mut t, b"\x1b[1$~\x1b[1;2'z\x1b[1'{");
        assert_eq!(
            tell_locator(
                &mut t,
                moved,
                un,
                cell(Col::at_index(2), Row::from_index(4)),
                none
            ),
            FRAME
        );
        assert!(line(&t).ends_with("[5,3]"), "{:?}", line(&t));
        assert_eq!(
            tell_locator(
                &mut t,
                down,
                un,
                cell(Col::at_index(2), Row::from_index(4)),
                left
            ),
            NOTHING
        );
        assert_eq!(t.reply(), b"", "a press is not told before the route");
        let drag = (ViewMouseKind::Dragged, ViewButton::Left);
        let child = LocatorHalf::Routed(To::Child);
        assert_eq!(
            tell_locator(
                &mut t,
                drag,
                child,
                cell(Col::at_index(3), Row::from_index(4)),
                left
            ),
            NOTHING
        );
        assert!(line(&t).ends_with("[5,3]"), "a drag is told unrouted");
        assert_eq!(
            tell_locator(
                &mut t,
                down,
                child,
                cell(Col::at_index(2), Row::from_index(4)),
                left
            ),
            BOTH
        );
        assert_eq!(t.reply(), b"\x1b[2;4;5;3&w");
        t.clear_reply();
        let select = LocatorHalf::Routed(To::Select);
        assert_eq!(
            tell_locator(
                &mut t,
                down,
                select,
                cell(Col::at_index(6), Row::from_index(1)),
                left
            ),
            FRAME
        );
        assert_eq!(t.reply(), b"", "the selection's press reports nothing");
        assert!(line(&t).ends_with("[2,7]"), "{:?}", line(&t));
        let exited = (ViewMouseKind::Exited, ViewButton::Left);
        assert_eq!(
            tell_locator(
                &mut t,
                exited,
                un,
                cell(Col::at_index(6), Row::from_index(1)),
                none
            ),
            FRAME
        );
        assert!(!line(&t).contains('['), "{:?}", line(&t));
    }

    /// **A ReGIS cursor the pointer moves owes a frame** with no line
    /// shown: multiple mode's crosshair follows the pointer inside the
    /// envelope, drawn into the page, and the move that redraws it is told
    /// unrouted.
    #[test]
    fn a_regis_cursor_the_pointer_moves_owes_a_frame() {
        let mut owned = Owned::new(&WS).with_images(baud_domain::ImageBudget::of(4 << 20));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_locator_device(LocatorDevice::Present);
        feed(&mut t, b"\x1bPpR(I1)");
        let at = Some(LocatorAt {
            col: Col::ZERO,
            row: Row::ZERO,
            px: Some(baud_domain::PointerPx {
                x: baud_domain::ImagePx::of(40),
                y: baud_domain::ImagePx::of(30),
            }),
        });
        let moved = (ViewMouseKind::Moved, ViewButton::Left);
        let none = LocatorButtons::NONE;
        assert_eq!(
            tell_locator(&mut t, moved, LocatorHalf::Unrouted, at, none),
            FRAME
        );
    }
}

#[cfg(test)]
mod drag_tests {
    use super::{Builds, builds};

    /// The body of the function whose signature line is `head`, to the
    /// first line that closes at `indent`'s depth.
    fn body<'a>(src: &'a str, head: &str, indent: &str) -> &'a str {
        let close = format!("\n{indent}}}\n");
        src.split_once(head)
            .and_then(|(_, rest)| rest.split_once(close.as_str()))
            .map(|(body, _)| body)
            .unwrap_or_else(|| panic!("{head}"))
    }

    /// **The launch, in the order it runs**: `run` up to the callback it
    /// registers for the end of the launch, with the shell's start and the
    /// window's build where `run` calls them; then the callback, and the
    /// window's `show`, which every window, the launch's and Command-N's,
    /// is shown by.
    struct Launch<'a> {
        before: String,
        callback: &'a str,
        after: &'a str,
        show: &'a str,
    }

    fn launch(src: &str) -> Launch<'_> {
        let run = body(src, concat!("\npub fn ", "run() {"), "");
        let (head, launched) = run
            .split_once(concat!("launch::when_", "launched(move || {"))
            .expect("shown once launched");
        let (callback, after) = launched
            .split_once("\n    });\n")
            .expect("the callback ends");
        let start = body(
            src,
            concat!("fn start(scale: CGFloat, size: NSSize, ", "inset: CGFloat,"),
            "    ",
        );
        let build = body(
            src,
            concat!("    fn new(\n        mtm: ", "MainThreadMarker,"),
            "    ",
        );
        let (to_shell, rest) = head
            .split_once(concat!("Shell::", "start("))
            .expect("run starts the shell");
        let (to_build, rest) = rest
            .split_once(concat!("Opening::", "new("))
            .expect("run builds the window");
        let show = body(
            src,
            concat!("fn show(self, mtm: MainThreadMarker, ", "scale: CGFloat,"),
            "    ",
        );
        Launch {
            before: format!("{to_shell}{start}{to_build}{build}{rest}"),
            callback,
            after,
            show,
        }
    }
    use baud_present_macos::InFlight;
    use baud_render::Tally;

    /// The sizes a shell was dragged through, as recorded.
    const STEPS: &str = include_str!("../../tests/resize/drag.steps");

    /// A grid size, columns by rows.
    type Size = (u16, u16);

    /// A moment of the drag, counted in its steps.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Step(usize);

    /// A set of surfaces, replaced whenever the size moves.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Set(usize);

    /// A frame the GPU is drawing: when it lands, its size and the
    /// surfaces it was drawn for.
    struct Flying {
        lands: Step,
        size: Size,
        set: Set,
    }

    /// The surfaces the presenter holds: their size and their set.
    struct Surfaces {
        size: Size,
        set: Set,
    }

    /// Where the model's `present` asks the gate: after the surfaces are
    /// replaced for the window's size, as `App::present` asks it, or
    /// before, the control.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Order {
        SurfacesFirst,
        GateFirst,
    }

    /// What a drag left: the frames built while one was in flight, the
    /// frames built, the size of the last frame shown, and the frames
    /// shown at a size the window had left.
    #[derive(Debug, Default)]
    struct Drag {
        over_live: Tally,
        built: Tally,
        shown: Option<Size>,
        stale: Tally,
    }

    /// The window and the frames between the driver and the GPU.
    struct Loop {
        order: Order,
        land: Step,
        window: Size,
        surfaces: Surfaces,
        flying: Option<Flying>,
        d: Drag,
    }

    impl Loop {
        /// The surfaces replaced when the window's size moved: a new set,
        /// which supersedes every frame committed against the old one.
        fn replace(&mut self) {
            if self.surfaces.size != self.window {
                self.surfaces = Surfaces {
                    size: self.window,
                    set: Set(self.surfaces.set.0 + 1),
                };
            }
        }

        /// `present` for the model: owed, built and refused, or committed.
        fn present(&mut self, now: Step) -> Builds {
            if self.order == Order::SurfacesFirst {
                self.replace();
            }
            let flight = match self.flying {
                Some(_) => InFlight::Busy,
                None => InFlight::Idle,
            };
            if builds(Some(flight)) == Builds::Owed {
                return Builds::Owed;
            }
            if self.order == Order::GateFirst {
                self.replace();
            }
            self.d.built.bump();
            if flight == InFlight::Busy {
                // Built, and refused by `draw_cells`: still owed.
                self.d.over_live.bump();
                return Builds::Owed;
            }
            self.flying = Some(Flying {
                lands: Step(now.0 + self.land.0),
                size: self.surfaces.size,
                set: self.surfaces.set,
            });
            Builds::Now
        }

        /// The frame in flight lands: shown if its set is still the
        /// presenter's (`journal::admits`), and stale if the window has
        /// left its size.
        fn landed(&mut self) {
            let Some(f) = self.flying.take() else { return };
            if f.set == self.surfaces.set {
                self.d.shown = Some(f.size);
                if f.size != self.window {
                    self.d.stale.bump();
                }
            }
        }
    }

    /// **A drag replayed against a GPU that takes `land` steps to land a
    /// frame**, as the driver runs it: every step moves the window and
    /// asks `present` for a frame (`relayout`); a landing wakes the pump,
    /// and every `tick` steps the display link does, and either asks
    /// `present` for the owed frame; under the old rule the model's
    /// `present` builds it even while one is in flight, which `pump`'s own
    /// gate never lets the app do.
    fn drag(sizes: &[Size], land: Step, tick: Step, order: Order) -> Drag {
        let mut l = Loop {
            order,
            land,
            window: sizes[0],
            surfaces: Surfaces {
                size: sizes[0],
                set: Set(0),
            },
            flying: None,
            d: Drag::default(),
        };
        let mut owed = Builds::Now;
        let mut now = Step(0);
        loop {
            if l.flying.as_ref().is_some_and(|f| f.lands == now) {
                l.landed();
                if owed == Builds::Owed {
                    owed = l.present(now);
                }
            }
            if let Some(&size) = sizes.get(now.0) {
                l.window = size;
                owed = l.present(now);
            } else if owed == Builds::Now && l.flying.is_none() {
                return l.d;
            }
            if now.0.is_multiple_of(tick.0) && owed == Builds::Owed {
                owed = l.present(now);
            }
            now = Step(now.0 + 1);
            assert!(
                now.0 < sizes.len() * 4 + 64,
                "the drag never settled: {:?}",
                l.d
            );
        }
    }

    fn sizes() -> Vec<Size> {
        STEPS
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(|l| {
                let (size, _) = l.split_once(' ').expect("COLSxROWS <len>");
                let (c, r) = size.split_once('x').expect("COLSxROWS");
                (c.parse().expect("columns"), r.parse().expect("rows"))
            })
            .collect()
    }

    /// **A drag step that finds a frame in flight builds nothing**, and
    /// the frame it owes is built when the slot frees: the last frame
    /// shown is the size the drag stopped at, wherever in the recording it
    /// stops, and no frame is shown at a size the window has left. Over a
    /// GPU that lands a frame in one step, three and seven,
    /// and a display link that ticks every step, every second and every
    /// fifth.
    #[test]
    fn a_drag_step_over_a_frame_in_flight_is_owed_not_built() {
        let sizes = sizes();
        assert!(sizes.len() > 300, "the recording: {}", sizes.len());
        for end in 1..=sizes.len() {
            let steps = &sizes[..end];
            let last = steps[end - 1];
            for land in [1, 3, 7] {
                for tick in [1, 2, 5] {
                    let d = drag(steps, Step(land), Step(tick), Order::SurfacesFirst);
                    let at = format!("{end} steps, land {land}, tick {tick}: {d:?}");
                    assert_eq!(d.over_live, Tally::default(), "{at}");
                    assert_eq!(d.shown, Some(last), "{at}");
                    assert_eq!(d.stale, Tally::default(), "{at}");
                    let built = d.built.value();
                    assert!(built > 0 && built <= Tally::of(end).value(), "{at}");
                }
            }
        }
    }

    /// **The control: the gate asked before the surfaces are replaced
    /// shows stale frames.** A step that finds a frame in flight leaves
    /// the old surfaces up, so that frame lands current at a size the
    /// window has left -- the order the pin below forbids.
    #[test]
    fn a_gate_before_the_surfaces_shows_a_size_the_window_left() {
        let d = drag(&sizes(), Step(3), Step(1), Order::GateFirst);
        assert!(d.stale.value() > 0, "{d:?}");
    }

    /// The gate itself: a frame in flight owes the next, an idle GPU and
    /// the CPU path, which has no presenter in flight, build it now.
    #[test]
    fn a_frame_is_owed_only_while_one_is_in_flight() {
        assert_eq!(builds(Some(InFlight::Busy)), Builds::Owed);
        assert_eq!(builds(Some(InFlight::Idle)), Builds::Now);
        assert_eq!(builds(None), Builds::Now);
    }

    /// **`present` asks the gate after the surfaces are replaced and
    /// before anything is built.** After: a step that replaces them
    /// supersedes the frame in flight, which was drawn for the size the
    /// window left, so it is never shown. Before: the styles synced, the
    /// glyph map walked, the window copied, the overlay walked or the
    /// instances converted, all of which `draw_cells` would throw away.
    /// And the frame stays owed: the branch leaves `pending` as it is.
    /// `present` is AppKit-bound, so the source is what can fail.
    #[test]
    fn present_asks_the_gate_between_the_surfaces_and_the_build() {
        const SRC: &str = include_str!("app.rs");
        let body = SRC
            .split_once(concat!(
                "fn present(&mut self, screen: &Drawable",
                "View, show: Show) {"
            ))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| body)
            .expect("present's body");
        let at = |tok: &str| body.find(tok).unwrap_or_else(|| panic!("{tok} in present"));
        let gate = at(concat!(
            "if builds(self.presenter.as_ref().map(Presenter::",
            "in_flight))"
        ));
        assert!(at("p.resize(sw, sh)") < gate, "the surfaces first");
        for built in [
            "self.renderer.set_blink(",
            "Presenter::building",
            "self.renderer.sync_styles(",
            ".convert(&self.term",
            ".render_at(",
        ] {
            assert!(gate < at(built), "{built} after the gate");
        }
        let branch = body[gate..]
            .split_once('}')
            .map(|(b, _)| b)
            .expect("the branch");
        assert!(branch.contains("return;"), "{branch}");
        assert!(
            !branch.contains("pending"),
            "the frame stays owed: {branch}"
        );
    }

    /// **Every frame that reaches the layer shows the window to the
    /// splash**: the first frame, drawn now on either GPU arm, every CPU
    /// frame, and a GPU frame the completion hands over. The first two set
    /// the layer's surface themselves and no completion follows them, so a
    /// mark only in `presented` leaves the splash waiting for good on the
    /// CPU path. A GPU frame drawn on an event is only committed when
    /// `present` returns, so it is not marked there. `present` is
    /// AppKit-bound, so the source is what can fail.
    #[test]
    fn every_frame_on_the_layer_shows_the_window_to_the_splash() {
        const SRC: &str = include_str!("app.rs");
        let body = SRC
            .split_once(concat!(
                "fn present(&mut self, screen: &Drawable",
                "View, show: Show) {"
            ))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| body)
            .expect("present's body");
        let mark = concat!("self.layer_", "shown = Done::Yes;");
        let calls = [
            "p.draw_cells(",
            "p.draw_cells_now(",
            "p.draw(",
            "p.draw_now(",
            "p.present_changed(",
        ];
        // From a call to the next, or to the end of `present`.
        let after = |call: &str| {
            let at = body
                .find(call)
                .unwrap_or_else(|| panic!("{call} in present"));
            let rest = &body[at + call.len()..];
            let end = calls
                .iter()
                .filter_map(|c| rest.find(c))
                .min()
                .unwrap_or(rest.len());
            &rest[..end]
        };
        for (call, shown) in [
            ("p.draw_cells_now(", "if show == Show::Now {"),
            ("p.draw_now(", "if show == Show::Now {"),
            ("p.present_changed(", "== Presented::Yes {"),
        ] {
            let arm = after(call);
            let at = arm
                .find(shown)
                .unwrap_or_else(|| panic!("the control: {shown} after {call}: {arm}"));
            assert!(
                arm[at..].contains(mark),
                "{call} sets the layer's surface and leaves the splash waiting: {arm}"
            );
        }
        let presented = SRC
            .split_once(concat!("fn presented(&mut self) {\n        ", "self.app."))
            .map(|(_, rest)| rest)
            .expect("the completion");
        assert!(
            presented.starts_with("layer_shown = Done::Yes;"),
            "the completion leaves the splash waiting"
        );
    }

    /// **The window is ordered front with no open animation**, so the
    /// gate's "on screen" is the window whole on screen and the launch
    /// logo never plays inside AppKit's animation of it: the launch sets
    /// its `animationBehavior` to none once, before it shows the window,
    /// and nothing else in the app sets it. AppKit posts nothing at the
    /// animation's end, so no later signal could stand in for it. The
    /// launch is AppKit-bound, so the source is what can fail.
    #[test]
    fn the_window_opens_whole_before_the_logo_starts() {
        const SRC: &str = include_str!("app.rs");
        let launch = launch(SRC).show;
        let set = concat!(
            ".setAnimation",
            "Behavior(NSWindowAnimationBehavior::None);"
        );
        let show = concat!("window.", "show();");
        let at = launch.find(set).expect("the open animation is turned off");
        let shown = launch.find(show).expect("the control: the window is shown");
        assert!(at < shown, "turned off after the window is shown");
        assert_eq!(launch.matches(show).count(), 1, "shown once");
        assert_eq!(
            SRC.matches(concat!(".setAnimation", "Behavior(")).count(),
            1,
            "set once"
        );
    }

    /// **The tick starts the splash only once the window is shown**: a
    /// frame on the layer and AppKit's window on screen and not wholly
    /// covered. The tick is AppKit-bound, so the source is what can fail.
    #[test]
    fn the_tick_starts_the_splash_only_once_the_window_is_shown() {
        const SRC: &str = include_str!("app.rs");
        let frame = SRC
            .split_once(concat!("fn frame(&mut self, ", "target: f64) {"))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| body)
            .expect("the tick");
        let start = concat!("self.app.splash.", "shown()");
        assert_eq!(frame.matches(start).count(), 1, "one start: {frame}");
        let (before, _) = frame.split_once(start).expect("the start");
        let gate = before
            .rsplit_once(concat!("if self.app.splash.", "waiting()"))
            .map(|(_, gate)| gate)
            .expect("the start is gated");
        for read in [
            "self.app.layer_shown == Done::Yes",
            "w.isVisible()",
            "w.occlusionState().contains(NSWindowOcclusionState::Visible)",
        ] {
            assert!(gate.contains(read), "{read} in the gate: {gate}");
        }
    }

    /// **The plasma's pipeline is built off the first frame's path**: not
    /// on the GPU's own thread, which the launch joins before the window's
    /// first frame, nor anywhere before that frame is drawn and the window
    /// shown, but on a thread of its own started once it is (`show`), and
    /// taken by the tick only once that thread has ended, never waited
    /// for -- the logo's start waits for it, and the gate reads it. Never
    /// built on a tick, so the logo's first frame waits on no build. The
    /// launch is AppKit-bound, so the source is what can fail.
    #[test]
    fn the_plasma_is_built_off_the_first_frames_path() {
        const SRC: &str = include_str!("app.rs");
        let probe = SRC
            .split_once(concat!("let probe = std::thread::", "spawn(move || {"))
            .and_then(|(_, rest)| rest.split_once("\n        });\n"))
            .map(|(body, _)| body)
            .expect("the GPU's thread");
        assert!(
            probe.contains("gpu pipelines:"),
            "the control: the five pipelines are built there"
        );
        for build in ["Effect::Plasma", "warm_effect(", "effect_build("] {
            assert!(!probe.contains(build), "{build} on the GPU's thread");
        }
        let launch = launch(SRC);
        let started = concat!("driver.app.", "build_plasma();");
        assert!(!launch.before.contains(started), "built before the window");
        let show = launch.show;
        let drawn = show
            .find(concat!("driver.app.present(&v, ", "Show::Now);"))
            .expect("the first frame is drawn there");
        let shown = show
            .find(concat!("window.", "show();"))
            .expect("shown there");
        let at = show.find(started).expect("the plasma is started in show");
        assert!(
            drawn < shown && shown < at,
            "after the first frame and the window"
        );
        let ready = body(
            SRC,
            concat!("fn plasma_", "ready(&mut self) -> bool {"),
            "    ",
        );
        let polled = ready.find(".is_finished()").expect("polled");
        let joined = ready.find(".join()").expect("joined once ended");
        assert!(polled < joined, "joined only once its thread has ended");
        let frame = body(
            SRC,
            concat!("fn frame(&mut self, ", "target: f64) {"),
            "    ",
        );
        let gate = frame
            .split_once(concat!("let plasma = self.app.", "plasma_ready();"))
            .map(|(_, after)| after)
            .expect("the tick reads it");
        assert!(
            gate.trim_start().starts_with(
                "if self.app.splash.waiting() && self.app.layer_shown == Done::Yes && plasma {"
            ),
            "the logo's start waits for it: {gate}"
        );
        for build in ["effect_build(", ".build()", "warm_effect("] {
            assert!(!frame.contains(build), "built on a tick: {build}");
        }
    }

    /// **The window is shown once the app has finished launching, its
    /// first frame already on its layer**: the launch registers for the end
    /// of `finishLaunching` before it runs the app, and there the first
    /// frame is drawn before the window is ordered front, which the launch
    /// does nowhere else. Shown before `run()`, the window stood on screen
    /// through the rest of the launch with no frame and then no logo. The
    /// launch is AppKit-bound, so the source is what can fail.
    #[test]
    fn the_window_is_shown_once_launched_with_its_first_frame() {
        const SRC: &str = include_str!("app.rs");
        let launch = launch(SRC);
        let show = concat!("window.", "show();");
        assert!(
            !launch.before.contains(show),
            "shown before the app finished launching"
        );
        assert!(
            launch.after.contains("ns_app.run();"),
            "registered before the app runs"
        );
        assert!(
            launch
                .callback
                .contains(concat!("opening.", "show(mtm, scale, None);")),
            "the launch's window is shown there: {}",
            launch.callback
        );
        let inside = launch.show;
        let drawn = inside
            .find(concat!("driver.app.present(&v, ", "Show::Now);"))
            .expect("the first frame is drawn there");
        let shown = inside.find(show).expect("the window is shown there");
        assert!(drawn < shown, "shown before its first frame");
        let installed = inside
            .find(concat!("view.set_", "responder("))
            .expect("the responder is installed there");
        assert!(shown < installed, "the responder last");
    }

    /// **Automatic tabbing is off before the first window is made**, so a
    /// Command-N window is never merged into a tab group (`windows.rs`):
    /// the class setting comes before the launch's
    /// window is built, and nothing turns it back on.
    #[test]
    fn automatic_tabbing_is_off_before_the_first_window() {
        const SRC: &str = include_str!("app.rs");
        let off = concat!("NSWindow::setAllowsAutomatic", "WindowTabbing(false, mtm);");
        assert_eq!(SRC.matches(off).count(), 1, "once");
        assert_eq!(
            SRC.matches(concat!("setAllowsAutomatic", "WindowTabbing(true"))
                .count(),
            0,
            "never back on"
        );
        let run = body(SRC, concat!("\npub fn ", "run() {"), "");
        let at = run.find(off).expect("in the launch");
        let built = run
            .find(concat!("Opening::", "new("))
            .expect("the launch builds its window");
        assert!(at < built, "before the window is built");
    }

    /// **A window's close takes down that window alone, and the shell's
    /// exit closes its window rather than quitting the app**: the app
    /// terminates itself in one place, the close of a window, and only
    /// when it was the last and `windows::LAST_WINDOW` says to quit, after
    /// the window's driver, and so its shell, is let go; the pump's end of
    /// the stream asks for its own window's close and terminates nothing,
    /// so a shell's exit in the last window goes through that close; every
    /// window is put in the open windows, and its close heard, before its
    /// responder is installed; and the close swaps the driver out on a
    /// later turn, its display link stopped. Command-N, heard in the key
    /// handler's Command arm, opens the window on a later turn too. The
    /// windows are AppKit's, so the source is what can fail.
    #[test]
    fn a_window_closes_alone_and_its_shells_exit_closes_only_it() {
        const SRC: &str = include_str!("app.rs");
        let terminate = concat!(".termi", "nate(");
        assert_eq!(
            SRC.matches(terminate).count(),
            1,
            "the app terminates itself in one place"
        );
        let pump = body(
            SRC,
            concat!("    fn pump(&mut self, ", "trigger: Trigger) {"),
            "    ",
        );
        let (_, closed) = pump
            .split_once(concat!("if drained.stream == ", "Stream::Closed {"))
            .expect("the control: the pump sees the end of the stream");
        let arm = closed.split_once("\n        }\n").expect("its arm ends").0;
        assert!(
            arm.contains(concat!("self.close_", "later();")),
            "the shell's exit closes its window"
        );
        assert!(!arm.contains(terminate), "and terminates nothing itself");
        let show = launch(SRC).show;
        let registered = show
            .find(concat!("o.open(", "Open {"))
            .expect("the window is put in the open windows");
        let heard = show
            .find(concat!("closing::when_", "closing("))
            .expect("its close is heard");
        let installed = show
            .find(concat!("view.set_", "responder("))
            .expect("the responder is installed");
        assert!(registered < installed && heard < installed, "{show}");
        let close = body(
            SRC,
            concat!("fn closed(id: crate::windows::", "WindowId) {"),
            "",
        );
        let stopped = close
            .find(concat!(".stop_display_", "link();"))
            .expect("its display link stops");
        let swapped = close
            .find(concat!(".set_responder(Box::new(", "Gone));"))
            .expect("its driver is swapped out");
        assert!(stopped < swapped, "{close}");
        let asked = close
            .find(concat!(
                "if crate::windows::after_close(left, ",
                "crate::windows::LAST_WINDOW)"
            ))
            .expect("the close asks what the app does after it");
        assert!(swapped < asked, "the driver let go first: {close}");
        let quits = close[asked..]
            .split_once(concat!("== crate::windows::AfterClose::", "Quit"))
            .expect("and quits when told to")
            .1;
        assert!(
            quits.contains(terminate),
            "told to quit, the last window's close quits the app: {close}"
        );
        assert!(
            SRC.contains(concat!("exec_async(move || ", "closed(id));")),
            "closed on a later turn"
        );
        let key = body(
            SRC,
            concat!("    fn key(&mut self, key: ", "&KeyPress) -> Handled {"),
            "    ",
        );
        let (command, _) = key
            .split_once("return Handled::Consumed;")
            .expect("the Command arm");
        assert!(
            command.contains(concat!("self.new_", "window();")),
            "Command-N in the Command arm"
        );
        let new = body(SRC, concat!("    fn new_", "window(&mut self) {"), "    ");
        assert!(
            new.contains(concat!("exec_async(move || ", "open_window(at));")),
            "opened on a later turn: {new}"
        );
    }

    /// **This machine's names are read off the main thread**: the launch
    /// starts the read on a thread of its own and never reads them itself,
    /// so `hostName`'s first call, 5-29 ms measured, is not between the
    /// window and its first frame.
    #[test]
    fn the_machines_names_are_read_off_the_launch() {
        const SRC: &str = include_str!("app.rs");
        let launch = launch(SRC);
        assert!(
            launch.before.contains("Hosts::start()"),
            "the read is started"
        );
        for part in [launch.before.as_str(), launch.callback, launch.show] {
            assert!(
                !part.contains(concat!("machine_", "names()")),
                "read on the launch's own thread"
            );
        }
        let start = SRC
            .split_once(concat!("fn start() -> ", "Hosts {"))
            .map(|(_, rest)| rest)
            .expect("the read's start");
        assert!(
            start
                .split_once("\n    }\n")
                .expect("its end")
                .0
                .contains(".spawn(machine_names)"),
            "on a thread of its own"
        );
    }

    /// **The shell is spawned before the GPU's probe and the read of the
    /// machine's names** (note AJ section 20): `fork` first takes dyld's
    /// `dlopen` lock and then runs every `pthread_atfork` handler, and
    /// libinfo's mDNS handler waits for a lookup in flight, so a thread
    /// reading the machine's name, or one loading the GPU's driver, held
    /// the shell's start for as long as it took. Both start once
    /// `Pty::spawn` has returned, and `Boot::prepare`, which runs before
    /// it, starts none; the font scan `main()` starts is the one thread
    /// that can still be alive at the fork. The launch is AppKit-bound, so the source is what can
    /// fail.
    #[test]
    fn the_shell_is_spawned_before_the_launchs_threads() {
        const SRC: &str = include_str!("app.rs");
        let launch = launch(SRC).before;
        let spawn = launch
            .find(concat!("Pty::", "spawn("))
            .expect("the control: the shell is spawned in the launch");
        for start in [
            concat!("Hosts::", "start()"),
            concat!("Boot::", "start_probe("),
        ] {
            let at = launch
                .find(start)
                .unwrap_or_else(|| panic!("{start} in the launch"));
            assert!(spawn < at, "{start} before the shell is spawned");
        }
        let prepare = SRC
            .split_once(concat!("fn prepare(scale: ", "CGFloat) -> Boot {"))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| body)
            .expect("the boot");
        assert!(
            prepare.contains(concat!("Config::", "launch()")),
            "the control: the boot reads the config"
        );
        assert!(
            !prepare.contains(concat!("thread::", "spawn")),
            "the boot starts a thread before the shell is spawned"
        );
    }

    /// **Every window plays the logo, as the launch's does**: a window
    /// opened after the launch -- Command-N, File > New Window, a Dock
    /// click -- goes through `open_window`, which starts its
    /// shell, builds it and shows it as `run` does and sets nothing of the
    /// splash, so its boot carries `[launch] splash` and `App::new` reads
    /// `BAUD_SPLASH` over it, as for the first window. Its shell is
    /// spawned in `Shell::start`, before the window is built, and its
    /// first frame is drawn in `show`. The windows are AppKit's, so the
    /// source is what can fail.
    #[test]
    fn every_window_plays_the_logo_as_the_launchs_does() {
        const SRC: &str = include_str!("app.rs");
        let open = body(
            SRC,
            concat!("\nfn open_", "window(at: Option<std::path::PathBuf>) {"),
            "",
        );
        for named in ["launch_splash", "LaunchSplash", "splash"] {
            assert!(!open.contains(named), "{named} in open_window: {open}");
        }
        let shell = open
            .find(concat!("Shell::", "start("))
            .expect("the control: the window's shell is started");
        let built = open
            .find(concat!("Opening::", "new("))
            .expect("the window is built");
        let shown = open
            .find(concat!("opening.", "show(mtm, scale, after);"))
            .expect("and shown");
        assert!(shell < built && built < shown, "{open}");
        let start = body(
            SRC,
            concat!("fn start(scale: CGFloat, size: NSSize, ", "inset: CGFloat,"),
            "    ",
        );
        assert!(start.contains(concat!("Pty::", "spawn(")), "spawned there");
        assert!(
            launch(SRC)
                .show
                .contains(concat!("driver.app.present(&v, ", "Show::Now);")),
            "the first frame is drawn in show"
        );
        let prepare = body(
            SRC,
            concat!("    fn prepare(scale: ", "CGFloat) -> Boot {"),
            "    ",
        );
        assert!(
            prepare.contains(concat!("launch_splash: cfg.", "launch_splash,")),
            "the boot's splash is the config's"
        );
        assert!(
            SRC.contains(concat!(
                "splash: crate::splash::Splash::new(crate::splash::",
                "splash_override(\n                std::env::var(\"BAUD_SPLASH\")"
            )),
            "BAUD_SPLASH over it, for every window"
        );
    }

    /// **Nothing a key or a read does waits on the splash**: the key's
    /// way to the shell (`key`, `typed`, `send`) and the pump that feeds
    /// the shell's output to the terminal and owes its frame name nothing
    /// of it, in every window; the splash is read by the frame, which
    /// draws it over the grid, and the tick. The windows are AppKit's, so
    /// the source is what can fail.
    #[test]
    fn no_key_or_read_waits_on_the_splash() {
        const SRC: &str = include_str!("app.rs");
        for head in [
            concat!("    fn key(&mut self, key: ", "&KeyPress) -> Handled {"),
            concat!(
                "    fn typed(&mut self, e: KeyEvent, ",
                "key: Option<VirtualKey>) {"
            ),
            concat!("    fn send(&mut self, e: ", "KeyEvent) -> Sent {"),
            concat!("    fn pump(&mut self, ", "trigger: Trigger) {"),
        ] {
            let path = body(SRC, head, "    ");
            assert!(!path.contains("splash"), "{head} reads the splash");
        }
        // And the frame gate the pump decides by, whose fields are the
        // only way anything reaches it (`frame_gate`).
        for (head, end) in [
            (
                concat!("fn frame_gate(g: ", "FrameGate) -> Decided {"),
                "\n}\n",
            ),
            (concat!("struct ", "FrameGate {"), "\n}\n"),
        ] {
            let at = SRC.find(head).unwrap_or_else(|| panic!("no {head}"));
            let gate = &SRC[at..at + SRC[at..].find(end).expect("its end")];
            assert!(gate.len() > 200, "{head} is read whole");
            assert!(
                !gate.contains("splash") && !gate.contains("Splash"),
                "{head} reads the splash"
            );
        }
        let drawn = body(
            SRC,
            concat!("    fn frame(&mut self, ", "target: f64) {"),
            "    ",
        );
        assert!(
            drawn.contains(concat!("self.app.splash.", "owes(target)")),
            "the control: the tick reads it"
        );
    }
}

#[cfg(test)]
mod refont_tests {
    use super::{Builds, Deferred, Pending, Refont, Reshape, builds, read_owes, refont_step};
    use crate::fonts::Slot;
    use baud_present_macos::InFlight;
    use baud_render::Tally;

    /// Where a change of font comes from.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum From {
        /// The View menu's zoom or its keys: `zoom`, from the event.
        Zoom,
        /// A program's `OSC 50`, read in the pump among the bytes around it
        /// (`font_requests`).
        Program,
        /// The config's fonts, read again at the top of the pump
        /// (`apply_config` through `reload_fonts`, which does not wait).
        ConfigFonts,
        /// The config's size alone, which `apply_config` hands to `zoom`.
        ConfigSize,
    }

    /// A font: the face a program or the config put in the lead, if any,
    /// and the size in points.
    #[derive(Clone, Copy, PartialEq, Debug)]
    struct Font {
        face: Option<Slot>,
        points: f32,
    }

    /// A moment, or a length of time, in the model's steps.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Step(usize);

    /// How many changes are asked at once.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Burst(usize);

    /// Whether the new cell size moves the grid, so `relayout` draws.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Grid {
        Moves,
        Stays,
    }

    /// Whether the program redraws when the first frame lands: bytes in
    /// the pump that performs a waiting change.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Redraw {
        Yes,
        No,
    }

    /// Whether a frame is in flight when the change arrives.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Start {
        Busy,
        Idle,
    }

    /// Whether a turn of the pump builds the frame owed (`due`).
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Due {
        Yes,
        No,
    }

    /// Whether a rescale is one `refont` holds back for a frame in flight.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Held {
        ByRefont,
        Not,
    }

    /// One scenario's knobs.
    #[derive(Clone, Copy, Debug)]
    struct Case {
        from: From,
        /// Changes asked at once, each a point past the size ahead.
        burst: Burst,
        /// Steps a frame takes to land.
        land: Step,
        /// The display link ticks every this many steps.
        tick: Step,
        grid: Grid,
        redraw: Redraw,
        start: Start,
    }

    /// What the model's driver did after the change arrived.
    #[derive(Debug, Default)]
    struct Report {
        /// Frames built, refused ones included, and the font of each.
        built: Vec<Font>,
        /// Frames built while the GPU was still drawing the last.
        over_live: Tally,
        /// Rescales and reflows done while a frame was in flight, on the
        /// paths `refont` holds back for it.
        reshaped_live: Tally,
    }

    /// The driver as far as a font change reaches it, over the real
    /// `refont_step`, `deferred`, `builds` and `read_owes`.
    struct Driver {
        case: Case,
        now: Step,
        /// When the frame in flight lands.
        flying: Option<Step>,
        pending: Pending,
        deferred: Deferred,
        /// The font the renderer is built at.
        font: Font,
        /// `App::frames`: frames that reached the end of `present`.
        frames: Tally,
        /// A config read the watcher saw, for the next pump.
        config: Option<Font>,
        /// A read waiting in the channel, with the `OSC 50`s it carries.
        bytes: Option<Vec<Font>>,
        r: Report,
    }

    impl Driver {
        fn flight(&self) -> InFlight {
            match self.flying {
                Some(_) => InFlight::Busy,
                None => InFlight::Idle,
            }
        }

        /// `Presenter::reshape`: a frame in flight is reading.
        fn reshape(&self) -> Reshape {
            match self.flying {
                Some(_) => Reshape::Wait,
                None => Reshape::Now,
            }
        }

        /// The renderer rebuilt at `f`, and what a frame in flight saw.
        fn refonted(&mut self, f: Font, held: Held) {
            if held == Held::ByRefont && self.flying.is_some() {
                self.r.reshaped_live.bump();
            }
            self.font = f;
        }

        /// `App::present`.
        fn present(&mut self) {
            if self.pending == Pending::Clean {
                return;
            }
            let flight = self.flight();
            if builds(Some(flight)) == Builds::Owed {
                return;
            }
            self.r.built.push(self.font);
            if flight == InFlight::Busy {
                // Built, and refused by `draw_cells`: still owed.
                self.r.over_live.bump();
                return;
            }
            self.flying = Some(Step(self.now.0 + self.case.land.0));
            self.pending = Pending::Clean;
            self.frames.bump();
        }

        /// `relayout`, for a new cell size: it draws when the grid moved.
        fn relayout(&mut self) -> super::Drew {
            if self.case.grid == Grid::Moves {
                self.present();
                return super::Drew::Yes;
            }
            super::Drew::No
        }

        /// `Driver::size_ahead`.
        fn ahead(&self) -> f32 {
            match self.deferred {
                Deferred::Zoom(p) | Deferred::Font(_, p) => p,
                Deferred::Nothing => self.font.points,
            }
        }

        /// `Driver::refont`.
        fn refont(&mut self, face: Option<Slot>, points: f32) {
            let leads = self.font.face;
            let step = refont_step(
                self.deferred,
                Some(self.reshape()),
                face,
                points,
                self.font.points,
                |f| Some(f) == leads,
            );
            let face = match step {
                Refont::Nothing => return,
                Refont::Wait(d) => {
                    self.deferred = d;
                    return;
                }
                Refont::Now(face) => face,
            };
            let f = Font {
                face: face.or(self.font.face),
                points,
            };
            self.refonted(f, Held::ByRefont);
            self.pending = Pending::Dirty;
            if self.relayout() == super::Drew::No {
                self.present();
            }
        }

        /// `apply_config`'s fonts and size, after every other part.
        fn config_read(&mut self, f: Font) {
            let before = self.frames;
            match self.case.from {
                From::ConfigFonts => {
                    self.refonted(f, Held::Not);
                    self.pending = Pending::Dirty;
                    if self.relayout() == super::Drew::No {
                        self.present();
                    }
                }
                _ => self.refont(None, f.points),
            }
            self.pending = read_owes(before, self.frames, self.pending);
        }

        /// `Driver::pump`, in its order: the config, the gate read once,
        /// the waiting change, the bytes and their `OSC 50`s, the frame.
        fn pump(&mut self, due: Due) {
            if let Some(f) = self.config.take() {
                self.config_read(f);
            }
            let busy = self.flight() == InFlight::Busy;
            if !busy {
                match core::mem::replace(&mut self.deferred, Deferred::Nothing) {
                    Deferred::Nothing => {}
                    Deferred::Zoom(p) => self.refont(None, p),
                    Deferred::Font(face, p) => self.refont(face, p),
                }
            }
            if let Some(asked) = self.bytes.take() {
                self.pending = Pending::Dirty;
                for f in asked {
                    self.refont(f.face, f.points);
                }
            }
            if due == Due::Yes && !busy {
                self.present();
            }
        }

        fn settled(&self) -> bool {
            self.flying.is_none()
                && self.pending == Pending::Clean
                && matches!(self.deferred, Deferred::Nothing)
                && self.config.is_none()
                && self.bytes.is_none()
        }
    }

    const OLD: Font = Font {
        face: None,
        points: 13.0,
    };

    /// The change arrives at step 0, over a frame of the old font in
    /// flight or none; the loop runs until nothing is owed. Returns what
    /// was built from then on, and the font asked.
    fn run(case: Case) -> (Report, Font) {
        let mut d = Driver {
            case,
            now: Step(0),
            flying: match case.start {
                Start::Busy => Some(case.land),
                Start::Idle => None,
            },
            pending: Pending::Clean,
            deferred: Deferred::Nothing,
            font: OLD,
            frames: Tally::default(),
            config: None,
            bytes: None,
            r: Report::default(),
        };
        let face = match case.from {
            From::Program | From::ConfigFonts => Some(Slot::TEST),
            From::Zoom | From::ConfigSize => None,
        };
        let mut points = OLD.points;
        for _ in 0..case.burst.0 {
            points += 1.0;
        }
        let asked = Font { face, points };
        match case.from {
            From::Zoom => {
                for _ in 0..case.burst.0 {
                    let to = d.ahead() + 1.0;
                    d.refont(None, to);
                }
            }
            From::Program => {
                let mut reads = Vec::new();
                let mut points = OLD.points;
                for _ in 0..case.burst.0 {
                    points += 1.0;
                    reads.push(Font { face, points });
                }
                d.bytes = Some(reads);
                d.pump(Due::No);
            }
            From::ConfigFonts | From::ConfigSize => {
                d.config = Some(asked);
                d.pump(Due::No);
            }
        }
        let mut redrawn = case.redraw == Redraw::No;
        loop {
            if d.flying == Some(d.now) {
                d.flying = None;
                if !redrawn {
                    d.bytes = Some(Vec::new());
                    redrawn = true;
                }
                // A frame's time has passed since the last commit, so the
                // wake draws (`due`).
                d.pump(Due::Yes);
            }
            if d.now.0.is_multiple_of(case.tick.0) {
                d.pump(Due::Yes);
            }
            if d.settled() && redrawn {
                return (d.r, asked);
            }
            d.now = Step(d.now.0 + 1);
            assert!(d.now.0 < 256, "never settled: {case:?} {:?}", d.r);
        }
    }

    fn cases() -> impl Iterator<Item = Case> {
        let froms = [
            From::Zoom,
            From::Program,
            From::ConfigFonts,
            From::ConfigSize,
        ];
        froms.into_iter().flat_map(|from| {
            [1, 3, 12].map(Burst).into_iter().flat_map(move |burst| {
                [1, 3, 7].map(Step).into_iter().flat_map(move |land| {
                    [1, 2, 5].map(Step).into_iter().flat_map(move |tick| {
                        [Grid::Stays, Grid::Moves]
                            .into_iter()
                            .flat_map(move |grid| {
                                [Redraw::No, Redraw::Yes]
                                    .into_iter()
                                    .flat_map(move |redraw| {
                                        // A burst over an idle GPU is a change now
                                        // and then one that waits: two frames, by
                                        // design. Bursts are asked over a frame.
                                        [Start::Busy, Start::Idle]
                                            .into_iter()
                                            .filter(move |&start| {
                                                start == Start::Busy || burst == Burst(1)
                                            })
                                            .map(move |start| Case {
                                                from,
                                                burst,
                                                land,
                                                tick,
                                                grid,
                                                redraw,
                                                start,
                                            })
                                    })
                            })
                    })
                })
            })
        })
    }

    /// **A zoom, a program's font or the config's, asked while a frame is
    /// in flight or not, builds one frame**, at the font asked, and none
    /// over a frame the GPU is still drawing; a burst of them builds one,
    /// at the last. When the program redraws as the first frame lands --
    /// in the pump that performs a waiting change, or after the change's
    /// own frame -- its bytes are a second frame, built once the first has
    /// landed, at the new font too; a frame still owed takes them in. The zoom and the program's change
    /// rescale and re-lay nothing under a frame in flight: they wait for
    /// it (`refont_step`). Over a GPU that lands a frame in one step,
    /// three and seven, and a display link every step, second and fifth.
    /// The config's cases over a frame in flight model a read the driver
    /// never makes: `follow_config` returns while the presenter says
    /// `Reshape::Wait`. They are kept as over-coverage.
    #[test]
    fn a_font_change_builds_one_frame_at_the_font_asked() {
        let mut n = 0;
        for case in cases() {
            let (r, asked) = run(case);
            let at = format!("{case:?}: {r:?}");
            assert_eq!(r.over_live, Tally::default(), "{at}");
            assert!(r.built.iter().all(|f| *f == asked), "{at}");
            // A frame left owed carries the bytes that arrive before it is
            // built: the config's fonts over a frame in flight.
            let carried = case.start == Start::Busy && case.from == From::ConfigFonts;
            let want = 1 + usize::from(case.redraw == Redraw::Yes && !carried);
            assert_eq!(r.built.len(), want, "{at}");
            if case.from != From::ConfigFonts {
                assert_eq!(r.reshaped_live, Tally::default(), "{at}");
            }
            n += 1;
        }
        assert_eq!(n, 4 * (3 + 1) * 3 * 3 * 2 * 2);
    }

    /// **`refont` draws once, after the damage**: the full damage and the
    /// owed frame are set before `relayout`, which draws the frame when the
    /// grid moved, and `refont` asks `present` itself only when it did not.
    /// It is AppKit-bound, so the source is what can fail. A second ask
    /// after the relayout was the double render: it found the first frame
    /// in flight and built nothing, or, before the damage moved ahead of
    /// the relayout, drew the old size first.
    #[test]
    fn refont_draws_once_after_the_damage() {
        const SRC: &str = include_str!("app.rs");
        let body = SRC
            .split_once(concat!(
                "fn refont(&mut self, face: Option<crate::fonts::Slot>, ",
                "points: f32) {"
            ))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| body)
            .expect("refont's body");
        let at = |tok: &str| body.find(tok).unwrap_or_else(|| panic!("{tok} in refont"));
        let relayout = at(concat!(
            "if self.relayout(self.view.frame().size, scale) ",
            "== Drew::No {"
        ));
        assert!(at("self.app.next_damage = Damage::Full;") < relayout);
        assert!(at("self.app.pending = Pending::Dirty;") < relayout);
        let present = concat!("self.app.", "present(");
        assert_eq!(body.matches(present).count(), 1, "one present in refont");
        assert!(
            relayout < at(present),
            "only when the relayout drew nothing"
        );
    }

    /// **The pump performs a waiting change before it feeds or draws**, so
    /// the frame a read owes is built at the new font rather than once at
    /// the old and again at the new; and a config read leaves owed what
    /// `read_owes` says, measured from after its last part.
    #[test]
    fn the_pump_performs_a_waiting_change_before_it_draws() {
        const SRC: &str = include_str!("app.rs");
        let pump = SRC
            .split_once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| body)
            .expect("the pump");
        let at = |tok: &str| {
            pump.find(tok)
                .unwrap_or_else(|| panic!("{tok} in the pump"))
        };
        let replay = at(concat!(
            "Deferred::Font(face, points) => ",
            "self.refont(face, points),"
        ));
        assert!(at("self.follow_config();") < replay, "the config first");
        assert!(replay < at("let drained = self.app.pty.drain();"));
        assert!(replay < at(concat!("self.app.", "present(&view, Show::Event);")));
        let config = SRC
            .split_once(concat!("    fn apply_", "config("))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| body)
            .expect("apply_config");
        let at = |tok: &str| {
            config
                .find(tok)
                .unwrap_or_else(|| panic!("{tok} in apply_config"))
        };
        let before = at("let before = self.app.frames;");
        assert!(at("for &p in parts {") < before, "after every part");
        assert!(before < at("if parts.contains(&Part::Fonts) {"));
        let owes = at(concat!(
            "self.app.pending = read_owes(",
            "before, self.app.frames, self.app.pending);"
        ));
        assert!(
            at(concat!(
                "if self.relayout(self.view.frame().size, self.app.scale) ",
                "== Drew::No {"
            )) < owes
        );
        assert_eq!(
            config
                .matches("self.app.pending = Pending::Dirty;\n        said")
                .count(),
            0
        );
    }

    /// `read_owes` on its own: a read that built a frame owes what is
    /// pending after it, one that built none owes a frame.
    #[test]
    fn a_read_owes_a_frame_only_when_it_built_none() {
        let (a, b) = (Tally::default(), Tally::of(1));
        assert!(read_owes(a, a, Pending::Clean) == Pending::Dirty);
        assert!(read_owes(a, b, Pending::Clean) == Pending::Clean);
        assert!(read_owes(a, b, Pending::Dirty) == Pending::Dirty);
    }
}

#[cfg(test)]
mod resize_selection_tests {
    use super::kept_across_resize;
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, Col, ReadLen, Row, Rows, WinSize};
    use baud_render::{Grain, Scrolled, Selection, Span};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 10,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// Forty numbered lines on a ten-row screen, `setup` after them, a
    /// selection from screen row `a` to row `b` numbered as the driver's
    /// count would have it, then each resize as `relayout` makes it: what
    /// the selection's text is after the last, or `None` when it went.
    fn across(setup: &[u8], (a, b): (usize, usize), sizes: &[(u16, u16)]) -> Option<String> {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let lines: Vec<u8> = (0..40)
            .flat_map(|i| format!("line {i:02}\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        feed(&mut t, setup);
        let mut scrolled = Scrolled::START.advanced(Rows::of_count(t.scrollback().len()));
        let span = |t: &Term<'_>, y: usize, s: Scrolled| {
            let r = Row::from_index(y);
            Span::cell(
                t.row(r).expect("a row on the grid"),
                s.line(Rows::ZERO, r),
                Col::ZERO,
            )
        };
        let mut sel = Selection::at(span(&t, a, scrolled), Grain::Line);
        sel.extend(span(&t, b, scrolled));
        let want = sel.text(&t, scrolled);
        let mut kept = Some(sel);
        for &(c, r) in sizes {
            let was = (t.history_mark(), t.geometry().0, t.screen());
            let _ = t.resize(&WinSize {
                ws_col: c,
                ws_row: r,
            });
            (kept, scrolled) = kept_across_resize(&t, was, kept, scrolled);
        }
        let got = kept.map(|s| s.text(&t, scrolled));
        if let Some(g) = &got {
            assert_eq!(g, &want, "a kept selection holds its own text");
        }
        got
    }

    /// **A height-only resize keeps the selection on its lines** (note V,
    /// V3): with the cursor on the last row a shrink to six rows retires
    /// four into the history and the growth back pulls them; the selection
    /// on rows 3 to 5 holds "line 34" to "line 36" through both, and through
    /// a growth that pulls more history than the shrink retired. Every
    /// change of grid cleared it until 2026-10-20.
    #[test]
    fn a_height_only_resize_keeps_the_selection_on_its_lines() {
        for sizes in [
            &[(20u16, 6u16)][..],
            &[(20, 6), (20, 10)],
            &[(20, 16)],
            &[(20, 4), (20, 3), (20, 12)],
        ] {
            let text = across(b"", (3, 5), sizes);
            assert!(
                text.as_deref().is_some_and(|t| t.starts_with("line 34")),
                "{sizes:?}: {text:?}"
            );
        }
    }

    /// **A change of columns clears it**: the re-cut renumbers every line
    /// and the columns no longer name its cells.
    #[test]
    fn a_width_change_clears_the_selection() {
        assert_eq!(across(b"", (3, 5), &[(16, 10)]), None);
        assert_eq!(across(b"", (3, 5), &[(20, 6), (24, 6)]), None);
    }

    /// **A resize on the alternate screen clears it**: the program redraws
    /// for the new size and the screen keeps no history to number by.
    #[test]
    fn a_resize_on_the_alternate_screen_clears_the_selection() {
        let alt: &[u8] = b"\x1b[?1049h\x1b[4;1Halt 3\r\nalt 4\r\nalt 5";
        assert_eq!(across(alt, (3, 5), &[(20, 8)]), None);
    }

    /// **A selection touching a row the shrink cut is cleared**, and one
    /// above the cut is kept: the cursor on row 2, so a shrink to six rows
    /// cuts rows 6 to 9 below it and retires none.
    #[test]
    fn a_selection_touching_cut_rows_is_cleared() {
        let up: &[u8] = b"\x1b[3;1H";
        assert_eq!(across(up, (4, 7), &[(20, 6)]), None, "into the cut rows");
        assert_eq!(across(up, (8, 9), &[(20, 6)]), None, "wholly in them");
        assert!(across(up, (1, 5), &[(20, 6)]).is_some(), "above them, kept");
    }
}

/// **What a full reset owes the app** (note V, V1): when the core's reset
/// count moves across a read, the selection goes, the echoes drawn ahead of
/// the shell are dropped as contradicted, the wheel's part-line settles, and
/// a window over the history comes to the live screen with its thumb revealed
/// -- and no screen-switch fade runs, because a reset on the primary puts no
/// other surface up. The window and the slide already follow
/// `Term::history_erasures`, which `RIS` moves; these hold what only the
/// reset count can say, and `CSI 3 J` -- which moves the erasure count and
/// keeps the screen -- is the control that tells the two apart. Each read
/// here is the pump's: `Before`, the feed, then `restart`; the window is the
/// core's (note V, V2).
#[cfg(test)]
mod restart_tests {
    use super::{Before, Guess, Kept, Prediction, Surface, Thumb, restart, window_of};
    use crate::latency::{Judged, Latency, Switch};
    use crate::reveal::Reveal;
    use crate::wheel::{Delta, Travel};
    use baud_core::{Owned, Screen, Term};
    use baud_domain::{BytePos, Codepoint, Col, HostTime, ReadLen, Row, Rows, WinSize};
    use baud_font::Px;
    use baud_render::{Grain, Scroll, Scrolled, Scroller, Selection, Slide, Span, bar};
    use std::time::{Duration, Instant};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 10,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// What the app keeps over the screen, owned, as `App` holds it.
    struct State {
        selection: Option<Selection>,
        predictions: Vec<Prediction>,
        guess: Guess,
        slide: Slide,
        travel: Travel,
        latency: Latency,
    }

    impl State {
        fn new() -> State {
            State {
                selection: None,
                predictions: Vec::new(),
                guess: Guess::None,
                slide: Slide::new(Px::of(20)),
                travel: Travel::default(),
                latency: Latency::new(Switch::On),
            }
        }

        /// One read, as the pump takes it.
        fn read(&mut self, t: &mut Term<'_>, bytes: &[u8]) -> (Surface, Thumb) {
            let before = Before::watched(t, self.selection.as_ref(), Scrolled::START);
            feed(t, bytes);
            restart(
                t,
                before,
                Kept {
                    selection: &mut self.selection,
                    predictions: &mut self.predictions,
                    guess: &mut self.guess,
                    slide: &mut self.slide,
                    travel: &mut self.travel,
                    latency: &mut self.latency,
                },
            )
        }

        /// Put the primary's window `back` lines up the history.
        fn look(&mut self, t: &mut Term<'_>, back: Rows) {
            t.set_window_back(back);
        }

        /// The primary's window, whatever screen is up.
        fn window(&self, t: &Term<'_>) -> Rows {
            t.primary_window_back()
        }

        /// A selection of screen row `r`'s first cell.
        fn select(&mut self, t: &Term<'_>, r: Row) {
            let line = Scrolled::START.line(Rows::ZERO, r);
            self.selection = Some(Selection::at(
                Span::cell(t.row(r).expect("a row on the grid"), line, Col::ZERO),
                Grain::Cell,
            ));
        }

        /// A key's echo drawn at `(row, col)` ahead of the shell, with its
        /// record open in the instrument: the record's seq.
        fn predict(&mut self, t: &Term<'_>, (row, col): (Row, Col)) -> baud_render::Tally {
            let at = |us| HostTime::of_nanos(1_000_000_000).plus(Duration::from_micros(us));
            self.latency.key(|| Some(at(0)), || at(10));
            self.latency.written(|| at(20));
            let seq = self
                .latency
                .predicted(|| at(30))
                .expect("a record to carry it");
            self.predictions.push(Prediction {
                row,
                col,
                cp: Codepoint::new(u32::from('l')).expect("a codepoint"),
                seq: Some(seq),
                at: Instant::now(),
                rot: t.rotation(),
            });
            self.guess = Guess::Owed;
            seq
        }
    }

    fn with_term(ws: &WinSize, f: impl FnOnce(&mut Term<'_>)) {
        let mut owned = Owned::new(ws);
        let mut t = Term::new(owned.storage(), ws);
        f(&mut t);
    }

    /// Forty lines on a ten-row screen: thirty-one behind it.
    fn with_history(f: impl FnOnce(&mut Term<'_>)) {
        with_term(&WS, |t| {
            let lines: Vec<u8> = (0..40)
                .flat_map(|i| format!("line {i}\r\n").into_bytes())
                .collect();
            feed(t, &lines);
            assert_eq!(t.scrollback().len(), 31);
            f(t);
        });
    }

    /// **A reset drops a selection, with or without a history**: on a fresh
    /// terminal nothing else across the read moves -- no line scrolled, no
    /// history to erase -- so only the reset count can say it. `DECSTR`,
    /// `ED 2` and `CSI 3 J` keep it: the screen it names is still the screen.
    #[test]
    fn a_reset_drops_a_selection_even_with_no_history() {
        with_term(&WS, |t| {
            let mut s = State::new();
            feed(t, b"$ echo word\r\nword\r\n$ ");
            s.select(t, Row::from_index(1));
            s.read(t, b"\x1b[!p");
            assert!(s.selection.is_some(), "DECSTR kept it");
            s.read(t, b"\x1bc");
            assert!(s.selection.is_none(), "a reset with no history dropped it");
        });
        with_history(|t| {
            let mut s = State::new();
            s.select(t, Row::from_index(4));
            for (bytes, what) in [
                (&b"\x1b[!p"[..], "DECSTR"),
                (b"\x1b[2J", "ED 2"),
                (b"\x1b[3J", "ED 3, which erased the history"),
            ] {
                s.read(t, bytes);
                assert!(s.selection.is_some(), "{what} dropped it");
            }
            assert_eq!(t.scrollback().len(), 0, "the control: ED 3 erased it");
            s.read(t, b"\x1bc");
            assert!(s.selection.is_none(), "a reset dropped it");
        });
    }

    /// **However the reset falls into reads**: the captured `reset(1)`, 20
    /// rows by 60 columns, fed whole up to its `ESC c` with a selection made
    /// there, and the rest in reads of 1, 2, 3, 4, 5, 8, 17 and 4096 bytes --
    /// one-byte reads put the `ESC` and the `c` in two.
    #[test]
    fn a_captured_reset_drops_the_selection_at_every_read_size() {
        const CAPTURE: &[u8] = include_bytes!("../../tests/reset/seq-reset.raw");
        let ws = WinSize {
            ws_col: 60,
            ws_row: 20,
        };
        let at = CAPTURE
            .windows(2)
            .position(|w| w == b"\x1bc")
            .expect("the capture's reset");
        for size in [1, 2, 3, 4, 5, 8, 17, 4096] {
            with_term(&ws, |t| {
                let mut s = State::new();
                s.read(t, &CAPTURE[..at]);
                assert_eq!(t.scrollback().len(), 26, "the control: it scrolled");
                s.select(t, Row::from_index(3));
                for chunk in CAPTURE[at..].chunks(size) {
                    s.read(t, chunk);
                }
                assert!(s.selection.is_none(), "read size {size} kept it");
                assert_eq!(s.window(t), Rows::ZERO, "read size {size}");
            });
        }
    }

    /// **The guesses go as contradicted**, on a reset and on a switch, each
    /// with its verdict in the instrument -- a bare clear left a key's record
    /// waiting for its timeout -- and stay pending across a read that does
    /// neither.
    #[test]
    fn a_reset_drops_outstanding_predictions_as_contradicted() {
        with_term(&WS, |t| {
            let mut s = State::new();
            feed(t, b"$ ");
            let seq = s.predict(t, (Row::from_index(0), Col::at_index(2)));
            s.read(t, b"\x1b[m");
            assert_eq!(
                s.predictions.len(),
                1,
                "the control: a read that resets nothing"
            );
            assert_eq!(s.latency.verdict(seq), Some(Judged::Unjudged));
            s.read(t, b"\x1bc");
            assert!(s.predictions.is_empty(), "the reset kept a guess");
            assert!(s.guess == Guess::None, "and its frame");
            assert_eq!(s.latency.verdict(seq), Some(Judged::Contradicted));
        });
    }

    #[test]
    fn a_switch_drops_predictions_with_a_verdict() {
        with_term(&WS, |t| {
            let mut s = State::new();
            feed(t, b"$ ");
            let seq = s.predict(t, (Row::from_index(0), Col::at_index(2)));
            s.read(t, b"\x1b[?1049h");
            assert!(s.predictions.is_empty(), "the switch kept a guess");
            assert_eq!(s.latency.verdict(seq), Some(Judged::Contradicted));
        });
    }

    /// **A switch drops the rest as a reset does, and settles the slide**,
    /// whose offset names a row the other surface does not have; a reset's
    /// slide is the pump's erase arm's, so `restart` leaves it -- the
    /// control.
    #[test]
    fn a_switch_drops_the_selection_and_settles_the_slide_and_the_travel() {
        with_history(|t| {
            let mut s = State::new();
            s.select(t, Row::from_index(4));
            s.slide.retire(Rows::of_count(1));
            let _ = s.travel.lines(Delta::Lines(0.5));
            assert_eq!(s.read(t, b"\x1b[?1049h").0, Surface::Switched);
            assert!(s.selection.is_none(), "the switch kept the selection");
            assert!(!s.slide.active(), "the switch kept the slide");
            assert_eq!(s.travel, Travel::default(), "the switch kept the travel");
            s.read(t, b"\x1b[?1049l");
            s.slide.retire(Rows::of_count(1));
            s.read(t, b"\x1bc");
            assert!(
                s.slide.active(),
                "the control: the reset's slide is the erase arm's"
            );
        });
    }

    /// **The wheel's part-line settles on a reset**, as a key's snap settles
    /// it, so the first turn after it does not carry travel from a window
    /// the reset ended.
    #[test]
    fn a_reset_settles_the_wheels_travel() {
        with_history(|t| {
            let mut s = State::new();
            s.look(t, Rows::of_count(5));
            let _ = s.travel.lines(Delta::Lines(0.5));
            s.read(t, b"x");
            assert_ne!(s.travel, Travel::default(), "the control: a read keeps it");
            s.read(t, b"\x1bc");
            assert_eq!(s.travel, Travel::default(), "the reset kept it");
        });
    }

    /// **A reset that brings a window over the history to the live screen
    /// reveals the thumb** (V1's first fork, yes), as a key's snap does: a
    /// programmatic scroll, which AppKit reveals. Not from the live screen,
    /// where nothing moved; not from the alternate screen, where the window
    /// drawn was the live one and the reset is a switch, which fades; and
    /// not on `CSI 3 J`'s snap, which V1 does not decide.
    #[test]
    fn a_reset_from_a_window_over_the_history_reveals_the_thumb() {
        with_history(|t| {
            let mut s = State::new();
            s.look(t, Rows::of_count(5));
            assert_eq!(s.read(t, b"\x1bc"), (Surface::Same, Thumb::Reveal));
            assert_eq!(s.window(t), Rows::ZERO, "the control: it came live");
        });
        with_history(|t| {
            let mut s = State::new();
            assert_eq!(s.read(t, b"\x1bc").1, Thumb::Leave, "from the live screen");
        });
        with_history(|t| {
            let mut s = State::new();
            s.look(t, Rows::of_count(5));
            assert_eq!(s.read(t, b"\x1b[3J").1, Thumb::Leave, "on ED 3");
            assert_eq!(s.window(t), Rows::ZERO, "the control: ED 3 brought it live");
        });
        with_history(|t| {
            let mut s = State::new();
            s.look(t, Rows::of_count(5));
            s.read(t, b"\x1b[?1049h");
            assert_eq!(
                s.window(t).index(),
                5,
                "the control: the primary's window kept"
            );
            assert_eq!(s.read(t, b"\x1bc"), (Surface::Switched, Thumb::Leave));
            assert_eq!(t.screen(), Screen::Normal);
            assert_eq!(s.window(t), Rows::ZERO);
        });
    }

    /// Whether any pump of `pumps` reveals the thumb, each a read as the
    /// pump takes it, from a window `back` rows up the history, after
    /// `setup` fed as a pump of its own.
    fn reveals(back: Rows, setup: &[u8], pumps: &[&[u8]]) -> bool {
        let mut any = false;
        with_history(|t| {
            let mut s = State::new();
            if !setup.is_empty() {
                s.read(t, setup);
            }
            assert!(
                t.scrollback().len() > back.index(),
                "the control: a history"
            );
            s.look(t, back);
            for bytes in pumps {
                any |= s.read(t, bytes).1 == Thumb::Reveal;
            }
        });
        any
    }

    /// **A reset's reveal is the same however its bytes fall into pumps**:
    /// the event loop feeds whatever arrived since its last turn, and the
    /// same bytes, with no frame between them, gave a reveal in one turn and
    /// none across two -- `?1049h` then `RIS`, the reset made from the
    /// alternate screen, whose drawn window was the live one. Each sequence
    /// is fed a byte a pump, which is the answer the rest must give, then
    /// at every read size up to whole, then cut in two at every byte; from
    /// a window five lines up, and from the live screen, the control, where
    /// nothing reveals; one row after an erasure in a pump before, so the
    /// watch must be armed at each pump's reading and not left where the
    /// terminal began. The rows pin the answer built at 7ade145 for a pump
    /// of its own: the reset reveals when the window it took live was the
    /// primary's and over the history at that byte (V1's first fork), so
    /// only the first erasure in a pump can, since any erasure takes the
    /// window live. The last four take the window live with `SD`, `RI` at
    /// the top or a prompt redraw before the reset: read at the pump's
    /// start, a pump holding both revealed (note V, V2).
    #[test]
    fn a_resets_reveal_is_the_same_however_the_bytes_fall_into_pumps() {
        let lines: Vec<u8> = (0..12)
            .flat_map(|i| format!("after {i}\r\n").into_bytes())
            .collect();
        let refill = [&b"\x1b[?1049h\x1bc"[..], &lines, b"\x1bc"].concat();
        let erased = [&b"\x1b[3J"[..], &lines].concat();
        let block = prompt_block();
        let rows: [(&str, &[u8], &[u8], bool); 15] = [
            ("a reset", b"", b"\x1bc", true),
            ("?1049h, a reset", b"", b"\x1b[?1049h\x1bc", false),
            ("?47h, a reset", b"", b"\x1b[?47h\x1bc", false),
            (
                "?1049h ?1049l, a reset",
                b"",
                b"\x1b[?1049h\x1b[?1049l\x1bc",
                true,
            ),
            ("a reset, ?1049h", b"", b"\x1bc\x1b[?1049h", true),
            (
                "on the alternate screen, ?1049l, a reset",
                b"\x1b[?1049h",
                b"\x1b[?1049l\x1bc",
                true,
            ),
            (
                "on the alternate screen, a reset",
                b"\x1b[?1049h",
                b"\x1bc",
                false,
            ),
            ("ED 3, a reset", b"", b"\x1b[3J\x1bc", false),
            ("a reset, ED 3", b"", b"\x1bc\x1b[3J", true),
            ("?1049h, a reset, lines, a reset", b"", &refill, false),
            (
                "after an erasure and a refill, a reset",
                &erased,
                b"\x1bc",
                true,
            ),
            ("SD 3, a reset", b"", b"\x1b[3T\x1bc", true),
            ("SD 5, a reset", b"", b"\x1b[5T\x1bc", false),
            (
                "five RI at the top, a reset",
                b"",
                b"\x1b[H\x1bM\x1bM\x1bM\x1bM\x1bM\x1bc",
                false,
            ),
            (
                "a prompt redraw, a reset",
                &block,
                b"\x1b[3;1H\x1b[J\x1b]133;A\x07\x1bc",
                false,
            ),
        ];
        for back in [Rows::of_count(5), Rows::ZERO] {
            let from = back.index();
            for (what, setup, bytes, want) in rows {
                let one: Vec<&[u8]> = bytes.chunks(1).collect();
                let canon = reveals(back, setup, &one);
                assert_eq!(
                    canon,
                    want && from != 0,
                    "{what}, from {from}, a byte a pump"
                );
                for size in 2..=bytes.len() {
                    let pumps: Vec<&[u8]> = bytes.chunks(size).collect();
                    assert_eq!(
                        reveals(back, setup, &pumps),
                        canon,
                        "{what}, from {from}, reads of {size}"
                    );
                }
                for cut in 1..bytes.len() {
                    let (a, b) = bytes.split_at(cut);
                    assert_eq!(
                        reveals(back, setup, &[a, b]),
                        canon,
                        "{what}, from {from}, cut at {cut}"
                    );
                }
            }
        }
    }

    /// A prompt block the shell opened on the last row and printed sixteen
    /// lines into, so its first row is seven lines up the history: a redraw
    /// from row 3 takes those seven back.
    fn prompt_block() -> Vec<u8> {
        [&b"\x1b]133;A\x07"[..], &b"p\r\n".repeat(16)].concat()
    }

    /// The primary's window after `pumps`, each a read as the pump takes it,
    /// from a window `back` lines up the history, after `setup` fed as a
    /// pump of its own.
    fn window_after(back: Rows, setup: &[u8], pumps: &[&[u8]]) -> Rows {
        let mut out = Rows::ZERO;
        with_history(|t| {
            let mut s = State::new();
            if !setup.is_empty() {
                s.read(t, setup);
            }
            s.look(t, back);
            for bytes in pumps {
                s.read(t, bytes);
            }
            out = s.window(t);
        });
        out
    }

    /// **A window over the history is the same however the bytes fall into
    /// pumps.** Each row is fed a byte a pump, which is the answer the rest
    /// must give, then at every read size, then cut in two at every byte,
    /// from a window five lines up. A full-screen `SD`, `RI` at the top and
    /// a prompt redraw take lines back from the history, and the five lines
    /// after them retire five again: a byte a pump, the take-back brings the
    /// window to the live screen and the lines leave it there, while a pump
    /// that holds both saw no net movement and left the window five up. The
    /// controls are the rows a window keeps: five lines alone, and `SD 3`,
    /// which leaves it two up for the lines to carry back.
    #[test]
    fn a_window_over_the_history_is_the_same_however_the_bytes_fall_into_pumps() {
        let five = b"\n".repeat(5);
        let block = prompt_block();
        let rows: [(&str, &[u8], Vec<u8>, usize); 5] = [
            ("five lines", b"", five.clone(), 10),
            (
                "SD 3, five lines",
                b"",
                [&b"\x1b[3T"[..], &five].concat(),
                7,
            ),
            (
                "SD 5, five lines",
                b"",
                [&b"\x1b[5T"[..], &five].concat(),
                0,
            ),
            (
                "five RI at the top, five lines",
                b"",
                [&b"\x1b[H\x1bM\x1bM\x1bM\x1bM\x1bM\x1b[10;1H"[..], &five].concat(),
                0,
            ),
            (
                "a prompt redraw, five lines",
                &block,
                [&b"\x1b[3;1H\x1b[J\x1b]133;A\x07$ \x1b[10;1H"[..], &five].concat(),
                0,
            ),
        ];
        let back = Rows::of_count(5);
        for (what, setup, bytes, want) in &rows {
            let one: Vec<&[u8]> = bytes.chunks(1).collect();
            let canon = window_after(back, setup, &one);
            assert_eq!(canon.index(), *want, "{what}, a byte a pump");
            for size in 2..=bytes.len() {
                let pumps: Vec<&[u8]> = bytes.chunks(size).collect();
                assert_eq!(
                    window_after(back, setup, &pumps),
                    canon,
                    "{what}, reads of {size}"
                );
            }
            for cut in 1..bytes.len() {
                let (a, b) = bytes.split_at(cut);
                assert_eq!(
                    window_after(back, setup, &[a, b]),
                    canon,
                    "{what}, cut at {cut}"
                );
            }
        }
    }

    /// **What a reset's reveal shows**, through the frame's own bar
    /// (`baud_render::bar`, the overlay style, at the frame's widths): the thumb a
    /// key's snap would show is revealed, but `RIS` empties the history and a
    /// window with nothing behind it has no knob, so nothing is drawn at the reset;
    /// lines printed within the hold give a knob at the bottom of its travel until
    /// the fade ends. That is what HEAD draws, pinned here because the reveal's
    /// test read only `Thumb`. What an emptied history shows is no knob, as built:
    /// V1's fork says reveal "on that snap", the scrollbar note draws no thumb with
    /// no history, and Apple's `flashScrollers` text permits a flash on new content
    /// without requiring one. The controls: the instrument draws a knob over the
    /// history before the reset, and with the reveal left out the lines show none.
    #[test]
    fn a_resets_reveal_draws_a_knob_only_over_lines_printed_after_it() {
        let size = (Px::of(800), Px::of(500));
        let knob = |t: &Term<'_>, rv: Reveal, now: HostTime| {
            let s = Scroll {
                window: window_of(t),
                look: rv.look_under(Scroller::Overlay, now),
                widths: crate::reveal::widths(crate::reveal::Scale(2.0)),
                scroller: Scroller::Overlay,
            };
            bar(s, size, Px::of(10)).and_then(|b| b.knob)
        };
        let lines: Vec<u8> = (0..15)
            .flat_map(|i| format!("after {i}\r\n").into_bytes())
            .collect();
        for revealed in [true, false] {
            with_history(|t| {
                let t0 = HostTime::of_nanos(1_000_000_000);
                let at = |ms: u64| t0.plus(Duration::from_millis(ms));
                let mut s = State::new();
                s.look(t, Rows::of_count(5));
                let mut shown = Reveal::HIDDEN;
                shown.reveal(t0);
                assert!(knob(t, shown, t0).is_some(), "the control");
                let mut rv = Reveal::HIDDEN;
                if s.read(t, b"\x1bc").1 == Thumb::Reveal && revealed {
                    rv.reveal(at(0));
                }
                let sample = |t: &Term<'_>, rv: &mut Reveal, ms: u64| {
                    rv.step(at(ms));
                    knob(t, *rv, at(ms))
                };
                assert_eq!(t.scrollback().len(), 0, "the reset emptied it");
                if revealed {
                    assert!(rv.look(at(10)).shown.get() > 0, "the thumb is revealed");
                }
                assert!(
                    sample(t, &mut rv, 10).is_none(),
                    "no knob at the reset (as built)"
                );
                assert!(sample(t, &mut rv, 90).is_none());
                s.read(t, &lines);
                assert_eq!(t.scrollback().len(), 6, "the lines refilled it");
                for ms in [110, 400, 700] {
                    assert_eq!(
                        sample(t, &mut rv, ms).is_some(),
                        revealed,
                        "a knob over the lines at {ms} ms, revealed: {revealed}"
                    );
                }
                assert!(sample(t, &mut rv, 800).is_none(), "faded");
            });
        }
    }

    /// **A reset on the primary is not a switch**, so the fade and what
    /// follows the cursor across one stay keyed on the screen epoch (V1's
    /// second fork, no); a reset from the alternate screen is both.
    #[test]
    fn a_reset_on_the_primary_runs_no_fade() {
        with_history(|t| {
            let mut s = State::new();
            assert_eq!(s.read(t, b"\x1bc").0, Surface::Same);
            s.read(t, b"\x1b[?1049h");
            assert_eq!(s.read(t, b"\x1bc").0, Surface::Switched);
        });
    }

    /// **The pump takes the read as these tests do.** It is AppKit-bound,
    /// so the source is held: the erasure watch and the row watch, on the
    /// selection the read finds, are armed before the feed, and the row
    /// watch read after it; no window is read there -- the reset's window
    /// is the core's, kept at its byte (note V, V2);
    /// `restart` runs after the `Retired` arm, which can start a slide, and
    /// after the erased snap, so neither undoes what it settles, and before
    /// the clip; and the reveal and the switch's fade follow what it says.
    #[test]
    fn the_pump_restarts_after_the_reads_bookkeeping() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart, so this test's own text is not what it finds.
        const PUMP: &str = concat!("    fn pump(&mut self, ", "trigger: Trigger) {");
        assert_eq!(SRC.matches(PUMP).count(), 1, "one pump");
        let pump = SRC.split(PUMP).nth(1).expect("the pump");
        let pump = &pump[..pump.find("\n    }\n").expect("the pump ends")];
        let at = |needle: &str| {
            assert_eq!(
                pump.matches(needle).count(),
                1,
                "once in the pump: {needle}"
            );
            pump.find(needle).expect("found")
        };
        let call = at("let (surface, thumb) = restart(");
        let arm = at("let before = Before::watched(");
        assert!(arm < at("crate::reads::paced("));
        let armed = &pump[arm..arm + pump[arm..].find(");").expect("the arm ends")];
        assert!(
            armed.contains("self.app.selection.as_ref()") && armed.contains("self.app.scrolled"),
            "the row watch is armed on the selection the read finds: {armed}"
        );
        assert!(at("crate::reads::paced(") < at("stale_in_moved_rows(&self.app.term)"));
        assert!(
            !pump.contains("let window = "),
            "a window read at the pump's start"
        );
        let renumbered =
            at("self.app.scrolled = match numbered(&self.app.term, before, resized) {");
        assert!(renumbered < call);
        // The selection is carried by the watch after the count moved, so
        // the lines it names are read on the count they are numbered by.
        let carried = at("followed(&self.app.term, &mut self.app.selection, self.app.scrolled);");
        assert!(renumbered < carried && carried < call);
        assert!(at("if erased(&self.app.term, before) {") < call);
        assert!(call < at("if thumb == Thumb::Reveal {"));
        let reveal = &pump[at("if thumb == Thumb::Reveal {")..];
        let reveal = &reveal[..reveal.find("\n        }\n").expect("the arm ends")];
        assert!(
            reveal.contains("self.app.reveal.reveal(now);"),
            "the reveal arm reveals the thumb"
        );
        assert!(call < at("if surface == Surface::Switched {"));
        assert!(at("if surface == Surface::Switched {") < at("let oldest = oldest(&self.app.term"));
        assert!(
            !pump.contains("if switched("),
            "the switch is `restart`'s to say"
        );
    }

    /// **The pump keeps a selection's text as the Linux seat does**
    /// (`keep::KeptText`, whose rules the seat's tests hold): taken before
    /// the row watch is armed, kept where the watch drops the selection,
    /// and the selection dropped off the primary's top while `?1040` is
    /// reset, after the count moved and before the scroll carries it, and
    /// kept as the scroll carried it and the clip cut it; Cmd-C copies the
    /// text taken when the selection was made while there is one, and the
    /// kept text when there is none; and a gesture tells it before the pump.
    /// The pump is AppKit-bound, so the source is held.
    #[test]
    fn the_pump_keeps_a_selections_text_as_the_seat_does() {
        const SRC: &str = include_str!("app.rs");
        const PUMP: &str = concat!("    fn pump(&mut self, ", "trigger: Trigger) {");
        let pump = SRC.split(PUMP).nth(1).expect("the pump");
        let pump = &pump[..pump.find("\n    }\n").expect("the pump ends")];
        let at = |needle: &str| {
            assert_eq!(pump.matches(needle).count(), 1, "once: {needle}");
            pump.find(needle).expect("found")
        };
        assert!(at("self.app.kept.take(") < at("let before = Before::watched("));
        let stale = at("if self.app.selection.is_some() && stale_in_moved_rows(&self.app.term) {");
        let drop = &pump[stale..stale + pump[stale..].find("\n        }\n").expect("ends")];
        assert!(
            drop.contains("self.app.kept.dropped(&self.app.term);"),
            "kept where the watch drops it: {drop}"
        );
        let renumbered =
            at("self.app.scrolled = match numbered(&self.app.term, before, resized) {");
        let off = at("scrolled_off_the_top(&self.app.term, &s, tops)");
        let carried = at("followed(&self.app.term, &mut self.app.selection, self.app.scrolled);");
        assert!(renumbered < off && off < carried);
        let clipped = at(".is_some_and(|s| s.clip(oldest) == Clip::Cleared);");
        let gone = &pump[clipped..clipped + pump[clipped..].find("\n        }\n").expect("ends")];
        assert!(
            gone.contains("self.app.kept.dropped(&self.app.term);"),
            "kept where the clip cuts it away: {gone}"
        );
        let rekeyed = at("self.app.kept.carried(self.app.selection.as_ref());");
        assert!(
            carried < clipped && clipped < rekeyed,
            "the text kept as the scroll carried it and the clip cut it"
        );
        const COPY: &str = concat!("    fn copy(", "&mut self) {");
        let copy = SRC.split(COPY).nth(1).expect("copy");
        let copy = &copy[..copy.find("\n    }\n").expect("copy ends")];
        assert!(
            copy.contains("crate::keep::copied(sel, &app.kept, &app.term, app.scrolled, wrap)")
                && copy.contains("clipboard::write_text(&text);")
                && !copy.contains("text_as("),
            "Cmd-C puts on the pasteboard what the copy rule a test reaches says \
             (`keep::copied`): the text taken when it was made, or the kept text: {copy}"
        );
        const SELECT: &str = concat!("    fn select(", "&mut self, event: &Mouse) {");
        let select = SRC.split(SELECT).nth(1).expect("select");
        let select = &select[..select.find("\n    }\n").expect("select ends")];
        let told = select
            .find("self.app.kept.gestured(")
            .expect("a gesture tells it");
        let pumped = select.find("self.pump(Trigger::Wake);").expect("pumps");
        assert!(told < pumped, "told before the pump takes the text");
    }
}

#[cfg(test)]
mod trail_cursor_tests {
    //! The cursor a trail follows, and helix, which hides the terminal's for
    //! good and draws its own at the selection. The rule is the core's
    //! (`Term::placed_cursor`); what is the app's is that a placement is
    //! followed, never drawn, and arrived at as a block.
    use super::{TrailCursor, live_cursor, trail_cursor};
    use crate::trail::{Chase, Style};
    use baud_core::{CursorStyle, Owned, Term};
    use baud_domain::{BytePos, Col, ReadLen, Row, Rows, WinSize};
    use baud_render::effect::Effect;
    use baud_render::{Rect, Shape, Smear, TrailLook};
    use std::time::{Duration, Instant};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 10,
    };
    const CELL: (f32, f32) = (10.0, 20.0);

    /// A 20 px cell's height in points at 2x, below the flash bound's gate.
    const PT: baud_render::Points = baud_render::Points(10.0);

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// The cell at row `r` and column `c`, from zero.
    macro_rules! cell {
        ($r:expr, $c:expr) => {
            (Row::from_index($r), Col::at_index($c))
        };
    }

    /// One of helix's frames, on its alternate screen: text drawn with the
    /// cursor hidden, then the cursor moved to the selection at `at` and
    /// hidden (`baud-core/tests/placed_cursor.rs`).
    fn frame((r, c): (Row, Col)) -> String {
        let (r, c) = (r.index() + 1, c.index() + 1);
        format!("\x1b[1;1Hline 001\x1b[10;1Hstatus\x1b[{r};{c}H\x1b[?25l")
    }

    /// The whole cell a cursor stands on, at `CELL`: what `Renderer::
    /// cursor_rect` gives a block.
    fn block((r, c): (Row, Col)) -> Rect {
        let (w, h) = CELL;
        let (x, y) = (c.index() as f32 * w, r.index() as f32 * h);
        Rect {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        }
    }

    #[test]
    fn a_placed_cursor_is_followed_and_never_drawn() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, frame(cell!(1, 7)).as_bytes());
        assert_eq!(live_cursor(&t), None, "the program hid it");
        let (r, c) = cell!(1, 7);
        assert_eq!(trail_cursor(&t), Some(TrailCursor::Placed(r, c)));
        feed(&mut t, frame(cell!(8, 7)).as_bytes());
        let (r, c) = cell!(8, 7);
        assert_eq!(trail_cursor(&t), Some(TrailCursor::Placed(r, c)), "ge");
        // A hidden cursor no program placed is followed by nothing.
        feed(&mut t, b"\x1b[5;5Hab\x1b[?25l");
        assert_eq!(trail_cursor(&t), None, "a hide after text");
        // Shown, it is the drawn cursor again.
        feed(&mut t, b"\x1b[?25h");
        let (r, c) = cell!(4, 6);
        assert_eq!(live_cursor(&t), Some((r, c)));
        assert_eq!(trail_cursor(&t), Some(TrailCursor::Shown(r, c)));
    }

    /// **The cursor is drawn on the status line while DECSASD selects it**,
    /// on the row below the grid, and on the grid again after.
    #[test]
    fn the_cursor_is_drawn_on_the_status_line_while_it_is_selected() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[3;4H\x1b[2$~\x1b[1$}ab");
        let below = Row::from_index(t.geometry().1.index());
        assert_eq!(live_cursor(&t), Some((below, Col::at_index(2))));
        feed(&mut t, b"\x1b[0$}");
        let (r, c) = cell!(2, 3);
        assert_eq!(live_cursor(&t), Some((r, c)));
    }

    #[test]
    fn a_placement_is_not_followed_while_looking_back() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let lines: Vec<u8> = (0..40)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        feed(&mut t, b"\x1b[3;4H\x1b[?25l");
        let (r, c) = cell!(2, 3);
        assert_eq!(
            trail_cursor(&t),
            Some(TrailCursor::Placed(r, c)),
            "the control: placed on the live screen"
        );
        t.set_window_back(Rows::of_count(3));
        assert_eq!(trail_cursor(&t), None);
    }

    /// **On the alternate screen the window is the live screen**, whatever
    /// the primary's window was left at -- `Term::window_back`'s clamp, which the source
    /// gate trusts `trail_cursor` to ask. A primary left scrolled back when
    /// helix starts must not take its trail away.
    #[test]
    fn a_placement_on_the_alternate_screen_is_followed_whatever_the_primarys_window() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let lines: Vec<u8> = (0..40)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        feed(&mut t, &lines);
        t.set_window_back(Rows::of_count(3));
        assert_eq!(
            trail_cursor(&t),
            None,
            "the control: looking back on the primary"
        );
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, frame(cell!(1, 7)).as_bytes());
        let (r, c) = cell!(1, 7);
        assert_eq!(trail_cursor(&t), Some(TrailCursor::Placed(r, c)));
        assert_eq!(
            t.primary_window_back().index(),
            3,
            "the control: the primary's window is still back"
        );
    }

    #[test]
    fn a_placed_cursor_is_arrived_at_as_a_block_whatever_the_style() {
        let (r, c) = cell!(1, 1);
        for style in [
            CursorStyle::BlinkingBlock,
            CursorStyle::SteadyBlock,
            CursorStyle::BlinkingUnderline,
            CursorStyle::SteadyUnderline,
            CursorStyle::BlinkingBar,
            CursorStyle::SteadyBar,
        ] {
            assert_eq!(
                TrailCursor::Placed(r, c).style(style),
                CursorStyle::SteadyBlock
            );
            assert_eq!(TrailCursor::Shown(r, c).style(style), style, "{style:?}");
        }
    }

    /// **Helix's `ge` and `gg` trail**, through the same chase the app runs:
    /// its frames on the alternate screen, each seen once a frame, the cursor
    /// followed where `trail_cursor` says. The control is the rule before this
    /// change -- the drawn cursor alone -- which follows nothing here, since
    /// these frames never show one (helix shows it for its command line). A
    /// move of a cell or two still snaps.
    #[test]
    fn helix_trails_on_ge_and_gg() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut chase: Chase<Instant> = Chase::new(Style::Smear(Shape::Quad));
        let mut before: Chase<Instant> = Chase::new(Style::Smear(Shape::Quad));
        let mut seen = |t: &Term<'_>, ms: u64| {
            let now = trail_cursor(t).map(|f| block(f.cell()));
            let old = live_cursor(t).map(block);
            (
                chase.frame(now, at(ms), CELL, PT).is_some(),
                before.frame(old, at(ms), CELL, PT).is_some(),
            )
        };
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, frame(cell!(1, 7)).as_bytes());
        assert_eq!(seen(&t, 0), (false, false), "the first sighting");
        // `ge`: a frame at the last line, seen, then seen again once the
        // cursor has rested out the dwell.
        feed(&mut t, frame(cell!(8, 7)).as_bytes());
        assert_eq!(seen(&t, 100), (false, false), "ge first seen");
        assert_eq!(seen(&t, 105), (false, false), "inside the dwell");
        assert_eq!(seen(&t, 120), (true, false), "ge trails");
        assert_eq!(seen(&t, 2000), (false, false), "landed");
        // `gg`, back to the first line.
        feed(&mut t, frame(cell!(1, 7)).as_bytes());
        assert_eq!(seen(&t, 2100), (false, false), "gg first seen");
        assert_eq!(seen(&t, 2120), (true, false), "gg trails");
        assert_eq!(seen(&t, 4000), (false, false), "landed");
        // `l`, a column on: a short move from rest snaps.
        feed(&mut t, frame(cell!(1, 8)).as_bytes());
        let (r, c) = cell!(1, 8);
        assert_eq!(
            trail_cursor(&t),
            Some(TrailCursor::Placed(r, c)),
            "the control: l placed the cursor a column on"
        );
        assert_eq!(seen(&t, 4100), (false, false), "l first seen");
        assert_eq!(seen(&t, 4120), (false, false), "l snaps");
    }

    /// **A fire keeps burning at a placed cursor**, where a hidden one fades
    /// it out: helix hides its cursor for good, so under the old rule a fire
    /// on `ge` could not even light, and a placement is a cursor to burn at.
    #[test]
    fn a_fire_burns_at_a_placed_cursor() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut chase: Chase<Instant> = Chase::new(Style::Effect(Effect::Fire));
        let mut drawn = |t: &Term<'_>, ms: u64| {
            let now = trail_cursor(t).map(|f| block(f.cell()));
            match chase.frame(now, at(ms), CELL, PT) {
                Some(TrailLook::Vfx(v)) => Some(v.emitter.opacity),
                _ => None,
            }
        };
        let mut fire = |t: &Term<'_>, ms: u64| drawn(t, ms) == Some(Smear::OPAQUE);
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, frame(cell!(1, 7)).as_bytes());
        assert!(!fire(&t, 0), "the first sighting");
        feed(&mut t, frame(cell!(8, 7)).as_bytes());
        assert!(!fire(&t, 100), "ge first seen");
        assert!(!fire(&t, 105), "inside the dwell");
        assert!(fire(&t, 120), "ge lights it");
        // The next frame, the cursor hidden and placed where it was: burning.
        feed(&mut t, frame(cell!(8, 7)).as_bytes());
        assert!(fire(&t, 200), "a placed cursor keeps it burning");
        // A hide that places nothing fades it out, as a hidden cursor does.
        feed(&mut t, b"\x1b[5;5Hab\x1b[?25l");
        assert!(!fire(&t, 210), "a cursor not placed fades it");
        assert!(drawn(&t, 220).is_some(), "fading");
        assert_eq!(drawn(&t, 600), None, "out");
    }

    /// **A wheel scroll back in helix moves every trail only down**, on
    /// every path. `tests/trail/helix-wheel.raw` is helix scrolled back a
    /// line at a time from the top of its window: each wheel line scrolls the
    /// view three lines and carries the placed cursor three rows down, until
    /// the bottom margin holds it. Fed a helix frame every 300 ms, the
    /// capture's key delay, with the trail drawn through the chase the app
    /// runs on a 120 Hz link, no corner of what is drawn ever moves up once
    /// the scroll has begun. The fire, then the default, jumped: a flame
    /// that had landed was drawn on the cursor's new cell through the move's
    /// dwell and back on the old one when the move was taken. The controls:
    /// the capture carries the cursor in three-row steps, and under the fire
    /// at least one of those steps begins from a flame that has landed and
    /// still burns, the state the jump came from -- on this schedule the step
    /// from row 11 to row 14; the others begin with the quad in flight or the
    /// flame out.
    #[test]
    fn a_wheel_scroll_back_in_helix_moves_every_trail_only_down() {
        use crate::config::CursorTrail;
        const RAW: &[u8] = include_bytes!("../../tests/trail/helix-wheel.raw");
        const HIDE: &[u8] = b"\x1b[?25l";
        let ws = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let feed = |t: &mut Term<'_>, bytes: &[u8]| {
            for c in bytes.chunks(1024) {
                t.write_at(BytePos::ZERO, c);
                t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
            }
        };
        // Helix's frames: each ends at its hide.
        let mut frames = Vec::new();
        let mut start = 0;
        for i in 0..=RAW.len() - HIDE.len() {
            if &RAW[i..i + HIDE.len()] == HIDE {
                frames.push(&RAW[start..i + HIDE.len()]);
                start = i + HIDE.len();
            }
        }
        let tick = Duration::from_micros(8_333);
        let trails = [
            CursorTrail::Quad,
            CursorTrail::Sweep,
            CursorTrail::Fire,
            CursorTrail::Plasma,
            CursorTrail::Lightning,
            CursorTrail::Ice,
            CursorTrail::Ufo,
            CursorTrail::Orb,
            CursorTrail::Liquid,
        ];
        for trail in trails {
            {
                let case = format!("{trail:?}");
                let mut owned = Owned::new(&ws);
                let mut t = Term::new(owned.storage(), &ws);
                let mut chase: Chase<Instant> = Chase::new(trail.style());
                let t0 = Instant::now();
                let (mut rows, mut wheel, mut from_landed) = (Vec::new(), false, 0);
                let mut last: Option<(TrailLook, Rect)> = None;
                for (k, bytes) in frames.iter().enumerate() {
                    feed(&mut t, bytes);
                    let cell = trail_cursor(&t).map(|f| f.cell());
                    let (r, c) = cell.unwrap_or_else(|| panic!("{case}: frame {k} placed nothing"));
                    assert_eq!(c.index(), 7, "{case}: frame {k}");
                    if rows.last() == Some(&5) && r.index() > 5 {
                        wheel = true;
                    }
                    // A step from a landed flame: the last frame drew the
                    // emitter on the cell the cursor has now left.
                    if let Some((TrailLook::Vfx(v), was)) = last {
                        if rows.last().is_some_and(|&p| r.index() >= p + 3)
                            && v.emitter.corners == was.corners()
                        {
                            from_landed += 1;
                        }
                    }
                    rows.push(r.index());
                    for n in 0..36u32 {
                        let now = t0 + tick * (u32::try_from(k).unwrap() * 36 + n);
                        let rect = trail_cursor(&t).map(|f| block(f.cell()));
                        let look = chase.frame(rect, now, CELL, PT);
                        let corners = |l: &TrailLook| match l {
                            TrailLook::Smear(s) => Some(s.corners),
                            TrailLook::Vfx(v) => Some(v.emitter.corners),
                            TrailLook::Ghost(_) => None,
                        };
                        if let (true, Some(a), Some(b)) = (
                            wheel,
                            last.as_ref().and_then(|(l, _)| corners(l)),
                            look.as_ref().and_then(corners),
                        ) {
                            for (p, q) in a.0.iter().zip(b.0) {
                                assert!(
                                    q.y >= p.y - 1.0,
                                    "{case}: frame {k}, tick {n}: a corner went up from {p:?} \
                                     to {q:?} while helix scrolled back"
                                );
                            }
                        }
                        last = look.zip(rect);
                    }
                }
                assert_eq!(
                    rows,
                    [0, 0, 16, 16, 16, 5, 8, 8, 11, 14, 16, 16, 16],
                    "{case}: the capture's placements -- the start, ge, gt, then the wheel"
                );
                if trail == CursorTrail::Fire {
                    assert!(
                        from_landed >= 1,
                        "{case}: {from_landed} steps began from a landed flame"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod ride_tests {
    //! Ride-scroll in the app: the ghost's ride, the one predicate that
    //! says where the grid slides, and the lift a trail is drawn with. The
    //! chase's ride is `trail.rs`'s; the calls from `pump` and `present` are
    //! AppKit-bound and held by the source gates at the end.
    use super::{GhostRide, SlideDrawn, TrailState, ride_ghost, ride_px, slides, trail_lift};
    use crate::config::{ScrollMode, TrailScroll};
    use baud_core::Screen;
    use baud_domain::{Col, Row, Rows};
    use baud_font::Px;
    use baud_present_macos::Mode;

    const CELL_H: f32 = 18.0;

    /// A ghost from row `from` to row `to`.
    fn st(from: Row, to: Row) -> TrailState {
        TrailState {
            from: (from, Col::at_index(3)),
            to: (to, Col::at_index(9)),
            start: 0.0,
            origin_dy: 0.0,
            ride: GhostRide::Riding(0.0),
            strength: baud_render::Strength::Full,
        }
    }

    /// Where the renderer draws the ghost's top at `p`, before the lift.
    fn top(s: TrailState, p: f32) -> f32 {
        let row = |r: Row| r.index() as f32 * CELL_H;
        (row(s.from.0) + s.origin_dy) * (1.0 - p) + row(s.to.0) * p
    }

    /// At any progress the ghost drawn moves by exactly the scroll, and two
    /// rides are one ride of their sum; the cell it reached does not move.
    #[test]
    fn a_ridden_ghost_moves_by_exactly_the_scroll() {
        for p in [0.0, 0.25, 0.5, 0.9] {
            let s = st(Row::from_index(20), Row::from_index(30));
            let r = ride_ghost(s, -CELL_H, p, CELL_H).expect("still on screen");
            assert!(
                (top(r, p) - (top(s, p) - CELL_H)).abs() < 1e-3,
                "at {p} the ghost rode {}, not one row",
                top(r, p) - top(s, p)
            );
            assert_eq!(r.to, s.to, "the cell it reached is the cursor's");
            let twice = ride_ghost(r, -CELL_H, p, CELL_H).expect("still on screen");
            let once = ride_ghost(s, -2.0 * CELL_H, p, CELL_H).expect("still on screen");
            assert!((twice.origin_dy - once.origin_dy).abs() < 1e-3);
            assert_eq!(twice.ride, once.ride);
        }
    }

    /// **Two rows, then it stops**: the ride that takes the ghost past two
    /// rows is rigid with the text, and after it the ghost rides nothing and
    /// finishes its sweep -- the quad's bound.
    #[test]
    fn a_ghost_rides_two_rows_then_stops() {
        let s = st(Row::from_index(20), Row::from_index(30));
        let two = ride_ghost(s, -2.0 * CELL_H, 0.3, CELL_H).expect("on screen");
        assert_eq!(
            two.ride,
            GhostRide::Riding(-2.0 * CELL_H),
            "two rows is the bound"
        );
        let past = ride_ghost(two, -CELL_H, 0.3, CELL_H).expect("on screen");
        assert_eq!(past.ride, GhostRide::Folded);
        assert!(
            (top(past, 0.3) - (top(s, 0.3) - 3.0 * CELL_H)).abs() < 1e-3,
            "the ride past the bound is rigid with the text"
        );
        let after = ride_ghost(past, -CELL_H, 0.5, CELL_H).expect("on screen");
        assert_eq!(after, past, "and after it the ghost rides nothing");
    }

    /// A landed ghost rides nothing, and one ridden wholly above the row
    /// that leaves the window is dropped -- nothing of it shows -- where one
    /// that still reaches into that row is kept.
    #[test]
    fn a_ghost_goes_when_it_lands_or_leaves_the_window() {
        assert!(
            ride_ghost(
                st(Row::from_index(5), Row::from_index(8)),
                -CELL_H,
                1.0,
                CELL_H
            )
            .is_none()
        );
        let near = st(Row::from_index(1), Row::from_index(8));
        assert!(
            ride_ghost(near, -2.0 * CELL_H, 0.0, CELL_H).is_some(),
            "a ghost reaching into the leaving row is kept"
        );
        assert!(
            ride_ghost(near, -3.0 * CELL_H - 0.5, 0.0, CELL_H).is_none(),
            "a ghost wholly above it is gone"
        );
    }

    /// A retired line rides the trail up a row, the floor is one window
    /// above the top, and a read that retired none rides nothing.
    #[test]
    fn a_retired_line_rides_a_row_up() {
        assert_eq!(
            ride_px(Rows::of_count(1), Rows::of_count(50), CELL_H),
            Some((-CELL_H, -50.0 * CELL_H))
        );
        assert_eq!(
            ride_px(Rows::of_count(3), Rows::of_count(24), CELL_H),
            Some((-3.0 * CELL_H, -24.0 * CELL_H))
        );
        assert_eq!(ride_px(Rows::ZERO, Rows::of_count(50), CELL_H), None);
    }

    /// Only the cell path draws the slide, so only a GPU presenter with the
    /// cell path is `Cells`.
    #[test]
    fn only_the_cell_path_draws_a_slide() {
        let (built, none) = (Some(&()), None::<&()>);
        assert_eq!(SlideDrawn::of(Some(Mode::Gpu), built), SlideDrawn::Cells);
        assert_eq!(SlideDrawn::of(Some(Mode::Gpu), none), SlideDrawn::Other);
        assert_eq!(SlideDrawn::of(Some(Mode::Cpu), built), SlideDrawn::Other);
        assert_eq!(SlideDrawn::of(Some(Mode::Cpu), none), SlideDrawn::Other);
        assert_eq!(SlideDrawn::of(None, built), SlideDrawn::Other);
    }

    /// The grid slides only under smooth scroll, at the live screen, on the
    /// primary, on the cell path and with no status line: every one of the
    /// thirty-two combinations, and the lift is the pan under `ride` and
    /// nothing under `stay`.
    #[test]
    fn the_grid_slides_only_where_every_gate_says_so() {
        for scroll in [ScrollMode::Smooth, ScrollMode::Normal] {
            for window in [0, 3] {
                for screen in [Screen::Normal, Screen::Alternate] {
                    for drawn in [SlideDrawn::Cells, SlideDrawn::Other] {
                        for status in [Rows::ZERO, Rows::of_count(1)] {
                            let want = scroll == ScrollMode::Smooth
                                && window == 0
                                && screen == Screen::Normal
                                && drawn == SlideDrawn::Cells
                                && status == Rows::ZERO;
                            assert_eq!(
                                slides(scroll, Rows::of_count(window), screen, drawn, status),
                                want,
                                "{scroll:?} {window} {screen:?} {drawn:?} {status:?}"
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(trail_lift(TrailScroll::Ride, Px::of(9)), Px::of(9));
        assert_eq!(trail_lift(TrailScroll::Stay, Px::of(9)), Px::of(0));
        assert_eq!(trail_lift(TrailScroll::Ride, Px::of(0)), Px::of(0));
    }

    /// `present` reads the slide's pan once, so the trail's lift, the grid's
    /// pan and the leaving row cannot disagree; and the ride is taken in
    /// one place, after the read's lines reached the slide, on every
    /// whole-screen scroll `trail_rides` names -- outside the slide's gate,
    /// so under `[scroll] mode = "normal"` and on the instance and CPU paths
    /// as well as where the grid slides, two of the cases the ride was built
    /// for on 2026-11-01 (the alternate screen and the status line are
    /// `trail_rides`' own, held in `effects.rs`). `ride_trail` asks no slide,
    /// and rides on the display link's clock the trail's frame reads.
    #[test]
    fn the_pan_is_read_once_and_the_ride_follows_every_whole_screen_retire() {
        const SRC: &str = include_str!("app.rs");
        let pan_read = concat!("self.", "slide.pan()");
        assert_eq!(SRC.matches(pan_read).count(), 1, "one read of the pan");
        let ride = concat!(".chase", ".ride(");
        assert_eq!(SRC.matches(ride).count(), 1, "one ride of the chase");
        let retire = concat!("self.app.slide", ".retire(n);");
        let at = SRC.find(retire).expect("the retire");
        let call = concat!("self.ride", "_trail(n);");
        assert_eq!(SRC.matches(call).count(), 1, "one call of the ride");
        let taken = SRC.find(call).unwrap_or(0);
        assert!(taken > at, "the ride is taken after the slide's retire");
        let between = &SRC[at..taken];
        assert!(
            between.contains(concat!("Ring::Restored(_)", " => {}")),
            "outside the slide's match, and so outside its gate"
        );
        assert!(
            between.contains(concat!("crate::effects::trail", "_rides(")),
            "on the lines trail_rides names"
        );
        let body = &SRC[SRC.find(concat!("fn ride", "_trail(")).expect("ride_trail")..];
        let body = &body[..body.find("\n    }\n").expect("its end")];
        assert!(
            !body.contains(concat!("slides", "(")) && !body.contains(concat!("slide", ".pan()")),
            "ride_trail asks no slide"
        );
        assert!(
            body.contains(concat!("HostTime::of_secs(self.app.", "tick_target)")),
            "the ride is on the display link's clock"
        );
        assert!(body.contains(ride), "and it is the chase's one ride");
        assert!(
            body.contains(concat!(
                "Rows::of_count(usize::from(self.app.grid",
                ".ws_row))"
            )) && body.contains(concat!("ride", "_px(n, rows, cell_h)")),
            "by the lines retired, on the screen's rows"
        );
    }
}

#[cfg(test)]
mod motion_tests {
    //! Reduce Motion: no trail, unless the config names one, and blinking
    //! text held shown. The decisions are pure and gated here;
    //! `poll_motion`, the tick and the launch read are AppKit-bound, held
    //! by the source gates at the end.
    use super::{MotionSource, Owes, motion_change, motion_override, retrail, system_motion};
    use crate::config::{CursorTrail as T, SystemMotion as M, TrailPick as P};
    use crate::trail::Chase;
    use baud_domain::{Col, HostTime, Row};
    use baud_render::Rect;
    use std::cell::Cell;

    /// A 20 px cell's height in points at 2x, below the flash bound's gate.
    const PT: baud_render::Points = baud_render::Points(10.0);

    const ALL: [T; 11] = [
        T::Off,
        T::Ghost,
        T::Sweep,
        T::Quad,
        T::Fire,
        T::Plasma,
        T::Lightning,
        T::Ice,
        T::Ufo,
        T::Orb,
        T::Liquid,
    ];

    fn at(s: f64) -> HostTime {
        HostTime::of_secs(s).expect("a time")
    }

    /// A 10x20 cell at column `c`, row `r`.
    fn cell(c: f32, r: f32) -> Rect {
        Rect {
            left: c * 10.0,
            top: r * 20.0,
            right: c * 10.0 + 10.0,
            bottom: r * 20.0 + 20.0,
        }
    }

    /// Only the default follows the system, both ways; a named trail never
    /// changes, and an unchanged trail is no change.
    #[test]
    fn motion_change_follows_only_the_default() {
        assert_eq!(
            motion_change(P::Default, T::Plasma, M::Reduced),
            Some(T::Off)
        );
        assert_eq!(motion_change(P::Default, T::Off, M::Full), Some(T::Plasma));
        assert_eq!(
            motion_change(P::Default, T::Plasma, M::Full),
            None,
            "unchanged"
        );
        assert_eq!(
            motion_change(P::Default, T::Off, M::Reduced),
            None,
            "unchanged"
        );
        for t in ALL {
            for m in [M::Full, M::Reduced] {
                assert_eq!(motion_change(P::Named(t), t, m), None, "{t:?} under {m:?}");
            }
        }
    }

    /// **A switch mid-fire owes one frame, to clear it, and then none**:
    /// the fire is cut, not left to burn out, and the chase restyled to
    /// the off trail's owes nothing over ten seconds of ticks.
    #[test]
    fn a_switch_mid_fire_owes_one_frame_and_then_none() {
        let mut chase: Chase<HostTime> = Chase::new(T::Fire.style());
        chase.frame(Some(cell(5.0, 3.0)), at(1.0), (10.0, 20.0), PT);
        chase.frame(Some(cell(60.0, 3.0)), at(1.008), (10.0, 20.0), PT);
        assert!(
            chase
                .frame(Some(cell(60.0, 3.0)), at(1.020), (10.0, 20.0), PT)
                .is_some(),
            "the control: a fire is lit"
        );
        let (mut ghost, mut last) = (None, Some((Row::ZERO, Col::ZERO)));
        let owes = retrail(&mut chase, &mut ghost, &mut last, T::Off, Some(at(1.021)));
        assert_eq!(owes, Owes::Frame, "one frame to clear the fire");
        assert_eq!(last, None, "the cursor's last cell is forgotten");
        for k in 0..1200u32 {
            let now = at(1.021 + f64::from(k) / 120.0);
            assert_eq!(chase.owes(now), Owes::Nothing, "tick {k}");
        }
    }

    /// A cell 40 points tall, above the flash bound's gate.
    const ABOVE: baud_render::Points = baud_render::Points(40.0);

    /// The ghost's sweep from `from` to `to` at `t`, on `ledger`, on the
    /// 10x20 cells of [`cell`] `pt` points tall.
    fn sweep(
        ledger: &mut crate::trail::Ledger<HostTime>,
        (from, to): ((usize, usize), (usize, usize)),
        t: f64,
        pt: baud_render::Points,
    ) -> Option<baud_render::Strength> {
        let rc = |(r, c): (usize, usize)| (Row::from_index(r), Col::at_index(c));
        let rect = |(r, c): (Row, Col)| {
            let n = |v: usize| f32::from(u16::try_from(v).unwrap());
            cell(n(c.index()), n(r.index()))
        };
        crate::trail::ghost_sweep(
            ledger,
            Some(rc(from)),
            Some(rc(to)),
            at(t),
            rect,
            ((10.0, 20.0), pt),
        )
        .map(|s| s.strength)
    }

    /// **A switch of trail keeps the second's passes**, both ways: a
    /// plasma's full pass, then a switch to the ghost and its sweep back
    /// over the same row inside the second, is a repeat above the gate; and
    /// a ghost's full sweep, then a switch to the plasma and its move back,
    /// is too. WCAG 2.3.1 counts flashes in "any one second period",
    /// whichever trail drew them. The controls: the same sweep a second
    /// later is full, and below the gate every pass is.
    #[test]
    fn a_switch_of_trail_keeps_the_seconds_passes() {
        use baud_render::Strength::{Full, UnderThreshold};
        for (pt, t, want) in [
            (ABOVE, 1.3, UnderThreshold),
            (ABOVE, 2.1, Full),
            (PT, 1.3, Full),
        ] {
            let mut chase: Chase<HostTime> = Chase::new(T::Plasma.style());
            chase.frame(Some(cell(5.0, 3.0)), at(1.0), (10.0, 20.0), pt);
            chase.frame(Some(cell(60.0, 3.0)), at(1.008), (10.0, 20.0), pt);
            let lit = chase.frame(Some(cell(60.0, 3.0)), at(1.020), (10.0, 20.0), pt);
            let strength = |l: Option<baud_render::TrailLook>| match l {
                Some(baud_render::TrailLook::Vfx(v)) => Some(v.emitter.strength),
                Some(baud_render::TrailLook::Smear(s)) => Some(s.strength),
                _ => None,
            };
            assert_eq!(strength(lit), Some(Full), "the control: a full plasma");
            let (mut ghost, mut last) = (None, None);
            let _ = retrail(&mut chase, &mut ghost, &mut last, T::Ghost, None);
            let back = sweep(chase.ledger(), ((3, 60), (3, 5)), t, pt);
            assert_eq!(
                back,
                Some(want),
                "the ghost after the plasma, {pt:?} at {t}"
            );

            let mut chase: Chase<HostTime> = Chase::new(T::Ghost.style());
            let first = sweep(chase.ledger(), ((3, 5), (3, 60)), 1.0, pt);
            assert_eq!(first, Some(Full), "the control: a full ghost");
            let _ = retrail(&mut chase, &mut ghost, &mut last, T::Plasma, None);
            chase.frame(Some(cell(60.0, 3.0)), at(t - 0.02), (10.0, 20.0), pt);
            chase.frame(Some(cell(5.0, 3.0)), at(t - 0.012), (10.0, 20.0), pt);
            let moved = chase.frame(Some(cell(5.0, 3.0)), at(t), (10.0, 20.0), pt);
            assert_eq!(
                strength(moved),
                Some(want),
                "the plasma after the ghost, {pt:?} at {t}"
            );
        }
    }

    /// **The app weighs the ghost on the chase's ledger**: each call of
    /// the ghost's rule on the app's own state -- `frame_trail`'s, handed
    /// the app's chase -- names the one ledger, and there is such a call.
    #[test]
    fn the_app_weighs_the_ghost_on_the_chases_ledger() {
        const SRC: &str = include_str!("app.rs");
        let bare: String = SRC.split_whitespace().collect();
        // Spelled apart, so this test does not match its own source first.
        let calls: Vec<&str> = bare
            .split(concat!("crate::trail::ghost_", "sweep("))
            .skip(1)
            .filter(|rest| rest.starts_with("self.") || rest.starts_with("chase."))
            .collect();
        assert!(!calls.is_empty(), "the control: the app sweeps the ghost");
        for rest in calls {
            assert!(
                rest.starts_with("chase.ledger(),"),
                "a ghost weighed apart: {}",
                &rest[..40.min(rest.len())]
            );
        }
    }

    /// A switch at rest owes no frame: nothing was drawn.
    #[test]
    fn a_switch_at_rest_owes_no_frame() {
        let mut chase: Chase<HostTime> = Chase::new(T::Fire.style());
        chase.frame(Some(cell(5.0, 3.0)), at(1.0), (10.0, 20.0), PT);
        let (mut ghost, mut last) = (None, None);
        let owes = retrail(&mut chase, &mut ghost, &mut last, T::Off, Some(at(2.0)));
        assert_eq!(owes, Owes::Nothing);
    }

    /// `BAUD_MOTION`: `reduced` and `full` fix it, anything else is the
    /// system's.
    #[test]
    fn the_motion_override_reads_reduced_and_full() {
        assert_eq!(
            motion_override(Some("reduced")),
            MotionSource::Fixed(M::Reduced)
        );
        assert_eq!(motion_override(Some("full")), MotionSource::Fixed(M::Full));
        assert_eq!(motion_override(None), MotionSource::System);
        assert_eq!(motion_override(Some("slow")), MotionSource::System);
    }

    /// A fixed preference never asks the system; the system's is asked once.
    #[test]
    fn a_fixed_motion_never_asks_the_system() {
        let asked = Cell::new(0);
        let read = || {
            asked.set(asked.get() + 1);
            true
        };
        assert_eq!(system_motion(MotionSource::Fixed(M::Full), read), M::Full);
        assert_eq!(asked.get(), 0, "fixed: not asked");
        assert_eq!(system_motion(MotionSource::System, read), M::Reduced);
        assert_eq!(asked.get(), 1, "the system's: asked once");
    }

    /// **The window's facts are read before every feed**, on a wake as on a
    /// tick: `pump` is AppKit-bound, so its source is what can fail.
    #[test]
    fn every_turn_reads_the_window_before_it_feeds() {
        const SRC: &str = include_str!("app.rs");
        let pump = SRC
            .split_once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"))
            .map(|(_, rest)| rest)
            .expect("pump");
        let poll = pump
            .find("self.poll_window();")
            .expect("pump polls the window");
        let tick = pump
            .find(concat!("if let Trigger::", "Tick = trigger {"))
            .expect("the tick arm");
        let arm_end = tick + pump[tick..].find('}').expect("its end");
        assert!(poll > arm_end, "the poll is outside the tick arm");
        let feed = pump.find("crate::reads::paced(").expect("the feed");
        assert!(poll < feed, "the poll comes before the feed");
    }

    /// **The cursor follows the window's key state on every poll**:
    /// `set_focus` hands the focus to `show_focus` before the early return
    /// a window that never had the keyboard would stop at, so one that
    /// starts behind another draws the setting's look. AppKit-bound, so the
    /// source is what can fail.
    #[test]
    fn the_cursor_look_follows_the_key_state_on_every_poll() {
        const SRC: &str = include_str!("app.rs");
        let focus = SRC
            .split_once(concat!(
                "fn set_",
                "focus(&mut self, f: baud_core::Focus) {"
            ))
            .map(|(_, rest)| rest)
            .expect("set_focus");
        let shown = focus
            .find(concat!("self.app.show_", "focus(f);"))
            .expect("set_focus shows the focus");
        let early = focus
            .find(concat!("if self.app.focus == ", "f {"))
            .expect("the early return");
        assert!(shown < early, "shown before the early return");
    }

    /// **The selection follows the window's key state and its
    /// appearance**: `set_focus` checks the unemphasized pair on every
    /// poll, before the early return a window that never had the keyboard
    /// would stop at, and `apply_style` resolves it again before a pinned
    /// palette's early return. AppKit-bound, so the source is what can fail.
    #[test]
    fn the_inactive_selection_follows_the_key_state_and_the_appearance() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart, so this test does not match its own source first.
        let body = |name: &str| {
            SRC.split_once(&format!("fn {name}(&mut self"))
                .map(|(_, rest)| rest.split_once("\n    }\n").map_or(rest, |(b, _)| b))
                .unwrap_or_else(|| panic!("{name}"))
        };
        let focus = body("set_focus");
        let checked = focus
            .find(concat!("self.inactive_", "selection(f);"))
            .expect("set_focus sets the pair");
        let early = focus
            .find("if self.app.focus == f {")
            .expect("the control: set_focus returns early on no change");
        assert!(checked < early, "checked before the early return");
        let style = body("apply_style");
        let resolved = style
            .find(concat!("self.inactive_", "selection(self.app.focus);"))
            .expect("apply_style resolves the pair again");
        let pinned = style
            .find("palette_switch(")
            .expect("the control: the palette's early return");
        assert!(
            resolved < pinned,
            "resolved before a pinned palette returns"
        );
        let set = body(concat!("inactive_", "selection"));
        assert!(
            set.contains("effectiveAppearance()"),
            "under the window's appearance: {set}"
        );
        assert!(set.contains("Pending::Dirty"), "a frame is owed: {set}");
        assert!(
            set.contains("baud_core::Focus::Gained => None,"),
            "none once the window is key: {set}"
        );
    }

    /// **The tick polls the motion preference**: `pump` is AppKit-bound, so
    /// the one thing that can fail on a removed call is its source.
    #[test]
    fn the_tick_polls_the_motion_preference() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart, so this test does not match its own source first.
        let tick = SRC
            .split_once(concat!("if let Trigger::", "Tick = trigger {"))
            .and_then(|(_, rest)| rest.split_once('}'))
            .map(|(arm, _)| arm)
            .expect("the tick arm");
        assert!(tick.contains("self.poll_focus();"), "the control: {tick}");
        assert!(tick.contains("self.poll_motion();"), "{tick}");
    }

    /// **The tick polls the display settings, and the launch reads them
    /// before the first frame**: both are AppKit-bound, so the source is
    /// what can fail.
    #[test]
    fn the_tick_and_the_launch_read_the_display_settings() {
        const SRC: &str = include_str!("app.rs");
        let tick = SRC
            .split_once(concat!("if let Trigger::", "Tick = trigger {"))
            .and_then(|(_, rest)| rest.split_once('}'))
            .map(|(arm, _)| arm)
            .expect("the tick arm");
        assert!(tick.contains("self.poll_motion();"), "the control: {tick}");
        assert!(tick.contains("self.poll_display();"), "{tick}");
        let read = SRC
            .find(concat!("driver.poll_", "display();"))
            .expect("the launch read");
        let first = SRC
            .find(concat!("driver.app.present(&v, ", "Show::Now);"))
            .expect("the first frame");
        assert!(read < first, "the settings are read after the first frame");
    }

    /// **A heard "Show scroll bars" change lays the window out at once**,
    /// either way between the two styles, and only when the style moved:
    /// the same style after a notification moves no column, and with
    /// nothing heard the setting is not read.
    #[test]
    fn a_heard_scroller_change_lays_the_window_out_at_once() {
        use super::{Relayout, Scroller, scroller_change};
        use baud_present_macos::settings::Heard;
        for (drawn, now) in [
            (Scroller::Legacy, Scroller::Overlay),
            (Scroller::Overlay, Scroller::Legacy),
        ] {
            assert_eq!(
                scroller_change(Heard::Changed, drawn, || now),
                Relayout::Now
            );
            assert_eq!(
                scroller_change(Heard::Changed, now, || now),
                Relayout::Nothing,
                "the control: an unmoved style lays nothing out"
            );
            assert_eq!(
                scroller_change(Heard::Unchanged, drawn, || panic!(
                    "read with nothing heard"
                )),
                Relayout::Nothing
            );
        }
    }

    /// **The window observes the settings from launch and the tick takes
    /// what was heard**: both AppKit-bound, so the source is what can fail.
    #[test]
    fn the_tick_follows_the_settings_heard() {
        const SRC: &str = include_str!("app.rs");
        let tick = SRC
            .split_once(concat!("if let Trigger::", "Tick = trigger {"))
            .and_then(|(_, rest)| rest.split_once('}'))
            .map(|(arm, _)| arm)
            .expect("the tick arm");
        assert!(tick.contains("self.poll_display();"), "the control: {tick}");
        assert!(
            tick.contains(concat!("self.follow_", "settings();")),
            "{tick}"
        );
        assert_eq!(
            SRC.matches(concat!(
                "settings: baud_present_macos::settings::Watch::",
                "start(),"
            ))
            .count(),
            1,
            "the driver starts its watch"
        );
        // Dim Flashing Lights: read at launch, and again when heard.
        assert_eq!(
            SRC.matches(concat!(
                "let flashing = baud_present_macos::settings::",
                "flashing();"
            ))
            .count(),
            1,
            "the launch reads Dim Flashing Lights"
        );
        let follow = SRC
            .split_once(concat!("fn follow_", "settings(&mut self) {"))
            .map(|(_, rest)| rest.split_once("\n    }\n").map_or(rest, |(body, _)| body))
            .expect("follow_settings");
        assert!(
            follow.contains("self.settings.take(Setting::DimFlashingLights)")
                && follow.contains(concat!(
                    "self.app.flashing = baud_present_macos::settings::",
                    "flashing();"
                )),
            "{follow}"
        );
        // Animated Images: read at launch, again when heard, and handed to
        // the animation step with the config's setting.
        assert_eq!(
            SRC.matches(concat!(
                "let animated = baud_present_macos::settings::",
                "animated_images();"
            ))
            .count(),
            1,
            "the launch reads Animated Images"
        );
        assert!(
            follow.contains("self.settings.take(Setting::AnimatedImages)")
                && follow.contains(concat!(
                    "self.app.animated = baud_present_macos::settings::",
                    "animated_images();"
                )),
            "{follow}"
        );
    }

    /// **The pump rings a read's bells before anything can return**, and
    /// the shim makes the calls `ring` names. Both are AppKit-bound, so the
    /// source is what can fail: a ring after the held-update or the
    /// clean-frame return would be lost exactly when no frame is drawn -- a
    /// minimized window, where `?1042` matters most.
    #[test]
    fn the_pump_rings_the_bell_before_it_can_return() {
        const SRC: &str = include_str!("app.rs");
        // A line that is only a `//` comment taken out, so a call written in
        // one is not read as code. A comment after code stays, so a `//`
        // inside a string cannot hide what follows it.
        let uncommented = |s: &str| {
            s.lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let bare = |s: &str| s.split_whitespace().collect::<String>();
        // Spelled apart, so this test does not match its own source first.
        const PUMP: &str = concat!("    fn pump(&mut self, ", "trigger: Trigger) {");
        const SHIM_FN: &str = concat!(
            "    fn ring_",
            "bell(&mut self, volume: baud_core::BellVolume) {"
        );
        // Each signature once, so no copy of either -- in a comment, a
        // string or an impl configured out -- is what is read below.
        assert_eq!(SRC.matches(PUMP).count(), 1, "one pump");
        assert_eq!(SRC.matches(SHIM_FN).count(), 1, "one shim");
        // The file without this test: each call the bell makes, once.
        let (head, rest) = SRC
            .split_once(concat!(
                "fn the_pump_rings_the_bell_",
                "before_it_can_return() {"
            ))
            .expect("this test");
        let code = uncommented(&format!(
            "{head}{}",
            rest.split_once("\n    }\n").expect("its end").1
        ));
        for call in [
            ".hear(",
            concat!(".ring_", "bell(volume)"),
            "NSBeep(",
            "requestUserAttention(",
        ] {
            assert_eq!(code.matches(call).count(), 1, "{call}");
        }
        // The bell's raise and `CSI 5 t`'s, the window operation's: the
        // same call, which takes no focus.
        assert_eq!(code.matches("orderFrontRegardless(").count(), 2);
        // `NSBeep` is AppKit's: imported from it, and named nowhere else.
        let kit = SRC
            .split_once("objc2_app_kit::{")
            .and_then(|(_, r)| r.split_once("};"))
            .map(|(names, _)| bare(names))
            .expect("the AppKit import");
        assert!(kit.split(',').any(|n| n == "NSBeep"), "{kit}");
        assert_eq!(code.matches("NSBeep").count(), 2, "imported and called");

        let pump = SRC
            .split_once(PUMP)
            .and_then(|(_, rest)| rest.split_once("\n    fn "))
            .map(|(body, _)| bare(&uncommented(body)))
            .expect("the pump");
        // From the read to the ring, exactly this: nothing between them can
        // return, take the ringer's answer first or put the ring under
        // another condition.
        // The poll's argument and the follow spelled apart, as their own
        // tests count them.
        const READ_TO_RING: &str = concat!(
            r#"crate::reads::paced(
                term,
                pace,
                Instant::now(),
                drained.chunks.iter().map(|chunk| {
                    if let Some(t) = trace.as_mut() { t.fed(&chunk.bytes); }
                    &chunk.bytes[..]
                }),
                |b| {
                    outbox.push(b);
                    answered = Answered::Yes;
                },
                |t| {
                    pasteboard("#,
            r#"clips.poll(t, osc52));
                    resized = honour(widths.poll(t, host), t).or(resized);
                    fonts_asked.extend_from_slice(t.font_requests());
                    t.clear_font_requests();
                    prints.take(t);
                    if t.printer_asked() == baud_core::PrinterAsked::Owed
                        && printing == crate::config::Printing::On
                        && watch.ask(Instant::now(), crate::print::Ask::Now)
                    { printer_list.read(); }
                    asked.extend_from_slice(t.window_requests());
                    t.clear_"#,
            r#"window_requests();
                    posted.extend(t.notices().map(|n| crate::notify::Posted::of(&n, icons)));
                    t.clear_notices();
                },
                hostlog,
            );
            self.app.pty.follow("#,
            r#"self.app.term.input_encoding());
            if let Some(ws) = resized { self.program_resized(ws); }
            if let Some(line) = self.app.clips
                .core_refusals(&self.app.term, self.app.clipboard.budget)
            { crate::warn!("{line}"); }
            if !drained.chunks.is_empty() || fed == crate::reads::Fed::Some {
                self.app.pending = Pending::Dirty;
            }
            if let Rings::Ring(volume) = self.app.ringer
                .hear(&self.app.term, Instant::now(), self.app.interval)
            { self.ring_bell(volume); }"#
        );
        let span = bare(READ_TO_RING);
        let fed = pump.find("crate::reads::paced(").expect("the read");
        assert!(pump[fed..].starts_with(&span), "the read, then the ring");
        let rung = fed + span.len();
        let gate = pump
            .find(&bare("let frame = frame_gate(FrameGate {"))
            .expect("the control: the frame gate");
        let held = pump
            .find(&bare(".observe(&self.app.term, Instant::now()));"))
            .expect("the held update");
        assert!(rung < gate, "before the gate, which can draw nothing");
        assert!(rung < held, "before the held update");

        let shim = SRC
            .split_once(SHIM_FN)
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| uncommented(body))
            .expect("the shim");
        // The whole body, compared with every space taken out of both
        // sides so rustfmt's layout cannot break the match: a call
        // outside its condition, a question in the other slot, a
        // rebinding, an early return or a filtered window each change the
        // text. The raise is `orderFrontRegardless`, which takes no focus,
        // as `XRaiseWindow` takes none.
        const SHIM: &str = r#"
            use crate::bell::{Attention, Key, Miniaturized, Raise, Shown, Sound, Window};
            let window = self.view.window();
            let state = match &window {
                Some(w) => Window::of(
                    if w.isKeyWindow() { Key::Key } else { Key::NotKey },
                    if w.isMiniaturized() { Miniaturized::Minimized }
                    else { Miniaturized::NotMinimized },
                ),
                None => Window::Unfocused,
            };
            let ring = crate::bell::ring(
                self.app.bell_style, self.app.motion, state,
                self.app.term.bell_is_urgent(), self.app.term.pop_on_bell(),
                volume,
            );
            if ring.sound == Sound::Beep { NSBeep(); }
            if ring.shown == Shown::Flash {
                self.app.bell.ring();
                self.app.pending = Pending::Dirty;
            }
            if ring.attention == Attention::Request {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm)
                        .requestUserAttention(NSRequestUserAttentionType::InformationalRequest);
                }
            }
            if ring.raise == Raise::Front {
                if let Some(w) = &window { w.orderFrontRegardless(); }
            }
        "#;
        assert_eq!(bare(&shim), bare(SHIM), "the shim is this, and only this");
    }

    /// **Reduce Motion holds blinking text shown.** Apple's HIG
    /// (`[[source: apple-hig-accessibility]]`): "Be cautious with
    /// fast-moving and blinking animations ... People who are prone to these
    /// effects can turn on the Reduce Motion accessibility setting. When this
    /// setting is active, ensure your app or game responds by reducing
    /// automatic and repetitive animations". WCAG 2.2.2
    /// (`[[source: wcag22]]`) asks that blinking which "starts
    /// automatically" and "lasts more than five seconds" have "a mechanism
    /// for the user to pause, stop, or hide it". Stopped, and stopped
    /// *shown*: hidden would take the application's text away, which is not
    /// reducing an animation, and it is the cursor's rule too -- a steady
    /// caret is a drawn one.
    #[test]
    fn reduce_motion_holds_blinking_text_shown() {
        use crate::effects::{BLINK_HALF, RAPID_HALF, text_blink};
        use baud_render::{Blink, BlinkPhases};
        // Every eighth of a slow half-cycle across ten seconds, every phase
        // of both rates.
        let times = (0..800).map(|i| f64::from(i) * BLINK_HALF / 8.0);
        let full: Vec<BlinkPhases> = times.clone().map(|t| text_blink(t, M::Full)).collect();
        for rate in [|p: &BlinkPhases| p.slow, |p: &BlinkPhases| p.rapid] {
            assert!(
                full.iter().any(|p| rate(p) == Blink::Shown)
                    && full.iter().any(|p| rate(p) == Blink::Hidden),
                "the control: with full motion SGR 5 and SGR 6 blink"
            );
        }
        assert_eq!(text_blink(0.1, M::Full), BlinkPhases::SHOWN);
        assert_eq!(text_blink(BLINK_HALF + 0.1, M::Full).slow, Blink::Hidden);
        assert_eq!(text_blink(RAPID_HALF + 0.1, M::Full).rapid, Blink::Hidden);
        for t in times {
            assert_eq!(text_blink(t, M::Reduced), BlinkPhases::SHOWN, "at {t}");
        }
    }

    /// **Dim Flashing Lights draws each off phase faint, at the same rates**:
    /// across ten seconds of the clock every rate's phase is the one it has
    /// without the setting, a hidden phase drawn faint instead -- ECMA-48's
    /// rates hold and the page changes less at each flip -- and under Reduce
    /// Motion with it the text is steady. Off, the phases are the clock's.
    #[test]
    fn dim_flashing_lights_draws_the_off_phase_faint_at_the_same_rates() {
        use super::dim_off_phases;
        use crate::effects::{BLINK_HALF, text_blink};
        use baud_present_macos::settings::Flashing;
        use baud_render::{Blink, BlinkPhases};
        let mut faint = (0, 0);
        for i in 0..800 {
            let t = f64::from(i) * BLINK_HALF / 8.0;
            let full = text_blink(t, M::Full);
            let dim = dim_off_phases(full, Flashing::Dimmed);
            for (rate, (was, now)) in [(full.slow, dim.slow), (full.rapid, dim.rapid)]
                .into_iter()
                .enumerate()
            {
                let want = match was {
                    Blink::Shown => Blink::Shown,
                    Blink::Hidden | Blink::Faint => Blink::Faint,
                };
                assert_eq!(now, want, "at {t}");
                if now == Blink::Faint {
                    match rate {
                        0 => faint.0 += 1,
                        _ => faint.1 += 1,
                    }
                }
            }
            assert_eq!(dim_off_phases(full, Flashing::Full), full, "at {t}");
            assert_eq!(
                dim_off_phases(text_blink(t, M::Reduced), Flashing::Dimmed),
                BlinkPhases::SHOWN,
                "at {t}"
            );
        }
        assert!(
            faint.0 > 0 && faint.1 > 0,
            "the control: both rates went faint"
        );
    }

    /// **Each rate blinks at its own rate**: across a minute of the display
    /// link's clock, sampled every millisecond, `SGR 5` hides 75 times and
    /// `SGR 6` 150 -- `[[source: ecma48]]` § 8.3.117's "less then 150 per
    /// minute" and "150 per minute or more" -- each shown for half its cycle.
    #[test]
    fn each_blink_rate_flips_at_its_own_rate() {
        use crate::effects::text_blink;
        use baud_render::{Blink, BlinkPhases};
        let phases: Vec<BlinkPhases> = (0..60_000)
            .map(|ms| text_blink(f64::from(ms) / 1000.0, M::Full))
            .collect();
        for (name, rate, want) in [
            (
                "slow",
                (|p: &BlinkPhases| p.slow) as fn(&BlinkPhases) -> Blink,
                75,
            ),
            ("rapid", |p: &BlinkPhases| p.rapid, 150),
        ] {
            let hides = phases
                .windows(2)
                .filter(|w| rate(&w[0]) == Blink::Shown && rate(&w[1]) == Blink::Hidden)
                .count();
            assert_eq!(hides, want, "{name} blinks {hides} times a minute");
            let shown = phases.iter().filter(|p| rate(p) == Blink::Shown).count();
            assert!(
                shown.abs_diff(30_000) <= want,
                "{name} shown {shown} ms of 60,000"
            );
        }
    }

    /// The tick's phase goes through the motion preference: like the poll,
    /// the tick is AppKit-bound, so its source is what can be checked.
    #[test]
    fn the_tick_takes_the_blink_phase_under_the_motion_preference() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart, so this test does not match its own source.
        let call = concat!(
            "let phase = dim_off_phases(text_",
            "blink(target, self.app.motion), self.app.flashing);"
        );
        assert_eq!(SRC.matches(call).count(), 1, "{call}");
        // And a flip owes a frame through the rates on screen, so a slow-only
        // screen does not repaint at the rapid flips.
        let flips = concat!("self.app.blink_present.", "flips(self.app.blink, phase)");
        assert_eq!(SRC.matches(flips).count(), 1, "{flips}");
        assert!(
            !SRC.contains(concat!("let phase = if (target / BLINK_", "HALF)")),
            "the tick computes the phase itself again, past the preference"
        );
    }

    /// **A kitty animation steps on the tick, through `[images]
    /// animation`, the motion preference and Animated Images**, and a frame
    /// changed owes a whole frame: the tick is AppKit-bound, so its source
    /// is what can be checked; the steps themselves are the core's
    /// (`tests/kitty_animation.rs`), the mapping `access.rs`'s and the
    /// Linux turn's (`timed.rs`).
    #[test]
    fn the_tick_steps_kitty_animations_under_the_motion_preference() {
        const SRC: &str = include_str!("app.rs");
        let motion = concat!(
            "crate::access::image_",
            "motion(\n                self.app.image_animation,\n",
            "                self.app.motion,\n",
            "                auto_play(self.app.animated),"
        );
        assert_eq!(SRC.matches(motion).count(), 1, "{motion}");
        let call = concat!("self.app.term.animate_", "images(now, motion)");
        assert_eq!(SRC.matches(call).count(), 1, "{call}");
    }

    /// A ReGIS page's blinking pixels flip with the slow phase the tick
    /// took through the motion preference, and a repaint owes a frame.
    #[test]
    fn the_tick_flips_regis_blink_with_the_slow_phase() {
        const SRC: &str = include_str!("app.rs");
        let call = concat!("set_regis_", "blink(regis_phase(phase.slow))");
        assert_eq!(SRC.matches(call).count(), 1, "{call}");
        use crate::effects::regis_phase;
        use baud_domain::RegisPhase;
        use baud_render::Blink;
        assert_eq!(regis_phase(Blink::Shown), RegisPhase::Shown);
        assert_eq!(regis_phase(Blink::Hidden), RegisPhase::Hidden);
        assert_eq!(regis_phase(Blink::Faint), RegisPhase::Faint);
    }
}

#[cfg(test)]
mod anchor_tests {
    use baud_core::{CacheGeometry, Depth, Owned, Term};
    use baud_domain::{BytePos, ReadLen, Rows, WinSize};

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// `n` numbered lines. A count in a signature is typed, as the audit
    /// asks of every signature here.
    fn lines(n: Rows) -> Vec<u8> {
        (0..n.index())
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect()
    }

    const TALL: WinSize = WinSize {
        ws_col: 20,
        ws_row: 48,
    };
    const SHORT: WinSize = WinSize {
        ws_col: 20,
        ws_row: 24,
    };

    /// **A window on the live screen stays there however far the ring
    /// moved** -- a resize never changes the scroll position. A zoom in
    /// shrinks the grid and the shorter screen retires the rows the cursor
    /// needs; the app once added that retirement to its window whatever the
    /// window was, so from the prompt the screen filled with the prompt
    /// blocks the shell had already written, a banner four times. The core
    /// holds the window now, and the resize leaves a live one live.
    #[test]
    fn a_window_on_the_live_screen_stays_there_however_far_the_ring_moved() {
        let mut owned = Owned::new(&TALL);
        let mut t = Term::new(owned.storage(), &TALL);
        feed(&mut t, &lines(Rows::of_count(448)));
        let before = t.scrollback().len();
        // The zoom-in case: a shorter screen retired twenty-four rows.
        let _ = t.resize(&SHORT);
        assert_eq!(t.scrollback().len(), before + 24, "the control: it retired");
        assert_eq!(t.window_back(), Rows::ZERO);
        // And the zoom-out case, where they came back.
        let _ = t.resize(&TALL);
        assert_eq!(t.scrollback().len(), before, "the control: they came back");
        assert_eq!(t.window_back(), Rows::ZERO);
    }

    /// A window looking back follows the lines it shows as they retire, a
    /// scroll down that takes back past its line brings it to the live
    /// screen, and it is never past what is retained.
    #[test]
    fn a_window_looking_back_follows_the_ring_and_is_bounded_by_it() {
        let ten = Rows::of_count(10);
        let mut owned = Owned::new(&SHORT);
        let mut t = Term::new(owned.storage(), &SHORT);
        feed(&mut t, &lines(Rows::of_count(400)));
        t.set_window_back(ten);
        feed(&mut t, &lines(Rows::of_count(24)));
        assert_eq!(t.window_back().index(), 34);
        t.set_window_back(ten);
        feed(&mut t, b"\x1b[24T");
        assert_eq!(t.window_back().index(), 0);
        // Never past what is retained, and never below the live screen.
        let mut owned = Owned::with_history(&SHORT, &CacheGeometry::UNKNOWN, Depth::of(40));
        let mut t = Term::new(owned.storage(), &SHORT);
        feed(&mut t, &lines(Rows::of_count(100)));
        assert_eq!(t.scrollback().len(), 40, "the control: the ring is full");
        t.set_window_back(ten);
        feed(&mut t, &lines(Rows::of_count(1000)));
        assert_eq!(t.window_back().index(), 40);
        feed(&mut t, b"\x1b[24T\x1b[24T");
        assert_eq!(
            t.scrollback().len(),
            0,
            "the control: the history came back"
        );
        assert_eq!(t.window_back().index(), 0);
        feed(&mut t, &lines(Rows::of_count(30)));
        assert_eq!(t.window_back().index(), 0, "and live stays live");
    }
}

/// `BAUD_TRACE=<prefix>`: every `Term::resize` and every chunk fed to
/// `Term::feed`, in the order the core received them, as `<prefix>.raw`
/// and `<prefix>.steps` -- the two-file form `tests/resize/` already
/// uses, one line per *event*.
///
/// **This exists because a reconstruction is not a recording.** The
/// resize fixtures were captured by running a shell through a separate
/// pty at recorded sizes, feeding after every resize. The app does not
/// do that: AppKit delivers a burst of `setFrameSize:` callbacks between
/// two display-link ticks, each a `term.resize` with nothing fed, while
/// the shell's redraws queue in the pty channel and land together
/// afterwards -- and `Pty::drain` hands the core one chunk per `read(2)`
/// at boundaries no reconstruction can guess. On those fixtures the
/// worst case was one stray prompt; a live screen showed five. A trace
/// *is* the session, so a test replaying it is a test of what happened.
///
/// A resize with no bytes before the next event is a step of length 0;
/// consecutive chunks at one size are consecutive lines at that size.
/// Replaying a resize to an unchanged size is a no-op, so the format
/// round-trips through `Term` exactly.
struct Trace {
    raw: std::fs::File,
    steps: std::fs::File,
    at: WinSize,
}

impl Trace {
    /// Opens at `ws`, the grid the core starts with, and records it as
    /// the first step: a feed before any resize is at *that* size, and
    /// a step of `0x0` -- which the first draft wrote -- replays as a
    /// `Term::resize` that underflows `rows - 1`.
    fn open(ws: &WinSize) -> Option<Trace> {
        let prefix = std::path::PathBuf::from(std::env::var_os("BAUD_TRACE")?);
        let mut raw_path = prefix.clone();
        raw_path.set_extension("raw");
        let mut steps_path = prefix;
        steps_path.set_extension("steps");
        let raw = std::fs::File::create(&raw_path).ok()?;
        let mut steps = std::fs::File::create(&steps_path).ok()?;
        use std::io::Write as _;
        let _ = writeln!(
            steps,
            "# COLSxROWS <bytes fed at that size>; a resize alone is 0"
        );
        let at = WinSize {
            ws_col: ws.ws_col,
            ws_row: ws.ws_row,
        };
        let _ = writeln!(steps, "{}x{} 0", at.ws_col, at.ws_row);
        Some(Trace { raw, steps, at })
    }

    /// A resize the core just took: a zero-length step at the new size.
    fn resized(&mut self, ws: WinSize) {
        use std::io::Write as _;
        self.at = ws;
        let _ = writeln!(self.steps, "{}x{} 0", self.at.ws_col, self.at.ws_row);
    }

    /// A chunk the core was just fed, at the size in force.
    fn fed(&mut self, chunk: &[u8]) {
        use std::io::Write as _;
        let _ = self.raw.write_all(chunk);
        let _ = writeln!(
            self.steps,
            "{}x{} {}",
            self.at.ws_col,
            self.at.ws_row,
            chunk.len()
        );
    }
}

/// A rescale asked for while a frame was in flight.
///
/// Rescaling replaces the atlas and clears the glyph map, and the atlas is
/// the one thing on this path the live draw samples that has no ring: two
/// textures, against an event a keystroke produces. So the *request*
/// waits rather than the frame, and what it waits for is the completion --
/// an event that arrives in about 300 microseconds. Nothing polls and
/// nothing sleeps, and a burst coalesces to its last member.
///
/// A resize used to be here too and is not: the reflow it does is safe
/// against a frame in flight now that the shader reads a copy of the
/// window rather than the ring.
#[derive(Clone, Copy)]
enum Deferred {
    Nothing,
    /// A zoom, in points.
    ///
    /// A resize is no longer here. It waited while the shader read the
    /// ring in place; since D4 it reads a copy, and what the wait cost was
    /// the `SIGWINCH` below it -- see `relayout`.
    Zoom(f32),
    /// A program's change of font (`OSC 50`): the face that leads the
    /// chain after, if it names one, and the size in points.
    Font(Option<crate::fonts::Slot>, f32),
}

/// The change that waits for a frame in flight once `face` at `points`
/// is asked: a face already waiting is kept when the new change names
/// none, so a size step after a program's `OSC 50` does not drop its
/// family.
fn deferred(waiting: Deferred, face: Option<crate::fonts::Slot>, points: f32) -> Deferred {
    let face = face.or(match waiting {
        Deferred::Font(f, _) => f,
        Deferred::Zoom(_) | Deferred::Nothing => None,
    });
    match face {
        Some(_) => Deferred::Font(face, points),
        None => Deferred::Zoom(points),
    }
}

/// What [`Driver::refont`] does with a change of face and size.
#[derive(Clone, Copy)]
enum Refont {
    /// It changes nothing.
    Nothing,
    /// It waits for the frame in flight, in place of what waited.
    Wait(Deferred),
    /// It is performed now, restacked to the face if there is one and at
    /// the size asked.
    Now(Option<crate::fonts::Slot>),
}

/// **What a change to `face` at `points` does**, with `waiting` the change
/// a frame in flight holds back, `reshape` whether one does, `size` the
/// size in force and `leads` the face in force. A face that already leads
/// is dropped only when the change is performed: while another waits,
/// naming the one in force is a change back to it, and the program's
/// latest request is the one that lands.
fn refont_step(
    waiting: Deferred,
    reshape: Option<Reshape>,
    face: Option<crate::fonts::Slot>,
    points: f32,
    size: f32,
    leads: impl Fn(crate::fonts::Slot) -> bool,
) -> Refont {
    if reshape == Some(Reshape::Wait) {
        return Refont::Wait(deferred(waiting, face, points));
    }
    let face = face.filter(|&f| !leads(f));
    match points == size && face.is_none() {
        true => Refont::Nothing,
        false => Refont::Now(face),
    }
}

/// Whether a relayout drew a frame of its own, so a caller that wants one
/// can tell whether it still has to ask.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drew {
    Yes,
    No,
}

/// **What a config read owes once every part is applied**: a frame,
/// unless the relayout or the zoom its fonts asked for built one after the
/// last part was applied (`before` and `after` are `App::frames` either
/// side of them), which then shows every part. Owing one anyway built a
/// second frame of the screen the relayout had just drawn, on the next
/// tick. A frame left owed, because one was in flight, stays owed.
fn read_owes(before: Tally, after: Tally, pending: Pending) -> Pending {
    match after == before {
        true => Pending::Dirty,
        false => pending,
    }
}

/// Whether `present` builds the frame it was asked for now, or leaves it
/// owed (`builds`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Builds {
    Now,
    Owed,
}

/// **A frame asked for while the GPU still reads the last one is owed,
/// not built.** Built, it would sync the styles, walk the glyph map, copy
/// the window and walk the overlay -- or convert, on the instance path --
/// and `draw_cells` would refuse it as `Busy` and throw the lot away. A
/// drag asks on every step (`relayout`), and so do a zoom, a font change
/// and the other events that call `present` directly rather than through
/// the pump, whose own gate already waits. Owed, the frame is built by the
/// completion's wake or the display link's next tick, at the size the
/// window has by then: `pending` stays set, so nothing is lost. The CPU
/// path has no frame in flight.
fn builds(flight: Option<InFlight>) -> Builds {
    match flight {
        Some(InFlight::Busy) => Builds::Owed,
        Some(InFlight::Idle) | None => Builds::Now,
    }
}

/// A key's echo drawn before the pty answered: the cell it stands in on the
/// live screen, the codepoint it shows, the latency record it belongs to
/// when the instrument is on, and when it was made.
struct Prediction {
    row: Row,
    col: Col,
    cp: Codepoint,
    seq: Option<Tally>,
    at: Instant,
    /// The ring's rotation when the cell was chosen. `row` is a *screen*
    /// row, so a read that scrolls moves the cell out from under it: the
    /// judge would then read whatever now sits there, and the draw would
    /// put the glyph on a line the user did not type it on. Only a read
    /// rotates the ring and every read judges, so comparing this against
    /// the rotation the read left is enough to catch it.
    rot: RowRot,
}

/// Whether a predicted echo is owed a frame: the reason a frame is due
/// now, beside a fed echo and the tick.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Guess {
    Owed,
    None,
}

/// Whether a change to the composition is owed a frame: the reason a frame
/// is due now, beside a fed echo and a prediction.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Marked {
    Owed,
    Drawn,
}

/// Whether a key goes to the input manager before baud reads it: **every
/// key while a composition is held**, Command chords aside, which are the
/// menu's. The input manager edits and commits its composition with Return,
/// Delete, Escape and the arrows, and `special` sends those to the child
/// outright -- so with a composition up, Return wrote a newline to the shell
/// and left the kana marked, and Delete erased the shell's line instead of
/// the composition's last character. `[[source: apple-nstextinputclient]]`:
/// the input manager is handed the key, and the client hears back through
/// `insertText`, `setMarkedText` and `doCommandBySelector`.
fn composing_first(key: &KeyPress, composing: &[Codepoint]) -> bool {
    !key.modifiers.command && !composing.is_empty()
}

/// Whether the app may predict: open, or held until the shell's next
/// prompt after a prediction the echo contradicted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hold {
    Open,
    UntilNextPrompt,
}

/// The hold after a read, and the prompt count to remember for the next: a
/// prompt opening on the primary since the last read releases a hold a
/// contradicted prediction set. **A count, not the zone sampled.** A read
/// carrying a whole command -- its `C`, the output and the next `A` --
/// leaves the zone at `Prompt` on both sides, and the hold stayed on until
/// a later prompt; the core counts each turn of the primary's zone to
/// `Prompt` as the bytes come, whatever the reads. **The primary's, not the
/// live screen's**: a full-screen program a line-editor widget opens with
/// no `C` turned the live zone to `Output` and back, and its closing read as
/// a prompt opening.
fn hold_after(term: &Term<'_>, hold: Hold, last: PromptsOpened) -> (Hold, PromptsOpened) {
    let now = term.prompts_opened();
    let hold = match now != last {
        true => Hold::Open,
        false => hold,
    };
    (hold, now)
}

/// A prediction outstanding this long is dropped as contradicted: a shell
/// that did not echo a printable at its prompt in this time is a shell
/// whose widgets are eating keys, and the glyph must not outlive that.
const PREDICT_MAX: Duration = Duration::from_millis(150);

/// Where the echo of the last key sent stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Echo {
    /// A key went to the child and no read has come back since.
    Wanted,
    /// The read after the key was fed and no frame has been committed
    /// since: a frame is owed now.
    Fed,
    /// Drawn, or no key outstanding.
    Drawn,
}

/// Whether a key sent anything to the child. The core answers a key with
/// the bytes its encoding produces, and some chords produce none; the
/// instrument opens a record on every key and closes the `written` leg
/// only on the ones that asked the child for an echo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sent {
    Nothing,
    Bytes,
}

/// **Whose action a reply in the core's buffer answers**, for AA: one the
/// user's key, click, wheel or paste brought, or one the terminal queued
/// on its own -- a focus report, a size report after a resize, a colour
/// scheme's, a held DSR's -- which nobody typed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Asked {
    Typed,
    Unasked,
}

/// Whether a pump's reads queued a reply, which AA shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Answered {
    No,
    Yes,
}

impl Driver {
    /// A key the user pressed: sent to the core and on to the child, with
    /// the instrument's first two stamps around it -- the key event's own
    /// timestamp and this responder running, then the `write(2)` having
    /// returned. The wheel on the alternate screen sends its arrow keys
    /// through `Term::wheel_key` in `Route::Keys`, not here: it is not a
    /// keystroke and is not stamped. `key` is the key whose press it is,
    /// recorded so its release follows (`Downs`); none for what no one key
    /// pressed.
    fn typed(&mut self, e: KeyEvent, key: Option<VirtualKey>) {
        let e = KeyEvent {
            kind: std::mem::take(&mut self.key_kind),
            ..e
        };
        if let Some(code) = key {
            self.downs.pressed(code, e);
        }
        self.app.latency.key(current_event_time, host_now);
        let (text, held) = (e.text, e.mods);
        let typed = predicts(e.code, self.app.term.keypad());
        match self.app.send(e) {
            Sent::Bytes => {
                // A key can start a job that prints nothing (`title.rs`).
                self.derive.owe();
                let act = self.hiding.key(&self.app.term);
                hide_pointer(act);
                self.app.latency.written(host_now);
                self.app.echo = Echo::Wanted;
                if let Some(c) = text.filter(|_| typed) {
                    self.predict(c, held);
                }
            }
            Sent::Nothing => self.app.latency.unsent(),
        }
        // SRM reset: the core drew the key itself, so its frame is owed now,
        // as a read's echo is, and nothing else pumps the loop for it.
        if self.app.term.local_echo() {
            self.app.pending = Pending::Dirty;
            self.app.echo = Echo::Fed;
            self.pump(Trigger::Wake);
        } else if self.app.guess == Guess::Owed {
            // A prediction is owed its frame now, not at the next wake: the
            // keystroke is the event, and nothing else pumps the loop for it.
            self.pump(Trigger::Wake);
        }
    }

    /// A key event that is not a keystroke -- a release, a modifier key
    /// alone -- sent to the core and on to the program, its frame owed now
    /// when SRM reset has the core draw it.
    fn sent_alone(&mut self, e: KeyEvent) {
        let _ = self.app.send(e);
        if self.app.term.local_echo() {
            self.app.pending = Pending::Dirty;
            self.pump(Trigger::Wake);
        }
    }

    /// Row 9: a printable typed at the shell's prompt is drawn before the
    /// pty answers, as an overlay the next read judges. The gates: the
    /// core's prompt zone, the primary screen, the live view, no synchronised update, a
    /// visible cursor that is not in the last column, an ASCII printable,
    /// and no contradiction since the prompt opened. A second key before
    /// the first echo is predicted after the first. An occupied cell is
    /// predicted over, the way Mosh inserts over one: at a prompt the
    /// occupant is the shell's own suggestion of what comes next, and
    /// refusing it refused a third of a sweep's keys.
    fn predict(&mut self, c: Codepoint, held: Modifiers) {
        // Only the cell path draws a prediction. Predicting on the
        // instance or CPU path would stamp a record and commit it on a
        // frame that never drew the glyph -- an instrument reporting a
        // gain nobody can see, which is the probe-that-lies failure this
        // repo keeps finding.
        let draws = matches!(
            (
                self.app.presenter.as_ref().map(Presenter::mode),
                self.app.cell.as_ref()
            ),
            (Some(Mode::Gpu), Some(_))
        );
        if !draws {
            self.app.latency.refused("path");
            return;
        }
        let app = &mut self.app;
        let printable =
            char::from_u32(c.value()).is_some_and(|ch| ch == ' ' || ch.is_ascii_graphic());
        let refused = if is_chord(held) {
            // A control or option chord carries text -- the unmodified
            // character -- and `send` writes the *chord*, so the glyph
            // would not be the byte. Only the input manager's `text` path
            // predicts, whose events carry Shift at most. Shift's capital
            // is the text the base layer sends below `modifyOtherKeys` 2; at
            // 2 and 3 it is `CSI 27 ; 2 ; N ~`, and the next read judges the
            // overlay, as it does for a plain key at level 3 or under
            // kitty's report-all-keys.
            Some("mods")
        } else if app.hold != Hold::Open {
            Some("hold")
        } else if app.term.zone() != Zone::Prompt {
            Some("zone")
        } else if app.term.screen() != Screen::Normal {
            Some("screen")
        } else if app.term.window_back().index() != 0 {
            Some("back")
        } else if app.term.synchronized_output() {
            Some("sync")
        } else if app.term.local_echo() {
            // SRM reset: the terminal drew the key itself, and the child's
            // echo, if it echoes too, is not one to predict.
            Some("srm")
        } else if !app.term.cursor_visible() {
            Some("cursor")
        } else if !printable {
            Some("printable")
        } else {
            None
        };
        if let Some(why) = refused {
            app.latency.refused(why);
            return;
        }
        let view = app.term.view(Rows::ZERO);
        let (row, col) = match app.predictions.last() {
            Some(p) => (p.row, p.col.after()),
            None => app.term.cursor(),
        };
        if col.index() + 1 >= view.cols().index() {
            app.latency.refused("edge");
            return;
        }
        let Some(cell) = view.row(row).get(col.index()) else {
            app.latency.refused("edge");
            return;
        };
        if !cell.content.is_empty() {
            app.latency.noted("over-occupied");
        }
        let seq = app.latency.predicted(host_now);
        app.predictions.push(Prediction {
            row,
            col,
            cp: c,
            seq,
            at: Instant::now(),
            rot: app.term.rotation(),
        });
        app.pending = Pending::Dirty;
        app.guess = Guess::Owed;
    }

    /// Row 9: the read that came is the echo. Predictions are held
    /// against the live grid oldest first: confirmed while each cell holds
    /// its codepoint, and dropped, because the grid draws them now; still
    /// pending at a cell the echo has not reached, which stays empty until
    /// the next read or the expiry; contradicted at a cell that holds
    /// anything else, with every prediction after it, because the chain
    /// stood on it. A contradiction holds prediction until the next prompt
    /// opens.
    fn judge(&mut self) {
        let app = &mut self.app;
        if !app.predictions.is_empty() {
            // A read that scrolled moved every predicted cell out from
            // under its screen row, so nothing here can be judged against
            // the grid any more: drop the lot as contradicted rather than
            // read a line the user did not type on.
            let rot = app.term.rotation();
            if app.predictions.iter().any(|p| p.rot != rot) {
                for p in app.predictions.drain(..) {
                    if let Some(seq) = p.seq {
                        app.latency.judged(seq, Judged::Contradicted);
                    }
                }
                app.hold = Hold::UntilNextPrompt;
                app.guess = Guess::None;
                app.latency.noted("scrolled-away");
            }
        }
        if !app.predictions.is_empty() {
            let view = app.term.view(Rows::ZERO);
            let mut confirmed = 0;
            let mut wrong = false;
            for p in &app.predictions {
                // The base the cell shows: an echo a mark joined holds its
                // slot, and the prediction was its base.
                let content = view
                    .row(p.row)
                    .get(p.col.index())
                    .map(|c| app.term.base_of(*c));
                match content {
                    Some(c) if c == Content::codepoint(p.cp) => confirmed += 1,
                    Some(c) if c.is_empty() => break,
                    _ => {
                        wrong = true;
                        break;
                    }
                }
            }
            let settled = if wrong {
                app.predictions.len()
            } else {
                confirmed
            };
            for (i, p) in app.predictions.drain(..settled).enumerate() {
                if let Some(seq) = p.seq {
                    let verdict = if i < confirmed {
                        Judged::Confirmed
                    } else {
                        Judged::Contradicted
                    };
                    app.latency.judged(seq, verdict);
                }
            }
            if wrong {
                app.hold = Hold::UntilNextPrompt;
            }
            if app.predictions.is_empty() {
                app.guess = Guess::None;
            }
        }
        (app.hold, app.last_prompts) = hold_after(&app.term, app.hold, app.last_prompts);
    }

    /// Row 9: predictions outstanding past `PREDICT_MAX` are dropped as
    /// contradicted, and the frame is owed again to draw the grid without
    /// them.
    fn expire_predictions(&mut self) {
        let app = &mut self.app;
        if app
            .predictions
            .first()
            .is_some_and(|p| p.at.elapsed() > PREDICT_MAX)
        {
            for p in app.predictions.drain(..) {
                if let Some(seq) = p.seq {
                    app.latency.judged(seq, Judged::Contradicted);
                }
            }
            app.hold = Hold::UntilNextPrompt;
            app.guess = Guess::None;
            app.pending = Pending::Dirty;
        }
    }

    /// One turn of the loop, on any event: while a frame is in flight the
    /// bytes stay in the channel, because the grid they would write is
    /// the grid the GPU is reading, and the completion wakes us. Otherwise
    /// feed everything that arrived, then draw -- at once on a tick, and
    /// on a wake only if a frame's worth of time has passed since the last
    /// commit, so a flood draws once a vsync and a keystroke draws now.
    fn pump(&mut self, trigger: Trigger) {
        // The config's files, on every turn and not only on a tick: the
        // display link pauses while the window is covered, which is when
        // the config is edited in another app (`follow_config`).
        self.follow_config();
        // The system's faces for what the last frame met and no face draws.
        self.follow_gaps();
        // A process the title watches forked, exec'd or exited.
        if self.watch.as_ref().map(crate::fg::Watch::take) == Some(crate::fg::Moved::Moved) {
            self.derive.moved();
            self.front.moved();
        }
        if let Trigger::Tick = trigger {
            self.title_check();
            self.notice_turn();
            self.poll_focus();
            self.poll_hover();
            self.poll_motion();
            self.poll_display();
            self.follow_settings();
            // The indicator line's printer field, read at the pace 070-14's
            // 80 ms needs at this display's rate (`print::indicator_every`).
            if self.app.term.status_type() == baud_core::StatusType::Indicator {
                let tick = self.app.interval;
                self.app
                    .read_printers(crate::print::Ask::Indicator { tick });
            }
        }
        // The printer list a read brought back: the core's printer, which
        // answers a held DSR `?15` before the reads behind it are fed.
        self.printers_read();
        // **The window's facts before any byte is fed**, on every turn and
        // not only on a tick: a report answers the window as it is when the
        // program's question is read.
        self.poll_window();
        // **The parser is not held here, and that is research decision D4
        // arriving.** This used to return while a frame was in flight,
        // because the shader read the core's grid where the core writes it,
        // so feeding would have mutated the memory the GPU was sampling.
        // The frame takes its own copy of the window now, so there is
        // nothing for a feed to corrupt -- and the terminal no longer stops
        // reading its child for the length of a GPU round trip, which is
        // the freeze.
        //
        // What still waits for the gate is a *geometry* change, and it has
        // to: `relayout` and `zoom` replace the surfaces and re-raster the
        // atlas, neither of which is ringed. Those are rare and bounded;
        // feeding is neither.
        let busy = self.app.presenter.as_ref().map(Presenter::in_flight) == Some(InFlight::Busy);
        if !busy {
            match core::mem::replace(&mut self.deferred, Deferred::Nothing) {
                Deferred::Nothing => {}
                Deferred::Zoom(points) => self.zoom(points),
                Deferred::Font(face, points) => self.refont(face, points),
            }
        }
        self.apply_style();
        let drained = self.app.pty.drain();
        for chunk in &drained.chunks {
            self.app
                .launch_log
                .read(chunk.at, baud_domain::ReadLen::of(&chunk.bytes));
            // HS: a read as long as the tty hands back, one compare a read.
            if baud_render::lights::full(&chunk.bytes) {
                self.app.activity.seen(Led::Hs, chunk.at);
            }
        }
        if !drained.chunks.is_empty() {
            startup_first("first bytes", &FIRST_BYTES);
            // RD: the latest read's own stamp, taken by the reader as the
            // read returned -- one store a drain, not a byte's work.
            if let Some(last) = drained.chunks.last() {
                self.app.activity.seen(Led::Rd, last.at);
            }
            // The reader stamped each read as it returned: the first one
            // after a key's write is that key's echo.
            self.app
                .latency
                .echoed(drained.chunks.iter().map(|chunk| chunk.at));
        }
        // The selection's text, taken while `?1040` would keep it past its
        // highlight (`keep::KeptText`).
        self.app.kept.take(
            self.app.selection.as_ref(),
            &self.app.term,
            self.app.scrolled,
        );
        let before = Before::watched(
            &mut self.app.term,
            self.app.selection.as_ref(),
            self.app.scrolled,
        );
        let refused = self.app.term.images().refusals;
        // Each read's answers go to the outbox before the next read is fed,
        // or every read after the first few lost its answers to the 256-byte
        // reply buffer (`reads::feed`). The write to the child is still the
        // one `flush_reply` below.
        // A program's `OSC 52` copy is polled after each read, not after the
        // batch: the core keeps only the last write it finished (`clip.rs`).
        // **And its page width at the request**: the read stopped there, the
        // grid takes the columns before the rest is parsed, and the rest of
        // the app follows after the batch (`width.rs`, `program_resized`).
        let (term, trace, outbox, clips, widths, hostlog, pace, prints) = (
            &mut self.app.term,
            &mut self.app.trace,
            &mut self.app.outbox,
            &mut self.app.clips,
            &mut self.app.widths,
            &mut self.app.hostlog,
            &mut self.app.pace,
            &mut self.app.prints,
        );
        let (watch, printer_list, printing) = (
            &mut self.app.watch,
            &self.app.printer_list,
            self.app.printing,
        );
        let osc52 = self.app.clipboard.osc52;
        let host = self.app.sizing.host();
        let mut resized = None;
        // AA, for a reply the feed queued: one store a reply, not a byte's.
        let mut answered = Answered::No;
        // A program's notifications, taken read by read: the core keeps
        // eight a read (`notify.rs`).
        let icons = &mut self.app.notes.icons;
        let mut posted = Vec::new();
        // A program's window operations, drained after each read as its
        // answers are, so the core's bounded queue never fills across a
        // batch, and performed after the bell (`window_requests`).
        let mut asked = Vec::new();
        // And its font changes, performed after the read (`fonts.rs`).
        let mut fonts_asked = Vec::new();
        // And its print jobs, taken after each part of a read as the core
        // finishes them, for the print panel (`print.rs`).
        // **A ReGIS delay holds the reads behind it** until the tick its
        // deadline falls on; every tick calls this, so the held reads go
        // on with no chunk of their own.
        let fed = crate::reads::paced(
            term,
            pace,
            Instant::now(),
            drained.chunks.iter().map(|chunk| {
                if let Some(t) = trace.as_mut() {
                    t.fed(&chunk.bytes);
                }
                &chunk.bytes[..]
            }),
            |b| {
                outbox.push(b);
                answered = Answered::Yes;
            },
            |t| {
                pasteboard(clips.poll(t, osc52));
                resized = honour(widths.poll(t, host), t).or(resized);
                fonts_asked.extend_from_slice(t.font_requests());
                t.clear_font_requests();
                prints.take(t);
                // A DSR `?15` held: the list is read off this thread.
                if t.printer_asked() == baud_core::PrinterAsked::Owed
                    && printing == crate::config::Printing::On
                    && watch.ask(Instant::now(), crate::print::Ask::Now)
                {
                    printer_list.read();
                }
                asked.extend_from_slice(t.window_requests());
                t.clear_window_requests();
                posted.extend(t.notices().map(|n| crate::notify::Posted::of(&n, icons)));
                t.clear_notices();
            },
            hostlog,
        );
        // Before any key is encoded: a key `Term::key` sends as ISO 8859-1
        // must reach a line discipline whose `IUTF8` is clear.
        self.app.pty.follow(self.app.term.input_encoding());
        if let Some(ws) = resized {
            self.program_resized(ws);
        }
        if let Some(line) = self
            .app
            .clips
            .core_refusals(&self.app.term, self.app.clipboard.budget)
        {
            crate::warn!("{line}");
        }
        if !drained.chunks.is_empty() || fed == crate::reads::Fed::Some {
            self.app.pending = Pending::Dirty;
        }
        // **The bells this read brought ring now**, before the early returns
        // for a held update or a busy frame: a sound, a bounce and a raise
        // are not drawn, and a minimized window draws nothing. Once a frame
        // interval (`Ringer`).
        if let Rings::Ring(volume) =
            self.app
                .ringer
                .hear(&self.app.term, Instant::now(), self.app.interval)
        {
            self.ring_bell(volume);
        }
        if answered == Answered::Yes {
            self.app.activity.seen(Led::Aa, host_now());
        }
        // A program's notifications, progress and attention request, shown
        // or not as `notify::plan` says (`notify.rs`), after the bell, as
        // a bell is, for a minimized window draws nothing.
        self.deliver_notices(posted);
        // The status line's frame first, so a program's resize in the same
        // read is measured on the window it leaves (`mac_status_frame`).
        let left = self.status_line();
        self.window_requests(&asked, left);
        self.scrollbar_mode();
        self.font_requests(&fonts_asked);
        self.print_next();
        // An image the store refused is said once a pump, by the limit it
        // hit: the count was a total nothing here read, and yazi's previews
        // went blank against the body limit with no word of why.
        if let Some(line) = crate::host::describe_refusals(
            &refused,
            &self.app.term.images().refusals,
            &self.app.images,
        ) {
            crate::warn!("{line}");
        }
        if !drained.chunks.is_empty() {
            self.app.latency.fed(host_now);
            // The first read after a key's write is that key's echo -- the
            // instrument's rule, and the loop's: it is owed a frame now.
            if self.app.echo == Echo::Wanted {
                self.app.echo = Echo::Fed;
            }
            self.judge();
            // A program's title: only a read can set one, so this is the
            // one place it can have changed. The read owes the derived
            // title a check too, made on the next tick (`title_check`).
            self.derive.owe();
            self.send_title();
            // A program's pointer shape, shown at once while the pointer is
            // over the window; outside it, the next move in shows it.
            self.app.reshape_pointer();
            // Tracking turned on, a new shape or `RIS` shows a pointer a
            // key hid (`hide.rs`).
            hide_pointer(self.hiding.read(&self.app.term));
        }
        // Looking back while output scrolls, the window stays on the lines
        // it was showing: the core holds it there, byte by byte, and sends
        // it live where a scroll down, `RI` at the top or a redraw mark took
        // its line back, or an erasure took the lines (note V, V2).
        let moved = read_moved(&self.app.term, before, resized);
        // The selection names absolute lines; the count that numbers them
        // moves with the screen, by the core's count of the lines the read
        // retired or took back (`numbered`). A surface switch is another
        // ring, and a line the ring recycled is clipped away. A line taken
        // back is renumbered with the rest: the core reaches the watch on
        // every line a take-back erases, so a selection still here holds
        // its text.
        let screen = self.app.term.screen();
        // A move that left nothing of it, a move under side margins, `IL`
        // or `DL` reaching it, a write or an erase on a selected line, a
        // line taken back over one: the selection names text that went
        // elsewhere or is gone. Every other move carries it (`followed`).
        if self.app.selection.is_some() && stale_in_moved_rows(&self.app.term) {
            self.app.selection = None;
            self.app.kept.dropped(&self.app.term);
        }
        let top_was = self.app.scrolled.top(Rows::ZERO);
        self.app.scrolled = match numbered(&self.app.term, before, resized) {
            Ring::Retired(n) => self.app.scrolled.advanced(n),
            Ring::Restored(n) => match self.app.scrolled.retreated(n) {
                Some(s) => s,
                // Taken back past the session's first line: a fresh
                // terminal's scroll down, which no count can number.
                None => {
                    self.app.selection = None;
                    Scrolled::START
                }
            },
        };
        // While `?1040` is reset, a selection the screen's scroll took off
        // the top goes, as xterm's reset disowns it (`scrolled_off_the_top`).
        let tops = (top_was, self.app.scrolled.top(Rows::ZERO));
        if self
            .app
            .selection
            .is_some_and(|s| scrolled_off_the_top(&self.app.term, &s, tops))
        {
            self.app.selection = None;
        }
        followed(&self.app.term, &mut self.app.selection, self.app.scrolled);
        match moved {
            Ring::Retired(n) => {
                // Smooth scroll: output scrolled the live screen up on the
                // primary surface, so start (or extend) a slide. The offset
                // is capped at one row inside `retire`, so a fast burst never
                // builds a backlog -- the whole latency argument. Only at the
                // live screen (`back == 0`): looking back, the window follows
                // the content up and the visible lines do not move. And only
                // when the cell path is drawing (`cell.is_some()`): the pan is
                // applied in the cell branch of `present` alone, so on the
                // instance and CPU paths a slide would force frames the way
                // the tick does while nothing on screen moved. And never when
                // a region retired a line: the rows under it did not move,
                // and a slide in flight is settled rather than left panning
                // them.
                if !whole_screen_scrolled(&self.app.term, before) {
                    self.app.slide.snap();
                } else if self.app.scroll == crate::config::ScrollMode::Smooth
                    && self.app.term.window_back().index() == 0
                    && screen == Screen::Normal
                    && self.app.cell.is_some()
                    && self.app.term.status_rows() == Rows::ZERO
                {
                    self.app.slide.retire(n);
                }
            }
            Ring::Restored(_) => {}
        }
        // A trail in flight rides every line the whole screen retired at the
        // live window, whether the grid slides or jumps, on every path and
        // either screen (`trail_rides`); where it slides, the slide has
        // taken the lines first and the trail is lifted by its pan.
        let rides =
            crate::effects::trail_rides(&self.app.term, before, moved, self.app.trail_scroll);
        if let Some(n) = rides {
            self.ride_trail(n);
        }
        // An erased history has no saved line for a slide to draw above the
        // screen as it pans, and the clamp would draw the screen's own first
        // row there instead -- so a slide settles, whichever way the erase
        // and the lines fell in the read. The selection needs nothing here:
        // the clip below drops what named an erased line, because the lines
        // that refill the history are numbered after them. A selection on
        // the screen a reset cleared is `restart`'s, below.
        if erased(&self.app.term, before) {
            self.app.slide.snap();
        }
        // A switch of surface, or a full reset: what the app keeps over the
        // screen names cells that are gone (note V, V1).
        let (surface, thumb) = restart(
            &self.app.term,
            before,
            Kept {
                selection: &mut self.app.selection,
                predictions: &mut self.app.predictions,
                guess: &mut self.app.guess,
                slide: &mut self.app.slide,
                travel: &mut self.app.travel,
                latency: &mut self.app.latency,
            },
        );
        if thumb == Thumb::Reveal {
            // A programmatic scroll, which AppKit reveals, as `snap_to_live`
            // does for a key.
            self.app.pending = Pending::Dirty;
            if let Some(now) = HostTime::of_secs(self.app.tick_target) {
                self.app.reveal.reveal(now);
            }
        }
        if surface == Surface::Switched {
            // A trail does not cross a switch: the cursor it would come from
            // was the other screen's. The ghost forgets its last cell and any
            // sweep in flight for the same reason; it never did, and a
            // placement made helix reach it from the prompt.
            self.app.chase.snap();
            self.app.last_cursor_cell = None;
            self.app.trail = None;
            self.app.carried.forget();
            // The alternate screen swapped: slide the new one in over the
            // old one, kept now with its cursor hidden, off `tick_target` the
            // way the trail is spawned -- or veil it, under Reduce Motion.
            if self.app.screen_transition == crate::config::ScreenTransition::On {
                let form = switch_form(self.app.term.screen(), self.app.motion);
                if let SwitchForm::Slide(_) = form {
                    self.app.keep_frame();
                }
                self.app.transition = Some(TransitionState {
                    start: self.app.tick_target,
                    form,
                });
            }
        }
        let oldest = oldest(&self.app.term, self.app.scrolled);
        let gone = self
            .app
            .selection
            .as_mut()
            .is_some_and(|s| s.clip(oldest) == Clip::Cleared);
        if gone {
            // Clipped away: `?1040` keeps its text, as the seat does.
            self.app.selection = None;
            self.app.kept.dropped(&self.app.term);
        }
        // Carried or clipped, the text kept is the one it was made with
        // (`keep::KeptText::carried`).
        self.app.kept.carried(self.app.selection.as_ref());
        // The find's matches after the read: the lines it retired dropped,
        // the lines it wrote read again, and the current match selected
        // again when it moved -- after reads a ReGIS delay held too, fed
        // on a tick with no chunk of their own.
        if !drained.chunks.is_empty() || fed == crate::reads::Fed::Some {
            if let Some(f) = self.app.finding.as_mut() {
                if f.refresh(&self.app.term) == crate::find::Moved::Yes {
                    self.app.selection = f.find().current().and_then(|m| {
                        crate::find::selection_of(&self.app.term, self.app.scrolled, m)
                    });
                }
                self.app.pending = Pending::Dirty;
            }
        }
        self.app.flush_reply(Asked::Unasked);
        if drained.stream == Stream::Closed {
            // The child is gone: the window has nothing left to show, once
            // the log has written what this read and the ones before it
            // queued, and it closes; the other windows and their shells go
            // on. Asked on the first turn after the close; the writer
            // wakes the loop when it is done, and the tick looks for the
            // bound (`HostLog::quit`). And once no notification waits for
            // its picture's check (`Delivery::checks`): the checker's wake
            // is a turn, whose delivery above posts it, so a program that
            // ends with a picture shows it.
            let waker = self.app.waker;
            if self.app.hostlog.quit(Instant::now(), move || waker.wake())
                == crate::hostlog::Finished::Yes
                && self.app.notes.checks() == crate::notify::Checks::Done
            {
                self.close_later();
            }
        }
        // Hold the frame while the application is mid synchronised update, so
        // a partial redraw is never shown -- btop wraps its menu redraw in
        // one, and presenting between the `?2026h` and the `?2026l` flickered
        // 89 times over a ten-second run. Not past SYNC_MAX, or an update that
        // opens and stalls -- or a program that dies inside one -- would
        // freeze the window until something closes it. The closing `?2026l`
        // feeds in and wakes us, and this call then presents the finished
        // screen.
        // Each hold is timed from the opening that began it: a close and a
        // reopen in one read restart the clock once a frame was drawn
        // (`UpdateClock`), where an instant kept across them let the new
        // update inherit an outlived one's start, and keep the hold's start
        // while frames are held, so a chain of them is not held for ever.
        // Asked only when a frame is owed.
        let update = (self.app.pending == Pending::Dirty)
            .then(|| self.app.updates.observe(&self.app.term, Instant::now()));
        // **One frame in flight -- D14 -- and now that is all the gate
        // says.** Before the copy landed, `busy` also meant "do not feed";
        // it means only "do not build a second frame" now. Building one
        // would copy the window, resolve the styles and walk the overlay
        // into a set, and `draw_cells` would refuse it and throw the lot
        // away. The completion wakes us and this runs again with the bytes
        // that arrived meanwhile already fed.
        // A key's echo is due the moment it is fed: waiting for the tick
        // here was the one place a keystroke waited for the display link,
        // and it did so exactly when output was already flowing -- a build
        // log scrolling under the typing -- because that is when the last
        // commit is under a frame's time old. A read with no key behind it
        // keeps the gate. With a frame in flight the echo stays owed, and
        // the completion wake draws it: paced by the GPU, not the tick.
        // The decision is `frame_gate`'s, a function of what this turn left.
        let frame = frame_gate(FrameGate {
            pending: self.app.pending,
            update,
            trigger,
            echo: self.app.echo,
            guess: self.app.guess,
            marked: self.app.marked,
            since_commit: self.app.last_commit.elapsed(),
            interval: self.app.interval,
            flight: match busy {
                true => InFlight::Busy,
                false => InFlight::Idle,
            },
        });
        match frame {
            Decided::Clean | Decided::Later => {}
            // Honoured: what the update changes of `?5` is its to hide.
            Decided::Held => self.app.bell.hold(&self.app.term),
            Decided::Now => {
                let view = self.view.clone();
                self.app.present(&view, Show::Event);
            }
        }
    }
}

/// **Command-C or Command-V that the Edit menu did not take**, by what the
/// key types with Command held, `NSEvent.characters`. The menu matches its
/// key equivalent against the key's own character: given the Hebrew, Russian
/// or Greek layout's Command-C, whose characters are `c` and whose own is
/// `ב`, `с` or `ψ`, an `NSMenu` holding Copy on `c` took nothing with the US
/// layout selected, where the US layout's event was taken (a headless probe
/// through `UCKeyTranslate` and `performKeyEquivalent:`); whether AppKit's
/// own localization of an unreachable key equivalent takes it with the
/// Hebrew layout selected was not observed, and a key the menu took never
/// gets here. Those layouts type the Latin letter under Command, so the copy
/// is found by it, as foot finds a binding on a non-Latin layout by the
/// key's Latin keysym. Command alone: Shift, Option or Control held makes
/// another command.
fn menu_missed(
    characters: &str,
    m: macos_liquid_glass::drawable::Modifiers,
) -> Option<&'static str> {
    if m.shift || m.option || m.control {
        return None;
    }
    match characters {
        "c" => Some("copy:"),
        "v" => Some("paste:"),
        _ => None,
    }
}

/// What a polled `OSC 52` write does: the pasteboard written or emptied, or a
/// refusal's one warning said.
fn pasteboard(polled: Option<(crate::clip::Decision<'_>, Option<String>)>) {
    let Some((decision, warn)) = polled else {
        return;
    };
    match decision {
        crate::clip::Decision::Set(text) => {
            if clipboard::write_text(text) == clipboard::Written::Refused {
                crate::warn!("clipboard: the pasteboard refused a program's copy");
            }
        }
        crate::clip::Decision::Clear => clipboard::clear(),
        crate::clip::Decision::Refused(_) => {}
    }
    if let Some(line) = warn {
        crate::warn!("{line}");
    }
}

impl Responder for Driver {
    fn key(&mut self, key: &KeyPress) -> Handled {
        self.key_kind = kind_of(key);
        self.keyed = None;
        self.handed = Handed::NONE;
        let flags = current_event_flags();
        self.downs.saw(flags);
        // A key without Command, or with Shift, says the link chord is not
        // held, and the preview goes with this key rather than after it.
        if (!key.modifiers.command || key.modifiers.shift) && self.app.hover.clear() {
            self.app.pending = Pending::Dirty;
            self.app.reshape_pointer();
        }
        // A key dismisses the hover text over DEC's line, and still goes
        // on to the program (`line_tip.rs`, WCAG 2.2's 1.4.13).
        if self.app.line_tip.dismiss(&self.app.term) == crate::line_tip::Changed::Yes {
            self.app.pending = Pending::Dirty;
        }
        if key.modifiers.command {
            // Menu territory: a key equivalent the menu carries never gets
            // here, and one it does not sends nothing to the child -- but
            // Command-Up and Command-Down, which jump between prompts, and
            // Command-C and Command-V the menu missed (`menu_missed`).
            let m = key.modifiers;
            if !(m.shift || m.option || m.control) {
                match key.key_code {
                    0x7E => self.jump_to_prompt(PromptJump::Previous),
                    0x7D => self.jump_to_prompt(PromptJump::Next),
                    _ => {}
                }
            }
            if let Some(selector) = menu_missed(&key.characters, m) {
                self.command(selector);
            }
            let held = match m.shift || m.option || m.control {
                true => crate::windows::Command::WithOthers,
                false => crate::windows::Command::Alone,
            };
            if crate::windows::mac_chord(&key.characters, held) == Some(crate::windows::Chord::New)
            {
                self.new_window();
            }
            // The find chords, which the menu carries no item for.
            if !(m.option || m.control) {
                let shift = match m.shift {
                    true => crate::find::Shift::Down,
                    false => crate::find::Shift::Up,
                };
                if let Some(chord) = crate::find::command_chord(&key.characters, shift) {
                    self.find_chord(chord);
                }
            }
            return Handled::Consumed;
        }
        // `?1011`, scroll to the bottom on a key press: set at power-on, so a
        // key snaps as it always has, and a program can turn it off -- but
        // not while the find field is open, whose keys are its own and
        // would snap the window away from the match it shows.
        if self.app.finding.is_none() && self.app.term.resource(baud_core::Resource::ScrollOnKey) {
            self.snap_to_live();
        }
        if composing_first(key, &self.app.composing) {
            return Handled::Interpret;
        }
        // The find field takes every key while it is open.
        if self.app.finding.is_some() {
            return match special(key) {
                Some(code) => {
                    let key = crate::find::field_key(code, mods(key.modifiers), None);
                    self.find_key(key);
                    Handled::Consumed
                }
                None if key.modifiers.control || key.modifiers.option => Handled::Consumed,
                // What it types arrives in `text`, through the input manager.
                None => Handled::Interpret,
            };
        }
        let m = held_with_locks(key, flags);
        let e = if let Some(code) = special(key) {
            pressed(code, key, m)
        } else if key.modifiers.control || key.modifiers.option {
            // A control or alt chord: the key's identity, folded by the core.
            let Some(e) = chord(key, m, shift_unshifted(key)) else {
                return Handled::Consumed;
            };
            e
        } else {
            // The input manager will call `text` or `command`, which stamp
            // the keystroke themselves; a record opened here as well would
            // be one no write ever completes. What the key typed and its
            // PC-101 character wait for `text`, for the base layout key.
            self.keyed = Some(Keyed {
                typed: key.characters.clone(),
                base: pc101(key),
                mods: m,
                unshifted: shift_unshifted(key),
            });
            self.handed.key(VirtualKey(key.key_code));
            return Handled::Interpret;
        };
        self.typed(e, Some(VirtualKey(key.key_code)));
        Handled::Consumed
    }

    fn text(&mut self, text: &str) {
        if let Some(f) = self.app.finding.as_mut() {
            f.typed(&self.app.term, &crate::marked::codepoints(text));
            self.find_show();
            return;
        }
        let keyed = self.keyed.take();
        // The key handed to the input manager pressed what it typed when
        // that is one character; a commit of more is no one key's.
        let code = self.handed.text(text);
        // One record per character: a paste through the input manager is
        // one `text` and as many keys as it has characters.
        for e in typed_text(text, keyed.as_ref()) {
            self.typed(e, code);
        }
    }

    /// A key came up. Its release goes to the program when its press did
    /// (`Downs`), and the core sends it only where kitty's event types
    /// asked for releases. A release is not a keystroke: the latency
    /// instrument, the prediction and `?1011`'s snap leave it alone.
    fn key_released(&mut self, key: &KeyPress) {
        self.handed.up(VirtualKey(key.key_code));
        if let Some(e) = self.downs.up(key, current_event_flags()) {
            self.sent_alone(e);
        }
    }

    /// A modifier key went down or came up alone. The link preview follows
    /// Command now, not at the next tick, and the key goes to the program
    /// as kitty's "report all keys" reports it (`modifier_event`): a press
    /// unless the find field has the keys, a release when its press went.
    fn modifiers_changed(&mut self, change: &ModifierChange) {
        if self.app.read_hover_as(link_chord(change.modifiers)) {
            self.app.pending = Pending::Dirty;
            self.pump(Trigger::Wake);
        }
        let kind = match change.pressed {
            true => KeyEventKind::Press,
            false => KeyEventKind::Release,
        };
        let flags = current_event_flags();
        self.downs.saw(flags);
        let Some(e) = modifier_event(change.key, kind, change.modifiers, flags) else {
            return;
        };
        let to = match self.app.finding {
            Some(_) => KeysTo::FindField,
            None => KeysTo::Program,
        };
        if let Some(e) = self.downs.modifier(VirtualKey(change.key_code), e, to) {
            self.sent_alone(e);
        }
    }

    fn command(&mut self, selector: &str) {
        match selector {
            "copy:" => return self.copy(),
            "paste:" => return self.paste(),
            // A step is relative to the size that *will* be in force, not
            // the one that is: while a zoom waits for a frame in flight,
            // `font_size` still names what the renderer is built at, so
            // stepping from it would make every click of a burst aim at
            // the same size and twelve of them arrive as one.
            "zoomIn:" => return self.zoom_step(crate::fonts::Zoom::In),
            "zoomOut:" => return self.zoom_step(crate::fonts::Zoom::Out),
            "zoomActual:" => return self.zoom_step(crate::fonts::Zoom::Reset),
            _ => {}
        }
        // The find field keeps the editing commands from the program; its
        // cancel closes it, as Escape does.
        if self.app.finding.is_some() {
            if selector == "cancelOperation:" {
                self.find_key(crate::find::FieldKey::Close);
            }
            return;
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
        let key = self.handed.command();
        self.typed(KeyEvent::press(code, m), key);
    }

    /// The input manager's composition changed: drawn at the cursor now,
    /// not at the next read, and gone when it is committed or cancelled --
    /// the crate calls this with "" before it hands the committed text to
    /// `text`.
    fn marked_text(&mut self, text: &str) {
        // A key that made the composition something typed nothing
        // (`Handed`).
        self.handed.marked(text);
        let composing = crate::marked::codepoints(text);
        if composing == self.app.composing {
            return;
        }
        self.app.composing = composing;
        self.app.marked = Marked::Owed;
        self.app.pending = Pending::Dirty;
        self.pump(Trigger::Wake);
    }

    /// A press, release or motion, reported to the application when it
    /// asked for that kind: the core keeps the tracking level and the
    /// encoding, refuses what the level does not cover and what X10 cannot
    /// encode, drops a move inside the cell it last reported, and this only
    /// turns the view's pixels into a cell. Every AppKit move and drag comes
    /// here, several to a cell, so the core's cell is what keeps them to one
    /// report a cell.
    fn mouse(&mut self, event: &Mouse) {
        // The left click that brought the window forward does nothing
        // else (`click_of`), judged by the focus polled before it.
        let click = click_of(self.click, event.kind, event.button, self.app.focus);
        self.click = click_held(self.click, event.kind, event.button, click);
        self.poll_focus();
        if click == Click::Activating {
            return;
        }
        // A move shows a pointer a key hid -- AppKit has already, "until
        // the mouse moves" -- and so does a press, any button, as xterm(1)
        // documents `pointerMode`; a crossing shows it but at mode 3
        // (`hide.rs`).
        let act = match event.kind {
            ViewMouseKind::Moved | ViewMouseKind::Dragged => self.hiding.moved(),
            ViewMouseKind::Down => self.hiding.pressed(),
            ViewMouseKind::Entered | ViewMouseKind::Exited => self.hiding.crossed(&self.app.term),
            ViewMouseKind::Up => crate::hide::Act::Nothing,
        };
        hide_pointer(act);
        // The hover text over DEC's line's words and over the lights:
        // where the pointer is, the line's row included, which a press or a
        // report clamps away; a pointer leaving the view is over no cell,
        // and perhaps over a light in the title strip.
        let in_view = match event.kind {
            ViewMouseKind::Exited => InView::Left,
            _ => InView::Yes,
        };
        self.tip_at((event.x, event.y), in_view);
        // Any press is input to the blink's stop -- one the scrollbar, a
        // link or a tracking child takes too -- before those take it, and
        // to a GIF frame waiting for the user (`Term::pointer_pressed`).
        if event.kind == ViewMouseKind::Down {
            self.app.caret.quiet(host_now());
            self.app.term.pointer_pressed();
        }
        // DEC's locator keeps the pointer whatever takes the event: a move,
        // a drag or a crossing here, before the scrollbar and links, so a
        // filter rectangle sees every position; a press and a release once
        // the route has said whose they are, below.
        let (kind, button) = (mouse_kind(event.kind), mouse_button(event.button));
        self.buttons = held_after(self.buttons, kind, button);
        let at = self.locator_under(event.x, event.y);
        let pointer = (event.kind, event.button);
        let half = LocatorHalf::Unrouted;
        let owed = tell_locator(&mut self.app.term, pointer, half, at, self.buttons);
        self.locator_owed(owed);
        self.hover_mouse(event);
        // The pointer over the window, after the preview it may show. On
        // the way out the preview has just taken its hand back, while the
        // pointer was still the window's; past that, what it crosses may
        // set another, so what shows is unknown.
        match event.kind {
            ViewMouseKind::Exited => self.app.pointer_shown = crate::pointer::Shown::Unknown,
            _ => self.app.show_pointer(),
        }
        if self.scrollbar(event) == Took::Yes {
            return;
        }
        // A Cmd press over a link -- an `OSC 8` run or a URI in the text
        // -- is the terminal's, the whole gesture, before the child's
        // tracking, like Shift below, so a child never sees a release
        // without its press.
        if self.links(event) == Took::Yes {
            return;
        }
        // The child's, when it tracks the mouse -- unless Shift is held,
        // which is a selection whatever the child asked for under
        // XTSHIFTESCAPE 0, and for the left button under 1
        // (`shift_overrides`). Decided at the press, so Shift mid-drag
        // cannot hand the release to the other side.
        let held = match event.modifiers.shift {
            true => Shift::Held,
            false => Shift::Up,
        };
        // ReGIS graphics input takes the buttons as a tracking child does
        // (`gesture::pointer_tracking`).
        let tracking = crate::gesture::pointer_tracking(
            self.app.term.mouse_tracking(),
            self.app.term.regis_input(),
        );
        let (to, owner) = pointer_route(
            self.owner,
            kind,
            tracking,
            shift_overrides(held, self.app.term.shift_escape(), kind, button),
        );
        self.owner = owner;
        let half = LocatorHalf::Routed(to);
        let owed = tell_locator(&mut self.app.term, pointer, half, at, self.buttons);
        self.locator_owed(owed);
        if to == To::Select {
            self.select(event);
            return;
        }
        let (row, col, px) = self.pointer_under(event.x, event.y);
        let pressed = button.reported();
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
            px: Some(px),
        };
        // A drag under `?1001` moves the end of the region the program's
        // answer started, and its release ends it, with no report of the move:
        // the frame follows the core's region, not the reply.
        let tracked = self.app.term.highlight();
        if self.app.term.mouse_report(e) {
            self.app.flush_reply(Asked::Typed);
        }
        if self.app.term.highlight() != tracked {
            self.app.pending = Pending::Dirty;
            self.pump(Trigger::Wake);
        }
    }

    /// The wheel moves the window over the history. A trackpad sends
    /// points and a wheel sends lines; both accumulate to whole rows of the
    /// cell height, positive `dy` being "show earlier lines" the way AppKit
    /// delivers it. The alternate screen keeps no history: there the wheel
    /// is the cursor keys while `?1007` is set, which it is until an
    /// application resets it, and nothing after (`wheel::route`).
    fn scroll(&mut self, event: &Scroll) {
        self.app.caret.quiet(host_now());
        if crate::wheel::predominant(event.dx, event.dy) == crate::wheel::Axis::Horizontal {
            self.scroll_across(event);
            return;
        }
        let (_, ch) = self.app.metrics_cell();
        let lines = self.app.travel.lines(match event.precise {
            true => Delta::Points {
                dy: event.dy,
                scale: self.app.scale,
                cell: ch,
            },
            false => Delta::Lines(event.dy),
        });
        // Whole lines, signed; the retained count bounds the window above
        // and the live screen bounds it below.
        let step = lines.get();
        if step == 0 {
            return;
        }
        // An application tracking the mouse gets the wheel as buttons 4
        // and 5, one press a line, at the pointer; one on the alternate
        // screen without tracking gets it as the arrow keys, which is what
        // a pager or an editor there reads; the primary screen without
        // tracking is the window over the history. `wheel::route` decides,
        // where a test can reach it.
        match crate::wheel::route(&self.app.term, mouse_mods(event.modifiers)) {
            Route::Report => {
                let at = self.pointer_under(event.x, event.y);
                // Every report into the outbox as it is queued -- the core's
                // buffer holds 21 of them -- and then one write: what the
                // kernel cannot take now stays owed to the next pump.
                let outbox = &mut self.app.outbox;
                let mods = mouse_mods(event.modifiers);
                crate::wheel::report(&mut self.app.term, lines, at, mods, |b| outbox.push(b));
                self.app.flush_reply(Asked::Typed);
                return;
            }
            Route::Keys => {
                // The wheel's keys, not keystrokes: KAM does not lock them
                // and SRM does not draw them (`Term::wheel_key`).
                for key in crate::wheel::keys(lines) {
                    let outbox = &mut self.app.outbox;
                    crate::reads::input(
                        &mut self.app.term,
                        |t| t.wheel_key(key),
                        |b| outbox.push(b),
                    );
                }
                self.app.caret.input(host_now());
                self.app.flush_reply(Asked::Typed);
                return;
            }
            Route::Nothing => return,
            Route::History => {}
        }
        // A wheel is where the thumb would be revealed, so the scroller the
        // setting asks for is read here as well as at a layout: a changed
        // style re-lays the grid first, and the step lands in the new
        // geometry, whose anchoring `relayout` has already done.
        if scroller_style() != self.app.scroller {
            let _ = self.relayout(self.view.frame().size, self.app.scale);
        }
        let Some(back) = scroll_history(&mut self.app.term, lines) else {
            return;
        };
        // A wheel that moved the window reveals the thumb, as AppKit's
        // does -- the one that lands on the live screen too, which shows it
        // at the bottom of its travel and fades it.
        if let Some(now) = HostTime::of_secs(self.app.tick_target) {
            self.app.reveal.reveal(now);
        }
        // The view left the live screen for history: a slide names rows of
        // the live screen, so settle it. The scrolled-back frame is drawn
        // un-panned regardless (`sliding` checks `back == 0`); this only
        // stops the tick forcing frames for a slide that draws nothing.
        if back.index() != 0 {
            self.app.slide.snap();
        }
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
                if self.app.rescale(px).is_some() {
                    self.app.next_damage = Damage::Full;
                    self.app.pending = Pending::Dirty;
                }
            }
        }
        let _ = self.relayout(size, scale);
    }

    fn frame(&mut self, target: f64) {
        let (began, start) = (host_now(), Instant::now());
        self.expire_predictions();
        // The vsync this tick's work is due at, announced before anything
        // this tick presents so a surface set from here on is bounded by it.
        self.app.latency.announced(|| HostTime::of_secs(target));
        startup_first("first tick", &FIRST_TICK);
        if let (Some(img), Some(mtm)) = (self.dock_icon.take(), MainThreadMarker::new()) {
            macos_liquid_glass::set_application_icon(&NSApplication::sharedApplication(mtm), &img);
            startup_mark("dock icon");
        }
        if self.app.caret_read == Done::No {
            self.app.caret_read = Done::Yes;
            self.read_caret_system();
            startup_mark("caret os");
        }
        if let Some(last) = self.app.last_tick {
            let dt = target - last;
            if dt > 0.001 && dt < 0.1 {
                self.app.interval = Duration::from_secs_f64(dt);
            }
        }
        self.app.last_tick = Some(target);
        self.app.tick_target = target;
        // Blink is driven off the display link's timestamp -- the hardware
        // vsync clock -- so each rate's phase is accurate without a timer of
        // its own. A slow cycle is two BLINK_HALF periods and a rapid one two
        // RAPID_HALF; a rate's flip owes a repaint only when a cell on screen
        // blinks at that rate, so a slow-only screen draws no frame at the
        // rapid flips. Under Reduce Motion both are held shown, so a change
        // of the preference to reduced flips a hidden phase back once and
        // then nothing flips. Under Dim Flashing Lights each off phase is
        // drawn faint rather than hidden, at the same flips.
        let phase = dim_off_phases(text_blink(target, self.app.motion), self.app.flashing);
        if phase != self.app.blink {
            if self.app.blink_present.flips(self.app.blink, phase) {
                self.app.next_damage = Damage::Full;
                self.app.pending = Pending::Dirty;
            }
            self.app.blink = phase;
        }
        // A ReGIS drawing's blinking pixels flip with SGR 5's phase, held
        // shown under Reduce Motion with it; the core repaints them and says
        // so, which owes a frame.
        if self.app.term.set_regis_blink(regis_phase(phase.slow)) == RegisRepainted::Yes {
            self.app.next_damage = Damage::Full;
            self.app.pending = Pending::Dirty;
        }
        // The cursor's blink owes a frame at each flip, and when it stops in
        // the off phase -- focus lost, a steady style, hidden, looked away
        // from -- the frame that shows it (`caret.rs`). Nothing while it does
        // not blink, so an idle steady cursor builds no frame, and nothing
        // while a synchronised update holds the frame (`Caret::tick`).
        if let Some(now) = HostTime::of_secs(target) {
            let blinks = crate::caret::blinks(
                self.app.term.cursor_style(),
                self.app.focus,
                live_cursor(&self.app.term),
            );
            let update = self.app.updates.state(&self.app.term, Instant::now());
            if self.app.caret.tick(now, blinks, update) == Owes::Frame {
                self.app.pending = Pending::Dirty;
            }
        }
        // A flash on screen ends on this clock: the tick whose target
        // reaches its deadline owes the frame that takes it down, and the
        // frame's own plan reads the same target (`present`), so it is one
        // frame, not one a tick until the wall clock catches up.
        if HostTime::of_secs(target).is_some_and(|now| self.app.bell.is_due(now)) {
            self.app.pending = Pending::Dirty;
        }
        // A trail keeps building frames while it sweeps, the way a blink
        // flip does -- but incrementally: the ghost's bounded damage is the
        // renderer's `trail_moved`, so this does not force `Damage::Full`.
        // When the sweep lands the trail clears itself and this stops
        // firing, so an idle screen returns to no frames.
        if self.app.trail.is_some() || self.app.transition.is_some() {
            self.app.pending = Pending::Dirty;
        }
        // **The window is shown to the splash** once a frame has reached
        // the layer and AppKit reports the window on screen and not wholly
        // covered -- `isVisible`, "visible onscreen", and the occlusion
        // state's `.visible`, "at least part of the window is visible"
        // ([[source: apple-nswindow-isvisible]],
        // [[source: apple-nswindow-occlusionstate]]): its clock starts on
        // this tick's frame, which this owes. The window was ordered front
        // with no open animation and its first frame already on its layer
        // (`run`), so on screen is whole on screen (note AJ, sections 13
        // and 15). It waits too for the plasma's pipelines, built on a
        // thread of their own from the window's being shown, so the
        // emerge's first frame never pays for them and nothing before it
        // waits on them (`App::build_plasma`).
        let plasma = self.app.plasma_ready();
        if self.app.splash.waiting() && self.app.layer_shown == Done::Yes && plasma {
            let visible = self.view.window().is_some_and(|w| {
                w.isVisible() && w.occlusionState().contains(NSWindowOcclusionState::Visible)
            });
            if visible {
                startup_first("window visible", &FIRST_VISIBLE);
            }
            if visible && self.app.splash.shown() == crate::splash::Owes::Frame {
                self.app.pending = Pending::Dirty;
                startup_mark("splash shown");
            }
        }
        // The launch splash owes every frame while it plays or fades, and
        // the one a reduced-motion hold ends on.
        if self.app.splash.owes(target) == crate::splash::Owes::Frame {
            self.app.pending = Pending::Dirty;
        }
        // iTerm2's fireworks owe every frame while a show is up, and the
        // one a request starts.
        if self.app.fireworks.owes(&self.app.term) == crate::fireworks::Owes::Frame {
            self.app.pending = Pending::Dirty;
        }
        // The quad owes every frame while it is in flight, and the one a
        // move's dwell runs out on: after a lone keystroke's echo nothing
        // else would draw it (`trail.rs`).
        if HostTime::of_secs(target).is_some_and(|now| self.app.chase.owes(now) == Owes::Frame) {
            self.app.pending = Pending::Dirty;
        }
        // The scrollbar's reveal steps off the same clock, and forces a
        // frame only when the look it gives differs from the one drawn: a
        // fade's frames, and nothing while held, hovered or hidden.
        if let Some(now) = HostTime::of_secs(target) {
            // The hover follows the window as well as the pointer
            // (`Reveal::tick`).
            let back = self.app.term.window_back();
            let drag = self.bar_hold.drag();
            self.app.reveal.tick(now, self.app.pointer, back, drag);
            if self.app.look(now) != self.app.drawn_look {
                self.app.pending = Pending::Dirty;
            }
        }
        // Smooth scroll: advance the slide toward settled off the display
        // link's own clock, at one row per SLIDE_SECS_PER_ROW, and force a
        // frame while it is in flight -- including the tick that settles it
        // to zero, so the last frame is the plain screen. An idle screen's
        // slide is settled, so this forces nothing, the trail's discipline.
        if self.app.slide.active() {
            self.app
                .slide
                .advance(self.app.interval.as_secs_f64() as f32);
            self.app.pending = Pending::Dirty;
        }
        self.app.ticks.bump();
        // A kitty picture placed before its pixels were made: a step a
        // tick (`reads::decode_step`), a frame when it shows, and its reply
        // sent by the pump's flush below.
        if crate::reads::decode_step(&mut self.app.term).1 == crate::reads::Shown::Changed {
            self.app.pending = Pending::Dirty;
        }
        // A kitty animation's next frame, off the display link's clock and
        // held as `[images] animation`, Reduce Motion and Animated Images
        // say (`access::image_motion`): a frame shown in another's place
        // moves no cell word, so the frame is whole.
        if let Some(now) = HostTime::of_secs(target) {
            let motion = crate::access::image_motion(
                self.app.image_animation,
                self.app.motion,
                auto_play(self.app.animated),
            );
            if self.app.term.animate_images(now, motion) == baud_domain::Animated::Changed {
                self.app.next_damage = Damage::Full;
                self.app.pending = Pending::Dirty;
            }
        }
        self.pump(Trigger::Tick);
        // The activity lights, after the pump has stamped this turn's
        // reads and writes: a swap of the view's picture, never a frame.
        self.show_lights(target);
        // Every tick, not every frame: an idle screen builds no frame and
        // is the one this has to be able to report on.
        self.app.report();
        // After the pump: a record shown since the last announcement takes
        // this target as its early bound, and one that already has its
        // early bound takes it as the late one.
        self.app.latency.tick(|| HostTime::of_secs(target));
        startup_first("first tick done", &FIRST_TICK_DONE);
        self.app.launch_log.turn(began, start.elapsed());
    }

    /// The system's light/dark flipped, or the window moved under a
    /// different appearance. Recorded rather than applied: this lands with
    /// the view's borrow held, and swapping the palette repaints.
    fn appearance(&mut self, appearance: Appearance) {
        self.style.0.set(Some(appearance));
    }

    fn presented(&mut self) {
        self.app.layer_shown = Done::Yes;
        self.app.presented();
    }

    fn wake(&mut self) {
        startup_first("run loop", &FIRST_WAKE);
        if self.app.frames.value() > 0
            && self.app.presenter.as_ref().map(Presenter::in_flight) != Some(InFlight::Busy)
        {
            startup_first("first completion", &FIRST_DONE);
        }
        // A New Window the File menu asked of this window
        // (`new_window_from_menu`), opened as its Command-N opens one.
        if ASKED.with(|a| a.take(self.window_id)) {
            self.new_window();
        }
        // The pointer crossing a light in the title strip, heard by its
        // tracking area since the last turn.
        self.lights_crossed();
        let (began, start) = (host_now(), Instant::now());
        self.pump(Trigger::Wake);
        self.app.launch_log.turn(began, start.elapsed());
    }

    /// The window gained or lost focus. Reported to the child only if it
    /// asked with `?1004` -- `Term::focus` decides, the same way
    /// `mouse_report` does -- and vim, which asks on its first line, uses it
    /// to redraw its status line. This was an empty body, and the request the
    /// corpus sends was dropped uncounted, until the sfte survey named it.
    fn focus(&mut self, focus: Focus) {
        // The crate reports the *view's* first-responder status, which
        // changes once -- at `show()`, before this driver is installed --
        // and never on an app switch. `poll_focus` is the source that
        // reaches the driver; this arm keeps the two in agreement.
        self.set_focus(match focus {
            Focus::Gained => baud_core::Focus::Gained,
            Focus::Lost => baud_core::Focus::Lost,
        });
    }

    fn cursor_rect(&self) -> Rect {
        let m = self.app.renderer.metrics();
        // The find field's caret while it is open, which takes the keys.
        let cell = match &self.app.finding {
            Some(f) => crate::marked::field_cell(
                &f.field(
                    &self.app.term,
                    &self.app.composing,
                    &[],
                    baud_render::Caret::End,
                ),
                m.cell_w,
                m.cell_h,
            ),
            None => crate::marked::cursor_cell(&self.app.term, m.cell_w, m.cell_h),
        };
        // In the view's points: the grid begins past the padding and the
        // exit-status strip (`pad_px`).
        let cell = crate::marked::in_frame(cell, self.app.pad_px);
        let [x, y, w, h] = cell.map(|p| f64::from(p.value()) / self.app.scale);
        Rect { x, y, w, h }
    }
}

/// Where the Dock's icon comes from: the bundle's `CFBundleIconFile` when
/// the binary runs inside an `.app`, else set from the embedded image.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DockIcon {
    Bundled,
    Ours,
}

/// baud's icon, rasterised from `assets/logo/` and embedded, so it needs no
/// bundle. The Dock wants a proper app icon -- a rounded square with the mark
/// in its upper left and padding around it, like every other app;
/// the titlebar wants the lockup snug against its box, or the strip
/// makes it tiny -- so the two read from different files (`mark.rs`).
fn icon() -> Option<Retained<NSImage>> {
    // A proper app icon for the Dock: a dark rounded square at ~80% of the
    // canvas with the amber mark in its upper left, 1.2 times the 512
    // icons' -- a prompt at the start of a terminal's first line, big
    // enough to read at the Dock's 32 pt -- and transparent padding
    // around it, the way every other Dock icon sits. The titlebar uses
    // the lockup (`mark.rs`); the Dock wants the box. The SVG
    // itself, not a raster, so NSImage renders it as a vector.
    const BYTES: &[u8] = include_bytes!("../assets/logo/baud-icon-dock.svg");
    macos_liquid_glass::image_from_bytes(BYTES)
}

/// **Below macOS 26 baud refuses, and says why.** baud runs on macOS 26
/// and later: the window is NSGlassEffectView, which exists from 26
/// (`[[source: sdk27-nsglasseffectview-h]]`), and there is no fallback
/// backdrop. The refusal has three layers. Launch Services refuses the
/// bundle, from Info.plist's `LSMinimumSystemVersion` of 26.0. A bare
/// `baud` run from a shell meets no such check -- macOS 13's dyld and
/// kernel do not refuse on the binary's `minos`
/// (`[[source: dyld-1042-1-loader]]`, `[[source: dyld-1066-10-loader]]`,
/// `[[source: xnu-8796-mach-loader]]`) -- so it reaches this, which
/// prints `GlassError`'s message on stderr and exits 1 where it used to
/// panic on `.expect("glass")`. It runs first in [`run`], ahead of the
/// display link and `-[NSApplication activate]` (macOS 14:
/// `[[source: sdk27-nsview-h]]`, `[[source: sdk27-nsapplication-h]]`) and
/// ahead of the shell's spawn, so nothing is started that it would then
/// abandon.
fn refusal(glass: Glass) -> Option<String> {
    match glass {
        Glass::Present => None,
        Glass::Absent => Some(refusal_message(&GlassError::Unsupported)),
    }
}

/// Whether this system has `NSGlassEffectView`, Liquid Glass. The
/// launch still asks for it under the standard material: macOS 26 is the
/// release baud is built and measured on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Glass {
    Present,
    Absent,
}

impl Glass {
    /// The crate's runtime class lookup, the condition
    /// `GlassSurface::new` itself fails on.
    fn of_system() -> Glass {
        if macos_liquid_glass::glass::is_supported() {
            Glass::Present
        } else {
            Glass::Absent
        }
    }
}

/// The line a refusal prints: the crate's own reason, which names the
/// release ("Liquid Glass requires macOS 26 or later"). It names Liquid
/// Glass even when the configured body is the standard material, because
/// the launch check still requires macOS 26 for either.
fn refusal_message(err: &GlassError) -> String {
    format!("baud: {err}")
}

/// Print a refusal on stderr and exit 1: the one way out for a system
/// without the glass, at the check and at the construction both.
fn refuse(message: String) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

#[cfg(test)]
mod refusal_tests {
    use super::{Glass, refusal};

    #[test]
    fn a_system_without_the_glass_is_refused_with_the_release_named() {
        let message = refusal(Glass::Absent).expect("refused");
        assert!(message.starts_with("baud: "), "{message}");
        assert!(message.contains("macOS 26 or later"), "{message}");
        assert_eq!(refusal(Glass::Present), None);
    }
}

pub fn run() {
    if let Some(message) = refusal(Glass::of_system()) {
        refuse(message);
    }
    let mtm = MainThreadMarker::new().expect("run() on the main thread");
    let size = WINDOW_SIZE;
    // What the grid needs before a window exists: the display's scale, and
    // the titlebar's height -- the window is FullSizeContentView, so its
    // content spans the frame behind the transparent titlebar and the
    // traffic lights, and the terminal leaves that strip so row 0 clears
    // the lights. Both are class-level answers, so the child shell can be
    // spawned at its real grid before AppKit starts, and its own startup
    // overlaps AppKit's rather than following it. The scale is not a cheap
    // answer: it is the first CoreGraphics call, the window server
    // connection, 23.6 ms on this box, and the `scale` mark says so. Row 3
    // of note N spawned the shell on a predicted scale ahead of it and
    // gained 8 ms on the prompt, under the 20 ms floor; the arm is the
    // commit this one reverts.
    let early_scale = crate::pty::display_scale();
    startup_mark("scale");
    let inset = predicted_inset(mtm, size);
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
    let shell = Shell::start(scale, size, inset, None);
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // **A Command-N window is a window of its own, never a tab**
    // (`windows.rs`): the window crate builds each one refusing tabs,
    // and the app opts out of automatic tabbing as a whole before the
    // first is made.
    NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
    startup_mark("nsapp");
    // Which AppKit these numbers were taken against -- and the reference
    // that keeps AppKit linked under the dead-strip `build.rs` asks for.
    startup_note(&format!("appkit {}", macos_liquid_glass::appkit_version()));
    let icon = icon();
    startup_mark("icon");
    // The Dock icon comes from the bundle when there is one: `CFBundleIconFile`
    // in the main bundle's Info.plist is the Dock's own source, and setting
    // an image on top of it was 10-13 ms of vImage on the main thread at the
    // first tick for a picture the Dock already had. A bare binary has no
    // bundle and keeps setting it, or the Dock would show a blank tile.
    let dock = match NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundleIconFile"))
    {
        Some(_) => DockIcon::Bundled,
        None => DockIcon::Ours,
    };
    startup_note(match dock {
        DockIcon::Bundled => "dock icon: the bundle's",
        DockIcon::Ours => "dock icon: set from the binary",
    });
    let dock_icon = match dock {
        DockIcon::Bundled => None,
        DockIcon::Ours => icon,
    };
    let opening = Opening::new(mtm, size, scale, inset, shell, dock_icon);
    // The menu before the run loop: key equivalents need it from the first
    // event, and nothing on screen waits for it.
    macos_liquid_glass::menu::install_main_menu(&ns_app, mtm, "baud");
    // baud > About baud, which the window crate's menu does not have. The
    // item's target is weak in AppKit's hands, so it is kept for the process.
    if let Some(target) =
        baud_present_macos::about::add_menu_item(&ns_app, mtm, "About baud", move || {
            crate::about::open(mtm);
        })
    {
        core::mem::forget(target);
    }
    // File > New Window reaches the app through its delegate, with a window
    // or without (`new_window_from_menu`). Kept for the process:
    // `NSApplication.delegate` holds it weakly.
    let delegate = macos_liquid_glass::app::AppDelegate::install(&ns_app, mtm);
    delegate.on_new_window(new_window_from_menu);
    core::mem::forget(delegate);
    startup_mark("menu");
    // **The window is shown once the app has finished launching, with its
    // first frame already on its layer**, and the launch logo starts on the
    // display link's first tick after: `finishLaunching`, which activates
    // the app, runs inside `run()` before the event loop
    // ([[source: apple-nsapplication-finishlaunching]],
    // [[source: apple-nsapplication-didfinishlaunchingnotification]]), and
    // the display link ticks only for a view on a display
    // ([[source: sdk27-nsview-h]]), so a window shown before `run()` stood
    // on screen through all of it, empty and then without its logo (note
    // AJ, section 16). Everything below keeps the order it had: the first
    // frame, the window, its titlebar icon, the glass, the style observer,
    // first responder, and the responder last, so nothing here re-enters a
    // responder holding the view's borrow.
    baud_present_macos::launch::when_launched(move || {
        startup_mark("launched");
        opening.show(mtm, scale, None);
    });
    ns_app.activate();
    startup_mark("activated");
    ns_app.run();
}

/// The size a window opens at, in points.
const WINDOW_SIZE: NSSize = NSSize::new(960.0, 600.0);

/// **Where a window opens, before it is shown** (`windows::SCREEN`):
/// centred by AppKit on the screen with the key window, or, under
/// `BAUD_SCREEN=primary`, centred in the primary screen's visible frame,
/// the first of `NSScreen.screens` (`[[source: apple-nsscreen-screens]]`).
/// A window opened from another is cascaded from it after this, in `show`.
fn open_on_screen(window: &GlassWindow, mtm: MainThreadMarker) {
    let on = match crate::windows::screen(std::env::var_os(crate::windows::SCREEN).as_deref()) {
        Ok(on) => on,
        Err(e) => {
            crate::warn!("{e}");
            crate::windows::Screen::KeyWindow
        }
    };
    let primary = match on {
        crate::windows::Screen::Primary => NSScreen::screens(mtm)
            .firstObject()
            .map(|s| s.visibleFrame()),
        crate::windows::Screen::KeyWindow => None,
    };
    match primary {
        Some(v) => {
            let f = window.ns_window().frame();
            let (x, y) = crate::windows::centred(
                (f.size.width, f.size.height),
                (v.origin.x, v.origin.y, v.size.width, v.size.height),
            );
            window.set_origin(NSPoint::new(x, y));
        }
        None => window.center(),
    }
}

/// The titlebar's height before a window exists: the window is
/// FullSizeContentView, so its content spans the frame behind the
/// transparent titlebar, and the terminal leaves that strip so row 0
/// clears the traffic lights. A class-level answer; `Opening::new` checks
/// it against the window once there is one.
fn predicted_inset(mtm: MainThreadMarker, size: NSSize) -> CGFloat {
    let content = NSRect::new(NSPoint::new(0.0, 0.0), size);
    let frame =
        NSWindow::frameRectForContentRect_styleMask(content, NSWindowStyleMask::Titled, mtm);
    (frame.size.height - size.height).max(0.0)
}

/// **What a window needs before it exists**: the boot -- the config, the
/// faces, the renderer -- the grid it opens at, and its shell, spawned at
/// that grid, with the threads that start after the shell (`Shell::start`).
struct Shell {
    boot: Boot,
    reload_from: Option<(crate::config::Config, crate::reload::Watch<Instant>)>,
    notice: crate::live::Notice,
    grid: Option<WinSize>,
    ws: WinSize,
    cw: f64,
    ch: f64,
    strip: baud_font::Px,
    late: Arc<OnceLock<Waker>>,
    pty: Pty,
    gpu_probe: std::thread::JoinHandle<Option<Gpu>>,
    hosts: Hosts,
    watch: Option<crate::fg::Watch>,
}

impl Shell {
    /// The boot and the shell, at `scale` for a window of `size` whose
    /// titlebar is `inset` high; the shell starts in `at` when it is given
    /// (`windows::inherited`).
    fn start(scale: CGFloat, size: NSSize, inset: CGFloat, at: Option<&std::path::Path>) -> Shell {
        let mut boot = Boot::prepare(scale);
        // The config's watch and notice, for the driver (`reload.rs`).
        let reload_from = boot.reload.take();
        let notice = boot.notice.clone();
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
        // The exit-status strip takes its column from the start, so the child
        // is spawned at the grid it keeps.
        let strip = boot.renderer.strip();
        // And the activity lights' own line its pixels, when it has one.
        let line = f64::from(boot.renderer.lights_height().value()) / scale;
        let ws = grid.as_ref().map_or_else(
            || {
                // The config's notice row, when the launch has one, is the
                // window's, as the status line's is (`Driver::relayout`).
                crate::width::less_status(
                    grid_for(
                        NSSize::new(
                            (size.width - 2.0 * padding - f64::from(strip.value()) / scale)
                                .max(0.0),
                            (size.height - inset - 2.0 * padding - line).max(0.0),
                        ),
                        scale,
                        (cw, ch),
                    ),
                    boot.renderer.notice_rows(),
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
        let cell = boot.renderer.metrics();
        let pty = Pty::spawn(&ws, cell.cell_w, cell.cell_h, at, &boot.shell, move || {
            if let Some(w) = wake_late.get() {
                w.wake();
            }
        })
        .expect("pty");
        startup_mark("pty");
        // **The launch's threads after the shell** (note AJ section 20):
        // `fork` takes dyld's `dlopen` lock and then runs each `pthread_atfork`
        // handler, libinfo's mDNS one waiting out a lookup in flight
        // ([[source: apple-libsystem-init-c]], [[source: apple-libinfo-mdns-module-c]]).
        // With the machine's names read first, `hostName`'s lookup held the
        // shell's start 3.8-36.9 ms in probes and the GPU's thread behind
        // it; the GPU's probe loads its driver. Started here, neither is in
        // either at the fork, and the probe starts 2-3 ms later.
        let gpu_probe = Boot::start_probe(boot.request);
        // This machine's names, read while the launch goes on (`Hosts`).
        let hosts = Hosts::start();
        startup_mark("hosts");
        // The process events the derived title is checked on (`fg::Watch`),
        // waking the main thread as the pty's reader does.
        let wake_watch = Arc::clone(&late);
        let watch = crate::fg::Watch::start(move || {
            if let Some(w) = wake_watch.get() {
                w.wake();
            }
        });
        Shell {
            boot,
            reload_from,
            notice,
            grid,
            ws,
            cw,
            ch,
            strip,
            late,
            pty,
            gpu_probe,
            hosts,
            watch,
        }
    }
}

/// **A window built and laid out, its first frame not yet drawn**: what
/// `show` puts on screen.
struct Opening {
    driver: Driver,
    view: Retained<DrawableView>,
    window: GlassWindow,
    glass: Body,
    container: Retained<NSView>,
    inset: CGFloat,
}

impl Opening {
    /// The window, its glass, its view and its driver for `shell`, laid
    /// out at the first frame's geometry. `dock_icon` is the image the
    /// first tick sets on the Dock, which only the launch's window sets.
    fn new(
        mtm: MainThreadMarker,
        size: NSSize,
        scale: CGFloat,
        inset: CGFloat,
        shell: Shell,
        dock_icon: Option<Retained<NSImage>>,
    ) -> Opening {
        let Shell {
            boot,
            reload_from,
            notice,
            grid,
            ws,
            cw,
            ch,
            strip,
            late,
            pty,
            gpu_probe,
            hosts,
            watch,
        } = shell;
        let padding = boot.padding;
        let theme_choice = boot.theme;
        let glass_cfg = boot.glass;
        let floor = boot.floor;
        let halves = boot.halves;
        let window = GlassWindow::new(mtm, size, "baud");
        startup_mark("glass window");
        // The inset predicted above, checked against the window that exists
        // now; the window's own value is what the layout uses.
        let measured = crate::strip::inset_of(window.ns_window());
        if (measured - inset).abs() > 0.5 {
            startup_note(&format!(
                "titlebar inset: predicted {inset:.1}, measured {measured:.1}"
            ));
        }
        let inset = measured;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
        // **The body is the config's, and it is chosen once.** `Standard`, the
        // default, is `.underWindowBackground`: the HIG puts "Standard
        // materials" behind "app backgrounds" and keeps Liquid Glass out of the
        // content layer ([[source: apple-hig-materials]]; note AT), the default
        // since 2026-10-18. `Regular` is the glass the HIG gives surfaces with
        // "a significant amount of text"; `Clear` the more transparent glass.
        // Unlike the dim, the tint and the shadow, the body does not change
        // with the appearance -- the standard material follows the window's
        // appearance and active state itself -- so it is a constructor argument
        // and nothing else.
        let glass = Body::of(mtm, frame, glass_cfg.style);
        // Tinted before it is shown rather than after: the surface is not the
        // window's content until the first frame is on the layer, so doing it
        // here costs nothing and there is no frame in which an untinted
        // material is visible.
        apply_glass(&glass, &window, &glass_cfg, glass.appearance(&window));
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
            // A legacy scroller's gutter is beside the grid, so the window is
            // that much wider; `relayout` then gives it the same gutter back.
            let bar = gutter(scroller_style(), crate::reveal::widths(Scale(scale)));
            let beside = f64::from(bar.value().saturating_add(strip.value()));
            let wanted = NSSize::new(
                f64::from(ws.ws_col) * cw / scale + 2.0 * padding + beside / scale,
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
            gpu_probe,
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
        crate::strip::apply(&view, &container, inset);
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
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
        open_on_screen(&window, mtm);
        startup_mark("centered");
        // The cell the observer writes and `pump` reads; cloned into both.
        let style = PendingStyle::default();
        let hiding = crate::hide::Hiding::new(&probe.term);
        let mut driver = Driver {
            app: probe,
            view: view.clone(),
            dock_icon,
            held: None,
            buttons: LocatorButtons::NONE,
            key_kind: KeyEventKind::Press,
            keyed: None,
            handed: Handed::NONE,
            downs: Downs::NONE,
            click: Click::Ours,
            armed: None,
            bar_hold: crate::bar::Hold::FREE,
            owner: Owner::Free,
            deferred: Deferred::Nothing,
            style: style.clone(),
            theme_choice,
            glass_cfg,
            themes: Themes::of(&glass_cfg, theme_choice, floor, &halves),
            display: Display::default(),
            settings: baud_present_macos::settings::Watch::start(),
            window_facts: baud_core::WindowFacts::UNKNOWN,
            status_took: crate::winops::StatusChange::Resize,
            saved_frame: None,
            performed: crate::winops::Performed::new(),
            palette_for: None,
            glass: glass.clone(),
            window: window.clone(),
            titles: crate::title::Shown::new(),
            mini: crate::title::Mini::new(),
            derive: crate::title::Derive::new(),
            watch,
            hosts,
            title_waits: TitleWaits::No,
            home: std::env::var_os("HOME")
                .map(|h| std::os::unix::ffi::OsStrExt::as_bytes(h.as_os_str()).to_vec()),
            hiding,
            // Placed once the window is shown, which lays out the traffic
            // lights it is placed against.
            icon: None,
            icon_inset: inset,
            lights: crate::lights::Strip::new(None, None, NSSize::new(0.0, 0.0)),
            front: crate::front::Front::new(),
            procs: crate::front::Live::new(),
            synced: crate::bell::Synced::new(),
            reload: None,
            notice,
            glass_live: std::rc::Rc::new(std::cell::Cell::new(glass_cfg)),
            window_id: None,
            close_asked: CloseAsked::No,
        };
        // **The config's watch, heard on the main queue** (`reload.rs`): the
        // dispatch source's handler drains the kqueue and wakes the view, and
        // `pump` takes the mark. No thread of baud's waits on it.
        if let Some((read, watch)) = reload_from {
            let waker = view.waker();
            let heard = watch.listen(dispatch2::DispatchQueue::main(), move || waker.wake());
            // A change heard while the config was read is due a settle after
            // it, woken here: no event may follow it.
            if let Some(at) = watch.deadline() {
                driver.wake_at(at);
            }
            driver.reload = Some(Reloading {
                _heard: heard,
                watch,
                read,
            });
        }
        startup_mark("driver");
        // The first frame's geometry and settings now, before the run loop:
        // the geometry the view's first resize callback would report. The
        // frame itself is drawn once the app has finished launching, below.
        // **The palette for the first frame, from the window rather than the
        // observer.** The observer below fires once the moment it is built and
        // would set this -- but it is built after the glass, which is after
        // this frame, so a light-mode launch would draw one dark frame and
        // correct it. The window knows its own appearance already.
        if theme_choice == crate::config::ThemeChoice::Auto {
            let appearance = appearance_of(&window);
            driver.palette_for = Some(appearance);
            driver.show_theme(appearance);
            crate::host::tell_defaults(
                &mut driver.app.term,
                driver.app.renderer.theme(),
                baud_core::Overrides::Keep,
            );
        }
        // **The display settings before the first frame**, pinned theme or
        // not, so a launch under Reduce Transparency never draws a dim frame.
        driver.poll_display();
        Responder::resized(&mut driver, view.frame().size, scale);
        startup_mark("laid out");
        Opening {
            driver,
            view,
            window,
            glass,
            container,
            inset,
        }
    }

    /// **On screen**: the first frame drawn, the window ordered front --
    /// cascaded from `after`, the frame of the window it was opened from
    /// -- its titlebar icon, the glass, the style observer, first
    /// responder, and the responder last, so nothing here re-enters a
    /// responder holding the view's borrow.
    fn show(self, mtm: MainThreadMarker, scale: CGFloat, after: Option<NSRect>) {
        let Opening {
            mut driver,
            view,
            window,
            glass,
            container,
            inset,
        } = self;
        // The first frame, drawn now on this thread: one draw, which
        // waits on the GPU and sets the layer itself (`draw_now`); no
        // completion follows it. Before the window is ordered front, so
        // the window is never on screen without it.
        // Said when the layer is missing, which would drop the frame, or
        // at a scale other than the display's, read from a window not yet
        // on screen.
        match view.layer() {
            None => startup_note("first frame: the view has no layer"),
            Some(l) if (l.contentsScale() - scale).abs() > 1e-6 => startup_note(&format!(
                "first frame: the layer's scale {:.2}, the display's {scale:.2}",
                l.contentsScale()
            )),
            Some(_) => {}
        }
        let v = driver.view.clone();
        startup_mark("first draw");
        driver.app.present(&v, Show::Now);
        startup_mark("first frame");
        // **What the shell has written is on the frame the window opens
        // with**: the reads that landed before the view could be woken wait
        // in the channel (`run`), and this turn feeds them -- the turn a
        // wake runs, less its frame -- and a frame drawn now shows them,
        // before the window is ordered front, rather than at the first
        // tick. Not while a synchronised update they opened is honoured
        // (`?2026`): the frame above, drawn before any byte, is the one the
        // window opens with, and the update's close draws the rest.
        driver.pump(Trigger::Opening);
        if driver.app.pending == Pending::Dirty
            && driver.app.updates.state(&driver.app.term, Instant::now()) != Update::Honoured
        {
            driver.app.present(&v, Show::Now);
            startup_mark("shell's first frame");
        }
        // **Ordered front at once, without AppKit's inferred open animation**,
        // so the window is whole on screen the moment it is shown and the
        // launch logo, which starts once it is shown (`frame`), plays over the
        // window rather than inside an animation that is still opening it.
        // `animationBehavior` is "used when the orderFront(_:) or orderOut(_:)
        // methods are called", and none disables it, "which may be useful if
        // that animation interferes with an animation that your application
        // implements" ([[source: apple-nswindow-animationbehavior]],
        // [[source: apple-nswindow-animationbehavior-none]]); AppKit posts no
        // notification at the animation's end ([[source: apple-nswindow]]).
        window
            .ns_window()
            .setAnimationBehavior(NSWindowAnimationBehavior::None);
        // A window opened from another is cascaded from it, as AppKit
        // offsets one window from the last: placed at that window's top
        // left, which hands back the point a step down and right, and then
        // at that point (`[[source: apple-nswindow-cascadetopleftfrompoint]]`).
        if let Some(f) = after {
            let w = window.ns_window();
            let next =
                w.cascadeTopLeftFromPoint(NSPoint::new(f.origin.x, f.origin.y + f.size.height));
            w.cascadeTopLeftFromPoint(next);
        }
        window.show();
        startup_mark("shown");
        // **The logo's plasma, built once the window is on screen** and on
        // a thread of its own, so neither the first frame nor the shell's
        // output waits on it; the logo starts on the first tick after it
        // is built (`App::plasma_ready`).
        driver.app.build_plasma();
        // **baud's lockup and the build's name** (`mark.rs`), a leading
        // titlebar accessory, sized after `show()` so the buttons are laid
        // out, in the form for the window's appearance now. The snug crop,
        // not the padded Dock mark, so the box is the size the lockup is
        // drawn at. **The activity lights** in a trailing one, which the
        // lights size to their plate at every layout, and the lockup moves
        // onto that plate (`strip.rs`, `lights::panel`). Both are
        // in the traffic lights' own view, which AppKit detaches from the
        // window in full screen, so neither stays behind in the content.
        driver.icon = crate::mark::Mark::put(mtm, &window, inset, crate::version::BUILD);
        let holder = crate::strip::Accessory::put(
            mtm,
            &window,
            TitlebarSide::Trailing,
            NSSize::new(0.0, inset),
        );
        let logo = driver
            .icon
            .as_ref()
            .map_or(NSSize::new(0.0, 0.0), crate::mark::Mark::size);
        driver.lights = crate::lights::Strip::new(Some(holder), Some(driver.view.waker()), logo);
        startup_mark("titlebar");
        // The glass under the terminal now that the first frame is on the
        // layer: ordering a window whose content is a glass effect view costs
        // 10-20 ms more than ordering one whose content is a plain view, so
        // the window went up bare and the material lands here, before the run
        // loop -- and before a responder exists, because moving the container
        // re-enters the view's resize callback, which a responder callback
        // holding the view's borrow cannot survive.
        window.set_content_view(glass.view());
        glass.set_content_view(&container);
        // The body sized the container to itself: the lockup against that.
        // The lights are drawn now rather than at the first tick, so a
        // window with them in its title bar has the lockup on the plate from
        // the start and never shows it beside the traffic lights.
        driver.draw_lights();
        driver.place_icon();
        driver.place_lights();
        startup_mark("glass installed");
        // **Follow the system's appearance and tint, which nothing did.** The
        // titlebar changed on a light/dark flip and the body did not, because
        // AppKit repaints the titlebar itself while the material and the
        // palette were both set once at launch and never again. The crate's
        // own example installs this observer and re-applies all three; the
        // callback fires once immediately, so building it is also what applies
        // the current style.
        //
        // It cannot be built before `show()` the way the example builds it:
        // the glass it tints does not exist until the line above, which is
        // after the first frame by design (ordering a window whose content is
        // a glass effect view costs 10-20 ms). The frame drawn in the gap gets
        // its palette from `window.is_dark()` above instead.
        //
        // The callback touches the window and the glass directly -- they are
        // AppKit objects, not the driver -- and leaves the palette in a cell
        // for `pump`, because it can land while a responder callback holds the
        // view's borrow.
        //
        // **It used to hand `s.tint()` to `set_tint_color`, and that was the
        // one thing the example's own comment forbids**: the theme colour
        // through this setter "floods the surface to a flat saturated slab at
        // full strength", which is why the example paints its wash in its own
        // `drawRect:` instead. The setter carries the material *darkening*
        // now, and nothing carries the wash -- see `apply_glass`.
        let style = {
            let glass_cb = glass.clone();
            let window_cb = window.clone();
            let glass_now = std::rc::Rc::clone(&driver.glass_live);
            let observer = macos_liquid_glass::icon_style::StyleObserver::new(mtm, move |_| {
                apply_glass(
                    &glass_cb,
                    &window_cb,
                    &glass_now.get(),
                    glass_cb.appearance(&window_cb),
                );
            });
            // The window's entry in the open windows holds it, and it goes
            // with the window (`closed`).
            startup_mark("style observer");
            observer
        };
        // **First responder AFTER the glass, because installing it takes the
        // chain away.** `set_content_view` replaces the window's content view,
        // which tears down the hierarchy the old one rooted and resigns first
        // responder back to the window. Claimed before this line -- where it
        // used to be -- the view is not in the responder chain afterwards, and
        // every menu item that targets it is disabled by AppKit's automatic
        // validation: Copy, Paste, Bigger, Smaller and Actual Size all read
        // `enabled = false`, while Minimize and Close, which AppKit answers
        // itself, read true. That is the whole signature of the defect.
        //
        // It cost copy, paste and zoom outright, because the key handler's
        // `Cmd-C`/`Cmd-V` arms were deleted when the menu landed: a key
        // equivalent is a menu item's, so with the item disabled nothing runs.
        // The glass move was a launch optimisation worth ~7 ms and it took
        // three commands with it, silently, because nothing asks a menu
        // whether it is enabled.
        window.make_first_responder(&view);
        // **Into the open windows before the responder is installed**, so
        // the driver knows the key its close is asked by (`close_later`),
        // and the window's close is heard: on every close, the button,
        // Command-W or the shell's exit, its observer asks for `closed` on
        // a later turn, outside whatever callback the close came from.
        let id = OPEN.with_borrow_mut(|o| {
            o.open(Open {
                window: window.clone(),
                view: view.clone(),
                _style: style,
                _closing: None,
                _full_screen: None,
                _keyups: None,
            })
        });
        let closing = baud_present_macos::closing::when_closing(window.ns_window(), move || {
            dispatch2::DispatchQueue::main().exec_async(move || closed(id));
        });
        // **Full screen moves the grid's top** (`strip.rs`): the whole
        // window once entered, the title bar's height down once a window
        // again, read then. The terminal's view takes the new frame here,
        // and its resize callback lays the grid out. In place, not on a
        // later turn: AppKit posts both at the end of a transition, from
        // the run loop, and baud asks for a transition only from a later
        // turn (`window_later`), so no responder callback holds the
        // view's borrow.
        let full_screen = {
            let (w, v, c) = (window.clone(), view.clone(), container.clone());
            baud_present_macos::full_screen::when_full_screen(window.ns_window(), move |change| {
                crate::strip::follow(change, w.ns_window(), &v, &c);
            })
        };
        OPEN.with_borrow_mut(|o| {
            if let Some(open) = o.get_mut(id) {
                open._closing = Some(closing);
                open._full_screen = Some(full_screen);
            }
        });
        // **The filming run's reading of full screen at rest**
        // (`full_screen_check.rs`): armed by its variable alone, which the
        // demo's `--record` sets and nothing else does, in the first window.
        if let Some(dir) = std::env::var_os(crate::full_screen_check::ENV)
            .filter(|_| crate::full_screen_check::step().is_none())
        {
            crate::full_screen_check::arm(
                crate::full_screen_check::Parts {
                    window: window.ns_window().retain(),
                    view: Retained::into_super(view.clone()),
                    container: container.clone(),
                    lockup: driver.icon.as_ref().map(crate::mark::Mark::accessory_view),
                    lights: driver.lights.holder_view(),
                },
                std::path::Path::new(&dir),
            );
        }
        driver.window_id = Some(id);
        view.set_responder(Box::new(driver));
        // A key let go with Command held, which AppKit keeps from the view,
        // until the window closes.
        let keyups =
            baud_present_macos::keyup::forward_command_key_ups(Retained::into_super(view.clone()));
        OPEN.with_borrow_mut(|o| {
            if let Some(open) = o.get_mut(id) {
                open._keyups = Some(keyups);
            }
        });
        view.start_display_link();
        startup_mark("display link");
        view.waker().wake();
    }
}

/// **A window's driver, dropped when its window closes** (`closed`):
/// the notifications still waiting for their pictures' checks post
/// first (`Driver::notices_at_close`).
impl Drop for Driver {
    fn drop(&mut self) {
        self.notices_at_close();
    }
}

/// The font sizes a zoom may reach, in points: below six nothing reads,
/// and past seventy-two a cell is most of a small window.
const ZOOM_MIN: f32 = crate::fonts::SIZE_MIN;
const ZOOM_MAX: f32 = crate::fonts::SIZE_MAX;

impl Driver {
    /// **Command-N**: a new window, opened on a later turn of the main
    /// queue (`open_window`), its shell started in this window's directory
    /// (`windows::inherited`). Before this machine's names are read, an
    /// `OSC 7` naming a host is judged against none.
    fn new_window(&mut self) {
        let names = self
            .hosts
            .names()
            .map(<[String]>::to_vec)
            .unwrap_or_default();
        let at =
            crate::windows::inherited(self.app.term.working_directory(), &names, self.derive.dir());
        dispatch2::DispatchQueue::main().exec_async(move || open_window(at));
    }

    /// **The window's close, asked for once**, on a later turn of the main
    /// queue: `close` posts the window's close notification at once, and
    /// the shell's exit is heard inside the view's callback. Asked only
    /// once the driver has its window: the opening turn runs before
    /// `show` gives it one, and a child already gone is closed by the
    /// turn of the wake that ends `show`, its stream still closed.
    fn close_later(&mut self) {
        if self.close_asked == CloseAsked::Yes {
            return;
        }
        let Some(id) = self.window_id else {
            return;
        };
        self.close_asked = CloseAsked::Yes;
        dispatch2::DispatchQueue::main().exec_async(move || {
            let window = OPEN.with_borrow(|o| o.get(id).map(|w| w.window.clone()));
            if let Some(w) = window {
                w.close();
            }
        });
    }

    /// Typing, and a paste, snap the window back to the live screen, where
    /// the keystroke's effect is.
    fn snap_to_live(&mut self) {
        if snap_live(&mut self.app.term) {
            self.app.travel.settle();
            self.app.pending = Pending::Dirty;
            // A programmatic scroll, which AppKit reveals: the thumb shows
            // at the bottom and fades.
            if let Some(now) = HostTime::of_secs(self.app.tick_target) {
                self.app.reveal.reveal(now);
            }
        }
    }

    /// **The window and its screen, for XTWINOPS's reports**: whether it is
    /// minimized (`[[source: apple-nswindow-isminiaturized]]`), its frame
    /// "in screen coordinates, including the title bar" (`[[source:
    /// apple-nswindow-frame]]`), the drawable view's rectangle taken to the
    /// screen through the window (`[[source: apple-nswindow-converttoscreen]]`),
    /// the screen "where most of the window is" (`[[source:
    /// apple-nswindow-screen]]`) and its full frame, "menu bar and dock"
    /// included (`[[source: apple-nsscreen-frame]]`), against the primary
    /// screen, "whose origin is at the point (0, 0)" (`[[source:
    /// apple-nsscreen-screens]]`), and the backing scale (`[[source:
    /// apple-nswindow-backingscalefactor]]`). Read on every turn of the
    /// loop, before the bytes are fed, and handed to the core when it
    /// changed; `window_facts` does the arithmetic.
    fn poll_window(&mut self) {
        let Some(w) = self.view.window() else {
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let primary = NSScreen::screens(mtm).firstObject().map(|s| s.frame());
        let view = w.convertRectToScreen(self.view.convertRect_toView(self.view.bounds(), None));
        let facts = window_facts(&Placed {
            minimized: match w.isMiniaturized() {
                true => Miniaturized::Yes,
                false => Miniaturized::No,
            },
            frame: w.frame(),
            view,
            pad: (self.app.pad_px.0.value(), self.app.pad_px.1.value()),
            screen: w.screen().map(|s| s.frame()),
            primary,
            scale: w.backingScaleFactor(),
        });
        if facts != self.window_facts {
            self.window_facts = facts;
            self.app.term.set_window(facts);
        }
    }

    /// The macOS window as AppKit has it now, for the window operations'
    /// policy (`winops.rs`): a program's requests and the status line's
    /// row.
    fn mac_window(&self, w: &NSWindow, mtm: MainThreadMarker) -> crate::winops::MacWindow {
        use crate::winops::{Full, MacWindow, Minimized, Rect};
        let rect = |r: NSRect| Rect {
            x: r.origin.x,
            y: r.origin.y,
            w: r.size.width,
            h: r.size.height,
        };
        let scale = w.backingScaleFactor();
        let bar = shown_gutter(
            self.app.scroller,
            self.app.bar_shown,
            crate::reveal::widths(Scale(scale)),
        );
        MacWindow {
            frame: rect(w.frame()),
            content: w.contentView().map_or((0.0, 0.0), |v| {
                let s = v.frame().size;
                (s.width, s.height)
            }),
            primary_top: NSScreen::screens(mtm).firstObject().map(|s| {
                let f = s.frame();
                f.origin.y + f.size.height
            }),
            visible: w.screen().map(|s| rect(s.visibleFrame())),
            scale,
            cell: self.app.metrics_cell(),
            padding: self.app.padding,
            gutter: self.app.beside(bar),
            grid: crate::width::Grid::of(&self.app.term),
            status: self.app.term.status_rows(),
            line: f64::from(self.app.renderer.lights_height().value()),
            minimized: match w.isMiniaturized() {
                true => Minimized::Yes,
                false => Minimized::No,
            },
            full: match w.styleMask().contains(NSWindowStyleMask::FullScreen) {
                true => Full::Yes,
                false => Full::No,
            },
            saved: self.saved_frame,
            host: self.app.sizing.host(),
        }
    }

    /// **A program's window operation, performed**: the window as AppKit
    /// has it now -- its frame and content view, the primary screen's top,
    /// its screen's visible frame (`[[source: apple-nsscreen-visibleframe]]`),
    /// minimised or not, in full screen or not (`[[source:
    /// apple-nswindow-stylemask-fullscreen]]`) -- and the grid, then
    /// [`crate::winops::mac_batch`]'s calls. Every AppKit call runs once the
    /// callback that asked has returned (`window_later`), as a page width's
    /// resize does, so the window is read once and each request measured on
    /// the window the calls before it leave -- `left`, the window the
    /// status line's frame leaves, where it queued one; a refresh is the
    /// next frame drawn whole.
    fn window_requests(
        &mut self,
        asked: &[baud_core::WindowRequest],
        left: Option<crate::winops::MacWindow>,
    ) {
        use crate::winops::MacAction;
        if asked.is_empty() {
            return;
        }
        let Some(w) = self.view.window() else {
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let at = left.unwrap_or_else(|| self.mac_window(&w, mtm));
        let (calls, left) = crate::winops::mac_batch(asked, at);
        self.saved_frame = left.saved;
        for action in calls {
            match action {
                MacAction::Nothing(why) => {
                    if let Some(line) = self.performed.mac_nothing(why) {
                        crate::warn!("{line}");
                    }
                    continue;
                }
                MacAction::Redraw => {
                    self.app.next_damage = Damage::Full;
                    self.app.pending = Pending::Dirty;
                }
                _ => window_later(&w, action),
            }
            self.performed.did();
        }
    }

    /// **The link preview at each display-link tick**: a press or release
    /// of Command alone is read as it happens (`modifiers_changed`), and
    /// the tick reads the modifiers held now as well (`App::read_hover`),
    /// for the view output moves under a held chord, and owes a
    /// frame when the preview changes, on an idle screen too -- a steady
    /// or hidden cursor draws no frame of its own to read them in. After
    /// `poll_focus`, so a focus lost has cleared the preview first.
    fn poll_hover(&mut self) {
        if self.app.read_hover() {
            self.app.pending = Pending::Dirty;
        }
    }

    /// Whether this window is the key window, read from AppKit now.
    ///
    /// **The live check found the focus gate dead.** `GlassWindow::show`
    /// makes the view first responder, so the crate's `Focus::Gained`
    /// fired before `set_responder`, into no one; the driver started at
    /// `Lost` and stayed there, a Cmd-click on a link never armed, and
    /// `?1004` never reported. The crate's hook is the view's
    /// first-responder status, which a single-view window changes once;
    /// what a terminal reports is whether its *window* is key, and the
    /// window knows. One message send, on the tick and before a mouse
    /// event, so a transition is seen within a frame and never after a
    /// click that depends on it.
    fn poll_focus(&mut self) {
        let key = self.view.window().is_some_and(|w| w.isKeyWindow());
        self.set_focus(if key {
            baud_core::Focus::Gained
        } else {
            baud_core::Focus::Lost
        });
    }

    /// **A read's notices delivered** (`notify.rs`): `Delivery::mac`
    /// decides, the window's key state and occlusion its focus and
    /// visibility, and this makes the calls it names -- the center's post
    /// and removal, an expiry scheduled on the main queue, the Dock tile's
    /// bar and `NSApplication`'s attention request -- and hands iTerm2's
    /// pictures to the checker, whose answers `Delivery::mac_checked`
    /// posts. Every turn of the loop comes here, the checker's wake among
    /// them, so a picture checked while the window is covered and the
    /// display link paused posts on that wake.
    fn deliver_notices(&mut self, posted: Vec<crate::notify::Posted>) {
        use crate::notify::{Focus as F, Visible};
        let window = self.view.window();
        let state = match &window {
            Some(w) => (
                if w.isKeyWindow() {
                    F::Focused
                } else {
                    F::Unfocused
                },
                if w.occlusionState().contains(NSWindowOcclusionState::Visible) {
                    Visible::Seen
                } else {
                    Visible::Covered
                },
            ),
            None => (F::Unfocused, Visible::Covered),
        };
        let prefix = notice_prefix(self.window_id);
        let outbox = &mut self.app.outbox;
        // The pictures the checker sent back since the last turn, their
        // notifications posted first: they came before this read's.
        let mut calls = Vec::new();
        for checked in self.app.checker.finished() {
            calls.extend(self.app.notes.mac_checked(
                &mut self.app.term,
                checked,
                Instant::now(),
                &prefix,
                |b| outbox.push(b),
            ));
        }
        calls.extend(self.app.notes.mac(
            &mut self.app.term,
            posted,
            state,
            Instant::now(),
            &prefix,
            &mut self.app.pictures,
            |b| outbox.push(b),
        ));
        self.notice_calls(calls);
    }

    /// **The notifications still waiting for their pictures' checks when
    /// the window closes, posted** from the driver's drop, which every
    /// close reaches -- the button, Command-W and the shell's exit -- before
    /// the center and the checker go with it: what the checker sent back
    /// with its picture, the rest with their text alone
    /// (`Delivery::mac_closing`). Nothing waits for a check; the close a
    /// shell's exit asks for has waited for them already (`pump`). It
    /// runs while the view's responder is being replaced, so nothing here
    /// touches the view.
    fn notices_at_close(&mut self) {
        let prefix = notice_prefix(self.window_id);
        let outbox = &mut self.app.outbox;
        let calls = self.app.notes.mac_closing(
            &mut self.app.term,
            self.app.checker.finished(),
            Instant::now(),
            &prefix,
            |b| outbox.push(b),
        );
        self.notice_calls(calls);
    }

    /// **The calls `Delivery` names, made**, and what it counts logged.
    fn notice_calls(&mut self, calls: Vec<crate::notify::MacCall>) {
        use crate::notify::{AttentionCall, MacCall};
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        for call in calls {
            match call {
                MacCall::Post(p) => {
                    if let Some(c) = &self.app.center {
                        c.post(mac_post(p));
                    }
                }
                // Checked off this thread; its notification posts on the
                // turn the checker's wake brings (`notify::Checker`).
                MacCall::Check(c) => self.app.checker.start(c),
                MacCall::Remove(id) => {
                    if let Some(c) = &self.app.center {
                        c.remove(&id);
                    }
                }
                MacCall::Expire(id, after) => {
                    if let Ok(when) = dispatch2::DispatchTime::try_from(after) {
                        let _ = dispatch2::DispatchQueue::main()
                            .after(when, move || baud_present_macos::notify::remove_by_id(&id));
                    }
                }
                // The Dock tile is the app's: it shows the most urgent
                // window's bar (`notify::DockBars`).
                MacCall::Bar(bar) => {
                    if let Some(id) = self.window_id {
                        show_dock(mtm, DOCK.with_borrow_mut(|d| d.set(id, bar)));
                    }
                }
                MacCall::Attention(a) => {
                    use baud_present_macos::notify::Ask;
                    if let Some(id) = self.app.attention.take() {
                        baud_present_macos::notify::cancel_attention(mtm, id);
                    }
                    self.app.attention = match a {
                        AttentionCall::Critical => {
                            Some(baud_present_macos::notify::attention(mtm, Ask::UntilActive))
                        }
                        AttentionCall::Informational => {
                            Some(baud_present_macos::notify::attention(mtm, Ask::Once))
                        }
                        AttentionCall::Cancel => None,
                    };
                }
            }
        }
        if let Some(line) = self.app.notes.rate_line() {
            crate::warn!("{line}");
        }
        if let Some(line) = self.app.notes.bare_line() {
            crate::warn!("{line}");
        }
        if let Some(line) = self.app.notes.image_line() {
            crate::warn!("{line}");
        }
    }

    /// **What the notification center said since the last tick**: the
    /// posts that waited for permission sent or dropped, said once, and
    /// the clicks -- reported to the program where it asked, and the
    /// window brought forward where `a` says focus.
    fn notice_turn(&mut self) {
        use baud_present_macos::notify::{DEFAULT_ACTION, Permission};
        let Some(c) = &self.app.center else {
            return;
        };
        let turn = c.turn();
        match turn.answer {
            Some(Permission::Granted) => crate::info!("notifications: allowed"),
            // **Not allowed: the core is told none is delivered**, so
            // kitty's query goes unanswered, and the posts that waited are
            // forgotten as not shown.
            Some(Permission::Denied) => {
                crate::info!(
                    "notifications: not allowed; System Settings > Notifications turns them on"
                );
                self.app.term.set_notice_support(None);
            }
            _ => {}
        }
        self.app
            .notes
            .mac_refused(&mut self.app.term, &turn.refused);
        // A `w` that has passed, and every few seconds the list
        // Notification Center holds, so `p=alive` names what is still
        // there: nothing on macOS says one closed.
        let now = Instant::now();
        self.app.notes.mac_expired(&mut self.app.term, now);
        if let Some(present) = c.delivered() {
            if let Some(asked) = self.app.delivered_asked.take() {
                self.app
                    .notes
                    .mac_absent(&mut self.app.term, &present, asked);
            }
        }
        let due = self.app.delivered_asked.is_none()
            && self.app.notes.shown.any()
            && c.allowed() == Permission::Granted
            && self
                .app
                .delivered_last
                .is_none_or(|t| now.duration_since(t) >= DELIVERED_EVERY);
        if due {
            self.app.delivered_asked = Some(now);
            self.app.delivered_last = Some(now);
            c.ask_delivered();
        }
        // **Only the clicks on this window's own posts** (`windows::clicked`):
        // the center and its delegate are the app's, and every window's
        // clicks arrive there.
        let base = notice_base();
        let me = self.window_id;
        // The windows open; while the set is being changed, every window
        // counts as open, so no click is dropped for a window that is.
        let open = OPEN.with(|o| o.try_borrow().map(|o| o.ids()).ok());
        let whose = |id: &str| {
            use baud_present_macos::notify::Whose;
            let Some(me) = me else {
                return Whose::Another;
            };
            let all = [me];
            match crate::windows::clicked(id, &base, me, open.as_deref().unwrap_or(&all)) {
                crate::windows::Clicked::This => Whose::This,
                crate::windows::Clicked::Another => Whose::Another,
                crate::windows::Clicked::Closed if open.is_none() => Whose::Another,
                crate::windows::Clicked::Closed => Whose::Closed,
            }
        };
        for click in c.clicks(whose) {
            let Some(token) = self.app.notes.shown.token(&click.id) else {
                continue;
            };
            let Some(button) = crate::notify::clicked(&click.action, DEFAULT_ACTION) else {
                continue;
            };
            if button.is_none() && self.app.term.notice_focuses(token) {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).activate();
                }
                if let Some(w) = self.view.window() {
                    w.makeKeyAndOrderFront(None);
                }
            }
            let outbox = &mut self.app.outbox;
            crate::reads::input(
                &mut self.app.term,
                |t| t.notice_activated(token, button),
                |b| outbox.push(b),
            );
        }
    }

    /// **The bell's AppKit shim**: the window as AppKit has it now -- key
    /// (`[[source: apple-nswindow-iskeywindow]]`), minimized (`[[source:
    /// apple-nswindow-isminiaturized]]`) or neither, asked here rather than
    /// taken from the tick's focus poll, which may not have run since the
    /// window changed -- classed by [`Window::of`](crate::bell::Window::of),
    /// then [`ring`](crate::bell::ring)'s decision, and the calls it names.
    /// Both are tested; this is the two questions and the four calls, each
    /// under its condition, and the source gate holds it to that text. The
    /// volume is the program's for the bells that rang ([`Rings`]).
    fn ring_bell(&mut self, volume: baud_core::BellVolume) {
        use crate::bell::{Attention, Key, Miniaturized, Raise, Shown, Sound, Window};
        let window = self.view.window();
        let state = match &window {
            Some(w) => Window::of(
                if w.isKeyWindow() {
                    Key::Key
                } else {
                    Key::NotKey
                },
                if w.isMiniaturized() {
                    Miniaturized::Minimized
                } else {
                    Miniaturized::NotMinimized
                },
            ),
            None => Window::Unfocused,
        };
        let ring = crate::bell::ring(
            self.app.bell_style,
            self.app.motion,
            state,
            self.app.term.bell_is_urgent(),
            self.app.term.pop_on_bell(),
            volume,
        );
        if ring.sound == Sound::Beep {
            NSBeep();
        }
        if ring.shown == Shown::Flash {
            self.app.bell.ring();
            self.app.pending = Pending::Dirty;
        }
        if ring.attention == Attention::Request {
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm)
                    .requestUserAttention(NSRequestUserAttentionType::InformationalRequest);
            }
        }
        if ring.raise == Raise::Front {
            if let Some(w) = &window {
                w.orderFrontRegardless();
            }
        }
    }

    /// **Output scrolled the whole screen `n` lines at the live window**,
    /// and the config says `ride` (`trail_rides`): what the trail drew from
    /// the text rides up with it -- the quad or sweep in flight for at most
    /// two rows, an effect's whole path, the ghost's origin (`trail.rs`,
    /// `ride_ghost`). On the display link's clock, which the trail's frame
    /// reads, and never past one window above the top. Where the grid
    /// slides, `present` lifts the trail by the slide's pan as it lifts the
    /// grid; where it jumps, the ride alone keeps the trail on its text.
    fn ride_trail(&mut self, n: Rows) {
        let cell_h = self.app.metrics_cell().1 as f32;
        let rows = Rows::of_count(usize::from(self.app.grid.ws_row));
        let Some((dy, floor)) = ride_px(n, rows, cell_h) else {
            return;
        };
        self.app.ridden = Rows::of_count(self.app.ridden.index() + n.index());
        if let Some(now) = HostTime::of_secs(self.app.tick_target) {
            self.app.chase.ride(dy, now, floor, cell_h);
        }
        let tick = self.app.tick_target;
        self.app.trail = self.app.trail.and_then(|st| {
            let p = crate::trail::ghost_progress(st.start, tick);
            ride_ghost(st, dy, p, cell_h)
        });
    }

    /// Whether the system asks for reduced motion, read on the tick like
    /// focus: one message send and a cached byte, about 0.04 us. **Polled,
    /// not observed**: AppKit registers its own observer of the
    /// display-options notification in `sharedApplication`, and its handler
    /// invalidates the cached answer before anything is posted, so a toggle
    /// reads fresh here within a frame (the design's probes; the live
    /// toggle is checked by hand, not by a test). Under `BAUD_MOTION`
    /// nothing is read.
    fn poll_motion(&mut self) {
        let m = system_motion(self.app.motion_source, reduce_motion);
        if m == self.app.motion {
            return;
        }
        self.app.motion = m;
        // The blink's animation follows the preference whatever the config
        // named: Reduce Motion always means a hard blink.
        self.app
            .caret
            .set_animation(self.app.blink_animation.effective(m));
        let Some(next) = motion_change(self.app.trail_pick, self.app.cursor_trail, m) else {
            return;
        };
        crate::info!("motion: {m:?}; trail: {next:?}");
        let owes = retrail(
            &mut self.app.chase,
            &mut self.app.trail,
            &mut self.app.last_cursor_cell,
            next,
            HostTime::of_secs(self.app.tick_target),
        );
        self.app.cursor_trail = next;
        if owes == Owes::Frame {
            self.app.pending = Pending::Dirty;
        }
    }

    /// The Accessibility Display settings, read on the tick beside Reduce
    /// Motion and on the same terms: AppKit's own observer of the
    /// display-options notification invalidates the cached answers, which
    /// the Reduce Motion design probed for that setting, so baud adds no
    /// observer of its own for these (`follow_settings` observes others);
    /// that a live toggle of these two reads fresh within a frame is
    /// checked by hand, not by a test, and the cost of these two reads is
    /// carried over from Reduce Motion's, not measured for them. **Not
    /// through `apply_style`**, whose early return for a pinned theme
    /// would leave a pinned reader's cells as they were: this rebuilds
    /// whichever theme is in force. The core is told nothing, because the
    /// settings move no colour it reports.
    fn poll_display(&mut self) {
        let display = display_settings(reduce_transparency, increase_contrast);
        if display == self.display {
            return;
        }
        self.display = display;
        crate::info!("display: {display:?}");
        self.show_theme(in_force(self.theme_choice, self.palette_for));
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
    }

    /// **A setting AppKit said changed, followed on this tick**: the
    /// observers in `baud_present_macos::settings` mark what was posted,
    /// and this takes the marks. A change of "Show scroll bars" lays the
    /// window out again at once when the style it asks for is not the one
    /// drawn, so the legacy strip comes or goes, and the grid's columns
    /// with it, without waiting for the next wheel or resize.
    ///
    /// A change of Dim Flashing Lights is read here too, and the next
    /// tick's phases take it (`dim_off_phases`): a blinking cell's off
    /// phase goes faint or hidden on the frame the phase change owes. So is
    /// a change of Animated Images, which the same tick's animation step
    /// takes: a held picture plays on, or a playing one holds where it is.
    fn follow_settings(&mut self) {
        let heard = self.settings.take(Setting::Scroller);
        if scroller_change(heard, self.app.scroller, scroller_style) == Relayout::Now {
            crate::info!("scroller: {:?}", scroller_style());
            let _ = self.relayout(self.view.frame().size, self.app.scale);
        }
        if self.settings.take(Setting::DimFlashingLights) == Heard::Changed {
            self.app.flashing = baud_present_macos::settings::flashing();
            crate::info!("flashing: {:?}", self.app.flashing);
        }
        if self.settings.take(Setting::AnimatedImages) == Heard::Changed {
            self.app.animated = baud_present_macos::settings::animated_images();
            crate::info!("animated images: {:?}", self.app.animated);
        }
    }

    /// **The config's files, followed** (`reload.rs`): what the kernel said
    /// since the last turn, a wake scheduled for the settle's end, and at
    /// that end the files read and, when one reads differently, the config
    /// read again. A reload that may restack waits, as a zoom does, for the
    /// frame that reads the atlas: the settle stays due, and the frame's
    /// completion wakes the next turn.
    fn follow_config(&mut self) {
        let Some(r) = self.reload.as_mut() else {
            return;
        };
        let now = Instant::now();
        let mut wake_at = None;
        if r.watch.heard(now) == crate::reload::Heard::Moved {
            wake_at = r.watch.deadline();
        }
        let due = r.watch.deadline().is_some_and(|d| d <= now);
        if let Some(at) = wake_at {
            self.wake_at(at);
        }
        if !due || self.app.presenter.as_ref().map(Presenter::reshape) == Some(Reshape::Wait) {
            return;
        }
        let Some(r) = self.reload.as_mut() else {
            return;
        };
        if r.watch.due(now) == crate::reload::Due::Changed {
            self.reload_config(now);
        }
    }

    /// **The system's faces for what no face draws** (`host::follow_gaps`):
    /// asked once the frames that met the characters are drawn, and taken,
    /// like a reload's fonts, once no frame in flight reads the atlas; the
    /// gaps wait in the renderer until then, and the frame's completion
    /// wakes the next turn. The cell size is the primary's, so the grid
    /// stays and the whole screen is drawn again from the new stack.
    fn follow_gaps(&mut self) {
        if self.app.presenter.as_ref().map(Presenter::reshape) == Some(Reshape::Wait) {
            return;
        }
        let Some(px) = PxSize::of(self.app.font_size * self.app.scale as f32) else {
            return;
        };
        let (renderer, fonts) = self.app.renderer.both();
        match crate::host::follow_gaps(renderer, fonts, px) {
            None => {}
            Some(Ok(given)) => {
                crate::info!(
                    "fonts: a system face added; {}",
                    crate::host::given_back(given)
                );
                self.app.rescaled();
                self.app.next_damage = Damage::Full;
                self.app.pending = Pending::Dirty;
            }
            Some(Err(e)) => crate::warn!("fonts: a system face did not load: {e}"),
        }
    }

    /// A turn of `pump` at `at`, from the main queue: the settle's end,
    /// reached whether or not the display link is running.
    fn wake_at(&self, at: Instant) {
        let waker = self.app.waker;
        let after = at.saturating_duration_since(Instant::now());
        if let Ok(when) = dispatch2::DispatchTime::try_from(after) {
            let _ = dispatch2::DispatchQueue::main().after(when, move || waker.wake());
        }
    }

    /// **The config read again, and what it changed applied** (`live.rs`):
    /// a file that does not parse, or the file in force gone, leaves the
    /// window as it is and says so on the notice's row; a file that parses
    /// is applied, part by part, its complaints on the row, and a setting a
    /// running window cannot take is named in the log as waiting for the
    /// next launch. The watch then follows the files the new config names.
    fn reload_config(&mut self, now: Instant) {
        let Some(r) = self.reload.as_ref() else {
            return;
        };
        let in_force = r.read.source.clone();
        let reading = match crate::config::Config::reread(in_force.as_deref()) {
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
        // **The files the new config names watched before they are read**,
        // as at launch (`reload::Watch::around`): a theme or font file it
        // names for the first time and edited while it is applied is heard.
        let mut again = None;
        if let Some(r) = self.reload.as_mut() {
            r.watch.rewatch(
                crate::reload::files(&watched.lookup, &watched.themes, &watched.faces),
                now,
            );
            // A change between the reading and the new watch is due a settle
            // after it, woken here: the display link may be paused.
            again = r.watch.deadline();
        }
        said.extend(self.apply_config(&config, &parts));
        crate::info!(
            "config: read again from {}",
            config
                .source
                .as_deref()
                .map_or("the defaults".to_string(), |p| p.display().to_string())
        );
        if let Some(line) = crate::live::later(&parts) {
            crate::info!("{line}");
        }
        let off = config.config_reload == crate::config::ConfigReload::Off;
        if let Some(r) = self.reload.as_mut() {
            r.read = config;
        }
        if let Some(at) = again {
            self.wake_at(at);
        }
        self.show_notice(crate::live::Notice::of(&said));
        // **Off is applied, then nothing after it until the next launch**,
        // as WezTerm stops watching when a reload says so.
        if off {
            crate::info!("config: config_reload is off; changes wait for the next launch");
            self.reload = None;
        }
    }

    /// **Each part of a config read again, applied to the window**: what
    /// the core is told, the palette and the glass, the fonts and their
    /// size, the cursor and its blink and trail, the bell, the scroll and
    /// the transition. Returns what it could not act on: a font that does
    /// not load, which leaves the fonts as they were.
    fn apply_config(
        &mut self,
        c: &crate::config::Config,
        parts: &[crate::live::Part],
    ) -> Vec<String> {
        use crate::live::Part;
        let mut said = Vec::new();
        let mut relayout = false;
        for &p in parts {
            match p {
                Part::TermOptions => {
                    crate::live::term_options(
                        &mut self.app.term,
                        c,
                        crate::winops::mac_abilities(),
                    );
                    self.app.clipboard.osc52 = c.clipboard.osc52;
                    // `prompt_at_bottom` turned on at a prompt moves its
                    // rows in the core with no output to draw them.
                    self.app.next_damage = Damage::Full;
                }
                Part::Keyboard => self.app.term.change_set_up(c.set_up),
                Part::Bell => self.app.bell_style = c.bell,
                Part::Emoji => {
                    self.app.renderer.set_emoji_default(c.emoji);
                    self.app.next_damage = Damage::Full;
                }
                Part::Notifications => {
                    self.app.notes.setting = c.notifications;
                    self.app.term.set_notice_support(match self.app.center {
                        Some(_) => {
                            crate::notify::support(c.notifications, crate::notify::Platform::MacOs)
                        }
                        None => None,
                    });
                }
                Part::CopyOnSelect => {
                    self.app.clipboard.copy_on_select = c.clipboard.copy_on_select;
                }
                // Put up or taken down at the next tick, and a line of their
                // own given its pixels now.
                Part::ActivityLights => {
                    let was = self.app.renderer.lights_height();
                    self.app.activity_lights = c.activity_lights;
                    self.app.renderer.set_lights_colours(
                        c.activity_lights_colours,
                        baud_render::Halo::of(c.activity_lights_glow),
                    );
                    self.app.pending = Pending::Dirty;
                    self.app
                        .renderer
                        .set_lights_line(c.activity_lights.line(), self.app.scale as f32);
                    self.lights_line(was);
                }
                // DEC's line given the words, or none, at the next tick.
                Part::StatusLineLights => self.app.status_line_lights = c.status_line_lights,
                // The layer's contents are shown at the display's refresh,
                // never torn: Linux only.
                Part::Tearing => {}
                // Off takes a splash still on screen down with one frame.
                Part::Splash => {
                    let setting = crate::splash::splash_override(
                        std::env::var("BAUD_SPLASH").ok().as_deref(),
                        c.launch_splash,
                    );
                    if self.app.splash.set(setting) == crate::splash::Owes::Frame {
                        self.app.next_damage = Damage::Full;
                        self.app.pending = Pending::Dirty;
                    }
                }
                Part::AmbiguousWidth => self.app.term.set_ambiguous_width(c.ambiguous_width),
                Part::SpacingMarks => self.app.term.set_spacing_marks(c.join_spacing_marks),
                Part::Scroll => self.app.scroll = c.scroll,
                Part::Transition => self.app.screen_transition = c.screen_transition,
                Part::Trail => {
                    self.app.trail_pick = c.cursor_trail;
                    self.app.trail_scroll = c.trail_scroll;
                    let next = c.cursor_trail.effective(self.app.motion);
                    if next != self.app.cursor_trail {
                        let owes = retrail(
                            &mut self.app.chase,
                            &mut self.app.trail,
                            &mut self.app.last_cursor_cell,
                            next,
                            HostTime::of_secs(self.app.tick_target),
                        );
                        self.app.cursor_trail = next;
                        if owes == Owes::Frame {
                            self.app.pending = Pending::Dirty;
                        }
                    }
                }
                Part::CursorStyle => {
                    self.app
                        .term
                        .reconfigure_cursor_style(baud_core::CursorStyle::of(
                            c.cursor_shape,
                            c.cursor_blink.power_on(),
                        ));
                }
                Part::Caret => {
                    self.app.blink_choice = c.cursor_blink;
                    self.app.blink_animation = c.blink_animation;
                    self.app.blink_interval = c.blink_interval;
                    self.app.caret.set_stop(c.cursor_stop);
                    self.app
                        .caret
                        .set_animation(c.blink_animation.effective(self.app.motion));
                    self.read_caret_system();
                }
                Part::CursorGuide => self.app.cursor_guide = c.cursor_guide,
                Part::Badge => self.app.badge = c.badge,
                Part::WrapWords => {
                    self.app.renderer.set_word_wrap(c.wrap_words);
                    self.app.kept.set_word_wrap(c.wrap_words);
                }
                Part::WrapMarker => self.app.renderer.set_wrap_marker(c.wrap_marker),
                Part::ImageAnimation => self.app.image_animation = c.image_animation,
                Part::Hollow => {
                    self.app.unfocused_cursor = c.unfocused_cursor;
                    let f = self.app.focus;
                    self.app.show_focus(f);
                }
                Part::StatusMarks => {
                    self.app.renderer.set_status_marks(c.exit_status);
                    relayout = true;
                }
                Part::Padding => {
                    self.app.padding = f64::from(c.padding);
                    relayout = true;
                }
                Part::Palette => said.extend(self.reload_palette(c)),
                // The fonts and their size together, below.
                Part::Fonts | Part::FontSize => {}
                Part::Reload => {}
                Part::ColourSpace
                | Part::GlassStyle
                | Part::Images
                | Part::Sixel
                | Part::Vs16Width
                | Part::Printing
                | Part::ClipboardArea
                | Part::ShellIntegration
                | Part::ShellProgram => {}
            }
        }
        // **A size the user zoomed to stays**, as alacritty keeps one
        // "changed at runtime"; Actual Size returns to the new configured
        // size either way.
        let points = crate::live::size_after(self.app.font_size, self.app.font_base, c.font_size);
        self.app.font_base = c.font_size;
        // Every part is applied by here, so a frame the fonts' relayout or
        // zoom builds below shows them all (`read_owes`).
        let before = self.app.frames;
        if parts.contains(&Part::Fonts) {
            match self.reload_fonts(c, points) {
                Ok(()) => relayout = true,
                Err(e) => said.push(format!("config: {e}; the fonts stay as they were")),
            }
        } else if points != self.app.font_size {
            self.zoom(points);
        }
        crate::host::tell_font(
            &mut self.app.term,
            self.app.renderer.fonts(),
            self.app.font_size,
            self.app.font_base,
        );
        if relayout {
            self.app.next_damage = Damage::Full;
            self.app.pending = Pending::Dirty;
            if self.relayout(self.view.frame().size, self.app.scale) == Drew::No {
                self.app.present(&self.view, Show::Event);
            }
        }
        self.app.pending = read_owes(before, self.app.frames, self.app.pending);
        said
    }

    /// **The palette and the glass from a config read again**: the theme
    /// files read anew, both themes built, the glass's tint and shadow
    /// applied, and the theme in force shown at full damage. Colours a
    /// program set keep winning: the core is told the new defaults with its
    /// overrides kept. Returns what the theme files could not act on.
    fn reload_palette(&mut self, c: &crate::config::Config) -> Vec<String> {
        let (mut halves, said) = crate::palette::load_halves(
            c.palette_light.as_deref(),
            c.palette_dark.as_deref(),
            &crate::palette::Dirs::from_env(c.source.as_deref()),
        );
        halves.selection = c.selection;
        halves.cursor = c.cursor;
        self.theme_choice = c.theme;
        self.glass_cfg = c.glass;
        self.glass_live.set(c.glass);
        self.themes = Themes::of(&c.glass, c.theme, c.floor, &halves);
        let appearance = match c.theme {
            crate::config::ThemeChoice::Dark => Appearance::Dark,
            crate::config::ThemeChoice::Light => Appearance::Light,
            crate::config::ThemeChoice::Auto => appearance_of(&self.window),
        };
        apply_glass(
            &self.glass,
            &self.window,
            &self.glass_cfg,
            self.glass.appearance(&self.window),
        );
        self.palette_for = (c.theme == crate::config::ThemeChoice::Auto).then_some(appearance);
        self.show_theme(appearance);
        crate::host::tell_defaults(
            &mut self.app.term,
            self.app.renderer.theme(),
            baud_core::Overrides::Keep,
        );
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        said
    }

    /// **The fonts a config read again names**, mapped, parsed and leading
    /// the chain at `points`: the stack restacked as a program's `OSC 50`
    /// restacks it, and a face a program led with dropped, its slot naming
    /// nothing in the new list. A font that does not load leaves the fonts
    /// as they were.
    fn reload_fonts(&mut self, c: &crate::config::Config, points: f32) -> Result<(), String> {
        let resolved = c.clone().resolved();
        let px = PxSize::of(points * self.app.scale as f32).ok_or("no size")?;
        let (renderer, fonts) = self.app.renderer.both();
        let given = crate::host::refont(renderer, fonts, &resolved, px)?;
        crate::info!("{}", crate::host::given_back(given));
        self.app.rescaled();
        self.app.font_size = points;
        Ok(())
    }

    /// **The config's notice on its row, or none**: the grid gives the row
    /// up while one is shown and takes it back when it goes, the program
    /// told each time.
    fn show_notice(&mut self, notice: crate::live::Notice) {
        if notice == self.notice {
            return;
        }
        let rows_moved = notice.rows() != self.notice.rows();
        self.app.renderer.set_notice(notice.codepoints());
        self.notice = notice;
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        if rows_moved && self.relayout(self.view.frame().size, self.app.scale) == Drew::No {
            self.app.present(&self.view, Show::Event);
        }
    }

    /// Set the renderer's theme to `appearance`'s as the display settings
    /// ask for it drawn: the one way a theme reaches the renderer, so a
    /// light/dark flip under Reduce Transparency stays opaque.
    fn show_theme(&mut self, appearance: Appearance) {
        let theme = accessible(self.themes.get(appearance).clone(), self.display);
        self.app.renderer.set_theme(theme);
    }

    /// Record a focus transition and report it if the application asked.
    /// Only a *change* reaches the core, so polling cannot repeat `CSI I`.
    fn set_focus(&mut self, f: baud_core::Focus) {
        // Checked on every poll, not only on a change: a window that never
        // had the keyboard starts `Lost` with no change to see.
        if (f == baud_core::Focus::Lost) != self.app.renderer.inactive_selection().is_some() {
            self.inactive_selection(f);
        }
        self.app.show_focus(f);
        if self.app.focus == f {
            return;
        }
        self.app.focus = f;
        if self.app.term.focus(f) {
            self.app.flush_reply(Asked::Unasked);
        }
        // Losing the keyboard shows a pointer a key hid, but at mode 3,
        // as xterm's `unselectwindow` does on a focus-out.
        if f == baud_core::Focus::Lost {
            // The keys still held are let go: their key-ups go elsewhere.
            // Sent here and drawn by the next frame, not pumped: a poll
            // can run inside the pump.
            for e in self.downs.let_go() {
                let _ = self.app.send(e);
                if self.app.term.local_echo() {
                    self.app.pending = Pending::Dirty;
                }
            }
            hide_pointer(self.hiding.crossed(&self.app.term));
            if self.app.hover.clear() {
                self.app.pending = Pending::Dirty;
                self.app.reshape_pointer();
            }
        }
        // A gain reads the system's caret again -- a change made in System
        // Settings while baud was in the background lands here -- and is
        // input, so the cursor comes back shown and the stop starts over.
        if f == baud_core::Focus::Gained {
            self.read_caret_system();
            // And the keyboard layouts, which DECHEBM needs a Hebrew one of
            // (`layouts.rs`).
            crate::host::set_hebrew_layout(&mut self.app.term, crate::layouts::hebrew_layout());
            self.app.caret.input(host_now());
            // And the printer list, off this thread: a printer added in
            // System Settings is the core's at the next gain (`print.rs`).
            self.app.read_printers(crate::print::Ask::Now);
        }
    }

    /// **The selection in a window that is not key**: AppKit's unemphasized
    /// pair, resolved under the window's appearance
    /// (`baud_present_macos::selection`), which reverse video takes while
    /// another window has the keyboard; none once this one has it. A frame
    /// is owed, as nothing else on screen changed.
    fn inactive_selection(&mut self, f: baud_core::Focus) {
        let pair = match f {
            baud_core::Focus::Gained => None,
            baud_core::Focus::Lost => Some(baud_present_macos::selection::unemphasized(
                &self.window.ns_window().effectiveAppearance(),
            )),
        };
        if pair != self.app.renderer.inactive_selection() {
            self.app.renderer.set_inactive_selection(pair);
            self.app.pending = Pending::Dirty;
        }
    }

    /// **The printer list a read brought back, to the core**: its printer,
    /// which answers a held DSR `?15` (`Term::set_printer`), and a frame
    /// when the indicator line's printer field changed.
    fn printers_read(&mut self) {
        let Some(listed) = self.app.printer_list.take() else {
            return;
        };
        self.app.watch.landed();
        let was = self.app.term.printer();
        crate::print::set_printer(&mut self.app.term, self.app.printing, listed);
        if self.app.term.printer() != was
            && self.app.term.status_type() == baud_core::StatusType::Indicator
        {
            self.app.pending = Pending::Dirty;
        }
    }

    /// The title when a program set none: the folder and program
    /// (`title::Derive::title`), else "baud". A report naming another
    /// host, refused, is logged once (`Derive::refusal`).
    /// While this machine's names are still being read, a report naming
    /// a host cannot be judged, and the title shown stays until they are
    /// (`TitleWaits`).
    fn derived_title(&mut self) -> String {
        let cwd = self.app.term.working_directory();
        let Some(names) = judged_by(self.hosts.names(), cwd) else {
            self.title_waits = TitleWaits::Names;
            return self.titles.sent().unwrap_or("baud").to_string();
        };
        let refused = self.derive.refused;
        let title = self.derive.title(cwd, names, self.home.as_deref());
        if self.derive.refused != refused {
            crate::info!("{}", self.derive.refusal());
        }
        title.unwrap_or_else(|| "baud".to_string())
    }

    /// The window's title, sent when it changed (`title::Shown`), and the
    /// minimized window's after it: the icon name, else the title
    /// (`title::Mini`), set again after every title set, which AppKit
    /// lets overwrite it. Through the crate's `ns_window`, as the window's
    /// size is set, since the crate has no call of its own for it.
    fn send_title(&mut self) {
        let fallback = self.derived_title();
        let was = self.app.session.clone();
        let s = &mut self.app.session;
        s.pid = self.app.pty.child();
        s.tty = self.app.pty.tty().map(|p| p.to_string_lossy().into_owned());
        s.job = self.derive.group();
        s.job_name = self.derive.name().map(str::to_string);
        s.dir = self.derive.dir().map(|d| d.to_string_lossy().into_owned());
        let sent = self.titles.next(
            self.app.term.title(),
            self.app.term.title_encoding(),
            &fallback,
            self.app.term.print_run(),
        );
        if let Some(t) = &sent {
            self.window.set_title(t);
        }
        self.app.session.title = self.titles.sent().map(str::to_string);
        // A badge naming the job, its folder or the title is drawn again
        // when they change, though the screen did not.
        if self.app.session != was && self.app.term.badge_format().is_some() {
            self.app.pending = Pending::Dirty;
        }
        let Some(title) = self.titles.sent() else {
            return;
        };
        if let Some(m) = self.mini.next(
            sent.as_deref(),
            self.app.term.icon_name(),
            self.app.term.icon_name_encoding(),
            title,
        ) {
            self.window
                .ns_window()
                .setMiniwindowTitle(Some(&NSString::from_str(&m)));
        }
    }

    /// **The derived title's check, once a tick and only when owed**
    /// (`title::Derive`). After a process event the last group's children
    /// are watched before the group is read, so a job that takes the
    /// terminal and execs after the read still says so (`fg::Watch`);
    /// the group read is watched after it. A title that waited for this
    /// machine's names is sent once they are read.
    fn title_check(&mut self) {
        // A title that waited for this machine's names, once they are read
        // (`TitleWaits`).
        if self.title_waits == TitleWaits::Names && self.hosts.names().is_some() {
            self.title_waits = TitleWaits::No;
            self.send_title();
        }
        if self.derive.owed() == crate::title::Owed::No {
            return;
        }
        if let (Some(w), Some(g), true) = (
            &self.watch,
            self.derive.group(),
            self.derive.moved_since_check(),
        ) {
            w.add_children(g);
        }
        if self.derive.check(&self.app.pty.reads()) == crate::title::Changed::Yes {
            if let (Some(w), Some(g)) = (&self.watch, self.derive.group()) {
                w.add(g);
            }
            self.send_title();
        }
    }

    /// **The waiting print job onto the print panel**, once the panel
    /// before it has gone: at most one at a time, shown as a sheet once the
    /// callback this runs in has returned (`print_later`), and gone when the
    /// window has no sheet attached.
    fn print_next(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        let panel = &self.app.panel;
        if panel.load(Relaxed) == PANEL_SHOWN && self.window.ns_window().attachedSheet().is_none() {
            panel.store(PANEL_IDLE, Relaxed);
        }
        if panel.load(Relaxed) != PANEL_IDLE {
            return;
        }
        let Some(job) = self.app.prints.next(self.app.term.printer()) else {
            return;
        };
        let fallback = self.derived_title();
        let window = crate::title::window_title(
            self.app.term.title(),
            self.app.term.title_encoding(),
            &fallback,
        );
        let panel = &self.app.panel;
        panel.store(PANEL_SCHEDULED, Relaxed);
        print_later(self.window.ns_window(), window, job, panel.clone());
    }

    /// The system caret's rate and its non-blinking switch (`caret.rs`),
    /// read on the first tick -- after the first present, so a launch's
    /// first frame does not wait for it, and where a scripted launch, whose
    /// window is never key, still pays and measures it -- and at every focus
    /// gain (`Caret::read_system`). Not observed: a change made while baud
    /// keeps focus lands at the next gain.
    fn read_caret_system(&mut self) {
        let start = Instant::now();
        let cycle = os_cycle(self.app.blink_interval);
        let (switch, overridden) = self
            .app
            .caret
            .read_system(cycle, self.app.blink_choice, || match os_at_least_15() {
                true => crate::pty::prefers_steady_caret(),
                false => NonBlinking::Off,
            });
        let took = start.elapsed();
        crate::info!(
            "caret: {cycle}, switch {switch:?}, read in {} us",
            took.as_micros()
        );
        if overridden == crate::caret::Overridden::Yes && self.app.warned_blink == Done::No {
            crate::warn!(
                "baud: [cursor] blink = true overrides the system's non-blinking caret; \
                 blink = \"auto\" honours it"
            );
            self.app.warned_blink = Done::Yes;
        }
    }

    /// **The link preview's share of a mouse event**: with Command held
    /// (and not Shift, the selection's), the link under the pointer is
    /// underlined and its address shown (`link::Hover`); a move without it,
    /// or the pointer leaving, takes them away. Mouse events carry the
    /// modifiers, and a press or release of Command alone with the pointer
    /// still is read from the window's modifier change
    /// (`modifiers_changed`) and again at each display-link tick, from
    /// `NSEvent.modifierFlags`, which owes a frame when the preview changes
    /// (`poll_hover`).
    /// The pointer's cell is kept whatever the modifiers, for that read.
    fn hover_mouse(&mut self, event: &Mouse) {
        use crate::link::{Chord, Host};
        let chord = if event.modifiers.command && !event.modifiers.shift {
            Chord::Cmd
        } else {
            Chord::Other
        };
        let at = (event.kind != ViewMouseKind::Exited).then(|| self.cell_under(event.x, event.y));
        let app = &mut self.app;
        // Over the scrollbar's strip while it takes a left press, a click
        // scrolls and opens nothing (`bar::left_press`); off the window
        // nothing is under the pointer.
        let strip = match event.kind {
            ViewMouseKind::Exited => Place::Grid,
            _ => {
                let (x, _) = app.frame_px(event.x, event.y);
                let f = app.bar_ctx().frame;
                baud_render::place(x, f.size.0, f.widths)
            }
        };
        app.strip = strip;
        let pressed =
            app.hover
                .set_left_press(crate::bar::left_press(&app.term, strip, app.bar_ctx()));
        let host = app.preview_host(chord);
        let hovered = app
            .hover
            .update(&app.term, at, app.scrolled, chord, app.focus, Host(&host));
        if hovered || pressed {
            app.reshape_pointer();
        }
        if hovered {
            app.pending = Pending::Dirty;
            app.present(&self.view, Show::Event);
        }
    }

    /// Link activation: a Cmd press over an `OSC 8` link or a URI in the
    /// text takes the whole gesture for the terminal, and its release over
    /// the same link opens it. The rules are `crate::link::gesture`'s, and
    /// what is under the pointer `crate::link::under`'s, which reads the
    /// text only for the press and its release; this maps the view's event
    /// onto them and the answer onto the driver's state.
    fn links(&mut self, event: &Mouse) -> Took {
        use crate::link::{Chord, Clicks, Gesture, Press, Spot};
        let press = match (event.kind, event.button) {
            (ViewMouseKind::Down, ViewButton::Left) => Press::Down,
            (ViewMouseKind::Up, ViewButton::Left) => Press::Up,
            (ViewMouseKind::Dragged, _) => Press::Dragged,
            (ViewMouseKind::Exited, _) => Press::Exited,
            _ => Press::Other,
        };
        let chord = if event.modifiers.command && !event.modifiers.shift {
            Chord::Cmd
        } else {
            Chord::Other
        };
        let clicks = if event.clicks <= 1 {
            Clicks::Single
        } else {
            Clicks::Repeat
        };
        let (row, col) = self.cell_under(event.x, event.y);
        // The character under it, half the column on a sized line
        // (`baud_render::char_col`).
        let back = self.app.term.window_back();
        let col = baud_render::char_col(&self.app.term.view(back), row, col);
        // The cell the pointer is over in the *view* -- scrolled back
        // included -- read the way `select` reads it; a wide tail carries
        // its lead's link, so either half resolves.
        let spot = Spot {
            scrolled: self.app.scrolled,
            back,
            row,
            col,
        };
        let armed = self.armed.as_ref();
        let under = crate::link::under(&self.app.term, spot, press, chord, self.app.focus, armed);
        match crate::link::gesture(press, chord, clicks, self.app.focus, armed, under.as_ref()) {
            Gesture::Pass => Took::No,
            Gesture::Arm(a) => {
                self.armed = Some(a);
                Took::Yes
            }
            Gesture::Hold => Took::Yes,
            Gesture::Drop => {
                self.armed = None;
                Took::Yes
            }
            Gesture::Open(link) => {
                self.armed = None;
                self.open_link(&link);
                Took::Yes
            }
        }
    }

    /// Hand a link to the OS, or refuse it and say why. The one call site
    /// of the opener, for an `OSC 8` link and a URI in the text alike, and
    /// it holds a `Target` the policy built rather than a string, so a URI
    /// the policy did not admit cannot reach it.
    fn open_link(&mut self, link: &crate::link::Under) {
        use crate::link::{Action, Host, Under};
        use baud_present_macos::link::{self as opener, Opened};
        let shown = |uri: &[u8]| {
            uri.iter()
                .take(80)
                .copied()
                .collect::<Vec<u8>>()
                .escape_ascii()
                .to_string()
        };
        let uri = match link {
            Under::Osc8(link) => match link.uri(&self.app.term) {
                Some(uri) => uri,
                None => {
                    // Gone: a sweep freed a slot of the link table after
                    // the link was read (`crate::link::Osc8`), so the index
                    // may name another target now; counted, and not
                    // opened.
                    self.app.links.refused.bump();
                    crate::warn!("link refused (gone)");
                    return;
                }
            },
            Under::Bare(bare) => match bare.uri() {
                Ok(uri) => uri,
                Err(why) => {
                    self.app.links.refused.bump();
                    crate::warn!("link refused ({why:?}): {}", shown(bare.text()));
                    return;
                }
            },
        };
        let shown = shown(uri);
        let host = opener::host_name();
        match crate::link::admit(uri, Host(&host)) {
            Err(why) => {
                self.app.links.refused.bump();
                crate::warn!("link refused ({why:?}): {shown}");
            }
            Ok(t) => {
                let done = match t.action() {
                    Action::Open => opener::open(t.as_str()),
                    Action::Reveal => opener::reveal(t.as_str()),
                };
                match (t.action(), done) {
                    (Action::Open, Opened::Taken) => {
                        self.app.links.opened.bump();
                        crate::info!("link opened ({:?}): {shown}", t.scheme());
                    }
                    (Action::Reveal, Opened::Taken) => {
                        self.app.links.revealed.bump();
                        crate::info!("link revealed: {shown}");
                    }
                    (_, other) => {
                        self.app.links.refused.bump();
                        crate::warn!("link refused ({other:?}): {shown}");
                    }
                }
            }
        }
    }

    /// A press, drag or release as a selection: one click a cell, two a
    /// word, three a line, a drag extending by the press's grain, Shift
    /// moving the head and keeping the anchor and the shape, and a click
    /// that never moved clearing what was there. **Option at the press
    /// starts a block**, and wins over Shift: under mouse tracking Shift is
    /// the only way in, so Shift+Option is how a block starts there, where
    /// letting Shift win would extend whatever was already on screen.
    fn select(&mut self, event: &Mouse) {
        let at = self.line_under(event.x, event.y);
        let selected = crate::gesture::select(
            &self.app.term,
            self.app.scrolled,
            &mut self.app.selection,
            &mut self.held,
            (
                mouse_kind(event.kind),
                mouse_button(event.button),
                event.clicks,
            ),
            press_of(event.modifiers),
            at,
        );
        if selected != Selected::Unchanged {
            // The kept text hears of the gesture: a newer selection, or a
            // press or click that took the highlight (`keep::KeptText`).
            self.app.kept.gestured(
                self.app.selection.as_ref(),
                &self.app.term,
                self.app.scrolled,
            );
        }
        match selected {
            Selected::Unchanged => return,
            Selected::Changed => {}
            Selected::Click => self.move_point(event),
            // A selection made goes to the pasteboard, V13's
            // copy-on-select, unless `[clipboard] copy_on_select` is off,
            // and then under `?1041` alone (`clip::selected_to_pasteboard`).
            Selected::Made => {
                if crate::clip::selected_to_pasteboard(
                    &self.app.term,
                    self.app.clipboard.copy_on_select,
                ) {
                    self.copy();
                }
            }
        }
        self.app.pending = Pending::Dirty;
        self.pump(Trigger::Wake);
    }

    /// **A click, to the prompt that asked for it or to `?2001`**
    /// (`gesture::clicked`): only an unmodified left click, the program not
    /// tracking the mouse (a click it tracks never reaches here) and the
    /// window on the live screen.
    fn move_point(&mut self, event: &Mouse) {
        let m = event.modifiers;
        if event.button != ViewButton::Left
            || m.shift
            || m.option
            || m.control
            || m.command
            || self.app.term.window_back().index() != 0
        {
            return;
        }
        let at = self.pointer_under(event.x, event.y);
        let cell_w = cell_px(self.app.renderer.metrics().cell_w);
        let outbox = &mut self.app.outbox;
        crate::gesture::clicked(&mut self.app.term, at, cell_w, |b| outbox.push(b));
        self.app.flush_reply(Asked::Typed);
    }

    /// **Command-Up and Command-Down jump to the previous and the next
    /// prompt** (`Term::jump_to_prompt`), the prompt's first line at the
    /// window's top: Terminal's "Jump to previous mark Command-Up Arrow"
    /// (`[[source: apple-terminal-shortcuts]]`), and Ghostty's macOS default
    /// ("matches Terminal.app", `[[source: ghostty-b58ef1d-config]]`). A
    /// jump reveals the thumb as a wheel's move does.
    fn jump_to_prompt(&mut self, to: PromptJump) {
        if !self.app.term.jump_to_prompt(to) {
            return;
        }
        if let Some(now) = HostTime::of_secs(self.app.tick_target) {
            self.app.reveal.reveal(now);
        }
        if self.app.term.window_back().index() != 0 {
            self.app.slide.snap();
        }
        self.app.pending = Pending::Dirty;
        self.pump(Trigger::Wake);
    }

    /// The scrollbar's strip: the overlay scroller's frame at the grid's
    /// right edge, or the legacy scroller's gutter beside it. Under the
    /// overlay style the pointer over it while scrolled back widens the
    /// thumb and holds it (`reveal`), and a press in it while the thumb is
    /// drawn is the scrollbar's -- on the knob it starts a drag, off it it
    /// jumps the thumb to the pointer -- with nothing drawn it is cells.
    /// Under the legacy style nothing hovers and the gutter is never cells.
    /// Either way a press the scrollbar takes keeps its drags and its
    /// release (`Takes::grabs`), and the window follows through the
    /// inverse geometry.
    fn scrollbar(&mut self, event: &Mouse) -> Took {
        use crate::bar::{At, Did, Held, Hover, Kind, Left, Moved};
        let (x, y) = self.app.frame_px(event.x, event.y);
        let kind = match event.kind {
            ViewMouseKind::Down => Kind::Pressed {
                left: match event.button {
                    ViewButton::Left => Left::Yes,
                    _ => Left::No,
                },
            },
            ViewMouseKind::Dragged => Kind::Moved { held: Held::Yes },
            ViewMouseKind::Moved => Kind::Moved { held: Held::No },
            ViewMouseKind::Up => Kind::Released,
            ViewMouseKind::Entered => Kind::Entered,
            ViewMouseKind::Exited => Kind::Left,
        };
        let c = self.app.bar_ctx();
        let a = crate::bar::press(
            &mut self.bar_hold,
            &mut self.app.reveal,
            &mut self.app.pointer,
            &mut self.app.term,
            At { x, y, kind },
            c,
        );
        if a.hover == Hover::Changed {
            self.app.pending = Pending::Dirty;
        }
        match a.did {
            Did::Nothing | Did::Started | Did::Scrolled(Moved::No) => {}
            Did::Ended => {
                self.app.pending = Pending::Dirty;
                self.pump(Trigger::Wake);
            }
            Did::Scrolled(Moved::Yes) => {
                // Dragging the thumb into history settles a slide, as the
                // wheel does: it names live-screen rows the scrolled-back
                // view lacks.
                if self.app.term.window_back().index() != 0 {
                    self.app.slide.snap();
                }
                self.app.pending = Pending::Dirty;
                self.pump(Trigger::Wake);
            }
        }
        a.took
    }

    /// **The activity lights at the tick's target**: the clocks stepped,
    /// and the view shown, swapped or taken down as `[window]
    /// activity_lights`, the scale, the appearance and Dim Flashing Lights
    /// say (`lights.rs`).
    fn show_lights(&mut self, target: f64) {
        let Some(now) = HostTime::of_secs(target) else {
            return;
        };
        let setting = self.app.activity_lights;
        // DEC's line: whether it shows the words now, at the cell drawn.
        let clause = crate::line_lamps::clause(self.app.metrics_cell(), self.app.scale);
        let line = crate::line_lamps::shown(self.app.status_line_lights, &self.app.term, clause);
        // OH and CD: the job in front, read only while the lights or the
        // line's words are on, and listed often enough for the line's 80 ms.
        let reads = crate::front::Reads::of(setting, line, self.app.interval);
        let shell = self.app.pty.child();
        let readings = self.front.tick(reads, &mut self.procs, shell, now);
        if let (Some((oh, cd)), true) = (readings, setting.shown()) {
            crate::lights::feed(&mut self.app.activity, Led::Oh, oh, now);
            crate::lights::feed(&mut self.app.activity, Led::Cd, cd, now);
        }
        // The line's words, as they are at this tick (`line_lamps.rs`).
        let (oh, cd) = readings.map_or((ReadLamp::default(), ReadLamp::default()), |(oh, cd)| {
            (crate::front::lamp(oh), crate::front::lamp(cd))
        });
        let lamps = crate::line_lamps::read(&self.app.term, &self.app.outbox, cd, oh);
        let given = crate::line_lamps::given(self.app.status_line_lights, clause, lamps);
        if crate::line_lamps::give(&mut self.app.term, given) == crate::line_lamps::Owes::Frame {
            self.app.pending = Pending::Dirty;
        }
        // SYN and FAX: the core's state at the tick, SYN's reading taken
        // whether or not the lights show, so showing them again lights
        // nothing opened while they were away. CS: dark while bytes are
        // owed the program that the kernel has not taken.
        let synced = self.synced.seen(&self.app.term);
        if setting.shown() {
            if synced {
                self.app.activity.seen(Led::Syn, now);
            }
            if self.app.term.images().any_placed() {
                self.app.activity.seen(Led::Fax, now);
            }
            crate::line_lamps::feed_cs(&mut self.app.activity, &self.app.outbox, now);
        }
        let pace = crate::access::lights_pace(self.app.motion);
        let shown = self.app.activity.step(now, pace);
        let glow = crate::lights::glow(self.app.flashing);
        let wanted = crate::lights::Drawn {
            scale: self.app.scale,
            look: self.app.renderer.lights_look(glow),
        };
        self.lights.show(
            setting,
            wanted,
            shown,
            &self.app.renderer,
            self.strip_edges(),
            self.icon_inset,
        );
        // The lights know their width only once they are drawn, and the
        // lockup stands on their plate, or beside the traffic lights with
        // no plate, and the build's name gives way to them.
        if self
            .icon
            .as_ref()
            .is_some_and(|m| m.lights_moved(self.lights.fitted()))
        {
            self.place_lights();
        }
        // **On their own line, the frame's**: the renderer draws the same
        // pictures there, and a light that changed owes a frame, which
        // names another picture and draws none (`Renderer::place_lights`).
        let line = setting
            .line()
            .map(|_| (shown, crate::lights::glow(self.app.flashing)));
        if line != self.app.line_shown {
            self.app.line_shown = line;
            if let Some((shown, glow)) = line {
                self.app.renderer.set_lights(shown, glow);
            }
            self.app.pending = Pending::Dirty;
        }
        self.place_tips();
    }

    /// **Where the lights are, for their hover text**, told to the tip,
    /// which each frame settles again (`line_tip.rs`).
    fn place_tips(&mut self) {
        let lights = self.lights_at();
        self.app.line_tip.set_lights(lights);
    }

    /// **Where the lights are, in the terminal view's device pixels**: on
    /// their own line, where the last frame placed them, the whole frame
    /// centred in the view as `grid_origin` centres it; in the title strip,
    /// each shown light's frame on the screen (`Strip::screen_frames`),
    /// through whichever window holds it, brought into the view through the
    /// terminal's window: above the view in a window, over its top in full
    /// screen, where the title bar is a window of its own. None with the
    /// lights off.
    fn lights_at(&self) -> Option<crate::line_tip::LightsAt> {
        let r = &self.app.renderer;
        let whole = r.whole_size();
        let o = grid_origin(self.app.view_px, whole, Px::of(0), Px::of(0));
        let px = |v: Px| f64::from(v.value());
        let corner = (px(o.0), px(o.1));
        if let Some(line) = crate::line_tip::LightsLine::drawn(r) {
            return Some(line.at(corner));
        }
        let frames = self.lights.screen_frames();
        let w = self.window.ns_window();
        let in_view = baud_render::ByLed::of(|led| {
            (*frames.get(led)).map(|s| baud_present_macos::hover::in_view(w, &self.view, s))
        });
        let m = r.metrics();
        strip_lights(
            in_view,
            self.app.scale,
            (corner.0 + px(r.strip()), corner.1 + px(r.grid_top())),
            (px(m.cell_w), px(m.cell_h)),
            corner.0 + px(whole.0),
        )
    }

    /// **The tip at view point `at`**: the cell under it, the line's row
    /// counted, or none where the pointer has left the view; and its pixel,
    /// for the lights, which a light in the title strip is above the view.
    fn tip_at(&mut self, at: (f64, f64), in_view: InView) {
        let (scale, cell, grid, pad) = self.pointer_geometry();
        let status = self.app.term.status_rows();
        let over = match in_view {
            InView::Left => crate::line_tip::Over::Elsewhere,
            InView::Yes => crate::line_tip::over_at(at, scale, cell, grid, pad, status),
        };
        let lights = self.lights_at();
        self.app.line_tip.set_lights(lights);
        let pixel = Some((at.0 * scale, at.1 * scale));
        if self.app.line_tip.update_at(&self.app.term, over, pixel) == crate::line_tip::Changed::Yes
        {
            self.app.pending = Pending::Dirty;
        }
    }

    /// **The pointer crossing a light's edge in the title strip**, heard
    /// by the lights' tracking areas since the last turn: the tip settled
    /// where the pointer is now, brought into the view, so it shows on the
    /// frame after the pointer reaches a light. Now and not where it
    /// crossed: the wake that brings a crossing can come after moves the
    /// view already heard, and its older point would bring back a tip the
    /// pointer has left.
    fn lights_crossed(&mut self) {
        if self.lights.crossed().is_empty() {
            return;
        }
        let now = self.window.ns_window().mouseLocationOutsideOfEventStream();
        let v = self.view.convertPoint_fromView(now, None);
        let in_view = in_view_at(self.view.bounds(), v);
        self.tip_at((v.x, v.y), in_view);
    }

    /// **The title strip's ends now** (`strip.rs`): the window's width,
    /// the traffic lights' end and the plate's gap at the right, read from
    /// the window as AppKit has laid it out.
    fn strip_edges(&self) -> crate::strip::Edges {
        crate::strip::edges(self.window.ns_window())
    }

    /// **The activity lights' pictures, and the plate, before the window's
    /// first tick**, at the state they are in.
    fn draw_lights(&mut self) {
        let glow = crate::lights::glow(self.app.flashing);
        let wanted = crate::lights::Drawn {
            scale: self.app.scale,
            look: self.app.renderer.lights_look(glow),
        };
        let pace = crate::access::lights_pace(self.app.motion);
        let shown = self.app.activity.step(host_now(), pace);
        self.lights.show(
            self.app.activity_lights,
            wanted,
            shown,
            &self.app.renderer,
            self.strip_edges(),
            self.icon_inset,
        );
    }

    /// The activity lights follow the window's top-right on every resize,
    /// and the lockup stands on their plate, or beside the traffic lights
    /// with no plate; the build's name gives way to them first
    /// (`Mark::fit`).
    fn place_lights(&self) {
        let plate = self.lights.place(self.strip_edges());
        if let Some(mark) = &self.icon {
            mark.fit(self.window.ns_window(), plate, self.lights.fitted());
        }
    }

    /// Re-centre the titlebar lockup in its accessory, whose height AppKit
    /// sets, with no plate. Cheap -- one `setFrame` -- and it runs on every
    /// resize so the lockup cannot drift or vanish.
    fn place_icon(&self) {
        if let Some(mark) = &self.icon {
            mark.place();
        }
    }

    /// A program's page width, which the grid took inside the read that
    /// asked (`honour`): the rest of what `relayout` does on a change of
    /// grid, then the window, to the columns and at its height, once this
    /// callback has returned (`widen_later`). Its resize callback then lays
    /// out the grid the core already has. **The trace records the resize
    /// after the read that asked**, whose bytes a replay feeds whole at the
    /// old width: its events are reads, and the request fell inside one.
    fn program_resized(&mut self, grid: crate::width::Grid) {
        let ws = grid.ws();
        self.app.chase.snap();
        self.app.carried.forget();
        self.app.predictions.clear();
        self.app.guess = Guess::None;
        self.app.slide.snap();
        if let Some(t) = self.app.trace.as_mut() {
            t.resized(grid.ws());
        }
        self.app.tell_pty();
        self.app.grid = ws;
        self.app.selection = None;
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        let scale = self.app.scale;
        let bar = shown_gutter(
            self.app.scroller,
            self.app.bar_shown,
            crate::reveal::widths(Scale(scale)),
        );
        let (cw, _) = self.app.metrics_cell();
        let width =
            crate::width::content_width(grid, cw, scale, self.app.padding, self.app.beside(bar));
        widen_later(self.window.ns_window(), width);
    }

    /// **`?30`, a program showing or hiding the scrollbar** ("Show
    /// scrollbar (rxvt)", `[[source: ctlseqs]]`), acted on after the read
    /// that changed it: hidden, nothing is drawn and nothing takes a
    /// press, a drag in flight dropped; under the legacy style the gutter
    /// goes, the window narrowing by it so the grid keeps its columns, as
    /// xterm's window does, or, where the window may not change size, the
    /// grid taking the columns ([`crate::reveal::bar_change`]). Shown, the
    /// reverse.
    fn scrollbar_mode(&mut self) {
        use crate::reveal::BarChange;
        use crate::winops::Full;
        let shown = self.app.term.scrollbar_shown();
        if shown == self.app.bar_shown {
            return;
        }
        self.app.bar_shown = shown;
        self.bar_hold.end_drag();
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        let full = match self.view.window() {
            Some(w) if w.styleMask().contains(NSWindowStyleMask::FullScreen) => Full::Yes,
            _ => Full::No,
        };
        match crate::reveal::bar_change(self.app.scroller, full, self.app.term.window_ops()) {
            BarChange::Redraw => {}
            // The grid kept: the window to its columns and the new gutter,
            // whose resize callback then lays out the grid it already has.
            BarChange::Resize => {
                let scale = self.app.scale;
                let bar = shown_gutter(
                    self.app.scroller,
                    shown,
                    crate::reveal::widths(Scale(scale)),
                );
                let grid = crate::width::Grid::of(&self.app.term);
                let (cw, _) = self.app.metrics_cell();
                let width = crate::width::content_width(
                    grid,
                    cw,
                    scale,
                    self.app.padding,
                    self.app.beside(bar),
                );
                widen_later(self.window.ns_window(), width);
            }
            BarChange::Relayout => {
                let _ = self.relayout(self.view.frame().size, self.app.scale);
            }
        }
    }

    /// **The status line shown or taken away** (DECSSDT, `status.rs` in the
    /// core), acted on after the read that changed it: the window grows or
    /// shrinks by the line's row, the grid keeping its rows, so a program
    /// asking for the size is told the main display's; where the window may
    /// not change size, or the screen has no room for the row, the grid
    /// gives the row up, and takes it back when the line goes
    /// ([`crate::winops::mac_status_change`]). Returns the window the
    /// line's frame leaves, for this read's window operations.
    fn status_line(&mut self) -> Option<crate::winops::MacWindow> {
        use crate::winops::StatusChange;
        let now = self.app.term.status_rows();
        if now == self.app.status_rows {
            return None;
        }
        self.app.status_rows = now;
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        let (Some(w), Some(mtm)) = (self.view.window(), MainThreadMarker::new()) else {
            return None;
        };
        let at = self.mac_window(&w, mtm);
        let ops = self.app.term.window_ops();
        let change = crate::winops::mac_status_change(&at, ops, self.status_took);
        if now != Rows::ZERO {
            self.status_took = change;
        }
        match change {
            // The window to the grid's rows and the line's, whose resize
            // callback then lays out the grid it already has.
            StatusChange::Resize => {
                let (frame, left) = crate::winops::mac_status_frame(at);
                window_later(&w, frame);
                Some(left)
            }
            StatusChange::Relayout => {
                let _ = self.relayout(self.view.frame().size, self.app.scale);
                None
            }
        }
    }

    /// **The activity lights' own line put up, taken down or resized**
    /// (`[window] activity_lights`), its pixels `was` before: the window
    /// grows or shrinks by the difference and the grid keeps its rows, as
    /// it does for the status line's row; where the window keeps its size
    /// the grid gives the pixels up or takes them back
    /// ([`crate::winops::mac_line_change`]).
    fn lights_line(&mut self, was: Px) {
        use crate::winops::StatusChange;
        let now = self.app.renderer.lights_height();
        if now == was {
            return;
        }
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        self.app.line_shown = None;
        let resize = match (self.view.window(), MainThreadMarker::new()) {
            (Some(w), Some(mtm)) => {
                let at = self.mac_window(&w, mtm);
                match crate::winops::mac_line_change(&at, f64::from(was.value())) {
                    StatusChange::Resize => Some((w, at)),
                    StatusChange::Relayout => None,
                }
            }
            _ => None,
        };
        match resize {
            Some((w, at)) => {
                window_later(&w, crate::winops::line_frame(&at, f64::from(was.value())))
            }
            None => {
                let _ = self.relayout(self.view.frame().size, self.app.scale);
            }
        }
    }

    /// The grid for a view of `size` points at `scale`, and the terminal,
    /// the child and the frame following it -- on a resize, and on a zoom.
    fn relayout(&mut self, size: NSSize, scale: CGFloat) -> Drew {
        // **A resize no longer waits for the frame in flight, and that is
        // the freeze a user reported.** It used to: a reflow rewrites the
        // ring, and while the shader read the ring in place that was
        // memory a live draw was sampling.
        //
        // What made the wait a freeze is what else is on this path.
        // `pty.resize` -- the `SIGWINCH` that tells the child its new size
        // -- is below, so a deferred step never told it. A drag delivers a
        // step every few milliseconds and a frame is in flight for most of
        // them, so through the whole gesture the child kept redrawing at
        // the size it was first told, and only the last step got through:
        // a window that grows around a screenful of stale content and
        // catches up a moment after the pointer stops. Exactly the report.
        //
        // Since D4 the shader reads a *copy* of the window taken before
        // the draw, so the reflow has nothing to collide with -- the frame
        // in flight is reading its own buffer. The surfaces are replaced
        // under the journal's epoch, which supersedes the frames drawn for
        // the old ones, and Metal retains a texture until the command
        // buffer that references it completes.
        //
        // The atlas is the one thing here that still has no ring, and a
        // resize does not touch it: only a rescale does, which is why
        // `Deferred::Zoom` still waits.
        // A resize or a zoom moves every cell's pixels under the quad trail,
        // which names them; the next sighting starts afresh (`trail.rs`).
        self.app.chase.snap();
        self.app.carried.forget();
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
        // The titlebar mark follows the window's top-left on every resize,
        // and the activity lights its top-right.
        self.place_icon();
        self.place_lights();
        // The scroller the setting asks for now, and the gutter the frame
        // keeps for it -- before either early return below, because a frame
        // that changed width is a new frame whether or not the grid did.
        self.app.scroller = scroller_style();
        let bar = shown_gutter(
            self.app.scroller,
            self.app.bar_shown,
            crate::reveal::widths(Scale(scale)),
        );
        let reframed = self.app.renderer.frame_size().0.value()
            != self.app.renderer.pixel_size().0.value() + bar.value();
        self.app.renderer.set_gutter(bar);
        // The lights' own line at this scale, its points' pixels.
        let line = self.app.renderer.lights_height();
        self.app
            .renderer
            .set_lights_line(self.app.activity_lights.line(), scale as f32);
        let reframed = reframed || line != self.app.renderer.lights_height();
        if reframed {
            self.app.next_damage = Damage::Full;
            self.app.pending = Pending::Dirty;
        }
        if self.app.sizing == Sizing::Fixed {
            // The grid is pinned, and a zoom still moves the pixels.
            self.app.tell_pty();
            return Drew::No;
        }
        // **The status line's row is the window's, below the grid**: the
        // grid is what is left, and a program asking for the size is told
        // it (`Term::status_rows`). So are the activity lights' line's
        // pixels, which are no row at all.
        let ws = crate::width::less_status(
            grid_in(
                (
                    px_w,
                    crate::width::less_line(px_h, self.app.renderer.lights_height()),
                ),
                self.app.padding * scale,
                self.app.beside(bar),
                (cw, ch),
            ),
            below_grid(&self.app.term, &self.app.renderer),
        );
        if (ws.ws_col, ws.ws_row) != (self.app.grid.ws_col, self.app.grid.ws_row) {
            let t_step = Instant::now();
            // Row 9: a prediction names a cell of the screen about to be
            // re-cut, and a reflow moves every one of them. The clear
            // belongs *here*, at the one place the grid changes shape,
            // rather than in `resized`: `zoom` re-lays through this
            // function too, and a prediction that survived it was drawn
            // and judged at whatever cell its old row and column now
            // name -- `Viewport::row` takes the row modulo the ring, so
            // a stale row is a wrong cell rather than a panic, and the
            // glyph would sit somewhere else until the next read
            // contradicted it.
            self.app.predictions.clear();
            self.app.guess = Guess::None;
            // A slide in flight names a leaving row of the ring about to be
            // re-cut: the same reason predictions are cleared here. Settle it
            // rather than draw a leaving row from the reflowed ring, which
            // would be a different line for the frame or two before it drains.
            self.app.slide.snap();
            // Truncated only past MAX_GRID, which the clamp above prevents.
            // The core holds a window that is *already* looking back on its
            // line, through the re-cut, and leaves one on the live screen
            // where it is -- a resize never changes the scroll position,
            // which a zoom that repeated a banner four times broke when the
            // app anchored it (`Term::resize`). `scrolled` is moved by what
            // the resize retired or took back, when the selection is kept
            // (`kept_across_resize`).
            let was = (
                self.app.term.history_mark(),
                self.app.term.geometry().0,
                self.app.term.screen(),
            );
            let _ = self.app.term.resize(&ws);
            if let Some(t) = self.app.trace.as_mut() {
                // What the core *has*, post-clamp, which is what a replay
                // must ask it for.
                let (c, r) = self.app.term.geometry();
                t.resized(WinSize {
                    ws_col: u16::try_from(c.index()).unwrap_or(u16::MAX),
                    ws_row: u16::try_from(r.index()).unwrap_or(u16::MAX),
                });
            }
            let t_reflow = t_step.elapsed();
            // **The child is told what the terminal has, not what the
            // window asked for.** `Term::resize` clamps: columns to
            // `MAX_COLS` and to the pitch the rows are laid at, rows to
            // `MAX_ROWS` and to the ring's row capacity. Sending the
            // *request* told a full-screen program about a grid that does
            // not exist -- and a program believes it. btop drew more rows
            // than the screen has, so its own output scrolled and three
            // copies of its lower panels stacked down the window at three
            // different sizes; asciiquarium drew its waterline past the
            // last column and the waves stopped short of the edge while
            // the fish it drew inside the grid reached it. Two reports,
            // one line.
            let asked = (ws.ws_col, ws.ws_row);
            let (has_cols, has_rows) = self.app.term.geometry();
            let ws = WinSize {
                ws_col: u16::try_from(has_cols.index()).unwrap_or(u16::MAX),
                ws_row: u16::try_from(has_rows.index()).unwrap_or(u16::MAX),
            };
            // The `SIGWINCH` is sent on the drag event itself, not deferred:
            // a full-screen program redraws into the new geometry as the
            // window moves, so it fills the rows the reflow opened rather
            // than leaving them blank until the drag stops -- the black gaps
            // a settle-delay left in a live resize.
            // `tell_pty` flushes the `?2048` report `Term::resize` queued,
            // so it leaves with the `SIGWINCH`; see there.
            self.app.tell_pty();
            self.app.grid = ws;
            // A reflow re-cuts every line, so the columns a selection named
            // no longer name cells; a change of height alone on the primary
            // keeps it on its lines (note V, V3).
            (self.app.selection, self.app.scrolled) = kept_across_resize(
                &self.app.term,
                was,
                self.app.selection.take(),
                self.app.scrolled,
            );
            // The find's matches on the re-laid grid, and its current
            // match selected again, before the frame below draws them: a
            // silent child sends no read to bring them up to date.
            if let Some(f) = self.app.finding.as_mut() {
                if let Some(sel) = f.after_resize(&self.app.term, self.app.scrolled) {
                    self.app.selection = Some(sel);
                }
            }
            // A new grid is a new surface, so repaint every row of it rather
            // than trusting the last frame, whose rows were a different width.
            self.app.next_damage = Damage::Full;
            self.app.pending = Pending::Dirty;
            // Ask for a frame at the new size, asynchronously -- nothing
            // waits. The GPU draws off the main thread and its completion
            // hands the surface to the layer through a wake delivered in the
            // run loop's *common* modes, so it lands during the modal resize
            // loop rather than sitting queued until the drag ends. A late
            // frame is a size behind the window for an instant and the layer
            // clips it top-left (`contentsGravity`) rather than stretching, so
            // it is a thin sliver at the moving edge, not a freeze. If the GPU
            // is still busy this step builds nothing (`builds`) and the
            // in-flight frame's completion drives the next; either way the
            // main thread returns at once and keeps tracking the drag.
            // `term.resize` kept the content -- the pitch is fixed, each row
            // clipped or extended in place -- and the child redraws into it
            // on `SIGWINCH`.
            let t_present = Instant::now();
            self.app.present(&self.view, Show::Event);
            if std::env::var_os("BAUD_DEBUG_RESIZE").is_some() {
                let g = &self.app.gpu_stats;
                // The zone and the cursor, because which rule a resize took
                // is not visible from the timings: a width change re-cuts
                // in `Zone::Output` and does not in `Zone::Prompt`, and a
                // duplicated prompt is what the wrong one looks like.
                eprintln!(
                    "resize asked {}x{} got {}x{} zone {:?} prompt {:?} cursor {:?} marks {}",
                    asked.0,
                    asked.1,
                    has_cols.index(),
                    has_rows.index(),
                    self.app.term.zone(),
                    self.app.term.prompt_row(),
                    self.app.term.cursor(),
                    self.app.term.semantic_marks,
                );
                eprintln!(
                    "resize {}x{}: step reflow {}us, present {}us (convert {}us, upload {}us, submit {}us) [{}]",
                    self.app.grid.ws_col,
                    self.app.grid.ws_row,
                    t_reflow.as_micros(),
                    t_present.elapsed().as_micros(),
                    self.app.stats.convert.as_micros(),
                    g.upload.as_micros(),
                    g.submit.as_micros(),
                    self.app.present_counts(),
                );
            }
            return Drew::Yes;
        }
        // The window moved but the grid did not: a sub-cell drag step, or a
        // zoom that landed on the same grid -- which changed every pixel of
        // the text area, so the pty is told; a drag step changed nothing,
        // and `Pty::resize` sends nothing.
        self.app.tell_pty();
        // The scroller's style changed and the columns happened not to: the
        // frame is still a different width, so it is drawn now.
        if reframed {
            self.app.present(&self.view, Show::Event);
            return Drew::Yes;
        }
        Drew::No
    }

    /// The View menu's zoom: a new font size in points, bounded, the
    /// renderer rescaled to it at the current backing scale, the grid
    /// re-laid for the same window, and a frame drawn now. The size is not
    /// written back to the config; Actual Size returns to what it said.
    /// A View menu zoom: the size the step lands on from the size ahead
    /// (`fonts::zoomed`), the rule the Linux zoom keys take.
    fn zoom_step(&mut self, step: crate::fonts::Zoom) {
        let points = crate::fonts::zoomed(step, self.size_ahead(), self.app.font_base);
        self.zoom(points);
    }

    /// The font at `points`, held within the sizes a zoom may reach.
    fn zoom(&mut self, points: f32) {
        self.refont(None, points.clamp(ZOOM_MIN, ZOOM_MAX));
    }

    /// The size a step is taken from: the one a waiting change will put in
    /// force, not the one that is. While a zoom waits for a frame in
    /// flight, `font_size` still names what the renderer is built at, so
    /// stepping from it would make every click of a burst aim at the same
    /// size and twelve of them arrive as one.
    fn size_ahead(&self) -> f32 {
        match self.deferred {
            Deferred::Zoom(points) | Deferred::Font(_, points) => points,
            Deferred::Nothing => self.app.font_size,
        }
    }

    /// **A program's font changes** (`OSC 50`), each planned whole against
    /// the faces loaded and the size bounds (`fonts.rs`) and performed as
    /// the View menu's zoom is; one that changes nothing is counted in the
    /// core (`host::plan_font`) and said at info.
    fn font_requests(&mut self, asked: &[baud_core::FontChange]) {
        for c in asked {
            let at = self.size_ahead();
            let app = &mut self.app;
            let plan = crate::host::plan_font(
                &mut app.term,
                app.renderer.fonts_mut(),
                c,
                at,
                app.font_size,
                app.font_base,
            );
            if let Some(plan) = plan {
                self.refont(plan.face, plan.points);
            }
        }
    }

    /// The font led by `face`, or the one in force, at `points`: the
    /// renderer restacked or rescaled, the grid re-laid for the same
    /// window, the core told, and a frame drawn now.
    fn refont(&mut self, face: Option<crate::fonts::Slot>, points: f32) {
        if std::env::var_os("BAUD_DEBUG_RESIZE").is_some() {
            eprintln!(
                "zoom: asked {points}, at {}, busy {:?}",
                self.app.font_size,
                self.app.presenter.as_ref().map(Presenter::in_flight)
            );
        }
        // The rescale itself is safe mid-flight now: the glyph map is built
        // over the other buffer and the atlas builds a replaced sheet
        // afresh, so neither writes what a live draw is reading. What still
        // waits is that a zoom *re-lays the grid*, and a reflow rewrites the
        // ring the shader reads. The two have to land together -- a frame
        // drawn at the new cell size against the old column count is neither
        // size -- so the whole step waits for the frame that is reading.
        let reshape = self.app.presenter.as_ref().map(Presenter::reshape);
        let fonts = self.app.renderer.fonts();
        let step = refont_step(
            self.deferred,
            reshape,
            face,
            points,
            self.app.font_size,
            |f| fonts.leads(f),
        );
        let face = match step {
            Refont::Nothing => return,
            Refont::Wait(d) => {
                self.deferred = d;
                return;
            }
            Refont::Now(face) => face,
        };
        let scale = self.app.scale;
        let Some(px) = PxSize::of(points * scale as f32) else {
            return;
        };
        let done = match face {
            Some(f) => self.app.restack(f, px),
            None => self.app.rescale(px),
        };
        if done.is_none() {
            return;
        }
        self.app.font_size = points;
        crate::host::tell_font(
            &mut self.app.term,
            self.app.renderer.fonts(),
            points,
            self.app.font_base,
        );
        // Full damage before the relayout, so the frame it draws is the
        // one at the new size: `zoom` used to ask for a second present
        // afterwards, which always found the first still in flight and so
        // drew nothing. Two renders on a font change, the first at the old
        // size, is what that looked like from outside.
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        if self.relayout(self.view.frame().size, scale) == Drew::No {
            // The grid did not change -- a fixed size, or a step too small
            // to move the column count -- so nothing has drawn the new
            // font yet.
            self.app.present(&self.view, Show::Event);
        }
    }

    /// Take what the style observer left and repaint in the new palette.
    ///
    /// **A theme swap moves no cell word and no `StyleId`**, which is what
    /// row damage compares, so the full damage below is what makes the
    /// screen change at all -- the same reason a blink phase flip forces
    /// one. `Renderer::set_theme` drops the renderer's own damage cache;
    /// these two lines are the frame the driver owes on top of it.
    fn apply_style(&mut self) {
        let Some(appearance) = self.style.0.take() else {
            return;
        };
        // The activity lights are drawn again in the theme the appearance
        // brings (`lights.rs`).
        self.lights.stale();
        // **With no plate the lockup's word follows the window's
        // appearance**, the one AppKit draws the traffic lights in, before
        // the early return: a pinned palette is the terminal's, the glass in
        // the title strip the window's. On a plate this leaves it, and the
        // next tick fits it to the plate the theme brings (`mark.rs`).
        if let Some(mark) = &self.icon {
            mark.follow_window(self.window.ns_window());
        }
        // The unemphasized pair is the appearance's, so it follows a flip
        // while the window is not key.
        self.inactive_selection(self.app.focus);
        // **The material before the early return, because the glass is the
        // window's and the palette is the terminal's.** A reader who pins
        // `theme = "dark"` has chosen a palette, not a window: the
        // material still follows the system, and returning above this
        // line left it frozen at whatever the launch resolved.
        // The material's own axis, not the one that just changed: under
        // an `Auto` widget token the two coincide and this is the re-apply
        // that case needs, and under any other token it is the same value
        // the observer last set.
        apply_glass(
            &self.glass,
            &self.window,
            &self.glass_cfg,
            self.glass.appearance(&self.window),
        );
        let PaletteSwitch::To(appearance) =
            palette_switch(self.theme_choice, self.palette_for, appearance)
        else {
            // No frame is owed: the material is AppKit's own drawing and
            // baud's surface did not change, because a pinned palette, or
            // one already set for this appearance, paints the same cells at
            // the same dim. The lines below are the repaint the *palette*
            // owes, and this branch has no palette change to repaint.
            return;
        };
        self.palette_for = Some(appearance);
        self.show_theme(appearance);
        // The one place a palette change drops the application's colours.
        crate::host::tell_defaults(
            &mut self.app.term,
            self.app.renderer.theme(),
            baud_core::Overrides::Drop,
        );
        self.app.next_damage = Damage::Full;
        self.app.pending = Pending::Dirty;
        self.app.present(&self.view, Show::Event);
    }

    /// **A find chord**: Command-F opens the field, on the query it holds
    /// or, closed, on the find pasteboard's string, selected; Command-E
    /// opens it on the selection's text; Command-G and Shift-Command-G
    /// step the current match, reopening the field first when it is
    /// closed, from the match it closed on.
    fn find_chord(&mut self, chord: crate::find::Chord) {
        use crate::find::{Chord, Finding};
        let selected = match chord {
            Chord::UseSelection => self.app.selection.map(|s| {
                s.text_as(
                    &self.app.term,
                    self.app.scrolled,
                    self.app.renderer.word_wrap(),
                )
            }),
            _ => None,
        };
        match (self.app.finding.as_mut(), selected) {
            (_, Some(text)) => {
                let query = crate::marked::codepoints(&text);
                self.app.finding = Some(Finding::open(&self.app.term, query));
            }
            (Some(f), None) => f.chord(chord),
            // Closed: reopened on the find pasteboard's string, the last
            // search here or in another application, and stepped from the
            // match it closed on (`Finding::reopen`).
            (None, None) => {
                let query = crate::marked::codepoints(&clipboard::read_find().unwrap_or_default());
                self.app.find_written = query.clone();
                self.app.finding = Some(Finding::reopen(
                    &self.app.term,
                    query,
                    self.app.find_left.as_ref(),
                    chord,
                ));
            }
        }
        self.find_show();
    }

    /// A key in the open field: the query edited, the current match
    /// stepped, or the field closed, the selection left on what it found.
    fn find_key(&mut self, key: crate::find::FieldKey) {
        let Some(f) = self.app.finding.as_mut() else {
            return;
        };
        if f.key(&self.app.term, key) == crate::find::Open::Closed {
            self.app.find_left = Some(f.left());
            self.app.finding = None;
            self.app.pending = Pending::Dirty;
            self.app.present(&self.view, Show::Event);
            return;
        }
        self.find_show();
    }

    /// **The current match shown**: the window moved over the history to
    /// it, which reveals the thumb, and selected; a frame now.
    fn find_show(&mut self) {
        let Some(f) = self.app.finding.as_ref() else {
            return;
        };
        // The query to the find pasteboard as it changes, as a find
        // field's search string goes there (`[[source:
        // iterm2-finddriver-654bb13]]` 480-496); an empty one is no search.
        let query = f.find().query();
        if !query.is_empty() && query != self.app.find_written.as_slice() {
            let text: String = query
                .iter()
                .filter_map(|c| char::from_u32(c.value()))
                .collect();
            let _ = clipboard::write_find(&text);
            self.app.find_written = query.to_vec();
        }
        let back = self.app.term.window_back();
        let selection = f.show(&mut self.app.term, self.app.scrolled);
        if selection.is_some() {
            self.app.selection = selection;
        }
        if self.app.term.window_back() != back {
            self.app.travel.settle();
            if let Some(now) = HostTime::of_secs(self.app.tick_target) {
                self.app.reveal.reveal(now);
            }
        }
        self.app.pending = Pending::Dirty;
        self.app.present(&self.view, Show::Event);
    }

    /// Cmd-C: the selection's text to the pasteboard, if there is one --
    /// the text taken when it was made while `?1040` kept it, however a
    /// read carried it since; without one, the text output or a click took
    /// a selection's highlight away from while `?1040` kept it
    /// (`keep::KeptText`).
    fn copy(&mut self) {
        let app = &self.app;
        let (sel, wrap) = (app.selection.as_ref(), app.renderer.word_wrap());
        if let Some(text) = crate::keep::copied(sel, &app.kept, &app.term, app.scrolled, wrap) {
            clipboard::write_text(&text);
        }
    }

    /// Cmd-V: the pasteboard's string to the child, bracketed when it
    /// asked (`?2004`), through the channel keys take.
    fn paste(&mut self) {
        let Some(text) = clipboard::read_text() else {
            return;
        };
        // A paste while the find field is open goes into the query.
        if let Some(f) = self.app.finding.as_mut() {
            f.typed(&self.app.term, &crate::marked::codepoints(&text));
            self.find_show();
            return;
        }
        self.snap_to_live();
        self.app.caret.input(host_now());
        // Through the outbox, behind anything owed and ahead of anything
        // typed after it: a paste longer than the slave's queue -- 1022
        // bytes on Darwin -- waited in `write(2)` for the child to read it.
        self.app.flush_reply(Asked::Unasked);
        let pty = &self.app.pty;
        let form = crate::paste::form(&self.app.term, || pty.literal_next(), || pty.specials());
        self.app.outbox.push(&crate::paste::bytes(&text, form));
        self.app.flush_reply(Asked::Typed);
    }

    /// The cell under a point in the view, clamped to the grid: the view
    /// is the terminal's area below the titlebar inset, in points, and the
    /// grid is in pixels of the cell size.
    fn cell_under(&self, x: f64, y: f64) -> (Row, Col) {
        let (scale, cell, grid, pad) = self.pointer_geometry();
        let (row, col) = cell_under((x, y), scale, cell, grid, pad);
        // The logical cell the visual one shows (`gesture::logical`).
        self.app.renderer.bidi().pointer(row, col)
    }

    /// **The stored cell under a point, for a selection**: [`cell_under`]'s,
    /// but a cell the display's word wrap draws from a line past the window
    /// is named by that line (`baud_render::Orders::pointer_line`).
    fn line_under(&self, x: f64, y: f64) -> (baud_render::WindowLine, Col) {
        line_under(self.app.renderer.bidi(), (x, y), self.pointer_geometry())
    }

    /// **The wheel across**: a tilt, Shift with a mouse wheel, a sideways
    /// swipe. Whole columns of the cell width, positive `dx` toward the
    /// start of the line as positive `dy` is toward the top -- AppKit's
    /// swipe note pairs 1 with "swipe left" and "swipe up" (`[[source:
    /// macos-sdk-nsevent-h]]`) -- to a program tracking the mouse as buttons
    /// 6 and 7, unless Shift's override takes it as it takes the vertical
    /// wheel, and nothing otherwise (`wheel::route_on`).
    fn scroll_across(&mut self, event: &Scroll) {
        let (cw, _) = self.app.metrics_cell();
        let cols = self.app.travel_across.lines(match event.precise {
            true => Delta::Points {
                dy: event.dx,
                scale: self.app.scale,
                cell: cw,
            },
            false => Delta::Lines(event.dx),
        });
        if cols.get() == 0 {
            return;
        }
        let mods = mouse_mods(event.modifiers);
        let axis = crate::wheel::Axis::Horizontal;
        if crate::wheel::route_on(&self.app.term, mods, axis) != Route::Report {
            return;
        }
        let at = self.pointer_under(event.x, event.y);
        let outbox = &mut self.app.outbox;
        crate::wheel::report_on(&mut self.app.term, axis, cols, at, mods, |b| outbox.push(b));
        self.app.flush_reply(Asked::Typed);
    }

    /// The cell under a view point and the pixel it is at: what a mouse
    /// report needs, `?1016`'s pixel included.
    fn pointer_under(&self, x: f64, y: f64) -> (Row, Col, PointerPx) {
        let (scale, cell, grid, pad) = self.pointer_geometry();
        crate::gesture::logical(
            self.app.renderer.bidi(),
            pointer_under((x, y), scale, cell, grid, pad),
            cell,
        )
    }

    /// What telling DEC's locator left owed: its report written, and a
    /// frame for the indicator line.
    fn locator_owed(&mut self, owed: LocatorOwed) {
        if owed.reply == Reply::Queued {
            self.app.flush_reply(Asked::Typed);
        }
        if owed.frame == LineFrame::Owed {
            self.app.pending = Pending::Dirty;
        }
    }

    /// The same point for DEC's locator: `None` off the text area.
    fn locator_under(&self, x: f64, y: f64) -> Option<LocatorAt> {
        let (scale, cell, grid, pad) = self.pointer_geometry();
        locator_under((x, y), scale, cell, grid, pad).map(|at| {
            let px = at.px.unwrap_or(PointerPx {
                x: ImagePx::ZERO,
                y: ImagePx::ZERO,
            });
            let (row, col, px) =
                crate::gesture::logical(self.app.renderer.bidi(), (at.row, at.col, px), cell);
            LocatorAt {
                row,
                col,
                px: at.px.map(|_| px),
            }
        })
    }

    /// What turns a view point into a cell and a pixel.
    fn pointer_geometry(&self) -> PointerGeometry {
        (
            self.app.scale,
            self.app.metrics_cell(),
            (self.app.grid.ws_col, self.app.grid.ws_row),
            (self.app.pad_px.0.value(), self.app.pad_px.1.value()),
        )
    }
}

/// **The band of the window's padding each side lends the word wrap's
/// marks**, in device pixels: the padding at `scale`, or half of what the
/// view leaves beside the grid's frame and the strip where that is less.
fn margin_band(view_w: Px, r: &Renderer<'_, '_>, padding: f64, scale: CGFloat) -> Px {
    let beside = r.frame_size().0.value().saturating_add(r.strip().value());
    band_of(view_w, Px::of(beside), padding, scale)
}

/// [`margin_band`]'s arithmetic, for a frame and strip `beside` wide.
fn band_of(view_w: Px, beside: Px, padding: f64, scale: CGFloat) -> Px {
    let left = view_w.value().saturating_sub(beside.value()) / 2;
    // The padding in points, at most a few dozen, times a scale of 1 to 3:
    // `as i64` is the widening the audit permits.
    let pad = u32::try_from((padding * scale).max(0.0).floor() as i64).unwrap_or(0);
    Px::of(pad.min(left))
}

/// Whether the pointer is over the terminal view or has left it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InView {
    Yes,
    Left,
}

/// Whether view point `p` is in the view's `bounds`.
fn in_view_at(bounds: NSRect, p: NSPoint) -> InView {
    let (o, s) = (bounds.origin, bounds.size);
    match (o.x..o.x + s.width).contains(&p.x) && (o.y..o.y + s.height).contains(&p.y) {
        true => InView::Yes,
        false => InView::Left,
    }
}

/// **The title strip's lights in the terminal view's device pixels**: each
/// shown light's frame in the flipped view's points, at `scale`, above the
/// view's top in a window and over it in full screen; the tip opens down
/// from the main display, whose first cell is at `origin`, a cell `cell`
/// and the frame's right edge at `right`. None with no light shown.
fn strip_lights(
    in_view: baud_render::ByLed<Option<NSRect>>,
    scale: f64,
    origin: (f64, f64),
    cell: (f64, f64),
    right: f64,
) -> Option<crate::line_tip::LightsAt> {
    if in_view.iter().all(|(_, r)| r.is_none()) {
        return None;
    }
    Some(crate::line_tip::LightsAt {
        edge: baud_render::LineEdge::Top,
        places: baud_render::ByLed::of(|led| {
            (*in_view.get(led)).map(|r| crate::line_tip::Spot {
                x: r.origin.x * scale,
                y: r.origin.y * scale,
                w: r.size.width * scale,
                h: r.size.height * scale,
            })
        }),
        origin,
        cell,
        right,
    })
}

/// **Where the grid begins in the view**, in device pixels: the whole
/// frame `whole` -- the margins, the exit-status strip, the grid and the
/// gutter, and the activity lights' line -- centred in the view `view`,
/// and the grid `left` in from its left (`Renderer::grid_left`) and `top`
/// down from its top (`Renderer::grid_top`).
fn grid_origin(view: (Px, Px), whole: (Px, Px), left: Px, top: Px) -> (Px, Px) {
    (
        Px::of((view.0.value().saturating_sub(whole.0.value()) / 2).saturating_add(left.value())),
        Px::of((view.1.value().saturating_sub(whole.1.value()) / 2).saturating_add(top.value())),
    )
}

/// What turns a view point into a cell and a pixel: the backing scale, the
/// cell in device pixels, the grid in cells, and the padding in device
/// pixels -- `pointer_under`'s arguments after the point, in its order.
type PointerGeometry = (CGFloat, (f64, f64), (u16, u16), (u32, u32));

/// `cell_under` as a pure function of the point, the scale, the cell size
/// in pixels and the grid, so a test can pin the arithmetic.
fn cell_under(
    at: (f64, f64),
    scale: CGFloat,
    cell: (f64, f64),
    grid: (u16, u16),
    pad: (u32, u32),
) -> (Row, Col) {
    let (row, col, _) = pointer_under(at, scale, cell, grid, pad);
    (row, col)
}

/// `line_under` as a pure function of the order the frame drew, the point
/// and the geometry: the cell [`cell_under`] names, by the stored line a
/// selection takes it from (`baud_render::Orders::pointer_line`).
fn line_under(
    orders: &baud_render::Orders,
    at: (f64, f64),
    (scale, cell, grid, pad): PointerGeometry,
) -> (baud_render::WindowLine, Col) {
    let (row, col) = cell_under(at, scale, cell, grid, pad);
    orders.pointer_line(row, col)
}

/// Whether AppKit has the window minimized.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Miniaturized {
    Yes,
    No,
}

/// What `poll_window` reads from AppKit, in points with AppKit's origin at
/// the primary screen's bottom-left and y up: the window's frame, the
/// drawable view's rectangle on the screen, the grid's letterbox inside
/// that view in device pixels, the window's screen and the primary one.
struct Placed {
    minimized: Miniaturized,
    frame: NSRect,
    view: NSRect,
    pad: (u32, u32),
    screen: Option<NSRect>,
    primary: Option<NSRect>,
    scale: CGFloat,
}

/// The facts XTWINOPS reports, from what AppKit said: positions measured
/// from the primary screen's top-left with y down, as X11 measures them,
/// and every position and size in device pixels -- points times the
/// window's backing scale, the unit the cell size and `?1016` use. The text
/// area's position is the grid's first pixel, the origin a pixel mouse
/// report counts from. With no primary screen there is no top to measure
/// down from, so neither position is known.
fn window_facts(p: &Placed) -> baud_core::WindowFacts {
    use baud_core::{PixelSize, ScreenCoord, ScreenPoint, WindowFacts, WindowState};
    let px = |pt: CGFloat| (pt * p.scale).round();
    // Through `i64`, where a float saturates, and then the narrow type,
    // which saturates again: a size past a `u16` is its widest.
    let wide = |v: f64| ImagePx::of(u16::try_from((v as i64).max(0)).unwrap_or(u16::MAX));
    let size = |r: NSRect| PixelSize {
        width: wide(px(r.size.width)),
        height: wide(px(r.size.height)),
    };
    let coord = |v: f64| {
        let v = v as i64;
        ScreenCoord::of(i32::try_from(v).unwrap_or(if v < 0 { i32::MIN } else { i32::MAX }))
    };
    let top_left = |r: NSRect, pad: (u32, u32)| {
        p.primary.map(|primary| {
            let top = primary.origin.y + primary.size.height;
            ScreenPoint {
                x: coord(px(r.origin.x) + f64::from(pad.0)),
                y: coord(px(top - (r.origin.y + r.size.height)) + f64::from(pad.1)),
            }
        })
    };
    WindowFacts {
        state: Some(match p.minimized {
            Miniaturized::Yes => WindowState::Iconified,
            Miniaturized::No => WindowState::Open,
        }),
        position: top_left(p.frame, (0, 0)),
        text_area: top_left(p.view, p.pad),
        window: Some(size(p.frame)),
        screen: p.screen.map(size),
    }
}

/// **The AppKit cursor a pointer is shown as**: the pointing hand is
/// `NSCursor.pointingHand`, "Returns the pointing-hand system cursor"
/// (`[[source: apple-nscursor-pointinghand]]`), the pointer the HIG gives
/// to "a URL link to a webpage, document, or other item"
/// (`[[source: apple-hig-pointing-devices-2026-10-29]]`).
fn system_cursor(p: crate::pointer::SystemPointer) -> Retained<NSCursor> {
    use crate::pointer::SystemPointer;
    match p {
        SystemPointer::Arrow => NSCursor::arrowCursor(),
        SystemPointer::IBeam => NSCursor::IBeamCursor(),
        SystemPointer::PointingHand => NSCursor::pointingHandCursor(),
        SystemPointer::OpenHand => NSCursor::openHandCursor(),
        SystemPointer::ClosedHand => NSCursor::closedHandCursor(),
        SystemPointer::Crosshair => NSCursor::crosshairCursor(),
        SystemPointer::NotAllowed => NSCursor::operationNotAllowedCursor(),
        SystemPointer::ContextualMenu => NSCursor::contextualMenuCursor(),
    }
}

/// **What a key, a move, a press or a read did to the pointer**, done
/// with AppKit: "Pass true to hide the cursor until one of the following
/// occurs: The mouse moves. You invoke the method again, with flag set to
/// false" (`[[source: apple-nscursor-sethiddenuntilmousemoves]]`). So a
/// move shows it without baud, and a press, a crossing or a reset shows it
/// by the second.
fn hide_pointer(act: crate::hide::Act) {
    match act {
        crate::hide::Act::Hide => NSCursor::setHiddenUntilMouseMoves(true),
        crate::hide::Act::Show => NSCursor::setHiddenUntilMouseMoves(false),
        crate::hide::Act::Nothing => {}
    }
}

/// The view's event as the shared routing names it (`gesture.rs`).
fn mouse_kind(k: ViewMouseKind) -> Kind {
    match k {
        ViewMouseKind::Down => Kind::Down,
        ViewMouseKind::Up => Kind::Up,
        ViewMouseKind::Moved => Kind::Moved,
        ViewMouseKind::Dragged => Kind::Dragged,
        ViewMouseKind::Entered | ViewMouseKind::Exited => Kind::Crossed,
    }
}

/// Which half of an AppKit pointer event DEC's locator is told: a move, a
/// drag or a crossing before the scrollbar and the links see it, so a
/// filter rectangle sees every position; a press or a release once the
/// route has said whose it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LocatorHalf {
    Unrouted,
    Routed(To),
}

/// Whether a locator report went into the core's reply buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reply {
    Queued,
    None,
}

/// Whether the indicator line owes a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LineFrame {
    Owed,
    None,
}

/// What [`tell_locator`] leaves the driver to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct LocatorOwed {
    reply: Reply,
    frame: LineFrame,
}

/// **DEC's locator told one AppKit pointer event, in the half `half` is**
/// (`Term::locator`): a move, a drag or a crossing in the unrouted half, a
/// press or a release in the routed one, and nothing told in the other.
/// `at` is the cell under the event, `None` off the text area, and a leave
/// is `None` wherever it was, the pointer having left the view; `held` the
/// buttons after it (`held_after`). A press the route gave the selection
/// is a move to the locator (`locator_input`). The indicator line owes a
/// frame whenever it is shown and the locator was told (`locator_redraws`).
fn tell_locator(
    term: &mut Term<'_>,
    (view_kind, view_button): (ViewMouseKind, ViewButton),
    half: LocatorHalf,
    at: Option<LocatorAt>,
    held: LocatorButtons,
) -> LocatorOwed {
    let (kind, button) = (mouse_kind(view_kind), mouse_button(view_button));
    let at = match view_kind {
        ViewMouseKind::Exited => None,
        _ => at,
    };
    let e = match (half, kind) {
        (LocatorHalf::Unrouted, Kind::Down | Kind::Up) => None,
        (LocatorHalf::Unrouted, _) => Some(LocatorInput {
            kind: LocatorKind::Moved,
            at,
            held,
        }),
        (LocatorHalf::Routed(to), Kind::Down | Kind::Up) => Some(locator_input(
            (kind, button, to),
            term.mouse_tracking(),
            at,
            held,
        )),
        (LocatorHalf::Routed(_), _) => None,
    };
    let Some(e) = e else {
        return LocatorOwed {
            reply: Reply::None,
            frame: LineFrame::None,
        };
    };
    // A ReGIS cursor the event moved under the pointer owes a frame too.
    let drawn = term.images().generation();
    let reply = match term.locator(e) {
        true => Reply::Queued,
        false => Reply::None,
    };
    let moved = term.images().generation() != drawn;
    LocatorOwed {
        reply,
        frame: match crate::gesture::locator_redraws(term) || moved {
            true => LineFrame::Owed,
            false => LineFrame::None,
        },
    }
}

/// The view's button as the shared routing names it.
fn mouse_button(b: ViewButton) -> Pressed {
    match b {
        ViewButton::Left => Pressed::Left,
        ViewButton::Middle => Pressed::Middle,
        ViewButton::Right => Pressed::Right,
        // `NSEvent.buttonNumber` 3 to 6, X's buttons 8 to 11, as XQuartz
        // numbers "additional mouse buttons (4 and higher in AppKit)"
        // (`[[source: xquartz-x11application]]`); the first, DEC's
        // locator's fourth.
        ViewButton::Other(3) => Pressed::Button8,
        ViewButton::Other(4) => Pressed::Button9,
        ViewButton::Other(5) => Pressed::Button10,
        ViewButton::Other(6) => Pressed::Button11,
        ViewButton::Other(_) => Pressed::Other,
    }
}

/// What a left press does to the selection, from the modifiers held:
/// Option starts a block and wins over Shift, because under mouse tracking
/// Shift is the only way into a selection at all -- were Shift to win, a
/// block could not be started while a selection was on screen.
fn press_of(m: macos_liquid_glass::drawable::Modifiers) -> Press {
    if m.option {
        Press::Block
    } else if m.shift {
        Press::Extend
    } else {
        Press::Stream
    }
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

/// How often Notification Center's list is asked for while any program's
/// notification is shown: a dismissal is out of `p=alive` at most this
/// late.
const DELIVERED_EVERY: Duration = Duration::from_secs(2);

/// A post as the center takes it.
fn mac_post(p: crate::notify::MacPost) -> baud_present_macos::notify::Post {
    use baud_present_macos::notify::{Level, Post, PostSound};
    // The image to a file of its own the center moves into its store.
    let image = p.image.and_then(|(bytes, ext)| {
        let dir = std::env::temp_dir().join(format!("baud-notify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("{}.{ext}", p.id));
        std::fs::write(&path, bytes).ok()?;
        Some(path)
    });
    Post {
        id: p.id,
        title: p.title,
        subtitle: p.subtitle,
        body: p.body,
        sound: match p.silent {
            crate::notify::Silence::Sound => PostSound::Default,
            crate::notify::Silence::Silent => PostSound::Silent,
        },
        level: match p.passive {
            crate::notify::Passive::Passive => Level::Passive,
            crate::notify::Passive::Active => Level::Active,
        },
        buttons: p.buttons,
        image,
    }
}

/// The Dock tile's bar as the tile draws it.
fn dock_bar(b: crate::notify::DockBar) -> baud_present_macos::notify::Bar {
    use baud_present_macos::notify::{Bar, Tone};
    Bar {
        fraction: b.fraction,
        tone: match b.tone {
            crate::notify::DockTone::Normal => Tone::Normal,
            crate::notify::DockTone::Error => Tone::Error,
            crate::notify::DockTone::Paused => Tone::Paused,
        },
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// **Command-C and Command-V the menu missed are copy and paste**, by
    /// what the key types with Command held: the Hebrew, Russian and Greek
    /// layouts' Command-C and Command-V type `c` and `v`, as
    /// `UCKeyTranslate` reads them, so the copy and the paste are found;
    /// their own characters are not Latin and find nothing; Shift, Option
    /// or Control held finds nothing. The key handler asks it for every
    /// Command key, after the prompt jumps.
    #[test]
    fn a_command_key_the_menu_missed_still_copies_and_pastes() {
        use macos_liquid_glass::drawable::Modifiers as Held;
        let cmd = Held {
            command: true,
            ..Held::default()
        };
        assert_eq!(menu_missed("c", cmd), Some("copy:"));
        assert_eq!(menu_missed("v", cmd), Some("paste:"));
        for own in ["\u{5d1}", "\u{5d4}", "\u{441}", "\u{3c8}", "x"] {
            assert_eq!(menu_missed(own, cmd), None, "{own}");
        }
        for held in [
            Held { shift: true, ..cmd },
            Held {
                option: true,
                ..cmd
            },
            Held {
                control: true,
                ..cmd
            },
        ] {
            assert_eq!(menu_missed("c", held), None, "{held:?}");
        }
        let src = include_str!("app.rs");
        let block = src
            .split(concat!("if key.modifiers.", "command {"))
            .nth(1)
            .expect("the Command block");
        let block = &block[..block.find("return Handled::Consumed;").expect("its end")];
        assert!(
            block.contains(concat!("menu_missed(&key.", "characters, m)")),
            "the key handler asks"
        );
    }

    /// A switch only under `auto` and only when the resolved appearance
    /// changes: a callback repeating the appearance the palette is set for --
    /// Increase Contrast toggling, say -- must not drop an application's
    /// colours.
    #[test]
    fn the_palette_switches_only_under_auto_and_only_on_a_change() {
        use crate::config::ThemeChoice::{Auto, Dark, Light};
        use Appearance as A;
        for (choice, set_for, now, want) in [
            (Auto, None, A::Dark, PaletteSwitch::To(A::Dark)),
            (Auto, Some(A::Dark), A::Light, PaletteSwitch::To(A::Light)),
            (Auto, Some(A::Light), A::Dark, PaletteSwitch::To(A::Dark)),
            (Auto, Some(A::Dark), A::Dark, PaletteSwitch::Stay),
            (Auto, Some(A::Light), A::Light, PaletteSwitch::Stay),
            (Dark, None, A::Light, PaletteSwitch::Stay),
            (Light, Some(A::Dark), A::Light, PaletteSwitch::Stay),
        ] {
            assert_eq!(
                palette_switch(choice, set_for, now),
                want,
                "{choice:?} {set_for:?} {now:?}"
            );
        }
    }

    /// **The standard material draws in the system's appearance, the glass
    /// in Icon & widget style's**, so the shadow follows the body drawn: on
    /// a Dark system under a ClearLight widget style the standard material
    /// is dark and `shadow = "auto"` casts one.
    #[test]
    fn each_body_draws_in_its_own_appearance() {
        for (system, widget) in [
            (Appearance::Dark, Appearance::Light),
            (Appearance::Light, Appearance::Dark),
            (Appearance::Dark, Appearance::Dark),
        ] {
            assert_eq!(drawn_in(BodyKind::Standard, system, widget), system);
            assert_eq!(drawn_in(BodyKind::Glass, system, widget), widget);
        }
    }

    /// **The floor reads text against the body actually drawn**: the
    /// standard material's own greys for each half while the palette
    /// follows the system, and black to white over either glass and under
    /// a pinned palette, which can sit on the other half's material.
    #[test]
    fn the_floor_reads_against_the_body_drawn() {
        use crate::config::{Glass, Material, ThemeChoice};
        use baud_render::{Backdrop, Floor};
        let halves = crate::palette::Halves::default();
        let of = |style, choice| {
            let glass = Glass {
                style,
                ..Glass::defaults()
            };
            let t = Themes::of(&glass, choice, Floor::DEFAULT, &halves);
            (t.dark.backdrop, t.light.backdrop)
        };
        assert_eq!(
            of(Material::Standard, ThemeChoice::Auto),
            (
                Backdrop::under_window_dark(),
                Backdrop::under_window_light()
            )
        );
        assert_eq!(
            of(Glass::defaults().style, ThemeChoice::Auto),
            (
                Backdrop::under_window_dark(),
                Backdrop::under_window_light()
            ),
            "the default body is the standard material"
        );
        for (style, choice) in [
            (Material::Standard, ThemeChoice::Dark),
            (Material::Standard, ThemeChoice::Light),
            (Material::Regular, ThemeChoice::Auto),
            (Material::Clear, ThemeChoice::Auto),
            (Material::Regular, ThemeChoice::Dark),
        ] {
            assert_eq!(
                of(style, choice),
                (Backdrop::any(), Backdrop::any()),
                "{style:?} {choice:?}"
            );
        }
        // Reduce Transparency leaves the backdrop alone: at alpha 1.0 the
        // floor's second pass reads nothing behind the page.
        let theme = theme_for(
            Appearance::Dark,
            &Glass::defaults(),
            ThemeChoice::Auto,
            Floor::DEFAULT,
            &halves,
        );
        let on = accessible(
            theme.clone(),
            Display {
                transparency: Transparency::Reduced,
                ..Display::default()
            },
        );
        assert_eq!((on.backdrop, on.bg_alpha.get()), (theme.backdrop, 0xFF));
    }

    /// **Reduce Transparency draws every background opaque**, the default
    /// one and every one a program sets, so the margin, which clears to the
    /// default background at the theme's alpha, is opaque too (note Y,
    /// decision 9). Nothing else in the theme moves: the colours the core
    /// reports are the same, so the poll owes it no new defaults.
    #[test]
    fn reduce_transparency_draws_every_background_opaque() {
        let glass = crate::config::Glass::defaults();
        for (theme, dim) in [
            (
                theme_for(
                    Appearance::Dark,
                    &glass,
                    crate::config::ThemeChoice::Auto,
                    baud_render::Floor::DEFAULT,
                    &Default::default(),
                ),
                glass.dim_dark,
            ),
            (
                theme_for(
                    Appearance::Light,
                    &glass,
                    crate::config::ThemeChoice::Auto,
                    baud_render::Floor::DEFAULT,
                    &Default::default(),
                ),
                glass.dim_light,
            ),
        ] {
            use baud_domain::Chan;
            let fields = |t: &Theme| (t.fg, t.bg, t.palette, t.floor, t.app_bg, t.picture_bg);
            assert_eq!(theme.bg_alpha, Chan::from_unit(dim), "the control");
            let off = accessible(theme.clone(), Display::default());
            assert_eq!(
                (off.bg_alpha, fields(&off)),
                (theme.bg_alpha, fields(&theme)),
                "off changes nothing"
            );
            let on = accessible(
                theme.clone(),
                Display {
                    transparency: Transparency::Reduced,
                    ..Display::default()
                },
            );
            assert_eq!(on.bg_alpha, Chan::from_u8(0xFF));
            assert!(!on.finds_pictures(), "every picture is already solid");
            assert_eq!(fields(&on), fields(&theme), "only the alpha moves");
        }
        assert_eq!(
            display_settings(|| true, || false).transparency,
            Transparency::Reduced
        );
        assert_eq!(display_settings(|| false, || false), Display::default());
    }

    /// **Increase Contrast makes the floor stricter**: a trigger of at
    /// least 3.0 and a target of at least 4.5 (note Y, decision 9), over a
    /// floor that is off too, by analogy with Reduce Transparency over the
    /// dim (note Y). A reader's stricter floor is kept, never lowered:
    /// `floor = 7` is a target of 7 already.
    #[test]
    fn increase_contrast_makes_the_floor_stricter() {
        use baud_render::Floor;
        let lift = |trigger, target| Floor::Lift { trigger, target };
        let on = Display {
            contrast: Contrast::Increased,
            ..Display::default()
        };
        let glass = crate::config::Glass::defaults();
        for (configured, want) in [
            (Floor::DEFAULT, lift(3.0, 4.5)),
            (Floor::Off, lift(3.0, 4.5)),
            (lift(3.0, 4.5), lift(3.0, 4.5)),
            (lift(2.0, 4.5), lift(3.0, 4.5)),
            (lift(7.0, 7.0), lift(7.0, 7.0)),
            (lift(1.0, 4.5), lift(3.0, 4.5)),
        ] {
            let theme = theme_for(
                Appearance::Dark,
                &glass,
                crate::config::ThemeChoice::Auto,
                configured,
                &Default::default(),
            );
            assert_eq!(theme.floor, configured, "the control");
            assert_eq!(accessible(theme.clone(), on).floor, want, "{configured:?}");
            let off = accessible(theme.clone(), Display::default());
            assert_eq!(off.floor, configured, "off keeps {configured:?}");
            let alpha = accessible(theme.clone(), on).bg_alpha;
            assert_eq!(alpha, theme.bg_alpha, "the dim is Reduce Transparency's");
        }
        let both = Display {
            transparency: Transparency::Reduced,
            contrast: Contrast::Increased,
        };
        let theme = theme_for(
            Appearance::Light,
            &glass,
            crate::config::ThemeChoice::Auto,
            Floor::DEFAULT,
            &Default::default(),
        );
        let drawn = accessible(theme, both);
        assert_eq!(
            (drawn.floor, drawn.bg_alpha.get()),
            (lift(3.0, 4.5), 0xFF),
            "both at once"
        );
        assert_eq!(display_settings(|| false, || true), on);
    }

    /// **A pinned theme is reached**: the poll rebuilds the theme in force,
    /// which for a pinned one is the pinned half whatever the appearance
    /// last set, and under `auto` the half the palette was set for.
    #[test]
    fn the_theme_in_force_is_the_pinned_half_or_the_one_last_set() {
        use crate::config::ThemeChoice::{Auto, Dark, Light};
        use Appearance as A;
        for (choice, set_for, want) in [
            (Light, None, A::Light),
            (Light, Some(A::Dark), A::Light),
            (Dark, None, A::Dark),
            (Auto, None, A::Dark),
            (Auto, Some(A::Light), A::Light),
            (Auto, Some(A::Dark), A::Dark),
        ] {
            assert_eq!(in_force(choice, set_for), want, "{choice:?} {set_for:?}");
        }
    }

    /// **One way to the renderer.** Every theme reaches it through
    /// `show_theme`, which applies the display settings, so a light/dark
    /// flip or the first frame cannot set a theme they did not see; and the
    /// poll rebuilds the theme in force itself rather than going through
    /// `apply_style`, whose early return would skip a pinned theme.
    #[test]
    fn every_theme_reaches_the_renderer_through_the_display_settings() {
        let app = include_str!("app.rs")
            .rsplit_once("#[cfg(test)]\nmod tests {")
            .map(|(c, _)| c)
            .expect("the code before the tests");
        let body = |name: &str| {
            let at = app.find(&format!("fn {name}(")).expect(name);
            &app[at..at + app[at..].find("\n    }\n").expect("its end")]
        };
        assert_eq!(app.matches(".set_theme(").count(), 1, "one call");
        let show = body("show_theme");
        assert!(show.contains(".set_theme("), "the call is show_theme's");
        assert!(show.contains("accessible("), "{show}");
        let poll = body("poll_display");
        assert!(
            poll.contains("display_settings(reduce_transparency, increase_contrast)"),
            "{poll}"
        );
        assert!(poll.contains("show_theme(in_force("), "{poll}");
        assert!(!poll.contains("palette_switch("), "{poll}");
        assert!(!poll.contains("apply_style("), "{poll}");
        assert!(body("apply_style").contains("show_theme("), "the flip");
    }

    /// **One place a driver drops.** `Overrides::Drop` appears once in the
    /// macOS driver's code outside its tests, in `apply_style`, behind
    /// `palette_switch`, and the flip takes its theme from the prebuilt
    /// pair, building none; once in the Linux driver's, for a change of the
    /// desktop's, in `appearance::overrides`, behind `appearance::switch`.
    /// A config read again keeps them, on both (`reload_palette`).
    #[test]
    fn only_the_appearance_switch_drops_program_colours() {
        // Everything before the file's own test module, which sits last --
        // not before the first `#[cfg(test)]`, which in `app.rs` is a helper
        // module ahead of most of the code, and cut there the gate read none.
        let code = |src: &'static str| {
            src.rsplit_once("#[cfg(test)]\nmod tests {")
                .map_or(src, |(c, _)| c)
        };
        let app = code(include_str!("app.rs"));
        let host = code(include_str!("host.rs"));
        let linux = code(include_str!("linux.rs"));
        let appearance = code(include_str!("appearance.rs"));
        let drops = |s: &str| s.matches("Overrides::Drop").count();
        assert!(app.len() > 100_000, "the gate reads the app's code");
        assert_eq!(
            (drops(app), drops(host), drops(linux), drops(appearance)),
            (1, 0, 0, 1)
        );
        // The Linux driver's, a desktop change's, behind the switch rule's
        // Linux form, its only switch of half.
        let at = appearance.find("fn overrides(").expect("overrides");
        let body = &appearance[at..at + appearance[at..].find("\n}\n").expect("its end")];
        assert!(body.contains("Origin::Changed => baud_core::Overrides::Drop"));
        // The other is a config read again, which keeps a program's
        // colours as the macOS driver's `reload_palette` does.
        assert_eq!(linux.matches("self.show_half(").count(), 2);
        assert_eq!(
            linux
                .matches("self.show_half(half, baud_core::Overrides::Keep)")
                .count(),
            1
        );
        assert!(linux.contains("crate::appearance::switch("));
        let at = app.find("fn apply_style(").expect("apply_style");
        let body = &app[at..at + app[at..].find("\n    }\n").expect("its end")];
        assert!(
            body.contains("Overrides::Drop"),
            "the drop is in apply_style"
        );
        assert!(body.contains("palette_switch("), "behind the switch rule");
        assert!(!body.contains("theme_for("), "the flip builds no theme");
    }

    /// **A flip of light and dark swaps the title bar's lockup with no
    /// plate**, from the driver's appearance path: `apply_style`, which a
    /// change of the view's effective appearance reaches through `pump`, has
    /// the lockup follow the window before the early return a pinned palette
    /// takes, so `theme = "dark"` under a light system with the lights off
    /// still shows the dark word on the light strip. The form is the
    /// window's, through `Mark::follow_window`, the reading `Mark::put` makes
    /// at the launch; on a plate it is the plate's
    /// (`baud-app/tests/titlebar_mark.rs` holds both in real windows).
    /// Perturbation: move the call below `palette_switch(`, or drop it,
    /// and this goes red.
    #[test]
    fn a_flip_of_light_and_dark_swaps_the_title_bars_lockup() {
        let app = include_str!("app.rs");
        let app = app
            .rsplit_once("#[cfg(test)]\nmod tests {")
            .map_or(app, |(c, _)| c);
        let at = app.find("fn apply_style(").expect("apply_style");
        let body = &app[at..at + app[at..].find("\n    }\n").expect("its end")];
        let follows = body
            .find("mark.follow_window(self.window.ns_window())")
            .expect("the lockup follows the window in apply_style");
        let early = body.find("palette_switch(").expect("the switch rule");
        assert!(follows < early, "before the early return");
        assert_eq!(app.matches(".follow_window(").count(), 1, "one caller");
    }

    /// Every keypad key code of `[[source: macos-sdk-events-h]]` (lines
    /// 244-261) and the keypad key it names. The codes skip `0x5A`, F20's,
    /// between `kVK_ANSI_Keypad7` and `kVK_ANSI_Keypad8`.
    const MAC_KEYPAD: [(u16, baud_domain::KeypadKey, &str); 17] = {
        use baud_domain::KeypadKey::*;
        [
            (0x41, Period, "."),
            (0x43, Multiply, "*"),
            (0x45, Add, "+"),
            (0x4B, Divide, "/"),
            (0x4C, Enter, "\u{3}"),
            (0x4E, Minus, "-"),
            (0x51, Equal, "="),
            (0x52, Digit0, "0"),
            (0x53, Digit1, "1"),
            (0x54, Digit2, "2"),
            (0x55, Digit3, "3"),
            (0x56, Digit4, "4"),
            (0x57, Digit5, "5"),
            (0x58, Digit6, "6"),
            (0x59, Digit7, "7"),
            (0x5B, Digit8, "8"),
            (0x5C, Digit9, "9"),
        ]
    };

    /// A Mac key press with `characters` as the US layout translates the
    /// key, which `UCKeyTranslate` gives for every keypad code. A macro, so
    /// the key code keeps `KeyPress`'s own type and no signature here
    /// carries it.
    macro_rules! mac_key {
        ($code:expr, $chars:expr, $mods:expr) => {
            KeyPress {
                key_code: $code,
                characters: $chars.to_string(),
                unmodified: $chars.to_string(),
                modifiers: $mods,
                repeat: false,
            }
        };
    }

    /// **Each keypad key of a Mac names its keypad key**, the main Return
    /// stays Return, and Clear, the key where a PC keypad has NumLock, is
    /// NumLock: the HID usage tables name one key "Keypad Num Lock and
    /// Clear" (`[[source: usb-hid-usage-tables-1-6]]`, usage 0x53), and
    /// kitty's macOS layer names `0x47` `GLFW_FKEY_NUM_LOCK` (`[[source:
    /// kitty-glfw-cocoa-window-851cb80]]`).
    #[test]
    fn every_mac_keypad_code_names_its_keypad_key() {
        let none = macos_liquid_glass::drawable::Modifiers::default();
        for (code, k, chars) in MAC_KEYPAD {
            assert_eq!(
                special(&mac_key!(code, chars, none)),
                Some(KeyCode::Keypad(k)),
                "{code:#x}"
            );
        }
        assert_eq!(special(&mac_key!(0x24, "\r", none)), Some(KeyCode::Enter));
        assert_eq!(
            special(&mac_key!(0x47, "\u{f739}", none)),
            Some(KeyCode::Pua(baud_domain::PuaKey::NumLock))
        );
    }

    /// **Control on a non-Latin Apple layout sends what the layout's
    /// Control layer types**, and the PC-101 key rides as the base layout
    /// key: Ctrl+С on Russian, whose Control layer types `0x03` (a
    /// `UCKeyTranslate` probe), names `c` for Control and is `^C` through
    /// the core, as Ctrl+ב on Hebrew and Ctrl+ψ on Greek; under kitty's
    /// disambiguate it is the key itself with its base layout key. Option
    /// alone is not Control. On US nothing is added, Shift held or not.
    #[test]
    fn control_on_a_non_latin_mac_layout_is_its_control_layers_key() {
        use macos_liquid_glass::drawable::Modifiers as Held;
        let ctrl = Held {
            control: true,
            ..Held::default()
        };
        let option = Held {
            option: true,
            ..Held::default()
        };
        let key = |code: u16, chars: &str, plain: &str, held: Held| KeyPress {
            key_code: code,
            characters: chars.to_string(),
            unmodified: plain.to_string(),
            modifiers: held,
            repeat: false,
        };
        let cp = |c: char| Some(Codepoint::from_char(c));
        let ws = WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let sent = |modes: &[u8], e: KeyEvent| {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            t.write_at(baud_domain::BytePos::ZERO, modes);
            t.feed(
                baud_domain::BytePos::ZERO,
                baud_domain::ReadLen::from_index(modes.len()),
            );
            t.clear_reply();
            let mut out = Vec::new();
            crate::reads::input(&mut t, |t| t.key(e), |b| out.extend_from_slice(b));
            out
        };
        for (code, chars, plain, want) in [
            (0x08, "\u{3}", "\u{441}", 'c'),
            (0x08, "\u{3}", "\u{5d1}", 'c'),
            (0x08, "\u{3}", "\u{3c8}", 'c'),
        ] {
            let e = chord(&key(code, chars, plain, ctrl), mods(ctrl), None).expect(plain);
            assert_eq!(
                e.code,
                KeyCode::Char(Codepoint::from_char(plain.chars().next().unwrap()))
            );
            assert_eq!((e.control, e.base_layout), (cp(want), cp(want)), "{plain}");
            assert_eq!(
                sent(b"", e),
                [u8::try_from(u32::from(want) & 0x1f).unwrap()],
                "{plain}"
            );
        }
        let es = chord(&key(0x08, "\u{3}", "\u{441}", ctrl), mods(ctrl), None).unwrap();
        assert_eq!(sent(b"\x1b[=5u", es), b"\x1b[1089::99;5u");
        let alt = chord(
            &key(0x08, "\u{2260}", "\u{441}", option),
            mods(option),
            None,
        )
        .unwrap();
        assert_eq!((alt.control, alt.base_layout), (None, cp('c')));
        let us = chord(&key(0x08, "\u{3}", "c", ctrl), mods(ctrl), None).unwrap();
        assert_eq!((us.control, us.base_layout), (None, None));
        let shift = Held {
            shift: true,
            ..ctrl
        };
        let us_shift = chord(&key(0x08, "\u{3}", "C", shift), mods(shift), None).unwrap();
        assert_eq!(
            us_shift.base_layout, None,
            "Shift does not make the key another"
        );
        assert_eq!(sent(b"", us), [3]);
    }

    /// The Control layer's character names the key that folds to it.
    #[test]
    fn a_control_layers_character_names_its_key() {
        for (typed, key) in [
            ("\u{0}", Some('@')),
            ("\u{3}", Some('c')),
            ("\u{1a}", Some('z')),
            ("\u{1b}", Some('[')),
            ("\u{1f}", Some('_')),
            ("\u{7f}", Some('?')),
            (";", Some(';')),
            ("\u{5f3}", None),
            ("", None),
        ] {
            assert_eq!(
                control_of(typed),
                key.map(Codepoint::from_char),
                "{typed:?}"
            );
        }
    }

    /// **The Mac's PC-101 table names the 48 printable keys once each**,
    /// the characters a US PC-101 board types unshifted.
    #[test]
    fn the_mac_pc101_table_is_the_101_keys_characters() {
        let none = macos_liquid_glass::drawable::Modifiers::default();
        let mut got: Vec<char> = (0..=0x7f)
            .filter_map(|code| pc101(&mac_key!(code, "", none)))
            .map(|c| char::from_u32(c.value()).unwrap())
            .collect();
        got.sort_unstable();
        let mut want: Vec<char> = "`1234567890-=qwertyuiop[]\\asdfghjkl;'zxcvbnm,./ "
            .chars()
            .collect();
        want.sort_unstable();
        assert_eq!(got, want);
    }

    /// **A key the input manager types as itself carries its base layout
    /// key; a commit or a paste does not**, and the chord path and the
    /// input manager's path build their events through these.
    #[test]
    fn the_input_managers_text_carries_the_keys_base_layout_key() {
        let c = Codepoint::from_char('c');
        let es = typed_text("\u{441}", Some(&keyed("\u{441}", c, Modifiers::NONE, None)));
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].base_layout, Some(c));
        let commit = typed_text(
            "\u{6f22}\u{5b57}",
            Some(&keyed("\u{20}", c, Modifiers::NONE, None)),
        );
        assert!(commit.iter().all(|e| e.base_layout.is_none()));
        let same = typed_text("c", Some(&keyed("c", c, Modifiers::NONE, None)));
        assert_eq!(same[0].base_layout, None, "the key's own");
        let src = include_str!("app.rs");
        assert!(src.contains(concat!(
            "let Some(e) = ",
            "chord(key, m, shift_unshifted(key)) else"
        )));
        assert!(src.contains(concat!("for e in ", "typed_text(text, keyed.as_ref())")));
    }

    /// The keystroke the input manager was handed, as `key` records it,
    /// on a layout whose key types `unshifted` without Shift.
    fn keyed(typed: &str, base: Codepoint, mods: Modifiers, unshifted: Option<char>) -> Keyed {
        Keyed {
            typed: typed.to_string(),
            base: Some(base),
            mods,
            unshifted: unshifted.map(Codepoint::from_char),
        }
    }

    /// What the core sends for `e` once `modes` is fed.
    fn core_sends(modes: &[u8], e: KeyEvent) -> Vec<u8> {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        t.write_at(baud_domain::BytePos::ZERO, modes);
        t.feed(
            baud_domain::BytePos::ZERO,
            baud_domain::ReadLen::from_index(modes.len()),
        );
        t.clear_reply();
        let mut out = Vec::new();
        crate::reads::input(&mut t, |t| t.key(e), |b| out.extend_from_slice(b));
        out
    }

    /// **Shift on a key the input manager types is reported beside the key
    /// it is on**, as kitty reports it (`[[source: kitty-keyboard]]`: the
    /// key is "the lower-case (or more technically, un-shifted) version",
    /// modifiers `1 + 1` for Shift, "the shifted key is simply the
    /// upper-case version"): Shift+a is `CSI 97 ; 2 u` under "report all
    /// keys", where it went out as `CSI 97 u`, and Shift+1 the `1` key with
    /// `!` shifted, `CSI 49 ; 2 u`, by the layout's own unshifted character.
    /// The legacy layer sends the text as before; `modifyOtherKeys` 2
    /// numbers the shifted character as xterm numbers its keysym. With no
    /// layout answer a capital's key is its lower case and `!` is left as
    /// typed; a commit that is not the keystroke's own carries no Shift.
    /// Perturbation: build the events at `Modifiers::NONE` again and every
    /// kitty row reads no Shift; drop the layout's character and Shift+1
    /// reads `CSI 33 u`.
    #[test]
    fn shift_on_a_typed_key_is_reported_beside_the_key_it_is_on() {
        let shift = Modifiers::SHIFT;
        let typed = |text: &str, base: char, unshifted: Option<char>| {
            typed_text(
                text,
                Some(&keyed(text, Codepoint::from_char(base), shift, unshifted)),
            )[0]
        };
        let a = typed("A", 'a', Some('a'));
        let one = typed("!", '1', Some('1'));
        let space = typed(" ", ' ', Some(' '));
        for (e, modes, want) in [
            (a, b"".as_slice(), b"A".as_slice()),
            (a, b"\x1b[>1u", b"A"),
            (a, b"\x1b[>8u", b"\x1b[97;2u"),
            (a, b"\x1b[>11u", b"\x1b[97;2u"),
            (a, b"\x1b[>12u", b"\x1b[97:65;2u"),
            (a, b"\x1b[>24u", b"\x1b[97;2;65u"),
            (a, b"\x1b[>4;2m", b"\x1b[27;2;65~"),
            (one, b"", b"!"),
            (one, b"\x1b[>1u", b"!"),
            (one, b"\x1b[>8u", b"\x1b[49;2u"),
            (one, b"\x1b[>12u", b"\x1b[49:33;2u"),
            (one, b"\x1b[>24u", b"\x1b[49;2;33u"),
            (one, b"\x1b[>4;2m", b"\x1b[27;2;33~"),
            (space, b"", b" "),
            (space, b"\x1b[>8u", b"\x1b[32;2u"),
        ] {
            assert_eq!(
                core_sends(modes, e),
                want,
                "{:?} after {:?}",
                e.text,
                String::from_utf8_lossy(modes)
            );
        }
        // No answer from the layout: the capital's lower case, and `!` as
        // it was typed.
        assert_eq!(
            core_sends(b"\x1b[>8u", typed("A", 'a', None)),
            b"\x1b[97;2u"
        );
        assert_eq!(core_sends(b"\x1b[>8u", typed("!", '1', None)), b"\x1b[33u");
        // Not the keystroke's own text: an input method's commit.
        let commit = typed_text(
            "A",
            Some(&keyed("a", Codepoint::from_char('a'), shift, Some('a'))),
        );
        assert_eq!(commit[0].mods, Modifiers::NONE);
        assert_eq!(core_sends(b"\x1b[>8u", commit[0]), b"\x1b[97u");
        // Shift alone is not a chord: its capital is predicted as typed.
        assert!(!is_chord(shift));
        assert!(is_chord(Modifiers::CTRL.with(shift)));
    }

    /// **A Control or Option chord with Shift names the key Shift is on**,
    /// `charactersIgnoringModifiers`' character being its shifted key, as
    /// kitty's macOS layer takes it: Control+Shift+a is `CSI 97 ; 6 u`
    /// under "report all keys", where it went out as `CSI 65 ; 6 u`, and
    /// still `^A` on the legacy layer; Control+Shift+2 is the `2` key
    /// (`CSI 50 ; 6 u`) and still NUL, the `@` key's, on the legacy layer.
    /// Perturbation: keep the shifted character as the key and the kitty
    /// rows read `65` and `64`.
    #[test]
    fn a_chord_with_shift_names_the_key_shift_is_on() {
        use macos_liquid_glass::drawable::Modifiers as Held;
        let held = Held {
            control: true,
            shift: true,
            ..Held::default()
        };
        let key = |code: u16, chars: &str, plain: &str| KeyPress {
            key_code: code,
            characters: chars.to_string(),
            unmodified: plain.to_string(),
            modifiers: held,
            repeat: false,
        };
        let cp = |c: char| Some(Codepoint::from_char(c));
        let a = chord(&key(0x00, "\u{1}", "A"), mods(held), cp('a')).expect("a");
        let two = chord(&key(0x13, "\u{0}", "@"), mods(held), cp('2')).expect("2");
        for (e, modes, want) in [
            (a, b"".as_slice(), b"\x01".as_slice()),
            (a, b"\x1b[>8u", b"\x1b[97;6u"),
            (a, b"\x1b[>12u", b"\x1b[97:65;6u"),
            (a, b"\x1b[>4;2m", b"\x1b[27;6;65~"),
            (two, b"", b"\x00"),
            (two, b"\x1b[>8u", b"\x1b[50;6u"),
            (two, b"\x1b[>12u", b"\x1b[50:64;6u"),
            (two, b"\x1b[>4;2m", b"\x1b[27;6;64~"),
        ] {
            assert_eq!(
                core_sends(modes, e),
                want,
                "{:?} after {:?}",
                e.code,
                String::from_utf8_lossy(modes)
            );
        }
    }

    /// **Every row of the table both drivers share is what a Mac sends**
    /// (`tests/keys/shift.txt`, which the Wayland driver's
    /// `the_shift_table_is_what_the_wayland_driver_sends` reads too): the
    /// key Shift is on, Shift beside it and what it typed its shifted key,
    /// on the US layout -- a Shift-only key through the input manager's
    /// `text`, a Control chord through `chord`. Perturbation: build the
    /// input manager's events at `Modifiers::NONE` again and every kitty
    /// row reads no Shift.
    #[test]
    fn the_shift_table_is_what_a_mac_sends() {
        use macos_liquid_glass::drawable::Modifiers as Held;
        let character = |s: &str| match s {
            "space" => ' ',
            _ => s.chars().next().expect("a character"),
        };
        let unescape = |s: &str| {
            let mut out = Vec::new();
            let mut i = 0;
            while i < s.len() {
                match &s[i..] {
                    t if t.starts_with("\\e") => {
                        out.push(0x1b);
                        i += 2;
                    }
                    t if t.starts_with("\\x") => {
                        out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).expect("hex"));
                        i += 4;
                    }
                    t => {
                        let c = t.chars().next().expect("a character");
                        out.extend_from_slice(c.to_string().as_bytes());
                        i += c.len_utf8();
                    }
                }
            }
            out
        };
        // The Mac's key code for the US key `c`, its PC-101 table's.
        let code_of = |c: char| {
            (0..0x80)
                .find(|&code| {
                    let k = KeyPress {
                        key_code: code,
                        characters: String::new(),
                        unmodified: String::new(),
                        modifiers: Held::default(),
                        repeat: false,
                    };
                    pc101(&k) == Some(Codepoint::from_char(c))
                })
                .expect("a key")
        };
        let mut rows = 0;
        for line in include_str!("../../tests/keys/shift.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let [keyed_as, mode, want] = line.split(" | ").collect::<Vec<_>>()[..] else {
                panic!("{line}")
            };
            let [chord_of, key, typed] = keyed_as.split(' ').collect::<Vec<_>>()[..] else {
                panic!("{line}")
            };
            let (key, typed) = (character(key), character(typed));
            let shift = chord_of.contains("shift");
            let held = Held {
                control: chord_of.contains("ctrl"),
                shift,
                ..Held::default()
            };
            // Caps Lock is in the key event's flags, which the window
            // crate's modifiers do not carry.
            let flags = match chord_of.contains("caps") {
                true => NSEventModifierFlags::CapsLock,
                false => NSEventModifierFlags(0),
            };
            let k = KeyPress {
                key_code: code_of(key),
                characters: match held.control {
                    true => char::from_u32(u32::from(typed) & 0x1f).expect("C0"),
                    false => typed,
                }
                .to_string(),
                // `charactersIgnoringModifiers` keeps Shift and drops Caps
                // Lock, as AppKit gives it.
                unmodified: match shift {
                    true => typed,
                    false => key,
                }
                .to_string(),
                modifiers: held,
                repeat: false,
            };
            let m = held_with_locks(&k, flags);
            let unshifted = Some(key).filter(|_| shift);
            let e = match held.control {
                false => {
                    let text = typed.to_string();
                    typed_text(
                        &text,
                        Some(&keyed(&text, Codepoint::from_char(key), m, unshifted)),
                    )[0]
                }
                true => chord(&k, m, unshifted.map(Codepoint::from_char)).expect("a chord"),
            };
            assert_eq!(e.code, KeyCode::Char(Codepoint::from_char(key)), "{line}");
            assert_eq!(e.mods.has(Modifiers::SHIFT), shift, "{line}");
            assert_eq!(
                e.mods.has(Modifiers::CAPS_LOCK),
                chord_of.contains("caps"),
                "{line}"
            );
            assert_eq!(
                e.shifted,
                Some(Codepoint::from_char(typed)).filter(|_| typed != key && shift),
                "{line}"
            );
            let modes = match mode.split_once('=') {
                None => String::new(),
                Some(("kitty", n)) => format!("\x1b[>{n}u"),
                Some(("mok", n)) => format!("\x1b[>4;{n}m"),
                _ => panic!("{mode}"),
            };
            assert_eq!(core_sends(modes.as_bytes(), e), unescape(want), "{line}");
            rows += 1;
        }
        assert_eq!(rows, 37, "the table's every row");
    }

    /// **Caps Lock is read from the key event's flags**: AppKit sets
    /// `NSEventModifierFlagCapsLock` in a key event made while the lock is
    /// on, and the key is encoded with Caps Lock's bit, which the window
    /// crate's modifiers do not carry. Asked of AppKit itself, as the
    /// syscall layouts are asked of the headers: a probe compiled now turns
    /// a keyboard event with the HID's lock flag, and one without, into
    /// `NSEvent`s off screen, nothing posted, and prints the SDK's mask and
    /// their flags. Caps Lock is no chord, so its capital is predicted as
    /// typed. `key` reads the dispatched event's flags, held in the source:
    /// off the main thread there is none. Perturbation: read no lock from
    /// the flags and the Caps rows of the shared table lose their `64`.
    #[test]
    fn caps_lock_is_read_from_the_key_events_flags() {
        // Indented: a `}` at the margin would end the test module where
        // the tests that read this file look for its end.
        const PROBE: &str = r#"
            #import <AppKit/AppKit.h>
            static unsigned long flags(CGEventFlags hid) {
                CGEventRef cg = CGEventCreateKeyboardEvent(NULL, 0, true);
                CGEventSetFlags(cg, hid);
                unsigned long f = (unsigned long)[[NSEvent eventWithCGEvent:cg] modifierFlags];
                CFRelease(cg);
                return f;
            }
            int main(void) {
                @autoreleasepool {
                    printf("mask=%lu\n", (unsigned long)NSEventModifierFlagCapsLock);
                    printf("on=%lu\n", flags(kCGEventFlagMaskAlphaShift));
                    printf("off=%lu\n", flags(0));
                }
                return 0;
            }
"#;
        let dir = std::env::temp_dir().join(format!("baud-caps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let (src, bin) = (dir.join("probe.m"), dir.join("probe"));
        std::fs::write(&src, PROBE).expect("write the probe");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let out = std::process::Command::new(&cc)
            .args(["-fobjc-arc", "-framework", "AppKit", "-o"])
            .arg(&bin)
            .arg(&src)
            .output()
            .unwrap_or_else(|e| panic!("{cc} did not run: {e}"));
        assert!(
            out.status.success(),
            "the probe did not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = std::process::Command::new(&bin).output().expect("run it");
        let _ = std::fs::remove_dir_all(&dir);
        let text = String::from_utf8_lossy(&run.stdout).into_owned();
        let said: std::collections::HashMap<&str, usize> = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .filter_map(|(k, v)| v.parse().ok().map(|n| (k, n)))
            .collect();
        assert_eq!(said.len(), 3, "the probe's three values: {text}");
        assert_eq!(
            said["mask"],
            NSEventModifierFlags::CapsLock.0,
            "the SDK's mask"
        );
        let k = KeyPress {
            key_code: 0,
            characters: "A".to_string(),
            unmodified: "a".to_string(),
            modifiers: macos_liquid_glass::drawable::Modifiers::default(),
            repeat: false,
        };
        let on = held_with_locks(&k, NSEventModifierFlags(said["on"]));
        let off = held_with_locks(&k, NSEventModifierFlags(said["off"]));
        assert_eq!((on, off), (Modifiers::CAPS_LOCK, Modifiers::NONE));
        assert!(!is_chord(on), "Caps Lock's capital is typed text");
        // Caps Lock with Shift: AppKit types `A` on the US layout, the key
        // Shift is on with both bits.
        let a = Codepoint::from_char('a');
        let both = typed_text(
            "A",
            Some(&keyed("A", a, Modifiers::SHIFT.with(on), Some('a'))),
        )[0];
        assert_eq!(core_sends(b"", both), b"A");
        assert_eq!(core_sends(b"\x1b[>8u", both), b"\x1b[97;66u");
        assert_eq!(core_sends(b"\x1b[>12u", both), b"\x1b[97:65;66u");
        assert!(is_chord(Modifiers::CTRL.with(on)));
        let src = include_str!("app.rs");
        let key = src
            .split(concat!(
                "    fn key(&mut self, key: ",
                "&KeyPress) -> Handled {"
            ))
            .nth(1)
            .expect("key");
        let key = &key[..key.find("\n    }\n").expect("its end")];
        let read = key
            .find(concat!("let flags = ", "current_event_flags();"))
            .expect("the dispatched event's flags");
        let m = key
            .find(concat!("let m = held_with_locks(", "key, flags);"))
            .expect("the key's modifiers from them");
        assert!(read < m);
    }

    /// What `key` sends for `k` once `modes` is fed: the event it builds,
    /// through the core.
    fn sent_for(modes: &[u8], k: &KeyPress) -> Vec<u8> {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        t.write_at(baud_domain::BytePos::ZERO, modes);
        t.feed(
            baud_domain::BytePos::ZERO,
            baud_domain::ReadLen::from_index(modes.len()),
        );
        t.clear_reply();
        let e = special(k)
            .map(|code| pressed(code, k, mods(k.modifiers)))
            .expect("a named key");
        let mut out = Vec::new();
        crate::reads::input(&mut t, |t| t.key(e), |b| out.extend_from_slice(b));
        out
    }

    /// **A Mac keypad key reaches the core as a keypad key**, so DECKPAM,
    /// DECNKM and kitty's keypad numbers act on it as on a PC keypad's,
    /// and a digit carries the text it types, as the Wayland driver's
    /// does: under kitty's disambiguate flag alone a typed `5` stays `5`
    /// ("all non text keypad keys" are the ones reported, `[[source:
    /// kitty-keyboard]]`). No NumLock is reported, a Mac keypad having no
    /// NumLock state, so `?1035` leaves DECKPAM as it is.
    #[test]
    fn a_mac_keypad_key_reaches_the_core_as_a_keypad_key() {
        use macos_liquid_glass::drawable::Modifiers as M;
        let none = M::default();
        let option = M {
            option: true,
            ..none
        };
        let key = |code: u16, m| {
            let chars = MAC_KEYPAD
                .iter()
                .find(|(c, _, _)| *c == code)
                .map_or("\u{f739}", |(_, _, ch)| *ch);
            mac_key!(code, chars, m)
        };
        let decnkm = b"\x1b[?66h".as_slice();
        for (modes, code, m, want) in [
            // Numeric, the power-on column: the character on the key.
            (b"".as_slice(), 0x57, none, b"5".as_slice()),
            (b"", 0x51, none, b"="),
            (b"", 0x4C, none, b"\r"),
            (b"", 0x47, none, b""),
            // DECKPAM, `ESC =`: line 2628's application column.
            (b"\x1b=", 0x57, none, b"\x1bOu"),
            (b"\x1b=", 0x4C, none, b"\x1bOM"),
            (b"\x1b=", 0x51, none, b"\x1bOX"),
            (b"\x1b=", 0x45, none, b"\x1bOk"),
            (b"\x1b=", 0x43, none, b"\x1bOj"),
            (b"\x1b=", 0x4B, none, b"\x1bOo"),
            (b"\x1b=", 0x4E, none, b"\x1bOm"),
            (b"\x1b=", 0x41, none, b"\x1bOn"),
            (b"\x1b=", 0x47, none, b""),
            // DECNKM set is DECKPAM; `?1035` reset changes nothing here.
            (decnkm, 0x45, none, b"\x1bOk"),
            (b"\x1b=\x1b[?1035l", 0x45, none, b"\x1bOk"),
            // kitty, disambiguate: a typed key keeps its legacy bytes, a
            // key that typed nothing is its number.
            (b"\x1b[>1u", 0x57, none, b"5"),
            (b"\x1b[>1u", 0x57, option, b"\x1b[57404;3u"),
            (b"\x1b[>1u", 0x4C, none, b"\x1b[57414u"),
            (b"\x1b[>1u", 0x47, none, b""),
            // kitty, report all keys: every keypad key its number, Clear
            // NumLock's.
            (b"\x1b[>9u", 0x57, none, b"\x1b[57404u"),
            (b"\x1b[>9u", 0x51, none, b"\x1b[57415u"),
            (b"\x1b[>9u", 0x41, none, b"\x1b[57409u"),
            (b"\x1b[>9u", 0x47, none, b"\x1b[57360u"),
        ] {
            assert_eq!(
                sent_for(modes, &key(code, m)),
                want,
                "{code:#x} after {:?}",
                String::from_utf8_lossy(modes)
            );
        }
        // Each digit under DECKPAM sends what the core sends for that
        // keypad key, and not the main row's digit.
        for (code, k, chars) in MAC_KEYPAD {
            let mut e = KeyEvent::press(KeyCode::Keypad(k), Modifiers::NONE);
            e.text = chars
                .chars()
                .next()
                .filter(|c| !c.is_control())
                .and_then(|c| Codepoint::new(u32::from(c)));
            let ws = WinSize {
                ws_col: 20,
                ws_row: 4,
            };
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            t.write_at(baud_domain::BytePos::ZERO, b"\x1b=");
            t.feed(
                baud_domain::BytePos::ZERO,
                baud_domain::ReadLen::from_index(2),
            );
            let mut core = Vec::new();
            crate::reads::input(&mut t, |t| t.key(e), |b| core.extend_from_slice(b));
            let got = sent_for(b"\x1b=", &key(code, none));
            assert_eq!(got, core, "{code:#x}");
            assert_ne!(got, chars.as_bytes(), "{code:#x}");
        }
    }

    /// **A keypad key is predicted only from the numeric column**: under
    /// DECKPAM a `5` goes out as `SS3 u`, and a `5` drawn ahead of the
    /// child's echo would be a glyph no echo confirms.
    #[test]
    fn a_keypad_key_is_predicted_only_where_it_sends_its_text() {
        use baud_domain::KeypadKey::Digit5;
        assert!(predicts(KeyCode::Keypad(Digit5), KeypadMode::Numeric));
        assert!(!predicts(KeyCode::Keypad(Digit5), KeypadMode::Application));
        let five = KeyCode::Char(Codepoint::new(u32::from('5')).unwrap());
        assert!(predicts(five, KeypadMode::Application));
    }

    /// **A Mac keypad key sends what the layout types on it**: the decimal
    /// key types `,` on 81 of the Mac's layouts, German and French among
    /// them, and the digits type Arabic-Indic digits on Arabic ones
    /// (`UCKeyTranslate`). From the numeric column, and under kitty's
    /// disambiguate flag, that character goes out and is the text handed to
    /// prediction, which draws only an ASCII printable (`Driver::predict`).
    /// A decimal key that types `,` is the keypad's comma, as a German
    /// Linux keypad's is (`KP_Separator`, `[[source:
    /// xkeyboard-config-kpdl-193cc6c]]`): line 2498's `SS3 l` under DECKPAM
    /// and kitty's `KP_SEPARATOR`, whatever Option does to the text.
    #[test]
    fn a_mac_keypad_key_sends_what_the_layout_types_on_it() {
        use baud_domain::KeypadKey as Kp;
        use macos_liquid_glass::drawable::Modifiers as M;
        let none = M::default();
        let option = M {
            option: true,
            ..none
        };
        let five = "\u{665}";
        for (modes, code, chars, want) in [
            (b"".as_slice(), 0x41, ",", b",".as_slice()),
            (b"", 0x57, five, five.as_bytes()),
            (b"\x1b[>1u", 0x41, ",", b","),
            (b"\x1b[>1u", 0x57, five, five.as_bytes()),
            (b"\x1b=", 0x41, ",", b"\x1bOl"),
            (b"\x1b=", 0x41, ".", b"\x1bOn"),
            (b"\x1b=", 0x57, five, b"\x1bOu"),
            (b"\x1b[>9u", 0x41, ",", b"\x1b[57416u"),
            (b"\x1b[>9u", 0x41, ".", b"\x1b[57409u"),
        ] {
            let k = mac_key!(code, chars, none);
            assert_eq!(sent_for(modes, &k), want, "{code:#x} {chars:?} {modes:?}");
            if modes.is_empty() {
                let e = pressed(special(&k).unwrap(), &k, Modifiers::NONE);
                let drawn = e.text.filter(|_| predicts(e.code, KeypadMode::Numeric));
                let drawn = drawn.and_then(|c| char::from_u32(c.value()));
                assert_eq!(drawn.map(String::from).as_deref(), Some(chars));
            }
        }
        // The identity is the key's with Option ignored, so an Option chord
        // that changes the characters does not change the key.
        let chord = KeyPress {
            characters: "\u{2026}".to_string(),
            ..mac_key!(0x41, ",", option)
        };
        assert_eq!(special(&chord), Some(KeyCode::Keypad(Kp::Comma)));
        assert_eq!(
            special(&mac_key!(0x41, "\u{66b}", none)),
            Some(KeyCode::Keypad(Kp::Period))
        );
    }

    /// A key AppKit marks as repeated reaches the core as a repeat, which it
    /// sends as the press unless an application asked for kitty's event
    /// types; an unmarked key is a press.
    #[test]
    fn a_repeated_key_is_a_repeat_and_a_fresh_one_a_press() {
        let key = |repeat| KeyPress {
            key_code: 0x7E,
            characters: String::new(),
            unmodified: String::new(),
            modifiers: macos_liquid_glass::drawable::Modifiers::default(),
            repeat,
        };
        assert_eq!(kind_of(&key(true)), KeyEventKind::Repeat);
        assert_eq!(kind_of(&key(false)), KeyEventKind::Press);
    }

    /// **A key's release reaches the core as its press let go**: the key
    /// its press named, with the modifiers held at the release, sent only
    /// where kitty's event types asked for releases -- `CSI 1;1:3A` for Up
    /// and `CSI 97;1:3u` for `a` under "report all keys" -- and nothing on
    /// the legacy layer, which has no release. Perturbation: send the
    /// release as a press and the event-type rows read the press.
    #[test]
    fn a_key_release_reaches_the_core_as_its_press_let_go() {
        use macos_liquid_glass::drawable::Modifiers as M;
        let none = M::default();
        let up = mac_key!(0x7E, "\u{f700}", none);
        let press = pressed(special(&up).expect("Up"), &up, mods(none));
        let a = Codepoint::from_char('a');
        let typed = typed_text("a", Some(&keyed("a", a, Modifiers::NONE, None)))[0];
        for (modes, code, e, want) in [
            (b"".as_slice(), 0x7E, press, b"".as_slice()),
            (b"\x1b[>3u", 0x7E, press, b"\x1b[1;1:3A"),
            (b"\x1b[>11u", 0x00, typed, b"\x1b[97;1:3u"),
            // Event types without "report all keys": a text key's release
            // is not reported, as its press is its text.
            (b"\x1b[>3u", 0x00, typed, b""),
        ] {
            let mut downs = Downs::NONE;
            downs.pressed(VirtualKey(code), e);
            let release = downs
                .released(VirtualKey(code), mods(none))
                .expect("its press went");
            assert_eq!(release.kind, KeyEventKind::Release);
            assert_eq!(release.code, e.code, "{code:#x}");
            assert_eq!(
                core_sends(modes, release),
                want,
                "{code:#x} after {:?}",
                String::from_utf8_lossy(modes)
            );
        }
    }

    /// **A release goes to the program only when its press did, and
    /// once**: a key whose press the program never got -- a Command chord,
    /// a composition, the find field -- has nothing to release, and a
    /// second release of one press is dropped. A repeat keeps the key held.
    /// Perturbation: release whatever comes up and the orphan reads a key.
    #[test]
    fn a_release_without_its_press_or_a_second_one_sends_nothing() {
        let none = Modifiers::NONE;
        let up = VirtualKey(0x7E);
        let mut downs = Downs::NONE;
        assert_eq!(downs.released(up, none), None, "never pressed");
        let mut e = KeyEvent::press(KeyCode::Up, none);
        downs.pressed(up, e);
        e.kind = KeyEventKind::Repeat;
        downs.pressed(up, e);
        let release = downs.released(up, Modifiers::SHIFT).expect("held");
        assert_eq!(release.kind, KeyEventKind::Release);
        assert_eq!(
            release.mods,
            Modifiers::SHIFT,
            "the modifiers at the release"
        );
        assert_eq!(downs.released(up, none), None, "released twice");
        // A key code past the table is no key the Mac names.
        downs.pressed(VirtualKey(0x80), e);
        assert_eq!(downs.released(VirtualKey(0x80), none), None);
    }

    /// **The keys still held when the window loses the keyboard are let
    /// go**, each its press released with no key held, and once: the table
    /// is empty after. Caps Lock's bit rides them while the lock is on, as
    /// the last key event the view was handed said, kitty's "if ... the
    /// lock (for caps lock/num lock) is enabled, the key event must have
    /// the bit for that modifier set" (`[[source: kitty-keyboard]]`): with
    /// the lock on, Command let go is `CSI 57444;65:3u` under "report all
    /// keys" with event types, and with a key-up after that turned it off,
    /// `CSI 57444;1:3u`. The handlers note every key event's flags, held in
    /// the source. Perturbations: read the table without emptying it and
    /// the second loss releases them again; let go with no lock and the
    /// lock's row loses its `64`; note no lock at a key-up and the row
    /// after the lock turned off keeps it.
    #[test]
    fn the_keys_held_when_the_window_loses_the_keyboard_are_let_go() {
        let mut downs = Downs::NONE;
        let up = KeyEvent::press(KeyCode::Up, Modifiers::SHIFT);
        let shift = KeyEvent::press(
            KeyCode::Pua(baud_domain::PuaKey::LeftShift),
            Modifiers::SHIFT,
        );
        downs.pressed(VirtualKey(0x7E), up);
        downs.pressed(VirtualKey(0x38), shift);
        let gone: Vec<KeyEvent> = downs.let_go().collect();
        assert_eq!(gone.len(), 2);
        assert!(gone.iter().all(|e| e.kind == KeyEventKind::Release));
        assert!(gone.iter().all(|e| e.mods == Modifiers::NONE));
        // By key code: Left Shift is 0x38, Up 0x7E.
        assert_eq!(core_sends(b"\x1b[>11u", gone[0]), b"\x1b[57441;1:3u");
        assert_eq!(core_sends(b"\x1b[>3u", gone[1]), b"\x1b[1;1:3A");
        assert_eq!(downs.let_go().count(), 0, "let go once");
        assert_eq!(downs.released(VirtualKey(0x7E), Modifiers::NONE), None);
        // Caps Lock on: the Command key went down with the lock on, then
        // Command-Tab took the keyboard.
        let (lock, off) = (NSEventModifierFlags::CapsLock, NSEventModifierFlags(0));
        let all = b"\x1b[>11u".as_slice();
        let command = KeyEvent::press(
            KeyCode::Pua(baud_domain::PuaKey::LeftSuper),
            Modifiers::SUPER.with(Modifiers::CAPS_LOCK),
        );
        downs.saw(lock);
        downs.pressed(VirtualKey(0x37), command);
        let gone: Vec<KeyEvent> = downs.let_go().collect();
        assert_eq!(gone.len(), 1);
        assert_eq!(core_sends(all, gone[0]), b"\x1b[57444;65:3u");
        // A key-up after the lock turned off: the let-go carries no lock.
        let a = KeyPress {
            key_code: 0x00,
            characters: "a".to_string(),
            unmodified: "a".to_string(),
            modifiers: macos_liquid_glass::drawable::Modifiers::default(),
            repeat: false,
        };
        downs.saw(lock);
        downs.pressed(VirtualKey(0x37), command);
        assert_eq!(downs.up(&a, off), None, "a key never pressed");
        let gone: Vec<KeyEvent> = downs.let_go().collect();
        assert_eq!(core_sends(all, gone[0]), b"\x1b[57444;1:3u");
        // Every key event's flags are noted: a press, a release, a change.
        const SRC: &str = include_str!("app.rs");
        let body = |head: &str| {
            let b = SRC.split(head).nth(1).expect(head);
            b[..b.find("\n    }\n").expect("its end")].to_string()
        };
        let key = body(concat!(
            "    fn key(&mut self, key: ",
            "&KeyPress) -> Handled {"
        ));
        assert!(key.contains(concat!("self.downs.", "saw(flags);")));
        let changed = body(concat!(
            "    fn modifiers_changed(&mut self, ",
            "change: &ModifierChange) {"
        ));
        assert!(changed.contains(concat!("self.downs.", "saw(flags);")));
        let up = body(concat!(
            "    fn up(&mut self, key: &KeyPress, ",
            "flags: NSEventModifierFlags) -> Option<KeyEvent> {"
        ));
        assert!(up.contains(concat!("self.", "saw(flags);")));
    }

    /// **A modifier key pressed while the find field is open sends
    /// nothing**, and leaves no press for its release to name; with the
    /// keys the program's, its press is sent and its release follows.
    /// The source holds the handler to it. Perturbation: drop the find
    /// field's arm and Shift reaches the program from the field.
    #[test]
    fn a_modifier_key_pressed_into_the_find_field_sends_nothing() {
        let shift = KeyCode::Pua(baud_domain::PuaKey::LeftShift);
        let down = KeyEvent::press(shift, Modifiers::SHIFT);
        let up = KeyEvent {
            kind: KeyEventKind::Release,
            ..KeyEvent::press(shift, Modifiers::NONE)
        };
        let key = VirtualKey(0x38);
        let mut downs = Downs::NONE;
        assert_eq!(downs.modifier(key, down, KeysTo::FindField), None);
        assert_eq!(downs.modifier(key, up, KeysTo::FindField), None);
        assert_eq!(downs.modifier(key, up, KeysTo::Program), None);
        assert_eq!(downs.modifier(key, down, KeysTo::Program), Some(down));
        let released = downs
            .modifier(key, up, KeysTo::Program)
            .expect("its release");
        assert_eq!(core_sends(b"\x1b[>11u", released), b"\x1b[57441;1:3u");
        // A release the field saw go down still goes: the press was sent.
        assert_eq!(downs.modifier(key, down, KeysTo::Program), Some(down));
        assert!(downs.modifier(key, up, KeysTo::FindField).is_some());
        // The source holds the handler to the field's arm.
        const SRC: &str = include_str!("app.rs");
        let changed = SRC
            .split(concat!(
                "    fn modifiers_changed(&mut self, ",
                "change: &ModifierChange) {"
            ))
            .nth(1)
            .expect("modifiers_changed");
        let changed = &changed[..changed.find("\n    }\n").expect("its end")];
        assert!(changed.contains(concat!("Some(_) => ", "KeysTo::FindField")));
    }

    /// **A commit of more than one character records no press**: it is no
    /// one key's, so the key handed to the input manager gets no release
    /// from it; a commit of one character is that key's. The source holds
    /// `text` to it. Perturbation: keep the key for any commit and the
    /// second character's press is recorded under it.
    #[test]
    fn a_commit_of_more_than_one_character_records_no_press() {
        let a = VirtualKey(0x00);
        assert_eq!(commit_key(Some(a), "a"), Some(a));
        assert_eq!(commit_key(Some(a), "\u{e9}"), Some(a));
        assert_eq!(commit_key(Some(a), "ab"), None);
        assert_eq!(commit_key(Some(a), "\u{65e5}\u{672c}"), None);
        assert_eq!(commit_key(None, "a"), None);
        const SRC: &str = include_str!("app.rs");
        let body = SRC
            .split(concat!("    fn text(&mut self, ", "text: &str) {"))
            .nth(1)
            .expect("text");
        let body = &body[..body.find("\n    }\n").expect("its end")];
        assert!(body.contains(concat!("self.handed.", "text(text)")));
    }

    /// **A key the input manager types is released in the window crate's
    /// own order**: its `keyDown:` hands the key to `key`, which answers
    /// `Handled::Interpret`; `interpretKeyEvents:` reaches `insertText:`,
    /// which calls `marked_text("")` and then `text`; its `keyUp:` reaches
    /// `key_released`. That empty composition is no composition, so the
    /// press of `a` is recorded under its key and its release sent,
    /// `CSI 97;1:3u` under "report all keys" with event types, and Shift+a
    /// `CSI 97;2u` and `CSI 97;2:3u`, and `a` with Caps Lock on
    /// `CSI 97;65u` and `CSI 97;65:3u`. The source holds the handlers to
    /// the order. Perturbation: drop the key at every composition change,
    /// the empty one too, as the handler did, and no press is recorded and
    /// no release sent.
    #[test]
    fn a_typed_key_is_released_in_the_window_crates_order() {
        use macos_liquid_glass::drawable::Modifiers as M;
        let all = b"\x1b[>11u".as_slice();
        let none = M::default();
        let shift = M {
            shift: true,
            ..none
        };
        let (lock, off) = (NSEventModifierFlags::CapsLock, NSEventModifierFlags(0));
        let a = Codepoint::from_char('a');
        for (typed, held, flags, mods, unshifted, down, up) in [
            (
                "a",
                none,
                off,
                Modifiers::NONE,
                None,
                b"\x1b[97u".as_slice(),
                b"\x1b[97;1:3u".as_slice(),
            ),
            (
                "A",
                shift,
                off,
                Modifiers::SHIFT,
                Some('a'),
                b"\x1b[97;2u",
                b"\x1b[97;2:3u",
            ),
            (
                "A",
                none,
                lock,
                Modifiers::CAPS_LOCK,
                None,
                b"\x1b[97;65u",
                b"\x1b[97;65:3u",
            ),
        ] {
            let key = mac_key!(0x00, typed, held);
            let (mut handed, mut downs) = (Handed::NONE, Downs::NONE);
            // keyDown: `key` hands it to the input manager.
            handed.key(VirtualKey(key.key_code));
            // insertText: the crate's two calls, in its order.
            handed.marked("");
            let code = handed.text(typed);
            assert_eq!(
                code,
                Some(VirtualKey(0x00)),
                "{typed:?}: the key's own commit"
            );
            let k = keyed(typed, a, mods, unshifted);
            let press = typed_text(typed, Some(&k))[0];
            assert_eq!(core_sends(all, press), down, "{typed:?} {flags:?}");
            if let Some(code) = code {
                downs.pressed(code, press);
            }
            // keyUp.
            handed.up(VirtualKey(key.key_code));
            let release = downs.up(&key, flags).expect("its release is sent");
            assert_eq!(core_sends(all, release), up, "{typed:?} {flags:?}");
        }
        // The handlers keep the order: the key handed at `key`, an empty
        // composition leaving it, the commit taking it, the key-up
        // dropping it.
        const SRC: &str = include_str!("app.rs");
        let body = |head: &str| {
            let b = SRC.split(head).nth(1).expect(head);
            b[..b.find("\n    }\n").expect("its end")].to_string()
        };
        let key = body(concat!(
            "    fn key(&mut self, key: ",
            "&KeyPress) -> Handled {"
        ));
        assert!(key.contains(concat!("self.handed = ", "Handed::NONE;")));
        assert!(key.contains(concat!(
            "self.handed.key(",
            "VirtualKey(key.key_code));\n            return Handled::Interpret;"
        )));
        let marked = body(concat!("    fn marked_text(&mut self, ", "text: &str) {"));
        assert!(marked.contains(concat!("self.handed.", "marked(text);")));
        let text = body(concat!("    fn text(&mut self, ", "text: &str) {"));
        assert!(text.contains(concat!("self.typed(", "e, code);")));
        let released = body(concat!(
            "    fn key_released(&mut self, ",
            "key: &KeyPress) {"
        ));
        assert!(released.contains(concat!("self.handed.up(", "VirtualKey(key.key_code));")));
    }

    /// **A dead key and the letter it composes release nothing**, and a
    /// commit while the dead key is still down is no key's: the dead key's
    /// `keyDown:` makes the composition `´`, which drops the key handed,
    /// and the letter after it, typed while the composition is open, is
    /// never handed (`composing_first`), so its commit `é`, after the
    /// crate's `marked_text("")`, records no press. Perturbation: keep the
    /// key at a composition that is something and the commit while the
    /// dead key is down is recorded under it.
    #[test]
    fn a_dead_key_and_the_letter_it_composes_release_nothing() {
        let (quote, e) = (VirtualKey(0x27), VirtualKey(0x0E));
        let mut handed = Handed::NONE;
        let mut downs = Downs::NONE;
        let off = NSEventModifierFlags(0);
        let up = |code: VirtualKey| KeyPress {
            key_code: code.0,
            characters: String::new(),
            unmodified: String::new(),
            modifiers: macos_liquid_glass::drawable::Modifiers::default(),
            repeat: false,
        };
        // The dead key: handed, then the composition it made.
        handed.key(quote);
        handed.marked("\u{b4}");
        // Committed while the dead key is down: no key's.
        let mut early = handed;
        early.marked("");
        assert_eq!(early.text("\u{b4}"), None, "the dead key typed nothing");
        handed.up(quote);
        assert_eq!(downs.up(&up(quote), off), None, "no release for it");
        // The letter: `key` resets and returns before handing it.
        handed = Handed::NONE;
        handed.marked("");
        let code = handed.text("\u{e9}");
        assert_eq!(code, None, "the composed letter is no key's");
        if let Some(code) = code {
            downs.pressed(
                code,
                KeyEvent::press(
                    KeyCode::Char(Codepoint::from_char('\u{e9}')),
                    Modifiers::NONE,
                ),
            );
        }
        handed.up(e);
        assert_eq!(downs.up(&up(e), off), None, "and no release for it");
    }

    /// **A commit after its key came up is no key's**: a key handed to the
    /// input manager that typed nothing drops off when it comes up, so a
    /// commit after -- the character palette's, an input method's on a
    /// click -- records no press, while another key's key-up leaves it.
    /// Perturbation: keep the key past its key-up and the palette's
    /// character is recorded under it.
    #[test]
    fn a_commit_after_its_key_came_up_is_no_keys() {
        let (a, b) = (VirtualKey(0x00), VirtualKey(0x0B));
        let mut handed = Handed::NONE;
        handed.key(a);
        handed.up(b);
        assert_eq!(handed, Handed(Some(a)), "another key's key-up");
        handed.up(a);
        assert_eq!(handed.text("x"), None, "the palette's character");
        // The control: a command the input manager made of the key takes it.
        handed.key(a);
        assert_eq!(handed.command(), Some(a));
        assert_eq!(handed.command(), None, "taken once");
    }

    /// The source holds where the keys are let go: on a focus change to
    /// lost, in `set_focus`, which every poll and the crate's hook reach.
    #[test]
    fn a_focus_lost_lets_go_of_the_keys_held() {
        const SRC: &str = include_str!("app.rs");
        let head = concat!("    fn set_focus(&mut self, ", "f: baud_core::Focus) {");
        let b = SRC.split(head).nth(1).expect("set_focus");
        let b = &b[..b.find("\n    }\n").expect("its end")];
        let lost = b
            .find(concat!("if f == baud_core::", "Focus::Lost {"))
            .expect("the lost branch");
        let let_go = b
            .find(concat!("self.downs.", "let_go()"))
            .expect("the keys let go");
        assert!(lost < let_go);
    }

    /// **A modifier key alone reaches the core as kitty's functional key**,
    /// under "report all keys" and nowhere else: Left Shift `57441`, the
    /// modifiers held after the change -- Shift set on its press, clear on
    /// its release, Control still set when the other Control key holds it
    /// -- Command as super, Option as alt, Caps Lock carrying its lock
    /// while on, and `fn` none (`[[source: kitty-keyboard]]`,
    /// `[[source: kitty-glfw-cocoa-window-851cb80]]`). Perturbation: swap
    /// the left and right keys and the side rows read the other number.
    #[test]
    fn a_modifier_key_alone_reaches_the_core_under_report_all_keys() {
        use macos_liquid_glass::drawable::{ModifierKey as K, Modifiers as M};
        let none = M::default();
        let shift = M {
            shift: true,
            ..none
        };
        let control = M {
            control: true,
            ..none
        };
        let command = M {
            command: true,
            ..none
        };
        let option = M {
            option: true,
            ..none
        };
        let all = b"\x1b[>11u".as_slice();
        use KeyEventKind::{Press as Down, Release as Up};
        for (modes, key, kind, held, want) in [
            (all, K::LeftShift, Down, shift, b"\x1b[57441;2u".as_slice()),
            (all, K::LeftShift, Up, none, b"\x1b[57441;1:3u"),
            (all, K::RightShift, Down, shift, b"\x1b[57447;2u"),
            (all, K::RightControl, Up, control, b"\x1b[57448;5:3u"),
            (all, K::LeftControl, Down, control, b"\x1b[57442;5u"),
            (all, K::LeftCommand, Down, command, b"\x1b[57444;9u"),
            (all, K::RightCommand, Up, none, b"\x1b[57450;1:3u"),
            (all, K::LeftOption, Down, option, b"\x1b[57443;3u"),
            (all, K::RightOption, Down, option, b"\x1b[57449;3u"),
            (all, K::CapsLock, Down, none, b"\x1b[57358;65u"),
            (all, K::CapsLock, Up, none, b"\x1b[57358;1:3u"),
            // "Report all keys" alone reports the press and no release.
            (b"\x1b[>8u", K::LeftShift, Down, shift, b"\x1b[57441;2u"),
            (b"\x1b[>8u", K::LeftShift, Up, none, b""),
            // Below it, and on the legacy layer, a modifier key is silent.
            (b"\x1b[>3u", K::LeftShift, Down, shift, b""),
            (b"", K::LeftShift, Down, shift, b""),
        ] {
            // The lock is on after the change that turns it on.
            let flags = match (key, kind) {
                (K::CapsLock, Down) => NSEventModifierFlags::CapsLock,
                _ => NSEventModifierFlags(0),
            };
            let e = modifier_event(Some(key), kind, held, flags).expect("a kitty key");
            assert_eq!(
                core_sends(modes, e),
                want,
                "{key:?} {kind:?} after {:?}",
                String::from_utf8_lossy(modes)
            );
        }
        let off = NSEventModifierFlags(0);
        assert_eq!(modifier_event(Some(K::Function), Down, none, off), None);
        assert_eq!(modifier_event(None, Down, command, off), None);
    }

    /// **A Shift+letter's press and release name one key**: Shift+a goes
    /// down as the `a` key with Shift and comes up as it, `CSI 97;2u` and
    /// `CSI 97;2:3u` under "report all keys" with event types, as kitty
    /// reports `shift+a` (`[[source: kitty-keyboard]]`), and the release
    /// after Shift is let go first carries no Shift, the modifiers "active
    /// when the key event occurs". Perturbations: build the press without
    /// the Shift the input manager's key held and it reads `CSI 97u`; take
    /// the release's modifiers from its press and the late release keeps
    /// the Shift already let go.
    #[test]
    fn a_shift_letters_press_and_release_name_one_key() {
        use macos_liquid_glass::drawable::Modifiers as M;
        let all = b"\x1b[>11u".as_slice();
        let none = M::default();
        let shift = M {
            shift: true,
            ..none
        };
        let a = Codepoint::from_char('a');
        let press = typed_text("A", Some(&keyed("A", a, Modifiers::SHIFT, Some('a'))))[0];
        assert_eq!(core_sends(all, press), b"\x1b[97;2u");
        let up = |held| KeyPress {
            key_code: 0x00,
            characters: "A".to_string(),
            unmodified: "A".to_string(),
            modifiers: held,
            repeat: false,
        };
        let off = NSEventModifierFlags(0);
        let mut downs = Downs::NONE;
        downs.pressed(VirtualKey(0x00), press);
        let release = downs.up(&up(shift), off).expect("its press went");
        assert_eq!(core_sends(all, release), b"\x1b[97;2:3u");
        // Shift let go before the letter.
        downs.pressed(VirtualKey(0x00), press);
        let late = downs.up(&up(none), off).expect("its press went");
        assert_eq!(core_sends(all, late), b"\x1b[97;1:3u");
    }

    /// **Caps Lock's bit rides a release and a modifier key while the lock
    /// is on**, read from the event's own flags as a press's is
    /// (`held_with_locks`): "if a modifier is active when the key event
    /// occurs, i.e. if the key is pressed or the lock (for caps lock/num
    /// lock) is enabled, the key event must have the bit for that modifier
    /// set" (`[[source: kitty-keyboard]]`). With the lock on, `a` goes down
    /// as `CSI 97;65u` and up as `CSI 97;65:3u`, and Shift alone as
    /// `CSI 57441;66u` and `CSI 57441;65:3u`, under "report all keys" with
    /// event types. The lock's own key carries the lock as the change
    /// leaves it, set at the press that turns it on and clear at the
    /// release that turns it off. The handlers read the dispatched event's
    /// flags, held in the source. Perturbations: read the release's
    /// modifiers from the window crate's alone and the release of `a`
    /// loses its `64`; read no lock for a modifier key but the lock's own
    /// and Shift's rows lose theirs.
    #[test]
    fn caps_lock_rides_a_release_and_a_modifier_key() {
        use KeyEventKind::{Press as Down, Release as Up};
        use macos_liquid_glass::drawable::{ModifierKey as K, Modifiers as M};
        let all = b"\x1b[>11u".as_slice();
        let none = M::default();
        let shift = M {
            shift: true,
            ..none
        };
        let (lock, off) = (NSEventModifierFlags::CapsLock, NSEventModifierFlags(0));
        // `a` with the lock on types `A`: the `a` key with Caps Lock's bit.
        let a = Codepoint::from_char('a');
        let press = typed_text("A", Some(&keyed("A", a, Modifiers::CAPS_LOCK, None)))[0];
        assert_eq!(core_sends(all, press), b"\x1b[97;65u");
        let up = KeyPress {
            key_code: 0x00,
            characters: "A".to_string(),
            unmodified: "a".to_string(),
            modifiers: none,
            repeat: false,
        };
        let mut downs = Downs::NONE;
        downs.pressed(VirtualKey(0x00), press);
        let release = downs.up(&up, lock).expect("its press went");
        assert_eq!(core_sends(all, release), b"\x1b[97;65:3u");
        // The lock turned off while `a` was held: its release has no bit.
        downs.pressed(VirtualKey(0x00), press);
        let unlocked = downs.up(&up, off).expect("its press went");
        assert_eq!(core_sends(all, unlocked), b"\x1b[97;1:3u");
        for (key, kind, held, flags, want) in [
            (
                K::LeftShift,
                Down,
                shift,
                lock,
                b"\x1b[57441;66u".as_slice(),
            ),
            (K::LeftShift, Up, none, lock, b"\x1b[57441;65:3u"),
            (K::RightShift, Down, shift, off, b"\x1b[57447;2u"),
            (K::CapsLock, Down, none, lock, b"\x1b[57358;65u"),
            (K::CapsLock, Up, none, off, b"\x1b[57358;1:3u"),
        ] {
            let e = modifier_event(Some(key), kind, held, flags).expect("a kitty key");
            assert_eq!(core_sends(all, e), want, "{key:?} {kind:?} {flags:?}");
        }
        // Both handlers hand the dispatched event's flags on.
        const SRC: &str = include_str!("app.rs");
        let body = |head: &str| {
            let b = SRC.split(head).nth(1).expect(head);
            b[..b.find("\n    }\n").expect("its end")].to_string()
        };
        let released = body(concat!(
            "    fn key_released(&mut self, ",
            "key: &KeyPress) {"
        ));
        assert!(released.contains(concat!("self.downs.up(key, ", "current_event_flags())")));
        let changed = body(concat!(
            "    fn modifiers_changed(&mut self, ",
            "change: &ModifierChange) {"
        ));
        assert!(changed.contains(concat!("let flags = ", "current_event_flags();")));
        assert!(changed.contains(concat!(
            "modifier_event(change.key, kind, change.modifiers, ",
            "flags)"
        )));
    }

    /// **The left click into an inactive window only activates it**: a
    /// left press while the window lacked the keyboard is the activating
    /// click, and its drag and release go with it, so neither a tracking
    /// program nor the selection hears a release without its press; the
    /// next press, with the keyboard, is a click like any other, and a
    /// right, middle or other press is one in the background too.
    /// Perturbations: judge the press by the focus after it and the
    /// activating click is ours; ignore the button and a right press into
    /// a background window is dropped.
    #[test]
    fn the_click_that_activates_the_window_does_nothing_else() {
        use ViewButton::{Left, Middle, Other, Right};
        use ViewMouseKind as V;
        use baud_core::Focus::{Gained, Lost};
        let down = click_of(Click::Ours, V::Down, Left, Lost);
        assert_eq!(down, Click::Activating);
        assert_eq!(click_of(down, V::Dragged, Left, Gained), Click::Activating);
        assert_eq!(click_of(down, V::Up, Left, Gained), Click::Activating);
        assert_eq!(click_of(Click::Ours, V::Down, Left, Gained), Click::Ours);
        assert_eq!(click_of(Click::Ours, V::Up, Left, Gained), Click::Ours);
        assert_eq!(click_of(down, V::Moved, Left, Gained), Click::Ours);
        // A right, middle or other press activates no window: it is the
        // program's and the terminal's, in the background as in front.
        for b in [Right, Middle, Other(3)] {
            assert_eq!(
                click_of(Click::Ours, V::Down, b, Lost),
                Click::Ours,
                "{b:?}"
            );
            assert_eq!(click_of(Click::Ours, V::Up, b, Lost), Click::Ours, "{b:?}");
            assert_eq!(click_of(down, V::Dragged, b, Gained), Click::Ours, "{b:?}");
            assert_eq!(click_of(down, V::Up, b, Gained), Click::Ours, "{b:?}");
        }
    }

    /// **Only the left button's press and release move the held click**:
    /// a right press and its release while the activating left press is
    /// held are the program's, and the left release that follows is still
    /// the activating click's; the next left press, with the keyboard, is
    /// ours. Perturbation: let any button's press set the held click and
    /// the left release is reported without its press.
    #[test]
    fn a_right_click_inside_the_activating_click_leaves_it_whole() {
        use ViewButton::{Left, Right};
        use ViewMouseKind as V;
        use baud_core::Focus::{Gained, Lost};
        let mut held = Click::Ours;
        let mut seen = Vec::new();
        for (kind, button, was) in [
            (V::Down, Left, Lost),
            (V::Down, Right, Gained),
            (V::Up, Right, Gained),
            (V::Dragged, Left, Gained),
            (V::Up, Left, Gained),
            (V::Down, Left, Gained),
        ] {
            let click = click_of(held, kind, button, was);
            held = click_held(held, kind, button, click);
            seen.push(click);
        }
        use Click::{Activating as A, Ours as O};
        assert_eq!(seen, [A, O, O, A, A, O]);
        assert_eq!(held, O);
    }

    /// The pump's caller is AppKit, so this holds the source: the mouse
    /// handler judges the click by the focus before it polls, and leaves
    /// before the caret, the locator or anything that reports a press.
    #[test]
    fn the_mouse_handler_asks_whose_click_before_anything_takes_it() {
        const SRC: &str = include_str!("app.rs");
        const HEAD: &str = concat!("    fn mouse(&mut self, ", "event: &Mouse) {");
        let body = SRC.split(HEAD).nth(1).expect("the mouse handler");
        let body = &body[..body.find("\n    }\n").expect("its end")];
        let at = |s: &str| body.find(s).unwrap_or_else(|| panic!("{s} in mouse"));
        let judged = at(concat!(
            "click_of(self.click, ",
            "event.kind, event.button, self.app.focus)"
        ));
        let polled = at(concat!("self.poll_", "focus();"));
        let left = at(concat!("if click == ", "Click::Activating {"));
        let caret = at(concat!("self.app.caret.", "quiet("));
        assert!(judged < polled, "judged by the focus before the click");
        assert!(
            polled < left && left < caret,
            "left before anything takes it"
        );
    }

    /// The source holds the release path apart from a keystroke's: a press
    /// is recorded where `typed` sends it, and a release or a modifier key
    /// goes through `sent_alone`, which neither stamps the latency
    /// instrument nor takes the kind `key` left for `typed`.
    #[test]
    fn a_release_is_sent_alone_and_a_press_recorded_where_it_is_typed() {
        const SRC: &str = include_str!("app.rs");
        let body = |head: &str| {
            let b = SRC.split(head).nth(1).expect(head);
            b[..b.find("\n    }\n").expect("its end")].to_string()
        };
        let typed = body(concat!(
            "    fn typed(&mut self, ",
            "e: KeyEvent, key: Option<VirtualKey>) {"
        ));
        assert!(typed.contains(concat!("self.downs.", "pressed(code, e);")));
        let released = body(concat!(
            "    fn key_released(&mut self, ",
            "key: &KeyPress) {"
        ));
        assert!(released.contains(concat!("self.downs.up(key, ", "current_event_flags())")));
        assert!(released.contains(concat!("self.sent_", "alone(e);")));
        assert!(!released.contains(concat!("self.", "typed(")));
        let alone = body(concat!("    fn sent_alone(&mut self, ", "e: KeyEvent) {"));
        assert!(!alone.contains("latency"), "a release is not a keystroke");
        let changed = body(concat!(
            "    fn modifiers_changed(&mut self, ",
            "change: &ModifierChange) {"
        ));
        assert!(changed.contains(concat!(
            "self.app.read_hover_as(",
            "link_chord(change.modifiers))"
        )));
        assert!(changed.contains(concat!("self.sent_", "alone(e);")));
    }

    /// **Command held without Shift is the link's chord**, read from the
    /// modifier change as from a key, so a press of Command alone lights a
    /// link under a resting pointer at once.
    #[test]
    fn command_without_shift_is_the_links_chord() {
        use crate::link::Chord;
        use macos_liquid_glass::drawable::Modifiers as M;
        let none = M::default();
        let command = M {
            command: true,
            ..none
        };
        assert_eq!(link_chord(command), Chord::Cmd);
        assert_eq!(
            link_chord(M {
                shift: true,
                ..command
            }),
            Chord::Other
        );
        assert_eq!(link_chord(none), Chord::Other);
    }

    /// **The File menu's New Window opens a window from the one it was
    /// asked of**: the app delegate is installed with the menu, before
    /// the run loop, and kept for the process; its hook asks the key or
    /// main window of baud's and wakes it, or opens one where a launch
    /// starts; the wake of the window asked opens it as its own Command-N
    /// does, before the pump. AppKit-bound, so this holds the source.
    /// Perturbation: drop the ask from the wake and the hook's window opens
    /// nothing.
    #[test]
    fn the_file_menus_new_window_opens_from_the_window_asked() {
        const SRC: &str = include_str!("app.rs");
        let menu = SRC
            .find(concat!(
                "menu::install_main_menu(",
                "&ns_app, mtm, \"baud\");"
            ))
            .expect("the menu");
        let delegate = SRC
            .find(concat!("app::AppDelegate::install(", "&ns_app, mtm)"))
            .expect("the delegate");
        let hook = SRC
            .find(concat!("delegate.on_new_window(", "new_window_from_menu);"))
            .expect("the hook");
        let kept = SRC
            .find(concat!("core::mem::forget(", "delegate);"))
            .expect("kept for the process");
        assert!(menu < delegate && delegate < hook && hook < kept);
        assert!(
            SRC[kept..].contains(concat!("ns_app.", "run();")),
            "before the run loop"
        );
        let from_menu = SRC
            .split(concat!("\nfn new_window_from_", "menu() {"))
            .nth(1)
            .expect("the hook's body");
        let from_menu = &from_menu[..from_menu.find("\n}\n").expect("its end")];
        assert!(from_menu.contains(concat!("a.ask(", "id)")));
        assert!(from_menu.contains(concat!("waker.", "wake();")));
        assert!(from_menu.contains(concat!("open_window(", "None)")));
        let wake = SRC
            .split(concat!(
                "    fn wake(&mut self) {\n        startup_first(",
                "\"run loop\""
            ))
            .nth(1)
            .expect("the driver's wake");
        let wake = &wake[..wake.find("\n    }\n").expect("its end")];
        let taken = wake
            .find(concat!("ASKED.with(|a| a.take(", "self.window_id))"))
            .expect("the ask taken");
        let opened = wake
            .find(concat!("self.new_", "window();"))
            .expect("opened");
        let pumped = wake
            .find(concat!("self.pump(", "Trigger::Wake);"))
            .expect("pumped");
        assert!(taken < opened && opened < pumped);
    }

    /// The monitor that hands AppKit's kept key-ups to the view is
    /// installed once the responder is, so the first such key-up has a
    /// responder to reach, and is kept with its window's entry in the open
    /// windows, so the window's close removes it.
    #[test]
    fn the_command_key_up_monitor_follows_the_responder() {
        const SRC: &str = include_str!("app.rs");
        let set = SRC
            .find(concat!("view.set_responder(", "Box::new(driver));"))
            .expect("the responder installed");
        let monitor = SRC
            .find(concat!("keyup::forward_command_", "key_ups("))
            .expect("the monitor installed");
        let kept = SRC
            .find(concat!("open._keyups = ", "Some(keyups);"))
            .expect("the monitor kept with the window");
        assert!(set < monitor && monitor < kept);
    }

    /// **While the input manager holds a composition, its keys are its**:
    /// Return commits, Delete edits, Escape cancels and the arrows move
    /// through it, and none of them may reach the child first. With no
    /// composition the same keys are the child's, and a Command chord is the
    /// menu's either way.
    #[test]
    fn a_held_composition_takes_every_key_but_the_menus() {
        use macos_liquid_glass::drawable::Modifiers;
        let key = |code, modifiers| KeyPress {
            key_code: code,
            characters: String::new(),
            unmodified: String::new(),
            modifiers,
            repeat: false,
        };
        let none = Modifiers::default();
        let option = Modifiers {
            option: true,
            ..none
        };
        let command = Modifiers {
            command: true,
            ..none
        };
        let kana: Vec<Codepoint> = "にほ"
            .chars()
            .map(|c| Codepoint::new(u32::from(c)).unwrap())
            .collect();
        // Return, Delete, Escape, Tab, the four arrows, a letter, an Option
        // chord.
        for code in [0x24, 0x33, 0x35, 0x30, 0x7B, 0x7C, 0x7D, 0x7E, 0x00] {
            assert!(composing_first(&key(code, none), &kana), "{code:#x}");
            assert!(
                composing_first(&key(code, option), &kana),
                "{code:#x} + option"
            );
            assert!(
                !composing_first(&key(code, none), &[]),
                "{code:#x} uncomposed"
            );
            assert!(
                !composing_first(&key(code, command), &kana),
                "{code:#x} + command"
            );
        }
    }

    /// The pump's caller is AppKit, so this holds the source: `key` asks
    /// `composing_first` before `special` names a key for the child.
    #[test]
    fn key_asks_the_composition_before_naming_a_special_key() {
        const SRC: &str = include_str!("app.rs");
        const KEY: &str = concat!("    fn key(&mut self, ", "key: &KeyPress) -> Handled {");
        let body = SRC.split(KEY).nth(1).expect("the key handler");
        let body = &body[..body.find("\n    }\n").expect("its end")];
        let ask = body
            .find(concat!("composing_first(key, ", "&self.app.composing)"))
            .expect("key asks composing_first");
        let special = body
            .find(concat!("special(", "key)"))
            .expect("key names special keys");
        assert!(ask < special, "the composition is asked first");
    }

    /// A changed composition is owed a frame, and the pump draws an owed
    /// one now: `marked_text` marks it `Owed` and `pump`'s `due` reads it.
    /// Without either, a dead key's accent waits for the next tick or read.
    #[test]
    fn a_changed_composition_makes_a_frame_due() {
        const SRC: &str = include_str!("app.rs");
        let body = |head: &str| {
            let b = SRC.split(head).nth(1).expect("the function");
            b[..b.find("\n    }\n").expect("its end")].to_string()
        };
        let marked = body(concat!("    fn marked_text(&mut self, ", "text: &str) {"));
        assert!(
            marked.contains(concat!("self.app.marked = ", "Marked::Owed;")),
            "marked_text owes a frame"
        );
        let pump = body(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"));
        assert!(
            pump.contains(concat!("marked: self.app.", "marked,")),
            "the pump hands the gate the composition"
        );
        let gate = SRC
            .split(concat!("fn frame_gate(g: ", "FrameGate) -> Decided {"))
            .nth(1)
            .expect("the gate");
        let gate = &gate[..gate.find("\n}\n").expect("its end")];
        let due = &gate[gate.find("let due = ").expect("the gate's due")..];
        let due = &due[..due.find(';').expect("its end")];
        assert!(
            due.contains(concat!("g.marked == ", "Marked::Owed")),
            "an owed composition makes a frame due"
        );
    }

    #[test]
    fn option_starts_a_block_and_wins_over_shift() {
        use macos_liquid_glass::drawable::Modifiers;
        let held = |shift, option| Modifiers {
            shift,
            option,
            ..Modifiers::default()
        };
        assert_eq!(press_of(held(false, false)), Press::Stream);
        assert_eq!(press_of(held(true, false)), Press::Extend);
        assert_eq!(press_of(held(false, true)), Press::Block);
        // Under mouse tracking Shift is the only way into a selection, so
        // Shift+Option is how a block starts there.
        assert_eq!(press_of(held(true, true)), Press::Block);
    }

    /// **A legacy scroller's gutter comes out of the grid's columns, and the
    /// frame fits the view**: the grid and the gutter beside it inside the
    /// padding, where the overlay style, lying over the grid, gives the view
    /// its whole width in columns. The rows are the same either way.
    #[test]
    fn a_legacy_gutter_takes_its_columns_and_the_frame_fits() {
        let (view, pad, cell) = ((1000.0, 600.0), 4.0, (10.0, 20.0));
        let overlay = grid_in(view, pad, 0.0, cell);
        let legacy = grid_in(view, pad, 34.0, cell);
        assert_eq!((overlay.ws_col, overlay.ws_row), (99, 29));
        assert_eq!((legacy.ws_col, legacy.ws_row), (95, 29));
        let frame = f64::from(legacy.ws_col) * cell.0 + 34.0;
        assert!(
            frame + 2.0 * pad <= view.0,
            "a frame {frame} px wide overflows"
        );
    }

    /// **The window the status line grows to lays out to the grid's rows
    /// and the line's**: `content_height` with the line inverted through
    /// `grid_in` and `less_status`, at cell heights across 12-40 px, three
    /// scales and three paddings, gives the grid back its rows, so the
    /// resize after the line appears changes nothing a program was told; a
    /// window that cannot grow gives one row up.
    #[test]
    fn the_status_lines_window_lays_out_to_the_grids_rows() {
        let one = Rows::of_count(1);
        let mut checked = 0;
        for rows in [24u16, 50] {
            for scale in [1.0, 2.0, 3.0] {
                for pad in [0.0, 4.0, 7.5] {
                    let mut ch = 12.0;
                    while ch < 40.0 {
                        let grid = crate::width::Grid { cols: 80, rows };
                        let h = crate::width::content_height(grid, one, ch, scale, pad);
                        let g = grid_in((1200.0, h * scale), pad * scale, 0.0, (10.0, ch));
                        let main = crate::width::less_status(g, one);
                        assert_eq!(main.ws_row, rows, "{rows} at {ch} px, {scale}x, {pad}");
                        // The window as it was, the line shown: a row gone.
                        let was = crate::width::content_height(grid, Rows::ZERO, ch, scale, pad);
                        let g = grid_in((1200.0, was * scale), pad * scale, 0.0, (10.0, ch));
                        assert_eq!(crate::width::less_status(g, one).ws_row, rows - 1);
                        checked += 1;
                        ch += 0.61;
                    }
                }
            }
        }
        assert_eq!(checked, 828, "every case ran");
    }

    /// **The window a program's page width asks for lays out to exactly
    /// its columns**: `content_width` inverted through `grid_in`, at cell
    /// widths across 6-20 px, three scales, three paddings and both
    /// gutters, so the resize callback that follows `widen_later` finds the
    /// grid the core already has and changes nothing. The control: without
    /// the half pixel, some of these land one short.
    #[test]
    fn a_program_width_lays_out_to_its_columns() {
        let mut short = 0;
        let mut checked = 0;
        for cols in [80u16, 132] {
            for scale in [1.0, 2.0, 3.0] {
                for pad in [0.0, 4.0, 7.5] {
                    for gutter in [0.0, 34.0] {
                        let mut cw = 6.0;
                        while cw < 20.0 {
                            let grid = crate::width::Grid { cols, rows: 30 };
                            let w = crate::width::content_width(grid, cw, scale, pad, gutter);
                            let g = grid_in((w * scale, 600.0), pad * scale, gutter, (cw, 20.0));
                            assert_eq!(
                                g.ws_col, cols,
                                "{cols} at {cw} px, {scale}x, {pad}, {gutter}"
                            );
                            let bare = (f64::from(cols) * cw + gutter) / scale + 2.0 * pad;
                            let g = grid_in((bare * scale, 600.0), pad * scale, gutter, (cw, 20.0));
                            short += usize::from(g.ws_col != cols);
                            checked += 1;
                            cw += 0.37;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 1368, "every case ran");
        assert!(short > 0, "the control: the bare inverse never fell short");
    }

    /// **`honour` resizes the grid inside the read that asked, keeping its
    /// rows**, so the mark drawn after the request lands at column 132, the
    /// pump's `after` composition run against a real core. Pinned, the grid
    /// stays and the mark is clamped to the old last column.
    #[test]
    fn honour_resizes_inside_the_read_and_keeps_the_rows() {
        use crate::width::{Grid, Host, Widths};
        let small = WinSize {
            ws_row: 30,
            ws_col: 100,
        };
        let room = WinSize {
            ws_row: 30,
            ws_col: 132,
        };
        let read: &[u8] = b"\x1b[?40h\x1b[?3h\x1b[1;132HX";
        for (host, want, col) in [
            (
                Host::Window,
                Some(Grid {
                    cols: 132,
                    rows: 30,
                }),
                131,
            ),
            (Host::Pinned, None, 99),
        ] {
            let mut s = baud_core::Owned::new(&room);
            let mut t = Term::new(s.storage(), &small);
            t.set_width_hold(baud_core::WidthHold::On);
            let mut widths = Widths::new(&t);
            let mut resized = None;
            crate::reads::feed(
                &mut t,
                [read],
                |_| {},
                |t| {
                    resized = honour(widths.poll(t, host), t).or(resized);
                },
                &mut crate::hostlog::NoLog,
            );
            assert_eq!(resized, want, "{host:?}");
            let row = t.row(baud_core::Row::ZERO).expect("row 0");
            assert_eq!(
                row[col].content.as_codepoint().map(|c| c.value()),
                Some(u32::from(b'X')),
                "{host:?}"
            );
        }
    }

    /// **A program's page width sends the window over the history live, at
    /// the request** (`honour`): the program is redrawing the page. The
    /// core's resize holds a window on its line, which the control shows:
    /// pinned, the grid stays and the window with it.
    #[test]
    fn honour_sends_the_window_over_the_history_live() {
        use crate::width::{Host, Widths};
        let small = WinSize {
            ws_row: 30,
            ws_col: 100,
        };
        let room = WinSize {
            ws_row: 30,
            ws_col: 132,
        };
        let lines: Vec<u8> = (0..60)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        for (host, want) in [(Host::Window, 0), (Host::Pinned, 5)] {
            let mut s = baud_core::Owned::new(&room);
            let mut t = Term::new(s.storage(), &small);
            t.set_width_hold(baud_core::WidthHold::On);
            crate::reads::feed(
                &mut t,
                [&lines[..]],
                |_| {},
                |_| {},
                &mut crate::hostlog::NoLog,
            );
            t.set_window_back(Rows::of_count(5));
            assert_eq!(t.window_back().index(), 5, "the control: five up");
            let mut widths = Widths::new(&t);
            crate::reads::feed(
                &mut t,
                [&b"\x1b[?40h\x1b[?3h"[..]],
                |_| {},
                |t| {
                    let _ = honour(widths.poll(t, host), t);
                },
                &mut crate::hostlog::NoLog,
            );
            assert_eq!(t.window_back().index(), want, "{host:?}");
        }
    }

    /// **A program's narrowing that reflows starts no slide.** A held
    /// DECSCPP 80 after a screen of 120-column lines re-cuts them into two
    /// rows each, retiring rows no output scrolled: `live_moved` reads the
    /// retire (the control), and `read_moved`, given the resize, reads none.
    #[test]
    fn a_program_width_change_moves_nothing_for_the_slide() {
        use crate::width::{Grid, Host, Widths};
        let ws = WinSize {
            ws_row: 10,
            ws_col: 132,
        };
        let mut s = baud_core::Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_width_hold(baud_core::WidthHold::On);
        let mut read: Vec<u8> = Vec::new();
        for i in 0..9 {
            read.extend_from_slice(format!("{i}{}\r\n", "x".repeat(119)).as_bytes());
        }
        read.extend_from_slice(b"\x1b[80$|");
        let before = Before::watched(&mut t, None, Scrolled::START);
        let mut widths = Widths::new(&t);
        let mut resized = None;
        crate::reads::feed(
            &mut t,
            [read.as_slice()],
            |_| {},
            |t| {
                resized = honour(widths.poll(t, Host::Window), t).or(resized);
            },
            &mut crate::hostlog::NoLog,
        );
        assert_eq!(resized, Some(Grid { cols: 80, rows: 10 }));
        let Ring::Retired(n) = live_moved(&t, before) else {
            panic!("the control: the reflow retired, not restored")
        };
        assert!(n.index() > 0, "the control: the reflow retired rows");
        assert_eq!(read_moved(&t, before, resized), Ring::Retired(Rows::ZERO));
        assert_eq!(
            read_moved(&t, before, None),
            live_moved(&t, before),
            "unresized, the read's own"
        );
    }

    /// **The window is resized after the callback that asked, and only
    /// there.** The pump is AppKit-bound and no test drives it, so this
    /// holds the source: `setContentSize` is called twice, at launch before
    /// any responder exists and inside `widen_later`'s main-queue block; the
    /// pump's one feed polls the widths in its per-read callback and calls
    /// `program_resized` after it; and `program_resized` is what calls
    /// `widen_later`.
    #[test]
    fn the_window_is_resized_after_the_callback_that_asked() {
        // The file less its own test module, which code follows.
        let src = include_str!("app.rs");
        let (before, rest) = src
            .split_once("#[cfg(test)]\nmod tests {")
            .expect("the test module");
        let code = format!(
            "{before}{}",
            rest.split_once("\n}\n").map_or("", |(_, a)| a)
        );
        let call = concat!("setContent", "Size(");
        assert_eq!(code.matches(call).count(), 2, "launch and widen_later");
        let build = code
            .split_once(concat!("    fn new(\n        mtm: ", "MainThreadMarker,"))
            .expect("the window's build")
            .1;
        let build = &build[..build.find("\n    }\n").expect("the build ends")];
        assert!(build.contains(call), "the launch sizing");
        let widen = code
            .split_once(concat!(
                "fn widen_",
                "later(window: &NSWindow, width: CGFloat) {"
            ))
            .expect("widen_later")
            .1;
        let widen = &widen[..widen.find("\n}\n").expect("its end")];
        let block = widen
            .split_once(concat!(".exec_", "async(move || {"))
            .expect("a main-queue block")
            .1;
        assert!(block.contains(call), "the resize is inside the block");
        let pump = code
            .split_once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"))
            .expect("the pump")
            .1;
        let pump = &pump[..pump.find("\n    }\n").expect("the pump ends")];
        let feed = pump
            .split_once(concat!("crate::reads::", "paced("))
            .expect("the feed")
            .1;
        let (args, after) = feed.split_once("\n        );").expect("the call ends");
        assert!(
            args.contains(concat!("honour(widths.", "poll(t, host), t)")),
            "the widths are polled at each read"
        );
        let follow = after.find("self.app.pty.follow(").expect("the follow");
        let rest = after
            .find(concat!(
                "if let Some(ws) = resized {\n            self.program_",
                "resized(ws);"
            ))
            .expect("the rest of the app follows the call");
        assert!(follow < rest, "after the follow, which comes first");
        let body = code
            .split_once(concat!("fn program_", "resized(&mut self"))
            .expect("program_resized")
            .1;
        let body = &body[..body.find("\n    }\n").expect("its end")];
        assert!(body.contains(concat!("widen_", "later(")));
    }

    /// **A selection's point over a cell the display draws from the row
    /// above the window names that row**: "Y" ten times, cut after its
    /// sixth letter at the window's top, is drawn whole on the first line,
    /// and a point over its third cell names the stored row one line up,
    /// column 15; a mouse report's point names the window's first row.
    #[test]
    fn a_selection_point_names_the_line_a_cell_is_drawn_from() {
        use baud_core::{Owned, Term};
        use baud_domain::{BytePos, ReadLen, Rows, WinSize};
        use baud_render::WindowLine;
        let ws = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let bytes = format!(
            "h0\r\nh1\r\n{} {} zz\r\nf1\r\nf2\r\nf3\r\nf4\r\n\x1b]133;A\x07$ ",
            "X".repeat(13),
            "Y".repeat(10)
        );
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let mut order = baud_render::Reorder::new();
        order.lay_out(&t, Rows::ZERO);
        // 1x scale, 10x10 px cells: point (25, 5) is cell (0, 2).
        let g: PointerGeometry = (1.0, (10.0, 10.0), (20, 6), (0, 0));
        assert_eq!(
            line_under(order.orders(), (25.0, 5.0), g),
            (
                WindowLine::of(Row::ZERO).up(Rows::of_count(1)),
                Col::at_index(16)
            )
        );
        assert_eq!(
            order.orders().pointer(Row::ZERO, Col::at_index(2)).0,
            Row::ZERO,
            "a report names a window row"
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

    /// **The grid begins after the exit-status strip**: the whole frame is
    /// centred, and a point over the strip names the first column, as one
    /// over the padding does; a point one strip in from the frame's left
    /// edge is the first pixel of the first cell.
    #[test]
    fn the_grid_begins_after_the_exit_status_strip() {
        let (cell, grid, scale) = ((20.0, 40.0), (10, 5), 2.0);
        let view = (Px::of(260), Px::of(220));
        let whole = (Px::of(220), Px::of(200));
        assert_eq!(
            grid_origin(view, whole, Px::of(0), Px::of(0)),
            (Px::of(20), Px::of(10))
        );
        let pad = grid_origin(view, whole, Px::of(20), Px::of(0));
        assert_eq!(pad, (Px::of(40), Px::of(10)));
        let pad = (pad.0.value(), pad.1.value());
        // At 2x: the strip's middle, (30, 10) px, is over it.
        let (r, c, px) = pointer_under((15.0, 5.0), scale, cell, grid, pad);
        assert_eq!((r.index(), c.index(), px.x), (0, 0, ImagePx::ZERO));
        // (40, 10) px, the strip's far edge, is the first cell's first pixel,
        // and (61, 10) the second cell's second.
        let (_, c, px) = pointer_under((20.0, 5.0), scale, cell, grid, pad);
        assert_eq!((c.index(), px.x), (0, ImagePx::ZERO));
        let (_, c, px) = pointer_under((30.5, 5.0), scale, cell, grid, pad);
        assert_eq!((c.index(), px.x), (1, ImagePx::of(21)));
    }

    /// **The word wrap's marks take the padding and no column**: across
    /// widths, scales and paddings, the grid a view holds is what it was
    /// (`grid_in` never reads the band), the band is at most the padding,
    /// the whole frame with a band each side still fits the view, and the
    /// grid begins where it began with none. A grid pinned wider than the
    /// view leaves the band only what the letterbox has; a view with no
    /// padding lends none.
    #[test]
    fn the_wrap_marks_take_the_padding_and_no_column() {
        let cell = (17.0, 34.0);
        for scale in [1.0, 2.0, 3.0] {
            for padding in [0.0, 4.0, 8.0, 12.5] {
                for view_w in (300..=900).step_by(37) {
                    let ws = grid_in((f64::from(view_w), 400.0), padding * scale, 0.0, cell);
                    let frame = u32::from(ws.ws_col) * 17;
                    let view = (Px::of(view_w), Px::of(400));
                    let b = band_of(view.0, Px::of(frame), padding, scale);
                    assert!(f64::from(b.value()) <= padding * scale);
                    assert!(
                        frame + 2 * b.value() <= view_w,
                        "{view_w} {padding} {scale}"
                    );
                    let whole = Px::of(frame + 2 * b.value());
                    assert_eq!(
                        grid_origin(view, (whole, Px::of(340)), b, Px::of(0)),
                        grid_origin(view, (Px::of(frame), Px::of(340)), Px::of(0), Px::of(0)),
                        "{view_w} {padding} {scale}: the grid where it was"
                    );
                }
            }
        }
        assert_eq!(
            band_of(Px::of(100), Px::of(96), 8.0, 2.0),
            Px::of(2),
            "pinned"
        );
        assert_eq!(
            band_of(Px::of(400), Px::of(300), 0.0, 2.0),
            Px::of(0),
            "none"
        );
    }

    /// **The grid begins below the lights' line at the top, and is centred
    /// above one at the bottom**: the whole frame, the line included, is
    /// centred, and with the line on top the grid's first row is the line's
    /// height further down; a point over the line names the first row, as
    /// one over the padding does. Perturbation: leave `top` out of
    /// `grid_origin` -- every press under a top line lands the line's
    /// height too high -- and this goes red.
    #[test]
    fn the_grid_begins_below_the_lights_line_at_the_top() {
        let (cell, grid, scale) = ((20.0, 40.0), (10, 5), 2.0);
        let view = (Px::of(260), Px::of(260));
        // A 200-pixel grid and a 30-pixel line: the whole is 230 tall,
        // 15 down in the view.
        let whole = (Px::of(220), Px::of(230));
        assert_eq!(
            grid_origin(view, whole, Px::of(0), Px::of(0)),
            (Px::of(20), Px::of(15)),
            "the line below: the grid at the whole's top"
        );
        let pad = grid_origin(view, whole, Px::of(0), Px::of(30));
        assert_eq!(pad, (Px::of(20), Px::of(45)), "the line on top");
        let pad = (pad.0.value(), pad.1.value());
        // (x, 45) px is the text area's first pixel, (x, 85) the second
        // row's first, 40 pixels down it.
        let (r, _, px) = pointer_under((15.0, 22.5), scale, cell, grid, pad);
        assert_eq!((r.index(), px.y), (0, ImagePx::ZERO));
        let (r, _, px) = pointer_under((15.0, 42.5), scale, cell, grid, pad);
        assert_eq!((r.index(), px.y), (1, ImagePx::of(40)));
        // Over the line, the first row's edge.
        let (r, _, px) = pointer_under((15.0, 10.0), scale, cell, grid, pad);
        assert_eq!((r.index(), px.y), (0, ImagePx::ZERO));
    }

    /// **The pixel `?1016` reports is in the cell the other encodings
    /// report.** Device pixels less the padding, zero-based, clamped to the
    /// text area: an application that divides it by the `CSI 16 t` cell size
    /// gets the cell back, at every point of a sweep across the padding, the
    /// grid and past its far edge.
    #[test]
    fn a_point_names_its_pixel_and_the_cell_that_holds_it() {
        let (scale, cell, grid, pad) = (2.0, (20.0, 40.0), (200, 50), (10, 6));
        // (105, 21) points at 2x is (210, 42); less the padding (200, 36).
        let (r, c, px) = pointer_under((105.0, 21.0), scale, cell, grid, pad);
        assert_eq!((px.x, px.y), (ImagePx::of(200), ImagePx::of(36)));
        assert_eq!((r.index(), c.index()), (0, 10));
        // Over the padding: the first pixel of the first cell.
        let (r, c, px) = pointer_under((1.0, 1.0), scale, cell, grid, pad);
        assert_eq!(
            (px.x, px.y, r.index(), c.index()),
            (ImagePx::ZERO, ImagePx::ZERO, 0, 0)
        );
        // Past the far edge: the last pixel of the last cell.
        let (r, c, px) = pointer_under((9999.0, 9999.0), scale, cell, grid, pad);
        assert_eq!((px.x, px.y), (ImagePx::of(3999), ImagePx::of(1999)));
        assert_eq!((r.index(), c.index()), (49, 199));
        // And everywhere between, the pixel divided by the cell is the cell.
        for i in 0..4200 {
            let p = f64::from(i) * 0.6;
            let (r, c, px) = pointer_under((p, p * 0.3), scale, cell, grid, pad);
            assert_eq!(px.x.index() / 20, c.index(), "x at {p}");
            assert_eq!(px.y.index() / 40, r.index(), "y at {p}");
            assert_eq!((r, c), cell_under((p, p * 0.3), scale, cell, grid, pad));
        }
    }

    /// **XTWINOPS's facts from AppKit's geometry**: a 2x window whose frame
    /// sits 100 pt from the primary screen's left and whose top is 50 pt
    /// below its top, the view 28 pt under the frame's top, the grid 6 and 4
    /// px into the view. Positions come out top-left with y down, in device
    /// pixels; a window on a screen left of the primary one is at a
    /// negative x; and with no primary screen no position is guessed.
    #[test]
    fn the_window_facts_are_appkits_geometry_in_device_pixels_from_the_top() {
        use baud_core::{PixelSize, ScreenCoord, ScreenPoint, WindowState};
        let rect = |x, y, w, h| NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
        let primary = rect(0.0, 0.0, 1512.0, 982.0);
        let mut placed = Placed {
            minimized: Miniaturized::No,
            frame: rect(100.0, 332.0, 960.0, 600.0),
            view: rect(100.0, 332.0, 960.0, 572.0),
            pad: (6, 4),
            screen: Some(primary),
            primary: Some(primary),
            scale: 2.0,
        };
        let f = window_facts(&placed);
        let at = |x, y| ScreenPoint {
            x: ScreenCoord::of(x),
            y: ScreenCoord::of(y),
        };
        assert_eq!(f.state, Some(WindowState::Open));
        assert_eq!(f.position, Some(at(200, 100)));
        assert_eq!(f.text_area, Some(at(206, 160)), "28 pt titlebar, 4 px pad");
        assert_eq!(
            f.window,
            Some(PixelSize {
                width: ImagePx::of(1920),
                height: ImagePx::of(1200)
            })
        );
        assert_eq!(
            f.screen,
            Some(PixelSize {
                width: ImagePx::of(3024),
                height: ImagePx::of(1964)
            })
        );
        placed.frame.origin.x = -400.0;
        placed.minimized = Miniaturized::Yes;
        let f = window_facts(&placed);
        assert_eq!(f.position, Some(at(-800, 100)));
        assert_eq!(f.state, Some(WindowState::Iconified));
        placed.primary = None;
        placed.screen = None;
        let f = window_facts(&placed);
        assert_eq!((f.position, f.text_area, f.screen), (None, None, None));
        assert!(f.window.is_some(), "the frame is still the window's");
    }

    /// **What `CSI 13 t` reports, `CSI 3 t` moves to**: the position
    /// `window_facts` gives a frame, on the primary screen and on one left
    /// of it, is the frame `winops::mac` puts the window at for that move,
    /// so a program that saves the position and sends it back puts the
    /// window where it was.
    #[test]
    fn the_reported_position_moved_to_is_where_the_window_was() {
        use crate::winops::{Full, MacAction, MacWindow, Minimized, Rect, Saved};
        let rect = |x, y, w, h| NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
        let primary = rect(0.0, 0.0, 1512.0, 982.0);
        for x in [100.0, -1400.0] {
            let frame = rect(x, 332.0, 960.0, 600.0);
            let placed = Placed {
                minimized: Miniaturized::No,
                frame,
                view: rect(x, 332.0, 960.0, 572.0),
                pad: (6, 4),
                screen: Some(primary),
                primary: Some(primary),
                scale: 2.0,
            };
            let reported = window_facts(&placed).position.expect("a position");
            let at = Rect {
                x: frame.origin.x,
                y: frame.origin.y,
                w: frame.size.width,
                h: frame.size.height,
            };
            let w = MacWindow {
                frame: Rect {
                    x: 0.0,
                    y: 0.0,
                    ..at
                },
                content: (960.0, 572.0),
                primary_top: Some(982.0),
                visible: None,
                scale: 2.0,
                cell: (16.0, 34.0),
                padding: 8.0,
                gutter: 0.0,
                grid: crate::width::Grid { cols: 80, rows: 24 },
                status: Rows::ZERO,
                line: 0.0,
                minimized: Minimized::No,
                full: Full::No,
                saved: None,
                host: crate::width::Host::Window,
            };
            assert_eq!(
                crate::winops::mac(baud_core::WindowRequest::Move(reported), &w),
                MacAction::Frame(at, Saved::Keep),
                "from x = {x}"
            );
        }
    }

    /// **Every window operation's AppKit call runs on the main queue, after
    /// the callback that asked.** No test opens a window, so the source is
    /// held: `window_requests` hands every call but the redraw to
    /// `window_later`, and each of the six calls is made inside its
    /// main-queue block and nowhere else in the driver.
    #[test]
    fn the_window_operations_run_after_the_callback_that_asked() {
        // The file less every test module, whose strings name the calls.
        let mut code = include_str!("app.rs").to_string();
        while let Some(at) = code.find("#[cfg(test)]\nmod ") {
            let end = code[at..].find("\n}\n").map_or(code.len(), |e| at + e + 3);
            code.replace_range(at..end, "");
        }
        let body = code
            .split_once(concat!(
                "fn window_",
                "later(window: &NSWindow, action: crate::winops::MacAction) {"
            ))
            .expect("window_later")
            .1;
        let body = &body[..body.find("\n}\n").expect("its end")];
        let block = body
            .split_once(concat!(".exec_", "async(move || {"))
            .expect("a main-queue block")
            .1;
        // Spelled apart, so the bell's count of its raise does not find
        // this test's.
        for call in [
            "w.deminiaturize(None)",
            "w.miniaturize(None)",
            concat!("w.orderFront", "Regardless()"),
            "w.orderBack(None)",
            "w.toggleFullScreen(None)",
            "w.setFrame_display(",
        ] {
            assert!(block.contains(call), "{call} in the block");
            let name = call.split_once('(').expect("a call").0;
            let name = name.trim_start_matches("w.");
            let elsewhere = code.matches(&format!(".{name}(")).count();
            let want = if name == "orderFrontRegardless" { 2 } else { 1 };
            assert_eq!(elsewhere, want, "{name} called outside the block");
        }
        let request = code
            .split_once(concat!(
                "fn window_",
                "requests(\n        &mut self,\n        asked: &[baud_core::WindowRequest],\n"
            ))
            .expect("window_requests")
            .1;
        let request = &request[..request.find("\n    }\n").expect("its end")];
        assert_eq!(request.matches("window_later(&w, action)").count(), 1);
    }

    /// **`BAUD_LOG` governs the trace lines.** They were bare `eprintln!`s,
    /// so `trace: off …` printed on every launch and `BAUD_LOG=err` did not
    /// silence it (note AI, section 8). The off and writing lines are
    /// startup diagnostics, at `info` beside `renderer:`; a `BAUD_TRACE`
    /// that could not be acted on is a `warn`, as `BAUD_GPU`'s is. Read from
    /// the source, because the macros write to stderr, where a test cannot
    /// look; the level each macro carries is the log module's own, asked
    /// through `log::shows`.
    #[test]
    fn baud_log_governs_the_trace_lines() {
        use crate::log::{Level, named, shows};
        // The file less its own test module, which holds these literals
        // too; code follows the module, so both sides are kept.
        let src = include_str!("app.rs");
        let (before, rest) = src
            .split_once("#[cfg(test)]\nmod tests {")
            .expect("the test module");
        let after = rest.split_once("\n}\n").map_or("", |(_, a)| a);
        let code = format!("{before}{after}");
        // The macro a literal is the format string of: the nearest `!(`
        // before it, and the path that names it. `None` is a bare
        // `eprintln!`, which no threshold silences.
        let level = |text: &str| -> Option<Level> {
            assert_eq!(code.matches(text).count(), 1, "one site for {text}");
            let at = code.find(text).expect(text);
            let open = code[..at].rfind("!(").expect("a macro");
            let name = code[..open]
                .rsplit(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .next();
            match name {
                Some("crate::err") => Some(Level::Err),
                Some("crate::warn") => Some(Level::Warn),
                Some("crate::info") => Some(Level::Info),
                Some("crate::debug") => Some(Level::Debug),
                Some("crate::verbose") => Some(Level::Verbose),
                Some("eprintln") => None,
                other => panic!("{text} is in {other:?}"),
            }
        };
        let shown = |text: &str, threshold: Level| level(text).is_none_or(|l| shows(l, threshold));
        // The control: a line known to be `info` and one known to be
        // `warn` read as such, so a matcher that resolves nothing cannot
        // pass.
        assert_eq!(level("\"renderer: {wanted:?}{}\""), Some(Level::Info));
        assert_eq!(
            level("\"BAUD_GPU={v:?} is not 0, 1 or cells; choosing automatically\""),
            Some(Level::Warn)
        );
        let off = "\"trace: off (set BAUD_TRACE=<prefix> to record the session)\"";
        let writing = "\"trace: writing {}.raw and .steps\"";
        let failed = "\"trace: BAUD_TRACE={} but the files could not be created\"";
        let (quiet, at_default, info) = (named(Some("err")), named(None), named(Some("info")));
        assert!(!shown(off, at_default), "trace: off is silent by default");
        assert!(!shown(off, quiet), "and under BAUD_LOG=err");
        assert!(shown(off, info), "and said under BAUD_LOG=info");
        assert!(!shown(writing, at_default), "writing is a diagnostic");
        assert!(shown(writing, info), "said under BAUD_LOG=info");
        assert!(shown(failed, at_default), "a failed BAUD_TRACE is said");
        assert!(!shown(failed, quiet), "unless BAUD_LOG=err");
    }
}

/// Which path draws a frame's smooth-scroll slide: only the cell path pans
/// the grid, so only there does a trail ride with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SlideDrawn {
    Cells,
    Other,
}

impl SlideDrawn {
    /// From the presenter's mode and the cell path's memory, if it was built.
    fn of<C>(mode: Option<Mode>, cell: Option<&C>) -> SlideDrawn {
        if mode == Some(Mode::Gpu) && cell.is_some() {
            SlideDrawn::Cells
        } else {
            SlideDrawn::Other
        }
    }
}

/// The rows below the grid that are no line of the ring: the status
/// line's and the config's notice's.
fn below_grid(term: &Term<'_>, renderer: &Renderer<'_, '_>) -> Rows {
    Rows::of_count(term.status_rows().index() + renderer.notice_rows().index())
}

/// **Whether the frame draws a smooth scroll's slide**: smooth scrolling,
/// the window at the live screen, the primary -- the alternate screen keeps
/// no ring and has no leaving row -- the cell path, and no status line,
/// whose row below the grid is no line of the ring and would pan with it.
/// The predicate the trail's lift takes; its ride takes every whole-screen
/// scroll (`trail_rides`).
fn slides(
    scroll: crate::config::ScrollMode,
    window: Rows,
    screen: Screen,
    drawn: SlideDrawn,
    status: Rows,
) -> bool {
    scroll == crate::config::ScrollMode::Smooth
        && window.index() == 0
        && screen == Screen::Normal
        && drawn == SlideDrawn::Cells
        && status == Rows::ZERO
}

/// The lift a trail is drawn with: the slide's pan when it rides, and
/// nothing when it stays, which is what it drew before the ride was built.
fn trail_lift(scroll: crate::config::TrailScroll, pan: Px) -> Px {
    match scroll {
        crate::config::TrailScroll::Ride => pan,
        crate::config::TrailScroll::Stay => Px::of(0),
    }
}

/// **The ghost rides a scroll** `dy` pixels, y down, at progress `p`, by
/// the rule both drivers take ([`crate::trail::ghost_ride`]): moved by
/// exactly `dy` while it rides, rigid then finishing its sweep past two
/// rows, and `None` once it has landed or lies wholly above the row that
/// leaves the window.
fn ride_ghost(st: TrailState, dy: f32, p: f32, cell_h: f32) -> Option<TrailState> {
    let (origin_dy, ride) =
        crate::trail::ghost_ride((st.from.0, st.to.0), (st.origin_dy, st.ride), dy, p, cell_h)?;
    Some(TrailState {
        origin_dy,
        ride,
        ..st
    })
}

/// Whether a one-time thing has happened yet: the first tick's read of the
/// system caret, and the warning that the config overrides its switch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Done {
    No,
    Yes,
}

/// The caret's rate (`caret::rate`): `[cursor] blink_interval_ms` if the
/// config sets it, else AppKit's own defaults,
/// `NSTextInsertionPointBlinkPeriodOn` and `...Off`, each read on its own
/// as AppKit reads it, with 250 ms where one is unset -- AppKit's unset
/// 650/350 is no longer taken. `doubleForKey` converts a number or a
/// numeric string, and answers 0 for a key that is absent or will not
/// convert -- which `of_defaults` reads as unset -- by its header. A set
/// interval reads no key at all.
fn os_cycle(interval: crate::config::BlinkInterval) -> Cycle {
    if let crate::config::BlinkInterval::Set(_) = interval {
        return Cycle::of_config(interval);
    }
    let d = NSUserDefaults::standardUserDefaults();
    crate::caret::rate(
        interval,
        d.doubleForKey(&NSString::from_str("NSTextInsertionPointBlinkPeriodOn")),
        d.doubleForKey(&NSString::from_str("NSTextInsertionPointBlinkPeriodOff")),
    )
}

/// Whether this is macOS 15 or later, where the non-blinking switch exists.
fn os_at_least_15() -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
        majorVersion: 15,
        minorVersion: 0,
        patchVersion: 0,
    })
}

/// macOS's Reduce Motion, from the window crate's safe getter.
fn reduce_motion() -> bool {
    macos_liquid_glass::accessibility::reduce_motion()
}

/// macOS's Reduce Transparency, from the window crate's safe getter
/// ([[source: mlg-accessibility]] 58-60).
fn reduce_transparency() -> bool {
    macos_liquid_glass::accessibility::reduce_transparency()
}

/// macOS's Increase Contrast, from the window crate's safe getter
/// ([[source: mlg-accessibility]] 62-70).
fn increase_contrast() -> bool {
    macos_liquid_glass::accessibility::increase_contrast()
}

/// The trail a change of the motion preference switches to, if it changes
/// the trail at all: only the default follows the system.
fn motion_change(
    pick: crate::config::TrailPick,
    current: crate::config::CursorTrail,
    motion: crate::config::SystemMotion,
) -> Option<crate::config::CursorTrail> {
    let next = pick.effective(motion);
    (next != current).then_some(next)
}

/// Switch the trail to `next` now, cutting any trail in flight -- a fire
/// is not left to burn out under Reduce Motion -- and say whether a frame
/// is owed to clear what was drawn: one if something was in flight at
/// `now`, none at rest.
fn retrail(
    chase: &mut crate::trail::Chase<HostTime>,
    ghost: &mut Option<TrailState>,
    last_cell: &mut Option<(Row, Col)>,
    next: crate::config::CursorTrail,
    now: Option<HostTime>,
) -> Owes {
    let drawn = now.is_some_and(|t| chase.owes(t) == Owes::Frame) || ghost.is_some();
    chase.restyle(next.style());
    *ghost = None;
    *last_cell = None;
    match drawn {
        true => Owes::Frame,
        false => Owes::Nothing,
    }
}

/// The scroller the user's "Show scroll bars" setting asks for:
/// `NSScroller`'s preferred style, which follows the setting and, under
/// Automatically, whether a mouse is connected. `BAUD_SCROLLER` names one
/// instead, for a check that must not change the user's settings -- an
/// instrument like `BAUD_GPU`, not a configuration. Off the main thread,
/// where AppKit cannot be asked, it is the overlay.
fn scroller_style() -> Scroller {
    match std::env::var("BAUD_SCROLLER").as_deref() {
        Ok("legacy") => return Scroller::Legacy,
        Ok("overlay") => return Scroller::Overlay,
        _ => {}
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return Scroller::Overlay;
    };
    match NSScroller::preferredScrollerStyle(mtm) {
        NSScrollerStyle::Legacy => Scroller::Legacy,
        _ => Scroller::Overlay,
    }
}

/// Animated Images as `access::image_motion` reads it.
fn auto_play(a: AnimatedImages) -> crate::access::AutoPlay {
    match a {
        AnimatedImages::Play => crate::access::AutoPlay::On,
        AnimatedImages::Pause => crate::access::AutoPlay::Off,
    }
}

/// **Blinking text's phases as Dim Flashing Lights asks them drawn**: each
/// off phase faint rather than hidden while it is on, the rates unchanged
/// -- ECMA-48's two rates hold, and the page shows less change at each
/// flip. Phases Reduce Motion held shown stay shown, so with both settings
/// on the text is steady.
fn dim_off_phases(phases: BlinkPhases, flashing: Flashing) -> BlinkPhases {
    match flashing {
        Flashing::Full => phases,
        Flashing::Dimmed => phases.dimmed(),
    }
}

/// What a change of a setting owes the window's layout.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Relayout {
    Now,
    Nothing,
}

/// **Whether a heard "Show scroll bars" change lays the window out
/// again**: only when the style the setting asks for now is not the one
/// drawn. A notification that leaves the style as it was -- a mouse
/// plugged in under "Always" -- moves no column, and with nothing heard
/// the style is not read at all.
fn scroller_change(heard: Heard, drawn: Scroller, read: impl FnOnce() -> Scroller) -> Relayout {
    match heard {
        Heard::Changed if read() != drawn => Relayout::Now,
        Heard::Changed | Heard::Unchanged => Relayout::Nothing,
    }
}

/// The print panel's states, in the atomic the driver and the block that
/// shows the panel share: none up, one scheduled on the main queue, one up.
const PANEL_IDLE: u8 = 0;
const PANEL_SCHEDULED: u8 = 1;
const PANEL_SHOWN: u8 = 2;

/// Whether macOS lists a printer, for the core's DSR `?15` (`print.rs`).
fn printers_listed() -> crate::print::Printers {
    match baud_present_macos::print::printers() {
        baud_present_macos::print::Listed::Some => crate::print::Printers::Listed,
        baud_present_macos::print::Listed::None => crate::print::Printers::None,
    }
}

/// **The printer list read on a thread of its own**: `printerNames` waits
/// 0.3-0.8 ms on the print system's daemon a call, and 40-78 ms on the
/// call that has to start it, which the thread that feeds the core and
/// draws does not. Started at the first read; each read's answer is left
/// for the pump and the main thread woken for it. The thread ends with
/// the app, its channel closed.
struct PrinterList {
    waker: Waker,
    ask: std::cell::OnceCell<std::sync::mpsc::Sender<()>>,
    found: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

/// What [`PrinterList::found`] holds: nothing new, or the last read's.
const LIST_NONE: u8 = 0;
const LIST_LISTED: u8 = 1;
const LIST_EMPTY: u8 = 2;

impl PrinterList {
    fn new(waker: Waker) -> PrinterList {
        PrinterList {
            waker,
            ask: std::cell::OnceCell::new(),
            found: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(LIST_NONE)),
        }
    }

    /// Read the list again, off this thread.
    fn read(&self) {
        let tx = self.ask.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let (found, waker) = (self.found.clone(), self.waker);
            std::thread::spawn(move || {
                while rx.recv().is_ok() {
                    let v = match printers_listed() {
                        crate::print::Printers::Listed => LIST_LISTED,
                        crate::print::Printers::None => LIST_EMPTY,
                    };
                    found.store(v, std::sync::atomic::Ordering::Release);
                    waker.wake();
                }
            });
            tx
        });
        let _ = tx.send(());
    }

    /// The last read's answer, once.
    fn take(&self) -> Option<crate::print::Printers> {
        match self
            .found
            .swap(LIST_NONE, std::sync::atomic::Ordering::Acquire)
        {
            LIST_LISTED => Some(crate::print::Printers::Listed),
            LIST_EMPTY => Some(crate::print::Printers::None),
            _ => None,
        }
    }
}

/// **This machine's names, read off the main thread**: `NSProcessInfo`'s
/// `hostName` took 5-29 ms on its first call in five runs of a probe on
/// this box, and it was read on the main thread between the window's
/// being shown and its first frame. `ProcessInfo` "is
/// thread-safe in macOS 10.7 and later" ([[source: apple-processinfo]]);
/// the archived Threading Programming Guide still lists `NSProcessInfo`
/// among the thread-unsafe classes
/// ([[source: apple-thread-safety-summary]]); the class reference is the
/// current text, and this rests on it. The names are taken at the first
/// derived title once the thread has ended, never waited for: the first
/// title is derived on the shell's first output, which comes while the
/// launch logo plays, and a wait there holds the logo's frames for as long
/// as the name takes to read (`derived_title`, note AJ section 18).
enum Hosts {
    Reading(std::thread::JoinHandle<Vec<String>>),
    Read(Vec<String>),
}

impl Hosts {
    /// The read started, on its own thread; on the calling thread when no
    /// thread can be made.
    fn start() -> Hosts {
        match std::thread::Builder::new()
            .name("hosts".into())
            .spawn(machine_names)
        {
            Ok(h) => Hosts::Reading(h),
            Err(_) => Hosts::Read(machine_names()),
        }
    }

    /// The names, once the thread that reads them has ended; `None` while
    /// it reads, never waited for. A thread that ended in a panic leaves
    /// `gethostname`'s, which no lookup waits on.
    fn names(&mut self) -> Option<&[String]> {
        if let Hosts::Reading(h) = self {
            if h.is_finished() {
                if let Hosts::Reading(h) = core::mem::replace(self, Hosts::Read(Vec::new())) {
                    *self =
                        Hosts::Read(h.join().unwrap_or_else(|_| {
                            crate::hostlog::machine_name().into_iter().collect()
                        }));
                }
            }
        }
        match self {
            Hosts::Read(names) => Some(names),
            Hosts::Reading(_) => None,
        }
    }
}

/// **The names a shell's report is judged by**: this machine's once read;
/// none for a report naming no host, which needs none; and `None`, wait,
/// for a report naming a host while they are still being read, which
/// judged against no names would be refused as another machine's.
fn judged_by<'n>(
    names: Option<&'n [String]>,
    cwd: Option<baud_core::WorkingDirectory<'_>>,
) -> Option<&'n [String]> {
    match (names, cwd.map(|d| d.host)) {
        (Some(names), _) => Some(names),
        (None, Some(baud_core::FileHost::Named(_))) => None,
        (None, _) => Some(&[]),
    }
}

/// Whether a title waits for this machine's names: the shell's `OSC 7`
/// named a host while they were still being read (`Hosts`), and the title
/// is derived again once they are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TitleWaits {
    No,
    Names,
}

/// **This machine's names, either of which an `OSC 7` may name.**
///
/// The name `gethostname` gives, which is what shells send (zsh's `$HOST`,
/// bash's `$HOSTNAME`, fish's `$hostname`), and `NSProcessInfo`'s
/// `hostName`, which on macOS can be a different name, such as
/// `<name>.local`. On a Mac with `HostName` set the two can be the same;
/// the code accepts either. RFC 8089 § 3's "resolves to the machine"
/// holds of both (`[[source: rfc8089]]`).
fn machine_names() -> Vec<String> {
    let mut names: Vec<String> = crate::hostlog::machine_name().into_iter().collect();
    let process = baud_present_macos::link::host_name();
    if !process.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(&process)) {
        names.push(process);
    }
    names
}

#[cfg(test)]
mod host_tests {
    use super::{Hosts, judged_by, machine_names};
    use baud_core::{FileHost, WorkingDirectory};

    /// **The names are never waited for** (note AJ section 18): the first
    /// title is derived on the shell's first output, while the launch logo
    /// plays on the same thread, so taking the names while their thread
    /// still reads answers at once with none -- here a thread held until
    /// after the answer -- and gives them once it has ended. Before, the
    /// take joined the thread, and the logo's frames waited on the read.
    #[test]
    fn the_machines_names_are_never_waited_for() {
        let (release, held) = std::sync::mpsc::channel::<()>();
        let reading = std::thread::spawn(move || {
            let _ = held.recv();
            vec!["here".to_string()]
        });
        let mut hosts = Hosts::Reading(reading);
        let (tx, rx) = std::sync::mpsc::channel();
        let taker = std::thread::spawn(move || {
            let _ = tx.send(hosts.names().map(<[String]>::to_vec));
            hosts
        });
        let answer = rx.recv_timeout(std::time::Duration::from_secs(2));
        release.send(()).expect("the read is held");
        let mut hosts = taker.join().expect("the take returns");
        assert_eq!(answer, Ok(None), "answered at once, with none");
        let names = loop {
            if let Some(n) = hosts.names() {
                break n.to_vec();
            }
            std::thread::yield_now();
        };
        assert_eq!(names, ["here"], "the names once the read has ended");
    }

    /// **A report naming a host waits for the names**, where one naming
    /// none needs none: judged against no names, `OSC 7` naming this
    /// machine would be refused as another's.
    #[test]
    fn a_report_naming_a_host_waits_for_the_names() {
        let named = WorkingDirectory {
            host: FileHost::Named(b"here"),
            path: b"/tmp",
        };
        let local = WorkingDirectory {
            host: FileHost::Local,
            path: b"/tmp",
        };
        let names = ["here".to_string()];
        assert_eq!(judged_by(None, Some(named)), None, "waits");
        assert_eq!(judged_by(None, Some(local)), Some(&[][..]));
        assert_eq!(judged_by(None, None), Some(&[][..]));
        assert_eq!(judged_by(Some(&names), Some(named)), Some(&names[..]));
    }

    /// **An `OSC 7` naming the machine as a shell names it is local**: the
    /// name `/bin/hostname` prints, which is `gethostname`'s and zsh's
    /// `$HOST`, gives the shell's path and so the title the shell's
    /// folder. On macOS `NSProcessInfo`'s name can be a different name,
    /// such as `<name>.local`, and comparing with it alone refused every
    /// local report as remote. On a Mac with `HostName` set the two can be
    /// the same; the code accepts either.
    #[test]
    fn an_osc_7_naming_the_hostname_is_this_machine() {
        let out = std::process::Command::new("/bin/hostname")
            .output()
            .expect("hostname runs");
        let name = String::from_utf8(out.stdout).expect("a UTF-8 name");
        let name = name.trim();
        assert!(!name.is_empty(), "the machine has a name");
        let hosts = machine_names();
        let d = WorkingDirectory {
            host: FileHost::Named(name.as_bytes()),
            path: b"/tmp",
        };
        assert_eq!(
            crate::title::local(d, &hosts),
            Some(&b"/tmp"[..]),
            "the shell's {name:?} against {hosts:?}"
        );
        let mut derive = crate::title::Derive::new();
        assert_eq!(derive.title(Some(d), &hosts, None).as_deref(), Some("/tmp"));
        assert_eq!(derive.refused, crate::title::Refused(0));
    }

    /// **A shell integrated for iTerm2 keeps its folder in the title**: it
    /// sends `RemoteHost=$USER@$(hostname -f)` and then `CurrentDir` at each
    /// prompt (`[[source: iterm2-escape-codes]]`), so the directory the core
    /// keeps names this machine by its fully qualified name, which must be
    /// one of the names the title takes as local; another host's is not.
    #[test]
    fn an_iterm2_shell_on_this_machine_titles_its_folder() {
        let out = std::process::Command::new("/bin/hostname")
            .arg("-f")
            .output()
            .expect("hostname runs");
        let name = String::from_utf8(out.stdout).expect("a UTF-8 name");
        let hosts = machine_names();
        let ws = baud_core::WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        for (host, want) in [(name.trim(), Some("/tmp")), ("build-box.example", None)] {
            let mut owned = baud_core::Owned::new(&ws);
            let mut t = baud_core::Term::new(owned.storage(), &ws);
            let bytes = format!("\x1b]1337;RemoteHost=me@{host}\x07\x1b]1337;CurrentDir=/tmp\x07");
            t.write_at(baud_core::BytePos::ZERO, bytes.as_bytes());
            t.feed(
                baud_core::BytePos::ZERO,
                baud_core::ReadLen::from_index(bytes.len()),
            );
            let mut derive = crate::title::Derive::new();
            let got = derive.title(t.working_directory(), &hosts, None);
            assert_eq!(got.as_deref(), want, "{host:?} against {hosts:?}");
        }
    }
}

/// **A print job onto the print panel, laid out off the main thread and
/// shown once the callback that took it has returned**, for the reason
/// `widen_later` waits. The sheet's area and the font's measures are read
/// here, on the main thread; the job is laid out on a thread of its own
/// (`crate::print::pages`, 63 ms for a 2 MiB job in a release build), so
/// the thread that feeds the core and draws does not wait for it; the text
/// views and the sheet are AppKit's, on the main queue
/// (`baud_present_macos::print::show_panel`), which returns with the sheet
/// up. Nothing prints until the user presses Print.
fn print_later(
    window: &NSWindow,
    name: String,
    job: crate::print::Job,
    panel: std::sync::Arc<std::sync::atomic::AtomicU8>,
) {
    use std::sync::atomic::Ordering::Relaxed;
    let (width, height) = baud_present_macos::print::sheet_area();
    let (advance, leading) = baud_present_macos::print::font_metrics();
    let w = dispatch2::MainThreadBound::new(window.retain(), window.mtm());
    std::thread::spawn(move || {
        let (pages, layout, stripped) =
            crate::print::pages(&job.bytes, width, height, advance, leading);
        crate::info!(
            "print: {} sheets at {:.1} pt, {stripped} controls not printed, {} pictures reduced, {} left out",
            pages.len(),
            layout.size,
            stripped.reduced(),
            stripped.left_out()
        );
        let title = crate::print::job_title(&name, job.cut, stripped);
        dispatch2::DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                panel.store(PANEL_IDLE, Relaxed);
                return;
            };
            // A job that prints nothing -- a controller run of SGR alone
            // -- is no panel: there is no sheet to offer.
            if pages.is_empty() {
                panel.store(PANEL_IDLE, Relaxed);
                return;
            }
            baud_present_macos::print::show_panel(w.get(mtm), &title, &pages, layout, mtm);
            panel.store(PANEL_SHOWN, Relaxed);
        });
    });
}

/// The window to `width` points of content, its height kept, **once the
/// callback that asked has returned**. `setContentSize:` resizes the view,
/// and the view's `setFrameSize:` reports to the responder, which the
/// window crate borrows for the whole of the callback this is called from
/// (`with_responder`): a resize from inside one is a second borrow of it.
/// The main queue runs the block from the run loop, in its common modes, as
/// the waker's wake is delivered.
fn widen_later(window: &NSWindow, width: CGFloat) {
    let w = dispatch2::MainThreadBound::new(window.retain(), window.mtm());
    dispatch2::DispatchQueue::main().exec_async(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let w = w.get(mtm);
        let height = w.contentView().map_or(0.0, |v| v.frame().size.height);
        w.setContentSize(NSSize::new(width, height));
    });
}

/// **A window operation's AppKit call, once the callback that asked has
/// returned**, for the reason `widen_later` waits: a frame change resizes
/// the view, whose `setFrameSize:` reports to the responder the window
/// crate borrows for the callback. `deminiaturize:` (`[[source:
/// apple-nswindow-deminiaturize]]`), `miniaturize:` (`[[source:
/// apple-nswindow-miniaturize]]`), `orderFrontRegardless`, which raises
/// the window "even if its application isn't active" and takes no focus
/// (`[[source: apple-nswindow-orderfrontregardless]]`), `orderBack:`,
/// "without changing either the key window or the main window" (`[[source:
/// apple-nswindow-orderback]]`), `toggleFullScreen:` (`[[source:
/// apple-nswindow-togglefullscreen]]`), and `setFrame:display:` (`[[source:
/// apple-nswindow-setframe]]`) for a move, a resize and a maximise.
fn window_later(window: &NSWindow, action: crate::winops::MacAction) {
    use crate::winops::MacAction;
    let w = dispatch2::MainThreadBound::new(window.retain(), window.mtm());
    dispatch2::DispatchQueue::main().exec_async(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let w = w.get(mtm);
        match action {
            MacAction::Deminiaturize => w.deminiaturize(None),
            MacAction::Miniaturize => w.miniaturize(None),
            MacAction::Front => w.orderFrontRegardless(),
            MacAction::Back => w.orderBack(None),
            MacAction::ToggleFullScreen => w.toggleFullScreen(None),
            MacAction::Frame(f, _) => w.setFrame_display(
                NSRect::new(NSPoint::new(f.x, f.y), NSSize::new(f.w, f.h)),
                true,
            ),
            MacAction::Redraw | MacAction::Nothing(_) => {}
        }
    });
}

/// The grid a view `px` pixels big holds, in cells of `cell` pixels: the
/// view less `pad` on each side, and less a legacy scroller's `gutter` on
/// the right, so the frame -- the grid and the gutter -- fits inside the
/// padding. `relayout`'s arithmetic, pure so the columns the gutter takes
/// are a test's to check.
fn grid_in(px: (f64, f64), pad: f64, gutter: f64, cell: (f64, f64)) -> WinSize {
    let cols = (((px.0 - 2.0 * pad - gutter).max(cell.0) / cell.0).floor() as i64)
        .clamp(2, i64::from(MAX_GRID.ws_col));
    let rows = (((px.1 - 2.0 * pad).max(cell.1) / cell.1).floor() as i64)
        .clamp(1, i64::from(MAX_GRID.ws_row));
    WinSize {
        ws_col: u16::try_from(cols).unwrap_or(2),
        ws_row: u16::try_from(rows).unwrap_or(1),
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

#[cfg(test)]
mod scrollbar_mode_tests {
    /// **`?30` reaches the window.** It is AppKit-bound, so the source is
    /// held: the pump acts on a change after the read's window requests,
    /// which follow the status line's frame;
    /// hidden, the frame is handed no scrollbar and a press in its strip is
    /// cells; and every layout keeps the gutter `?30` leaves
    /// (`shown_gutter`), never the style's alone.
    #[test]
    fn the_scrollbar_mode_reaches_the_window() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart, so this test's own text is not what it finds.
        let once = |needle: &str| {
            assert_eq!(SRC.matches(needle).count(), 1, "once: {needle}");
            SRC.find(needle).expect("found")
        };
        // The status line's frame first, so a request in the same read is
        // measured on the window it leaves (`mac_status_frame`).
        let requests = once(concat!(
            "let left = self.status_line();\n",
            "        self.window_requests(&asked, left);\n",
            "        self.scrollbar_mode();"
        ));
        let pump = once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"));
        assert!(pump < requests, "in the pump");
        let shown = once(concat!("let bar = match ", "self.bar_shown {"));
        let handed = once(concat!("self.renderer.set_scrollbar(", "bar);"));
        assert!(shown < handed, "the frame is handed the bar `?30` leaves");
        once(concat!("baud_core::ScrollbarShown::Hidden => ", "None,"));
        // The press rules are `bar::press`, which reads `?30` as the
        // driver last acted on it, before the strip takes a press.
        once(concat!("shown: self.", "bar_shown,"));
        once(concat!("crate::bar::", "press("));
        const BAR: &str = include_str!("bar.rs");
        let in_bar = |needle: &str| {
            assert_eq!(BAR.matches(needle).count(), 1, "once in bar.rs: {needle}");
            BAR.find(needle).expect("found")
        };
        let hidden = in_bar(concat!(
            "if c.shown == ",
            "ScrollbarShown::Hidden {\n        *pointer = Place::Grid;"
        ));
        let strip = in_bar(concat!("let strip = place(", "x, f.size.0, f.widths);"));
        assert!(hidden < strip, "before the strip takes a press");
        assert_eq!(
            SRC.matches(concat!("gutter(self.app.", "scroller,"))
                .count(),
            0,
            "a layout that ignores ?30"
        );
    }
}

#[cfg(test)]
mod deferred_tests {
    use super::{Deferred, Refont, Reshape, deferred, refont_step};

    /// **A face waiting for a frame survives a size step asked after it**:
    /// `OSC 50 ; Menlo` and then `#+1` or Bigger while a frame is in flight
    /// leave both, the step's size and the program's face.
    #[test]
    fn a_waiting_face_survives_a_step_after_it() {
        let slot = |f: Deferred| match f {
            Deferred::Font(face, p) => (face, p),
            Deferred::Zoom(p) => (None, p),
            Deferred::Nothing => (None, 0.0),
        };
        let menlo = crate::fonts::Slot::TEST;
        let after = deferred(Deferred::Font(Some(menlo), 13.0), None, 14.0);
        assert_eq!(slot(after), (Some(menlo), 14.0));
        assert!(
            matches!(deferred(Deferred::Zoom(13.0), None, 14.0), Deferred::Zoom(p) if p == 14.0)
        );
        assert!(
            matches!(deferred(Deferred::Nothing, Some(menlo), 12.0), Deferred::Font(Some(_), p) if p == 12.0)
        );
    }

    /// **Naming the face in force replaces a face that waits**: with
    /// Proto Hebrew Mono C leading at 13 points and a frame in flight,
    /// `OSC 50 ; STIX Two Math` and then `OSC 50 ; Proto Hebrew Mono C`
    /// leave Proto leading, and so does `#+1 Proto Hebrew Mono C`, at 14.
    /// Performed, the face in force is dropped and only the size changes.
    #[test]
    fn naming_the_face_in_force_replaces_a_waiting_one() {
        let (stix, proto) = (crate::fonts::Slot::TEST, crate::fonts::Slot::FIRST);
        let leads = |f| f == proto;
        let wait = Some(Reshape::Wait);
        let stix_waits = Deferred::Font(Some(stix), 13.0);
        for to in [13.0, 14.0] {
            let got = refont_step(stix_waits, wait, Some(proto), to, 13.0, leads);
            assert!(
                matches!(got, Refont::Wait(Deferred::Font(Some(f), p)) if f == proto && p == to),
                "at {to}"
            );
        }
        let now = Some(Reshape::Now);
        let done = |to| refont_step(Deferred::Nothing, now, Some(proto), to, 13.0, leads);
        assert!(matches!(done(13.0), Refont::Nothing));
        assert!(matches!(done(14.0), Refont::Now(None)));
        assert!(matches!(
            refont_step(Deferred::Nothing, now, Some(stix), 13.0, 13.0, leads),
            Refont::Now(Some(f)) if f == stix
        ));
    }

    /// **The find is brought up to date after every read fed**, its own
    /// chunks or the reads a ReGIS delay held, which a tick feeds with no
    /// chunk: the refresh is gated on what the frame's own gate is, so a
    /// read that owes a frame owes the find too.
    #[test]
    fn the_find_refreshes_after_every_read_fed() {
        let src = include_str!("app.rs");
        let pump = src
            .split_once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"))
            .expect("the pump")
            .1;
        let fed = "if !drained.chunks.is_empty() || fed == crate::reads::Fed::Some {";
        let at = pump
            .find("if let Some(f) = self.app.finding.as_mut() {")
            .expect("the find's refresh");
        let gate = pump[..at].rfind("if ").expect("its gate");
        assert_eq!(pump[gate..at].trim(), fed, "the find's gate");
        let dirty = pump
            .find("self.app.pending = Pending::Dirty;")
            .expect("the frame's");
        let frame = pump[..dirty].rfind("if ").expect("its gate");
        assert_eq!(pump[frame..dirty].trim(), fed, "the frame's gate");
    }
}

#[cfg(test)]
mod link_pointer_tests {
    use super::*;

    /// **A function's body in `src`**, from its head to the first line that
    /// closes at its indent; one head of it in the file.
    pub(super) fn body<'a>(src: &'a str, head: &str) -> &'a str {
        assert_eq!(src.matches(head).count(), 1, "one {head}");
        let b = src.split(head).nth(1).expect(head);
        &b[..b.find("\n    }\n").expect("the function ends")]
    }

    /// **The functions of `src`, by name, each with its code collapsed**:
    /// comment lines and whitespace left out. A function starts at a line
    /// that opens one at the top level or in an `impl`.
    fn functions(src: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for line in src.lines() {
            let indent = line.len() - line.trim_start().len();
            let t = line.trim_start();
            let head = ["fn ", "pub fn ", "pub(crate) fn "]
                .iter()
                .find_map(|h| t.strip_prefix(h))
                .filter(|_| indent <= 4);
            if let Some(rest) = head {
                out.push((
                    rest.chars().take_while(|c| *c != '(').collect(),
                    String::new(),
                ));
            } else if !t.starts_with("//") {
                if let Some((_, code)) = out.last_mut() {
                    code.extend(t.chars().filter(|c| !c.is_whitespace()));
                }
            }
        }
        out
    }

    /// **The pointing hand is AppKit's**: the pointer the window wants
    /// over a link a click opens is the pointing hand, over a program's
    /// `OSC 22` too, and the cursor set for it is `NSCursor.pointingHand`.
    /// AppKit's cursors are not asked for here -- off the main thread and
    /// with no application, `+[NSCursor arrowCursor]` answered NULL -- so
    /// the call is held in the source. Perturbation: the hand shown as the
    /// arrow -- the source assertion fails.
    #[test]
    fn a_link_a_click_opens_shows_appkits_pointing_hand() {
        use crate::link::{Chord, Host, Hover};
        use crate::pointer::{SystemPointer, system_pointer};
        const SRC: &str = include_str!("app.rs");
        let cursor = body(
            SRC,
            concat!(
                "fn system_",
                "cursor(p: crate::pointer::SystemPointer) -> Retained<NSCursor> {"
            ),
        );
        assert!(
            cursor.contains(concat!(
                "SystemPointer::PointingHand => ",
                "NSCursor::pointingHandCursor(),"
            )),
            "the hand is not AppKit's pointing hand"
        );
        let ws = baud_core::WinSize {
            ws_row: 4,
            ws_col: 40,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let bytes = b"\x1b]22;crosshair\x1b\\\x1b]8;;https://h.example/\x1b\\web\x1b]8;;\x1b\\";
        t.write_at(baud_core::BytePos::ZERO, bytes);
        t.feed(
            baud_core::BytePos::ZERO,
            baud_core::ReadLen::from_index(bytes.len()),
        );
        let mut h = Hover::default();
        let at = Some((Row::ZERO, Col::ZERO));
        let s = Scrolled::START;
        let _ = h.update(&t, at, s, Chord::Cmd, baud_core::Focus::Gained, Host(""));
        let want = h.pointer(t.pointer_shape()).map(system_pointer);
        assert_eq!(want, Some(SystemPointer::PointingHand));
        let _ = h.update(&t, at, s, Chord::Other, baud_core::Focus::Gained, Host(""));
        let want = h.pointer(t.pointer_shape()).map(system_pointer);
        assert_eq!(want, Some(SystemPointer::Crosshair), "the chord let go");
    }

    /// **Every change of the link preview shows the pointer again**, and
    /// the pointer shown is the preview's: the mouse handler shows it after
    /// the preview reads the move, every function that updates, refreshes
    /// or clears the preview, or tells it who a left press goes to -- a
    /// move, the display-link tick and a frame's read, the frame's thumb,
    /// a key without Command, the keyboard lost -- shows it again,
    /// leaving takes the hand back before what shows is unknown, and
    /// showing it asks the preview with the program's `OSC 22` shape.
    /// AppKit-bound, so this holds the source; `link.rs`'s
    /// `the_hand_shows_while_a_click_would_open_the_link` holds the
    /// decision. Perturbations: drop the show from `read_hover_as` -- the tick
    /// and a frame's read keep the hand after Command goes up -- or show
    /// the program's shape alone, or mark the pointer unknown before the
    /// preview reads the exit; each fails here.
    #[test]
    fn every_change_of_the_preview_shows_the_pointer_again() {
        const SRC: &str = include_str!("app.rs");
        // Spelled apart, so this test's own text is not what it finds.
        let calls = [
            concat!("hover.", "update("),
            concat!("hover.", "refresh("),
            concat!("hover.", "clear()"),
            concat!("hover.", "set_left_press("),
        ];
        let found: Vec<(String, String)> = functions(SRC)
            .into_iter()
            .filter(|(_, code)| calls.iter().any(|c| code.contains(c)))
            .collect();
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "read_hover_as",
                "present",
                "key",
                "set_focus",
                "hover_mouse"
            ],
            "a new site: it must show the pointer too"
        );
        for (name, code) in &found {
            assert!(
                code.contains(concat!("reshape_", "pointer()")),
                "{name} changes the preview and does not show the pointer"
            );
        }
        let show = body(SRC, concat!("    fn show_", "pointer(&mut self) {"));
        assert!(
            show.contains(concat!(".pointer(self.term.pointer_", "shape())"))
                && show.contains(concat!("system_cursor(p).", "set()")),
            "the pointer shown is not the preview's"
        );
        // A move shows the pointer after the preview reads it, and leaving
        // marks it unknown only after the preview's clear has taken the
        // hand back: marked first, the clear sets nothing and the hand
        // stays as the pointer leaves, and over the window when it comes
        // back without a shape to set.
        let mouse = body(SRC, concat!("    fn mouse(&mut self, ", "event: &Mouse) {"));
        let at = |s: &str| mouse.find(s).unwrap_or_else(|| panic!("{s}"));
        let hover = at(concat!("self.hover_", "mouse(event);"));
        assert!(hover < at(concat!("self.app.show_", "pointer()")));
        assert_eq!(mouse.matches(concat!("Shown::", "Unknown")).count(), 1);
        assert!(
            hover < at(concat!("Shown::", "Unknown")),
            "the pointer is marked unknown before the preview takes its hand back"
        );
    }

    /// **A frame tests a left press where the preview last read the
    /// pointer**: the hover's own read of the event comes before the
    /// scrollbar's (`mouse`), and a frame it presents runs before the
    /// scrollbar has moved `pointer` to this event, so testing the press at
    /// `pointer` would keep the hand over the legacy gutter, whose click the
    /// scrollbar takes, and drop it over a link a click opens.
    #[test]
    fn a_frame_tests_the_press_where_the_preview_read_the_pointer() {
        const SRC: &str = include_str!("app.rs");
        let present = body(
            SRC,
            concat!(
                "    fn present(&mut self, ",
                "screen: &DrawableView, show: Show) {"
            ),
        );
        assert!(
            present.contains(concat!("left_press(&self.term, self.", "strip,")),
            "a frame tests the press at the scrollbar's stale place"
        );
        let hover = body(
            SRC,
            concat!("    fn hover_", "mouse(&mut self, event: &Mouse) {"),
        );
        let set = hover
            .find(concat!("app.strip = ", "strip;"))
            .expect("the preview's place is kept");
        let first = hover
            .find(concat!(".set_left_", "press("))
            .expect("the press is tested");
        let shown = hover
            .find(concat!("app.", "present("))
            .expect("a changed preview is shown");
        assert!(
            set < first && set < shown,
            "the press is tested before the place is kept"
        );
    }
}

#[cfg(test)]
mod driver_rule_tests {
    use super::link_pointer_tests::body;

    /// **The window reads the system's caret at every focus gain, through
    /// the rule a test reaches** (`Caret::read_system`, held by
    /// `a_switch_turned_in_the_background_lands_at_the_focus_gain`): the
    /// gain's arm of `set_focus` asks `read_caret_system`, after the early
    /// return for no change, and that hands the rule the system's switch.
    /// Perturbation: the gain's read dropped, or the switch read outside
    /// the rule, fails this.
    #[test]
    fn a_focus_gain_reads_the_system_caret_through_the_rule() {
        const SRC: &str = include_str!("app.rs");
        let focus = body(
            SRC,
            concat!("    fn set_", "focus(&mut self, f: baud_core::Focus) {"),
        );
        let unchanged = focus
            .find(concat!("if self.app.focus == f {\n", "            return;"))
            .expect("a poll with no change returns");
        let gained = focus
            .find(concat!(
                "if f == baud_core::Focus::",
                "Gained {\n            self.read_caret_system();"
            ))
            .expect("a gain reads the system's caret");
        assert!(unchanged < gained, "read only on the change");
        let read = body(SRC, concat!("    fn read_caret_", "system(&mut self) {"));
        assert!(
            read.contains(concat!(".read_", "system(cycle, self.app.blink_choice,"))
                && read.contains(concat!("crate::pty::prefers_steady_", "caret()")),
            "the switch is not read through the rule"
        );
        assert_eq!(
            read.matches(concat!("set_", "system(")).count(),
            0,
            "set around the rule"
        );
    }
}

#[cfg(test)]
mod mark_tests {
    /// `show`'s body, from its signature to the line that closes it.
    fn show(src: &str) -> &str {
        let head = concat!("fn show(self, mtm: MainThreadMarker, ", "scale: CGFloat,");
        src.split_once(head)
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(body, _)| body)
            .expect("Opening::show")
    }

    /// **The window opens with what the shell already wrote**: `show`
    /// draws the first frame before any byte, as it always did, runs the
    /// opening turn, which drains the pty's channel into the terminal as a
    /// wake's turn does, and draws again before the window is ordered front
    /// when the turn fed something -- unless a synchronised update the
    /// bytes opened is honoured, which the frame drawn first then stands
    /// for -- and still wakes the view once the responder is in, for what
    /// lands after the turn. The windows are AppKit's, so the source is
    /// what can fail; every anchor is asserted present first, as the
    /// control, and the opening's gate draws nothing of its own
    /// (`frame_gate`).
    /// Perturbation: the turn moved after the window is shown, or the
    /// update's test dropped, fails it.
    #[test]
    fn the_first_frame_follows_the_opening_turn() {
        let src = include_str!("app.rs");
        let body = show(src);
        let first = concat!(
            "driver.app.present(&v, ",
            "Show::Now);\n        startup_mark(\"first frame\");"
        );
        let turn = concat!("driver.pump(", "Trigger::Opening);");
        let owed = concat!(
            "if driver.app.pending == Pending::Dirty\n",
            "            && driver.app.updates.state(&driver.app.term, Instant::now()) != ",
            "Update::Honoured\n        {\n",
            "            driver.app.present(&v, Show::Now);"
        );
        let shown = concat!("window.", "show();");
        let woken = concat!("view.waker()", ".wake();");
        for anchor in [first, turn, owed, shown, woken] {
            assert_eq!(body.matches(anchor).count(), 1, "once in show: {anchor}");
        }
        let at = |a: &str| body.find(a).expect("found");
        assert!(at(first) < at(turn), "the frame before any byte first");
        assert!(at(turn) < at(owed), "then the turn, then its frame");
        assert!(at(owed) < at(shown), "both before the window is on screen");
        assert!(at(shown) < at(woken), "the wake after it, kept");
        // The turn is the pump's whole, which feeds the drained reads.
        let pump = src
            .split_once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(b, _)| b)
            .expect("the pump");
        let drained = pump
            .find("let drained = self.app.pty.drain();")
            .expect("the drain");
        let fed = pump.find("crate::reads::paced(").expect("the feed");
        assert!(drained < fed);
        let gate = |trigger| {
            super::frame_gate(super::FrameGate {
                pending: super::Pending::Dirty,
                update: Some(crate::bell::Update::Closed),
                trigger,
                echo: super::Echo::Fed,
                guess: super::Guess::Owed,
                marked: super::Marked::Owed,
                since_commit: std::time::Duration::from_secs(1),
                interval: std::time::Duration::from_millis(8),
                flight: baud_present_macos::InFlight::Idle,
            })
        };
        assert_eq!(gate(super::Trigger::Opening), super::Decided::Later);
        assert_eq!(
            gate(super::Trigger::Wake),
            super::Decided::Now,
            "the control"
        );
    }

    /// **The strip is in the title bar and the grid's top follows full
    /// screen**: `show` puts the lockup in a leading titlebar accessory
    /// and the lights in a trailing one, computing no position from the
    /// size `new` built the container at -- a pinned grid's window had
    /// already changed it -- and re-centres the lockup once the body has
    /// the container. It hears the window's full screen, and the
    /// observer's block calls `strip::follow`, which takes the grid's top
    /// from `strip::grid_top` and gives it to the terminal's view with
    /// `strip::apply`; the observer is kept with the window. Where things
    /// go is `strip.rs`'s and `mark.rs`'s, and
    /// `baud-app/tests/titlebar_mark.rs` checks them in real windows,
    /// posting the full-screen notifications by hand. Perturbation: drop
    /// the observer and the third check fails; drop the `place_icon()`
    /// after the body and the second does.
    #[test]
    fn show_puts_the_strip_in_the_title_bar_and_follows_full_screen() {
        let src = include_str!("app.rs");
        let body = show(src);
        assert!(!body.contains(concat!("full", ".height")), "{body}");
        let put = body
            .find(concat!("crate::mark::Mark::", "put(mtm, &window, inset,"))
            .expect("show puts the mark in the title bar");
        let taken = body
            .find(concat!("glass.set_content_view(", "&container);"))
            .expect("the body takes the container");
        let placed = body[taken..]
            .find(concat!("driver.place_", "icon();"))
            .map(|at| taken + at)
            .expect("the mark re-placed after the body takes the container");
        assert!(put < taken && taken < placed);
        let lights = body
            .find(concat!("crate::strip::Accessory::", "put("))
            .expect("show puts the lights' accessory");
        assert!(body[lights..].starts_with(concat!(
            "crate::strip::Accessory::put(\n            mtm,\n            &window,\n            ",
            "TitlebarSide::Trailing,"
        )));
        let observed = body
            .find(concat!(
                "baud_present_macos::full_screen::",
                "when_full_screen("
            ))
            .expect("show hears the window's full screen");
        assert!(body[observed..].contains(concat!(
            "crate::strip::follow(change, ",
            "w.ns_window(), &v, &c)"
        )));
        assert!(body.contains(concat!("open._full_screen = ", "Some(full_screen);")));
        let strip = include_str!("strip.rs");
        let follow = strip
            .split_once(concat!("pub(crate) fn follow(", "change: Change"))
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(b, _)| b)
            .expect("strip::follow");
        assert!(follow.contains(concat!("grid_top(", "change,")), "{follow}");
        assert!(
            follow.contains(concat!("apply(view, ", "container, top)")),
            "{follow}"
        );
        let mark = include_str!("mark.rs");
        assert!(mark.contains(concat!("TitlebarSide::", "Leading,")));
        assert!(!mark.contains(concat!("container.", "addSubview")));
    }

    /// **A close asked before the window exists is asked again once it
    /// does**: the opening turn runs before `show` gives the driver its
    /// window, and a child that has already exited reaches `close_later`
    /// there. It marks the close asked only past the window's guard, so
    /// the turn of the wake at the end of `show`, which finds the stream
    /// still closed, dispatches it: no turn ends before the closed
    /// stream's branch. The close is a dispatch to AppKit's main queue,
    /// so the source is what can fail; every anchor is asserted present
    /// first, as the control. Perturbation: the store put back before
    /// the guard fails it, and so does a return on an empty drain.
    #[test]
    fn a_close_before_the_window_is_asked_again_after_it() {
        let src = include_str!("app.rs");
        let body = src
            .split_once(concat!("    fn close_later(", "&mut self) {"))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(b, _)| b)
            .expect("close_later");
        let asked = concat!("if self.close_asked == ", "CloseAsked::Yes {");
        let guard = concat!("let Some(id) = ", "self.window_id else");
        let store = concat!("self.close_asked = ", "CloseAsked::Yes;");
        let dispatch = concat!("dispatch2::DispatchQueue::main()", ".exec_async(");
        for anchor in [asked, guard, store, dispatch] {
            assert_eq!(body.matches(anchor).count(), 1, "once: {anchor}");
        }
        let at = |a: &str| body.find(a).expect("found");
        assert!(at(asked) < at(guard), "asked once");
        assert!(at(guard) < at(store), "marked only with a window");
        assert!(at(store) < at(dispatch), "then dispatched");
        // The opening turn runs before the window is the driver's.
        let show = show(src);
        let turn = show
            .find(concat!("driver.pump(", "Trigger::Opening);"))
            .expect("the turn");
        let given = show
            .find(concat!("driver.window_id = ", "Some(id);"))
            .expect("the window given");
        let woken = show
            .rfind(concat!("view.waker()", ".wake();"))
            .expect("the wake");
        assert!(turn < given && given < woken);
        // And no turn ends before the closed stream's branch, a drain
        // that found nothing included.
        let pump = src
            .split_once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(b, _)| b)
            .expect("the pump");
        let drained = pump
            .find(concat!("let drained = ", "self.app.pty.drain();"))
            .expect("the drain");
        let closed = pump
            .find(concat!("if drained.stream == ", "Stream::Closed {"))
            .expect("the closed stream's branch");
        assert!(drained < closed);
        let before = &pump[drained..closed];
        assert!(
            !before.lines().any(|l| {
                let l = l.trim_start();
                !l.starts_with("//") && (l.starts_with("return") || l.contains(" return"))
            }),
            "no return before the closed stream's branch"
        );
        // The stream stays closed: every drain after the child's end says
        // so, which is what the wake's turn closes on.
        let pty = include_str!("pty.rs");
        assert!(pty.contains(concat!(
            "Err(std::sync::mpsc::TryRecvError::Disconnected) => ",
            "break Stream::Closed,"
        )));
    }
}

/// **A cursor the program's scroll took with its line trails only the
/// rest of its move**, on macOS: the captures of `tests/trail/scroll/`
/// through `frame_trail`, the trail `present` hands the renderer, with its
/// cursor rectangles from the renderer the app draws with, on a 120 Hz
/// display link. The same pins the Linux driver is held to
/// (`carried::captures`), for the default trail, the ghost and every chase.
#[cfg(test)]
mod carried_tests {
    use super::{TrailState, frame_trail};
    use crate::carried::Carried;
    use crate::carried::captures::{ALL, check, replay};
    use crate::config::{CursorTrail, SystemMotion, TrailScroll};
    use crate::trail::Count;
    use baud_domain::{Cols, Rows};
    use baud_render::{Blend, PxSize, Renderer, Stack, Theme};

    /// **Every capture's jumps trail and its scrolls snap on macOS**, at
    /// the Retina scale: the default trail, the ghost -- a sweep across
    /// more than two cells counted as its long move -- and every other
    /// trail that rides the chase. Perturbations: the carry dropped from
    /// `frame_trail`; the ghost's last cell left where it was (the ghost
    /// sweeps helix's wheel steps).
    #[test]
    fn every_captures_scrolls_snap_and_jumps_trail_on_macos() {
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).unwrap()];
        let renderer = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(26.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(80),
            Rows::of_index(24),
        )
        .unwrap();
        let default = crate::config::Config::launch_at(&[])
            .config
            .cursor_trail
            .effective(SystemMotion::Full);
        for kind in [
            default,
            CursorTrail::Ghost,
            CursorTrail::Quad,
            CursorTrail::Sweep,
            CursorTrail::Fire,
            CursorTrail::Lightning,
            CursorTrail::Ice,
            CursorTrail::Ufo,
            CursorTrail::Orb,
            CursorTrail::Liquid,
        ] {
            for (name, raw, steps) in ALL {
                let mut chase = crate::trail::Chase::new(kind.style());
                let (mut trail, mut last): (Option<TrailState>, _) = (None, None);
                let mut carried = Carried::new();
                let (mut sweeps, mut swept) = (Count(0), None);
                let moves = replay((raw, steps), |t, secs| {
                    let _ = frame_trail(
                        (kind, TrailScroll::Ride),
                        t,
                        &renderer,
                        2.0,
                        secs,
                        (&mut chase, &mut trail, &mut last),
                        (&mut carried, Rows::ZERO),
                    );
                    match kind {
                        CursorTrail::Ghost => {
                            if let Some(g) = trail.filter(|g| Some(g.start) != swept) {
                                swept = Some(g.start);
                                let far = |a: usize, b: usize| a.abs_diff(b) > 2;
                                if far(g.from.0.index(), g.to.0.index())
                                    || far(g.from.1.index(), g.to.1.index())
                                {
                                    sweeps.0 += 1;
                                }
                            }
                            sweeps
                        }
                        _ => chase.longs,
                    }
                });
                check(name, &moves, &format!("macOS {kind:?}"));
            }
        }
    }

    /// **A ghost in flight moves with a carried cursor's text on macOS**,
    /// as on Linux (`timed.rs`): a sweep from row 4 to row 8, then the
    /// view redrawn three lines up the file with the cursor on its line
    /// inside the sweep -- the sweep's end moves three rows and its start
    /// as far, and no sweep starts. Perturbation: the ghost in flight left
    /// where it was.
    #[test]
    fn a_ghost_in_flight_moves_with_a_carried_cursor_on_macos() {
        use crate::carried::tests::{Line, view};
        use baud_core::{Owned, Term};
        use baud_domain::{BytePos, Col, ReadLen, Row, WinSize};
        let ws = WinSize {
            ws_col: 40,
            ws_row: 12,
        };
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).unwrap()];
        let renderer = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(26.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(40),
            Rows::of_index(12),
        )
        .unwrap();
        let cell_h = f32::from(u16::try_from(renderer.metrics().cell_h.value()).unwrap());
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        let mut chase = crate::trail::Chase::new(CursorTrail::Ghost.style());
        let (mut trail, mut last): (Option<TrailState>, _) = (None, None);
        let mut carried = Carried::new();
        let mut frame = |t: &Term<'_>, secs: f64| {
            let _ = frame_trail(
                (CursorTrail::Ghost, TrailScroll::Ride),
                t,
                &renderer,
                2.0,
                secs,
                (&mut chase, &mut trail, &mut last),
                (&mut carried, Rows::ZERO),
            );
            trail
        };
        feed(&mut t, b"\x1b[?1049h");
        feed(&mut t, &view(Line(10), Line(13)));
        let _ = frame(&t, 1.0);
        feed(&mut t, &view(Line(10), Line(17)));
        let was = frame(&t, 1.008).expect("a sweep");
        assert_eq!(
            was.to,
            (Row::from_index(7), Col::at_index(0)),
            "the control"
        );
        feed(&mut t, &view(Line(7), Line(17)));
        let now = frame(&t, 1.016).expect("still sweeping");
        assert_eq!(now.start, was.start, "no sweep starts for the carry");
        assert_eq!(
            now.to,
            (Row::from_index(10), Col::at_index(0)),
            "its end moved with the line"
        );
        assert_eq!(now.from, was.from);
        assert!((now.origin_dy - was.origin_dy - 3.0 * cell_h).abs() < 1e-3);
    }
}

#[cfg(test)]
mod strip_lights_tests {
    use super::{InView, in_view_at, strip_lights};
    use baud_render::{ByLed, LineEdge};
    use macos_liquid_glass::objc2_foundation::{NSPoint, NSRect, NSSize};

    /// **The title strip's lights in the view's pixels**: a light's frame
    /// 20 by 28 points at (450, -28) in the flipped view, above its top, is
    /// 40 by 56 pixels at (900, -56) at 2x, the tip opening down; none
    /// shown, none at all; and the pointer over the light has left the
    /// view, a point below its top has not. Perturbation: take the frame's
    /// bottom-left corner, as an unflipped view has it, and RD sits below
    /// the strip.
    #[test]
    fn the_strips_lights_are_above_the_view_in_its_pixels() {
        let rect = |x, y| Some(NSRect::new(NSPoint::new(x, y), NSSize::new(20.0, 28.0)));
        let frames = ByLed {
            rd: rect(450.0, -28.0),
            sd: rect(471.0, -28.0),
            ..ByLed::default()
        };
        let got = strip_lights(frames, 2.0, (0.0, 20.0), (16.0, 34.0), 1290.0).expect("lights");
        assert_eq!(got.edge, LineEdge::Top);
        let rd = got.places.rd.expect("RD");
        assert_eq!((rd.x, rd.y, rd.w, rd.h), (900.0, -56.0, 40.0, 56.0));
        assert_eq!(got.light_at((910.0, -1.0)), Some(baud_render::Led::Rd));
        assert_eq!(got.light_at((910.0, 1.0)), None, "the view's first row");
        assert!(got.places.hs.is_none());
        assert!(strip_lights(ByLed::default(), 2.0, (0.0, 0.0), (16.0, 34.0), 1.0).is_none());
        // Where the pointer is when a crossing is taken: over the light,
        // above the view, it has left the view; just under it, in it.
        let view = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 400.0));
        assert_eq!(in_view_at(view, NSPoint::new(455.0, -1.0)), InView::Left);
        assert_eq!(in_view_at(view, NSPoint::new(455.0, 0.5)), InView::Yes);
    }
}

/// **The launch logo never moves the frame of a read that lands on a
/// tick, and moves a read between ticks by one vsync at most**, through
/// the pump's own frame gate (`frame_gate`): a prompt of 469
/// bytes in one read -- a starship prompt's shape, four rows, OSC 133
/// marks and an OSC 7 naming a neutral user and host -- replayed at 120 Hz
/// on a virtual clock in the loop's order: each tick, the logo's owed
/// frame (`frame`) and then the pump's gate; each read, a wake's turn; each
/// frame's completion, 2 ms after it was drawn, a wake's turn too. The
/// ticks jitter, as the display link's do. A frame drawn at `t` reaches
/// the screen at the first vsync after its completion. The arms: the logo
/// off, waiting for a window it never sees, and playing from tick 0, 5, 30
/// and 60.
///
/// * **A read that lands on a tick** -- with the logo's frames, one is
///   drawn on every tick -- is on the screen at the same vsync in every
///   arm.
/// * **A read that lands between ticks** is drawn no later than the next
///   tick in every arm. Not at the same vsync: with no frame since the
///   last tick a wake draws at once, and while the logo draws a frame on
///   every tick the wake finds one younger than the interval and leaves
///   the read to the next tick -- one vsync later, the flood rule every
///   timed effect shares (a trail, a transition, the scrollbar's fade).
#[cfg(test)]
mod gate_tests {
    use super::{Decided, Echo, FrameGate, Guess, Marked, Pending, Trigger, frame_gate};
    use crate::bell::Update;
    use crate::config::{LaunchSplash, SystemMotion};
    use crate::splash::{Owes, Splash};
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen, Row, WinSize};
    use baud_present_macos::InFlight;
    use std::time::Duration;

    const WS: WinSize = WinSize {
        ws_col: 80,
        ws_row: 24,
    };
    /// 120 Hz, in microseconds.
    const TICK: u64 = 8_333;
    /// From a frame drawn to its completion.
    const GPU: u64 = 2_000;
    /// How far each tick lands from its vsync, in turn.
    const JITTER: [i64; 4] = [0, 600, -400, 200];

    /// When tick `n` fires.
    fn tick_at(n: Tick) -> Duration {
        let jitter = JITTER[usize::try_from(n.0 % 4).expect("an index")];
        Duration::from_micros((n.0 * TICK).saturating_add_signed(jitter))
    }

    /// A tick's number.
    #[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
    struct Tick(u64);

    /// The prompt: 469 bytes in one read.
    fn prompt() -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(b"\x1b]133;A\x07\x1b]7;file://host/home/user\x07");
        p.extend_from_slice(
            b"\r\n\x1b[1;36mhome/user/code/project\x1b[0m on \x1b[1;35m branch main\x1b[0m ",
        );
        p.extend_from_slice(b"\x1b[1;31m[!?]\x1b[0m is \x1b[1;38;5;208m v1.0.0\x1b[0m via \x1b[1;31m rust 1.96\x1b[0m\r\n");
        p.extend_from_slice(b"\x1b[33mtook 2s\x1b[0m \x1b[2mat 12:00:00\x1b[0m\r\n");
        p.extend_from_slice(b"\x1b[1;32muser@host\x1b[0m ");
        let tail = b"\x1b[1;32m\xe2\x9d\xaf\x1b[0m \x1b]133;B\x07";
        while p.len() + tail.len() < 469 {
            p.push(b' ');
        }
        p.extend_from_slice(tail);
        p
    }

    fn shows_prompt(t: &Term<'_>) -> bool {
        let view = t.view(t.window_back());
        (0..view.rows().index()).any(|y| {
            let row: String = view
                .row(Row::from_index(y))
                .iter()
                .filter_map(|c| c.content.as_codepoint())
                .filter_map(|p| char::from_u32(p.value()))
                .collect();
            row.contains("user@host")
        })
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Arm {
        Off,
        Waiting,
        PlayingFrom(Tick),
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
    enum Event {
        /// Before a tick at the same time, so a read on a tick is fed
        /// before the tick's turn.
        Read,
        Done,
        Tick(Tick),
    }

    /// The vsync on which the prompt is first on screen, and the time its
    /// frame was drawn in microseconds, for a read landing at `read`.
    fn replay(arm: Arm, read: Duration, gate: fn(FrameGate) -> Decided) -> (u64, u64) {
        let us = |d: Duration| u64::try_from(d.as_micros()).expect("a time");
        let read = us(read);
        let mut owned = Owned::new(&WS);
        let mut term = Term::new(owned.storage(), &WS);
        let mut splash = Splash::new(match arm {
            Arm::Off => LaunchSplash::Off,
            _ => LaunchSplash::On,
        });
        let mut events: Vec<(u64, Event)> = (0..=120_u64)
            .map(|n| (us(tick_at(Tick(n))), Event::Tick(Tick(n))))
            .collect();
        events.push((read, Event::Read));
        let mut pending = Pending::Clean;
        let mut last_commit = 0_u64;
        let mut busy = InFlight::Idle;
        let bytes = prompt();
        assert_eq!(bytes.len(), 469);
        loop {
            events.sort();
            if events.is_empty() {
                panic!("{arm:?}: the prompt was never drawn");
            }
            let (now, event) = events.remove(0);
            let trigger = match event {
                Event::Tick(n) => {
                    if arm == Arm::PlayingFrom(n) && splash.shown() == Owes::Frame {
                        pending = Pending::Dirty;
                    }
                    if splash.owes(Duration::from_micros(now).as_secs_f64()) == Owes::Frame {
                        pending = Pending::Dirty;
                    }
                    Trigger::Tick
                }
                Event::Read => {
                    term.write_at(BytePos::ZERO, &bytes);
                    term.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
                    pending = Pending::Dirty;
                    Trigger::Wake
                }
                Event::Done => {
                    busy = InFlight::Idle;
                    Trigger::Wake
                }
            };
            let frame = gate(FrameGate {
                pending,
                update: (pending == Pending::Dirty).then_some(Update::Closed),
                trigger,
                echo: Echo::Drawn,
                guess: Guess::None,
                marked: Marked::Drawn,
                since_commit: Duration::from_micros(now - last_commit),
                interval: Duration::from_micros(TICK),
                flight: busy,
            });
            if frame == Decided::Now {
                let at = Duration::from_micros(now).as_secs_f64();
                let _ = splash.look(Some(at), &term, SystemMotion::Full);
                pending = Pending::Clean;
                last_commit = now;
                busy = InFlight::Busy;
                events.push((now + GPU, Event::Done));
                if shows_prompt(&term) {
                    return ((now + GPU).div_ceil(TICK), now);
                }
            }
        }
    }

    const ARMS: [Arm; 6] = [
        Arm::Off,
        Arm::Waiting,
        Arm::PlayingFrom(Tick(0)),
        Arm::PlayingFrom(Tick(5)),
        Arm::PlayingFrom(Tick(30)),
        Arm::PlayingFrom(Tick(60)),
    ];

    /// The prompt lands early, mid-way and late in the logo, on a tick
    /// and between ticks.
    fn check(gate: fn(FrameGate) -> Decided) -> Result<(), String> {
        for n in [3_u64, 10, 40, 70] {
            let tick = tick_at(Tick(n));
            let on: Vec<u64> = ARMS.iter().map(|&a| replay(a, tick, gate).0).collect();
            if on.iter().any(|&v| v != on[0]) {
                return Err(format!("a read on tick {n}: vsyncs {on:?} across {ARMS:?}"));
            }
            let between = tick + Duration::from_micros(TICK / 2);
            let next = tick_at(Tick(n + 1));
            for a in ARMS {
                let drawn = Duration::from_micros(replay(a, between, gate).1);
                if drawn > next {
                    return Err(format!(
                        "{a:?}: a read between ticks {n} and {} drawn at {drawn:?}, after the tick at {next:?}",
                        n + 1
                    ));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn a_read_on_a_tick_is_on_screen_at_the_same_vsync_under_the_logo() {
        assert_eq!(check(frame_gate), Ok(()));
        // The control: the probe sees the logo's frames, which leave a
        // read between ticks to the next tick, where the logo off draws
        // it at once.
        let between = 40 * TICK + TICK / 2;
        let at = Duration::from_micros(between);
        let off = replay(Arm::Off, at, frame_gate);
        let on = replay(Arm::PlayingFrom(Tick(30)), at, frame_gate);
        assert_eq!(off.1, between, "the logo off: drawn on the read's wake");
        assert!(on.1 > between, "the logo on: left to the tick");
        assert_eq!(on.0, off.0 + 1, "one vsync later");
    }

    /// **Perturbation, compiled**: a gate whose tick is due only once a
    /// frame's time has passed since the last commit -- the tick's own
    /// rule dropped -- leaves a read past the next tick when the display
    /// link fires early, and the check says so.
    #[test]
    fn a_gate_without_the_ticks_rule_fails_the_check() {
        fn no_tick_rule(g: FrameGate) -> Decided {
            match (g.trigger, g.since_commit < g.interval) {
                (Trigger::Tick, true) if g.pending == Pending::Dirty => Decided::Later,
                _ => frame_gate(g),
            }
        }
        assert!(check(no_tick_rule).is_err());
    }

    /// **The pump decides by this gate**, and by no copy of it: its body
    /// hands `frame_gate` a `FrameGate` and draws only on `Decided::Now`. Every
    /// anchor is asserted present first.
    #[test]
    fn the_pump_calls_the_gate() {
        let src = include_str!("app.rs");
        let pump = src
            .split_once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"))
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(b, _)| b)
            .expect("the pump");
        let call = concat!("let frame = frame_gate(", "FrameGate {");
        let now = concat!(
            "Decided::Now => {\n",
            "                let view = self.view.clone();"
        );
        let present = concat!("self.app.present(&view, ", "Show::Event);");
        for anchor in [call, now, present] {
            assert_eq!(
                pump.matches(anchor).count(),
                1,
                "once in the pump: {anchor}"
            );
        }
        assert!(pump.find(call) < pump.find(now) && pump.find(now) < pump.find(present));
        assert_eq!(
            src.matches(concat!("fn frame_gate(", "g: FrameGate)"))
                .count(),
            1
        );
        assert!(!pump.contains("let due = "), "no second gate in the pump");
    }
}
